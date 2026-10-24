//! Scenario 1 — stale or duplicate session messages.
//!
//! Invariant under test: a replay, duplicate or out-of-order frame can never
//! re-enter the committed input set. The relay model's `seq` rule drops
//! duplicates before they reach a seat; the production lockstep validator
//! rejects anything scheduled below a watermark the peer already committed,
//! and malformed frames never decode.

use super::{
    EvidenceStatus, InputSpec, Scenario, ScenarioResult, ScenarioSpec, TopologyMode, finalize,
};
use crate::agent::{EvilWeevil, IntentKind};
use crate::harness::{IsolatedHarness, ValidatorOutcome};
use crate::policy::{Permission, Principal};
use crate::relay_model::RelayModel;
use crate::telemetry::ViolationKind;
use crate::wire;
use idaptik_net::envelope::Role;
use idaptik_net::lockstep::LockstepCore;

const ID: &str = "EW-01-stale-duplicate-messages";

/// The fixed, bounded input sequence. Every frame is pre-declared; nothing
/// is discovered or generated at runtime.
fn inputs() -> Vec<InputSpec> {
    vec![
        InputSpec { step: 0, intent: "commit watermark through tick 5".into(), expected: "accepted".into() },
        InputSpec { step: 1, intent: "NetSsh at tick 4 (below watermark), fresh seq".into(), expected: "lockstep rejection: stale after commit".into() },
        InputSpec { step: 2, intent: "same envelope replayed with the same seq".into(), expected: "relay drop: stale_or_duplicate".into() },
        InputSpec { step: 3, intent: "frame with a lower seq than the last".into(), expected: "relay drop: stale_or_duplicate".into() },
        InputSpec { step: 4, intent: "NetSsh at tick 6 (at the frontier), new seq".into(), expected: "accepted".into() },
        InputSpec { step: 5, intent: "command frame missing its at schedule".into(), expected: "decoder rejection".into() },
        InputSpec { step: 6, intent: "unknown command tag WarpToRoom".into(), expected: "decoder rejection: unknown tag".into() },
    ]
}

/// Scenario 1's corpus entry.
pub struct StaleDuplicateScenario;

impl Scenario for StaleDuplicateScenario {
    fn spec(&self) -> ScenarioSpec {
        ScenarioSpec {
            id: ID,
            title: "Stale or duplicate session messages",
            principal: Principal::adversary(Role::Hacker, 1).identity.label,
            permitted_operations: vec![Permission::ProposeBoundedIntent],
            invariant: "No frame at or below a committed watermark can re-enter the lockstep input set; duplicate seq envelopes are dropped, not relayed.",
            bounded_inputs: inputs(),
            expectation: "Relay drops the duplicate/out-of-order envelopes; the production lockstep validator rejects the stale tick and the malformed frames; the valid frontier frame is accepted; sim state is untouched by refusals.",
            topology: TopologyMode::SingleRendezvousRelay,
            game_server_admin_present: false,
            evidence: EvidenceStatus::Demonstrated,
            replay_artifact: "result.json",
        }
    }

    fn run(&self) -> ScenarioResult {
        let spec = self.spec();
        let mut harness = IsolatedHarness::new(ID, 777_001);
        let script = crate::harness::load_live_script();
        let mut target =
            LockstepCore::new(Role::Infiltrator, 1, &script).expect("production core builds");
        let mut relay = RelayModel::join("idaptik", Role::Hacker).expect("model relay joins");
        let proposer = Principal::adversary(Role::Hacker, 1);
        let mut weevil = EvilWeevil::new(proposer.clone());

        // Permission separation is load-bearing: the agent cannot reach the
        // validator or the policy file directly. Evaluate before the mutable
        // borrow of `harness.check`, so the two borrows never overlap.
        let adversary_propose_only =
            harness.authorise(&proposer, Permission::ProposeBoundedIntent)
                && !harness.authorise(&proposer, Permission::FeedProductionValidator)
                && !harness.authorise(&proposer, Permission::ConfigureEnforcementPolicy);
        harness.check(
            "permission:adversary-propose-only",
            adversary_propose_only,
            "adversary holds only ProposeBoundedIntent",
        );
        let harness_cannot_propose =
            !harness.authorise(&Principal::harness(), Permission::ProposeBoundedIntent);
        harness.check(
            "permission:harness-cannot-propose",
            harness_cannot_propose,
            "the delivery identity cannot author adversarial intents",
        );

        // 1. Move the production watermark to tick 6 (peer_next = through+1).
        harness
            .feed_commit(&mut target, Role::Hacker, 5)
            .expect("watermark commit accepted");

        // 2. A stale frame below the watermark: the relay relays it (fresh
        //    seq), the production validator rejects it.
        let stale = weevil
            .propose(
                IntentKind::StaleReplay,
                wire::with_seq(wire::net_ssh(4, 0), 1),
                "replay a tick-4 NetSsh after the peer committed through 5",
            )
            .unwrap();
        harness.record_intent(&stale);
        let stale_outcome =
            harness.deliver_command(&mut relay, &mut target, &proposer, stale.wire.clone());
        harness.check(
            "stale-frame-rejected-by-lockstep",
            matches!(stale_outcome, ValidatorOutcome::Rejected(_)),
            "on_peer_command refuses a tick below the watermark",
        );

        // 3. The exact same envelope again: duplicate seq, dropped at the relay.
        let dup = weevil
            .propose(
                IntentKind::DuplicateEnvelope,
                wire::with_seq(wire::net_ssh(4, 0), 1),
                "duplicate the seq=1 envelope verbatim",
            )
            .unwrap();
        harness.record_intent(&dup);
        let dup_outcome =
            harness.deliver_command(&mut relay, &mut target, &proposer, dup.wire.clone());
        harness.check(
            "duplicate-envelope-dropped",
            matches!(dup_outcome, ValidatorOutcome::Rejected(_))
                && harness
                    .telemetry()
                    .iter()
                    .any(|t| matches!(t, crate::telemetry::TelemetryEvent::RelayDropped { .. })),
            "the relay acknowledges-and-drops the duplicate seq",
        );

        // 4. A lower seq than the last: also stale at the relay.
        let low = weevil
            .propose(
                IntentKind::DuplicateEnvelope,
                wire::with_seq(wire::net_ssh(5, 0), 0),
                "out-of-order seq below the high-water mark",
            )
            .unwrap();
        harness.record_intent(&low);
        let low_outcome =
            harness.deliver_command(&mut relay, &mut target, &proposer, low.wire.clone());
        harness.check(
            "lower-seq-dropped",
            matches!(low_outcome, ValidatorOutcome::Rejected(_)),
            "seq regression is a relay drop, never a relay",
        );

        // 5. A valid frame exactly at the frontier: accepted by the host.
        let fresh = weevil
            .propose(
                IntentKind::InvalidTargetClaim,
                wire::with_seq(wire::net_ssh(6, 0), 2),
                "in-range tick at the watermark frontier",
            )
            .unwrap();
        harness.record_intent(&fresh);
        let fresh_outcome =
            harness.deliver_command(&mut relay, &mut target, &proposer, fresh.wire.clone());
        harness.check(
            "frontier-frame-accepted",
            matches!(fresh_outcome, ValidatorOutcome::Accepted),
            "a fresh, in-range frame still works — refusals are scoped, not blanket",
        );

        // 6. A frame missing its schedule: the decoder refuses to guess.
        let no_at = weevil
            .propose(
                IntentKind::MalformedPayload,
                wire::command_without_at(serde_json::json!({ "cmd": "NetSsh", "node": 0 })),
                "strip the mandatory at key",
            )
            .unwrap();
        harness.record_intent(&no_at);
        let no_at_outcome = harness.feed_raw_to_validator(&mut target, &no_at.wire);
        harness.check(
            "missing-at-rejected",
            matches!(no_at_outcome, ValidatorOutcome::Undecodable(_)),
            "decode_command requires an explicit schedule",
        );

        // 7. An unknown command tag: the typed protocol has no such verb.
        let unknown = weevil
            .propose(
                IntentKind::MalformedPayload,
                wire::unknown_command(7, "WarpToRoom"),
                "forge a command the enum does not carry",
            )
            .unwrap();
        harness.record_intent(&unknown);
        let unknown_outcome = harness.feed_raw_to_validator(&mut target, &unknown.wire);
        let unknown_tag_evidence = matches!(unknown_outcome, ValidatorOutcome::Undecodable(_))
            && harness.telemetry().iter().any(|t| matches!(
                t,
                crate::telemetry::TelemetryEvent::ValidatorRejected {
                    violation: ViolationKind::UnknownCommandTag,
                    ..
                }
            ));
        harness.check(
            "unknown-tag-rejected",
            unknown_tag_evidence,
            "unknown command tags cannot decode into a Command",
        );

        // Refusals must leave the sim untouched: the core has executed no
        // steps (startup events are drained at construction, so only the
        // step counter can distinguish an untouched sim here).
        harness.check(
            "no-execution-from-refused-frames",
            target.step() == 0,
            "rejected frames never advance the simulation",
        );
        harness.check(
            "relay-accounting",
            relay.relayed_count == 2 && relay.dropped_count == 2 && relay.rejected_count == 0,
            format!(
                "relayed={}, dropped={}, rejected={}",
                relay.relayed_count, relay.dropped_count, relay.rejected_count
            ),
        );

        weevil.retire();
        let notes = vec![
            "Real-burble confirmation of the seq rule runs in scripts/evil_weevil_relay_check.sh against a throwaway local relay.".into(),
            "A duplicate with a fresh seq is an in-protocol second command; relay-side de-duplication keys on seq, which this scenario exercises directly.".into(),
            "The replay artefact records the exact raw payloads, including the malformed ones.".into(),
        ];
        finalize(spec, &mut harness, notes)
    }
}

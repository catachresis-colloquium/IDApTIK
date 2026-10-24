//! Scenario 5 — delayed or withheld observations, distinguishing faults
//! from hostility.
//!
//! The in-memory wire can inject two transport pathologies deterministically:
//! bounded delivery delay (frames arrive late, nothing is lost) and
//! withholding (a completeness watermark is held past its logical deadline).
//! Both are *faults*: properties of a pipe, with no intent evidence attached.
//! The scenario proves (a) delay changes pacing only — the run still matches
//! the headless reference byte-for-byte — (b) withholding trips the same
//! `blocked_on_peer` loss predicate the real driver uses, and recovery still
//! reconstructs the reference, and (c) the observer never calls a fault
//! hostile, while a genuine protocol violation is flagged for review and the
//! enforcement policy can only act locally.

use super::{
    EvidenceStatus, InputSpec, Scenario, ScenarioResult, ScenarioSpec, TopologyMode, finalize,
};
use crate::agent::{EvilWeevil, IntentKind};
use crate::enforce::{EnforcementAction, EnforcementMode, EnforcementPolicy};
use crate::harness::{IsolatedHarness, SeatPair, ValidatorOutcome, WireFrame, load_live_script};
use crate::observer::{DefensiveObserver, EvidenceClass, Recommendation};
use crate::policy::{Permission, Principal};
use crate::telemetry::FaultKind;
use crate::wire;
use idaptik_core::scenario::command::Command;
use idaptik_core::scenario::tuning::ActionKind;
use idaptik_net::envelope::Role;
use idaptik_net::lockstep::LockstepCore;
use idaptik_tui::headless;
use std::collections::VecDeque;

const ID: &str = "EW-05-delayed-withheld-observations";
const DELAY_ROUNDS: u64 = 14;
const WITHHOLD_ROUNDS: u64 = 4;

fn inputs() -> Vec<InputSpec> {
    vec![
        InputSpec {
            step: DELAY_ROUNDS,
            intent: "seeded jitter: 1-3 deliveries per in-memory turn".into(),
            expected: "pacing stalls only; reference run reproduced".into(),
        },
        InputSpec {
            step: WITHHOLD_ROUNDS,
            intent: "withhold every frame for four turns".into(),
            expected: "blocked_on_peer loss predicate trips; faults recorded".into(),
        },
        InputSpec {
            step: 60,
            intent: "release the held queue and finish".into(),
            expected: "recovered run byte-identical to headless".into(),
        },
        InputSpec {
            step: 1,
            intent: "contrast: one genuine stale-after-commit frame".into(),
            expected: "violation flagged for operator review".into(),
        },
        InputSpec {
            step: 1,
            intent: "observe-only and local-protective enforcement".into(),
            expected: "no remote action exists; local actions are operator-gated".into(),
        },
    ]
}

pub struct WithheldObservationScenario;

impl Scenario for WithheldObservationScenario {
    fn spec(&self) -> ScenarioSpec {
        ScenarioSpec {
            id: ID,
            title: "Delayed or withheld observations: faults versus hostility",
            principal: Principal::adversary(Role::Infiltrator, 5).identity.label,
            permitted_operations: vec![Permission::ProposeBoundedIntent],
            invariant: "Transport pathologies are classified as faults, never hostility; recovery preserves the deterministic run; enforcement is local and operator-gated.",
            bounded_inputs: inputs(),
            expectation: "Delayed and withheld runs reproduce the reference; the observer reports zero false positives on faults, flags the real violation, and never recommends retaliation.",
            topology: TopologyMode::SingleRendezvousRelay,
            game_server_admin_present: false,
            evidence: EvidenceStatus::Demonstrated,
            replay_artifact: "result.json",
        }
    }

    fn run(&self) -> ScenarioResult {
        let spec = self.spec();
        let mut harness = IsolatedHarness::new(ID, 777_005);
        let script = load_live_script();
        let proposer = Principal::adversary(Role::Infiltrator, 5);
        let mut weevil = EvilWeevil::new(proposer.clone());

        // -- Phase A: bounded delay, nothing lost ----------------------------
        let mut delayed = SeatPair::new(3, 3);
        let mut wire_a: VecDeque<WireFrame> = VecDeque::new();
        for _ in 0..DELAY_ROUNDS {
            let jitter = 1 + (harness.rng_mut().next_u32() % 3) as usize;
            harness.exchange_round(&mut delayed, &mut wire_a, jitter);
            harness.record_fault(
                FaultKind::DelayedDelivery,
                format!("{jitter} frame(s) delivered this turn; remainder queued"),
            );
        }
        // Flush and finish under the same bounded accounting.
        let mut guard = 0;
        while !(delayed.infiltrator.finished() && delayed.hacker.finished()) {
            guard += 1;
            assert!(guard <= 200, "delayed run did not converge");
            harness.exchange_round(&mut delayed, &mut wire_a, 16);
        }
        let (_, reference_log) = headless::simulate(&script).expect("headless reference");
        let reference = serde_json::to_value(&reference_log).expect("reference serialises");
        harness.check(
            "delay-is-pacing-only",
            serde_json::to_value(delayed.infiltrator.log()).unwrap() == reference
                && serde_json::to_value(delayed.hacker.log()).unwrap() == reference,
            "late delivery changes no event, only timing",
        );

        // -- Phase B: withheld watermarks trip the loss predicate -----------
        let mut withheld = SeatPair::new(3, 3);
        let mut held: VecDeque<WireFrame> = VecDeque::new();
        for _ in 0..WITHHOLD_ROUNDS {
            let oa = withheld.infiltrator.pump_outgoing(&mut withheld.inf_feed);
            let ob = withheld.hacker.pump_outgoing(&mut withheld.hac_feed);
            for o in oa {
                held.push_back(WireFrame::from_outgoing(Role::Infiltrator, o));
            }
            for o in ob {
                held.push_back(WireFrame::from_outgoing(Role::Hacker, o));
            }
            harness.metrics_peak(held.len());
            withheld.infiltrator.advance_with(|_, _| {});
            withheld.hacker.advance_with(|_, _| {});
            harness.bump_step();
            harness.record_fault(FaultKind::WithheldWatermark, "commit held off the wire");
        }
        let blocked = withheld.infiltrator.blocked_on_peer() || withheld.hacker.blocked_on_peer();
        harness.check(
            "withholding-trips-loss-predicate",
            blocked,
            "a missing watermark is observable as blocked_on_peer, exactly as on the real socket",
        );
        harness.record_fault(
            FaultKind::SilenceGraceExpired,
            "logical grace window elapsed with no release",
        );

        // Release: ordinary rounds drain the held queue, then finish.
        let mut guard = 0;
        while !(withheld.infiltrator.finished() && withheld.hacker.finished()) {
            guard += 1;
            assert!(guard <= 200, "withheld run did not converge after release");
            harness.exchange_round(&mut withheld, &mut held, 16);
        }
        harness.check(
            "withheld-run-recovers-to-reference",
            serde_json::to_value(withheld.infiltrator.log()).unwrap() == reference
                && serde_json::to_value(withheld.hacker.log()).unwrap() == reference,
            "release plus lockstep reconstructs the same run",
        );
        harness.record_recovery("watermark release and catch-up");

        // -- Phase C: a real violation, for the contrast ---------------------
        let mut core = LockstepCore::new(Role::Infiltrator, 1, &script).expect("core builds");
        harness
            .feed_commit(&mut core, Role::Hacker, 3)
            .expect("watermark accepted");
        let hostile = weevil
            .propose(
                IntentKind::StaleReplay,
                wire::typed_command(
                    1,
                    &Command::Uplink {
                        kind: ActionKind::Camera,
                    },
                ),
                "a frame below the committed watermark",
            )
            .unwrap();
        harness.record_intent(&hostile);
        let outcome = harness.feed_raw_to_validator(&mut core, &hostile.wire);
        harness.check(
            "contrast-violation-rejected",
            matches!(outcome, ValidatorOutcome::Rejected(_)),
            "the host rejects the stale frame",
        );

        // -- Observer: zero false positives on the fault traces --------------
        let report = DefensiveObserver::evaluate(harness.telemetry());
        let fault_set: std::collections::BTreeSet<usize> = report.faults.iter().copied().collect();
        let flagged_a_fault = report.flagged.iter().any(|i| fault_set.contains(i));
        harness.check(
            "faults-never-flagged-hostile",
            !flagged_a_fault && report.counts.fault > 0 && report.counts.violation > 0,
            format!(
                "faults={}, violations={}, flagged={}",
                report.counts.fault,
                report.counts.violation,
                report.flagged.len()
            ),
        );
        let violation_recommendation = report
            .classifications
            .iter()
            .find(|c| c.class == EvidenceClass::Violation)
            .map(|c| c.recommendation)
            .expect("the contrast violation is classified");
        harness.check(
            "strongest-recommendation-is-review",
            violation_recommendation == Recommendation::FlagForOperatorReview,
            "the observer cannot punish, only ask a human",
        );

        // -- Enforcement boundary: local, explicit, operator-gated ----------
        let observe_only = EnforcementPolicy::observe_only();
        harness.check(
            "observe-only-never-acts",
            observe_only.mode() == EnforcementMode::ObserveOnly
                && observe_only.decide(violation_recommendation) == EnforcementAction::LogOnly
                && observe_only.decide(Recommendation::RequestResync) == EnforcementAction::LogOnly,
            "the default policy records and does nothing",
        );
        let operator = Principal::operator();
        let protective = EnforcementPolicy::local_protective(&operator)
            .expect("operators may select local protection");
        // Exhaustive match: a new, possibly-remote action variant would fail
        // to compile here, the compile-time guarantee the task asks for.
        let every_action_is_local = match protective.decide(Recommendation::FlagForOperatorReview) {
            EnforcementAction::None
            | EnforcementAction::LogOnly
            | EnforcementAction::RequestLocalResync
            | EnforcementAction::EndLocalSession
            | EnforcementAction::FlagForOperatorReview => true,
        };
        harness.check(
            "enforcement-alphabet-is-local-only",
            every_action_is_local,
            "no ban, blocklist or remote retaliation variant exists",
        );
        for usurper in [
            Principal::harness(),
            Principal::observer(),
            proposer.clone(),
        ] {
            harness.check(
                &format!("non-operator-cannot-configure:{:?}", usurper.kind),
                EnforcementPolicy::local_protective(&usurper).is_err(),
                "only the HostOperator holds enforcement authority",
            );
        }
        harness.check(
            "operator-may-end-only-the-local-session",
            protective.operator_end_local_session(&operator)
                == Ok(EnforcementAction::EndLocalSession),
            "termination is explicit, local, and operator-only",
        );

        weevil.retire();
        let notes = vec![
            "Packet loss, timeout and divergence are never treated as intent: the observer maps every FaultKind to benign/fault classes by construction.".into(),
            "No remote machine is contacted in this crate; the transport is an in-memory VecDeque, so retaliation is not merely unauthorised, it is unaddressable.".into(),
            "False-positive and false-negative rates are measured in every run; finalize() fails the scenario on any fault misclassified as hostile.".into(),
        ];
        finalize(spec, &mut harness, notes)
    }
}

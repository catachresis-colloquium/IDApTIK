//! Scenario 4 — disconnect/reconnect and obsolete messages from the
//! previous session.
//!
//! This re-drives the production loss/recovery machinery
//! ([`LockstepCore::on_peer_lost`], [`LockstepCore::make_resync`],
//! [`LockstepCore::adopt_resync`]) from the adversarial angle: the dying
//! seat leaves an orphan command whose commit never arrived, the survivor
//! must make it un-happen, and any frame the replaced session then offers
//! below the new watermark is refused. The recovered run is byte-compared
//! against the headless reference, so recovery is demonstrated, not claimed.

use super::{
    EvidenceStatus, InputSpec, Scenario, ScenarioResult, ScenarioSpec, TopologyMode, finalize,
};
use crate::agent::{EvilWeevil, IntentKind};
use crate::harness::{IsolatedHarness, SeatPair, ValidatorOutcome, load_live_script};
use crate::policy::{Permission, Principal};
use crate::wire;
use idaptik_core::scenario::command::Command;
use idaptik_core::scenario::tuning::ActionKind;
use idaptik_net::envelope::{Hello, Role};
use idaptik_net::lockstep::{LockstepCore, ScriptFeed};
use idaptik_tui::headless;
use std::collections::VecDeque;

const ID: &str = "EW-04-reconnect-obsolete-messages";
const DIE_AT_STEP: u64 = 8;

fn inputs() -> Vec<InputSpec> {
    vec![
        InputSpec { step: DIE_AT_STEP, intent: "exchange frames until the survivor reaches step 8".into(), expected: "both seats in lockstep".into() },
        InputSpec { step: DIE_AT_STEP, intent: "dying hacker's orphan command arrives without its commit".into(), expected: "buffered, then pruned by on_peer_lost".into() },
        InputSpec { step: DIE_AT_STEP, intent: "survivor builds the net:resync hand-off".into(), expected: "your_pending carries no orphan".into() },
        InputSpec { step: DIE_AT_STEP + 1, intent: "fresh hacker adopts the resync payload".into(), expected: "adopt_resync accepted; run resumes".into() },
        InputSpec { step: DIE_AT_STEP + 1, intent: "obsolete command below the survivor's watermark".into(), expected: "lockstep rejection: stale after commit".into() },
        InputSpec { step: DIE_AT_STEP + 1, intent: "old-session hello on a mismatched seed".into(), expected: "epoch incompatibility".into() },
        InputSpec { step: 60, intent: "drive the recovered run to completion".into(), expected: "both event logs byte-identical to the headless reference".into() },
    ]
}

pub struct ReconnectObsoleteScenario;

impl Scenario for ReconnectObsoleteScenario {
    fn spec(&self) -> ScenarioSpec {
        ScenarioSpec {
            id: ID,
            title: "Disconnect/reconnect and obsolete prior-session messages",
            principal: Principal::adversary(Role::Hacker, 4).identity.label,
            permitted_operations: vec![Permission::ProposeBoundedIntent],
            invariant: "Uncommitted commands never happened; a replaced session cannot inject frames below the committed watermark, and resync reconstructs the reference run exactly.",
            bounded_inputs: inputs(),
            expectation: "Orphan pruned, resync adopted, obsolete frame rejected, mismatched old hello refused, recovered run byte-identical to headless.",
            topology: TopologyMode::SingleRendezvousRelay,
            game_server_admin_present: false,
            evidence: EvidenceStatus::Demonstrated,
            replay_artifact: "result.json",
        }
    }

    fn run(&self) -> ScenarioResult {
        let spec = self.spec();
        let mut harness = IsolatedHarness::new(ID, 777_004);
        let script = load_live_script();
        let proposer = Principal::adversary(Role::Hacker, 4);
        let mut weevil = EvilWeevil::new(proposer.clone());
        let mut pair = SeatPair::new(3, 3);
        let mut queue = VecDeque::new();

        // 1. Lockstep up to the death point.
        let mut guard = 0;
        while pair.infiltrator.step() < DIE_AT_STEP {
            guard += 1;
            assert!(guard <= 100, "death point never reached within the bounded run");
            harness.exchange_round(&mut pair, &mut queue, 8);
        }

        // 2. The dying hacker's last breath: an orphan command with no commit.
        let frontier = pair.infiltrator.peer_watermark();
        assert!(frontier < pair.infiltrator.max_steps());
        pair.infiltrator
            .on_peer_command(
                frontier,
                Command::Uplink {
                    kind: ActionKind::Camera,
                },
            )
            .expect("the orphan buffers at the watermark frontier");

        // 3. Loss: the survivor prunes everything uncommitted.
        pair.infiltrator.on_peer_lost();
        let resync = pair.infiltrator.make_resync();
        harness.check(
            "orphan-pruned-on-loss",
            resync.your_pending.is_empty(),
            "an uncommitted orphan never travels inside net:resync",
        );
        harness.check(
            "resync-carries-committed-history",
            resync.event_log.len() > 1 && resync.snapshot.tick <= DIE_AT_STEP,
            format!(
                "resync ships {} events and a step-{} snapshot",
                resync.event_log.len(),
                resync.snapshot.tick
            ),
        );

        // 4. A fresh hacker process adopts the hand-off.
        let rejoined = LockstepCore::adopt_resync(Role::Hacker, 3, resync.clone());
        harness.check(
            "resync-adopted-by-fresh-seat",
            rejoined.is_ok(),
            rejoined
                .as_ref()
                .map(|_| "fresh hacker reconstructed mid-run".to_owned())
                .unwrap_or_else(|e| e.to_string()),
        );
        pair.hacker = rejoined.expect("resync adoption");
        pair.hac_feed = ScriptFeed::new(&script, Role::Hacker);
        harness.record_recovery("net:resync snapshot hand-off");

        // 5. An obsolete frame from the prior session, below the watermark.
        let stale_at = pair.infiltrator.peer_watermark().saturating_sub(1);
        let obsolete = weevil
            .propose(
                IntentKind::ObsoleteFromPriorSession,
                wire::typed_command(
                    stale_at,
                    &Command::Uplink {
                        kind: ActionKind::Camera,
                    },
                ),
                "replay the dead session's command below the committed frontier",
            )
            .unwrap();
        harness.record_intent(&obsolete);
        let outcome = harness.feed_raw_to_validator(&mut pair.infiltrator, &obsolete.wire);
        harness.check(
            "obsolete-prior-session-frame-rejected",
            matches!(outcome, ValidatorOutcome::Rejected(_)),
            "the production validator refuses frames below its watermark after resync",
        );

        // 6. A hello from the old session epoch cannot redefine the run.
        let old_hello_value = wire::raw_hello(
            Role::Hacker,
            &script,
            0,
            None,
            Some(script.seed.wrapping_add(1)),
            None,
            None,
            Some(false),
        );
        let old_hello: Hello = serde_json::from_value(old_hello_value).expect("valid hello shape");
        let survivor_epoch = Hello::new(Role::Infiltrator, &script, 0);
        harness.check(
            "old-session-hello-epoch-refused",
            !survivor_epoch.compatible_with(&old_hello),
            "a mismatched seed cannot join the recovered epoch",
        );
        harness.record_validator_rejection(
            crate::telemetry::ViolationKind::EpochConflict,
            "old-session hello carried a different seed",
        );

        // 7. Complete the recovered run and compare both seats against the
        //    exact headless reference (the existing lockstep oracle).
        let mut finish_guard = 0;
        while !(pair.infiltrator.finished() && pair.hacker.finished()) {
            finish_guard += 1;
            assert!(finish_guard <= 200, "recovered run did not converge");
            harness.exchange_round(&mut pair, &mut queue, 8);
        }
        let (_, reference_log) = headless::simulate(&script).expect("headless reference");
        let reference = serde_json::to_value(&reference_log).expect("reference serialises");
        let inf_log = serde_json::to_value(pair.infiltrator.log()).expect("log serialises");
        let hac_log = serde_json::to_value(pair.hacker.log()).expect("log serialises");
        harness.check(
            "survivor-matches-reference",
            inf_log == reference,
            "the surviving seat's full event log is the headless run",
        );
        harness.check(
            "rejoined-seat-matches-reference",
            hac_log == reference,
            "the replaced seat reconstructs the whole run it half-missed",
        );

        weevil.retire();
        let notes = vec![
            "The same death/resync path is exercised over a real burble relay by scripts/loopback_check.sh run 4; this scenario adds the adversarial obsolete-frame and old-epoch checks.".into(),
            "Recovery authority is peer-to-peer snapshot exchange: the relay holds no sim state and no administrative component participates.".into(),
        ];
        finalize(spec, &mut harness, notes)
    }
}

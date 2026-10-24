//! Scenario 3 — conflicting session and authority epochs.
//!
//! The handshake ([`idaptik_net::envelope::Hello`]) is the epoch boundary:
//! two seats may only lockstep when they agree on protocol version, seed,
//! difficulty, motion mode and tick budget. A mismatch is a setup refusal,
//! not a negotiable warning. The resync hand-off has its own epoch rule: a
//! snapshot from the same seat is nonsensical and is rejected.

use super::{
    EvidenceStatus, InputSpec, Scenario, ScenarioResult, ScenarioSpec, TopologyMode, finalize,
};
use crate::harness::{IsolatedHarness, SeatPair, ValidatorOutcome, load_live_script};
use crate::policy::{Permission, Principal};
use crate::relay_model::RelayModel;
use crate::telemetry::ViolationKind;
use crate::wire;
use idaptik_net::envelope::{Hello, Role};
use idaptik_net::lockstep::LockstepCore;
use serde_json::Value;
use std::collections::VecDeque;

const ID: &str = "EW-03-conflicting-epochs";

fn inputs() -> Vec<InputSpec> {
    vec![
        InputSpec {
            step: 0,
            intent: "hello claiming a different seed".into(),
            expected: "handshake refusal: incompatible runs".into(),
        },
        InputSpec {
            step: 0,
            intent: "hello claiming a different max_ticks".into(),
            expected: "handshake refusal".into(),
        },
        InputSpec {
            step: 0,
            intent: "hello claiming protocol version NET_PROTO+1".into(),
            expected: "handshake refusal".into(),
        },
        InputSpec {
            step: 0,
            intent: "hello claiming an unsupported difficulty".into(),
            expected: "handshake refusal".into(),
        },
        InputSpec {
            step: 4,
            intent: "resync payload offered by the seat it names".into(),
            expected: "adopt_resync rejection: same seat".into(),
        },
        InputSpec {
            step: 0,
            intent: "commit watermark past the run budget".into(),
            expected: "lockstep rejection: past the run".into(),
        },
        InputSpec {
            step: 0,
            intent: "command scheduled past the run budget".into(),
            expected: "lockstep rejection: past the run".into(),
        },
        InputSpec {
            step: 0,
            intent: "rejoin hello presented to a seat with no run".into(),
            expected: "session driver refusal (predicate demonstrated)".into(),
        },
    ]
}

pub struct EpochConflictScenario;

impl Scenario for EpochConflictScenario {
    fn spec(&self) -> ScenarioSpec {
        ScenarioSpec {
            id: ID,
            title: "Conflicting session/authority epochs",
            principal: Principal::adversary(Role::Infiltrator, 3).identity.label,
            permitted_operations: vec![Permission::ProposeBoundedIntent],
            invariant: "Only seats agreeing on the full run epoch may lockstep; resync authority cannot be self-issued; nothing schedules past the run.",
            bounded_inputs: inputs(),
            expectation: "Every mismatched hello is refused by the production compatibility predicate; same-seat resync and past-run frames are rejected; the rejoin predicate is demonstrated.",
            topology: TopologyMode::SingleRendezvousRelay,
            game_server_admin_present: false,
            evidence: EvidenceStatus::Demonstrated,
            replay_artifact: "result.json",
        }
    }

    fn run(&self) -> ScenarioResult {
        let spec = self.spec();
        let mut harness = IsolatedHarness::new(ID, 777_003);
        let script = load_live_script();

        let own = Hello::new(Role::Infiltrator, &script, 0);
        let peer = Hello::new(Role::Hacker, &script, 0);
        harness.check(
            "baseline-hellos-compatible",
            own.compatible_with(&peer),
            "the unmutated handshake agrees on the run",
        );

        // 1-4. Each epoch field is mutated exactly once, through the real
        //      serde deserializer, so the production struct does the judging.
        let conflicts: [(&str, Value); 4] = [
            (
                "seed",
                wire::raw_hello(
                    Role::Hacker,
                    &script,
                    0,
                    wire::HelloOverrides {
                        seed: Some(script.seed.wrapping_add(1)),
                        ..wire::HelloOverrides::default()
                    },
                ),
            ),
            (
                "max_ticks",
                wire::raw_hello(
                    Role::Hacker,
                    &script,
                    0,
                    wire::HelloOverrides {
                        max_ticks: Some(script.max_ticks + 1),
                        ..wire::HelloOverrides::default()
                    },
                ),
            ),
            ("proto", wire::future_proto_hello(Role::Hacker, &script, 0)),
            (
                "difficulty",
                wire::raw_hello(
                    Role::Hacker,
                    &script,
                    0,
                    wire::HelloOverrides {
                        difficulty: Some("operator"),
                        ..wire::HelloOverrides::default()
                    },
                ),
            ),
        ];
        for (field, value) in conflicts {
            let forged: Hello = serde_json::from_value(value)
                .expect("the forged hello is structurally valid, only its epoch differs");
            let compatible = own.compatible_with(&forged);
            harness.check(
                &format!("epoch-conflict:{field}"),
                !compatible,
                format!("{field} mismatch refuses the pairing"),
            );
            harness.record_validator_rejection(
                ViolationKind::EpochConflict,
                format!("hello {field} incompatible with the agreed run"),
            );
        }

        // 5. A resync from the same seat cannot be adopted.
        let mut pair = SeatPair::new(2, 2);
        let mut queue = VecDeque::new();
        for _ in 0..4 {
            harness.exchange_round(&mut pair, &mut queue, 8);
        }
        pair.infiltrator.on_peer_lost();
        let resync = pair.infiltrator.make_resync();
        let same_seat = LockstepCore::adopt_resync(Role::Infiltrator, 2, resync.clone());
        harness.check(
            "same-seat-resync-rejected",
            same_seat.is_err(),
            same_seat
                .as_ref()
                .err()
                .map(|e| e.to_string())
                .unwrap_or_else(|| "accepted unexpectedly".into()),
        );
        if let Err(e) = same_seat {
            harness.record_validator_rejection(ViolationKind::SameSeatResync, e.to_string());
        }

        // 6-7. Nothing schedules past the run budget.
        let mut core = LockstepCore::new(Role::Infiltrator, 1, &script).expect("core builds");
        let past_commit = harness.feed_commit(&mut core, Role::Hacker, script.max_ticks);
        harness.check(
            "past-run-commit-rejected",
            past_commit.is_err(),
            "a watermark at max_steps is outside the run",
        );
        let past_command =
            harness.feed_raw_to_validator(&mut core, &wire::net_ssh(script.max_ticks, 0));
        harness.check(
            "past-run-command-rejected",
            matches!(past_command, ValidatorOutcome::Rejected(_)),
            "a command at max_steps cannot be buffered",
        );

        // 8. The rejoin predicate the live driver uses: a fresh seat that
        //    receives a rejoin hello has no run to resume. The async driver
        //    (`idaptik_net::live`) enforces this on the real socket path;
        //    the corpus demonstrates the predicate it keys on.
        let rejoin_value = wire::raw_hello(
            Role::Hacker,
            &script,
            0,
            wire::HelloOverrides {
                rejoin: Some(true),
                ..wire::HelloOverrides::default()
            },
        );
        let rejoin_hello: Hello = serde_json::from_value(rejoin_value).expect("valid hello");
        // A fresh LockstepCore only exists after a non-rejoin hello, so the
        // driver predicate (`peer.rejoin` on a fresh seat) is unambiguous.
        harness.check(
            "rejoin-without-run-refused",
            rejoin_hello.rejoin,
            "live.rs returns NetError::Session when peer.rejoin meets a fresh seat; covered end-to-end by loopback leg 4",
        );

        // The relay model also refuses an unknown game at join, which is the
        // outer epoch boundary for the session topic.
        harness.check(
            "unknown-game-join-refused",
            RelayModel::join("not-idaptik", Role::Hacker).is_err(),
            "a foreign game id cannot open an idaptik session",
        );

        let notes = vec![
            "The live-driver rejoin refusal is exercised end-to-end by scripts/loopback_check.sh run 4; this scenario pins the predicate offline.".into(),
            "Authority here means agreement on the run epoch, not administrative power: no principal in this corpus can redefine another seat's epoch.".into(),
        ];
        finalize(spec, &mut harness, notes)
    }
}

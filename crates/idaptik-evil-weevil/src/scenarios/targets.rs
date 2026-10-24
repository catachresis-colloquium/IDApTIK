//! Scenario 2 — impossible movement or invalid target claims.
//!
//! Two distinct host surfaces are under test, and the distinction is the
//! point:
//!
//! - **Protocol-level** frames (unknown tags, malformed payloads) are
//!   rejected by the decoders before any gameplay code sees them.
//! - **Typed but impossible** gameplay intents (an out-of-graph node index,
//!   an uplink with no route, a pivot the network refuses) decode fine and
//!   reach the real [`GhostLobbySim`], which no-ops or emits a typed
//!   `*Denied` event. A typed denial is normal host-coped behaviour, not a
//!   protocol violation — the observer classifies it benign.
//!
//! The wire carries no position-claim command at all: movement is a held
//! button intent, and the sim alone decides where the body ends up, clamped
//! inside the authored world.

use super::{
    EvidenceStatus, InputSpec, Scenario, ScenarioResult, ScenarioSpec, TopologyMode, finalize,
};
use crate::agent::{EvilWeevil, IntentKind};
use crate::harness::{IsolatedHarness, ValidatorOutcome, load_live_script};
use crate::policy::{Permission, Principal};
use crate::wire;
use idaptik_core::scenario::command::{
    Button, Buttons, Command, NetNodeIndex, PivotTarget, fold,
};
use idaptik_core::scenario::common::DenyReason;
use idaptik_core::scenario::constants::WORLD_WIDTH;
use idaptik_core::scenario::event::Event;
use idaptik_core::scenario::tuning::ActionKind;
use idaptik_net::envelope::Role;
use idaptik_net::lockstep::LockstepCore;
use idaptik_tui::headless;

const ID: &str = "EW-02-impossible-movement-invalid-targets";

fn inputs() -> Vec<InputSpec> {
    vec![
        InputSpec { step: 1, intent: "NetSsh claiming a node index beyond the graph".into(), expected: "decodes; sim no-op, state unchanged, no panic".into() },
        InputSpec { step: 1, intent: "NetHack claiming the same out-of-graph index".into(), expected: "decodes; sim no-op".into() },
        InputSpec { step: 1, intent: "Camera uplink cold from the van (no pivot)".into(), expected: "UplinkDenied(NoRoute)".into() },
        InputSpec { step: 1, intent: "Pivot onto the grid jump host cold".into(), expected: "PivotDenied (ISP hop required first)".into() },
        InputSpec { step: 30, intent: "hold right for 30 ticks against the world edge".into(), expected: "position stays finite and inside WORLD_WIDTH".into() },
        InputSpec { step: 0, intent: "forge a TeleportToRoom position-claim tag".into(), expected: "decoder rejection: unknown command".into() },
    ]
}

pub struct InvalidTargetScenario;

impl Scenario for InvalidTargetScenario {
    fn spec(&self) -> ScenarioSpec {
        ScenarioSpec {
            id: ID,
            title: "Impossible movement or invalid target claims",
            principal: Principal::adversary(Role::Hacker, 2).identity.label,
            permitted_operations: vec![Permission::ProposeBoundedIntent],
            invariant: "The wire cannot author positions; typed-but-impossible intents are no-ops or typed denials that leave state coherent, and untyped frames never reach the sim.",
            bounded_inputs: inputs(),
            expectation: "Out-of-graph indices change nothing and never panic; unreachable actions produce UplinkDenied/PivotDenied; movement stays inside the world; the forged position verb is rejected by the decoder.",
            topology: TopologyMode::SingleRendezvousRelay,
            game_server_admin_present: false,
            evidence: EvidenceStatus::Demonstrated,
            replay_artifact: "result.json",
        }
    }

    fn run(&self) -> ScenarioResult {
        let spec = self.spec();
        let mut harness = IsolatedHarness::new(ID, 777_002);
        let script = load_live_script();
        let proposer = Principal::adversary(Role::Hacker, 2);
        let mut weevil = EvilWeevil::new(proposer.clone());

        // An index the authored graph cannot contain.
        let probe_sim = headless::build(&script).expect("sim builds");
        let invalid_node = probe_sim.graph().nodes.len() as u32 + 100;

        // 1-2. Out-of-graph NetSsh/NetHack: compare against a quiet tick.
        let (baseline_sim, _, _) = IsolatedHarness::sim_tick(&script, &[]);
        let (ssh_sim, ssh_events, _) =
            IsolatedHarness::sim_tick(&script, &[Command::NetSsh {
                node: NetNodeIndex(invalid_node),
            }]);
        let (hack_sim, _, _) =
            IsolatedHarness::sim_tick(&script, &[Command::NetHack {
                node: NetNodeIndex(invalid_node),
            }]);
        harness.check(
            "out-of-graph-netssh-is-a-noop",
            ssh_sim.state() == baseline_sim.state() && ssh_events.iter().all(|e| !matches!(
                e,
                Event::PivotOpened { .. } | Event::NetHackAction { .. }
            )),
            "an unknown index neither pivots nor acts",
        );
        harness.check(
            "out-of-graph-nethack-is-a-noop",
            hack_sim.state() == baseline_sim.state(),
            "the NetHack path also shrugs at an unknown index",
        );
        harness.record_sim_outcome("NetSsh", None, true);
        harness.record_sim_outcome("NetHack", None, true);

        // 3. Cold camera uplink: the van has no route to the floor.
        let (_, uplink_events, _) =
            IsolatedHarness::sim_tick(&script, &[Command::Uplink {
                kind: ActionKind::Camera,
            }]);
        let uplink_denied = uplink_events.iter().any(|e| matches!(
            e,
            Event::UplinkDenied {
                reason: DenyReason::NoRoute,
                ..
            }
        ));
        harness.check(
            "cold-uplink-denied-no-route",
            uplink_denied,
            "UplinkDenied(NoRoute) is emitted rather than applying",
        );
        harness.record_sim_outcome(
            "Uplink",
            uplink_denied.then_some("NoRoute"),
            false,
        );

        // 4. Grid jump pivot cold: the ISP hop is a precondition.
        let (_, pivot_events, _) =
            IsolatedHarness::sim_tick(&script, &[Command::Pivot {
                target: PivotTarget::GridJump,
            }]);
        let pivot_denied = pivot_events
            .iter()
            .any(|e| matches!(e, Event::PivotDenied { .. }));
        harness.check(
            "cold-grid-pivot-denied",
            pivot_denied,
            "the upstream line is walked one hop at a time",
        );
        harness.record_sim_outcome("Pivot", pivot_denied.then_some("NoRoute"), false);

        // 5. Held movement can never leave the authored world.
        let mut move_sim = headless::build(&script).expect("sim builds");
        let _startup = move_sim.drain_events();
        let mut held = Buttons::default();
        for _ in 0..30 {
            let input = fold(
                &[Command::SetButton {
                    button: Button::Right,
                    down: true,
                }],
                &mut held,
            );
            move_sim.tick(&input);
        }
        let x = move_sim.state().player.x;
        harness.check(
            "movement-clamped-inside-the-world",
            x.is_finite() && x >= 0.0 && x <= WORLD_WIDTH,
            format!("player x after 30 held-right ticks: {x} (WORLD_WIDTH={WORLD_WIDTH})"),
        );

        // 6. The wire has no position-claim verb: forging one fails at the
        //    production decoder, not inside the sim.
        let mut core = LockstepCore::new(Role::Infiltrator, 1, &script).expect("core builds");
        let forged = weevil
            .propose(
                IntentKind::InvalidTargetClaim,
                wire::unknown_command(0, "TeleportToRoom"),
                "claim a position the sender has no authority over",
            )
            .unwrap();
        harness.record_intent(&forged);
        let outcome = harness.feed_raw_to_validator(&mut core, &forged.wire);
        harness.check(
            "position-claim-verb-does-not-exist",
            matches!(outcome, ValidatorOutcome::Undecodable(_)),
            "no Command variant carries a position; the forged tag cannot decode",
        );

        // The fault/hostility distinction, made explicit: the four gameplay
        // outcomes are benign host-coped evidence; the only violation in the
        // trace is the forged protocol-level tag above, which never reached
        // the sim at all.
        let report = crate::observer::DefensiveObserver::evaluate(harness.telemetry());
        harness.check(
            "gameplay-denials-stay-benign",
            report.counts.benign >= 4 && report.counts.violation == 1,
            format!(
                "benign={}, fault={}, violation={} (the single violation is the forged wire tag)",
                report.counts.benign, report.counts.fault, report.counts.violation
            ),
        );

        weevil.retire();
        let notes = vec![
            "The sim's unknown-id no-op behaviour is itself load-bearing: the no_panic_fuzz suite drives the same surface with ids no fixture answers to.".into(),
            "There is no server-side authority to lie to about positions in relay-only topology; the authoritative check is the local sim on every seat, compared by digest.".into(),
        ];
        finalize(spec, &mut harness, notes)
    }
}

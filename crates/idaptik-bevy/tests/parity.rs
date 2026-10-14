//! Frontend parity: a headless Bevy `App` stepping a scripted [`Command`]
//! stream must emit exactly the `Event`s a plain headless run of the same
//! script emits — the invariant that makes the Bevy layer a *renderer* and
//! never a second simulation.
//!
//! The reference loop below is the TUI's driver loop distilled (`headless.rs`
//! / `app.rs`: fold the tick's commands into a `TickInput`, call `tick`,
//! collect the events). The Bevy side drives the identical script through
//! [`idaptik_bevy::driver`]'s `FixedUpdate` system instead, with no window and
//! no render plugins.

use bevy::prelude::*;
use idaptik_bevy::driver::{CommandQueue, SimDriverPlugin, SimState};
use idaptik_core::RunConfig;
use idaptik_core::scenario::command::{Button, Buttons, Command, PivotTarget, fold};
use idaptik_core::scenario::event::Event as SimEvent;
use idaptik_core::scenario::tuning::ActionKind;
use idaptik_core::scenario::{GhostLobbySim, RuntimeSnapshot, ghost_lobby};

const SEED: u32 = 123456;
const TICKS: u64 = 900;

/// The scripted command stream: every prototype verb the keyboard can produce,
/// including a mid-run restart, so the parity assertion covers the whole wire.
fn commands_at(tick: u64) -> Vec<Command> {
    let held = |button, down| Command::SetButton { button, down };
    match tick {
        0 => vec![held(Button::Right, true), held(Button::Sprint, true)],
        40 => vec![Command::Jump],
        90 => vec![held(Button::Sprint, false)],
        120 => vec![held(Button::Crouch, true)],
        180 => vec![held(Button::Crouch, false)],
        200 => vec![held(Button::Right, false), held(Button::Left, true)],
        240 => vec![held(Button::Interact, true), Command::Interact],
        330 => vec![held(Button::Interact, false)],
        // Uplinks, cold from the van (the denials are events too).
        400 => vec![Command::Uplink {
            kind: ActionKind::Camera,
        }],
        420 => vec![Command::Uplink {
            kind: ActionKind::Door,
        }],
        440 => vec![Command::Uplink {
            kind: ActionKind::Vacuum,
        }],
        460 => vec![Command::Uplink {
            kind: ActionKind::Lights,
        }],
        // The pivot line: bridge, both upstream hops, and back out.
        500 => vec![Command::Pivot {
            target: PivotTarget::Bridge,
        }],
        520 => vec![Command::Pivot {
            target: PivotTarget::IspOps,
        }],
        540 => vec![Command::Pivot {
            target: PivotTarget::GridJump,
        }],
        560 => vec![Command::Unpivot],
        600 => vec![Command::ThrowUsb],
        620 => vec![Command::Pause { on: true }],
        660 => vec![Command::Pause { on: false }],
        700 => vec![Command::Restart],
        720 => vec![held(Button::Right, true)],
        760 => vec![Command::Jump],
        800 => vec![Command::Uplink {
            kind: ActionKind::Camera,
        }],
        840 => vec![Command::Pivot {
            target: PivotTarget::Bridge,
        }],
        _ => Vec::new(),
    }
}

/// The reference: the TUI's headless driver semantics against the bare sim.
fn reference_run() -> (Vec<SimEvent>, RuntimeSnapshot) {
    let mut sim = GhostLobbySim::new(ghost_lobby(), RunConfig::standard(), SEED)
        .expect("the canonical definition is valid");
    let mut events = sim.drain_events();
    let mut held = Buttons::default();
    for tick in 0..TICKS {
        let cmds = commands_at(tick);
        let input = fold(&cmds, &mut held);
        events.extend(sim.tick(&input));
    }
    (events, sim.snapshot())
}

/// The same script through the headless Bevy `App`, one `FixedUpdate` per tick.
fn bevy_run() -> (Vec<SimEvent>, RuntimeSnapshot) {
    let mut app = App::new();
    app.add_plugins(SimDriverPlugin {
        cfg: RunConfig::standard(),
        seed: SEED,
        wired: false,
    });
    for tick in 0..TICKS {
        app.world_mut()
            .resource_mut::<CommandQueue>()
            .pending
            .extend(commands_at(tick));
        app.world_mut().run_schedule(FixedUpdate);
    }
    let state = app.world().resource::<SimState>();
    (state.event_log.clone(), state.sim.snapshot())
}

#[test]
fn the_bevy_frontend_replays_the_headless_run_event_for_event() {
    let (reference_events, reference_snapshot) = reference_run();
    let (bevy_events, bevy_snapshot) = bevy_run();

    assert_eq!(
        bevy_events.len(),
        reference_events.len(),
        "the Bevy driver must emit exactly as many events as the headless run"
    );
    for (i, (bevy, reference)) in bevy_events.iter().zip(&reference_events).enumerate() {
        assert_eq!(bevy, reference, "event #{i} diverged");
    }
    assert_eq!(
        bevy_snapshot, reference_snapshot,
        "the final runtime snapshot must be identical too"
    );

    // The script is not allowed to be trivial: the run must actually have
    // walked the wire (movement, uplinks, pivots, a restart).
    assert!(
        reference_events
            .iter()
            .any(|e| matches!(e, SimEvent::Restarted { .. })),
        "the script restarts mid-run"
    );
    assert!(
        reference_events.iter().any(|e| matches!(
            e,
            SimEvent::PivotOpened { .. } | SimEvent::PivotDenied { .. }
        )),
        "the script walks the pivot line"
    );
}

#[test]
fn the_same_seed_and_script_reproduce_the_same_run() {
    // Bevy only renders: two runs of the identical seed + input are identical.
    let (first_events, first_snapshot) = bevy_run();
    let (second_events, second_snapshot) = bevy_run();
    assert_eq!(first_events, second_events);
    assert_eq!(first_snapshot, second_snapshot);
}

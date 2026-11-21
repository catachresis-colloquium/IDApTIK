//! Stage B: snapshot at tick N, restore, continue M ticks == uninterrupted N+M.

mod common;
use common::Runner;
use idaptik_core::scenario::command::{Button, Command, PivotTarget, TickInput, fold};
use idaptik_core::scenario::definition::ValidationError;
use idaptik_core::scenario::event::Event;
use idaptik_core::scenario::{GhostLobbySim, ghost_lobby};

fn step_stream(t: u64) -> Vec<Command> {
    let mut cmds = vec![Command::SetButton {
        button: Button::Right,
        down: true,
    }];
    // The uplinks below are route-gated, so the stream opens with the pivot —
    // through the canonical command path, so a recorded stream replays whole.
    if t == 0 {
        cmds.push(Command::Pivot {
            target: PivotTarget::Bridge,
        });
    }
    if t == 30 {
        cmds.push(Command::ForceCrisis);
    }
    if t == 80 {
        cmds.push(Command::Uplink {
            kind: idaptik_core::scenario::ActionKind::Camera,
        });
    }
    // An uplink on the far side of the snapshot: the hacker's pivot lives in the
    // runtime state, so this only lands if the session survived the round trip.
    if t == 200 {
        cmds.push(Command::Uplink {
            kind: idaptik_core::scenario::ActionKind::Lights,
        });
    }
    cmds
}

#[test]
fn snapshot_restore_continues_identically() {
    // Uninterrupted reference run of 300 ticks.
    let mut reference = Runner::standard();
    for t in 0..300u64 {
        reference.step(&step_stream(t));
    }

    // Interrupted run: 150 ticks, snapshot, restore, 150 more.
    let mut a = Runner::standard();
    for t in 0..150u64 {
        a.step(&step_stream(t));
    }
    let snap = a.sim.snapshot();
    let held = a.held;
    let mut restored = GhostLobbySim::restore(ghost_lobby(), snap).expect("restore");

    let mut log: Vec<Event> = a.log.clone();
    let mut held = held;
    for t in 150..300u64 {
        let cmds = step_stream(t);
        let input: TickInput = fold(&cmds, &mut held);
        log.extend(restored.tick(&input));
    }

    let full = serde_json::to_string(&reference.log).unwrap();
    let split = serde_json::to_string(&log).unwrap();
    assert_eq!(full, split, "snapshot/restore must not perturb the stream");
    assert_eq!(reference.sim.current_tick(), restored.current_tick());
}

#[test]
fn paused_snapshot_restores_paused() {
    // Pause mid-run, snapshot, restore: the restored sim must still be paused so
    // subsequent non-pause commands are ignored (matching the paused original).
    let mut a = Runner::standard();
    for t in 0..40u64 {
        a.step(&step_stream(t));
    }
    a.step(&[Command::Pause { on: true }]);
    assert!(a.sim.is_paused());
    let snap = a.sim.snapshot();
    assert!(snap.paused, "snapshot captures the paused flag");

    let mut restored = GhostLobbySim::restore(ghost_lobby(), snap).expect("restore");
    assert!(restored.is_paused(), "restore round-trips paused");

    // A held-right tick is a no-op while paused: tick unchanged, no events.
    let before_tick = restored.current_tick();
    let mut held = a.held;
    let input = fold(
        &[Command::SetButton {
            button: Button::Right,
            down: true,
        }],
        &mut held,
    );
    let ev = restored.tick(&input);
    assert!(ev.is_empty(), "paused restore ignores the command stream");
    assert_eq!(restored.current_tick(), before_tick);
}

#[test]
fn restore_refuses_a_foreign_snapshot_format() {
    // A snapshot from another format family (say, a v1 file on disk) must be
    // refused up front with a typed error, not left to fail obscurely on shape.
    let mut r = Runner::standard();
    for t in 0..10u64 {
        r.step(&step_stream(t));
    }
    let mut snap = r.sim.snapshot();
    snap.format = "idaptik-ghost-lobby-runtime-v1".to_owned();
    let errs = GhostLobbySim::restore(ghost_lobby(), snap).expect_err("v1 must be refused");
    assert_eq!(
        errs,
        vec![ValidationError::UnsupportedSnapshotFormat {
            found: "idaptik-ghost-lobby-runtime-v1".to_owned()
        }]
    );
}

#[test]
fn snapshot_round_trips_through_json() {
    let mut r = Runner::standard();
    for t in 0..90u64 {
        r.step(&step_stream(t));
    }
    let snap = r.sim.snapshot();
    let json = serde_json::to_string(&snap).unwrap();
    let back: idaptik_core::scenario::RuntimeSnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(snap, back);
    assert_eq!(
        back.format,
        idaptik_core::scenario::snapshot::SNAPSHOT_FORMAT
    );
}

#[test]
fn restore_refuses_a_state_whose_shape_differs_from_its_definition() {
    use idaptik_core::scenario::command::RunConfig;
    use idaptik_core::scenario::rooftop::ghost_lobby_rooftop;
    // Lobby: one door state missing.
    let mut r = Runner::standard();
    r.step(&step_stream(0));
    let mut snap = r.sim.snapshot();
    snap.state.doors.pop();
    let errs = GhostLobbySim::restore(ghost_lobby(), snap).expect_err("short doors");
    assert_eq!(
        errs,
        vec![ValidationError::SnapshotStateMismatch {
            field: "doors".into()
        }]
    );
    // Rooftop: the signal devices cut to 2. Before the check this restored
    // and the first tick panicked with an index out of bounds.
    let sim = GhostLobbySim::new(
        ghost_lobby_rooftop(),
        RunConfig {
            difficulty: idaptik_core::scenario::tuning::DifficultyId::Standard,
            reduced_motion: false,
            supervised: false,
        },
        7,
    )
    .expect("valid");
    let mut snap = sim.snapshot();
    snap.state
        .signal
        .as_mut()
        .expect("rooftop is wired")
        .devices
        .truncate(2);
    let errs = GhostLobbySim::restore(ghost_lobby_rooftop(), snap).expect_err("short signal");
    assert_eq!(
        errs,
        vec![ValidationError::SnapshotStateMismatch {
            field: "signal".into()
        }]
    );
}

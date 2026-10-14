//! Render interpolation, driven through the real Bevy driver.
//!
//! `tests/parity.rs` proves the Bevy layer simulates identically to a headless
//! run. This file proves the *smoothing* layered on top of it is honest: at a
//! tick boundary the picture is exactly what the un-interpolated renderer drew,
//! between boundaries it stays inside the interval, and a restart does not
//! render as a slide across the level.

use bevy::prelude::*;
use idaptik_bevy::driver::{CommandQueue, SimDriverPlugin, SimState, VisualBuffers};
use idaptik_core::RunConfig;
use idaptik_core::interp::{VisualSlot, door_opens_of, poses_of};
use idaptik_core::scenario::command::{Button, Command};
use idaptik_core::scenario::event::Event as SimEvent;

const SEED: u32 = 123456;

/// A headless app with the sim driver and nothing else — no window, no render
/// plugins. Exactly what `parity.rs` builds.
fn headless_app() -> App {
    let mut app = App::new();
    app.add_plugins(SimDriverPlugin {
        cfg: RunConfig::standard(),
        seed: SEED,
        wired: false,
    });
    app
}

fn step(app: &mut App, cmds: Vec<Command>) {
    app.world_mut()
        .resource_mut::<CommandQueue>()
        .pending
        .extend(cmds);
    app.world_mut().run_schedule(FixedUpdate);
}

fn walk_right(app: &mut App, ticks: u32) {
    step(
        app,
        vec![Command::SetButton {
            button: Button::Right,
            down: true,
        }],
    );
    for _ in 1..ticks {
        step(app, Vec::new());
    }
}

#[test]
fn at_a_tick_boundary_the_picture_is_the_un_interpolated_one() {
    // alpha = 1.0 is the moment the tick lands. Interpolation must be a no-op
    // there: anything else means smoothing changed what the renderer draws,
    // rather than only filling the gaps between draws.
    let mut app = headless_app();
    walk_right(&mut app, 90);

    let state = app.world().resource::<SimState>();
    let visual = app.world().resource::<VisualBuffers>();
    let live = poses_of(state.sim.state());
    let live_doors = door_opens_of(state.sim.state());

    for slot in [
        VisualSlot::Player,
        VisualSlot::Billy,
        VisualSlot::Note,
        VisualSlot::Usb,
        VisualSlot::Vacuum,
    ] {
        let sampled = visual.poses.sample(slot.index(), 1.0);
        assert_eq!(
            sampled,
            live[slot.index()],
            "{slot:?} at alpha 1 must equal the live tick exactly"
        );
    }
    for (i, live_open) in live_doors.iter().enumerate() {
        assert_eq!(
            visual.doors.sample(i, 1.0),
            *live_open,
            "door {i} at alpha 1"
        );
    }
}

#[test]
fn between_ticks_the_player_stays_inside_the_interval() {
    let mut app = headless_app();
    walk_right(&mut app, 60);

    let slot = VisualSlot::Player.index();
    let (prev, curr) = {
        let visual = app.world().resource::<VisualBuffers>();
        (visual.poses.prev(slot).x, visual.poses.curr(slot).x)
    };
    assert_ne!(
        prev, curr,
        "the player must actually be moving for this to test anything"
    );

    let visual = app.world().resource::<VisualBuffers>();
    let (lo, hi) = if prev <= curr {
        (prev, curr)
    } else {
        (curr, prev)
    };
    for step_n in 0..=8 {
        let alpha = f64::from(step_n) / 8.0;
        let x = visual.poses.sample(slot, alpha).x;
        assert!(
            (lo..=hi).contains(&x),
            "alpha {alpha} sampled {x}, outside [{lo}, {hi}]"
        );
    }
}

#[test]
fn interpolation_advances_monotonically_with_alpha() {
    let mut app = headless_app();
    walk_right(&mut app, 60);

    let slot = VisualSlot::Player.index();
    let visual = app.world().resource::<VisualBuffers>();
    let forward = visual.poses.curr(slot).x >= visual.poses.prev(slot).x;

    let mut last = visual.poses.sample(slot, 0.0).x;
    for step_n in 1..=8 {
        let alpha = f64::from(step_n) / 8.0;
        let x = visual.poses.sample(slot, alpha).x;
        if forward {
            assert!(x >= last, "went backwards at alpha {alpha}: {last} → {x}");
        } else {
            assert!(x <= last, "went forwards at alpha {alpha}: {last} → {x}");
        }
        last = x;
    }
}

#[test]
fn a_restart_does_not_render_as_a_slide_back_to_spawn() {
    // The discontinuity case, driven through the real driver rather than the
    // buffer in isolation. Walk far from spawn, restart, and assert that every
    // alpha in the interval containing the restart sits at the spawn position.
    // If `step_sim` ever snaps without committing, this fails.
    let mut app = headless_app();
    walk_right(&mut app, 200);

    let travelled = {
        let state = app.world().resource::<SimState>();
        state.sim.state().player.x
    };

    let before_events = app.world().resource::<SimState>().event_log.len();
    step(&mut app, vec![Command::Restart]);

    let state = app.world().resource::<SimState>();
    let restarted = state.event_log[before_events..]
        .iter()
        .any(|e| matches!(e, SimEvent::Restarted { .. }));
    assert!(
        restarted,
        "the Restart command must actually have restarted the run"
    );

    let spawn_x = state.sim.state().player.x;
    assert_ne!(
        spawn_x, travelled,
        "the player must have moved away from spawn for this to test anything"
    );

    let visual = app.world().resource::<VisualBuffers>();
    let slot = VisualSlot::Player.index();
    for step_n in 0..=8 {
        let alpha = f64::from(step_n) / 8.0;
        let x = visual.poses.sample(slot, alpha).x;
        assert_eq!(
            x, spawn_x,
            "alpha {alpha} rendered {x}: the restart is being interpolated across \
             (was at {travelled}, spawn is {spawn_x})"
        );
    }
}

#[test]
fn the_first_frame_does_not_slide_from_the_origin() {
    // Before any tick has run, both buffers must already hold the start state,
    // or the opening frame draws the actors racing in from wherever
    // `Pose::default()` happens to be.
    let app = headless_app();
    let state = app.world().resource::<SimState>();
    let visual = app.world().resource::<VisualBuffers>();
    let live = poses_of(state.sim.state());
    let slot = VisualSlot::Player.index();

    assert_eq!(visual.poses.prev(slot), live[slot]);
    assert_eq!(
        visual.poses.sample(slot, 0.0),
        live[slot],
        "no slide at alpha 0"
    );
    assert_eq!(
        visual.poses.sample(slot, 1.0),
        live[slot],
        "no slide at alpha 1"
    );
}

#[test]
fn buffers_do_not_perturb_the_simulation() {
    // The determinism guard at the frontend seam: `parity.rs` proves the driver
    // matches a headless run, and this proves adding the visual buffers did not
    // change that. Two identical runs must stay identical while sampled.
    let mut a = headless_app();
    let mut b = headless_app();

    for tick in 0..300 {
        let cmds = if tick == 0 {
            vec![Command::SetButton {
                button: Button::Right,
                down: true,
            }]
        } else {
            Vec::new()
        };
        step(&mut a, cmds.clone());
        step(&mut b, cmds);

        // Sample `a` as a renderer would; `b` is never sampled.
        let visual = a.world().resource::<VisualBuffers>();
        for step_n in 0..=3 {
            let _ = visual
                .poses
                .sample(VisualSlot::Player.index(), f64::from(step_n) / 3.0);
        }
    }

    assert_eq!(
        a.world().resource::<SimState>().sim.snapshot(),
        b.world().resource::<SimState>().sim.snapshot(),
        "sampling changed the run"
    );
}

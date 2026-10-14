//! Billy's arrival clock: the canonical floor starts it with the run; the
//! played (wired) floor holds it until the infiltrator leaves the room they
//! began in, so nobody is caught while still learning the controls.

use idaptik_core::scenario::command::{Button, Command, PivotTarget, RunConfig, fold};
use idaptik_core::scenario::common::Phase;
use idaptik_core::scenario::event::Event;
use idaptik_core::scenario::ghost_lobby::ghost_lobby;
use idaptik_core::scenario::signal::ghost_lobby_wiring::ghost_lobby_wired;
use idaptik_core::scenario::tuning::ActionKind;
use idaptik_core::scenario::{Buttons, GhostLobbySim};

const HZ: u32 = 60;

fn idle(sim: &mut GhostLobbySim, seconds: u32) -> Vec<Event> {
    let mut held = Buttons::default();
    (0..seconds * HZ)
        .flat_map(|_| sim.tick(&fold(&[], &mut held)))
        .collect()
}

#[test]
fn the_canonical_floor_sends_billy_on_the_run_clock() {
    let mut sim = GhostLobbySim::new(ghost_lobby(), RunConfig::standard(), 123456).expect("valid");
    let events = idle(&mut sim, 40);
    // An idle new player is not just found but caught: the run is already over.
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::CrisisBegan { .. })),
        "Billy came without being asked"
    );
    assert!(!events.contains(&Event::ArrivalClockStarted));
}

#[test]
fn the_played_floor_holds_billy_while_you_learn_the_controls() {
    let mut sim =
        GhostLobbySim::new(ghost_lobby_wired(), RunConfig::standard(), 123456).expect("valid");
    let before = sim.state().crisis_at;
    let events = idle(&mut sim, 120);
    assert_eq!(
        sim.state().phase,
        Phase::Quiet,
        "two idle minutes, no Billy"
    );
    assert!(sim.state().arrival_held);
    assert!(!events.contains(&Event::ArrivalClockStarted));
    // The countdown itself is untouched: the time left is what it was.
    assert!((sim.state().quiet_remaining - before).abs() < 0.05);
}

#[test]
fn leaving_the_kitchen_starts_the_clock_and_billy_then_comes() {
    let mut sim =
        GhostLobbySim::new(ghost_lobby_wired(), RunConfig::standard(), 123456).expect("valid");
    let mut held = Buttons::default();
    let mut events = Vec::new();
    let mut started_at = None;
    // Pivot onto the bridge, walk right, and keep asking for the door until
    // the hold opens it and we are through.
    for tick in 0..(60 * HZ) {
        let mut cmds = Vec::new();
        if tick == 0 {
            cmds.push(Command::Pivot {
                target: PivotTarget::Bridge,
            });
            cmds.push(Command::SetButton {
                button: Button::Right,
                down: true,
            });
        }
        if tick % 30 == 10 {
            cmds.push(Command::Uplink {
                kind: ActionKind::Door,
            });
        }
        let out = sim.tick(&fold(&cmds, &mut held));
        if started_at.is_none() && out.contains(&Event::ArrivalClockStarted) {
            started_at = Some(sim.state().t);
        }
        events.extend(out);
        if sim.state().phase == Phase::Crisis {
            break;
        }
    }
    let started_at = started_at.expect("walking out of the kitchen started the clock");
    assert!(!sim.state().arrival_held);
    assert_eq!(
        sim.state().phase,
        Phase::Crisis,
        "Billy came once the clock ran"
    );
    assert!(sim.state().t > started_at, "and only after it started");
}

//! Consoles: using one, and what its screen shows whom. The hacker has total
//! system knowledge and zero eyes — a screen shows systems within reach, and
//! people only as fixes the building logged.

use idaptik_core::scenario::command::{Button, Command, PivotTarget, RunConfig, fold};
use idaptik_core::scenario::console::{ScreenKind, ScreenView};
use idaptik_core::scenario::event::Event;
use idaptik_core::scenario::ghost_lobby::ghost_lobby;
use idaptik_core::scenario::signal::ghost_lobby_wiring::ghost_lobby_wired;
use idaptik_core::scenario::signal::sim::Seat;
use idaptik_core::scenario::tuning::ActionKind;
use idaptik_core::scenario::{Buttons, GhostLobbySim};

fn wired() -> GhostLobbySim {
    GhostLobbySim::new(ghost_lobby_wired(), RunConfig::standard(), 123456).expect("valid")
}

fn tick(sim: &mut GhostLobbySim, held: &mut Buttons, cmds: &[Command]) -> Vec<Event> {
    sim.tick(&fold(cmds, held))
}

fn deck(sim: &GhostLobbySim) -> ScreenView {
    sim.screen_view("van-deck", Seat::Hacker)
        .expect("the van has a deck")
}

#[test]
fn the_canonical_floor_has_no_consoles_and_logs_nothing() {
    let mut sim = GhostLobbySim::new(ghost_lobby(), RunConfig::standard(), 1).expect("valid");
    let mut held = Buttons::default();
    for _ in 0..600 {
        tick(&mut sim, &mut held, &[]);
    }
    assert!(sim.screen_view("van-deck", Seat::Hacker).is_none());
    assert!(sim.state().intel.is_empty());
}

#[test]
fn cold_from_the_van_the_deck_shows_no_building() {
    let sim = wired();
    let view = deck(&sim);
    assert!(view.devices.is_empty(), "{:?}", view.devices);
    assert!(view.infiltrator.is_none());
    assert_eq!(view.rooms.len(), 5, "the floor plan is public knowledge");
}

#[test]
fn pivoting_onto_the_bridge_brings_the_floor_onto_the_screen() {
    let mut sim = wired();
    let mut held = Buttons::default();
    tick(
        &mut sim,
        &mut held,
        &[Command::Pivot {
            target: PivotTarget::Bridge,
        }],
    );
    let view = deck(&sim);
    let kinds = |k: ScreenKind| view.devices.iter().filter(|d| d.kind == k).count();
    assert_eq!(kinds(ScreenKind::Door), 4);
    assert_eq!(kinds(ScreenKind::Light), 5);
    assert_eq!(kinds(ScreenKind::Camera), 3);
    assert!(view.devices.iter().all(|d| d.actionable), "the deck acts");
    let d1 = view
        .devices
        .iter()
        .find(|d| d.node == "door-d1")
        .expect("door-d1");
    assert_eq!(d1.status, "SHUT");
}

#[test]
fn a_routed_door_reads_as_routing_on_the_screen() {
    let mut sim = wired();
    let mut held = Buttons::default();
    tick(
        &mut sim,
        &mut held,
        &[
            Command::Pivot {
                target: PivotTarget::Bridge,
            },
            Command::Uplink {
                kind: ActionKind::Door,
            },
        ],
    );
    let view = deck(&sim);
    let d1 = view
        .devices
        .iter()
        .find(|d| d.node == "door-d1")
        .expect("door-d1");
    assert!(
        d1.status.starts_with("ROUTING") || d1.status.starts_with("OPEN"),
        "{}",
        d1.status
    );
}

#[test]
fn a_door_passage_becomes_a_fix_that_blurs_with_time() {
    let mut sim = wired();
    let mut held = Buttons::default();
    // Pivot, walk right, keep routing the door until through into the hall.
    for t in 0..(30 * 60) {
        let mut cmds = Vec::new();
        if t == 0 {
            cmds.push(Command::Pivot {
                target: PivotTarget::Bridge,
            });
            cmds.push(Command::SetButton {
                button: Button::Right,
                down: true,
            });
        }
        if t % 30 == 10 {
            cmds.push(Command::Uplink {
                kind: ActionKind::Door,
            });
        }
        tick(&mut sim, &mut held, &cmds);
        if sim.state().intel.infiltrator.is_some() {
            break;
        }
    }
    let first = deck(&sim).infiltrator.expect("the deck saw the passage");
    // Stop and wait out of every camera's sight.
    tick(
        &mut sim,
        &mut held,
        &[Command::SetButton {
            button: Button::Right,
            down: false,
        }],
    );
    for _ in 0..120 {
        tick(&mut sim, &mut held, &[]);
    }
    let later = deck(&sim).infiltrator.expect("still known");
    assert!(later.radius >= first.radius, "an old fix is a vaguer fix");
}

#[test]
fn the_office_pc_shows_building_systems_but_no_cameras_and_cannot_act() {
    let sim = wired();
    // Not at a console in the kitchen: interact does nothing console-ish.
    assert!(sim.console_in_reach().is_none());
    let view = sim
        .screen_view("office-pc", Seat::Infiltrator)
        .expect("the office has a PC");
    assert!(
        view.devices.iter().all(|d| !d.actionable),
        "look, don't touch"
    );
    assert!(
        view.devices.iter().all(|d| d.kind != ScreenKind::Camera),
        "the office PC has no camera feeds"
    );
    assert!(view.wiring);
}

#[test]
fn walking_to_the_office_pc_sitting_down_and_standing_up_again() {
    let mut sim = wired();
    let mut held = Buttons::default();
    // Walk right, routing whichever door is nearest, until at the PC.
    let mut arrived = false;
    for t in 0..(90 * 60) {
        let mut cmds = Vec::new();
        if t == 0 {
            cmds.push(Command::Pivot {
                target: PivotTarget::Bridge,
            });
            cmds.push(Command::SetButton {
                button: Button::Right,
                down: true,
            });
        }
        if t % 30 == 10 {
            cmds.push(Command::Uplink {
                kind: ActionKind::Door,
            });
        }
        tick(&mut sim, &mut held, &cmds);
        if sim.console_in_reach().is_some_and(|c| c.id == "office-pc") {
            arrived = true;
            break;
        }
    }
    assert!(arrived, "walked to the office PC");
    let events = tick(
        &mut sim,
        &mut held,
        &[
            Command::SetButton {
                button: Button::Right,
                down: false,
            },
            Command::Interact,
        ],
    );
    assert_eq!(sim.state().using_console.as_deref(), Some("office-pc"));
    assert!(events.contains(&Event::ConsoleUsed {
        console: "office-pc".into(),
        on: true
    }));

    // Seated: walking does nothing.
    let x = sim.state().player.x;
    tick(
        &mut sim,
        &mut held,
        &[Command::SetButton {
            button: Button::Right,
            down: true,
        }],
    );
    for _ in 0..30 {
        tick(&mut sim, &mut held, &[]);
    }
    assert_eq!(sim.state().player.x, x);

    // Interact again stands up, and the held key walks on.
    tick(&mut sim, &mut held, &[Command::Interact]);
    assert!(sim.state().using_console.is_none());
    for _ in 0..30 {
        tick(&mut sim, &mut held, &[]);
    }
    assert!(sim.state().player.x > x);
}

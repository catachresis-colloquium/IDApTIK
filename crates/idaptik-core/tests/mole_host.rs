//! The scenario host's order gates and snapshot lifecycle, separate from
//! the standalone companion simulation's tests.

use idaptik_core::companion::{MoleEvent, MoleState, moletaire};
use idaptik_core::scenario::command::{Command, MoleOp, TickInput};
use idaptik_core::scenario::constants as c;
use idaptik_core::scenario::{
    Event, GhostLobbySim, MoleDenyReason, RunConfig, RuntimeSnapshot, ghost_lobby, pump_house,
};

fn sim() -> GhostLobbySim {
    let mut sim = GhostLobbySim::new(pump_house(), RunConfig::standard(), 11).unwrap();
    sim.drain_events();
    sim
}

fn order(sim: &mut GhostLobbySim, op: MoleOp) -> Vec<Event> {
    sim.tick(&TickInput {
        immediates: vec![Command::Mole { op }],
        ..TickInput::default()
    })
}

fn restore(snapshot: RuntimeSnapshot) -> GhostLobbySim {
    GhostLobbySim::restore(snapshot.definition.clone(), snapshot).unwrap()
}

#[test]
fn every_order_on_a_floor_without_a_mole_is_explicitly_denied() {
    for op in [
        MoleOp::Dig,
        MoleOp::Sabotage,
        MoleOp::Synth { index: 0 },
        MoleOp::MoveTo {
            x: 400.0,
            underground: true,
        },
    ] {
        let mut sim = GhostLobbySim::new(ghost_lobby(), RunConfig::standard(), 11).unwrap();
        sim.drain_events();
        let events = order(&mut sim, op);
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, Event::MoleDenied { .. }))
                .collect::<Vec<_>>(),
            vec![&Event::MoleDenied {
                reason: MoleDenyReason::NotHosted
            }]
        );
        assert!(sim.state().mole.is_none());
        assert!(sim.state().diversion.is_none());
    }
}

#[test]
fn digging_and_sabotage_require_strictly_more_than_the_minimum_depth() {
    let minimum = moletaire().tuning.dig_min_depth;
    let initial = sim().snapshot();
    for op in [MoleOp::Dig, MoleOp::Sabotage] {
        for (depth, allowed) in [(0.0, false), (minimum, false), (minimum + 1e-6, true)] {
            let mut snapshot = initial.clone();
            let mole = &mut snapshot.state.mole.as_mut().unwrap().state;
            mole.depth = depth;
            mole.state = MoleState::Idle;
            mole.target_x = None;
            mole.target_depth = None;
            let mut sim = restore(snapshot);
            let events = order(&mut sim, op);
            let denied: Vec<_> = events
                .iter()
                .filter(|e| matches!(e, Event::MoleDenied { .. }))
                .collect();
            if allowed {
                assert!(denied.is_empty(), "{op:?} at {depth}: {events:?}");
                let expected = match op {
                    MoleOp::Dig => MoleState::DiggingTrap,
                    _ => MoleState::SabotagingCable,
                };
                assert_eq!(sim.state().mole.as_ref().unwrap().state.state, expected);
            } else {
                assert_eq!(
                    denied,
                    vec![&Event::MoleDenied {
                        reason: MoleDenyReason::Unavailable
                    }]
                );
            }
        }
    }
}

#[test]
fn synth_denials_and_successes_are_checked_per_order_at_the_fitted_limit() {
    let initial = sim().snapshot();
    let level = initial
        .state
        .mole
        .as_ref()
        .unwrap()
        .state
        .coprocessors
        .audio_synthesiser;
    let count = moletaire().coprocessors.audio_sound_count(level);
    assert!((1..=255).contains(&count));
    for (index, allowed) in [
        (0, true),
        (count as u8 - 1, true),
        (count as u8, false),
        (255, false),
    ] {
        let mut sim = restore(initial.clone());
        let events = order(&mut sim, MoleOp::Synth { index });
        if allowed {
            assert!(events.iter().any(|e| matches!(
                e,
                Event::Mole {
                    mole: MoleEvent::SynthSoundPlayed { .. }
                }
            )));
            assert!(!events.iter().any(|e| matches!(e, Event::MoleDenied { .. })));
            let diversion = sim.state().diversion.as_ref().expect("canonical diversion");
            assert_eq!(diversion.intensity, c::MOLE_SYNTH_NOISE);
            assert!(diversion.t > 0.0 && diversion.t <= c::MOLE_SYNTH_DURATION);
        } else {
            assert_eq!(
                events
                    .iter()
                    .filter(|e| matches!(e, Event::MoleDenied { .. }))
                    .collect::<Vec<_>>(),
                vec![&Event::MoleDenied {
                    reason: MoleDenyReason::Unavailable
                }]
            );
            assert!(!events.iter().any(|e| matches!(
                e,
                Event::Mole {
                    mole: MoleEvent::SynthSoundPlayed { .. }
                }
            )));
            assert!(sim.state().diversion.is_none());
        }
    }
}

#[test]
fn an_unupgraded_synth_cannot_create_a_diversion() {
    let mut def = pump_house();
    def.mole.as_mut().unwrap().upgrades.clear();
    let mut sim = GhostLobbySim::new(def, RunConfig::standard(), 11).unwrap();
    sim.drain_events();
    assert!(
        order(&mut sim, MoleOp::Synth { index: 0 }).contains(&Event::MoleDenied {
            reason: MoleDenyReason::Unavailable
        })
    );
    assert!(sim.state().diversion.is_none());
}

#[test]
fn dead_and_missing_restored_companions_have_distinct_denials() {
    for (missing, reason) in [
        (false, MoleDenyReason::Gone),
        (true, MoleDenyReason::NotHosted),
    ] {
        let mut snapshot = sim().snapshot();
        if missing {
            snapshot.state.mole = None;
        } else {
            let mole = &mut snapshot.state.mole.as_mut().unwrap().state;
            mole.alive = false;
            mole.state = MoleState::Dead;
        }
        let mut sim = restore(snapshot);
        let events = order(&mut sim, MoleOp::Synth { index: 0 });
        assert!(events.contains(&Event::MoleDenied { reason }), "{events:?}");
        assert!(sim.state().diversion.is_none());
    }
}

#[test]
fn paused_and_ended_runs_ignore_orders_without_mutating_the_snapshot() {
    for paused in [true, false] {
        let mut snapshot = sim().snapshot();
        snapshot.paused = paused;
        snapshot.state.ended = !paused;
        let mut sim = restore(snapshot);
        let before = sim.snapshot();
        for op in [
            MoleOp::Dig,
            MoleOp::Sabotage,
            MoleOp::Synth { index: 0 },
            MoleOp::MoveTo {
                x: 900.0,
                underground: true,
            },
        ] {
            assert!(order(&mut sim, op).is_empty());
            assert_eq!(sim.snapshot(), before);
        }
    }
}

#[test]
fn resetting_clears_diversions_and_rebuilds_the_companion_from_the_seed() {
    let mut sim = sim();
    order(&mut sim, MoleOp::Synth { index: 0 });
    order(
        &mut sim,
        MoleOp::MoveTo {
            x: 900.0,
            underground: false,
        },
    );
    assert!(sim.state().diversion.is_some());
    sim.reset(11);
    let fresh = GhostLobbySim::new(pump_house(), RunConfig::standard(), 11).unwrap();
    assert_eq!(sim.snapshot(), fresh.snapshot());
}

#[test]
fn hosting_a_companion_does_not_advance_the_runs_rng_stream() {
    let with = sim().snapshot();
    let mut def = pump_house();
    def.mole = None;
    let without = GhostLobbySim::new(def, RunConfig::standard(), 11)
        .unwrap()
        .snapshot();
    assert_eq!(with.rng, without.rng);
    assert_eq!(with.state.note, without.state.note);
    assert_eq!(with.state.usb, without.state.usb);
}

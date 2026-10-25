//! Infrastructure consequences reach actors only as observations (ADR-0013).
//!
//! The contract these tests hold:
//!
//! * a floor without infrastructure behaves exactly as before — no new state
//!   keys in its snapshots, no new events, and the mole verb denies cleanly;
//! * on the challenge-set floor, water, shorts and alarms become canonical
//!   state and canonical events, and Billy reacts to what his own senses
//!   carry — pulled toward the *noise*, never toward the player's hidden
//!   position;
//! * the whole subsystem survives snapshot/restore mid-flood, companion RNG
//!   included, so replay and resynchronisation carry it.

use idaptik_core::scenario::command::{
    Button, Buttons, Command, Edge, MoleOp, NetNodeIndex, PivotTarget, TickInput,
};
use idaptik_core::scenario::common::BillyMode;
use idaptik_core::scenario::event::Event;
use idaptik_core::scenario::floor_graph::{floor_graph, valve_node_id};
use idaptik_core::scenario::infra::ObservationKind;
use idaptik_core::scenario::{GhostLobbySim, HALL_SUMP, RunConfig, ghost_lobby, pump_house};

fn hold(buttons: &[Button]) -> TickInput {
    let mut b = Buttons::default();
    for x in buttons {
        b.set(*x, true);
    }
    TickInput {
        buttons: b,
        ..TickInput::default()
    }
}

fn cmds(commands: &[Command]) -> TickInput {
    TickInput {
        immediates: commands.to_vec(),
        ..TickInput::default()
    }
}

fn press_interact() -> TickInput {
    let mut b = Buttons::default();
    b.set(Button::Interact, true);
    TickInput {
        buttons: b,
        edges: vec![Edge::InteractPress],
        ..TickInput::default()
    }
}

/// A pump-house run with the crisis forced and Billy on his feet assessing —
/// the state every observation test starts from. The player is left on the
/// roof, out of the building, so anything Billy does below is a reaction to
/// the building alone.
fn assessing_pump_house(seed: u32) -> (GhostLobbySim, Vec<Event>) {
    let mut sim = GhostLobbySim::new(pump_house(), RunConfig::standard(), seed).expect("valid");
    let mut log = sim.drain_events();
    log.extend(sim.tick(&cmds(&[Command::ForceCrisis])));
    for _ in 0..1200 {
        log.extend(sim.tick(&TickInput::default()));
        if sim.state().billy.mode == BillyMode::Assess {
            return (sim, log);
        }
    }
    panic!("Billy never reached Assess");
}

/// Pivot onto the bridge and hack the sump valve — the remote release.
fn hack_the_valve(sim: &mut GhostLobbySim, log: &mut Vec<Event>) {
    let def = sim.definition().clone();
    let index = floor_graph(&def)
        .nodes
        .iter()
        .position(|n| n.id == valve_node_id(HALL_SUMP))
        .expect("the valve node is derived") as u32;
    log.extend(sim.tick(&cmds(&[
        Command::Pivot {
            target: PivotTarget::Bridge,
        },
        Command::NetHack {
            node: NetNodeIndex(index),
        },
    ])));
}

fn wait_for(
    sim: &mut GhostLobbySim,
    log: &mut Vec<Event>,
    budget: usize,
    pred: impl Fn(&Vec<Event>) -> bool,
) -> bool {
    for _ in 0..budget {
        log.extend(sim.tick(&TickInput::default()));
        if pred(log) {
            return true;
        }
    }
    false
}

fn saw(log: &[Event], target: &Event) -> bool {
    log.contains(target)
}

//## Floors without infrastructure are untouched

#[test]
fn an_infra_free_floor_carries_no_infra_state_and_denies_the_mole_verb() {
    let mut sim = GhostLobbySim::new(ghost_lobby(), RunConfig::standard(), 123456).expect("valid");
    let mut log = sim.drain_events();
    // The mole verb on a floor with no host is a typed denial, not a panic
    // and not a silent swallow.
    log.extend(
        sim.tick(&cmds(&[Command::Mole {
            op: MoleOp::Dig,
        }])),
    );
    assert!(
        saw(
            &log,
            &Event::MoleDenied {
                reason: idaptik_core::scenario::common::MoleDenyReason::NotHosted,
            }
        ),
        "the unhosted floor must deny the mole verb: {log:?}"
    );
    for _ in 0..900 {
        log.extend(sim.tick(&TickInput::default()));
    }
    // No infrastructure events ever fire on the canonical floor.
    assert!(
        !log.iter().any(|e| matches!(
            e,
            Event::WaterReleased { .. }
                | Event::InfraShorted { .. }
                | Event::AlarmChanged { .. }
                | Event::GuardObserved { .. }
        )),
        "the canonical floor invented infrastructure events"
    );
    // The snapshot stays byte-clean: the optional tail fields are skipped.
    let value = serde_json::to_value(sim.snapshot()).expect("serialises");
    let state = value.get("state").and_then(|s| s.as_object()).expect("state");
    for key in ["infra", "mole", "diversion", "guard_noticed"] {
        assert!(
            !state.contains_key(key),
            "an infra-free snapshot must not carry {key:?}"
        );
    }
}

//## The observation gate

#[test]
fn a_dry_pump_house_offers_nothing_to_observe() {
    let (sim, _log) = assessing_pump_house(7);
    let obs = sim.actor_observations(sim.state().billy.x);
    assert!(obs.is_empty(), "a dry, quiet floor invented observations: {obs:?}");
}

#[test]
fn the_flood_reaches_billy_as_observations_and_pulls_him_to_the_noise() {
    let (mut sim, mut log) = assessing_pump_house(7);
    let player_x = sim.state().player.x;
    hack_the_valve(&mut sim, &mut log);
    assert!(
        wait_for(
            &mut sim,
            &mut log,
            60,
            |log| saw(
                log,
                &Event::WaterReleased {
                    zone: HALL_SUMP.into(),
                    room: "hall".into(),
                    cause: idaptik_core::scenario::infra::ReleaseCause::Valve,
                }
            )
        ),
        "the hacked valve did not release: {log:?}"
    );
    // The alarm trips on the rising water (level 0.25 of a 24 s fill ≈ 6 s).
    assert!(
        wait_for(
            &mut sim,
            &mut log,
            600,
            |log| saw(
                log,
                &Event::AlarmChanged {
                    alarm: "flood-alarm".into(),
                    room: "office".into(),
                    sounding: true,
                }
            )
        ),
        "the flood alarm never tripped: {log:?}"
    );
    // Give the Assess loop a beat to hear it and commit.
    for _ in 0..120 {
        log.extend(sim.tick(&TickInput::default()));
    }

    // Player-visible evidence: the guard announced what he perceived...
    assert!(
        log.iter()
            .any(|e| matches!(e, Event::GuardObserved { .. })),
        "no GuardObserved record: {log:?}"
    );
    // ...and his FSM committed to the noise, not to the player. The player
    // has been parked on the roof at the west end the whole time; the alarm
    // sits at the control office's centre, far east of Billy's patrol band.
    let billy = &sim.state().billy;
    assert!(
        matches!(
            billy.mode,
            BillyMode::Assess | BillyMode::Investigate | BillyMode::Pursue
        ),
        "Billy ignored the alarm: {:?}",
        billy.mode
    );
    assert!(
        (billy.last_known_x - 652.5).abs() < 1.0,
        "Billy was not pulled to the alarm's room centre: {}",
        billy.last_known_x
    );
    assert!(
        (billy.last_known_x - player_x).abs() > 300.0,
        "Billy's pull tracked the player, not the observation"
    );

    // The observation list itself carries only building facts.
    let obs = sim.actor_observations(billy.x);
    assert!(
        obs.iter().any(|o| o.kind == ObservationKind::Alarm),
        "the sounding alarm is not observable: {obs:?}"
    );
    for o in &obs {
        assert!(
            (o.x - player_x).abs() > 100.0,
            "an observation leaked the player's position: {o:?}"
        );
    }

    // The short follows the flood: the room's electrics die through the
    // ordinary power-loss path, as canonical events.
    assert!(
        wait_for(&mut sim, &mut log, 900, |log| log
            .iter()
            .any(|e| matches!(e, Event::InfraShorted { zone, .. } if zone == HALL_SUMP))),
        "the flooded hall never shorted: {log:?}"
    );
}

#[test]
fn the_hosted_companion_denies_orders_it_cannot_take() {
    let mut sim = GhostLobbySim::new(pump_house(), RunConfig::standard(), 11).expect("valid");
    let mut log = sim.drain_events();
    // Fresh spawn: the mole is at the surface, so a dig order is refused...
    log.extend(
        sim.tick(&cmds(&[Command::Mole {
            op: MoleOp::Dig,
        }])),
    );
    assert!(
        saw(
            &log,
            &Event::MoleDenied {
                reason: idaptik_core::scenario::common::MoleDenyReason::Unavailable,
            }
        ),
        "a surface dig must be refused: {log:?}"
    );
    // ...a sound index the fitted bay does not carry is refused...
    log.extend(
        sim.tick(&cmds(&[Command::Mole {
            op: MoleOp::Synth { index: 9 },
        }])),
    );
    // ...and a sound it does carry plays, landing as a typed companion event
    // plus a diversion in canonical state.
    log.extend(
        sim.tick(&cmds(&[Command::Mole {
            op: MoleOp::Synth { index: 0 },
        }])),
    );
    assert!(
        log.iter().any(|e| matches!(
            e,
            Event::Mole {
                mole: idaptik_core::companion::MoleEvent::SynthSoundPlayed { .. }
            }
        )),
        "the fitted synth never played: {log:?}"
    );
    assert!(
        sim.state().diversion.is_some(),
        "the decoy is not canonical state"
    );
}

//## Replay and resynchronisation carry the subsystems

#[test]
fn a_mid_flood_snapshot_restores_and_replays_identically() {
    let (mut sim, mut log) = assessing_pump_house(5);
    hack_the_valve(&mut sim, &mut log);
    // Advance past the alarm and well into the fill (the short fires at
    // level 0.55), with the companion tunnelling, then fork the run through
    // a snapshot.
    assert!(
        wait_for(&mut sim, &mut log, 900, |log| log
            .iter()
            .any(|e| matches!(e, Event::InfraShorted { zone, .. } if zone == HALL_SUMP))),
        "the hall never flooded far enough to short: {log:?}"
    );
    log.extend(
        sim.tick(&cmds(&[Command::Mole {
            op: MoleOp::MoveTo {
                x: 520.0,
                underground: true,
            },
        }])),
    );
    for _ in 0..60 {
        log.extend(sim.tick(&TickInput::default()));
    }

    let snapshot = sim.snapshot();
    let mut restored =
        GhostLobbySim::restore(pump_house(), snapshot).expect("the snapshot restores");
    assert!(
        restored.state().mole.is_some(),
        "the restored run lost the hosted companion"
    );

    // Both runs continue from the fork; their events and canonical state must
    // agree tick for tick — the companion's RNG stream resumes mid-sequence.
    let mut a = Vec::new();
    let mut b = Vec::new();
    for _ in 0..240 {
        a.extend(sim.tick(&TickInput::default()));
        b.extend(restored.tick(&TickInput::default()));
    }
    assert_eq!(
        serde_json::to_value(&a).unwrap(),
        serde_json::to_value(&b).unwrap(),
        "the restored run diverged from the original"
    );
    assert_eq!(sim.state().infra, restored.state().infra);
    assert_eq!(sim.state().mole, restored.state().mole);
}

//## Wading: the flood is a fact for every body, and it is loud

#[test]
fn wading_through_the_flood_slows_and_betrayes_the_wader() {
    // Two identical runs, same seed and inputs; one floods the hall before
    // the walk, one does not. Both enter the pump hall the legitimate way —
    // rope down, picked front door, and a routed hold on D1 (the closed
    // interior doors stop everyone; the network opens the one nearest the
    // body, and the walk blocks at the door until the hold lands). The
    // flooded walk must cover less ground and register more noise — from
    // canonical state, never a render quantity.
    fn enter_hall_and_walk_west(flood: bool) -> (f64, f64) {
        let mut sim = GhostLobbySim::new(pump_house(), RunConfig::standard(), 21).expect("valid");
        let mut log = sim.drain_events();
        // Tick one: pivot onto the bridge — the wet run hacks the derived
        // valve node in the same breath (the remote release); the dry run
        // carries the identical pivot so both twins hold the same reach.
        let def = sim.definition().clone();
        let index = floor_graph(&def)
            .nodes
            .iter()
            .position(|n| n.id == valve_node_id(HALL_SUMP))
            .expect("valve node") as u32;
        let mut opening = vec![Command::Pivot {
            target: PivotTarget::Bridge,
        }];
        if flood {
            opening.push(Command::NetHack {
                node: NetNodeIndex(index),
            });
        }
        log.extend(sim.tick(&cmds(&opening)));
        // Roof walk west to the rope, and down to the street.
        let rope_x = sim.definition().exterior.as_ref().unwrap().rope.top_x;
        for _ in 0..600 {
            let cx = sim.state().player.x + sim.definition().player.w / 2.0;
            if (cx - rope_x).abs() <= 4.0 {
                break;
            }
            log.extend(sim.tick(&hold(&[Button::Left])));
        }
        log.extend(sim.tick(&press_interact()));
        assert!(
            log.iter().any(|e| matches!(e, Event::RopeTaken { down: true })),
            "the rope never took: {log:?}"
        );
        let mut landed = false;
        for _ in 0..400 {
            log.extend(sim.tick(&TickInput::default()));
            if sim.state().player.place == idaptik_core::scenario::state::Place::Floor
                && sim.infiltrator_outside()
            {
                landed = true;
                break;
            }
        }
        assert!(landed, "never reached the street");
        // Approach the locked front door and pick it (the rooftop suite's
        // proven sequence: press against the door, then hold interact).
        for _ in 0..240 {
            log.extend(sim.tick(&hold(&[Button::Right])));
        }
        let mut picked = false;
        for _ in 0..330 {
            log.extend(sim.tick(&hold(&[Button::Interact])));
            if log.iter().any(|e| matches!(e, Event::FrontDoorPicked)) {
                picked = true;
                break;
            }
        }
        assert!(picked, "the lock never gave");
        let mut inside = false;
        for _ in 0..120 {
            log.extend(sim.tick(&hold(&[Button::Right])));
            if !sim.infiltrator_outside() {
                inside = true;
                break;
            }
        }
        assert!(inside, "never got inside");
        // East toward the hall; at the kitchen's east end order the routed
        // hold on the nearest door (D1) and keep walking into it.
        let mut uplinked = false;
        let mut arrived = false;
        for _ in 0..900 {
            let cx = sim.state().player.x + sim.definition().player.w / 2.0;
            if (cx - 450.0).abs() <= 4.0 {
                arrived = true;
                break;
            }
            if !uplinked && cx >= 240.0 {
                uplinked = true;
                log.extend(sim.tick(&cmds(&[Command::Uplink {
                    kind: idaptik_core::scenario::tuning::ActionKind::Door,
                }])));
                continue;
            }
            log.extend(sim.tick(&hold(&[Button::Right])));
        }
        assert!(uplinked && arrived, "the hall never opened to the routed hold");
        // The measured window: 60 ticks due west from the sump wheel —
        // short enough that the wet run never reaches the door behind it
        // and the dry run's extra ground is the flood's own doing.
        let start_x = sim.state().player.x;
        let mut loudest = 0.0f64;
        for _ in 0..60 {
            log.extend(sim.tick(&hold(&[Button::Left])));
            loudest = loudest.max(sim.state().player.noise);
        }
        let _ = log;
        (start_x - sim.state().player.x, loudest)
    }

    let (dry_dx, dry_noise) = enter_hall_and_walk_west(false);
    let (wet_dx, wet_noise) = enter_hall_and_walk_west(true);
    assert!(dry_dx > 100.0, "the dry walk covered nothing: {dry_dx}");
    assert!(
        wet_dx < dry_dx - 10.0,
        "the flood did not slow the wader: wet {wet_dx} vs dry {dry_dx}"
    );
    assert!(
        wet_noise > dry_noise,
        "wading was not louder: wet {wet_noise} vs dry {dry_noise}"
    );
    assert!(
        wet_noise >= idaptik_core::scenario::constants::WATER_WADE_NOISE - 0.05,
        "the wade never reached the splash floor: {wet_noise}"
    );
}

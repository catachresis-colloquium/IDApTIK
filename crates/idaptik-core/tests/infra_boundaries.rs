//! Unit coverage of the infrastructure's canonical state and perception gates.

use idaptik_core::netsim::Effect;
use idaptik_core::scenario::agents::apply_effects;
use idaptik_core::scenario::constants as c;
use idaptik_core::scenario::floor_graph::{light_node_id, pump_node_id, valve_node_id};
use idaptik_core::scenario::infra::{
    Hearing, InfraState, NoiseSource, Observation, ObservationKind, ReleaseCause, WaterZoneState,
    noise_sources, observations, room_centre, room_id_at, wade_factor, wade_noise, water_at,
};
use idaptik_core::scenario::state::RuntimeState;
use idaptik_core::scenario::{
    Event, GhostLobbySim, HALL_SUMP, RunConfig, ScenarioDefinition, pump_house,
};

fn fixture() -> (ScenarioDefinition, RuntimeState) {
    let def = pump_house();
    let sim = GhostLobbySim::new(def.clone(), RunConfig::standard(), 7).unwrap();
    (def, sim.state().clone())
}

fn flood(state: &mut RuntimeState, level: f64) {
    let infra = state.infra.as_mut().unwrap();
    infra.release(HALL_SUMP, ReleaseCause::Valve);
    infra.zones.get_mut(HALL_SUMP).unwrap().level = level;
}

fn at(def: &ScenarioDefinition, room: &str) -> f64 {
    room_centre(def, room) - def.world.room_offset
}

#[test]
fn initial_infrastructure_is_dry_silent_and_independent() {
    let (def, _) = fixture();
    let infra = def.infra.as_ref().unwrap();
    let mut a = InfraState::initial(infra);
    let b = InfraState::initial(infra);
    assert_eq!(a.zones.len(), infra.water_zones.len());
    assert_eq!(a.alarms.len(), infra.alarms.len());
    assert!(a.zones.values().all(|z| z == &WaterZoneState::default()));
    assert!(a.alarms.values().all(|sounding| !sounding));
    assert!(a.release(HALL_SUMP, ReleaseCause::Valve));
    assert!(!b.zones[HALL_SUMP].released);
}

#[test]
fn rereleasing_a_zone_preserves_its_entire_progress() {
    for first in [ReleaseCause::Valve, ReleaseCause::PumpFailure] {
        let (_, mut state) = fixture();
        let infra = state.infra.as_mut().unwrap();
        assert!(infra.release(HALL_SUMP, first));
        let zone = infra.zones.get_mut(HALL_SUMP).unwrap();
        zone.level = 0.7;
        zone.shorted = true;
        zone.short_flash = 0.25;
        let before = infra.clone();
        assert!(!infra.release(HALL_SUMP, ReleaseCause::PumpFailure));
        assert!(!infra.release("unknown", ReleaseCause::Valve));
        assert_eq!(*infra, before);
    }
}

#[test]
fn water_depth_uses_only_released_positive_zones_in_the_actors_room() {
    let (mut def, initial) = fixture();
    let infra = def.infra.as_mut().unwrap();
    let mut second = infra.water_zones[0].clone();
    second.id = "second-sump".into();
    infra.water_zones.push(second);
    for (released, level, expected) in [
        (false, 0.9, Some(0.3)),
        (true, 0.0, Some(0.3)),
        (true, 0.2, Some(0.3)),
        (true, 0.8, Some(0.8)),
    ] {
        let mut state = initial.clone();
        flood(&mut state, 0.3);
        state.infra.as_mut().unwrap().zones.insert(
            "second-sump".into(),
            WaterZoneState {
                released,
                level,
                ..WaterZoneState::default()
            },
        );
        assert_eq!(water_at(&def, &state, at(&def, "hall")), expected);
        assert_eq!(water_at(&def, &state, at(&def, "office")), None);
    }
    // A declared zone with missing runtime data contributes no water.
    assert_eq!(water_at(&def, &initial, at(&def, "hall")), None);
}

#[test]
fn wading_begins_at_the_threshold_and_applies_only_in_the_wet_room() {
    let (def, initial) = fixture();
    for (level, speed, noise) in [
        (0.0, 1.0, 0.0),
        (c::WATER_WADE_AT - 1e-9, 1.0, 0.0),
        (c::WATER_WADE_AT, c::WATER_WADE_SPEED, c::WATER_WADE_NOISE),
        (1.0, c::WATER_WADE_SPEED, c::WATER_WADE_NOISE),
    ] {
        let mut state = initial.clone();
        flood(&mut state, level);
        assert_eq!(
            wade_factor(&def, &state, at(&def, "hall")),
            speed,
            "{level}"
        );
        assert_eq!(wade_noise(&def, &state, at(&def, "hall")), noise, "{level}");
        assert_eq!(wade_factor(&def, &state, at(&def, "office")), 1.0);
        assert_eq!(wade_noise(&def, &state, at(&def, "office")), 0.0);
    }
}

#[test]
fn room_lookup_honours_offset_half_open_spans_and_empty_floors() {
    let (mut def, _) = fixture();
    let room = def.rooms[0].clone();
    let offset = def.world.room_offset;
    assert_eq!(room_id_at(&def, room.x - offset), Some(room.id.as_str()));
    assert_eq!(
        room_id_at(&def, room.x + room.w - offset),
        Some(def.rooms[1].id.as_str())
    );
    assert_eq!(
        room_id_at(&def, -1000.0),
        Some(def.rooms.last().unwrap().id.as_str())
    );
    assert_eq!(room_centre(&def, "missing"), def.world.width / 2.0);
    def.rooms.clear();
    assert_eq!(room_id_at(&def, 0.0), None);
}

#[test]
fn noise_stops_when_filling_and_sparking_finish_but_alarms_continue() {
    let (def, mut state) = fixture();
    flood(&mut state, 0.5);
    let infra = state.infra.as_mut().unwrap();
    infra.zones.get_mut(HALL_SUMP).unwrap().short_flash = 0.1;
    infra.alarms.insert("flood-alarm".into(), true);
    let sources = noise_sources(&def, &state);
    assert_eq!(
        sources.iter().map(|s| s.kind).collect::<Vec<_>>(),
        [
            ObservationKind::UnusualNoise,
            ObservationKind::EquipmentFailure,
            ObservationKind::Alarm,
        ]
    );
    assert_eq!(sources[0].intensity, c::WATER_SPLASH_NOISE);
    assert_eq!(sources[1].intensity, c::SHORT_FLASH_NOISE);
    assert_eq!(sources[2].room.as_str(), "office");
    let zone = state
        .infra
        .as_mut()
        .unwrap()
        .zones
        .get_mut(HALL_SUMP)
        .unwrap();
    zone.level = 1.0;
    zone.short_flash = 0.0;
    assert_eq!(noise_sources(&def, &state), vec![sources[2].clone()]);
}

#[test]
fn hearing_requires_strictly_nearer_and_strictly_louder_sources() {
    let (def, state) = fixture();
    let actor_x = at(&def, "hall");
    let hearing = Hearing {
        dist: 100.0,
        threshold: 0.5,
    };
    for (offset, intensity, heard) in [
        (99.0, 0.6, true),
        (-99.0, 0.6, true),
        (100.0, 0.6, false),
        (-100.0, 0.6, false),
        (101.0, 0.6, false),
        (0.0, 0.5, false),
        (0.0, 0.49, false),
    ] {
        let source = NoiseSource {
            x: actor_x + offset,
            room: "hall".into(),
            intensity,
            kind: ObservationKind::UnusualNoise,
        };
        let obs = observations(
            &def,
            &state,
            actor_x,
            hearing,
            std::slice::from_ref(&source),
        );
        assert_eq!(
            obs.len(),
            usize::from(heard),
            "{offset}, {intensity}: {obs:?}"
        );
        if heard {
            assert_eq!(
                obs[0],
                Observation {
                    kind: source.kind,
                    room: source.room,
                    x: source.x,
                    intensity,
                }
            );
        }
    }
}

#[test]
fn alarms_bypass_distance_but_still_require_the_actors_hearing_threshold() {
    let (def, mut state) = fixture();
    state
        .infra
        .as_mut()
        .unwrap()
        .alarms
        .insert("flood-alarm".into(), true);
    for (threshold, heard) in [(c::ALARM_NOISE - 1e-9, true), (c::ALARM_NOISE, false)] {
        let obs = observations(
            &def,
            &state,
            at(&def, "kitchen"),
            Hearing {
                dist: 0.0,
                threshold,
            },
            &[],
        );
        assert_eq!(obs.len(), usize::from(heard));
        if heard {
            assert_eq!(obs[0].kind, ObservationKind::Alarm);
            assert_eq!(obs[0].room.as_str(), "office");
            assert_eq!(obs[0].x, room_centre(&def, "office"));
        }
    }
}

#[test]
fn visible_changes_are_local_and_do_not_require_hearing() {
    let (mut def, mut state) = fixture();
    def.rooms
        .iter_mut()
        .find(|r| r.id.as_str() == "hall")
        .unwrap()
        .lit = true;
    state.dead_nodes.insert(light_node_id("hall"));
    flood(&mut state, 0.4);
    state
        .infra
        .as_mut()
        .unwrap()
        .zones
        .get_mut(HALL_SUMP)
        .unwrap()
        .short_flash = 0.1;
    let deaf = Hearing {
        dist: 0.0,
        threshold: 1.0,
    };
    let obs = observations(&def, &state, at(&def, "hall"), deaf, &[]);
    assert_eq!(
        obs.iter().map(|o| o.kind).collect::<Vec<_>>(),
        [
            ObservationKind::Darkness,
            ObservationKind::Water,
            ObservationKind::EquipmentFailure,
        ]
    );
    assert_eq!(obs[1].intensity, 0.4);
    assert!(observations(&def, &state, at(&def, "office"), deaf, &[]).is_empty());
    def.rooms
        .iter_mut()
        .find(|r| r.id.as_str() == "hall")
        .unwrap()
        .lit = false;
    assert!(
        !observations(&def, &state, at(&def, "hall"), deaf, &[])
            .iter()
            .any(|o| o.kind == ObservationKind::Darkness)
    );
}

#[test]
fn absent_infrastructure_and_empty_rooms_are_total() {
    let (def, mut state) = fixture();
    flood(&mut state, 0.9);
    for (has_def, has_state) in [(false, true), (true, false), (false, false)] {
        let mut def = def.clone();
        let mut state = state.clone();
        if !has_def {
            def.infra = None;
        }
        if !has_state {
            state.infra = None;
        }
        assert_eq!(water_at(&def, &state, at(&def, "hall")), None);
        assert!(noise_sources(&def, &state).is_empty());
        assert_eq!(wade_factor(&def, &state, at(&def, "hall")), 1.0);
        assert_eq!(wade_noise(&def, &state, at(&def, "hall")), 0.0);
    }
    let mut empty = def;
    empty.rooms.clear();
    assert!(
        observations(
            &empty,
            &state,
            0.0,
            Hearing {
                dist: 1000.0,
                threshold: 0.0
            },
            &[]
        )
        .is_empty()
    );
}

#[test]
fn notice_identity_is_kind_and_room_not_position_or_strength() {
    let observation = Observation {
        kind: ObservationKind::Water,
        room: "hall".into(),
        x: 400.0,
        intensity: 0.2,
    };
    let mut changed = observation.clone();
    changed.x = 410.0;
    changed.intensity = 0.8;
    assert_eq!(observation.notice_key(), changed.notice_key());
    changed.kind = ObservationKind::Alarm;
    assert_ne!(observation.notice_key(), changed.notice_key());
    changed.kind = observation.kind;
    changed.room = "office".into();
    assert_ne!(observation.notice_key(), changed.notice_key());
}

#[test]
fn only_fail_open_pump_power_loss_releases_water() {
    for (fail_open, node, releases) in [
        (true, pump_node_id(HALL_SUMP), true),
        (false, pump_node_id(HALL_SUMP), false),
        (true, valve_node_id(HALL_SUMP), false),
        (true, "pump-unknown".into(), false),
    ] {
        let (mut def, mut state) = fixture();
        def.infra.as_mut().unwrap().water_zones[0].fail_open = fail_open;
        let events = apply_effects(
            &def,
            &mut state,
            RunConfig::standard(),
            &[Effect::DevicePowerLost(node)],
        );
        assert_eq!(
            state.infra.as_ref().unwrap().zones[HALL_SUMP].released,
            releases
        );
        let expected = if releases {
            vec![Event::WaterReleased {
                zone: HALL_SUMP.into(),
                room: "hall".into(),
                cause: ReleaseCause::PumpFailure,
            }]
        } else {
            vec![]
        };
        assert_eq!(events, expected);
    }
}

#[test]
fn valve_and_pump_effect_order_keeps_one_release_and_the_first_cause() {
    let valve = Effect::WaterReleased(valve_node_id(HALL_SUMP));
    let pump = Effect::DevicePowerLost(pump_node_id(HALL_SUMP));
    for (effects, cause) in [
        (
            vec![valve.clone(), pump.clone(), valve.clone()],
            ReleaseCause::Valve,
        ),
        (
            vec![pump.clone(), valve.clone(), pump.clone()],
            ReleaseCause::PumpFailure,
        ),
    ] {
        let (def, mut state) = fixture();
        let events = apply_effects(&def, &mut state, RunConfig::standard(), &effects);
        assert_eq!(
            events,
            vec![Event::WaterReleased {
                zone: HALL_SUMP.into(),
                room: "hall".into(),
                cause
            }]
        );
        assert_eq!(
            state.infra.as_ref().unwrap().zones[HALL_SUMP].cause,
            Some(cause)
        );
        assert!(state.dead_nodes.contains(&pump_node_id(HALL_SUMP)));
    }
}

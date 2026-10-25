//! Authoring failures must retain their typed diagnostics at the load gate.

use idaptik_core::scenario::floor_graph::pump_node_id;
use idaptik_core::scenario::infra::AlarmTrip;
use idaptik_core::scenario::{HALL_SUMP, ScenarioDefinition, ValidationError, pump_house};

fn rejects(def: &ScenarioDefinition, id: &str, expected: ValidationError) {
    let report = def.validate();
    assert!(!report.passed(), "invalid content passed validation");
    let check = report
        .checks
        .iter()
        .find(|c| c.id == id)
        .expect("check exists");
    assert!(!check.passed, "{id}: {}", check.detail);
    assert_eq!(check.error, Some(expected.clone()));
    assert!(report.ok().unwrap_err().contains(&expected));
}

#[test]
fn fill_time_must_be_positive_and_finite() {
    for time in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut def = pump_house();
        def.infra.as_mut().unwrap().water_zones[0].fill_time = time;
        rejects(
            &def,
            "infra.zones.timing",
            ValidationError::InfraZoneTimingInvalid {
                zone: HALL_SUMP.into(),
            },
        );
    }
    let mut def = pump_house();
    def.infra.as_mut().unwrap().water_zones[0].fill_time = 0.001;
    assert!(def.validate().passed());
}

#[test]
fn duplicate_zone_and_alarm_ids_have_distinct_diagnostics() {
    let mut def = pump_house();
    let infra = def.infra.as_mut().unwrap();
    infra.water_zones.push(infra.water_zones[0].clone());
    infra.alarms.push(infra.alarms[0].clone());
    rejects(
        &def,
        "infra.zones.unique_ids",
        ValidationError::InfraDuplicateZone(HALL_SUMP.into()),
    );
    rejects(
        &def,
        "infra.alarms.unique_ids",
        ValidationError::InfraDuplicateAlarm("flood-alarm".into()),
    );
}

#[test]
fn manual_valve_placement_uses_a_half_open_room_span() {
    let original = pump_house();
    let room = original
        .rooms
        .iter()
        .find(|r| r.id.as_str() == "hall")
        .unwrap();
    for (x, valid) in [
        (room.x, true),
        (room.x + room.w - 1e-9, true),
        (room.x - 1e-9, false),
        (room.x + room.w, false),
        (f64::NAN, false),
        (f64::INFINITY, false),
    ] {
        let mut def = original.clone();
        def.infra.as_mut().unwrap().water_zones[0].valve_x = x;
        if valid {
            assert!(def.validate().passed(), "handle at {x}");
        } else {
            rejects(
                &def,
                "infra.zones.handle_in_room",
                ValidationError::InfraZoneHandleOffRoom {
                    zone: HALL_SUMP.into(),
                    room: "hall".into(),
                },
            );
        }
    }
}

#[test]
fn an_unknown_room_also_makes_its_zone_unaddressable() {
    let mut def = pump_house();
    def.infra.as_mut().unwrap().water_zones[0].room = "unwired-room".into();
    rejects(
        &def,
        "infra.zones.rooms_known",
        ValidationError::InfraZoneRoomMissing {
            zone: HALL_SUMP.into(),
            room: "unwired-room".into(),
        },
    );
    rejects(
        &def,
        "infra.zones.addressable",
        ValidationError::InfraZoneUnaddressable {
            zone: HALL_SUMP.into(),
            room: "unwired-room".into(),
        },
    );
}

#[test]
fn alarms_need_a_real_room_and_a_real_watched_node() {
    let mut def = pump_house();
    def.infra.as_mut().unwrap().alarms[0].room = "missing-room".into();
    rejects(
        &def,
        "infra.alarms.rooms_known",
        ValidationError::InfraAlarmRoomMissing {
            alarm: "flood-alarm".into(),
            room: "missing-room".into(),
        },
    );

    let mut def = pump_house();
    def.infra.as_mut().unwrap().alarms[1].trip = AlarmTrip::PowerLost {
        watch: "missing-pump".into(),
    };
    rejects(
        &def,
        "infra.alarms.references",
        ValidationError::InfraAlarmWatchMissing {
            alarm: "sump-tamper".into(),
            watch: "missing-pump".into(),
        },
    );
    def.infra.as_mut().unwrap().alarms[1].trip = AlarmTrip::PowerLost {
        watch: pump_node_id(HALL_SUMP),
    };
    assert!(
        def.validate().passed(),
        "derived pump nodes are valid alarm targets"
    );
}

#[test]
fn mole_spawns_include_world_endpoints_and_reject_nonfinite_positions() {
    let original = pump_house();
    for (x, valid) in [
        (0.0, true),
        (original.world.width, true),
        (-1e-9, false),
        (original.world.width + 1e-9, false),
        (f64::NAN, false),
        (f64::INFINITY, false),
        (f64::NEG_INFINITY, false),
    ] {
        let mut def = original.clone();
        def.mole.as_mut().unwrap().spawn_x = x;
        if valid {
            assert!(def.validate().passed(), "spawn at {x}");
        } else {
            rejects(
                &def,
                "mole.spawn_in_bounds",
                ValidationError::MoleSpawnOutOfBounds,
            );
        }
    }
}

#[test]
fn omitted_fail_open_and_mole_options_keep_the_documented_defaults() {
    let zone: idaptik_core::scenario::infra::WaterZoneDef =
        serde_json::from_str(r#"{"id":"sump","room":"hall","valve_x":470.0,"fill_time":24.0}"#)
            .unwrap();
    assert!(zone.fail_open);
    let mole: idaptik_core::scenario::MoleHostDef =
        serde_json::from_str(r#"{"spawn_x":0.0}"#).unwrap();
    assert!(!mole.underground);
    assert!(mole.upgrades.is_empty());
}

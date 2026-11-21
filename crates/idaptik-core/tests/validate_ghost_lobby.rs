//! The canonical definition validates clean, and each single-field corruption
//! produces exactly one typed `ValidationError`.

use idaptik_core::ScenarioDefinition;
use idaptik_core::ghost_lobby;
use idaptik_core::scenario::constants as c;
use idaptik_core::scenario::definition::{RoomDef, ValidationError};
use idaptik_core::scenario::ids::{CameraId, DoorId, ObjectiveId, RoomId};
use idaptik_core::scenario::tuning::{ActionKind, DifficultyId, DifficultyPreset};

#[test]
fn canonical_definition_validates_clean() {
    let def = ghost_lobby();
    assert!(def.validate().passed());
    assert_eq!(def.ok(), Ok(()));
}

/// Assert a mutated definition yields exactly the one expected error.
#[track_caller]
fn expect_one(def: &ScenarioDefinition, want: ValidationError) {
    match def.ok() {
        Ok(()) => panic!("expected {want:?}, but validation passed"),
        Err(errs) => {
            assert_eq!(errs.len(), 1, "expected exactly one error, got {errs:?}");
            assert_eq!(errs[0], want);
        }
    }
}

#[test]
fn empty_rooms_reports_only_empty_rooms() {
    let mut def = ghost_lobby();
    def.rooms.clear();
    expect_one(&def, ValidationError::EmptyRooms);
}

#[test]
fn duplicate_room_id() {
    let mut def = ghost_lobby();
    // Append a contiguous extra room that reuses an existing id.
    def.rooms.push(RoomDef {
        id: "kitchen".into(),
        name: "DUP".into(),
        x: 1260.0,
        w: 100.0,
        support: 0.5,
        ping_support_bonus: 0.0,
        sight_multiplier: 1.0,
        lit: false,
    });
    expect_one(
        &def,
        ValidationError::DuplicateRoomId(RoomId::from("kitchen")),
    );
}

#[test]
fn duplicate_door_id() {
    let mut def = ghost_lobby();
    def.doors[1].id = "D1".into();
    expect_one(&def, ValidationError::DuplicateDoorId(DoorId::from("D1")));
}

#[test]
fn rooms_not_contiguous() {
    let mut def = ghost_lobby();
    def.rooms[2].x += 5.0; // office slides right, opening a gap after the hall
    expect_one(
        &def,
        ValidationError::RoomsNotContiguous {
            gap_after: RoomId::from("hall"),
        },
    );
}

#[test]
fn door_off_room_boundary() {
    let mut def = ghost_lobby();
    def.doors[0].x = 999.0;
    expect_one(
        &def,
        ValidationError::DoorOffRoomBoundary(DoorId::from("D1")),
    );
}

#[test]
fn camera_range_inverted() {
    let mut def = ghost_lobby();
    def.cameras[0].range = (500.0, 100.0);
    expect_one(
        &def,
        ValidationError::CameraRangeInverted(CameraId::from("cam-hall")),
    );
}

#[test]
fn arrival_inverted() {
    let mut def = ghost_lobby();
    if let Some(p) = def.difficulty.get_mut(&DifficultyId::Standard) {
        p.arrival = (30.0, 10.0);
    }
    expect_one(
        &def,
        ValidationError::ArrivalInverted(DifficultyId::Standard),
    );
}

#[test]
fn difficulty_missing() {
    let mut def = ghost_lobby();
    def.difficulty.remove(&DifficultyId::Operator);
    expect_one(
        &def,
        ValidationError::DifficultyMissing(DifficultyId::Operator),
    );
}

#[test]
fn negative_action_cost() {
    let mut def = ghost_lobby();
    if let Some(a) = def.actions.get_mut(&ActionKind::Camera) {
        a.cost = -1.0;
    }
    expect_one(
        &def,
        ValidationError::NegativeActionCost(ActionKind::Camera),
    );
}

#[test]
fn support_out_of_range() {
    let mut def = ghost_lobby();
    def.rooms[0].support = 2.0;
    expect_one(
        &def,
        ValidationError::SupportOutOfRange(RoomId::from("kitchen")),
    );
}

#[test]
fn player_spawn_out_of_bounds() {
    let mut def = ghost_lobby();
    def.player.spawn_x = 99_999.0;
    expect_one(&def, ValidationError::PlayerSpawnOutOfBounds);
}

#[test]
fn objective_room_missing() {
    let mut def = ghost_lobby();
    def.objectives[0].room = Some("nowhere".into());
    expect_one(
        &def,
        ValidationError::ObjectiveRoomMissing(ObjectiveId::from("note")),
    );
}

#[test]
fn no_extraction_room() {
    let mut def = ghost_lobby();
    def.objectives
        .retain(|o| o.kind != idaptik_core::scenario::definition::ObjectiveKind::Exit);
    expect_one(&def, ValidationError::NoExtractionRoom);
}

#[test]
fn too_many_cameras_fails_validation_without_panicking() {
    use idaptik_core::scenario::definition::CameraDef;
    use idaptik_core::scenario::floor_graph::MAX_CAMERAS;
    let mut def = ghost_lobby();
    // Grow the camera list one past the addressable cap. Duplicated fixtures on
    // the same watched room are addressing-valid; only the count overflows.
    let template = def.cameras[0].clone();
    while def.cameras.len() <= MAX_CAMERAS {
        let mut cam = CameraDef {
            id: format!("cam-extra-{}", def.cameras.len()).into(),
            ..template.clone()
        };
        cam.stale = true;
        def.cameras.push(cam);
    }
    let count = def.cameras.len();
    expect_one(
        &def,
        ValidationError::TooManyCameras {
            count,
            max: MAX_CAMERAS,
        },
    );
    // And the defence in depth behind the check: even unvalidated, building the
    // sim must not panic on the overflowing addresses.
    let report = def.validate();
    assert!(!report.passed());
}

#[test]
fn too_many_doors_in_one_room_fails_validation_without_panicking() {
    use idaptik_core::scenario::definition::DoorDef;
    use idaptik_core::scenario::floor_graph::MAX_DOORS_PER_ROOM;
    let mut def = ghost_lobby();
    let anchor_x = def.doors[0].x;
    let mut n = 0usize;
    while def
        .doors
        .iter()
        .filter(|d| (d.x - anchor_x).abs() < 1e-9)
        .count()
        <= MAX_DOORS_PER_ROOM
    {
        def.doors.push(DoorDef {
            id: format!("D-extra-{n}").into(),
            x: anchor_x,
            label: "EXTRA".into(),
        });
        n += 1;
    }
    let errs = def.ok().expect_err("an overflowing room must fail");
    assert!(
        errs.iter().any(|e| matches!(
            e,
            ValidationError::TooManyDoorsInRoom { max, .. } if *max == MAX_DOORS_PER_ROOM
        )),
        "expected TooManyDoorsInRoom, got {errs:?}"
    );
}

#[test]
fn object_spawn_out_of_bounds() {
    let mut def = ghost_lobby();
    def.props.note.x = 99_999.0;
    expect_one(
        &def,
        ValidationError::ObjectSpawnOutOfBounds { obj: "note".into() },
    );
}

/// Every prop the infiltrator must physically reach sits within a body-height
/// of the floor. The contact note once carried an absolute `y` of 282.0 —
/// 303px up, over five player-heights — which the Bevy frontend draws
/// literally, floating the mission's real objective at ceiling level while the
/// interaction (x-distance + room only) still handed it over.
#[test]
fn reachable_props_stay_within_a_body_height_of_the_floor() {
    let def = ghost_lobby();
    let reach = c::FLOOR - c::PLAYER_H;
    for (name, y) in [
        ("note", def.props.note.y),
        ("usb", def.props.usb.y),
        ("vacuum", def.props.vacuum.y),
    ] {
        assert!(
            y >= reach,
            "{name} sits {:.0}px above the floor — out of a standing player's reach \
             (floor {}, player height {})",
            c::FLOOR - y,
            c::FLOOR,
            c::PLAYER_H
        );
        assert!(y <= c::FLOOR, "{name} sits below the floor");
    }
}

#[test]
fn more_doors_than_the_frontend_buffer_fails_validation() {
    use idaptik_core::interp::MAX_DOORS;
    use idaptik_core::scenario::definition::DoorDef;
    let mut def = ghost_lobby();
    let anchor_x = def.doors[0].x;
    while def.doors.len() <= MAX_DOORS {
        def.doors.push(DoorDef {
            id: format!("D-extra-{}", def.doors.len()).into(),
            x: anchor_x,
            label: "EXTRA".into(),
        });
    }
    expect_one(
        &def,
        ValidationError::TooManyDoors {
            count: MAX_DOORS + 1,
            max: MAX_DOORS,
        },
    );
}

/// Corrupt one Standard preset field and expect exactly its range error.
#[track_caller]
fn expect_preset_rejected(field: &str, corrupt: impl Fn(&mut DifficultyPreset)) {
    let mut def = ghost_lobby();
    corrupt(def.difficulty.get_mut(&DifficultyId::Standard).unwrap());
    expect_one(
        &def,
        ValidationError::DifficultyValueOutOfRange {
            difficulty: DifficultyId::Standard,
            field: field.into(),
        },
    );
}

#[test]
fn preset_values_that_break_a_run_fail_validation() {
    expect_preset_rejected("trace_threshold", |p| p.trace_threshold = 0);
    expect_preset_rejected("billy_speed", |p| p.billy_speed = -1.0);
    expect_preset_rejected("sprint", |p| p.sprint = 0.0);
    expect_preset_rejected("player_speed", |p| p.player_speed = f64::NAN);
}

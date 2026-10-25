//! The authoring surface, from the outside: the challenge-set floor must
//! audit clean inside a building, the infra-free canonical floor must audit
//! exactly as it always has (fixtures preserved), and infrastructure that
//! dangles must be refused at validation — an error, never a silent floor.
//!
//! These are the level-authoring guarantees the task asks for, exercised
//! through the same public API an authoring tool consumes:
//! `BuildingDefinition::validate` and `BuildingDefinition::audit`.

use idaptik_core::scenario::building::{
    BUILDING_FORMAT, BuildingDefinition, FloorDef, FloorKind, RoomRef, scenario_floor_portals,
};
use idaptik_core::scenario::definition::ScenarioDefinition;
use idaptik_core::scenario::{AuthoringReport, ghost_lobby, pump_house};

/// One Billy floor carrying `scenario`, entered through the kitchen — the
/// committed audit fixture's shape, pointed at whichever floor is under test.
fn building_with(scenario: ScenarioDefinition) -> BuildingDefinition {
    let floor = FloorDef::from_scenario("ground".into(), 0, FloorKind::Billy, scenario.clone());
    BuildingDefinition {
        format: BUILDING_FORMAT.to_owned(),
        building_id: "audit-integration".to_owned(),
        name: "Audit integration".to_owned(),
        entry: RoomRef::new("ground", "kitchen"),
        portals: scenario_floor_portals(&"ground".into(), &scenario),
        circuits: Vec::new(),
        zones: Vec::new(),
        floors: vec![floor],
    }
}

/// The check ids the authoring surface promises. Pinned here so a rename or a
/// silently dropped check fails a test, not a downstream tool.
const CHECK_IDS: [&str; 6] = [
    "refs.controller_grounded",
    "refs.door_crosses_floors",
    "routes.objectives_reachable",
    "challenge.locked_detour",
    "softlock.cut_strands_objectives",
    "softlock.destructive_cut",
];

#[test]
fn a_door_crossing_floors_is_an_advisory_and_a_stair_is_clean() {
    use idaptik_core::scenario::{AuditSeverity, PortalDef, PortalKind};

    let mut building = building_with(pump_house());
    let mut upper = building.floors[0].clone();
    upper.id = "upper".into();
    upper.level = 1;
    upper.kind = FloorKind::Standard;
    upper.scenario = None;
    upper.rooms.truncate(1);
    building.floors.push(upper);
    building.portals.push(PortalDef {
        id: "upstairs".into(),
        kind: PortalKind::Door,
        label: "Upstairs".into(),
        from: RoomRef::new("ground", "kitchen"),
        to: RoomRef::new("upper", "kitchen"),
        travel_time: 1.0,
        lock: None,
        circuit: None,
    });
    assert!(building.validate().passed());
    let report = building.audit();
    assert!(report.passed(), "an advisory must not fail the audit");
    assert!(report.errors().is_empty());
    let advisories = report.advisories();
    assert_eq!(advisories.len(), 1);
    assert_eq!(advisories[0].id, "refs.door_crosses_floors");
    assert_eq!(advisories[0].severity, AuditSeverity::Advisory);
    building.portals.last_mut().unwrap().kind = PortalKind::Stair;
    assert!(building.validate().passed());
    assert!(building.audit().advisories().is_empty());
}

#[test]
fn a_disconnected_objective_fails_the_audit_with_an_error() {
    use idaptik_core::scenario::AuditSeverity;

    let mut building = building_with(pump_house());
    // Keep the note's kitchen reachable and isolate the exit objective.
    building
        .portals
        .retain(|p| p.from.room.as_str() != "exit" && p.to.room.as_str() != "exit");
    let report = building.audit();
    assert!(!report.passed());
    let errors = report.errors();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].id, "routes.objectives_reachable");
    assert_eq!(errors[0].severity, AuditSeverity::Error);
    assert!(report.advisories().is_empty());
    let encoded = serde_json::to_string(&report).unwrap();
    assert_eq!(
        serde_json::from_str::<AuthoringReport>(&encoded).unwrap(),
        report
    );
}

#[test]
fn the_pump_house_floor_validates_and_audits_clean() {
    let scenario = pump_house();
    // The scenario itself validates first — the infrastructure block
    // included (zones known, alarms grounded, the wheel standing in its
    // zone's room).
    assert!(
        scenario.validate().passed(),
        "the challenge-set scenario does not validate: {:?}",
        scenario.validate().ok()
    );

    let building = building_with(scenario);
    let report = building.validate();
    assert!(report.passed(), "the building does not validate");

    let audit = building.audit();
    assert!(
        audit.passed(),
        "the challenge-set floor does not audit clean: {:?}",
        audit
            .errors()
            .iter()
            .map(|c| (c.id.clone(), c.detail.clone()))
            .collect::<Vec<_>>()
    );
    // Every promised check ran, and the objectives on the flooded floor are
    // statically reachable — the water is a play consequence, not a route
    // blocker, and the audit says so.
    for id in CHECK_IDS {
        let check = audit
            .checks
            .iter()
            .find(|c| c.id == id)
            .unwrap_or_else(|| panic!("audit dropped check {id}"));
        assert!(check.passed, "check {id} failed: {}", check.detail);
    }
}

#[test]
fn the_infra_free_canonical_floor_audits_exactly_as_before() {
    let scenario = ghost_lobby();
    assert!(scenario.validate().passed());
    let building = building_with(scenario);
    assert!(building.validate().passed());
    let audit = building.audit();
    assert!(audit.passed(), "the canonical floor stopped auditing clean");
    // The audit's shape is identical with and without infrastructure: the
    // same six checks, the same families. Infrastructure extended the
    // authoring surface; it did not fork it.
    let with_infra = building_with(pump_house()).audit();
    let ids = |r: &AuthoringReport| r.checks.iter().map(|c| c.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&audit), ids(&with_infra));
}

#[test]
fn dangling_infrastructure_references_are_refused_not_rendered() {
    // A water zone in a room the floor does not carry.
    let mut def = pump_house();
    def.infra.as_mut().expect("infra").water_zones[0].room = "nowhere".into();
    assert!(
        !def.validate().passed(),
        "a zone in an unknown room validated"
    );
    // And the building that carries it fails too — the refusal surfaces at
    // the authoring level, not only inside the sim.
    assert!(
        !building_with(def).validate().passed(),
        "a building with a dangling zone validated"
    );

    // An alarm wired to a zone that does not exist.
    let mut def = pump_house();
    let infra = def.infra.as_mut().expect("infra");
    match &mut infra.alarms[0].trip {
        idaptik_core::scenario::infra::AlarmTrip::Water { zone } => {
            *zone = "phantom-zone".to_owned();
        }
        other => panic!("the flood alarm changed shape: {other:?}"),
    }
    assert!(
        !def.validate().passed(),
        "an alarm on a phantom zone validated"
    );

    // A wheel standing in the wrong room: the physical release must be
    // reachable in the zone it floods.
    let mut def = pump_house();
    def.infra.as_mut().expect("infra").water_zones[0].valve_x = 1242.0;
    assert!(
        !def.validate().passed(),
        "a wheel outside its zone's room validated"
    );
}

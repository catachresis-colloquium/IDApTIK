//! The authored secondary network for the Ghost Lobby floor: one maintenance
//! route (hall grille -> service grille), one real cable feeding the
//! substation, and one damaged-cable section the mole can encounter on his
//! way back.
//!
//! This is content, not physics: every id is stable and every section is
//! declared. Cable-sabotage events resolve to `cable-substation-feed`, never
//! to an id invented from position.

use crate::scenario::constants as c;
use crate::scenario::secondary_spaces::model::*;

/// Authored secondary content for the Ghost Lobby floor.
pub fn ghost_lobby_secondary_spec() -> SecondarySpec {
    SecondarySpec {
        routes: vec![RouteSpec {
            id: RouteId::from("maint-hall-service"),
            kind: SpaceKind::Duct,
            label: "Maintenance access duct: hall to service exit".into(),
            geometry: RouteGeometry {
                x_range: (390.0, 1190.0),
                depth: (0.1, 0.6),
            },
            clearance: Clearance { w: 22.0, h: 18.0 },
            actors: ActorClassSet::only(ActorClass::Moletaire),
            travel_cost_per_sec: 0.005,
            equipment_required: vec![],
            access_points: [
                AccessPointId::from("grille-hall"),
                AccessPointId::from("grille-service"),
            ],
            connects_node: Some("substation".into()),
            sealed: false,
        }],
        access_points: vec![
            AccessPointSpec {
                id: AccessPointId::from("grille-hall"),
                kind: AccessPointKind::Grille,
                room: Some("hall".into()),
                x: 400.0,
                y: c::FLOOR,
                sealed: false,
                equipment_to_open: None,
            },
            AccessPointSpec {
                id: AccessPointId::from("grille-service"),
                kind: AccessPointKind::Grille,
                room: Some("exit".into()),
                x: 1180.0,
                y: c::FLOOR,
                sealed: false,
                equipment_to_open: None,
            },
        ],
        cables: vec![CableSpec {
            id: CableId::from("cable-substation-feed"),
            label: "Substation mains feed".into(),
            node_id: "substation".into(),
            route_id: RouteId::from("maint-hall-service"),
            position: 1180.0,
            reach: 14.0,
            state: CableState::Intact,
        }],
        spaces: vec![SpaceSpec {
            id: SpaceId::from("space-substation-feed"),
            route_id: RouteId::from("maint-hall-service"),
            x_range: (1170.0, 1190.0),
            kind: SpaceKind::DamagedCable,
            ref_id: Some("cable-substation-feed".into()),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_spec_is_self_consistent() {
        let spec = ghost_lobby_secondary_spec();
        let def = crate::scenario::ghost_lobby();
        let graph = crate::scenario::floor_graph::floor_graph(&def);
        let problems = spec.validate(&graph);
        assert!(problems.is_empty(), "spec must validate: {problems:?}");

        let route = spec
            .routes
            .iter()
            .find(|r| r.id == RouteId::from("maint-hall-service"))
            .expect("route exists");
        assert_eq!(route.access_points.len(), 2);
        assert!(route.connects_node.is_some());

        // The cable's node is real infrastructure (the substation is real in
        // the Ghost Lobby grounded graph).
        let cable = spec.cables.first().expect("cable exists");
        assert_eq!(cable.id.as_str(), "cable-substation-feed");
        assert!(graph.node(&cable.node_id).is_some());
    }
}

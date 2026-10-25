// Actor access to routes: who may traverse what, and why not.
//
// Human, Moletaire and the security robot deliberately do **not** inherit
// identical access: the check combines the route's actor mask, its
// clearance vs the actor's body box, its current route state, and its
// equipment requirements.
use serde::{Deserialize, Serialize};

use crate::companion::Equipment;
use crate::scenario::secondary_spaces::model::*;

/// The result of asking whether an actor may traverse a route right now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RouteAccess {
    Allowed,
    Denied { reason: AccessDenial },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AccessDenial {
    RouteSealed,
    RouteCollapsed,
    ActorNotAdmitted {
        actor: ActorClass,
    },
    ClearanceTooSmall {
        actor: ActorClass,
        need: ActorBody,
        have: Clearance,
    },
    EquipmentMissing {
        actor: ActorClass,
        missing: Vec<Equipment>,
    },
}

/// Whether `actor` may traverse `route` given the current route/runtime
/// state. Checks run in order: state, admission mask, clearance, equipment.
///
/// Equipment is checked against Moletaire's single equipped item; other
/// actors are not modelled as carrying equipment in this prototype, so a
/// non-empty `equipment_required` denies them.
pub fn route_accessibility(
    actor: ActorClass,
    route: &RouteSpec,
    state: &SecondaryState,
) -> RouteAccess {
    let route_state = state
        .route_states
        .get(&route.id)
        .copied()
        .unwrap_or(RouteState::Open);
    match route_state {
        RouteState::Sealed => {
            return RouteAccess::Denied {
                reason: AccessDenial::RouteSealed,
            };
        }
        RouteState::Collapsed => {
            return RouteAccess::Denied {
                reason: AccessDenial::RouteCollapsed,
            };
        }
        RouteState::Open => {}
    }

    if !route.actors.contains(actor) {
        return RouteAccess::Denied {
            reason: AccessDenial::ActorNotAdmitted { actor },
        };
    }

    if !route.clearance.fits(actor.body()) {
        return RouteAccess::Denied {
            reason: AccessDenial::ClearanceTooSmall {
                actor,
                need: actor.body(),
                have: route.clearance,
            },
        };
    }

    let equipped = match actor {
        ActorClass::Moletaire => state.mole.state().equipped,
        _ => None,
    };
    let missing: Vec<Equipment> = route
        .equipment_required
        .iter()
        .filter(|required| equipped.map_or(true, |carried| carried != **required))
        .cloned()
        .collect();
    if !missing.is_empty() {
        return RouteAccess::Denied {
            reason: AccessDenial::EquipmentMissing { actor, missing },
        };
    }

    RouteAccess::Allowed
}

/// Effective seconds per canvas unit for Moletaire on this route: the authored
/// cost divided by his PathOptimiser coprocessor path efficiency
/// (`1.0` stock, higher on upgraded rungs). This is a fictional in-world
/// augment from [`crate::companion::coprocessors`]; no host timing is
/// involved, so coprocessor upgrades and host-PC acceleration stay distinct.
pub fn mole_travel_cost_per_sec(route: &RouteSpec, path_efficiency: f64) -> f64 {
    route.travel_cost_per_sec / path_efficiency.max(0.01)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    fn duct_route() -> RouteSpec {
        RouteSpec {
            id: RouteId::from("duct-1"),
            kind: SpaceKind::Duct,
            label: "test duct".into(),
            geometry: RouteGeometry {
                x_range: (0.0, 100.0),
                depth: (0.1, 0.5),
            },
            clearance: Clearance { w: 22.0, h: 18.0 },
            actors: ActorClassSet::only(ActorClass::Moletaire),
            travel_cost_per_sec: 0.005,
            equipment_required: vec![],
            access_points: [AccessPointId::from("ap-a"), AccessPointId::from("ap-b")],
            connects_node: None,
            sealed: false,
        }
    }

    fn fresh_state() -> SecondaryState {
        SecondaryState {
            mole: crate::companion::sim::MoletaireSim::new(
                crate::companion::definition::moletaire(),
                crate::companion::sim::MoleParams::default(),
                1,
            )
            .unwrap(),
            mole_route: None,
            mole_was_underground: false,
            mole_target_x: None,
            mole_entry_ap: None,
            route_states: BTreeMap::new(),
            cable_states: BTreeMap::new(),
            discovered: BTreeSet::new(),
            grille_states: BTreeMap::new(),
            topology_version: 0,
            noise: 0.0,
            reported_damage: BTreeSet::new(),
        }
    }

    #[test]
    fn moletaire_can_use_the_duct_while_humans_and_robots_cannot() {
        let route = duct_route();
        let state = fresh_state();
        assert!(matches!(
            route_accessibility(ActorClass::Moletaire, &route, &state),
            RouteAccess::Allowed
        ));
        assert!(matches!(
            route_accessibility(ActorClass::Human, &route, &state),
            RouteAccess::Denied { reason: AccessDenial::ActorNotAdmitted { actor } }
            if actor == ActorClass::Human
        ));
        assert!(matches!(
            route_accessibility(ActorClass::SecurityRobot, &route, &state),
            RouteAccess::Denied { reason: AccessDenial::ActorNotAdmitted { actor } }
            if actor == ActorClass::SecurityRobot
        ));
    }

    #[test]
    fn clearance_denies_larger_actors_even_when_admitted() {
        let mut route = duct_route();
        route.actors = ActorClassSet::all();
        route.clearance = Clearance { w: 20.0, h: 10.0 }; // mole fits, nobody else
        let state = fresh_state();
        assert!(matches!(
            route_accessibility(ActorClass::Moletaire, &route, &state),
            RouteAccess::Allowed
        ));
        assert!(matches!(
            route_accessibility(ActorClass::Human, &route, &state),
            RouteAccess::Denied { reason: AccessDenial::ClearanceTooSmall { actor, .. } }
            if actor == ActorClass::Human
        ));
        assert!(matches!(
            route_accessibility(ActorClass::SecurityRobot, &route, &state),
            RouteAccess::Denied { reason: AccessDenial::ClearanceTooSmall { actor, .. } }
            if actor == ActorClass::SecurityRobot
        ));
    }

    #[test]
    fn sealed_route_denies_everyone() {
        let route = duct_route();
        let mut state = fresh_state();
        state
            .route_states
            .insert(RouteId::from("duct-1"), RouteState::Sealed);
        for actor in ActorClass::ALL {
            assert!(matches!(
                route_accessibility(actor, &route, &state),
                RouteAccess::Denied {
                    reason: AccessDenial::RouteSealed
                }
            ));
        }
    }

    #[test]
    fn equipment_requirement_blocks_unequipped_mole() {
        let mut route = duct_route();
        route.equipment_required = vec![crate::companion::equipment::Equipment::FlashCamera];
        let mut state = fresh_state();
        assert!(matches!(
            route_accessibility(ActorClass::Moletaire, &route, &state),
            RouteAccess::Denied { reason: AccessDenial::EquipmentMissing { actor, .. } }
            if actor == ActorClass::Moletaire
        ));
        state.mole.state_mut().equipped = Some(crate::companion::equipment::Equipment::FlashCamera);
        assert!(matches!(
            route_accessibility(ActorClass::Moletaire, &route, &state),
            RouteAccess::Allowed
        ));
    }

    #[test]
    fn mole_travel_cost_is_authored_not_measured() {
        let route = duct_route();
        let stock = mole_travel_cost_per_sec(&route, 1.0);
        let upgraded = mole_travel_cost_per_sec(&route, 2.0);
        assert_eq!(stock, route.travel_cost_per_sec);
        assert!(upgraded < stock);
        assert!(upgraded > 0.0);
    }
}

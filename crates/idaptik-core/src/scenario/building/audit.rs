//! The building **authoring audit** — the deeper level-authoring surface
//! beside [`BuildingDefinition::validate`] (ADR-0013).
//!
//! `validate` is the load gate: dangling references, duplicates, malformed
//! subnets, structural connectivity. Its report is embedded in committed
//! goldens, so it grows only deliberately. The audit answers the questions an
//! author asks *after* a definition loads:
//!
//! * **Actor-specific route reachability** — can a run that enters at
//!   [`BuildingDefinition::entry`] actually reach every embedded scenario's
//!   objective rooms, through locks that can genuinely be disengaged?
//! * **Softlocks** — is there a circuit whose cut (a permanent act: the
//!   runtime has no restore verb) strands an objective with no alternate
//!   route? Is there a lock controller the derived network never grounds, so
//!   its portal can never open at all?
//! * **Destructive failure states** — which cuts strand *rooms* while the
//!   run stays winnable? Those are consequences, not defects, and are
//!   reported as advisories.
//! * **Intended challenge vs invalid content** — the severity split. A floor
//!   that cannot be completed is an `error`; a floor that punishes a chosen
//!   cut, or that demands network play to progress, is an `advisory`: the
//!   audit surfaces it so the author can confirm it is deliberate.
//!
//! The audit never feeds back into the runtime and never gates a load: it is
//! a report over the definition, deterministic (definition order +
//! `BTreeSet` reachability), and additive — a building with no circuits, no
//! locks and no embedded scenario audits clean.

use crate::netsim::graph::GroundedGraph;
use crate::scenario::building::graph::reachable_rooms;
use crate::scenario::building::network::building_network;
use crate::scenario::building::{BuildingDefinition, PortalDef, PortalKind, RoomRef};
use crate::scenario::ids::CircuitId;
use serde::{Deserialize, Serialize};

/// How an author should read a failed audit check: `Error` marks content
/// that cannot be played to its objectives (fix it or lose the win);
/// `Advisory` marks a consequence or a demand that is very often deliberate
/// (confirm it, then ship it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditSeverity {
    /// Invalid content: the floor cannot be completed as authored.
    Error,
    /// Intended-challenge candidate: consequential, but playable.
    Advisory,
}

/// One named check in the authoring audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoringCheck {
    pub family: String,
    pub id: String,
    pub label: String,
    pub severity: AuditSeverity,
    pub passed: bool,
    pub detail: String,
}

/// The result of [`BuildingDefinition::audit`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoringReport {
    pub checks: Vec<AuthoringCheck>,
}

impl AuthoringReport {
    /// Whether the building is authorable as-is: every `Error`-severity
    /// check passed. Advisories never fail a report — they are surfaced, not
    /// enforced.
    pub fn passed(&self) -> bool {
        !self
            .checks
            .iter()
            .any(|c| c.severity == AuditSeverity::Error && !c.passed)
    }

    /// The failed error checks, in report order.
    pub fn errors(&self) -> Vec<&AuthoringCheck> {
        self.checks
            .iter()
            .filter(|c| c.severity == AuditSeverity::Error && !c.passed)
            .collect()
    }

    /// The fired advisories, in report order.
    pub fn advisories(&self) -> Vec<&AuthoringCheck> {
        self.checks
            .iter()
            .filter(|c| c.severity == AuditSeverity::Advisory && !c.passed)
            .collect()
    }
}

/// Push one audit check.
fn check(
    checks: &mut Vec<AuthoringCheck>,
    family: &str,
    id: &str,
    label: &str,
    severity: AuditSeverity,
    passed: bool,
    detail: String,
) {
    checks.push(AuthoringCheck {
        family: family.to_owned(),
        id: id.to_owned(),
        label: label.to_owned(),
        severity,
        passed,
        detail,
    });
}

/// Every embedded scenario's objective rooms, as building references, in
/// floor-then-objective order. A building with no scenario floors has none,
/// and the objective checks pass vacuously with a saying-so detail.
fn objective_rooms(def: &BuildingDefinition) -> Vec<RoomRef> {
    def.floors
        .iter()
        .filter_map(|f| f.scenario.as_ref().map(|s| (f, s)))
        .flat_map(|(f, s)| {
            s.objectives.iter().filter_map(|o| {
                o.room.as_ref().map(|room| RoomRef {
                    floor: f.id.clone(),
                    room: room.clone(),
                })
            })
        })
        .collect()
}

impl BuildingDefinition {
    /// The authoring audit: the deeper report beside [`Self::validate`].
    /// Deterministic and additive; see the module docs for the severity
    /// contract. This never gates a load — `validate` remains the load gate.
    pub fn audit(&self) -> AuthoringReport {
        let mut checks: Vec<AuthoringCheck> = Vec::new();
        let net = building_network(self);
        let objectives = objective_rooms(self);

        // --- refs: a lock controller the derived network never grounds -----
        // `building_network` mints a controller node only when one of the
        // portal's endpoint rooms sits in a zone; a lock whose controller is
        // absent can never be disengaged, so its portal never opens.
        let ungrounded = self.portals.iter().find(|p| {
            p.lock
                .as_ref()
                .is_some_and(|l| net.node(l.controller.as_str()).is_none())
        });
        check(
            &mut checks,
            "refs",
            "refs.controller_grounded",
            "Every lock controller is grounded in the building's network",
            AuditSeverity::Error,
            ungrounded.is_none(),
            ungrounded
                .map(|p| {
                    format!(
                        "{} lock controller {:?} is not a network node (no zone serves its endpoints?)",
                        p.id,
                        p.lock.as_ref().map(|l| l.controller.clone()).unwrap_or_default()
                    )
                })
                .unwrap_or_else(|| "all grounded".into()),
        );

        // --- refs: a door between two floors is physically incoherent ------
        // Traversable, so not invalid content — but almost always an authoring
        // slip (a stair or lift was meant), so it is surfaced as an advisory.
        let crossing = self
            .portals
            .iter()
            .find(|p| p.kind == PortalKind::Door && p.from.floor != p.to.floor);
        check(
            &mut checks,
            "refs",
            "refs.door_crosses_floors",
            "No Door-kind portal spans two floors",
            AuditSeverity::Advisory,
            crossing.is_none(),
            crossing
                .map(|p| format!("{} is a door between {} and {}", p.id, p.from, p.to))
                .unwrap_or_else(|| "none".into()),
        );

        // --- routes: objectives reachable through *openable* portals -------
        // Passability as the run starts: unlocked portals, plus locked ones
        // whose controller the network grounds (a hacker can disengage those
        // while every circuit is powered, as they all are at reset).
        let openable = |p: &PortalDef| -> bool {
            match &p.lock {
                None => true,
                Some(l) => net.node(l.controller.as_str()).is_some(),
            }
        };
        let reachable = reachable_rooms(self, &self.entry, openable);
        let stranded = objectives.iter().find(|r| !reachable.contains(r));
        check(
            &mut checks,
            "routes",
            "routes.objectives_reachable",
            "Every embedded objective room is reachable from the entry",
            AuditSeverity::Error,
            stranded.is_none(),
            match (objectives.is_empty(), stranded) {
                (true, _) => "no embedded objectives".into(),
                (_, Some(r)) => format!("{r} cannot be reached from {}", self.entry),
                (_, None) => format!("{} objective room(s) reachable", objectives.len()),
            },
        );

        // --- challenge: objectives that demand network play ----------------
        // Reachable only by disengaging a lock: an intended challenge, worth
        // confirming but never a defect.
        let walkable = reachable_rooms(self, &self.entry, |p| p.lock.is_none());
        let needs_network = objectives
            .iter()
            .find(|r| reachable.contains(r) && !walkable.contains(r));
        check(
            &mut checks,
            "challenge",
            "challenge.locked_detour",
            "Objectives do not silently require disengaging a lock",
            AuditSeverity::Advisory,
            needs_network.is_none(),
            needs_network
                .map(|r| format!("{r} opens only by disengaging a lock"))
                .unwrap_or_else(|| "walkable or no objectives".into()),
        );

        // --- softlocks: what a permanent circuit cut can strand ------------
        // The runtime has no restore verb, so a cut lasts the run. For each
        // circuit: portals drawing from it become impassable, and locks whose
        // controllers draw from it can never be disengaged. Stranding an
        // objective is an unwinnable state (error); stranding any other room
        // is a destructive-but-playable consequence (advisory).
        let mut strands_objective: Option<(CircuitId, RoomRef)> = None;
        let mut strands_room: Option<(CircuitId, RoomRef)> = None;
        for circuit in &self.circuits {
            let cut = |p: &PortalDef| -> bool {
                if p.circuit.as_ref() == Some(&circuit.id) {
                    return false;
                }
                match &p.lock {
                    None => true,
                    // A locked portal on a *different* circuit keeps its
                    // grounded controller; one on the cut circuit is already
                    // barred above by the traversal gate.
                    Some(l) => net.node(l.controller.as_str()).is_some(),
                }
            };
            let reach = reachable_rooms(self, &self.entry, cut);
            if let Some(r) = objectives.iter().find(|r| !reach.contains(r)) {
                strands_objective = Some((circuit.id.clone(), r.clone()));
                break;
            }
            if strands_room.is_none() {
                if let Some(r) = self.all_rooms().iter().find(|r| !reach.contains(r)) {
                    strands_room = Some((circuit.id.clone(), r.clone()));
                }
            }
        }
        check(
            &mut checks,
            "softlock",
            "softlock.cut_strands_objectives",
            "No single circuit cut can strand an objective for the run",
            AuditSeverity::Error,
            strands_objective.is_none(),
            strands_objective
                .map(|(c, r)| {
                    format!("cutting {c} permanently strands {r} (no restore verb; give a second route or move the objective)")
                })
                .unwrap_or_else(|| "no cut strands an objective".into()),
        );
        check(
            &mut checks,
            "softlock",
            "softlock.destructive_cut",
            "No circuit cut silently walls off part of the building",
            AuditSeverity::Advisory,
            strands_room.is_none(),
            strands_room
                .map(|(c, r)| format!("cutting {c} walls off {r} while the run stays winnable"))
                .unwrap_or_else(|| "no cut walls off a room".into()),
        );

        AuthoringReport { checks }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::building::{
        CircuitDef, FloorDef, FloorKind, PortalDef, PortalLock, RoomRef, ZoneDef,
        scenario_floor_portals,
    };
    use crate::scenario::ghost_lobby::ghost_lobby;
    use crate::scenario::ids::RoomId;

    /// One Billy floor carrying the canonical scenario, entered through the
    /// canteen: every objective walkable, no circuits, no locks. Audits clean.
    fn walkable_building() -> BuildingDefinition {
        let scenario = ghost_lobby();
        let floor = FloorDef::from_scenario("ground".into(), 0, FloorKind::Billy, scenario.clone());
        BuildingDefinition {
            format: crate::scenario::building::BUILDING_FORMAT.to_owned(),
            building_id: "audit-fixture".to_owned(),
            name: "Audit fixture".to_owned(),
            entry: RoomRef::new("ground", "kitchen"),
            portals: scenario_floor_portals(&"ground".into(), &scenario),
            circuits: Vec::new(),
            zones: Vec::new(),
            floors: vec![floor],
        }
    }

    /// The same floor entered from a cellar, where the only way up is a lift
    /// on a circuit: cutting that circuit strands every objective.
    fn lift_building() -> BuildingDefinition {
        let mut def = walkable_building();
        def.floors.push(FloorDef {
            id: "cellar".into(),
            name: "Cellar".into(),
            level: -1,
            kind: FloorKind::Service,
            rooms: vec![crate::scenario::building::BuildingRoomDef {
                id: RoomId::from("landing"),
                name: "Lift landing".into(),
            }],
            scenario: None,
        });
        def.entry = RoomRef::new("cellar", "landing");
        def.zones.push(ZoneDef {
            id: "z-cellar".into(),
            name: "Cellar zone".into(),
            subnet: "10.60.1.".to_owned(),
            rooms: vec![
                RoomRef::new("cellar", "landing"),
                RoomRef::new("ground", "kitchen"),
            ],
        });
        def.circuits.push(CircuitDef {
            id: "c-lift".into(),
            name: "Lift power".into(),
            source: "lift-feed".to_owned(),
            zone: "z-cellar".into(),
        });
        def.portals.push(PortalDef {
            id: "lift-1".into(),
            kind: PortalKind::Lift,
            label: "Cellar lift".into(),
            from: RoomRef::new("cellar", "landing"),
            to: RoomRef::new("ground", "kitchen"),
            travel_time: 3.0,
            lock: None,
            circuit: Some("c-lift".into()),
        });
        def
    }

    #[test]
    fn a_walkable_building_audits_clean() {
        let report = walkable_building().audit();
        assert!(report.passed(), "errors: {:?}", report.errors());
        assert!(
            report.advisories().is_empty(),
            "advisories: {:?}",
            report.advisories()
        );
    }

    #[test]
    fn a_lift_only_route_flags_the_stranding_cut_as_an_error() {
        let def = lift_building();
        // Before any cut the objectives are reachable (the lift is powered),
        // so the route check passes...
        let report = def.audit();
        let routes = report
            .checks
            .iter()
            .find(|c| c.id == "routes.objectives_reachable")
            .expect("route check present");
        assert!(routes.passed, "{}", routes.detail);
        // ...but the single lift circuit's cut is an unrecoverable softlock.
        let cut = report
            .checks
            .iter()
            .find(|c| c.id == "softlock.cut_strands_objectives")
            .expect("cut check present");
        assert!(!cut.passed, "the lift cut must be flagged");
        assert_eq!(cut.severity, AuditSeverity::Error);
        assert!(!report.passed());
        assert!(cut.detail.contains("c-lift"), "{}", cut.detail);
    }

    #[test]
    fn a_lock_no_network_grounds_is_an_error() {
        let mut def = walkable_building();
        // No zones at all, so the controller node is never minted.
        def.portals.push(PortalDef {
            id: "vault".into(),
            kind: PortalKind::Door,
            label: "Vault door".into(),
            from: RoomRef::new("ground", "kitchen"),
            to: RoomRef::new("ground", "hall"),
            travel_time: 1.0,
            lock: Some(PortalLock {
                controller: "vault-ctrl".to_owned(),
            }),
            circuit: None,
        });
        let report = def.audit();
        let grounded = report
            .checks
            .iter()
            .find(|c| c.id == "refs.controller_grounded")
            .expect("grounding check present");
        assert!(!grounded.passed);
        assert!(grounded.detail.contains("vault-ctrl"), "{}", grounded.detail);
        assert!(!report.passed());
    }

    #[test]
    fn a_cut_that_walls_off_a_side_room_is_only_an_advisory() {
        let mut def = walkable_building();
        // An annex store room served only by a lift on its own circuit:
        // cutting it walls the store off, but every objective stays reachable
        // — a destructive consequence, not a defect.
        def.floors.push(FloorDef {
            id: "annex".into(),
            name: "Annex".into(),
            level: 1,
            kind: FloorKind::Standard,
            rooms: vec![crate::scenario::building::BuildingRoomDef {
                id: RoomId::from("store"),
                name: "Store".into(),
            }],
            scenario: None,
        });
        def.zones.push(ZoneDef {
            id: "z-store".into(),
            name: "Store zone".into(),
            subnet: "10.60.2.".to_owned(),
            rooms: vec![
                RoomRef::new("annex", "store"),
                RoomRef::new("ground", "hall"),
            ],
        });
        def.circuits.push(CircuitDef {
            id: "c-store".into(),
            name: "Store lift power".into(),
            source: "store-feed".to_owned(),
            zone: "z-store".into(),
        });
        def.portals.push(PortalDef {
            id: "store-lift".into(),
            kind: PortalKind::Lift,
            label: "Store lift".into(),
            from: RoomRef::new("ground", "hall"),
            to: RoomRef::new("annex", "store"),
            travel_time: 2.0,
            lock: None,
            circuit: Some("c-store".into()),
        });
        let report = def.audit();
        assert!(report.passed(), "errors: {:?}", report.errors());
        let destructive = report
            .checks
            .iter()
            .find(|c| c.id == "softlock.destructive_cut")
            .expect("destructive check present");
        assert!(!destructive.passed, "the store cut must be surfaced");
        assert_eq!(destructive.severity, AuditSeverity::Advisory);
        assert!(destructive.detail.contains("store"), "{}", destructive.detail);
    }

    #[test]
    fn an_objective_behind_a_lock_is_an_advisory_not_an_error() {
        let mut def = walkable_building();
        // Replace the canteen/hall door with a locked one whose controller IS
        // grounded (the zone serves both rooms): the note objective now opens
        // only by disengaging a lock — intended challenge, advisory severity.
        def.zones.push(ZoneDef {
            id: "z-west".into(),
            name: "West zone".into(),
            subnet: "10.60.3.".to_owned(),
            rooms: vec![
                RoomRef::new("ground", "kitchen"),
                RoomRef::new("ground", "hall"),
            ],
        });
        let door = def
            .portals
            .iter_mut()
            .find(|p| p.id.as_str() == "ground-d1")
            .expect("the projected canteen/hall door");
        door.lock = Some(PortalLock {
            controller: "west-ctrl".to_owned(),
        });
        let report = def.audit();
        assert!(report.passed(), "errors: {:?}", report.errors());
        let detour = report
            .checks
            .iter()
            .find(|c| c.id == "challenge.locked_detour")
            .expect("detour check present");
        assert!(!detour.passed, "the locked detour must be surfaced");
        assert_eq!(detour.severity, AuditSeverity::Advisory);
        // The route check still passes: a grounded lock is openable.
        assert!(
            report
                .checks
                .iter()
                .find(|c| c.id == "routes.objectives_reachable")
                .is_some_and(|c| c.passed)
        );
    }
}

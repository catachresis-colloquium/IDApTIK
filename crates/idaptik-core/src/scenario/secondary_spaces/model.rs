// Secondary spaces: ducts, maintenance passages, cable conduits, traversable
// pipes, excavatable material and structural barriers beneath the Ghost
// Lobby floor, and Moletaire's access to them.
//
// This layer is authored content, not structural physics. Every route,
// grille, cable and hazard is declared in content ([`SecondarySpec`]) with
// stable ids, and every traversal rule is a simple static check: actor class
// vs the route's actor mask and clearance (body dimensions), route state
// ([`Open`](RouteState::Open) / [`Sealed`](RouteState::Sealed) /
// [`Collapsed`](RouteState::Collapsed)), equipment requirements, position on
// an authored route span. Topology changes invalidate cached navigation via
// [`SecondaryState::topology_version`] and surface readable events
// ([`SecondaryEvent::RouteObstructed`],
// [`SecondaryEvent::MoleEntrapped`]). Entrapment, injury and irreversible
// consequences are events with explicit reasons, never silent state changes.
//
// # Moletaire vs host-PC acceleration
//
// Route travel cost is expressed per route ([`RouteSpec::travel_cost_per_sec`]).
// When Moletaire traverses, his fictional PathOptimiser coprocessor improves
// the effective cost — an in-world augment modelled by the companion
// definition's ladders. No host clock, CPU measurement or wall time is ever
// read: time is the fixed [`crate::scenario::mathf::TICK_DT`] only, so
// coprocessor upgrades and host-PC acceleration stay distinct.
use crate::companion::equipment::Equipment;
use crate::scenario::ids::RoomId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

macro_rules! secondary_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_owned())
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

secondary_id!(RouteId);
secondary_id!(AccessPointId);
secondary_id!(CableId);
secondary_id!(SpaceId);

/// Body box the simulation reserves for an actor class (canvas units).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ActorBody {
    pub w: f64,
    pub h: f64,
}

impl ActorBody {
    /// Moletaire's body box: mirrors
    /// [`crate::companion::definition::moletaire`] tuning
    /// (`body_width` / `body_height`), kept as a named constant so this layer
    /// does not reach into the companion sim.
    pub const MOLETAIRE: ActorBody = ActorBody { w: 20.0, h: 14.0 };
    /// The infiltrator's body box: matches
    /// [`crate::scenario::constants::PLAYER_W`] / [`PLAYER_H`].
    pub const HUMAN: ActorBody = ActorBody { w: 28.0, h: 54.0 };
    /// A security robot: deliberately bulkier than Moletaire, so it cannot
    /// inherit Moletaire's access to small ducts.
    pub const SECURITY_ROBOT: ActorBody = ActorBody { w: 30.0, h: 28.0 };
}

/// An actor class that might want to use a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActorClass {
    /// The human infiltrator. Large; cannot fit ducts or small conduits.
    Human,
    /// Moletaire. Small; can tunnel and surface at access points.
    Moletaire,
    /// A security robot. Bulkier than Moletaire; cannot dig.
    SecurityRobot,
}

impl ActorClass {
    /// All three classes, so callers can iterate instead of assuming access.
    pub const ALL: [ActorClass; 3] = [
        ActorClass::Human,
        ActorClass::Moletaire,
        ActorClass::SecurityRobot,
    ];

    /// The body box this actor occupies while traversing.
    pub fn body(self) -> ActorBody {
        match self {
            ActorClass::Human => ActorBody::HUMAN,
            ActorClass::Moletaire => ActorBody::MOLETAIRE,
            ActorClass::SecurityRobot => ActorBody::SECURITY_ROBOT,
        }
    }

    /// The bit this class sets in an [`ActorClassSet`].
    pub const fn bit(self) -> u8 {
        match self {
            ActorClass::Human => 1,
            ActorClass::Moletaire => 2,
            ActorClass::SecurityRobot => 4,
        }
    }
}

/// Actor classes as a bitmask. A route's [`RouteSpec::actors`] is one of
/// these, so Human / Moletaire / security robot never inherit identical
/// access by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActorClassSet(pub u8);

impl ActorClassSet {
    pub const fn none() -> Self {
        Self(0)
    }

    pub const fn all() -> Self {
        Self(0b111)
    }

    pub const fn only(actor: ActorClass) -> Self {
        Self(actor.bit())
    }

    pub fn contains(self, actor: ActorClass) -> bool {
        self.0 & actor.bit() != 0
    }

    /// Human-readable names, for the debug route view.
    pub fn names(self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for actor in ActorClass::ALL {
            if self.contains(actor) {
                out.push(match actor {
                    ActorClass::Human => "human",
                    ActorClass::Moletaire => "moletaire",
                    ActorClass::SecurityRobot => "security-robot",
                });
            }
        }
        out
    }
}

/// Clearance a route offers (canvas units). An actor fits when both
/// dimensions are at least the actor's body box.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Clearance {
    pub w: f64,
    pub h: f64,
}

impl Clearance {
    pub fn fits(self, body: ActorBody) -> bool {
        self.w >= body.w && self.h >= body.h
    }
}

/// What a section of the underground is, physically. [`Duct`](SpaceKind::Duct)
/// and [`CableConduit`](SpaceKind::CableConduit) are the authored traversable
/// classes; [`Pipe`](SpaceKind::Pipe) is traversable only where explicitly
/// authored as such; [`StructuralBarrier`](SpaceKind::StructuralBarrier) is
/// never traversable. [`Fan`](SpaceKind::Fan), [`Valve`](SpaceKind::Valve),
/// [`Water`](SpaceKind::Water) and [`DamagedCable`](SpaceKind::DamagedCable)
/// are inline hazards met along a route section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SpaceKind {
    Duct,
    MaintenancePassage,
    CableConduit,
    Pipe,
    Excavation,
    StructuralBarrier,
    ProtectedInfrastructure,
    Opening,
    Fan,
    Valve,
    Water,
    DamagedCable,
}

/// The x span and depth band a route occupies in world space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RouteGeometry {
    /// World x span the route covers, `[lo, hi]`.
    pub x_range: (f64, f64),
    /// Depth band the route occupies (`0.0` surface .. `1.0` deepest).
    pub depth: (f64, f64),
}

/// One authored maintenance route.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteSpec {
    pub id: RouteId,
    pub kind: SpaceKind,
    pub label: String,
    pub geometry: RouteGeometry,
    pub clearance: Clearance,
    /// Which actor classes may traverse: mask over [`ActorClass`].
    pub actors: ActorClassSet,
    /// Seconds per canvas unit for a baseline traversal (Moletaire, stock
    /// coprocessors). See [`crate::scenario::secondary_spaces::topology`]
    /// for how the PathOptimiser coprocessor improves effective cost in
    /// fiction.
    pub travel_cost_per_sec: f64,
    /// Equipment the actor must carry to traverse (Moletaire's single
    /// equipped item is checked).
    pub equipment_required: Vec<Equipment>,
    /// Access points in order `[entry, egress]`: entry is where the mole
    /// spawns and surfaces, egress the far end.
    pub access_points: [AccessPointId; 2],
    /// Stable grounded-network node id of the service device this route
    /// serves, if any — real infrastructure, never an id invented from
    /// position.
    pub connects_node: Option<String>,
    /// Whether the route is sealed shut (dogged grille, closed bulkhead).
    #[serde(default)]
    pub sealed: bool,
}

/// One grille/panel/hatch where a route meets a room (or a service device).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccessPointSpec {
    pub id: AccessPointId,
    pub kind: AccessPointKind,
    /// Room it opens into, if any. `None` means it opens at a service device
    /// / grounded node rather than a room.
    pub room: Option<RoomId>,
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub sealed: bool,
    /// Equipment needed to open it (Moletaire's equipped item).
    pub equipment_to_open: Option<Equipment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AccessPointKind {
    Grille,
    Panel,
    Hatch,
}

/// The state of a real authored cable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum CableState {
    #[default]
    Intact,
    Damaged,
    /// Cut through: the node it feeds loses power (cascade via the grounded
    /// network). Irreversible in this prototype.
    Severed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CableSpec {
    pub id: CableId,
    pub label: String,
    /// Grounded-network node id this cable feeds — the real affected
    /// infrastructure when the cable is sabotaged.
    pub node_id: String,
    pub route_id: RouteId,
    /// World x where the mole can reach and chew the cable.
    pub position: f64,
    /// Reach around `position` within which sabotage resolves this cable.
    pub reach: f64,
    #[serde(default)]
    pub state: CableState,
}

/// An inline section of a route: a fan, valve, water trace, or a length of
/// damaged cable the mole passes through.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpaceSpec {
    pub id: SpaceId,
    pub route_id: RouteId,
    pub x_range: (f64, f64),
    pub kind: SpaceKind,
    /// The cable id when `kind` is [`SpaceKind::DamagedCable`].
    pub ref_id: Option<CableId>,
}

/// The complete authored secondary-space content for a floor. Optional in
/// [`crate::scenario::definition::ScenarioDefinition`]; when absent the floor
/// has no maintenance routes and every existing replay is unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecondarySpec {
    pub routes: Vec<RouteSpec>,
    pub access_points: Vec<AccessPointSpec>,
    pub cables: Vec<CableSpec>,
    pub spaces: Vec<SpaceSpec>,
}

impl SecondarySpec {
    /// Simple authored-content checks (no physics): access points exist,
    /// clearance and cost are positive, cables reference real nodes and lie
    /// on their route. Returned strings are human-readable problems; an empty
    /// vector means the spec is authored consistently.
    pub fn validate(&self, graph: &crate::netsim::GroundedGraph) -> Vec<String> {
        let mut problems = Vec::new();
        for route in &self.routes {
            for ap_id in &route.access_points {
                if !self.access_points.iter().any(|ap| &ap.id == ap_id) {
                    problems.push(format!(
                        "secondary.routes.{}: access point {:?} is not declared",
                        route.id, ap_id
                    ));
                }
            }
            if route.clearance.w <= 0.0 || route.clearance.h <= 0.0 {
                problems.push(format!(
                    "secondary.routes.{}: clearance must be positive",
                    route.id
                ));
            }
            if route.travel_cost_per_sec <= 0.0 {
                problems.push(format!(
                    "secondary.routes.{}: travel_cost_per_sec must be positive",
                    route.id
                ));
            }
            if route.geometry.x_range.0 > route.geometry.x_range.1 {
                problems.push(format!("secondary.routes.{}: empty x_range", route.id));
            }
        }
        for cable in &self.cables {
            if graph.node(&cable.node_id).is_none() {
                problems.push(format!(
                    "secondary.cables.{}: node_id {:?} is not grounded network infrastructure",
                    cable.id, cable.node_id
                ));
            }
            let on_route = self
                .routes
                .iter()
                .find(|r| &r.id == &cable.route_id)
                .map(|r| {
                    cable.position >= r.geometry.x_range.0 && cable.position <= r.geometry.x_range.1
                })
                .unwrap_or(false);
            if !on_route {
                problems.push(format!(
                    "secondary.cables.{}: position {} is not on its route {:?}",
                    cable.id, cable.position, cable.route_id
                ));
            }
        }
        for ap in &self.access_points {
            if ap.x < 0.0 {
                problems.push(format!(
                    "secondary.access_points.{}: x must be within the floor",
                    ap.id
                ));
            }
        }
        problems
    }
}

/// Route runtime state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteState {
    Open,
    Sealed,
    /// Collapsed: traversal is impossible and entrapment inside is
    /// irreversible for the run.
    Collapsed,
}

/// Runtime state of the secondary-space layer on one floor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecondaryState {
    pub mole: crate::companion::sim::MoletaireSim,
    /// Route the mole is currently inside, if any.
    pub mole_route: Option<RouteId>,
    /// Was the mole underground on the previous tick (entry/exit edge
    /// detection).
    pub mole_was_underground: bool,
    /// Target x the mole was ordered to on the previous tick (reversal
    /// detection).
    pub mole_target_x: Option<f64>,
    /// Access point the mole entered through: his return target.
    pub mole_entry_ap: Option<AccessPointId>,
    pub route_states: BTreeMap<RouteId, RouteState>,
    pub cable_states: BTreeMap<CableId, CableState>,
    /// Routes the mole has discovered (by entering or scanning). Undiscovered
    /// routes are hidden from the playable view; the debug view shows all.
    pub discovered: BTreeSet<RouteId>,
    /// Grille/panel sealed states, keyed by access point id: mirrors
    /// [`AccessPointSpec::sealed`] at runtime so play can change it.
    pub grille_states: BTreeMap<AccessPointId, bool>,
    /// Bumped whenever topology changes (a seal, a collapse, an opened
    /// grille, a new connection). Cached navigation computed under an older
    /// version is stale.
    pub topology_version: u64,
    /// Decaying noise level left by mole activity (digging, sabotage).
    pub noise: f64,
    /// Damaged cables the mole has already reported while traversing, so the
    /// evidence event fires once per cable.
    pub reported_damage: BTreeSet<CableId>,
}

impl SecondaryState {
    pub fn initial(spec: &Option<SecondarySpec>, seed: u32) -> Option<SecondaryState> {
        let spec = spec.as_ref()?;
        let definition = crate::companion::definition::moletaire();
        let first_ap = spec.access_points.first()?;
        // The mole starts on the surface at his entry access point; y matches
        // the player's ground convention (feet on the floor line).
        let mole = crate::companion::sim::MoletaireSim::new(
            definition,
            crate::companion::sim::MoleParams {
                id: crate::companion::definition::COMPANION_ID.to_owned(),
                x: first_ap.x,
                y: crate::scenario::constants::FLOOR,
                equipped: None,
                hunger_level_multiplier: 1.0,
            },
            seed,
        )
        .ok()?;

        let route_states: BTreeMap<RouteId, RouteState> = spec
            .routes
            .iter()
            .map(|route| {
                (
                    route.id.clone(),
                    if route.sealed {
                        RouteState::Sealed
                    } else {
                        RouteState::Open
                    },
                )
            })
            .collect();
        let cable_states: BTreeMap<CableId, CableState> = spec
            .cables
            .iter()
            .map(|cable| (cable.id.clone(), cable.state))
            .collect();
        let grille_states: BTreeMap<AccessPointId, bool> = spec
            .access_points
            .iter()
            .map(|ap| (ap.id.clone(), ap.sealed))
            .collect();

        Some(SecondaryState {
            mole,
            mole_route: None,
            mole_was_underground: false,
            mole_target_x: None,
            mole_entry_ap: None,
            route_states,
            cable_states,
            discovered: BTreeSet::new(),
            grille_states,
            topology_version: 0,
            noise: 0.0,
            reported_damage: BTreeSet::new(),
        })
    }
}

/// Entry/refusal/obstruction/sabotage refusal reasons — read-only, deliberate,
/// human-readable explanations kept in the event log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryRefusalReason {
    AlreadyInRoute,
    NotAtAccessPoint,
    RouteSealed,
    RouteCollapsed,
    NoAuthoredRoute,
    NotTraversable { actor: ActorClass },
    MoleDead,
    MoleBusy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObstructionReason {
    Sealed,
    Collapsed,
    OutsideAuthoredRoute,
    NoAccessPoint,
    NotTraversableForActor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SabotageRefusalReason {
    NotUnderground,
    TooShallow,
    MoleDead,
    NoCableInReach,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceKind {
    CutCableEnds,
    DisturbedGrille,
    FreshDebris,
    WaterTrace,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_body_matches_companion_tuning() {
        // The constants here exist to mirror companion tuning; if someone
        // changes the definition, this test is the place to update both sides
        // deliberately.
        assert_eq!(ActorBody::MOLETAIRE, ActorBody { w: 20.0, h: 14.0 });
    }

    #[test]
    fn actor_class_set_contains_only_its_bit() {
        assert!(ActorClassSet::only(ActorClass::Moletaire).contains(ActorClass::Moletaire));
        assert!(!ActorClassSet::only(ActorClass::Moletaire).contains(ActorClass::Human));
        assert!(ActorClassSet::all().contains(ActorClass::SecurityRobot));
        assert!(!ActorClassSet::none().contains(ActorClass::Moletaire));
    }

    #[test]
    fn clearance_fits_is_symmetric_on_both_dimensions() {
        let small = Clearance { w: 20.0, h: 14.0 };
        assert!(small.fits(ActorBody::MOLETAIRE));
        assert!(!Clearance { w: 19.0, h: 14.0 }.fits(ActorBody::MOLETAIRE));
        assert!(!Clearance { w: 20.0, h: 13.0 }.fits(ActorBody::MOLETAIRE));
    }
}

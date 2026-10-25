//! The Exchange House / UMS building runtime — a scenario as **one floor of a
//! data-defined building**.
//!
//! This ports the Exchange House prototype's building model into
//! `idaptik-core`, following the exact shape the Ghost Lobby established:
//! declarative [`BuildingDefinition`] (pure serde) → [`BuildingDefinition::validate`]
//! (an Exchange-House-style report of named checks with typed
//! [`BuildingValidationError`]s) → a deterministic runtime ([`sim::BuildingSim`])
//! → the prototype's four JSON export surfaces (definition / runtime snapshot /
//! legacy levelconfig / after-action, see [`sim`]).
//!
//! The building model adds what one floor could not express:
//!
//! * multiple [`FloorDef`]s, including a designated **Billy floor**
//!   ([`FloorKind::Billy`]) — the floor a guard scenario plays out on;
//! * [`PortalDef`]s (doors, stairs, lifts, ladders, vents) with a travel time,
//!   an optional lock behind a controller device, and an optional power
//!   circuit (an unpowered lift does not move);
//! * [`CircuitDef`] power circuits, grounded as sources in the building's
//!   network so cutting one cascades to the fixtures it feeds;
//! * [`ZoneDef`] network zones, mapped one-to-one onto `netsim` segments by
//!   [`network::building_network`] — the building's controllers and feeds are
//!   ordinary [`crate::netsim::GroundedGraph`] nodes, so reachability, DNS and
//!   [`crate::netsim::apply_actuation`] all reuse the proven machinery;
//! * a physical graph over rooms and portals with deterministic
//!   shortest-path / reachability ([`graph`]).
//!
//! # Ghost Lobby as one floor
//!
//! A floor can carry a whole [`ScenarioDefinition`] as its content
//! ([`FloorDef::from_scenario`] projects the scenario's rooms into the floor;
//! [`scenario_floor_portals`] projects its doors into building portals). The
//! scenario sim's public API is untouched: `GhostLobbySim::new` consumes the
//! embedded definition exactly as it consumes the standalone one.
//!
//! # UMS scenario-definition seam
//!
//! [`load_building`] is where a UMS `scenario-definition` DLC (payload format
//! [`BUILDING_FORMAT`], `"idaptik-scenario/1"`) plugs in: hand it the payload
//! JSON and it returns a validated [`BuildingDefinition`]. The DLC transport,
//! merging and licensing live above this crate; the format gate and validation
//! live here, mirroring [`crate::scenario::actor::load_actor_pack`].
//!
//! The UMS side has additionally declared an `idaptik-edit/1` edit-script
//! contract (metadatastician/universal-modding-studio PR #2): its verbs (add/move/remove
//! floors, rooms, portals, circuits, zones) target exactly this building
//! model, so the ids and shapes here are that contract's ground truth.

pub mod audit;
pub mod exchange_house;
pub mod graph;
pub mod network;
pub mod sim;

use crate::scenario::definition::{ScenarioDefinition, ValidationError};
use crate::scenario::ids::{CircuitId, FloorId, PortalId, RoomId, ZoneId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::net::Ipv4Addr;

/// Small tolerance for float boundary comparisons (door-to-boundary matching).
const EPS: f64 = 1e-6;

/// The building payload format this build reads and writes — the versioned
/// `payload.format` tag the `idaptik-ums` `scenario-definition` DLC kind
/// reserved for game-side consumption.
pub const BUILDING_FORMAT: &str = "idaptik-scenario/1";

/// Travel time of a portal projected from a scenario door (seconds).
const SCENARIO_DOOR_TRAVEL_TIME: f64 = 1.0;

/// A reference to one room on one floor. Room ids only need to be unique
/// within their floor; the pair is globally unique.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RoomRef {
    pub floor: FloorId,
    pub room: RoomId,
}

impl RoomRef {
    /// Build a reference from raw id strings.
    pub fn new(floor: &str, room: &str) -> Self {
        Self {
            floor: floor.into(),
            room: room.into(),
        }
    }
}

impl core::fmt::Display for RoomRef {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}/{}", self.floor, self.room)
    }
}

/// A room as building content: an id (unique within its floor) and a label.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildingRoomDef {
    pub id: RoomId,
    pub name: String,
}

/// What kind of floor this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FloorKind {
    /// An ordinary floor.
    Standard,
    /// The floor a guard scenario plays out on — every building carries
    /// exactly one.
    Billy,
    /// A service layer (plant rooms, crawlspaces, risers).
    Service,
}

/// One floor of the building.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FloorDef {
    pub id: FloorId,
    pub name: String,
    /// Storey number (ground = 0; service layers may be negative).
    pub level: i32,
    pub kind: FloorKind,
    pub rooms: Vec<BuildingRoomDef>,
    /// A whole scenario as this floor's content. The rooms above are the
    /// projection of the scenario's rooms (validated to match), so the
    /// building graph and the scenario sim agree on what the floor contains.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scenario: Option<ScenarioDefinition>,
}

impl FloorDef {
    /// Embed a scenario as one floor of a building: the floor's rooms are the
    /// projection of the scenario's rooms. Pair with [`scenario_floor_portals`]
    /// for the floor's internal portals.
    pub fn from_scenario(
        id: FloorId,
        level: i32,
        kind: FloorKind,
        scenario: ScenarioDefinition,
    ) -> Self {
        let rooms = scenario
            .rooms
            .iter()
            .map(|r| BuildingRoomDef {
                id: r.id.clone(),
                name: r.name.clone(),
            })
            .collect();
        Self {
            id,
            name: scenario.name.clone(),
            level,
            kind,
            rooms,
            scenario: Some(scenario),
        }
    }
}

/// What kind of portal connects two rooms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PortalKind {
    Door,
    Stair,
    Lift,
    Ladder,
    Vent,
}

/// A lock on a portal: the portal starts locked, and the named controller
/// device (a node in [`network::building_network`]) disengages it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortalLock {
    /// Node id of the controller in the building's grounded network.
    pub controller: String,
}

/// A bidirectional connection between two rooms, possibly across floors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortalDef {
    pub id: PortalId,
    pub kind: PortalKind,
    pub label: String,
    pub from: RoomRef,
    pub to: RoomRef,
    /// Seconds to traverse.
    pub travel_time: f64,
    /// Present iff the portal starts locked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lock: Option<PortalLock>,
    /// The circuit this portal (and its lock controller) draws from. A powered
    /// portal with its circuit cut cannot be traversed; lifts must declare one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub circuit: Option<CircuitId>,
}

/// A power circuit: a source node in the building's grounded network, sitting
/// on one of the zones' segments. Cutting the source de-powers every portal
/// and controller that draws from the circuit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CircuitDef {
    pub id: CircuitId,
    pub name: String,
    /// Node id of the feed in the building's grounded network.
    pub source: String,
    /// The zone whose segment the source sits on.
    pub zone: ZoneId,
}

/// A network zone: a set of rooms served by one `netsim` segment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoneDef {
    pub id: ZoneId,
    pub name: String,
    /// Dotted /24-style subnet prefix, e.g. `"10.60.10."`.
    pub subnet: String,
    pub rooms: Vec<RoomRef>,
}

/// The complete, self-describing building definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildingDefinition {
    pub format: String,
    pub building_id: String,
    pub name: String,
    /// Where an agent enters the building.
    pub entry: RoomRef,
    pub floors: Vec<FloorDef>,
    pub portals: Vec<PortalDef>,
    pub circuits: Vec<CircuitDef>,
    pub zones: Vec<ZoneDef>,
}

/// Project a scenario floor's doors into building portals: each door sits on
/// the boundary between two rooms, and becomes a `Door` portal between them.
/// A door on the world's outer edge (no room on one side) projects nothing —
/// it is the scenario's own extraction surface, not a building connection.
pub fn scenario_floor_portals(floor: &FloorId, scenario: &ScenarioDefinition) -> Vec<PortalDef> {
    let mut portals = Vec::new();
    for door in &scenario.doors {
        let left = scenario
            .rooms
            .iter()
            .find(|r| (r.right() - door.x).abs() <= EPS);
        let right = scenario.rooms.iter().find(|r| (r.x - door.x).abs() <= EPS);
        let (Some(left), Some(right)) = (left, right) else {
            continue;
        };
        portals.push(PortalDef {
            id: format!("{}-{}", floor.as_str(), door.id.as_str().to_lowercase()).into(),
            kind: PortalKind::Door,
            label: door.label.clone(),
            from: RoomRef {
                floor: floor.clone(),
                room: left.id.clone(),
            },
            to: RoomRef {
                floor: floor.clone(),
                room: right.id.clone(),
            },
            travel_time: SCENARIO_DOOR_TRAVEL_TIME,
            lock: None,
            circuit: None,
        });
    }
    portals
}

/// A typed building validation failure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BuildingValidationError {
    UnknownFormat {
        found: String,
    },
    EmptyFloors,
    DuplicateFloorId(FloorId),
    DuplicateFloorLevel(i32),
    NoBillyFloor,
    MultipleBillyFloors,
    EmptyFloorRooms(FloorId),
    DuplicateRoom(RoomRef),
    ScenarioFloorInvalid {
        floor: FloorId,
        errors: Vec<ValidationError>,
    },
    ScenarioRoomsMismatch {
        floor: FloorId,
    },
    DuplicatePortalId(PortalId),
    PortalEndpointMissing {
        portal: PortalId,
        end: RoomRef,
    },
    PortalSelfLoop(PortalId),
    PortalTravelTimeInvalid(PortalId),
    LockControllerInvalid(PortalId),
    LiftWithoutCircuit(PortalId),
    PortalCircuitMissing {
        portal: PortalId,
        circuit: CircuitId,
    },
    DuplicateCircuitId(CircuitId),
    CircuitSourceInvalid(CircuitId),
    CircuitZoneMissing {
        circuit: CircuitId,
        zone: ZoneId,
    },
    DuplicateZoneId(ZoneId),
    ZoneSubnetInvalid(ZoneId),
    ZoneRoomMissing {
        zone: ZoneId,
        room: RoomRef,
    },
    RoomInMultipleZones(RoomRef),
    EntryMissing(RoomRef),
    RoomUnreachable(RoomRef),
    /// A snapshot whose format tag is not the one this build restores.
    UnsupportedSnapshotFormat {
        found: String,
    },
}

/// One named check in the Exchange-House-style building validation report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildingCheck {
    pub id: String,
    pub label: String,
    pub passed: bool,
    pub detail: String,
    pub family: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<BuildingValidationError>,
}

/// The result of [`BuildingDefinition::validate`] — a list of named checks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildingValidationReport {
    pub checks: Vec<BuildingCheck>,
}

impl BuildingValidationReport {
    /// Whether every check passed.
    pub fn passed(&self) -> bool {
        self.checks.iter().all(|c| c.passed)
    }

    /// The typed errors of all failed checks, or `Ok(())` if all passed.
    pub fn ok(&self) -> Result<(), Vec<BuildingValidationError>> {
        let errs: Vec<BuildingValidationError> = self
            .checks
            .iter()
            .filter(|c| !c.passed)
            .filter_map(|c| c.error.clone())
            .collect();
        if errs.is_empty() { Ok(()) } else { Err(errs) }
    }
}

/// Push one check. `err` is `Some` only when the condition failed.
fn check(
    checks: &mut Vec<BuildingCheck>,
    family: &str,
    id: &str,
    label: &str,
    passed: bool,
    detail: String,
    err: Option<BuildingValidationError>,
) {
    checks.push(BuildingCheck {
        id: id.to_owned(),
        label: label.to_owned(),
        passed,
        detail,
        family: family.to_owned(),
        error: if passed { None } else { err },
    });
}

/// Whether `subnet` is a usable dotted /24-style prefix (e.g. `"10.60.10."`).
fn subnet_ok(subnet: &str) -> bool {
    format!("{subnet}1").parse::<Ipv4Addr>().is_ok() && subnet.ends_with('.')
}

impl BuildingDefinition {
    /// Run the full validation suite, returning a report of named checks.
    ///
    /// Each check reports the *first* violation in its family, so a single
    /// malformed field yields exactly one failed check. When `floors` is empty
    /// the room-dependent checks are skipped (they would all fail vacuously).
    pub fn validate(&self) -> BuildingValidationReport {
        let mut checks: Vec<BuildingCheck> = Vec::new();

        // --- format ---------------------------------------------------------
        let format_ok = self.format == BUILDING_FORMAT;
        check(
            &mut checks,
            "format",
            "format.recognised",
            "The payload format tag is the one this build reads",
            format_ok,
            format!("found {:?}, expected {BUILDING_FORMAT:?}", self.format),
            Some(BuildingValidationError::UnknownFormat {
                found: self.format.clone(),
            }),
        );

        // --- floors present ---------------------------------------------------
        let floors_present = !self.floors.is_empty();
        check(
            &mut checks,
            "floors",
            "floors.present",
            "At least one floor is defined",
            floors_present,
            format!("{} floor(s)", self.floors.len()),
            Some(BuildingValidationError::EmptyFloors),
        );
        if floors_present {
            self.validate_floors(&mut checks);
            self.validate_portals(&mut checks);
            self.validate_circuits(&mut checks);
            self.validate_zones(&mut checks);
            self.validate_reachability(&mut checks);
        }

        BuildingValidationReport { checks }
    }

    /// Convenience: `validate().ok()`.
    pub fn ok(&self) -> Result<(), Vec<BuildingValidationError>> {
        self.validate().ok()
    }

    /// Every room in the building, in floor order then room order.
    pub fn all_rooms(&self) -> Vec<RoomRef> {
        self.floors
            .iter()
            .flat_map(|f| {
                f.rooms.iter().map(|r| RoomRef {
                    floor: f.id.clone(),
                    room: r.id.clone(),
                })
            })
            .collect()
    }

    /// Whether the building carries this room.
    pub fn has_room(&self, room: &RoomRef) -> bool {
        self.floors
            .iter()
            .any(|f| f.id == room.floor && f.rooms.iter().any(|r| r.id == room.room))
    }

    /// Look up a floor by id.
    pub fn floor(&self, id: &FloorId) -> Option<&FloorDef> {
        self.floors.iter().find(|f| &f.id == id)
    }

    /// Look up a portal by id.
    pub fn portal(&self, id: &PortalId) -> Option<&PortalDef> {
        self.portals.iter().find(|p| &p.id == id)
    }

    /// Look up a circuit by id.
    pub fn circuit(&self, id: &CircuitId) -> Option<&CircuitDef> {
        self.circuits.iter().find(|c| &c.id == id)
    }

    /// The first floor carrying an embedded scenario, with its definition —
    /// the "Ghost Lobby as one floor" accessor.
    pub fn scenario_floor(&self) -> Option<(&FloorDef, &ScenarioDefinition)> {
        self.floors
            .iter()
            .find_map(|f| f.scenario.as_ref().map(|s| (f, s)))
    }

    fn validate_floors(&self, checks: &mut Vec<BuildingCheck>) {
        // unique floor ids
        let mut dup: Option<FloorId> = None;
        {
            let mut seen: Vec<&FloorId> = Vec::new();
            for f in &self.floors {
                if seen.iter().any(|s| **s == f.id) {
                    dup = Some(f.id.clone());
                    break;
                }
                seen.push(&f.id);
            }
        }
        check(
            checks,
            "floors",
            "floors.unique_ids",
            "Floor ids are unique",
            dup.is_none(),
            dup.as_ref()
                .map(|f| format!("duplicate {f}"))
                .unwrap_or_else(|| "all unique".into()),
            dup.map(BuildingValidationError::DuplicateFloorId),
        );

        // unique storey levels
        let mut dup_level: Option<i32> = None;
        {
            let mut seen: Vec<i32> = Vec::new();
            for f in &self.floors {
                if seen.contains(&f.level) {
                    dup_level = Some(f.level);
                    break;
                }
                seen.push(f.level);
            }
        }
        check(
            checks,
            "floors",
            "floors.unique_levels",
            "Storey levels are unique",
            dup_level.is_none(),
            dup_level
                .map(|l| format!("duplicate level {l}"))
                .unwrap_or_else(|| "all unique".into()),
            dup_level.map(BuildingValidationError::DuplicateFloorLevel),
        );

        // exactly one Billy floor
        let billy_count = self
            .floors
            .iter()
            .filter(|f| f.kind == FloorKind::Billy)
            .count();
        check(
            checks,
            "floors",
            "floors.billy",
            "Exactly one Billy floor is designated",
            billy_count == 1,
            format!("{billy_count} Billy floor(s)"),
            match billy_count {
                0 => Some(BuildingValidationError::NoBillyFloor),
                _ => Some(BuildingValidationError::MultipleBillyFloors),
            },
        );

        // every floor has rooms
        let empty = self
            .floors
            .iter()
            .find(|f| f.rooms.is_empty())
            .map(|f| f.id.clone());
        check(
            checks,
            "floors",
            "floors.rooms_present",
            "Every floor has at least one room",
            empty.is_none(),
            empty
                .as_ref()
                .map(|f| format!("{f} has no rooms"))
                .unwrap_or_else(|| "all populated".into()),
            empty.map(BuildingValidationError::EmptyFloorRooms),
        );

        // globally unique (floor, room) pairs
        let mut dup_room: Option<RoomRef> = None;
        {
            let mut seen: BTreeSet<RoomRef> = BTreeSet::new();
            for room in self.all_rooms() {
                if !seen.insert(room.clone()) {
                    dup_room = Some(room);
                    break;
                }
            }
        }
        check(
            checks,
            "rooms",
            "rooms.unique",
            "Every (floor, room) pair is unique",
            dup_room.is_none(),
            dup_room
                .as_ref()
                .map(|r| format!("duplicate {r}"))
                .unwrap_or_else(|| "all unique".into()),
            dup_room.map(BuildingValidationError::DuplicateRoom),
        );

        // embedded scenarios validate
        let scenario_bad = self.floors.iter().find_map(|f| {
            f.scenario
                .as_ref()
                .and_then(|s| s.ok().err())
                .map(|errors| (f.id.clone(), errors))
        });
        check(
            checks,
            "scenario",
            "scenario.valid",
            "Every embedded scenario definition validates",
            scenario_bad.is_none(),
            scenario_bad
                .as_ref()
                .map(|(f, e)| format!("{f} scenario has {} error(s)", e.len()))
                .unwrap_or_else(|| "all valid".into()),
            scenario_bad.map(
                |(floor, errors)| BuildingValidationError::ScenarioFloorInvalid { floor, errors },
            ),
        );

        // floor rooms are the projection of the embedded scenario's rooms
        let mismatch = self
            .floors
            .iter()
            .find(|f| {
                f.scenario.as_ref().is_some_and(|s| {
                    s.rooms.len() != f.rooms.len()
                        || s.rooms
                            .iter()
                            .zip(&f.rooms)
                            .any(|(sr, fr)| sr.id != fr.id || sr.name != fr.name)
                })
            })
            .map(|f| f.id.clone());
        check(
            checks,
            "scenario",
            "scenario.rooms_match",
            "Every scenario floor's rooms mirror its scenario's rooms",
            mismatch.is_none(),
            mismatch
                .as_ref()
                .map(|f| format!("{f} rooms drifted from its scenario"))
                .unwrap_or_else(|| "all mirrored".into()),
            mismatch.map(|floor| BuildingValidationError::ScenarioRoomsMismatch { floor }),
        );
    }

    fn validate_portals(&self, checks: &mut Vec<BuildingCheck>) {
        // unique portal ids
        let mut dup: Option<PortalId> = None;
        {
            let mut seen: Vec<&PortalId> = Vec::new();
            for p in &self.portals {
                if seen.iter().any(|s| **s == p.id) {
                    dup = Some(p.id.clone());
                    break;
                }
                seen.push(&p.id);
            }
        }
        check(
            checks,
            "portals",
            "portals.unique_ids",
            "Portal ids are unique",
            dup.is_none(),
            dup.as_ref()
                .map(|p| format!("duplicate {p}"))
                .unwrap_or_else(|| "all unique".into()),
            dup.map(BuildingValidationError::DuplicatePortalId),
        );

        // endpoints resolve
        let missing = self.portals.iter().find_map(|p| {
            [&p.from, &p.to]
                .into_iter()
                .find(|end| !self.has_room(end))
                .map(|end| (p.id.clone(), end.clone()))
        });
        check(
            checks,
            "portals",
            "portals.endpoints_resolve",
            "Every portal endpoint names a real room",
            missing.is_none(),
            missing
                .as_ref()
                .map(|(p, end)| format!("{p} references missing {end}"))
                .unwrap_or_else(|| "all resolve".into()),
            missing.map(
                |(portal, end)| BuildingValidationError::PortalEndpointMissing { portal, end },
            ),
        );

        // no self-loops
        let self_loop = self
            .portals
            .iter()
            .find(|p| p.from == p.to)
            .map(|p| p.id.clone());
        check(
            checks,
            "portals",
            "portals.no_self_loop",
            "No portal connects a room to itself",
            self_loop.is_none(),
            self_loop
                .as_ref()
                .map(|p| format!("{p} is a self-loop"))
                .unwrap_or_else(|| "none".into()),
            self_loop.map(BuildingValidationError::PortalSelfLoop),
        );

        // travel times finite and non-negative
        let bad_time = self
            .portals
            .iter()
            .find(|p| !p.travel_time.is_finite() || p.travel_time < 0.0)
            .map(|p| p.id.clone());
        check(
            checks,
            "portals",
            "portals.travel_time",
            "Every travel time is finite and non-negative",
            bad_time.is_none(),
            bad_time
                .as_ref()
                .map(|p| format!("{p} travel time invalid"))
                .unwrap_or_else(|| "all valid".into()),
            bad_time.map(BuildingValidationError::PortalTravelTimeInvalid),
        );

        // lock controllers non-empty and unique
        let bad_lock = self
            .portals
            .iter()
            .find(|p| {
                p.lock.as_ref().is_some_and(|l| {
                    l.controller.is_empty()
                        || self.portals.iter().any(|q| {
                            q.id != p.id
                                && q.lock
                                    .as_ref()
                                    .is_some_and(|m| m.controller == l.controller)
                        })
                })
            })
            .map(|p| p.id.clone());
        check(
            checks,
            "portals",
            "portals.lock_controller",
            "Every lock controller id is non-empty and unique",
            bad_lock.is_none(),
            bad_lock
                .as_ref()
                .map(|p| format!("{p} controller invalid"))
                .unwrap_or_else(|| "all valid".into()),
            bad_lock.map(BuildingValidationError::LockControllerInvalid),
        );

        // lifts declare a circuit
        let dead_lift = self
            .portals
            .iter()
            .find(|p| p.kind == PortalKind::Lift && p.circuit.is_none())
            .map(|p| p.id.clone());
        check(
            checks,
            "portals",
            "portals.lift_powered",
            "Every lift draws from a circuit",
            dead_lift.is_none(),
            dead_lift
                .as_ref()
                .map(|p| format!("{p} has no circuit"))
                .unwrap_or_else(|| "all powered".into()),
            dead_lift.map(BuildingValidationError::LiftWithoutCircuit),
        );

        // portal circuits resolve
        let bad_circuit = self.portals.iter().find_map(|p| {
            p.circuit
                .as_ref()
                .filter(|c| self.circuit(c).is_none())
                .map(|c| (p.id.clone(), c.clone()))
        });
        check(
            checks,
            "portals",
            "portals.circuit_resolves",
            "Every portal circuit names a real circuit",
            bad_circuit.is_none(),
            bad_circuit
                .as_ref()
                .map(|(p, c)| format!("{p} references missing circuit {c}"))
                .unwrap_or_else(|| "all resolve".into()),
            bad_circuit.map(
                |(portal, circuit)| BuildingValidationError::PortalCircuitMissing {
                    portal,
                    circuit,
                },
            ),
        );
    }

    fn validate_circuits(&self, checks: &mut Vec<BuildingCheck>) {
        // unique circuit ids
        let mut dup: Option<CircuitId> = None;
        {
            let mut seen: Vec<&CircuitId> = Vec::new();
            for c in &self.circuits {
                if seen.iter().any(|s| **s == c.id) {
                    dup = Some(c.id.clone());
                    break;
                }
                seen.push(&c.id);
            }
        }
        check(
            checks,
            "circuits",
            "circuits.unique_ids",
            "Circuit ids are unique",
            dup.is_none(),
            dup.as_ref()
                .map(|c| format!("duplicate {c}"))
                .unwrap_or_else(|| "all unique".into()),
            dup.map(BuildingValidationError::DuplicateCircuitId),
        );

        // sources non-empty and unique
        let bad_source = self
            .circuits
            .iter()
            .find(|c| {
                c.source.is_empty()
                    || self
                        .circuits
                        .iter()
                        .any(|d| d.id != c.id && d.source == c.source)
            })
            .map(|c| c.id.clone());
        check(
            checks,
            "circuits",
            "circuits.source",
            "Every circuit source id is non-empty and unique",
            bad_source.is_none(),
            bad_source
                .as_ref()
                .map(|c| format!("{c} source invalid"))
                .unwrap_or_else(|| "all valid".into()),
            bad_source.map(BuildingValidationError::CircuitSourceInvalid),
        );

        // circuit zones resolve
        let bad_zone = self
            .circuits
            .iter()
            .find(|c| !self.zones.iter().any(|z| z.id == c.zone))
            .map(|c| (c.id.clone(), c.zone.clone()));
        check(
            checks,
            "circuits",
            "circuits.zone_resolves",
            "Every circuit sits on a real zone",
            bad_zone.is_none(),
            bad_zone
                .as_ref()
                .map(|(c, z)| format!("{c} references missing zone {z}"))
                .unwrap_or_else(|| "all resolve".into()),
            bad_zone.map(
                |(circuit, zone)| BuildingValidationError::CircuitZoneMissing { circuit, zone },
            ),
        );
    }

    fn validate_zones(&self, checks: &mut Vec<BuildingCheck>) {
        // unique zone ids
        let mut dup: Option<ZoneId> = None;
        {
            let mut seen: Vec<&ZoneId> = Vec::new();
            for z in &self.zones {
                if seen.iter().any(|s| **s == z.id) {
                    dup = Some(z.id.clone());
                    break;
                }
                seen.push(&z.id);
            }
        }
        check(
            checks,
            "zones",
            "zones.unique_ids",
            "Zone ids are unique",
            dup.is_none(),
            dup.as_ref()
                .map(|z| format!("duplicate {z}"))
                .unwrap_or_else(|| "all unique".into()),
            dup.map(BuildingValidationError::DuplicateZoneId),
        );

        // subnets are usable prefixes
        let bad_subnet = self
            .zones
            .iter()
            .find(|z| !subnet_ok(&z.subnet))
            .map(|z| z.id.clone());
        check(
            checks,
            "zones",
            "zones.subnet",
            "Every zone subnet is a usable dotted prefix",
            bad_subnet.is_none(),
            bad_subnet
                .as_ref()
                .map(|z| format!("{z} subnet malformed"))
                .unwrap_or_else(|| "all usable".into()),
            bad_subnet.map(BuildingValidationError::ZoneSubnetInvalid),
        );

        // zone rooms resolve
        let missing = self.zones.iter().find_map(|z| {
            z.rooms
                .iter()
                .find(|r| !self.has_room(r))
                .map(|r| (z.id.clone(), r.clone()))
        });
        check(
            checks,
            "zones",
            "zones.rooms_resolve",
            "Every zone room names a real room",
            missing.is_none(),
            missing
                .as_ref()
                .map(|(z, r)| format!("{z} references missing {r}"))
                .unwrap_or_else(|| "all resolve".into()),
            missing.map(|(zone, room)| BuildingValidationError::ZoneRoomMissing { zone, room }),
        );

        // rooms sit in at most one zone (address determinism)
        let mut shared: Option<RoomRef> = None;
        {
            let mut seen: BTreeSet<&RoomRef> = BTreeSet::new();
            'outer: for z in &self.zones {
                for r in &z.rooms {
                    if !seen.insert(r) {
                        shared = Some(r.clone());
                        break 'outer;
                    }
                }
            }
        }
        check(
            checks,
            "zones",
            "zones.rooms_disjoint",
            "Every room sits in at most one zone",
            shared.is_none(),
            shared
                .as_ref()
                .map(|r| format!("{r} is in multiple zones"))
                .unwrap_or_else(|| "all disjoint".into()),
            shared.map(BuildingValidationError::RoomInMultipleZones),
        );
    }

    fn validate_reachability(&self, checks: &mut Vec<BuildingCheck>) {
        // entry resolves
        let entry_ok = self.has_room(&self.entry);
        check(
            checks,
            "graph",
            "graph.entry_resolves",
            "The entry names a real room",
            entry_ok,
            format!("entry {}", self.entry),
            Some(BuildingValidationError::EntryMissing(self.entry.clone())),
        );

        // every room reachable from the entry, ignoring locks and power
        // (structural connectivity — a locked door is still a connection)
        if entry_ok {
            let reachable = graph::reachable_rooms(self, &self.entry, |_| true);
            let stranded = self
                .all_rooms()
                .into_iter()
                .find(|r| !reachable.contains(r));
            check(
                checks,
                "graph",
                "graph.connected",
                "Every room is reachable from the entry",
                stranded.is_none(),
                stranded
                    .as_ref()
                    .map(|r| format!("{r} is unreachable"))
                    .unwrap_or_else(|| format!("{} room(s) reachable", reachable.len())),
                stranded.map(BuildingValidationError::RoomUnreachable),
            );
        }
    }
}

/// Parse and validate a building payload — the seam a UMS `scenario-definition`
/// DLC (payload format `"idaptik-scenario/1"`) plugs into, mirroring
/// [`crate::scenario::actor::load_actor_pack`]. The transport and merge policy
/// live above this crate; this function guarantees that whatever comes through
/// is a building the runtime can trust.
pub fn load_building(json: &str) -> Result<BuildingDefinition, BuildingPackError> {
    let def: BuildingDefinition =
        serde_json::from_str(json).map_err(|e| BuildingPackError::Parse(e.to_string()))?;
    def.ok().map_err(BuildingPackError::Invalid)?;
    Ok(def)
}

/// Why a building payload was refused.
#[derive(Debug, Clone, PartialEq)]
pub enum BuildingPackError {
    /// The payload is not the JSON shape of a [`BuildingDefinition`].
    Parse(String),
    /// The payload parsed but failed validation (including a wrong
    /// `payload.format` tag).
    Invalid(Vec<BuildingValidationError>),
}

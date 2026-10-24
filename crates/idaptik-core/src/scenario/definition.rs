//! The declarative scenario model — content as data.
//!
//! A [`ScenarioDefinition`] is pure serde data: rooms, doors, cameras, hide
//! spots, props, objectives, the difficulty presets, the full tuning table, and
//! the scoring weights. It round-trips through JSON unchanged and validates via
//! [`ScenarioDefinition::validate`], which returns an Exchange-House-style
//! [`ValidationReport`] of named checks; [`ScenarioDefinition::ok`] projects the
//! failed checks into typed [`ValidationError`]s.

use crate::scenario::floor_graph::floor_graph;
use crate::scenario::ids::{CameraId, DoorId, HideSpotId, ObjectiveId, RoomId, SignalId};
use crate::scenario::secondary_spaces::SecondarySpec;
use crate::scenario::signal::{SignalNetworkDef, SignalValidationError};
use crate::scenario::tuning::{
    ActionKind, ActionSpec, DifficultyId, DifficultyPreset, ScoringDef, TuningConstants,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Small tolerance for float boundary comparisons (contiguity, door edges).
const EPS: f64 = 1e-6;

/// World-level constants: the floor line and overall extent.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WorldDef {
    pub floor: f64,
    pub room_offset: f64,
    pub width: f64,
}

/// A room: an `[x, x + w)` span with per-room support / sight quirks baked in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoomDef {
    pub id: RoomId,
    pub name: String,
    pub x: f64,
    pub w: f64,
    /// Base support envelope in this room (`0.05..=1.0`).
    pub support: f64,
    /// Extra support while a camera ping is active in this room.
    pub ping_support_bonus: f64,
    /// Billy's sight range multiplier in this room (lit office sees further).
    pub sight_multiplier: f64,
    /// Whether the room is lit (the office USB trap).
    pub lit: bool,
}

impl RoomDef {
    /// Right edge (`x + w`).
    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    /// Whether world coordinate `cx` lies in `[x, x + w)`.
    pub fn contains(&self, cx: f64) -> bool {
        cx >= self.x && cx < self.right()
    }
}

/// A door on a room boundary. Opens for everyone; Billy badges through.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DoorDef {
    pub id: DoorId,
    pub x: f64,
    pub label: String,
}

/// A hide spot: crouch within `radius` of `x` in `room`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HideSpotDef {
    pub id: HideSpotId,
    pub room: RoomId,
    pub x: f64,
    pub radius: f64,
    pub label: String,
}

/// A patrol camera with a sinusoidal sweep over `[range.0, range.1]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraDef {
    pub id: CameraId,
    pub room: RoomId,
    pub x: f64,
    pub range: (f64, f64),
    pub phase: f64,
    /// A stale (laundry) feed never detects.
    pub stale: bool,
}

/// A static prop spawn position (`note.x` / `usb.x` are re-rolled at reset).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PropSpawn {
    pub x: f64,
    pub y: f64,
}

/// The four interactive props.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PropsDef {
    pub note: PropSpawn,
    pub usb: PropSpawn,
    pub chute: PropSpawn,
    pub vacuum: PropSpawn,
}

/// The infiltrator's spawn and body box.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlayerDef {
    pub spawn_x: f64,
    pub spawn_y: f64,
    pub w: f64,
    pub h: f64,
}

/// Billy's spawn and body box.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BillyDef {
    pub spawn_x: f64,
    pub spawn_y: f64,
    pub w: f64,
    pub h: f64,
}

/// Objective category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ObjectiveKind {
    Note,
    Misdirect,
    Exit,
}

/// A tracked objective for the ledger.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectiveDef {
    pub id: ObjectiveId,
    pub kind: ObjectiveKind,
    pub label: String,
    /// Optional room the objective is anchored to (validated to exist).
    pub room: Option<RoomId>,
}

/// What a console can show. A screen is a view over the network session of
/// whoever is using it, filtered by what the equipment can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ConsoleCap {
    /// Doors, lights and the vacuum: the building automation.
    Building,
    /// Camera feeds, and with them sightings of people.
    Cameras,
    /// The signal network: wires and relays.
    Wiring,
}

/// Where a console physically is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConsolePlace {
    /// In a room on this floor, at canvas `x`. The infiltrator uses it by
    /// standing at it and pressing interact.
    Room { room: RoomId, x: f64 },
    /// In the hacker's van: the hacker's own kit.
    Van,
}

/// A piece of equipment with a screen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsoleDef {
    pub id: String,
    pub label: String,
    pub place: ConsolePlace,
    pub caps: Vec<ConsoleCap>,
}

/// When the arrival countdown (the seeded `arrival` roll) starts running.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArrivalClock {
    /// From the first tick (the prototype's behaviour).
    #[default]
    RunStart,
    /// From the moment the infiltrator first leaves the room they spawned in.
    LeftSpawnRoom,
}

impl ArrivalClock {
    fn is_run_start(&self) -> bool {
        *self == ArrivalClock::RunStart
    }
}

/// The (base, span) reset-RNG ranges — draw order is load-bearing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpawnRanges {
    pub note_x: (f64, f64),
    pub usb_x: (f64, f64),
    pub stale_pulse: (f64, f64),
    pub snack_x: (f64, f64),
    pub door_delay: (f64, f64),
    pub operator_door_penalty: f64,
}

/// The complete, self-describing scenario definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScenarioDefinition {
    pub format: String,
    pub scenario_id: String,
    pub name: String,
    pub floor_id: u32,
    pub world: WorldDef,
    pub rooms: Vec<RoomDef>,
    pub doors: Vec<DoorDef>,
    pub hide_spots: Vec<HideSpotDef>,
    pub cameras: Vec<CameraDef>,
    pub props: PropsDef,
    pub player: PlayerDef,
    pub billy: BillyDef,
    pub objectives: Vec<ObjectiveDef>,
    pub actions: BTreeMap<ActionKind, ActionSpec>,
    pub difficulty: BTreeMap<DifficultyId, DifficultyPreset>,
    pub tuning: TuningConstants,
    pub scoring: ScoringDef,
    pub spawn: SpawnRanges,
    /// The floor's signal network (ADR-0012), if it has one. Absent on the
    /// canonical Ghost Lobby, so its golden and every replay are unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<SignalNetworkDef>,
    /// When Billy's arrival countdown starts. The canonical floor starts it with
    /// the run; a floor for new players can hold it until the infiltrator has
    /// found their way out of the starting room.
    #[serde(default, skip_serializing_if = "ArrivalClock::is_run_start")]
    pub arrival_clock: ArrivalClock,
    /// Equipment with screens on this floor. Empty on the canonical floor.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub consoles: Vec<ConsoleDef>,
    /// The building's outside: its roof, a rope down the front, hatches, the
    /// street and a front door. Absent on the canonical floor, which starts
    /// inside; a floor with one starts the infiltrator on the roof.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exterior: Option<ExteriorDef>,
    /// The floor's secondary spaces: ducts, maintenance passages, cable
    /// conduits and the routes between them. Absent on most floors; when
    /// present Moletaire can enter maintenance routes from the floor.
    /// Absent on the canonical Ghost Lobby, so its golden and every replay
    /// are unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary: Option<SecondarySpec>,
}

/// Outside the building: the roof the infiltrator starts on, two ways in
/// (a rope down the front facade to the street and the front door, or a hatch
/// straight into a room), and the street in front.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExteriorDef {
    /// Canvas y of the roof's walking surface (feet stand on it).
    pub roof_y: f64,
    /// How far along the roof the body can go (its left edge, like the
    /// floor's clamp).
    pub roof: (f64, f64),
    /// How far down the street the body can go (left edge), outside the front
    /// door.
    pub street_lo: f64,
    /// The rope down the front facade.
    pub rope: RopeDef,
    /// Hatches in the roof, each dropping into the room below.
    pub hatches: Vec<HatchDef>,
    /// The front door: one of `doors`, on the building's outer wall. It is
    /// locked: the infiltrator picks it from the street, or the hacker opens
    /// it like any other door.
    pub front_door: DoorId,
    /// Seconds of picking that open the front door's lock.
    pub pick_time: f64,
    /// The hacker's support outside (the van is in plain sight).
    pub support: f64,
}

/// A rope from the roof's edge down to the street.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RopeDef {
    /// Where on the roof the rope is tied off (body centre).
    pub top_x: f64,
    /// Where it reaches the street, just off the facade (body centre).
    pub foot_x: f64,
    /// Climbing and abseiling speed, canvas units a second.
    pub speed: f64,
}

/// A hatch in the roof over a room.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HatchDef {
    pub id: String,
    /// Centre, canvas x.
    pub x: f64,
    pub room: RoomId,
}

/// A typed validation failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidationError {
    EmptyRooms,
    DuplicateRoomId(RoomId),
    DuplicateDoorId(DoorId),
    RoomsNotContiguous {
        gap_after: RoomId,
    },
    HideSpotRoomMissing(HideSpotId),
    CameraRoomMissing(CameraId),
    CameraRangeInverted(CameraId),
    DoorOffRoomBoundary(DoorId),
    ArrivalInverted(DifficultyId),
    PlayerSpawnOutOfBounds,
    ObjectiveRoomMissing(ObjectiveId),
    NoExtractionRoom,
    DifficultyMissing(DifficultyId),
    NegativeActionCost(ActionKind),
    SupportOutOfRange(RoomId),
    ObjectSpawnOutOfBounds {
        obj: String,
    },
    /// More cameras than the security subnet can address (see
    /// [`crate::scenario::floor_graph::MAX_CAMERAS`]).
    TooManyCameras {
        count: usize,
        max: usize,
    },
    /// More doors in one room than its automation subnet can address (see
    /// [`crate::scenario::floor_graph::MAX_DOORS_PER_ROOM`]).
    TooManyDoorsInRoom {
        room: RoomId,
        count: usize,
        max: usize,
    },
    /// A snapshot whose format tag is not the one this build restores.
    UnsupportedSnapshotFormat {
        found: String,
    },
    /// The floor's signal network does not validate.
    SignalNetwork {
        errors: Vec<SignalValidationError>,
    },
    /// A signal device names a grounded-network node the floor does not carry.
    SignalNodeMissing {
        device: SignalId,
        node: String,
    },
    /// A console stands in a room the floor does not have, or outside it.
    ConsoleOffFloor {
        console: String,
    },
}

/// One named check in the Exchange-House-style validation report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Check {
    pub id: String,
    pub label: String,
    pub passed: bool,
    pub detail: String,
    pub family: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ValidationError>,
}

/// The result of [`ScenarioDefinition::validate`] — a list of named checks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationReport {
    pub checks: Vec<Check>,
}

impl ValidationReport {
    /// Whether every check passed.
    pub fn passed(&self) -> bool {
        self.checks.iter().all(|c| c.passed)
    }

    /// The typed errors of all failed checks, or `Ok(())` if all passed.
    pub fn ok(&self) -> Result<(), Vec<ValidationError>> {
        let errs: Vec<ValidationError> = self
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
    checks: &mut Vec<Check>,
    family: &str,
    id: &str,
    label: &str,
    passed: bool,
    detail: String,
    err: Option<ValidationError>,
) {
    checks.push(Check {
        id: id.to_owned(),
        label: label.to_owned(),
        passed,
        detail,
        family: family.to_owned(),
        error: if passed { None } else { err },
    });
}

impl ScenarioDefinition {
    /// Run the full validation suite, returning a report of named checks.
    ///
    /// Each check reports the *first* violation in its family, so a single
    /// malformed field yields exactly one failed check. When `rooms` is empty
    /// the room-dependent checks are skipped (they would all fail vacuously).
    pub fn validate(&self) -> ValidationReport {
        let mut checks: Vec<Check> = Vec::new();

        // --- rooms present -------------------------------------------------
        let rooms_present = !self.rooms.is_empty();
        check(
            &mut checks,
            "rooms",
            "rooms.present",
            "At least one room is defined",
            rooms_present,
            format!("{} room(s)", self.rooms.len()),
            Some(ValidationError::EmptyRooms),
        );

        if rooms_present {
            self.validate_rooms(&mut checks);
        }

        // --- doors: unique ids (boundary check needs rooms; done above) ----
        {
            let mut dup: Option<DoorId> = None;
            let mut seen: Vec<&DoorId> = Vec::new();
            for d in &self.doors {
                if seen.iter().any(|s| **s == d.id) {
                    dup = Some(d.id.clone());
                    break;
                }
                seen.push(&d.id);
            }
            check(
                &mut checks,
                "doors",
                "doors.unique_ids",
                "Door ids are unique",
                dup.is_none(),
                dup.as_ref()
                    .map(|d| format!("duplicate {d}"))
                    .unwrap_or_else(|| "all unique".into()),
                dup.map(ValidationError::DuplicateDoorId),
            );
        }

        // --- difficulties present + arrival ordered ------------------------
        for id in DifficultyId::ALL {
            let present = self.difficulty.contains_key(&id);
            check(
                &mut checks,
                "difficulty",
                "difficulty.present",
                "All three difficulties are present",
                present,
                format!("{id:?} present={present}"),
                Some(ValidationError::DifficultyMissing(id)),
            );
            if let Some(p) = self.difficulty.get(&id) {
                let ordered = p.arrival.0 <= p.arrival.1;
                check(
                    &mut checks,
                    "difficulty",
                    "difficulty.arrival_ordered",
                    "Arrival window is non-inverted",
                    ordered,
                    format!("{id:?} arrival={:?}", p.arrival),
                    Some(ValidationError::ArrivalInverted(id)),
                );
            }
        }

        // --- actions: non-negative cost/cooldown ---------------------------
        {
            let bad = self
                .actions
                .iter()
                .find(|(_, s)| s.cost < 0.0 || s.cooldown < 0.0)
                .map(|(k, _)| *k);
            check(
                &mut checks,
                "actions",
                "actions.non_negative",
                "Action cost and cooldown are non-negative",
                bad.is_none(),
                bad.map(|k| format!("{k:?} is negative"))
                    .unwrap_or_else(|| "all non-negative".into()),
                bad.map(ValidationError::NegativeActionCost),
            );
        }

        // --- addressing capacity ---------------------------------------------
        // The floor graph addresses every camera and door by a host octet; the
        // caps are derived from the same constants the derivation uses, so the
        // two cannot drift. Overflow here would once have panicked
        // `GhostLobbySim::new` deep inside `floor_graph`; now it is a plain
        // validation failure.
        {
            let max = crate::scenario::floor_graph::MAX_CAMERAS;
            let count = self.cameras.len();
            check(
                &mut checks,
                "capacity",
                "capacity.cameras",
                "Cameras fit the security subnet's host range",
                count <= max,
                format!("{count} camera(s), max {max}"),
                Some(ValidationError::TooManyCameras { count, max }),
            );
        }
        if rooms_present {
            let max = crate::scenario::floor_graph::MAX_DOORS_PER_ROOM;
            let mut per_room: Vec<usize> = vec![0; self.rooms.len()];
            for d in &self.doors {
                let index = crate::scenario::sim::GhostLobbySim::room_index_at(self, d.x);
                if let Some(slot) = per_room.get_mut(index) {
                    *slot += 1;
                }
            }
            let over = per_room
                .iter()
                .enumerate()
                .find(|(_, n)| **n > max)
                .and_then(|(i, n)| self.rooms.get(i).map(|r| (r.id.clone(), *n)));
            check(
                &mut checks,
                "capacity",
                "capacity.doors_per_room",
                "Each room's doors fit its automation subnet's host range",
                over.is_none(),
                over.as_ref()
                    .map(|(room, n)| format!("{room} has {n} door(s), max {max}"))
                    .unwrap_or_else(|| format!("all rooms within {max}")),
                over.map(|(room, count)| ValidationError::TooManyDoorsInRoom { room, count, max }),
            );
        }

        // --- extraction path exists ----------------------------------------
        {
            let has_exit = self
                .objectives
                .iter()
                .any(|o| o.kind == ObjectiveKind::Exit);
            check(
                &mut checks,
                "objectives",
                "objectives.extraction",
                "At least one extraction objective exists",
                has_exit,
                format!("exit objective present={has_exit}"),
                Some(ValidationError::NoExtractionRoom),
            );
        }

        // --- signal network (only when the floor has one, so a floor without
        // wiring reports exactly the checks it always did) -------------------
        if let Some(net) = &self.signal {
            let errors = net.validate();
            check(
                &mut checks,
                "signal",
                "signal.valid",
                "The signal network validates",
                errors.is_empty(),
                format!(
                    "{} device(s), {} problem(s)",
                    net.devices.len(),
                    errors.len()
                ),
                Some(ValidationError::SignalNetwork { errors }),
            );
            let graph = floor_graph(self);
            let missing = net.devices.iter().find_map(|d| {
                let node = d.node.as_ref()?;
                graph
                    .node(node)
                    .is_none()
                    .then(|| (d.id.clone(), node.clone()))
            });
            check(
                &mut checks,
                "signal",
                "signal.nodes",
                "Every signal device's node exists on the floor",
                missing.is_none(),
                format!("missing={missing:?}"),
                missing.map(|(device, node)| ValidationError::SignalNodeMissing { device, node }),
            );
        }

        // --- secondary spaces (only when the floor has them) --------------
        if let Some(net) = &self.secondary {
            let graph = floor_graph(self);
            let problems = net.validate(&graph);
            check(
                &mut checks,
                "secondary",
                "secondary.valid",
                "The secondary-space network validates",
                problems.is_empty(),
                format!("{problems:?}"),
                None,
            );
        }

        // --- consoles stand inside the room they name (only when present) ---
        if !self.consoles.is_empty() {
            let off = self.consoles.iter().find(|c| match &c.place {
                ConsolePlace::Van => false,
                ConsolePlace::Room { room, x } => !self
                    .rooms
                    .iter()
                    .any(|r| &r.id == room && *x >= r.x && *x < r.x + r.w),
            });
            check(
                &mut checks,
                "consoles",
                "consoles.placed",
                "Every console stands inside the room it names",
                off.is_none(),
                format!("off-floor={:?}", off.map(|c| &c.id)),
                off.map(|c| ValidationError::ConsoleOffFloor {
                    console: c.id.clone(),
                }),
            );
        }

        ValidationReport { checks }
    }

    /// Convenience: `validate().ok()`.
    pub fn ok(&self) -> Result<(), Vec<ValidationError>> {
        self.validate().ok()
    }

    // Room-dependent checks (only run when `rooms` is non-empty).
    fn validate_rooms(&self, checks: &mut Vec<Check>) {
        // unique room ids
        let mut dup_room: Option<RoomId> = None;
        {
            let mut seen: Vec<&RoomId> = Vec::new();
            for r in &self.rooms {
                if seen.iter().any(|s| **s == r.id) {
                    dup_room = Some(r.id.clone());
                    break;
                }
                seen.push(&r.id);
            }
        }
        check(
            checks,
            "rooms",
            "rooms.unique_ids",
            "Room ids are unique",
            dup_room.is_none(),
            dup_room
                .as_ref()
                .map(|r| format!("duplicate {r}"))
                .unwrap_or_else(|| "all unique".into()),
            dup_room.map(ValidationError::DuplicateRoomId),
        );

        // contiguity: room[i].right() == room[i+1].x
        let mut gap: Option<RoomId> = None;
        for pair in self.rooms.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if (a.right() - b.x).abs() > EPS {
                gap = Some(a.id.clone());
                break;
            }
        }
        check(
            checks,
            "rooms",
            "rooms.contiguous",
            "Rooms tile the world with no gaps or overlaps",
            gap.is_none(),
            gap.as_ref()
                .map(|r| format!("gap after {r}"))
                .unwrap_or_else(|| "contiguous".into()),
            gap.map(|gap_after| ValidationError::RoomsNotContiguous { gap_after }),
        );

        // room support in [0.05, 1.0]
        let bad_support = self
            .rooms
            .iter()
            .find(|r| !(0.05..=1.0).contains(&r.support))
            .map(|r| r.id.clone());
        check(
            checks,
            "rooms",
            "rooms.support_range",
            "Room support is within [0.05, 1.0]",
            bad_support.is_none(),
            bad_support
                .as_ref()
                .map(|r| format!("{r} support out of range"))
                .unwrap_or_else(|| "in range".into()),
            bad_support.map(ValidationError::SupportOutOfRange),
        );

        // door x on a room boundary
        let boundary_bad = self
            .doors
            .iter()
            .find(|d| !self.is_on_boundary(d.x))
            .map(|d| d.id.clone());
        check(
            checks,
            "doors",
            "doors.on_boundary",
            "Every door sits on a room boundary",
            boundary_bad.is_none(),
            boundary_bad
                .as_ref()
                .map(|d| format!("{d} off boundary"))
                .unwrap_or_else(|| "all on boundaries".into()),
            boundary_bad.map(ValidationError::DoorOffRoomBoundary),
        );

        // hide spot rooms exist
        let hide_bad = self
            .hide_spots
            .iter()
            .find(|h| !self.has_room(&h.room))
            .map(|h| h.id.clone());
        check(
            checks,
            "hide_spots",
            "hide_spots.room_exists",
            "Every hide spot references a real room",
            hide_bad.is_none(),
            hide_bad
                .as_ref()
                .map(|h| format!("{h} references a missing room"))
                .unwrap_or_else(|| "all valid".into()),
            hide_bad.map(ValidationError::HideSpotRoomMissing),
        );

        // camera rooms exist
        let cam_room_bad = self
            .cameras
            .iter()
            .find(|c| !self.has_room(&c.room))
            .map(|c| c.id.clone());
        check(
            checks,
            "cameras",
            "cameras.room_exists",
            "Every camera references a real room",
            cam_room_bad.is_none(),
            cam_room_bad
                .as_ref()
                .map(|c| format!("{c} references a missing room"))
                .unwrap_or_else(|| "all valid".into()),
            cam_room_bad.map(ValidationError::CameraRoomMissing),
        );

        // camera ranges non-inverted
        let cam_range_bad = self
            .cameras
            .iter()
            .find(|c| c.range.0 >= c.range.1)
            .map(|c| c.id.clone());
        check(
            checks,
            "cameras",
            "cameras.range_ordered",
            "Every camera sweep range is non-inverted",
            cam_range_bad.is_none(),
            cam_range_bad
                .as_ref()
                .map(|c| format!("{c} range inverted"))
                .unwrap_or_else(|| "all ordered".into()),
            cam_range_bad.map(ValidationError::CameraRangeInverted),
        );

        // objective rooms exist
        let obj_bad = self
            .objectives
            .iter()
            .find(|o| o.room.as_ref().is_some_and(|r| !self.has_room(r)))
            .map(|o| o.id.clone());
        check(
            checks,
            "objectives",
            "objectives.room_exists",
            "Every objective room reference resolves",
            obj_bad.is_none(),
            obj_bad
                .as_ref()
                .map(|o| format!("{o} references a missing room"))
                .unwrap_or_else(|| "all valid".into()),
            obj_bad.map(ValidationError::ObjectiveRoomMissing),
        );

        // player spawn within world bounds
        let (left, right) = self.world_bounds();
        let player_ok = self.player.spawn_x >= left && self.player.spawn_x <= right;
        check(
            checks,
            "player",
            "player.spawn_in_bounds",
            "Player spawns within the world",
            player_ok,
            format!("spawn_x={} bounds=[{left}, {right}]", self.player.spawn_x),
            Some(ValidationError::PlayerSpawnOutOfBounds),
        );

        // note / usb static spawns within world bounds
        for (name, x) in [("note", self.props.note.x), ("usb", self.props.usb.x)] {
            let in_bounds = x >= left && x <= right;
            check(
                checks,
                "props",
                "props.spawn_in_bounds",
                "Prop spawn base is within the world",
                in_bounds,
                format!("{name}.x={x} bounds=[{left}, {right}]"),
                Some(ValidationError::ObjectSpawnOutOfBounds {
                    obj: name.to_owned(),
                }),
            );
        }
    }

    fn has_room(&self, id: &RoomId) -> bool {
        self.rooms.iter().any(|r| &r.id == id)
    }

    fn is_on_boundary(&self, x: f64) -> bool {
        self.rooms
            .iter()
            .any(|r| (r.x - x).abs() <= EPS || (r.right() - x).abs() <= EPS)
    }

    /// Leftmost room `x` and rightmost room `right()`.
    fn world_bounds(&self) -> (f64, f64) {
        let left = self.rooms.iter().map(|r| r.x).fold(f64::INFINITY, f64::min);
        let right = self
            .rooms
            .iter()
            .map(|r| r.right())
            .fold(f64::NEG_INFINITY, f64::max);
        (left, right)
    }
}

//! Content ids are serde-transparent `String` newtypes. They keep the
//! declarative definition human-readable and homoiconic, and are resolved to
//! `Vec` indices **once** at construction ([`IdIndex`]) so the hot tick loop
//! never hashes strings.

use crate::scenario::definition::ScenarioDefinition;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

macro_rules! id_newtype {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            /// Borrow the underlying id string.
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

        impl core::fmt::Display for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

id_newtype!(RoomId, "Stable id of a room.");
id_newtype!(DoorId, "Stable id of a door.");
id_newtype!(HideSpotId, "Stable id of a hide spot.");
id_newtype!(CameraId, "Stable id of a patrol camera.");
id_newtype!(ObjectiveId, "Stable id of an objective.");
id_newtype!(FloorId, "Stable id of a building floor.");
id_newtype!(
    PortalId,
    "Stable id of a building portal (door/stair/lift/ladder/vent)."
);
id_newtype!(CircuitId, "Stable id of a building power circuit.");
id_newtype!(
    PowerSourceId,
    "Stable id of a power source: mains or backup."
);
id_newtype!(FeedId, "Stable id of a power feed out of a source.");
id_newtype!(BreakerId, "Stable id of a breaker protecting a feed.");
id_newtype!(LoadId, "Stable id of a load the power network carries.");
id_newtype!(HazardId, "Stable id of an electrical hazard site.");
id_newtype!(ZoneId, "Stable id of a building network zone.");
id_newtype!(
    SignalId,
    "Stable id of a device in a signal network (door, relay, console, ...)."
);
id_newtype!(
    CompartmentId,
    "Stable id of an infrastructure compartment: `floor/room`, or `exterior/<name>`."
);
id_newtype!(
    OpeningId,
    "Stable id of a door, hatch, grille or window opening."
);
id_newtype!(ReservoirId, "Stable id of an authored water reservoir.");
id_newtype!(JunctionId, "Stable id of a water-pipe junction.");
id_newtype!(PipeId, "Stable id of a pressurised water pipe.");
id_newtype!(ValveId, "Stable id of a water valve.");
id_newtype!(WaterOutletId, "Stable id of a water outlet or tap.");
id_newtype!(LeakId, "Stable id of a water leak point.");
id_newtype!(DrainId, "Stable id of a floor drain.");
id_newtype!(
    GasSpeciesId,
    "Stable id of an explicitly fictional gas species."
);
id_newtype!(GasSourceId, "Stable id of a gas source or canister.");
id_newtype!(DuctId, "Stable id of a ventilation duct segment.");
id_newtype!(FanId, "Stable id of a duct fan.");
id_newtype!(DamperId, "Stable id of a duct damper.");
id_newtype!(FilterId, "Stable id of a duct filter.");
id_newtype!(EquipmentId, "Stable id of a powered equipment fixture.");
id_newtype!(TraceSourceId, "Stable id of a scent or noise source.");

/// Resolves content ids to `Vec` indices once, so the simulation can index
/// directly. Built at [`crate::scenario::GhostLobbySim`] construction and after
/// snapshot restore.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdIndex {
    rooms: HashMap<RoomId, usize>,
    doors: HashMap<DoorId, usize>,
    hide_spots: HashMap<HideSpotId, usize>,
    cameras: HashMap<CameraId, usize>,
    objectives: HashMap<ObjectiveId, usize>,
}

impl IdIndex {
    /// Build the index from a definition. Later duplicate ids overwrite earlier
    /// ones; [`ScenarioDefinition::validate`] rejects duplicates up front.
    pub fn resolve(def: &ScenarioDefinition) -> Self {
        let mut idx = IdIndex::default();
        for (i, r) in def.rooms.iter().enumerate() {
            idx.rooms.insert(r.id.clone(), i);
        }
        for (i, d) in def.doors.iter().enumerate() {
            idx.doors.insert(d.id.clone(), i);
        }
        for (i, h) in def.hide_spots.iter().enumerate() {
            idx.hide_spots.insert(h.id.clone(), i);
        }
        for (i, c) in def.cameras.iter().enumerate() {
            idx.cameras.insert(c.id.clone(), i);
        }
        for (i, o) in def.objectives.iter().enumerate() {
            idx.objectives.insert(o.id.clone(), i);
        }
        idx
    }

    /// Index of a room id, if present.
    pub fn room(&self, id: &RoomId) -> Option<usize> {
        self.rooms.get(id).copied()
    }

    /// Index of a door id, if present.
    pub fn door(&self, id: &DoorId) -> Option<usize> {
        self.doors.get(id).copied()
    }

    /// Index of a hide-spot id, if present.
    pub fn hide_spot(&self, id: &HideSpotId) -> Option<usize> {
        self.hide_spots.get(id).copied()
    }

    /// Index of a camera id, if present.
    pub fn camera(&self, id: &CameraId) -> Option<usize> {
        self.cameras.get(id).copied()
    }

    /// Index of an objective id, if present.
    pub fn objective(&self, id: &ObjectiveId) -> Option<usize> {
        self.objectives.get(id).copied()
    }
}

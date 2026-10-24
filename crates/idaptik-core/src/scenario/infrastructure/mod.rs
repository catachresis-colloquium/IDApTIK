//! Water, gas and airflow as distinct but interoperable **building systems**.
//!
//! This module is deliberately a *coarse, qualitative* model. It does not solve
//! fluid dynamics, does not track water inside pressurised pipes, and uses
//! fictional gas categories and abstract "units" rather than real chemical
//! concentrations or engineering figures. [`model_declaration`] states exactly
//! what is and is not simulated; the same declaration travels in every
//! snapshot so a consumer cannot mistake it for a physical model.
//!
//! # Shape
//!
//! * [`InfrastructureDefinition`] — serde definition-as-data: authored
//!   [`CompartmentDef`]s, [`OpeningDef`]s, and optional [`water::WaterNetworkDef`]
//!   and [`air::AirNetworkDef`] systems. Format `idaptik-infrastructure/1`.
//! * [`sim::InfrastructureSim`] — the deterministic runtime. One [`sim::step`]
//!   advances both systems and their narrow coupling; typed commands
//!   ([`sim::InfrastructureCommand`]) and events ([`sim::InfrastructureEvent`])
//!   are the only inputs and outputs.
//! * [`fixtures`] — three committed goldens: a minimal water fixture, a minimal
//!   ventilation fixture, and a combined fixture that proves the two systems
//!   interoperate without becoming one model.
//!
//! # Reused building concepts
//!
//! Compartments are [`RoomRef`]s rendered as `floor/room`, or authored
//! `exterior/<name>` boundaries. Openings may name the [`PortalId`] that owns
//! their **traversal**, fans and equipment name the building [`CircuitId`] that
//! powers them, and [`InfrastructureDefinition::validate_against_building`]
//! checks all of that against an existing [`BuildingDefinition`]. Traversal is
//! never inferred from medium flow: water can fill a room and a duct can crawl,
//! but neither changes the building's portal graph by itself.
//!
//! # Model boundaries
//!
//! * **Water** is volume in compartments plus an explicit reservoir. Pressurised
//!   pipes are capacities in a directed graph; the water *inside* them is not
//!   modelled. Outlets, leaks and drains move water between the reservoir, the
//!   compartments and authored exterior sinks.
//! * **Air** is a fixed-volume carrier per compartment plus a pressure scalar.
//!   Ducts move air only when an authored fan runs; dampers scale the flow;
//!   filters remove a bounded fraction of gas species. Openings equalise
//!   pressure passively.
//! * **Gas** species ride in air. Their categories are explicitly fictional:
//!   `haze` obscures, `hush` is a calmant-like gameplay category, and only
//!   `ember` is flammable. No real concentrations are implied.
//! * **Scent and noise** are stylised airborne traces carried by the same air
//!   movement, so redirecting airflow redirects them. They decay.
//! * **Water/gas coupling** is narrow and declared: water absorbs soluble gas,
//!   carries dissolved gas when it drains or spills, and conductive standing
//!   water can trip powered equipment. Clean water is not uniformly conductive.
//!
//! Every quantity is an integer (`u64`); there are no floats in the runtime.
//! Transport is clamped by available source matter, destination free capacity
//! and network capacity, and all flows are accounted in explicit ledgers.

pub mod air;
pub mod fixtures;
pub mod sim;
pub mod water;

pub use air::{
    AirBody, AirLedger, AirNetworkDef, AirState, AirTuning, DamperDef, DuctDef, EquipmentDef,
    EquipmentTarget, EquipmentTrip, FanDef, FilterDef, FlowDirection, GasClass, GasSourceDef,
    GasSpeciesDef, TraceSourceDef,
};
pub use sim::{
    AirAccounting, CompartmentConditions, GasReading, InfrastructureAccounting,
    InfrastructureCommand, InfrastructureDefinitionExport, InfrastructureDenyReason,
    InfrastructureEvent, InfrastructureSim, InfrastructureSnapshot, InfrastructureState,
    InfrastructureValidationReport, SpeciesAccounting, TraceAccounting, TripCause, WaterAccounting,
};
pub use water::{
    DrainDef, JunctionDef, LeakDef, PipeDef, WaterBody, WaterEndpoint, WaterLedger,
    WaterNetworkDef, WaterOutletDef, WaterReservoirDef, WaterState, WaterTuning, WaterValveDef,
};

use crate::scenario::RoomRef;
use crate::scenario::building::BuildingDefinition;
use crate::scenario::ids::FloorId;
use crate::scenario::ids::{
    CircuitId, CompartmentId, DamperId, DrainId, DuctId, EquipmentId, FanId, FilterId, GasSourceId,
    GasSpeciesId, JunctionId, LeakId, OpeningId, PipeId, PortalId, ReservoirId, RoomId,
    TraceSourceId, ValveId, WaterOutletId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The infrastructure payload format this build reads and writes.
pub const INFRASTRUCTURE_FORMAT: &str = "idaptik-infrastructure/1";

/// The runtime snapshot format this build reads and writes.
pub const INFRASTRUCTURE_SNAPSHOT_FORMAT: &str = "idaptik-infrastructure-snapshot/1";

/// Upper bound on any authored quantity. It keeps every bounded transport sum
/// comfortably inside `u64` and rejects absurd payloads before play starts.
pub const MAX_QUANTITY: u64 = 1_000_000_000;

/// Pressure is expressed in stylised per-mille units around a baseline of
/// `1000`; it is not kPa, pascals or any physical pressure.
pub const PRESSURE_BASE: u32 = 1000;

/// What a compartment is: a room in the building, or an authored exterior
/// boundary. Exteriors are *reservoirs*, not rooms: their air pressure is held
/// at [`PRESSURE_BASE`], they accept unlimited water, and every unit crossing
/// the boundary is recorded in a ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompartmentKind {
    Room,
    Exterior,
}

/// One coarse compartment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompartmentDef {
    /// `floor/room` for a room, `exterior/<name>` for an authored boundary.
    pub id: CompartmentId,
    pub name: String,
    pub kind: CompartmentKind,
    /// Stylised free-air volume in air units. Ignored for exteriors.
    pub air_volume: u64,
    /// Stylised water capacity in water units. Ignored for exteriors.
    pub water_capacity: u64,
}

impl CompartmentDef {
    /// Whether this is an authored exterior reservoir.
    pub fn is_exterior(&self) -> bool {
        self.kind == CompartmentKind::Exterior
    }
}

impl CompartmentId {
    /// The compartment id for a building room (`"floor/room"`).
    pub fn room(room: &RoomRef) -> Self {
        format!("{}/{}", room.floor, room.room).into()
    }

    /// Whether this names an authored exterior boundary.
    pub fn is_exterior(&self) -> bool {
        self.0.starts_with("exterior/")
    }

    /// The room this names, or `None` for an exterior or malformed interior
    /// id. This is the inverse of [`CompartmentId::room`].
    pub fn as_room(&self) -> Option<RoomRef> {
        if self.is_exterior() {
            return None;
        }
        let (floor, room) = self.0.split_once('/')?;
        if floor.is_empty() || room.is_empty() {
            return None;
        }
        Some(RoomRef {
            floor: floor.into(),
            room: room.into(),
        })
    }
}

impl From<&RoomRef> for CompartmentId {
    fn from(room: &RoomRef) -> Self {
        CompartmentId::room(room)
    }
}

impl From<RoomRef> for CompartmentId {
    fn from(room: RoomRef) -> Self {
        CompartmentId::room(&room)
    }
}

/// What an opening is, for authoring and cue text. The kind carries no physics;
/// [`OpeningDef::traversal`] and the two conductances do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpeningKind {
    Door,
    Hatch,
    Grille,
    Window,
    DuctAccess,
}

/// A physical opening between two compartments.
///
/// The three channels are independent by design:
///
/// * `traversal` says whether people may pass when the opening is open; this
///   does **not** follow from the conductances;
/// * `air_conductance` lets air (and gas, scent and noise) equalise pressure;
/// * `water_conductance` lets standing water spill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpeningDef {
    pub id: OpeningId,
    pub name: String,
    pub kind: OpeningKind,
    pub a: CompartmentId,
    pub b: CompartmentId,
    /// Air units per tick per 1000 units of pressure difference.
    pub air_conductance: u64,
    /// Water units per tick when water is above the spill threshold.
    pub water_conductance: u64,
    /// People can pass through this opening when it is open. Locks are the
    /// building's business and never stop media.
    pub traversal: bool,
    /// The building portal that owns traversal, when this opening mirrors one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub portal: Option<PortalId>,
    pub start_open: bool,
}

impl OpeningDef {
    /// The compartment at the other end, if `from` is one of its ends.
    pub fn other(&self, from: &CompartmentId) -> Option<&CompartmentId> {
        if &self.a == from {
            Some(&self.b)
        } else if &self.b == from {
            Some(&self.a)
        } else {
            None
        }
    }
}

/// A sensory cue grade. `Trace` / `Warning` / `Critical` deliberately reuse the
/// same ladder as hazards so the UI can colour both without a second scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Clear,
    Trace,
    Warning,
    Critical,
}

impl Tier {
    /// A small integer for frontends.
    pub fn as_u8(self) -> u8 {
        match self {
            Tier::Clear => 0,
            Tier::Trace => 1,
            Tier::Warning => 2,
            Tier::Critical => 3,
        }
    }

    /// The highest of two grades.
    pub fn max(self, other: Tier) -> Tier {
        if self >= other { self } else { other }
    }
}

/// A readable sensory cue. Hazards map onto one of these; the mapping is stable
/// and part of the contract (see [`HazardKind::cue`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CueKind {
    /// Standing water, puddles, wet footprints.
    Puddle,
    /// A pressurised jet or spray.
    Spray,
    /// Gas escaping, or a pressure warning.
    Hiss,
    /// Haze or smoke reducing visibility.
    Haze,
    /// A pressure whine from a duct or a strained fan.
    PressureWhine,
    /// A filter has loaded and no longer cleans.
    FilterLoaded,
    /// Flammable gas has ignited.
    Ignition,
    /// Water gurgling into a drain or a clogged drain backing up.
    Gurgle,
    /// An airborne scent trace.
    Scent,
    /// An airborne noise trace.
    Noise,
}

/// The two non-hazard airborne traces the airflow model carries, so that
/// redirecting a duct redirects the *information* a guard could notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceKind {
    Scent,
    Noise,
}

/// A hazard axis. Gas hazards are per species because the species have
/// different consequences.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HazardKind {
    Water,
    Gas(GasSpeciesId),
    Visibility,
    Pressure,
}

impl HazardKind {
    /// The cue this hazard always emits. Gas species use their own cue; the
    /// caller supplies it for `Gas` (this method returns [`CueKind::Hiss`]).
    pub fn cue(&self) -> CueKind {
        match self {
            HazardKind::Water => CueKind::Puddle,
            HazardKind::Gas(_) => CueKind::Hiss,
            HazardKind::Visibility => CueKind::Haze,
            HazardKind::Pressure => CueKind::PressureWhine,
        }
    }
}

/// The stated nature of the model, carried in every snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelDeclaration {
    /// A stable model class tag, not a claim of physical fidelity.
    pub class: String,
    /// One-paragraph statement for UI and documentation.
    pub statement: String,
    /// things this module deliberately does not simulate.
    pub not_simulated: Vec<String>,
}

/// The module's model declaration. Every parameter below is a styled game
/// parameter; none of it is engineering advice or a real safety threshold.
pub fn model_declaration() -> ModelDeclaration {
    ModelDeclaration {
        class: "qualitative-game-model".to_owned(),
        statement: "A coarse, deterministic game model of water, air and fictional gas \
                    categories in connected room compartments. Units are abstract gameplay \
                    units, not litres, cubic metres, ppm or LEL. Do not use for safety, \
                    engineering, or real building design."
            .to_owned(),
        not_simulated: vec![
            "fluid dynamics, turbulence, head loss or pipe friction".to_owned(),
            "water stored inside pressurised pipes".to_owned(),
            "gas density, stratification, displacement or real concentrations".to_owned(),
            "chemical reactions, toxicity, combustion chemistry or fire spread".to_owned(),
            "evaporation, condensation or humidity".to_owned(),
            "sensor physics, heat transfer or acoustic modelling".to_owned(),
            "door opening forces, pressure-driven door motion or lift behaviour".to_owned(),
        ],
    }
}

/// Per-system tuning and hazard thresholds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InfrastructureTuning {
    pub water: WaterTuning,
    pub air: AirTuning,
}

/// A complete infrastructure definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InfrastructureDefinition {
    pub format: String,
    pub infrastructure_id: String,
    pub name: String,
    pub compartments: Vec<CompartmentDef>,
    pub openings: Vec<OpeningDef>,
    /// Absent means this payload authors no water system at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub water: Option<WaterNetworkDef>,
    /// Absent means this payload authors no ventilation system at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub air: Option<AirNetworkDef>,
    #[serde(default)]
    pub tuning: InfrastructureTuning,
}

impl InfrastructureDefinition {
    /// The compartment with this id.
    pub fn compartment(&self, id: &CompartmentId) -> Option<&CompartmentDef> {
        self.compartments.iter().find(|c| &c.id == id)
    }

    /// The opening with this id.
    pub fn opening(&self, id: &OpeningId) -> Option<&OpeningDef> {
        self.openings.iter().find(|o| &o.id == id)
    }

    /// Every interior room compartment, in sorted order.
    pub fn rooms(&self) -> impl Iterator<Item = &CompartmentDef> {
        self.compartments.iter().filter(|c| !c.is_exterior())
    }

    /// Every authored exterior boundary, in sorted order.
    pub fn exteriors(&self) -> impl Iterator<Item = &CompartmentDef> {
        self.compartments.iter().filter(|c| c.is_exterior())
    }

    /// The water tier a quantity of standing water represents.
    pub fn water_tier(&self, units: u64) -> Tier {
        tier_of(units, &self.tuning.water)
    }

    /// Whether `units` of water blocks traversal (queried separately from the
    /// building's portal graph; see [`sim::CompartmentConditions`]).
    pub fn water_blocks_traversal(&self, units: u64) -> bool {
        self.water_tier(units) >= self.tuning.water.traversal_block_tier
    }

    /// Run the full authoring validation. Returns every problem found, in a
    /// deterministic order; an empty vector means the payload is usable.
    pub fn validate(&self) -> Vec<InfrastructureValidationError> {
        use InfrastructureValidationError as E;
        let mut errs = Vec::new();
        if self.format != INFRASTRUCTURE_FORMAT {
            errs.push(E::UnsupportedFormat {
                found: self.format.clone(),
            });
        }
        if self.compartments.is_empty() {
            errs.push(E::EmptyCompartments);
        }

        let mut seen = BTreeSet::new();
        for c in &self.compartments {
            if !seen.insert(c.id.clone()) {
                errs.push(E::DuplicateCompartment { id: c.id.clone() });
            }
            let convention_ok = match c.kind {
                CompartmentKind::Exterior => c.id.as_str().starts_with("exterior/"),
                CompartmentKind::Room => {
                    !c.id.as_str().starts_with("exterior/") && c.id.as_room().is_some()
                }
            };
            if !convention_ok {
                errs.push(E::CompartmentKindMismatch { id: c.id.clone() });
            }
            if c.kind == CompartmentKind::Room && (c.air_volume == 0 || c.water_capacity == 0) {
                errs.push(E::EmptyRoomCapacity { id: c.id.clone() });
            }
            if c.air_volume > MAX_QUANTITY || c.water_capacity > MAX_QUANTITY {
                errs.push(E::QuantityTooLarge {
                    what: c.id.to_string(),
                });
            }
        }

        let mut opening_ids = BTreeSet::new();
        for o in &self.openings {
            if !opening_ids.insert(o.id.clone()) {
                errs.push(E::DuplicateOpening { id: o.id.clone() });
            }
            for end in [&o.a, &o.b] {
                if self.compartment(end).is_none() {
                    errs.push(E::UnknownCompartment {
                        id: end.clone(),
                        referenced_by: o.id.to_string(),
                    });
                }
            }
            if o.a == o.b {
                errs.push(E::OpeningSelfLoop { id: o.id.clone() });
            }
            if o.air_conductance > MAX_QUANTITY || o.water_conductance > MAX_QUANTITY {
                errs.push(E::QuantityTooLarge {
                    what: o.id.to_string(),
                });
            }
        }

        if let Some(water) = &self.water {
            water::validate_water(self, water, &mut errs);
        }
        if let Some(air) = &self.air {
            air::validate_air(self, air, &mut errs);
        }
        {
            let InfrastructureTuning { water, .. } = &self.tuning;
            if !(water.trace_units < water.warning_units
                && water.warning_units < water.critical_units)
            {
                errs.push(E::WaterThresholdsOutOfOrder);
            }
        }
        if !(self.tuning.air.min_pressure < PRESSURE_BASE
            && PRESSURE_BASE < self.tuning.air.max_pressure)
        {
            errs.push(E::AirPressureBoundsInvalid);
        }
        if self.tuning.air.trace_decay_per_mille > 1000 {
            errs.push(E::AirTraceDecayTooHigh);
        }
        errs
    }

    /// `Ok(())` iff [`Self::validate`] found nothing.
    pub fn ok(&self) -> Result<(), Vec<InfrastructureValidationError>> {
        let errs = self.validate();
        if errs.is_empty() { Ok(()) } else { Err(errs) }
    }

    /// Validate this payload *and* its placement in an existing building:
    /// every interior compartment must be a real room, every referenced portal
    /// must exist and join those rooms, and every referenced circuit must be a
    /// building circuit.
    pub fn validate_against_building(
        &self,
        building: &BuildingDefinition,
    ) -> Vec<InfrastructureValidationError> {
        use InfrastructureValidationError as E;
        let mut errs = self.validate();

        for c in &self.compartments {
            if c.is_exterior() {
                continue;
            }
            if c.id.as_room().is_none() || !building.has_room(&room_of(&c.id)) {
                errs.push(E::UnknownBuildingRoom { id: c.id.clone() });
            }
        }
        for o in &self.openings {
            let Some(portal_id) = &o.portal else {
                continue;
            };
            let Some(portal) = building.portals.iter().find(|p| &p.id == portal_id) else {
                errs.push(E::UnknownBuildingPortal {
                    opening: o.id.clone(),
                    portal: portal_id.clone(),
                });
                continue;
            };
            let a = room_of(&o.a);
            let b = room_of(&o.b);
            let matches =
                (portal.from == a && portal.to == b) || (portal.from == b && portal.to == a);
            if !matches {
                errs.push(E::PortalEndpointMismatch {
                    opening: o.id.clone(),
                    portal: portal_id.clone(),
                });
            }
        }
        for circuit in referenced_circuits(self) {
            if !building.circuits.iter().any(|c| c.id == circuit) {
                errs.push(E::UnknownBuildingCircuit {
                    circuit,
                    referenced_by: "infrastructure".to_owned(),
                });
            }
        }
        errs
    }

    /// `Ok(())` iff [`Self::validate_against_building`] found nothing.
    pub fn ok_in_building(
        &self,
        building: &BuildingDefinition,
    ) -> Result<(), Vec<InfrastructureValidationError>> {
        let errs = self.validate_against_building(building);
        if errs.is_empty() { Ok(()) } else { Err(errs) }
    }
}

/// Every circuit id this payload names, in sorted order.
pub fn referenced_circuits(def: &InfrastructureDefinition) -> BTreeSet<CircuitId> {
    let mut out = BTreeSet::new();
    if let Some(air) = &def.air {
        for fan in &air.fans {
            if let Some(c) = &fan.circuit {
                out.insert(c.clone());
            }
        }
        for e in &air.equipment {
            if let Some(c) = &e.circuit {
                out.insert(c.clone());
            }
        }
    }
    out
}

fn room_of(id: &CompartmentId) -> RoomRef {
    id.as_room().unwrap_or_else(|| RoomRef {
        floor: FloorId("".into()),
        room: RoomId("".into()),
    })
}

/// Map a quantity to the shared [`Tier`] ladder.
pub fn tier_of(units: u64, tuning: &WaterTuning) -> Tier {
    if units >= tuning.critical_units {
        Tier::Critical
    } else if units >= tuning.warning_units {
        Tier::Warning
    } else if units >= tuning.trace_units {
        Tier::Trace
    } else {
        Tier::Clear
    }
}

/// The proportion `amount * part / whole`, computed in `u128` so no authored
/// quantity can overflow. `whole == 0` yields zero.
pub fn proportion(amount: u64, part: u64, whole: u64) -> u64 {
    if whole == 0 {
        return 0;
    }
    ((amount as u128 * part as u128) / whole as u128).min(u64::MAX as u128) as u64
}

/// A typed authoring failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum InfrastructureValidationError {
    UnsupportedFormat {
        found: String,
    },
    UnsupportedSnapshotFormat {
        found: String,
    },
    SnapshotStateMismatch {
        detail: String,
    },
    EmptyCompartments,
    DuplicateCompartment {
        id: CompartmentId,
    },
    CompartmentKindMismatch {
        id: CompartmentId,
    },
    EmptyRoomCapacity {
        id: CompartmentId,
    },
    QuantityTooLarge {
        what: String,
    },
    UnknownCompartment {
        id: CompartmentId,
        referenced_by: String,
    },
    DuplicateOpening {
        id: OpeningId,
    },
    OpeningSelfLoop {
        id: OpeningId,
    },

    WaterThresholdsOutOfOrder,
    DuplicateReservoir {
        id: ReservoirId,
    },
    ReservoirCapacityInvalid {
        id: ReservoirId,
    },
    ReservoirInitialInvalid {
        id: ReservoirId,
    },
    ReservoirSupplyInvalid {
        id: ReservoirId,
    },
    DuplicateJunction {
        id: JunctionId,
    },
    DuplicatePipe {
        id: PipeId,
    },
    PipeEndpointUnknown {
        pipe: PipeId,
    },
    PipeSelfLoop {
        pipe: PipeId,
    },
    PipeCapacityZero {
        pipe: PipeId,
    },
    DuplicateValve {
        id: ValveId,
    },
    ValveUnknownPipe {
        valve: ValveId,
    },
    PipeHasTwoValves {
        pipe: PipeId,
    },
    DuplicateOutlet {
        id: WaterOutletId,
    },
    OutletUnknownCompartment {
        outlet: WaterOutletId,
    },
    OutletOwnerIsExterior {
        outlet: WaterOutletId,
    },
    OutletZeroRate {
        outlet: WaterOutletId,
    },
    DuplicateLeak {
        id: LeakId,
    },
    LeakUnknownCompartment {
        leak: LeakId,
    },
    LeakOwnerIsExterior {
        leak: LeakId,
    },
    LeakZeroRate {
        leak: LeakId,
    },
    DuplicateDrain {
        id: DrainId,
    },
    DrainUnknownCompartment {
        drain: DrainId,
    },
    DrainOwnerIsExterior {
        drain: DrainId,
    },
    DrainTargetNotExterior {
        drain: DrainId,
    },
    DrainZeroCapacity {
        drain: DrainId,
    },
    OutletUnreachable {
        outlet: WaterOutletId,
    },
    LeakUnreachable {
        leak: LeakId,
    },

    AirPressureBoundsInvalid,
    AirTraceDecayTooHigh,
    DuplicateSpecies {
        id: GasSpeciesId,
    },
    SpeciesThresholdOrder {
        id: GasSpeciesId,
    },
    SpeciesSolubilityInvalid {
        id: GasSpeciesId,
    },
    DuplicateDuct {
        id: DuctId,
    },
    DuctUnknownCompartment {
        duct: DuctId,
        compartment: CompartmentId,
    },
    DuctSelfLoop {
        duct: DuctId,
    },
    DuctBothEndsExterior {
        duct: DuctId,
    },
    DuctCapacityZero {
        duct: DuctId,
    },
    DuplicateFan {
        id: FanId,
    },
    FanUnknownDuct {
        fan: FanId,
    },
    FanZeroFlow {
        fan: FanId,
    },
    FanDuctHasFan {
        duct: DuctId,
    },
    DuplicateDamper {
        id: DamperId,
    },
    DamperUnknownDuct {
        damper: DamperId,
    },
    DamperStartTooHigh {
        damper: DamperId,
    },
    DamperDuctHasDamper {
        duct: DuctId,
    },
    DuplicateFilter {
        id: FilterId,
    },
    FilterUnknownDuct {
        filter: FilterId,
    },
    FilterEfficiencyTooHigh {
        filter: FilterId,
    },
    FilterCapacityZero {
        filter: FilterId,
    },
    FilterDuctHasFilter {
        duct: DuctId,
    },
    DuplicateGasSource {
        id: GasSourceId,
    },
    GasSourceUnknownSpecies {
        source: GasSourceId,
    },
    GasSourceUnknownCompartment {
        source: GasSourceId,
    },
    GasSourceOwnerIsExterior {
        source: GasSourceId,
    },
    GasSourceZeroRate {
        source: GasSourceId,
    },
    GasSourceEmpty {
        source: GasSourceId,
    },
    DuplicateTraceSource {
        id: TraceSourceId,
    },
    TraceSourceUnknownCompartment {
        source: TraceSourceId,
    },
    TraceSourceOwnerIsExterior {
        source: TraceSourceId,
    },
    TraceSourceZeroRate {
        source: TraceSourceId,
    },
    TraceSourceEmpty {
        source: TraceSourceId,
    },
    DuplicateEquipment {
        id: EquipmentId,
    },
    EquipmentUnknownCompartment {
        equipment: EquipmentId,
    },
    EquipmentOwnerIsExterior {
        equipment: EquipmentId,
    },
    EquipmentUnknownTarget {
        equipment: EquipmentId,
    },
    DuplicateEquipmentTarget {
        equipment: EquipmentId,
    },
    EquipmentTripInvalid {
        equipment: EquipmentId,
    },

    UnknownBuildingRoom {
        id: CompartmentId,
    },
    UnknownBuildingPortal {
        opening: OpeningId,
        portal: PortalId,
    },
    PortalEndpointMismatch {
        opening: OpeningId,
        portal: PortalId,
    },
    UnknownBuildingCircuit {
        circuit: CircuitId,
        referenced_by: String,
    },
}

/// Reject an authored quantity above [`MAX_QUANTITY`].
pub(crate) fn check_qty(errs: &mut Vec<InfrastructureValidationError>, what: &str, value: u64) {
    if value > MAX_QUANTITY {
        errs.push(InfrastructureValidationError::QuantityTooLarge {
            what: what.to_owned(),
        });
    }
}

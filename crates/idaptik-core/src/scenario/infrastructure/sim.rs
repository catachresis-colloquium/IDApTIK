//! The deterministic infrastructure runtime.
//!
//! [`InfrastructureSim::step`] advances one fixed tick: equipment trips are
//! settled from the water already present, water drains/spills/supplies, air is
//! moved by fans and equalised through openings, gas sources release and filters
//! capture, combustion is checked, soluble gas is absorbed by standing water,
//! and finally hazard tiers are diffed into readable [`InfrastructureEvent`]s.
//!
//! There is no RNG and no wall clock. Every transfer is bounded by source
//! availability, destination free capacity and network capacity. Every
//! boundary crossing and sink is recorded, and [`InfrastructureSim::accounting`]
//! exposes the ledgers for inspection. Commands are the only mutation path;
//! the host is responsible for deciding whether a seat may issue one.

use super::air::{AirNetworkDef, AirState, FlowDirection};
use super::water::{WaterEndpoint, WaterNetworkDef, WaterState};
use super::{
    CueKind, HazardKind, InfrastructureDefinition, InfrastructureValidationError, ModelDeclaration,
    PRESSURE_BASE, Tier, TraceKind, model_declaration, referenced_circuits,
};
use crate::scenario::ids::{
    CircuitId, CompartmentId, DamperId, DrainId, DuctId, EquipmentId, FanId, FilterId, GasSourceId,
    GasSpeciesId, LeakId, OpeningId, ReservoirId, TraceSourceId, ValveId, WaterOutletId,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Why a command was refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "why", rename_all = "snake_case")]
pub enum InfrastructureDenyReason {
    /// No fixture with this id exists in the definition.
    UnknownFixture { id: String },
    /// Only a reversible fan accepts a direction change.
    NotReversible { fan: FanId },
    /// Only a crawlable duct has a traversal hatch to open.
    NotCrawlable { duct: DuctId },
}

/// A typed command. The host gates who may issue one and from where; this
/// runtime owns the effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum InfrastructureCommand {
    SetValve {
        valve: ValveId,
        open: bool,
    },
    SetOutlet {
        outlet: WaterOutletId,
        open: bool,
    },
    PatchLeak {
        leak: LeakId,
    },
    ClearDrain {
        drain: DrainId,
    },
    SetDamper {
        damper: DamperId,
        position: u8,
    },
    SetFan {
        fan: FanId,
        on: bool,
    },
    SetFanDirection {
        fan: FanId,
        direction: FlowDirection,
    },
    ReplaceFilter {
        filter: FilterId,
    },
    ResetEquipment {
        equipment: EquipmentId,
    },
    SetHatch {
        duct: DuctId,
        open: bool,
    },
    SetGasSource {
        source: GasSourceId,
        on: bool,
    },
    SetTraceSource {
        source: TraceSourceId,
        on: bool,
    },
}

/// Why a powered equipment item tripped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TripCause {
    /// Conductive standing water reached the fixture.
    ConductiveWater,
    /// Flammable gas ignited at the fixture.
    Ignition,
}

/// Why a running fan is moving no air.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FanStallReason {
    /// The fan is switched on but its circuit is dead or a tripped fixture
    /// controls it.
    Unpowered,
    /// The damper is fully closed.
    DamperClosed,
    /// The source compartment is at its lower pressure bound.
    SourceStarved,
    /// The destination compartment is at its upper pressure bound.
    TargetFull,
}

/// A typed event. Continuous flows are read from state; events mark state
/// changes, hazards, and consequences so the log stays readable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum InfrastructureEvent {
    ValveSet {
        valve: ValveId,
        open: bool,
    },
    OutletSet {
        outlet: WaterOutletId,
        open: bool,
    },
    LeakPatched {
        leak: LeakId,
    },
    DrainCleared {
        drain: DrainId,
    },
    DamperSet {
        damper: DamperId,
        position: u8,
    },
    FanSet {
        fan: FanId,
        on: bool,
    },
    FanDirectionSet {
        fan: FanId,
        direction: FlowDirection,
    },
    FilterReplaced {
        filter: FilterId,
        disposed: u64,
    },
    EquipmentReset {
        equipment: EquipmentId,
    },
    HatchSet {
        duct: DuctId,
        open: bool,
    },
    GasSourceSet {
        source: GasSourceId,
        on: bool,
    },
    TraceSourceSet {
        source: TraceSourceId,
        on: bool,
    },
    OpeningSynced {
        opening: OpeningId,
        open: bool,
    },
    CircuitSynced {
        circuit: CircuitId,
        live: bool,
    },
    ReservoirEmptied {
        reservoir: ReservoirId,
    },
    GasSourceEmptied {
        source: GasSourceId,
    },
    EquipmentTripped {
        equipment: EquipmentId,
        cause: TripCause,
    },
    FilterSaturated {
        filter: FilterId,
    },
    AirflowStalled {
        duct: DuctId,
        reason: FanStallReason,
    },
    Combustion {
        compartment: CompartmentId,
        species: GasSpeciesId,
        amount: u64,
    },
    /// A hazard crossed a tier in either direction. `cue` is the stable sensory
    /// cue for that hazard; `tier: Clear` means the cue is gone.
    HazardChanged {
        compartment: CompartmentId,
        hazard: HazardKind,
        tier: Tier,
        cue: CueKind,
    },
    /// A one-shot cue with no hazard axis (ignition, for example).
    Cue {
        compartment: CompartmentId,
        cue: CueKind,
        tier: Tier,
    },
    Denied {
        reason: InfrastructureDenyReason,
    },
}

/// The complete restorable runtime state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InfrastructureState {
    pub tick: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub water: Option<WaterState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub air: Option<AirState>,
    /// Aperture state for every authored opening. This mirrors the building's
    /// doors where applicable; a door's lock never changes it.
    pub openings_open: BTreeMap<OpeningId, bool>,
}

impl InfrastructureState {
    /// The state a fresh run starts in.
    pub fn initial(def: &InfrastructureDefinition) -> Self {
        Self {
            tick: 0,
            water: def.water.as_ref().map(|w| WaterState::initial(def, w)),
            air: def.air.as_ref().map(|a| AirState::initial(def, a)),
            openings_open: def
                .openings
                .iter()
                .map(|o| (o.id.clone(), o.start_open))
                .collect(),
        }
    }
}

/// A restorable snapshot, including the model declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InfrastructureSnapshot {
    pub format: String,
    pub definition: InfrastructureDefinition,
    pub state: InfrastructureState,
    pub model: ModelDeclaration,
}

/// The validation result of one definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InfrastructureValidationReport {
    pub errors: Vec<InfrastructureValidationError>,
}

impl InfrastructureValidationReport {
    pub fn passed(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn ok(&self) -> Result<(), Vec<InfrastructureValidationError>> {
        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors.clone())
        }
    }
}

/// A definition plus its validation report, for editors and UMS payloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InfrastructureDefinitionExport {
    pub format: String,
    pub definition: InfrastructureDefinition,
    /// `None` when the payload validates against its own compartments. A
    /// building-relative check is separate (`validate_against_building`).
    pub validation: InfrastructureValidationReport,
}

/// Load a UMS-style infrastructure payload: parse it, gate on the format tag,
/// then validate it before the runtime trusts it.
pub fn load_infrastructure(
    json: &str,
) -> Result<InfrastructureDefinition, Vec<InfrastructureValidationError>> {
    let def: InfrastructureDefinition = serde_json::from_str(json).map_err(|e| {
        vec![InfrastructureValidationError::UnsupportedFormat {
            found: format!("parse error: {e}"),
        }]
    })?;
    def.ok()?;
    Ok(def)
}

/// One gas species' reading in a compartment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GasReading {
    pub units: u64,
    /// Per-mille of the compartment's air units. This is a game number.
    pub concentration: u64,
    pub tier: Tier,
}

/// The player- and host-facing readout for one compartment. Hazards are
/// queries, not graph mutations: `traversal_blocked` is separate from the
/// building's portal graph and from every medium path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompartmentConditions {
    pub compartment: CompartmentId,
    pub air_units: u64,
    /// Stylised per-mille pressure around a baseline of 1000.
    pub pressure: u32,
    pub pressure_tier: Tier,
    pub water_units: u64,
    pub water_tier: Tier,
    pub conductivity: u16,
    pub dissolved_units: u64,
    pub gas: BTreeMap<GasSpeciesId, GasReading>,
    pub visibility: Tier,
    /// Standing water has reached the authored traversal-block tier. The host
    /// applies this to movement; the runtime never edits the portal graph.
    pub traversal_blocked: bool,
    /// Every cue currently at or above `Trace`, with its grade.
    pub cues: BTreeMap<CueKind, Tier>,
    pub scent: u64,
    pub noise: u64,
}

/// Water accounting, all values cumulative except the two current totals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaterAccounting {
    pub initial_reservoir: u64,
    pub recharged: u64,
    pub current_reservoirs: u64,
    pub current_compartments: u64,
    pub drained_to_exterior: u64,
    pub spilled_to_exterior: u64,
    pub balanced: bool,
}

/// One species' accounting across air, filters and water.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeciesAccounting {
    pub released: u64,
    pub current_air: u64,
    pub current_filter_load: u64,
    pub filter_disposed: u64,
    pub vented: u64,
    pub combusted: u64,
    pub absorbed: u64,
    pub current_dissolved: u64,
    pub drained: u64,
    pub spilled: u64,
    pub balanced: bool,
}

/// One trace's accounting across air and decay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceAccounting {
    pub released: u64,
    pub current_air: u64,
    pub vented: u64,
    pub decayed: u64,
    pub balanced: bool,
}

/// Air, gas and trace accounting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AirAccounting {
    pub initial_air: u64,
    pub current_air: u64,
    pub drawn_from_exterior: u64,
    pub vented_to_exterior: u64,
    pub air_balanced: bool,
    pub species: BTreeMap<GasSpeciesId, SpeciesAccounting>,
    pub species_balanced: bool,
    pub traces: BTreeMap<TraceKind, TraceAccounting>,
    pub traces_balanced: bool,
}

/// The whole accounting surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InfrastructureAccounting {
    pub water: Option<WaterAccounting>,
    pub air: Option<AirAccounting>,
    pub balanced: bool,
}

/// The deterministic infrastructure runtime.
#[derive(Debug, Clone)]
pub struct InfrastructureSim {
    def: InfrastructureDefinition,
    state: InfrastructureState,
}

impl InfrastructureSim {
    /// Start a run. Validates the definition first.
    pub fn new(def: InfrastructureDefinition) -> Result<Self, Vec<InfrastructureValidationError>> {
        def.ok()?;
        let state = InfrastructureState::initial(&def);
        Ok(Self { def, state })
    }

    /// Restore a run from a snapshot, gating on its format tag.
    pub fn restore(
        snapshot: InfrastructureSnapshot,
    ) -> Result<Self, Vec<InfrastructureValidationError>> {
        use InfrastructureValidationError as E;
        if snapshot.format != super::INFRASTRUCTURE_SNAPSHOT_FORMAT {
            return Err(vec![E::UnsupportedSnapshotFormat {
                found: snapshot.format,
            }]);
        }
        snapshot.definition.ok()?;
        if snapshot.definition.water.is_some() != snapshot.state.water.is_some() {
            return Err(vec![E::SnapshotStateMismatch {
                detail: "water section and water state disagree".to_owned(),
            }]);
        }
        if snapshot.definition.air.is_some() != snapshot.state.air.is_some() {
            return Err(vec![E::SnapshotStateMismatch {
                detail: "air section and air state disagree".to_owned(),
            }]);
        }
        Ok(Self {
            def: snapshot.definition,
            state: snapshot.state,
        })
    }

    pub fn snapshot(&self) -> InfrastructureSnapshot {
        InfrastructureSnapshot {
            format: super::INFRASTRUCTURE_SNAPSHOT_FORMAT.to_owned(),
            definition: self.def.clone(),
            state: self.state.clone(),
            model: model_declaration(),
        }
    }

    pub fn definition(&self) -> &InfrastructureDefinition {
        &self.def
    }

    pub fn state(&self) -> &InfrastructureState {
        &self.state
    }

    /// Apply one command, returning the events it produced.
    pub fn apply(&mut self, cmd: &InfrastructureCommand) -> Vec<InfrastructureEvent> {
        apply_command(&self.def, &mut self.state, cmd)
    }

    /// Advance exactly one tick.
    pub fn step(&mut self) -> Vec<InfrastructureEvent> {
        step(&self.def, &mut self.state)
    }

    /// Host sync: mirror a building door's aperture into this model. Locks are
    /// deliberately absent; a locked open door still passes air and water.
    pub fn sync_opening(&mut self, id: &OpeningId, open: bool) -> Vec<InfrastructureEvent> {
        if self.def.opening(id).is_none() {
            return vec![InfrastructureEvent::Denied {
                reason: InfrastructureDenyReason::UnknownFixture { id: id.to_string() },
            }];
        }
        let previous = self.state.openings_open.insert(id.clone(), open);
        if previous == Some(open) {
            Vec::new()
        } else {
            vec![InfrastructureEvent::OpeningSynced {
                opening: id.clone(),
                open,
            }]
        }
    }

    /// Host sync: mirror a building circuit's live state. A fan on a dead
    /// circuit stops without losing its switched-on state.
    pub fn sync_circuit(&mut self, id: &CircuitId, live: bool) -> Vec<InfrastructureEvent> {
        if !referenced_circuits(&self.def).contains(id) {
            return vec![InfrastructureEvent::Denied {
                reason: InfrastructureDenyReason::UnknownFixture { id: id.to_string() },
            }];
        }
        let Some(air) = self.state.air.as_mut() else {
            return Vec::new();
        };
        let previous = air.circuits_live.insert(id.clone(), live);
        if previous == Some(live) {
            Vec::new()
        } else {
            vec![InfrastructureEvent::CircuitSynced {
                circuit: id.clone(),
                live,
            }]
        }
    }

    /// One compartment's conditions, or `None` for an unknown/exterior id.
    pub fn conditions(&self, id: &CompartmentId) -> Option<CompartmentConditions> {
        conditions_for(&self.def, &self.state, id)
    }

    /// Every room compartment's conditions, sorted by id.
    pub fn all_conditions(&self) -> BTreeMap<CompartmentId, CompartmentConditions> {
        self.def
            .rooms()
            .filter_map(|c| {
                conditions_for(&self.def, &self.state, &c.id).map(|cond| (c.id.clone(), cond))
            })
            .collect()
    }

    /// The actual air movement through one duct on the last tick.
    pub fn duct_flow(&self, id: &DuctId) -> u64 {
        self.state
            .air
            .as_ref()
            .and_then(|air| air.duct_flow.get(id).copied())
            .unwrap_or(0)
    }

    /// Traversable openings that are currently open. The host combines these
    /// with building locks; this list is not a movement authority.
    pub fn passable_openings(&self) -> Vec<(OpeningId, CompartmentId, CompartmentId)> {
        self.def
            .openings
            .iter()
            .filter(|o| o.traversal)
            .filter(|o| {
                self.state
                    .openings_open
                    .get(&o.id)
                    .copied()
                    .unwrap_or(o.start_open)
            })
            .map(|o| (o.id.clone(), o.a.clone(), o.b.clone()))
            .collect()
    }

    /// Crawlable ducts whose hatches are open. A closed damper does not remove
    /// this route; a duct that is not authored crawlable never appears.
    pub fn crawlable_ducts(&self) -> Vec<(DuctId, CompartmentId, CompartmentId)> {
        let Some(air_def) = &self.def.air else {
            return Vec::new();
        };
        let Some(air) = &self.state.air else {
            return Vec::new();
        };
        air_def
            .ducts
            .iter()
            .filter(|d| d.crawlable)
            .filter(|d| {
                air.hatch_open
                    .get(&d.id)
                    .copied()
                    .unwrap_or(d.start_hatch_open)
            })
            .map(|d| (d.id.clone(), d.a.clone(), d.b.clone()))
            .collect()
    }

    /// The whole ledger surface. `balanced` is true iff every water, air,
    /// species and trace equation closes exactly.
    pub fn accounting(&self) -> InfrastructureAccounting {
        let water = self.state.water.as_ref().map(|w| WaterAccounting {
            initial_reservoir: w.ledger.initial_reservoir,
            recharged: w.ledger.recharged,
            current_reservoirs: w.water_in_reservoirs(),
            current_compartments: w.water_in_compartments(),
            drained_to_exterior: w.ledger.drained_to_exterior,
            spilled_to_exterior: w.ledger.spilled_to_exterior,
            balanced: w.balanced(),
        });
        let air = match (&self.def.air, &self.state.air) {
            (Some(air_def), Some(air)) => {
                let mut species = BTreeMap::new();
                let mut species_balanced = true;
                for s in &air_def.species {
                    let id = &s.id;
                    let current_dissolved: u64 = self
                        .state
                        .water
                        .as_ref()
                        .map(|w| {
                            w.bodies
                                .values()
                                .map(|b| b.dissolved.get(id).copied().unwrap_or(0))
                                .sum()
                        })
                        .unwrap_or(0);
                    let entry = SpeciesAccounting {
                        released: air.ledger.species_released.get(id).copied().unwrap_or(0),
                        current_air: air.species_in_compartments(id),
                        current_filter_load: air.filter_load_total(id),
                        filter_disposed: air.ledger.filter_disposed.get(id).copied().unwrap_or(0),
                        vented: air.ledger.species_vented.get(id).copied().unwrap_or(0),
                        combusted: air.ledger.species_combusted.get(id).copied().unwrap_or(0),
                        absorbed: air.ledger.species_absorbed.get(id).copied().unwrap_or(0),
                        current_dissolved,
                        drained: air.ledger.species_drained.get(id).copied().unwrap_or(0),
                        spilled: air.ledger.species_spilled.get(id).copied().unwrap_or(0),
                        balanced: false,
                    };
                    let balanced = entry.released
                        == entry
                            .current_air
                            .saturating_add(entry.current_filter_load)
                            .saturating_add(entry.filter_disposed)
                            .saturating_add(entry.vented)
                            .saturating_add(entry.combusted)
                            .saturating_add(entry.current_dissolved)
                            .saturating_add(entry.drained)
                            .saturating_add(entry.spilled);
                    species_balanced &= balanced;
                    species.insert(id.clone(), SpeciesAccounting { balanced, ..entry });
                }
                let mut traces = BTreeMap::new();
                let mut traces_balanced = true;
                for kind in [TraceKind::Scent, TraceKind::Noise] {
                    let entry = TraceAccounting {
                        released: air.ledger.traces_released.get(&kind).copied().unwrap_or(0),
                        current_air: air.trace_in_compartments(kind),
                        vented: air.ledger.traces_vented.get(&kind).copied().unwrap_or(0),
                        decayed: air.ledger.traces_decayed.get(&kind).copied().unwrap_or(0),
                        balanced: false,
                    };
                    let balanced = entry.released
                        == entry
                            .current_air
                            .saturating_add(entry.vented)
                            .saturating_add(entry.decayed);
                    traces_balanced &= balanced;
                    traces.insert(kind, TraceAccounting { balanced, ..entry });
                }
                Some(AirAccounting {
                    initial_air: air.ledger.initial_air,
                    current_air: air.air_in_compartments(),
                    drawn_from_exterior: air.ledger.drawn_from_exterior,
                    vented_to_exterior: air.ledger.vented_to_exterior,
                    air_balanced: air.air_balanced(),
                    species,
                    species_balanced,
                    traces,
                    traces_balanced,
                })
            }
            _ => None,
        };
        let balanced = water.as_ref().map(|w| w.balanced).unwrap_or(true)
            && air
                .as_ref()
                .map(|a| a.air_balanced && a.species_balanced && a.traces_balanced)
                .unwrap_or(true);
        InfrastructureAccounting {
            water,
            air,
            balanced,
        }
    }

    /// Every conservation failure as a human-readable line; empty means every
    /// ledger closes. Intended for tests, fuzzers and developer overlays.
    pub fn conservation_report(&self) -> Vec<String> {
        let accounting = self.accounting();
        let mut problems = Vec::new();
        if let Some(water) = &accounting.water
            && !water.balanced
        {
            problems.push(format!(
                "water: initial {} + recharged {} != reservoirs {} + compartments {} + drained {} + spilled {}",
                water.initial_reservoir,
                water.recharged,
                water.current_reservoirs,
                water.current_compartments,
                water.drained_to_exterior,
                water.spilled_to_exterior
            ));
        }
        if let Some(air) = &accounting.air {
            if !air.air_balanced {
                problems.push(format!(
                    "air: initial {} + drawn {} != current {} + vented {}",
                    air.initial_air,
                    air.drawn_from_exterior,
                    air.current_air,
                    air.vented_to_exterior
                ));
            }
            for (species, entry) in &air.species {
                if !entry.balanced {
                    problems.push(format!(
                        "species {species}: released {} != air {} + filter {} + disposed {} + vented {} + combusted {} + dissolved {} + drained {} + spilled {}",
                        entry.released,
                        entry.current_air,
                        entry.current_filter_load,
                        entry.filter_disposed,
                        entry.vented,
                        entry.combusted,
                        entry.current_dissolved,
                        entry.drained,
                        entry.spilled
                    ));
                }
            }
            for (kind, entry) in &air.traces {
                if !entry.balanced {
                    problems.push(format!(
                        "trace {kind:?}: released {} != air {} + vented {} + decayed {}",
                        entry.released, entry.current_air, entry.vented, entry.decayed
                    ));
                }
            }
        }
        problems
    }
}

// --- commands ---------------------------------------------------------------

fn denied(id: &str) -> Vec<InfrastructureEvent> {
    vec![InfrastructureEvent::Denied {
        reason: InfrastructureDenyReason::UnknownFixture { id: id.to_owned() },
    }]
}

fn apply_command(
    def: &InfrastructureDefinition,
    state: &mut InfrastructureState,
    cmd: &InfrastructureCommand,
) -> Vec<InfrastructureEvent> {
    match cmd {
        InfrastructureCommand::SetValve { valve, open } => {
            let Some(water_def) = &def.water else {
                return denied(valve.as_str());
            };
            if water_def.valve(valve).is_none() {
                return denied(valve.as_str());
            }
            let Some(water) = state.water.as_mut() else {
                return denied(valve.as_str());
            };
            water.valves_open.insert(valve.clone(), *open);
            vec![InfrastructureEvent::ValveSet {
                valve: valve.clone(),
                open: *open,
            }]
        }
        InfrastructureCommand::SetOutlet { outlet, open } => {
            let Some(water_def) = &def.water else {
                return denied(outlet.as_str());
            };
            if water_def.outlet(outlet).is_none() {
                return denied(outlet.as_str());
            }
            let Some(water) = state.water.as_mut() else {
                return denied(outlet.as_str());
            };
            water.outlets_open.insert(outlet.clone(), *open);
            vec![InfrastructureEvent::OutletSet {
                outlet: outlet.clone(),
                open: *open,
            }]
        }
        InfrastructureCommand::PatchLeak { leak } => {
            let Some(water_def) = &def.water else {
                return denied(leak.as_str());
            };
            if water_def.leak(leak).is_none() {
                return denied(leak.as_str());
            }
            let Some(water) = state.water.as_mut() else {
                return denied(leak.as_str());
            };
            water.leaks_patched.insert(leak.clone(), true);
            vec![InfrastructureEvent::LeakPatched { leak: leak.clone() }]
        }
        InfrastructureCommand::ClearDrain { drain } => {
            let Some(water_def) = &def.water else {
                return denied(drain.as_str());
            };
            if water_def.drain(drain).is_none() {
                return denied(drain.as_str());
            }
            let Some(water) = state.water.as_mut() else {
                return denied(drain.as_str());
            };
            water.drains_clear.insert(drain.clone(), true);
            vec![InfrastructureEvent::DrainCleared {
                drain: drain.clone(),
            }]
        }
        InfrastructureCommand::SetDamper { damper, position } => {
            let Some(air_def) = &def.air else {
                return denied(damper.as_str());
            };
            if air_def.damper(damper).is_none() {
                return denied(damper.as_str());
            }
            let Some(air) = state.air.as_mut() else {
                return denied(damper.as_str());
            };
            let position = (*position).min(100);
            air.damper_positions.insert(damper.clone(), position);
            vec![InfrastructureEvent::DamperSet {
                damper: damper.clone(),
                position,
            }]
        }
        InfrastructureCommand::SetFan { fan, on } => {
            let Some(air_def) = &def.air else {
                return denied(fan.as_str());
            };
            if air_def.fan(fan).is_none() {
                return denied(fan.as_str());
            }
            let Some(air) = state.air.as_mut() else {
                return denied(fan.as_str());
            };
            air.fans_on.insert(fan.clone(), *on);
            vec![InfrastructureEvent::FanSet {
                fan: fan.clone(),
                on: *on,
            }]
        }
        InfrastructureCommand::SetFanDirection { fan, direction } => {
            let Some(air_def) = &def.air else {
                return denied(fan.as_str());
            };
            let Some(fan_def) = air_def.fan(fan) else {
                return denied(fan.as_str());
            };
            if !fan_def.reversible {
                return vec![InfrastructureEvent::Denied {
                    reason: InfrastructureDenyReason::NotReversible { fan: fan.clone() },
                }];
            }
            let Some(air) = state.air.as_mut() else {
                return denied(fan.as_str());
            };
            air.fan_directions.insert(fan.clone(), *direction);
            vec![InfrastructureEvent::FanDirectionSet {
                fan: fan.clone(),
                direction: *direction,
            }]
        }
        InfrastructureCommand::ReplaceFilter { filter } => {
            let Some(air_def) = &def.air else {
                return denied(filter.as_str());
            };
            if air_def.filter(filter).is_none() {
                return denied(filter.as_str());
            }
            let Some(air) = state.air.as_mut() else {
                return denied(filter.as_str());
            };
            let disposed = air.load_of(filter);
            let captured = air.filter_captured.remove(filter).unwrap_or_default();
            for (species, amount) in captured {
                let entry = air.ledger.filter_disposed.entry(species).or_insert(0);
                *entry = entry.saturating_add(amount);
            }
            air.filter_reported_saturated.insert(filter.clone(), false);
            vec![InfrastructureEvent::FilterReplaced {
                filter: filter.clone(),
                disposed,
            }]
        }
        InfrastructureCommand::ResetEquipment { equipment } => {
            let Some(air_def) = &def.air else {
                return denied(equipment.as_str());
            };
            if air_def.equipment(equipment).is_none() {
                return denied(equipment.as_str());
            }
            let Some(air) = state.air.as_mut() else {
                return denied(equipment.as_str());
            };
            air.equipment_tripped.insert(equipment.clone(), false);
            vec![InfrastructureEvent::EquipmentReset {
                equipment: equipment.clone(),
            }]
        }
        InfrastructureCommand::SetHatch { duct, open } => {
            let Some(air_def) = &def.air else {
                return denied(duct.as_str());
            };
            let Some(duct_def) = air_def.duct(duct) else {
                return denied(duct.as_str());
            };
            if !duct_def.crawlable {
                return vec![InfrastructureEvent::Denied {
                    reason: InfrastructureDenyReason::NotCrawlable { duct: duct.clone() },
                }];
            }
            let Some(air) = state.air.as_mut() else {
                return denied(duct.as_str());
            };
            air.hatch_open.insert(duct.clone(), *open);
            vec![InfrastructureEvent::HatchSet {
                duct: duct.clone(),
                open: *open,
            }]
        }
        InfrastructureCommand::SetGasSource { source, on } => {
            let Some(air_def) = &def.air else {
                return denied(source.as_str());
            };
            if air_def.gas_source(source).is_none() {
                return denied(source.as_str());
            }
            let Some(air) = state.air.as_mut() else {
                return denied(source.as_str());
            };
            air.gas_sources_on.insert(source.clone(), *on);
            vec![InfrastructureEvent::GasSourceSet {
                source: source.clone(),
                on: *on,
            }]
        }
        InfrastructureCommand::SetTraceSource { source, on } => {
            let Some(air_def) = &def.air else {
                return denied(source.as_str());
            };
            if !air_def.trace_sources.iter().any(|s| &s.id == source) {
                return denied(source.as_str());
            }
            let Some(air) = state.air.as_mut() else {
                return denied(source.as_str());
            };
            air.trace_sources_on.insert(source.clone(), *on);
            vec![InfrastructureEvent::TraceSourceSet {
                source: source.clone(),
                on: *on,
            }]
        }
    }
}

// --- tick -------------------------------------------------------------------

fn step(
    def: &InfrastructureDefinition,
    state: &mut InfrastructureState,
) -> Vec<InfrastructureEvent> {
    state.tick = state.tick.saturating_add(1);
    let before = hazard_map(def, state);
    let mut events = Vec::new();
    settle_equipment_trips(def, state, &mut events);
    if def.water.is_some() {
        water_step(def, state, &mut events);
    }
    if def.air.is_some() {
        air_step(def, state, &mut events);
    }
    absorb_step(def, state);
    emit_hazard_changes(def, state, &before, &mut events);
    events
}

/// Equipment trips are settled from the water present at the start of the tick.
/// Cutting a circuit first is therefore a legitimate way to avoid a trip: dead
/// equipment cannot fault.
fn settle_equipment_trips(
    def: &InfrastructureDefinition,
    state: &mut InfrastructureState,
    events: &mut Vec<InfrastructureEvent>,
) {
    let (Some(air_def), Some(air)) = (&def.air, state.air.as_mut()) else {
        return;
    };
    let Some(water) = state.water.as_ref() else {
        return;
    };
    for e in &air_def.equipment {
        if air.equipment_tripped.get(&e.id).copied().unwrap_or(false) {
            continue;
        }
        let Some(trip) = &e.trip else {
            continue;
        };
        if !air.equipment_powered(e) {
            continue;
        }
        let Some(body) = water.body(&e.compartment) else {
            continue;
        };
        if body.water == 0 {
            continue;
        }
        let tier = def.water_tier(body.water);
        if tier >= trip.at && body.conductivity >= trip.conductivity_min {
            air.equipment_tripped.insert(e.id.clone(), true);
            events.push(InfrastructureEvent::EquipmentTripped {
                equipment: e.id.clone(),
                cause: TripCause::ConductiveWater,
            });
        }
    }
}

fn water_step(
    def: &InfrastructureDefinition,
    state: &mut InfrastructureState,
    events: &mut Vec<InfrastructureEvent>,
) {
    let Some(water_def) = &def.water else {
        return;
    };
    let Some(water) = state.water.as_mut() else {
        return;
    };
    for flow in water.outlet_flow.values_mut() {
        *flow = 0;
    }
    for flow in water.leak_flow.values_mut() {
        *flow = 0;
    }
    for flow in water.drain_flow.values_mut() {
        *flow = 0;
    }

    drains_step(def, state, water_def, events);
    spill_step(def, state);
    supply_step(def, state, water_def, events);
}

fn drain_available(
    def: &InfrastructureDefinition,
    state: &InfrastructureState,
    drain: &DrainId,
) -> bool {
    match (&def.air, &state.air) {
        (Some(air_def), Some(air)) => air.drain_available(air_def, drain),
        _ => true,
    }
}

fn drains_step(
    def: &InfrastructureDefinition,
    state: &mut InfrastructureState,
    water_def: &WaterNetworkDef,
    events: &mut Vec<InfrastructureEvent>,
) {
    let mut drained: Vec<(DrainId, CompartmentId, u64, BTreeMap<GasSpeciesId, u64>)> = Vec::new();
    for drain in &water_def.drains {
        let available = drain_available(def, state, &drain.id);
        let clear = state
            .water
            .as_ref()
            .and_then(|w| w.drains_clear.get(&drain.id).copied())
            .unwrap_or(drain.start_clear);
        if !clear || !available {
            continue;
        }
        let Some(water) = state.water.as_mut() else {
            continue;
        };
        let Some(body) = water.bodies.get_mut(&drain.compartment) else {
            continue;
        };
        let (taken, dissolved) = body.take_water(drain.capacity);
        if taken > 0 {
            water.ledger.drained_to_exterior =
                water.ledger.drained_to_exterior.saturating_add(taken);
            water.drain_flow.insert(drain.id.clone(), taken);
            drained.push((
                drain.id.clone(),
                drain.compartment.clone(),
                taken,
                dissolved,
            ));
        }
    }
    if let Some(air) = state.air.as_mut() {
        for (_, _, _, dissolved) in &drained {
            for (species, amount) in dissolved {
                let entry = air
                    .ledger
                    .species_drained
                    .entry(species.clone())
                    .or_insert(0);
                *entry = entry.saturating_add(*amount);
            }
        }
    }
    let _ = events;
}

fn spill_step(def: &InfrastructureDefinition, state: &mut InfrastructureState) {
    let Some(water) = state.water.as_mut() else {
        return;
    };
    let spill_units = def.tuning.water.spill_units;
    for opening in &def.openings {
        let open = state
            .openings_open
            .get(&opening.id)
            .copied()
            .unwrap_or(opening.start_open);
        if !open || opening.water_conductance == 0 {
            continue;
        }
        let a = def.compartment(&opening.a);
        let b = def.compartment(&opening.b);
        let (Some(a_def), Some(b_def)) = (a, b) else {
            continue;
        };
        let a_water = if a_def.is_exterior() {
            0
        } else {
            water.bodies.get(&opening.a).map(|w| w.water).unwrap_or(0)
        };
        let b_water = if b_def.is_exterior() {
            0
        } else {
            water.bodies.get(&opening.b).map(|w| w.water).unwrap_or(0)
        };
        let (from, to, to_exterior, from_water, to_water) = if a_water > b_water {
            (
                &opening.a,
                &opening.b,
                b_def.is_exterior(),
                a_water,
                b_water,
            )
        } else if b_water > a_water {
            (
                &opening.b,
                &opening.a,
                a_def.is_exterior(),
                b_water,
                a_water,
            )
        } else {
            continue;
        };
        if from_water <= spill_units {
            continue;
        }
        let above = from_water - spill_units;
        let half_difference = (from_water - to_water) / 2;
        let mut amount = opening.water_conductance.min(above);
        if !to_exterior {
            amount = amount.min(half_difference);
            let Some(to_c) = def.compartment(to) else {
                continue;
            };
            let free = to_c.water_capacity.saturating_sub(to_water);
            amount = amount.min(free);
        }
        if amount == 0 {
            continue;
        }
        let Some(source_body) = water.bodies.get_mut(from) else {
            continue;
        };
        let conductivity = source_body.conductivity;
        let (taken, dissolved) = source_body.take_water(amount);
        if taken == 0 {
            continue;
        }
        if to_exterior {
            water.ledger.spilled_to_exterior =
                water.ledger.spilled_to_exterior.saturating_add(taken);
            if let Some(air) = state.air.as_mut() {
                for (species, count) in dissolved {
                    let entry = air.ledger.species_spilled.entry(species).or_insert(0);
                    *entry = entry.saturating_add(count);
                }
            }
        } else if let Some(target) = water.bodies.get_mut(to) {
            target.add(taken, conductivity, dissolved);
        }
    }
}

/// A max-flow solution over one reservoir's view of the pipe graph. The water
/// in the pipes is not stored: each tick, the reservoir pushes at most its
/// supply budget through the remaining pipe capacity into open discharge
/// points. Sink allocation is deterministic (outlet/leak id order) and bounded
/// by each room's free capacity.
fn push_supply(
    def: &InfrastructureDefinition,
    water_def: &WaterNetworkDef,
    reservoir: &super::water::WaterReservoirDef,
    water: &mut WaterState,
    pipe_used: &mut BTreeMap<crate::scenario::ids::PipeId, u64>,
) -> u64 {
    let mut nodes: BTreeSet<WaterEndpoint> = BTreeSet::new();
    nodes.insert(WaterEndpoint::Reservoir(reservoir.id.clone()));
    for j in &water_def.junctions {
        nodes.insert(WaterEndpoint::Junction(j.id.clone()));
    }
    for o in &water_def.outlets {
        nodes.insert(WaterEndpoint::Outlet(o.id.clone()));
    }
    for l in &water_def.leaks {
        nodes.insert(WaterEndpoint::Leak(l.id.clone()));
    }
    for p in &water_def.pipes {
        nodes.insert(p.from.clone());
        nodes.insert(p.to.clone());
    }
    let nodes: Vec<WaterEndpoint> = nodes.into_iter().collect();
    let index: BTreeMap<WaterEndpoint, usize> = nodes
        .iter()
        .cloned()
        .enumerate()
        .map(|(i, e)| (e, i))
        .collect();
    let n = nodes.len();
    let source = n;
    let sink = n + 1;
    let mut cap = vec![vec![0u64; n + 2]; n + 2];

    let supply = water
        .reservoir_remaining
        .get(&reservoir.id)
        .copied()
        .unwrap_or(0)
        .min(reservoir.supply_per_tick);
    if supply == 0 {
        return 0;
    }
    if let Some(&i) = index.get(&WaterEndpoint::Reservoir(reservoir.id.clone())) {
        cap[source][i] = supply;
    }

    let mut pipe_edges: Vec<(usize, usize, crate::scenario::ids::PipeId, u64)> = Vec::new();
    for pipe in &water_def.pipes {
        let open = match water_def.valves.iter().find(|v| v.pipe == pipe.id) {
            Some(v) => water
                .valves_open
                .get(&v.id)
                .copied()
                .unwrap_or(v.start_open),
            None => true,
        };
        if !open {
            continue;
        }
        let used = pipe_used.get(&pipe.id).copied().unwrap_or(0);
        let remaining = pipe.capacity.saturating_sub(used);
        if remaining == 0 {
            continue;
        }
        let (Some(&from), Some(&to)) = (index.get(&pipe.from), index.get(&pipe.to)) else {
            continue;
        };
        cap[from][to] = cap[from][to].saturating_add(remaining);
        pipe_edges.push((from, to, pipe.id.clone(), remaining));
    }

    // Allocate room space across discharge points in sorted id order. This is
    // an authored deterministic policy, not a physical priority.
    let mut space: BTreeMap<CompartmentId, u64> = BTreeMap::new();
    for c in &def.compartments {
        if c.kind == super::CompartmentKind::Room {
            let held = water.bodies.get(&c.id).map(|b| b.water).unwrap_or(0);
            space.insert(c.id.clone(), c.water_capacity.saturating_sub(held));
        }
    }
    let mut sinks: Vec<(usize, WaterEndpoint, CompartmentId, u64)> = Vec::new();
    for outlet in &water_def.outlets {
        let open = water
            .outlets_open
            .get(&outlet.id)
            .copied()
            .unwrap_or(outlet.start_open);
        if !open {
            continue;
        }
        let Some(&i) = index.get(&WaterEndpoint::Outlet(outlet.id.clone())) else {
            continue;
        };
        sinks.push((
            i,
            WaterEndpoint::Outlet(outlet.id.clone()),
            outlet.compartment.clone(),
            outlet.rate,
        ));
    }
    for leak in &water_def.leaks {
        let patched = water
            .leaks_patched
            .get(&leak.id)
            .copied()
            .unwrap_or(leak.start_patched);
        if patched {
            continue;
        }
        let Some(&i) = index.get(&WaterEndpoint::Leak(leak.id.clone())) else {
            continue;
        };
        sinks.push((
            i,
            WaterEndpoint::Leak(leak.id.clone()),
            leak.compartment.clone(),
            leak.rate,
        ));
    }
    sinks.sort_by_key(|(_, endpoint, _, _)| endpoint.label());
    let mut sink_original: Vec<(usize, WaterEndpoint, u64)> = Vec::new();
    for (i, endpoint, compartment, rate) in sinks {
        let free = space.get(&compartment).copied().unwrap_or(0);
        let amount = rate.min(free);
        space.insert(compartment, free.saturating_sub(amount));
        cap[i][sink] = amount;
        sink_original.push((i, endpoint, amount));
    }

    edmonds_karp(&mut cap, source, sink);

    let mut total = 0u64;
    for (i, endpoint, original) in sink_original {
        let flow = original.saturating_sub(cap[i][sink]);
        if flow == 0 {
            continue;
        }
        total = total.saturating_add(flow);
        if let WaterEndpoint::Outlet(id) = &endpoint
            && let Some(entry) = water.outlet_flow.get_mut(id)
        {
            *entry = entry.saturating_add(flow);
        }
        if let WaterEndpoint::Leak(id) = &endpoint
            && let Some(entry) = water.leak_flow.get_mut(id)
        {
            *entry = entry.saturating_add(flow);
        }
        let compartment = match &endpoint {
            WaterEndpoint::Outlet(id) => water_def.outlet(id).map(|o| o.compartment.clone()),
            WaterEndpoint::Leak(id) => water_def.leak(id).map(|l| l.compartment.clone()),
            _ => None,
        };
        if let Some(compartment) = compartment
            && let Some(body) = water.bodies.get_mut(&compartment)
        {
            body.add(flow, reservoir.conductivity, BTreeMap::new());
        }
    }
    for (from, to, pipe, original) in pipe_edges {
        let flow = original.saturating_sub(cap[from][to]);
        if flow > 0 {
            let entry = pipe_used.entry(pipe).or_insert(0);
            *entry = entry.saturating_add(flow);
        }
    }
    if let Some(remaining) = water.reservoir_remaining.get_mut(&reservoir.id) {
        *remaining = remaining.saturating_sub(total);
    }
    total
}

fn supply_step(
    def: &InfrastructureDefinition,
    state: &mut InfrastructureState,
    water_def: &WaterNetworkDef,
    events: &mut Vec<InfrastructureEvent>,
) {
    let Some(water) = state.water.as_mut() else {
        return;
    };
    let mut pipe_used = BTreeMap::new();
    let reservoirs: Vec<super::water::WaterReservoirDef> = water_def.reservoirs.clone();
    for reservoir in reservoirs {
        let before = water
            .reservoir_remaining
            .get(&reservoir.id)
            .copied()
            .unwrap_or(0);
        if before == 0 {
            continue;
        }
        let _ = push_supply(def, water_def, &reservoir, water, &mut pipe_used);
        let after = water
            .reservoir_remaining
            .get(&reservoir.id)
            .copied()
            .unwrap_or(0);
        if before > 0 && after == 0 && water.reservoir_reported_empty.insert(reservoir.id.clone()) {
            events.push(InfrastructureEvent::ReservoirEmptied {
                reservoir: reservoir.id.clone(),
            });
        }
    }
}

/// Deterministic Edmonds-Karp. Node order is the sorted endpoint order, so the
/// augmenting paths (and therefore the allocation) are reproducible.
fn edmonds_karp(cap: &mut [Vec<u64>], source: usize, sink: usize) -> u64 {
    let n = cap.len();
    let mut total = 0u64;
    loop {
        let mut parent = vec![usize::MAX; n];
        parent[source] = source;
        let mut queue = VecDeque::new();
        queue.push_back(source);
        while let Some(u) = queue.pop_front() {
            if u == sink {
                break;
            }
            for (v, slot) in parent.iter_mut().enumerate() {
                if *slot == usize::MAX && cap[u][v] > 0 {
                    *slot = u;
                    queue.push_back(v);
                }
            }
        }
        if parent[sink] == usize::MAX {
            break;
        }
        let mut augment = u64::MAX;
        let mut v = sink;
        while v != source {
            let u = parent[v];
            augment = augment.min(cap[u][v]);
            v = u;
        }
        v = sink;
        while v != source {
            let u = parent[v];
            cap[u][v] = cap[u][v].saturating_sub(augment);
            cap[v][u] = cap[v][u].saturating_add(augment);
            v = u;
        }
        total = total.saturating_add(augment);
    }
    total
}

// --- air --------------------------------------------------------------------

fn room_air_bounds(
    def: &InfrastructureDefinition,
    compartment: &CompartmentId,
) -> Option<(u64, u64)> {
    let c = def.compartment(compartment)?;
    if c.kind != super::CompartmentKind::Room {
        return None;
    }
    let tune = &def.tuning.air;
    let min = super::proportion(c.air_volume, tune.min_pressure as u64, 1000);
    let max = super::proportion(c.air_volume, tune.max_pressure as u64, 1000);
    Some((min, max))
}

fn pressure_of(
    def: &InfrastructureDefinition,
    state: &InfrastructureState,
    id: &CompartmentId,
) -> u32 {
    let Some(c) = def.compartment(id) else {
        return PRESSURE_BASE;
    };
    if c.is_exterior() {
        return PRESSURE_BASE;
    }
    state
        .air
        .as_ref()
        .and_then(|air| air.body(id))
        .map(|b| b.pressure(c.air_volume))
        .unwrap_or(PRESSURE_BASE)
}

fn move_room_to_room(
    air: &mut AirState,
    from: &CompartmentId,
    to: &CompartmentId,
    q: u64,
    filter: Option<(&FilterId, u16, u64)>,
) -> BTreeMap<GasSpeciesId, u64> {
    let Some(mut src) = air.bodies.remove(from) else {
        return BTreeMap::new();
    };
    let Some(mut dst) = air.bodies.remove(to) else {
        air.bodies.insert(from.clone(), src);
        return BTreeMap::new();
    };
    let old_air = src.air;
    let mut captured = BTreeMap::new();
    let mut filter_room = filter.map(|(_, _, remaining)| remaining).unwrap_or(0);
    let efficiency = filter.map(|(_, efficiency, _)| efficiency).unwrap_or(0);
    for (species, amount) in std::mem::take(&mut src.species) {
        let moved = super::proportion(amount, q, old_air);
        let left = amount.saturating_sub(moved);
        if left > 0 {
            src.species.insert(species.clone(), left);
        }
        if moved == 0 {
            continue;
        }
        let proposed = super::proportion(moved, efficiency as u64, 1000).min(filter_room);
        filter_room = filter_room.saturating_sub(proposed);
        if proposed > 0 {
            captured.insert(species.clone(), proposed);
        }
        let delivered = moved.saturating_sub(proposed);
        if delivered > 0 {
            let entry = dst.species.entry(species).or_insert(0);
            *entry = entry.saturating_add(delivered);
        }
    }
    for (kind, amount) in std::mem::take(&mut src.traces) {
        let moved = super::proportion(amount, q, old_air);
        let left = amount.saturating_sub(moved);
        if left > 0 {
            src.traces.insert(kind, left);
        }
        if moved > 0 {
            let entry = dst.traces.entry(kind).or_insert(0);
            *entry = entry.saturating_add(moved);
        }
    }
    src.air = src.air.saturating_sub(q);
    dst.air = dst.air.saturating_add(q);
    air.bodies.insert(from.clone(), src);
    air.bodies.insert(to.clone(), dst);
    captured
}

fn move_room_to_exterior(
    air: &mut AirState,
    from: &CompartmentId,
    q: u64,
    filter: Option<(&FilterId, u16, u64)>,
) -> BTreeMap<GasSpeciesId, u64> {
    let Some(mut src) = air.bodies.remove(from) else {
        return BTreeMap::new();
    };
    let old_air = src.air;
    let mut captured = BTreeMap::new();
    let mut filter_room = filter.map(|(_, _, remaining)| remaining).unwrap_or(0);
    let efficiency = filter.map(|(_, efficiency, _)| efficiency).unwrap_or(0);
    for (species, amount) in std::mem::take(&mut src.species) {
        let moved = super::proportion(amount, q, old_air);
        let left = amount.saturating_sub(moved);
        if left > 0 {
            src.species.insert(species.clone(), left);
        }
        if moved == 0 {
            continue;
        }
        let proposed = super::proportion(moved, efficiency as u64, 1000).min(filter_room);
        filter_room = filter_room.saturating_sub(proposed);
        if proposed > 0 {
            captured.insert(species.clone(), proposed);
        }
        let vented = moved.saturating_sub(proposed);
        if vented > 0 {
            let entry = air.ledger.species_vented.entry(species).or_insert(0);
            *entry = entry.saturating_add(vented);
        }
    }
    for (kind, amount) in std::mem::take(&mut src.traces) {
        let moved = super::proportion(amount, q, old_air);
        let left = amount.saturating_sub(moved);
        if left > 0 {
            src.traces.insert(kind, left);
        }
        if moved > 0 {
            let entry = air.ledger.traces_vented.entry(kind).or_insert(0);
            *entry = entry.saturating_add(moved);
        }
    }
    src.air = src.air.saturating_sub(q);
    air.ledger.vented_to_exterior = air.ledger.vented_to_exterior.saturating_add(q);
    air.bodies.insert(from.clone(), src);
    captured
}

fn move_exterior_to_room(air: &mut AirState, to: &CompartmentId, q: u64) {
    let Some(mut dst) = air.bodies.remove(to) else {
        return;
    };
    dst.air = dst.air.saturating_add(q);
    air.ledger.drawn_from_exterior = air.ledger.drawn_from_exterior.saturating_add(q);
    air.bodies.insert(to.clone(), dst);
}

fn note_capture(
    air: &mut AirState,
    filter: &FilterId,
    captured: BTreeMap<GasSpeciesId, u64>,
    capacity: u64,
    events: &mut Vec<InfrastructureEvent>,
) {
    if captured.is_empty() {
        return;
    }
    let entry = air.filter_captured.entry(filter.clone()).or_default();
    for (species, amount) in captured {
        let held = entry.entry(species).or_insert(0);
        *held = held.saturating_add(amount);
    }
    if air.load_of(filter) >= capacity
        && !air
            .filter_reported_saturated
            .get(filter)
            .copied()
            .unwrap_or(false)
    {
        air.filter_reported_saturated.insert(filter.clone(), true);
        events.push(InfrastructureEvent::FilterSaturated {
            filter: filter.clone(),
        });
    }
}

fn stall(
    air: &mut AirState,
    duct: &DuctId,
    reason: FanStallReason,
    events: &mut Vec<InfrastructureEvent>,
) {
    if air.stalled.insert(duct.clone()) {
        events.push(InfrastructureEvent::AirflowStalled {
            duct: duct.clone(),
            reason,
        });
    }
}

fn release_sources(
    air_def: &AirNetworkDef,
    air: &mut AirState,
    events: &mut Vec<InfrastructureEvent>,
) {
    for source in &air_def.gas_sources {
        let on = air
            .gas_sources_on
            .get(&source.id)
            .copied()
            .unwrap_or(source.start_on);
        let remaining = air
            .gas_source_remaining
            .get(&source.id)
            .copied()
            .unwrap_or(source.reservoir);
        if !on || remaining == 0 {
            continue;
        }
        let amount = source.release_per_tick.min(remaining);
        if let Some(body) = air.bodies.get_mut(&source.compartment) {
            let held = body.species_amount(&source.species);
            body.set_species(&source.species, held.saturating_add(amount));
            air.gas_source_remaining
                .insert(source.id.clone(), remaining.saturating_sub(amount));
            let entry = air
                .ledger
                .species_released
                .entry(source.species.clone())
                .or_insert(0);
            *entry = entry.saturating_add(amount);
            if remaining == amount {
                events.push(InfrastructureEvent::GasSourceEmptied {
                    source: source.id.clone(),
                });
            }
        }
    }
    for source in &air_def.trace_sources {
        let on = air
            .trace_sources_on
            .get(&source.id)
            .copied()
            .unwrap_or(source.start_on);
        let remaining = air
            .trace_source_remaining
            .get(&source.id)
            .copied()
            .unwrap_or(source.reservoir);
        if !on || remaining == 0 {
            continue;
        }
        let amount = source.release_per_tick.min(remaining);
        if let Some(body) = air.bodies.get_mut(&source.compartment) {
            let held = body.trace_amount(source.kind);
            body.set_trace(source.kind, held.saturating_add(amount));
            air.trace_source_remaining
                .insert(source.id.clone(), remaining.saturating_sub(amount));
            let entry = air.ledger.traces_released.entry(source.kind).or_insert(0);
            *entry = entry.saturating_add(amount);
        }
    }
}

fn duct_step(
    def: &InfrastructureDefinition,
    air_def: &AirNetworkDef,
    air: &mut AirState,
    events: &mut Vec<InfrastructureEvent>,
) {
    for duct in &air_def.ducts {
        air.duct_flow.insert(duct.id.clone(), 0);
        let Some(fan) = air_def.fan_for_duct(&duct.id) else {
            continue;
        };
        let switched_on = air.fans_on.get(&fan.id).copied().unwrap_or(fan.start_on);
        if !switched_on {
            continue;
        }
        if !air.fan_running(air_def, fan) {
            stall(air, &duct.id, FanStallReason::Unpowered, events);
            continue;
        }
        let position = air_def
            .damper_for_duct(&duct.id)
            .map(|d| {
                air.damper_positions
                    .get(&d.id)
                    .copied()
                    .unwrap_or(d.start_position)
            })
            .unwrap_or(100);
        let target_flow = duct.capacity.min(fan.flow);
        let mut q = super::proportion(target_flow, position as u64, 100);
        if q == 0 {
            stall(air, &duct.id, FanStallReason::DamperClosed, events);
            continue;
        }
        let direction = air
            .fan_directions
            .get(&fan.id)
            .copied()
            .unwrap_or(fan.start_direction);
        let (from, to) = match direction {
            FlowDirection::AToB => (&duct.a, &duct.b),
            FlowDirection::BToA => (&duct.b, &duct.a),
        };
        let from_room = def
            .compartment(from)
            .map(|c| !c.is_exterior())
            .unwrap_or(false);
        let to_room = def
            .compartment(to)
            .map(|c| !c.is_exterior())
            .unwrap_or(false);
        if from_room && let Some((min, _)) = room_air_bounds(def, from) {
            let available = air
                .body(from)
                .map(|b| b.air.saturating_sub(min))
                .unwrap_or(0);
            q = q.min(available);
        }
        if to_room && let Some((_, max)) = room_air_bounds(def, to) {
            let space = air.body(to).map(|b| max.saturating_sub(b.air)).unwrap_or(0);
            q = q.min(space);
        }
        if q == 0 {
            let reason = if from_room {
                FanStallReason::SourceStarved
            } else {
                FanStallReason::TargetFull
            };
            stall(air, &duct.id, reason, events);
            continue;
        }
        let filter = air_def.filter_for_duct(&duct.id);
        let filter_info = filter.map(|f| (f.id.clone(), f.efficiency, f.capacity));
        let filter_ref = filter_info.as_ref().map(|(id, efficiency, capacity)| {
            (id, *efficiency, capacity.saturating_sub(air.load_of(id)))
        });
        let captured = match (from_room, to_room) {
            (true, true) => move_room_to_room(air, from, to, q, filter_ref),
            (true, false) => move_room_to_exterior(air, from, q, filter_ref),
            (false, true) => {
                move_exterior_to_room(air, to, q);
                BTreeMap::new()
            }
            (false, false) => BTreeMap::new(),
        };
        if let Some((filter_id, _, capacity)) = filter_info {
            note_capture(air, &filter_id, captured, capacity, events);
        }
        air.duct_flow.insert(duct.id.clone(), q);
        air.stalled.remove(&duct.id);
    }
}

fn openings_step(def: &InfrastructureDefinition, state: &mut InfrastructureState) {
    for opening in &def.openings {
        let open = state
            .openings_open
            .get(&opening.id)
            .copied()
            .unwrap_or(opening.start_open);
        if !open || opening.air_conductance == 0 {
            continue;
        }
        let pa = pressure_of(def, state, &opening.a);
        let pb = pressure_of(def, state, &opening.b);
        if pa == pb {
            continue;
        }
        let (from, to, dp) = if pa > pb {
            (&opening.a, &opening.b, pa - pb)
        } else {
            (&opening.b, &opening.a, pb - pa)
        };
        let mut q = super::proportion(opening.air_conductance, dp as u64, 1000);
        if q == 0 {
            continue;
        }
        let from_room = def
            .compartment(from)
            .map(|c| !c.is_exterior())
            .unwrap_or(false);
        let to_room = def
            .compartment(to)
            .map(|c| !c.is_exterior())
            .unwrap_or(false);
        if from_room && let Some((min, _)) = room_air_bounds(def, from) {
            let available = state
                .air
                .as_ref()
                .and_then(|air| air.body(from))
                .map(|b| b.air.saturating_sub(min))
                .unwrap_or(0);
            q = q.min(available);
        }
        if to_room && let Some((_, max)) = room_air_bounds(def, to) {
            let space = state
                .air
                .as_ref()
                .and_then(|air| air.body(to))
                .map(|b| max.saturating_sub(b.air))
                .unwrap_or(0);
            q = q.min(space);
        }
        if q == 0 {
            continue;
        }
        let Some(air) = state.air.as_mut() else {
            continue;
        };
        match (from_room, to_room) {
            (true, true) => {
                let _ = move_room_to_room(air, from, to, q, None);
            }
            (true, false) => {
                let _ = move_room_to_exterior(air, from, q, None);
            }
            (false, true) => move_exterior_to_room(air, to, q),
            (false, false) => {}
        }
    }
}

fn decay_traces(def: &InfrastructureDefinition, air: &mut AirState) {
    let per_mille = def.tuning.air.trace_decay_per_mille as u64;
    if per_mille == 0 {
        return;
    }
    let mut decayed: BTreeMap<TraceKind, u64> = BTreeMap::new();
    for body in air.bodies.values_mut() {
        for kind in [TraceKind::Scent, TraceKind::Noise] {
            let amount = body.trace_amount(kind);
            if amount == 0 {
                continue;
            }
            let removed = super::proportion(amount, per_mille, 1000)
                .max(1)
                .min(amount);
            body.set_trace(kind, amount.saturating_sub(removed));
            let entry = decayed.entry(kind).or_insert(0);
            *entry = entry.saturating_add(removed);
        }
    }
    for (kind, amount) in decayed {
        let entry = air.ledger.traces_decayed.entry(kind).or_insert(0);
        *entry = entry.saturating_add(amount);
    }
}

fn combustion_step(
    def: &InfrastructureDefinition,
    air_def: &AirNetworkDef,
    air: &mut AirState,
    events: &mut Vec<InfrastructureEvent>,
) {
    for equipment in &air_def.equipment {
        if !equipment.spark_when_live {
            continue;
        }
        if air
            .equipment_tripped
            .get(&equipment.id)
            .copied()
            .unwrap_or(false)
        {
            continue;
        }
        if !air.equipment_powered(equipment) {
            continue;
        }
        let Some(body) = air.bodies.get_mut(&equipment.compartment) else {
            continue;
        };
        let mut burned: Vec<(GasSpeciesId, u64)> = Vec::new();
        for species in &air_def.species {
            if !species.class.flammable() {
                continue;
            }
            let amount = body.species_amount(&species.id);
            if amount == 0 {
                continue;
            }
            if species.tier(body.concentration(&species.id)) != Tier::Critical {
                continue;
            }
            body.set_species(&species.id, 0);
            burned.push((species.id.clone(), amount));
        }
        if burned.is_empty() {
            continue;
        }
        for (species, amount) in &burned {
            let entry = air
                .ledger
                .species_combusted
                .entry(species.clone())
                .or_insert(0);
            *entry = entry.saturating_add(*amount);
            events.push(InfrastructureEvent::Combustion {
                compartment: equipment.compartment.clone(),
                species: species.clone(),
                amount: *amount,
            });
        }
        events.push(InfrastructureEvent::Cue {
            compartment: equipment.compartment.clone(),
            cue: CueKind::Ignition,
            tier: Tier::Critical,
        });
        for other in &air_def.equipment {
            if other.compartment != equipment.compartment {
                continue;
            }
            if air
                .equipment_tripped
                .get(&other.id)
                .copied()
                .unwrap_or(false)
            {
                continue;
            }
            air.equipment_tripped.insert(other.id.clone(), true);
            events.push(InfrastructureEvent::EquipmentTripped {
                equipment: other.id.clone(),
                cause: TripCause::Ignition,
            });
        }
    }
    let _ = def;
}

fn absorb_step(def: &InfrastructureDefinition, state: &mut InfrastructureState) {
    let (Some(air_def), Some(air), Some(water)) =
        (&def.air, state.air.as_mut(), state.water.as_mut())
    else {
        return;
    };
    let mut absorbed: BTreeMap<GasSpeciesId, u64> = BTreeMap::new();
    for (compartment, body) in &mut air.bodies {
        let Some(water_body) = water.bodies.get_mut(compartment) else {
            continue;
        };
        if water_body.water == 0 {
            continue;
        }
        for species in &air_def.species {
            if species.solubility == 0 {
                continue;
            }
            let in_air = body.species_amount(&species.id);
            if in_air == 0 {
                continue;
            }
            let capacity = super::proportion(water_body.water, species.solubility as u64, 1000);
            let absorb = in_air.min(capacity);
            if absorb == 0 {
                continue;
            }
            body.set_species(&species.id, in_air.saturating_sub(absorb));
            water_body.dissolve(&species.id, absorb);
            let entry = absorbed.entry(species.id.clone()).or_insert(0);
            *entry = entry.saturating_add(absorb);
        }
    }
    for (species, amount) in absorbed {
        let entry = air.ledger.species_absorbed.entry(species).or_insert(0);
        *entry = entry.saturating_add(amount);
    }
}

fn air_step(
    def: &InfrastructureDefinition,
    state: &mut InfrastructureState,
    events: &mut Vec<InfrastructureEvent>,
) {
    let Some(air_def) = &def.air else {
        return;
    };
    if let Some(air) = state.air.as_mut() {
        release_sources(air_def, air, events);
        duct_step(def, air_def, air, events);
    }
    openings_step(def, state);
    if let Some(air) = state.air.as_mut() {
        decay_traces(def, air);
        combustion_step(def, air_def, air, events);
    }
}

// --- hazards and readouts ---------------------------------------------------

fn pressure_tier(def: &InfrastructureDefinition, pressure: u32) -> Tier {
    let min = def.tuning.air.min_pressure;
    let max = def.tuning.air.max_pressure;
    if pressure <= min.saturating_add(100) || pressure >= max.saturating_sub(100) {
        Tier::Critical
    } else if pressure <= min.saturating_add(200) || pressure >= max.saturating_sub(200) {
        Tier::Warning
    } else if pressure <= min.saturating_add(300) || pressure >= max.saturating_sub(300) {
        Tier::Trace
    } else {
        Tier::Clear
    }
}

fn hazard_map(
    def: &InfrastructureDefinition,
    state: &InfrastructureState,
) -> BTreeMap<(CompartmentId, HazardKind), Tier> {
    let mut out = BTreeMap::new();
    for c in &def.compartments {
        if c.is_exterior() {
            continue;
        }
        let water_units = state
            .water
            .as_ref()
            .and_then(|water| water.body(&c.id))
            .map(|body| body.water)
            .unwrap_or(0);
        out.insert(
            (c.id.clone(), HazardKind::Water),
            def.water_tier(water_units),
        );
        let pressure = pressure_of(def, state, &c.id);
        out.insert(
            (c.id.clone(), HazardKind::Pressure),
            pressure_tier(def, pressure),
        );
        let mut visibility = Tier::Clear;
        if let Some(air_def) = &def.air {
            for species in &air_def.species {
                let (units, air_units) = state
                    .air
                    .as_ref()
                    .and_then(|air| air.body(&c.id))
                    .map(|body| (body.species_amount(&species.id), body.air))
                    .unwrap_or((0, 0));
                let concentration = super::proportion(units, 1000, air_units);
                let tier = species.tier(concentration);
                out.insert((c.id.clone(), HazardKind::Gas(species.id.clone())), tier);
                if species.class.obscures() {
                    visibility = visibility.max(tier);
                }
            }
        }
        out.insert((c.id.clone(), HazardKind::Visibility), visibility);
    }
    out
}

fn cue_for(def: &InfrastructureDefinition, hazard: &HazardKind) -> CueKind {
    match hazard {
        HazardKind::Gas(species) => def
            .air
            .as_ref()
            .and_then(|air| air.species(species))
            .map(|s| s.cue)
            .unwrap_or(CueKind::Hiss),
        other => other.cue(),
    }
}

fn emit_hazard_changes(
    def: &InfrastructureDefinition,
    state: &InfrastructureState,
    before: &BTreeMap<(CompartmentId, HazardKind), Tier>,
    events: &mut Vec<InfrastructureEvent>,
) {
    let after = hazard_map(def, state);
    for (key, tier) in &after {
        let previous = before.get(key).copied().unwrap_or(Tier::Clear);
        if previous == *tier {
            continue;
        }
        let (compartment, hazard) = key;
        events.push(InfrastructureEvent::HazardChanged {
            compartment: compartment.clone(),
            hazard: hazard.clone(),
            tier: *tier,
            cue: cue_for(def, hazard),
        });
    }
}

fn conditions_for(
    def: &InfrastructureDefinition,
    state: &InfrastructureState,
    id: &CompartmentId,
) -> Option<CompartmentConditions> {
    let c = def.compartment(id)?;
    if c.is_exterior() {
        return None;
    }
    let water_body = state.water.as_ref().and_then(|water| water.body(id));
    let water_units = water_body.map(|b| b.water).unwrap_or(0);
    let conductivity = water_body.map(|b| b.conductivity).unwrap_or(0);
    let dissolved_units = water_body.map(|b| b.dissolved_total()).unwrap_or(0);
    let air_body = state.air.as_ref().and_then(|air| air.body(id));
    let air_units = air_body.map(|b| b.air).unwrap_or(c.air_volume);
    let pressure = air_body
        .map(|b| b.pressure(c.air_volume))
        .unwrap_or(PRESSURE_BASE);
    let pressure_tier = pressure_tier(def, pressure);
    let water_tier = def.water_tier(water_units);
    let mut gas = BTreeMap::new();
    let mut visibility = Tier::Clear;
    let mut cues: BTreeMap<CueKind, Tier> = BTreeMap::new();
    if water_tier > Tier::Clear {
        cues.insert(CueKind::Puddle, water_tier);
    }
    if pressure_tier > Tier::Clear {
        cues.insert(CueKind::PressureWhine, pressure_tier);
    }
    if let Some(air_def) = &def.air {
        for species in &air_def.species {
            let units = air_body
                .map(|body| body.species_amount(&species.id))
                .unwrap_or(0);
            let concentration = super::proportion(units, 1000, air_units);
            let tier = species.tier(concentration);
            if tier > Tier::Clear {
                let entry = cues.entry(species.cue).or_insert(Tier::Clear);
                *entry = (*entry).max(tier);
            }
            if species.class.obscures() {
                visibility = visibility.max(tier);
            }
            gas.insert(
                species.id.clone(),
                GasReading {
                    units,
                    concentration,
                    tier,
                },
            );
        }
    }
    if visibility > Tier::Clear {
        let entry = cues.entry(CueKind::Haze).or_insert(Tier::Clear);
        *entry = (*entry).max(visibility);
    }
    Some(CompartmentConditions {
        compartment: id.clone(),
        air_units,
        pressure,
        pressure_tier,
        water_units,
        water_tier,
        conductivity,
        dissolved_units,
        gas,
        visibility,
        traversal_blocked: def.water_blocks_traversal(water_units),
        cues,
        scent: air_body
            .map(|b| b.trace_amount(TraceKind::Scent))
            .unwrap_or(0),
        noise: air_body
            .map(|b| b.trace_amount(TraceKind::Noise))
            .unwrap_or(0),
    })
}

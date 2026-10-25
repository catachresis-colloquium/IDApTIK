//! The ventilation model: ducts, fans, dampers, filters, fictional gas
//! species, and the scent/noise traces carried along with the air.
//!
//! # Deliberate simplifications
//!
//! A room has a fixed authored air volume and a pressure scalar in per-mille
//! units. Ducts move air only when an authored fan runs; there is no passive
//! stack effect, no duct friction and no density model. Openings equalise
//! pressure passively. Air is conserved: every transfer is clamped by the
//! source's available air and the destination's free volume, and every crossing
//! of an exterior boundary is recorded. Gas species and traces ride in that
//! air proportionally; filters remove a bounded fraction and record what they
//! capture. See the parent module for the full model declaration.

use super::{
    CompartmentId, CompartmentKind, CueKind, GasSpeciesId, HazardKind, InfrastructureDefinition,
    InfrastructureValidationError, MAX_QUANTITY, PRESSURE_BASE, Tier, TraceKind, TraceSourceId,
    check_qty, referenced_circuits,
};
use crate::scenario::ids::{
    CircuitId, DamperId, DrainId, DuctId, EquipmentId, FanId, FilterId, GasSourceId,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Per-mille efficiency/fraction cap.
pub const MAX_PER_MILLE: u16 = 1000;

/// The explicitly fictional gas categories. Only `Ember` is flammable; `Haze`
/// obscures vision; `Hush` is a calmant-like category whose only modelled
/// consequence is the tier the host applies to NPC alertness. None of these is
/// a real chemical or a real exposure limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GasClass {
    Haze,
    Hush,
    Ember,
}

impl GasClass {
    /// Only the fictional `Ember` category is flammable.
    pub fn flammable(self) -> bool {
        matches!(self, GasClass::Ember)
    }

    /// Whether this category reduces visibility when it reaches a tier.
    pub fn obscures(self) -> bool {
        matches!(self, GasClass::Haze)
    }

    /// The cue this category always emits.
    pub fn cue(self) -> CueKind {
        match self {
            GasClass::Haze => CueKind::Haze,
            GasClass::Hush | GasClass::Ember => CueKind::Hiss,
        }
    }
}

/// One fictional gas species. Thresholds are *per-mille of the compartment's
/// air units*, not ppm, LEL, or any real concentration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GasSpeciesDef {
    pub id: GasSpeciesId,
    pub name: String,
    pub class: GasClass,
    /// Per-mille solubility factor used by the water surface sink. 0 means the
    /// species does not dissolve into standing water.
    pub solubility: u16,
    pub trace_concentration: u16,
    pub warning_concentration: u16,
    pub critical_concentration: u16,
    /// The cue this species emits; defaults to the class cue in authoring.
    pub cue: CueKind,
}

impl GasSpeciesDef {
    /// The tier a concentration (per-mille) represents for this species.
    pub fn tier(&self, concentration: u64) -> Tier {
        if concentration >= self.critical_concentration as u64 {
            Tier::Critical
        } else if concentration >= self.warning_concentration as u64 {
            Tier::Warning
        } else if concentration >= self.trace_concentration as u64 {
            Tier::Trace
        } else {
            Tier::Clear
        }
    }

    /// The hazard axis this species occupies.
    pub fn hazard(&self) -> HazardKind {
        HazardKind::Gas(self.id.clone())
    }
}

/// Direction of a duct's fan-driven flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowDirection {
    AToB,
    BToA,
}

impl FlowDirection {
    pub fn reversed(self) -> Self {
        match self {
            FlowDirection::AToB => FlowDirection::BToA,
            FlowDirection::BToA => FlowDirection::AToB,
        }
    }
}

/// One duct segment joining two compartments. A duct is a media path first; it
/// is a crawl route only when `crawlable` **and** its hatch is open. Those are
/// independent facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuctDef {
    pub id: DuctId,
    pub name: String,
    pub a: CompartmentId,
    pub b: CompartmentId,
    /// Maximum air units this duct can move per tick.
    pub capacity: u64,
    /// People can crawl this duct when the hatch is open.
    pub crawlable: bool,
    pub start_hatch_open: bool,
}

impl DuctDef {
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

/// A fan on exactly one duct. A fan never creates air; it biases movement from
/// one compartment to the other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FanDef {
    pub id: FanId,
    pub name: String,
    pub duct: DuctId,
    /// Maximum air units moved per tick before the damper and duct capacity.
    pub flow: u64,
    /// The building circuit that powers it. `None` means always powered (a
    /// hand-cranked or fixture-local fan).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub circuit: Option<CircuitId>,
    pub start_on: bool,
    pub reversible: bool,
    pub start_direction: FlowDirection,
}

/// A damper on one duct. Position is 0 (closed) to 100 (open).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DamperDef {
    pub id: DamperId,
    pub name: String,
    pub duct: DuctId,
    pub start_position: u8,
}

/// A filter on one duct. Efficiency is per-mille; load is captured gas units
/// and never exceeds capacity. A saturated filter stops cleaning but still
/// passes air.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterDef {
    pub id: FilterId,
    pub name: String,
    pub duct: DuctId,
    /// Fraction of each passing species captured, in per-mille.
    pub efficiency: u16,
    /// Total captured gas units before saturation. Filters are authored clean;
    /// per-species capture is tracked so conservation stays exact.
    pub capacity: u64,
}

/// A finite gas source. Its reservoir is explicit; when empty it stops
/// releasing and can never produce more.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GasSourceDef {
    pub id: GasSourceId,
    pub name: String,
    pub species: GasSpeciesId,
    pub compartment: CompartmentId,
    pub reservoir: u64,
    pub release_per_tick: u64,
    pub start_on: bool,
}

/// A finite scent or noise source. Traces are stylised airborne cues, not
/// acoustics or chemistry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceSourceDef {
    pub id: TraceSourceId,
    pub name: String,
    pub kind: TraceKind,
    pub compartment: CompartmentId,
    pub reservoir: u64,
    pub release_per_tick: u64,
    pub start_on: bool,
}

/// The fixture a powered equipment item controls. A tripped item disables its
/// target until reset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum EquipmentTarget {
    Fan(FanId),
    Drain(DrainId),
}

/// The conditions under which a fixture trips.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EquipmentTrip {
    /// Water at or above this tier...
    pub at: Tier,
    /// ...and at least this conductivity (per-mille) trips the fixture.
    pub conductivity_min: u16,
}

/// A powered fixture that can trip. Trips are state, not destruction: a reset
/// restores it if (and only if) the hazard has receded by the next tick.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EquipmentDef {
    pub id: EquipmentId,
    pub name: String,
    pub compartment: CompartmentId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub circuit: Option<CircuitId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controls: Option<EquipmentTarget>,
    /// A live, untripped fixture in this compartment can ignite flammable gas.
    pub spark_when_live: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trip: Option<EquipmentTrip>,
    pub start_tripped: bool,
}

/// The authored ventilation system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AirNetworkDef {
    pub species: Vec<GasSpeciesDef>,
    pub ducts: Vec<DuctDef>,
    pub fans: Vec<FanDef>,
    pub dampers: Vec<DamperDef>,
    pub filters: Vec<FilterDef>,
    pub gas_sources: Vec<GasSourceDef>,
    pub trace_sources: Vec<TraceSourceDef>,
    pub equipment: Vec<EquipmentDef>,
}

impl AirNetworkDef {
    pub fn species(&self, id: &GasSpeciesId) -> Option<&GasSpeciesDef> {
        self.species.iter().find(|s| &s.id == id)
    }

    pub fn duct(&self, id: &DuctId) -> Option<&DuctDef> {
        self.ducts.iter().find(|d| &d.id == id)
    }

    pub fn fan(&self, id: &FanId) -> Option<&FanDef> {
        self.fans.iter().find(|f| &f.id == id)
    }

    pub fn damper(&self, id: &DamperId) -> Option<&DamperDef> {
        self.dampers.iter().find(|d| &d.id == id)
    }

    pub fn filter(&self, id: &FilterId) -> Option<&FilterDef> {
        self.filters.iter().find(|f| &f.id == id)
    }

    pub fn gas_source(&self, id: &GasSourceId) -> Option<&GasSourceDef> {
        self.gas_sources.iter().find(|s| &s.id == id)
    }

    pub fn equipment(&self, id: &EquipmentId) -> Option<&EquipmentDef> {
        self.equipment.iter().find(|e| &e.id == id)
    }

    pub fn fan_for_duct(&self, duct: &DuctId) -> Option<&FanDef> {
        self.fans.iter().find(|f| &f.duct == duct)
    }

    pub fn damper_for_duct(&self, duct: &DuctId) -> Option<&DamperDef> {
        self.dampers.iter().find(|d| &d.duct == duct)
    }

    pub fn filter_for_duct(&self, duct: &DuctId) -> Option<&FilterDef> {
        self.filters.iter().find(|f| &f.duct == duct)
    }

    /// The equipment item that controls this fan, if any.
    pub fn equipment_for_fan(&self, fan: &FanId) -> Option<&EquipmentDef> {
        self.equipment
            .iter()
            .find(|e| matches!(&e.controls, Some(EquipmentTarget::Fan(id)) if id == fan))
    }

    /// The equipment item that controls this drain, if any.
    pub fn equipment_for_drain(&self, drain: &DrainId) -> Option<&EquipmentDef> {
        self.equipment
            .iter()
            .find(|e| matches!(&e.controls, Some(EquipmentTarget::Drain(id)) if id == drain))
    }
}

/// Ventilation tuning; all values are stylised game parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AirTuning {
    /// Lower pressure bound (per-mille) before outflow is refused.
    pub min_pressure: u32,
    /// Upper pressure bound (per-mille) before inflow is refused.
    pub max_pressure: u32,
    /// Fraction of each trace that decays per tick, in per-mille.
    pub trace_decay_per_mille: u16,
}

impl Default for AirTuning {
    fn default() -> Self {
        Self {
            min_pressure: 200,
            max_pressure: 2000,
            trace_decay_per_mille: 20,
        }
    }
}

/// One compartment's air and carried contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AirBody {
    /// Stylised air units. Fixed-volume compartments still let this vary within
    /// the pressure bounds, which is how backpressure is represented.
    pub air: u64,
    pub species: BTreeMap<GasSpeciesId, u64>,
    pub traces: BTreeMap<TraceKind, u64>,
}

impl AirBody {
    pub fn initial(air_volume: u64) -> Self {
        Self {
            air: air_volume,
            species: BTreeMap::new(),
            traces: BTreeMap::new(),
        }
    }

    /// The body's pressure in per-mille units.
    pub fn pressure(&self, volume: u64) -> u32 {
        if volume == 0 {
            return PRESSURE_BASE;
        }
        super::proportion(self.air, PRESSURE_BASE as u64, volume) as u32
    }

    /// A species amount in the carrier.
    pub fn species_amount(&self, id: &GasSpeciesId) -> u64 {
        self.species.get(id).copied().unwrap_or(0)
    }

    /// The amount of a trace currently present.
    pub fn trace_amount(&self, kind: TraceKind) -> u64 {
        self.traces.get(&kind).copied().unwrap_or(0)
    }

    /// The concentration of a species in per-mille of this body's air.
    pub fn concentration(&self, id: &GasSpeciesId) -> u64 {
        super::proportion(self.species_amount(id), 1000, self.air)
    }

    /// Add or remove a species amount without changing the air carrier.
    pub fn set_species(&mut self, id: &GasSpeciesId, amount: u64) {
        if amount == 0 {
            self.species.remove(id);
        } else {
            self.species.insert(id.clone(), amount);
        }
    }

    /// Add or remove a trace amount.
    pub fn set_trace(&mut self, kind: TraceKind, amount: u64) {
        if amount == 0 {
            self.traces.remove(&kind);
        } else {
            self.traces.insert(kind, amount);
        }
    }
}

/// Air, gas and trace ledger. It accounts for every boundary crossing and every
/// sink so that no material can appear or vanish unnoticed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AirLedger {
    pub initial_air: u64,
    pub drawn_from_exterior: u64,
    pub vented_to_exterior: u64,
    pub species_released: BTreeMap<GasSpeciesId, u64>,
    pub species_vented: BTreeMap<GasSpeciesId, u64>,
    pub species_combusted: BTreeMap<GasSpeciesId, u64>,
    pub species_absorbed: BTreeMap<GasSpeciesId, u64>,
    pub species_drained: BTreeMap<GasSpeciesId, u64>,
    pub species_spilled: BTreeMap<GasSpeciesId, u64>,
    pub filter_disposed: BTreeMap<GasSpeciesId, u64>,
    pub traces_released: BTreeMap<TraceKind, u64>,
    pub traces_vented: BTreeMap<TraceKind, u64>,
    pub traces_decayed: BTreeMap<TraceKind, u64>,
}

/// Live ventilation state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AirState {
    pub bodies: BTreeMap<CompartmentId, AirBody>,
    pub fans_on: BTreeMap<FanId, bool>,
    pub fan_directions: BTreeMap<FanId, FlowDirection>,
    pub damper_positions: BTreeMap<DamperId, u8>,
    /// Captured gas per filter and species. The filter's total load is their
    /// sum and is bounded by its authored capacity.
    pub filter_captured: BTreeMap<FilterId, BTreeMap<GasSpeciesId, u64>>,
    pub filter_reported_saturated: BTreeMap<FilterId, bool>,
    pub hatch_open: BTreeMap<DuctId, bool>,
    pub gas_sources_on: BTreeMap<GasSourceId, bool>,
    pub gas_source_remaining: BTreeMap<GasSourceId, u64>,
    pub trace_sources_on: BTreeMap<TraceSourceId, bool>,
    pub trace_source_remaining: BTreeMap<TraceSourceId, u64>,
    pub equipment_tripped: BTreeMap<EquipmentId, bool>,
    pub circuits_live: BTreeMap<CircuitId, bool>,
    pub duct_flow: BTreeMap<DuctId, u64>,
    pub stalled: BTreeSet<DuctId>,
    pub ledger: AirLedger,
}

impl AirState {
    pub fn initial(def: &InfrastructureDefinition, air: &AirNetworkDef) -> Self {
        let mut bodies = BTreeMap::new();
        let mut initial_air = 0u64;
        for c in &def.compartments {
            if c.kind == CompartmentKind::Room {
                bodies.insert(c.id.clone(), AirBody::initial(c.air_volume));
                initial_air = initial_air.saturating_add(c.air_volume);
            }
        }
        Self {
            bodies,
            fans_on: air
                .fans
                .iter()
                .map(|f| (f.id.clone(), f.start_on))
                .collect(),
            fan_directions: air
                .fans
                .iter()
                .map(|f| (f.id.clone(), f.start_direction))
                .collect(),
            damper_positions: air
                .dampers
                .iter()
                .map(|d| (d.id.clone(), d.start_position))
                .collect(),
            filter_captured: air
                .filters
                .iter()
                .map(|f| (f.id.clone(), BTreeMap::new()))
                .collect(),
            filter_reported_saturated: air.filters.iter().map(|f| (f.id.clone(), false)).collect(),
            hatch_open: air
                .ducts
                .iter()
                .map(|d| (d.id.clone(), d.start_hatch_open))
                .collect(),
            gas_sources_on: air
                .gas_sources
                .iter()
                .map(|s| (s.id.clone(), s.start_on))
                .collect(),
            gas_source_remaining: air
                .gas_sources
                .iter()
                .map(|s| (s.id.clone(), s.reservoir))
                .collect(),
            trace_sources_on: air
                .trace_sources
                .iter()
                .map(|s| (s.id.clone(), s.start_on))
                .collect(),
            trace_source_remaining: air
                .trace_sources
                .iter()
                .map(|s| (s.id.clone(), s.reservoir))
                .collect(),
            equipment_tripped: air
                .equipment
                .iter()
                .map(|e| (e.id.clone(), e.start_tripped))
                .collect(),
            circuits_live: referenced_circuits(def)
                .into_iter()
                .map(|c| (c, true))
                .collect(),
            duct_flow: air.ducts.iter().map(|d| (d.id.clone(), 0)).collect(),
            stalled: BTreeSet::new(),
            ledger: AirLedger {
                initial_air,
                ..AirLedger::default()
            },
        }
    }

    pub fn body(&self, id: &CompartmentId) -> Option<&AirBody> {
        self.bodies.get(id)
    }

    pub fn body_mut(&mut self, id: &CompartmentId) -> Option<&mut AirBody> {
        self.bodies.get_mut(id)
    }

    /// Total air currently in room compartments.
    pub fn air_in_compartments(&self) -> u64 {
        self.bodies.values().map(|b| b.air).sum()
    }

    /// Total of one species currently in room compartments.
    pub fn species_in_compartments(&self, species: &GasSpeciesId) -> u64 {
        self.bodies
            .values()
            .map(|b| b.species_amount(species))
            .sum()
    }

    /// Current captured load across all filters for one species.
    pub fn filter_load_total(&self, species: &GasSpeciesId) -> u64 {
        self.filter_captured
            .values()
            .map(|captured| captured.get(species).copied().unwrap_or(0))
            .sum()
    }

    /// Current captured load for a filter, across all species.
    pub fn load_of(&self, filter: &FilterId) -> u64 {
        self.filter_captured
            .get(filter)
            .map(|captured| captured.values().copied().sum())
            .unwrap_or(0)
    }

    /// Captured amount of one species in one filter.
    pub fn captured_of(&self, filter: &FilterId, species: &GasSpeciesId) -> u64 {
        self.filter_captured
            .get(filter)
            .and_then(|captured| captured.get(species).copied())
            .unwrap_or(0)
    }

    /// Total of one trace in room compartments.
    pub fn trace_in_compartments(&self, kind: TraceKind) -> u64 {
        self.bodies.values().map(|b| b.trace_amount(kind)).sum()
    }

    /// Whether a fan is running: switched on, powered, and not disabled by a
    /// tripped equipment item.
    pub fn fan_running(&self, air: &AirNetworkDef, fan: &FanDef) -> bool {
        let on = self.fans_on.get(&fan.id).copied().unwrap_or(fan.start_on);
        if !on {
            return false;
        }
        let powered = match &fan.circuit {
            None => true,
            Some(circuit) => self.circuits_live.get(circuit).copied().unwrap_or(true),
        };
        let tripped = air
            .equipment_for_fan(&fan.id)
            .is_some_and(|e| self.equipment_tripped.get(&e.id).copied().unwrap_or(false));
        powered && !tripped
    }

    /// Whether a drain is unobstructed by tripped equipment.
    pub fn drain_available(&self, air: &AirNetworkDef, drain: &DrainId) -> bool {
        !air.equipment_for_drain(drain)
            .is_some_and(|e| self.equipment_tripped.get(&e.id).copied().unwrap_or(false))
    }

    /// Whether an equipment item's circuit is live.
    pub fn equipment_powered(&self, equipment: &EquipmentDef) -> bool {
        match &equipment.circuit {
            None => true,
            Some(circuit) => self.circuits_live.get(circuit).copied().unwrap_or(true),
        }
    }

    /// Whether the air ledger balances exactly.
    pub fn air_balanced(&self) -> bool {
        self.ledger
            .initial_air
            .saturating_add(self.ledger.drawn_from_exterior)
            == self
                .air_in_compartments()
                .saturating_add(self.ledger.vented_to_exterior)
    }
}

/// Validate a ventilation network against its containing definition.
pub(crate) fn validate_air(
    def: &InfrastructureDefinition,
    air: &AirNetworkDef,
    errs: &mut Vec<InfrastructureValidationError>,
) {
    use InfrastructureValidationError as E;

    let mut species = BTreeSet::new();
    for s in &air.species {
        if !species.insert(s.id.clone()) {
            errs.push(E::DuplicateSpecies { id: s.id.clone() });
        }
        if !(s.trace_concentration > 0
            && s.trace_concentration < s.warning_concentration
            && s.warning_concentration < s.critical_concentration)
        {
            errs.push(E::SpeciesThresholdOrder { id: s.id.clone() });
        }
        if s.solubility > MAX_PER_MILLE {
            errs.push(E::SpeciesSolubilityInvalid { id: s.id.clone() });
        }
    }

    let mut ducts = BTreeSet::new();
    for d in &air.ducts {
        if !ducts.insert(d.id.clone()) {
            errs.push(E::DuplicateDuct { id: d.id.clone() });
        }
        let mut exterior_count = 0;
        for end in [&d.a, &d.b] {
            match def.compartment(end) {
                None => errs.push(E::DuctUnknownCompartment {
                    duct: d.id.clone(),
                    compartment: end.clone(),
                }),
                Some(c) if c.kind == CompartmentKind::Exterior => exterior_count += 1,
                Some(_) => {}
            }
        }
        if d.a == d.b {
            errs.push(E::DuctSelfLoop { duct: d.id.clone() });
        }
        if exterior_count == 2 {
            errs.push(E::DuctBothEndsExterior { duct: d.id.clone() });
        }
        if d.capacity == 0 || d.capacity > MAX_QUANTITY {
            errs.push(E::DuctCapacityZero { duct: d.id.clone() });
        }
    }

    let mut fan_ids = BTreeSet::new();
    let mut fan_ducts = BTreeSet::new();
    for f in &air.fans {
        if !fan_ids.insert(f.id.clone()) {
            errs.push(E::DuplicateFan { id: f.id.clone() });
        }
        match air.duct(&f.duct) {
            None => errs.push(E::FanUnknownDuct { fan: f.id.clone() }),
            Some(_) => {
                if !fan_ducts.insert(f.duct.clone()) {
                    errs.push(E::FanDuctHasFan {
                        duct: f.duct.clone(),
                    });
                }
            }
        }
        if f.flow == 0 || f.flow > MAX_QUANTITY {
            errs.push(E::FanZeroFlow { fan: f.id.clone() });
        }
    }

    let mut damper_ids = BTreeSet::new();
    let mut damper_ducts = BTreeSet::new();
    for d in &air.dampers {
        if !damper_ids.insert(d.id.clone()) {
            errs.push(E::DuplicateDamper { id: d.id.clone() });
        }
        match air.duct(&d.duct) {
            None => errs.push(E::DamperUnknownDuct {
                damper: d.id.clone(),
            }),
            Some(_) => {
                if !damper_ducts.insert(d.duct.clone()) {
                    errs.push(E::DamperDuctHasDamper {
                        duct: d.duct.clone(),
                    });
                }
            }
        }
        if d.start_position > 100 {
            errs.push(E::DamperStartTooHigh {
                damper: d.id.clone(),
            });
        }
    }

    let mut filter_ids = BTreeSet::new();
    let mut filter_ducts = BTreeSet::new();
    for f in &air.filters {
        if !filter_ids.insert(f.id.clone()) {
            errs.push(E::DuplicateFilter { id: f.id.clone() });
        }
        match air.duct(&f.duct) {
            None => errs.push(E::FilterUnknownDuct {
                filter: f.id.clone(),
            }),
            Some(_) => {
                if !filter_ducts.insert(f.duct.clone()) {
                    errs.push(E::FilterDuctHasFilter {
                        duct: f.duct.clone(),
                    });
                }
            }
        }
        if f.efficiency > MAX_PER_MILLE {
            errs.push(E::FilterEfficiencyTooHigh {
                filter: f.id.clone(),
            });
        }
        if f.capacity == 0 || f.capacity > MAX_QUANTITY {
            errs.push(E::FilterCapacityZero {
                filter: f.id.clone(),
            });
        }
    }

    let mut gas_ids = BTreeSet::new();
    for s in &air.gas_sources {
        if !gas_ids.insert(s.id.clone()) {
            errs.push(E::DuplicateGasSource { id: s.id.clone() });
        }
        if air.species(&s.species).is_none() {
            errs.push(E::GasSourceUnknownSpecies {
                source: s.id.clone(),
            });
        }
        match def.compartment(&s.compartment) {
            None => errs.push(E::GasSourceUnknownCompartment {
                source: s.id.clone(),
            }),
            Some(c) if c.kind != CompartmentKind::Room => errs.push(E::GasSourceOwnerIsExterior {
                source: s.id.clone(),
            }),
            Some(_) => {}
        }
        if s.release_per_tick == 0 || s.release_per_tick > MAX_QUANTITY {
            errs.push(E::GasSourceZeroRate {
                source: s.id.clone(),
            });
        }
        if s.reservoir == 0 || s.reservoir > MAX_QUANTITY {
            errs.push(E::GasSourceEmpty {
                source: s.id.clone(),
            });
        }
    }

    let mut trace_ids = BTreeSet::new();
    for s in &air.trace_sources {
        if !trace_ids.insert(s.id.clone()) {
            errs.push(E::DuplicateTraceSource { id: s.id.clone() });
        }
        match def.compartment(&s.compartment) {
            None => errs.push(E::TraceSourceUnknownCompartment {
                source: s.id.clone(),
            }),
            Some(c) if c.kind != CompartmentKind::Room => {
                errs.push(E::TraceSourceOwnerIsExterior {
                    source: s.id.clone(),
                })
            }
            Some(_) => {}
        }
        if s.release_per_tick == 0 || s.release_per_tick > MAX_QUANTITY {
            errs.push(E::TraceSourceZeroRate {
                source: s.id.clone(),
            });
        }
        if s.reservoir == 0 || s.reservoir > MAX_QUANTITY {
            errs.push(E::TraceSourceEmpty {
                source: s.id.clone(),
            });
        }
    }

    let mut equipment_ids = BTreeSet::new();
    let mut targets: BTreeSet<String> = BTreeSet::new();
    for e in &air.equipment {
        if !equipment_ids.insert(e.id.clone()) {
            errs.push(E::DuplicateEquipment { id: e.id.clone() });
        }
        match def.compartment(&e.compartment) {
            None => errs.push(E::EquipmentUnknownCompartment {
                equipment: e.id.clone(),
            }),
            Some(c) if c.kind != CompartmentKind::Room => errs.push(E::EquipmentOwnerIsExterior {
                equipment: e.id.clone(),
            }),
            Some(_) => {}
        }
        if let Some(target) = &e.controls {
            let exists = match target {
                EquipmentTarget::Fan(id) => air.fan(id).is_some(),
                EquipmentTarget::Drain(id) => {
                    def.water.as_ref().is_some_and(|w| w.drain(id).is_some())
                }
            };
            if !exists {
                errs.push(E::EquipmentUnknownTarget {
                    equipment: e.id.clone(),
                });
            }
            let key = match target {
                EquipmentTarget::Fan(id) => format!("fan:{id}"),
                EquipmentTarget::Drain(id) => format!("drain:{id}"),
            };
            if !targets.insert(key) {
                errs.push(E::DuplicateEquipmentTarget {
                    equipment: e.id.clone(),
                });
            }
        }
        if let Some(trip) = &e.trip {
            if trip.conductivity_min > MAX_PER_MILLE {
                errs.push(E::EquipmentTripInvalid {
                    equipment: e.id.clone(),
                });
            }
        }
    }

    for d in &air.ducts {
        check_qty(errs, d.id.as_str(), d.capacity);
    }
    for f in &air.fans {
        check_qty(errs, f.id.as_str(), f.flow);
    }
    for f in &air.filters {
        check_qty(errs, f.id.as_str(), f.capacity);
    }
    for s in &air.gas_sources {
        check_qty(errs, s.id.as_str(), s.reservoir);
        check_qty(errs, s.id.as_str(), s.release_per_tick);
    }
    for s in &air.trace_sources {
        check_qty(errs, s.id.as_str(), s.reservoir);
        check_qty(errs, s.id.as_str(), s.release_per_tick);
    }
}

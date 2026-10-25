//! The pressurised water model: authored reservoirs, a directed pipe-capacity
//! graph, outlets, leaks, drains and per-compartment accumulation.
//!
//! # Deliberate simplifications
//!
//! Water inside pipes is **not stored**. A pipe is a per-tick capacity in a
//! directed graph from a reservoir to discharge points. Supply is a bounded
//! max-flow allocation each tick; a room that is full refuses further water
//! (backpressure) rather than creating it. Drains and spills send water to an
//! authored exterior boundary and record it. See the module docs for the full
//! model declaration.
//!
//! Water carries a stylised conductivity and a small dissolved-gas inventory so
//! the combined model can distinguish clean supply water from conductive sump
//! water, and can carry soluble gas out with a drain. Neither is a chemical
//! model.

use super::{
    CompartmentDef, CompartmentId, CompartmentKind, GasSpeciesId, InfrastructureDefinition,
    InfrastructureValidationError, MAX_QUANTITY, Tier, check_qty,
};
use crate::scenario::ids::{
    DrainId, JunctionId, LeakId, PipeId, ReservoirId, ValveId, WaterOutletId,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Per-mille conductivity cap. Conductivity is a game scalar, not µS/cm.
pub const MAX_CONDUCTIVITY: u16 = 1000;

/// Where a pipe starts or ends. Every endpoint is authored; a pipe can only
/// join two of them.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum WaterEndpoint {
    Reservoir(ReservoirId),
    Junction(JunctionId),
    Outlet(WaterOutletId),
    Leak(LeakId),
}

impl WaterEndpoint {
    /// A stable string for diagnostics and deterministic ordering.
    pub fn label(&self) -> String {
        match self {
            WaterEndpoint::Reservoir(id) => format!("reservoir:{id}"),
            WaterEndpoint::Junction(id) => format!("junction:{id}"),
            WaterEndpoint::Outlet(id) => format!("outlet:{id}"),
            WaterEndpoint::Leak(id) => format!("leak:{id}"),
        }
    }
}

/// An authored pressurised supply reservoir.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaterReservoirDef {
    pub id: ReservoirId,
    pub name: String,
    /// Stylised water units the reservoir can hold.
    pub capacity: u64,
    pub initial: u64,
    /// Maximum units this reservoir pushes into the network per tick. This is
    /// the standalone "pressure budget"; no physical head is modelled.
    pub supply_per_tick: u64,
    /// Stylised conductivity in per-mille units (0 = clean, 1000 = highly
    /// conductive). It is not a material property or a safety figure.
    pub conductivity: u16,
}

/// A pipe junction. It stores nothing; it exists to make the authored graph
/// readable and to let several branches share a capacity-limited trunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JunctionDef {
    pub id: JunctionId,
    pub name: String,
}

/// A directed pressurised pipe with a per-tick capacity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipeDef {
    pub id: PipeId,
    pub name: String,
    pub from: WaterEndpoint,
    pub to: WaterEndpoint,
    pub capacity: u64,
}

/// A valve on one pipe. A closed valve removes that pipe's capacity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaterValveDef {
    pub id: ValveId,
    pub name: String,
    pub pipe: PipeId,
    pub start_open: bool,
}

/// A controlled discharge point. Water leaves the pipe network into the
/// compartment it names, bounded by `rate` and by the room's free capacity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaterOutletDef {
    pub id: WaterOutletId,
    pub name: String,
    pub compartment: CompartmentId,
    pub rate: u64,
    pub start_open: bool,
}

/// A leak: an authored discharge point that starts unpatched (or patched) and
/// can be sealed. It is deliberately its own fixture, not a random failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeakDef {
    pub id: LeakId,
    pub name: String,
    pub compartment: CompartmentId,
    pub rate: u64,
    pub start_patched: bool,
}

/// A floor drain: a gravity sink from one room to an authored exterior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrainDef {
    pub id: DrainId,
    pub name: String,
    pub compartment: CompartmentId,
    /// Units removed per tick when clear and unobstructed.
    pub capacity: u64,
    /// Must be an authored exterior compartment.
    pub to: CompartmentId,
    pub start_clear: bool,
}

/// The authored water system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaterNetworkDef {
    pub reservoirs: Vec<WaterReservoirDef>,
    pub junctions: Vec<JunctionDef>,
    pub pipes: Vec<PipeDef>,
    pub valves: Vec<WaterValveDef>,
    pub outlets: Vec<WaterOutletDef>,
    pub leaks: Vec<LeakDef>,
    pub drains: Vec<DrainDef>,
}

impl WaterNetworkDef {
    pub fn reservoir(&self, id: &ReservoirId) -> Option<&WaterReservoirDef> {
        self.reservoirs.iter().find(|r| &r.id == id)
    }

    pub fn pipe(&self, id: &PipeId) -> Option<&PipeDef> {
        self.pipes.iter().find(|p| &p.id == id)
    }

    pub fn valve(&self, id: &ValveId) -> Option<&WaterValveDef> {
        self.valves.iter().find(|v| &v.id == id)
    }

    pub fn outlet(&self, id: &WaterOutletId) -> Option<&WaterOutletDef> {
        self.outlets.iter().find(|o| &o.id == id)
    }

    pub fn leak(&self, id: &LeakId) -> Option<&LeakDef> {
        self.leaks.iter().find(|l| &l.id == id)
    }

    pub fn drain(&self, id: &DrainId) -> Option<&DrainDef> {
        self.drains.iter().find(|d| &d.id == id)
    }

    fn endpoint_exists(&self, e: &WaterEndpoint) -> bool {
        match e {
            WaterEndpoint::Reservoir(id) => self.reservoir(id).is_some(),
            WaterEndpoint::Junction(id) => self.junctions.iter().any(|j| &j.id == id),
            WaterEndpoint::Outlet(id) => self.outlet(id).is_some(),
            WaterEndpoint::Leak(id) => self.leak(id).is_some(),
        }
    }

    /// Every pipe that leaves `from`, in authored order.
    fn outgoing(&self, from: &WaterEndpoint) -> impl Iterator<Item = &PipeDef> {
        self.pipes.iter().filter(move |p| &p.from == from)
    }

    /// Whether supply can reach this endpoint from any reservoir along
    /// authored pipe direction, ignoring valve state and capacity.
    fn reachable_from_a_reservoir(&self, target: &WaterEndpoint) -> bool {
        let mut seen: BTreeSet<WaterEndpoint> = BTreeSet::new();
        let mut frontier: VecDeque<WaterEndpoint> = self
            .reservoirs
            .iter()
            .map(|r| WaterEndpoint::Reservoir(r.id.clone()))
            .collect();
        for e in &frontier {
            seen.insert(e.clone());
        }
        while let Some(here) = frontier.pop_front() {
            if &here == target {
                return true;
            }
            for pipe in self.outgoing(&here) {
                if seen.insert(pipe.to.clone()) {
                    frontier.push_back(pipe.to.clone());
                }
            }
        }
        false
    }
}

/// Water tuning; all values are stylised game parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaterTuning {
    /// Standing water at or above this many units reads as `Trace`.
    pub trace_units: u64,
    /// At or above this many units reads as `Warning`.
    pub warning_units: u64,
    /// At or above this many units reads as `Critical`.
    pub critical_units: u64,
    /// Water at or above this many units can spill through an open opening.
    pub spill_units: u64,
    /// The tier at which [`super::sim::CompartmentConditions::traversal_blocked`]
    /// becomes true. This is a separate query, never a change to the portal
    /// graph.
    pub traversal_block_tier: Tier,
}

impl Default for WaterTuning {
    fn default() -> Self {
        Self {
            trace_units: 1,
            warning_units: 150,
            critical_units: 400,
            spill_units: 40,
            traversal_block_tier: Tier::Critical,
        }
    }
}

/// One compartment's water inventory.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaterBody {
    /// Stylised water units.
    pub water: u64,
    /// Stylised conductivity, mixed by volume when water from different
    /// sources accumulates. Zero when the compartment is dry.
    pub conductivity: u16,
    /// Gas currently dissolved in this water, per species. Tracked so that
    /// draining water carries the gas out with it rather than deleting it.
    pub dissolved: BTreeMap<GasSpeciesId, u64>,
}

impl WaterBody {
    /// Add water from a source with the given conductivity and dissolved gas.
    /// The caller is responsible for not exceeding the compartment capacity.
    pub fn add(&mut self, units: u64, conductivity: u16, dissolved: BTreeMap<GasSpeciesId, u64>) {
        if units == 0 {
            return;
        }
        self.conductivity = mix_conductivity(
            self.conductivity,
            self.water,
            conductivity.min(MAX_CONDUCTIVITY),
            units,
        );
        self.water = self.water.saturating_add(units).min(MAX_QUANTITY);
        for (species, amount) in dissolved {
            let entry = self.dissolved.entry(species).or_insert(0);
            *entry = entry.saturating_add(amount);
        }
    }

    /// Remove up to `units`, returning how much was actually removed and the
    /// dissolved gas that left with it. Never goes negative.
    pub fn take_water(&mut self, units: u64) -> (u64, BTreeMap<GasSpeciesId, u64>) {
        let taken = units.min(self.water);
        if taken == 0 {
            return (0, BTreeMap::new());
        }
        let old_water = self.water;
        let mut dissolved = BTreeMap::new();
        if taken >= old_water {
            dissolved = std::mem::take(&mut self.dissolved);
            self.water = 0;
            self.conductivity = 0;
        } else {
            for (species, amount) in &self.dissolved {
                let moved = super::proportion(*amount, taken, old_water);
                if moved > 0 {
                    dissolved.insert(species.clone(), moved);
                }
            }
            for (species, amount) in &dissolved {
                if let Some(total) = self.dissolved.get_mut(species) {
                    *total = total.saturating_sub(*amount);
                }
            }
            self.dissolved.retain(|_, amount| *amount > 0);
            self.water -= taken;
        }
        (taken, dissolved)
    }

    /// Add already-dissolved gas without adding water (used by absorption).
    pub fn dissolve(&mut self, species: &GasSpeciesId, amount: u64) {
        if amount == 0 {
            return;
        }
        let entry = self.dissolved.entry(species.clone()).or_insert(0);
        *entry = entry.saturating_add(amount);
    }

    /// Remove dissolved gas, bounded by what is present. Returns the amount
    /// actually removed.
    pub fn undissolve(&mut self, species: &GasSpeciesId, amount: u64) -> u64 {
        let Some(held) = self.dissolved.get_mut(species) else {
            return 0;
        };
        let taken = amount.min(*held);
        *held -= taken;
        if *held == 0 {
            self.dissolved.remove(species);
        }
        taken
    }

    /// Total dissolved gas across all species.
    pub fn dissolved_total(&self) -> u64 {
        self.dissolved.values().copied().sum()
    }
}

fn mix_conductivity(current: u16, current_units: u64, add: u16, add_units: u64) -> u16 {
    let total = current_units.saturating_add(add_units);
    if total == 0 {
        return 0;
    }
    let weighted = current as u128 * current_units as u128 + add as u128 * add_units as u128;
    (weighted / total as u128).min(MAX_CONDUCTIVITY as u128) as u16
}

/// The water source/sink ledger. It makes creation and deletion visible.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaterLedger {
    /// Sum of authored reservoir `initial` values at construction.
    pub initial_reservoir: u64,
    /// Explicit reservoir recharge (reserved; currently authored as zero).
    pub recharged: u64,
    /// Water sent to an authored exterior by a drain.
    pub drained_to_exterior: u64,
    /// Water sent to an authored exterior by spilling through an opening.
    pub spilled_to_exterior: u64,
}

/// Live water state. All maps are keyed by authored ids and sorted, so
/// iteration order can never leak into a result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaterState {
    pub bodies: BTreeMap<CompartmentId, WaterBody>,
    pub reservoir_remaining: BTreeMap<ReservoirId, u64>,
    pub valves_open: BTreeMap<ValveId, bool>,
    pub outlets_open: BTreeMap<WaterOutletId, bool>,
    pub leaks_patched: BTreeMap<LeakId, bool>,
    pub drains_clear: BTreeMap<DrainId, bool>,
    /// Actual units moved through each fixture on the last tick, for readouts.
    pub outlet_flow: BTreeMap<WaterOutletId, u64>,
    pub leak_flow: BTreeMap<LeakId, u64>,
    pub drain_flow: BTreeMap<DrainId, u64>,
    /// Reservoirs that have already emitted an "emptied" event.
    pub reservoir_reported_empty: BTreeSet<ReservoirId>,
    pub ledger: WaterLedger,
}

impl WaterState {
    /// Build the initial state for a definition's compartments.
    pub fn initial(def: &InfrastructureDefinition, water: &WaterNetworkDef) -> Self {
        let mut bodies = BTreeMap::new();
        for c in &def.compartments {
            if c.kind == CompartmentKind::Room {
                bodies.insert(c.id.clone(), WaterBody::default());
            }
        }
        let reservoir_remaining: BTreeMap<ReservoirId, u64> = water
            .reservoirs
            .iter()
            .map(|r| (r.id.clone(), r.initial))
            .collect();
        let initial_reservoir = reservoir_remaining.values().copied().sum();
        Self {
            bodies,
            reservoir_remaining,
            valves_open: water
                .valves
                .iter()
                .map(|v| (v.id.clone(), v.start_open))
                .collect(),
            outlets_open: water
                .outlets
                .iter()
                .map(|o| (o.id.clone(), o.start_open))
                .collect(),
            leaks_patched: water
                .leaks
                .iter()
                .map(|l| (l.id.clone(), l.start_patched))
                .collect(),
            drains_clear: water
                .drains
                .iter()
                .map(|d| (d.id.clone(), d.start_clear))
                .collect(),
            outlet_flow: water.outlets.iter().map(|o| (o.id.clone(), 0)).collect(),
            leak_flow: water.leaks.iter().map(|l| (l.id.clone(), 0)).collect(),
            drain_flow: water.drains.iter().map(|d| (d.id.clone(), 0)).collect(),
            reservoir_reported_empty: BTreeSet::new(),
            ledger: WaterLedger {
                initial_reservoir,
                ..WaterLedger::default()
            },
        }
    }

    /// One compartment's water body, if it has one.
    pub fn body(&self, id: &CompartmentId) -> Option<&WaterBody> {
        self.bodies.get(id)
    }

    /// Mutable access to one compartment's water body.
    pub fn body_mut(&mut self, id: &CompartmentId) -> Option<&mut WaterBody> {
        self.bodies.get_mut(id)
    }

    /// Total water currently held in compartments.
    pub fn water_in_compartments(&self) -> u64 {
        self.bodies.values().map(|b| b.water).sum()
    }

    /// Total water currently left in reservoirs.
    pub fn water_in_reservoirs(&self) -> u64 {
        self.reservoir_remaining.values().copied().sum()
    }

    /// Whether the water ledger balances exactly.
    pub fn balanced(&self) -> bool {
        self.ledger
            .initial_reservoir
            .saturating_add(self.ledger.recharged)
            == self
                .water_in_reservoirs()
                .saturating_add(self.water_in_compartments())
                .saturating_add(self.ledger.drained_to_exterior)
                .saturating_add(self.ledger.spilled_to_exterior)
    }

    /// The current free water capacity of a room, zero for exteriors.
    pub fn free_capacity(&self, compartments: &[CompartmentDef], id: &CompartmentId) -> u64 {
        let Some(c) = compartments.iter().find(|c| &c.id == id) else {
            return 0;
        };
        if c.kind != CompartmentKind::Room {
            return 0;
        }
        let held = self.body(id).map(|b| b.water).unwrap_or(0);
        c.water_capacity.saturating_sub(held)
    }
}

/// Validate a water network against its containing definition.
pub(crate) fn validate_water(
    def: &InfrastructureDefinition,
    water: &WaterNetworkDef,
    errs: &mut Vec<InfrastructureValidationError>,
) {
    use InfrastructureValidationError as E;

    let mut reservoirs = BTreeSet::new();
    for r in &water.reservoirs {
        if !reservoirs.insert(r.id.clone()) {
            errs.push(E::DuplicateReservoir { id: r.id.clone() });
        }
        if r.capacity == 0 || r.capacity > MAX_QUANTITY {
            errs.push(E::ReservoirCapacityInvalid { id: r.id.clone() });
        }
        if r.initial > r.capacity {
            errs.push(E::ReservoirInitialInvalid { id: r.id.clone() });
        }
        if r.supply_per_tick == 0 || r.supply_per_tick > MAX_QUANTITY {
            errs.push(E::ReservoirSupplyInvalid { id: r.id.clone() });
        }
        if r.conductivity > MAX_CONDUCTIVITY {
            errs.push(E::QuantityTooLarge {
                what: r.id.to_string(),
            });
        }
    }

    let mut junctions = BTreeSet::new();
    for j in &water.junctions {
        if !junctions.insert(j.id.clone()) {
            errs.push(E::DuplicateJunction { id: j.id.clone() });
        }
    }

    let mut pipe_ids = BTreeSet::new();
    for p in &water.pipes {
        if !pipe_ids.insert(p.id.clone()) {
            errs.push(E::DuplicatePipe { id: p.id.clone() });
        }
        if !water.endpoint_exists(&p.from) || !water.endpoint_exists(&p.to) {
            errs.push(E::PipeEndpointUnknown { pipe: p.id.clone() });
        }
        if p.from == p.to {
            errs.push(E::PipeSelfLoop { pipe: p.id.clone() });
        }
        if p.capacity == 0 || p.capacity > MAX_QUANTITY {
            errs.push(E::PipeCapacityZero { pipe: p.id.clone() });
        }
    }

    let mut valve_ids = BTreeSet::new();
    let mut valved_pipes = BTreeSet::new();
    for v in &water.valves {
        if !valve_ids.insert(v.id.clone()) {
            errs.push(E::DuplicateValve { id: v.id.clone() });
        }
        match water.pipe(&v.pipe) {
            None => errs.push(E::ValveUnknownPipe {
                valve: v.id.clone(),
            }),
            Some(_) => {
                if !valved_pipes.insert(v.pipe.clone()) {
                    errs.push(E::PipeHasTwoValves {
                        pipe: v.pipe.clone(),
                    });
                }
            }
        }
    }

    let mut outlets = BTreeSet::new();
    for o in &water.outlets {
        if !outlets.insert(o.id.clone()) {
            errs.push(E::DuplicateOutlet { id: o.id.clone() });
        }
        match def.compartment(&o.compartment) {
            None => errs.push(E::OutletUnknownCompartment {
                outlet: o.id.clone(),
            }),
            Some(c) if c.kind != CompartmentKind::Room => errs.push(E::OutletOwnerIsExterior {
                outlet: o.id.clone(),
            }),
            Some(_) => {}
        }
        if o.rate == 0 || o.rate > MAX_QUANTITY {
            errs.push(E::OutletZeroRate {
                outlet: o.id.clone(),
            });
        }
    }

    let mut leaks = BTreeSet::new();
    for l in &water.leaks {
        if !leaks.insert(l.id.clone()) {
            errs.push(E::DuplicateLeak { id: l.id.clone() });
        }
        match def.compartment(&l.compartment) {
            None => errs.push(E::LeakUnknownCompartment { leak: l.id.clone() }),
            Some(c) if c.kind != CompartmentKind::Room => {
                errs.push(E::LeakOwnerIsExterior { leak: l.id.clone() })
            }
            Some(_) => {}
        }
        if l.rate == 0 || l.rate > MAX_QUANTITY {
            errs.push(E::LeakZeroRate { leak: l.id.clone() });
        }
    }

    let mut drains = BTreeSet::new();
    for d in &water.drains {
        if !drains.insert(d.id.clone()) {
            errs.push(E::DuplicateDrain { id: d.id.clone() });
        }
        match def.compartment(&d.compartment) {
            None => errs.push(E::DrainUnknownCompartment {
                drain: d.id.clone(),
            }),
            Some(c) if c.kind != CompartmentKind::Room => errs.push(E::DrainOwnerIsExterior {
                drain: d.id.clone(),
            }),
            Some(_) => {}
        }
        match def.compartment(&d.to) {
            Some(c) if c.kind == CompartmentKind::Exterior => {}
            _ => errs.push(E::DrainTargetNotExterior {
                drain: d.id.clone(),
            }),
        }
        if d.capacity == 0 || d.capacity > MAX_QUANTITY {
            errs.push(E::DrainZeroCapacity {
                drain: d.id.clone(),
            });
        }
    }

    for o in &water.outlets {
        let endpoint = WaterEndpoint::Outlet(o.id.clone());
        if !water.reachable_from_a_reservoir(&endpoint) {
            errs.push(E::OutletUnreachable {
                outlet: o.id.clone(),
            });
        }
    }
    for l in &water.leaks {
        let endpoint = WaterEndpoint::Leak(l.id.clone());
        if !water.reachable_from_a_reservoir(&endpoint) {
            errs.push(E::LeakUnreachable { leak: l.id.clone() });
        }
    }

    for r in &water.reservoirs {
        check_qty(errs, r.id.as_str(), r.capacity);
        check_qty(errs, r.id.as_str(), r.initial);
        check_qty(errs, r.id.as_str(), r.supply_per_tick);
    }
    for p in &water.pipes {
        check_qty(errs, p.id.as_str(), p.capacity);
    }
    for o in &water.outlets {
        check_qty(errs, o.id.as_str(), o.rate);
    }
    for l in &water.leaks {
        check_qty(errs, l.id.as_str(), l.rate);
    }
    for d in &water.drains {
        check_qty(errs, d.id.as_str(), d.capacity);
    }
}

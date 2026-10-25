//! Three committed fixtures, in the order the design asks for them.
//!
//! 1. [`minimal_water`] — pressurised supply, a valve, an outlet, a patchable
//!    leak and a drain to an authored exterior. No air system at all.
//! 2. [`minimal_ventilation`] — a duct with a fan, a damper and a filter, two
//!    fictional gas categories, a scent/noise trace source and equipment that
//!    can ignite. No water system at all.
//! 3. [`combined`] — the same vocabulary on one floor plan, proving the two
//!    systems interoperate without collapsing into a single model: water can
//!    trip powered equipment, and air can be redirected to clear a hazard.
//!
//! Every fixture is authored with the same private helpers, so the shapes stay
//! readable next to `building/exchange_house.rs`. All numbers are stylised game
//! parameters. None of them is a real concentration, flow rate or pressure.

use super::air::{
    AirNetworkDef, AirTuning, DamperDef, DuctDef, EquipmentDef, EquipmentTarget, EquipmentTrip,
    FanDef, FilterDef, FlowDirection, GasClass, GasSourceDef, GasSpeciesDef, TraceSourceDef,
};
use super::water::{
    DrainDef, JunctionDef, LeakDef, PipeDef, WaterNetworkDef, WaterOutletDef, WaterReservoirDef,
    WaterTuning, WaterValveDef,
};
use super::{
    CompartmentDef, CompartmentKind, CueKind, INFRASTRUCTURE_FORMAT, InfrastructureDefinition,
    InfrastructureTuning, OpeningDef, OpeningKind, Tier, TraceKind,
};
use crate::scenario::ids::{
    CircuitId, CompartmentId, DamperId, DrainId, DuctId, EquipmentId, FanId, FilterId, GasSourceId,
    GasSpeciesId, JunctionId, LeakId, OpeningId, PipeId, ReservoirId, TraceSourceId, ValveId,
    WaterOutletId,
};

/// The three fixtures in the order they are meant to be read.
pub fn all() -> Vec<InfrastructureDefinition> {
    vec![minimal_water(), minimal_ventilation(), combined()]
}

// --- authoring helpers ------------------------------------------------------

fn room(id: &str, name: &str, air_volume: u64, water_capacity: u64) -> CompartmentDef {
    CompartmentDef {
        id: CompartmentId::from(id),
        name: name.to_owned(),
        kind: CompartmentKind::Room,
        air_volume,
        water_capacity,
    }
}

fn exterior(id: &str, name: &str) -> CompartmentDef {
    CompartmentDef {
        id: CompartmentId::from(id),
        name: name.to_owned(),
        kind: CompartmentKind::Exterior,
        air_volume: 0,
        water_capacity: 0,
    }
}

#[allow(clippy::too_many_arguments)]
fn opening(
    id: &str,
    name: &str,
    kind: OpeningKind,
    a: &str,
    b: &str,
    air_conductance: u64,
    water_conductance: u64,
    traversal: bool,
    start_open: bool,
) -> OpeningDef {
    OpeningDef {
        id: OpeningId::from(id),
        name: name.to_owned(),
        kind,
        a: CompartmentId::from(a),
        b: CompartmentId::from(b),
        air_conductance,
        water_conductance,
        traversal,
        portal: None,
        start_open,
    }
}

fn reservoir(id: &str, name: &str, capacity: u64, initial: u64, supply: u64) -> WaterReservoirDef {
    WaterReservoirDef {
        id: ReservoirId::from(id),
        name: name.to_owned(),
        capacity,
        initial,
        supply_per_tick: supply,
        conductivity: 0,
    }
}

fn junction(id: &str, name: &str) -> JunctionDef {
    JunctionDef {
        id: JunctionId::from(id),
        name: name.to_owned(),
    }
}

fn pipe(
    id: &str,
    name: &str,
    from: super::water::WaterEndpoint,
    to: super::water::WaterEndpoint,
    capacity: u64,
) -> PipeDef {
    PipeDef {
        id: PipeId::from(id),
        name: name.to_owned(),
        from,
        to,
        capacity,
    }
}

fn valve(id: &str, name: &str, pipe: &str, start_open: bool) -> WaterValveDef {
    WaterValveDef {
        id: ValveId::from(id),
        name: name.to_owned(),
        pipe: PipeId::from(pipe),
        start_open,
    }
}

fn outlet(id: &str, name: &str, compartment: &str, rate: u64, start_open: bool) -> WaterOutletDef {
    WaterOutletDef {
        id: WaterOutletId::from(id),
        name: name.to_owned(),
        compartment: CompartmentId::from(compartment),
        rate,
        start_open,
    }
}

fn leak(id: &str, name: &str, compartment: &str, rate: u64, start_patched: bool) -> LeakDef {
    LeakDef {
        id: LeakId::from(id),
        name: name.to_owned(),
        compartment: CompartmentId::from(compartment),
        rate,
        start_patched,
    }
}

fn drain(id: &str, name: &str, compartment: &str, capacity: u64, to: &str) -> DrainDef {
    DrainDef {
        id: DrainId::from(id),
        name: name.to_owned(),
        compartment: CompartmentId::from(compartment),
        capacity,
        to: CompartmentId::from(to),
        start_clear: false,
    }
}

#[allow(clippy::too_many_arguments)]
fn species(
    id: &str,
    name: &str,
    class: GasClass,
    solubility: u16,
    trace: u16,
    warning: u16,
    critical: u16,
    cue: CueKind,
) -> GasSpeciesDef {
    GasSpeciesDef {
        id: GasSpeciesId::from(id),
        name: name.to_owned(),
        class,
        solubility,
        trace_concentration: trace,
        warning_concentration: warning,
        critical_concentration: critical,
        cue,
    }
}

fn duct(id: &str, name: &str, a: &str, b: &str, capacity: u64, crawlable: bool) -> DuctDef {
    DuctDef {
        id: DuctId::from(id),
        name: name.to_owned(),
        a: CompartmentId::from(a),
        b: CompartmentId::from(b),
        capacity,
        crawlable,
        start_hatch_open: false,
    }
}

fn fan(
    id: &str,
    name: &str,
    duct: &str,
    flow: u64,
    circuit: &str,
    start_on: bool,
    reversible: bool,
) -> FanDef {
    FanDef {
        id: FanId::from(id),
        name: name.to_owned(),
        duct: DuctId::from(duct),
        flow,
        circuit: Some(CircuitId::from(circuit)),
        start_on,
        reversible,
        start_direction: FlowDirection::AToB,
    }
}

fn damper(id: &str, name: &str, duct: &str, start_position: u8) -> DamperDef {
    DamperDef {
        id: DamperId::from(id),
        name: name.to_owned(),
        duct: DuctId::from(duct),
        start_position,
    }
}

fn filter(id: &str, name: &str, duct: &str, efficiency: u16, capacity: u64) -> FilterDef {
    FilterDef {
        id: FilterId::from(id),
        name: name.to_owned(),
        duct: DuctId::from(duct),
        efficiency,
        capacity,
    }
}

fn gas_source(
    id: &str,
    name: &str,
    species: &str,
    compartment: &str,
    reservoir: u64,
    release: u64,
    start_on: bool,
) -> GasSourceDef {
    GasSourceDef {
        id: GasSourceId::from(id),
        name: name.to_owned(),
        species: GasSpeciesId::from(species),
        compartment: CompartmentId::from(compartment),
        reservoir,
        release_per_tick: release,
        start_on,
    }
}

fn trace_source(
    id: &str,
    name: &str,
    kind: TraceKind,
    compartment: &str,
    reservoir: u64,
    release: u64,
) -> TraceSourceDef {
    TraceSourceDef {
        id: TraceSourceId::from(id),
        name: name.to_owned(),
        kind,
        compartment: CompartmentId::from(compartment),
        reservoir,
        release_per_tick: release,
        start_on: true,
    }
}

fn definition(
    id: &str,
    name: &str,
    compartments: Vec<CompartmentDef>,
    openings: Vec<OpeningDef>,
    water: Option<WaterNetworkDef>,
    air: Option<AirNetworkDef>,
) -> InfrastructureDefinition {
    InfrastructureDefinition {
        format: INFRASTRUCTURE_FORMAT.to_owned(),
        infrastructure_id: id.to_owned(),
        name: name.to_owned(),
        compartments,
        openings,
        water,
        air,
        tuning: InfrastructureTuning {
            water: WaterTuning::default(),
            air: AirTuning::default(),
        },
    }
}

// --- 1. water only ----------------------------------------------------------

/// A water-only fixture: one store room, one hall, an authored storm drain.
///
/// Player-facing causal chain: open the valve and the tank pressurises the
/// store outlet; the hall drain is clogged until cleared; the elbow leak can be
/// patched; water that overflows the store door spills into the hall, where a
/// deeper puddle blocks traversal.
pub fn minimal_water() -> InfrastructureDefinition {
    use super::water::WaterEndpoint;

    let store = "service/store";
    let hall = "service/hall";
    let storm = "exterior/storm";

    let water = WaterNetworkDef {
        reservoirs: vec![reservoir("tank-roof", "Roof tank", 2400, 900, 20)],
        junctions: vec![junction("j-riser", "Service riser")],
        pipes: vec![
            pipe(
                "p-riser",
                "Riser main",
                WaterEndpoint::Reservoir(ReservoirId::from("tank-roof")),
                WaterEndpoint::Junction(JunctionId::from("j-riser")),
                120,
            ),
            pipe(
                "p-store",
                "Store branch",
                WaterEndpoint::Junction(JunctionId::from("j-riser")),
                WaterEndpoint::Outlet(WaterOutletId::from("o-store")),
                80,
            ),
            pipe(
                "p-leak",
                "Elbow leak feed",
                WaterEndpoint::Junction(JunctionId::from("j-riser")),
                WaterEndpoint::Leak(LeakId::from("leak-elbow")),
                40,
            ),
        ],
        valves: vec![
            valve("v-main", "Main stopcock", "p-riser", true),
            valve("v-store", "Store isolator", "p-store", false),
        ],
        outlets: vec![outlet("o-store", "Store tap", store, 30, false)],
        leaks: vec![leak("leak-elbow", "Split elbow", store, 12, true)],
        drains: vec![drain("d-hall", "Hall floor drain", hall, 60, storm)],
    };

    definition(
        "fixture-water",
        "Water fixture: store, hall, storm drain",
        vec![
            room(store, "Store", 3000, 600),
            room(hall, "Hall", 5000, 900),
            exterior(storm, "Storm drain"),
        ],
        vec![
            opening(
                "open-store-hall",
                "Store door",
                OpeningKind::Door,
                store,
                hall,
                300,
                120,
                true,
                false,
            ),
            opening(
                "open-hall-weather",
                "Hall grille",
                OpeningKind::Grille,
                hall,
                storm,
                500,
                0,
                false,
                true,
            ),
        ],
        Some(water),
        None,
    )
}

// --- 2. ventilation only ----------------------------------------------------

/// A ventilation-only fixture: a lab ducted to the roof, a corridor grille, a
/// reversible fan, a damper, a filter, and two fictional gas categories.
///
/// Player-facing causal chain: run the fan to clear `haze` from the lab; the
/// filter loads and stops cleaning; `ember` released at the bench ignites only
/// if the bench equipment is live and the concentration reaches Critical, which
/// also trips every other piece of equipment in the room.
pub fn minimal_ventilation() -> InfrastructureDefinition {
    let lab = "research/lab";
    let corridor = "research/corridor";
    let roof = "exterior/roof";

    let air = AirNetworkDef {
        species: vec![
            species(
                "haze",
                "Haze",
                GasClass::Haze,
                0,
                5,
                120,
                300,
                CueKind::Haze,
            ),
            species(
                "ember",
                "Ember",
                GasClass::Ember,
                0,
                2,
                80,
                250,
                CueKind::Ignition,
            ),
        ],
        ducts: vec![duct("duct-lab", "Lab extract", lab, roof, 140, false)],
        fans: vec![fan(
            "fan-lab",
            "Lab extract fan",
            "duct-lab",
            120,
            "circuit-lab",
            true,
            true,
        )],
        dampers: vec![damper("damper-lab", "Lab extract damper", "duct-lab", 100)],
        filters: vec![filter(
            "filter-lab",
            "Lab extract filter",
            "duct-lab",
            600,
            300,
        )],
        gas_sources: vec![gas_source(
            "src-ember",
            "Bench canister",
            "ember",
            lab,
            1600,
            20,
            false,
        )],
        trace_sources: vec![
            trace_source("src-scent", "Solvent smell", TraceKind::Scent, lab, 200, 8),
            trace_source(
                "src-noise",
                "Extract rattle",
                TraceKind::Noise,
                corridor,
                200,
                6,
            ),
        ],
        equipment: vec![EquipmentDef {
            id: EquipmentId::from("eq-bench"),
            name: "Bench rig".to_owned(),
            compartment: CompartmentId::from(lab),
            circuit: Some(CircuitId::from("circuit-lab")),
            controls: Some(EquipmentTarget::Fan(FanId::from("fan-lab"))),
            spark_when_live: true,
            trip: Some(EquipmentTrip {
                at: Tier::Warning,
                conductivity_min: 500,
            }),
            start_tripped: false,
        }],
    };

    definition(
        "fixture-ventilation",
        "Ventilation fixture: lab, corridor, roof extract",
        vec![
            room(lab, "Lab", 2600, 200),
            room(corridor, "Corridor", 4200, 200),
            exterior(roof, "Roof"),
        ],
        vec![
            opening(
                "open-lab-corridor",
                "Lab door",
                OpeningKind::Door,
                lab,
                corridor,
                700,
                40,
                true,
                true,
            ),
            opening(
                "open-lab-grille",
                "Lab transfer grille",
                OpeningKind::Grille,
                lab,
                corridor,
                150,
                0,
                false,
                true,
            ),
        ],
        None,
        Some(air),
    )
}

// --- 3. combined ------------------------------------------------------------

/// Both systems on one floor plan.
///
/// The point of this fixture is the *narrow coupling*: the plant room's water
/// trips the extract fan's equipment when it is conductive enough, and while
/// the fan is down the `ember` canister's output accumulates instead of being
/// extracted. Neither system reads the other's internals; both meet at the
/// equipment trip and at the shared compartment bodies.
pub fn combined() -> InfrastructureDefinition {
    use super::water::WaterEndpoint;

    let plant = "service/plant";
    let hall = "exchange/hall";
    let yard = "exterior/yard";

    let water = WaterNetworkDef {
        reservoirs: vec![reservoir("tank-yard", "Yard tank", 1800, 600, 15)],
        junctions: vec![junction("j-plant", "Plant manifold")],
        pipes: vec![
            pipe(
                "p-yard",
                "Yard feed",
                WaterEndpoint::Reservoir(ReservoirId::from("tank-yard")),
                WaterEndpoint::Junction(JunctionId::from("j-plant")),
                90,
            ),
            pipe(
                "p-plant-leak",
                "Manifold leak feed",
                WaterEndpoint::Junction(JunctionId::from("j-plant")),
                WaterEndpoint::Leak(LeakId::from("leak-manifold")),
                30,
            ),
            pipe(
                "p-hall",
                "Hall branch",
                WaterEndpoint::Junction(JunctionId::from("j-plant")),
                WaterEndpoint::Outlet(WaterOutletId::from("o-hall")),
                60,
            ),
        ],
        valves: vec![
            valve("v-yard", "Yard stopcock", "p-yard", true),
            valve("v-hall", "Hall isolator", "p-hall", false),
        ],
        outlets: vec![outlet("o-hall", "Hall standpipe", hall, 25, false)],
        leaks: vec![leak("leak-manifold", "Manifold gasket", plant, 10, false)],
        drains: vec![drain("d-plant", "Plant sump", plant, 50, yard)],
    };

    let air = AirNetworkDef {
        species: vec![
            species(
                "haze",
                "Haze",
                GasClass::Haze,
                0,
                5,
                120,
                300,
                CueKind::Haze,
            ),
            species(
                "ember",
                "Ember",
                GasClass::Ember,
                0,
                2,
                80,
                250,
                CueKind::Ignition,
            ),
            species(
                "hush",
                "Hush",
                GasClass::Hush,
                400,
                4,
                100,
                260,
                CueKind::Hiss,
            ),
        ],
        ducts: vec![
            duct("duct-plant", "Plant extract", plant, yard, 120, true),
            duct("duct-hall", "Hall supply", yard, hall, 80, false),
        ],
        fans: vec![
            fan(
                "fan-plant",
                "Plant extract fan",
                "duct-plant",
                100,
                "circuit-plant",
                true,
                false,
            ),
            fan(
                "fan-hall",
                "Hall supply fan",
                "duct-hall",
                70,
                "circuit-plant",
                false,
                true,
            ),
        ],
        dampers: vec![
            damper("damper-plant", "Plant extract damper", "duct-plant", 100),
            damper("damper-hall", "Hall supply damper", "duct-hall", 50),
        ],
        filters: vec![filter(
            "filter-plant",
            "Plant extract filter",
            "duct-plant",
            500,
            240,
        )],
        gas_sources: vec![gas_source(
            "src-ember",
            "Plant canister",
            "ember",
            plant,
            500,
            15,
            false,
        )],
        trace_sources: vec![trace_source(
            "src-scent",
            "Plant solvent smell",
            TraceKind::Scent,
            plant,
            200,
            8,
        )],
        equipment: vec![
            EquipmentDef {
                id: EquipmentId::from("eq-fan"),
                name: "Extract fan starter".to_owned(),
                compartment: CompartmentId::from(plant),
                circuit: Some(CircuitId::from("circuit-plant")),
                controls: Some(EquipmentTarget::Fan(FanId::from("fan-plant"))),
                spark_when_live: true,
                trip: Some(EquipmentTrip {
                    at: Tier::Warning,
                    conductivity_min: 400,
                }),
                start_tripped: false,
            },
            EquipmentDef {
                id: EquipmentId::from("eq-sump"),
                name: "Sump pump".to_owned(),
                compartment: CompartmentId::from(plant),
                circuit: Some(CircuitId::from("circuit-plant")),
                controls: Some(EquipmentTarget::Drain(DrainId::from("d-plant"))),
                spark_when_live: false,
                trip: Some(EquipmentTrip {
                    at: Tier::Critical,
                    conductivity_min: 600,
                }),
                start_tripped: false,
            },
        ],
    };

    definition(
        "fixture-combined",
        "Combined fixture: plant, hall, yard",
        vec![
            room(plant, "Plant room", 2400, 500),
            room(hall, "Exchange hall", 4800, 800),
            exterior(yard, "Yard"),
        ],
        vec![
            opening(
                "open-plant-hall",
                "Plant door",
                OpeningKind::Door,
                plant,
                hall,
                250,
                80,
                true,
                false,
            ),
            opening(
                "open-hall-yard",
                "Hall window",
                OpeningKind::Window,
                hall,
                yard,
                400,
                0,
                false,
                false,
            ),
            opening(
                "open-plant-hatch",
                "Plant hatch",
                OpeningKind::Hatch,
                plant,
                yard,
                0,
                0,
                false,
                false,
            ),
        ],
        Some(water),
        Some(air),
    )
}

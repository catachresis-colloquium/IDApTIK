//! The "Envelope 001 – Ghost Lobby" scenario: a deterministic, event-sourced,
//! definition-as-data port of the canonical HTML prototype (ADR-0004).
//!
//! # Shape
//!
//! * [`ScenarioDefinition`] — declarative content (rooms, doors, cameras, props,
//!   objectives, difficulty presets, the full [`tuning`] table, scoring). Pure
//!   serde data; round-trips through JSON and self-validates via
//!   [`ScenarioDefinition::validate`] / [`ScenarioDefinition::ok`].
//! * [`ghost_lobby`] — builds the canonical definition by projecting
//!   [`constants`] (the single source of truth for every ported magic number).
//! * [`Command`] / [`TickInput`] / [`Buttons`] — the typed input the simulation
//!   consumes; [`Event`] — the typed output it emits; [`log_view`] renders the
//!   human [`LogLine`] view.
//! * [`Mulberry32`] — the exact PRNG port; [`roll_init`] the reset roll.
//!
//! The simulation itself ([`GhostLobbySim`]) and the JSON export/debrief
//! surfaces are added in the next stage; this stage lays the deterministic
//! foundation (RNG, constants, definition, commands, events).

pub mod actor;
pub mod agents;
pub mod building;
pub mod command;
pub mod common;
pub mod console;
pub mod constants;
pub mod definition;
pub mod event;
pub mod floor_graph;
pub mod ghost_lobby;
pub mod ids;
pub mod infra;
pub mod mathf;
pub mod outcome;
pub mod pump_house;
pub mod rng;
pub mod rooftop;
pub mod signal;
pub mod sim;
pub mod snapshot;
pub mod state;
pub mod tuning;
pub mod vsm;

pub use actor::{
    ACTORS_FORMAT, ACTORS_JSON, ActorArchetype, ActorCheck, ActorPackError, ActorRegistry,
    ActorStats, ActorValidationError, ActorValidationReport, ComposeError, ComposedActor,
    InterestProfile, Leakage, Modifier, ObjectClass, StatId, StatOp, StatOpKind, billy_actor,
    billy_archetype, default_registry, load_actor_pack,
};
pub use agents::Agents;
pub use building::audit::{AuditSeverity, AuthoringCheck, AuthoringReport};
pub use building::exchange_house::{EXCHANGE_HOUSE_JSON, EXCHANGE_HOUSE_RUN_JSON, exchange_house};
pub use building::sim::{
    BUILDING_SNAPSHOT_FORMAT, BuildingCommand, BuildingDebrief, BuildingDebriefExport,
    BuildingDefinitionExport, BuildingDenyReason, BuildingEvent, BuildingExport, BuildingSim,
    BuildingSnapshot, LegacyLevelConfig,
};
pub use building::{
    BUILDING_FORMAT, BuildingCheck, BuildingDefinition, BuildingPackError, BuildingRoomDef,
    BuildingValidationError, BuildingValidationReport, CircuitDef, FloorDef, FloorKind, PortalDef,
    PortalKind, PortalLock, RoomRef, ZoneDef, load_building, scenario_floor_portals,
};
pub use command::{
    Button, Buttons, Command, Edge, MoleOp, PivotTarget, RunConfig, TickInput, fold,
};
pub use common::BillyMode;
pub use common::{
    Channel, ChuteMethod, CrisisReason, DenyReason, ExtractMethod, FailReason, Grade,
    MoleDenyReason, ObjectKind, ObjectiveStatus, Outcome, Phase, ReportedTarget, Severity, Tone,
};
pub use definition::{
    BillyDef, CameraDef, Check, DoorDef, HideSpotDef, MoleHostDef, ObjectiveDef, ObjectiveKind,
    PlayerDef, PropSpawn, PropsDef, RoomDef, ScenarioDefinition, SpawnRanges, ValidationError,
    ValidationReport, WorldDef,
};
pub use event::{Event, LogLine, format_time, log_view};
pub use floor_graph::{
    FLOOR_GRAPH_JSON, VACUUM_NODE_ID, camera_node_id, door_node_id, floor_graph, inside_vantage,
    light_node_id, pivot_host, pump_node_id, valve_node_id, van_vantage,
};
pub use ghost_lobby::{GHOST_LOBBY_JSON, ghost_lobby};
pub use ids::{CameraId, DoorId, HideSpotId, IdIndex, ObjectiveId, RoomId};
pub use infra::{
    AlarmDef, AlarmTrip, Hearing, InfraDef, InfraState, NoiseSource, Observation, ObservationKind,
    ReleaseCause, WaterZoneDef, WaterZoneState,
};
pub use mathf::TICK_DT;
pub use outcome::{Debrief, ScoreBreakdown, Tag, debrief_text, grade_for};
pub use pump_house::{HALL_SUMP, PUMP_HOUSE_ID, pump_house};
pub use rng::{InitRoll, Mulberry32, roll_init};
pub use rooftop::ghost_lobby_rooftop;
pub use sim::GhostLobbySim;
pub use snapshot::{
    DebriefExport, DefinitionExport, RuntimeSnapshot, SNAPSHOT_FORMAT, ScenarioExport,
};
pub use state::{DiversionState, MoleHostState, RuntimeState, Stats};
pub use tuning::{
    ActionKind, ActionSpec, DifficultyId, DifficultyPreset, GradeBands, ScoringDef, TuningConstants,
};
pub use vsm::{
    DifficultyProposal, EvidenceLedger, FocalMass, GhostLobbySupervisor, Hypothesis,
    HypothesisEvidence, HypothesisLedger, MASS_TOTAL, Operator, OperatorTeam,
    PlayerTacticObservation, SupervisionState, TACTIC_INFRA_SABOTAGE, VsmDirector, VsmEvent,
    VsmSystem, ghost_lobby_evidence, is_infra_sabotage,
};

//! Secondary spaces: ducts, maintenance passages, cable conduits, traversable
//! pipes, excavatable material and structural barriers beneath the Ghost
//! Lobby floor, and Moletaire's access to them.
//!
//! # Vertical slice
//!
//! Moletaire enters an authored maintenance route at an access point,
/// reaches a real service device (a grounded-network node), performs one
/// action (sabotaging a real cable), and returns; the action changes the
//! main scenario through the normal power-cascade systems.
//!
//! Layout: `model` (authored content + runtime state), `access` (who may
//! traverse what), `topology` (spatial queries), `events` (log renderings),
//! `host` (integration into GhostLobbySim) and `fixture` (the authored
//! Ghost Lobby secondary network).
//!
//! Nothing here runs unless [`crate::scenario::definition::ScenarioDefinition::secondary`]
//! is Some; when absent every existing replay and snapshot is unchanged.

pub mod access;
pub mod events;
pub mod fixture;
pub mod host;
pub mod model;
pub mod topology;

pub use access::{route_accessibility, RouteAccess};
pub use events::SecondaryEvent;
pub use host::{AccessPointDebug, MoleDebug, RouteDebugEntry, RouteDebugView};
pub use model::{
    ActorBody, ActorClass, ActorClassSet, CableId, CableState, Clearance, EvidenceKind,
    EntryRefusalReason, ObstructionReason, RouteId, RouteState, SabotageRefusalReason,
    SecondarySpec, SecondaryState, SpaceId,
};
pub use topology::{discovered_routes, resolve_sabotage_cable, RouteNetwork};

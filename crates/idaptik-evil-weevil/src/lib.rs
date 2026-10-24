//! # Evil Weevil — IDApTIK's adversarial testing adapter (ADR-0013)
//!
//! A separate, deterministic, **sans-IO** test adapter that exercises
//! IDApTIK's multiplayer acceptance rules and recovery behaviour inside
//! isolated, authorised test environments. It never opens a socket, runs a
//! shell command, resolves a remote address or touches a database: every
//! endpoint is an in-memory model and every identity a disposable dummy.
//!
//! ## Four permission realms, never interchangeable
//!
//! | Component | Module | Powers |
//! |---|---|---|
//! | Evil Weevil agent | [`agent`] | propose bounded intents only |
//! | Test harness | [`harness`] | authorise attempts, own the in-memory wire, feed the real production validators, record outcomes |
//! | Defensive observer | [`observer`] | read validated telemetry, classify, recommend — read-only |
//! | Host operator policy | [`enforce`] | choose observe-only/local-protective; local actions only |
//!
//! The allow-list lives in [`policy`]. There is no permission to bypass a
//! host validation, mutate sim state directly, or act against a remote
//! machine. The gameplay guards themselves —
//! [`idaptik_net::envelope::decode_command`],
//! [`idaptik_net::lockstep::LockstepCore`] and
//! [`idaptik_core::scenario::GhostLobbySim`] — are unchanged production code
//! and keep the final accept/reject decision.
//!
//! ## Data flow
//!
//! 1. [`agent::EvilWeevil`] proposes an [`agent::Intent`] carrying a **raw,
//!    unsanitised** wire payload ([`wire`]).
//! 2. [`policy::TestPolicy`] decides whether the attempt is authorised in
//!    the scenario.
//! 3. [`relay_model::RelayModel`] applies the documented burble relay rules
//!    offline (role table, `seq` de-duplication).
//! 4. Relayed frames go through the **real** production decoders and
//!    lockstep/sim validators inside [`harness`]; outcomes become validated
//!    [`telemetry::TelemetryEvent`]s.
//! 5. [`observer::DefensiveObserver`] classifies the telemetry (faults are
//!    never hostile) and recommends; [`enforce::EnforcementPolicy`] decides
//!    any local response.
//! 6. [`report`] records the versioned, redacted corpus artifact.
//!
//! The five fixed scenarios live in [`scenarios`]. Adaptive action selection
//! is deliberately absent until the fixed corpus proves its coverage; there
//! is no LLM in this crate.
//!
//! ## Scope and non-goals
//!
//! - No new transport or consensus protocol; the adapter models the existing
//!   relay-only rendezvous topology, and records unimplemented topologies
//!   (P2P game lanes, federation, authoritative host, game-server-admin) as
//!   gaps rather than simulating them.
//! - No native code, FFI, NIF/SNIF usage, or `unsafe`; the wasm32 behaviour
//!   of any native ABI is out of scope. SNIF guests in burble remain pure
//!   numeric/buffer DSP kernels with no network or database I/O.
//! - No availability testing, scanning, fuzzing of live systems, arbitrary
//!   target addresses, or unrestricted shell execution. Bounds are in
//!   [`limits`] and enforced on every run.

#![forbid(unsafe_code)]

pub mod agent;
pub mod enforce;
pub mod harness;
pub mod limits;
pub mod observer;
pub mod policy;
pub mod relay_model;
pub mod report;
pub mod scenarios;
pub mod telemetry;
pub mod wire;

pub use report::CorpusReport;
pub use scenarios::{CORPUS_VERSION, Scenario, ScenarioResult, corpus};

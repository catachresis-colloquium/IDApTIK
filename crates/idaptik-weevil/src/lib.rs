//! A thin Rust host adapter over the Evil Weevil native C ABI.
//!
//! Evil Weevil is a host-agnostic enemy-intelligence kernel: it owns perception
//! memory and decision state behind a frozen, Idris2-proven C ABI, and it never
//! calls back into the host mid-tick. The game supplies a validated snapshot
//! once per tick and receives a proposed intent.
//!
//! This crate is the seam between the two. It is deliberately small and it
//! respects the architectural boundary:
//!
//! * The **game** owns world truth, physics, navigation, collision, guard
//!   identity and entity lifecycle, perception generation, and authoritative
//!   action validation and application.
//! * The **kernel** owns its documented per-agent memory and decision state, and
//!   selects proposed intents.
//!
//! # What this crate guarantees
//!
//! * It uses the **generated** ABI (`ee.h`) and never hand-edits it. The struct
//!   mirrors are transcriptions with compile-time layout assertions, so a moved
//!   header fails the build here as well as in the host's C compiler.
//! * It never passes native pointers across a boundary it does not own, and it
//!   never hands a Rust-native layout straight across the C boundary: every
//!   value is converted and range-checked at the seam.
//! * It validates conversions, enum values, ranges and lifetimes, and it keeps
//!   the host's authority: the kernel proposes, the host decides.
//! * It records enough state to replay a decision, and it checks replay equality
//!   only within an explicitly supported configuration.
//!
//! # What it does not claim
//!
//! * The kernel's weights are not configurable through this adapter — they are
//!   not ABI (ADR-0010). Profiles vary the seams the architecture assigns, not
//!   the kernel's policy numbers.
//! * The ABI's bearing is a `u16` the kernel zero-extends, so the kernel cannot
//!   express a signed direction. The host therefore applies its own navigation;
//!   the kernel's move vector is validated and recorded but not trusted for
//!   steering. See [`observation`]'s module docs.
//! * Native pointer checks are not memory isolation. This adapter is an
//!   in-process seam, not a security boundary against a hostile kernel.

pub mod abi;
pub mod canonical;
pub mod epistemic;
pub mod error;
pub mod fx;
pub mod host;
pub mod intent;
pub mod mind_trace;
pub mod observation;
pub mod profile;
pub mod replay;
pub mod world;

pub use error::{Result, WeevilError};
pub use fx::Fx2;
pub use host::{
    AgentState, Exposure, Memory, MindOutcome, MindRecord, ModelOutcome, TickOutcome, WeevilHost,
};
pub use intent::{Action, Intent, IntentFlags};
pub use observation::{Contact, ContactSource, Observation};
pub use profile::{HostTuning, Profile, ProfileSet};
pub use replay::{
    AppliedEffect, InitialContext, IntentRecord, ObservationRecord, ReplayEntry, ReplayHeader,
    ReplayMismatch, ReplayRecorder, ReplayTrace, decision_identity, header_from, replay_equal,
};
pub use world::{Entrance, HostWorld, Step, TickTruth, contact_knows};

/// Run a bounded headless trace through the adapter and return the replay.
///
/// This drives the kernel with the deterministic corridor model in
/// [`world`], recording every observation, proposal, host acceptance and applied
/// effect. It is the reproducible replay the first milestone ships.
pub fn run_trace(ticks: u64, profile: &Profile, seed: u64) -> Result<ReplayTrace> {
    run_trace_with_truth(ticks, profile, seed).map(|(trace, _)| trace)
}

/// [`run_trace`], also returning the corridor's truth for every tick, taken
/// when the guard observed.
///
/// The trace is the same one [`run_trace`] returns: it is the one loop, and
/// recording the truth reads the world without changing it.
pub fn run_trace_with_truth(
    ticks: u64,
    profile: &Profile,
    seed: u64,
) -> Result<(ReplayTrace, Vec<TickTruth>)> {
    run_corridor(ticks, profile, seed, false).map(|run| (run.trace, run.truth))
}

/// One headless corridor run: the replay, the world's truth on every tick, and,
/// when the run went through the ABI 1.1 mind, one record per tick; when it
/// went through the ABI 1.2 model trace, one exposure record per tick as well.
#[derive(Debug, Clone)]
pub struct CorridorRun {
    pub trace: ReplayTrace,
    pub truth: Vec<TickTruth>,
    /// Empty unless the run used `ee_tick_mind`.
    pub mind: Vec<host::MindRecord>,
    /// Empty unless the run used the 1.2 model trace.
    pub exposure: Vec<host::Exposure>,
}

/// The scenario id a corridor run records: a mind run is a different decision
/// sequence from a plain one, and a model run (the 1.2 trace and the cue) from
/// a mind run, so no two of them may replay as equal.
pub fn corridor_scenario_id(entrance: Entrance) -> &'static str {
    match entrance {
        Entrance::Tick => "headless-corridor",
        Entrance::Mind => "headless-corridor-mind",
        Entrance::Model => "headless-corridor-model",
    }
}

/// The entrance a corridor run of `profile` takes: `ee_tick`, or with `mind`
/// the mind, which for a model scenario
/// ([`profile::CorridorScript::model`]) is the 1.2 model trace.
pub fn corridor_entrance(profile: &Profile, mind: bool) -> Entrance {
    let model = profile.host.script.as_ref().is_some_and(|s| s.model);
    match (mind, model) {
        (false, _) => Entrance::Tick,
        (true, false) => Entrance::Mind,
        (true, true) => Entrance::Model,
    }
}

/// Run the corridor through `ee_tick`, or with `mind` through `ee_tick_mind`
/// (with the model trace for a model scenario).
pub fn run_corridor(ticks: u64, profile: &Profile, seed: u64, mind: bool) -> Result<CorridorRun> {
    let entrance = corridor_entrance(profile, mind);
    let mut host = WeevilHost::init(
        profile.capabilities,
        profile.max_agents,
        profile.budget_units,
        seed,
        60,
        1,
    )?;
    let mut world = HostWorld::canonical(profile);
    let mut recorder = ReplayRecorder::new(header_from(
        &host,
        profile,
        seed,
        (60, 1),
        corridor_scenario_id(entrance),
    ));
    let mut truth = Vec::with_capacity(ticks as usize);
    let mut records = Vec::new();
    let mut exposure = Vec::new();
    for _ in 0..ticks {
        let step = world.step_with(&mut host, profile, &mut recorder, entrance)?;
        truth.push(step.truth);
        records.extend(step.mind);
        exposure.extend(step.exposure);
    }
    Ok(CorridorRun {
        trace: recorder.finish(),
        truth,
        mind: records,
        exposure,
    })
}

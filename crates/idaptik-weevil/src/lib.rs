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
pub mod error;
pub mod fx;
pub mod host;
pub mod intent;
pub mod observation;
pub mod profile;
pub mod replay;
pub mod world;

pub use error::{Result, WeevilError};
pub use fx::Fx2;
pub use host::{AgentState, Memory, TickOutcome, WeevilHost};
pub use intent::{Action, Intent, IntentFlags};
pub use observation::{Contact, ContactSource, Observation};
pub use profile::{HostTuning, Profile, ProfileSet};
pub use replay::{
    AppliedEffect, InitialContext, IntentRecord, ObservationRecord, ReplayEntry, ReplayHeader,
    ReplayMismatch, ReplayRecorder, ReplayTrace, decision_identity, header_from, replay_equal,
};
pub use world::HostWorld;

/// Run a bounded headless trace through the adapter and return the replay.
///
/// This drives the kernel with the deterministic corridor model in
/// [`world`], recording every observation, proposal, host acceptance and applied
/// effect. It is the reproducible replay the first milestone ships.
pub fn run_trace(
    ticks: u64,
    profile: &Profile,
    seed: u64,
) -> Result<ReplayTrace> {
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
        "headless-corridor",
    ));
    for _ in 0..ticks {
        world.step(&mut host, profile, &mut recorder)?;
    }
    Ok(recorder.finish())
}

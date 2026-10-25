//! Durable session storage for IDApTIK.
//!
//! # What this crate is, and is not
//!
//! It is a **store beside the tick**, not a database the simulation talks to.
//! `GhostLobbySim::tick`, `BuildingSim::apply` and `MoletaireSim::tick` stay
//! in memory, synchronous and pure; nothing here is on the hot path. What this
//! crate holds is the three things a run produces and cannot keep:
//!
//! 1. **the command log** — every `Command`, in order, keyed by a store-assigned
//!    sequence number;
//! 2. **checkpoints** — `RuntimeSnapshot`s, deduplicated against the definition
//!    they embed;
//! 3. **the session index** — what exists, what it can be resumed to.
//!
//! # The one invariant that matters
//!
//! **Commands are primary; checkpoints are derived.** A checkpoint may be
//! deleted at any time and rebuilt from `seed + config + commands`. A command
//! may never be reconstructed from a checkpoint. [`Store::verify`] exists to
//! keep that true: it replays the log independently of the checkpoints and
//! reports every disagreement.
//!
//! # Why `seq` and not `tick`
//!
//! `GhostLobbySim::reset` sets `self.tick = 0`, and `Command::Restart` calls it.
//! So a restarted run's tick numbers repeat: keying the log on `(session, tick)`
//! would make a post-restart tick 5 silently overwrite the pre-restart tick 5,
//! in both the command table and the checkpoint table. Nothing would report it.
//!
//! The key is therefore a store-assigned `seq` that never rewinds. The `tick`
//! and the `epoch` (bumped by the caller on each restart) are carried as
//! payload, so the log is still faithful — `Restart` is in the log like every
//! other command — without anything colliding.
//!
//! # Why `held` is stored at all
//!
//! `Buttons` — the held movement keys — lives in the frontend
//! (`CommandQueue.held`), not in `RuntimeSnapshot`. A resume that restored only
//! the snapshot would start with no buttons held while the recorded run had
//! some, and the two would diverge on the first tick after the resume — while
//! `verify`, which replays from `seq 0`, would still pass. A divergence that
//! verification cannot see is the worst failure this layer can have, so the
//! checkpoint records `held`, and `resume` rebuilds it by folding the logged
//! commands through [`idaptik_core::scenario::fold`] rather than reimplementing
//! "which command changes which button".
//!
//! # Storage layout
//!
//! ```text
//! META        &str          -> bytes   schema tag
//! SESSIONS    &str          -> json    SessionRecord
//! COMMANDS    (&str, u64)   -> json    CommandBatch
//! CHECKPOINTS (&str, u64)   -> json    CheckpointRecord
//! BLOBS       &str          -> bytes   scenario definition, keyed by digest
//! ```
//!
//! `BLOBS` is not an optimisation. A `BuildingSnapshot` is about 17 KB, of
//! which the definition is about 13 KB and the game state 0.5 KB; storing the
//! definition inline in every checkpoint would make the save 35× larger than
//! the thing being saved. Checkpoints therefore carry a **digest** and the
//! definition is stored once.
//!
//! The digest is FNV-1a 64 over the definition bytes. It is an *identity*
//! hash — enough to deduplicate and to catch accidental corruption — not a
//! cryptographic one, and nothing here treats it as a security boundary.

#![forbid(unsafe_code)]

pub mod digest;
pub mod error;
pub mod record;
pub mod schema;
pub mod store;

pub use error::PersistError;
pub use record::{
    CheckpointRecord, CommandBatch, FlushPolicy, Resume, SessionId, SessionRecord, SessionStatus,
    SimKind, VerifyIssue, VerifyReport,
};
pub use store::Store;

/// The persist-schema version. Bumped only when the *layout* changes.
///
/// This is deliberately not the same thing as a snapshot's own `format` tag.
/// A store that opened a newer layout and wrote into it would corrupt data it
/// does not understand, so [`Store::open`] refuses a mismatch outright rather
/// than migrating. Snapshots carry the same discipline one level down: a
/// `idaptik-ghost-lobby-runtime-v2` body does not restore into a v3 build, and
/// this crate propagates that refusal instead of papering over it.
pub const SCHEMA_VERSION: u32 = 1;

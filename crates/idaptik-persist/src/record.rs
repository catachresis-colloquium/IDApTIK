//! The records the store keeps: one per session, one per command batch, one per
//! checkpoint — plus the reports `resume` and `verify` return.

use idaptik_core::scenario::{Buttons, Command, RuntimeSnapshot};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Identifies one recorded run. Opaque to callers; the store never parses it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(String);

impl SessionId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Which simulation produced a session.
///
/// The three runtimes in this repository do **not** share a replay contract,
/// so the kind is recorded rather than inferred:
///
/// | kind | replay is exact when… |
/// |---|---|
/// | [`SimKind::GhostLobby`] | the seed, the RNG and every tick's `TickInput` are recorded |
/// | [`SimKind::Building`] | every `BuildingCommand` is recorded — there is no RNG and no tick |
/// | Moletaire *(not supported yet)* | a per-tick `dt` is recorded, which nothing currently does |
///
/// Moletaire is absent on purpose. `MoletaireSnapshot` carries `seed` and
/// `tick` but not `dt`, and `MoletaireSim::tick` takes a variable `dt`, so a
/// checkpoint of it is not replayable until the caller also logs the frame
/// delta. Adding the variant before that would make the type claim something
/// the data cannot support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SimKind {
    GhostLobby,
    Building,
}

impl SimKind {
    /// The snapshot format tag this kind can restore.
    pub fn expected_format(self) -> &'static str {
        match self {
            Self::GhostLobby => idaptik_core::scenario::SNAPSHOT_FORMAT,
            // `BuildingSim::restore` gates on its own constant; recording it
            // here means a store written for one sim kind cannot be resumed by
            // the other by accident.
            Self::Building => "idaptik-building-runtime-v1",
        }
    }
}

/// How often the caller should flush.
///
/// This is advice recorded with the session, not a timer the store runs: the
/// store is synchronous and knows nothing about frames. It exists so the two
/// runtimes' genuinely different cadences are written down once instead of
/// being rediscovered per integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FlushPolicy {
    /// Batch for `millis`, then commit one transaction.
    ///
    /// Correct for `GhostLobby`: 60 Hz of dense `SetButton` traffic, where a
    /// second of lost play is cheap and 60× fewer transactions matters.
    TimeBatched { millis: u64 },
    /// Flush after every command.
    ///
    /// Correct for `Building`: its nine-command canonical run *is* the whole
    /// game, there is no tick rate to batch against, and a checkpoint costs
    /// about 500 bytes of new information.
    PerCommand,
}

/// Whether a session is still being written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionStatus {
    Open,
    Closed,
}

/// Everything the store knows about a session without opening its log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionRecord {
    /// Which simulation wrote this.
    pub sim_kind: SimKind,
    /// The scenario id the definition declares, for display and lookup.
    pub scenario_id: String,
    /// The run seed. Recorded even for [`SimKind::Building`], which does not
    /// use one, so the field has the same meaning in every session.
    pub seed: u32,
    /// The snapshot `format` tag the checkpoints carry.
    pub format_tag: String,
    /// Unix seconds at which the session was begun.
    pub created_unix: u64,
    /// The highest `seq` written, or `None` for a session whose log is still
    /// empty.
    ///
    /// `Option` rather than a `0` sentinel: `seq 0` is a legitimate row, and a
    /// sentinel would make the first batch unwritable. The distinction matters
    /// to [`crate::Store::verify`], which can only reconstruct `held` when the
    /// log really does begin at zero.
    pub last_seq: Option<u64>,
    /// The epoch of the most recent batch. Bumped by the caller on `Restart`,
    /// because the sim's own `tick` rewinds there.
    pub last_epoch: u64,
    /// The `seq` of every checkpoint, ascending.
    pub checkpoints: Vec<u64>,
    pub status: SessionStatus,
    /// The cadence the caller intends to use. Advisory; see [`FlushPolicy`].
    pub flush: FlushPolicy,
}

/// One tick's (or one command's) worth of input, as written to the log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandBatch {
    /// Restart generation. Starts at 0 and increments on every `Restart`.
    pub epoch: u64,
    /// The simulation's own tick counter **at the time of writing**. Recorded
    /// for diagnostics only — never used as a key, because `Restart` rewinds it.
    pub tick: u64,
    pub cmds: Vec<Command>,
}

/// A stored checkpoint.
///
/// The snapshot's `definition` field is removed and replaced by
/// `definition_digest`; the bytes live once in the `BLOBS` table. Everything
/// else in the snapshot is kept verbatim in `body`, so this crate does not have
/// to know the shape of `RuntimeState`, `Mulberry32` or anything else that may
/// change between snapshot versions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckpointRecord {
    pub seq: u64,
    pub epoch: u64,
    /// The simulation's tick at checkpoint time. `Restart` rewinds it, so this
    /// is payload, not identity.
    pub tick: u64,
    /// The frontend's held-button set. Not part of `RuntimeSnapshot`; see the
    /// crate docs for why a resume that omitted it would diverge invisibly.
    pub held: Buttons,
    /// The frontend's pause latch, which is *also* not necessarily the
    /// snapshot's `paused` field — the two are maintained separately.
    pub paused: bool,
    pub definition_digest: String,
    /// The snapshot JSON with `definition` removed.
    pub body: serde_json::Value,
}

/// What a caller needs to continue a run.
#[derive(Debug, Clone, PartialEq)]
pub struct Resume {
    /// The newest checkpoint, with its definition spliced back in.
    pub snapshot: RuntimeSnapshot,
    /// Held buttons as of the newest checkpoint, advanced through every logged
    /// command after it.
    pub held: Buttons,
    /// Pause latch as of the newest checkpoint.
    pub paused: bool,
    /// The checkpoint this resumed from.
    pub checkpoint_seq: u64,
    /// The next `seq` the caller should write.
    pub next_seq: u64,
    /// The epoch to write on the next batch.
    pub epoch: u64,
    /// Batches replayed after the checkpoint to rebuild `held`. Zero when the
    /// checkpoint is the newest thing in the log.
    pub replayed_batches: usize,
}

/// One disagreement found by [`crate::Store::verify`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "issue", rename_all = "kebab-case")]
pub enum VerifyIssue {
    /// `seq` numbers are not contiguous from the checkpoint floor upward.
    MissingSeq { expected: u64, found: u64 },
    /// The log does not begin at `seq 0`, so `held` cannot be reconstructed
    /// from it and the held-set comparison is not meaningful for this session.
    NonZeroStart { first: u64 },
    /// A command batch does not decode.
    UndecodableCommand { seq: u64, detail: String },
    /// A checkpoint does not decode.
    UndecodableCheckpoint { seq: u64, detail: String },
    /// A checkpoint names a definition blob that is not in the store.
    MissingDefinition { seq: u64, digest: String },
    /// A definition blob's bytes do not hash to its key.
    DefinitionDigestMismatch { digest: String, computed: String },
    /// The held-button set recorded at a checkpoint is not the set the log
    /// replays to. This is the check that a snapshot-only resume would fail.
    HeldMismatch {
        seq: u64,
        recorded: Buttons,
        replayed: Buttons,
    },
    /// A checkpoint's `tick` is behind the last batch at or below its `seq`.
    TickBehindLog { seq: u64, recorded: u64, logged: u64 },
    /// Epoch went backwards between consecutive batches.
    EpochRegressed { seq: u64, from: u64, to: u64 },
}

/// The outcome of a verification pass.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerifyReport {
    pub session: String,
    /// Batches walked.
    pub commands: usize,
    /// Checkpoints walked.
    pub checkpoints: usize,
    /// Empty means the log and the checkpoints agree.
    pub issues: Vec<VerifyIssue>,
}

impl VerifyReport {
    pub fn ok(&self) -> bool {
        self.issues.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_id_round_trips_transparently() {
        let id = SessionId::new("s-1");
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"s-1\"");
        assert_eq!(id.as_str(), "s-1");
        assert_eq!(id.to_string(), "s-1");
    }

    #[test]
    fn sim_kind_tags_are_kebab_and_distinct() {
        assert_eq!(
            serde_json::to_string(&SimKind::GhostLobby).unwrap(),
            "\"ghost-lobby\""
        );
        assert_eq!(
            serde_json::to_string(&SimKind::Building).unwrap(),
            "\"building\""
        );
        assert_ne!(
            SimKind::GhostLobby.expected_format(),
            SimKind::Building.expected_format()
        );
    }
}

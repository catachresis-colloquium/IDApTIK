//! The store itself: open, append, checkpoint, resume, verify.

use crate::SCHEMA_VERSION;
use crate::digest::digest_hex;
use crate::error::PersistError;
use crate::record::{
    CheckpointRecord, CommandBatch, Resume, SessionId, SessionRecord, SimKind, VerifyIssue,
    VerifyReport,
};
use crate::schema::{self, BLOBS, CHECKPOINTS, COMMANDS, META, SESSIONS};
use idaptik_core::scenario::{Buttons, Command, RuntimeSnapshot, fold};
use redb::{Database, ReadableTable};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// One indexed session, including a decode error when its record is damaged.
pub type SessionListEntry = (SessionId, Result<SessionRecord, String>);

/// A durable store. Cheap to keep alive; every method opens its own redb
/// transaction.
///
/// Deliberately not `Clone`: one handle per writer. `redb` itself would permit
/// concurrent writers, but a command log with two writers is a command log with
/// two opinions about `last_seq`, and the contention would surface as a
/// `SeqRegression` far from the code that caused it.
pub struct Store {
    db: Database,
    path: PathBuf,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").field("path", &self.path).finish()
    }
}

impl Store {
    /// Open (creating if absent) the store in `dir`.
    ///
    /// Refuses a store whose schema tag is not [`SCHEMA_VERSION`]. There is no
    /// migration path on purpose: a store that silently upgraded would be a
    /// store that silently reinterpreted somebody else's bytes.
    pub fn open(dir: impl AsRef<Path>) -> Result<Self, PersistError> {
        let dir = dir.as_ref();
        std::fs::create_dir_all(dir)?;
        let path = dir.join("idaptik.redb");
        let db = Database::create(&path)?;
        let store = Self { db, path };
        store.write_schema_tag()?;
        Ok(store)
    }

    /// The file this store is backed by.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Create the tables and check (or write) the schema tag.
    fn write_schema_tag(&self) -> Result<(), PersistError> {
        let txn = self.db.begin_write()?;
        {
            // Opening every table makes the file fully shaped even when empty,
            // so a later `verify` on a fresh store reads structurally rather
            // than tripping over a missing table.
            let _sessions = txn.open_table(SESSIONS)?;
            let _commands = txn.open_table(COMMANDS)?;
            let _checkpoints = txn.open_table(CHECKPOINTS)?;
            let _blobs = txn.open_table(BLOBS)?;

            let mut meta = txn.open_table(META)?;
            let existing = meta
                .get(schema::SCHEMA_KEY)?
                .map(|guard| guard.value().to_vec());
            match existing {
                None => {
                    let bytes = serde_json::to_vec(&SCHEMA_VERSION)?;
                    meta.insert(schema::SCHEMA_KEY, bytes.as_slice())?;
                }
                Some(bytes) => {
                    let found: u32 =
                        serde_json::from_slice(&bytes).map_err(PersistError::Decode)?;
                    if found != SCHEMA_VERSION {
                        return Err(PersistError::SchemaMismatch {
                            found,
                            expected: SCHEMA_VERSION,
                        });
                    }
                }
            }
        }
        txn.commit()?;
        Ok(())
    }

    // --- sessions -----------------------------------------------------------

    /// Record a new session under a caller-chosen id.
    ///
    /// The id is supplied rather than generated so that tests are
    /// deterministic; use [`SessionId::generate`] when one is wanted.
    pub fn begin_session(&self, id: &SessionId, rec: &SessionRecord) -> Result<(), PersistError> {
        let txn = self.db.begin_write()?;
        {
            let mut sessions = txn.open_table(SESSIONS)?;
            if sessions.get(id.as_str())?.is_some() {
                return Err(PersistError::SessionExists(id.clone()));
            }
            let bytes = serde_json::to_vec(rec)?;
            sessions.insert(id.as_str(), bytes.as_slice())?;
        }
        txn.commit()?;
        Ok(())
    }

    /// Read one session record.
    pub fn load_session(&self, id: &SessionId) -> Result<SessionRecord, PersistError> {
        let txn = self.db.begin_read()?;
        let sessions = txn.open_table(SESSIONS)?;
        match sessions.get(id.as_str())? {
            None => Err(PersistError::UnknownSession(id.clone())),
            Some(guard) => serde_json::from_slice(guard.value()).map_err(PersistError::Decode),
        }
    }

    /// Every session in the store, ordered by id.
    ///
    /// A record that will not decode is reported rather than skipping the whole
    /// enumeration: one unreadable session must not hide the other twenty.
    pub fn list_sessions(&self) -> Result<Vec<SessionListEntry>, PersistError> {
        let txn = self.db.begin_read()?;
        let sessions = txn.open_table(SESSIONS)?;
        let mut out = Vec::new();
        for entry in sessions.iter()? {
            let (k, v) = entry?;
            let id = SessionId::new(k.value());
            out.push((
                id,
                serde_json::from_slice(v.value()).map_err(|e| e.to_string()),
            ));
        }
        Ok(out)
    }

    /// Mark a session closed. Resume still works; the flag only says the writer
    /// is finished.
    pub fn close_session(&self, id: &SessionId) -> Result<(), PersistError> {
        let txn = self.db.begin_write()?;
        {
            let mut sessions = txn.open_table(SESSIONS)?;
            let mut rec = read_session(&sessions, id)?;
            rec.status = crate::record::SessionStatus::Closed;
            let bytes = serde_json::to_vec(&rec)?;
            sessions.insert(id.as_str(), bytes.as_slice())?;
        }
        txn.commit()?;
        Ok(())
    }

    // --- the log ------------------------------------------------------------

    /// Append one batch of commands at `seq`.
    ///
    /// The batch row and the session's `last_seq` are written in **one**
    /// transaction, so a crash cannot leave the index claiming a `seq` the log
    /// does not contain.
    pub fn append_commands(
        &self,
        id: &SessionId,
        seq: u64,
        epoch: u64,
        tick: u64,
        cmds: &[Command],
    ) -> Result<(), PersistError> {
        let txn = self.db.begin_write()?;
        {
            let mut sessions = txn.open_table(SESSIONS)?;
            let mut rec = read_session(&sessions, id)?;
            if let Some(last) = rec.last_seq
                && seq <= last
            {
                return Err(PersistError::SeqRegression {
                    last,
                    attempted: seq,
                });
            }
            let batch = CommandBatch {
                epoch,
                tick,
                cmds: cmds.to_vec(),
            };
            let bytes = serde_json::to_vec(&batch)?;
            let mut table = txn.open_table(COMMANDS)?;
            table.insert((id.as_str(), seq), bytes.as_slice())?;
            drop(table);

            rec.last_seq = Some(seq);
            rec.last_epoch = epoch;
            let rec_bytes = serde_json::to_vec(&rec)?;
            sessions.insert(id.as_str(), rec_bytes.as_slice())?;
        }
        txn.commit()?;
        Ok(())
    }

    // --- checkpoints --------------------------------------------------------

    /// Store a snapshot as a checkpoint at `seq`.
    ///
    /// The definition is pulled out into the blob table and replaced by its
    /// digest. `held` and `paused` come from the frontend and are **not** part
    /// of the snapshot; see the crate docs.
    ///
    /// The checkpoint row, any new definition blob, and the session index all
    /// commit together.
    #[allow(clippy::too_many_arguments)]
    pub fn checkpoint(
        &self,
        id: &SessionId,
        seq: u64,
        epoch: u64,
        snapshot: &RuntimeSnapshot,
        held: Buttons,
        paused: bool,
    ) -> Result<(), PersistError> {
        let txn = self.db.begin_write()?;
        {
            let mut sessions = txn.open_table(SESSIONS)?;
            let mut rec = read_session(&sessions, id)?;
            if snapshot.format != rec.format_tag {
                return Err(PersistError::FormatMismatch {
                    found: snapshot.format.clone(),
                    expected: rec.format_tag.clone(),
                });
            }

            // Pull the definition out. Everything else in the snapshot is
            // stored verbatim, so this crate never has to model `RuntimeState`.
            let mut body = serde_json::to_value(snapshot)?;
            let obj = body.as_object_mut().ok_or(PersistError::NotAnObject)?;
            let definition = obj.remove("definition").ok_or(PersistError::NotAnObject)?;
            let definition_bytes = serde_json::to_vec(&definition)?;
            let digest = digest_hex(&definition_bytes);

            {
                let mut blobs = txn.open_table(BLOBS)?;
                if blobs.get(digest.as_str())?.is_none() {
                    blobs.insert(digest.as_str(), definition_bytes.as_slice())?;
                }
            }

            let record = CheckpointRecord {
                seq,
                epoch,
                tick: snapshot.tick,
                held,
                paused,
                definition_digest: digest,
                body,
            };
            let bytes = serde_json::to_vec(&record)?;
            let mut checkpoints = txn.open_table(CHECKPOINTS)?;
            checkpoints.insert((id.as_str(), seq), bytes.as_slice())?;
            drop(checkpoints);

            if !rec.checkpoints.contains(&seq) {
                rec.checkpoints.push(seq);
                rec.checkpoints.sort_unstable();
            }
            match rec.last_seq {
                Some(last) if last >= seq => {}
                _ => rec.last_seq = Some(seq),
            }
            if epoch > rec.last_epoch {
                rec.last_epoch = epoch;
            }
            let rec_bytes = serde_json::to_vec(&rec)?;
            sessions.insert(id.as_str(), rec_bytes.as_slice())?;
        }
        txn.commit()?;
        Ok(())
    }

    /// Store a definition blob without a checkpoint, returning its digest.
    ///
    /// Useful when a caller wants to record the authored definition before the
    /// first tick.
    pub fn put_definition(&self, bytes: &[u8]) -> Result<String, PersistError> {
        let digest = digest_hex(bytes);
        let txn = self.db.begin_write()?;
        {
            let mut blobs = txn.open_table(BLOBS)?;
            if blobs.get(digest.as_str())?.is_none() {
                blobs.insert(digest.as_str(), bytes)?;
            }
        }
        txn.commit()?;
        Ok(digest)
    }

    // --- resume -------------------------------------------------------------

    /// Restore the newest checkpoint and bring `held` up to date.
    ///
    /// Only [`SimKind::GhostLobby`] has a typed payload in this version.
    /// Building sessions can be written and verified already; their typed
    /// resume arrives with the runtime that consumes it, because inventing a
    /// payload type here would be a second definition of `BuildingSnapshot`.
    pub fn resume(&self, id: &SessionId) -> Result<Resume, PersistError> {
        let rec = self.load_session(id)?;
        if rec.sim_kind != SimKind::GhostLobby {
            return Err(PersistError::UnsupportedSimKind(rec.sim_kind));
        }
        let checkpoint_seq = *rec
            .checkpoints
            .last()
            .ok_or_else(|| PersistError::NoCheckpoint(id.clone()))?;

        let txn = self.db.begin_read()?;
        let checkpoints = txn.open_table(CHECKPOINTS)?;
        let record = match checkpoints.get((id.as_str(), checkpoint_seq))? {
            None => return Err(PersistError::NoCheckpoint(id.clone())),
            Some(guard) => serde_json::from_slice::<CheckpointRecord>(guard.value())
                .map_err(PersistError::Decode)?,
        };
        drop(checkpoints);

        let (snapshot, paused, mut held) = materialise(&txn, record)?;

        // Advance `held` through everything logged after the checkpoint. This
        // goes through `fold` rather than matching on `SetButton`, so the store
        // and the live tick cannot disagree about what a command means.

        let commands = txn.open_table(COMMANDS)?;
        let mut replayed = 0usize;
        let range = commands
            .range((id.as_str(), checkpoint_seq.saturating_add(1))..=(id.as_str(), u64::MAX))?;
        for entry in range {
            let (_k, v) = entry?;
            let batch: CommandBatch =
                serde_json::from_slice(v.value()).map_err(PersistError::Decode)?;
            let _ = fold(&batch.cmds, &mut held);
            replayed += 1;
        }
        drop(commands);

        Ok(Resume {
            snapshot,
            held,
            paused,
            checkpoint_seq,
            next_seq: match rec.last_seq {
                Some(last) => last.saturating_add(1),
                None => 0,
            },
            epoch: rec.last_epoch,
            replayed_batches: replayed,
        })
    }

    // --- verification -------------------------------------------------------

    /// Replay the log independently of the checkpoints and report every
    /// disagreement.
    ///
    /// This is the check that a snapshot-only resume would fail: `held` is
    /// rebuilt from the commands and compared against what each checkpoint
    /// recorded.
    ///
    /// Verification assumes a session whose log starts at `seq 0`, which is
    /// what [`Store::begin_session`] plus [`Store::append_commands`] produce. A
    /// store assembled some other way is reported as
    /// [`VerifyIssue::NonZeroStart`] rather than silently trusting a `held` set
    /// it cannot know.
    pub fn verify(&self, id: &SessionId) -> Result<VerifyReport, PersistError> {
        // Existence check: an unknown session is an error, an empty one is a
        // clean report with zero rows.
        self.load_session(id)?;
        let txn = self.db.begin_read()?;

        let mut issues = Vec::new();

        // Load the log.
        let mut batches: BTreeMap<u64, CommandBatch> = BTreeMap::new();
        {
            let commands = txn.open_table(COMMANDS)?;
            let range = commands.range((id.as_str(), 0u64)..=(id.as_str(), u64::MAX))?;
            for entry in range {
                let (k, v) = entry?;
                let seq = k.value().1;
                match serde_json::from_slice::<CommandBatch>(v.value()) {
                    Ok(b) => {
                        batches.insert(seq, b);
                    }
                    Err(e) => issues.push(VerifyIssue::UndecodableCommand {
                        seq,
                        detail: e.to_string(),
                    }),
                }
            }
        }

        // Load the checkpoints.
        let mut cps: BTreeMap<u64, CheckpointRecord> = BTreeMap::new();
        {
            let checkpoints = txn.open_table(CHECKPOINTS)?;
            let range = checkpoints.range((id.as_str(), 0u64)..=(id.as_str(), u64::MAX))?;
            for entry in range {
                let (k, v) = entry?;
                let seq = k.value().1;
                match serde_json::from_slice::<CheckpointRecord>(v.value()) {
                    Ok(c) => {
                        cps.insert(seq, c);
                    }
                    Err(e) => issues.push(VerifyIssue::UndecodableCheckpoint {
                        seq,
                        detail: e.to_string(),
                    }),
                }
            }
        }

        // Blob integrity: every digest a checkpoint names must exist and hash
        // to its key.
        {
            let blobs = txn.open_table(BLOBS)?;
            let mut seen: BTreeMap<String, u64> = BTreeMap::new();
            for (seq, cp) in &cps {
                seen.entry(cp.definition_digest.clone()).or_insert(*seq);
            }
            for (digest, seq) in seen {
                match blobs.get(digest.as_str())? {
                    None => issues.push(VerifyIssue::MissingDefinition { seq, digest }),
                    Some(g) => {
                        let computed = digest_hex(g.value());
                        if computed != digest {
                            issues.push(VerifyIssue::DefinitionDigestMismatch { digest, computed });
                        }
                    }
                }
            }
        }

        // Contiguity and epoch monotonicity, walked in `seq` order.
        let mut expected: Option<u64> = None;
        let mut last_epoch: Option<u64> = None;
        for (&seq, batch) in &batches {
            if let Some(exp) = expected {
                if seq != exp {
                    issues.push(VerifyIssue::MissingSeq {
                        expected: exp,
                        found: seq,
                    });
                }
            } else {
                if seq != 0 {
                    issues.push(VerifyIssue::NonZeroStart { first: seq });
                }
            }
            expected = Some(seq.saturating_add(1));
            if let Some(prev) = last_epoch
                && batch.epoch < prev
            {
                issues.push(VerifyIssue::EpochRegressed {
                    seq,
                    from: prev,
                    to: batch.epoch,
                });
            }
            last_epoch = Some(batch.epoch);
        }

        // The substantive check: rebuild `held` from the commands and compare
        // it with what each checkpoint said.
        let mut held = Buttons::default();
        let mut next_cp = cps.iter().peekable();
        for (&seq, batch) in &batches {
            let _ = fold(&batch.cmds, &mut held);
            while let Some(&(cp_seq, cp)) = next_cp.peek() {
                if *cp_seq > seq {
                    break;
                }
                if cp.held != held {
                    issues.push(VerifyIssue::HeldMismatch {
                        seq: *cp_seq,
                        recorded: cp.held,
                        replayed: held,
                    });
                }
                next_cp.next();
            }
        }
        // Checkpoints beyond the last batch (a checkpoint with no commands at
        // that seq) still have to agree with the final held set.
        for (&cp_seq, cp) in next_cp {
            if cp.held != held {
                issues.push(VerifyIssue::HeldMismatch {
                    seq: cp_seq,
                    recorded: cp.held,
                    replayed: held,
                });
            }
        }

        // A checkpoint's tick must not lag the last batch at or below its seq.
        for (&cp_seq, cp) in &cps {
            if let Some((_, batch)) = batches.range(..=cp_seq).next_back()
                && cp.tick < batch.tick
            {
                issues.push(VerifyIssue::TickBehindLog {
                    seq: cp_seq,
                    recorded: cp.tick,
                    logged: batch.tick,
                });
            }
        }

        Ok(VerifyReport {
            session: id.as_str().to_owned(),
            commands: batches.len(),
            checkpoints: cps.len(),
            issues,
        })
    }
}

/// Read a session inside an already-open table, for the read-modify-write paths.
fn read_session(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    id: &SessionId,
) -> Result<SessionRecord, PersistError> {
    match table.get(id.as_str())? {
        None => Err(PersistError::UnknownSession(id.clone())),
        Some(guard) => serde_json::from_slice(guard.value()).map_err(PersistError::Decode),
    }
}

/// Rebuild a `RuntimeSnapshot` from a checkpoint: fetch the definition blob and
/// splice it back into the body.
fn materialise(
    txn: &redb::ReadTransaction,
    record: CheckpointRecord,
) -> Result<(RuntimeSnapshot, bool, Buttons), PersistError> {
    let blobs = txn.open_table(BLOBS)?;
    let definition_bytes = match blobs.get(record.definition_digest.as_str())? {
        None => {
            return Err(PersistError::MissingDefinition(
                record.definition_digest.clone(),
            ));
        }
        Some(guard) => guard.value().to_vec(),
    };
    drop(blobs);

    let computed = digest_hex(&definition_bytes);
    if computed != record.definition_digest {
        return Err(PersistError::DigestMismatch {
            expected: record.definition_digest,
            found: computed,
        });
    }

    let definition: serde_json::Value =
        serde_json::from_slice(&definition_bytes).map_err(PersistError::Decode)?;
    let mut body = record.body;
    let obj = body.as_object_mut().ok_or(PersistError::NotAnObject)?;
    obj.insert("definition".to_owned(), definition);

    let snapshot: RuntimeSnapshot = serde_json::from_value(body).map_err(PersistError::Decode)?;
    Ok((snapshot, record.paused, record.held))
}

impl SessionId {
    /// A fresh id derived from the wall clock.
    ///
    /// Not a UUID: the store does not need one, and a time-ordered id sorts
    /// usefully in [`Store::list_sessions`].
    pub fn generate() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Self::new(format!("s-{}-{:09}", now.as_secs(), now.subsec_nanos()))
    }
}

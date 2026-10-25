//! The store itself: open, append, checkpoint, resume, verify.

use crate::digest::digest_hex;
use crate::error::PersistError;
use crate::record::{
    CheckpointRecord, CommandBatch, Resume, SessionId, SessionRecord, SimKind, VerifyIssue,
    VerifyReport,
};
use crate::schema::{self, BLOBS, CHECKPOINTS, COMMANDS, META, SESSIONS};
use crate::SCHEMA_VERSION;
use idaptik_core::scenario::{Buttons, Command, GhostLobbySim, RuntimeSnapshot, fold};
use redb::{Database, ReadableTable};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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
            let existing = match meta.get(schema::SCHEMA_KEY)? {
                None => None,
                Some(guard) => Some(guard.value().to_vec()),
            };
            match existing {
                None => {
                    let bytes = serde_json::to_vec(&SCHEMA_VERSION)?;
                    meta.insert(schema::SCHEMA_KEY, bytes.as_slice())?;
                }
                Some(bytes) => {
                    let found: u32 = serde_json::from_slice(&bytes).map_err(PersistError::Decode)?;
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
    pub fn list_sessions(&self) -> Result<Vec<(SessionId, Result<SessionRecord, String>)>, PersistError> {
        let txn = self.db.begin_read()?;
        let sessions = txn.open_table(SESSIONS)?;
        let mut out = Vec::new();
        for entry in sessions.iter()? {
            let (k, v) = entry?;
            let id = SessionId::new(k.value());
            out.push((id, serde_json::from_slice(v.value()).map_err(|e| e.to_string())));
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
            if let Some(last) = rec.last_seq {
                if seq <= last {
                    return Err(PersistError::SeqRegression {
                        last,
                        attempted: seq,
                    });
                }
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

    /// Store a snapshot as a checkpoint covering the first `at` commands.
    ///
    /// `at` is a **boundary**: `0` is the initial state (no commands applied),
    /// `n` means commands `0..n-1` are included. It is deliberately not the
    /// command `seq` space, and it does not advance the session's `last_seq` —
    /// see [`CheckpointRecord::at`] for why the two were separated.
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
        at: u64,
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
                at,
                epoch,
                tick: snapshot.tick,
                held,
                paused,
                definition_digest: digest,
                body,
            };
            let bytes = serde_json::to_vec(&record)?;
            let mut checkpoints = txn.open_table(CHECKPOINTS)?;
            checkpoints.insert((id.as_str(), at), bytes.as_slice())?;
            drop(checkpoints);

            if !rec.checkpoints.contains(&at) {
                rec.checkpoints.push(at);
                rec.checkpoints.sort_unstable();
            }
            // `last_seq` belongs to the command log and is advanced only by
            // `append_commands`. A checkpoint that touched it would make the
            // initial checkpoint consume `seq 0`.
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

    /// Restore the newest checkpoint and **replay everything after it through
    /// the simulation**.
    ///
    /// This is the whole point of the store: the returned
    /// [`Resume::snapshot`] is the state *after* every logged command, not the
    /// checkpoint's own state. A resume that returned the checkpoint verbatim
    /// while setting `next_seq` past the trailing batches would silently drop
    /// every command recorded since — a `Restart`, a `Pause`, an `Interact`,
    /// or a whole second of movement.
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
        let checkpoint_at = *rec
            .checkpoints
            .last()
            .ok_or_else(|| PersistError::NoCheckpoint(id.clone()))?;

        let txn = self.db.begin_read()?;
        let checkpoints = txn.open_table(CHECKPOINTS)?;
        let record = match checkpoints.get((id.as_str(), checkpoint_at))? {
            None => return Err(PersistError::NoCheckpoint(id.clone())),
            Some(guard) => serde_json::from_slice::<CheckpointRecord>(guard.value())
                .map_err(PersistError::Decode)?,
        };
        drop(checkpoints);

        let (base, base_paused, base_held) = materialise(&txn, record)?;

        // Rebuild the live simulation from the checkpoint, then advance it.
        // `restore` is the same entry point a cold load uses, so the replayed
        // run is the run — not an approximation of it.
        let mut sim = GhostLobbySim::restore(base.definition.clone(), base)
            .map_err(|e| PersistError::Restore(format!("{e:?}")))?;

        // `held` and `paused` are frontend state, absent from the snapshot.
        let mut held = base_held;
        let mut paused = base_paused;

        // Commands with `seq >= checkpoint_at` are the ones the checkpoint has
        // not applied. `at` is a count, so it doubles as the first unapplied
        // sequence number.
        let commands = txn.open_table(COMMANDS)?;
        let mut replayed = 0usize;
        let range = commands.range((id.as_str(), checkpoint_at)..=(id.as_str(), u64::MAX))?;
        for entry in range {
            let (_k, v) = entry?;
            let batch: CommandBatch =
                serde_json::from_slice(v.value()).map_err(PersistError::Decode)?;
            replay_batch(&mut sim, &mut held, &mut paused, &batch);
            replayed += 1;
        }
        drop(commands);

        Ok(Resume {
            snapshot: sim.snapshot(),
            held,
            paused,
            checkpoint_at,
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
    /// disagreement — including disagreement about **simulation state**.
    ///
    /// Two passes:
    ///
    /// 1. **Log-level** — sequence contiguity, epoch monotonicity, decodability,
    ///    blob integrity, and the held-button set each checkpoint recorded.
    /// 2. **State-level** — restore the earliest checkpoint into a
    ///    `GhostLobbySim`, tick the log between checkpoints, and compare the
    ///    resulting `RuntimeSnapshot` with each later checkpoint.
    ///
    /// Pass 2 is what makes the crate's claim honest. Checking only `held`
    /// would certify a checkpoint whose RNG, actor state or pause flag had
    /// diverged, and the next resume would then continue from corrupt state.
    ///
    /// Verification assumes the log starts at `seq 0`, which is what
    /// [`Store::begin_session`] plus [`Store::append_commands`] produce. A log
    /// that starts elsewhere is reported as [`VerifyIssue::NonZeroStart`] and
    /// the state pass begins from the earliest checkpoint instead.
    pub fn verify(&self, id: &SessionId) -> Result<VerifyReport, PersistError> {
        // Existence check: an unknown session is an error, an empty one is a
        // clean report with zero rows.
        let rec = self.load_session(id)?;
        let txn = self.db.begin_read()?;

        let mut issues = Vec::new();

        // --- pass 1: the log -------------------------------------------------

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

        let mut cps: BTreeMap<u64, CheckpointRecord> = BTreeMap::new();
        {
            let checkpoints = txn.open_table(CHECKPOINTS)?;
            let range = checkpoints.range((id.as_str(), 0u64)..=(id.as_str(), u64::MAX))?;
            for entry in range {
                let (k, v) = entry?;
                let at = k.value().1;
                match serde_json::from_slice::<CheckpointRecord>(v.value()) {
                    Ok(c) => {
                        cps.insert(at, c);
                    }
                    Err(e) => issues.push(VerifyIssue::UndecodableCheckpoint {
                        at,
                        detail: e.to_string(),
                    }),
                }
            }
        }

        {
            let blobs = txn.open_table(BLOBS)?;
            let mut seen: BTreeMap<String, u64> = BTreeMap::new();
            for (at, cp) in &cps {
                seen.entry(cp.definition_digest.clone()).or_insert(*at);
            }
            for (digest, at) in seen {
                match blobs.get(digest.as_str())? {
                    None => issues.push(VerifyIssue::MissingDefinition { at, digest }),
                    Some(g) => {
                        let computed = digest_hex(g.value());
                        if computed != digest {
                            issues.push(VerifyIssue::DefinitionDigestMismatch { digest, computed });
                        }
                    }
                }
            }
        }

        let mut expected: Option<u64> = None;
        let mut last_epoch: Option<u64> = None;
        for (&seq, batch) in &batches {
            match expected {
                Some(exp) if seq != exp => {
                    issues.push(VerifyIssue::MissingSeq { expected: exp, found: seq });
                }
                None if seq != 0 => issues.push(VerifyIssue::NonZeroStart { first: seq }),
                _ => {}
            }
            expected = Some(seq.saturating_add(1));
            if let Some(prev) = last_epoch {
                if batch.epoch < prev {
                    issues.push(VerifyIssue::EpochRegressed {
                        seq,
                        from: prev,
                        to: batch.epoch,
                    });
                }
            }
            last_epoch = Some(batch.epoch);
        }

        for (&at, cp) in &cps {
            if let Some((_, batch)) = batches.range(..at).next_back() {
                if cp.tick < batch.tick {
                    issues.push(VerifyIssue::TickBehindLog {
                        at,
                        recorded: cp.tick,
                        logged: batch.tick,
                    });
                }
            }
        }

        // --- pass 2: the simulation ------------------------------------------

        let mut replayed = 0usize;
        match rec.sim_kind {
            SimKind::GhostLobby => {
                if let Some((&first_at, first_cp)) = cps.iter().next() {
                    let (base, base_paused, base_held) =
                        materialise(&txn, first_cp.clone())?;
                    let mut sim = match GhostLobbySim::restore(base.definition.clone(), base) {
                        Ok(s) => s,
                        Err(e) => {
                            issues.push(VerifyIssue::StateUnverified {
                                reason: format!("checkpoint at {first_at} does not restore: {e:?}"),
                            });
                            return Ok(finish(id, batches.len(), cps.len(), 0, issues));
                        }
                    };
                    let mut held = base_held;
                    let mut paused = base_paused;
                    // The earliest checkpoint may be at > 0; the prefix cannot
                    // be checked, which `NonZeroStart` has already said.
                    let mut next_seq = first_at;

                    for (&at, cp) in cps.iter().filter(|(a, _)| **a > first_at) {
                        // Advance to this checkpoint's boundary.
                        for (&seq, batch) in batches.range(next_seq..at) {
                            replay_batch(&mut sim, &mut held, &mut paused, batch);
                            replayed += 1;
                            next_seq = seq.saturating_add(1);
                        }

                        if cp.definition_digest != first_cp.definition_digest {
                            issues.push(VerifyIssue::DefinitionMismatch {
                                at,
                                expected: first_cp.definition_digest.clone(),
                                found: cp.definition_digest.clone(),
                            });
                            continue;
                        }

                        if cp.held != held {
                            issues.push(VerifyIssue::HeldMismatch {
                                at,
                                recorded: cp.held,
                                replayed: held,
                            });
                        }

                        // Compare the *whole* snapshot, not a selected few
                        // fields: the point of this pass is to catch drift the
                        // caller did not think to enumerate. The per-field walk
                        // below only names which part differed first.
                        let actual = sim.snapshot();
                        match materialise(&txn, cp.clone()) {
                            Err(e) => issues.push(VerifyIssue::StateUnverified {
                                reason: format!("checkpoint at {at} does not load: {e}"),
                            }),
                            Ok((recorded, _, _)) => {
                                if actual != recorded {
                                    let (field, a, r) = first_difference(&actual, &recorded);
                                    issues.push(VerifyIssue::StateMismatch {
                                        at,
                                        field,
                                        replayed: a,
                                        recorded: r,
                                    });
                                }
                            }
                        }
                    }

                    // Trailing batches after the last checkpoint still have to
                    // run, so that a held-set disagreement at the last
                    // checkpoint is not the only thing checked.
                    for batch in batches.range(next_seq..).map(|(_, b)| b) {
                        replay_batch(&mut sim, &mut held, &mut paused, batch);
                        replayed += 1;
                    }
                } else if !batches.is_empty() {
                    issues.push(VerifyIssue::StateUnverified {
                        reason: "no checkpoint to replay from".to_owned(),
                    });
                }
            }
            other => issues.push(VerifyIssue::StateUnverified {
                reason: format!("{other:?} has no typed replay in this build"),
            }),
        }

        Ok(finish(id, batches.len(), cps.len(), replayed, issues))
    }
}

/// Tick one recorded batch through the simulation, exactly as `step_sim` does.
fn replay_batch(
    sim: &mut GhostLobbySim,
    held: &mut Buttons,
    paused: &mut bool,
    batch: &CommandBatch,
) {
    let input = fold(&batch.cmds, held);
    // The frontend latches `paused` from the same command it sends to the sim;
    // mirroring that here keeps the returned latch in step with the tick.
    for cmd in &batch.cmds {
        if let Command::Pause { on } = cmd {
            *paused = *on;
        }
    }
    let _events = sim.tick(&input);
    // Discard: events are derived from the command stream and are not part of
    // `Resume`. Draining keeps the sim's own buffer from growing across a long
    // replay.
    let _ = sim.drain_events();
}

fn finish(
    id: &SessionId,
    commands: usize,
    checkpoints: usize,
    replayed: usize,
    issues: Vec<VerifyIssue>,
) -> VerifyReport {
    VerifyReport {
        session: id.as_str().to_owned(),
        commands,
        checkpoints,
        replayed,
        issues,
    }
}

/// Name the first field that differs between two snapshots, with both values.
///
/// The comparison itself is [`RuntimeSnapshot`]'s derived `PartialEq`; this
/// only exists so a report says *what* diverged rather than "not equal".
/// Reporting only the first difference is deliberate: a run that diverged at
/// the RNG will differ in everything downstream, and a hundred downstream
/// entries would bury the cause.
fn first_difference(
    actual: &RuntimeSnapshot,
    recorded: &RuntimeSnapshot,
) -> (String, String, String) {
    let field = |name: &str, a: String, r: String| (name.to_owned(), a, r);
    if actual.format != recorded.format {
        return field("format", actual.format.clone(), recorded.format.clone());
    }
    if actual.seed != recorded.seed {
        return field("seed", actual.seed.to_string(), recorded.seed.to_string());
    }
    if actual.tick != recorded.tick {
        return field("tick", actual.tick.to_string(), recorded.tick.to_string());
    }
    if actual.paused != recorded.paused {
        return field(
            "paused",
            actual.paused.to_string(),
            recorded.paused.to_string(),
        );
    }
    if actual.rng != recorded.rng {
        return field("rng", format!("{:?}", actual.rng), "differs".to_owned());
    }
    if actual.cfg != recorded.cfg {
        return field("cfg", format!("{:?}", actual.cfg), "differs".to_owned());
    }
    if actual.state != recorded.state {
        return field("state", "differs".to_owned(), "as recorded".to_owned());
    }
    if actual.validation != recorded.validation {
        return field("validation", "differs".to_owned(), "as recorded".to_owned());
    }
    // Equal snapshots never reach here; if they do, say so rather than lying.
    field("unknown", "differs".to_owned(), "differs".to_owned())
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

    let snapshot: RuntimeSnapshot =
        serde_json::from_value(body).map_err(PersistError::Decode)?;
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

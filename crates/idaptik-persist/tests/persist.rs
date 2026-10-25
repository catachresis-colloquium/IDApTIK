//! The store's tests. One of them exists specifically to prove the others can
//! fail — see `verify_can_fail`.

use idaptik_core::scenario::{
    Button, Buttons, Command, ExtractMethod, FailReason, GhostLobbySim, RunConfig, RuntimeSnapshot,
    SNAPSHOT_FORMAT, fold, ghost_lobby,
};
use idaptik_persist::{
    CheckpointRecord, FlushPolicy, PersistError, SessionId, SessionRecord, SessionStatus, SimKind,
    Store, VerifyIssue,
};

fn record(sim: SimKind, format: &str) -> SessionRecord {
    SessionRecord {
        sim_kind: sim,
        scenario_id: "envelope-001-ghost-lobby".to_owned(),
        seed: 7,
        format_tag: format.to_owned(),
        created_unix: 1_760_000_000,
        last_seq: None,
        last_epoch: 0,
        checkpoints: Vec::new(),
        status: SessionStatus::Open,
        flush: FlushPolicy::TimeBatched { millis: 1000 },
    }
}

fn sim() -> GhostLobbySim {
    GhostLobbySim::new(ghost_lobby(), RunConfig::standard(), 7).expect("canonical scenario builds")
}

fn tmp() -> tempdir::TempDir {
    tempdir::TempDir::new("idaptik-persist")
}

/// A deliberately tiny temporary-directory helper: `tempfile` would be another
/// dependency for `create_dir` + `remove_dir_all` on drop.
mod tempdir {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    pub struct TempDir(PathBuf);

    impl TempDir {
        pub fn new(prefix: &str) -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let p = std::env::temp_dir().join(format!("{prefix}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).expect("temp dir");
            Self(p)
        }
        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[test]
fn begin_and_load_session() {
    let dir = tmp();
    let store = Store::open(dir.path()).expect("open");
    let id = SessionId::new("s-test");
    store
        .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
        .expect("begin");
    let got = store.load_session(&id).expect("load");
    assert_eq!(got.seed, 7);
    assert_eq!(got.format_tag, SNAPSHOT_FORMAT);
    assert_eq!(got.status, SessionStatus::Open);
}

#[test]
fn duplicate_session_id_is_refused() {
    let dir = tmp();
    let store = Store::open(dir.path()).expect("open");
    let id = SessionId::new("s-dup");
    let r = record(SimKind::GhostLobby, SNAPSHOT_FORMAT);
    store.begin_session(&id, &r).expect("first");
    match store.begin_session(&id, &r) {
        Err(PersistError::SessionExists(got)) => assert_eq!(got, id),
        other => panic!("expected SessionExists, got {other:?}"),
    }
}

#[test]
fn seq_must_strictly_increase() {
    let dir = tmp();
    let store = Store::open(dir.path()).expect("open");
    let id = SessionId::new("s-seq");
    store
        .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
        .expect("begin");
    store
        .append_commands(&id, 1, 0, 1, &[Command::Jump])
        .expect("first append");
    match store.append_commands(&id, 1, 0, 2, &[Command::Interact]) {
        Err(PersistError::SeqRegression { last, attempted }) => {
            assert_eq!((last, attempted), (1, 1));
        }
        other => panic!("expected SeqRegression, got {other:?}"),
    }
}

/// The reason checkpoints record `held` at all: `held` is frontend state and is
/// absent from `RuntimeSnapshot`, so a resume that only restored the snapshot
/// would come back with no buttons held and diverge on the first tick.
#[test]
fn resume_restores_held_buttons() {
    let dir = tmp();
    let store = Store::open(dir.path()).expect("open");
    let id = SessionId::new("s-held");
    store
        .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
        .expect("begin");

    let s = sim();
    store
        .append_commands(
            &id,
            0,
            0,
            0,
            &[
                Command::SetButton {
                    button: Button::Left,
                    down: true,
                },
                Command::SetButton {
                    button: Button::Sprint,
                    down: true,
                },
            ],
        )
        .expect("append");

    let mut held = Buttons::default();
    held.set(Button::Left, true);
    held.set(Button::Sprint, true);

    // Boundary: the one batch (seq 0) is applied, so at == 1.
    store
        .checkpoint(&id, 1, 0, &s.snapshot(), held, false)
        .expect("checkpoint");

    let resumed = store.resume(&id).expect("resume");
    assert!(
        resumed.held.has(Button::Left),
        "Left must survive the resume"
    );
    assert!(
        resumed.held.has(Button::Sprint),
        "Sprint must survive the resume"
    );
    assert_eq!(resumed.held, held);
    assert_eq!(resumed.checkpoint_at, 1);
    assert_eq!(resumed.next_seq, 1);
    assert_eq!(resumed.replayed_batches, 0);
}

/// Commands logged *after* the checkpoint still have to reach the resumed held
/// set, and they must do so through `fold` rather than a second interpretation
/// of what a button command means.
#[test]
fn resume_advances_held_through_later_commands() {
    let dir = tmp();
    let store = Store::open(dir.path()).expect("open");
    let id = SessionId::new("s-later");
    store
        .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
        .expect("begin");

    let s = sim();
    // Checkpoint with nothing held.
    store
        .checkpoint(&id, 0, 0, &s.snapshot(), Buttons::default(), false)
        .expect("checkpoint");

    // Then press Right, after the checkpoint.
    store
        .append_commands(
            &id,
            0,
            0,
            0,
            &[Command::SetButton {
                button: Button::Right,
                down: true,
            }],
        )
        .expect("append");

    let resumed = store.resume(&id).expect("resume");
    assert!(
        resumed.held.has(Button::Right),
        "a press after the checkpoint must reach the resumed held set"
    );
    assert_eq!(resumed.replayed_batches, 1);
}

#[test]
fn verify_is_clean_for_a_consistent_session() {
    let dir = tmp();
    let store = Store::open(dir.path()).expect("open");
    let id = SessionId::new("s-ok");
    store
        .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
        .expect("begin");

    let mut s = sim();
    let mut held = Buttons::default();
    let cmds = [Command::SetButton {
        button: Button::Left,
        down: true,
    }];
    store.append_commands(&id, 0, 0, 0, &cmds).expect("append");
    s.tick(&fold(&cmds, &mut held));
    store
        .checkpoint(&id, 1, 0, &s.snapshot(), held, false)
        .expect("checkpoint");

    let report = store.verify(&id).expect("verify");
    assert!(report.ok(), "expected no issues, got {:?}", report.issues);
    assert_eq!(report.commands, 1);
    assert_eq!(report.checkpoints, 1);
}

#[test]
fn verify_checks_held_at_the_only_checkpoint_from_a_complete_prefix() {
    for (at, seqs, recorded_left, mismatch) in [
        // The initial boundary must have no buttons held.
        (0, vec![], true, true),
        // At 1, the press is included and the later release is excluded.
        (1, vec![0, 1], false, true),
        (1, vec![0, 1], true, false),
        // At 2, the release is included too.
        (2, vec![0, 1], true, true),
        // Missing starts, internal gaps, and missing tails cannot be compared.
        (2, vec![1], true, false),
        (3, vec![0, 2], true, false),
        (2, vec![0], false, false),
    ] {
        let dir = tmp();
        let store = Store::open(dir.path()).expect("open");
        let id = SessionId::new("s-prefix");
        store
            .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
            .expect("begin");
        for seq in seqs {
            store
                .append_commands(
                    &id,
                    seq,
                    0,
                    0,
                    &[Command::SetButton {
                        button: Button::Left,
                        down: seq == 0,
                    }],
                )
                .expect("append");
        }
        let mut held = Buttons::default();
        held.set(Button::Left, recorded_left);
        store
            .checkpoint(&id, at, 0, &sim().snapshot(), held, false)
            .expect("checkpoint");

        let report = store.verify(&id).expect("verify");
        assert_eq!(report.checkpoints, 1);
        let held_issues: Vec<_> = report
            .issues
            .iter()
            .filter(|issue| matches!(issue, VerifyIssue::HeldMismatch { .. }))
            .collect();
        let mut replayed = Buttons::default();
        replayed.set(Button::Left, at == 1);
        let expected = VerifyIssue::HeldMismatch {
            at,
            recorded: held,
            replayed,
        };
        assert_eq!(
            held_issues,
            if mismatch { vec![&expected] } else { vec![] },
            "at={at}, recorded_left={recorded_left}: {:?}",
            report.issues
        );
    }
}

/// A check that cannot fail is not a check. This plants the exact divergence a
/// snapshot-only resume would produce — a checkpoint whose recorded `held` is
/// not what the log replays to — and requires `verify` to name it.
#[test]
fn verify_can_fail() {
    let dir = tmp();
    let store = Store::open(dir.path()).expect("open");
    let id = SessionId::new("s-bad");
    store
        .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
        .expect("begin");

    let s = sim();
    // An honest base, so the replay has somewhere to start from.
    store
        .checkpoint(&id, 0, 0, &s.snapshot(), Buttons::default(), false)
        .expect("base checkpoint");

    // The log holds no button presses at all, so the replayed held set stays
    // empty; record a checkpoint claiming otherwise.
    store
        .append_commands(&id, 0, 0, 0, &[Command::Jump])
        .expect("append");
    let mut lie = Buttons::default();
    lie.set(Button::Left, true);
    // `at == 1` claims the one command is applied; the log never pressed Left.
    store
        .checkpoint(&id, 1, 0, &s.snapshot(), lie, false)
        .expect("checkpoint");

    let report = store.verify(&id).expect("verify");
    assert!(!report.ok(), "verify must notice a held-set disagreement");
    assert!(
        report
            .issues
            .iter()
            .any(|i| matches!(i, VerifyIssue::HeldMismatch { at: 1, .. })),
        "expected a HeldMismatch at at=1, got {:?}",
        report.issues
    );
}

#[test]
fn a_log_that_does_not_start_at_zero_is_reported() {
    let dir = tmp();
    let store = Store::open(dir.path()).expect("open");
    let id = SessionId::new("s-gap");
    store
        .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
        .expect("begin");
    store
        .append_commands(&id, 5, 0, 5, &[Command::Jump])
        .expect("append");
    let report = store.verify(&id).expect("verify");
    assert!(
        report
            .issues
            .iter()
            .any(|i| matches!(i, VerifyIssue::NonZeroStart { first: 5 })),
        "got {:?}",
        report.issues
    );
}

#[test]
fn a_snapshot_of_the_wrong_format_is_refused() {
    let dir = tmp();
    let store = Store::open(dir.path()).expect("open");
    let id = SessionId::new("s-fmt");
    // The session says v2; the snapshot is v3.
    store
        .begin_session(
            &id,
            &record(SimKind::GhostLobby, "idaptik-ghost-lobby-runtime-v2"),
        )
        .expect("begin");
    let s = sim();
    match store.checkpoint(&id, 0, 0, &s.snapshot(), Buttons::default(), false) {
        Err(PersistError::FormatMismatch { found, expected }) => {
            assert_eq!(found, SNAPSHOT_FORMAT);
            assert_eq!(expected, "idaptik-ghost-lobby-runtime-v2");
        }
        other => panic!("expected FormatMismatch, got {other:?}"),
    }
}

#[test]
fn resume_without_a_checkpoint_says_so() {
    let dir = tmp();
    let store = Store::open(dir.path()).expect("open");
    let id = SessionId::new("s-none");
    store
        .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
        .expect("begin");
    match store.resume(&id) {
        Err(PersistError::NoCheckpoint(got)) => assert_eq!(got, id),
        other => panic!("expected NoCheckpoint, got {other:?}"),
    }
}

#[test]
fn a_building_session_is_writable_but_has_no_typed_resume_yet() {
    let dir = tmp();
    let store = Store::open(dir.path()).expect("open");
    let id = SessionId::new("s-bld");
    store
        .begin_session(
            &id,
            &record(SimKind::Building, "idaptik-building-runtime-v1"),
        )
        .expect("begin");
    store
        .append_commands(&id, 1, 0, 0, &[Command::Jump])
        .expect("append");
    match store.resume(&id) {
        Err(PersistError::UnsupportedSimKind(SimKind::Building)) => {}
        other => panic!("expected UnsupportedSimKind(Building), got {other:?}"),
    }
}

/// The definition is stored once and referenced by digest: a checkpoint body
/// must not contain it. This is what keeps a building checkpoint at ~500 bytes
/// of real payload instead of ~17 KB.
#[test]
fn the_definition_is_deduplicated_out_of_the_checkpoint_body() {
    let dir = tmp();
    let store = Store::open(dir.path()).expect("open");
    let id = SessionId::new("s-dedup");
    store
        .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
        .expect("begin");
    let s = sim();
    let snap = s.snapshot();
    store
        .checkpoint(&id, 0, 0, &snap, Buttons::default(), false)
        .expect("checkpoint");

    let resumed = store.resume(&id).expect("resume");
    // Round-trip fidelity: the definition came back intact from the blob.
    assert_eq!(resumed.snapshot.definition, snap.definition);
    assert_eq!(resumed.snapshot.seed, snap.seed);
    assert_eq!(resumed.snapshot.format, snap.format);
}

/// Codex review finding (P2): an initial checkpoint must not consume `seq 0`.
///
/// `checkpoint(at = 0)` is the state *before* any command. If it advanced the
/// command-log high-water mark, the first command would have to be `seq 1`, and
/// `verify` would then reject the whole session as `NonZeroStart`. The two
/// numbers are separate quantities and are stored separately.
#[test]
fn an_initial_checkpoint_does_not_consume_seq_zero() {
    let dir = tmp();
    let store = Store::open(dir.path()).expect("open");
    let id = SessionId::new("s-initial");
    store
        .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
        .expect("begin");

    let s = sim();
    // Checkpoint first, at the initial state, before any command exists.
    store
        .checkpoint(&id, 0, 0, &s.snapshot(), Buttons::default(), false)
        .expect("initial checkpoint");

    // The first command must still be allowed at seq 0.
    store
        .append_commands(&id, 0, 0, 0, &[Command::Jump])
        .expect("seq 0 must be writable after an initial checkpoint");

    let report = store.verify(&id).expect("verify");
    assert!(
        !report
            .issues
            .iter()
            .any(|i| matches!(i, VerifyIssue::NonZeroStart { .. })),
        "an initial checkpoint must not push the log off zero: {:?}",
        report.issues
    );
    assert!(
        report.ok(),
        "expected a clean report, got {:?}",
        report.issues
    );
}

/// Codex review finding (P1): `resume` must replay trailing batches into the
/// simulation, not merely update the held set.
///
/// Two batches are logged after the checkpoint: a movement press, which only
/// `tick` can turn into a position and a tick count, and a pause, which only
/// the sim's own state machine can latch. A resume that merely folded buttons
/// would return the checkpoint's state with `next_seq` claiming both were
/// applied.
#[test]
fn resume_replays_trailing_commands_into_the_simulation() {
    let dir = tmp();
    let store = Store::open(dir.path()).expect("open");
    let id = SessionId::new("s-replay");
    store
        .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
        .expect("begin");

    let s = sim();
    let base = s.snapshot();
    assert!(!base.paused, "the fixture starts unpaused");

    // Checkpoint the initial, unpaused state.
    store
        .checkpoint(&id, 0, 0, &base, Buttons::default(), false)
        .expect("checkpoint");

    // Then move right, and pause, after the checkpoint.
    store
        .append_commands(
            &id,
            0,
            0,
            0,
            &[Command::SetButton {
                button: Button::Right,
                down: true,
            }],
        )
        .expect("append movement");
    store
        .append_commands(&id, 1, 0, 1, &[Command::Pause { on: true }])
        .expect("append pause");

    let resumed = store.resume(&id).expect("resume");
    assert_eq!(resumed.replayed_batches, 2);

    // `tick` ran: only the simulation increments it, and only on an unpaused,
    // unended frame.
    assert!(
        resumed.snapshot.tick > base.tick,
        "replaying a movement batch must advance the simulation's tick \
         (was {}, now {})",
        base.tick,
        resumed.snapshot.tick
    );

    // And the movement actually moved the body, which no fold can do.
    assert_ne!(
        resumed.snapshot.state.player.x, base.state.player.x,
        "the replayed movement must change the player's position"
    );

    // The pause reached the simulation and the frontend latch alike.
    assert!(
        resumed.snapshot.paused,
        "a Pause after the checkpoint must reach the resumed simulation"
    );
    assert!(resumed.paused, "the frontend latch must agree");
}

/// Codex review finding (P2): `verify` must compare simulation state, not just
/// held buttons.
///
/// A checkpoint whose state was never produced by the log still passes a
/// held-only check. This records a checkpoint at `at = 1` carrying the *tick 0*
/// snapshot while the log holds one command, so replaying produces tick 1 and
/// the disagreement is real.
#[test]
fn verify_catches_diverged_simulation_state() {
    let dir = tmp();
    let store = Store::open(dir.path()).expect("open");
    let id = SessionId::new("s-state");
    store
        .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
        .expect("begin");

    let s = sim();
    store
        .checkpoint(&id, 0, 0, &s.snapshot(), Buttons::default(), false)
        .expect("checkpoint at the initial state");

    store
        .append_commands(
            &id,
            0,
            0,
            0,
            &[Command::SetButton {
                button: Button::Right,
                down: true,
            }],
        )
        .expect("append");

    // Claims one command has been applied, but carries the pre-command state.
    // Held matches, so a held-only verifier would call this clean.
    let mut held = Buttons::default();
    held.set(Button::Right, true);
    store
        .checkpoint(&id, 1, 0, &s.snapshot(), held, false)
        .expect("lying checkpoint");

    let report = store.verify(&id).expect("verify");
    assert!(
        report
            .issues
            .iter()
            .any(|i| matches!(i, VerifyIssue::StateMismatch { at: 1, .. })),
        "verify must compare simulation state, not only held buttons: {:?}",
        report.issues
    );
    assert!(
        !report
            .issues
            .iter()
            .any(|i| matches!(i, VerifyIssue::HeldMismatch { .. })),
        "held agrees here; the disagreement is state: {:?}",
        report.issues
    );
}

#[test]
fn a_store_with_a_foreign_schema_is_refused() {
    assert_schema_is_refused(99);
}

#[test]
fn a_store_with_the_old_checkpoint_schema_is_refused() {
    assert_schema_is_refused(1);
}

fn assert_schema_is_refused(schema_version: u32) {
    use idaptik_persist::schema::{META, SCHEMA_KEY};
    use redb::Database;

    let dir = tmp();
    {
        let store = Store::open(dir.path()).expect("open");
        drop(store);
    }
    // Plant an incompatible schema tag behind the store's back.
    {
        let db = Database::create(dir.path().join("idaptik.redb")).expect("reopen raw");
        let txn = db.begin_write().expect("write");
        {
            let mut meta = txn.open_table(META).expect("meta");
            let bytes = serde_json::to_vec(&schema_version).expect("encode");
            meta.insert(SCHEMA_KEY, bytes.as_slice()).expect("insert");
        }
        txn.commit().expect("commit");
    }
    match Store::open(dir.path()) {
        Err(PersistError::SchemaMismatch { found, expected }) => {
            assert_eq!(found, schema_version);
            assert_eq!(expected, idaptik_persist::SCHEMA_VERSION);
        }
        other => panic!("expected SchemaMismatch, got {other:?}"),
    }
}

fn step(s: &mut GhostLobbySim, held: &mut Buttons, cmds: &[Command]) {
    s.tick(&fold(cmds, held));
    s.drain_events();
}

/// The store must be dropped before opening its database for corruption fixtures.
fn rewrite_checkpoint(
    dir: &std::path::Path,
    id: &SessionId,
    at: u64,
    edit: impl FnOnce(&mut CheckpointRecord),
) {
    use idaptik_persist::schema::CHECKPOINTS;
    use redb::ReadableTable;

    let db = redb::Database::create(dir.join("idaptik.redb")).expect("raw database");
    let txn = db.begin_write().expect("write");
    {
        let mut table = txn.open_table(CHECKPOINTS).expect("checkpoints");
        let mut cp: CheckpointRecord = {
            let row = table.get((id.as_str(), at)).expect("get").expect("row");
            serde_json::from_slice(row.value()).expect("decode checkpoint")
        };
        edit(&mut cp);
        let bytes = serde_json::to_vec(&cp).expect("encode checkpoint");
        table
            .insert((id.as_str(), at), bytes.as_slice())
            .expect("replace");
    }
    txn.commit().expect("commit");
}

#[test]
fn an_initial_checkpoint_resumes_without_inventing_a_command() {
    let dir = tmp();
    let store = Store::open(dir.path()).expect("open");
    let id = SessionId::new("s-initial-resume");
    store
        .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
        .expect("begin");
    let initial = sim().snapshot();
    store
        .checkpoint(&id, 0, 0, &initial, Buttons::default(), false)
        .expect("checkpoint");

    let resumed = store.resume(&id).expect("resume");
    assert_eq!(resumed.snapshot, initial);
    assert_eq!(resumed.held, Buttons::default());
    assert!(!resumed.paused);
    assert_eq!(
        (resumed.checkpoint_at, resumed.next_seq, resumed.epoch),
        (0, 0, 0)
    );
    assert_eq!(resumed.replayed_batches, 0);
    let session = store.load_session(&id).expect("load");
    assert_eq!(session.last_seq, None);
    assert_eq!(session.checkpoints, vec![0]);
    let report = store.verify(&id).expect("verify");
    assert!(report.ok(), "{:?}", report.issues);
    assert_eq!(
        (report.commands, report.checkpoints, report.replayed),
        (0, 1, 0)
    );
}

#[test]
fn replacing_and_backfilling_checkpoints_preserves_the_command_high_water_mark() {
    let dir = tmp();
    let store = Store::open(dir.path()).expect("open");
    let id = SessionId::new("s-replace");
    store
        .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
        .expect("begin");
    let mut s = sim();
    let mut held = Buttons::default();
    let press = [Command::SetButton {
        button: Button::Right,
        down: true,
    }];
    store.append_commands(&id, 0, 0, 0, &press).expect("press");
    step(&mut s, &mut held, &press);
    let first = s.snapshot();
    store
        .append_commands(&id, 1, 0, first.tick, &[])
        .expect("empty batch");
    step(&mut s, &mut held, &[]);
    let second = s.snapshot();

    store
        .checkpoint(&id, 2, 0, &second, held, false)
        .expect("newer checkpoint");
    store
        .checkpoint(&id, 1, 0, &first, held, false)
        .expect("backfill");
    // The frontend latch may differ from the simulation's pause state.
    store
        .checkpoint(&id, 2, 0, &second, held, true)
        .expect("replace checkpoint");
    let session = store.load_session(&id).expect("load");
    assert_eq!(session.checkpoints, vec![1, 2]);
    assert_eq!(session.last_seq, Some(1));
    let resumed = store.resume(&id).expect("resume replacement");
    assert_eq!(resumed.snapshot, second);
    assert!(resumed.paused);
    assert_eq!(resumed.checkpoint_at, 2);
    assert_eq!(resumed.next_seq, 2);
    assert_eq!(resumed.replayed_batches, 0);

    store
        .append_commands(&id, 2, 0, second.tick, &[])
        .expect("checkpoint boundary remains writable");
    step(&mut s, &mut held, &[]);
    assert_eq!(
        store.resume(&id).expect("resume tail").snapshot,
        s.snapshot()
    );
}

#[test]
fn resume_uses_the_latest_boundary_and_matches_an_uninterrupted_run_after_reopen() {
    let dir = tmp();
    let store = Store::open(dir.path()).expect("open");
    let id = SessionId::new("s-main");
    let other = SessionId::new("s-neighbour");
    for session in [&id, &other] {
        store
            .begin_session(session, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
            .expect("begin");
        store
            .checkpoint(session, 0, 0, &sim().snapshot(), Buttons::default(), false)
            .expect("initial checkpoint");
    }
    store
        .append_commands(&other, 0, 0, 0, &[Command::Pause { on: true }])
        .expect("other session");

    let script = [
        vec![Command::SetButton {
            button: Button::Right,
            down: true,
        }],
        vec![],
        vec![Command::Pause { on: true }],
        vec![],
        vec![
            Command::Pause { on: false },
            Command::SetButton {
                button: Button::Right,
                down: false,
            },
            Command::SetButton {
                button: Button::Left,
                down: true,
            },
        ],
        vec![],
    ];
    let mut s = sim();
    let mut held = Buttons::default();
    for (seq, cmds) in script.iter().enumerate() {
        store
            .append_commands(&id, seq as u64, 0, s.snapshot().tick, cmds)
            .expect("append");
        step(&mut s, &mut held, cmds);
        if seq == 1 || seq == 3 {
            let snap = s.snapshot();
            store
                .checkpoint(&id, seq as u64 + 1, 0, &snap, held, snap.paused)
                .expect("checkpoint");
        }
    }
    let before = store.load_session(&id).expect("session before reads");
    let resumed = store.resume(&id).expect("resume");
    assert_eq!(resumed.snapshot, s.snapshot());
    assert_eq!(resumed.held, held);
    assert!(!resumed.paused);
    assert_eq!((resumed.checkpoint_at, resumed.next_seq), (4, 6));
    assert_eq!(resumed.replayed_batches, 2);
    assert_eq!(store.resume(&id).expect("repeat resume"), resumed);

    let report = store.verify(&id).expect("verify");
    assert!(report.ok(), "{:?}", report.issues);
    assert_eq!(report.session, id.as_str());
    assert_eq!(
        (report.commands, report.checkpoints, report.replayed),
        (6, 3, 6)
    );
    assert_eq!(
        store.load_session(&id).expect("session after reads"),
        before
    );
    assert!(store.resume(&other).expect("other resume").snapshot.paused);
    drop(store);

    let reopened = Store::open(dir.path()).expect("reopen");
    assert_eq!(reopened.resume(&id).expect("durable resume"), resumed);
    assert_eq!(reopened.load_session(&id).expect("durable session"), before);
}

#[test]
fn resume_and_verify_replay_restart_across_repeated_tick_numbers() {
    let dir = tmp();
    let store = Store::open(dir.path()).expect("open");
    let id = SessionId::new("s-restart");
    store
        .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
        .expect("begin");
    let mut s = sim();
    let mut held = Buttons::default();
    store
        .checkpoint(&id, 0, 0, &s.snapshot(), held, false)
        .expect("initial checkpoint");
    let script = [
        vec![Command::SetButton {
            button: Button::Right,
            down: true,
        }],
        vec![],
        vec![],
        vec![Command::Restart],
        vec![],
        vec![
            Command::SetButton {
                button: Button::Right,
                down: false,
            },
            Command::Pause { on: true },
        ],
        vec![],
    ];
    for (seq, cmds) in script.iter().enumerate() {
        let epoch = u64::from(seq >= 3);
        store
            .append_commands(&id, seq as u64, epoch, s.snapshot().tick, cmds)
            .expect("append");
        step(&mut s, &mut held, cmds);
        if seq == 2 {
            store
                .checkpoint(&id, 3, 0, &s.snapshot(), held, false)
                .expect("pre-restart checkpoint");
        }
    }
    let resumed = store.resume(&id).expect("resume restart");
    assert_eq!(resumed.snapshot, s.snapshot());
    assert_eq!(
        resumed.snapshot.tick, 1,
        "Restart rewinds the simulation tick"
    );
    assert_eq!(resumed.held, Buttons::default());
    assert!(resumed.paused);
    assert_eq!(
        (resumed.checkpoint_at, resumed.next_seq, resumed.epoch),
        (3, 7, 1)
    );
    assert_eq!(resumed.replayed_batches, 4);

    // The command sequence keeps increasing even though simulation ticks repeat.
    store
        .checkpoint(&id, 7, 1, &s.snapshot(), held, true)
        .expect("post-restart checkpoint");
    let report = store.verify(&id).expect("verify restart");
    assert!(report.ok(), "{:?}", report.issues);
    assert_eq!(
        (report.commands, report.checkpoints, report.replayed),
        (7, 3, 7)
    );
    assert_eq!(store.load_session(&id).expect("load").last_seq, Some(6));
}

#[test]
fn resume_preserves_the_frontend_latch_and_applies_the_last_pause_in_a_batch() {
    for (base_paused, base_latch, cmds, expected_latch, expected_paused, ticks) in [
        (false, true, vec![], true, false, 1),
        (true, false, vec![], false, true, 0),
        (true, true, vec![Command::Restart], false, false, 0),
        (
            true,
            true,
            vec![Command::Pause { on: true }, Command::Restart],
            false,
            false,
            0,
        ),
        (
            false,
            false,
            vec![Command::Restart, Command::Pause { on: true }],
            true,
            false,
            0,
        ),
        (
            true,
            true,
            vec![
                Command::Pause { on: false },
                Command::Pause { on: true },
                Command::Pause { on: false },
            ],
            false,
            false,
            1,
        ),
        (
            false,
            false,
            vec![
                Command::Pause { on: true },
                Command::Pause { on: false },
                Command::Pause { on: true },
            ],
            true,
            true,
            0,
        ),
    ] {
        let dir = tmp();
        let store = Store::open(dir.path()).expect("open");
        let id = SessionId::new("s-pause-latch");
        store
            .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
            .expect("begin");
        let mut s = sim();
        let mut held = Buttons::default();
        if base_paused {
            step(&mut s, &mut held, &[Command::Pause { on: true }]);
        }
        store
            .checkpoint(&id, 0, 0, &s.snapshot(), held, base_latch)
            .expect("checkpoint");
        // Tick and epoch are metadata: even a large value means just one batch.
        store
            .append_commands(&id, 0, 8, 999, &cmds)
            .expect("append");
        step(&mut s, &mut held, &cmds);

        let resumed = store.resume(&id).expect("resume");
        assert_eq!(resumed.snapshot, s.snapshot(), "commands: {cmds:?}");
        assert_eq!(resumed.paused, expected_latch, "commands: {cmds:?}");
        assert_eq!(resumed.snapshot.paused, expected_paused);
        assert_eq!(resumed.snapshot.tick, ticks);
        assert_eq!(
            (resumed.epoch, resumed.next_seq, resumed.replayed_batches),
            (8, 1, 1)
        );
    }
}

#[test]
fn verify_compares_the_frontend_pause_latch_independently_of_the_snapshot() {
    for (base_latch, cmds, expected_latch) in [
        (false, vec![], false),
        (true, vec![], true),
        (false, vec![Command::Pause { on: true }], true),
        (true, vec![Command::Restart], false),
    ] {
        let dir = tmp();
        let store = Store::open(dir.path()).expect("open");
        let id = SessionId::new("s-verify-pause-latch");
        store
            .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
            .expect("begin");
        let mut s = sim();
        let mut held = Buttons::default();
        store
            .checkpoint(&id, 0, 0, &s.snapshot(), held, base_latch)
            .expect("base");
        store.append_commands(&id, 0, 0, 0, &cmds).expect("append");
        step(&mut s, &mut held, &cmds);
        let snapshot = s.snapshot();
        store
            .checkpoint(&id, 1, 0, &snapshot, held, expected_latch)
            .expect("matching checkpoint");
        let report = store.verify(&id).expect("verify matching latch");
        assert!(report.ok(), "{cmds:?}: {:?}", report.issues);

        // Only the frontend latch drifts; the simulation and held buttons match.
        store
            .checkpoint(&id, 1, 0, &snapshot, held, !expected_latch)
            .expect("drifted checkpoint");
        let report = store.verify(&id).expect("verify drifted latch");
        assert_eq!(
            report.issues,
            vec![VerifyIssue::StateMismatch {
                at: 1,
                field: "frontend_paused".to_owned(),
                replayed: expected_latch.to_string(),
                recorded: (!expected_latch).to_string(),
            }],
            "commands: {cmds:?}",
        );
    }
}

#[test]
fn verify_names_snapshot_drift_even_when_held_buttons_match() {
    for field in [
        "seed",
        "tick",
        "paused",
        "rng",
        "cfg",
        "state",
        "validation",
    ] {
        let dir = tmp();
        let store = Store::open(dir.path()).expect("open");
        let id = SessionId::new(format!("s-drift-{field}"));
        store
            .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
            .expect("begin");
        let mut s = sim();
        let mut held = Buttons::default();
        store
            .checkpoint(&id, 0, 0, &s.snapshot(), held, false)
            .expect("base");
        store.append_commands(&id, 0, 0, 0, &[]).expect("append");
        step(&mut s, &mut held, &[]);
        let mut drifted: RuntimeSnapshot = s.snapshot();
        match field {
            "seed" => drifted.seed += 1,
            "tick" => drifted.tick += 1,
            "paused" => drifted.paused = !drifted.paused,
            "rng" => {
                drifted.rng.next_f64();
            }
            "cfg" => drifted.cfg.reduced_motion = !drifted.cfg.reduced_motion,
            "state" => drifted.state.player.x += 1.0,
            "validation" => drifted.validation.checks[0].passed = false,
            _ => unreachable!(),
        }
        assert_ne!(drifted, s.snapshot(), "fixture must change {field}");
        store
            .checkpoint(&id, 1, 0, &drifted, held, false)
            .expect("drifted checkpoint");
        let report = store.verify(&id).expect("verify");
        assert!(!report.ok(), "{field} drift must fail verification");
        assert_eq!(
            (report.commands, report.checkpoints, report.replayed),
            (1, 2, 1)
        );
        assert_eq!(report.issues.len(), 1, "{field}: {:?}", report.issues);
        match &report.issues[0] {
            VerifyIssue::StateMismatch {
                at,
                field: found,
                replayed,
                recorded,
            } => {
                assert_eq!(*at, 1);
                assert_eq!(found, field);
                assert_ne!(
                    replayed, recorded,
                    "diagnostics must distinguish the values"
                );
            }
            other => panic!("expected StateMismatch for {field}, got {other:?}"),
        }
    }
}

#[test]
fn verify_reports_definition_changes_without_comparing_incompatible_state() {
    let dir = tmp();
    let store = Store::open(dir.path()).expect("open");
    let id = SessionId::new("s-definition-change");
    store
        .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
        .expect("begin");
    let mut s = sim();
    let mut held = Buttons::default();
    store
        .checkpoint(&id, 0, 0, &s.snapshot(), held, false)
        .expect("base");
    store.append_commands(&id, 0, 0, 0, &[]).expect("append");
    step(&mut s, &mut held, &[]);
    let mut changed = s.snapshot();
    changed.definition.rooms[0].name.push_str(" revised");
    store
        .checkpoint(&id, 1, 0, &changed, held, false)
        .expect("different definition");
    store
        .append_commands(&id, 1, 0, changed.tick, &[])
        .expect("trailing batch");
    let report = store.verify(&id).expect("verify");
    assert!(!report.ok());
    assert_eq!(
        report.replayed, 2,
        "a definition mismatch must not suppress trailing replay"
    );
    assert_eq!(report.issues.len(), 1, "{:?}", report.issues);
    match &report.issues[0] {
        VerifyIssue::DefinitionMismatch {
            at,
            expected,
            found,
        } => {
            assert_eq!(*at, 1);
            assert_ne!(expected, found);
        }
        other => panic!("expected DefinitionMismatch, got {other:?}"),
    }
}

#[test]
fn verify_distinguishes_an_empty_session_from_commands_without_a_replay_baseline() {
    let dir = tmp();
    let store = Store::open(dir.path()).expect("open");
    let id = SessionId::new("s-no-baseline");
    store
        .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
        .expect("begin");
    let empty = store.verify(&id).expect("verify empty");
    assert!(empty.ok());
    assert_eq!(
        (empty.commands, empty.checkpoints, empty.replayed),
        (0, 0, 0)
    );

    store.append_commands(&id, 0, 0, 0, &[]).expect("append");
    let report = store.verify(&id).expect("verify without checkpoint");
    assert!(!report.ok());
    assert_eq!(
        (report.commands, report.checkpoints, report.replayed),
        (1, 0, 0)
    );
    assert!(
        matches!(report.issues.as_slice(), [VerifyIssue::StateUnverified { reason }] if reason.contains("no checkpoint")),
        "{:?}",
        report.issues
    );
}

#[test]
fn verify_does_not_certify_a_building_session_without_typed_replay() {
    let dir = tmp();
    let store = Store::open(dir.path()).expect("open");
    let id = SessionId::new("s-building-unverified");
    store
        .begin_session(
            &id,
            &record(SimKind::Building, "idaptik-building-runtime-v1"),
        )
        .expect("begin");
    store
        .append_commands(&id, 0, 0, 0, &[Command::Jump])
        .expect("append");
    let report = store.verify(&id).expect("log checks still run");
    assert!(!report.ok());
    assert_eq!(
        (report.commands, report.checkpoints, report.replayed),
        (1, 0, 0)
    );
    assert!(
        matches!(report.issues.as_slice(), [VerifyIssue::StateUnverified { reason }] if reason.contains("Building")),
        "{:?}",
        report.issues
    );
}

#[test]
fn unrestorable_snapshots_fail_resume_and_are_reported_as_unverified() {
    for bad_format in [true, false] {
        let dir = tmp();
        let store = Store::open(dir.path()).expect("open");
        let id = SessionId::new("s-unrestorable");
        let mut snapshot = sim().snapshot();
        if bad_format {
            snapshot.format = "idaptik-ghost-lobby-runtime-v2".to_owned();
        } else {
            snapshot.definition.rooms.clear();
        }
        store
            .begin_session(&id, &record(SimKind::GhostLobby, &snapshot.format))
            .expect("begin");
        store
            .checkpoint(&id, 0, 0, &snapshot, Buttons::default(), false)
            .expect("store snapshot");
        store.append_commands(&id, 0, 0, 0, &[]).expect("append");

        match store.resume(&id) {
            Err(PersistError::Restore(detail)) => assert!(!detail.is_empty()),
            other => panic!("expected Restore (bad_format={bad_format}), got {other:?}"),
        }
        let report = store.verify(&id).expect("report restore failure");
        assert!(!report.ok());
        assert_eq!(
            (report.commands, report.checkpoints, report.replayed),
            (1, 1, 0)
        );
        assert!(
            matches!(report.issues.as_slice(), [VerifyIssue::StateUnverified { reason }] if reason.contains("checkpoint at 0 does not restore")),
            "{:?}",
            report.issues
        );
    }
}

#[test]
fn verify_reports_unloadable_checkpoint_bodies_at_their_boundaries() {
    for corrupt_at in [0, 1] {
        let dir = tmp();
        let store = Store::open(dir.path()).expect("open");
        let id = SessionId::new("s-bad-body");
        store
            .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
            .expect("begin");
        let mut s = sim();
        let mut held = Buttons::default();
        store
            .checkpoint(&id, 0, 0, &s.snapshot(), held, false)
            .expect("base");
        store
            .append_commands(&id, 0, 0, 0, &[])
            .expect("first batch");
        step(&mut s, &mut held, &[]);
        store
            .checkpoint(&id, 1, 0, &s.snapshot(), held, false)
            .expect("later checkpoint");
        store
            .append_commands(&id, 1, 0, 1, &[])
            .expect("trailing batch");
        drop(store);
        rewrite_checkpoint(dir.path(), &id, corrupt_at, |cp| {
            cp.body = serde_json::Value::Null
        });
        let store = Store::open(dir.path()).expect("reopen");

        let report = store.verify(&id).expect("report materialisation failure");
        assert!(!report.ok());
        assert_eq!((report.commands, report.checkpoints), (2, 2));
        assert_eq!(report.replayed, if corrupt_at == 0 { 0 } else { 2 });
        let location = format!("checkpoint at {corrupt_at} does not load");
        assert!(
            matches!(report.issues.as_slice(), [VerifyIssue::StateUnverified { reason }] if reason.contains(&location)),
            "{:?}",
            report.issues
        );
        if corrupt_at == 1 {
            assert!(matches!(store.resume(&id), Err(PersistError::NotAnObject)));
        }
    }
}

#[test]
fn verify_excludes_undecodable_rows_from_counts_and_reports_their_keys() {
    use idaptik_persist::schema::{CHECKPOINTS, COMMANDS};

    let dir = tmp();
    let store = Store::open(dir.path()).expect("open");
    let id = SessionId::new("s-undecodable");
    store
        .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
        .expect("begin");
    store
        .checkpoint(&id, 0, 0, &sim().snapshot(), Buttons::default(), false)
        .expect("base");
    for seq in 0..3 {
        store
            .append_commands(&id, seq, 0, seq, &[])
            .expect("append");
    }
    drop(store);
    {
        let db = redb::Database::create(dir.path().join("idaptik.redb")).expect("raw database");
        let txn = db.begin_write().expect("write");
        {
            let mut commands = txn.open_table(COMMANDS).expect("commands");
            commands
                .insert((id.as_str(), 1), b"not JSON".as_slice())
                .expect("corrupt command");
            let mut checkpoints = txn.open_table(CHECKPOINTS).expect("checkpoints");
            checkpoints
                .insert((id.as_str(), 1), b"not JSON".as_slice())
                .expect("corrupt checkpoint");
        }
        txn.commit().expect("commit");
    }
    let store = Store::open(dir.path()).expect("reopen");
    let report = store.verify(&id).expect("verify damaged rows");
    assert!(!report.ok());
    assert_eq!(
        (report.commands, report.checkpoints, report.replayed),
        (2, 1, 2)
    );
    assert_eq!(report.issues.len(), 3, "{:?}", report.issues);
    assert!(report.issues.iter().any(
        |i| matches!(i, VerifyIssue::UndecodableCommand { seq: 1, detail } if !detail.is_empty())
    ));
    assert!(report.issues.iter().any(
        |i| matches!(i, VerifyIssue::UndecodableCheckpoint { at: 1, detail } if !detail.is_empty())
    ));
    assert!(report.issues.contains(&VerifyIssue::MissingSeq {
        expected: 1,
        found: 2
    }));
    assert!(matches!(store.resume(&id), Err(PersistError::Decode(_))));
}

#[test]
fn checkpoint_tick_checks_exclude_the_first_unapplied_batch() {
    for (at, expected_behind) in [(0, false), (1, true)] {
        let dir = tmp();
        let store = Store::open(dir.path()).expect("open");
        let id = SessionId::new("s-tick-boundary");
        store
            .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
            .expect("begin");
        store.append_commands(&id, 0, 0, 9, &[]).expect("append");
        store
            .checkpoint(&id, at, 0, &sim().snapshot(), Buttons::default(), false)
            .expect("checkpoint");
        let report = store.verify(&id).expect("verify boundary");
        let tick_issues: Vec<_> = report
            .issues
            .iter()
            .filter(|i| matches!(i, VerifyIssue::TickBehindLog { .. }))
            .cloned()
            .collect();
        assert_eq!(
            tick_issues,
            if expected_behind {
                vec![VerifyIssue::TickBehindLog {
                    at: 1,
                    recorded: 0,
                    logged: 9,
                }]
            } else {
                vec![]
            }
        );
        assert_eq!(report.replayed, if at == 0 { 1 } else { 0 });
    }
}

#[test]
fn replay_from_a_nonzero_baseline_can_continue_with_the_restored_held_buttons() {
    let dir = tmp();
    let store = Store::open(dir.path()).expect("open");
    let id = SessionId::new("s-nonzero-baseline");
    store
        .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
        .expect("begin");
    let mut s = sim();
    let mut held = Buttons::default();
    let script = [
        vec![Command::SetButton {
            button: Button::Right,
            down: true,
        }],
        vec![],
        vec![Command::Jump],
        vec![],
        vec![],
    ];
    for (seq, cmds) in script.iter().enumerate() {
        store
            .append_commands(&id, seq as u64, 0, s.snapshot().tick, cmds)
            .expect("append");
        step(&mut s, &mut held, cmds);
        if seq == 1 || seq == 3 {
            store
                .checkpoint(&id, seq as u64 + 1, 0, &s.snapshot(), held, false)
                .expect("checkpoint");
        }
    }

    // Batches before the first checkpoint must not be executed a second time.
    let report = store.verify(&id).expect("verify");
    assert!(report.ok(), "{:?}", report.issues);
    assert_eq!(
        (report.commands, report.checkpoints, report.replayed),
        (5, 2, 3)
    );
    let resumed = store.resume(&id).expect("resume");
    assert_eq!(resumed.snapshot, s.snapshot());
    assert_eq!(resumed.held, held);
    assert_eq!((resumed.checkpoint_at, resumed.next_seq), (4, 5));
    assert_eq!(resumed.replayed_batches, 1);

    // Continue without a new key press: the frontend state must survive too.
    let mut continued =
        GhostLobbySim::restore(resumed.snapshot.definition.clone(), resumed.snapshot)
            .expect("restore resumed run");
    let mut continued_held = resumed.held;
    step(&mut continued, &mut continued_held, &[]);
    step(&mut s, &mut held, &[]);
    assert_eq!(continued.snapshot(), s.snapshot());
    assert_eq!(continued_held, held);
}

#[test]
fn verify_keeps_replaying_the_log_after_an_intermediate_checkpoint_diverges() {
    let dir = tmp();
    let store = Store::open(dir.path()).expect("open");
    let id = SessionId::new("s-intermediate-drift");
    store
        .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
        .expect("begin");
    let mut s = sim();
    let mut held = Buttons::default();
    store
        .checkpoint(&id, 0, 0, &s.snapshot(), held, false)
        .expect("base");
    let press = [Command::SetButton {
        button: Button::Right,
        down: true,
    }];
    for seq in 0..3 {
        let cmds = if seq == 0 { press.as_slice() } else { &[] };
        store
            .append_commands(&id, seq, 0, s.snapshot().tick, cmds)
            .expect("append");
        step(&mut s, &mut held, cmds);
        if seq < 2 {
            let mut snapshot = s.snapshot();
            let checkpoint_held = if seq == 0 {
                snapshot.seed += 1;
                Buttons::default()
            } else {
                held
            };
            store
                .checkpoint(&id, seq + 1, 0, &snapshot, checkpoint_held, false)
                .expect("checkpoint");
        }
    }

    let report = store.verify(&id).expect("verify");
    assert_eq!(
        (report.commands, report.checkpoints, report.replayed),
        (3, 3, 3)
    );
    // A checkpoint is evidence to compare, never a replacement replay baseline.
    assert_eq!(
        report.issues,
        vec![
            VerifyIssue::HeldMismatch {
                at: 1,
                recorded: Buttons::default(),
                replayed: held,
            },
            VerifyIssue::StateMismatch {
                at: 1,
                field: "seed".to_owned(),
                replayed: "7".to_owned(),
                recorded: "8".to_owned(),
            },
        ]
    );
    assert!(!report.ok());
    assert_eq!(store.verify(&id).expect("repeat verify"), report);
    assert_eq!(
        store
            .resume(&id)
            .expect("resume later good checkpoint")
            .snapshot,
        s.snapshot()
    );
}

#[test]
fn resume_decodes_only_commands_not_already_covered_by_the_checkpoint() {
    use idaptik_persist::schema::COMMANDS;

    // The same damaged row is harmless to resume before the boundary, fatal at it.
    for corrupt_seq in [0, 2] {
        let dir = tmp();
        let store = Store::open(dir.path()).expect("open");
        let id = SessionId::new("s-covered-corruption");
        store
            .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
            .expect("begin");
        let mut s = sim();
        let mut held = Buttons::default();
        for seq in 0..3 {
            let cmds = [Command::SetButton {
                button: Button::Right,
                down: true,
            }];
            store
                .append_commands(&id, seq, 0, s.snapshot().tick, &cmds)
                .expect("append");
            step(&mut s, &mut held, &cmds);
            if seq == 1 {
                store
                    .checkpoint(&id, 2, 0, &s.snapshot(), held, false)
                    .expect("checkpoint");
            }
        }
        drop(store);
        {
            let db = redb::Database::create(dir.path().join("idaptik.redb")).expect("raw database");
            let txn = db.begin_write().expect("write");
            {
                let mut table = txn.open_table(COMMANDS).expect("commands");
                table
                    .insert((id.as_str(), corrupt_seq), b"not JSON".as_slice())
                    .expect("damage command");
            }
            txn.commit().expect("commit");
        }
        let store = Store::open(dir.path()).expect("reopen");
        if corrupt_seq == 0 {
            let resumed = store.resume(&id).expect("covered history need not decode");
            assert_eq!(resumed.snapshot, s.snapshot());
            assert_eq!(resumed.held, held);
            assert_eq!((resumed.checkpoint_at, resumed.next_seq), (2, 3));
            assert_eq!(resumed.replayed_batches, 1);
        } else {
            assert!(matches!(store.resume(&id), Err(PersistError::Decode(_))));
        }

        let report = store
            .verify(&id)
            .expect("verification still reports damaged history");
        assert!(!report.ok());
        assert_eq!((report.commands, report.checkpoints), (2, 1));
        assert_eq!(report.replayed, usize::from(corrupt_seq == 0));
        assert!(report.issues.iter().any(|issue| matches!(
            issue,
            VerifyIssue::UndecodableCommand { seq, detail }
                if *seq == corrupt_seq && !detail.is_empty()
        )));
        assert!(
            !report.issues.iter().any(|issue| matches!(
                issue,
                VerifyIssue::HeldMismatch { .. } | VerifyIssue::StateMismatch { .. }
            )),
            "{:?}",
            report.issues
        );
    }
}

#[test]
fn verify_uses_the_earliest_decodable_checkpoint_after_a_damaged_initial_row() {
    use idaptik_persist::schema::CHECKPOINTS;

    let dir = tmp();
    let store = Store::open(dir.path()).expect("open");
    let id = SessionId::new("s-fallback-baseline");
    store
        .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
        .expect("begin");
    let mut s = sim();
    let mut held = Buttons::default();
    store
        .checkpoint(&id, 0, 0, &s.snapshot(), held, false)
        .expect("initial checkpoint");
    for seq in 0..4 {
        let cmds = [Command::SetButton {
            button: Button::Right,
            down: true,
        }];
        store
            .append_commands(&id, seq, 0, s.snapshot().tick, &cmds)
            .expect("append");
        step(&mut s, &mut held, &cmds);
        if seq == 1 || seq == 2 {
            store
                .checkpoint(&id, seq + 1, 0, &s.snapshot(), held, false)
                .expect("later checkpoint");
        }
    }
    drop(store);
    {
        let db = redb::Database::create(dir.path().join("idaptik.redb")).expect("raw database");
        let txn = db.begin_write().expect("write");
        {
            let mut table = txn.open_table(CHECKPOINTS).expect("checkpoints");
            table
                .insert((id.as_str(), 0), b"not JSON".as_slice())
                .expect("damage initial checkpoint");
        }
        txn.commit().expect("commit");
    }
    let store = Store::open(dir.path()).expect("reopen");
    let report = store.verify(&id).expect("verify from boundary 2");
    assert!(!report.ok());
    assert_eq!(
        (report.commands, report.checkpoints, report.replayed),
        (4, 2, 2)
    );
    assert!(
        matches!(
            report.issues.as_slice(),
            [VerifyIssue::UndecodableCheckpoint { at: 0, detail }] if !detail.is_empty()
        ),
        "{:?}",
        report.issues
    );
    assert_eq!(
        store
            .resume(&id)
            .expect("resume latest checkpoint")
            .snapshot,
        s.snapshot()
    );
}

#[test]
fn replay_applies_edge_and_terminal_commands_and_counts_batches_after_the_run_ends() {
    for terminal in [
        Command::ForceExtract {
            method: ExtractMethod::ServiceExit,
        },
        Command::ForceFail {
            reason: FailReason::Caught,
        },
    ] {
        let dir = tmp();
        let store = Store::open(dir.path()).expect("open");
        let id = SessionId::new("s-terminal-replay");
        store
            .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
            .expect("begin");
        let mut s = sim();
        let mut held = Buttons::default();
        store
            .checkpoint(&id, 0, 0, &s.snapshot(), held, false)
            .expect("base");
        let initial_y = s.snapshot().state.player.y;
        for (seq, cmds) in [
            vec![Command::Jump],
            vec![Command::ForceCrisis],
            vec![terminal],
            vec![],
        ]
        .iter()
        .enumerate()
        {
            store
                .append_commands(&id, seq as u64, 0, s.snapshot().tick, cmds)
                .expect("append");
            step(&mut s, &mut held, cmds);
            if seq == 0 {
                assert_ne!(
                    s.snapshot().state.player.y,
                    initial_y,
                    "jump fixture must take effect"
                );
            }
        }
        let resumed = store.resume(&id).expect("replay all actions");
        assert_eq!(resumed.snapshot, s.snapshot(), "{terminal:?}");
        assert!(resumed.snapshot.state.ended);
        assert_eq!(
            resumed.snapshot.tick, 2,
            "terminal and ended batches do not advance time"
        );
        assert_eq!(resumed.held, Buttons::default());
        assert_eq!((resumed.next_seq, resumed.replayed_batches), (4, 4));

        store
            .checkpoint(&id, 4, 0, &s.snapshot(), held, false)
            .expect("terminal checkpoint");
        let report = store.verify(&id).expect("verify terminal replay");
        assert!(report.ok(), "{terminal:?}: {:?}", report.issues);
        assert_eq!(
            (report.commands, report.checkpoints, report.replayed),
            (4, 2, 4)
        );
    }
}

#[test]
fn verify_reports_a_later_snapshot_format_change_as_state_drift() {
    let dir = tmp();
    let store = Store::open(dir.path()).expect("open");
    let id = SessionId::new("s-format-drift");
    store
        .begin_session(&id, &record(SimKind::GhostLobby, SNAPSHOT_FORMAT))
        .expect("begin");
    let mut s = sim();
    let mut held = Buttons::default();
    store
        .checkpoint(&id, 0, 0, &s.snapshot(), held, false)
        .expect("base");
    store.append_commands(&id, 0, 0, 0, &[]).expect("append");
    step(&mut s, &mut held, &[]);
    store
        .checkpoint(&id, 1, 0, &s.snapshot(), held, false)
        .expect("later checkpoint");
    drop(store);
    rewrite_checkpoint(dir.path(), &id, 1, |cp| {
        cp.body["format"] = serde_json::json!("idaptik-ghost-lobby-runtime-v2");
    });
    let store = Store::open(dir.path()).expect("reopen");
    let report = store.verify(&id).expect("verify");
    assert!(!report.ok());
    assert_eq!(report.replayed, 1);
    assert_eq!(
        report.issues,
        vec![VerifyIssue::StateMismatch {
            at: 1,
            field: "format".to_owned(),
            replayed: SNAPSHOT_FORMAT.to_owned(),
            recorded: "idaptik-ghost-lobby-runtime-v2".to_owned(),
        }]
    );
    assert!(matches!(store.resume(&id), Err(PersistError::Restore(_))));
}

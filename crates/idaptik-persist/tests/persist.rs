//! The store's tests. One of them exists specifically to prove the others can
//! fail — see `verify_can_fail`.

use idaptik_core::scenario::{
    Button, Buttons, Command, GhostLobbySim, RunConfig, SNAPSHOT_FORMAT, ghost_lobby,
};
use idaptik_persist::{
    FlushPolicy, PersistError, SessionId, SessionRecord, SessionStatus, SimKind, Store, VerifyIssue,
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
    assert!(resumed.held.has(Button::Left), "Left must survive the resume");
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

    let s = sim();
    let mut held = Buttons::default();
    store
        .append_commands(
            &id,
            0,
            0,
            0,
            &[Command::SetButton {
                button: Button::Left,
                down: true,
            }],
        )
        .expect("append");
    held.set(Button::Left, true);
    store
        .checkpoint(&id, 0, 0, &s.snapshot(), held, false)
        .expect("checkpoint");

    let report = store.verify(&id).expect("verify");
    assert!(report.ok(), "expected no issues, got {:?}", report.issues);
    assert_eq!(report.commands, 1);
    assert_eq!(report.checkpoints, 1);
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
        .begin_session(&id, &record(SimKind::GhostLobby, "idaptik-ghost-lobby-runtime-v2"))
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
        .begin_session(&id, &record(SimKind::Building, "idaptik-building-runtime-v1"))
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
    assert!(report.ok(), "expected a clean report, got {:?}", report.issues);
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
    use idaptik_persist::schema::{META, SCHEMA_KEY};
    use redb::Database;

    let dir = tmp();
    {
        let store = Store::open(dir.path()).expect("open");
        drop(store);
    }
    // Rewrite the schema tag behind the store's back, as a newer build would.
    {
        let db = Database::create(dir.path().join("idaptik.redb")).expect("reopen raw");
        let txn = db.begin_write().expect("write");
        {
            let mut meta = txn.open_table(META).expect("meta");
            let bytes = serde_json::to_vec(&99u32).expect("encode");
            meta.insert(SCHEMA_KEY, bytes.as_slice()).expect("insert");
        }
        txn.commit().expect("commit");
    }
    match Store::open(dir.path()) {
        Err(PersistError::SchemaMismatch { found, expected }) => {
            assert_eq!(found, 99);
            assert_eq!(expected, idaptik_persist::SCHEMA_VERSION);
        }
        other => panic!("expected SchemaMismatch, got {other:?}"),
    }
}

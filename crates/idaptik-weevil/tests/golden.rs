//! The committed Enaction documents are exactly what the driver writes.
//!
//! `tests/fixtures/enaction/` holds the `enaction-runner-output/2` and
//! `enaction-mind-trace/1` documents of twelve runs (six of them through the
//! ABI 1.2 model record, checked under `enaction-policies-models.json`), for a
//! consumer that can
//! fetch this repository but cannot build the kernel (the UMS preview). These
//! tests keep them honest:
//!
//! * each one is byte-equal to what the built binary writes today for the same
//!   arguments, so a change to the emitter or the kernel fails here rather than
//!   silently leaving stale goldens behind;
//! * `SHA256SUMS` is regenerated over the complete sorted file set and
//!   compared, so an extra, missing or changed file fails;
//! * every mind trace keeps its SHA-256 from 462af7c, before contacts had a
//!   memory: the memory is the world's, and the kernel sees nothing new.

use idaptik_weevil::canonical::sha256_hex;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Runs whose runner output is committed.
const RUNNER_ONLY: [&str; 2] = ["decoy-lure", "cautious-sentry"];
/// Runs whose runner output and mind trace are both committed: the four 1.1
/// mind runs, then the six slice 2 model runs.
const WITH_MIND: [&str; 10] = [
    "the-grudge",
    "the-grudge-control",
    "the-flinch",
    "the-flinch-control",
    "the-stalker",
    "the-stalker-control",
    "caught",
    "caught-control",
    "false-exposure",
    "false-exposure-control",
];

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/enaction")
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Run the driver with the fixtures' arguments and return what it wrote.
fn emit(profile: &str, mind: bool) -> (Vec<u8>, Option<Vec<u8>>) {
    // One directory per call: two tests of this process may emit the same
    // profile at once.
    static CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "idaptik-weevil-golden-{}-{call}-{profile}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let runner = dir.join("runner.json");
    let trace = dir.join("mind.json");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_idaptik-weevil-replay"));
    cmd.args(["--profile", profile, "--ticks", "200", "--seed", "7"])
        .arg("--emit-epistemic")
        .arg(&runner);
    if mind {
        cmd.arg("--emit-mind-trace").arg(&trace);
    }
    let status = cmd.status().expect("the replay driver runs");
    assert!(status.success(), "{profile}: the driver failed");
    let out = (read(&runner), mind.then(|| read(&trace)));
    std::fs::remove_dir_all(&dir).ok();
    out
}

#[test]
fn every_committed_document_is_what_the_binary_writes() {
    let dir = fixtures();
    for profile in RUNNER_ONLY {
        let (runner, _) = emit(profile, false);
        assert!(
            runner == read(&dir.join(format!("{profile}.runner-output.json"))),
            "{profile}.runner-output.json is stale"
        );
    }
    for profile in WITH_MIND {
        let (runner, trace) = emit(profile, true);
        assert!(
            runner == read(&dir.join(format!("{profile}.runner-output.json"))),
            "{profile}.runner-output.json is stale"
        );
        assert!(
            trace.expect("a mind trace") == read(&dir.join(format!("{profile}.mind-trace.json"))),
            "{profile}.mind-trace.json is stale"
        );
    }
}

/// SHA-256 of each mind trace as committed at 462af7c, before contacts had
/// a memory. A contact's memory is the world's, and the kernel receives no
/// new input from it, so the kernel's mind must not change.
const MIND_TRACES_AT_462AF7C: [(&str, &str); 10] = [
    (
        "caught-control",
        "bc23fdc6efa04ff4e855ac20b7c96ebadace5ee9e21989942d63e4be59ad06dd",
    ),
    (
        "caught",
        "90856df753a61e2e58c156698e19078bb3b5f20c581404759f7d22a5071d98d8",
    ),
    (
        "false-exposure-control",
        "f9fdc8519d5df0bfce6c5948b819b206e934689a10eb6c00e76e46a4d9c590a4",
    ),
    (
        "false-exposure",
        "7b6eaa3a0a477ffc69ef97f7000fa0c68f020e4bbc30d18c0cfcb0d0b16e2432",
    ),
    (
        "the-flinch-control",
        "a9dc497770383b262a1bf508fd6fd9fb4ddceb6ff38911af3e8f9f35b09835e0",
    ),
    (
        "the-flinch",
        "06ecc36e59fcacd4f766e854b0736d389b4a1ca0b6050e2c040a06987a20ce69",
    ),
    (
        "the-grudge-control",
        "b61e8e18b771ce9fcf5e84a5c0792ebcc4de72b8ceec5e847b91bb5fe3f40807",
    ),
    (
        "the-grudge",
        "5fda5e6e2f35dbebd1686493d69e661a6cd56b195459444d2b5358582817424f",
    ),
    (
        "the-stalker-control",
        "bc23fdc6efa04ff4e855ac20b7c96ebadace5ee9e21989942d63e4be59ad06dd",
    ),
    (
        "the-stalker",
        "98254aa0bcf72115d26bdb2c80222bc1cbba8b8c32cea599790f985f413f8b36",
    ),
];

#[test]
fn every_mind_trace_is_the_one_committed_before_contacts_remembered() {
    let dir = fixtures();
    let mut pinned: Vec<&str> = MIND_TRACES_AT_462AF7C.iter().map(|(p, _)| *p).collect();
    pinned.sort_unstable();
    let mut all = WITH_MIND.to_vec();
    all.sort_unstable();
    assert_eq!(pinned, all, "every mind trace is pinned");
    for (profile, sum) in MIND_TRACES_AT_462AF7C {
        let committed = read(&dir.join(format!("{profile}.mind-trace.json")));
        assert_eq!(sha256_hex(&committed), sum, "{profile}.mind-trace.json");
        let (_, written) = emit(profile, true);
        assert_eq!(
            sha256_hex(&written.expect("a mind trace")),
            sum,
            "{profile}: the binary's mind trace"
        );
    }
}

#[test]
fn sha256sums_cover_the_complete_sorted_file_set() {
    let dir = fixtures();
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("the fixture directory")
        .map(|e| {
            e.expect("an entry")
                .file_name()
                .into_string()
                .expect("UTF-8")
        })
        .filter(|n| n != "SHA256SUMS")
        .collect();
    names.sort();
    let expected: String = names
        .iter()
        .map(|n| format!("{}  {n}\n", sha256_hex(&read(&dir.join(n)))))
        .collect();
    let committed = String::from_utf8(read(&dir.join("SHA256SUMS"))).expect("UTF-8");
    assert_eq!(
        committed, expected,
        "SHA256SUMS does not match the files on disk"
    );

    // The set itself: every scenario's documents and the README, nothing else.
    let mut wanted: Vec<String> = RUNNER_ONLY
        .iter()
        .chain(&WITH_MIND)
        .map(|p| format!("{p}.runner-output.json"))
        .chain(WITH_MIND.iter().map(|p| format!("{p}.mind-trace.json")))
        .chain(["README.md".to_owned()])
        .collect();
    wanted.sort();
    assert_eq!(
        names, wanted,
        "the fixture set has an extra or a missing file"
    );
}

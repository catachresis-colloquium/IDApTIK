//! The committed Enaction documents are exactly what the driver writes.
//!
//! `tests/fixtures/enaction/` holds the `enaction-runner-output/1` and
//! `enaction-mind-trace/1` documents of eight runs, for a consumer that can
//! fetch this repository but cannot build the kernel (the UMS preview). These
//! tests keep them honest:
//!
//! * each one is byte-equal to what the built binary writes today for the same
//!   arguments, so a change to the emitter or the kernel fails here rather than
//!   silently leaving stale goldens behind;
//! * `SHA256SUMS` is regenerated over the complete sorted file set and
//!   compared, so an extra, missing or changed file fails.

use idaptik_weevil::canonical::sha256_hex;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Runs whose runner output is committed.
const RUNNER_ONLY: [&str; 2] = ["decoy-lure", "cautious-sentry"];
/// Runs whose runner output and mind trace are both committed.
const WITH_MIND: [&str; 4] = [
    "the-grudge",
    "the-grudge-control",
    "the-flinch",
    "the-flinch-control",
];

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/enaction")
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Run the driver with the fixtures' arguments and return what it wrote.
fn emit(profile: &str, mind: bool) -> (Vec<u8>, Option<Vec<u8>>) {
    let dir = std::env::temp_dir().join(format!(
        "idaptik-weevil-golden-{}-{profile}",
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

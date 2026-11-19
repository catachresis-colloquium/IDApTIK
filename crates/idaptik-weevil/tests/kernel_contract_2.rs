//! Enaction kernel contract 2.0 in the committed runner outputs.
//!
//! Enaction ADR-0030 makes the kernel contract 2.0. At enaction-engine
//! `11bd247` that version is a draft: its shape and rules may still change
//! with no further bump, and nothing may be published as conforming to it.
//! These tests pin the goldens to the draft as it stands there.
//!
//! ADR-0009 forbids an Enaction dependency, so every rule here is written
//! from the contract text (CAC-KERNEL sections 6 and 9, ADR-0029 and
//! ADR-0030) and from the bytes of Enaction's `conformance/tom/v2` corpus,
//! never from Enaction's source:
//!
//! * The envelope is `enaction-runner-output/2`. The manifest declares
//!   `kernel_version` 2.0 and a profile whose `targets_kernel` is 2.0. Every
//!   v2 corpus package keeps `migration_from` at 1.0, and so does this one.
//! * `enaction-policies/1` and `enaction-mind-trace/1` keep their tags.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Every committed run: the six model scenarios last.
const RUNS: [&str; 12] = [
    "decoy-lure",
    "cautious-sentry",
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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn runner(profile: &str) -> Value {
    let path = fixtures().join(format!("enaction/{profile}.runner-output.json"));
    serde_json::from_slice(&read(&path)).expect("a JSON document")
}

#[test]
fn every_golden_is_an_enaction_runner_output_2_document() {
    let v2 = json!({"major": 2, "minor": 0});
    let v1 = json!({"major": 1, "minor": 0});
    for profile in RUNS {
        let doc = runner(profile);
        assert_eq!(doc["contract"], "enaction-runner-output/2", "{profile}");
        let manifest = &doc["package"]["manifest"];
        assert_eq!(manifest["kernel_version"], v2, "{profile}: kernel_version");
        assert_eq!(
            manifest["profile"]["targets_kernel"], v2,
            "{profile}: the profile targets kernel 2.0"
        );
        assert_eq!(
            manifest["migration_from"], v1,
            "{profile}: migration_from as the v2 corpus keeps it"
        );
    }
}

#[test]
fn the_policies_and_the_mind_traces_keep_their_tags() {
    for name in ["enaction-policies.json", "enaction-policies-models.json"] {
        let doc: Value = serde_json::from_slice(&read(&fixtures().join(name))).expect("JSON");
        assert_eq!(doc["contract"], "enaction-policies/1", "{name}");
    }
    let mut traces = 0;
    for profile in RUNS {
        let path = fixtures().join(format!("enaction/{profile}.mind-trace.json"));
        if !path.exists() {
            continue;
        }
        let doc: Value = serde_json::from_slice(&read(&path)).expect("JSON");
        assert_eq!(doc["contract"], "enaction-mind-trace/1", "{profile}");
        traces += 1;
    }
    assert_eq!(traces, 10, "ten runs carry a mind trace");
}

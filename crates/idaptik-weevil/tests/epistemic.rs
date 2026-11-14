//! The `enaction-runner-output/1` document the replay driver emits.
//!
//! These tests link the native kernel, like `replay_repro.rs`. They check the
//! properties the document must have for `enaction-validate` to admit it and
//! for a reader to trust it:
//!
//! * the bytes are canonical (RFC 8785 JCS over integer-only I-JSON) and a
//!   pure function of the run configuration;
//! * `manifest.digest` is the SHA-256 of the canonical package without that
//!   one member, so changing any byte of an event breaks it;
//! * every belief sits on a tick where the kernel acted from memory, about the
//!   target it acted on, and cites observations of that target;
//! * a guard fooled by a decoy holds a belief the world's truth does not
//!   contain, and a guard remembering the real player does not.

use idaptik_weevil::canonical::{sha256_hex, to_jcs};
use idaptik_weevil::epistemic::{
    BELIEVED_FLOOR, REJECTED_CEILING, SUSPECTED_FLOOR, UNKNOWN_BAND, runner_output,
};
use idaptik_weevil::profile::ProfileSet;
use idaptik_weevil::run_trace_with_truth;
use serde_json::Value;
use std::collections::BTreeSet;
use std::process::Command;

fn emit(profile_id: &str, ticks: u64, seed: u64) -> Vec<u8> {
    let set = ProfileSet::canonical();
    let profile = set.get(profile_id).expect("known profile");
    let (trace, truth) = run_trace_with_truth(ticks, profile, seed).expect("a trace runs");
    runner_output(&trace, &truth)
        .expect("the run has a document")
        .to_canonical_bytes()
        .expect("the document is in the integer subset")
}

fn parse(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).expect("the document is JSON")
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .expect("an array")
        .iter()
        .map(|s| s.as_str().expect("a string").to_owned())
        .collect()
}

#[test]
fn the_document_is_canonical_and_a_pure_function_of_the_run() {
    let a = emit("decoy-lure", 200, 7);
    let b = emit("decoy-lure", 200, 7);
    assert_eq!(a, b, "same configuration, same bytes");
    let reencoded = to_jcs(&parse(&a)).expect("canonical");
    assert_eq!(
        reencoded.as_bytes(),
        &a[..],
        "the emitted bytes are already in JCS form"
    );
    assert_eq!(parse(&a)["contract"], "enaction-runner-output/1");
}

#[test]
fn the_digest_covers_the_package_without_itself() {
    let doc = parse(&emit("decoy-lure", 200, 7));
    let mut package = doc["package"].clone();
    let declared = package["manifest"]["digest"]
        .as_str()
        .expect("a digest")
        .to_owned();
    package["manifest"]
        .as_object_mut()
        .expect("a manifest")
        .remove("digest");
    let computed = sha256_hex(to_jcs(&package).expect("canonical").as_bytes());
    assert_eq!(declared, computed);
    assert_eq!(declared.len(), 64);

    // One changed proposition must move the digest: a digest that ignored
    // the events would pass the line above and vouch for anything.
    package["events"][0]["proposition"] = Value::String("tampered".into());
    let tampered = sha256_hex(to_jcs(&package).expect("canonical").as_bytes());
    assert_ne!(declared, tampered);
}

#[test]
fn every_belief_is_a_from_memory_decision_backed_by_observations_of_its_target() {
    let set = ProfileSet::canonical();
    let profile = set.get("decoy-lure").expect("known profile");
    let (trace, truth) = run_trace_with_truth(200, profile, 7).expect("a trace runs");
    let doc = parse(
        &runner_output(&trace, &truth)
            .expect("a document")
            .to_canonical_bytes()
            .expect("bytes"),
    );
    let pkg = &doc["package"];
    let beliefs = pkg["beliefs"].as_array().expect("beliefs");
    let from_memory: Vec<_> = trace
        .entries
        .iter()
        .filter(|e| e.intent.from_memory)
        .collect();
    assert!(
        !from_memory.is_empty(),
        "the decoy run must act from memory"
    );
    assert_eq!(
        beliefs.len(),
        from_memory.len(),
        "one belief per from-memory decision, no more and no fewer"
    );

    let events = pkg["events"].as_array().expect("events");
    let mut last = (0u64, 0u64);
    for e in events {
        assert_eq!(e["kind"], "observation");
        assert_eq!(e["observer"], "guard-0");
        let stamp = (
            e["tick"].as_u64().expect("tick"),
            e["sequence"].as_u64().expect("sequence"),
        );
        assert!(
            stamp > last,
            "events are strictly ordered by (tick, sequence)"
        );
        last = stamp;
    }

    for (belief, entry) in beliefs.iter().zip(from_memory) {
        assert_eq!(belief["bearer"], "guard-0");
        assert_eq!(belief["valid_from"].as_u64(), Some(entry.tick));
        let target = format!("contact-{}", entry.intent.target_id);
        let prop = belief["proposition"].as_str().expect("a proposition");
        assert!(
            prop.starts_with(&format!("{target} in room-")),
            "{prop} is about {target}"
        );
        let sources = belief["source_events"].as_array().expect("sources");
        assert!(!sources.is_empty());
        for id in sources {
            let ev = events
                .iter()
                .find(|e| e["event_id"] == *id)
                .expect("a cited event is in the package");
            assert_eq!(ev["subject"]["agent"], target.as_str());
            assert!(ev["tick"].as_u64().expect("tick") <= entry.tick);
        }
    }
}

#[test]
fn a_guard_fooled_by_the_decoy_believes_what_the_world_does_not_hold() {
    let doc = parse(&emit("decoy-lure", 200, 7));
    let truth: BTreeSet<String> = strings(&doc["world_truth"]).into_iter().collect();
    let wrong: Vec<String> = doc["package"]["beliefs"]
        .as_array()
        .expect("beliefs")
        .iter()
        .map(|b| b["proposition"].as_str().expect("a string").to_owned())
        .filter(|p| !truth.contains(p))
        .collect();
    assert!(!wrong.is_empty(), "the decoy must leave a false belief");
    assert!(
        wrong.iter().all(|p| p.starts_with("contact-200 ")),
        "only the decoy is believed falsely: {wrong:?}"
    );
}

#[test]
fn a_guard_remembering_the_real_player_is_right() {
    let doc = parse(&emit("cautious-sentry", 200, 7));
    let beliefs = doc["package"]["beliefs"].as_array().expect("beliefs");
    assert!(!beliefs.is_empty(), "the sentry acts from memory");
    let truth: BTreeSet<String> = strings(&doc["world_truth"]).into_iter().collect();
    for b in beliefs {
        let p = b["proposition"].as_str().expect("a string");
        assert!(truth.contains(p), "{p} is true in the corridor");
    }
}

#[test]
fn a_run_that_never_acts_from_memory_holds_no_belief() {
    let doc = parse(&emit("aggressive-pursuer", 200, 7));
    assert_eq!(doc["package"]["beliefs"].as_array().map(Vec::len), Some(0));
    assert_eq!(doc["package"]["events"].as_array().map(Vec::len), Some(0));
    assert_eq!(doc["world_truth"].as_array().map(Vec::len), Some(0));
}

#[test]
fn the_policies_fixture_is_the_policy_the_emitter_declares_statuses_under() {
    let raw = include_str!("fixtures/enaction-policies.json");
    let policies: Value = serde_json::from_str(raw).expect("JSON");
    assert_eq!(
        raw.trim_end(),
        to_jcs(&policies).expect("canonical"),
        "the fixture is canonical"
    );
    assert_eq!(policies["contract"], "enaction-policies/1");
    let s = &policies["status"];
    assert_eq!(
        s["rejected_ceiling"].as_u64(),
        Some(u64::from(REJECTED_CEILING))
    );
    assert_eq!(s["unknown_band"].as_u64(), Some(u64::from(UNKNOWN_BAND)));
    assert_eq!(
        s["believed_floor"].as_u64(),
        Some(u64::from(BELIEVED_FLOOR))
    );
    assert_eq!(
        s["suspected_floor"].as_u64(),
        Some(u64::from(SUSPECTED_FLOOR))
    );
    assert_eq!(policies["depth"]["max_order"].as_u64(), Some(0));
}

#[test]
fn two_processes_write_identical_bytes() {
    let dir = std::env::temp_dir().join(format!("idaptik-weevil-epistemic-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let mut outputs = Vec::new();
    for run in ["a", "b"] {
        let path = dir.join(format!("{run}.json"));
        let status = Command::new(env!("CARGO_BIN_EXE_idaptik-weevil-replay"))
            .args(["--profile", "decoy-lure", "--ticks", "200", "--seed", "7"])
            .arg("--emit-epistemic")
            .arg(&path)
            .status()
            .expect("the replay driver runs");
        assert!(status.success());
        outputs.push(std::fs::read(&path).expect("the document was written"));
    }
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(outputs[0], outputs[1]);
    assert_eq!(outputs[0], emit("decoy-lure", 200, 7));
}

/// `(contact, room, tick)` from a stated proposition, in the one form the
/// emitter writes: `contact-<id> in room-<r> at tick <t>`.
fn parse_located(p: &str) -> (u32, usize, u64) {
    let rest = p.strip_prefix("contact-").expect("a contact proposition");
    let (id, rest) = rest.split_once(" in room-").expect("a room");
    let (room, tick) = rest.split_once(" at tick ").expect("a tick");
    (
        id.parse().expect("an id"),
        room.parse().expect("a room index"),
        tick.parse().expect("a tick"),
    )
}

/// CAC-KERNEL §9: world truth is closed over the propositions the package
/// states. A stated proposition absent from `world_truth` is false, so every
/// proposition a belief or an event states must be in `world_truth` exactly
/// when the corridor's own state makes it true on its tick.
#[test]
fn world_truth_is_closed_over_every_stated_proposition() {
    let set = ProfileSet::canonical();
    for profile_id in ["decoy-lure", "cautious-sentry"] {
        let profile = set.get(profile_id).expect("known profile");
        let (trace, truth) = run_trace_with_truth(200, profile, 7).expect("a trace runs");
        let doc = parse(
            &runner_output(&trace, &truth)
                .expect("a document")
                .to_canonical_bytes()
                .expect("bytes"),
        );
        let atoms: BTreeSet<String> = strings(&doc["world_truth"]).into_iter().collect();
        let pkg = &doc["package"];
        let stated: BTreeSet<String> = pkg["events"]
            .as_array()
            .expect("events")
            .iter()
            .chain(pkg["beliefs"].as_array().expect("beliefs"))
            .map(|x| x["proposition"].as_str().expect("a string").to_owned())
            .collect();
        assert!(!stated.is_empty(), "{profile_id} states something");
        for p in &stated {
            let (id, room, tick) = parse_located(p);
            let at = truth
                .iter()
                .find(|t| t.tick == tick)
                .expect("the world was recorded on that tick");
            let holds = at.contacts.contains(&(id, room));
            assert_eq!(
                atoms.contains(p),
                holds,
                "{profile_id}: `{p}` is {} in the world but {} world_truth",
                if holds { "true" } else { "false" },
                if atoms.contains(p) {
                    "in"
                } else {
                    "absent from"
                }
            );
        }
        for a in &atoms {
            assert!(
                stated.contains(a),
                "{profile_id}: truth atom `{a}` is stated by nothing"
            );
        }
    }
}

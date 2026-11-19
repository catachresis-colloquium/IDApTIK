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
//! * Every supersession link is `{belief, because}`, and its cause meets
//!   the six cause rules. The beliefs of one key form one chain (ADR-0029).
//!
//! One planted mutant for each cause rule shows that the checker here can
//! fail. The ignored test hands every golden and every mutant to the real
//! `enaction-validate`; run it with `ENACTION_VALIDATE` set to the binary and
//! `--ignored`. Without the variable it fails, it does not pass.

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

// ---------------------------------------------------------------------------
// The supersession link names its cause (ADR-0030, stage B).
//
// * `supersedes` is absent, or `{belief, because}`: the id of the belief
//   replaced and the id of an event of the same package.
// * The revising mind of a belief is its outermost holder, or its bearer when
//   it has no holder.
// * Six rules, in the order of the contract's table. A link is refused when:
//   `supersession-cause-present`, `because` names no event of the package;
//   `supersession-cause-observed`, the cause's `observer` is not the revising
//   mind of the successor; `supersession-cause-cited`, the cause is not among
//   the successor's `source_events`; `supersession-cause-new`, the cause is
//   among the `source_events` of the belief replaced;
//   `supersession-cause-not-earlier`, the cause's `tick` is before the
//   `valid_from` of the belief replaced; `supersession-cause-not-later`, the
//   cause's `tick` is after the successor's `valid_from`.
// * With no event for `because`, only `-present` is reported. With no belief
//   to replace, `supersedes-present` reports it and `-new` and `-not-earlier`
//   are skipped.
//
// ADR-0029, one key one chain: the beliefs that share a key (`holders`,
// `bearer`, `proposition`) form one chain under `supersedes`. A belief of the
// key other than the earliest by (`valid_from`, id) that supersedes no belief
// of the key is a silent overwrite; a second belief that supersedes a belief
// of the key that another already supersedes is a fork. Both are
// `belief-key-chained`. ADR-0029's second rule, `supersession-monotone`, is
// no longer a rule: the two timing rules above imply it.
// ---------------------------------------------------------------------------

/// The runs that state models; they are checked under depth 1.
const MODEL_RUNS: [&str; 6] = [
    "the-stalker",
    "the-stalker-control",
    "caught",
    "caught-control",
    "false-exposure",
    "false-exposure-control",
];

fn id(v: &Value) -> u64 {
    v.as_u64().expect("an integer id")
}

fn ids(v: &Value) -> Vec<u64> {
    v.as_array().expect("a list").iter().map(id).collect()
}

fn holders(b: &Value) -> Vec<String> {
    b.get("holders")
        .map(|h| {
            h.as_array()
                .expect("holders")
                .iter()
                .map(|s| s.as_str().expect("a name").to_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// The outermost holder, or the bearer when there is no holder.
fn revising_mind(b: &Value) -> String {
    holders(b)
        .first()
        .cloned()
        .unwrap_or_else(|| b["bearer"].as_str().expect("a bearer").to_owned())
}

/// `(belief, because)` of a link, in the only shape 2.0 has.
fn link(b: &Value) -> Option<(u64, u64)> {
    let s = b.get("supersedes")?;
    let o = s.as_object().expect("a link is an object");
    let mut members: Vec<&str> = o.keys().map(String::as_str).collect();
    members.sort_unstable();
    assert_eq!(members, ["because", "belief"], "a link has two members");
    Some((id(&s["belief"]), id(&s["because"])))
}

fn beliefs(doc: &Value) -> &Vec<Value> {
    doc["package"]["beliefs"].as_array().expect("beliefs")
}

fn find<'a>(list: &'a Value, key: &str, want: u64) -> Option<&'a Value> {
    list.as_array()
        .expect("a list")
        .iter()
        .find(|x| id(&x[key]) == want)
}

/// Every fault of the six cause rules and of `supersedes-present`, as
/// `(rule, successor)`, by successor in document order and rule in table order.
fn cause_faults(doc: &Value) -> Vec<(&'static str, u64)> {
    let pkg = &doc["package"];
    let mut out = Vec::new();
    for b in beliefs(doc) {
        let Some((replaced, because)) = link(b) else {
            continue;
        };
        let me = id(&b["belief_id"]);
        let old = find(&pkg["beliefs"], "belief_id", replaced);
        if old.is_none() {
            out.push(("supersedes-present", me));
        }
        let Some(cause) = find(&pkg["events"], "event_id", because) else {
            out.push(("supersession-cause-present", me));
            continue;
        };
        let tick = id(&cause["tick"]);
        if cause["observer"].as_str() != Some(revising_mind(b).as_str()) {
            out.push(("supersession-cause-observed", me));
        }
        if !ids(&b["source_events"]).contains(&because) {
            out.push(("supersession-cause-cited", me));
        }
        if let Some(old) = old {
            if ids(&old["source_events"]).contains(&because) {
                out.push(("supersession-cause-new", me));
            }
            if tick < id(&old["valid_from"]) {
                out.push(("supersession-cause-not-earlier", me));
            }
        }
        if tick > id(&b["valid_from"]) {
            out.push(("supersession-cause-not-later", me));
        }
    }
    out
}

/// Every `belief-key-chained` fault, as `(shape, belief)`.
fn key_chain_faults(doc: &Value) -> Vec<(&'static str, u64)> {
    type Key = (Vec<String>, String, String);
    let key = |b: &Value| -> Key {
        (
            holders(b),
            b["bearer"].as_str().expect("a bearer").to_owned(),
            b["proposition"].as_str().expect("a proposition").to_owned(),
        )
    };
    let mut groups: std::collections::BTreeMap<Key, Vec<&Value>> = Default::default();
    for b in beliefs(doc) {
        groups.entry(key(b)).or_default().push(b);
    }
    let mut out = Vec::new();
    for members in groups.values() {
        let in_key: Vec<u64> = members.iter().map(|b| id(&b["belief_id"])).collect();
        let earliest = members
            .iter()
            .map(|b| (id(&b["valid_from"]), id(&b["belief_id"])))
            .min()
            .expect("a key has a belief")
            .1;
        let mut by_id = members.clone();
        by_id.sort_by_key(|b| id(&b["belief_id"]));
        let mut replaced = std::collections::BTreeSet::new();
        for b in by_id {
            let me = id(&b["belief_id"]);
            let in_key_link = link(b).filter(|(old, _)| in_key.contains(old));
            if let Some((old, _)) = in_key_link {
                // A second successor of one belief is a fork.
                if !replaced.insert(old) {
                    out.push(("fork", me));
                }
            } else if me != earliest {
                out.push(("silent-overwrite", me));
            }
        }
    }
    out
}

fn links(doc: &Value) -> usize {
    beliefs(doc).iter().filter(|b| link(b).is_some()).count()
}

#[test]
fn every_golden_link_names_a_cause_that_meets_the_six_rules() {
    let mut total = 0;
    for profile in RUNS {
        let doc = runner(profile);
        assert_eq!(cause_faults(&doc), [], "{profile}");
        total += links(&doc);
    }
    // caught 1, the-stalker 1, false-exposure 3, false-exposure-control 1:
    // the rules above are not checked over an empty set.
    assert_eq!(total, 6, "the goldens hold six links");
}

#[test]
fn every_golden_keeps_one_key_one_chain() {
    for profile in RUNS {
        assert_eq!(key_chain_faults(&runner(profile)), [], "{profile}");
    }
}

/// Recompute `manifest.digest` after a mutation, so that a mutant is refused
/// for its planted fault and not for a stale digest.
fn redigest(doc: &mut Value) {
    use idaptik_weevil::canonical::{sha256_hex, to_jcs};
    let mut package = doc["package"].clone();
    package["manifest"]
        .as_object_mut()
        .expect("a manifest")
        .remove("digest");
    let digest = sha256_hex(to_jcs(&package).expect("canonical").as_bytes());
    doc["package"]["manifest"]["digest"] = Value::String(digest);
}

/// Set one belief's link cause and sources in a copy of a golden.
fn plant(doc: &Value, belief: u64, because: u64, sources: &[u64]) -> Value {
    let mut doc = doc.clone();
    let b = doc["package"]["beliefs"]
        .as_array_mut()
        .expect("beliefs")
        .iter_mut()
        .find(|b| id(&b["belief_id"]) == belief)
        .expect("the belief");
    b["supersedes"]["because"] = json!(because);
    b["source_events"] = json!(sources);
    redigest(&mut doc);
    doc
}

/// One mutant of `false-exposure` for each cause rule, each breaking that
/// rule alone. The golden holds three links: 3 replaces 1 because of event 2
/// (the cue, tick 21), 4 replaces 2 because of event 4 (the contact's memory
/// ends, tick 52), and 5 replaces 3 because of event 5 (expiry, tick 112).
/// Event 1 is the guard's sighting at tick 1 and event 3 the contact's own
/// sight of the guard at tick 21.
fn mutants() -> Vec<(&'static str, u64, Value)> {
    let doc = runner("false-exposure");
    let shape = |b: u64| {
        let b = find(&doc["package"]["beliefs"], "belief_id", b).expect("a belief");
        (link(b), ids(&b["source_events"]))
    };
    // The links the mutants start from, so a regenerated golden that moved
    // them fails here and not in a confusing way below.
    assert_eq!(shape(3), (Some((1, 2)), vec![2]));
    assert_eq!(shape(4), (Some((2, 4)), vec![4]));
    assert_eq!(shape(5), (Some((3, 5)), vec![5]));
    vec![
        // An event the package does not have.
        ("supersession-cause-present", 4, plant(&doc, 4, 99, &[4])),
        // The contact's event as the cause of the guard's revision.
        ("supersession-cause-observed", 5, plant(&doc, 5, 4, &[4, 5])),
        // The expiry, not cited by the belief it licenses.
        ("supersession-cause-cited", 5, plant(&doc, 5, 5, &[1])),
        // The cause taken from the replaced belief's own sources.
        ("supersession-cause-new", 4, plant(&doc, 4, 3, &[3, 4])),
        // A cause before the replaced belief starts.
        (
            "supersession-cause-not-earlier",
            5,
            plant(&doc, 5, 1, &[1, 5]),
        ),
        // A cause after the successor starts: the expiry at tick 112 licenses
        // a model dated 111. (Citing event 5 from belief 3 instead would also
        // make it old evidence for belief 5, and break `-new` there.)
        ("supersession-cause-not-later", 5, {
            let mut d = plant(&doc, 5, 5, &[5]);
            d["package"]["beliefs"][4]["valid_from"] = json!(111);
            redigest(&mut d);
            d
        }),
    ]
}

#[test]
fn each_planted_mutant_breaks_exactly_its_rule() {
    let all = mutants();
    assert_eq!(all.len(), 6, "one mutant for each cause rule");
    for (rule, belief, doc) in all {
        assert_eq!(cause_faults(&doc), [(rule, belief)], "{rule}");
        assert_eq!(key_chain_faults(&doc), [], "{rule}: the chain is intact");
    }
}

#[test]
fn the_key_chain_check_refuses_an_unlinked_restatement_and_a_fork() {
    let doc = runner("false-exposure");
    let mut unlinked = doc.clone();
    unlinked["package"]["beliefs"][3]
        .as_object_mut()
        .expect("belief 4")
        .remove("supersedes");
    assert_eq!(key_chain_faults(&unlinked), [("silent-overwrite", 4)]);
    let mut fork = doc.clone();
    fork["package"]["beliefs"][4]["supersedes"]["belief"] = json!(1);
    assert_eq!(key_chain_faults(&fork), [("fork", 5)]);
}

/// The real `enaction-validate` (enaction-engine `11bd247`) over every golden
/// and every mutant. It needs a built validator, named by
/// `ENACTION_VALIDATE`; without one it fails rather than passes.
#[test]
#[ignore = "needs enaction-validate from enaction-engine; set ENACTION_VALIDATE to its path"]
fn the_real_validator_admits_every_golden_and_refuses_each_mutant() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let bin = std::env::var("ENACTION_VALIDATE")
        .expect("ENACTION_VALIDATE names the enaction-validate binary");
    let version = Command::new(&bin)
        .arg("--version")
        .output()
        .expect("the validator runs");
    let version = String::from_utf8(version.stdout).expect("UTF-8");
    println!("{}", version.trim_end());
    assert!(version.contains("kernel contract 2.0"), "{version}");

    let validate = |input: &[u8], policies: &Path, trace: Option<&Path>| {
        let mut cmd = Command::new(&bin);
        cmd.arg("--policies").arg(policies);
        if let Some(t) = trace {
            cmd.arg("--mind-trace").arg(t);
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the validator starts");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(input)
            .expect("the document is written");
        let out = child.wait_with_output().expect("the validator ends");
        (
            out.status.code(),
            String::from_utf8(out.stdout).expect("UTF-8"),
        )
    };

    let depth0 = fixtures().join("enaction-policies.json");
    let depth1 = fixtures().join("enaction-policies-models.json");
    for profile in RUNS {
        let path = fixtures().join(format!("enaction/{profile}.runner-output.json"));
        let golden = read(&path);
        let trace = fixtures().join(format!("enaction/{profile}.mind-trace.json"));
        let trace = trace.exists().then_some(trace);
        let policies = if MODEL_RUNS.contains(&profile) {
            &depth1
        } else {
            &depth0
        };
        let (code, stdout) = validate(&golden, policies, trace.as_deref());
        assert_eq!(code, Some(0), "{profile}: {stdout}");
        assert_eq!(
            stdout.lines().next().map(str::as_bytes),
            Some(&golden[..]),
            "{profile}: the validator echoes the golden's canonical bytes"
        );
        println!(
            "{profile}: exit 0 under {}{}",
            policies.file_name().and_then(|n| n.to_str()).unwrap_or("?"),
            if trace.is_some() {
                " with its mind trace"
            } else {
                ""
            }
        );
    }

    for (rule, belief, doc) in mutants() {
        let bytes = idaptik_weevil::canonical::to_jcs(&doc).expect("canonical");
        let (code, stdout) = validate(bytes.as_bytes(), &depth1, None);
        println!("mutant {rule}: exit {code:?} {}", stdout.trim_end());
        assert_eq!(code, Some(1), "{rule}");
        let faults: Value = serde_json::from_str(&stdout).expect("one JSON line");
        let path = format!("package.beliefs[belief_id={belief}].supersedes.because");
        let faults = faults["faults"].as_array().expect("faults");
        assert_eq!(faults.len(), 1, "{rule}: exactly one fault");
        assert_eq!(faults[0]["rule"], rule);
        assert_eq!(faults[0]["path"], path.as_str());
    }
}

//! The `enaction-mind-trace/1` document the replay driver emits beside its
//! `enaction-runner-output/1`, from one run through the ABI 1.1 mind.
//!
//! These tests link the native kernel. They check what `enaction-validate`
//! checks and what a reader relies on:
//!
//! * one record per agent per tick, the vocabularies in the kernel's order,
//!   and no score or reservoir anywhere in the bytes;
//! * every change of affect token names at least one event, every named event
//!   is in the paired runner output, and an unchanged token names none;
//! * "the grudge" shows commitment override and its control does not; "the
//!   flinch" carries a startle and its control does not.

use idaptik_weevil::canonical::to_jcs;
use idaptik_weevil::mind_trace::documents;
use idaptik_weevil::profile::ProfileSet;
use idaptik_weevil::run_corridor;
use serde_json::Value;
use std::collections::BTreeSet;
use std::process::Command;

fn emit(profile_id: &str) -> (Value, Value, Vec<u8>, Vec<u8>) {
    let set = ProfileSet::canonical();
    let profile = set.get(profile_id).expect("known profile");
    let run = run_corridor(200, profile, 7, true).expect("a mind run");
    let (runner, mind) = documents(&run).expect("both documents");
    let rb = runner.to_canonical_bytes().expect("runner bytes");
    let mb = mind.to_canonical_bytes().expect("mind bytes");
    (
        serde_json::from_slice(&rb).expect("JSON"),
        serde_json::from_slice(&mb).expect("JSON"),
        rb,
        mb,
    )
}

fn names(v: &Value) -> Vec<String> {
    v.as_array()
        .expect("an array")
        .iter()
        .map(|s| s.as_str().expect("a string").to_owned())
        .collect()
}

/// Ticks where the selected candidate is the incumbent and not first.
fn overrides(mind: &Value) -> Vec<u64> {
    mind["ticks"]
        .as_array()
        .expect("ticks")
        .iter()
        .filter(|t| {
            let c = &t["conation"];
            c["selected"] == c["incumbent"] && c["ranking"][0] != c["selected"]
        })
        .map(|t| t["tick"].as_u64().expect("a tick"))
        .collect()
}

fn ticks_with_tell(mind: &Value, tell: &str) -> Vec<u64> {
    mind["ticks"]
        .as_array()
        .expect("ticks")
        .iter()
        .filter(|t| names(&t["tells"]).iter().any(|n| n == tell))
        .map(|t| t["tick"].as_u64().expect("a tick"))
        .collect()
}

#[test]
fn the_document_is_canonical_one_record_per_tick_in_kernel_vocabulary() {
    let (_, mind, _, mb) = emit("the-grudge");
    assert_eq!(
        to_jcs(&mind).expect("canonical").as_bytes(),
        &mb[..],
        "the emitted bytes are already JCS"
    );
    assert_eq!(mind["contract"], "enaction-mind-trace/1");
    assert_eq!(mind["producer"]["kernel"], "evil-weevil");
    assert_eq!(mind["producer"]["abi"], "1.2");
    let v = &mind["vocabulary"];
    assert_eq!(
        names(&v["affect_tokens"]),
        ["calm", "wary", "afraid", "angry"]
    );
    assert_eq!(
        names(&v["candidates"]),
        ["advance", "engage", "evade", "reload", "investigate"]
    );
    assert_eq!(names(&v["tells"]), ["startle", "hesitate", "fixate"]);
    let ticks: Vec<u64> = mind["ticks"]
        .as_array()
        .expect("ticks")
        .iter()
        .map(|t| t["tick"].as_u64().expect("a tick"))
        .collect();
    assert_eq!(ticks, (1..=200).collect::<Vec<_>>());
}

#[test]
fn no_score_and_no_reservoir_crosses_into_the_document() {
    for id in ["the-grudge", "the-flinch"] {
        let (_, mind, _, mb) = emit(id);
        let text = String::from_utf8(mb).expect("UTF-8");
        for word in [
            "score",
            "utility",
            "arousal",
            "fear",
            "grievance",
            "reservoir",
        ] {
            assert!(!text.contains(word), "{id}: `{word}` is in the mind trace");
        }
        for t in mind["ticks"].as_array().expect("ticks") {
            if let Some(a) = t["affect"].get("attention") {
                assert!(
                    a.as_u64().expect("an integer") <= 10_000,
                    "attention is a mass"
                );
            }
        }
    }
}

#[test]
fn every_affect_change_is_caused_by_events_of_the_paired_runner_output() {
    for id in [
        "the-grudge",
        "the-grudge-control",
        "the-flinch",
        "the-flinch-control",
    ] {
        let (runner, mind, _, _) = emit(id);
        let event_ids: BTreeSet<u64> = runner["package"]["events"]
            .as_array()
            .expect("events")
            .iter()
            .map(|e| e["event_id"].as_u64().expect("an id"))
            .collect();
        let mut previous: Option<String> = None;
        let mut changes = 0;
        for t in mind["ticks"].as_array().expect("ticks") {
            let token = t["affect"]["token"].as_str().expect("a token").to_owned();
            let causes: Vec<u64> = t["affect"]["caused_by"]
                .as_array()
                .expect("caused_by")
                .iter()
                .map(|c| c.as_u64().expect("an id"))
                .collect();
            let changed = previous.as_ref().is_some_and(|p| *p != token)
                || (previous.is_none() && token != "calm");
            if changed {
                changes += 1;
                assert!(
                    !causes.is_empty(),
                    "{id}: tick {} changes token uncaused",
                    t["tick"]
                );
            } else {
                assert!(
                    causes.is_empty(),
                    "{id}: tick {} names a cause without a change",
                    t["tick"]
                );
            }
            for c in causes {
                assert!(event_ids.contains(&c), "{id}: cause {c} is not an event");
            }
            previous = Some(token);
        }
        assert!(changes > 0, "{id}: the run changes affect at least once");
    }
}

#[test]
fn the_grudge_overrides_its_ranking_and_its_control_does_not() {
    let (_, grudge, _, _) = emit("the-grudge");
    let o = overrides(&grudge);
    assert!(!o.is_empty(), "the grudge shows commitment override");
    // The kernel's own fixture measures 37 ticks, 20 to 56. The corridor
    // stages that fixture, and these are its numbers.
    assert_eq!(o.len(), 37);
    assert_eq!((o[0], o[o.len() - 1]), (20, 56));
    let (_, control, _, _) = emit("the-grudge-control");
    assert!(overrides(&control).is_empty(), "no grievance, no override");
}

#[test]
fn the_flinch_startles_and_its_control_does_not() {
    let (_, flinch, _, _) = emit("the-flinch");
    assert_eq!(ticks_with_tell(&flinch, "startle"), [10]);
    let (_, control, _, _) = emit("the-flinch-control");
    assert!(ticks_with_tell(&control, "startle").is_empty());
}

#[test]
fn two_processes_write_identical_bytes_for_both_documents() {
    let dir = std::env::temp_dir().join(format!("idaptik-weevil-mind-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let mut outputs = Vec::new();
    for run in ["a", "b"] {
        let epi = dir.join(format!("{run}-runner.json"));
        let mind = dir.join(format!("{run}-mind.json"));
        let status = Command::new(env!("CARGO_BIN_EXE_idaptik-weevil-replay"))
            .args(["--profile", "the-grudge", "--ticks", "200", "--seed", "7"])
            .arg("--emit-epistemic")
            .arg(&epi)
            .arg("--emit-mind-trace")
            .arg(&mind)
            .status()
            .expect("the replay driver runs");
        assert!(status.success());
        outputs.push((
            std::fs::read(&epi).expect("runner output written"),
            std::fs::read(&mind).expect("mind trace written"),
        ));
    }
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(outputs[0], outputs[1]);
    let (_, _, rb, mb) = emit("the-grudge");
    assert_eq!(
        outputs[0],
        (rb, mb),
        "the binary writes what the library builds"
    );
}

#[test]
fn a_mind_trace_without_its_runner_output_is_refused() {
    let status = Command::new(env!("CARGO_BIN_EXE_idaptik-weevil-replay"))
        .args(["--profile", "the-grudge", "--ticks", "20"])
        .arg("--emit-mind-trace")
        .arg(std::env::temp_dir().join("idaptik-weevil-lonely-mind.json"))
        .status()
        .expect("the replay driver runs");
    assert_eq!(
        status.code(),
        Some(2),
        "the two documents come from one run, so they are asked for together"
    );
}

/// Whether the corridor's recorded truth makes a stated proposition true,
/// decided here from the world model, independently of the emitter. A form
/// this test does not know fails the test, so a new kind of event cannot
/// slip past it.
fn holds(p: &str, truth: &[idaptik_weevil::TickTruth]) -> bool {
    let (body, tick) = p
        .rsplit_once(" at tick ")
        .expect("every proposition is timed");
    let tick: u64 = tick.parse().expect("a tick");
    let at = truth
        .iter()
        .find(|t| t.tick == tick)
        .expect("a recorded tick");
    let contact_id = |s: &str| -> u32 {
        s.strip_prefix("contact-")
            .expect("a contact")
            .parse()
            .expect("an id")
    };
    if let Some((who, room)) = body.split_once(" in room-") {
        return at
            .contacts
            .contains(&(contact_id(who), room.parse().expect("a room")));
    }
    if let Some(who) = body.strip_suffix(" damaged guard-0") {
        return at.hits.contains(&contact_id(who));
    }
    if body == "guard-0 took damage" {
        return !at.hits.is_empty();
    }
    if let Some(who) = body.strip_suffix(" not found") {
        let id = contact_id(who);
        return !at.visible.contains(&id) && !at.contacts.contains(&(id, at.guard_room));
    }
    if body == "no contact visible to guard-0" {
        return !at.contacts.iter().any(|(_, room)| *room == at.guard_room);
    }
    if body == "guard-0 health below half" {
        return at.guard_health < 32_768;
    }
    if body == "guard-0 out of ammo" {
        return at.guard_ammo == 0;
    }
    panic!("a proposition form this test cannot decide: `{p}`");
}

/// CAC-KERNEL §9, as for the epistemic document alone: world truth is closed
/// over every proposition the package states, now including the events that
/// affect changes cite.
#[test]
fn world_truth_is_closed_over_every_stated_proposition_of_a_mind_run() {
    let set = ProfileSet::canonical();
    for id in [
        "the-grudge",
        "the-grudge-control",
        "the-flinch",
        "the-flinch-control",
    ] {
        let run = run_corridor(200, set.get(id).expect("known"), 7, true).expect("a mind run");
        let (runner, _) = documents(&run).expect("documents");
        let doc: Value =
            serde_json::from_slice(&runner.to_canonical_bytes().expect("bytes")).expect("JSON");
        let atoms: BTreeSet<String> = names(&doc["world_truth"]).into_iter().collect();
        let pkg = &doc["package"];
        let stated: BTreeSet<String> = pkg["events"]
            .as_array()
            .expect("events")
            .iter()
            .chain(pkg["beliefs"].as_array().expect("beliefs"))
            .map(|x| x["proposition"].as_str().expect("a string").to_owned())
            .collect();
        for p in &stated {
            assert_eq!(
                atoms.contains(p),
                holds(p, &run.truth),
                "{id}: `{p}` and world_truth disagree"
            );
        }
        for a in &atoms {
            assert!(stated.contains(a), "{id}: atom `{a}` is stated by nothing");
        }
    }
}

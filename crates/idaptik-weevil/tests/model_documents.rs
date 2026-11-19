//! The documents of the slice 2 scenarios: four separate facts for one
//! proposition.
//!
//! * **What the world is**: `world_truth`, closed over every stated order-0
//!   proposition and over no model as such.
//! * **What the contact believes**: order-0 beliefs with `bearer` the contact,
//!   one for each change of the world's rule ([`knows`]), each superseding the
//!   last.
//! * **What the guard thinks the contact believes**: order-1 beliefs,
//!   `holders = ["guard-0"]`, the same proposition, one for each change of the
//!   kernel's exposure and never restated.
//! * **Whether the guard is right about the contact** is left to the reader
//!   (CAC-KERNEL §9): the producer asserts nothing about it, and the document
//!   has no member for it.
//!
//! Every expectation here is computed from the run's own records (the
//! world's truth and the kernel's model records), not from the emitter.

use idaptik_weevil::abi;
use idaptik_weevil::mind_trace::documents;
use idaptik_weevil::profile::ProfileSet;
use idaptik_weevil::world::knows;
use idaptik_weevil::{CorridorRun, TickTruth, run_corridor};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const CONTACT: &str = "contact-41";
const POSITION: &str = "guard-0 in room-0";

const SCENARIOS: [&str; 6] = [
    "the-stalker",
    "the-stalker-control",
    "caught",
    "caught-control",
    "false-exposure",
    "false-exposure-control",
];

fn emit(id: &str) -> (CorridorRun, Value, Value) {
    let set = ProfileSet::canonical();
    let run = run_corridor(200, set.get(id).expect("known"), 7, true).expect("a model run");
    let (runner, mind) = documents(&run).expect("both documents");
    let r = serde_json::from_slice(&runner.to_canonical_bytes().expect("bytes")).expect("JSON");
    let m = serde_json::from_slice(&mind.to_canonical_bytes().expect("bytes")).expect("JSON");
    (run, r, m)
}

fn beliefs(doc: &Value) -> &Vec<Value> {
    doc["package"]["beliefs"].as_array().expect("beliefs")
}

fn events(doc: &Value) -> BTreeMap<u64, Value> {
    doc["package"]["events"]
        .as_array()
        .expect("events")
        .iter()
        .map(|e| (e["event_id"].as_u64().expect("an id"), e.clone()))
        .collect()
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

fn models(doc: &Value) -> Vec<Value> {
    beliefs(doc)
        .iter()
        .filter(|b| !holders(b).is_empty())
        .cloned()
        .collect()
}

fn own(doc: &Value) -> Vec<Value> {
    beliefs(doc)
        .iter()
        .filter(|b| holders(b).is_empty() && b["bearer"] == CONTACT)
        .cloned()
        .collect()
}

fn tick(b: &Value) -> u64 {
    b["valid_from"].as_u64().expect("valid_from")
}

fn status(b: &Value) -> &str {
    b["status"].as_str().expect("a status")
}

fn holds_status(s: &str) -> bool {
    matches!(s, "certain" | "believed" | "suspected")
}

/// Each belief after the first supersedes the one before it, and the first
/// supersedes nothing. A link is `{belief, because}` (Enaction ADR-0030).
fn assert_chain(id: &str, chain: &[Value]) {
    for (i, b) in chain.iter().enumerate() {
        match i {
            0 => assert!(b.get("supersedes").is_none(), "{id}: the first is new"),
            _ => assert_eq!(
                b["supersedes"]["belief"],
                chain[i - 1]["belief_id"],
                "{id}: belief {} supersedes the one before it",
                b["belief_id"]
            ),
        }
    }
}

/// The event a link names as its cause.
fn because<'a>(b: &Value, events: &'a BTreeMap<u64, Value>) -> &'a Value {
    let id = b["supersedes"]["because"].as_u64().expect("a cause");
    &events[&id]
}

#[test]
fn the_contact_believes_exactly_what_the_world_rule_says_it_knows() {
    for id in SCENARIOS {
        let (run, doc, _) = emit(id);
        let own = own(&doc);
        assert_chain(id, &own);
        // The changes of the world's rule, tick by tick.
        let mut expected = Vec::new();
        let mut before = false;
        for t in &run.truth {
            let now = knows(t, 41);
            if now != before {
                expected.push((t.tick, now));
            }
            before = now;
        }
        let stated: Vec<(u64, bool)> = own
            .iter()
            .map(|b| (tick(b), holds_status(status(b))))
            .collect();
        assert_eq!(stated, expected, "{id}");
        let events = events(&doc);
        for b in &own {
            assert_eq!(b["proposition"], POSITION, "{id}: time is not in it");
            assert!(matches!(status(b), "certain" | "unknown"), "{id}");
            // The contact's belief rests on the contact's own evidence.
            for e in b["source_events"].as_array().expect("sources") {
                let e = &events[&e.as_u64().expect("an id")];
                assert_eq!(e["observer"], CONTACT, "{id}: belief {}", b["belief_id"]);
                assert_eq!(e["subject"]["agent"], "guard-0", "{id}");
                assert_eq!(e["tick"], b["valid_from"], "{id}");
            }
            // A change names the contact's evidence of that tick as its
            // cause: its sight of the guard (or else the blow it took) when
            // it comes to know, its loss of sight when its memory ends.
            if b.get("supersedes").is_some() {
                let cause = because(b, &events);
                let first = b["source_events"][0].as_u64().expect("a source");
                assert_eq!(cause["event_id"].as_u64(), Some(first), "{id}");
                let form = if holds_status(status(b)) {
                    "guard-0 in room-0"
                } else {
                    "contact-41 does not observe guard-0 at tick"
                };
                let p = cause["proposition"].as_str().expect("a proposition");
                assert!(
                    p.starts_with(form) || p.starts_with("guard-0 attacked"),
                    "{id}: {p}"
                );
            }
        }
    }
}

#[test]
fn the_guard_models_the_contact_only_when_the_kernel_changes_its_mind() {
    for id in SCENARIOS {
        let (run, doc, _) = emit(id);
        let models = models(&doc);
        if id.ends_with("-control") {
            assert!(models.is_empty(), "{id}: no capability, no claim, no model");
            continue;
        }
        assert_chain(id, &models);
        // The first model is the kernel's first claim (a target, no cue);
        // every later one is a tick on which exposure changed, and every such
        // tick has one.
        let first_claim = run
            .mind
            .iter()
            .zip(&run.model)
            .find(|(m, x)| m.target_id != 0 || x.exposure)
            .map(|(m, _)| m.tick)
            .expect("a claim");
        let mut expected = vec![(first_claim, run.model[first_claim as usize - 1].exposure)];
        for (t, m) in run.truth.iter().zip(&run.model) {
            if m.changed() {
                expected.push((t.tick, m.exposure));
            }
        }
        let stated: Vec<(u64, bool)> = models
            .iter()
            .map(|b| (tick(b), holds_status(status(b))))
            .collect();
        assert_eq!(stated, expected, "{id}");
        for b in &models {
            assert_eq!(holders(b), ["guard-0"], "{id}");
            assert_eq!(b["bearer"], CONTACT, "{id}");
            assert_eq!(b["proposition"], POSITION, "{id}: the contact's own");
            assert!(matches!(status(b), "certain" | "rejected"), "{id}");
        }
    }
}

#[test]
fn each_model_change_cites_the_event_that_caused_it() {
    for id in ["the-stalker", "caught", "false-exposure"] {
        let (run, doc, _) = emit(id);
        let events = events(&doc);
        for b in models(&doc).iter().skip(1) {
            let t = tick(b);
            let cause = run.model[t as usize - 1].cause;
            let cited: Vec<&Value> = b["source_events"]
                .as_array()
                .expect("sources")
                .iter()
                .map(|e| &events[&e.as_u64().expect("an id")])
                .collect();
            assert!(!cited.is_empty(), "{id}: tick {t} cites something");
            for e in &cited {
                assert_eq!(e["tick"].as_u64(), Some(t), "{id}: an event of tick {t}");
                assert_eq!(e["observer"], "guard-0", "{id}: the guard's evidence");
            }
            match cause {
                abi::EXPOSURE_CAUSE_CUE => {
                    assert_eq!(cited.len(), 1);
                    assert_eq!(cited[0]["kind"], "observation");
                    assert_eq!(
                        cited[0]["proposition"],
                        format!("{CONTACT} observes guard-0 at tick {t}")
                    );
                }
                abi::EXPOSURE_CAUSE_OWN_ATTACK => {
                    assert_eq!(cited.len(), 1);
                    assert_eq!(cited[0]["kind"], "action_outcome");
                    assert_eq!(
                        cited[0]["proposition"],
                        format!("guard-0 attacked {CONTACT} at tick {t}")
                    );
                }
                abi::EXPOSURE_CAUSE_EXPIRY => {
                    // The decay convention: the guard's perception that tick.
                    for e in &cited {
                        assert_eq!(e["kind"], "observation");
                        assert_eq!(e["subject"]["agent"], CONTACT);
                    }
                }
                other => panic!("{id}: tick {t} has a model and cause {other}"),
            }
            // The link names the event the change cites: the cue or the
            // attack, and for an expiry the first event of the guard's
            // perception that tick.
            assert_eq!(because(b, &events), cited[0], "{id}: tick {t}");
        }
    }
}

#[test]
fn false_exposure_holds_a_model_that_outlives_the_contacts_own_belief() {
    let (_, doc, _) = emit("false-exposure");
    let models = models(&doc);
    let own = own(&doc);
    let span: Vec<(u64, &str)> = models.iter().map(|b| (tick(b), status(b))).collect();
    assert_eq!(span, [(1, "rejected"), (21, "certain"), (112, "rejected")]);
    let mine: Vec<(u64, &str)> = own.iter().map(|b| (tick(b), status(b))).collect();
    // The contact remembers the glance at 21 for 30 ticks, so its own
    // belief is superseded by `unknown` on 52, not on 22.
    assert_eq!(mine, [(21, "certain"), (52, "unknown")]);
    // One model stated at 21 stands until 112, while the contact's own
    // belief stopped holding at 52: the stale model a reader must see.
    assert_eq!(models[2]["supersedes"]["belief"], models[1]["belief_id"]);
    let (_, control, _) = emit("false-exposure-control");
    assert!(self::models(&control).is_empty());
    assert_eq!(
        self::own(&control).len(),
        2,
        "the same world, the same contact"
    );
}

/// Whether the world makes a stated proposition true, decided here from the
/// corridor's recorded truth alone. A form this test does not know fails.
fn holds(p: &str, truth: &[TickTruth]) -> bool {
    if p == POSITION {
        return truth.iter().all(|t| t.guard_room == 0);
    }
    let (body, tick) = p.rsplit_once(" at tick ").expect("a timed proposition");
    let tick: u64 = tick.parse().expect("a tick");
    let at = truth.iter().find(|t| t.tick == tick).expect("a tick");
    match body {
        "contact-41 observes guard-0" => at.observers.contains(&41),
        "contact-41 does not observe guard-0" => !at.observers.contains(&41),
        "guard-0 attacked contact-41" => at.struck.contains(&41),
        "contact-41 in room-0" => at.contacts.contains(&(41, 0)),
        "no contact visible to guard-0" => !at.contacts.iter().any(|(_, r)| *r == at.guard_room),
        _ => panic!("a proposition form this test cannot decide: `{p}`"),
    }
}

#[test]
fn world_truth_is_closed_over_every_stated_order_0_proposition_and_no_model() {
    for id in SCENARIOS {
        let (run, doc, _) = emit(id);
        let atoms: BTreeSet<String> = doc["world_truth"]
            .as_array()
            .expect("atoms")
            .iter()
            .map(|a| a.as_str().expect("a string").to_owned())
            .collect();
        let pkg = &doc["package"];
        let stated: BTreeSet<String> = pkg["events"]
            .as_array()
            .expect("events")
            .iter()
            .chain(beliefs(&doc).iter().filter(|b| holders(b).is_empty()))
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
            assert!(
                stated.contains(a),
                "{id}: `{a}` is stated by no order-0 belief or event"
            );
        }
    }
}

#[test]
fn the_mind_trace_declares_stalk_and_caught_in_the_kernels_order() {
    for id in SCENARIOS {
        let (run, _, mind) = emit(id);
        let names = |v: &Value| -> Vec<String> {
            v.as_array()
                .expect("names")
                .iter()
                .map(|s| s.as_str().expect("a name").to_owned())
                .collect()
        };
        assert_eq!(
            names(&mind["vocabulary"]["candidates"]),
            [
                "advance",
                "engage",
                "evade",
                "reload",
                "investigate",
                "stalk"
            ],
            "{id}"
        );
        assert_eq!(
            names(&mind["vocabulary"]["tells"]),
            ["startle", "hesitate", "fixate", "caught"],
            "{id}"
        );
        let caught: Vec<u64> = mind["ticks"]
            .as_array()
            .expect("ticks")
            .iter()
            .filter(|t| names(&t["tells"]).iter().any(|n| n == "caught"))
            .map(|t| t["tick"].as_u64().expect("a tick"))
            .collect();
        let from_records: Vec<u64> = run
            .mind
            .iter()
            .filter(|m| m.tells & abi::TELL_CAUGHT != 0)
            .map(|m| m.tick)
            .collect();
        assert_eq!(caught, from_records, "{id}");
        let text = serde_json::to_string(&mind).expect("text");
        for word in ["score", "utility", "exposure_ttl", "reservoir"] {
            assert!(!text.contains(word), "{id}: `{word}` in the mind trace");
        }
    }
}

#[test]
fn the_models_policies_fixture_is_the_status_policy_with_depth_1() {
    let old: Value = serde_json::from_str(include_str!("fixtures/enaction-policies.json"))
        .expect("the existing policies");
    let raw = include_str!("fixtures/enaction-policies-models.json");
    let new: Value = serde_json::from_str(raw).expect("JSON");
    assert_eq!(
        raw.trim_end(),
        idaptik_weevil::canonical::to_jcs(&new).expect("canonical"),
        "the fixture is canonical"
    );
    assert_eq!(new["contract"], old["contract"]);
    assert_eq!(new["status"], old["status"], "the same status policy");
    assert_eq!(old["depth"]["max_order"].as_u64(), Some(0), "unchanged");
    assert_eq!(new["depth"]["max_order"].as_u64(), Some(1));
}

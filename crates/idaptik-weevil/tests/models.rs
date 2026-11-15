//! Mind slice 2, "it models you": the six model scenarios of the corridor.
//!
//! These tests link the native kernel (ABI 1.2, evil-weevil ADR-0014). They
//! check four separate facts a reader must be able to see in one runner
//! output: what the world is, what the contact believes, what the guard thinks
//! the contact believes, and whether the guard is right about the contact.
//!
//! The figures are the kernel fixtures' (ADR-0014 §7), each one tick later,
//! because the corridor counts from tick 1 and the fixtures from tick 0.

use idaptik_weevil::abi;
use idaptik_weevil::mind_trace::documents;
use idaptik_weevil::profile::ProfileSet;
use idaptik_weevil::{CorridorRun, TickTruth, run_corridor};
use serde_json::Value;
use std::collections::BTreeSet;
use std::process::Command;

const SCENARIOS: [&str; 3] = ["the-stalker", "caught", "false-exposure"];
const CONTROLS: [&str; 3] = [
    "the-stalker-control",
    "caught-control",
    "false-exposure-control",
];
const CONTACT: u32 = 41;
const LOCATED: &str = "guard-0 in room-0";

fn run(id: &str) -> CorridorRun {
    let set = ProfileSet::canonical();
    run_corridor(200, set.get(id).expect("a known profile"), 7, true).expect("a model run")
}

fn docs(id: &str) -> (Value, Value) {
    let (runner, mind) = documents(&run(id)).expect("both documents");
    (
        serde_json::from_slice(&runner.to_canonical_bytes().expect("bytes")).expect("JSON"),
        serde_json::from_slice(&mind.to_canonical_bytes().expect("bytes")).expect("JSON"),
    )
}

fn beliefs(doc: &Value) -> &Vec<Value> {
    doc["package"]["beliefs"].as_array().expect("beliefs")
}

fn event(doc: &Value, id: &Value) -> Value {
    doc["package"]["events"]
        .as_array()
        .expect("events")
        .iter()
        .find(|e| e["event_id"] == *id)
        .cloned()
        .expect("a cited event is in the package")
}

fn is_model(b: &Value) -> bool {
    b.get("holders").is_some()
}

/// The first tick whose intent is ATTACK, and the contact's fx distance then.
fn first_engage(run: &CorridorRun) -> Option<(u64, i32)> {
    run.trace
        .entries
        .iter()
        .find(|e| e.intent.action == "ATTACK")
        .map(|e| (e.tick, e.observation.contacts[0].distance))
}

#[test]
fn the_stalker_engages_only_inside_strike_range_and_its_control_earlier() {
    let r = run("the-stalker");
    // The kernel fixture: first engage at tick 160 at range 0.5 fx
    // (STRIKE_RANGE), after 160 ticks of stalking.
    assert_eq!(first_engage(&r), Some((161, abi::FX_ONE / 2)));
    let stalks = r.mind.iter().filter(|m| m.selected == 5).count();
    assert_eq!(stalks, 160);
    assert!(
        r.trace.entries[..160]
            .iter()
            .all(|e| e.intent.action == "TAKE_COVER")
    );
    // Its control: first engage at tick 64 at 1.0 fx (ATTACK_RANGE), never a
    // stalk in any ranking, no exposure.
    let c = run("the-stalker-control");
    assert_eq!(first_engage(&c), Some((65, abi::FX_ONE)));
    assert!(c.mind.iter().all(|m| !m.ranking.contains(&5)));
    assert!(c.exposure.iter().all(|x| !x.now && !x.prev));
}

#[test]
fn caught_shows_exactly_one_caught_tell_on_the_cue_tick() {
    let caught_ticks = |id: &str| -> Vec<u64> {
        let (_, mind) = docs(id);
        mind["ticks"]
            .as_array()
            .expect("ticks")
            .iter()
            .filter(|t| {
                t["tells"]
                    .as_array()
                    .expect("tells")
                    .contains(&"caught".into())
            })
            .map(|t| t["tick"].as_u64().expect("a tick"))
            .collect()
    };
    assert_eq!(caught_ticks("caught"), [101]);
    assert!(caught_ticks("caught-control").is_empty());
    let r = run("caught");
    // Exposed from the cue to the end, and no stalk after it; engaged at 116.
    let exposed: Vec<u64> = r
        .exposure
        .iter()
        .zip(&r.mind)
        .filter(|(x, _)| x.now)
        .map(|(_, m)| m.tick)
        .collect();
    assert_eq!(exposed, (101..=200).collect::<Vec<_>>());
    assert!(r.mind.iter().all(|m| m.tick <= 100 || m.selected != 5));
    assert_eq!(first_engage(&r).map(|(t, _)| t), Some(116));
}

/// (h): from the world model only, never from a contract derivation. A tick
/// of false exposure is one on which the kernel's exposure is true and, by the
/// world's own rule, the contact does not know where the guard is.
fn false_exposure_ticks(r: &CorridorRun) -> Vec<u64> {
    r.exposure
        .iter()
        .zip(&r.truth)
        .filter(|(x, t)| x.now && !t.knows.contains(&CONTACT))
        .map(|(_, t)| t.tick)
        .collect()
}

#[test]
fn false_exposure_outlives_the_gaze_by_the_kernels_lifetime() {
    let r = run("false-exposure");
    let ticks = false_exposure_ticks(&r);
    assert!(!ticks.is_empty(), "at least one tick of false exposure");
    assert_eq!(ticks.len(), 90, "EXPOSURE_TTL");
    assert_eq!((ticks[0], ticks[ticks.len() - 1]), (22, 111));
    // The contact knows on the glance only; the belief expires at 112.
    let knows: Vec<u64> = r
        .truth
        .iter()
        .filter(|t| t.knows.contains(&CONTACT))
        .map(|t| t.tick)
        .collect();
    assert_eq!(knows, [21]);
    let expiry: Vec<u64> = r
        .exposure
        .iter()
        .zip(&r.mind)
        .filter(|(x, _)| x.cause == abi::EXPOSURE_CAUSE_EXPIRY)
        .map(|(_, m)| m.tick)
        .collect();
    assert_eq!(expiry, [112]);
    let control = run("false-exposure-control");
    assert_eq!(false_exposure_ticks(&control).len(), 0);
}

#[test]
fn the_contacts_knowledge_is_the_worlds_rule_not_the_kernels_belief() {
    for id in SCENARIOS.iter().chain(&CONTROLS) {
        let r = run(id);
        for t in &r.truth {
            let rule = t.observers.contains(&CONTACT) || t.attacked.contains(&CONTACT);
            assert_eq!(t.knows.contains(&CONTACT), rule, "{id}: tick {}", t.tick);
        }
    }
    // And the two differ somewhere: that is the effect.
    assert!(!false_exposure_ticks(&run("false-exposure")).is_empty());
}

/// Which own belief holds at each tick, from `valid_from` and `supersedes`,
/// read from the document. One holds at a time; none before the first.
fn own_holds_at(doc: &Value, tick: u64) -> Option<bool> {
    let own: Vec<&Value> = beliefs(doc).iter().filter(|b| !is_model(b)).collect();
    let live: Vec<&&Value> = own
        .iter()
        .filter(|b| b["valid_from"].as_u64().expect("a tick") <= tick)
        .filter(|b| {
            !own.iter().any(|s| {
                s["supersedes"] == b["belief_id"]
                    && s["valid_from"].as_u64().expect("a tick") <= tick
            })
        })
        .collect();
    assert!(
        live.len() <= 1,
        "tick {tick}: {} own beliefs hold",
        live.len()
    );
    live.first().map(|b| {
        matches!(
            b["status"].as_str(),
            Some("certain" | "believed" | "suspected")
        )
    })
}

#[test]
fn the_contacts_own_belief_holds_exactly_when_the_world_says_it_knows() {
    for id in SCENARIOS.iter().chain(&CONTROLS) {
        let r = run(id);
        let (doc, _) = docs(id);
        for t in &r.truth {
            assert_eq!(
                own_holds_at(&doc, t.tick).unwrap_or(false),
                t.knows.contains(&CONTACT),
                "{id}: tick {}",
                t.tick
            );
        }
        for b in beliefs(&doc).iter().filter(|b| !is_model(b)) {
            assert_eq!(b["bearer"], "contact-41");
            assert_eq!(b["proposition"], LOCATED);
        }
    }
}

#[test]
fn a_model_is_stated_where_the_kernel_first_claims_and_then_only_where_exposure_changes() {
    for id in SCENARIOS {
        let r = run(id);
        let (doc, _) = docs(id);
        let models: Vec<&Value> = beliefs(&doc).iter().filter(|b| is_model(b)).collect();
        let changes: Vec<u64> = r
            .exposure
            .iter()
            .zip(&r.mind)
            .filter(|(x, _)| x.cause != abi::EXPOSURE_CAUSE_NONE)
            .map(|(_, m)| m.tick)
            .collect();
        let ticks: Vec<u64> = models
            .iter()
            .map(|b| b["valid_from"].as_u64().expect("a tick"))
            .collect();
        let mut expected = vec![1];
        expected.extend(&changes);
        assert_eq!(ticks, expected, "{id}: never restated");
        for (n, b) in models.iter().enumerate() {
            assert_eq!(b["holders"], serde_json::json!(["guard-0"]));
            assert_eq!(b["bearer"], "contact-41");
            assert_eq!(b["proposition"], LOCATED, "{id}: the contact's own string");
            let exposed = r.exposure[ticks[n] as usize - 1].now;
            assert_eq!(
                b["status"],
                if exposed { "certain" } else { "rejected" },
                "{id}: belief {}",
                b["belief_id"]
            );
            match n {
                0 => assert!(b.get("supersedes").is_none()),
                _ => assert_eq!(b["supersedes"], models[n - 1]["belief_id"]),
            }
        }
    }
    for id in CONTROLS {
        let (doc, _) = docs(id);
        assert_eq!(
            beliefs(&doc).iter().filter(|b| is_model(b)).count(),
            0,
            "{id}: without the capability the kernel makes no claim"
        );
    }
}

#[test]
fn every_exposure_change_cites_the_event_that_caused_it() {
    for id in SCENARIOS {
        let r = run(id);
        let (doc, _) = docs(id);
        for b in beliefs(&doc).iter().filter(|b| is_model(b)) {
            let tick = b["valid_from"].as_u64().expect("a tick");
            let x = r.exposure[tick as usize - 1];
            let cited = event(&doc, &b["source_events"][0]);
            let p = cited["proposition"].as_str().expect("a proposition");
            match x.cause {
                abi::EXPOSURE_CAUSE_CUE => {
                    assert_eq!(cited["kind"], "observation");
                    assert_eq!(p, format!("contact-41 observes guard-0 at tick {tick}"));
                }
                abi::EXPOSURE_CAUSE_OWN_ATTACK => {
                    assert_eq!(cited["kind"], "action_outcome");
                    assert_eq!(p, format!("guard-0 attacked contact-41 at tick {tick}"));
                }
                // Expiry, and the first claim, cite the guard's perception on
                // the tick, as decay does (Enaction ADR-0027's recorded limit).
                abi::EXPOSURE_CAUSE_EXPIRY | abi::EXPOSURE_CAUSE_NONE => {
                    assert_eq!(cited["observer"], "guard-0");
                    assert_eq!(p, format!("contact-41 in room-0 at tick {tick}"));
                }
                other => panic!("{id}: cause {other}"),
            }
            assert_eq!(cited["tick"].as_u64(), Some(tick));
        }
    }
}

/// Whether the corridor's recorded truth makes one of the model run's
/// propositions true, decided here from the world model alone. A form this
/// test does not know fails it.
fn holds(p: &str, truth: &[TickTruth]) -> bool {
    if p == LOCATED {
        return truth.iter().all(|t| t.guard_room == 0);
    }
    let (body, tick) = p.rsplit_once(" at tick ").expect("a timed proposition");
    let tick: u64 = tick.parse().expect("a tick");
    let at = truth
        .iter()
        .find(|t| t.tick == tick)
        .expect("a recorded tick");
    if let Some((who, room)) = body.split_once(" in room-") {
        let id: u32 = who
            .strip_prefix("contact-")
            .expect("a contact")
            .parse()
            .expect("an id");
        return at.contacts.contains(&(id, room.parse().expect("a room")));
    }
    if body == "contact-41 observes guard-0" {
        return at.observers.contains(&CONTACT);
    }
    if body == "guard-0 attacked contact-41" {
        return at.attacked.contains(&CONTACT);
    }
    if body == "no contact visible to guard-0" {
        return !at.contacts.iter().any(|(_, room)| *room == at.guard_room);
    }
    panic!("a proposition form this test cannot decide: `{p}`");
}

/// CAC-KERNEL §9: world truth is closed over every stated proposition, models
/// included, and no atom stands for a model as such.
#[test]
fn world_truth_is_closed_over_every_stated_proposition_of_a_model_run() {
    for id in SCENARIOS.iter().chain(&CONTROLS) {
        let r = run(id);
        let (doc, _) = docs(id);
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
            .chain(pkg["beliefs"].as_array().expect("beliefs"))
            .map(|x| x["proposition"].as_str().expect("a string").to_owned())
            .collect();
        for p in &stated {
            assert_eq!(
                atoms.contains(p),
                holds(p, &r.truth),
                "{id}: `{p}` and world_truth disagree"
            );
        }
        for a in &atoms {
            assert!(stated.contains(a), "{id}: atom `{a}` is stated by nothing");
            assert!(
                !a.contains("knows"),
                "{id}: `{a}` is a truth atom for a model"
            );
        }
    }
}

#[test]
fn the_mind_trace_declares_stalk_and_caught_in_the_kernels_order() {
    for id in SCENARIOS.iter().chain(&CONTROLS) {
        let (_, mind) = docs(id);
        let v = &mind["vocabulary"];
        assert_eq!(
            v["candidates"],
            serde_json::json!([
                "advance",
                "engage",
                "evade",
                "reload",
                "investigate",
                "stalk"
            ])
        );
        assert_eq!(
            v["tells"],
            serde_json::json!(["startle", "hesitate", "fixate", "caught"])
        );
        let text = serde_json::to_string(&mind).expect("text");
        for word in ["score", "utility", "exposure_ttl", "reservoir"] {
            assert!(!text.contains(word), "{id}: `{word}` is in the mind trace");
        }
    }
}

#[test]
fn two_processes_write_identical_bytes_for_every_model_scenario() {
    let dir = std::env::temp_dir().join(format!("idaptik-weevil-model-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    for id in SCENARIOS.iter().chain(&CONTROLS) {
        let mut outputs = Vec::new();
        for n in ["a", "b"] {
            let epi = dir.join(format!("{id}-{n}-runner.json"));
            let mind = dir.join(format!("{id}-{n}-mind.json"));
            let status = Command::new(env!("CARGO_BIN_EXE_idaptik-weevil-replay"))
                .args(["--profile", id, "--ticks", "200", "--seed", "7"])
                .arg("--emit-epistemic")
                .arg(&epi)
                .arg("--emit-mind-trace")
                .arg(&mind)
                .status()
                .expect("the replay driver runs");
            assert!(status.success(), "{id}");
            outputs.push((
                std::fs::read(&epi).expect("runner output written"),
                std::fs::read(&mind).expect("mind trace written"),
            ));
        }
        assert_eq!(outputs[0], outputs[1], "{id}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_host_refuses_a_cue_from_a_contact_the_guard_does_not_perceive() {
    use idaptik_weevil::fx::to_fx;
    use idaptik_weevil::{Fx2, Observation, WeevilError, WeevilHost};
    let caps = abi::CAP_NAVMESH | abi::CAP_LINE_OF_SIGHT | abi::CAP_OBSERVATION_CUES;
    let mut host = WeevilHost::init(caps, 1, 4096, 7, 60, 1).expect("init");
    let obs = Observation {
        agent_id: 0,
        tick: 1,
        self_pos: Fx2 {
            x: to_fx(50.0),
            y: 0,
        },
        self_vel: Fx2::ZERO,
        health: abi::FX_ONE,
        ammo: 3 * abi::FX_ONE,
        capability_mask: caps,
        contacts: Vec::new(),
        cooldown: 0,
    };
    assert_eq!(
        host.tick_model(&obs, None, &[CONTACT]).unwrap_err(),
        WeevilError::InvalidTarget(CONTACT)
    );
    assert_eq!(host.agent(0).expect("slot").tick_last, 0, "nothing ran");
}

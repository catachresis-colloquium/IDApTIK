//! The reader's view of the slice 2 documents: each model's timeline and
//! whether it is stale-divergent, derived here from the rule text of
//! CAC-KERNEL section 9 ("Models are compared with minds").
//!
//! ADR-0009 forbids an Enaction dependency, so this file reads the contract,
//! not Enaction's source. The rules it follows, in the contract's words:
//!
//! * The compared belief of a model with `holders = [h1, ..., hn]` and
//!   bearer `b` has `holders = [h2, ..., hn]`, bearer `b` and the same
//!   proposition.
//! * A belief holds its proposition when its status is `certain`,
//!   `believed` or `suspected`.
//! * A model with such a status says the compared belief holds it; with
//!   `rejected`, that it does not; with `unknown`, it makes no claim.
//! * The verdict agrees when the compared belief holds the proposition and
//!   the model says it does, or when neither holds it. It diverges
//!   otherwise.
//! * A belief holds at tick `t` when its `valid_from` is at most `t` and no
//!   belief that names it in `supersedes` has a `valid_from` of at most `t`.
//!   At a tick where no belief of the compared kind holds, the bearer does
//!   not hold the proposition. More than one is undecidable.
//! * A model's lifetime runs from its `valid_from` to `until`, the earliest
//!   `valid_from` of a belief that supersedes it, or to the end of the
//!   package. Its timeline is the list of segments `{from_tick, verdict}`,
//!   with a new segment at each tick in the lifetime where the verdict
//!   changes.
//! * A model is stale-divergent when its first segment agrees and a later
//!   segment diverges.

use idaptik_weevil::mind_trace::documents;
use idaptik_weevil::profile::{MODEL_CONTACT_MEMORY, ProfileSet};
use idaptik_weevil::run_corridor;
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Agrees,
    Diverges,
    NoClaim,
}

#[derive(Debug, PartialEq, Eq)]
struct Timeline {
    belief_id: u64,
    segments: Vec<(u64, Verdict)>,
    until: Option<u64>,
    stale_divergent: bool,
}

struct B {
    id: u64,
    holders: Vec<String>,
    bearer: String,
    proposition: String,
    status: String,
    valid_from: u64,
    supersedes: Option<u64>,
}

fn read(doc: &Value) -> Vec<B> {
    doc["package"]["beliefs"]
        .as_array()
        .expect("beliefs")
        .iter()
        .map(|b| B {
            id: b["belief_id"].as_u64().expect("an id"),
            holders: b
                .get("holders")
                .map(|h| {
                    h.as_array()
                        .expect("holders")
                        .iter()
                        .map(|s| s.as_str().expect("a name").to_owned())
                        .collect()
                })
                .unwrap_or_default(),
            bearer: b["bearer"].as_str().expect("a bearer").to_owned(),
            proposition: b["proposition"].as_str().expect("a string").to_owned(),
            status: b["status"].as_str().expect("a status").to_owned(),
            valid_from: b["valid_from"].as_u64().expect("valid_from"),
            // A 2.0 link is `{belief, because}`. The reader needs the belief.
            supersedes: b
                .get("supersedes")
                .map(|s| s["belief"].as_u64().expect("an id")),
        })
        .collect()
}

fn holds_status(s: &str) -> bool {
    matches!(s, "certain" | "believed" | "suspected")
}

/// The earliest `valid_from` of a belief that supersedes `id`.
fn until(all: &[B], id: u64) -> Option<u64> {
    all.iter()
        .filter(|x| x.supersedes == Some(id))
        .map(|x| x.valid_from)
        .min()
}

fn holds_at(all: &[B], b: &B, t: u64) -> bool {
    b.valid_from <= t && until(all, b.id).is_none_or(|u| u > t)
}

fn timelines(doc: &Value) -> Vec<Timeline> {
    let all = read(doc);
    let mut out = Vec::new();
    for m in all.iter().filter(|b| !b.holders.is_empty()) {
        let end = until(&all, m.id);
        let claim = match m.status.as_str() {
            "unknown" => None,
            "rejected" => Some(false),
            s => Some(holds_status(s)),
        };
        let compared: Vec<&B> = all
            .iter()
            .filter(|x| {
                x.holders.as_slice() == &m.holders[1..]
                    && x.bearer == m.bearer
                    && x.proposition == m.proposition
            })
            .collect();
        let verdict = |t: u64| -> Verdict {
            let Some(claim) = claim else {
                return Verdict::NoClaim;
            };
            let holding: Vec<&&B> = compared.iter().filter(|x| holds_at(&all, x, t)).collect();
            assert!(
                holding.len() <= 1,
                "model {} is undecidable at tick {t}",
                m.id
            );
            let held = holding.first().is_some_and(|x| holds_status(&x.status));
            if held == claim {
                Verdict::Agrees
            } else {
                Verdict::Diverges
            }
        };
        // The verdict can change only where a compared belief starts or
        // stops holding: at a `valid_from` of one, or of its superseder.
        let mut ticks: BTreeSet<u64> = BTreeSet::from([m.valid_from]);
        for x in &compared {
            ticks.insert(x.valid_from);
            ticks.extend(until(&all, x.id));
        }
        let mut segments: Vec<(u64, Verdict)> = Vec::new();
        for t in ticks {
            if t < m.valid_from || end.is_some_and(|u| t >= u) {
                continue;
            }
            let v = verdict(t);
            if segments.last().map(|s| s.1) != Some(v) {
                segments.push((t, v));
            }
        }
        if end.is_some_and(|u| u <= m.valid_from) {
            segments = vec![(m.valid_from, verdict(m.valid_from))];
        }
        let stale_divergent = segments.first().map(|s| s.1) == Some(Verdict::Agrees)
            && segments[1..].iter().any(|s| s.1 == Verdict::Diverges);
        out.push(Timeline {
            belief_id: m.id,
            segments,
            until: end,
            stale_divergent,
        });
    }
    out
}

fn timelines_of(id: &str) -> Vec<Timeline> {
    let set = ProfileSet::canonical();
    let run = run_corridor(200, set.get(id).expect("known"), 7, true).expect("a model run");
    let (runner, _) = documents(&run).expect("both documents");
    let doc: Value =
        serde_json::from_slice(&runner.to_canonical_bytes().expect("bytes")).expect("JSON");
    let t = timelines(&doc);
    for m in &t {
        eprintln!("{id}: {m:?}");
    }
    t
}

use Verdict::{Agrees, Diverges};

#[test]
fn the_reader_rule_marks_a_restated_model_as_not_stale() {
    // The reader itself, on a hand-built package of the contract's own
    // `false-exposure` and `false-exposure-restated` shapes, so that this
    // check can fail: an unrestated model goes stale, a restated one does
    // not.
    let b = |id: u64, holders: &[&str], status: &str, from: u64, sup: Option<u64>| {
        let mut v = serde_json::json!({
            "belief_id": id, "bearer": "contact-21", "proposition": "p",
            "status": status, "valid_from": from,
        });
        if !holders.is_empty() {
            v["holders"] = serde_json::json!(holders);
        }
        if let Some(s) = sup {
            // The reader does not read the cause, and this package has no
            // events, so `because` names none.
            v["supersedes"] = serde_json::json!({ "belief": s, "because": 0 });
        }
        v
    };
    let doc = |beliefs: Vec<Value>| serde_json::json!({ "package": { "beliefs": beliefs } });
    let stale = timelines(&doc(vec![
        b(1, &[], "certain", 1, None),
        b(2, &["guard-0"], "certain", 2, None),
        b(3, &[], "unknown", 6, Some(1)),
    ]));
    assert_eq!(
        stale,
        [Timeline {
            belief_id: 2,
            segments: vec![(2, Agrees), (6, Diverges)],
            until: None,
            stale_divergent: true,
        }]
    );
    let restated = timelines(&doc(vec![
        b(1, &[], "certain", 1, None),
        b(2, &["guard-0"], "certain", 2, None),
        b(3, &[], "unknown", 6, Some(1)),
        b(4, &["guard-0"], "certain", 6, Some(2)),
    ]));
    assert!(restated.iter().all(|m| !m.stale_divergent));
    assert_eq!(restated[0].segments, [(2, Agrees)]);
    assert_eq!(restated[0].until, Some(6));
    assert_eq!(restated[1].segments, [(6, Diverges)]);
}

#[test]
fn caught_no_model_is_stale_divergent() {
    let t = timelines_of("caught");
    assert!(!t.is_empty(), "caught states models");
    let stale: Vec<u64> = t
        .iter()
        .filter(|m| m.stale_divergent)
        .map(|m| m.belief_id)
        .collect();
    assert_eq!(stale, Vec::<u64>::new(), "caught: no stale model");
}

#[test]
fn false_exposure_exactly_one_model_is_stale_and_it_agrees_through_the_contacts_memory() {
    let t = timelines_of("false-exposure");
    let stale: Vec<&Timeline> = t.iter().filter(|m| m.stale_divergent).collect();
    assert_eq!(stale.len(), 1, "false-exposure: exactly one stale model");
    // Stated at the cue (21); agrees through 21 + memory, the last tick the
    // contact remembers, and diverges from the tick after.
    let last_known = 21 + MODEL_CONTACT_MEMORY;
    assert_eq!(last_known, 51, "the stated memory of 30 ticks");
    assert_eq!(
        stale[0].segments,
        [(21, Agrees), (last_known + 1, Diverges)],
        "agrees on 21..={last_known}"
    );
    assert_eq!(
        stale[0].until,
        Some(112),
        "superseded when exposure expires"
    );
}

#[test]
fn the_controls_state_no_model() {
    for id in ["caught-control", "false-exposure-control"] {
        assert!(timelines_of(id).is_empty(), "{id}: no capability, no model");
    }
}

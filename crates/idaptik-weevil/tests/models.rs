//! Slice 2, "it models you", in the corridor: the world's rule for what a
//! contact knows, and the kernel's model of it, measured against each other.
//!
//! Everything a figure here counts comes from the corridor's own state
//! ([`idaptik_weevil::world::knows`] over the recorded [`TickTruth`]) and from
//! the kernel's model records. Nothing is derived from an Enaction document or
//! from a contract derivation: these are the facts the documents must later
//! show.
//!
//! Corridor ticks count from 1 and the kernel's fixtures from 0, so a corridor
//! tick is the fixture's tick plus one.

use idaptik_weevil::abi;
use idaptik_weevil::profile::{MODEL_CONTACT_MEMORY, ProfileSet};
use idaptik_weevil::world::knows;
use idaptik_weevil::{CorridorRun, TickTruth, run_corridor};

const CONTACT: u32 = 41;

fn run(id: &str) -> CorridorRun {
    let set = ProfileSet::canonical();
    run_corridor(200, set.get(id).expect("a known profile"), 7, true).expect("a model run")
}

fn first_engage(r: &CorridorRun) -> Option<u64> {
    r.trace
        .entries
        .iter()
        .find(|e| e.intent.action == "ATTACK")
        .map(|e| e.tick)
}

fn caught_ticks(r: &CorridorRun) -> Vec<u64> {
    r.mind
        .iter()
        .filter(|m| m.tells & abi::TELL_CAUGHT != 0)
        .map(|m| m.tick)
        .collect()
}

/// Ticks on which the kernel holds "the contact knows where I am" and the
/// world says the contact does not know.
fn exposed_but_unknown(r: &CorridorRun) -> Vec<u64> {
    r.model
        .iter()
        .zip(&r.truth)
        .filter(|(m, t)| m.exposure && !knows(t, CONTACT))
        .map(|(_, t)| t.tick)
        .collect()
}

fn known_ticks(truth: &[TickTruth]) -> Vec<u64> {
    truth
        .iter()
        .filter(|t| knows(t, CONTACT))
        .map(|t| t.tick)
        .collect()
}

#[test]
fn every_model_scenario_runs_through_the_model_record_and_every_change_has_a_cause() {
    for id in [
        "the-stalker",
        "the-stalker-control",
        "caught",
        "caught-control",
        "false-exposure",
        "false-exposure-control",
    ] {
        let r = run(id);
        assert_eq!(r.model.len(), 200, "{id}: one model record per tick");
        assert_eq!(r.trace.header.scenario_id, "headless-corridor-model");
        for m in &r.model {
            assert_eq!(m.changed(), m.cause != abi::EXPOSURE_CAUSE_NONE, "{id}");
        }
        let cues = ProfileSet::canonical()
            .get(id)
            .expect("known")
            .has_capability(abi::CAP_OBSERVATION_CUES);
        assert_eq!(cues, !id.ends_with("-control"), "{id}: the one difference");
        if !cues {
            assert!(r.model.iter().all(|m| !m.exposure), "{id}: no claim");
            assert!(caught_ticks(&r).is_empty(), "{id}: no CAUGHT");
            assert!(
                r.mind.iter().all(|m| !m.ranking.contains(&5)),
                "{id}: stalk is never ranked without the capability"
            );
        }
    }
}

#[test]
fn the_stalker_engages_only_inside_strike_range_and_its_control_engages_earlier() {
    let stalker = run("the-stalker");
    let control = run("the-stalker-control");
    // The kernel fixture: first engage at its tick 160, at range 0.5.
    assert_eq!(first_engage(&stalker), Some(161));
    let at = stalker
        .trace
        .entries
        .iter()
        .find(|e| e.intent.action == "ATTACK")
        .expect("an engagement");
    assert_eq!(at.observation.contacts[0].distance, abi::FX_ONE / 2);
    // Stalk on every tick before it.
    assert!(
        stalker.mind[..160].iter().all(|m| m.selected == 5),
        "the stalker stalks from its first tick to strike range"
    );
    // The kernel fixture's control engages at its tick 64, at range 1.0.
    assert_eq!(first_engage(&control), Some(65));
    assert!(first_engage(&control) < first_engage(&stalker));
    // The contact never observes the guard; it learns where the guard is only
    // from the blow.
    assert!(stalker.truth.iter().all(|t| t.observers.is_empty()));
    assert_eq!(known_ticks(&stalker.truth).first(), Some(&161));
}

#[test]
fn caught_shows_exactly_one_caught_tell_on_the_observation_tick() {
    let caught = run("caught");
    assert_eq!(caught_ticks(&caught), [101]);
    let cue = &caught.model[100];
    assert_eq!(cue.cause, abi::EXPOSURE_CAUSE_CUE);
    assert_eq!(u32::from(cue.contact), CONTACT);
    assert!(
        caught.mind[101..].iter().all(|m| m.selected != 5),
        "once caught the guard never stalks again"
    );
    // The kernel fixture: engages at its tick 115.
    assert_eq!(first_engage(&caught), Some(116));
    assert!(caught_ticks(&run("caught-control")).is_empty());
}

#[test]
fn false_exposure_the_guard_thinks_it_is_known_while_the_contact_does_not_know() {
    let fe = run("false-exposure");
    // The contact observes the guard once, at 21, and remembers for 30
    // ticks: it knows from 21 to 51 and not after.
    assert_eq!(
        fe.truth.iter().filter(|t| !t.observers.is_empty()).count(),
        1
    );
    assert_eq!(known_ticks(&fe.truth), (21..=51).collect::<Vec<u64>>());
    // The contact's memory ends well before the kernel's belief does: the
    // kernel holds exposure for 90 ticks after the cue and lets it go at 112.
    let unknown = exposed_but_unknown(&fe);
    assert!(!unknown.is_empty());
    assert_eq!(unknown, (52..=111).collect::<Vec<u64>>(), "60 ticks");
    let expiry: Vec<u64> = fe
        .model
        .iter()
        .zip(&fe.truth)
        .filter(|(m, _)| m.cause == abi::EXPOSURE_CAUSE_EXPIRY)
        .map(|(_, t)| t.tick)
        .collect();
    assert_eq!(expiry, [112]);
    // The glance came while the guard stalked: it is a CAUGHT as well.
    assert_eq!(caught_ticks(&fe), [21]);

    let control = run("false-exposure-control");
    assert_eq!(
        known_ticks(&control.truth),
        known_ticks(&fe.truth),
        "the same world"
    );
    assert_eq!(exposed_but_unknown(&control).len(), 0);
}

/// The ticks on which the world's rule changes its answer for the contact,
/// with the new answer: the ticks on which the contact's own belief must be
/// restated.
fn knowledge_changes(truth: &[TickTruth]) -> Vec<(u64, bool)> {
    let mut before = false;
    let mut out = Vec::new();
    for t in truth {
        let now = knows(t, CONTACT);
        if now != before {
            out.push((t.tick, now));
        }
        before = now;
    }
    out
}

#[test]
fn false_exposure_the_contact_forgets_on_the_first_tick_after_its_memory() {
    // Known from the glance at 21, forgotten on 21 + 30 + 1 = 52, not on 22.
    assert_eq!(
        knowledge_changes(&run("false-exposure").truth),
        [(21, true), (52, false)]
    );
}

#[test]
fn every_model_scenario_gives_its_contact_one_stated_memory() {
    // One value for all six: the three scenarios and their controls.
    assert_eq!(MODEL_CONTACT_MEMORY, 30);
    let set = ProfileSet::canonical();
    for id in [
        "the-stalker",
        "the-stalker-control",
        "caught",
        "caught-control",
        "false-exposure",
        "false-exposure-control",
    ] {
        let script = set
            .get(id)
            .expect("known")
            .host
            .script
            .clone()
            .expect("a cast");
        assert_eq!(script.contacts.len(), 1, "{id}");
        assert_eq!(script.contacts[0].memory, MODEL_CONTACT_MEMORY, "{id}");
    }
}

#[test]
fn caught_the_contact_knows_on_every_tick_the_kernel_is_exposed() {
    let caught = run("caught");
    // The glance at 101 and the blows from 116: the contact's memory spans
    // the 15 ticks between them, so the guard is never exposed to a contact
    // that does not know.
    assert_eq!(caught.truth[100].observers, [CONTACT]);
    assert_eq!(first_engage(&caught), Some(116));
    assert_eq!(exposed_but_unknown(&caught), Vec::<u64>::new());
    let exposed = caught.model.iter().filter(|m| m.exposure).count();
    assert!(exposed > 0, "the kernel is exposed after the cue");
    assert_eq!(knowledge_changes(&caught.truth)[0], (101, true));
}

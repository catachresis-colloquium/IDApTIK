//! The headless reproducibility gate for the Evil Weevil adapter.
//!
//! This test links the native kernel, so it only runs where the kernel library
//! has been built. It checks the properties the first milestone requires of a
//! replay: two runs under the same supported configuration are bit-identical, a
//! different seed is a different decision sequence, and the kernel's documented
//! memory behaviour — investigate while memory is fresh, fall back once it
//! expires — actually happens.

use idaptik_weevil::profile::ProfileSet;
use idaptik_weevil::replay::{decision_identity, replay_equal};
use idaptik_weevil::run_trace;

#[test]
fn two_runs_under_the_same_configuration_are_bit_identical() {
    let canonical = ProfileSet::canonical();
    let profile = &canonical.aggressive_pursuer;
    let a = run_trace(200, profile, 7).expect("a trace runs");
    let b = run_trace(200, profile, 7).expect("a trace runs");
    replay_equal(&a, &b).expect("the same seed and profile replay identically");
    assert_eq!(
        decision_identity(&a),
        decision_identity(&b),
        "the decision-cache identity is stable across identical runs"
    );
}

#[test]
fn a_different_seed_is_a_different_decision_sequence() {
    let profile = &ProfileSet::canonical().cautious_sentry;
    let a = run_trace(120, profile, 1).expect("a trace runs");
    let b = run_trace(120, profile, 2).expect("a trace runs");
    assert!(
        replay_equal(&a, &b).is_err(),
        "a seed change must be reported as a configuration difference, not ignored"
    );
    assert_ne!(decision_identity(&a), decision_identity(&b));
}

#[test]
fn the_kernel_remembers_a_bearing_then_falls_back_when_memory_expires() {
    let profile = &ProfileSet::canonical().cautious_sentry;
    let trace = run_trace(120, profile, 5).expect("a trace runs");

    let investigate = trace
        .entries
        .iter()
        .filter(|e| e.intent.action == "INVESTIGATE")
        .count();
    let advance = trace
        .entries
        .iter()
        .filter(|e| e.intent.action == "ADVANCE")
        .count();
    let from_memory = trace
        .entries
        .iter()
        .filter(|e| e.intent.from_memory)
        .count();

    assert!(investigate > 0, "the guard investigated from memory");
    assert!(
        from_memory > 0,
        "and the decision was flagged as from-memory"
    );
    assert!(
        advance > 0,
        "and fell back to advancing once the memory aged out"
    );
}

#[test]
fn a_guard_pressed_against_a_closed_door_does_not_pass_through_it() {
    let profile = &ProfileSet::canonical().cautious_sentry;
    let trace = run_trace(240, profile, 11).expect("a trace runs");
    let entries = &trace.entries;
    let mut blocked = 0usize;
    for pair in entries.windows(2) {
        if pair[0].applied.blocked {
            blocked += 1;
            let here = pair[0].observation.self_pos;
            let next = pair[1].observation.self_pos;
            assert_eq!(
                here, next,
                "a tick the host reports as blocked must not move the guard through the door"
            );
        }
    }
    assert!(
        blocked > 0,
        "the corridor has closed doors, so some ticks are blocked"
    );
}

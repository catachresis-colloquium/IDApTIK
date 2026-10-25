//! Whole-corpus guarantees: every fixed scenario passes, every required
//! spec field is present, the strict limits hold, and the defensive
//! observer's false-positive/false-negative counts are zero across the run.

use idaptik_evil_weevil::corpus;
use idaptik_evil_weevil::policy::Permission;
use idaptik_evil_weevil::report::CorpusReport;
use idaptik_evil_weevil::scenarios::{EvidenceStatus, TopologyMode};

#[test]
fn every_corpus_scenario_passes_and_holds_its_invariant() {
    let results: Vec<_> = corpus().iter().map(|s| s.run()).collect();
    assert_eq!(
        results.len(),
        6,
        "the bounded corpus is the five tasked scenarios plus EW-06"
    );
    for result in &results {
        assert!(
            result.passed,
            "scenario {} failed:\n{}",
            result.id,
            result
                .checks
                .iter()
                .filter(|c| !c.passed)
                .map(|c| format!("  - {}: {}", c.name, c.detail))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let failed_checks: Vec<_> = result.checks.iter().filter(|c| !c.passed).collect();
        assert!(failed_checks.is_empty(), "{failed_checks:?}");
        // The replay artifact carries observable evidence for every run,
        // and every recorded intent reached the artifact exactly once.
        assert!(!result.telemetry.is_empty(), "{}", result.id);
        assert!(!result.effects.is_empty(), "{}", result.id);
        assert_eq!(
            result.intents.len(),
            result.metrics.intents,
            "{}",
            result.id
        );
    }
}

#[test]
fn every_scenario_declares_the_full_evidence_contract() {
    for scenario in corpus() {
        let spec = scenario.spec();
        assert!(!spec.id.is_empty());
        assert!(!spec.title.is_empty());
        assert!(!spec.invariant.is_empty());
        assert!(!spec.expectation.is_empty());
        assert!(
            !spec.bounded_inputs.is_empty(),
            "{} needs a bounded input sequence",
            spec.id
        );
        assert!(!spec.replay_artifact.is_empty());
        assert!(
            spec.principal.starts_with("ew-dummy-"),
            "dummy identities only"
        );
        // The adversary's only grant across the whole corpus.
        assert_eq!(
            spec.permitted_operations,
            vec![Permission::ProposeBoundedIntent]
        );
        // The offline corpus models the one implemented topology, without
        // game-server-admin; everything else is a documented gap, not a
        // simulated mode.
        assert_eq!(spec.topology, TopologyMode::SingleRendezvousRelay);
        assert!(!spec.game_server_admin_present);
        assert_eq!(spec.evidence, EvidenceStatus::Demonstrated);
    }
}

#[test]
fn the_corpus_stays_inside_strict_limits() {
    let results: Vec<_> = corpus().iter().map(|s| s.run()).collect();
    for result in &results {
        assert!(
            result.metrics.within_limits(),
            "{} crossed a bounded-test limit: {:?}",
            result.id,
            result.metrics
        );
        // No availability testing: the fixed, hand-written sequences stay
        // small by design, well below the hard caps in limits.rs.
        assert!(result.metrics.intents <= 8, "{}", result.id);
        assert!(result.metrics.peak_queue_depth < 32, "{}", result.id);
    }
}

#[test]
fn the_observer_has_zero_false_positives_and_zero_false_negatives() {
    let results: Vec<_> = corpus().iter().map(|s| s.run()).collect();
    let report = CorpusReport::new(777_001, results);
    assert_eq!(
        report.totals.false_positives, 0,
        "a fault was called hostile"
    );
    assert_eq!(
        report.totals.false_negatives, 0,
        "a real violation was missed"
    );
    assert!(
        report.totals.transport_faults > 0,
        "scenario 5 must inject faults"
    );
    assert!(
        report.totals.protocol_violations > 0,
        "the corpus must show real rejections"
    );
}

#[test]
fn the_redacted_artifact_carries_no_secrets_and_keeps_dummy_evidence() {
    let results: Vec<_> = corpus().iter().map(|s| s.run()).collect();
    let report = CorpusReport::new(777_001, results);
    let rendered = serde_json::to_string_pretty(&report).unwrap();
    let lowered = rendered.to_ascii_lowercase();
    for banned in ["token\":", "password\":", "bearer ", "ghp_", "secret\":"] {
        assert!(!lowered.contains(banned), "artifact leaked {banned}");
    }
    assert!(rendered.contains("ew-dummy-"));
    assert!(rendered.contains("idaptik-evil-weevil-corpus-v1"));
    // Deterministic run id: the wall clock never enters the artifact.
    assert_eq!(report.run_id, "ew-corpus-v2-seed-777001");
}

#[test]
fn the_corpus_is_deterministic_across_runs() {
    let first: Vec<_> = corpus().iter().map(|s| s.run()).collect();
    let second: Vec<_> = corpus().iter().map(|s| s.run()).collect();
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(&second).unwrap(),
        "two runs of the fixed corpus must be byte-identical"
    );
}

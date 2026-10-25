//! The bounded adversarial corpus.
//!
//! Every scenario is a fixed, deterministic script over the shared agent
//! interface — the "start simple" slice of ADR-0013. Each one declares the
//! five things the task requires: its principal and permitted operations,
//! the protocol invariant under test, its exact bounded input sequence, the
//! expected rejection/acceptance/recovery, and the observable result and
//! replay artefact.

pub mod epochs;
pub mod infra_orders;
pub mod observations;
pub mod reconnect;
pub mod stale;
pub mod targets;

use crate::agent::Intent;
use crate::harness::{IsolatedHarness, ObservedEffect, ScenarioCheck};
use crate::limits::HarnessMetrics;
use crate::observer::{DefensiveObserver, DefensiveReport};
use crate::policy::Permission;
use crate::telemetry::TelemetryEvent;
use serde::{Deserialize, Serialize};

/// Honesty labels for what a scenario actually demonstrates. The runbook
/// separates these three categories and never upgrades one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceStatus {
    /// Asserted against the real production validators in this repository.
    Demonstrated,
    /// A production mode that is not implemented; recorded, not simulated.
    Gap,
    /// A plausible future finding not exercised by the current corpus.
    Hypothesis,
}

/// The deployment topologies the task asks the corpus to reason about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TopologyMode {
    /// Direct peer-to-peer game transport.
    P2p,
    /// Multiple federated servers sharing sessions.
    Federated,
    /// One rendezvous relay that holds no gameplay authority (the
    /// implemented burble `game:` lane, modelled offline here).
    SingleRendezvousRelay,
    /// A server running the authoritative simulation (ADR-0005 option 1).
    SingleAuthoritativeServer,
}

/// One entry in the bounded input sequence, as declared up front rather
/// than discovered at runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputSpec {
    pub step: u64,
    pub intent: String,
    pub expected: String,
}

/// The static specification of a scenario — its evidence contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenarioSpec {
    pub id: &'static str,
    pub title: &'static str,
    pub principal: String,
    pub permitted_operations: Vec<Permission>,
    pub invariant: &'static str,
    pub bounded_inputs: Vec<InputSpec>,
    pub expectation: &'static str,
    pub topology: TopologyMode,
    pub game_server_admin_present: bool,
    pub evidence: EvidenceStatus,
    pub replay_artifact: &'static str,
}

/// The serialisable result of running one scenario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScenarioResult {
    pub id: String,
    pub title: String,
    pub invariant: String,
    pub expectation: String,
    pub principal: String,
    pub permitted_operations: Vec<Permission>,
    pub evidence: EvidenceStatus,
    pub topology: TopologyMode,
    pub game_server_admin_present: bool,
    pub passed: bool,
    pub checks: Vec<ScenarioCheck>,
    /// The exact adversarial intents proposed, with raw wire payloads — the
    /// input side of the replay artifact.
    pub intents: Vec<Intent>,
    pub effects: Vec<ObservedEffect>,
    pub telemetry: Vec<TelemetryEvent>,
    pub observer: DefensiveReport,
    pub metrics: HarnessMetrics,
    pub replay_artifact: String,
    pub notes: Vec<String>,
}

/// A corpus scenario: a static spec plus its deterministic run.
pub trait Scenario {
    fn spec(&self) -> ScenarioSpec;
    fn run(&self) -> ScenarioResult;
}

/// Assemble a result from a harness once a scenario body has finished. The
/// pass condition is conjunctive: every named check passed, the strict
/// resource limits held, and the observer produced no false positives.
pub fn finalize(
    spec: ScenarioSpec,
    harness: &mut IsolatedHarness,
    notes: Vec<String>,
) -> ScenarioResult {
    harness.enforce_limits();
    let observer = DefensiveObserver::evaluate(harness.telemetry());
    let false_positives =
        DefensiveObserver::false_positives_on_faults(harness.telemetry(), &observer);
    if false_positives > 0 {
        harness.check(
            "observer:no-false-positives",
            false,
            format!("{false_positives} fault(s) misclassified as hostile"),
        );
    } else {
        harness.check(
            "observer:no-false-positives",
            true,
            "no transport fault was classified as a violation".to_owned(),
        );
    }
    let limits_ok = harness.metrics().within_limits();
    let checks_ok = harness.checks().iter().all(|c| c.passed);
    let passed = limits_ok && checks_ok;

    ScenarioResult {
        id: spec.id.to_owned(),
        title: spec.title.to_owned(),
        invariant: spec.invariant.to_owned(),
        expectation: spec.expectation.to_owned(),
        principal: spec.principal,
        permitted_operations: spec.permitted_operations,
        evidence: spec.evidence,
        topology: spec.topology,
        game_server_admin_present: spec.game_server_admin_present,
        passed,
        checks: harness.checks().to_vec(),
        intents: harness.intents().to_vec(),
        effects: harness.effects().to_vec(),
        telemetry: harness.telemetry().to_vec(),
        observer,
        metrics: harness.metrics(),
        replay_artifact: spec.replay_artifact.to_owned(),
        notes,
    }
}

/// The corpus: the five scenarios the task named, then the
/// infrastructure-orders scenario ADR-0014 added.
pub fn corpus() -> Vec<Box<dyn Scenario>> {
    vec![
        Box::new(stale::StaleDuplicateScenario),
        Box::new(targets::InvalidTargetScenario),
        Box::new(epochs::EpochConflictScenario),
        Box::new(reconnect::ReconnectObsoleteScenario),
        Box::new(observations::WithheldObservationScenario),
        Box::new(infra_orders::InfraOrdersScenario),
    ]
}

/// The corpus version, bumped when any scenario's input sequence changes so
/// replay traces stay interpretable.
pub const CORPUS_VERSION: u32 = 2;

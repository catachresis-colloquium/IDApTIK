//! The read-only defensive observer.
//!
//! It consumes validated telemetry ([`crate::telemetry::TelemetryEvent`]),
//! classifies evidence, and recommends an action. It holds no sender, no
//! session handle and no enforcement power: every method takes `&self` or a
//! slice reference and returns data. Enforcement lives exclusively in
//! [`crate::enforce`], behind an explicit host/operator policy.
//!
//! Two rules are non-negotiable and tested:
//!
//! 1. **Faults are never hostile.** Delay, loss, silence, reconnect
//!    duplicates and divergence evidence classify as benign/fault, never as a
//!    violation — there is no intent evidence in a dropped packet.
//! 2. **Recommendations never retaliate.** The strongest recommendation is
//!    [`Recommendation::FlagForOperatorReview`]; there is no ban, disconnect
//!    or remote-action variant, and local protective decisions never assume
//!    universal ban authority.

use crate::telemetry::{FaultKind, TelemetryEvent};
use serde::{Deserialize, Serialize};

/// Evidence classes, ordered from least to most severe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceClass {
    /// Normal accepted traffic or a host-coped denial/no-op.
    Benign,
    /// A transport fault with no intent attribution.
    Fault,
    /// A host-validated protocol violation.
    Violation,
}

/// Non-binding recommendations to the host operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Recommendation {
    /// No action warranted.
    None,
    /// Write to the local trace only.
    LogOnly,
    /// Offer the peer the existing resync path (recovery, not punishment).
    RequestResync,
    /// Ask a human operator to review the evidence. Strongest option.
    FlagForOperatorReview,
}

/// One deterministic classification. Confidence is basis points (`0..=1000`)
/// and rule-derived, so replay is exact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Classification {
    pub index: usize,
    pub class: EvidenceClass,
    pub recommendation: Recommendation,
    pub confidence_bp: u16,
    pub label: String,
}

/// Aggregate counts for the false-positive/false-negative measurement.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassCounts {
    pub benign: usize,
    pub fault: usize,
    pub violation: usize,
}

/// The observer's report over one trace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefensiveReport {
    pub counts: ClassCounts,
    pub classifications: Vec<Classification>,
    /// Violations flagged (event indices), for operator review.
    pub flagged: Vec<usize>,
    /// Fault events (event indices) explicitly *not* treated as hostile.
    pub faults: Vec<usize>,
}

/// The stateless observer. Constructing it requires no capability beyond
/// read access; the policy still grants that separately.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefensiveObserver;

/// Rule confidence: these are deterministic rules over host-validated
/// evidence, not probabilistic guesses.
const RULE_CONFIDENCE_BP: u16 = 950;
/// Recovery recommendations after a fault-induced loss are slightly less
/// certain — the fault might recur — but still rule-derived.
const RECOVERY_CONFIDENCE_BP: u16 = 800;

impl DefensiveObserver {
    pub fn new() -> Self {
        Self
    }

    /// Classify one validated event. This is the whole rule set; it has no
    /// hidden state and no I/O.
    pub fn classify(index: usize, event: &TelemetryEvent) -> Classification {
        match event {
            TelemetryEvent::Accepted { .. } | TelemetryEvent::SimDenied { .. }
            | TelemetryEvent::SimNoOp { .. } => Classification {
                index,
                class: EvidenceClass::Benign,
                recommendation: Recommendation::None,
                confidence_bp: RULE_CONFIDENCE_BP,
                label: "host-coped".into(),
            },
            TelemetryEvent::RelayDropped { reason } => {
                // The relay drops reconnect duplicates and stale seq frames
                // as ordinary network facts. Record, never escalate.
                Classification {
                    index,
                    class: EvidenceClass::Fault,
                    recommendation: Recommendation::LogOnly,
                    confidence_bp: RULE_CONFIDENCE_BP,
                    label: format!("relay-drop:{reason}"),
                }
            }
            TelemetryEvent::Fault { kind, .. } => {
                let recommendation = match kind {
                    // A withheld watermark or closed transport may need the
                    // existing resync path; delay and silence only get logged.
                    FaultKind::WithheldWatermark | FaultKind::TransportClosed => {
                        Recommendation::RequestResync
                    }
                    FaultKind::DelayedDelivery
                    | FaultKind::SilenceGraceExpired
                    | FaultKind::ReconnectDuplicate => Recommendation::LogOnly,
                };
                Classification {
                    index,
                    class: EvidenceClass::Fault,
                    recommendation,
                    confidence_bp: RECOVERY_CONFIDENCE_BP,
                    label: format!("fault:{kind:?}"),
                }
            }
            TelemetryEvent::Recovered { .. } => Classification {
                index,
                class: EvidenceClass::Benign,
                recommendation: Recommendation::None,
                confidence_bp: RULE_CONFIDENCE_BP,
                label: "recovered".into(),
            },
            TelemetryEvent::ValidatorRejected { violation, .. } => Classification {
                index,
                class: EvidenceClass::Violation,
                // The ceiling of the observer's power: ask a human. It never
                // ends a session itself and cannot reach a remote machine.
                recommendation: Recommendation::FlagForOperatorReview,
                confidence_bp: RULE_CONFIDENCE_BP,
                label: format!("violation:{violation:?}"),
            },
        }
    }

    /// Classify a full trace and tally it.
    pub fn evaluate(events: &[TelemetryEvent]) -> DefensiveReport {
        let mut counts = ClassCounts::default();
        let mut flagged = Vec::new();
        let mut faults = Vec::new();
        let mut classifications = Vec::with_capacity(events.len());
        for (index, event) in events.iter().enumerate() {
            let c = Self::classify(index, event);
            match c.class {
                EvidenceClass::Benign => counts.benign += 1,
                EvidenceClass::Fault => {
                    counts.fault += 1;
                    faults.push(index);
                }
                EvidenceClass::Violation => {
                    counts.violation += 1;
                    flagged.push(index);
                }
            }
            classifications.push(c);
        }
        DefensiveReport {
            counts,
            classifications,
            flagged,
            faults,
        }
    }

    /// False positives on a fault-only trace: violations reported where no
    /// violation exists. Must always be zero.
    pub fn false_positives_on_faults(report: &DefensiveReport) -> usize {
        report
            .classifications
            .iter()
            .filter(|c| c.class == EvidenceClass::Violation)
            .count()
    }

    /// False negatives on a trace known to contain `expected_violations`
    /// validator rejections: violations the observer failed to flag.
    pub fn false_negatives(
        events: &[TelemetryEvent],
        report: &DefensiveReport,
    ) -> usize {
        let known = events.iter().filter(|e| e.is_violation()).count();
        known.saturating_sub(report.flagged.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::ViolationKind;

    fn fault_trace() -> Vec<TelemetryEvent> {
        vec![
            TelemetryEvent::Fault {
                kind: FaultKind::DelayedDelivery,
                detail: "2 turns".into(),
            },
            TelemetryEvent::Fault {
                kind: FaultKind::WithheldWatermark,
                detail: "commit held".into(),
            },
            TelemetryEvent::Fault {
                kind: FaultKind::SilenceGraceExpired,
                detail: "grace".into(),
            },
            TelemetryEvent::RelayDropped {
                reason: "stale_or_duplicate".into(),
            },
        ]
    }

    #[test]
    fn faults_never_classify_as_hostile() {
        let trace = fault_trace();
        let report = DefensiveObserver::evaluate(&trace);
        assert_eq!(report.counts.violation, 0);
        assert_eq!(DefensiveObserver::false_positives_on_faults(&report), 0);
        assert_eq!(report.faults.len(), 4);
    }

    #[test]
    fn violations_are_flagged_without_missed_detections() {
        let mut trace = fault_trace();
        trace.push(TelemetryEvent::ValidatorRejected {
            violation: ViolationKind::StaleAfterCommit,
            detail: "after watermark".into(),
        });
        let report = DefensiveObserver::evaluate(&trace);
        assert_eq!(report.counts.violation, 1);
        assert_eq!(DefensiveObserver::false_negatives(&trace, &report), 0);
        assert_eq!(
            report.classifications[report.flagged[0]].recommendation,
            Recommendation::FlagForOperatorReview
        );
    }

    #[test]
    fn no_recommendation_can_retaliate() {
        // The enum has no ban/disconnect/remote variant; assert the strongest
        // available recommendation is operator review.
        let trace = vec![TelemetryEvent::ValidatorRejected {
            violation: ViolationKind::RoleForbidden,
            detail: "seat mismatch".into(),
        }];
        let report = DefensiveObserver::evaluate(&trace);
        for c in &report.classifications {
            assert_ne!(c.recommendation, Recommendation::None);
        }
        assert_eq!(report.flagged.len(), 1);
    }

    #[test]
    fn host_coped_denials_stay_benign() {
        let trace = vec![
            TelemetryEvent::SimDenied {
                command: "Uplink".into(),
                reason: "NoRoute".into(),
            },
            TelemetryEvent::SimNoOp {
                command: "NetSsh".into(),
            },
        ];
        let report = DefensiveObserver::evaluate(&trace);
        assert_eq!(report.counts.benign, 2);
        assert_eq!(report.counts.violation, 0);
    }
}

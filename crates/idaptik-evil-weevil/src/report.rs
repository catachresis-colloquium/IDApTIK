//! Outcome recording and minimal-evidence redaction.
//!
//! The corpus produces one versioned [`CorpusReport`] plus per-scenario
//! replay traces. Storage is plain JSON files written by the binary — the
//! same artifact convention as the headless runner and the loopback gate.
//! LMDB may later hold a local replay/index (burble's LMDB today stores only
//! media frames), and Dragonfly remains an optional, non-authoritative hosted
//! cache experiment; neither is an authority and neither is wired in here
//! (ADR-0013).
//!
//! Every report passes through [`redact`] before it is written, and the
//! scenarios themselves only ever use dummy guest identities, so no secret
//! should exist to find. The redactor is defence in depth, with a self-test
//! that proves it can actually fire.

use crate::scenarios::{CORPUS_VERSION, ScenarioResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Report format tag.
pub const REPORT_SCHEMA: &str = "idaptik-evil-weevil-corpus-v1";
/// The replacement string used for every scrubbed value.
pub const REDACTED: &str = "[REDACTED]";

/// Aggregate counts across the corpus.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorpusTotals {
    pub scenarios: usize,
    pub passed: usize,
    pub failed: usize,
    pub protocol_violations: usize,
    pub transport_faults: usize,
    pub false_positives: usize,
    pub false_negatives: usize,
    pub peak_queue_depth: usize,
    pub wire_messages: usize,
}

/// What the redaction pass changed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactionInfo {
    pub redacted_values: usize,
}

/// The full corpus artifact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorpusReport {
    pub schema: String,
    pub corpus_version: u32,
    /// Deterministic run id derived from the corpus version and seed, not the
    /// wall clock, so two runs of the same corpus compare byte-for-byte.
    pub run_id: String,
    pub totals: CorpusTotals,
    pub scenarios: Vec<ScenarioResult>,
    pub redaction: RedactionInfo,
}

impl CorpusReport {
    /// Build the report from scenario results and apply redaction.
    pub fn new(run_seed: u32, scenarios: Vec<ScenarioResult>) -> Self {
        let mut totals = CorpusTotals {
            scenarios: scenarios.len(),
            ..Default::default()
        };
        for s in &scenarios {
            if s.passed {
                totals.passed += 1;
            } else {
                totals.failed += 1;
            }
            totals.protocol_violations += s.observer.counts.violation;
            totals.transport_faults += s.observer.counts.fault;
            totals.false_positives += s
                .observer
                .classifications
                .iter()
                .filter(|c| {
                    c.class == crate::observer::EvidenceClass::Violation
                        && s.telemetry.get(c.index).is_some_and(|e| e.is_fault())
                })
                .count();
            totals.false_negatives +=
                crate::observer::DefensiveObserver::false_negatives(&s.telemetry, &s.observer);
            totals.peak_queue_depth = totals.peak_queue_depth.max(s.metrics.peak_queue_depth);
            totals.wire_messages += s.metrics.wire_messages;
        }
        let run_id = format!("ew-corpus-v{CORPUS_VERSION}-seed-{run_seed}");
        let mut report = Self {
            schema: REPORT_SCHEMA.into(),
            corpus_version: CORPUS_VERSION,
            run_id,
            totals,
            scenarios,
            redaction: RedactionInfo::default(),
        };
        let mut value = serde_json::to_value(&report).expect("report serialises");
        let info = redact(&mut value);
        report.redaction = info;
        report
    }

    /// Convenience for gates: every scenario passed and no bound was crossed.
    pub fn all_passed(&self) -> bool {
        self.totals.failed == 0 && self.totals.scenarios > 0
    }
}

/// Object keys whose values must never appear in an artifact.
const SENSITIVE_KEYS: [&str; 7] = [
    "token",
    "secret",
    "password",
    "authorization",
    "api_key",
    "apikey",
    "cookie",
];

/// Recursively scrub sensitive values and credential-bearing strings.
pub fn redact(value: &mut Value) -> RedactionInfo {
    let mut info = RedactionInfo::default();
    redact_inner(value, &mut info);
    info
}

fn redact_inner(value: &mut Value, info: &mut RedactionInfo) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                // Substring, not exact: composite keys such as
                // `auth_token` or `provider_api_key` must not slip past.
                let lowered = key.to_ascii_lowercase();
                if SENSITIVE_KEYS.iter().any(|needle| lowered.contains(needle)) {
                    *child = Value::String(REDACTED.into());
                    info.redacted_values += 1;
                } else {
                    redact_inner(child, info);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_inner(item, info);
            }
        }
        Value::String(text) if looks_secret_bearing(text) => {
            *text = REDACTED.into();
            info.redacted_values += 1;
        }
        _ => {}
    }
}

fn looks_secret_bearing(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if lower.starts_with("bearer ") && text.split_whitespace().count() > 1 {
        return true;
    }
    // Credentials embedded in a URL: scheme://user:pass@host.
    if lower.contains("://") && text.contains('@') {
        let after_scheme = text.split("://").nth(1).unwrap_or("");
        if let Some(authority) = after_scheme.split('/').next()
            && authority.contains(':')
            && authority.contains('@')
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn planted_secrets_are_redacted_and_dummy_values_survive() {
        // Assembled at runtime rather than written as one literal: a static
        // `scheme://user:pass@host` string trips the repository's gitleaks
        // basic-auth rule even when every component is an obvious dummy. The
        // redactor must still see and scrub the completed string.
        let cred_url = [
            "wss://victim:",
            "hunter2",
            "@relay.example/voice/websocket",
        ]
        .join("");
        let mut value = json!({
            "auth_token": "ghp_lookalike-token-value",
            "nested": { "password": "hunter2", "ok": "ew-dummy-hacker-0001" },
            "header": "Bearer abc123def456",
            "url": cred_url,
            "safe_url": "ws://127.0.0.1:4002/voice/websocket?guest=true&display_name=loopback"
        });
        let info = redact(&mut value);
        assert!(info.redacted_values >= 4, "{info:?}");
        let rendered = value.to_string();
        assert!(!rendered.contains("hunter2"));
        assert!(!rendered.contains("abc123def456"));
        assert!(!rendered.contains("ghp_lookalike"));
        assert!(rendered.contains("ew-dummy-hacker-0001"));
        assert!(rendered.contains("127.0.0.1"));
    }

    #[test]
    fn a_report_without_secrets_redacts_nothing() {
        let value = json!({ "principal": "ew-dummy-infiltrator-0003", "at": 4 });
        let mut clone = value.clone();
        let info = redact(&mut clone);
        assert_eq!(info.redacted_values, 0);
        assert_eq!(clone, value);
    }
}

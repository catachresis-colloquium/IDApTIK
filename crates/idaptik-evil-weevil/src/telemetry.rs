//! Validated telemetry — the defensive observer's only input.
//!
//! A [`TelemetryEvent`] is constructed exclusively by the harness
//! ([`crate::harness`]) from an *observed* fact: a relay-model verdict, a
//! production validator's typed error, a sim denial event, or an injected
//! delivery fault. The observer never sees raw bytes and cannot be handed
//! invented evidence: the constructors live next to the harness's delivery
//! path and each one records the detail string the host component produced.
//!
//! The fault/violation split is the module's load-bearing distinction.
//! Packet loss, delay, silence and reconnect duplicates are **faults**:
//! properties of the transport, never evidence of intent. Only a frame the
//! host's own rules refuse as a protocol breach is a violation. The observer
//! is tested against that boundary.

use serde::{Deserialize, Serialize};

/// Transport-level faults the in-memory wire can inject. None of these
/// attributes intent to a remote machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FaultKind {
    /// Delivery deferred by a bounded number of in-memory turns.
    DelayedDelivery,
    /// A completeness watermark held back past its deadline.
    WithheldWatermark,
    /// No frame arrived within the logical grace window.
    SilenceGraceExpired,
    /// A retransmitted frame from a superseded connection, dropped by rules.
    ReconnectDuplicate,
    /// The modelled transport closed before the run completed.
    TransportClosed,
}

/// Protocol breaches named by the host's own validators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViolationKind {
    /// A command scheduled below the peer's committed watermark.
    StaleAfterCommit,
    /// A command or commit outside the run's tick budget.
    PastRunSchedule,
    /// A `net:hello`/resync epoch conflicting with the agreed run.
    EpochConflict,
    /// A resync payload offered by the seat it claims to be from.
    SameSeatResync,
    /// A frame that cannot be decoded into the protocol vocabulary.
    MalformedPayload,
    /// A command tag neither the relay nor the client recognises.
    UnknownCommandTag,
    /// A command the joining seat is not authorised to send.
    RoleForbidden,
    /// A frame belonging to a session that loss already superseded.
    ObsoleteFromPriorSession,
}

/// One piece of validated evidence, with the exact detail the host produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TelemetryEvent {
    /// The relay model accepted and relayed the frame.
    Accepted { command: String, at: u64 },
    /// The relay model acknowledged and dropped the frame without relaying.
    RelayDropped { reason: String },
    /// A production validator refused the frame.
    ValidatorRejected {
        violation: ViolationKind,
        detail: String,
    },
    /// The frame decoded and reached the sim, which refused its effect with
    /// a typed denial — normal host-coped behaviour, not a protocol breach.
    SimDenied { command: String, reason: String },
    /// The frame decoded and reached the sim, which no-op'd it (e.g. an
    /// out-of-graph target index).
    SimNoOp { command: String },
    /// An injected transport fault.
    Fault {
        kind: FaultKind,
        detail: String,
    },
    /// The session recovered after a loss (resync accepted, run resumed).
    Recovered { via: String },
}

impl TelemetryEvent {
    /// Whether this evidence is a transport fault rather than a breach. The
    /// observer's no-false-positive guarantee is asserted against this set.
    pub fn is_fault(&self) -> bool {
        matches!(self, TelemetryEvent::Fault { .. })
    }

    /// Whether this evidence is a host-validated protocol violation.
    pub fn is_violation(&self) -> bool {
        matches!(self, TelemetryEvent::ValidatorRejected { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faults_and_violations_are_disjoint_sets() {
        let fault = TelemetryEvent::Fault {
            kind: FaultKind::DelayedDelivery,
            detail: "held 2 turns".into(),
        };
        let breach = TelemetryEvent::ValidatorRejected {
            violation: ViolationKind::StaleAfterCommit,
            detail: "arrived after its commit".into(),
        };
        assert!(fault.is_fault() && !fault.is_violation());
        assert!(breach.is_violation() && !breach.is_fault());
    }

    #[test]
    fn host_coped_denials_are_neither_fault_nor_violation() {
        let denied = TelemetryEvent::SimDenied {
            command: "Uplink".into(),
            reason: "NoRoute".into(),
        };
        assert!(!denied.is_fault());
        assert!(!denied.is_violation());
    }
}

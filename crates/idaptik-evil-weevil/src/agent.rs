//! Evil Weevil: the adversarial agent, as a small explicit state machine.
//!
//! The agent only *proposes*. It holds no handle to the simulation, the
//! lockstep cores, the relay model or the observer — it cannot apply a
//! gameplay action, deliver a frame, read telemetry or change policy. The
//! harness ([`crate::harness`]) holds all of those, and a proposal reaches a
//! production validator only after the policy gate
//! ([`crate::policy::TestPolicy`]) authorises the attempt.
//!
//! This is deliberately a fixed, deterministic machine over a scenario's
//! pre-declared sequence — not an adaptive planner and not an LLM. Adaptive
//! action selection is explicitly deferred (ADR-0013, "start simple") until
//! the fixed corpus demonstrates reproducible coverage; the in-game enemy
//! utility policy is gameplay AI and is not a security policy.

use crate::policy::Principal;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Version tag for every recorded intent, so replay traces stay parseable as
/// the format evolves.
pub const INTENT_SCHEMA: &str = "idaptik-evil-weevil-intent-v1";

/// The adversarial phase. Transitions are fixed (see
/// [`EvilWeevil::transition`]) and observable in the replay trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentPhase {
    /// No proposal yet; the agent has only its dummy identity.
    Grounded,
    /// Probing the acceptance surface with one malformed or impossible frame.
    Probing,
    /// Replaying frames from an earlier point in the session.
    Replaying,
    /// Withholding an observation the peer is waiting on.
    Withholding,
    /// Proposal budget spent; the agent makes no further attempts.
    Retiring,
}

/// What class of adversarial behaviour an intent represents. The harness
/// maps each kind to its expected host response; the observer maps it to an
/// evidence class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntentKind {
    /// A frame replayed at or below a watermark the peer already committed.
    StaleReplay,
    /// The same relay envelope (`seq`) sent a second time.
    DuplicateEnvelope,
    /// A frame whose shape the typed decoder cannot accept.
    MalformedPayload,
    /// A syntactically valid frame naming an impossible target.
    InvalidTargetClaim,
    /// A handshake/epoch field that conflicts with the run already agreed.
    EpochConflict,
    /// A message belonging to a session that was already superseded.
    ObsoleteFromPriorSession,
    /// An observation deliberately held back from the peer (fault injection).
    WithheldObservation,
    /// An observation whose delivery is delayed but eventually complete.
    DelayedObservation,
    /// A command the sender's seat is not authorised to send.
    RoleForbidden,
}

/// One proposal: a raw, unsanitised wire payload plus the provenance the
/// harness and replay trace need.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Intent {
    pub schema: String,
    /// Dummy principal id (never a real credential).
    pub principal: String,
    pub sequence: usize,
    pub phase: AgentPhase,
    pub kind: IntentKind,
    /// The exact frame the harness is asked to put on the in-memory wire.
    pub wire: Value,
    /// Why this frame exists in the bounded sequence.
    pub note: String,
}

/// The Evil Weevil agent. Construction takes only a principal and a fixed
/// budget; there is no RNG in the agent itself — deterministic by construction.
pub struct EvilWeevil {
    principal: Principal,
    phase: AgentPhase,
    budget: usize,
    proposals: usize,
    trace: Vec<Intent>,
}

impl EvilWeevil {
    /// An agent with the corpus's standard per-scenario budget.
    pub fn new(principal: Principal) -> Self {
        Self::with_budget(principal, crate::limits::MAX_INTENTS_PER_SCENARIO)
    }

    pub fn with_budget(principal: Principal, budget: usize) -> Self {
        Self {
            principal,
            phase: AgentPhase::Grounded,
            budget,
            proposals: 0,
            trace: Vec::new(),
        }
    }

    pub fn principal(&self) -> &Principal {
        &self.principal
    }

    pub fn phase(&self) -> AgentPhase {
        self.phase
    }

    pub fn proposals(&self) -> usize {
        self.proposals
    }

    /// The fixed transition table: each intent kind implies one phase, and
    /// the phase advances deterministically. `Withholding` is terminal until
    /// [`Self::retire`] because a withholding scenario has nothing left to
    /// send.
    fn transition(kind: IntentKind) -> AgentPhase {
        match kind {
            IntentKind::StaleReplay
            | IntentKind::DuplicateEnvelope
            | IntentKind::ObsoleteFromPriorSession => AgentPhase::Replaying,
            IntentKind::WithheldObservation | IntentKind::DelayedObservation => {
                AgentPhase::Withholding
            }
            IntentKind::MalformedPayload
            | IntentKind::InvalidTargetClaim
            | IntentKind::EpochConflict
            | IntentKind::RoleForbidden => AgentPhase::Probing,
        }
    }

    /// Propose an intent. Refuses once the budget is exhausted, which is the
    /// guard against an accidental open-ended loop: an over-budget proposal is
    /// an error the scenario records rather than sends.
    pub fn propose(&mut self, kind: IntentKind, wire: Value, note: &str) -> Result<Intent, String> {
        if self.proposals >= self.budget {
            return Err(format!(
                "Evil Weevil intent budget ({}) exhausted; refusing to propose further",
                self.budget
            ));
        }
        self.phase = Self::transition(kind);
        let intent = Intent {
            schema: INTENT_SCHEMA.into(),
            principal: self.principal.identity.label.clone(),
            sequence: self.proposals,
            phase: self.phase,
            kind,
            wire,
            note: note.to_owned(),
        };
        self.proposals += 1;
        self.trace.push(intent.clone());
        Ok(intent)
    }

    /// Move to the retired phase explicitly.
    pub fn retire(&mut self) {
        self.phase = AgentPhase::Retiring;
    }

    /// The ordered proposals so far — the replay artefact's input side.
    pub fn trace(&self) -> &[Intent] {
        &self.trace
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use idaptik_net::envelope::Role;
    use serde_json::json;

    fn agent() -> EvilWeevil {
        EvilWeevil::with_budget(Principal::adversary(Role::Hacker, 1), 2)
    }

    #[test]
    fn phases_follow_the_fixed_transition_table() {
        let mut w = agent();
        assert_eq!(w.phase(), AgentPhase::Grounded);
        w.propose(IntentKind::MalformedPayload, json!({}), "probe")
            .unwrap();
        assert_eq!(w.phase(), AgentPhase::Probing);
        w.propose(IntentKind::StaleReplay, json!({}), "replay")
            .unwrap();
        assert_eq!(w.phase(), AgentPhase::Replaying);
        w.retire();
        assert_eq!(w.phase(), AgentPhase::Retiring);
    }

    #[test]
    fn the_budget_is_a_hard_stop() {
        let mut w = agent();
        w.propose(IntentKind::MalformedPayload, json!({}), "one")
            .unwrap();
        w.propose(IntentKind::MalformedPayload, json!({}), "two")
            .unwrap();
        let err = w.propose(IntentKind::MalformedPayload, json!({}), "three");
        assert!(err.is_err(), "a third proposal must be refused");
        assert_eq!(w.proposals(), 2);
    }

    #[test]
    fn every_intent_carries_the_versioned_schema_and_dummy_owner() {
        let mut w = agent();
        let intent = w
            .propose(IntentKind::RoleForbidden, json!({}), "x")
            .unwrap();
        assert_eq!(intent.schema, INTENT_SCHEMA);
        assert!(intent.principal.starts_with("ew-dummy-"));
    }
}

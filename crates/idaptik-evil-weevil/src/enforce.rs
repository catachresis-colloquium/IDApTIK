//! Host/operator enforcement policy — the only place an *action* may follow
//! an observer recommendation.
//!
//! The observer ([`crate::observer`]) recommends; this policy decides, and it
//! decides only for the local test session. The action alphabet deliberately
//! contains no remote ban, blocklist, retaliation or packet-injection
//! variant, so no configuration can create universal ban authority — local
//! protective decisions do not assume it. The default mode is
//! [`EnforcementMode::ObserveOnly`]: evidence is recorded and nothing acts.

use crate::observer::Recommendation;
use crate::policy::{DummyIdentity, Principal, PrincipalKind};
use serde::{Deserialize, Serialize};

/// How the local host responds to recommendations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnforcementMode {
    /// Record evidence and recommendations; never act. The default.
    ObserveOnly,
    /// Permit the bounded local protective actions below. Still no remote
    /// act of any kind.
    LocalProtective,
}

impl Default for EnforcementMode {
    fn default() -> Self {
        EnforcementMode::ObserveOnly
    }
}

/// Everything the host can do. Exhaustive on purpose: a request for a new
/// enforcement action is an explicit code review, not a config flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnforcementAction {
    /// Do nothing beyond what the observer already logged.
    None,
    /// Keep the local trace entry (the default fate of every event).
    LogOnly,
    /// Trigger the existing recovery path for the *local* seat.
    RequestLocalResync,
    /// End the local test session cleanly.
    EndLocalSession,
    /// Surface the evidence to a human. No automatic consequence.
    FlagForOperatorReview,
}

/// The policy held by the host operator principal.
#[derive(Debug, Clone)]
pub struct EnforcementPolicy {
    mode: EnforcementMode,
    operator: DummyIdentity,
}

impl EnforcementPolicy {
    /// The safe default: observe only.
    pub fn observe_only() -> Self {
        Self {
            mode: EnforcementMode::ObserveOnly,
            operator: DummyIdentity::operator(),
        }
    }

    /// The explicit opt-in. Constructing it requires an operator principal,
    /// which the permission policy only grants to [`PrincipalKind::HostOperator`].
    pub fn local_protective(operator: &Principal) -> Result<Self, String> {
        if operator.kind != PrincipalKind::HostOperator {
            return Err(format!(
                "only a HostOperator may select LocalProtective; got {:?}",
                operator.kind
            ));
        }
        Ok(Self {
            mode: EnforcementMode::LocalProtective,
            operator: operator.identity.clone(),
        })
    }

    pub fn mode(&self) -> EnforcementMode {
        self.mode
    }

    pub fn operator_label(&self) -> &str {
        &self.operator.label
    }

    /// Map an observer recommendation to an action. Even under
    /// `LocalProtective`, the strongest automatic response is ending the
    /// *local* session; review flags remain review flags.
    pub fn decide(&self, recommendation: Recommendation) -> EnforcementAction {
        match (self.mode, recommendation) {
            (EnforcementMode::ObserveOnly, Recommendation::None) => EnforcementAction::None,
            (EnforcementMode::ObserveOnly, _) => EnforcementAction::LogOnly,
            (EnforcementMode::LocalProtective, Recommendation::None) => EnforcementAction::None,
            (EnforcementMode::LocalProtective, Recommendation::LogOnly) => {
                EnforcementAction::LogOnly
            }
            (EnforcementMode::LocalProtective, Recommendation::RequestResync) => {
                EnforcementAction::RequestLocalResync
            }
            (EnforcementMode::LocalProtective, Recommendation::FlagForOperatorReview) => {
                EnforcementAction::FlagForOperatorReview
            }
        }
    }

    /// The one explicit operator override: end this seat's local session.
    /// Nothing remote happens, and it requires the operator principal even in
    /// a protective policy.
    pub fn operator_end_local_session(
        &self,
        operator: &Principal,
    ) -> Result<EnforcementAction, String> {
        if operator.kind != PrincipalKind::HostOperator {
            return Err("session termination is an operator-only action".into());
        }
        if self.mode != EnforcementMode::LocalProtective {
            return Err("ObserveOnly policy cannot end sessions".into());
        }
        Ok(EnforcementAction::EndLocalSession)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::Principal;

    #[test]
    fn observe_only_logs_everything_and_acts_on_nothing() {
        let policy = EnforcementPolicy::observe_only();
        assert_eq!(policy.mode(), EnforcementMode::ObserveOnly);
        assert_eq!(
            policy.decide(Recommendation::FlagForOperatorReview),
            EnforcementAction::LogOnly
        );
        assert_eq!(
            policy.decide(Recommendation::RequestResync),
            EnforcementAction::LogOnly
        );
        let operator = Principal::operator();
        assert!(policy.operator_end_local_session(&operator).is_err());
    }

    #[test]
    fn local_protective_is_operator_only() {
        let harness = Principal::harness();
        assert!(EnforcementPolicy::local_protective(&harness).is_err());
        let operator = Principal::operator();
        let policy = EnforcementPolicy::local_protective(&operator).unwrap();
        assert_eq!(
            policy.decide(Recommendation::FlagForOperatorReview),
            EnforcementAction::FlagForOperatorReview
        );
        assert_eq!(
            policy.decide(Recommendation::RequestResync),
            EnforcementAction::RequestLocalResync
        );
        assert_eq!(
            policy.operator_end_local_session(&operator).unwrap(),
            EnforcementAction::EndLocalSession
        );
    }

    #[test]
    fn non_operator_principals_cannot_terminate() {
        let operator = Principal::operator();
        let policy = EnforcementPolicy::local_protective(&operator).unwrap();
        for usurper in [
            Principal::harness(),
            Principal::observer(),
            Principal::adversary(idaptik_net::envelope::Role::Hacker, 1),
        ] {
            assert!(policy.operator_end_local_session(&usurper).is_err());
        }
    }
}

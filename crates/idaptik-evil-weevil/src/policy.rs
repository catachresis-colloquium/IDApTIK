//! The test permission policy — who may do what inside the adversarial
//! adapter, enforced as a separate decision from every production guard.
//!
//! Four principals exist, and their permissions are deliberately not
//! interchangeable:
//!
//! | Principal | May | May never |
//! |---|---|---|
//! | [`PrincipalKind::Adversary`] (Evil Weevil) | propose bounded intents from a scenario's fixed vocabulary | touch the sim, deliver frames, read telemetry, change policy |
//! | [`PrincipalKind::TestHarness`] | deliver proposals through the in-memory test relay and the *real* production validators, record outcomes | mutate sim state directly, relax a production check, grant permissions |
//! | [`PrincipalKind::DefensiveObserver`] | read validated telemetry, classify it, recommend an operator action | send anything, end a session, enforce |
//! | [`PrincipalKind::HostOperator`] | hold the enforcement policy, take local protective action | ban or retaliate against a remote machine (no such permission exists) |
//!
//! There is deliberately no `BypassHostValidation` permission: the production
//! protocol handler's acceptance rules are outside this policy's reach. The
//! harness decides whether an *attempt* is authorised inside the scenario;
//! the host (`idaptik-net`'s decoders/lockstep core and `idaptik-core`'s sim)
//! independently decides whether the resulting frame is acceptable.

use idaptik_net::envelope::Role;
use serde::{Deserialize, Serialize};

/// A disposable, non-secret test identity. Every name here is a fixed dummy
/// string — no real accounts, tokens or credentials exist in the corpus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DummyIdentity {
    /// Stable label, e.g. `ew-dummy-hacker-0001`.
    pub label: String,
    /// Which game seat the dummy joins as, when the principal has a seat.
    pub seat: Option<Role>,
}

impl DummyIdentity {
    pub fn adversary(seat: Role, n: u8) -> Self {
        Self {
            label: format!("ew-dummy-{}-{n:04}", seat.as_str()),
            seat: Some(seat),
        }
    }

    pub fn harness() -> Self {
        Self {
            label: "ew-harness".into(),
            seat: None,
        }
    }

    pub fn observer() -> Self {
        Self {
            label: "ew-defensive-observer".into(),
            seat: None,
        }
    }

    pub fn operator() -> Self {
        Self {
            label: "ew-local-operator".into(),
            seat: None,
        }
    }
}

/// The four non-interchangeable permission realms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrincipalKind {
    /// Evil Weevil: proposes adversarial intents, holds no other power.
    Adversary,
    /// The test harness: owns the in-memory wire and the production cores.
    TestHarness,
    /// The read-only defensive observer.
    DefensiveObserver,
    /// The local host/operator behind the enforcement policy.
    HostOperator,
}

/// A principal acting in a scenario.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    pub identity: DummyIdentity,
    pub kind: PrincipalKind,
}

impl Principal {
    pub fn adversary(seat: Role, n: u8) -> Self {
        Self {
            identity: DummyIdentity::adversary(seat, n),
            kind: PrincipalKind::Adversary,
        }
    }

    pub fn harness() -> Self {
        Self {
            identity: DummyIdentity::harness(),
            kind: PrincipalKind::TestHarness,
        }
    }

    pub fn observer() -> Self {
        Self {
            identity: DummyIdentity::observer(),
            kind: PrincipalKind::DefensiveObserver,
        }
    }

    pub fn operator() -> Self {
        Self {
            identity: DummyIdentity::operator(),
            kind: PrincipalKind::HostOperator,
        }
    }
}

/// The finite permission alphabet. Each variant names one capability the
/// adapter can grant; anything not listed is impossible by construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Permission {
    /// Propose an intent from the scenario's bounded, pre-declared sequence.
    ProposeBoundedIntent,
    /// Pass a frame through the in-process model of the relay rules.
    DeliverThroughTestRelay,
    /// Feed a decoded frame to the real production protocol validators.
    FeedProductionValidator,
    /// Record an observed outcome into the scenario trace.
    RecordOutcome,
    /// Read telemetry the harness has already validated.
    ReadValidatedTelemetry,
    /// Classify validated telemetry as benign / fault / violation.
    ClassifyEvidence,
    /// Produce a non-binding recommendation for the host operator.
    RecommendOperatorAction,
    /// Choose the local enforcement mode (observe-only vs local-protective).
    ConfigureEnforcementPolicy,
    /// End the *local* test session. There is no remote-actuation permission.
    EndLocalSession,
}

/// The policy's answer to a permission request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    Deny,
}

/// The per-scenario allow-list. Constructed once per scenario through the
/// fixed [`Self::corpus_default`] grants; scenarios narrow it, never widen it.
#[derive(Debug, Clone)]
pub struct TestPolicy {
    scenario_id: &'static str,
    grants: Vec<(PrincipalKind, Permission)>,
}

impl TestPolicy {
    /// The fixed grant table every corpus scenario starts from. The
    /// adversary's only grant is [`Permission::ProposeBoundedIntent`].
    pub fn corpus_default(scenario_id: &'static str) -> Self {
        use Permission as P;
        use PrincipalKind as K;
        let grants = vec![
            (K::Adversary, P::ProposeBoundedIntent),
            (K::TestHarness, P::DeliverThroughTestRelay),
            (K::TestHarness, P::FeedProductionValidator),
            (K::TestHarness, P::RecordOutcome),
            (K::DefensiveObserver, P::ReadValidatedTelemetry),
            (K::DefensiveObserver, P::ClassifyEvidence),
            (K::DefensiveObserver, P::RecommendOperatorAction),
            (K::HostOperator, P::ConfigureEnforcementPolicy),
            (K::HostOperator, P::EndLocalSession),
        ];
        Self {
            scenario_id,
            grants,
        }
    }

    pub fn scenario_id(&self) -> &'static str {
        self.scenario_id
    }

    /// The permissions this policy grants to one principal.
    pub fn permissions_of(&self, kind: PrincipalKind) -> Vec<Permission> {
        self.grants
            .iter()
            .filter(|(k, _)| *k == kind)
            .map(|(_, p)| *p)
            .collect()
    }

    /// The harness-side gate: is this operation allowed for this principal in
    /// this scenario? This never consults, and can never relax, a production
    /// validator — it only decides whether the attempt may be made.
    pub fn decide(&self, principal: &Principal, permission: Permission) -> PolicyDecision {
        if self.grants.contains(&(principal.kind, permission)) {
            PolicyDecision::Allow
        } else {
            PolicyDecision::Deny
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_adversary_only_proposes() {
        let policy = TestPolicy::corpus_default("unit");
        let weevil = Principal::adversary(Role::Hacker, 1);
        assert_eq!(
            policy.permissions_of(PrincipalKind::Adversary),
            vec![Permission::ProposeBoundedIntent]
        );
        assert_eq!(
            policy.decide(&weevil, Permission::ProposeBoundedIntent),
            PolicyDecision::Allow
        );
        // Every operational permission is refused to the adversary.
        for denied in [
            Permission::DeliverThroughTestRelay,
            Permission::FeedProductionValidator,
            Permission::ReadValidatedTelemetry,
            Permission::ConfigureEnforcementPolicy,
            Permission::EndLocalSession,
        ] {
            assert_eq!(policy.decide(&weevil, denied), PolicyDecision::Deny);
        }
    }

    #[test]
    fn the_observer_is_read_only() {
        let policy = TestPolicy::corpus_default("unit");
        let observer = Principal::observer();
        for allowed in [
            Permission::ReadValidatedTelemetry,
            Permission::ClassifyEvidence,
            Permission::RecommendOperatorAction,
        ] {
            assert_eq!(policy.decide(&observer, allowed), PolicyDecision::Allow);
        }
        for denied in [
            Permission::ProposeBoundedIntent,
            Permission::DeliverThroughTestRelay,
            Permission::FeedProductionValidator,
            Permission::EndLocalSession,
        ] {
            assert_eq!(policy.decide(&observer, denied), PolicyDecision::Deny);
        }
    }

    #[test]
    fn the_harness_cannot_grant_itself_an_escape_hatch() {
        let policy = TestPolicy::corpus_default("unit");
        let harness = Principal::harness();
        // Policy configuration belongs to the operator; the harness cannot
        // take it, and nothing can bypass the host because no such permission
        // exists in the alphabet.
        assert_eq!(
            policy.decide(&harness, Permission::ConfigureEnforcementPolicy),
            PolicyDecision::Deny
        );
        assert_eq!(
            policy.decide(&harness, Permission::EndLocalSession),
            PolicyDecision::Deny
        );
    }

    #[test]
    fn dummy_identities_are_disposable_and_seat_typed() {
        let inf = DummyIdentity::adversary(Role::Infiltrator, 7);
        assert_eq!(inf.label, "ew-dummy-infiltrator-0007");
        assert_eq!(inf.seat, Some(Role::Infiltrator));
        assert!(DummyIdentity::harness().seat.is_none());
    }
}

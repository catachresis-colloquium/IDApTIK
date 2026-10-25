//! Boundary and negative cases for the declarative ADR-0015 model.
//! These tests do not claim a running transport, server, or durable store.
use std::collections::BTreeSet;

use idaptik_net::deployment::*;

const TOPOLOGIES: [NetworkTopology; 3] = [
    NetworkTopology::PeerToPeer,
    NetworkTopology::Hosted,
    NetworkTopology::Federated,
];
const GSA_MODES: [GsaIntegration; 2] = [GsaIntegration::Absent, GsaIntegration::Present];

fn ids(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|s| (*s).into()).collect()
}

fn operators(values: &[(&str, &str)]) -> AdministrativeOwnership {
    AdministrativeOwnership::FederatedOperators(
        values
            .iter()
            .map(|(s, o)| ((*s).into(), (*o).into()))
            .collect(),
    )
}

fn server(id: &str, epoch: u64) -> SimulationAuthority {
    SimulationAuthority::SingleServer {
        server_id: id.into(),
        epoch,
    }
}

fn hosted_owner(server_id: &str, operator_id: &str) -> AdministrativeOwnership {
    AdministrativeOwnership::SingleOperator {
        server_id: server_id.into(),
        operator_id: operator_id.into(),
    }
}

fn validate_in_both_gsa_modes(
    topology: NetworkTopology,
    administration: AdministrativeOwnership,
    authority: SimulationAuthority,
    expected: Result<(), DeploymentError>,
) {
    for gsa in GSA_MODES {
        let axes = DeploymentAxes {
            topology,
            administration: administration.clone(),
            authority: authority.clone(),
            gsa,
        };
        assert_eq!(axes.validate(), expected, "{axes:?}");
    }
}

#[test]
fn only_documented_combinations_of_topology_administration_and_authority_are_supported() {
    let administrations = [
        AdministrativeOwnership::Participants(ids(&["alice", "bob"])),
        hosted_owner("home", "operator"),
        operators(&[("home", "alice"), ("remote", "bob")]),
    ];
    let authorities = [
        SimulationAuthority::PeerReplicas(ids(&["node-1", "node-2"])),
        server("home", 1),
    ];
    for (topology_index, topology) in TOPOLOGIES.into_iter().enumerate() {
        for (admin_index, administration) in administrations.iter().enumerate() {
            for (authority_index, authority) in authorities.iter().enumerate() {
                let supported = topology_index == admin_index
                    && authority_index == usize::from(topology != NetworkTopology::PeerToPeer);
                validate_in_both_gsa_modes(
                    topology,
                    administration.clone(),
                    authority.clone(),
                    if supported {
                        Ok(())
                    } else {
                        Err(DeploymentError::AxesDoNotDescribeASupportedProfile)
                    },
                );
            }
        }
    }
}

#[test]
fn peer_owners_and_replicas_each_require_at_least_two_distinct_identities() {
    for small in [ids(&[]), ids(&["only"]), ids(&["same", "same"])] {
        for (owners, peers) in [
            (small.clone(), ids(&["node-1", "node-2"])),
            (ids(&["alice", "bob"]), small),
        ] {
            validate_in_both_gsa_modes(
                NetworkTopology::PeerToPeer,
                AdministrativeOwnership::Participants(owners),
                SimulationAuthority::PeerReplicas(peers),
                Err(DeploymentError::AtLeastTwoPeersRequired),
            );
        }
    }
    // The two namespaces need not have equal cardinality or matching IDs.
    validate_in_both_gsa_modes(
        NetworkTopology::PeerToPeer,
        AdministrativeOwnership::Participants(ids(&["alice", "bob", "carol"])),
        SimulationAuthority::PeerReplicas(ids(&["node-1", "node-2"])),
        Ok(()),
    );
}

#[test]
fn blank_peer_owner_or_replica_is_rejected_even_when_cardinality_is_sufficient() {
    for blank in ["", " \t\n", "\u{2003}"] {
        for (owners, peers) in [
            (ids(&[blank, "alice"]), ids(&["node-1", "node-2"])),
            (ids(&["alice", "bob"]), ids(&[blank, "node-1"])),
        ] {
            validate_in_both_gsa_modes(
                NetworkTopology::PeerToPeer,
                AdministrativeOwnership::Participants(owners),
                SimulationAuthority::PeerReplicas(peers),
                Err(DeploymentError::EmptyIdentity),
            );
        }
    }
}

#[test]
fn hosted_identity_epoch_and_exact_authority_match_are_required() {
    for blank in ["", " \t\n", "\u{2003}"] {
        for owner in [hosted_owner(blank, "operator"), hosted_owner("home", blank)] {
            validate_in_both_gsa_modes(
                NetworkTopology::Hosted,
                owner,
                server("home", 1),
                Err(DeploymentError::EmptyIdentity),
            );
        }
    }
    validate_in_both_gsa_modes(
        NetworkTopology::Hosted,
        hosted_owner("home", "operator"),
        server("home", 0),
        Err(DeploymentError::NonzeroAuthorityEpochRequired),
    );
    for other in ["other", "home ", ""] {
        validate_in_both_gsa_modes(
            NetworkTopology::Hosted,
            hosted_owner("home", "operator"),
            server(other, 1),
            Err(DeploymentError::AxesDoNotDescribeASupportedProfile),
        );
    }
    validate_in_both_gsa_modes(
        NetworkTopology::Hosted,
        hosted_owner("home", "operator"),
        server("home", u64::MAX),
        Ok(()),
    );
}

#[test]
fn federation_requires_two_servers_with_nonblank_owners_and_a_configured_fenced_authority() {
    for entries in [
        vec![],
        vec![("home", "operator")],
        vec![("home", "a"), ("home", "b")],
    ] {
        validate_in_both_gsa_modes(
            NetworkTopology::Federated,
            operators(&entries),
            server("home", 1),
            Err(DeploymentError::AtLeastTwoFederatedServersRequired),
        );
    }
    for blank in ["", " \t\n", "\u{2003}"] {
        for (administration, authority) in [
            (operators(&[(blank, "a"), ("home", "b")]), server("home", 1)),
            (
                operators(&[("home", "a"), ("remote", blank)]),
                server("home", 1),
            ),
            (
                operators(&[("home", "a"), ("remote", "b")]),
                server(blank, 1),
            ),
        ] {
            validate_in_both_gsa_modes(
                NetworkTopology::Federated,
                administration,
                authority,
                Err(DeploymentError::EmptyIdentity),
            );
        }
    }
    let administration = operators(&[("home", "a"), ("remote", "b")]);
    validate_in_both_gsa_modes(
        NetworkTopology::Federated,
        administration.clone(),
        server("home", 0),
        Err(DeploymentError::NonzeroAuthorityEpochRequired),
    );
    validate_in_both_gsa_modes(
        NetworkTopology::Federated,
        administration.clone(),
        server("unknown", 1),
        Err(DeploymentError::AxesDoNotDescribeASupportedProfile),
    );
    // Any configured server can own its own session, including the highest epoch.
    validate_in_both_gsa_modes(
        NetworkTopology::Federated,
        administration,
        server("remote", u64::MAX),
        Ok(()),
    );
}

fn placement() -> SessionPlacement {
    SessionPlacement {
        session_id: "session".into(),
        authority: AuthorityStamp {
            server_id: "home".into(),
            epoch: 7,
        },
    }
}

#[test]
fn routing_never_substitutes_a_reachable_server_or_changes_the_authority_stamp() {
    let placement = placement();
    for reachable in [ids(&[]), ids(&["other"]), ids(&["home "])] {
        assert_eq!(
            route_authority(&placement, &reachable),
            AuthorityRoute::Unavailable(placement.authority.clone())
        );
    }
    for reachable in [ids(&["home"]), ids(&["other", "home"])] {
        assert_eq!(
            route_authority(&placement, &reachable),
            AuthorityRoute::Reachable(placement.authority.clone())
        );
    }
}

#[test]
fn recovery_accepts_first_and_last_available_sequences_but_never_wraps() {
    let placement = placement();
    for (committed_through, expected) in [
        (0, Ok(1)),
        (u64::MAX - 1, Ok(u64::MAX)),
        (u64::MAX, Err(RecoveryError::SequenceExhausted)),
    ] {
        let checkpoint = SessionCheckpoint {
            session_id: placement.session_id.clone(),
            authority: placement.authority.clone(),
            committed_through,
        };
        assert_eq!(recover_next_sequence(&placement, &checkpoint), expected);
    }
}

#[test]
fn recovery_checks_session_and_both_authority_fields_before_sequence_arithmetic() {
    let placement = placement();
    for committed_through in [0, u64::MAX] {
        for (session_id, server_id, epoch, expected) in [
            ("other", "home", 7, RecoveryError::SessionMismatch),
            ("session", "other", 7, RecoveryError::AuthorityMismatch),
            ("session", "home", 6, RecoveryError::AuthorityMismatch),
            ("session", "home", 8, RecoveryError::AuthorityMismatch),
            ("other", "other", 6, RecoveryError::SessionMismatch),
        ] {
            let checkpoint = SessionCheckpoint {
                session_id: session_id.into(),
                authority: AuthorityStamp {
                    server_id: server_id.into(),
                    epoch,
                },
                committed_through,
            };
            assert_eq!(
                recover_next_sequence(&placement, &checkpoint),
                Err(expected)
            );
        }
    }
}

#[test]
fn state_class_inventory_has_no_omissions_or_duplicates() {
    let expected = [
        StateClass::MembershipAndPresence,
        StateClass::PendingInputs,
        StateClass::ActiveSimulation,
        StateClass::CommittedHistory,
        StateClass::FederationDirectory,
        StateClass::OperatorConfiguration,
        StateClass::OperationsCatalog,
        StateClass::GameContent,
        StateClass::DerivedCache,
        StateClass::MediaPlayout,
    ];
    assert_eq!(STATE_CLASSES.len(), expected.len());
    for class in expected {
        assert_eq!(
            STATE_CLASSES.iter().filter(|&&c| c == class).count(),
            1,
            "{class:?}"
        );
    }
}

#[test]
fn state_storage_and_recovery_follow_the_adr0015_policy_table() {
    use CacheStore::{None, OptionalEtsOrDragonfly};
    use Consistency as C;
    use Durability as D;
    use HotStore as H;
    use NetworkTopology::{Federated, Hosted, PeerToPeer};
    use PartitionBehavior as P;
    use Recovery as R;
    use StateClass as S;
    use StateOwner as O;

    // Expected policy rows are taken from ADR-0015, section 4. Each tuple
    // covers all seven dimensions, including those not checked by older tests.
    let rows = [
        (
            S::MembershipAndPresence,
            vec![PeerToPeer],
            O::PeerSet,
            H::RustProcessMemory,
            D::Volatile,
            None,
            C::BestEffortEphemeral,
            P::ExpireAndReannounce,
            R::RejoinAndReannounce,
        ),
        (
            S::MembershipAndPresence,
            vec![Hosted, Federated],
            O::SessionProcess,
            H::BeamProcessAndEts,
            D::Volatile,
            None,
            C::BestEffortEphemeral,
            P::ExpireAndReannounce,
            R::RejoinAndReannounce,
        ),
        (
            S::PendingInputs,
            TOPOLOGIES.to_vec(),
            O::InputSeatUntilCommit,
            H::RustProcessMemory,
            D::Volatile,
            None,
            C::OrderedSeatInputs,
            P::HoldAtMissingWatermark,
            R::DropUncommittedAndResample,
        ),
        (
            S::ActiveSimulation,
            vec![PeerToPeer],
            O::PeerSet,
            H::RustProcessMemory,
            D::Volatile,
            None,
            C::DeterministicPeerLockstep,
            P::PauseAndResyncFromSurvivor,
            R::AdoptSurvivorSnapshotAndReplay,
        ),
        (
            S::ActiveSimulation,
            vec![Hosted, Federated],
            O::SessionAuthority,
            H::RustProcessMemory,
            D::LocalAuthorityLmdbJournal,
            None,
            C::SingleAuthoritySequence,
            P::FailClosedWithoutAuthority,
            R::ReplaySameAuthorityJournal,
        ),
        (
            S::CommittedHistory,
            vec![PeerToPeer],
            O::PeerSet,
            H::RustProcessMemory,
            D::Volatile,
            None,
            C::DeterministicPeerLockstep,
            P::PauseAndResyncFromSurvivor,
            R::AdoptSurvivorSnapshotAndReplay,
        ),
        (
            S::CommittedHistory,
            vec![Hosted, Federated],
            O::SessionAuthority,
            H::RustProcessMemory,
            D::LocalAuthorityLmdbJournal,
            None,
            C::SingleAuthoritySequence,
            P::FailClosedWithoutAuthority,
            R::ReplaySameAuthorityJournal,
        ),
        (
            S::FederationDirectory,
            vec![PeerToPeer, Hosted],
            O::LocalOperator,
            H::NotRequired,
            D::NotRequired,
            None,
            C::NotApplicable,
            P::NotRequired,
            R::NotRequired,
        ),
        (
            S::FederationDirectory,
            vec![Federated],
            O::FederationOperators,
            H::VersionedLocalConfig,
            D::VersionedConfigFile,
            OptionalEtsOrDragonfly,
            C::EventualAdvisoryDirectory,
            P::StaleDirectoryNeverPromotesAuthority,
            R::ReannounceConfiguredNodes,
        ),
        (
            S::OperatorConfiguration,
            vec![PeerToPeer],
            O::PeerSet,
            H::VersionedLocalConfig,
            D::VersionedConfigFile,
            None,
            C::VersionedSingleWriter,
            P::KeepLastValidatedConfigDenyEdits,
            R::ReloadAndReconcileConfig,
        ),
        (
            S::OperatorConfiguration,
            vec![Hosted, Federated],
            O::LocalOperator,
            H::VersionedLocalConfig,
            D::VersionedConfigFile,
            None,
            C::VersionedSingleWriter,
            P::KeepLastValidatedConfigDenyEdits,
            R::ReloadAndReconcileConfig,
        ),
        (
            S::OperationsCatalog,
            vec![PeerToPeer],
            O::OperationsPlane,
            H::NotRequired,
            D::NotRequired,
            None,
            C::NotApplicable,
            P::NotRequired,
            R::NotRequired,
        ),
        (
            S::OperationsCatalog,
            vec![Hosted, Federated],
            O::OperationsPlane,
            H::OptionalVeriSimDb,
            D::OptionalOperationsDatabase,
            OptionalEtsOrDragonfly,
            C::EventualOperationsProjection,
            P::ContinueGameplayWithoutOperationsPlane,
            R::RebuildProjectionFromLocalSource,
        ),
        (
            S::GameContent,
            TOPOLOGIES.to_vec(),
            O::GameRelease,
            H::VersionedPackageFiles,
            D::VersionedPackageFile,
            None,
            C::ImmutableContentHash,
            P::UseInstalledContentOrRefuse,
            R::FetchExactContentDigest,
        ),
        // ADR-0015, state-class table: a derived cache has "no authoritative
        // owner" and, in P2P, "no required store" — the peers hold nothing
        // between them that is not recomputable, so there is no hot store to
        // name. Hosted and federated sessions keep a local ETS or an optional
        // Dragonfly. The same sentence governs media playout, which is why the
        // two rows are split the same way.
        (
            S::DerivedCache,
            vec![PeerToPeer],
            O::DerivedOnly,
            H::NotRequired,
            D::Volatile,
            None,
            C::Recomputable,
            P::CacheMissAndReadSource,
            R::RecomputeFromAuthority,
        ),
        (
            S::DerivedCache,
            vec![Hosted, Federated],
            O::DerivedOnly,
            H::EtsOrDragonfly,
            D::Volatile,
            None,
            C::Recomputable,
            P::CacheMissAndReadSource,
            R::RecomputeFromAuthority,
        ),
        (
            S::MediaPlayout,
            vec![PeerToPeer],
            O::MediaSubsystem,
            H::NotRequired,
            D::SemanticallyDisposableMediaBuffer,
            None,
            C::OrderedBoundedMediaFrames,
            P::DropMediaFrames,
            R::FlushAndRejoinMedia,
        ),
        (
            S::MediaPlayout,
            vec![Hosted, Federated],
            O::MediaSubsystem,
            H::BurbleLmdbRingBufferOrEts,
            D::SemanticallyDisposableMediaBuffer,
            None,
            C::OrderedBoundedMediaFrames,
            P::DropMediaFrames,
            R::FlushAndRejoinMedia,
        ),
    ];
    let mut checked = 0;
    for (
        class,
        topologies,
        owner,
        hot_store,
        durability,
        cache,
        consistency,
        partition,
        recovery,
    ) in rows
    {
        for topology in topologies {
            assert_eq!(
                state_contract(class, topology),
                StateContract {
                    owner,
                    hot_store,
                    durability,
                    cache,
                    consistency,
                    partition,
                    recovery,
                },
                "{class:?} / {topology:?}"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 30);
}

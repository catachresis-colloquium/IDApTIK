//! Deployment axes and state-placement contract (ADR-0013).
//!
//! This is a declarative architecture model, not a server launcher, network
//! directory, persistence adapter, or GSA client. The current live runtime is
//! still the relay-only client path documented in ADR-0005/0006. These types
//! keep topology, administrative ownership, simulation authority, and GSA
//! presence distinct while implementation is phased across repositories.

use std::collections::{BTreeMap, BTreeSet};

/// How nodes are connected. This does not, by itself, name who may write game
/// state or who owns server administration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkTopology {
    /// Players exchange lockstep data directly; no persistent central service
    /// is required. A reachable LAN/VPN/manual route is still required by the
    /// eventual transport implementation.
    PeerToPeer,
    /// Multiple independently operated servers expose sessions through an
    /// explicitly configured federation/directory boundary.
    Federated,
    /// One server deployment provides a session's authority and service.
    Hosted,
}

/// Who owns administrative policy and configuration. GSA is not an owner in
/// this model; it is an optional management client for the actual owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdministrativeOwnership {
    /// Each named participant owns their local client/device configuration.
    Participants(BTreeSet<String>),
    /// One operator owns one hosted server's configuration.
    SingleOperator {
        server_id: String,
        operator_id: String,
    },
    /// Every federation server remains under its own operator.
    FederatedOperators(BTreeMap<String, String>),
}

/// Who is allowed to decide the simulation's next committed state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimulationAuthority {
    /// Deterministic replicas run the same committed input prefix. This is
    /// lockstep, not leader election, consensus, or an anti-cheat guarantee.
    PeerReplicas(BTreeSet<String>),
    /// One server is the sole writer for this session. `epoch` is an authority
    /// fence to be persisted and checked by a future server implementation; it
    /// is not itself a lease, election, or consensus protocol.
    SingleServer { server_id: String, epoch: u64 },
}

/// Whether a deployment has an optional Game Server Admin integration.
///
/// `Present` records configuration only. It does not claim that the current
/// GSA repository exposes a live IDApTIK runtime adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GsaIntegration {
    Absent,
    Present,
}

/// The four independently meaningful deployment axes for a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeploymentAxes {
    pub topology: NetworkTopology,
    pub administration: AdministrativeOwnership,
    pub authority: SimulationAuthority,
    pub gsa: GsaIntegration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentError {
    EmptyIdentity,
    AtLeastTwoPeersRequired,
    AtLeastTwoFederatedServersRequired,
    AxesDoNotDescribeASupportedProfile,
    NonzeroAuthorityEpochRequired,
}

impl DeploymentAxes {
    /// Validate that the four separate axes form one of the documented
    /// profiles. GSA presence is deliberately not part of these checks.
    pub fn validate(&self) -> Result<(), DeploymentError> {
        match (&self.topology, &self.administration, &self.authority) {
            (
                NetworkTopology::PeerToPeer,
                AdministrativeOwnership::Participants(owners),
                SimulationAuthority::PeerReplicas(peers),
            ) => {
                if owners.len() < 2 || peers.len() < 2 {
                    return Err(DeploymentError::AtLeastTwoPeersRequired);
                }
                if owners.iter().chain(peers).any(|id| id.trim().is_empty()) {
                    return Err(DeploymentError::EmptyIdentity);
                }
                // Administrative participant IDs and replica/node IDs live
                // in separate namespaces; validate both, but do not collapse
                // the ownership and authority axes into one identifier set.
                Ok(())
            }
            (
                NetworkTopology::Hosted,
                AdministrativeOwnership::SingleOperator {
                    server_id,
                    operator_id,
                },
                SimulationAuthority::SingleServer {
                    server_id: authority_server,
                    epoch,
                },
            ) => {
                if server_id.trim().is_empty() || operator_id.trim().is_empty() {
                    return Err(DeploymentError::EmptyIdentity);
                }
                if *epoch == 0 {
                    return Err(DeploymentError::NonzeroAuthorityEpochRequired);
                }
                if server_id != authority_server {
                    return Err(DeploymentError::AxesDoNotDescribeASupportedProfile);
                }
                Ok(())
            }
            (
                NetworkTopology::Federated,
                AdministrativeOwnership::FederatedOperators(operators),
                SimulationAuthority::SingleServer { server_id, epoch },
            ) => {
                if operators.len() < 2 {
                    return Err(DeploymentError::AtLeastTwoFederatedServersRequired);
                }
                if server_id.trim().is_empty()
                    || operators.iter().any(|(server, operator)| {
                        server.trim().is_empty() || operator.trim().is_empty()
                    })
                {
                    return Err(DeploymentError::EmptyIdentity);
                }
                if *epoch == 0 {
                    return Err(DeploymentError::NonzeroAuthorityEpochRequired);
                }
                if !operators.contains_key(server_id) {
                    return Err(DeploymentError::AxesDoNotDescribeASupportedProfile);
                }
                Ok(())
            }
            _ => Err(DeploymentError::AxesDoNotDescribeASupportedProfile),
        }
    }
}

/// State classes whose owner, storage, consistency, partition, and recovery
/// rules are specified in ADR-0013.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateClass {
    MembershipAndPresence,
    PendingInputs,
    ActiveSimulation,
    CommittedHistory,
    FederationDirectory,
    OperatorConfiguration,
    OperationsCatalog,
    GameContent,
    DerivedCache,
    MediaPlayout,
}

pub const STATE_CLASSES: [StateClass; 10] = [
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateOwner {
    PeerSet,
    InputSeatUntilCommit,
    SessionAuthority,
    SessionProcess,
    FederationOperators,
    LocalOperator,
    OperationsPlane,
    GameRelease,
    DerivedOnly,
    MediaSubsystem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotStore {
    NotRequired,
    RustProcessMemory,
    BeamProcessAndEts,
    VersionedLocalConfig,
    OptionalVeriSimDb,
    EtsOrDragonfly,
    BurbleLmdbRingBufferOrEts,
    VersionedPackageFiles,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Durability {
    NotRequired,
    Volatile,
    LocalAuthorityLmdbJournal,
    VersionedConfigFile,
    OptionalOperationsDatabase,
    VersionedPackageFile,
    SemanticallyDisposableMediaBuffer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheStore {
    None,
    OptionalEtsOrDragonfly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Consistency {
    BestEffortEphemeral,
    OrderedSeatInputs,
    DeterministicPeerLockstep,
    SingleAuthoritySequence,
    VersionedSingleWriter,
    EventualAdvisoryDirectory,
    EventualOperationsProjection,
    ImmutableContentHash,
    Recomputable,
    OrderedBoundedMediaFrames,
    NotApplicable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionBehavior {
    ExpireAndReannounce,
    HoldAtMissingWatermark,
    PauseAndResyncFromSurvivor,
    FailClosedWithoutAuthority,
    StaleDirectoryNeverPromotesAuthority,
    KeepLastValidatedConfigDenyEdits,
    ContinueGameplayWithoutOperationsPlane,
    UseInstalledContentOrRefuse,
    CacheMissAndReadSource,
    DropMediaFrames,
    NotRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recovery {
    RejoinAndReannounce,
    DropUncommittedAndResample,
    AdoptSurvivorSnapshotAndReplay,
    ReplaySameAuthorityJournal,
    ReannounceConfiguredNodes,
    ReloadAndReconcileConfig,
    RebuildProjectionFromLocalSource,
    FetchExactContentDigest,
    RecomputeFromAuthority,
    FlushAndRejoinMedia,
    NotRequired,
}

/// Normative, topology-specific contract for one state class. It is a policy
/// description only; no persistence or network adapter is implied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateContract {
    pub owner: StateOwner,
    pub hot_store: HotStore,
    pub durability: Durability,
    pub cache: CacheStore,
    pub consistency: Consistency,
    pub partition: PartitionBehavior,
    pub recovery: Recovery,
}

pub fn state_contract(class: StateClass, topology: NetworkTopology) -> StateContract {
    use self::CacheStore as Cache;
    use self::Consistency as C;
    use self::Durability as D;
    use self::HotStore as H;
    use self::PartitionBehavior as P;
    use self::Recovery as R;
    use self::StateOwner as O;

    match class {
        StateClass::MembershipAndPresence => StateContract {
            owner: if topology == NetworkTopology::PeerToPeer {
                O::PeerSet
            } else {
                O::SessionProcess
            },
            hot_store: if topology == NetworkTopology::PeerToPeer {
                H::RustProcessMemory
            } else {
                H::BeamProcessAndEts
            },
            durability: D::Volatile,
            // ETS is the volatile membership store on relay nodes, not a
            // second cache in front of an authoritative membership store.
            cache: Cache::None,
            consistency: C::BestEffortEphemeral,
            partition: P::ExpireAndReannounce,
            recovery: R::RejoinAndReannounce,
        },
        StateClass::PendingInputs => StateContract {
            owner: O::InputSeatUntilCommit,
            hot_store: H::RustProcessMemory,
            durability: D::Volatile,
            cache: Cache::None,
            consistency: C::OrderedSeatInputs,
            partition: P::HoldAtMissingWatermark,
            recovery: R::DropUncommittedAndResample,
        },
        StateClass::ActiveSimulation => {
            let peer = topology == NetworkTopology::PeerToPeer;
            StateContract {
                owner: if peer {
                    O::PeerSet
                } else {
                    O::SessionAuthority
                },
                hot_store: H::RustProcessMemory,
                durability: if peer {
                    D::Volatile
                } else {
                    D::LocalAuthorityLmdbJournal
                },
                cache: Cache::None,
                consistency: if peer {
                    C::DeterministicPeerLockstep
                } else {
                    C::SingleAuthoritySequence
                },
                partition: if peer {
                    P::PauseAndResyncFromSurvivor
                } else {
                    P::FailClosedWithoutAuthority
                },
                recovery: if peer {
                    R::AdoptSurvivorSnapshotAndReplay
                } else {
                    R::ReplaySameAuthorityJournal
                },
            }
        }
        StateClass::CommittedHistory => {
            let peer = topology == NetworkTopology::PeerToPeer;
            StateContract {
                owner: if peer {
                    O::PeerSet
                } else {
                    O::SessionAuthority
                },
                hot_store: H::RustProcessMemory,
                durability: if peer {
                    D::Volatile
                } else {
                    D::LocalAuthorityLmdbJournal
                },
                cache: Cache::None,
                consistency: if peer {
                    C::DeterministicPeerLockstep
                } else {
                    C::SingleAuthoritySequence
                },
                partition: if peer {
                    P::PauseAndResyncFromSurvivor
                } else {
                    P::FailClosedWithoutAuthority
                },
                recovery: if peer {
                    R::AdoptSurvivorSnapshotAndReplay
                } else {
                    R::ReplaySameAuthorityJournal
                },
            }
        }
        StateClass::FederationDirectory => {
            let federated = topology == NetworkTopology::Federated;
            StateContract {
                owner: if federated {
                    O::FederationOperators
                } else {
                    O::LocalOperator
                },
                hot_store: if federated {
                    H::VersionedLocalConfig
                } else {
                    H::NotRequired
                },
                durability: if federated {
                    D::VersionedConfigFile
                } else {
                    D::NotRequired
                },
                cache: if federated {
                    Cache::OptionalEtsOrDragonfly
                } else {
                    Cache::None
                },
                consistency: if federated {
                    C::EventualAdvisoryDirectory
                } else {
                    C::NotApplicable
                },
                partition: if federated {
                    P::StaleDirectoryNeverPromotesAuthority
                } else {
                    P::NotRequired
                },
                recovery: if federated {
                    R::ReannounceConfiguredNodes
                } else {
                    R::NotRequired
                },
            }
        }
        StateClass::OperatorConfiguration => StateContract {
            owner: if topology == NetworkTopology::PeerToPeer {
                O::PeerSet
            } else {
                O::LocalOperator
            },
            hot_store: H::VersionedLocalConfig,
            durability: D::VersionedConfigFile,
            cache: Cache::None,
            consistency: C::VersionedSingleWriter,
            partition: P::KeepLastValidatedConfigDenyEdits,
            recovery: R::ReloadAndReconcileConfig,
        },
        StateClass::OperationsCatalog => StateContract {
            owner: O::OperationsPlane,
            hot_store: if topology == NetworkTopology::PeerToPeer {
                H::NotRequired
            } else {
                H::OptionalVeriSimDb
            },
            durability: if topology == NetworkTopology::PeerToPeer {
                D::NotRequired
            } else {
                D::OptionalOperationsDatabase
            },
            cache: if topology == NetworkTopology::PeerToPeer {
                Cache::None
            } else {
                Cache::OptionalEtsOrDragonfly
            },
            consistency: if topology == NetworkTopology::PeerToPeer {
                C::NotApplicable
            } else {
                C::EventualOperationsProjection
            },
            partition: if topology == NetworkTopology::PeerToPeer {
                P::NotRequired
            } else {
                P::ContinueGameplayWithoutOperationsPlane
            },
            recovery: if topology == NetworkTopology::PeerToPeer {
                R::NotRequired
            } else {
                R::RebuildProjectionFromLocalSource
            },
        },
        StateClass::GameContent => StateContract {
            owner: O::GameRelease,
            hot_store: H::VersionedPackageFiles,
            durability: D::VersionedPackageFile,
            cache: Cache::None,
            consistency: C::ImmutableContentHash,
            partition: P::UseInstalledContentOrRefuse,
            recovery: R::FetchExactContentDigest,
        },
        StateClass::DerivedCache => StateContract {
            owner: O::DerivedOnly,
            hot_store: H::EtsOrDragonfly,
            durability: D::Volatile,
            cache: Cache::None,
            consistency: C::Recomputable,
            partition: P::CacheMissAndReadSource,
            recovery: R::RecomputeFromAuthority,
        },
        StateClass::MediaPlayout => StateContract {
            owner: O::MediaSubsystem,
            hot_store: H::BurbleLmdbRingBufferOrEts,
            durability: D::SemanticallyDisposableMediaBuffer,
            cache: Cache::None,
            consistency: C::OrderedBoundedMediaFrames,
            partition: P::DropMediaFrames,
            recovery: R::FlushAndRejoinMedia,
        },
    }
}

/// A stable per-session owner stamp used by the federation contract model.
/// It does not elect a leader or provide consensus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityStamp {
    pub server_id: String,
    pub epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPlacement {
    pub session_id: String,
    pub authority: AuthorityStamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCheckpoint {
    pub session_id: String,
    pub authority: AuthorityStamp,
    pub committed_through: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorityRoute {
    Reachable(AuthorityStamp),
    Unavailable(AuthorityStamp),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryError {
    SessionMismatch,
    AuthorityMismatch,
    SequenceExhausted,
}

/// Route to the *configured* session authority, or fail closed if that server
/// is not reachable. This intentionally never selects a substitute node.
pub fn route_authority(
    placement: &SessionPlacement,
    reachable_servers: &BTreeSet<String>,
) -> AuthorityRoute {
    if reachable_servers.contains(&placement.authority.server_id) {
        AuthorityRoute::Reachable(placement.authority.clone())
    } else {
        AuthorityRoute::Unavailable(placement.authority.clone())
    }
}

/// Restore the next sequence number from a checkpoint only when it belongs to
/// the same session and exact authority epoch. Actual storage and fencing are
/// supplied by the future hosted/federated server implementation.
pub fn recover_next_sequence(
    placement: &SessionPlacement,
    checkpoint: &SessionCheckpoint,
) -> Result<u64, RecoveryError> {
    if checkpoint.session_id != placement.session_id {
        return Err(RecoveryError::SessionMismatch);
    }
    if checkpoint.authority != placement.authority {
        return Err(RecoveryError::AuthorityMismatch);
    }
    checkpoint
        .committed_through
        .checked_add(1)
        .ok_or(RecoveryError::SequenceExhausted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(topology: NetworkTopology, gsa: GsaIntegration) -> DeploymentAxes {
        match topology {
            NetworkTopology::PeerToPeer => {
                let peers = BTreeSet::from(["peer-a".to_string(), "peer-b".to_string()]);
                DeploymentAxes {
                    topology,
                    administration: AdministrativeOwnership::Participants(peers.clone()),
                    authority: SimulationAuthority::PeerReplicas(peers),
                    gsa,
                }
            }
            NetworkTopology::Hosted => DeploymentAxes {
                topology,
                administration: AdministrativeOwnership::SingleOperator {
                    server_id: "host-a".into(),
                    operator_id: "operator-a".into(),
                },
                authority: SimulationAuthority::SingleServer {
                    server_id: "host-a".into(),
                    epoch: 1,
                },
                gsa,
            },
            NetworkTopology::Federated => DeploymentAxes {
                topology,
                administration: AdministrativeOwnership::FederatedOperators(BTreeMap::from([
                    ("server-a".into(), "operator-a".into()),
                    ("server-b".into(), "operator-b".into()),
                ])),
                authority: SimulationAuthority::SingleServer {
                    server_id: "server-a".into(),
                    epoch: 1,
                },
                gsa,
            },
        }
    }

    #[test]
    fn all_three_topologies_allow_gsa_present_or_absent_without_merging_axes() {
        for topology in [
            NetworkTopology::PeerToPeer,
            NetworkTopology::Federated,
            NetworkTopology::Hosted,
        ] {
            let without_gsa = profile(topology, GsaIntegration::Absent);
            let with_gsa = profile(topology, GsaIntegration::Present);
            assert_eq!(without_gsa.validate(), Ok(()));
            assert_eq!(with_gsa.validate(), Ok(()));
            assert_eq!(without_gsa.topology, with_gsa.topology);
            assert_eq!(without_gsa.administration, with_gsa.administration);
            assert_eq!(without_gsa.authority, with_gsa.authority);
        }
    }

    #[test]
    fn peer_administration_and_replica_identity_are_independent_axes() {
        let mut plan = profile(NetworkTopology::PeerToPeer, GsaIntegration::Absent);
        plan.administration = AdministrativeOwnership::Participants(BTreeSet::from([
            "player-alice".into(),
            "player-bob".into(),
        ]));
        plan.authority = SimulationAuthority::PeerReplicas(BTreeSet::from([
            "device-17".into(),
            "device-42".into(),
        ]));
        assert_eq!(plan.validate(), Ok(()));
    }

    #[test]
    fn peer_profile_has_no_central_storage_or_authority_dependency() {
        let plan = profile(NetworkTopology::PeerToPeer, GsaIntegration::Absent);
        assert_eq!(plan.validate(), Ok(()));

        let world = state_contract(StateClass::ActiveSimulation, plan.topology);
        assert_eq!(world.owner, StateOwner::PeerSet);
        assert_eq!(world.hot_store, HotStore::RustProcessMemory);
        assert_eq!(world.durability, Durability::Volatile);
        assert_eq!(world.cache, CacheStore::None);
        assert_eq!(world.consistency, Consistency::DeterministicPeerLockstep);
        assert_eq!(
            world.partition,
            PartitionBehavior::PauseAndResyncFromSurvivor
        );

        let history = state_contract(StateClass::CommittedHistory, plan.topology);
        assert_eq!(history.durability, Durability::Volatile);
        assert_eq!(history.hot_store, HotStore::RustProcessMemory);
        let directory = state_contract(StateClass::FederationDirectory, plan.topology);
        assert_eq!(directory.hot_store, HotStore::NotRequired);
        assert_eq!(directory.cache, CacheStore::None);
        let operations = state_contract(StateClass::OperationsCatalog, plan.topology);
        assert_eq!(operations.hot_store, HotStore::NotRequired);
        assert_eq!(operations.durability, Durability::NotRequired);
        assert_eq!(operations.cache, CacheStore::None);
    }

    #[test]
    fn hosted_profile_without_gsa_keeps_server_authority_and_local_durability() {
        let plan = profile(NetworkTopology::Hosted, GsaIntegration::Absent);
        assert_eq!(plan.validate(), Ok(()));
        assert!(matches!(
            &plan.authority,
            SimulationAuthority::SingleServer { server_id, .. } if server_id == "host-a"
        ));

        let live = state_contract(StateClass::ActiveSimulation, plan.topology);
        let history = state_contract(StateClass::CommittedHistory, plan.topology);
        assert_eq!(live.owner, StateOwner::SessionAuthority);
        assert_eq!(live.durability, Durability::LocalAuthorityLmdbJournal);
        assert_eq!(history.durability, Durability::LocalAuthorityLmdbJournal);
        assert_eq!(
            live.partition,
            PartitionBehavior::FailClosedWithoutAuthority
        );

        let with_gsa = profile(NetworkTopology::Hosted, GsaIntegration::Present);
        assert_eq!(
            state_contract(StateClass::ActiveSimulation, with_gsa.topology),
            live
        );
        assert_eq!(with_gsa.authority, plan.authority);
    }

    #[test]
    fn every_state_class_has_explicit_owner_storage_cache_consistency_partition_and_recovery() {
        for topology in [
            NetworkTopology::PeerToPeer,
            NetworkTopology::Federated,
            NetworkTopology::Hosted,
        ] {
            for class in STATE_CLASSES {
                let contract = state_contract(class, topology);
                let expected_owner = match class {
                    StateClass::MembershipAndPresence => {
                        if topology == NetworkTopology::PeerToPeer {
                            StateOwner::PeerSet
                        } else {
                            StateOwner::SessionProcess
                        }
                    }
                    StateClass::PendingInputs => StateOwner::InputSeatUntilCommit,
                    StateClass::ActiveSimulation | StateClass::CommittedHistory => {
                        if topology == NetworkTopology::PeerToPeer {
                            StateOwner::PeerSet
                        } else {
                            StateOwner::SessionAuthority
                        }
                    }
                    StateClass::FederationDirectory => {
                        if topology == NetworkTopology::Federated {
                            StateOwner::FederationOperators
                        } else {
                            StateOwner::LocalOperator
                        }
                    }
                    StateClass::OperatorConfiguration => {
                        if topology == NetworkTopology::PeerToPeer {
                            StateOwner::PeerSet
                        } else {
                            StateOwner::LocalOperator
                        }
                    }
                    StateClass::OperationsCatalog => StateOwner::OperationsPlane,
                    StateClass::GameContent => StateOwner::GameRelease,
                    StateClass::DerivedCache => StateOwner::DerivedOnly,
                    StateClass::MediaPlayout => StateOwner::MediaSubsystem,
                };
                assert_eq!(contract.owner, expected_owner, "{class:?} / {topology:?}");

                match class {
                    StateClass::PendingInputs => {
                        assert_eq!(contract.durability, Durability::Volatile);
                        assert_eq!(
                            contract.partition,
                            PartitionBehavior::HoldAtMissingWatermark
                        );
                        assert_eq!(contract.recovery, Recovery::DropUncommittedAndResample);
                    }
                    StateClass::ActiveSimulation | StateClass::CommittedHistory => {
                        let expected_durability = if topology == NetworkTopology::PeerToPeer {
                            Durability::Volatile
                        } else {
                            Durability::LocalAuthorityLmdbJournal
                        };
                        assert_eq!(contract.durability, expected_durability);
                    }
                    StateClass::DerivedCache => {
                        assert_eq!(contract.durability, Durability::Volatile);
                        assert_eq!(contract.consistency, Consistency::Recomputable);
                        assert_eq!(
                            contract.partition,
                            PartitionBehavior::CacheMissAndReadSource
                        );
                    }
                    StateClass::GameContent => {
                        assert_eq!(contract.durability, Durability::VersionedPackageFile);
                        assert_eq!(contract.consistency, Consistency::ImmutableContentHash);
                    }
                    StateClass::MembershipAndPresence => {
                        let expected_store = if topology == NetworkTopology::PeerToPeer {
                            HotStore::RustProcessMemory
                        } else {
                            HotStore::BeamProcessAndEts
                        };
                        assert_eq!(contract.hot_store, expected_store);
                        assert_eq!(contract.cache, CacheStore::None);
                        assert_eq!(contract.durability, Durability::Volatile);
                        assert_eq!(contract.consistency, Consistency::BestEffortEphemeral);
                    }
                    StateClass::FederationDirectory if topology == NetworkTopology::Federated => {
                        assert_eq!(
                            contract.partition,
                            PartitionBehavior::StaleDirectoryNeverPromotesAuthority
                        );
                    }
                    StateClass::OperationsCatalog if topology != NetworkTopology::PeerToPeer => {
                        assert_eq!(
                            contract.partition,
                            PartitionBehavior::ContinueGameplayWithoutOperationsPlane
                        );
                    }
                    StateClass::OperatorConfiguration
                    | StateClass::FederationDirectory
                    | StateClass::OperationsCatalog
                    | StateClass::MediaPlayout => {}
                }

                // All policy dimensions are explicit closed enums rather than
                // missing/implicit fields.
                let _ = (
                    contract.hot_store,
                    contract.cache,
                    contract.consistency,
                    contract.partition,
                    contract.recovery,
                );
            }
        }
    }

    #[test]
    fn federation_partition_is_fail_closed_and_same_authority_recovery_is_fenced() {
        let session_a = SessionPlacement {
            session_id: "match-a".into(),
            authority: AuthorityStamp {
                server_id: "server-a".into(),
                epoch: 7,
            },
        };
        let session_b = SessionPlacement {
            session_id: "match-b".into(),
            authority: AuthorityStamp {
                server_id: "server-b".into(),
                epoch: 3,
            },
        };
        let reachable = BTreeSet::from(["server-b".to_string()]);

        assert_eq!(
            route_authority(&session_a, &reachable),
            AuthorityRoute::Unavailable(session_a.authority.clone()),
            "server-b must not take over server-a's session during a partition"
        );
        assert_eq!(
            route_authority(&session_b, &reachable),
            AuthorityRoute::Reachable(session_b.authority.clone()),
            "sessions owned by a healthy server remain available"
        );

        let checkpoint = SessionCheckpoint {
            session_id: "match-a".into(),
            authority: session_a.authority.clone(),
            committed_through: 41,
        };
        assert_eq!(recover_next_sequence(&session_a, &checkpoint), Ok(42));

        let recovered_servers = BTreeSet::from(["server-a".to_string(), "server-b".to_string()]);
        assert_eq!(
            route_authority(&session_a, &recovered_servers),
            AuthorityRoute::Reachable(session_a.authority.clone()),
            "the original authority resumes routing after recovery"
        );

        let stale_epoch = SessionCheckpoint {
            authority: AuthorityStamp {
                server_id: "server-a".into(),
                epoch: 6,
            },
            ..checkpoint.clone()
        };
        assert_eq!(
            recover_next_sequence(&session_a, &stale_epoch),
            Err(RecoveryError::AuthorityMismatch)
        );

        let wrong_server = SessionCheckpoint {
            authority: session_b.authority.clone(),
            ..checkpoint.clone()
        };
        assert_eq!(
            recover_next_sequence(&session_a, &wrong_server),
            Err(RecoveryError::AuthorityMismatch)
        );

        let wrong_session = SessionCheckpoint {
            session_id: "match-b".into(),
            ..checkpoint
        };
        assert_eq!(
            recover_next_sequence(&session_a, &wrong_session),
            Err(RecoveryError::SessionMismatch)
        );
    }

    #[test]
    fn rejects_crossed_authority_and_administration_axes() {
        let mut hosted = profile(NetworkTopology::Hosted, GsaIntegration::Absent);
        hosted.authority = SimulationAuthority::SingleServer {
            server_id: "other-host".into(),
            epoch: 1,
        };
        assert_eq!(
            hosted.validate(),
            Err(DeploymentError::AxesDoNotDescribeASupportedProfile)
        );

        let mut peer = profile(NetworkTopology::PeerToPeer, GsaIntegration::Absent);
        peer.authority = SimulationAuthority::SingleServer {
            server_id: "host-a".into(),
            epoch: 1,
        };
        assert_eq!(
            peer.validate(),
            Err(DeploymentError::AxesDoNotDescribeASupportedProfile)
        );
    }
}

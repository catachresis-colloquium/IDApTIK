//! Host-invariant regression tests: an agent that proposes a prohibited
//! action must still be stopped by the production validator, and the
//! adapter's permission realms must not be interchangeable.
//!
//! These tests deliberately bypass the corpus scenarios and drive the
//! production surfaces directly so a weakening of one scenario cannot hide
//! a regression in the underlying guard.

use idaptik_evil_weevil::agent::{EvilWeevil, IntentKind};
use idaptik_evil_weevil::enforce::{EnforcementAction, EnforcementPolicy};
use idaptik_evil_weevil::harness::{IsolatedHarness, SeatPair, ValidatorOutcome, load_live_script};
use idaptik_evil_weevil::observer::DefensiveObserver;
use idaptik_evil_weevil::policy::{Permission, Principal};
use idaptik_evil_weevil::relay_model::RelayModel;
use idaptik_evil_weevil::telemetry::{FaultKind, TelemetryEvent, ViolationKind};
use idaptik_evil_weevil::wire;
use idaptik_core::scenario::command::Command;
use idaptik_core::scenario::tuning::ActionKind;
use idaptik_net::envelope::Role;
use idaptik_net::lockstep::LockstepCore;

fn harness() -> IsolatedHarness {
    IsolatedHarness::new("regression", 777_099)
}

#[test]
fn stale_frame_after_a_commit_is_rejected_by_the_host() {
    let mut h = harness();
    let script = load_live_script();
    let mut core = LockstepCore::new(Role::Infiltrator, 1, &script).unwrap();
    h.feed_commit(&mut core, Role::Hacker, 4).unwrap();

    let proposer = Principal::adversary(Role::Hacker, 9);
    let mut weevil = EvilWeevil::new(proposer.clone());
    let intent = weevil
        .propose(
            IntentKind::StaleReplay,
            wire::typed_command(2, &Command::Uplink {
                kind: ActionKind::Camera,
            }),
            "below the watermark",
        )
        .unwrap();
    h.record_intent(&intent);
    let outcome = h.feed_raw_to_validator(&mut core, &intent.wire);
    assert!(matches!(outcome, ValidatorOutcome::Rejected(_)));
    assert!(h.telemetry().iter().any(|t| matches!(
        t,
        TelemetryEvent::ValidatorRejected {
            violation: ViolationKind::StaleAfterCommit,
            ..
        }
    )));
    assert_eq!(core.step(), 0, "a rejected frame never advances the sim");
}

#[test]
fn forged_command_tags_and_shapeless_frames_never_decode() {
    let mut h = harness();
    let script = load_live_script();
    let mut core = LockstepCore::new(Role::Infiltrator, 1, &script).unwrap();

    let unknown = wire::unknown_command(0, "WarpToRoom");
    assert!(matches!(
        h.feed_raw_to_validator(&mut core, &unknown),
        ValidatorOutcome::Undecodable(_)
    ));
    let no_at = wire::command_without_at(serde_json::json!({ "cmd": "Jump" }));
    assert!(matches!(
        h.feed_raw_to_validator(&mut core, &no_at),
        ValidatorOutcome::Undecodable(_)
    ));
}

#[test]
fn the_relay_model_stops_role_misrouting_and_duplicate_seq() {
    let mut infiltrator = RelayModel::join("idaptik", Role::Infiltrator).unwrap();
    // A hacker verb from the infiltrator seat.
    let misrouted = wire::with_seq(wire::net_ssh(1, 0), 1);
    assert!(matches!(
        infiltrator.command(misrouted),
        idaptik_evil_weevil::relay_model::RelayVerdict::Rejected { .. }
    ));

    let mut hacker = RelayModel::join("idaptik", Role::Hacker).unwrap();
    let frame = wire::with_seq(wire::net_ssh(1, 0), 7);
    assert!(matches!(
        hacker.command(frame.clone()),
        idaptik_evil_weevil::relay_model::RelayVerdict::Relayed { .. }
    ));
    assert!(matches!(
        hacker.command(frame),
        idaptik_evil_weevil::relay_model::RelayVerdict::Dropped { .. }
    ));
}

#[test]
fn out_of_graph_targets_are_no_ops_that_never_panic() {
    let script = load_live_script();
    let probe = idaptik_tui::headless::build(&script).unwrap();
    let beyond = probe.graph().nodes.len() as u32 + 1_000;
    let (baseline, _, _) = IsolatedHarness::sim_tick(&script, &[]);
    let (attacked, events, _) = IsolatedHarness::sim_tick(&script, &[
        Command::NetSsh {
            node: idaptik_core::scenario::command::NetNodeIndex(beyond),
        },
        Command::NetHack {
            node: idaptik_core::scenario::command::NetNodeIndex(beyond),
        },
    ]);
    assert_eq!(baseline.state(), attacked.state());
    assert!(events.iter().all(|e| !matches!(
        e,
        idaptik_core::scenario::event::Event::PivotOpened { .. }
    )));
}

#[test]
fn a_resync_cannot_be_self_issued() {
    let mut h = harness();
    let mut pair = SeatPair::new(2, 2);
    let mut queue = std::collections::VecDeque::new();
    for _ in 0..3 {
        h.exchange_round(&mut pair, &mut queue, 8);
    }
    pair.infiltrator.on_peer_lost();
    let resync = pair.infiltrator.make_resync();
    assert!(LockstepCore::adopt_resync(Role::Infiltrator, 2, resync).is_err());
}

#[test]
fn permission_realms_are_not_interchangeable() {
    let mut h = harness();
    let weevil = Principal::adversary(Role::Hacker, 1);
    let observer = Principal::observer();
    let operator = Principal::operator();

    assert!(h.authorise(&weevil, Permission::ProposeBoundedIntent));
    for p in [
        Permission::DeliverThroughTestRelay,
        Permission::FeedProductionValidator,
        Permission::ReadValidatedTelemetry,
        Permission::ClassifyEvidence,
        Permission::ConfigureEnforcementPolicy,
        Permission::EndLocalSession,
    ] {
        assert!(!h.authorise(&weevil, p), "adversary granted {p:?}");
    }
    for p in [
        Permission::ReadValidatedTelemetry,
        Permission::ClassifyEvidence,
        Permission::RecommendOperatorAction,
    ] {
        assert!(h.authorise(&observer, p), "observer denied {p:?}");
    }
    assert!(!h.authorise(&observer, Permission::EndLocalSession));
    assert!(h.authorise(&operator, Permission::ConfigureEnforcementPolicy));
    assert!(!h.authorise(&operator, Permission::ProposeBoundedIntent));
}

#[test]
fn faults_are_never_hostile_and_enforcement_has_no_remote_arm() {
    let trace = vec![
        TelemetryEvent::Fault {
            kind: FaultKind::DelayedDelivery,
            detail: "jitter".into(),
        },
        TelemetryEvent::Fault {
            kind: FaultKind::SilenceGraceExpired,
            detail: "grace".into(),
        },
        TelemetryEvent::RelayDropped {
            reason: "stale_or_duplicate".into(),
        },
    ];
    let report = DefensiveObserver::evaluate(&trace);
    assert_eq!(report.counts.violation, 0);
    assert_eq!(
        DefensiveObserver::false_positives_on_faults(&trace, &report),
        0
    );

    let policy = EnforcementPolicy::observe_only();
    for recommendation in [
        idaptik_evil_weevil::observer::Recommendation::None,
        idaptik_evil_weevil::observer::Recommendation::LogOnly,
        idaptik_evil_weevil::observer::Recommendation::RequestResync,
        idaptik_evil_weevil::observer::Recommendation::FlagForOperatorReview,
    ] {
        // Exhaustive on EnforcementAction: adding a remote variant forces a
        // decision here and in the scenario's alphabet test.
        match policy.decide(recommendation) {
            EnforcementAction::None | EnforcementAction::LogOnly => {}
            EnforcementAction::RequestLocalResync
            | EnforcementAction::EndLocalSession
            | EnforcementAction::FlagForOperatorReview => {
                panic!("observe-only policy must never take protective action")
            }
        }
    }
}

#[test]
fn the_signal_relay_drift_remains_a_documented_finding() {
    // Pins the demonstrated cross-repository gap: burble's idaptik profile
    // has no Signal entry at the pinned revision. If burble adds it, this
    // test fails and ADR-0013's defect entry must be re-evaluated.
    assert_eq!(
        idaptik_evil_weevil::relay_model::RelayModel::client_tags_missing_from_relay(),
        vec!["Signal"]
    );
}

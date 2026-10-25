//! Scenario 6 — infrastructure and companion orders: seat rules, the relay
//! gap, and the observation trail.
//!
//! Invariant under test: the infrastructure verbs added by ADR-0014
//! (`Command::Mole`, and the electrical `Command::Power`) obey the same
//! acceptance rules as every other frame — the relay's role table is the
//! seat authority and fails closed on tags it does not carry, the lockstep
//! watermark rejects stale orders, an accepted order reaches the sim as a
//! typed command — and the consequences it sets off enter canonical state
//! (a `DiversionState`) and flow back through the production perception door
//! (`GhostLobbySim::actor_observations`) strictly as what an actor's own
//! senses can perceive, never as a leak of the hidden order. The
//! player-visible `Event::GuardObserved` announce path is additionally
//! gated on Crisis phase and hearing range; its positive case is proved by
//! idaptik-core's own `infra_observations` suite, while this scenario pins
//! the gates it cannot cross in a short lockstep run.
//!
//! Cross-repository finding, recorded not simulated: at the pinned burble
//! revision `command_roles()` carries neither `Mole` nor `Power`, so the
//! live rendezvous relay refuses both — cooperative play over the relay
//! needs the profile extended (`Mole` → infiltrator; `Power` carries its
//! seat per-op, which a tag-level table cannot express without splitting
//! the verb). Locally — the in-process two-seat pipeline the loopback gate
//! also drives — the orders route by the client seat table and run.

use super::{
    EvidenceStatus, InputSpec, Scenario, ScenarioResult, ScenarioSpec, TopologyMode, finalize,
};
use crate::agent::{EvilWeevil, IntentKind};
use crate::harness::{IsolatedHarness, ValidatorOutcome};
use crate::policy::{Permission, Principal};
use crate::relay_model::RelayModel;
use crate::telemetry::ViolationKind;
use crate::wire;
use idaptik_core::companion::MoleEvent;
use idaptik_core::scenario::DiversionState;
use idaptik_core::scenario::Phase;
use idaptik_core::scenario::command::{Command, MoleOp};
use idaptik_core::scenario::constants::MOLE_SYNTH_NOISE;
use idaptik_core::scenario::event::Event;
use idaptik_core::scenario::infra::ObservationKind;
use idaptik_core::scenario::power::sim::{PowerIndex, PowerOp};
use idaptik_core::scenario::signal::sim::Seat;
use idaptik_net::envelope::Role;
use idaptik_net::lockstep::{LockstepCore, ScriptFeed};

const ID: &str = "EW-06-infrastructure-orders-and-observations";

/// What the perception probes captured on the tick the synth diversion
/// played. All fields come from production surfaces: the phase and diversion
/// out of canonical [`idaptik_core::scenario::RuntimeState`], the observation
/// lists out of `GhostLobbySim::actor_observations` — the only door the
/// guard's own watch system uses.
#[derive(Debug)]
struct PerceptionCapture {
    phase: Phase,
    billy_x: f64,
    diversion: Option<DiversionState>,
    /// Observation kinds at Billy's actual position.
    at_billy: Vec<ObservationKind>,
    /// Observation kinds at a position inside the diversion's hearing range.
    in_earshot: Vec<ObservationKind>,
}

/// The fixed, bounded input sequence. Every frame is pre-declared; nothing
/// is discovered or generated at runtime.
fn inputs() -> Vec<InputSpec> {
    vec![
        InputSpec {
            step: 0,
            intent: "commit the infiltrator watermark through tick 1".into(),
            expected: "accepted; the frontier moves to tick 2".into(),
        },
        InputSpec {
            step: 1,
            intent: "Mole order proposed from the hacker seat, through the model relay".into(),
            expected: "relay refusal: the burble profile carries no Mole tag (fails closed)".into(),
        },
        InputSpec {
            step: 2,
            intent: "Power inspect order proposed from the hacker seat, through the model relay"
                .into(),
            expected: "relay refusal: the profile carries no Power tag either".into(),
        },
        InputSpec {
            step: 3,
            intent: "ForceCrisis (harness control) then the same Mole order on the \
                     infiltrator peer path, at the frontier, direct to the production validator"
                .into(),
            expected: "both accepted into the peer schedule; the floor tips to Crisis \
                       before the synth plays"
                .into(),
        },
        InputSpec {
            step: 4,
            intent: "commit the infiltrator horizon through tick 3, sample the hacker \
                     seat's own schedule, and advance in rounds"
                .into(),
            expected: "ticks 0-3 execute: the sim applies the order, a canonical diversion \
                       appears, and the production perception door carries it only inside \
                       hearing range"
                .into(),
        },
        InputSpec {
            step: 5,
            intent: "a Mole order below the committed watermark, direct to the validator".into(),
            expected: "lockstep rejection: stale after commit".into(),
        },
    ]
}

/// Scenario 6's corpus entry.
pub struct InfraOrdersScenario;

impl Scenario for InfraOrdersScenario {
    fn spec(&self) -> ScenarioSpec {
        ScenarioSpec {
            id: ID,
            title: "Infrastructure and companion orders under the acceptance rules",
            principal: Principal::adversary(Role::Hacker, 1).identity.label,
            permitted_operations: vec![Permission::ProposeBoundedIntent],
            invariant: "An infrastructure order is refused at the relay when the profile \
                        does not carry its tag, refused by the watermark when stale, and \
                        only an accepted order moves the sim — whose consequences return \
                        as actor observations, never as a leak of the order itself.",
            bounded_inputs: inputs(),
            expectation: "Both relay refusals are recorded as role violations; the frontier \
                          Mole order applies on the pump-house floor (a typed companion \
                          event, no denial) and lands as a canonical DiversionState; the \
                          production perception door (actor_observations) reports the \
                          UnusualNoise only inside the archetype's hearing range and nothing \
                          at Billy's out-of-range position, so the watch system emits no \
                          GuardObserved it cannot perceive; the stale order never re-enters \
                          the input set. The positive Crisis-plus-in-earshot announce path is \
                          covered by idaptik-core's infra_observations tests.",
            topology: TopologyMode::SingleRendezvousRelay,
            game_server_admin_present: false,
            evidence: EvidenceStatus::Demonstrated,
            replay_artifact: "result.json",
        }
    }

    fn run(&self) -> ScenarioResult {
        let spec = self.spec();
        let mut harness = IsolatedHarness::new(ID, 777_006);
        // The shared live-seat fixture, pointed at the pump-house floor by
        // its `scenario` field: the floor that hosts a mole. No
        // scenario-specific fixture copy that could drift.
        let mut script = crate::harness::load_live_script();
        script.scenario = Some("pump-house".to_owned());
        // The local seat is the hacker; the infiltrator is the peer whose
        // frames the adversary forges.
        let mut target =
            LockstepCore::new(Role::Hacker, 1, &script).expect("production core builds");
        let mut relay = RelayModel::join("idaptik", Role::Hacker).expect("model relay joins");
        let proposer = Principal::adversary(Role::Hacker, 1);
        let mut weevil = EvilWeevil::new(proposer.clone());

        // Permission separation is load-bearing: the agent cannot reach the
        // validator or the policy file directly.
        let adversary_propose_only = harness.authorise(&proposer, Permission::ProposeBoundedIntent)
            && !harness.authorise(&proposer, Permission::FeedProductionValidator)
            && !harness.authorise(&proposer, Permission::ConfigureEnforcementPolicy);
        harness.check(
            "permission:adversary-propose-only",
            adversary_propose_only,
            "adversary holds only ProposeBoundedIntent",
        );

        // 0. Move the peer (infiltrator) watermark to tick 2.
        harness
            .feed_commit(&mut target, Role::Infiltrator, 1)
            .expect("watermark commit accepted");

        // 1. A Mole order from the hacker seat, through the model relay:
        //    the burble profile has no Mole tag, so the relay refuses it —
        //    the documented cross-repository gap, failing closed.
        let mole_relayed = weevil
            .propose(
                IntentKind::RoleForbidden,
                wire::with_seq(
                    wire::typed_command(
                        2,
                        &Command::Mole {
                            op: MoleOp::Synth { index: 0 },
                        },
                    ),
                    0,
                ),
                "author a Mole order on the hacker seat and push it at the relay",
            )
            .unwrap();
        harness.record_intent(&mole_relayed);
        let mole_relay_outcome = harness.deliver_command(
            &mut relay,
            &mut target,
            &proposer,
            mole_relayed.wire.clone(),
        );
        let mole_gap_evidence = matches!(mole_relay_outcome, ValidatorOutcome::Rejected(_))
            && harness.telemetry().iter().any(|t| {
                matches!(
                    t,
                    crate::telemetry::TelemetryEvent::ValidatorRejected {
                        violation: ViolationKind::RoleForbidden,
                        ..
                    }
                )
            });
        harness.check(
            "mole-order-refused-at-relay",
            mole_gap_evidence,
            "the relay profile carries no Mole tag; the refusal fails closed",
        );

        // 2. A Power inspect order, same seat, same relay: refused the same
        //    way. The profile is tag-keyed, and Power's seat lives per-op —
        //    a tag-level table cannot route it without splitting the verb.
        let power_relayed = weevil
            .propose(
                IntentKind::RoleForbidden,
                wire::with_seq(
                    wire::typed_command(
                        3,
                        &Command::Power {
                            op: PowerOp::Inspect {
                                by: Seat::Infiltrator,
                                load: PowerIndex(0),
                            },
                        },
                    ),
                    1,
                ),
                "author a Power inspect on the hacker seat and push it at the relay",
            )
            .unwrap();
        harness.record_intent(&power_relayed);
        let power_relay_outcome = harness.deliver_command(
            &mut relay,
            &mut target,
            &proposer,
            power_relayed.wire.clone(),
        );
        harness.check(
            "power-order-refused-at-relay",
            matches!(power_relay_outcome, ValidatorOutcome::Rejected(_)),
            "the relay profile carries no Power tag either",
        );

        // 3. The same Mole order on its sanctioned path: authored for the
        //    infiltrator peer seat, at the frontier, straight to the
        //    production validator (the relay cannot carry it yet — that is
        //    the finding above, not a licence to smuggle it). First schedule
        //    the production ForceCrisis control at the same tick: the guard's
        //    announce-once watch path only runs in Crisis, so without it the
        //    observation contract below could never be exercised. This is a
        //    harness control, not an adversarial intent — Evil Weevil never
        //    proposes it and it does not spend the agent's intent budget.
        let crisis = wire::with_seq(wire::typed_command(2, &Command::ForceCrisis), 4);
        let crisis_outcome = harness.feed_raw_to_validator(&mut target, &crisis);
        harness.check(
            "force-crisis-accepted",
            matches!(crisis_outcome, ValidatorOutcome::Accepted),
            "the production validator schedules ForceCrisis at the frontier",
        );
        let mole_peer = weevil
            .propose(
                IntentKind::InvalidTargetClaim,
                wire::with_seq(
                    wire::typed_command(
                        2,
                        &Command::Mole {
                            op: MoleOp::Synth { index: 0 },
                        },
                    ),
                    2,
                ),
                "the identical Mole order at the frontier on the infiltrator peer path",
            )
            .unwrap();
        harness.record_intent(&mole_peer);
        let peer_outcome = harness.feed_raw_to_validator(&mut target, &mole_peer.wire);
        harness.check(
            "frontier-mole-order-accepted",
            matches!(peer_outcome, ValidatorOutcome::Accepted),
            "the lockstep validator accepts the frontier order for the peer seat",
        );

        // Commit the executed horizon so the run may proceed through the
        // tick the order lives on: advance stops at the peer watermark, and
        // a scheduled-but-uncommitted frame never executes.
        harness
            .feed_commit(&mut target, Role::Infiltrator, 3)
            .expect("horizon commit accepted");

        // Run the ready steps, collecting events; on the tick the synth
        // plays, sample the perception contract through the production door
        // (`GhostLobbySim::actor_observations`), the same call the guard's
        // watch system makes. Sampling the seat's own script horizon first is
        // what lets the run proceed: advance stops at either watermark, and
        // the sampling horizon only moves in pump_outgoing — the same rounds
        // the observations scenario runs. Two rounds carry the run past the
        // order's tick.
        let mut feed = ScriptFeed::new(&script, Role::Hacker);
        let mut events: Vec<Event> = Vec::new();
        let mut capture: Option<PerceptionCapture> = None;
        for _round in 0..3 {
            let _ = target.pump_outgoing(&mut feed);
            target.advance_with(|sim, fresh| {
                events.extend(fresh.iter().cloned());
                let synth_now = fresh.iter().any(|e| {
                    matches!(
                        e,
                        Event::Mole {
                            mole: MoleEvent::SynthSoundPlayed { .. }
                        }
                    )
                });
                if synth_now && capture.is_none() {
                    let st = sim.state();
                    // The same production call the watch system uses: it
                    // applies the archetype's own hearing and the hosting
                    // seam's extra_noise (the live diversion) to canonical
                    // state, so the hidden order never leaks as world truth.
                    let at_billy = sim
                        .actor_observations(st.billy.x)
                        .into_iter()
                        .map(|o| o.kind)
                        .collect();
                    // An actor standing 100px short of the diversion is
                    // inside the 260px hearing range; the noise intensity
                    // (MOLE_SYNTH_NOISE) clears the archetype threshold.
                    let in_earshot_x = st
                        .diversion
                        .as_ref()
                        .map(|d| d.x - 100.0)
                        .unwrap_or(st.billy.x);
                    let in_earshot = sim
                        .actor_observations(in_earshot_x)
                        .into_iter()
                        .map(|o| o.kind)
                        .collect();
                    capture = Some(PerceptionCapture {
                        phase: st.phase,
                        billy_x: st.billy.x,
                        diversion: st.diversion.clone(),
                        at_billy,
                        in_earshot,
                    });
                }
            });
            if capture.is_some() {
                break;
            }
        }
        let capture = capture.expect("the synth tick executed and produced a perception capture");

        let applied = events.iter().any(|e| {
            matches!(
                e,
                Event::Mole {
                    mole: MoleEvent::SynthSoundPlayed { .. }
                }
            )
        });
        harness.check(
            "mole-order-applied-by-sim",
            applied,
            "the accepted order reached the hosted companion as a typed event",
        );
        harness.check(
            "no-denial-for-legitimate-order",
            !events.iter().any(|e| matches!(e, Event::MoleDenied { .. })),
            "the pump-house floor hosts the mole; the legitimate order was not refused",
        );

        // The observation trail, checked against the production architecture
        // rather than an imagined one. Two facts make the player-visible
        // `Event::GuardObserved` unreachable in this short lockstep run:
        //   1. the watch system announces only in Crisis (forced above via a
        //      production control command), and
        //   2. it announces only what `actor_observations` carries at Billy's
        //      own position, and the mole spawns 364px away — beyond the
        //      archetype's 260px hearing range.
        // What IS demonstrable through this pipeline, and checked here:
        let in_crisis = capture.phase == Phase::Crisis;
        harness.check(
            "floor-entered-crisis-through-production-command",
            in_crisis,
            "ForceCrisis, fed through the lockstep validator, tips the phase to Crisis",
        );
        let diversion_is_canonical = capture.diversion.as_ref().is_some_and(|d| {
            d.intensity == MOLE_SYNTH_NOISE && d.t > 0.0
        });
        harness.check(
            "synth-diversion-enters-canonical-state",
            diversion_is_canonical,
            "the hidden order's consequence is a canonical DiversionState (mole position, \
             MOLE_SYNTH_NOISE intensity, bounded window), never a leak of the order itself",
        );
        // The production perception door, sampled the same way the guard's
        // watch system samples it: the noise is present for an actor inside
        // earshot and absent at Billy's actual out-of-range position — the
        // hearing gate, not omniscience.
        let door_carries_the_noise =
            capture.in_earshot.contains(&ObservationKind::UnusualNoise)
                && !capture.at_billy.contains(&ObservationKind::UnusualNoise);
        harness.check(
            "perception-door-gates-the-diversion-by-earshot",
            door_carries_the_noise,
            format!(
                "actor_observations in-earshot={:?}, at billy.x={:.1}: {:?} — the UnusualNoise \
                 is carried by the production door only inside the 260px hearing range",
                capture.in_earshot, capture.billy_x, capture.at_billy
            ),
        );
        // The announce-once record cannot invent a perception: while the
        // diversion is outside Billy's earshot, no GuardObserved(UnusualNoise)
        // appears even though the floor is in Crisis. The positive announce
        // path (Crisis + in-earshot) is proved by idaptik-core's own
        // infra_observations suite, which drives Billy next to the diversion.
        let announced = events.iter().any(|e| {
            matches!(
                e,
                Event::GuardObserved {
                    kind: ObservationKind::UnusualNoise,
                    ..
                }
            )
        });
        harness.check(
            "guard-announce-respects-the-earshot-gate",
            !announced,
            "no GuardObserved(UnusualNoise) while the diversion is outside Billy's hearing \
             range: the watch system reports perceptions, never world truth",
        );

        // 5. A stale Mole order below the watermark: the acceptance rules
        //    make no exception for the new verbs.
        let stale = weevil
            .propose(
                IntentKind::StaleReplay,
                wire::with_seq(
                    wire::typed_command(1, &Command::Mole { op: MoleOp::Dig }),
                    3,
                ),
                "replay a Mole order scheduled below the committed watermark",
            )
            .unwrap();
        harness.record_intent(&stale);
        let stale_outcome = harness.feed_raw_to_validator(&mut target, &stale.wire);
        let stale_evidence = matches!(stale_outcome, ValidatorOutcome::Rejected(_))
            && harness.telemetry().iter().any(|t| {
                matches!(
                    t,
                    crate::telemetry::TelemetryEvent::ValidatorRejected {
                        violation: ViolationKind::StaleAfterCommit,
                        ..
                    }
                )
            });
        harness.check(
            "stale-mole-order-rejected",
            stale_evidence,
            "on_peer_command refuses a Mole order below the watermark",
        );

        harness.check(
            "relay-accounting",
            relay.rejected_count == 2 && relay.relayed_count == 0 && relay.dropped_count == 0,
            format!(
                "relayed={}, dropped={}, rejected={}",
                relay.relayed_count, relay.dropped_count, relay.rejected_count
            ),
        );

        weevil.retire();
        let notes = vec![
            "Cross-repository gap (recorded, not simulated): burble's idaptik profile needs \
             command_roles() += Mole -> infiltrator before cooperative play carries companion \
             orders over the live relay; Power additionally carries its seat per-op, which a \
             tag-keyed table cannot express without splitting the verb. Until then the relay \
             fails closed, which is the safe direction."
                .into(),
            "The observation evidence is read through the production door \
             GhostLobbySim::actor_observations, which is also the only input to the guard's \
             announce-once watch system: an in-earshot position carries UnusualNoise while \
             Billy's actual out-of-range position (the mole spawns 364px away, beyond the \
             260px hearing range) carries none, so no GuardObserved is invented. The positive \
             Crisis-plus-in-earshot announcement is demonstrated in idaptik-core's \
             infra_observations tests against the same pump-house floor."
                .into(),
            "The run rides the shared live-seat fixture with its scenario field pointed at \
             pump-house; no scenario-specific fixture copy exists to drift."
                .into(),
        ];
        finalize(spec, &mut harness, notes)
    }
}

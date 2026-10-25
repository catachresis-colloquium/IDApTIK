//! Scenario 6 — infrastructure and companion orders: seat rules, the relay
//! gap, and the observation trail.
//!
//! Invariant under test: the infrastructure verbs added by ADR-0014
//! (`Command::Mole`, and the electrical `Command::Power`) obey the same
//! acceptance rules as every other frame — the relay's role table is the
//! seat authority and fails closed on tags it does not carry, the lockstep
//! watermark rejects stale orders, an accepted order reaches the sim as a
//! typed command — and the consequences it sets off come back through the
//! canonical event log *as observations* (`Event::GuardObserved`) computed
//! by the actor's own senses, never as a leak of the hidden order.
//!
//! Two preconditions decide whether that second half can be observed at all,
//! and the run states both rather than assuming them: the guard has to be on
//! shift (Billy is `Offsite` until a crisis, and an off-site guard hears
//! nothing), and the noise has to land inside his hearing radius at the
//! position the crisis assigns him. A diversion played at the mole's spawn is
//! 364 units away against a 260-unit gate: audible in the abstract, never
//! heard in fact.
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
use idaptik_core::scenario::command::{Command, MoleOp};
use idaptik_core::scenario::common::BillyMode;
use idaptik_core::scenario::event::Event;
use idaptik_core::scenario::infra::ObservationKind;
use idaptik_core::scenario::power::sim::{PowerIndex, PowerOp};
use idaptik_core::scenario::signal::sim::Seat;
use idaptik_net::envelope::Role;
use idaptik_net::lockstep::{LockstepCore, ScriptFeed};

const ID: &str = "EW-06-infrastructure-orders-and-observations";

/// The tick the diversion is played on. Late enough that the mole has walked
/// from its spawn (390) into the guard's hearing at 240: 150 units at the
/// companion's 200 px/s underground is 0.75 s, and `TICK_DT` is 1/60.
const SYNTH_AT: u64 = 52;

/// The tick the scene is set on. Not tick 1: a commit *through* `n` advances
/// the peer watermark to `n + 1`, so the opening `feed_commit(.., 1)` in this
/// scenario would reject a frame for tick 1 as arriving after its own commit.
/// Tick 2 is the first the watermark permits.
const SCENE_AT: u64 = 2;

/// The peer watermark committed before the run is driven. Past the synth, so
/// the tick the diversion plays on is provably uncommitted until now and then
/// executed.
const HORIZON: u64 = SYNTH_AT + 4;

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
            intent: "the same Mole order on the infiltrator peer path, at the frontier, \
                     direct to the production validator"
                .into(),
            expected: "accepted into the peer schedule".into(),
        },
        InputSpec {
            step: 4,
            intent: "three frontier scene frames for the infiltrator seat: force the \
                     crisis at tick 1, walk the mole underground to 240, and schedule \
                     the synth for tick 52"
                .into(),
            expected: "all accepted: the guard is on shift and the diversion is inside \
                       his hearing rather than 364 units outside it"
                .into(),
        },
        InputSpec {
            step: 5,
            intent: "commit the infiltrator horizon through SYNTH_AT + 4, sample the hacker \
                     seat's own schedule, and advance in rounds"
                .into(),
            expected: "the ordered ticks execute: the sim applies the synth and the guard \
                       announces the noise he actually heard"
                .into(),
        },
        InputSpec {
            step: 6,
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
                          event, no denial); with the guard on shift and the mole walked \
                          into earshot, Billy's own senses turn the synth diversion into a \
                          GuardObserved(UnusualNoise) event and the same observations list \
                          it; the stale order never re-enters the input set.",
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
        //
        // Two edits to the shared fixture, both deliberate and both narrow:
        //
        // * the horizon is raised, because the diversion has to be played
        //   where the guard can hear it (see the timing note below), and
        // * the fixture's own tail is dropped. Its `at: 40` line is
        //   `force_extract`, which would end the mission at tick 40 and stop
        //   the run before the synth plays, and its `at: 6`/`at: 10` pause and
        //   resume would stall the timeline this run has to advance through.
        //   The `at: 26` line is kept because it is the `hold: []` release:
        //   without it the opening `hold right` would run for the whole
        //   horizon. Seed, difficulty and the opening movement are unchanged,
        //   so the provenance is still the shared fixture, not a private copy.
        let mut script = crate::harness::load_live_script();
        script.scenario = Some("pump-house".to_owned());
        script.max_ticks = 90;
        script.commands.retain(|line| line.at < 5 || line.at == 26);
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
        //    the finding above, not a licence to smuggle it).
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

        // Scene-setting peer frames. Two things have to be true before a
        // guard can report anything at all, and neither is true at tick 0:
        //
        // * **Somebody is on shift.** Billy starts every run `Offsite`, and
        //   `begin_crisis` is the only exit from that mode. `Offsite` is the
        //   watcher's gate -- an off-site guard hears nothing -- so without a
        //   crisis the run has nobody to speak for the plant, however loud it
        //   is. This frame is authored for the infiltrator seat, which is what
        //   `seat_for` permits for a test hook.
        // * **The diversion is within earshot of where the crisis puts him.**
        //   `begin_crisis` assigns `CRISIS_BILLY_X` (26) and the mole spawns at
        //   390. The hearing gate is `NOISE_INVEST_DIST` (260), and 364 > 260,
        //   so a synth played from the spawn point is inaudible from the
        //   position the crisis is about to assign -- the order would run, the
        //   noise would exist, and no observation would ever reach the guard.
        //   240 is 214 units from Billy and 150 from the spawn: 0.75 s of
        //   underground travel at the companion's 200 px/s, which is why the
        //   synth below is not played at the frontier.
        for (seq, at, cmd) in [
            (4, SCENE_AT, Command::ForceCrisis),
            (
                5,
                SCENE_AT,
                Command::Mole {
                    op: MoleOp::MoveTo {
                        x: 240.0,
                        underground: true,
                    },
                },
            ),
            (
                6,
                SYNTH_AT,
                Command::Mole {
                    op: MoleOp::Synth { index: 0 },
                },
            ),
        ] {
            let frame = wire::with_seq(wire::typed_command(at, &cmd), seq);
            let outcome = harness.feed_raw_to_validator(&mut target, &frame);
            harness.check(
                &format!("scene-frame-accepted-at-{at}"),
                matches!(outcome, ValidatorOutcome::Accepted),
                format!("the frontier frame for tick {at} must schedule: {cmd:?}"),
            );
        }

        // Commit the executed horizon so the run may proceed: advance stops at
        // the peer watermark, and a scheduled-but-uncommitted frame never
        // executes. The horizon covers the tick the synth plays on.
        harness
            .feed_commit(&mut target, Role::Infiltrator, HORIZON)
            .expect("horizon commit accepted");

        // Run the ready steps, collecting events; on the tick the synth plays,
        // read the perception contract where Billy stands. Sampling the seat's
        // own script horizon first is what lets the run proceed: advance stops
        // at either watermark, and the sampling horizon only moves in
        // pump_outgoing -- the same rounds the observations scenario runs. The
        // loop ratchets towards SYNTH_AT two ticks at a time and stops as soon
        // as the diversion has been heard.
        let mut feed = ScriptFeed::new(&script, Role::Hacker);
        let mut events: Vec<Event> = Vec::new();
        let mut obs_at_billy: Option<(f64, Vec<ObservationKind>)> = None;
        let mut guard_on_shift = false;
        for _round in 0..200 {
            if obs_at_billy.is_some() || target.finished() {
                break;
            }
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
                if synth_now && obs_at_billy.is_none() {
                    let st = sim.state();
                    guard_on_shift = st.billy.mode != BillyMode::Offsite;
                    // The detector the watcher itself uses: canonical
                    // observations at the actor's own position, including the
                    // companion's noise. Sampling it through the sim rather
                    // than re-deriving it here is what stops this assertion
                    // drifting away from `system_infra_watch` again.
                    obs_at_billy = Some((
                        st.billy.x,
                        sim.actor_observations(st.billy.x)
                            .into_iter()
                            .map(|o| o.kind)
                            .collect(),
                    ));
                }
            });
        }
        harness.check(
            "guard-on-shift-when-the-synth-plays",
            guard_on_shift,
            "the diversion has to be played where the guard can hear it; an \
             off-site Billy is not a witness to anything",
        );

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

        // The observation trail: Billy heard the diversion through his own
        // senses — the player-visible record is the GuardObserved event, and
        // the perception contract lists the same noise where he stands.
        let observed = events.iter().any(|e| {
            matches!(
                e,
                Event::GuardObserved {
                    kind: ObservationKind::UnusualNoise,
                    ..
                }
            )
        });
        harness.check(
            "guard-observed-the-diversion",
            observed,
            "the synth diversion came back as an UnusualNoise observation event",
        );
        let contract_lists_noise = obs_at_billy
            .as_ref()
            .is_some_and(|(_, kinds)| kinds.contains(&ObservationKind::UnusualNoise));
        harness.check(
            "perception-contract-lists-the-noise",
            contract_lists_noise,
            match &obs_at_billy {
                Some((x, kinds)) => format!(
                    "observations at billy.x={x:.1}: {kinds:?} — computed from canonical \
                     state at the actor's own position"
                ),
                None => "the run never reached the tick the synth played".to_owned(),
            },
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
            "The observation evidence is the player-visible pair: Event::GuardObserved in the \
             canonical log (rendered as Billy's own words) and the observations() contract \
             evaluated at Billy's position with the archetype's hearing — the guard reacts to \
             what it perceives, never to the hidden Mole order."
                .into(),
            "The run still rides the shared live-seat fixture, with its scenario field \
             pointed at pump-house; it is not a private copy. Two fields are adjusted in \
             place and both are visible here: the horizon is raised, because the diversion \
             has to be played after the mole has walked into earshot, and the fixture's \
             tail is dropped because its at: 40 force_extract would end the mission before \
             the synth plays."
                .into(),
        ];
        finalize(spec, &mut harness, notes)
    }
}

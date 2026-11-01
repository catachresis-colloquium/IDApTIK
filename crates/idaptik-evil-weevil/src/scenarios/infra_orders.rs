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
//! nothing), and the noise has to land inside his hearing radius where he
//! actually stands when the synth plays. The second one is the trap: the
//! crisis *assigns* Billy to `CRISIS_BILLY_X`, but his FSM leaves that point
//! immediately (Entering walks him to his seed-rolled snack, then patrol),
//! so the position at the synth tick is a property of this run's trajectory,
//! not of the assignment. The run therefore shadows the same deterministic
//! sim (same seed, script, crisis frame and frontier acceptance probe) and
//! walks the mole into the intersection of his hearing radius and the mole's
//! reachable range. The later move is silent underground travel; the shadow
//! includes the earlier synth probe rather than assuming it cannot be heard.
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
use idaptik_core::scenario::infra::{Observation, ObservationKind};
use idaptik_core::scenario::power::sim::{PowerIndex, PowerOp};
use idaptik_core::scenario::signal::sim::Seat;
use idaptik_net::envelope::Role;
use idaptik_net::lockstep::{LockstepCore, ScriptFeed};

const ID: &str = "EW-06-infrastructure-orders-and-observations";

/// The tick the diversion is played on. The mole walks from its spawn
/// (390) at 200 px/s underground (3.33 px/tick), so with the scene set at
/// tick 2 it has 79 ticks before the synth — 263 px, which covers the
/// worst case of 250 px (the inner edge of Billy's 260-unit hearing
/// radius) plus a 4-tick stand margin. The target itself is not a
/// constant: the crisis puts Billy at 26 and then his FSM walks him (to
/// his snack, then on patrol), so the run shadows his trajectory first
/// and walks the mole into his earshot wherever that leaves him.
const SYNTH_AT: u64 = 81;

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
                     crisis at tick 2, walk the mole underground into Billy's \
                     shadow-computed earshot, and schedule the synth for tick 81"
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

/// Read-only evidence captured immediately after the scene's synth command.
/// The observation is matched to the canonical diversion, not merely to the
/// `UnusualNoise` tag: an earlier probe or an unrelated alarm must not satisfy
/// the observation trail for this order (issue #191).
struct SynthWitness {
    billy_x: f64,
    hearing_dist: f64,
    hearing_threshold: f64,
    diversion_x: f64,
    diversion_intensity: f64,
    on_shift: bool,
    observations: Vec<Observation>,
    /// Audible unusual-noise observations in the diversion's room. The event
    /// vocabulary carries only kind + room and the watcher deduplicates on
    /// that key, so attribution is sound only when this count is one.
    room_noise_sources: usize,
    noise_announcements: usize,
}

impl SynthWitness {
    fn heard_diversion(&self) -> Option<&Observation> {
        self.observations.iter().find(|o| {
            o.kind == ObservationKind::UnusualNoise
                && o.x == self.diversion_x
                && o.intensity == self.diversion_intensity
        })
    }
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
                          into earshot, Billy's own senses turn that synth diversion into a \
                          GuardObserved(UnusualNoise) event on the same tick, and the perception \
                          contract lists a noise matching its source; the earlier acceptance \
                          probe is not a witness, and the stale order never re-enters the input set.",
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
        // * **The diversion is within earshot of where Billy actually is at
        //   the synth tick.** Not where the crisis puts him: the moment
        //   `begin_crisis` assigns `CRISIS_BILLY_X` (26), the guard's FSM
        //   leaves that point (Entering walks him to his snack, then patrol
        //   takes over), so by `SYNTH_AT` he is elsewhere. Replay the exact
        //   pre-diversion prefix in a shadow: same seed, script, frontier
        //   acceptance probe and crisis frame, in the same merge order. Do
        //   not omit the probe on the assumption that it is inaudible -- that
        //   is a property to check, not a premise for the prediction. Only
        //   the later silent underground MoveTo and the scene synth itself
        //   are omitted; neither precedes the position being predicted.
        let mut shadow = LockstepCore::new(Role::Hacker, 1, &script).expect("shadow core builds");
        shadow
            .on_peer_commit(1)
            .expect("shadow: opening commit accepted");
        let (probe_at, probe_cmd) = idaptik_net::envelope::decode_command(&mole_peer.wire)
            .expect("accepted frontier probe decodes");
        shadow
            .on_peer_command(probe_at, probe_cmd)
            .expect("shadow: frontier probe accepted");
        let crisis_frame = wire::with_seq(wire::typed_command(SCENE_AT, &Command::ForceCrisis), 4);
        let (at, cmd) =
            idaptik_net::envelope::decode_command(&crisis_frame).expect("crisis frame decodes");
        shadow
            .on_peer_command(at, cmd)
            .expect("shadow: crisis accepted");
        shadow
            .on_peer_commit(SYNTH_AT + 4)
            .expect("shadow: horizon commit accepted");
        let mut shadow_feed = ScriptFeed::new(&script, Role::Hacker);
        let mut billy_at_synth: Option<f64> = None;
        for _ in 0..=(SYNTH_AT + 2) {
            if billy_at_synth.is_some() || shadow.finished() {
                break;
            }
            let _ = shadow.pump_outgoing(&mut shadow_feed);
            shadow.advance_with(|sim, _| {
                if sim.current_tick() == SYNTH_AT + 1 {
                    billy_at_synth = Some(sim.state().billy.x);
                }
            });
        }
        let billy_at_synth: f64 = billy_at_synth.expect("shadow reached the synth tick");

        // The diversion target: inside the 260-unit hearing gate (the gate
        // is strict, so keep a 40-unit corridor and stand at its centre) and
        // reachable from the spawn (390) in at most SYNTH_AT - SCENE_AT - 4
        // ticks at 200 px/s, the 4 ticks being a stand margin. The
        // intersection is never empty while Billy is on this floor, but it
        // can be degenerate if the tuning drifts far enough -- fail loudly
        // with the geometry rather than as a silent empty observation list.
        // Restated, next to the math it guards: the hearing gate is
        // NOISE_INVEST_DIST (260, strict) and the mole's underground speed
        // is 200 px/s on a 1/60-s tick.
        const HEARING: f64 = 260.0;
        const WALK_PX_PER_TICK: f64 = 200.0 / 60.0;
        const WALK_MARGIN_TICKS: u64 = 4;
        let max_walk_px = (SYNTH_AT - SCENE_AT - WALK_MARGIN_TICKS) as f64 * WALK_PX_PER_TICK;
        let reachable_lo = 390.0 - max_walk_px;
        let lo = reachable_lo.max(billy_at_synth - HEARING);
        let hi = 390.0_f64.min(billy_at_synth + HEARING);
        let corridor = hi - lo;
        let mole_target = (lo + hi) / 2.0;
        harness.check(
            "diversion-target-inside-audible-reachable-corridor",
            corridor >= 40.0 && (mole_target - billy_at_synth).abs() <= HEARING - 20.0,
            format!(
                "Billy is {billy_at_synth:.1} at the synth tick; audible band \
                 [{:.1}, {:.1}], reachable band [{:.1}, 390.0], corridor {:.1} units",
                billy_at_synth - HEARING,
                billy_at_synth + HEARING,
                reachable_lo,
                corridor
            ),
        );

        for (seq, at, cmd) in [
            (4, SCENE_AT, Command::ForceCrisis),
            (
                5,
                SCENE_AT,
                Command::Mole {
                    op: MoleOp::MoveTo {
                        x: mole_target,
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
        // loop ratchets towards SYNTH_AT two ticks at a time and stops after
        // sampling the scene synth, whether or not Billy could hear it.
        let mut feed = ScriptFeed::new(&script, Role::Hacker);
        let mut events: Vec<Event> = Vec::new();
        let mut witness: Option<SynthWitness> = None;
        for _round in 0..200 {
            if witness.is_some() || target.finished() {
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
                // The frontier acceptance probe also plays a synth at tick 2.
                // Only the scene command at SYNTH_AT is the subject of the
                // observation trail. advance_with calls us after the tick, so
                // the scene's command tick is SYNTH_AT + 1 here.
                if synth_now && witness.is_none() && sim.current_tick() == SYNTH_AT + 1 {
                    let st = sim.state();
                    if let Some(diversion) = st.diversion.as_ref() {
                        // The detector the watcher itself uses: canonical
                        // observations at Billy's own position, including
                        // companion noise. The free observations() function
                        // needs extra_noise supplied explicitly; the sim
                        // method is the complete perception contract.
                        let observations = sim.actor_observations(st.billy.x);
                        let source = observations.iter().find(|o| {
                            o.kind == ObservationKind::UnusualNoise
                                && o.x == diversion.x
                                && o.intensity == diversion.intensity
                        });
                        let room_noise_sources = source.map_or(0, |s| {
                            observations
                                .iter()
                                .filter(|o| {
                                    o.kind == ObservationKind::UnusualNoise && o.room == s.room
                                })
                                .count()
                        });
                        let noise_announcements = fresh
                            .iter()
                            .filter(|e| match e {
                                Event::GuardObserved {
                                    kind: ObservationKind::UnusualNoise,
                                    room,
                                } => source.is_some_and(|o| o.room == *room),
                                _ => false,
                            })
                            .count();
                        witness = Some(SynthWitness {
                            billy_x: st.billy.x,
                            hearing_dist: sim.actor().stats.invest_dist,
                            hearing_threshold: sim.actor().stats.invest_noise,
                            diversion_x: diversion.x,
                            diversion_intensity: diversion.intensity,
                            on_shift: st.billy.mode != BillyMode::Offsite,
                            observations,
                            room_noise_sources,
                            noise_announcements,
                        });
                    }
                }
            });
        }
        harness.check(
            "mole-order-applied-by-sim",
            witness.is_some(),
            format!(
                "SynthSoundPlayed and its canonical diversion were sampled after command \
                 tick {SYNTH_AT}, not after the earlier acceptance probe"
            ),
        );
        harness.check(
            "guard-on-shift-when-the-synth-plays",
            witness.as_ref().is_some_and(|w| w.on_shift),
            "an off-site Billy is not a witness to anything",
        );
        harness.check(
            "shadow-position-matches-billy-at-synth",
            witness
                .as_ref()
                .is_some_and(|w| w.billy_x == billy_at_synth),
            match &witness {
                Some(w) => format!(
                    "shadow predicted billy.x={billy_at_synth:.1}; production run has {:.1}",
                    w.billy_x
                ),
                None => "the scene synth was never sampled".to_owned(),
            },
        );
        harness.check(
            "diversion-source-within-billys-hearing",
            witness.as_ref().is_some_and(|w| {
                (w.diversion_x - w.billy_x).abs() < w.hearing_dist
                    && w.diversion_intensity > w.hearing_threshold
            }),
            match &witness {
                Some(w) => format!(
                    "billy.x={:.1}, diversion.x={:.1}, gap={:.1} < hearing={:.1}; \
                     intensity={:.2} > threshold={:.2}",
                    w.billy_x,
                    w.diversion_x,
                    (w.diversion_x - w.billy_x).abs(),
                    w.hearing_dist,
                    w.diversion_intensity,
                    w.hearing_threshold
                ),
                None => "the scene synth was never sampled".to_owned(),
            },
        );
        harness.check(
            "no-denial-for-legitimate-order",
            !events.iter().any(|e| matches!(e, Event::MoleDenied { .. })),
            "the pump-house floor hosts the mole; the legitimate order was not refused",
        );

        // The observation trail is source-specific and same-tick. A noise tag
        // alone could belong to the acceptance probe or another consequence.
        let unambiguous = witness
            .as_ref()
            .is_some_and(|w| w.heard_diversion().is_some() && w.room_noise_sources == 1);
        harness.check(
            "diversion-announcement-source-unambiguous",
            unambiguous,
            match &witness {
                Some(w) => format!(
                    "{} audible UnusualNoise observations in the diversion's room on the \
                     scene tick; GuardObserved carries kind + room, not a source id",
                    w.room_noise_sources
                ),
                None => "the scene synth was never sampled".to_owned(),
            },
        );
        let observed =
            unambiguous && witness.as_ref().is_some_and(|w| w.noise_announcements == 1);
        harness.check(
            "guard-observed-the-diversion",
            observed,
            match &witness {
                Some(w) => format!(
                    "{} matching GuardObserved(UnusualNoise) events on command tick {SYNTH_AT}",
                    w.noise_announcements
                ),
                None => "the scene synth was never sampled".to_owned(),
            },
        );
        harness.check(
            "perception-contract-lists-the-noise",
            witness
                .as_ref()
                .is_some_and(|w| w.heard_diversion().is_some()),
            match &witness {
                Some(w) => format!(
                    "observations at billy.x={:.1}: {:?}; diversion source x={:.1}, \
                     intensity={:.2} — matched through actor_observations()",
                    w.billy_x,
                    w.observations.iter().map(|o| o.kind).collect::<Vec<_>>(),
                    w.diversion_x,
                    w.diversion_intensity
                ),
                None => "the scene synth was never sampled".to_owned(),
            },
        );
        let noise_event_count = events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    Event::GuardObserved {
                        kind: ObservationKind::UnusualNoise,
                        ..
                    }
                )
            })
            .count();
        harness.check(
            "only-the-scene-diversion-announced",
            observed && noise_event_count == 1,
            format!(
                "{noise_event_count} UnusualNoise announcements through the scene synth; \
                 the acceptance probe must not supply the witness"
            ),
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
             canonical log (rendered as Billy's own words) and actor_observations() evaluated \
             at Billy's position with the archetype's hearing. The observed noise matches the \
             scene diversion's canonical source and is announced on that same command tick; \
             the earlier acceptance probe is not evidence that this order was heard."
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

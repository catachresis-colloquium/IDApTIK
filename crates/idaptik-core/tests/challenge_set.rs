//! The pump-house challenge set: four solutions, four evidence trails.
//!
//! `docs/scenarios/challenge-set.md` describes the floor and the four
//! intended lines; this file *runs* them, tick by tick, through the public
//! canonical API only — the same surface a frontend or the replay pipeline
//! consumes — and asserts the player-visible evidence for each:
//!
//! 1. **Physical access** — rope, lockpick, the valve wheel turned by hand,
//!    and a sprint through the window the flood's own short-circuit opens.
//! 2. **Hacker/control** — the valve released remotely through the bridge
//!    pivot (the wheel never turns), and the office lights killed over the
//!    network to cover the crossing.
//! 3. **Cooperative, Moletaire-assisted** — the hosted companion stages a
//!    chain of synth decoys that walks the guard all the way to the sluice
//!    exit while the infiltrator takes the note and the chute. No water, no
//!    hacks: the building stays dry and the guard is never fooled about the
//!    *player*, only about the noise.
//! 4. **Emergent cross-system** — a grid cut aimed at the upstream power
//!    line cascades through the feed into the sump pump; the zone's
//!    fail-open valve floods the pump hall *by consequence*, the tamper and
//!    flood alarms pull the guard east, the dead lights blind every room,
//!    and the infiltrator walks out through the dark. No objective on this
//!    floor mentions water, pumps or power: the flood is a consequence the
//!    systems produce together, never a scripted trigger.
//!
//! Every run also asserts the shared invariants: the guard's pulls are
//! recorded as `GuardObserved` observations, `billy.last_known_x` tracks the
//! *noise* and never the hidden player, and no run ends in `MissionFailed`.
//! All waits are adaptive (event- or state-gated with generous budgets), so
//! the runs ride the seed-rolled arrival window instead of assuming it.

use idaptik_core::companion::MoleEvent;
use idaptik_core::scenario::command::{
    Button, Buttons, Command, Edge, MoleOp, NetNodeIndex, PivotTarget, TickInput,
};
use idaptik_core::scenario::definition::ObjectiveKind;
use idaptik_core::scenario::event::Event;
use idaptik_core::scenario::floor_graph::{floor_graph, light_node_id, valve_node_id};
use idaptik_core::scenario::infra::{ObservationKind, ReleaseCause};
use idaptik_core::scenario::state::Place;
use idaptik_core::scenario::tuning::ActionKind;
use idaptik_core::scenario::{GhostLobbySim, HALL_SUMP, RunConfig, pump_house};

//## Harness

fn hold(buttons: &[Button]) -> TickInput {
    let mut b = Buttons::default();
    for x in buttons {
        b.set(*x, true);
    }
    TickInput {
        buttons: b,
        ..TickInput::default()
    }
}

fn press_interact() -> TickInput {
    let mut b = Buttons::default();
    b.set(Button::Interact, true);
    TickInput {
        buttons: b,
        edges: vec![Edge::InteractPress],
        ..TickInput::default()
    }
}

fn cmds(commands: &[Command]) -> TickInput {
    TickInput {
        immediates: commands.to_vec(),
        ..TickInput::default()
    }
}

const CROUCH: &[Button] = &[Button::Crouch];
const CROUCH_INTERACT: &[Button] = &[Button::Crouch, Button::Interact];
const SPRINT_EAST: &[Button] = &[Button::Right, Button::Sprint];
const WALK_EAST: &[Button] = &[Button::Right];
const CROUCH_EAST: &[Button] = &[Button::Right, Button::Crouch];

/// One recorded attempt: the sim, everything it has emitted, and the adaptive
/// movement helpers every solution shares.
struct Run {
    sim: GhostLobbySim,
    log: Vec<Event>,
}

impl Run {
    fn new(seed: u32) -> Self {
        let mut sim = GhostLobbySim::new(pump_house(), RunConfig::standard(), seed)
            .expect("pump-house valid");
        let log = sim.drain_events();
        Self { sim, log }
    }

    fn tick(&mut self, input: &TickInput) {
        self.log.extend(self.sim.tick(input));
    }

    fn cmd(&mut self, commands: &[Command]) {
        self.tick(&cmds(commands));
    }

    fn saw(&self, e: &Event) -> bool {
        self.log.contains(e)
    }

    fn any(&self, f: impl Fn(&Event) -> bool) -> bool {
        self.log.iter().any(f)
    }

    /// Body centre, the x every interaction measures from.
    fn cx(&self) -> f64 {
        self.sim.state().player.x + self.sim.definition().player.w / 2.0
    }

    fn billy_x(&self) -> f64 {
        self.sim.state().billy.x
    }

    fn node(&self, id: &str) -> NetNodeIndex {
        let def = self.sim.definition().clone();
        let index = floor_graph(&def)
            .nodes
            .iter()
            .position(|n| n.id == id)
            .unwrap_or_else(|| panic!("node {id} is derived"));
        NetNodeIndex(index as u32)
    }

    /// Walk (either direction) until the centre is within 4 px of `target`.
    fn walk_to(&mut self, target: f64, budget: usize) {
        for _ in 0..budget {
            let cx = self.cx();
            if (cx - target).abs() <= 4.0 {
                self.tick(&TickInput::default());
                return;
            }
            let dir = if cx < target {
                Button::Right
            } else {
                Button::Left
            };
            self.tick(&hold(&[dir]));
        }
        panic!("never reached cx {target} (stuck at {})", self.cx());
    }

    /// Hold `input` until `pred` holds, or panic with `what` — the adaptive
    /// wait every choreography below is built from. Generous budgets: a wait
    /// that fails is a broken solution, not a tight frame.
    fn wait_for(
        &mut self,
        budget: usize,
        input: &TickInput,
        pred: impl Fn(&Self) -> bool,
        what: &str,
    ) {
        for _ in 0..budget {
            self.tick(input);
            if pred(self) {
                return;
            }
        }
        panic!("timed out waiting for {what}");
    }

    //## Shared opening: roof, rope, lockpick, note.

    /// The rooftop approach every solution starts with: walk the roof to the
    /// rope, ride it down, and pick the locked front door from the street.
    fn enter_via_rope_and_door(&mut self) {
        let rope_x = self
            .sim
            .definition()
            .exterior
            .as_ref()
            .expect("pump-house keeps the roof")
            .rope
            .top_x;
        self.walk_to(rope_x, 600);
        self.tick(&press_interact());
        assert!(
            self.saw(&Event::RopeTaken { down: true }),
            "the rope never took: {:?}",
            &self.log[self.log.len().saturating_sub(4)..]
        );
        self.wait_for(
            400,
            &TickInput::default(),
            |r| r.sim.state().player.place == Place::Floor && r.sim.infiltrator_outside(),
            "the rope ride down",
        );
        // The locked front door stops the body on the street; hold interact
        // at it until the lock gives (the rooftop suite's proven sequence).
        for _ in 0..240 {
            self.tick(&hold(WALK_EAST));
        }
        self.wait_for(
            330,
            &hold(&[Button::Interact]),
            |r| r.saw(&Event::FrontDoorPicked),
            "the lock to give",
        );
        self.wait_for(
            120,
            &hold(WALK_EAST),
            |r| !r.sim.infiltrator_outside(),
            "the door to open inward",
        );
    }

    /// Secure the contact note in the wet canteen — the objective every
    /// solution takes while the guard is still offsite.
    fn grab_note(&mut self) {
        self.walk_to(130.0, 400);
        self.wait_for(
            150,
            &hold(&[Button::Interact]),
            |r| r.any(|e| matches!(e, Event::NoteSecured { .. })),
            "the note",
        );
    }

    fn crisis_began(&self) -> bool {
        self.any(|e| matches!(e, Event::CrisisBegan { .. }))
    }

    //## Shared ending: the sump chute.

    /// Search the chute (unless the floor already revealed it) and ride it
    /// out. `input` is how the body should stand while searching — crouched
    /// when the guard is close enough to hear a footfall.
    fn extract_via_chute(&mut self, search_input: &TickInput) {
        if !self.sim.state().chute.revealed {
            self.wait_for(
                300,
                search_input,
                |r| r.sim.state().chute.revealed,
                "the chute search",
            );
        }
        self.wait_for(
            90,
            &press_interact(),
            |r| {
                r.saw(&Event::Extracted {
                    method: idaptik_core::scenario::common::ExtractMethod::LaundryChute,
                })
            },
            "the chute ride",
        );
    }

    fn assert_clean_finish(&self) {
        assert!(
            self.saw(&Event::Extracted {
                method: idaptik_core::scenario::common::ExtractMethod::LaundryChute,
            }),
            "the run never extracted"
        );
        assert!(
            !self.any(|e| matches!(e, Event::MissionFailed { .. })),
            "the solution ended in failure: {:?}",
            self.log
                .iter()
                .filter(|e| matches!(e, Event::MissionFailed { .. }))
                .collect::<Vec<_>>()
        );
        assert!(
            !self.any(|e| matches!(e, Event::BossCalled { .. })),
            "the guard ever locked eyes on the infiltrator — the line was seen"
        );
    }

    /// The pull every alarm-driven solution shares: the guard's committed
    /// position is the flood bell's room centre, east of the crossing, and
    /// nowhere near the infiltrator's route.
    fn assert_pulled_to_the_flood_bell(&self) {
        let billy = &self.sim.state().billy;
        assert!(
            (billy.last_known_x - 652.5).abs() < 1.0,
            "the guard was not pulled to the control-office bell: {}",
            billy.last_known_x
        );
        assert!(
            self.any(|e| matches!(
                e,
                Event::GuardObserved {
                    kind: ObservationKind::Alarm,
                    ..
                }
            )),
            "the guard never announced hearing the alarm"
        );
    }
}

//## Solution 1 — physical access

/// Rope down, pick the front door, take the note, turn the sump wheel *by
/// hand*, and wait at the pump-hall's east end. When the guard's shift
/// starts, the flood bell is already ringing: he commits to the control
/// office and is still wading the pump hall when the body rides the chute
/// out. No network, no companion — hands and timing only.
#[test]
fn solution_one_physical_access_turns_the_wheel_and_outruns_the_shift() {
    let mut run = Run::new(101);
    run.enter_via_rope_and_door();
    run.grab_note();

    // The wheel: hold interact at the handle until the zone releases,
    // watching the turn accumulate — the release resets the hold, so the
    // physical-turn evidence is captured mid-hold.
    run.walk_to(470.0, 400);
    let mut max_turn = 0.0f64;
    let mut released = false;
    for _ in 0..240 {
        run.tick(&hold(&[Button::Interact]));
        max_turn = max_turn.max(run.sim.state().player.valve_turn);
        if run.saw(&Event::WaterReleased {
            zone: HALL_SUMP.into(),
            room: "hall".into(),
            cause: ReleaseCause::Valve,
        }) {
            released = true;
            break;
        }
    }
    assert!(released, "the wheel never opened the sump");
    assert!(
        max_turn >= 1.4,
        "the release was not a sustained physical hold: max valve_turn = {max_turn}"
    );

    // Wait at the pump hall's east end — support is strong in the hall, and
    // the dash from here beats the guard to the office from any seed-rolled
    // arrival.
    run.walk_to(505.0, 200);
    run.wait_for(
        1800,
        &TickInput::default(),
        Run::crisis_began,
        "the shift to start",
    );

    // The dash: sprint east the moment the crisis begins. The guard enters
    // at the far west end and the flood bell pulls him to the control
    // office; the crossing is done before he clears the pump hall.
    run.wait_for(
        160,
        &hold(SPRINT_EAST),
        |r| r.cx() >= 920.0,
        "the chute mouth",
    );
    assert!(
        run.billy_x() < 520.0,
        "the guard was already east of the pump hall at the crossing: {}",
        run.billy_x()
    );
    run.extract_via_chute(&hold(&[Button::Interact]));
    run.assert_clean_finish();

    //## Player-visible evidence
    assert!(run.saw(&Event::RopeTaken { down: true }));
    assert!(run.saw(&Event::FrontDoorPicked));
    assert!(run.any(|e| matches!(e, Event::NoteSecured { .. })));
    assert!(run.saw(&Event::WaterReleased {
        zone: HALL_SUMP.into(),
        room: "hall".into(),
        cause: ReleaseCause::Valve,
    }));
    // The flood's own consequences, all canonical events: the bell trips on
    // the rising water and the hall's electrics short out.
    assert!(run.saw(&Event::AlarmChanged {
        alarm: "flood-alarm".into(),
        room: "office".into(),
        sounding: true,
    }));
    assert!(run.any(|e| matches!(
        e,
        Event::InfraShorted { zone, .. } if zone == HALL_SUMP
    )));
    run.assert_pulled_to_the_flood_bell();
    // The wheel was physical, and the run never touched the network.
    assert!(
        !run.any(|e| matches!(e, Event::NetHackAction { .. } | Event::UplinkAction { .. })),
        "the physical line used the network"
    );
}

//## Solution 2 — hacker / control

/// The same floor, played from the van: pivot onto the maintenance bridge
/// and open the sump valve *remotely* — the wheel never turns — then, with
/// the guard committed to the flood bell, kill the control-office lights
/// over the network and cross under the blackout's flicker and stun.
#[test]
fn solution_two_hacker_control_opens_the_valve_and_kills_the_lights() {
    let mut run = Run::new(202);
    run.enter_via_rope_and_door();
    run.grab_note();

    // Remote release: pivot to the bridge and hack the derived valve node.
    let valve = run.node(&valve_node_id(HALL_SUMP));
    run.cmd(&[
        Command::Pivot {
            target: PivotTarget::Bridge,
        },
        Command::NetHack { node: valve },
    ]);
    assert!(
        run.saw(&Event::WaterReleased {
            zone: HALL_SUMP.into(),
            room: "hall".into(),
            cause: ReleaseCause::Valve,
        }),
        "the hacked valve never released: {:?}",
        run.log
    );
    assert!(
        run.any(|e| matches!(
            e,
            Event::NetHackAction { node_id } if node_id == &valve_node_id(HALL_SUMP)
        )),
        "the release is not attributable to the hack"
    );
    assert_eq!(
        run.sim.state().player.valve_turn,
        0.0,
        "the wheel turned by hand — this is the remote line"
    );

    // Hide in the tool recess while the flood does its work: the bell trips,
    // the hall shorts dark, and the guard's shift starts into a building
    // already in alarm.
    run.walk_to(404.0, 300);
    run.wait_for(
        120,
        &hold(CROUCH),
        |r| r.sim.state().player.hidden,
        "the recess",
    );
    run.wait_for(1800, &hold(CROUCH), Run::crisis_began, "the shift to start");
    run.wait_for(
        1200,
        &hold(CROUCH),
        |r| r.any(|e| matches!(e, Event::InfraShorted { .. })) && r.billy_x() > 560.0,
        "the guard to commit east of the shorted hall",
    );

    // Slip east along the dark pump hall — crouched: the guard is close
    // enough to hear a footfall, but the splash floor is louder than a
    // crouch and his attention is on the bell.
    run.wait_for(
        240,
        &hold(CROUCH_EAST),
        |r| r.cx() >= 500.0,
        "the hall's east end",
    );

    // Kill the office lights over the network, unwind the pivot so support
    // recovers before the exposed crossing, and sprint in the same breath.
    let lights = run.node(&light_node_id("office"));
    run.cmd(&[Command::NetHack { node: lights }, Command::Unpivot]);
    assert!(
        run.any(|e| matches!(
            e,
            Event::UplinkAction {
                kind: ActionKind::Lights
            }
        )),
        "the lights hack never landed"
    );
    assert!(
        run.sim.state().lights_flicker > 0.0,
        "the blackout window never opened"
    );
    assert!(
        run.any(|e| matches!(e, Event::PivotClosed { hops: 0 })),
        "the pivot was never unwound"
    );

    // The crossing: the flicker blinds and the stun holds the guard still
    // while the body passes; by the time the office lights come back the
    // infiltrator is in the gallery, another room away.
    run.wait_for(
        160,
        &hold(SPRINT_EAST),
        |r| r.cx() >= 920.0,
        "the chute mouth",
    );
    run.extract_via_chute(&hold(&[Button::Interact]));
    run.assert_clean_finish();

    //## Player-visible evidence
    assert!(run.saw(&Event::AlarmChanged {
        alarm: "flood-alarm".into(),
        room: "office".into(),
        sounding: true,
    }));
    assert!(run.any(|e| matches!(
        e,
        Event::InfraShorted { zone, .. } if zone == HALL_SUMP
    )));
    run.assert_pulled_to_the_flood_bell();
    assert!(
        run.sim.state().lights_uses >= 1,
        "the deliberate lights kill was not counted"
    );
}

//## Solution 3 — cooperative, Moletaire-assisted

/// The companion runs the guard ragged: a chain of synth decoys — each just
/// inside his hearing of the last — stages him from the canteen all the way
/// to the sluice exit, while the infiltrator (hidden behind the counter,
/// then quiet on foot) takes the note's room back and rides the chute. The
/// building stays dry: no valve, no hacks, no water.
#[test]
fn solution_three_cooperative_decoys_walk_the_guard_to_the_exit() {
    let mut run = Run::new(303);
    run.enter_via_rope_and_door();
    run.grab_note();

    // Hide behind the canteen counter; the crisis begins on its own clock.
    run.walk_to(205.0, 300);
    run.wait_for(
        120,
        &hold(CROUCH),
        |r| r.sim.state().player.hidden,
        "the counter shadow",
    );
    run.wait_for(1800, &hold(CROUCH), Run::crisis_began, "the shift to start");

    // The decoy chain. Every leg: order the mole underground to the next
    // staging point, keep refreshing the synth so the diversion never lapses,
    // and wait until the guard has committed past the previous one. Leg
    // spacing (~150 px) is always inside his 260 px hearing, so each decoy
    // is audible from where the last one left him.
    const LEGS: [(f64, usize); 6] = [
        (390.0, 330),
        (560.0, 500),
        (700.0, 640),
        (850.0, 790),
        (1000.0, 940),
        (1150.0, 1040),
    ];
    for (leg, (target, gate)) in LEGS.iter().enumerate() {
        run.cmd(&[Command::Mole {
            op: MoleOp::MoveTo {
                x: *target,
                underground: true,
            },
        }]);
        let sound = (leg % 3) as u8;
        // Advance until the guard is past the gate, synthesising every half
        // second. The synth rides along with the body's buttons — never
        // instead of them — so a decoy refresh never drops the crouch while
        // the guard is crossing the canteen.
        let mut i = 0usize;
        loop {
            let buttons = if run.sim.state().player.hidden || run.billy_x() < 300.0 {
                let mut b = Buttons::default();
                b.set(Button::Crouch, true);
                b
            } else {
                Buttons::default()
            };
            let immediates = if i % 30 == 0 {
                vec![Command::Mole {
                    op: MoleOp::Synth { index: sound },
                }]
            } else {
                Vec::new()
            };
            run.tick(&TickInput {
                buttons,
                immediates,
                ..TickInput::default()
            });
            i += 1;
            if run.billy_x() > *gate as f64 {
                break;
            }
            assert!(i < 1800, "the guard never followed the decoy to {target}");
        }
    }
    assert!(
        run.billy_x() > 1040.0,
        "the guard never reached the sluice exit: {}",
        run.billy_x()
    );

    // The quiet run: sprint while he is too far east to hear it, then
    // crouch-walk the last stretch — a crouch is below his footfall
    // threshold even at point-blank range.
    let mut i = 0usize;
    loop {
        let base = if run.cx() < 790.0 {
            SPRINT_EAST
        } else {
            CROUCH_EAST
        };
        let mut input = hold(base);
        if i % 30 == 0 {
            input.immediates.push(Command::Mole {
                op: MoleOp::Synth { index: 0 },
            });
        }
        run.tick(&input);
        i += 1;
        if run.cx() >= 920.0 {
            break;
        }
        assert!(i < 600, "the run east never reached the chute");
    }

    // Search and ride, still pinning the guard east with the synth.
    let mut extracted = false;
    for i in 0..420 {
        let mut input = if !run.sim.state().chute.revealed {
            hold(CROUCH_INTERACT)
        } else {
            press_interact()
        };
        if i % 30 == 0 {
            input.immediates.push(Command::Mole {
                op: MoleOp::Synth { index: 1 },
            });
        }
        run.tick(&input);
        if run.saw(&Event::Extracted {
            method: idaptik_core::scenario::common::ExtractMethod::LaundryChute,
        }) {
            extracted = true;
            break;
        }
    }
    assert!(extracted, "the cooperative line never extracted");
    run.assert_clean_finish();

    //## Player-visible evidence
    // The building stayed dry — this line never touched the water.
    assert!(
        !run.any(|e| matches!(e, Event::WaterReleased { .. })),
        "the cooperative line flooded the hall"
    );
    assert_eq!(
        run.sim
            .state()
            .infra
            .as_ref()
            .and_then(|i| i.zones.get(HALL_SUMP))
            .map(|z| z.level),
        Some(0.0),
        "the sump zone holds no water in the companion line"
    );
    // The companion's decoys are typed events, and the decoy is canonical
    // state a resync would carry.
    assert!(
        run.any(|e| matches!(
            e,
            Event::Mole {
                mole: MoleEvent::SynthSoundPlayed { .. }
            }
        )),
        "no synth decoy was ever played"
    );
    assert!(
        run.sim.state().diversion.is_some(),
        "the running decoy is not canonical state at the finish"
    );
    // The guard perceived *noises*, stage by stage — and stood pinned in the
    // sluice exit while the chute was ridden two rooms west.
    assert!(
        run.any(|e| matches!(
            e,
            Event::GuardObserved {
                kind: ObservationKind::UnusualNoise,
                ..
            }
        )),
        "the guard never announced hearing the decoys"
    );
    assert!(
        run.billy_x() > 950.0,
        "the guard was not held east at the finish: {}",
        run.billy_x()
    );
    assert!(
        run.sim.state().billy.last_known_x > 950.0,
        "the guard's commitment tracked the player instead of the decoy: {}",
        run.sim.state().billy.last_known_x
    );
    assert!(run.any(|e| matches!(e, Event::NoteSecured { .. })));
    // No network was used at all.
    assert!(
        !run.any(|e| matches!(e, Event::NetHackAction { .. } | Event::UplinkAction { .. })),
        "the cooperative line used the network"
    );
}

//## Solution 4 — emergent cross-system

/// The grid cut nobody scripted as a flood. Two hops up the ISP line reach
/// the utility substation; cutting it cascades through the feed into every
/// fixture — including the sump pump, whose zone is fail-open. The pump
/// hall floods *by consequence* (`WaterReleased` with `PumpFailure`, never
/// `Valve`), the tamper and flood bells ring, every room's lights die, and
/// the guard — arriving into a dark, ringing building — is pulled to the
/// control office while the infiltrator slips from the locker bay to the
/// chute. The floor's objectives are unchanged canonical kinds: no trigger
/// anywhere names water, pumps or power.
#[test]
fn solution_four_emergent_cascade_floods_by_consequence_not_by_script() {
    let def = pump_house();
    // The emergence claim, checked structurally: the scripted objectives are
    // the canonical note/misdirect/exit — nothing scripts the cascade.
    assert!(
        def.objectives.iter().all(|o| matches!(
            o.kind,
            ObjectiveKind::Note | ObjectiveKind::Misdirect | ObjectiveKind::Exit
        )),
        "the challenge floor grew a scripted infrastructure objective"
    );
    assert!(
        !def.objectives.iter().any(|o| {
            let id = o.id.as_str().to_lowercase();
            let label = o.label.to_lowercase();
            ["pump", "water", "flood", "valve", "power"]
                .iter()
                .any(|w| id.contains(w) || label.contains(w))
        }),
        "an objective references the infrastructure consequences"
    );

    let mut run = Run::new(404);
    run.enter_via_rope_and_door();
    run.grab_note();

    // Stage in the locker bay, then cut the grid: two ssh hops up the ISP
    // line and one hack on the substation, unwinding the pivots immediately
    // so the run's support recovers while the cascade lands.
    run.walk_to(885.0, 600);
    run.wait_for(
        120,
        &hold(CROUCH),
        |r| r.sim.state().player.hidden,
        "the locker bay",
    );
    let ops = run.node("isp-ops");
    let jump = run.node("grid-jump");
    let substation = run.node("substation");
    run.cmd(&[
        Command::NetSsh { node: ops },
        Command::NetSsh { node: jump },
        Command::NetHack { node: substation },
        Command::Unpivot,
        Command::Unpivot,
    ]);
    assert!(
        run.any(|e| matches!(e, Event::NetHackAction { node_id } if node_id == "substation")),
        "the grid cut never landed"
    );
    // The cascade is canonical state, immediately: the pump and the lights
    // are dead nodes, and the fail-open zone released on its own.
    assert!(
        run.saw(&Event::WaterReleased {
            zone: HALL_SUMP.into(),
            room: "hall".into(),
            cause: ReleaseCause::PumpFailure,
        }),
        "the fail-open pump never released the zone: {:?}",
        run.log
    );
    assert!(
        !run.any(|e| matches!(
            e,
            Event::WaterReleased {
                cause: ReleaseCause::Valve,
                ..
            }
        )),
        "the valve released — this flood must be the pump's failure alone"
    );
    assert_eq!(run.sim.state().player.valve_turn, 0.0);
    assert!(
        run.sim
            .state()
            .dead_nodes
            .iter()
            .any(|n| n.starts_with("pump-")),
        "the pump never lost power"
    );
    assert!(
        run.sim
            .state()
            .dead_nodes
            .contains(&light_node_id("office")),
        "the cascade never reached the lights"
    );
    assert!(
        run.any(|e| matches!(e, Event::PowerLost { nodes } if *nodes >= 2)),
        "the cascade was not reported"
    );
    // Both bells: the tamper circuit de-energises with the pump, and the
    // rising water trips the flood bell six seconds later.
    assert!(run.saw(&Event::AlarmChanged {
        alarm: "sump-tamper".into(),
        room: "hall".into(),
        sounding: true,
    }));
    run.wait_for(
        600,
        &hold(CROUCH),
        |r| {
            r.saw(&Event::AlarmChanged {
                alarm: "flood-alarm".into(),
                room: "office".into(),
                sounding: true,
            })
        },
        "the flood bell",
    );

    // The guard arrives into a dark, ringing building and is pulled east.
    run.wait_for(1800, &hold(CROUCH), Run::crisis_began, "the shift to start");
    run.wait_for(
        1200,
        &hold(CROUCH),
        |r| r.billy_x() > 560.0,
        "the guard to commit to the control office",
    );

    // The quiet slip: crouch-walk to the chute (below the guard's footfall
    // threshold even inside his hearing ring), search it, and ride it out
    // while he stands in the dark office two rooms west.
    run.wait_for(240, &hold(CROUCH_EAST), |r| r.cx() >= 920.0, "the chute mouth");
    run.extract_via_chute(&hold(CROUCH_INTERACT));
    run.assert_clean_finish();

    //## Player-visible evidence
    run.assert_pulled_to_the_flood_bell();
    // He perceived the dark and the bells — observations, never the hack.
    assert!(
        run.any(|e| matches!(
            e,
            Event::GuardObserved {
                kind: ObservationKind::Darkness,
                ..
            }
        )),
        "the guard never announced the dead lights"
    );
    // The short still fires on schedule even with the room already dead —
    // the water keeps climbing past the trip level.
    assert!(run.any(|e| matches!(
        e,
        Event::InfraShorted { zone, .. } if zone == HALL_SUMP
    )));
    assert!(run.any(|e| matches!(e, Event::NoteSecured { .. })));
}

//## The set as a whole

/// The four lines leave four different canonical fingerprints on the same
/// floor: the release cause, the wheel, and the network separate them, and
/// the floor itself is byte-identical between runs.
#[test]
fn the_four_solutions_share_one_floor_and_differ_in_their_footprints() {
    let a = pump_house();
    let b = pump_house();
    assert_eq!(a.scenario_id, b.scenario_id, "the builder is deterministic");
    assert_eq!(a.scenario_id.as_str(), "challenge-set-pump-house");
    // One zone, fail-open, with the wheel in the pump hall; two alarms; the
    // hosted companion fitted with the audio bay.
    let infra = a.infra.as_ref().expect("the floor carries infrastructure");
    assert_eq!(infra.water_zones.len(), 1);
    let zone = &infra.water_zones[0];
    assert_eq!(zone.id.as_str(), HALL_SUMP);
    assert!(zone.fail_open, "solution four needs the fail-open pump");
    assert_eq!(zone.room.as_str(), "hall");
    assert_eq!(zone.valve_x, 470.0, "the wheel stands by the office door");
    assert_eq!(infra.alarms.len(), 2);
    assert!(a.mole.is_some(), "solution three needs the hosted companion");
}

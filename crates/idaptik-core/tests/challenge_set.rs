//! The pump-house challenge set: four recorded solutions to the same floor,
//! each run tick by tick against the canonical simulation, each asserting the
//! player-visible evidence that explains *why* it works.
//!
//! The floor (`scenario::pump_house`) is the canonical Ghost Lobby rooftop
//! scenario relabelled as a municipal pump house and carrying infrastructure:
//! a floodable sump zone in the pump hall with a manual wheel and a derived
//! pump node on the power graph, a flood bell in the control office, a
//! normally-energised tamper circuit in the hall, and a hosted Moletaire
//! companion. See `docs/scenarios/challenge-set.md` and ADR-0013.
//!
//! The four lines:
//!
//! 1. **Physical access** — hands and timing only: the rope, the picked
//!    front door, the note, and the roof line over the hatch. The building
//!    stays dry; the line beats (or sidesteps) the guard's arrival clock.
//! 2. **Hacker / control** — the sump valve released over the network (the
//!    wheel never turns), every interior door walked open by routed holds,
//!    the office lights killed so the guard arrives into a dark, flooding,
//!    ringing building committed east of the body's chute ride.
//! 3. **Cooperative, Moletaire-assisted** — no water, no network: the
//!    companion's synth decoys sweep the guard east and then back west while
//!    the body crosses the roof and drops into an empty gallery.
//! 4. **Emergent cross-system** — a grid cut aimed at the upstream power
//!    line cascades into the fail-open sump pump: the hall floods *by
//!    consequence* (`WaterReleased { cause: PumpFailure }`, never `Valve`),
//!    both bells ring, every light dies, and the guard is pulled to the
//!    flood bell while the body leaves the way it came in — over the roof.
//!
//! Interior doors on this floor are the canonical closed doors: they block
//! everyone, and the infiltrator cannot open them by hand. They open three
//! ways — a routed hold from the network (`Uplink{Door}` on the nearest,
//! `NetHack` on a specific door node), or the guard's own badge as he walks
//! his route. The roof line (rope back up, hatch over the sump gallery) is
//! the physical bypass every seat can take. The recorded solutions use
//! exactly these mechanics and nothing else.
//!
//! Every run also asserts the shared invariants: the guard's pulls are
//! recorded as `GuardObserved` observations, `billy.last_known_x` tracks the
//! *noise* and never the hidden player, and no run ends in `MissionFailed`.
//! All waits are adaptive (event- or state-gated with generous budgets), so
//! the runs ride the seed-rolled arrival window instead of assuming it.

use idaptik_core::companion::{CoprocessorType, MoleEvent};
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
const WALK_EAST: &[Button] = &[Button::Right];
const WALK_WEST: &[Button] = &[Button::Left];

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

    /// Commands that ride *with* the body's buttons in one tick — immediates
    /// merge into a full button-bearing input, they never replace it.
    fn cmd_holding(&mut self, buttons: &[Button], commands: &[Command]) {
        let mut input = hold(buttons);
        input.immediates.extend(commands.iter().cloned());
        self.tick(&input);
    }

    fn saw(&self, e: &Event) -> bool {
        self.log.contains(e)
    }

    fn any(&self, f: impl Fn(&Event) -> bool) -> bool {
        self.log.iter().any(f)
    }

    fn count(&self, f: impl Fn(&Event) -> bool) -> usize {
        self.log.iter().filter(|e| f(e)).count()
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

    /// Whether the named door currently stands open (routed hold or badge).
    fn door_open(&self, id: &str) -> bool {
        self.sim
            .state()
            .doors
            .iter()
            .any(|d| d.id.as_str() == id && d.open > 0.0)
    }

    /// The sump zone's canonical water level (0.0 on a dry floor).
    fn water_level(&self) -> f64 {
        self.sim
            .state()
            .infra
            .as_ref()
            .and_then(|i| i.zones.get(HALL_SUMP))
            .map(|z| z.level)
            .unwrap_or(0.0)
    }

    /// Walk (either direction) until the centre is within 4 px of `target`.
    /// A closed door in the way makes this block until something opens it —
    /// which is exactly how the routed-hold crossings below are written.
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

    //## The roof line: out the front door, up the rope, over the hatch.

    /// Leave the way the rope came in: west through the picked front door to
    /// the street, then back up the rope to the roof.
    fn exit_to_roof(&mut self) {
        self.wait_for(
            500,
            &hold(WALK_WEST),
            |r| r.sim.infiltrator_outside(),
            "the street",
        );
        self.wait_for(300, &hold(WALK_WEST), |r| r.cx() <= 6.0, "the rope foot");
        self.tick(&press_interact());
        assert!(
            self.saw(&Event::RopeTaken { down: false }),
            "the rope never took upward"
        );
        self.wait_for(
            600,
            &TickInput::default(),
            |r| r.sim.state().player.place == Place::Roof,
            "the rope climb",
        );
    }

    /// Walk the roof to the hatch over the sump gallery and drop through.
    /// The fall is loud (`NOISE_LAND`) — every caller lands only once the
    /// guard is beyond hearing of the gallery.
    fn drop_via_hatch(&mut self) {
        self.walk_to(990.0, 900);
        self.tick(&press_interact());
        assert!(
            self.any(|e| matches!(e, Event::HatchDropped { .. })),
            "the hatch never dropped"
        );
        self.wait_for(
            400,
            &TickInput::default(),
            |r| r.sim.state().player.place == Place::Floor,
            "the hatch fall",
        );
    }

    //## Shared ending: the sump chute.

    /// Search the chute (unless the floor already revealed it) and ride it
    /// out. `input` is how the body should stand while searching — the
    /// search itself is silent; only the walk to it makes noise.
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

    fn assert_no_water_and_no_network(&self) {
        assert!(
            !self.any(|e| matches!(e, Event::WaterReleased { .. })),
            "this line never touched the water, yet the zone released"
        );
        assert_eq!(
            self.water_level(),
            0.0,
            "the sump zone holds water in a dry line"
        );
        assert!(
            !self.any(|e| matches!(e, Event::NetHackAction { .. } | Event::UplinkAction { .. })),
            "this line used the network"
        );
    }
}

//## Solution 1 — physical access

/// Rope down, pick the front door, take the note — then back out to the
/// street, up the rope, and across the roof to the hatch over the sump
/// gallery. No network, no companion, no water: the physical line beats the
/// guard's arrival clock, and even when the shift starts mid-run the guard
/// is a floor below and a building away — the roof is invisible, and the
/// hatch drop lands beyond his hearing while he patrols the west end.
#[test]
fn solution_one_physical_access_takes_the_roof_line_and_outruns_the_shift() {
    let mut run = Run::new(101);
    run.enter_via_rope_and_door();
    run.grab_note();

    // Out the way we came in, and over the roof.
    run.exit_to_roof();
    run.drop_via_hatch();

    // The landing put the body in the chute's own search band; search, then
    // ride.
    run.extract_via_chute(&hold(CROUCH_INTERACT));
    run.assert_clean_finish();

    //## Player-visible evidence
    assert!(run.saw(&Event::RopeTaken { down: true }));
    assert!(run.saw(&Event::FrontDoorPicked));
    assert!(
        run.saw(&Event::NoteSecured { seen: false }),
        "the note was taken in plain sight"
    );
    assert!(run.saw(&Event::RopeTaken { down: false }));
    assert!(run.any(|e| matches!(e, Event::HatchDropped { .. })));
    run.assert_no_water_and_no_network();
    assert!(
        !run.any(|e| matches!(e, Event::Mole { .. } | Event::MoleDenied { .. })),
        "the physical line used the companion"
    );
    // The clock: whenever the shift started, the guard never got east of
    // the pump hall — his patrol band and the bell-less building keep him
    // west while the body rides the chute.
    assert!(
        run.billy_x() < 520.0,
        "the guard was east of the pump hall at the finish: {}",
        run.billy_x()
    );
}

//## Solution 2 — hacker / control

/// The remote line. From the roof, before the body even drops: pivot onto
/// the maintenance bridge and hack the derived valve node — the sump zone
/// releases with the wheel never touched. The body then walks the floor
/// east through doors the network opens one by one (a routed hold on the
/// nearest door, `Uplink{Door}`, is the hacker's canonical answer to a
/// closed door), crossing the office inside the flicker a lights kill buys,
/// and stages in the sump gallery — all before the shift starts. When the
/// guard arrives, the flood bell pulls him to the control office, committed
/// east of the gallery while the body searches and rides the chute.
#[test]
fn solution_two_hacker_control_opens_the_valve_and_walks_the_doors() {
    let mut run = Run::new(202);

    // The remote release, first thing: the network does not care where the
    // body is. Pivot onto the bridge and hack the derived valve node in the
    // same breath.
    let valve = run.node(&valve_node_id(HALL_SUMP));
    run.cmd(&[
        Command::Pivot {
            target: PivotTarget::Bridge,
        },
        Command::NetHack { node: valve },
    ]);
    assert!(
        run.any(|e| matches!(
            e,
            Event::NetHackAction { node_id } if node_id == &valve_node_id(HALL_SUMP)
        )),
        "the release is not attributable to the hack"
    );
    assert!(
        run.saw(&Event::WaterReleased {
            zone: HALL_SUMP.into(),
            room: "hall".into(),
            cause: ReleaseCause::Valve,
        }),
        "the hacked valve did not release the zone"
    );
    assert_eq!(
        run.sim.state().player.valve_turn,
        0.0,
        "the wheel turned by hand — this is the remote line"
    );

    run.enter_via_rope_and_door();
    run.grab_note();

    // The first door: a routed hold opens the door nearest the body. Walk
    // east until the hold is the D1 crossing, issue it, and keep walking —
    // the walk blocks at the closed door until the hold lands.
    run.walk_to(250.0, 400);
    run.cmd(&[Command::Uplink {
        kind: ActionKind::Door,
    }]);
    run.wait_for(90, &TickInput::default(), |r| r.door_open("D1"), "D1 to open");
    // The pump hall is already wading (the release was seconds ago); cross
    // it to the tool recess while the guard is still offsite — nobody is
    // listening to the splashes yet.
    run.walk_to(404.0, 600);

    // The second door, from the recess: nearest door is D2. Kill the office
    // lights in the same tick — the office goes dark before the body steps
    // into it, and the guard's shift will start into a dark east wing.
    let lights = run.node(&light_node_id("office"));
    run.cmd_holding(
        &[],
        &[
            Command::Uplink {
                kind: ActionKind::Door,
            },
            Command::NetHack { node: lights },
        ],
    );
    assert!(
        run.any(|e| matches!(
            e,
            Event::UplinkAction {
                kind: ActionKind::Lights
            }
        )),
        "the lights hack never landed"
    );
    run.wait_for(90, &TickInput::default(), |r| r.door_open("D2"), "D2 to open");

    // Cross the dark office. The third door is a *named* hold — `NetHack`
    // on the door node itself, because the nearest-door uplink from here
    // would answer D2 again. The Door economy's one-second cooldown has
    // long expired during the walk.
    run.walk_to(560.0, 400);
    let d3 = run.node("door-d3");
    run.cmd(&[Command::NetHack { node: d3 }]);
    run.wait_for(90, &TickInput::default(), |r| r.door_open("D3"), "D3 to open");
    run.walk_to(885.0, 600);
    run.wait_for(
        150,
        &hold(CROUCH),
        |r| r.sim.state().player.hidden,
        "the locker bay",
    );

    // The shift starts on its own clock; the flood bell has been ringing
    // since six seconds after the release. Wait until the guard has
    // committed east of the office door — pulled to the bell, wading his
    // own patrol no more.
    run.wait_for(2400, &hold(CROUCH), Run::crisis_began, "the shift to start");
    run.wait_for(
        1800,
        &hold(CROUCH),
        |r| r.billy_x() > 560.0,
        "the guard to commit to the control office",
    );

    // The quiet walk: the chute search band is beyond his 260px hearing
    // from the bell, and a crouch-walk is below his footfall threshold
    // anyway. Search, then ride.
    run.walk_to(920.0, 200);
    run.extract_via_chute(&hold(CROUCH_INTERACT));
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
    assert!(
        run.count(|e| matches!(
            e,
            Event::UplinkAction {
                kind: ActionKind::Door
            }
        )) >= 3,
        "the line did not walk three doors open on routed holds"
    );
    // The guard ended where the bell put him, nowhere near the chute.
    assert!(
        (run.billy_x() - 652.5).abs() < 60.0,
        "the guard was not held at the bell: {}",
        run.billy_x()
    );
}

//## Solution 3 — cooperative, Moletaire-assisted

/// The companion runs the guard ragged. From behind the canteen counter the
/// infiltrator flies the mole: synth decoys refresh every half second while
/// an underground run sweeps the guard east — each metre of the sweep stays
/// inside his 260px hearing — pins him at the sluice exit, then sweeps back
/// west and parks him at the pump hall's west door. Meanwhile the body
/// leaves the counter, climbs back to the roof, and waits over the hatch
/// until the guard is beyond hearing of the gallery. The drop, the silent
/// search and the ride all happen with the guard walking the *wrong* way,
/// chasing a sound. The building stays dry: no valve, no hacks, no water.
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

    // One state machine drives both seats: the mole's sweeps and the body's
    // roof crossing. Every synth refresh rides along with the body's
    // buttons — never instead of them — so a decoy refresh never drops the
    // crouch or the walk.
    let mut phase = 0u8; // 0 hidden, 1 exiting, 2 climbing, 3 roof run, 4 at hatch, 5 dropped
    let mut east_fired = false;
    let mut west_fired = false;
    let mut i = 0usize;
    loop {
        let mut immediates = Vec::new();

        // The decoy heartbeat: every 30 ticks while the body is still
        // crossing. The mole starts at its host spawn (390, pump hall) —
        // the first decoys play from there and the guard's own patrol
        // brings him inside their 260px hearing.
        if i % 30 == 0 && phase < 5 {
            immediates.push(Command::Mole {
                op: MoleOp::Synth {
                    index: ((i / 30) % 3) as u8,
                },
            });
        }
        // The east sweep, once the guard is moving: one long underground
        // run to the sluice exit, refreshed all the way — he follows the
        // sound across the whole floor, badging every door behind him.
        if !east_fired && run.billy_x() > 330.0 {
            east_fired = true;
            immediates.push(Command::Mole {
                op: MoleOp::MoveTo {
                    x: 1150.0,
                    underground: true,
                },
            });
        }
        // The west sweep, once he is pinned east: back to the pump hall's
        // west door, far from the hatch and the chute.
        if east_fired && !west_fired && run.billy_x() > 1000.0 {
            west_fired = true;
            immediates.push(Command::Mole {
                op: MoleOp::MoveTo {
                    x: 390.0,
                    underground: true,
                },
            });
        }

        let input = match phase {
            0 => {
                if run.billy_x() > 560.0 {
                    phase = 1;
                }
                let mut t = hold(CROUCH);
                t.immediates = immediates;
                t
            }
            1 => {
                // Out through the picked front door to the rope foot.
                if run.sim.infiltrator_outside() && run.cx() <= 6.0 {
                    let mut t = press_interact();
                    t.immediates = immediates;
                    run.tick(&t);
                    assert!(
                        run.saw(&Event::RopeTaken { down: false }),
                        "the rope never took upward"
                    );
                    phase = 2;
                    i += 1;
                    assert!(i < 7200, "the cooperative line never finished");
                    continue;
                }
                let mut t = hold(WALK_WEST);
                t.immediates = immediates;
                t
            }
            2 => {
                if run.sim.state().player.place == Place::Roof {
                    phase = 3;
                }
                let mut t = TickInput::default();
                t.immediates = immediates;
                t
            }
            3 => {
                if (run.cx() - 990.0).abs() <= 10.0 {
                    phase = 4;
                }
                // Walked, never sprinted: the guard below hears through the
                // roof — a walk (0.165) is under his 0.45 footfall
                // threshold, a sprint (0.68+) would pull him off the decoy.
                let mut t = hold(WALK_EAST);
                t.immediates = immediates;
                t
            }
            4 => {
                // Over the hatch: the fall is loud, so it lands only once
                // the west sweep has the guard beyond hearing of the
                // gallery (990 - 700 > 260px of cover).
                if west_fired && run.billy_x() <= 700.0 {
                    let mut t = press_interact();
                    t.immediates = immediates;
                    run.tick(&t);
                    assert!(
                        run.any(|e| matches!(e, Event::HatchDropped { .. })),
                        "the hatch never dropped"
                    );
                    phase = 5;
                    i += 1;
                    assert!(i < 7200, "the cooperative line never finished");
                    continue;
                }
                let mut t = TickInput::default();
                t.immediates = immediates;
                t
            }
            _ => {
                // Dropped: one settling tick, then the bottom of the loop
                // breaks out to the search and ride.
                let mut t = TickInput::default();
                t.immediates = immediates;
                t
            }
        };
        run.tick(&input);
        i += 1;
        if phase == 5 && run.sim.state().player.place == Place::Floor {
            break;
        }
        assert!(i < 7200, "the cooperative line never finished");
    }

    // Search and ride; the guard is chasing a decoy two rooms west and the
    // search itself is silent.
    run.extract_via_chute(&hold(CROUCH_INTERACT));
    run.assert_clean_finish();

    //## Player-visible evidence
    run.assert_no_water_and_no_network();
    // The companion's decoys are typed events — the seat-visible record of
    // every sound the guard chased.
    assert!(
        run.count(|e| matches!(
            e,
            Event::Mole {
                mole: MoleEvent::SynthSoundPlayed { .. }
            }
        )) >= 6,
        "the decoy chain never played"
    );
    assert!(
        !run.any(|e| matches!(e, Event::MoleDenied { .. })),
        "the companion refused an order it was fitted for: {:?}",
        run.log
            .iter()
            .filter(|e| matches!(e, Event::MoleDenied { .. }))
            .collect::<Vec<_>>()
    );
    // The guard perceived *noises* — announced per room as he chased the
    // sweep east and back west — and ended west of the gallery, walking
    // away from the chute.
    let noise_rooms: Vec<String> = run
        .log
        .iter()
        .filter_map(|e| match e {
            Event::GuardObserved {
                kind: ObservationKind::UnusualNoise,
                room,
            } => Some(room.to_string()),
            _ => None,
        })
        .collect();
    assert!(
        !noise_rooms.is_empty(),
        "the guard never announced hearing the decoys"
    );
    assert!(
        run.billy_x() < 780.0,
        "the guard was not held west of the gallery at the finish: {}",
        run.billy_x()
    );
    assert!(
        run.sim.state().billy.last_known_x < 800.0,
        "the guard's commitment tracked the player instead of the decoy: {}",
        run.sim.state().billy.last_known_x
    );
    assert!(run.any(|e| matches!(e, Event::NoteSecured { .. })));
}

//## Solution 4 — emergent cross-system

/// The grid cut nobody scripted as a flood. From behind the canteen counter:
/// two ssh hops up the ISP line and one hack on the utility substation. The
/// cut cascades substation → feed → every fixture: all lights die (every
/// room goes dark), the cameras die, and the **sump pump dies — releasing
/// its fail-open zone with `cause: PumpFailure`**. The tamper bell trips
/// the same tick (power loss *is* its trip), the flood bell follows six
/// seconds later, and the guard — arriving into a dark, ringing building —
/// is pulled to the control office. The body leaves the way the roof line
/// allows: out the front door, up the rope, over the hatch, and down into a
/// gallery the guard is nowhere near. No objective anywhere names water,
/// pumps, valves or power.
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

    // Stage behind the canteen counter, then cut the grid: two ssh hops up
    // the ISP line and one hack on the substation, unwinding the pivots
    // immediately so the run's trace economics recover while the cascade
    // lands. The infiltrator never leaves the canteen until the guard has.
    run.walk_to(205.0, 300);
    run.wait_for(
        120,
        &hold(CROUCH),
        |r| r.sim.state().player.hidden,
        "the counter shadow",
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

    // The guard arrives into a dark, ringing building and is pulled east —
    // past the hidden body, through the pump hall he never thinks to
    // search, to the control office and its bell.
    run.wait_for(1800, &hold(CROUCH), Run::crisis_began, "the shift to start");
    run.wait_for(
        1800,
        &hold(CROUCH),
        |r| r.billy_x() > 560.0,
        "the guard to commit to the control office",
    );

    // And the body leaves over the roof: the guard is 300+ px from the
    // hatch and the chute, the whole building is dark behind him, and the
    // ringing bells outrank every footfall he could hear.
    run.exit_to_roof();
    run.drop_via_hatch();
    run.extract_via_chute(&hold(CROUCH_INTERACT));
    run.assert_clean_finish();

    //## Player-visible evidence
    run.assert_pulled_to_the_flood_bell();
    // The guard announced the dark and the alarm — perceptions, not the
    // hidden player's position.
    assert!(
        run.any(|e| matches!(
            e,
            Event::GuardObserved {
                kind: ObservationKind::Darkness,
                ..
            }
        )),
        "the guard never announced the dark building"
    );
    assert!(
        (run.billy_x() - 652.5).abs() < 60.0,
        "the guard was not held at the bell: {}",
        run.billy_x()
    );
    assert!(run.saw(&Event::RopeTaken { down: false }));
    assert!(run.any(|e| matches!(e, Event::HatchDropped { .. })));
    assert!(run.any(|e| matches!(e, Event::NoteSecured { .. })));
}

//## The floor itself

/// The challenge floor is the canonical rooftop scenario plus infrastructure:
/// the same five room ids (every interaction lines up), the roof and the
/// rope, one zone, two alarms, the derived valve and pump nodes on the power
/// graph, and a hosted companion.
#[test]
fn the_challenge_floor_is_the_canonical_rooftop_plus_infrastructure() {
    let def = pump_house();
    assert_eq!(def.scenario_id, "challenge-set-pump-house");
    let rooms: Vec<&str> = def.rooms.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(rooms, ["kitchen", "hall", "office", "laundry", "exit"]);
    assert!(def.exterior.is_some(), "the roof line is part of the floor");

    let infra = def.infra.as_ref().expect("the floor carries infrastructure");
    assert_eq!(infra.water_zones.len(), 1);
    let zone = &infra.water_zones[0];
    assert_eq!(zone.id, HALL_SUMP);
    assert_eq!(zone.room.as_str(), "hall");
    assert!((zone.valve_x - 470.0).abs() < f64::EPSILON);
    assert!(zone.fail_open, "the pump's zone must fail open for the cascade");
    assert_eq!(infra.alarms.len(), 2);

    // The derived power-graph fixtures the hacker line and the cascade use.
    let graph = floor_graph(&def);
    for id in [
        valve_node_id(HALL_SUMP),
        idaptik_core::scenario::floor_graph::pump_node_id(HALL_SUMP),
        light_node_id("office"),
    ] {
        assert!(
            graph.nodes.iter().any(|n| n.id == id),
            "the floor graph is missing {id}"
        );
    }

    // The companion is hosted, with the synth bay fitted.
    let host = def.mole.as_ref().expect("the floor hosts the companion");
    assert!((host.spawn_x - 390.0).abs() < f64::EPSILON);
    assert!(
        host.upgrades
            .iter()
            .any(|u| matches!(u, CoprocessorType::AudioSynthesiser)),
        "no synth bay"
    );

    // The whole floor validates — the challenge set ships clean content.
    assert!(
        def.validate().passed(),
        "the challenge floor does not validate: {:?}",
        def.validate().checks
    );
}

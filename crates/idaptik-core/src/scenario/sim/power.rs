//! Hosting the floor's electrical infrastructure (ADR-0013) inside the Ghost
//! Lobby run.
//!
//! The network itself lives in [`crate::scenario::power`]; this is the seam that
//! connects it to the world:
//!
//! * **Access** — the infiltrator touches what stands in their room (or what
//!   their own signal access already reaches); the hacker touches what has a
//!   networked panel on a node they can reach. A board authored local-only is
//!   reachable but not workable, and a cable cut is never remote work at all.
//! * **Readout** — the signal devices that are switched on or locked (for
//!   interlocks) and where the body is (for exposed conductors).
//! * **Effects** — a device with no operating load is *disabled on the floor*: a
//!   dead door does not slide, a dead vacuum does not move, a dead camera is
//!   blind, a dark room hides the infiltrator. The signal network is told which
//!   devices are inert through its own readout, so no wire can make one work.
//!
//! A floor with no power network takes none of these paths, so every existing
//! replay is unchanged.

use super::GhostLobbySim;
use crate::netsim::access::can_reach;
use crate::scenario::event::Event;
use crate::scenario::floor_graph::{VACUUM_NODE_ID, camera_node_id, door_node_id, light_node_id};
use crate::scenario::ids::{LoadId, RoomId};
use crate::scenario::mathf::TICK_DT;
use crate::scenario::power::LoadKind;
use crate::scenario::power::sim::{
    BodyAt, LoadReport, PowerAccess, PowerDenyReason, PowerEvent, PowerOp, PowerReadout,
};
use crate::scenario::signal::sim::Seat;

/// Whether the body standing in `here` is standing in `room`.
///
/// `GhostLobbySim::room_id_at` answers in plain strings (it is older than the id
/// newtypes), so the comparison is made on the borrowed text rather than on the
/// newtype.
fn room_matches(here: Option<&str>, room: Option<&RoomId>) -> bool {
    match (here, room) {
        (Some(a), Some(b)) => a == b.as_str(),
        _ => false,
    }
}

impl GhostLobbySim {
    /// Which electrical elements each seat can touch right now.
    ///
    /// One id space holds loads, feeds, breakers and sources (the definition is
    /// validated to keep it unique), so this is one set per seat rather than a
    /// map per kind.
    pub fn power_access(&self) -> PowerAccess {
        let mut access = PowerAccess::default();
        let Some(net) = &self.def.power else {
            return access;
        };
        let here = Self::room_id_at(&self.def, self.state.player.x);
        let signal = self.signal_access();

        for s in &net.sources {
            if room_matches(here, s.room.as_ref()) {
                access.infiltrator.insert(s.id.0.clone());
            }
            if self.hacker_reaches(s.node.as_ref()) {
                access.hacker.insert(s.id.0.clone());
            }
        }
        for f in &net.feeds {
            if room_matches(here, f.room.as_ref()) {
                access.infiltrator.insert(f.id.0.clone());
            }
            if self.hacker_reaches(f.node.as_ref()) {
                access.hacker.insert(f.id.0.clone());
            }
        }
        for b in &net.breakers {
            if room_matches(here, b.room.as_ref()) {
                access.infiltrator.insert(b.id.0.clone());
            }
            if self.hacker_reaches(b.node.as_ref()) {
                access.hacker.insert(b.id.0.clone());
            }
        }
        for l in &net.loads {
            let by_room = room_matches(here, l.room.as_ref());
            let by_signal = l
                .signal
                .as_ref()
                .is_some_and(|s| signal.infiltrator.contains(s));
            if by_room || by_signal {
                access.infiltrator.insert(l.id.0.clone());
            }
            let by_node = self.hacker_reaches(l.node.as_ref());
            let by_signal_reach = l.signal.as_ref().is_some_and(|s| signal.hacker.contains(s));
            if by_node || by_signal_reach {
                access.hacker.insert(l.id.0.clone());
            }
        }
        access
    }

    /// Whether the hacker's current vantage can reach a panel on `node`.
    fn hacker_reaches(&self, node: Option<&String>) -> bool {
        let hacker = &self.state.agents.hacker;
        let from = hacker.vantage_ip();
        node.and_then(|n| self.graph.node(n))
            .is_some_and(|n| hacker.stands_on(n.ip) || can_reach(&self.graph, from, n.ip))
    }

    /// The host's view of the world for one tick of the power network.
    pub fn power_readout(&self) -> PowerReadout {
        let mut readout = PowerReadout::default();
        if let (Some(net), Some(st)) = (&self.def.signal, &self.state.signal) {
            for (d, s) in net.devices.iter().zip(&st.devices) {
                if s.active {
                    readout.signal_active.insert(d.id.clone());
                }
                if s.locked {
                    readout.signal_locked.insert(d.id.clone());
                }
            }
        }
        if let Some(room) = Self::room_id_at(&self.def, self.state.player.x) {
            readout.body = Some(BodyAt {
                room: RoomId::from(room),
                x: self.state.player.x + self.def.player.w / 2.0,
            });
        }
        readout
    }

    /// `Command::Power` (immediate; self-guards on paused/ended).
    pub(crate) fn power_command(&mut self, op: PowerOp) {
        if self.paused || self.state.ended {
            return;
        }
        let Some(net) = &self.def.power else {
            return;
        };
        let by = op.seat();
        let cmd = match op.resolve(net) {
            Ok(cmd) => cmd,
            Err(reason) => {
                self.events.push(Event::Power {
                    power: PowerEvent::Denied { by, reason },
                });
                return;
            }
        };
        let cost = cmd.cost(net);
        if f64::from(cost.bandwidth) > self.state.bandwidth {
            self.events.push(Event::Power {
                power: PowerEvent::Denied {
                    by,
                    reason: PowerDenyReason::NoBandwidth,
                },
            });
            return;
        }
        if by == Seat::Infiltrator && self.state.power_busy > 0.0 {
            self.events.push(Event::Power {
                power: PowerEvent::Denied {
                    by,
                    reason: PowerDenyReason::Busy,
                },
            });
            return;
        }
        let access = self.power_access();
        let Some(st) = self.state.power.as_mut() else {
            return;
        };
        let events = st.apply(net, &cmd, &access);
        for e in events {
            if let Some(cost) = e.cost() {
                self.state.bandwidth = (self.state.bandwidth - f64::from(cost.bandwidth)).max(0.0);
                self.state.power_busy += f64::from(cost.busy_ticks) * TICK_DT;
                self.state.player.noise = self.state.player.noise.max(cost.noise);
                if by == Seat::Hacker {
                    self.state.stats.hacker_actions += 1;
                }
            }
            self.events.push(Event::Power { power: e });
        }
    }

    /// One tick of the electrical infrastructure, run before the signal network
    /// so a device that loses supply this tick is already inert when the wires
    /// are read.
    pub(super) fn system_power(&mut self) {
        self.state.power_busy = (self.state.power_busy - TICK_DT).max(0.0);
        if self.def.power.is_none() || self.state.power.is_none() {
            return;
        }
        let readout = self.power_readout();
        let events = {
            let (Some(net), Some(st)) = (&self.def.power, self.state.power.as_mut()) else {
                return;
            };
            st.step(net, &readout)
        };
        for e in events {
            // Live metal holds the body still for as long as the hazard says.
            if let PowerEvent::Shocked { ticks, .. } = &e {
                self.state.power_busy = f64::from(*ticks) * TICK_DT;
            }
            self.events.push(Event::Power { power: e });
        }

        // Disabled equipment is disabled *on the floor*, not merely quiet on the
        // network: a dead door does not slide and a dead vacuum does not move.
        let dead: Vec<(String, LoadKind)> = match (&self.def.power, &self.state.power) {
            (Some(net), Some(st)) => net
                .loads
                .iter()
                .zip(&st.loads)
                .filter(|(_, s)| !s.operating)
                .map(|(l, _)| (l.id.0.clone(), l.kind))
                .collect(),
            _ => Vec::new(),
        };
        for (id, kind) in dead {
            match kind {
                LoadKind::Door => {
                    for door in &mut self.state.doors {
                        if door_node_id(door.id.clone()) == id {
                            door.open = 0.0;
                            door.pending = 0.0;
                        }
                    }
                }
                LoadKind::Vacuum if id == VACUUM_NODE_ID => {
                    self.state.vacuum.active = false;
                }
                _ => {}
            }
        }
    }

    /// The full account of one device: what it is, what it is doing, and — the
    /// point of the whole subsystem — why it is off.
    pub fn power_report(&self, load: &str) -> Option<LoadReport> {
        let (net, st) = (self.def.power.as_ref()?, self.state.power.as_ref()?);
        st.report(net, &LoadId::from(load))
    }

    /// Whether a powered load is currently holding `node`'s device inert.
    fn power_device_dead(&self, node: &str) -> bool {
        let (Some(net), Some(st)) = (&self.def.power, &self.state.power) else {
            return false;
        };
        net.loads
            .iter()
            .zip(&st.loads)
            .any(|(l, s)| l.id.0 == node && !s.operating)
    }

    /// Whether the camera on `i` has no working supply.
    pub fn power_blinds_camera(&self, i: usize) -> bool {
        self.power_device_dead(&camera_node_id(i))
    }

    /// Whether the room's lights have no working supply.
    pub fn power_room_dark(&self, room: &str) -> bool {
        self.power_device_dead(&light_node_id(room))
    }
}

#[cfg(test)]
mod tests {
    use crate::scenario::GhostLobbySim;
    use crate::scenario::command::{Command, PivotTarget, RunConfig, TickInput, fold};
    use crate::scenario::event::Event;
    use crate::scenario::floor_graph::door_node_id;
    use crate::scenario::ids::{FeedId, LoadId, SignalId};
    use crate::scenario::power::sim::{
        OffReason, PowerDenyReason, PowerEvent, PowerIndex, PowerOp,
    };
    use crate::scenario::power::test_floor::{
        BRK_B, DOOR_EXIT, FEED_A, FEED_B, PUMP_LAUNDRY, powered_ghost_lobby,
    };
    use crate::scenario::signal::sim::Seat;

    fn sim() -> GhostLobbySim {
        GhostLobbySim::new(powered_ghost_lobby(), RunConfig::standard(), 123456).expect("valid")
    }

    fn step(sim: &mut GhostLobbySim, cmds: &[Command]) -> Vec<Event> {
        let mut held = Default::default();
        let input: TickInput = fold(cmds, &mut held);
        sim.tick(&input)
    }

    /// Stand the body in the middle of a room, so the infiltrator can reach the
    /// boards that live there.
    fn stand(sim: &mut GhostLobbySim, room: &str) {
        let Some(r) = sim
            .definition()
            .rooms
            .iter()
            .find(|r| r.id.as_str() == room)
        else {
            return;
        };
        sim.state.player.x = r.x + r.w / 2.0;
    }

    fn power_events(events: &[Event]) -> Vec<&PowerEvent> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::Power { power } => Some(power),
                _ => None,
            })
            .collect()
    }

    fn feed_index(sim: &GhostLobbySim, id: &str) -> PowerIndex {
        let i = sim
            .definition()
            .power
            .as_ref()
            .and_then(|n| n.feed_index(&FeedId::from(id)))
            .expect("a feed");
        PowerIndex(i as u16)
    }

    fn load_index(sim: &GhostLobbySim, id: &str) -> PowerIndex {
        let i = sim
            .definition()
            .power
            .as_ref()
            .and_then(|n| n.load_index(&LoadId::from(id)))
            .expect("a load");
        PowerIndex(i as u16)
    }

    fn breaker_index(sim: &GhostLobbySim, id: &str) -> PowerIndex {
        let i = sim
            .definition()
            .power
            .as_ref()
            .and_then(|n| n.breaker_index(&crate::scenario::ids::BreakerId::from(id)))
            .expect("a breaker");
        PowerIndex(i as u16)
    }

    #[test]
    fn an_unpowered_floor_has_no_power_state_at_all() {
        let sim = GhostLobbySim::new(
            crate::scenario::ghost_lobby::ghost_lobby(),
            RunConfig::standard(),
            1,
        )
        .expect("valid");
        assert!(sim.state().power.is_none());
        assert!(sim.power_access().infiltrator.is_empty());
        assert!(sim.power_report("cam-0").is_none());
    }

    #[test]
    fn a_command_cannot_make_an_unpowered_device_run() {
        let mut sim = sim();
        stand(&mut sim, "hall");
        let feed = feed_index(&sim, FEED_B);
        step(
            &mut sim,
            &[Command::Power {
                op: PowerOp::CutCable {
                    by: Seat::Infiltrator,
                    feed,
                },
            }],
        );
        // The cut took two seconds of hands: let them come free before asking
        // for anything else, or the request is refused for being *busy* rather
        // than for there being no supply — a different sentence, and a real one.
        for _ in 0..130 {
            step(&mut sim, &[]);
        }
        // Walk to the office and ask the camera there to come on. The command
        // is legal, reachable and lands; the supply is what refuses.
        stand(&mut sim, "office");
        let cam = load_index(&sim, "cam-1");
        let events = step(
            &mut sim,
            &[Command::Power {
                op: PowerOp::SetCommand {
                    by: Seat::Infiltrator,
                    load: cam,
                    on: true,
                },
            }],
        );
        assert!(
            power_events(&events)
                .iter()
                .any(|e| matches!(e, PowerEvent::Commanded { on: true, .. })),
            "the command itself lands: {:?}",
            power_events(&events)
        );
        step(&mut sim, &[]);
        let report = sim.power_report("cam-1").expect("a load");
        assert!(report.commanded, "it was asked");
        assert!(!report.operating, "and it still does not run");
        assert!(
            matches!(report.why, Some(OffReason::Unavailable { .. })),
            "{:?}",
            report.why
        );
    }

    #[test]
    fn the_backup_camera_carries_on_then_stops() {
        let mut sim = sim();
        stand(&mut sim, "hall");
        let feed = feed_index(&sim, FEED_B);
        let events = step(
            &mut sim,
            &[Command::Power {
                op: PowerOp::CutCable {
                    by: Seat::Infiltrator,
                    feed,
                },
            }],
        );
        assert!(
            power_events(&events)
                .iter()
                .any(|e| matches!(e, PowerEvent::BackupEngaged { .. })),
            "{:?}",
            power_events(&events)
        );
        let cam = sim.power_report("cam-0").expect("a load");
        assert!(cam.operating, "the hall camera is still running");
        for _ in 0..960 {
            step(&mut sim, &[]);
        }
        let spent = sim.power_report("cam-0").expect("a load");
        assert!(!spent.operating, "and now it is not");
    }

    #[test]
    fn the_hacker_cannot_cut_a_cable_but_can_work_the_remote_board() {
        let mut sim = sim();
        step(
            &mut sim,
            &[Command::Pivot {
                target: PivotTarget::Bridge,
            }],
        );
        let feed = feed_index(&sim, FEED_A);
        let events = step(
            &mut sim,
            &[Command::Power {
                op: PowerOp::CutCable {
                    by: Seat::Hacker,
                    feed,
                },
            }],
        );
        assert!(
            power_events(&events).iter().any(|e| matches!(
                e,
                PowerEvent::Denied {
                    reason: PowerDenyReason::NotPhysical,
                    ..
                }
            )),
            "{:?}",
            power_events(&events)
        );

        // The hall board is remote on the maintenance bridge.
        let breaker = breaker_index(&sim, BRK_B);
        let events = step(
            &mut sim,
            &[Command::Power {
                op: PowerOp::Isolate {
                    by: Seat::Hacker,
                    breaker,
                    open: true,
                },
            }],
        );
        assert!(
            power_events(&events)
                .iter()
                .any(|e| matches!(e, PowerEvent::Isolated { .. })),
            "{:?}",
            power_events(&events)
        );
    }

    #[test]
    fn the_exit_door_stays_shut_while_its_mechanism_is_dead() {
        let mut sim = sim();
        stand(&mut sim, "hall");
        let feed = feed_index(&sim, FEED_B);
        step(
            &mut sim,
            &[Command::Power {
                op: PowerOp::CutCable {
                    by: Seat::Infiltrator,
                    feed,
                },
            }],
        );
        for _ in 0..4 {
            step(&mut sim, &[]);
        }
        let exit = sim
            .definition()
            .doors
            .iter()
            .position(|d| door_node_id(d.id.clone()) == DOOR_EXIT);
        if let Some(i) = exit {
            assert_eq!(sim.state().doors[i].open, 0.0, "no supply, no slide");
        }
        assert!(!sim.power_report(DOOR_EXIT).expect("a load").operating);
    }

    #[test]
    fn the_laundry_lights_interlock_holds_the_pump() {
        let mut sim = sim();
        step(&mut sim, &[]);
        let pump = sim.power_report(PUMP_LAUNDRY).expect("a load");
        assert!(!pump.operating);
        assert!(
            matches!(pump.why, Some(OffReason::Interlock { .. })),
            "{:?}",
            pump.why
        );
        // Switch the laundry lights off and the pump is free to run, once its
        // own warm-up is done.
        let lights = sim
            .definition()
            .signal
            .as_ref()
            .and_then(|net| net.index_of(&SignalId::from("light-laundry")));
        if let Some(i) = lights
            && let Some(st) = sim.state.signal.as_mut()
            && let Some(d) = st.devices.get_mut(i)
        {
            d.active = false;
        }
        for _ in 0..260 {
            step(&mut sim, &[]);
        }
        assert!(
            sim.power_report(PUMP_LAUNDRY).expect("a load").operating,
            "the interlock released and the pump warmed up"
        );
    }

    #[test]
    fn inspecting_a_device_explains_which_rule_won() {
        let mut sim = sim();
        step(&mut sim, &[]);
        // A settled floor: the laundry camera is simply running, and says so.
        let running = sim.power_report("cam-2").expect("a load");
        assert!(running.operating);
        assert!(
            running.explain().contains("running"),
            "{}",
            running.explain()
        );

        // Switch it off from the room, then ask why. The answer names the device,
        // the rule that won, and the board it sits on.
        stand(&mut sim, "laundry");
        let load = load_index(&sim, "cam-2");
        step(
            &mut sim,
            &[Command::Power {
                op: PowerOp::SetCommand {
                    by: Seat::Infiltrator,
                    load,
                    on: false,
                },
            }],
        );
        let events = step(
            &mut sim,
            &[Command::Power {
                op: PowerOp::Inspect {
                    by: Seat::Hacker,
                    load,
                },
            }],
        );
        // From the van the hacker cannot reach the laundry camera's panel, so the
        // answer is a refusal; reachable or not, the report is what a player reads.
        assert!(!power_events(&events).is_empty());
        let report = sim.power_report("cam-2").expect("a load");
        assert_eq!(report.why, Some(OffReason::NoCommand));
        let said = report.explain();
        assert!(said.contains("Laundry camera"), "{said}");
        assert!(said.contains("no command"), "{said}");
        assert!(said.contains("feed-b"), "{said}");
    }
}

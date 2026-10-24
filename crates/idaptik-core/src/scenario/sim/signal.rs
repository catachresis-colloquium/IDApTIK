//! Hosting the floor's signal network (ADR-0012) inside the Ghost Lobby run.
//!
//! The network itself lives in [`crate::scenario::signal`]; this is the seam
//! that feeds it the world and applies what it decides:
//!
//! * **Readout** — doors open on their own route or badge, cameras with the
//!   infiltrator in frame, Billy pursuing or investigating, terminals the
//!   hacker has pivoted onto, the vacuum running.
//! * **Access** — the infiltrator touches devices in the room they stand in and
//!   doors within arm's reach; the hacker touches devices whose node is
//!   reachable from the host they are acting from (or that they stand on).
//! * **Effects** — a locked door stays shut against routes and badges, an open
//!   one stays open, a switched-off camera sees nothing, a dark room hides the
//!   infiltrator from cameras and Billy, and a wire into the vacuum routes it.
//!
//! A floor with no signal network takes none of these paths, so every existing
//! replay is unchanged.

use super::GhostLobbySim;
use crate::netsim::Effect;
use crate::netsim::access::can_reach;
use crate::scenario::agents::apply_effects;
use crate::scenario::common::BillyMode;
use crate::scenario::event::Event;
use crate::scenario::floor_graph::{VACUUM_NODE_ID, camera_node_id, door_node_id, light_node_id};
use crate::scenario::mathf::TICK_DT;
use crate::scenario::signal::SignalKind;
use crate::scenario::signal::sim::{
    Access, Readout, Seat, SignalDenyReason, SignalEvent, SignalOp,
};

/// How close (world units) the infiltrator must stand to a door to work its
/// panel, from either side.
pub const DOOR_REACH: f64 = 60.0;

impl GhostLobbySim {
    /// Which devices each seat can touch right now.
    pub fn signal_access(&self) -> Access {
        let mut access = Access::default();
        let Some(net) = &self.def.signal else {
            return access;
        };
        let here = Self::room_id_at(&self.def, self.state.player.x);
        let cx = self.state.player.x + self.def.player.w / 2.0;
        let hacker = &self.state.agents.hacker;
        let from = hacker.vantage_ip();
        let infiltrator = &self.state.agents.infiltrator;
        // Sitting at a console that can wire extends the infiltrator's hands
        // to whatever their own session reaches from where they sit.
        let at_wiring_console = self.state.using_console.as_ref().is_some_and(|id| {
            self.def.consoles.iter().any(|con| {
                &con.id == id
                    && con
                        .caps
                        .contains(&crate::scenario::definition::ConsoleCap::Wiring)
            })
        });
        for d in &net.devices {
            let in_room = here.is_some() && d.room.as_deref() == here;
            let at_door = d.kind == SignalKind::Door
                && d.node.as_ref().is_some_and(|node| {
                    self.def.doors.iter().any(|door| {
                        door_node_id(door.id.clone()) == *node && (door.x - cx).abs() <= DOOR_REACH
                    })
                });
            let via_console = at_wiring_console
                && d.node
                    .as_ref()
                    .and_then(|node| self.graph.node(node))
                    .is_some_and(|n| can_reach(&self.graph, infiltrator.vantage_ip(), n.ip));
            if in_room || at_door || via_console {
                access.infiltrator.insert(d.id.clone());
            }
            let reachable = d
                .node
                .as_ref()
                .and_then(|node| self.graph.node(node))
                .is_some_and(|n| hacker.stands_on(n.ip) || can_reach(&self.graph, from, n.ip));
            if reachable {
                access.hacker.insert(d.id.clone());
            }
        }
        access
    }

    /// The host-owned outputs for this tick, read from the world. Devices the
    /// power network is holding inert come along in the same readout: a camera
    /// with no supply cannot trip, however the wiring is set.
    pub fn signal_readout(&self) -> Readout {
        let mut readout = Readout::default();
        let Some(net) = &self.def.signal else {
            return readout;
        };
        let s = &self.state;
        let floor_dark = s.camera_lockout > 0.0 || s.lights_flicker > 0.0;
        for d in &net.devices {
            let node = d.node.as_deref();
            let high = match d.kind {
                SignalKind::Door => s.doors.iter().any(|door| {
                    Some(door_node_id(door.id.clone()).as_str()) == node && door.open > 0.0
                }),
                SignalKind::Camera => (0..self.def.cameras.len()).any(|i| {
                    Some(camera_node_id(i).as_str()) == node
                        && !floor_dark
                        && self.camera_sees_player(i)
                }),
                SignalKind::Guard => {
                    s.phase == crate::scenario::common::Phase::Crisis
                        && matches!(s.billy.mode, BillyMode::Pursue | BillyMode::Investigate)
                }
                SignalKind::Terminal => node
                    .and_then(|n| self.graph.node(n))
                    .is_some_and(|n| s.agents.hacker.stands_on(n.ip)),
                SignalKind::Vacuum => s.vacuum.active,
                SignalKind::Sensor | SignalKind::Light | SignalKind::Relay => false,
            };
            if high {
                for port in d.kind.host_outputs() {
                    readout.high.insert((d.id.clone(), *port));
                }
            }
        }
        if let (Some(power), Some(pst)) = (&self.def.power, &self.state.power) {
            readout.unpowered = pst.inert_signals(power);
        }
        readout
    }

    /// Whether a camera `i` is blind: a wire switched it off, its supply is
    /// gone, or the lights it needs are out.
    ///
    /// The name is historical — callers ask "can this camera see?" and every
    /// one of those three answers is no. It is the single camera-seam, so the
    /// power leg belongs here rather than being remembered at each call site.
    pub fn signal_blinds_camera(&self, i: usize) -> bool {
        let Some(cam) = self.def.cameras.get(i) else {
            return false;
        };
        self.signal_device_off(&camera_node_id(i))
            || self.power_blinds_camera(i)
            || self.signal_room_dark(cam.room.as_str())
    }

    /// Whether a wire has locked the door on grounded node `node`.
    pub fn signal_door_locked(&self, node: &str) -> bool {
        let (Some(net), Some(st)) = (&self.def.signal, &self.state.signal) else {
            return false;
        };
        net.devices
            .iter()
            .zip(&st.devices)
            .any(|(d, s)| d.node.as_deref() == Some(node) && s.locked)
    }

    /// Whether a wire has switched off `room`'s lights.
    pub fn signal_room_dark(&self, room: &str) -> bool {
        self.signal_device_off(&light_node_id(room)) || self.power_room_dark(room)
    }

    /// Whether the signal device on `node` is off — switched off by a wire, or
    /// with no supply to run at all.
    ///
    /// One seam, two reasons. A device the power model is holding inert is not
    /// operating whatever the wiring says, so every caller that asks "is this
    /// device off?" gets the same answer rather than half of it.
    fn signal_device_off(&self, node: &str) -> bool {
        let (Some(net), Some(st)) = (&self.def.signal, &self.state.signal) else {
            return false;
        };
        net.devices.iter().zip(&st.devices).any(|(d, s)| {
            d.node.as_deref() == Some(node) && (!s.active || self.power_holds_signal_device(&d.id))
        })
    }

    /// `Command::Signal` (immediate; self-guards on paused/ended).
    pub(crate) fn signal_command(&mut self, op: SignalOp) {
        if self.paused || self.state.ended {
            return;
        }
        let Some(net) = &self.def.signal else {
            return;
        };
        let by = op.seat();
        let deny = |reason| Event::Signal {
            signal: SignalEvent::Denied { by, reason },
        };
        let cmd = match op.resolve(net) {
            Ok(cmd) => cmd,
            Err(reason) => {
                self.events.push(deny(reason));
                return;
            }
        };
        let profile = match by {
            Seat::Infiltrator => &net.tuning.infiltrator,
            Seat::Hacker => &net.tuning.hacker,
        };
        if by == Seat::Infiltrator && self.hands_busy() > 0.0 {
            self.events.push(deny(SignalDenyReason::Busy));
            return;
        }
        if f64::from(profile.wire_bandwidth) > self.state.bandwidth {
            self.events.push(deny(SignalDenyReason::NoBandwidth));
            return;
        }
        let access = self.signal_access();
        let Some(st) = self.state.signal.as_mut() else {
            return;
        };
        let events = st.apply(net, &cmd, &access);
        for e in events {
            let cost = match &e {
                SignalEvent::Wired { cost, .. }
                | SignalEvent::Cut { cost, .. }
                | SignalEvent::Inverted { cost, .. }
                | SignalEvent::Reprogrammed { cost, .. } => Some(*cost),
                _ => None,
            };
            if let Some(cost) = cost {
                self.state.bandwidth = (self.state.bandwidth - f64::from(cost.bandwidth)).max(0.0);
                self.state.signal_busy += f64::from(cost.busy_ticks) * TICK_DT;
                if by == Seat::Hacker {
                    self.state.stats.hacker_actions += 1;
                }
            }
            self.events.push(Event::Signal { signal: e });
        }
    }

    /// One tick of the signal network: read the world, step the network,
    /// apply what it holds to the floor. Runs right after the timers, so a
    /// lock lands before anybody moves this tick.
    pub(super) fn system_signal(&mut self) {
        self.state.signal_busy = (self.state.signal_busy - TICK_DT).max(0.0);
        if self.def.signal.is_none() || self.state.signal.is_none() {
            return;
        }
        let readout = self.signal_readout();
        let (Some(net), Some(st)) = (&self.def.signal, self.state.signal.as_mut()) else {
            return;
        };
        let events = st.step(net, &readout);

        // Doors: a lock beats every route and badge; a held door stays open.
        for (d, ds) in net.devices.iter().zip(&st.devices) {
            if d.kind != SignalKind::Door || readout.unpowered.contains(&d.id) {
                continue;
            }
            let Some(door) = self
                .state
                .doors
                .iter_mut()
                .find(|door| d.node.as_deref() == Some(door_node_id(door.id.clone()).as_str()))
            else {
                continue;
            };
            if ds.locked {
                door.open = 0.0;
                door.pending = 0.0;
            } else if ds.open {
                door.open = door.open.max(2.0 * TICK_DT);
            }
        }

        for e in events {
            if matches!(e, SignalEvent::VacuumRouted { .. }) {
                let routed = apply_effects(
                    &self.def,
                    &mut self.state,
                    self.cfg,
                    &[Effect::VacuumRun(VACUUM_NODE_ID.to_owned())],
                );
                self.events.extend(routed);
            }
            self.events.push(Event::Signal { signal: e });
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::scenario::GhostLobbySim;
    use crate::scenario::command::{Button, Command, RunConfig, TickInput, fold};
    use crate::scenario::event::Event;
    use crate::scenario::ghost_lobby::ghost_lobby;
    use crate::scenario::signal::ghost_lobby_wiring::{BRIDGE, ghost_lobby_wired};
    use crate::scenario::signal::sim::{Seat, SignalEvent, SignalIndex, SignalOp, Switch};
    use crate::scenario::signal::{InPort, OutPort};

    fn wired() -> GhostLobbySim {
        GhostLobbySim::new(ghost_lobby_wired(), RunConfig::standard(), 123456).expect("valid")
    }

    fn index(sim: &GhostLobbySim, id: &str) -> SignalIndex {
        let net = sim.definition().signal.as_ref().expect("wired");
        SignalIndex(
            net.devices
                .iter()
                .position(|d| d.id.as_str() == id)
                .expect("device") as u16,
        )
    }

    fn step(sim: &mut GhostLobbySim, cmds: &[Command]) -> Vec<Event> {
        let mut held = Default::default();
        let input: TickInput = fold(cmds, &mut held);
        sim.tick(&input)
    }

    fn signal_events(events: &[Event]) -> Vec<&SignalEvent> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::Signal { signal } => Some(signal),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn an_unwired_floor_offers_no_access_and_no_readout() {
        let sim = GhostLobbySim::new(ghost_lobby(), RunConfig::standard(), 1).expect("valid");
        assert!(sim.signal_access().infiltrator.is_empty());
        assert!(sim.signal_readout().high.is_empty());
        assert!(sim.state().signal.is_none());
    }

    #[test]
    fn the_infiltrator_reaches_the_kitchen_and_the_hacker_only_the_bridge_from_the_van() {
        let sim = wired();
        let access = sim.signal_access();
        assert!(access.infiltrator.contains(&"light-kitchen".into()));
        assert!(!access.infiltrator.contains(&"light-office".into()));
        // From the van the bridge segment answers; the floor waits on a pivot.
        assert!(access.hacker.contains(&BRIDGE.into()));
        assert!(
            !access.hacker.contains(&"door-d1".into()),
            "{:?}",
            access.hacker
        );
    }

    #[test]
    fn pivoting_onto_the_bridge_lets_the_hacker_wire_a_door_open() {
        let mut sim = wired();
        step(
            &mut sim,
            &[Command::Pivot {
                target: crate::scenario::command::PivotTarget::Bridge,
            }],
        );
        assert!(sim.signal_access().hacker.contains(&"door-d1".into()));
        let bandwidth = sim.state().bandwidth;
        let op = SignalOp::Wire {
            by: Seat::Hacker,
            from: index(&sim, BRIDGE),
            from_port: OutPort::Compromised,
            to: index(&sim, "door-d1"),
            to_port: InPort::Open,
        };
        let mut events = step(&mut sim, &[Command::Signal { op }]);
        events.extend(step(&mut sim, &[]));
        events.extend(step(&mut sim, &[]));
        let sig = signal_events(&events);
        assert!(
            sig.iter().any(|e| matches!(e, SignalEvent::Wired { .. })),
            "{sig:?}"
        );
        assert!(sig.iter().any(|e| matches!(e, SignalEvent::Switched { device, what: Switch::Open, on: true } if device.as_str() == "door-d1")), "{sig:?}");
        assert!(
            sim.state().doors[0].open > 0.0,
            "the scenario's own door is open"
        );
        assert!(
            sim.state().bandwidth < bandwidth + 1.0,
            "wiring cost bandwidth"
        );
    }

    #[test]
    fn wiring_by_hand_keeps_the_infiltrator_busy() {
        let mut sim = wired();
        let op = SignalOp::Wire {
            by: Seat::Infiltrator,
            from: index(&sim, "cam-0"),
            from_port: OutPort::Tripped,
            to: index(&sim, "light-kitchen"),
            to_port: InPort::Disable,
        };
        step(&mut sim, &[Command::Signal { op }]);
        assert!(sim.state().signal_busy > 1.0);
        // Busy hands don't walk.
        let x = sim.state().player.x;
        step(
            &mut sim,
            &[Command::SetButton {
                button: Button::Right,
                down: true,
            }],
        );
        assert_eq!(sim.state().player.x, x);
    }

    #[test]
    fn a_dark_room_blinds_its_camera() {
        let mut sim = wired();
        let i = sim
            .definition()
            .cameras
            .iter()
            .position(|c| c.room.as_str() == "hall")
            .expect("hall camera");
        let net_idx = index(&sim, "light-hall");
        if let Some(st) = sim.state.signal.as_mut() {
            st.devices[usize::from(net_idx.0)].active = false;
        }
        assert!(sim.signal_blinds_camera(i));
        assert!(sim.signal_room_dark("hall"));
        assert!(!sim.signal_room_dark("kitchen"));
    }
}

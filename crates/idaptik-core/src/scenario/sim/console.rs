//! Consoles in the Ghost Lobby run: using one, the building's intel log, and
//! the [`ScreenView`] a console shows a seat. See [`crate::scenario::console`].

use super::GhostLobbySim;
use crate::netsim::access::can_reach;
use crate::netsim::session::AgentSession;
use crate::scenario::common::{BillyMode, Phase};
use crate::scenario::console::{
    Estimate, Fix, FixSource, ScreenDevice, ScreenKind, ScreenRoom, ScreenView,
};
use crate::scenario::constants as c;
use crate::scenario::definition::{ConsoleCap, ConsoleDef, ConsolePlace};
use crate::scenario::event::Event;
use crate::scenario::floor_graph::{VACUUM_NODE_ID, camera_node_id, door_node_id, light_node_id};
use crate::scenario::ids::RoomId;
use crate::scenario::mathf::sin;
use crate::scenario::signal::sim::Seat;

/// How close (canvas units) the infiltrator must stand to use a console.
pub const CONSOLE_REACH: f64 = 40.0;
/// Uncertainty of a fresh fix, and how fast it grows per second (a fraction
/// of walking speed: people mostly creep).
const FIX_BASE_RADIUS: f64 = 22.0;
const FIX_GROWTH: f64 = 0.6;
const FIX_MAX_RADIUS: f64 = 420.0;

impl GhostLobbySim {
    /// The console the infiltrator is standing at, if any.
    pub fn console_in_reach(&self) -> Option<&ConsoleDef> {
        let cx = self.state.player.x + self.def.player.w / 2.0;
        let here = Self::room_id_at(&self.def, self.state.player.x)?;
        self.def.consoles.iter().find(|con| match &con.place {
            ConsolePlace::Room { room, x } => {
                room.as_str() == here && (x - cx).abs() <= CONSOLE_REACH
            }
            ConsolePlace::Van => false,
        })
    }

    /// The infiltrator's console interaction: interact at a console sits down
    /// at it; interact again steps away. Returns whether a console was in
    /// reach (so lower-priority interactions do not also fire).
    pub(super) fn console_interaction(&mut self, pressed: bool) -> bool {
        if let Some(id) = self.state.using_console.clone() {
            if pressed {
                self.state.using_console = None;
                self.events.push(Event::ConsoleUsed {
                    console: id,
                    on: false,
                });
            }
            return true;
        }
        let Some(id) = self.console_in_reach().map(|con| con.id.clone()) else {
            return false;
        };
        if pressed {
            self.state.using_console = Some(id.clone());
            self.state.player.vx = 0.0;
            self.events.push(Event::ConsoleUsed {
                console: id,
                on: true,
            });
        }
        true
    }

    /// Whether camera `i` frames canvas `x` in `room` right now (geometry,
    /// power, loops and wires — not the floor-wide lockout).
    fn camera_frames(&self, i: usize, x: f64, room: &str) -> bool {
        let Some(cam) = self.def.cameras.get(i) else {
            return false;
        };
        let s = &self.state;
        if cam.room.as_str() != room
            || cam.stale
            || s.camera_looped.get(i).is_some_and(|left| *left > 0.0)
            || s.dead_nodes.contains(&camera_node_id(i))
            || self.signal_blinds_camera(i)
        {
            return false;
        }
        let sweep = sin(s.t * c::CAM_SWEEP_W + cam.phase) * c::CAM_SWEEP_A;
        x >= cam.range.0 + sweep.min(0.0) && x <= cam.range.1 + sweep.max(0.0)
    }

    /// Log what the building saw this tick. Only on floors with consoles, so
    /// the canonical floor's state and replays are untouched.
    pub(super) fn system_intel(&mut self) {
        if self.def.consoles.is_empty() {
            return;
        }
        let s = &self.state;
        let t = s.t;
        let floor_dark = s.camera_lockout > 0.0 || s.lights_flicker > 0.0;
        let cx = s.player.x + self.def.player.w / 2.0;
        let here: Option<RoomId> = Self::room_id_at(&self.def, s.player.x).map(RoomId::from);

        // A door controller logs the infiltrator passing through.
        let mut fix = None;
        if let (Some(now), Some(before)) = (&here, &s.intel.last_room)
            && now != before
            && let Some(door) = Self::nearest_door(&s.doors, cx).and_then(|i| s.doors.get(i))
        {
            fix = Some(Fix {
                x: door.x,
                t,
                room: now.clone(),
                source: FixSource::Door {
                    door: door.id.clone(),
                },
            });
        }
        // A live camera is better than a door log.
        if !floor_dark
            && let Some(room) = &here
            && let Some(index) = (0..self.def.cameras.len()).find(|&i| self.camera_sees_player(i))
        {
            fix = Some(Fix {
                x: cx,
                t,
                room: room.clone(),
                source: FixSource::Camera { index },
            });
        }
        // Billy is only ever seen by cameras: he carries nothing that logs.
        let b = &s.billy;
        let bx = b.x + self.def.billy.w / 2.0;
        let billy_fix = if !floor_dark && b.mode != BillyMode::Offsite && s.phase == Phase::Crisis {
            Self::room_id_at(&self.def, b.x).and_then(|room| {
                (0..self.def.cameras.len())
                    .find(|&i| self.camera_frames(i, bx, room))
                    .map(|index| Fix {
                        x: bx,
                        t,
                        room: RoomId::from(room),
                        source: FixSource::Camera { index },
                    })
            })
        } else {
            None
        };

        let intel = &mut self.state.intel;
        intel.last_room = here;
        if fix.is_some() {
            intel.infiltrator = fix;
        }
        if billy_fix.is_some() {
            intel.billy = billy_fix;
        }
    }

    /// The network session a seat looks through.
    fn session(&self, seat: Seat) -> &AgentSession {
        match seat {
            Seat::Hacker => &self.state.agents.hacker,
            Seat::Infiltrator => &self.state.agents.infiltrator,
        }
    }

    /// Whether `seat` can reach grounded node `node` right now.
    fn seat_reaches(&self, seat: Seat, node: &str) -> bool {
        let session = self.session(seat);
        self.graph.node(node).is_some_and(|n| {
            session.stands_on(n.ip) || can_reach(&self.graph, session.vantage_ip(), n.ip)
        })
    }

    /// Whether the device that produced `fix` is still within `seat`'s reach
    /// and shown by a console with `caps`.
    fn fix_visible(&self, fix: &Fix, seat: Seat, caps: &[ConsoleCap]) -> bool {
        match &fix.source {
            FixSource::Camera { index } => {
                caps.contains(&ConsoleCap::Cameras)
                    && self.seat_reaches(seat, &camera_node_id(*index))
            }
            FixSource::Door { door } => {
                caps.contains(&ConsoleCap::Building)
                    && self.seat_reaches(seat, &door_node_id(door.clone()))
            }
        }
    }

    fn estimate(&self, fix: &Fix) -> Estimate {
        let age = (self.state.t - fix.t).max(0.0);
        let radius =
            (FIX_BASE_RADIUS + self.preset.player_speed * FIX_GROWTH * age).min(FIX_MAX_RADIUS);
        Estimate {
            x: fix.x,
            room: fix.room.clone(),
            radius,
            age,
            live: age < 0.05,
        }
    }

    /// What console `id` shows `seat` right now, or `None` for no such console.
    pub fn screen_view(&self, id: &str, seat: Seat) -> Option<ScreenView> {
        let con = self.def.consoles.iter().find(|c| c.id == id)?;
        let caps = &con.caps;
        let s = &self.state;
        let mut devices = Vec::new();
        let index_of = |node: &str| {
            self.graph
                .nodes
                .iter()
                .position(|n| n.id == node)
                .map(|i| i as u32)
        };
        // The hacker acts from their own kit; a screen the infiltrator sits
        // at shows, but does not yet act.
        let actionable = seat == Seat::Hacker && con.place == ConsolePlace::Van;
        let mut push = |node: String,
                        kind: ScreenKind,
                        label: String,
                        x: f64,
                        room: Option<RoomId>,
                        status: String| {
            if !self.seat_reaches(seat, &node) {
                return;
            }
            let Some(net_index) = index_of(&node) else {
                return;
            };
            let status = if s.dead_nodes.contains(&node) {
                "NO POWER".to_owned()
            } else {
                status
            };
            devices.push(ScreenDevice {
                node,
                kind,
                label,
                x,
                room,
                status,
                net_index,
                actionable,
            });
        };

        if caps.contains(&ConsoleCap::Building) {
            for (def_door, door) in self.def.doors.iter().zip(&s.doors) {
                let node = door_node_id(door.id.clone());
                let locked = self.signal_door_locked(&node);
                let status = if locked {
                    "LOCKED".to_owned()
                } else if door.open > 0.0 {
                    format!("OPEN {:.1}s", door.open)
                } else if door.pending > 0.0 {
                    format!("ROUTING {:.1}s", door.pending)
                } else {
                    "SHUT".to_owned()
                };
                let room = Self::room_id_at(&self.def, door.x).map(RoomId::from);
                push(
                    node,
                    ScreenKind::Door,
                    def_door.label.clone(),
                    door.x,
                    room,
                    status,
                );
            }
            for room in &self.def.rooms {
                let node = light_node_id(room.id.as_str());
                let status = if self.signal_room_dark(room.id.as_str()) {
                    "OFF".to_owned()
                } else if s.lights_flicker > 0.0 {
                    "FLICKER".to_owned()
                } else {
                    "ON".to_owned()
                };
                push(
                    node,
                    ScreenKind::Light,
                    format!("{} lights", room.name),
                    room.x + room.w * 0.5,
                    Some(room.id.clone()),
                    status,
                );
            }
            let v = &s.vacuum;
            let status = if v.fallen {
                "GONE"
            } else if v.active {
                "RUNNING"
            } else {
                "DOCKED"
            };
            let room = Self::room_id_at(&self.def, v.x).map(RoomId::from);
            push(
                VACUUM_NODE_ID.to_owned(),
                ScreenKind::Vacuum,
                "vacuum".to_owned(),
                v.x,
                room,
                status.to_owned(),
            );
        }
        if caps.contains(&ConsoleCap::Cameras) {
            for (i, cam) in self.def.cameras.iter().enumerate() {
                let status = if self.signal_blinds_camera(i) {
                    "OFF".to_owned()
                } else if let Some(left) = s.camera_looped.get(i).filter(|l| **l > 0.0) {
                    format!("LOOPED {left:.0}s")
                } else if cam.stale {
                    "STALE FEED".to_owned()
                } else {
                    "LIVE".to_owned()
                };
                push(
                    camera_node_id(i),
                    ScreenKind::Camera,
                    cam.id.as_str().to_owned(),
                    cam.x,
                    Some(cam.room.clone()),
                    status,
                );
            }
        }

        let infiltrator = s
            .intel
            .infiltrator
            .as_ref()
            .filter(|f| self.fix_visible(f, seat, caps))
            .map(|f| self.estimate(f));
        let billy = s
            .intel
            .billy
            .as_ref()
            .filter(|f| self.fix_visible(f, seat, caps))
            .map(|f| self.estimate(f));
        let session = self.session(seat);
        Some(ScreenView {
            console: con.id.clone(),
            label: con.label.clone(),
            seat,
            rooms: self
                .def
                .rooms
                .iter()
                .map(|r| ScreenRoom {
                    id: r.id.clone(),
                    name: r.name.clone(),
                    x: r.x,
                    w: r.w,
                })
                .collect(),
            devices,
            infiltrator,
            billy,
            wiring: caps.contains(&ConsoleCap::Wiring) && self.def.signal.is_some(),
            trace: session.trace_fraction(),
            hops: session.hops(),
            bandwidth: s.bandwidth,
        })
    }
}

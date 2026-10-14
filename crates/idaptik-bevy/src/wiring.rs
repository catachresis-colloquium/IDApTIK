//! The wiring overlay: the floor's signal network (ADR-0012), drawn on a
//! console's screen and re-wired with the mouse.
//!
//! Wiring is screen work. It is drawn on the schematic of a console that can
//! show it (`C` while a screen is on), never floating over the infiltrator's
//! world — art rule 5. The mouse wires as the seat whose screen it is over
//! (`B` chooses which seats are on screen). Both can wire, from
//! different places at different prices: the infiltrator from a console they
//! sit at, slowly and for free; the hacker from the van deck, anything
//! reachable from the host they act from, instantly, for bandwidth, and alone
//! can re-program relays and wire into hacker-only devices.
//!
//! Drag from an output port (right of a device) to an input port (left of a
//! device) to lay a wire. Right-click a wire to cut it; Ctrl+right-click to
//! invert it. Click a relay to cycle its gate. Every gesture becomes a
//! `Command::Signal` in the same queue the keyboard fills; nothing here
//! decides anything.
//!
//! Colour follows the storyboard's seat law: everything wireable is phosphor;
//! the installer's hardwired runs are grey; a wire carrying a high signal is
//! drawn bright. Devices the active seat cannot touch are dimmed. A locked
//! door is marked red and a room whose lights a wire has killed is shaded
//! whether or not the overlay is open — those are facts about the world, not
//! about the wiring.

use bevy::prelude::*;
use idaptik_core::scenario::GhostLobbySim;
use idaptik_core::scenario::command::Command;
use idaptik_core::scenario::floor_graph::{camera_node_id, door_node_id, light_node_id};
use idaptik_core::scenario::signal::sim::{Access, Seat, SignalIndex, SignalOp};
use idaptik_core::scenario::signal::{Gate, InPort, OutPort, SignalKind};

use crate::driver::{CommandQueue, SimState};
use crate::scene::{CEILING_Y, GhostLobbySceneMarker, SceneMap};
use crate::screen::{Place, Screen, ScreenMap};

use crate::palette::{PHOSPHOR, SIGNAL};

const HARDWIRED: Color = Color::srgba(0.55, 0.57, 0.62, 0.7);

/// The seat law: the hacker wires in phosphor, the infiltrator in signal yellow.
fn seat_tone(seat: Seat) -> Color {
    match seat {
        Seat::Hacker => PHOSPHOR,
        Seat::Infiltrator => SIGNAL,
    }
}

/// Port marker radius, and how close a click must land to hit one.
pub const PORT_R: f32 = 5.0;
pub const PORT_HIT: f32 = 10.0;
/// How close a right-click must land to a wire to hit it.
pub const WIRE_HIT: f32 = 7.0;
/// Vertical spacing of a device's stacked ports.
const PORT_GAP: f32 = 14.0;
/// Horizontal offset of a device's ports from its anchor.
const PORT_SIDE: f32 = 16.0;

/// Overlay state: shown or not, the seat played (with both seats on screen,
/// the one under the cursor), and a drag in progress.
#[derive(Resource)]
pub struct Wiring {
    pub shown: bool,
    pub seat: Seat,
    /// The output port a drag started from, and on whose screen.
    pub drag: Option<(usize, OutPort, Seat)>,
}

impl Default for Wiring {
    fn default() -> Self {
        Self {
            shown: false,
            seat: Seat::Infiltrator,
            drag: None,
        }
    }
}

/// One device's place on screen and where its ports sit.
#[derive(Debug, Clone)]
pub struct DeviceLayout {
    pub index: usize,
    pub anchor: Vec2,
    pub outs: Vec<(OutPort, Vec2)>,
    pub ins: Vec<(InPort, Vec2)>,
}

/// Where device `i` stands, in canvas coordinates (x right, y down). Fixtures
/// sit on the same rows the screen draws them on (see
/// [`crate::screen::device_row`]), so the ports wrap the screen's own device
/// squares. Everything without a fixture — the bridge terminal, its relay,
/// and Billy, who is a signal source but never a picture — sits in a plant
/// strip above the floor plan.
fn canvas_anchor(sim: &GhostLobbySim, i: usize) -> Option<(f64, f64)> {
    use crate::screen::device_row;
    use idaptik_core::scenario::console::ScreenKind;
    let def = sim.definition();
    let net = def.signal.as_ref()?;
    let d = net.devices.get(i)?;
    let s = sim.state();
    let node = d.node.as_deref();
    let floor = def.world.floor;
    let row = |k: ScreenKind| device_row(k, floor).0;
    match d.kind {
        SignalKind::Door => def
            .doors
            .iter()
            .find(|door| Some(door_node_id(door.id.clone()).as_str()) == node)
            .map(|door| (door.x, row(ScreenKind::Door))),
        SignalKind::Camera => def
            .cameras
            .iter()
            .enumerate()
            .find(|(k, _)| Some(camera_node_id(*k).as_str()) == node)
            .map(|(_, cam)| (cam.x, row(ScreenKind::Camera))),
        SignalKind::Light => def
            .rooms
            .iter()
            .find(|r| Some(light_node_id(r.id.as_str()).as_str()) == node)
            .map(|r| (r.x + r.w * 0.5, row(ScreenKind::Light))),
        SignalKind::Vacuum => Some((s.vacuum.x, row(ScreenKind::Vacuum))),
        SignalKind::Terminal | SignalKind::Relay | SignalKind::Guard | SignalKind::Sensor => {
            // The plant strip: left to right in definition order.
            let k = net.devices[..i]
                .iter()
                .filter(|o| {
                    matches!(
                        o.kind,
                        SignalKind::Terminal
                            | SignalKind::Relay
                            | SignalKind::Guard
                            | SignalKind::Sensor
                    )
                })
                .count();
            Some((140.0 + k as f64 * 220.0, CEILING_Y - 70.0))
        }
    }
}

/// Whether the screen already labels device `i` itself (fixtures do; the
/// plant strip does not).
fn screen_labels(kind: SignalKind) -> bool {
    matches!(
        kind,
        SignalKind::Door | SignalKind::Camera | SignalKind::Light | SignalKind::Vacuum
    )
}

/// Every signal device's on-screen layout, in definition order.
pub fn layout(sim: &GhostLobbySim, map: &impl Place) -> Vec<DeviceLayout> {
    let Some(net) = sim.definition().signal.as_ref() else {
        return Vec::new();
    };
    net.devices
        .iter()
        .enumerate()
        .filter_map(|(i, d)| {
            let (x, y) = canvas_anchor(sim, i)?;
            let anchor = map.place(x, y);
            let stack =
                |n: usize, k: usize| (n as f32 - 1.0) * PORT_GAP * 0.5 - k as f32 * PORT_GAP;
            let outs = d.kind.outputs();
            let ins = d.kind.inputs();
            Some(DeviceLayout {
                index: i,
                anchor,
                outs: outs
                    .iter()
                    .enumerate()
                    .map(|(k, p)| (*p, anchor + Vec2::new(PORT_SIDE, stack(outs.len(), k))))
                    .collect(),
                ins: ins
                    .iter()
                    .enumerate()
                    .map(|(k, p)| (*p, anchor + Vec2::new(-PORT_SIDE, stack(ins.len(), k))))
                    .collect(),
            })
        })
        .collect()
}

/// The output port nearest `at`, within [`PORT_HIT`].
pub fn out_port_at(layouts: &[DeviceLayout], at: Vec2) -> Option<(usize, OutPort)> {
    layouts
        .iter()
        .flat_map(|l| l.outs.iter().map(move |(p, pos)| (l.index, *p, *pos)))
        .filter(|(_, _, pos)| pos.distance(at) <= PORT_HIT)
        .min_by(|a, b| a.2.distance(at).total_cmp(&b.2.distance(at)))
        .map(|(i, p, _)| (i, p))
}

/// The input port nearest `at`, within [`PORT_HIT`].
pub fn in_port_at(layouts: &[DeviceLayout], at: Vec2) -> Option<(usize, InPort)> {
    layouts
        .iter()
        .flat_map(|l| l.ins.iter().map(move |(p, pos)| (l.index, *p, *pos)))
        .filter(|(_, _, pos)| pos.distance(at) <= PORT_HIT)
        .min_by(|a, b| a.2.distance(at).total_cmp(&b.2.distance(at)))
        .map(|(i, p, _)| (i, p))
}

/// The position of one port, if the device is laid out.
fn out_pos(layouts: &[DeviceLayout], i: usize, p: OutPort) -> Option<Vec2> {
    let l = layouts.iter().find(|l| l.index == i)?;
    l.outs.iter().find(|(q, _)| *q == p).map(|(_, pos)| *pos)
}

fn in_pos(layouts: &[DeviceLayout], i: usize, p: InPort) -> Option<Vec2> {
    let l = layouts.iter().find(|l| l.index == i)?;
    l.ins.iter().find(|(q, _)| *q == p).map(|(_, pos)| *pos)
}

/// Distance from `p` to the segment `a`–`b`.
fn segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = if ab.length_squared() > 0.0 {
        ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(a + ab * t)
}

/// The wire nearest `at` within [`WIRE_HIT`]: its index in the live link list.
pub fn wire_at(sim: &GhostLobbySim, layouts: &[DeviceLayout], at: Vec2) -> Option<usize> {
    let net = sim.definition().signal.as_ref()?;
    let st = sim.state().signal.as_ref()?;
    st.links
        .iter()
        .enumerate()
        .filter_map(|(k, l)| {
            let a = out_pos(layouts, net.index_of(&l.from)?, l.from_port)?;
            let b = in_pos(layouts, net.index_of(&l.to)?, l.to_port)?;
            Some((k, segment_distance(at, a, b)))
        })
        .filter(|(_, d)| *d <= WIRE_HIT)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(k, _)| k)
}

fn seat_can(access: &Access, seat: Seat, id: &idaptik_core::scenario::ids::SignalId) -> bool {
    access.can(seat, id)
}

//## Input

/// `C` shows/hides the wiring on a screen that can show it. Runs in local
/// play only.
pub fn wiring_keys(
    keys: Res<ButtonInput<KeyCode>>,
    sim: Res<SimState>,
    screen: Res<Screen>,
    mut wiring: ResMut<Wiring>,
) {
    let can_wire = !wiring_seats(&sim, &screen).is_empty();
    if !can_wire {
        wiring.shown = false;
        wiring.drag = None;
    } else if keys.just_pressed(KeyCode::KeyC) {
        wiring.shown = !wiring.shown;
        wiring.drag = None;
    }
}

/// The seats whose screen is on and shows the wiring.
pub fn wiring_seats(sim: &SimState, screen: &Screen) -> Vec<Seat> {
    [Seat::Infiltrator, Seat::Hacker]
        .into_iter()
        .filter(|s| crate::screen::current_view(sim, screen, *s).is_some_and(|v| v.wiring))
        .collect()
}

/// The window and the cameras that show screens: which seat's schematic the
/// cursor is over, and where on it.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Cursor<'w, 's> {
    windows: Query<'w, 's, &'static Window>,
    cameras: Query<
        'w,
        's,
        (
            &'static Camera,
            &'static GlobalTransform,
            Option<&'static crate::seats::SeatCam>,
            Has<crate::seats::DeckCam>,
        ),
    >,
}

impl Cursor<'_, '_> {
    /// The seat whose screen (among `seats`) is under the cursor, and the
    /// cursor's place in that screen's world space.
    fn on_screen(&self, seats: &[Seat]) -> Option<(Seat, Vec2)> {
        let cursor = self.windows.single().ok()?.cursor_position()?;
        self.cameras
            .iter()
            .find_map(|(camera, transform, seat_cam, deck)| {
                // The hacker's screen is the deck layer; the infiltrator's is
                // their own camera, cut to the console's schematic.
                let seat = match (seat_cam, deck) {
                    (_, true) => Seat::Hacker,
                    (Some(crate::seats::SeatCam(Seat::Infiltrator)), false) => Seat::Infiltrator,
                    _ => return None,
                };
                let over = camera.is_active
                    && camera
                        .logical_viewport_rect()
                        .is_some_and(|r| r.contains(cursor));
                if !over || !seats.contains(&seat) {
                    return None;
                }
                let at = camera.viewport_to_world_2d(transform, cursor).ok()?;
                Some((seat, at))
            })
    }
}

/// The wiring HUD line's text and colour.
type HudText<'w, 's> = Query<
    'w,
    's,
    (&'static mut Text, &'static mut TextColor),
    (With<WiringHud>, Without<WiringLabel>),
>;

/// Mouse gestures → `Command::Signal`.
pub fn wiring_mouse(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    cursor: Cursor,
    sim: Res<SimState>,
    screen: Res<Screen>,
    mut wiring: ResMut<Wiring>,
    mut queue: ResMut<CommandQueue>,
) {
    // Shown only ever means "on a screen that can wire" (see `wiring_keys`).
    if !wiring.shown {
        return;
    }
    let def = sim.sim.definition();
    let Some(net) = def.signal.as_ref() else {
        return;
    };
    let Some((by, at)) = cursor.on_screen(&wiring_seats(&sim, &screen)) else {
        return;
    };
    let layouts = layout(
        &sim.sim,
        &ScreenMap::new(def.world.width, def.world.floor, by),
    );
    let ix = |i: usize| SignalIndex(i as u16);

    if buttons.just_pressed(MouseButton::Left) {
        if let Some((i, p)) = out_port_at(&layouts, at) {
            wiring.drag = Some((i, p, by));
        } else if let Some(relay) = layouts.iter().find(|l| {
            net.devices[l.index].kind == SignalKind::Relay
                && l.anchor.distance(at) <= PORT_HIT * 1.5
        }) {
            let current = sim
                .sim
                .state()
                .signal
                .as_ref()
                .and_then(|st| st.devices.get(relay.index))
                .and_then(|d| d.gate)
                .unwrap_or(Gate::And);
            queue.push(Command::Signal {
                op: SignalOp::Reprogram {
                    by,
                    relay: ix(relay.index),
                    gate: next_gate(current),
                },
            });
        }
    }
    if buttons.just_released(MouseButton::Left)
        && let Some((from, from_port, on)) = wiring.drag.take()
        && on == by
        && let Some((to, to_port)) = in_port_at(&layouts, at)
    {
        queue.push(Command::Signal {
            op: SignalOp::Wire {
                by,
                from: ix(from),
                from_port,
                to: ix(to),
                to_port,
            },
        });
    }
    if buttons.just_pressed(MouseButton::Right)
        && let Some(k) = wire_at(&sim.sim, &layouts, at)
        && let Some(link) = sim
            .sim
            .state()
            .signal
            .as_ref()
            .and_then(|st| st.links.get(k))
        && let (Some(from), Some(to)) = (net.index_of(&link.from), net.index_of(&link.to))
    {
        let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
        let (from, from_port, to, to_port) = (ix(from), link.from_port, ix(to), link.to_port);
        let op = if ctrl {
            SignalOp::Invert {
                by,
                from,
                from_port,
                to,
                to_port,
            }
        } else {
            SignalOp::Cut {
                by,
                from,
                from_port,
                to,
                to_port,
            }
        };
        queue.push(Command::Signal { op });
    }
}

/// The gate a relay click moves to.
pub fn next_gate(g: Gate) -> Gate {
    match g {
        Gate::And => Gate::Or,
        Gate::Or => Gate::Xor,
        Gate::Xor => Gate::Nand,
        Gate::Nand => Gate::Nor,
        Gate::Nor => Gate::Latch,
        Gate::Latch => Gate::And,
    }
}

//## Drawing

/// Marker: one device's overlay label.
#[derive(Component)]
pub struct WiringLabel(pub usize);

/// Marker: a room's darkness shade.
#[derive(Component)]
pub struct DarkShade(pub String);

/// Marker: the wiring HUD line.
#[derive(Component)]
pub struct WiringHud;

/// Spawn the per-device labels, the room shades and the HUD line.
pub fn setup_wiring(mut commands: Commands, sim: Res<SimState>) {
    let def = sim.sim.definition();
    let map = SceneMap::new(def.world.width, def.world.floor, def.floor_id);
    let room_h = def.world.floor - CEILING_Y;
    for room in &def.rooms {
        commands.spawn((
            GhostLobbySceneMarker,
            DarkShade(room.id.as_str().to_owned()),
            Sprite::from_color(
                Color::srgba(0.0, 0.0, 0.02, 0.62),
                Vec2::new(room.w as f32 - 6.0, room_h as f32),
            ),
            Transform::from_translation(
                map.box_center(room.x, CEILING_Y, room.w, room_h)
                    .extend(1.2),
            ),
            Visibility::Hidden,
        ));
    }
    if let Some(net) = &def.signal {
        for (i, d) in net.devices.iter().enumerate() {
            commands.spawn((
                GhostLobbySceneMarker,
                WiringLabel(i),
                Text2d::new(d.id.as_str().to_owned()),
                TextFont::from_font_size(10.0),
                TextColor(PHOSPHOR),
                Transform::from_xyz(0.0, 0.0, 6.0),
                Visibility::Hidden,
            ));
        }
    }
    commands.spawn((
        GhostLobbySceneMarker,
        WiringHud,
        crate::deck::InPane::hud(None, Some(8.0), Some(12.0), None),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(8.0),
            right: Val::Px(12.0),
            ..Default::default()
        },
        Text::new(""),
        TextFont::from_font_size(12.0),
        TextColor(PHOSPHOR),
    ));
}

/// Keep labels, shades and the HUD line in step with the run.
pub fn sync_wiring(
    sim: Res<SimState>,
    screen: Res<Screen>,
    wiring: Res<Wiring>,
    mut labels: Query<(
        &WiringLabel,
        &mut Transform,
        &mut Visibility,
        &mut Text2d,
        &mut TextColor,
    )>,
    mut shades: Query<(&DarkShade, &mut Visibility), Without<WiringLabel>>,
    mut hud: HudText,
) {
    let sim_ref = &sim.sim;
    for (shade, mut vis) in &mut shades {
        *vis = if sim_ref.signal_room_dark(&shade.0) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    let Some(net) = sim_ref.definition().signal.as_ref() else {
        return;
    };
    let def = sim_ref.definition();
    // Labels go on one screen: the seat played's, else the other's.
    let can = wiring_seats(&sim, &screen);
    let on = if can.contains(&wiring.seat) {
        wiring.seat
    } else {
        can.first().copied().unwrap_or(wiring.seat)
    };
    let layouts = layout(
        sim_ref,
        &ScreenMap::new(def.world.width, def.world.floor, on),
    );
    let access = sim_ref.signal_access();
    let st = sim_ref.state().signal.as_ref();
    for (label, mut transform, mut vis, mut text, mut color) in &mut labels {
        let Some(l) = layouts.iter().find(|l| l.index == label.0) else {
            continue;
        };
        // The screen labels its own fixtures; the overlay labels the rest.
        *vis = if wiring.shown && !screen_labels(net.devices[label.0].kind) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        transform.translation = (l.anchor + Vec2::new(0.0, 22.0)).extend(9.0);
        let d = &net.devices[label.0];
        let gate = st
            .and_then(|st| st.devices.get(label.0))
            .and_then(|s| s.gate)
            .map(|g| format!(" [{g:?}]"))
            .unwrap_or_default();
        text.0 = format!("{}{gate}", d.id);
        let tone = seat_tone(on);
        color.0 = if seat_can(&access, on, &d.id) {
            tone
        } else {
            tone.with_alpha(0.22)
        };
    }
    if let Ok((mut text, mut color)) = hud.single_mut() {
        let s = sim_ref.state();
        let laid = st.map(|st| st.wires_laid()).unwrap_or(0);
        let (seat, badge) = match wiring.seat {
            Seat::Hacker => ("HACKER", PHOSPHOR),
            Seat::Infiltrator => ("INFILTRATOR", SIGNAL),
        };
        color.0 = badge;
        text.0 = if wiring.shown {
            format!(
                "WIRING as {seat}   wires {laid}/{}   bandwidth {:.0}{}\n\
                 drag output -> input to wire / right-click cut / ctrl+right-click invert / click relay to re-program / C close",
                net.budget,
                s.bandwidth,
                if s.signal_busy > 0.0 {
                    format!("   hands busy {:.1}s", s.signal_busy)
                } else {
                    String::new()
                }
            )
        } else {
            // What the use key would do right now.
            match (wiring.seat, screen.on(wiring.seat)) {
                (_, true) => {
                    if !can.is_empty() {
                        "C: wiring".to_owned()
                    } else {
                        String::new()
                    }
                }
                (Seat::Hacker, false) => "H: van deck".to_owned(),
                (Seat::Infiltrator, false) => sim_ref
                    .console_in_reach()
                    .map(|c| format!("E: use {}", c.label))
                    .unwrap_or_default(),
            }
        };
    }
}

/// Gizmo pass on the screen: device markers, ports, wires, the drag line.
pub fn draw_wiring(
    sim: Res<SimState>,
    screen: Res<Screen>,
    wiring: Res<Wiring>,
    cursor: Cursor,
    mut gizmos: Gizmos,
) {
    let sim_ref = &sim.sim;
    let (Some(net), Some(st)) = (
        sim_ref.definition().signal.as_ref(),
        sim_ref.state().signal.as_ref(),
    ) else {
        return;
    };
    if !wiring.shown {
        return;
    }
    // The wiring itself is drawn on each screen's schematic that shows it,
    // in that seat's colour.
    let def = sim_ref.definition();
    let access = sim_ref.signal_access();
    let outs = st.outputs(net, &sim_ref.signal_readout());
    let can = wiring_seats(&sim, &screen);
    for &on in &can {
        let layouts = layout(
            sim_ref,
            &ScreenMap::new(def.world.width, def.world.floor, on),
        );

        for l in &layouts {
            let d = &net.devices[l.index];
            let touchable = seat_can(&access, on, &d.id);
            let seat = seat_tone(on);
            let tone = if touchable {
                seat
            } else {
                seat.with_alpha(0.22)
            };
            gizmos.rect_2d(l.anchor, Vec2::splat(12.0), tone);
            for (p, pos) in &l.outs {
                let high = outs
                    .get(l.index)
                    .is_some_and(|b| b & (1 << (*p as u8)) != 0);
                gizmos.circle_2d(*pos, PORT_R, tone);
                if high {
                    gizmos.circle_2d(*pos, PORT_R * 0.5, seat);
                }
            }
            for (_, pos) in &l.ins {
                gizmos.rect_2d(*pos, Vec2::splat(PORT_R * 1.6), tone);
            }
        }

        for link in &st.links {
            let (Some(fi), Some(ti)) = (net.index_of(&link.from), net.index_of(&link.to)) else {
                continue;
            };
            let (Some(a), Some(b)) = (
                out_pos(&layouts, fi, link.from_port),
                in_pos(&layouts, ti, link.to_port),
            ) else {
                continue;
            };
            let high = outs
                .get(fi)
                .is_some_and(|bits| bits & (1 << (link.from_port as u8)) != 0)
                != link.invert;
            let color = match (link.builtin, high) {
                (true, false) => HARDWIRED,
                (true, true) => Color::srgb(0.85, 0.86, 0.9),
                (false, false) => Color::srgba(0.50, 0.88, 0.69, 0.55),
                (false, true) => PHOSPHOR,
            };
            gizmos.line_2d(a, b, color);
            if high {
                gizmos.line_2d(a + Vec2::Y, b + Vec2::Y, color);
            }
            if link.invert {
                gizmos.circle_2d(b + (a - b).normalize_or_zero() * 9.0, 3.0, color);
            }
        }

        if let Some((i, p, by)) = wiring.drag
            && by == on
            && let Some(a) = out_pos(&layouts, i, p)
            && let Some((over, cursor)) = cursor.on_screen(&can)
            && over == on
        {
            gizmos.line_2d(a, cursor, seat_tone(on));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use idaptik_core::RunConfig;
    use idaptik_core::scenario::signal::ghost_lobby_wiring::ghost_lobby_wired;

    fn sim() -> GhostLobbySim {
        GhostLobbySim::new(ghost_lobby_wired(), RunConfig::standard(), 7).expect("valid")
    }

    fn map(sim: &GhostLobbySim) -> SceneMap {
        let def = sim.definition();
        SceneMap::new(def.world.width, def.world.floor, def.floor_id)
    }

    #[test]
    fn every_device_is_laid_out_with_every_port() {
        let sim = sim();
        let layouts = layout(&sim, &map(&sim));
        let net = sim.definition().signal.as_ref().expect("wired");
        // Billy is a signal source in the plant strip, never a picture, so
        // every device is laid out from the start.
        assert_eq!(layouts.len(), net.devices.len());
        for l in &layouts {
            let kind = net.devices[l.index].kind;
            assert_eq!(l.outs.len(), kind.outputs().len());
            assert_eq!(l.ins.len(), kind.inputs().len());
        }
    }

    #[test]
    fn a_click_on_a_port_finds_that_port() {
        let sim = sim();
        let layouts = layout(&sim, &map(&sim));
        for l in &layouts {
            for (p, pos) in &l.outs {
                assert_eq!(out_port_at(&layouts, *pos), Some((l.index, *p)));
            }
            for (p, pos) in &l.ins {
                assert_eq!(in_port_at(&layouts, *pos), Some((l.index, *p)));
            }
        }
    }

    #[test]
    fn the_hardwired_interlock_is_hit_by_a_click_on_its_midpoint() {
        let sim = sim();
        let layouts = layout(&sim, &map(&sim));
        let net = sim.definition().signal.as_ref().expect("wired");
        let st = sim.state().signal.as_ref().expect("state");
        let link = &st.links[2];
        let a = out_pos(
            &layouts,
            net.index_of(&link.from).expect("from"),
            link.from_port,
        )
        .expect("out");
        let b = in_pos(&layouts, net.index_of(&link.to).expect("to"), link.to_port).expect("in");
        assert_eq!(wire_at(&sim, &layouts, (a + b) * 0.5), Some(2));
    }

    #[test]
    fn gates_cycle_through_all_six() {
        let mut g = Gate::And;
        let mut seen = Vec::new();
        for _ in 0..6 {
            seen.push(g);
            g = next_gate(g);
        }
        assert_eq!(g, Gate::And);
        seen.sort_by_key(|g| format!("{g:?}"));
        seen.dedup();
        assert_eq!(seen.len(), 6);
    }
}

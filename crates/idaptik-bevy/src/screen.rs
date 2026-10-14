//! Screens: using a console, and drawing what it shows.
//!
//! A screen is equipment in the world, and each seat has its own. The
//! infiltrator uses a console by standing at it and pressing `E` (the sim
//! seats them and holds them still): their camera dives into the console's
//! CRT and the view becomes the screen. The hacker's screen is the deck in the
//! van, opened with `H` (or `E` while playing only the hacker): it comes up as
//! a layer over the van, the street still showing round it (see
//! [`crate::seats`]). Either way the screen is a phosphor schematic of the
//! floor built from [`ScreenView`] — what the console can show, filtered by
//! what its user's session can reach. Rooms are boxes, devices are labelled
//! with what the system reports, and people are never drawn, only the
//! building's fixes on them: an exact mark while a live camera has them,
//! otherwise a dashed circle that grows with the fix's age.
//!
//! On the deck, a click selects a device; its card (see [`crate::deck`]) acts
//! on it through the same `Command::NetHack` the network map uses. `C` shows
//! the wiring on the screen. `E` or `Esc` stands the infiltrator up; `H` closes
//! the deck.

use bevy::prelude::*;
use idaptik_core::scenario::command::{Command, NetNodeIndex};
use idaptik_core::scenario::console::{Estimate, ScreenDevice, ScreenKind, ScreenView};
use idaptik_core::scenario::definition::{ConsoleDef, ConsolePlace};
use idaptik_core::scenario::signal::sim::Seat;

use crate::driver::{CommandQueue, SimState};
use crate::palette;
use crate::scene::{CEILING_Y, SceneMap};
use crate::seats::{DeckCam, Seats};
use crate::wiring::Wiring;

/// Where the infiltrator's console schematic is drawn: far from the
/// building, so the two never overlap, and the camera can cut between them.
pub const SCREEN_AT: Vec2 = Vec2::new(0.0, -6000.0);
/// Where the hacker's deck schematic is drawn, for the deck camera.
pub const DECK_AT: Vec2 = Vec2::new(0.0, -9000.0);
/// The deck's phosphor-black glass.
pub const DECK_GLASS: Color = Color::srgb(0.027, 0.067, 0.051);

/// Where a seat's schematic is drawn.
pub fn screen_at(seat: Seat) -> Vec2 {
    match seat {
        Seat::Infiltrator => SCREEN_AT,
        Seat::Hacker => DECK_AT,
    }
}
/// Canvas → screen scale.
const SCREEN_SCALE: f32 = 0.62;
/// How far into the CRT the camera dives before cutting to the screen.
pub const DIVE_SCALE: f32 = 0.06;

/// Anything that places canvas coordinates (x right, y down) in the world.
pub trait Place {
    fn place(&self, x: f64, y: f64) -> Vec2;
}

impl Place for SceneMap {
    fn place(&self, x: f64, y: f64) -> Vec2 {
        self.point(x, y)
    }
}

/// The schematic's mapping: the floor, shrunk, drawn around the seat's
/// [`screen_at`].
#[derive(Debug, Clone, Copy)]
pub struct ScreenMap {
    width: f64,
    floor: f64,
    at: Vec2,
}

impl ScreenMap {
    pub fn new(width: f64, floor: f64, seat: Seat) -> Self {
        Self {
            width,
            floor,
            at: screen_at(seat),
        }
    }
}

impl Place for ScreenMap {
    fn place(&self, x: f64, y: f64) -> Vec2 {
        self.at
            + Vec2::new(
                (x - self.width * 0.5) as f32 * SCREEN_SCALE,
                (self.floor - y) as f32 * SCREEN_SCALE - 110.0,
            )
    }
}

/// Where a screen is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Closed,
    /// The camera is diving into the CRT.
    DivingIn,
    /// The screen fills the view.
    On,
    /// The camera is pulling back out of the CRT.
    PullingOut,
}

/// One seat's screen: which console, and how far the camera has got.
#[derive(Debug, Default, Clone)]
pub struct SeatScreen {
    pub console: Option<String>,
    pub phase: Phase,
}

impl SeatScreen {
    /// Whether the schematic is what is on view.
    pub fn on(&self) -> bool {
        self.phase == Phase::On
    }
}

/// Both seats' screens.
#[derive(Resource, Default)]
pub struct Screen {
    pub infiltrator: SeatScreen,
    pub hacker: SeatScreen,
}

impl Screen {
    pub fn of(&self, seat: Seat) -> &SeatScreen {
        match seat {
            Seat::Infiltrator => &self.infiltrator,
            Seat::Hacker => &self.hacker,
        }
    }

    /// Whether a seat's schematic is on view.
    pub fn on(&self, seat: Seat) -> bool {
        self.of(seat).on()
    }
}

/// The device selected on the deck, by grounded-network node id.
#[derive(Resource, Default, Debug, Clone, PartialEq, Eq)]
pub struct DeckSelection(pub Option<String>);

/// Where a console's CRT is in the scene (canvas), for the camera to dive at.
pub fn crt_canvas(con: &ConsoleDef, world_width: f64, floor: f64) -> (f64, f64) {
    match &con.place {
        ConsolePlace::Van => {
            let (x, y) = crate::art::van_anchor(world_width, floor);
            (x + 14.0, y - 3.0)
        }
        ConsolePlace::Room { x, .. } => (*x, floor - 52.0),
    }
}

/// Keys and sim state → which screens are in use.
///
/// The infiltrator's screen follows the sim (it seats them). The hacker's
/// deck is the hacker's own kit: `H` opens and closes it, as do `E` and `Esc`
/// while the keyboard is the hacker's alone. Opening it from the van, still
/// cold, logs the deck into the building's bridge first: sitting down at the
/// deck is how the hacker starts work.
pub fn screen_keys(
    keys: Res<ButtonInput<KeyCode>>,
    sim: Res<SimState>,
    seats: Option<Res<Seats>>,
    mut screen: ResMut<Screen>,
    mut queue: ResMut<CommandQueue>,
) {
    let def = sim.sim.definition();
    let using = sim.sim.state().using_console.clone();
    let keys_infiltrator = crate::seats::keys_drive_infiltrator(seats.as_deref());

    let inf = &mut screen.infiltrator;
    // Esc stands up too: it sends the same interact the sim expects.
    if keys_infiltrator && using.is_some() && inf.on() && keys.just_pressed(KeyCode::Escape) {
        queue.push(Command::Interact);
    }
    match (&using, inf.console.is_some()) {
        (Some(id), false) => {
            inf.console = Some(id.clone());
            inf.phase = Phase::DivingIn;
        }
        (None, true) if inf.phase != Phase::PullingOut => inf.phase = Phase::PullingOut,
        _ => {}
    }

    let toggle = keys.just_pressed(KeyCode::KeyH)
        || (!keys_infiltrator
            && (keys.just_pressed(KeyCode::KeyE)
                || (screen.hacker.on() && keys.just_pressed(KeyCode::Escape))));
    if toggle {
        if screen.hacker.on() {
            screen.hacker = SeatScreen::default();
        } else if let Some(deck) = def.consoles.iter().find(|c| c.place == ConsolePlace::Van) {
            // The deck is a layer over the van, not a dive: it is simply on.
            screen.hacker = SeatScreen {
                console: Some(deck.id.clone()),
                phase: Phase::On,
            };
            if sim.sim.state().agents.hacker.hops() == 0 {
                queue.push(Command::Pivot {
                    target: idaptik_core::scenario::command::PivotTarget::Bridge,
                });
            }
        }
    }
}

/// What a seat's screen shows, if it is on.
pub fn current_view(sim: &SimState, screen: &Screen, seat: Seat) -> Option<ScreenView> {
    let s = screen.of(seat);
    if !s.on() {
        return None;
    }
    sim.sim.screen_view(s.console.as_deref()?, seat)
}

/// The deck's glass: an opaque backdrop behind the deck schematic, so the
/// layer hides the van beneath it.
pub fn setup_deck_glass(mut commands: Commands) {
    commands.spawn((
        Sprite::from_color(DECK_GLASS, Vec2::new(8000.0, 6000.0)),
        Transform::from_translation(DECK_AT.extend(-20.0)),
    ));
}

/// Marker: one text on the schematic.
#[derive(Component)]
pub struct ScreenText;

/// The schematics' texts, rebuilt each frame a screen is on (a few dozen
/// short labels: cheaper than tracking which changed).
pub fn screen_text(
    mut commands: Commands,
    sim: Res<SimState>,
    screen: Res<Screen>,
    existing: Query<Entity, With<ScreenText>>,
) {
    for e in &existing {
        commands.entity(e).despawn();
    }
    for seat in [Seat::Infiltrator, Seat::Hacker] {
        if let Some(view) = current_view(&sim, &screen, seat) {
            spawn_view_text(&mut commands, &sim, &view);
        }
    }
}

fn spawn_view_text(commands: &mut Commands, sim: &SimState, view: &ScreenView) {
    let def = sim.sim.definition();
    let m = ScreenMap::new(def.world.width, def.world.floor, view.seat);
    let floor = def.world.floor;
    let text = |s: String, size: f32, color: Color, at: Vec2| {
        (
            ScreenText,
            Text2d::new(s),
            TextFont::from_font_size(size),
            TextColor(color),
            Transform::from_translation(at.extend(8.0)),
        )
    };
    let dim = palette::PHOSPHOR.with_alpha(0.55);
    commands.spawn(text(
        format!(
            "{}   trace {:.0}%   hops {}   bandwidth {:.0}",
            view.label.to_uppercase(),
            view.trace * 100.0,
            view.hops,
            view.bandwidth
        ),
        20.0,
        palette::PHOSPHOR,
        m.place(def.world.width * 0.5, CEILING_Y - 150.0),
    ));
    let help = match (view.seat, view.devices.iter().any(|d| d.actionable)) {
        (Seat::Hacker, true) => "click a device for its card   C wiring   H close the deck",
        (Seat::Hacker, false) => {
            "nothing on the floor answers from here: pivot in (P)   H close the deck"
        }
        (Seat::Infiltrator, _) => "look only from this PC   C wiring   E / Esc stand up",
    };
    commands.spawn(text(
        help.to_owned(),
        15.0,
        dim,
        m.place(def.world.width * 0.5, floor + 70.0),
    ));
    for room in &view.rooms {
        commands.spawn(text(
            room.name.clone(),
            13.0,
            dim,
            m.place(room.x + room.w * 0.5, CEILING_Y + 14.0),
        ));
    }
    for d in &view.devices {
        let (y, color) = device_row(d.kind, floor);
        let c = if d.status == "LOCKED" || d.status == "NO POWER" {
            palette::ALARM
        } else {
            color
        };
        commands.spawn(text(
            format!("{}\n{}", d.node, d.status),
            12.0,
            c,
            m.place(d.x, y) + Vec2::new(0.0, -20.0),
        ));
    }
    let mut person = |e: &Estimate, name: &str, color: Color| {
        let what = if e.live {
            format!("{name} · LIVE")
        } else {
            format!("{name}? {:.0}s ago", e.age)
        };
        commands.spawn(text(what, 14.0, color, m.place(e.x, floor - 150.0)));
    };
    if let Some(e) = &view.infiltrator {
        person(e, "INF", palette::SIGNAL);
    }
    if let Some(e) = &view.billy {
        person(e, "GUARD", palette::ALARM);
    }
}

/// Where a device kind sits on the schematic, and its colour.
pub fn device_row(kind: ScreenKind, floor: f64) -> (f64, Color) {
    match kind {
        ScreenKind::Camera => (CEILING_Y + 30.0, palette::PHOSPHOR),
        ScreenKind::Light => (CEILING_Y + 90.0, palette::PHOSPHOR),
        ScreenKind::Door => (floor - 60.0, palette::PHOSPHOR),
        ScreenKind::Vacuum => (floor - 12.0, palette::PHOSPHOR),
    }
}

/// The schematics' lines: the bezel, rooms, devices, the fixes on people, and
/// on the deck the selected device's ring.
pub fn draw_screen(
    sim: Res<SimState>,
    screen: Res<Screen>,
    selection: Option<Res<DeckSelection>>,
    time: Res<Time>,
    mut gizmos: Gizmos,
) {
    for seat in [Seat::Infiltrator, Seat::Hacker] {
        let Some(view) = current_view(&sim, &screen, seat) else {
            continue;
        };
        let selected = match (seat, selection.as_deref()) {
            (Seat::Hacker, Some(DeckSelection(Some(node)))) => Some(node.as_str()),
            _ => None,
        };
        draw_view(&mut gizmos, &sim, &view, selected, time.elapsed_secs());
    }
}

fn draw_view(
    gizmos: &mut Gizmos,
    sim: &SimState,
    view: &ScreenView,
    selected: Option<&str>,
    t: f32,
) {
    let def = sim.sim.definition();
    let m = ScreenMap::new(def.world.width, def.world.floor, view.seat);
    let floor = def.world.floor;
    let ph = palette::PHOSPHOR;

    // The bezel.
    let a = m.place(-60.0, CEILING_Y - 190.0);
    let b = m.place(def.world.width + 60.0, floor + 100.0);
    gizmos.rect_2d((a + b) * 0.5, (b - a).abs(), ph.with_alpha(0.35));

    for room in &view.rooms {
        let a = m.place(room.x + 2.0, CEILING_Y);
        let b = m.place(room.x + room.w - 2.0, floor);
        gizmos.rect_2d((a + b) * 0.5, (b - a).abs(), ph.with_alpha(0.6));
    }
    for d in &view.devices {
        let (y, color) = device_row(d.kind, floor);
        let at = m.place(d.x, y);
        let c = if d.status == "LOCKED" || d.status == "NO POWER" {
            palette::ALARM
        } else if d.status.starts_with("OFF") || d.status == "SHUT" || d.status == "DOCKED" {
            color.with_alpha(0.5)
        } else {
            color
        };
        gizmos.rect_2d(at, Vec2::splat(10.0), c);
        if d.status.starts_with("OPEN")
            || d.status == "RUNNING"
            || d.status == "LIVE"
            || d.status == "ON"
        {
            gizmos.rect_2d(at, Vec2::splat(5.0), c);
        }
        if selected == Some(d.node.as_str()) {
            // A slow pulse, bone-white: the device the card is about.
            let r = 11.0 + 1.5 * (t * 4.0).sin();
            gizmos.circle_2d(at, r, palette::BONE);
        }
    }
    let mut person = |e: &Estimate, color: Color| {
        let at = m.place(e.x, floor - 30.0);
        if e.live {
            gizmos.circle_2d(at, 6.0, color);
            gizmos.circle_2d(at, 2.0, color);
        } else {
            let r = e.radius as f32 * 0.62;
            let n = 36;
            for k in 0..n {
                if k % 2 == 0 {
                    let a0 = k as f32 / n as f32 * std::f32::consts::TAU;
                    let a1 = (k + 1) as f32 / n as f32 * std::f32::consts::TAU;
                    gizmos.line_2d(
                        at + Vec2::new(a0.cos(), a0.sin()) * r,
                        at + Vec2::new(a1.cos(), a1.sin()) * r,
                        color.with_alpha(0.8),
                    );
                }
            }
            gizmos.circle_2d(at, 2.0, color);
        }
    };
    if let Some(e) = &view.infiltrator {
        person(e, palette::SIGNAL);
    }
    if let Some(e) = &view.billy {
        person(e, palette::ALARM);
    }
}

/// The device nearest `at` (deck world space) within reach of a click.
pub fn device_at<'a>(
    view: &'a ScreenView,
    m: &ScreenMap,
    floor: f64,
    at: Vec2,
) -> Option<&'a ScreenDevice> {
    view.devices
        .iter()
        .map(|d| (d, m.place(d.x, device_row(d.kind, floor).0).distance(at)))
        .filter(|(_, dist)| *dist <= 14.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(d, _)| d)
}

/// On the deck, a click selects a device (its card acts on it); a click on
/// bare schematic clears the selection.
#[allow(clippy::too_many_arguments)]
pub fn deck_click(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<DeckCam>>,
    sim: Res<SimState>,
    screen: Res<Screen>,
    wiring: Res<Wiring>,
    seats: Option<Res<Seats>>,
    mut selection: ResMut<DeckSelection>,
) {
    // With the wiring up, the deck's clicks belong to the wires.
    if (wiring.shown && wiring.seat == Seat::Hacker) || !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(view) = current_view(&sim, &screen, Seat::Hacker) else {
        selection.0 = None;
        return;
    };
    let (Some(cursor), Ok((camera, transform))) = (
        windows.single().ok().and_then(|w| w.cursor_position()),
        cameras.single(),
    ) else {
        return;
    };
    // Only the schematic selects; the controls under it are buttons.
    let schematic = seats.as_ref().and_then(|s| s.panes.deck);
    if !camera.is_active || !schematic.is_some_and(|r| r.contains(cursor)) {
        return;
    }
    let Ok(at) = camera.viewport_to_world_2d(transform, cursor) else {
        return;
    };
    let def = sim.sim.definition();
    let m = ScreenMap::new(def.world.width, def.world.floor, Seat::Hacker);
    selection.0 = device_at(&view, &m, def.world.floor, at).map(|d| d.node.clone());
}

/// Act on a device from the deck.
pub fn hack(queue: &mut CommandQueue, device: &ScreenDevice) {
    queue.push(Command::NetHack {
        node: NetNodeIndex(device.net_index),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_schematic_is_drawn_well_away_from_the_building_and_each_other() {
        let scene = SceneMap::new(1260.0, 585.0, 0);
        let inf = ScreenMap::new(1260.0, 585.0, Seat::Infiltrator);
        let deck = ScreenMap::new(1260.0, 585.0, Seat::Hacker);
        let w = scene.point(630.0, 400.0);
        assert!(inf.place(630.0, 400.0).distance(w) > 3000.0);
        assert!(deck.place(630.0, 400.0).distance(w) > 3000.0);
        // A schematic spans about 1300 units: the two must not overlap.
        assert!(inf.place(630.0, 400.0).distance(deck.place(630.0, 400.0)) > 2000.0);
    }

    #[test]
    fn each_seat_has_its_own_screen() {
        let mut screen = Screen::default();
        screen.hacker.phase = Phase::On;
        assert!(screen.on(Seat::Hacker));
        assert!(!screen.on(Seat::Infiltrator));
    }
}

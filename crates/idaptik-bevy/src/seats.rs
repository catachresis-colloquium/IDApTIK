//! Both seats at once: the window split between the infiltrator's floor and
//! the hacker in the van.
//!
//! Local play shows both seats. The keyboard drives the infiltrator and the
//! mouse drives the hacker, so one person can play both, as two players
//! would. Each seat has its own world camera (see [`SeatCam`]). The hacker's
//! deck is a layer on top of the van camera ([`DeckCam`]), inset so a margin
//! of the street around the van stays in view: the hacker's peripheral vision.
//!
//! The split follows what is on show. Both world views are wide and short,
//! so with nobody at a screen the seats are stacked, floor above van. When
//! either seat is at a screen (the infiltrator at a console, or the hacker's
//! deck open), the seats sit side by side, since a screen wants height. The
//! divider sits in the middle, and leans a little, slowly, only when one seat
//! has been in steady use while the other has been left alone for a while:
//! keys moving the infiltrator toward the floor, clicks on the deck toward the
//! van. Moving the mouse alone does not count.
//!
//! When the two are in the same physical space — the infiltrator outside,
//! near enough the van that one frame holds both, and the hacker not at the
//! deck — the split closes and one camera frames them together; it opens
//! again when they part, go in, or the deck comes up.
//!
//! `B` steps through both seats, the floor alone and the van alone. Net View,
//! and netplay (one seat per player), show one seat.

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, ScalingMode, Viewport};
use bevy::prelude::*;
use bevy::ui::IsDefaultUiCamera;
use idaptik_core::scenario::signal::sim::Seat;

use crate::net_view::AppMode;
use crate::screen::{DECK_AT, Screen};
use crate::wiring::Wiring;

/// A world camera following one seat's body.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeatCam(pub Seat);

/// The camera Net View and single-seat play use: the infiltrator's.
#[derive(Component)]
pub struct MainCam;

/// The hacker's deck: the schematic, drawn as a layer over the van.
#[derive(Component)]
pub struct DeckCam;

/// The full-window camera every UI node draws on. It renders no sprites.
#[derive(Component)]
pub struct UiCam;

/// The render layer only [`UiCam`] looks at (nothing is drawn on it).
pub const UI_LAYER: usize = 7;

/// How the two seats share the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Arrangement {
    /// Floor above, van below.
    #[default]
    Stacked,
    /// Floor left, van right.
    SideBySide,
}

/// Where each view is on the window, in logical pixels (origin top-left).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Panes {
    pub infiltrator: Option<Rect>,
    pub hacker: Option<Rect>,
    /// The deck's schematic, inside the hacker's pane.
    pub deck: Option<Rect>,
    /// The deck's controls, under the schematic.
    pub deck_ui: Option<Rect>,
}

impl Panes {
    pub fn of(&self, seat: Seat) -> Option<Rect> {
        match seat {
            Seat::Infiltrator => self.infiltrator,
            Seat::Hacker => self.hacker,
        }
    }
}

/// Who is on screen. `B` steps through them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    /// Both seats: keys drive the floor, the mouse the van.
    #[default]
    Both,
    /// The infiltrator's floor alone.
    Floor,
    /// The hacker's van alone: the keyboard is the hacker's too.
    Van,
}

impl View {
    pub fn next(self) -> Self {
        match self {
            View::Both => View::Floor,
            View::Floor => View::Van,
            View::Van => View::Both,
        }
    }
}

/// The split's state.
#[derive(Resource, Debug, Default)]
pub struct Seats {
    pub view: View,
    /// The infiltrator's share of the window, eased toward its target.
    pub share: f32,
    pub arrangement: Arrangement,
    /// When the keys (the infiltrator) and the mouse (the hacker) were last
    /// used, in seconds of app time.
    pub keys_at: f32,
    pub mouse_at: f32,
    /// What is on screen this frame (eased toward the layout's target).
    pub panes: Panes,
    /// The arrangement on screen, which lags `arrangement` while the van's
    /// pane slides shut to change it.
    shown: Arrangement,
    /// The infiltrator's share on screen (0 to 1), easing toward its target.
    on_screen: f32,
    /// How far the deck has come up, 0 (off) to 1 (on).
    deck_up: f32,
    /// One frame holds both characters, so the window is not split.
    pub merged: bool,
}

impl Seats {
    pub fn new() -> Self {
        Self {
            share: 0.5,
            on_screen: 0.5,
            keys_at: f32::NEG_INFINITY,
            mouse_at: f32::NEG_INFINITY,
            ..Default::default()
        }
    }
}

/// How long a hand counts as busy after its last input.
const BUSY: f32 = 4.0;
/// How long the other hand must have been left alone before the divider
/// leans toward the busy one.
const IDLE: f32 = 12.0;
/// The share the busy seat gets.
const LEAN: f32 = 0.58;
/// How fast the divider eases toward its target (per second): slow, so a
/// lean is a drift rather than a jump.
const EASE: f32 = 0.6;
/// How fast the panes slide when the arrangement or the view changes, and the
/// deck comes up or goes down (per second): about a quarter-second each way.
const MORPH: f32 = 14.0;
/// The divider between the panes, logical pixels.
pub const DIVIDER: f32 = 4.0;
/// The deck's controls strip under the schematic.
const DECK_UI_H: f32 = 210.0;
/// The schematic's height for its width (the floor plan is long and low).
const DECK_ASPECT: f32 = 0.42;
/// World units across the deck: the schematic's bezel, with a little glass.
const DECK_VIEW_W: f32 = 900.0;
/// The schematic bezel's centre, below [`DECK_AT`] (see `screen::draw_view`).
const BEZEL_DROP: f32 = 36.0;

/// Room left round the two characters in a merged frame (canvas units).
const MERGE_MARGIN: f64 = 140.0;
/// Extra room needed to merge, over what keeps a merge: so the split does not
/// flicker open and shut at the edge.
const MERGE_HYSTERESIS: f64 = 120.0;

/// Whether one frame on the outside holds both the infiltrator and the van:
/// the infiltrator outside, the two no further apart than the frame is wide
/// (less a margin each side). `view_w` is the frame's width in canvas units;
/// `merged` says whether they are merged already (it takes more to merge than
/// to stay merged).
pub fn one_frame_holds_both(
    outside: bool,
    infiltrator_x: f64,
    van_x: f64,
    view_w: f64,
    merged: bool,
) -> bool {
    let slack = if merged { 0.0 } else { MERGE_HYSTERESIS };
    outside && (infiltrator_x - van_x).abs() + 2.0 * MERGE_MARGIN + slack <= view_w
}

/// Stacked unless someone is at a screen.
pub fn arrangement_for(infiltrator_screen: bool, deck: bool) -> Arrangement {
    if infiltrator_screen || deck {
        Arrangement::SideBySide
    } else {
        Arrangement::Stacked
    }
}

/// The infiltrator's share for how long ago each hand was used.
pub fn lean_for(keys_age: f32, mouse_age: f32) -> f32 {
    if keys_age < BUSY && mouse_age > IDLE {
        LEAN
    } else if mouse_age < BUSY && keys_age > IDLE {
        1.0 - LEAN
    } else {
        0.5
    }
}

/// The margin of street left round the deck: the hacker's peripheral vision.
pub fn deck_margin(pane: Rect) -> f32 {
    (pane.width().min(pane.height()) * 0.07).clamp(16.0, 64.0)
}

/// The two seats' panes on a `window` (logical size), the infiltrator taking
/// `share` of it (0 to 1). A pane with no room is not on screen. The two
/// always tile the window, whatever the share, so a slide never opens a gap.
pub fn split(window: Vec2, share: f32, arrangement: Arrangement) -> (Option<Rect>, Option<Rect>) {
    let full = Rect::from_corners(Vec2::ZERO, window);
    if share >= 0.999 {
        return (Some(full), None);
    }
    if share <= 0.001 {
        return (None, Some(full));
    }
    let half = DIVIDER * 0.5;
    let (a, b) = match arrangement {
        Arrangement::Stacked => {
            let cut = (window.y * share).round();
            (
                Rect::new(0.0, 0.0, window.x, cut - half),
                Rect::new(0.0, cut + half, window.x, window.y),
            )
        }
        Arrangement::SideBySide => {
            let cut = (window.x * share).round();
            (
                Rect::new(0.0, 0.0, cut - half, window.y),
                Rect::new(cut + half, 0.0, window.x, window.y),
            )
        }
    };
    let some = |r: Rect| (r.width() >= 1.0 && r.height() >= 1.0).then_some(r);
    (some(a), some(b))
}

/// The infiltrator's share a view asks for.
pub fn share_for(view: View, share: f32) -> f32 {
    match view {
        View::Both => share,
        View::Floor => 1.0,
        View::Van => 0.0,
    }
}

/// The deck in a hacker's pane: a monitor in the middle of the van view, the
/// schematic on top and the controls under it, street all round.
pub fn deck_rects(pane: Rect) -> (Rect, Rect) {
    let m = deck_margin(pane);
    let inner = Rect::new(
        pane.min.x + m,
        pane.min.y + m,
        pane.max.x - m,
        pane.max.y - m,
    );
    let h = (inner.width() * DECK_ASPECT)
        .min(inner.height() - DECK_UI_H)
        .max(0.0);
    let ui = DECK_UI_H.min(inner.height());
    let top = (inner.center().y - (h + ui) * 0.5).max(inner.min.y);
    (
        Rect::new(inner.min.x, top, inner.max.x, top + h),
        Rect::new(inner.min.x, top + h, inner.max.x, top + h + ui),
    )
}

/// The full layout once everything has settled.
pub fn panes(window: Vec2, view: View, share: f32, arrangement: Arrangement, deck: bool) -> Panes {
    let (infiltrator, hacker) = split(window, share_for(view, share), arrangement);
    let (deck, deck_ui) = match hacker.filter(|_| deck) {
        Some(pane) => {
            let (d, u) = deck_rects(pane);
            (Some(d), Some(u))
        }
        None => (None, None),
    };
    Panes {
        infiltrator,
        hacker,
        deck,
        deck_ui,
    }
}

/// Scale a rect about a point.
fn scale_about(r: Rect, at: Vec2, t: f32) -> Rect {
    Rect::from_corners(at + (r.min - at) * t, at + (r.max - at) * t)
}

/// Spawn the cameras: one per seat, the deck layer and the UI camera.
pub fn spawn_cameras(commands: &mut Commands, base_scale: f32) {
    let ortho = |scale: f32| {
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::WindowSize,
            scale,
            ..OrthographicProjection::default_2d()
        })
    };
    // The infiltrator's camera clears the window; every later camera draws
    // over what is already there.
    commands.spawn((
        Camera2d,
        MainCam,
        SeatCam(Seat::Infiltrator),
        Camera {
            order: 0,
            ..Default::default()
        },
        ortho(base_scale),
    ));
    commands.spawn((
        Camera2d,
        SeatCam(Seat::Hacker),
        Camera {
            order: 1,
            is_active: false,
            clear_color: ClearColorConfig::None,
            ..Default::default()
        },
        ortho(base_scale),
    ));
    // The whole deck is one surface: this camera covers the schematic and
    // the controls alike, all on the deck's glass, and the controls' UI is
    // drawn over it without a backdrop of its own.
    commands.spawn((
        Camera2d,
        DeckCam,
        Camera {
            order: 2,
            is_active: false,
            clear_color: ClearColorConfig::None,
            ..Default::default()
        },
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedHorizontal {
                viewport_width: DECK_VIEW_W,
            },
            ..OrthographicProjection::default_2d()
        }),
        Transform::from_translation(DECK_AT.extend(0.0)),
    ));
    commands.spawn((
        Camera2d,
        UiCam,
        IsDefaultUiCamera,
        RenderLayers::layer(UI_LAYER),
        Camera {
            order: 3,
            clear_color: ClearColorConfig::None,
            ..Default::default()
        },
    ));
}

/// Inputs that count as each hand being busy.
const INFILTRATOR_KEYS: [KeyCode; 10] = [
    KeyCode::KeyA,
    KeyCode::KeyD,
    KeyCode::KeyS,
    KeyCode::KeyW,
    KeyCode::KeyE,
    KeyCode::Space,
    KeyCode::ArrowLeft,
    KeyCode::ArrowRight,
    KeyCode::ArrowUp,
    KeyCode::ArrowDown,
];

/// Whether the keyboard moves the infiltrator right now: unless the van is
/// on screen alone. Netplay (no split) always does.
pub fn keys_drive_infiltrator(seats: Option<&Seats>) -> bool {
    seats.is_none_or(|s| s.view != View::Van)
}

fn to_viewport(r: Rect, scale: f32) -> Option<Viewport> {
    let min = (r.min * scale).round();
    let size = (r.size() * scale).round();
    (size.x >= 1.0 && size.y >= 1.0).then(|| Viewport {
        physical_position: UVec2::new(min.x as u32, min.y as u32),
        physical_size: UVec2::new(size.x as u32, size.y as u32),
        ..Default::default()
    })
}

/// The cameras that draw into a pane (every one but the UI camera).
type PaneCameras<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Camera,
        &'static mut Transform,
        Option<&'static SeatCam>,
        Has<DeckCam>,
    ),
    Without<UiCam>,
>;

/// Lay the window out each frame and point the cameras at their panes.
#[allow(clippy::too_many_arguments)]
pub fn layout_seats(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    mode: Res<State<AppMode>>,
    mut seats: Option<ResMut<Seats>>,
    mut wiring: Option<ResMut<Wiring>>,
    mut queue: Option<ResMut<crate::driver::CommandQueue>>,
    screen: Option<Res<Screen>>,
    sim: Option<Res<crate::driver::SimState>>,
    mut cameras: PaneCameras,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let size = Vec2::new(window.width(), window.height());
    let full = Rect::from_corners(Vec2::ZERO, size);
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    let net_view = *mode.get() == AppMode::NetView;
    let infiltrator_screen = screen
        .as_ref()
        .is_some_and(|s| s.infiltrator.phase != crate::screen::Phase::Closed);
    let deck = !net_view && screen.as_ref().is_some_and(|s| s.hacker.on());

    let layout = match seats.as_deref_mut() {
        _ if net_view => Panes {
            infiltrator: Some(full),
            ..Default::default()
        },
        // Netplay and tests: one seat, the infiltrator's.
        None => Panes {
            infiltrator: Some(full),
            ..Default::default()
        },
        Some(seats) => {
            if keys.just_pressed(KeyCode::KeyB) {
                seats.view = seats.view.next();
                // Handing the keyboard to the hacker: let go of everything
                // the infiltrator was holding.
                if seats.view == View::Van
                    && let Some(queue) = queue.as_deref_mut()
                {
                    use idaptik_core::scenario::command::{Button, Command};
                    for button in [
                        Button::Left,
                        Button::Right,
                        Button::Crouch,
                        Button::Sprint,
                        Button::Interact,
                    ] {
                        queue.push(Command::SetButton {
                            button,
                            down: false,
                        });
                    }
                }
            }
            if INFILTRATOR_KEYS.iter().any(|k| keys.pressed(*k)) {
                seats.keys_at = now;
            }
            // The hacker is busy when clicking, not when the mouse drifts.
            if buttons.get_pressed().next().is_some() {
                seats.mouse_at = now;
            }
            seats.arrangement = arrangement_for(infiltrator_screen, deck);
            let target = lean_for(now - seats.keys_at, now - seats.mouse_at);
            seats.share += (target - seats.share) * (1.0 - (-EASE * dt).exp());

            // What is on screen eases toward the layout. Changing arrangement
            // slides the van's pane shut, swaps the divider's axis, and slides
            // the pane back open from its new side; the panes always tile the
            // window. The deck powers up out of its own centre.
            let k = 1.0 - (-MORPH * dt).exp();
            if seats.view != View::Both {
                seats.shown = seats.arrangement;
            }
            // Together outside, with the deck down: one frame for both.
            seats.merged = seats.view == View::Both
                && !deck
                && !infiltrator_screen
                && sim.as_ref().is_some_and(|sim| {
                    let def = sim.sim.definition();
                    let p = &sim.sim.state().player;
                    let (van_x, _) = crate::art::van_anchor(def.world.width, def.world.floor);
                    let view_w = f64::from(
                        size.x * crate::scene::PIXEL_SCALE * crate::scene::Space::Street.zoom(),
                    );
                    one_frame_holds_both(
                        sim.sim.infiltrator_outside(),
                        p.x + def.player.w * 0.5,
                        van_x,
                        view_w,
                        seats.merged,
                    )
                });
            let changing = seats.shown != seats.arrangement;
            let want = if seats.merged || changing {
                // Merged, or swapping the axis: the van pane slides shut.
                1.0
            } else {
                share_for(seats.view, seats.share)
            };
            seats.on_screen += (want - seats.on_screen) * k;
            if (want - seats.on_screen).abs() < 0.004 {
                seats.on_screen = want;
                if changing {
                    seats.shown = seats.arrangement;
                }
            }
            let (inf, hack) = split(size, seats.on_screen, seats.shown);
            // The deck comes up once its pane has slid into place.
            let settled = seats.shown == seats.arrangement;
            let up = if deck && settled && hack.is_some() {
                1.0
            } else {
                0.0
            };
            seats.deck_up += (up - seats.deck_up) * k;
            if (up - seats.deck_up).abs() < 0.01 {
                seats.deck_up = up;
            }
            let (d, u) = match hack.filter(|_| seats.deck_up > 0.02) {
                Some(pane) => {
                    let (d, u) = deck_rects(pane);
                    let at = d.union(u).center();
                    (
                        Some(scale_about(d, at, seats.deck_up)),
                        Some(scale_about(u, at, seats.deck_up)),
                    )
                }
                None => (None, None),
            };
            seats.panes = Panes {
                infiltrator: inf,
                hacker: hack,
                deck: d,
                deck_ui: u,
            };

            // The seat the mouse acts for: the one it is over, when both are
            // on screen.
            if let Some(w) = wiring.as_deref_mut() {
                w.seat = match seats.view {
                    View::Floor => Seat::Infiltrator,
                    View::Van => Seat::Hacker,
                    View::Both => match window.cursor_position() {
                        Some(c) if hack.is_some_and(|r| r.contains(c)) => Seat::Hacker,
                        Some(_) => Seat::Infiltrator,
                        None => w.seat,
                    },
                };
            }
            seats.panes
        }
    };

    let scale = window.scale_factor();
    for (mut camera, mut transform, seat, is_deck) in &mut cameras {
        let rect = match (seat, is_deck) {
            (Some(SeatCam(s)), _) => layout.of(*s),
            // The deck's camera covers the whole deck.
            (None, true) => layout.deck.zip(layout.deck_ui).map(|(d, u)| d.union(u)),
            (None, false) => None,
        };
        if is_deck
            && let (Some(all), Some(d)) = (rect, layout.deck)
            && all.width() > 0.0
        {
            // Put the schematic's bezel in the deck's upper part: the
            // camera's centre sits that far below the bezel's.
            let per_px = DECK_VIEW_W / all.width();
            let above = all.center().y - d.center().y;
            transform.translation.x = DECK_AT.x;
            transform.translation.y = DECK_AT.y - BEZEL_DROP - above * per_px;
        }
        let viewport = rect.and_then(|r| to_viewport(r, scale));
        camera.is_active = viewport.is_some();
        if viewport.is_some() {
            camera.viewport = viewport;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Vec2 = Vec2::new(1600.0, 900.0);

    #[test]
    fn a_screen_puts_the_seats_side_by_side() {
        assert_eq!(arrangement_for(false, false), Arrangement::Stacked);
        assert_eq!(arrangement_for(true, false), Arrangement::SideBySide);
        assert_eq!(arrangement_for(false, true), Arrangement::SideBySide);
    }

    #[test]
    fn the_divider_leans_only_when_one_hand_has_been_left_alone() {
        assert!(
            lean_for(0.5, 30.0) > 0.5,
            "keys busy, mouse idle: the floor grows"
        );
        assert!(
            lean_for(30.0, 0.5) < 0.5,
            "clicks, keys idle: the van grows"
        );
        // Both seats in use, even a few seconds apart: no lean.
        assert_eq!(lean_for(0.5, 6.0), 0.5);
        assert_eq!(lean_for(6.0, 0.5), 0.5);
        assert_eq!(lean_for(0.5, 0.5), 0.5);
        assert_eq!(lean_for(30.0, 30.0), 0.5);
        // A lean is modest.
        assert!(lean_for(0.5, 30.0) < 0.6);
    }

    #[test]
    fn both_seats_tile_the_window_with_a_divider() {
        for arr in [Arrangement::Stacked, Arrangement::SideBySide] {
            let p = panes(WINDOW, View::Both, 0.6, arr, false);
            let (a, b) = (p.infiltrator.unwrap(), p.hacker.unwrap());
            let area = a.width() * a.height() + b.width() * b.height();
            let gap = match arr {
                Arrangement::Stacked => WINDOW.x * DIVIDER,
                Arrangement::SideBySide => WINDOW.y * DIVIDER,
            };
            assert!((area + gap - WINDOW.x * WINDOW.y).abs() < 1.0, "{arr:?}");
            assert!(a.intersect(b).is_empty());
        }
    }

    #[test]
    fn b_steps_through_both_floor_and_van() {
        assert_eq!(View::Both.next(), View::Floor);
        assert_eq!(View::Floor.next(), View::Van);
        assert_eq!(View::Van.next(), View::Both);
        assert!(keys_drive_infiltrator(None));
        let mut s = Seats::new();
        assert!(keys_drive_infiltrator(Some(&s)));
        s.view = View::Van;
        assert!(!keys_drive_infiltrator(Some(&s)));
    }

    #[test]
    fn every_share_tiles_the_window() {
        for arr in [Arrangement::Stacked, Arrangement::SideBySide] {
            for k in 0..=100 {
                let share = k as f32 / 100.0;
                let (a, b) = split(WINDOW, share, arr);
                let area = |r: Option<Rect>| r.map_or(0.0, |r| r.width() * r.height());
                let gap = match (a, b, arr) {
                    (Some(_), Some(_), Arrangement::Stacked) => WINDOW.x * DIVIDER,
                    (Some(_), Some(_), Arrangement::SideBySide) => WINDOW.y * DIVIDER,
                    _ => 0.0,
                };
                // Up to a pixel-thin pane rounded away at either end.
                let missing = WINDOW.x * WINDOW.y - area(a) - area(b) - gap;
                assert!(
                    missing.abs() <= WINDOW.x.max(WINDOW.y) * 2.0,
                    "{arr:?} {share}: {missing}"
                );
            }
        }
    }

    #[test]
    fn the_seats_merge_only_outside_and_near_enough_and_stay_merged_at_the_edge() {
        let view = 900.0;
        // Inside: never, however near.
        assert!(!one_frame_holds_both(false, 1200.0, 1300.0, view, false));
        // Outside and near: merge.
        assert!(one_frame_holds_both(true, 1200.0, 1300.0, view, false));
        // Far apart: split.
        assert!(!one_frame_holds_both(true, 0.0, 1300.0, view, true));
        // At the edge: a merge holds where a fresh one would not start.
        let edge = view - 2.0 * MERGE_MARGIN - MERGE_HYSTERESIS * 0.5;
        assert!(!one_frame_holds_both(
            true,
            1300.0 - edge,
            1300.0,
            view,
            false
        ));
        assert!(one_frame_holds_both(
            true,
            1300.0 - edge,
            1300.0,
            view,
            true
        ));
    }

    #[test]
    fn one_seat_fills_the_window() {
        let p = panes(WINDOW, View::Van, 0.5, Arrangement::Stacked, false);
        assert_eq!(p.infiltrator, None);
        assert_eq!(p.hacker, Some(Rect::from_corners(Vec2::ZERO, WINDOW)));
    }

    #[test]
    fn the_deck_leaves_a_margin_of_street_round_it() {
        let p = panes(WINDOW, View::Both, 0.5, Arrangement::SideBySide, true);
        let (pane, deck, ui) = (p.hacker.unwrap(), p.deck.unwrap(), p.deck_ui.unwrap());
        let m = deck_margin(pane);
        assert!(m >= 16.0);
        assert!((deck.min.x - pane.min.x - m).abs() < 0.5);
        assert!((pane.max.x - deck.max.x - m).abs() < 0.5);
        // Centred, with at least the margin of street above and below.
        assert!(deck.min.y >= pane.min.y + m - 0.5);
        assert!(ui.max.y <= pane.max.y - m + 0.5);
        assert!(((deck.min.y - pane.min.y) - (pane.max.y - ui.max.y)).abs() < 1.0);
        // The controls sit right under the schematic.
        assert_eq!(deck.max.y, ui.min.y);
        // Without the deck open, no layer.
        let closed = panes(WINDOW, View::Both, 0.5, Arrangement::Stacked, false);
        assert_eq!(closed.deck, None);
    }
}

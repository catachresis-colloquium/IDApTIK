//! Bevy frontend for IDApTIK (evaluation — see ADR-0003).
//!
//! Deliberately thin: it owns rendering/input only and treats [`idaptik_core`]
//! as the source of gameplay truth. The frontend speaks the exact same
//! [`idaptik_core::scenario::Command`] / [`idaptik_core::scenario::Event`] wire
//! API the TUI uses: keyboard input becomes `Command`s, a fixed 60 Hz step
//! folds them into a `TickInput` and calls `GhostLobbySim::tick`, and every
//! visual is a pure view over the returned state and events. No game logic
//! lives here.
//!
//! * [`driver`] — the sim resource and the fixed-rate tick. Render-free, so a
//!   headless test `App` can drive it without a window (see `tests/parity.rs`).
//! * [`keymap`] — Bevy keyboard events → `Command`s, mirroring the canonical
//!   bindings in `idaptik-tui/src/keymap.rs`.
//! * [`scene`] — the side-on 2.5D cross-section: rooms as a row of flat quads
//!   (laid out data-driven from the room definitions so a second floor row
//!   slots in above), door slabs, camera view cones, the player and Billy.
//! * [`hud`] — status line, meter bars, event log tail and the result overlay.
//! * [`seats`] — both seats at once: the window split between the
//!   infiltrator's floor and the hacker's van, one camera each, the hacker's
//!   deck as a layer over the van.
//! * [`screen`] / [`deck`] — the consoles' schematics, and the deck's card
//!   for acting on a selected device.

pub mod art;
pub mod backdrop;
pub mod cones;
pub mod deck;
pub mod driver;
pub mod fonts;
#[cfg(feature = "weevil")]
pub mod guard_weevil;
pub mod hud;
pub mod keymap;
pub mod net_view;
pub mod netplay;
pub mod palette;
pub mod pixel;
pub mod scene;
pub mod screen;
pub mod seats;
pub mod sprites;
pub mod wiring;

use bevy::prelude::*;

/// The camera, scene, sprites and HUD shared by local and networked play.
///
/// This plugin owns no simulation truth and decodes no input. Local play adds
/// [`FrontendPlugin`] for the command queue; netplay places it above a
/// one-way render mirror of the lockstep simulation.
pub struct FrontendRenderPlugin;

impl Plugin for FrontendRenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<net_view::AppMode>()
            .add_systems(
                Startup,
                (
                    scene::setup_scene,
                    fonts::install_ui_font,
                    backdrop::setup_backdrop,
                    art::setup_art
                        .after(scene::setup_scene)
                        .after(backdrop::setup_backdrop),
                    cones::setup_cones,
                    hud::setup_hud,
                ),
            )
            .add_systems(
                Update,
                (
                    scene::sync_doors,
                    scene::sync_player,
                    scene::follow_camera.after(scene::sync_player),
                    scene::sync_billy,
                    scene::sync_props,
                    cones::draw_cones,
                    art::animate_art,
                    art::sync_room_light,
                    art::animate_hacker,
                    fonts::crisp_text,
                    hud::update_meters,
                    hud::update_status_text,
                    hud::update_log_text,
                    hud::update_result_text,
                )
                    .run_if(in_state(net_view::AppMode::GhostLobby)),
            );

        // The guard-brain developer view is part of the weevil integration.
        #[cfg(feature = "weevil")]
        app.add_systems(
            Update,
            hud::update_guard_brain_text.run_if(in_state(net_view::AppMode::GhostLobby)),
        );

        // The split runs in every mode: Net View takes the whole window.
        app.add_systems(
            Update,
            (
                seats::layout_seats.before(scene::follow_camera),
                deck::place_in_panes.after(seats::layout_seats),
            ),
        );
    }
}

/// Everything local windowed play adds on top of
/// [`driver::SimDriverPlugin`]: rendering plus command-queue input decoding.
pub struct FrontendPlugin;

impl Plugin for FrontendPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FrontendRenderPlugin)
            .add_observer(deck::card_press)
            .init_resource::<net_view::NetViewDrag>()
            .init_resource::<wiring::Wiring>()
            .init_resource::<screen::Screen>()
            .init_resource::<screen::DeckSelection>()
            .insert_resource(seats::Seats::new())
            .add_systems(
                Startup,
                (
                    wiring::setup_wiring,
                    screen::setup_deck_glass,
                    deck::setup_deck,
                ),
            )
            .add_systems(
                Update,
                (
                    screen::screen_keys,
                    wiring::wiring_keys,
                    wiring::wiring_mouse,
                    screen::deck_click,
                    wiring::sync_wiring,
                    wiring::draw_wiring,
                    screen::screen_text,
                    screen::draw_screen,
                )
                    .chain()
                    .before(seats::layout_seats)
                    .before(scene::follow_camera)
                    .run_if(in_state(net_view::AppMode::GhostLobby)),
            )
            // The deck's panel and bezel are UI; its schematic is a camera.
            // Both must use this frame's layout, or the two halves part
            // while the divider moves.
            .add_systems(
                Update,
                deck::update_deck
                    .after(seats::layout_seats)
                    .run_if(in_state(net_view::AppMode::GhostLobby)),
            )
            .add_systems(
                // Decode the keyboard right before the fixed main loop so a
                // press lands on this frame's tick, not the next frame's.
                RunFixedMainLoop,
                keymap::keyboard_input.in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),
            )
            .add_systems(
                Update,
                (
                    // `pan_net_view` must run before `net_view_click`: the click
                    // reads this frame's dragging latch before handling pivot.
                    (net_view::pan_net_view, net_view::net_view_click).chain(),
                    net_view::draw_segment_edges,
                    net_view::update_net_hud,
                )
                    .run_if(in_state(net_view::AppMode::NetView)),
            )
            .add_systems(
                OnEnter(net_view::AppMode::NetView),
                (
                    scene::centre_camera,
                    net_view::hide_ghost_lobby_ui,
                    net_view::setup_net_view,
                    net_view::setup_net_hud,
                ),
            )
            .add_systems(
                OnExit(net_view::AppMode::NetView),
                (net_view::show_ghost_lobby_ui, net_view::teardown_net_view),
            );
    }
}

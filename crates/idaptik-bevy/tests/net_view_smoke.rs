//! Headless: Net View must spawn exactly one entity per graph node, and none
//! once torn down -- the same style `tests/parity.rs` already uses to drive
//! `SimDriverPlugin` without a window.

use bevy::prelude::*;
use idaptik_bevy::driver::{SimDriverPlugin, SimState};
use idaptik_bevy::net_view::{AppMode, NetNodeMarker, setup_net_view, teardown_net_view};
use idaptik_core::RunConfig;

/// A window-free `App` that carries just enough to exercise `setup_net_view`.
///
/// `setup_net_view` needs `Assets<Mesh>` and `Assets<ColorMaterial>` (its
/// device icons add meshes and materials). This crate already establishes how
/// to supply those headlessly: `sprites.rs`'s `device_icon_tests` add
/// `MinimalPlugins` and `init_resource` the two `Assets` collections directly,
/// rather than pulling in `AssetPlugin`. `Assets::add` mints its own handles,
/// so no asset-server machinery is needed; we mirror that convention here.
fn headless_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        // `init_state` requires the `StateTransition` schedule, which
        // `StatesPlugin` installs. `DefaultPlugins` would carry it, but a
        // headless run wants none of the render/window stack `DefaultPlugins`
        // drags in, so we add just this one plugin explicitly.
        .add_plugins(bevy::state::app::StatesPlugin)
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<ColorMaterial>>()
        .init_resource::<Assets<Image>>()
        .add_plugins(SimDriverPlugin {
            cfg: RunConfig::standard(),
            seed: 123456,
            wired: true,
        })
        .init_state::<AppMode>()
        .add_systems(OnEnter(AppMode::NetView), setup_net_view)
        .add_systems(OnExit(AppMode::NetView), teardown_net_view);
    app
}

#[test]
fn net_view_spawns_exactly_one_entity_per_graph_node() {
    let mut app = headless_app();
    app.update();

    // Every node in the Ghost Lobby floor graph sits in a segment that the
    // graph itself declares, so `setup_net_view` (which iterates segments and
    // spawns each node found within one) reaches all of them: the expected
    // count is simply `graph().nodes.len()`. Were any node an orphan -- its
    // `segment` matching no `segment.id` -- it would be silently dropped and
    // this equality would not hold; the fixture has none (verified: 20 nodes,
    // 11 segments, zero orphans).
    let expected = app.world().resource::<SimState>().sim.graph().nodes.len();

    app.world_mut()
        .resource_mut::<NextState<AppMode>>()
        .set(AppMode::NetView);
    app.update();
    app.update();

    let spawned = app
        .world_mut()
        .query::<&NetNodeMarker>()
        .iter(app.world())
        .count();
    assert_eq!(spawned, expected);
}

#[test]
fn net_view_teardown_leaves_no_node_markers_behind() {
    let mut app = headless_app();
    app.update();

    app.world_mut()
        .resource_mut::<NextState<AppMode>>()
        .set(AppMode::NetView);
    app.update();
    app.update();

    app.world_mut()
        .resource_mut::<NextState<AppMode>>()
        .set(AppMode::GhostLobby);
    app.update();
    app.update();

    let remaining = app
        .world_mut()
        .query::<&NetNodeMarker>()
        .iter(app.world())
        .count();
    assert_eq!(remaining, 0);
}

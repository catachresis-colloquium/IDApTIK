//! Regression: graphical netplay must install the actual scene, not merely a
//! network-status overlay in an otherwise empty Bevy window.

use bevy::ecs::schedule::Schedules;
use bevy::prelude::*;
use idaptik_bevy::FrontendRenderPlugin;
use idaptik_bevy::driver::SimDriverPlugin;
use idaptik_bevy::hud::StatusText;
use idaptik_bevy::scene::PlayerMarker;
use idaptik_core::RunConfig;

#[test]
fn shared_renderer_spawns_camera_player_and_hud() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::state::app::StatesPlugin)
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<ColorMaterial>>()
        .init_resource::<Assets<Image>>()
        .add_plugins(SimDriverPlugin {
            cfg: RunConfig::standard(),
            seed: 123456,
            wired: true,
        })
        .add_plugins(FrontendRenderPlugin);

    app.world_mut().run_schedule(Startup);

    // Initialize the complete renderer schedule without executing it. System
    // initialization is where Bevy rejects overlapping mutable queries; a
    // headless app intentionally lacks several window/render resources needed
    // to execute every presentation system.
    let mut update = app
        .world_mut()
        .resource_mut::<Schedules>()
        .remove(Update)
        .expect("Update schedule must exist");
    update
        .initialize(app.world_mut())
        .expect("renderer schedule must initialize without conflicting queries");
    app.world_mut().resource_mut::<Schedules>().insert(update);

    let world = app.world_mut();
    let cameras = world.query::<&Camera2d>().iter(world).count();
    let players = world.query::<&PlayerMarker>().iter(world).count();
    let status_lines = world.query::<&StatusText>().iter(world).count();

    // One per seat, the hacker's deck layer and the UI camera.
    assert_eq!(cameras, 4);
    assert_eq!(players, 1);
    assert_eq!(status_lines, 1);
}

//! Exercise activation observers in a headless ECS world, without a window.
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::ui_widgets::{Activate, Button as UiButton};
use idaptik_bevy::deck::{CardVerb, card_press};
use idaptik_bevy::driver::{CommandQueue, SimState};
use idaptik_bevy::net_view::AppMode;
use idaptik_bevy::netplay::ui::{
    NetplayAppState, RoleButton, RoleSelectionPlugin, RoleSelectionState, RoleSelectionUi,
};
use idaptik_bevy::screen::{DeckSelection, Phase, Screen, SeatScreen, current_view};
use idaptik_core::RunConfig;
use idaptik_core::scenario::command::{Buttons, Command, NetNodeIndex, PivotTarget, fold};
use idaptik_core::scenario::signal::ghost_lobby_wiring::ghost_lobby_wired;
use idaptik_core::scenario::signal::sim::Seat;
use idaptik_net::envelope::Role;

fn role_app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin, RoleSelectionPlugin));
    app.update();
    app.world_mut()
        .resource_mut::<NextState<NetplayAppState>>()
        .set(NetplayAppState::RoleSelection);
    app.update();
    assert!(app.world().resource::<RoleSelectionState>().active);
    app
}

#[test]
fn either_role_button_selects_its_role_highlights_and_returns_to_normal() {
    for wanted in [Role::Infiltrator, Role::Hacker] {
        let mut app = role_app();
        let buttons: Vec<_> = app
            .world_mut()
            .query::<(Entity, &RoleButton, &BackgroundColor, &UiButton)>()
            .iter(app.world())
            .map(|(entity, button, background, _)| (entity, button.role, *background))
            .collect();
        assert_eq!(
            buttons.len(),
            2,
            "both production buttons must use the activation widget"
        );
        let &(target, _, _) = buttons.iter().find(|(_, role, _)| *role == wanted).unwrap();
        app.world_mut().trigger(Activate { entity: target });
        let selection = app.world().resource::<RoleSelectionState>();
        assert_eq!(selection.selected_role, Some(wanted));
        assert!(!selection.active);
        assert_eq!(
            app.world().get::<BackgroundColor>(target).unwrap().0,
            Color::srgb(0.4, 0.6, 0.8)
        );
        for (entity, role, previous) in &buttons {
            if *role != wanted {
                assert_eq!(app.world().get::<BackgroundColor>(*entity), Some(previous));
            }
        }
        app.update();
        assert_eq!(
            *app.world().resource::<State<NetplayAppState>>().get(),
            NetplayAppState::Normal
        );
        let world = app.world_mut();
        let roots: Vec<_> = world
            .query_filtered::<&Visibility, With<RoleSelectionUi>>()
            .iter(world)
            .collect();
        assert_eq!(roots.len(), 1);
        assert_eq!(*roots[0], Visibility::Hidden);
    }
}

#[test]
fn role_activation_does_not_require_a_background_component() {
    let mut app = role_app();
    let target = app
        .world_mut()
        .spawn(RoleButton { role: Role::Hacker })
        .id();
    app.world_mut().trigger(Activate { entity: target });
    assert_eq!(
        app.world().resource::<RoleSelectionState>().selected_role,
        Some(Role::Hacker)
    );
    app.update();
    assert_eq!(
        *app.world().resource::<State<NetplayAppState>>().get(),
        NetplayAppState::Normal
    );
}

#[test]
fn unrelated_activation_preserves_role_selection_and_its_pending_state() {
    let mut app = role_app();
    app.world_mut()
        .resource_mut::<RoleSelectionState>()
        .selected_role = Some(Role::Infiltrator);
    let background = BackgroundColor(Color::srgb(0.1, 0.2, 0.3));
    let target = app
        .world_mut()
        .spawn((UiButton::default(), background))
        .id();
    app.world_mut().trigger(Activate { entity: target });
    let selection = app.world().resource::<RoleSelectionState>();
    assert!(selection.active);
    assert_eq!(selection.selected_role, Some(Role::Infiltrator));
    assert_eq!(
        app.world().get::<BackgroundColor>(target),
        Some(&background)
    );
    app.update();
    assert_eq!(
        *app.world().resource::<State<NetplayAppState>>().get(),
        NetplayAppState::RoleSelection
    );
}

fn card_app() -> (App, Entity, String, u32) {
    let mut sim = SimState::with_definition(ghost_lobby_wired(), RunConfig::standard(), 123456)
        .expect("canonical wired floor");
    let input = fold(
        &[Command::Pivot {
            target: PivotTarget::Bridge,
        }],
        &mut Buttons::default(),
    );
    sim.sim.tick(&input);
    let screen = Screen {
        hacker: SeatScreen {
            console: Some("van-deck".into()),
            phase: Phase::On,
        },
        ..default()
    };
    let view = current_view(&sim, &screen, Seat::Hacker).expect("open hacker deck");
    let device = view
        .devices
        .iter()
        .find(|d| d.actionable)
        .expect("reachable actionable device");
    let (node, index) = (device.node.clone(), device.net_index);
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin))
        .init_state::<AppMode>()
        .insert_resource(sim)
        .insert_resource(screen)
        .insert_resource(DeckSelection(Some(node.clone())))
        .init_resource::<CommandQueue>()
        .add_observer(card_press);
    let button = app.world_mut().spawn((CardVerb, UiButton::default())).id();
    (app, button, node, index)
}

#[test]
fn card_activation_appends_one_hack_per_event_without_mutating_the_simulation() {
    let (mut app, target, _, index) = card_app();
    let before = app.world().resource::<SimState>().sim.snapshot();
    app.world_mut()
        .resource_mut::<CommandQueue>()
        .push(Command::Jump);
    for _ in 0..2 {
        app.world_mut().trigger(Activate { entity: target });
    }
    assert_eq!(
        app.world().resource::<CommandQueue>().pending,
        vec![
            Command::Jump,
            Command::NetHack {
                node: NetNodeIndex(index)
            },
            Command::NetHack {
                node: NetNodeIndex(index)
            },
        ]
    );
    assert_eq!(app.world().resource::<SimState>().sim.snapshot(), before);
}

#[test]
fn card_activation_ignores_other_targets_and_net_view_mode() {
    let (mut app, target, _, _) = card_app();
    let unrelated = app.world_mut().spawn(UiButton::default()).id();
    app.world_mut().trigger(Activate { entity: unrelated });
    assert!(app.world().resource::<CommandQueue>().pending.is_empty());
    app.world_mut()
        .insert_resource(State::new(AppMode::NetView));
    app.world_mut().trigger(Activate { entity: target });
    assert!(app.world().resource::<CommandQueue>().pending.is_empty());
}

#[test]
fn card_activation_requires_a_current_hacker_view_and_a_selected_actionable_device() {
    for phase in [Phase::Closed, Phase::DivingIn, Phase::PullingOut] {
        let (mut app, target, _, _) = card_app();
        app.world_mut().resource_mut::<Screen>().hacker.phase = phase;
        app.world_mut().trigger(Activate { entity: target });
        assert!(
            app.world().resource::<CommandQueue>().pending.is_empty(),
            "{phase:?}"
        );
    }
    for console in [None, Some("missing-console"), Some("office-pc")] {
        let (mut app, target, _, _) = card_app();
        app.world_mut().resource_mut::<Screen>().hacker.console = console.map(str::to_owned);
        if console == Some("office-pc") {
            let view = current_view(
                app.world().resource::<SimState>(),
                app.world().resource::<Screen>(),
                Seat::Hacker,
            )
            .unwrap();
            let device = view
                .devices
                .first()
                .expect("read-only console must expose a device");
            assert!(!device.actionable);
            app.world_mut().resource_mut::<DeckSelection>().0 = Some(device.node.clone());
        }
        app.world_mut().trigger(Activate { entity: target });
        assert!(
            app.world().resource::<CommandQueue>().pending.is_empty(),
            "{console:?}"
        );
    }
    for selection in [None, Some("not-in-this-view".into())] {
        let (mut app, target, _, _) = card_app();
        app.world_mut().resource_mut::<DeckSelection>().0 = selection;
        app.world_mut().trigger(Activate { entity: target });
        assert!(app.world().resource::<CommandQueue>().pending.is_empty());
    }
}

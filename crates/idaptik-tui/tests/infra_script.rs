//! Unit tests for ADR-0013's headless scenario and companion vocabulary.

use idaptik_core::companion::MoleEvent;
use idaptik_core::scenario::command::{Button, Command, MoleOp};
use idaptik_core::scenario::{
    Event, ghost_lobby, ghost_lobby_rooftop, pump_house,
    signal::ghost_lobby_wiring::ghost_lobby_wired,
};
use idaptik_tui::headless::{scenario_for, simulate};
use idaptik_tui::script::{ScriptFile, expand, press_command};
use serde_json::json;

fn script() -> ScriptFile {
    serde_json::from_value(json!({"seed": 7, "max_ticks": 4})).unwrap()
}

#[test]
fn mole_tokens_preserve_order_parameters_and_normalise_case() {
    for (token, op) in [
        (" mole:DIG ", MoleOp::Dig),
        ("MOLE:SABOTAGE", MoleOp::Sabotage),
        ("mole:synth: 0 ", MoleOp::Synth { index: 0 }),
        ("mole:synth:255", MoleOp::Synth { index: 255 }),
        (
            "mole:move: -12.5 ",
            MoleOp::MoveTo {
                x: -12.5,
                underground: false,
            },
        ),
        (
            " MOLE:MOVE: 520.25 :UNDER ",
            MoleOp::MoveTo {
                x: 520.25,
                underground: true,
            },
        ),
    ] {
        assert_eq!(press_command(token), Some(Command::Mole { op }), "{token}");
    }
}

#[test]
fn malformed_mole_tokens_do_not_become_commands() {
    for token in [
        "mole",
        "mole:",
        "mole:dgi",
        "mole:dig:extra",
        "mole:sabotage:extra",
        "mole:synth",
        "mole:synth:",
        "mole:synth:-1",
        "mole:synth:256",
        "mole:synth:1.5",
        "mole:synth:1:extra",
        "mole:move",
        "mole:move:",
        "mole:move:west",
        "mole:move:1:surface",
        "mole:move:1:under:extra",
    ] {
        assert_eq!(press_command(token), None, "{token}");
    }
}

#[test]
fn expansion_fires_mole_orders_once_and_preserves_same_tick_order() {
    let script: ScriptFile = serde_json::from_value(json!({
        "seed": 7, "max_ticks": 4, "scenario": "pump-house",
        "commands": [
            {"at": 2, "press": ["mole:sabotage"]},
            {"at": 1, "hold": ["left"], "press": ["mole:move:520:under", "mole:dig"]},
            {"at": 1, "press": ["mole:synth:0"]},
            {"at": 4, "press": ["mole:synth:1"]}
        ]
    }))
    .unwrap();
    let ticks = expand(&script);
    assert!(ticks[0].immediates.is_empty());
    assert_eq!(
        ticks[1].immediates,
        vec![
            Command::Mole {
                op: MoleOp::MoveTo {
                    x: 520.0,
                    underground: true
                }
            },
            Command::Mole { op: MoleOp::Dig },
            Command::Mole {
                op: MoleOp::Synth { index: 0 }
            },
        ]
    );
    assert_eq!(
        ticks[2].immediates,
        vec![Command::Mole {
            op: MoleOp::Sabotage
        }]
    );
    assert!(ticks[3].immediates.is_empty(), "presses do not persist");
    assert!(ticks.iter().all(|tick| tick.edges.is_empty()));
    assert!(!ticks[0].buttons.has(Button::Left));
    assert!(ticks[1..].iter().all(|tick| tick.buttons.has(Button::Left)));
}

#[test]
fn scenario_selection_preserves_defaults_and_selects_each_named_floor() {
    for (name, expected) in [
        (None, ghost_lobby()),
        (Some("ghost-lobby"), ghost_lobby()),
        (Some("ghost-lobby-wired"), ghost_lobby_wired()),
        (Some("ghost-lobby-rooftop"), ghost_lobby_rooftop()),
        (Some("pump-house"), pump_house()),
    ] {
        let mut script = script();
        script.scenario = name.map(str::to_owned);
        let actual = scenario_for(&script).unwrap();
        assert_eq!(actual, expected, "{name:?}");
        assert!(actual.validate().passed());
    }
}

#[test]
fn unknown_scenarios_return_an_error_instead_of_silently_running_ghost_lobby() {
    for name in ["", "pump-hosue", "Pump-House", " ghost-lobby "] {
        let mut script = script();
        script.scenario = Some(name.into());
        let error = scenario_for(&script).unwrap_err();
        assert!(error.contains("unknown scenario"), "{error}");
        assert!(simulate(&script).is_err(), "{name:?}");
    }
}

#[test]
fn a_headless_pump_house_script_delivers_its_mole_order_to_the_host() {
    let script: ScriptFile = serde_json::from_value(json!({
        "seed": 7, "max_ticks": 1, "scenario": "pump-house",
        "commands": [{"at": 0, "press": ["mole:synth:0"]}]
    }))
    .unwrap();
    let (sim, log) = simulate(&script).unwrap();
    assert_eq!(sim.definition().scenario_id, pump_house().scenario_id);
    assert!(log.iter().any(|e| matches!(
        e,
        Event::Mole {
            mole: MoleEvent::SynthSoundPlayed { .. }
        }
    )));
    assert!(!log.iter().any(|e| matches!(e, Event::MoleDenied { .. })));
    assert!(sim.state().diversion.is_some());
    assert!(sim.state().infra.is_some());
}

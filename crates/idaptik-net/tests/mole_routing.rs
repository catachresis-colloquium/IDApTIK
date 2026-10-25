//! Companion orders use the infiltrator's existing envelope and schedule.

use idaptik_core::scenario::command::{Command, MoleOp};
use idaptik_net::envelope::{
    Role, Seat, decode_command, encode_command, scripted_sender, seat_for,
};
use idaptik_net::seat::{merged_by_tick, seat_schedule, tick_inputs};
use idaptik_tui::script::{ScriptFile, expand};
use serde_json::json;

#[test]
fn all_mole_operations_belong_to_the_infiltrator_and_survive_relay_metadata() {
    for op in [
        MoleOp::Dig,
        MoleOp::Sabotage,
        MoleOp::Synth { index: 255 },
        MoleOp::MoveTo {
            x: 520.25,
            underground: true,
        },
        MoleOp::MoveTo {
            x: 0.0,
            underground: false,
        },
    ] {
        let cmd = Command::Mole { op };
        assert_eq!(seat_for(&cmd), Seat::Infiltrator);
        assert_eq!(scripted_sender(&cmd), Role::Infiltrator);
        let mut wire = encode_command(&cmd, 42, 17).unwrap();
        assert_eq!(wire["seq"], 42);
        assert_eq!(wire["at"], 17);
        assert_eq!(decode_command(&wire).unwrap(), (17, cmd));
        wire.as_object_mut().unwrap().remove("seq");
        assert_eq!(decode_command(&wire).unwrap(), (17, cmd));
        wire.as_object_mut().unwrap().remove("at");
        assert!(
            decode_command(&wire).is_err(),
            "orders require a scheduled tick"
        );
    }
}

#[test]
fn splitting_and_recombining_a_mole_script_preserves_each_ticks_input() {
    // Infiltrator orders precede hacker orders within each tick, as required
    // by the existing script-splitting contract.
    let script: ScriptFile = serde_json::from_value(json!({
        "seed": 7, "max_ticks": 4, "scenario": "pump-house",
        "commands": [
            {"at": 0, "hold": ["right"], "press": ["mole:move:520:under", "pivot"]},
            {"at": 1, "press": ["mole:dig", "mole:sabotage", "hack:0"]},
            {"at": 2, "hold": [], "press": ["mole:synth:0"]}
        ]
    }))
    .unwrap();
    let infiltrator = seat_schedule(&script, Role::Infiltrator);
    let hacker = seat_schedule(&script, Role::Hacker);
    assert_eq!(
        infiltrator
            .iter()
            .filter(|s| matches!(s.cmd, Command::Mole { .. }))
            .count(),
        4
    );
    assert_eq!(hacker.len(), 2);
    assert!(
        hacker
            .iter()
            .all(|s| !matches!(s.cmd, Command::Mole { .. }))
    );
    assert_eq!(
        tick_inputs(&merged_by_tick(script.max_ticks, &infiltrator, &hacker)),
        expand(&script)
    );
}

//! The session-relay wire fixture round-trips through the Rust types unchanged.
//!
//! `fixtures/session_relay/` (repo root) is shared with the burble relay's
//! channel tests in the sibling Burble repository
//! (`server/test/burble_web/channels/game_channel_test.exs` there):
//! the Elixir side (in burble) proves the relay passes every fixture value through
//! verbatim; this side proves each of those values *is* a `Command` / `Event`
//! — it deserializes into the Rust type and re-serializes to the identical
//! JSON. Together they are the cross-language mapping proof issue #5 asks for.
//!
//! `events.json` is captured from a real run:
//!   idaptik-tui --headless --script fixtures/session_relay/capture_script.json
//! (the `event_log` field of its stdout). `commands.json` is the wire form of
//! the script's verbs plus the rest of the `Command` alphabet.

use idaptik_core::scenario::command::Command;
use idaptik_core::scenario::event::Event;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::PathBuf;

fn fixture(name: &str) -> Vec<Value> {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("../../fixtures/session_relay");
    p.push(name);
    let text =
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read fixture {}: {e}", p.display()));
    let v: Value = serde_json::from_str(&text).expect("fixture is valid JSON");
    v.as_array().expect("fixture is a JSON array").clone()
}

#[test]
fn relayed_commands_round_trip_unchanged() {
    let items = fixture("commands.json");
    assert!(!items.is_empty(), "the command stream is not empty");
    for item in &items {
        let cmd: Command = serde_json::from_value(item.clone())
            .unwrap_or_else(|e| panic!("not a Command: {item} ({e})"));
        let back = serde_json::to_value(cmd).expect("re-serializes");
        assert_eq!(&back, item, "Command round-trip changed the JSON");
    }
}

#[test]
fn relayed_events_round_trip_unchanged() {
    let items = fixture("events.json");
    assert!(!items.is_empty(), "the event log is not empty");
    for item in &items {
        let ev: Event = serde_json::from_value(item.clone())
            .unwrap_or_else(|e| panic!("not an Event: {item} ({e})"));
        let back = serde_json::to_value(&ev).expect("re-serializes");
        assert_eq!(&back, item, "Event round-trip changed the JSON");
    }
}

#[test]
fn command_fixture_covers_the_whole_alphabet() {
    // The relay's role table enumerates every `Command` variant by tag; the
    // fixture must keep exercising all of them so a new variant that lands in
    // the enum without landing on the wire (or in the Elixir table) fails here.
    let tags: BTreeSet<String> = fixture("commands.json")
        .iter()
        .map(|item| {
            item["cmd"]
                .as_str()
                .expect("tagged with \"cmd\"")
                .to_owned()
        })
        .collect();
    let expected: BTreeSet<String> = [
        "SetButton",
        "Jump",
        "Interact",
        "ThrowUsb",
        "Uplink",
        "Pivot",
        "Unpivot",
        "NetSsh",
        "NetHack",
        "Signal",
        "Mole",
        "ForceCrisis",
        "ForceExtract",
        "ForceFail",
        "Pause",
        "Restart",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(
        tags, expected,
        "commands.json drifted from the Command enum"
    );
}

#[test]
fn event_fixture_exercises_the_pivot_events() {
    // The v2 additions issue #5 calls out: the captured log must keep proving
    // PivotOpened / PivotDenied / PivotClosed survive the wire.
    let tags: BTreeSet<String> = fixture("events.json")
        .iter()
        .map(|item| {
            item["event"]
                .as_str()
                .expect("tagged with \"event\"")
                .to_owned()
        })
        .collect();
    for required in ["PivotOpened", "PivotDenied", "PivotClosed"] {
        assert!(tags.contains(required), "events.json lost {required}");
    }
}

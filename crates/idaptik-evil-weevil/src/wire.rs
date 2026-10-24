//! Raw wire-payload construction — the one place in the workspace allowed to
//! forge frames the typed client would never produce.
//!
//! The production clients build frames through
//! [`idaptik_net::envelope::encode_command`], which only accepts a typed
//! [`idaptik_core::scenario::command::Command`] and therefore cannot express a
//! malformed, stale or impossible frame. Adversarial testing must exercise the
//! *decoder's* response to exactly those frames, so Evil Weevil constructs
//! [`serde_json::Value`] payloads directly and the harness feeds them through
//! the same production decode path the live driver uses
//! ([`idaptik_net::envelope::decode_command`],
//! [`idaptik_net::lockstep::LockstepCore::on_peer_command`]).
//!
//! Nothing here sanitises or repairs an invalid condition: the invalidity is
//! the subject of the test. These helpers exist only inside this crate.

use idaptik_net::envelope::{HELLO_TAG, Role, NET_PROTO};
use idaptik_tui::script::ScriptFile;
use serde_json::{Value, json};

/// Put an arbitrary command object on the wire scheduled for `at`.
/// `cmd` is inserted verbatim, including unknown tags or out-of-range fields.
pub fn raw_command(at: u64, cmd: Value) -> Value {
    let mut map = match cmd {
        Value::Object(map) => map,
        other => {
            let mut map = serde_json::Map::new();
            map.insert("cmd".into(), other);
            map
        }
    };
    map.insert("at".into(), json!(at));
    Value::Object(map)
}

/// A command frame missing the mandatory `at` schedule — the decoder must
/// refuse it rather than guess a tick ([`idaptik_net::envelope::decode_command`]).
pub fn command_without_at(cmd: Value) -> Value {
    cmd
}

/// A command carrying a tag the `Command` enum does not know. The relay model
/// rejects unknown tags; the typed decoder cannot represent the frame.
pub fn unknown_command(at: u64, tag: &str) -> Value {
    raw_command(at, json!({ "cmd": tag }))
}

/// A `NetSsh` claim against an arbitrary graph index — including indices the
/// floor graph does not contain. The value decodes (any `u32` is a valid
/// field); the sim must treat an out-of-graph index as a no-op.
pub fn net_ssh(at: u64, node: u32) -> Value {
    raw_command(at, json!({ "cmd": "NetSsh", "node": node }))
}

/// A `NetHack` claim against an arbitrary graph index.
pub fn net_hack(at: u64, node: u32) -> Value {
    raw_command(at, json!({ "cmd": "NetHack", "node": node }))
}

/// A typed command re-serialised onto the wire with an explicit schedule.
/// Used to replay a *valid* frame at the wrong (stale) tick.
pub fn typed_command(at: u64, cmd: &idaptik_core::scenario::command::Command) -> Value {
    let value = serde_json::to_value(cmd).expect("typed commands always serialise");
    raw_command(at, value)
}

/// Attach the relay-side `seq` envelope to a frame. The relay strips this
/// before relaying; the in-process relay model uses it to judge staleness.
pub fn with_seq(mut payload: Value, seq: u64) -> Value {
    if let Some(map) = payload.as_object_mut() {
        map.insert("seq".into(), json!(seq));
    }
    payload
}

/// Per-field epoch overrides for [`raw_hello`]. Every field is optional; a
/// `None` field keeps the value the scripted hello would carry, so a scenario
/// can mutate exactly one epoch field at a time.
#[derive(Debug, Clone, Default)]
pub struct HelloOverrides<'a> {
    /// Override the advertised protocol version.
    pub proto: Option<u64>,
    /// Override the run-seed claim.
    pub seed: Option<u32>,
    /// Override the advertised run length.
    pub max_ticks: Option<u64>,
    /// Override the advertised difficulty label.
    pub difficulty: Option<&'a str>,
    /// Claim the rejoin (session-recovery) flag.
    pub rejoin: Option<bool>,
}

/// A `net:hello` control frame with per-field overrides.
pub fn raw_hello(
    role: Role,
    script: &ScriptFile,
    commands: u64,
    overrides: HelloOverrides<'_>,
) -> Value {
    let mut hello = idaptik_net::envelope::Hello::new(role, script, commands);
    if let Some(proto) = overrides.proto {
        hello.proto = proto;
    }
    if let Some(seed) = overrides.seed {
        hello.seed = seed;
    }
    if let Some(max_ticks) = overrides.max_ticks {
        hello.max_ticks = max_ticks;
    }
    if let Some(difficulty) = overrides.difficulty {
        hello.difficulty = difficulty.into();
    }
    if let Some(rejoin) = overrides.rejoin {
        hello.rejoin = rejoin;
    }
    serde_json::to_value(hello).expect("hello serialises")
}

/// A `net:hello` claiming a protocol version one above the supported one —
/// the epoch-conflict scenario's forged-epoch frame.
pub fn future_proto_hello(role: Role, script: &ScriptFile, commands: u64) -> Value {
    raw_hello(
        role,
        script,
        commands,
        HelloOverrides {
            proto: Some(NET_PROTO + 1),
            ..HelloOverrides::default()
        },
    )
}

/// A raw `net:commit` watermark with an explicit (possibly impossible)
/// `through` value.
pub fn raw_commit(role: Role, through: u64) -> Value {
    serde_json::json!({
        "event": idaptik_net::envelope::COMMIT_TAG,
        "role": role,
        "through": through,
    })
}

/// A bare control event with an arbitrary `net:`-namespaced tag, used to
/// prove the control vocabulary is closed: an unknown `net:` tag is ignored
/// by the drivers rather than interpreted.
pub fn unknown_control_event(tag: &str) -> Value {
    debug_assert!(tag.starts_with("net:"));
    json!({ "event": tag })
}

/// Re-serialize a [`idaptik_net::lockstep::Resync`] so the harness can mutate
/// individual fields before handing it to the production `adopt_resync`.
pub fn resync_to_value(
    resync: &idaptik_net::lockstep::Resync,
) -> Value {
    serde_json::to_value(resync).expect("resync serialises")
}

/// Parse a mutated resync value back through the production deserializer.
pub fn value_to_resync(
    value: Value,
) -> Result<idaptik_net::lockstep::Resync, serde_json::Error> {
    serde_json::from_value(value)
}

/// A recognisable but fake `net:` tag that collides with no real control
/// frame — namespaced correctly, vocabulary unknown.
pub const FAKE_CONTROL_TAG: &str = "net:evil-weevil-fake";

/// Keep the import used even when a build trims helpers.
pub const HELLO_EVENT: &str = HELLO_TAG;

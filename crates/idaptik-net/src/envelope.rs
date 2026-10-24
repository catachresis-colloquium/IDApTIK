//! The relay envelope and the `net:` control vocabulary.
//!
//! A relayed `"command"` payload is the serde `Command` JSON (tagged `"cmd"`,
//! ADR-0005) plus up to two envelope keys the sim never sees:
//!
//! - `"seq"` — strictly-increasing per seat; the relay acknowledges-and-drops
//!   duplicates and strips the key before relaying (ADR-0005).
//! - `"at"` — the lockstep tick this command is scheduled for. Authored and
//!   consumed by clients only; the relay relays it untouched without reading
//!   it (it strips exactly `"seq"`), and serde's internally-tagged decoding
//!   ignores it, so the typed payload stays byte-compatible. This is the
//!   scheduling seam ADR-0005 predicted would attach "when real clients
//!   arrive" (amended there, 2026-07-21).
//!
//! Control messages between the two net layers ride the relay's `"event"`
//! pass-through, namespaced `"net:*"` so they can never collide with the sim's
//! `Event` alphabet (Rust variant tags are `UpperCamelCase`; `:` cannot appear
//! in one). They are consumed by `idaptik-net` and never fed to the sim.

use crate::error::NetError;
use idaptik_core::scenario::command::Command;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A playable seat. `Role` is what you join as; either-seat commands have no
/// single role, so the routing table speaks [`Seat`], not `Role`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Infiltrator,
    Hacker,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Infiltrator => "infiltrator",
            Role::Hacker => "hacker",
        }
    }
}

impl std::str::FromStr for Role {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "infiltrator" => Ok(Role::Infiltrator),
            "hacker" => Ok(Role::Hacker),
            other => Err(format!("role must be infiltrator or hacker, got {other:?}")),
        }
    }
}

/// Which seat may send a command — the client-side mirror of the relay's
/// `command_roles()` from the game profile (`Burble.Games.Idaptik` in burble).
/// The relay enforces this; the client uses it to split a script and to route
/// either-seat commands deterministically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seat {
    Infiltrator,
    Hacker,
    Either,
}

/// The seat that owns `cmd`. Exhaustive on purpose: a new `Command` variant
/// must pick its seat here or this crate stops compiling — the same forcing
/// function the relay's routing table applies to the Elixir side.
pub fn seat_for(cmd: &Command) -> Seat {
    match cmd {
        Command::SetButton { .. }
        | Command::Jump
        | Command::Interact
        | Command::ThrowUsb
        | Command::MoleEnterRoute
        | Command::MoleSabotageCable
        | Command::MoleReturnToEntry
        | Command::MoleSurface => Seat::Infiltrator,
        Command::Uplink { .. }
        | Command::Pivot { .. }
        | Command::Unpivot
        | Command::NetSsh { .. }
        | Command::NetHack { .. } => Seat::Hacker,
        Command::Signal { op } => match op.seat() {
            idaptik_core::scenario::signal::sim::Seat::Infiltrator => Seat::Infiltrator,
            idaptik_core::scenario::signal::sim::Seat::Hacker => Seat::Hacker,
        },
        Command::ForceCrisis
        | Command::ForceExtract { .. }
        | Command::ForceFail { .. }
        | Command::Pause { .. }
        | Command::Restart => Seat::Either,
    }
}

/// Whether `role` sends `cmd` in a scripted run. Either-seat commands are
/// authored by the infiltrator: one deterministic owner, and the relay accepts
/// them from any seat, so the choice is free — but it must be *a* choice.
pub fn scripted_sender(cmd: &Command) -> Role {
    match seat_for(cmd) {
        Seat::Infiltrator | Seat::Either => Role::Infiltrator,
        Seat::Hacker => Role::Hacker,
    }
}

/// Wrap a command for the wire: the `Command` JSON plus `seq` + `at`.
pub fn encode_command(cmd: &Command, seq: u64, at: u64) -> Result<Value, NetError> {
    let value = serde_json::to_value(cmd)
        .map_err(|e| NetError::Protocol(format!("encode command: {e}")))?;
    let Value::Object(mut map) = value else {
        return Err(NetError::Protocol(
            "command did not serialize to an object".into(),
        ));
    };
    map.insert("seq".into(), seq.into());
    map.insert("at".into(), at.into());
    Ok(Value::Object(map))
}

/// Unwrap a relayed command payload into `(at, Command)`. The relay stripped
/// `seq`; `at` is required — a scheduled command with no schedule is a
/// protocol error, not a guess.
pub fn decode_command(payload: &Value) -> Result<(u64, Command), NetError> {
    let at = payload
        .get("at")
        .and_then(Value::as_u64)
        .ok_or_else(|| NetError::Protocol(format!("relayed command lacks \"at\": {payload}")))?;
    let cmd: Command = serde_json::from_value(payload.clone())
        .map_err(|e| NetError::Protocol(format!("decode relayed command: {e}")))?;
    Ok((at, cmd))
}

/// Control-message tags (values of the `"event"` key on the event relay).
pub const HELLO_TAG: &str = "net:hello";
pub const DIGEST_TAG: &str = "net:digest";
pub const COMMIT_TAG: &str = "net:commit";
pub const RESYNC_TAG: &str = "net:resync";

/// The lockstep protocol version inside `net:hello`. v2 added the live-seat
/// vocabulary: per-tick `net:commit` watermarks, the `rejoin` hello flag and
/// `net:resync` snapshot hand-off. A v1 (batch) seat and a v2 seat would talk
/// past each other, so the version gate refuses the pairing up front.
pub const NET_PROTO: u64 = 2;

/// The session handshake (ADR-0006 §3): announce who you are, the run config
/// you intend (fixture-style — the script header), and how many scheduled
/// commands you will send. Seats assert each other's copy matches; a mismatch
/// is a setup bug, and lockstep on mismatched configs would just be two
/// different games.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    pub event: String,
    pub proto: u64,
    pub role: Role,
    pub seed: u32,
    pub difficulty: String,
    pub reduced_motion: bool,
    pub max_ticks: u64,
    /// How many `"command"` pushes this seat will make. Meaningful for batch
    /// (scripted) seats; a live seat does not know its future and says `0`.
    pub commands: u64,
    /// A live seat re-entering a session it lost: it expects a `net:resync`
    /// from the surviving seat instead of a fresh tick-0 start.
    #[serde(default)]
    pub rejoin: bool,
}

impl Hello {
    pub fn new(role: Role, script: &idaptik_tui::script::ScriptFile, commands: u64) -> Self {
        Self {
            event: HELLO_TAG.into(),
            proto: NET_PROTO,
            role,
            seed: script.seed,
            difficulty: script.difficulty.clone(),
            reduced_motion: script.reduced_motion,
            max_ticks: script.max_ticks,
            commands,
            rejoin: false,
        }
    }

    /// The seats must be playing the same run. `role`/`commands`/`rejoin`
    /// legitimately differ; everything else must not. (Input delay is absent on
    /// purpose: it is per-seat local — every command carries its own `at` and
    /// every commit its own watermark, so seats with different delays
    /// interoperate.)
    pub fn compatible_with(&self, other: &Hello) -> bool {
        self.proto == other.proto
            && self.seed == other.seed
            && self.difficulty == other.difficulty
            && self.reduced_motion == other.reduced_motion
            && self.max_ticks == other.max_ticks
    }
}

/// The per-tick completeness watermark (live seats only): "every command of
/// mine scheduled `at <= through` has been pushed". Over an ordered transport
/// its arrival proves those commands already arrived, which is exactly what a
/// sparse command stream cannot say on its own — silence at tick T is
/// otherwise indistinguishable from a command still in flight.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Commit {
    pub role: Role,
    pub through: u64,
}

impl Commit {
    pub fn to_control(self) -> Value {
        serde_json::json!({
            "event": COMMIT_TAG,
            "role": self.role,
            "through": self.through,
        })
    }

    pub fn from_control(payload: &Value) -> Option<Self> {
        if event_tag(payload) != Some(COMMIT_TAG) {
            return None;
        }
        Some(Self {
            role: serde_json::from_value(payload.get("role")?.clone()).ok()?,
            through: payload.get("through")?.as_u64()?,
        })
    }
}

/// The end-of-run cross-check: a 64-bit FNV-1a digest of the serialized event
/// log. The loopback gate's authoritative comparison is the orchestrator's
/// byte-diff of both seats' artifacts; this in-band digest lets each seat
/// *also* observe divergence itself (ADR-0005's drift-detection mitigation).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Digest {
    pub event: String,
    pub role: Role,
    pub fnv1a64: String,
}

impl Digest {
    pub fn new(role: Role, event_log_json: &str) -> Self {
        Self {
            event: DIGEST_TAG.into(),
            role,
            fnv1a64: format!("{:016x}", fnv1a64(event_log_json.as_bytes())),
        }
    }
}

/// FNV-1a, 64-bit. Not cryptographic — a drift tripwire, not an integrity
/// proof — and dependency-free, which is what the job needs.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// Read the `"event"` tag of a relayed event payload, if any.
pub fn event_tag(payload: &Value) -> Option<&str> {
    payload.get("event").and_then(Value::as_str)
}

/// Whether a relayed event payload is `idaptik-net` control traffic (never fed
/// to the sim).
pub fn is_control(payload: &Map<String, Value>) -> bool {
    payload
        .get("event")
        .and_then(Value::as_str)
        .is_some_and(|t| t.starts_with("net:"))
}

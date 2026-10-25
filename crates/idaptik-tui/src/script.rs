//! The headless script / replay file format — the `__IDAPTIK_TEST__` API as data.
//!
//! A [`ScriptFile`] is a sparse, tick-indexed command timeline: `hold` sets the
//! persistent held-button set (until the next line changes it), `press` fires
//! edge/uplink commands on a single tick, and `test` injects a Force* hook.

use idaptik_core::scenario::ActionKind;
use idaptik_core::scenario::command::{
    Button, Buttons, Command, Edge, MoleOp, NetNodeIndex, PivotTarget, TickInput,
};
use idaptik_core::scenario::common::{ExtractMethod, FailReason};
use serde::{Deserialize, Serialize};

/// A full scripted run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptFile {
    pub seed: u32,
    #[serde(default = "default_difficulty")]
    pub difficulty: String,
    /// Which floor the script runs: absent (or `"ghost-lobby"`) is the
    /// canonical floor, so every existing script is byte-identical;
    /// `"ghost-lobby-wired"`, `"ghost-lobby-rooftop"` and `"pump-house"` name
    /// the other committed floors (see [`crate::headless::scenario_for`]).
    #[serde(default)]
    pub scenario: Option<String>,
    #[serde(default)]
    pub reduced_motion: bool,
    /// Run the VSM security-team supervision inside the sim. Defaults off, so
    /// every existing script keeps its byte-identical behaviour.
    #[serde(default)]
    pub supervised: bool,
    pub max_ticks: u64,
    #[serde(default)]
    pub commands: Vec<ScriptLine>,
    /// Optional recorded event log (for `--replay` cross-checking).
    #[serde(default)]
    pub expected_event_log: Option<serde_json::Value>,
}

fn default_difficulty() -> String {
    "standard".to_owned()
}

/// One sparse line in the timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptLine {
    pub at: u64,
    /// Held buttons that persist until the next line's `hold` replaces them.
    #[serde(default)]
    pub hold: Option<Vec<String>>,
    /// Edge/uplink commands fired only on tick `at`.
    #[serde(default)]
    pub press: Option<Vec<String>>,
    /// A Force* test hook fired on tick `at`.
    #[serde(default)]
    pub test: Option<TestHook>,
}

/// A scripted test hook (the headless side of `__IDAPTIK_TEST__`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "hook", rename_all = "snake_case")]
#[allow(clippy::enum_variant_names)]
pub enum TestHook {
    ForceCrisis,
    ForceExtract { method: String },
    ForceFail { reason: String },
}

impl TestHook {
    /// The immediate [`Command`] this hook injects — the one mapping shared by
    /// [`expand`] and the per-seat split in `idaptik-net`.
    pub fn command(&self) -> Command {
        match self {
            TestHook::ForceCrisis => Command::ForceCrisis,
            TestHook::ForceExtract { method } => Command::ForceExtract {
                method: extract_method(method),
            },
            TestHook::ForceFail { reason } => Command::ForceFail {
                reason: fail_reason(reason),
            },
        }
    }
}

/// Resolve a script `hold` token to a [`Button`]. Public because the token
/// vocabulary is a contract: `idaptik-net` splits one script across two seats
/// and must resolve tokens exactly as [`expand`] does.
pub fn button_from(name: &str) -> Option<Button> {
    match name.trim().to_ascii_lowercase().as_str() {
        "left" | "a" => Some(Button::Left),
        "right" | "d" => Some(Button::Right),
        "crouch" | "s" => Some(Button::Crouch),
        "sprint" | "shift" => Some(Button::Sprint),
        "interact" | "e" => Some(Button::Interact),
        _ => None,
    }
}

/// Resolve a script `press` token to a [`Command`]. Public for the same reason
/// as [`button_from`]: one token vocabulary, resolved identically everywhere.
/// Trims whitespace and ignores ASCII case. Supports `hack:<node-index>`,
/// `mole:dig`, `mole:sabotage`, `mole:synth:<index>`, and `mole:move:<x>[:under]`.
/// Move coordinates are world-x positions. Unknown tokens and numeric parse
/// failures return `None`; target existence and order availability are checked
/// by the simulation.
pub fn press_command(name: &str) -> Option<Command> {
    match name.trim().to_ascii_lowercase().as_str() {
        "jump" | "w" | "space" => Some(Command::Jump),
        "interact" | "e" => Some(Command::Interact),
        "throw" | "q" => Some(Command::ThrowUsb),
        "camera" | "1" => Some(Command::Uplink {
            kind: ActionKind::Camera,
        }),
        "door" | "2" => Some(Command::Uplink {
            kind: ActionKind::Door,
        }),
        "vacuum" | "3" => Some(Command::Uplink {
            kind: ActionKind::Vacuum,
        }),
        "lights" | "4" => Some(Command::Uplink {
            kind: ActionKind::Lights,
        }),
        // The pivots. A script name is lower-cased before it is matched, so the
        // keyboard's case distinction between `p` and `P` cannot survive here;
        // each foothold is spelt out by name instead.
        "pivot" | "bridge" | "p" => Some(Command::Pivot {
            target: PivotTarget::Bridge,
        }),
        "isp" | "ops" => Some(Command::Pivot {
            target: PivotTarget::IspOps,
        }),
        // Not "jump": that is already the `w` key, and the grid's jump host is not
        // a thing the body does.
        "grid" | "g" => Some(Command::Pivot {
            target: PivotTarget::GridJump,
        }),
        "unpivot" | "x" => Some(Command::Unpivot),
        // Session commands, spelt as explicit states rather than the keyboard's
        // toggle so a script line means the same thing wherever it lands.
        "pause" => Some(Command::Pause { on: true }),
        "resume" => Some(Command::Pause { on: false }),
        // The match arm sees the original token, so re-normalise the case for
        // the parameterised families (`Mole:Dig` means `mole:dig`).
        _ => parameterised(&name.trim().to_ascii_lowercase()),
    }
}

/// The parameterised token families: `hack:<node>` for the Net View click and
/// the `mole:*` orders for a hosted companion (ADR-0014). A malformed token is
/// `None` exactly like an unknown bare token — scripts are validated by what
/// they do, and a typo'd order simply never fires.
fn parameterised(name: &str) -> Option<Command> {
    if let Some(index) = name.strip_prefix("hack:") {
        let index: u32 = index.trim().parse().ok()?;
        return Some(Command::NetHack {
            node: NetNodeIndex(index),
        });
    }
    let rest = name.strip_prefix("mole:")?;
    let op = match rest.split(':').collect::<Vec<_>>().as_slice() {
        ["dig"] => MoleOp::Dig,
        ["sabotage"] => MoleOp::Sabotage,
        ["synth", index] => MoleOp::Synth {
            index: index.trim().parse::<u8>().ok()?,
        },
        ["move", x] => MoleOp::MoveTo {
            x: x.trim().parse::<f64>().ok()?,
            underground: false,
        },
        ["move", x, "under"] => MoleOp::MoveTo {
            x: x.trim().parse::<f64>().ok()?,
            underground: true,
        },
        _ => return None,
    };
    Some(Command::Mole { op })
}

fn extract_method(name: &str) -> ExtractMethod {
    match name
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '-'], "_")
        .as_str()
    {
        "laundry_chute" | "chute" => ExtractMethod::LaundryChute,
        _ => ExtractMethod::ServiceExit,
    }
}

fn fail_reason(name: &str) -> FailReason {
    match name.trim().to_ascii_lowercase().as_str() {
        "partition" => FailReason::Partition,
        "lockdown" => FailReason::Lockdown,
        "traced" => FailReason::Traced,
        _ => FailReason::Caught,
    }
}

/// Convert a press/edge command into an [`Edge`] or an immediate [`Command`].
fn classify(cmd: Command, edges: &mut Vec<Edge>, immediates: &mut Vec<Command>) {
    match cmd {
        Command::Jump => edges.push(Edge::JumpUp),
        Command::Interact => edges.push(Edge::InteractPress),
        Command::ThrowUsb => edges.push(Edge::Throw),
        other => immediates.push(other),
    }
}

/// Expand a sparse [`ScriptFile`] into a dense per-tick [`TickInput`] sequence.
///
/// `hold` persists across ticks until replaced; `press`/`test` fire only on
/// their line's tick.
pub fn expand(script: &ScriptFile) -> Vec<TickInput> {
    let mut lines = script.commands.clone();
    lines.sort_by_key(|l| l.at);

    let mut inputs: Vec<TickInput> = Vec::with_capacity(script.max_ticks as usize);
    let mut held_names: Vec<String> = Vec::new();
    let mut cursor = 0usize;

    for tick in 0..script.max_ticks {
        let mut edges: Vec<Edge> = Vec::new();
        let mut immediates: Vec<Command> = Vec::new();

        while cursor < lines.len() && lines[cursor].at == tick {
            let line = &lines[cursor];
            if let Some(hold) = &line.hold {
                held_names = hold.clone();
            }
            if let Some(press) = &line.press {
                for name in press {
                    if let Some(cmd) = press_command(name) {
                        classify(cmd, &mut edges, &mut immediates);
                    }
                }
            }
            if let Some(test) = &line.test {
                immediates.push(test.command());
            }
            cursor += 1;
        }

        let mut buttons = Buttons::default();
        for name in &held_names {
            if let Some(b) = button_from(name) {
                buttons.set(b, true);
            }
        }

        inputs.push(TickInput {
            buttons,
            edges,
            immediates,
        });
    }
    inputs
}

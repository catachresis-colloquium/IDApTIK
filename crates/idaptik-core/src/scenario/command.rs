//! Typed commands and the per-tick input the simulation consumes.
//!
//! Held movement is a [`Buttons`] bitset that persists across ticks; discrete
//! actions are [`Edge`]s; and session/uplink/test commands are *immediates*
//! applied before the systems run. [`fold`] collapses an ordered stream of
//! [`Command`]s into one [`TickInput`], mutating the caller's persistent held
//! set — this is the seam the Elixir session layer and the TUI frontend share.

use crate::scenario::common::{ExtractMethod, FailReason};
use crate::scenario::power::sim::PowerOp;
use crate::scenario::signal::sim::SignalOp;
use crate::scenario::tuning::{ActionKind, DifficultyId};
use serde::{Deserialize, Serialize};

/// A held movement button. The discriminant is the bitset flag.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Button {
    Left = 1,
    Right = 2,
    Crouch = 4,
    Sprint = 8,
    Interact = 16,
}

/// The set of currently-held movement buttons.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Buttons(pub u8);

impl Buttons {
    /// Whether `b` is held.
    pub fn has(self, b: Button) -> bool {
        self.0 & (b as u8) != 0
    }

    /// Press or release `b`.
    pub fn set(&mut self, b: Button, down: bool) {
        if down {
            self.0 |= b as u8;
        } else {
            self.0 &= !(b as u8);
        }
    }
}

/// A discrete (edge-triggered) action consumed inside the systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Edge {
    /// A jump was requested this tick.
    JumpUp,
    /// The interact key was pressed this tick (distinct from being held).
    InteractPress,
    /// Throw the carried USB.
    Throw,
    /// An uplink action edge (frontend convenience; the sim applies uplinks as
    /// immediates — see [`fold`]).
    Uplink(ActionKind),
}

/// Per-run configuration that affects simulation math.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RunConfig {
    pub difficulty: DifficultyId,
    /// Shortens the lights flicker window (with `supervised`, one of the two
    /// knobs that change sim math — ADR-0004 §6 as amended by the
    /// supervised-coverage slice).
    pub reduced_motion: bool,
    /// Runs the VSM security-team supervision inside the sim: evidence folds
    /// into `RuntimeState::supervision` and Billy's Assess patrol band bends
    /// toward the coverage target. Defaults off, which keeps every existing
    /// script and golden byte-identical.
    #[serde(default)]
    pub supervised: bool,
}

impl RunConfig {
    /// Standard difficulty, full motion, unsupervised — the canonical/headless
    /// default.
    pub fn standard() -> Self {
        Self {
            difficulty: DifficultyId::Standard,
            reduced_motion: false,
            supervised: false,
        }
    }
}

/// A foothold the hacker can pivot onto, named rather than addressed.
///
/// The hostnames themselves are authored in the floor's backbone, so the wire
/// carries the *choice* and [`crate::scenario::floor_graph::pivot_host`] resolves
/// it. That keeps [`Command`] `Copy` (a `String` host would not be) and keeps a
/// renamed host from breaking every recorded script, exactly as [`ActionKind`]
/// names the four uplinks rather than the fixtures they land on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PivotTarget {
    /// The building's maintenance bridge: one hop, and the whole floor answers.
    Bridge,
    /// The ISP's operations host: the first hop of the upstream line.
    IspOps,
    /// The grid jump host: the second hop, and the only way to the substation.
    GridJump,
}

/// A node in the grounded network graph, addressed by its position in
/// `GroundedGraph::nodes` rather than by id or hostname. This keeps `Command`
/// `Copy`, exactly as `PivotTarget` keeps pivoting off a named enum rather
/// than a `String` host; the index is stable because the graph is built once,
/// deterministically, from the scenario definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NetNodeIndex(pub u32);

/// One order to a hosted Moletaire companion (ADR-0014). The companion's own
/// [`crate::companion::MoleCommand`] vocabulary is richer (items, equipment,
/// feeding); the wire carries only the orders a seat issues mid-run, and only
/// in `Copy` shapes — an `f64` position and a small sound index, never a
/// `String`. A floor with no hosted mole denies these; the command never
/// panics and never reaches the companion sim except through the host's
/// typed boundary.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum MoleOp {
    /// Order the mole to a position, tunnelling or on the surface.
    MoveTo { x: f64, underground: bool },
    /// Dig a trap at the mole's current position.
    Dig,
    /// Chew through the cable at the mole's current position — the nearest
    /// device node inside [`crate::scenario::constants::MOLE_SABOTAGE_RADIUS`]
    /// loses power through the ordinary cascade path when the chew finishes.
    Sabotage,
    /// Play one of the synth diversion sounds (needs an AudioSynthesiser the
    /// floor fitted; a stock bay plays nothing).
    Synth { index: u8 },
}

/// A single wire command.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd")]
pub enum Command {
    /// Press/release a held movement button.
    SetButton { button: Button, down: bool },
    /// Edge: jump.
    Jump,
    /// Edge: interact press.
    Interact,
    /// Edge: throw the carried USB.
    ThrowUsb,
    /// Immediate: perform an uplink action.
    Uplink { kind: ActionKind },
    /// Immediate: pivot the hacker onto a foothold. Cold from the van nothing on
    /// the floor answers, so this is the price of acting at all.
    Pivot { target: PivotTarget },
    /// Immediate: back out of one pivot, handing the link back what the reach
    /// was costing it.
    Unpivot,
    /// Immediate: pivot the hacker onto an arbitrary graph node (Net View
    /// click). Costs exactly what a named pivot costs today: trace advance,
    /// nothing more.
    NetSsh { node: NetNodeIndex },
    /// Immediate: hack an arbitrary graph node (Net View click). A node that
    /// already belongs to one of the four uplink actions runs that action's
    /// exact economy; every other actuatable node runs a new, separate flat
    /// cost/cooldown gate.
    NetHack { node: NetNodeIndex },
    /// Immediate: lay, cut or invert a wire, or re-program a relay, on the
    /// floor's signal network (ADR-0012). Either seat; `op` says which, and
    /// the sim checks that seat can touch the receiving device.
    Signal { op: SignalOp },
    /// Immediate: order the hosted Moletaire companion (ADR-0014). Denied on
    /// a floor with no mole; the mole is the infiltrator's companion, so the
    /// seat table routes it to the infiltrator.
    Mole { op: MoleOp },
    /// Immediate: work the floor's electrical infrastructure (ADR-0016) —
    /// cut a cable by hand, rewire a load, work a breaker, throw a source,
    /// switch a device, or ask why one is off. `op` says which, and the sim
    /// checks that seat can touch what it names.
    Power { op: PowerOp },
    /// Immediate test hook: force the crisis phase.
    ForceCrisis,
    /// Immediate test hook: force an extraction.
    ForceExtract { method: ExtractMethod },
    /// Immediate test hook: force a mission failure.
    ForceFail { reason: FailReason },
    /// Immediate: pause/resume.
    Pause { on: bool },
    /// Immediate: restart from the same seed.
    Restart,
}

/// One tick's worth of input.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TickInput {
    /// The held button set (carried across ticks).
    pub buttons: Buttons,
    /// Edge actions for this tick, in stream order.
    pub edges: Vec<Edge>,
    /// Immediates (uplink/test/session) for this tick, in stream order.
    pub immediates: Vec<Command>,
}

/// Fold an ordered command stream into a [`TickInput`], mutating the persistent
/// held-button set. `SetButton` updates `held`; edges and immediates are queued
/// in stream order.
pub fn fold(cmds: &[Command], held: &mut Buttons) -> TickInput {
    let mut edges = Vec::new();
    let mut immediates = Vec::new();
    for cmd in cmds {
        match cmd {
            Command::SetButton { button, down } => held.set(*button, *down),
            Command::Jump => edges.push(Edge::JumpUp),
            Command::Interact => edges.push(Edge::InteractPress),
            Command::ThrowUsb => edges.push(Edge::Throw),
            Command::Uplink { .. }
            | Command::Pivot { .. }
            | Command::Unpivot
            | Command::NetSsh { .. }
            | Command::NetHack { .. }
            | Command::Signal { .. }
            | Command::Mole { .. }
            | Command::Power { .. }
            | Command::ForceCrisis
            | Command::ForceExtract { .. }
            | Command::ForceFail { .. }
            | Command::Pause { .. }
            | Command::Restart => immediates.push(*cmd),
        }
    }
    TickInput {
        buttons: *held,
        edges,
        immediates,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_bitset() {
        let mut b = Buttons::default();
        assert!(!b.has(Button::Left));
        b.set(Button::Left, true);
        b.set(Button::Sprint, true);
        assert!(b.has(Button::Left));
        assert!(b.has(Button::Sprint));
        assert!(!b.has(Button::Right));
        b.set(Button::Left, false);
        assert!(!b.has(Button::Left));
        assert!(b.has(Button::Sprint));
    }

    #[test]
    fn fold_persists_held_and_orders_edges() {
        let mut held = Buttons::default();
        let stream = [
            Command::SetButton {
                button: Button::Right,
                down: true,
            },
            Command::Jump,
            Command::Uplink {
                kind: ActionKind::Camera,
            },
            Command::Interact,
        ];
        let input = fold(&stream, &mut held);
        assert!(input.buttons.has(Button::Right));
        assert_eq!(input.edges, vec![Edge::JumpUp, Edge::InteractPress]);
        assert_eq!(input.immediates.len(), 1);
        // held persists into the next (empty) tick.
        let next = fold(&[], &mut held);
        assert!(next.buttons.has(Button::Right));
        assert!(next.edges.is_empty());
    }

    #[test]
    fn the_pivot_verbs_fold_into_immediates() {
        // A pivot changes where the hacker plays from, which every later system
        // reads. It must land before them, exactly as an uplink does, rather than
        // queue as an edge the systems consume at their leisure.
        let mut held = Buttons::default();
        let input = fold(
            &[
                Command::Pivot {
                    target: PivotTarget::IspOps,
                },
                Command::Pivot {
                    target: PivotTarget::GridJump,
                },
                Command::Unpivot,
            ],
            &mut held,
        );
        assert!(input.edges.is_empty());
        assert_eq!(
            input.immediates,
            vec![
                Command::Pivot {
                    target: PivotTarget::IspOps
                },
                Command::Pivot {
                    target: PivotTarget::GridJump
                },
                Command::Unpivot,
            ],
            "the stream order is the pivot order, and it is load-bearing"
        );
    }

    #[test]
    fn the_pivot_verbs_round_trip_on_the_wire() {
        // `Command` is the protocol the Elixir session layer and the TUI share, so
        // a new variant that does not survive the wire is not a command at all.
        for cmd in [
            Command::Pivot {
                target: PivotTarget::Bridge,
            },
            Command::Pivot {
                target: PivotTarget::IspOps,
            },
            Command::Pivot {
                target: PivotTarget::GridJump,
            },
            Command::Unpivot,
        ] {
            let json = serde_json::to_string(&cmd).expect("serialises");
            let back: Command = serde_json::from_str(&json).expect("deserialises");
            assert_eq!(back, cmd, "{json}");
        }
        // The externally-tagged shape the existing variants carry, pinned.
        assert_eq!(
            serde_json::to_string(&Command::Unpivot).expect("serialises"),
            r#"{"cmd":"Unpivot"}"#
        );
        assert_eq!(
            serde_json::to_string(&Command::Pivot {
                target: PivotTarget::GridJump
            })
            .expect("serialises"),
            r#"{"cmd":"Pivot","target":"GridJump"}"#
        );
    }

    #[test]
    fn the_mole_verb_round_trips_on_the_wire() {
        // The companion orders ride the same `Command` protocol as everything
        // else: a variant that does not survive the wire is not a command.
        for cmd in [
            Command::Mole { op: MoleOp::Dig },
            Command::Mole {
                op: MoleOp::Sabotage,
            },
            Command::Mole {
                op: MoleOp::Synth { index: 1 },
            },
            Command::Mole {
                op: MoleOp::MoveTo {
                    x: 350.0,
                    underground: true,
                },
            },
        ] {
            let json = serde_json::to_string(&cmd).expect("serialises");
            let back: Command = serde_json::from_str(&json).expect("deserialises");
            assert_eq!(back, cmd, "{json}");
        }
        assert_eq!(
            serde_json::to_string(&Command::Mole { op: MoleOp::Dig }).expect("serialises"),
            r#"{"cmd":"Mole","op":{"op":"Dig"}}"#
        );
    }

    #[test]
    fn the_net_view_verbs_round_trip_on_the_wire() {
        // Net View addresses a node by its position in `GroundedGraph::nodes`
        // rather than by a `String` host or id, so `Command` stays `Copy`:
        // exactly the reason `PivotTarget` is a named enum and not a raw host.
        for cmd in [
            Command::NetSsh {
                node: NetNodeIndex(0),
            },
            Command::NetHack {
                node: NetNodeIndex(3),
            },
        ] {
            let json = serde_json::to_string(&cmd).expect("serialises");
            let back: Command = serde_json::from_str(&json).expect("deserialises");
            assert_eq!(back, cmd, "{json}");
        }
        assert_eq!(
            serde_json::to_string(&Command::NetSsh {
                node: NetNodeIndex(0)
            })
            .expect("serialises"),
            r#"{"cmd":"NetSsh","node":0}"#
        );
        assert_eq!(
            serde_json::to_string(&Command::NetHack {
                node: NetNodeIndex(3)
            })
            .expect("serialises"),
            r#"{"cmd":"NetHack","node":3}"#
        );
    }
}

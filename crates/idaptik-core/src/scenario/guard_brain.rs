//! The seam between IDApTIK's guard AI and an external decision kernel.
//!
//! IDApTIK already has a complete, deterministic guard FSM. This module adds a
//! **selectable** brain seam so a kernel — Evil Weevil — can drive the same
//! guard without the game giving up any of its authority:
//!
//! * The game owns truth, physics, collision, navigation, guard identity and
//!   entity lifecycle, and it validates and applies every proposed action.
//! * The kernel owns per-agent memory and decision state, and it proposes.
//!
//! The types here are plain data with no dependency on the kernel. That keeps
//! `idaptik-core` free of the native adapter: the game never links the kernel,
//! and the two can evolve, be tested and be rolled back independently.

/// Which brain is driving the guard. The classic FSM is the default and is
/// always selectable, so the kernel can be compared against it and rolled back
/// without a rebuild.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GuardBrainKind {
    /// The deterministic IDApTIK FSM. This is the default and the rollback path.
    #[default]
    Classic,
    /// An external kernel behind [`GuardPerception`] / [`GuardDecision`].
    Kernel,
}

/// Host-assigned ids for the contacts the game can put in front of the guard.
/// The kernel sees these as intent target ids; the game resolves them against
/// its own entity lifecycle before it acts on them.
pub mod contact_id {
    /// The infiltrator.
    pub const PLAYER: u32 = 1;
    /// The contact note, while it is still on the table.
    pub const NOTE: u32 = 2;
    /// The USB drive, while it is still loose.
    pub const USB: u32 = 3;
    /// The robot vacuum, while it is running. Exposed to the guard as a decoy.
    pub const VACUUM: u32 = 4;
}

/// One contact the guard can perceive this tick, in plain world coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct GuardContact {
    /// Host-assigned id. The kernel sees it as the target id on an intent.
    pub id: u32,
    /// `"player"`, `"object:<id>"`, `"decoy"` or `"sound"`.
    pub source: String,
    pub x: f64,
    pub y: f64,
    pub distance: f64,
    pub threat: f64,
    pub confidence: f64,
    /// Ticks since the host last refreshed this contact.
    pub age_ticks: u32,
}

/// Everything the guard can perceive this tick. This is the observation the
/// host passes to the kernel; it is a *perception*, not the world's truth.
///
/// The host sets the kernel's capability mask itself — it is a host property,
/// not world state, so it does not belong on the perception.
#[derive(Debug, Clone, PartialEq)]
pub struct GuardPerception {
    pub tick: u64,
    pub self_x: f64,
    pub self_y: f64,
    pub facing: f64,
    pub health: f64,
    pub ammo: f64,
    pub contacts: Vec<GuardContact>,
}

/// An action the kernel proposes. The host validates it and decides what, if
/// anything, to do with it — the kernel never moves the guard itself.
#[derive(Debug, Clone, PartialEq)]
pub struct GuardDecision {
    /// The kernel's action name, e.g. `"ADVANCE"` or `"INVESTIGATE"`.
    pub action: String,
    /// The contact id the action refers to, or `0` when none.
    pub target_id: u32,
    /// The world position the kernel wants to reach. Advisory: the host applies
    /// its own navigation from the action and target.
    pub x: f64,
    pub y: f64,
    pub speed: f64,
    pub priority: u64,
    pub degraded: bool,
    pub new_target: bool,
    pub unreachable: bool,
    pub from_memory: bool,
}

impl GuardDecision {
    /// Build a decision from a kernel intent, in the adapter's own vocabulary.
    #[must_use]
    pub fn new(action: impl Into<String>, target_id: u32, x: f64, y: f64, speed: f64) -> Self {
        Self {
            action: action.into(),
            target_id,
            x,
            y,
            speed,
            priority: 0,
            degraded: false,
            new_target: false,
            unreachable: false,
            from_memory: false,
        }
    }
}

/// What the host did with a proposed decision. Recorded for the replay and the
/// developer view, so a rejected proposal is visible rather than silent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuardApplyOutcome {
    pub accepted: bool,
    pub moved: bool,
    pub blocked: bool,
    pub reason: &'static str,
}

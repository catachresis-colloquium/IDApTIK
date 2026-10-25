//! Kernel → host intents.
//!
//! An [`Intent`] is the kernel's *proposal*: what action it wants, at what
//! target, with what direction, at what speed, and which of its flags fired.
//! The host owns the validator: it decides whether the proposal is legal in this
//! world (is the target alive? is the speed within the game's bounds? did the
//! kernel degrade?) and what to actually do about it.
//!
//! The adapter's job here is to decode and range-check the ABI's raw intent, so
//! a kernel that returns a code outside the frozen enum fails loudly instead of
//! being silently mapped to `IDLE`.

use crate::abi::{self, EeIntent};
use crate::error::{Result, WeevilError};
use crate::fx::Fx2;

/// What the kernel asks the host to do. Values are stable ABI; new actions are
/// appended, never reordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Idle,
    Advance,
    Hold,
    Flee,
    Attack,
    Reload,
    TakeCover,
    Regroup,
    Investigate,
}

impl Action {
    /// The ABI's numeric code.
    pub fn code(self) -> u32 {
        match self {
            Self::Idle => abi::ACTION_IDLE,
            Self::Advance => abi::ACTION_ADVANCE,
            Self::Hold => abi::ACTION_HOLD,
            Self::Flee => abi::ACTION_FLEE,
            Self::Attack => abi::ACTION_ATTACK,
            Self::Reload => abi::ACTION_RELOAD,
            Self::TakeCover => abi::ACTION_TAKE_COVER,
            Self::Regroup => abi::ACTION_REGROUP,
            Self::Investigate => abi::ACTION_INVESTIGATE,
        }
    }

    /// The stable name, for logs and the developer view.
    pub fn name(self) -> &'static str {
        match self {
            Self::Idle => "IDLE",
            Self::Advance => "ADVANCE",
            Self::Hold => "HOLD",
            Self::Flee => "FLEE",
            Self::Attack => "ATTACK",
            Self::Reload => "RELOAD",
            Self::TakeCover => "TAKE_COVER",
            Self::Regroup => "REGROUP",
            Self::Investigate => "INVESTIGATE",
        }
    }

    /// Decode an ABI code, rejecting anything outside the frozen enum.
    pub fn from_code(code: u32) -> Result<Self> {
        Ok(match code {
            abi::ACTION_IDLE => Self::Idle,
            abi::ACTION_ADVANCE => Self::Advance,
            abi::ACTION_HOLD => Self::Hold,
            abi::ACTION_FLEE => Self::Flee,
            abi::ACTION_ATTACK => Self::Attack,
            abi::ACTION_RELOAD => Self::Reload,
            abi::ACTION_TAKE_COVER => Self::TakeCover,
            abi::ACTION_REGROUP => Self::Regroup,
            abi::ACTION_INVESTIGATE => Self::Investigate,
            other => return Err(WeevilError::InvalidAction(other)),
        })
    }

    /// The mode bit this action commits the agent to (ADR-0012).
    pub fn mode_bit(self) -> u32 {
        match self {
            Self::Reload => abi::AGENT_MODE_RELOAD,
            Self::Flee => abi::AGENT_MODE_EVADE,
            Self::Attack => abi::AGENT_MODE_ENGAGE,
            Self::Investigate => abi::AGENT_MODE_INVESTIGATE,
            // HOLD, IDLE and the actions the graph does not yet produce are the
            // neutral mode, exactly as `Abi.Modes.actionMode` says.
            _ => abi::AGENT_MODE_ADVANCE,
        }
    }
}

/// The bits the kernel can set in `ee_intent.flags`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IntentFlags {
    pub degraded: bool,
    pub new_target: bool,
    pub unreachable: bool,
    pub from_memory: bool,
}

impl IntentFlags {
    /// Decode the raw flag word, ignoring bits the ABI has not defined.
    pub fn from_raw(raw: u32) -> Self {
        Self {
            degraded: raw & abi::INTENT_FLAG_DEGRADED != 0,
            new_target: raw & abi::INTENT_FLAG_NEW_TARGET != 0,
            unreachable: raw & abi::INTENT_FLAG_UNREACHABLE != 0,
            from_memory: raw & abi::INTENT_FLAG_FROM_MEMORY != 0,
        }
    }

    /// The raw flag word.
    pub fn to_raw(self) -> u32 {
        let mut raw = 0;
        if self.degraded {
            raw |= abi::INTENT_FLAG_DEGRADED;
        }
        if self.new_target {
            raw |= abi::INTENT_FLAG_NEW_TARGET;
        }
        if self.unreachable {
            raw |= abi::INTENT_FLAG_UNREACHABLE;
        }
        if self.from_memory {
            raw |= abi::INTENT_FLAG_FROM_MEMORY;
        }
        raw
    }
}

/// A validated kernel proposal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Intent {
    pub action: Action,
    pub target_id: u32,
    /// Proposed movement direction, Q16.16. Advisory: see the bearing note in
    /// [`crate::observation`]. The host validates and applies its own nav.
    pub r#move: Fx2,
    pub look: Fx2,
    /// Proposed speed in fx units.
    pub speed: i32,
    pub priority: u32,
    pub budget_used: u32,
    pub flags: IntentFlags,
}

impl Intent {
    /// Decode a raw ABI intent.
    ///
    /// This checks what the ABI can guarantee — the action is a defined code —
    /// and leaves target liveness and speed bounds to the host, which is the
    /// only thing that knows which entities are alive and how fast they may go.
    pub fn from_ee(raw: &EeIntent) -> Result<Self> {
        Ok(Self {
            action: Action::from_code(raw.action)?,
            target_id: raw.target_id,
            r#move: Fx2 {
                x: raw.move_x,
                y: raw.move_y,
            },
            look: Fx2 {
                x: raw.look_x,
                y: raw.look_y,
            },
            speed: raw.speed,
            priority: raw.priority,
            budget_used: raw.budget_used,
            flags: IntentFlags::from_raw(raw.flags),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_codes_round_trip() {
        for a in [
            Action::Idle,
            Action::Advance,
            Action::Hold,
            Action::Flee,
            Action::Attack,
            Action::Reload,
            Action::TakeCover,
            Action::Regroup,
            Action::Investigate,
        ] {
            assert_eq!(Action::from_code(a.code()).unwrap(), a);
        }
    }

    #[test]
    fn an_out_of_range_action_is_refused() {
        assert_eq!(
            Action::from_code(abi::ACTION_MAX + 1).unwrap_err(),
            WeevilError::InvalidAction(abi::ACTION_MAX + 1)
        );
    }

    #[test]
    fn flags_decode_and_reencode() {
        let f = IntentFlags {
            degraded: true,
            from_memory: true,
            ..Default::default()
        };
        assert_eq!(IntentFlags::from_raw(f.to_raw()), f);
    }
}

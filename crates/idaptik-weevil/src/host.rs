//! The host: owns the kernel context and agent slots, drives the tick.
//!
//! [`WeevilHost`] is the whole of the native integration. It owns the
//! kernel-owned storage (`ee_context` plus one `ee_agent` per guard), performs
//! the handshake, converts each observation into a snapshot, calls `ee_tick`,
//! and decodes the returned intent. Every `unsafe` FFI call in the crate lives
//! behind this module's private boundary; the rest of the crate, and the whole
//! of `idaptik-core`, is safe Rust.
//!
//! # Authority
//!
//! The host never lets the kernel move anything. It validates the proposal and
//! returns it; the game's action validator decides whether it is legal and
//! applies the effect. The host additionally enforces the invariants the kernel
//! cannot: the observation's tick must be exactly one past the agent's last, an
//! unknown agent id is refused, and a restarted or despawned guard gets fresh
//! state rather than inheriting a predecessor's memory.

use crate::abi::{self, EeAgent, EeContext, EeInitDesc, EeIntent, EeSnapshot, EeVersion};
use crate::error::{Result, WeevilError};
use crate::fx::Fx2;
use crate::intent::Intent;
use crate::observation::Observation;
use core::mem::size_of;

/// The kernel-owned memory of one contact it no longer sees: a bearing and an
/// age. **Not a position.** The kernel has no fixed-point vector arithmetic to
/// correct a remembered position as the agent moves, so it remembers a direction
/// (ADR-0009). The host must not treat this as a world coordinate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Memory {
    /// The remembered direction, Q16.16.
    pub bearing: Fx2,
    /// Ticks since the sighting. Valid only when [`Memory::valid`].
    pub age: u32,
    /// The contact id the memory is about — with a caveat the host must not
    /// paper over. v0's `ee_agent` has no separate field for the remembered
    /// target, so this is read back from `cached_target`, which the kernel
    /// overwrites every tick from the *decision's* target. The two agree on
    /// exactly the ticks where the memory is what decided — an `INVESTIGATE`,
    /// whose target is `cached_target` unchanged — and diverge on a `RELOAD`
    /// or a no-contact `ADVANCE`, which name target `0`. So this field is a
    /// reliable "what the kernel is investigating" only while the memory is
    /// the deciding factor; the bearing and the age are the load-bearing part.
    pub target: u32,
}

impl Memory {
    /// Whether this memory is still actionable.
    pub fn valid(self) -> bool {
        self.age < abi::MEMORY_TTL
    }
}

/// The kernel-owned per-agent state, decoded for the host and the replay.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AgentState {
    pub rng_state: u64,
    pub tick_last: u64,
    pub memory: Option<Memory>,
    pub cached_action: u32,
    pub cached_target: u32,
    pub budget_used: u32,
    pub degraded_ticks: u32,
    pub mode: u32,
    pub flags: u32,
}

impl AgentState {
    fn from_ee(a: &EeAgent) -> Self {
        let memory_valid = a.flags & abi::AGENT_FLAG_MEMORY_VALID != 0;
        let memory = if memory_valid {
            Some(Memory {
                bearing: Fx2 {
                    x: a.memory_x,
                    y: a.memory_y,
                },
                age: a.memory_age,
                target: a.cached_target,
            })
        } else {
            None
        };
        let mode = (a.flags & abi::AGENT_MODE_MASK) >> abi::AGENT_MODE_SHIFT;
        Self {
            rng_state: a.rng_state,
            tick_last: a.tick_last,
            memory,
            cached_action: a.cached_action,
            cached_target: a.cached_target,
            budget_used: a.budget_used,
            degraded_ticks: a.degraded_ticks,
            mode,
            flags: a.flags,
        }
    }
}

/// One tick's outcome: the validated proposal and the agent state it left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TickOutcome {
    pub intent: Intent,
    pub agent: AgentState,
    pub context: EeContext,
}

/// The native host.
#[derive(Debug, Clone)]
pub struct WeevilHost {
    ctx: EeContext,
    agents: Vec<EeAgent>,
    initialized: bool,
    max_agents: u32,
    seed: u64,
    kernel_version: EeVersion,
    kernel_fingerprint: u64,
}

impl WeevilHost {
    /// Perform the handshake and allocate the agent slots.
    ///
    /// `capabilities` is the mask the host declares; the kernel degrades along
    /// its documented ladder when a bit is absent. `budget_units` is accepted
    /// for forward compatibility and is **not carried into the ABI**: v0's
    /// `ee_init_desc` has no budget field, so the kernel overwrites the
    /// context's allowance with its own hard-coded 16. Call
    /// [`WeevilHost::budget_units`] for the value that is actually in effect.
    /// `tick_num`/`tick_den` describe the host's tick rate (the kernel records
    /// it; it does not enforce a wall-clock limit).
    pub fn init(
        capabilities: u32,
        max_agents: u32,
        budget_units: u32,
        seed: u64,
        tick_num: u64,
        tick_den: u64,
    ) -> Result<Self> {
        let desc = EeInitDesc {
            struct_size: size_of::<EeInitDesc>() as u32,
            abi_major: abi::ABI_MAJOR,
            abi_minor: abi::ABI_MINOR,
            capabilities,
            max_agents,
            flags: 0,
            host_tick_num: tick_num,
            host_tick_den: tick_den,
            rng_seed: seed,
            abi_fingerprint: abi::ABI_FINGERPRINT,
            reserved0: 0,
        };
        let mut ctx = EeContext::default();
        // SAFETY: `desc` and `ctx` are valid, aligned, owned values that outlive
        // the call; the kernel only reads `desc` and writes `ctx`.
        let status = unsafe { abi::ee_init(&desc, &mut ctx) };
        if status != abi::STATUS_OK {
            return Err(match status {
                abi::STATUS_VERSION_MISMATCH => {
                    let k = unsafe { abi::ee_version_info() };
                    WeevilError::VersionMismatch {
                        host: abi::ABI_MAJOR,
                        kernel: k.major as u32,
                    }
                }
                abi::STATUS_FINGERPRINT_MISMATCH => WeevilError::FingerprintMismatch {
                    host: abi::ABI_FINGERPRINT,
                    kernel: unsafe { abi::ee_abi_fingerprint() },
                },
                abi::STATUS_CAPABILITY_UNSUPPORTED => {
                    WeevilError::CapabilityUnsupported(capabilities)
                }
                abi::STATUS_BUFFER_TOO_SMALL => WeevilError::BufferTooSmall,
                abi::STATUS_BAD_PARAM => WeevilError::BadParam,
                abi::STATUS_PANIC_CAUGHT => WeevilError::PanicCaught,
                other => WeevilError::UnknownStatus(other),
            });
        }

        // SAFETY: both entry points take no arguments and return by value.
        let kernel_version = unsafe { abi::ee_version_info() };
        let kernel_fingerprint = unsafe { abi::ee_abi_fingerprint() };
        if kernel_version.major as u32 != abi::ABI_MAJOR {
            return Err(WeevilError::VersionMismatch {
                host: abi::ABI_MAJOR,
                kernel: kernel_version.major as u32,
            });
        }
        if kernel_fingerprint != abi::ABI_FINGERPRINT {
            return Err(WeevilError::FingerprintMismatch {
                host: abi::ABI_FINGERPRINT,
                kernel: kernel_fingerprint,
            });
        }

        let mut agents = vec![EeAgent::default(); max_agents as usize];
        for (i, a) in agents.iter_mut().enumerate() {
            a.rng_state = seed ^ ((i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        }

        Ok(Self {
            ctx,
            agents,
            initialized: true,
            max_agents,
            seed,
            kernel_version,
            kernel_fingerprint,
        })
    }

    /// The kernel's reported version.
    pub fn kernel_version(&self) -> EeVersion {
        self.kernel_version
    }

    /// The kernel's reported layout fingerprint.
    pub fn kernel_fingerprint(&self) -> u64 {
        self.kernel_fingerprint
    }

    /// The context as the kernel last left it (ticks run, degraded ticks, …).
    pub fn context(&self) -> EeContext {
        self.ctx
    }

    /// The number of agent slots.
    pub fn max_agents(&self) -> u32 {
        self.max_agents
    }

    /// The work allowance the kernel actually put in the context.
    ///
    /// This is what the kernel will degrade against, and in v0 it is the
    /// kernel's own constant rather than the host's declared one — see
    /// [`WeevilHost::init`]. A host that records or compares a budget must read
    /// it from here, not from the profile it declared.
    pub fn budget_units(&self) -> u32 {
        self.ctx.budget_units
    }

    /// The state of one agent, if it exists.
    pub fn agent(&self, id: u32) -> Option<AgentState> {
        self.agents.get(id as usize).map(|a| AgentState::from_ee(a))
    }

    /// All agents, in slot order.
    pub fn agents(&self) -> impl Iterator<Item = (u32, AgentState)> + '_ {
        self.agents
            .iter()
            .enumerate()
            .map(|(i, a)| (i as u32, AgentState::from_ee(a)))
    }

    /// Whether the host has been initialised.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Drop one agent's kernel state and hand it a fresh, deterministic RNG.
    ///
    /// Called when a guard despawns or a run restarts: reusing a predecessor's
    /// memory would let a stale bearing steer a guard that has no business
    /// remembering it.
    pub fn reset_agent(&mut self, id: u32) -> Result<()> {
        let Some(a) = self.agents.get_mut(id as usize) else {
            return Err(WeevilError::AgentNotFound(id));
        };
        *a = EeAgent::default();
        a.rng_state = self.seed ^ ((id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        Ok(())
    }

    /// Reset every agent (a run restart).
    pub fn reset_all(&mut self) {
        for (i, a) in self.agents.iter_mut().enumerate() {
            *a = EeAgent::default();
            a.rng_state = self.seed ^ ((i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        }
        self.ctx.ticks_run = 0;
        self.ctx.degraded_ticks = 0;
    }

    /// Run one tick for one guard.
    ///
    /// Validates the observation (agent liveness, tick continuity, contact
    /// count), calls the kernel, and decodes the proposal. The returned
    /// [`TickOutcome`] is the kernel's *proposal*; the caller decides whether to
    /// accept it and what effect to apply.
    pub fn tick(&mut self, obs: &Observation) -> Result<TickOutcome> {
        if !self.initialized {
            return Err(WeevilError::NotInitialized);
        }
        let id = obs.agent_id;
        if id >= self.max_agents {
            return Err(WeevilError::AgentNotFound(id));
        }
        let slot = id as usize;
        let expected = self.agents[slot].tick_last + 1;
        if obs.tick != expected {
            return Err(WeevilError::StaleTick {
                expected,
                got: obs.tick,
            });
        }

        let snap: EeSnapshot = obs.to_snapshot()?;
        let mut intent = EeIntent::default();
        // SAFETY: `snap` is a valid owned value; `self.ctx` and the agent slot
        // are valid, aligned and live as long as the call; `intent` is a valid
        // out-pointer. The kernel writes the agent and `intent` and returns a
        // status code; it does not retain any of these pointers.
        let status = unsafe {
            abi::ee_tick(
                &mut self.ctx as *mut EeContext,
                &snap as *const EeSnapshot,
                &mut self.agents[slot] as *mut EeAgent,
                &mut intent as *mut EeIntent,
            )
        };
        if status != abi::STATUS_OK {
            return Err(match status {
                abi::STATUS_BAD_PARAM => WeevilError::BadParam,
                abi::STATUS_VERSION_MISMATCH => WeevilError::VersionMismatch {
                    host: abi::ABI_MAJOR,
                    kernel: self.kernel_version.major as u32,
                },
                abi::STATUS_FINGERPRINT_MISMATCH => WeevilError::FingerprintMismatch {
                    host: abi::ABI_FINGERPRINT,
                    kernel: self.kernel_fingerprint,
                },
                abi::STATUS_BUFFER_TOO_SMALL => WeevilError::BufferTooSmall,
                abi::STATUS_PANIC_CAUGHT => WeevilError::PanicCaught,
                abi::STATUS_CAPABILITY_UNSUPPORTED => {
                    WeevilError::CapabilityUnsupported(obs.capability_mask)
                }
                other => WeevilError::UnknownStatus(other),
            });
        }

        let agent = AgentState::from_ee(&self.agents[slot]);
        Ok(TickOutcome {
            intent: Intent::from_ee(&intent)?,
            agent,
            context: self.ctx,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::{FX_ONE, to_fx};

    /// A minimal observation at the given tick.
    fn obs(id: u32, tick: u64) -> Observation {
        Observation {
            agent_id: id,
            tick,
            self_pos: Fx2 { x: to_fx(100.0), y: 0 },
            self_vel: Fx2::ZERO,
            health: FX_ONE,
            ammo: 3 * FX_ONE,
            capability_mask: abi::CAP_NAVMESH | abi::CAP_LINE_OF_SIGHT,
            contacts: Vec::new(),
            cooldown: 0,
        }
    }

    #[test]
    fn a_missing_kernel_is_a_link_failure_not_a_skip() {
        // Compile-time only: the crate links `libevil_weevil`. If the archive is
        // absent the build fails at link time with the archive's name. There is
        // deliberately no fallback path that would let a run proceed without the
        // kernel it claims to drive.
        assert_eq!(size_of::<EeSnapshot>(), 184);
    }

    #[test]
    fn agent_ids_beyond_the_context_are_refused() {
        // Constructed without `init` (which needs the native lib) so the guard
        // paths are exercised on the error side only.
        let host = WeevilHost {
            ctx: EeContext::default(),
            agents: vec![EeAgent::default(); 2],
            initialized: true,
            max_agents: 2,
            seed: 1,
            kernel_version: EeVersion::default(),
            kernel_fingerprint: 0,
        };
        assert_eq!(host.tick(&obs(9, 1)).unwrap_err(), WeevilError::AgentNotFound(9));
    }

    #[test]
    fn a_tick_that_is_not_the_next_one_is_refused() {
        // The same constructed host: `tick_last` starts at zero, so the first
        // observation an agent may carry is tick 1 — not tick 0. This is the
        // contract `HostWorld` relies on (it advances to tick 1 before its
        // first observation) and it is enforced here rather than left to the
        // kernel, which records whatever tick it is given.
        let host = WeevilHost {
            ctx: EeContext::default(),
            agents: vec![EeAgent::default(); 2],
            initialized: true,
            max_agents: 2,
            seed: 1,
            kernel_version: EeVersion::default(),
            kernel_fingerprint: 0,
        };
        assert_eq!(
            host.tick(&obs(0, 0)).unwrap_err(),
            WeevilError::StaleTick { expected: 1, got: 0 }
        );
        assert_eq!(
            host.tick(&obs(0, 7)).unwrap_err(),
            WeevilError::StaleTick { expected: 1, got: 7 }
        );
        // A refused tick must not have advanced the agent's own counter.
        assert_eq!(host.agent(0).unwrap().tick_last, 0);
    }
}

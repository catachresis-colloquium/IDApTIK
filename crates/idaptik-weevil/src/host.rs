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

use crate::abi::{
    self, EeAgent, EeContext, EeInitDesc, EeIntent, EeMind, EeMindTrace, EeModelTrace, EeSnapshot,
    EeVersion,
};
use crate::error::{Result, WeevilError};
use crate::fx::Fx2;
use crate::intent::Intent;
use crate::observation::Observation;
use core::mem::size_of;
use serde::{Deserialize, Serialize};

/// The kernel-owned memory of one contact it no longer sees: a bearing and an
/// age. **Not a position.** The kernel has no fixed-point vector arithmetic to
/// correct a remembered position as the agent moves, so it remembers a direction
/// (ADR-0009). The host must not treat this as a world coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    /// ABI 1.1: one host-owned mind per agent slot, used only by
    /// [`WeevilHost::tick_mind`]. A host that never calls it never reads these.
    minds: Vec<EeMind>,
    mind_fingerprint: u64,
    /// ABI 1.2: the fingerprint the kernel reports for `ee_model_trace`.
    model_fingerprint: u64,
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
        // Accepted and deliberately unused: see the doc comment above.
        let _ = budget_units;
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

        // SAFETY: no arguments, returns by value.
        let mind_fingerprint = unsafe { abi::ee_mind_fingerprint() };
        // SAFETY: no arguments, returns by value.
        let model_fingerprint = unsafe { abi::ee_model_fingerprint() };
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
            minds: vec![fresh_mind(); max_agents as usize],
            mind_fingerprint,
            model_fingerprint,
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

    /// The fingerprint the kernel reports for the 1.1 mind structs.
    pub fn mind_fingerprint(&self) -> u64 {
        self.mind_fingerprint
    }

    /// The fingerprint the kernel reports for the 1.2 model trace.
    pub fn model_fingerprint(&self) -> u64 {
        self.model_fingerprint
    }

    /// One agent's mind, as the kernel last left it. Host-side only: the
    /// reservoirs it holds never go into a trace or a document.
    pub fn mind(&self, id: u32) -> Option<EeMind> {
        self.minds.get(id as usize).copied()
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
        self.agents.get(id as usize).map(AgentState::from_ee)
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
        self.minds[id as usize] = fresh_mind();
        Ok(())
    }

    /// Reset every agent (a run restart).
    pub fn reset_all(&mut self) {
        for (i, a) in self.agents.iter_mut().enumerate() {
            *a = EeAgent::default();
            a.rng_state = self.seed ^ ((i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        }
        for m in &mut self.minds {
            *m = fresh_mind();
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
        let slot = self.check_tick(obs)?;

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
            return Err(self.status_error(status, obs.capability_mask));
        }

        let agent = AgentState::from_ee(&self.agents[slot]);
        Ok(TickOutcome {
            intent: Intent::from_ee(&intent)?,
            agent,
            context: self.ctx,
        })
    }

    /// ABI 1.1: run one tick for one guard through the kernel's mind.
    ///
    /// The same validation as [`WeevilHost::tick`], then `ee_tick_mind` with this
    /// agent's host-owned [`EeMind`] and a 1.1-sized trace record. `damage_source`
    /// is the contact the host attributes this tick's damage to. The host may only
    /// attribute damage to a contact the guard perceives this tick, so an id that
    /// is not in `obs.contacts` is refused rather than passed on.
    ///
    /// The mind's fingerprint is checked on every call, because `ee_init`
    /// checks only the 1.0 one: a kernel whose mind structs differ from this
    /// mirror must not be handed them.
    pub fn tick_mind(
        &mut self,
        obs: &Observation,
        damage_source: Option<u32>,
    ) -> Result<MindOutcome> {
        let (intent, agent, record, _) = self.mind_call(obs, damage_source, &[], false)?;
        Ok(MindOutcome {
            intent,
            agent,
            record,
        })
    }

    /// ABI 1.2: [`WeevilHost::tick_mind`] with the model of the target's
    /// knowledge (evil-weevil ADR-0014).
    ///
    /// `observers` are the contacts the host's world says observe the guard on
    /// this tick: each gets `EE_CONTACT_FLAG_OBSERVES`. Like a damage source, an
    /// observer must be a contact the guard perceives this tick; any other id is
    /// refused. The kernel reads the cue only under `CAP_OBSERVATION_CUES`, so a
    /// host that sets it without declaring the capability gets 1.1 behaviour,
    /// which is how a control differs from its scenario in one input only.
    ///
    /// The trace is the 40-byte `ee_model_trace`, passed by its first member.
    /// The model fingerprint is checked on every call, as the mind's is.
    pub fn tick_model(
        &mut self,
        obs: &Observation,
        damage_source: Option<u32>,
        observers: &[u32],
    ) -> Result<ModelOutcome> {
        let (intent, agent, record, exposure) =
            self.mind_call(obs, damage_source, observers, true)?;
        let exposure = exposure.ok_or_else(|| {
            WeevilError::InvalidMindTrace("a model tick without its exposure fields".into())
        })?;
        Ok(ModelOutcome {
            intent,
            agent,
            record,
            exposure,
        })
    }

    /// The one call behind both mind entrances. With `model` the trace is a
    /// whole `ee_model_trace` and its exposure fields are decoded too.
    fn mind_call(
        &mut self,
        obs: &Observation,
        damage_source: Option<u32>,
        observers: &[u32],
        model: bool,
    ) -> Result<(Intent, AgentState, MindRecord, Option<Exposure>)> {
        let slot = self.check_tick(obs)?;
        if self.mind_fingerprint != abi::MIND_FINGERPRINT {
            return Err(WeevilError::FingerprintMismatch {
                host: abi::MIND_FINGERPRINT,
                kernel: self.mind_fingerprint,
            });
        }
        if model && self.model_fingerprint != abi::MODEL_FINGERPRINT {
            return Err(WeevilError::FingerprintMismatch {
                host: abi::MODEL_FINGERPRINT,
                kernel: self.model_fingerprint,
            });
        }
        let mut snap: EeSnapshot = obs.to_snapshot()?;
        let marks = damage_source
            .into_iter()
            .map(|id| (id, abi::CONTACT_FLAG_DAMAGE_SOURCE))
            .chain(observers.iter().map(|id| (*id, abi::CONTACT_FLAG_OBSERVES)));
        for (id, flag) in marks {
            let Some(i) = obs.contacts.iter().position(|c| c.id == id) else {
                return Err(WeevilError::InvalidTarget(id));
            };
            let slot_contact = match i {
                0 => &mut snap.contact0,
                1 => &mut snap.contact1,
                2 => &mut snap.contact2,
                3 => &mut snap.contact3,
                4 => &mut snap.contact4,
                5 => &mut snap.contact5,
                6 => &mut snap.contact6,
                _ => &mut snap.contact7,
            };
            slot_contact.flags |= flag;
        }
        let mut intent = EeIntent::default();
        let size = if model {
            size_of::<EeModelTrace>()
        } else {
            size_of::<EeMindTrace>()
        };
        let mut trace = EeModelTrace::default();
        trace.mind.struct_size = size as u32;
        // SAFETY: as in `tick`; `self.minds[slot]` and `trace` are valid, owned,
        // aligned values. `trace.mind` is the first member of a whole 40-byte
        // `ee_model_trace`, so whichever size it declares, the kernel writes
        // inside storage this function owns. The kernel retains no pointer.
        let status = unsafe {
            abi::ee_tick_mind(
                &mut self.ctx as *mut EeContext,
                &snap as *const EeSnapshot,
                &mut self.agents[slot] as *mut EeAgent,
                &mut self.minds[slot] as *mut EeMind,
                &mut intent as *mut EeIntent,
                &mut trace.mind as *mut EeMindTrace,
            )
        };
        if status != abi::STATUS_OK {
            return Err(self.status_error(status, obs.capability_mask));
        }
        let record = MindRecord::from_ee(&trace.mind, obs.tick)?;
        let tells = ((intent.flags >> 4) & 0xF) as u8;
        if tells != record.tells {
            return Err(WeevilError::InvalidMindTrace(format!(
                "intent tells {tells:#x} differ from the trace's {:#x}",
                record.tells
            )));
        }
        let exposure = if model {
            Some(Exposure::from_ee(&trace)?)
        } else {
            None
        };
        Ok((
            Intent::from_ee(&intent)?,
            AgentState::from_ee(&self.agents[slot]),
            record,
            exposure,
        ))
    }

    /// The checks both entrances make before calling the kernel: initialised,
    /// a known agent, and the tick exactly one past the agent's last.
    fn check_tick(&self, obs: &Observation) -> Result<usize> {
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
        Ok(slot)
    }

    /// A kernel status other than OK, as this crate's error.
    fn status_error(&self, status: abi::EeStatus, capabilities: u32) -> WeevilError {
        match status {
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
            abi::STATUS_CAPABILITY_UNSUPPORTED => WeevilError::CapabilityUnsupported(capabilities),
            other => WeevilError::UnknownStatus(other),
        }
    }
}

/// A zeroed mind with its `struct_size` set, as the kernel requires.
fn fresh_mind() -> EeMind {
    EeMind {
        struct_size: size_of::<EeMind>() as u32,
        ..Default::default()
    }
}

/// The mode codes the mind ranks, in the kernel's order: five from ADR-0013
/// §8, and stalk, appended by ABI 1.2 (ADR-0014 §4).
pub const MODE_ADVANCE: u8 = 0;
pub const MODE_ENGAGE: u8 = 1;
pub const MODE_EVADE: u8 = 2;
pub const MODE_RELOAD: u8 = 3;
pub const MODE_INVESTIGATE: u8 = 4;
/// ABI 1.2: stalk, the action `TAKE_COVER`. Ranked only under
/// `CAP_OBSERVATION_CUES`, which makes a ranking six long.
pub const MODE_STALK: u8 = 5;

/// One `ee_mind_trace`, decoded and range-checked.
///
/// It is everything the kernel says about one mind tick: a ranking of modes
/// (best first) and the selected and incumbent modes, the affect token with
/// the token it replaced and the input that caused the change, the attention
/// mass, and the tells. No score and no reservoir value are in it, because the
/// kernel does not put them in the trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MindRecord {
    pub tick: u64,
    pub target_id: u32,
    /// Mode codes, best first. One, five or (with stalk ranked) six long.
    pub ranking: Vec<u8>,
    pub selected: u8,
    pub incumbent: u8,
    pub token: u8,
    pub token_prev: u8,
    /// `abi::CAUSE_*`: `CAUSE_NONE` exactly when the token did not change.
    pub cause: u8,
    /// The contact behind the cause, or 0.
    pub cause_contact: u16,
    /// A mass in 1/10 000.
    pub attention: u16,
    /// `abi::TELL_*` bits.
    pub tells: u8,
}

impl MindRecord {
    /// Decode a trace record, refusing any value outside its declared range.
    fn from_ee(t: &EeMindTrace, tick: u64) -> Result<Self> {
        let bad = |what: String| Err(WeevilError::InvalidMindTrace(what));
        if t.tick != tick {
            return bad(format!("trace tick {} for observation tick {tick}", t.tick));
        }
        let count = t.ranking_count as usize;
        if !(1..=6).contains(&count) {
            return bad(format!("ranking_count {count}"));
        }
        let ranking: Vec<u8> = (0..count)
            .map(|i| ((t.ranking >> (4 * i)) & 0xF) as u8)
            .collect();
        for &mode in ranking.iter().chain([t.selected, t.incumbent].iter()) {
            if mode > MODE_STALK {
                return bad(format!("mode code {mode}"));
            }
        }
        if t.token > abi::AFFECT_ANGRY || t.token_prev > abi::AFFECT_ANGRY {
            return bad(format!(
                "affect token {} (previous {})",
                t.token, t.token_prev
            ));
        }
        if t.cause > abi::CAUSE_DECAY {
            return bad(format!("cause {}", t.cause));
        }
        if (t.token != t.token_prev) != (t.cause != abi::CAUSE_NONE) {
            return bad(format!(
                "token {} -> {} with cause {}",
                t.token_prev, t.token, t.cause
            ));
        }
        if t.attention > abi::MASS_FULL {
            return bad(format!("attention {}", t.attention));
        }
        if t.tells & !(abi::TELL_STARTLE | abi::TELL_HESITATE | abi::TELL_FIXATE | abi::TELL_CAUGHT)
            != 0
        {
            return bad(format!("tells {:#x}", t.tells));
        }
        Ok(Self {
            tick: t.tick,
            target_id: t.target_id,
            ranking,
            selected: t.selected,
            incumbent: t.incumbent,
            token: t.token,
            token_prev: t.token_prev,
            cause: t.cause,
            cause_contact: t.cause_contact,
            attention: t.attention,
            tells: t.tells,
        })
    }

    /// Commitment override, derived as the contract derives it: the selected
    /// mode is the incumbent and is not first in the ranking.
    pub fn is_commitment_override(&self) -> bool {
        self.selected == self.incumbent && self.ranking.first() != Some(&self.selected)
    }
}

/// One mind tick's outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct MindOutcome {
    pub intent: Intent,
    pub agent: AgentState,
    pub record: MindRecord,
}

/// ABI 1.2: the exposure fields of one `ee_model_trace`, decoded and
/// range-checked. Exposure is the kernel's one belief about a mind: "a target
/// knows where I am". It is a belief, set by evidence (the host's cue or the
/// agent's own attack) and cleared when its lifetime runs out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exposure {
    /// The belief as the tick opened.
    pub prev: bool,
    /// The belief as the tick closed.
    pub now: bool,
    /// `abi::EXPOSURE_CAUSE_*`: NONE exactly when `prev == now`.
    pub cause: u8,
    /// The contact behind the cause: the observer for a cue, the struck
    /// target for an own attack, 0 otherwise.
    pub contact: u16,
}

impl Exposure {
    /// Decode the fields past the 1.1 prefix, refusing any value outside its
    /// declared range and any change without a cause.
    fn from_ee(t: &EeModelTrace) -> Result<Self> {
        let bad = |what: String| Err(WeevilError::InvalidMindTrace(what));
        if t.mind.struct_size as usize != size_of::<EeModelTrace>() {
            return bad(format!(
                "model trace struct_size {} after the call",
                t.mind.struct_size
            ));
        }
        if t.exposure > 1 || t.exposure_prev > 1 {
            return bad(format!(
                "exposure {} (previous {})",
                t.exposure, t.exposure_prev
            ));
        }
        if t.exposure_cause > abi::EXPOSURE_CAUSE_EXPIRY {
            return bad(format!("exposure cause {}", t.exposure_cause));
        }
        if (t.exposure != t.exposure_prev) != (t.exposure_cause != abi::EXPOSURE_CAUSE_NONE) {
            return bad(format!(
                "exposure {} -> {} with cause {}",
                t.exposure_prev, t.exposure, t.exposure_cause
            ));
        }
        let names_contact = matches!(
            t.exposure_cause,
            abi::EXPOSURE_CAUSE_CUE | abi::EXPOSURE_CAUSE_OWN_ATTACK
        );
        if names_contact != (t.exposure_contact != 0) {
            return bad(format!(
                "exposure cause {} with contact {}",
                t.exposure_cause, t.exposure_contact
            ));
        }
        if t.reserved0 != 0 || t.reserved1 != 0 {
            return bad("reserved model trace bytes are not zero".into());
        }
        Ok(Self {
            prev: t.exposure_prev == 1,
            now: t.exposure == 1,
            cause: t.exposure_cause,
            contact: t.exposure_contact,
        })
    }
}

/// One model tick's outcome: a mind tick plus the exposure belief.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelOutcome {
    pub intent: Intent,
    pub agent: AgentState,
    pub record: MindRecord,
    pub exposure: Exposure,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::FX_ONE;
    use crate::fx::to_fx;

    /// A minimal observation at the given tick.
    fn obs(id: u32, tick: u64) -> Observation {
        Observation {
            agent_id: id,
            tick,
            self_pos: Fx2 {
                x: to_fx(100.0),
                y: 0,
            },
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
        let mut host = WeevilHost {
            ctx: EeContext::default(),
            agents: vec![EeAgent::default(); 2],
            initialized: true,
            max_agents: 2,
            seed: 1,
            kernel_version: EeVersion::default(),
            kernel_fingerprint: 0,
            minds: vec![fresh_mind(); 2],
            mind_fingerprint: 0,
            model_fingerprint: 0,
        };
        assert_eq!(
            host.tick(&obs(9, 1)).unwrap_err(),
            WeevilError::AgentNotFound(9)
        );
    }

    #[test]
    fn a_tick_that_is_not_the_next_one_is_refused() {
        // The same constructed host: `tick_last` starts at zero, so the first
        // observation an agent may carry is tick 1 — not tick 0. This is the
        // contract `HostWorld` relies on (it advances to tick 1 before its
        // first observation) and it is enforced here rather than left to the
        // kernel, which records whatever tick it is given.
        let mut host = WeevilHost {
            ctx: EeContext::default(),
            agents: vec![EeAgent::default(); 2],
            initialized: true,
            max_agents: 2,
            seed: 1,
            kernel_version: EeVersion::default(),
            kernel_fingerprint: 0,
            minds: vec![fresh_mind(); 2],
            mind_fingerprint: 0,
            model_fingerprint: 0,
        };
        assert_eq!(
            host.tick(&obs(0, 0)).unwrap_err(),
            WeevilError::StaleTick {
                expected: 1,
                got: 0
            }
        );
        assert_eq!(
            host.tick(&obs(0, 7)).unwrap_err(),
            WeevilError::StaleTick {
                expected: 1,
                got: 7
            }
        );
        // A refused tick must not have advanced the agent's own counter.
        assert_eq!(host.agent(0).unwrap().tick_last, 0);
    }
}

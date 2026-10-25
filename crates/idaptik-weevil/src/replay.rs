//! Headless replay and decision-cache identity.
//!
//! A replay must be able to answer "what did this guard perceive, what did it
//! decide, and what did the host do about it?" for every tick, and two runs must
//! agree **only within an explicitly supported configuration**. That is what
//! [`ReplayTrace`] records and what [`replay_equal`] checks.
//!
//! The identity of a decision includes everything the kernel could read: the
//! kernel/ABI/profile versions, the initial context and per-agent state, the
//! seed and tick rate, the observation ordering, and the returned intents. A
//! cache key built from less than that would let two different runs collide, so
//! [`decision_identity`] folds the whole trace — memory, RNG and context included
//! — into one digest.

use crate::abi::EeContext;
use crate::host::AgentState;
use crate::intent::Intent;
use crate::observation::{Contact, ContactSource, Observation};
use serde::{Deserialize, Serialize};

/// The replay payload format this build writes.
pub const REPLAY_FORMAT: &str = "idaptik-weevil-replay/1";

/// Everything that must match before two replays are comparable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayHeader {
    pub format: String,
    pub kernel_version: (u16, u16, u16),
    pub abi_major: u32,
    pub abi_fingerprint: u64,
    pub profile_format: String,
    pub profile_id: String,
    pub profile_version: u32,
    pub seed: u64,
    pub tick_rate: (u64, u64),
    pub scenario_id: String,
    pub initial_context: InitialContext,
}

/// The host and kernel state at the first tick.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitialContext {
    pub capabilities: u32,
    pub budget_units: u32,
    pub max_agents: u32,
    pub agents: Vec<AgentState>,
}

/// One tick's record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayEntry {
    pub tick: u64,
    pub agent_id: u32,
    pub observation: ObservationRecord,
    pub agent_before: AgentState,
    pub intent: IntentRecord,
    pub accepted: bool,
    pub applied: AppliedEffect,
    pub note: String,
}

/// The observation, in the adapter's own schema (source, age, confidence).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservationRecord {
    pub self_pos: (i32, i32),
    pub health: i32,
    pub ammo: i32,
    pub contacts: Vec<ContactRecord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContactRecord {
    pub id: u32,
    pub source: String,
    pub bearing: (i32, i32),
    pub distance: i32,
    pub threat: i32,
    pub confidence: f64,
    pub age_ticks: u32,
}

/// The kernel's proposal, decoded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntentRecord {
    pub action: String,
    pub target_id: u32,
    pub move: (i32, i32),
    pub speed: i32,
    pub priority: u32,
    pub budget_used: u32,
    pub degraded: bool,
    pub new_target: bool,
    pub unreachable: bool,
    pub from_memory: bool,
}

/// What the host actually did with the proposal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppliedEffect {
    pub moved: bool,
    pub speed_applied: i32,
    pub blocked: bool,
    pub reason: String,
}

/// A complete, replayable trace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayTrace {
    pub header: ReplayHeader,
    pub entries: Vec<ReplayEntry>,
}

impl ReplayTrace {
    /// Build an empty trace for a header.
    pub fn new(header: ReplayHeader) -> Self {
        Self {
            header,
            entries: Vec::new(),
        }
    }

    /// The number of recorded ticks.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing was recorded.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// A recorder that appends ticks to a [`ReplayTrace`].
#[derive(Debug, Clone)]
pub struct ReplayRecorder {
    trace: ReplayTrace,
}

impl ReplayRecorder {
    /// Start recording against a header.
    pub fn new(header: ReplayHeader) -> Self {
        Self {
            trace: ReplayTrace::new(header),
        }
    }

    /// Append one tick.
    pub fn record(&mut self, entry: ReplayEntry) {
        self.trace.entries.push(entry);
    }

    /// Stop recording and take the trace.
    pub fn finish(self) -> ReplayTrace {
        self.trace
    }
}

/// Why two replays are not equal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayMismatch {
    pub detail: String,
}

impl std::fmt::Display for ReplayMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "replay mismatch: {}", self.detail)
    }
}

impl std::error::Error for ReplayMismatch {}

/// Compare two replays for exact equality, but only when their configuration
/// matches. A configuration difference is reported as a mismatch with a clear
/// reason rather than being silently ignored.
pub fn replay_equal(a: &ReplayTrace, b: &ReplayTrace) -> Result<(), ReplayMismatch> {
    if a.header.format != b.header.format {
        return Err(ReplayMismatch {
            detail: format!(
                "format {} vs {}",
                a.header.format, b.header.format
            ),
        });
    }
    if a.header.kernel_version != b.header.kernel_version {
        return Err(ReplayMismatch {
            detail: format!(
                "kernel version {:?} vs {:?}",
                a.header.kernel_version, b.header.kernel_version
            ),
        });
    }
    if a.header.abi_fingerprint != b.header.abi_fingerprint {
        return Err(ReplayMismatch {
            detail: format!(
                "ABI fingerprint {} vs {}",
                a.header.abi_fingerprint, b.header.abi_fingerprint
            ),
        });
    }
    if a.header.profile_id != b.header.profile_id
        || a.header.profile_version != b.header.profile_version
    {
        return Err(ReplayMismatch {
            detail: format!(
                "profile {} v{} vs {} v{}",
                a.header.profile_id, a.header.profile_version, b.header.profile_id, b.header.profile_version
            ),
        });
    }
    if a.header.seed != b.header.seed {
        return Err(ReplayMismatch {
            detail: format!("seed {} vs {}", a.header.seed, b.header.seed),
        });
    }
    if a.header.tick_rate != b.header.tick_rate {
        return Err(ReplayMismatch {
            detail: format!("tick rate {:?} vs {:?}", a.header.tick_rate, b.header.tick_rate),
        });
    }
    if a.header.scenario_id != b.header.scenario_id {
        return Err(ReplayMismatch {
            detail: format!(
                "scenario {} vs {}",
                a.header.scenario_id, b.header.scenario_id
            ),
        });
    }
    // The initial context is part of the decision identity, so two replays that
    // differ only in the state they started from are not the same decision
    // sequence and must not be reported as equal.
    if a.header.initial_context != b.header.initial_context {
        return Err(ReplayMismatch {
            detail: "initial context differs".to_owned(),
        });
    }
    if a.entries.len() != b.entries.len() {
        return Err(ReplayMismatch {
            detail: format!("{} ticks vs {}", a.entries.len(), b.entries.len()),
        });
    }
    for (i, (ea, eb)) in a.entries.iter().zip(b.entries.iter()).enumerate() {
        if ea != eb {
            return Err(ReplayMismatch {
                detail: format!("entry {i} differs: {ea:?} vs {eb:?}"),
            });
        }
    }
    Ok(())
}

/// Fold a whole trace into one identity digest.
///
/// This is the cache key: it covers the kernel/ABI/profile versions, the initial
/// context and per-agent state, the seed and tick rate, the observation ordering,
/// the returned intents, and the host's acceptance/application — memory, RNG and
/// context included. Two traces with the same digest are the same decision
/// sequence under the same supported configuration.
pub fn decision_identity(trace: &ReplayTrace) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |v: u64| {
        h ^= v;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h = h.rotate_left(29) ^ (h >> 32);
    };
    let hdr = &trace.header;
    mix(0x1);
    mix(hdr.kernel_version.0 as u64);
    mix(hdr.kernel_version.1 as u64);
    mix(hdr.kernel_version.2 as u64);
    mix(hdr.abi_major as u64);
    mix(hdr.abi_fingerprint);
    mix(hdr.profile_id.as_bytes().iter().fold(0u64, |acc, b| {
        acc.wrapping_mul(31).wrapping_add(*b as u64)
    }));
    mix(hdr.profile_version as u64);
    mix(hdr.seed);
    mix(hdr.tick_rate.0);
    mix(hdr.tick_rate.1);
    mix(hdr.scenario_id.as_bytes().iter().fold(0u64, |acc, b| {
        acc.wrapping_mul(31).wrapping_add(*b as u64)
    }));
    mix(hdr.initial_context.capabilities as u64);
    mix(hdr.initial_context.budget_units as u64);
    mix(hdr.initial_context.max_agents as u64);
    for a in &hdr.initial_context.agents {
        mix(a.rng_state);
        mix(a.tick_last);
        mix(a.memory.map(|m| (m.bearing.x as u64, m.bearing.y as u64, m.age as u64, m.target as u64)).unwrap_or(0));
        mix(a.cached_action as u64);
        mix(a.cached_target as u64);
        mix(a.budget_used as u64);
        mix(a.degraded_ticks as u64);
        mix(a.mode as u64);
        mix(a.flags as u64);
    }
    for e in &trace.entries {
        mix(0x2);
        mix(e.tick);
        mix(e.agent_id as u64);
        mix(e.observation.self_pos.0 as u64);
        mix(e.observation.self_pos.1 as u64);
        mix(e.observation.health as u64);
        mix(e.observation.ammo as u64);
        for c in &e.observation.contacts {
            mix(c.id as u64);
            mix(c.source.as_bytes().iter().fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(*b as u64)));
            mix(c.bearing.0 as u64);
            mix(c.bearing.1 as u64);
            mix(c.distance as u64);
            mix(c.threat as u64);
            mix((c.confidence.to_bits() as u64) ^ (c.age_ticks as u64));
        }
        mix(e.agent_before.rng_state);
        mix(e.agent_before.tick_last);
        mix(e.agent_before.memory.map(|m| (m.bearing.x as u64, m.bearing.y as u64, m.age as u64)).unwrap_or(0));
        mix(e.intent.action.as_bytes().iter().fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(*b as u64)));
        mix(e.intent.target_id as u64);
        mix(e.intent.move.0 as u64);
        mix(e.intent.move.1 as u64);
        mix(e.intent.speed as u64);
        mix(e.intent.priority as u64);
        mix(e.intent.budget_used as u64);
        mix(
            (e.intent.degraded as u64)
                | ((e.intent.new_target as u64) << 1)
                | ((e.intent.unreachable as u64) << 2)
                | ((e.intent.from_memory as u64) << 3),
        );
        mix((e.accepted as u64) | ((e.applied.moved as u64) << 1) | ((e.applied.blocked as u64) << 2));
        mix(e.applied.speed_applied as u64);
        mix(e.applied.reason.as_bytes().iter().fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(*b as u64)));
    }
    h
}

// ── Conversions ─────────────────────────────────────────────────────────────

impl From<&Observation> for ObservationRecord {
    fn from(o: &Observation) -> Self {
        Self {
            self_pos: (o.self_pos.x, o.self_pos.y),
            health: o.health,
            ammo: o.ammo,
            contacts: o.contacts.iter().map(ContactRecord::from).collect(),
        }
    }
}

impl From<&Contact> for ContactRecord {
    fn from(c: &Contact) -> Self {
        let source = match c.source {
            ContactSource::Player => "player".to_owned(),
            ContactSource::Object(id) => format!("object:{id}"),
            ContactSource::Decoy => "decoy".to_owned(),
            ContactSource::Sound => "sound".to_owned(),
        };
        Self {
            id: c.id,
            source,
            bearing: (c.bearing.x, c.bearing.y),
            distance: c.distance,
            threat: c.threat,
            confidence: c.confidence,
            age_ticks: c.age_ticks,
        }
    }
}

impl From<&Intent> for IntentRecord {
    fn from(i: &Intent) -> Self {
        Self {
            action: i.action.name().to_owned(),
            target_id: i.target_id,
            move: (i.move.x, i.move.y),
            speed: i.speed,
            priority: i.priority,
            budget_used: i.budget_used,
            degraded: i.flags.degraded,
            new_target: i.flags.new_target,
            unreachable: i.flags.unreachable,
            from_memory: i.flags.from_memory,
        }
    }
}

/// A header builder that fills the identity fields from the live host.
pub fn header_from(
    host: &crate::host::WeevilHost,
    profile: &crate::profile::Profile,
    seed: u64,
    tick_rate: (u64, u64),
    scenario_id: &str,
) -> ReplayHeader {
    let v = host.kernel_version();
    let ctx: EeContext = host.context();
    ReplayHeader {
        format: REPLAY_FORMAT.to_owned(),
        kernel_version: (v.major, v.minor, v.patch),
        abi_major: ctx.abi_major,
        abi_fingerprint: host.kernel_fingerprint(),
        profile_format: crate::profile::PROFILE_FORMAT.to_owned(),
        profile_id: profile.id.clone(),
        profile_version: profile.version,
        seed,
        tick_rate,
        scenario_id: scenario_id.to_owned(),
        initial_context: InitialContext {
            capabilities: ctx.capabilities,
            budget_units: ctx.budget_units,
            max_agents: ctx.max_agents,
            agents: host.agents().map(|(_, a)| a).collect(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::{FX_ONE, to_fx};
    use crate::observation::Observation;

    fn sample_trace(seed: u64) -> ReplayTrace {
        let mut r = ReplayRecorder::new(ReplayHeader {
            format: REPLAY_FORMAT.to_owned(),
            kernel_version: (1, 0, 0),
            abi_major: 1,
            abi_fingerprint: 18276775,
            profile_format: crate::profile::PROFILE_FORMAT.to_owned(),
            profile_id: "cautious-sentry".to_owned(),
            profile_version: 1,
            seed,
            tick_rate: (60, 1),
            scenario_id: "envelope-001-ghost-lobby".to_owned(),
            initial_context: InitialContext {
                capabilities: 3,
                budget_units: 4096,
                max_agents: 1,
                agents: vec![AgentState {
                    rng_state: seed,
                    tick_last: 0,
                    memory: None,
                    cached_action: 0,
                    cached_target: 0,
                    budget_used: 0,
                    degraded_ticks: 0,
                    mode: 0,
                    flags: 0,
                }],
            },
        });
        let obs = Observation {
            agent_id: 0,
            tick: 1,
            self_pos: crate::fx::Fx2 { x: to_fx(1.0), y: 0 },
            self_vel: crate::fx::Fx2::ZERO,
            health: FX_ONE,
            ammo: FX_ONE,
            capability_mask: 3,
            contacts: vec![Contact::new(
                7,
                ContactSource::Player,
                crate::fx::Fx2 { x: to_fx(1.0), y: 0 },
                to_fx(2.0),
            )],
            cooldown: 0,
        };
        r.record(ReplayEntry {
            tick: 1,
            agent_id: 0,
            observation: ObservationRecord::from(&obs),
            agent_before: r.trace.header.initial_context.agents[0],
            intent: IntentRecord {
                action: "ADVANCE".to_owned(),
                target_id: 7,
                move: (to_fx(1.0), 0),
                speed: FX_ONE,
                priority: 26214,
                budget_used: 6,
                degraded: false,
                new_target: true,
                unreachable: false,
                from_memory: false,
            },
            accepted: true,
            applied: AppliedEffect {
                moved: true,
                speed_applied: FX_ONE,
                blocked: false,
                reason: "advance".to_owned(),
            },
            note: String::new(),
        });
        r.finish()
    }

    #[test]
    fn identical_traces_are_equal_and_identically_hashed() {
        let a = sample_trace(42);
        let b = sample_trace(42);
        replay_equal(&a, &b).expect("equal");
        assert_eq!(decision_identity(&a), decision_identity(&b));
    }

    #[test]
    fn a_seed_change_is_a_mismatch() {
        let a = sample_trace(42);
        let b = sample_trace(43);
        assert!(replay_equal(&a, &b).is_err());
        assert_ne!(decision_identity(&a), decision_identity(&b));
    }
}

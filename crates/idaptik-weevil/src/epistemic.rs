//! The guard's beliefs, as an `enaction-runner-output/1` document.
//!
//! Enaction's validator (`enaction-validate`, CAC-KERNEL §9) admits a game
//! runner's output as data, through a process. IDApTIK's ADR-0009 forbids a
//! dependency on any Enaction crate, so this module writes the document's
//! wire shape itself and leaves the checking to the validator.
//!
//! # What is in it, and what is not
//!
//! The document holds one perspective: guard `guard-0`. Nothing in it is
//! invented. It is built from three things the run really computed:
//!
//! * **Observations**, from the host's perception. Every contact the guard
//!   perceived is in the replay with its id, its source and the host's
//!   confidence. Perception needs a shared room, so the contact's room is the
//!   guard's room on that tick.
//! * **Beliefs**, one for each tick on which the kernel set
//!   `EE_INTENT_FLAG_FROM_MEMORY`. On such a tick the kernel acts on a target
//!   it no longer perceives, so it holds that the target is still where it was
//!   last perceived. The kernel's memory is a target and an age, not a
//!   position: its remembered bearing is the ABI's truncated `u16` and reads
//!   `(0, 0)` in the corridor (see [`crate::observation`]). The room in the
//!   belief therefore comes from the last observation of that target, which
//!   is the event the belief cites. A from-memory tick whose target was never
//!   perceived, or whose target is not the kernel's remembered one, is an
//!   error rather than a guess.
//! * **World truth**, from the corridor's own state on each proposition's tick
//!   ([`crate::world::TickTruth`]). It is closed over what the package states
//!   (CAC-KERNEL §9): every proposition a belief or an event states is a truth
//!   atom if and only if the world agrees, and an absent one is false. A belief
//!   whose proposition is missing from `world_truth` is one the guard holds
//!   wrongly; so is an observation of the decoy.
//!
//! `world_truth` is a set of atoms with no tick and no negation, so the tick
//! is part of the proposition: `contact-1 in room-0 at tick 16`. The belief
//! and its truth atom are the same string.
//!
//! The six-stage `CacTrace` in `idaptik-core` is a hard-coded illustration
//! and is not used here.
//!
//! # Confidence and status
//!
//! An event's confidence is the host's confidence in the contact, in the
//! contract's mass units (`10000` is full). A belief's interval is
//! `[c, 10000]`, where `c` is the confidence of the observation it rests on:
//! that much is committed, and nothing in the run argued against it, so the
//! rest is ignorance, not disbelief. The status is the one the caller's policy
//! derives from that interval. The policy is the caller's (it is passed to
//! the validator, never carried in the document); the thresholds below are
//! the ones `tests/fixtures/enaction-policies.json` declares, and a test keeps
//! the two equal.

use crate::canonical::{MAX_SAFE_INTEGER, NotCanonical, sha256_hex, to_jcs};
use crate::error::{Result, WeevilError};
use crate::replay::ReplayTrace;
use crate::world::TickTruth;
use serde::Serialize;
use std::collections::BTreeMap;

/// The envelope's contract tag.
pub const RUNNER_OUTPUT_CONTRACT: &str = "enaction-runner-output/1";
/// The contract's full mass.
pub const MASS_FULL: u16 = 10_000;

/// The status policy the emitter declares statuses under (ADR-0016 order).
pub const STATUS_POLICY_ID: &str = "idaptik-weevil/status/v1";
pub const REJECTED_CEILING: u16 = 1_000;
pub const UNKNOWN_BAND: u16 = 6_000;
pub const BELIEVED_FLOOR: u16 = 7_000;
pub const SUSPECTED_FLOOR: u16 = 5_000;

/// The profile reference the package declares.
pub const PROFILE_ID: &str = "idaptik/weevil-corridor";
/// The guarantee the package declares: the replay driver refuses to write
/// anything unless two runs of the same configuration are equal.
pub const GUARANTEE_REPLAY_DETERMINISTIC: &str = "esm/replay-deterministic";

/// A `{major, minor}` contract version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
}

const V1_0: Version = Version { major: 1, minor: 0 };

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProfileRef {
    pub id: String,
    pub version: Version,
    pub targets_kernel: Version,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Seed {
    Fixed(u64),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Manifest {
    pub kernel_version: Version,
    pub profile: ProfileRef,
    pub schema_version: Version,
    pub source: String,
    pub digest: String,
    pub migration_from: Version,
    pub seed: Seed,
    pub guarantees: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Subject {
    Agent(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Provenance {
    pub origin: String,
    pub channel: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Event {
    pub event_id: u64,
    pub sequence: u64,
    pub tick: u64,
    pub observer: String,
    pub subject: Subject,
    pub kind: String,
    pub proposition: String,
    pub confidence: u16,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Interval {
    pub belief: u16,
    pub plausibility: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Certain,
    Believed,
    Suspected,
    Rejected,
    Unknown,
}

impl Status {
    /// The status the emitter's declared policy derives from an interval, in
    /// the order ADR-0016 makes normative.
    pub fn of(interval: Interval) -> Self {
        let ignorance = interval.plausibility.saturating_sub(interval.belief);
        if interval.belief == MASS_FULL {
            Self::Certain
        } else if interval.plausibility <= REJECTED_CEILING {
            Self::Rejected
        } else if ignorance >= UNKNOWN_BAND {
            Self::Unknown
        } else if interval.belief >= BELIEVED_FLOOR {
            Self::Believed
        } else if interval.plausibility >= SUSPECTED_FLOOR {
            Self::Suspected
        } else {
            Self::Unknown
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Belief {
    pub belief_id: u64,
    pub bearer: String,
    pub proposition: String,
    pub status: Status,
    pub confidence: Interval,
    pub source_events: Vec<u64>,
    pub valid_from: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Package {
    pub manifest: Manifest,
    pub events: Vec<Event>,
    pub beliefs: Vec<Belief>,
}

/// The whole document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunnerOutput {
    pub contract: String,
    pub package: Package,
    pub world_truth: Vec<String>,
}

impl RunnerOutput {
    /// The document's canonical bytes (RFC 8785 JCS), without a newline.
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>> {
        canonical_text(self).map(String::into_bytes)
    }
}

/// `manifest.digest`: lowercase hex SHA-256 of the canonical bytes of the
/// package with that one member omitted, as CAC-KERNEL §6 specifies.
pub fn package_digest(package: &Package) -> Result<String> {
    let mut value = serde_json::to_value(package).map_err(|e| epistemic(e.to_string()))?;
    value["manifest"]
        .as_object_mut()
        .ok_or_else(|| epistemic("the package has no manifest object".into()))?
        .remove("digest");
    let text = to_jcs(&value).map_err(not_canonical)?;
    Ok(sha256_hex(text.as_bytes()))
}

fn canonical_text<T: Serialize>(v: &T) -> Result<String> {
    let value = serde_json::to_value(v).map_err(|e| epistemic(e.to_string()))?;
    to_jcs(&value).map_err(not_canonical)
}

fn epistemic(msg: String) -> WeevilError {
    WeevilError::Epistemic(msg)
}

fn not_canonical(e: NotCanonical) -> WeevilError {
    epistemic(e.to_string())
}

fn agent(id: u32) -> String {
    format!("guard-{id}")
}

fn contact(id: u32) -> String {
    format!("contact-{id}")
}

/// The proposition both a belief and its truth atom carry.
pub fn located(contact_id: u32, room: usize, tick: u64) -> String {
    format!("{} in room-{room} at tick {tick}", contact(contact_id))
}

/// The host's confidence as a mass. The host keeps it as `0.0..=1.0`.
fn mass(confidence: f64) -> Result<u16> {
    if !(0.0..=1.0).contains(&confidence) {
        return Err(epistemic(format!(
            "contact confidence {confidence} is outside 0..=1"
        )));
    }
    Ok((confidence * f64::from(MASS_FULL)).round() as u16)
}

/// Where a provenance channel came from, by the host's contact source.
fn origin(source: &str) -> &'static str {
    if source == "sound" {
        "hearing"
    } else {
        "sight"
    }
}

/// One observation of one contact, before ids are assigned.
#[derive(Debug, Clone)]
struct Sighting {
    tick: u64,
    /// Position in that tick's contact list, for a stable order within a tick.
    slot: usize,
    observer: u32,
    contact: u32,
    room: usize,
    confidence: u16,
    source: String,
    /// Whether the corridor's own state has the contact in `room` on `tick`.
    holds: bool,
}

/// A belief whose source is still a sighting key, before ids are assigned.
struct PendingBelief {
    tick: u64,
    bearer: u32,
    proposition: String,
    interval: Interval,
    source: (u64, usize),
}

/// Build the document for one run.
///
/// `truth` is what [`crate::run_trace_with_truth`] returns beside `trace`:
/// one entry per tick, in the same order.
pub fn runner_output(trace: &ReplayTrace, truth: &[TickTruth]) -> Result<RunnerOutput> {
    if trace.entries.len() != truth.len() {
        return Err(epistemic(format!(
            "{} replay entries but {} truth records",
            trace.entries.len(),
            truth.len()
        )));
    }

    // The latest sighting of each contact, as of the tick being read.
    let mut last_seen: BTreeMap<u32, Sighting> = BTreeMap::new();
    // Cited sightings, keyed by their place in the run.
    let mut cited: BTreeMap<(u64, usize), Sighting> = BTreeMap::new();
    // Beliefs, with sources as sighting keys until ids are assigned.
    let mut pending: Vec<PendingBelief> = Vec::new();
    // True belief propositions, merged with the true event propositions below.
    let mut belief_truth: Vec<String> = Vec::new();

    for (entry, now) in trace.entries.iter().zip(truth) {
        if entry.tick != now.tick {
            return Err(epistemic(format!(
                "replay tick {} paired with truth tick {}",
                entry.tick, now.tick
            )));
        }
        for (slot, c) in entry.observation.contacts.iter().enumerate() {
            last_seen.insert(
                c.id,
                Sighting {
                    tick: entry.tick,
                    slot,
                    observer: entry.agent_id,
                    contact: c.id,
                    room: now.guard_room,
                    confidence: mass(c.confidence)?,
                    source: c.source.clone(),
                    holds: now.contacts.contains(&(c.id, now.guard_room)),
                },
            );
        }

        if !entry.intent.from_memory {
            continue;
        }
        let target = entry.intent.target_id;
        match entry.agent_before.memory {
            Some(m) if m.target == target => {}
            Some(m) => {
                return Err(epistemic(format!(
                    "tick {}: the kernel acted from memory on contact {target} but remembers {}",
                    entry.tick, m.target
                )));
            }
            None => {
                return Err(epistemic(format!(
                    "tick {}: the kernel acted from memory on contact {target} with no memory",
                    entry.tick
                )));
            }
        }
        let Some(seen) = last_seen.get(&target) else {
            return Err(epistemic(format!(
                "tick {}: the kernel acted from memory on contact {target}, which the guard never perceived",
                entry.tick
            )));
        };
        let proposition = located(target, seen.room, entry.tick);
        let interval = Interval {
            belief: seen.confidence,
            plausibility: MASS_FULL,
        };
        let key = (seen.tick, seen.slot);
        cited.entry(key).or_insert_with(|| seen.clone());
        if now.contacts.contains(&(target, seen.room)) {
            belief_truth.push(proposition.clone());
        }
        pending.push(PendingBelief {
            tick: entry.tick,
            bearer: entry.agent_id,
            proposition,
            interval,
            source: key,
        });
    }

    // Events in (tick, slot) order: ids and sequences count from 1 in that
    // order, so (tick, sequence) is strictly increasing.
    let mut ids: BTreeMap<(u64, usize), u64> = BTreeMap::new();
    let mut events = Vec::with_capacity(cited.len());
    let mut event_truth: Vec<String> = Vec::new();
    for (n, (key, s)) in cited.into_iter().enumerate() {
        let id = n as u64 + 1;
        ids.insert(key, id);
        let proposition = located(s.contact, s.room, s.tick);
        if s.holds {
            event_truth.push(proposition.clone());
        }
        events.push(Event {
            event_id: id,
            sequence: id,
            tick: s.tick,
            observer: agent(s.observer),
            subject: Subject::Agent(contact(s.contact)),
            kind: "observation".to_owned(),
            proposition,
            confidence: s.confidence,
            provenance: Provenance {
                origin: origin(&s.source).to_owned(),
                channel: s.source,
            },
        });
    }

    // World truth is closed over what the package states (CAC-KERNEL §9): every
    // stated proposition that the world makes true is an atom, and an absent one
    // is false. A belief and the observation it cites state the same string
    // when the sighting is on the belief's own tick, so the atoms are deduplicated:
    // a repeated atom is `DuplicateTruthAtom`.
    let mut seen_atoms = std::collections::BTreeSet::new();
    let world_truth: Vec<String> = event_truth
        .into_iter()
        .chain(belief_truth)
        .filter(|p| seen_atoms.insert(p.clone()))
        .collect();

    let beliefs = pending
        .into_iter()
        .enumerate()
        .map(|(n, p)| Belief {
            belief_id: n as u64 + 1,
            bearer: agent(p.bearer),
            proposition: p.proposition,
            status: Status::of(p.interval),
            confidence: p.interval,
            source_events: vec![ids[&p.source]],
            valid_from: p.tick,
        })
        .collect();

    let hdr = &trace.header;
    if hdr.seed > MAX_SAFE_INTEGER {
        return Err(epistemic(format!(
            "seed {} is above 2^53 - 1 and has no canonical form",
            hdr.seed
        )));
    }
    let (kmaj, kmin, kpatch) = hdr.kernel_version;
    let mut package = Package {
        manifest: Manifest {
            kernel_version: V1_0,
            profile: ProfileRef {
                id: PROFILE_ID.to_owned(),
                version: V1_0,
                targets_kernel: V1_0,
            },
            schema_version: V1_0,
            source: format!(
                "idaptik-weevil-replay:{}:{}@{}:evil-weevil-{kmaj}.{kmin}.{kpatch}:ticks-{}",
                hdr.scenario_id,
                hdr.profile_id,
                hdr.profile_version,
                trace.entries.len()
            ),
            digest: String::new(),
            migration_from: V1_0,
            seed: Seed::Fixed(hdr.seed),
            guarantees: vec![GUARANTEE_REPLAY_DETERMINISTIC.to_owned()],
        },
        events,
        beliefs,
    };
    package.manifest.digest = package_digest(&package)?;

    Ok(RunnerOutput {
        contract: RUNNER_OUTPUT_CONTRACT.to_owned(),
        package,
        world_truth,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_follows_the_declared_policy_order() {
        let i = |belief, plausibility| Interval {
            belief,
            plausibility,
        };
        assert_eq!(Status::of(i(10_000, 10_000)), Status::Certain);
        assert_eq!(Status::of(i(0, 1_000)), Status::Rejected);
        // Wide ignorance reads Unknown even above the believed floor.
        assert_eq!(Status::of(i(3_000, 10_000)), Status::Unknown);
        assert_eq!(Status::of(i(8_000, 10_000)), Status::Believed);
        assert_eq!(Status::of(i(6_000, 10_000)), Status::Suspected);
        assert_eq!(Status::of(i(2_000, 4_000)), Status::Unknown);
    }

    #[test]
    fn host_confidence_becomes_mass_and_out_of_range_is_refused() {
        assert_eq!(mass(1.0).unwrap(), 10_000);
        assert_eq!(mass(0.6).unwrap(), 6_000);
        assert_eq!(mass(0.8).unwrap(), 8_000);
        assert!(mass(1.5).is_err());
        assert!(mass(f64::NAN).is_err());
    }
}

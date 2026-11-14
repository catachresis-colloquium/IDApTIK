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

use crate::abi;
use crate::canonical::{MAX_SAFE_INTEGER, NotCanonical, sha256_hex, to_jcs};
use crate::error::{Result, WeevilError};
use crate::host::MindRecord;
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

/// One stated event before ids are assigned, with whether the world holds it.
#[derive(Debug, Clone)]
struct Draft {
    tick: u64,
    observer: u32,
    subject: String,
    kind: &'static str,
    proposition: String,
    confidence: u16,
    origin: &'static str,
    channel: String,
    /// Whether the corridor's own state makes `proposition` true.
    holds: bool,
}

/// An event's place in the run: `(tick, class, slot)`. Within a tick,
/// sightings (class 0, by their position in the observation) come before the
/// inputs the mind reported (class 1 onwards), so ids follow (tick, sequence).
type DraftKey = (u64, u8, usize);

const CLASS_SIGHTING: u8 = 0;
const CLASS_DAMAGE: u8 = 1;
const CLASS_NOT_FOUND: u8 = 2;
const CLASS_NOTHING_VISIBLE: u8 = 3;
const CLASS_LOW_HEALTH: u8 = 4;
const CLASS_NO_AMMO: u8 = 5;

/// A belief whose source is still a draft key, before ids are assigned.
struct PendingBelief {
    tick: u64,
    bearer: u32,
    proposition: String,
    interval: Interval,
    source: DraftKey,
}

/// Build the document for one run through `ee_tick`.
///
/// `truth` is what [`crate::run_trace_with_truth`] returns beside `trace`:
/// one entry per tick, in the same order.
pub fn runner_output(trace: &ReplayTrace, truth: &[TickTruth]) -> Result<RunnerOutput> {
    runner_output_with_causes(trace, truth, &[]).map(|(doc, _)| doc)
}

/// Build the document for one run, and, for a run through `ee_tick_mind`, the
/// events behind every affect change the kernel reported.
///
/// `mind` is the run's mind records ([`crate::CorridorRun::mind`]), one per
/// tick, or empty. The second value holds, for each record, the ids of the
/// events in this document that `caused_by` must name: empty exactly when the
/// token did not change. Each cause the kernel reports (`cause` and
/// `cause_contact`) is mapped to the input that produced it:
///
/// * `CLOSE_CONTACT`: the sighting of `cause_contact` on that tick.
/// * `THREATS`: the sightings of every contact perceived on that tick.
/// * `DAMAGE`: an `action_outcome`, `guard-0 took damage at tick T`.
/// * `ATTRIBUTED_DAMAGE`: an `action_outcome`,
///   `contact-X damaged guard-0 at tick T`, with X the attributed contact.
/// * `CONTRADICTION`: an observation, `contact-X not found at tick T`, with X
///   `cause_contact`, or when that is 0 the target of the kernel's own 1.0
///   memory before the tick. A lead with no target is an error.
/// * `LOW_HEALTH` and `NO_AMMO`: observations of the guard's own state.
/// * `DECAY`: the guard's perception on that tick, which brought no input:
///   its sightings, or `no contact visible to guard-0 at tick T` when it saw
///   nothing.
///
/// Every event's proposition is decided against the corridor's truth for its
/// tick, and is a truth atom exactly when it holds.
pub fn runner_output_with_causes(
    trace: &ReplayTrace,
    truth: &[TickTruth],
    mind: &[MindRecord],
) -> Result<(RunnerOutput, Vec<Vec<u64>>)> {
    if trace.entries.len() != truth.len() {
        return Err(epistemic(format!(
            "{} replay entries but {} truth records",
            trace.entries.len(),
            truth.len()
        )));
    }
    if !mind.is_empty() && mind.len() != truth.len() {
        return Err(epistemic(format!(
            "{} mind records for {} ticks",
            mind.len(),
            truth.len()
        )));
    }

    // Every sighting of the run, so a belief or a cause can cite one.
    let mut sightings: BTreeMap<(u64, u32), (DraftKey, Draft)> = BTreeMap::new();
    // The latest sighting key of each contact, as of the tick being read.
    let mut last_seen: BTreeMap<u32, (u64, u32)> = BTreeMap::new();
    let mut cited: BTreeMap<DraftKey, Draft> = BTreeMap::new();
    let mut pending: Vec<PendingBelief> = Vec::new();
    let mut cause_keys: Vec<Vec<DraftKey>> = Vec::with_capacity(mind.len());

    for (i, (entry, now)) in trace.entries.iter().zip(truth).enumerate() {
        if entry.tick != now.tick {
            return Err(epistemic(format!(
                "replay tick {} paired with truth tick {}",
                entry.tick, now.tick
            )));
        }
        let tick = entry.tick;
        let guard = entry.agent_id;
        for (slot, c) in entry.observation.contacts.iter().enumerate() {
            let draft = Draft {
                tick,
                observer: guard,
                subject: contact(c.id),
                kind: "observation",
                proposition: located(c.id, now.guard_room, tick),
                confidence: mass(c.confidence)?,
                origin: origin(&c.source),
                channel: c.source.clone(),
                holds: now.contacts.contains(&(c.id, now.guard_room)),
            };
            sightings.insert((tick, c.id), ((tick, CLASS_SIGHTING, slot), draft));
            last_seen.insert(c.id, (tick, c.id));
        }
        let sighting = |id: u32| sightings.get(&(tick, id)).cloned();

        if entry.intent.from_memory {
            let target = entry.intent.target_id;
            match entry.agent_before.memory {
                Some(m) if m.target == target => {}
                Some(m) => {
                    return Err(epistemic(format!(
                        "tick {tick}: the kernel acted from memory on contact {target} but remembers {}",
                        m.target
                    )));
                }
                None => {
                    return Err(epistemic(format!(
                        "tick {tick}: the kernel acted from memory on contact {target} with no memory"
                    )));
                }
            }
            let Some(seen) = last_seen
                .get(&target)
                .and_then(|k| sightings.get(k))
                .cloned()
            else {
                return Err(epistemic(format!(
                    "tick {tick}: the kernel acted from memory on contact {target}, which the guard never perceived"
                )));
            };
            let (key, draft) = seen;
            // The room of the sighting the memory rests on.
            let room = room_of(&draft.proposition);
            let proposition = located(target, room, tick);
            let interval = Interval {
                belief: draft.confidence,
                plausibility: MASS_FULL,
            };
            cited.entry(key).or_insert(draft);
            pending.push(PendingBelief {
                tick,
                bearer: guard,
                proposition,
                interval,
                source: key,
            });
        }

        let Some(record) = mind.get(i) else {
            continue;
        };
        if record.tick != tick {
            return Err(epistemic(format!(
                "mind record for tick {} at replay tick {tick}",
                record.tick
            )));
        }
        let mut keys = Vec::new();
        let mut cite_perception = |keys: &mut Vec<DraftKey>| {
            for c in &entry.observation.contacts {
                let (key, draft) = sighting(c.id).expect("every perceived contact was recorded");
                cited.entry(key).or_insert(draft);
                keys.push(key);
            }
        };
        let own =
            |class: u8, kind: &'static str, proposition: String, origin, channel: &str, holds| {
                (
                    (tick, class, 0),
                    Draft {
                        tick,
                        observer: guard,
                        subject: agent(guard),
                        kind,
                        proposition,
                        confidence: MASS_FULL,
                        origin,
                        channel: channel.to_owned(),
                        holds,
                    },
                )
            };
        let mut extra: Vec<(DraftKey, Draft)> = Vec::new();
        match record.cause {
            abi::CAUSE_NONE => {}
            abi::CAUSE_CLOSE_CONTACT => {
                let id = u32::from(record.cause_contact);
                let Some((key, draft)) = sighting(id) else {
                    return Err(epistemic(format!(
                        "tick {tick}: close contact {id} is not in the guard's observation"
                    )));
                };
                cited.entry(key).or_insert(draft);
                keys.push(key);
            }
            abi::CAUSE_THREATS => {
                if entry.observation.contacts.is_empty() {
                    return Err(epistemic(format!(
                        "tick {tick}: a threat cause with nothing perceived"
                    )));
                }
                cite_perception(&mut keys);
            }
            abi::CAUSE_DAMAGE => extra.push(own(
                CLASS_DAMAGE,
                "action_outcome",
                format!("{} took damage at tick {tick}", agent(guard)),
                "damage",
                "health",
                !now.hits.is_empty(),
            )),
            abi::CAUSE_ATTRIBUTED_DAMAGE => {
                let id = u32::from(record.cause_contact);
                if id == 0 {
                    return Err(epistemic(format!(
                        "tick {tick}: attributed damage with no contact"
                    )));
                }
                let (key, mut draft) = own(
                    CLASS_DAMAGE,
                    "action_outcome",
                    format!("{} damaged {} at tick {tick}", contact(id), agent(guard)),
                    "damage",
                    "health",
                    now.hits.contains(&id),
                );
                draft.subject = contact(id);
                extra.push((key, draft));
            }
            abi::CAUSE_CONTRADICTION => {
                let id = match record.cause_contact {
                    0 => entry
                        .agent_before
                        .memory
                        .map(|m| m.target)
                        .filter(|t| *t != 0),
                    c => Some(u32::from(c)),
                };
                let Some(id) = id else {
                    return Err(epistemic(format!(
                        "tick {tick}: a lead came to nothing, but the kernel names no contact and holds no memory"
                    )));
                };
                let (key, mut draft) = own(
                    CLASS_NOT_FOUND,
                    "observation",
                    format!("{} not found at tick {tick}", contact(id)),
                    "sight",
                    "lead-expired",
                    !now.visible.contains(&id)
                        && !now
                            .contacts
                            .iter()
                            .any(|(c, r)| *c == id && *r == now.guard_room),
                );
                draft.subject = contact(id);
                extra.push((key, draft));
            }
            abi::CAUSE_LOW_HEALTH => extra.push(own(
                CLASS_LOW_HEALTH,
                "observation",
                format!("{} health below half at tick {tick}", agent(guard)),
                "proprioception",
                "health",
                now.guard_health < abi::FX_ONE / 2,
            )),
            abi::CAUSE_NO_AMMO => extra.push(own(
                CLASS_NO_AMMO,
                "observation",
                format!("{} out of ammo at tick {tick}", agent(guard)),
                "proprioception",
                "ammo",
                now.guard_ammo == 0,
            )),
            abi::CAUSE_DECAY => {
                if entry.observation.contacts.is_empty() {
                    extra.push(own(
                        CLASS_NOTHING_VISIBLE,
                        "observation",
                        format!("no contact visible to {} at tick {tick}", agent(guard)),
                        "sight",
                        "none",
                        !now.contacts.iter().any(|(_, r)| *r == now.guard_room),
                    ));
                } else {
                    cite_perception(&mut keys);
                }
            }
            other => {
                return Err(epistemic(format!("tick {tick}: unknown cause {other}")));
            }
        }
        for (key, draft) in extra {
            cited.entry(key).or_insert(draft);
            keys.push(key);
        }
        cause_keys.push(keys);
    }

    // Ids and sequences count from 1 in (tick, class, slot) order, so
    // (tick, sequence) is strictly increasing.
    let mut ids: BTreeMap<DraftKey, u64> = BTreeMap::new();
    let mut events = Vec::with_capacity(cited.len());
    // World truth is closed over what the package states (CAC-KERNEL §9):
    // every stated proposition the world makes true is an atom, and an absent
    // one is false. A belief and the sighting it cites state one string when
    // they share a tick, so the atoms are deduplicated: a repeated atom is
    // `DuplicateTruthAtom`.
    let mut seen_atoms = std::collections::BTreeSet::new();
    let mut world_truth: Vec<String> = Vec::new();
    for (n, (key, d)) in cited.into_iter().enumerate() {
        let id = n as u64 + 1;
        ids.insert(key, id);
        if d.holds && seen_atoms.insert(d.proposition.clone()) {
            world_truth.push(d.proposition.clone());
        }
        events.push(Event {
            event_id: id,
            sequence: id,
            tick: d.tick,
            observer: agent(d.observer),
            subject: Subject::Agent(d.subject),
            kind: d.kind.to_owned(),
            proposition: d.proposition,
            confidence: d.confidence,
            provenance: Provenance {
                origin: d.origin.to_owned(),
                channel: d.channel,
            },
        });
    }

    let mut beliefs = Vec::with_capacity(pending.len());
    for (n, p) in pending.into_iter().enumerate() {
        let (_, tick) = (p.source, p.tick);
        let (target, room) = target_room(&p.proposition);
        let at = truth
            .iter()
            .find(|t| t.tick == tick)
            .expect("a truth record for every tick");
        if at.contacts.contains(&(target, room)) && seen_atoms.insert(p.proposition.clone()) {
            world_truth.push(p.proposition.clone());
        }
        beliefs.push(Belief {
            belief_id: n as u64 + 1,
            bearer: agent(p.bearer),
            proposition: p.proposition,
            status: Status::of(p.interval),
            confidence: p.interval,
            source_events: vec![ids[&p.source]],
            valid_from: tick,
        });
    }
    let causes = cause_keys
        .into_iter()
        .map(|keys| keys.iter().map(|k| ids[k]).collect())
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

    Ok((
        RunnerOutput {
            contract: RUNNER_OUTPUT_CONTRACT.to_owned(),
            package,
            world_truth,
        },
        causes,
    ))
}

/// The room index in a `located` proposition this module wrote.
fn room_of(proposition: &str) -> usize {
    target_room(proposition).1
}

/// `(contact, room)` from a `located` proposition this module wrote.
fn target_room(proposition: &str) -> (u32, usize) {
    let parse = || -> Option<(u32, usize)> {
        let rest = proposition.strip_prefix("contact-")?;
        let (id, rest) = rest.split_once(" in room-")?;
        let (room, _) = rest.split_once(" at tick ")?;
        Some((id.parse().ok()?, room.parse().ok()?))
    };
    parse().expect("a located proposition written by this module")
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

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
//! # Models (ABI 1.2, slice 2)
//!
//! A run through the 1.2 model record (a mind run of a `cues` script) also
//! states what a contact believes about the guard and what the guard thinks
//! the contact believes. Both are about one proposition, the guard's
//! position, written without a tick (`guard-0 in room-0`), because time is
//! carried by `valid_from` and `supersedes` so that one model can outlive a
//! change of the contact's own belief. The corridor's model scenarios keep the
//! guard in one room for the whole run, so the world decides that proposition
//! once; a run in which the guard changes room is refused rather than given a
//! truth atom it cannot decide (CAC-KERNEL §9).
//!
//! * **The contact's own belief** (order 0, `bearer` the contact) follows the
//!   world's rule ([`crate::world::knows`]): a `certain` belief from the tick
//!   it comes to know, citing the contact's observation of the guard or the
//!   blow it took; superseded by an `unknown` one (`[0, 10000]`, as
//!   `conformance/tom/v1` does) from the tick it stops knowing, citing
//!   `contact-X does not observe guard-0 at tick T`. Never restated in between.
//! * **The guard's model** (order 1, `holders = ["guard-0"]`, the same
//!   bearer and proposition) follows the kernel's exposure. It is `certain`
//!   (`[10000, 10000]`) while exposure is true and `rejected` (`[0, 0]`) while
//!   it is false. The first model is stated on the first tick the kernel
//!   makes a claim (it has a target, or it is exposed) and cites the guard's
//!   perception of the bearer then. After that a model is stated only on a
//!   tick where exposure changed, superseding the one before, and cites the
//!   cause: a cue as the guard's observation `contact-X observes guard-0 at
//!   tick T`, its own attack as the action outcome `guard-0 attacked contact-X
//!   at tick T`, and expiry as decay is cited, by the guard's perception on
//!   that tick. A run without `CAP_OBSERVATION_CUES` states no model: the
//!   kernel makes no claim.
//! * **World truth** is closed over the order-0 propositions and the events,
//!   as before. A model is never compared with world truth, so no atom is
//!   added for a model as such.
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
use crate::host::{MindRecord, ModelRecord};
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
    /// The ascription path, outermost first; empty (and omitted) for the
    /// bearer's own belief. `["guard-0"]` is the guard's model of the bearer.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub holders: Vec<String>,
    pub bearer: String,
    pub proposition: String,
    pub status: Status,
    pub confidence: Interval,
    pub source_events: Vec<u64>,
    pub valid_from: u64,
    /// The belief this one ends, when it is a later state of the same mind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<u64>,
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
    /// Who received the evidence: the guard, or for a contact's own evidence
    /// about the guard, the contact.
    observer: String,
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
// Slice 2: the guard's perception of a cue, its own attack, and the contact's
// own evidence about the guard (slot: the contact id).
const CLASS_CUE: u8 = 6;
const CLASS_OWN_ATTACK: u8 = 7;
const CLASS_CONTACT_SEES: u8 = 8;
const CLASS_CONTACT_STRUCK: u8 = 9;
const CLASS_CONTACT_LOSES: u8 = 10;

/// An order-0 belief of a contact or an order-1 model of it, before ids are
/// assigned. `chain` names the mind it belongs to, so each one supersedes the
/// previous one of its chain.
struct PendingModel {
    tick: u64,
    /// 0 for the contact's own belief, 1 for the guard's model of it: the
    /// order within a tick, and the order of the belief.
    order: u8,
    holder: Option<u32>,
    bearer: u32,
    interval: Interval,
    sources: Vec<DraftKey>,
}

const HELD: Interval = Interval {
    belief: MASS_FULL,
    plausibility: MASS_FULL,
};
const NOT_HELD_MODEL: Interval = Interval {
    belief: 0,
    plausibility: 0,
};
const NOT_KNOWN: Interval = Interval {
    belief: 0,
    plausibility: MASS_FULL,
};

/// The guard's position, the one proposition a contact's belief and the
/// guard's model of it share. No tick: time is in `valid_from`.
pub fn positioned(agent_id: u32, room: usize) -> String {
    format!("{} in room-{room}", agent(agent_id))
}

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
    runner_output_with_models(trace, truth, mind, &[])
}

/// [`runner_output_with_causes`] for a run through the 1.2 model record:
/// `model` holds one record per tick, and the document also states the
/// contacts' own beliefs about the guard and the guard's models of them (see
/// the module docs). With `model` empty it is exactly
/// [`runner_output_with_causes`].
pub fn runner_output_with_models(
    trace: &ReplayTrace,
    truth: &[TickTruth],
    mind: &[MindRecord],
    model: &[ModelRecord],
) -> Result<(RunnerOutput, Vec<Vec<u64>>)> {
    if !model.is_empty() && model.len() != mind.len() {
        return Err(epistemic(format!(
            "{} model records for {} mind records",
            model.len(),
            mind.len()
        )));
    }
    // Whether the kernel makes a claim at all: without the capability it
    // reads no cue and writes no exposure.
    let claims = trace.header.initial_context.capabilities & abi::CAP_OBSERVATION_CUES != 0;
    let mut models: Vec<PendingModel> = Vec::new();
    // The contacts whose knowledge the run states, and whether each knew on
    // the tick before.
    let mut knew: BTreeMap<u32, bool> = BTreeMap::new();
    // The bearer of the guard's current model, once it has one.
    let mut modelled: Option<u32> = None;
    // The guard's room on every tick of a model run, which must not change.
    let mut rooms = std::collections::BTreeSet::new();
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
                observer: agent(guard),
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
                        observer: agent(guard),
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

        let Some(m) = model.get(i) else {
            continue;
        };
        rooms.insert(now.guard_room);
        let event = |class: u8,
                     slot: u32,
                     observer: String,
                     subject: String,
                     kind: &'static str,
                     proposition: String,
                     origin: &'static str,
                     channel: &str,
                     holds: bool| {
            (
                (tick, class, slot as usize),
                Draft {
                    tick,
                    observer,
                    subject,
                    kind,
                    proposition,
                    confidence: MASS_FULL,
                    origin,
                    channel: channel.to_owned(),
                    holds,
                },
            )
        };

        // The contacts' own beliefs, by the world's rule. A contact's events
        // have the contact as observer; `contact()` and `agent()` name the
        // two sides.
        let mut here: Vec<u32> = now
            .observers
            .iter()
            .chain(&now.struck)
            .chain(&now.knowers)
            .copied()
            .collect();
        here.extend(knew.keys().copied());
        here.sort_unstable();
        here.dedup();
        for c in here {
            let knows_now = crate::world::knows(now, c);
            let before = knew.get(&c).copied().unwrap_or(false);
            knew.insert(c, knows_now);
            if knows_now == before {
                continue;
            }
            let mut sources = Vec::new();
            let mut push = |(key, draft): (DraftKey, Draft)| {
                cited.entry(key).or_insert(draft);
                sources.push(key);
            };
            if knows_now {
                if now.observers.contains(&c) {
                    push(event(
                        CLASS_CONTACT_SEES,
                        c,
                        contact(c),
                        agent(guard),
                        "observation",
                        positioned(guard, now.guard_room),
                        "sight",
                        "gaze",
                        true,
                    ));
                }
                if now.struck.contains(&c) {
                    push(event(
                        CLASS_CONTACT_STRUCK,
                        c,
                        contact(c),
                        agent(guard),
                        "action_outcome",
                        format!("{} attacked {} at tick {tick}", agent(guard), contact(c)),
                        "damage",
                        "attack",
                        true,
                    ));
                }
            } else {
                push(event(
                    CLASS_CONTACT_LOSES,
                    c,
                    contact(c),
                    agent(guard),
                    "observation",
                    format!(
                        "{} does not observe {} at tick {tick}",
                        contact(c),
                        agent(guard)
                    ),
                    "sight",
                    "gaze",
                    !now.observers.contains(&c),
                ));
            }
            models.push(PendingModel {
                tick,
                order: 0,
                holder: None,
                bearer: c,
                interval: if knows_now { HELD } else { NOT_KNOWN },
                sources,
            });
        }

        // The guard's model, by the kernel's exposure.
        if !claims {
            if m.exposure || m.changed() {
                return Err(epistemic(format!(
                    "tick {tick}: exposure without CAP_OBSERVATION_CUES"
                )));
            }
            continue;
        }
        let first = modelled.is_none() && (m.exposure || record.target_id != 0);
        if !first && !(modelled.is_some() && m.changed()) {
            continue;
        }
        let mut sources = Vec::new();
        let bearer = match m.cause {
            abi::EXPOSURE_CAUSE_CUE | abi::EXPOSURE_CAUSE_OWN_ATTACK => {
                let c = u32::from(m.contact);
                if c == 0 {
                    return Err(epistemic(format!(
                        "tick {tick}: exposure cause {} names no contact",
                        m.cause
                    )));
                }
                let (key, draft) = if m.cause == abi::EXPOSURE_CAUSE_CUE {
                    event(
                        CLASS_CUE,
                        c,
                        agent(guard),
                        contact(c),
                        "observation",
                        format!("{} observes {} at tick {tick}", contact(c), agent(guard)),
                        "sight",
                        "gaze",
                        now.observers.contains(&c),
                    )
                } else {
                    event(
                        CLASS_OWN_ATTACK,
                        c,
                        agent(guard),
                        contact(c),
                        "action_outcome",
                        format!("{} attacked {} at tick {tick}", agent(guard), contact(c)),
                        "damage",
                        "attack",
                        now.struck.contains(&c),
                    )
                };
                cited.entry(key).or_insert(draft);
                sources.push(key);
                c
            }
            abi::EXPOSURE_CAUSE_EXPIRY => {
                let Some(c) = modelled else {
                    return Err(epistemic(format!(
                        "tick {tick}: exposure expired before any model"
                    )));
                };
                // ADR-0027's recorded limit, as for decay: the kernel reports
                // no input, so the guard's perception on that tick is cited.
                if entry.observation.contacts.is_empty() {
                    let (key, draft) = own(
                        CLASS_NOTHING_VISIBLE,
                        "observation",
                        format!("no contact visible to {} at tick {tick}", agent(guard)),
                        "sight",
                        "none",
                        !now.contacts.iter().any(|(_, r)| *r == now.guard_room),
                    );
                    cited.entry(key).or_insert(draft);
                    sources.push(key);
                } else {
                    for seen in &entry.observation.contacts {
                        let (key, draft) =
                            sighting(seen.id).expect("every perceived contact was recorded");
                        cited.entry(key).or_insert(draft);
                        sources.push(key);
                    }
                }
                c
            }
            abi::EXPOSURE_CAUSE_NONE => {
                // The first claim, with nothing changed: the guard sees a
                // target and has no cue. It rests on that sighting.
                let c = record.target_id;
                let Some((key, draft)) = sighting(c) else {
                    return Err(epistemic(format!(
                        "tick {tick}: the kernel's first claim is about contact {c}, which the guard does not perceive"
                    )));
                };
                cited.entry(key).or_insert(draft);
                sources.push(key);
                c
            }
            other => {
                return Err(epistemic(format!(
                    "tick {tick}: unknown exposure cause {other}"
                )));
            }
        };
        modelled = Some(bearer);
        models.push(PendingModel {
            tick,
            order: 1,
            holder: Some(guard),
            bearer,
            interval: if m.exposure { HELD } else { NOT_HELD_MODEL },
            sources,
        });
    }
    if !models.is_empty() && rooms.len() > 1 {
        return Err(epistemic(format!(
            "the guard is in rooms {rooms:?} during the run: a position without a tick cannot be decided"
        )));
    }
    let room = rooms.first().copied().unwrap_or(0);

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
            observer: d.observer,
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
            supersedes: None,
            holders: Vec::new(),
        });
    }
    // The contacts' own beliefs and the guard's models of them, by tick, own
    // belief before model; each supersedes the last of its own chain.
    models.sort_by_key(|p| (p.tick, p.order, p.bearer));
    let mut last: BTreeMap<(Option<u32>, u32), u64> = BTreeMap::new();
    for p in models {
        let belief_id = beliefs.len() as u64 + 1;
        let proposition = positioned(trace.entries[0].agent_id, room);
        // An order-0 proposition is decided by the world, whatever the
        // bearer's status: the guard is in `room` on every tick, so it holds.
        // A model adds no atom.
        if p.order == 0 && seen_atoms.insert(proposition.clone()) {
            world_truth.push(proposition.clone());
        }
        beliefs.push(Belief {
            belief_id,
            holders: p.holder.map(agent).into_iter().collect(),
            bearer: contact(p.bearer),
            proposition,
            status: Status::of(p.interval),
            confidence: p.interval,
            source_events: p.sources.iter().map(|k| ids[k]).collect(),
            valid_from: p.tick,
            supersedes: last.insert((p.holder, p.bearer), belief_id),
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

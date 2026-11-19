//! The guard's mind, as an `enaction-mind-trace/1` document.
//!
//! Evil Weevil ABI 1.1 (ADR-0013) writes one `ee_mind_trace` for every
//! `ee_tick_mind` call. This module turns a run's records into the contract's
//! document (CAC-KERNEL §10), paired with the `enaction-runner-output/2`
//! document of the same run. As with that one, ADR-0009 forbids a dependency on
//! any Enaction crate: the shape is written here and checked by the validator.
//!
//! What the document carries, all of it from the kernel's record:
//!
//! * one record per agent per tick;
//! * the affect token, the attention mass, and `caused_by`: when the token
//!   differs from the one the tick opened with, the ids of the events in the
//!   paired runner output that the kernel's `cause` names (see
//!   [`crate::epistemic::runner_output_with_causes`]); otherwise none;
//! * the ranking of candidates, best first, the incumbent and the selected
//!   candidate, and the target when the decision has one;
//! * the tells.
//!
//! It carries no score and no reservoir value, because the kernel's record has
//! none: the ranking is an order, the attention is a quantised mass, and the
//! reservoirs stay in the host-owned `ee_mind`. Commitment override is not
//! asserted: a reader derives it from `ranking`, `incumbent` and `selected`.
//!
//! The vocabularies are declared in the kernel's own order: tokens by
//! `EE_AFFECT_*`, candidates by mode code (advance 0, engage 1, evade 2,
//! reload 3, investigate 4), tells by `EE_TELL_*` bit. A run through the 1.2
//! model record (evil-weevil ADR-0014) declares the two terms 1.2 adds, in the
//! same order: the candidate `stalk` (mode 5) and the tell `caught` (bit 3).
//! A 1.1 run declares the 1.1 vocabulary, whatever kernel ran it.

use crate::CorridorRun;
use crate::abi;
use crate::canonical::to_jcs;
use crate::epistemic::{PROFILE_ID, RunnerOutput, runner_output_with_models};
use crate::error::{Result, WeevilError};
use crate::host::MindRecord;
use serde::Serialize;

/// The document's contract tag.
pub const MIND_TRACE_CONTRACT: &str = "enaction-mind-trace/1";

/// Affect tokens, indexed by `EE_AFFECT_*`.
pub const AFFECT_TOKENS: [&str; 4] = ["calm", "wary", "afraid", "angry"];
/// Candidates, indexed by the kernel's mode code.
pub const CANDIDATES: [&str; 5] = ["advance", "engage", "evade", "reload", "investigate"];
/// Tells, in `EE_TELL_*` bit order.
pub const TELLS: [(u8, &str); 3] = [
    (abi::TELL_STARTLE, "startle"),
    (abi::TELL_HESITATE, "hesitate"),
    (abi::TELL_FIXATE, "fixate"),
];
/// The candidates of a run through the 1.2 model record: 1.1's, then stalk.
pub const MODEL_CANDIDATES: [&str; 6] = [
    "advance",
    "engage",
    "evade",
    "reload",
    "investigate",
    "stalk",
];
/// The tells of a run through the 1.2 model record: 1.1's, then caught.
pub const MODEL_TELLS: [(u8, &str); 4] = [
    (abi::TELL_STARTLE, "startle"),
    (abi::TELL_HESITATE, "hesitate"),
    (abi::TELL_FIXATE, "fixate"),
    (abi::TELL_CAUGHT, "caught"),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Producer {
    pub kernel: String,
    pub abi: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProfileRef {
    pub id: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Vocabulary {
    pub affect_tokens: Vec<String>,
    pub candidates: Vec<String>,
    pub tells: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Affect {
    pub token: String,
    pub attention: u16,
    pub caused_by: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Conation {
    pub ranking: Vec<String>,
    pub incumbent: String,
    pub selected: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TickRecord {
    pub tick: u64,
    pub agent: String,
    pub affect: Affect,
    pub conation: Conation,
    pub tells: Vec<String>,
}

/// The whole document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MindTrace {
    pub contract: String,
    pub producer: Producer,
    pub profile: ProfileRef,
    pub vocabulary: Vocabulary,
    pub ticks: Vec<TickRecord>,
}

impl MindTrace {
    /// The document's canonical bytes (RFC 8785 JCS), without a newline.
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>> {
        let value = serde_json::to_value(self).map_err(|e| mind(e.to_string()))?;
        to_jcs(&value)
            .map(String::into_bytes)
            .map_err(|e| mind(e.to_string()))
    }
}

fn mind(msg: String) -> WeevilError {
    WeevilError::Epistemic(msg)
}

fn candidate(candidates: &[&str], mode: u8) -> Result<String> {
    candidates
        .get(usize::from(mode))
        .map(|s| (*s).to_owned())
        .ok_or_else(|| mind(format!("mode code {mode} has no candidate")))
}

fn token(code: u8) -> Result<String> {
    AFFECT_TOKENS
        .get(usize::from(code))
        .map(|s| (*s).to_owned())
        .ok_or_else(|| mind(format!("affect code {code} has no token")))
}

/// Both documents of one run through the mind: the runner output, with the
/// events every affect change cites, and the mind trace that cites them.
pub fn documents(run: &CorridorRun) -> Result<(RunnerOutput, MindTrace)> {
    if run.mind.len() != run.trace.entries.len() {
        return Err(mind(format!(
            "{} mind records for {} ticks: the run did not go through ee_tick_mind",
            run.mind.len(),
            run.trace.entries.len()
        )));
    }
    let (runner, causes) =
        runner_output_with_models(&run.trace, &run.truth, &run.mind, &run.model)?;
    let hdr = &run.trace.header;
    let (candidates, tells): (&[&str], &[(u8, &str)]) = if run.model.is_empty() {
        (&CANDIDATES, &TELLS)
    } else {
        (&MODEL_CANDIDATES, &MODEL_TELLS)
    };
    let mut ticks = Vec::with_capacity(run.mind.len());
    for ((entry, record), caused_by) in run.trace.entries.iter().zip(&run.mind).zip(causes) {
        ticks.push(tick_record(
            entry.agent_id,
            record,
            caused_by,
            candidates,
            tells,
        )?);
    }
    let trace = MindTrace {
        contract: MIND_TRACE_CONTRACT.to_owned(),
        producer: Producer {
            kernel: "evil-weevil".to_owned(),
            abi: format!("{}.{}", hdr.kernel_version.0, hdr.kernel_version.1),
        },
        profile: ProfileRef {
            id: PROFILE_ID.to_owned(),
            version: "1.0".to_owned(),
        },
        vocabulary: Vocabulary {
            affect_tokens: AFFECT_TOKENS.iter().map(|s| (*s).to_owned()).collect(),
            candidates: candidates.iter().map(|s| (*s).to_owned()).collect(),
            tells: tells.iter().map(|(_, s)| (*s).to_owned()).collect(),
        },
        ticks,
    };
    Ok((runner, trace))
}

fn tick_record(
    agent: u32,
    r: &MindRecord,
    caused_by: Vec<u64>,
    candidates: &[&str],
    tells: &[(u8, &str)],
) -> Result<TickRecord> {
    let changed = r.token != r.token_prev;
    if changed == caused_by.is_empty() {
        return Err(mind(format!(
            "tick {}: token {} -> {} with {} cited events",
            r.tick,
            r.token_prev,
            r.token,
            caused_by.len()
        )));
    }
    Ok(TickRecord {
        tick: r.tick,
        agent: format!("guard-{agent}"),
        affect: Affect {
            token: token(r.token)?,
            attention: r.attention,
            caused_by,
        },
        conation: Conation {
            ranking: r
                .ranking
                .iter()
                .map(|m| candidate(candidates, *m))
                .collect::<Result<_>>()?,
            incumbent: candidate(candidates, r.incumbent)?,
            selected: candidate(candidates, r.selected)?,
            target: (r.target_id != 0).then(|| format!("contact-{}", r.target_id)),
        },
        tells: tells
            .iter()
            .filter(|(bit, _)| r.tells & bit != 0)
            .map(|(_, name)| (*name).to_owned())
            .collect(),
    })
}

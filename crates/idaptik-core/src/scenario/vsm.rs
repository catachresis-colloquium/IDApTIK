//! First VSM-shaped supervision model for NPC teams and adaptive difficulty.
//!
//! This is an experiment, not yet part of the Ghost Lobby simulation. It keeps
//! the useful VSM roles explicit and deterministic so the model can be tested
//! before it is connected to gameplay events.

use super::Event;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Functional VSM roles. These are roles, not required processes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VsmSystem {
    Operations,
    Coordination,
    Regulation,
    Audit,
    Intelligence,
    Policy,
}

/// An NPC that can receive a team order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operator {
    pub id: String,
    pub role: String,
    pub readiness: u8,
}

/// A recursive operational unit: a team is made from operators and may itself
/// be an operator at a higher faction/session level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorTeam {
    pub id: String,
    pub operators: Vec<Operator>,
    pub attention: u8,
}

/// An observation about a player's tactic. It is deliberately a hypothesis
/// input, not a claim that the player has a fixed identity or strategy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerTacticObservation {
    pub tactic_id: String,
    pub occurrence: u32,
    pub confidence: u8,
}

/// A bounded adaptation proposed by the intelligence function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DifficultyProposal {
    pub from: u8,
    pub to: u8,
    pub reason: String,
    pub tactic_id: String,
}

/// A bounded, provenance-bearing hypothesis. Confidence is represented as
/// basis points (`0..=1000`) rather than floating point so replay is exact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hypothesis {
    pub proposition: String,
    pub confidence: u16,
    pub supporting_events: Vec<String>,
    pub contrary_events: Vec<String>,
}

/// Evidence updates one or more competing explanations. The ledger never sees
/// hidden world truth; callers provide only evidence available to the observer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HypothesisEvidence {
    pub event_id: String,
    pub support: Vec<(String, u16)>,
    pub contrary: Vec<(String, u16)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HypothesisLedger {
    pub hypotheses: Vec<Hypothesis>,
}

pub const MASS_TOTAL: u32 = 10_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocalMass {
    pub hypotheses: Vec<String>,
    pub mass: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceLedger {
    pub frame: Vec<String>,
    pub focal_masses: Vec<FocalMass>,
    pub conflict_mass: u32,
}

impl EvidenceLedger {
    pub fn from_evidence(frame: Vec<String>, focal_masses: Vec<FocalMass>) -> Self {
        let mut ledger = Self {
            frame,
            focal_masses: vec![],
            conflict_mass: 0,
        };
        for focal in focal_masses {
            ledger.add_focal(focal);
        }
        ledger
    }

    fn add_focal(&mut self, mut focal: FocalMass) {
        focal.hypotheses.sort();
        focal.hypotheses.dedup();
        if focal.mass == 0 {
            return;
        }
        if let Some(existing) = self
            .focal_masses
            .iter_mut()
            .find(|f| f.hypotheses == focal.hypotheses)
        {
            existing.mass = existing.mass.saturating_add(focal.mass).min(MASS_TOTAL);
        } else {
            self.focal_masses.push(focal);
        }
        self.focal_masses
            .sort_by(|a, b| a.hypotheses.cmp(&b.hypotheses));
    }

    pub fn combine_conjunctive(&self, other: &Self) -> Self {
        let mut result = Self {
            frame: self.frame.clone(),
            focal_masses: vec![],
            conflict_mass: self.conflict_mass.saturating_add(other.conflict_mass),
        };
        for left in &self.focal_masses {
            for right in &other.focal_masses {
                let intersection: Vec<String> = left
                    .hypotheses
                    .iter()
                    .filter(|h| right.hypotheses.binary_search(h).is_ok())
                    .cloned()
                    .collect();
                let mass = ((left.mass as u64 * right.mass as u64) / MASS_TOTAL as u64) as u32;
                if intersection.is_empty() {
                    result.conflict_mass = result.conflict_mass.saturating_add(mass);
                } else {
                    result.add_focal(FocalMass {
                        hypotheses: intersection,
                        mass,
                    });
                }
            }
        }
        result
    }

    pub fn combine_in_place(&mut self, evidence: Self) {
        *self = self.combine_conjunctive(&evidence);
    }

    pub fn belief(&self, proposition: &[String]) -> u32 {
        self.focal_masses
            .iter()
            .filter(|f| {
                f.hypotheses
                    .iter()
                    .all(|h| proposition.binary_search(h).is_ok())
            })
            .map(|f| f.mass)
            .sum()
    }

    pub fn plausibility(&self, proposition: &[String]) -> u32 {
        self.focal_masses
            .iter()
            .filter(|f| {
                f.hypotheses
                    .iter()
                    .any(|h| proposition.binary_search(h).is_ok())
            })
            .map(|f| f.mass)
            .sum()
    }

    /// Convert an evidence query into a bounded present-time attention value.
    /// Policy stays outside the ledger: this is the first VSM regulation seam.
    pub fn recommended_attention(&self, proposition: &[String]) -> u8 {
        (self.plausibility(proposition).saturating_mul(100) / MASS_TOTAL).min(100) as u8
    }
}

/// The evidence frame's discernible coverage targets, in declaration order —
/// the tie-break order for target selection.
pub const COVERAGE_TARGETS: [&str; 2] = ["usb", "fridge_note"];

/// The most plausible coverage target under `evidence`, ties resolving to
/// declaration order — the first entry of [`COVERAGE_TARGETS`] wins.
///
/// Declaration order is the whole point: it is a stable, platform-independent
/// tie-break, exactly as [`HypothesisLedger::most_likely`] promises for
/// hypotheses. `Iterator::max_by_key` cannot be used here — it returns the
/// *last* maximum, which would silently hand every tie to the last-declared
/// target while the surrounding contract claims otherwise.
pub fn most_plausible_target(evidence: &EvidenceLedger) -> &'static str {
    let mut best = COVERAGE_TARGETS[0];
    let mut best_attention = evidence.recommended_attention(&[best.to_owned()]);
    for candidate in &COVERAGE_TARGETS[1..] {
        let attention = evidence.recommended_attention(&[(*candidate).to_owned()]);
        if attention > best_attention {
            best_attention = attention;
            best = candidate;
        }
    }
    best
}

/// The tactic id the supervisor counts when infrastructure is sabotaged.
pub const TACTIC_INFRA_SABOTAGE: &str = "infrastructure_sabotage";

/// Whether an event is intruder-shaped infrastructure sabotage (ADR-0013):
/// a valve deliberately opened, equipment shorting out, an alarm tripped, or
/// a hosted mole chewing a cable. A pump failing open is *not* sabotage —
/// nobody's hand is on it — but its consequences (`InfraShorted`, a tripped
/// alarm) are still counted when they arrive as their own events. These are
/// observations of the building, not of the player: the supervisor never
/// learns where anyone is from them.
pub fn is_infra_sabotage(event: &Event) -> bool {
    match event {
        Event::WaterReleased {
            cause: crate::scenario::infra::ReleaseCause::Valve,
            ..
        }
        | Event::InfraShorted { .. }
        | Event::AlarmChanged { sounding: true, .. } => true,
        Event::Mole { mole } => {
            matches!(mole, crate::companion::MoleEvent::CableSabotaged { .. })
        }
        _ => false,
    }
}

/// Translate existing Ghost Lobby events into observer-relative evidence.
pub fn ghost_lobby_evidence(event: &Event, _event_id: &str) -> Option<EvidenceLedger> {
    let frame = vec!["usb".into(), "fridge_note".into(), "unknown".into()];
    let focal = match event {
        Event::UsbThrown | Event::UsbTaken { seen: true } | Event::BillyTookUsb => FocalMass {
            hypotheses: vec!["usb".into()],
            mass: 2_500,
        },
        Event::NoteExposed | Event::NoteSecured { seen: true } | Event::BillyTookNote => {
            FocalMass {
                hypotheses: vec!["fridge_note".into()],
                mass: 2_500,
            }
        }
        _ => return None,
    };
    Some(EvidenceLedger::from_evidence(
        frame.clone(),
        vec![
            focal,
            FocalMass {
                hypotheses: frame,
                mass: 7_500,
            },
        ],
    ))
}

/// The Ghost Lobby security team's supervisor. Part of the deterministic
/// runtime state (`RuntimeState::supervision`) when the run is supervised: the
/// sim folds its own canonical event stream through `ingest`, so every path —
/// TUI, headless, networked seats — replicates the same supervision state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GhostLobbySupervisor {
    pub director: VsmDirector,
    pub evidence: Option<EvidenceLedger>,
    pub team: OperatorTeam,
    pub coverage_target: Option<String>,
    pub observed_events: u64,
}

impl GhostLobbySupervisor {
    pub fn new(team_id: impl Into<String>) -> Self {
        Self {
            director: VsmDirector::new(0, 1),
            evidence: None,
            team: OperatorTeam {
                id: team_id.into(),
                operators: vec![],
                attention: 0,
            },
            coverage_target: None,
            observed_events: 0,
        }
    }

    /// Fold a batch of sim events into the evidence picture, then re-allocate
    /// attention — but only when the batch actually carried evidence, so a
    /// steady picture neither reallocates every tick nor grows the director's
    /// trace unboundedly (the trace now lives inside snapshots).
    ///
    /// Frontend-only events (`ContextHint`, `TutorialCue`) are skipped: they
    /// are excluded from the canonical determinism diff, so counting them
    /// would let two frontends disagree about `observed_events`.
    pub fn ingest(&mut self, events: &[Event]) {
        let mut evidence_changed = false;
        for event in events {
            if matches!(event, Event::ContextHint { .. } | Event::TutorialCue { .. }) {
                continue;
            }
            self.observed_events = self.observed_events.saturating_add(1);
            let Some(next) = ghost_lobby_evidence(event, &self.observed_events.to_string()) else {
                continue;
            };
            evidence_changed = true;
            if let Some(current) = &mut self.evidence {
                current.combine_in_place(next);
            } else {
                self.evidence = Some(next);
            }
        }
        // Sabotage is a tactic the supervisor can *count* even where it
        // carries no objective evidence: released water, arcing shorts,
        // tripped alarms and chewed cables are intruder-shaped. Occurrence
        // numbers are derived from the director's own count, so every seat
        // folds the same event stream into the same trace, and a repeated
        // tactic still escalates through the ordinary bounded proposal path.
        for event in events {
            if !is_infra_sabotage(event) {
                continue;
            }
            let occurrence = self
                .director
                .observations
                .get(TACTIC_INFRA_SABOTAGE)
                .copied()
                .unwrap_or(0)
                + 1;
            let proposal = self.director.observe(PlayerTacticObservation {
                tactic_id: TACTIC_INFRA_SABOTAGE.to_owned(),
                occurrence,
                confidence: 75,
            });
            if let Some(proposal) = proposal {
                self.director.apply(proposal);
            }
        }
        if !evidence_changed {
            return;
        }
        if let Some(evidence) = &self.evidence {
            let target = [most_plausible_target(evidence).to_owned()];
            self.director
                .allocate_from_evidence(&mut self.team, evidence, &target);
            if self.team.attention > 0 {
                self.coverage_target = target.into_iter().next();
            }
        }
    }
}

/// The supervision half of the runtime state: the supervisor plus the
/// announce-once latches that keep its allocation visible in the canonical
/// event log without re-announcing every tick.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisionState {
    pub supervisor: GhostLobbySupervisor,
    /// Last announced attention (0 = never announced, matching the initial
    /// allocation of 0).
    pub announced_attention: u8,
    /// Last announced coverage target.
    pub announced_target: Option<String>,
}

impl SupervisionState {
    pub fn new(team_id: impl Into<String>) -> Self {
        Self {
            supervisor: GhostLobbySupervisor::new(team_id),
            announced_attention: 0,
            announced_target: None,
        }
    }
}

impl HypothesisLedger {
    pub fn new(propositions: impl IntoIterator<Item = String>) -> Self {
        Self {
            hypotheses: propositions
                .into_iter()
                .map(|proposition| Hypothesis {
                    proposition,
                    confidence: 500,
                    supporting_events: Vec::new(),
                    contrary_events: Vec::new(),
                })
                .collect(),
        }
    }

    pub fn observe(&mut self, evidence: HypothesisEvidence) {
        for (proposition, amount) in evidence.support {
            if let Some(hypothesis) = self
                .hypotheses
                .iter_mut()
                .find(|candidate| candidate.proposition == proposition)
            {
                hypothesis.confidence = hypothesis.confidence.saturating_add(amount).min(1000);
                hypothesis.supporting_events.push(evidence.event_id.clone());
            }
        }
        for (proposition, amount) in evidence.contrary {
            if let Some(hypothesis) = self
                .hypotheses
                .iter_mut()
                .find(|candidate| candidate.proposition == proposition)
            {
                hypothesis.confidence = hypothesis.confidence.saturating_sub(amount);
                hypothesis.contrary_events.push(evidence.event_id.clone());
            }
        }
    }

    /// Ties resolve to declaration order, making the result stable across
    /// platforms and independent of map iteration order.
    pub fn most_likely(&self) -> Option<&Hypothesis> {
        self.hypotheses
            .iter()
            .max_by_key(|hypothesis| hypothesis.confidence)
    }
}

/// Events make supervision and adaptation visible in a replayable trace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum VsmEvent {
    PlayerTacticObserved {
        tactic_id: String,
        occurrence: u32,
        confidence: u8,
    },
    TeamAttentionAllocated {
        team_id: String,
        attention: u8,
    },
    DifficultyProposalGenerated(DifficultyProposal),
    DifficultyPolicyChecked {
        accepted: bool,
        target: u8,
    },
    AdaptiveInterventionApplied {
        from: u8,
        to: u8,
        tactic_id: String,
    },
    AuditRecorded {
        system: VsmSystem,
        message: String,
    },
}

/// Small deterministic director for a first adaptive-difficulty experiment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VsmDirector {
    pub current_difficulty: u8,
    pub maximum_difficulty: u8,
    pub observations: BTreeMap<String, u32>,
    pub trace: Vec<VsmEvent>,
}

impl VsmDirector {
    pub fn new(current_difficulty: u8, maximum_difficulty: u8) -> Self {
        Self {
            current_difficulty: current_difficulty.min(maximum_difficulty),
            maximum_difficulty,
            observations: BTreeMap::new(),
            trace: Vec::new(),
        }
    }

    /// Record an observation and produce a proposal once the tactic repeats.
    /// The threshold is intentionally simple and will move into a profile later.
    pub fn observe(&mut self, observation: PlayerTacticObservation) -> Option<DifficultyProposal> {
        let count = self
            .observations
            .entry(observation.tactic_id.clone())
            .or_insert(0);
        *count = (*count).max(observation.occurrence);
        self.trace.push(VsmEvent::PlayerTacticObserved {
            tactic_id: observation.tactic_id.clone(),
            occurrence: *count,
            confidence: observation.confidence.min(100),
        });

        if *count < 2 || observation.confidence < 60 {
            return None;
        }

        let target = self
            .current_difficulty
            .saturating_add(1)
            .min(self.maximum_difficulty);
        Some(DifficultyProposal {
            from: self.current_difficulty,
            to: target,
            reason: "repeated tactic with sufficient confidence".into(),
            tactic_id: observation.tactic_id,
        })
    }

    /// Apply a proposal only inside the declared policy envelope.
    pub fn apply(&mut self, proposal: DifficultyProposal) -> bool {
        let accepted = proposal.from == self.current_difficulty
            && proposal.to >= proposal.from
            && proposal.to <= self.maximum_difficulty;
        self.trace.push(VsmEvent::DifficultyPolicyChecked {
            accepted,
            target: proposal.to,
        });
        if !accepted {
            self.trace.push(VsmEvent::AuditRecorded {
                system: VsmSystem::Audit,
                message: "difficulty proposal rejected by policy".into(),
            });
            return false;
        }
        self.current_difficulty = proposal.to;
        self.trace
            .push(VsmEvent::DifficultyProposalGenerated(proposal.clone()));
        self.trace.push(VsmEvent::AdaptiveInterventionApplied {
            from: proposal.from,
            to: proposal.to,
            tactic_id: proposal.tactic_id,
        });
        true
    }

    /// Allocate team attention as a bounded present-time regulation action.
    pub fn allocate_attention(&mut self, team: &mut OperatorTeam, attention: u8) {
        team.attention = attention.min(100);
        self.trace.push(VsmEvent::TeamAttentionAllocated {
            team_id: team.id.clone(),
            attention: team.attention,
        });
    }

    pub fn allocate_from_evidence(
        &mut self,
        team: &mut OperatorTeam,
        ledger: &EvidenceLedger,
        proposition: &[String],
    ) {
        self.allocate_attention(team, ledger.recommended_attention(proposition));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_confident_tactic_proposes_bounded_intervention() {
        let mut director = VsmDirector::new(1, 2);
        assert!(
            director
                .observe(PlayerTacticObservation {
                    tactic_id: "hidden-route".into(),
                    occurrence: 1,
                    confidence: 80,
                })
                .is_none()
        );
        let proposal = director
            .observe(PlayerTacticObservation {
                tactic_id: "hidden-route".into(),
                occurrence: 2,
                confidence: 80,
            })
            .expect("repeat should produce a proposal");
        assert_eq!(proposal.to, 2);
        assert!(director.apply(proposal));
        assert_eq!(director.current_difficulty, 2);
    }

    #[test]
    fn policy_rejects_stale_or_over_limit_proposals() {
        let mut director = VsmDirector::new(1, 2);
        assert!(!director.apply(DifficultyProposal {
            from: 0,
            to: 2,
            reason: "stale".into(),
            tactic_id: "route".into(),
        }));
        assert!(!director.apply(DifficultyProposal {
            from: 1,
            to: 3,
            reason: "over limit".into(),
            tactic_id: "route".into(),
        }));
    }

    #[test]
    fn attention_is_bounded_and_traceable() {
        let mut director = VsmDirector::new(0, 1);
        let mut team = OperatorTeam {
            id: "patrol-a".into(),
            operators: vec![],
            attention: 0,
        };
        director.allocate_attention(&mut team, 120);
        assert_eq!(team.attention, 100);
        assert!(matches!(
            director.trace.last(),
            Some(VsmEvent::TeamAttentionAllocated { attention: 100, .. })
        ));
    }

    #[test]
    fn ledger_can_hold_a_rational_but_wrong_usb_hypothesis() {
        let mut ledger =
            HypothesisLedger::new(["player_wants_usb".into(), "player_wants_fridge_note".into()]);
        ledger.observe(HypothesisEvidence {
            event_id: "usb-interest-1".into(),
            support: vec![("player_wants_usb".into(), 180)],
            contrary: vec![],
        });
        ledger.observe(HypothesisEvidence {
            event_id: "usb-interest-2".into(),
            support: vec![("player_wants_usb".into(), 180)],
            contrary: vec![],
        });
        assert_eq!(
            ledger.most_likely().unwrap().proposition,
            "player_wants_usb"
        );

        // The player actually wanted the note, but the observer has not seen
        // that fact. Later evidence can revise the belief without erasing the
        // original supporting events.
        ledger.observe(HypothesisEvidence {
            event_id: "fridge-note-found".into(),
            support: vec![("player_wants_fridge_note".into(), 420)],
            contrary: vec![("player_wants_usb".into(), 260)],
        });
        assert_eq!(
            ledger.most_likely().unwrap().proposition,
            "player_wants_fridge_note"
        );
        assert_eq!(ledger.hypotheses[0].supporting_events.len(), 2);
        assert_eq!(
            ledger.hypotheses[0].contrary_events,
            vec!["fridge-note-found"]
        );
    }

    #[test]
    fn evidence_fixture_preserves_unknown_and_conflict() {
        let frame = vec!["front_door".into(), "ventilation".into(), "unknown".into()];
        let first = EvidenceLedger::from_evidence(
            frame.clone(),
            vec![
                FocalMass {
                    hypotheses: vec!["ventilation".into()],
                    mass: 4_000,
                },
                FocalMass {
                    hypotheses: vec!["front_door".into(), "ventilation".into()],
                    mass: 4_000,
                },
                FocalMass {
                    hypotheses: vec!["front_door".into(), "ventilation".into(), "unknown".into()],
                    mass: 2_000,
                },
            ],
        );
        let second = EvidenceLedger::from_evidence(
            frame,
            vec![
                FocalMass {
                    hypotheses: vec!["front_door".into()],
                    mass: 7_000,
                },
                FocalMass {
                    hypotheses: vec!["front_door".into(), "ventilation".into(), "unknown".into()],
                    mass: 3_000,
                },
            ],
        );
        let combined = first.combine_conjunctive(&second);
        let front = vec!["front_door".into()];
        assert!(combined.plausibility(&front) >= combined.belief(&front));
        assert!(combined.conflict_mass > 0);
    }

    #[test]
    fn evidence_fixture_supports_usb_then_note_revision() {
        let frame = vec!["usb".into(), "fridge_note".into(), "unknown".into()];
        let usb = EvidenceLedger::from_evidence(
            frame.clone(),
            vec![
                FocalMass {
                    hypotheses: vec!["usb".into()],
                    mass: 6_000,
                },
                FocalMass {
                    hypotheses: frame.clone(),
                    mass: 4_000,
                },
            ],
        );
        let note = EvidenceLedger::from_evidence(
            frame.clone(),
            vec![
                FocalMass {
                    hypotheses: vec!["fridge_note".into()],
                    mass: 7_000,
                },
                FocalMass {
                    hypotheses: frame,
                    mass: 3_000,
                },
            ],
        );
        let revised = usb.combine_conjunctive(&note);
        assert!(revised.conflict_mass > 0);
        assert!(revised.plausibility(&["fridge_note".into()]) > 0);
    }

    #[test]
    fn ghost_lobby_deception_loop_reallocates_patrol_attention() {
        let frame = vec!["front_door".into(), "ventilation".into(), "unknown".into()];
        let staged_vent = EvidenceLedger::from_evidence(
            frame.clone(),
            vec![
                FocalMass {
                    hypotheses: vec!["ventilation".into()],
                    mass: 7_000,
                },
                FocalMass {
                    hypotheses: frame.clone(),
                    mass: 3_000,
                },
            ],
        );
        assert_eq!(
            staged_vent.recommended_attention(&["ventilation".into()]),
            100
        );

        let direct_front = EvidenceLedger::from_evidence(
            frame.clone(),
            vec![
                FocalMass {
                    hypotheses: vec!["front_door".into()],
                    mass: 8_000,
                },
                FocalMass {
                    hypotheses: frame,
                    mass: 2_000,
                },
            ],
        );
        assert!(direct_front.recommended_attention(&["front_door".into()]) > 70);
        assert!(staged_vent.recommended_attention(&["front_door".into()]) < 70);
    }

    #[test]
    fn evidence_updates_operator_team_and_trace() {
        let frame = vec!["front_door".into(), "ventilation".into(), "unknown".into()];
        let ledger = EvidenceLedger::from_evidence(
            frame,
            vec![
                FocalMass {
                    hypotheses: vec!["ventilation".into()],
                    mass: 7_000,
                },
                FocalMass {
                    hypotheses: vec!["front_door".into(), "ventilation".into(), "unknown".into()],
                    mass: 3_000,
                },
            ],
        );
        let mut director = VsmDirector::new(0, 1);
        let mut team = OperatorTeam {
            id: "patrol-a".into(),
            operators: vec![],
            attention: 0,
        };
        director.allocate_from_evidence(&mut team, &ledger, &["ventilation".into()]);
        assert_eq!(team.attention, 100);
        assert!(
            matches!(director.trace.last(), Some(VsmEvent::TeamAttentionAllocated { team_id, attention: 100 }) if team_id == "patrol-a")
        );
    }

    #[test]
    fn ghost_lobby_events_feed_the_evidence_ledger() {
        let evidence =
            ghost_lobby_evidence(&Event::UsbThrown, "usb-1").expect("USB event evidence");
        assert!(evidence.plausibility(&["usb".into()]) > 0);
        assert!(
            ghost_lobby_evidence(&Event::LightsFlickered { third_use: false }, "noise").is_none()
        );
    }

    #[test]
    fn a_tie_resolves_to_the_declared_first_target() {
        // A frame-wide mass alone makes every target equally plausible. The
        // contract (and WORKPLAN) promise declaration order, so `usb` — first
        // in COVERAGE_TARGETS — must win. `max_by_key` returns the LAST
        // maximum and handed these ties to `fridge_note`.
        let frame: Vec<String> = COVERAGE_TARGETS
            .iter()
            .map(|t| (*t).to_owned())
            .chain(std::iter::once("unknown".to_owned()))
            .collect();
        let vacuous = EvidenceLedger::from_evidence(
            frame.clone(),
            vec![FocalMass {
                hypotheses: frame,
                mass: MASS_TOTAL,
            }],
        );
        for target in COVERAGE_TARGETS {
            assert_eq!(
                vacuous.recommended_attention(&[target.to_owned()]),
                100,
                "{target} is equally plausible under a vacuous ledger"
            );
        }
        assert_eq!(most_plausible_target(&vacuous), COVERAGE_TARGETS[0]);
    }

    #[test]
    fn a_clear_winner_beats_declaration_order() {
        // Ties aside, plausibility decides: a thrown USB really does outcompete
        // the note, and a note-heavy picture really does take the target back.
        let frame = vec!["usb".into(), "fridge_note".into(), "unknown".into()];
        let note_heavy = EvidenceLedger::from_evidence(
            frame.clone(),
            vec![FocalMass {
                hypotheses: vec!["fridge_note".into()],
                mass: MASS_TOTAL,
            }],
        );
        assert_eq!(most_plausible_target(&note_heavy), "fridge_note");
    }

    #[test]
    fn supervisor_consumes_tick_events_without_mutating_simulation() {
        let mut supervisor = GhostLobbySupervisor::new("security-team");
        supervisor.ingest(&[Event::UsbThrown]);
        assert_eq!(supervisor.observed_events, 1);
        assert!(supervisor.evidence.is_some());
        assert!(supervisor.team.attention > 0);
        assert_eq!(supervisor.coverage_target.as_deref(), Some("usb"));
    }

    #[test]
    fn sabotage_counts_as_a_tactic_but_a_pump_failure_is_not_one() {
        use crate::companion::MoleEvent;
        use crate::scenario::infra::ReleaseCause;
        // Deliberate hands: an opened valve, the shorts and alarms that
        // follow, and a hosted mole's chewed cable.
        assert!(is_infra_sabotage(&Event::WaterReleased {
            zone: "wet-corridor".into(),
            room: "corridor".into(),
            cause: ReleaseCause::Valve,
        }));
        assert!(is_infra_sabotage(&Event::InfraShorted {
            zone: "wet-corridor".into(),
            room: "corridor".into(),
            devices: 2,
        }));
        assert!(is_infra_sabotage(&Event::AlarmChanged {
            alarm: "corridor-alarm".into(),
            room: "corridor".into(),
            sounding: true,
        }));
        assert!(is_infra_sabotage(&Event::Mole {
            mole: MoleEvent::CableSabotaged {
                cable_id: "cable-1".into(),
            },
        }));
        // No player hand, an alarm falling silent, a decoy (a diversion, not
        // sabotage), and ordinary objective play are all *not* this tactic.
        assert!(!is_infra_sabotage(&Event::WaterReleased {
            zone: "wet-corridor".into(),
            room: "corridor".into(),
            cause: ReleaseCause::PumpFailure,
        }));
        assert!(!is_infra_sabotage(&Event::AlarmChanged {
            alarm: "corridor-alarm".into(),
            room: "corridor".into(),
            sounding: false,
        }));
        assert!(!is_infra_sabotage(&Event::Mole {
            mole: MoleEvent::SynthSoundPlayed {
                sound: "bark".into(),
            },
        }));
        assert!(!is_infra_sabotage(&Event::UsbThrown));
    }

    #[test]
    fn repeated_sabotage_escalates_through_the_bounded_proposal_path() {
        use crate::scenario::infra::ReleaseCause;
        let mut supervisor = GhostLobbySupervisor::new("security-team");
        let event = Event::WaterReleased {
            zone: "wet-corridor".into(),
            room: "corridor".into(),
            cause: ReleaseCause::Valve,
        };
        supervisor.ingest(&[event.clone()]);
        assert_eq!(
            supervisor.director.observations.get(TACTIC_INFRA_SABOTAGE),
            Some(&1)
        );
        assert_eq!(supervisor.director.current_difficulty, 0);
        supervisor.ingest(&[event]);
        // The second occurrence clears the repetition and confidence gates, so
        // the bounded proposal is generated and accepted inside the envelope.
        assert_eq!(
            supervisor.director.observations.get(TACTIC_INFRA_SABOTAGE),
            Some(&2)
        );
        assert_eq!(supervisor.director.current_difficulty, 1);
        assert!(matches!(
            supervisor.director.trace.last(),
            Some(VsmEvent::AdaptiveInterventionApplied {
                from: 0,
                to: 1,
                tactic_id,
            }) if tactic_id == TACTIC_INFRA_SABOTAGE
        ));
    }
}

//! Mission framing for the interactive frontend: the opening brief and one cue
//! per beat of the loop (purpose → opportunity → response → exploit → retry).
//!
//! These are [`LogLine`]s on [`Channel::Tutorial`], which the simulation
//! already treats as frontend-only: `scenario::vsm` skips tutorial and prompt
//! lines when folding supervision, and the canonical determinism diff compares
//! `Log` + `Telemetry` only. So teaching can be added without touching gameplay
//! truth, a snapshot, an event stream, or any golden.
//!
//! Nothing here reads hidden state. Every cue is driven by an event the player
//! can also see in the log, which is the point: the player should learn to
//! notice the same evidence the game acts on.

use idaptik_core::scenario::{
    Channel, Event as SimEvent, FailReason, LogLine, ObjectKind, ObjectiveStatus,
    ScenarioDefinition, Severity,
};

/// Build one tutorial line.
fn cue(text: impl Into<String>, severity: Severity, tick: u64, t: f64) -> LogLine {
    LogLine { tick, t, text: text.into(), severity, channel: Channel::Tutorial }
}

const BRIEF_PRIZE: &str = "You are here for the contact note: a boring sheet of paper in the \
     kitchen. The office drive is the decoy Billy is meant to fixate on.";
const BRIEF_LOOP: &str = "A small change to the building changes what Billy does. H repeats \
     the hint; R restarts the floor with the same seed.";
const OPPORTUNITY: &str = "Billy is inside and reading the room. What he sees is what he will \
     decide you came for.";
const BELIEF_USB: &str = "Billy has decided the drive was the target. That is the story you \
     want him to tell his boss.";
const BELIEF_NOTE: &str = "Billy has decided the note was the target. He will go and secure \
     it, so be quick or be somewhere else.";
const MISDIRECT_LIVE: &str = "The misdirect is live: Billy's report will name the drive.";
const MISDIRECT_BLOWN: &str = "Billy reported the note. The misdirect is blown; leave while \
     the way out is still open.";

/// The opening brief: why you are here, what to take, where to leave, and the
/// one idea the mission is teaching.
pub fn brief(def: &ScenarioDefinition, tick: u64, t: f64) -> Vec<LogLine> {
    let mut lines = Vec::with_capacity(def.objectives.len() + 3);
    lines.push(cue(def.name.clone(), Severity::Info, tick, t));
    lines.push(cue(BRIEF_PRIZE, Severity::Info, tick, t));
    for objective in &def.objectives {
        let label = &objective.label;
        let text = format!("  objectives · {label}");
        lines.push(cue(text, Severity::Info, tick, t));
    }
    lines.push(cue(BRIEF_LOOP, Severity::Info, tick, t));
    lines
}

/// Why the run ended, in the player's terms rather than the enum's.
fn reason_label(reason: &FailReason) -> &'static str {
    match reason {
        FailReason::Caught => "collared by Billy",
        FailReason::Partition => "the uplink was cut",
        FailReason::Lockdown => "the floor locked down",
        FailReason::Traced => "the connection was traced back",
    }
}

/// Emits each cue at most once per run, and re-arms on restart.
#[derive(Debug, Default)]
pub struct Tutor {
    crisis_cued: bool,
    belief_cued: bool,
    misdirect_cued: bool,
    end_cued: bool,
}

impl Tutor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold one tick's events into the cue state, appending any new cue.
    pub fn observe(&mut self, events: &[SimEvent], tick: u64, t: f64, out: &mut Vec<LogLine>) {
        for event in events {
            match event {
                SimEvent::Restarted { .. } => *self = Self::new(),
                SimEvent::CrisisBegan { .. } if !self.crisis_cued => {
                    self.crisis_cued = true;
                    out.push(cue(OPPORTUNITY, Severity::Billy, tick, t));
                }
                SimEvent::BillyBeliefFormed { belief } if !self.belief_cued => {
                    self.belief_cued = true;
                    let text = if *belief == ObjectKind::Usb {
                        BELIEF_USB
                    } else {
                        BELIEF_NOTE
                    };
                    out.push(cue(text, Severity::Billy, tick, t));
                }
                SimEvent::ObjectivesUpdated { misdirect, .. } if !self.misdirect_cued => {
                    match misdirect {
                        ObjectiveStatus::Done => {
                            self.misdirect_cued = true;
                            out.push(cue(MISDIRECT_LIVE, Severity::Good, tick, t));
                        }
                        ObjectiveStatus::Failed => {
                            self.misdirect_cued = true;
                            out.push(cue(MISDIRECT_BLOWN, Severity::Warn, tick, t));
                        }
                        _ => {}
                    }
                }
                SimEvent::MissionFailed { reason } if !self.end_cued => {
                    self.end_cued = true;
                    let why = reason_label(reason);
                    let text =
                        format!("Run over ({why}). R restarts the floor with the same seed.");
                    out.push(cue(text, Severity::Bad, tick, t));
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use idaptik_core::scenario::{CrisisReason, ghost_lobby};

    fn cues(events: &[SimEvent], tutor: &mut Tutor) -> Vec<LogLine> {
        let mut out = Vec::new();
        tutor.observe(events, 1, 0.5, &mut out);
        out
    }

    #[test]
    fn brief_names_the_prize_and_every_objective() {
        let def = ghost_lobby();
        let lines = brief(&def, 0, 0.0);

        assert!(lines.iter().all(|l| l.channel == Channel::Tutorial));
        assert!(lines.iter().any(|l| l.text == def.name));
        assert!(lines.iter().any(|l| l.text.contains("contact note")));
        for objective in &def.objectives {
            assert!(
                lines.iter().any(|l| l.text.contains(&objective.label)),
                "the brief never mentions {:?}",
                objective.label
            );
        }
    }

    #[test]
    fn each_cue_fires_once_per_run() {
        let mut tutor = Tutor::new();
        let crisis = [SimEvent::CrisisBegan { reason: CrisisReason::Timer }];

        assert_eq!(cues(&crisis, &mut tutor).len(), 1);
        assert_eq!(cues(&crisis, &mut tutor).len(), 0);
    }

    #[test]
    fn a_restart_re_arms_the_cues() {
        let mut tutor = Tutor::new();
        let crisis = [SimEvent::CrisisBegan { reason: CrisisReason::Usb }];

        assert_eq!(cues(&crisis, &mut tutor).len(), 1);
        assert_eq!(cues(&[SimEvent::Restarted { seed: 7 }], &mut tutor).len(), 0);
        assert_eq!(cues(&crisis, &mut tutor).len(), 1);
    }

    #[test]
    fn the_belief_cue_says_which_story_billy_believes() {
        let mut tutor = Tutor::new();
        let usb = [SimEvent::BillyBeliefFormed { belief: ObjectKind::Usb }];
        let note = [SimEvent::BillyBeliefFormed { belief: ObjectKind::Note }];

        let drive = cues(&usb, &mut tutor);
        assert_eq!(drive.len(), 1);
        assert!(drive[0].text.contains("drive"));

        let mut tutor = Tutor::new();
        let paper = cues(&note, &mut tutor);
        assert_eq!(paper.len(), 1);
        assert!(paper[0].text.contains("note"));
    }

    #[test]
    fn the_misdirect_cue_distinguishes_live_from_blown() {
        let mut tutor = Tutor::new();
        let live = [SimEvent::ObjectivesUpdated {
            note: ObjectiveStatus::Done,
            misdirect: ObjectiveStatus::Done,
            exit: ObjectiveStatus::Available,
        }];
        let line = cues(&live, &mut tutor);
        assert_eq!(line.len(), 1);
        assert_eq!(line[0].severity, Severity::Good);

        let mut tutor = Tutor::new();
        let blown = [SimEvent::ObjectivesUpdated {
            note: ObjectiveStatus::Done,
            misdirect: ObjectiveStatus::Failed,
            exit: ObjectiveStatus::Available,
        }];
        let line = cues(&blown, &mut tutor);
        assert_eq!(line.len(), 1);
        assert_eq!(line[0].severity, Severity::Warn);
    }

    #[test]
    fn a_failed_run_names_the_reason_in_plain_words() {
        let mut tutor = Tutor::new();
        let over = [SimEvent::MissionFailed { reason: FailReason::Partition }];
        let line = cues(&over, &mut tutor);
        assert_eq!(line.len(), 1);
        assert!(line[0].text.contains("uplink"));
        assert!(line[0].text.contains("R restarts"));
    }
}

//! Tests for the frontend-only mission brief and event-driven tutorial cues.

use idaptik_core::scenario::{
    Channel, CrisisReason, Event, FailReason, ObjectKind, ObjectiveStatus, Severity, ghost_lobby,
};
use idaptik_tui::tutorial::{Tutor, brief};

fn objectives(misdirect: ObjectiveStatus) -> Event {
    Event::ObjectivesUpdated {
        note: ObjectiveStatus::Done,
        misdirect,
        exit: ObjectiveStatus::Available,
    }
}

#[test]
fn brief_preserves_objective_order_and_line_metadata() {
    let def = ghost_lobby();
    let lines = brief(&def, 42, 3.5);

    assert_eq!(lines.len(), def.objectives.len() + 3);
    assert_eq!(lines[0].text, def.name);
    assert!(lines[1].text.contains("contact note"));
    assert!(lines[1].text.contains("office drive"));
    for (line, objective) in lines[2..].iter().zip(&def.objectives) {
        assert_eq!(line.text, format!("  objectives · {}", objective.label));
    }
    assert!(lines.last().unwrap().text.contains("R restarts"));
    assert!(lines.iter().all(|line| line.channel == Channel::Tutorial
        && line.severity == Severity::Info
        && line.tick == 42
        && line.t == 3.5));
}

#[test]
fn brief_with_no_objectives_still_has_the_mission_framing() {
    let mut def = ghost_lobby();
    def.objectives.clear();
    let lines = brief(&def, 7, 1.25);

    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0].text, def.name);
    assert!(lines[1].text.contains("contact note"));
    assert!(lines[2].text.contains("R restarts"));
}

#[test]
fn pending_objectives_and_unrelated_events_do_not_consume_a_cue() {
    let mut tutor = Tutor::new();
    let mut out = Vec::new();
    tutor.observe(
        &[
            Event::Paused,
            objectives(ObjectiveStatus::Open),
            objectives(ObjectiveStatus::Available),
            objectives(ObjectiveStatus::Locked),
        ],
        3,
        0.75,
        &mut out,
    );
    assert!(out.is_empty());
    tutor.observe(&[objectives(ObjectiveStatus::Failed)], 4, 1.0, &mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].severity, Severity::Warn);
    assert!(out[0].text.contains("failed"));
    tutor.observe(&[objectives(ObjectiveStatus::Done)], 5, 1.25, &mut out);
    assert_eq!(out.len(), 1, "a later outcome must not repeat the beat");
}

#[test]
fn cues_append_in_event_order_are_deduplicated_and_restart_in_the_same_batch() {
    let mut tutor = Tutor::new();
    let mut out = brief(&ghost_lobby(), 0, 0.0);
    let original = out.clone();
    let events = [
        Event::CrisisBegan {
            reason: CrisisReason::Timer,
        },
        Event::CrisisBegan {
            reason: CrisisReason::Usb,
        },
        Event::BillyBeliefFormed {
            belief: ObjectKind::Usb,
        },
        Event::BillyBeliefFormed {
            belief: ObjectKind::Note,
        },
        objectives(ObjectiveStatus::Done),
        objectives(ObjectiveStatus::Failed),
        Event::MissionFailed {
            reason: FailReason::Caught,
        },
        Event::MissionFailed {
            reason: FailReason::Traced,
        },
        Event::Restarted { seed: 7 },
        Event::CrisisBegan {
            reason: CrisisReason::Usb,
        },
        Event::BillyBeliefFormed {
            belief: ObjectKind::Note,
        },
        objectives(ObjectiveStatus::Failed),
        Event::MissionFailed {
            reason: FailReason::Traced,
        },
    ];
    tutor.observe(&events, 99, 9.5, &mut out);

    assert_eq!(&out[..original.len()], original.as_slice());
    let added = &out[original.len()..];
    assert_eq!(added.len(), 8);
    assert_eq!(
        added.iter().map(|line| line.severity).collect::<Vec<_>>(),
        vec![
            Severity::Billy,
            Severity::Billy,
            Severity::Good,
            Severity::Bad,
            Severity::Billy,
            Severity::Billy,
            Severity::Warn,
            Severity::Bad,
        ]
    );
    assert!(added[1].text.contains("drive"));
    assert!(added[3].text.contains("collared"));
    assert!(added[5].text.contains("note"));
    assert!(added[7].text.contains("traced"));
    assert!(
        added
            .iter()
            .all(|line| line.channel == Channel::Tutorial && line.tick == 99 && line.t == 9.5)
    );
    tutor.observe(&events[9..], 100, 10.0, &mut out);
    assert_eq!(out.len(), original.len() + 8);
}

#[test]
fn every_failure_reason_is_explained_and_restarts_rearm_the_failure_cue() {
    let mut tutor = Tutor::new();
    let mut out = Vec::new();
    for (reason, expected) in [
        (FailReason::Caught, "collared by Billy"),
        (FailReason::Partition, "uplink was cut"),
        (FailReason::Lockdown, "floor locked down"),
        (FailReason::Traced, "connection was traced back"),
    ] {
        tutor.observe(
            &[
                Event::Restarted { seed: 7 },
                Event::MissionFailed { reason },
            ],
            10,
            2.0,
            &mut out,
        );
        let line = out.last().unwrap();
        assert!(line.text.contains(expected), "{}", line.text);
        assert!(line.text.contains("R restarts"));
        assert_eq!(line.severity, Severity::Bad);
    }
    assert_eq!(out.len(), 4);
}

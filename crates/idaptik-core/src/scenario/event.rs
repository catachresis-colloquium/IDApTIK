//! Typed events — the event-sourced spine of the scenario.
//!
//! Every `logEvent` and state transition in the prototype becomes a typed
//! [`Event`]. The ordered `Vec<Event>` a run produces is the canonical
//! deterministic artifact. Human log lines are a pure *view*: [`log_view`] maps
//! an event to an optional [`LogLine`]. Structural / telemetry events return
//! `None` (they still exist in the event stream, they just aren't rendered).
//!
//! Log text uses this crate's own deterministic formatter (fixed decimals via
//! Rust formatting), **not** JS `toFixed` — the goldens are ours.

use crate::netsim::session::SessionError;
use crate::scenario::common::{
    Channel, ChuteMethod, CrisisReason, DenyReason, ExtractMethod, FailReason, ObjectKind,
    ObjectiveStatus, Outcome, Phase, ReportedTarget, Severity,
};
use crate::scenario::ids::{DoorId, RoomId};
use crate::scenario::tuning::{ActionKind, DifficultyId};
use serde::{Deserialize, Serialize};

/// A typed simulation event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum Event {
    /// The run began.
    RunStarted {
        seed: u32,
        difficulty: DifficultyId,
        reduced_motion: bool,
        /// The infiltrator starts outside (on the roof). Absent from the
        /// canonical floor's events, so its replays are unchanged.
        #[serde(default, skip_serializing_if = "is_false")]
        outside: bool,
    },
    /// Hacker channel: the seed readout.
    SeedAnnounced { seed: u32 },
    /// Structural: a phase transition.
    PhaseChanged {
        from: Phase,
        to: Phase,
        reason: Option<CrisisReason>,
    },
    /// The floor tipped into crisis.
    CrisisBegan { reason: CrisisReason },
    /// Telemetry: an uplink action was performed (ledger/stats).
    UplinkAction { kind: ActionKind },
    /// The hacker pivoted onto a foothold; `hops` is the depth they now stand at.
    PivotOpened { host: String, hops: u32 },
    /// A pivot was refused. The reason is the whole of its use: it is what tells
    /// the player the upstream line is walked one hop at a time.
    PivotDenied { host: String, reason: SessionError },
    /// The hacker backed out of a pivot; `hops` is the depth they came back to.
    PivotClosed { hops: u32 },
    /// Camera ping activated.
    CameraPinged { laundry_view: bool },
    /// A door hold was routed.
    DoorRouted { door: DoorId, delay: f64 },
    /// The robot vacuum route was accepted.
    VacuumRouted,
    /// The lights were flickered.
    LightsFlickered { third_use: bool },
    /// An uplink action was denied.
    UplinkDenied {
        kind: ActionKind,
        reason: DenyReason,
    },
    /// Telemetry: an arbitrary graph node (not one of the four uplinks) was
    /// hacked directly through Net View.
    NetHackAction { node_id: String },
    /// A direct Net View hack was denied.
    NetHackDenied { node_id: String, reason: DenyReason },
    /// Something happened on the floor's signal network (ADR-0012).
    Signal {
        signal: crate::scenario::signal::sim::SignalEvent,
    },
    /// A water zone released and began to fill. `cause` is the provenance the
    /// player reads: a valve they opened, or a pump they killed without
    /// knowing what it held up (ADR-0013).
    WaterReleased {
        zone: String,
        room: RoomId,
        cause: crate::scenario::infra::ReleaseCause,
    },
    /// Rising water shorted a room's electrics: `devices` nodes joined the
    /// dead set in one failure (ADR-0013).
    InfraShorted {
        zone: String,
        room: RoomId,
        devices: usize,
    },
    /// An alarm started or stopped sounding (ADR-0013).
    AlarmChanged {
        alarm: String,
        room: RoomId,
        sounding: bool,
    },
    /// A guard perceived an infrastructure fact through its own senses — the
    /// player-visible record that the guard reacts to *observations*, never
    /// to the hidden actions behind them (ADR-0013). Announce-once per
    /// episode.
    GuardObserved {
        kind: crate::scenario::infra::ObservationKind,
        room: RoomId,
    },
    /// Something the hosted Moletaire companion did, as its own typed event
    /// (ADR-0013). Rendered selectively; the rest is telemetry.
    Mole { mole: crate::companion::MoleEvent },
    /// An order to the hosted companion was refused.
    MoleDenied {
        reason: crate::scenario::common::MoleDenyReason,
    },
    /// A door hold became active.
    DoorHoldActive { door: DoorId, duration: f64 },
    /// The contact note was secured.
    NoteSecured { seen: bool },
    /// The note peel was seen by Billy (latches note exposure).
    NoteExposed,
    /// The USB was taken (device lockdown).
    UsbTaken { seen: bool },
    /// The USB was thrown as a decoy.
    UsbThrown,
    /// The USB self-wiped, leaving a trace.
    UsbSelfWiped,
    /// Billy grabbed the note.
    BillyTookNote,
    /// Billy grabbed the USB.
    BillyTookUsb,
    /// The laundry chute was revealed.
    ChuteRevealed { method: ChuteMethod },
    /// The vacuum began to lag out of control.
    VacuumLagWarned,
    /// The vacuum fell down the chute.
    VacuumFell,
    /// A camera flagged the infiltrator.
    CameraFlag { room: RoomId },
    /// The support envelope is fraying.
    SupportFraying,
    /// A power cut cascaded, felling `nodes` devices downstream of it.
    PowerLost { nodes: usize },
    /// A pickpocket during lights-out succeeded.
    PickpocketSucceeded,
    /// Structural: Billy changed FSM mode.
    BillyStateChanged {
        from: crate::scenario::common::BillyMode,
        to: crate::scenario::common::BillyMode,
    },
    /// Billy formed a belief (announce-once).
    BillyBeliefFormed { belief: ObjectKind },
    /// Billy badged through a door.
    BillyBadgedDoor { door: DoorId },
    /// Billy called his boss.
    BossCalled { reported: ReportedTarget },
    /// The one-shot rescue fired.
    RescueUsed,
    /// Structural: the objective ledger changed.
    ObjectivesUpdated {
        note: ObjectiveStatus,
        misdirect: ObjectiveStatus,
        exit: ObjectiveStatus,
    },
    /// The infiltrator extracted.
    Extracted { method: ExtractMethod },
    /// The mission failed.
    MissionFailed { reason: FailReason },
    /// Structural: the run ended (the debrief is the payload).
    RunEnded { outcome: Outcome },
    /// Session: paused.
    Paused,
    /// Session: resumed.
    Resumed,
    /// Session: restarted.
    Restarted { seed: u32 },
    /// Supervision: the security team's attention allocation changed
    /// (announce-once; supervised runs only).
    TeamAttentionAllocated { team_id: String, attention: u8 },
    /// Supervision: the security team retargeted world coverage
    /// (announce-once; supervised runs only).
    CoverageRetargeted { target: String },
    /// The infiltrator sat down at a console (`on`) or stepped away from it.
    ConsoleUsed { console: String, on: bool },
    /// Billy's arrival countdown started, on a floor that holds it until the
    /// infiltrator leaves the room they began in (or, starting outside, gets in).
    ArrivalClockStarted,
    /// The infiltrator took the rope: abseiling down, or climbing up.
    RopeTaken { down: bool },
    /// The infiltrator dropped through a roof hatch.
    HatchDropped { hatch: String },
    /// The infiltrator picked the front door's lock.
    FrontDoorPicked,
    /// Frontend-only prompt (excluded from the canonical determinism diff).
    ContextHint { text: String },
    /// Frontend-only tutorial cue (excluded from the canonical determinism diff).
    TutorialCue { text: String, seconds: f64 },
}

/// A rendered log line — a *view* over an [`Event`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogLine {
    pub tick: u64,
    pub t: f64,
    pub text: String,
    pub severity: Severity,
    pub channel: Channel,
}

/// Format a timestamp as `MM:SS` (own deterministic formatter).
pub fn format_time(t: f64) -> String {
    let total = if t.is_finite() && t >= 0.0 {
        t.floor() as u64
    } else {
        0
    };
    format!("{:02}:{:02}", total / 60, total % 60)
}

/// One-decimal fixed formatter (deterministic; not JS `toFixed`).
fn dec1(x: f64) -> String {
    format!("{x:.1}")
}

/// Map an event to an optional rendered log line. Structural, telemetry, and
/// frontend-only events (and the vacuum chute reveal, covered by [`Event::VacuumFell`])
/// return `None`.
pub fn log_view(e: &Event, tick: u64, t: f64) -> Option<LogLine> {
    let line = |text: String, severity: Severity, channel: Channel| {
        Some(LogLine {
            tick,
            t,
            text,
            severity,
            channel,
        })
    };
    use Channel::{Log, Prompt, Tutorial};
    use Severity::{Bad, Billy, Good, Hacker, Info, Warn};

    match e {
        Event::RunStarted { outside: false, .. } => line(
            "You are in the dark kitchen. Search fast. The note is boring, the USB shiny, the chute silly.".into(),
            Info,
            Log,
        ),
        Event::RunStarted { outside: true, .. } => line(
            "You are on the roof. Rope at the front edge, down to the street and the front door; \
             or the hatch over the laundry, straight in."
                .into(),
            Info,
            Log,
        ),
        Event::SeedAnnounced { seed } => line(
            format!("Run seed {seed:08X}. Billy's arrival estimate is deliberately imprecise."),
            Hacker,
            Log,
        ),
        Event::CrisisBegan { reason } => {
            let text = match reason {
                CrisisReason::Usb => "Billy: \"They pulled the drive. Lock the floor.\"".into(),
                CrisisReason::Timer => "Billy: \"I'm back early. Something's off in here.\"".into(),
                CrisisReason::Test => "Billy: \"Someone tripped the alarm. Moving in.\"".into(),
            };
            line(text, Billy, Log)
        }
        Event::CameraPinged { laundry_view } => line(
            "Camera ping — a clean estimate for a few seconds.".into(),
            if *laundry_view { Warn } else { Hacker },
            Log,
        ),
        Event::DoorRouted { door, delay } => line(
            format!("Routing a hold to {door} in {}s.", dec1(*delay)),
            Hacker,
            Log,
        ),
        Event::PivotOpened { host, hops } => line(
            format!("Pivoted onto {host}. You are {hops} deep; the trace is bounced that far."),
            Hacker,
            Log,
        ),
        Event::PivotDenied { host, reason } => {
            let why = match reason {
                SessionError::Unresolved => "no such host",
                SessionError::NoRoute => "no route from where you are standing",
                SessionError::NotPivotable => "nothing there to stand on",
                SessionError::NoSuchNode => "no such machine",
                SessionError::AlreadyThere => "already standing on it",
            };
            line(format!("Pivot to {host} refused — {why}."), Warn, Log)
        }
        Event::PivotClosed { hops } => line(
            format!("Backed out. You are {hops} deep."),
            Hacker,
            Log,
        ),
        Event::VacuumRouted => line("Robot vacuum route accepted.".into(), Hacker, Log),
        Event::LightsFlickered { third_use } => line(
            "Hacker flickers the lights.".into(),
            if *third_use { Warn } else { Hacker },
            Log,
        ),
        Event::UplinkDenied { kind, reason } => {
            let why = match reason {
                DenyReason::Cooldown => "on cooldown",
                DenyReason::Bandwidth => "not enough bandwidth",
                DenyReason::VacuumFallen => "vacuum already gone",
                DenyReason::NoRoute => "no route to target",
            };
            line(
                format!("Uplink {kind:?} denied — {why}."),
                // A cooldown or a thin pipe is a wait; no route at all is a wall.
                if matches!(reason, DenyReason::VacuumFallen | DenyReason::NoRoute) {
                    Bad
                } else {
                    Warn
                },
                Log,
            )
        }
        Event::NetHackDenied { node_id, reason } => {
            let why = match reason {
                DenyReason::Cooldown => "on cooldown",
                DenyReason::Bandwidth => "not enough bandwidth",
                DenyReason::VacuumFallen => "vacuum already gone",
                DenyReason::NoRoute => "no route to target",
            };
            line(
                format!("Net hack on {node_id} denied: {why}."),
                if matches!(reason, DenyReason::VacuumFallen | DenyReason::NoRoute) {
                    Bad
                } else {
                    Warn
                },
                Log,
            )
        }
        Event::DoorHoldActive { door, duration } => line(
            format!("{door} hold active for {} seconds.", dec1(*duration)),
            Hacker,
            Log,
        ),
        Event::NoteSecured { seen } => {
            if *seen {
                line("Contact note peeled — in Billy's line of sight.".into(), Bad, Log)
            } else {
                line("Contact lead secured, quietly.".into(), Good, Log)
            }
        }
        Event::UsbTaken { .. } => line("DEVICE LOCKDOWN — the drive begins to wipe.".into(), Warn, Log),
        Event::UsbThrown => line("USB thrown — a shiny decoy skitters away.".into(), Good, Log),
        Event::UsbSelfWiped => line("The drive finished wiping. A trace is left behind.".into(), Bad, Log),
        Event::BillyTookNote => line("Billy pockets the note.".into(), Bad, Log),
        Event::BillyTookUsb => line("Billy grabs the shiny drive instead.".into(), Good, Log),
        Event::ChuteRevealed { method } => match method {
            ChuteMethod::Physical => line("Laundry chute found — a way down.".into(), Good, Log),
            ChuteMethod::Vacuum => None,
        },
        Event::VacuumLagWarned => line("Vacuum control is lagging.".into(), Warn, Log),
        Event::VacuumFell => line("The robot vacuum vanishes down a hidden chute.".into(), Good, Log),
        Event::CameraFlag { room } => line(format!("Camera flag in {room}."), Bad, Log),
        Event::SupportFraying => line("Support is fraying.".into(), Warn, Log),
        Event::PowerLost { nodes } => {
            let noun = if *nodes == 1 { "device" } else { "devices" };
            line(format!("GRID: {nodes} {noun} lost power."), Warn, Log)
        }
        Event::PickpocketSucceeded => line("Lifted the note from Billy's pocket in the dark.".into(), Good, Log),
        Event::BillyBeliefFormed { belief } => {
            let text = match belief {
                ObjectKind::Note => "Billy: \"They were after paperwork. The note.\"".into(),
                ObjectKind::Usb => "Billy: \"They went straight for that little drive.\"".into(),
            };
            line(text, Billy, Log)
        }
        Event::BillyBadgedDoor { door } => line(format!("Billy badges through {door}."), Billy, Log),
        Event::BossCalled { reported } => {
            let text = match reported {
                ReportedTarget::Note => "Billy: \"Boss, they took a note off the board.\"".into(),
                ReportedTarget::Usb => "Billy: \"Boss, they went for that little drive.\"".into(),
                ReportedTarget::Intruder => "Billy: \"Boss, there's someone on the floor.\"".into(),
            };
            line(text, Billy, Log)
        }
        Event::RescueUsed => line("Hacker burns bandwidth to yank you clear.".into(), Hacker, Log),
        Event::Extracted { method } => {
            let text = match method {
                ExtractMethod::ServiceExit => "Clear of the service exit.".into(),
                ExtractMethod::LaundryChute => "Down the laundry chute and gone.".into(),
            };
            line(text, Good, Log)
        }
        Event::MissionFailed { reason } => {
            let text = match reason {
                FailReason::Caught => "Caught on the floor.".into(),
                FailReason::Partition => "Support partitioned — you are on your own, and it shows.".into(),
                FailReason::Lockdown => "Building lockdown. The floor is sealed.".into(),
                FailReason::Traced => "Trace complete. They have the address it came from.".into(),
            };
            line(text, Bad, Log)
        }
        Event::TeamAttentionAllocated { attention, .. } => line(
            format!("Security desk: sweep attention at {attention}%."),
            Billy,
            Log,
        ),
        Event::CoverageRetargeted { target } => {
            let text = match target.as_str() {
                "usb" => "Security desk: \"Sweep past that little drive again.\"".into(),
                "fridge_note" => "Security desk: \"Keep eyes on the noticeboard.\"".into(),
                other => format!("Security desk: \"Cover {other}.\""),
            };
            line(text, Billy, Log)
        }
        Event::Signal { signal } => {
            let (text, severity) = signal_line(signal)?;
            line(text, severity, Log)
        }
        Event::WaterReleased { room, cause, .. } => {
            let text = match cause {
                crate::scenario::infra::ReleaseCause::Valve => {
                    format!("A valve cracks open — water starts into the {room}.")
                }
                crate::scenario::infra::ReleaseCause::PumpFailure => {
                    format!("A pump dies and its valve fails open — water starts into the {room}.")
                }
            };
            line(text, Warn, Log)
        }
        Event::InfraShorted { room, devices, .. } => line(
            format!("Water reaches the {room}'s electrics — {devices} device(s) short out."),
            Bad,
            Log,
        ),
        Event::AlarmChanged {
            room, sounding, ..
        } => line(
            if *sounding {
                format!("An alarm sounds in the {room}.")
            } else {
                format!("The alarm in the {room} falls silent.")
            },
            if *sounding { Bad } else { Info },
            Log,
        ),
        Event::GuardObserved { kind, room } => {
            use crate::scenario::infra::ObservationKind as K;
            let text = match kind {
                K::Darkness => {
                    format!("Billy: \"Who killed the lights?\" He checks toward the {room}.")
                }
                K::UnusualNoise => {
                    format!("Billy hears something wrong coming from the {room}.")
                }
                K::Water => format!("Billy: \"There's water in the {room}.\""),
                K::EquipmentFailure => {
                    format!("Billy sees electrics sparking in the {room}.")
                }
                K::Alarm => format!("Billy hears the alarm going off in the {room}."),
            };
            line(text, Billy, Log)
        }
        Event::Mole { mole } => {
            let (text, severity) = mole_line(mole)?;
            line(text, severity, Log)
        }
        Event::MoleDenied { reason } => {
            use crate::scenario::common::MoleDenyReason as M;
            let why = match reason {
                M::NotHosted => "this floor has no mole",
                M::Gone => "the mole is gone",
                M::Unavailable => "the mole cannot do that right now",
            };
            line(format!("Mole order refused — {why}."), Warn, Log)
        }
        Event::ConsoleUsed { console, on } => line(
            if *on {
                format!("You sit down at {console}.")
            } else {
                format!("You step away from {console}.")
            },
            Info,
            Log,
        ),
        Event::ArrivalClockStarted => line(
            "Hacker: you're under way. Billy's shift clock is running now.".into(),
            Hacker,
            Log,
        ),
        Event::RopeTaken { down } => line(
            if *down {
                "You clip on and go over the edge.".into()
            } else {
                "You take the rope and climb.".into()
            },
            Info,
            Log,
        ),
        Event::HatchDropped { .. } => line(
            "You drop through the hatch. That landing was not quiet.".into(),
            Info,
            Log,
        ),
        Event::FrontDoorPicked => line(
            "The front door's lock gives. You're in.".into(),
            Info,
            Log,
        ),
        Event::ContextHint { text } => line(text.clone(), Info, Prompt),
        Event::TutorialCue { text, .. } => line(text.clone(), Info, Tutorial),

        // Structural / telemetry / session — present in the event stream, but
        // not rendered as log lines.
        Event::PhaseChanged { .. }
        | Event::UplinkAction { .. }
        | Event::NetHackAction { .. }
        | Event::NoteExposed
        | Event::BillyStateChanged { .. }
        | Event::ObjectivesUpdated { .. }
        | Event::RunEnded { .. }
        | Event::Paused
        | Event::Resumed
        | Event::Restarted { .. } => None,
    }
}

/// The log line for a signal-network event, or `None` for pure telemetry.
fn signal_line(e: &crate::scenario::signal::sim::SignalEvent) -> Option<(String, Severity)> {
    use crate::scenario::signal::sim::{Seat, SignalDenyReason as D, SignalEvent as S, Switch};
    let who = |by: &Seat| match by {
        Seat::Infiltrator => "You",
        Seat::Hacker => "Hacker",
    };
    let wire = |l: &crate::scenario::signal::LinkDef| {
        format!(
            "{}.{:?} -> {}.{:?}{}",
            l.from,
            l.from_port,
            l.to,
            l.to_port,
            if l.invert { " (inverted)" } else { "" }
        )
    };
    Some(match e {
        S::Wired { link, by, .. } => (
            format!("{} wired {}.", who(by), wire(link)),
            Severity::Hacker,
        ),
        S::Cut { link, by, .. } => (format!("{} cut {}.", who(by), wire(link)), Severity::Hacker),
        S::Inverted { link, by, .. } => (
            format!("{} flipped {}.", who(by), wire(link)),
            Severity::Hacker,
        ),
        S::Reprogrammed {
            relay, gate, by, ..
        } => (
            format!("{} re-programmed {relay} as {gate:?}.", who(by)),
            Severity::Hacker,
        ),
        S::Denied { by, reason } => {
            let why = match reason {
                D::NoAccess { device } => format!("can't reach {device} from here"),
                D::BadWire { .. } => "those ports don't connect".to_owned(),
                D::Hardwired => "that wire is hardwired".to_owned(),
                D::NoSuchWire => "no wire there".to_owned(),
                D::AlreadyWired => "already wired".to_owned(),
                D::BudgetFull { budget } => format!("all {budget} spare wires are in use"),
                D::NotARelay { device } => format!("{device} is not a relay"),
                D::Unskilled => "that needs the hacker".to_owned(),
                D::HackerOnly { device } => format!("only the hacker can wire into {device}"),
                D::NoSuchDevice { index } => format!("no device #{index}"),
                D::NoBandwidth => "not enough bandwidth".to_owned(),
                D::Busy => "hands still busy at the panel".to_owned(),
            };
            (format!("{}: {why}.", who(by)), Severity::Warn)
        }
        S::Switched { device, what, on } => {
            let state = match (what, on) {
                (Switch::Open, true) => "opens",
                (Switch::Open, false) => "closes",
                (Switch::Locked, true) => "locks",
                (Switch::Locked, false) => "unlocks",
                (Switch::Active, true) => "comes on",
                (Switch::Active, false) => "goes dark",
            };
            (format!("{device} {state}."), Severity::Info)
        }
        S::VacuumRouted { .. } => return None,
    })
}

/// The log line for a hosted-companion event, or `None` for pure telemetry
/// (movement completions, item shuffles and the vibration/scan feeds are the
/// companion's own surfaces, not floor news).
fn mole_line(e: &crate::companion::MoleEvent) -> Option<(String, Severity)> {
    use crate::companion::MoleEvent as M;
    Some(match e {
        M::SynthSoundPlayed { sound } => (
            format!("The mole plays {sound} — a diversion, somewhere ahead."),
            Severity::Good,
        ),
        M::CableSabotaged { cable_id } => (
            format!("The mole chews through a cable ({cable_id})."),
            Severity::Good,
        ),
        M::TrapTriggered { .. } => ("The mole's trap is dug and waiting.".into(), Severity::Info),
        M::MoleDied { .. } => (
            "The mole is gone. It does not come back.".into(),
            Severity::Bad,
        ),
        M::HungerResistanceStarted => (
            "The mole is starving and fighting its orders.".into(),
            Severity::Warn,
        ),
        M::HungerResistanceEnded => (
            "The mole settles back under control.".into(),
            Severity::Info,
        ),
        _ => return None,
    })
}

/// Serde: leave a false flag out of the wire form.
fn is_false(b: &bool) -> bool {
    !*b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_time_is_mm_ss() {
        assert_eq!(format_time(0.0), "00:00");
        assert_eq!(format_time(65.4), "01:05");
        assert_eq!(format_time(-3.0), "00:00");
    }

    #[test]
    fn structural_events_have_no_log_line() {
        assert!(log_view(&Event::NoteExposed, 0, 0.0).is_none());
        assert!(
            log_view(
                &Event::UplinkAction {
                    kind: ActionKind::Door
                },
                1,
                0.0
            )
            .is_none()
        );
        assert!(
            log_view(
                &Event::ChuteRevealed {
                    method: ChuteMethod::Vacuum
                },
                1,
                0.0
            )
            .is_none()
        );
    }

    #[test]
    fn a_power_cascade_reads_as_a_grid_report() {
        let many = log_view(&Event::PowerLost { nodes: 4 }, 0, 0.0).unwrap();
        assert_eq!(many.text, "GRID: 4 devices lost power.");
        assert_eq!(many.severity, Severity::Warn);
        let one = log_view(&Event::PowerLost { nodes: 1 }, 0, 0.0).unwrap();
        assert_eq!(one.text, "GRID: 1 device lost power.");
    }

    #[test]
    fn logged_events_carry_tick_and_time() {
        let l = log_view(&Event::SeedAnnounced { seed: 0x1E240 }, 42, 1.5).unwrap();
        assert_eq!(l.tick, 42);
        assert_eq!(l.channel, Channel::Log);
        assert!(l.text.contains("0001E240"));
    }

    #[test]
    fn a_net_hack_action_has_no_log_line_but_a_denial_does() {
        // NetHackAction is telemetry, exactly like UplinkAction; the denial is
        // the thing a player needs to read.
        assert!(
            log_view(
                &Event::NetHackAction {
                    node_id: "substation".into()
                },
                0,
                0.0
            )
            .is_none()
        );
        let denied = log_view(
            &Event::NetHackDenied {
                node_id: "substation".into(),
                reason: DenyReason::Cooldown,
            },
            0,
            0.0,
        )
        .expect("a denial reads as a log line");
        assert!(denied.text.contains("substation"));
        assert!(denied.text.contains("cooldown"));
    }
}

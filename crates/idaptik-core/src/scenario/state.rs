//! Owned, serde runtime state — the mutable half of an event-sourced run.
//!
//! Every field is a faithful transliteration of the prototype's `game.*` object.
//! The state is pure data: the systems in [`crate::scenario::sim`] mutate it in a
//! fixed order and emit typed events; [`RuntimeState`] itself contains no logic
//! beyond [`RuntimeState::initial`], which builds the quiet-phase start from a
//! reset roll.

use crate::scenario::agents::Agents;
use crate::scenario::command::RunConfig;
use crate::scenario::common::{
    BillyMode, ChuteMethod, ExtractMethod, ObjectKind, ObjectiveStatus, Phase, ReportedTarget,
};
use crate::scenario::constants as c;
use crate::scenario::definition::ScenarioDefinition;
use crate::scenario::floor_graph::floor_graph;
use crate::scenario::ids::DoorId;
use crate::scenario::outcome::Debrief;
use crate::scenario::rng::InitRoll;
use crate::scenario::secondary_spaces::SecondaryState;
use crate::scenario::signal::sim::SignalState;
use crate::scenario::sim::GhostLobbySim;
use crate::scenario::tuning::{ActionKind, DifficultyPreset};
use crate::scenario::vsm::SupervisionState;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Where the infiltrator's body is: on the floor line (indoors, or on the
/// street outside), on the roof, or on the rope between them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub enum Place {
    #[default]
    Floor,
    Roof,
    Rope {
        /// Abseiling down (or climbing up).
        down: bool,
    },
}

impl Place {
    fn is_floor(&self) -> bool {
        *self == Place::Floor
    }
}

/// The infiltrator's physical and interaction state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayerState {
    pub x: f64,
    pub y: f64,
    pub vx: f64,
    pub vy: f64,
    pub facing: f64,
    pub stamina: f64,
    pub interaction_progress: f64,
    pub noise: f64,
    pub caught_grace: f64,
    pub grounded: bool,
    pub crouching: bool,
    pub hidden: bool,
    pub sprinting: bool,
    pub has_note: bool,
    pub has_usb: bool,
    /// Index of the nearby hide spot, if the player is within one.
    pub hide_spot: Option<usize>,
    /// Floor, roof or rope. Always the floor on a floor with no outside, so
    /// the canonical snapshots are unchanged.
    #[serde(default, skip_serializing_if = "Place::is_floor")]
    pub place: Place,
    /// Seconds spent picking the front door's lock.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub lockpick: f64,
}

/// Billy's physical state, FSM bookkeeping and belief meters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BillyState {
    pub x: f64,
    pub y: f64,
    pub vx: f64,
    pub facing: f64,
    pub state_timer: f64,
    pub note_interest: f64,
    pub usb_interest: f64,
    pub player_interest: f64,
    pub last_known_x: f64,
    pub last_seen_ago: f64,
    pub patrol_target: f64,
    pub door_wait: f64,
    pub stun: f64,
    pub guard_timer: f64,
    pub call_timer: f64,
    pub snack_x: f64,
    pub mode: BillyMode,
    pub target: Option<ObjectKind>,
    pub belief: Option<ObjectKind>,
    pub belief_announced: Option<ObjectKind>,
    pub blocked_door: Option<usize>,
    pub reported_target: Option<ReportedTarget>,
    pub called: bool,
    pub has_note: bool,
    pub has_usb: bool,
}

/// A door's runtime state (route countdown, hold, badge latch).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DoorState {
    pub id: DoorId,
    pub x: f64,
    pub open: f64,
    pub pending: f64,
    pub route_delay: f64,
    pub route_duration: f64,
    pub badge_logged: bool,
    /// A picked lock: the door no longer stops anyone.
    #[serde(default, skip_serializing_if = "is_false")]
    pub unlocked: bool,
}

/// The contact note.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoteState {
    pub x: f64,
    pub y: f64,
    pub progress: f64,
    pub held: bool,
    pub billy_has: bool,
    pub exposed: bool,
}

/// The USB trap (ballistics + self-wipe).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsbState {
    pub x: f64,
    pub y: f64,
    pub vx: f64,
    pub vy: f64,
    pub timer: f64,
    pub held: bool,
    pub billy_has: bool,
    pub thrown: bool,
    pub wiped: bool,
    pub on_floor: bool,
}

/// The laundry chute.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChuteState {
    pub x: f64,
    pub y: f64,
    pub progress: f64,
    pub revealed: bool,
    pub used: bool,
}

/// The robot vacuum distraction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VacuumState {
    pub x: f64,
    pub y: f64,
    pub control: f64,
    pub target: f64,
    pub active: bool,
    pub fallen: bool,
    pub lag_warned: bool,
}

/// One uplink action's cooldown state.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ActionState {
    pub cd: f64,
    pub max: f64,
}

/// Fixed-key log-throttle timestamps (never a `String` map — determinism).
/// Slots default to `-999.0` so the first emit of each key always passes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Throttles {
    /// Per-action cooldown-denial throttle, indexed by [`ActionKind::index`].
    pub cd: [f64; 4],
    /// Per-action bandwidth-denial throttle.
    pub bw: [f64; 4],
    /// Support-fraying throttle.
    pub support_fray: f64,
    /// Per-door badge throttle.
    pub badge: [f64; 4],
    /// Per-action route-denial throttle, indexed by [`ActionKind::index`].
    /// Kept apart from `cd`: a cooldown denial and a later no-route denial for
    /// the same action are different news, and sharing a slot let firing one
    /// silently swallow the other for a throttle window.
    pub route: [f64; 4],
    /// Net View direct-hack denial throttle. A single slot, not indexed by
    /// `ActionKind`: today there is genuinely only one direct-hack target
    /// (the substation), so one slot is all the state this needs.
    pub net_hack_denial: f64,
}

impl Default for Throttles {
    fn default() -> Self {
        Self {
            cd: [-999.0; 4],
            bw: [-999.0; 4],
            support_fray: -999.0,
            badge: [-999.0; 4],
            route: [-999.0; 4],
            net_hack_denial: -999.0,
        }
    }
}

/// Run statistics — the raw counters the debrief scores.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub hacker_actions: u32,
    pub failed_actions: u32,
    pub camera_detections: u32,
    pub doors_held: u32,
    pub light_flickers: u32,
    pub jumps: u32,
    /// Accumulated sprint seconds (a float, as in the prototype).
    pub sprints: f64,
    pub hidden_seconds: f64,
    pub support_broken_time: f64,
    pub max_isolation: f64,
    pub rescue_used: bool,
    pub usb_trace: bool,
    pub boss_called: bool,
    pub note_exposed: bool,
    pub vacuum_used: bool,
    pub chute_revealed_by: Option<ChuteMethod>,
    pub extraction: Option<ExtractMethod>,
}

/// The full mutable runtime state of a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeState {
    pub phase: Phase,
    pub t: f64,
    pub quiet_remaining: f64,
    pub crisis_at: f64,
    pub support: f64,
    pub isolation: f64,
    pub bandwidth: f64,
    pub alert: f64,
    pub max_alert: f64,
    pub camera_ping: f64,
    pub lights_flicker: f64,
    pub camera_detection: f64,
    pub camera_lockout: f64,
    pub lights_uses: u32,
    pub camera_seen_count: u32,
    pub player: PlayerState,
    pub billy: BillyState,
    pub doors: Vec<DoorState>,
    pub note: NoteState,
    pub usb: UsbState,
    pub chute: ChuteState,
    pub vacuum: VacuumState,
    pub actions: BTreeMap<ActionKind, ActionState>,
    pub throttles: Throttles,
    /// Cooldown remaining on Net View's direct-hack gate (shared across every
    /// non-uplink actuatable node; today, in practice, only the substation).
    /// Original addition, not a ported prototype field: see `constants.rs`.
    pub net_hack_cd: f64,
    pub stats: Stats,
    /// The two players' network sessions.
    pub agents: Agents,
    /// Per camera, in definition order: seconds of looped footage remaining.
    pub camera_looped: Vec<f64>,
    /// The nodes currently without power. A `BTreeSet`, not a `HashSet`: the
    /// simulation iterates it, so its order must be deterministic or the event
    /// log stops being reproducible.
    pub dead_nodes: BTreeSet<String>,
    /// The last emitted objective ledger — a change emits `ObjectivesUpdated`.
    pub objective_ledger: (ObjectiveStatus, ObjectiveStatus, ObjectiveStatus),
    /// The security team's VSM supervision state. Always present so the
    /// snapshot shape is config-independent; it only *changes* when
    /// `RunConfig::supervised` is on, which keeps unsupervised runs
    /// tick-identical to pre-supervision builds.
    pub supervision: SupervisionState,
    pub ended: bool,
    pub result: Option<Debrief>,
    /// The floor's signal network state, when the definition carries one
    /// (ADR-0012). Skipped when absent, so an unwired run's snapshot is
    /// byte-identical to one from before wiring existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<SignalState>,
    /// Seconds the infiltrator's hands are still busy at a wiring panel.
    /// Movement and interaction wait until it runs out.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub signal_busy: f64,
    /// Billy's arrival countdown is held (frozen) until the infiltrator first
    /// leaves the spawn room. Only ever set on a floor that asks for it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub arrival_held: bool,
    /// The console the infiltrator is using, if any. They hold still while
    /// they do; interact again steps away.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub using_console: Option<String>,
    /// The mole's secondary-space state, when the definition carries
    /// secondary spaces. Skipped when absent, so a floor without routes has
    /// the same snapshot shape as before this layer existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary: Option<SecondaryState>,
    /// What the building's systems have observed (only on floors with
    /// consoles).
    #[serde(
        default,
        skip_serializing_if = "crate::scenario::console::Intel::is_empty"
    )]
    pub intel: crate::scenario::console::Intel,
}

impl RuntimeState {
    /// Build the quiet-phase start from a reset roll and difficulty preset.
    pub fn initial(
        def: &ScenarioDefinition,
        roll: &InitRoll,
        _cfg: RunConfig,
        preset: &DifficultyPreset,
    ) -> Self {
        let doors = def
            .doors
            .iter()
            .enumerate()
            .map(|(i, d)| DoorState {
                id: d.id.clone(),
                x: d.x,
                open: 0.0,
                pending: 0.0,
                route_delay: roll.door_delay.get(i).copied().unwrap_or(0.0),
                route_duration: c::ROUTE_DURATION,
                badge_logged: false,
                unlocked: false,
            })
            .collect();

        let actions = ActionKind::ALL
            .iter()
            .map(|k| {
                let max = def.actions.get(k).map(|s| s.cooldown).unwrap_or(0.0);
                (*k, ActionState { cd: 0.0, max })
            })
            .collect();

        // The infiltrator's vantage is the room they spawn in, judged exactly as
        // the simulation judges it. `player.x` below is `def.player.spawn_x`, so
        // the lookup reads the same value the field is about to take. A
        // definition with no rooms yields no room id, and `Agents::initial` then
        // seats the infiltrator in the van rather than panicking.
        let graph = floor_graph(def);
        let start_room = GhostLobbySim::room_id_at(def, def.player.spawn_x).unwrap_or_default();
        let agents = Agents::initial(&graph, start_room, preset.trace_threshold);

        RuntimeState {
            phase: Phase::Quiet,
            t: 0.0,
            quiet_remaining: roll.arrival,
            crisis_at: roll.arrival,
            support: 1.0,
            isolation: 0.0,
            bandwidth: 100.0,
            alert: 0.0,
            max_alert: 0.0,
            camera_ping: 0.0,
            lights_flicker: 0.0,
            camera_detection: 0.0,
            camera_lockout: 0.0,
            lights_uses: 0,
            camera_seen_count: 0,
            player: PlayerState {
                x: def.player.spawn_x,
                y: def.player.spawn_y,
                vx: 0.0,
                vy: 0.0,
                facing: 1.0,
                stamina: 100.0,
                interaction_progress: 0.0,
                noise: 0.0,
                caught_grace: 0.0,
                grounded: true,
                crouching: false,
                hidden: false,
                sprinting: false,
                has_note: false,
                has_usb: false,
                hide_spot: None,
                // A floor with an outside starts on the roof.
                place: if def.exterior.is_some() {
                    Place::Roof
                } else {
                    Place::Floor
                },
                lockpick: 0.0,
            },
            billy: BillyState {
                x: def.billy.spawn_x,
                y: def.billy.spawn_y,
                vx: 0.0,
                facing: 1.0,
                state_timer: 0.0,
                note_interest: 0.0,
                usb_interest: 0.0,
                player_interest: 0.0,
                last_known_x: c::BILLY_LAST_KNOWN_X,
                last_seen_ago: c::BILLY_LAST_SEEN_AGO,
                patrol_target: c::BILLY_PATROL_TARGET,
                door_wait: 0.0,
                stun: 0.0,
                guard_timer: 0.0,
                call_timer: 0.0,
                snack_x: roll.snack_x,
                mode: BillyMode::Offsite,
                target: None,
                belief: None,
                belief_announced: None,
                blocked_door: None,
                reported_target: None,
                called: false,
                has_note: false,
                has_usb: false,
            },
            doors,
            note: NoteState {
                x: roll.note_x,
                y: def.props.note.y,
                progress: 0.0,
                held: false,
                billy_has: false,
                exposed: false,
            },
            usb: UsbState {
                x: roll.usb_x,
                y: def.props.usb.y,
                vx: 0.0,
                vy: 0.0,
                timer: 0.0,
                held: false,
                billy_has: false,
                thrown: false,
                wiped: false,
                on_floor: true,
            },
            chute: ChuteState {
                x: def.props.chute.x,
                y: def.props.chute.y,
                progress: 0.0,
                revealed: false,
                used: false,
            },
            vacuum: VacuumState {
                x: def.props.vacuum.x,
                y: def.props.vacuum.y,
                control: 1.0,
                target: c::VAC_TARGET,
                active: false,
                fallen: false,
                lag_warned: false,
            },
            actions,
            throttles: Throttles::default(),
            net_hack_cd: 0.0,
            stats: Stats::default(),
            agents,
            camera_looped: vec![0.0; def.cameras.len()],
            dead_nodes: BTreeSet::new(),
            objective_ledger: (
                ObjectiveStatus::Open,
                ObjectiveStatus::Open,
                ObjectiveStatus::Locked,
            ),
            supervision: SupervisionState::new(c::SUPERVISION_TEAM_ID),
            ended: false,
            result: None,
            signal: def.signal.as_ref().map(SignalState::initial),
            signal_busy: 0.0,
            arrival_held: def.arrival_clock
                == crate::scenario::definition::ArrivalClock::LeftSpawnRoom,
            using_console: None,
            secondary: None,
            intel: crate::scenario::console::Intel::default(),
        }
    }
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

fn is_false(b: &bool) -> bool {
    !*b
}

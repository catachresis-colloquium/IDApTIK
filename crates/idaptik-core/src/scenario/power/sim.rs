//! The deterministic power runtime.
//!
//! [`PowerSim::apply`] takes one [`PowerCommand`] and returns what it produced;
//! [`PowerSim::step`] advances one fixed tick against the host's [`PowerReadout`]
//! and returns the events that tick produced. The shape is the signal network's
//! shape on purpose — commands apply immediately, the world settles one tick at a
//! time, and a state that is a pure function of the definition plus the command
//! stream snapshots exactly.
//!
//! # The tick, in order
//!
//! 1. **Sources.** Start-up counts down; a backup decides whether to engage
//!    against *last* tick's supply and burns a tick of runtime doing it. A backup
//!    that runs out is spent for the run.
//! 2. **Protection.** A feed over its rating counts up; at
//!    [`PowerTuning::trip_ticks`] its breaker opens and latches an overload
//!    fault. A feed back inside its rating clears an overload fault, but the
//!    breaker still has to be closed by somebody.
//! 3. **Feeds.** Live iff not cut, breaker closed, and the primary source is
//!    delivering. Transitions are reported once per feed, not once per device.
//! 4. **Loads.** Supply is read, then a fail-safe load drops its command, then a
//!    resuming load starts its warm-up. Finally each load's operating state and
//!    its [`OffReason`] are computed against the arbitration ladder.
//! 5. **Hazards.** An authored hazard is live by its own rule and shocks a body
//!    inside its bounds once per contact.
//!
//! Interlocks read the equipment's state at the *start* of the tick, the way a
//! real interlock reads a contactor rather than this tick's arithmetic. That is
//! what makes a pair of mutually interlocked machines deterministic instead of
//! order-dependent.

use super::{
    BackupMode, Condition, HazardLive, InterlockWhen, LoadKind, PowerCosts, PowerNetworkDef,
    PowerTuning, PowerValidationError, Restart,
};
use crate::scenario::ids::{BreakerId, FeedId, HazardId, LoadId, PowerSourceId, RoomId, SignalId};
use crate::scenario::signal::sim::Seat;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The snapshot format this build reads and writes.
pub const POWER_SNAPSHOT_FORMAT: &str = "idaptik-power-snapshot/1";

/// Which elements a seat can touch right now — loads, feeds, breakers and
/// sources in one id space (the definition is validated to keep it unique).
///
/// The host computes this, exactly as it computes the signal network's
/// [`crate::scenario::signal::sim::Access`], and hands it in with every command.
/// The difference is permission, not access: either seat may switch a device it
/// can reach, but only the infiltrator can cut a cable and only a board authored
/// `remote` can be worked over the network at all.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerAccess {
    pub infiltrator: BTreeSet<String>,
    pub hacker: BTreeSet<String>,
}

impl PowerAccess {
    /// Both seats can touch every element (tests, and a sandbox view).
    pub fn everyone(def: &PowerNetworkDef) -> Self {
        let mut all: BTreeSet<String> = BTreeSet::new();
        all.extend(def.sources.iter().map(|s| s.id.0.clone()));
        all.extend(def.feeds.iter().map(|f| f.id.0.clone()));
        all.extend(def.loads.iter().map(|l| l.id.0.clone()));
        all.extend(def.breakers.iter().map(|b| b.id.0.clone()));
        Self {
            infiltrator: all.clone(),
            hacker: all,
        }
    }

    /// Whether `seat` can touch `element`.
    pub fn can(&self, seat: Seat, element: &str) -> bool {
        match seat {
            Seat::Infiltrator => self.infiltrator.contains(element),
            Seat::Hacker => self.hacker.contains(element),
        }
    }
}

/// Where the infiltrator's body is, for hazards: the room and the body centre.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BodyAt {
    pub room: RoomId,
    pub x: f64,
}

/// The host's view of the world for one tick.
///
/// * `signal_active` / `signal_locked` feed the `SignalActive` and
///   `SignalLocked` interlocks — the power network never reaches into the signal
///   network to look.
/// * `body` is how an exposed conductor finds out whether somebody is standing
///   in it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PowerReadout {
    pub signal_active: BTreeSet<SignalId>,
    pub signal_locked: BTreeSet<SignalId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<BodyAt>,
}

impl PowerReadout {
    /// Assert a signal device is switched on.
    pub fn active(&mut self, device: &str) -> &mut Self {
        self.signal_active.insert(device.into());
        self
    }

    /// Assert a signal device is locked.
    pub fn locked(&mut self, device: &str) -> &mut Self {
        self.signal_locked.insert(device.into());
        self
    }

    /// Place the body, for hazard containment.
    pub fn body(&mut self, room: &str, x: f64) -> &mut Self {
        self.body = Some(BodyAt {
            room: room.into(),
            x,
        });
        self
    }
}

/// A typed command the power runtime consumes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum PowerCommand {
    /// **Physical sabotage.** Cut the conductor feeding `feed`. Infiltrator
    /// only, at the cable, and never undone: the breaker trips on the fault, the
    /// feed's loads go dark, and no panel in the building can splice it. The
    /// logical answer is a [`PowerCommand::Route`] onto another feed.
    CutCable { by: Seat, feed: FeedId },
    /// **Logical rewiring.** Move `load` onto `feed`. Reversible, capacity
    /// checked by protection rather than refused, and it reconnects a
    /// [`Condition::Disconnected`] load.
    Route {
        by: Seat,
        load: LoadId,
        feed: FeedId,
    },
    /// Work the protection: `open` isolates (deliberately or as a lockout),
    /// `!open` is a deliberate reset, refused while a fault is present.
    Isolate {
        by: Seat,
        breaker: BreakerId,
        open: bool,
    },
    /// Throw a source on or off. A spent backup refuses to come back.
    SwitchSource {
        by: Seat,
        source: PowerSourceId,
        on: bool,
    },
    /// Switch one device. Manual and remote share this verb and this authority;
    /// only access, cost and the `remote` flag differ. It is a *request*: it
    /// cannot make an unpowered device operate.
    SetCommand { by: Seat, load: LoadId, on: bool },
    /// Ask why a device is off. The answer is a [`LoadReport`].
    Inspect { by: Seat, load: LoadId },
}

impl PowerCommand {
    /// What this command costs its seat, before it is applied. The host checks
    /// and charges it; the runtime's event carries the same figure.
    pub fn cost(&self, def: &PowerNetworkDef) -> WorkCost {
        let by = self.seat();
        match self {
            PowerCommand::CutCable { .. } => cut_cost(def, by),
            PowerCommand::Route { .. } => route_cost(def, by),
            PowerCommand::Isolate { .. } => isolate_cost(def, by),
            PowerCommand::SwitchSource { .. } => source_cost(def, by),
            PowerCommand::SetCommand { .. } => command_cost(def, by),
            // Asking is free: knowing why is not a resource.
            PowerCommand::Inspect { .. } => WorkCost::default(),
        }
    }

    /// The seat issuing the command.
    pub fn seat(&self) -> Seat {
        match self {
            PowerCommand::CutCable { by, .. }
            | PowerCommand::Route { by, .. }
            | PowerCommand::Isolate { by, .. }
            | PowerCommand::SwitchSource { by, .. }
            | PowerCommand::SetCommand { by, .. }
            | PowerCommand::Inspect { by, .. } => *by,
        }
    }
}

/// What a piece of electrical work cost. The host charges it: bandwidth against
/// the hacker's link, busy ticks against the infiltrator's hands, noise against
/// how much attention the floor is paying.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct WorkCost {
    pub bandwidth: u32,
    pub busy_ticks: u32,
    pub noise: f64,
}

/// Why a command was refused. Refusals are about *permission and possibility*,
/// never about the state a device happens to be in: a command that cannot make a
/// device run still succeeds, and the device reports why it did not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "why", rename_all = "snake_case")]
pub enum PowerDenyReason {
    /// The seat cannot reach this element from where it is.
    NoAccess { element: String },
    /// A cable cut is a hand tool's work. No bandwidth buys one.
    NotPhysical,
    /// The board is local-only: reachable over the network, but not workable
    /// over it.
    RemotePanel { element: String },
    /// This conductor is already severed.
    AlreadyCut { feed: FeedId },
    /// The load is already on that feed.
    SameFeed { load: LoadId },
    /// The breaker will not close while the fault that opened it stands.
    FaultPresent { fault: Fault },
    /// The backup has spent its runtime; it does not come back this run.
    BackupExhausted { source: PowerSourceId },
    /// A command named an index the network does not have.
    NoSuchElement { index: u16 },
    /// The hacker cannot afford the bandwidth (host-checked).
    NoBandwidth,
    /// The infiltrator's hands are still busy (host-checked).
    Busy,
}

/// The protective state of a feed, as its breaker sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fault {
    None,
    /// The conductor has been severed. Permanent: this run has no splice.
    ConductorCut,
    /// The feed was worked past its rating. Clears when the rating is respected;
    /// the breaker still needs a deliberate reset.
    Overload,
}

/// Why a device is off. The ladder is ordered, and the *first* rung that denies
/// is what a player is shown:
///
/// 1. [`OffReason::Damaged`] — the device itself is broken.
/// 2. [`OffReason::Disconnected`] — the device is wired to nothing.
/// 3. [`OffReason::Interlock`] — a local safety is holding it.
/// 4. [`OffReason::Unavailable`] — its supply is gone for the rest of the run.
/// 5. [`OffReason::Isolated`] — its protection is open.
/// 6. [`OffReason::Unpowered`] — its supply is off but recoverable.
/// 7. [`OffReason::Restarting`] — supply is back and it is warming up.
/// 8. [`OffReason::NoCommand`] — everything is ready and nobody asked.
///
/// The order walks upstream from the device: its own fault, then its local
/// safeties, then its supply, then the order someone gave it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "why", rename_all = "snake_case")]
pub enum OffReason {
    Damaged,
    Disconnected,
    Interlock { interlock: String, label: String },
    Unavailable { feed: FeedId },
    Isolated { breaker: BreakerId },
    Unpowered { feed: FeedId },
    Restarting { ticks: u32 },
    NoCommand,
}

impl OffReason {
    /// The noun the log line uses: *why* the device is off, in one phrase.
    pub fn phrase(&self) -> String {
        match self {
            OffReason::Damaged => "damaged".to_owned(),
            OffReason::Disconnected => "disconnected".to_owned(),
            OffReason::Interlock { label, .. } => format!("interlock: {label}"),
            OffReason::Unavailable { feed } => format!("no supply ({feed} is severed)"),
            OffReason::Isolated { breaker } => format!("isolated at {breaker}"),
            OffReason::Unpowered { feed } => format!("unpowered ({feed})"),
            OffReason::Restarting { ticks } => format!("restarting ({ticks} ticks)"),
            OffReason::NoCommand => "no command".to_owned(),
        }
    }
}

/// The answer to *why is this device off* — the whole reason the ladder exists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoadReport {
    pub load: LoadId,
    pub label: String,
    pub kind: LoadKind,
    /// Where it is wired now, which a `Route` may have changed.
    pub feed: FeedId,
    pub condition: Condition,
    pub energized: bool,
    pub commanded: bool,
    pub isolated: bool,
    /// The interlock holding it, if one is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interlocked: Option<String>,
    pub restart_left: u32,
    /// Its connected rating, and the rating of the feed it is on.
    pub draw: u32,
    pub capacity: u32,
    pub operating: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<OffReason>,
}

impl LoadReport {
    /// One line a player can act on.
    pub fn explain(&self) -> String {
        match &self.why {
            None => format!("{} is running ({}).", self.label, self.feed),
            Some(why) => format!(
                "{} is off — {}. [{} {}/{} on {}]",
                self.label,
                why.phrase(),
                self.kind.noun(),
                self.draw,
                self.capacity,
                self.feed
            ),
        }
    }
}

/// A typed event the power runtime emits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum PowerEvent {
    /// A conductor was cut by hand: physical, permanent, loud.
    CableCut {
        feed: FeedId,
        by: Seat,
        cost: WorkCost,
        /// How many loads went dark with it.
        loads_lost: usize,
    },
    /// A load was rewired onto another feed: logical, reversible, priced.
    Routed {
        load: LoadId,
        from: FeedId,
        to: FeedId,
        by: Seat,
        cost: WorkCost,
    },
    /// A breaker was opened deliberately.
    Isolated {
        breaker: BreakerId,
        feed: FeedId,
        by: Seat,
        cost: WorkCost,
        fault: Fault,
    },
    /// A breaker was deliberately reset.
    Reset {
        breaker: BreakerId,
        feed: FeedId,
        by: Seat,
        cost: WorkCost,
    },
    /// Protection operated on its own.
    Tripped {
        breaker: BreakerId,
        feed: FeedId,
        fault: Fault,
    },
    /// A feed is being worked past its rating. It trips in `trip_ticks`.
    Overloaded {
        feed: FeedId,
        draw: u32,
        capacity: u32,
        trip_ticks: u32,
    },
    /// A feed lost supply, taking `loads` devices with it.
    FeedLost {
        feed: FeedId,
        loads: usize,
    },
    /// A feed has supply again. What each device does about it is its own
    /// restart policy's business.
    FeedRestored {
        feed: FeedId,
    },
    /// A source was thrown.
    Switched {
        source: PowerSourceId,
        on: bool,
        by: Seat,
        cost: WorkCost,
    },
    /// A backup has picked up the loads it serves, for `runtime_ticks` more.
    BackupEngaged {
        source: PowerSourceId,
        loads: usize,
        runtime_ticks: u32,
    },
    /// A backup has run out. It is spent for the run.
    BackupSpent {
        source: PowerSourceId,
    },
    /// A load was switched. `on` is the *command*, not the outcome.
    Commanded {
        load: LoadId,
        on: bool,
        by: Seat,
        cost: WorkCost,
    },
    /// A device started or stopped running for a reason other than its feed
    /// going up or down (a command, an interlock, a warm-up ending, damage).
    LoadChanged {
        load: LoadId,
        operating: bool,
        why: Option<OffReason>,
    },
    /// Somebody touched an exposed conductor.
    Shocked {
        hazard: HazardId,
        room: RoomId,
        ticks: u32,
    },
    /// The answer to an inspection.
    Inspected {
        by: Seat,
        load: LoadId,
        /// Boxed: the report is the largest thing any power event carries, and
        /// every other variant pays for it in an enum this hot loop copies.
        report: Box<LoadReport>,
    },
    Denied {
        by: Seat,
        reason: PowerDenyReason,
    },
}

impl PowerEvent {
    /// The seat a work event belongs to, or `None` for a world event.
    pub fn seat(&self) -> Option<Seat> {
        match self {
            PowerEvent::CableCut { by, .. }
            | PowerEvent::Routed { by, .. }
            | PowerEvent::Isolated { by, .. }
            | PowerEvent::Reset { by, .. }
            | PowerEvent::Switched { by, .. }
            | PowerEvent::Commanded { by, .. }
            | PowerEvent::Inspected { by, .. }
            | PowerEvent::Denied { by, .. } => Some(*by),
            _ => None,
        }
    }

    /// What this event cost the seat that caused it, if anything.
    pub fn cost(&self) -> Option<WorkCost> {
        match self {
            PowerEvent::CableCut { cost, .. }
            | PowerEvent::Routed { cost, .. }
            | PowerEvent::Isolated { cost, .. }
            | PowerEvent::Reset { cost, .. }
            | PowerEvent::Switched { cost, .. }
            | PowerEvent::Commanded { cost, .. } => Some(*cost),
            _ => None,
        }
    }
}

/// One source's runtime state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceState {
    pub on: bool,
    /// Delivering this tick — for a primary source, to its feed; for a backup,
    /// to the loads it serves.
    pub output: bool,
    /// Backup runtime units left. Unused by a primary source.
    pub runtime_left: u32,
    /// Ticks until a slow source is up to speed.
    pub startup_left: u32,
    /// A backup that has spent its runtime never comes back this run.
    pub spent: bool,
    /// A backup is currently carrying its loads (and burning runtime).
    pub engaged: bool,
}

/// One feed's runtime state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeedState {
    /// The conductor has been severed.
    pub cut: bool,
    /// Deliverable this tick.
    pub live: bool,
}

/// One breaker's runtime state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BreakerState {
    /// Open: isolated, tripped or locked out. Nothing downstream is energized.
    pub open: bool,
    pub fault: Fault,
    /// Consecutive ticks the feed has sat over its rating.
    pub overload_ticks: u32,
}

/// One load's runtime state. The current feed lives here, not in the definition,
/// because a `Route` moves it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoadState {
    /// The order: is the device being asked to run? For a door that means its
    /// mechanism is enabled, not that it is open.
    pub commanded: bool,
    pub condition: Condition,
    /// Where it is wired now.
    pub feed: FeedId,
    /// Supply is reaching it (through its feed, or through a backup).
    pub energized: bool,
    /// It is actually running.
    pub operating: bool,
    /// Warm-up ticks left after supply returned.
    pub restart_left: u32,
    /// The first rung of the ladder that denied it, if any. Derived every tick —
    /// carried in the snapshot so the reason survives a save and never has to be
    /// re-derived against a different readout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<OffReason>,
}

/// One hazard's runtime state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HazardState {
    pub live: bool,
    /// Somebody is currently being held in it; a second shock waits for them to
    /// get clear and come back.
    pub touching: bool,
}

/// The complete runtime state — pure serde, so a snapshot restores exactly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PowerState {
    pub tick: u64,
    pub sources: Vec<SourceState>,
    pub feeds: Vec<FeedState>,
    pub breakers: Vec<BreakerState>,
    pub loads: Vec<LoadState>,
    pub hazards: Vec<HazardState>,
}

impl PowerState {
    /// The state a fresh run starts in.
    ///
    /// Supply is settled here rather than at the first tick: a source that is on
    /// and quick is already delivering, and a feed with a closed breaker is
    /// already live. Otherwise an automatic backup would see the whole floor as
    /// dark on tick one and spend a tick of runtime for nothing.
    pub fn initial(def: &PowerNetworkDef) -> Self {
        let source_up: Vec<bool> = def
            .sources
            .iter()
            .map(|s| s.start_on && s.startup_ticks == 0)
            .collect();
        let sources = def
            .sources
            .iter()
            .enumerate()
            .map(|(i, s)| SourceState {
                on: s.start_on,
                // A backup delivers nothing until something is actually lost.
                output: source_up.get(i).copied().unwrap_or(false) && s.backup.is_none(),
                runtime_left: s.backup.as_ref().map(|b| b.runtime_ticks).unwrap_or(0),
                startup_left: if s.start_on { s.startup_ticks } else { 0 },
                spent: false,
                engaged: false,
            })
            .collect();
        let feeds = def
            .feeds
            .iter()
            .map(|f| {
                let breaker_open = def
                    .breakers
                    .iter()
                    .find(|b| b.id == f.breaker)
                    .map(|b| b.start_open)
                    .unwrap_or(true);
                let primary_up = def
                    .source_index(&f.source)
                    .and_then(|i| source_up.get(i))
                    .copied()
                    .unwrap_or(false);
                FeedState {
                    cut: false,
                    live: !breaker_open && primary_up,
                }
            })
            .collect();
        Self {
            tick: 0,
            sources,
            feeds,
            breakers: def
                .breakers
                .iter()
                .map(|b| BreakerState {
                    open: b.start_open,
                    fault: Fault::None,
                    overload_ticks: 0,
                })
                .collect(),
            loads: def
                .loads
                .iter()
                .map(|l| LoadState {
                    commanded: l.start_on,
                    condition: l.condition,
                    feed: l.feed.clone(),
                    energized: false,
                    operating: false,
                    restart_left: 0,
                    why: Some(OffReason::Unpowered {
                        feed: l.feed.clone(),
                    }),
                })
                .collect(),
            hazards: def
                .hazards
                .iter()
                .map(|_| HazardState {
                    live: false,
                    touching: false,
                })
                .collect(),
        }
    }

    /// The feed each load is wired to, in definition order — the runtime
    /// assignment that [`PowerNetworkDef::connected_draw`] needs.
    pub fn assignment(&self) -> Vec<FeedId> {
        self.loads.iter().map(|l| l.feed.clone()).collect()
    }

    /// The connected draw of `feed` under the current assignment.
    pub fn connected_draw(&self, def: &PowerNetworkDef, feed: &FeedId) -> u32 {
        def.connected_draw(feed, &self.assignment())
    }

    /// Whether a feed's *primary* source is delivering.
    fn primary_output(&self, def: &PowerNetworkDef, feed: &FeedId) -> bool {
        let Some(f) = def.feed(feed) else {
            return false;
        };
        match def.source_index(&f.source) {
            Some(i) => self.sources.get(i).map(|s| s.output).unwrap_or(false),
            None => false,
        }
    }

    /// Whether a breaker is open. A feed whose breaker is missing is treated as
    /// open: protection that cannot be found does not pass current.
    fn breaker_open(&self, def: &PowerNetworkDef, feed: &FeedId) -> bool {
        match def.breaker_of(feed) {
            Some(i) => self.breakers.get(i).map(|b| b.open).unwrap_or(true),
            None => true,
        }
    }

    /// Whether a feed can deliver this tick.
    pub fn feed_live(&self, def: &PowerNetworkDef, feed: &FeedId) -> bool {
        let Some(fi) = def.feed_index(feed) else {
            return false;
        };
        if self.feeds.get(fi).map(|f| f.cut).unwrap_or(true) {
            return false;
        }
        if self.breaker_open(def, feed) {
            return false;
        }
        self.primary_output(def, feed)
    }

    /// Whether some backup is currently carrying `load`.
    fn backup_delivering(&self, def: &PowerNetworkDef, load: &LoadId) -> bool {
        def.sources.iter().enumerate().any(|(i, s)| {
            s.backup.as_ref().is_some_and(|b| b.serves.contains(load))
                && self.sources.get(i).is_some_and(|st| st.output)
        })
    }

    /// Whether supply is reaching load `i` — through its feed, or a backup.
    pub fn load_energized(&self, def: &PowerNetworkDef, i: usize) -> bool {
        let Some(s) = self.loads.get(i) else {
            return false;
        };
        let Some(ldef) = def.loads.get(i) else {
            return false;
        };
        self.feed_live(def, &s.feed) || self.backup_delivering(def, &ldef.id)
    }

    /// Every signal device a dead load makes inert.
    ///
    /// This is the seam the signal network reads: an inert device cannot emit an
    /// output (a blind camera does not trip) and its inputs are ignored, so no
    /// wire and no command can make it work.
    pub fn inert_signals(&self, def: &PowerNetworkDef) -> BTreeSet<SignalId> {
        def.loads
            .iter()
            .zip(&self.loads)
            .filter(|(_, s)| !s.operating)
            .filter_map(|(l, _)| l.signal.clone())
            .collect()
    }

    /// Why load `i` is off, walking the ladder. `None` means it is running.
    fn why_off(
        &self,
        def: &PowerNetworkDef,
        i: usize,
        readout: &PowerReadout,
        was_operating: &[bool],
    ) -> Option<OffReason> {
        let ldef = def.loads.get(i)?;
        let s = self.loads.get(i)?;

        // 1-2. The device itself.
        if s.condition == Condition::Damaged {
            return Some(OffReason::Damaged);
        }
        if s.condition == Condition::Disconnected {
            return Some(OffReason::Disconnected);
        }

        // 3. Its local safeties, read against the start of the tick.
        let held = def.interlocks.iter().find(|il| {
            il.load == ldef.id
                && match &il.holds_when {
                    InterlockWhen::LoadOperating(other) => def
                        .load_index(other)
                        .and_then(|oi| was_operating.get(oi))
                        .copied()
                        .unwrap_or(false),
                    InterlockWhen::SignalActive(sig) => readout.signal_active.contains(sig),
                    InterlockWhen::SignalLocked(sig) => readout.signal_locked.contains(sig),
                }
        });
        if let Some(il) = held {
            return Some(OffReason::Interlock {
                interlock: il.id.clone(),
                label: il.label.clone(),
            });
        }

        // 4. Supply that is gone for good.
        let cut = def
            .feed_index(&s.feed)
            .and_then(|fi| self.feeds.get(fi))
            .map(|f| f.cut)
            .unwrap_or(false);
        if cut {
            return Some(OffReason::Unavailable {
                feed: s.feed.clone(),
            });
        }

        // 5. Its protection.
        if let Some(bi) = def.breaker_of(&s.feed)
            && let Some(b) = self.breakers.get(bi)
            && b.open
        {
            return Some(OffReason::Isolated {
                breaker: b.id.clone(),
            });
        }

        // 6. Its supply, right now.
        if !s.energized {
            return Some(OffReason::Unpowered {
                feed: s.feed.clone(),
            });
        }

        // 7. Its warm-up.
        if s.restart_left > 0 {
            return Some(OffReason::Restarting {
                ticks: s.restart_left,
            });
        }

        // 8. The order.
        if !s.commanded {
            return Some(OffReason::NoCommand);
        }

        None
    }

    /// The full account of one device: what it is, what it is doing, and why.
    pub fn report(&self, def: &PowerNetworkDef, load: &LoadId) -> Option<LoadReport> {
        let i = def.load_index(load)?;
        let ldef = def.loads.get(i)?;
        let s = self.loads.get(i)?;
        let isolated = def
            .breaker_of(&s.feed)
            .and_then(|bi| self.breakers.get(bi))
            .map(|b| b.open)
            .unwrap_or(false);
        let interlocked = match &s.why {
            Some(OffReason::Interlock { interlock, .. }) => Some(interlock.clone()),
            _ => None,
        };
        Some(LoadReport {
            load: ldef.id.clone(),
            label: ldef.label.clone(),
            kind: ldef.kind,
            feed: s.feed.clone(),
            condition: s.condition,
            energized: s.energized,
            commanded: s.commanded,
            isolated,
            interlocked,
            restart_left: s.restart_left,
            draw: ldef.draw,
            capacity: def.feed(&s.feed).map(|f| f.capacity).unwrap_or(0),
            operating: s.operating,
            why: s.why.clone(),
        })
    }

    /// Apply one command, returning what it produced.
    pub fn apply(
        &mut self,
        def: &PowerNetworkDef,
        cmd: &PowerCommand,
        access: &PowerAccess,
    ) -> Vec<PowerEvent> {
        let by = cmd.seat();
        let result = match cmd {
            PowerCommand::CutCable { feed, .. } => self.cut_cable(def, by, feed, access),
            PowerCommand::Route { load, feed, .. } => self.route(def, by, load, feed, access),
            PowerCommand::Isolate { breaker, open, .. } => {
                self.isolate(def, by, breaker, *open, access)
            }
            PowerCommand::SwitchSource { source, on, .. } => {
                self.switch_source(def, by, source, *on, access)
            }
            PowerCommand::SetCommand { load, on, .. } => {
                self.set_command(def, by, load, *on, access)
            }
            PowerCommand::Inspect { load, .. } => self.inspect(def, by, load, access),
        };
        result.unwrap_or_else(|reason| vec![PowerEvent::Denied { by, reason }])
    }

    fn cut_cable(
        &mut self,
        def: &PowerNetworkDef,
        by: Seat,
        feed: &FeedId,
        access: &PowerAccess,
    ) -> Result<Vec<PowerEvent>, PowerDenyReason> {
        if by != Seat::Infiltrator {
            return Err(PowerDenyReason::NotPhysical);
        }
        if !access.can(by, feed.as_str()) {
            return Err(PowerDenyReason::NoAccess {
                element: feed.0.clone(),
            });
        }
        let Some(fi) = def.feed_index(feed) else {
            return Err(PowerDenyReason::NoAccess {
                element: feed.0.clone(),
            });
        };
        let mut loads_lost = 0usize;
        for l in &self.loads {
            if &l.feed == feed {
                loads_lost += 1;
            }
        }
        let Some(fs) = self.feeds.get(fi) else {
            return Err(PowerDenyReason::NoAccess {
                element: feed.0.clone(),
            });
        };
        if fs.cut {
            return Err(PowerDenyReason::AlreadyCut { feed: feed.clone() });
        }
        if let Some(fs) = self.feeds.get_mut(fi) {
            fs.cut = true;
        }
        // The arc takes the protection out with it.
        if let Some(bi) = def.breaker_of(feed)
            && let Some(b) = self.breakers.get_mut(bi)
        {
            b.open = true;
            b.fault = Fault::ConductorCut;
            b.overload_ticks = 0;
        }
        Ok(vec![PowerEvent::CableCut {
            feed: feed.clone(),
            by,
            cost: cut_cost(def, by),
            loads_lost,
        }])
    }

    fn route(
        &mut self,
        def: &PowerNetworkDef,
        by: Seat,
        load: &LoadId,
        to: &FeedId,
        access: &PowerAccess,
    ) -> Result<Vec<PowerEvent>, PowerDenyReason> {
        if !access.can(by, load.as_str()) {
            return Err(PowerDenyReason::NoAccess {
                element: load.0.clone(),
            });
        }
        let Some(li) = def.load_index(load) else {
            return Err(PowerDenyReason::NoAccess {
                element: load.0.clone(),
            });
        };
        if def.feed_index(to).is_none() {
            return Err(PowerDenyReason::NoAccess {
                element: to.0.clone(),
            });
        }
        let panel_is_remote = def.loads.get(li).is_some_and(|l| l.node.is_some());
        if by == Seat::Hacker && !panel_is_remote {
            return Err(PowerDenyReason::RemotePanel {
                element: load.0.clone(),
            });
        }
        let Some(s) = self.loads.get_mut(li) else {
            return Err(PowerDenyReason::NoAccess {
                element: load.0.clone(),
            });
        };
        if &s.feed == to {
            return Err(PowerDenyReason::SameFeed { load: load.clone() });
        }
        let from = s.feed.clone();
        s.feed = to.clone();
        // Rewiring a disconnected device is what reconnects it.
        if s.condition == Condition::Disconnected {
            s.condition = Condition::Intact;
        }
        // A fresh connection starts from rest; nothing resumes by itself.
        s.restart_left = 0;
        Ok(vec![PowerEvent::Routed {
            load: load.clone(),
            from,
            to: to.clone(),
            by,
            cost: route_cost(def, by),
        }])
    }

    fn isolate(
        &mut self,
        def: &PowerNetworkDef,
        by: Seat,
        breaker: &BreakerId,
        open: bool,
        access: &PowerAccess,
    ) -> Result<Vec<PowerEvent>, PowerDenyReason> {
        if !access.can(by, breaker.as_str()) {
            return Err(PowerDenyReason::NoAccess {
                element: breaker.0.clone(),
            });
        }
        let Some(bi) = def.breaker_index(breaker) else {
            return Err(PowerDenyReason::NoAccess {
                element: breaker.0.clone(),
            });
        };
        let Some(bdef) = def.breakers.get(bi) else {
            return Err(PowerDenyReason::NoAccess {
                element: breaker.0.clone(),
            });
        };
        if by == Seat::Hacker && !bdef.remote {
            return Err(PowerDenyReason::RemotePanel {
                element: breaker.0.clone(),
            });
        }
        let feed = bdef.feed.clone();
        let cost = isolate_cost(def, by);
        let Some(b) = self.breakers.get_mut(bi) else {
            return Err(PowerDenyReason::NoAccess {
                element: breaker.0.clone(),
            });
        };
        if open {
            b.open = true;
            b.overload_ticks = 0;
            let fault = b.fault;
            Ok(vec![PowerEvent::Isolated {
                breaker: breaker.clone(),
                feed,
                by,
                cost,
                fault,
            }])
        } else {
            if b.fault != Fault::None {
                return Err(PowerDenyReason::FaultPresent { fault: b.fault });
            }
            b.open = false;
            b.overload_ticks = 0;
            Ok(vec![PowerEvent::Reset {
                breaker: breaker.clone(),
                feed,
                by,
                cost,
            }])
        }
    }

    fn switch_source(
        &mut self,
        def: &PowerNetworkDef,
        by: Seat,
        source: &PowerSourceId,
        on: bool,
        access: &PowerAccess,
    ) -> Result<Vec<PowerEvent>, PowerDenyReason> {
        if !access.can(by, source.as_str()) {
            return Err(PowerDenyReason::NoAccess {
                element: source.0.clone(),
            });
        }
        let Some(si) = def.source_index(source) else {
            return Err(PowerDenyReason::NoAccess {
                element: source.0.clone(),
            });
        };
        let Some(sdef) = def.sources.get(si) else {
            return Err(PowerDenyReason::NoAccess {
                element: source.0.clone(),
            });
        };
        if by == Seat::Hacker && !sdef.remote {
            return Err(PowerDenyReason::RemotePanel {
                element: source.0.clone(),
            });
        }
        let startup = sdef.startup_ticks;
        let Some(s) = self.sources.get_mut(si) else {
            return Err(PowerDenyReason::NoAccess {
                element: source.0.clone(),
            });
        };
        if on && s.spent {
            return Err(PowerDenyReason::BackupExhausted {
                source: source.clone(),
            });
        }
        s.on = on;
        s.startup_left = if on { startup } else { 0 };
        if !on {
            s.engaged = false;
            s.output = false;
        }
        Ok(vec![PowerEvent::Switched {
            source: source.clone(),
            on,
            by,
            cost: source_cost(def, by),
        }])
    }

    fn set_command(
        &mut self,
        def: &PowerNetworkDef,
        by: Seat,
        load: &LoadId,
        on: bool,
        access: &PowerAccess,
    ) -> Result<Vec<PowerEvent>, PowerDenyReason> {
        if !access.can(by, load.as_str()) {
            return Err(PowerDenyReason::NoAccess {
                element: load.0.clone(),
            });
        }
        let Some(li) = def.load_index(load) else {
            return Err(PowerDenyReason::NoAccess {
                element: load.0.clone(),
            });
        };
        let Some(state) = self.loads.get_mut(li) else {
            return Err(PowerDenyReason::NoAccess {
                element: load.0.clone(),
            });
        };
        // A command is a request. It changes what the device is *asked* to do
        // and nothing else: whether it runs is settled next tick, by the ladder.
        state.commanded = on;
        Ok(vec![PowerEvent::Commanded {
            load: load.clone(),
            on,
            by,
            cost: command_cost(def, by),
        }])
    }

    fn inspect(
        &mut self,
        def: &PowerNetworkDef,
        by: Seat,
        load: &LoadId,
        access: &PowerAccess,
    ) -> Result<Vec<PowerEvent>, PowerDenyReason> {
        if !access.can(by, load.as_str()) {
            return Err(PowerDenyReason::NoAccess {
                element: load.0.clone(),
            });
        }
        match self.report(def, load) {
            Some(report) => Ok(vec![PowerEvent::Inspected {
                by,
                load: load.clone(),
                report: Box::new(report),
            }]),
            None => Err(PowerDenyReason::NoAccess {
                element: load.0.clone(),
            }),
        }
    }

    /// One tick against the host's readout.
    pub fn step(&mut self, def: &PowerNetworkDef, readout: &PowerReadout) -> Vec<PowerEvent> {
        let mut events: Vec<PowerEvent> = Vec::new();
        self.tick += 1;

        // Which served loads are off supply at the start of the tick — a backup
        // engages against this, never against its own effect.
        let unserved: Vec<bool> = (0..def.loads.len())
            .map(|i| !self.load_energized(def, i))
            .collect();

        // 1. Sources.
        for (i, sdef) in def.sources.iter().enumerate() {
            let Some(s) = self.sources.get_mut(i) else {
                continue;
            };
            if s.startup_left > 0 {
                s.startup_left -= 1;
            }
            match &sdef.backup {
                None => {
                    s.engaged = false;
                    s.output = s.on && s.startup_left == 0;
                }
                Some(spec) => {
                    let ready = s.on && !s.spent && s.startup_left == 0 && s.runtime_left > 0;
                    let wanted = match spec.mode {
                        BackupMode::Automatic => {
                            ready
                                && spec.serves.iter().any(|l| {
                                    def.load_index(l)
                                        .and_then(|li| unserved.get(li))
                                        .copied()
                                        .unwrap_or(false)
                                })
                        }
                        BackupMode::Manual => ready,
                    };
                    let was = s.engaged;
                    s.engaged = wanted;
                    s.output = wanted;
                    if wanted {
                        s.runtime_left = s.runtime_left.saturating_sub(1);
                        if s.runtime_left == 0 {
                            s.spent = true;
                            s.engaged = false;
                            s.output = false;
                            events.push(PowerEvent::BackupSpent {
                                source: sdef.id.clone(),
                            });
                        } else if !was {
                            events.push(PowerEvent::BackupEngaged {
                                source: sdef.id.clone(),
                                loads: spec.serves.len(),
                                runtime_ticks: s.runtime_left,
                            });
                        }
                    }
                }
            }
        }

        // 2. Protection. Draw is read before the mutable pass so no breaker sees
        // another breaker's arithmetic.
        let draws: Vec<u32> = def
            .feeds
            .iter()
            .map(|f| self.connected_draw(def, &f.id))
            .collect();
        for (i, bdef) in def.breakers.iter().enumerate() {
            let Some(fi) = def.feed_index(&bdef.feed) else {
                continue;
            };
            let capacity = def.feeds.get(fi).map(|f| f.capacity).unwrap_or(0);
            let draw = draws.get(fi).copied().unwrap_or(0);
            let Some(b) = self.breakers.get_mut(i) else {
                continue;
            };
            // An overload fault clears the moment the rating is respected again,
            // open or closed: the fault is the overload, not the trip. A cut
            // conductor never clears, because this run has no splice.
            if b.fault == Fault::Overload && draw <= capacity {
                b.fault = Fault::None;
            }
            if b.open {
                b.overload_ticks = 0;
                continue;
            }
            if draw > capacity {
                b.overload_ticks += 1;
                if b.overload_ticks == 1 {
                    events.push(PowerEvent::Overloaded {
                        feed: bdef.feed.clone(),
                        draw,
                        capacity,
                        trip_ticks: def.tuning.trip_ticks,
                    });
                }
                if b.overload_ticks >= def.tuning.trip_ticks.max(1) {
                    b.open = true;
                    b.fault = Fault::Overload;
                    events.push(PowerEvent::Tripped {
                        breaker: bdef.id.clone(),
                        feed: bdef.feed.clone(),
                        fault: Fault::Overload,
                    });
                }
            } else {
                b.overload_ticks = 0;
            }
        }

        // 3. Feeds.
        let mut moved: BTreeSet<FeedId> = BTreeSet::new();
        for (i, fdef) in def.feeds.iter().enumerate() {
            let live = self.feed_live(def, &fdef.id);
            let loads = self.loads.iter().filter(|l| l.feed == fdef.id).count();
            let Some(fs) = self.feeds.get_mut(i) else {
                continue;
            };
            let was = fs.live;
            fs.live = live;
            if was != live {
                moved.insert(fdef.id.clone());
                if live {
                    events.push(PowerEvent::FeedRestored {
                        feed: fdef.id.clone(),
                    });
                } else {
                    events.push(PowerEvent::FeedLost {
                        feed: fdef.id.clone(),
                        loads,
                    });
                }
            }
        }

        // 4. Loads. First the transitions, then the reasons.
        let was_operating: Vec<bool> = self.loads.iter().map(|l| l.operating).collect();
        for (i, ldef) in def.loads.iter().enumerate() {
            let energized = self.load_energized(def, i);
            let Some(s) = self.loads.get_mut(i) else {
                continue;
            };
            let was = s.energized;
            s.energized = energized;
            if was && !energized {
                // A fail-safe device drops the order: it is not waiting to
                // spring back to life when somebody else restores supply.
                if ldef.restart == Restart::FailSafe {
                    s.commanded = false;
                }
                s.restart_left = 0;
            } else if !was && energized {
                s.restart_left = match ldef.restart {
                    Restart::Resume => ldef.restart_ticks.unwrap_or(def.tuning.restart_ticks),
                    Restart::FailSafe | Restart::Hold => 0,
                };
            } else if s.restart_left > 0 {
                s.restart_left -= 1;
            }
        }
        for (i, ldef) in def.loads.iter().enumerate() {
            let why = self.why_off(def, i, readout, &was_operating);
            let Some(s) = self.loads.get_mut(i) else {
                continue;
            };
            let was = was_operating.get(i).copied().unwrap_or(false);
            s.operating = why.is_none();
            s.why = why.clone();
            // A feed transition already said its piece once; this reports only
            // what a feed going up or down does not explain.
            if s.operating != was && !moved.contains(&s.feed) {
                events.push(PowerEvent::LoadChanged {
                    load: ldef.id.clone(),
                    operating: s.operating,
                    why,
                });
            }
        }

        // 5. Hazards.
        for (i, hdef) in def.hazards.iter().enumerate() {
            let live = match hdef.live {
                HazardLive::FeedEnergized => self.feed_live(def, &hdef.feed),
                HazardLive::FeedCut => def
                    .feed_index(&hdef.feed)
                    .and_then(|fi| self.feeds.get(fi))
                    .map(|f| f.cut)
                    .unwrap_or(false),
            };
            let touching = readout
                .body
                .is_some_and(|b| b.room == hdef.room && b.x >= hdef.x.0 && b.x <= hdef.x.1);
            let shock = live && touching;
            let Some(h) = self.hazards.get_mut(i) else {
                continue;
            };
            let was = h.touching;
            h.live = live;
            h.touching = shock;
            if shock && !was {
                events.push(PowerEvent::Shocked {
                    hazard: hdef.id.clone(),
                    room: hdef.room.clone(),
                    ticks: hdef.shock_ticks,
                });
            }
        }

        events
    }
}

fn costs(def: &PowerNetworkDef, by: Seat) -> &PowerCosts {
    match by {
        Seat::Infiltrator => &def.tuning.infiltrator,
        Seat::Hacker => &def.tuning.hacker,
    }
}

fn cut_cost(def: &PowerNetworkDef, by: Seat) -> WorkCost {
    let c = costs(def, by);
    WorkCost {
        bandwidth: 0,
        busy_ticks: c.cut_busy_ticks,
        noise: c.cut_noise,
    }
}

fn route_cost(def: &PowerNetworkDef, by: Seat) -> WorkCost {
    let c = costs(def, by);
    WorkCost {
        bandwidth: c.route_bandwidth,
        busy_ticks: c.route_busy_ticks,
        noise: 0.0,
    }
}

fn isolate_cost(def: &PowerNetworkDef, by: Seat) -> WorkCost {
    let c = costs(def, by);
    WorkCost {
        bandwidth: c.isolate_bandwidth,
        busy_ticks: c.isolate_busy_ticks,
        noise: 0.0,
    }
}

fn source_cost(def: &PowerNetworkDef, by: Seat) -> WorkCost {
    let c = costs(def, by);
    WorkCost {
        bandwidth: c.source_bandwidth,
        busy_ticks: c.source_busy_ticks,
        noise: 0.0,
    }
}

fn command_cost(def: &PowerNetworkDef, by: Seat) -> WorkCost {
    let c = costs(def, by);
    WorkCost {
        bandwidth: c.command_bandwidth,
        busy_ticks: c.command_busy_ticks,
        noise: 0.0,
    }
}

/// A restorable snapshot: the definition plus the runtime state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PowerSnapshot {
    pub format: String,
    pub definition: PowerNetworkDef,
    pub state: PowerState,
}

/// The deterministic power runtime.
#[derive(Debug, Clone)]
pub struct PowerSim {
    def: PowerNetworkDef,
    state: PowerState,
}

impl PowerSim {
    /// Start a run. Validates the definition first.
    pub fn new(def: PowerNetworkDef) -> Result<Self, Vec<PowerValidationError>> {
        def.ok()?;
        let state = PowerState::initial(&def);
        Ok(Self { def, state })
    }

    /// Restore a run from a snapshot, gating on its format tag.
    pub fn restore(snapshot: PowerSnapshot) -> Result<Self, Vec<PowerValidationError>> {
        if snapshot.format != POWER_SNAPSHOT_FORMAT {
            return Err(vec![PowerValidationError::UnsupportedSnapshotFormat {
                found: snapshot.format,
            }]);
        }
        snapshot.definition.ok()?;
        let d = &snapshot.definition;
        let n = d.loads.len();
        let st = &snapshot.state;
        let mismatched = st.loads.len() != n
            || st.sources.len() != d.sources.len()
            || st.feeds.len() != d.feeds.len()
            || st.breakers.len() != d.breakers.len()
            || st.hazards.len() != d.hazards.len();
        if mismatched {
            return Err(vec![PowerValidationError::UnsupportedSnapshotFormat {
                found: "state does not match its definition".into(),
            }]);
        }
        Ok(Self {
            def: snapshot.definition,
            state: snapshot.state,
        })
    }

    /// A snapshot of the run.
    pub fn snapshot(&self) -> PowerSnapshot {
        PowerSnapshot {
            format: POWER_SNAPSHOT_FORMAT.to_owned(),
            definition: self.def.clone(),
            state: self.state.clone(),
        }
    }

    /// The definition being played.
    pub fn definition(&self) -> &PowerNetworkDef {
        &self.def
    }

    /// The current state.
    pub fn state(&self) -> &PowerState {
        &self.state
    }

    /// Apply one command under `access`, returning what it produced.
    pub fn apply(&mut self, cmd: &PowerCommand, access: &PowerAccess) -> Vec<PowerEvent> {
        self.state.apply(&self.def, cmd, access)
    }

    /// Advance one tick against the host's readout.
    pub fn step(&mut self, readout: &PowerReadout) -> Vec<PowerEvent> {
        self.state.step(&self.def, readout)
    }

    /// The full account of one device, or `None` if the network has no such load.
    pub fn report(&self, load: &LoadId) -> Option<LoadReport> {
        self.state.report(&self.def, load)
    }

    /// Every signal device this network is currently holding inert.
    pub fn inert_signals(&self) -> BTreeSet<SignalId> {
        self.state.inert_signals(&self.def)
    }

    /// The tuning this network plays with.
    pub fn tuning(&self) -> &PowerTuning {
        &self.def.tuning
    }
}

/// A load addressed by its position in [`PowerNetworkDef::loads`], a feed by its
/// position in `feeds`, and so on — how a wire [`crate::scenario::Command`] names
/// one, so the command stays `Copy` and fixed-size, exactly as `SignalIndex`
/// does for the signal network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PowerIndex(pub u16);

/// The wire form of a [`PowerCommand`]: the same verbs with elements by index.
/// The namespace is the variant's own, so one index type serves all five lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum PowerOp {
    CutCable {
        by: Seat,
        feed: PowerIndex,
    },
    Route {
        by: Seat,
        load: PowerIndex,
        feed: PowerIndex,
    },
    Isolate {
        by: Seat,
        breaker: PowerIndex,
        open: bool,
    },
    SwitchSource {
        by: Seat,
        source: PowerIndex,
        on: bool,
    },
    SetCommand {
        by: Seat,
        load: PowerIndex,
        on: bool,
    },
    Inspect {
        by: Seat,
        load: PowerIndex,
    },
}

impl PowerOp {
    /// The seat issuing the op.
    pub fn seat(&self) -> Seat {
        match self {
            PowerOp::CutCable { by, .. }
            | PowerOp::Route { by, .. }
            | PowerOp::Isolate { by, .. }
            | PowerOp::SwitchSource { by, .. }
            | PowerOp::SetCommand { by, .. }
            | PowerOp::Inspect { by, .. } => *by,
        }
    }

    /// Resolve element indices against `def`, or name the first bad index.
    pub fn resolve(&self, def: &PowerNetworkDef) -> Result<PowerCommand, PowerDenyReason> {
        let feed = |i: PowerIndex| {
            def.feeds
                .get(usize::from(i.0))
                .map(|f| f.id.clone())
                .ok_or(PowerDenyReason::NoSuchElement { index: i.0 })
        };
        let load = |i: PowerIndex| {
            def.loads
                .get(usize::from(i.0))
                .map(|l| l.id.clone())
                .ok_or(PowerDenyReason::NoSuchElement { index: i.0 })
        };
        let breaker = |i: PowerIndex| {
            def.breakers
                .get(usize::from(i.0))
                .map(|b| b.id.clone())
                .ok_or(PowerDenyReason::NoSuchElement { index: i.0 })
        };
        let source = |i: PowerIndex| {
            def.sources
                .get(usize::from(i.0))
                .map(|s| s.id.clone())
                .ok_or(PowerDenyReason::NoSuchElement { index: i.0 })
        };
        Ok(match *self {
            PowerOp::CutCable { by, feed: f } => PowerCommand::CutCable { by, feed: feed(f)? },
            PowerOp::Route {
                by,
                load: l,
                feed: f,
            } => PowerCommand::Route {
                by,
                load: load(l)?,
                feed: feed(f)?,
            },
            PowerOp::Isolate {
                by,
                breaker: b,
                open,
            } => PowerCommand::Isolate {
                by,
                breaker: breaker(b)?,
                open,
            },
            PowerOp::SwitchSource { by, source: s, on } => PowerCommand::SwitchSource {
                by,
                source: source(s)?,
                on,
            },
            PowerOp::SetCommand { by, load: l, on } => PowerCommand::SetCommand {
                by,
                load: load(l)?,
                on,
            },
            PowerOp::Inspect { by, load: l } => PowerCommand::Inspect { by, load: load(l)? },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ladder_reads_upstream_from_the_device() {
        assert_eq!(
            OffReason::Unpowered {
                feed: "feed-a".into()
            }
            .phrase(),
            "unpowered (feed-a)"
        );
        assert_eq!(OffReason::NoCommand.phrase(), "no command");
    }

    #[test]
    fn work_costs_are_per_seat_and_never_free_by_accident() {
        let mut def = PowerNetworkDef {
            format: super::super::POWER_FORMAT.into(),
            network_id: "t".into(),
            name: "t".into(),
            sources: vec![],
            feeds: vec![],
            loads: vec![],
            breakers: vec![],
            interlocks: vec![],
            hazards: vec![],
            tuning: PowerTuning::default(),
        };
        def.tuning.infiltrator.cut_busy_ticks = 120;
        def.tuning.hacker.cut_busy_ticks = 0;
        assert_eq!(cut_cost(&def, Seat::Infiltrator).busy_ticks, 120);
        assert!(cut_cost(&def, Seat::Hacker).busy_ticks == 0);
    }

    #[test]
    fn a_power_op_resolves_its_own_namespace() {
        let def = super::super::test_floor::power_test_floor();
        let op = PowerOp::SetCommand {
            by: Seat::Hacker,
            load: PowerIndex(0),
            on: true,
        };
        let cmd = op.resolve(&def).expect("index 0 is a load");
        assert!(matches!(cmd, PowerCommand::SetCommand { on: true, .. }));
        let bad = PowerOp::SetCommand {
            by: Seat::Hacker,
            load: PowerIndex(999),
            on: true,
        };
        assert_eq!(
            bad.resolve(&def),
            Err(PowerDenyReason::NoSuchElement { index: 999 })
        );
    }
}

//! The floor's electrical infrastructure: sources, distribution, loads,
//! protection, backup supply and exposed power.
//!
//! The signal network ([`crate::scenario::signal`]) answers *what happens when a
//! command lands*; the grounded network ([`crate::netsim`]) answers *who can
//! reach what*. This module answers the question underneath both: **is there
//! any power, and is the device allowed to run?** See ADR-0013.
//!
//! The model is deliberately small. It is not a circuit solver: no volts, no
//! amps, no impedance, no inrush. Every quantity is an abstract integer with a
//! readable threshold — a feed's capacity in "draw units", a backup's runtime in
//! "seconds" of ticks, a warm-up in ticks. The interesting question is never a
//! number, it is *which rule won*.
//!
//! # The six distinctions
//!
//! | Piece | Type | What it is for |
//! |---|---|---|
//! | Power source | [`SourceDef`] | where energy enters: mains, generator, battery |
//! | Distribution | [`FeedDef`] | the circuit that carries it, with a rating and a breaker |
//! | Load / device | [`LoadDef`] | what draws: a door, a camera, a fan, a pump |
//! | Backup supply | [`BackupSpec`] | a second source that serves named loads, for a while |
//! | Protective isolation | [`BreakerDef`] | the thing that opens before the wiring does, and needs a deliberate reset |
//! | Damage | [`Condition`] | intact, damaged, disconnected — and *unavailable* as a derived reason |
//!
//! Control signal stays where it was: the signal network. A load may name the
//! signal device it *is* ([`LoadDef::signal`]); a load that cannot run makes
//! that device inert, so a wire can never make a dead camera see.
//!
//! # The arbitration rule (one line)
//!
//! ```text
//! operating = commanded AND energized AND intact AND not interlocked AND warmed up
//! ```
//!
//! A command — manual or remote, either seat — is only ever *one* term of that
//! conjunction. It cannot beat a flat battery, a cut conductor, a tripped
//! breaker, an open panel interlock, or a broken pump. That is why
//! [`sim::OffReason`] reports the first term that failed rather than a boolean:
//! the sentence "off because the feed is dead" is the whole design.

pub mod sim;
pub mod test_floor;

use crate::scenario::ids::{BreakerId, FeedId, HazardId, LoadId, PowerSourceId, RoomId, SignalId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The power-network payload format this build reads and writes.
pub const POWER_FORMAT: &str = "idaptik-power/1";

/// What a source is. The kind is flavour and one rule: a mains supply is not a
/// backup, and a backup is the only thing that can carry a load past its feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// The utility supply. Off only when somebody throws it, or when the run
    /// starts with it thrown.
    Mains,
    /// An engine-driven set: comes up to speed over [`SourceDef::startup_ticks`].
    Genset,
    /// A battery or UPS: instant, and the only kind that normally carries a
    /// backup spec.
    Battery,
}

/// How a backup supply is brought in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupMode {
    /// Engages on its own the tick after any load it serves loses supply.
    Automatic,
    /// Engages only when a seat throws the source on.
    Manual,
}

/// A backup supply: a second source serving named loads for a stated runtime.
///
/// A backup does not feed a distribution circuit — it feeds the devices it names,
/// through its own socket. That keeps the model honest about UPSes and lets level
/// design say "this camera keeps watching for fifteen seconds" without pretending
/// the whole floor survives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackupSpec {
    /// Abstract runtime units: ticks of operation while carrying its loads.
    /// Readable as seconds at the fixed 60 Hz tick.
    pub runtime_ticks: u32,
    /// The loads this backup can carry. Never empty (validated).
    pub serves: Vec<LoadId>,
    pub mode: BackupMode,
}

/// An electrical source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceDef {
    pub id: PowerSourceId,
    pub label: String,
    pub kind: SourceKind,
    /// Whether it is already on when the run starts. Authored, never defaulted:
    /// a source that silently starts off is a level bug, not a convenience.
    pub start_on: bool,
    /// Ticks to come up to speed once on (0 for mains and batteries).
    pub startup_ticks: u32,
    /// Present iff this source is a backup. A source with a backup spec may not
    /// be any feed's primary source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup: Option<BackupSpec>,
    /// The room its switchgear stands in, if the infiltrator can reach it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<RoomId>,
    /// The grounded-network node its switchgear is on, if the hacker can reach it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    /// Whether a seat that can reach the node may work it remotely at all.
    /// A local-only board is a locked door for the hacker, by design.
    #[serde(default, skip_serializing_if = "is_false")]
    pub remote: bool,
}

/// A distribution feed — the power module's `CircuitDef`.
///
/// One feed is one circuit: it draws from exactly one primary source, carries a
/// rated capacity in abstract draw units, and is protected by exactly one
/// breaker. `building::CircuitDef` is the building-level version of the same
/// idea (one source node, one zone) and is untouched by this module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeedDef {
    pub id: FeedId,
    pub label: String,
    /// The primary source.
    pub source: PowerSourceId,
    /// Rated capacity, in draw units, against *connected* draw. Routing more
    /// onto the feed than it can carry trips its breaker after
    /// [`PowerTuning::trip_ticks`].
    pub capacity: u32,
    /// The one protective device on this feed. Required: an unprotected feed is
    /// not a feed, it is a fire waiting for a level designer.
    pub breaker: BreakerId,
    /// The room the cable run can be reached in (where it can be cut by hand).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<RoomId>,
    /// The grounded-network node the feed's board is on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
}

/// What a load physically is. Used for readable text and for level intent; the
/// simulation treats every load the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadKind {
    Door,
    Camera,
    Light,
    Sensor,
    Fan,
    Pump,
    Terminal,
    Vacuum,
    /// Anything else with a plug.
    Other,
}

impl LoadKind {
    /// The noun a log line uses.
    pub fn noun(self) -> &'static str {
        match self {
            LoadKind::Door => "door",
            LoadKind::Camera => "camera",
            LoadKind::Light => "lighting",
            LoadKind::Sensor => "sensor",
            LoadKind::Fan => "fan",
            LoadKind::Pump => "pump",
            LoadKind::Terminal => "terminal",
            LoadKind::Vacuum => "vacuum",
            LoadKind::Other => "device",
        }
    }
}

/// What a device does when power comes back. There is no "auto" default: level
/// design must say which of the three it means, or the floor will not validate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Restart {
    /// Loss of supply drops the command. Power coming back does **not** resume
    /// the device: it sits off until a fresh command arrives. The safe default
    /// for anything that moves — a door that re-opens on restore is a gift to
    /// the infiltrator.
    FailSafe,
    /// The command survives. Power coming back starts `restart_ticks` of warm-up
    /// and then the device resumes what it was told to do.
    Resume,
    /// The command survives and there is no warm-up: it resumes the tick power
    /// returns. For lights and instruments with nothing to spin up.
    Hold,
}

/// The physical condition of a load.
///
/// * `Intact` — it works if it is powered and told to.
/// * `Damaged` — it is broken. No command, no reset and no supply restores it
///   this run; it draws nothing.
/// * `Disconnected` — it is wired to nothing. A [`sim::PowerCommand::Route`]
///   reconnects it (that is what rewiring is for), nothing else does.
///
/// *Unavailable* is not a condition but a derived reason
/// ([`sim::OffReason::Unavailable`]): the supply is gone for the rest of the run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    #[default]
    Intact,
    Damaged,
    Disconnected,
}

/// A load: something that draws from a feed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoadDef {
    pub id: LoadId,
    pub label: String,
    pub kind: LoadKind,
    /// The feed it is wired to at the start of the run. A `Route` command can
    /// move it, which is why the current feed lives in the runtime state.
    pub feed: FeedId,
    /// Connected draw, in abstract units, whatever it is doing.
    pub draw: u32,
    /// The signal device this load *is*, if any. A load that cannot operate
    /// makes that device inert: no wire can make it see, open or speak.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<SignalId>,
    /// The room it stands in (the infiltrator can work its panel from here).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<RoomId>,
    pub restart: Restart,
    /// Whether it is switched on when the run starts.
    pub start_on: bool,
    #[serde(default, skip_serializing_if = "is_intact")]
    pub condition: Condition,
    /// Warm-up after supply returns, overriding [`PowerTuning::restart_ticks`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restart_ticks: Option<u32>,
    /// The grounded-network node its control panel is on, if it has one. A load
    /// with no node is *dumb*: the hacker cannot switch it remotely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
}

/// A protective device on one feed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BreakerDef {
    pub id: BreakerId,
    pub label: String,
    /// The feed it protects. Must agree with [`FeedDef::breaker`].
    pub feed: FeedId,
    /// Starts open: tripped or locked out. Closing it is a deliberate act.
    #[serde(default, skip_serializing_if = "is_false")]
    pub start_open: bool,
    /// Whether a seat that can reach it may work it remotely.
    #[serde(default, skip_serializing_if = "is_false")]
    pub remote: bool,
    /// The room its panel is in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<RoomId>,
    /// The grounded-network node its panel is on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
}

/// What holds an interlock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterlockWhen {
    /// Holds while another load is operating (an extract fan and the plant it
    /// serves, a pump and its circulation).
    LoadOperating(LoadId),
    /// Holds while a signal device is switched on.
    SignalActive(SignalId),
    /// Holds while a signal device is locked.
    SignalLocked(SignalId),
}

/// A safety interlock: a local rule that refuses a device permission to run.
///
/// An interlock is **above** the command and **below** physical fault: nothing a
/// player sends can defeat it, and it stops mattering the moment the device is
/// broken anyway. It is the third rung of the ladder in [`sim::OffReason`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterlockDef {
    /// Stable id, used in the reason the player is shown.
    pub id: String,
    pub label: String,
    /// The load it can hold off.
    pub load: LoadId,
    pub holds_when: InterlockWhen,
}

/// When an authored hazard is live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HazardLive {
    /// Live while the feed is energized: an exposed busbar, a missing panel.
    FeedEnergized,
    /// Live while the feed has been cut: a severed cable left fed from upstream.
    FeedCut,
}

/// An authored, spatially bounded exposed-power hazard.
///
/// It is only dangerous inside its own bounds, in its own room, and only while
/// [`HazardLive`] says it is live. Touching it throws the body clear for
/// `shock_ticks` and reports itself; it is not a damage model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HazardDef {
    pub id: HazardId,
    pub label: String,
    pub feed: FeedId,
    pub room: RoomId,
    /// Horizontal bounds, canvas units, body centre.
    pub x: (f64, f64),
    pub live: HazardLive,
    /// Ticks the body is thrown clear for.
    pub shock_ticks: u32,
}

/// What one seat's electrical work costs.
///
/// The infiltrator pays in hands (busy ticks) and noise; the hacker pays in
/// bandwidth. Physical work is the infiltrator's alone — no bandwidth buys a
/// cable cut.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PowerCosts {
    /// Cutting a conductor by hand.
    pub cut_busy_ticks: u32,
    /// Noise a cable cut makes (0..1, the same scale movement noise uses).
    pub cut_noise: f64,
    /// Moving a load's feed.
    pub route_bandwidth: u32,
    pub route_busy_ticks: u32,
    /// Opening or resetting a breaker.
    pub isolate_bandwidth: u32,
    pub isolate_busy_ticks: u32,
    /// Throwing a source.
    pub source_bandwidth: u32,
    pub source_busy_ticks: u32,
    /// Switching a single load.
    pub command_bandwidth: u32,
    pub command_busy_ticks: u32,
}

/// Per-network power tuning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PowerTuning {
    pub infiltrator: PowerCosts,
    pub hacker: PowerCosts,
    /// Warm-up ticks after supply returns, for a [`Restart::Resume`] load that
    /// does not override it.
    pub restart_ticks: u32,
    /// Ticks a feed may sit over its rating before its breaker trips.
    pub trip_ticks: u32,
}

impl Default for PowerTuning {
    /// At 60 Hz: a cut takes two seconds of hands, a warm-up is two seconds, a
    /// breaker allows three quarters of a second of overload. The infiltrator
    /// does the physical work; the hacker does the logical work, immediately,
    /// for bandwidth.
    fn default() -> Self {
        Self {
            infiltrator: PowerCosts {
                cut_busy_ticks: 120,
                cut_noise: 0.8,
                route_bandwidth: 0,
                route_busy_ticks: 90,
                isolate_bandwidth: 0,
                isolate_busy_ticks: 45,
                source_bandwidth: 0,
                source_busy_ticks: 30,
                command_bandwidth: 0,
                command_busy_ticks: 0,
            },
            hacker: PowerCosts {
                cut_busy_ticks: 0,
                cut_noise: 0.0,
                route_bandwidth: 8,
                route_busy_ticks: 0,
                isolate_bandwidth: 6,
                isolate_busy_ticks: 0,
                source_bandwidth: 5,
                source_busy_ticks: 0,
                command_bandwidth: 4,
                command_busy_ticks: 0,
            },
            restart_ticks: 120,
            trip_ticks: 45,
        }
    }
}

/// A complete power network: sources, feeds, loads, protection, interlocks and
/// authored hazards.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PowerNetworkDef {
    pub format: String,
    pub network_id: String,
    pub name: String,
    pub sources: Vec<SourceDef>,
    pub feeds: Vec<FeedDef>,
    pub loads: Vec<LoadDef>,
    pub breakers: Vec<BreakerDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub interlocks: Vec<InterlockDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hazards: Vec<HazardDef>,
    #[serde(default)]
    pub tuning: PowerTuning,
}

/// Why a power network definition is unusable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum PowerValidationError {
    UnsupportedFormat {
        found: String,
    },
    UnsupportedSnapshotFormat {
        found: String,
    },
    EmptyNetwork,
    /// An id used twice, whether in one collection or across two: seat access
    /// is one id space, so the runtime refuses to guess which is meant.
    DuplicateId {
        id: String,
    },
    FeedSourceMissing {
        feed: FeedId,
        source: PowerSourceId,
    },
    FeedBreakerMissing {
        feed: FeedId,
        breaker: BreakerId,
    },
    BreakerFeedMissing {
        breaker: BreakerId,
        feed: FeedId,
    },
    BreakerFeedMismatch {
        feed: FeedId,
        breaker: BreakerId,
    },
    FeedUnprotected {
        feed: FeedId,
    },
    LoadFeedMissing {
        load: LoadId,
        feed: FeedId,
    },
    HazardFeedMissing {
        hazard: HazardId,
        feed: FeedId,
    },
    HazardBoundsInverted {
        hazard: HazardId,
    },
    HazardShockTooShort {
        hazard: HazardId,
    },
    /// A backup that serves nothing, or runs for no time, is not a backup.
    BackupServesNothing {
        source: PowerSourceId,
    },
    BackupRuntimeZero {
        source: PowerSourceId,
    },
    BackupServesUnknownLoad {
        source: PowerSourceId,
        load: LoadId,
    },
    /// A source with a backup spec may not be a feed's primary source: the
    /// runtime resolves primary supply before backup engagement, and letting one
    /// source be both is how you write a circular definition.
    BackupDrivesFeed {
        source: PowerSourceId,
        feed: FeedId,
    },
    InterlockLoadMissing {
        interlock: String,
        load: LoadId,
    },
    InterlockSelfReference {
        interlock: String,
    },
    InterlockUnknownLoad {
        interlock: String,
        load: LoadId,
    },
}

impl PowerNetworkDef {
    /// The source with this id.
    pub fn source(&self, id: &PowerSourceId) -> Option<&SourceDef> {
        self.sources.iter().find(|s| &s.id == id)
    }

    /// The feed with this id.
    pub fn feed(&self, id: &FeedId) -> Option<&FeedDef> {
        self.feeds.iter().find(|f| &f.id == id)
    }

    /// The load with this id.
    pub fn load(&self, id: &LoadId) -> Option<&LoadDef> {
        self.loads.iter().find(|l| &l.id == id)
    }

    /// The index of a feed.
    pub fn feed_index(&self, id: &FeedId) -> Option<usize> {
        self.feeds.iter().position(|f| &f.id == id)
    }

    /// The index of a load.
    pub fn load_index(&self, id: &LoadId) -> Option<usize> {
        self.loads.iter().position(|l| &l.id == id)
    }

    /// The index of a breaker.
    pub fn breaker_index(&self, id: &BreakerId) -> Option<usize> {
        self.breakers.iter().position(|b| &b.id == id)
    }

    /// The index of a source.
    pub fn source_index(&self, id: &PowerSourceId) -> Option<usize> {
        self.sources.iter().position(|s| &s.id == id)
    }

    /// The breaker protecting `feed`, by index.
    pub fn breaker_of(&self, feed: &FeedId) -> Option<usize> {
        self.feed(feed)
            .and_then(|f| self.breaker_index(&f.breaker))
    }

    /// The closed-book connected draw of `feed` under the runtime assignment:
    /// the sum of every load's rating, whatever it is doing. Capacity is a
    /// rating against connected equipment, not instantaneous load — the abstract
    /// sim does not model inrush, and a number a player can read off the board is
    /// worth more than a number that changes sixty times a second.
    pub fn connected_draw(&self, feed: &FeedId, feeds: &[FeedId]) -> u32 {
        self.loads
            .iter()
            .enumerate()
            .filter(|(i, _)| feeds.get(*i) == Some(feed))
            .map(|(_, l)| l.draw)
            .sum()
    }

    /// Every id in the network, one id space, duplicates found.
    fn duplicate_id(&self) -> Option<String> {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut ids: Vec<String> = Vec::new();
        ids.extend(self.sources.iter().map(|s| s.id.0.clone()));
        ids.extend(self.feeds.iter().map(|f| f.id.0.clone()));
        ids.extend(self.loads.iter().map(|l| l.id.0.clone()));
        ids.extend(self.breakers.iter().map(|b| b.id.0.clone()));
        ids.extend(self.hazards.iter().map(|h| h.id.0.clone()));
        ids.extend(self.interlocks.iter().map(|i| i.id.clone()));
        for id in ids {
            if !seen.insert(id.clone()) {
                return Some(id);
            }
        }
        None
    }

    /// Every problem with the definition; empty means it is playable.
    pub fn validate(&self) -> Vec<PowerValidationError> {
        use PowerValidationError as E;
        let mut errs = Vec::new();
        if self.format != POWER_FORMAT {
            errs.push(E::UnsupportedFormat {
                found: self.format.clone(),
            });
        }
        if self.sources.is_empty() || self.feeds.is_empty() {
            errs.push(E::EmptyNetwork);
        }
        if let Some(id) = self.duplicate_id() {
            errs.push(E::DuplicateId { id });
        }

        // Sources: backups must be coherent.
        for s in &self.sources {
            if let Some(b) = &s.backup {
                if b.serves.is_empty() {
                    errs.push(E::BackupServesNothing {
                        source: s.id.clone(),
                    });
                }
                if b.runtime_ticks == 0 {
                    errs.push(E::BackupRuntimeZero {
                        source: s.id.clone(),
                    });
                }
                for load in &b.serves {
                    if self.load(load).is_none() {
                        errs.push(E::BackupServesUnknownLoad {
                            source: s.id.clone(),
                            load: load.clone(),
                        });
                    }
                }
            }
        }

        // Feeds: a source, a breaker, a rating, and no circular backup.
        for f in &self.feeds {
            match self.source(&f.source) {
                None => errs.push(E::FeedSourceMissing {
                    feed: f.id.clone(),
                    source: f.source.clone(),
                }),
                Some(s) if s.backup.is_some() => errs.push(E::BackupDrivesFeed {
                    source: s.id.clone(),
                    feed: f.id.clone(),
                }),
                Some(_) => {}
            }
            match self.breakers.iter().find(|b| b.id == f.breaker) {
                None => errs.push(E::FeedBreakerMissing {
                    feed: f.id.clone(),
                    breaker: f.breaker.clone(),
                }),
                Some(b) if b.feed != f.id => errs.push(E::BreakerFeedMismatch {
                    feed: f.id.clone(),
                    breaker: b.id.clone(),
                }),
                Some(_) => {}
            }
            if !self.breakers.iter().any(|b| b.feed == f.id) {
                errs.push(E::FeedUnprotected { feed: f.id.clone() });
            }
        }

        for b in &self.breakers {
            if self.feed(&b.feed).is_none() {
                errs.push(E::BreakerFeedMissing {
                    breaker: b.id.clone(),
                    feed: b.feed.clone(),
                });
            }
        }

        for l in &self.loads {
            if self.feed(&l.feed).is_none() {
                errs.push(E::LoadFeedMissing {
                    load: l.id.clone(),
                    feed: l.feed.clone(),
                });
            }
        }

        for h in &self.hazards {
            if self.feed(&h.feed).is_none() {
                errs.push(E::HazardFeedMissing {
                    hazard: h.id.clone(),
                    feed: h.feed.clone(),
                });
            }
            if h.x.1 <= h.x.0 {
                errs.push(E::HazardBoundsInverted {
                    hazard: h.id.clone(),
                });
            }
            if h.shock_ticks == 0 {
                errs.push(E::HazardShockTooShort {
                    hazard: h.id.clone(),
                });
            }
        }

        for i in &self.interlocks {
            if self.load(&i.load).is_none() {
                errs.push(E::InterlockLoadMissing {
                    interlock: i.id.clone(),
                    load: i.load.clone(),
                });
            }
            match &i.holds_when {
                InterlockWhen::LoadOperating(other) => {
                    if other == &i.load {
                        errs.push(E::InterlockSelfReference {
                            interlock: i.id.clone(),
                        });
                    }
                    if self.load(other).is_none() {
                        errs.push(E::InterlockUnknownLoad {
                            interlock: i.id.clone(),
                            load: other.clone(),
                        });
                    }
                }
                InterlockWhen::SignalActive(_) | InterlockWhen::SignalLocked(_) => {}
            }
        }

        errs
    }

    /// `Ok` iff [`Self::validate`] found nothing.
    pub fn ok(&self) -> Result<(), Vec<PowerValidationError>> {
        let errs = self.validate();
        if errs.is_empty() { Ok(()) } else { Err(errs) }
    }
}

/// Why a power payload could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PowerPackError {
    /// The JSON did not parse as a power network.
    Parse(String),
    /// It parsed but is not playable.
    Invalid(Vec<PowerValidationError>),
}

/// The UMS seam: parse a power-network payload, gate on its format tag and
/// validate it before the runtime trusts it — the same contract as
/// [`crate::scenario::signal::load_signal_network`].
pub fn load_power_network(json: &str) -> Result<PowerNetworkDef, PowerPackError> {
    let def: PowerNetworkDef =
        serde_json::from_str(json).map_err(|e| PowerPackError::Parse(e.to_string()))?;
    def.ok().map_err(PowerPackError::Invalid)?;
    Ok(def)
}

/// Serde: leave a false flag out of the wire form.
fn is_false(b: &bool) -> bool {
    !*b
}

/// Serde: an intact device needs no note in the wire form.
fn is_intact(c: &Condition) -> bool {
    *c == Condition::Intact
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(id: &str, kind: SourceKind, backup: Option<BackupSpec>) -> SourceDef {
        SourceDef {
            id: id.into(),
            label: id.into(),
            kind,
            start_on: true,
            startup_ticks: 0,
            backup,
            room: None,
            node: None,
            remote: false,
        }
    }

    fn feed(id: &str, source: &str, breaker: &str, capacity: u32) -> FeedDef {
        FeedDef {
            id: id.into(),
            label: id.into(),
            source: source.into(),
            capacity,
            breaker: breaker.into(),
            room: None,
            node: None,
        }
    }

    fn breaker(id: &str, feed: &str) -> BreakerDef {
        BreakerDef {
            id: id.into(),
            label: id.into(),
            feed: feed.into(),
            start_open: false,
            remote: false,
            room: None,
            node: None,
        }
    }

    fn load(id: &str, feed: &str, draw: u32) -> LoadDef {
        LoadDef {
            id: id.into(),
            label: id.into(),
            kind: LoadKind::Other,
            feed: feed.into(),
            draw,
            signal: None,
            room: None,
            restart: Restart::FailSafe,
            start_on: false,
            condition: Condition::Intact,
            restart_ticks: None,
            node: None,
        }
    }

    fn network() -> PowerNetworkDef {
        PowerNetworkDef {
            format: POWER_FORMAT.into(),
            network_id: "test".into(),
            name: "test".into(),
            sources: vec![source("mains", SourceKind::Mains, None)],
            feeds: vec![feed("feed-a", "mains", "brk-a", 10)],
            loads: vec![load("fan", "feed-a", 4)],
            breakers: vec![breaker("brk-a", "feed-a")],
            interlocks: vec![],
            hazards: vec![],
            tuning: PowerTuning::default(),
        }
    }

    #[test]
    fn a_small_network_validates() {
        assert_eq!(network().validate(), vec![]);
    }

    #[test]
    fn an_unprotected_feed_is_not_a_feed() {
        let mut net = network();
        net.breakers.clear();
        let errs = net.validate();
        assert!(errs.contains(&PowerValidationError::FeedBreakerMissing {
            feed: "feed-a".into(),
            breaker: "brk-a".into(),
        }));
        assert!(errs.contains(&PowerValidationError::FeedUnprotected {
            feed: "feed-a".into(),
        }));
    }

    #[test]
    fn a_backup_may_not_drive_a_feed() {
        let mut net = network();
        net.sources.push(source(
            "ups",
            SourceKind::Battery,
            Some(BackupSpec {
                runtime_ticks: 60,
                serves: vec!["fan".into()],
                mode: BackupMode::Automatic,
            }),
        ));
        net.feeds[0].source = "ups".into();
        assert!(net.validate().contains(&PowerValidationError::BackupDrivesFeed {
            source: "ups".into(),
            feed: "feed-a".into(),
        }));
    }

    #[test]
    fn an_id_may_not_mean_two_things() {
        let mut net = network();
        net.loads.push(load("feed-a", "feed-a", 1));
        assert_eq!(
            net.validate(),
            vec![PowerValidationError::DuplicateId {
                id: "feed-a".into()
            }]
        );
    }

    #[test]
    fn draw_is_a_connected_rating() {
        let mut net = network();
        net.loads.push(load("pump", "feed-a", 6));
        let feeds = vec![FeedId::from("feed-a"), FeedId::from("feed-a")];
        assert_eq!(net.connected_draw(&"feed-a".into(), &feeds), 10);
        let moved = vec![FeedId::from("feed-a"), FeedId::from("feed-b")];
        assert_eq!(net.connected_draw(&"feed-a".into(), &moved), 4);
    }

    #[test]
    fn the_wire_format_round_trips() {
        let json = serde_json::to_string(&network()).expect("serializes");
        let back: PowerNetworkDef = serde_json::from_str(&json).expect("parses");
        assert_eq!(back, network());
        assert!(load_power_network(&json).is_ok());
    }
}

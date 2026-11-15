//! Versioned guard profiles.
//!
//! A [`Profile`] is the smallest configuration surface that gives meaningfully
//! different guards **without inventing behaviour the kernel does not have**.
//!
//! The kernel's policy weights are deliberately not ABI (ADR-0010): a host that
//! hard-codes a weight would break the day the graph is tuned. So profiles do
//! not carry kernel weights. They vary only the seams the architecture actually
//! assigns:
//!
//! * `capabilities` — the mask the host declares. The kernel degrades along a
//!   documented ladder when a capability is absent (ADR-0004), so this is a real
//!   behavioural switch: a guard without `CAP_NAVMESH` gets `ADVANCE` downgraded
//!   to `HOLD`.
//! * `budget_units` — the per-tick work allowance, *declared* for forward
//!   compatibility. **The v0 kernel does not read it.** `ee_init_desc` has no
//!   budget field (adding one is a minor-version bump, ADR-0005), so
//!   `ee_init` overwrites the context's `budget_units` with a hard-coded 16 and
//!   the kernel's own `workUnits` is `5 + contact_count`, which reaches 13 at
//!   the ABI's maximum of eight contacts. Degradation therefore **cannot fire**
//!   in v0, and a profile's `budget_units` does not observably change anything
//!   today. It is carried so a profile written against the future field is
//!   already well-formed; read the effective value back with
//!   [`crate::WeevilHost::budget_units`] rather than assuming the declared one.
//! * `host` — how the *game* interprets and applies the proposal: the world
//!   scale, the movement limit, the engagement distances, the patrol route, and
//!   whether objects are reported as contacts. The game owns these; the profile
//!   makes them authorable.
//!
//! What a profile is **not**: a set of invented labels. `ProfileSet::validate`
//! refuses a profile whose declared capability bits the kernel does not define,
//! and every field here either drives a decision or is declared here *and
//! named as not yet consumed* — nothing is accepted that quietly does nothing.
//!
//! The live fields today are `capabilities` (the kernel's degradation ladder),
//! `max_agents`, `fx_per_world_unit`, `max_speed_world`, `engage_dist_world`,
//! `patrol_x` and `report_objects` (all read by the host's perception and
//! application), plus `decoy` (read by the headless corridor world).
//! `budget_units` and `investigate_dist_world` are declared and validated but
//! have no consumer yet; see their own docs.

use crate::abi;
use crate::fx::Fx2;
use serde::{Deserialize, Serialize};

/// The profile payload format this build reads.
pub const PROFILE_FORMAT: &str = "idaptik-weevil-profile/1";

/// How the host maps world distance to the ABI's fx units.
///
/// The kernel compares distance against fixed fx thresholds (attack at `1.0`,
/// flee at `2.0`), so the host picks the unit: this is the number of ABI fx
/// units in **one** canvas unit, and the conversion is
/// `world_to_fx(w) = to_fx(w * fx_per_world_unit)`.
///
/// `1.0 / 8.0` therefore makes one fx unit eight canvas units, i.e. the guard
/// engages at eight canvas units and flees at sixteen — a sensible corridor
/// scale for the Ghost Lobby.
///
/// The default is a *fraction* on purpose. Writing `8.0` here inverts the
/// meaning: the kernel's 1.0 attack range would then be one eighth of a canvas
/// unit, the guard would never close to engage, and a corridor patrol would
/// crawl at a sixteenth of its speed. The arithmetic is a multiply, so the
/// value must be `<= 1.0` for the kernel's ranges to be at least one canvas
/// unit. Measured, not assumed: with `8.0` the guard covers ~30 canvas units
/// in 240 ticks and never reaches the first closed door, so the door test in
/// `tests/replay_repro.rs` cannot pass.
pub const DEFAULT_FX_PER_WORLD_UNIT: f64 = 1.0 / 8.0;

/// The host-side application tuning for one guard.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostTuning {
    /// ABI fx units per world (canvas) unit. Must be strictly positive.
    pub fx_per_world_unit: f64,
    /// The game-defined movement limit, in world units per tick. The host
    /// clamps any proposed speed to this; the kernel's own speed is advisory.
    pub max_speed_world: f64,
    /// World distance at which an `ATTACK` intent becomes an engagement.
    pub engage_dist_world: f64,
    /// World distance within which a lost contact is worth investigating.
    ///
    /// **Declared and validated, not yet consumed.** The v0 host investigates
    /// its last known position unconditionally, and the game's own
    /// `billy_investigate` stops on a hard-coded reach. Wiring this field needs
    /// the game to read a profile it does not currently hold, so it is carried
    /// here rather than silently ignored: a profile that sets it changes
    /// nothing yet, and that is stated rather than implied.
    pub investigate_dist_world: f64,
    /// The guard's patrol route as world-x waypoints. Empty means "hold".
    pub patrol_x: Vec<f64>,
    /// Whether tracked objects (note, drive, …) are reported as contacts. This
    /// is a perception policy, which the game owns.
    pub report_objects: bool,
    /// One controlled in-world deception: a decoy/spoofed contact the guard will
    /// investigate. Read by the headless corridor world in
    /// [`crate::world::HostWorld::canonical`], where it *replaces* the scripted
    /// decoy the corridor ships with — `None` leaves the scripted one in place,
    /// because the deception is scenario content and a profile that does not
    /// mention it should not delete it.
    ///
    /// The running game's deception is not profile-driven: it is the robot
    /// vacuum, a fixed part of the Ghost Lobby scenario, reported to the guard
    /// as a `decoy` contact while the world's truth about it is a machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decoy: Option<DecoyTuning>,
    /// A scripted cast for the headless corridor, replacing its default player
    /// schedule and decoy. `None` keeps the default corridor. Read only by
    /// [`crate::world::HostWorld::canonical`]; the running game ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<CorridorScript>,
}

/// A scripted decoy: a contact that is real in the world (a sensor reading, a
/// speaker, a skittering object) but is not the infiltrator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecoyTuning {
    /// Contact id the decoy presents as. Must not collide with a live entity.
    pub id: u32,
    /// Where the decoy is perceived to be, world coordinates.
    pub origin_world: Fx2,
    /// How long the decoy is active, in ticks, from run start.
    pub active_ticks: u64,
    /// The perceived distance from the guard, in world units. The corridor
    /// derives the distance from the guard's own position instead, so this is
    /// an authoring hint it does not read.
    pub distance_world: f64,
    /// The perceived threat, in fx units. The corridor reports a fixed threat
    /// for its decoy, so this is an authoring hint it does not read.
    pub threat: i32,
}

/// A scripted cast: contacts that are really in the corridor for a window of
/// ticks and may hit the guard. They are people, not decoys, so they are in
/// the world's truth while present.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorridorScript {
    pub contacts: Vec<ScriptedContact>,
    /// ABI 1.2: a mind run of this script goes through the 40-byte model
    /// record ([`crate::WeevilHost::tick_model`]), and the host gives the
    /// observation cue on each contact's `observes` ticks. Whether the kernel
    /// reads the cue is the profile's `CAP_OBSERVATION_CUES`, so a control
    /// sets this too and withholds only the capability.
    #[serde(default, skip_serializing_if = "is_false")]
    pub cues: bool,
    /// The kernel fixtures' world rule for slice 2 (evil-weevil ADR-0014 §7):
    /// while the guard advances or stalks, it closes on the kernel's target by
    /// the proposed speed over [`crate::world::APPROACH_DIVISOR`], and it
    /// moves on no other action. Without it, a scripted contact keeps its
    /// distance, as in the corridor's 1.1 scenarios.
    #[serde(default, skip_serializing_if = "is_false")]
    pub approach: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// One scripted contact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScriptedContact {
    /// Contact id. Must not collide with the corridor's player (1), its object
    /// (100) or another scripted contact, and must fit the ABI's `u16`.
    pub id: u32,
    /// Where it stands, world x. It does not move.
    pub x: f64,
    /// Present on ticks `present_from .. present_until`.
    pub present_from: u64,
    pub present_until: u64,
    /// Perceived threat, `0.0..=1.0` of full.
    pub threat: f64,
    /// Ticks on which it hits the guard, and for how much health.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hits: Vec<ScriptedHit>,
    /// Ticks on which it observes the guard: the world's schedule, read by
    /// the world's rule of what a contact knows ([`crate::world::knows`]) and
    /// given to the kernel as the cue in a `cues` script.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub observes: Vec<u64>,
    /// How many ticks the contact keeps what it learned about the guard: its
    /// memory window in the world's rule ([`crate::world::knows_within`]).
    /// The designer sets it for each contact. 0, the default, means the
    /// contact knows only on the tick it observes the guard or is struck.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub memory: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// One hit on the guard.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScriptedHit {
    pub tick: u64,
    /// Health removed, `0.0 < damage <= 1.0`.
    pub damage: f64,
}

/// A versioned guard profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub format: String,
    pub id: String,
    pub version: u32,
    /// Capability mask the host declares for this guard.
    pub capabilities: u32,
    /// Per-tick work allowance, *declared* for forward compatibility. The v0
    /// kernel ignores it — see the module docs — so it does not affect the
    /// decision or the `DEGRADED` flag today.
    pub budget_units: u32,
    /// How many agents this profile may drive in one context.
    pub max_agents: u32,
    pub host: HostTuning,
}

impl Profile {
    /// The ABI fx distance for a world distance under this profile's scale.
    pub fn world_to_fx(&self, world: f64) -> i32 {
        crate::fx::to_fx(world * self.host.fx_per_world_unit)
    }

    /// The world distance for an ABI fx distance under this profile's scale.
    pub fn fx_to_world(&self, fx: i32) -> f64 {
        crate::fx::from_fx(fx) / self.host.fx_per_world_unit
    }

    /// The movement limit expressed in fx units.
    ///
    /// Informational. The host applies `max_speed_world` as a **world** limit
    /// after converting the kernel's proposed speed out of fx, so this is not
    /// the ceiling a proposal is clamped against — comparing the kernel's
    /// 1.0-fx advance to a 1.6-fx form of the limit would never bite, and the
    /// guard would run at 8 canvas units per tick whatever the profile said.
    pub fn max_speed_fx(&self) -> i32 {
        crate::fx::to_fx(self.host.max_speed_world)
    }

    /// Whether this profile declares a capability.
    pub fn has_capability(&self, mask: u32) -> bool {
        self.capabilities & mask != 0
    }

    /// Whether this profile declares the navmesh capability, which the kernel
    /// requires before it will ask an agent to leave its ground.
    pub fn has_navmesh(&self) -> bool {
        self.has_capability(abi::CAP_NAVMESH)
    }
}

/// A profile that failed validation.
#[derive(Debug, Clone, PartialEq)]
pub enum ProfileError {
    WrongFormat { found: String },
    FutureVersion { found: u32, max: u32 },
    UnknownCapability(u32),
    NonPositiveScale(f64),
    NonPositiveBudget(u32),
    ZeroAgents(u32),
    NonPositiveMovementLimit(f64),
    DecoyIdCollidesWith(u32),
    InvalidScript(String),
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongFormat { found } => write!(f, "profile format {found} is not supported"),
            Self::FutureVersion { found, max } => {
                write!(
                    f,
                    "profile version {found} is newer than this build ({max})"
                )
            }
            Self::UnknownCapability(mask) => {
                write!(f, "profile declares undefined capability bit 0x{mask:08x}")
            }
            Self::NonPositiveScale(s) => write!(f, "fx_per_world_unit must be > 0 (got {s})"),
            Self::NonPositiveBudget(b) => write!(f, "budget_units must be > 0 (got {b})"),
            Self::ZeroAgents(n) => write!(f, "max_agents must be > 0 (got {n})"),
            Self::NonPositiveMovementLimit(s) => {
                write!(f, "max_speed_world must be > 0 (got {s})")
            }
            Self::DecoyIdCollidesWith(id) => {
                write!(f, "decoy contact id {id} collides with a live entity id")
            }
            Self::InvalidScript(why) => write!(f, "corridor script: {why}"),
        }
    }
}

impl std::error::Error for ProfileError {}

/// The set of known, defined capabilities — the kernel's own ladder.
pub const KNOWN_CAPABILITIES: u32 = abi::CAP_NAVMESH
    | abi::CAP_LINE_OF_SIGHT
    | abi::CAP_RAYCAST
    | abi::CAP_WAYPOINT_GRAPH
    | abi::CAP_SPAWN_RIGHTS
    | abi::CAP_ANIMATION_HOOKS
    | abi::CAP_DAMAGE_EVENTS
    | abi::CAP_SOUND_EVENTS
    | abi::CAP_VEHICLE_BOARDING
    | abi::CAP_TERRAIN_HEIGHT
    | abi::CAP_OBSERVATION_CUES;

/// Strictly greater than zero. NaN is not, so a NaN scale or limit is refused.
fn is_positive(v: f64) -> bool {
    v.partial_cmp(&0.0) == Some(std::cmp::Ordering::Greater)
}

/// Ids the default corridor already uses: its player and its object.
const CORRIDOR_RESERVED_IDS: [u32; 2] = [1, 100];

fn validate_script(script: &CorridorScript) -> Result<(), ProfileError> {
    let bad = |why: String| Err(ProfileError::InvalidScript(why));
    let mut seen = std::collections::BTreeSet::new();
    for c in &script.contacts {
        if c.id == 0 || c.id > u32::from(u16::MAX) || CORRIDOR_RESERVED_IDS.contains(&c.id) {
            return bad(format!(
                "contact id {} is reserved or does not fit the ABI",
                c.id
            ));
        }
        if !seen.insert(c.id) {
            return bad(format!("contact id {} appears twice", c.id));
        }
        if !(0.0..1000.0).contains(&c.x) {
            return bad(format!(
                "contact {} stands outside the corridor at x={}",
                c.id, c.x
            ));
        }
        if c.present_from >= c.present_until {
            return bad(format!("contact {} is never present", c.id));
        }
        if !(0.0..=1.0).contains(&c.threat) {
            return bad(format!(
                "contact {} threat {} is outside 0..=1",
                c.id, c.threat
            ));
        }
        for t in &c.observes {
            if !(c.present_from..c.present_until).contains(t) {
                return bad(format!(
                    "contact {} observes the guard on tick {t} while absent",
                    c.id
                ));
            }
        }
        for h in &c.hits {
            if !(c.present_from..c.present_until).contains(&h.tick) {
                return bad(format!(
                    "contact {} hits on tick {} while absent",
                    c.id, h.tick
                ));
            }
            if !(h.damage > 0.0 && h.damage <= 1.0) {
                return bad(format!(
                    "contact {} hit damage {} is outside (0, 1]",
                    c.id, h.damage
                ));
            }
        }
    }
    Ok(())
}

/// The memory window of contact 41 in the six slice 2 model scenarios, the
/// three scenarios and their controls alike.
pub const MODEL_CONTACT_MEMORY: u64 = 0;

/// The largest profile version this build understands.
pub const PROFILE_VERSION_MAX: u32 = 1;

impl Profile {
    /// Validate a profile against this build's known capabilities and ranges.
    pub fn validate(&self) -> Result<(), ProfileError> {
        if self.format != PROFILE_FORMAT {
            return Err(ProfileError::WrongFormat {
                found: self.format.clone(),
            });
        }
        if self.version > PROFILE_VERSION_MAX {
            return Err(ProfileError::FutureVersion {
                found: self.version,
                max: PROFILE_VERSION_MAX,
            });
        }
        if self.capabilities & !KNOWN_CAPABILITIES != 0 {
            return Err(ProfileError::UnknownCapability(
                self.capabilities & !KNOWN_CAPABILITIES,
            ));
        }
        if !is_positive(self.host.fx_per_world_unit) {
            return Err(ProfileError::NonPositiveScale(self.host.fx_per_world_unit));
        }
        if self.budget_units == 0 {
            return Err(ProfileError::NonPositiveBudget(self.budget_units));
        }
        if self.max_agents == 0 {
            return Err(ProfileError::ZeroAgents(self.max_agents));
        }
        if !is_positive(self.host.max_speed_world) {
            return Err(ProfileError::NonPositiveMovementLimit(
                self.host.max_speed_world,
            ));
        }
        if let Some(d) = &self.host.decoy
            && d.id < 1
        {
            return Err(ProfileError::DecoyIdCollidesWith(d.id));
        }
        if let Some(script) = &self.host.script {
            validate_script(script)?;
        }
        Ok(())
    }

    /// Build a profile from a compact, validated constructor.
    pub fn build(
        id: &str,
        capabilities: u32,
        budget_units: u32,
        max_agents: u32,
        host: HostTuning,
    ) -> Result<Self, ProfileError> {
        let p = Self {
            format: PROFILE_FORMAT.to_owned(),
            id: id.to_owned(),
            version: PROFILE_VERSION_MAX,
            capabilities,
            budget_units,
            max_agents,
            host,
        };
        p.validate()?;
        Ok(p)
    }
}

/// The three named profiles the first milestone ships, plus `decoy-lure`, the
/// sentry with a decoy in its room for the epistemic document. The four
/// `the-*` profiles stage the kernel's mind fixtures in the corridor.
///
/// They are *not* behaviour trees. Each is the same kernel utility graph
/// fed a different perception policy and applied with different host limits; the
/// differences are the seams the architecture assigns, and each is observable in
/// the replay.
pub struct ProfileSet {
    pub cautious_sentry: Profile,
    pub aggressive_pursuer: Profile,
    pub equipment_specialist: Profile,
    pub decoy_lure: Profile,
    pub the_grudge: Profile,
    pub the_grudge_control: Profile,
    pub the_flinch: Profile,
    pub the_flinch_control: Profile,
    pub the_stalker: Profile,
    pub the_stalker_control: Profile,
    pub caught: Profile,
    pub caught_control: Profile,
    pub false_exposure: Profile,
    pub false_exposure_control: Profile,
}

impl ProfileSet {
    /// The canonical three profiles, all valid by construction.
    pub fn canonical() -> Self {
        let sentry = Profile::build(
            "cautious-sentry",
            abi::CAP_NAVMESH | abi::CAP_LINE_OF_SIGHT,
            4096,
            4,
            HostTuning {
                fx_per_world_unit: DEFAULT_FX_PER_WORLD_UNIT,
                // Holds ground; engages late and from further away.
                max_speed_world: 1.6,
                engage_dist_world: 9.0,
                investigate_dist_world: 40.0,
                patrol_x: vec![250.0, 520.0],
                report_objects: false,
                decoy: None,
                script: None,
            },
        )
        .expect("canonical sentry profile is valid");

        let pursuer = Profile::build(
            "aggressive-pursuer",
            abi::CAP_NAVMESH | abi::CAP_LINE_OF_SIGHT | abi::CAP_WAYPOINT_GRAPH,
            8192,
            4,
            HostTuning {
                fx_per_world_unit: DEFAULT_FX_PER_WORLD_UNIT,
                // Charges in; engages close and fast.
                max_speed_world: 3.2,
                engage_dist_world: 4.0,
                investigate_dist_world: 60.0,
                patrol_x: vec![220.0, 560.0, 880.0],
                report_objects: false,
                decoy: None,
                script: None,
            },
        )
        .expect("canonical pursuer profile is valid");

        let specialist = Profile::build(
            "equipment-specialist",
            abi::CAP_NAVMESH | abi::CAP_WAYPOINT_GRAPH | abi::CAP_ANIMATION_HOOKS,
            6144,
            4,
            HostTuning {
                fx_per_world_unit: DEFAULT_FX_PER_WORLD_UNIT,
                // Patrolling the equipment rooms; reacts to objects as contacts.
                max_speed_world: 2.4,
                engage_dist_world: 6.0,
                investigate_dist_world: 30.0,
                patrol_x: vec![330.0, 655.0],
                report_objects: true,
                decoy: None,
                script: None,
            },
        )
        .expect("canonical specialist profile is valid");

        // The sentry's seams, plus a decoy in its own room. The decoy is
        // perceived from tick 1 to tick 30 and stands for nothing in the
        // world's truth, so once it falls silent the kernel investigates a
        // contact that is not there. This is the run whose epistemic document
        // shows a belief the world does not hold.
        let lure = Profile::build(
            "decoy-lure",
            abi::CAP_NAVMESH | abi::CAP_LINE_OF_SIGHT,
            4096,
            4,
            HostTuning {
                decoy: Some(DecoyTuning {
                    id: 200,
                    // The corridor reads `origin_world.x` as a world x.
                    origin_world: Fx2 { x: 150, y: 0 },
                    active_ticks: 30,
                    distance_world: 100.0,
                    threat: crate::fx::to_fx(0.4),
                }),
                ..sentry.host.clone()
            },
        )
        .expect("canonical decoy-lure profile is valid");

        // The kernel's own mind fixtures (evil-weevil ADR-0013 §10), staged in the
        // corridor. The guard holds at x = 50 (no patrol); each contact stands
        // where the fixture puts it, as a world distance at the corridor's scale
        // of 8 world units to 1.0 fx: 100000 fx is 12.20703125, 40000 fx is
        // 4.8828125, 98304 fx is 12. The guard is not pinned in place, so once
        // the host applies a move the distances are the corridor's, not the
        // fixture's.
        let damage = abi::CAP_NAVMESH | abi::CAP_LINE_OF_SIGHT | abi::CAP_DAMAGE_EVENTS;
        let no_damage = abi::CAP_NAVMESH | abi::CAP_LINE_OF_SIGHT;
        let stage = |id: &str, caps: u32, contacts: Vec<ScriptedContact>| {
            Profile::build(
                id,
                caps,
                4096,
                4,
                HostTuning {
                    patrol_x: Vec::new(),
                    decoy: None,
                    script: Some(CorridorScript {
                        contacts,
                        cues: false,
                        approach: false,
                    }),
                    ..sentry.host.clone()
                },
            )
            .expect("canonical mind scenario profile is valid")
        };
        // "The grudge": A (21) hits the guard on ticks 10, 11 and 12 and is gone
        // from tick 13; B (22), nearer and less threatening, arrives at tick 20.
        let grudge_cast = || {
            vec![
                ScriptedContact {
                    id: 21,
                    x: 62.207_031_25,
                    present_from: 0,
                    present_until: 13,
                    threat: 0.5,
                    hits: [10, 11, 12]
                        .into_iter()
                        .map(|tick| ScriptedHit {
                            tick,
                            damage: 0.125,
                        })
                        .collect(),
                    observes: Vec::new(),
                    memory: 0,
                },
                ScriptedContact {
                    id: 22,
                    x: 54.882_812_5,
                    present_from: 20,
                    present_until: u64::MAX,
                    threat: 0.125,
                    hits: Vec::new(),
                    observes: Vec::new(),
                    memory: 0,
                },
            ]
        };
        // "The flinch": C (31) is there on ticks 10 to 19, close (a surprise) or,
        // for the control, at 1.5 fx (not one).
        let flinch_cast = |x: f64| {
            vec![ScriptedContact {
                id: 31,
                x,
                present_from: 10,
                present_until: 20,
                threat: 0.5,
                hits: Vec::new(),
                observes: Vec::new(),
                memory: 0,
            }]
        };
        let the_grudge = stage("the-grudge", damage, grudge_cast());
        // The same cast without the damage capability: grievance cannot form.
        let the_grudge_control = stage("the-grudge-control", no_damage, grudge_cast());
        let the_flinch = stage("the-flinch", damage, flinch_cast(54.882_812_5));
        let the_flinch_control = stage("the-flinch-control", damage, flinch_cast(62.0));

        // Slice 2, "it models you" (evil-weevil ADR-0014 §7), staged in the
        // corridor. One contact, 41, stands 3.0 fx from the guard (24 world
        // units at the corridor's scale: x = 74 against the guard's 50), as in
        // the kernel's fixtures, and is there on every tick. The scenario
        // declares CAP_OBSERVATION_CUES; its control withholds that one bit
        // and changes nothing else, cue schedule included. Corridor ticks
        // count from 1, the fixtures' from 0, so a glance on the fixture's
        // tick 100 is on the corridor's tick 101.
        let cues = abi::CAP_NAVMESH | abi::CAP_LINE_OF_SIGHT | abi::CAP_OBSERVATION_CUES;
        let model = |id: &str, caps: u32, observes: Vec<u64>, approach: bool| {
            Profile::build(
                id,
                caps,
                4096,
                4,
                HostTuning {
                    patrol_x: Vec::new(),
                    decoy: None,
                    script: Some(CorridorScript {
                        contacts: vec![ScriptedContact {
                            id: 41,
                            x: 74.0,
                            present_from: 0,
                            present_until: u64::MAX,
                            threat: 0.5,
                            hits: Vec::new(),
                            observes,
                            memory: MODEL_CONTACT_MEMORY,
                        }],
                        cues: true,
                        approach,
                    }),
                    ..sentry.host.clone()
                },
            )
            .expect("canonical model scenario profile is valid")
        };
        // "The stalker": the contact never observes the guard, and the guard
        // closes on it.
        let the_stalker = model("the-stalker", cues, Vec::new(), true);
        let the_stalker_control = model("the-stalker-control", no_damage, Vec::new(), true);
        // "Caught": one glance while the guard stalks.
        let caught = model("caught", cues, vec![101], true);
        let caught_control = model("caught-control", no_damage, vec![101], true);
        // "False exposure": one glance, then the contact looks away and keeps
        // its distance (no approach, as in the fixture, where the range does
        // not close).
        let false_exposure = model("false-exposure", cues, vec![21], false);
        let false_exposure_control = model("false-exposure-control", no_damage, vec![21], false);

        Self {
            cautious_sentry: sentry,
            aggressive_pursuer: pursuer,
            equipment_specialist: specialist,
            decoy_lure: lure,
            the_grudge,
            the_grudge_control,
            the_flinch,
            the_flinch_control,
            the_stalker,
            the_stalker_control,
            caught,
            caught_control,
            false_exposure,
            false_exposure_control,
        }
    }

    /// Look up a profile by id.
    pub fn get(&self, id: &str) -> Option<&Profile> {
        match id {
            "cautious-sentry" => Some(&self.cautious_sentry),
            "aggressive-pursuer" => Some(&self.aggressive_pursuer),
            "equipment-specialist" => Some(&self.equipment_specialist),
            "decoy-lure" => Some(&self.decoy_lure),
            "the-grudge" => Some(&self.the_grudge),
            "the-grudge-control" => Some(&self.the_grudge_control),
            "the-flinch" => Some(&self.the_flinch),
            "the-flinch-control" => Some(&self.the_flinch_control),
            "the-stalker" => Some(&self.the_stalker),
            "the-stalker-control" => Some(&self.the_stalker_control),
            "caught" => Some(&self.caught),
            "caught-control" => Some(&self.caught_control),
            "false-exposure" => Some(&self.false_exposure),
            "false-exposure-control" => Some(&self.false_exposure_control),
            _ => None,
        }
    }

    /// All of them, in a stable order.
    pub fn all(&self) -> [&Profile; 14] {
        [
            &self.cautious_sentry,
            &self.aggressive_pursuer,
            &self.equipment_specialist,
            &self.decoy_lure,
            &self.the_grudge,
            &self.the_grudge_control,
            &self.the_flinch,
            &self.the_flinch_control,
            &self.the_stalker,
            &self.the_stalker_control,
            &self.caught,
            &self.caught_control,
            &self.false_exposure,
            &self.false_exposure_control,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_profiles_validate() {
        for p in ProfileSet::canonical().all() {
            p.validate().expect("canonical profile valid");
        }
    }

    #[test]
    fn an_undefined_capability_is_refused() {
        let mut p = ProfileSet::canonical().cautious_sentry.clone();
        p.capabilities |= 1u32 << 31;
        assert_eq!(
            p.validate().unwrap_err(),
            ProfileError::UnknownCapability(1u32 << 31)
        );
    }

    #[test]
    fn profiles_differ_observably() {
        let set = ProfileSet::canonical();
        assert_ne!(
            set.cautious_sentry.host.max_speed_world,
            set.aggressive_pursuer.host.max_speed_world
        );
        assert_ne!(
            set.cautious_sentry.host.engage_dist_world,
            set.aggressive_pursuer.host.engage_dist_world
        );
        assert!(set.equipment_specialist.host.report_objects);
        assert!(!set.cautious_sentry.host.report_objects);
    }
}

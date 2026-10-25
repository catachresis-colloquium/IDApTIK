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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileError {
    WrongFormat { found: String },
    FutureVersion { found: u32, max: u32 },
    UnknownCapability(u32),
    NonPositiveScale(f64),
    NonPositiveBudget(u32),
    ZeroAgents(u32),
    NonPositiveMovementLimit(f64),
    DecoyIdCollidesWith(u32),
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongFormat { found } => write!(f, "profile format {found} is not supported"),
            Self::FutureVersion { found, max } => {
                write!(f, "profile version {found} is newer than this build ({max})")
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
    | abi::CAP_TERRAIN_HEIGHT;

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
        if !(self.host.fx_per_world_unit > 0.0) {
            return Err(ProfileError::NonPositiveScale(self.host.fx_per_world_unit));
        }
        if self.budget_units == 0 {
            return Err(ProfileError::NonPositiveBudget(self.budget_units));
        }
        if self.max_agents == 0 {
            return Err(ProfileError::ZeroAgents(self.max_agents));
        }
        if !(self.host.max_speed_world > 0.0) {
            return Err(ProfileError::NonPositiveMovementLimit(self.host.max_speed_world));
        }
        if let Some(d) = &self.host.decoy
            && d.id < 1
        {
            return Err(ProfileError::DecoyIdCollidesWith(d.id));
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

/// The three named profiles the first milestone ships.
///
/// They are *not* three behaviour trees. Each is the same kernel utility graph
/// fed a different perception policy and applied with different host limits; the
/// differences are the seams the architecture assigns, and each is observable in
/// the replay.
pub struct ProfileSet {
    pub cautious_sentry: Profile,
    pub aggressive_pursuer: Profile,
    pub equipment_specialist: Profile,
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
            },
        )
        .expect("canonical specialist profile is valid");

        Self {
            cautious_sentry: sentry,
            aggressive_pursuer: pursuer,
            equipment_specialist: specialist,
        }
    }

    /// Look up a profile by id.
    pub fn get(&self, id: &str) -> Option<&Profile> {
        match id {
            "cautious-sentry" => Some(&self.cautious_sentry),
            "aggressive-pursuer" => Some(&self.aggressive_pursuer),
            "equipment-specialist" => Some(&self.equipment_specialist),
            _ => None,
        }
    }

    /// All three, in a stable order.
    pub fn all(&self) -> [&Profile; 3] {
        [
            &self.cautious_sentry,
            &self.aggressive_pursuer,
            &self.equipment_specialist,
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
        assert_eq!(p.validate().unwrap_err(), ProfileError::UnknownCapability(1u32 << 31));
    }

    #[test]
    fn profiles_differ_observably() {
        let set = ProfileSet::canonical();
        assert_ne!(set.cautious_sentry.host.max_speed_world, set.aggressive_pursuer.host.max_speed_world);
        assert_ne!(set.cautious_sentry.host.engage_dist_world, set.aggressive_pursuer.host.engage_dist_world);
        assert!(set.equipment_specialist.host.report_objects);
        assert!(!set.cautious_sentry.host.report_objects);
    }
}

//! Infrastructure and its consequences: water, equipment failure, alarms — and
//! the *observations* through which actors ever learn about them.
//!
//! The rule this module exists to enforce: **guards react to observations, not
//! to hidden player actions.** A hacker opening a valve is a command; the
//! water it releases is canonical world state; the splash that water makes is
//! an [`Observation`]; and only the observation — gated by the actor's own
//! hearing and by which room the actor stands in — may move the actor. Nothing
//! here ever hands an actor the player's intent, the command stream, or any
//! fact the actor's senses could not carry.
//!
//! # Ownership
//!
//! All of this state is canonical simulation state, owned by
//! [`crate::scenario::state::RuntimeState`] and stepped by
//! [`crate::scenario::sim`] in the fixed pipeline. It is pure serde data,
//! snapshot-restorable, and a pure function of `(definition, config, seed,
//! command stream)` like everything else. Rendering and acceleration backends
//! draw water and hazards *from* this state and its events; they must never
//! carry an independent authoritative fluid or hazard state of their own, and
//! nothing render-derived feeds back in (the repository invariant, restated
//! where the fluid state lives).
//!
//! # Absent by default
//!
//! A floor without an [`InfraDef`] takes none of these paths: the definition
//! and state fields are `Option`, skipped in serde when absent, so the
//! canonical Ghost Lobby floor, its goldens and every recorded replay are
//! byte-identical — the same posture as the signal network (ADR-0012).
//!
//! # Shape
//!
//! * [`InfraDef`] — declarative content: [`WaterZoneDef`]s (each with a
//!   derived valve node a hacker can open and a derived pump node whose power
//!   loss fails the valve *open*), and [`AlarmDef`]s (tripped by water or by
//!   the loss of a watched node — a normally-energised tamper circuit).
//! * [`InfraState`] — the runtime: per-zone release/level/short state, alarm
//!   latches, and the guard's announce-once memory of what it has noticed.
//! * [`Observation`] / [`ObservationKind`] — the perception surface. The sim
//!   computes these per actor per tick from canonical state alone
//!   ([`observations`]), gated by room and by the actor's own hearing stats.
//! * [`NoiseSource`] / [`noise_sources`] — where the floor is making noise
//!   right now, independent of who can hear it; the observation gate decides
//!   who hears.
//!
//! # Evil Weevil adapter
//!
//! No Evil Weevil adapter is present in this tree (checked at integration
//! time, 2026-10-24). Should one land, the seam it plugs into is this
//! module's observation surface: an adapter actor consumes [`observations`]
//! for its own position and senses exactly as Billy's FSM does, and never the
//! world state behind them. See ADR-0013.

use crate::scenario::constants as c;
use crate::scenario::definition::ScenarioDefinition;
use crate::scenario::ids::RoomId;
use crate::scenario::mathf::clamp;
use crate::scenario::state::RuntimeState;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What released a zone's water — the provenance the event log carries, so a
/// player can read *why* the floor is flooding (a valve they opened, or a
/// pump they killed without knowing what it held up).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReleaseCause {
    /// The zone's valve was actuated (a deliberate hack).
    Valve,
    /// The zone's pump lost power and the valve failed open (a consequence
    /// nobody ordered).
    PumpFailure,
}

/// One floodable zone: a room's plumbing or sprinkler run, behind a valve a
/// hacker can open and a pump that keeps it dry while it has power.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaterZoneDef {
    /// Stable zone id; also the suffix of its derived valve/pump node ids
    /// (`valve-<id>`, `pump-<id>` — see [`crate::scenario::floor_graph`]).
    pub id: String,
    /// The room the water lands in.
    pub room: RoomId,
    /// World-x of the manual handle, standing in the zone's room: the
    /// physical release. Holding interact beside it turns it, and the zone
    /// releases through the same canonical path as hacking the valve node —
    /// one consequence model, two ways in.
    pub valve_x: f64,
    /// Seconds from release to full depth. Must be positive and finite.
    pub fill_time: f64,
    /// Whether the zone's pump valve fails open: losing the pump's power
    /// releases the zone. Defaults true — the interesting case, and the one
    /// the authoring audit reasons about.
    #[serde(default = "default_fail_open")]
    pub fail_open: bool,
}

fn default_fail_open() -> bool {
    true
}

/// What trips an alarm.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AlarmTrip {
    /// Water reaching [`c::ALARM_WATER_TRIP_LEVEL`] in `zone`.
    Water { zone: String },
    /// The watched grounded-network node losing power — a normally-energised
    /// tamper circuit: cutting the power *is* the trip.
    PowerLost { watch: String },
}

/// One alarm: a sounder in a room, tripped by a declared condition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlarmDef {
    pub id: String,
    /// The room the sounder stands in (where a guard sent to look would go).
    pub room: RoomId,
    pub trip: AlarmTrip,
}

/// A floor's declared infrastructure. Absent (`None`) on every existing floor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InfraDef {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub water_zones: Vec<WaterZoneDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alarms: Vec<AlarmDef>,
}

impl InfraDef {
    /// Whether this declaration carries any content at all.
    pub fn is_empty(&self) -> bool {
        self.water_zones.is_empty() && self.alarms.is_empty()
    }
}

/// One zone's runtime state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaterZoneState {
    /// The valve has released (by hack or by pump failure); the level climbs.
    pub released: bool,
    /// Why it released — `None` while dry. The event log's provenance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<ReleaseCause>,
    /// Depth, `0..=1`; climbs at `1/fill_time` a second once released. Water
    /// is never pumped back out inside a run — a flood is a commitment.
    pub level: f64,
    /// The room's electrics have shorted (one-shot, latched).
    pub shorted: bool,
    /// Seconds of sparking left after the short (a seen/heard failure).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub short_flash: f64,
}

impl Default for WaterZoneState {
    fn default() -> Self {
        Self {
            released: false,
            cause: None,
            level: 0.0,
            shorted: false,
            short_flash: 0.0,
        }
    }
}

/// The floor's infrastructure runtime state — pure serde, so a snapshot
/// restores it exactly. The guard's announce-once memory of what it has
/// noticed lives on [`RuntimeState::guard_noticed`], not here: a floor can
/// host mole-made noise with no infrastructure at all, and the perception
/// latch belongs to the run, not to the plumbing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InfraState {
    /// Per-zone state, keyed by zone id (a `BTreeMap`: iteration order is
    /// part of determinism).
    pub zones: BTreeMap<String, WaterZoneState>,
    /// Per-alarm sounding latch, keyed by alarm id.
    pub alarms: BTreeMap<String, bool>,
}

impl InfraState {
    /// Build the dry, silent start for a floor's declaration.
    pub fn initial(def: &InfraDef) -> Self {
        Self {
            zones: def
                .water_zones
                .iter()
                .map(|z| (z.id.clone(), WaterZoneState::default()))
                .collect(),
            alarms: def.alarms.iter().map(|a| (a.id.clone(), false)).collect(),
        }
    }

    /// Release a zone (idempotent; the first cause is the recorded one).
    /// Returns `true` when this call is what released it.
    pub fn release(&mut self, zone: &str, cause: ReleaseCause) -> bool {
        let Some(z) = self.zones.get_mut(zone) else {
            return false;
        };
        if z.released {
            return false;
        }
        z.released = true;
        z.cause = Some(cause);
        true
    }
}

/// What kind of fact an actor perceived. The vocabulary is deliberately about
/// the *building*, never about the players: an actor learns "the hall is
/// flooding", not "the hacker opened valve hall-main".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ObservationKind {
    /// A room that is normally lit has gone dark.
    Darkness,
    /// An unusual noise at a position (splash, sparking, digging, a synth
    /// diversion). The player's own movement noise is *not* this: it travels
    /// the archetype's existing `invest_noise` hearing gate unchanged.
    UnusualNoise,
    /// Standing water in the actor's room.
    Water,
    /// Equipment failing right now (sparking short) in the actor's room.
    EquipmentFailure,
    /// An alarm is sounding (heard floor-wide).
    Alarm,
}

/// One observation available to an actor: a kind, where it is, and how strong
/// it is on the same scale as the player's noise meter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    pub kind: ObservationKind,
    /// The room the observation is about.
    pub room: RoomId,
    /// World-x the actor would investigate (source of the noise, centre of
    /// the dark or flooded room, the alarm's sounder).
    pub x: f64,
    /// Strength on the noise scale (`0..=1`); `1.0` for things that cannot be
    /// missed (a sounding alarm, water at the actor's feet).
    pub intensity: f64,
}

impl Observation {
    /// The announce-once key for this observation (episode identity: the same
    /// kind about the same room is one episode).
    pub fn notice_key(&self) -> String {
        format!("{:?}:{}", self.kind, self.room.as_str())
    }
}

/// A noise the floor is making right now, before anyone's hearing gates it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoiseSource {
    /// World-x the noise comes from.
    pub x: f64,
    /// The room it is in.
    pub room: RoomId,
    /// Loudness on the noise scale.
    pub intensity: f64,
    /// The observation kind it becomes for anyone who hears it (always
    /// [`ObservationKind::UnusualNoise`] for sound; the field keeps the
    /// source's provenance for logs and tests).
    pub kind: ObservationKind,
}

/// The centre-x of a room, or the world's midpoint for a room the definition
/// does not carry (total: a dangling room id degrades, never panics).
pub fn room_centre(def: &ScenarioDefinition, room: &str) -> f64 {
    def.rooms
        .iter()
        .find(|r| r.id.as_str() == room)
        .map(|r| r.x + r.w / 2.0)
        .unwrap_or(def.world.width / 2.0)
}

/// Whether a room's lights are off right now, from canonical state alone:
/// the global lights-out flicker, the room's light node dead (power cascade
/// or a short), or the room's light device switched off over the signal
/// network. This is the same trio the sim's own sight checks read; it is a
/// free function so the observation gate and the authoring audit share one
/// definition of "dark".
pub fn room_unlit(def: &ScenarioDefinition, state: &RuntimeState, room: &str) -> bool {
    if state.lights_flicker > 0.0 || state.camera_lockout > 0.0 {
        return true;
    }
    let light = crate::scenario::floor_graph::light_node_id(room);
    if state.dead_nodes.contains(light.as_str()) {
        return true;
    }
    // A wire has switched the room's lights off (ADR-0012).
    if let (Some(net), Some(sig)) = (&def.signal, &state.signal) {
        for (d, s) in net.devices.iter().zip(&sig.devices) {
            if d.node.as_deref() == Some(light.as_str()) && !s.active {
                return true;
            }
        }
    }
    false
}

/// The water, if any, at world-x: the deepest released zone in the room the
/// body stands in. `None` on a floor with no infrastructure, in a room with
/// no zone, or while every zone is dry.
pub fn water_at(def: &ScenarioDefinition, state: &RuntimeState, x: f64) -> Option<f64> {
    let infra_def = def.infra.as_ref()?;
    let infra = state.infra.as_ref()?;
    let room = room_id_at(def, x)?;
    let mut deepest = None;
    for zone in &infra_def.water_zones {
        if zone.room.as_str() != room {
            continue;
        }
        if let Some(z) = infra.zones.get(zone.id.as_str()) {
            if z.released && z.level > 0.0 {
                deepest = Some(deepest.map_or(z.level, |d: f64| d.max(z.level)));
            }
        }
    }
    deepest
}

/// The speed multiplier wading imposes on a body at world-x: `1.0` unless the
/// room's water is at or over the wade band.
pub fn wade_factor(def: &ScenarioDefinition, state: &RuntimeState, x: f64) -> f64 {
    match water_at(def, state, x) {
        Some(level) if level >= c::WATER_WADE_AT => c::WATER_WADE_SPEED,
        _ => 1.0,
    }
}

/// The noise floor wading sets on a moving body (splashing betrays a wader
/// through the actor's *existing* hearing gate). `0.0` when dry.
pub fn wade_noise(def: &ScenarioDefinition, state: &RuntimeState, x: f64) -> f64 {
    match water_at(def, state, x) {
        Some(level) if level >= c::WATER_WADE_AT => c::WATER_WADE_NOISE,
        _ => 0.0,
    }
}

/// Every noise the infrastructure is making this tick, in definition order:
/// zones still filling splash, shorted rooms spark for their flash window,
/// sounding alarms ring floor-wide. Mole-made noise is added by the hosting
/// seam in [`crate::scenario::sim`], not here — this function reads canonical
/// infrastructure state only.
pub fn noise_sources(def: &ScenarioDefinition, state: &RuntimeState) -> Vec<NoiseSource> {
    let mut out = Vec::new();
    let (Some(infra_def), Some(infra)) = (def.infra.as_ref(), state.infra.as_ref()) else {
        return out;
    };
    for zone in &infra_def.water_zones {
        let Some(z) = infra.zones.get(zone.id.as_str()) else {
            continue;
        };
        if !z.released {
            continue;
        }
        let x = room_centre(def, zone.room.as_str());
        if z.level < 1.0 {
            out.push(NoiseSource {
                x,
                room: zone.room.clone(),
                intensity: c::WATER_SPLASH_NOISE,
                kind: ObservationKind::UnusualNoise,
            });
        }
        if z.short_flash > 0.0 {
            out.push(NoiseSource {
                x,
                room: zone.room.clone(),
                intensity: c::SHORT_FLASH_NOISE,
                kind: ObservationKind::EquipmentFailure,
            });
        }
    }
    for alarm in &infra_def.alarms {
        if infra.alarms.get(alarm.id.as_str()).copied() == Some(true) {
            out.push(NoiseSource {
                x: room_centre(def, alarm.room.as_str()),
                room: alarm.room.clone(),
                intensity: c::ALARM_NOISE,
                kind: ObservationKind::Alarm,
            });
        }
    }
    out
}

/// One actor's hearing: how far it hears, and how loud a noise must be to
/// carry that far. Billy's come from the archetype's existing `invest_dist`
/// and `invest_noise` stats — the infrastructure layer invents no new senses
/// and gets no better ears than the FSM already has.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hearing {
    pub dist: f64,
    pub threshold: f64,
}

/// The observations available to an actor standing at world-x, computed from
/// canonical state alone. This is the *only* door infrastructure facts walk
/// through to reach an actor:
///
/// * darkness — the actor's own room, and only when that room is normally
///   lit (a room authored dark is not an observation);
/// * water and equipment failure — the actor's own room (seen);
/// * unusual noise — any source inside the actor's hearing distance whose
///   intensity clears the actor's threshold (heard);
/// * alarms — floor-wide, ungated (an alarm is meant to be heard).
///
/// Nothing here reads the player's hidden state, the command stream, or any
/// fact outside the actor's room and earshot; an actor fed only this list can
/// never be omniscient.
pub fn observations(
    def: &ScenarioDefinition,
    state: &RuntimeState,
    at_x: f64,
    hearing: Hearing,
    extra_noise: &[NoiseSource],
) -> Vec<Observation> {
    let mut out = Vec::new();
    let Some(room) = room_id_at(def, at_x) else {
        return out;
    };
    let room = room.to_owned();

    // Darkness: the actor's own room, normally lit, now unlit.
    if def.rooms.iter().any(|r| r.id == room && r.lit) && room_unlit(def, state, &room) {
        out.push(Observation {
            kind: ObservationKind::Darkness,
            x: room_centre(def, &room),
            intensity: 1.0,
            room: room.clone(),
        });
    }

    // Water and sparking electrics: seen, in the actor's own room.
    if let Some(level) = water_at(def, state, at_x) {
        out.push(Observation {
            kind: ObservationKind::Water,
            x: room_centre(def, &room),
            intensity: clamp(level, 0.0, 1.0),
            room: room.clone(),
        });
    }
    if let (Some(infra_def), Some(infra)) = (def.infra.as_ref(), state.infra.as_ref()) {
        let sparking = infra_def.water_zones.iter().any(|zone| {
            zone.room == room
                && infra
                    .zones
                    .get(zone.id.as_str())
                    .is_some_and(|z| z.short_flash > 0.0)
        });
        if sparking {
            out.push(Observation {
                kind: ObservationKind::EquipmentFailure,
                x: room_centre(def, &room),
                intensity: c::SHORT_FLASH_NOISE,
                room: room.clone(),
            });
        }
    }

    // Heard: every noise source inside this actor's hearing, gated by its
    // own threshold. Alarms skip the distance gate — that is what makes them
    // alarms — but still arrive as observations, not as world truth.
    let mut sources = noise_sources(def, state);
    sources.extend(extra_noise.iter().cloned());
    for src in sources {
        let heard = if src.kind == ObservationKind::Alarm {
            src.intensity > hearing.threshold
        } else {
            src.intensity > hearing.threshold && (src.x - at_x).abs() < hearing.dist
        };
        if heard {
            out.push(Observation {
                kind: src.kind,
                x: src.x,
                intensity: src.intensity,
                room: src.room,
            });
        }
    }
    out
}

/// The room id at world-x, judged exactly as the simulation judges it. A
/// local copy rather than a call into `sim` so this module stays a pure
/// function of definition and state (and `sim` keeps its crate-private
/// helper).
pub fn room_id_at(def: &ScenarioDefinition, x: f64) -> Option<&str> {
    let cx = x + def.world.room_offset;
    def.rooms
        .iter()
        .find(|r| r.contains(cx))
        .or_else(|| def.rooms.last())
        .map(|r| r.id.as_str())
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-room definition with one lit room, one dark-by-authoring room,
    /// and one floodable zone in the lit room.
    fn fixture() -> (ScenarioDefinition, InfraDef) {
        let mut def = crate::scenario::ghost_lobby::ghost_lobby();
        def.rooms.iter_mut().next().unwrap().lit = true;
        let infra = InfraDef {
            water_zones: vec![WaterZoneDef {
                id: "z1".into(),
                room: def.rooms.first().unwrap().id.clone(),
                valve_x: 150.0,
                fill_time: 4.0,
                fail_open: true,
            }],
            alarms: vec![AlarmDef {
                id: "a1".into(),
                room: def.rooms.first().unwrap().id.clone(),
                trip: AlarmTrip::Water { zone: "z1".into() },
            }],
        };
        (def, infra)
    }

    #[test]
    fn a_dry_floor_observes_nothing() {
        let (def, infra) = fixture();
        let mut def = def;
        def.infra = Some(infra.clone());
        let state = RuntimeState {
            infra: Some(InfraState::initial(&infra)),
            ..RuntimeState::initial(
                &def,
                &crate::scenario::rng::roll_init(
                    &def,
                    &mut crate::scenario::rng::Mulberry32::new(1),
                    crate::scenario::tuning::DifficultyId::Standard,
                ),
                crate::scenario::command::RunConfig::standard(),
                &def.difficulty
                    .get(&crate::scenario::tuning::DifficultyId::Standard)
                    .cloned()
                    .unwrap(),
            )
        };
        let at = def.rooms.first().unwrap().x + 10.0;
        let obs = observations(
            &def,
            &state,
            at,
            Hearing {
                dist: 260.0,
                threshold: 0.45,
            },
            &[],
        );
        assert!(obs.is_empty(), "a dry, silent, lit floor is no news: {obs:?}");
    }

    #[test]
    fn release_is_idempotent_and_keeps_the_first_cause() {
        let mut st = InfraState {
            zones: BTreeMap::from([("z".to_owned(), WaterZoneState::default())]),
            ..Default::default()
        };
        assert!(st.release("z", ReleaseCause::PumpFailure));
        assert!(!st.release("z", ReleaseCause::Valve));
        assert_eq!(st.zones["z"].cause, Some(ReleaseCause::PumpFailure));
        assert!(!st.release("missing", ReleaseCause::Valve));
    }
}

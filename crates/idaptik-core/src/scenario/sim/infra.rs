//! Hosting the floor's infrastructure (ADR-0013) inside the Ghost Lobby run.
//!
//! The infrastructure model itself lives in [`crate::scenario::infra`]; this
//! is the seam that steps it and applies what it decides:
//!
//! * **Water** — a released zone climbs toward full depth on its own clock.
//!   At [`c::WATER_SHORT_AT`] the room's electrics short out through the
//!   ordinary [`crate::netsim::Effect::DevicePowerLost`] path, so a flood
//!   darkens lights, kills cameras and jams the network exactly the way a
//!   power cascade does — one consequence model, two ways in.
//! * **Alarms** — a water trip or a tamper (normally-energised) trip starts a
//!   sounder; while any sounder runs, the floor's existing `alert` meter
//!   climbs, which sharpens the guard through the archetype's *existing*
//!   alert stats rather than any new channel.
//! * **Perception** — [`GhostLobbySim::actor_observations`] is the only door
//!   any of this walks through to reach an actor: observations computed from
//!   canonical state at the actor's own position, gated by its own hearing.
//!   [`GhostLobbySim::system_infra_watch`] keeps the announce-once memory and
//!   emits the player-visible [`Event::GuardObserved`] record; Billy's FSM
//!   consults [`GhostLobbySim::infra_investigation_target`] inside its
//!   existing Assess-mode noise gate.
//!
//! A floor with no [`crate::scenario::infra::InfraDef`] and no hosted
//! companion takes none of these paths, so every existing replay, golden and
//! snapshot is unchanged.
//!
//! **Ownership.** Everything here mutates canonical [`RuntimeState`] inside
//! the deterministic tick. No rendering or acceleration backend contributes:
//! frontends draw water, sparks and alarms from these events and state, and
//! nothing render-derived feeds back in.

use super::GhostLobbySim;
use crate::netsim::Effect;
use crate::scenario::agents::apply_effects;
use crate::scenario::common::Phase;
use crate::scenario::constants as c;
use crate::scenario::event::Event;
use crate::scenario::floor_graph::{
    camera_node_id, door_node_id, light_node_id, pump_node_id, valve_node_id,
};
use crate::scenario::ids::RoomId;
use crate::scenario::infra::{
    self, AlarmTrip, NoiseSource, Observation, ObservationKind, water_at,
};
use crate::scenario::mathf::{TICK_DT, clamp};
use std::collections::BTreeSet;

impl GhostLobbySim {
    /// One tick of infrastructure: diversion windows shrink, water climbs,
    /// rooms short out, alarms trip and clear, and a sounding alarm climbs
    /// the floor's alert. Runs right after the signal network and before
    /// anything moves, so a body reads this tick's water.
    pub(super) fn system_infra(&mut self) {
        let dt = TICK_DT;
        if self.def.infra.is_none() && self.state.diversion.is_none() {
            return;
        }

        // The synth diversion's window.
        if let Some(d) = self.state.diversion.as_mut() {
            d.t = (d.t - dt).max(0.0);
        }
        if self.state.diversion.as_ref().is_some_and(|d| d.t <= 0.0) {
            self.state.diversion = None;
        }

        let Some(infra_def) = self.def.infra.clone() else {
            return;
        };
        let mut events: Vec<Event> = Vec::new();
        let mut shorts: Vec<(String, String)> = Vec::new();
        let mut alarm_changed: Vec<(String, RoomId, bool)> = Vec::new();
        let mut sounding = false;

        {
            let Some(st) = self.state.infra.as_mut() else {
                return;
            };
            for zone in &infra_def.water_zones {
                let Some(z) = st.zones.get_mut(zone.id.as_str()) else {
                    continue;
                };
                if z.short_flash > 0.0 {
                    z.short_flash = (z.short_flash - dt).max(0.0);
                }
                if !z.released || z.level >= 1.0 {
                    continue;
                }
                z.level = clamp(z.level + dt / zone.fill_time, 0.0, 1.0);
                if !z.shorted && z.level >= c::WATER_SHORT_AT {
                    z.shorted = true;
                    z.short_flash = c::WATER_SHORT_FLASH;
                    shorts.push((zone.id.clone(), zone.room.as_str().to_owned()));
                }
            }
            for alarm in &infra_def.alarms {
                let want = match &alarm.trip {
                    AlarmTrip::Water { zone } => st
                        .zones
                        .get(zone.as_str())
                        .is_some_and(|z| z.released && z.level >= c::ALARM_WATER_TRIP_LEVEL),
                    AlarmTrip::PowerLost { watch } => {
                        self.state.dead_nodes.contains(watch.as_str())
                    }
                };
                let was = st.alarms.get(alarm.id.as_str()).copied() == Some(true);
                if want != was {
                    st.alarms.insert(alarm.id.clone(), want);
                    alarm_changed.push((alarm.id.clone(), alarm.room.clone(), want));
                }
                sounding |= want;
            }
        }

        for (alarm, room, on) in alarm_changed {
            events.push(Event::AlarmChanged {
                alarm,
                room,
                sounding: on,
            });
        }

        // A sounding alarm climbs the floor's existing alert meter, scaled by
        // the difficulty preset exactly as the boss call is.
        if sounding {
            self.state.alert = clamp(
                self.state.alert + c::ALARM_ALERT_RATE * self.preset.alert_gain * dt,
                0.0,
                100.0,
            );
        }

        self.events.extend(events);

        // Shorts: the room's electrics die through the ordinary power-loss
        // path — the same one a cascade uses — so a flood darkens and blinds
        // with no bespoke consequence code.
        for (zone, room) in shorts {
            let mut nodes: Vec<String> = Vec::new();
            nodes.push(light_node_id(room.as_str()));
            for (i, cam) in self.def.cameras.iter().enumerate() {
                if cam.room.as_str() == room {
                    nodes.push(camera_node_id(i));
                }
            }
            for door in &self.def.doors {
                if Self::room_id_at(&self.def, door.x).unwrap_or_default() == room {
                    nodes.push(door_node_id(door.id.clone()));
                }
            }
            nodes.push(valve_node_id(&zone));
            nodes.push(pump_node_id(&zone));
            let mut devices = 0usize;
            for node in nodes {
                if self.state.dead_nodes.contains(node.as_str()) {
                    continue;
                }
                let killed = apply_effects(
                    &self.def,
                    &mut self.state,
                    self.cfg,
                    &[Effect::DevicePowerLost(node)],
                );
                self.events.extend(killed);
                devices += 1;
            }
            self.events.push(Event::InfraShorted {
                zone,
                room: RoomId::from(room.as_str()),
                devices,
            });
        }
    }

    /// The observations available to an actor standing at `at_x`, gated by
    /// the archetype's own hearing (`invest_dist` / `invest_noise`). Public
    /// because it is the perception contract: frontends may *show* a player
    /// what the guard can perceive, and any future actor adapter (the Evil
    /// Weevil seam, see [`crate::scenario::infra`]) consumes exactly this —
    /// never the world state behind it.
    pub fn actor_observations(&self, at_x: f64) -> Vec<Observation> {
        if self.def.infra.is_none() && self.def.mole.is_none() {
            return Vec::new();
        }
        let hearing = infra::Hearing {
            dist: self.actor.stats.invest_dist,
            threshold: self.actor.stats.invest_noise,
        };
        infra::observations(&self.def, &self.state, at_x, hearing, &self.extra_noise())
    }

    /// Noise the hosted companion is making that the infrastructure layer
    /// does not own: the running synth diversion, and the mole's own digging
    /// and chewing. Canonical state only — the companion sim's runtime is
    /// part of the snapshot, so every seat agrees on the same noise.
    fn extra_noise(&self) -> Vec<NoiseSource> {
        let mut out = Vec::new();
        if let Some(d) = &self.state.diversion {
            if d.t > 0.0 {
                out.push(NoiseSource {
                    x: d.x,
                    room: room_at_or_last(&self.def, d.x),
                    intensity: d.intensity,
                    kind: ObservationKind::UnusualNoise,
                });
            }
        }
        if let Some(mole) = &self.mole {
            let ms = mole.state();
            let busy = ms.alive
                && matches!(
                    ms.state,
                    crate::companion::MoleState::DiggingTrap
                        | crate::companion::MoleState::SabotagingCable
                );
            if busy {
                out.push(NoiseSource {
                    x: ms.x,
                    room: room_at_or_last(&self.def, ms.x),
                    intensity: c::MOLE_DIG_NOISE,
                    kind: ObservationKind::UnusualNoise,
                });
            }
        }
        out
    }

    /// Where Billy's Assess mode would be pulled by infrastructure this tick:
    /// the strongest *heard* observation (unusual noise, or an alarm — alarms
    /// carry floor-wide). Seen-room observations (darkness, water at his
    /// feet, sparking electrics) do not pull: he is already standing in them.
    /// `None` on a floor without infrastructure or a companion, which keeps
    /// the canonical FSM untouched.
    pub(crate) fn infra_investigation_target(&self) -> Option<f64> {
        let obs = self.actor_observations(self.state.billy.x);
        let mut best: Option<&Observation> = None;
        for o in obs.iter().filter(|o| {
            matches!(
                o.kind,
                ObservationKind::UnusualNoise | ObservationKind::Alarm
            ) && (o.x - self.state.billy.x).abs() >= 15.0
        }) {
            // The hearing gate inside `observations` already applied this
            // actor's threshold and distance; the tie-break is strongest
            // first, earlier-declared first on equal intensity, so the choice
            // is deterministic.
            match best {
                Some(b) if o.intensity <= b.intensity => {}
                _ => best = Some(o),
            }
        }
        best.map(|o| o.x)
    }

    /// The announce-once guard-perception record: diff the observations
    /// Billy's senses currently carry against what he has already been seen
    /// to notice, announce the new episodes, and forget the ended ones so a
    /// recurrence is news again. Player-visible evidence that the guard
    /// reacts to observations (ADR-0013).
    pub(super) fn system_infra_watch(&mut self) {
        if self.def.infra.is_none() && self.def.mole.is_none() {
            return;
        }
        if self.state.phase != Phase::Crisis
            || self.state.billy.mode == crate::scenario::common::BillyMode::Offsite
        {
            return;
        }
        let obs = self.actor_observations(self.state.billy.x);
        let current: BTreeSet<String> = obs.iter().map(|o| o.notice_key()).collect();
        self.state.guard_noticed.retain(|k| current.contains(k));
        for o in &obs {
            if self.state.guard_noticed.insert(o.notice_key()) {
                self.events.push(Event::GuardObserved {
                    kind: o.kind,
                    room: o.room.clone(),
                });
            }
        }
    }

    /// The zone whose manual valve handle the body's centre at `cx` can
    /// reach, standing in `room`, and only while the zone is still dry — a
    /// flooding zone's handle is no longer a decision point. `None` on any
    /// floor without infrastructure.
    pub(crate) fn valve_handle_at(&self, room: Option<&str>, cx: f64) -> Option<String> {
        let infra_def = self.def.infra.as_ref()?;
        let infra = self.state.infra.as_ref()?;
        infra_def
            .water_zones
            .iter()
            .find(|z| {
                Some(z.room.as_str()) == room
                    && (cx - z.valve_x).abs() < c::VALVE_DIST
                    && !infra.zones.get(z.id.as_str()).is_some_and(|z| z.released)
            })
            .map(|z| z.id.clone())
    }

    /// The speed multiplier wading imposes on a body at `x` (1.0 on any
    /// floor without infrastructure, so existing movement math is untouched).
    pub(crate) fn wade_factor_at(&self, x: f64) -> f64 {
        infra::wade_factor(&self.def, &self.state, x)
    }

    /// The noise floor wading sets on a moving body at `x` (0.0 when dry).
    pub(crate) fn wade_noise_at(&self, x: f64) -> f64 {
        infra::wade_noise(&self.def, &self.state, x)
    }

    /// The water depth at `x`, for frontends and tests reading canonical
    /// state (never a render-side quantity).
    pub fn water_depth_at(&self, x: f64) -> Option<f64> {
        water_at(&self.def, &self.state, x)
    }
}

/// The room a world-x falls in, or the last room — the simulation's own
/// fallback, expressed as data for a [`NoiseSource`].
fn room_at_or_last(def: &crate::scenario::definition::ScenarioDefinition, x: f64) -> RoomId {
    infra::room_id_at(def, x)
        .map(RoomId::from)
        .unwrap_or_else(|| {
            def.rooms
                .last()
                .map(|r| r.id.clone())
                .unwrap_or_else(|| RoomId::from(""))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::command::RunConfig;
    use crate::scenario::pump_house::pump_house;
    use crate::scenario::state::DiversionState;

    #[test]
    fn heard_sources_inside_the_arrival_distance_do_not_restart_investigation() {
        for kind in [ObservationKind::UnusualNoise, ObservationKind::Alarm] {
            let mut sim = GhostLobbySim::new(pump_house(), RunConfig::standard(), 7)
                .expect("valid pump house");
            let alarm = sim.def.infra.as_ref().unwrap().alarms[0].clone();
            let source_x = infra::room_centre(&sim.def, alarm.room.as_str());
            match kind {
                ObservationKind::UnusualNoise => {
                    sim.state.diversion = Some(DiversionState {
                        x: source_x,
                        t: 1.0,
                        intensity: 1.0,
                    });
                }
                ObservationKind::Alarm => {
                    sim.state
                        .infra
                        .as_mut()
                        .unwrap()
                        .alarms
                        .insert(alarm.id, true);
                }
                _ => unreachable!(),
            }
            for offset in [-16.0_f64, -15.0, -14.0, 0.0, 14.0, 15.0, 16.0] {
                sim.state.billy.x = source_x + offset;
                assert!(
                    sim.actor_observations(sim.state.billy.x)
                        .iter()
                        .any(|o| o.kind == kind && o.x == source_x),
                    "the source must remain observable: {kind:?}, {offset}"
                );
                assert_eq!(
                    sim.infra_investigation_target(),
                    (offset.abs() >= 15.0).then_some(source_x),
                    "{kind:?}, {offset}"
                );
            }
        }
    }
}

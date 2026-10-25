//! Hosting a Moletaire companion (ADR-0013) inside a floor run.
//!
//! The companion sim ([`crate::companion::MoletaireSim`]) stays exactly what
//! it was ported as: deterministic, dt-stepped, typed commands in, typed
//! events out, one seeded RNG stream of its own. This module is only the
//! *host*: it builds the companion from the floor's declaration and the run
//! seed, folds its events into the world, and keeps the canonical snapshot in
//! step. Nothing here gives the companion (or anything watching it) a fact
//! the world did not already carry:
//!
//! * a synth decoy becomes a **diversion noise source** in canonical state —
//!   guards hear it through the same observation gate as a splash, and it
//!   pulls Billy's Assess patrol exactly the way any other unusual noise
//!   does. It never reads or reveals where the player is.
//! * a chewed cable becomes an ordinary
//!   [`crate::netsim::Effect::DevicePowerLost`] on the nearest device node —
//!   the failure, the darkness and the guard reaction all arrive through the
//!   infrastructure consequence model, not a companion shortcut.
//!
//! A floor with no [`crate::scenario::definition::MoleHostDef`] builds no
//! companion and takes none of these paths, so every existing replay is
//! unchanged. The mole's RNG is its own stream (the run seed folded with
//! [`c::MOLE_SEED_FOLD`]), so hosting one never moves the run's draw counts.

use super::GhostLobbySim;
use crate::companion::{MoleCommand, MoleEvent, MoleParams, MoletaireSim, moletaire};
use crate::netsim::Effect;
use crate::scenario::agents::apply_effects;
use crate::scenario::command::MoleOp;
use crate::scenario::common::MoleDenyReason;
use crate::scenario::constants as c;
use crate::scenario::event::Event;
use crate::scenario::floor_graph::{camera_node_id, door_node_id, light_node_id};
use crate::scenario::mathf::TICK_DT;
use crate::scenario::state::{DiversionState, MoleHostState};

impl GhostLobbySim {
    /// Build the live companion from the floor's declaration and the run's
    /// seed, fit its declared upgrades, and record it in canonical state.
    /// Called by `new` and `reset`; a floor with no mole clears both slots.
    /// Construction errors also clear both slots. Setup events are discarded.
    pub(super) fn build_mole(&mut self) {
        let Some(host) = self.def.mole.clone() else {
            self.mole = None;
            self.state.mole = None;
            return;
        };
        let params = MoleParams {
            id: "moletaire".to_owned(),
            x: host.spawn_x,
            y: self.def.world.floor,
            ..MoleParams::default()
        };
        let seed = self.seed ^ c::MOLE_SEED_FOLD;
        // The canonical companion definition always validates; a construction
        // failure degrades to "this floor has no mole" rather than panicking.
        let Ok(mut sim) = MoletaireSim::new(moletaire(), params, seed) else {
            self.mole = None;
            self.state.mole = None;
            return;
        };
        if host.underground {
            sim.apply(MoleCommand::MoveTo {
                target_x: host.spawn_x,
                underground: true,
            });
        }
        for ctype in &host.upgrades {
            sim.apply(MoleCommand::UpgradeCoprocessor { ctype: *ctype });
        }
        // Construction-side events are setup, not run news: the run's event
        // log starts clean, identically on every seat.
        let _ = sim.drain_events();
        self.state.mole = Some(MoleHostState::from_sim(&sim));
        self.mole = Some(sim);
    }

    /// Rebuild the live companion from the canonical state — the `restore`
    /// path. The host record travels inside every snapshot, so a restored
    /// seat resumes the companion's RNG mid-sequence exactly.
    /// An absent host record or a companion restore error leaves no live mole;
    /// restore errors are not propagated to the caller.
    pub(super) fn adopt_mole(&mut self) {
        self.mole = self
            .state
            .mole
            .as_ref()
            .and_then(|host| MoletaireSim::restore(host.to_snapshot()).ok());
    }

    /// Apply a hosted companion order and update its canonical host record.
    /// Paused or ended runs silently ignore the order. Otherwise emits
    /// `MoleDenied` for an absent or dead companion, insufficient digging
    /// depth, or a synth index unavailable with the fitted upgrade.
    pub(crate) fn mole_command(&mut self, op: MoleOp) {
        if self.paused || self.state.ended {
            return;
        }
        let deny = |reason| Event::MoleDenied { reason };
        if self.def.mole.is_none() {
            self.events.push(deny(MoleDenyReason::NotHosted));
            return;
        }
        let Some(mole) = self.mole.as_mut() else {
            self.events.push(deny(MoleDenyReason::NotHosted));
            return;
        };
        if !mole.state().alive {
            self.events.push(deny(MoleDenyReason::Gone));
            return;
        }
        // Precondition gates, so an order the companion cannot take is a
        // refusal the seat can read rather than a swallowed no-op.
        let tuning = &mole.definition().tuning;
        let depth_ok = mole.state().depth > tuning.dig_min_depth;
        let available = match op {
            MoleOp::MoveTo { .. } => true,
            MoleOp::Dig | MoleOp::Sabotage => depth_ok,
            MoleOp::Synth { index } => {
                let sounds = mole
                    .definition()
                    .coprocessors
                    .audio_sound_count(mole.state().coprocessors.audio_synthesiser);
                u32::from(index) < sounds
            }
        };
        if !available {
            self.events.push(deny(MoleDenyReason::Unavailable));
            return;
        }
        let cmd = match op {
            MoleOp::MoveTo { x, underground } => MoleCommand::MoveTo {
                target_x: x,
                underground,
            },
            MoleOp::Dig => MoleCommand::DigTrap,
            MoleOp::Sabotage => MoleCommand::SabotageCable,
            MoleOp::Synth { index } => MoleCommand::PlaySynthSound {
                sound_index: index as usize,
            },
        };
        mole.apply(cmd);
        self.state.mole = Some(MoleHostState::from_sim(mole));
    }

    /// One tick of the companion: step its own deterministic sim (no edibles
    /// are modelled on a scenario floor — the hunger clock still runs, as
    /// ported), fold each typed event into the world, and keep the canonical
    /// host record in step for snapshots. Runs after the signal network and
    /// before the infrastructure tick, so a chewed cable's darkness and a
    /// synth diversion's noise land before bodies and observations read them.
    pub(super) fn system_mole(&mut self) {
        // Step the companion and re-record its canonical host state inside a
        // block, so the fold below sees the whole `self` again: the world
        // consequences of a chewed cable go through `apply_effects`, which
        // borrows the definition and the state the companion is not part of.
        let (events, mx) = {
            let Some(mole) = self.mole.as_mut() else {
                return;
            };
            let events = mole.tick(TICK_DT, &[]);
            let mx = mole.state().x;
            self.state.mole = Some(MoleHostState::from_sim(mole));
            (events, mx)
        };
        for ev in events {
            // The companion's own typed event is the player-visible record.
            self.events.push(Event::Mole { mole: ev.clone() });
            match ev {
                // A synth decoy becomes a diversion in canonical state: a
                // position and a window, heard through the ordinary
                // observation gate. It says nothing about the player.
                MoleEvent::SynthSoundPlayed { .. } => {
                    self.state.diversion = Some(DiversionState {
                        x: mx,
                        t: c::MOLE_SYNTH_DURATION,
                        intensity: c::MOLE_SYNTH_NOISE,
                    });
                }
                // A chewed cable fells the nearest device node through the
                // ordinary power-loss path — infrastructure consequences,
                // not a companion-specific shortcut.
                MoleEvent::CableSabotaged { .. } => {
                    if let Some(node) = self.nearest_device_node(mx, c::MOLE_SABOTAGE_RADIUS) {
                        let killed = apply_effects(
                            &self.def,
                            &mut self.state,
                            self.cfg,
                            &[Effect::DevicePowerLost(node)],
                        );
                        self.events.extend(killed);
                    }
                }
                _ => {}
            }
        }
    }

    /// The derived device node closest to `x` within `radius`: doors, cameras
    /// and room lights, in that fixed scan order, strict-nearest wins and
    /// ties go to the earlier scan (deterministic). A mole chewing where no
    /// device runs hits nothing — a fizzle, not a panic.
    fn nearest_device_node(&self, x: f64, radius: f64) -> Option<String> {
        let mut best: Option<(f64, String)> = None;
        let mut consider = |node: String, at: f64| {
            let d = (at - x).abs();
            if d <= radius && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                best = Some((d, node));
            }
        };
        for door in &self.def.doors {
            consider(door_node_id(door.id.clone()), door.x);
        }
        for (i, cam) in self.def.cameras.iter().enumerate() {
            consider(camera_node_id(i), cam.x);
        }
        for room in &self.def.rooms {
            consider(light_node_id(room.id.as_str()), room.x + room.w / 2.0);
        }
        best.map(|(_, node)| node)
    }
}

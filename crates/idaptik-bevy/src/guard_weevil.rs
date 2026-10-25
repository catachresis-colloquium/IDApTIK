//! Evil Weevil driving the Ghost Lobby's guard, behind the game's own seam.
//!
//! This module is the Bevy half of the integration. It owns a [`WeevilHost`]
//! and, each tick, turns the sim's [`GuardPerception`] into a kernel
//! observation, asks the kernel what it wants to do, and hands the proposal back
//! to the sim through [`GhostLobbySim::set_guard_decision`]. The sim validates
//! it against its own entity lifecycle and movement limits and applies it — the
//! kernel never moves the guard.
//!
//! The brain is **selectable**: [`GuardBrainKind::Classic`] runs the existing
//! deterministic FSM untouched, so the kernel can be compared against it and
//! rolled back at run time. When the native kernel library is not built, the
//! plugin degrades to `Classic` and records why, rather than failing the app.

use bevy::prelude::*;
use idaptik_core::scenario::guard_brain::{
    GuardApplyOutcome, GuardBrainKind, GuardContact, GuardDecision, GuardPerception,
};
use idaptik_core::scenario::GhostLobbySim;
use idaptik_weevil::fx::{Fx2, bearing_to, from_fx, to_fx};
use idaptik_weevil::intent::Intent;
use idaptik_weevil::observation::{Contact, ContactSource, Observation};
use idaptik_weevil::profile::{Profile, ProfileSet};
use idaptik_weevil::WeevilHost;

use crate::driver::SimState;

/// The live state of the guard brain, read by the developer view.
#[derive(Resource, Default, Debug)]
pub struct GuardBrainState {
    /// Which brain is selected.
    pub kind: GuardBrainKind,
    /// The profile in use, when the kernel is loaded.
    pub profile_id: Option<String>,
    /// Whether the native kernel library loaded and initialised.
    pub kernel_loaded: bool,
    /// Why the kernel is not loaded, if it is not.
    pub load_error: Option<String>,
    /// The kernel/ABI versions, recorded once at init.
    pub kernel_version: Option<(u16, u16, u16)>,
    pub abi_fingerprint: Option<u64>,
    /// The contacts the guard perceived on the last tick.
    pub contacts: Vec<GuardContact>,
    /// The decision the kernel proposed last tick.
    pub last_decision: Option<GuardDecision>,
    /// The host's acceptance/application outcome for the last proposal.
    pub last_outcome: Option<GuardApplyOutcome>,
    /// Any error the adapter raised on the last tick.
    pub last_error: Option<String>,
}

/// Owns the kernel host and the profile, and drives the sim from it.
#[derive(Resource)]
pub struct GuardBrain {
    host: Option<WeevilHost>,
    profile: Option<Profile>,
    /// fx units per world unit.
    scale: f64,
    pub state: GuardBrainState,
}

impl Default for GuardBrain {
    fn default() -> Self {
        Self {
            host: None,
            profile: None,
            scale: 1.0,
            state: GuardBrainState::default(),
        }
    }
}

impl GuardBrain {
    /// Initialise the brain from a profile id and a kernel seed.
    ///
    /// A failure to load the native library is not fatal: the brain stays
    /// unloaded, the sim keeps running the classic FSM, and the reason is
    /// recorded for the developer view.
    pub fn new(profile_id: &str, seed: u64) -> Self {
        let mut state = GuardBrainState {
            kind: GuardBrainKind::Classic,
            ..Default::default()
        };
        let Some(profile) = ProfileSet::canonical().get(profile_id) else {
            state.load_error = Some(format!("unknown profile '{profile_id}'"));
            return Self {
                host: None,
                profile: None,
                scale: 1.0,
                state,
            };
        };
        let host = match WeevilHost::init(
            profile.capabilities,
            profile.max_agents,
            profile.budget_units,
            seed,
            60,
            1,
        ) {
            Ok(host) => Some(host),
            Err(e) => {
                state.load_error = Some(format!("ee_init refused: {e}"));
                None
            }
        };
        if let Some(host) = host.as_ref() {
            state.kernel_loaded = true;
            state.profile_id = Some(profile_id.to_owned());
            let v = host.kernel_version();
            state.kernel_version = Some((v.major, v.minor, v.patch));
            state.abi_fingerprint = Some(host.kernel_fingerprint());
        }
        Self {
            host,
            profile: Some(profile),
            scale: profile.host.fx_per_world_unit,
            state,
        }
    }

    /// Whether the kernel is loaded and ready to drive.
    pub fn is_loaded(&self) -> bool {
        self.host.is_some()
    }

    /// Convert the sim's perception into the adapter's observation.
    ///
    /// The bearing is computed from the *relative* direction in world units and
    /// converted at the seam; the kernel sees a bearing, never a position.
    fn observation(&self, p: &GuardPerception) -> Observation {
        let to_fx_world = |w: f64| to_fx(w * self.scale);
        let mut contacts = Vec::with_capacity(p.contacts.len());
        for c in &p.contacts {
            let bearing = bearing_to(
                Fx2 {
                    x: to_fx_world(c.x - p.self_x),
                    y: to_fx_world(c.y - p.self_y),
                },
                Fx2::ZERO,
            );
            let source = match c.source.as_str() {
                "decoy" => ContactSource::Decoy,
                "sound" => ContactSource::Sound,
                "player" => ContactSource::Player,
                other => ContactSource::Object(other_id(other)),
            };
            contacts.push(Contact {
                id: c.id,
                source,
                bearing,
                distance: to_fx_world(c.distance),
                threat: to_fx_world(c.threat),
                confidence: c.confidence,
                age_ticks: c.age_ticks,
            });
        }
        Observation {
            agent_id: 0,
            tick: p.tick,
            self_pos: Fx2 {
                x: to_fx_world(p.self_x),
                y: to_fx_world(p.self_y),
            },
            self_vel: Fx2::ZERO,
            health: to_fx_world(p.health),
            ammo: to_fx_world(p.ammo),
            capability_mask: self.profile.as_ref().map_or(0, |p| p.capabilities),
            contacts,
            cooldown: 0,
        }
    }

    /// Ask the kernel for a proposal and queue it on the sim.
    ///
    /// Runs before [`crate::driver::step_sim`], so the proposal is applied by
    /// the sim on the tick about to run.
    pub fn propose(&mut self, sim: &mut GhostLobbySim) {
        if self.state.kind != GuardBrainKind::Kernel || self.host.is_none() {
            return;
        }
        let perc = sim.guard_perception();
        self.state.contacts = perc.contacts.clone();
        let obs = self.observation(&perc);
        let Some(host) = self.host.as_mut() else {
            return;
        };
        match host.tick(&obs) {
            Ok(outcome) => {
                let decision = decision_from(&outcome.intent);
                self.state.last_decision = Some(decision.clone());
                self.state.last_error = None;
                sim.set_guard_decision(Some(decision));
            }
            Err(e) => {
                self.state.last_error = Some(e.to_string());
                sim.set_guard_decision(None);
            }
        }
        self.state.last_outcome = sim.last_guard_outcome();
    }
}

/// Parse an `"object:<id>"` source label back to its id, refusing anything that
/// is not a usable integer.
fn other_id(label: &str) -> u32 {
    label
        .strip_prefix("object:")
        .and_then(|rest| rest.parse::<u32>().ok())
        .unwrap_or(0)
}

/// Convert a kernel intent into the game's decision type, in the adapter's own
/// vocabulary, so the developer view shows one language.
pub fn decision_from(intent: &Intent) -> GuardDecision {
    GuardDecision {
        action: intent.action.name().to_owned(),
        target_id: intent.target_id,
        x: 0.0,
        y: 0.0,
        speed: from_fx(intent.speed),
        priority: intent.priority as u64,
        degraded: intent.flags.degraded,
        new_target: intent.flags.new_target,
        unreachable: intent.flags.unreachable,
        from_memory: intent.flags.from_memory,
    }
}

/// Toggle the guard brain between the kernel and the classic FSM, then queue the
/// kernel's proposal for the tick about to run.
///
/// `V` flips the brain at run time: same binary, same world, no rebuild. That is
/// the comparison and the rollback the seam exists for. `V` is free in both
/// keymaps — the infiltrator claims A/D/S/W/Shift/E/Q, the hacker 1–4/P/G/X.
///
/// The toggle runs in `Update`, not `FixedUpdate`: `just_pressed` is a per-frame
/// fact and a `FixedUpdate` system can run several times in one frame, which would
/// flip the brain twice and leave it exactly where it started.
///
/// The sim stays the authority. `set_guard_brain` clears any queued proposal, and
/// a kernel brain with no kernel loaded proposes nothing, so the sim falls through
/// to the classic FSM rather than leaving the guard standing still.
pub fn guard_brain_toggle(
    mut brain: ResMut<GuardBrain>,
    mut sim: ResMut<SimState>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    if !keys.just_pressed(KeyCode::KeyV) {
        return;
    }
    let next = match sim.sim.guard_brain() {
        GuardBrainKind::Classic => GuardBrainKind::Kernel,
        GuardBrainKind::Kernel => GuardBrainKind::Classic,
    };
    sim.sim.set_guard_brain(next);
    brain.state.kind = next;
}

/// Queue the kernel's proposal for the tick about to run, then let the sim
/// apply it inside its own tick.
pub fn guard_brain_step(mut brain: ResMut<GuardBrain>, mut sim: ResMut<SimState>) {
    brain.propose(&mut sim.sim);
}

/// Insert the brain resource and the systems that drive it.
#[derive(Resource, Default)]
pub struct GuardBrainPlugin {
    pub profile_id: String,
    pub seed: u64,
}

impl GuardBrainPlugin {
    /// Build the plugin for a profile and seed.
    #[must_use]
    pub fn for_profile(profile_id: impl Into<String>, seed: u64) -> Self {
        Self {
            profile_id: profile_id.into(),
            seed,
        }
    }
}

impl Plugin for GuardBrainPlugin {
    fn build(&self, app: &mut App) {
        let mut brain = GuardBrain::new(&self.profile_id, self.seed);
        if brain.is_loaded() {
            brain.state.kind = GuardBrainKind::Kernel;
        }
        app.insert_resource(brain)
            .add_systems(
                Update,
                guard_brain_toggle.run_if(in_state(crate::net_view::AppMode::GhostLobby)),
            )
            .add_systems(
                FixedUpdate,
                guard_brain_step.before(crate::driver::step_sim),
            );
    }
}

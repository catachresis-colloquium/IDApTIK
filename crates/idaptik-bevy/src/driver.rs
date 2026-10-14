//! The sim driver: [`GhostLobbySim`] as a Bevy resource, stepped at a fixed
//! 60 Hz from a queued [`Command`] stream — the same wire API the TUI speaks.
//!
//! This module is deliberately render-free: a headless test `App` can add
//! [`SimDriverPlugin`] alone and drive [`FixedUpdate`] by hand, which is how
//! the frontend-parity test in `tests/parity.rs` works.

use bevy::prelude::*;
use idaptik_core::RunConfig;
use idaptik_core::interp::{DoorBuffer, PoseBuffer, door_opens_of, poses_of};
use idaptik_core::scenario::command::{Buttons, Command, fold};
use idaptik_core::scenario::event::{Event as SimEvent, LogLine};
use idaptik_core::scenario::{GhostLobbySim, ghost_lobby, log_view};

/// The simulation tick rate, matching [`idaptik_core::scenario::TICK_DT`].
pub const SIM_HZ: f64 = 60.0;

/// How many rendered log lines the HUD keeps.
const LOG_CAP: usize = 200;

/// The authoritative simulation plus its pure event views.
#[derive(Resource)]
pub struct SimState {
    /// The gameplay truth. The frontend only ever calls `tick` on it.
    pub sim: GhostLobbySim,
    /// Rendered log lines (a pure view of the events; cleared on restart).
    pub log: Vec<LogLine>,
    /// Every event the run has emitted, in order (the deterministic artifact).
    pub event_log: Vec<SimEvent>,
}

impl SimState {
    /// Build a fresh run and ingest its startup events.
    pub fn new(cfg: RunConfig, seed: u32) -> Result<Self, String> {
        Self::with_definition(ghost_lobby(), cfg, seed)
    }

    /// Build a fresh run of an explicit floor (the wired Ghost Lobby, say).
    pub fn with_definition(
        def: idaptik_core::scenario::ScenarioDefinition,
        cfg: RunConfig,
        seed: u32,
    ) -> Result<Self, String> {
        let mut sim =
            GhostLobbySim::new(def, cfg, seed).map_err(|e| format!("invalid scenario: {e:?}"))?;
        let startup = sim.drain_events();
        let mut state = Self {
            sim,
            log: Vec::new(),
            event_log: Vec::new(),
        };
        state.ingest(startup);
        Ok(state)
    }

    /// Build a render-facing view from an already constructed simulation.
    ///
    /// Netplay keeps gameplay truth inside `LockstepCore`; this clone is a
    /// one-way view for Bevy's scene and HUD and is never fed back into it.
    pub fn from_sim(mut sim: GhostLobbySim) -> Self {
        let startup = sim.drain_events();
        let mut state = Self {
            sim,
            log: Vec::new(),
            event_log: Vec::new(),
        };
        state.ingest(startup);
        state
    }

    /// Refresh the render-only view after one or more lockstep ticks.
    pub(crate) fn mirror_lockstep_tick(&mut self, sim: &GhostLobbySim, events: &[SimEvent]) {
        if events
            .iter()
            .any(|event| matches!(event, SimEvent::Restarted { .. }))
        {
            self.log.clear();
        }
        self.sim = sim.clone();
        self.ingest(events.to_vec());
    }

    /// Replace the render-only view after network resynchronisation.
    pub(crate) fn mirror_lockstep_history(&mut self, sim: &GhostLobbySim, events: &[SimEvent]) {
        self.sim = sim.clone();
        self.log.clear();
        self.event_log.clear();
        self.ingest(events.to_vec());
    }

    /// Append fresh events to the event log and render them into log lines.
    fn ingest(&mut self, events: Vec<SimEvent>) {
        let tick = self.sim.current_tick();
        let t = self.sim.time();
        for e in &events {
            if let Some(line) = log_view(e, tick, t) {
                self.log.push(line);
            }
        }
        if self.log.len() > LOG_CAP {
            let drop = self.log.len() - LOG_CAP;
            self.log.drain(0..drop);
        }
        self.event_log.extend(events);
    }
}

/// The last two simulated ticks of visual state, for render interpolation.
///
/// The sim advances at [`SIM_HZ`]; the display refreshes at whatever rate it
/// likes. Reading live sim state every frame would show each position for a
/// varying number of frames — visible judder on anything that is not exactly
/// 60 Hz. Keeping tick *N* alongside tick *N+1* lets [`crate::scene`] draw
/// between them.
///
/// This is a pure view. Nothing here is read by the simulation.
#[derive(Resource)]
pub struct VisualBuffers {
    /// Actor and prop poses, indexed by `VisualSlot`.
    pub poses: PoseBuffer,
    /// Door openness, indexed to match `state().doors`.
    pub doors: DoorBuffer,
}

impl VisualBuffers {
    /// Both ticks seeded from the run's starting state, so the first rendered
    /// interval is stationary rather than a slide from the origin.
    #[must_use]
    pub fn primed(sim: &GhostLobbySim) -> Self {
        let mut poses = PoseBuffer::new();
        let mut doors = DoorBuffer::new();
        poses.prime(&poses_of(sim.state()));
        doors.prime(&door_opens_of(sim.state()));
        Self { poses, doors }
    }

    /// Record the tick that just completed.
    ///
    /// `discontinuous` marks a tick the actors did not *travel* through — a
    /// restart. The fresh state is committed either way, so it always enters
    /// the buffer; the extra `snap` then discards the stale previous tick that
    /// would otherwise render as a slide across the jump. Committing first is
    /// the whole point: snapping alone would leave the new state out of the
    /// buffer and keep drawing the pre-restart position.
    pub fn commit(&mut self, sim: &GhostLobbySim, discontinuous: bool) {
        self.poses.commit(&poses_of(sim.state()));
        self.doors.commit(&door_opens_of(sim.state()));
        if discontinuous {
            self.poses.snap();
            self.doors.snap();
        }
    }
}

/// Commands queued by the frontend between ticks, plus the persistent
/// held-button set [`fold`] mutates — the exact seam the TUI's `InputState`
/// occupies.
#[derive(Resource, Default)]
pub struct CommandQueue {
    /// Commands accumulated since the last tick, in stream order.
    pub pending: Vec<Command>,
    /// The held movement buttons, carried across ticks.
    pub held: Buttons,
    /// Frontend-side pause latch (the sim is told via `Command::Pause`).
    pub paused: bool,
}

impl CommandQueue {
    /// Queue one command for the next tick.
    pub fn push(&mut self, cmd: Command) {
        self.pending.push(cmd);
    }
}

/// Inserts the sim, the command queue and the 60 Hz fixed timestep, and steps
/// the simulation every [`FixedUpdate`].
pub struct SimDriverPlugin {
    pub cfg: RunConfig,
    pub seed: u32,
    /// Play the played floor: the Ghost Lobby with its signal network
    /// attached (ADR-0012), started from the roof (`scenario::rooftop`).
    /// Off for the parity tests, which replay the canonical unwired floor.
    pub wired: bool,
}

impl Plugin for SimDriverPlugin {
    fn build(&self, app: &mut App) {
        // The played floor starts on the roof (see `scenario::rooftop`).
        let def = if self.wired {
            idaptik_core::scenario::ghost_lobby_rooftop()
        } else {
            ghost_lobby()
        };
        let state = SimState::with_definition(def, self.cfg, self.seed)
            .expect("the canonical Ghost Lobby definition is valid");
        let visual = VisualBuffers::primed(&state.sim);
        app.insert_resource(state)
            .insert_resource(visual)
            .init_resource::<CommandQueue>()
            .insert_resource(Time::<Fixed>::from_hz(SIM_HZ))
            .add_systems(FixedUpdate, step_sim);
    }
}

/// One fixed tick: drain the queued commands, fold them into a `TickInput`,
/// advance the sim, ingest the events it emitted, and record the new state for
/// render interpolation.
pub fn step_sim(
    mut queue: ResMut<CommandQueue>,
    mut state: ResMut<SimState>,
    mut visual: ResMut<VisualBuffers>,
) {
    let cmds = std::mem::take(&mut queue.pending);
    let input = fold(&cmds, &mut queue.held);
    let events = state.sim.tick(&input);
    let restarted = events
        .iter()
        .any(|e| matches!(e, SimEvent::Restarted { .. }));
    if restarted {
        state.log.clear();
    }
    visual.commit(&state.sim, restarted);
    state.ingest(events);
}

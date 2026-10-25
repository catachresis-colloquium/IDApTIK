//! The deterministic fixed-tick simulation: [`GhostLobbySim`].
//!
//! A [`GhostLobbySim`] owns the definition, the run config, the RNG, the tick
//! counter and the [`RuntimeState`]. Each call to [`GhostLobbySim::tick`] advances
//! exactly one 60 Hz frame — applying immediates before the systems, then running
//! the systems in a fixed, load-bearing order (see `systems.rs`, plus the
//! companion and infrastructure hosts in `mole.rs`/`infra.rs`), then the
//! post-systems phase/lockdown checks — and returns the typed events it emitted.
//!
//! A floor may also *host* optional subsystems: a Moletaire companion
//! ([`crate::companion`], ADR-0013) and building infrastructure — water zones
//! and alarms ([`crate::scenario::infra`]). Both are declared per-floor, both
//! no-op completely when absent, and both keep their changing half in the
//! canonical snapshot, so hosting never forks authority or breaks replay.
//!
//! The code path is panic-free: no `unwrap`/`expect`/panicking index; fallible
//! construction returns `Result`; arithmetic is clamped; matches are exhaustive.

mod console;
mod infra;
mod mole;
mod signal;
mod systems;

use crate::companion::MoletaireSim;
use crate::netsim::graph::GroundedGraph;
use crate::scenario::actor::{ComposedActor, billy_actor};
use crate::scenario::agents::apply_effects;
use crate::scenario::command::{NetNodeIndex, PivotTarget, RunConfig, TickInput};
use crate::scenario::common::{
    BillyMode, CrisisReason, DenyReason, ExtractMethod, FailReason, Outcome, Phase, ReportedTarget,
    Tone,
};
use crate::scenario::constants as c;
use crate::scenario::definition::{RoomDef, ScenarioDefinition, ValidationError};
use crate::scenario::event::Event;
use crate::scenario::floor_graph::{
    VACUUM_NODE_ID, camera_node_id, door_node_id, floor_graph, inside_vantage, light_node_id,
    pivot_host,
};
use crate::scenario::ids::IdIndex;
use crate::scenario::mathf::{TICK_DT, clamp, js_round, sign_or};
use crate::scenario::outcome::{
    DEBRIEF_FORMAT, Debrief, ScoreBreakdown, Tag, debrief_text, grade_for,
};
use crate::scenario::rng::{Mulberry32, roll_init};
use crate::scenario::snapshot::{RuntimeSnapshot, SNAPSHOT_FORMAT};
use crate::scenario::state::{DoorState, RuntimeState};
use crate::scenario::tuning::{ActionKind, DifficultyPreset};

/// The playable Ghost Lobby simulation — forward-compatible as a UMS floor.
#[derive(Debug, Clone)]
pub struct GhostLobbySim {
    def: ScenarioDefinition,
    cfg: RunConfig,
    seed: u32,
    rng: Mulberry32,
    tick: u64,
    state: RuntimeState,
    idx: IdIndex,
    /// The active difficulty preset (resolved once; rebuilt on reset/restore).
    preset: DifficultyPreset,
    /// The floor's grounded network, derived from the definition once. It is a
    /// pure function of `def`, so a reset cannot stale it and a snapshot need not
    /// carry it; `restore` rebuilds it from the definition it is handed.
    graph: GroundedGraph,
    /// Billy expressed as data: the default archetype's composed form. Like
    /// `graph` it is a pure function of committed content, so a snapshot need
    /// not carry it and `restore`/`reset` cannot stale it. The FSM, sight and
    /// belief systems read every tunable number from here, so any registry
    /// archetype drives the same machinery.
    actor: ComposedActor,
    /// The hosted Moletaire companion, on a floor that declares one
    /// (ADR-0013). Unlike `graph` and `actor` it carries mutable state, so
    /// its changing half *is* snapshotted (`RuntimeState::mole`); `restore`
    /// rebuilds the live sim from that record and `new`/`reset` build it from
    /// the definition and seed.
    mole: Option<MoletaireSim>,
    paused: bool,
    events: Vec<Event>,
}

/// A last-resort preset so construction stays total even if a preset is missing;
/// [`GhostLobbySim::new`] rejects such definitions via [`ScenarioDefinition::ok`]
/// first, so this is never actually observed by a valid scenario.
fn fallback_preset() -> DifficultyPreset {
    DifficultyPreset {
        label: "FALLBACK".to_owned(),
        arrival: (0.0, 0.0),
        player_speed: 0.0,
        sprint: 1.0,
        billy_speed: 0.0,
        billy_sight: 0.0,
        support_limit: 1.0,
        bandwidth_regen: 0.0,
        badge_delay: 1.0,
        usb_timer: 0.0,
        camera_lock: 1.0,
        alert_gain: 1.0,
        score_mult: 1.0,
        rescue: false,
        // Every other field here is a benign zero, but a zero trace threshold is
        // not benign: `traced()` is `progress >= threshold`, so it would trip on
        // the first tick and end the run instantly. A last-resort preset must
        // fail safe, so it borrows the standard threshold.
        trace_threshold: c::STANDARD_TRACE_THRESHOLD,
    }
}

impl GhostLobbySim {
    /// Construct a new run. Validates the definition, seeds the RNG, performs the
    /// reset roll and emits `RunStarted` + `SeedAnnounced` (drain them with
    /// [`GhostLobbySim::drain_events`] before the first tick, or let the first
    /// tick return them ahead of its own events).
    /// Returns the definition's validation errors if it is invalid. Companion
    /// construction errors instead leave the run without a live companion.
    pub fn new(
        def: ScenarioDefinition,
        cfg: RunConfig,
        seed: u32,
    ) -> Result<Self, Vec<ValidationError>> {
        def.ok()?;
        let preset = def
            .difficulty
            .get(&cfg.difficulty)
            .cloned()
            .unwrap_or_else(fallback_preset);
        let idx = IdIndex::resolve(&def);
        let mut rng = Mulberry32::new(seed);
        let roll = roll_init(&def, &mut rng, cfg.difficulty);
        let state = RuntimeState::initial(&def, &roll, cfg, &preset);
        let graph = floor_graph(&def);
        let mut sim = Self {
            def,
            cfg,
            seed,
            rng,
            tick: 0,
            state,
            idx,
            preset,
            graph,
            actor: billy_actor(),
            mole: None,
            paused: false,
            events: Vec::new(),
        };
        sim.build_mole();
        sim.emit_start();
        Ok(sim)
    }

    /// Rebuild an equivalent sim from a snapshot and its definition. A snapshot
    /// tagged with any other format is refused up front, as a typed error,
    /// rather than left to fail later (or worse, restore wrongly) on shape.
    /// Also returns definition validation errors. Companion restore errors
    /// leave no live companion and are not returned as run restore errors.
    pub fn restore(
        def: ScenarioDefinition,
        snap: RuntimeSnapshot,
    ) -> Result<Self, Vec<ValidationError>> {
        if snap.format != SNAPSHOT_FORMAT {
            return Err(vec![ValidationError::UnsupportedSnapshotFormat {
                found: snap.format,
            }]);
        }
        def.ok()?;
        let preset = def
            .difficulty
            .get(&snap.cfg.difficulty)
            .cloned()
            .unwrap_or_else(fallback_preset);
        let idx = IdIndex::resolve(&def);
        let graph = floor_graph(&def);
        let mut sim = Self {
            def,
            cfg: snap.cfg,
            seed: snap.seed,
            rng: snap.rng,
            tick: snap.tick,
            state: snap.state,
            idx,
            preset,
            graph,
            actor: billy_actor(),
            mole: None,
            paused: snap.paused,
            events: Vec::new(),
        };
        sim.adopt_mole();
        Ok(sim)
    }

    /// Restart from `seed`, re-rolling the reset and emitting the start events.
    pub fn reset(&mut self, seed: u32) {
        self.seed = seed;
        self.rng = Mulberry32::new(seed);
        let roll = roll_init(&self.def, &mut self.rng, self.cfg.difficulty);
        self.tick = 0;
        self.state = RuntimeState::initial(&self.def, &roll, self.cfg, &self.preset);
        self.build_mole();
        self.paused = false;
        self.emit_start();
    }

    fn emit_start(&mut self) {
        self.events.push(Event::RunStarted {
            seed: self.seed,
            difficulty: self.cfg.difficulty,
            reduced_motion: self.cfg.reduced_motion,
            outside: self.def.exterior.is_some(),
        });
        self.events.push(Event::SeedAnnounced { seed: self.seed });
    }

    // --- accessors ---------------------------------------------------------

    /// Borrow the runtime state.
    pub fn state(&self) -> &RuntimeState {
        &self.state
    }

    /// Borrow the definition.
    pub fn definition(&self) -> &ScenarioDefinition {
        &self.def
    }

    /// Borrow the floor's grounded network.
    pub fn graph(&self) -> &GroundedGraph {
        &self.graph
    }

    /// Borrow the composed actor whose archetype drives Billy's FSM.
    pub fn actor(&self) -> &ComposedActor {
        &self.actor
    }

    /// The resolved id index.
    pub fn id_index(&self) -> &IdIndex {
        &self.idx
    }

    /// The run config.
    pub fn config(&self) -> RunConfig {
        self.cfg
    }

    /// The current tick number.
    pub fn current_tick(&self) -> u64 {
        self.tick
    }

    /// The run seed.
    pub fn seed(&self) -> u32 {
        self.seed
    }

    /// The current physics time (accumulated `t`).
    pub fn time(&self) -> f64 {
        self.state.t
    }

    /// Whether the run has ended.
    pub fn is_ended(&self) -> bool {
        self.state.ended
    }

    /// Whether the run is paused.
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// The after-action debrief, if the run has ended.
    pub fn debrief(&self) -> Option<&Debrief> {
        self.state.result.as_ref()
    }

    /// Take the pending events (the `RunStarted`/`SeedAnnounced` after `new`).
    pub fn drain_events(&mut self) -> Vec<Event> {
        self.take_events()
    }

    /// The single seam through which events leave the sim: in a supervised run
    /// the pending buffer is folded into the supervision state first, so every
    /// canonical event is observed exactly once, in order, on every run path —
    /// TUI, headless, and networked seats alike. The announce-once events the
    /// fold itself emits are appended after the fold and are never re-folded.
    fn take_events(&mut self) -> Vec<Event> {
        if self.cfg.supervised && !self.events.is_empty() {
            self.system_supervision();
        }
        std::mem::take(&mut self.events)
    }

    /// Fold the pending events into `state.supervision`, then make any changed
    /// allocation visible in the canonical log (announce-once, mirroring the
    /// `belief_announced` pattern).
    fn system_supervision(&mut self) {
        let sup = &mut self.state.supervision;
        sup.supervisor.ingest(&self.events);
        let attention = sup.supervisor.team.attention;
        let team_id = sup.supervisor.team.id.clone();
        let target = sup.supervisor.coverage_target.clone();
        if attention != sup.announced_attention {
            sup.announced_attention = attention;
            self.events
                .push(Event::TeamAttentionAllocated { team_id, attention });
        }
        if let Some(target) = target
            && sup.announced_target.as_deref() != Some(target.as_str())
        {
            sup.announced_target = Some(target.clone());
            self.events.push(Event::CoverageRetargeted { target });
        }
    }

    /// The definition export surface (definition + validation report).
    pub fn definition_export(&self) -> crate::scenario::snapshot::DefinitionExport {
        crate::scenario::snapshot::DefinitionExport {
            format: self.def.format.clone(),
            definition: self.def.clone(),
            validation: self.def.validate(),
        }
    }

    /// The debrief export surface (present once the run has ended).
    pub fn debrief_export(&self) -> crate::scenario::snapshot::DebriefExport {
        crate::scenario::snapshot::DebriefExport {
            format: DEBRIEF_FORMAT.to_owned(),
            debrief: self.state.result.clone(),
        }
    }

    /// The combined Exchange-House-style export: definition + snapshot + debrief
    /// + the caller-supplied canonical event log.
    pub fn export(&self, event_log: Vec<Event>) -> crate::scenario::snapshot::ScenarioExport {
        crate::scenario::snapshot::ScenarioExport {
            format: crate::scenario::snapshot::EXPORT_FORMAT.to_owned(),
            definition: self.def.clone(),
            snapshot: self.snapshot(),
            debrief: self.state.result.clone(),
            event_log,
        }
    }

    /// A full, restorable snapshot at the current tick.
    pub fn snapshot(&self) -> RuntimeSnapshot {
        RuntimeSnapshot {
            format: SNAPSHOT_FORMAT.to_owned(),
            definition: self.def.clone(),
            cfg: self.cfg,
            seed: self.seed,
            tick: self.tick,
            rng: self.rng.clone(),
            state: self.state.clone(),
            paused: self.paused,
            validation: self.def.validate(),
        }
    }

    // --- the tick ----------------------------------------------------------

    /// Advance exactly one fixed 60 Hz frame, returning the emitted events.
    pub fn tick(&mut self, input: &TickInput) -> Vec<Event> {
        // PRE — P0: pause / restart (mirror synchronous keydown handlers).
        for imm in &input.immediates {
            match imm {
                crate::scenario::command::Command::Pause { on } if *on != self.paused => {
                    self.paused = *on;
                    self.events
                        .push(if *on { Event::Paused } else { Event::Resumed });
                }
                crate::scenario::command::Command::Restart => {
                    self.events.push(Event::Restarted { seed: self.seed });
                    self.reset(self.seed);
                    return self.take_events();
                }
                _ => {}
            }
        }
        if self.paused {
            return self.take_events();
        }

        // P2 — other immediates at the pre-increment `t`.
        if !self.state.ended {
            for imm in &input.immediates {
                if self.state.ended {
                    break;
                }
                match imm {
                    crate::scenario::command::Command::Uplink { kind } => {
                        self.perform_action(*kind);
                    }
                    crate::scenario::command::Command::Pivot { target } => {
                        self.pivot(*target);
                    }
                    crate::scenario::command::Command::Unpivot => {
                        self.unpivot();
                    }
                    crate::scenario::command::Command::NetSsh { node } => {
                        self.net_ssh(*node);
                    }
                    crate::scenario::command::Command::NetHack { node } => {
                        self.net_hack(*node);
                    }
                    crate::scenario::command::Command::Signal { op } => {
                        self.signal_command(*op);
                    }
                    crate::scenario::command::Command::Mole { op } => {
                        self.mole_command(*op);
                    }
                    crate::scenario::command::Command::ForceCrisis => {
                        self.begin_crisis(CrisisReason::Test);
                    }
                    crate::scenario::command::Command::ForceExtract { method } => {
                        self.extract(*method);
                    }
                    crate::scenario::command::Command::ForceFail { reason } => {
                        self.fail_mission(*reason);
                    }
                    _ => {}
                }
            }
        }

        // P3 — if ended, do not run the update.
        if !self.state.ended {
            self.update(input);
        }

        self.take_events()
    }

    /// The system pipeline for one non-ended, non-paused frame.
    fn update(&mut self, input: &TickInput) {
        self.state.t += TICK_DT;
        self.tick += 1;

        self.system_timers();
        // Hands busy at a wiring panel hold still and touch nothing else. A
        // run that never wires never sets the timer, so this is the input
        // unchanged.
        // Sitting at a console is the same: still, but interact still works,
        // because interact is how you get up again.
        let idle;
        let input = if self.state.signal_busy > 0.0 {
            idle = TickInput::default();
            &idle
        } else if self.state.using_console.is_some() {
            idle = TickInput {
                edges: input
                    .edges
                    .iter()
                    .copied()
                    .filter(|e| *e == crate::scenario::command::Edge::InteractPress)
                    .collect(),
                ..TickInput::default()
            };
            &idle
        } else {
            input
        };
        self.system_signal();
        // The companion and the infrastructure land before anything moves: a
        // chewed cable's darkness, a released zone's water and a sounding
        // alarm are all facts of this tick by the time bodies and perceptions
        // read them. Both no-op entirely on a floor without them.
        self.system_mole();
        self.system_infra();
        self.system_player(input);

        //## The network vantage follows the body
        // The infiltrator plays from wherever they physically stand, so the vantage
        // is recomputed the moment the body has moved and before anything reads it.
        // `set_vantage` drops the pivot stack, which is the whole point: a foothold
        // cannot be carried down the corridor. It is therefore called only when the
        // room they now stand in offers a different vantage from the one they hold;
        // calling it every tick would wipe a pivot they had only just made. A room
        // the graph gives no vantage leaves them where they were rather than
        // unseating them, since the fuzz test drives this with any definition.
        if !self.infiltrator_outside()
            && let Some(room) = Self::room_id_at(&self.def, self.state.player.x)
            && let Some(vantage) = inside_vantage(&self.graph, room)
            && vantage.entry_ip != self.state.agents.infiltrator.vantage().entry_ip
        {
            self.state.agents.infiltrator.set_vantage(vantage);
        }

        //## Billy waits until the infiltrator is under way
        // On a floor that asks for it, the arrival countdown is frozen until
        // the infiltrator first leaves the room they spawned in: the seeded
        // roll still decides how long Billy then takes, but nobody is caught
        // while still learning the controls. Pulling the USB still starts the
        // crisis at once.
        if self.state.arrival_held {
            let spawn_room = Self::room_id_at(&self.def, self.def.player.spawn_x);
            let here = Self::room_id_at(&self.def, self.state.player.x);
            // Starting outside, the clock runs once they are in the building.
            let under_way = if self.def.exterior.is_some() {
                !self.infiltrator_outside()
            } else {
                here.is_some() && here != spawn_room
            };
            if under_way {
                self.state.arrival_held = false;
                self.events.push(Event::ArrivalClockStarted);
            } else {
                self.state.crisis_at += TICK_DT;
            }
        }

        self.system_interactions(input);
        if self.state.ended {
            return;
        }
        self.system_usb();
        self.system_vacuum();
        self.system_cameras();
        self.system_intel();
        self.system_support();
        if self.state.ended {
            return;
        }
        // The guard's perception of the building: announce-once, before the
        // belief and FSM systems act on what he can sense.
        self.system_infra_watch();
        self.system_behaviour();
        self.system_billy();
        self.system_collisions();
        if self.state.ended {
            return;
        }
        self.system_objectives();

        // POST — timer crisis (billy first moves NEXT tick) then lockdown.
        if self.state.phase == Phase::Quiet {
            self.state.quiet_remaining = (self.state.crisis_at - self.state.t).max(0.0);
            if self.state.quiet_remaining <= 0.0 {
                self.begin_crisis(CrisisReason::Timer);
            }
        }
        if self.state.alert >= c::LOCKDOWN && !self.state.ended {
            self.fail_mission(FailReason::Lockdown);
        }

        //## The trace is each agent's own clock
        // Checked here in POST beside Lockdown, so a hack that trips the trace still
        // lands its effects this tick before the run ends.
        //
        // Both peers are checked, and deliberately so. Typical play has only the
        // hacker reaching in from the van, but the two are symmetric: an infiltrator
        // working off their own segment traces exactly as the hacker does, and
        // whoever is traced, the run ends.
        if (self.state.agents.hacker.traced() || self.state.agents.infiltrator.traced())
            && !self.state.ended
        {
            self.fail_mission(FailReason::Traced);
        }
    }

    // --- shared helpers ----------------------------------------------------

    /// Room index containing world-x (uses the `+ROOM_OFFSET` centre; defaults to
    /// the last room, as the prototype does).
    pub(crate) fn room_index_at(def: &ScenarioDefinition, x: f64) -> usize {
        let cx = x + def.world.room_offset;
        def.rooms
            .iter()
            .position(|r| r.contains(cx))
            .unwrap_or(def.rooms.len().saturating_sub(1))
    }

    /// The id string of the room containing world-x, if any.
    pub(crate) fn room_id_at(def: &ScenarioDefinition, x: f64) -> Option<&str> {
        def.rooms
            .get(Self::room_index_at(def, x))
            .map(|r| r.id.as_str())
    }

    /// The room def containing world-x, if any.
    pub(crate) fn room_at(def: &ScenarioDefinition, x: f64) -> Option<&RoomDef> {
        def.rooms.get(Self::room_index_at(def, x))
    }

    /// The front door's x, on a floor with an outside.
    pub(crate) fn front_door_x(&self) -> Option<f64> {
        let ext = self.def.exterior.as_ref()?;
        self.def
            .doors
            .iter()
            .find(|d| d.id == ext.front_door)
            .map(|d| d.x)
    }

    /// Whether a body (left edge `x`, width `w`) on the floor line is out on
    /// the street, before the front door.
    fn on_street(&self, x: f64, w: f64) -> bool {
        self.front_door_x().is_some_and(|door| x + w / 2.0 < door)
    }

    /// Whether the infiltrator is outside the building: on the roof, on the
    /// rope, or on the street. Never, on a floor with no outside.
    pub fn infiltrator_outside(&self) -> bool {
        match self.state.player.place {
            crate::scenario::state::Place::Floor => {
                self.on_street(self.state.player.x, self.def.player.w)
            }
            _ => true,
        }
    }

    /// Whether Billy is out on the street.
    pub(crate) fn billy_outside(&self) -> bool {
        self.on_street(self.state.billy.x, self.def.billy.w)
    }

    /// Index of the nearest door to `x` (first wins ties).
    pub(crate) fn nearest_door(doors: &[DoorState], x: f64) -> Option<usize> {
        let mut best: Option<(usize, f64)> = None;
        for (i, d) in doors.iter().enumerate() {
            let gap = (d.x - x).abs();
            match best {
                Some((_, bg)) if gap >= bg => {}
                _ => best = Some((i, gap)),
            }
        }
        best.map(|(i, _)| i)
    }

    /// Push a new `x` back inside `[old_x, new_x]` when a closed door lies
    /// between the old and new centre; returns the constrained x and whether a
    /// door blocked the move.
    pub(crate) fn constrain_by_doors(
        doors: &[DoorState],
        old_x: f64,
        new_x: f64,
        w: f64,
    ) -> (f64, bool) {
        let old_center = old_x + w / 2.0;
        let mut x = new_x;
        let mut new_center = x + w / 2.0;
        let mut blocked = false;
        for door in doors {
            if door.open > 0.0 || door.unlocked {
                continue;
            }
            let crossed_right = old_center < door.x && new_center >= door.x;
            let crossed_left = old_center > door.x && new_center <= door.x;
            if !crossed_right && !crossed_left {
                continue;
            }
            if crossed_right {
                x = door.x - w / 2.0 - 3.0;
            } else {
                x = door.x - w / 2.0 + 3.0;
            }
            new_center = x + w / 2.0;
            blocked = true;
        }
        (x, blocked)
    }

    /// Whether Billy can currently see the player (recomputed on demand — never
    /// cached for the whole tick).
    pub(crate) fn can_billy_see_player(&self) -> bool {
        let s = &self.state;
        if s.phase != Phase::Crisis || s.billy.mode == BillyMode::Offsite {
            return false;
        }
        let p = &s.player;
        let b = &s.billy;
        if p.hidden || s.lights_flicker > 0.0 || b.stun > 0.0 {
            return false;
        }
        // A room whose lights a wire has switched off is dark to him too.
        if Self::room_id_at(&self.def, p.x).is_some_and(|room| self.signal_room_dark(room)) {
            return false;
        }
        if Self::room_index_at(&self.def, b.x) != Self::room_index_at(&self.def, p.x) {
            return false;
        }
        // Not up on the roof, and not through the building's outer wall.
        if p.place != crate::scenario::state::Place::Floor
            || self.infiltrator_outside() != self.billy_outside()
        {
            return false;
        }
        let a = &self.actor.stats;
        let dx = (p.x + self.def.player.w / 2.0) - (b.x + self.def.billy.w / 2.0);
        let in_front = sign_or(dx, b.facing) == b.facing;
        let mut range = self.preset.billy_sight * if in_front { 1.0 } else { a.sight_back };
        if p.crouching {
            range *= a.sight_crouch;
        }
        if let Some(room) = Self::room_at(&self.def, p.x) {
            range *= room.sight_multiplier;
        }
        if s.alert > a.sight_alert_hi {
            range *= a.sight_alert;
        }
        dx.abs() <= range
    }

    /// Change Billy's FSM mode, emitting `BillyStateChanged` on an actual change.
    pub(crate) fn set_billy_mode(&mut self, to: BillyMode) {
        let from = self.state.billy.mode;
        if from != to {
            self.state.billy.mode = to;
            self.events.push(Event::BillyStateChanged { from, to });
        }
    }

    /// Tip the floor into crisis (idempotent; only from the quiet phase).
    pub(crate) fn begin_crisis(&mut self, reason: CrisisReason) {
        if self.state.ended || self.state.phase != Phase::Quiet {
            return;
        }
        let from = self.state.phase;
        self.state.phase = Phase::Crisis;
        self.state.billy.x = c::CRISIS_BILLY_X;
        self.state.billy.state_timer = 1.1;
        let boost = if reason == CrisisReason::Usb {
            14.0
        } else {
            8.0
        };
        self.state.alert = self.state.alert.max(boost);
        self.events.push(Event::PhaseChanged {
            from,
            to: Phase::Crisis,
            reason: Some(reason),
        });
        self.set_billy_mode(BillyMode::Entering);
        self.events.push(Event::CrisisBegan { reason });
    }

    /// Which node an action targets, given who is acting and where they stand.
    /// `None` names no fixture the floor carries, which is a content gap rather
    /// than a play state; the caller shrugs rather than denying or panicking.
    fn action_target(&self, kind: ActionKind) -> Option<String> {
        match kind {
            ActionKind::Camera => {
                // The camera watching the room the agent stands in, or failing
                // that whichever the definition lists first.
                let here = Self::room_id_at(&self.def, self.state.player.x);
                let index = here
                    .and_then(|room| {
                        self.def
                            .cameras
                            .iter()
                            .position(|c| c.room.as_str() == room)
                    })
                    .or_else(|| (!self.def.cameras.is_empty()).then_some(0))?;
                Some(camera_node_id(index))
            }
            ActionKind::Door => {
                let centre = self.state.player.x + self.def.player.w / 2.0;
                let door = self
                    .state
                    .doors
                    .get(Self::nearest_door(&self.state.doors, centre)?)?;
                Some(door_node_id(door.id.clone()))
            }
            ActionKind::Vacuum => Some(VACUUM_NODE_ID.to_owned()),
            ActionKind::Lights => Some(light_node_id(Self::room_id_at(
                &self.def,
                self.state.player.x,
            )?)),
        }
    }

    /// Which `ActionKind`, if any, `node_id` belongs to -- independent of the
    /// player's current position, unlike `action_target` (which names only
    /// the contextually nearest fixture for Camera/Door/Lights). A Net View
    /// click on a specific node must run that node's action economy
    /// regardless of where the player happens to be standing.
    fn action_kind_of_node(&self, node_id: &str) -> Option<ActionKind> {
        if node_id == VACUUM_NODE_ID {
            return Some(ActionKind::Vacuum);
        }
        if (0..self.def.cameras.len()).any(|i| camera_node_id(i) == node_id) {
            return Some(ActionKind::Camera);
        }
        if self
            .state
            .doors
            .iter()
            .any(|d| door_node_id(d.id.clone()) == node_id)
        {
            return Some(ActionKind::Door);
        }
        if self
            .def
            .rooms
            .iter()
            .any(|r| light_node_id(r.id.as_str()) == node_id)
        {
            return Some(ActionKind::Lights);
        }
        None
    }

    /// Pivot the hacker onto a named foothold (immediate; self-guards on
    /// paused/ended).
    ///
    /// This is the verb the four uplinks were always waiting on: cold from the van
    /// every action answers `NoRoute`, so without a way to say this the hacker
    /// cannot act at all. A refusal is reported rather than swallowed, because
    /// which refusal it was is how the player learns the shape of the network:
    /// the grid jump host answers `NoRoute` until they have gone through the ISP.
    ///
    /// It costs no bandwidth and no cooldown. The price of reach is paid in the
    /// two currencies the model already has: the trace `ssh` advances, and the
    /// support the hop penalty frays for as long as they stand there.
    pub(crate) fn pivot(&mut self, target: PivotTarget) {
        if self.paused || self.state.ended {
            return;
        }
        self.ssh_and_report(pivot_host(target).to_owned());
    }

    /// Shared tail of `pivot`/`net_ssh`: attempt the ssh, report whichever of
    /// `PivotOpened`/`PivotDenied` results. `host` is owned because one caller
    /// only has a borrow to hand over (`pivot_host` is `&'static str`) while
    /// the other already owns a freshly-formatted `String`; taking ownership
    /// here lets both move it straight into the event instead of cloning.
    fn ssh_and_report(&mut self, host: String) {
        match self.state.agents.hacker.ssh(&self.graph, &host) {
            Ok(_) => {
                let hops = self.state.agents.hacker.hops();
                self.events.push(Event::PivotOpened { host, hops });
            }
            Err(reason) => self.events.push(Event::PivotDenied { host, reason }),
        }
    }

    /// Back the hacker out of one pivot (immediate; self-guards on paused/ended).
    /// Standing at the vantage there is nothing to pop, and nothing to report.
    pub(crate) fn unpivot(&mut self) {
        if self.paused || self.state.ended {
            return;
        }
        if self.state.agents.hacker.exit() {
            let hops = self.state.agents.hacker.hops();
            self.events.push(Event::PivotClosed { hops });
        }
    }

    /// Pivot the hacker onto an arbitrary graph node by index (Net View
    /// click; immediate, self-guards on paused/ended). Generalises `pivot`
    /// from a fixed `PivotTarget`-resolved host to any node in the graph.
    /// Costs exactly what a named pivot costs: nothing beyond the trace
    /// advance `ssh` already charges.
    pub(crate) fn net_ssh(&mut self, node: NetNodeIndex) {
        if self.paused || self.state.ended {
            return;
        }
        let Some(target) = self.graph.nodes.get(node.0 as usize) else {
            return;
        };
        self.ssh_and_report(target.ip.to_string());
    }

    /// Hack an arbitrary graph node by index (Net View click; immediate,
    /// self-guards on paused/ended). A node already reachable through one of
    /// the four uplink actions (judged by identity via `action_kind_of_node`,
    /// not by the player's current position) runs through the exact same
    /// economy as that action. Every other actuatable node -- today, only the
    /// substation -- runs a new, separate flat-cost gate: a genuine new
    /// capability, since nothing before this reached it from any `Command`.
    pub(crate) fn net_hack(&mut self, node: NetNodeIndex) {
        if self.paused || self.state.ended {
            return;
        }
        let Some(target) = self.graph.nodes.get(node.0 as usize) else {
            return;
        };
        let node_id = target.id.clone();
        if let Some(kind) = self.action_kind_of_node(&node_id) {
            self.perform_action_on(kind, Some(node_id));
            return;
        }

        let t = self.state.t;
        // Denials are throttled like every other repeated-press event in this
        // sim (`Throttles`' whole purpose): a click that keeps landing on a
        // cooling-down or unaffordable target must not flood the log the way
        // a held key on the keyboard path never could.
        let throttled_deny = |sim: &mut Self, node_id: String, reason: DenyReason| {
            sim.state.stats.failed_actions += 1;
            let slot = &mut sim.state.throttles.net_hack_denial;
            if t - *slot >= c::THROTTLE_CDBW {
                *slot = t;
                sim.events.push(Event::NetHackDenied { node_id, reason });
            }
        };

        if self.state.net_hack_cd > 0.0 {
            throttled_deny(self, node_id, DenyReason::Cooldown);
            return;
        }
        if self.state.bandwidth < c::NET_HACK_COST {
            throttled_deny(self, node_id, DenyReason::Bandwidth);
            return;
        }
        let effects = match self.state.agents.hacker.hack(&self.graph, &node_id) {
            Ok(effects) => effects,
            Err(_) => {
                throttled_deny(self, node_id, DenyReason::NoRoute);
                return;
            }
        };

        self.state.bandwidth -= c::NET_HACK_COST;
        self.state.net_hack_cd = c::NET_HACK_COOLDOWN;
        self.state.stats.hacker_actions += 1;
        self.events.push(Event::NetHackAction {
            node_id: node_id.clone(),
        });
        let events = apply_effects(&self.def, &mut self.state, self.cfg, &effects);
        self.events.extend(events);
    }

    /// Perform an uplink action (immediate; self-guards on paused/ended).
    ///
    /// An action is no longer a direct mutation of the floor: it resolves to a
    /// node and is a hack against it, which only lands if the hacker can actually
    /// reach that node from where they are playing. Cold from the van nothing on
    /// the floor answers, so the pivot into the building is the price of acting at
    /// all.
    pub(crate) fn perform_action(&mut self, kind: ActionKind) {
        if self.paused || self.state.ended {
            return;
        }
        let target = self.action_target(kind);
        self.perform_action_on(kind, target);
    }

    /// The economics and effect-application shared by every uplink action,
    /// against an explicit `target` node id rather than one `action_target`
    /// derives from the player's current position. `perform_action` (the
    /// keyboard path) derives `target` from position; the Net View path
    /// (Task 7) passes the exact node the player clicked, which may not be the
    /// contextually nearest one.
    fn perform_action_on(&mut self, kind: ActionKind, target: Option<String>) {
        let spec = match self.def.actions.get(&kind) {
            Some(s) => *s,
            None => return,
        };
        let t = self.state.t;
        let i = kind.index();

        // cooldown gate
        let cd = self.state.actions.get(&kind).map(|a| a.cd).unwrap_or(0.0);
        if cd > 0.0 {
            self.state.stats.failed_actions += 1;
            if let Some(slot) = self.state.throttles.cd.get_mut(i)
                && t - *slot >= c::THROTTLE_CDBW
            {
                *slot = t;
                self.events.push(Event::UplinkDenied {
                    kind,
                    reason: crate::scenario::common::DenyReason::Cooldown,
                });
            }
            return;
        }
        // bandwidth gate
        if self.state.bandwidth < spec.cost {
            self.state.stats.failed_actions += 1;
            if let Some(slot) = self.state.throttles.bw.get_mut(i)
                && t - *slot >= c::THROTTLE_CDBW
            {
                *slot = t;
                self.events.push(Event::UplinkDenied {
                    kind,
                    reason: crate::scenario::common::DenyReason::Bandwidth,
                });
            }
            return;
        }

        // The route is consulted last: after both gates, so their throttling is
        // untouched, and before any charge, so a route never had costs nothing.
        // A fixture the floor does not carry is a content gap, not a denial.
        let Some(target) = target else {
            return;
        };
        let effects = match self.state.agents.hacker.hack(&self.graph, &target) {
            Ok(effects) => effects,
            // `NoRoute` is the play state the hacker must answer by pivoting in.
            // Every other error means the graph carries no such node, which is a
            // content bug; it denies identically rather than panicking.
            Err(_) => {
                self.state.stats.failed_actions += 1;
                if let Some(slot) = self.state.throttles.route.get_mut(i)
                    && t - *slot >= c::THROTTLE_CDBW
                {
                    *slot = t;
                    self.events.push(Event::UplinkDenied {
                        kind,
                        reason: DenyReason::NoRoute,
                    });
                }
                return;
            }
        };

        self.state.bandwidth -= spec.cost;
        if let Some(a) = self.state.actions.get_mut(&kind) {
            a.cd = spec.cooldown;
        }
        self.state.stats.hacker_actions += 1;
        self.events.push(Event::UplinkAction { kind });

        let events = apply_effects(&self.def, &mut self.state, self.cfg, &effects);
        self.events.extend(events);

        // The vacuum's own fall is not a gate: the command above was paid for
        // in full. This is feedback on the attempt's effect, not a denial of
        // the attempt itself, so it is unthrottled, exactly as the original
        // short-circuiting denial was.
        if kind == ActionKind::Vacuum && self.state.vacuum.fallen {
            self.events.push(Event::UplinkDenied {
                kind,
                reason: DenyReason::VacuumFallen,
            });
        }

        // The escalating Lights extra reads `lights_uses` after the effect has
        // counted this use, exactly as the direct mutation it replaces did: the
        // old arm incremented the counter and only then sized the extra from it.
        let extra = if kind == ActionKind::Lights && self.state.lights_uses >= 3 {
            6.0 + f64::from(self.state.lights_uses) * 1.5
        } else {
            0.0
        };
        let ag = self.preset.alert_gain;
        self.state.alert = clamp(
            self.state.alert + (spec.alert_gain + extra) * ag,
            0.0,
            100.0,
        );
    }

    /// Whether Billy is still pressed against the closed door that last blocked
    /// him, wanting the far side. True on the non-colliding ticks of a door
    /// wait: he is within reach of the plane and his target lies beyond it.
    fn still_pressed_at_door(&self, target_x: f64) -> bool {
        let Some(di) = self.state.billy.blocked_door else {
            return false;
        };
        let Some(door) = self.state.doors.get(di) else {
            return false;
        };
        if door.open > 0.0 {
            return false;
        }
        let center = self.state.billy.x + self.def.billy.w / 2.0;
        let side = center - door.x;
        if side.abs() > c::BILLY_DOOR_REACH {
            return false;
        }
        // Still on one side wanting the other (a target on his own side means
        // he has changed his mind and the wait is abandoned).
        (target_x - door.x) * side < 0.0
    }

    /// Move Billy toward a target x at `speed`, handling door-blocking and the
    /// badge-through behaviour (emitting `BillyBadgedDoor` under an 8 s throttle).
    pub(crate) fn move_billy_toward(&mut self, target_x: f64, speed: f64) {
        let dt = TICK_DT;
        let old_x = self.state.billy.x;
        // Flooded rooms slow the guard exactly as they slow the player:
        // released water is a fact of the building, not of who walks in it.
        let speed = speed * self.wade_factor_at(old_x);
        let dx = target_x - old_x;
        let dir = if dx.abs() < 2.0 {
            0.0
        } else {
            crate::scenario::mathf::sign(dx)
        };
        if dir != 0.0 {
            self.state.billy.facing = dir;
        }
        self.state.billy.vx = crate::scenario::mathf::approach(
            self.state.billy.vx,
            dir * speed,
            self.actor.stats.accel * dt,
        );
        self.state.billy.x += self.state.billy.vx * dt;
        self.state.billy.x = clamp(self.state.billy.x, c::BILLY_CLAMP_LO, c::BILLY_CLAMP_HI);
        let before = self.state.billy.x;
        let (nx, _) = Self::constrain_by_doors(
            &self.state.doors,
            old_x,
            self.state.billy.x,
            self.def.billy.w,
        );
        self.state.billy.x = nx;
        let blocked = (before - self.state.billy.x).abs() > 0.1;
        if blocked {
            self.state.billy.vx = 0.0;
            let center = self.state.billy.x + self.def.billy.w / 2.0;
            let di = match Self::nearest_door(&self.state.doors, center) {
                Some(d) => d,
                None => return,
            };
            if self.state.billy.blocked_door != Some(di) {
                self.state.billy.blocked_door = Some(di);
                self.state.billy.door_wait = 0.0;
            }
        } else if !self.still_pressed_at_door(target_x) {
            self.state.billy.door_wait = 0.0;
            self.state.billy.blocked_door = None;
            return;
        }

        //## The wait survives the bounce
        // `constrain_by_doors` puts a blocked Billy back 3px clear of the door
        // plane, so he spends most ticks of a door wait *not* colliding —
        // re-accelerating from a standstill into the door he just hit. Resetting
        // the timer on those ticks capped it at one tick per bounce cycle, so
        // `badge_delay` was unreachable at any speed and a closed door was a
        // permanent wall rather than a delay. The wait now accrues for as long
        // as he is pressed against that door and still wants the far side.
        let Some(di) = self.state.billy.blocked_door else {
            return;
        };
        self.state.billy.door_wait += dt;
        if self.state.billy.door_wait >= self.preset.badge_delay {
            self.state.billy.door_wait = 0.0;
            let t = self.state.t;
            if let Some(d) = self.state.doors.get_mut(di) {
                d.open = d.open.max(c::BADGE_OPEN);
                if !d.badge_logged {
                    d.badge_logged = true;
                    let door_id = d.id.clone();
                    if let Some(slot) = self.state.throttles.badge.get_mut(di)
                        && t - *slot >= c::THROTTLE_BADGE
                    {
                        *slot = t;
                        self.events.push(Event::BillyBadgedDoor { door: door_id });
                    }
                }
            }
        }
    }

    // --- endings + scoring -------------------------------------------------

    /// Whether Billy ended believing the USB mattered (has it, reported it, or
    /// interest crossed the misdirect threshold).
    fn misled(&self) -> bool {
        self.state.billy.has_usb
            || self.state.billy.reported_target == Some(ReportedTarget::Usb)
            || self.state.billy.usb_interest >= c::MISDIRECT_THRESHOLD
    }

    /// Extract via `method`, building the success debrief and ending the run.
    pub(crate) fn extract(&mut self, method: ExtractMethod) {
        if self.state.ended {
            return;
        }
        self.state.chute.used = method == ExtractMethod::LaundryChute;
        self.state.stats.extraction = Some(method);
        let misled = self.misled();
        let has_note = self.state.player.has_note;
        let mut tags: Vec<Tag> = Vec::new();
        let mut bd = ScoreBreakdown::default();
        let sc = self.def.scoring;
        let mut score = sc.base;
        bd.base = sc.base;

        if has_note {
            tags.push(Tag::new(
                "Strategic success: contact lead secured",
                Tone::Good,
            ));
            score += sc.note;
            bd.note = sc.note;
        } else if self.state.note.billy_has {
            tags.push(Tag::new("Compromised: Billy has the real lead", Tone::Bad));
            score += sc.note_lost;
            bd.note = sc.note_lost;
        } else {
            tags.push(Tag::new("Survival only: real lead not secured", Tone::Warn));
            score += sc.note_none;
            bd.note = sc.note_none;
        }

        if misled {
            tags.push(Tag::new(
                "Decoy success: Billy thinks the USB mattered",
                Tone::Good,
            ));
            score += sc.misdir;
            bd.misdirect = sc.misdir;
        } else if self.state.billy.reported_target == Some(ReportedTarget::Note) {
            tags.push(Tag::new(
                "Behavioural leak: Billy identified the note",
                Tone::Bad,
            ));
            score += sc.misdir_leak;
            bd.misdirect = sc.misdir_leak;
        } else {
            tags.push(Tag::new("Billy remained uncertain", Tone::Warn));
        }

        if self.state.billy.called {
            tags.push(Tag::new("Boss heard Billy's report", Tone::Warn));
            score += sc.boss;
            bd.boss = sc.boss;
        } else {
            tags.push(Tag::new("No report completed", Tone::Good));
            score += sc.noboss;
            bd.boss = sc.noboss;
        }

        let cams = self.state.stats.camera_detections;
        if cams == 0 {
            tags.push(Tag::new("No confirmed camera motion flags", Tone::Good));
            score += sc.nocam;
            bd.camera = sc.nocam;
        } else {
            let pen = sc.cam_each * f64::from(cams);
            let plural = if cams == 1 { "" } else { "s" };
            tags.push(Tag {
                text: format!("{cams} camera motion flag{plural}"),
                tone: Tone::Warn,
            });
            score += pen;
            bd.camera = pen;
        }

        if self.state.stats.max_isolation > self.preset.support_limit * sc.iso_snap_frac {
            tags.push(Tag::new("Support envelope nearly snapped", Tone::Warn));
            score += sc.iso_snap;
            bd.isolation = sc.iso_snap;
        } else {
            tags.push(Tag::new("Support relationship preserved", Tone::Good));
            score += sc.iso_ok;
            bd.isolation = sc.iso_ok;
        }

        if method == ExtractMethod::LaundryChute {
            tags.push(Tag::new(
                "Humiliating extraction route validated",
                Tone::Good,
            ));
            score += sc.chute;
            bd.chute = sc.chute;
        } else {
            tags.push(Tag::new("Service exit extraction", Tone::Good));
        }
        if self.state.stats.usb_trace {
            tags.push(Tag::new("USB trace handshake completed", Tone::Bad));
            score += sc.usbtrace;
            bd.usb_trace = sc.usbtrace;
        }
        if self.state.stats.rescue_used {
            tags.push(Tag::new("Automatic rescue consumed", Tone::Warn));
            score += sc.rescue;
            bd.rescue = sc.rescue;
        }
        tags.push(Tag::new("Both agents remain playable", Tone::Good));

        let time_bonus = (sc.time_base - self.state.t * sc.time_k).max(0.0);
        score += time_bonus;
        bd.time_bonus = time_bonus;
        let alert_pen = -self.state.max_alert * sc.alert_k;
        score += alert_pen;
        bd.alert_penalty = alert_pen;
        let fa_pen = -f64::from(self.state.stats.failed_actions) * sc.fail_act;
        score += fa_pen;
        bd.failed_actions_penalty = fa_pen;

        bd.raw = score;
        bd.score_mult = self.preset.score_mult;
        let final_f = js_round(score * self.preset.score_mult).max(0.0);
        let final_u = final_f as u32;
        bd.final_score = final_u;

        let grade = grade_for(final_u, true, &sc.grades);
        let title = if has_note {
            "Extraction reached"
        } else {
            "Extraction without the answer"
        };
        let summary = self.build_summary(method, misled);
        // HTML debrief() keys the strong paragraph on `hasUSB || reportedTarget=="usb"`
        // only — NOT the full misdirect predicate (which also counts usb_interest>=72).
        let debrief_misled = self.state.billy.has_usb
            || self.state.billy.reported_target == Some(ReportedTarget::Usb);
        let text = debrief_text(Outcome::Extracted, has_note, debrief_misled);
        let debrief = Debrief {
            format: DEBRIEF_FORMAT.to_owned(),
            success: true,
            reason: Outcome::Extracted,
            title: title.to_owned(),
            summary,
            grade,
            score: final_u,
            tags,
            breakdown: bd,
            debrief_text: text,
            stats: self.state.stats.clone(),
            time_s: self.state.t,
            max_alert: self.state.max_alert,
        };
        self.events.push(Event::Extracted { method });
        self.finish(debrief, Outcome::Extracted);
    }

    fn build_summary(&self, method: ExtractMethod, misled: bool) -> String {
        let method_s = match method {
            ExtractMethod::ServiceExit => "service exit",
            ExtractMethod::LaundryChute => "laundry chute",
        };
        let lead = if self.state.player.has_note {
            "The contact lead is secured."
        } else if self.state.note.billy_has {
            "Billy retains the real lead."
        } else {
            "The real lead remains unresolved."
        };
        let theory = if misled {
            "Billy's story centres on the USB."
        } else if self.state.billy.reported_target == Some(ReportedTarget::Note) {
            "Billy learned what truly mattered."
        } else {
            "Billy never formed a stable object theory."
        };
        format!("Both agents remain playable after a {method_s} extraction. {lead} {theory}")
    }

    /// Fail the run for `reason`, building the failure debrief and ending it.
    pub(crate) fn fail_mission(&mut self, reason: FailReason) {
        if self.state.ended {
            return;
        }
        let has_note = self.state.player.has_note;
        let usb_believed =
            self.state.billy.has_usb || self.state.billy.usb_interest >= c::MISDIRECT_THRESHOLD;
        let sc = self.def.scoring;
        let mut bd = ScoreBreakdown::default();
        let note_term = if has_note { sc.fail_note } else { sc.fail_base };
        // HTML fail score adds the USB term only on `usbInterest >= 72` — Billy
        // physically holding the drive does NOT contribute to the failure score
        // (it only earns the "believed the USB mattered" tag below).
        let usb_term = if self.state.billy.usb_interest >= c::MISDIRECT_THRESHOLD {
            sc.fail_usb
        } else {
            0.0
        };
        let alert_term = -self.state.alert * sc.fail_alert_k;
        bd.note = note_term;
        bd.misdirect = usb_term;
        bd.alert_penalty = alert_term;
        let raw = note_term + usb_term + alert_term;
        bd.raw = raw;
        bd.score_mult = 1.0;
        let final_u = js_round(raw).max(0.0) as u32;
        bd.final_score = final_u;

        let (outcome, title, first_tag) = match reason {
            FailReason::Caught => (
                Outcome::Caught,
                "Caught by Billy",
                Tag::new("Isolation failure", Tone::Bad),
            ),
            FailReason::Partition => (
                Outcome::Partition,
                "Security partition trap",
                Tag::new("Team-support failure", Tone::Bad),
            ),
            FailReason::Lockdown => (
                Outcome::Lockdown,
                "Building-wide lockdown",
                Tag::new("Infrastructure fully awake", Tone::Bad),
            ),
            FailReason::Traced => (
                Outcome::Traced,
                "Traced to the source",
                Tag::new("The trace ran to completion", Tone::Bad),
            ),
        };
        let mut tags = vec![first_tag];
        if has_note {
            tags.push(Tag::new(
                "Contact lead was secured before failure",
                Tone::Warn,
            ));
        }
        if usb_believed {
            tags.push(Tag::new("Billy believed the USB mattered", Tone::Good));
        }
        if self.state.billy.called {
            tags.push(Tag::new("Boss heard Billy's report", Tone::Warn));
        }
        tags.push(Tag::new("Field agent no longer operational", Tone::Bad));

        let summary = self.fail_summary(reason);
        let grade = grade_for(final_u, false, &sc.grades);
        let text = debrief_text(outcome, has_note, self.misled());
        let debrief = Debrief {
            format: DEBRIEF_FORMAT.to_owned(),
            success: false,
            reason: outcome,
            title: title.to_owned(),
            summary,
            grade,
            score: final_u,
            tags,
            breakdown: bd,
            debrief_text: text,
            stats: self.state.stats.clone(),
            time_s: self.state.t,
            max_alert: self.state.max_alert,
        };
        self.events.push(Event::MissionFailed { reason });
        self.finish(debrief, outcome);
    }

    fn fail_summary(&self, reason: FailReason) -> String {
        match reason {
            FailReason::Caught => "Billy catches the field agent outside a viable support response. Solo brilliance was not enough.".to_owned(),
            FailReason::Partition => "A security partition slams down. You are trapped. This is a team-support failure, not a solo death.".to_owned(),
            FailReason::Lockdown => "The building reaches a complete administrative opinion. Every partition closes at once.".to_owned(),
            FailReason::Traced => "The trace fills and the connection is followed back to the place it was opened from. Nobody had to be caught on the floor; an address was enough.".to_owned(),
        }
    }

    fn finish(&mut self, debrief: Debrief, outcome: Outcome) {
        let from = self.state.phase;
        self.state.ended = true;
        self.state.phase = Phase::Result;
        self.state.result = Some(debrief);
        self.events.push(Event::PhaseChanged {
            from,
            to: Phase::Result,
            reason: None,
        });
        self.events.push(Event::RunEnded { outcome });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::ghost_lobby::ghost_lobby;

    /// A standard run with the hacker already pivoted onto the floor, so that an
    /// uplink resolves rather than being denied on route. The pivot goes through
    /// the canonical immediate — the same guard and events a played
    /// `Command::Pivot` takes — so these fixtures never diverge from a replay.
    fn pivoted() -> GhostLobbySim {
        let mut sim = GhostLobbySim::new(ghost_lobby(), RunConfig::standard(), 123456)
            .expect("the definition is valid");
        let _ = sim.drain_events();
        sim.pivot(PivotTarget::Bridge);
        assert!(
            sim.drain_events()
                .iter()
                .any(|e| matches!(e, Event::PivotOpened { .. })),
            "the van can reach the maintenance bridge"
        );
        sim
    }

    #[test]
    fn action_kind_of_node_classifies_by_identity_not_position() {
        // Unlike `action_target`, which names only the contextually nearest
        // fixture, this must recognise every camera/door/light/vacuum node
        // regardless of where the player currently stands.
        let sim = GhostLobbySim::new(ghost_lobby(), RunConfig::standard(), 123456)
            .expect("the definition is valid");
        assert_eq!(
            sim.action_kind_of_node(crate::scenario::floor_graph::VACUUM_NODE_ID),
            Some(ActionKind::Vacuum)
        );
        assert_eq!(
            sim.action_kind_of_node(&crate::scenario::floor_graph::camera_node_id(0)),
            Some(ActionKind::Camera)
        );
        let door_id = sim.def.doors[0].id.clone();
        assert_eq!(
            sim.action_kind_of_node(&crate::scenario::floor_graph::door_node_id(door_id)),
            Some(ActionKind::Door)
        );
        let room_id = sim.def.rooms[0].id.clone();
        assert_eq!(
            sim.action_kind_of_node(&crate::scenario::floor_graph::light_node_id(
                room_id.as_str()
            )),
            Some(ActionKind::Lights)
        );
        assert_eq!(sim.action_kind_of_node("substation"), None);
        assert_eq!(sim.action_kind_of_node("no-such-node"), None);
    }

    #[test]
    fn net_ssh_opens_a_reachable_pivotable_node_by_index() {
        let mut sim = GhostLobbySim::new(ghost_lobby(), RunConfig::standard(), 123456)
            .expect("the definition is valid");
        let _ = sim.drain_events();
        let bridge_index = sim
            .graph
            .nodes
            .iter()
            .position(|n| n.id == "bms-bridge")
            .expect("the bridge is on the floor graph");
        sim.net_ssh(NetNodeIndex(bridge_index as u32));
        assert!(
            sim.drain_events()
                .iter()
                .any(|e| matches!(e, Event::PivotOpened { .. })),
            "the van can reach the maintenance bridge by index exactly as by name"
        );
        assert_eq!(sim.state.agents.hacker.hops(), 1);
    }

    #[test]
    fn net_ssh_denies_an_unreachable_node_and_reports_why() {
        let mut sim = GhostLobbySim::new(ghost_lobby(), RunConfig::standard(), 123456)
            .expect("the definition is valid");
        let _ = sim.drain_events();
        let substation_index = sim
            .graph
            .nodes
            .iter()
            .position(|n| n.id == "substation")
            .expect("the substation is on the floor graph");
        sim.net_ssh(NetNodeIndex(substation_index as u32));
        let events = sim.drain_events();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::PivotDenied { .. })),
            "cold from the van the substation is not yet reachable: {events:?}"
        );
        assert_eq!(
            sim.state.agents.hacker.hops(),
            0,
            "a refused pivot buys no depth"
        );
    }

    #[test]
    fn net_ssh_out_of_range_index_is_a_no_op() {
        let mut sim = GhostLobbySim::new(ghost_lobby(), RunConfig::standard(), 123456)
            .expect("the definition is valid");
        let _ = sim.drain_events();
        let before = sim.state.clone();
        sim.net_ssh(NetNodeIndex(u32::MAX));
        assert!(sim.drain_events().is_empty());
        assert_eq!(sim.state, before);
    }

    #[test]
    fn net_hack_on_a_camera_node_delegates_to_the_existing_uplink_economy() {
        // Clicking the same node id a keyboard `1` would target must produce
        // the identical event and charge the identical cost.
        let mut sim = pivoted();
        sim.net_ssh(NetNodeIndex(
            sim.graph
                .nodes
                .iter()
                .position(|n| n.id == "isp-ops")
                .expect("ops host exists") as u32,
        ));
        let _ = sim.drain_events();
        let camera_index = sim
            .graph
            .nodes
            .iter()
            .position(|n| n.id == crate::scenario::floor_graph::camera_node_id(0))
            .expect("camera-0 is on the floor graph");
        let bandwidth_before = sim.state.bandwidth;
        sim.net_hack(NetNodeIndex(camera_index as u32));
        let events = sim.drain_events();
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::UplinkAction {
                    kind: ActionKind::Camera
                }
            )),
            "a camera-node click must read as the Camera uplink: {events:?}"
        );
        assert!(
            sim.state.bandwidth < bandwidth_before,
            "the uplink cost was charged"
        );
    }

    #[test]
    fn net_hack_on_the_substation_is_a_new_direct_capability() {
        // The substation sits two pivots deep from the van: out to the ISP ops
        // host, then on through the grid jump host. The maintenance bridge opens
        // the floor fixtures but is a dead end for the upstream power line (the
        // bms segment cannot reach the isp segment), so the path to the
        // substation runs straight from the van, not through the bridge.
        let mut sim = GhostLobbySim::new(ghost_lobby(), RunConfig::standard(), 123456)
            .expect("the definition is valid");
        let _ = sim.drain_events();
        let ops = sim
            .graph
            .nodes
            .iter()
            .position(|n| n.id == "isp-ops")
            .expect("ops host exists");
        sim.net_ssh(NetNodeIndex(ops as u32));
        assert_eq!(
            sim.state.agents.hacker.hops(),
            1,
            "the van reaches the ISP ops host"
        );
        let jump = sim
            .graph
            .nodes
            .iter()
            .position(|n| n.id == "grid-jump")
            .expect("grid jump host exists");
        sim.net_ssh(NetNodeIndex(jump as u32));
        assert_eq!(
            sim.state.agents.hacker.hops(),
            2,
            "the ISP reaches the grid jump host"
        );
        let _ = sim.drain_events();

        let substation = sim
            .graph
            .nodes
            .iter()
            .position(|n| n.id == "substation")
            .expect("substation exists");
        let bandwidth_before = sim.state.bandwidth;
        sim.net_hack(NetNodeIndex(substation as u32));
        let events = sim.drain_events();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::NetHackAction { node_id } if node_id == "substation")),
            "the substation must report through NetHackAction: {events:?}"
        );
        assert!(sim.state.dead_nodes.contains("substation"), "the cut lands");
        assert_eq!(
            sim.state.bandwidth,
            bandwidth_before - crate::scenario::constants::NET_HACK_COST
        );
        assert_eq!(
            sim.state.net_hack_cd,
            crate::scenario::constants::NET_HACK_COOLDOWN
        );

        // A second click while the cooldown is live must be denied, not free.
        sim.net_hack(NetNodeIndex(substation as u32));
        let events = sim.drain_events();
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::NetHackDenied {
                    reason: DenyReason::Cooldown,
                    ..
                }
            )),
            "a second click under cooldown must be denied: {events:?}"
        );
    }

    #[test]
    fn net_hack_on_an_unreachable_node_denies_with_no_route() {
        // Cold from the van: the substation is not reachable without pivoting
        // through the ISP first.
        let mut sim = GhostLobbySim::new(ghost_lobby(), RunConfig::standard(), 123456)
            .expect("the definition is valid");
        let _ = sim.drain_events();
        let substation = sim
            .graph
            .nodes
            .iter()
            .position(|n| n.id == "substation")
            .expect("substation exists");
        sim.net_hack(NetNodeIndex(substation as u32));
        let events = sim.drain_events();
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::NetHackDenied {
                    reason: DenyReason::NoRoute,
                    ..
                }
            )),
            "{events:?}"
        );
        assert!(!sim.state.dead_nodes.contains("substation"));
    }

    #[test]
    fn the_net_view_commands_reach_the_sim_through_a_tick() {
        // Unlike the direct `sim.net_ssh(...)` calls above (which exercise the
        // method in isolation, mirroring how `pivoted()` already calls
        // `sim.pivot(...)` directly), this drives the exact path a real Net
        // View click takes: through `Command` and `tick`.
        let mut sim = GhostLobbySim::new(ghost_lobby(), RunConfig::standard(), 123456)
            .expect("the definition is valid");
        let _ = sim.drain_events();
        let bridge_index = sim
            .graph
            .nodes
            .iter()
            .position(|n| n.id == "bms-bridge")
            .expect("the bridge is on the floor graph") as u32;
        let input = TickInput {
            immediates: vec![crate::scenario::command::Command::NetSsh {
                node: NetNodeIndex(bridge_index),
            }],
            ..Default::default()
        };
        let events = sim.tick(&input);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::PivotOpened { .. })),
            "{events:?}"
        );
    }

    #[test]
    fn the_lights_escalation_sizes_itself_on_the_use_it_is_charging_for() {
        // The escalation reads `lights_uses` AFTER the effect counts this use, as
        // the direct mutation this replaces did: it incremented, then sized the
        // extra from the incremented count. Third use => extra = 6.0 + 3 * 1.5.
        // Reading the pre-increment count would size it 6.0 + 2 * 1.5 instead, so
        // this pins the ordering, not merely the presence, of the escalation.
        let mut sim = pivoted();
        sim.state.lights_uses = 2;
        sim.state.alert = 0.0;
        let ag = sim.preset.alert_gain;
        let gain = sim
            .def
            .actions
            .get(&ActionKind::Lights)
            .expect("the definition tunes the lights")
            .alert_gain;

        // Called directly, so no system runs to decay the alert underneath us.
        sim.perform_action(ActionKind::Lights);

        assert_eq!(sim.state.lights_uses, 3, "the use is counted");
        assert_eq!(sim.state.alert, (gain + 6.0 + 3.0 * 1.5) * ag);
    }

    #[test]
    fn a_route_denial_spends_neither_bandwidth_nor_cooldown() {
        // Cold from the van: the same action, without the pivot.
        let mut sim = GhostLobbySim::new(ghost_lobby(), RunConfig::standard(), 123456)
            .expect("the definition is valid");
        let _ = sim.drain_events();
        let bandwidth = sim.state.bandwidth;
        sim.perform_action(ActionKind::Lights);

        assert_eq!(sim.state.bandwidth, bandwidth, "no route, no charge");
        assert_eq!(
            sim.state.actions.get(&ActionKind::Lights).map(|a| a.cd),
            Some(0.0),
            "no route, no cooldown"
        );
        assert_eq!(sim.state.stats.hacker_actions, 0);
        assert_eq!(sim.state.stats.failed_actions, 1);
        assert_eq!(sim.state.alert, 0.0, "a wall raises no alert");
    }

    #[test]
    fn each_action_resolves_to_a_node_the_floor_actually_carries() {
        // `action_target` naming a node the graph lacks would deny every action of
        // that kind for ever, silently. Every kind must resolve against the graph.
        let sim = pivoted();
        for kind in [
            ActionKind::Camera,
            ActionKind::Door,
            ActionKind::Vacuum,
            ActionKind::Lights,
        ] {
            let target = sim
                .action_target(kind)
                .unwrap_or_else(|| panic!("{kind:?} resolves to a target"));
            assert!(
                sim.graph.node(&target).is_some(),
                "{kind:?} resolved to {target}, which the floor graph does not carry"
            );
        }
    }

    #[test]
    fn a_missing_fixture_does_not_swallow_the_cooldown_gate() {
        // Regression: the cooldown and bandwidth gates must run, and may emit
        // their throttled denial, REGARDLESS of whether `action_target` finds a
        // fixture. A content gap (here, a floor authored with no cameras, so
        // `action_target(Camera)` is None) coinciding with that same kind being
        // on cooldown must still deny on cooldown and count a failed action;
        // the null-check belongs after the gates, never before them.
        let mut sim = GhostLobbySim::new(ghost_lobby(), RunConfig::standard(), 123456)
            .expect("the definition is valid");
        sim.def.cameras.clear();
        assert_eq!(
            sim.action_target(ActionKind::Camera),
            None,
            "with no cameras authored, the camera uplink resolves to no node"
        );
        let _ = sim.drain_events();
        sim.state
            .actions
            .get_mut(&ActionKind::Camera)
            .expect("the definition tunes the camera")
            .cd = 5.0;

        sim.perform_action(ActionKind::Camera);

        assert_eq!(
            sim.state.stats.failed_actions, 1,
            "the cooldown gate ran and counted the failure despite the missing fixture"
        );
        assert!(
            sim.events.iter().any(|e| matches!(
                e,
                Event::UplinkDenied {
                    kind: ActionKind::Camera,
                    reason: DenyReason::Cooldown,
                }
            )),
            "the cooldown denial still fires though the fixture is absent"
        );
    }

    #[test]
    fn driving_a_fallen_vacuum_still_charges_but_denies_the_effect() {
        // The retune moved the fallen check into `apply_effects`, so the player
        // pays for the attempt in full; only its effect on the world is refused.
        let mut sim = pivoted();
        sim.state.vacuum.fallen = true;
        let bandwidth = sim.state.bandwidth;

        sim.perform_action(ActionKind::Vacuum);

        assert!(
            sim.state.bandwidth < bandwidth,
            "the command is paid for even though the vacuum cannot move"
        );
        assert_eq!(
            sim.state.stats.hacker_actions, 1,
            "the attempt lands and is counted"
        );
        assert_eq!(
            sim.state.stats.failed_actions, 0,
            "the world refusing the effect is not a failed action"
        );
        assert!(
            sim.events.iter().any(|e| matches!(
                e,
                Event::UplinkDenied {
                    kind: ActionKind::Vacuum,
                    reason: DenyReason::VacuumFallen,
                }
            )),
            "the player learns the robot did not move"
        );
    }

    #[test]
    fn a_cooldown_denial_does_not_swallow_a_later_route_denial() {
        // Regression: `route` used to share `cd`'s throttle slot, so a
        // cooldown denial (which stamps the slot) could silently swallow a
        // route denial for the same action moments later. They are different
        // news to the player and must not compete for one slot.
        let mut sim = GhostLobbySim::new(ghost_lobby(), RunConfig::standard(), 123456)
            .expect("the definition is valid");
        let _ = sim.drain_events();

        // First call: force the action onto cooldown so it denies on Cooldown
        // and stamps `throttles.cd`.
        sim.state
            .actions
            .get_mut(&ActionKind::Lights)
            .expect("the definition tunes the lights")
            .cd = 5.0;
        sim.perform_action(ActionKind::Lights);
        assert!(
            sim.events.iter().any(|e| matches!(
                e,
                Event::UplinkDenied {
                    kind: ActionKind::Lights,
                    reason: DenyReason::Cooldown,
                }
            )),
            "the first call denies on cooldown: {:?}",
            sim.events
        );

        // Second call, same tick: the cooldown clears, but cold from the van
        // the room's lights are not yet reachable, so this must deny on
        // NoRoute -- and it must still fire, not be swallowed by `cd`'s
        // just-stamped slot.
        sim.state
            .actions
            .get_mut(&ActionKind::Lights)
            .expect("the definition tunes the lights")
            .cd = 0.0;
        sim.perform_action(ActionKind::Lights);
        assert!(
            sim.events.iter().any(|e| matches!(
                e,
                Event::UplinkDenied {
                    kind: ActionKind::Lights,
                    reason: DenyReason::NoRoute,
                }
            )),
            "the second call must still report NoRoute, not be swallowed by \
             the cooldown denial's slot: {:?}",
            sim.events
        );
    }

    #[test]
    fn repeated_net_hack_denials_collapse_to_one_event() {
        // net_hack's denials are throttled exactly like perform_action_on's:
        // a click that keeps landing on a cooling-down or unreachable target
        // must not flood the log the way a held key on the keyboard path
        // never could.
        let mut sim = GhostLobbySim::new(ghost_lobby(), RunConfig::standard(), 123456)
            .expect("the definition is valid");
        let _ = sim.drain_events();
        let substation = sim
            .graph
            .nodes
            .iter()
            .position(|n| n.id == "substation")
            .expect("substation exists") as u32;

        // Cold from the van: every one of these denies on NoRoute, same tick.
        sim.net_hack(NetNodeIndex(substation));
        sim.net_hack(NetNodeIndex(substation));
        sim.net_hack(NetNodeIndex(substation));

        let denials = sim
            .events
            .iter()
            .filter(|e| matches!(e, Event::NetHackDenied { .. }))
            .count();
        assert_eq!(
            denials, 1,
            "three denials inside one throttle window must collapse to one event: {:?}",
            sim.events
        );
        assert_eq!(
            sim.state.stats.failed_actions, 3,
            "every attempt still counts as a failure, even when its event is throttled"
        );
    }
}

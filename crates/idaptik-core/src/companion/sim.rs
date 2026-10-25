//! The deterministic Moletaire simulation: [`MoletaireSim`].
//!
//! A dt-stepped port of the archive's `Moletaire.res` entity update. Commands
//! go in (typed [`MoleCommand`]s or the equivalent direct methods), events
//! come out (typed [`MoleEvent`]s), and every run is reproducible from
//! `(definition, params, seed, command script)` — the two `Math.random()`
//! sites in the archive (the delivery eat roll and the starving wander
//! direction) draw from one seeded [`Mulberry32`] stream instead.
//!
//! # RNG stream
//!
//! One `Mulberry32` seeded at construction. Draw order is load-bearing:
//! * one draw per delivered item (the eat roll — drawn *before* the eat
//!   chance is compared, exactly as the archive draws `Math.random()` first);
//! * one draw per tick in which the mole is starving with no edible within
//!   `wander_min_distance` (the wander direction).
//!
//! # Per-tick integration order (archive `update`)
//!
//! 1. state-machine / movement update (may emit one event);
//! 2. dodge-timer countdown;
//! 3. periodic 0.5 s underground coprocessor scans (signal scan while
//!    tunnelling/digging, then vibration read);
//! 4. hunger integration + gravity pull + resistance episodes + starving
//!    wander (may emit resistance transition events);
//! 5. rendering is **not** here — [`MoletaireSim::view`] is the pure
//!    view-state projection frontends draw from.
//!
//! # Archive quirks preserved deliberately
//!
//! * `MoleDied` carries [`MoleState::Dead`], not the fatal state: the archive
//!   reassigns `mole.state = Dead` *before* constructing the event.
//! * [`MoletaireSim::crush`] / [`MoletaireSim::catch_by_dog`] clear `alive`,
//!   and the state-machine's `Crushed | CaughtByDog → MoleDied` arm only runs
//!   while `alive` — so those two commands never themselves produce a
//!   `MoleDied` event, exactly as in the archive (its game loop set the state
//!   directly when it wanted the event).
//! * The hunger-resistance episode timer replicates the archive's exact
//!   branch shape (including `is_resisting` flapping back to `false` while
//!   `fight_timer <= interval`).

use crate::companion::coprocessors::{CoprocessorBay, CoprocessorType, VibrationReading};
use crate::companion::definition::{
    CompanionDefinition, CompanionValidationError, MOLETAIRE_SNAPSHOT_FORMAT,
};
use crate::companion::equipment::Equipment;
use crate::companion::hunger::{self, EdibleObject};
use crate::scenario::rng::Mulberry32;
use serde::{Deserialize, Serialize};

/// The 11-state mole state machine (archive `moleState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MoleState {
    /// Stationary, awaiting orders.
    Idle,
    /// Fast tunnelling (~200 px/s).
    MovingUnderground,
    /// Very slow surface movement (~25 px/s).
    MovingAboveGround,
    /// 4 s dig, then the guard falls (dodge or be crushed).
    DiggingTrap,
    /// 3 s chew through a cable.
    SabotagingCable,
    /// Holding items — eat roll on delivery.
    CarryingItem,
    /// Drawn to loose wires, 5 s duration.
    Distracted,
    /// Uncontrollable flight, height → distance.
    Gliding,
    /// Permadeath: guard fell into the trap and the mole didn't dodge.
    Crushed,
    /// Permadeath: RoboDog dug down and caught the mole.
    CaughtByDog,
    /// Terminal state (permadeath finalised).
    Dead,
}

impl MoleState {
    /// The ordinal the archive's render kernels used (`stateOrdinal`, 0..=10).
    pub fn ordinal(self) -> u8 {
        match self {
            MoleState::Idle => 0,
            MoleState::MovingUnderground => 1,
            MoleState::MovingAboveGround => 2,
            MoleState::DiggingTrap => 3,
            MoleState::SabotagingCable => 4,
            MoleState::CarryingItem => 5,
            MoleState::Distracted => 6,
            MoleState::Gliding => 7,
            MoleState::Crushed => 8,
            MoleState::CaughtByDog => 9,
            MoleState::Dead => 10,
        }
    }

    /// HUD label (archive `stateToString`).
    pub fn label(self) -> &'static str {
        match self {
            MoleState::Idle => "IDLE",
            MoleState::MovingUnderground => "TUNNELLING",
            MoleState::MovingAboveGround => "SURFACE",
            MoleState::DiggingTrap => "DIGGING TRAP",
            MoleState::SabotagingCable => "SABOTAGING",
            MoleState::CarryingItem => "CARRYING",
            MoleState::Distracted => "DISTRACTED",
            MoleState::Gliding => "GLIDING",
            MoleState::Crushed => "CRUSHED",
            MoleState::CaughtByDog => "CAUGHT",
            MoleState::Dead => "DEAD",
        }
    }

    /// Body colour for this state — the archive render palette.
    pub fn body_color(self) -> u32 {
        match self {
            MoleState::Idle | MoleState::CarryingItem => 0x006b_4226, // Warm brown
            MoleState::MovingUnderground => 0x005a_3518,              // Darker (underground)
            MoleState::MovingAboveGround => 0x007a_5230,              // Lighter (surface)
            MoleState::DiggingTrap => 0x008b_6914,                    // Yellowish (digging)
            MoleState::SabotagingCable => 0x0088_4422,                // Reddish-brown
            MoleState::Distracted => 0x00aa_7744,                     // Lighter (confused)
            MoleState::Gliding => 0x0066_88aa,                        // Blue tint (sky)
            MoleState::Crushed | MoleState::CaughtByDog | MoleState::Dead => 0x0033_3333,
        }
    }
}

/// Horizontal facing (archive `facing`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Facing {
    Left,
    Right,
}

impl Facing {
    /// The ordinal the archive's render kernels used (Left 0, Right 1).
    pub fn ordinal(self) -> u8 {
        match self {
            Facing::Left => 0,
            Facing::Right => 1,
        }
    }
}

/// Inbound commands — the typed boundary a game loop drives the mole with.
/// Each maps 1:1 to an archive command function (see [`MoletaireSim::apply`];
/// the direct methods additionally expose the archive's return values).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MoleCommand {
    /// Order the mole to a position, tunnelling or on the surface.
    MoveTo { target_x: f64, underground: bool },
    /// Dig a trap at the current position (requires depth > `dig_min_depth`).
    DigTrap,
    /// Chew through a cable at the current position (same depth gate).
    SabotageCable,
    /// Hand the mole an item to carry (respects carry capacity).
    GiveItem { item_id: String },
    /// Drop a specific carried item.
    DropItem { item_id: String },
    /// Deliver carried items: move to `delivery_x`, drop on arrival.
    Deliver { delivery_x: f64 },
    /// Equip a single item (replaces any current equipment).
    Equip { item: Equipment },
    /// Remove all equipment.
    Unequip,
    /// Fire the flash (requires the FlashCamera).
    UseFlash,
    /// Launch the glider from a height (requires the Glider, on the surface).
    LaunchGlider { launch_height: f64 },
    /// Play a synthesised diversion sound by index.
    PlaySynthSound { sound_index: usize },
    /// A loose wire pulls the mole (involuntary, within 200 px).
    DistractByWire { wire_x: f64 },
    /// Guard fell into the trap and the mole didn't dodge. Permadeath.
    Crush,
    /// RoboDog dug down and caught the mole. Permadeath.
    CatchByDog,
    /// Feed the mole — resets hunger to 0.
    Feed,
    /// Point the mole's appetite at a component.
    SetHungerTarget { target_x: f64 },
    /// Upgrade one coprocessor slot to the next level.
    UpgradeCoprocessor { ctype: CoprocessorType },
}

/// Outbound events (archive `moleEvent`) — game-level consequences for the
/// caller to act on.
///
/// # Integration seam (hosted by a floor that declares a mole)
///
/// A scenario floor folds these into its own world through the host in
/// `crate::scenario::sim` (ADR-0014); this module never touches scenario
/// state itself. Per the archive game loop:
/// * [`MoleEvent::SynthSoundPlayed`] distracts security dogs within **250 px**
///   of the mole's position (the sound spawns at the mole);
/// * [`MoleEvent::VibrationDetected`] feeds the hacker's (Q's) view — the
///   overseer polls the reading, no world mutation;
/// * [`MoleEvent::UndergroundScanComplete`] drives the mole's own scan
///   overlay only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum MoleEvent {
    /// Trap dig finished at (x, y) — dodge window active.
    TrapTriggered { x: f64, y: f64 },
    /// Cable destroyed.
    CableSabotaged { cable_id: String },
    /// Item delivered to the destination.
    ItemDelivered { item_id: String },
    /// The mole ate the item (eat roll).
    ItemEaten { item_id: String },
    /// Flash stun triggered.
    FlashFired,
    /// Permadeath. Carries [`MoleState::Dead`] (archive quirk — see module docs).
    MoleDied { state: MoleState },
    /// The mole was pulled toward loose wires.
    DistractionStarted,
    /// Glide landed at this x.
    GlideComplete { x: f64 },
    /// A MoveTo order completed.
    ReachedDestination,
    /// The mole was fed.
    FoodEaten,
    /// Item picked up.
    ItemPickedUp { item_id: String },
    /// Item dropped.
    ItemDropped { item_id: String },
    /// Glider launched at this height.
    GlideStarted { height: f64 },
    /// The mole began resisting control due to hunger.
    HungerResistanceStarted,
    /// The mole stopped resisting control.
    HungerResistanceEnded,
    /// The mole entered a building.
    EnteredBuilding,
    /// The mole climbed to this floor.
    ClimbedFloor { floor: i32 },
    /// The mole jumped from a building.
    JumpedFromBuilding,
    /// Jessica caught the mole.
    CaughtByJessica,
    /// A catch attempt missed.
    MissedCatch,
    /// A dog detected the mole's position.
    DogDetectedMole,
    /// A dog successfully caught the mole.
    DogCaughtMole,
    /// The AudioSynthesiser played a diversion sound (see seam note above).
    SynthSoundPlayed { sound: String },
    /// The VibrationAnalyser sensed movement above (see seam note above).
    VibrationDetected { reading: VibrationReading },
    /// The SignalProcessor detected this many objects ahead.
    UndergroundScanComplete { objects: u32 },
}

/// Construction parameters (archive `make` arguments).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoleParams {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub equipped: Option<Equipment>,
    /// Per-level hunger difficulty multiplier (see `HungerConfig`).
    pub hunger_level_multiplier: f64,
}

impl Default for MoleParams {
    fn default() -> Self {
        MoleParams {
            id: "moletaire".to_owned(),
            x: 0.0,
            y: 0.0,
            equipped: None,
            hunger_level_multiplier: 1.0,
        }
    }
}

/// The full mutable runtime state (the archive entity minus graphics).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoleRuntimeState {
    pub id: String,
    pub state: MoleState,
    pub x: f64,
    /// Ground-level y (rendering anchor).
    pub y: f64,
    /// 0.0 = surface, 1.0 = deepest underground.
    pub depth: f64,
    pub facing: Facing,
    /// Where the mole is ordered to go.
    pub target_x: Option<f64>,
    /// Target depth (0.0 to dig up, 1.0 to dig down).
    pub target_depth: Option<f64>,
    /// Equipment — single slot.
    pub equipped: Option<Equipment>,
    /// Generic countdown for the current action.
    pub action_timer: f64,
    /// Countdown for the trap dodge window.
    pub dodge_timer: f64,
    /// Item IDs being carried.
    pub carried_items: Vec<String>,
    pub distraction_timer: f64,
    pub distraction_x: Option<f64>,
    pub glide_start_height: f64,
    pub glide_distance: f64,
    /// 0.0 to 1.0.
    pub glide_progress: f64,
    pub alive: bool,
    /// 0.0 (full) to 1.0 (starving).
    pub hunger: f64,
    /// When hungry, periodically ignores input.
    pub hunger_fight_timer: f64,
    /// True when hunger overrides player input.
    pub is_resisting_control: bool,
    /// Where the mole is trying to eat.
    pub hunger_target_x: Option<f64>,
    /// Per-level hunger difficulty multiplier.
    pub hunger_level_multiplier: f64,
    /// Coprocessor augments (computational, not chassis).
    pub coprocessors: CoprocessorBay,
    /// Timer for the periodic underground scans.
    pub scan_timer: f64,
    /// Pending events, drained by the caller (or by `tick`).
    pub pending_events: Vec<MoleEvent>,
}

/// Pure view-state projection for frontends (no Pixi graphics are ported;
/// draw from this instead).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MoleViewState {
    /// `stateOrdinal` (0..=10).
    pub state_ordinal: u8,
    /// Facing ordinal (Left 0, Right 1).
    pub facing_ordinal: u8,
    /// World x.
    pub x: f64,
    /// `y + depth * visual_depth_offset` — the archive's surface-projected y.
    pub visual_y: f64,
    /// Body colour by state (archive palette).
    pub body_color: u32,
    /// Whether only the dirt-mound indicator should show (depth > threshold).
    pub underground: bool,
    pub alive: bool,
}

/// Body rectangle for collision detection (archive `getBodyRect`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BodyRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// A full, restorable snapshot of a run (versioned, serde — the companion
/// analogue of `scenario::RuntimeSnapshot`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoletaireSnapshot {
    pub format: String,
    pub definition: CompanionDefinition,
    pub seed: u32,
    pub tick: u64,
    pub rng: Mulberry32,
    pub state: MoleRuntimeState,
}

/// The deterministic Moletaire companion simulation.
#[derive(Debug, Clone)]
pub struct MoletaireSim {
    def: CompanionDefinition,
    seed: u32,
    rng: Mulberry32,
    tick: u64,
    state: MoleRuntimeState,
}

/// JS-flavoured float-to-string for the cable id: integers print without a
/// trailing `.0` (ReScript `Float.toString(4.0)` is `"4"`). Non-integer values
/// use Rust's shortest-roundtrip formatting, which matches JS for typical
/// gameplay coordinates; this formatter is ours, not a bit-for-bit `toString`
/// port (same stance as the scenario event log).
fn js_float_string(x: f64) -> String {
    if x.is_finite() && x.fract() == 0.0 && x.abs() < 9_007_199_254_740_992.0 {
        format!("{}", x as i64)
    } else {
        format!("{x}")
    }
}

impl MoletaireSim {
    /// Construct a mole (archive `make` defaults: surface, facing right,
    /// well-fed, stock coprocessor bay). Validates the definition.
    pub fn new(
        def: CompanionDefinition,
        params: MoleParams,
        seed: u32,
    ) -> Result<Self, Vec<CompanionValidationError>> {
        def.ok()?;
        Ok(MoletaireSim {
            def,
            seed,
            rng: Mulberry32::new(seed),
            tick: 0,
            state: MoleRuntimeState {
                id: params.id,
                state: MoleState::Idle,
                x: params.x,
                y: params.y,
                depth: 0.0,
                facing: Facing::Right,
                target_x: None,
                target_depth: None,
                equipped: params.equipped,
                action_timer: 0.0,
                dodge_timer: 0.0,
                carried_items: Vec::new(),
                distraction_timer: 0.0,
                distraction_x: None,
                glide_start_height: 0.0,
                glide_distance: 0.0,
                glide_progress: 0.0,
                alive: true,
                hunger: 0.0,
                hunger_fight_timer: 0.0,
                is_resisting_control: false,
                hunger_target_x: None,
                hunger_level_multiplier: params.hunger_level_multiplier,
                coprocessors: CoprocessorBay::new(),
                scan_timer: 0.0,
                pending_events: Vec::new(),
            },
        })
    }

    // ── Event queue ──

    /// Push an event to the pending queue (archive `emitEvent`).
    pub fn emit(&mut self, event: MoleEvent) {
        self.state.pending_events.push(event);
    }

    /// Drain all pending events (archive `drainEvents`).
    pub fn drain_events(&mut self) -> Vec<MoleEvent> {
        std::mem::take(&mut self.state.pending_events)
    }

    // ── Commands ──

    /// Apply a typed command. `UseFlash` additionally emits
    /// [`MoleEvent::FlashFired`] on success (the archive emitted it from the
    /// game loop); every other command queues exactly the archive's events.
    /// The direct methods expose the archive's return values where they exist.
    pub fn apply(&mut self, cmd: MoleCommand) {
        match cmd {
            MoleCommand::MoveTo {
                target_x,
                underground,
            } => self.order_move_to(target_x, underground),
            MoleCommand::DigTrap => self.order_dig_trap(),
            MoleCommand::SabotageCable => self.order_sabotage_cable(),
            MoleCommand::GiveItem { item_id } => {
                let _ = self.give_item(&item_id);
            }
            MoleCommand::DropItem { item_id } => {
                let _ = self.drop_item(&item_id);
            }
            MoleCommand::Deliver { delivery_x } => self.order_deliver(delivery_x),
            MoleCommand::Equip { item } => self.equip(item),
            MoleCommand::Unequip => self.unequip(),
            MoleCommand::UseFlash => {
                if self.use_flash() {
                    self.emit(MoleEvent::FlashFired);
                }
            }
            MoleCommand::LaunchGlider { launch_height } => {
                let _ = self.launch_glider(launch_height);
            }
            MoleCommand::PlaySynthSound { sound_index } => {
                let _ = self.play_synth_sound(sound_index);
            }
            MoleCommand::DistractByWire { wire_x } => {
                let _ = self.distract_by_wire(wire_x);
            }
            MoleCommand::Crush => self.crush(),
            MoleCommand::CatchByDog => self.catch_by_dog(),
            MoleCommand::Feed => self.feed(),
            MoleCommand::SetHungerTarget { target_x } => self.set_hunger_target(target_x),
            MoleCommand::UpgradeCoprocessor { ctype } => {
                let _ = self.state.coprocessors.upgrade(ctype);
            }
        }
    }

    /// Order the mole to a position (archive `orderMoveTo`). Auto-dives to
    /// 0.5 when ordered underground from near the surface; auto-surfaces when
    /// ordered above ground while underground. Ignored when dead.
    pub fn order_move_to(&mut self, target_x: f64, underground: bool) {
        let s = &mut self.state;
        if s.alive && s.state != MoleState::Dead {
            s.target_x = Some(target_x);
            if underground {
                s.state = MoleState::MovingUnderground;
                if s.depth < self.def.tuning.dive_trigger_depth {
                    s.target_depth = Some(self.def.tuning.dive_target_depth);
                }
            } else {
                s.state = MoleState::MovingAboveGround;
                if s.depth > self.def.tuning.surface_threshold {
                    s.target_depth = Some(0.0);
                }
            }
        }
    }

    /// Dig a trap at the current position (archive `orderDigTrap`; requires
    /// depth > `dig_min_depth`).
    pub fn order_dig_trap(&mut self) {
        let s = &mut self.state;
        if s.alive && s.state != MoleState::Dead && s.depth > self.def.tuning.dig_min_depth {
            s.state = MoleState::DiggingTrap;
            s.action_timer = self.def.tuning.trap_dig_duration_sec;
        }
    }

    /// Sabotage a cable at the current position (archive `orderSabotageCable`).
    pub fn order_sabotage_cable(&mut self) {
        let s = &mut self.state;
        if s.alive && s.state != MoleState::Dead && s.depth > self.def.tuning.dig_min_depth {
            s.state = MoleState::SabotagingCable;
            s.action_timer = self.def.tuning.cable_sabotage_duration_sec;
        }
    }

    /// Max carry capacity: 1 normally, 3 with the rucksack (archive
    /// `getCarryCapacity`).
    pub fn carry_capacity(&self) -> u32 {
        if self.state.equipped == Some(Equipment::Rucksack) {
            self.def.tuning.rucksack_carry_capacity
        } else {
            self.def.tuning.base_carry_capacity
        }
    }

    /// Give an item to carry; respects capacity (archive `giveItem`).
    pub fn give_item(&mut self, item_id: &str) -> bool {
        if self.state.alive && (self.state.carried_items.len() as u32) < self.carry_capacity() {
            self.state.carried_items.push(item_id.to_owned());
            self.state.state = MoleState::CarryingItem;
            self.emit(MoleEvent::ItemPickedUp {
                item_id: item_id.to_owned(),
            });
            true
        } else {
            false
        }
    }

    /// Drop a specific item; `true` if it was carried (archive `dropItem`).
    pub fn drop_item(&mut self, item_id: &str) -> bool {
        let s = &mut self.state;
        if let Some(idx) = s.carried_items.iter().position(|id| id == item_id) {
            s.carried_items.remove(idx);
            if s.carried_items.is_empty() {
                s.state = MoleState::Idle;
            }
            self.emit(MoleEvent::ItemDropped {
                item_id: item_id.to_owned(),
            });
            true
        } else {
            false
        }
    }

    /// Deliver carried items: move to the delivery point, drop on arrival
    /// (archive `orderDeliver` — only sets the target; the state machine
    /// completes it).
    pub fn order_deliver(&mut self, delivery_x: f64) {
        if self.state.alive && !self.state.carried_items.is_empty() {
            self.state.target_x = Some(delivery_x);
        }
    }

    /// Equip a single item, replacing any current one (archive `equip`).
    /// Switching away from the rucksack silently sheds excess items.
    pub fn equip(&mut self, item: Equipment) {
        let s = &mut self.state;
        if s.equipped == Some(Equipment::Rucksack) && item != Equipment::Rucksack {
            s.carried_items
                .truncate(self.def.tuning.base_carry_capacity as usize);
        }
        s.equipped = Some(item);
    }

    /// Remove all equipment (archive `unequip`).
    pub fn unequip(&mut self) {
        let s = &mut self.state;
        if s.equipped == Some(Equipment::Rucksack) {
            s.carried_items
                .truncate(self.def.tuning.base_carry_capacity as usize);
        }
        s.equipped = None;
    }

    /// Whether the flash can fire (archive `useFlash`).
    pub fn use_flash(&self) -> bool {
        self.state.alive && self.state.equipped == Some(Equipment::FlashCamera)
    }

    /// Launch the glider (archive `launchGlider`): requires the Glider,
    /// alive, on the surface. Emits `GlideStarted`.
    pub fn launch_glider(&mut self, launch_height: f64) -> bool {
        let t = &self.def.tuning;
        let s = &mut self.state;
        if s.alive && s.equipped == Some(Equipment::Glider) && s.depth < t.surface_threshold {
            s.state = MoleState::Gliding;
            s.glide_start_height = launch_height;
            s.glide_distance = launch_height * t.glider_height_multiplier;
            s.glide_progress = 0.0;
            self.emit(MoleEvent::GlideStarted {
                height: launch_height,
            });
            true
        } else {
            false
        }
    }

    // ── Coprocessor commands ──

    /// Play a synthesised sound by index (archive `playSynthSound`). Returns
    /// the sound name if the AudioSynthesiser level allows it, else `None`.
    pub fn play_synth_sound(&mut self, sound_index: usize) -> Option<String> {
        if !self.state.alive {
            return None;
        }
        let max_sounds =
            self.def
                .coprocessors
                .audio_sound_count(self.state.coprocessors.audio_synthesiser) as usize;
        if max_sounds == 0 || sound_index >= max_sounds {
            return None;
        }
        let sound = self.def.available_sounds.get(sound_index)?.clone();
        self.emit(MoleEvent::SynthSoundPlayed {
            sound: sound.clone(),
        });
        Some(sound)
    }

    /// Whether the mole can mimic a guard's voice right now (archive
    /// `canMimicVoice` — alive + MK-III AudioSynthesiser).
    pub fn can_mimic_voice(&self) -> bool {
        self.state.alive && self.state.coprocessors.can_mimic_voice()
    }

    /// Scan ahead underground (archive `scanAhead`): 0 when dead or on the
    /// surface, else the SignalProcessor's sensor range in grid cells.
    pub fn scan_ahead(&self) -> u32 {
        if !self.state.alive || self.state.depth <= self.def.tuning.surface_threshold {
            0
        } else {
            self.def
                .coprocessors
                .sensor_range(self.state.coprocessors.signal_processor)
        }
    }

    /// Whether vault weak points are detectable right now (archive
    /// `canDetectVaultWeakPoints` — alive, underground, MK-III SignalProcessor).
    pub fn can_detect_vault_weak_points(&self) -> bool {
        self.state.alive
            && self.state.depth > self.def.tuning.surface_threshold
            && self.state.coprocessors.can_detect_vault_weak_points()
    }

    /// Read vibration data from above (archive `readVibrations`): `NoData`
    /// when dead or on the surface, else the VibrationAnalyser's quality.
    pub fn read_vibrations(&self) -> VibrationReading {
        if !self.state.alive || self.state.depth <= self.def.tuning.surface_threshold {
            VibrationReading::NoData
        } else {
            self.def
                .coprocessors
                .vibration_quality(self.state.coprocessors.vibration_analyser)
        }
    }

    // ── Distraction and death ──

    /// A loose wire pulls the mole (archive `distractByWire`): within 200 px,
    /// unless dead, digging, or sabotaging.
    pub fn distract_by_wire(&mut self, wire_x: f64) -> bool {
        let t = &self.def.tuning;
        let s = &mut self.state;
        if s.alive
            && s.state != MoleState::Dead
            && s.state != MoleState::DiggingTrap
            && s.state != MoleState::SabotagingCable
            && (wire_x - s.x).abs() < t.wire_distraction_range
        {
            s.state = MoleState::Distracted;
            s.distraction_timer = t.distraction_duration_sec;
            s.distraction_x = Some(wire_x);
            true
        } else {
            false
        }
    }

    /// Guard fell into the trap and the mole didn't dodge (archive `crushMole`).
    pub fn crush(&mut self) {
        if self.state.alive {
            self.state.state = MoleState::Crushed;
            self.state.alive = false;
        }
    }

    /// RoboDog dug down and caught the mole (archive `catchByDog`).
    pub fn catch_by_dog(&mut self) {
        if self.state.alive {
            self.state.state = MoleState::CaughtByDog;
            self.state.alive = false;
        }
    }

    /// Feed the mole — resets hunger to 0 (archive `feed`). Emits `FoodEaten`.
    pub fn feed(&mut self) {
        self.state.hunger = 0.0;
        self.state.hunger_fight_timer = 0.0;
        self.state.is_resisting_control = false;
        self.state.hunger_target_x = None;
        self.emit(MoleEvent::FoodEaten);
    }

    /// Set a hunger target (archive `setHungerTarget`).
    pub fn set_hunger_target(&mut self, target_x: f64) {
        self.state.hunger_target_x = Some(target_x);
    }

    // ── Movement update (archive `updateMovement`) ──

    fn update_movement(&mut self, dt: f64) -> Option<MoleEvent> {
        let t = self.def.tuning.clone();

        // Move toward the depth target.
        if let Some(target) = self.state.target_depth {
            let depth_diff = target - self.state.depth;
            if depth_diff.abs() < t.depth_epsilon {
                self.state.depth = target;
                self.state.target_depth = None;
            } else {
                let direction = if depth_diff > 0.0 { 1.0 } else { -1.0 };
                self.state.depth += direction * t.depth_speed * dt;
            }
        }

        // Move toward the x target.
        let target = self.state.target_x?;
        let dx = target - self.state.x;
        let dist = dx.abs();
        // PathOptimiser: smarter routing applies to all underground movement.
        let path_mult = self
            .def
            .coprocessors
            .path_efficiency(self.state.coprocessors.path_optimiser);
        let speed = match self.state.state {
            MoleState::MovingUnderground => t.underground_speed * path_mult,
            MoleState::MovingAboveGround => {
                if self.state.equipped == Some(Equipment::Skateboard) {
                    t.skateboard_speed
                } else {
                    t.above_ground_speed
                }
            }
            MoleState::CarryingItem => {
                if self.state.depth > t.surface_threshold {
                    t.underground_speed * t.carrying_underground_multiplier * path_mult
                } else {
                    t.above_ground_speed
                }
            }
            _ => 0.0,
        };

        if dist < t.arrive_distance {
            self.state.x = target;
            self.state.target_x = None;

            // Deliver items on arrival if carrying.
            if self.state.state == MoleState::CarryingItem && !self.state.carried_items.is_empty() {
                let item_id = self.state.carried_items.remove(0);
                // Eat roll — drawn before the chance comparison, one draw per
                // delivery (RNG stream, see module docs).
                let roll = self.rng.next_f64();
                if self.state.carried_items.is_empty() {
                    self.state.state = MoleState::Idle;
                }
                // StabilisationCore reduces the eat chance.
                let eat_chance = t.item_eat_chance
                    * self
                        .def
                        .coprocessors
                        .eat_chance_multiplier(self.state.coprocessors.stabilisation_core);
                if roll < eat_chance {
                    Some(MoleEvent::ItemEaten { item_id })
                } else {
                    Some(MoleEvent::ItemDelivered { item_id })
                }
            } else {
                self.state.state = MoleState::Idle;
                Some(MoleEvent::ReachedDestination)
            }
        } else {
            let direction = if dx > 0.0 { 1.0 } else { -1.0 };
            self.state.x += direction * speed * dt;
            self.state.facing = if dx > 0.0 {
                Facing::Right
            } else {
                Facing::Left
            };
            None
        }
    }

    // ── State machine update (archive `updateState`) ──

    fn update_state(&mut self, dt: f64) -> Option<MoleEvent> {
        if !self.state.alive {
            return None;
        }
        match self.state.state {
            MoleState::Idle | MoleState::Dead => None,

            MoleState::MovingUnderground
            | MoleState::MovingAboveGround
            | MoleState::CarryingItem => self.update_movement(dt),

            MoleState::DiggingTrap => {
                self.state.action_timer -= dt;
                if self.state.action_timer <= 0.0 {
                    self.state.state = MoleState::Idle;
                    // Trap is ready — the caller checks for guard collision.
                    Some(MoleEvent::TrapTriggered {
                        x: self.state.x,
                        y: self.state.y,
                    })
                } else {
                    None
                }
            }

            MoleState::SabotagingCable => {
                self.state.action_timer -= dt;
                if self.state.action_timer <= 0.0 {
                    self.state.state = MoleState::Idle;
                    Some(MoleEvent::CableSabotaged {
                        cable_id: format!("cable_{}", js_float_string(self.state.x)),
                    })
                } else {
                    None
                }
            }

            MoleState::Distracted => {
                self.state.distraction_timer -= dt;
                // Move toward the distraction source.
                if let Some(d_x) = self.state.distraction_x {
                    let dx = d_x - self.state.x;
                    let direction = if dx > 0.0 { 1.0 } else { -1.0 };
                    self.state.x += direction * self.def.tuning.above_ground_speed * dt;
                    self.state.facing = if dx > 0.0 {
                        Facing::Right
                    } else {
                        Facing::Left
                    };
                }
                if self.state.distraction_timer <= 0.0 {
                    self.state.state = MoleState::Idle;
                    self.state.distraction_x = None;
                }
                None
            }

            MoleState::Gliding => {
                // Horizontal glide: distance/2 scaled by fall speed over 60.
                let glide_speed =
                    (self.state.glide_distance / 2.0) * self.def.tuning.glider_fall_speed / 60.0;
                let direction = match self.state.facing {
                    Facing::Right => 1.0,
                    Facing::Left => -1.0,
                };
                self.state.x += direction * glide_speed * dt;
                self.state.glide_progress += dt * self.def.tuning.glide_progress_rate;
                if self.state.glide_progress >= 1.0 {
                    self.state.state = MoleState::Idle;
                    self.state.depth = 0.0;
                    Some(MoleEvent::GlideComplete { x: self.state.x })
                } else {
                    None
                }
            }

            MoleState::Crushed | MoleState::CaughtByDog => {
                // Archive quirk: state is reassigned to Dead before the event
                // is built, so MoleDied carries Dead (see module docs).
                self.state.alive = false;
                self.state.state = MoleState::Dead;
                Some(MoleEvent::MoleDied {
                    state: self.state.state,
                })
            }
        }
    }

    // ── Per-tick update (archive `update`) ──

    /// Advance one dt step. `edibles` is the world's current edible set (the
    /// hunger gravity field). Returns every event this tick produced, in
    /// order — command-queued events first, then the state-machine event,
    /// then scans, then hunger transitions.
    pub fn tick(&mut self, dt: f64, edibles: &[EdibleObject]) -> Vec<MoleEvent> {
        self.tick += 1;
        let t = self.def.tuning.clone();

        // 1. State machine / movement.
        if let Some(ev) = self.update_state(dt) {
            self.state.pending_events.push(ev);
        }

        // 2. Dodge timer countdown.
        if self.state.dodge_timer > 0.0 {
            self.state.dodge_timer -= dt;
        }

        // 3. Periodic coprocessor scans (alive and underground only).
        if self.state.alive && self.state.depth > t.surface_threshold {
            self.state.scan_timer -= dt;
            if self.state.scan_timer <= 0.0 {
                self.state.scan_timer = t.scan_interval_sec;

                // 3a. Underground scan (SignalProcessor) while moving/digging.
                if self.state.state == MoleState::MovingUnderground
                    || self.state.state == MoleState::DiggingTrap
                {
                    let objects = self.scan_ahead();
                    if objects > 0 {
                        self.emit(MoleEvent::UndergroundScanComplete { objects });
                    }
                }

                // 3b. Vibration analysis (VibrationAnalyser).
                let reading = self.read_vibrations();
                if reading != VibrationReading::NoData {
                    self.emit(MoleEvent::VibrationDetected { reading });
                }
            }
        }

        // 4. Hunger integration + gravity pull.
        if self.state.alive {
            let was_resisting = self.state.is_resisting_control;
            let is_moving = matches!(
                self.state.state,
                MoleState::MovingUnderground
                    | MoleState::MovingAboveGround
                    | MoleState::CarryingItem
            );
            // StabilisationCore reduces metabolic drain as well as eat chance.
            let coprocessor_mult = self
                .def
                .coprocessors
                .eat_chance_multiplier(self.state.coprocessors.stabilisation_core);
            let hunger_increase = hunger::calculate_hunger_rate(
                t.hunger_rate,
                is_moving,
                self.state.hunger,
                coprocessor_mult,
                self.state.hunger_level_multiplier,
            );
            self.state.hunger = (self.state.hunger + hunger_increase * dt).min(1.0);

            // Total gravitational pull toward all nearby edibles.
            let (total_pull, nearest_id, nearest_dist) = hunger::calculate_total_pull(
                self.state.x,
                self.state.y,
                self.state.hunger,
                edibles,
            );

            // Track the nearest edible (visual feedback: where he's looking).
            self.state.hunger_target_x = nearest_id
                .and_then(|id| edibles.iter().find(|e| e.id == id))
                .map(|e| e.x);

            if self.state.hunger > t.hungry_threshold {
                // Periodic control-resistance episodes (exact archive shape).
                self.state.hunger_fight_timer += dt;
                if self.state.hunger_fight_timer > t.hunger_fight_interval {
                    self.state.is_resisting_control = true;
                    if self.state.hunger_fight_timer
                        > t.hunger_fight_interval + t.hunger_fight_duration
                    {
                        self.state.hunger_fight_timer = 0.0;
                        self.state.is_resisting_control = false;
                    }
                } else {
                    self.state.is_resisting_control = false;
                }

                if self.state.is_resisting_control {
                    // Full gravity pull — the mole moves autonomously to food.
                    self.state.x += total_pull * dt;
                    if total_pull != 0.0 {
                        self.state.facing = if total_pull > 0.0 {
                            Facing::Right
                        } else {
                            Facing::Left
                        };
                    }
                } else {
                    // Partial drag — slows the mole when moving away from food.
                    self.state.x += total_pull * t.hunger_drag_multiplier * dt;
                }

                // Starving with nothing nearby — wander aimlessly (one RNG
                // draw per qualifying tick; see module docs).
                if self.state.hunger > t.starving_threshold && nearest_dist > t.wander_min_distance
                {
                    let wander_dir = if self.rng.next_f64() > 0.5 { 1.0 } else { -1.0 };
                    self.state.x +=
                        wander_dir * t.above_ground_speed * t.wander_speed_multiplier * dt;
                }
            } else {
                self.state.is_resisting_control = false;
            }

            // Resistance transition events.
            if !was_resisting && self.state.is_resisting_control {
                self.emit(MoleEvent::HungerResistanceStarted);
            } else if was_resisting && !self.state.is_resisting_control {
                self.emit(MoleEvent::HungerResistanceEnded);
            }
        }

        // 5. No rendering — `view()` is the pure projection.
        self.drain_events()
    }

    // ── Queries (archive query block) ──

    pub fn is_alive(&self) -> bool {
        self.state.alive
    }
    pub fn is_dead(&self) -> bool {
        !self.state.alive
    }
    pub fn is_underground(&self) -> bool {
        self.state.depth > self.def.tuning.surface_threshold
    }
    pub fn is_idle(&self) -> bool {
        self.state.state == MoleState::Idle
    }
    pub fn is_digging(&self) -> bool {
        self.state.state == MoleState::DiggingTrap || self.state.state == MoleState::SabotagingCable
    }
    pub fn is_distracted(&self) -> bool {
        self.state.state == MoleState::Distracted
    }
    pub fn is_carrying(&self) -> bool {
        !self.state.carried_items.is_empty()
    }
    pub fn is_gliding(&self) -> bool {
        self.state.state == MoleState::Gliding
    }
    /// Is the mole currently fighting the controller due to hunger?
    pub fn is_resisting_control(&self) -> bool {
        self.state.is_resisting_control
    }
    /// Hunger level (0.0 full, 1.0 starving).
    pub fn hunger(&self) -> f64 {
        self.state.hunger
    }
    /// Is the mole starving (will eat objectives)?
    pub fn is_starving(&self) -> bool {
        self.state.hunger > self.def.tuning.starving_threshold
    }
    pub fn depth(&self) -> f64 {
        self.state.depth
    }
    pub fn distance_to(&self, x: f64) -> f64 {
        (x - self.state.x).abs()
    }
    /// The full runtime state, read-only (tests and overseer views).
    pub fn state(&self) -> &MoleRuntimeState {
        &self.state
    }
    /// Mutable runtime state — the test-oracle escape hatch the archive's
    /// record mutability afforded (e.g. forcing `depth` before `orderDigTrap`).
    pub fn state_mut(&mut self) -> &mut MoleRuntimeState {
        &mut self.state
    }
    /// The validated definition this sim runs on.
    pub fn definition(&self) -> &CompanionDefinition {
        &self.def
    }
    /// Elapsed ticks.
    pub fn ticks(&self) -> u64 {
        self.tick
    }
    /// The construction seed.
    pub fn seed(&self) -> u32 {
        self.seed
    }
    /// The RNG stream, resumed mid-sequence by [`MoletaireSim::restore`]. Read
    /// by the scenario host that snapshots the mole's changing parts without
    /// re-carrying the (constant) companion definition every tick.
    pub fn rng(&self) -> &Mulberry32 {
        &self.rng
    }

    // ── View state and hitbox ──

    /// Pure view-state projection: state/facing ordinals, the
    /// surface-projected `visual_y`, and the archive body-colour palette.
    pub fn view(&self) -> MoleViewState {
        MoleViewState {
            state_ordinal: self.state.state.ordinal(),
            facing_ordinal: self.state.facing.ordinal(),
            x: self.state.x,
            visual_y: self.state.y + self.state.depth * self.def.tuning.visual_depth_offset,
            body_color: self.state.state.body_color(),
            underground: self.state.depth > self.def.tuning.surface_threshold,
            alive: self.state.alive,
        }
    }

    /// Body rectangle for collision detection (archive `getBodyRect`).
    pub fn body_rect(&self) -> BodyRect {
        let t = &self.def.tuning;
        BodyRect {
            x: self.state.x - t.body_width / 2.0,
            y: self.state.y - t.body_height - self.state.depth * t.hitbox_depth_offset,
            w: t.body_width,
            h: t.body_height,
        }
    }

    // ── Snapshot / restore ──

    /// A full, restorable snapshot (format
    /// [`MOLETAIRE_SNAPSHOT_FORMAT`]).
    pub fn snapshot(&self) -> MoletaireSnapshot {
        MoletaireSnapshot {
            format: MOLETAIRE_SNAPSHOT_FORMAT.to_owned(),
            definition: self.def.clone(),
            seed: self.seed,
            tick: self.tick,
            rng: self.rng.clone(),
            state: self.state.clone(),
        }
    }

    /// Restore a sim from a snapshot: format tag checked, definition
    /// re-validated, RNG resumed mid-sequence.
    pub fn restore(snap: MoletaireSnapshot) -> Result<Self, Vec<CompanionValidationError>> {
        if snap.format != MOLETAIRE_SNAPSHOT_FORMAT {
            return Err(vec![CompanionValidationError::UnsupportedSnapshotFormat {
                found: snap.format,
            }]);
        }
        snap.definition.ok()?;
        Ok(MoletaireSim {
            def: snap.definition,
            seed: snap.seed,
            rng: snap.rng,
            tick: snap.tick,
            state: snap.state,
        })
    }
}

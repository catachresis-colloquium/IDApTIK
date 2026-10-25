//! Ported numeric constants — the single source of truth for every magic number
//! in the "Ghost Lobby" prototype.
//!
//! These values are transliterated faithfully from the canonical HTML prototype
//! (`idaptik - ghost lobby.html`). They are grouped by system to match the tick
//! pipeline. [`crate::scenario::ghost_lobby::ghost_lobby`] projects them into the
//! declarative [`crate::scenario::definition::ScenarioDefinition`] so that both a
//! compile-time audit (these `const`s) and a JSON round-trip (the definition)
//! stay in lock-step; `json_roundtrip` asserts the projection equality.
//!
//! Every value here is documented in `docs/scenarios/ghost-lobby.md`; the code
//! deliberately does *not* cite individual HTML line numbers.

// ---------------------------------------------------------------------------
// Geometry — the world is a flat side-view; a single floor line, entities as
// axis-aligned boxes, rooms as [x, x+w) spans.
// ---------------------------------------------------------------------------
pub const FLOOR: f64 = 585.0;
/// Both entities' "centre" for room/collision tests is `x + ROOM_OFFSET`.
pub const ROOM_OFFSET: f64 = 14.0;
pub const PLAYER_W: f64 = 28.0;
pub const PLAYER_H: f64 = 54.0;
pub const BILLY_W: f64 = 36.0;
pub const BILLY_H: f64 = 64.0;
pub const COLLISION_RADIUS: f64 = 30.0;
/// Right edge of the last room (exit.x + exit.w) — the world width.
pub const WORLD_WIDTH: f64 = 1260.0;

// ---------------------------------------------------------------------------
// Player movement
// ---------------------------------------------------------------------------
pub const GRAVITY: f64 = 1080.0;
pub const JUMP_VY: f64 = -430.0;
pub const ACCEL_DRIVE: f64 = 980.0;
pub const ACCEL_BRAKE: f64 = 1250.0;
pub const CROUCH_SPEED: f64 = 0.46;
pub const STAM_DRAIN: f64 = 24.0;
pub const STAM_REGEN: f64 = 17.0;
pub const STAM_REGEN_CROUCH: f64 = 11.0;
pub const SPRINT_MIN_STAM: f64 = 2.0;
pub const HIDE_MAX_VX: f64 = 92.0;
pub const CROUCH_CAM_VX: f64 = 80.0;
/// Velocity threshold above which an entity counts as "moving".
pub const MOVING_VX: f64 = 65.0;
pub const PLAYER_CLAMP_LO: f64 = 26.0;
pub const PLAYER_CLAMP_HI: f64 = 1250.0;
pub const LAND_VY: f64 = 190.0;

// ---------------------------------------------------------------------------
// Noise model — every contribution is max()'d together, then decays once.
// ---------------------------------------------------------------------------
pub const NOISE_JUMP: f64 = 0.7;
pub const NOISE_LAND: f64 = 0.8;
pub const NOISE_DECAY: f64 = 1.1;
pub const NOISE_SPRINT_BASE: f64 = 0.68;
pub const NOISE_SPRINT_K: f64 = 0.3;
pub const NOISE_WALK_K: f64 = 0.24;
pub const NOISE_CROUCH: f64 = 0.08;
pub const NOISE_MOVE_MIN_VX: f64 = 15.0;

// ---------------------------------------------------------------------------
// Billy — spawn, motion, FSM timers, sight.
// ---------------------------------------------------------------------------
pub const BILLY_START_X: f64 = -90.0;
pub const CRISIS_BILLY_X: f64 = 26.0;
pub const ENTER_SPEED: f64 = 0.72;
pub const SHOCK_T: f64 = 1.15;
pub const ASSESS_SPEED: f64 = 0.55;
pub const PATROL_LO: f64 = 130.0;
pub const PATROL_HI: f64 = 355.0;
pub const PATROL_PIVOT: f64 = 220.0;
pub const INVEST_SPEED: f64 = 0.82;
pub const SECURE_SPEED: f64 = 0.92;
pub const GRAB_DIST: f64 = 31.0;
pub const GUARD_SPEED: f64 = 0.45;
pub const GUARD_T_NOTE: f64 = 1.9;
pub const GUARD_T_USB: f64 = 1.7;
pub const CALL_T: f64 = 1.9;
pub const PURSUE_SPEED: f64 = 1.2;
pub const PURSUE_TRIGGER: f64 = 115.0;
pub const PURSUE_GIVEUP_AGO: f64 = 3.2;
pub const PURSUE_GIVEUP_DIST: f64 = 20.0;
pub const BILLY_ACCEL: f64 = 520.0;
pub const BILLY_CLAMP_LO: f64 = 25.0;
pub const BILLY_CLAMP_HI: f64 = 1230.0;

// ---------------------------------------------------------------------------
// Supervision (supervised runs only) — the security team directing Billy's
// Assess sweep. Kept as constants for this slice; projection into the tuning
// table (and the golden scenario JSON) is deferred until they need to vary
// per difficulty.
// ---------------------------------------------------------------------------
/// The Ghost Lobby security team's id (event payloads and state).
pub const SUPERVISION_TEAM_ID: &str = "ghost-lobby-security";
/// Minimum team attention (0..=100) before the patrol band bends at all.
pub const SUPERVISION_ATTENTION_MIN: u8 = 25;
/// Patrol half-width around the coverage target at full attention.
pub const SUPERVISION_COVERAGE_BAND: f64 = 90.0;
pub const BADGE_OPEN: f64 = 1.55;
/// How close Billy's centre must stay to a closed door's plane to keep his
/// badge wait running. The door constraint bounces him 3px clear and he
/// re-accelerates from rest, so a blocked Billy oscillates within a few pixels
/// of the plane; this window covers that cycle without letting a wait survive
/// him walking away.
pub const BILLY_DOOR_REACH: f64 = 12.0;
pub const SNACK_REACH: f64 = 12.0;
pub const ALERT_BOOST_DIV: f64 = 260.0;
pub const LIGHTS_BOOST_K: f64 = 0.035;
pub const VAC_DISTRACT: f64 = 245.0;
/// Sight multipliers: facing away, crouched target, lit office, high-alert.
pub const SIGHT_BACK: f64 = 0.34;
pub const SIGHT_CROUCH: f64 = 0.62;
pub const SIGHT_OFFICE: f64 = 1.15;
pub const SIGHT_ALERT: f64 = 1.12;
pub const SIGHT_ALERT_HI: f64 = 65.0;
pub const NOISE_INVEST_NOISE: f64 = 0.45;
pub const NOISE_INVEST_DIST: f64 = 260.0;

// ---------------------------------------------------------------------------
// Belief model — Billy's read of note / usb / player interest (0..100+).
// ---------------------------------------------------------------------------
pub const BELIEF_THRESHOLD: f64 = 48.0;
pub const MISDIRECT_THRESHOLD: f64 = 72.0;
pub const PI_SEEN: f64 = 4.0;
pub const PI_SPRINT: f64 = 12.0;
pub const NOTE_NEAR: f64 = 105.0;
pub const USB_NEAR: f64 = 112.0;
pub const NOTE_URG_SPRINT: f64 = 20.0;
pub const NOTE_URG_MOVE: f64 = 10.0;
pub const NOTE_URG_STILL: f64 = 4.0;
pub const USB_URG_SPRINT: f64 = 17.0;
pub const USB_URG_MOVE: f64 = 8.0;
pub const USB_URG_STILL: f64 = 3.0;
pub const NOTE_CARRY: f64 = 13.0;
pub const USB_CARRY: f64 = 12.0;
pub const PROG_GATE: f64 = 0.12;
pub const PI_DECAY: f64 = 0.9;
pub const NOTE_DECAY: f64 = 0.25;
pub const USB_DECAY: f64 = 0.2;
pub const PEEL_INTEREST: f64 = 36.0;
pub const SECURE_NOTE_SEEN: f64 = 72.0;
pub const TAKE_USB_SEEN: f64 = 74.0;
pub const TAKE_USB_UNSEEN: f64 = 28.0;
pub const CAMERA_PLAYER_INT: f64 = 30.0;

// ---------------------------------------------------------------------------
// Support envelope + isolation + bandwidth + alert
// ---------------------------------------------------------------------------
pub const SUPPORT_PING_HALL: f64 = 0.16;
pub const SUPPORT_PING_OFFICE: f64 = 0.30;
pub const SUPPORT_PING_LAUNDRY: f64 = 0.05;
pub const SUPPORT_PING_EXIT: f64 = 0.12;
pub const SUPPORT_LOWBW_X: f64 = 18.0;
pub const SUPPORT_LOWBW_PEN: f64 = 0.12;
pub const SUPPORT_ALERT_PEN: f64 = 0.0022;
pub const SUPPORT_HIDDEN: f64 = 0.06;
pub const SUPPORT_FLICKER: f64 = 0.04;
// What each pivot the hacker stands on costs the link. Acting close is safe;
// reaching far frays it. The bridge route is one hop, the upstream substation
// route two, and that depth is the only thing that tells the two lines apart.
pub const SUPPORT_HOP_PEN: f64 = 0.07;
pub const SUPPORT_CLAMP_MIN: f64 = 0.05;
pub const SUPPORT_APPROACH: f64 = 1.15;
pub const ISO_GATE: f64 = 0.4;
pub const ISO_PRESSURE_DIV: f64 = 130.0;
pub const FRAY_FRAC: f64 = 0.5;
pub const ISO_DECAY: f64 = 1.35;
pub const ISO_DECAY_HIDDEN: f64 = 2.2;
pub const PING_DUR: f64 = 3.4;
pub const BW_REGEN_A: f64 = 0.55;
pub const BW_REGEN_B: f64 = 0.75;
pub const ALERT_DECAY_QUIET: f64 = 0.45;
pub const ALERT_DECAY_CRISIS: f64 = 0.18;
pub const LOCKDOWN: f64 = 100.0;

// ---------------------------------------------------------------------------
// Uplink actions — cost / cooldown / alert-gain, plus timing.
// ---------------------------------------------------------------------------
pub const ACT_CAMERA_COST: f64 = 18.0;
pub const ACT_CAMERA_CD: f64 = 4.2;
pub const ACT_CAMERA_ALERT: f64 = 2.4;
pub const ACT_DOOR_COST: f64 = 17.0;
pub const ACT_DOOR_CD: f64 = 1.0;
pub const ACT_DOOR_ALERT: f64 = 3.2;
pub const ACT_VACUUM_COST: f64 = 25.0;
pub const ACT_VACUUM_CD: f64 = 8.0;
pub const ACT_VACUUM_ALERT: f64 = 4.2;
pub const ACT_LIGHTS_COST: f64 = 15.0;
pub const ACT_LIGHTS_CD: f64 = 5.0;
/// Base lights alert gain; escalates by `8.5 + escalation` on repeat uses.
pub const ACT_LIGHTS_ALERT: f64 = 8.5;
pub const LIGHTS_DUR: f64 = 1.45;
pub const LIGHTS_DUR_RM: f64 = 0.7;
pub const LIGHTS_STUN: f64 = 1.05;
pub const ROUTE_DURATION: f64 = 3.2;
pub const THROTTLE_CDBW: f64 = 1.2;
pub const THROTTLE_FRAY: f64 = 4.2;
pub const THROTTLE_BADGE: f64 = 8.0;
pub const CALL_ALERT: f64 = 12.0;

// ---------------------------------------------------------------------------
// Patrol cameras
// ---------------------------------------------------------------------------
pub const CAM_SWEEP_W: f64 = 0.75;
pub const CAM_SWEEP_A: f64 = 25.0;
/// Seconds of looped footage a hacked camera replays. Not a prototype value: the
/// prototype had no camera-loop hack. Chosen to outlast [`PING_DUR`] so that
/// holding a camera is meaningfully longer-lived than the intel ping it grants.
pub const CAM_LOOP_DUR: f64 = 4.0;
pub const CAM_LOCKOUT: f64 = 5.5;
pub const CAM_ALERT: f64 = 14.0;
pub const CAM_DECAY_UNSEEN: f64 = 1.4;
pub const CAM_DECAY_OFF: f64 = 2.2;

// ---------------------------------------------------------------------------
// USB device — throw ballistics, drag, self-wipe.
// ---------------------------------------------------------------------------
pub const USB_THROW_VX: f64 = 330.0;
pub const USB_THROW_VY: f64 = -170.0;
pub const USB_THROW_OFF: f64 = 34.0;
pub const USB_HELD_OFF: f64 = 15.0;
pub const USB_GRAV: f64 = 780.0;
pub const USB_DRAG: f64 = 0.985;
pub const USB_FLOOR: f64 = FLOOR - 11.0;
pub const USB_BOUNCE: f64 = -0.24;
pub const USB_FRICTION: f64 = 0.72;
pub const USB_REST_VX: f64 = 14.0;
pub const USB_REST_VY: f64 = 18.0;
pub const USB_CLAMP_LO: f64 = 35.0;
pub const USB_CLAMP_HI: f64 = 1235.0;
pub const USB_WIPE_ALERT: f64 = 27.0;
pub const USB_THROW_ALERT: f64 = 5.0;

// ---------------------------------------------------------------------------
// Robot vacuum — distraction that reveals the chute.
// ---------------------------------------------------------------------------
pub const VAC_LAG_X: f64 = 795.0;
pub const VAC_CTRL_LOSS: f64 = 0.42;
pub const VAC_CTRL_MIN: f64 = 0.22;
pub const VAC_LAG_WARN: f64 = 0.72;
pub const VAC_CTRL_GAIN: f64 = 0.18;
pub const VAC_SPEED: f64 = 72.0;
pub const VAC_MOVE_MIN: f64 = 0.26;
pub const VAC_TARGET: f64 = 958.0;
pub const VAC_FALL_OFF: f64 = 8.0;

// ---------------------------------------------------------------------------
// Interactions — hold durations and reach distances.
// ---------------------------------------------------------------------------
pub const NOTE_DIST: f64 = 58.0;
pub const NOTE_HOLD: f64 = 0.85;
pub const USB_DIST: f64 = 50.0;
pub const CHUTE_ENTER: f64 = 62.0;
pub const CHUTE_SEARCH_OFF: f64 = 18.0;
pub const CHUTE_SEARCH_DIST: f64 = 70.0;
pub const CHUTE_HOLD: f64 = 1.55;
pub const PICKPOCKET_DIST: f64 = 57.0;
pub const PICKPOCKET_HOLD: f64 = 0.72;
pub const EXIT_PROMPT_X: f64 = 1215.0;
pub const EXIT_X: f64 = 1242.0;

// ---------------------------------------------------------------------------
// One-shot rescue
// ---------------------------------------------------------------------------
pub const RESCUE_SUPPORT_MIN: f64 = 0.68;
pub const RESCUE_BW_MIN: f64 = 20.0;
pub const RESCUE_BW_COST: f64 = 20.0;
pub const RESCUE_STUN: f64 = 1.6;
pub const RESCUE_GRACE: f64 = 1.8;
pub const RESCUE_DISPLACE: f64 = 58.0;

// ---------------------------------------------------------------------------
// Scoring / debrief
// ---------------------------------------------------------------------------
pub const SCORE_BASE: f64 = 700.0;
pub const SCORE_NOTE: f64 = 650.0;
pub const SCORE_NOTE_LOST: f64 = -260.0;
pub const SCORE_NOTE_NONE: f64 = -80.0;
pub const SCORE_MISDIR: f64 = 330.0;
pub const SCORE_MISDIR_LEAK: f64 = -180.0;
pub const SCORE_NOBOSS: f64 = 150.0;
pub const SCORE_BOSS: f64 = -140.0;
pub const SCORE_NOCAM: f64 = 120.0;
pub const SCORE_CAM_EACH: f64 = -65.0;
pub const SCORE_ISO_OK: f64 = 100.0;
pub const SCORE_ISO_SNAP: f64 = -90.0;
pub const SCORE_ISO_SNAP_FRAC: f64 = 0.55;
pub const SCORE_CHUTE: f64 = 110.0;
pub const SCORE_USBTRACE: f64 = -170.0;
pub const SCORE_RESCUE: f64 = -80.0;
pub const SCORE_TIME_BASE: f64 = 220.0;
pub const SCORE_TIME_K: f64 = 1.7;
pub const SCORE_ALERT_K: f64 = 2.7;
pub const SCORE_FAIL_ACT: f64 = 28.0;
pub const FAIL_NOTE: f64 = 420.0;
pub const FAIL_BASE: f64 = 120.0;
pub const FAIL_USB: f64 = 180.0;
pub const FAIL_ALERT_K: f64 = 2.0;

// Grade thresholds (raw score, pre score-multiplier).
pub const GRADE_S: f64 = 1850.0;
pub const GRADE_A: f64 = 1450.0;
pub const GRADE_B: f64 = 1050.0;
pub const GRADE_C: f64 = 700.0;
pub const GRADE_FAIL_C: f64 = 500.0;

// ---------------------------------------------------------------------------
// Object spawn positions (static bases; note.x / usb.x are re-rolled at reset).
// ---------------------------------------------------------------------------
/// The contact note sits on the fridge door at about eye height — the real
/// objective, hiding in plain sight next to the shiny decoy. Expressed off
/// `FLOOR` like every other prop: the old absolute 282.0 put it 303px up
/// (5.6 player-heights, ceiling level), which the Bevy frontend draws
/// literally, so the thing the mission turns on floated out of reach.
pub const NOTE_Y: f64 = FLOOR - 46.0;
pub const USB_Y: f64 = FLOOR - 16.0;
pub const CHUTE_X: f64 = 946.0;
pub const CHUTE_Y: f64 = FLOOR - 90.0;
pub const VACUUM_X: f64 = 592.0;
pub const VACUUM_Y: f64 = FLOOR - 17.0;
pub const PLAYER_SPAWN_X: f64 = 72.0;
pub const PLAYER_SPAWN_Y: f64 = FLOOR - PLAYER_H;
pub const BILLY_SPAWN_X: f64 = -90.0;
pub const BILLY_SPAWN_Y: f64 = FLOOR - BILLY_H;
/// Billy's initial belief bookkeeping (constant, not rolled).
pub const BILLY_LAST_KNOWN_X: f64 = 130.0;
pub const BILLY_PATROL_TARGET: f64 = 155.0;
pub const BILLY_LAST_SEEN_AGO: f64 = 999.0;

// ---------------------------------------------------------------------------
// Reset RNG spawn ranges (base, span) — draw order is load-bearing.
// ---------------------------------------------------------------------------
pub const SPAWN_NOTE_X_BASE: f64 = 92.0;
pub const SPAWN_NOTE_X_SPAN: f64 = 78.0;
pub const SPAWN_USB_X_BASE: f64 = 622.0;
pub const SPAWN_USB_X_SPAN: f64 = 112.0;
pub const SPAWN_STALE_BASE: f64 = 2.5;
pub const SPAWN_STALE_SPAN: f64 = 3.5;
pub const SPAWN_SNACK_BASE: f64 = 165.0;
pub const SPAWN_SNACK_SPAN: f64 = 68.0;
pub const SPAWN_DOOR_DELAY_BASE: f64 = 0.22;
pub const SPAWN_DOOR_DELAY_SPAN: f64 = 0.5;
pub const OPERATOR_DOOR_PENALTY: f64 = 0.45;

// ---------------------------------------------------------------------------
// Per-room support / sight quirks (baked into RoomDef so no code branches on
// room string ids). Camera-ping support bonus + billy-sight multiplier.
// ---------------------------------------------------------------------------
pub const ROOM_KITCHEN_SUPPORT: f64 = 1.00;
pub const ROOM_HALL_SUPPORT: f64 = 0.84;
pub const ROOM_OFFICE_SUPPORT: f64 = 0.34;
pub const ROOM_LAUNDRY_SUPPORT: f64 = 0.50;
pub const ROOM_EXIT_SUPPORT: f64 = 0.69;

// ---------------------------------------------------------------------------
// Difficulty presets: story / standard / operator.
// ---------------------------------------------------------------------------
pub const STORY_ARRIVAL: (f64, f64) = (24.0, 31.0);
pub const STORY_PLAYER_SPEED: f64 = 172.0;
pub const STORY_SPRINT: f64 = 250.0;
pub const STORY_BILLY_SPEED: f64 = 77.0;
pub const STORY_BILLY_SIGHT: f64 = 210.0;
pub const STORY_SUPPORT_LIMIT: f64 = 7.2;
pub const STORY_BANDWIDTH_REGEN: f64 = 8.0;
pub const STORY_BADGE_DELAY: f64 = 1.65;
pub const STORY_USB_TIMER: f64 = 14.0;
pub const STORY_CAMERA_LOCK: f64 = 1.05;
pub const STORY_ALERT_GAIN: f64 = 0.78;
pub const STORY_SCORE_MULT: f64 = 0.85;
pub const STORY_RESCUE: bool = true;
/// Trace score at which a session is traced. Higher is more forgiving.
pub const STORY_TRACE_THRESHOLD: u32 = 900;

pub const STANDARD_ARRIVAL: (f64, f64) = (19.0, 26.0);
pub const STANDARD_PLAYER_SPEED: f64 = 166.0;
pub const STANDARD_SPRINT: f64 = 242.0;
pub const STANDARD_BILLY_SPEED: f64 = 91.0;
pub const STANDARD_BILLY_SIGHT: f64 = 245.0;
pub const STANDARD_SUPPORT_LIMIT: f64 = 5.2;
pub const STANDARD_BANDWIDTH_REGEN: f64 = 6.1;
pub const STANDARD_BADGE_DELAY: f64 = 1.32;
pub const STANDARD_USB_TIMER: f64 = 11.0;
pub const STANDARD_CAMERA_LOCK: f64 = 0.78;
pub const STANDARD_ALERT_GAIN: f64 = 1.0;
pub const STANDARD_SCORE_MULT: f64 = 1.0;
pub const STANDARD_RESCUE: bool = true;
pub const STANDARD_TRACE_THRESHOLD: u32 = 600;

pub const OPERATOR_ARRIVAL: (f64, f64) = (15.0, 21.0);
pub const OPERATOR_PLAYER_SPEED: f64 = 162.0;
pub const OPERATOR_SPRINT: f64 = 235.0;
pub const OPERATOR_BILLY_SPEED: f64 = 106.0;
pub const OPERATOR_BILLY_SIGHT: f64 = 280.0;
pub const OPERATOR_SUPPORT_LIMIT: f64 = 3.8;
pub const OPERATOR_BANDWIDTH_REGEN: f64 = 4.5;
pub const OPERATOR_BADGE_DELAY: f64 = 1.02;
pub const OPERATOR_USB_TIMER: f64 = 8.5;
pub const OPERATOR_CAMERA_LOCK: f64 = 0.58;
pub const OPERATOR_ALERT_GAIN: f64 = 1.2;
pub const OPERATOR_SCORE_MULT: f64 = 1.25;
pub const OPERATOR_RESCUE: bool = false;
pub const OPERATOR_TRACE_THRESHOLD: u32 = 400;

//## Net View direct-hack
// Original addition -- not ported from the HTML prototype; every other
// constant in this file is. Flagged for review: this is a new capability
// (hacking e.g. the substation directly was previously reachable only from a
// raw test call, never from any Command), and these two numbers have no
// canonical source to check against. Starting point is parity with the most
// expensive existing uplink action (Vacuum).
pub const NET_HACK_COST: f64 = 25.0;
pub const NET_HACK_COOLDOWN: f64 = 8.0;

// ---------------------------------------------------------------------------
// Outside the building (a floor with an `ExteriorDef`).
// ---------------------------------------------------------------------------
/// A room's height, floor line to ceiling.
pub const ROOM_H: f64 = 150.0;
/// Canvas y of the roof's walking surface: the top of the roof slab above
/// the ceiling line.
pub const ROOF_Y: f64 = FLOOR - ROOM_H - 21.0;
/// The parapet at the roof's far end: its width (it stops the body) and how
/// far it stands above the roof.
pub const PARAPET_W: f64 = 8.0;
pub const PARAPET_H: f64 = 26.0;
/// How close (body centre) to the rope's top or foot to take hold of it.
pub const ROPE_REACH: f64 = 20.0;
/// Abseiling and climbing speed, canvas units a second.
pub const ROPE_SPEED: f64 = 120.0;
/// How close (body centre) to a hatch to drop through it.
pub const HATCH_REACH: f64 = 18.0;
/// How close to the front door, from the street, to pick its lock.
pub const PICK_REACH: f64 = 26.0;
/// Seconds of picking that open the front door.
pub const PICK_TIME: f64 = 4.0;
/// The noise lockpicking makes (a scratch, not a bang).
pub const NOISE_PICK: f64 = 0.3;
/// The hacker's support outside: the van is in plain sight.
pub const EXTERIOR_SUPPORT: f64 = 0.9;
/// The street's far end (body left edge), well beyond the front door.
pub const STREET_LO: f64 = -230.0;

// ---------------------------------------------------------------------------
// Infrastructure consequences (a floor with an `InfraDef`).
// ---------------------------------------------------------------------------
//## Infrastructure
// Original addition — not ported from the HTML prototype. These numbers have
// no canonical source to check against; they are the first tuning pass for
// the water/failure/alarm layer and are flagged for review exactly as the
// Net View constants above are. A floor without an `InfraDef` reads none of
// them, so every existing replay is untouched.
/// Water level (0..=1) at which a flooding zone shorts the room's electrics.
pub const WATER_SHORT_AT: f64 = 0.55;
/// Water level (0..=1) at which wading starts to slow a body and splash.
pub const WATER_WADE_AT: f64 = 0.15;
/// Speed multiplier for a body wading through water: flat across the whole
/// wade band, from `WATER_WADE_AT` to full depth.
pub const WATER_WADE_SPEED: f64 = 0.62;
/// The noise floor wading sets on the player's own noise meter while moving
/// (above `NOISE_INVEST_NOISE`, so splashing betrays a wader to Billy's
/// existing hearing — the guard reacts to the noise, not to the choice).
pub const WATER_WADE_NOISE: f64 = 0.62;
/// How loud a zone's fill splashes, as a noise intensity on the same scale as
/// the player's noise meter, while the level is still climbing.
pub const WATER_SPLASH_NOISE: f64 = 0.7;
/// Seconds a shorted room's electrics spark and crack (a seen/heard failure).
pub const WATER_SHORT_FLASH: f64 = 0.6;
/// Water level (0..=1) at which a water-triggered alarm trips.
pub const ALARM_WATER_TRIP_LEVEL: f64 = 0.25;
/// Alert units per second a sounding alarm adds (scaled by the difficulty
/// preset's `alert_gain`, exactly as `CALL_ALERT` is). A sustained flood
/// alarm is pressure with a deadline: it climbs the alert toward the
/// lockdown band, but slowly enough that a run in progress can finish.
pub const ALARM_ALERT_RATE: f64 = 0.9;
/// How loud a sounding alarm is, as a noise intensity: above every archetype's
/// `invest_noise`, and heard floor-wide (no distance gate).
pub const ALARM_NOISE: f64 = 1.0;
/// How loud a sparking short is, as a noise intensity (distance-gated like
/// every other source).
pub const SHORT_FLASH_NOISE: f64 = 0.8;
/// How close (world units, body centre) the infiltrator stands to reach a
/// zone's manual valve handle.
pub const VALVE_DIST: f64 = 40.0;
/// Seconds of held interact that turn a manual handle and release its zone.
pub const VALVE_TURN_HOLD: f64 = 1.5;

//## Moletaire hosting (a floor with a `MoleHostDef`).
// Original addition — the companion module is a port, but hosting it on a
// scenario floor is new. Flagged for review like the blocks above.
/// How loud the mole's synth diversion is, as a noise intensity: above
/// Billy's `invest_noise`, so the existing hearing gate takes it seriously.
pub const MOLE_SYNTH_NOISE: f64 = 0.8;
/// Seconds a synth diversion keeps pulling attention after it plays.
pub const MOLE_SYNTH_DURATION: f64 = 2.2;
/// How loud the mole digging or chewing a cable is (quieter than the synth:
/// it carries, but not across the floor).
pub const MOLE_DIG_NOISE: f64 = 0.5;
/// How close (world units) a sabotaged cable takes a device node down: the
/// nearest light, camera or door node to the mole inside this radius loses
/// power through the ordinary `DevicePowerLost` path.
pub const MOLE_SABOTAGE_RADIUS: f64 = 90.0;
/// The mole's hosted seed is the run seed folded with this constant, so its
/// RNG stream is its own and the run's draw counts never move.
pub const MOLE_SEED_FOLD: u32 = 0x4D4F_4C45;

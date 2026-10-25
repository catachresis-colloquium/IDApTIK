//! The canonical "Envelope 001 – Ghost Lobby" scenario, built by projecting
//! [`crate::scenario::constants`] into a [`ScenarioDefinition`]. This is the
//! single place that turns the compile-time constant audit into declarative
//! data; `json_roundtrip` asserts the projection stays faithful.

use crate::scenario::constants as c;
use crate::scenario::definition::{
    BillyDef, CameraDef, DoorDef, HideSpotDef, ObjectiveDef, ObjectiveKind, PlayerDef, PropSpawn,
    PropsDef, RoomDef, ScenarioDefinition, SpawnRanges, WorldDef,
};
use crate::scenario::tuning::{
    ActionKind, ActionSpec, ActionTiming, BandwidthTuning, BeliefTuning, CameraTuning,
    DifficultyId, DifficultyPreset, FsmTuning, GeometryTuning, GradeBands, InteractionTuning,
    MovementTuning, NoiseTuning, RescueTuning, ScoringDef, SupportTuning, TuningConstants,
    UsbTuning, VacuumTuning,
};
use std::collections::BTreeMap;

/// The scenario format tag (definition surface). v2: the runtime the definition
/// drives grew agent sessions, camera loops and dead nodes (the grounded
/// network fusion), so the surface family is bumped in step.
pub const FORMAT: &str = "idaptik-ghost-lobby-v2";
/// The stable scenario id.
pub const SCENARIO_ID: &str = "envelope-001-ghost-lobby";

/// A committed pretty-printed JSON of [`ghost_lobby`]. Regenerate with the
/// ignored `regenerate_golden_json` test if the definition ever changes; the
/// `json_roundtrip` test proves it parses back equal to [`ghost_lobby`].
pub const GHOST_LOBBY_JSON: &str =
    include_str!("../../../../contracts/idaptik/v1/fixtures/ghost-lobby-scenario.json");

fn tuning() -> TuningConstants {
    TuningConstants {
        geometry: GeometryTuning {
            floor: c::FLOOR,
            room_offset: c::ROOM_OFFSET,
            player_w: c::PLAYER_W,
            player_h: c::PLAYER_H,
            billy_w: c::BILLY_W,
            billy_h: c::BILLY_H,
            collision_radius: c::COLLISION_RADIUS,
            world_width: c::WORLD_WIDTH,
        },
        movement: MovementTuning {
            gravity: c::GRAVITY,
            jump_vy: c::JUMP_VY,
            accel_drive: c::ACCEL_DRIVE,
            accel_brake: c::ACCEL_BRAKE,
            crouch_speed: c::CROUCH_SPEED,
            stam_drain: c::STAM_DRAIN,
            stam_regen: c::STAM_REGEN,
            stam_regen_crouch: c::STAM_REGEN_CROUCH,
            sprint_min_stam: c::SPRINT_MIN_STAM,
            hide_max_vx: c::HIDE_MAX_VX,
            crouch_cam_vx: c::CROUCH_CAM_VX,
            moving_vx: c::MOVING_VX,
            clamp_lo: c::PLAYER_CLAMP_LO,
            clamp_hi: c::PLAYER_CLAMP_HI,
            land_vy: c::LAND_VY,
        },
        noise: NoiseTuning {
            jump: c::NOISE_JUMP,
            land: c::NOISE_LAND,
            decay: c::NOISE_DECAY,
            sprint_base: c::NOISE_SPRINT_BASE,
            sprint_k: c::NOISE_SPRINT_K,
            walk_k: c::NOISE_WALK_K,
            crouch: c::NOISE_CROUCH,
            move_min_vx: c::NOISE_MOVE_MIN_VX,
        },
        fsm: FsmTuning {
            start_x: c::BILLY_START_X,
            crisis_x: c::CRISIS_BILLY_X,
            enter_speed: c::ENTER_SPEED,
            shock_t: c::SHOCK_T,
            assess_speed: c::ASSESS_SPEED,
            patrol_lo: c::PATROL_LO,
            patrol_hi: c::PATROL_HI,
            patrol_pivot: c::PATROL_PIVOT,
            invest_speed: c::INVEST_SPEED,
            secure_speed: c::SECURE_SPEED,
            grab_dist: c::GRAB_DIST,
            guard_speed: c::GUARD_SPEED,
            guard_t_note: c::GUARD_T_NOTE,
            guard_t_usb: c::GUARD_T_USB,
            call_t: c::CALL_T,
            pursue_speed: c::PURSUE_SPEED,
            pursue_trigger: c::PURSUE_TRIGGER,
            pursue_giveup_ago: c::PURSUE_GIVEUP_AGO,
            pursue_giveup_dist: c::PURSUE_GIVEUP_DIST,
            accel: c::BILLY_ACCEL,
            clamp_lo: c::BILLY_CLAMP_LO,
            clamp_hi: c::BILLY_CLAMP_HI,
            badge_open: c::BADGE_OPEN,
            snack_reach: c::SNACK_REACH,
            alert_boost_div: c::ALERT_BOOST_DIV,
            lights_boost_k: c::LIGHTS_BOOST_K,
            vac_distract: c::VAC_DISTRACT,
            sight_back: c::SIGHT_BACK,
            sight_crouch: c::SIGHT_CROUCH,
            sight_office: c::SIGHT_OFFICE,
            sight_alert: c::SIGHT_ALERT,
            sight_alert_hi: c::SIGHT_ALERT_HI,
            invest_noise: c::NOISE_INVEST_NOISE,
            invest_dist: c::NOISE_INVEST_DIST,
        },
        belief: BeliefTuning {
            threshold: c::BELIEF_THRESHOLD,
            misdirect_threshold: c::MISDIRECT_THRESHOLD,
            moving_vx: c::MOVING_VX,
            pi_seen: c::PI_SEEN,
            pi_sprint: c::PI_SPRINT,
            note_near: c::NOTE_NEAR,
            usb_near: c::USB_NEAR,
            note_urg_sprint: c::NOTE_URG_SPRINT,
            note_urg_move: c::NOTE_URG_MOVE,
            note_urg_still: c::NOTE_URG_STILL,
            usb_urg_sprint: c::USB_URG_SPRINT,
            usb_urg_move: c::USB_URG_MOVE,
            usb_urg_still: c::USB_URG_STILL,
            note_carry: c::NOTE_CARRY,
            usb_carry: c::USB_CARRY,
            prog_gate: c::PROG_GATE,
            pi_decay: c::PI_DECAY,
            note_decay: c::NOTE_DECAY,
            usb_decay: c::USB_DECAY,
            peel_interest: c::PEEL_INTEREST,
            secure_note_seen: c::SECURE_NOTE_SEEN,
            take_usb_seen: c::TAKE_USB_SEEN,
            take_usb_unseen: c::TAKE_USB_UNSEEN,
            camera_player_int: c::CAMERA_PLAYER_INT,
        },
        support: SupportTuning {
            ping_hall: c::SUPPORT_PING_HALL,
            ping_office: c::SUPPORT_PING_OFFICE,
            ping_laundry: c::SUPPORT_PING_LAUNDRY,
            ping_exit: c::SUPPORT_PING_EXIT,
            lowbw_x: c::SUPPORT_LOWBW_X,
            lowbw_pen: c::SUPPORT_LOWBW_PEN,
            alert_pen: c::SUPPORT_ALERT_PEN,
            hidden: c::SUPPORT_HIDDEN,
            flicker: c::SUPPORT_FLICKER,
            hop_pen: c::SUPPORT_HOP_PEN,
            clamp_min: c::SUPPORT_CLAMP_MIN,
            approach: c::SUPPORT_APPROACH,
            iso_gate: c::ISO_GATE,
            iso_pressure_div: c::ISO_PRESSURE_DIV,
            fray_frac: c::FRAY_FRAC,
            iso_decay: c::ISO_DECAY,
            iso_decay_hidden: c::ISO_DECAY_HIDDEN,
            ping_dur: c::PING_DUR,
        },
        bandwidth: BandwidthTuning {
            regen_a: c::BW_REGEN_A,
            regen_b: c::BW_REGEN_B,
            alert_decay_quiet: c::ALERT_DECAY_QUIET,
            alert_decay_crisis: c::ALERT_DECAY_CRISIS,
            lockdown: c::LOCKDOWN,
        },
        action: ActionTiming {
            lights_dur: c::LIGHTS_DUR,
            lights_dur_rm: c::LIGHTS_DUR_RM,
            lights_stun: c::LIGHTS_STUN,
            route_duration: c::ROUTE_DURATION,
            throttle_cdbw: c::THROTTLE_CDBW,
            throttle_fray: c::THROTTLE_FRAY,
            throttle_badge: c::THROTTLE_BADGE,
            call_alert: c::CALL_ALERT,
        },
        camera: CameraTuning {
            sweep_w: c::CAM_SWEEP_W,
            sweep_a: c::CAM_SWEEP_A,
            lockout: c::CAM_LOCKOUT,
            alert: c::CAM_ALERT,
            decay_unseen: c::CAM_DECAY_UNSEEN,
            decay_off: c::CAM_DECAY_OFF,
        },
        usb: UsbTuning {
            throw_vx: c::USB_THROW_VX,
            throw_vy: c::USB_THROW_VY,
            throw_off: c::USB_THROW_OFF,
            held_off: c::USB_HELD_OFF,
            grav: c::USB_GRAV,
            drag: c::USB_DRAG,
            floor: c::USB_FLOOR,
            bounce: c::USB_BOUNCE,
            friction: c::USB_FRICTION,
            rest_vx: c::USB_REST_VX,
            rest_vy: c::USB_REST_VY,
            clamp_lo: c::USB_CLAMP_LO,
            clamp_hi: c::USB_CLAMP_HI,
            wipe_alert: c::USB_WIPE_ALERT,
            throw_alert: c::USB_THROW_ALERT,
        },
        vacuum: VacuumTuning {
            lag_x: c::VAC_LAG_X,
            ctrl_loss: c::VAC_CTRL_LOSS,
            ctrl_min: c::VAC_CTRL_MIN,
            lag_warn: c::VAC_LAG_WARN,
            ctrl_gain: c::VAC_CTRL_GAIN,
            speed: c::VAC_SPEED,
            move_min: c::VAC_MOVE_MIN,
            target: c::VAC_TARGET,
            fall_off: c::VAC_FALL_OFF,
        },
        interaction: InteractionTuning {
            note_dist: c::NOTE_DIST,
            note_hold: c::NOTE_HOLD,
            usb_dist: c::USB_DIST,
            chute_enter: c::CHUTE_ENTER,
            chute_search_off: c::CHUTE_SEARCH_OFF,
            chute_search_dist: c::CHUTE_SEARCH_DIST,
            chute_hold: c::CHUTE_HOLD,
            pickpocket_dist: c::PICKPOCKET_DIST,
            pickpocket_hold: c::PICKPOCKET_HOLD,
            exit_prompt_x: c::EXIT_PROMPT_X,
            exit_x: c::EXIT_X,
        },
        rescue: RescueTuning {
            support_min: c::RESCUE_SUPPORT_MIN,
            bw_min: c::RESCUE_BW_MIN,
            bw_cost: c::RESCUE_BW_COST,
            stun: c::RESCUE_STUN,
            grace: c::RESCUE_GRACE,
            displace: c::RESCUE_DISPLACE,
        },
    }
}

fn scoring() -> ScoringDef {
    ScoringDef {
        base: c::SCORE_BASE,
        note: c::SCORE_NOTE,
        note_lost: c::SCORE_NOTE_LOST,
        note_none: c::SCORE_NOTE_NONE,
        misdir: c::SCORE_MISDIR,
        misdir_leak: c::SCORE_MISDIR_LEAK,
        noboss: c::SCORE_NOBOSS,
        boss: c::SCORE_BOSS,
        nocam: c::SCORE_NOCAM,
        cam_each: c::SCORE_CAM_EACH,
        iso_ok: c::SCORE_ISO_OK,
        iso_snap: c::SCORE_ISO_SNAP,
        iso_snap_frac: c::SCORE_ISO_SNAP_FRAC,
        chute: c::SCORE_CHUTE,
        usbtrace: c::SCORE_USBTRACE,
        rescue: c::SCORE_RESCUE,
        time_base: c::SCORE_TIME_BASE,
        time_k: c::SCORE_TIME_K,
        alert_k: c::SCORE_ALERT_K,
        fail_act: c::SCORE_FAIL_ACT,
        fail_note: c::FAIL_NOTE,
        fail_base: c::FAIL_BASE,
        fail_usb: c::FAIL_USB,
        fail_alert_k: c::FAIL_ALERT_K,
        grades: GradeBands {
            s: c::GRADE_S,
            a: c::GRADE_A,
            b: c::GRADE_B,
            c: c::GRADE_C,
            fail_c: c::GRADE_FAIL_C,
        },
    }
}

fn actions() -> BTreeMap<ActionKind, ActionSpec> {
    BTreeMap::from([
        (
            ActionKind::Camera,
            ActionSpec {
                cost: c::ACT_CAMERA_COST,
                cooldown: c::ACT_CAMERA_CD,
                alert_gain: c::ACT_CAMERA_ALERT,
            },
        ),
        (
            ActionKind::Door,
            ActionSpec {
                cost: c::ACT_DOOR_COST,
                cooldown: c::ACT_DOOR_CD,
                alert_gain: c::ACT_DOOR_ALERT,
            },
        ),
        (
            ActionKind::Vacuum,
            ActionSpec {
                cost: c::ACT_VACUUM_COST,
                cooldown: c::ACT_VACUUM_CD,
                alert_gain: c::ACT_VACUUM_ALERT,
            },
        ),
        (
            ActionKind::Lights,
            ActionSpec {
                cost: c::ACT_LIGHTS_COST,
                cooldown: c::ACT_LIGHTS_CD,
                alert_gain: c::ACT_LIGHTS_ALERT,
            },
        ),
    ])
}

fn difficulty() -> BTreeMap<DifficultyId, DifficultyPreset> {
    BTreeMap::from([
        (
            DifficultyId::Story,
            DifficultyPreset {
                label: "STORY".into(),
                arrival: c::STORY_ARRIVAL,
                player_speed: c::STORY_PLAYER_SPEED,
                sprint: c::STORY_SPRINT,
                billy_speed: c::STORY_BILLY_SPEED,
                billy_sight: c::STORY_BILLY_SIGHT,
                support_limit: c::STORY_SUPPORT_LIMIT,
                bandwidth_regen: c::STORY_BANDWIDTH_REGEN,
                badge_delay: c::STORY_BADGE_DELAY,
                usb_timer: c::STORY_USB_TIMER,
                camera_lock: c::STORY_CAMERA_LOCK,
                alert_gain: c::STORY_ALERT_GAIN,
                score_mult: c::STORY_SCORE_MULT,
                rescue: c::STORY_RESCUE,
                trace_threshold: c::STORY_TRACE_THRESHOLD,
            },
        ),
        (
            DifficultyId::Standard,
            DifficultyPreset {
                label: "STANDARD".into(),
                arrival: c::STANDARD_ARRIVAL,
                player_speed: c::STANDARD_PLAYER_SPEED,
                sprint: c::STANDARD_SPRINT,
                billy_speed: c::STANDARD_BILLY_SPEED,
                billy_sight: c::STANDARD_BILLY_SIGHT,
                support_limit: c::STANDARD_SUPPORT_LIMIT,
                bandwidth_regen: c::STANDARD_BANDWIDTH_REGEN,
                badge_delay: c::STANDARD_BADGE_DELAY,
                usb_timer: c::STANDARD_USB_TIMER,
                camera_lock: c::STANDARD_CAMERA_LOCK,
                alert_gain: c::STANDARD_ALERT_GAIN,
                score_mult: c::STANDARD_SCORE_MULT,
                rescue: c::STANDARD_RESCUE,
                trace_threshold: c::STANDARD_TRACE_THRESHOLD,
            },
        ),
        (
            DifficultyId::Operator,
            DifficultyPreset {
                label: "OPERATOR".into(),
                arrival: c::OPERATOR_ARRIVAL,
                player_speed: c::OPERATOR_PLAYER_SPEED,
                sprint: c::OPERATOR_SPRINT,
                billy_speed: c::OPERATOR_BILLY_SPEED,
                billy_sight: c::OPERATOR_BILLY_SIGHT,
                support_limit: c::OPERATOR_SUPPORT_LIMIT,
                bandwidth_regen: c::OPERATOR_BANDWIDTH_REGEN,
                badge_delay: c::OPERATOR_BADGE_DELAY,
                usb_timer: c::OPERATOR_USB_TIMER,
                camera_lock: c::OPERATOR_CAMERA_LOCK,
                alert_gain: c::OPERATOR_ALERT_GAIN,
                score_mult: c::OPERATOR_SCORE_MULT,
                rescue: c::OPERATOR_RESCUE,
                trace_threshold: c::OPERATOR_TRACE_THRESHOLD,
            },
        ),
    ])
}

fn rooms() -> Vec<RoomDef> {
    vec![
        RoomDef {
            id: "kitchen".into(),
            name: "KITCHEN / BREAK ROOM".into(),
            x: 20.0,
            w: 250.0,
            support: c::ROOM_KITCHEN_SUPPORT,
            ping_support_bonus: 0.0,
            sight_multiplier: 1.0,
            lit: false,
        },
        RoomDef {
            id: "hall".into(),
            name: "HALLWAY".into(),
            x: 270.0,
            w: 240.0,
            support: c::ROOM_HALL_SUPPORT,
            ping_support_bonus: c::SUPPORT_PING_HALL,
            sight_multiplier: 1.0,
            lit: false,
        },
        RoomDef {
            id: "office".into(),
            name: "LIT OFFICE / USB TRAP".into(),
            x: 510.0,
            w: 285.0,
            support: c::ROOM_OFFICE_SUPPORT,
            ping_support_bonus: c::SUPPORT_PING_OFFICE,
            sight_multiplier: c::SIGHT_OFFICE,
            lit: true,
        },
        RoomDef {
            id: "laundry".into(),
            name: "LAUNDRY / WARDROBE".into(),
            x: 795.0,
            w: 245.0,
            support: c::ROOM_LAUNDRY_SUPPORT,
            ping_support_bonus: c::SUPPORT_PING_LAUNDRY,
            sight_multiplier: 1.0,
            lit: false,
        },
        RoomDef {
            id: "exit".into(),
            name: "SERVICE EXIT".into(),
            x: 1040.0,
            w: 220.0,
            support: c::ROOM_EXIT_SUPPORT,
            ping_support_bonus: c::SUPPORT_PING_EXIT,
            sight_multiplier: 1.0,
            lit: false,
        },
    ]
}

fn doors() -> Vec<DoorDef> {
    vec![
        DoorDef {
            id: "D1".into(),
            x: 270.0,
            label: "KITCHEN/HALL".into(),
        },
        DoorDef {
            id: "D2".into(),
            x: 510.0,
            label: "HALL/OFFICE".into(),
        },
        DoorDef {
            id: "D3".into(),
            x: 795.0,
            label: "OFFICE/LAUNDRY".into(),
        },
        DoorDef {
            id: "D4".into(),
            x: 1040.0,
            label: "SERVICE EXIT".into(),
        },
    ]
}

fn hide_spots() -> Vec<HideSpotDef> {
    vec![
        HideSpotDef {
            id: "counter".into(),
            room: "kitchen".into(),
            x: 205.0,
            radius: 58.0,
            label: "prep counter shadow".into(),
        },
        HideSpotDef {
            id: "copier".into(),
            room: "hall".into(),
            x: 404.0,
            radius: 50.0,
            label: "copier recess".into(),
        },
        HideSpotDef {
            id: "desk".into(),
            room: "office".into(),
            x: 655.0,
            radius: 62.0,
            label: "office desk".into(),
        },
        HideSpotDef {
            id: "wardrobe".into(),
            room: "laundry".into(),
            x: 885.0,
            radius: 58.0,
            label: "wardrobe".into(),
        },
        HideSpotDef {
            id: "crate".into(),
            room: "exit".into(),
            x: 1125.0,
            radius: 50.0,
            label: "delivery crate".into(),
        },
    ]
}

fn cameras() -> Vec<CameraDef> {
    vec![
        CameraDef {
            id: "cam-hall".into(),
            room: "hall".into(),
            x: 330.0,
            range: (292.0, 495.0),
            phase: 0.2,
            stale: false,
        },
        CameraDef {
            id: "cam-office".into(),
            room: "office".into(),
            x: 565.0,
            range: (530.0, 782.0),
            phase: 1.3,
            stale: false,
        },
        CameraDef {
            id: "cam-laundry".into(),
            room: "laundry".into(),
            x: 830.0,
            range: (812.0, 1015.0),
            phase: 2.1,
            stale: true,
        },
    ]
}

fn objectives() -> Vec<ObjectiveDef> {
    vec![
        ObjectiveDef {
            id: "note".into(),
            kind: ObjectiveKind::Note,
            label: "Secure the contact note".into(),
            room: Some("kitchen".into()),
        },
        ObjectiveDef {
            id: "misdirect".into(),
            kind: ObjectiveKind::Misdirect,
            label: "Convince Billy it was the drive".into(),
            room: Some("office".into()),
        },
        ObjectiveDef {
            id: "exit".into(),
            kind: ObjectiveKind::Exit,
            label: "Reach the service exit or laundry chute".into(),
            room: Some("exit".into()),
        },
    ]
}

/// Build the canonical Ghost Lobby scenario definition.
pub fn ghost_lobby() -> ScenarioDefinition {
    ScenarioDefinition {
        format: FORMAT.to_owned(),
        scenario_id: SCENARIO_ID.to_owned(),
        name: "Envelope 001 — Ghost Lobby".to_owned(),
        floor_id: 0,
        world: WorldDef {
            floor: c::FLOOR,
            room_offset: c::ROOM_OFFSET,
            width: c::WORLD_WIDTH,
        },
        rooms: rooms(),
        doors: doors(),
        hide_spots: hide_spots(),
        cameras: cameras(),
        props: PropsDef {
            note: PropSpawn {
                x: c::SPAWN_NOTE_X_BASE,
                y: c::NOTE_Y,
            },
            usb: PropSpawn {
                x: c::SPAWN_USB_X_BASE,
                y: c::USB_Y,
            },
            chute: PropSpawn {
                x: c::CHUTE_X,
                y: c::CHUTE_Y,
            },
            vacuum: PropSpawn {
                x: c::VACUUM_X,
                y: c::VACUUM_Y,
            },
        },
        player: PlayerDef {
            spawn_x: c::PLAYER_SPAWN_X,
            spawn_y: c::PLAYER_SPAWN_Y,
            w: c::PLAYER_W,
            h: c::PLAYER_H,
        },
        billy: BillyDef {
            spawn_x: c::BILLY_SPAWN_X,
            spawn_y: c::BILLY_SPAWN_Y,
            w: c::BILLY_W,
            h: c::BILLY_H,
        },
        objectives: objectives(),
        actions: actions(),
        difficulty: difficulty(),
        tuning: tuning(),
        scoring: scoring(),
        spawn: SpawnRanges {
            note_x: (c::SPAWN_NOTE_X_BASE, c::SPAWN_NOTE_X_SPAN),
            usb_x: (c::SPAWN_USB_X_BASE, c::SPAWN_USB_X_SPAN),
            stale_pulse: (c::SPAWN_STALE_BASE, c::SPAWN_STALE_SPAN),
            snack_x: (c::SPAWN_SNACK_BASE, c::SPAWN_SNACK_SPAN),
            door_delay: (c::SPAWN_DOOR_DELAY_BASE, c::SPAWN_DOOR_DELAY_SPAN),
            operator_door_penalty: c::OPERATOR_DOOR_PENALTY,
        },
        signal: None,
        arrival_clock: crate::scenario::definition::ArrivalClock::RunStart,
        consoles: Vec::new(),
        exterior: None,
        infra: None,
        mole: None,
        // The canonical floor has no power network: every device is simply as
        // alive as the signal graph says, exactly as before ADR-0013.
        power: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rewrites the committed golden JSON from the current definition. Ignored
    /// by default; run explicitly after an intentional definition change:
    /// `cargo test -p idaptik-core regenerate_golden_json -- --ignored`.
    #[test]
    #[ignore = "regenerates the committed golden; run manually"]
    fn regenerate_golden_json() {
        let def = ghost_lobby();
        let json = serde_json::to_string_pretty(&def).expect("serialize");
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../contracts/idaptik/v1/fixtures/ghost-lobby-scenario.json"
        );
        std::fs::write(path, json.as_bytes()).expect("write golden json");
    }

    /// Round-trips the Nickel-authored scenario (config/ghost_lobby_scenario.ncl)
    /// through serde and the validator: the export must be a definition
    /// `validate()` accepts, equal to the Rust-authored [`ghost_lobby`].
    /// Numeric formatting is the one normalisation (Nickel prints `585`, serde
    /// `585.0`); parsing both sides into [`ScenarioDefinition`] absorbs it.
    /// Ignored because it needs a prior `nickel export`; run via
    /// `just config-check`, which seeds /tmp/ghost_lobby_scenario_raw.json.
    #[test]
    #[ignore = "needs /tmp/ghost_lobby_scenario_raw.json from nickel export; run via just config-check"]
    fn nickel_scenario_round_trips_and_validates() {
        let raw =
            std::fs::read_to_string("/tmp/ghost_lobby_scenario_raw.json").expect("nickel export");
        let def: ScenarioDefinition = serde_json::from_str(&raw).expect("parse nickel json");
        let report = def.validate();
        assert!(report.passed(), "validate() rejected: {:?}", report.ok());
        assert_eq!(
            def,
            ghost_lobby(),
            "the Nickel-authored scenario drifted from the Rust-authored definition"
        );
    }
}

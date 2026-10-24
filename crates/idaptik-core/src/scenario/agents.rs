//! The two players' network sessions, as game state.
//!
//! Both players are the same kind of peer on the same floor graph; nothing here
//! grants either one a power the other lacks. Only their vantages differ: the
//! hacker plays from the van, the infiltrator from the room they stand in. The
//! sessions are pure serde data so they snapshot and restore with the rest of
//! [`crate::scenario::state::RuntimeState`].
use crate::netsim::effect::Effect;
use crate::netsim::graph::GroundedGraph;
use crate::netsim::session::AgentSession;
use crate::scenario::command::RunConfig;
use crate::scenario::constants as c;
use crate::scenario::definition::ScenarioDefinition;
use crate::scenario::event::Event;
use crate::scenario::floor_graph::{
    camera_node_id, door_node_id, inside_vantage, light_node_id, pump_node_id, valve_node_id,
    van_vantage,
};
use crate::scenario::infra::ReleaseCause;
use crate::scenario::sim::GhostLobbySim;
use crate::scenario::state::RuntimeState;
use serde::{Deserialize, Serialize};

/// The two symmetric peers on the floor graph. Nothing here distinguishes their
/// powers; only their vantages differ.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agents {
    pub hacker: AgentSession,
    pub infiltrator: AgentSession,
}

impl Agents {
    /// Open both sessions: the hacker from the van, the infiltrator from
    /// `start_room`. A room with no `Inside` vantage falls back to the van rather
    /// than panicking, since `RuntimeState::initial` is driven by the fuzz test.
    pub fn initial(graph: &GroundedGraph, start_room: &str, trace_threshold: u32) -> Self {
        Self {
            hacker: AgentSession::new(van_vantage(graph), trace_threshold),
            infiltrator: AgentSession::new(
                inside_vantage(graph, start_room).unwrap_or_else(|| van_vantage(graph)),
                trace_threshold,
            ),
        }
    }
}

//## Applying network effects to the world

/// The index of the camera `id` names, if it names one.
fn camera_index(def: &ScenarioDefinition, id: &str) -> Option<usize> {
    (0..def.cameras.len()).find(|i| camera_node_id(*i) == id)
}

/// Whether `id` names a room's lights.
fn is_light_node(def: &ScenarioDefinition, id: &str) -> bool {
    def.rooms.iter().any(|r| light_node_id(r.id.as_str()) == id)
}

/// Release a water zone and report it, if the floor carries the zone and it
/// is still dry. The node id is matched against the derived valve/pump ids
/// rather than string-surgery'd back into a zone, exactly as `DoorHeld`
/// recovers its door. Returns the event, or `None` when nothing was released
/// (no infrastructure, unknown node, or the zone is already flooding — the
/// first cause stands, as [`crate::scenario::infra::InfraState::release`]
/// guarantees).
fn release_zone_by_node(
    def: &ScenarioDefinition,
    state: &mut RuntimeState,
    node_id: &str,
    cause: ReleaseCause,
) -> Option<Event> {
    let infra_def = def.infra.as_ref()?;
    let infra = state.infra.as_mut()?;
    // The cause decides which derived node can release the zone: a hacked
    // valve releases through the valve; a pump failure releases only through
    // a fail-open pump. A valve that merely lost power stays shut.
    let zone = match cause {
        ReleaseCause::Valve => infra_def
            .water_zones
            .iter()
            .find(|z| valve_node_id(&z.id) == node_id)?,
        ReleaseCause::PumpFailure => infra_def
            .water_zones
            .iter()
            .find(|z| z.fail_open && pump_node_id(&z.id) == node_id)?,
    };
    infra
        .release(&zone.id, cause)
        .then(|| Event::WaterReleased {
            zone: zone.id.clone(),
            room: zone.room.clone(),
            cause,
        })
}

/// The lights-out window, honouring reduced motion exactly as `perform_action`
/// does: the flag lives on [`RunConfig`], not on the definition.
fn lights_duration(cfg: RunConfig) -> f64 {
    if cfg.reduced_motion {
        c::LIGHTS_DUR_RM
    } else {
        c::LIGHTS_DUR
    }
}

/// Darken the floor: the physical half of a lights-out, shared by the deliberate
/// kill and by a light losing power. It raises no alert and counts no use; the
/// caller adds those only when a hacker chose to flick the switch.
fn darken(state: &mut RuntimeState, cfg: RunConfig) {
    state.lights_flicker = lights_duration(cfg);
    state.billy.stun = state.billy.stun.max(c::LIGHTS_STUN);
}

/// Kill a node's power: it joins the dead nodes, and if it is a room's lights the
/// floor darkens exactly as a killed light darkens it. Nobody chose this one,
/// though, so no use is counted against the hacker. A camera needs nothing
/// further; the dead-node entry is the whole of its death.
fn unpower(def: &ScenarioDefinition, state: &mut RuntimeState, cfg: RunConfig, id: &str) {
    state.dead_nodes.insert(id.to_owned());
    if is_light_node(def, id) {
        darken(state, cfg);
    }
}

/// Apply the physical effects of a hack back into the scenario world.
/// Returns the events the effects raise, for the caller to emit.
///
/// This is deliberately blind to `alert`, `bandwidth` and the action cooldowns:
/// those are the price of *acting*, charged once by `perform_action`, not a
/// property of an effect landing. A power cascade that darkens a room raises no
/// alert of its own. It consumes no randomness, so the reset draw counts hold.
///
/// An unrecognised node id is a no-op rather than a panic: the fuzz test drives
/// this with ids no fixture answers to.
pub fn apply_effects(
    def: &ScenarioDefinition,
    state: &mut RuntimeState,
    cfg: RunConfig,
    effects: &[Effect],
) -> Vec<Event> {
    let mut events = Vec::new();
    for effect in effects {
        match effect {
            Effect::DoorHeld(id) => {
                // Recover the door by matching the node id against the helper that
                // minted it, rather than by cutting the prefix off the string.
                let Some(def_door) = def.doors.iter().find(|d| door_node_id(d.id.clone()) == *id)
                else {
                    continue;
                };
                let Some(door) = state.doors.iter_mut().find(|d| d.id == def_door.id) else {
                    continue;
                };
                door.pending = door.route_delay;
                door.badge_logged = false;
                let (door_id, delay) = (door.id.clone(), door.route_delay);
                state.stats.doors_held += 1;
                events.push(Event::DoorRouted {
                    door: door_id,
                    delay,
                });
            }
            Effect::CameraLooped(id) => {
                let Some(index) = camera_index(def, id) else {
                    continue;
                };
                let Some(slot) = state.camera_looped.get_mut(index) else {
                    continue;
                };
                *slot = c::CAM_LOOP_DUR;
                // Sitting inside the camera is intel as well as cover.
                state.camera_ping = c::PING_DUR;
                let laundry_view =
                    GhostLobbySim::room_id_at(def, state.player.x) == Some("laundry");
                events.push(Event::CameraPinged { laundry_view });
            }
            Effect::CameraDisabled(id) => {
                // A dead camera is recorded as a dead node, never as an infinite
                // loop: an infinity in `camera_looped` would break the clamp
                // invariants and serde.
                state.dead_nodes.insert(id.clone());
            }
            Effect::LightsKilled(_) => {
                darken(state, cfg);
                state.lights_uses += 1;
                state.stats.light_flickers += 1;
                events.push(Event::LightsFlickered {
                    third_use: state.lights_uses == 3,
                });
            }
            Effect::VacuumRun(_) => {
                // A vacuum down the chute is gone for good; re-routing it is not a
                // denial here, only nothing at all. `perform_action` owns the
                // denial event, because the denial is about the attempt.
                if state.vacuum.fallen {
                    continue;
                }
                state.vacuum.active = true;
                state.vacuum.target = c::VAC_TARGET;
                state.stats.vacuum_used = true;
                events.push(Event::VacuumRouted);
            }
            Effect::PowerCut(id) => {
                unpower(def, state, cfg, id);
                // The cascade is reported once, at the cut that caused it, rather
                // than once per device: a tally is what the hacker can read from a
                // van, and it is their only sign that the upstream line worked. A
                // cut that fells nothing has cascaded nowhere and says nothing.
                let nodes = effects
                    .iter()
                    .filter(|e| matches!(e, Effect::DevicePowerLost(_)))
                    .count();
                if nodes > 0 {
                    events.push(Event::PowerLost { nodes });
                }
            }
            Effect::DevicePowerLost(id) => {
                unpower(def, state, cfg, id);
                // A sump pump felled by the cascade fails its zone's valve
                // open — the consequence nobody ordered, arriving through the
                // ordinary power-loss path rather than any new channel.
                if let Some(event) = release_zone_by_node(def, state, id, ReleaseCause::PumpFailure)
                {
                    events.push(event);
                }
            }
            Effect::WaterReleased(id) => {
                // The deliberate flood: a hacked valve node releases its
                // zone. Recovered by matching the minted node id, never by
                // cutting a prefix off the string.
                if let Some(event) = release_zone_by_node(def, state, id, ReleaseCause::Valve) {
                    events.push(event);
                }
            }
            // The floor graph authors no node with these actuations, so nothing on
            // this floor can raise them. They are matched explicitly, and left to
            // do nothing, so that a future `Effect` variant is a compile error here
            // rather than a silent no-op.
            Effect::LockDisengaged(_) | Effect::ElevatorCalled(_) | Effect::SensorMuted(_) => {}
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::netsim::graph::VantageKind;
    use crate::scenario::floor_graph::{VACUUM_NODE_ID, floor_graph};
    use crate::scenario::ghost_lobby::ghost_lobby;
    use crate::scenario::rng::{Mulberry32, roll_init};
    use crate::scenario::tuning::DifficultyPreset;

    /// A quiet-phase start, built the way `GhostLobbySim::new` builds one. The
    /// roll draws from its own RNG, so it cannot disturb the sim's draw counts.
    fn start(cfg: RunConfig) -> (ScenarioDefinition, RuntimeState) {
        let def = ghost_lobby();
        let preset: DifficultyPreset = def
            .difficulty
            .get(&cfg.difficulty)
            .cloned()
            .expect("the definition carries every difficulty");
        let roll = roll_init(&def, &mut Mulberry32::new(123456), cfg.difficulty);
        let state = RuntimeState::initial(&def, &roll, cfg, &preset);
        (def, state)
    }

    /// The canonical default: Standard, full motion.
    fn standard() -> (ScenarioDefinition, RuntimeState) {
        start(RunConfig::standard())
    }

    #[test]
    fn the_two_agents_start_on_their_own_vantages() {
        let def = ghost_lobby();
        let g = floor_graph(&def);
        let a = Agents::initial(&g, "kitchen", 1000);
        assert_eq!(a.hacker.vantage().kind, VantageKind::Van);
        assert_eq!(a.infiltrator.vantage().kind, VantageKind::Inside);
        // The infiltrator owns the room they stand in; the hacker owns none of it.
        assert!(a.infiltrator.is_local(&g, &light_node_id("kitchen")));
        assert!(!a.hacker.is_local(&g, &light_node_id("kitchen")));
    }

    #[test]
    fn agents_round_trip_through_serde() {
        let def = ghost_lobby();
        let g = floor_graph(&def);
        let a = Agents::initial(&g, "kitchen", 1000);
        let json = serde_json::to_string(&a).expect("serialises");
        let back: Agents = serde_json::from_str(&json).expect("deserialises");
        assert_eq!(back, a);
    }

    #[test]
    fn an_unknown_start_room_falls_back_to_the_van() {
        // RuntimeState::initial must never panic, so a room with no Inside vantage
        // seats the infiltrator in the van rather than unwrapping a None.
        let def = ghost_lobby();
        let g = floor_graph(&def);
        let a = Agents::initial(&g, "no-such-room", 1000);
        assert_eq!(a.infiltrator.vantage().kind, VantageKind::Van);
    }

    #[test]
    fn holding_a_door_routes_the_matching_scenario_door() {
        let (def, mut state) = standard();
        let door = def.doors[0].id.clone();
        let effects = vec![Effect::DoorHeld(door_node_id(door.clone()))];
        let events = apply_effects(&def, &mut state, RunConfig::standard(), &effects);
        let d = state
            .doors
            .iter()
            .find(|d| d.id == door)
            .expect("the door exists");
        assert!(d.pending > 0.0, "the door must be routing");
        assert!(!d.badge_logged);
        assert_eq!(state.stats.doors_held, 1);
        assert!(matches!(events.as_slice(), [Event::DoorRouted { .. }]));
    }

    #[test]
    fn cutting_the_substation_darkens_the_floor_and_kills_the_cameras() {
        let (def, mut state) = standard();
        let g = floor_graph(&def);
        let effects = crate::netsim::apply_actuation(&g, "substation");
        apply_effects(&def, &mut state, RunConfig::standard(), &effects);
        assert!(state.dead_nodes.contains("substation"));
        assert!(
            state.dead_nodes.contains(&camera_node_id(0)),
            "cameras lose power"
        );
        assert!(state.lights_flicker > 0.0, "the floor goes dark");
        // The floor going dark is not the hacker flicking a switch: no use is
        // counted against them, and no alert is charged here.
        assert_eq!(state.lights_uses, 0);
        assert_eq!(state.alert, 0.0);
    }

    #[test]
    fn a_power_cut_reports_its_cascade_once_with_the_tally() {
        // The tally is the hacker's only sign that the upstream line landed, so it
        // must count every device the cut felled and be raised exactly once.
        let (def, mut state) = standard();
        let g = floor_graph(&def);
        let effects = crate::netsim::apply_actuation(&g, "substation");
        let felled = effects
            .iter()
            .filter(|e| matches!(e, Effect::DevicePowerLost(_)))
            .count();
        assert!(felled > 0, "the substation must actually cascade");
        let events = apply_effects(&def, &mut state, RunConfig::standard(), &effects);
        let tallies: Vec<usize> = events
            .iter()
            .filter_map(|e| match e {
                Event::PowerLost { nodes } => Some(*nodes),
                _ => None,
            })
            .collect();
        assert_eq!(tallies, vec![felled]);
    }

    #[test]
    fn a_cut_that_fells_nothing_reports_nothing() {
        // A cut with no dependents has cascaded nowhere; there is no news in it.
        let (def, mut state) = standard();
        let events = apply_effects(
            &def,
            &mut state,
            RunConfig::standard(),
            &[Effect::PowerCut("substation".into())],
        );
        assert!(
            state.dead_nodes.contains("substation"),
            "the cut still lands"
        );
        assert!(events.is_empty());
    }

    #[test]
    fn a_fallen_vacuum_cannot_be_run_again() {
        let (def, mut state) = standard();
        state.vacuum.fallen = true;
        let events = apply_effects(
            &def,
            &mut state,
            RunConfig::standard(),
            &[Effect::VacuumRun(VACUUM_NODE_ID.into())],
        );
        assert!(!state.vacuum.active, "a fallen vacuum stays down");
        assert!(events.is_empty(), "a refusal raises no event");
    }

    #[test]
    fn looping_a_camera_records_the_loop_and_the_intel_ping() {
        let (def, mut state) = standard();
        apply_effects(
            &def,
            &mut state,
            RunConfig::standard(),
            &[Effect::CameraLooped(camera_node_id(0))],
        );
        assert!(state.camera_looped[0] > 0.0);
        assert!(state.camera_ping > 0.0, "being in the camera is also intel");
        assert!(
            state.camera_looped[0] > state.camera_ping,
            "the loop must outlast the ping it grants"
        );
    }

    #[test]
    fn an_unrecognised_node_id_is_a_no_op() {
        // The fuzz test drives this with ids no fixture answers to; every arm must
        // shrug rather than panic or index out of bounds.
        let (def, mut state) = standard();
        let before = state.clone();
        let events = apply_effects(
            &def,
            &mut state,
            RunConfig::standard(),
            &[
                Effect::DoorHeld("door-nowhere".into()),
                Effect::CameraLooped("cam-999".into()),
                Effect::LockDisengaged("lock-0".into()),
                Effect::ElevatorCalled("lift-0".into()),
                Effect::SensorMuted("pir-0".into()),
            ],
        );
        assert!(events.is_empty());
        assert_eq!(state, before, "an unknown id changes nothing");
    }

    #[test]
    fn reduced_motion_shortens_the_lights_out() {
        let cfg = RunConfig {
            reduced_motion: true,
            ..RunConfig::standard()
        };
        let (def, mut state) = start(cfg);
        apply_effects(
            &def,
            &mut state,
            cfg,
            &[Effect::LightsKilled(light_node_id("kitchen"))],
        );
        assert_eq!(state.lights_flicker, c::LIGHTS_DUR_RM);
        assert_eq!(state.lights_uses, 1);
        assert_eq!(state.stats.light_flickers, 1);
        assert!(state.billy.stun >= c::LIGHTS_STUN);
    }
}

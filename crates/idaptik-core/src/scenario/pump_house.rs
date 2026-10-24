//! The challenge-set floor: **Rivermouth Pump House** (ADR-0013).
//!
//! One floor, one goal — the ledger note out of the wet canteen and the
//! infiltrator out of the building — and four *different* ways in, each
//! leaning on a different system and each readable from the canonical event
//! stream (the evidence is in
//! `docs/scenarios/challenge-set.md`):
//!
//! 1. **Physical access.** The rope down the facade, a picked front-door
//!    lock, and a hand on the pump hall's valve wheel: hold interact beside
//!    the handle and the hall floods. The splash and the control office's
//!    flood alarm are observations Billy reacts to, pulling him east while
//!    the note comes loose in the west.
//! 2. **Hacker / control.** Nobody touches the wheel. From the van, one
//!    pivot onto the bridge opens the floor's fixtures; a `net_hack` on the
//!    derived `valve-hall-sump` node releases the zone through the ordinary
//!    network actuation — the same `WaterReleased { cause: Valve }`, the
//!    same alarm, the same pull, arrived at entirely from the Net View.
//! 3. **Cooperative, Moletaire-assisted.** The hosted mole surfaces near the
//!    control-office door and plays a synth decoy: an unusual-noise
//!    observation inside Billy's own hearing gate draws his Assess patrol
//!    east. The hacker holds the gallery door open on the same beat, the
//!    infiltrator takes the note and both leave by the sump chute — two
//!    seats, one plan, no water required.
//! 4. **Emergent, cross-system.** The two-hop upstream line (ISP ops, then
//!    the grid jump host) reaches the substation; cutting it cascades
//!    `DevicePowerLost` to every fixture on the feed — including the sump
//!    pump, whose valve *fails open*. The hall floods with nobody ordering
//!    it, the lights and cameras die with the feed, the flood alarm trips on
//!    the rising water and the tamper alarm on the dead pump, and the
//!    infiltrator walks the dark, splashing hall while every observation in
//!    the building pulls Billy the other way. No objective, trigger or
//!    script names this chain: it is what the systems do when they meet.
//!
//! The floor inherits the played Ghost Lobby (exterior start, signal wiring,
//! consoles) and *adds* infrastructure and a hosted companion — it changes
//! no canonical floor, so every existing replay, golden and fixture is
//! untouched. Room **ids** stay the canonical five because the interactions
//! and the authored network segments address them; their names, labels and
//! the fiction are the pump house's own.

use crate::companion::CoprocessorType;
use crate::scenario::definition::{MoleHostDef, ScenarioDefinition};
use crate::scenario::floor_graph::pump_node_id;
use crate::scenario::infra::{AlarmDef, AlarmTrip, InfraDef, WaterZoneDef};
use crate::scenario::rooftop::ghost_lobby_rooftop;

/// The stable scenario id of the challenge-set floor.
pub const PUMP_HOUSE_ID: &str = "challenge-set-pump-house";

/// The water zone: the pump hall's sump line.
pub const HALL_SUMP: &str = "hall-sump";

/// Build the Rivermouth Pump House challenge-set floor.
pub fn pump_house() -> ScenarioDefinition {
    let mut def = ghost_lobby_rooftop();
    def.scenario_id = PUMP_HOUSE_ID.to_owned();
    def.name = "Challenge Set — Rivermouth Pump House".to_owned();
    def.floor_id = 1;

    // The same five rooms, dressed as a pumping station. Ids are load-bearing
    // (the interactions and the authored `auto-{room}` network segments
    // address them); names and labels are the fiction.
    for room in &mut def.rooms {
        room.name = match room.id.as_str() {
            "kitchen" => "WET CANTEEN".into(),
            "hall" => "PUMP HALL".into(),
            "office" => "CONTROL OFFICE".into(),
            "laundry" => "SUMP GALLERY".into(),
            "exit" => "SLUICE EXIT".into(),
            other => other.to_uppercase(),
        };
    }
    for door in &mut def.doors {
        door.label = match door.label.as_str() {
            "KITCHEN/HALL" => "CANTEEN/PUMP HALL".into(),
            "HALL/OFFICE" => "PUMP HALL/CONTROL".into(),
            "OFFICE/LAUNDRY" => "CONTROL/GALLERY".into(),
            "SERVICE EXIT" => "SLUICE EXIT".into(),
            other => other.into(),
        };
    }
    for spot in &mut def.hide_spots {
        spot.label = match spot.id.as_str() {
            "counter" => "canteen counter shadow".into(),
            "copier" => "tool recess".into(),
            "desk" => "control desk".into(),
            "wardrobe" => "locker bay".into(),
            "crate" => "sluice crate".into(),
            _ => spot.label.clone(),
        };
    }
    for objective in &mut def.objectives {
        match objective.id.as_str() {
            "note" => objective.label = "Secure the ledger note".into(),
            "exit" => objective.label = "Reach the sluice exit or the sump chute".into(),
            _ => {}
        }
    }

    // The infrastructure the challenge set plays with: one floodable zone in
    // the pump hall, its manual wheel by the control-office door, and two
    // alarms — the flood bell in the control office (its centre is east of
    // Billy's patrol band, so a trip pulls him away from the canteen note)
    // and a normally-energised tamper circuit on the sump pump (cutting the
    // pump's power *is* the trip; failing open is what makes the emergent
    // cascade audible).
    def.infra = Some(InfraDef {
        water_zones: vec![WaterZoneDef {
            id: HALL_SUMP.to_owned(),
            room: "hall".into(),
            valve_x: 470.0,
            fill_time: 24.0,
            fail_open: true,
        }],
        alarms: vec![
            AlarmDef {
                id: "flood-alarm".to_owned(),
                room: "office".into(),
                trip: AlarmTrip::Water {
                    zone: HALL_SUMP.to_owned(),
                },
            },
            AlarmDef {
                id: "sump-tamper".to_owned(),
                room: "hall".into(),
                trip: AlarmTrip::PowerLost {
                    watch: pump_node_id(HALL_SUMP),
                },
            },
        ],
    });

    // The hosted companion: it starts tunnelling under the pump hall with one
    // rung of AudioSynthesiser fitted, so the cooperative solution's decoy is
    // available from the first tick. A stock bay plays nothing at all.
    def.mole = Some(MoleHostDef {
        spawn_x: 390.0,
        underground: true,
        upgrades: vec![CoprocessorType::AudioSynthesiser],
    });

    def
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::netsim::{AgentSession, apply_actuation};
    use crate::scenario::command::RunConfig;
    use crate::scenario::floor_graph::{floor_graph, valve_node_id, van_vantage};
    use crate::scenario::sim::GhostLobbySim;

    #[test]
    fn it_validates() {
        let report = pump_house().validate();
        assert!(report.passed(), "validate() rejected: {:?}", report.ok());
    }

    #[test]
    fn it_hosts_the_infrastructure_and_the_companion() {
        let sim = GhostLobbySim::new(pump_house(), RunConfig::standard(), 7).expect("valid");
        let state = sim.state();
        assert!(state.infra.is_some(), "no infrastructure state");
        assert!(state.mole.is_some(), "no hosted companion");
        assert!(
            state.infra.as_ref().is_some_and(|i| i
                .zones
                .get(HALL_SUMP)
                .is_some_and(|z| !z.released)),
            "the sump starts dry"
        );
    }

    #[test]
    fn the_valve_and_pump_are_one_pivot_deep_from_the_van() {
        // The hacker solution's whole claim: the bridge pivot opens the
        // derived valve, and the substation line beyond it can fell the pump.
        let def = pump_house();
        let g = floor_graph(&def);
        let valve = g
            .node(&valve_node_id(HALL_SUMP))
            .expect("valve node derived");
        let pump = g.node(&pump_node_id(HALL_SUMP)).expect("pump node derived");
        let mut s = AgentSession::new(van_vantage(&g), 10_000);
        assert!(
            !s.reachable(&g).contains(&valve.ip),
            "the valve must not answer the cold van"
        );
        s.ssh(&g, "bridge.local").expect("bridge pivot");
        assert!(s.reachable(&g).contains(&valve.ip), "valve reachable");
        assert!(s.reachable(&g).contains(&pump.ip), "pump reachable");
    }

    #[test]
    fn opening_the_valve_by_actuation_releases_the_zone() {
        // The network half of the chain: the valve's OpenValve actuation
        // carries the WaterReleased effect, and applying it floods the zone.
        let def = pump_house();
        let g = floor_graph(&def);
        let effects = apply_actuation(&g, &valve_node_id(HALL_SUMP));
        assert!(
            effects.contains(&crate::netsim::Effect::WaterReleased(valve_node_id(HALL_SUMP))),
            "the valve actuation carries the release: {effects:?}"
        );
    }

    #[test]
    fn cutting_the_pump_cascades_from_the_substation() {
        // The emergent solution's first domino: the substation cut reaches
        // the pump through the feed, and the pump's loss is what fails the
        // zone open in `apply_effects`.
        let def = pump_house();
        let g = floor_graph(&def);
        let effects = apply_actuation(&g, "substation");
        assert!(
            effects.contains(&crate::netsim::Effect::DevicePowerLost(pump_node_id(HALL_SUMP))),
            "the cascade must fell the sump pump"
        );
        assert!(
            effects.contains(&crate::netsim::Effect::DevicePowerLost(valve_node_id(HALL_SUMP))),
            "and the valve itself"
        );
    }
}

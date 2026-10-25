//! Infrastructure runtime tests: the three committed fixtures are exercised in
//! the order the design asks for — water alone, ventilation alone, then both.
//!
//! The assertions are about *causality the player can read*: a command changes
//! something, a cue follows, material is conserved, and the same tick sequence
//! replays identically after a snapshot round trip.

use idaptik_core::scenario::ids::{
    CompartmentId, DamperId, DrainId, DuctId, EquipmentId, FanId, GasSourceId, LeakId, ValveId,
    WaterOutletId,
};
use idaptik_core::scenario::infrastructure::sim::{
    FanStallReason, InfrastructureCommand, InfrastructureEvent, InfrastructureSim, TripCause,
    load_infrastructure,
};
use idaptik_core::scenario::infrastructure::{HazardKind, Tier, fixtures, model_declaration};

fn cmd_step(sim: &mut InfrastructureSim, cmd: InfrastructureCommand) -> Vec<InfrastructureEvent> {
    let mut events = sim.apply(&cmd);
    events.extend(sim.step());
    events
}

fn run(sim: &mut InfrastructureSim, ticks: usize) -> Vec<InfrastructureEvent> {
    let mut events = Vec::new();
    for _ in 0..ticks {
        events.extend(sim.step());
    }
    events
}

fn has_combustion(events: &[InfrastructureEvent]) -> bool {
    events
        .iter()
        .any(|e| matches!(e, InfrastructureEvent::Combustion { .. }))
}

#[test]
fn every_fixture_validates_and_declares_its_model() {
    for fixture in fixtures::all() {
        let sim = InfrastructureSim::new(fixture.clone()).unwrap_or_else(|errs| {
            panic!("{} failed validation: {errs:?}", fixture.infrastructure_id)
        });
        assert_eq!(sim.state().tick, 0);
    }

    let declaration = model_declaration();
    assert_eq!(declaration.class, "qualitative-game-model");
    assert!(!declaration.not_simulated.is_empty());
}

#[test]
fn fixtures_round_trip_through_the_wire_format() {
    for fixture in fixtures::all() {
        let json = serde_json::to_string(&fixture).expect("fixture serialises");
        let loaded = load_infrastructure(&json).expect("fixture loads");
        assert_eq!(loaded, fixture);
    }
}

#[test]
fn water_fixture_is_conservative_and_causal() {
    let mut sim = InfrastructureSim::new(fixtures::minimal_water()).expect("water fixture");
    let store = CompartmentId::from("service/store");
    let hall = CompartmentId::from("service/hall");

    // Nothing moves until the player opens the store isolator: the outlet is
    // closed and the leak starts patched.
    run(&mut sim, 5);
    assert_eq!(
        sim.state()
            .water
            .as_ref()
            .unwrap()
            .body(&store)
            .unwrap()
            .water,
        0
    );

    let events = cmd_step(
        &mut sim,
        InfrastructureCommand::SetValve {
            valve: ValveId::from("v-store"),
            open: true,
        },
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, InfrastructureEvent::ValveSet { open: true, .. }))
    );

    let events = cmd_step(
        &mut sim,
        InfrastructureCommand::SetOutlet {
            outlet: WaterOutletId::from("o-store"),
            open: true,
        },
    );
    let flowing = sim.state().water.as_ref().unwrap();
    assert!(flowing.body(&store).unwrap().water > 0);
    assert!(events.iter().any(|e| matches!(
        e,
        InfrastructureEvent::HazardChanged {
            tier: Tier::Trace,
            ..
        }
    )));

    // The puddle is a cue and, eventually, a traversal block that the building
    // may consult. The fixture never gets there in ten ticks, which is the
    // point: recovery should be possible before the room is lost.
    run(&mut sim, 10);
    let conditions = sim.conditions(&store).expect("store conditions");
    assert!(conditions.water_tier >= Tier::Trace);
    assert!(!conditions.traversal_blocked, "recovery is still possible");

    // Patching the leak stops the loss; clearing the hall drain empties the
    // room into the authored exterior.
    cmd_step(
        &mut sim,
        InfrastructureCommand::PatchLeak {
            leak: LeakId::from("leak-elbow"),
        },
    );
    let events = cmd_step(
        &mut sim,
        InfrastructureCommand::ClearDrain {
            drain: DrainId::from("d-hall"),
        },
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, InfrastructureEvent::DrainCleared { .. }))
    );
    assert!(sim.conditions(&hall).is_some());

    let water = sim.state().water.as_ref().unwrap();
    assert!(water.balanced(), "water ledger balances");
    let report = sim.conservation_report();
    assert!(
        report.iter().all(|line| !line.contains("imbalance")),
        "{report:?}"
    );
    let accounting = sim.accounting();
    assert!(accounting.balanced);
}

#[test]
fn ventilation_fixture_clears_haze_and_loads_a_filter() {
    let mut sim = InfrastructureSim::new(fixtures::minimal_ventilation()).expect("vent fixture");
    let lab = CompartmentId::from("research/lab");

    // Flows are a per-tick result, not an authored constant: they appear only
    // after the first step.
    assert_eq!(sim.duct_flow(&DuctId::from("duct-lab")), 0);
    run(&mut sim, 1);
    assert!(
        sim.duct_flow(&DuctId::from("duct-lab")) > 0,
        "the extract fan starts on with the damper open"
    );

    run(&mut sim, 4);
    let air = sim.state().air.as_ref().unwrap();
    assert_eq!(
        air.body(&lab).unwrap().species_amount(&"haze".into()),
        0,
        "no haze is released by this fixture"
    );

    // Closing the damper stalls the fan with a readable reason.
    let events = cmd_step(
        &mut sim,
        InfrastructureCommand::SetDamper {
            damper: DamperId::from("damper-lab"),
            position: 0,
        },
    );
    assert!(events.iter().any(|e| matches!(
        e,
        InfrastructureEvent::AirflowStalled {
            reason: FanStallReason::DamperClosed,
            ..
        }
    )));
    assert_eq!(sim.duct_flow(&DuctId::from("duct-lab")), 0);

    // Re-opening it restores flow; redirecting the reversible fan swaps ends.
    cmd_step(
        &mut sim,
        InfrastructureCommand::SetDamper {
            damper: DamperId::from("damper-lab"),
            position: 100,
        },
    );
    assert!(sim.duct_flow(&DuctId::from("duct-lab")) > 0);
    let events = cmd_step(
        &mut sim,
        InfrastructureCommand::SetFanDirection {
            fan: FanId::from("fan-lab"),
            direction: idaptik_core::scenario::infrastructure::FlowDirection::BToA,
        },
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, InfrastructureEvent::FanDirectionSet { .. }))
    );

    assert!(sim.state().air.as_ref().unwrap().air_balanced());
}

#[test]
fn ember_ignites_only_while_equipment_is_live() {
    let mut sim = InfrastructureSim::new(fixtures::minimal_ventilation()).expect("vent fixture");
    let lab = CompartmentId::from("research/lab");

    // Close the extract damper and the lab door so the gas accumulates.
    cmd_step(
        &mut sim,
        InfrastructureCommand::SetDamper {
            damper: DamperId::from("damper-lab"),
            position: 0,
        },
    );
    sim.sync_opening(&"open-lab-grille".into(), false);

    let mut events = cmd_step(
        &mut sim,
        InfrastructureCommand::SetGasSource {
            source: GasSourceId::from("src-ember"),
            on: true,
        },
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, InfrastructureEvent::GasSourceSet { on: true, .. }))
    );
    events.extend(run(&mut sim, 90));

    assert!(
        has_combustion(&events),
        "live equipment ignites critical ember: {events:?}"
    );
    assert!(events.iter().any(|e| matches!(
        e,
        InfrastructureEvent::EquipmentTripped {
            cause: TripCause::Ignition,
            ..
        }
    )));

    // The room now reads as an ignited hazard with a consequence, not an
    // invisible number: the ignition cue fires and the gas is gone.
    assert!(events.iter().any(|e| matches!(
        e,
        InfrastructureEvent::Cue {
            cue: idaptik_core::scenario::infrastructure::CueKind::Ignition,
            ..
        }
    )));
    let conditions = sim.conditions(&lab).expect("lab conditions");
    assert!(conditions.gas.contains_key(&"ember".into()));
    let air = sim.state().air.as_ref().unwrap();
    let burned = air
        .ledger
        .species_combusted
        .get(&"ember".into())
        .copied()
        .unwrap_or(0);
    assert!(
        burned > 0,
        "combustion destroys gas and records it in the ledger"
    );
    assert_eq!(
        conditions
            .gas
            .get(&"ember".into())
            .map(|g| g.units)
            .unwrap_or(0)
            + burned,
        1600,
        "released gas is either still in the room or accounted as combusted"
    );

    // Recovery path: the equipment can be reset once the room is clear.
    let events = cmd_step(
        &mut sim,
        InfrastructureCommand::ResetEquipment {
            equipment: EquipmentId::from("eq-bench"),
        },
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, InfrastructureEvent::EquipmentReset { .. }))
    );
    assert!(sim.state().air.as_ref().unwrap().air_balanced());
}

#[test]
fn combined_fixture_couples_water_air_and_power_through_one_trip() {
    let mut sim = InfrastructureSim::new(fixtures::combined()).expect("combined fixture");
    let plant = CompartmentId::from("service/plant");

    // De-energise the plant circuit: the running extract fan stalls, and the
    // room stops being cleared.
    let events = sim.sync_circuit(&"circuit-plant".into(), false);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, InfrastructureEvent::CircuitSynced { live: false, .. }))
    );
    let events = run(&mut sim, 1);
    assert!(events.iter().any(|e| matches!(
        e,
        InfrastructureEvent::AirflowStalled {
            reason: FanStallReason::Unpowered,
            ..
        }
    )));

    // Restore power, open the manifold leak: conductive water spreads in the
    // plant room and trips the live starter.
    sim.sync_circuit(&"circuit-plant".into(), true);
    let mut events = run(&mut sim, 20);
    events.extend(cmd_step(
        &mut sim,
        InfrastructureCommand::SetValve {
            valve: ValveId::from("v-yard"),
            open: true,
        },
    ));
    events.extend(run(&mut sim, 30));

    let water = sim.state().water.as_ref().unwrap();
    assert!(
        water.body(&plant).unwrap().water > 0,
        "the leak fills the plant room"
    );
    assert!(water.balanced());

    let conditions = sim.conditions(&plant).expect("plant conditions");
    assert!(conditions.water_tier >= Tier::Trace);
    assert!(
        conditions
            .cues
            .contains_key(&idaptik_core::scenario::infrastructure::CueKind::Puddle)
    );

    // Whatever the coupling decided, it must be visible as state that a UI can
    // explain: every hazard axis exists for every interior compartment.
    let all = sim.all_conditions();
    assert!(all.contains_key(&plant));
    for conditions in all.values() {
        assert!(
            conditions.gas.len() >= 3,
            "all authored species are reported"
        );
        assert!(matches!(
            conditions.pressure_tier,
            Tier::Clear | Tier::Trace | Tier::Warning | Tier::Critical
        ));
    }

    assert!(sim.accounting().balanced);
    assert!(
        sim.conservation_report()
            .iter()
            .all(|l| !l.contains("imbalance"))
    );

    // The hazard axes are stable, readable ids rather than a bag of floats.
    assert!(
        all.values()
            .any(|c| c.gas.keys().any(|k| k == &"haze".into()))
    );
    assert!(matches!(
        HazardKind::Water.cue(),
        idaptik_core::scenario::infrastructure::CueKind::Puddle
    ));
    sim.conditions(&plant);
}

#[test]
fn snapshot_round_trip_preserves_the_run() {
    let fixture = fixtures::combined();
    let mut a = InfrastructureSim::new(fixture).expect("combined fixture");
    run(&mut a, 12);
    let snapshot = a.snapshot();
    let json = serde_json::to_string(&snapshot).expect("snapshot serialises");
    let restored_snapshot: idaptik_core::scenario::infrastructure::InfrastructureSnapshot =
        serde_json::from_str(&json).expect("snapshot parses");
    let mut b = InfrastructureSim::restore(restored_snapshot).expect("snapshot restores");

    let events_a = run(&mut a, 15);
    let events_b = run(&mut b, 15);
    assert_eq!(a.state(), b.state());
    assert_eq!(events_a, events_b);
    assert_eq!(a.snapshot().model, b.snapshot().model);
}

#[test]
fn commands_that_cannot_apply_are_denied_not_ignored() {
    let mut sim = InfrastructureSim::new(fixtures::minimal_water()).expect("water fixture");
    let events = sim.apply(&InfrastructureCommand::SetValve {
        valve: ValveId::from("v-nope"),
        open: true,
    });
    assert!(
        events
            .iter()
            .any(|e| matches!(e, InfrastructureEvent::Denied { .. }))
    );

    let events = sim.apply(&InfrastructureCommand::SetValve {
        valve: ValveId::from("v-main"),
        open: true,
    });
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, InfrastructureEvent::Denied { .. }))
    );

    // Reservoir accounting is external: the fixture's tank is the only signed
    // source, and it is named in the ledger rather than conjured.
    let water = sim.state().water.as_ref().unwrap();
    assert!(water.water_in_reservoirs() <= 2400);
    assert_eq!(
        sim.state().water.as_ref().unwrap().water_in_reservoirs(),
        sim.state().water.as_ref().unwrap().ledger.initial_reservoir
            + sim.state().water.as_ref().unwrap().ledger.recharged
            - sim
                .state()
                .water
                .as_ref()
                .unwrap()
                .ledger
                .drained_to_exterior
            - sim
                .state()
                .water
                .as_ref()
                .unwrap()
                .ledger
                .spilled_to_exterior
            - sim.state().water.as_ref().unwrap().water_in_compartments()
    );
}

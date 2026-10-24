//! The electrical infrastructure (ADR-0013), end to end: the test floor's golden
//! and validator, the arbitration ladder, selective loss, backup runtime,
//! restart policy, protective trips and resets, exposed power, and the distinct
//! permissions and costs of a physical cut versus logical rewiring.

use idaptik_core::scenario::ids::{BreakerId, FeedId, HazardId, LoadId};
use idaptik_core::scenario::power::sim::{
    Fault, OffReason, POWER_SNAPSHOT_FORMAT, PowerAccess, PowerCommand, PowerDenyReason,
    PowerEvent, PowerIndex, PowerOp, PowerReadout, PowerSim, PowerSnapshot,
};
use idaptik_core::scenario::power::test_floor::{
    BRK_A, BRK_B, CAM_HALL, DOOR_EXIT, FAN_KITCHEN, FEED_A, FEED_B, MAINS_A, MAINS_B,
    POWER_TEST_FLOOR_JSON, PUMP_LAUNDRY, UPS_HALL, power_test_floor, powered_ghost_lobby,
};
use idaptik_core::scenario::power::{
    BackupMode, BackupSpec, Condition, InterlockDef, InterlockWhen, LoadKind, POWER_FORMAT,
    PowerNetworkDef, PowerTuning, PowerValidationError, Restart, SourceKind, load_power_network,
};
use idaptik_core::scenario::signal::sim::Seat;

const OFFICE_CAM: &str = "cam-1";
const LAUNDRY_LIGHT: &str = "light-laundry";

fn floor() -> PowerSim {
    PowerSim::new(power_test_floor()).expect("the test floor validates")
}

fn all(sim: &PowerSim) -> PowerAccess {
    PowerAccess::everyone(sim.definition())
}

fn idle() -> PowerReadout {
    PowerReadout::default()
}

fn run(sim: &mut PowerSim, ticks: u32) -> Vec<PowerEvent> {
    let readout = idle();
    (0..ticks).flat_map(|_| sim.step(&readout)).collect()
}

fn feed(id: &str) -> FeedId {
    FeedId::from(id)
}

fn load(id: &str) -> LoadId {
    LoadId::from(id)
}

fn breaker(id: &str) -> BreakerId {
    BreakerId::from(id)
}

fn why(sim: &PowerSim, id: &str) -> Option<OffReason> {
    sim.report(&load(id)).and_then(|r| r.why)
}

fn operating(sim: &PowerSim, id: &str) -> bool {
    sim.report(&load(id)).is_some_and(|r| r.operating)
}

fn isolate(sim: &mut PowerSim, by: Seat, id: &str, open: bool) -> Vec<PowerEvent> {
    let access = all(sim);
    let cmd = PowerCommand::Isolate {
        by,
        breaker: breaker(id),
        open,
    };
    sim.apply(&cmd, &access)
}

fn cut(sim: &mut PowerSim, by: Seat, id: &str) -> Vec<PowerEvent> {
    let access = all(sim);
    let cmd = PowerCommand::CutCable { by, feed: feed(id) };
    sim.apply(&cmd, &access)
}

fn route(sim: &mut PowerSim, by: Seat, load_id: &str, to: &str) -> Vec<PowerEvent> {
    let access = all(sim);
    let cmd = PowerCommand::Route {
        by,
        load: load(load_id),
        feed: feed(to),
    };
    sim.apply(&cmd, &access)
}

fn command(sim: &mut PowerSim, by: Seat, load_id: &str, on: bool) -> Vec<PowerEvent> {
    let access = all(sim);
    let cmd = PowerCommand::SetCommand {
        by,
        load: load(load_id),
        on,
    };
    sim.apply(&cmd, &access)
}

fn switch(sim: &mut PowerSim, by: Seat, source: &str, on: bool) -> Vec<PowerEvent> {
    let access = all(sim);
    let cmd = PowerCommand::SwitchSource {
        by,
        source: idaptik_core::scenario::ids::PowerSourceId::from(source),
        on,
    };
    sim.apply(&cmd, &access)
}

fn denied(events: &[PowerEvent]) -> Option<&PowerDenyReason> {
    events.iter().find_map(|e| match e {
        PowerEvent::Denied { reason, .. } => Some(reason),
        _ => None,
    })
}

// --- definition ---------------------------------------------------------------

#[test]
fn the_committed_golden_matches_the_derived_network() {
    let parsed: PowerNetworkDef = serde_json::from_str(POWER_TEST_FLOOR_JSON).expect("parses");
    assert_eq!(parsed, power_test_floor());
}

#[test]
fn the_ums_seam_gates_on_the_format_tag() {
    assert!(load_power_network(POWER_TEST_FLOOR_JSON).is_ok());
    let mut value: serde_json::Value = serde_json::from_str(POWER_TEST_FLOOR_JSON).expect("parses");
    value["format"] = serde_json::Value::String("idaptik-power/0".into());
    let text = serde_json::to_string(&value).expect("serializes");
    let err = load_power_network(&text).expect_err("the wrong format is refused");
    assert!(format!("{err:?}").contains("UnsupportedFormat"));
}

#[test]
fn the_floor_has_two_independent_feeds_and_one_backup_powered_device() {
    let def = power_test_floor();
    assert_eq!(def.feeds.len(), 2);
    assert_ne!(def.feeds[0].source, def.feeds[1].source);
    let backup = def
        .sources
        .iter()
        .find(|s| s.backup.is_some())
        .expect("one backup");
    let spec = backup.backup.as_ref().expect("spec");
    assert_eq!(spec.serves, vec![load(CAM_HALL)]);
    assert_eq!(spec.mode, BackupMode::Automatic);
    // And it is attached to the floor, not just to the network.
    assert!(powered_ghost_lobby().power.is_some());
    assert!(
        idaptik_core::scenario::ghost_lobby::ghost_lobby()
            .power
            .is_none()
    );
}

#[test]
fn an_unprotected_or_circular_network_is_rejected() {
    let mut def = power_test_floor();
    def.breakers.retain(|b| b.id != BreakerId::from(BRK_A));
    assert!(
        def.validate()
            .iter()
            .any(|e| matches!(e, PowerValidationError::FeedBreakerMissing { .. }))
    );

    let mut def = power_test_floor();
    if let Some(s) = def.sources.get_mut(0) {
        s.backup = Some(BackupSpec {
            runtime_ticks: 60,
            serves: vec![load(FAN_KITCHEN)],
            mode: BackupMode::Manual,
        });
    }
    assert!(
        def.validate()
            .iter()
            .any(|e| matches!(e, PowerValidationError::BackupDrivesFeed { .. }))
    );
}

// --- the ladder ---------------------------------------------------------------

#[test]
fn a_settled_floor_runs_its_devices() {
    let mut sim = floor();
    run(&mut sim, 260);
    assert!(operating(&sim, CAM_HALL));
    assert!(operating(&sim, OFFICE_CAM));
    assert!(operating(&sim, DOOR_EXIT));
    assert!(operating(&sim, FAN_KITCHEN));
    // The pump is interlocked while the laundry lights are on, and the test
    // floor's readout says they are not: with no readout, nothing interlocks.
    assert!(operating(&sim, PUMP_LAUNDRY));
}

#[test]
fn every_rung_of_the_ladder_can_be_seen() {
    // No command.
    let mut sim = floor();
    run(&mut sim, 200);
    command(&mut sim, Seat::Infiltrator, OFFICE_CAM, false);
    run(&mut sim, 2);
    assert_eq!(why(&sim, OFFICE_CAM), Some(OffReason::NoCommand));

    // Unpowered: the source is off, which is recoverable.
    let mut sim = floor();
    run(&mut sim, 200);
    switch(&mut sim, Seat::Infiltrator, MAINS_B, false);
    run(&mut sim, 2);
    assert_eq!(
        why(&sim, OFFICE_CAM),
        Some(OffReason::Unpowered { feed: feed(FEED_B) })
    );

    // Isolated: protection open.
    let mut sim = floor();
    run(&mut sim, 200);
    isolate(&mut sim, Seat::Infiltrator, BRK_B, true);
    run(&mut sim, 2);
    assert_eq!(
        why(&sim, OFFICE_CAM),
        Some(OffReason::Isolated {
            breaker: breaker(BRK_B)
        })
    );

    // Unavailable: the conductor is gone for the run.
    let mut sim = floor();
    run(&mut sim, 200);
    cut(&mut sim, Seat::Infiltrator, FEED_B);
    run(&mut sim, 2);
    assert_eq!(
        why(&sim, OFFICE_CAM),
        Some(OffReason::Unavailable { feed: feed(FEED_B) })
    );

    // Damaged and disconnected are properties of the device, not its supply.
    let mut def = power_test_floor();
    if let Some(l) = def.loads.iter_mut().find(|l| l.id == load(OFFICE_CAM)) {
        l.condition = Condition::Damaged;
    }
    let mut sim = PowerSim::new(def).expect("valid");
    run(&mut sim, 200);
    assert_eq!(why(&sim, OFFICE_CAM), Some(OffReason::Damaged));

    let mut def = power_test_floor();
    if let Some(l) = def.loads.iter_mut().find(|l| l.id == load(OFFICE_CAM)) {
        l.condition = Condition::Disconnected;
    }
    let mut sim = PowerSim::new(def).expect("valid");
    run(&mut sim, 200);
    assert_eq!(why(&sim, OFFICE_CAM), Some(OffReason::Disconnected));

    // Interlock: a local safety, above the command.
    let mut def = power_test_floor();
    def.interlocks.push(InterlockDef {
        id: "il-cam".into(),
        label: "Camera maintenance lockout".into(),
        load: load(OFFICE_CAM),
        holds_when: InterlockWhen::SignalLocked(idaptik_core::scenario::ids::SignalId::from(
            DOOR_EXIT,
        )),
    });
    let mut sim = PowerSim::new(def).expect("valid");
    let mut readout = PowerReadout::default();
    readout.locked(DOOR_EXIT);
    for _ in 0..200 {
        sim.step(&readout);
    }
    assert!(matches!(
        why(&sim, OFFICE_CAM),
        Some(OffReason::Interlock { .. })
    ));
}

#[test]
fn a_command_never_beats_a_dead_supply() {
    let mut sim = floor();
    run(&mut sim, 200);
    cut(&mut sim, Seat::Infiltrator, FEED_B);
    run(&mut sim, 2);
    // Ask everything on the dead feed to come on.
    for id in [OFFICE_CAM, CAM_HALL, DOOR_EXIT] {
        command(&mut sim, Seat::Hacker, id, true);
    }
    run(&mut sim, 2);
    assert!(sim.report(&load(OFFICE_CAM)).is_some_and(|r| r.commanded));
    assert!(!operating(&sim, OFFICE_CAM));
    assert!(!operating(&sim, CAM_HALL));
    assert!(!operating(&sim, DOOR_EXIT));
}

// --- loss, restart, backup -----------------------------------------------------

#[test]
fn losing_one_feed_does_not_touch_the_other() {
    let mut sim = floor();
    run(&mut sim, 260);
    assert!(operating(&sim, LAUNDRY_LIGHT) && operating(&sim, OFFICE_CAM));
    cut(&mut sim, Seat::Infiltrator, FEED_A);
    run(&mut sim, 2);
    // The plant and lighting are down…
    assert!(
        !operating(&sim, LAUNDRY_LIGHT),
        "lighting is on the dead feed"
    );
    assert!(!operating(&sim, FAN_KITCHEN), "so is the extract fan");
    // …and security is not.
    assert!(operating(&sim, OFFICE_CAM));
    assert!(operating(&sim, DOOR_EXIT));
}

#[test]
fn restart_behaviour_is_stated_per_device_not_discovered() {
    let mut sim = floor();
    run(&mut sim, 200);
    // Isolation is reversible, so this is the clean way to test a restore.
    isolate(&mut sim, Seat::Infiltrator, BRK_B, true);
    run(&mut sim, 2);
    // The camera keeps its order; the door does not.
    assert!(
        sim.report(&load(OFFICE_CAM)).is_some_and(|r| r.commanded),
        "Resume keeps the command"
    );
    assert!(
        !sim.report(&load(DOOR_EXIT)).is_some_and(|r| r.commanded),
        "FailSafe drops it"
    );

    isolate(&mut sim, Seat::Infiltrator, BRK_B, false);
    let events = run(&mut sim, 1);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, PowerEvent::FeedRestored { .. })),
        "{events:?}"
    );
    // No phantom doors: the fail-safe device waits for a fresh order.
    assert_eq!(why(&sim, DOOR_EXIT), Some(OffReason::NoCommand));
    // The resuming one warms up first.
    assert!(matches!(
        why(&sim, OFFICE_CAM),
        Some(OffReason::Restarting { .. })
    ));
    run(&mut sim, 130);
    assert!(operating(&sim, OFFICE_CAM), "warm-up over, it resumes");
    assert!(!operating(&sim, DOOR_EXIT), "still waiting to be asked");
    command(&mut sim, Seat::Infiltrator, DOOR_EXIT, true);
    run(&mut sim, 1);
    assert!(operating(&sim, DOOR_EXIT), "a new order starts it at once");
}

#[test]
fn the_backup_runs_for_its_stated_runtime_and_then_stops() {
    let mut sim = floor();
    run(&mut sim, 200);
    let events = cut(&mut sim, Seat::Infiltrator, FEED_B);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, PowerEvent::CableCut { .. })),
        "{events:?}"
    );
    let engaged = run(&mut sim, 1);
    assert!(
        engaged
            .iter()
            .any(|e| matches!(e, PowerEvent::BackupEngaged { .. })),
        "{engaged:?}"
    );
    // "Temporary operation": the camera never blinks, because the UPS picks up
    // in the same tick the cable goes.
    assert!(operating(&sim, CAM_HALL));
    assert!(!operating(&sim, OFFICE_CAM), "the UPS serves one device");
    run(&mut sim, 898);
    assert!(operating(&sim, CAM_HALL), "900 ticks of runtime");
    let spent = run(&mut sim, 3);
    assert!(
        spent
            .iter()
            .any(|e| matches!(e, PowerEvent::BackupSpent { .. })),
        "{spent:?}"
    );
    assert!(!operating(&sim, CAM_HALL), "and then it is spent");
    // A spent backup does not come back.
    let events = switch(&mut sim, Seat::Infiltrator, UPS_HALL, true);
    assert_eq!(
        denied(&events),
        Some(&PowerDenyReason::BackupExhausted {
            source: idaptik_core::scenario::ids::PowerSourceId::from(UPS_HALL)
        })
    );
}

// --- protection ---------------------------------------------------------------

#[test]
fn an_overload_trips_the_protection_and_only_a_clear_fault_resets_it() {
    let mut sim = floor();
    run(&mut sim, 200);
    // Feed B carries 13 of its 14 units. Put the 6-unit pump on it.
    route(&mut sim, Seat::Infiltrator, PUMP_LAUNDRY, FEED_B);
    let warned = run(&mut sim, 1);
    assert!(
        warned
            .iter()
            .any(|e| matches!(e, PowerEvent::Overloaded { .. })),
        "{warned:?}"
    );
    let tripped = run(&mut sim, 60);
    assert!(
        tripped
            .iter()
            .any(|e| matches!(e, PowerEvent::Tripped { .. })),
        "{tripped:?}"
    );
    assert!(!operating(&sim, OFFICE_CAM));

    // The fault is still there, so the breaker will not close.
    let refused = isolate(&mut sim, Seat::Infiltrator, BRK_B, false);
    assert_eq!(
        denied(&refused),
        Some(&PowerDenyReason::FaultPresent {
            fault: Fault::Overload
        })
    );

    // Take the load back off, and the protection can be deliberately reset.
    route(&mut sim, Seat::Infiltrator, PUMP_LAUNDRY, FEED_A);
    run(&mut sim, 1);
    let reset = isolate(&mut sim, Seat::Infiltrator, BRK_B, false);
    assert!(
        reset.iter().any(|e| matches!(e, PowerEvent::Reset { .. })),
        "{reset:?}"
    );
    run(&mut sim, 130);
    assert!(operating(&sim, OFFICE_CAM));
}

#[test]
fn a_cut_conductor_trips_the_protection_and_never_clears() {
    let mut sim = floor();
    run(&mut sim, 200);
    cut(&mut sim, Seat::Infiltrator, FEED_A);
    run(&mut sim, 2);
    let refused = isolate(&mut sim, Seat::Infiltrator, BRK_A, false);
    assert_eq!(
        denied(&refused),
        Some(&PowerDenyReason::FaultPresent {
            fault: Fault::ConductorCut
        })
    );
    // And no amount of rewiring brings the feed back: the cable is gone.
    run(&mut sim, 10);
    assert_eq!(
        why(&sim, LAUNDRY_LIGHT),
        Some(OffReason::Unavailable { feed: feed(FEED_A) })
    );
}

// --- exposed power -------------------------------------------------------------

#[test]
fn exposed_power_is_live_only_where_it_is_authored_and_only_while_fed() {
    let hazard = HazardId::from("haz-pump");
    let mut sim = floor();
    run(&mut sim, 200);

    // In the laundry, inside the authored bounds.
    let mut readout = PowerReadout::default();
    readout.body("laundry", 960.0);
    let touched = sim.step(&readout);
    assert!(
        touched
            .iter()
            .any(|e| matches!(e, PowerEvent::Shocked { hazard: h, .. } if *h == hazard)),
        "{touched:?}"
    );
    // Still standing in it: one shock per contact, not one per tick.
    let again = sim.step(&readout);
    assert!(
        !again
            .iter()
            .any(|e| matches!(e, PowerEvent::Shocked { .. })),
        "{again:?}"
    );

    // Out of bounds is safe.
    let mut outside = PowerReadout::default();
    outside.body("laundry", 850.0);
    let clear = sim.step(&outside);
    assert!(
        !clear
            .iter()
            .any(|e| matches!(e, PowerEvent::Shocked { .. }))
    );

    // Isolating the feed makes the exposed housing dead.
    isolate(&mut sim, Seat::Infiltrator, BRK_A, true);
    sim.step(&readout);
    let safe = sim.step(&readout);
    assert!(
        !safe.iter().any(|e| matches!(e, PowerEvent::Shocked { .. })),
        "{safe:?}"
    );
}

// --- permissions and costs -----------------------------------------------------

#[test]
fn a_cable_cut_is_hand_work_and_rewiring_is_not() {
    let mut sim = floor();
    run(&mut sim, 200);

    // The hacker can reach the feed and still cannot cut it.
    assert_eq!(
        denied(&cut(&mut sim, Seat::Hacker, FEED_A)),
        Some(&PowerDenyReason::NotPhysical)
    );

    // A cut costs hands and noise; a rewire costs bandwidth, and vice versa.
    let infiltrator_cut = sim.definition().tuning.infiltrator.cut_busy_ticks;
    let hacker_route = sim.definition().tuning.hacker.route_bandwidth;
    assert!(infiltrator_cut > 0 && hacker_route > 0);
    let cut_events = cut(&mut sim, Seat::Infiltrator, FEED_A);
    let cut_cost = cut_events
        .iter()
        .find_map(|e| e.cost())
        .expect("a cut is paid for");
    assert_eq!(cut_cost.busy_ticks, infiltrator_cut);
    assert_eq!(cut_cost.bandwidth, 0);
    assert!(cut_cost.noise > 0.0, "cutting a cable is loud");

    let routed = route(&mut sim, Seat::Hacker, OFFICE_CAM, FEED_A);
    let route_cost = routed.iter().find_map(|e| e.cost()).expect("a rewire");
    assert_eq!(route_cost.bandwidth, hacker_route);
    assert_eq!(route_cost.busy_ticks, 0);
    assert_eq!(route_cost.noise, 0.0, "rewiring is quiet");
}

#[test]
fn a_local_board_is_not_a_remote_board() {
    let mut sim = floor();
    run(&mut sim, 200);
    // Give the hacker the *access* and it still cannot work a local-only board.
    let mut access = PowerAccess::default();
    access.hacker.insert(BRK_A.to_owned());
    let cmd = PowerCommand::Isolate {
        by: Seat::Hacker,
        breaker: breaker(BRK_A),
        open: true,
    };
    let events = sim.apply(&cmd, &access);
    assert_eq!(
        denied(&events),
        Some(&PowerDenyReason::RemotePanel {
            element: BRK_A.to_owned()
        })
    );

    // The security board is remote, and the same command lands.
    access.hacker.insert(BRK_B.to_owned());
    let cmd = PowerCommand::Isolate {
        by: Seat::Hacker,
        breaker: breaker(BRK_B),
        open: true,
    };
    let events = sim.apply(&cmd, &access);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, PowerEvent::Isolated { .. }))
    );
}

#[test]
fn a_dumb_load_has_no_remote_panel() {
    let mut sim = floor();
    run(&mut sim, 200);
    let mut access = PowerAccess::default();
    access.hacker.insert(PUMP_LAUNDRY.to_owned());
    let cmd = PowerCommand::Route {
        by: Seat::Hacker,
        load: load(PUMP_LAUNDRY),
        feed: feed(FEED_B),
    };
    let events = sim.apply(&cmd, &access);
    assert_eq!(
        denied(&events),
        Some(&PowerDenyReason::RemotePanel {
            element: PUMP_LAUNDRY.to_owned()
        })
    );
}

#[test]
fn rewiring_a_disconnected_device_reconnects_it() {
    let mut def = power_test_floor();
    // The office camera is off the plant board and wired to nothing, and the
    // security board has the headroom to take it: a *disconnected* device draws
    // nothing, so neither board is over its rating until the rewire lands.
    if let Some(l) = def.loads.iter_mut().find(|l| l.id == load(OFFICE_CAM)) {
        l.condition = Condition::Disconnected;
        l.feed = feed(FEED_A);
    }
    if let Some(f) = def.feeds.iter_mut().find(|f| f.id == feed(FEED_B)) {
        f.capacity = 20;
    }
    let mut sim = PowerSim::new(def).expect("valid");
    run(&mut sim, 200);
    assert_eq!(why(&sim, OFFICE_CAM), Some(OffReason::Disconnected));
    let events = route(&mut sim, Seat::Infiltrator, OFFICE_CAM, FEED_B);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, PowerEvent::Routed { .. })),
        "{events:?}"
    );
    run(&mut sim, 130);
    assert!(operating(&sim, OFFICE_CAM), "wired back in and running");
}

// --- wire form, snapshots, and the whole floor ---------------------------------

#[test]
fn the_wire_form_names_one_namespace_per_verb() {
    let def = power_test_floor();
    let op = PowerOp::Route {
        by: Seat::Infiltrator,
        load: PowerIndex(0),
        feed: PowerIndex(1),
    };
    let json = serde_json::to_string(&op).expect("serializes");
    let back: PowerOp = serde_json::from_str(&json).expect("parses");
    assert_eq!(back, op);
    assert!(matches!(op.resolve(&def), Ok(PowerCommand::Route { .. })));
    assert_eq!(
        PowerOp::Route {
            by: Seat::Infiltrator,
            load: PowerIndex(99),
            feed: PowerIndex(1)
        }
        .resolve(&def),
        Err(PowerDenyReason::NoSuchElement { index: 99 })
    );
}

#[test]
fn a_snapshot_restores_the_run_exactly() {
    let mut a = floor();
    run(&mut a, 40);
    cut(&mut a, Seat::Infiltrator, FEED_B);
    run(&mut a, 30);

    let snapshot: PowerSnapshot = a.snapshot();
    assert_eq!(snapshot.format, POWER_SNAPSHOT_FORMAT);
    let text = serde_json::to_string(&snapshot).expect("serializes");
    let parsed: PowerSnapshot = serde_json::from_str(&text).expect("parses");
    let mut b = PowerSim::restore(parsed).expect("restores");

    let readout = idle();
    let left: Vec<PowerEvent> = (0..20).flat_map(|_| a.step(&readout)).collect();
    let right: Vec<PowerEvent> = (0..20).flat_map(|_| b.step(&readout)).collect();
    assert_eq!(left, right);
    assert_eq!(a.state(), b.state());
}

#[test]
fn the_snapshot_format_is_not_negotiable() {
    let mut snapshot = floor().snapshot();
    snapshot.format = "idaptik-power-snapshot/0".into();
    assert!(PowerSim::restore(snapshot).is_err());
}

#[test]
fn the_whole_definitions_tuning_is_readable() {
    let def = power_test_floor();
    assert_eq!(def.format, POWER_FORMAT);
    assert_eq!(def.tuning, PowerTuning::default());
    assert_eq!(
        def.loads
            .iter()
            .filter(|l| l.kind == LoadKind::Camera)
            .count(),
        3
    );
    assert_eq!(
        def.loads
            .iter()
            .filter(|l| l.restart == Restart::FailSafe)
            .count(),
        1,
        "exactly one device refuses to come back by itself"
    );
    assert_eq!(def.sources[0].kind, SourceKind::Mains);
    assert_eq!(def.sources[2].kind, SourceKind::Battery);
    assert!(def.sources.iter().any(|s| s.id.as_str() == MAINS_A));
    assert!(def.feeds.iter().any(|f| f.id.as_str() == FEED_A));
}

//! The signal network (ADR-0012), end to end: the Ghost Lobby wiring fixture
//! and its golden, the validator, tick semantics, both seats' access and skill,
//! relays, and determinism through snapshot/restore.

use idaptik_core::scenario::floor_graph::floor_graph;
use idaptik_core::scenario::ghost_lobby::ghost_lobby;
use idaptik_core::scenario::ids::SignalId;
use idaptik_core::scenario::signal::ghost_lobby_wiring::{
    BILLY, BRIDGE, GHOST_LOBBY_WIRING_JSON, INTERLOCK, ghost_lobby_wiring,
};
use idaptik_core::scenario::signal::sim::{
    Access, Readout, Seat, SignalCommand, SignalDenyReason, SignalEvent, SignalSim, SignalSnapshot,
    Switch, WireEnds, WorkCost,
};
use idaptik_core::scenario::signal::{
    Gate, InPort, LinkDef, OutPort, SIGNAL_FORMAT, SignalDeviceDef, SignalKind, SignalNetworkDef,
    SignalValidationError, load_signal_network,
};

const D1: &str = "door-d1";
const D4: &str = "door-d4";
const HALL_CAM: &str = "cam-0";

fn sim() -> SignalSim {
    SignalSim::new(ghost_lobby_wiring()).expect("the fixture validates")
}

fn idle() -> Readout {
    Readout::default()
}

fn high(device: &str, port: OutPort) -> Readout {
    let mut r = Readout::default();
    r.set(device, port);
    r
}

fn run(s: &mut SignalSim, readout: &Readout, ticks: u32) -> Vec<SignalEvent> {
    (0..ticks).flat_map(|_| s.step(readout)).collect()
}

fn switched(events: &[SignalEvent], device: &str, what: Switch, on: bool) -> bool {
    count_switched(events, device, what, on) > 0
}

fn count_switched(events: &[SignalEvent], device: &str, what: Switch, on: bool) -> usize {
    let want = SignalEvent::Switched {
        device: device.into(),
        what,
        on,
    };
    events.iter().filter(|e| **e == want).count()
}

fn wire(by: Seat, from: &str, fp: OutPort, to: &str, tp: InPort) -> SignalCommand {
    SignalCommand::Wire {
        by,
        ends: WireEnds::new(from, fp, to, tp),
    }
}

fn denied(events: &[SignalEvent]) -> Option<&SignalDenyReason> {
    match events {
        [SignalEvent::Denied { reason, .. }] => Some(reason),
        _ => None,
    }
}

fn only(ids: &[&str]) -> std::collections::BTreeSet<SignalId> {
    ids.iter().map(|s| SignalId::from(*s)).collect()
}

// --- definition --------------------------------------------------------------

#[test]
fn the_committed_golden_matches_the_derived_network() {
    let parsed: SignalNetworkDef = serde_json::from_str(GHOST_LOBBY_WIRING_JSON).expect("parses");
    assert_eq!(parsed, ghost_lobby_wiring());
}

#[test]
fn the_committed_golden_is_serde_round_trip_stable() {
    let round = serde_json::to_string_pretty(&ghost_lobby_wiring()).expect("serialise");
    assert_eq!(round.trim(), GHOST_LOBBY_WIRING_JSON.trim());
}

#[test]
fn the_fixture_loads_through_the_ums_seam() {
    assert_eq!(
        load_signal_network(GHOST_LOBBY_WIRING_JSON),
        Ok(ghost_lobby_wiring())
    );
}

#[test]
fn every_node_the_fixture_names_exists_in_the_grounded_network() {
    let graph = floor_graph(&ghost_lobby());
    for d in &ghost_lobby_wiring().devices {
        if let Some(node) = &d.node {
            assert!(
                graph.node(node).is_some(),
                "{} names missing node {node}",
                d.id
            );
        }
    }
}

#[test]
fn the_fixture_covers_every_ghost_lobby_door_camera_and_room() {
    let def = ghost_lobby();
    let net = ghost_lobby_wiring();
    let count = |k: SignalKind| net.devices.iter().filter(|d| d.kind == k).count();
    assert_eq!(count(SignalKind::Door), def.doors.len());
    assert_eq!(count(SignalKind::Camera), def.cameras.len());
    assert_eq!(count(SignalKind::Light), def.rooms.len());
}

#[test]
fn the_validator_names_every_authoring_mistake() {
    let mut def = ghost_lobby_wiring();
    def.format = "idaptik-signal/0".into();
    def.devices.push(def.devices[0].clone());
    def.devices.push(SignalDeviceDef {
        id: "gateless".into(),
        kind: SignalKind::Relay,
        label: "gateless".into(),
        node: None,
        room: None,
        gate: None,
        start_locked: true,
        hacker_only: false,
    });
    def.links.push(LinkDef {
        from: "light-hall".into(),
        from_port: OutPort::Tripped,
        to: "nope".into(),
        to_port: InPort::Open,
        invert: false,
        builtin: false,
    });
    def.links.push(def.links[0].clone());
    def.budget = 0;
    let errs = def.validate();
    let has = |want: SignalValidationError| errs.contains(&want);
    assert!(has(SignalValidationError::UnsupportedFormat {
        found: "idaptik-signal/0".into()
    }));
    assert!(has(SignalValidationError::DuplicateDevice {
        id: D1.into()
    }));
    assert!(has(SignalValidationError::RelayWithoutGate {
        device: "gateless".into()
    }));
    assert!(has(SignalValidationError::LockOnNonDoor {
        device: "gateless".into()
    }));
    assert!(has(SignalValidationError::NoSuchOutput {
        device: "light-hall".into(),
        port: OutPort::Tripped
    }));
    assert!(has(SignalValidationError::UnknownDevice {
        id: "nope".into()
    }));
    assert!(has(SignalValidationError::DuplicateLink {
        from: BILLY.into(),
        to: INTERLOCK.into()
    }));
    assert!(has(SignalValidationError::OverBudget {
        laid: 1,
        budget: 0
    }));
}

// --- the installer's wiring --------------------------------------------------

#[test]
fn billy_in_pursuit_locks_the_service_exit() {
    let mut s = sim();
    let events = run(&mut s, &high(BILLY, OutPort::Tripped), 3);
    assert!(switched(&events, D4, Switch::Locked, true));
    assert!(s.device(D4).expect("exit").locked);
}

#[test]
fn a_disabled_camera_cannot_trip_the_interlock() {
    let mut s = sim();
    let all = Access::everyone(s.definition());
    // The hacker wires the bridge to disable the hall camera once compromised;
    // then the camera "sees" the infiltrator.
    s.apply(
        &wire(
            Seat::Hacker,
            BRIDGE,
            OutPort::Compromised,
            HALL_CAM,
            InPort::Disable,
        ),
        &all,
    );
    // A signal crosses one wire per tick: disable first, then be seen.
    let events = run(&mut s, &high(BRIDGE, OutPort::Compromised), 2);
    assert!(switched(&events, HALL_CAM, Switch::Active, false));
    let mut r = high(BRIDGE, OutPort::Compromised);
    r.set(HALL_CAM, OutPort::Tripped);
    let events = run(&mut s, &r, 5);
    assert!(!switched(&events, D4, Switch::Locked, true));
}

#[test]
fn edge_inputs_fire_once_however_long_the_wire_is_held() {
    let mut s = sim();
    let all = Access::everyone(s.definition());
    s.apply(
        &wire(
            Seat::Hacker,
            BRIDGE,
            OutPort::Compromised,
            D1,
            InPort::Toggle,
        ),
        &all,
    );
    let events = run(&mut s, &high(BRIDGE, OutPort::Compromised), 120);
    assert_eq!(count_switched(&events, D1, Switch::Open, true), 1);
    assert_eq!(count_switched(&events, D1, Switch::Open, false), 0);
}

#[test]
fn a_pulse_holds_a_door_for_the_routed_hold_duration() {
    let mut s = sim();
    let all = Access::everyone(s.definition());
    let pulse = s.definition().tuning.door_pulse_ticks;
    assert_eq!(pulse, 192, "ROUTE_DURATION 3.2 s at 60 Hz");
    s.apply(
        &wire(
            Seat::Hacker,
            BRIDGE,
            OutPort::Compromised,
            D1,
            InPort::Pulse,
        ),
        &all,
    );
    let events = run(&mut s, &high(BRIDGE, OutPort::Compromised), pulse + 5);
    assert_eq!(count_switched(&events, D1, Switch::Open, true), 1);
    assert_eq!(count_switched(&events, D1, Switch::Open, false), 1);
    assert!(!s.device(D1).expect("door").open);
}

#[test]
fn a_wire_into_the_vacuum_asks_the_host_to_route_it() {
    let mut s = sim();
    let all = Access::everyone(s.definition());
    s.apply(
        &wire(
            Seat::Infiltrator,
            HALL_CAM,
            OutPort::Tripped,
            "vac-0",
            InPort::Pulse,
        ),
        &all,
    );
    // The same trip also reaches the interlock; only the vacuum matters here.
    let routed: Vec<_> = run(&mut s, &high(HALL_CAM, OutPort::Tripped), 4)
        .into_iter()
        .filter(|e| matches!(e, SignalEvent::VacuumRouted { .. }))
        .collect();
    assert_eq!(
        routed,
        vec![SignalEvent::VacuumRouted {
            vacuum: "vac-0".into()
        }]
    );
}

// --- access, skill and cost ----------------------------------------------------

#[test]
fn a_seat_wires_only_what_it_can_touch_at_the_receiving_end() {
    let mut s = sim();
    // The infiltrator is in the kitchen; the hacker has pivoted onto the bridge.
    let access = Access {
        infiltrator: only(&["light-kitchen"]),
        hacker: only(&[BRIDGE, INTERLOCK]),
    };
    let to_d1 = |by| wire(by, BRIDGE, OutPort::Compromised, D1, InPort::Open);
    for by in [Seat::Infiltrator, Seat::Hacker] {
        assert_eq!(
            denied(&s.apply(&to_d1(by), &access)),
            Some(&SignalDenyReason::NoAccess { device: D1.into() })
        );
    }
    // In the kitchen, the infiltrator can wire the kitchen lights by hand.
    let kitchen = wire(
        Seat::Infiltrator,
        HALL_CAM,
        OutPort::Tripped,
        "light-kitchen",
        InPort::Disable,
    );
    assert!(matches!(
        s.apply(&kitchen, &access).as_slice(),
        [SignalEvent::Wired { .. }]
    ));
}

#[test]
fn both_seats_can_wire_but_they_pay_differently() {
    let mut s = sim();
    let all = Access::everyone(s.definition());
    let cost = |events: &[SignalEvent]| match events {
        [SignalEvent::Wired { cost, .. }] => *cost,
        other => panic!("expected a wire, got {other:?}"),
    };
    let by_hand = cost(&s.apply(
        &wire(
            Seat::Infiltrator,
            BRIDGE,
            OutPort::Compromised,
            D1,
            InPort::Open,
        ),
        &all,
    ));
    let remote = cost(&s.apply(
        &wire(
            Seat::Hacker,
            BRIDGE,
            OutPort::Compromised,
            "door-d2",
            InPort::Open,
        ),
        &all,
    ));
    assert_eq!(
        by_hand,
        WorkCost {
            bandwidth: 0,
            busy_ticks: 90
        }
    );
    assert_eq!(
        remote,
        WorkCost {
            bandwidth: 10,
            busy_ticks: 0
        }
    );
}

#[test]
fn only_the_hacker_wires_into_a_hacker_only_device() {
    let mut s = sim();
    let all = Access::everyone(s.definition());
    let into_relay = |by| wire(by, BRIDGE, OutPort::Compromised, INTERLOCK, InPort::A);
    assert_eq!(
        denied(&s.apply(&into_relay(Seat::Infiltrator), &all)),
        Some(&SignalDenyReason::HackerOnly {
            device: INTERLOCK.into()
        })
    );
    assert!(matches!(
        s.apply(&into_relay(Seat::Hacker), &all).as_slice(),
        [SignalEvent::Wired { .. }]
    ));
}

#[test]
fn only_the_hacker_can_reprogram_a_relay() {
    let mut s = sim();
    let all = Access::everyone(s.definition());
    let re = |by| SignalCommand::Reprogram {
        by,
        relay: INTERLOCK.into(),
        gate: Gate::And,
    };
    assert_eq!(
        denied(&s.apply(&re(Seat::Infiltrator), &all)),
        Some(&SignalDenyReason::Unskilled)
    );
    assert!(matches!(
        s.apply(&re(Seat::Hacker), &all).as_slice(),
        [SignalEvent::Reprogrammed {
            gate: Gate::And,
            ..
        }]
    ));
    // Now Billy alone no longer locks the exit: AND needs the camera too.
    let events = run(&mut s, &high(BILLY, OutPort::Tripped), 5);
    assert!(!switched(&events, D4, Switch::Locked, true));

    let not_relay = SignalCommand::Reprogram {
        by: Seat::Hacker,
        relay: D1.into(),
        gate: Gate::Or,
    };
    assert_eq!(
        denied(&s.apply(&not_relay, &all)),
        Some(&SignalDenyReason::NotARelay { device: D1.into() })
    );
}

#[test]
fn hardwired_wires_cannot_be_cut_and_the_budget_is_shared() {
    let mut s = sim();
    let all = Access::everyone(s.definition());
    let cut = SignalCommand::Cut {
        by: Seat::Hacker,
        ends: WireEnds::new(INTERLOCK, OutPort::Q, D4, InPort::Lock),
    };
    assert_eq!(
        denied(&s.apply(&cut, &all)),
        Some(&SignalDenyReason::Hardwired)
    );

    let w = |by, to: &str| wire(by, BRIDGE, OutPort::Compromised, to, InPort::Open);
    s.apply(&w(Seat::Infiltrator, D1), &all);
    s.apply(&w(Seat::Hacker, "door-d2"), &all);
    s.apply(&w(Seat::Hacker, "door-d3"), &all);
    assert_eq!(s.wires_laid(), 3);
    assert_eq!(
        denied(&s.apply(&w(Seat::Hacker, D4), &all)),
        Some(&SignalDenyReason::BudgetFull { budget: 3 })
    );
    assert_eq!(
        denied(&s.apply(&w(Seat::Infiltrator, D1), &all)),
        Some(&SignalDenyReason::AlreadyWired)
    );

    // Cutting either seat's wire frees a slot for the other.
    let cut_theirs = SignalCommand::Cut {
        by: Seat::Infiltrator,
        ends: WireEnds::new(BRIDGE, OutPort::Compromised, "door-d2", InPort::Open),
    };
    assert!(matches!(
        s.apply(&cut_theirs, &all).as_slice(),
        [SignalEvent::Cut { .. }]
    ));
    assert!(matches!(
        s.apply(&w(Seat::Hacker, D4), &all).as_slice(),
        [SignalEvent::Wired { .. }]
    ));
}

#[test]
fn an_inverted_wire_carries_the_negation() {
    let mut s = sim();
    let all = Access::everyone(s.definition());
    let ends = WireEnds::new(BRIDGE, OutPort::Compromised, D1, InPort::Open);
    s.apply(
        &SignalCommand::Wire {
            by: Seat::Hacker,
            ends: ends.clone(),
        },
        &all,
    );
    assert!(matches!(
        s.apply(&SignalCommand::Invert { by: Seat::Hacker, ends }, &all).as_slice(),
        [SignalEvent::Inverted { link, .. }] if link.invert
    ));
    // Bridge not compromised, inverted wire: the door opens anyway.
    let events = run(&mut s, &idle(), 2);
    assert!(switched(&events, D1, Switch::Open, true));
}

#[test]
fn a_wire_into_a_port_the_device_lacks_is_refused() {
    let mut s = sim();
    let all = Access::everyone(s.definition());
    let events = s.apply(
        &wire(
            Seat::Hacker,
            BRIDGE,
            OutPort::Compromised,
            BILLY,
            InPort::Open,
        ),
        &all,
    );
    assert!(matches!(
        denied(&events),
        Some(SignalDenyReason::BadWire { .. })
    ));
}

// --- relays --------------------------------------------------------------------

/// A terminal into a relay into a light.
fn relay_rig(gate: Gate) -> SignalSim {
    let dev = |id: &str, kind| SignalDeviceDef {
        id: id.into(),
        kind,
        label: id.into(),
        node: None,
        room: None,
        gate: (kind == SignalKind::Relay).then_some(gate),
        start_locked: false,
        hacker_only: false,
    };
    let link = |from: &str, fp, to: &str, tp| LinkDef {
        from: from.into(),
        from_port: fp,
        to: to.into(),
        to_port: tp,
        invert: false,
        builtin: true,
    };
    SignalSim::new(SignalNetworkDef {
        format: SIGNAL_FORMAT.into(),
        network_id: "rig".into(),
        name: "rig".into(),
        devices: vec![
            dev("T", SignalKind::Terminal),
            dev("R", SignalKind::Relay),
            dev("L", SignalKind::Light),
        ],
        links: vec![
            link("T", OutPort::Compromised, "R", InPort::A),
            link("R", OutPort::Q, "L", InPort::Toggle),
        ],
        budget: 0,
        tuning: Default::default(),
    })
    .expect("rig validates")
}

#[test]
fn every_gate_computes_its_truth_table() {
    let table = |g: Gate| -> Vec<bool> {
        [(false, false), (false, true), (true, false), (true, true)]
            .iter()
            .map(|&(a, b)| g.eval(a, b, false))
            .collect()
    };
    assert_eq!(table(Gate::And), [false, false, false, true]);
    assert_eq!(table(Gate::Or), [false, true, true, true]);
    assert_eq!(table(Gate::Xor), [false, true, true, false]);
    assert_eq!(table(Gate::Nand), [true, true, true, false]);
    assert_eq!(table(Gate::Nor), [true, false, false, false]);
}

#[test]
fn a_latch_flips_on_each_rising_edge_not_each_tick() {
    let mut s = relay_rig(Gate::Latch);
    let on = high("T", OutPort::Compromised);
    let toggles = |events: &[SignalEvent]| {
        events
            .iter()
            .filter(|e| matches!(e, SignalEvent::Switched { device, .. } if device.as_str() == "L"))
            .count()
    };
    // Edge 1 (held 20 ticks): latch sets, Q rises once, the light toggles once.
    assert_eq!(toggles(&run(&mut s, &on, 20)), 1);
    assert_eq!(toggles(&run(&mut s, &idle(), 5)), 0);
    // Edge 2: latch clears, Q falls — a falling edge does not toggle.
    assert_eq!(toggles(&run(&mut s, &on, 20)), 0);
    assert_eq!(toggles(&run(&mut s, &idle(), 5)), 0);
    // Edge 3: Q rises again.
    assert_eq!(toggles(&run(&mut s, &on, 20)), 1);
}

/// A readout that holds `ids` inert: no supply, damaged, warming up — the
/// device cannot operate whatever its wires say.
fn unpowered(ids: &[&str]) -> Readout {
    let mut r = Readout::default();
    for id in ids {
        r.unpower(id);
    }
    r
}

#[test]
fn a_pulse_interrupted_by_an_outage_finishes_when_power_returns() {
    // A momentary pulse opens a normally closed door. The supply dies halfway
    // through it. The mechanism cannot move while it is dark — but the *order*
    // it was given is still standing, so the door must end up closed once it
    // has current again. Clearing the countdown and the saved state together
    // would leave a pulse as an indefinitely open door.
    let mut s = sim();
    let all = Access::everyone(s.definition());
    let pulse = s.definition().tuning.door_pulse_ticks;
    s.apply(
        &wire(
            Seat::Hacker,
            BRIDGE,
            OutPort::Compromised,
            D1,
            InPort::Pulse,
        ),
        &all,
    );
    let held = high(BRIDGE, OutPort::Compromised);
    run(&mut s, &held, 3);
    assert!(s.device(D1).expect("door").open, "the pulse opened it");

    // Dark for the rest of the pulse and well beyond it.
    run(&mut s, &unpowered(&[D1]), pulse + 10);
    // Still open: a mechanism with no current has not finished travelling.
    assert!(s.device(D1).expect("door").open);

    // Current returns with the input low; the interrupted pulse completes.
    run(&mut s, &idle(), pulse + 5);
    assert!(
        !s.device(D1).expect("door").open,
        "the pulse ran out and the door went back to closed"
    );
}

#[test]
fn a_latch_does_not_toggle_on_power_up() {
    // A latch relay latches on an A rising edge. Here the edge arrives while
    // the relay is dark and the input stays high until supply returns. On
    // restore there is no new edge, so the latch must not move: the cached
    // input history has to be synchronised while the coil is inert.
    let mut s = relay_rig(Gate::Latch);
    let on = high("T", OutPort::Compromised);
    let toggles = |events: &[SignalEvent]| {
        events
            .iter()
            .filter(|e| matches!(e, SignalEvent::Switched { device, .. } if device.as_str() == "L"))
            .count()
    };
    assert_eq!(toggles(&run(&mut s, &on, 5)), 1, "the first edge latches");

    // Dark, and T stays high throughout: an edge that arrives while inert.
    let mut dark = on.clone();
    dark.unpower("R");
    run(&mut s, &dark, 5);

    // Supply returns with T still high: no edge, no toggle.
    assert_eq!(toggles(&run(&mut s, &on, 20)), 0, "no phantom edge");
}

#[test]
fn signals_cross_one_wire_per_tick() {
    let mut s = relay_rig(Gate::Or);
    let on = high("T", OutPort::Compromised);
    // t0: the relay samples the terminal. t1: Q is high, the light toggles.
    assert!(s.step(&on).is_empty());
    assert_eq!(
        s.step(&on),
        vec![SignalEvent::Switched {
            device: "L".into(),
            what: Switch::Active,
            on: false
        }]
    );
}

// --- determinism -------------------------------------------------------------------

/// A scripted run: commands at fixed ticks, the world changing underneath.
fn scripted(s: &mut SignalSim, from: u32, to: u32) -> Vec<SignalEvent> {
    let all = Access::everyone(s.definition());
    let mut out = Vec::new();
    for t in from..to {
        match t {
            5 => out.extend(s.apply(
                &wire(
                    Seat::Hacker,
                    BRIDGE,
                    OutPort::Compromised,
                    D1,
                    InPort::Pulse,
                ),
                &all,
            )),
            9 => out.extend(s.apply(
                &wire(
                    Seat::Infiltrator,
                    HALL_CAM,
                    OutPort::Tripped,
                    "light-hall",
                    InPort::Toggle,
                ),
                &all,
            )),
            30 => out.extend(s.apply(
                &SignalCommand::Reprogram {
                    by: Seat::Hacker,
                    relay: INTERLOCK.into(),
                    gate: Gate::Latch,
                },
                &all,
            )),
            _ => {}
        }
        let mut r = Readout::default();
        if (10..300).contains(&t) {
            r.set(BRIDGE, OutPort::Compromised);
        }
        if (20..60).contains(&t) || (80..90).contains(&t) {
            r.set(HALL_CAM, OutPort::Tripped);
        }
        if (40..70).contains(&t) {
            r.set(BILLY, OutPort::Tripped);
        }
        out.extend(s.step(&r));
    }
    out
}

#[test]
fn a_run_is_a_pure_function_of_its_commands() {
    let a = scripted(&mut sim(), 0, 300);
    let b = scripted(&mut sim(), 0, 300);
    assert!(a.len() > 5);
    assert_eq!(a, b);
}

#[test]
fn a_restored_snapshot_continues_identically() {
    let mut straight = sim();
    let mut whole = scripted(&mut straight, 0, 45);
    whole.extend(scripted(&mut straight, 45, 300));

    let mut first = sim();
    let mut split = scripted(&mut first, 0, 45);
    let json = serde_json::to_string(&first.snapshot()).expect("serialise");
    let snap: SignalSnapshot = serde_json::from_str(&json).expect("parse");
    let mut resumed = SignalSim::restore(snap).expect("restores");
    split.extend(scripted(&mut resumed, 45, 300));

    assert_eq!(whole, split);
    assert_eq!(straight.state(), resumed.state());
}

#[test]
fn a_snapshot_with_the_wrong_format_is_refused() {
    let mut snap = sim().snapshot();
    snap.format = "idaptik-signal-snapshot/0".into();
    assert!(SignalSim::restore(snap).is_err());
}

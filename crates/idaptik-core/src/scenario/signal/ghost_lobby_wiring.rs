//! The Ghost Lobby floor's devices as a signal network — a fixture, not
//! authored level design.
//!
//! Every device is derived from the canonical [`ghost_lobby`] definition and
//! carries the grounded-network node id [`crate::scenario::floor_graph`]
//! already gives it: one door per scenario door, one camera per scenario
//! camera, one light per room, the vacuum, Billy, and the maintenance bridge
//! (`bms-bridge`) as the terminal. One relay sits on the bridge so relays have
//! something to exercise.
//!
//! The installer's wiring is deliberately minimal and illustrative: the bridge
//! relay ORs "Billy is pursuing" with "the hall camera has tripped" and locks
//! the service exit. Real wiring for a real floor belongs to level design.

use super::{
    Gate, InPort, LinkDef, OutPort, SIGNAL_FORMAT, SignalDeviceDef, SignalKind, SignalNetworkDef,
    SignalTuning,
};
use crate::scenario::floor_graph::{VACUUM_NODE_ID, camera_node_id, door_node_id, light_node_id};
use crate::scenario::ghost_lobby::ghost_lobby;

/// The committed golden of [`ghost_lobby_wiring`].
pub const GHOST_LOBBY_WIRING_JSON: &str = include_str!("ghost_lobby_wiring.json");

/// Signal id of Billy.
pub const BILLY: &str = "billy";
/// Signal id (and node id) of the maintenance bridge terminal.
pub const BRIDGE: &str = "bms-bridge";
/// Signal id of the bridge's interlock relay.
pub const INTERLOCK: &str = "bms-interlock";

fn device(id: String, kind: SignalKind, label: &str) -> SignalDeviceDef {
    SignalDeviceDef {
        id: id.into(),
        kind,
        label: label.to_owned(),
        node: None,
        room: None,
        gate: None,
        start_locked: false,
        hacker_only: false,
    }
}

fn hardwired(from: &str, from_port: OutPort, to: &str, to_port: InPort) -> LinkDef {
    LinkDef {
        from: from.into(),
        from_port,
        to: to.into(),
        to_port,
        invert: false,
        builtin: true,
    }
}

/// The Ghost Lobby floor's signal network. Signal ids equal node ids wherever
/// the device has one.
pub fn ghost_lobby_wiring() -> SignalNetworkDef {
    let def = ghost_lobby();
    let mut devices = Vec::new();

    for door in &def.doors {
        let node = door_node_id(door.id.clone());
        // A door stands on the boundary it opens onto: the room starting there.
        let room = def
            .rooms
            .iter()
            .find(|r| (r.x - door.x).abs() < 1e-6)
            .map(|r| r.id.as_str().to_owned());
        devices.push(SignalDeviceDef {
            node: Some(node.clone()),
            room,
            ..device(node, SignalKind::Door, &door.label)
        });
    }
    for (i, cam) in def.cameras.iter().enumerate() {
        let node = camera_node_id(i);
        devices.push(SignalDeviceDef {
            node: Some(node.clone()),
            room: Some(cam.room.as_str().to_owned()),
            ..device(node, SignalKind::Camera, cam.id.as_str())
        });
    }
    for room in &def.rooms {
        let node = light_node_id(room.id.as_str());
        devices.push(SignalDeviceDef {
            node: Some(node.clone()),
            room: Some(room.id.as_str().to_owned()),
            ..device(node, SignalKind::Light, &room.name)
        });
    }
    devices.push(SignalDeviceDef {
        node: Some(VACUUM_NODE_ID.to_owned()),
        ..device(
            VACUUM_NODE_ID.to_owned(),
            SignalKind::Vacuum,
            "Robot vacuum",
        )
    });
    devices.push(device(BILLY.to_owned(), SignalKind::Guard, "Billy"));
    devices.push(SignalDeviceDef {
        node: Some(BRIDGE.to_owned()),
        ..device(
            BRIDGE.to_owned(),
            SignalKind::Terminal,
            "Maintenance bridge",
        )
    });
    devices.push(SignalDeviceDef {
        node: Some(BRIDGE.to_owned()),
        gate: Some(Gate::Or),
        hacker_only: true,
        ..device(INTERLOCK.to_owned(), SignalKind::Relay, "Bridge interlock")
    });

    let hall_cam = camera_node_id(0);
    let exit_door = def
        .doors
        .last()
        .map(|d| door_node_id(d.id.clone()))
        .unwrap_or_default();

    SignalNetworkDef {
        format: SIGNAL_FORMAT.to_owned(),
        network_id: "ghost-lobby-wiring".to_owned(),
        name: format!("{} — signal network", def.name),
        devices,
        links: vec![
            hardwired(BILLY, OutPort::Tripped, INTERLOCK, InPort::A),
            hardwired(&hall_cam, OutPort::Tripped, INTERLOCK, InPort::B),
            hardwired(INTERLOCK, OutPort::Q, &exit_door, InPort::Lock),
        ],
        budget: 3,
        tuning: SignalTuning::default(),
    }
}

/// The canonical Ghost Lobby with its signal network attached: the floor the
/// frontends play when wiring is on. [`ghost_lobby`] itself stays unwired, so
/// its golden and every recorded replay are untouched.
pub fn ghost_lobby_wired() -> crate::scenario::definition::ScenarioDefinition {
    use crate::scenario::definition::{ConsoleCap, ConsoleDef, ConsolePlace};
    crate::scenario::definition::ScenarioDefinition {
        signal: Some(ghost_lobby_wiring()),
        // The played floor gives a new player time to learn the controls.
        arrival_clock: crate::scenario::definition::ArrivalClock::LeftSpawnRoom,
        consoles: vec![
            // The hacker's own kit: everything they can reach, all of it.
            ConsoleDef {
                id: "van-deck".to_owned(),
                label: "Van deck".to_owned(),
                place: ConsolePlace::Van,
                caps: vec![
                    ConsoleCap::Building,
                    ConsoleCap::Cameras,
                    ConsoleCap::Wiring,
                ],
            },
            // An office PC on the automation segment: building systems and the
            // panel wiring, but no camera feeds — those are on another segment.
            // At the quiet end of the office, clear of the USB's spawn range.
            ConsoleDef {
                id: "office-pc".to_owned(),
                label: "Office PC".to_owned(),
                place: ConsolePlace::Room {
                    room: "office".into(),
                    x: 548.0,
                },
                caps: vec![ConsoleCap::Building, ConsoleCap::Wiring],
            },
        ],
        ..ghost_lobby()
    }
}

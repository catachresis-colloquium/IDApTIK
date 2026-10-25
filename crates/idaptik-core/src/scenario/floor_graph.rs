//! The Ghost Lobby floor's grounded graph, derived from the scenario definition.
//!
//! The rooms, doors and cameras are already authored once, in
//! [`crate::scenario::definition::ScenarioDefinition`]. Restating them as network
//! nodes would guarantee drift, so only the backbone (the segments, the upstream
//! power line, the DNS records and the `Van` vantage) is authored, in
//! config/ghost_lobby_floor.ncl; every fixture on the floor and every `Inside`
//! vantage is derived from `def` by [`floor_graph`].
use crate::device::{DeviceKind, SecurityLevel};
use crate::netsim::addressing::segment_of;
use crate::netsim::graph::{Actuation, Dependency, GroundedGraph, Node, VantageDef, VantageKind};
use crate::scenario::command::PivotTarget;
use crate::scenario::definition::ScenarioDefinition;
use crate::scenario::ids::DoorId;
use crate::scenario::sim::GhostLobbySim;
use std::net::Ipv4Addr;

/// The authored backbone, exported from config/ghost_lobby_floor.ncl. Regenerate
/// with the ignored `regenerate_ghost_lobby_floor_json` test after editing the
/// Nickel.
const GHOST_LOBBY_FLOOR_JSON: &str = include_str!("ghost_lobby_floor.json");

/// The committed, pretty-printed derived floor graph. Regenerate with the ignored
/// `regenerate_floor_graph_json` test after changing the derivation, the Nickel,
/// or the scenario definition.
pub const FLOOR_GRAPH_JSON: &str = include_str!("floor_graph.json");

/// The node id of the robot vacuum.
pub const VACUUM_NODE_ID: &str = "vac-0";

/// The node every derived fixture draws its power from. It in turn draws from the
/// substation, as authored, so cutting the substation cascades to the whole floor.
const FEED_NODE_ID: &str = "feed";

/// The segment the cameras sit on.
const CAMERA_SEGMENT: &str = "security";

/// The segment the robot vacuum sits on.
const VACUUM_SEGMENT: &str = "auto-hall";

/// Host octet of a room's lights within its automation subnet.
const LIGHT_HOST: usize = 20;

/// Host octet of a water zone's valve within its room's automation subnet
/// (a floor with an `InfraDef` only; well clear of the door, light and entry
/// octets the canonical derivation already allocates).
const VALVE_HOST: usize = 50;

/// Host octet of a water zone's sump pump within its room's automation subnet.
const PUMP_HOST: usize = 51;

/// Host octet of the first camera within the security subnet. Public so that
/// [`crate::scenario::definition::ScenarioDefinition::validate`] derives its
/// camera capacity from the same constant the derivation addresses by.
pub const CAMERA_HOST_BASE: usize = 30;

/// Host octet base of a room's doors within its automation subnet (the first
/// door in a room lands on `DOOR_HOST_BASE + 1`). Public for the same reason as
/// [`CAMERA_HOST_BASE`]: validation and derivation must not drift apart.
pub const DOOR_HOST_BASE: usize = 10;

/// The largest usable host octet in a dotted /24-style subnet prefix.
const MAX_HOST_OCTET: usize = 254;

/// The most cameras a definition can carry before camera addressing would
/// overflow the security subnet's host octet.
pub const MAX_CAMERAS: usize = MAX_HOST_OCTET - CAMERA_HOST_BASE + 1;

/// The most doors one room can carry before door addressing would overflow the
/// room's automation subnet host octet.
pub const MAX_DOORS_PER_ROOM: usize = MAX_HOST_OCTET - DOOR_HOST_BASE;

/// Host octet of the robot vacuum within its subnet.
const VACUUM_HOST: usize = 40;

/// Host octet an `Inside` agent enters their room's automation subnet on.
const INSIDE_ENTRY_HOST: usize = 2;

/// Physical-discovery risk of playing from inside the building.
const INSIDE_RISK: u8 = 85;

/// The automation segment serving a room.
fn room_segment_id(room_id: &str) -> String {
    format!("auto-{room_id}")
}

/// An address built from a segment's dotted subnet prefix and a host octet, or
/// `None` if the octet overflows the subnet (or the prefix is malformed).
/// Deliberately non-panicking: [`ScenarioDefinition::validate`] rejects
/// definitions that would overflow (see [`MAX_CAMERAS`] /
/// [`MAX_DOORS_PER_ROOM`]), and this is the defence in depth behind it, so a
/// definition that slips past validation degrades to a missing node rather than
/// a panic in the sim.
fn host_ip(subnet: &str, host: usize) -> Option<Ipv4Addr> {
    if host > MAX_HOST_OCTET {
        return None;
    }
    format!("{subnet}{host}").parse().ok()
}

/// A derived fixture: a weakly secured, actuable device drawing from the feed.
fn fixture(
    id: String,
    name: String,
    ip: Ipv4Addr,
    segment: String,
    kind: DeviceKind,
    actuation: Actuation,
) -> Node {
    passive_fixture(id, name, ip, segment, kind, Some(actuation))
}

/// A derived fixture whose actuation is optional: passive plant (a sump pump)
/// draws from the feed exactly like an actuable one — that is the whole point,
/// since the feed's cascade is what fells it.
fn passive_fixture(
    id: String,
    name: String,
    ip: Ipv4Addr,
    segment: String,
    kind: DeviceKind,
    actuation: Option<Actuation>,
) -> Node {
    Node {
        id,
        name,
        ip,
        segment,
        kind,
        security: SecurityLevel::Weak,
        actuation,
        deps: vec![Dependency {
            on: FEED_NODE_ID.into(),
        }],
    }
}

/// The host a [`PivotTarget`] names, as authored in config/ghost_lobby_floor.ncl.
///
/// The floor offers two lines and they are not the same depth. The bridge is one
/// hop and opens every fixture in the building. The substation is two: out to the
/// ISP's operations host first, and only from the grid jump host beyond it does
/// the utility segment answer. A player given only the first hop of the upstream
/// line can never reach the substation, so all three footholds are named here.
pub fn pivot_host(target: PivotTarget) -> &'static str {
    match target {
        PivotTarget::Bridge => "bridge.local",
        PivotTarget::IspOps => "ops.isp.net",
        PivotTarget::GridJump => "jump.grid.local",
    }
}

/// The node id of a door.
pub fn door_node_id(id: DoorId) -> String {
    format!("door-{}", id.as_str().to_lowercase())
}

/// The node id of a room's lights.
pub fn light_node_id(room_id: &str) -> String {
    format!("light-{room_id}")
}

/// The node id of the camera at `index` in the definition's camera list.
pub fn camera_node_id(index: usize) -> String {
    format!("cam-{index}")
}

/// The node id of a water zone's valve — the node a deliberate flood hacks.
pub fn valve_node_id(zone_id: &str) -> String {
    format!("valve-{zone_id}")
}

/// The node id of a water zone's pump — the node whose power loss fails the
/// zone's valve open (when the zone declares `fail_open`).
pub fn pump_node_id(zone_id: &str) -> String {
    format!("pump-{zone_id}")
}

/// Deserialise the authored backbone.
fn authored() -> GroundedGraph {
    serde_json::from_str(GHOST_LOBBY_FLOOR_JSON).expect("embedded ghost_lobby_floor.json is valid")
}

/// The floor's grounded graph: the authored network structure, plus one node
/// per door, light, camera and the vacuum, derived from `def`.
/// Declared water zones add an actuable valve and a passive pump on their
/// room's automation segment. Fixtures with no segment or usable address
/// are omitted.
pub fn floor_graph(def: &ScenarioDefinition) -> GroundedGraph {
    let mut graph = authored();
    let mut nodes: Vec<Node> = Vec::new();

    // A door belongs to the room its x falls in, judged exactly as the simulation
    // judges it, and is addressed by its rank among that room's doors, so two
    // doors in one room cannot collide on an address.
    let mut door_count: Vec<usize> = vec![0; def.rooms.len()];
    for door in &def.doors {
        let index = GhostLobbySim::room_index_at(def, door.x);
        let Some(room) = def.rooms.get(index) else {
            continue;
        };
        let segment_id = room_segment_id(room.id.as_str());
        let Some(segment) = graph.segment(&segment_id) else {
            continue;
        };
        door_count[index] += 1;
        let Some(ip) = host_ip(&segment.subnet, DOOR_HOST_BASE + door_count[index]) else {
            continue;
        };
        nodes.push(fixture(
            door_node_id(door.id.clone()),
            format!("{} DOOR", door.label),
            ip,
            segment_id,
            DeviceKind::SmartDoor,
            Actuation::HoldDoor,
        ));
    }

    // Every room is lit, whether or not the definition calls it lit: `lit` says
    // the lights are on, not that the fitting exists.
    for room in &def.rooms {
        let segment_id = room_segment_id(room.id.as_str());
        let Some(segment) = graph.segment(&segment_id) else {
            continue;
        };
        let Some(ip) = host_ip(&segment.subnet, LIGHT_HOST) else {
            continue;
        };
        nodes.push(fixture(
            light_node_id(room.id.as_str()),
            format!("{} LIGHTS", room.id.as_str().to_uppercase()),
            ip,
            segment_id,
            DeviceKind::Light,
            Actuation::KillLights,
        ));
    }

    // The cameras are wired back to the security segment rather than to the room
    // they watch, as a real installation is.
    if let Some(segment) = graph.segment(CAMERA_SEGMENT) {
        let subnet = segment.subnet.clone();
        for (index, camera) in def.cameras.iter().enumerate() {
            let Some(ip) = host_ip(&subnet, CAMERA_HOST_BASE + index) else {
                continue;
            };
            nodes.push(fixture(
                camera_node_id(index),
                format!("{} CAMERA", camera.room.as_str().to_uppercase()),
                ip,
                CAMERA_SEGMENT.into(),
                DeviceKind::Camera,
                Actuation::LoopCamera,
            ));
        }
    }

    if let Some(segment) = graph.segment(VACUUM_SEGMENT)
        && let Some(ip) = host_ip(&segment.subnet, VACUUM_HOST)
    {
        nodes.push(fixture(
            VACUUM_NODE_ID.into(),
            "ROBOT VACUUM".into(),
            ip,
            VACUUM_SEGMENT.into(),
            DeviceKind::Sensor,
            Actuation::RunVacuum,
        ));
    }

    // Infrastructure: one valve node and one pump node per declared water
    // zone, on the zone room's automation segment. A floor with no `InfraDef`
    // derives neither, so the committed graph golden is unchanged. The valve
    // is the deliberate-flood actuation; the pump carries no actuation at all
    // — its whole role is to *lose power* in a cascade, which the host reads
    // as the zone's fail-open release.
    if let Some(infra) = &def.infra {
        for zone in &infra.water_zones {
            let segment_id = room_segment_id(zone.room.as_str());
            let Some(segment) = graph.segment(&segment_id) else {
                continue;
            };
            if let Some(ip) = host_ip(&segment.subnet, VALVE_HOST) {
                nodes.push(fixture(
                    valve_node_id(&zone.id),
                    format!("{} WATER VALVE", zone.room.as_str().to_uppercase()),
                    ip,
                    segment_id.clone(),
                    DeviceKind::Sensor,
                    Actuation::OpenValve,
                ));
            }
            if let Some(ip) = host_ip(&segment.subnet, PUMP_HOST) {
                nodes.push(passive_fixture(
                    pump_node_id(&zone.id),
                    format!("{} SUMP PUMP", zone.room.as_str().to_uppercase()),
                    ip,
                    segment_id,
                    DeviceKind::PowerSupply,
                    None,
                ));
            }
        }
    }
    graph.nodes.extend(nodes);

    // An agent standing in a room enters the network on that room's automation
    // segment, at high physical risk.
    let mut vantages: Vec<VantageDef> = Vec::new();
    for room in &def.rooms {
        let Some(segment) = graph.segment(&room_segment_id(room.id.as_str())) else {
            continue;
        };
        let Some(entry_ip) = host_ip(&segment.subnet, INSIDE_ENTRY_HOST) else {
            continue;
        };
        vantages.push(VantageDef {
            kind: VantageKind::Inside,
            entry_ip,
            physical_risk: INSIDE_RISK,
        });
    }
    graph.vantages.extend(vantages);
    graph
}

/// The `Inside` vantage for a room id, or `None` if the room has no automation
/// segment.
pub fn inside_vantage(graph: &GroundedGraph, room_id: &str) -> Option<VantageDef> {
    // The segment records the room it serves, so match on that rather than
    // recovering the room from an id by string surgery.
    let segment = graph
        .segments
        .iter()
        .find(|s| s.location.as_deref() == Some(room_id))?;
    graph
        .vantages
        .iter()
        .find(|v| {
            v.kind == VantageKind::Inside
                && segment_of(graph, v.entry_ip).is_some_and(|s| s.id == segment.id)
        })
        .cloned()
}

/// The `Van` vantage the hacker plays from.
pub fn van_vantage(graph: &GroundedGraph) -> VantageDef {
    graph
        .vantages
        .iter()
        .find(|v| v.kind == VantageKind::Van)
        .cloned()
        .expect("the floor graph carries a Van vantage")
}

#[cfg(test)]
mod regen {
    use super::*;

    /// Rewrites the committed export of config/ghost_lobby_floor.ncl from the raw
    /// Nickel export. Seed the raw input first, then run:
    /// `nickel export config/ghost_lobby_floor.ncl --format json > /tmp/ghost_lobby_floor_raw.json`
    /// `cargo test -p idaptik-core regenerate_ghost_lobby_floor_json -- --ignored`.
    /// The embedded input is read at compile time, so regenerate the derived
    /// golden in a later invocation, never the same one.
    #[test]
    #[ignore = "regenerates the committed golden; run manually"]
    fn regenerate_ghost_lobby_floor_json() {
        let raw =
            std::fs::read_to_string("/tmp/ghost_lobby_floor_raw.json").expect("nickel export");
        let g: GroundedGraph = serde_json::from_str(&raw).expect("parse nickel json");
        let mut pretty = serde_json::to_string_pretty(&g).expect("serialize");
        pretty.push('\n');
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/scenario/ghost_lobby_floor.json"
        );
        std::fs::write(path, pretty.as_bytes()).expect("write golden");
    }

    /// Rewrite the derived golden from the current derivation. Run with
    /// `cargo test -p idaptik-core regenerate_floor_graph_json -- --ignored`.
    #[test]
    #[ignore = "regenerates the committed golden; run manually"]
    fn regenerate_floor_graph_json() {
        let def = crate::scenario::ghost_lobby::ghost_lobby();
        let json = serde_json::to_string_pretty(&floor_graph(&def)).expect("serialises");
        std::fs::write(
            concat!(env!("CARGO_MANIFEST_DIR"), "/src/scenario/floor_graph.json"),
            json + "\n",
        )
        .expect("the golden is writable");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::netsim::effect::Effect;
    use crate::netsim::{AgentSession, resolve};
    use crate::scenario::ghost_lobby::ghost_lobby;

    #[test]
    fn the_embedded_golden_matches_the_authored_graph() {
        let def = ghost_lobby();
        let built = floor_graph(&def);
        let parsed: GroundedGraph =
            serde_json::from_str(FLOOR_GRAPH_JSON).expect("the golden parses");
        assert_eq!(built, parsed);
    }

    #[test]
    fn the_authored_json_is_serde_round_trip_stable() {
        // Guards serde-canonical stability of the committed export: it must parse
        // to a graph that re-serialises identically. It does NOT check freshness
        // against config/ghost_lobby_floor.ncl; regenerate after editing the Nickel.
        let g = authored();
        let round = serde_json::to_string_pretty(&g).expect("serialize");
        assert_eq!(round.trim(), GHOST_LOBBY_FLOOR_JSON.trim());
    }

    #[test]
    fn every_room_has_a_light_and_an_inside_vantage() {
        let def = ghost_lobby();
        let g = floor_graph(&def);
        for room in &def.rooms {
            assert!(
                g.node(&light_node_id(room.id.as_str())).is_some(),
                "room {} has no light node",
                room.id
            );
            assert!(
                inside_vantage(&g, room.id.as_str()).is_some(),
                "room {} has no Inside vantage",
                room.id
            );
        }
    }

    #[test]
    fn every_door_and_camera_in_the_definition_has_a_node() {
        let def = ghost_lobby();
        let g = floor_graph(&def);
        for door in &def.doors {
            assert!(
                g.node(&door_node_id(door.id.clone())).is_some(),
                "door {:?} has no node",
                door.id
            );
        }
        for i in 0..def.cameras.len() {
            assert!(
                g.node(&camera_node_id(i)).is_some(),
                "camera {i} has no node"
            );
        }
    }

    #[test]
    fn every_named_foothold_resolves_on_the_floor() {
        // A `PivotTarget` naming a host the floor's DNS does not carry would deny
        // that key for ever, silently, exactly as a mis-named action target would.
        let def = ghost_lobby();
        let g = floor_graph(&def);
        for target in [
            PivotTarget::Bridge,
            PivotTarget::IspOps,
            PivotTarget::GridJump,
        ] {
            let host = pivot_host(target);
            let ip = resolve(&g, host).unwrap_or_else(|| panic!("{target:?} names {host}"));
            assert!(
                g.nodes.iter().any(|n| n.ip == ip),
                "{host} resolves to {ip}, which the floor graph does not carry"
            );
        }
    }

    #[test]
    fn the_two_lines_the_floor_offers_are_walkable_by_their_named_footholds() {
        // The keys the frontend binds, walked end to end through `pivot_host`
        // alone. Without the second upstream hop the substation is unreachable and
        // the whole power-line strategy would ship as a unit test nobody can play.
        let def = ghost_lobby();
        let g = floor_graph(&def);
        let substation = g.node("substation").expect("the substation exists").ip;

        let mut building = AgentSession::new(van_vantage(&g), 10_000);
        building
            .ssh(&g, pivot_host(PivotTarget::Bridge))
            .expect("the bridge is one hop from the van");
        assert!(
            building
                .reachable(&g)
                .contains(&g.node(&light_node_id("hall")).expect("hall light").ip),
            "the building line opens the floor in one hop"
        );

        let mut upstream = AgentSession::new(van_vantage(&g), 10_000);
        upstream
            .ssh(&g, pivot_host(PivotTarget::IspOps))
            .expect("the ISP ops host is one hop from the van");
        upstream
            .ssh(&g, pivot_host(PivotTarget::GridJump))
            .expect("the grid jump host is one hop from the ISP");
        assert_eq!(upstream.hops(), 2, "the upstream line is two hops deep");
        assert!(
            upstream.reachable(&g).contains(&substation),
            "and only at that depth does the substation answer"
        );
    }

    #[test]
    fn the_van_must_pivot_to_reach_any_fixture() {
        let def = ghost_lobby();
        let g = floor_graph(&def);
        let mut s = AgentSession::new(van_vantage(&g), 10_000);
        let hall_light = g
            .node(&light_node_id("hall"))
            .expect("hall light exists")
            .ip;
        assert!(
            !s.reachable(&g).contains(&hall_light),
            "the van must not reach a fixture without pivoting"
        );
        s.ssh(&g, "bridge.local")
            .expect("the van can pivot to the bridge");
        assert!(
            s.reachable(&g).contains(&hall_light),
            "pivoting through the bridge must open the floor"
        );
    }

    #[test]
    fn the_upstream_power_line_needs_two_pivots_from_the_van() {
        let def = ghost_lobby();
        let g = floor_graph(&def);
        let substation = resolve(&g, "substation.grid.local").expect("substation resolves");
        let grid_jump = resolve(&g, "jump.grid.local").expect("grid jump host resolves");
        let mut s = AgentSession::new(van_vantage(&g), 10_000);
        assert!(!s.reachable(&g).contains(&substation), "not reachable cold");

        s.ssh(&g, "ops.isp.net")
            .expect("the van can pivot to the ISP ops host");
        assert!(
            !s.reachable(&g).contains(&substation),
            "one pivot into the ISP must not yet reach the substation"
        );
        assert!(
            s.reachable(&g).contains(&grid_jump),
            "one pivot into the ISP must open the grid jump host"
        );

        s.ssh(&g, "jump.grid.local")
            .expect("the van can pivot on from the ISP to the grid jump host");
        assert!(
            s.reachable(&g).contains(&substation),
            "a second pivot through the grid jump host must open the substation"
        );

        // Depth is proven indirectly, by popping the pivot stack: two successful
        // exits, then nothing left to pop.
        assert!(s.exit(), "the grid jump pivot pops");
        assert!(s.exit(), "the ISP ops pivot pops");
        assert!(!s.exit(), "back at the van, nothing left to pop");
    }

    #[test]
    fn an_infiltrator_owns_their_own_room_and_nothing_else_locally() {
        let def = ghost_lobby();
        let g = floor_graph(&def);
        let v = inside_vantage(&g, "hall").expect("the hall has a vantage");
        let s = AgentSession::new(v, 10_000);
        assert!(
            s.is_local(&g, &light_node_id("hall")),
            "the hall light is theirs"
        );
        assert!(
            !s.is_local(&g, &light_node_id("office")),
            "the office light is not"
        );
    }

    #[test]
    fn cutting_the_substation_cascades_to_every_fixture() {
        let def = ghost_lobby();
        let g = floor_graph(&def);
        let effects = crate::netsim::apply_actuation(&g, "substation");
        assert!(effects.contains(&Effect::PowerCut("substation".into())));
        assert!(effects.contains(&Effect::DevicePowerLost(light_node_id("hall"))));
        assert!(effects.contains(&Effect::DevicePowerLost(camera_node_id(0))));
    }
}

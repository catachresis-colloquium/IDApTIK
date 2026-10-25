//! Physical effects of hacking a node, and the power cascade: cutting a supply
//! propagates a loss of power to every node that draws from it, directly or
//! transitively.
use crate::netsim::graph::{Actuation, GroundedGraph};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// A physical change in the shared world. The string is the affected node id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Effect {
    DoorHeld(String),
    LockDisengaged(String),
    CameraLooped(String),
    CameraDisabled(String),
    ElevatorCalled(String),
    LightsKilled(String),
    PowerCut(String),
    SensorMuted(String),
    VacuumRun(String),
    DevicePowerLost(String),
    /// A water zone's valve opened; the string is the valve's node id (the
    /// zone is recovered by matching it against
    /// [`crate::scenario::floor_graph::valve_node_id`], exactly as `DoorHeld`
    /// recovers its door).
    WaterReleased(String),
}

/// Map an actuation to its immediate effect on `id`, without a power cascade.
fn direct(act: Actuation, id: &str) -> Effect {
    let id = id.to_string();
    match act {
        Actuation::HoldDoor => Effect::DoorHeld(id),
        Actuation::DisengageLock => Effect::LockDisengaged(id),
        Actuation::LoopCamera => Effect::CameraLooped(id),
        Actuation::DisableCamera => Effect::CameraDisabled(id),
        Actuation::CallElevator => Effect::ElevatorCalled(id),
        Actuation::KillLights => Effect::LightsKilled(id),
        Actuation::CutPower => Effect::PowerCut(id),
        Actuation::MuteSensor => Effect::SensorMuted(id),
        Actuation::RunVacuum => Effect::VacuumRun(id),
        Actuation::OpenValve => Effect::WaterReleased(id),
    }
}

/// Apply the actuation on `node_id`: its own effect, plus power-loss on every
/// dependent node when the actuation cuts power.
pub fn apply_actuation(graph: &GroundedGraph, node_id: &str) -> Vec<Effect> {
    let Some(node) = graph.node(node_id) else {
        return vec![];
    };
    let Some(act) = node.actuation else {
        return vec![];
    };
    let mut out = vec![direct(act, node_id)];
    if act == Actuation::CutPower {
        // Reverse walk over the deps graph: seed the seen-set with the cut node
        // and grow it with every node whose deps touch the frontier. The seen-set
        // makes this terminate even on cyclic deps and visits each node once.
        let mut seen: HashSet<&str> = HashSet::from([node_id]);
        let mut frontier: Vec<&str> = vec![node_id];
        while let Some(current) = frontier.pop() {
            for other in &graph.nodes {
                if other.deps.iter().any(|d| d.on == current) && seen.insert(&other.id) {
                    frontier.push(&other.id);
                }
            }
        }
        for other in &graph.nodes {
            if other.id != node_id && seen.contains(other.id.as_str()) {
                out.push(Effect::DevicePowerLost(other.id.clone()));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{DeviceKind, SecurityLevel};
    use crate::netsim::graph::{Actuation, Dependency, GroundedGraph, Node};
    use std::net::Ipv4Addr;

    fn node(id: &str, kind: DeviceKind, act: Option<Actuation>, deps: &[&str]) -> Node {
        Node {
            id: id.into(),
            name: id.into(),
            ip: Ipv4Addr::new(10, 20, 0, 1),
            segment: "local".into(),
            kind,
            security: SecurityLevel::Weak,
            actuation: act,
            deps: deps
                .iter()
                .map(|d| Dependency { on: d.to_string() })
                .collect(),
        }
    }

    #[test]
    fn cutting_power_cascades_to_dependents() {
        let g = GroundedGraph {
            segments: vec![],
            dns: vec![],
            vantages: vec![],
            nodes: vec![
                node(
                    "substation",
                    DeviceKind::Substation,
                    Some(Actuation::CutPower),
                    &[],
                ),
                node("feed", DeviceKind::PowerStation, None, &["substation"]),
                node(
                    "maglock-door",
                    DeviceKind::SmartDoor,
                    Some(Actuation::HoldDoor),
                    &["feed"],
                ),
            ],
        };
        let effects = apply_actuation(&g, "substation");
        assert!(effects.contains(&Effect::PowerCut("substation".into())));
        // The door depends on the feed which depends on the substation: it loses power.
        assert!(effects.contains(&Effect::DevicePowerLost("maglock-door".into())));
    }

    #[test]
    fn holding_a_door_has_no_cascade() {
        let g = GroundedGraph {
            segments: vec![],
            dns: vec![],
            vantages: vec![],
            nodes: vec![node(
                "door",
                DeviceKind::SmartDoor,
                Some(Actuation::HoldDoor),
                &[],
            )],
        };
        assert_eq!(
            apply_actuation(&g, "door"),
            vec![Effect::DoorHeld("door".into())]
        );
    }

    #[test]
    fn running_the_vacuum_has_no_cascade() {
        let g = GroundedGraph {
            segments: vec![],
            dns: vec![],
            vantages: vec![],
            nodes: vec![node(
                "vac-0",
                DeviceKind::IotCamera,
                Some(Actuation::RunVacuum),
                &[],
            )],
        };
        assert_eq!(
            apply_actuation(&g, "vac-0"),
            vec![Effect::VacuumRun("vac-0".into())]
        );
    }
}

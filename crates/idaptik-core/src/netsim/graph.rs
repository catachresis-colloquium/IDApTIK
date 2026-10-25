//! The grounded-network graph: nodes (devices/systems), the segments they sit
//! in, and the data authored in Nickel. Reachability, effects, and the hacker
//! session are built over this.
use crate::device::{DeviceKind, SecurityLevel};
use crate::network::{Range, Zone};
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;

/// A physical effect hacking a node performs in the shared world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Actuation {
    HoldDoor,
    DisengageLock,
    LoopCamera,
    DisableCamera,
    CallElevator,
    KillLights,
    CutPower,
    MuteSensor,
    RunVacuum,
    /// Open a water zone's valve (a floor with an
    /// [`crate::scenario::infra::InfraDef`] derives one per zone).
    OpenValve,
}

/// A node's upstream dependency (chiefly power): the id of the node it draws from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependency {
    pub on: String,
}

/// A concrete network segment. Reachability keys on the segment; `range` places
/// it from far (WideArea) to near (LocalLan).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub id: String,
    pub range: Range,
    pub category: Zone,
    pub subnet: String,
    #[serde(default)]
    pub can_access: Vec<String>,
    #[serde(default)]
    pub location: Option<String>,
}

/// A device or system on the grounded network.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub name: String,
    pub ip: Ipv4Addr,
    pub segment: String,
    pub kind: DeviceKind,
    pub security: SecurityLevel,
    #[serde(default)]
    pub actuation: Option<Actuation>,
    #[serde(default)]
    pub deps: Vec<Dependency>,
}

/// One authoritative DNS record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DnsRecord {
    pub host: String,
    pub ip: Ipv4Addr,
}

/// Where the hacker physically is. Governs their entry node and physical risk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VantageKind {
    Base,
    InternetCafe,
    Van,
    Inside,
}

/// A vantage the level offers: an origin node and a physical-discovery risk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VantageDef {
    pub kind: VantageKind,
    pub entry_ip: Ipv4Addr,
    pub physical_risk: u8,
}

/// The whole grounded network as data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroundedGraph {
    pub segments: Vec<Segment>,
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub dns: Vec<DnsRecord>,
    #[serde(default)]
    pub vantages: Vec<VantageDef>,
}

impl GroundedGraph {
    /// Look up a node by id.
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Look up a segment by id.
    pub fn segment(&self, id: &str) -> Option<&Segment> {
        self.segments.iter().find(|s| s.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{DeviceKind, SecurityLevel};
    use crate::network::{Range, Zone};
    use std::net::Ipv4Addr;

    #[test]
    fn graph_looks_up_nodes_and_segments_by_id() {
        let g = GroundedGraph {
            segments: vec![Segment {
                id: "local-auto".into(),
                range: Range::LocalLan,
                category: Zone::Internal,
                subnet: "10.20.0.".into(),
                can_access: vec![],
                location: Some("building".into()),
            }],
            nodes: vec![Node {
                id: "door-d2".into(),
                name: "HALL/OFFICE DOOR".into(),
                ip: Ipv4Addr::new(10, 20, 0, 12),
                segment: "local-auto".into(),
                kind: DeviceKind::SmartDoor,
                security: SecurityLevel::Weak,
                actuation: Some(Actuation::HoldDoor),
                deps: vec![],
            }],
            dns: vec![],
            vantages: vec![],
        };
        assert_eq!(g.node("door-d2").unwrap().kind, DeviceKind::SmartDoor);
        assert_eq!(g.segment("local-auto").unwrap().range, Range::LocalLan);
        assert!(g.node("missing").is_none());
    }
}

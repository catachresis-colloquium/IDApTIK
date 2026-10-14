//! The stateful agent session: a vantage, a stack of pivots, an active trace
//! (reused from `crate::trace`), the effects fired, and the logs left behind.
use crate::device::{DeviceKind, SecurityLevel};
use crate::netsim::access::can_reach;
use crate::netsim::addressing::segment_of;
use crate::netsim::dns::resolve;
use crate::netsim::effect::{Effect, apply_actuation};
use crate::netsim::graph::{GroundedGraph, VantageDef};
use crate::netsim::reach::{reachable_count_from, reachable_from};
use crate::trace::Trace;
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;

/// Why a session operation failed. Serde-carrying, because a refused pivot is
/// news the player must be told: which of these it was is the difference between
/// "try the other host" and "you have to go through the ISP first".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionError {
    /// The given host or node id could not be resolved.
    Unresolved,
    /// There is no route to the target from the active host.
    NoRoute,
    /// The target node exists but cannot be pivoted through.
    NotPivotable,
    /// No node with the given id exists in the graph.
    NoSuchNode,
    /// The target is the active host or already on the pivot stack.
    AlreadyThere,
}

/// A live agent session against a grounded network: a vantage, a stack of
/// pivots, an active trace, and the logs left behind. Both players are the same
/// kind of peer; only the vantage differs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSession {
    vantage: VantageDef,
    stack: Vec<Ipv4Addr>,
    trace: Trace,
    logs: Vec<String>,
}

/// Whether a device kind can be used as a pivot foothold. General-purpose
/// hosts qualify (Desktop is the UMS-authored sibling of Laptop/Terminal);
/// infrastructure such as Router/Switch/AccessPoint and passive plant do not.
fn pivotable(kind: DeviceKind) -> bool {
    matches!(
        kind,
        DeviceKind::Server | DeviceKind::Laptop | DeviceKind::Terminal | DeviceKind::Desktop
    )
}

/// The trace cost of touching a node, scaled by how hard it is secured: a
/// hardened substation is a louder, longer fight than an open kiosk, and the
/// trace clock is where that difference is paid.
fn trace_cost(sec: SecurityLevel) -> u32 {
    match sec {
        SecurityLevel::Open => 5,
        SecurityLevel::Weak => 10,
        SecurityLevel::Medium => 20,
        SecurityLevel::Strong => 40,
    }
}

impl AgentSession {
    /// Open a session from `vantage`; the active trace trips at `trace_threshold`.
    ///
    /// No graph is taken: a session is not bound to one. Every operation on it
    /// (`ssh`, `hack`, `is_local`, `reachable`) is handed the graph afresh, which
    /// is what lets one session be read against the floor it is played on.
    pub fn new(vantage: VantageDef, trace_threshold: u32) -> Self {
        Self {
            vantage,
            stack: Vec::new(),
            trace: Trace::new(trace_threshold),
            logs: Vec::new(),
        }
    }

    /// Move the agent's physical vantage. Remote footholds do not survive a move,
    /// so the pivot stack is dropped; the trace and the logs left behind persist.
    pub fn set_vantage(&mut self, vantage: VantageDef) {
        self.vantage = vantage;
        self.stack.clear();
    }

    /// Where the agent physically is.
    pub fn vantage(&self) -> &VantageDef {
        &self.vantage
    }

    /// The host the agent is currently acting from.
    pub fn vantage_ip(&self) -> Ipv4Addr {
        *self.stack.last().unwrap_or(&self.vantage.entry_ip)
    }

    /// Whether the agent has pivoted onto `ip` and still stands on it — the
    /// host's sense of "this terminal is compromised".
    pub fn stands_on(&self, ip: Ipv4Addr) -> bool {
        self.stack.contains(&ip)
    }

    /// How deep the agent has reached: the number of pivots they stand on.
    pub fn hops(&self) -> u32 {
        self.stack.len() as u32
    }

    /// Nodes reachable from the current active host.
    pub fn reachable(&self, graph: &GroundedGraph) -> Vec<Ipv4Addr> {
        reachable_from(graph, self.vantage_ip())
    }

    /// How many nodes are reachable from the current active host. For a readout
    /// that wants only the tally, this counts them without collecting the list.
    pub fn reachable_count(&self, graph: &GroundedGraph) -> usize {
        reachable_count_from(graph, self.vantage_ip())
    }

    /// How many hops the active trace should divide its rate by right now.
    fn bounce_hops(&self) -> u32 {
        (self.stack.len() as u32) + 1
    }

    /// Pivot into `host` if reachable and pivotable, advancing the trace.
    pub fn ssh(&mut self, graph: &GroundedGraph, host: &str) -> Result<Ipv4Addr, SessionError> {
        let ip = resolve(graph, host).ok_or(SessionError::Unresolved)?;
        // Pivoting into a host already occupied would inflate bounce_hops and
        // cheaply slow the trace, so the self-pivot is rejected outright. The
        // entry host is guarded explicitly: `vantage_ip()` answers the stack top
        // once anything is stacked, which would otherwise leave the agent free to
        // ssh out and straight back into their own entry host for a free hop.
        if ip == self.vantage_ip() || ip == self.vantage.entry_ip || self.stack.contains(&ip) {
            return Err(SessionError::AlreadyThere);
        }
        if !can_reach(graph, self.vantage_ip(), ip) {
            return Err(SessionError::NoRoute);
        }
        let node = graph
            .nodes
            .iter()
            .find(|n| n.ip == ip)
            .ok_or(SessionError::NoSuchNode)?;
        if !pivotable(node.kind) {
            return Err(SessionError::NotPivotable);
        }
        self.trace
            .advance(trace_cost(node.security), self.bounce_hops());
        self.stack.push(ip);
        Ok(ip)
    }

    /// Whether `node_id` sits on the agent's own home segment. An agent fully owns
    /// their local environment: acting there is free of trace. This is judged from
    /// the home vantage, never from a pivot, so that pivoting onto a segment cannot
    /// launder later hacks there into local ones.
    pub fn is_local(&self, graph: &GroundedGraph, node_id: &str) -> bool {
        if !self.stack.is_empty() {
            return false;
        }
        let Some(node) = graph.node(node_id) else {
            return false;
        };
        let Some(home) = segment_of(graph, self.vantage.entry_ip) else {
            return false;
        };
        node.segment == home.id
    }

    /// Hack a node (actuate it) if reachable, logging the touch and tracing.
    pub fn hack(
        &mut self,
        graph: &GroundedGraph,
        node_id: &str,
    ) -> Result<Vec<Effect>, SessionError> {
        let node = graph.node(node_id).ok_or(SessionError::NoSuchNode)?;
        if !can_reach(graph, self.vantage_ip(), node.ip) {
            return Err(SessionError::NoRoute);
        }
        let local = self.is_local(graph, node_id);
        // Your own local segment is yours: no trace. Everything else traces, at a
        // rate the target's security level sets.
        if !local {
            self.trace
                .advance(trace_cost(node.security), self.bounce_hops());
        }
        self.logs.push(node_id.to_string());
        Ok(apply_actuation(graph, node_id))
    }

    /// Remove a node's log (cover your tracks).
    pub fn scrub(&mut self, node_id: &str) {
        self.logs.retain(|l| l != node_id);
    }

    /// Pop one pivot layer; false if already at the vantage.
    pub fn exit(&mut self) -> bool {
        self.stack.pop().is_some()
    }

    /// Whether the active trace has completed.
    pub fn traced(&self) -> bool {
        self.trace.traced()
    }

    /// Active-trace progress fraction.
    pub fn trace_fraction(&self) -> f32 {
        self.trace.fraction()
    }

    /// The logs left on touched nodes.
    pub fn logs(&self) -> &[String] {
        &self.logs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::{DeviceKind, SecurityLevel};
    use crate::netsim::graph::{GroundedGraph, Node, Segment, VantageDef, VantageKind};
    use crate::network::{Range, Zone};
    use std::net::Ipv4Addr;

    fn node(id: &str, ip: [u8; 4], seg: &str, kind: DeviceKind) -> Node {
        Node {
            id: id.into(),
            name: id.into(),
            ip: Ipv4Addr::from(ip),
            segment: seg.into(),
            kind,
            security: SecurityLevel::Weak,
            actuation: None,
            deps: vec![],
        }
    }
    fn seg(id: &str, subnet: &str, access: &[&str]) -> Segment {
        Segment {
            id: id.into(),
            range: Range::LocalLan,
            category: Zone::Internal,
            subnet: subnet.into(),
            can_access: access.iter().map(|s| s.to_string()).collect(),
            location: None,
        }
    }
    fn graph() -> GroundedGraph {
        GroundedGraph {
            segments: vec![
                seg("dmz", "10.0.0.", &["internal"]),
                seg("internal", "10.0.1.", &[]),
            ],
            nodes: vec![
                node("web", [10, 0, 0, 25], "dmz", DeviceKind::Server),
                node("db", [10, 0, 1, 50], "internal", DeviceKind::Server),
            ],
            dns: vec![],
            vantages: vec![],
        }
    }

    /// The UMS parity kinds split the same way the originals do: an authored
    /// Desktop is a foothold like Laptop/Terminal, while Switch/AccessPoint are
    /// infrastructure like Router and passive plant is never a foothold.
    #[test]
    fn ums_parity_kinds_split_pivotable_as_ruled() {
        assert!(pivotable(DeviceKind::Desktop));
        for kind in [
            DeviceKind::PatchPanel,
            DeviceKind::FibreHub,
            DeviceKind::PhoneSystem,
            DeviceKind::AccessPoint,
            DeviceKind::Switch,
            DeviceKind::PowerSupply,
        ] {
            assert!(!pivotable(kind), "{kind:?} must not be a pivot foothold");
        }
    }

    #[test]
    fn pivoting_opens_a_segment_that_was_out_of_reach() {
        let g = graph();
        let v = VantageDef {
            kind: VantageKind::Van,
            entry_ip: Ipv4Addr::new(10, 0, 0, 25),
            physical_risk: 40,
        };
        let mut s = AgentSession::new(v, 1000);
        // From the DMZ foothold the internal db is reachable; ssh into web first is a no-op
        // (already there), so pivot straight to db via web's segment.
        assert!(s.reachable(&g).contains(&Ipv4Addr::new(10, 0, 1, 50)));
        let landed = s
            .ssh(&g, "10.0.1.50")
            .expect("db is reachable and pivotable");
        assert_eq!(landed, Ipv4Addr::new(10, 0, 1, 50));
        assert_eq!(s.vantage_ip(), Ipv4Addr::new(10, 0, 1, 50));
        assert!(s.exit());
        assert_eq!(s.vantage_ip(), Ipv4Addr::new(10, 0, 0, 25));
        assert!(!s.exit()); // back at the vantage, nothing to pop
    }

    #[test]
    fn sshing_back_into_your_own_entry_host_is_refused() {
        // Two mutually-accessible segments, both carrying pivotable servers. Once
        // the agent has pivoted out, `vantage_ip()` answers the stack top, so the
        // entry host must be guarded explicitly: ssh-ing back into it would be a
        // free extra bounce_hop that cheaply slows the trace.
        let g = GroundedGraph {
            segments: vec![
                seg("dmz", "10.0.0.", &["internal"]),
                seg("internal", "10.0.1.", &["dmz"]),
            ],
            nodes: vec![
                node("web", [10, 0, 0, 25], "dmz", DeviceKind::Server),
                node("db", [10, 0, 1, 50], "internal", DeviceKind::Server),
            ],
            dns: vec![],
            vantages: vec![],
        };
        let v = VantageDef {
            kind: VantageKind::Van,
            entry_ip: Ipv4Addr::new(10, 0, 0, 25),
            physical_risk: 40,
        };
        let mut s = AgentSession::new(v, 1000);
        s.ssh(&g, "10.0.1.50")
            .expect("db is reachable and pivotable");
        assert_eq!(
            s.ssh(&g, "10.0.0.25"),
            Err(SessionError::AlreadyThere),
            "the agent's own entry host must never be a pivot target"
        );
    }

    #[test]
    fn a_strong_node_advances_the_trace_four_times_a_weak_one() {
        // The security level is the trace-cost dial: Strong (40) is 4x Weak (10).
        // Both hacks are made from the same remote vantage at the same depth, so
        // the security level is the only variable in the comparison.
        let mut g = graph();
        g.nodes.push({
            let mut n = node("vault", [10, 0, 1, 60], "internal", DeviceKind::Server);
            n.security = SecurityLevel::Strong;
            n
        });
        let v = VantageDef {
            kind: VantageKind::Van,
            entry_ip: Ipv4Addr::new(10, 0, 0, 2),
            physical_risk: 40,
        };
        let mut weak = AgentSession::new(v.clone(), 1000);
        weak.hack(&g, "db")
            .expect("dmz reaches the weak internal db");
        let mut strong = AgentSession::new(v, 1000);
        strong
            .hack(&g, "vault")
            .expect("dmz reaches the strong internal vault");
        assert!(weak.trace_fraction() > 0.0, "a remote hack always traces");
        assert_eq!(
            strong.trace_fraction(),
            4.0 * weak.trace_fraction(),
            "a Strong node must cost exactly four times a Weak one"
        );
    }

    #[test]
    fn moving_the_vantage_drops_the_pivot_stack() {
        let g = graph();
        let v = VantageDef {
            kind: VantageKind::Van,
            entry_ip: Ipv4Addr::new(10, 0, 0, 25),
            physical_risk: 40,
        };
        let mut s = AgentSession::new(v, 1000);
        s.ssh(&g, "10.0.1.50")
            .expect("db is reachable and pivotable");
        assert_eq!(s.vantage_ip(), Ipv4Addr::new(10, 0, 1, 50));
        // Physically moving cannot carry remote footholds with you.
        s.set_vantage(VantageDef {
            kind: VantageKind::Inside,
            entry_ip: Ipv4Addr::new(10, 0, 1, 50),
            physical_risk: 85,
        });
        assert_eq!(s.vantage().kind, VantageKind::Inside);
        assert_eq!(s.vantage_ip(), Ipv4Addr::new(10, 0, 1, 50));
        assert!(!s.exit(), "the pivot stack must be empty after moving");
    }

    #[test]
    fn a_session_round_trips_through_serde() {
        let g = graph();
        let v = VantageDef {
            kind: VantageKind::Van,
            entry_ip: Ipv4Addr::new(10, 0, 0, 25),
            physical_risk: 40,
        };
        let mut s = AgentSession::new(v, 1000);
        s.ssh(&g, "10.0.1.50")
            .expect("db is reachable and pivotable");
        let json = serde_json::to_string(&s).expect("session serialises");
        let back: AgentSession = serde_json::from_str(&json).expect("session deserialises");
        assert_eq!(back, s);
    }

    #[test]
    fn acting_on_your_own_local_segment_leaves_no_trace() {
        let mut g = graph();
        g.nodes.push(Node {
            id: "door".into(),
            name: "DOOR".into(),
            ip: Ipv4Addr::new(10, 0, 0, 30),
            segment: "dmz".into(),
            kind: DeviceKind::SmartDoor,
            security: SecurityLevel::Weak,
            actuation: Some(crate::netsim::graph::Actuation::HoldDoor),
            deps: vec![],
        });
        let v = VantageDef {
            kind: VantageKind::Inside,
            entry_ip: Ipv4Addr::new(10, 0, 0, 25),
            physical_risk: 85,
        };
        let mut s = AgentSession::new(v, 100);
        assert!(s.is_local(&g, "door"), "same segment as the vantage");
        s.hack(&g, "door").expect("a local door is hackable");
        assert_eq!(
            s.trace_fraction(),
            0.0,
            "a local hack must not advance the trace"
        );
        // The log is still left behind: locality buys no anonymity, only no trace.
        assert_eq!(s.logs(), &["door".to_string()]);
    }

    #[test]
    fn reaching_off_your_own_segment_advances_the_trace() {
        let mut g = graph();
        g.nodes.push(Node {
            id: "door".into(),
            name: "DOOR".into(),
            ip: Ipv4Addr::new(10, 0, 1, 30),
            segment: "internal".into(),
            kind: DeviceKind::SmartDoor,
            security: SecurityLevel::Weak,
            actuation: Some(crate::netsim::graph::Actuation::HoldDoor),
            deps: vec![],
        });
        let v = VantageDef {
            kind: VantageKind::Van,
            entry_ip: Ipv4Addr::new(10, 0, 0, 25),
            physical_risk: 40,
        };
        let mut s = AgentSession::new(v, 100);
        assert!(!s.is_local(&g, "door"), "the door is on another segment");
        s.hack(&g, "door").expect("dmz can reach internal");
        assert!(
            s.trace_fraction() > 0.0,
            "a remote hack must advance the trace"
        );
    }

    #[test]
    fn locality_is_judged_from_the_active_host_not_the_home_vantage() {
        // After pivoting, "local" means the segment you are standing on now. A pivot
        // into a segment must not launder later hacks there into free ones.
        let mut g = graph();
        g.nodes.push(Node {
            id: "door".into(),
            name: "DOOR".into(),
            ip: Ipv4Addr::new(10, 0, 1, 30),
            segment: "internal".into(),
            kind: DeviceKind::SmartDoor,
            security: SecurityLevel::Weak,
            actuation: Some(crate::netsim::graph::Actuation::HoldDoor),
            deps: vec![],
        });
        let v = VantageDef {
            kind: VantageKind::Van,
            entry_ip: Ipv4Addr::new(10, 0, 0, 25),
            physical_risk: 40,
        };
        let mut s = AgentSession::new(v, 1000);
        s.ssh(&g, "10.0.1.50")
            .expect("db is reachable and pivotable");
        // Standing on `internal` via a pivot, the door is on that segment. It is only
        // free when it is your OWN home segment, never one you pivoted onto.
        assert!(
            !s.is_local(&g, "door"),
            "a pivoted-onto segment is not your own"
        );
        let before = s.trace_fraction();
        s.hack(&g, "door").expect("reachable from the pivot");
        assert!(
            s.trace_fraction() > before,
            "a hack from a pivot always traces"
        );
    }

    #[test]
    fn a_pivot_suspends_your_ownership_of_your_own_segment() {
        // The guard in `is_local` is the only thing that makes this true: while the
        // agent stands on a pivot, even their own home segment is reached from
        // somewhere else, so nothing is local. Without the guard the bare segment
        // comparison would still answer "dmz == dmz" and wrongly call it local.
        let mut g = graph();
        g.nodes.push(Node {
            id: "home-door".into(),
            name: "HOME DOOR".into(),
            ip: Ipv4Addr::new(10, 0, 0, 30),
            segment: "dmz".into(),
            kind: DeviceKind::SmartDoor,
            security: SecurityLevel::Weak,
            actuation: Some(crate::netsim::graph::Actuation::HoldDoor),
            deps: vec![],
        });
        let v = VantageDef {
            kind: VantageKind::Inside,
            entry_ip: Ipv4Addr::new(10, 0, 0, 25),
            physical_risk: 85,
        };
        let mut s = AgentSession::new(v, 1000);
        assert!(
            s.is_local(&g, "home-door"),
            "at home, the door on your segment is yours"
        );
        s.ssh(&g, "10.0.1.50")
            .expect("db is reachable and pivotable");
        assert!(
            !s.is_local(&g, "home-door"),
            "standing on a pivot, even your own segment is no longer local"
        );
    }
}

// Human-readable renderings of secondary events, for the scenario log view.
use crate::scenario::common::Severity;
use crate::scenario::secondary_spaces::model::*;
use serde::{Deserialize, Serialize};

/// A typed event from the secondary-space layer, wrapped as
/// [`crate::scenario::event::Event::Secondary`] so it appears in the same log
/// as everything else the run produces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum SecondaryEvent {
    RouteEntered {
        route_id: RouteId,
    },
    RouteLeft {
        route_id: RouteId,
    },
    RouteReversed {
        route_id: RouteId,
    },
    RouteObstructed {
        route_id: Option<RouteId>,
        reason: ObstructionReason,
    },
    EntryRefused {
        route_id: Option<RouteId>,
        reason: EntryRefusalReason,
    },
    GrilleOpened {
        access_point: AccessPointId,
    },
    GrilleClosed {
        access_point: AccessPointId,
    },
    ConnectionCreated {
        from: AccessPointId,
        to: AccessPointId,
    },
    CableSabotaged {
        cable_id: CableId,
        node_id: String,
    },
    CableDamaged {
        cable_id: CableId,
    },
    SabotageRefused {
        reason: SabotageRefusalReason,
    },
    FanEncountered {
        fan: String,
    },
    ValveEncountered {
        valve: String,
        open: bool,
    },
    WaterEntered {
        at: f64,
    },
    NoiseEmitted {
        at: f64,
        loudness: f64,
    },
    VibrationDetected {
        reading: crate::companion::coprocessors::VibrationReading,
    },
    ScanComplete {
        objects: u32,
    },
    DebrisLeft {
        at: f64,
    },
    EvidenceLeft {
        at: f64,
        kind: EvidenceKind,
    },
    MoleEntrapped {
        route_id: RouteId,
    },
}

impl SecondaryEvent {
    /// Human-readable description plus severity. Ids only: the playable view
    /// should not learn about routes the mole has not discovered yet.
    pub fn describe(&self) -> Option<(String, Severity)> {
        use Severity::{Bad, Info, Warn};
        match self {
            SecondaryEvent::RouteEntered { route_id } => {
                Some((format!("Moletaire enters route {route_id}."), Info))
            }
            SecondaryEvent::RouteLeft { route_id } => {
                Some((format!("Moletaire leaves route {route_id}."), Info))
            }
            SecondaryEvent::RouteReversed { route_id } => {
                Some((format!("Moletaire reverses in route {route_id}."), Info))
            }
            SecondaryEvent::RouteObstructed { route_id, reason } => {
                let text = match route_id {
                    Some(id) => format!("Route {id} is blocked: {reason:?}."),
                    None => format!("Route blocked: {reason:?}."),
                };
                Some((text, Warn))
            }
            SecondaryEvent::EntryRefused { route_id, reason } => {
                let text = match route_id {
                    Some(id) => format!("Cannot enter route {id}: {reason:?}."),
                    None => format!("Cannot enter route: {reason:?}."),
                };
                Some((text, Warn))
            }
            SecondaryEvent::GrilleOpened { access_point } => {
                Some((format!("Grille {access_point} is now open."), Info))
            }
            SecondaryEvent::GrilleClosed { access_point } => {
                Some((format!("Grille {access_point} is now closed."), Info))
            }
            SecondaryEvent::ConnectionCreated { from, to } => {
                Some((format!("New opening: {from} <-> {to}."), Info))
            }
            SecondaryEvent::CableSabotaged { cable_id, node_id } => Some((
                format!("Moletaire chews through cable {cable_id} feeding node {node_id}."),
                Bad,
            )),
            SecondaryEvent::CableDamaged { cable_id } => {
                Some((format!("Cut cable ends at {cable_id}."), Warn))
            }
            SecondaryEvent::SabotageRefused { reason } => {
                Some((format!("Sabotage refused: {reason:?}."), Warn))
            }
            SecondaryEvent::FanEncountered { fan } => {
                Some((format!("A fan spins in the duct ahead ({fan})."), Info))
            }
            SecondaryEvent::ValveEncountered { valve, open } => Some((
                format!(
                    "Valve {valve} is {}.",
                    if *open { "open" } else { "closed" }
                ),
                Info,
            )),
            SecondaryEvent::WaterEntered { at } => {
                Some((format!("Standing water in the route at {at}."), Warn))
            }
            SecondaryEvent::NoiseEmitted { at, loudness } => {
                Some((format!("Noise at {at} (loudness {loudness:.2})."), Info))
            }
            SecondaryEvent::VibrationDetected { reading } => {
                Some((format!("Vibration above: {reading:?}."), Info))
            }
            SecondaryEvent::ScanComplete { objects } => {
                Some((format!("Route scan: {objects} objects ahead."), Info))
            }
            SecondaryEvent::DebrisLeft { at } => Some((format!("Debris left at {at}."), Info)),
            SecondaryEvent::EvidenceLeft { at, kind } => {
                Some((format!("Evidence: {kind:?} at {at}."), Warn))
            }
            SecondaryEvent::MoleEntrapped { route_id } => Some((
                format!("Moletaire is entrapped in route {route_id}: collapse is irreversible.",),
                Bad,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_event_renders_a_readable_line() {
        let cable = CableId::from("cable-substation-feed");
        let route = RouteId::from("maint-hall-service");
        let events = [
            SecondaryEvent::RouteEntered {
                route_id: route.clone(),
            },
            SecondaryEvent::RouteLeft {
                route_id: route.clone(),
            },
            SecondaryEvent::RouteReversed {
                route_id: route.clone(),
            },
            SecondaryEvent::RouteObstructed {
                route_id: Some(route.clone()),
                reason: ObstructionReason::Sealed,
            },
            SecondaryEvent::EntryRefused {
                route_id: None,
                reason: EntryRefusalReason::NotAtAccessPoint,
            },
            SecondaryEvent::GrilleOpened {
                access_point: AccessPointId::from("grille-hall"),
            },
            SecondaryEvent::GrilleClosed {
                access_point: AccessPointId::from("grille-hall"),
            },
            SecondaryEvent::ConnectionCreated {
                from: AccessPointId::from("grille-hall"),
                to: AccessPointId::from("grille-service"),
            },
            SecondaryEvent::CableSabotaged {
                cable_id: cable.clone(),
                node_id: "substation".into(),
            },
            SecondaryEvent::CableDamaged {
                cable_id: cable.clone(),
            },
            SecondaryEvent::SabotageRefused {
                reason: SabotageRefusalReason::NotUnderground,
            },
            SecondaryEvent::FanEncountered {
                fan: "fan-1".into(),
            },
            SecondaryEvent::ValveEncountered {
                valve: "valve-1".into(),
                open: true,
            },
            SecondaryEvent::WaterEntered { at: 400.0 },
            SecondaryEvent::NoiseEmitted {
                at: 400.0,
                loudness: 1.0,
            },
            SecondaryEvent::VibrationDetected {
                reading: crate::companion::coprocessors::VibrationReading::FullProfile,
            },
            SecondaryEvent::ScanComplete { objects: 3 },
            SecondaryEvent::DebrisLeft { at: 400.0 },
            SecondaryEvent::EvidenceLeft {
                at: 400.0,
                kind: EvidenceKind::CutCableEnds,
            },
            SecondaryEvent::MoleEntrapped { route_id: route },
        ];
        for event in events {
            let (text, severity) = event.describe().expect("every event renders");
            assert!(!text.is_empty(), "{event:?} rendered empty");
            assert!(
                matches!(severity, Severity::Info | Severity::Warn | Severity::Bad),
                "{event:?} has unknown severity"
            );
            let _ = severity;
        }
    }
}

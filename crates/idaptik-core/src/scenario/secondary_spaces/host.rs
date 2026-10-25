// Hosting secondary spaces inside a Ghost Lobby run.
// Mirrors `sim/signal.rs`: optional content
// ([`crate::scenario::definition::ScenarioDefinition::secondary`]), optional
// runtime state ([`crate::scenario::state::RuntimeState::secondary`]),
// driven each tick from [`GhostLobbySim::system_secondary`] and by the
// [`crate::scenario::command::Command::Mole*`] immediates. When a floor has
// no secondary spec, none of this runs and every existing replay is
// byte-identical.
use crate::companion::hunger::EdibleObject;
use crate::companion::sim::MoleEvent;
use crate::netsim::apply_actuation;
use crate::scenario::agents::apply_effects;
use crate::scenario::constants as c;
use crate::scenario::event::Event;
use crate::scenario::GhostLobbySim;
use crate::scenario::mathf::TICK_DT;
use serde::{Deserialize, Serialize};

use crate::scenario::secondary_spaces::access::{route_accessibility, RouteAccess};
use crate::scenario::secondary_spaces::model::{
    AccessPointId, AccessPointKind, AccessPointSpec, ActorClass, CableState, Clearance,
    EntryRefusalReason, EvidenceKind, ObstructionReason, RouteId, RouteSpec, RouteState,
    SabotageRefusalReason, SecondarySpec, SecondaryState, SpaceKind,
};
use crate::scenario::secondary_spaces::topology::{self, RouteNetwork};
use crate::scenario::secondary_spaces::events::SecondaryEvent;

/// Mirrors [`crate::companion::definition::moletaire().tuning.dig_min_depth`]:
/// the mole can only chew or dig while underground. Kept as a named constant
/// so the host gate reads like the rule, not a magic number.
const DIG_MIN_DEPTH: f64 = 0.1;

/// How far (canvas units) from an access point the mole may be to enter or
/// leave a route through it.
const ENTRY_REACH: f64 = 16.0;

/// Debug view of every authored route: state, actors, clearance, cost,
/// discovery, and the mole's position when he is on a route. Intended for
/// the debug route view, not the playable HUD (undiscovered routes show
/// here; playable views must filter by [`SecondaryState::discovered`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteDebugView {
    pub routes: Vec<RouteDebugEntry>,
    pub topology_version: u64,
    pub mole: Option<MoleDebug>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RouteDebugEntry {
    pub id: RouteId,
    pub kind: crate::scenario::secondary_spaces::model::SpaceKind,
    pub label: String,
    pub state: RouteState,
    pub actors: Vec<String>,
    pub clearance: Clearance,
    pub travel_cost_per_sec: f64,
    pub connects_node: Option<String>,
    pub discovered: bool,
    pub mole_on_route: bool,
    pub access_points: Vec<AccessPointDebug>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccessPointDebug {
    pub id: AccessPointId,
    pub kind: AccessPointKind,
    pub room: Option<String>,
    pub x: f64,
    pub sealed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoleDebug {
    pub x: f64,
    pub depth: f64,
    pub hunger: f64,
    pub state: String,
    pub underground: bool,
}

impl GhostLobbySim {
    /// One fixed tick of the secondary-space layer. Runs after signals and
    /// before the player system, so a severed cable's power cascade lands in
    /// the main scenario the same tick the mole finishes chewing.
    pub(crate) fn system_secondary(&mut self) {
        if self.paused || self.state.ended {
            return;
        }
        let Some(net) = self.def.secondary.as_ref() else {
            return;
        };
        let mut st = match self.state.secondary.take() {
            Some(st) => st,
            None => return,
        };

        let mut events: Vec<Event> = Vec::new();
        let mut pending_cut: Option<String> = None;

        // Decay the noise the mole leaves behind (digging, sabotage).
        st.noise = (st.noise - c::NOISE_DECAY * TICK_DT).max(0.0);

        // No edibles in the secondary spaces: Moletaire's hunger still ticks
        // (and starves on nothing), which is why hunger must survive the run.
        const NO_EDIBLES: &[EdibleObject] = &[];

        // 1. Tick the mole sim itself: hunger, movement, scans, the
        //    sabotage timer. Nothing here touches the main scenario yet.
        let mole_events = st.mole.tick(TICK_DT, NO_EDIBLES);
        for ev in mole_events {
            if let Some(node_id) =
                secondary_mole_event(net, &mut st, &mut events, ev)
            {
                pending_cut = Some(node_id);
            }
        }

        // 2. Route transitions: entering, leaving, reversing, obstruction,
        //    and inline hazards (fans, valves, water, damaged cables).
        secondary_transit(net, &mut st, &mut events);

        self.state.secondary = Some(st);

        // 3. A severed cable cuts its node: apply_actuation runs the power
        //    cascade, and apply_effects turns it into main-scenario events
        //    (dead nodes, darkened lights, silence) this tick.
        if let Some(node_id) = pending_cut.take() {
            let effects = apply_actuation(&self.graph, &node_id);
            let effect_events =
                apply_effects(&self.def, &mut self.state, self.cfg, &effects);
            self.events.extend(effect_events);
        }

        self.events.extend(events);
    }

    /// The mole enters the maintenance route at his current position, which
    /// must be at an authored access point on an open route. Orders movement
    /// underground to the route's far end;
    /// [`SecondaryEvent::RouteEntered`] fires immediately so the playable
    /// view learns the route the moment he goes underground.
    pub(crate) fn mole_enter_route(&mut self) {
        if self.paused || self.state.ended {
            return;
        }
        let (Some(net), Some(st)) =
            (&self.def.secondary, self.state.secondary.as_mut())
        else {
            return;
        };
        let s = st.mole.state();
        if !s.alive {
            return;
        }
        if st.mole_route.is_some() {
            self.events.push(Event::Secondary {
                secondary: SecondaryEvent::EntryRefused {
                    route_id: st.mole_route.clone(),
                    reason: EntryRefusalReason::AlreadyInRoute,
                },
            });
            return;
        }
        if st.mole.is_underground() {
            self.events.push(Event::Secondary {
                secondary: SecondaryEvent::EntryRefused {
                    route_id: None,
                    reason: EntryRefusalReason::NotAtAccessPoint,
                },
            });
            return;
        }
        let room = GhostLobbySim::room_id_at(&self.def, s.x);
        let ap = {
            let network = RouteNetwork { spec: net, state: st };
            network.access_point_near(s.x, room.as_deref(), ENTRY_REACH)
        };
        let Some(ap) = ap else {
            self.events.push(Event::Secondary {
                secondary: SecondaryEvent::EntryRefused {
                    route_id: None,
                    reason: EntryRefusalReason::NotAtAccessPoint,
                },
            });
            return;
        };
        let sealed = st
            .grille_states
            .get(&ap.id)
            .copied()
            .unwrap_or(ap.sealed);
        if sealed {
            self.events.push(Event::Secondary {
                secondary: SecondaryEvent::EntryRefused {
                    route_id: None,
                    reason: EntryRefusalReason::RouteSealed,
                },
            });
            return;
        }
        let route = net
            .routes
            .iter()
            .find(|r| r.access_points.contains(&ap.id));
        let Some(route) = route else {
            self.events.push(Event::Secondary {
                secondary: SecondaryEvent::EntryRefused {
                    route_id: None,
                    reason: EntryRefusalReason::NoAuthoredRoute,
                },
            });
            return;
        };
        if let RouteAccess::Denied { .. } =
            route_accessibility(ActorClass::Moletaire, route, st)
        {
            self.events.push(Event::Secondary {
                secondary: SecondaryEvent::EntryRefused {
                    route_id: Some(route.id.clone()),
                    reason: EntryRefusalReason::NotTraversable {
                        actor: ActorClass::Moletaire,
                    },
                },
            });
            return;
        }

        let egress = route
            .access_points
            .iter()
            .skip(1)
            .next()
            .or(route.access_points.first());
        let Some(egress) = egress else {
            return;
        };
        let Some(egress_ap) = net.access_points.iter().find(|a| &a.id == egress) else {
            return;
        };
        // The entry is complete at the access point: record it now so the
        // mole cannot "enter" mid-route just by diving.
        st.mole_route = Some(route.id.clone());
        st.mole_entry_ap = Some(ap.id.clone());
        st.discovered.insert(route.id.clone());
        st.mole.order_move_to(egress_ap.x, true);
        self.events.push(Event::Secondary {
            secondary: SecondaryEvent::RouteEntered { route_id: route.id.clone() },
        });
    }

    /// The mole chews the cable at his position. Underground only, above a
    /// minimum depth, and only where an authored cable is in reach.
    /// [`SecondaryEvent::CableSabotaged`] names the real affected cable and
    /// the grounded node it feeds; the node loses power once the timer
    /// completes.
    pub(crate) fn mole_sabotage_cable(&mut self) {
        if self.paused || self.state.ended {
            return;
        }
        let (Some(net), Some(st)) =
            (&self.def.secondary, self.state.secondary.as_mut())
        else {
            return;
        };
        let s = st.mole.state();
        if !s.alive {
            return;
        }
        if !st.mole.is_underground() {
            self.events.push(Event::Secondary {
                secondary: SecondaryEvent::SabotageRefused {
                    reason: SabotageRefusalReason::NotUnderground,
                },
            });
            return;
        }
        if s.depth <= DIG_MIN_DEPTH {
            self.events.push(Event::Secondary {
                secondary: SecondaryEvent::SabotageRefused {
                    reason: SabotageRefusalReason::TooShallow,
                },
            });
            return;
        }
        if topology::resolve_sabotage_cable(net, s.x).is_none() {
            self.events.push(Event::Secondary {
                secondary: SecondaryEvent::SabotageRefused {
                    reason: SabotageRefusalReason::NoCableInReach,
                },
            });
            return;
        }
        st.mole.order_sabotage_cable();
    }

    /// The mole reverses and returns to the route's entry access point.
    /// Refused while entrapped (a collapsed route is irreversible).
    /// [`SecondaryEvent::RouteReversed`] fires when his travel target flips.
    pub(crate) fn mole_return_to_entry(&mut self) {
        if self.paused || self.state.ended {
            return;
        }
        let (Some(net), Some(st)) =
            (&self.def.secondary, self.state.secondary.as_mut())
        else {
            return;
        };
        let Some(route_id) = st.mole_route.clone() else {
            return;
        };
        if st.route_states.get(&route_id) == Some(&RouteState::Collapsed) {
            // Entrapped: irreversible, cannot act.
            return;
        }
        let s = st.mole.state();
        let network = RouteNetwork { spec: net, state: st };
        let entry_x = st
            .mole_entry_ap
            .and_then(|id| network.access_point(&id))
            .map(|ap| ap.x)
            .unwrap_or(s.x);
        st.mole.order_move_to(entry_x, true);
        // RouteReversed surfaces on the next tick when the target flips.
    }

    /// The mole surfaces at his position, leaving the route. Only valid at
    /// one of this route's access points; the depth-decay tick then emits
    /// [`SecondaryEvent::RouteLeft`].
    pub(crate) fn mole_surface(&mut self) {
        if self.paused || self.state.ended {
            return;
        }
        let (Some(net), Some(st)) =
            (&self.def.secondary, self.state.secondary.as_mut())
        else {
            return;
        };
        let Some(route_id) = &st.mole_route else {
            return;
        };
        if st.route_states.get(route_id) == Some(&RouteState::Collapsed) {
            return;
        }
        let s = st.mole.state();
        let network = RouteNetwork { spec: net, state: st };
        let Some(route) = network.route(route_id) else {
            return;
        };
        let at_ap = route.access_points.iter().any(|id| {
            network.access_point(id).is_some_and(|ap| (ap.x - s.x).abs() <= ENTRY_REACH)
        });
        if at_ap {
            st.mole.order_move_to(s.x, false);
        } else {
            self.events.push(Event::Secondary {
                secondary: SecondaryEvent::RouteObstructed {
                    route_id: Some(route_id.clone()),
                    reason: ObstructionReason::NoAccessPoint,
                },
            });
        }
    }

    pub fn route_debug_view(&self) -> RouteDebugView {
        let net = match &self.def.secondary {
            Some(net) => net,
            None => {
                return RouteDebugView {
                    routes: Vec::new(),
                    topology_version: 0,
                    mole: None,
                };
            }
        };
        let st = self.state.secondary.as_ref();
        let mole_route = st.and_then(|s| s.mole_route.as_ref());
        let mole = st.map(|s| {
            let ms = s.mole.state();
            MoleDebug {
                x: ms.x,
                depth: ms.depth,
                hunger: ms.hunger,
                state: format!("{:?}", ms.state),
                underground: s.mole.is_underground(),
            }
        });
        let routes = net
            .routes
            .iter()
            .map(|route| {
                let state = st
                    .and_then(|s| s.route_states.get(&route.id))
                    .copied()
                    .unwrap_or(RouteState::Open);
                let discovered = st
                    .map_or(false, |s| s.discovered.contains(&route.id));
                let access_points = route
                    .access_points
                    .iter()
                    .filter_map(|ap_id| {
                        net.access_points
                            .iter()
                            .find(|a| &a.id == ap_id)
                            .map(|ap| {
                                let sealed = st
                                    .and_then(|s| s.grille_states.get(ap_id))
                                    .copied()
                                    .unwrap_or(ap.sealed);
                                AccessPointDebug {
                                    id: ap.id.clone(),
                                    kind: ap.kind,
                                    room: ap.room.as_ref().map(|r| r.to_string()),
                                    x: ap.x,
                                    sealed,
                                }
                            })
                    })
                    .collect();
                RouteDebugEntry {
                    id: route.id.clone(),
                    kind: route.kind,
                    label: route.label.clone(),
                    state,
                    actors: route
                        .actors
                        .names()
                        .into_iter()
                        .map(ToOwned::to_owned)
                        .collect(),
                    clearance: route.clearance,
                    travel_cost_per_sec: route.travel_cost_per_sec,
                    connects_node: route.connects_node.clone(),
                    discovered,
                    mole_on_route: mole_route.map_or(false, |r| *r == route.id),
                    access_points,
                }
            })
            .collect();
        RouteDebugView {
            routes,
            topology_version: st.map_or(0, |s| s.topology_version),
            mole,
        }
    }
}

fn secondary_mole_event(
    net: &SecondarySpec,
    st: &mut SecondaryState,
    events: &mut Vec<Event>,
    ev: MoleEvent,
) -> Option<String> {
    match ev {
        MoleEvent::CableSabotaged { cable_id } => {
            // The companion sim invents `cable_<x>` from position; resolve it
            // to a real authored cable. Nothing authored at that position
            // means the mole chewed dirt: no event, no node cut.
            let x = cable_id.strip_prefix("cable_").and_then(|s| s.parse::<f64>().ok())?;
            let cable_id = topology::resolve_sabotage_cable(net, x)?;
            let Some(cable) = net
                .cables
                .iter()
                .find(|c| c.id == cable_id)
                .cloned()
            else {
                return None;
            };
            st.cable_states.insert(cable_id.clone(), CableState::Severed);
            events.push(Event::Secondary {
                secondary: SecondaryEvent::CableSabotaged {
                    cable_id: cable_id.clone(),
                    node_id: cable.node_id.clone(),
                },
            });
            events.push(Event::Secondary {
                secondary: SecondaryEvent::NoiseEmitted {
                    at: cable.position,
                    loudness: 1.0,
                },
            });
            events.push(Event::Secondary {
                secondary: SecondaryEvent::DebrisLeft { at: cable.position },
            });
            events.push(Event::Secondary {
                secondary: SecondaryEvent::EvidenceLeft {
                    at: cable.position,
                    kind: EvidenceKind::CutCableEnds,
                },
            });
            st.noise = 1.0;
            Some(cable.node_id)
        }
        MoleEvent::VibrationDetected { reading } => {
            events.push(Event::Secondary {
                secondary: SecondaryEvent::VibrationDetected { reading },
            });
            None
        }
        MoleEvent::UndergroundScanComplete { objects } => {
            events.push(Event::Secondary {
                secondary: SecondaryEvent::ScanComplete { objects },
            });
            if let Some(route) = &st.mole_route {
                st.discovered.insert(route.clone());
            }
            None
        }
        _ => None,
    }
}

fn secondary_transit(
    net: &SecondarySpec,
    st: &mut SecondaryState,
    events: &mut Vec<Event>,
) {
    let s = st.mole.state();
    let now_underground = st.mole.is_underground();

    if !st.mole_was_underground && now_underground && st.mole_route.is_none() {
        // The mole just entered the earth: he must be on an authored route
        // at an authored access point. (MoleEnterRoute records the entry
        // itself at the access point, so this branch is a guardrail for any
        // other path underground.)
        if let Some((route, ap)) = entry_route_at(net, st, s.x) {
            let route_state = st
                .route_states
                .get(&route.id)
                .copied()
                .unwrap_or(RouteState::Open);
            if route_state == RouteState::Open {
                let route_id = route.id.clone();
                events.push(Event::Secondary {
                    secondary: SecondaryEvent::RouteEntered { route_id: route_id.clone() },
                });
                st.mole_route = Some(route_id);
                st.mole_entry_ap = Some(ap.id.clone());
                st.discovered.insert(route.id.clone());
            } else {
                let reason = match route_state {
                    RouteState::Sealed => ObstructionReason::Sealed,
                    RouteState::Collapsed => ObstructionReason::Collapsed,
                    RouteState::Open => unreachable!(),
                };
                events.push(Event::Secondary {
                    secondary: SecondaryEvent::RouteObstructed {
                        route_id: Some(route.id.clone()),
                        reason,
                    },
                });
                st.mole.state_mut().target_x = None;
            }
        } else {
            events.push(Event::Secondary {
                secondary: SecondaryEvent::RouteObstructed {
                    route_id: None,
                    reason: ObstructionReason::OutsideAuthoredRoute,
                },
            });
            st.mole.state_mut().target_x = None;
        }
    } else if st.mole_was_underground && !now_underground {
        // Surfaced: leaving is only valid at one of this route's access
        // points.
        if let Some(route_id) = st.mole_route.take() {
            let network = RouteNetwork { spec: net, state: st };
            let at_ap = network.route(&route_id).is_some_and(|route| {
                route.access_points.iter().any(|id| {
                    network
                        .access_point(id)
                        .is_some_and(|ap| (ap.x - s.x).abs() <= ENTRY_REACH)
                })
            });
            if at_ap {
                events.push(Event::Secondary {
                    secondary: SecondaryEvent::RouteLeft { route_id },
                });
            } else {
                events.push(Event::Secondary {
                    secondary: SecondaryEvent::RouteObstructed {
                        route_id: Some(route_id),
                        reason: ObstructionReason::OutsideAuthoredRoute,
                    },
                });
            }
            st.mole_entry_ap = None;
        }
    }
    st.mole_was_underground = now_underground;

    // Reversal: his travel target flipped while inside a route — he came from
    // the entry, went toward egress, and is now heading back (or turned
    // around mid-route). `target_x` is `None` right after he arrives at
    // egress, so a fresh `None -> Some` heading back toward the entry counts.
    if let Some(route_id) = &st.mole_route {
        let target = s.target_x;
        let network = RouteNetwork { spec: net, state: st };
        let entry_x = st
            .mole_entry_ap
            .and_then(|id| network.access_point(&id))
            .map(|ap| ap.x)
            .unwrap_or(s.x);
        let entry_dir = (entry_x - s.x).signum();
        match (st.mole_target_x.take(), target) {
            (Some(prev), Some(curr)) => {
                let prev_dir = (prev - s.x).signum();
                let curr_dir = (curr - s.x).signum();
                let reversed = (curr_dir != 0.0)
                    && ((prev_dir != 0.0 && curr_dir != prev_dir)
                        || (prev_dir == 0.0 && curr_dir == entry_dir));
                if reversed {
                    events.push(Event::Secondary {
                        secondary: SecondaryEvent::RouteReversed {
                            route_id: route_id.clone(),
                        },
                    });
                }
            }
            (None, Some(curr)) => {
                let curr_dir = (curr - s.x).signum();
                if entry_dir != 0.0 && curr_dir == entry_dir {
                    events.push(Event::Secondary {
                        secondary: SecondaryEvent::RouteReversed {
                            route_id: route_id.clone(),
                        },
                    });
                }
            }
            _ => {}
        }
        st.mole_target_x = target;
    }

    // Obstruction by route state while inside a route.
    if let Some(route_id) = &st.mole_route {
        let route_state = st
            .route_states
            .get(route_id)
            .copied()
            .unwrap_or(RouteState::Open);
        if route_state != RouteState::Open {
            let reason = match route_state {
                RouteState::Sealed => ObstructionReason::Sealed,
                RouteState::Collapsed => ObstructionReason::Collapsed,
                RouteState::Open => unreachable!(),
            };
            events.push(Event::Secondary {
                secondary: SecondaryEvent::RouteObstructed {
                    route_id: Some(route_id.clone()),
                    reason,
                },
            });
            st.mole.state_mut().target_x = None;
            if route_state == RouteState::Collapsed {
                events.push(Event::Secondary {
                    secondary: SecondaryEvent::MoleEntrapped {
                        route_id: route_id.clone(),
                    },
                });
            }
        }
    }

    // Inline hazards met along the route section the mole currently
    // occupies; each damage/evidence event fires once per cable.
    if let Some(route_id) = &st.mole_route {
        for space in &net.spaces {
            if space.route_id != *route_id {
                continue;
            }
            if s.x < space.x_range.0 || s.x > space.x_range.1 {
                continue;
            }
            match space.kind {
                SpaceKind::Fan => events.push(Event::Secondary {
                    secondary: SecondaryEvent::FanEncountered {
                        fan: space.id.as_str().to_owned(),
                    },
                }),
                SpaceKind::Valve => events.push(Event::Secondary {
                    secondary: SecondaryEvent::ValveEncountered {
                        valve: space.id.as_str().to_owned(),
                        open: false,
                    },
                }),
                SpaceKind::Water => events.push(Event::Secondary {
                    secondary: SecondaryEvent::WaterEntered { at: s.x },
                }),
                SpaceKind::DamagedCable => {
                    if let Some(ref_id) = &space.ref_id {
                        let severed =
                            st.cable_states.get(ref_id) == Some(&CableState::Severed);
                        if severed && !st.reported_damage.contains(ref_id) {
                            st.reported_damage.insert(ref_id.clone());
                            events.push(Event::Secondary {
                                secondary: SecondaryEvent::CableDamaged {
                                    cable_id: ref_id.clone(),
                                },
                            });
                            events.push(Event::Secondary {
                                secondary: SecondaryEvent::EvidenceLeft {
                                    at: (space.x_range.0 + space.x_range.1) / 2.0,
                                    kind: EvidenceKind::CutCableEnds,
                                },
                            });
                            events.push(Event::Secondary {
                                secondary: SecondaryEvent::NoiseEmitted {
                                    at: s.x,
                                    loudness: 0.3,
                                },
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// The route and access point covering `x`, if any: an authored access
/// point on a route whose span covers `x`.
fn entry_route_at<'a>(
    net: &'a SecondarySpec,
    _state: &SecondaryState,
    x: f64,
) -> Option<(&'a RouteSpec, &'a AccessPointSpec)> {
    let route = net
        .routes
        .iter()
        .find(|r| x >= r.geometry.x_range.0 && x <= r.geometry.x_range.1)?;
    let ap = route.access_points.iter().find_map(|id| {
        net.access_points
            .iter()
            .find(|ap| &ap.id == id && (ap.x - x).abs() <= ENTRY_REACH)
    })?;
    Some((route, ap))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::command::{Command, TickInput};
    use crate::scenario::{CableId, RouteId, RunConfig};
    use crate::scenario::event::Event;
    use crate::scenario::ghost_lobby::ghost_lobby_secondary;

    const ROUTE_NAME: &str = "maint-hall-service";
    const CABLE_NAME: &str = "cable-substation-feed";

    fn run(sim: &mut GhostLobbySim, log: &mut Vec<Event>, ticks: u64) {
        for _ in 0..ticks {
            log.extend(sim.tick(&TickInput::default()));
        }
    }

    fn tick_cmd(sim: &mut GhostLobbySim, log: &mut Vec<Event>, cmd: Command) {
        let mut input = TickInput::default();
        input.immediates = vec![cmd];
        log.extend(sim.tick(&input));
    }

    fn mole_state(sim: &GhostLobbySim) -> &crate::companion::sim::MoleRuntimeState {
        sim.state()
            .secondary
            .as_ref()
            .expect("secondary state present")
            .mole
            .state()
    }

    fn secondary(sim: &GhostLobbySim) -> &SecondaryState {
        sim.state()
            .secondary
            .as_ref()
            .expect("secondary state present")
    }

    fn entry_refused(log: &[Event], reason: EntryRefusalReason) -> bool {
        log.iter().any(|e| {
            matches!(
                e,
                Event::Secondary {
                    secondary: SecondaryEvent::EntryRefused { reason: r, .. },
                }
                if r == reason
            )
        })
    }

    /// The vertical slice: Moletaire enters an authored maintenance route,
    /// reaches the real service device at its far end, performs one action
    /// (chewing a real cable), and returns; the action changes the main
    /// scenario through the normal power-cascade systems.
    #[test]
    fn moletaire_enters_maintenance_route_sabotages_real_cable_and_returns() {
        let mut sim =
            GhostLobbySim::new(ghost_lobby_secondary(), RunConfig::standard(), 424242)
                .expect("the authored floor builds");
        let mut log = Vec::new();
        let route = RouteId::from(ROUTE_NAME);
        let cable = CableId::from(CABLE_NAME);

        run(&mut sim, &mut log, 30);
        tick_cmd(&mut sim, &mut log, Command::MoleEnterRoute);

        // He enters the earth at the hall grille; RouteEntered names the real
        // route and marks it discovered.
        let mut entered = false;
        for _ in 0..120 {
            run(&mut sim, &mut log, 1);
            if log.iter().any(|e| {
                matches!(
                    e,
                    Event::Secondary { secondary: SecondaryEvent::RouteEntered { route_id } }
                        if route_id == &route
                )
            }) {
                entered = true;
                break;
            }
        }
        assert!(entered, "mole never entered the route: {log:?}");
        assert!(secondary(&sim).discovered.contains(&route));

        // Travel to the far end: the service device at x = 1180.
        let mut at_egress = false;
        for _ in 0..300 {
            run(&mut sim, &mut log, 1);
            if mole_state(&sim).x >= 1170.0 {
                at_egress = true;
                break;
            }
        }
        assert!(
            at_egress,
            "mole never reached the service device (x={})",
            mole_state(&sim).x
        );

        // One action: chew the cable at the service device.
        tick_cmd(&mut sim, &mut log, Command::MoleSabotageCable);

        // The sabotage timer runs; three seconds later the real cable is cut.
        let mut sabotaged = false;
        for _ in 0..250 {
            run(&mut sim, &mut log, 1);
            if log.iter().any(|e| {
                matches!(
                    e,
                    Event::Secondary { secondary: SecondaryEvent::CableSabotaged { cable_id, .. } }
                        if cable_id == &cable
                )
            }) {
                sabotaged = true;
                break;
            }
        }
        assert!(sabotaged, "sabotage never completed: {log:?}");

        // The action changed the main scenario: a real node dies and its
        // cascade darkens the floor.
        assert!(
            sim.state.dead_nodes.contains("substation"),
            "substation must be dead: {:?}",
            sim.state.dead_nodes
        );
        assert!(
            sim.state
                .dead_nodes
                .iter()
                .any(|n| n.starts_with("cam-")),
            "cameras must lose power: {:?}",
            sim.state.dead_nodes
        );
        assert!(
            log.iter().any(|e| matches!(e, Event::PowerLost { .. })),
            "PowerLost must be in the log: {log:?}"
        );
        assert!(
            sim.state.lights_flicker > 0.0,
            "the floor must go dark"
        );

        // The cable state names the real cable, not an id invented from
        // position.
        assert_eq!(
            secondary(&sim).cable_states.get(&cable),
            Some(&CableState::Severed)
        );

        // Hunger, equipment and sensing survive the run intact.
        let mole = mole_state(&sim);
        assert!(mole.alive);
        assert!(
            mole.hunger > 0.0,
            "hunger must not vanish"
        );
        assert_eq!(mole.equipped, None);

        // Reversing and returning to the entry grille.
        tick_cmd(&mut sim, &mut log, Command::MoleReturnToEntry);
        let mut reversed = false;
        for _ in 0..80 {
            run(&mut sim, &mut log, 1);
            if log.iter().any(|e| {
                matches!(
                    e,
                    Event::Secondary { secondary: SecondaryEvent::RouteReversed { .. } }
                )
            }) {
                reversed = true;
                break;
            }
        }
        assert!(reversed, "mole never reversed: {log:?}");

        let mut back = false;
        for _ in 0..300 {
            run(&mut sim, &mut log, 1);
            if mole_state(&sim).x <= 410.0 {
                back = true;
                break;
            }
        }
        assert!(
            back,
            "mole never returned to the entry (x={})",
            mole_state(&sim).x
        );

        // Surfacing at the entry access point leaves the route.
        tick_cmd(&mut sim, &mut log, Command::MoleSurface);
        let mut left = false;
        for _ in 0..100 {
            run(&mut sim, &mut log, 1);
            if log.iter().any(|e| {
                matches!(
                    e,
                    Event::Secondary { secondary: SecondaryEvent::RouteLeft { .. } }
                )
            }) {
                left = true;
                break;
            }
        }
        assert!(left, "mole never left the route: {log:?}");
        assert!(secondary(&sim).mole_route.is_none());

        // On the way back he passed the damaged-cable section: one evidence
        // event per cable.
        assert!(
            log.iter().any(|e| {
                matches!(
                    e,
                    Event::Secondary { secondary: SecondaryEvent::CableDamaged { .. } }
                )
            }),
            "damaged-cable evidence missing: {log:?}"
        );
    }

    #[test]
    fn entry_is_refused_outside_an_access_point() {
        let mut sim =
            GhostLobbySim::new(ghost_lobby_secondary(), RunConfig::standard(), 1)
                .expect("builds");
        // Mid-floor, not at a grille: entry is refused and no route begins.
        tick_cmd(&mut sim, &mut Vec::new(), Command::MoleEnterRoute);
        assert!(
            sim.state.secondary.as_ref().unwrap().mole_route.is_none()
        );
        assert!(entry_refused(
            &sim.drain_events(),
            EntryRefusalReason::NotAtAccessPoint
        ));
    }

    #[test]
    fn sealed_route_cannot_be_entered() {
        let mut sim =
            GhostLobbySim::new(ghost_lobby_secondary(), RunConfig::standard(), 1)
                .expect("builds");
        let route = RouteId::from(ROUTE_NAME);
        // Seal the route from outside play: the grille is dogged.
        sim.state.secondary.as_mut().unwrap().route_states.insert(route, RouteState::Sealed);
        tick_cmd(&mut sim, &mut Vec::new(), Command::MoleEnterRoute);
        assert!(
            sim.state.secondary.as_ref().unwrap().mole_route.is_none()
        );
        let log = sim.drain_events();
        assert!(entry_refused(&log, EntryRefusalReason::RouteSealed));
    }
}

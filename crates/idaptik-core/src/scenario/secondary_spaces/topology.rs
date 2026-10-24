//! Spatial queries over the authored secondary network: which route covers a
/// world position, where its access points are, which cable sits at a
/// position, and how topology changes invalidate navigation.

use crate::scenario::secondary_spaces::model::*;

/// Read-only view over one route spec and its current runtime state.
pub struct RouteNetwork<'a> {
    pub spec: &'a SecondarySpec,
    pub state: &'a SecondaryState,
}

impl<'a> RouteNetwork<'a> {
    pub fn route(&self, id: &RouteId) -> Option<&RouteSpec> {
        self.spec.routes.iter().find(|r| &r.id == id)
    }

    pub fn access_point(&self, id: &AccessPointId) -> Option<&AccessPointSpec> {
        self.spec.access_points.iter().find(|ap| &ap.id == id)
    }

    /// The route whose x span covers `x`, if any.
    pub fn route_covering(&self, x: f64) -> Option<&RouteSpec> {
        self.spec
            .routes
            .iter()
            .find(|r| x >= r.geometry.x_range.0 && x <= r.geometry.x_range.1)
    }

    /// An access point near `x`, optionally restricted to one room.
    pub fn access_point_near(
        &self,
        x: f64,
        room: Option<&str>,
        reach: f64,
    ) -> Option<&AccessPointSpec> {
        self.spec
            .access_points
            .iter()
            .filter(|ap| (ap.x - x).abs() <= reach)
            .filter(|ap| room.map_or(true, |r| ap.room.as_deref() == Some(r)))
            .min_by(|a, b| {
                ((a.x - x).abs())
                    .partial_cmp(&(b.x - x).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// `(entry_x, egress_x)` for a route entered through `entry`.
    pub fn entry_egress(&self, route: &RouteSpec, entry: &AccessPointId) -> Option<(f64, f64)> {
        let entry_ap = self.access_point(entry)?;
        let other = route
            .access_points
            .iter()
            .find(|id| **id != *entry)
            .or(route.access_points.first())?;
        let egress_ap = self.access_point(other)?;
        Some((entry_ap.x, egress_ap.x))
    }

    /// The authored cable nearest `x` within its sabotage reach, if any.
    pub fn cable_at(&self, x: f64) -> Option<&CableSpec> {
        self.spec
            .cables
            .iter()
            .filter(|c| (c.position - x).abs() <= c.reach)
            .min_by(|a, b| {
                ((a.position - x).abs())
                    .partial_cmp(&(b.position - x).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// Inline sections of `route` that cover `x`, in declaration order.
    pub fn spaces_at<'b>(
        &'b self,
        route_id: &RouteId,
        x: f64,
    ) -> impl Iterator<Item = &'b SpaceSpec> + 'b {
        self.spec.spaces.iter().filter(move |space| {
            space.route_id == *route_id && x >= space.x_range.0 && x <= space.x_range.1
        })
    }
}

/// Routes the mole knows about: entered or scanned. Hidden from the playable
/// view until discovered; the debug view shows everything.
pub fn discovered_routes<'a>(
    state: &'a SecondaryState,
    spec: &'a SecondarySpec,
) -> Vec<&'a RouteSpec> {
    spec.routes
        .iter()
        .filter(|route| state.discovered.contains(&route.id))
        .collect()
}

/// Resolve the mole's invented sabotage id `cable_<x>` (emitted by the
/// companion sim from position) to a real authored cable, or `None` when
/// nothing authored sits at that position — the sabotage then hit dirt,
/// not a cable. Cable-sabotage events always name a real affected cable.
pub fn resolve_sabotage_cable(spec: &SecondarySpec, x: f64) -> Option<CableId> {
    let mut best: Option<(CableId, f64)> = None;
    for cable in &spec.cables {
        let distance = (cable.position - x).abs();
        if distance <= cable.reach {
            let replace = match &best {
                Some((_, best_distance)) => distance < *best_distance,
                None => true,
            };
            if replace {
                best = Some((cable.id.clone(), distance));
            }
        }
    }
    best.map(|(id, _)| id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> SecondarySpec {
        SecondarySpec {
            routes: vec![RouteSpec {
                id: RouteId::from("maint-hall-service"),
                kind: SpaceKind::Duct,
                label: "test duct".into(),
                geometry: RouteGeometry { x_range: (390.0, 1190.0), depth: (0.1, 0.6) },
                clearance: Clearance { w: 22.0, h: 18.0 },
                actors: ActorClassSet::only(ActorClass::Moletaire),
                travel_cost_per_sec: 0.005,
                equipment_required: vec![],
                access_points: [
                    AccessPointId::from("grille-hall"),
                    AccessPointId::from("grille-service"),
                ],
                connects_node: Some("substation".into()),
                sealed: false,
            }],
            access_points: vec![
                AccessPointSpec {
                    id: AccessPointId::from("grille-hall"),
                    kind: AccessPointKind::Grille,
                    room: Some("hall".into()),
                    x: 400.0,
                    y: 585.0,
                    sealed: false,
                    equipment_to_open: None,
                },
                AccessPointSpec {
                    id: AccessPointId::from("grille-service"),
                    kind: AccessPointKind::Grille,
                    room: Some("exit".into()),
                    x: 1180.0,
                    y: 585.0,
                    sealed: false,
                    equipment_to_open: None,
                },
            ],
            cables: vec![CableSpec {
                id: CableId::from("cable-substation-feed"),
                label: "feed".into(),
                node_id: "substation".into(),
                route_id: RouteId::from("maint-hall-service"),
                position: 1180.0,
                reach: 14.0,
                state: CableState::Intact,
            }],
            spaces: vec![],
        }
    }

    fn state() -> SecondaryState {
        SecondaryState {
            mole: crate::companion::sim::MoletaireSim::new(
                crate::companion::definition::moletaire(),
                crate::companion::sim::MoleParams::default(),
                1,
            )
            .unwrap(),
            mole_route: None,
            mole_was_underground: false,
            mole_target_x: None,
            mole_entry_ap: None,
            route_states: BTreeMap::new(),
            cable_states: BTreeMap::new(),
            discovered: BTreeSet::new(),
            grille_states: BTreeMap::new(),
            topology_version: 0,
            noise: 0.0,
            reported_damage: BTreeSet::new(),
        }
    }

    #[test]
    fn route_covering_returns_the_authored_route() {
        let net = RouteNetwork { spec: &spec(), state: &state() };
        assert_eq!(
            net.route_covering(1180.0).map(|r| r.id.as_str()),
            Some("maint-hall-service")
        );
        assert!(net.route_covering(2000.0).is_none());
    }

    #[test]
    fn entry_point_near_restricts_to_room_and_reach() {
        let net = RouteNetwork { spec: &spec(), state: &state() };
        let hall = net
            .access_point_near(400.0, Some("hall"), 16.0)
            .map(|ap| ap.id.as_str())
            .unwrap();
        assert_eq!(hall, "grille-hall");
        assert!(net.access_point_near(400.0, Some("exit"), 16.0).is_none());
        assert!(net.access_point_near(900.0, None, 16.0).is_none());
    }

    #[test]
    fn resolve_sabotage_names_the_real_cable_not_the_position() {
        let spec = spec();
        assert_eq!(
            resolve_sabotage_cable(&spec, 1180.0).map(|id| id.as_str()),
            Some("cable-substation-feed")
        );
        // A position invented from outside the authored reach resolves to
        // nothing: the mole chewed dirt.
        assert!(resolve_sabotage_cable(&spec, 9999.0).is_none());
        assert!(resolve_sabotage_cable(&spec, 500.0).is_none());
    }

    #[test]
    fn discovered_routes_filters_hidden_routes() {
        let spec = spec();
        let mut state = state();
        state.discovered.insert(RouteId::from("maint-hall-service"));
        let found = discovered_routes(&state, &spec);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id.as_str(), "maint-hall-service");
    }
}

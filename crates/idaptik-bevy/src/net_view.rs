//! Net View: a second screen over the same running `GhostLobbySim`, showing
//! `sim.graph()`'s nodes laid out by segment and letting the player click one
//! to pivot (`ssh`) or hack it -- the same `CommandQueue`/hacker session the
//! keyboard-driven Ghost Lobby scene already drives.

use bevy::prelude::*;

use crate::hud::GhostLobbyHudMarker;
use crate::scene::GhostLobbySceneMarker;

/// Which screen `idaptik-bevy` is currently showing. `N` toggles between the
/// two; both share the same window and the same `GhostLobbySim`.
#[derive(States, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum AppMode {
    #[default]
    GhostLobby,
    NetView,
}

/// Every entity the Ghost Lobby scene or HUD spawns, shown/hidden as a block.
type GhostLobbyUiQuery<'w, 's> = Query<
    'w,
    's,
    &'static mut Visibility,
    Or<(With<GhostLobbySceneMarker>, With<GhostLobbyHudMarker>)>,
>;

fn set_ghost_lobby_visibility(mut q: GhostLobbyUiQuery, target: Visibility) {
    for mut vis in &mut q {
        *vis = target;
    }
}

/// Hide the whole Ghost Lobby scene and HUD in one pass when Net View opens.
pub fn hide_ghost_lobby_ui(q: GhostLobbyUiQuery) {
    set_ghost_lobby_visibility(q, Visibility::Hidden);
}

/// Show it again when Net View closes.
pub fn show_ghost_lobby_ui(q: GhostLobbyUiQuery) {
    set_ghost_lobby_visibility(q, Visibility::Inherited);
}

//## Layout

use idaptik_core::netsim::graph::GroundedGraph;

/// Radial distance from the root (depth 0) to depth 1, and each depth step
/// beyond it -- a genuine hub renders as a fan of spokes at increasing
/// radius, not as a left-to-right grid a shared row could make look chained.
const DEPTH_RADIUS: f32 = 220.0;
/// Vertical gap between devices stacked at one segment's own anchor.
const NODE_STACK_GAP: f32 = 70.0;

/// One segment's place in the radial tree: its own anchor point (never a
/// specific device's position, so a connection line never looks like it
/// singles one device out) and, for every non-root segment, the parent it
/// hangs from.
#[derive(Clone, Copy)]
pub struct SegmentLayout {
    pub anchor: Vec2,
    pub parent: Option<usize>,
}

/// `layout_segments`'s result for the sim's current graph, resolved once by
/// `setup_net_view` rather than recomputed every frame: the graph does not
/// change shape while Net View is open, so neither does the layout.
#[derive(Resource)]
pub struct NetViewLayout(pub Vec<SegmentLayout>);

/// Lay out every segment in `graph` as a radial tree/star, rooted at segment
/// 0 (the network's entry point -- in the authored floor graphs this is
/// always the outermost perimeter segment). A hub (many segments reachable
/// from one) fans its children across an angular slice of the circle, so it
/// reads as spokes radiating from the hub rather than a chain running
/// through every device in between.
///
/// Built from `Segment::can_access` treated as undirected (either direction
/// counts) via one BFS pass, which also resolves it to a spanning tree even
/// where the underlying data has extra cross-links -- see `draw_segment_edges`
/// for how those extras still get drawn, just not as part of the tree shape.
/// A segment unreachable from the root (should not happen in an authored
/// floor graph, but must not panic) is placed as a root child on its own
/// angle so it still renders rather than being silently dropped.
pub fn layout_segments(graph: &GroundedGraph) -> Vec<SegmentLayout> {
    let n = graph.segments.len();
    if n == 0 {
        return Vec::new();
    }

    // Undirected adjacency from `can_access`, checked both ways.
    let adjacency: Vec<Vec<usize>> = (0..n)
        .map(|i| {
            (0..n)
                .filter(|&j| {
                    j != i
                        && (graph.segments[i].can_access.contains(&graph.segments[j].id)
                            || graph.segments[j].can_access.contains(&graph.segments[i].id))
                })
                .collect()
        })
        .collect();

    // BFS from segment 0: one parent and one depth per segment.
    let mut parent: Vec<Option<usize>> = vec![None; n];
    let mut depth: Vec<u32> = vec![0; n];
    let mut visited = vec![false; n];
    let mut queue = std::collections::VecDeque::new();
    visited[0] = true;
    queue.push_back(0);
    while let Some(current) = queue.pop_front() {
        for &next in &adjacency[current] {
            if !visited[next] {
                visited[next] = true;
                parent[next] = Some(current);
                depth[next] = depth[current] + 1;
                queue.push_back(next);
            }
        }
    }
    // Any segment the BFS never reached still needs a position: hang it off
    // the root at depth 1 so nothing vanishes from the diagram.
    for i in 0..n {
        if !visited[i] {
            parent[i] = Some(0);
            depth[i] = 1;
        }
    }

    // Children lists, in segment-array order (deterministic).
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, &p) in parent.iter().enumerate() {
        if let Some(parent_index) = p {
            children[parent_index].push(i);
        }
    }

    let mut anchor = vec![Vec2::ZERO; n];
    // Root's children fan across the full circle; each subtree recursively
    // subdivides the angular slice it was handed, so a subtree's own
    // children stay grouped in roughly its parent's direction.
    let root_children = &children[0];
    let root_slice = std::f32::consts::TAU / (root_children.len().max(1) as f32);
    let mut stack: Vec<(usize, f32, f32)> = root_children
        .iter()
        .enumerate()
        .map(|(i, &child)| (child, i as f32 * root_slice, (i + 1) as f32 * root_slice))
        .collect();
    while let Some((node, lo, hi)) = stack.pop() {
        let mid = (lo + hi) * 0.5;
        anchor[node] = Vec2::new(mid.cos(), mid.sin()) * (depth[node] as f32 * DEPTH_RADIUS);
        let kids = &children[node];
        if !kids.is_empty() {
            let step = (hi - lo) / kids.len() as f32;
            for (i, &child) in kids.iter().enumerate() {
                stack.push((child, lo + i as f32 * step, lo + (i + 1) as f32 * step));
            }
        }
    }

    (0..n)
        .map(|i| SegmentLayout {
            anchor: if i == 0 { Vec2::ZERO } else { anchor[i] },
            parent: parent[i],
        })
        .collect()
}

//## Hit testing

/// How close a click must land to a node's centre to hit it.
pub const HIT_RADIUS: f32 = 40.0;

/// The graph-node index whose position is closest to `cursor_world`, if any
/// is within `hit_radius` -- kept pure (no `Query`, no `Commands`) so it is
/// directly unit-testable without spinning up an `App`.
pub fn node_at(cursor_world: Vec2, nodes: &[(usize, Vec2)], hit_radius: f32) -> Option<usize> {
    nodes
        .iter()
        .map(|(index, pos)| (*index, pos.distance(cursor_world)))
        .filter(|(_, dist)| *dist <= hit_radius)
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .map(|(index, _)| index)
}

#[cfg(test)]
mod hit_test_tests {
    use super::*;

    #[test]
    fn a_click_inside_the_radius_hits_the_nearest_node() {
        let nodes = [(0, Vec2::new(0.0, 0.0)), (1, Vec2::new(200.0, 0.0))];
        assert_eq!(node_at(Vec2::new(10.0, 0.0), &nodes, HIT_RADIUS), Some(0));
        assert_eq!(node_at(Vec2::new(190.0, 0.0), &nodes, HIT_RADIUS), Some(1));
    }

    #[test]
    fn a_click_outside_every_radius_hits_nothing() {
        let nodes = [(0, Vec2::new(0.0, 0.0))];
        assert_eq!(node_at(Vec2::new(1000.0, 0.0), &nodes, HIT_RADIUS), None);
    }

    #[test]
    fn a_click_equidistant_between_two_nodes_picks_the_lower_index() {
        // total_cmp's tie-break is stable iteration order, i.e. whichever
        // comes first in `nodes`; document that explicitly here so a future
        // change to the iteration order is a visible test failure, not a
        // silent behaviour change.
        let nodes = [(0, Vec2::new(-10.0, 0.0)), (1, Vec2::new(10.0, 0.0))];
        assert_eq!(node_at(Vec2::ZERO, &nodes, HIT_RADIUS), Some(0));
    }
}

//## Spawn and teardown

use crate::driver::SimState;
use crate::sprites;
use std::net::Ipv4Addr;

/// Marker: every entity Net View spawns, so teardown is one query.
#[derive(Component)]
pub struct NetViewMarker;

/// Marker: the single parent entity every node hangs beneath. Panning moves
/// this one `Transform`, so the shared `Camera2d` (used by the Ghost Lobby
/// scene too) is never displaced.
#[derive(Component)]
pub struct NetViewRoot;

/// Marker: one graph node's icon, carrying what a click needs to target it.
/// `index` is the node's position in `sim.graph().nodes`, resolved once here
/// at spawn time rather than rescanned per click -- `NetNodeIndex` addresses
/// by this same position.
#[derive(Component)]
pub struct NetNodeMarker {
    pub index: usize,
    pub ip: Ipv4Addr,
}

/// Spawn every node in `sim.graph()`, one column per segment, each icon
/// labelled with the node's name. No camera is created here: the app has
/// exactly one `Camera2d`, spawned once by `scene::setup_scene` at `Startup`,
/// and Net View reuses it (Task 14 hides that scene's own entities, but the
/// camera itself is shared, not scene-scoped).
pub fn setup_net_view(
    mut commands: Commands,
    sim: Res<SimState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let graph = sim.sim.graph();
    let layouts = layout_segments(graph);
    // Every node's position in `graph.nodes`, resolved once here rather than
    // rescanned per click (see `NetNodeMarker`).
    let node_index: std::collections::HashMap<&str, usize> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();
    // One root parents every node, so a single `Transform` pans the lot; each
    // node's radial anchor becomes a local offset beneath it, so no per-node
    // position maths changes on reparenting.
    commands
        .spawn((
            NetViewMarker,
            NetViewRoot,
            Transform::default(),
            Visibility::Inherited,
        ))
        .with_children(|root| {
            for (segment_index, segment) in graph.segments.iter().enumerate() {
                for (stack_index, node) in graph
                    .nodes
                    .iter()
                    .filter(|n| n.segment == segment.id)
                    .enumerate()
                {
                    let pos = layouts[segment_index].anchor
                        + Vec2::new(0.0, -(stack_index as f32) * NODE_STACK_GAP);
                    root.spawn((
                        NetViewMarker,
                        NetNodeMarker {
                            index: node_index[node.id.as_str()],
                            ip: node.ip,
                        },
                        Transform::from_translation(pos.extend(0.0)),
                        Visibility::Inherited,
                    ))
                    .with_children(|parent| {
                        sprites::spawn_network_icon(
                            parent,
                            &mut meshes,
                            &mut materials,
                            node.kind,
                            node.security,
                        );
                        parent.spawn((
                            Text2d::new(node.name.clone()),
                            TextFont::from_font_size(11.0),
                            TextColor(Color::srgb(0.75, 0.78, 0.85)),
                            Transform::from_translation(Vec3::new(0.0, -60.0, 0.5)),
                        ));
                    });
                }
            }
        });
    commands.insert_resource(NetViewLayout(layouts));
}

/// Despawn every Net-View-only entity on the way out, so re-entering rebuilds
/// cleanly from the sim's current state rather than reusing stale positions.
pub fn teardown_net_view(mut commands: Commands, entities: Query<Entity, With<NetViewMarker>>) {
    for entity in &entities {
        commands.entity(entity).despawn();
    }
    commands.remove_resource::<NetViewLayout>();
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    use idaptik_core::device::{DeviceKind, SecurityLevel};
    use idaptik_core::netsim::graph::{Node, Segment};
    use idaptik_core::network::{Range, Zone};
    use std::net::Ipv4Addr;

    fn seg(id: &str, can_access: &[&str]) -> Segment {
        Segment {
            id: id.into(),
            range: Range::LocalLan,
            category: Zone::Internal,
            subnet: "10.0.0.".into(),
            can_access: can_access.iter().map(|s| s.to_string()).collect(),
            location: None,
        }
    }

    fn node(id: &str, seg: &str, ip: [u8; 4]) -> Node {
        Node {
            id: id.into(),
            name: id.into(),
            ip: Ipv4Addr::from(ip),
            segment: seg.into(),
            kind: DeviceKind::Server,
            security: SecurityLevel::Weak,
            actuation: None,
            deps: vec![],
        }
    }

    /// A hub with three children, mirroring the real floor graph's `bms`
    /// shape at a smaller scale: root -> hub -> {a, b, c}.
    fn hub_graph() -> GroundedGraph {
        GroundedGraph {
            segments: vec![
                seg("root", &["hub"]),
                seg("hub", &["root", "a", "b", "c"]),
                seg("a", &["hub"]),
                seg("b", &["hub"]),
                seg("c", &["hub"]),
            ],
            nodes: vec![
                node("n-root", "root", [10, 0, 0, 1]),
                node("n-hub", "hub", [10, 0, 0, 2]),
                node("n-a", "a", [10, 0, 0, 3]),
                node("n-b", "b", [10, 0, 0, 4]),
                node("n-c", "c", [10, 0, 0, 5]),
            ],
            dns: vec![],
            vantages: vec![],
        }
    }

    #[test]
    fn the_root_segment_sits_at_the_origin_with_no_parent() {
        let layout = layout_segments(&hub_graph());
        assert_eq!(layout[0].anchor, Vec2::ZERO);
        assert_eq!(layout[0].parent, None);
    }

    #[test]
    fn a_childs_anchor_is_further_from_the_origin_than_its_parent() {
        let layout = layout_segments(&hub_graph());
        // hub (index 1) is a direct child of root.
        assert!(layout[1].anchor.length() > layout[0].anchor.length());
        // a, b, c (indices 2,3,4) are children of hub, one depth further out.
        for i in [2, 3, 4] {
            assert!(
                layout[i].anchor.length() > layout[1].anchor.length(),
                "segment {i} must sit further out than its parent hub"
            );
            assert_eq!(layout[i].parent, Some(1));
        }
    }

    #[test]
    fn a_hubs_children_get_distinct_angles_not_a_single_overlapping_line() {
        let layout = layout_segments(&hub_graph());
        let positions: Vec<Vec2> = [2, 3, 4].iter().map(|&i| layout[i].anchor).collect();
        // No two of the hub's three children may land on the same point --
        // that collinear-overlap is exactly the bug this layout replaces.
        for i in 0..positions.len() {
            for j in (i + 1)..positions.len() {
                assert!(
                    positions[i].distance(positions[j]) > 1.0,
                    "children {i} and {j} must not coincide: {:?} vs {:?}",
                    positions[i],
                    positions[j]
                );
            }
        }
    }

    #[test]
    fn a_segment_unreachable_from_the_root_still_gets_a_position() {
        let mut g = hub_graph();
        g.segments.push(seg("orphan", &[]));
        g.nodes.push(node("n-orphan", "orphan", [10, 0, 0, 6]));
        let layout = layout_segments(&g);
        // Must not panic (already implied by reaching this line), and must
        // land somewhere other than the origin so it's visible.
        assert_ne!(layout[5].anchor, Vec2::ZERO);
    }

    #[test]
    fn an_empty_graph_returns_an_empty_layout_without_panicking() {
        let g = GroundedGraph {
            segments: vec![],
            nodes: vec![],
            dns: vec![],
            vantages: vec![],
        };
        assert!(layout_segments(&g).is_empty());
    }
}

//## Pan input

/// Movement, in screen pixels, beyond which a left-button press-and-hold reads
/// as a drag (pans, fires no command) rather than a click (pivots). Kept small
/// so a deliberate click tolerates a little hand tremor yet any real drag
/// crosses it at once.
const PAN_THRESHOLD: f32 = 6.0;

/// Left-button drag state. `origin_cursor` is where the press began (for the
/// threshold test); `last_cursor` is the previous frame's cursor (for the
/// per-frame delta applied to the root); `dragging` latches true once the
/// threshold is crossed and stays true through the release frame, so that
/// `net_view_click` can suppress the click it would otherwise fire.
#[derive(Resource, Default)]
pub struct NetViewDrag {
    pub origin_cursor: Option<Vec2>,
    pub last_cursor: Option<Vec2>,
    pub dragging: bool,
}

/// Pan the whole graph by dragging with the left button held. Moves the
/// `NetViewRoot`'s `Transform` only, never the shared camera. Converts the
/// screen-space cursor delta into world units via the same
/// `viewport_to_world_2d` lookup `net_view_click` uses, so a pan tracks the
/// cursor exactly whatever the camera's projection scale.
pub fn pan_net_view(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<crate::seats::MainCam>>,
    mut drag: ResMut<NetViewDrag>,
    mut roots: Query<&mut Transform, With<NetViewRoot>>,
) {
    // A fresh press starts a candidate drag: record the origin, clear the
    // latch. Whether it becomes a drag depends on how far the cursor then moves.
    if buttons.just_pressed(MouseButton::Left) {
        let cursor = windows.single().ok().and_then(|w| w.cursor_position());
        drag.origin_cursor = cursor;
        drag.last_cursor = cursor;
        drag.dragging = false;
        return;
    }
    // On release, forget the origin but leave `dragging` set: `net_view_click`
    // runs after this system and still needs it to decide whether to suppress
    // the click. The next press clears the latch.
    if buttons.just_released(MouseButton::Left) {
        drag.origin_cursor = None;
        drag.last_cursor = None;
        return;
    }
    if !buttons.pressed(MouseButton::Left) {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let (Some(origin), Some(last)) = (drag.origin_cursor, drag.last_cursor) else {
        return;
    };
    if origin.distance(cursor) > PAN_THRESHOLD {
        drag.dragging = true;
    }
    if drag.dragging
        && let Ok((camera, camera_transform)) = cameras.single()
        && let (Ok(last_world), Ok(cursor_world)) = (
            camera.viewport_to_world_2d(camera_transform, last),
            camera.viewport_to_world_2d(camera_transform, cursor),
        )
    {
        let delta = cursor_world - last_world;
        for mut transform in &mut roots {
            transform.translation.x += delta.x;
            transform.translation.y += delta.y;
        }
    }
    drag.last_cursor = Some(cursor);
}

//## Click input

use crate::driver::CommandQueue;
use idaptik_core::scenario::command::{Command, NetNodeIndex};

/// Left-click pivots (`ssh`) onto the node under the cursor; right-click
/// hacks it. Both resolve to the node's index into `sim.graph().nodes` (not
/// its id, read straight off `NetNodeMarker`), matching `NetNodeIndex`'s
/// addressing, and push straight into the same `CommandQueue` the keyboard
/// already fills -- Net View is a second input surface over the one running
/// sim, not a separate one.
///
/// The left pivot fires on release, not press, and only when the press-to-
/// release sequence never crossed `PAN_THRESHOLD` (`!drag.dragging`): that is
/// how a drag-to-pan is told apart from a click-to-pivot. Right-click hacks on
/// press as before, unaffected by dragging since only the left button pans.
pub fn net_view_click(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    // The single shared camera (see Task 14): Net View spawns no camera of
    // its own, so this is the same `Camera2d` the Ghost Lobby scene uses.
    cameras: Query<(&Camera, &GlobalTransform), With<crate::seats::MainCam>>,
    nodes: Query<(&NetNodeMarker, &GlobalTransform)>,
    drag: Res<NetViewDrag>,
    mut queue: ResMut<CommandQueue>,
) {
    let left = buttons.just_released(MouseButton::Left) && !drag.dragging;
    let right = buttons.just_pressed(MouseButton::Right);
    if !left && !right {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok((camera, camera_transform)) = cameras.single() else {
        return;
    };
    let Ok(cursor_world) = camera.viewport_to_world_2d(camera_transform, cursor) else {
        return;
    };

    let candidates: Vec<(usize, Vec2)> = nodes
        .iter()
        .map(|(marker, transform)| (marker.index, transform.translation().truncate()))
        .collect();
    let Some(index) = node_at(cursor_world, &candidates, HIT_RADIUS) else {
        return;
    };
    let node = NetNodeIndex(index as u32);
    if left {
        queue.push(Command::NetSsh { node });
    } else {
        queue.push(Command::NetHack { node });
    }
}

//## Connection lines

/// Draw the segment graph as a radial star/tree: the spanning tree the layout
/// resolved (the dominant read) plus any real `can_access` cross-link the tree
/// did not already draw, dimmer. The relation is treated as undirected ("can
/// these two talk"), so a pair is drawn once whether declared one way or both.
///
/// Gizmos live in absolute world space and ignore the entity hierarchy, so the
/// `NetViewRoot`'s current world translation is added to each segment anchor;
/// without it the lines would stay put while a pan slid the nodes away.
pub fn draw_segment_edges(
    sim: Res<SimState>,
    layout: Res<NetViewLayout>,
    roots: Query<&GlobalTransform, With<NetViewRoot>>,
    mut gizmos: Gizmos,
) {
    let Ok(root) = roots.single() else {
        return;
    };
    let offset = root.translation().truncate();
    let graph = sim.sim.graph();
    let layouts = &layout.0;

    // Primary: the spanning tree itself -- this is the star/hub shape the
    // layout was built to show.
    let tree_colour = Color::srgba(0.5, 0.65, 0.9, 0.55);
    for layout in layouts {
        if let Some(parent) = layout.parent {
            gizmos.line_2d(
                layouts[parent].anchor + offset,
                layout.anchor + offset,
                tree_colour,
            );
        }
    }

    // Secondary: any real can_access relationship the spanning tree did not
    // already draw (extra cross-links a floor graph might one day author
    // beyond a strict tree) -- dimmer, so the tree/star shape stays the
    // dominant read.
    let extra_colour = Color::srgba(0.4, 0.5, 0.7, 0.2);
    for a in 0..graph.segments.len() {
        for b in (a + 1)..graph.segments.len() {
            if layouts[a].parent == Some(b) || layouts[b].parent == Some(a) {
                continue;
            }
            let seg_a = &graph.segments[a];
            let seg_b = &graph.segments[b];
            let connected =
                seg_a.can_access.contains(&seg_b.id) || seg_b.can_access.contains(&seg_a.id);
            if connected {
                gizmos.line_2d(
                    layouts[a].anchor + offset,
                    layouts[b].anchor + offset,
                    extra_colour,
                );
            }
        }
    }
}

//## HUD strip

use idaptik_core::scenario::common::Channel;

/// Marker: Net View's own status/log text (a strip, not the full Ghost Lobby
/// HUD -- it reuses `hud.rs`'s visual conventions, not its entities, since
/// the two screens' layouts differ).
#[derive(Component)]
pub struct NetHudText;

/// Spawn the HUD strip: vantage/hops/reachable, the trace fraction, and a
/// short log tail, bottom-left -- the same corner and font sizes `hud.rs`
/// uses, so the two screens read as one app.
pub fn setup_net_hud(mut commands: Commands) {
    commands.spawn((
        NetViewMarker,
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(8.0),
            left: Val::Px(10.0),
            ..Default::default()
        },
        Text::new(""),
        TextFont::from_font_size(12.0),
        TextColor(Color::srgb(0.8, 0.83, 0.9)),
        NetHudText,
    ));
}

/// Drive the HUD strip from the live sim -- the same values `hud.rs`'s status
/// line already reads, plus a short tail of the net-related log lines.
pub fn update_net_hud(sim: Res<SimState>, mut text: Query<&mut Text, With<NetHudText>>) {
    let Ok(mut text) = text.single_mut() else {
        return;
    };
    let hacker = &sim.sim.state().agents.hacker;
    let tail: Vec<String> = sim
        .log
        .iter()
        .filter(|l| matches!(l.channel, Channel::Log))
        .rev()
        .take(5)
        .map(|l| l.text.clone())
        .collect();
    text.0 = format!(
        "N: back to floor   left-click: pivot   right-click: hack\n\
         vantage {:?}   pivot depth {}   nodes reachable {}   trace {:.0}%\n\n{}",
        hacker.vantage().kind,
        hacker.hops(),
        hacker.reachable_count(sim.sim.graph()),
        f64::from(hacker.trace_fraction()) * 100.0,
        tail.into_iter().rev().collect::<Vec<_>>().join("\n"),
    );
}

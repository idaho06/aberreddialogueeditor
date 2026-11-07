//! ECS projection system: executes the pure projection plan ([`plan_projection`]) and keeps the
//! ACTIVE document drawn as world-space entities — node roots (`GraphNode` + `WorldPosition`)
//! with `GraphPart` children (body, header, texts, pins) and edge entities (`GraphEdge`) with one
//! arrowhead child. Edges are rewritten in place when a node moves, never respawned.
//!
//! Lock discipline: the workspace is locked only to snapshot the active doc and clone the data
//! the plan needs (nodes to spawn/respawn); every `Commands`/query call happens after the guard
//! is dropped. When doc id, revision and font-metrics readiness are unchanged since the last
//! tick, the system returns without even snapshotting.

use aberredengine::core::components::global_transform2d::GlobalTransform2D;
use aberredengine::core::resources::font_metrics::{FontMetricsStore, FontMetricsWarnCache};
use aberredengine::prelude::*;
use rustc_hash::FxHashMap;

use crate::graph::projection_plan::{EdgeView, NodeView, ProjOp, Snapshot, is_structural};
use crate::graph::{
    CORNER_RADIUS, EDGE_COLOR, EDGE_SELECTED_COLOR, EDGE_THICKNESS, EdgeEntry, FONT, FONT_SIZE,
    GraphEdge, GraphEntities, GraphNode, GraphPart, HEADER_H, NODE_W, OUTLINE_THICKNESS, PIN_R,
    Z_ARROWHEAD, Z_EDGE, Z_HEADER, Z_NODE_BODY, Z_PIN, Z_TEXT, arrow_tip_dir, describe_node,
    edge_curve, plan_projection, snapshot_of,
};
use crate::model::{EdNode, NodeKey, Slot};
use crate::state::WorkspaceMutex;

/// Arrowhead triangle base width (perpendicular to the edge tangent at the tip).
pub const ARROWHEAD_W: f32 = 10.0;
/// Arrowhead triangle length (from base centre to tip, along the edge tangent).
pub const ARROWHEAD_L: f32 = 12.0;

/// The arrowhead triangle in edge-local coordinates: the tip sits at `to_rel` (the curve end),
/// the base is `ARROWHEAD_W` wide and `ARROWHEAD_L` long behind the tip along the curve tangent
/// ([`arrow_tip_dir`]). Pure geometry — tested without an ECS.
pub fn arrowhead_points(c2_rel: Vec2, to_rel: Vec2) -> [Vec2; 3] {
    let dir = arrow_tip_dir(c2_rel, to_rel);
    let perp = Vec2::new(-dir.y, dir.x);
    let base = to_rel - dir * ARROWHEAD_L;
    let half = perp * (ARROWHEAD_W / 2.0);
    [to_rel, base + half, base - half]
}

/// Edge colour: the selected source node's outgoing edges are highlighted.
fn edge_color(selected_source: bool) -> Color {
    if selected_source {
        EDGE_SELECTED_COLOR
    } else {
        EDGE_COLOR
    }
}

/// The `GlobalTransform2D` to spawn a child WITH: the engine's `propagate_transforms` inserts this
/// component via deferred Commands ("visible next frame"), so a freshly spawned child would
/// otherwise render its first frame at its raw local `WorldPosition` (near world origin).
/// `parent_world` is the parent's world position, `local` the child's local offset.
fn child_global(parent_world: Vec2, local: Vec2) -> GlobalTransform2D {
    GlobalTransform2D {
        position: parent_world + local,
        rotation_degrees: 0.0,
        scale: Vec2::ONE,
    }
}

/// Spawns one `GraphPart` child of `root` at `local` (relative to the root's world position
/// `parent_world`), WITH a `GlobalTransform2D` (see [`child_global`]), and returns its entity.
fn spawn_part<B: Bundle>(
    commands: &mut Commands,
    root: Entity,
    parent_world: Vec2,
    local: Vec2,
    z: f32,
    bundle: B,
) -> Entity {
    commands
        .spawn((
            GraphPart,
            ChildOf(root),
            WorldPosition::from_vec(local),
            child_global(parent_world, local),
            ZIndex(z),
            bundle,
        ))
        .id()
}

/// The bezier edge curve from world `from` to world `to` as a `Shape`, plus the arrowhead inputs
/// (`c2_rel` and `to_rel`, both relative to `from` — the edge entity's position).
pub(super) fn curve_shape(from: Vec2, to: Vec2, color: Color) -> (Shape, Vec2, Vec2) {
    let (c1, c2, to_rel) = edge_curve(from, to);
    (
        Shape::bezier(c1, c2, to_rel, color).with_thickness(EDGE_THICKNESS),
        c2,
        to_rel,
    )
}

/// Assigns the entity's world position only when different (cheap change detection).
pub(super) fn set_pos_if_changed(
    positions: &mut Query<&mut WorldPosition>,
    entity: Entity,
    pos: Vec2,
) {
    if let Ok(mut p) = positions.get_mut(entity)
        && p.pos != pos
    {
        p.set_pos(pos);
    }
}

/// Assigns the entity's shape only when different (cheap change detection).
pub(super) fn set_shape_if_changed(shapes: &mut Query<&mut Shape>, entity: Entity, shape: Shape) {
    if let Ok(mut s) = shapes.get_mut(entity)
        && *s != shape
    {
        *s = shape;
    }
}

/// Draws the ACTIVE document and keeps the entities in sync with it. Runs late in the sim tick
/// (`add_late_system`) so it sees this tick's edits.
#[allow(clippy::too_many_arguments)]
pub fn graph_projection_system(
    mut commands: Commands,
    mut ents: ResMut<GraphEntities>,
    app_state: Res<AppState>,
    metrics: Res<FontMetricsStore>,
    pending: Res<PendingAssets>,
    mut warn: ResMut<FontMetricsWarnCache>,
    mut shapes: Query<&mut Shape>,
    mut positions: Query<&mut WorldPosition>,
) {
    // Font metrics arrive asynchronously: until they do, nodes spawn without their text items and
    // are respawned once `metrics_ready` flips (it is part of the node signature).
    let key = metrics.resolve_key(FONT, &pending, &mut warn);
    let font = metrics.0.get(key);
    let metrics_ready = font.is_some();
    let measure = |s: &str| font.map_or(0.0, |m| m.measure_text(s, FONT_SIZE).x);

    let Some(handle) = app_state.get::<WorkspaceMutex>().cloned() else {
        return;
    };

    // Lock the workspace only to snapshot the active doc and clone what the plan needs.
    let (snapshot, ops, mut node_data) = {
        let mut ws = handle.lock().unwrap();
        let (doc_id, revision) = match ws.active_doc() {
            Some(doc) => (doc.doc_id, doc.revision),
            None => (0, 0),
        };
        // Cheap path: nothing changed since the last executed plan — do not even snapshot.
        if ents.shown.doc_id == doc_id
            && ents.shown.revision == revision
            && ents.shown.metrics_ready == metrics_ready
        {
            return;
        }
        let snapshot = match ws.active_doc_mut() {
            Some(doc) => snapshot_of(doc, metrics_ready),
            // No active doc: plan against an empty snapshot (despawns everything).
            None => Snapshot {
                metrics_ready,
                ..Snapshot::default()
            },
        };
        let ops = plan_projection(&ents.shown, &snapshot);
        // Clone the nodes the plan spawns/respawns (plus their view data) while still locked.
        let mut node_by_key: FxHashMap<NodeKey, &EdNode> = FxHashMap::default();
        if let Some(doc) = ws.active_doc() {
            node_by_key = doc.nodes.iter().map(|n| (n.key, n)).collect();
        }
        let view_by_key: FxHashMap<NodeKey, &NodeView> =
            snapshot.nodes.iter().map(|n| (n.key, n)).collect();
        let mut node_data = FxHashMap::default();
        for op in &ops {
            if let ProjOp::SpawnNode(key) | ProjOp::RespawnNode(key) = op
                && let (Some(node), Some(view)) = (node_by_key.get(key), view_by_key.get(key))
            {
                node_data.insert(*key, ((*node).clone(), (*view).clone()));
            }
        }
        (snapshot, ops, node_data)
    }; // guard dropped: commands and queries only run from here on

    // Log only structural plans (spawn/despawn): a drag produces a MoveNode/RewriteEdge-only
    // plan on every tick and must stay silent.
    if is_structural(&ops) {
        log::debug!(
            "projection: {} nodes, {} edges",
            snapshot.nodes.len(),
            snapshot.edges.len()
        );
    }

    let edge_by_key: FxHashMap<(NodeKey, Slot), &EdgeView> = snapshot
        .edges
        .iter()
        .map(|e| ((e.from_key, e.slot), e))
        .collect();

    for op in &ops {
        match *op {
            ProjOp::DespawnAll => {
                for (_, root) in ents.nodes.drain() {
                    commands.entity(root).despawn();
                }
                for (_, entry) in ents.edges.drain() {
                    commands.entity(entry.entity).despawn();
                }
            }
            ProjOp::SpawnNode(key) | ProjOp::RespawnNode(key) => {
                if matches!(op, ProjOp::RespawnNode(_))
                    && let Some(old) = ents.nodes.remove(&key)
                {
                    commands.entity(old).despawn();
                }
                if let Some((node, view)) = node_data.remove(&key) {
                    spawn_node(&mut commands, &mut ents, key, &node, &view, &measure);
                }
            }
            ProjOp::MoveNode(key, pos) => {
                if let Some(&root) = ents.nodes.get(&key) {
                    set_pos_if_changed(&mut positions, root, pos);
                }
            }
            ProjOp::DespawnNode(key) => {
                if let Some(root) = ents.nodes.remove(&key) {
                    commands.entity(root).despawn();
                }
            }
            ProjOp::SpawnEdge(from_key, slot) => {
                if let Some(ev) = edge_by_key.get(&(from_key, slot)) {
                    spawn_edge(&mut commands, &mut ents, ev);
                }
            }
            ProjOp::RewriteEdge(from_key, slot) => {
                if let (Some(ev), Some(entry)) = (
                    edge_by_key.get(&(from_key, slot)),
                    ents.edges.get(&(from_key, slot)),
                ) {
                    rewrite_edge(&mut shapes, &mut positions, ev, entry.entity, entry.arrow);
                }
            }
            ProjOp::DespawnEdge(from_key, slot) => {
                if let Some(entry) = ents.edges.remove(&(from_key, slot)) {
                    commands.entity(entry.entity).despawn();
                }
            }
        }
    }

    ents.shown = snapshot;
}

/// Spawns one node: a root entity at `view.pos` plus `GraphPart` children at local positions.
/// The root itself gets NO `GlobalTransform2D` — only children are spawned WITH one (see
/// [`child_global`]). Texts are dropped while font metrics are unavailable
/// (`sig.metrics_ready == false`).
fn spawn_node(
    commands: &mut Commands,
    ents: &mut GraphEntities,
    key: NodeKey,
    node: &EdNode,
    view: &NodeView,
    measure: &dyn Fn(&str) -> f32,
) {
    let visual = describe_node(
        node,
        view.sig.is_start,
        view.sig.is_selected,
        view.sig.has_error,
        measure,
    );
    let root = commands
        .spawn((
            GraphNode,
            WorldPosition::from_vec(view.pos),
            ZIndex(Z_NODE_BODY),
        ))
        .id();

    let mut body = Shape::rounded_rect(visual.size, CORNER_RADIUS, visual.body_color)
        .filled()
        .with_outline_color(visual.outline_color);
    if visual.outline_thick {
        body = body.with_thickness(OUTLINE_THICKNESS);
    }
    spawn_part(commands, root, view.pos, Vec2::ZERO, Z_NODE_BODY, body);
    spawn_part(
        commands,
        root,
        view.pos,
        Vec2::ZERO,
        Z_HEADER,
        Shape::rect(Vec2::new(NODE_W, HEADER_H), visual.header_color).filled(),
    );
    if view.sig.metrics_ready {
        for text in &visual.texts {
            spawn_part(
                commands,
                root,
                view.pos,
                text.local,
                Z_TEXT,
                DynamicText::new(text.text.clone(), FONT, FONT_SIZE, text.color),
            );
        }
    }
    for pin in &visual.pins {
        // `Shape::circle` is centred on its local origin, so the pin's local position is its
        // centre — no `.with_origin` adjustment needed.
        spawn_part(
            commands,
            root,
            view.pos,
            pin.local,
            Z_PIN,
            Shape::circle(PIN_R, pin.color).filled(),
        );
    }

    ents.nodes.insert(key, root);
}

/// Spawns one edge at its source pin (world `from`) with one arrowhead child. The bezier control
/// points and endpoint are relative to `from` (`Shape::bezier` is local to its entity).
fn spawn_edge(commands: &mut Commands, ents: &mut GraphEntities, ev: &EdgeView) {
    let color = edge_color(ev.selected_source);
    let (curve, c2, to_rel) = curve_shape(ev.from, ev.to, color);
    let entity = commands
        .spawn((
            GraphEdge,
            WorldPosition::from_vec(ev.from),
            ZIndex(Z_EDGE),
            curve,
        ))
        .id();
    let arrow = spawn_part(
        commands,
        entity,
        ev.from,
        Vec2::ZERO,
        Z_ARROWHEAD,
        Shape::polygon(&arrowhead_points(c2, to_rel), color).filled(),
    );
    ents.edges
        .insert((ev.from_key, ev.slot), EdgeEntry { entity, arrow });
}

/// Rewrites an existing edge in place (position, curve, arrowhead) — a node move never respawns
/// an edge. Components are assigned only when different so change detection stays cheap.
fn rewrite_edge(
    shapes: &mut Query<&mut Shape>,
    positions: &mut Query<&mut WorldPosition>,
    ev: &EdgeView,
    entity: Entity,
    arrow: Entity,
) {
    let color = edge_color(ev.selected_source);
    set_pos_if_changed(positions, entity, ev.from);
    let (curve, c2, to_rel) = curve_shape(ev.from, ev.to, color);
    set_shape_if_changed(shapes, entity, curve);
    set_shape_if_changed(
        shapes,
        arrow,
        Shape::polygon(&arrowhead_points(c2, to_rel), color).filled(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::fixtures::close;

    #[test]
    fn child_global_composes_parent_world_and_local_offset() {
        let g = child_global(Vec2::new(120.0, -45.0), Vec2::new(8.0, 24.0));
        assert!(close(g.position, Vec2::new(128.0, -21.0)));
        assert_eq!(g.rotation_degrees, 0.0);
        assert_eq!(g.scale, Vec2::ONE);
        // A child at local ZERO sits exactly at the parent's world position.
        let g0 = child_global(Vec2::new(120.0, -45.0), Vec2::ZERO);
        assert!(close(g0.position, Vec2::new(120.0, -45.0)));
    }

    #[test]
    fn arrowhead_tip_is_at_to_rel_and_base_behind_it_along_the_tangent() {
        // Forward edge from (0,0) to (300,100): c2 = to_rel - (150, 0), tangent at t=1 is (1, 0).
        let to_rel = Vec2::new(300.0, 100.0);
        let c2 = Vec2::new(150.0, 100.0);
        let pts = arrowhead_points(c2, to_rel);
        assert_eq!(pts[0], to_rel, "the tip is the curve end");
        assert!(close(arrow_tip_dir(c2, to_rel), Vec2::new(1.0, 0.0)));
        let base_mid = (pts[1] + pts[2]) / 2.0;
        assert!(
            close(base_mid, to_rel - Vec2::new(ARROWHEAD_L, 0.0)),
            "base centre is ARROWHEAD_L behind the tip along the tangent"
        );
        let base = pts[1] - pts[2];
        assert!(
            (base.length() - ARROWHEAD_W).abs() < 1e-5,
            "base is ARROWHEAD_W wide"
        );
        assert!(
            close(base, Vec2::new(0.0, ARROWHEAD_W)),
            "base is perpendicular to the tangent"
        );
    }

    #[test]
    fn arrowhead_points_follow_a_diagonal_tangent() {
        let to_rel = Vec2::new(0.0, 100.0);
        let c2 = Vec2::new(-50.0, 50.0); // tangent at t=1: normalized (50, 50)
        let pts = arrowhead_points(c2, to_rel);
        assert_eq!(pts[0], to_rel);
        let dir = arrow_tip_dir(c2, to_rel);
        let base_mid = (pts[1] + pts[2]) / 2.0;
        assert!(close(base_mid, to_rel - dir * ARROWHEAD_L));
        let base = pts[1] - pts[2];
        assert!(
            base.dot(dir).abs() < 1e-5,
            "base is perpendicular to the tangent"
        );
        assert!((base.length() - ARROWHEAD_W).abs() < 1e-5);
    }

    #[test]
    fn arrowhead_with_degenerate_tangent_uses_the_fallback_direction() {
        let p = Vec2::new(5.0, 5.0);
        let pts = arrowhead_points(p, p);
        assert_eq!(pts[0], p);
        let base_mid = (pts[1] + pts[2]) / 2.0;
        assert!(close(base_mid, p - Vec2::new(ARROWHEAD_L, 0.0)));
    }
}

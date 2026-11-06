//! ECS projection system: executes the pure projection plan ([`plan_projection`]) and keeps the
//! ACTIVE document drawn as world-space entities — node roots (`GraphNode` + `WorldPosition`)
//! with `GraphPart` children (body, header, texts, pins) and edge entities (`GraphEdge`) with one
//! arrowhead child. Edges are rewritten in place when a node moves, never respawned.
//!
//! Lock discipline: the workspace is locked only to snapshot the active doc and clone the data
//! the plan needs (nodes to spawn/respawn); every `Commands`/query call happens after the guard
//! is dropped. When doc id, revision and font-metrics readiness are unchanged since the last
//! tick, the system returns without even snapshotting.

use aberredengine::core::resources::font_metrics::{FontMetricsStore, FontMetricsWarnCache};
use aberredengine::prelude::*;
use rustc_hash::FxHashMap;

use crate::graph::projection_plan::{EdgeView, ProjOp, Snapshot};
use crate::graph::{
    CORNER_RADIUS, EDGE_COLOR, EDGE_SELECTED_COLOR, EDGE_THICKNESS, EdgeEntry, FONT, FONT_SIZE,
    GraphEdge, GraphEntities, GraphNode, GraphPart, HEADER_H, NODE_W, NodeEntry, NodeView,
    OUTLINE_THICKNESS, PIN_R, Z_ARROWHEAD, Z_EDGE, Z_HEADER, Z_NODE_BODY, Z_PIN, Z_TEXT,
    arrow_tip_dir, describe_node, edge_curve, plan_projection, snapshot_of,
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
    let (snapshot, ops, revision, mut node_data) = {
        let mut ws = handle.lock().unwrap();
        let (doc_id, revision) = match ws.active_doc() {
            Some(doc) => (doc.doc_id, doc.revision),
            None => (0, 0),
        };
        // Cheap path: nothing changed since the last executed plan — do not even snapshot.
        if ents.shown_doc == Some(doc_id)
            && ents.shown_revision == revision
            && ents.metrics_ready == metrics_ready
        {
            return;
        }
        let snapshot = match ws.active_doc_mut() {
            Some(doc) => snapshot_of(doc, metrics_ready),
            // No active doc: plan against an empty snapshot (despawns everything).
            None => Snapshot {
                doc_id: 0,
                metrics_ready,
                nodes: Vec::new(),
                edges: Vec::new(),
            },
        };
        let ops = plan_projection(&ents.shown, &snapshot);
        // Clone the nodes the plan spawns/respawns (plus their view data) while still locked.
        let doc = ws.active_doc();
        let mut node_data = FxHashMap::default();
        for op in &ops {
            if let ProjOp::SpawnNode(key) | ProjOp::RespawnNode(key) = op
                && let Some(node) = doc.and_then(|d| d.node(*key)).cloned()
                && let Some(view) = snapshot.nodes.iter().find(|n| n.key == *key)
            {
                node_data.insert(*key, (node, view.clone()));
            }
        }
        (snapshot, ops, revision, node_data)
    }; // guard dropped: commands and queries only run from here on

    if !ops.is_empty() {
        log::debug!(
            "projection: {} nodes, {} edges",
            snapshot.nodes.len(),
            snapshot.edges.len()
        );
    }

    for op in &ops {
        match *op {
            ProjOp::DespawnAll => {
                for (_, entry) in ents.nodes.drain() {
                    commands.entity(entry.root).despawn();
                }
                for (_, entry) in ents.edges.drain() {
                    commands.entity(entry.entity).despawn();
                }
            }
            ProjOp::SpawnNode(key) | ProjOp::RespawnNode(key) => {
                if matches!(op, ProjOp::RespawnNode(_))
                    && let Some(old) = ents.nodes.remove(&key)
                {
                    commands.entity(old.root).despawn();
                }
                if let Some((node, view)) = node_data.remove(&key) {
                    spawn_node(&mut commands, &mut ents, key, &node, &view, &measure);
                }
            }
            ProjOp::MoveNode(key, pos) => {
                if let Some(entry) = ents.nodes.get_mut(&key) {
                    if let Ok(mut p) = positions.get_mut(entry.root)
                        && p.pos != pos
                    {
                        p.set_pos(pos);
                    }
                    entry.pos = pos;
                }
            }
            ProjOp::DespawnNode(key) => {
                if let Some(entry) = ents.nodes.remove(&key) {
                    commands.entity(entry.root).despawn();
                }
            }
            ProjOp::SpawnEdge(from_key, slot) => {
                if let Some(ev) = find_edge(&snapshot, from_key, slot) {
                    spawn_edge(&mut commands, &mut ents, ev);
                }
            }
            ProjOp::RewriteEdge(from_key, slot) => {
                if let (Some(ev), Some(entry)) = (
                    find_edge(&snapshot, from_key, slot),
                    ents.edges.get_mut(&(from_key, slot)),
                ) {
                    rewrite_edge(&mut shapes, &mut positions, ev, entry);
                }
            }
            ProjOp::DespawnEdge(from_key, slot) => {
                if let Some(entry) = ents.edges.remove(&(from_key, slot)) {
                    commands.entity(entry.entity).despawn();
                }
            }
        }
    }

    ents.shown.record(&snapshot);
    ents.shown_doc = Some(snapshot.doc_id);
    ents.shown_revision = revision;
    ents.metrics_ready = metrics_ready;
}

/// The snapshot edge for `(from_key, slot)`.
fn find_edge(snapshot: &Snapshot, from_key: NodeKey, slot: Slot) -> Option<&EdgeView> {
    snapshot
        .edges
        .iter()
        .find(|e| e.from_key == from_key && e.slot == slot)
}

/// Spawns one node: a root entity at `view.pos` plus `GraphPart` children at local positions.
/// Texts are dropped while font metrics are unavailable (`sig.metrics_ready == false`).
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
            GraphNode { key },
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
    commands.spawn((
        GraphPart,
        ChildOf(root),
        WorldPosition::from_vec(Vec2::ZERO),
        ZIndex(Z_NODE_BODY),
        body,
    ));
    commands.spawn((
        GraphPart,
        ChildOf(root),
        WorldPosition::from_vec(Vec2::ZERO),
        ZIndex(Z_HEADER),
        Shape::rect(Vec2::new(NODE_W, HEADER_H), visual.header_color).filled(),
    ));
    if view.sig.metrics_ready {
        for text in &visual.texts {
            commands.spawn((
                GraphPart,
                ChildOf(root),
                WorldPosition::from_vec(text.local),
                ZIndex(Z_TEXT),
                DynamicText::new(text.text.clone(), FONT, FONT_SIZE, text.color),
            ));
        }
    }
    for pin in &visual.pins {
        // `Shape::circle` is centred on its local origin, so the pin's local position is its
        // centre — no `.with_origin` adjustment needed.
        commands.spawn((
            GraphPart,
            ChildOf(root),
            WorldPosition::from_vec(pin.local),
            ZIndex(Z_PIN),
            Shape::circle(PIN_R, pin.color).filled(),
        ));
    }

    ents.nodes.insert(
        key,
        NodeEntry {
            root,
            sig: view.sig.clone(),
            pos: view.pos,
        },
    );
}

/// Spawns one edge at its source pin (world `from`) with one arrowhead child. The bezier control
/// points and endpoint are relative to `from` (`Shape::bezier` is local to its entity).
fn spawn_edge(commands: &mut Commands, ents: &mut GraphEntities, ev: &EdgeView) {
    let (c1, c2, to_rel) = edge_curve(ev.from, ev.to);
    let color = edge_color(ev.selected_source);
    let entity = commands
        .spawn((
            GraphEdge {
                from: ev.from_key,
                slot: ev.slot,
            },
            WorldPosition::from_vec(ev.from),
            ZIndex(Z_EDGE),
            Shape::bezier(c1, c2, to_rel, color).with_thickness(EDGE_THICKNESS),
        ))
        .id();
    let arrow = commands
        .spawn((
            GraphPart,
            ChildOf(entity),
            WorldPosition::from_vec(Vec2::ZERO),
            ZIndex(Z_ARROWHEAD),
            Shape::polygon(&arrowhead_points(c2, to_rel), color).filled(),
        ))
        .id();
    ents.edges.insert(
        (ev.from_key, ev.slot),
        EdgeEntry {
            entity,
            arrow,
            from: ev.from,
            to: ev.to,
            selected_source: ev.selected_source,
            target: ev.target,
        },
    );
}

/// Rewrites an existing edge in place (position, curve, arrowhead) — a node move never respawns
/// an edge. Components are assigned only when different so change detection stays cheap.
fn rewrite_edge(
    shapes: &mut Query<&mut Shape>,
    positions: &mut Query<&mut WorldPosition>,
    ev: &EdgeView,
    entry: &mut EdgeEntry,
) {
    if let Ok(mut p) = positions.get_mut(entry.entity)
        && p.pos != ev.from
    {
        p.set_pos(ev.from);
    }
    let (c1, c2, to_rel) = edge_curve(ev.from, ev.to);
    let color = edge_color(ev.selected_source);
    let curve = Shape::bezier(c1, c2, to_rel, color).with_thickness(EDGE_THICKNESS);
    if let Ok(mut s) = shapes.get_mut(entry.entity)
        && *s != curve
    {
        *s = curve;
    }
    let arrow = Shape::polygon(&arrowhead_points(c2, to_rel), color).filled();
    if let Ok(mut s) = shapes.get_mut(entry.arrow)
        && *s != arrow
    {
        *s = arrow;
    }
    entry.from = ev.from;
    entry.to = ev.to;
    entry.selected_source = ev.selected_source;
    entry.target = ev.target;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec2, b: Vec2) -> bool {
        a.abs_diff_eq(b, 1e-5)
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

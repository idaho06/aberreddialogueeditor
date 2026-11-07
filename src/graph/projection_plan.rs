//! Pure projection planning: `snapshot_of` captures the active doc as plain data and
//! `plan_projection` diffs a new snapshot against the remembered one, producing the `ProjOp`s
//! the projection system executes. No ECS types here — the only engine import is `Vec2`.

use aberredengine::prelude::Vec2;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::graph::describe::{row_link, row_text};
use crate::graph::geometry;
use crate::model::{DialogueDoc, EdNode, Link, NodeKey, Severity, Slot};

/// What an output row's target resolves to (for the signature).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    End,
    Node,
    Dangling,
}

/// One output row's rendered label and target kind.
#[derive(Debug, Clone, PartialEq)]
pub struct RowSig {
    pub text: String,
    pub target: TargetKind,
}

/// Everything about a node that changes what is drawn (position excluded — moves are cheap).
#[derive(Debug, Clone, PartialEq)]
pub struct NodeSig {
    pub id: String,
    pub speaker: Option<String>,
    pub text: String,
    pub labels: Vec<String>,
    pub rows: Vec<RowSig>,
    pub is_start: bool,
    pub is_selected: bool,
    pub has_error: bool,
    /// Font-metrics availability: part of the signature so that arriving metrics respawn the
    /// node and add its texts.
    pub metrics_ready: bool,
}

/// A node in a [`Snapshot`]: key, signature and world position.
#[derive(Debug, Clone, PartialEq)]
pub struct NodeView {
    pub key: NodeKey,
    pub sig: NodeSig,
    pub pos: Vec2,
}

/// An edge in a [`Snapshot`]: identified by `(from_key, slot)`, `from`/`to` are WORLD positions
/// of the source output pin and the target input pin.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeView {
    pub from_key: NodeKey,
    pub slot: Slot,
    pub target: NodeKey,
    pub from: Vec2,
    pub to: Vec2,
    pub selected_source: bool,
}

/// A point-in-time capture of one doc, in deterministic order (nodes in doc order, then edges).
/// The ECS side remembers the last executed snapshot as the `prev` side of the next diff;
/// `Default` (doc_id 0, revision 0, nothing, metrics not ready) means "nothing shown".
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub doc_id: u64,
    pub revision: u64,
    pub metrics_ready: bool,
    pub nodes: Vec<NodeView>,
    pub edges: Vec<EdgeView>,
}

/// One step of a projection plan.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProjOp {
    DespawnAll,
    SpawnNode(NodeKey),
    RespawnNode(NodeKey),
    MoveNode(NodeKey, Vec2),
    DespawnNode(NodeKey),
    SpawnEdge(NodeKey, Slot),
    RewriteEdge(NodeKey, Slot),
    DespawnEdge(NodeKey, Slot),
}

/// Whether a plan changes the entity STRUCTURE (any spawn/despawn), as opposed to only moving
/// nodes or rewriting edges in place. Only structural plans are worth logging: a plan made of
/// just `MoveNode`/`RewriteEdge` happens on every tick while a node is dragged.
pub fn is_structural(ops: &[ProjOp]) -> bool {
    ops.iter().any(|op| {
        matches!(
            op,
            ProjOp::DespawnAll
                | ProjOp::SpawnNode(_)
                | ProjOp::RespawnNode(_)
                | ProjOp::DespawnNode(_)
                | ProjOp::SpawnEdge(_, _)
                | ProjOp::DespawnEdge(_, _)
        )
    })
}

fn target_kind(link: &Link) -> TargetKind {
    match link {
        Link::End => TargetKind::End,
        Link::Node(_) => TargetKind::Node,
        Link::Dangling(_) => TargetKind::Dangling,
    }
}

/// Captures `doc` as a [`Snapshot`]. `has_error` comes from the doc's lint problems (an
/// [`Severity::Error`] whose node is that node); edges exist only for `Link::Node` targets
/// that still exist; `from`/`to` are the world positions of the output/input pins.
pub fn snapshot_of(doc: &mut DialogueDoc, metrics_ready: bool) -> Snapshot {
    let error_nodes: FxHashSet<NodeKey> = {
        let problems = doc.problems();
        problems
            .iter()
            .filter(|p| p.severity == Severity::Error)
            .filter_map(|p| p.node)
            .collect()
    };

    let nodes = doc
        .nodes
        .iter()
        .map(|node| {
            let rows = (0..geometry::rows(node))
                .map(|i| RowSig {
                    text: row_text(node, i),
                    target: target_kind(row_link(node, i)),
                })
                .collect();
            NodeView {
                key: node.key,
                sig: NodeSig {
                    id: node.id.clone(),
                    speaker: node.speaker.clone(),
                    text: node.text.clone(),
                    labels: node.labels.clone(),
                    rows,
                    is_start: doc.start == Some(node.key),
                    is_selected: doc.selected == Some(node.key),
                    has_error: error_nodes.contains(&node.key),
                    metrics_ready,
                },
                pos: node.pos,
            }
        })
        .collect();

    let node_by_key: FxHashMap<NodeKey, &EdNode> = doc.nodes.iter().map(|n| (n.key, n)).collect();

    let mut edges = Vec::new();
    for node in &doc.nodes {
        for i in 0..geometry::rows(node) {
            if let Link::Node(target) = row_link(node, i)
                && let Some(&target_node) = node_by_key.get(target)
            {
                edges.push(EdgeView {
                    from_key: node.key,
                    slot: geometry::slot_at(node, i),
                    target: *target,
                    from: geometry::output_pin(node, i),
                    to: geometry::input_pin(target_node),
                    selected_source: doc.selected == Some(node.key),
                });
            }
        }
    }

    Snapshot {
        doc_id: doc.doc_id,
        revision: doc.revision,
        metrics_ready,
        nodes,
        edges,
    }
}

/// Diffs `next` against the remembered `prev` snapshot. Rules: a different `doc_id` (or a
/// first non-empty snapshot after nothing was shown) rebuilds from scratch
/// (`[DespawnAll, Spawn*...]`); a new node spawns; a changed signature respawns; only a changed
/// position moves; a gone node despawns together with every edge touching it; a new edge
/// spawns; changed `from`/`to`/`selected_source` rewrites the edge (never respawned for a move);
/// a gone edge despawns. Identical snapshots produce no ops.
pub fn plan_projection(prev: &Snapshot, next: &Snapshot) -> Vec<ProjOp> {
    let prev_nothing = prev.doc_id == 0 && prev.nodes.is_empty() && prev.edges.is_empty();
    let next_nothing = next.doc_id == 0 && next.nodes.is_empty() && next.edges.is_empty();
    if prev.doc_id != next.doc_id || (prev_nothing && !next_nothing) {
        let mut ops = vec![ProjOp::DespawnAll];
        ops.extend(next.nodes.iter().map(|n| ProjOp::SpawnNode(n.key)));
        ops.extend(
            next.edges
                .iter()
                .map(|e| ProjOp::SpawnEdge(e.from_key, e.slot)),
        );
        return ops;
    }

    let mut ops = Vec::new();

    // Nodes: spawn / respawn / move, in next (doc) order.
    let prev_nodes: FxHashMap<NodeKey, &NodeView> = prev.nodes.iter().map(|n| (n.key, n)).collect();
    for nv in &next.nodes {
        match prev_nodes.get(&nv.key) {
            None => ops.push(ProjOp::SpawnNode(nv.key)),
            Some(pv) => {
                if pv.sig != nv.sig {
                    ops.push(ProjOp::RespawnNode(nv.key));
                } else if pv.pos != nv.pos {
                    ops.push(ProjOp::MoveNode(nv.key, nv.pos));
                }
            }
        }
    }

    // Gone nodes: despawn the node and every edge touching it (in prev order).
    let next_keys: FxHashSet<NodeKey> = next.nodes.iter().map(|n| n.key).collect();
    let mut despawned_edges: FxHashSet<(NodeKey, Slot)> = FxHashSet::default();
    for pv in &prev.nodes {
        if !next_keys.contains(&pv.key) {
            ops.push(ProjOp::DespawnNode(pv.key));
            for e in prev
                .edges
                .iter()
                .filter(|e| e.from_key == pv.key || e.target == pv.key)
            {
                ops.push(ProjOp::DespawnEdge(e.from_key, e.slot));
                despawned_edges.insert((e.from_key, e.slot));
            }
        }
    }

    // Edges: spawn / rewrite, in next order.
    let prev_edges: FxHashMap<(NodeKey, Slot), &EdgeView> = prev
        .edges
        .iter()
        .map(|e| ((e.from_key, e.slot), e))
        .collect();
    for ev in &next.edges {
        match prev_edges.get(&(ev.from_key, ev.slot)) {
            None => ops.push(ProjOp::SpawnEdge(ev.from_key, ev.slot)),
            Some(pe) => {
                if pe.from != ev.from
                    || pe.to != ev.to
                    || pe.selected_source != ev.selected_source
                    || pe.target != ev.target
                {
                    ops.push(ProjOp::RewriteEdge(ev.from_key, ev.slot));
                }
            }
        }
    }

    // Gone edges despawn (edges of deleted nodes were already emitted above).
    let next_edge_keys: FxHashSet<(NodeKey, Slot)> =
        next.edges.iter().map(|e| (e.from_key, e.slot)).collect();
    for pe in &prev.edges {
        let k = (pe.from_key, pe.slot);
        if !despawned_edges.contains(&k) && !next_edge_keys.contains(&k) {
            ops.push(ProjOp::DespawnEdge(pe.from_key, pe.slot));
        }
    }

    ops
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::fixtures::{DANGLING, GUARD, LOOPS, doc_from};
    use crate::model::Workspace;

    /// A workspace with one doc (guard.json) so it gets a real `doc_id`.
    fn ws_with(json: &str) -> Workspace {
        let mut ws = Workspace::default();
        ws.add_doc(doc_from(json));
        ws
    }

    fn snap(doc: &mut DialogueDoc) -> Snapshot {
        snapshot_of(doc, true)
    }

    #[test]
    fn snapshot_captures_world_pin_positions_and_lint_errors() {
        let mut ws = ws_with(DANGLING);
        let doc = ws.active_doc_mut().unwrap();
        assert_eq!(doc.doc_id, 1);
        let s = snap(doc);
        assert_eq!(s.doc_id, 1);
        assert_eq!(s.nodes.len(), 2);
        assert!(s.nodes[0].sig.has_error, "dangling next is an Error");
        assert!(s.nodes[1].sig.has_error, "dangling choice is an Error");
        // node a (key 1) has a dangling next: no edge for it; node b has one End choice.
        assert!(s.edges.is_empty());

        let mut ws = ws_with(GUARD);
        let doc = ws.active_doc_mut().unwrap();
        let s = snap(doc);
        // guard: greet(1) choices -> friend(2), pass(3), End; pass(3) next -> friend(2).
        assert_eq!(s.nodes.len(), 3);
        assert!(s.nodes[0].sig.is_start);
        assert_eq!(s.edges.len(), 3);
        let e = &s.edges[0];
        assert_eq!(
            (e.from_key, e.slot, e.target),
            (NodeKey(1), Slot::Choice(0), NodeKey(2))
        );
        let greet = doc.node(NodeKey(1)).unwrap();
        let friend = doc.node(NodeKey(2)).unwrap();
        assert_eq!(
            e.from,
            geometry::output_pin(greet, 0),
            "from = world output pin"
        );
        assert_eq!(e.to, geometry::input_pin(friend), "to = world input pin");
        assert!(!e.selected_source);
        let e = &s.edges[2];
        assert_eq!(
            (e.from_key, e.slot, e.target),
            (NodeKey(3), Slot::Next, NodeKey(2))
        );
    }

    #[test]
    fn moving_a_node_yields_only_move_and_rewrite_ops() {
        let mut ws = ws_with(GUARD);
        let doc = ws.active_doc_mut().unwrap();
        let shown = snap(doc);

        let new_pos = Vec2::new(500.0, 300.0);
        doc.set_node_pos(NodeKey(3), new_pos); // pass -> friend
        let after = snap(doc);
        let ops = plan_projection(&shown, &after);
        assert_eq!(
            ops,
            vec![
                ProjOp::MoveNode(NodeKey(3), new_pos),
                ProjOp::RewriteEdge(NodeKey(1), Slot::Choice(1)), // edge into pass: `to` moved
                ProjOp::RewriteEdge(NodeKey(3), Slot::Next),      // edge out of pass: `from` moved
            ],
            "a move never spawns or respawns"
        );
        assert!(!ops.iter().any(|op| matches!(
            op,
            ProjOp::SpawnNode(_)
                | ProjOp::RespawnNode(_)
                | ProjOp::DespawnNode(_)
                | ProjOp::SpawnEdge(_, _)
                | ProjOp::DespawnEdge(_, _)
        )));
    }

    #[test]
    fn renaming_a_node_respawns_just_that_node() {
        let mut ws = ws_with(GUARD);
        let doc = ws.active_doc_mut().unwrap();
        let shown = snap(doc);

        doc.rename_node(NodeKey(2), "pal".to_owned());
        let ops = plan_projection(&shown, &snap(doc));
        assert_eq!(ops, vec![ProjOp::RespawnNode(NodeKey(2))]);
    }

    #[test]
    fn deleting_a_node_despawns_it_and_its_edges() {
        let mut ws = ws_with(GUARD);
        let doc = ws.active_doc_mut().unwrap();
        let shown = snap(doc);

        doc.delete_node(NodeKey(1)); // greet: edges (1,Choice0) -> 2 and (1,Choice1) -> 3
        let ops = plan_projection(&shown, &snap(doc));
        assert_eq!(
            ops,
            vec![
                ProjOp::DespawnNode(NodeKey(1)),
                ProjOp::DespawnEdge(NodeKey(1), Slot::Choice(0)),
                ProjOp::DespawnEdge(NodeKey(1), Slot::Choice(1)),
            ]
        );
    }

    #[test]
    fn adding_a_node_and_a_link_spawns_node_and_edge_and_respawns_the_source() {
        let mut ws = ws_with(GUARD);
        let doc = ws.active_doc_mut().unwrap();
        let shown = snap(doc);

        let c = doc.add_node(Vec2::new(700.0, 0.0));
        doc.set_link(NodeKey(2), Slot::Next, Link::Node(c)); // friend: End -> Node(c)
        let ops = plan_projection(&shown, &snap(doc));
        assert_eq!(
            ops,
            vec![
                ProjOp::RespawnNode(NodeKey(2)), // row target kind/text changed
                ProjOp::SpawnNode(c),
                ProjOp::SpawnEdge(NodeKey(2), Slot::Next),
            ]
        );
    }

    #[test]
    fn switching_doc_id_rebuilds_everything() {
        let mut ws = ws_with(GUARD);
        let shown = snap(ws.active_doc_mut().unwrap());

        ws.add_doc(doc_from(LOOPS)); // doc_id 2, four nodes a..d
        let next = snap(ws.active_doc_mut().unwrap());
        assert_eq!(next.doc_id, 2);
        let ops = plan_projection(&shown, &next);
        assert_eq!(ops[0], ProjOp::DespawnAll);
        assert_eq!(
            ops.len(),
            1 + 4 + 5,
            "DespawnAll + 4 SpawnNode + 5 SpawnEdge"
        );
        assert!(
            ops[1..5]
                .iter()
                .all(|op| matches!(op, ProjOp::SpawnNode(_)))
        );
        assert!(
            ops[5..]
                .iter()
                .all(|op| matches!(op, ProjOp::SpawnEdge(_, _)))
        );
        assert_eq!(ops[5], ProjOp::SpawnEdge(NodeKey(1), Slot::Choice(0)));
    }

    #[test]
    fn selecting_a_node_respawns_it_and_rewrites_only_its_outgoing_edges() {
        let mut ws = ws_with(GUARD);
        let doc = ws.active_doc_mut().unwrap();
        let shown = snap(doc);

        doc.select(Some(NodeKey(1))); // greet
        let ops = plan_projection(&shown, &snap(doc));
        assert_eq!(
            ops,
            vec![
                ProjOp::RespawnNode(NodeKey(1)),
                ProjOp::RewriteEdge(NodeKey(1), Slot::Choice(0)),
                ProjOp::RewriteEdge(NodeKey(1), Slot::Choice(1)),
            ],
            "selected_source only changes the selected node's own edges"
        );
    }

    #[test]
    fn identical_snapshots_give_no_ops() {
        let mut ws = ws_with(GUARD);
        let doc = ws.active_doc_mut().unwrap();
        let shown = snap(doc);
        let again = snap(doc);
        assert_eq!(plan_projection(&shown, &again), Vec::<ProjOp>::new());
        // Also when nothing is shown and the next snapshot is empty.
        let empty = Snapshot {
            metrics_ready: true,
            ..Snapshot::default()
        };
        assert_eq!(
            plan_projection(&Snapshot::default(), &empty),
            Vec::<ProjOp>::new()
        );
    }

    #[test]
    fn closing_the_active_doc_despawns_everything() {
        let mut ws = ws_with(GUARD);
        let shown = snap(ws.active_doc_mut().unwrap());
        assert!(!shown.nodes.is_empty());

        let empty = Snapshot {
            metrics_ready: true,
            ..Snapshot::default()
        };
        assert_eq!(plan_projection(&shown, &empty), vec![ProjOp::DespawnAll]);
    }

    #[test]
    fn is_structural_is_true_only_for_spawn_or_despawn_ops() {
        assert!(!is_structural(&[]), "an empty plan is not structural");
        assert!(
            !is_structural(&[
                ProjOp::MoveNode(NodeKey(1), Vec2::new(5.0, 5.0)),
                ProjOp::RewriteEdge(NodeKey(1), Slot::Next),
            ]),
            "move/rewrite-only plans (a drag) stay silent"
        );
        for op in [
            ProjOp::DespawnAll,
            ProjOp::SpawnNode(NodeKey(1)),
            ProjOp::RespawnNode(NodeKey(1)),
            ProjOp::DespawnNode(NodeKey(1)),
            ProjOp::SpawnEdge(NodeKey(1), Slot::Next),
            ProjOp::DespawnEdge(NodeKey(1), Slot::Next),
        ] {
            assert!(is_structural(&[op]), "{op:?} is structural");
            assert!(
                is_structural(&[ProjOp::MoveNode(NodeKey(2), Vec2::ZERO), op]),
                "one structural op makes the whole plan structural: {op:?}"
            );
        }
    }

    #[test]
    fn metrics_readiness_is_part_of_the_node_sig() {
        let mut ws = ws_with(GUARD);
        let doc = ws.active_doc_mut().unwrap();
        let shown = snapshot_of(doc, false);
        let ops = plan_projection(&shown, &snapshot_of(doc, true));
        assert_eq!(
            ops,
            vec![
                ProjOp::RespawnNode(NodeKey(1)),
                ProjOp::RespawnNode(NodeKey(2)),
                ProjOp::RespawnNode(NodeKey(3)),
            ],
            "metrics arriving respawns every node (to add texts), edges are untouched"
        );
    }
}

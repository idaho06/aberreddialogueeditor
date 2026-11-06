//! Pure layered auto-layout for the graph view: left-to-right columns by BFS depth from the
//! start node. No ECS, no imgui — only `Vec2` from the engine prelude.
//!
//! `auto_layout` writes positions through `DialogueDoc::set_node_pos`, which bumps `revision`
//! but never `dirty`: laying out a doc is a view-only change. It is deterministic and
//! terminates on cycles and self-loops (BFS only ever assigns a node once).

use std::collections::{HashMap, VecDeque};

use aberredengine::prelude::Vec2;

use crate::graph::geometry::node_height;
use crate::graph::style::{COL_GAP, NODE_W, ROW_GAP};
use crate::model::{DialogueDoc, EdNode, Link, NodeKey, Outputs};

/// Outgoing `Link::Node` targets of `node` in slot order (the `next` slot, then the choices).
fn node_targets(node: &EdNode) -> Vec<NodeKey> {
    match &node.out {
        Outputs::Next(Link::Node(target)) => vec![*target],
        Outputs::Next(_) => Vec::new(),
        Outputs::Choices(choices) => choices
            .iter()
            .filter_map(|choice| match choice.target {
                Link::Node(target) => Some(target),
                _ => None,
            })
            .collect(),
    }
}

/// BFS from `seed` (assigned layer `seed_layer`), extending `depth` and `layers`. A node that
/// already has a depth is skipped, so back edges (target depth <= source depth), cycles and
/// self-loops never reassign anything.
fn bfs(
    adjacency: &[Vec<usize>],
    depth: &mut [Option<usize>],
    layers: &mut Vec<Vec<usize>>,
    seed: usize,
    seed_layer: usize,
) {
    let mut queue = VecDeque::from([seed]);
    depth[seed] = Some(seed_layer);
    while let Some(node) = queue.pop_front() {
        let layer = depth[node].expect("queued nodes have a depth");
        while layers.len() <= layer {
            layers.push(Vec::new());
        }
        layers[layer].push(node);
        for &target in &adjacency[node] {
            if depth[target].is_none() {
                depth[target] = Some(layer + 1);
                queue.push_back(target);
            }
        }
    }
}

/// Mean position of `node`'s predecessors in layer `layer - 1` (back edges and edges from
/// other layers are ignored). A node without such a predecessor keeps its current position.
fn barycenter(
    node: usize,
    layer: usize,
    predecessors: &[Vec<usize>],
    depth: &[Option<usize>],
    position: &[usize],
) -> f32 {
    let mut sum = 0usize;
    let mut count = 0usize;
    for &pred in &predecessors[node] {
        if depth[pred] == Some(layer - 1) {
            sum += position[pred];
            count += 1;
        }
    }
    if count == 0 {
        position[node] as f32
    } else {
        sum as f32 / count as f32
    }
}

/// Layered left-to-right auto-layout of `doc`:
///
/// a) a node's layer is its BFS shortest-path depth from `doc.start` (back edges are ignored
///    for ordering); nodes unreachable from the start — or all nodes when there is no start —
///    get trailing layers, BFS-ed from each remaining node in node order after the last
///    reachable layer;
/// b) within a layer, nodes are ordered by the barycenter of their predecessors' positions in
///    the previous layer (two forward sweeps, ties broken by node order; initial order is node
///    order);
/// c) `x = layer * (NODE_W + COL_GAP)`; within a layer nodes are stacked top to bottom with
///    `ROW_GAP` between them and the layer is centred on `y = 0` (the centre line of the
///    tallest layer).
pub fn auto_layout(doc: &mut DialogueDoc) {
    let node_count = doc.nodes.len();
    if node_count == 0 {
        return;
    }

    // Key -> node index; adjacency and predecessors are built in node order for determinism.
    let index_of: HashMap<NodeKey, usize> = doc
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| (node.key, i))
        .collect();
    let adjacency: Vec<Vec<usize>> = doc
        .nodes
        .iter()
        .map(|node| {
            node_targets(node)
                .into_iter()
                .filter_map(|key| index_of.get(&key).copied())
                .collect()
        })
        .collect();
    let mut predecessors: Vec<Vec<usize>> = vec![Vec::new(); node_count];
    for (source, targets) in adjacency.iter().enumerate() {
        for &target in targets {
            predecessors[target].push(source);
        }
    }

    // a) Layers by BFS depth from the start, then trailing layers for the unreachable rest.
    let mut depth: Vec<Option<usize>> = vec![None; node_count];
    let mut layers: Vec<Vec<usize>> = Vec::new();
    if let Some(start) = doc.start.and_then(|key| index_of.get(&key).copied()) {
        bfs(&adjacency, &mut depth, &mut layers, start, 0);
    }
    for node in 0..node_count {
        if depth[node].is_none() {
            let seed_layer = layers.len();
            bfs(&adjacency, &mut depth, &mut layers, node, seed_layer);
        }
    }

    // b) Initial order is node order, then two forward barycenter sweeps.
    for layer in layers.iter_mut() {
        layer.sort_unstable();
    }
    let mut position: Vec<usize> = vec![0; node_count];
    for _ in 0..2 {
        for layer in &layers {
            for (p, &node) in layer.iter().enumerate() {
                position[node] = p;
            }
        }
        for (layer_index, layer) in layers.iter_mut().enumerate().skip(1) {
            let mut keyed: Vec<(f32, usize)> = layer
                .iter()
                .map(|&node| {
                    (
                        barycenter(node, layer_index, &predecessors, &depth, &position),
                        node,
                    )
                })
                .collect();
            keyed.sort_by(|a, b| {
                a.0.partial_cmp(&b.0)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.1.cmp(&b.1))
            });
            *layer = keyed.into_iter().map(|(_, node)| node).collect();
            for (p, &node) in layer.iter().enumerate() {
                position[node] = p;
            }
        }
    }

    // c) Columns at layer * (NODE_W + COL_GAP), each layer centred on y = 0.
    let column_width = NODE_W + COL_GAP;
    let layer_heights: Vec<f32> = layers
        .iter()
        .map(|layer| {
            layer
                .iter()
                .map(|&node| node_height(&doc.nodes[node]))
                .sum::<f32>()
                + ROW_GAP * layer.len().saturating_sub(1) as f32
        })
        .collect();
    for (layer_index, layer) in layers.iter().enumerate() {
        let x = layer_index as f32 * column_width;
        let mut y = -layer_heights[layer_index] / 2.0;
        for &node in layer {
            let key = doc.nodes[node].key;
            let height = node_height(&doc.nodes[node]);
            doc.set_node_pos(key, Vec2::new(x, y));
            y += height + ROW_GAP;
        }
    }
    log::debug!("auto_layout: {node_count} nodes in {} layers", layers.len());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::geometry::node_rect;
    use crate::model::from_def;
    use aberredengine::prelude::DialogueDef;

    const GUARD: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/guard.json"
    ));
    const LOOPS: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/loops.json"
    ));

    fn doc_from(json: &str) -> DialogueDoc {
        let def: DialogueDef = serde_json::from_str(json).expect("fixture must parse");
        from_def(&def)
    }

    fn guard_doc() -> DialogueDoc {
        doc_from(GUARD)
    }

    fn loops_doc() -> DialogueDoc {
        doc_from(LOOPS)
    }

    fn positions(doc: &DialogueDoc) -> Vec<(NodeKey, Vec2)> {
        doc.nodes.iter().map(|node| (node.key, node.pos)).collect()
    }

    fn pos_of(doc: &DialogueDoc, key: NodeKey) -> Vec2 {
        doc.node(key).expect("node exists").pos
    }

    /// No two node rects overlap (touching edges do not count).
    fn assert_no_overlap(doc: &DialogueDoc) {
        for (i, a) in doc.nodes.iter().enumerate() {
            for b in doc.nodes.iter().skip(i + 1) {
                let (a_min, a_max) = node_rect(a);
                let (b_min, b_max) = node_rect(b);
                let overlaps = a_min.x < b_max.x
                    && b_min.x < a_max.x
                    && a_min.y < b_max.y
                    && b_min.y < a_max.y;
                assert!(!overlaps, "nodes \"{}\" and \"{}\" overlap", a.id, b.id);
            }
        }
    }

    fn assert_distinct_positions(doc: &DialogueDoc) {
        for (i, a) in doc.nodes.iter().enumerate() {
            for b in doc.nodes.iter().skip(i + 1) {
                assert_ne!(
                    a.pos, b.pos,
                    "nodes \"{}\" and \"{}\" share a position",
                    a.id, b.id
                );
            }
        }
    }

    #[test]
    fn guard_start_is_in_layer_zero_and_no_rects_overlap() {
        let mut doc = guard_doc();
        auto_layout(&mut doc);
        let start = doc.start.expect("guard.json has a start");
        let start_x = doc.node(start).expect("start exists").pos.x;
        assert_eq!(start_x, 0.0, "the start node is in layer 0 (smallest x)");
        for node in &doc.nodes {
            if node.key != start {
                assert!(
                    node.pos.x > start_x,
                    "node \"{}\" is right of the start",
                    node.id
                );
            }
        }
        assert_no_overlap(&doc);
        assert_distinct_positions(&doc);
    }

    #[test]
    fn layout_is_deterministic_laying_out_twice_gives_equal_positions() {
        for json in [GUARD, LOOPS] {
            let mut doc = doc_from(json);
            auto_layout(&mut doc);
            let first = positions(&doc);
            auto_layout(&mut doc);
            assert_eq!(first, positions(&doc), "laying out twice is identical");
        }
    }

    #[test]
    fn loops_fixture_terminates_and_layers_by_depth() {
        let mut doc = loops_doc();
        auto_layout(&mut doc); // must terminate despite the a <-> b cycle
        let column = NODE_W + COL_GAP;
        let x = |id: &str| {
            doc.nodes
                .iter()
                .find(|node| node.id == id)
                .unwrap_or_else(|| panic!("node {id} exists"))
                .pos
                .x
        };
        assert_eq!(x("a"), 0.0, "start a is in layer 0");
        assert_eq!(x("b"), column, "b is one edge from a");
        assert_eq!(x("d"), column, "d is one edge from a");
        assert_eq!(
            x("c"),
            2.0 * column,
            "c is two edges from a (the b->a back edge is ignored)"
        );
        assert_no_overlap(&doc);
        assert_distinct_positions(&doc);
    }

    #[test]
    fn unreachable_nodes_land_right_of_all_reachable_ones() {
        let mut doc = DialogueDoc::new_empty();
        let a = doc.add_node(Vec2::ZERO); // becomes start
        let b = doc.add_node(Vec2::ZERO);
        let c = doc.add_node(Vec2::ZERO);
        doc.set_link(a, crate::model::Slot::Next, Link::Node(b));
        auto_layout(&mut doc);
        assert!(
            pos_of(&doc, c).x > pos_of(&doc, a).x && pos_of(&doc, c).x > pos_of(&doc, b).x,
            "the unreachable node is right of every reachable node"
        );

        // With no start, ALL nodes are trailing: BFS from each remaining node in node order,
        // starting at layer 0.
        doc.start = None;
        auto_layout(&mut doc);
        assert_eq!(
            pos_of(&doc, a).x,
            0.0,
            "the first node in node order gets layer 0"
        );
        assert_distinct_positions(&doc);
    }

    #[test]
    fn empty_doc_and_single_node_do_not_panic() {
        let mut empty = DialogueDoc::new_empty();
        auto_layout(&mut empty); // no nodes: no-op
        assert!(empty.nodes.is_empty());

        let mut single = DialogueDoc::new_empty();
        let only = single.add_node(Vec2::new(50.0, 60.0));
        auto_layout(&mut single);
        let node = single.node(only).expect("node exists");
        let height = node_height(node);
        assert_eq!(node.pos.x, 0.0, "a single node sits in layer 0");
        assert!(
            (node.pos.y - (-height / 2.0)).abs() < 1e-5,
            "a single node is centred on y = 0"
        );

        // A self-loop must terminate too.
        let mut looped = DialogueDoc::new_empty();
        let only = looped.add_node(Vec2::ZERO);
        looped.set_link(only, crate::model::Slot::Next, Link::Node(only));
        auto_layout(&mut looped);
        assert_eq!(looped.node(only).expect("node exists").pos.x, 0.0);
    }

    #[test]
    fn auto_layout_leaves_dirty_false() {
        let mut doc = guard_doc();
        assert!(!doc.dirty, "a freshly converted doc is clean");
        auto_layout(&mut doc);
        assert!(!doc.dirty, "layout must not dirty the doc");
        assert_eq!(
            doc.revision,
            doc.nodes.len() as u64,
            "each set_node_pos bumps the revision exactly once"
        );
    }
}

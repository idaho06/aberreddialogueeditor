//! Pure node geometry, hit testing and edge curves for the graph view. No ECS, no imgui —
//! only `Vec2` from the engine prelude. World coords, y down; a node's `pos` is its top-left.

use aberredengine::prelude::Vec2;

use crate::graph::style::{HEADER_H, NODE_W, ROW_H, TEXT_AREA_H};
use crate::model::{DialogueDoc, EdNode, NodeKey, Outputs, Slot};

/// Number of output rows a node has: one per choice, or one for a `Next` node.
pub fn rows(node: &EdNode) -> usize {
    match &node.out {
        Outputs::Choices(choices) => choices.len(),
        Outputs::Next(_) => 1,
    }
}

/// Total node height: header + fixed text area + one row per output slot.
pub fn node_height(node: &EdNode) -> f32 {
    HEADER_H + TEXT_AREA_H + rows(node).max(1) as f32 * ROW_H
}

/// Node body rect as `(min, max)` in world coords.
pub fn node_rect(node: &EdNode) -> (Vec2, Vec2) {
    let min = node.pos;
    let max = min + Vec2::new(NODE_W, node_height(node));
    (min, max)
}

/// Centre of the node's input pin: left edge, vertically centred on the header.
pub fn input_pin(node: &EdNode) -> Vec2 {
    Vec2::new(node.pos.x, node.pos.y + HEADER_H / 2.0)
}

/// Centre of output pin `slot_index`: right edge, in the row below the text area.
pub fn output_pin(node: &EdNode, slot_index: usize) -> Vec2 {
    Vec2::new(
        node.pos.x + NODE_W,
        node.pos.y + HEADER_H + TEXT_AREA_H + (slot_index as f32 + 0.5) * ROW_H,
    )
}

/// What a world-space point hits on the canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    OutputPin(NodeKey, Slot),
    InputPin(NodeKey),
    Body(NodeKey),
    Empty,
}

/// Hit test at world point `p`. Pins take priority over bodies (they are drawn above them);
/// among overlapping nodes the one LATER in `doc.nodes` order wins (it is drawn on top).
/// `pin_hit_radius` is the pin click radius (callers pass `PIN_R + 4` scaled by `1/zoom`).
pub fn hit_test(doc: &DialogueDoc, p: Vec2, pin_hit_radius: f32) -> Hit {
    for node in doc.nodes.iter().rev() {
        for i in 0..rows(node) {
            let slot = match &node.out {
                Outputs::Next(_) => Slot::Next,
                Outputs::Choices(_) => Slot::Choice(i),
            };
            if (output_pin(node, i) - p).length() <= pin_hit_radius {
                return Hit::OutputPin(node.key, slot);
            }
        }
        if (input_pin(node) - p).length() <= pin_hit_radius {
            return Hit::InputPin(node.key);
        }
    }
    for node in doc.nodes.iter().rev() {
        let (min, max) = node_rect(node);
        if p.x >= min.x && p.x <= max.x && p.y >= min.y && p.y <= max.y {
            return Hit::Body(node.key);
        }
    }
    Hit::Empty
}

/// Cubic bezier control points for an edge, RELATIVE to `from` (`Shape::bezier` is local to
/// its entity), plus the endpoint relative to `from`. Forward case (`to.x >= from.x + 20`)
/// leaves horizontally; back/self case loops below both pins.
pub fn edge_curve(from: Vec2, to: Vec2) -> (Vec2, Vec2, Vec2) {
    let to_rel = to - from;
    if to.x >= from.x + 20.0 {
        let dx = 60.0_f32.max((to.x - from.x) / 2.0);
        let c1 = Vec2::new(dx, 0.0);
        let c2 = to_rel - Vec2::new(dx, 0.0);
        (c1, c2, to_rel)
    } else {
        let h = 80.0 + (to.y - from.y).abs() / 2.0;
        let c1 = Vec2::new(80.0, h);
        let c2 = to_rel + Vec2::new(-80.0, h);
        (c1, c2, to_rel)
    }
}

/// Arrowhead direction at the edge end: the bezier tangent at t=1, i.e. normalized
/// `to_rel - c2_rel` (control points relative to `from`). Falls back to `(1, 0)` when the
/// tangent has zero length.
pub fn arrow_tip_dir(c2_rel: Vec2, to_rel: Vec2) -> Vec2 {
    let d = to_rel - c2_rel;
    if d.length_squared() > 0.0 {
        d / d.length()
    } else {
        Vec2::new(1.0, 0.0)
    }
}

/// Union of all node rects as `(min, max)`; `None` when the doc has no nodes.
pub fn graph_bounds(doc: &DialogueDoc) -> Option<(Vec2, Vec2)> {
    let mut iter = doc.nodes.iter();
    let (mut min, mut max) = node_rect(iter.next()?);
    for node in iter {
        let (nmin, nmax) = node_rect(node);
        min = min.min(nmin);
        max = max.max(nmax);
    }
    Some((min, max))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::from_def;
    use aberredengine::prelude::DialogueDef;

    const GUARD: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/guard.json"
    ));

    /// guard.json: keys are 1-based in node order — greet(1, 3 choices), friend(2, Next(End)),
    /// pass(3, Next(friend)).
    fn guard_doc() -> DialogueDoc {
        let def: DialogueDef = serde_json::from_str(GUARD).expect("fixture must parse");
        from_def(&def)
    }

    fn close(a: Vec2, b: Vec2) -> bool {
        a.abs_diff_eq(b, 1e-5)
    }

    fn node_at(doc: &DialogueDoc, key: NodeKey, pos: Vec2) -> EdNode {
        let mut node = doc.node(key).expect("node exists").clone();
        node.pos = pos;
        node
    }

    #[test]
    fn rows_and_height_next_node_vs_three_choices() {
        let doc = guard_doc();
        let friend = node_at(&doc, NodeKey(2), Vec2::ZERO); // Next(End)
        let greet = node_at(&doc, NodeKey(1), Vec2::ZERO); // 3 choices
        assert_eq!(rows(&friend), 1);
        assert_eq!(rows(&greet), 3);
        assert!(close(
            Vec2::new(0.0, node_height(&friend)),
            Vec2::new(0.0, HEADER_H + TEXT_AREA_H + ROW_H)
        ));
        assert!(close(
            Vec2::new(0.0, node_height(&greet)),
            Vec2::new(0.0, HEADER_H + TEXT_AREA_H + 3.0 * ROW_H)
        ));
    }

    #[test]
    fn pin_positions_for_next_node_and_three_choice_node() {
        let doc = guard_doc();
        let pos = Vec2::new(100.0, 50.0);
        let friend = node_at(&doc, NodeKey(2), pos); // Next node
        let greet = node_at(&doc, NodeKey(1), pos); // 3 choices

        // input pin: (pos.x, pos.y + HEADER_H/2)
        assert!(close(
            input_pin(&friend),
            Vec2::new(100.0, 50.0 + HEADER_H / 2.0)
        ));
        // Next node output pin (slot 0): (pos.x + NODE_W, pos.y + HEADER_H + TEXT_AREA_H + 0.5*ROW_H)
        assert!(close(
            output_pin(&friend, 0),
            Vec2::new(100.0 + NODE_W, 50.0 + HEADER_H + TEXT_AREA_H + 0.5 * ROW_H)
        ));
        // choice node output pins: slots 0..2 in the rows below the text area
        for i in 0..3 {
            assert!(close(
                output_pin(&greet, i),
                Vec2::new(
                    100.0 + NODE_W,
                    50.0 + HEADER_H + TEXT_AREA_H + (i as f32 + 0.5) * ROW_H
                )
            ));
        }
        // node rect spans pos .. pos + (NODE_W, height)
        let (min, max) = node_rect(&greet);
        assert!(close(min, pos));
        assert!(close(max, pos + Vec2::new(NODE_W, node_height(&greet))));
    }

    #[test]
    fn hit_test_pin_beats_body_and_topmost_node_wins() {
        let mut doc = guard_doc();
        let a = NodeKey(2); // friend, Next node
        let b = NodeKey(3); // pass, Next node
        doc.set_node_pos(a, Vec2::new(0.0, 0.0));
        doc.set_node_pos(b, Vec2::new(10.0, 10.0)); // overlaps a; b is later in doc.nodes

        // A point on a's output pin is inside b's body too: the pin wins over any body.
        let pin_a = output_pin(&node_at(&doc, a, Vec2::new(0.0, 0.0)), 0);
        assert!(node_rect(&node_at(&doc, b, Vec2::new(10.0, 10.0))).0.x <= pin_a.x);
        assert_eq!(
            hit_test(&doc, pin_a, 10.0),
            Hit::OutputPin(a, Slot::Next),
            "pins take priority over bodies"
        );

        // Input pin hit.
        let in_a = input_pin(&node_at(&doc, a, Vec2::new(0.0, 0.0)));
        assert_eq!(hit_test(&doc, in_a, 10.0), Hit::InputPin(a));

        // Overlapping bodies: the later node (drawn on top) wins.
        let overlap = Vec2::new(100.0, 60.0); // inside both rects
        assert_eq!(hit_test(&doc, overlap, 10.0), Hit::Body(b));

        // A point only inside a's body.
        let only_a = Vec2::new(5.0, 100.0);
        assert_eq!(hit_test(&doc, only_a, 10.0), Hit::Body(a));

        // Choice slots map to Slot::Choice(i): greet has 3 choices.
        doc.set_node_pos(NodeKey(1), Vec2::new(1000.0, 1000.0));
        let greet = node_at(&doc, NodeKey(1), Vec2::new(1000.0, 1000.0));
        assert_eq!(
            hit_test(&doc, output_pin(&greet, 2), 10.0),
            Hit::OutputPin(NodeKey(1), Slot::Choice(2))
        );

        // Far away: Empty.
        assert_eq!(hit_test(&doc, Vec2::new(-500.0, -500.0), 10.0), Hit::Empty);
    }

    #[test]
    fn edge_curve_forward_case_is_relative_and_ends_at_to_minus_from() {
        let from = Vec2::new(0.0, 0.0);
        let to = Vec2::new(300.0, 100.0);
        let (c1, c2, to_rel) = edge_curve(from, to);
        assert!(close(to_rel, to - from), "endpoint is `to - from`");
        let dx = 60.0_f32.max((to.x - from.x) / 2.0); // 150
        assert!(close(c1, Vec2::new(dx, 0.0)));
        assert!(close(c2, (to - from) - Vec2::new(dx, 0.0)));
    }

    #[test]
    fn edge_curve_back_case_loops_below_both_pins() {
        let from = Vec2::new(0.0, 0.0);
        let to = Vec2::new(0.0, 50.0); // to.x < from.x + 20 -> back/self case
        let (c1, c2, to_rel) = edge_curve(from, to);
        assert!(close(to_rel, to - from), "endpoint is `to - from`");
        let h = 80.0 + (to.y - from.y).abs() / 2.0;
        assert!(close(c1, Vec2::new(80.0, h)));
        assert!(close(c2, (to - from) + Vec2::new(-80.0, h)));
        assert!(
            c1.y > from.y && c2.y > to.y,
            "back-case control points are below both pins"
        );
    }

    #[test]
    fn arrow_tip_dir_is_tangent_at_t1_with_unit_fallback() {
        let dir = arrow_tip_dir(Vec2::new(150.0, 100.0), Vec2::new(300.0, 100.0));
        assert!(close(dir, Vec2::new(1.0, 0.0)));
        let up = arrow_tip_dir(Vec2::new(0.0, 100.0), Vec2::new(0.0, 0.0));
        assert!(close(up, Vec2::new(0.0, -1.0)));
        let fallback = arrow_tip_dir(Vec2::new(5.0, 5.0), Vec2::new(5.0, 5.0));
        assert!(
            close(fallback, Vec2::new(1.0, 0.0)),
            "zero-length tangent falls back to (1,0)"
        );
    }

    #[test]
    fn graph_bounds_is_union_of_node_rects() {
        let mut doc = guard_doc();
        doc.set_node_pos(NodeKey(1), Vec2::new(100.0, 200.0));
        doc.set_node_pos(NodeKey(2), Vec2::new(-50.0, 0.0));
        doc.set_node_pos(NodeKey(3), Vec2::new(300.0, 10.0));
        let (min, max) = graph_bounds(&doc).expect("doc has nodes");
        let mut want_min = Vec2::new(f32::INFINITY, f32::INFINITY);
        let mut want_max = Vec2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
        for node in &doc.nodes {
            let (nmin, nmax) = node_rect(node);
            want_min = want_min.min(nmin);
            want_max = want_max.max(nmax);
        }
        assert!(close(min, want_min));
        assert!(close(max, want_max));
        assert!(min.x < 0.0 && min.y < 100.0 && max.x >= 300.0 + NODE_W && max.y > 200.0);

        let empty = DialogueDoc::new_empty();
        assert_eq!(graph_bounds(&empty), None);
    }
}

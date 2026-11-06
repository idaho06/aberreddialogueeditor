//! Pure "frame all" helper: fits a doc's graph into the viewport. No ECS, no imgui — only `Vec2`
//! from the engine prelude plus the pure camera/geometry helpers.

use aberredengine::prelude::Vec2;

use super::{frame_all, graph_bounds};
use crate::model::DialogueDoc;

/// Padding in screen pixels kept around the framed graph on every side.
pub const FRAME_MARGIN_PX: f32 = 40.0;

/// Frames the whole doc: `doc.set_camera(frame_all(graph_bounds(doc), viewport, FRAME_MARGIN_PX))`.
/// An empty doc gets the default camera (target ZERO, zoom 1). View-only: `set_camera` bumps neither
/// `revision` nor `dirty`.
pub fn frame_doc(doc: &mut DialogueDoc, viewport: Vec2) {
    doc.set_camera(frame_all(graph_bounds(doc), viewport, FRAME_MARGIN_PX));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::fixtures::{GUARD, doc_from};
    use crate::graph::{auto_layout, node_rect, world_to_screen};
    use crate::model::CamState;

    #[test]
    fn frame_doc_contains_all_node_corners() {
        let viewport = Vec2::new(1280.0, 720.0);
        let mut doc = doc_from(GUARD);
        auto_layout(&mut doc);
        frame_doc(&mut doc, viewport);
        let cam = doc.camera;
        assert!(
            cam != CamState::default(),
            "framing a laid-out doc changes the default camera"
        );
        for node in &doc.nodes {
            let (min, max) = node_rect(node);
            for corner in [min, max, Vec2::new(min.x, max.y), Vec2::new(max.x, min.y)] {
                let s = world_to_screen(&cam, viewport, corner);
                assert!(
                    s.x >= FRAME_MARGIN_PX - 1e-3
                        && s.x <= viewport.x - FRAME_MARGIN_PX + 1e-3
                        && s.y >= FRAME_MARGIN_PX - 1e-3
                        && s.y <= viewport.y - FRAME_MARGIN_PX + 1e-3,
                    "corner {corner:?} of node {:?} maps to {s:?}, outside the padded viewport",
                    node.id
                );
            }
        }
    }

    #[test]
    fn frame_doc_on_an_empty_doc_gives_the_default_camera() {
        let mut doc = DialogueDoc::new_empty();
        frame_doc(&mut doc, Vec2::new(800.0, 600.0));
        assert_eq!(doc.camera, CamState::default());
        assert_eq!(doc.camera.target, Vec2::ZERO);
        assert!((doc.camera.zoom - 1.0).abs() < 1e-6);
    }
}

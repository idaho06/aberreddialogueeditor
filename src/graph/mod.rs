//! Graph-view logic: style constants, node geometry/hit testing, text wrap, camera math,
//! layered auto-layout, pure node descriptions, projection planning, the pure mouse-gesture
//! state machine and its pure action application. No imgui; the only ECS imports live in
//! `components.rs` (markers + the `GraphEntities` resource), `input_system.rs` (the `GraphInput`
//! resource + the sample builder), `camera_system.rs` (the ECS frame system), `projection.rs`
//! (the ECS projection system) and `preview.rs` (the ECS drag-preview system). All other files
//! are pure — engine imports there are limited to `Vec2` and `Color`.

mod apply;
mod camera;
mod camera_system;
mod components;
mod describe;
mod frame;
mod geometry;
mod input_system;
mod interaction;
mod layout;
mod preview;
mod projection;
mod projection_plan;
mod style;
mod wrap;

pub use apply::apply_actions;
pub use camera::{center_on, frame_all, pan_by_screen_delta, screen_to_world, zoom_at};
pub use camera_system::graph_frame_system;
pub use components::{EdgeEntry, GraphEdge, GraphEntities, GraphNode, GraphPart, GraphPreview};
pub use describe::describe_node;
pub use frame::frame_doc;
pub use geometry::{arrow_tip_dir, edge_curve, graph_bounds, node_rect};
pub use input_system::{ButtonSample, GraphInput, build_sample};
pub use interaction::{GraphInteraction, drag_preview, step};
pub use layout::auto_layout;
pub use preview::graph_preview_system;
pub use projection::graph_projection_system;
pub use projection_plan::{plan_projection, snapshot_of};
pub use style::{
    CORNER_RADIUS, EDGE_COLOR, EDGE_SELECTED_COLOR, EDGE_THICKNESS, FONT, FONT_SIZE, HEADER_H,
    NODE_W, OUTLINE_THICKNESS, PIN_R, Z_ARROWHEAD, Z_EDGE, Z_HEADER, Z_NODE_BODY, Z_PIN, Z_PREVIEW,
    Z_TEXT,
};

#[cfg(test)]
pub(crate) mod fixtures {
    //! JSON fixtures and shared helpers for the graph test modules.
    use aberredengine::prelude::{DialogueDef, Vec2};

    use super::auto_layout;
    use crate::model::{DialogueDoc, EdNode, NodeKey, from_def};

    pub const GUARD: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/guard.json"
    ));
    pub const LOOPS: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/loops.json"
    ));
    pub const DANGLING: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/invalid_dangling.json"
    ));

    /// guard.json keys (1-based in node order): greet = 1 (3 choices), friend = 2 (Next(End)),
    /// pass = 3 (Next(friend)).
    pub const GREET: NodeKey = NodeKey(1);
    pub const FRIEND: NodeKey = NodeKey(2);
    pub const PASS: NodeKey = NodeKey(3);

    pub fn doc_from(json: &str) -> DialogueDoc {
        let def: DialogueDef = serde_json::from_str(json).expect("fixture must parse");
        from_def(&def)
    }

    /// Float comparison for geometry assertions.
    pub fn close(a: Vec2, b: Vec2) -> bool {
        a.abs_diff_eq(b, 1e-5)
    }

    /// guard.json with `auto_layout` applied — the laid-out doc the interaction/apply tests drive.
    pub fn guard_doc() -> DialogueDoc {
        let mut doc = doc_from(GUARD);
        auto_layout(&mut doc);
        doc
    }

    /// A clone of `doc`'s node `key` with its position set to `pos`.
    pub fn node_at(doc: &DialogueDoc, key: NodeKey, pos: Vec2) -> EdNode {
        let mut node = doc.node(key).expect("node exists").clone();
        node.pos = pos;
        node
    }
}

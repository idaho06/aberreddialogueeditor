//! Graph-view logic: style constants, node geometry/hit testing, text wrap, camera math,
//! layered auto-layout, pure node descriptions, projection planning, the pure mouse-gesture
//! state machine and its pure action application. No imgui; the only ECS imports live in
//! `components.rs` (markers + the `GraphEntities` resource), `input_system.rs` (the `GraphInput`
//! resource + the sample builder), `camera_system.rs` (the ECS frame system), `projection.rs`
//! (the ECS projection system) and `preview.rs` (the ECS drag-preview system). All other files
//! are pure — engine imports there are limited to `Vec2` and `Color`.
#![allow(dead_code, unused_imports)] // consumed by later plan steps

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
pub use camera::{
    MAX_ZOOM, MIN_ZOOM, frame_all, pan_by_screen_delta, screen_to_world, world_to_screen, zoom_at,
};
pub use camera_system::{apply_wheel, camera_update, graph_frame_system, pan_step};
pub use components::{
    EdgeEntry, GraphEdge, GraphEntities, GraphNode, GraphPart, GraphPreview, NodeEntry,
};
pub use describe::{NodeVisual, PinItem, TextItem, describe_node, row_link, row_text};
pub use frame::{FRAME_MARGIN_PX, frame_doc};
pub use geometry::{
    Hit, arrow_tip_dir, edge_curve, graph_bounds, hit_test, input_pin, node_height, node_rect,
    output_pin, rows,
};
pub use input_system::{ButtonSample, GraphInput, build_sample};
pub use interaction::{Action, GraphInteraction, InputSample, Phase, drag_preview, step};
pub use layout::auto_layout;
pub use preview::graph_preview_system;
pub use projection::{ARROWHEAD_L, ARROWHEAD_W, arrowhead_points, graph_projection_system};
pub use projection_plan::{
    EdgeView, NodeSig, NodeView, ProjOp, RowSig, Shown, Snapshot, TargetKind, plan_projection,
    snapshot_of,
};
pub use style::{
    COL_GAP, CORNER_RADIUS, DIM_TEXT_COLOR, EDGE_COLOR, EDGE_SELECTED_COLOR, EDGE_THICKNESS,
    ERROR_OUTLINE_COLOR, FONT, FONT_SIZE, HEADER_COLOR, HEADER_H, NODE_BODY_COLOR, NODE_W,
    OUTLINE_COLOR, OUTLINE_THICKNESS, PAD, PIN_DANGLING_COLOR, PIN_IN_COLOR, PIN_OUT_COLOR, PIN_R,
    ROW_GAP, ROW_H, SELECTED_OUTLINE_COLOR, START_HEADER_COLOR, TEXT_AREA_H, TEXT_COLOR,
    TEXT_MAX_LINES, Z_ARROWHEAD, Z_EDGE, Z_HEADER, Z_NODE_BODY, Z_PIN, Z_PREVIEW, Z_TEXT,
};
pub use wrap::{fit_line, wrap_text};

#[cfg(test)]
pub(crate) mod fixtures {
    //! JSON fixtures shared by the graph test modules.
    use aberredengine::prelude::DialogueDef;

    use crate::model::{DialogueDoc, from_def};

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

    pub fn doc_from(json: &str) -> DialogueDoc {
        let def: DialogueDef = serde_json::from_str(json).expect("fixture must parse");
        from_def(&def)
    }
}

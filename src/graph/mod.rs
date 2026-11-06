//! Pure graph-view logic: style constants, node geometry/hit testing, text wrap, camera math
//! and layered auto-layout. No ECS, no imgui — the only engine import is `Vec2` from the
//! prelude. Projection and interaction land in later plan steps.
#![allow(dead_code, unused_imports)] // consumed by later plan steps

mod camera;
mod geometry;
mod layout;
mod style;
mod wrap;

pub use camera::{
    MAX_ZOOM, MIN_ZOOM, frame_all, pan_by_screen_delta, screen_to_world, world_to_screen, zoom_at,
};
pub use geometry::{
    Hit, arrow_tip_dir, edge_curve, graph_bounds, hit_test, input_pin, node_height, node_rect,
    output_pin, rows,
};
pub use layout::auto_layout;
pub use style::{
    COL_GAP, CORNER_RADIUS, EDGE_THICKNESS, FONT, FONT_SIZE, HEADER_H, NODE_W, OUTLINE_THICKNESS,
    PAD, PIN_R, ROW_GAP, ROW_H, TEXT_AREA_H, TEXT_MAX_LINES,
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

    pub fn doc_from(json: &str) -> DialogueDoc {
        let def: DialogueDef = serde_json::from_str(json).expect("fixture must parse");
        from_def(&def)
    }
}

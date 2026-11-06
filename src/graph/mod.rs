//! Pure graph-view logic: style constants, node geometry/hit testing, text wrap and camera
//! math. No ECS, no imgui — the only engine import is `Vec2` from the prelude. Projection,
//! layout and interaction land in later plan steps.
#![allow(dead_code, unused_imports)] // consumed by later plan steps

mod camera;
mod geometry;
mod style;
mod wrap;

pub use camera::{
    MAX_ZOOM, MIN_ZOOM, frame_all, pan_by_screen_delta, screen_to_world, world_to_screen, zoom_at,
};
pub use geometry::{
    Hit, arrow_tip_dir, edge_curve, graph_bounds, hit_test, input_pin, node_height, node_rect,
    output_pin, rows,
};
pub use style::{
    COL_GAP, CORNER_RADIUS, EDGE_THICKNESS, FONT, FONT_SIZE, HEADER_H, NODE_W, OUTLINE_THICKNESS,
    PAD, PIN_R, ROW_GAP, ROW_H, TEXT_AREA_H, TEXT_MAX_LINES,
};
pub use wrap::{fit_line, wrap_text};

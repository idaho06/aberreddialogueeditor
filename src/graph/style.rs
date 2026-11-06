//! Style constants of the graph view: numeric sizes/gaps plus the palette and Z indices.
//! The only engine import is `Color` (its `new` is a `const fn`, so the palette is const).

use aberredengine::prelude::Color;

pub const NODE_W: f32 = 240.0;
pub const HEADER_H: f32 = 24.0;
pub const TEXT_AREA_H: f32 = 64.0;
pub const ROW_H: f32 = 22.0;
pub const PAD: f32 = 8.0;
pub const PIN_R: f32 = 6.0;
pub const FONT_SIZE: f32 = 12.0;
pub const TEXT_MAX_LINES: usize = 4;
pub const COL_GAP: f32 = 120.0;
pub const ROW_GAP: f32 = 24.0;
pub const EDGE_THICKNESS: f32 = 2.0;
pub const OUTLINE_THICKNESS: f32 = 2.0;
pub const CORNER_RADIUS: f32 = 6.0;
/// `to.x >= from.x + EDGE_FORWARD_DX` selects the forward case of `edge_curve`.
pub const EDGE_FORWARD_DX: f32 = 20.0;
/// Minimum horizontal control-point offset of a forward edge.
pub const EDGE_MIN_CTRL_DX: f32 = 60.0;
/// Horizontal offset (and base vertical drop) of a back/self edge's loop control points.
pub const EDGE_LOOP_DX: f32 = 80.0;
pub const FONT: &str = "default";

// Palette (RGBA).
pub const NODE_BODY_COLOR: Color = Color::new(45, 48, 58, 255);
pub const HEADER_COLOR: Color = Color::new(70, 78, 102, 255);
pub const START_HEADER_COLOR: Color = Color::new(46, 125, 50, 255);
pub const OUTLINE_COLOR: Color = Color::new(110, 118, 140, 255);
pub const SELECTED_OUTLINE_COLOR: Color = Color::new(255, 217, 0, 255);
pub const ERROR_OUTLINE_COLOR: Color = Color::new(220, 60, 60, 255);
pub const TEXT_COLOR: Color = Color::new(230, 230, 230, 255);
pub const DIM_TEXT_COLOR: Color = Color::new(150, 155, 165, 255);
pub const EDGE_COLOR: Color = Color::new(160, 170, 200, 255);
pub const EDGE_SELECTED_COLOR: Color = Color::new(255, 217, 0, 255);
pub const PIN_OUT_COLOR: Color = Color::new(120, 200, 255, 255);
pub const PIN_IN_COLOR: Color = Color::new(255, 170, 80, 255);
pub const PIN_DANGLING_COLOR: Color = Color::new(220, 60, 60, 255);

// Z indices (the `ZIndex` payload; higher draws on top).
pub const Z_EDGE: f32 = 0.0;
pub const Z_ARROWHEAD: f32 = 1.0;
pub const Z_NODE_BODY: f32 = 10.0;
pub const Z_HEADER: f32 = 11.0;
pub const Z_TEXT: f32 = 12.0;
pub const Z_PIN: f32 = 13.0;
pub const Z_PREVIEW: f32 = 20.0;

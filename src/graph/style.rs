//! Numeric style constants of the graph view (node size, pins, gaps, edge/outline metrics).
//! Colours and Z indices are added in a later step.

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

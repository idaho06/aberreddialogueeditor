//! Style constants of the graph view: numeric sizes/gaps plus the palette and Z indices.
//! The only engine import is `Color` (its `new` is a `const fn`, so the palette is const).

use aberredengine::prelude::Color;

/// Node width in world units.
pub const NODE_W: f32 = 240.0;
/// Height of the node's header band.
pub const HEADER_H: f32 = 24.0;
/// Fixed height of the node's text area.
pub const TEXT_AREA_H: f32 = 64.0;
/// Height of one output row below the text area.
pub const ROW_H: f32 = 22.0;
/// Inner padding between a node's edge and its texts.
pub const PAD: f32 = 8.0;
/// Radius of a pin circle.
pub const PIN_R: f32 = 6.0;
/// Font size of all node texts.
pub const FONT_SIZE: f32 = 12.0;
/// Maximum number of wrapped lines shown in a node's text area.
pub const TEXT_MAX_LINES: usize = 4;
/// Horizontal gap between two layout columns.
pub const COL_GAP: f32 = 120.0;
/// Vertical gap between two nodes in the same layout column.
pub const ROW_GAP: f32 = 24.0;
/// Edge line thickness.
pub const EDGE_THICKNESS: f32 = 2.0;
/// Outline thickness of a selected or erroneous node.
pub const OUTLINE_THICKNESS: f32 = 2.0;
/// Corner radius of a node body.
pub const CORNER_RADIUS: f32 = 6.0;
/// `to.x >= from.x + EDGE_FORWARD_DX` selects the forward case of `edge_curve`.
pub const EDGE_FORWARD_DX: f32 = 20.0;
/// Minimum horizontal control-point offset of a forward edge.
pub const EDGE_MIN_CTRL_DX: f32 = 60.0;
/// Horizontal offset (and base vertical drop) of a back/self edge's loop control points.
pub const EDGE_LOOP_DX: f32 = 80.0;
/// Font used for all node texts.
pub const FONT: &str = "default";

// Palette (RGBA).
/// Fill of a node's body.
pub const NODE_BODY_COLOR: Color = Color::new(45, 48, 58, 255);
/// Fill of a node's header band.
pub const HEADER_COLOR: Color = Color::new(70, 78, 102, 255);
/// Fill of the start node's header band.
pub const START_HEADER_COLOR: Color = Color::new(46, 125, 50, 255);
/// Outline of an unselected, error-free node.
pub const OUTLINE_COLOR: Color = Color::new(110, 118, 140, 255);
/// Outline of the selected node.
pub const SELECTED_OUTLINE_COLOR: Color = Color::new(255, 217, 0, 255);
/// Outline of a node with a lint error.
pub const ERROR_OUTLINE_COLOR: Color = Color::new(220, 60, 60, 255);
/// Main text color.
pub const TEXT_COLOR: Color = Color::new(230, 230, 230, 255);
/// Dimmed text color (the `next` row of a `Next` node).
pub const DIM_TEXT_COLOR: Color = Color::new(150, 155, 165, 255);
/// Edge line color.
pub const EDGE_COLOR: Color = Color::new(160, 170, 200, 255);
/// Edge line color when its source node is selected.
pub const EDGE_SELECTED_COLOR: Color = Color::new(255, 217, 0, 255);
/// Output pin color.
pub const PIN_OUT_COLOR: Color = Color::new(120, 200, 255, 255);
/// Input pin color.
pub const PIN_IN_COLOR: Color = Color::new(255, 170, 80, 255);
/// Output pin color of a dangling link.
pub const PIN_DANGLING_COLOR: Color = Color::new(220, 60, 60, 255);

// Z indices (the `ZIndex` payload; higher draws on top).
/// Edge bezier lines.
pub const Z_EDGE: f32 = 0.0;
/// Edge arrowheads.
pub const Z_ARROWHEAD: f32 = 1.0;
/// Node bodies.
pub const Z_NODE_BODY: f32 = 10.0;
/// Node header bands.
pub const Z_HEADER: f32 = 11.0;
/// Node texts.
pub const Z_TEXT: f32 = 12.0;
/// Node pins (above bodies and headers).
pub const Z_PIN: f32 = 13.0;
/// Drag-preview link.
pub const Z_PREVIEW: f32 = 20.0;

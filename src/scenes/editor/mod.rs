mod actions;
mod context_menu;
mod gui;
mod inspector;
mod lifecycle;
mod menu;
mod modals;
mod nodes;
mod problems;
mod shortcuts;
mod status;
mod tabs;

pub use gui::gui;
pub use lifecycle::enter;

use aberredengine::prelude::Vec2;
use aberredengine::prelude::imgui::Ui;

/// The window size in screen pixels as a `Vec2` (== the render size), read by the producers of
/// the viewport-carrying actions (`FrameAll`, `Relayout`, `AddNode`).
pub(super) fn display_viewport(ui: &Ui) -> Vec2 {
    let size = ui.io().display_size;
    Vec2::new(size[0], size[1])
}

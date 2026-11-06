use std::sync::atomic::Ordering;

use aberredengine::prelude::*;

use super::menu;
use crate::state::UiCaptureHandle;

/// Scene GUI callback (render thread). Publishes imgui's capture flags into the shared `UiCapture` every
/// frame so logic-side input systems can ignore mouse/keyboard while the GUI wants them, then draws the UI.
pub fn gui(ctx: &mut GuiCtx) {
    if let Some(capture) = ctx.app_state.get::<UiCaptureHandle>() {
        capture
            .mouse
            .store(ctx.ui.io().want_capture_mouse, Ordering::Relaxed);
        capture
            .keyboard
            .store(ctx.ui.io().want_capture_keyboard, Ordering::Relaxed);
    }
    menu::draw(ctx);
}

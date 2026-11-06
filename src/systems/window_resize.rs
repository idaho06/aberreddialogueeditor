use aberredengine::core::events::window::WindowResized;
use aberredengine::prelude::*;

/// Keeps the render target and the camera offset in sync with the window size, so mouse coordinates are
/// 1:1 with render-target pixels. Sizes <= 0 are ignored.
pub fn on_window_resized(
    trigger: On<WindowResized>,
    mut config: ResMut<GameConfig>,
    mut camera: ResMut<MainCamera>,
) {
    let event = trigger.event();
    let (w, h) = (event.w, event.h);
    if w <= 0 || h <= 0 {
        return;
    }
    config.set_render_size(w as u32, h as u32);
    camera.0.offset = Vec2::new(w as f32 / 2.0, h as f32 / 2.0);
}

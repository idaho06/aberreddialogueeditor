//! ECS camera system for the graph view: `graph_frame_system` (registered with `add_system`, runs in
//! Update) makes `MainCamera` follow the ACTIVE doc's [`CamState`] every tick and applies wheel zoom
//! and middle-mouse panning to it. The camera write lives in the plain function [`camera_update`]; the
//! input math lives in the pure, unit-tested helpers [`apply_wheel`] and [`pan_step`]. Later steps
//! extend this system with the rest of the graph input so that camera update and input run in a fixed
//! order.
//!
//! Lock discipline: the workspace is locked only for the short per-tick read/mutate of the active
//! doc's camera. Mouse input is ignored while the GUI captures it (`UiCapture::mouse`), except a pan
//! gesture already in progress, which continues.

use std::sync::atomic::Ordering;

use aberredengine::prelude::*;

use super::{pan_by_screen_delta, zoom_at};
use crate::model::CamState;
use crate::state::{UiCaptureHandle, WorkspaceMutex};

/// Zoom factor per unit of wheel scroll: one tick of `scroll_y` zooms by `1.1^scroll_y`.
const WHEEL_ZOOM_BASE: f32 = 1.1;

/// One wheel tick: zooms by `1.1^scroll_y` around the screen cursor, so the world point under the
/// cursor stays under it (clamped to `[MIN_ZOOM, MAX_ZOOM]`). Pure.
pub fn apply_wheel(cam: &CamState, viewport: Vec2, cursor: Vec2, scroll_y: f32) -> CamState {
    zoom_at(cam, viewport, cursor, WHEEL_ZOOM_BASE.powf(scroll_y))
}

/// One pan step. `prev` is the mouse position stored on the previous tick (`None` when no pan is in
/// progress), `now` the current screen-pixel position, `held` whether the pan button is held. On the
/// press tick (`prev == None`) only the position is stored — no movement. While held, the screen-pixel
/// delta since `prev` pans via [`pan_by_screen_delta`]. On release (`held == false`) the gesture resets
/// to `None`. Pure.
pub fn pan_step(
    prev: Option<Vec2>,
    now: Vec2,
    held: bool,
    cam: &CamState,
) -> (Option<Vec2>, CamState) {
    if !held {
        return (None, *cam);
    }
    let Some(prev) = prev else {
        return (Some(now), *cam);
    };
    (Some(now), pan_by_screen_delta(cam, now - prev))
}

/// Writes the doc camera into the `MainCamera` resource: the world point `cam.target` is shown at the
/// screen centre (`offset = viewport / 2`) with `cam.zoom`.
pub fn camera_update(camera: &mut MainCamera, viewport: Vec2, cam: CamState) {
    camera.0 = Camera2D::new(cam.target, viewport / 2.0).with_zoom(cam.zoom);
}

/// Makes `MainCamera` follow the active doc's camera and handles wheel zoom and middle-mouse pan.
/// With no active doc the camera is left alone.
pub fn graph_frame_system(
    app_state: Res<AppState>,
    config: Res<GameConfig>,
    input: Res<InputState>,
    mut camera: ResMut<MainCamera>,
    mut pan_prev: Local<Option<Vec2>>,
) {
    let viewport = Vec2::new(config.render_width as f32, config.render_height as f32);
    let mouse_captured = app_state
        .get::<UiCaptureHandle>()
        .is_some_and(|capture| capture.mouse.load(Ordering::Relaxed));
    let Some(handle) = app_state.get::<WorkspaceMutex>().cloned() else {
        return;
    };
    let mut ws = handle.lock().unwrap();
    let Some(doc) = ws.active_doc_mut() else {
        // No active doc: leave the camera alone (and drop any stale pan gesture).
        *pan_prev = None;
        return;
    };
    let orig = doc.camera;
    let mut cam = orig;
    // Wheel zoom around the cursor; ignored while the GUI captures the mouse.
    if !mouse_captured && input.scroll_y != 0.0 {
        cam = apply_wheel(
            &cam,
            viewport,
            Vec2::new(input.mouse_x, input.mouse_y),
            input.scroll_y,
        );
    }
    // Middle-mouse pan by screen-pixel delta (never `mouse_world_*`, which lags the camera by one
    // tick). A pan already in progress continues even when the GUI captures the mouse.
    let held = input.action(InputAction::Action3).active;
    let track = held && (!mouse_captured || pan_prev.is_some());
    let (prev, panned) = pan_step(
        *pan_prev,
        Vec2::new(input.mouse_x, input.mouse_y),
        track,
        &cam,
    );
    *pan_prev = prev;
    cam = panned;
    if cam != orig {
        doc.set_camera(cam);
    }
    camera_update(&mut camera, viewport, cam);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{MAX_ZOOM, MIN_ZOOM, screen_to_world};

    fn cam() -> CamState {
        CamState {
            target: Vec2::new(10.0, 20.0),
            zoom: 2.0,
        }
    }

    #[test]
    fn apply_wheel_zooms_by_1_point_1_pow_scroll_and_keeps_the_cursor_point_fixed() {
        let cam = cam();
        let viewport = Vec2::new(800.0, 600.0);
        let cursor = Vec2::new(123.0, 456.0);
        let scrolled = apply_wheel(&cam, viewport, cursor, 2.0);
        assert!((scrolled.zoom - cam.zoom * 1.1f32.powf(2.0)).abs() < 1e-5);
        let before = screen_to_world(&cam, viewport, cursor);
        let after = screen_to_world(&scrolled, viewport, cursor);
        assert!(
            before.abs_diff_eq(after, 1e-4),
            "the world point under the cursor must stay under it: {before:?} vs {after:?}"
        );
        // No scroll: unchanged.
        assert_eq!(apply_wheel(&cam, viewport, cursor, 0.0), cam);
        // Clamps to [MIN_ZOOM, MAX_ZOOM].
        assert!((apply_wheel(&cam, viewport, cursor, 100.0).zoom - MAX_ZOOM).abs() < 1e-6);
        assert!((apply_wheel(&cam, viewport, cursor, -100.0).zoom - MIN_ZOOM).abs() < 1e-6);
    }

    #[test]
    fn pan_step_press_drag_release() {
        let cam = cam();
        let p0 = Vec2::new(100.0, 100.0);
        let p1 = Vec2::new(130.0, 90.0);
        // Press tick: only the position is stored, no movement.
        let (prev, c) = pan_step(None, p0, true, &cam);
        assert_eq!(prev, Some(p0));
        assert_eq!(c, cam, "no movement on the press tick");
        // Drag: the screen delta (divided by zoom inside pan_by_screen_delta) pans the camera.
        let (prev, c) = pan_step(prev, p1, true, &cam);
        assert_eq!(prev, Some(p1));
        assert_eq!(c, pan_by_screen_delta(&cam, p1 - p0));
        // Release: the gesture resets, no movement (the passed-in camera is returned unchanged).
        let (prev, c) = pan_step(prev, p1, false, &cam);
        assert_eq!(prev, None);
        assert_eq!(c, cam);
    }
}

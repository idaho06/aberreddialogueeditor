//! ECS camera+input system for the graph view: `graph_frame_system` (registered with `add_system`,
//! runs in Update) makes `MainCamera` follow the ACTIVE doc's [`CamState`] every tick, applies
//! wheel zoom and middle-mouse panning to it, then feeds the mouse through the pure interaction
//! state machine ([`step`]) and applies the returned [`Action`]s to the doc ([`apply_actions`]).
//! The camera write lives in the plain function [`camera_update`]; the input math lives in the
//! pure, unit-tested helpers [`apply_wheel`], [`pan_step`] and [`build_sample`].
//!
//! Lock discipline: the workspace is locked ONCE per tick, briefly, for the whole frame step —
//! camera zoom/pan, sample building, `step`/`apply_actions` and the pending-request store all
//! happen under that one lock. Mouse input is ignored while the GUI captures it
//! (`UiCapture::mouse`), except a pan gesture already in progress, which continues.

use std::sync::atomic::Ordering;

use aberredengine::prelude::*;

use super::{
    ButtonSample, GraphInput, GraphInteraction, apply_actions, build_sample, drag_preview,
    pan_by_screen_delta, step, zoom_at,
};
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

/// Makes `MainCamera` follow the active doc's camera, handles wheel zoom and middle-mouse pan,
/// then runs one tick of the graph interaction state machine and applies its actions to the doc.
/// With no active doc the camera is left alone and the interaction is reset.
pub fn graph_frame_system(
    app_state: Res<AppState>,
    config: Res<GameConfig>,
    input: Res<InputState>,
    mut camera: ResMut<MainCamera>,
    mut pan_prev: Local<Option<Vec2>>,
    mut graph_input: ResMut<GraphInput>,
    mut prev_mouse: Local<Vec2>,
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
        // No active doc: no preview, no gesture, and the camera is left alone.
        graph_input.preview = None;
        graph_input.interaction = GraphInteraction::default();
        graph_input.last_doc_id = 0;
        *pan_prev = None;
        return;
    };
    // (a) Wheel zoom around the cursor and middle-mouse pan by screen-pixel delta (never
    // `mouse_world_*`, which lags the camera by one tick). Both are ignored while the GUI
    // captures the mouse, except a pan already in progress.
    let orig = doc.camera;
    let mut cam = orig;
    if !mouse_captured && input.scroll_y != 0.0 {
        cam = apply_wheel(
            &cam,
            viewport,
            Vec2::new(input.mouse_x, input.mouse_y),
            input.scroll_y,
        );
    }
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
    // (b) One input sample, with the mouse world position computed from the doc camera as it is
    // AFTER (a) — `input.mouse_world_*` would use the camera from before this tick's systems.
    let mouse_screen = Vec2::new(input.mouse_x, input.mouse_y);
    let left = input.mouse_left_button;
    let right = input.action(InputAction::Action2);
    let sample = build_sample(
        mouse_screen,
        ButtonSample {
            active: left.active,
            just_pressed: left.just_pressed,
            just_released: left.just_released,
        },
        ButtonSample {
            active: right.active,
            just_pressed: right.just_pressed,
            just_released: right.just_released,
        },
        mouse_captured,
        &doc.camera,
        viewport,
    );
    // A gesture never survives a tab switch.
    if graph_input.last_doc_id != doc.doc_id {
        graph_input.interaction = GraphInteraction::default();
        graph_input.last_doc_id = doc.doc_id;
    }
    // (c) Run the state machine and apply its actions, still under the same lock; a returned
    // context-menu request goes to the workspace for the GUI to consume.
    let actions = step(&mut graph_input.interaction, doc, &sample, *prev_mouse);
    let request = apply_actions(doc, &actions);
    graph_input.preview = drag_preview(&graph_input.interaction, &sample);
    *prev_mouse = mouse_screen;
    let final_cam = doc.camera;
    if let Some(request) = request {
        ws.pending_context_menu = Some(request);
    }
    // (d) LAST: write MainCamera from the FINAL doc camera, so the drawn frame and the world
    // coordinates used by the input agree.
    camera_update(&mut camera, viewport, final_cam);
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

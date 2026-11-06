//! ECS input side of the graph view: the [`GraphInput`] resource — the interaction state machine's
//! memory between ticks plus the current drag-preview endpoints — and the pure [`build_sample`]
//! helper that turns raw `InputState` values into one [`InputSample`]. The frame system itself
//! (which owns the one workspace lock per tick) lives in `camera_system.rs`.

// The `Resource` derive macro expands to `bevy_ecs::...` paths; this crate does not depend on
// `bevy_ecs` directly, so the facade's re-export brings the name into scope.
use aberredengine::bevy_ecs;
use aberredengine::prelude::{Resource, Vec2};

use super::camera::screen_to_world;
use super::interaction::{GraphInteraction, InputSample};
use crate::model::CamState;

/// Logic-side graph input state, inserted at editor scene enter: the pure interaction state
/// machine's memory, the drag-preview endpoints `(from_pin_world, mouse_world)` read by the
/// preview system, and the doc the interaction belongs to (a gesture never survives a tab
/// switch, so the frame system resets the interaction when `last_doc_id` changes).
#[derive(Resource, Debug, Default)]
pub struct GraphInput {
    pub interaction: GraphInteraction,
    pub preview: Option<(Vec2, Vec2)>,
    pub last_doc_id: u64,
}

/// One button's raw state for a tick, mirroring the engine's `BoolState` fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonSample {
    pub active: bool,
    pub just_pressed: bool,
    pub just_released: bool,
}

/// Builds one per-tick [`InputSample`] from raw input values. `cam` is the doc camera AFTER this
/// tick's wheel/pan, so `mouse_world` agrees with the frame drawn this tick — never
/// `InputState::mouse_world_*`, which lags the camera. Pure.
pub fn build_sample(
    mouse_screen: Vec2,
    left: ButtonSample,
    right: ButtonSample,
    mouse_captured_by_ui: bool,
    cam: &CamState,
    viewport: Vec2,
) -> InputSample {
    InputSample {
        mouse_screen,
        mouse_world: screen_to_world(cam, viewport, mouse_screen),
        left_down: left.active,
        left_pressed: left.just_pressed,
        left_released: left.just_released,
        right_down: right.active,
        right_pressed: right.just_pressed,
        right_released: right.just_released,
        mouse_captured_by_ui,
        zoom: cam.zoom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::camera::screen_to_world;

    #[test]
    fn build_sample_maps_the_buttons_and_computes_world_with_the_given_camera() {
        let cam = CamState {
            target: Vec2::new(10.0, 20.0),
            zoom: 2.0,
        };
        let viewport = Vec2::new(800.0, 600.0);
        let screen = Vec2::new(123.0, 456.0);
        let sample = build_sample(
            screen,
            ButtonSample {
                active: true,
                just_pressed: true,
                just_released: false,
            },
            ButtonSample {
                active: false,
                just_pressed: false,
                just_released: true,
            },
            true,
            &cam,
            viewport,
        );
        assert_eq!(sample.mouse_screen, screen);
        assert_eq!(
            sample.mouse_world,
            screen_to_world(&cam, viewport, screen),
            "mouse_world uses the camera as passed in (after this tick's wheel/pan)"
        );
        assert!(sample.left_down && sample.left_pressed && !sample.left_released);
        assert!(!sample.right_down && !sample.right_pressed && sample.right_released);
        assert!(sample.mouse_captured_by_ui);
        assert!(
            (sample.zoom - 2.0).abs() < 1e-6,
            "zoom comes from the camera"
        );
    }

    #[test]
    fn build_sample_reflects_a_zoomed_camera() {
        let cam = CamState {
            target: Vec2::ZERO,
            zoom: 0.5,
        };
        let viewport = Vec2::new(800.0, 600.0);
        let screen = Vec2::new(400.0, 300.0); // the screen centre shows the target
        let sample = build_sample(
            screen,
            ButtonSample {
                active: false,
                just_pressed: false,
                just_released: false,
            },
            ButtonSample {
                active: false,
                just_pressed: false,
                just_released: false,
            },
            false,
            &cam,
            viewport,
        );
        assert!(
            sample.mouse_world.abs_diff_eq(Vec2::ZERO, 1e-5),
            "the screen centre maps to the camera target"
        );
        assert!((sample.zoom - 0.5).abs() < 1e-6);
    }
}

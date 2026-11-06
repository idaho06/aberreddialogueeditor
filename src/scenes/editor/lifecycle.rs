use aberredengine::core::resources::input_bindings::{InputBinding, InputBindings, MouseButton};
use aberredengine::prelude::*;

use crate::graph::GraphEntities;

/// Scene-enter observer for the `editor` scene: centres the camera on the render target, disables camera
/// follow, rebinds `Action2` to the right mouse button only (its default is Enter OR right mouse), and
/// inserts the graph projection's entity bookkeeping resource.
pub fn enter(
    _: On<SceneEntered>,
    mut commands: Commands,
    config: Res<GameConfig>,
    mut follow: ResMut<CameraFollowConfig>,
    mut bindings: ResMut<InputBindings>,
) {
    let half = Vec2::new(
        config.render_width as f32 / 2.0,
        config.render_height as f32 / 2.0,
    );
    commands.insert_resource(MainCamera(Camera2D::new(Vec2::ZERO, half)));
    commands.insert_resource(GraphEntities::default());
    follow.enabled = false;
    bindings.rebind(
        InputAction::Action2,
        InputBinding::MouseButton(MouseButton::MOUSE_BUTTON_RIGHT),
    );
}

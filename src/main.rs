//! AberredDialogueEditor: a node-graph editor for AberredEngine dialogue JSON.
//!
//! `main` wires the engine: the shared `AppState` handles are inserted once in `on_setup`, the
//! `intro` splash plays first and hands over to the `editor` scene. GUI callbacks run on the
//! render thread while systems and observers run on the logic thread; the two share state only
//! through those `AppState` handles.

mod graph;
mod model;
mod ops;
mod scenes;
mod state;
mod systems;
#[cfg(test)]
mod testkit;

use aberredengine::prelude::*;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    if let Err(err) = EngineBuilder::new()
        .config_str(include_str!("../config.ini"))
        .on_setup(systems::setup::load_state)
        .add_system(systems::file_dialogs::poll_async_dialogs)
        .add_system(graph::graph_frame_system)
        .add_late_system(graph::graph_projection_system)
        .add_late_system(graph::graph_preview_system)
        .add_observer(systems::window_resize::on_window_resized)
        .add_scene("intro")
        .on_scene_enter("intro", scenes::intro::intro_enter)
        .add_scene_system("intro", scenes::intro::intro_update)
        .add_scene_system("intro", scenes::intro::intro_shader_system)
        .on_scene_exit("intro", scenes::intro::intro_exit)
        .add_scene("editor")
        .on_scene_enter("editor", scenes::editor::enter)
        .add_scene_gui("editor", scenes::editor::gui)
        .initial_scene("intro")
        .try_run()
    {
        log::error!("engine exited with an error: {err}");
        std::process::exit(1);
    }
}

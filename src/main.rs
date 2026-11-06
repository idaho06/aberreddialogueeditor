mod graph;
mod model;
mod ops;
mod scenes;
mod state;
mod systems;

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
        .add_scene("editor")
        .on_scene_enter("editor", scenes::editor::enter)
        .add_scene_gui("editor", scenes::editor::gui)
        .initial_scene("editor")
        .try_run()
    {
        log::error!("engine exited with an error: {err}");
        std::process::exit(1);
    }
}

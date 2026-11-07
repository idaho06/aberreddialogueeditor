//! The engine's `on_setup` hook, run once at startup: inserts the shared `AppState` handles,
//! seeds the graph resources the globally registered graph systems read, and queues the intro
//! scene's shaders and logo texture.

use aberredengine::prelude::*;

use crate::graph::{GraphEntities, GraphInput};
use crate::state::{UiCaptureHandle, WorkspaceMutex};
use crate::systems::file_dialogs::AsyncFileDialogMutex;

/// Embedded intro assets, compiled in so the splash needs no on-disk loads.
const SHADER_GLITCH_SRC: &str = include_str!("../../assets/shaders/glitch.fs");
const SHADER_FADE_SRC: &str = include_str!("../../assets/shaders/fade.fs");
const TEXTURE_LOGO: &[u8] =
    include_bytes!("../../assets/textures/aberred_engine_isometric_alpha.png");

/// `on_setup` hook: inserts the shared handles into `AppState` exactly once and seeds the graph
/// resources the globally registered graph systems read, so they cannot panic while the intro
/// scene is active. The render thread's cloned `AppState` aliases the same `Arc`s, so later
/// mutations are visible on both threads. Also queues the intro's shader and texture loads; the
/// engine leaves its `Setup` state only after they have replied, so the intro starts with the
/// assets ready.
pub fn load_state(
    mut commands: Commands,
    mut app_state: ResMut<AppState>,
    mut asset_cmds: MessageWriter<RenderAssetCmd>,
) {
    app_state.insert(UiCaptureHandle::default());
    app_state.insert(WorkspaceMutex::default());
    app_state.insert(AsyncFileDialogMutex::default());
    commands.insert_resource(GraphEntities::default());
    commands.insert_resource(GraphInput::default());
    asset_cmds.write(RenderAssetCmd::ShaderFromMemory {
        key: "glitch".to_string(),
        vs_src: None,
        fs_src: Some(SHADER_GLITCH_SRC.to_string()),
    });
    asset_cmds.write(RenderAssetCmd::ShaderFromMemory {
        key: "fade".to_string(),
        vs_src: None,
        fs_src: Some(SHADER_FADE_SRC.to_string()),
    });
    asset_cmds.write(RenderAssetCmd::TextureFromMemory {
        key: "aberred_engine_isometric_alpha".to_string(),
        ext: ".png".to_string(),
        bytes: TEXTURE_LOGO.to_vec(),
        filter: TextureFilter::Nearest,
    });
    log::info!(
        "load_state: inserted UiCaptureHandle, WorkspaceMutex, AsyncFileDialogMutex, GraphEntities and GraphInput; queued glitch/fade shaders and the logo texture"
    );
}

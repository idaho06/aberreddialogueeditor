use aberredengine::prelude::*;

use crate::state::{UiCaptureHandle, WorkspaceMutex};
use crate::systems::file_dialogs::AsyncFileDialogMutex;

/// `on_setup` hook: inserts the shared handles into `AppState` exactly once. The render thread's cloned
/// `AppState` aliases the same `Arc`s, so later mutations are visible on both threads.
pub fn load_state(mut app_state: ResMut<AppState>) {
    app_state.insert(UiCaptureHandle::default());
    app_state.insert(WorkspaceMutex::default());
    app_state.insert(AsyncFileDialogMutex::default());
}

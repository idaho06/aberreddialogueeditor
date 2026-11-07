use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use aberredengine::prelude::AppState;

use crate::model::Workspace;

/// Input-capture flags published by the scene GUI every frame and read by logic-side input systems.
///
/// The engine does not mask input for scene GUIs, so the GUI stores imgui's `want_capture_mouse` /
/// `want_capture_keyboard` here; graph input systems ignore the mouse/keyboard while they are set.
#[derive(Default)]
pub struct UiCapture {
    pub mouse: AtomicBool,
    pub keyboard: AtomicBool,
}

/// Shared handle stored in `AppState` (insert once in `on_setup`); the render-thread clone aliases it.
pub type UiCaptureHandle = Arc<UiCapture>;

/// Shared handle to the single source of truth, stored in `AppState` (insert once in `on_setup`).
pub type WorkspaceMutex = Arc<Mutex<Workspace>>;

/// Runs `f` with the workspace locked and returns its result, or `None` when the
/// `WorkspaceMutex` handle is missing from `app_state`. The guard lives only for the closure,
/// so a caller structurally cannot hold the lock across imgui/rfd calls — the short-lock rule
/// becomes the shape of the code. Use this for snapshots and short re-lock-and-mutate blocks;
/// sites that need the `Arc` handle itself (or hold the guard across several statements) keep
/// using `app_state.get::<WorkspaceMutex>()` directly.
pub fn with_workspace<R>(app_state: &AppState, f: impl FnOnce(&mut Workspace) -> R) -> Option<R> {
    let handle = app_state.get::<WorkspaceMutex>()?;
    let mut ws = handle.lock().unwrap();
    Some(f(&mut ws))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_workspace_returns_none_without_a_handle() {
        let app_state = AppState::default();
        assert!(
            with_workspace(&app_state, |ws| ws.docs.len()).is_none(),
            "no WorkspaceMutex in AppState: None, the closure never runs"
        );
    }

    #[test]
    fn with_workspace_runs_the_closure_and_the_mutation_is_visible_afterwards() {
        let mut app_state = AppState::default();
        app_state.insert(WorkspaceMutex::default());
        let seen = with_workspace(&app_state, |ws| {
            ws.active = 3;
            ws.docs.len()
        });
        assert_eq!(seen, Some(0), "the closure ran under the lock");
        let after = with_workspace(&app_state, |ws| (ws.active, ws.docs.len()));
        assert_eq!(after, Some((3, 0)), "the mutation outlives the guard");
    }
}

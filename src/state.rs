use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

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

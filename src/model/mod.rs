//! Editor data model. Placeholder for now — the real dialogue document model arrives in the next step.

/// All open documents plus the active one. Shared between the render-thread GUI and logic-thread systems
/// through [`crate::state::WorkspaceMutex`]. Placeholder fields are not read until the real model lands.
#[derive(Default)]
#[allow(dead_code)]
pub struct Workspace {
    pub docs: Vec<()>,
    pub active: usize,
}

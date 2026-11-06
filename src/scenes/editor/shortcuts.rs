//! Keyboard shortcuts read from imgui's key state. Only active when imgui does not want text
//! input (no input field focused), so typing Ctrl+S into a text box never triggers a save.

use aberredengine::prelude::imgui::Key;
use aberredengine::prelude::*;

use super::actions::EditorAction;

/// Polls the Ctrl shortcuts and pushes the matching [`EditorAction`]s. `active` is the
/// workspace's active tab index when a doc is open (for Ctrl+W).
pub fn poll(ctx: &GuiCtx, active: Option<usize>, actions: &mut Vec<EditorAction>) {
    let ui = ctx.ui;
    let io = ui.io();
    if io.want_text_input {
        return;
    }
    let ctrl = io.key_ctrl;
    let shift = io.key_shift;
    if ctrl && ui.is_key_pressed(Key::N) {
        actions.push(EditorAction::New);
    }
    if ctrl && ui.is_key_pressed(Key::O) {
        actions.push(EditorAction::Open);
    }
    if ctrl && ui.is_key_pressed(Key::S) {
        if shift {
            actions.push(EditorAction::SaveAs);
        } else {
            actions.push(EditorAction::Save);
        }
    }
    if ctrl
        && ui.is_key_pressed(Key::W)
        && let Some(index) = active
    {
        actions.push(EditorAction::CloseTab(index));
    }
    if ctrl && ui.is_key_pressed(Key::Q) {
        actions.push(EditorAction::Quit);
    }
}

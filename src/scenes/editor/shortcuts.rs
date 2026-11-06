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
    if io.want_text_input || !io.key_ctrl {
        return;
    }
    for (key, action) in [
        (Key::N, EditorAction::New),
        (Key::O, EditorAction::Open),
        (Key::Q, EditorAction::Quit),
    ] {
        if ui.is_key_pressed(key) {
            actions.push(action);
        }
    }
    if ui.is_key_pressed(Key::S) {
        actions.push(if io.key_shift {
            EditorAction::SaveAs
        } else {
            EditorAction::Save
        });
    }
    if ui.is_key_pressed(Key::W)
        && let Some(index) = active
    {
        actions.push(EditorAction::CloseTab(index));
    }
}

//! Keyboard shortcuts read from imgui's key state. Only active when imgui does not want text
//! input (no input field focused), so typing Ctrl+S into a text box never triggers a save.

use aberredengine::prelude::imgui::Key;
use aberredengine::prelude::*;

use super::actions::EditorAction;

/// Polls the shortcuts and pushes the matching [`EditorAction`]s. `active` is the workspace's
/// active tab index when a doc is open (for Ctrl+W). Everything is gated on `!want_text_input`,
/// so typing into a text box never triggers a shortcut.
pub fn poll(ctx: &GuiCtx, active: Option<usize>, actions: &mut Vec<EditorAction>) {
    let ui = ctx.ui;
    let io = ui.io();
    if io.want_text_input {
        return;
    }
    // F = frame all (not a Ctrl shortcut); the viewport is imgui's display size (== render size).
    if ui.is_key_pressed(Key::F) {
        let size = io.display_size;
        actions.push(EditorAction::FrameAll {
            viewport: Vec2::new(size[0], size[1]),
        });
    }
    if !io.key_ctrl {
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

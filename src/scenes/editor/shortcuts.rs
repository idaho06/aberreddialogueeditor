//! Keyboard shortcuts read from imgui's key state. Only active when imgui does not want text
//! input (no input field focused), so typing Ctrl+S into a text box never triggers a save.

use aberredengine::prelude::imgui::Key;
use aberredengine::prelude::*;

use super::actions::EditorAction;

/// The plain (non-Ctrl) shortcuts, decided purely from the key states: Delete fires with no
/// modifier required; F = frame all must NOT fire while Ctrl is held (Ctrl+F is not frame-all).
/// `viewport` is imgui's display size (== render size), needed by the frame-all action.
fn plain_shortcuts(
    delete_pressed: bool,
    f_pressed: bool,
    ctrl_held: bool,
    viewport: Vec2,
) -> Vec<EditorAction> {
    let mut out = Vec::new();
    if delete_pressed {
        out.push(EditorAction::DeleteSelected);
    }
    if f_pressed && !ctrl_held {
        out.push(EditorAction::FrameAll { viewport });
    }
    out
}

/// Polls the shortcuts and pushes the matching [`EditorAction`]s. `active` is the workspace's
/// active tab index when a doc is open (for Ctrl+W). Everything is gated on `!want_text_input`,
/// so typing into a text box never triggers a shortcut.
pub fn poll(ctx: &GuiCtx, active: Option<usize>, actions: &mut Vec<EditorAction>) {
    let ui = ctx.ui;
    let io = ui.io();
    if io.want_text_input {
        return;
    }
    let size = io.display_size;
    let viewport = Vec2::new(size[0], size[1]);
    // Plain keys (no Ctrl): Delete deletes the selection; F frames all unless Ctrl is held.
    actions.extend(plain_shortcuts(
        ui.is_key_pressed(Key::Delete),
        ui.is_key_pressed(Key::F),
        io.key_ctrl,
        viewport,
    ));
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

#[cfg(test)]
mod tests {
    use super::*;
    use aberredengine::prelude::Vec2;

    #[test]
    fn delete_fires_without_modifiers_and_f_only_when_ctrl_is_not_held() {
        let viewport = Vec2::new(800.0, 600.0);
        // Delete alone fires.
        assert_eq!(
            plain_shortcuts(true, false, false, viewport),
            vec![EditorAction::DeleteSelected]
        );
        // Delete also fires with Ctrl held (no Ctrl/Shift required).
        assert_eq!(
            plain_shortcuts(true, false, true, viewport),
            vec![EditorAction::DeleteSelected]
        );
        // F alone frames all.
        assert_eq!(
            plain_shortcuts(false, true, false, viewport),
            vec![EditorAction::FrameAll { viewport }]
        );
        // Ctrl+F must NOT frame.
        assert!(plain_shortcuts(false, true, true, viewport).is_empty());
        // Both keys without Ctrl: both actions, Delete first.
        assert_eq!(
            plain_shortcuts(true, true, false, viewport),
            vec![
                EditorAction::DeleteSelected,
                EditorAction::FrameAll { viewport }
            ]
        );
        // Neither key: nothing.
        assert!(plain_shortcuts(false, false, false, viewport).is_empty());
        assert!(plain_shortcuts(false, false, true, viewport).is_empty());
    }
}

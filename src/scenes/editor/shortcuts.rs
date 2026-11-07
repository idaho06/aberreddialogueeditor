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

/// The pressed state of the Ctrl-modified shortcut keys, snapshotted from imgui in one place.
#[derive(Debug, Clone, Copy, Default)]
struct CtrlKeys {
    n: bool,
    o: bool,
    q: bool,
    s: bool,
    w: bool,
    p: bool,
    shift: bool,
}

/// The Ctrl-modified shortcuts, decided purely from the key states: Ctrl+N/O/Q, Ctrl+S
/// (Save) / Ctrl+Shift+S (Save As), Ctrl+W closes the active tab, and Ctrl+Shift+P toggles
/// the Problems window (plain Ctrl+P without Shift does nothing). `active` is the workspace's
/// active tab index when a doc is open (for Ctrl+W).
fn ctrl_shortcuts(keys: CtrlKeys, active: Option<usize>) -> Vec<EditorAction> {
    let mut out = Vec::new();
    for (pressed, action) in [
        (keys.n, EditorAction::New),
        (keys.o, EditorAction::Open),
        (keys.q, EditorAction::Quit),
    ] {
        if pressed {
            out.push(action);
        }
    }
    if keys.s {
        out.push(if keys.shift {
            EditorAction::SaveAs
        } else {
            EditorAction::Save
        });
    }
    if keys.w
        && let Some(index) = active
    {
        out.push(EditorAction::CloseTab(index));
    }
    if keys.p && keys.shift {
        out.push(EditorAction::ToggleProblems);
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
    let viewport = super::display_viewport(ui);
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
    actions.extend(ctrl_shortcuts(
        CtrlKeys {
            n: ui.is_key_pressed(Key::N),
            o: ui.is_key_pressed(Key::O),
            q: ui.is_key_pressed(Key::Q),
            s: ui.is_key_pressed(Key::S),
            w: ui.is_key_pressed(Key::W),
            p: ui.is_key_pressed(Key::P),
            shift: io.key_shift,
        },
        active,
    ));
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

    /// A `CtrlKeys` with only the given keys pressed.
    fn keys(n: bool, o: bool, q: bool, s: bool, w: bool, p: bool, shift: bool) -> CtrlKeys {
        CtrlKeys {
            n,
            o,
            q,
            s,
            w,
            p,
            shift,
        }
    }

    #[test]
    fn ctrl_shift_p_toggles_problems_and_plain_ctrl_p_does_nothing() {
        // Ctrl+Shift+P toggles the Problems window.
        assert_eq!(
            ctrl_shortcuts(keys(false, false, false, false, false, true, true), None),
            vec![EditorAction::ToggleProblems]
        );
        // Ctrl+P without Shift: no binding, nothing fires.
        assert!(
            ctrl_shortcuts(keys(false, false, false, false, false, true, false), None).is_empty()
        );
        // Ctrl+Shift+P alongside the others: everything fires, Problems last.
        assert_eq!(
            ctrl_shortcuts(keys(true, true, true, true, true, true, true), Some(2)),
            vec![
                EditorAction::New,
                EditorAction::Open,
                EditorAction::Quit,
                EditorAction::SaveAs,
                EditorAction::CloseTab(2),
                EditorAction::ToggleProblems,
            ]
        );
    }

    #[test]
    fn ctrl_shortcuts_keep_the_existing_bindings_unchanged() {
        // Ctrl+S is Save, Ctrl+Shift+S is Save As.
        assert_eq!(
            ctrl_shortcuts(keys(false, false, false, true, false, false, false), None),
            vec![EditorAction::Save]
        );
        assert_eq!(
            ctrl_shortcuts(keys(false, false, false, true, false, false, true), None),
            vec![EditorAction::SaveAs]
        );
        // Ctrl+W closes the active tab only when there is one.
        assert_eq!(
            ctrl_shortcuts(
                keys(false, false, false, false, true, false, false),
                Some(1)
            ),
            vec![EditorAction::CloseTab(1)]
        );
        assert!(
            ctrl_shortcuts(keys(false, false, false, false, true, false, false), None).is_empty()
        );
        // N/O/Q in that order; nothing pressed -> nothing.
        assert_eq!(
            ctrl_shortcuts(keys(true, true, true, false, false, false, false), None),
            vec![EditorAction::New, EditorAction::Open, EditorAction::Quit]
        );
        assert!(ctrl_shortcuts(CtrlKeys::default(), None).is_empty());
        // Shift with Ctrl but no key: nothing.
        assert!(
            ctrl_shortcuts(keys(false, false, false, false, false, false, true), None).is_empty()
        );
    }
}

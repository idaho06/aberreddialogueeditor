//! User intents and their single point of application.
//!
//! The menu, the keyboard shortcuts and the tab close buttons all produce an [`EditorAction`];
//! exactly one function, [`apply_action`], turns it into workspace mutations plus GUI-side
//! effects ([`ActionEffects`]). `apply_action` works on a plain [`Workspace`] so it is unit
//! testable; the native-dialog requests go through the optional `app_state` parameter — with
//! `Some` they are requested immediately, with `None` they are returned in
//! [`ActionEffects::dialog`] for the caller to dispatch (the GUI does that after releasing the
//! workspace lock, so no dialog code ever runs under it).

use aberredengine::prelude::*;

use crate::model::Workspace;
use crate::ops;
use crate::systems::file_dialogs::{
    AsyncFileDialogRequest, dialog_in_flight, request_async_dialog,
};

/// One user intent, produced by the File menu, a shortcut or a tab close button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorAction {
    /// File > New / Ctrl+N: append an untitled doc and activate it.
    New,
    /// File > Open... / Ctrl+O: open the native open dialog.
    Open,
    /// File > Save / Ctrl+S: save the active doc (to its path, or Save As when untitled).
    Save,
    /// File > Save As... / Ctrl+Shift+S: open the native save dialog for the active doc.
    SaveAs,
    /// File > Close Tab / Ctrl+W / tab close button: close the doc at the given index.
    CloseTab(usize),
    /// File > Quit / Ctrl+Q.
    Quit,
}

/// Effects of an applied action that need more than the workspace itself.
#[derive(Debug, Default)]
pub struct ActionEffects {
    /// The caller must run `intents.request_quit()`.
    pub quit: bool,
    /// A native file dialog to request. Only set when `apply_action` ran with `app_state: None`;
    /// with `Some(app_state)` the dialog was requested inside [`apply_action`] instead.
    pub dialog: Option<AsyncFileDialogRequest>,
}

/// Tab label: the doc title, `*` when dirty, and a `##<index>` id suffix so that equal titles
/// cannot collide in imgui's id stack.
pub fn tab_label(title: &str, dirty: bool, index: usize) -> String {
    let star = if dirty { "*" } else { "" };
    format!("{title}{star}##{index}")
}

/// What closing a tab with the given dirty state requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseDecision {
    /// The doc is clean: close immediately.
    CloseNow,
    /// The doc is dirty: ask first (the confirm modal).
    Confirm,
}

/// Decides how a close request for a doc with the given dirty flag is handled.
pub fn close_decision(dirty: bool) -> CloseDecision {
    if dirty {
        CloseDecision::Confirm
    } else {
        CloseDecision::CloseNow
    }
}

/// What a quit request requires, given the whole workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuitDecision {
    /// No doc is dirty: quit immediately.
    QuitNow,
    /// At least one doc is dirty: ask first (the confirm modal).
    Confirm,
}

/// Any dirty doc means quit needs confirmation.
pub fn quit_decision(ws: &Workspace) -> QuitDecision {
    if ws.docs.iter().any(|doc| doc.dirty) {
        QuitDecision::Confirm
    } else {
        QuitDecision::QuitNow
    }
}

/// Applies one user intent to the workspace. Pure with respect to the GUI: the only engine
/// interaction is the optional `app_state` used for native dialog requests and the in-flight
/// check that suspends tab closes while a native dialog is in flight.
pub fn apply_action(
    ws: &mut Workspace,
    action: EditorAction,
    app_state: Option<&AppState>,
) -> ActionEffects {
    let mut fx = ActionEffects::default();
    match action {
        EditorAction::New => ops::new_doc(ws),
        EditorAction::Open => {
            request_dialog(&mut fx, app_state, AsyncFileDialogRequest::OpenDialogue);
        }
        EditorAction::Save => {
            let path = ws.active_doc().and_then(|doc| doc.path.clone());
            match path {
                Some(path) => {
                    let result = ws.active_doc_mut().map(|doc| ops::save_doc(doc, &path));
                    if let Some(Err(err)) = result {
                        ws.notifications.push(ops::error_notification(&err));
                    }
                }
                // Untitled (or no) active doc: Save behaves like Save As.
                None => {
                    log::debug!(
                        "action {action:?}: active doc has no path, falling back to Save As"
                    );
                    request_save_as(&mut fx, app_state, ws);
                }
            }
        }
        EditorAction::SaveAs => request_save_as(&mut fx, app_state, ws),
        EditorAction::CloseTab(index) => {
            if app_state.is_some_and(dialog_in_flight) {
                log::debug!(
                    "action {action:?} ignored: a file dialog is in flight \
                     (active={}, docs={}, pending_close={:?}, pending_quit={})",
                    ws.active,
                    ws.docs.len(),
                    ws.pending_close,
                    ws.pending_quit
                );
                return fx;
            }
            let Some(dirty) = ws.docs.get(index).map(|doc| doc.dirty) else {
                log::debug!(
                    "action {action:?} ignored: no doc at index {index} (docs={})",
                    ws.docs.len()
                );
                return fx;
            };
            match close_decision(dirty) {
                CloseDecision::CloseNow => ops::close_doc(ws, index),
                CloseDecision::Confirm => ws.pending_close = Some(index),
            }
        }
        EditorAction::Quit => match quit_decision(ws) {
            QuitDecision::QuitNow => fx.quit = true,
            QuitDecision::Confirm => ws.pending_quit = true,
        },
    }
    log::debug!(
        "action {action:?} applied: active={}, docs={}, pending_close={:?}, pending_quit={}",
        ws.active,
        ws.docs.len(),
        ws.pending_close,
        ws.pending_quit
    );
    fx
}

/// Routes a dialog request: straight to the dialog layer when an `AppState` is available,
/// otherwise into the returned effects for the caller to dispatch.
fn request_dialog(
    fx: &mut ActionEffects,
    app_state: Option<&AppState>,
    request: AsyncFileDialogRequest,
) {
    match app_state {
        Some(app_state) => request_async_dialog(app_state, request),
        None => fx.dialog = Some(request),
    }
}

/// Requests the Save As dialog for the active doc; a no-op when no doc is open.
fn request_save_as(fx: &mut ActionEffects, app_state: Option<&AppState>, ws: &Workspace) {
    if ws.active_doc().is_some() {
        request_dialog(
            fx,
            app_state,
            AsyncFileDialogRequest::SaveDialogueAs {
                doc_index: ws.active,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenes::editor::testkit::{TempDir, fixture};
    use aberredengine::prelude::Vec2;

    /// A workspace with one valid dirty doc (one node, so the engine gate passes).
    fn ws_with_dirty_valid_doc() -> Workspace {
        let mut ws = Workspace::default();
        ops::new_doc(&mut ws);
        ws.active_doc_mut().unwrap().add_node(Vec2::ZERO);
        ws
    }

    #[test]
    fn tab_label_appends_star_and_index_suffix() {
        assert_eq!(tab_label("guard.json", false, 0), "guard.json##0");
        assert_eq!(tab_label("guard.json", true, 2), "guard.json*##2");
        assert_eq!(tab_label("Untitled", true, 1), "Untitled*##1");
        assert_ne!(
            tab_label("Untitled", false, 0),
            tab_label("Untitled", false, 1),
            "equal titles must not collide"
        );
    }

    #[test]
    fn close_decision_confirms_only_when_dirty() {
        assert_eq!(close_decision(false), CloseDecision::CloseNow);
        assert_eq!(close_decision(true), CloseDecision::Confirm);
    }

    #[test]
    fn quit_decision_confirms_when_any_doc_is_dirty() {
        let mut ws = Workspace::default();
        assert_eq!(quit_decision(&ws), QuitDecision::QuitNow, "empty workspace");
        ops::new_doc(&mut ws);
        assert_eq!(
            quit_decision(&ws),
            QuitDecision::QuitNow,
            "a fresh doc is clean"
        );
        ws.active_doc_mut().unwrap().add_node(Vec2::ZERO);
        assert_eq!(
            quit_decision(&ws),
            QuitDecision::Confirm,
            "adding a node dirties the doc"
        );
        ws.active_doc_mut().unwrap().mark_clean();
        ops::new_doc(&mut ws);
        ws.active_doc_mut().unwrap().add_node(Vec2::ZERO);
        assert_eq!(
            quit_decision(&ws),
            QuitDecision::Confirm,
            "any dirty doc (not just the active one) confirms"
        );
    }

    #[test]
    fn new_adds_an_untitled_doc_and_activates_it() {
        let mut ws = Workspace::default();
        let fx = apply_action(&mut ws, EditorAction::New, None);
        assert!(!fx.quit && fx.dialog.is_none());
        assert_eq!(ws.docs.len(), 1);
        assert_eq!(ws.active, 0);
        assert!(ws.active_doc().unwrap().path.is_none(), "untitled");
        apply_action(&mut ws, EditorAction::New, None);
        assert_eq!(ws.docs.len(), 2);
        assert_eq!(ws.active, 1);
    }

    #[test]
    fn open_without_app_state_queues_the_open_dialog() {
        let mut ws = Workspace::default();
        let fx = apply_action(&mut ws, EditorAction::Open, None);
        assert!(
            matches!(fx.dialog, Some(AsyncFileDialogRequest::OpenDialogue)),
            "the open dialog is requested: {:?}",
            fx.dialog
        );
        assert!(ws.docs.is_empty(), "no AppState: nothing else happens");
    }

    #[test]
    fn save_as_queues_the_save_dialog_for_the_active_doc() {
        let mut ws = ws_with_dirty_valid_doc();
        ops::new_doc(&mut ws); // active doc is now index 1
        let fx = apply_action(&mut ws, EditorAction::SaveAs, None);
        assert!(matches!(
            fx.dialog,
            Some(AsyncFileDialogRequest::SaveDialogueAs { doc_index: 1 })
        ));
    }

    #[test]
    fn save_as_without_an_active_doc_does_nothing() {
        let mut ws = Workspace::default();
        let fx = apply_action(&mut ws, EditorAction::SaveAs, None);
        assert!(fx.dialog.is_none() && !fx.quit);
    }

    #[test]
    fn save_without_a_path_behaves_like_save_as() {
        let mut ws = ws_with_dirty_valid_doc();
        assert!(ws.active_doc().unwrap().path.is_none());
        let fx = apply_action(&mut ws, EditorAction::Save, None);
        assert!(matches!(
            fx.dialog,
            Some(AsyncFileDialogRequest::SaveDialogueAs { doc_index: 0 })
        ));
        assert!(ws.docs.len() == 1 && ws.active_doc().unwrap().dirty);
    }

    #[test]
    fn save_with_a_path_on_a_valid_doc_clears_dirty_and_writes_the_file() {
        let tmp = TempDir::new("actions", "save-valid");
        let mut ws = ws_with_dirty_valid_doc();
        let out = tmp.join("out.json");
        ws.active_doc_mut().unwrap().path = Some(out.clone());
        let fx = apply_action(&mut ws, EditorAction::Save, None);
        assert!(fx.dialog.is_none() && !fx.quit);
        assert!(ws.notifications.is_empty(), "a good save is silent");
        let doc = ws.active_doc().unwrap();
        assert!(!doc.dirty, "save clears dirty");
        assert_eq!(doc.path, Some(out.canonicalize().unwrap()));
        assert!(out.exists(), "the file was written");
    }

    #[test]
    fn save_with_a_path_on_an_invalid_doc_pushes_the_engine_message_and_writes_nothing() {
        let tmp = TempDir::new("actions", "save-invalid");
        let mut ws = Workspace::default();
        ops::open_path(&mut ws, &fixture("invalid_dangling.json")).expect("opens");
        let out = tmp.join("out.json");
        ws.active_doc_mut().unwrap().path = Some(out.clone());
        let fx = apply_action(&mut ws, EditorAction::Save, None);
        assert!(fx.dialog.is_none() && !fx.quit);
        assert_eq!(ws.notifications.len(), 1);
        let note = &ws.notifications[0];
        assert!(
            note.contains("dialogue node \"a\" links to unknown node \"ghost\""),
            "the engine's own message: {note}"
        );
        assert!(!note.starts_with("invalid dialogue"), "no prefix: {note}");
        assert!(!out.exists(), "the gate blocks the write");
        assert!(
            ws.active_doc().unwrap().path == Some(out),
            "a rejected save leaves the path alone"
        );
    }

    #[test]
    fn close_tab_clean_closes_immediately_and_dirty_asks() {
        let mut ws = Workspace::default();
        ops::new_doc(&mut ws); // doc 0, clean
        ops::new_doc(&mut ws); // doc 1, active, clean
        ws.active_doc_mut().unwrap().add_node(Vec2::ZERO); // doc 1 dirty

        let fx = apply_action(&mut ws, EditorAction::CloseTab(0), None);
        assert!(fx.dialog.is_none() && !fx.quit);
        assert_eq!(ws.docs.len(), 1, "the clean tab closed at once");
        assert_eq!(ws.pending_close, None);
        assert_eq!(ws.active, 0, "active shifted down with the docs");

        apply_action(&mut ws, EditorAction::CloseTab(0), None);
        assert_eq!(ws.docs.len(), 1, "the dirty tab stays open");
        assert_eq!(ws.pending_close, Some(0), "the confirm modal is requested");
    }

    #[test]
    fn close_tab_out_of_range_is_a_no_op() {
        let mut ws = ws_with_dirty_valid_doc();
        let fx = apply_action(&mut ws, EditorAction::CloseTab(9), None);
        assert!(fx.dialog.is_none() && !fx.quit);
        assert_eq!(ws.docs.len(), 1);
        assert_eq!(ws.pending_close, None);
    }

    #[test]
    fn quit_is_immediate_when_clean_and_confirms_when_dirty() {
        let mut ws = Workspace::default();
        ops::new_doc(&mut ws);
        let fx = apply_action(&mut ws, EditorAction::Quit, None);
        assert!(fx.quit, "clean workspace quits now");
        assert!(!ws.pending_quit);

        ws.active_doc_mut().unwrap().add_node(Vec2::ZERO);
        let fx = apply_action(&mut ws, EditorAction::Quit, None);
        assert!(!fx.quit, "dirty workspace must not quit yet");
        assert!(ws.pending_quit, "the confirm modal is requested");
    }
}

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

use crate::graph::{auto_layout, center_on, frame_doc, node_rect, screen_to_world};
use crate::model::{DialogueDoc, NodeKey, Workspace};
use crate::ops;
use crate::systems::file_dialogs::{
    AsyncFileDialogRequest, dialog_in_flight, request_async_dialog,
};

/// One user intent, produced by a menu item, a shortcut, a tab close button or a graph
/// context-menu item.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EditorAction {
    /// File > New / Ctrl+N: append an untitled doc and activate it.
    New,
    /// File > Open... / Ctrl+O: open the native open dialog.
    Open,
    /// File > Save / Ctrl+S: save the active doc (to its path, or Save As when untitled).
    Save,
    /// File > Save As... / Ctrl+Shift+S: open the native save dialog for the active doc — but
    /// only after the save pre-check (`ops::check_saveable`) passes; a rejected doc pushes a
    /// SaveRejected notification and no dialog is requested.
    SaveAs,
    /// File > Close Tab / Ctrl+W / tab close button: close the doc at the given index.
    CloseTab(usize),
    /// File > Quit / Ctrl+Q.
    Quit,
    /// View > "Frame all" / F: fit the active doc's graph into the viewport. No-op without an
    /// active doc. `viewport` is the window size in screen pixels, read by the producer from
    /// `ui.io().display_size`.
    FrameAll { viewport: Vec2 },
    /// View > "Re-layout": re-run the auto-layout on the active doc, then frame all like
    /// [`EditorAction::FrameAll`]. Positions are view-only, so a clean doc stays clean (and a
    /// dirty one stays dirty). No-op without an active doc. `viewport` is read by the producer
    /// from `ui.io().display_size`.
    Relayout { viewport: Vec2 },
    /// Help > "Controls": show the controls help modal (workspace-level, works with and without
    /// an active doc).
    ShowControls,
    /// Context menu "Add node here": append a node at the given world position and select it.
    /// No-op without an active doc.
    AddNodeAt(Vec2),
    /// Edit > "Add node": append a node at the world position of the screen centre and select
    /// it. No-op without an active doc. `viewport` is read by the producer from
    /// `ui.io().display_size`.
    AddNode { viewport: Vec2 },
    /// Context menu "Delete node": remove the node (links to it become End). No-op without an
    /// active doc or for an unknown key.
    DeleteNode(NodeKey),
    /// Context menu "Set as start": make the node the doc's start. No-op without an active doc
    /// or for an unknown key.
    SetStart(NodeKey),
    /// Delete key: remove the selected node, if any. No-op without an active doc or selection.
    DeleteSelected,
    /// Problems panel row click: select the node and centre the camera on it (view-only:
    /// `select` bumps `revision` but never sets `dirty`; `set_camera` bumps nothing). No-op
    /// without an active doc or for an unknown node key.
    FocusNode(NodeKey),
    /// View > "Problems" / Ctrl+Shift+P / the Problems window's close button: toggle the
    /// Problems window (the `Workspace.problems_hidden` flag). Works with and without a doc.
    ToggleProblems,
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

/// Tab label: the doc title, `*` when dirty, and a `###doc{doc_id}` suffix. Everything before
/// `###` is display-only; imgui's id is always `doc{doc_id}`, so equal titles cannot collide and
/// editing a doc (the dirty star) or closing a tab never changes a tab's identity.
pub fn tab_label(title: &str, dirty: bool, doc_id: u64) -> String {
    let star = if dirty { "*" } else { "" };
    format!("{title}{star}###doc{doc_id}")
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

/// Any dirty doc means quit needs confirmation: the same decision as closing a dirty tab.
pub fn quit_decision(ws: &Workspace) -> QuitDecision {
    match close_decision(ws.docs.iter().any(|doc| doc.dirty)) {
        CloseDecision::CloseNow => QuitDecision::QuitNow,
        CloseDecision::Confirm => QuitDecision::Confirm,
    }
}

/// Applies `f` to the active doc; a no-op when no doc is open.
fn with_active_doc(ws: &mut Workspace, f: impl FnOnce(&mut DialogueDoc)) {
    if let Some(doc) = ws.active_doc_mut() {
        f(doc);
    }
}

/// The Save arm: save to the doc's path when it has one (a gate rejection pushes a
/// notification), otherwise fall back to Save As (untitled or no active doc).
fn save_active(ws: &mut Workspace, fx: &mut ActionEffects, app_state: Option<&AppState>) {
    let path = ws.active_doc().and_then(|doc| doc.path.clone());
    match path {
        Some(path) => {
            let result = ws.active_doc_mut().map(|doc| ops::save_doc(doc, &path));
            if let Some(Err(err)) = result {
                ops::notify(ws, &err);
            }
        }
        None => {
            log::debug!(
                "action {:?}: active doc has no path, falling back to Save As",
                EditorAction::Save
            );
            request_save_as(fx, app_state, ws, EditorAction::Save);
        }
    }
}

/// The CloseTab arm. Returns `true` when the close was APPLIED and `false` when the action
/// was IGNORED (a native dialog is in flight, or there is no doc at `index`) — the caller
/// returns early without the applied log when it returns `false`.
fn close_tab(ws: &mut Workspace, index: usize, app_state: Option<&AppState>) -> bool {
    let action = EditorAction::CloseTab(index);
    if app_state.is_some_and(dialog_in_flight) {
        log::debug!(
            "action {action:?} ignored: a file dialog is in flight \
             (active={}, docs={}, pending_close={:?}, pending_quit={})",
            ws.active,
            ws.docs.len(),
            ws.pending_close,
            ws.pending_quit
        );
        return false;
    }
    let Some(doc) = ws.docs.get(index) else {
        log::debug!(
            "action {action:?} ignored: no doc at index {index} (docs={})",
            ws.docs.len()
        );
        return false;
    };
    match close_decision(doc.dirty) {
        CloseDecision::CloseNow => ops::close_doc(ws, index),
        // Keyed by the doc's unique, never-reused doc_id (not its tab index), so tabs shifting
        // while the confirm modal is open cannot redirect the close to the wrong tab.
        CloseDecision::Confirm => ws.pending_close = Some(doc.doc_id),
    }
    true
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
        EditorAction::Save => save_active(ws, &mut fx, app_state),
        EditorAction::SaveAs => request_save_as(&mut fx, app_state, ws, EditorAction::SaveAs),
        EditorAction::CloseTab(index) => {
            if !close_tab(ws, index, app_state) {
                return fx;
            }
        }
        EditorAction::Quit => match quit_decision(ws) {
            QuitDecision::QuitNow => fx.quit = true,
            QuitDecision::Confirm => ws.pending_quit = true,
        },
        EditorAction::FrameAll { viewport } => {
            with_active_doc(ws, |doc| frame_doc(doc, viewport));
        }
        EditorAction::Relayout { viewport } => {
            // auto_layout writes positions via set_node_pos (view-only, never dirty) and
            // frame_doc only sets the camera.
            with_active_doc(ws, |doc| {
                auto_layout(doc);
                frame_doc(doc, viewport);
            });
        }
        EditorAction::ShowControls => {
            ws.show_controls = true;
        }
        EditorAction::AddNodeAt(world) => add_node_and_select(ws, world),
        EditorAction::AddNode { viewport } => {
            // The screen centre maps to the camera target.
            let world = ws
                .active_doc()
                .map(|doc| screen_to_world(&doc.camera, viewport, viewport / 2.0));
            if let Some(world) = world {
                add_node_and_select(ws, world);
            }
        }
        EditorAction::DeleteNode(key) => with_active_doc(ws, |doc| doc.delete_node(key)),
        EditorAction::SetStart(key) => with_active_doc(ws, |doc| doc.set_start(key)),
        EditorAction::DeleteSelected => {
            with_active_doc(ws, |doc| {
                if let Some(key) = doc.selected {
                    doc.delete_node(key);
                }
            });
        }
        EditorAction::FocusNode(key) => {
            with_active_doc(ws, |doc| {
                let centre = doc.node(key).map(|node| {
                    let (min, max) = node_rect(node);
                    (min + max) / 2.0
                });
                if let Some(centre) = centre {
                    doc.select(Some(key));
                    let camera = doc.camera;
                    doc.set_camera(center_on(&camera, centre));
                }
            });
        }
        EditorAction::ToggleProblems => {
            ws.problems_hidden = !ws.problems_hidden;
        }
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

/// Appends a node at `world` in the active doc and selects it; no-op without an active doc.
fn add_node_and_select(ws: &mut Workspace, world: Vec2) {
    with_active_doc(ws, |doc| {
        let key = doc.add_node(world);
        doc.select(Some(key));
    });
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

/// The Save As arm: runs the save pre-check ([`ops::check_saveable`], the same predicate and
/// message `ops::save_doc` enforces) BEFORE any native picker opens — a doc the engine would
/// reject pushes a SaveRejected notification onto `ws.notifications` and requests no dialog.
/// The request is keyed by the active doc's unique, never-reused `doc_id` (not its tab index),
/// so tabs closed or shifted while the picker is open cannot redirect the save.
/// No active doc: no-op (no dialog, no notification).
fn request_save_as(
    fx: &mut ActionEffects,
    app_state: Option<&AppState>,
    ws: &mut Workspace,
    action: EditorAction,
) {
    let Some(doc) = ws.active_doc() else {
        return;
    };
    if let Err(err) = ops::check_saveable(doc) {
        log::debug!(
            "action {action:?}: save pre-check rejected the active doc, no dialog requested"
        );
        ops::notify(ws, &err);
        return;
    }
    let doc_id = doc.doc_id;
    request_dialog(
        fx,
        app_state,
        AsyncFileDialogRequest::SaveDialogueAs {
            doc_id,
            close_after: false,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CamState, Link, NotificationKind, Outputs};
    use crate::testkit::{TempDir, fixture, ws_with_duplicate_id_doc, ws_with_two_linked_nodes};
    use aberredengine::prelude::Vec2;

    /// A workspace with one valid dirty doc (one node, so the engine gate passes).
    fn ws_with_dirty_valid_doc() -> Workspace {
        let mut ws = Workspace::default();
        ops::new_doc(&mut ws);
        ws.active_doc_mut().unwrap().add_node(Vec2::ZERO);
        ws
    }

    #[test]
    fn tab_label_uses_a_stable_doc_id_after_the_hashes() {
        assert_eq!(tab_label("guard.json", false, 7), "guard.json###doc7");
        assert_eq!(tab_label("guard.json", true, 7), "guard.json*###doc7");
        assert_eq!(tab_label("Untitled", true, 1), "Untitled*###doc1");
        assert_ne!(
            tab_label("Untitled", false, 1),
            tab_label("Untitled", false, 2),
            "equal titles with different doc ids must not collide"
        );
        // The id part (after ###) is identical whether the doc is dirty or clean.
        let clean = tab_label("Untitled", false, 3);
        let dirty = tab_label("Untitled", true, 3);
        assert_eq!(clean.split("###").nth(1), Some("doc3"));
        assert_eq!(clean.split("###").nth(1), dirty.split("###").nth(1));
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
    fn save_as_queues_the_save_dialog_for_the_active_docs_doc_id() {
        let mut ws = ws_with_dirty_valid_doc();
        ops::new_doc(&mut ws); // active doc is now index 1
        // An empty new doc has no start and would be rejected up front: make it valid.
        ws.active_doc_mut().unwrap().add_node(Vec2::ZERO);
        assert_eq!(ws.active, 1, "the second doc is active");
        let active_id = ws.active_doc().unwrap().doc_id;
        let fx = apply_action(&mut ws, EditorAction::SaveAs, None);
        assert!(
            matches!(
                fx.dialog,
                Some(AsyncFileDialogRequest::SaveDialogueAs { doc_id, close_after: false })
                    if doc_id == active_id
            ),
            "the request carries the ACTIVE doc's doc_id, not its index: {:?}",
            fx.dialog
        );
        assert!(ws.notifications.is_empty(), "a passing pre-check is silent");
    }

    #[test]
    fn save_as_on_a_valid_doc_still_queues_the_dialog_with_the_doc_id() {
        let mut ws = ws_with_dirty_valid_doc();
        let doc_id = ws.active_doc().unwrap().doc_id;
        let fx = apply_action(&mut ws, EditorAction::SaveAs, None);
        assert!(matches!(
            fx.dialog,
            Some(AsyncFileDialogRequest::SaveDialogueAs { doc_id: id, close_after: false })
                if id == doc_id
        ));
        assert!(ws.notifications.is_empty());
    }

    #[test]
    fn save_as_on_an_invalid_doc_requests_no_dialog_and_pushes_one_save_rejected() {
        let mut ws = ws_with_duplicate_id_doc();
        let expected = match ops::check_saveable(ws.active_doc().unwrap()) {
            Err(ops::OpError::Invalid(msg)) => msg,
            other => panic!("expected Invalid, got {other:?}"),
        };
        let fx = apply_action(&mut ws, EditorAction::SaveAs, None);
        assert!(
            fx.dialog.is_none(),
            "no picker for a doc the engine would reject"
        );
        assert!(!fx.quit);
        assert_eq!(ws.notifications.len(), 1, "exactly one notification");
        let note = &ws.notifications[0];
        assert_eq!(note.kind, NotificationKind::SaveRejected);
        assert_eq!(
            note.message, expected,
            "the notification carries the engine's own message, like save_doc's rejection"
        );
        assert!(
            note.message.contains("duplicate dialogue node id"),
            "the engine's message: {}",
            note.message
        );
    }

    #[test]
    fn save_as_on_a_brand_new_empty_doc_is_rejected_before_the_dialog() {
        let mut ws = Workspace::default();
        ops::new_doc(&mut ws); // empty doc: no start node, the engine gate rejects it
        let fx = apply_action(&mut ws, EditorAction::SaveAs, None);
        assert!(fx.dialog.is_none() && !fx.quit);
        assert_eq!(ws.notifications.len(), 1);
        let note = &ws.notifications[0];
        assert_eq!(note.kind, NotificationKind::SaveRejected);
        assert!(
            note.message.contains("is not among the defined nodes"),
            "the engine's no-start message: {}",
            note.message
        );
    }

    #[test]
    fn save_as_without_an_active_doc_does_nothing() {
        let mut ws = Workspace::default();
        for action in [EditorAction::SaveAs, EditorAction::Save] {
            let fx = apply_action(&mut ws, action, None);
            assert!(fx.dialog.is_none() && !fx.quit, "{action:?}");
            assert!(
                ws.notifications.is_empty(),
                "{action:?}: no active doc, no notification"
            );
        }
    }

    #[test]
    fn save_without_a_path_behaves_like_save_as_with_the_active_doc_id() {
        let mut ws = ws_with_dirty_valid_doc();
        ops::new_doc(&mut ws); // second, untitled doc is active
        ws.active_doc_mut().unwrap().add_node(Vec2::ZERO); // valid
        assert_eq!(ws.active, 1);
        assert!(ws.active_doc().unwrap().path.is_none(), "untitled");
        let active_id = ws.active_doc().unwrap().doc_id;
        let fx = apply_action(&mut ws, EditorAction::Save, None);
        assert!(
            matches!(
                fx.dialog,
                Some(AsyncFileDialogRequest::SaveDialogueAs { doc_id, close_after: false })
                    if doc_id == active_id
            ),
            "Save on an untitled valid doc requests Save As for the ACTIVE doc's id: {:?}",
            fx.dialog
        );
        assert!(ws.notifications.is_empty());
        assert!(ws.docs.len() == 2 && ws.active_doc().unwrap().dirty);
    }

    #[test]
    fn save_on_an_untitled_invalid_doc_behaves_like_save_as_no_dialog_one_notification() {
        let mut ws = ws_with_duplicate_id_doc();
        assert!(ws.active_doc().unwrap().path.is_none(), "untitled");
        let fx = apply_action(&mut ws, EditorAction::Save, None);
        assert!(
            fx.dialog.is_none() && !fx.quit,
            "Save on an untitled rejected doc must not open a picker"
        );
        assert_eq!(ws.notifications.len(), 1, "exactly one notification");
        let note = &ws.notifications[0];
        assert_eq!(note.kind, NotificationKind::SaveRejected);
        assert!(
            note.message.contains("duplicate dialogue node id"),
            "the engine's message: {}",
            note.message
        );
        assert!(ws.active_doc().unwrap().dirty, "nothing was saved");
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
        assert_eq!(
            note.kind,
            crate::model::NotificationKind::SaveRejected,
            "an engine-gate rejection from Save is a SaveRejected notification"
        );
        assert!(
            note.message
                .contains("dialogue node \"a\" links to unknown node \"ghost\""),
            "the engine's own message: {}",
            note.message
        );
        assert!(
            !note.message.starts_with("invalid dialogue"),
            "no prefix: {}",
            note.message
        );
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
        assert_eq!(
            ws.pending_close,
            Some(ws.docs[0].doc_id),
            "the confirm modal is requested for the dirty doc's doc_id"
        );
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
    fn frame_all_sets_the_active_docs_camera_view_only_and_no_ops_without_a_doc() {
        let viewport = Vec2::new(800.0, 600.0);
        let mut ws = Workspace::default();
        // No active doc: no-op (no panic, nothing happens).
        let fx = apply_action(&mut ws, EditorAction::FrameAll { viewport }, None);
        assert!(fx.dialog.is_none() && !fx.quit);
        assert!(ws.docs.is_empty());

        ops::open_path(&mut ws, &fixture("guard.json")).expect("opens");
        let revision = ws.active_doc().unwrap().revision;
        let mut expected_doc = ws.active_doc().unwrap().clone();
        frame_doc(&mut expected_doc, viewport);
        assert_ne!(
            ws.active_doc().unwrap().camera,
            expected_doc.camera,
            "framing changes the default camera of a laid-out doc"
        );
        let fx = apply_action(&mut ws, EditorAction::FrameAll { viewport }, None);
        assert!(fx.dialog.is_none() && !fx.quit);
        let doc = ws.active_doc().unwrap();
        assert_eq!(doc.camera, expected_doc.camera);
        assert_eq!(
            doc.revision, revision,
            "FrameAll is view-only: no revision bump"
        );
        assert!(!doc.dirty, "FrameAll does not dirty the doc");
    }

    #[test]
    fn relayout_relayouts_and_frames_the_active_doc_view_only_and_no_ops_without_a_doc() {
        let viewport = Vec2::new(800.0, 600.0);
        let mut ws = Workspace::default();
        // No active doc: no-op (no panic, nothing happens).
        let fx = apply_action(&mut ws, EditorAction::Relayout { viewport }, None);
        assert!(fx.dialog.is_none() && !fx.quit);
        assert!(ws.docs.is_empty());

        ops::open_path(&mut ws, &fixture("guard.json")).expect("opens");
        // Mess up the layout and the camera.
        let revision = {
            let doc = ws.active_doc_mut().unwrap();
            let keys: Vec<NodeKey> = doc.nodes.iter().map(|n| n.key).collect();
            for (i, key) in keys.iter().enumerate() {
                doc.set_node_pos(*key, Vec2::new(1000.0 + i as f32 * 10.0, -500.0));
            }
            doc.set_camera(CamState {
                target: Vec2::new(9.0, 9.0),
                zoom: 3.0,
            });
            assert!(!doc.dirty, "set_node_pos/set_camera never dirty");
            doc.revision
        };
        let mut expected = ws.active_doc().unwrap().clone();
        auto_layout(&mut expected);
        frame_doc(&mut expected, viewport);

        let fx = apply_action(&mut ws, EditorAction::Relayout { viewport }, None);
        assert!(fx.dialog.is_none() && !fx.quit);
        let doc = ws.active_doc().unwrap();
        let positions = |d: &crate::model::DialogueDoc| {
            d.nodes.iter().map(|n| (n.key, n.pos)).collect::<Vec<_>>()
        };
        assert_eq!(
            positions(doc),
            positions(&expected),
            "positions match a fresh auto_layout of the messed-up doc"
        );
        assert_eq!(doc.camera, expected.camera, "the camera matches frame_doc");
        assert!(!doc.dirty, "re-layout of a clean doc stays clean");
        assert!(
            doc.revision > revision,
            "set_node_pos bumps revision (the projection key)"
        );

        // A dirty doc stays dirty and is re-laid out the same way.
        {
            let doc = ws.active_doc_mut().unwrap();
            let keys: Vec<NodeKey> = doc.nodes.iter().map(|n| n.key).collect();
            for (i, key) in keys.iter().enumerate() {
                doc.set_node_pos(*key, Vec2::new(-2000.0, 700.0 + i as f32));
            }
            doc.add_node(Vec2::new(3.0, 3.0)); // content mutation: dirty
            assert!(doc.dirty);
        }
        let mut expected = ws.active_doc().unwrap().clone();
        auto_layout(&mut expected);
        frame_doc(&mut expected, viewport);
        let fx = apply_action(&mut ws, EditorAction::Relayout { viewport }, None);
        assert!(fx.dialog.is_none() && !fx.quit);
        let doc = ws.active_doc().unwrap();
        assert_eq!(positions(doc), positions(&expected));
        assert_eq!(doc.camera, expected.camera);
        assert!(doc.dirty, "re-layout keeps a dirty doc dirty");
    }

    #[test]
    fn show_controls_sets_the_workspace_flag_with_and_without_a_doc() {
        let mut ws = Workspace::default();
        let fx = apply_action(&mut ws, EditorAction::ShowControls, None);
        assert!(fx.dialog.is_none() && !fx.quit);
        assert!(ws.show_controls, "workspace-level: works without a doc");

        ws.show_controls = false;
        ops::open_path(&mut ws, &fixture("guard.json")).expect("opens");
        let fx = apply_action(&mut ws, EditorAction::ShowControls, None);
        assert!(fx.dialog.is_none() && !fx.quit);
        assert!(ws.show_controls, "still works with an active doc");
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

    #[test]
    fn add_node_at_adds_a_selected_dirty_node_at_the_position_and_the_first_node_becomes_start() {
        let mut ws = Workspace::default();
        ops::new_doc(&mut ws);
        ws.active_doc_mut().unwrap().mark_clean();
        let pos = Vec2::new(12.0, -7.0);
        let fx = apply_action(&mut ws, EditorAction::AddNodeAt(pos), None);
        assert!(fx.dialog.is_none() && !fx.quit);
        let doc = ws.active_doc().unwrap();
        assert_eq!(doc.nodes.len(), 1);
        let key = doc.nodes[0].key;
        assert_eq!(doc.node(key).map(|n| n.pos), Some(pos));
        assert_eq!(doc.selected, Some(key), "the new node is selected");
        assert!(doc.dirty, "adding a node dirties the doc");
        assert_eq!(
            doc.start,
            Some(key),
            "the first node of an empty doc becomes start"
        );
    }

    #[test]
    fn add_node_places_the_node_at_the_world_position_of_the_screen_centre() {
        let viewport = Vec2::new(800.0, 600.0);
        let mut ws = Workspace::default();
        ops::new_doc(&mut ws);
        // Default camera (target ZERO, zoom 1): the screen centre maps to the target.
        let fx = apply_action(&mut ws, EditorAction::AddNode { viewport }, None);
        assert!(fx.dialog.is_none() && !fx.quit);
        let doc = ws.active_doc().unwrap();
        assert_eq!(doc.nodes.len(), 1);
        assert_eq!(doc.camera, CamState::default());
        assert_eq!(doc.nodes[0].pos, doc.camera.target);
        assert_eq!(doc.selected, Some(doc.nodes[0].key));
        assert!(doc.dirty);
        // A moved/zoomed camera: the screen centre still maps to the target.
        let target = Vec2::new(50.0, -20.0);
        ws.active_doc_mut()
            .unwrap()
            .set_camera(CamState { target, zoom: 2.0 });
        apply_action(&mut ws, EditorAction::AddNode { viewport }, None);
        let doc = ws.active_doc().unwrap();
        assert_eq!(doc.nodes.len(), 2);
        assert_eq!(doc.nodes[1].pos, target);
        assert_eq!(doc.selected, Some(doc.nodes[1].key));
    }

    #[test]
    fn delete_node_removes_it_turns_incoming_links_into_end_and_clears_start_and_selection() {
        let (mut ws, a, b) = ws_with_two_linked_nodes();
        {
            let doc = ws.active_doc_mut().unwrap();
            doc.set_start(b);
            doc.select(Some(b));
            doc.mark_clean();
        }
        let revision = ws.active_doc().unwrap().revision;
        let fx = apply_action(&mut ws, EditorAction::DeleteNode(b), None);
        assert!(fx.dialog.is_none() && !fx.quit);
        let doc = ws.active_doc().unwrap();
        assert!(doc.node(b).is_none(), "the node is gone");
        assert!(
            matches!(doc.node(a).map(|n| &n.out), Some(Outputs::Next(Link::End))),
            "the link to the deleted node became End"
        );
        assert_eq!(doc.start, None, "start referenced the deleted node");
        assert_eq!(doc.selected, None, "selection referenced the deleted node");
        assert!(doc.dirty);
        assert_eq!(doc.revision, revision + 1);
        // Unknown key: no-op.
        let fx = apply_action(&mut ws, EditorAction::DeleteNode(NodeKey(999)), None);
        assert!(fx.dialog.is_none() && !fx.quit);
        assert_eq!(ws.active_doc().unwrap().revision, revision + 1);
        assert_eq!(ws.active_doc().unwrap().nodes.len(), 1);
    }

    #[test]
    fn set_start_changes_the_start_node_and_unknown_keys_are_no_ops() {
        let (mut ws, a, b) = ws_with_two_linked_nodes();
        assert_eq!(
            ws.active_doc().unwrap().start,
            Some(a),
            "first node is start"
        );
        ws.active_doc_mut().unwrap().mark_clean();
        let revision = ws.active_doc().unwrap().revision;
        let fx = apply_action(&mut ws, EditorAction::SetStart(b), None);
        assert!(fx.dialog.is_none() && !fx.quit);
        let doc = ws.active_doc().unwrap();
        assert_eq!(doc.start, Some(b));
        assert!(doc.dirty);
        assert_eq!(doc.revision, revision + 1);
        // Unknown key: no-op.
        apply_action(&mut ws, EditorAction::SetStart(NodeKey(999)), None);
        let doc = ws.active_doc().unwrap();
        assert_eq!(doc.start, Some(b));
        assert_eq!(doc.revision, revision + 1, "unknown key must not bump");
    }

    #[test]
    fn delete_selected_deletes_the_selection_and_no_ops_without_one() {
        let (mut ws, a, b) = ws_with_two_linked_nodes();
        ws.active_doc_mut().unwrap().select(Some(b));
        ws.active_doc_mut().unwrap().mark_clean();
        let fx = apply_action(&mut ws, EditorAction::DeleteSelected, None);
        assert!(fx.dialog.is_none() && !fx.quit);
        let doc = ws.active_doc().unwrap();
        assert!(doc.node(b).is_none(), "the selected node is deleted");
        assert!(
            matches!(doc.node(a).map(|n| &n.out), Some(Outputs::Next(Link::End))),
            "the link to it became End"
        );
        assert_eq!(doc.selected, None);
        assert!(doc.dirty);

        // No selection: a no-op (no revision bump, not dirty).
        ws.active_doc_mut().unwrap().mark_clean();
        let revision = ws.active_doc().unwrap().revision;
        let fx = apply_action(&mut ws, EditorAction::DeleteSelected, None);
        assert!(fx.dialog.is_none() && !fx.quit);
        let doc = ws.active_doc().unwrap();
        assert_eq!(doc.revision, revision, "no selection: no-op");
        assert!(!doc.dirty);
        assert_eq!(doc.nodes.len(), 1, "nothing was deleted");
    }

    #[test]
    fn node_actions_are_no_ops_without_an_active_doc() {
        let mut ws = Workspace::default();
        let viewport = Vec2::new(800.0, 600.0);
        for action in [
            EditorAction::AddNodeAt(Vec2::new(1.0, 2.0)),
            EditorAction::AddNode { viewport },
            EditorAction::DeleteNode(NodeKey(1)),
            EditorAction::SetStart(NodeKey(1)),
            EditorAction::DeleteSelected,
            EditorAction::FocusNode(NodeKey(1)),
        ] {
            let fx = apply_action(&mut ws, action, None);
            assert!(fx.dialog.is_none() && !fx.quit, "{action:?}");
            assert!(ws.docs.is_empty(), "{action:?}: nothing happened");
        }
    }

    #[test]
    fn focus_node_selects_the_node_and_centres_the_camera_without_dirtying() {
        let (mut ws, a, b) = ws_with_two_linked_nodes();
        {
            let doc = ws.active_doc_mut().unwrap();
            doc.set_camera(CamState {
                target: Vec2::new(999.0, -999.0),
                zoom: 2.5,
            });
            doc.select(None);
            doc.mark_clean();
        }
        let revision = ws.active_doc().unwrap().revision;
        let fx = apply_action(&mut ws, EditorAction::FocusNode(b), None);
        assert!(fx.dialog.is_none() && !fx.quit);
        let doc = ws.active_doc().unwrap();
        assert_eq!(doc.selected, Some(b), "the node is selected");
        let (min, max) = crate::graph::node_rect(doc.node(b).unwrap());
        let centre = (min + max) / 2.0;
        assert_eq!(
            doc.camera.target, centre,
            "the camera targets the node centre"
        );
        assert!(
            (doc.camera.zoom - 2.5).abs() < 1e-6,
            "the zoom is unchanged"
        );
        assert!(!doc.dirty, "FocusNode is view-only: never dirty");
        assert_eq!(
            doc.revision,
            revision + 1,
            "select bumps revision once; set_camera bumps nothing"
        );

        // Unknown key: a no-op (no selection change, no revision bump).
        let revision = doc.revision;
        let fx = apply_action(&mut ws, EditorAction::FocusNode(NodeKey(999)), None);
        assert!(fx.dialog.is_none() && !fx.quit);
        let doc = ws.active_doc().unwrap();
        assert_eq!(doc.selected, Some(b), "the selection is untouched");
        assert_eq!(doc.camera.target, centre, "the camera is untouched");
        assert_eq!(doc.revision, revision, "an unknown key is a no-op");
        assert!(!doc.dirty);

        // The other node focuses too.
        apply_action(&mut ws, EditorAction::FocusNode(a), None);
        let doc = ws.active_doc().unwrap();
        assert_eq!(doc.selected, Some(a));
        let (min, max) = crate::graph::node_rect(doc.node(a).unwrap());
        assert_eq!(doc.camera.target, (min + max) / 2.0);
    }

    #[test]
    fn toggle_problems_flips_the_visibility_flag_and_works_without_a_doc() {
        let mut ws = Workspace::default();
        assert!(
            !ws.problems_hidden,
            "the Problems window is visible by default"
        );
        let fx = apply_action(&mut ws, EditorAction::ToggleProblems, None);
        assert!(fx.dialog.is_none() && !fx.quit);
        assert!(ws.problems_hidden, "toggled: hidden");
        assert!(ws.docs.is_empty(), "workspace-level: works without a doc");
        apply_action(&mut ws, EditorAction::ToggleProblems, None);
        assert!(!ws.problems_hidden, "toggled back: visible");

        ops::new_doc(&mut ws);
        apply_action(&mut ws, EditorAction::ToggleProblems, None);
        assert!(ws.problems_hidden, "still works with an active doc");
    }
}

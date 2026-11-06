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

use crate::graph::{auto_layout, frame_doc, screen_to_world};
use crate::model::{NodeKey, Workspace};
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
    /// File > Save As... / Ctrl+Shift+S: open the native save dialog for the active doc.
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
        EditorAction::FrameAll { viewport } => {
            // No active doc: no-op.
            if let Some(doc) = ws.active_doc_mut() {
                frame_doc(doc, viewport);
            }
        }
        EditorAction::Relayout { viewport } => {
            // No active doc: no-op. auto_layout writes positions via set_node_pos (view-only,
            // never dirty) and frame_doc only sets the camera.
            if let Some(doc) = ws.active_doc_mut() {
                auto_layout(doc);
                frame_doc(doc, viewport);
            }
        }
        EditorAction::ShowControls => {
            ws.show_controls = true;
        }
        EditorAction::AddNodeAt(world) => add_node_and_select(ws, world),
        EditorAction::AddNode { viewport } => {
            // No active doc: no-op. The screen centre maps to the camera target.
            let world = ws
                .active_doc()
                .map(|doc| screen_to_world(&doc.camera, viewport, viewport / 2.0));
            if let Some(world) = world {
                add_node_and_select(ws, world);
            }
        }
        EditorAction::DeleteNode(key) => {
            // No active doc or unknown key: no-op (the mutator no-ops on unknown keys).
            if let Some(doc) = ws.active_doc_mut() {
                doc.delete_node(key);
            }
        }
        EditorAction::SetStart(key) => {
            // No active doc or unknown key: no-op (the mutator no-ops on unknown keys).
            if let Some(doc) = ws.active_doc_mut() {
                doc.set_start(key);
            }
        }
        EditorAction::DeleteSelected => {
            // No active doc or no selection: no-op.
            if let Some(doc) = ws.active_doc_mut()
                && let Some(key) = doc.selected
            {
                doc.delete_node(key);
            }
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
    if let Some(doc) = ws.active_doc_mut() {
        let key = doc.add_node(world);
        doc.select(Some(key));
    }
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
    use crate::model::{CamState, Link, Outputs, Slot};
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

    /// A workspace with one clean doc holding `a --next--> b`, start = a, no selection.
    fn ws_with_two_linked_nodes() -> (Workspace, NodeKey, NodeKey) {
        let mut ws = Workspace::default();
        ops::new_doc(&mut ws);
        let (a, b) = {
            let doc = ws.active_doc_mut().unwrap();
            let a = doc.add_node(Vec2::ZERO);
            let b = doc.add_node(Vec2::new(100.0, 0.0));
            doc.set_link(a, Slot::Next, Link::Node(b));
            doc.mark_clean();
            (a, b)
        };
        (ws, a, b)
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
        ] {
            let fx = apply_action(&mut ws, action, None);
            assert!(fx.dialog.is_none() && !fx.quit, "{action:?}");
            assert!(ws.docs.is_empty(), "{action:?}: nothing happened");
        }
    }
}

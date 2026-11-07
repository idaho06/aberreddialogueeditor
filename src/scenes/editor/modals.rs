//! Modal popups, opened from the frame's top-level scope after the menu scope: the notification
//! modal (first entry of `ws.notifications` — title chosen by its [`NotificationKind`]),
//! the "Unsaved changes" confirm for a pending tab close, the "Unsaved changes" confirm for a
//! pending quit, the Help > "Controls" modal (`ws.show_controls`) and the Help > "About..." modal
//! (`ws.show_about`). Only one shows at a
//! time; priority is notification, close, quit, then Controls, then About (lowest).
//!
//! Locking: the workspace is locked to snapshot the modal state, the guard is dropped while
//! the widgets are drawn, and a button click is applied with a fresh short lock. The Save As
//! dialog request (untitled doc in the close modal, after its save pre-check passes) happens
//! after that lock is released; it is keyed by the doc's `doc_id` and carries `close_after`, so
//! the tab closes after a successful save.

use aberredengine::prelude::imgui::Ui;
use aberredengine::prelude::*;

use crate::model::{Notification, NotificationKind, Workspace};
use crate::ops;
use crate::state::with_workspace;
use crate::systems::file_dialogs::{AsyncFileDialogRequest, request_async_dialog};

const ERROR_MODAL: &str = "Error##modal";
const SAVE_REJECTED_MODAL: &str = "Cannot save: the engine would reject this dialogue##modal";
const CLOSE_MODAL: &str = "Unsaved changes##close";
const QUIT_MODAL: &str = "Unsaved changes##quit";
const CONTROLS_MODAL: &str = "Controls##help";
const ABOUT_MODAL: &str = "About###about";

/// The modal title/id for a notification of `kind`: a plain [`NotificationKind::Error`] keeps
/// `"Error##modal"`; a [`NotificationKind::SaveRejected`] (the engine's save gate rejected the
/// doc) uses the "Cannot save: ..." title. Everything before `###` is display-only, so the id
/// part (`modal`) is stable across kinds.
fn modal_title(kind: NotificationKind) -> &'static str {
    match kind {
        NotificationKind::Error => ERROR_MODAL,
        NotificationKind::SaveRejected => SAVE_REJECTED_MODAL,
    }
}

/// The Help > "Controls" modal body: `(action, input)` rows, drawn as two aligned columns.
const CONTROLS: &[(&str, &str)] = &[
    ("Move a node", "Left-drag its body or header"),
    ("Select a node", "Left-click it"),
    (
        "Connect nodes",
        "Drag from a right-side pin onto another node",
    ),
    ("Disconnect", "Drag a linked pin off onto empty space"),
    ("Context menu", "Right-click"),
    ("Pan", "Middle-drag or Right-drag"),
    ("Zoom", "Mouse wheel"),
    ("Frame all", "F or View > Frame all"),
    ("Re-layout", "View > Re-layout"),
    (
        "Add node",
        "Right-click canvas > Add node here, or Edit > Add node",
    ),
    ("Delete node", "Delete key or right-click > Delete node"),
    (
        "New / Open / Save / Save As / Close tab / Quit",
        "Ctrl+N / Ctrl+O / Ctrl+S / Ctrl+Shift+S / Ctrl+W / Ctrl+Q",
    ),
];

/// What a modal button decided this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModalChoice {
    ErrorOk,
    CloseSave,
    CloseDiscard,
    CloseCancel,
    QuitDiscard,
    QuitCancel,
    ControlsOk,
    AboutOk,
}

/// Snapshot of the modal-relevant workspace state.
#[derive(Default)]
struct ModalSnap {
    /// First queued notification, if any.
    error: Option<Notification>,
    /// Pending close, resolved from its `doc_id` to `(tab index, title)` for the message. A doc
    /// that is no longer open resolves to `None`: no close modal is shown.
    pending_close: Option<(usize, String)>,
    pending_quit: bool,
    /// Help > "Controls" requested; shown only when no other modal is showing.
    show_controls: bool,
    /// Help > "About..." requested; shown only when no other modal is showing.
    show_about: bool,
}

fn snapshot(app_state: &AppState) -> ModalSnap {
    with_workspace(app_state, |ws| ModalSnap {
        error: ws.notifications.first().cloned(),
        pending_close: ws.pending_close.and_then(|doc_id| {
            ws.position(doc_id)
                .map(|index| (index, ops::doc_title(&ws.docs[index])))
        }),
        pending_quit: ws.pending_quit,
        show_controls: ws.show_controls,
        show_about: ws.show_about,
    })
    .unwrap_or_default()
}

/// Opens `id` as a modal popup and runs `body` while it is open (no-op when it cannot open).
fn with_modal(ui: &Ui, id: &str, body: impl FnOnce(&Ui)) {
    ui.open_popup(id);
    if let Some(_token) = ui.begin_modal_popup(id) {
        body(ui);
    }
}

/// One modal button: records `choice` and closes the popup on click.
fn choice_button(ui: &Ui, label: &str, choice: &mut Option<ModalChoice>, value: ModalChoice) {
    if ui.button(label) {
        *choice = Some(value);
        ui.close_current_popup();
    }
    ui.same_line();
}

/// Draws at most one modal and applies its button choice. Returns `true` when quit was
/// confirmed ("Discard and quit"), for the caller to run `intents.request_quit()`.
pub fn draw(ctx: &mut GuiCtx) -> bool {
    let snap = snapshot(ctx.app_state);
    let mut choice = None;
    let ui = ctx.ui;
    if let Some(note) = &snap.error {
        let id = modal_title(note.kind);
        with_modal(ui, id, |ui| {
            ui.text_wrapped(&note.message);
            if note.kind == NotificationKind::SaveRejected {
                ui.text_disabled("See the Problems panel for all issues.");
            }
            ui.separator();
            choice_button(ui, "OK", &mut choice, ModalChoice::ErrorOk);
        });
    } else if let Some((_index, title)) = &snap.pending_close {
        with_modal(ui, CLOSE_MODAL, |ui| {
            ui.text_wrapped(format!("\"{title}\" has unsaved changes."));
            choice_button(ui, "Save", &mut choice, ModalChoice::CloseSave);
            choice_button(ui, "Discard", &mut choice, ModalChoice::CloseDiscard);
            choice_button(ui, "Cancel", &mut choice, ModalChoice::CloseCancel);
        });
    } else if snap.pending_quit {
        with_modal(ui, QUIT_MODAL, |ui| {
            ui.text_wrapped("There are unsaved changes.");
            choice_button(
                ui,
                "Discard and quit",
                &mut choice,
                ModalChoice::QuitDiscard,
            );
            choice_button(ui, "Cancel", &mut choice, ModalChoice::QuitCancel);
        });
    } else if snap.show_controls {
        with_modal(ui, CONTROLS_MODAL, |ui| {
            ui.columns(2, "controls_columns", false);
            for (action, input) in CONTROLS {
                ui.text(*action);
                ui.next_column();
                ui.text(*input);
                ui.next_column();
            }
            ui.columns(1, "controls_columns", false);
            ui.separator();
            choice_button(ui, "OK", &mut choice, ModalChoice::ControlsOk);
        });
    } else if snap.show_about {
        with_modal(ui, ABOUT_MODAL, |ui| {
            ui.text(format!(
                "Aberred Dialogue Editor {}",
                env!("CARGO_PKG_VERSION")
            ));
            ui.text("Node-graph editor for AberredEngine dialogue JSON.");
            ui.text("By Idaho06 from AkinoSoft!");
            ui.text("(c) 2026 - MIT License");
            ui.text("https://github.com/idaho06/aberreddialogueeditor");
            ui.text("Built on AberredEngine.");
            ui.separator();
            choice_button(ui, "OK", &mut choice, ModalChoice::AboutOk);
        });
    }
    apply_choice(ctx.app_state, choice)
}

/// What the caller must do after the workspace lock is dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChoiceEffect {
    Nothing,
    /// The caller must run `intents.request_quit()` ("Discard and quit").
    Quit,
    /// The caller must request the Save As dialog for the doc with this unique, never-reused
    /// `doc_id` (untitled doc in the close modal), with `close_after: true` so the tab closes
    /// after a successful save.
    SaveAsDialog {
        doc_id: u64,
    },
}

/// Applies a modal button choice to the workspace. Pure over `&mut Workspace` (no imgui, no
/// `AppState`), so it is unit-testable; the caller dispatches the returned effect after the lock
/// is dropped.
fn apply_choice_ws(ws: &mut Workspace, choice: ModalChoice) -> ChoiceEffect {
    match choice {
        ModalChoice::ErrorOk => {
            if !ws.notifications.is_empty() {
                ws.notifications.remove(0);
                log::debug!("modal {choice:?}: dismissed the queued error notification");
            }
        }
        ModalChoice::CloseSave => {
            // Resolve the pending close's doc_id to its CURRENT tab index; a doc that is no
            // longer open means no modal target — clear the stale value like an out-of-range
            // index used to.
            let Some(index) = ws.pending_close.and_then(|doc_id| ws.position(doc_id)) else {
                ws.pending_close = None;
                log::debug!("modal {choice:?}: pending close doc is no longer open, cleared");
                return ChoiceEffect::Nothing;
            };
            match ws.docs[index].path.clone() {
                Some(path) => match ops::save_doc(&mut ws.docs[index], &path) {
                    Ok(()) => {
                        ops::close_doc(ws, index);
                        ws.pending_close = None;
                        log::debug!("modal {choice:?}: saved and closed tab {index}");
                    }
                    Err(err) => {
                        // A failure pushes a notification (shown by the error modal first) and
                        // keeps both the tab and the pending close.
                        ops::notify(ws, &err);
                        log::debug!(
                            "modal {choice:?}: save of tab {index} failed, error modal queued"
                        );
                    }
                },
                None => {
                    // Untitled doc: run the save pre-check (the same gate/message save_doc
                    // enforces) BEFORE handing over to the Save As dialog, so a doc the engine
                    // would reject never opens a picker. The tab stays open. The dialog request
                    // is keyed by the doc's unique doc_id (tabs may shift while the picker is
                    // open) and carries close_after, so a successful save closes the tab.
                    let doc_id = ws.docs[index].doc_id;
                    ws.pending_close = None;
                    let check = ops::check_saveable(&ws.docs[index]);
                    if let Err(err) = check {
                        log::debug!(
                            "modal {choice:?}: save pre-check rejected the untitled doc, no dialog"
                        );
                        ops::notify(ws, &err);
                        return ChoiceEffect::Nothing;
                    }
                    log::debug!(
                        "modal {choice:?}: tab {index} (doc {doc_id}) is untitled, Save As dialog next"
                    );
                    return ChoiceEffect::SaveAsDialog { doc_id };
                }
            }
        }
        ModalChoice::CloseDiscard => {
            if let Some(index) = ws.pending_close.and_then(|doc_id| ws.position(doc_id)) {
                ops::close_doc(ws, index);
                log::debug!("modal {choice:?}: discarded changes and closed tab {index}");
            }
            ws.pending_close = None;
        }
        ModalChoice::CloseCancel => {
            ws.pending_close = None;
            log::debug!("modal {choice:?}: close cancelled");
        }
        ModalChoice::QuitDiscard => {
            ws.pending_quit = false;
            log::debug!("modal {choice:?}: quit confirmed");
            return ChoiceEffect::Quit;
        }
        ModalChoice::QuitCancel => {
            ws.pending_quit = false;
            log::debug!("modal {choice:?}: quit cancelled");
        }
        ModalChoice::ControlsOk => {
            ws.show_controls = false;
            log::debug!("modal {choice:?}: controls modal closed");
        }
        ModalChoice::AboutOk => {
            ws.show_about = false;
            log::debug!("modal {choice:?}: about modal closed");
        }
    }
    ChoiceEffect::Nothing
}

/// Applies a modal button choice with a short workspace lock; the Save As dialog for an
/// untitled doc is requested after the lock is dropped. Returns `true` for "Discard and quit".
fn apply_choice(app_state: &AppState, choice: Option<ModalChoice>) -> bool {
    let Some(choice) = choice else {
        return false;
    };
    // Short lock; the Save As dialog request happens after it is dropped.
    let Some(effect) = with_workspace(app_state, |ws| {
        log::debug!("modal choice {choice:?} applied");
        apply_choice_ws(ws, choice)
    }) else {
        return false;
    };
    match effect {
        ChoiceEffect::Nothing => false,
        ChoiceEffect::Quit => true,
        ChoiceEffect::SaveAsDialog { doc_id } => {
            // close_after: the unsaved-changes Save closes the tab once the save succeeded.
            request_async_dialog(
                app_state,
                AsyncFileDialogRequest::SaveDialogueAs {
                    doc_id,
                    close_after: true,
                },
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{TempDir, fixture, ws_with_dirty_valid_doc, ws_with_duplicate_id_doc};
    use aberredengine::prelude::Vec2;

    #[test]
    fn close_save_on_an_untitled_invalid_doc_returns_nothing_and_queues_save_rejected() {
        let mut ws = ws_with_duplicate_id_doc();
        ws.pending_close = Some(ws.docs[0].doc_id);
        assert_eq!(
            apply_choice_ws(&mut ws, ModalChoice::CloseSave),
            ChoiceEffect::Nothing,
            "no Save As dialog for a doc the engine would reject"
        );
        assert_eq!(ws.pending_close, None, "the pending close is cleared");
        assert_eq!(ws.docs.len(), 1, "the tab stays open");
        assert_eq!(ws.notifications.len(), 1, "exactly one notification");
        let note = &ws.notifications[0];
        assert_eq!(note.kind, NotificationKind::SaveRejected);
        assert!(
            note.message.contains("duplicate dialogue node id"),
            "the engine's message: {}",
            note.message
        );
    }

    #[test]
    fn close_save_on_an_untitled_valid_doc_returns_the_save_as_dialog_with_the_doc_id() {
        let mut ws = ws_with_dirty_valid_doc(); // one node = start: valid
        let doc_id = ws.docs[0].doc_id;
        ws.pending_close = Some(doc_id);
        assert_eq!(
            apply_choice_ws(&mut ws, ModalChoice::CloseSave),
            ChoiceEffect::SaveAsDialog { doc_id }
        );
        assert_eq!(ws.pending_close, None);
        assert!(ws.notifications.is_empty(), "a passing pre-check is silent");
        assert_eq!(
            ws.docs.len(),
            1,
            "the tab stays open until the save completes"
        );
    }

    #[test]
    fn close_save_on_a_doc_with_a_path_saves_then_closes_as_before() {
        let tmp = TempDir::new("modals", "close-save-path");
        let mut ws = Workspace::default();
        ops::open_path(&mut ws, &fixture("guard.json")).expect("guard.json opens");
        ws.active_doc_mut().unwrap().add_node(Vec2::ZERO); // dirty
        let out = tmp.join("out.json");
        ws.active_doc_mut().unwrap().path = Some(out.clone());
        ws.pending_close = Some(ws.docs[0].doc_id);
        assert_eq!(
            apply_choice_ws(&mut ws, ModalChoice::CloseSave),
            ChoiceEffect::Nothing
        );
        assert!(ws.docs.is_empty(), "the saved tab closed");
        assert_eq!(ws.pending_close, None);
        assert!(ws.notifications.is_empty(), "a good save is silent");
        assert!(out.exists(), "the file was written");
    }

    #[test]
    fn close_discard_after_another_tab_was_closed_discards_the_original_pending_tab() {
        // The data-loss scenario: tabs [B clean(0), A dirty(1), C dirty(2)]; X on A sets
        // pending_close = A's doc_id and opens the modal; a shortcut (Ctrl+W) closes B while the
        // modal is open; "Discard" must close A — the tab the user confirmed — not C, which now
        // sits at index 0.
        let mut ws = Workspace::default();
        ops::new_doc(&mut ws); // B, index 0, clean
        ws.docs[0].path = Some(std::path::PathBuf::from("/tmp/b.json"));
        ops::new_doc(&mut ws); // A, index 1
        ws.docs[1].path = Some(std::path::PathBuf::from("/tmp/a.json"));
        ws.active_doc_mut().unwrap().add_node(Vec2::ZERO); // A dirty
        let a_id = ws.docs[1].doc_id;
        ops::new_doc(&mut ws); // C, index 2
        ws.docs[2].path = Some(std::path::PathBuf::from("/tmp/c.json"));
        ws.active_doc_mut().unwrap().add_node(Vec2::ZERO); // C dirty

        ws.pending_close = Some(a_id); // X clicked on A
        ops::close_doc(&mut ws, 0); // Ctrl+W closed B while the modal was open
        assert_eq!(
            ws.pending_close,
            Some(a_id),
            "pending_close holds A's doc_id: closing B does not change it"
        );
        assert_eq!(ws.position(a_id), Some(0), "A shifted to index 0");
        assert_eq!(ws.docs.len(), 2);

        assert_eq!(
            apply_choice_ws(&mut ws, ModalChoice::CloseDiscard),
            ChoiceEffect::Nothing
        );
        assert_eq!(ws.pending_close, None, "the modal is settled");
        assert_eq!(ws.docs.len(), 1, "exactly the confirmed tab closed");
        assert_eq!(
            ws.docs[0].path.as_deref(),
            Some(std::path::Path::new("/tmp/c.json")),
            "C survived; A (the confirmed tab) was discarded"
        );
    }

    #[test]
    fn modal_title_is_error_for_plain_errors_and_cannot_save_for_save_rejected() {
        assert_eq!(modal_title(NotificationKind::Error), "Error##modal");
        assert_eq!(
            modal_title(NotificationKind::SaveRejected),
            "Cannot save: the engine would reject this dialogue##modal"
        );
        // The imgui id part (after ##) is stable across kinds, only the title changes.
        assert_eq!(
            modal_title(NotificationKind::Error).split("##").nth(1),
            Some("modal")
        );
        assert_eq!(
            modal_title(NotificationKind::SaveRejected)
                .split("##")
                .nth(1),
            Some("modal")
        );
    }

    #[test]
    fn error_ok_removes_the_first_notification_regardless_of_kind() {
        let mut ws = Workspace::default();
        ws.notifications.push(Notification {
            kind: NotificationKind::SaveRejected,
            message: "engine message".to_owned(),
        });
        ws.notifications.push(Notification {
            kind: NotificationKind::Error,
            message: "io error".to_owned(),
        });
        assert_eq!(
            apply_choice_ws(&mut ws, ModalChoice::ErrorOk),
            ChoiceEffect::Nothing
        );
        assert_eq!(ws.notifications.len(), 1, "the first entry is removed");
        assert_eq!(ws.notifications[0].kind, NotificationKind::Error);
        assert_eq!(ws.notifications[0].message, "io error");
    }

    #[test]
    fn controls_ok_clears_the_show_controls_flag() {
        let mut ws = Workspace::default();
        assert!(!ws.show_controls, "default is false");
        ws.show_controls = true;
        assert_eq!(
            apply_choice_ws(&mut ws, ModalChoice::ControlsOk),
            ChoiceEffect::Nothing
        );
        assert!(!ws.show_controls, "OK clears the flag");
    }

    #[test]
    fn about_ok_clears_the_show_about_flag() {
        let mut ws = Workspace::default();
        assert!(!ws.show_about, "default is false");
        ws.show_about = true;
        assert_eq!(
            apply_choice_ws(&mut ws, ModalChoice::AboutOk),
            ChoiceEffect::Nothing
        );
        assert!(!ws.show_about, "OK clears the flag");
    }

    #[test]
    fn controls_list_covers_the_mouse_gestures_and_ctrl_shortcuts() {
        assert!(!CONTROLS.is_empty());
        let has = |action: &str, input: &str| CONTROLS.contains(&(action, input));
        // Mouse gestures.
        assert!(has("Move a node", "Left-drag its body or header"));
        assert!(has("Select a node", "Left-click it"));
        assert!(has(
            "Connect nodes",
            "Drag from a right-side pin onto another node"
        ));
        assert!(has("Disconnect", "Drag a linked pin off onto empty space"));
        assert!(has("Context menu", "Right-click"));
        assert!(has("Pan", "Middle-drag or Right-drag"));
        assert!(has("Zoom", "Mouse wheel"));
        // Menu/key entries.
        assert!(has("Frame all", "F or View > Frame all"));
        assert!(has("Re-layout", "View > Re-layout"));
        assert!(has(
            "Add node",
            "Right-click canvas > Add node here, or Edit > Add node"
        ));
        assert!(has(
            "Delete node",
            "Delete key or right-click > Delete node"
        ));
        // Ctrl shortcuts.
        assert!(has(
            "New / Open / Save / Save As / Close tab / Quit",
            "Ctrl+N / Ctrl+O / Ctrl+S / Ctrl+Shift+S / Ctrl+W / Ctrl+Q"
        ));
    }
}

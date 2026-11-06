//! Modal popups, opened from the frame's top-level scope after the menu scope: the error modal
//! (first entry of `ws.notifications`), the "Unsaved changes" confirm for a pending tab close,
//! and the "Unsaved changes" confirm for a pending quit. Only one shows at a time; the error
//! modal has priority.
//!
//! Locking: the workspace is locked to snapshot the modal state, the guard is dropped while
//! the widgets are drawn, and a button click is applied with a fresh short lock. The Save As
//! dialog request (untitled doc in the close modal) happens after that lock is released.

use aberredengine::prelude::*;

use crate::ops;
use crate::state::WorkspaceMutex;
use crate::systems::file_dialogs::{AsyncFileDialogRequest, request_async_dialog};

const ERROR_MODAL: &str = "Error##modal";
const CLOSE_MODAL: &str = "Unsaved changes##close";
const QUIT_MODAL: &str = "Unsaved changes##quit";

/// What a modal button decided this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModalChoice {
    ErrorOk,
    CloseSave,
    CloseDiscard,
    CloseCancel,
    QuitDiscard,
    QuitCancel,
}

/// Snapshot of the modal-relevant workspace state.
struct ModalSnap {
    /// First queued notification, if any.
    error: Option<String>,
    /// Pending close: tab index plus its title for the message.
    pending_close: Option<(usize, String)>,
    pending_quit: bool,
}

fn snapshot(app_state: &AppState) -> ModalSnap {
    let empty = ModalSnap {
        error: None,
        pending_close: None,
        pending_quit: false,
    };
    let Some(handle) = app_state.get::<WorkspaceMutex>() else {
        return empty;
    };
    let ws = handle.lock().unwrap();
    ModalSnap {
        error: ws.notifications.first().cloned(),
        pending_close: ws
            .pending_close
            .filter(|index| *index < ws.docs.len())
            .map(|index| (index, ops::doc_title(&ws.docs[index]))),
        pending_quit: ws.pending_quit,
    }
}

/// Draws at most one modal and applies its button choice. Returns `true` when quit was
/// confirmed ("Discard and quit"), for the caller to run `intents.request_quit()`.
pub fn draw(ctx: &mut GuiCtx) -> bool {
    let snap = snapshot(ctx.app_state);
    let mut choice = None;
    let ui = ctx.ui;
    if let Some(message) = &snap.error {
        ui.open_popup(ERROR_MODAL);
        if let Some(_token) = ui.begin_modal_popup(ERROR_MODAL) {
            ui.text_wrapped(message);
            ui.separator();
            if ui.button("OK") {
                choice = Some(ModalChoice::ErrorOk);
                ui.close_current_popup();
            }
        }
    } else if let Some((_index, title)) = &snap.pending_close {
        ui.open_popup(CLOSE_MODAL);
        if let Some(_token) = ui.begin_modal_popup(CLOSE_MODAL) {
            ui.text_wrapped(format!("\"{title}\" has unsaved changes."));
            if ui.button("Save") {
                choice = Some(ModalChoice::CloseSave);
                ui.close_current_popup();
            }
            ui.same_line();
            if ui.button("Discard") {
                choice = Some(ModalChoice::CloseDiscard);
                ui.close_current_popup();
            }
            ui.same_line();
            if ui.button("Cancel") {
                choice = Some(ModalChoice::CloseCancel);
                ui.close_current_popup();
            }
        }
    } else if snap.pending_quit {
        ui.open_popup(QUIT_MODAL);
        if let Some(_token) = ui.begin_modal_popup(QUIT_MODAL) {
            ui.text_wrapped("There are unsaved changes.");
            if ui.button("Discard and quit") {
                choice = Some(ModalChoice::QuitDiscard);
                ui.close_current_popup();
            }
            ui.same_line();
            if ui.button("Cancel") {
                choice = Some(ModalChoice::QuitCancel);
                ui.close_current_popup();
            }
        }
    }
    apply_choice(ctx, choice)
}

/// Applies a modal button choice with a short workspace lock; the Save As dialog for an
/// untitled doc is requested after the lock is dropped. Returns `true` for "Discard and quit".
fn apply_choice(ctx: &GuiCtx, choice: Option<ModalChoice>) -> bool {
    let Some(choice) = choice else {
        return false;
    };
    let Some(handle) = ctx.app_state.get::<WorkspaceMutex>().cloned() else {
        return false;
    };
    let mut ws = handle.lock().unwrap();
    match choice {
        ModalChoice::ErrorOk => {
            if !ws.notifications.is_empty() {
                ws.notifications.remove(0);
            }
        }
        ModalChoice::CloseSave => {
            let Some(index) = ws.pending_close else {
                return false;
            };
            if index >= ws.docs.len() {
                ws.pending_close = None;
                return false;
            }
            match ws.docs[index].path.clone() {
                Some(path) => match ops::save_doc(&mut ws.docs[index], &path) {
                    Ok(()) => {
                        ops::close_doc(&mut ws, index);
                        ws.pending_close = None;
                    }
                    Err(err) => {
                        // A failure pushes a notification (shown by the error modal first) and
                        // keeps both the tab and the pending close.
                        ws.notifications.push(ops::error_notification(&err));
                    }
                },
                None => {
                    // Untitled doc: hand over to the Save As dialog and keep the tab open.
                    ws.pending_close = None;
                    drop(ws);
                    request_async_dialog(
                        ctx.app_state,
                        AsyncFileDialogRequest::SaveDialogueAs { doc_index: index },
                    );
                }
            }
        }
        ModalChoice::CloseDiscard => {
            if let Some(index) = ws.pending_close
                && index < ws.docs.len()
            {
                ops::close_doc(&mut ws, index);
            }
            ws.pending_close = None;
        }
        ModalChoice::CloseCancel => ws.pending_close = None,
        ModalChoice::QuitDiscard => {
            ws.pending_quit = false;
            return true;
        }
        ModalChoice::QuitCancel => ws.pending_quit = false,
    }
    false
}

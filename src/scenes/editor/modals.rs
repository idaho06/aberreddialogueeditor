//! Modal popups, opened from the frame's top-level scope after the menu scope: the error modal
//! (first entry of `ws.notifications`), the "Unsaved changes" confirm for a pending tab close,
//! and the "Unsaved changes" confirm for a pending quit. Only one shows at a time; the error
//! modal has priority.
//!
//! Locking: the workspace is locked to snapshot the modal state, the guard is dropped while
//! the widgets are drawn, and a button click is applied with a fresh short lock. The Save As
//! dialog request (untitled doc in the close modal) happens after that lock is released.

use aberredengine::prelude::imgui::Ui;
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
#[derive(Default)]
struct ModalSnap {
    /// First queued notification, if any.
    error: Option<String>,
    /// Pending close: tab index plus its title for the message.
    pending_close: Option<(usize, String)>,
    pending_quit: bool,
}

fn snapshot(app_state: &AppState) -> ModalSnap {
    app_state
        .get::<WorkspaceMutex>()
        .map(|handle| {
            let ws = handle.lock().unwrap();
            ModalSnap {
                error: ws.notifications.first().cloned(),
                pending_close: ws
                    .pending_close
                    .filter(|index| *index < ws.docs.len())
                    .map(|index| (index, ops::doc_title(&ws.docs[index]))),
                pending_quit: ws.pending_quit,
            }
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
    if let Some(message) = &snap.error {
        with_modal(ui, ERROR_MODAL, |ui| {
            ui.text_wrapped(message);
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
    }
    apply_choice(ctx.app_state, choice)
}

/// Applies a modal button choice with a short workspace lock; the Save As dialog for an
/// untitled doc is requested after the lock is dropped. Returns `true` for "Discard and quit".
fn apply_choice(app_state: &AppState, choice: Option<ModalChoice>) -> bool {
    let Some(choice) = choice else {
        return false;
    };
    let Some(handle) = app_state.get::<WorkspaceMutex>().cloned() else {
        return false;
    };
    log::debug!("modal choice {choice:?} applied");
    let mut ws = handle.lock().unwrap();
    match choice {
        ModalChoice::ErrorOk => {
            if !ws.notifications.is_empty() {
                ws.notifications.remove(0);
                log::debug!("modal {choice:?}: dismissed the queued error notification");
            }
        }
        ModalChoice::CloseSave => {
            let Some(index) = ws.pending_close.filter(|index| *index < ws.docs.len()) else {
                ws.pending_close = None;
                log::debug!("modal {choice:?}: pending close index out of range, cleared");
                return false;
            };
            match ws.docs[index].path.clone() {
                Some(path) => match ops::save_doc(&mut ws.docs[index], &path) {
                    Ok(()) => {
                        ops::close_doc(&mut ws, index);
                        ws.pending_close = None;
                        log::debug!("modal {choice:?}: saved and closed tab {index}");
                    }
                    Err(err) => {
                        // A failure pushes a notification (shown by the error modal first) and
                        // keeps both the tab and the pending close.
                        ws.notifications.push(ops::error_notification(&err));
                        log::debug!(
                            "modal {choice:?}: save of tab {index} failed, error modal queued"
                        );
                    }
                },
                None => {
                    // Untitled doc: hand over to the Save As dialog and keep the tab open.
                    ws.pending_close = None;
                    drop(ws);
                    log::debug!(
                        "modal {choice:?}: tab {index} is untitled, Save As dialog requested"
                    );
                    request_async_dialog(
                        app_state,
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
            return true;
        }
        ModalChoice::QuitCancel => {
            ws.pending_quit = false;
            log::debug!("modal {choice:?}: quit cancelled");
        }
    }
    false
}

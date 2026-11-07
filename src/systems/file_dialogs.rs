//! Async native file dialogs (rfd) bridged into the logic thread, plus the system that applies the
//! dialog results to the [`Workspace`].
//!
//! The GUI (render thread) calls [`request_async_dialog`]; the rfd future is built on the caller
//! thread, then a worker thread drives it with `futures::executor::block_on` and sends the result
//! over a crossbeam channel stored in `AppState`. [`poll_async_dialogs`] (logic thread) drains the
//! channel once per tick and applies the result through the pure [`apply_result`].

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use aberredengine::prelude::*;
use crossbeam_channel::{Receiver, TryRecvError, unbounded};
use futures::FutureExt;
use rfd::AsyncFileDialog;

use crate::model::{Notification, NotificationKind, Workspace};
use crate::ops;
use crate::state::with_workspace;

/// A file-dialog request from the GUI. Only one dialog can be in flight at a time.
#[derive(Debug)]
pub enum AsyncFileDialogRequest {
    /// "Open dialogue...": pick one `.json` file to open.
    OpenDialogue,
    /// "Save as..." for the doc with this unique, never-reused `doc_id`. Keyed by `doc_id` (not
    /// tab index), so closing or shifting tabs while the picker is open cannot redirect the
    /// save. `close_after`: close the doc after a successful save (the unsaved-changes modal's
    /// Save on an untitled doc).
    SaveDialogueAs { doc_id: u64, close_after: bool },
}

/// The outcome of a finished dialog. Paths stay ABSOLUTE as returned by the dialog.
#[derive(Debug)]
pub enum AsyncFileDialogResult {
    OpenDialogue {
        path: PathBuf,
    },
    /// `doc_id`/`close_after` are carried through from the request; a cancelled picker produces
    /// no result at all.
    SaveDialogueAs {
        doc_id: u64,
        close_after: bool,
        path: PathBuf,
    },
}

/// The boxed dialog future: built on the caller thread, driven on a worker thread.
type DialogTask = Pin<Box<dyn Future<Output = Option<AsyncFileDialogResult>> + Send>>;

/// Shared dialog state: the receiver of the in-flight dialog, if any.
#[derive(Default)]
pub struct AsyncFileDialogState {
    receiver: Option<Receiver<Option<AsyncFileDialogResult>>>,
}

/// Shared handle stored in `AppState` (insert once in `on_setup`); the render thread's cloned
/// `AppState` aliases the same `Arc`.
pub type AsyncFileDialogMutex = Arc<Mutex<AsyncFileDialogState>>;

/// A native dialog filtered to dialogue JSON files.
fn dialogue_json_dialog() -> AsyncFileDialog {
    AsyncFileDialog::new().add_filter("Dialogue JSON", &["json"])
}

/// Opens a native file dialog on a worker thread. If a dialog is already in flight (a receiver
/// exists), the request is ignored; if `AsyncFileDialogMutex` is missing from `app_state`, warns
/// and returns.
pub fn request_async_dialog(app_state: &AppState, request: AsyncFileDialogRequest) {
    let Some(handle) = app_state.get::<AsyncFileDialogMutex>() else {
        log::warn!("file dialog requested but AsyncFileDialogMutex is missing from AppState");
        return;
    };
    let mut state = handle.lock().unwrap();
    if state.receiver.is_some() {
        log::debug!("a file dialog is already in flight; ignoring the new request");
        return;
    }
    // Build the rfd future on THIS (caller) thread; only block_on runs on the worker.
    let task: DialogTask = match request {
        AsyncFileDialogRequest::OpenDialogue => {
            Box::pin(dialogue_json_dialog().pick_file().map(|handle| {
                handle.map(|file| AsyncFileDialogResult::OpenDialogue {
                    path: file.path().to_path_buf(),
                })
            }))
        }
        AsyncFileDialogRequest::SaveDialogueAs {
            doc_id,
            close_after,
        } => Box::pin(
            dialogue_json_dialog()
                .set_file_name("dialogue.json")
                .save_file()
                .map(move |handle| {
                    handle.map(|file| AsyncFileDialogResult::SaveDialogueAs {
                        doc_id,
                        close_after,
                        path: file.path().to_path_buf(),
                    })
                }),
        ),
    };
    let (sender, receiver) = unbounded();
    std::thread::spawn(move || {
        let result = futures::executor::block_on(task);
        if sender.send(result).is_err() {
            log::debug!("file dialog finished after the receiver was dropped");
        }
    });
    state.receiver = Some(receiver);
    log::debug!("file dialog started: {request:?}");
}

/// True while a native dialog is in flight (a receiver exists). The GUI reads this to suspend
/// tab closes while a dialog is open (one dialog at a time). The pending Save As itself is keyed
/// by `doc_id`, so a tab closed meanwhile can no longer redirect it — the skip stays so a close
/// and a dialog result never interleave. Open/Save As requests are already ignored by
/// [`request_async_dialog`] while a dialog is in flight.
pub fn dialog_in_flight(app_state: &AppState) -> bool {
    app_state
        .get::<AsyncFileDialogMutex>()
        .is_some_and(|handle| handle.lock().unwrap().receiver.is_some())
}

/// Drains one completed dialog from `state`, if any: `Ok(result)` (Some = picked, None = cancelled)
/// clears the receiver and returns the result; `Empty` returns None and keeps the receiver;
/// `Disconnected` warns, clears the receiver and returns None. Lock-free so tests can inject a
/// channel.
fn drain_completed(state: &mut AsyncFileDialogState) -> Option<AsyncFileDialogResult> {
    let receiver = state.receiver.as_ref()?;
    match receiver.try_recv() {
        Ok(result) => {
            state.receiver = None;
            match &result {
                Some(AsyncFileDialogResult::OpenDialogue { path }) => {
                    log::debug!("file dialog completed: open {path:?}");
                }
                Some(AsyncFileDialogResult::SaveDialogueAs { doc_id, path, .. }) => {
                    log::debug!("file dialog completed: save as doc {doc_id} to {path:?}");
                }
                None => log::debug!("file dialog cancelled by the user"),
            }
            result
        }
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => {
            log::warn!("the file dialog worker thread disconnected without sending a result");
            state.receiver = None;
            None
        }
    }
}

/// Applies a finished dialog's result to `ws`. PURE (no ECS): every failure becomes one entry in
/// `ws.notifications` for the GUI's error modal.
pub fn apply_result(ws: &mut Workspace, result: AsyncFileDialogResult) {
    match result {
        AsyncFileDialogResult::OpenDialogue { path } => {
            log::debug!("applying dialog result: open {path:?}");
            // Load warnings are shown in the Nodes panel, not as notifications: ignore them.
            if let Err(err) = ops::open_path(ws, &path) {
                log::warn!("dialog result open failed, error notification pushed: {err}");
                ws.notifications.push(Notification {
                    kind: NotificationKind::Error,
                    message: err.to_string(),
                });
            }
        }
        AsyncFileDialogResult::SaveDialogueAs {
            doc_id,
            close_after,
            path,
        } => {
            log::debug!("applying dialog result: save as doc {doc_id} to {path:?}");
            // The pending Save As is keyed by the doc's unique, never-reused doc_id: look up the
            // CURRENT index, so tabs closed or shifted while the picker was open cannot
            // redirect the save. A doc that is no longer open is ignored silently (one debug
            // log, no notification, nothing written).
            let Some(index) = ws.docs.iter().position(|d| d.doc_id == doc_id) else {
                log::debug!(
                    "dialog result save as for doc {doc_id} ignored: the doc is no longer open"
                );
                return;
            };
            // The engine's own message goes to the modal without the "invalid dialogue: " prefix.
            if let Err(err) = ops::save_doc_at(ws, index, &path) {
                let note = ops::error_notification(&err);
                // Log the kind only: a SaveRejected message carries the engine's error text,
                // which quotes node ids (document content) and must not reach the log.
                log::warn!(
                    "dialog result save as failed, notification pushed: kind={:?}",
                    note.kind
                );
                ws.notifications.push(note);
                // On any error the document stays open and the close intent is dropped.
                return;
            }
            if close_after {
                // save_doc_at does not reorder the docs, so the index is still valid.
                ops::close_doc(ws, index);
                log::debug!("dialog result save as closed doc {doc_id} after the successful save");
            }
        }
    }
}

/// Logic system, once per tick: drains a finished file dialog (if any) and applies it to the
/// workspace. Requests ONLY `Res<AppState>` (never `ResMut`, never twice — B0002), and never holds
/// one lock while taking the other. Cheap when idle.
pub fn poll_async_dialogs(app_state: Res<AppState>) {
    let Some(dialog_handle) = app_state.get::<AsyncFileDialogMutex>().cloned() else {
        return;
    };
    let result = {
        let mut state = dialog_handle.lock().unwrap();
        drain_completed(&mut state)
    };
    let Some(result) = result else {
        return;
    };
    // One short workspace lock for the apply; warns when the handle is missing.
    if with_workspace(&app_state, |ws| apply_result(ws, result)).is_none() {
        log::warn!("a file dialog finished but WorkspaceMutex is missing from AppState");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{TempDir, fixture};

    fn open_guard_result() -> AsyncFileDialogResult {
        AsyncFileDialogResult::OpenDialogue {
            path: fixture("guard.json"),
        }
    }

    fn save_as_result(doc_id: u64, close_after: bool, path: PathBuf) -> AsyncFileDialogResult {
        AsyncFileDialogResult::SaveDialogueAs {
            doc_id,
            close_after,
            path,
        }
    }

    /// State with `rx` as the in-flight dialog's receiver.
    fn pending(rx: Receiver<Option<AsyncFileDialogResult>>) -> AsyncFileDialogState {
        AsyncFileDialogState { receiver: Some(rx) }
    }

    #[test]
    fn dialog_in_flight_reflects_the_receiver_presence() {
        let mut app_state = AppState::default();
        assert!(
            !dialog_in_flight(&app_state),
            "no handle inserted: nothing in flight"
        );
        let (tx, rx) = unbounded();
        let handle: AsyncFileDialogMutex = Arc::new(Mutex::new(pending(rx)));
        app_state.insert(handle);
        assert!(
            dialog_in_flight(&app_state),
            "a receiver means a dialog is open"
        );
        drop(tx);
        let handle: AsyncFileDialogMutex = Arc::new(Mutex::new(AsyncFileDialogState::default()));
        app_state.insert(handle);
        assert!(!dialog_in_flight(&app_state), "no receiver: idle again");
    }

    #[test]
    fn drain_completed_returns_the_result_and_clears_the_receiver() {
        let (tx, rx) = unbounded();
        let mut state = pending(rx);
        tx.send(Some(open_guard_result())).unwrap();
        let drained = drain_completed(&mut state);
        assert!(
            matches!(drained, Some(AsyncFileDialogResult::OpenDialogue { .. })),
            "the picked result is returned"
        );
        assert!(
            state.receiver.is_none(),
            "a finished dialog clears the receiver"
        );
    }

    #[test]
    fn drain_completed_cancel_clears_the_receiver_and_returns_none() {
        let (tx, rx) = unbounded();
        let mut state = pending(rx);
        tx.send(None).unwrap(); // cancelled
        assert!(drain_completed(&mut state).is_none());
        assert!(state.receiver.is_none(), "a cancelled dialog is done too");
    }

    #[test]
    fn drain_completed_empty_keeps_the_receiver() {
        let (_tx, rx) = unbounded();
        let mut state = pending(rx);
        assert!(drain_completed(&mut state).is_none());
        assert!(
            state.receiver.is_some(),
            "an in-flight dialog keeps its receiver"
        );
    }

    #[test]
    fn drain_completed_disconnected_clears_the_receiver() {
        let (tx, rx) = unbounded();
        drop(tx);
        let mut state = pending(rx);
        assert!(drain_completed(&mut state).is_none());
        assert!(state.receiver.is_none());
    }

    #[test]
    fn drain_completed_without_a_receiver_is_none() {
        let mut state = AsyncFileDialogState::default();
        assert!(drain_completed(&mut state).is_none());
    }

    #[test]
    fn apply_open_adds_a_doc_without_notifications() {
        let mut ws = Workspace::default();
        apply_result(&mut ws, open_guard_result());
        assert_eq!(ws.docs.len(), 1, "the fixture opens as one doc");
        assert_eq!(ws.active, 0);
        assert!(ws.notifications.is_empty());
        assert_eq!(
            ws.docs[0].path,
            Some(fixture("guard.json").canonicalize().unwrap()),
            "the doc stores the canonical path"
        );
    }

    #[test]
    fn apply_open_of_a_missing_file_pushes_a_notification_and_adds_no_doc() {
        let tmp = TempDir::new("dialogs", "open-missing");
        let mut ws = Workspace::default();
        apply_result(
            &mut ws,
            AsyncFileDialogResult::OpenDialogue {
                path: tmp.join("nope.json"),
            },
        );
        assert!(ws.docs.is_empty(), "a failed open adds no doc");
        assert_eq!(ws.notifications.len(), 1);
        let note = &ws.notifications[0];
        assert_eq!(
            note.kind,
            NotificationKind::Error,
            "an open failure is a plain error"
        );
        assert!(
            note.message.contains("nope.json"),
            "the notification names the path: {}",
            note.message
        );
    }

    #[test]
    fn apply_save_as_of_an_invalid_doc_pushes_the_engine_message_and_writes_nothing() {
        let tmp = TempDir::new("dialogs", "save-invalid");
        let mut ws = Workspace::default();
        ops::open_path(&mut ws, &fixture("invalid_dangling.json"))
            .expect("invalid fixtures still open");
        let doc_id = ws.docs[0].doc_id;
        let out = tmp.join("out.json");
        // close_after = true: the gate rejection drops the close intent, the doc stays open.
        apply_result(&mut ws, save_as_result(doc_id, true, out.clone()));
        assert_eq!(ws.notifications.len(), 1);
        let note = &ws.notifications[0];
        assert_eq!(
            note.kind,
            NotificationKind::SaveRejected,
            "an engine-gate rejection from Save As is a SaveRejected notification"
        );
        assert!(
            note.message
                .contains("dialogue node \"a\" links to unknown node \"ghost\""),
            "the notification is the engine's own message: {}",
            note.message
        );
        assert!(
            !note.message.starts_with("invalid dialogue"),
            "no \"invalid dialogue: \" prefix: {}",
            note.message
        );
        assert!(!out.exists(), "a gated save writes no file");
        assert_eq!(ws.docs.len(), 1, "a failed save keeps the doc open");
        assert!(
            ws.docs[0].path.is_none()
                || ws.docs[0].path
                    == Some(fixture("invalid_dangling.json").canonicalize().unwrap()),
            "the rejected save leaves the doc's path alone"
        );
    }

    #[test]
    fn apply_save_as_success_without_close_after_marks_clean_sets_the_path_and_keeps_the_tab() {
        let tmp = TempDir::new("dialogs", "save-ok");
        let mut ws = Workspace::default();
        ops::open_path(&mut ws, &fixture("guard.json")).unwrap();
        let doc_id = ws.docs[0].doc_id;
        let doc = ws.active_doc_mut().unwrap();
        doc.rename_node(doc.nodes[0].key, "renamed".to_owned());
        assert!(doc.dirty, "renaming dirties the doc");

        let out = tmp.join("saved.json");
        apply_result(&mut ws, save_as_result(doc_id, false, out.clone()));
        assert!(ws.notifications.is_empty(), "a good save is silent");
        assert_eq!(ws.docs.len(), 1, "close_after = false keeps the tab open");
        let doc = ws.active_doc().unwrap();
        assert!(!doc.dirty, "save-as marks the doc clean");
        assert_eq!(doc.path, Some(out.canonicalize().unwrap()));
        assert!(out.exists(), "the file was written");
    }

    #[test]
    fn apply_save_as_onto_another_tabs_path_pushes_a_notification_and_keeps_the_doc_open() {
        let mut ws = Workspace::default();
        ops::open_path(&mut ws, &fixture("guard.json")).unwrap(); // doc 0
        ops::open_path(&mut ws, &fixture("loops.json")).unwrap(); // doc 1, active
        let doc_id = ws.docs[1].doc_id;
        // close_after = true: the AlreadyOpen rejection drops the close intent.
        apply_result(&mut ws, save_as_result(doc_id, true, fixture("guard.json")));
        assert_eq!(ws.notifications.len(), 1);
        let note = &ws.notifications[0];
        assert_eq!(
            note.kind,
            NotificationKind::Error,
            "a tab collision is a plain error, not a save rejection"
        );
        assert!(
            note.message.contains("already open in another tab"),
            "the notification explains the collision: {}",
            note.message
        );
        assert_eq!(ws.docs.len(), 2, "no doc is added or removed");
        assert_eq!(
            ws.docs[1].path,
            Some(fixture("loops.json").canonicalize().unwrap()),
            "the rejected save-as leaves the doc's path alone"
        );
    }

    #[test]
    fn apply_save_as_is_keyed_by_doc_id_and_survives_tabs_closed_while_the_picker_was_open() {
        let tmp = TempDir::new("dialogs", "save-by-doc-id-shift");
        let mut ws = Workspace::default();
        // Tabs [A, B, C] with distinct content (one node each, renamed to a/b/c).
        for name in ["a", "b", "c"] {
            ops::new_doc(&mut ws);
            let doc = ws.active_doc_mut().unwrap();
            let key = doc.add_node(Vec2::ZERO);
            doc.rename_node(key, name.to_owned());
        }
        assert_eq!(ws.docs.len(), 3);
        let b_id = ws.docs[1].doc_id;
        let b_def = crate::model::to_def(&ws.docs[1]);

        // The Save As picker was requested for B (doc_id), then tab A is closed while the
        // picker is open: [A, B, C] -> [B, C]. The result must still save B, not C.
        ops::close_doc(&mut ws, 0);
        assert_eq!(ws.docs.len(), 2);
        assert_eq!(ws.docs[0].doc_id, b_id, "B shifted to index 0");

        let out = tmp.join("picked.json");
        apply_result(&mut ws, save_as_result(b_id, false, out.clone()));
        assert!(ws.notifications.is_empty(), "a good save is silent");

        // The file at the picked path holds B's content (compared via to_def/load).
        let mut ws2 = Workspace::default();
        ops::open_path(&mut ws2, &out).expect("the saved file reopens");
        assert_eq!(
            crate::model::to_def(ws2.active_doc().unwrap()),
            b_def,
            "the picked path holds B's content, not C's"
        );

        // B is clean with its path set; C is still dirty and unsaved.
        let b = &ws.docs[0];
        assert_eq!(b.doc_id, b_id);
        assert!(!b.dirty, "B was saved and marked clean");
        assert_eq!(b.path, Some(out.canonicalize().unwrap()));
        let c = &ws.docs[1];
        assert_ne!(c.doc_id, b_id);
        assert!(c.dirty, "C was not written and stays dirty");
        assert!(c.path.is_none(), "C stays untitled/unsaved");
        assert_eq!(ws.active, 1, "active was C and stays valid");
    }

    #[test]
    fn apply_save_as_for_a_doc_closed_while_the_picker_was_open_writes_nothing_and_pushes_nothing()
    {
        let tmp = TempDir::new("dialogs", "save-doc-gone");
        let mut ws = Workspace::default();
        ops::new_doc(&mut ws);
        let doc_id = {
            let doc = ws.active_doc_mut().unwrap();
            let key = doc.add_node(Vec2::ZERO);
            doc.rename_node(key, "gone".to_owned());
            doc.doc_id
        };
        ops::close_doc(&mut ws, 0); // the doc was closed/discarded while the picker was open
        assert!(ws.docs.is_empty());

        let out = tmp.join("picked.json");
        apply_result(&mut ws, save_as_result(doc_id, true, out.clone()));
        assert!(
            ws.notifications.is_empty(),
            "a vanished doc gets no notification"
        );
        assert!(!out.exists(), "nothing is written");
        assert!(ws.docs.is_empty(), "no doc is resurrected");
    }

    #[test]
    fn apply_save_as_with_close_after_closes_the_doc_on_success_and_keeps_active_valid() {
        let tmp = TempDir::new("dialogs", "save-close-after");
        let mut ws = Workspace::default();
        ops::new_doc(&mut ws); // doc A, id 1
        ops::new_doc(&mut ws); // doc B, id 2, active
        let b_id = ws.docs[1].doc_id;
        ws.active_doc_mut().unwrap().add_node(Vec2::ZERO); // B valid + dirty

        let out = tmp.join("b.json");
        apply_result(&mut ws, save_as_result(b_id, true, out.clone()));
        assert!(ws.notifications.is_empty(), "a good save is silent");
        assert_eq!(ws.docs.len(), 1, "the saved doc was closed");
        assert_eq!(ws.docs[0].doc_id, 1, "A survived");
        assert!(ws.active < ws.docs.len(), "active stays valid");
        assert_eq!(ws.active, 0);
        assert!(out.exists(), "the file was written");

        // close_after on the ACTIVE doc: active stays valid too.
        let a_id = ws.docs[0].doc_id;
        ws.active_doc_mut().unwrap().add_node(Vec2::ZERO);
        let out2 = tmp.join("a.json");
        apply_result(&mut ws, save_as_result(a_id, true, out2.clone()));
        assert!(ws.notifications.is_empty());
        assert!(ws.docs.is_empty(), "the last doc closed after its save");
        assert!(
            ws.active_doc().is_none(),
            "an empty workspace has no active doc"
        );
        assert!(out2.exists());
    }
}

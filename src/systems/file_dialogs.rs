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

use crate::model::Workspace;
use crate::ops;
use crate::state::WorkspaceMutex;

/// A file-dialog request from the GUI. Only one dialog can be in flight at a time.
#[derive(Debug)]
pub enum AsyncFileDialogRequest {
    /// "Open dialogue...": pick one `.json` file to open.
    OpenDialogue,
    /// "Save as..." for the doc at `doc_index`.
    SaveDialogueAs { doc_index: usize },
}

/// The outcome of a finished dialog. Paths stay ABSOLUTE as returned by the dialog.
#[derive(Debug)]
pub enum AsyncFileDialogResult {
    OpenDialogue { path: PathBuf },
    SaveDialogueAs { doc_index: usize, path: PathBuf },
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
        AsyncFileDialogRequest::SaveDialogueAs { doc_index } => Box::pin(
            dialogue_json_dialog()
                .set_file_name("dialogue.json")
                .save_file()
                .map(move |handle| {
                    handle.map(|file| AsyncFileDialogResult::SaveDialogueAs {
                        doc_index,
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
/// tab closes: a pending Save As result is applied by tab index, so closing a tab meanwhile
/// would shift the indices. Open/Save As requests are already ignored by
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
                Some(AsyncFileDialogResult::SaveDialogueAs { doc_index, path }) => {
                    log::debug!("file dialog completed: save as doc {doc_index} to {path:?}");
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
                log::warn!("dialog result open failed, notification pushed: {err}");
                ws.notifications.push(err.to_string());
            }
        }
        AsyncFileDialogResult::SaveDialogueAs { doc_index, path } => {
            log::debug!("applying dialog result: save as doc {doc_index} to {path:?}");
            // The engine's own message goes to the modal without the "invalid dialogue: " prefix.
            if let Err(err) = ops::save_doc_at(ws, doc_index, &path) {
                let note = ops::error_notification(&err);
                log::warn!("dialog result save as failed, notification pushed: {note}");
                ws.notifications.push(note);
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
    let Some(ws_handle) = app_state.get::<WorkspaceMutex>().cloned() else {
        log::warn!("a file dialog finished but WorkspaceMutex is missing from AppState");
        return;
    };
    let mut ws = ws_handle.lock().unwrap();
    apply_result(&mut ws, result);
}

/// Drops the in-flight dialog's receiver (the worker's send then fails and is only logged).
#[allow(dead_code)] // reserved for scene-exit cleanup in a later plan step
pub fn clear_async_dialog(app_state: &AppState) {
    let Some(handle) = app_state.get::<AsyncFileDialogMutex>() else {
        return;
    };
    handle.lock().unwrap().receiver = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenes::editor::testkit::{TempDir, fixture};

    fn open_guard_result() -> AsyncFileDialogResult {
        AsyncFileDialogResult::OpenDialogue {
            path: fixture("guard.json"),
        }
    }

    fn save_as_result(doc_index: usize, path: PathBuf) -> AsyncFileDialogResult {
        AsyncFileDialogResult::SaveDialogueAs { doc_index, path }
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
        assert!(
            ws.notifications[0].contains("nope.json"),
            "the notification names the path: {}",
            ws.notifications[0]
        );
    }

    #[test]
    fn apply_save_as_of_an_invalid_doc_pushes_the_engine_message_and_writes_nothing() {
        let tmp = TempDir::new("dialogs", "save-invalid");
        let mut ws = Workspace::default();
        ops::open_path(&mut ws, &fixture("invalid_dangling.json"))
            .expect("invalid fixtures still open");
        let out = tmp.join("out.json");
        apply_result(&mut ws, save_as_result(0, out.clone()));
        assert_eq!(ws.notifications.len(), 1);
        let note = &ws.notifications[0];
        assert!(
            note.contains("dialogue node \"a\" links to unknown node \"ghost\""),
            "the notification is the engine's own message: {note}"
        );
        assert!(
            !note.starts_with("invalid dialogue"),
            "no \"invalid dialogue: \" prefix: {note}"
        );
        assert!(!out.exists(), "a gated save writes no file");
    }

    #[test]
    fn apply_save_as_success_marks_clean_and_sets_the_path() {
        let tmp = TempDir::new("dialogs", "save-ok");
        let mut ws = Workspace::default();
        ops::open_path(&mut ws, &fixture("guard.json")).unwrap();
        let doc = ws.active_doc_mut().unwrap();
        doc.rename_node(doc.nodes[0].key, "renamed".to_owned());
        assert!(doc.dirty, "renaming dirties the doc");

        let out = tmp.join("saved.json");
        apply_result(&mut ws, save_as_result(0, out.clone()));
        assert!(ws.notifications.is_empty(), "a good save is silent");
        let doc = ws.active_doc().unwrap();
        assert!(!doc.dirty, "save-as marks the doc clean");
        assert_eq!(doc.path, Some(out.canonicalize().unwrap()));
        assert!(out.exists(), "the file was written");
    }

    #[test]
    fn apply_save_as_onto_another_tabs_path_pushes_a_notification() {
        let mut ws = Workspace::default();
        ops::open_path(&mut ws, &fixture("guard.json")).unwrap(); // doc 0
        ops::open_path(&mut ws, &fixture("loops.json")).unwrap(); // doc 1, active
        apply_result(&mut ws, save_as_result(1, fixture("guard.json")));
        assert_eq!(ws.notifications.len(), 1);
        assert!(
            ws.notifications[0].contains("already open in another tab"),
            "the notification explains the collision: {}",
            ws.notifications[0]
        );
        assert_eq!(ws.docs.len(), 2, "no doc is added or removed");
        assert_eq!(
            ws.docs[1].path,
            Some(fixture("loops.json").canonicalize().unwrap()),
            "the rejected save-as leaves the doc's path alone"
        );
    }
}

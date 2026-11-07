use std::sync::atomic::Ordering;

use aberredengine::prelude::*;

use super::{
    actions, context_menu, inspector, menu, modals, nodes, problems, shortcuts, status, tabs,
};
use crate::state::{UiCaptureHandle, with_workspace};
use crate::systems::file_dialogs::{dialog_in_flight, request_async_dialog};

/// Scene GUI callback (render thread). Publishes imgui's capture flags into the shared `UiCapture`
/// every frame so logic-side input systems can ignore mouse/keyboard while the GUI wants them,
/// then draws the UI: menu, shortcuts, tab strip, Nodes, Inspector and Problems windows, the
/// status bar, the graph context menu and modals. Menu/shortcut/tab/context-menu/problem-row
/// intents are collected as [`actions::EditorAction`]s and applied in one place afterwards, under
/// the lock discipline documented in [`actions`]: `apply_action` runs with `app_state: None`
/// under the workspace lock, and the returned dialog request is dispatched after the guard drops.
pub fn gui(ctx: &mut GuiCtx) {
    if let Some(capture) = ctx.app_state.get::<UiCaptureHandle>() {
        capture
            .mouse
            .store(ctx.ui.io().want_capture_mouse, Ordering::Relaxed);
        capture
            .keyboard
            .store(ctx.ui.io().want_capture_keyboard, Ordering::Relaxed);
    }

    // Brief snapshot for the menu/shortcut enabled states, the Problems check mark and the modal
    // gate; the guard is dropped immediately.
    let (active, problems_visible, has_modal) = with_workspace(ctx.app_state, |ws| {
        (
            ws.active_doc().is_some().then_some(ws.active),
            !ws.problems_hidden,
            ws.has_modal(),
        )
    })
    .unwrap_or((None, true, false));

    let mut action_queue = Vec::new();
    menu::draw(ctx, active, problems_visible, &mut action_queue);
    // imgui modals block mouse input to other windows, but `ui.is_key_pressed` is io-level and
    // still fires while a modal is open, so shortcuts are NOT polled on a modal frame — otherwise
    // e.g. Ctrl+W or Ctrl+Shift+S would act behind the unsaved-changes confirm. Menu, tabs,
    // windows and the modals themselves are drawn exactly as before, in the same order.
    if !has_modal {
        shortcuts::poll(ctx, active, &mut action_queue);
    }
    tabs::draw(ctx, &mut action_queue);
    nodes::draw(ctx);
    inspector::draw(ctx);
    problems::draw(ctx, &mut action_queue);
    status::draw(ctx);
    context_menu::draw(ctx, &mut action_queue);
    let modal_quit = modals::draw(ctx);

    if action_queue.is_empty() && !modal_quit {
        return;
    }
    // While a native dialog is in flight, tab closes are ignored: one dialog at a time, so a
    // close and a dialog result never interleave. (The pending Save As itself is keyed by the
    // doc's doc_id, so a tab closed meanwhile could no longer redirect the save — the skip
    // stays.) Open/Save As requests are already ignored inside `request_async_dialog`.
    let dialog_busy = dialog_in_flight(ctx.app_state);
    for action in action_queue {
        if dialog_busy && matches!(action, actions::EditorAction::CloseTab(_)) {
            continue;
        }
        // One short lock per action; the returned dialog request is dispatched after the
        // guard drops.
        if let Some(fx) =
            with_workspace(ctx.app_state, |ws| actions::apply_action(ws, action, None))
        {
            if fx.quit {
                ctx.intents.request_quit();
            }
            if let Some(request) = fx.dialog {
                request_async_dialog(ctx.app_state, request);
            }
        }
    }
    if modal_quit {
        ctx.intents.request_quit();
    }
}

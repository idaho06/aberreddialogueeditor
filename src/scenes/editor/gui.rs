use std::sync::atomic::Ordering;

use aberredengine::prelude::*;

use super::{actions, context_menu, menu, modals, nodes, shortcuts, tabs};
use crate::state::{UiCaptureHandle, WorkspaceMutex};
use crate::systems::file_dialogs::{dialog_in_flight, request_async_dialog};

/// Scene GUI callback (render thread). Publishes imgui's capture flags into the shared `UiCapture`
/// every frame so logic-side input systems can ignore mouse/keyboard while the GUI wants them,
/// then draws the UI: menu, shortcuts, tab strip, Nodes window, the graph context menu and
/// modals. Menu/shortcut/tab/context-menu intents are collected as [`actions::EditorAction`]s and
/// applied in one place afterwards, under
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

    // Brief snapshot for the menu/shortcut enabled states; the guard is dropped immediately.
    let active = ctx.app_state.get::<WorkspaceMutex>().and_then(|handle| {
        let ws = handle.lock().unwrap();
        ws.active_doc().is_some().then_some(ws.active)
    });

    let mut action_queue = Vec::new();
    menu::draw(ctx, active, &mut action_queue);
    shortcuts::poll(ctx, active, &mut action_queue);
    tabs::draw(ctx, &mut action_queue);
    nodes::draw(ctx);
    context_menu::draw(ctx, &mut action_queue);
    let modal_quit = modals::draw(ctx);

    if action_queue.is_empty() && !modal_quit {
        return;
    }
    // While a native dialog is in flight, tab closes are ignored: a pending Save As result is
    // applied by tab index, and closing a tab would shift the indices. Open/Save As requests
    // are already ignored inside `request_async_dialog`.
    let dialog_busy = dialog_in_flight(ctx.app_state);
    if let Some(handle) = ctx.app_state.get::<WorkspaceMutex>().cloned() {
        for action in action_queue {
            if dialog_busy && matches!(action, actions::EditorAction::CloseTab(_)) {
                continue;
            }
            let fx = {
                let mut ws = handle.lock().unwrap();
                actions::apply_action(&mut ws, action, None)
            };
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

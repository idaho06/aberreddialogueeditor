//! The graph canvas context menu (opened by a right-click without dragging). The logic-side
//! input system stores a [`ContextMenuRequest`] in `Workspace.pending_context_menu`; this
//! module drains it, opens the popup once per request and renders the items. It runs from the
//! frame's top-level scope (NOT inside a window closure), after `nodes::draw` and before
//! `modals::draw`.
//!
//! Locking: the workspace is locked only to `take()` the pending request; the guard is dropped
//! before any imgui call.
//!
//! The GUI callback is a plain `fn` pointer (no captures), so the request being shown lives in
//! the `PENDING` thread-local: `ui.open_popup` must be called exactly ONCE per taken request,
//! while `ui.popup` renders the stored request's items every frame the popup is open. The stored
//! request is cleared once the popup is no longer open.

use std::cell::Cell;

use aberredengine::prelude::*;

use super::actions::EditorAction;
use crate::model::ContextMenuRequest;
use crate::state::with_workspace;

/// imgui id of the context-menu popup.
const POPUP_ID: &str = "graph_ctx";

thread_local! {
    /// The request currently shown by the popup; `None` while no popup is open.
    static PENDING: Cell<Option<ContextMenuRequest>> = const { Cell::new(None) };
}

/// The context-menu items for a request, in a stable order and without separators (the renderer
/// draws one before the last item when there is more than one).
pub fn items_for(request: ContextMenuRequest) -> Vec<(&'static str, EditorAction)> {
    match request.node {
        Some(key) => vec![
            ("Set as start", EditorAction::SetStart(key)),
            ("Delete node", EditorAction::DeleteNode(key)),
            ("Add node here", EditorAction::AddNodeAt(request.world)),
        ],
        None => vec![("Add node here", EditorAction::AddNodeAt(request.world))],
    }
}

/// Drains `Workspace.pending_context_menu` (short lock, dropped before any imgui call), opens
/// the popup once per request and renders its items. Clicking an item pushes the
/// [`EditorAction`] and closes the popup; the queued actions are applied by
/// [`crate::scenes::editor::gui`] afterwards.
pub fn draw(ctx: &mut GuiCtx, actions: &mut Vec<EditorAction>) {
    let request = with_workspace(ctx.app_state, |ws| ws.pending_context_menu.take()).flatten();
    if let Some(request) = request {
        PENDING.with(|pending| pending.set(Some(request)));
        ctx.ui.open_popup(POPUP_ID);
    }

    let ui = ctx.ui;
    let mut open = false;
    ui.popup(POPUP_ID, || {
        open = true;
        if let Some(request) = PENDING.with(|pending| pending.get()) {
            let items = items_for(request);
            let multi = items.len() > 1;
            for (index, (label, action)) in items.iter().enumerate() {
                // One separator before the last item when there is more than one item.
                if multi && index + 1 == items.len() {
                    ui.separator();
                }
                if ui.menu_item(label) {
                    actions.push(*action);
                    ui.close_current_popup();
                }
            }
        }
    });
    if !open {
        // The popup is not open any more: forget the stored request.
        PENDING.with(|pending| pending.set(None));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::NodeKey;
    use aberredengine::prelude::Vec2;

    #[test]
    fn node_hit_gives_set_start_delete_node_and_add_node_here_in_order() {
        let key = NodeKey(7);
        let world = Vec2::new(3.0, -4.0);
        let items = items_for(ContextMenuRequest {
            world,
            node: Some(key),
        });
        assert_eq!(
            items,
            vec![
                ("Set as start", EditorAction::SetStart(key)),
                ("Delete node", EditorAction::DeleteNode(key)),
                ("Add node here", EditorAction::AddNodeAt(world)),
            ]
        );
    }

    #[test]
    fn empty_hit_gives_exactly_add_node_here() {
        let world = Vec2::new(1.0, 2.0);
        assert_eq!(
            items_for(ContextMenuRequest { world, node: None }),
            vec![("Add node here", EditorAction::AddNodeAt(world))]
        );
    }
}

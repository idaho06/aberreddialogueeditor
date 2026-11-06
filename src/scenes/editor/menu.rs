//! The main menu bar. File-menu items produce [`EditorAction`]s (applied later by
//! [`crate::scenes::editor::gui`]); Edit/View stay disabled placeholders.

use aberredengine::prelude::*;

use super::actions::EditorAction;

/// Draws the main menu bar. `active` is the workspace's active tab index when a doc is open.
/// Menu clicks are pushed into `actions`; they are applied after the menu scope ends.
pub fn draw(ctx: &mut GuiCtx, active: Option<usize>, actions: &mut Vec<EditorAction>) {
    let ui = ctx.ui;
    let Some(_bar) = ui.begin_main_menu_bar() else {
        return;
    };
    ui.menu("File", || {
        if ui.menu_item_config("New").shortcut("Ctrl+N").build() {
            actions.push(EditorAction::New);
        }
        if ui.menu_item_config("Open...").shortcut("Ctrl+O").build() {
            actions.push(EditorAction::Open);
        }
        let has_active = active.is_some();
        if ui
            .menu_item_config("Save")
            .shortcut("Ctrl+S")
            .enabled(has_active)
            .build()
        {
            actions.push(EditorAction::Save);
        }
        if ui
            .menu_item_config("Save As...")
            .shortcut("Ctrl+Shift+S")
            .enabled(has_active)
            .build()
        {
            actions.push(EditorAction::SaveAs);
        }
        if ui
            .menu_item_config("Close Tab")
            .shortcut("Ctrl+W")
            .enabled(has_active)
            .build()
            && let Some(index) = active
        {
            actions.push(EditorAction::CloseTab(index));
        }
        ui.separator();
        if ui.menu_item_config("Quit").shortcut("Ctrl+Q").build() {
            actions.push(EditorAction::Quit);
        }
    });
    ui.menu("Edit", || {
        ui.menu_item_config("Add node").enabled(false).build();
    });
    ui.menu("View", || {
        ui.menu_item_config("Re-layout").enabled(false).build();
        ui.menu_item_config("Frame all").enabled(false).build();
    });
}

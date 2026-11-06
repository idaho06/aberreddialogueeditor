//! The main menu bar. File-menu items produce [`EditorAction`]s (applied later by
//! [`crate::scenes::editor::gui`]); Edit/View stay disabled placeholders.

use aberredengine::prelude::imgui::Ui;
use aberredengine::prelude::*;

use super::actions::EditorAction;

/// One menu item with a shortcut display; returns true when clicked.
fn item(ui: &Ui, label: &str, shortcut: &str, enabled: bool) -> bool {
    ui.menu_item_config(label)
        .shortcut(shortcut)
        .enabled(enabled)
        .build()
}

/// Draws the main menu bar. `active` is the workspace's active tab index when a doc is open.
/// Menu clicks are pushed into `actions`; they are applied after the menu scope ends.
pub fn draw(ctx: &mut GuiCtx, active: Option<usize>, actions: &mut Vec<EditorAction>) {
    let ui = ctx.ui;
    let Some(_bar) = ui.begin_main_menu_bar() else {
        return;
    };
    ui.menu("File", || {
        if item(ui, "New", "Ctrl+N", true) {
            actions.push(EditorAction::New);
        }
        if item(ui, "Open...", "Ctrl+O", true) {
            actions.push(EditorAction::Open);
        }
        let has_active = active.is_some();
        if item(ui, "Save", "Ctrl+S", has_active) {
            actions.push(EditorAction::Save);
        }
        if item(ui, "Save As...", "Ctrl+Shift+S", has_active) {
            actions.push(EditorAction::SaveAs);
        }
        if item(ui, "Close Tab", "Ctrl+W", has_active)
            && let Some(index) = active
        {
            actions.push(EditorAction::CloseTab(index));
        }
        ui.separator();
        if item(ui, "Quit", "Ctrl+Q", true) {
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

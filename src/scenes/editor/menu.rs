//! The main menu bar. Menu items produce [`EditorAction`]s (applied later by
//! [`crate::scenes::editor::gui`]): the File items, Edit > "Add node", View > "Re-layout",
//! "Frame all" (both enabled only with an active doc), View > "Problems" (a check-marked toggle,
//! always enabled) and Help > "Controls" / "About..." (always enabled).

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

/// Draws the main menu bar. `active` is the workspace's active tab index when a doc is open;
/// `problems_visible` is the check mark of View > "Problems". Menu clicks are pushed into
/// `actions`; they are applied after the menu scope ends.
pub fn draw(
    ctx: &mut GuiCtx,
    active: Option<usize>,
    problems_visible: bool,
    actions: &mut Vec<EditorAction>,
) {
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
        // Enabled only with an active doc; the viewport is imgui's display size (== render size).
        if item(ui, "Add node", "", active.is_some()) {
            actions.push(EditorAction::AddNode {
                viewport: super::display_viewport(ui),
            });
        }
    });
    ui.menu("View", || {
        // Enabled only with an active doc; the viewport is imgui's display size (== render size).
        if item(ui, "Re-layout", "", active.is_some()) {
            actions.push(EditorAction::Relayout {
                viewport: super::display_viewport(ui),
            });
        }
        if item(ui, "Frame all", "F", active.is_some()) {
            actions.push(EditorAction::FrameAll {
                viewport: super::display_viewport(ui),
            });
        }
        ui.separator();
        // Always enabled (workspace-level toggle); the check mark shows the window state.
        if ui
            .menu_item_config("Problems")
            .shortcut("Ctrl+Shift+P")
            .selected(problems_visible)
            .enabled(true)
            .build()
        {
            actions.push(EditorAction::ToggleProblems);
        }
    });
    ui.menu("Help", || {
        if item(ui, "Controls", "", true) {
            actions.push(EditorAction::ShowControls);
        }
        ui.separator();
        if item(ui, "About...", "", true) {
            actions.push(EditorAction::ShowAbout);
        }
    });
}

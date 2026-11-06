use aberredengine::prelude::*;

/// Draws the main menu bar. Every item except File > Quit is a disabled placeholder for now.
pub fn draw(ctx: &mut GuiCtx) {
    let ui = ctx.ui;
    let Some(_bar) = ui.begin_main_menu_bar() else {
        return;
    };
    ui.menu("File", || {
        ui.menu_item_config("New").enabled(false).build();
        ui.menu_item_config("Open...").enabled(false).build();
        ui.menu_item_config("Save").enabled(false).build();
        ui.menu_item_config("Save As...").enabled(false).build();
        ui.menu_item_config("Close Tab").enabled(false).build();
        ui.separator();
        if ui.menu_item_config("Quit").build() {
            ctx.intents.request_quit();
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

//! The tab bar below the main menu bar: one tab per open doc with a close button.
//!
//! Locking: the workspace is locked only to snapshot the tab labels, then again briefly to
//! apply a click on another tab; the widgets are drawn in between with the guard dropped.
//!
//! Programmatic changes of `ws.active` (open/new/focus) must select the matching tab without
//! flicker or fighting the user's click, so `TabItemFlags::SET_SELECTED` is passed only on the
//! frame the active index changed, tracked in the `LAST_ACTIVE` thread-local.

use std::cell::Cell;

use aberredengine::prelude::imgui::{Condition, TabItemFlags, WindowFlags};
use aberredengine::prelude::*;

use super::actions::{EditorAction, tab_label};
use crate::ops;
use crate::state::WorkspaceMutex;

thread_local! {
    /// The `ws.active` value seen on the previous frame; `None` before the first frame.
    static LAST_ACTIVE: Cell<Option<usize>> = const { Cell::new(None) };
}

/// Snapshot of the tab strip: one label per doc plus the active index. Locks the workspace briefly.
fn snapshot(app_state: &AppState) -> Option<(Vec<String>, usize)> {
    let handle = app_state.get::<WorkspaceMutex>()?;
    let ws = handle.lock().unwrap();
    let labels = ws
        .docs
        .iter()
        .enumerate()
        .map(|(index, doc)| tab_label(&ops::doc_title(doc), doc.dirty, index))
        .collect();
    Some((labels, ws.active))
}

/// Draws the tab strip in a borderless window pinned below the main menu bar. Tab clicks set
/// `ws.active`; close buttons push [`EditorAction::CloseTab`].
pub fn draw(ctx: &mut GuiCtx, actions: &mut Vec<EditorAction>) {
    let Some((labels, active)) = snapshot(ctx.app_state) else {
        return;
    };
    // SET_SELECTED only on the frame the active index changed (programmatic open/new/focus).
    let select_frame = LAST_ACTIVE.with(|last| {
        let changed = last.get() != Some(active);
        last.set(Some(active));
        changed
    });

    let ui = ctx.ui;
    let menu_bar_height = ui.frame_height();
    let width = ui.io().display_size[0];
    let strip_flags = WindowFlags::NO_TITLE_BAR
        | WindowFlags::NO_RESIZE
        | WindowFlags::NO_MOVE
        | WindowFlags::NO_SCROLLBAR
        | WindowFlags::NO_COLLAPSE
        | WindowFlags::NO_SAVED_SETTINGS
        | WindowFlags::NO_NAV_INPUTS
        | WindowFlags::NO_BRING_TO_FRONT_ON_FOCUS;
    let mut focus = None;
    ui.window("##tabstrip")
        .flags(strip_flags)
        .position([0.0, menu_bar_height], Condition::Always)
        .size([width, ui.frame_height_with_spacing()], Condition::Always)
        .build(|| {
            if labels.is_empty() {
                ui.text_disabled("File > New or File > Open...");
                return;
            }
            if let Some(_bar) = ui.tab_bar("##docs") {
                for (index, label) in labels.iter().enumerate() {
                    let mut opened = true;
                    let mut flags = TabItemFlags::empty();
                    if select_frame && index == active {
                        flags |= TabItemFlags::SET_SELECTED;
                    }
                    let selected = ui
                        .tab_item_with_flags(label, Some(&mut opened), flags)
                        .is_some();
                    if selected && index != active {
                        focus = Some(index);
                    }
                    if !opened {
                        actions.push(EditorAction::CloseTab(index));
                    }
                }
            }
        });

    // Clicking a tab makes it active (brief re-lock; the widgets above are already drawn).
    if let Some(index) = focus
        && let Some(handle) = ctx.app_state.get::<WorkspaceMutex>().cloned()
    {
        let mut ws = handle.lock().unwrap();
        if index < ws.docs.len() {
            ws.active = index;
        }
    }
}

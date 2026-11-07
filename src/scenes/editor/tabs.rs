//! The tab bar below the main menu bar: one tab per open doc with a close button.
//!
//! Locking: the workspace is locked only to snapshot the tab labels, then again briefly to
//! apply a click on another tab; the widgets are drawn in between with the guard dropped.
//!
//! Programmatic changes of `ws.active` (open/new/focus) must select the matching tab without
//! flicker or fighting the user's click, so `TabItemFlags::SET_SELECTED` is passed only on the
//! frame the active index changed, tracked in the `LAST_ACTIVE` thread-local.
//!
//! Tab ids are `###doc{doc_id}` (see [`tab_label`]), so a dirty star or a closed tab never
//! changes a tab's imgui identity. A frame with a clicked X ignores imgui's neighbour
//! re-selection (`resolve_focus`) and re-asserts the real active tab on the next frame.

use std::cell::Cell;

use aberredengine::prelude::imgui::{Condition, TabItemFlags, WindowFlags};
use aberredengine::prelude::*;

use super::actions::{EditorAction, tab_label};
use crate::ops;
use crate::state::with_workspace;

thread_local! {
    /// The `ws.active` value seen on the previous frame; `None` before the first frame.
    static LAST_ACTIVE: Cell<Option<usize>> = const { Cell::new(None) };
}

/// Snapshot of the tab strip: one label per doc plus the active index. Locks the workspace briefly.
fn snapshot(app_state: &AppState) -> Option<(Vec<String>, usize)> {
    with_workspace(app_state, |ws| {
        let labels = ws
            .docs
            .iter()
            .map(|doc| tab_label(&ops::doc_title(doc), doc.dirty, doc.doc_id))
            .collect();
        (labels, ws.active)
    })
}

/// Which tab a click should focus. On the `SET_SELECTED` frame imgui still reports the tab that
/// was selected before this frame as selected, so that stale report must be ignored — otherwise
/// the active tab ping-pongs between the two tabs on every frame.
fn clicked_focus(select_frame: bool, active: usize, index: usize, selected: bool) -> Option<usize> {
    if !select_frame && selected && index != active {
        Some(index)
    } else {
        None
    }
}

/// Resolves the tab focus after the strip was drawn. When a close button was clicked this frame
/// (`closing`), imgui re-selects a neighbouring tab in the same frame and that tab reports
/// selected — imgui's own re-selection, not a user click — so the pending focus is dropped and
/// the second value asks the caller to re-assert the real active tab on the next frame.
fn resolve_focus(focus: Option<usize>, closing: bool) -> (Option<usize>, bool) {
    if closing {
        (None, true)
    } else {
        (focus, false)
    }
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
    let mut closing = false;
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
                    if let Some(clicked) = clicked_focus(select_frame, active, index, selected) {
                        focus = Some(clicked);
                    }
                    if !opened {
                        closing = true;
                        actions.push(EditorAction::CloseTab(index));
                    }
                }
            }
        });

    // A clicked X makes imgui re-select a neighbour in this same frame; that report is not a
    // click, so drop it and re-assert the real active tab on the next frame (LAST_ACTIVE reset
    // makes it a SET_SELECTED frame, whose stale reports `clicked_focus` already ignores).
    let (focus, reselect) = resolve_focus(focus, closing);
    if reselect {
        LAST_ACTIVE.with(|last| last.set(None));
    }

    // Clicking a tab makes it active (brief re-lock; the widgets above are already drawn).
    if let Some(index) = focus {
        with_workspace(ctx.app_state, |ws| {
            if index < ws.docs.len() {
                ws.active = index;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{clicked_focus, resolve_focus};

    #[test]
    fn closing_drops_a_pending_focus_and_requests_a_reselect() {
        assert_eq!(resolve_focus(Some(1), true), (None, true));
        assert_eq!(resolve_focus(None, true), (None, true));
    }

    #[test]
    fn not_closing_passes_the_focus_through_and_requests_nothing() {
        assert_eq!(resolve_focus(Some(1), false), (Some(1), false));
        assert_eq!(resolve_focus(None, false), (None, false));
    }

    #[test]
    fn select_frame_ignores_the_stale_selected_report() {
        // On the SET_SELECTED frame imgui still reports the tab selected before this frame as
        // selected.
        assert_eq!(clicked_focus(true, 1, 0, true), None);
        assert_eq!(clicked_focus(true, 1, 1, true), None);
        assert_eq!(clicked_focus(true, 0, 1, true), None);
    }

    #[test]
    fn normal_frame_focuses_a_different_selected_tab() {
        assert_eq!(clicked_focus(false, 0, 1, true), Some(1));
        assert_eq!(clicked_focus(false, 1, 0, true), Some(0));
    }

    #[test]
    fn active_tab_never_focuses_itself() {
        assert_eq!(clicked_focus(false, 2, 2, true), None);
        assert_eq!(clicked_focus(true, 2, 2, true), None);
    }

    #[test]
    fn unselected_tabs_never_focus() {
        assert_eq!(clicked_focus(false, 0, 1, false), None);
        assert_eq!(clicked_focus(true, 0, 1, false), None);
    }
}

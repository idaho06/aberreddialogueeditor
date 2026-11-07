//! Small widgets shared by the Inspector's condition/effect/choice editors. Every helper keeps
//! the imgui id discipline of its callers: fixed `"##..."` label suffixes, `push_id_usize` per
//! entry, never the edited text.

use aberredengine::prelude::imgui::Ui;

/// Faint red for the "(empty key)" hint (dimmer than the lint-error red).
const FAINT_RED: [f32; 4] = [1.0, 0.45, 0.45, 0.55];

/// The key input shared by the leaf condition kinds and every effect kind, plus the faint-red
/// "(empty key)" hint when empty (a hint, not an error: the engine accepts empty keys).
pub(super) fn key_editor(ui: &Ui, key: &mut String) -> bool {
    let changed = ui.input_text("Key", key).build();
    if key.is_empty() {
        ui.text_colored(FAINT_RED, "(empty key)");
    }
    changed
}

/// A type combo: `label` (with its fixed `"##..."` id suffix) showing `preview`, one selectable
/// per entry of `kinds` (each in its own `push_id_usize` scope, labelled `name(kind)`). Picking an
/// entry different from `current` calls `on_pick(kind)` and returns `true`.
pub(super) fn type_combo<T: Copy + PartialEq>(
    ui: &Ui,
    label: &str,
    preview: &str,
    current: T,
    kinds: &[T],
    name: impl Fn(T) -> &'static str,
    mut on_pick: impl FnMut(T),
) -> bool {
    let mut changed = false;
    if let Some(combo) = ui.begin_combo(label, preview) {
        for (i, kind) in kinds.iter().enumerate() {
            let _entry = ui.push_id_usize(i);
            if ui
                .selectable_config(name(*kind))
                .selected(*kind == current)
                .build()
                && *kind != current
            {
                on_pick(*kind);
                changed = true;
            }
        }
        combo.end();
    }
    changed
}

/// What the Up/Down/Delete row of a list item requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RowAction {
    /// Move the item by `delta` positions (-1 = up, +1 = down).
    Move(i32),
    Delete,
}

/// The Up/Down/Delete small-button row of list item `i` (with `last` = `len - 1`): Up disabled
/// on the first row, Down on the last, via `begin_disabled`. The labels are the fixed widget ids,
/// scoped by the caller's per-row `push_id`.
pub(super) fn reorder_delete_row(ui: &Ui, i: usize, last: usize) -> Option<RowAction> {
    let mut action = None;
    if i == 0 {
        let _disabled = ui.begin_disabled(true);
        ui.small_button("Up");
    } else if ui.small_button("Up") {
        action = Some(RowAction::Move(-1));
    }
    ui.same_line();
    if i == last {
        let _disabled = ui.begin_disabled(true);
        ui.small_button("Down");
    } else if ui.small_button("Down") {
        action = Some(RowAction::Move(1));
    }
    ui.same_line();
    if ui.small_button("Delete") {
        action = Some(RowAction::Delete);
    }
    action
}

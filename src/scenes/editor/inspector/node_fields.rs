//! The Inspector window's body: the widgets editing one node's fields. Drawn against a CLONED
//! [`EdNode`] (the workspace lock is dropped while the widgets run); every widget that reports
//! a change pushes the matching [`Edit`].
//!
//! Widget ids come from the node key (the caller wraps the whole body in a `push_id` keyed by the
//! NodeKey) and from row indices — never from the edited text, so typing never changes an id and
//! the focused input never loses focus.

use aberredengine::prelude::imgui::Ui;

use super::{Edit, outputs};
use crate::model::{EdNode, NodeKey};

/// Lint-derived flags of the node being edited, computed in the snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NodeFlags {
    /// The node is the doc's start node.
    pub is_start: bool,
    /// The linter reports a duplicate id for this node.
    pub duplicate_id: bool,
    /// The linter reports an empty (or all-whitespace) id for this node.
    pub empty_id: bool,
}

/// Red note colour for the lint flags under the id input.
const RED: [f32; 4] = [1.0, 0.35, 0.35, 1.0];

/// Draws the node-field widgets for `node` and returns the edits the widgets requested (empty
/// when nothing changed this frame). `targets` is the doc's `(key, id)` list in node order, for
/// the outputs' target combos. Local `String` copies feed the input widgets, so the snapshot
/// stays immutable while drawing.
pub fn draw(ui: &Ui, node: &EdNode, flags: &NodeFlags, targets: &[(NodeKey, String)]) -> Vec<Edit> {
    let key = node.key;
    let mut edits = Vec::new();

    // Id: renames on every change. Links hold NodeKeys, so they survive the rename; duplicate
    // ids are only flagged by the linter (the red note below).
    let mut id = node.id.clone();
    if ui.input_text("Id", &mut id).build() {
        edits.push(Edit::SetId(key, id));
    }
    if flags.duplicate_id {
        ui.text_colored(RED, "duplicate id");
    }
    if flags.empty_id {
        ui.text_colored(RED, "empty id");
    }

    // Start node: a disabled "Start node" button when it already is, else "Set as start".
    if flags.is_start {
        let _disabled = ui.begin_disabled(true);
        ui.button("Start node");
    } else if ui.button("Set as start") {
        edits.push(Edit::SetStart(key));
    }

    // Speaker: checkbox plus a single-line input while checked. Unchecking emits
    // SetSpeaker(None); checking emits SetSpeaker(Some("")).
    let mut has_speaker = node.speaker.is_some();
    if ui.checkbox("Has speaker", &mut has_speaker) {
        edits.push(Edit::SetSpeaker(key, has_speaker.then(String::new)));
    }
    if has_speaker {
        let mut speaker = node.speaker.clone().unwrap_or_default();
        if ui.input_text("Speaker", &mut speaker).build() {
            edits.push(Edit::SetSpeaker(key, Some(speaker)));
        }
    }

    // Text: multiline, ~6 lines high, full width.
    let mut text = node.text.clone();
    let height = 6.0 * ui.text_line_height_with_spacing();
    if ui
        .input_text_multiline("Text", &mut text, [0.0, height])
        .build()
    {
        edits.push(Edit::SetText(key, text));
    }

    // Labels.
    if let Some(labels) = labels_editor(ui, "labels", &node.labels) {
        edits.push(Edit::SetLabels(key, labels));
    }

    // Outputs (next link / choices); on-enter effects stay a read-only placeholder for now.
    ui.separator();
    ui.text(format!("On enter effects: {}", node.on_enter.len()));
    edits.extend(outputs::draw(ui, node, targets));

    edits
}

/// The labels list: one single-line input per label with a "[-]" button on the same row (row ids
/// come from the index, never the text), plus a "[+ Add label]" button. `id` scopes the widget
/// ids (fixed string, so node labels and choice labels never collide). Returns `Some(new)` when
/// anything changed, `None` otherwise.
pub(super) fn labels_editor(ui: &Ui, id: &str, labels: &[String]) -> Option<Vec<String>> {
    let _scope = ui.push_id(id);
    let mut labels = labels.to_vec();
    let mut changed = false;
    let mut remove = None;
    for (index, label) in labels.iter_mut().enumerate() {
        let _row = ui.push_id_usize(index);
        // Leave room for the "[-]" button on the same line.
        ui.set_next_item_width(-40.0);
        if ui.input_text("Label", label).build() {
            changed = true;
        }
        ui.same_line();
        if ui.small_button("[-]") {
            remove = Some(index);
        }
    }
    if let Some(index) = remove {
        labels.remove(index);
        changed = true;
    }
    if ui.button("[+ Add label]") {
        labels.push(String::new());
        changed = true;
    }
    changed.then_some(labels)
}

//! The Inspector window's body: the widgets editing one node's fields. Drawn against a CLONED
//! [`EdNode`] (the workspace lock is dropped while the widgets run); every widget that reports
//! a change pushes the matching [`Edit`].
//!
//! Widget ids come from the node key (the caller wraps the whole body in a `push_id` keyed by the
//! NodeKey) and from row indices — never from the edited text, so typing never changes an id and
//! the focused input never loses focus.

use aberredengine::prelude::imgui::Ui;

use super::Edit;
use crate::model::{EdNode, NodeKey, Outputs};

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
/// when nothing changed this frame). Local `String` copies feed the input widgets, so the
/// snapshot stays immutable while drawing.
pub fn draw(ui: &Ui, node: &EdNode, flags: &NodeFlags) -> Vec<Edit> {
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
    edits.extend(labels_editor(ui, key, &node.labels));

    // Read-only placeholders for the next steps.
    ui.separator();
    ui.text(format!("On enter effects: {}", node.on_enter.len()));
    let outputs = match &node.out {
        Outputs::Next(_) => "next".to_owned(),
        Outputs::Choices(choices) => format!("choices ({})", choices.len()),
    };
    ui.text(format!("Outputs: {outputs}"));

    edits
}

/// The labels list: one single-line input per label with a "[-]" button on the same row (row ids
/// come from the index, never the text), plus a "[+ Add label]" button. Every change emits one
/// `SetLabels` with the whole new vector.
fn labels_editor(ui: &Ui, key: NodeKey, labels: &[String]) -> Vec<Edit> {
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
    if changed {
        vec![Edit::SetLabels(key, labels)]
    } else {
        Vec::new()
    }
}

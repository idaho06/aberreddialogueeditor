//! The "Inspector" window: shows the SELECTED node of the ACTIVE doc and edits its fields (id,
//! speaker, text, labels, start, outputs/choices incl. each choice's `when` condition and
//! `effects`, and the node's on-enter effects).
//!
//! The PURE edit core — the [`Edit`] enum, [`OutputsMode`], [`change_mode`] and [`apply_edits`] —
//! lives in `edit.rs` and is re-exported here; this module keeps the window itself.
//!
//! Locking (the standard panel pattern): the workspace is locked only to snapshot the selected
//! node (a clone), its [`NodeFlags`] and the doc's `doc_id`, and again to apply the collected
//! edits — only when the active doc's `doc_id` still matches the snapshot's. The widgets are
//! drawn with the guard dropped, and the lock is never held across widget calls.
//!
//! Edits are applied here through the pure [`apply_edits`], NOT through
//! `actions::apply_action`: that logs `{action:?}`, and the edit payloads carry node content,
//! which must never reach the log. [`apply_edits`] logs only the edit KIND and the node key.

mod edit;
mod effects;
mod expr;
mod node_fields;
mod outputs;
mod widgets;

use aberredengine::prelude::imgui::Condition;
use aberredengine::prelude::*;

use crate::model::{DialogueDoc, EdNode, NodeKey, ProblemKind};
use crate::state::with_workspace;
use node_fields::NodeFlags;

pub use edit::{Edit, OutputsMode, apply_edits};

/// What the Inspector window shows, snapshotted under a short workspace lock.
enum Panel {
    /// No doc is open.
    NoDoc,
    /// A doc is open but nothing is selected (or the selected key no longer exists).
    NothingSelected,
    /// The selected node of the active doc. Boxed: the snapshot (node + targets) is by far the
    /// largest `Panel` variant.
    Node(Box<NodeSnap>),
}

/// Snapshot of the selected node plus the doc identity the edits are guarded by.
struct NodeSnap {
    doc_id: u64,
    node: EdNode,
    flags: NodeFlags,
    /// `(key, id)` of every node of the doc, in node order — the target-combo entries.
    targets: Vec<(NodeKey, String)>,
}

/// The lint-derived flags of the node `key`: is it the start node, and do the doc's problems
/// flag its id (duplicate / empty)? Computed in the snapshot from `doc.problems()`.
fn node_flags(doc: &mut DialogueDoc, key: NodeKey) -> NodeFlags {
    let start = doc.start;
    let problems = doc.problems();
    NodeFlags {
        is_start: start == Some(key),
        duplicate_id: problems
            .iter()
            .any(|p| p.node == Some(key) && p.kind == ProblemKind::DuplicateId),
        empty_id: problems
            .iter()
            .any(|p| p.node == Some(key) && p.kind == ProblemKind::EmptyId),
    }
}

/// Locks the workspace briefly and clones what the panel needs; the guard is dropped on return.
fn snapshot(app_state: &AppState) -> Panel {
    with_workspace(app_state, |ws| {
        let Some(doc) = ws.active_doc_mut() else {
            return Panel::NoDoc;
        };
        let Some(key) = doc.selected else {
            return Panel::NothingSelected;
        };
        let Some(node) = doc.node(key).cloned() else {
            // The selected key vanished (e.g. the node was deleted): treat as no selection.
            return Panel::NothingSelected;
        };
        let flags = node_flags(doc, key);
        let targets = doc.nodes.iter().map(|n| (n.key, n.id.clone())).collect();
        Panel::Node(Box::new(NodeSnap {
            doc_id: doc.doc_id,
            node,
            flags,
            targets,
        }))
    })
    .unwrap_or(Panel::NoDoc)
}

/// Draws the Inspector window for the active doc's selected node and applies the edits the
/// widgets requested (when the active doc is still the snapshotted one).
pub fn draw(ctx: &mut GuiCtx) {
    let panel = snapshot(ctx.app_state);
    let doc_id = match &panel {
        Panel::Node(snap) => Some(snap.doc_id),
        _ => None,
    };

    let mut edits = Vec::new();
    let ui = ctx.ui;
    let display = ui.io().display_size;
    ui.window("Inspector")
        .position([display[0] - 360.0, 60.0], Condition::FirstUseEver)
        .size([340.0, display[1] - 80.0], Condition::FirstUseEver)
        .build(|| match &panel {
            Panel::NoDoc => {
                ui.text_disabled("No document");
            }
            Panel::NothingSelected => {
                ui.text_disabled("Nothing selected");
            }
            Panel::Node(snap) => {
                // Widget ids come from the NodeKey, never from the edited text: typing must
                // not change an id (the input would lose focus).
                let _id = ui.push_id(format!("node{}", snap.node.key.0));
                edits = node_fields::draw(ui, &snap.node, &snap.flags, &snap.targets);
            }
        });

    if edits.is_empty() {
        return;
    }
    // Apply the edits only if the active doc is still the one the snapshot was taken from.
    let Some(doc_id) = doc_id else {
        return;
    };
    with_workspace(ctx.app_state, |ws| {
        if let Some(doc) = ws.active_doc_mut().filter(|doc| doc.doc_id == doc_id) {
            apply_edits(doc, &edits);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::linked_doc;

    /// A clean doc with `a --next--> b`; `a` has id node_1, speaker Guard, text hello, one label.
    fn base_doc() -> (DialogueDoc, NodeKey, NodeKey) {
        let (mut doc, a, b) = linked_doc();
        doc.set_text(a, "hello".to_owned());
        doc.set_speaker(a, Some("Guard".to_owned()));
        doc.set_labels(a, vec!["l1".to_owned()]);
        doc.mark_clean();
        (doc, a, b)
    }

    #[test]
    fn node_flags_reflect_start_and_id_problems() {
        let (mut doc, a, b) = base_doc();
        let flags = node_flags(&mut doc, a);
        assert!(flags.is_start);
        assert!(!flags.duplicate_id);
        assert!(!flags.empty_id);

        let bid = doc.node(b).unwrap().id.clone();
        apply_edits(&mut doc, &[Edit::SetId(a, bid)]);
        let flags = node_flags(&mut doc, a);
        assert!(flags.duplicate_id, "duplicate id is flagged");
        assert!(!flags.empty_id);

        apply_edits(
            &mut doc,
            &[Edit::SetId(a, "  ".to_owned()), Edit::SetStart(b)],
        );
        let flags = node_flags(&mut doc, a);
        assert!(flags.empty_id, "all-whitespace id is flagged empty");
        assert!(
            !flags.duplicate_id,
            "a whitespace id is not a duplicate of node_2"
        );
        assert!(!flags.is_start, "a is no longer the start");
        let flags_b = node_flags(&mut doc, b);
        assert!(flags_b.is_start, "b is the start now");
        assert!(!flags_b.duplicate_id && !flags_b.empty_id);
    }
}

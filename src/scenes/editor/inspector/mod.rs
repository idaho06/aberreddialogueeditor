//! The "Inspector" window: shows the SELECTED node of the ACTIVE doc and edits its fields (id,
//! speaker, text, labels, start). Outputs and on-enter effects are read-only placeholders for
//! the next steps.
//!
//! Locking (the standard panel pattern): the workspace is locked only to snapshot the selected
//! node (a clone), its [`NodeFlags`] and the doc's `doc_id`, and again to apply the collected
//! edits — only when the active doc's `doc_id` still matches the snapshot's. The widgets are
//! drawn with the guard dropped, and the lock is never held across widget calls.
//!
//! Edits are applied here through the pure [`apply_edits`], NOT through
//! `actions::apply_action`: that logs `{action:?}`, and the edit payloads carry node content,
//! which must never reach the log. [`apply_edits`] logs only the edit KIND and the node key.

mod node_fields;

use aberredengine::prelude::imgui::Condition;
use aberredengine::prelude::*;

use crate::model::{DialogueDoc, EdNode, NodeKey, ProblemKind};
use crate::state::WorkspaceMutex;
use node_fields::NodeFlags;

/// One requested change to a node, produced by the inspector widgets and applied by
/// [`apply_edits`]. The payloads carry node content, so they must never be logged.
// The `Set*` variant names are the plan-04 API (Edit::SetText etc.), so the common-prefix lint
// is allowed here on purpose.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    /// Rename the node (links hold NodeKeys, so they survive; duplicates are only lint-flagged).
    SetId(NodeKey, String),
    /// Set or clear the speaker.
    SetSpeaker(NodeKey, Option<String>),
    SetText(NodeKey, String),
    /// Replace the whole labels vector.
    SetLabels(NodeKey, Vec<String>),
    /// Make the node the doc's start.
    SetStart(NodeKey),
}

/// Applies `edits` in order through the [`DialogueDoc`] mutators. An edit addressed to a NodeKey
/// that no longer exists is ignored (the mutators no-op on unknown keys: no panic, no revision
/// bump). Logs only the edit KIND and the node key — never the content.
pub fn apply_edits(doc: &mut DialogueDoc, edits: &[Edit]) {
    for edit in edits {
        match edit {
            Edit::SetId(key, id) => {
                log::trace!("edit SetId: node={}", key.0);
                doc.rename_node(*key, id.clone());
            }
            Edit::SetSpeaker(key, speaker) => {
                log::trace!("edit SetSpeaker: node={}", key.0);
                doc.set_speaker(*key, speaker.clone());
            }
            Edit::SetText(key, text) => {
                log::trace!("edit SetText: node={}", key.0);
                doc.set_text(*key, text.clone());
            }
            Edit::SetLabels(key, labels) => {
                log::trace!("edit SetLabels: node={}", key.0);
                doc.set_labels(*key, labels.clone());
            }
            Edit::SetStart(key) => {
                log::debug!("edit SetStart: node={}", key.0);
                doc.set_start(*key);
            }
        }
    }
}

/// What the Inspector window shows, snapshotted under a short workspace lock.
enum Panel {
    /// No doc is open.
    NoDoc,
    /// A doc is open but nothing is selected (or the selected key no longer exists).
    NothingSelected,
    /// The selected node of the active doc.
    Node(NodeSnap),
}

/// Snapshot of the selected node plus the doc identity the edits are guarded by.
struct NodeSnap {
    doc_id: u64,
    node: EdNode,
    flags: NodeFlags,
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
    let Some(handle) = app_state.get::<WorkspaceMutex>() else {
        return Panel::NoDoc;
    };
    let mut ws = handle.lock().unwrap();
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
    Panel::Node(NodeSnap {
        doc_id: doc.doc_id,
        node,
        flags,
    })
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
                edits = node_fields::draw(ui, &snap.node, &snap.flags);
            }
        });

    if edits.is_empty() {
        return;
    }
    // Apply the edits only if the active doc is still the one the snapshot was taken from.
    let Some(doc_id) = doc_id else {
        return;
    };
    if let Some(handle) = ctx.app_state.get::<WorkspaceMutex>().cloned() {
        let mut ws = handle.lock().unwrap();
        if let Some(doc) = ws.active_doc_mut().filter(|doc| doc.doc_id == doc_id) {
            apply_edits(doc, &edits);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Link, Outputs, Slot, from_def, to_def};
    use aberredengine::prelude::{DialogueDef, Vec2};

    const GUARD: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/guard.json"
    ));

    /// A clean doc with `a --next--> b`; `a` has id node_1, speaker Guard, text hello, one label.
    fn base_doc() -> (DialogueDoc, NodeKey, NodeKey) {
        let mut doc = DialogueDoc::new_empty();
        let a = doc.add_node(Vec2::ZERO);
        let b = doc.add_node(Vec2::new(10.0, 0.0));
        doc.set_link(a, Slot::Next, Link::Node(b));
        doc.set_text(a, "hello".to_owned());
        doc.set_speaker(a, Some("Guard".to_owned()));
        doc.set_labels(a, vec!["l1".to_owned()]);
        doc.mark_clean();
        (doc, a, b)
    }

    /// Test helper: applies `edit` to a field of the expected node clone.
    type Expected = Box<dyn FnOnce(&mut EdNode)>;

    #[test]
    fn every_edit_variant_changes_exactly_its_field_and_bumps_both_revisions_and_sets_dirty() {
        let a = NodeKey(1);
        let cases: Vec<(&str, Edit, Expected)> = vec![
            (
                "SetId",
                Edit::SetId(a, "renamed".to_owned()),
                Box::new(|n| n.id = "renamed".to_owned()),
            ),
            (
                "SetSpeaker",
                Edit::SetSpeaker(a, Some("Paladin".to_owned())),
                Box::new(|n| n.speaker = Some("Paladin".to_owned())),
            ),
            (
                "SetText",
                Edit::SetText(a, "new text".to_owned()),
                Box::new(|n| n.text = "new text".to_owned()),
            ),
            (
                "SetLabels",
                Edit::SetLabels(a, vec!["x".to_owned(), "y".to_owned()]),
                Box::new(|n| n.labels = vec!["x".to_owned(), "y".to_owned()]),
            ),
        ];
        for (name, edit, mutate) in cases {
            let (mut doc, _a, _b) = base_doc();
            let before = doc.node(a).expect("node exists").clone();
            let revision = doc.revision;
            let content_revision = doc.content_revision;
            assert!(!doc.dirty, "{name}: the base doc is clean");
            apply_edits(&mut doc, &[edit]);
            let mut expected = before.clone();
            mutate(&mut expected);
            assert_eq!(
                doc.node(a).expect("node exists"),
                &expected,
                "{name}: exactly the intended field changed"
            );
            assert_eq!(doc.revision, revision + 1, "{name}: revision bumps once");
            assert_eq!(
                doc.content_revision,
                content_revision + 1,
                "{name}: content_revision bumps once"
            );
            assert!(doc.dirty, "{name}: dirty is set");
        }
    }

    #[test]
    fn set_start_edit_changes_doc_start_and_bumps_like_a_content_edit() {
        let (mut doc, a, b) = base_doc();
        assert_eq!(doc.start, Some(a));
        let revision = doc.revision;
        let content_revision = doc.content_revision;
        assert!(!doc.dirty);
        apply_edits(&mut doc, &[Edit::SetStart(b)]);
        assert_eq!(doc.start, Some(b), "SetStart changes doc.start");
        assert_eq!(doc.revision, revision + 1);
        assert_eq!(doc.content_revision, content_revision + 1);
        assert!(doc.dirty);
    }

    #[test]
    fn edits_for_a_vanished_node_key_are_ignored_without_bumping_or_dirtying() {
        let (mut doc, a, _b) = base_doc();
        let ghost = NodeKey(999);
        let revision = doc.revision;
        let content_revision = doc.content_revision;
        apply_edits(
            &mut doc,
            &[
                Edit::SetId(ghost, "x".to_owned()),
                Edit::SetSpeaker(ghost, Some("s".to_owned())),
                Edit::SetText(ghost, "t".to_owned()),
                Edit::SetLabels(ghost, vec!["l".to_owned()]),
                Edit::SetStart(ghost),
            ],
        );
        assert_eq!(doc.revision, revision, "no revision bump");
        assert_eq!(doc.content_revision, content_revision, "no content bump");
        assert!(!doc.dirty, "not dirty");
        assert_eq!(doc.start, Some(a), "start untouched");
        assert!(doc.node(ghost).is_none());
    }

    #[test]
    fn a_sequence_of_edits_applies_in_order() {
        let (mut doc, a, _b) = base_doc();
        let revision = doc.revision;
        let content_revision = doc.content_revision;
        apply_edits(
            &mut doc,
            &[
                Edit::SetText(a, "first".to_owned()),
                Edit::SetText(a, "second".to_owned()),
                Edit::SetId(a, "final".to_owned()),
            ],
        );
        let node = doc.node(a).expect("node exists");
        assert_eq!(node.text, "second", "the later SetText wins");
        assert_eq!(node.id, "final");
        assert_eq!(doc.revision, revision + 3, "one bump per applied edit");
        assert_eq!(doc.content_revision, content_revision + 3);
        assert!(doc.dirty);
    }

    #[test]
    fn set_speaker_none_clears_the_speaker() {
        let (mut doc, a, _b) = base_doc();
        assert_eq!(doc.node(a).unwrap().speaker, Some("Guard".to_owned()));
        apply_edits(&mut doc, &[Edit::SetSpeaker(a, None)]);
        assert_eq!(
            doc.node(a).unwrap().speaker,
            None,
            "None clears the speaker"
        );
        apply_edits(&mut doc, &[Edit::SetSpeaker(a, Some("Paladin".to_owned()))]);
        assert_eq!(doc.node(a).unwrap().speaker, Some("Paladin".to_owned()));
    }

    #[test]
    fn renaming_a_node_keeps_links_pointing_at_it_via_to_def() {
        let def: DialogueDef = serde_json::from_str(GUARD).expect("fixture parses");
        let mut doc = from_def(&def);
        // guard.json: greet (key 1) has choices targeting friend (key 2) and pass (key 3);
        // pass (key 3) has next -> friend.
        doc.mark_clean();
        apply_edits(&mut doc, &[Edit::SetId(NodeKey(2), "pal".to_owned())]);
        assert_eq!(doc.node(NodeKey(2)).unwrap().id, "pal");
        let def = to_def(&doc);
        let greet = def.nodes.iter().find(|n| n.id == "greet").unwrap();
        let choices = greet.choices.as_ref().expect("greet has choices");
        assert_eq!(
            choices[0].next.as_deref(),
            Some("pal"),
            "the choice link now carries the renamed node's new id"
        );
        assert_eq!(choices[1].next.as_deref(), Some("pass"));
        assert_eq!(choices[2].next, None);
        let pass = def.nodes.iter().find(|n| n.id == "pass").unwrap();
        assert_eq!(
            pass.next.as_deref(),
            Some("pal"),
            "the next link now carries the new id (links hold NodeKeys)"
        );
    }

    #[test]
    fn apply_edits_allows_duplicate_ids_and_the_linter_reports_them() {
        let (mut doc, a, b) = base_doc();
        let bid = doc.node(b).unwrap().id.clone();
        apply_edits(&mut doc, &[Edit::SetId(a, bid.clone())]);
        assert_eq!(
            doc.node(a).unwrap().id,
            bid,
            "the duplicate rename is applied (only the linter flags it)"
        );
        let problems = doc.problems();
        let dups: Vec<&crate::model::Problem> = problems
            .iter()
            .filter(|p| p.kind == ProblemKind::DuplicateId)
            .collect();
        assert_eq!(dups.len(), 2, "both nodes carrying the id are reported");
        assert!(dups.iter().all(|p| p.node == Some(a) || p.node == Some(b)));
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

    #[test]
    fn apply_edits_on_an_empty_slice_is_a_no_op() {
        let (mut doc, _a, _b) = base_doc();
        let revision = doc.revision;
        apply_edits(&mut doc, &[]);
        assert_eq!(doc.revision, revision);
        assert!(!doc.dirty);
        assert!(
            matches!(
                &doc.node(NodeKey(1)).unwrap().out,
                Outputs::Next(Link::Node(NodeKey(2)))
            ),
            "the node content is untouched"
        );
    }
}

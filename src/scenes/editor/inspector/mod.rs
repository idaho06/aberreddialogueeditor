//! The "Inspector" window: shows the SELECTED node of the ACTIVE doc and edits its fields (id,
//! speaker, text, labels, start, outputs/choices incl. each choice's `when` condition and
//! `effects`, and the node's on-enter effects).
//!
//! Locking (the standard panel pattern): the workspace is locked only to snapshot the selected
//! node (a clone), its [`NodeFlags`] and the doc's `doc_id`, and again to apply the collected
//! edits — only when the active doc's `doc_id` still matches the snapshot's. The widgets are
//! drawn with the guard dropped, and the lock is never held across widget calls.
//!
//! Edits are applied here through the pure [`apply_edits`], NOT through
//! `actions::apply_action`: that logs `{action:?}`, and the edit payloads carry node content,
//! which must never reach the log. [`apply_edits`] logs only the edit KIND and the node key.

mod effects;
mod expr;
mod node_fields;
mod outputs;

use aberredengine::prelude::imgui::Condition;
use aberredengine::prelude::*;

use crate::model::{DialogueDoc, EdChoice, EdNode, Link, NodeKey, Outputs, ProblemKind, Slot};
use crate::state::WorkspaceMutex;
use node_fields::NodeFlags;

/// The two output modes of a node, switched by the Inspector's "Next" / "Choices" radio buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputsMode {
    /// A single unconditional `next` link.
    Next,
    /// A list of player choices.
    Choices,
}

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
    /// Switch the node's output mode (see [`change_mode`] for the conversion semantics).
    ChangeMode(NodeKey, OutputsMode),
    /// Rewrite one output slot: `Slot::Next` on a `Next` node, `Slot::Choice(i)` on a choices node.
    SetLink(NodeKey, Slot, Link),
    /// Append a choice. On a choices node this appends an empty choice; on a `Next` node (not
    /// emitted by the UI, but if applied) it converts like [`change_mode`] so the next link is
    /// NOT lost.
    AddChoice(NodeKey),
    /// Delete choice `index`; deleting the last choice converts the node to `Next(End)`.
    DeleteChoice(NodeKey, usize),
    /// Move choice `index` by `delta` positions (clamped at both ends).
    MoveChoice(NodeKey, usize, i32),
    SetChoiceText(NodeKey, usize, String),
    /// Replace the whole labels vector of choice `index`.
    SetChoiceLabels(NodeKey, usize, Vec<String>),
    /// Replace the whole `when` condition of choice `index` (`None` clears it).
    SetChoiceWhen(NodeKey, usize, Option<DialogueCondition>),
    /// Replace the whole effects list of choice `index` (an empty Vec clears it).
    SetChoiceEffects(NodeKey, usize, Vec<DialogueEffect>),
    /// Replace the node's whole on-enter effects list (an empty Vec clears it).
    SetOnEnter(NodeKey, Vec<DialogueEffect>),
}

/// The pure mode-switch conversion: `None` when the node already is in `mode` (a no-op — no
/// bump). `Next -> Choices` builds exactly ONE empty choice whose target INHERITS the current
/// next link (a `Dangling` one included); `Choices -> Next` keeps the FIRST choice's target
/// (`End` when there is none) and drops the choices. Never produces `Outputs::Choices(vec![])`.
fn change_mode(out: &Outputs, mode: OutputsMode) -> Option<Outputs> {
    match (out, mode) {
        (Outputs::Next(_), OutputsMode::Next) | (Outputs::Choices(_), OutputsMode::Choices) => None,
        (Outputs::Next(link), OutputsMode::Choices) => Some(Outputs::Choices(vec![EdChoice {
            text: String::new(),
            target: link.clone(),
            when: None,
            effects: Vec::new(),
            labels: Vec::new(),
        }])),
        (Outputs::Choices(choices), OutputsMode::Next) => Some(Outputs::Next(
            choices
                .first()
                .map(|c| c.target.clone())
                .unwrap_or(Link::End),
        )),
    }
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
            Edit::ChangeMode(key, mode) => {
                // Structural kind: debug. Logs the mode kind only, never content.
                log::debug!("edit ChangeMode: node={} mode={:?}", key.0, mode);
                let out = doc.node(*key).map(|n| n.out.clone());
                if let Some(out) = out
                    && let Some(new) = change_mode(&out, *mode)
                {
                    doc.set_outputs(*key, new);
                }
            }
            Edit::SetLink(key, slot, link) => {
                // The slot (with its index) is logged; the link payload never is (a Dangling
                // link carries an id string).
                log::debug!("edit SetLink: node={} slot={:?}", key.0, slot);
                doc.set_link(*key, *slot, link.clone());
            }
            Edit::AddChoice(key) => {
                log::debug!("edit AddChoice: node={}", key.0);
                let out = doc.node(*key).map(|n| n.out.clone());
                match out {
                    // Not emitted by the UI on a Next node, but if applied, convert like
                    // ChangeMode so the current next link is not lost.
                    Some(out @ Outputs::Next(_)) => {
                        if let Some(new) = change_mode(&out, OutputsMode::Choices) {
                            doc.set_outputs(*key, new);
                        }
                    }
                    Some(Outputs::Choices(_)) => doc.add_choice(*key),
                    None => {}
                }
            }
            Edit::DeleteChoice(key, index) => {
                log::debug!("edit DeleteChoice: node={} choice={index}", key.0);
                doc.delete_choice(*key, *index);
            }
            Edit::MoveChoice(key, index, delta) => {
                log::debug!(
                    "edit MoveChoice: node={} choice={index} delta={delta}",
                    key.0
                );
                doc.move_choice(*key, *index, *delta);
            }
            Edit::SetChoiceText(key, index, text) => {
                // Per-keystroke kind: trace, and never the text itself.
                log::trace!("edit SetChoiceText: node={} choice={index}", key.0);
                doc.set_choice_text(*key, *index, text.clone());
            }
            Edit::SetChoiceLabels(key, index, labels) => {
                log::trace!("edit SetChoiceLabels: node={} choice={index}", key.0);
                doc.set_choice_labels(*key, *index, labels.clone());
            }
            Edit::SetChoiceWhen(key, index, when) => {
                // Per-keystroke kind: trace, kind + node key + index only, never the condition.
                log::trace!("edit SetChoiceWhen: node={} choice={index}", key.0);
                doc.set_choice_when(*key, *index, when.clone());
            }
            Edit::SetChoiceEffects(key, index, effects) => {
                // Per-keystroke kind: trace, kind + node key + index only, never the effects.
                log::trace!("edit SetChoiceEffects: node={} choice={index}", key.0);
                doc.set_choice_effects(*key, *index, effects.clone());
            }
            Edit::SetOnEnter(key, effects) => {
                log::trace!("edit SetOnEnter: node={}", key.0);
                doc.set_on_enter(*key, effects.clone());
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
    let targets = doc.nodes.iter().map(|n| (n.key, n.id.clone())).collect();
    Panel::Node(Box::new(NodeSnap {
        doc_id: doc.doc_id,
        node,
        flags,
        targets,
    }))
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
    use crate::model::{Link, Outputs, Slot, engine_gate, from_def, to_def};
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

    /// A clean doc where `a` is a choices node with two choices: "go" -> End, "stay" -> b.
    fn choices_doc() -> (DialogueDoc, NodeKey, NodeKey) {
        let (mut doc, a, b) = base_doc();
        doc.add_choice(a); // Next node: one empty choice (the raw mutator drops the next link)
        doc.add_choice(a); // appends
        doc.set_choice_text(a, 0, "go".to_owned());
        doc.set_choice_text(a, 1, "stay".to_owned());
        doc.set_link(a, Slot::Choice(1), Link::Node(b));
        doc.mark_clean();
        (doc, a, b)
    }

    /// Extracts the choices of `key` as (text, target) pairs.
    fn choice_pairs(doc: &DialogueDoc, key: NodeKey) -> Vec<(String, Link)> {
        match &doc.node(key).expect("node exists").out {
            Outputs::Choices(v) => v
                .iter()
                .map(|c| (c.text.clone(), c.target.clone()))
                .collect(),
            Outputs::Next(_) => panic!("expected choices"),
        }
    }

    #[test]
    fn change_mode_next_to_choices_builds_one_empty_choice_inheriting_the_next_link() {
        // Inherits Link::Node.
        let (mut doc, a, _b) = base_doc();
        let revision = doc.revision;
        let content_revision = doc.content_revision;
        assert!(!doc.dirty);
        apply_edits(&mut doc, &[Edit::ChangeMode(a, OutputsMode::Choices)]);
        match &doc.node(a).unwrap().out {
            Outputs::Choices(v) => {
                assert_eq!(v.len(), 1, "exactly ONE choice");
                assert_eq!(v[0].text, "");
                assert_eq!(v[0].target, Link::Node(_b), "inherits the next link");
                assert!(v[0].when.is_none());
                assert!(v[0].effects.is_empty());
                assert!(v[0].labels.is_empty());
            }
            Outputs::Next(_) => panic!("expected choices"),
        }
        assert_eq!(doc.revision, revision + 1);
        assert_eq!(doc.content_revision, content_revision + 1);
        assert!(doc.dirty);

        // Inherits Link::End.
        let (mut doc, _a, _b) = base_doc();
        let c = doc.add_node(Vec2::new(20.0, 0.0)); // Next(End)
        doc.mark_clean();
        let revision = doc.revision;
        apply_edits(&mut doc, &[Edit::ChangeMode(c, OutputsMode::Choices)]);
        match &doc.node(c).unwrap().out {
            Outputs::Choices(v) => {
                assert_eq!(v.len(), 1);
                assert_eq!(v[0].target, Link::End, "inherits End");
            }
            Outputs::Next(_) => panic!("expected choices"),
        }
        assert_eq!(doc.revision, revision + 1);

        // Inherits a Dangling link.
        let (mut doc, a, _b) = base_doc();
        apply_edits(
            &mut doc,
            &[Edit::SetLink(
                a,
                Slot::Next,
                Link::Dangling("ghost".to_owned()),
            )],
        );
        doc.mark_clean();
        let revision = doc.revision;
        apply_edits(&mut doc, &[Edit::ChangeMode(a, OutputsMode::Choices)]);
        match &doc.node(a).unwrap().out {
            Outputs::Choices(v) => {
                assert_eq!(v.len(), 1);
                assert!(
                    matches!(&v[0].target, Link::Dangling(s) if s == "ghost"),
                    "a Dangling next link is inherited verbatim"
                );
            }
            Outputs::Next(_) => panic!("expected choices"),
        }
        assert_eq!(doc.revision, revision + 1);
    }

    #[test]
    fn change_mode_choices_to_next_keeps_the_first_choice_link_and_drops_the_rest() {
        // First choice targets End -> Next(End); the second choice (-> b) is dropped.
        let (mut doc, a, b) = choices_doc();
        assert_eq!(
            choice_pairs(&doc, a),
            [
                ("go".to_owned(), Link::End),
                ("stay".to_owned(), Link::Node(b))
            ]
        );
        let revision = doc.revision;
        let content_revision = doc.content_revision;
        assert!(!doc.dirty);
        apply_edits(&mut doc, &[Edit::ChangeMode(a, OutputsMode::Next)]);
        assert!(
            matches!(&doc.node(a).unwrap().out, Outputs::Next(Link::End)),
            "the FIRST choice's target (End) becomes the next link"
        );
        assert_eq!(doc.revision, revision + 1);
        assert_eq!(doc.content_revision, content_revision + 1);
        assert!(doc.dirty);

        // First choice targets a node -> Next(Node(b)).
        let (mut doc, a, b) = choices_doc();
        doc.set_link(a, Slot::Choice(0), Link::Node(b));
        doc.mark_clean();
        apply_edits(&mut doc, &[Edit::ChangeMode(a, OutputsMode::Next)]);
        assert!(matches!(&doc.node(a).unwrap().out, Outputs::Next(Link::Node(k)) if *k == b));

        // No choice at all (only reachable via set_outputs) -> Next(End).
        let (mut doc, a, _b) = base_doc();
        doc.set_outputs(a, Outputs::Choices(vec![]));
        doc.mark_clean();
        let revision = doc.revision;
        apply_edits(&mut doc, &[Edit::ChangeMode(a, OutputsMode::Next)]);
        assert!(matches!(
            &doc.node(a).unwrap().out,
            Outputs::Next(Link::End)
        ));
        assert_eq!(doc.revision, revision + 1);
    }

    #[test]
    fn change_mode_to_the_current_mode_is_a_no_op_without_bump_or_dirty() {
        let (mut doc, a, _b) = base_doc(); // a is Next
        let revision = doc.revision;
        let content_revision = doc.content_revision;
        apply_edits(&mut doc, &[Edit::ChangeMode(a, OutputsMode::Next)]);
        assert_eq!(doc.revision, revision, "no bump");
        assert_eq!(doc.content_revision, content_revision, "no content bump");
        assert!(!doc.dirty, "not dirty");
        assert!(matches!(&doc.node(a).unwrap().out, Outputs::Next(_)));

        let (mut doc, a, _b) = choices_doc(); // a is Choices
        let revision = doc.revision;
        apply_edits(&mut doc, &[Edit::ChangeMode(a, OutputsMode::Choices)]);
        assert_eq!(doc.revision, revision, "no bump");
        assert!(!doc.dirty, "not dirty");
        assert_eq!(choice_pairs(&doc, a).len(), 2, "choices untouched");
    }

    #[test]
    fn delete_last_remaining_choice_converts_the_node_to_next_end() {
        let (mut doc, a, _b) = choices_doc();
        apply_edits(&mut doc, &[Edit::DeleteChoice(a, 1)]);
        assert_eq!(choice_pairs(&doc, a).len(), 1, "one choice left");
        let revision = doc.revision;
        apply_edits(&mut doc, &[Edit::DeleteChoice(a, 0)]);
        assert!(
            matches!(&doc.node(a).unwrap().out, Outputs::Next(Link::End)),
            "deleting the last choice converts to Next(End)"
        );
        assert_eq!(doc.revision, revision + 1);
    }

    #[test]
    fn delete_a_middle_choice_keeps_the_others_in_order() {
        let (mut doc, a, b) = choices_doc();
        doc.add_choice(a);
        doc.set_choice_text(a, 2, "wait".to_owned());
        doc.set_link(a, Slot::Choice(2), Link::Node(b));
        doc.mark_clean();
        assert_eq!(
            choice_pairs(&doc, a),
            [
                ("go".to_owned(), Link::End),
                ("stay".to_owned(), Link::Node(b)),
                ("wait".to_owned(), Link::Node(b)),
            ]
        );
        apply_edits(&mut doc, &[Edit::DeleteChoice(a, 1)]);
        assert_eq!(
            choice_pairs(&doc, a),
            [
                ("go".to_owned(), Link::End),
                ("wait".to_owned(), Link::Node(b))
            ],
            "the remaining choices keep their order and targets"
        );
    }

    #[test]
    fn move_choice_up_and_down_with_clamping_at_both_ends() {
        let (mut doc, a, b) = choices_doc();
        doc.add_choice(a);
        doc.set_choice_text(a, 2, "wait".to_owned());
        doc.set_link(a, Slot::Choice(2), Link::Node(b));
        doc.mark_clean();
        let texts = |doc: &DialogueDoc| {
            choice_pairs(doc, a)
                .into_iter()
                .map(|(t, _)| t)
                .collect::<Vec<_>>()
        };
        assert_eq!(texts(&doc), ["go", "stay", "wait"]);

        apply_edits(&mut doc, &[Edit::MoveChoice(a, 0, -1)]); // clamped at the top
        assert_eq!(
            texts(&doc),
            ["go", "stay", "wait"],
            "no movement past the start"
        );
        apply_edits(&mut doc, &[Edit::MoveChoice(a, 0, 1)]); // down
        assert_eq!(texts(&doc), ["stay", "go", "wait"]);
        apply_edits(&mut doc, &[Edit::MoveChoice(a, 2, 5)]); // clamped at the bottom
        assert_eq!(
            texts(&doc),
            ["stay", "go", "wait"],
            "no movement past the end"
        );
        apply_edits(&mut doc, &[Edit::MoveChoice(a, 2, -1)]); // up
        assert_eq!(texts(&doc), ["stay", "wait", "go"]);
        // Targets travel with the choice.
        assert_eq!(
            choice_pairs(&doc, a),
            [
                ("stay".to_owned(), Link::Node(b)),
                ("wait".to_owned(), Link::Node(b)),
                ("go".to_owned(), Link::End),
            ]
        );
    }

    #[test]
    fn set_link_works_in_both_modes_and_ignores_mismatched_slots() {
        // Next mode: Slot::Next rewrites the next link.
        let (mut doc, a, b) = base_doc();
        let revision = doc.revision;
        apply_edits(&mut doc, &[Edit::SetLink(a, Slot::Next, Link::End)]);
        assert!(matches!(
            &doc.node(a).unwrap().out,
            Outputs::Next(Link::End)
        ));
        assert_eq!(doc.revision, revision + 1);
        apply_edits(&mut doc, &[Edit::SetLink(a, Slot::Next, Link::Node(b))]);
        assert!(matches!(&doc.node(a).unwrap().out, Outputs::Next(Link::Node(k)) if *k == b));
        // A Slot::Choice on a Next node is a no-op (no bump).
        let revision = doc.revision;
        apply_edits(&mut doc, &[Edit::SetLink(a, Slot::Choice(0), Link::End)]);
        assert_eq!(doc.revision, revision, "mismatched slot does not bump");
        assert!(matches!(&doc.node(a).unwrap().out, Outputs::Next(_)));

        // Choices mode: Slot::Choice(i) rewrites that choice's target.
        let (mut doc, a, b) = choices_doc();
        apply_edits(
            &mut doc,
            &[Edit::SetLink(a, Slot::Choice(0), Link::Node(b))],
        );
        assert_eq!(choice_pairs(&doc, a)[0].1, Link::Node(b));
        apply_edits(&mut doc, &[Edit::SetLink(a, Slot::Choice(1), Link::End)]);
        assert_eq!(choice_pairs(&doc, a)[1].1, Link::End);
        // Slot::Next on a choices node is a no-op; an out-of-range index is a no-op.
        let revision = doc.revision;
        apply_edits(
            &mut doc,
            &[
                Edit::SetLink(a, Slot::Next, Link::End),
                Edit::SetLink(a, Slot::Choice(9), Link::End),
            ],
        );
        assert_eq!(doc.revision, revision, "no bump for inapplicable slots");
        assert_eq!(choice_pairs(&doc, a).len(), 2);
    }

    #[test]
    fn set_choice_text_and_labels_change_exactly_their_choice() {
        let (mut doc, a, _b) = choices_doc();
        let revision = doc.revision;
        let content_revision = doc.content_revision;
        assert!(!doc.dirty);
        apply_edits(
            &mut doc,
            &[
                Edit::SetChoiceText(a, 1, "renamed".to_owned()),
                Edit::SetChoiceLabels(a, 0, vec!["l1".to_owned(), "l2".to_owned()]),
            ],
        );
        let pairs = choice_pairs(&doc, a);
        assert_eq!(pairs[0].0, "go", "the other choice's text is untouched");
        assert_eq!(pairs[1].0, "renamed");
        match &doc.node(a).unwrap().out {
            Outputs::Choices(v) => {
                assert_eq!(v[0].labels, ["l1", "l2"]);
                assert!(
                    v[1].labels.is_empty(),
                    "the other choice's labels untouched"
                );
            }
            Outputs::Next(_) => panic!("expected choices"),
        }
        assert_eq!(doc.revision, revision + 2, "one bump per applied edit");
        assert_eq!(doc.content_revision, content_revision + 2);
        assert!(doc.dirty);
    }

    #[test]
    fn new_edit_variants_for_a_vanished_node_or_out_of_range_index_are_ignored() {
        let (mut doc, a, _b) = choices_doc();
        let ghost = NodeKey(999);
        let revision = doc.revision;
        let content_revision = doc.content_revision;
        assert!(!doc.dirty);
        apply_edits(
            &mut doc,
            &[
                Edit::ChangeMode(ghost, OutputsMode::Choices),
                Edit::SetLink(ghost, Slot::Next, Link::End),
                Edit::AddChoice(ghost),
                Edit::DeleteChoice(ghost, 0),
                Edit::MoveChoice(ghost, 0, 1),
                Edit::SetChoiceText(ghost, 0, "x".to_owned()),
                Edit::SetChoiceLabels(ghost, 0, vec!["l".to_owned()]),
                // Valid node, out-of-range choice indices.
                Edit::DeleteChoice(a, 99),
                Edit::MoveChoice(a, 99, 1),
                Edit::SetChoiceText(a, 99, "x".to_owned()),
                Edit::SetChoiceLabels(a, 99, vec!["l".to_owned()]),
                Edit::SetLink(a, Slot::Choice(99), Link::End),
            ],
        );
        assert_eq!(doc.revision, revision, "no bump");
        assert_eq!(doc.content_revision, content_revision, "no content bump");
        assert!(!doc.dirty, "not dirty");
        assert_eq!(choice_pairs(&doc, a).len(), 2, "choices untouched");
        assert!(doc.node(ghost).is_none());
    }

    #[test]
    fn add_choice_on_a_next_node_keeps_the_next_link_as_the_first_choice_target() {
        let (mut doc, a, b) = base_doc(); // a: Next(Node(b))
        let revision = doc.revision;
        apply_edits(&mut doc, &[Edit::AddChoice(a)]);
        match &doc.node(a).unwrap().out {
            Outputs::Choices(v) => {
                assert_eq!(v.len(), 1, "exactly ONE choice (never an empty list)");
                assert_eq!(v[0].target, Link::Node(b), "the next link is NOT lost");
                assert_eq!(v[0].text, "");
            }
            Outputs::Next(_) => panic!("expected choices"),
        }
        assert_eq!(doc.revision, revision + 1);

        // On a choices node it appends an empty choice.
        let (mut doc, a, _b) = choices_doc();
        apply_edits(&mut doc, &[Edit::AddChoice(a)]);
        assert_eq!(choice_pairs(&doc, a).len(), 3);
        assert_eq!(choice_pairs(&doc, a)[2], (String::new(), Link::End));
    }

    /// No sequence of edits may ever leave a node with `Outputs::Choices(vec![])`.
    fn assert_no_empty_choices(doc: &DialogueDoc) {
        for node in &doc.nodes {
            assert!(
                !matches!(&node.out, Outputs::Choices(v) if v.is_empty()),
                "node {} ended with an empty Choices list",
                node.key.0
            );
        }
    }

    #[test]
    fn no_edit_sequence_in_any_order_ever_leaves_an_empty_choices_list() {
        let (_doc, a, b) = base_doc();
        let pool: Vec<Edit> = vec![
            Edit::ChangeMode(a, OutputsMode::Choices),
            Edit::ChangeMode(a, OutputsMode::Next),
            Edit::AddChoice(a),
            Edit::DeleteChoice(a, 0),
            Edit::DeleteChoice(a, 1),
            Edit::DeleteChoice(a, 2),
            Edit::MoveChoice(a, 0, 1),
            Edit::MoveChoice(a, 1, -1),
            Edit::MoveChoice(a, 2, -5),
            Edit::SetLink(a, Slot::Next, Link::Node(b)),
            Edit::SetLink(a, Slot::Next, Link::End),
            Edit::SetLink(a, Slot::Choice(0), Link::Node(b)),
            Edit::SetLink(a, Slot::Choice(1), Link::End),
            Edit::SetChoiceText(a, 0, "x".to_owned()),
            Edit::SetChoiceLabels(a, 1, vec!["l".to_owned()]),
            Edit::ChangeMode(b, OutputsMode::Choices),
            Edit::AddChoice(b),
            Edit::DeleteChoice(b, 0),
        ];
        // All permutations of every 4-edit subset would be large; instead: all permutations of
        // the first 5 pool entries (120 sequences) plus 400 deterministic pseudo-random
        // sequences over the whole pool, checking the invariant after EVERY applied edit.
        let mut sequences: Vec<Vec<Edit>> = Vec::new();
        let head: Vec<Edit> = pool[..5].to_vec();
        permute(&head, 0, &mut sequences);
        let mut state = 0x2545F4914F6CDD1Du64;
        for _ in 0..400 {
            let mut seq = Vec::new();
            for _ in 0..12 {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                seq.push(pool[(state >> 33) as usize % pool.len()].clone());
            }
            sequences.push(seq);
        }
        for seq in &sequences {
            let (mut doc, _a, _b) = base_doc();
            for edit in seq {
                apply_edits(&mut doc, std::slice::from_ref(edit));
                assert_no_empty_choices(&doc);
            }
            assert_no_empty_choices(&doc);
        }
    }

    /// In-place Heap permutation of `items[from..]`, collecting every permutation.
    fn permute(items: &[Edit], from: usize, out: &mut Vec<Vec<Edit>>) {
        if from == items.len() {
            out.push(items.to_vec());
            return;
        }
        let mut v: Vec<Edit> = items.to_vec();
        for i in from..v.len() {
            v.swap(from, i);
            permute(&v, from + 1, out);
            v.swap(from, i);
        }
    }

    #[test]
    fn next_choices_next_round_trips_to_the_original_def() {
        let (mut doc, a, _b) = base_doc();
        let c = doc.add_node(Vec2::new(20.0, 0.0)); // a Next(End) node
        doc.mark_clean();
        let before = to_def(&doc);

        // Next(Node) -> Choices -> Next keeps the link; Next(End) -> Choices -> Next keeps End.
        apply_edits(
            &mut doc,
            &[
                Edit::ChangeMode(a, OutputsMode::Choices),
                Edit::ChangeMode(a, OutputsMode::Next),
                Edit::ChangeMode(c, OutputsMode::Choices),
                Edit::ChangeMode(c, OutputsMode::Next),
            ],
        );
        assert!(matches!(
            &doc.node(a).unwrap().out,
            Outputs::Next(Link::Node(_))
        ));
        assert!(matches!(
            &doc.node(c).unwrap().out,
            Outputs::Next(Link::End)
        ));
        assert_eq!(
            to_def(&doc),
            before,
            "the round trip reproduces the original def"
        );
    }

    /// The 3-level nested condition used by the when/effects tests:
    /// `all(any(flag, not(integer)), string_equals)`.
    fn nested_when() -> DialogueCondition {
        DialogueCondition::all(vec![
            DialogueCondition::any(vec![
                DialogueCondition::flag("has_pass"),
                DialogueCondition::not(DialogueCondition::integer("gold", DialogueCmp::Ge, 10)),
            ]),
            DialogueCondition::string_equals("name", "guard"),
        ])
    }

    /// One effect of every kind.
    fn all_effect_kinds() -> Vec<DialogueEffect> {
        vec![
            DialogueEffect::set_flag("f"),
            DialogueEffect::remove_flag("g"),
            DialogueEffect::set_integer("i", 5),
            DialogueEffect::add_integer("i", -2),
            DialogueEffect::set_scalar("x", 0.5),
            DialogueEffect::set_string("s", "v"),
        ]
    }

    #[test]
    fn set_choice_when_effects_and_on_enter_replace_whole_values_and_bump_like_content_edits() {
        let (mut doc, a, _b) = choices_doc();
        let revision = doc.revision;
        let content_revision = doc.content_revision;
        assert!(!doc.dirty, "the base doc is clean");
        let when = Some(nested_when());
        let effects = all_effect_kinds();
        let on_enter = vec![
            DialogueEffect::set_flag("entered"),
            DialogueEffect::add_integer("visits", 1),
        ];
        apply_edits(
            &mut doc,
            &[
                Edit::SetChoiceWhen(a, 0, when.clone()),
                Edit::SetChoiceEffects(a, 0, effects.clone()),
                Edit::SetOnEnter(a, on_enter.clone()),
            ],
        );
        match &doc.node(a).expect("node exists").out {
            Outputs::Choices(v) => {
                assert_eq!(v[0].when, when, "the whole when value is replaced");
                assert_eq!(v[0].effects, effects, "the whole effects list is replaced");
                assert!(v[1].when.is_none(), "the other choice is untouched");
                assert!(v[1].effects.is_empty(), "the other choice is untouched");
            }
            Outputs::Next(_) => panic!("expected choices"),
        }
        assert_eq!(doc.node(a).unwrap().on_enter, on_enter);
        assert_eq!(doc.revision, revision + 3, "one bump per applied edit");
        assert_eq!(doc.content_revision, content_revision + 3);
        assert!(doc.dirty, "dirty is set");
    }

    #[test]
    fn set_choice_when_none_clears_the_condition_and_empty_vecs_clear_the_lists() {
        let (mut doc, a, _b) = choices_doc();
        apply_edits(
            &mut doc,
            &[
                Edit::SetChoiceWhen(a, 1, Some(DialogueCondition::flag("f"))),
                Edit::SetChoiceEffects(a, 1, vec![DialogueEffect::set_flag("g")]),
                Edit::SetOnEnter(a, vec![DialogueEffect::set_integer("i", 1)]),
            ],
        );
        apply_edits(
            &mut doc,
            &[
                Edit::SetChoiceWhen(a, 1, None),
                Edit::SetChoiceEffects(a, 1, Vec::new()),
                Edit::SetOnEnter(a, Vec::new()),
            ],
        );
        match &doc.node(a).expect("node exists").out {
            Outputs::Choices(v) => {
                assert_eq!(v[1].when, None, "None clears the condition");
                assert!(v[1].effects.is_empty(), "an empty Vec clears the effects");
            }
            Outputs::Next(_) => panic!("expected choices"),
        }
        assert!(
            doc.node(a).unwrap().on_enter.is_empty(),
            "empty Vec clears on_enter"
        );
    }

    #[test]
    fn when_effects_on_enter_edits_for_a_vanished_node_or_out_of_range_index_are_ignored() {
        let (mut doc, a, _b) = choices_doc();
        let ghost = NodeKey(999);
        let revision = doc.revision;
        let content_revision = doc.content_revision;
        assert!(!doc.dirty);
        apply_edits(
            &mut doc,
            &[
                Edit::SetChoiceWhen(ghost, 0, Some(DialogueCondition::flag("f"))),
                Edit::SetChoiceEffects(ghost, 0, all_effect_kinds()),
                Edit::SetOnEnter(ghost, vec![DialogueEffect::set_flag("f")]),
                // Valid node, out-of-range choice indices.
                Edit::SetChoiceWhen(a, 99, Some(DialogueCondition::flag("f"))),
                Edit::SetChoiceEffects(a, 99, vec![DialogueEffect::set_flag("g")]),
            ],
        );
        assert_eq!(doc.revision, revision, "no bump");
        assert_eq!(doc.content_revision, content_revision, "no content bump");
        assert!(!doc.dirty, "not dirty");
        match &doc.node(a).expect("node exists").out {
            Outputs::Choices(v) => {
                assert!(v.iter().all(|c| c.when.is_none() && c.effects.is_empty()));
            }
            Outputs::Next(_) => panic!("expected choices"),
        }
        assert!(doc.node(a).unwrap().on_enter.is_empty());
        assert!(doc.node(ghost).is_none());
    }

    #[test]
    fn save_path_doc_with_nested_condition_all_effect_kinds_and_on_enter_gate_round_trip() {
        let (mut doc, a, _b) = choices_doc();
        apply_edits(
            &mut doc,
            &[
                Edit::SetChoiceWhen(a, 0, Some(nested_when())),
                Edit::SetChoiceEffects(a, 0, all_effect_kinds()),
                Edit::SetOnEnter(
                    a,
                    vec![
                        DialogueEffect::set_flag("entered"),
                        DialogueEffect::add_integer("visits", 1),
                    ],
                ),
            ],
        );
        // The save gate accepts the doc.
        assert!(
            engine_gate(&doc).is_ok(),
            "engine_gate accepts the edited doc: {:?}",
            engine_gate(&doc).err()
        );
        // to_def -> pretty JSON -> from_str reproduces the def exactly.
        let def = to_def(&doc);
        let json = serde_json::to_string_pretty(&def).expect("the def serialises");
        let back: DialogueDef = serde_json::from_str(&json).expect("the saved JSON parses");
        assert_eq!(back, def, "the JSON round trip reproduces the def exactly");
        // And the engine builds a tree from the saved def.
        assert!(
            DialogueTree::try_from(back).is_ok(),
            "DialogueTree::try_from accepts the saved def"
        );
    }
}

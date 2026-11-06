//! One open dialogue document and the [`Workspace`] that holds all of them.
//!
//! [`DialogueDoc`] is the single source of truth for a tab. There is deliberately no public `&mut EdNode`
//! accessor: every change goes through a named mutator so `revision`/`dirty` bookkeeping stays in one
//! place. Pure queries (`node`, field reads) never touch `revision`/`dirty`.

use std::path::{Path, PathBuf};

use aberredengine::prelude::{DialogueCondition, DialogueEffect, Vec2};

/// Stable identifier of an [`EdNode`] within its doc. Keys are never reused: [`DialogueDoc::fresh_key`]
/// only moves `next_key` forward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeKey(pub u64);

/// Where a node's `next` output or a choice's target leads.
#[derive(Debug, Clone, PartialEq)]
pub enum Link {
    /// Terminal: no outgoing edge (`next: None` in the engine def).
    End,
    /// A live edge to another node of the same doc.
    Node(NodeKey),
    /// A reference to an id that no longer resolves (a dangling `next` in the loaded file). Kept visible
    /// so the linter can report it and `to_def` writes it back unchanged for the engine's own validation.
    Dangling(String),
}

/// One player choice at a choice node.
#[derive(Debug, Clone, PartialEq)]
pub struct EdChoice {
    pub text: String,
    pub target: Link,
    pub when: Option<DialogueCondition>,
    pub effects: Vec<DialogueEffect>,
    pub labels: Vec<String>,
}

impl EdChoice {
    fn empty() -> Self {
        Self {
            text: String::new(),
            target: Link::End,
            when: None,
            effects: Vec::new(),
            labels: Vec::new(),
        }
    }
}

/// A node's outgoing edges: either a single unconditional `next` ([`Link::End`] = terminal node) or a
/// list of choices.
#[derive(Debug, Clone, PartialEq)]
pub enum Outputs {
    Next(Link),
    Choices(Vec<EdChoice>),
}

/// One dialogue node of the graph.
#[derive(Debug, Clone, PartialEq)]
pub struct EdNode {
    pub key: NodeKey,
    pub id: String,
    pub speaker: Option<String>,
    pub text: String,
    pub labels: Vec<String>,
    pub on_enter: Vec<DialogueEffect>,
    pub out: Outputs,
    /// Canvas position. Never saved; auto-layout assigns it on load.
    pub pos: Vec2,
}

/// Which output slot of a node [`DialogueDoc::set_link`] addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Next,
    Choice(usize),
}

/// Camera of one doc's graph view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CamState {
    pub target: Vec2,
    pub zoom: f32,
}

impl Default for CamState {
    fn default() -> Self {
        Self {
            target: Vec2::ZERO,
            zoom: 1.0,
        }
    }
}

/// One open dialogue document: the graph plus editor-side state (selection, camera, load warnings).
///
/// Mutation rule: every mutator bumps `revision`; every mutator also sets `dirty = true` except
/// [`DialogueDoc::set_node_pos`] and [`DialogueDoc::select`] (view-only changes — positions are never
/// saved, selection is not content) and [`DialogueDoc::mark_clean`] (clears `dirty`). A call that cannot
/// apply (unknown key, inapplicable slot) is a no-op and does not bump. Pure queries never bump.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DialogueDoc {
    pub nodes: Vec<EdNode>,
    pub start: Option<NodeKey>,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub revision: u64,
    pub selected: Option<NodeKey>,
    pub camera: CamState,
    pub load_warnings: Vec<String>,
    pub next_key: u64,
}

impl DialogueDoc {
    /// A brand-new empty document.
    pub fn new_empty() -> Self {
        Self::default()
    }

    /// Read-only node lookup.
    pub fn node(&self, key: NodeKey) -> Option<&EdNode> {
        self.nodes.iter().find(|n| n.key == key)
    }

    /// Allocates the next never-before-used key. Pure allocator: does not touch `revision`/`dirty`.
    pub fn fresh_key(&mut self) -> NodeKey {
        self.next_key += 1;
        NodeKey(self.next_key)
    }

    /// Appends a terminal node at `pos` with a unique `node_N` id (smallest N >= 1 not used by any
    /// existing id). The first node added to a doc with no start becomes `start`.
    pub fn add_node(&mut self, pos: Vec2) -> NodeKey {
        let key = self.fresh_key();
        let id = self.fresh_id();
        self.nodes.push(EdNode {
            key,
            id,
            speaker: None,
            text: String::new(),
            labels: Vec::new(),
            on_enter: Vec::new(),
            out: Outputs::Next(Link::End),
            pos,
        });
        if self.start.is_none() {
            self.start = Some(key);
        }
        self.bump();
        key
    }

    /// Removes the node; every `Link::Node(key)` pointing at it becomes `Link::End`, `start`/`selected`
    /// are cleared if they referenced it.
    pub fn delete_node(&mut self, key: NodeKey) {
        if self.node(key).is_none() {
            return;
        }
        self.nodes.retain(|n| n.key != key);
        for node in &mut self.nodes {
            match &mut node.out {
                Outputs::Next(link) => clear_link(link, key),
                Outputs::Choices(choices) => {
                    for choice in choices.iter_mut() {
                        clear_link(&mut choice.target, key);
                    }
                }
            }
        }
        if self.start == Some(key) {
            self.start = None;
        }
        if self.selected == Some(key) {
            self.selected = None;
        }
        self.bump();
    }

    /// Sets the start node (must exist).
    pub fn set_start(&mut self, key: NodeKey) {
        if self.node(key).is_none() {
            return;
        }
        self.start = Some(key);
        self.bump();
    }

    /// Renames the node. No uniqueness check here — the linter reports duplicate ids.
    pub fn rename_node(&mut self, key: NodeKey, id: String) {
        match self.nodes.iter_mut().find(|n| n.key == key) {
            Some(node) => node.id = id,
            None => return,
        }
        self.bump();
    }

    /// Moves the node on the canvas. Bumps `revision` but NOT `dirty`: positions are never saved.
    pub fn set_node_pos(&mut self, key: NodeKey, pos: Vec2) {
        match self.nodes.iter_mut().find(|n| n.key == key) {
            Some(node) => node.pos = pos,
            None => return,
        }
        self.bump_view();
    }

    pub fn set_text(&mut self, key: NodeKey, text: String) {
        match self.nodes.iter_mut().find(|n| n.key == key) {
            Some(node) => node.text = text,
            None => return,
        }
        self.bump();
    }

    pub fn set_speaker(&mut self, key: NodeKey, speaker: Option<String>) {
        match self.nodes.iter_mut().find(|n| n.key == key) {
            Some(node) => node.speaker = speaker,
            None => return,
        }
        self.bump();
    }

    pub fn set_labels(&mut self, key: NodeKey, labels: Vec<String>) {
        match self.nodes.iter_mut().find(|n| n.key == key) {
            Some(node) => node.labels = labels,
            None => return,
        }
        self.bump();
    }

    pub fn set_on_enter(&mut self, key: NodeKey, on_enter: Vec<DialogueEffect>) {
        match self.nodes.iter_mut().find(|n| n.key == key) {
            Some(node) => node.on_enter = on_enter,
            None => return,
        }
        self.bump();
    }

    pub fn set_outputs(&mut self, key: NodeKey, out: Outputs) {
        match self.nodes.iter_mut().find(|n| n.key == key) {
            Some(node) => node.out = out,
            None => return,
        }
        self.bump();
    }

    /// Rewrites one output slot: `Slot::Next` on a `Next` node, `Slot::Choice(i)` on a choices node.
    pub fn set_link(&mut self, key: NodeKey, slot: Slot, link: Link) {
        let node = match self.nodes.iter_mut().find(|n| n.key == key) {
            Some(node) => node,
            None => return,
        };
        match (&mut node.out, slot) {
            (Outputs::Next(l), Slot::Next) => *l = link,
            (Outputs::Choices(v), Slot::Choice(i)) => match v.get_mut(i) {
                Some(choice) => choice.target = link,
                None => return,
            },
            _ => return,
        }
        self.bump();
    }

    /// Adds a choice: converts a `Next` node to `Choices` with one empty choice, appends an empty
    /// choice to a choices node.
    pub fn add_choice(&mut self, key: NodeKey) {
        match self.nodes.iter_mut().find(|n| n.key == key) {
            Some(node) => {
                if matches!(&node.out, Outputs::Next(_)) {
                    node.out = Outputs::Choices(vec![EdChoice::empty()]);
                } else if let Outputs::Choices(v) = &mut node.out {
                    v.push(EdChoice::empty());
                }
            }
            None => return,
        }
        self.bump();
    }

    /// Deletes choice `index`; deleting the last choice converts the node to `Outputs::Next(Link::End)`.
    pub fn delete_choice(&mut self, key: NodeKey, index: usize) {
        let node = match self.nodes.iter_mut().find(|n| n.key == key) {
            Some(node) => node,
            None => return,
        };
        let became_empty = match &mut node.out {
            Outputs::Choices(v) if index < v.len() => {
                v.remove(index);
                v.is_empty()
            }
            _ => return,
        };
        if became_empty {
            node.out = Outputs::Next(Link::End);
        }
        self.bump();
    }

    /// Moves choice `index` by `delta` positions, clamped to the list (no movement past either end).
    pub fn move_choice(&mut self, key: NodeKey, index: usize, delta: i32) {
        match self.nodes.iter_mut().find(|n| n.key == key) {
            Some(node) => match &mut node.out {
                Outputs::Choices(v) if index < v.len() => {
                    let last = (v.len() - 1) as i64;
                    let new = (index as i64 + delta as i64).clamp(0, last) as usize;
                    if new != index {
                        let choice = v.remove(index);
                        v.insert(new, choice);
                    }
                }
                _ => return,
            },
            None => return,
        }
        self.bump();
    }

    pub fn set_choice_text(&mut self, key: NodeKey, index: usize, text: String) {
        match self.choice_mut(key, index) {
            Some(choice) => choice.text = text,
            None => return,
        }
        self.bump();
    }

    pub fn set_choice_when(&mut self, key: NodeKey, index: usize, when: Option<DialogueCondition>) {
        match self.choice_mut(key, index) {
            Some(choice) => choice.when = when,
            None => return,
        }
        self.bump();
    }

    pub fn set_choice_effects(&mut self, key: NodeKey, index: usize, effects: Vec<DialogueEffect>) {
        match self.choice_mut(key, index) {
            Some(choice) => choice.effects = effects,
            None => return,
        }
        self.bump();
    }

    pub fn set_choice_labels(&mut self, key: NodeKey, index: usize, labels: Vec<String>) {
        match self.choice_mut(key, index) {
            Some(choice) => choice.labels = labels,
            None => return,
        }
        self.bump();
    }

    /// Selects a node (or clears the selection). Bumps `revision` but NOT `dirty`.
    pub fn select(&mut self, key: Option<NodeKey>) {
        self.selected = key;
        self.bump_view();
    }

    /// Clears the dirty flag after a save.
    pub fn mark_clean(&mut self) {
        self.dirty = false;
        self.bump_view();
    }

    fn choice_mut(&mut self, key: NodeKey, index: usize) -> Option<&mut EdChoice> {
        let node = self.nodes.iter_mut().find(|n| n.key == key)?;
        match &mut node.out {
            Outputs::Choices(v) => v.get_mut(index),
            Outputs::Next(_) => None,
        }
    }

    /// Content change: `revision += 1`, `dirty = true`.
    fn bump(&mut self) {
        self.revision += 1;
        self.dirty = true;
    }

    /// View-only change: `revision += 1`, `dirty` untouched.
    fn bump_view(&mut self) {
        self.revision += 1;
    }

    /// Smallest `node_N` (N >= 1) not used by any existing node id.
    fn fresh_id(&self) -> String {
        let mut n = 1u64;
        loop {
            let candidate = format!("node_{n}");
            if !self.nodes.iter().any(|node| node.id == candidate) {
                return candidate;
            }
            n += 1;
        }
    }
}

/// Rewrites `link` to `Link::End` when it points at `deleted`.
fn clear_link(link: &mut Link, deleted: NodeKey) {
    if *link == Link::Node(deleted) {
        *link = Link::End;
    }
}

/// All open documents plus the index of the active one. Shared between the render-thread GUI and
/// logic-thread systems through [`crate::state::WorkspaceMutex`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Workspace {
    pub docs: Vec<DialogueDoc>,
    pub active: usize,
}

impl Workspace {
    pub fn active_doc(&self) -> Option<&DialogueDoc> {
        self.docs.get(self.active)
    }

    pub fn active_doc_mut(&mut self) -> Option<&mut DialogueDoc> {
        self.docs.get_mut(self.active)
    }

    /// Appends a doc and makes it active.
    pub fn add_doc(&mut self, doc: DialogueDoc) {
        self.docs.push(doc);
        self.active = self.docs.len() - 1;
    }

    /// Removes the doc at `index`, keeping `active` valid (and pointing at the same doc when possible).
    pub fn close_doc(&mut self, index: usize) {
        if index >= self.docs.len() {
            return;
        }
        self.docs.remove(index);
        if self.active > index {
            self.active -= 1;
        }
        if self.active >= self.docs.len() {
            self.active = self.docs.len().saturating_sub(1);
        }
    }

    /// Index of the first doc whose path equals `path`, if any.
    pub fn find_by_path(&self, path: &Path) -> Option<usize> {
        self.docs
            .iter()
            .position(|d| d.path.as_deref() == Some(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aberredengine::prelude::{DialogueCondition, DialogueEffect};

    /// A doc with `a --next--> b`, `b` having one choice ("go", target End), start = a, clean.
    fn base_doc() -> (DialogueDoc, NodeKey, NodeKey) {
        let mut doc = DialogueDoc::new_empty();
        let a = doc.add_node(Vec2::ZERO);
        let b = doc.add_node(Vec2::new(10.0, 0.0));
        doc.set_link(a, Slot::Next, Link::Node(b));
        doc.add_choice(b);
        doc.set_choice_text(b, 0, "go".to_owned());
        doc.mark_clean();
        (doc, a, b)
    }

    fn choice_texts(doc: &DialogueDoc, key: NodeKey) -> Vec<&str> {
        match &doc.node(key).expect("node exists").out {
            Outputs::Choices(v) => v.iter().map(|c| c.text.as_str()).collect(),
            Outputs::Next(_) => panic!("expected choices"),
        }
    }

    #[test]
    fn add_node_assigns_unique_ids_filling_gaps_and_first_node_becomes_start() {
        let mut doc = DialogueDoc::new_empty();
        assert_eq!(doc.start, None);
        let a = doc.add_node(Vec2::ZERO);
        assert_eq!(doc.node(a).map(|n| n.id.as_str()), Some("node_1"));
        assert_eq!(
            doc.start,
            Some(a),
            "first node in a doc with no start becomes start"
        );
        let b = doc.add_node(Vec2::ZERO);
        assert_eq!(doc.node(b).map(|n| n.id.as_str()), Some("node_2"));
        assert_eq!(doc.start, Some(a), "later nodes do not steal start");
        doc.delete_node(a);
        assert_eq!(doc.start, None, "deleting the start node clears start");
        let c = doc.add_node(Vec2::ZERO);
        assert_eq!(
            doc.node(c).map(|n| n.id.as_str()),
            Some("node_1"),
            "the freed id gap is refilled"
        );
        assert_ne!(c, a, "keys are never reused");
        assert_eq!(
            doc.start,
            Some(c),
            "first node added after start was cleared becomes start"
        );
    }

    #[test]
    fn delete_node_rewrites_incoming_links_to_end_and_clears_start_and_selected() {
        let (mut doc, a, b) = base_doc();
        let c = doc.add_node(Vec2::new(20.0, 0.0));
        doc.add_choice(c);
        doc.set_link(c, Slot::Choice(0), Link::Node(b)); // an incoming link to b
        doc.set_start(b);
        doc.select(Some(b));
        doc.delete_node(b);
        assert!(doc.node(b).is_none());
        assert!(
            matches!(doc.node(a).unwrap().out, Outputs::Next(Link::End)),
            "next link to the deleted node becomes End"
        );
        match &doc.node(c).unwrap().out {
            Outputs::Choices(v) => {
                assert!(matches!(v[0].target, Link::End), "choice link becomes End")
            }
            Outputs::Next(_) => panic!("expected choices"),
        }
        assert_eq!(doc.start, None);
        assert_eq!(doc.selected, None);
    }

    #[test]
    fn add_choice_converts_next_and_appends_to_choices() {
        let (mut doc, a, b) = base_doc();
        doc.add_choice(a); // a is Next -> Choices with one empty choice
        match &doc.node(a).unwrap().out {
            Outputs::Choices(v) => {
                assert_eq!(v.len(), 1);
                assert_eq!(v[0].text, "");
                assert!(matches!(v[0].target, Link::End));
                assert!(v[0].when.is_none());
            }
            Outputs::Next(_) => panic!("expected choices"),
        }
        doc.add_choice(b); // b already has choices -> appends
        assert_eq!(choice_texts(&doc, b).len(), 2);
    }

    #[test]
    fn delete_last_choice_converts_the_node_to_next_end() {
        let (mut doc, _a, b) = base_doc();
        assert_eq!(choice_texts(&doc, b), ["go"]);
        doc.delete_choice(b, 0);
        assert!(matches!(doc.node(b).unwrap().out, Outputs::Next(Link::End)));
    }

    #[test]
    fn move_choice_reorders_and_clamps_at_the_ends() {
        let (mut doc, _a, b) = base_doc();
        doc.add_choice(b);
        doc.add_choice(b);
        doc.set_choice_text(b, 0, "zero".to_owned());
        doc.set_choice_text(b, 1, "one".to_owned());
        doc.set_choice_text(b, 2, "two".to_owned());
        doc.move_choice(b, 0, 1);
        assert_eq!(choice_texts(&doc, b), ["one", "zero", "two"]);
        doc.move_choice(b, 2, -5); // clamped to index 0
        assert_eq!(choice_texts(&doc, b), ["two", "one", "zero"]);
        doc.move_choice(b, 0, -1); // no-op at the start
        assert_eq!(choice_texts(&doc, b), ["two", "one", "zero"]);
        doc.move_choice(b, 2, 99); // clamped to the end
        assert_eq!(choice_texts(&doc, b), ["two", "one", "zero"]);
    }

    #[test]
    fn every_mutator_bumps_revision_and_dirty_except_pos_select_and_mark_clean() {
        enum Dirty {
            Set,
            Untouched,
            Cleared,
        }
        type Mutator = fn(&mut DialogueDoc, NodeKey, NodeKey);
        let cases: Vec<(&str, Dirty, Mutator)> = vec![
            ("add_node", Dirty::Set, |d, _, _| {
                d.add_node(Vec2::ZERO);
            }),
            ("delete_node", Dirty::Set, |d, a, _| d.delete_node(a)),
            ("set_start", Dirty::Set, |d, _, b| d.set_start(b)),
            ("rename_node", Dirty::Set, |d, a, _| {
                d.rename_node(a, "renamed".to_owned())
            }),
            ("set_node_pos", Dirty::Untouched, |d, a, _| {
                d.set_node_pos(a, Vec2::new(5.0, 5.0))
            }),
            ("set_text", Dirty::Set, |d, a, _| {
                d.set_text(a, "t".to_owned())
            }),
            ("set_speaker", Dirty::Set, |d, a, _| {
                d.set_speaker(a, Some("S".to_owned()))
            }),
            ("set_labels", Dirty::Set, |d, a, _| {
                d.set_labels(a, vec!["l".to_owned()])
            }),
            ("set_on_enter", Dirty::Set, |d, a, _| {
                d.set_on_enter(a, vec![DialogueEffect::set_flag("f")])
            }),
            ("set_outputs", Dirty::Set, |d, a, _| {
                d.set_outputs(a, Outputs::Next(Link::End))
            }),
            ("set_link next", Dirty::Set, |d, a, _| {
                d.set_link(a, Slot::Next, Link::End)
            }),
            ("set_link choice", Dirty::Set, |d, _, b| {
                d.set_link(b, Slot::Choice(0), Link::End)
            }),
            ("add_choice", Dirty::Set, |d, a, _| d.add_choice(a)),
            ("delete_choice", Dirty::Set, |d, _, b| d.delete_choice(b, 0)),
            ("move_choice", Dirty::Set, |d, _, b| d.move_choice(b, 0, -1)),
            ("set_choice_text", Dirty::Set, |d, _, b| {
                d.set_choice_text(b, 0, "x".to_owned())
            }),
            ("set_choice_when", Dirty::Set, |d, _, b| {
                d.set_choice_when(b, 0, Some(DialogueCondition::flag("f")))
            }),
            ("set_choice_effects", Dirty::Set, |d, _, b| {
                d.set_choice_effects(b, 0, vec![DialogueEffect::set_flag("g")])
            }),
            ("set_choice_labels", Dirty::Set, |d, _, b| {
                d.set_choice_labels(b, 0, vec!["l".to_owned()])
            }),
            ("select", Dirty::Untouched, |d, a, _| d.select(Some(a))),
            ("mark_clean", Dirty::Cleared, |d, _, _| d.mark_clean()),
        ];
        for (name, dirty_expectation, op) in cases {
            let (mut doc, a, b) = base_doc();
            let revision = doc.revision;
            assert!(!doc.dirty, "base doc is clean");
            op(&mut doc, a, b);
            assert_eq!(
                doc.revision,
                revision + 1,
                "{name}: revision must bump exactly once"
            );
            match dirty_expectation {
                Dirty::Set => assert!(doc.dirty, "{name}: must set dirty"),
                Dirty::Untouched => assert!(!doc.dirty, "{name}: must not touch dirty"),
                Dirty::Cleared => assert!(!doc.dirty, "{name}: must clear dirty"),
            }
        }
    }

    #[test]
    fn mutators_on_unknown_keys_are_no_ops() {
        let (mut doc, _a, _b) = base_doc();
        let revision = doc.revision;
        let ghost = NodeKey(999);
        doc.delete_node(ghost);
        doc.set_start(ghost);
        doc.rename_node(ghost, "x".to_owned());
        doc.set_node_pos(ghost, Vec2::ZERO);
        doc.set_text(ghost, "x".to_owned());
        doc.set_speaker(ghost, None);
        doc.set_labels(ghost, vec![]);
        doc.set_on_enter(ghost, vec![]);
        doc.set_outputs(ghost, Outputs::Next(Link::End));
        doc.set_link(ghost, Slot::Next, Link::End);
        doc.add_choice(ghost);
        doc.delete_choice(ghost, 0);
        doc.move_choice(ghost, 0, 1);
        doc.set_choice_text(ghost, 0, "x".to_owned());
        doc.set_choice_when(ghost, 0, None);
        doc.set_choice_effects(ghost, 0, vec![]);
        doc.set_choice_labels(ghost, 0, vec![]);
        assert_eq!(doc.revision, revision, "no-op calls must not bump revision");
    }

    #[test]
    fn workspace_add_close_keeps_active_valid_and_find_by_path_works() {
        let mut ws = Workspace::default();
        assert!(ws.active_doc().is_none());
        assert!(ws.active_doc_mut().is_none());
        ws.add_doc(DialogueDoc::new_empty());
        ws.add_doc(DialogueDoc::new_empty());
        ws.add_doc(DialogueDoc::new_empty());
        assert_eq!(ws.docs.len(), 3);
        assert_eq!(ws.active, 2, "add_doc makes the new doc active");
        assert!(ws.active_doc().is_some());

        ws.docs[0].path = Some(PathBuf::from("/tmp/one.json"));
        ws.docs[2].path = Some(PathBuf::from("/tmp/three.json"));
        assert_eq!(ws.find_by_path(Path::new("/tmp/one.json")), Some(0));
        assert_eq!(ws.find_by_path(Path::new("/tmp/three.json")), Some(2));
        assert_eq!(ws.find_by_path(Path::new("/tmp/missing.json")), None);

        ws.close_doc(2); // closing the active (last) doc moves active to the new last
        assert_eq!(ws.active, 1);
        assert_eq!(ws.docs.len(), 2);
        assert!(ws.active_doc().is_some());

        ws.close_doc(0); // closing an earlier doc keeps active on the same doc (shifted down)
        assert_eq!(ws.active, 0);
        assert_eq!(ws.docs.len(), 1);

        let key = ws.active_doc_mut().unwrap().add_node(Vec2::ZERO);
        assert!(ws.active_doc().unwrap().node(key).is_some());

        ws.close_doc(0);
        assert_eq!(ws.active, 0);
        assert!(
            ws.active_doc().is_none(),
            "empty workspace has no active doc"
        );
        ws.close_doc(0); // out of range: no-op
        assert_eq!(ws.docs.len(), 0);
    }
}

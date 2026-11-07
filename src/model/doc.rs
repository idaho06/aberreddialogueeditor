//! One open dialogue document and the [`Workspace`] that holds all of them.
//!
//! [`DialogueDoc`] is the single source of truth for a tab. There is deliberately no public `&mut EdNode`
//! accessor: every change goes through a named mutator so `revision`/`content_revision`/`dirty`
//! bookkeeping stays in one place. Pure queries (`node`, field reads) never touch `revision`/`dirty`.

use std::path::{Path, PathBuf};

use aberredengine::prelude::{DialogueCondition, DialogueEffect, Vec2};

use crate::model::lint::{Problem, Severity, lint};
use crate::model::notify::Notification;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
/// saved, selection is not content) and [`DialogueDoc::mark_clean`] (clears `dirty`), and except
/// [`DialogueDoc::set_camera`], which bumps nothing at all: the camera is pure view state, so it must
/// not invalidate the lint cache or trigger a projection. A call that cannot apply (unknown key,
/// inapplicable slot) is a no-op and does not bump. Pure queries never bump.
///
/// Two revision counters: `revision` is bumped by EVERY mutator and is the graph-projection key;
/// `content_revision` is bumped only by content mutators (the private `bump`), never by the view-only
/// ones or `set_camera` — the lint cache is keyed on it, because lint depends only on node content,
/// never on positions, selection or camera.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DialogueDoc {
    /// Unique id of this doc within its [`Workspace`] (assigned by [`Workspace::add_doc`],
    /// starting at 1). `0` for a doc that was never added to a workspace; the graph projection
    /// treats `doc_id == 0` with no content as "nothing shown".
    pub doc_id: u64,
    pub nodes: Vec<EdNode>,
    pub start: Option<NodeKey>,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub revision: u64,
    /// Content-only revision: incremented only by content mutators (the private `bump`), never by
    /// `bump_view` (view-only changes) and never by [`DialogueDoc::set_camera`]. The lint cache is
    /// keyed on this — lint depends only on node content, never on pos/selection/camera.
    pub content_revision: u64,
    pub selected: Option<NodeKey>,
    pub camera: CamState,
    pub load_warnings: Vec<String>,
    pub next_key: u64,
    /// Cached [`lint`] result for `content_revision`: `None` until the first [`DialogueDoc::problems`]
    /// call, then `(content_revision, problems)`; recomputed only when `content_revision` differs
    /// (view-only changes never invalidate it).
    pub lint_cache: Option<(u64, Vec<Problem>)>,
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
        self.edit_node(key, |node| node.id = id);
    }

    /// Moves the node on the canvas. Bumps `revision` but NOT `dirty` or `content_revision`
    /// (positions are never saved and never affect lint).
    pub fn set_node_pos(&mut self, key: NodeKey, pos: Vec2) {
        let Some(node) = self.node_mut(key) else {
            return;
        };
        node.pos = pos;
        self.bump_view();
    }

    pub fn set_text(&mut self, key: NodeKey, text: String) {
        self.edit_node(key, |node| node.text = text);
    }

    pub fn set_speaker(&mut self, key: NodeKey, speaker: Option<String>) {
        self.edit_node(key, |node| node.speaker = speaker);
    }

    pub fn set_labels(&mut self, key: NodeKey, labels: Vec<String>) {
        self.edit_node(key, |node| node.labels = labels);
    }

    pub fn set_on_enter(&mut self, key: NodeKey, on_enter: Vec<DialogueEffect>) {
        self.edit_node(key, |node| node.on_enter = on_enter);
    }

    pub fn set_outputs(&mut self, key: NodeKey, out: Outputs) {
        self.edit_node(key, |node| node.out = out);
    }

    /// Rewrites one output slot: `Slot::Next` on a `Next` node, `Slot::Choice(i)` on a choices node.
    /// A slot that does not apply (mismatched mode, out-of-range index) is a no-op without a bump.
    pub fn set_link(&mut self, key: NodeKey, slot: Slot, link: Link) {
        let Some(node) = self.node_mut(key) else {
            return;
        };
        let applied = match (&mut node.out, slot) {
            (Outputs::Next(l), Slot::Next) => {
                *l = link;
                true
            }
            (Outputs::Choices(v), Slot::Choice(i)) => match v.get_mut(i) {
                Some(choice) => {
                    choice.target = link;
                    true
                }
                None => false,
            },
            _ => false,
        };
        if applied {
            self.bump();
        }
    }

    /// Adds a choice: converts a `Next` node to `Choices` with one empty choice (the next link is
    /// dropped), appends an empty choice to a choices node.
    pub fn add_choice(&mut self, key: NodeKey) {
        self.edit_node(key, |node| {
            if matches!(&node.out, Outputs::Next(_)) {
                node.out = Outputs::Choices(vec![EdChoice::empty()]);
            } else if let Outputs::Choices(v) = &mut node.out {
                v.push(EdChoice::empty());
            }
        });
    }

    /// Deletes choice `index`; deleting the last choice converts the node to `Outputs::Next(Link::End)`.
    pub fn delete_choice(&mut self, key: NodeKey, index: usize) {
        let Some(node) = self.node_mut(key) else {
            return;
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
        let Some(node) = self.node_mut(key) else {
            return;
        };
        match &mut node.out {
            Outputs::Choices(v) if index < v.len() => {
                let last = (v.len() - 1) as i64;
                let new = (index as i64 + delta as i64).clamp(0, last) as usize;
                if new != index {
                    let choice = v.remove(index);
                    v.insert(new, choice);
                }
            }
            _ => return,
        }
        self.bump();
    }

    pub fn set_choice_text(&mut self, key: NodeKey, index: usize, text: String) {
        self.edit_choice(key, index, |choice| choice.text = text);
    }

    pub fn set_choice_when(&mut self, key: NodeKey, index: usize, when: Option<DialogueCondition>) {
        self.edit_choice(key, index, |choice| choice.when = when);
    }

    pub fn set_choice_effects(&mut self, key: NodeKey, index: usize, effects: Vec<DialogueEffect>) {
        self.edit_choice(key, index, |choice| choice.effects = effects);
    }

    pub fn set_choice_labels(&mut self, key: NodeKey, index: usize, labels: Vec<String>) {
        self.edit_choice(key, index, |choice| choice.labels = labels);
    }

    /// Selects a node (or clears the selection). Bumps `revision` but NOT `dirty` or
    /// `content_revision`.
    pub fn select(&mut self, key: Option<NodeKey>) {
        self.selected = key;
        self.bump_view();
    }

    /// Clears the dirty flag after a save.
    pub fn mark_clean(&mut self) {
        self.dirty = false;
        self.bump_view();
    }

    /// Sets the graph-view camera. View-only like [`DialogueDoc::set_node_pos`]/[`select`], but bumps
    /// NOTHING — not even `revision` or `content_revision`: the camera is view state, so the lint
    /// cache stays valid and no projection is triggered.
    pub fn set_camera(&mut self, camera: CamState) {
        self.camera = camera;
    }

    /// The doc's current lint problems, cached per `content_revision`: recomputed only when
    /// `content_revision` changed since the last call — view-only changes (position, selection,
    /// camera) never trigger a recompute. A fresh doc (`content_revision == 0`, cache `None`)
    /// computes on the first call.
    pub fn problems(&mut self) -> &[Problem] {
        let stale = self
            .lint_cache
            .as_ref()
            .is_none_or(|(cached, _)| *cached != self.content_revision);
        if stale {
            let problems = lint(self);
            let errors = problems
                .iter()
                .filter(|p| p.severity == Severity::Error)
                .count();
            log::debug!(
                "lint cache recomputed: content_revision={}, {errors} errors, {} warnings",
                self.content_revision,
                problems.len() - errors
            );
            self.lint_cache = Some((self.content_revision, problems));
        }
        self.lint_cache
            .as_ref()
            .map_or(&[], |(_, problems)| problems.as_slice())
    }

    /// Mutable node lookup (private: there is no public `&mut EdNode` accessor — every change
    /// goes through a named mutator so the bump bookkeeping stays in one place).
    fn node_mut(&mut self, key: NodeKey) -> Option<&mut EdNode> {
        self.nodes.iter_mut().find(|n| n.key == key)
    }

    fn choice_mut(&mut self, key: NodeKey, index: usize) -> Option<&mut EdChoice> {
        let node = self.node_mut(key)?;
        match &mut node.out {
            Outputs::Choices(v) => v.get_mut(index),
            Outputs::Next(_) => None,
        }
    }

    /// Runs `f` on the node `key`, then bumps as a CONTENT change (`bump`). Returns `false` and
    /// bumps nothing for an unknown key.
    fn edit_node(&mut self, key: NodeKey, f: impl FnOnce(&mut EdNode)) -> bool {
        let Some(node) = self.node_mut(key) else {
            return false;
        };
        f(node);
        self.bump();
        true
    }

    /// Twin of [`Self::edit_node`] for choice `index` of `key`: runs `f` on the choice, then the
    /// content `bump`. A `Next` node or an out-of-range index returns `false` and bumps nothing.
    fn edit_choice(&mut self, key: NodeKey, index: usize, f: impl FnOnce(&mut EdChoice)) -> bool {
        let Some(choice) = self.choice_mut(key, index) else {
            return false;
        };
        f(choice);
        self.bump();
        true
    }

    /// Content change: `revision += 1`, `content_revision += 1`, `dirty = true`.
    fn bump(&mut self) {
        self.revision += 1;
        self.content_revision += 1;
        self.dirty = true;
    }

    /// View-only change: `revision += 1` only; `content_revision` (the lint-cache key) and
    /// `dirty` stay untouched.
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

/// A request for the graph canvas's context menu, queued by the logic side (a right-click without
/// movement) and drained by the GUI, which opens the popup. Defined in the model so neither side
/// needs graph types: `world` is the canvas position of the press, `node` the hit node (if any).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContextMenuRequest {
    pub world: Vec2,
    pub node: Option<NodeKey>,
}

/// All open documents plus the index of the active one. Shared between the render-thread GUI and
/// logic-thread systems through [`crate::state::WorkspaceMutex`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Workspace {
    pub docs: Vec<DialogueDoc>,
    pub active: usize,
    /// Allocator for [`DialogueDoc::doc_id`]: the next id `add_doc` assigns (ids start at 1).
    pub next_doc_id: u64,
    /// Notification queue for the GUI's modal: each entry is one typed [`Notification`] (failed
    /// open/save, engine save-gate rejections, ...). The GUI drains it one entry at a time.
    pub notifications: Vec<Notification>,
    /// The Problems window is visible by default; this flag stores the INVERSE (hidden), so the
    /// derived `Default` (false) means "show the panel". Flipped by `EditorAction::ToggleProblems`
    /// (View > "Problems", Ctrl+Shift+P, the window's close button).
    pub problems_hidden: bool,
    /// Doc id whose close was requested while the doc is dirty; the GUI shows a confirm modal
    /// and either closes that tab or clears this. Keyed by the unique, never-reused `doc_id`
    /// (NOT a tab index — resolved with [`Workspace::position`] at use), so tabs shifting while
    /// the modal is open cannot redirect the confirm to the wrong tab.
    pub pending_close: Option<u64>,
    /// Set when quit was requested while any doc is dirty; the GUI shows a confirm modal.
    pub pending_quit: bool,
    /// Set by Help > "Controls"; the GUI shows the controls modal while this is true.
    pub show_controls: bool,
    /// Pending graph context-menu request (right-click without movement); the GUI consumes it.
    pub pending_context_menu: Option<ContextMenuRequest>,
}

impl Workspace {
    pub fn active_doc(&self) -> Option<&DialogueDoc> {
        self.docs.get(self.active)
    }

    pub fn active_doc_mut(&mut self) -> Option<&mut DialogueDoc> {
        self.docs.get_mut(self.active)
    }

    /// Appends a doc, assigns it the next unique `doc_id` (starting at 1) and makes it active.
    pub fn add_doc(&mut self, mut doc: DialogueDoc) {
        self.next_doc_id += 1;
        doc.doc_id = self.next_doc_id;
        self.docs.push(doc);
        self.active = self.docs.len() - 1;
    }

    /// Removes the doc at `index`, keeping `active` valid (and pointing at the same doc when possible).
    /// `pending_close` holds a `doc_id`, so it needs no index arithmetic here: it is cleared exactly
    /// when the closed doc IS the pending one. Without that, a close issued while the unsaved-changes
    /// modal is open would make the modal confirm the WRONG tab (and discard its changes).
    pub fn close_doc(&mut self, index: usize) {
        let Some(closed_id) = self.docs.get(index).map(|doc| doc.doc_id) else {
            return;
        };
        self.docs.remove(index);
        if self.active > index {
            self.active -= 1;
        }
        if self.active >= self.docs.len() {
            self.active = self.docs.len().saturating_sub(1);
        }
        if self.pending_close == Some(closed_id) {
            self.pending_close = None;
        }
    }

    /// Index of the still-open doc with `doc_id`, if any.
    pub fn position(&self, doc_id: u64) -> Option<usize> {
        self.docs.iter().position(|doc| doc.doc_id == doc_id)
    }

    /// Whether any modal is currently open: a queued notification, the unsaved-changes close or quit
    /// confirm, or Help > "Controls". The GUI gates keyboard-shortcut polling on this, because imgui
    /// modals block mouse input to other windows but NOT io-level key polling (`ui.is_key_pressed`
    /// still fires while a modal is open).
    pub fn has_modal(&self) -> bool {
        !self.notifications.is_empty()
            || self.pending_close.is_some()
            || self.pending_quit
            || self.show_controls
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
    use crate::model::lint::ProblemKind;
    use crate::model::notify::NotificationKind;
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

    /// Test-only: appends a sentinel to the cached problem list so a recompute is detectable
    /// (a recompute replaces the whole vector, a cache hit keeps the sentinel).
    fn poison_lint_cache(doc: &mut DialogueDoc) {
        let (_, problems) = doc
            .lint_cache
            .as_mut()
            .expect("the lint cache must exist after problems()");
        problems.push(Problem {
            severity: Severity::Warning,
            kind: ProblemKind::Unreachable,
            node: None,
            message: "poisoned".to_owned(),
        });
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
    fn set_camera_is_view_only_and_keeps_the_lint_cache_valid() {
        let (mut doc, _a, _b) = base_doc();
        let _ = doc.problems(); // fill the lint cache at the current revision
        let revision = doc.revision;
        let cached = doc.lint_cache.clone();
        assert!(cached.is_some(), "the cache is filled");
        assert!(!doc.dirty, "base doc is clean");
        doc.set_camera(CamState {
            target: Vec2::new(50.0, -20.0),
            zoom: 2.5,
        });
        assert_eq!(doc.camera.target, Vec2::new(50.0, -20.0));
        assert!((doc.camera.zoom - 2.5).abs() < 1e-6);
        assert_eq!(doc.revision, revision, "set_camera must not bump revision");
        assert!(!doc.dirty, "set_camera must not touch dirty");
        assert_eq!(doc.lint_cache, cached, "the lint cache is untouched");
        // problems() still serves the cached result (no recompute at an unchanged revision).
        let problems = doc.problems().len();
        doc.set_camera(CamState::default());
        assert_eq!(doc.problems().len(), problems);
        assert_eq!(doc.revision, revision);
        // A dirty doc stays dirty.
        doc.add_node(Vec2::new(1.0, 1.0));
        let revision = doc.revision;
        doc.set_camera(CamState::default());
        assert_eq!(doc.revision, revision);
        assert!(doc.dirty, "set_camera must not clear dirty either");
    }

    #[test]
    fn view_only_mutators_keep_the_lint_cache_and_content_revision_untouched() {
        let (mut doc, a, b) = base_doc();
        let _ = doc.problems(); // fill the lint cache
        poison_lint_cache(&mut doc); // so a recompute is detectable
        let cached = doc.lint_cache.clone();
        let revision = doc.revision;
        let content_revision = doc.content_revision;
        assert!(!doc.dirty, "base doc is clean");

        // set_node_pos and select still bump `revision` (the projection key) ...
        doc.set_node_pos(a, Vec2::new(42.0, 7.0));
        assert_eq!(
            doc.revision,
            revision + 1,
            "set_node_pos still bumps revision"
        );
        doc.select(Some(b));
        assert_eq!(doc.revision, revision + 2, "select still bumps revision");
        // ... and mark_clean too (it clears dirty via bump_view) ...
        doc.mark_clean();
        assert_eq!(
            doc.revision,
            revision + 3,
            "mark_clean still bumps revision"
        );
        assert!(!doc.dirty);
        // ... while set_camera bumps nothing at all.
        doc.set_camera(CamState {
            target: Vec2::new(9.0, 9.0),
            zoom: 1.5,
        });
        assert_eq!(
            doc.revision,
            revision + 3,
            "set_camera must not bump revision"
        );

        assert_eq!(
            doc.content_revision, content_revision,
            "view-only changes must never bump content_revision"
        );
        assert_eq!(doc.lint_cache, cached, "the cache tuple is untouched");
        let problems = doc.problems();
        assert!(
            problems.iter().any(|p| p.message == "poisoned"),
            "problems() must NOT recompute after view-only changes"
        );
        assert_eq!(doc.lint_cache, cached, "the cache tuple is still untouched");
    }

    #[test]
    fn content_mutators_bump_both_revisions_set_dirty_and_recompute_the_lint_cache() {
        type Mutator = fn(&mut DialogueDoc, NodeKey, NodeKey);
        let cases: Vec<(&str, Mutator)> = vec![
            ("set_text", |d, a, _| d.set_text(a, "changed".to_owned())),
            ("set_link", |d, a, _| d.set_link(a, Slot::Next, Link::End)),
            ("add_node", |d, _, _| {
                d.add_node(Vec2::new(3.0, 3.0));
            }),
            ("delete_node", |d, _, b| d.delete_node(b)),
        ];
        for (name, op) in cases {
            let (mut doc, a, b) = base_doc();
            let _ = doc.problems(); // fill the lint cache
            poison_lint_cache(&mut doc); // so a recompute is detectable
            let revision = doc.revision;
            let content_revision = doc.content_revision;
            assert!(!doc.dirty, "{name}: base doc is clean");
            op(&mut doc, a, b);
            assert_eq!(doc.revision, revision + 1, "{name}: revision bumps");
            assert_eq!(
                doc.content_revision,
                content_revision + 1,
                "{name}: content_revision bumps"
            );
            assert!(doc.dirty, "{name}: dirty is set");
            let problems = doc.problems();
            assert!(
                !problems.iter().any(|p| p.message == "poisoned"),
                "{name}: problems() must recompute after a content change"
            );
            assert_eq!(
                doc.lint_cache.as_ref().map(|(key, _)| *key),
                Some(doc.content_revision),
                "{name}: the cache is keyed on the new content_revision"
            );
        }
    }

    #[test]
    fn lint_results_after_a_view_only_change_match_a_fresh_lint() {
        let (mut doc, a, b) = base_doc();
        let before = doc.problems().to_vec();
        doc.set_node_pos(a, Vec2::new(100.0, -50.0));
        doc.select(Some(b));
        doc.set_camera(CamState {
            target: Vec2::new(1.0, 2.0),
            zoom: 0.5,
        });
        doc.mark_clean();
        let after = doc.problems().to_vec();
        assert_eq!(
            after, before,
            "view-only changes cannot change lint results"
        );
        assert_eq!(after, lint(&doc), "cached results equal a fresh lint(&doc)");
    }

    #[test]
    fn mutators_on_unknown_keys_are_no_ops() {
        let (mut doc, _a, _b) = base_doc();
        let revision = doc.revision;
        let content_revision = doc.content_revision;
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
        assert_eq!(
            doc.content_revision, content_revision,
            "no-op calls must not bump content_revision"
        );
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

    #[test]
    fn problems_panel_is_visible_by_default_and_notifications_start_empty() {
        let ws = Workspace::default();
        assert!(
            !ws.problems_hidden,
            "the derived Default must mean 'Problems window visible'"
        );
        assert!(
            ws.notifications.is_empty(),
            "a fresh workspace has no queued notifications"
        );
    }

    #[test]
    fn slot_is_hashable() {
        let mut set = std::collections::HashSet::new();
        set.insert(Slot::Next);
        set.insert(Slot::Choice(0));
        set.insert(Slot::Choice(3));
        assert!(set.contains(&Slot::Next));
        assert!(set.contains(&Slot::Choice(0)));
        assert!(!set.contains(&Slot::Choice(1)));
        assert_eq!(set.len(), 3);
    }

    #[test]
    fn add_doc_assigns_unique_doc_ids_starting_at_1() {
        assert_eq!(
            DialogueDoc::new_empty().doc_id,
            0,
            "a bare doc has no id yet"
        );
        let mut ws = Workspace::default();
        ws.add_doc(DialogueDoc::new_empty());
        ws.add_doc(DialogueDoc::new_empty());
        ws.add_doc(DialogueDoc::new_empty());
        assert_eq!(ws.docs[0].doc_id, 1);
        assert_eq!(ws.docs[1].doc_id, 2);
        assert_eq!(ws.docs[2].doc_id, 3);
        let ids: std::collections::HashSet<u64> = ws.docs.iter().map(|d| d.doc_id).collect();
        assert_eq!(ids.len(), 3, "doc ids are unique per workspace");
        assert_eq!(ws.next_doc_id, 3);
        ws.close_doc(1);
        ws.add_doc(DialogueDoc::new_empty());
        assert_eq!(ws.docs[2].doc_id, 4, "closed ids are never reused");
    }

    /// A workspace with three docs (indices 0, 1, 2).
    fn ws_three_docs() -> Workspace {
        let mut ws = Workspace::default();
        ws.add_doc(DialogueDoc::new_empty());
        ws.add_doc(DialogueDoc::new_empty());
        ws.add_doc(DialogueDoc::new_empty());
        assert_eq!(ws.docs.len(), 3);
        ws
    }

    #[test]
    fn close_doc_leaves_pending_close_pointing_at_the_same_doc_when_another_tab_closes() {
        // Closing a tab BEFORE the pending one leaves the pending doc id alone (it just shifts index).
        let mut ws = ws_three_docs();
        let pending_id = ws.docs[2].doc_id;
        ws.pending_close = Some(pending_id);
        ws.close_doc(0);
        assert_eq!(
            ws.pending_close,
            Some(pending_id),
            "pending_close is a doc id: closing another tab does not change it"
        );
        assert_eq!(
            ws.position(pending_id),
            Some(1),
            "the pending doc shifted to index 1"
        );

        // Closing a tab AFTER the pending one leaves it alone too.
        let mut ws = ws_three_docs();
        let pending_id = ws.docs[0].doc_id;
        ws.pending_close = Some(pending_id);
        ws.close_doc(2);
        assert_eq!(ws.pending_close, Some(pending_id));
        assert_eq!(ws.position(pending_id), Some(0));
    }

    #[test]
    fn close_doc_of_the_pending_doc_clears_pending_close() {
        let mut ws = ws_three_docs();
        let pending_id = ws.docs[1].doc_id;
        ws.pending_close = Some(pending_id);
        ws.close_doc(1);
        assert_eq!(ws.pending_close, None, "closing the pending doc clears it");

        // Same after a shift: close an earlier tab first, then the (now shifted) pending doc.
        let mut ws = ws_three_docs();
        let pending_id = ws.docs[2].doc_id;
        ws.pending_close = Some(pending_id);
        ws.close_doc(0);
        assert_eq!(
            ws.pending_close,
            Some(pending_id),
            "still pending after the shift"
        );
        ws.close_doc(1); // the pending doc, now at index 1
        assert_eq!(ws.pending_close, None, "closing the pending doc clears it");
    }

    #[test]
    fn close_doc_out_of_range_changes_nothing_about_pending_close() {
        let mut ws = ws_three_docs();
        let pending_id = ws.docs[1].doc_id;
        ws.pending_close = Some(pending_id);
        ws.close_doc(7);
        assert_eq!(
            ws.pending_close,
            Some(pending_id),
            "out-of-range close changes nothing"
        );
        assert_eq!(ws.docs.len(), 3);

        // No pending close: stays None.
        let mut ws = ws_three_docs();
        assert_eq!(ws.pending_close, None);
        ws.close_doc(1);
        assert_eq!(ws.pending_close, None);
    }

    #[test]
    fn position_resolves_a_doc_id_to_its_current_index_and_none_when_closed() {
        let mut ws = ws_three_docs();
        let ids: Vec<u64> = ws.docs.iter().map(|d| d.doc_id).collect();
        assert_eq!(ws.position(ids[0]), Some(0));
        assert_eq!(ws.position(ids[1]), Some(1));
        assert_eq!(ws.position(ids[2]), Some(2));
        assert_eq!(ws.position(999), None, "an unknown id has no position");
        ws.close_doc(0);
        assert_eq!(ws.position(ids[0]), None, "a closed doc has no position");
        assert_eq!(ws.position(ids[1]), Some(0), "the others shifted down");
        assert_eq!(ws.position(ids[2]), Some(1));
    }

    #[test]
    fn has_modal_is_true_exactly_when_a_notification_close_quit_or_controls_modal_is_open() {
        // (notifications queued, pending_close, pending_quit, show_controls) -> expected has_modal
        let cases = [
            (false, false, false, false, false),
            (true, false, false, false, true),
            (false, true, false, false, true),
            (false, false, true, false, true),
            (false, false, false, true, true),
            (true, true, true, true, true),
        ];
        for (notes, close, quit, controls, expected) in cases {
            let mut ws = Workspace::default();
            if notes {
                ws.notifications.push(Notification {
                    kind: NotificationKind::Error,
                    message: "m".to_owned(),
                });
            }
            if close {
                ws.pending_close = Some(0);
            }
            ws.pending_quit = quit;
            ws.show_controls = controls;
            assert_eq!(
                ws.has_modal(),
                expected,
                "notifications={notes} pending_close={close} pending_quit={quit} show_controls={controls}"
            );
        }
    }
}

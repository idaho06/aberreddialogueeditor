//! Test scaffolding shared by every `#[cfg(test)]` module in this binary crate (`tests/`
//! cannot import its modules). Fixture files under `tests/fixtures/` are read-only: tests never
//! save over them; save tests write to a [`TempDir`].

use std::path::PathBuf;

use aberredengine::prelude::{AppState, Vec2};

use crate::model::{DialogueDoc, Link, NodeKey, Slot, Workspace};
use crate::ops;
use crate::state::WorkspaceMutex;

/// A path inside `tests/fixtures/`. Fixtures are never saved over; save tests write to a
/// [`TempDir`] instead.
pub fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// The fixture JSON files under `tests/fixtures/`, embedded once — the single source of the
/// fixture constants every test module uses (`graph::fixtures` re-exports GUARD/LOOPS/DANGLING).
pub mod json {
    /// Embedded contents of `tests/fixtures/guard.json`.
    pub const GUARD: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/guard.json"
    ));
    /// Embedded contents of `tests/fixtures/loops.json`.
    pub const LOOPS: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/loops.json"
    ));
    /// Embedded contents of `tests/fixtures/invalid_dangling.json`.
    pub const DANGLING: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/invalid_dangling.json"
    ));
    /// Embedded contents of `tests/fixtures/invalid_dup_id.json`.
    pub const DUP_ID: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/invalid_dup_id.json"
    ));
    /// Embedded contents of `tests/fixtures/invalid_both_next_choices.json`.
    pub const BOTH: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/invalid_both_next_choices.json"
    ));
    /// Embedded contents of `tests/fixtures/invalid_empty_choices.json`.
    pub const EMPTY_CHOICES: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/invalid_empty_choices.json"
    ));
    /// Embedded contents of `tests/fixtures/invalid_no_start.json`.
    pub const NO_START: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/invalid_no_start.json"
    ));
}

/// An `AppState` with `ws` inserted as the `WorkspaceMutex` handle (as `on_setup` does).
pub fn app_state_with(ws: Workspace) -> AppState {
    let mut app_state = AppState::default();
    app_state.insert(WorkspaceMutex::new(std::sync::Mutex::new(ws)));
    app_state
}

/// A doc with `a --next--> b` (start = a, no selection; dirty from the added nodes).
pub fn linked_doc() -> (DialogueDoc, NodeKey, NodeKey) {
    let mut doc = DialogueDoc::new_empty();
    let a = doc.add_node(Vec2::ZERO);
    let b = doc.add_node(Vec2::new(100.0, 0.0));
    doc.set_link(a, Slot::Next, Link::Node(b));
    (doc, a, b)
}

/// A workspace with one doc holding `a --next--> b` (start = a, no selection; dirty from
/// the added nodes).
pub fn ws_with_two_linked_nodes() -> (Workspace, NodeKey, NodeKey) {
    let (doc, a, b) = linked_doc();
    let mut ws = Workspace::default();
    ws.add_doc(doc);
    (ws, a, b)
}

/// Adds a new untitled doc with one node to `ws` and activates it — a VALID dirty doc (the one
/// node is the start, so the engine save gate passes) — and returns its `doc_id`.
pub fn push_valid_doc(ws: &mut Workspace) -> u64 {
    ops::new_doc(ws);
    ws.active_doc_mut().unwrap().add_node(Vec2::ZERO);
    ws.active_doc().unwrap().doc_id
}

/// A workspace with one valid dirty doc (one node, so the engine gate passes).
pub fn ws_with_dirty_valid_doc() -> Workspace {
    let mut ws = Workspace::default();
    push_valid_doc(&mut ws);
    ws
}

/// A workspace with one UNTITLED doc holding two nodes with the SAME id (a duplicate id:
/// the engine save gate rejects the doc).
pub fn ws_with_duplicate_id_doc() -> Workspace {
    let mut ws = Workspace::default();
    ops::new_doc(&mut ws);
    {
        let doc = ws.active_doc_mut().unwrap();
        let a = doc.add_node(Vec2::ZERO);
        let b = doc.add_node(Vec2::new(100.0, 0.0));
        let id = doc.node(a).unwrap().id.clone();
        doc.rename_node(b, id);
    }
    ws
}

/// A unique temp directory per test, removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    /// Creates the directory. `scope` keeps same-named directories of different test modules
    /// from colliding inside one test process.
    pub fn new(scope: &str, name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "aberred-dialogue-{scope}-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the temp dir");
        Self(dir)
    }

    /// A path inside the temp directory.
    pub fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

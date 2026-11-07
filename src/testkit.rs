//! Test scaffolding shared by the editor, ops and file-dialog test modules.

use std::path::PathBuf;

use aberredengine::prelude::{AppState, Vec2};

use crate::model::{Link, NodeKey, Slot, Workspace};
use crate::ops;
use crate::state::WorkspaceMutex;

/// A path inside `tests/fixtures/`.
pub fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// An `AppState` with `ws` inserted as the `WorkspaceMutex` handle (as `on_setup` does).
pub fn app_state_with(ws: Workspace) -> AppState {
    let mut app_state = AppState::default();
    app_state.insert(WorkspaceMutex::new(std::sync::Mutex::new(ws)));
    app_state
}

/// A workspace with one doc holding `a --next--> b` (start = a, no selection; dirty from
/// the added nodes).
pub fn ws_with_two_linked_nodes() -> (Workspace, NodeKey, NodeKey) {
    let mut ws = Workspace::default();
    ops::new_doc(&mut ws);
    let (a, b) = {
        let doc = ws.active_doc_mut().unwrap();
        let a = doc.add_node(Vec2::ZERO);
        let b = doc.add_node(Vec2::new(100.0, 0.0));
        doc.set_link(a, Slot::Next, Link::Node(b));
        (a, b)
    };
    (ws, a, b)
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

/// A unique temp directory per test, removed on drop. `scope` keeps same-named directories
/// of different test modules from colliding inside one test process.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(scope: &str, name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "aberred-dialogue-{scope}-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create the temp dir");
        Self(dir)
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

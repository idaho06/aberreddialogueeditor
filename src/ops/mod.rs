//! File operations on the [`Workspace`]: plain functions over the model — no imgui, no engine ECS.
//!
//! Engine imports are limited to the dialogue types (`DialogueDef`) plus `load_dialogue` in tests.
#![allow(dead_code)] // consumed by the next plan step

use std::path::{Path, PathBuf};

use aberredengine::prelude::DialogueDef;

use crate::model::{DialogueDoc, Workspace, engine_gate, from_def, to_def};

/// What [`open_path`] did with the requested file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenOutcome {
    /// A doc with the same canonical path was already open; it was made active and nothing was read.
    Focused,
    /// The file was read, parsed and pushed as the new active doc.
    Opened {
        /// The new doc's `load_warnings`, cloned.
        warnings: Vec<String>,
    },
}

/// Errors of the file operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpError {
    /// Reading/writing/renaming failed; the message includes the path and the OS error.
    Io(String),
    /// The file's JSON did not parse as a `DialogueDef`; the message includes the path and the parse error.
    Json(String),
    /// The engine's save gate ([`engine_gate`]) rejected the doc; the message is the engine's own error.
    Invalid(String),
}

impl std::fmt::Display for OpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpError::Io(msg) => write!(f, "io error: {msg}"),
            OpError::Json(msg) => write!(f, "json error: {msg}"),
            OpError::Invalid(msg) => write!(f, "invalid dialogue: {msg}"),
        }
    }
}

impl std::error::Error for OpError {}

/// `canonicalize(path)`, falling back to `path` itself when canonicalization fails.
fn canonical_or(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// `<path>.tmp` in the same directory.
fn tmp_sibling(path: &Path) -> PathBuf {
    let mut os = path.as_os_str().to_owned();
    os.push(".tmp");
    PathBuf::from(os)
}

/// Opens `path` in `ws`: if a doc with the same canonical path is already open, makes it active and
/// returns [`OpenOutcome::Focused`]; otherwise reads and parses the file, pushes the new doc (which
/// becomes active) with `path` set to the canonical path and `dirty = false`, and returns
/// [`OpenOutcome::Opened`] with the doc's load warnings.
pub fn open_path(ws: &mut Workspace, path: &Path) -> Result<OpenOutcome, OpError> {
    let canonical = canonical_or(path);
    let existing = ws.find_by_path(&canonical).or_else(|| {
        ws.docs.iter().position(|doc| {
            doc.path
                .as_ref()
                .is_some_and(|p| canonical_or(p) == canonical)
        })
    });
    if let Some(index) = existing {
        ws.active = index;
        return Ok(OpenOutcome::Focused);
    }

    let text = std::fs::read_to_string(path)
        .map_err(|err| OpError::Io(format!("cannot read {}: {err}", path.display())))?;
    let def: DialogueDef = serde_json::from_str(&text)
        .map_err(|err| OpError::Json(format!("cannot parse {}: {err}", path.display())))?;
    let mut doc = from_def(&def);
    doc.path = Some(canonical);
    doc.dirty = false;
    let warnings = doc.load_warnings.clone();
    ws.add_doc(doc);
    Ok(OpenOutcome::Opened { warnings })
}

/// Saves `doc` to `path` as pretty-printed `DialogueDef` JSON plus a trailing newline, atomically
/// (write `<path>.tmp` in the same directory, then rename over `path`). The engine gate runs FIRST:
/// on failure nothing is written — no file, no `.tmp`. On success the doc's `path` is set to the
/// canonical path of the saved file and the doc is marked clean.
pub fn save_doc(doc: &mut DialogueDoc, path: &Path) -> Result<(), OpError> {
    if let Err(err) = engine_gate(doc) {
        return Err(OpError::Invalid(err.to_string()));
    }
    let mut text = serde_json::to_string_pretty(&to_def(doc))
        .map_err(|err| OpError::Json(format!("cannot serialize {}: {err}", path.display())))?;
    text.push('\n');
    let tmp = tmp_sibling(path);
    if let Err(err) = std::fs::write(&tmp, &text).and_then(|_| std::fs::rename(&tmp, path)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(OpError::Io(format!(
            "cannot write {}: {err}",
            path.display()
        )));
    }
    doc.path = Some(canonical_or(path));
    doc.mark_clean();
    Ok(())
}

/// Pushes a new empty doc and makes it active.
pub fn new_doc(ws: &mut Workspace) {
    ws.add_doc(DialogueDoc::new_empty());
}

/// Closes the doc at `index` (wraps [`Workspace::close_doc`], keeping `active` valid).
pub fn close_doc(ws: &mut Workspace, index: usize) {
    ws.close_doc(index);
}

/// Tab title of a doc: the file name of its path, or "Untitled". No dirty marker here.
pub fn doc_title(doc: &DialogueDoc) -> String {
    doc.path
        .as_deref()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aberredengine::prelude::load_dialogue;

    const GUARD_JSON: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/guard.json"
    ));

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    /// A unique temp directory per test, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "aberred-dialogue-ops-{}-{name}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create the temp dir");
            Self(dir)
        }

        fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn open_guard(ws: &mut Workspace) -> OpenOutcome {
        open_path(ws, &fixture("guard.json")).expect("guard.json opens")
    }

    #[test]
    fn open_save_reload_round_trips_the_def() {
        let tmp = TempDir::new("roundtrip");
        let mut ws = Workspace::default();
        let outcome = open_guard(&mut ws);
        assert_eq!(outcome, OpenOutcome::Opened { warnings: vec![] });
        let doc = ws.active_doc().unwrap();
        assert!(!doc.dirty, "a freshly opened doc is clean");
        assert_eq!(
            doc.path,
            Some(fixture("guard.json").canonicalize().unwrap()),
            "open stores the canonical path"
        );

        let save_path = tmp.join("guard-saved.json");
        let doc = ws.active_doc_mut().unwrap();
        save_doc(doc, &save_path).expect("save succeeds");
        assert!(!doc.dirty, "save marks the doc clean");
        assert_eq!(
            doc.path,
            Some(save_path.canonicalize().unwrap()),
            "save stores the canonical path of the saved file"
        );

        let text = std::fs::read_to_string(&save_path).unwrap();
        assert!(text.ends_with('\n'), "the saved file ends with a newline");
        assert!(
            !tmp_sibling(&save_path).exists(),
            "no .tmp is left after a successful save"
        );

        let mut ws2 = Workspace::default();
        open_path(&mut ws2, &save_path).expect("the saved file reopens");
        let original: DialogueDef = serde_json::from_str(GUARD_JSON).unwrap();
        assert_eq!(
            to_def(ws2.active_doc().unwrap()),
            original,
            "to_def of the reloaded doc equals the original DialogueDef"
        );
    }

    #[test]
    fn saved_file_passes_the_engine_load_dialogue() {
        let tmp = TempDir::new("engine-load");
        let mut ws = Workspace::default();
        open_guard(&mut ws);
        let save_path = tmp.join("guard.json");
        save_doc(ws.active_doc_mut().unwrap(), &save_path).expect("save succeeds");
        load_dialogue(&save_path)
            .expect("aberredengine::prelude::load_dialogue loads the saved file");
    }

    #[test]
    fn save_is_gated_by_the_engine_and_writes_nothing_on_failure() {
        let cases: [(&str, &str); 3] = [
            ("invalid_dangling.json", "links to unknown node"),
            ("invalid_dup_id.json", "duplicate dialogue node id"),
            ("invalid_no_start.json", "is not among the defined nodes"),
        ];
        for (i, (name, needle)) in cases.iter().enumerate() {
            let tmp = TempDir::new(&format!("gate-{i}"));
            let mut ws = Workspace::default();
            open_path(&mut ws, &fixture(name))
                .expect("invalid fixtures still open: from_def is tolerant");
            let save_path = tmp.join("out.json");
            let err = save_doc(ws.active_doc_mut().unwrap(), &save_path)
                .expect_err("the engine gate must block the save");
            assert!(
                matches!(&err, OpError::Invalid(msg) if msg.contains(needle)),
                "{name}: expected OpError::Invalid carrying the engine message {needle:?}, got {err}"
            );
            assert!(
                err.to_string().contains(needle),
                "{name}: Display carries the engine message: {err}"
            );
            assert!(!save_path.exists(), "{name}: no file may be written");
            assert!(
                !tmp_sibling(&save_path).exists(),
                "{name}: no .tmp may be left behind"
            );
        }
    }

    #[test]
    fn open_same_path_twice_focuses_the_existing_doc_even_via_a_different_spelling() {
        let mut ws = Workspace::default();
        let guard = fixture("guard.json");
        assert!(matches!(
            open_path(&mut ws, &guard).unwrap(),
            OpenOutcome::Opened { .. }
        ));
        assert_eq!(ws.docs.len(), 1);
        assert_eq!(open_path(&mut ws, &guard).unwrap(), OpenOutcome::Focused);
        assert_eq!(ws.docs.len(), 1, "focusing must not add a doc");

        let dotted = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/./fixtures/guard.json");
        assert_eq!(
            open_path(&mut ws, &dotted).unwrap(),
            OpenOutcome::Focused,
            "a differently spelled path to the same file focuses the open doc"
        );
        assert_eq!(ws.docs.len(), 1);
        assert_eq!(ws.active, 0);
        assert_eq!(ws.docs[0].path, Some(guard.canonicalize().unwrap()));

        // Focusing from another tab switches active back to that doc.
        open_path(&mut ws, &fixture("loops.json")).unwrap();
        assert_eq!(ws.active, 1);
        assert_eq!(open_path(&mut ws, &guard).unwrap(), OpenOutcome::Focused);
        assert_eq!(ws.active, 0, "focusing an earlier tab makes it active");
        assert_eq!(ws.docs.len(), 2);
    }

    #[test]
    fn open_canonicalizes_both_sides_when_matching_an_open_doc() {
        let mut ws = Workspace::default();
        open_guard(&mut ws);
        // A stored path that is NOT canonical as a Path (a `..` component) but canonicalizes to
        // the same file: the raw find_by_path misses it, the canonicalized comparison must hit.
        let non_canonical =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/../fixtures/guard.json");
        assert_ne!(non_canonical, fixture("guard.json").canonicalize().unwrap());
        ws.docs[0].path = Some(non_canonical);
        assert_eq!(
            open_path(&mut ws, &fixture("guard.json")).unwrap(),
            OpenOutcome::Focused
        );
        assert_eq!(ws.docs.len(), 1);
    }

    #[test]
    fn open_reports_json_and_io_errors_with_the_path() {
        let tmp = TempDir::new("open-errors");
        let mut ws = Workspace::default();

        let bad = tmp.join("bad.json");
        std::fs::write(&bad, "{ this is not json").unwrap();
        let err = open_path(&mut ws, &bad).expect_err("invalid JSON must fail");
        assert!(
            matches!(&err, OpError::Json(msg) if msg.contains(&bad.display().to_string())),
            "the Json error must name the path, got {err}"
        );
        assert!(
            err.to_string().contains(&bad.display().to_string()),
            "Display names the path: {err}"
        );

        let missing = tmp.join("missing.json");
        let err = open_path(&mut ws, &missing).expect_err("a missing file must fail");
        assert!(
            matches!(&err, OpError::Io(msg) if msg.contains("missing.json")),
            "the Io error must name the path, got {err}"
        );
        assert!(ws.docs.is_empty(), "failed opens add no doc");
    }

    #[test]
    fn open_returns_the_docs_load_warnings() {
        let mut ws = Workspace::default();
        let outcome = open_path(&mut ws, &fixture("invalid_no_start.json")).unwrap();
        match outcome {
            OpenOutcome::Opened { warnings } => {
                assert_eq!(
                    warnings.len(),
                    1,
                    "the no_start fixture has one load warning"
                )
            }
            OpenOutcome::Focused => panic!("expected Opened, got Focused"),
        }
        assert_eq!(ws.active_doc().unwrap().load_warnings.len(), 1);
    }

    #[test]
    fn successful_save_marks_clean_sets_canonical_path_and_leaves_no_tmp() {
        let tmp = TempDir::new("save-clean");
        let mut ws = Workspace::default();
        open_guard(&mut ws);
        let doc = ws.active_doc_mut().unwrap();
        let key = doc.nodes[0].key;
        doc.rename_node(key, "renamed".to_owned());
        assert!(doc.dirty, "renaming dirties the doc");

        let save_path = tmp.join("out.json");
        save_doc(doc, &save_path).expect("the renamed doc still passes the gate");
        assert!(!doc.dirty, "a successful save clears dirty");
        assert_eq!(doc.path, Some(save_path.canonicalize().unwrap()));
        assert!(save_path.exists());
        assert!(
            !tmp_sibling(&save_path).exists(),
            "the atomic write leaves no .tmp after success"
        );
        let text = std::fs::read_to_string(&save_path).unwrap();
        assert!(text.ends_with('\n'));
        assert!(
            text.contains("\"renamed\""),
            "the saved JSON has the new id"
        );
    }

    #[test]
    fn doc_title_new_doc_and_close_doc_keep_active_valid() {
        let mut ws = Workspace::default();
        new_doc(&mut ws);
        assert_eq!(ws.docs.len(), 1);
        assert_eq!(ws.active, 0);
        assert_eq!(doc_title(ws.active_doc().unwrap()), "Untitled");

        ws.active_doc_mut().unwrap().path = Some(PathBuf::from("/tmp/dir/guard.json"));
        assert_eq!(doc_title(ws.active_doc().unwrap()), "guard.json");
        ws.active_doc_mut().unwrap().path = Some(PathBuf::from("/"));
        assert_eq!(
            doc_title(ws.active_doc().unwrap()),
            "Untitled",
            "a path without a file name falls back to Untitled"
        );

        new_doc(&mut ws);
        assert_eq!(ws.docs.len(), 2);
        assert_eq!(ws.active, 1, "new_doc activates the new doc");
        assert_eq!(doc_title(ws.active_doc().unwrap()), "Untitled");

        close_doc(&mut ws, 1);
        assert_eq!(ws.docs.len(), 1);
        assert_eq!(
            ws.active, 0,
            "closing the active last doc keeps active valid"
        );
        close_doc(&mut ws, 0);
        assert!(ws.docs.is_empty());
        assert!(
            ws.active_doc().is_none(),
            "an empty workspace has no active doc"
        );
        close_doc(&mut ws, 7); // out of range: no-op
        assert!(ws.docs.is_empty());
    }
}

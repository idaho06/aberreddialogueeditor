//! File operations on the [`Workspace`]: plain functions over the model — no imgui, no engine ECS.
//!
//! Engine imports are limited to the dialogue types (`DialogueDef`) plus `load_dialogue` in tests.

use std::path::{Path, PathBuf};

use aberredengine::prelude::DialogueDef;

use crate::graph::auto_layout;
use crate::model::{
    DialogueDoc, Notification, NotificationKind, Workspace, engine_gate, from_def, to_def,
};

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
    /// Another open tab already holds the same canonical path.
    AlreadyOpen(String),
}

impl std::fmt::Display for OpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpError::Io(msg) => write!(f, "io error: {msg}"),
            OpError::Json(msg) => write!(f, "json error: {msg}"),
            OpError::Invalid(msg) => write!(f, "invalid dialogue: {msg}"),
            OpError::AlreadyOpen(path) => write!(f, "already open in another tab: {path}"),
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
    log::debug!(
        "open_path: requested {}, canonical {}",
        path.display(),
        canonical.display()
    );
    let existing = ws.find_by_path(&canonical).or_else(|| {
        ws.docs.iter().position(|doc| {
            doc.path
                .as_ref()
                .is_some_and(|p| canonical_or(p) == canonical)
        })
    });
    if let Some(index) = existing {
        ws.active = index;
        log::debug!("open_path: already open, focusing tab {index}");
        return Ok(OpenOutcome::Focused);
    }

    let text = std::fs::read_to_string(path).map_err(|err| {
        let err = OpError::Io(format!("cannot read {}: {err}", path.display()));
        log::warn!("open_path failed: {err}");
        err
    })?;
    let def: DialogueDef = serde_json::from_str(&text).map_err(|err| {
        let err = OpError::Json(format!("cannot parse {}: {err}", path.display()));
        log::warn!("open_path failed: {err}");
        err
    })?;
    let mut doc = from_def(&def);
    doc.path = Some(canonical);
    doc.dirty = false;
    let warnings = doc.load_warnings.clone();
    for warning in &warnings {
        log::warn!("open_path: load warning from {}: {warning}", path.display());
    }
    let node_count = doc.nodes.len();
    auto_layout(&mut doc);
    ws.add_doc(doc);
    log::debug!(
        "open_path: opened {} as tab {} with {node_count} nodes and {} load warnings",
        path.display(),
        ws.active,
        warnings.len()
    );
    Ok(OpenOutcome::Opened { warnings })
}

/// The save pre-check: the engine save gate ([`engine_gate`]) mapped to [`OpError::Invalid`] with
/// the engine's own message — the SAME predicate and message [`save_doc`] enforces, so the two can
/// never disagree. Pure: it does not mutate the doc and writes nothing. Run it BEFORE opening a
/// Save As picker, so a doc the engine would reject never reaches the dialog.
pub fn check_saveable(doc: &DialogueDoc) -> Result<(), OpError> {
    engine_gate(doc).map_err(|err| OpError::Invalid(err.to_string()))
}

/// Saves `doc` to `path` as pretty-printed `DialogueDef` JSON plus a trailing newline, atomically
/// (write `<path>.tmp` in the same directory, then rename over `path`). The engine gate runs FIRST
/// (via [`check_saveable`]): on failure nothing is written — no file, no `.tmp`. On success the
/// doc's `path` is set to the canonical path of the saved file and the doc is marked clean.
pub fn save_doc(doc: &mut DialogueDoc, path: &Path) -> Result<(), OpError> {
    log::debug!("save_doc: target {}", path.display());
    if let Err(err) = check_saveable(doc) {
        log::warn!("save_doc: engine gate rejected {}: {err}", path.display());
        return Err(err);
    }
    let mut text = serde_json::to_string_pretty(&to_def(doc))
        .map_err(|err| OpError::Json(format!("cannot serialize {}: {err}", path.display())))?;
    text.push('\n');
    let tmp = tmp_sibling(path);
    if let Err(err) = std::fs::write(&tmp, &text).and_then(|_| std::fs::rename(&tmp, path)) {
        let _ = std::fs::remove_file(&tmp);
        let err = OpError::Io(format!("cannot write {}: {err}", path.display()));
        log::warn!("save_doc failed: {err}");
        return Err(err);
    }
    doc.path = Some(canonical_or(path));
    doc.mark_clean();
    log::debug!(
        "save_doc: wrote {} bytes to {} via .tmp rename",
        text.len(),
        path.display()
    );
    Ok(())
}

/// Saves the doc at `index` to `path` (delegates to [`save_doc`]), after a tab-collision check: if a
/// DIFFERENT doc already holds the same canonical path — compared canonical-or-given the way
/// [`open_path`] does, so the target file may not exist yet — returns [`OpError::AlreadyOpen`] and
/// writes nothing. An out-of-range `index` is [`OpError::Io`] "no such document".
pub fn save_doc_at(ws: &mut Workspace, index: usize, path: &Path) -> Result<(), OpError> {
    log::debug!("save_doc_at: doc {index} to {}", path.display());
    if index >= ws.docs.len() {
        log::warn!("save_doc_at failed: no document at index {index}");
        return Err(OpError::Io("no such document".to_owned()));
    }
    let target = canonical_or(path);
    let clash =
        ws.docs.iter().enumerate().any(|(i, doc)| {
            i != index && doc.path.as_ref().is_some_and(|p| canonical_or(p) == target)
        });
    if clash {
        log::warn!(
            "save_doc_at failed: {} is already open in another tab",
            path.display()
        );
        return Err(OpError::AlreadyOpen(path.display().to_string()));
    }
    save_doc(&mut ws.docs[index], path)
}

/// Pushes a new empty doc and makes it active.
pub fn new_doc(ws: &mut Workspace) {
    ws.add_doc(DialogueDoc::new_empty());
    log::debug!("new_doc: {} docs, active {}", ws.docs.len(), ws.active);
}

/// Closes the doc at `index` (wraps [`Workspace::close_doc`], keeping `active` valid).
pub fn close_doc(ws: &mut Workspace, index: usize) {
    ws.close_doc(index);
    log::debug!(
        "close_doc: closed tab {index}, {} docs left, active {}",
        ws.docs.len(),
        ws.active
    );
}

/// Tab title of a doc: the file name of its path, or "Untitled". No dirty marker here.
pub fn doc_title(doc: &DialogueDoc) -> String {
    doc.path
        .as_deref()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".to_owned())
}

/// The typed notification for a failed operation, shown in the GUI's modal:
/// [`OpError::Invalid`] (the engine save gate rejected the doc) becomes a
/// [`NotificationKind::SaveRejected`] keeping only the engine's own message (no "invalid
/// dialogue: " prefix), so the modal can use the "Cannot save" title and point at the Problems
/// panel; every other error becomes a [`NotificationKind::Error`] with its `Display` text.
pub fn error_notification(err: &OpError) -> Notification {
    match err {
        OpError::Invalid(msg) => Notification {
            kind: NotificationKind::SaveRejected,
            message: msg.clone(),
        },
        err => Notification {
            kind: NotificationKind::Error,
            message: err.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenes::editor::testkit::{TempDir, fixture};
    use aberredengine::prelude::{Vec2, load_dialogue};

    const GUARD_JSON: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/guard.json"
    ));

    /// `path.canonicalize()`, for comparing against the canonical paths ops stores on docs.
    fn canon(path: &Path) -> PathBuf {
        path.canonicalize().expect("the fixture exists")
    }

    fn open_guard(ws: &mut Workspace) -> OpenOutcome {
        open_path(ws, &fixture("guard.json")).expect("guard.json opens")
    }

    /// Asserts the variant message and the Display text of `err` both contain `needle`.
    fn assert_err_contains(err: &OpError, needle: &str) {
        let msg = match err {
            OpError::Io(msg)
            | OpError::Json(msg)
            | OpError::Invalid(msg)
            | OpError::AlreadyOpen(msg) => msg,
        };
        assert!(
            msg.contains(needle),
            "the error message contains {needle:?}, got {err}"
        );
        assert!(
            err.to_string().contains(needle),
            "Display contains {needle:?}: {err}"
        );
    }

    /// Reads a freshly saved file, asserting it ends with a newline and leaves no `.tmp` behind.
    fn read_saved(path: &Path) -> String {
        let text = std::fs::read_to_string(path).expect("the saved file exists");
        assert!(text.ends_with('\n'), "the saved file ends with a newline");
        assert!(!tmp_sibling(path).exists(), "no .tmp is left after saving");
        text
    }

    #[test]
    fn open_save_reload_round_trips_the_def() {
        let tmp = TempDir::new("ops", "roundtrip");
        let mut ws = Workspace::default();
        let outcome = open_guard(&mut ws);
        assert_eq!(outcome, OpenOutcome::Opened { warnings: vec![] });
        let doc = ws.active_doc().unwrap();
        assert!(!doc.dirty, "a freshly opened doc is clean");
        assert_eq!(
            doc.path,
            Some(canon(&fixture("guard.json"))),
            "open stores the canonical path"
        );

        let save_path = tmp.join("guard-saved.json");
        let doc = ws.active_doc_mut().unwrap();
        save_doc(doc, &save_path).expect("save succeeds");
        assert!(!doc.dirty, "save marks the doc clean");
        assert_eq!(
            doc.path,
            Some(canon(&save_path)),
            "save stores the canonical path of the saved file"
        );
        read_saved(&save_path);

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
        let tmp = TempDir::new("ops", "engine-load");
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
        let tmp = TempDir::new("ops", "gate");
        for (i, &(name, needle)) in cases.iter().enumerate() {
            let mut ws = Workspace::default();
            open_path(&mut ws, &fixture(name))
                .expect("invalid fixtures still open: from_def is tolerant");
            let save_path = tmp.join(&format!("out-{i}.json"));
            let err = save_doc(ws.active_doc_mut().unwrap(), &save_path)
                .expect_err("the engine gate must block the save");
            assert!(
                matches!(&err, OpError::Invalid(_)),
                "{name}: expected OpError::Invalid, got {err}"
            );
            assert_err_contains(&err, needle);
            assert!(!save_path.exists(), "{name}: no file may be written");
            assert!(
                !tmp_sibling(&save_path).exists(),
                "{name}: no .tmp may be left behind"
            );
        }
    }

    #[test]
    fn check_saveable_accepts_a_valid_doc_without_mutating_it() {
        let mut ws = Workspace::default();
        open_guard(&mut ws);
        let (revision, content_revision, dirty) = {
            let doc = ws.active_doc().unwrap();
            (doc.revision, doc.content_revision, doc.dirty)
        };
        let before = to_def(ws.active_doc().unwrap());
        check_saveable(ws.active_doc().unwrap()).expect("a valid doc passes the pre-check");
        let doc = ws.active_doc().unwrap();
        assert_eq!(
            (doc.revision, doc.content_revision, doc.dirty),
            (revision, content_revision, dirty),
            "the pre-check does not mutate the doc"
        );
        assert_eq!(to_def(doc), before, "the doc's content is unchanged");
    }

    #[test]
    fn check_saveable_rejects_invalid_docs_with_the_exact_error_save_doc_returns() {
        let tmp = TempDir::new("ops", "check-saveable");
        let cases: [(&str, &str); 2] = [
            ("invalid_dup_id.json", "duplicate dialogue node id"),
            ("invalid_no_start.json", "is not among the defined nodes"),
        ];
        for (i, &(name, needle)) in cases.iter().enumerate() {
            let mut ws = Workspace::default();
            open_path(&mut ws, &fixture(name))
                .expect("invalid fixtures still open: from_def is tolerant");
            let out = tmp.join(&format!("out-{i}.json"));
            let pre = check_saveable(ws.active_doc().unwrap())
                .expect_err("the pre-check rejects the invalid doc");
            assert!(
                matches!(&pre, OpError::Invalid(_)),
                "{name}: expected OpError::Invalid, got {pre}"
            );
            assert_err_contains(&pre, needle);
            let save = save_doc(ws.active_doc_mut().unwrap(), &out)
                .expect_err("save_doc rejects the same doc");
            assert_eq!(
                pre, save,
                "{name}: the pre-check and save_doc return the identical error (same predicate and message)"
            );
            assert!(!out.exists(), "{name}: the rejected save wrote nothing");
            assert!(
                !tmp_sibling(&out).exists(),
                "{name}: no .tmp may be left behind"
            );
        }

        // A brand-new empty doc has no start node and is rejected as well.
        let mut ws = Workspace::default();
        new_doc(&mut ws);
        let pre = check_saveable(ws.active_doc().unwrap())
            .expect_err("an empty doc has no start, the pre-check rejects it");
        assert!(matches!(&pre, OpError::Invalid(_)), "got {pre}");
        assert_err_contains(&pre, "is not among the defined nodes");
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

        let respelled = fixture("../fixtures/guard.json");
        assert_eq!(
            open_path(&mut ws, &respelled).unwrap(),
            OpenOutcome::Focused,
            "a differently spelled path to the same file focuses the open doc"
        );
        assert_eq!(ws.docs.len(), 1);
        assert_eq!(ws.active, 0);
        assert_eq!(ws.docs[0].path, Some(canon(&guard)));

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
        let non_canonical = fixture("../fixtures/guard.json");
        assert_ne!(non_canonical, canon(&fixture("guard.json")));
        ws.docs[0].path = Some(non_canonical);
        assert_eq!(
            open_path(&mut ws, &fixture("guard.json")).unwrap(),
            OpenOutcome::Focused
        );
        assert_eq!(ws.docs.len(), 1);
    }

    #[test]
    fn open_reports_json_and_io_errors_with_the_path() {
        let tmp = TempDir::new("ops", "open-errors");
        let mut ws = Workspace::default();

        let bad = tmp.join("bad.json");
        std::fs::write(&bad, "{ this is not json").unwrap();
        let err = open_path(&mut ws, &bad).expect_err("invalid JSON must fail");
        assert!(matches!(&err, OpError::Json(_)), "must be Json, got {err}");
        assert_err_contains(&err, &bad.display().to_string());

        let missing = tmp.join("missing.json");
        let err = open_path(&mut ws, &missing).expect_err("a missing file must fail");
        assert!(matches!(&err, OpError::Io(_)), "must be Io, got {err}");
        assert_err_contains(&err, "missing.json");
        assert!(ws.docs.is_empty(), "failed opens add no doc");
    }

    #[test]
    fn open_returns_the_docs_load_warnings() {
        let mut ws = Workspace::default();
        let outcome = open_path(&mut ws, &fixture("invalid_no_start.json")).unwrap();
        let OpenOutcome::Opened { warnings } = outcome else {
            panic!("expected Opened, got {outcome:?}");
        };
        assert_eq!(
            warnings.len(),
            1,
            "the no_start fixture has one load warning"
        );
        assert_eq!(ws.active_doc().unwrap().load_warnings.len(), 1);
    }

    #[test]
    fn open_path_auto_layouts_the_new_doc_without_dirtying_it() {
        let mut ws = Workspace::default();
        assert!(matches!(open_guard(&mut ws), OpenOutcome::Opened { .. }));
        let doc = ws.active_doc().unwrap();
        assert!(!doc.dirty, "auto-layout on load must not dirty the doc");
        let mut seen = std::collections::HashSet::new();
        for node in &doc.nodes {
            assert!(
                node.pos != Vec2::ZERO,
                "node \"{}\" got a real position",
                node.id
            );
            assert!(
                seen.insert((node.pos.x.to_bits(), node.pos.y.to_bits())),
                "node \"{}\" has a distinct position: {:?}",
                node.id,
                node.pos
            );
        }

        // Re-opening the same path (Focused) must NOT re-layout a moved node.
        let doc = ws.active_doc_mut().unwrap();
        let key = doc.nodes[0].key;
        doc.set_node_pos(key, Vec2::new(999.0, 999.0));
        let moved = doc.node(key).unwrap().pos;
        assert_eq!(
            open_path(&mut ws, &fixture("guard.json")).unwrap(),
            OpenOutcome::Focused
        );
        assert_eq!(
            ws.active_doc().unwrap().node(key).unwrap().pos,
            moved,
            "the Focused path must not re-layout"
        );
    }

    #[test]
    fn successful_save_marks_clean_sets_canonical_path_and_leaves_no_tmp() {
        let tmp = TempDir::new("ops", "save-clean");
        let mut ws = Workspace::default();
        open_guard(&mut ws);
        let doc = ws.active_doc_mut().unwrap();
        let key = doc.nodes[0].key;
        doc.rename_node(key, "renamed".to_owned());
        assert!(doc.dirty, "renaming dirties the doc");

        let save_path = tmp.join("out.json");
        save_doc(doc, &save_path).expect("the renamed doc still passes the gate");
        assert!(!doc.dirty, "a successful save clears dirty");
        assert_eq!(doc.path, Some(canon(&save_path)));
        let text = read_saved(&save_path);
        assert!(
            text.contains("\"renamed\""),
            "the saved JSON has the new id"
        );
    }

    #[test]
    fn save_doc_at_out_of_range_is_io_no_such_document() {
        let tmp = TempDir::new("ops", "save-at-range");
        let mut ws = Workspace::default();
        new_doc(&mut ws);
        let out = tmp.join("out.json");
        let err = save_doc_at(&mut ws, 5, &out).expect_err("out-of-range index must fail");
        assert!(
            matches!(&err, OpError::Io(msg) if msg == "no such document"),
            "expected Io(\"no such document\"), got {err}"
        );
        assert!(!out.exists(), "nothing may be written");
    }

    #[test]
    fn save_doc_at_collision_with_another_tab_writes_nothing() {
        let tmp = TempDir::new("ops", "save-at-collision");
        let mut ws = Workspace::default();
        open_guard(&mut ws); // doc 0: canonical guard.json
        open_path(&mut ws, &fixture("loops.json")).unwrap(); // doc 1, active
        let loops_path = canon(&fixture("loops.json"));

        // A differently spelled path to doc 0's existing file collides.
        let respelled = fixture("../fixtures/guard.json");
        let original_guard = std::fs::read_to_string(&respelled).unwrap();
        let err = save_doc_at(&mut ws, 1, &respelled).expect_err("must collide with doc 0");
        assert!(
            matches!(&err, OpError::AlreadyOpen(_)),
            "expected AlreadyOpen, got {err}"
        );
        assert!(
            err.to_string().contains("already open in another tab"),
            "Display text: {err}"
        );
        assert_eq!(
            std::fs::read_to_string(&respelled).unwrap(),
            original_guard,
            "the collided target file is untouched"
        );

        // A not-yet-existing target collides when another doc holds the same given spelling.
        let planned = tmp.join("planned.json");
        ws.docs[0].path = Some(planned.clone());
        let err =
            save_doc_at(&mut ws, 1, &planned).expect_err("must collide via the given spelling");
        assert!(matches!(&err, OpError::AlreadyOpen(_)), "got {err}");
        assert!(!planned.exists(), "nothing may be written");

        assert_eq!(ws.docs.len(), 2);
        assert_eq!(
            ws.docs[1].path,
            Some(loops_path.clone()),
            "the rejected save-as leaves the doc's path alone"
        );
        assert_eq!(ws.docs[0].path, Some(planned), "doc 0 keeps its path");
    }

    #[test]
    fn save_doc_at_success_saves_through_save_doc_and_allows_its_own_path() {
        let tmp = TempDir::new("ops", "save-at-ok");
        let mut ws = Workspace::default();
        new_doc(&mut ws);
        ws.active_doc_mut().unwrap().add_node(Vec2::ZERO);
        assert!(ws.active_doc().unwrap().dirty);

        let out = tmp.join("out.json");
        save_doc_at(&mut ws, 0, &out).expect("save-as succeeds");
        let doc = ws.active_doc().unwrap();
        assert!(!doc.dirty, "save-as marks the doc clean");
        assert_eq!(
            doc.path,
            Some(canon(&out)),
            "save-as sets the canonical path"
        );
        read_saved(&out);

        // Saving onto the doc's OWN path is not a collision (the index matches).
        save_doc_at(&mut ws, 0, &out).expect("re-saving to the same path is allowed");
        assert_eq!(ws.docs.len(), 1);
    }

    #[test]
    fn error_notification_keeps_only_the_engine_message_for_invalid() {
        let invalid =
            OpError::Invalid("dialogue node \"a\" links to unknown node \"b\"".to_owned());
        let note = error_notification(&invalid);
        assert_eq!(
            note.kind,
            NotificationKind::SaveRejected,
            "an engine-gate rejection is a SaveRejected notification"
        );
        assert_eq!(
            note.message, "dialogue node \"a\" links to unknown node \"b\"",
            "no \"invalid dialogue: \" prefix"
        );
        let io = OpError::Io("cannot read x.json: nope".to_owned());
        let note = error_notification(&io);
        assert_eq!(note.kind, NotificationKind::Error);
        assert_eq!(note.message, io.to_string());
        let json = OpError::Json("cannot parse y.json: bad".to_owned());
        let note = error_notification(&json);
        assert_eq!(note.kind, NotificationKind::Error);
        assert_eq!(note.message, json.to_string());
        let dup = OpError::AlreadyOpen("/tmp/z.json".to_owned());
        let note = error_notification(&dup);
        assert_eq!(note.kind, NotificationKind::Error);
        assert_eq!(note.message, dup.to_string());
    }

    #[test]
    fn error_notification_kinds_for_every_op_error_variant() {
        let cases: [(OpError, NotificationKind); 4] = [
            (
                OpError::Io("cannot read x.json: nope".to_owned()),
                NotificationKind::Error,
            ),
            (
                OpError::Json("cannot parse y.json: bad".to_owned()),
                NotificationKind::Error,
            ),
            (
                OpError::Invalid("engine says no".to_owned()),
                NotificationKind::SaveRejected,
            ),
            (
                OpError::AlreadyOpen("/tmp/z.json".to_owned()),
                NotificationKind::Error,
            ),
        ];
        for (err, expected_kind) in cases {
            let note = error_notification(&err);
            assert_eq!(note.kind, expected_kind, "{err}");
            match &err {
                OpError::Invalid(msg) => assert_eq!(&note.message, msg),
                _ => assert_eq!(note.message, err.to_string()),
            }
        }
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

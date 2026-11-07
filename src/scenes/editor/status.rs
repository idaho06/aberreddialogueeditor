//! The status bar: a borderless, non-interactive window pinned to the bottom of the display
//! (the same window-flag style as the tab strip in `tabs.rs`), one line high, showing the
//! active doc's path (or "Untitled"), the dirty marker, the problem counts, the node count and
//! the zoom percentage — or "No document" when nothing is open.
//!
//! Locking: the workspace is locked only to snapshot the displayed values; the widgets are
//! drawn with the guard dropped. All text is built by the pure helpers below (unit-tested).

use std::path::Path;

use aberredengine::prelude::imgui::{Condition, WindowFlags};
use aberredengine::prelude::*;

use super::problems::{header_color, problem_counts};
use crate::state::WorkspaceMutex;

/// Snapshot of the status bar, taken under a short workspace lock.
struct StatusSnap {
    doc: Option<DocSnap>,
}

struct DocSnap {
    path: Option<std::path::PathBuf>,
    dirty: bool,
    errors: usize,
    warnings: usize,
    nodes: usize,
    zoom: f32,
}

fn snapshot(app_state: &AppState) -> StatusSnap {
    let Some(handle) = app_state.get::<WorkspaceMutex>() else {
        return StatusSnap { doc: None };
    };
    let mut ws = handle.lock().unwrap();
    let Some(doc) = ws.active_doc_mut() else {
        return StatusSnap { doc: None };
    };
    let (errors, warnings) = problem_counts(doc.problems());
    StatusSnap {
        doc: Some(DocSnap {
            path: doc.path.clone(),
            dirty: doc.dirty,
            errors,
            warnings,
            nodes: doc.nodes.len(),
            zoom: doc.camera.zoom,
        }),
    }
}

/// `n` with the right plural: "1 node", "2 nodes".
fn count_text(n: usize, singular: &str) -> String {
    if n == 1 {
        format!("1 {singular}")
    } else {
        format!("{n} {singular}s")
    }
}

/// The file part: the path's display text (or "Untitled") plus a `*` when dirty.
pub fn file_text(path: Option<&Path>, dirty: bool) -> String {
    let base = path.map_or_else(|| "Untitled".to_owned(), |p| p.display().to_string());
    if dirty { format!("{base}*") } else { base }
}

/// The problems part: "E errors / W warnings" (pluralised), coloured by
/// [`header_color`](super::problems::header_color).
pub fn problems_text(errors: usize, warnings: usize) -> String {
    format!(
        "{} / {}",
        count_text(errors, "error"),
        count_text(warnings, "warning")
    )
}

/// The node-count part: "N nodes".
pub fn nodes_text(nodes: usize) -> String {
    count_text(nodes, "node")
}

/// The zoom part: "zoom 100%" (the percentage is rounded to a whole number).
pub fn zoom_text(zoom: f32) -> String {
    format!("zoom {:.0}%", zoom * 100.0)
}

/// Draws the status bar pinned to the bottom of the display.
pub fn draw(ctx: &mut GuiCtx) {
    let snap = snapshot(ctx.app_state);
    let ui = ctx.ui;
    let display = ui.io().display_size;
    let height = ui.frame_height_with_spacing();
    let flags = WindowFlags::NO_TITLE_BAR
        | WindowFlags::NO_RESIZE
        | WindowFlags::NO_MOVE
        | WindowFlags::NO_SCROLLBAR
        | WindowFlags::NO_COLLAPSE
        | WindowFlags::NO_SAVED_SETTINGS
        | WindowFlags::NO_NAV_INPUTS
        | WindowFlags::NO_BRING_TO_FRONT_ON_FOCUS;
    ui.window("##statusbar")
        .flags(flags)
        .position([0.0, display[1] - height], Condition::Always)
        .size([display[0], height], Condition::Always)
        .build(|| match &snap.doc {
            None => ui.text_disabled("No document"),
            Some(doc) => {
                ui.text(file_text(doc.path.as_deref(), doc.dirty));
                ui.same_line();
                ui.text_colored(
                    header_color(doc.errors, doc.warnings),
                    problems_text(doc.errors, doc.warnings),
                );
                ui.same_line();
                ui.text(nodes_text(doc.nodes));
                ui.same_line();
                ui.text(zoom_text(doc.zoom));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DialogueDoc, Workspace};
    use aberredengine::prelude::Vec2;

    #[test]
    fn file_text_shows_the_path_or_untitled_and_the_dirty_star() {
        assert_eq!(file_text(None, false), "Untitled");
        assert_eq!(file_text(None, true), "Untitled*");
        let path = Path::new("/tmp/dir/guard.json");
        assert_eq!(file_text(Some(path), false), "/tmp/dir/guard.json");
        assert_eq!(file_text(Some(path), true), "/tmp/dir/guard.json*");
    }

    #[test]
    fn problems_text_pluralises_and_matches_the_spec_shape() {
        assert_eq!(problems_text(0, 0), "0 errors / 0 warnings");
        assert_eq!(problems_text(2, 1), "2 errors / 1 warning");
        assert_eq!(problems_text(1, 0), "1 error / 0 warnings");
        assert_eq!(problems_text(0, 3), "0 errors / 3 warnings");
    }

    #[test]
    fn nodes_text_pluralises() {
        assert_eq!(nodes_text(0), "0 nodes");
        assert_eq!(nodes_text(1), "1 node");
        assert_eq!(nodes_text(7), "7 nodes");
    }

    #[test]
    fn zoom_text_rounds_the_percentage() {
        assert_eq!(zoom_text(1.0), "zoom 100%");
        assert_eq!(zoom_text(0.5), "zoom 50%");
        assert_eq!(zoom_text(2.5), "zoom 250%");
        assert_eq!(zoom_text(1.234), "zoom 123%");
        assert_eq!(zoom_text(0.1), "zoom 10%");
        assert_eq!(zoom_text(4.0), "zoom 400%");
    }

    #[test]
    fn snapshot_without_a_doc_or_workspace_reports_no_document() {
        let app_state = AppState::default();
        let snap = snapshot(&app_state);
        assert!(snap.doc.is_none(), "no WorkspaceMutex handle");

        let handle: WorkspaceMutex =
            std::sync::Arc::new(std::sync::Mutex::new(Workspace::default()));
        let mut app_state = AppState::default();
        app_state.insert(handle);
        let snap = snapshot(&app_state);
        assert!(snap.doc.is_none(), "no active doc");
    }

    #[test]
    fn snapshot_captures_path_dirty_counts_nodes_and_zoom() {
        let mut app_state = AppState::default();
        let handle: WorkspaceMutex = std::sync::Arc::new(std::sync::Mutex::new({
            let mut ws = Workspace::default();
            crate::ops::new_doc(&mut ws);
            let doc = ws.active_doc_mut().unwrap();
            let a = doc.add_node(Vec2::ZERO);
            let b = doc.add_node(Vec2::new(10.0, 0.0));
            doc.set_link(a, crate::model::Slot::Next, crate::model::Link::Node(b));
            doc.set_camera(crate::model::CamState {
                target: Vec2::new(1.0, 2.0),
                zoom: 1.5,
            });
            doc.path = Some(std::path::PathBuf::from("/tmp/x/guard.json"));
            ws
        }));
        app_state.insert(handle);
        let snap = snapshot(&app_state);
        let doc = snap.doc.expect("a doc is open");
        assert_eq!(
            doc.path,
            Some(std::path::PathBuf::from("/tmp/x/guard.json"))
        );
        assert!(doc.dirty, "adding nodes dirties the doc");
        assert_eq!(
            (doc.errors, doc.warnings),
            (0, 0),
            "a valid two-node doc is clean"
        );
        assert_eq!(doc.nodes, 2);
        assert!((doc.zoom - 1.5).abs() < 1e-6);

        // The drawn strings come from the pure helpers over the snapshot values.
        assert_eq!(
            file_text(doc.path.as_deref(), doc.dirty),
            "/tmp/x/guard.json*"
        );
        assert_eq!(
            problems_text(doc.errors, doc.warnings),
            "0 errors / 0 warnings"
        );
        assert_eq!(nodes_text(doc.nodes), "2 nodes");
        assert_eq!(zoom_text(doc.zoom), "zoom 150%");

        // An untitled clean doc.
        let mut ws = Workspace::default();
        let mut doc = DialogueDoc::new_empty();
        doc.add_node(Vec2::ZERO);
        doc.mark_clean();
        ws.add_doc(doc);
        let handle: WorkspaceMutex = std::sync::Arc::new(std::sync::Mutex::new(ws));
        let mut app_state = AppState::default();
        app_state.insert(handle);
        let snap = snapshot(&app_state);
        let doc = snap.doc.expect("a doc is open");
        assert_eq!(file_text(doc.path.as_deref(), doc.dirty), "Untitled");
        assert_eq!(nodes_text(doc.nodes), "1 node");
    }
}

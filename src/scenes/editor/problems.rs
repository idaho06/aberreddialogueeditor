//! The "Problems" window: the active doc's lint problems (from `doc.problems()`, cached on
//! `content_revision`), grouped errors-first, plus its load warnings in a collapsed section.
//! Clicking a row whose node still exists pushes [`EditorAction::FocusNode`], which selects the
//! node and centres the camera on it.
//!
//! Locking (the standard panel pattern): the workspace is locked only to snapshot the problems,
//! their node-exists flags, the load warnings and the visibility flag; the widgets are drawn
//! with the guard dropped, and the collected actions are applied by the caller (`gui.rs`).
//!
//! Window ids: rows are wrapped in `push_id_usize(index)` and use a fixed `###row` label suffix,
//! the load-warnings header a fixed `###loadwarn` — never the (content-carrying) message text.

use aberredengine::prelude::imgui::{Condition, TreeNodeFlags};
use aberredengine::prelude::*;

use super::actions::EditorAction;
use crate::model::{NodeKey, Problem, Severity};
use crate::state::with_workspace;

/// Red for errors / a non-clean header.
const RED: [f32; 4] = [1.0, 0.35, 0.35, 1.0];
/// Amber for warnings / a warnings-only header.
const AMBER: [f32; 4] = [1.0, 0.75, 0.3, 1.0];
/// Green for "No problems".
const GREEN: [f32; 4] = [0.4, 0.85, 0.4, 1.0];

/// Snapshot of the Problems panel, taken under a short workspace lock.
struct ProblemsSnap {
    /// Whether the window is shown (`!ws.problems_hidden`).
    visible: bool,
    /// The cloned problems of the active doc, or `None` when no doc is open.
    problems: Option<Vec<Problem>>,
    /// Parallel to `problems`: whether the problem's node still exists in the doc (a
    /// `StartMissing` problem may name a stale key).
    node_exists: Vec<bool>,
    load_warnings: Vec<String>,
}

impl Default for ProblemsSnap {
    /// No workspace handle at all: the window is visible (the derived `Workspace` default) and
    /// there is nothing to show.
    fn default() -> Self {
        ProblemsSnap {
            visible: true,
            problems: None,
            node_exists: Vec::new(),
            load_warnings: Vec::new(),
        }
    }
}

fn snapshot(app_state: &AppState) -> ProblemsSnap {
    with_workspace(app_state, |ws| {
        let visible = !ws.problems_hidden;
        let Some(doc) = ws.active_doc_mut() else {
            return ProblemsSnap {
                visible,
                ..ProblemsSnap::default()
            };
        };
        let problems = doc.problems().to_vec();
        let node_exists = problems
            .iter()
            .map(|p| p.node.is_some_and(|key| doc.node(key).is_some()))
            .collect();
        ProblemsSnap {
            visible,
            problems: Some(problems),
            node_exists,
            load_warnings: doc.load_warnings.clone(),
        }
    })
    .unwrap_or_default()
}

/// `(errors, warnings)` counts of `problems`.
pub fn problem_counts(problems: &[Problem]) -> (usize, usize) {
    let errors = problems
        .iter()
        .filter(|p| p.severity == Severity::Error)
        .count();
    (errors, problems.len() - errors)
}

/// Display grouping of `problems`: all errors first, then all warnings, each group keeping the
/// original (lint) order. Entries are `(severity, index into problems)`.
pub fn grouped_rows(problems: &[Problem]) -> Vec<(Severity, usize)> {
    let mut rows = Vec::new();
    for severity in [Severity::Error, Severity::Warning] {
        rows.extend(
            problems
                .iter()
                .enumerate()
                .filter(|(_, p)| p.severity == severity)
                .map(|(i, p)| (p.severity, i)),
        );
    }
    rows
}

/// The node a problem row focuses on click: `Some(key)` only when the problem names a node AND
/// that node still exists (a stale `StartMissing` key is shown but not clickable).
pub fn row_target(problem: &Problem, node_exists: bool) -> Option<NodeKey> {
    problem.node.filter(|_| node_exists)
}

/// The header line: "N errors, M warnings", or "No problems" when both are 0.
pub fn header_text(errors: usize, warnings: usize) -> String {
    if errors == 0 && warnings == 0 {
        "No problems".to_owned()
    } else {
        format!("{errors} errors, {warnings} warnings")
    }
}

/// The header colour: red with errors, amber with warnings only, green when clean.
pub fn header_color(errors: usize, warnings: usize) -> [f32; 4] {
    if errors > 0 {
        RED
    } else if warnings > 0 {
        AMBER
    } else {
        GREEN
    }
}

/// The severity colour of a group label / row marker.
pub fn severity_color(severity: Severity) -> [f32; 4] {
    match severity {
        Severity::Error => RED,
        Severity::Warning => AMBER,
    }
}

/// The severity marker drawn before a row's message.
fn severity_marker(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "E",
        Severity::Warning => "W",
    }
}

/// Whether the Problems window is drawn at all. Dear ImGui's `Begin(name, p_open, ..)` only uses
/// `p_open` to show the title-bar close button and to write `false` into it when that button is
/// clicked — it does NOT hide a window whose `*p_open` is already false (the imgui demo code
/// always guards with `if (show_x) ShowX(&show_x)`). A hidden window therefore must not be
/// built at all. `visible` is `!ws.problems_hidden`.
fn should_draw(visible: bool) -> bool {
    visible
}

/// Draws the Problems window; pushes [`EditorAction::ToggleProblems`] when its title-bar close
/// button hides it, and [`EditorAction::FocusNode`] for a clicked row whose node still exists.
pub fn draw(ctx: &mut GuiCtx, actions: &mut Vec<EditorAction>) {
    let snap = snapshot(ctx.app_state);
    // A hidden window is not drawn at all — imgui's `p_open` does not hide it (see `should_draw`).
    if !should_draw(snap.visible) {
        return;
    }
    let ui = ctx.ui;
    let display = ui.io().display_size;
    // Default: bottom-left, above the status bar, ~420x200 — only on first use.
    let status_height = ui.frame_height_with_spacing();
    let mut open = snap.visible;
    let mut focus = None;
    ui.window("Problems")
        .position(
            [8.0, display[1] - status_height - 208.0],
            Condition::FirstUseEver,
        )
        .size([420.0, 200.0], Condition::FirstUseEver)
        .opened(&mut open)
        .build(|| {
            let Some(problems) = &snap.problems else {
                ui.text_disabled("No document");
                return;
            };
            let (errors, warnings) = problem_counts(problems);
            ui.text_colored(
                header_color(errors, warnings),
                header_text(errors, warnings),
            );
            let grouped = grouped_rows(problems);
            for severity in [Severity::Error, Severity::Warning] {
                let group: Vec<usize> = grouped
                    .iter()
                    .filter(|(s, _)| *s == severity)
                    .map(|(_, i)| *i)
                    .collect();
                if group.is_empty() {
                    continue;
                }
                let label = match severity {
                    Severity::Error => "Errors",
                    Severity::Warning => "Warnings",
                };
                ui.text_colored(severity_color(severity), label);
                for i in group {
                    let problem = &problems[i];
                    let target = row_target(problem, snap.node_exists[i]);
                    // The row id comes from the index, never from the message.
                    let _id = ui.push_id_usize(i);
                    ui.text_colored(severity_color(severity), severity_marker(severity));
                    ui.same_line();
                    let row_label = format!("{}###row", problem.message);
                    if ui
                        .selectable_config(row_label)
                        .disabled(target.is_none())
                        .build()
                        && let Some(key) = target
                    {
                        focus = Some(key);
                    }
                }
            }
            let n = snap.load_warnings.len();
            if n > 0
                && ui.collapsing_header(
                    format!("Load warnings ({n})###loadwarn"),
                    TreeNodeFlags::empty(),
                )
            {
                for warning in &snap.load_warnings {
                    ui.bullet_text(warning);
                }
            }
        });

    // The title-bar close button hides the window: flip the workspace flag.
    if snap.visible && !open {
        actions.push(EditorAction::ToggleProblems);
    }
    if let Some(key) = focus {
        actions.push(EditorAction::FocusNode(key));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DialogueDoc, ProblemKind, Workspace, from_def, lint};
    use crate::state::WorkspaceMutex;
    use crate::testkit::{app_state_with, fixture, push_valid_doc};
    use aberredengine::prelude::{DialogueDef, Vec2};

    fn prob(severity: Severity, kind: ProblemKind, node: Option<NodeKey>, msg: &str) -> Problem {
        Problem {
            severity,
            kind,
            node,
            message: msg.to_owned(),
        }
    }

    #[test]
    fn problem_counts_returns_errors_and_warnings() {
        assert_eq!(problem_counts(&[]), (0, 0));
        let problems = vec![
            prob(Severity::Error, ProblemKind::StartMissing, None, "a"),
            prob(
                Severity::Warning,
                ProblemKind::Unreachable,
                Some(NodeKey(1)),
                "b",
            ),
            prob(
                Severity::Error,
                ProblemKind::DuplicateId,
                Some(NodeKey(2)),
                "c",
            ),
            prob(
                Severity::Warning,
                ProblemKind::Unreachable,
                Some(NodeKey(3)),
                "d",
            ),
        ];
        assert_eq!(problem_counts(&problems), (2, 2));
    }

    #[test]
    fn problem_counts_agrees_with_lint_on_fixtures_and_the_cached_problems() {
        for name in [
            "guard.json",
            "loops.json",
            "invalid_dangling.json",
            "invalid_dup_id.json",
            "invalid_no_start.json",
        ] {
            let json = std::fs::read_to_string(fixture(name)).expect("fixture exists");
            let def: DialogueDef = serde_json::from_str(&json).expect("fixture parses");
            let mut doc = from_def(&def);
            let fresh = lint(&doc);
            let cached = doc.problems().to_vec();
            assert_eq!(cached, fresh, "{name}: the cache equals a fresh lint");
            assert_eq!(
                problem_counts(&fresh),
                problem_counts(&cached),
                "{name}: counts agree"
            );
            // Spot-check the expected split (dangling fixture: 2 dangling targets + 1
            // unreachable; dup-id fixture: 2 duplicate ids + 2 unreachable).
            let (errors, warnings) = problem_counts(&fresh);
            match name {
                "guard.json" | "loops.json" => assert_eq!((errors, warnings), (0, 0)),
                "invalid_dangling.json" => assert_eq!((errors, warnings), (2, 1)),
                "invalid_dup_id.json" => assert_eq!((errors, warnings), (2, 2)),
                "invalid_no_start.json" => assert_eq!((errors, warnings), (1, 0)),
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn grouped_rows_puts_errors_first_then_warnings_keeping_original_order() {
        let problems = vec![
            prob(
                Severity::Warning,
                ProblemKind::Unreachable,
                Some(NodeKey(1)),
                "w0",
            ),
            prob(
                Severity::Error,
                ProblemKind::DuplicateId,
                Some(NodeKey(2)),
                "e0",
            ),
            prob(
                Severity::Warning,
                ProblemKind::Unreachable,
                Some(NodeKey(3)),
                "w1",
            ),
            prob(Severity::Error, ProblemKind::StartMissing, None, "e1"),
        ];
        assert_eq!(
            grouped_rows(&problems),
            vec![
                (Severity::Error, 1),
                (Severity::Error, 3),
                (Severity::Warning, 0),
                (Severity::Warning, 2),
            ],
            "errors first, then warnings; original order kept inside each group"
        );
        assert!(grouped_rows(&[]).is_empty());
    }

    #[test]
    fn grouped_rows_keeps_the_lint_order_inside_each_group() {
        // A doc with two duplicate-id errors and two unreachable warnings.
        let mut doc = DialogueDoc::new_empty();
        let a = doc.add_node(Vec2::ZERO);
        let b = doc.add_node(Vec2::new(10.0, 0.0));
        doc.add_node(Vec2::new(20.0, 0.0)); // unreachable
        doc.add_node(Vec2::new(30.0, 0.0)); // unreachable
        doc.rename_node(a, "dup".to_owned());
        doc.rename_node(b, "dup".to_owned());
        // a -> b, so only the last two nodes are unreachable.
        doc.set_link(a, crate::model::Slot::Next, crate::model::Link::Node(b));
        let problems = lint(&doc);
        let rows = grouped_rows(&problems);
        let errors: Vec<usize> = rows
            .iter()
            .filter(|(s, _)| *s == Severity::Error)
            .map(|(_, i)| *i)
            .collect();
        let warnings: Vec<usize> = rows
            .iter()
            .filter(|(s, _)| *s == Severity::Warning)
            .map(|(_, i)| *i)
            .collect();
        // lint order: DuplicateId(a), DuplicateId(b), Unreachable(c), Unreachable(d).
        assert_eq!(errors, [0, 1], "duplicate-id errors in lint order");
        assert_eq!(warnings, [2, 3], "unreachable warnings in lint order");
        assert_eq!(
            rows.len(),
            problems.len(),
            "every problem appears exactly once"
        );
        assert!(
            rows[..2].iter().all(|(s, _)| *s == Severity::Error),
            "all errors come first"
        );
    }

    #[test]
    fn row_target_is_clickable_only_for_an_existing_node() {
        let stale = prob(
            Severity::Error,
            ProblemKind::StartMissing,
            Some(NodeKey(999)),
            "start",
        );
        assert_eq!(
            row_target(&stale, false),
            None,
            "a stale StartMissing key is not clickable"
        );
        assert_eq!(
            row_target(&stale, true),
            Some(NodeKey(999)),
            "an existing node is clickable"
        );
        let no_node = prob(Severity::Error, ProblemKind::StartMissing, None, "no start");
        assert_eq!(row_target(&no_node, true), None, "no node: not clickable");
        assert_eq!(row_target(&no_node, false), None);
    }

    #[test]
    fn header_text_and_color_follow_the_counts() {
        assert_eq!(header_text(0, 0), "No problems");
        assert_eq!(header_text(2, 1), "2 errors, 1 warnings");
        assert_eq!(header_text(0, 3), "0 errors, 3 warnings");
        assert_eq!(header_color(0, 0), GREEN);
        assert_eq!(header_color(1, 0), RED);
        assert_eq!(header_color(3, 2), RED, "any error makes it red");
        assert_eq!(header_color(0, 1), AMBER, "warnings only: amber");
    }

    #[test]
    fn severity_colors_and_markers() {
        assert_eq!(severity_color(Severity::Error), RED);
        assert_eq!(severity_color(Severity::Warning), AMBER);
        assert_eq!(severity_marker(Severity::Error), "E");
        assert_eq!(severity_marker(Severity::Warning), "W");
    }

    #[test]
    fn snapshot_without_a_workspace_handle_shows_no_document_and_visible() {
        let app_state = AppState::default();
        let snap = snapshot(&app_state);
        assert!(snap.visible, "visible by default (no handle)");
        assert!(snap.problems.is_none(), "no doc without a workspace");
        assert!(snap.node_exists.is_empty());
        assert!(snap.load_warnings.is_empty());
    }

    #[test]
    fn snapshot_captures_problems_node_existence_warnings_and_visibility() {
        let app_state = app_state_with({
            let mut ws = Workspace::default();
            push_valid_doc(&mut ws);
            let doc = ws.active_doc_mut().unwrap();
            doc.start = Some(NodeKey(999)); // stale start key: StartMissing with node_exists=false
            doc.load_warnings.push("warned".to_owned());
            ws
        });
        let snap = snapshot(&app_state);
        assert!(snap.visible);
        let problems = snap.problems.expect("a doc is open");
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].kind, ProblemKind::StartMissing);
        assert_eq!(problems[0].node, Some(NodeKey(999)));
        assert_eq!(
            snap.node_exists,
            vec![false],
            "the stale key no longer exists"
        );
        assert_eq!(snap.load_warnings, vec!["warned".to_owned()]);

        // Hidden flag is reflected.
        let handle: WorkspaceMutex = app_state.get::<WorkspaceMutex>().unwrap().clone();
        {
            let mut ws = handle.lock().unwrap();
            ws.problems_hidden = true;
        }
        let snap = snapshot(&app_state);
        assert!(!snap.visible, "problems_hidden=true means hidden");
    }
}

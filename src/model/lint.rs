//! Live linting of a [`DialogueDoc`] plus the engine's save gate.
//!
//! [`lint`] reports ALL problems of a doc (not just the first), in a deterministic order: doc-level
//! problems first, then by node order, then by [`ProblemKind`] (declaration order). [`engine_gate`]
//! is the save gate — it runs the engine's own `DialogueTree::try_from(to_def(doc))` validation and
//! the engine rules are never reimplemented here. Pure: no ECS, no imgui.

use std::collections::{HashMap, HashSet};

use aberredengine::prelude::{DialogueError, DialogueTree};

use crate::model::convert::to_def;
use crate::model::doc::{DialogueDoc, EdChoice, Link, NodeKey, Outputs};

/// How bad a [`Problem`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Blocks saving (the engine gate rejects the doc, or the model rule is stricter than the engine).
    Error,
    /// Reported but does not block saving.
    Warning,
}

/// What kind of problem was found. Declaration order is the report order within one node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProblemKind {
    /// `start` is `None`, or names a key that is no longer a node of the doc.
    StartMissing,
    /// A node id is empty or all whitespace (stricter than the engine, which accepts an empty id).
    EmptyId,
    /// Several nodes share one id; reported for EVERY node carrying that id.
    DuplicateId,
    /// A `next` or choice target is a [`Link::Dangling`] reference.
    DanglingTarget,
    /// The node is not reachable from `start` by following [`Link::Node`] edges (warning only;
    /// skipped when the start is missing, to avoid a flood of warnings).
    Unreachable,
}

/// One lint finding.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub severity: Severity,
    pub kind: ProblemKind,
    /// The node the problem belongs to; `None` for doc-level problems such as a missing start.
    pub node: Option<NodeKey>,
    pub message: String,
}

/// Reports every problem of `doc`, deterministically ordered: the doc-level [`ProblemKind::StartMissing`]
/// first, then per node in `doc.nodes` order, and within a node by [`ProblemKind`] order (a node's
/// `next` dangling link is reported before its choices' dangling links).
pub fn lint(doc: &DialogueDoc) -> Vec<Problem> {
    let mut problems = Vec::new();

    let start = doc.start.filter(|key| doc.node(*key).is_some());
    if start.is_none() {
        let message = match doc.start {
            Some(key) => format!("start key {} is not a node of this doc", key.0),
            None => "the doc has no start node".to_owned(),
        };
        problems.push(Problem {
            severity: Severity::Error,
            kind: ProblemKind::StartMissing,
            node: doc.start,
            message,
        });
    }

    let mut id_counts: HashMap<&str, usize> = HashMap::new();
    for node in &doc.nodes {
        *id_counts.entry(node.id.as_str()).or_default() += 1;
    }

    // Reachable set from start over Link::Node edges only (BFS; cycles and self-loops are fine).
    // None when the start is missing: Unreachable is skipped entirely then.
    let reachable: Option<HashSet<NodeKey>> = start.map(|start_key| {
        let mut seen = HashSet::new();
        let mut stack = vec![start_key];
        while let Some(key) = stack.pop() {
            if !seen.insert(key) {
                continue;
            }
            if let Some(node) = doc.node(key) {
                for link in out_links(&node.out) {
                    if let Link::Node(target) = link {
                        stack.push(*target);
                    }
                }
            }
        }
        seen
    });

    for node in &doc.nodes {
        let key = node.key;
        if node.id.trim().is_empty() {
            problems.push(Problem {
                severity: Severity::Error,
                kind: ProblemKind::EmptyId,
                node: Some(key),
                message: format!("node #{} has an empty or all-whitespace id", key.0),
            });
        }
        if id_counts.get(node.id.as_str()).is_some_and(|&n| n > 1) {
            problems.push(Problem {
                severity: Severity::Error,
                kind: ProblemKind::DuplicateId,
                node: Some(key),
                message: format!("duplicate node id \"{}\"", node.id),
            });
        }
        for link in out_links(&node.out) {
            if let Link::Dangling(target) = link {
                problems.push(Problem {
                    severity: Severity::Error,
                    kind: ProblemKind::DanglingTarget,
                    node: Some(key),
                    // Mirrors the engine's DialogueError::UnknownTarget Display.
                    message: format!(
                        "dialogue node \"{}\" links to unknown node \"{}\"",
                        node.id, target
                    ),
                });
            }
        }
        if let Some(reachable) = &reachable
            && !reachable.contains(&key)
        {
            problems.push(Problem {
                severity: Severity::Warning,
                kind: ProblemKind::Unreachable,
                node: Some(key),
                message: format!("node \"{}\" is unreachable from the start node", node.id),
            });
        }
    }

    problems
}

/// A node's outgoing links in report order: the `next` link first, then the choices' targets.
fn out_links(out: &Outputs) -> impl Iterator<Item = &Link> {
    let (next, choices): (Option<&Link>, &[EdChoice]) = match out {
        Outputs::Next(link) => (Some(link), &[]),
        Outputs::Choices(choices) => (None, choices),
    };
    next.into_iter()
        .chain(choices.iter().map(|choice| &choice.target))
}

/// The save gate: the engine's own validation of `to_def(doc)`. Never reimplemented in the model.
pub fn engine_gate(doc: &DialogueDoc) -> Result<(), DialogueError> {
    DialogueTree::try_from(to_def(doc)).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::convert::{from_def, to_def};
    use crate::model::doc::{DialogueDoc, NodeKey, Outputs, Slot};
    use aberredengine::prelude::{DialogueDef, Vec2};

    /// True when `problems` contains at least one [`Severity::Error`].
    fn has_errors(problems: &[Problem]) -> bool {
        problems.iter().any(|p| p.severity == Severity::Error)
    }

    macro_rules! fixture {
        ($name:literal) => {
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/",
                $name,
                ".json"
            ))
        };
    }
    const GUARD: &str = fixture!("guard");
    const LOOPS: &str = fixture!("loops");
    const DANGLING: &str = fixture!("invalid_dangling");
    const DUP_ID: &str = fixture!("invalid_dup_id");
    const BOTH: &str = fixture!("invalid_both_next_choices");
    const EMPTY_CHOICES: &str = fixture!("invalid_empty_choices");
    const NO_START: &str = fixture!("invalid_no_start");

    fn parse(json: &str) -> DialogueDef {
        serde_json::from_str(json).expect("fixture must parse as DialogueDef")
    }

    fn fixture_docs() -> Vec<(&'static str, DialogueDoc)> {
        [
            ("guard", GUARD),
            ("loops", LOOPS),
            ("dangling", DANGLING),
            ("dup_id", DUP_ID),
            ("both", BOTH),
            ("empty_choices", EMPTY_CHOICES),
            ("no_start", NO_START),
        ]
        .into_iter()
        .map(|(name, json)| (name, from_def(&parse(json))))
        .collect()
    }

    /// A doc with `a --next--> b`, `b` having one choice ("go", target End), start = a.
    fn base_doc() -> (DialogueDoc, NodeKey, NodeKey) {
        let mut doc = DialogueDoc::new_empty();
        let a = doc.add_node(Vec2::ZERO);
        let b = doc.add_node(Vec2::new(10.0, 0.0));
        doc.set_link(a, Slot::Next, Link::Node(b));
        doc.add_choice(b);
        doc.set_choice_text(b, 0, "go".to_owned());
        (doc, a, b)
    }

    /// The engine gate's error for a doc that must fail it.
    fn gate_err(doc: &DialogueDoc) -> DialogueError {
        engine_gate(doc).expect_err("the mutation must fail the engine gate")
    }

    /// The problems of `kind`, in report order.
    fn of_kind(problems: &[Problem], kind: ProblemKind) -> impl Iterator<Item = &Problem> {
        problems.iter().filter(move |p| p.kind == kind)
    }

    /// True when `problems` contains an [`Severity::Error`] of `kind`.
    fn has_error_of_kind(problems: &[Problem], kind: ProblemKind) -> bool {
        of_kind(problems, kind).any(|p| p.severity == Severity::Error)
    }

    /// True when `key` is reported [`ProblemKind::Unreachable`].
    fn is_unreachable(problems: &[Problem], key: NodeKey) -> bool {
        of_kind(problems, ProblemKind::Unreachable).any(|p| p.node == Some(key))
    }

    fn has_no_kind(problems: &[Problem], kind: ProblemKind) -> bool {
        of_kind(problems, kind).next().is_none()
    }

    /// The `content_revision` stored in the lint cache, if any.
    fn cached_content_revision(doc: &DialogueDoc) -> Option<u64> {
        doc.lint_cache.as_ref().map(|(rev, _)| *rev)
    }

    fn problem(severity: Severity, kind: ProblemKind) -> Problem {
        Problem {
            severity,
            kind,
            node: None,
            message: String::new(),
        }
    }

    type Mutator = fn(&mut DialogueDoc, NodeKey, NodeKey);

    /// Mutations applied to a base doc; args are (first node key, second node key or the first).
    /// All are no-op safe when their node is gone.
    fn mutators() -> Vec<(&'static str, Mutator)> {
        vec![
            ("rename_a_to_b_id", |d, a, b| {
                if let Some(id) = d.node(b).map(|n| n.id.clone()) {
                    d.rename_node(a, id);
                }
            }),
            ("delete_a", |d, a, _| d.delete_node(a)),
            ("delete_b", |d, _, b| d.delete_node(b)),
            ("dangling_next", |d, a, _| {
                d.set_link(a, Slot::Next, Link::Dangling("ghost".to_owned()))
            }),
            ("dangling_choice", |d, _, b| {
                d.add_choice(b);
                let last = match d.node(b).map(|n| &n.out) {
                    Some(Outputs::Choices(v)) => v.len() - 1,
                    _ => return,
                };
                d.set_link(b, Slot::Choice(last), Link::Dangling("ghost2".to_owned()));
            }),
            ("empty_id", |d, a, _| d.rename_node(a, String::new())),
            ("whitespace_id", |d, a, _| {
                d.rename_node(a, " \t ".to_owned())
            }),
            ("clear_start", |d, _, _| d.start = None),
            ("cut_link", |d, a, _| d.set_link(a, Slot::Next, Link::End)),
            ("choice_end", |d, _, b| {
                d.set_link(b, Slot::Choice(0), Link::End)
            }),
            ("self_loop", |d, a, _| {
                d.set_link(a, Slot::Next, Link::Node(a))
            }),
            ("choice_to_a", |d, a, b| {
                d.set_link(b, Slot::Choice(0), Link::Node(a))
            }),
        ]
    }

    /// All fixture docs plus the generated mutation family: every sequence of 1..=2 mutators on
    /// every fixture, 1..=3 on guard.json.
    fn all_docs() -> Vec<DialogueDoc> {
        let muts = mutators();
        let mut docs = Vec::new();
        for (name, base) in fixture_docs() {
            let a = base.nodes.first().map(|n| n.key).unwrap_or(NodeKey(0));
            let b = base.nodes.get(1).map(|n| n.key).unwrap_or(a);
            let depth = if name == "guard" { 3 } else { 2 };
            docs.push(base.clone());
            let mut level = vec![base.clone()];
            for _ in 1..=depth {
                let mut next = Vec::new();
                for doc in &level {
                    for (_, m) in &muts {
                        let mut child = doc.clone();
                        m(&mut child, a, b);
                        docs.push(child.clone());
                        next.push(child);
                    }
                }
                level = next;
            }
        }
        docs
    }

    #[test]
    fn soundness_lint_without_errors_implies_engine_gate_ok() {
        let docs = all_docs();
        assert!(
            docs.len() > 2000,
            "mutation family is generated: {}",
            docs.len()
        );
        for doc in &docs {
            let problems = lint(doc);
            if !has_errors(&problems) {
                assert!(
                    engine_gate(doc).is_ok(),
                    "lint-clean doc must pass the engine gate; problems: {problems:?}; doc: {doc:?}"
                );
            }
        }
    }

    #[test]
    fn coverage_engine_errors_map_to_lint_errors() {
        // StartMissing: no start at all.
        let (mut doc, _a, _b) = base_doc();
        doc.start = None;
        let err = gate_err(&doc);
        assert!(
            matches!(&err, DialogueError::StartMissing { .. }),
            "got {err:?}"
        );
        assert!(
            has_error_of_kind(&lint(&doc), ProblemKind::StartMissing),
            "lint must report StartMissing as an Error"
        );

        // StartMissing: start key no longer present.
        let (mut doc, _a, _b) = base_doc();
        doc.start = Some(NodeKey(999));
        let err = gate_err(&doc);
        assert!(
            matches!(&err, DialogueError::StartMissing { .. }),
            "got {err:?}"
        );
        assert!(has_error_of_kind(&lint(&doc), ProblemKind::StartMissing));

        // DuplicateNode: every node sharing the id is reported, message names the id.
        let (mut doc, a, b) = base_doc();
        let bid = doc.node(b).expect("b exists").id.clone();
        doc.rename_node(a, bid.clone());
        let err = gate_err(&doc);
        assert!(
            matches!(&err, DialogueError::DuplicateNode { id } if id == &bid),
            "got {err:?}"
        );
        let problems = lint(&doc);
        let dups: Vec<&Problem> = of_kind(&problems, ProblemKind::DuplicateId).collect();
        assert_eq!(dups.len(), 2, "every node with the shared id is reported");
        assert!(dups.iter().all(|p| p.severity == Severity::Error));
        assert!(dups.iter().all(|p| p.node == Some(a) || p.node == Some(b)));
        assert!(
            dups.iter()
                .all(|p| p.message == format!("duplicate node id \"{bid}\"")),
            "message names the id: {dups:?}"
        );

        // UnknownTarget through `next`; the lint message mirrors the engine's Display.
        let (mut doc, a, _b) = base_doc();
        doc.set_link(a, Slot::Next, Link::Dangling("ghost".to_owned()));
        let err = gate_err(&doc);
        assert!(
            matches!(&err, DialogueError::UnknownTarget { node, target } if node == "node_1" && target == "ghost"),
            "got {err:?}"
        );
        let problems = lint(&doc);
        let dangling: Vec<&Problem> = of_kind(&problems, ProblemKind::DanglingTarget).collect();
        assert_eq!(dangling.len(), 1);
        assert_eq!(dangling[0].severity, Severity::Error);
        assert_eq!(dangling[0].node, Some(a));
        assert_eq!(
            dangling[0].message,
            err.to_string(),
            "lint message mirrors the engine's UnknownTarget Display"
        );

        // UnknownTarget through a choice.
        let (mut doc, _a, b) = base_doc();
        doc.set_link(b, Slot::Choice(0), Link::Dangling("ghost2".to_owned()));
        let err = gate_err(&doc);
        assert!(
            matches!(&err, DialogueError::UnknownTarget { node, target } if node == "node_2" && target == "ghost2"),
            "got {err:?}"
        );
        assert!(
            of_kind(&lint(&doc), ProblemKind::DanglingTarget)
                .any(|p| p.severity == Severity::Error && p.node == Some(b))
        );
    }

    #[test]
    fn to_def_never_emits_next_and_choices_together_or_an_empty_choices_list() {
        for doc in all_docs() {
            for node in &to_def(&doc).nodes {
                assert!(
                    !(node.next.is_some() && node.choices.is_some()),
                    "node \"{}\" must not set both next and choices",
                    node.id
                );
                assert!(
                    node.choices.as_ref().is_none_or(|v| !v.is_empty()),
                    "node \"{}\" must not emit an empty choices list",
                    node.id
                );
            }
        }
    }

    #[test]
    fn unreachable_is_a_warning_only_and_passes_the_engine_gate() {
        let (mut doc, a, b) = base_doc();
        doc.set_link(a, Slot::Next, Link::End); // b is now unreachable
        assert!(
            engine_gate(&doc).is_ok(),
            "an unreachable node is not an engine error"
        );
        let problems = lint(&doc);
        assert!(!has_errors(&problems), "no Error: {problems:?}");
        let unreachable: Vec<&Problem> = of_kind(&problems, ProblemKind::Unreachable).collect();
        assert_eq!(unreachable.len(), 1);
        assert_eq!(unreachable[0].severity, Severity::Warning);
        assert_eq!(unreachable[0].node, Some(b));
    }

    #[test]
    fn bfs_handles_cycles_and_self_loops() {
        // Self-loop: a stays reachable, b becomes unreachable.
        let (mut doc, a, b) = base_doc();
        doc.set_link(a, Slot::Next, Link::Node(a));
        let problems = lint(&doc);
        assert!(
            !is_unreachable(&problems, a),
            "a is reachable through its self-loop: {problems:?}"
        );
        assert!(is_unreachable(&problems, b));

        // Two-node cycle a <-> b: everything reachable, no Unreachable at all.
        let (mut doc, a, b) = base_doc();
        doc.set_link(b, Slot::Choice(0), Link::Node(a));
        let problems = lint(&doc);
        assert!(
            has_no_kind(&problems, ProblemKind::Unreachable),
            "cycle a<->b: all nodes reachable: {problems:?}"
        );

        // loops.json (a<->b cycle plus a join on c, detour d): no Unreachable, engine Ok.
        let loops = from_def(&parse(LOOPS));
        assert!(has_no_kind(&lint(&loops), ProblemKind::Unreachable));
        assert!(engine_gate(&loops).is_ok());
    }

    #[test]
    fn unreachable_is_skipped_when_start_is_missing() {
        let doc = from_def(&parse(NO_START));
        let problems = lint(&doc);
        assert!(has_error_of_kind(&problems, ProblemKind::StartMissing));
        assert!(
            has_no_kind(&problems, ProblemKind::Unreachable),
            "no flood of Unreachable warnings without a start: {problems:?}"
        );

        let empty = DialogueDoc::new_empty();
        let problems = lint(&empty);
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].kind, ProblemKind::StartMissing);
        assert_eq!(problems[0].node, None);
    }

    #[test]
    fn lint_reports_all_problems_in_node_order_then_kind() {
        let (mut doc, a, b) = base_doc();
        let c = doc.add_node(Vec2::new(20.0, 0.0));
        doc.rename_node(a, "dup".to_owned());
        doc.rename_node(b, "dup".to_owned());
        doc.rename_node(c, String::new());
        doc.set_link(a, Slot::Next, Link::Dangling("ghost".to_owned()));
        // start = a; a's next dangles, so only a is reachable; b and c are Unreachable.
        let problems = lint(&doc);
        let order: Vec<(Option<NodeKey>, ProblemKind)> =
            problems.iter().map(|p| (p.node, p.kind)).collect();
        assert_eq!(
            order,
            vec![
                (Some(a), ProblemKind::DuplicateId),
                (Some(a), ProblemKind::DanglingTarget),
                (Some(b), ProblemKind::DuplicateId),
                (Some(b), ProblemKind::Unreachable),
                (Some(c), ProblemKind::EmptyId),
                (Some(c), ProblemKind::Unreachable),
            ],
            "all problems, by node order then kind"
        );
        assert!(has_errors(&problems));
    }

    #[test]
    fn has_errors_detects_any_error_severity() {
        assert!(!has_errors(&[]));
        assert!(!has_errors(&[problem(
            Severity::Warning,
            ProblemKind::Unreachable
        )]));
        assert!(has_errors(&[problem(
            Severity::Error,
            ProblemKind::StartMissing
        )]));
    }

    /// Test-only accessor: appends a sentinel to the cached problem list without recomputing.
    fn poison_cache(doc: &mut DialogueDoc) {
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
    fn problems_cache_computes_on_first_call_and_recomputes_only_on_content_revision_change() {
        // A fresh doc (content_revision 0) still computes on the first call.
        let mut fresh = DialogueDoc::new_empty();
        assert_eq!(fresh.revision, 0);
        assert_eq!(fresh.content_revision, 0);
        assert_eq!(fresh.lint_cache, None);
        let problems = fresh.problems();
        assert!(
            has_error_of_kind(problems, ProblemKind::StartMissing),
            "a fresh doc computes its lint on the first call: {problems:?}"
        );
        assert_eq!(
            cached_content_revision(&fresh),
            Some(0),
            "the cache is stored at content_revision 0"
        );

        let (mut doc, a, _b) = base_doc();
        let first = doc.problems().to_vec();
        assert_eq!(cached_content_revision(&doc), Some(doc.content_revision));

        // Unchanged content_revision: the cached vector survives (even a poisoned one).
        poison_cache(&mut doc);
        // A view-only change bumps `revision` but NOT `content_revision`: still no recompute.
        let revision = doc.revision;
        let content_revision = doc.content_revision;
        doc.set_node_pos(a, Vec2::new(5.0, 5.0));
        assert_eq!(doc.revision, revision + 1, "set_node_pos bumps revision");
        assert_eq!(
            doc.content_revision, content_revision,
            "set_node_pos must not bump content_revision"
        );
        let again = doc.problems();
        assert_eq!(
            again.len(),
            first.len() + 1,
            "the cache is returned unchanged when content_revision did not change (view-only included)"
        );
        assert!(again.iter().any(|p| p.message == "poisoned"));

        // A content mutator bumps content_revision: the cache is recomputed.
        doc.set_text(a, "changed".to_owned());
        let recomputed = doc.problems();
        assert_eq!(
            recomputed,
            &first[..],
            "after a content_revision bump the cache is replaced (text does not affect lint)"
        );
        assert!(!recomputed.iter().any(|p| p.message == "poisoned"));
        assert_eq!(
            cached_content_revision(&doc),
            Some(doc.content_revision),
            "the cache is keyed on the new content_revision"
        );
    }
}

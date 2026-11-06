//! Conversion between the engine's `DialogueDef` and the editor's [`DialogueDoc`].
//!
//! [`from_def`] is tolerant and never fails: anything the engine's validator would reject is loaded
//! into the model with a `load_warnings` entry (and dangling references kept as [`Link::Dangling`]).
//! [`to_def`] is total and exact: it never sets both `next` and `choices` on a node and never emits
//! `choices: Some(vec![])`.

use std::collections::HashMap;

use aberredengine::prelude::{ChoiceDef, DialogueDef, NodeDef, Vec2};

use crate::model::doc::{DialogueDoc, EdChoice, EdNode, Link, NodeKey, Outputs};

/// Builds a doc from an engine def. Keys are assigned sequentially (1-based) in node order; duplicate
/// ids resolve to the FIRST node with that id. All positions are `Vec2::ZERO`; `dirty = false`,
/// `revision = 0`, `content_revision = 0` (built without mutators, so the first `problems()` call
/// computes the lint).
pub fn from_def(def: &DialogueDef) -> DialogueDoc {
    let mut doc = DialogueDoc::new_empty();

    // First pass: assign keys and map id -> key (first node with a duplicate id wins).
    let mut id_to_key: HashMap<&str, NodeKey> = HashMap::new();
    for (i, node_def) in def.nodes.iter().enumerate() {
        let key = NodeKey(i as u64 + 1);
        id_to_key.entry(node_def.id.as_str()).or_insert(key);
        doc.nodes.push(EdNode {
            key,
            id: node_def.id.clone(),
            speaker: node_def.speaker.clone(),
            text: node_def.text.clone(),
            labels: node_def.labels.clone(),
            on_enter: node_def.on_enter.clone(),
            out: Outputs::Next(Link::End), // replaced in the second pass
            pos: Vec2::ZERO,
        });
    }
    doc.next_key = def.nodes.len() as u64;

    doc.start = match id_to_key.get(def.start.as_str()) {
        Some(&key) => Some(key),
        None => {
            let warning = format!(
                "start \"{}\" does not match any node id; start cleared",
                def.start
            );
            log::warn!("from_def load warning: {warning}");
            doc.load_warnings.push(warning);
            None
        }
    };

    // Second pass: resolve links now that every id is mapped (forward references included).
    for (i, node_def) in def.nodes.iter().enumerate() {
        let out = match &node_def.choices {
            Some(choices) if !choices.is_empty() => {
                if node_def.next.is_some() {
                    let warning = format!(
                        "node \"{}\" sets both next and choices; kept the choices and dropped next",
                        node_def.id
                    );
                    log::warn!("from_def load warning: {warning}");
                    doc.load_warnings.push(warning);
                }
                Outputs::Choices(
                    choices
                        .iter()
                        .map(|c| EdChoice {
                            text: c.text.clone(),
                            target: resolve_link(&c.next, &id_to_key),
                            when: c.when.clone(),
                            effects: c.effects.clone(),
                            labels: c.labels.clone(),
                        })
                        .collect(),
                )
            }
            Some(_) => {
                let warning = format!(
                    "node \"{}\" has an explicit empty choices list; treated as terminal",
                    node_def.id
                );
                log::warn!("from_def load warning: {warning}");
                doc.load_warnings.push(warning);
                Outputs::Next(Link::End)
            }
            None => Outputs::Next(resolve_link(&node_def.next, &id_to_key)),
        };
        doc.nodes[i].out = out;
    }
    log::debug!(
        "from_def: {} nodes, start {}",
        doc.nodes.len(),
        if doc.start.is_some() {
            "resolved"
        } else {
            "not resolved"
        }
    );
    doc
}

/// Builds an engine def from a doc. `start: None` becomes `start: ""`; a `Link::Node` whose key no
/// longer exists is treated as `End`; `Link::Dangling(s)` is written back as `Some(s)`.
pub fn to_def(doc: &DialogueDoc) -> DialogueDef {
    let nodes = doc
        .nodes
        .iter()
        .map(|node| {
            let (next, choices) = match &node.out {
                Outputs::Next(link) => (link_to_next(doc, link), None),
                Outputs::Choices(v) if !v.is_empty() => (
                    None,
                    Some(
                        v.iter()
                            .map(|c| ChoiceDef {
                                text: c.text.clone(),
                                next: link_to_next(doc, &c.target),
                                when: c.when.clone(),
                                effects: c.effects.clone(),
                                labels: c.labels.clone(),
                            })
                            .collect::<Vec<_>>(),
                    ),
                ),
                // An empty Choices vec never occurs through the mutators; if one slips through,
                // emit Next(End) semantics: no `next`, no `choices`.
                Outputs::Choices(_) => (None, None),
            };
            NodeDef {
                id: node.id.clone(),
                speaker: node.speaker.clone(),
                text: node.text.clone(),
                labels: node.labels.clone(),
                on_enter: node.on_enter.clone(),
                next,
                choices,
            }
        })
        .collect();
    let start = doc
        .start
        .and_then(|key| node_id(doc, key))
        .unwrap_or_default();
    DialogueDef { start, nodes }
}

/// `None` -> `End`; a resolvable id -> `Node`; anything else stays `Dangling`.
fn resolve_link(next: &Option<String>, id_to_key: &HashMap<&str, NodeKey>) -> Link {
    match next {
        None => Link::End,
        Some(id) => match id_to_key.get(id.as_str()) {
            Some(&key) => Link::Node(key),
            None => Link::Dangling(id.clone()),
        },
    }
}

/// The id of the node with `key`, if it still exists.
fn node_id(doc: &DialogueDoc, key: NodeKey) -> Option<String> {
    doc.nodes
        .iter()
        .find(|n| n.key == key)
        .map(|n| n.id.clone())
}

/// `End` -> `None`; `Node` -> that node's id (a missing key is treated as `End`); `Dangling` -> the
/// original string, so the engine's own validation can see the broken reference.
fn link_to_next(doc: &DialogueDoc, link: &Link) -> Option<String> {
    match link {
        Link::End => None,
        Link::Node(key) => node_id(doc, *key),
        Link::Dangling(id) => Some(id.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::doc::{DialogueDoc, EdChoice, Link, NodeKey, Outputs, Slot};
    use aberredengine::prelude::{DialogueCondition, DialogueDef, Vec2};

    const GUARD: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/guard.json"
    ));
    const LOOPS: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/loops.json"
    ));
    const DANGLING: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/invalid_dangling.json"
    ));
    const DUP_ID: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/invalid_dup_id.json"
    ));
    const BOTH: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/invalid_both_next_choices.json"
    ));
    const EMPTY_CHOICES: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/invalid_empty_choices.json"
    ));
    const NO_START: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/invalid_no_start.json"
    ));

    fn parse(json: &str) -> DialogueDef {
        serde_json::from_str(json).expect("fixture must parse as DialogueDef")
    }

    #[test]
    fn round_trip_guard() {
        let def = parse(GUARD);
        let doc = from_def(&def);
        assert!(
            doc.load_warnings.is_empty(),
            "guard.json is valid, got warnings: {:?}",
            doc.load_warnings
        );
        assert!(!doc.dirty);
        assert_eq!(doc.revision, 0);
        assert_eq!(doc.content_revision, 0);
        assert_eq!(doc.nodes.len(), 3);
        assert!(doc.nodes.iter().all(|n| n.pos == Vec2::ZERO));
        assert_eq!(to_def(&doc), def, "to_def(from_def(def)) == def");
    }

    #[test]
    fn round_trip_loops() {
        let def = parse(LOOPS);
        let doc = from_def(&def);
        assert!(
            doc.load_warnings.is_empty(),
            "loops.json is valid, got warnings: {:?}",
            doc.load_warnings
        );
        assert_eq!(to_def(&doc), def, "to_def(from_def(def)) == def");
    }

    #[test]
    fn dangling_links_load_as_dangling_and_round_trip() {
        let def = parse(DANGLING);
        let doc = from_def(&def);
        let a = doc.node(NodeKey(1)).unwrap();
        assert!(
            matches!(&a.out, Outputs::Next(Link::Dangling(s)) if s == "ghost"),
            "a.next must stay dangling"
        );
        let b = doc.node(NodeKey(2)).unwrap();
        match &b.out {
            Outputs::Choices(v) => {
                assert!(matches!(&v[0].target, Link::Dangling(s) if s == "ghost2"));
                assert!(matches!(v[1].target, Link::End), "missing next is End");
            }
            Outputs::Next(_) => panic!("expected choices"),
        }
        assert_eq!(to_def(&doc), def, "dangling references round-trip verbatim");
    }

    #[test]
    fn duplicate_ids_resolve_to_the_first_node() {
        let def = parse(DUP_ID);
        let doc = from_def(&def);
        assert_eq!(doc.nodes.len(), 3);
        assert_eq!(
            doc.start,
            Some(NodeKey(1)),
            "start resolves to the first node with id \"x\""
        );
        let y = doc.node(NodeKey(2)).unwrap();
        assert!(
            matches!(&y.out, Outputs::Next(Link::Node(NodeKey(1)))),
            "next \"x\" resolves to the first node with that id"
        );
    }

    #[test]
    fn both_next_and_choices_keeps_choices_drops_next_and_warns() {
        let doc = from_def(&parse(BOTH));
        let a = doc.node(NodeKey(1)).unwrap();
        match &a.out {
            Outputs::Choices(v) => assert_eq!(v.len(), 1, "choices are kept"),
            Outputs::Next(_) => panic!("next must be dropped"),
        }
        assert_eq!(doc.load_warnings.len(), 1);
        assert!(
            doc.load_warnings[0].contains('a'),
            "warning names the node: {}",
            doc.load_warnings[0]
        );
    }

    #[test]
    fn explicit_empty_choices_becomes_next_end_and_warns() {
        let doc = from_def(&parse(EMPTY_CHOICES));
        let a = doc.node(NodeKey(1)).unwrap();
        assert!(matches!(&a.out, Outputs::Next(Link::End)));
        assert_eq!(doc.load_warnings.len(), 1);
        assert!(
            doc.load_warnings[0].contains('a'),
            "warning names the node: {}",
            doc.load_warnings[0]
        );
    }

    #[test]
    fn unresolvable_start_clears_start_and_warns() {
        let doc = from_def(&parse(NO_START));
        assert_eq!(doc.start, None);
        assert_eq!(doc.load_warnings.len(), 1);
        assert!(
            doc.load_warnings[0].contains("start"),
            "warning mentions the start: {}",
            doc.load_warnings[0]
        );
    }

    #[test]
    fn to_def_never_emits_next_and_choices_together_or_an_empty_choices_list() {
        let mut doc = DialogueDoc::new_empty();
        let a = doc.add_node(Vec2::ZERO);
        let b = doc.add_node(Vec2::new(64.0, 0.0));
        let ops: Vec<fn(&mut DialogueDoc, NodeKey, NodeKey)> = vec![
            |d, a, b| d.set_link(a, Slot::Next, Link::Node(b)),
            |d, a, _| d.set_link(a, Slot::Next, Link::Dangling("ghost".to_owned())),
            |d, a, _| d.set_link(a, Slot::Next, Link::End),
            |d, a, _| d.add_choice(a),
            |d, a, b| d.set_link(a, Slot::Choice(0), Link::Node(b)),
            |d, a, _| d.add_choice(a),
            |d, a, _| d.move_choice(a, 0, 1),
            |d, a, _| d.move_choice(a, 1, -5),
            |d, a, _| d.set_choice_text(a, 1, "x".to_owned()),
            |d, a, _| d.delete_choice(a, 1),
            |d, a, _| d.delete_choice(a, 0), // last choice -> Next(End)
            // an empty Choices vec slipping through set_outputs must serialize as choices: None
            |d, a, _| d.set_outputs(a, Outputs::Choices(vec![])),
            |d, a, b| {
                let choice = EdChoice {
                    text: "c".to_owned(),
                    target: Link::Node(a),
                    when: Some(DialogueCondition::flag("f")),
                    effects: vec![],
                    labels: vec!["l".to_owned()],
                };
                d.set_outputs(b, Outputs::Choices(vec![choice]))
            },
            |d, a, _| d.delete_node(a), // b's choice now points at a key that no longer exists
        ];
        for op in ops {
            op(&mut doc, a, b);
            assert_def_invariants(&doc);
        }
    }

    fn assert_def_invariants(doc: &DialogueDoc) {
        for node in &to_def(doc).nodes {
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

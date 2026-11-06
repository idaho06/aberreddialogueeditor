//! The Inspector's outputs editor: the "Next" / "Choices" mode radios, the next-target combo
//! (Next mode) and the per-choice collapsing sections (Choices mode). Drawn against a CLONED
//! [`EdNode`]; every widget that reports a change pushes the matching [`Edit`].
//!
//! Widget ids come from the node key (the caller wraps the panel body in a `push_id` keyed by the
//! NodeKey), from choice/entry indices and from fixed strings — never from the edited text. In
//! particular a choice's collapsing header is labelled `"Choice N: <text>###choice{i}"`: the
//! visible part may change with the text, but the imgui id is ONLY the `###choice{i}` part, so
//! typing in the choice text never changes the widget id (the section stays open and the input
//! keeps focus).

use std::collections::HashMap;

use aberredengine::prelude::imgui::{StyleColor, TreeNodeFlags, Ui};

use super::node_fields::labels_editor;
use super::{Edit, OutputsMode, effects, expr};
use crate::model::{EdNode, Link, NodeKey, Outputs, Slot};

/// Red note colour for a dangling target's combo preview.
const RED: [f32; 4] = [1.0, 0.35, 0.35, 1.0];

/// How many chars of the choice text the collapsing header shows before truncating.
const HEADER_TEXT_MAX: usize = 24;

/// Draws the outputs widgets for `node` (`targets` = the doc's `(key, id)` list in node order,
/// feeding the target combos) and returns the requested edits.
pub fn draw(ui: &Ui, node: &EdNode, targets: &[(NodeKey, String)]) -> Vec<Edit> {
    let key = node.key;
    let mut edits = Vec::new();
    let entries = combo_entries(targets);

    // Mode radios: clicking the INACTIVE one requests the switch (a same-mode ChangeMode is a
    // no-op in apply_edits anyway, but the UI does not even emit it).
    let is_choices = matches!(node.out, Outputs::Choices(_));
    if ui.radio_button_bool("Next", !is_choices) && is_choices {
        edits.push(Edit::ChangeMode(key, OutputsMode::Next));
    }
    ui.same_line();
    if ui.radio_button_bool("Choices", is_choices) && !is_choices {
        edits.push(Edit::ChangeMode(key, OutputsMode::Choices));
    }

    match &node.out {
        Outputs::Next(link) => {
            if let Some(new) = target_combo(ui, "Next", link, &entries) {
                edits.push(Edit::SetLink(key, Slot::Next, new));
            }
        }
        Outputs::Choices(choices) => {
            let last = choices.len().saturating_sub(1);
            for (i, choice) in choices.iter().enumerate() {
                // Row id from the index only; the header's `###choice{i}` keeps its own id
                // stable while the visible text changes.
                let _item = ui.push_id_usize(i);
                let header = format!(
                    "Choice {}: {}###choice{}",
                    i + 1,
                    truncate_chars(&choice.text, HEADER_TEXT_MAX),
                    i
                );
                if ui.collapsing_header(header, TreeNodeFlags::DEFAULT_OPEN) {
                    ui.indent();
                    let mut text = choice.text.clone();
                    if ui.input_text("Text", &mut text).build() {
                        edits.push(Edit::SetChoiceText(key, i, text));
                    }
                    if let Some(new) = target_combo(ui, "Target", &choice.target, &entries) {
                        edits.push(Edit::SetLink(key, Slot::Choice(i), new));
                    }
                    if let Some(labels) = labels_editor(ui, "labels", &choice.labels) {
                        edits.push(Edit::SetChoiceLabels(key, i, labels));
                    }
                    // Up / Down / Delete on one row; Up disabled on the first, Down on the last.
                    if i == 0 {
                        let _disabled = ui.begin_disabled(true);
                        ui.small_button("Up");
                    } else if ui.small_button("Up") {
                        edits.push(Edit::MoveChoice(key, i, -1));
                    }
                    ui.same_line();
                    if i == last {
                        let _disabled = ui.begin_disabled(true);
                        ui.small_button("Down");
                    } else if ui.small_button("Down") {
                        edits.push(Edit::MoveChoice(key, i, 1));
                    }
                    ui.same_line();
                    if ui.small_button("Delete") {
                        edits.push(Edit::DeleteChoice(key, i));
                    }
                    // When: the optional-condition editor (a "When" checkbox + nested editor)
                    // on a LOCAL clone; a change emits the whole new Option<DialogueCondition>.
                    let mut when = choice.when.clone();
                    if expr::optional_condition_editor(ui, &mut when) {
                        edits.push(Edit::SetChoiceWhen(key, i, when));
                    }
                    // Effects: a collapsing sub-section. The imgui id is ONLY the fixed
                    // "###effects" part — the visible count may change, the id must not.
                    if ui.collapsing_header(
                        format!("Effects ({})###effects", choice.effects.len()),
                        TreeNodeFlags::DEFAULT_OPEN,
                    ) {
                        ui.indent();
                        let mut new_effects = choice.effects.clone();
                        if effects::effects_editor(ui, &mut new_effects) {
                            edits.push(Edit::SetChoiceEffects(key, i, new_effects));
                        }
                        ui.unindent();
                    }
                    ui.unindent();
                }
            }
            if ui.button("[+ Add choice]") {
                edits.push(Edit::AddChoice(key));
            }
        }
    }

    edits
}

/// Char-safe truncation: returns `s` unchanged when it has at most `max` chars, otherwise the
/// first `max` chars plus `"..."`. Never panics on non-ASCII (counts `char`s, not bytes).
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push_str("...");
    out
}

/// The target-combo entries: first `(end)` ([`Link::End`]), then every node in node order with
/// label = its id. When several nodes share an id (or the id is empty) the label becomes
/// `"{id} (#{n})"` with `n` = the node's 1-based position in node order, so entries stay
/// distinguishable; an empty id shows as `"(empty id) (#{n})"`.
pub fn combo_entries(nodes: &[(NodeKey, String)]) -> Vec<(Link, String)> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for (_, id) in nodes {
        *counts.entry(id.as_str()).or_insert(0) += 1;
    }
    let mut entries = Vec::with_capacity(nodes.len() + 1);
    entries.push((Link::End, "(end)".to_owned()));
    for (pos, (key, id)) in nodes.iter().enumerate() {
        let n = pos + 1;
        let label = if id.is_empty() {
            format!("(empty id) (#{n})")
        } else if counts[id.as_str()] > 1 {
            format!("{id} (#{n})")
        } else {
            id.clone()
        };
        entries.push((Link::Node(*key), label));
    }
    entries
}

/// The reusable target combo. `id` is the visible label (the imgui id is the fixed `"target"`,
/// scoped by the caller's `push_id`); the preview is the current entry's label, or — for a
/// `Link::Dangling(s)` — `"unknown: {s}"` drawn in RED (the dropdown still lists the normal
/// entries). Returns `Some(new_link)` when an entry different from the current one is selected,
/// `None` otherwise (including re-selecting the current entry).
pub fn target_combo(ui: &Ui, id: &str, current: &Link, entries: &[(Link, String)]) -> Option<Link> {
    let preview = match current {
        Link::Dangling(s) => format!("unknown: {s}"),
        _ => entries
            .iter()
            .find(|(link, _)| link == current)
            .map(|(_, label)| label.clone())
            .unwrap_or_else(|| "(end)".to_owned()),
    };
    let label = format!("{id}##target");
    // Push the red Text colour only around begin_combo: the preview is drawn inside that call,
    // so the dropdown entries below keep the normal colour.
    let combo = {
        let _red = matches!(current, Link::Dangling(_))
            .then(|| ui.push_style_color(StyleColor::Text, RED));
        ui.begin_combo(label, &preview)
    };
    let mut chosen = None;
    if let Some(combo) = combo {
        for (index, (link, entry_label)) in entries.iter().enumerate() {
            let _row = ui.push_id_usize(index);
            if ui
                .selectable_config(entry_label)
                .selected(link == current)
                .build()
                && *link != *current
            {
                chosen = Some(link.clone());
            }
        }
        combo.end();
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_chars_ascii_cut_and_no_cut() {
        assert_eq!(truncate_chars("hello world", 5), "hello...");
        assert_eq!(
            truncate_chars("hello", 5),
            "hello",
            "exact boundary: unchanged"
        );
        assert_eq!(
            truncate_chars("hello", 6),
            "hello",
            "shorter than max: unchanged"
        );
        assert_eq!(truncate_chars("hi", 5), "hi");
        assert_eq!(truncate_chars("", 3), "");
        assert_eq!(truncate_chars("abc", 0), "...");
    }

    #[test]
    fn truncate_chars_is_char_safe_on_non_ascii() {
        // Multi-byte chars are counted as chars, never split (no panic, no replacement char).
        assert_eq!(truncate_chars("héllo wörld", 4), "héll...");
        assert_eq!(truncate_chars("日本語です", 3), "日本語...");
        assert_eq!(
            truncate_chars("日本語です", 5),
            "日本語です",
            "exact boundary on non-ASCII"
        );
        assert_eq!(truncate_chars("aé日", 2), "aé...");
        assert_eq!(truncate_chars("🙂🙃🙂", 2), "🙂🙃...");
    }

    #[test]
    fn combo_entries_puts_end_first_then_nodes_in_order() {
        let nodes = vec![
            (NodeKey(1), "greet".to_owned()),
            (NodeKey(2), "friend".to_owned()),
        ];
        let entries = combo_entries(&nodes);
        assert_eq!(
            entries,
            vec![
                (Link::End, "(end)".to_owned()),
                (Link::Node(NodeKey(1)), "greet".to_owned()),
                (Link::Node(NodeKey(2)), "friend".to_owned()),
            ]
        );
        assert_eq!(combo_entries(&[]), vec![(Link::End, "(end)".to_owned())]);
    }

    #[test]
    fn combo_entries_disambiguates_duplicate_ids_with_1_based_positions() {
        let nodes = vec![
            (NodeKey(1), "x".to_owned()),
            (NodeKey(2), "y".to_owned()),
            (NodeKey(3), "x".to_owned()),
        ];
        let entries = combo_entries(&nodes);
        assert_eq!(entries[0], (Link::End, "(end)".to_owned()));
        assert_eq!(entries[1], (Link::Node(NodeKey(1)), "x (#1)".to_owned()));
        assert_eq!(
            entries[2],
            (Link::Node(NodeKey(2)), "y".to_owned()),
            "a unique id stays plain"
        );
        assert_eq!(entries[3], (Link::Node(NodeKey(3)), "x (#3)".to_owned()));
    }

    #[test]
    fn combo_entries_shows_an_empty_id_as_empty_id_with_position() {
        let nodes = vec![
            (NodeKey(1), String::new()),
            (NodeKey(2), "b".to_owned()),
            (NodeKey(3), String::new()),
        ];
        let entries = combo_entries(&nodes);
        assert_eq!(
            entries[1],
            (Link::Node(NodeKey(1)), "(empty id) (#1)".to_owned())
        );
        assert_eq!(entries[2], (Link::Node(NodeKey(2)), "b".to_owned()));
        assert_eq!(
            entries[3],
            (Link::Node(NodeKey(3)), "(empty id) (#3)".to_owned())
        );
    }
}

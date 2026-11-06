//! Pure description of one node's visuals: `describe_node` turns an [`EdNode`] into the texts,
//! pins and colours the projection system draws. No ECS, no imgui — engine imports are limited
//! to `Vec2` and `Color`. All positions are LOCAL to the node's top-left (`node.pos`).

use aberredengine::prelude::{Color, Vec2};

use crate::graph::geometry;
use crate::graph::style::{
    DIM_TEXT_COLOR, ERROR_OUTLINE_COLOR, HEADER_COLOR, HEADER_H, NODE_BODY_COLOR, NODE_W,
    OUTLINE_COLOR, PAD, PIN_DANGLING_COLOR, PIN_IN_COLOR, PIN_OUT_COLOR, PIN_R, ROW_H,
    SELECTED_OUTLINE_COLOR, START_HEADER_COLOR, TEXT_AREA_H, TEXT_COLOR, TEXT_MAX_LINES,
};
use crate::graph::wrap::{fit_line, wrap_text};
use crate::model::{EdNode, Link, Outputs};

/// One text drawn inside a node, at `local` (relative to the node's top-left).
#[derive(Debug, Clone, PartialEq)]
pub struct TextItem {
    pub text: String,
    pub local: Vec2,
    pub color: Color,
}

/// One pin drawn at the edge of a node, at `local` (relative to the node's top-left).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PinItem {
    pub local: Vec2,
    pub color: Color,
    pub is_output: bool,
}

/// Everything needed to draw one node (as children of its root entity).
#[derive(Debug, Clone, PartialEq)]
pub struct NodeVisual {
    pub size: Vec2,
    pub body_color: Color,
    pub outline_color: Color,
    /// Thick outline when the node is selected or has a lint error.
    pub outline_thick: bool,
    pub header_color: Color,
    pub texts: Vec<TextItem>,
    pub pins: Vec<PinItem>,
}

/// Output link of row `i`: the `next` link of a `Next` node, else the choice's target.
pub fn row_link(node: &EdNode, i: usize) -> &Link {
    match &node.out {
        Outputs::Choices(choices) => &choices[i].target,
        Outputs::Next(link) => link,
    }
}

/// Rendered label of output row `i` (before `fit_line`): the choice text with a `" ?"` suffix
/// when it has a `when` condition and a `" !"` suffix when it has effects, or `"next"` /
/// `"(end)"` for a `Next` node.
pub fn row_text(node: &EdNode, i: usize) -> String {
    match &node.out {
        Outputs::Choices(choices) => {
            let choice = &choices[i];
            let mut label = choice.text.clone();
            if choice.when.is_some() {
                label.push_str(" ?");
            }
            if !choice.effects.is_empty() {
                label.push_str(" !");
            }
            label
        }
        Outputs::Next(link) => match link {
            Link::End => "(end)".to_owned(),
            Link::Node(_) | Link::Dangling(_) => "next".to_owned(),
        },
    }
}

/// Full visual description of `node`. `measure` is the text-width callback (font metrics on the
/// ECS side; a deterministic stub in tests).
pub fn describe_node(
    node: &EdNode,
    is_start: bool,
    is_selected: bool,
    has_error: bool,
    measure: &dyn Fn(&str) -> f32,
) -> NodeVisual {
    let size = Vec2::new(NODE_W, geometry::node_height(node));
    let outline_color = if is_selected {
        SELECTED_OUTLINE_COLOR
    } else if has_error {
        ERROR_OUTLINE_COLOR
    } else {
        OUTLINE_COLOR
    };
    let outline_thick = is_selected || has_error;
    let header_color = if is_start {
        START_HEADER_COLOR
    } else {
        HEADER_COLOR
    };

    let mut texts = Vec::new();
    texts.push(TextItem {
        text: fit_line(&node.id, NODE_W - 2.0 * PAD, measure),
        local: Vec2::new(PAD, 4.0),
        color: TEXT_COLOR,
    });
    let body = match &node.speaker {
        Some(speaker) => format!("{speaker}: {}", node.text),
        None => node.text.clone(),
    };
    texts.push(TextItem {
        text: wrap_text(&body, NODE_W - 2.0 * PAD, TEXT_MAX_LINES, measure).join("\n"),
        local: Vec2::new(PAD, HEADER_H + 4.0),
        color: TEXT_COLOR,
    });
    let row_width = NODE_W - 2.0 * PAD - 2.0 * PIN_R - 8.0;
    let row_count = geometry::rows(node);
    for i in 0..row_count {
        // Choice rows are normal text; the `next` row of a `Next` node is dim.
        let color = match &node.out {
            Outputs::Choices(_) => TEXT_COLOR,
            Outputs::Next(_) => DIM_TEXT_COLOR,
        };
        texts.push(TextItem {
            text: fit_line(&row_text(node, i), row_width, measure),
            local: Vec2::new(PAD, HEADER_H + TEXT_AREA_H + i as f32 * ROW_H + 4.0),
            color,
        });
    }

    let mut pins = Vec::new();
    pins.push(PinItem {
        local: geometry::input_pin(node) - node.pos,
        color: PIN_IN_COLOR,
        is_output: false,
    });
    for i in 0..row_count {
        let color = if matches!(row_link(node, i), Link::Dangling(_)) {
            PIN_DANGLING_COLOR
        } else {
            PIN_OUT_COLOR
        };
        pins.push(PinItem {
            local: geometry::output_pin(node, i) - node.pos,
            color,
            is_output: true,
        });
    }

    NodeVisual {
        size,
        body_color: NODE_BODY_COLOR,
        outline_color,
        outline_thick,
        header_color,
        texts,
        pins,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::fixtures::{DANGLING, GUARD, doc_from};
    use crate::model::NodeKey;

    /// Deterministic measure: 10 px per char.
    fn measure(s: &str) -> f32 {
        s.chars().count() as f32 * 10.0
    }

    fn close(a: Vec2, b: Vec2) -> bool {
        a.abs_diff_eq(b, 1e-5)
    }

    fn node(doc: &crate::model::DialogueDoc, key: NodeKey, pos: Vec2) -> EdNode {
        let mut node = doc.node(key).expect("node exists").clone();
        node.pos = pos;
        node
    }

    #[test]
    fn next_node_description() {
        let doc = doc_from(GUARD);
        let friend = node(&doc, NodeKey(2), Vec2::new(100.0, 50.0)); // Next(End)
        let v = describe_node(&friend, false, false, false, &measure);

        assert!(close(
            v.size,
            Vec2::new(NODE_W, HEADER_H + TEXT_AREA_H + ROW_H)
        ));
        assert_eq!(v.body_color, NODE_BODY_COLOR);
        assert_eq!(v.outline_color, OUTLINE_COLOR);
        assert!(!v.outline_thick);
        assert_eq!(v.header_color, HEADER_COLOR);

        assert_eq!(v.texts.len(), 3, "header + text area + one row");
        assert_eq!(v.texts[0].text, "friend");
        assert!(close(v.texts[0].local, Vec2::new(PAD, 4.0)));
        assert_eq!(v.texts[0].color, TEXT_COLOR);
        assert_eq!(v.texts[1].text, "Guard: Hm. Move along.");
        assert!(close(v.texts[1].local, Vec2::new(PAD, HEADER_H + 4.0)));
        assert_eq!(v.texts[2].text, "(end)", "Next(End) row is dim (end)");
        assert!(close(
            v.texts[2].local,
            Vec2::new(PAD, HEADER_H + TEXT_AREA_H + 4.0)
        ));
        assert_eq!(v.texts[2].color, DIM_TEXT_COLOR);

        assert_eq!(v.pins.len(), 2, "one input pin + one output pin");
        assert!(close(
            v.pins[0].local,
            geometry::input_pin(&friend) - friend.pos
        ));
        assert_eq!(v.pins[0].color, PIN_IN_COLOR);
        assert!(!v.pins[0].is_output);
        assert!(close(
            v.pins[1].local,
            geometry::output_pin(&friend, 0) - friend.pos
        ));
        assert_eq!(v.pins[1].color, PIN_OUT_COLOR, "Link::End is not dangling");
        assert!(v.pins[1].is_output);
    }

    #[test]
    fn three_choice_node_description_with_suffixes_and_pins() {
        let doc = doc_from(GUARD);
        let greet = node(&doc, NodeKey(1), Vec2::new(10.0, 20.0)); // 3 choices
        let v = describe_node(&greet, true, false, false, &measure);

        assert_eq!(v.header_color, START_HEADER_COLOR, "is_start header");
        assert!(close(
            v.size,
            Vec2::new(NODE_W, HEADER_H + TEXT_AREA_H + 3.0 * ROW_H)
        ));

        assert_eq!(v.texts.len(), 2 + 3);
        assert_eq!(v.texts[0].text, "greet");
        assert_eq!(v.texts[1].text, "Guard: Halt! Who goes\nthere?");
        let row_y = |i: usize| HEADER_H + TEXT_AREA_H + i as f32 * ROW_H + 4.0;
        assert_eq!(v.texts[2].text, "A friend.");
        assert_eq!(
            v.texts[3].text, "Here is my pass. ? !",
            "when -> ? and effects -> !"
        );
        assert_eq!(v.texts[4].text, "Never mind.");
        for (i, t) in v.texts[2..].iter().enumerate() {
            assert!(close(t.local, Vec2::new(PAD, row_y(i))));
            assert_eq!(t.color, TEXT_COLOR);
        }

        assert_eq!(v.pins.len(), 1 + 3);
        assert!(close(
            v.pins[0].local,
            geometry::input_pin(&greet) - greet.pos
        ));
        for i in 0..3 {
            let pin = v.pins[1 + i];
            assert!(close(
                pin.local,
                geometry::output_pin(&greet, i) - greet.pos
            ));
            assert!(pin.is_output);
            assert_eq!(
                pin.color, PIN_OUT_COLOR,
                "targets are Node/End, not dangling"
            );
        }
    }

    #[test]
    fn dangling_target_pin_is_red_and_next_row_says_next() {
        let doc = doc_from(DANGLING);
        let a = node(&doc, NodeKey(1), Vec2::ZERO); // next -> "ghost" (dangling)
        let v = describe_node(&a, false, false, false, &measure);
        assert_eq!(v.texts.len(), 3);
        assert_eq!(v.texts[2].text, "next", "Dangling renders like a live next");
        assert_eq!(v.texts[2].color, DIM_TEXT_COLOR);
        assert_eq!(v.pins.len(), 2);
        assert_eq!(v.pins[1].color, PIN_DANGLING_COLOR);

        let b = node(&doc, NodeKey(2), Vec2::ZERO); // choice 0 -> dangling, choice 1 -> End
        let v = describe_node(&b, false, false, false, &measure);
        assert_eq!(v.pins[1].color, PIN_DANGLING_COLOR, "choice 0 dangles");
        assert_eq!(v.pins[2].color, PIN_OUT_COLOR, "choice 1 ends");
        assert_eq!(v.texts[2].text, "Into the void.");
        assert_eq!(v.texts[3].text, "End.");
    }

    #[test]
    fn outline_priority_selected_over_error_over_normal() {
        let doc = doc_from(GUARD);
        let friend = node(&doc, NodeKey(2), Vec2::ZERO);

        let normal = describe_node(&friend, false, false, false, &measure);
        assert_eq!(normal.outline_color, OUTLINE_COLOR);
        assert!(!normal.outline_thick);

        let error = describe_node(&friend, false, false, true, &measure);
        assert_eq!(error.outline_color, ERROR_OUTLINE_COLOR);
        assert!(error.outline_thick);

        let selected = describe_node(&friend, false, true, false, &measure);
        assert_eq!(selected.outline_color, SELECTED_OUTLINE_COLOR);
        assert!(selected.outline_thick);

        let both = describe_node(&friend, false, true, true, &measure);
        assert_eq!(
            both.outline_color, SELECTED_OUTLINE_COLOR,
            "selected beats error"
        );
        assert!(both.outline_thick);
    }

    #[test]
    fn long_texts_are_wrapped_and_truncated_by_measure() {
        let doc = doc_from(GUARD);
        let friend = node(&doc, NodeKey(2), Vec2::ZERO);

        let mut wide = friend.clone();
        wide.text = "x".repeat(100);
        wide.speaker = None;
        let v = describe_node(&wide, false, false, false, &measure);
        // NODE_W - 2*PAD = 224 -> 22 chars per line, TEXT_MAX_LINES = 4: 100 chars need 5
        // lines, so the 4th is truncated with "...".
        let lines: Vec<&str> = v.texts[1].text.split('\n').collect();
        assert!(lines.len() > 1 && lines.len() <= TEXT_MAX_LINES);
        assert!(v.texts[1].text.ends_with("..."));
        for line in lines {
            assert!(measure(line) <= NODE_W - 2.0 * PAD);
        }

        let mut long_id = friend.clone();
        long_id.id = "an_extremely_long_node_identifier_that_cannot_fit".to_owned();
        let v = describe_node(&long_id, false, false, false, &measure);
        assert!(v.texts[0].text.ends_with("..."));
        assert!(measure(&v.texts[0].text) <= NODE_W - 2.0 * PAD);
    }
}

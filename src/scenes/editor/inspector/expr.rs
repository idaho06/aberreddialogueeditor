//! The Inspector's recursive condition editor (`when` of a choice), plus the pure
//! [`convert_condition`] used when the user switches a condition's type.
//!
//! The editors work on the caller's LOCAL clone and return `true` when anything changed; the
//! caller turns a change into a whole-value [`Edit`](super::Edit). Widget ids never depend on
//! edited text: every recursion level and every list item is wrapped in `ui.push_id_usize(..)`
//! (plus fixed `"##..."` label suffixes), so typing never changes an id and the focused input
//! never loses focus. An empty key shows a faint red "(empty key)" hint — a hint, not an error:
//! the engine accepts empty keys.

use aberredengine::prelude::imgui::Ui;
use aberredengine::prelude::{DialogueCmp, DialogueCondition};

/// Faint red for the "(empty key)" hint (dimmer than the lint-error red).
const FAINT_RED: [f32; 4] = [1.0, 0.45, 0.45, 0.55];

/// The comparison operators of an integer condition, in combo order, with their display symbol.
const CMP_SYMBOLS: [(DialogueCmp, &str); 6] = [
    (DialogueCmp::Eq, "=="),
    (DialogueCmp::Ne, "!="),
    (DialogueCmp::Lt, "<"),
    (DialogueCmp::Le, "<="),
    (DialogueCmp::Gt, ">"),
    (DialogueCmp::Ge, ">="),
];

/// The six condition kinds the type combo can switch between.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CondKind {
    Flag,
    Integer,
    StringEquals,
    Not,
    All,
    Any,
}

impl CondKind {
    /// All kinds, in combo order.
    pub const KINDS: [CondKind; 6] = [
        CondKind::Flag,
        CondKind::Integer,
        CondKind::StringEquals,
        CondKind::Not,
        CondKind::All,
        CondKind::Any,
    ];

    /// The kind of an existing condition.
    pub fn of(cond: &DialogueCondition) -> Self {
        match cond {
            DialogueCondition::Flag { .. } => Self::Flag,
            DialogueCondition::Integer { .. } => Self::Integer,
            DialogueCondition::StringEquals { .. } => Self::StringEquals,
            DialogueCondition::Not { .. } => Self::Not,
            DialogueCondition::All { .. } => Self::All,
            DialogueCondition::Any { .. } => Self::Any,
        }
    }

    /// The display name shown in the type combo.
    pub fn name(self) -> &'static str {
        match self {
            Self::Flag => "flag",
            Self::Integer => "integer",
            Self::StringEquals => "string equals",
            Self::Not => "not",
            Self::All => "all",
            Self::Any => "any",
        }
    }
}

/// The key carried over when converting INTO a leaf kind (Flag/Integer/StringEquals): a leaf
/// keeps its key; a compound kind (Not/All/Any) drops its structure on purpose and starts from
/// an empty key.
fn leaf_key(old: &DialogueCondition) -> String {
    match old {
        DialogueCondition::Flag { key }
        | DialogueCondition::Integer { key, .. }
        | DialogueCondition::StringEquals { key, .. } => key.clone(),
        DialogueCondition::Not { .. }
        | DialogueCondition::All { .. }
        | DialogueCondition::Any { .. } => String::new(),
    }
}

/// The child list carried over when converting INTO All/Any: All<->Any keep the children; a
/// `Not{cond}` contributes `[*cond]`; a leaf contributes `[old]` — no work is lost.
fn children_of(old: &DialogueCondition) -> Vec<DialogueCondition> {
    match old {
        DialogueCondition::All { all } => all.clone(),
        DialogueCondition::Any { any } => any.clone(),
        DialogueCondition::Not { cond } => vec![(**cond).clone()],
        leaf => vec![leaf.clone()],
    }
}

/// PURE type conversion of a condition. Converting to the SAME kind returns `old` unchanged.
/// Leaf targets keep the key of a leaf `old` (Integer defaults `op: Eq, value: 0`;
/// StringEquals defaults `value: ""`) and use an empty key from a compound `old` (its structure
/// is dropped on purpose when the user picks a leaf type). Target `Not` wraps the WHOLE `old`
/// (`Not{cond: Box::new(old)}`) when `old` is not already a `Not`. Target `All`/`Any` use
/// [`children_of`].
pub fn convert_condition(old: &DialogueCondition, to: CondKind) -> DialogueCondition {
    if CondKind::of(old) == to {
        return old.clone();
    }
    match to {
        CondKind::Flag => DialogueCondition::Flag { key: leaf_key(old) },
        CondKind::Integer => DialogueCondition::Integer {
            key: leaf_key(old),
            op: DialogueCmp::Eq,
            value: 0,
        },
        CondKind::StringEquals => DialogueCondition::StringEquals {
            key: leaf_key(old),
            value: String::new(),
        },
        CondKind::Not => DialogueCondition::Not {
            cond: Box::new(old.clone()),
        },
        CondKind::All => DialogueCondition::All {
            all: children_of(old),
        },
        CondKind::Any => DialogueCondition::Any {
            any: children_of(old),
        },
    }
}

/// The key input shared by the leaf kinds, plus the faint-red "(empty key)" hint when empty.
fn key_editor(ui: &Ui, key: &mut String) -> bool {
    let changed = ui.input_text("Key", key).build();
    if key.is_empty() {
        ui.text_colored(FAINT_RED, "(empty key)");
    }
    changed
}

/// The operator combo of an integer condition, showing "==", "!=", "<", "<=", ">", ">=".
fn cmp_combo(ui: &Ui, op: &mut DialogueCmp) -> bool {
    let preview = CMP_SYMBOLS
        .iter()
        .find(|(cmp, _)| *cmp == *op)
        .map(|(_, symbol)| *symbol)
        .unwrap_or("==");
    let mut changed = false;
    if let Some(combo) = ui.begin_combo("Op##cond_op", preview) {
        for (i, (cmp, symbol)) in CMP_SYMBOLS.iter().enumerate() {
            let _entry = ui.push_id_usize(i);
            if ui.selectable_config(symbol).selected(*cmp == *op).build() && *cmp != *op {
                *op = *cmp;
                changed = true;
            }
        }
        combo.end();
    }
    changed
}

/// The `All`/`Any` child list: one nested editor per child (each in its own id scope and
/// indent), a "[-]" button after each, and a "[+ Add condition]" button at the end (adds an
/// empty `Flag`). Removing the last child is allowed — an empty `All`/`Any` is valid in the
/// engine.
fn child_list_editor(ui: &Ui, items: &mut Vec<DialogueCondition>) -> bool {
    let mut changed = false;
    let mut remove = None;
    for (i, item) in items.iter_mut().enumerate() {
        let _item = ui.push_id_usize(i);
        ui.indent();
        changed |= condition_editor(ui, item);
        ui.unindent();
        if ui.small_button("[-]") {
            remove = Some(i);
        }
    }
    if let Some(i) = remove {
        items.remove(i);
        changed = true;
    }
    if ui.button("[+ Add condition]") {
        items.push(DialogueCondition::Flag { key: String::new() });
        changed = true;
    }
    changed
}

/// The recursive condition editor. Returns `true` when anything changed; mutates the caller's
/// LOCAL clone. Renders a type combo (picking another kind calls [`convert_condition`]) and the
/// fields per kind: a key input for the leaves, an operator combo + `input_int` value for
/// Integer, a value input for StringEquals, ONE nested editor indented for `Not`, and a child
/// list for `All`/`Any`. Recursion depth is not limited.
pub fn condition_editor(ui: &Ui, cond: &mut DialogueCondition) -> bool {
    let mut changed = false;
    let current = CondKind::of(cond);

    // Type combo: picking another kind converts the whole condition.
    if let Some(combo) = ui.begin_combo("Type##cond_type", current.name()) {
        for (i, kind) in CondKind::KINDS.iter().enumerate() {
            let _entry = ui.push_id_usize(i);
            if ui
                .selectable_config(kind.name())
                .selected(*kind == current)
                .build()
                && *kind != current
            {
                *cond = convert_condition(cond, *kind);
                changed = true;
            }
        }
        combo.end();
    }

    match cond {
        DialogueCondition::Flag { key } => {
            changed |= key_editor(ui, key);
        }
        DialogueCondition::Integer { key, op, value } => {
            changed |= key_editor(ui, key);
            changed |= cmp_combo(ui, op);
            if ui.input_int("Value", value).build() {
                changed = true;
            }
        }
        DialogueCondition::StringEquals { key, value } => {
            changed |= key_editor(ui, key);
            if ui.input_text("Value", value).build() {
                changed = true;
            }
        }
        DialogueCondition::Not { cond: inner } => {
            ui.indent();
            changed |= {
                let _level = ui.push_id_usize(0);
                condition_editor(ui, inner)
            };
            ui.unindent();
        }
        DialogueCondition::All { all } => {
            changed |= child_list_editor(ui, all);
        }
        DialogueCondition::Any { any } => {
            changed |= child_list_editor(ui, any);
        }
    }
    changed
}

/// The optional-condition editor used by choices: a "When" checkbox (checked = `Some`) —
/// checking creates an empty `Flag`, unchecking sets `None`; while `Some` it renders
/// [`condition_editor`] below, indented. Works on the caller's LOCAL clone.
pub fn optional_condition_editor(ui: &Ui, when: &mut Option<DialogueCondition>) -> bool {
    let mut changed = false;
    let mut checked = when.is_some();
    if ui.checkbox("When", &mut checked) {
        *when = checked.then(|| DialogueCondition::Flag { key: String::new() });
        changed = true;
    }
    if let Some(cond) = when.as_mut() {
        ui.indent();
        changed |= {
            let _level = ui.push_id_usize(0);
            condition_editor(ui, cond)
        };
        ui.unindent();
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Six distinctive conditions, one per kind, used as conversion sources.
    fn sources() -> Vec<(&'static str, DialogueCondition)> {
        vec![
            ("flag", DialogueCondition::flag("fk")),
            (
                "integer",
                DialogueCondition::integer("ik", DialogueCmp::Gt, 7),
            ),
            (
                "string_equals",
                DialogueCondition::string_equals("sk", "sv"),
            ),
            ("not", DialogueCondition::not(DialogueCondition::flag("nk"))),
            (
                "all",
                DialogueCondition::all(vec![
                    DialogueCondition::flag("a1"),
                    DialogueCondition::integer("a2", DialogueCmp::Lt, 2),
                ]),
            ),
            (
                "any",
                DialogueCondition::any(vec![
                    DialogueCondition::string_equals("y1", "y2"),
                    DialogueCondition::not(DialogueCondition::flag("y3")),
                ]),
            ),
        ]
    }

    #[test]
    fn cond_kind_of_and_names() {
        let cases = [
            (DialogueCondition::flag("k"), CondKind::Flag, "flag"),
            (
                DialogueCondition::integer("k", DialogueCmp::Eq, 0),
                CondKind::Integer,
                "integer",
            ),
            (
                DialogueCondition::string_equals("k", "v"),
                CondKind::StringEquals,
                "string equals",
            ),
            (
                DialogueCondition::not(DialogueCondition::flag("k")),
                CondKind::Not,
                "not",
            ),
            (DialogueCondition::all(vec![]), CondKind::All, "all"),
            (DialogueCondition::any(vec![]), CondKind::Any, "any"),
        ];
        for (cond, kind, name) in cases {
            assert_eq!(CondKind::of(&cond), kind);
            assert_eq!(kind.name(), name);
        }
        assert_eq!(CondKind::KINDS.len(), 6);
    }

    /// All 36 (old kind x target kind) pairs, with the expected result written out literally.
    #[test]
    fn convert_condition_table_all_36_pairs() {
        let f = DialogueCondition::flag("fk");
        let i = DialogueCondition::integer("ik", DialogueCmp::Gt, 7);
        let s = DialogueCondition::string_equals("sk", "sv");
        let n = DialogueCondition::not(DialogueCondition::flag("nk"));
        let a = DialogueCondition::all(vec![
            DialogueCondition::flag("a1"),
            DialogueCondition::integer("a2", DialogueCmp::Lt, 2),
        ]);
        let y = DialogueCondition::any(vec![
            DialogueCondition::string_equals("y1", "y2"),
            DialogueCondition::not(DialogueCondition::flag("y3")),
        ]);
        // (old, to, expected)
        let cases: Vec<(DialogueCondition, CondKind, DialogueCondition)> = vec![
            // from flag: the key is kept on leaf targets
            (f.clone(), CondKind::Flag, f.clone()),
            (
                f.clone(),
                CondKind::Integer,
                DialogueCondition::integer("fk", DialogueCmp::Eq, 0),
            ),
            (
                f.clone(),
                CondKind::StringEquals,
                DialogueCondition::string_equals("fk", ""),
            ),
            (f.clone(), CondKind::Not, DialogueCondition::not(f.clone())),
            (
                f.clone(),
                CondKind::All,
                DialogueCondition::all(vec![f.clone()]),
            ),
            (
                f.clone(),
                CondKind::Any,
                DialogueCondition::any(vec![f.clone()]),
            ),
            // from integer: the i32 payload is NOT carried into a leaf of another type
            (i.clone(), CondKind::Flag, DialogueCondition::flag("ik")),
            (i.clone(), CondKind::Integer, i.clone()),
            (
                i.clone(),
                CondKind::StringEquals,
                DialogueCondition::string_equals("ik", ""),
            ),
            (i.clone(), CondKind::Not, DialogueCondition::not(i.clone())),
            (
                i.clone(),
                CondKind::All,
                DialogueCondition::all(vec![i.clone()]),
            ),
            (
                i.clone(),
                CondKind::Any,
                DialogueCondition::any(vec![i.clone()]),
            ),
            // from string_equals
            (s.clone(), CondKind::Flag, DialogueCondition::flag("sk")),
            (
                s.clone(),
                CondKind::Integer,
                DialogueCondition::integer("sk", DialogueCmp::Eq, 0),
            ),
            (s.clone(), CondKind::StringEquals, s.clone()),
            (s.clone(), CondKind::Not, DialogueCondition::not(s.clone())),
            (
                s.clone(),
                CondKind::All,
                DialogueCondition::all(vec![s.clone()]),
            ),
            (
                s.clone(),
                CondKind::Any,
                DialogueCondition::any(vec![s.clone()]),
            ),
            // from not: leaves get an EMPTY key (structure dropped); the Not child becomes the
            // single list entry of All/Any
            (n.clone(), CondKind::Flag, DialogueCondition::flag("")),
            (
                n.clone(),
                CondKind::Integer,
                DialogueCondition::integer("", DialogueCmp::Eq, 0),
            ),
            (
                n.clone(),
                CondKind::StringEquals,
                DialogueCondition::string_equals("", ""),
            ),
            (n.clone(), CondKind::Not, n.clone()),
            (
                n.clone(),
                CondKind::All,
                DialogueCondition::all(vec![DialogueCondition::flag("nk")]),
            ),
            (
                n.clone(),
                CondKind::Any,
                DialogueCondition::any(vec![DialogueCondition::flag("nk")]),
            ),
            // from all: children preserved on All<->Any
            (a.clone(), CondKind::Flag, DialogueCondition::flag("")),
            (
                a.clone(),
                CondKind::Integer,
                DialogueCondition::integer("", DialogueCmp::Eq, 0),
            ),
            (
                a.clone(),
                CondKind::StringEquals,
                DialogueCondition::string_equals("", ""),
            ),
            (a.clone(), CondKind::Not, DialogueCondition::not(a.clone())),
            (a.clone(), CondKind::All, a.clone()),
            (
                a.clone(),
                CondKind::Any,
                DialogueCondition::any(vec![
                    DialogueCondition::flag("a1"),
                    DialogueCondition::integer("a2", DialogueCmp::Lt, 2),
                ]),
            ),
            // from any: children preserved on Any<->All
            (y.clone(), CondKind::Flag, DialogueCondition::flag("")),
            (
                y.clone(),
                CondKind::Integer,
                DialogueCondition::integer("", DialogueCmp::Eq, 0),
            ),
            (
                y.clone(),
                CondKind::StringEquals,
                DialogueCondition::string_equals("", ""),
            ),
            (y.clone(), CondKind::Not, DialogueCondition::not(y.clone())),
            (
                y.clone(),
                CondKind::All,
                DialogueCondition::all(vec![
                    DialogueCondition::string_equals("y1", "y2"),
                    DialogueCondition::not(DialogueCondition::flag("y3")),
                ]),
            ),
            (y.clone(), CondKind::Any, y.clone()),
        ];
        assert_eq!(cases.len(), 36, "the table covers every 6x6 pair");
        for (old, to, expected) in cases {
            assert_eq!(
                convert_condition(&old, to),
                expected,
                "convert {old:?} -> {to:?}"
            );
        }
    }

    #[test]
    fn convert_condition_preserves_nested_children() {
        let nested = DialogueCondition::all(vec![
            DialogueCondition::any(vec![
                DialogueCondition::flag("deep"),
                DialogueCondition::not(DialogueCondition::integer("n", DialogueCmp::Ge, 3)),
            ]),
            DialogueCondition::string_equals("s", "v"),
        ]);
        // all -> any keeps the whole subtree as the child list
        assert_eq!(
            convert_condition(&nested, CondKind::Any),
            DialogueCondition::any(vec![
                DialogueCondition::any(vec![
                    DialogueCondition::flag("deep"),
                    DialogueCondition::not(DialogueCondition::integer("n", DialogueCmp::Ge, 3)),
                ]),
                DialogueCondition::string_equals("s", "v"),
            ])
        );
        // all -> not wraps the WHOLE subtree
        assert_eq!(
            convert_condition(&nested, CondKind::Not),
            DialogueCondition::not(nested.clone())
        );
        // not -> all unwraps exactly one level
        let not = DialogueCondition::not(nested.clone());
        assert_eq!(
            convert_condition(&not, CondKind::All),
            DialogueCondition::all(vec![nested.clone()])
        );
        // all -> flag drops the structure on purpose (empty key)
        assert_eq!(
            convert_condition(&nested, CondKind::Flag),
            DialogueCondition::flag("")
        );
    }

    #[test]
    fn every_condition_variant_serialises_to_the_engine_json_shape() {
        assert_eq!(
            serde_json::to_value(DialogueCondition::flag("")).unwrap(),
            json!({"type": "flag", "key": ""})
        );
        assert_eq!(
            serde_json::to_value(DialogueCondition::integer("", DialogueCmp::Eq, 0)).unwrap(),
            json!({"type": "integer", "key": "", "op": "eq", "value": 0})
        );
        for (op, name) in [
            (DialogueCmp::Eq, "eq"),
            (DialogueCmp::Ne, "ne"),
            (DialogueCmp::Lt, "lt"),
            (DialogueCmp::Le, "le"),
            (DialogueCmp::Gt, "gt"),
            (DialogueCmp::Ge, "ge"),
        ] {
            let v = serde_json::to_value(DialogueCondition::integer("k", op, 1)).unwrap();
            assert_eq!(v["op"], json!(name), "op serialises as snake_case");
        }
        assert_eq!(
            serde_json::to_value(DialogueCondition::string_equals("", "")).unwrap(),
            json!({"type": "string_equals", "key": "", "value": ""})
        );
        assert_eq!(
            serde_json::to_value(DialogueCondition::not(DialogueCondition::flag("x"))).unwrap(),
            json!({"type": "not", "cond": {"type": "flag", "key": "x"}})
        );
        assert_eq!(
            serde_json::to_value(DialogueCondition::all(vec![DialogueCondition::flag("a")]))
                .unwrap(),
            json!({"type": "all", "all": [{"type": "flag", "key": "a"}]})
        );
        assert_eq!(
            serde_json::to_value(DialogueCondition::any(vec![])).unwrap(),
            json!({"type": "any", "any": []})
        );
    }

    #[test]
    fn every_converter_output_serialises_and_round_trips() {
        for (_name, old) in sources() {
            for to in CondKind::KINDS {
                let converted = convert_condition(&old, to);
                let value = serde_json::to_value(&converted).expect("converter output serialises");
                assert!(
                    value.get("type").is_some_and(|t| t.is_string()),
                    "internally tagged with a string type: {value}"
                );
                let back: DialogueCondition =
                    serde_json::from_value(value).expect("converter output deserialises");
                assert_eq!(back, converted, "round trip unchanged: {to:?} of {old:?}");
            }
        }
    }

    #[test]
    fn nested_conditions_round_trip_through_serde_json_unchanged() {
        let nested = DialogueCondition::all(vec![
            DialogueCondition::any(vec![
                DialogueCondition::flag("has_pass"),
                DialogueCondition::not(DialogueCondition::integer("gold", DialogueCmp::Ge, 10)),
            ]),
            DialogueCondition::string_equals("name", "guard"),
        ]);
        let value = serde_json::to_value(&nested).unwrap();
        let back: DialogueCondition = serde_json::from_value(value).unwrap();
        assert_eq!(back, nested, "to_value/from_value round trip");
        let text = serde_json::to_string(&nested).unwrap();
        let back: DialogueCondition = serde_json::from_str(&text).unwrap();
        assert_eq!(back, nested, "to_string/from_str round trip");
    }
}

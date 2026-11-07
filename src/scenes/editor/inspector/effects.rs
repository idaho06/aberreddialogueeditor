//! The Inspector's effects list editor (the `effects` of a choice and the node's `on_enter`),
//! plus the pure [`convert_effect`] used when the user switches an effect's type and the pure
//! [`move_item`] used by the Up/Down buttons.
//!
//! The editor works on the caller's LOCAL clone and returns `true` when anything changed; the
//! caller turns a change into a whole-value [`Edit`](super::Edit). Widget ids never depend on
//! edited text: every list row is wrapped in `ui.push_id_usize(i)` (plus fixed `"##..."` label
//! suffixes). An empty key shows a faint red "(empty key)" hint — a hint, not an error: the
//! engine accepts empty keys.

use aberredengine::prelude::DialogueEffect;
use aberredengine::prelude::imgui::Ui;

use super::widgets::{RowAction, key_editor, reorder_delete_row, type_combo};

/// The six effect kinds the type combo can switch between.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectKind {
    /// Sets a boolean flag on a signal key.
    SetFlag,
    /// Clears a boolean flag on a signal key.
    RemoveFlag,
    /// Sets an integer signal to a value.
    SetInteger,
    /// Adds a delta to an integer signal.
    AddInteger,
    /// Sets a scalar signal to a value.
    SetScalar,
    /// Sets a string signal to a value.
    SetString,
}

impl EffectKind {
    /// All kinds, in combo order.
    pub const KINDS: [EffectKind; 6] = [
        EffectKind::SetFlag,
        EffectKind::RemoveFlag,
        EffectKind::SetInteger,
        EffectKind::AddInteger,
        EffectKind::SetScalar,
        EffectKind::SetString,
    ];

    /// The kind of an existing effect.
    pub fn of(effect: &DialogueEffect) -> Self {
        match effect {
            DialogueEffect::SetFlag { .. } => Self::SetFlag,
            DialogueEffect::RemoveFlag { .. } => Self::RemoveFlag,
            DialogueEffect::SetInteger { .. } => Self::SetInteger,
            DialogueEffect::AddInteger { .. } => Self::AddInteger,
            DialogueEffect::SetScalar { .. } => Self::SetScalar,
            DialogueEffect::SetString { .. } => Self::SetString,
        }
    }

    /// The display name shown in the type combo.
    pub fn name(self) -> &'static str {
        match self {
            Self::SetFlag => "set flag",
            Self::RemoveFlag => "remove flag",
            Self::SetInteger => "set integer",
            Self::AddInteger => "add integer",
            Self::SetScalar => "set scalar",
            Self::SetString => "set string",
        }
    }
}

/// The signal key of an effect (every variant carries one).
fn effect_key(effect: &DialogueEffect) -> &str {
    match effect {
        DialogueEffect::SetFlag { key }
        | DialogueEffect::RemoveFlag { key }
        | DialogueEffect::SetInteger { key, .. }
        | DialogueEffect::AddInteger { key, .. }
        | DialogueEffect::SetScalar { key, .. }
        | DialogueEffect::SetString { key, .. } => key,
    }
}

/// The mutable signal key of an effect.
fn effect_key_mut(effect: &mut DialogueEffect) -> &mut String {
    match effect {
        DialogueEffect::SetFlag { key }
        | DialogueEffect::RemoveFlag { key }
        | DialogueEffect::SetInteger { key, .. }
        | DialogueEffect::AddInteger { key, .. }
        | DialogueEffect::SetScalar { key, .. }
        | DialogueEffect::SetString { key, .. } => key,
    }
}

/// PURE type conversion of an effect. Converting to the SAME kind returns `old` unchanged.
/// The `key` is ALWAYS kept. `SetInteger`<->`AddInteger` keep the i32 payload (`value`<->`delta`);
/// every other numeric/string payload gets its default (`0`, `0.0`, `""`).
pub fn convert_effect(old: &DialogueEffect, to: EffectKind) -> DialogueEffect {
    if EffectKind::of(old) == to {
        return old.clone();
    }
    let key = effect_key(old).to_owned();
    let int_payload = match old {
        DialogueEffect::SetInteger { value, .. } => Some(*value),
        DialogueEffect::AddInteger { delta, .. } => Some(*delta),
        _ => None,
    };
    match to {
        EffectKind::SetFlag => DialogueEffect::SetFlag { key },
        EffectKind::RemoveFlag => DialogueEffect::RemoveFlag { key },
        EffectKind::SetInteger => DialogueEffect::SetInteger {
            key,
            value: int_payload.unwrap_or(0),
        },
        EffectKind::AddInteger => DialogueEffect::AddInteger {
            key,
            delta: int_payload.unwrap_or(0),
        },
        EffectKind::SetScalar => DialogueEffect::SetScalar { key, value: 0.0 },
        EffectKind::SetString => DialogueEffect::SetString {
            key,
            value: String::new(),
        },
    }
}

/// PURE list reorder: moves the element at `index` by `delta` positions, clamped to the list.
/// A no-op when the list is empty or `index` is out of range.
pub fn move_item<T>(items: &mut Vec<T>, index: usize, delta: i32) {
    if items.is_empty() || index >= items.len() {
        return;
    }
    let last = (items.len() - 1) as i64;
    let new = (index as i64 + delta as i64).clamp(0, last) as usize;
    if new != index {
        let item = items.remove(index);
        items.insert(new, item);
    }
}

/// The effects list editor. Each row (wrapped in `push_id_usize(i)`) holds a type combo
/// (picking another kind calls [`convert_effect`]), a key input, a value widget per kind
/// (`input_int` for SetInteger value / AddInteger delta, `input_float` for SetScalar value, a
/// text input for SetString value; none for the flag kinds), Up/Down/Delete small buttons (Up
/// disabled on the first row, Down on the last, via `begin_disabled`), and a "[+ Add effect]"
/// button ends the list (adds `SetFlag{key: ""}`). Returns `true` when anything changed;
/// mutates the caller's LOCAL clone.
pub fn effects_editor(ui: &Ui, effects: &mut Vec<DialogueEffect>) -> bool {
    let mut changed = false;
    let mut remove = None;
    let mut reorder = None;
    let last = effects.len().saturating_sub(1);
    for (i, effect) in effects.iter_mut().enumerate() {
        let _row = ui.push_id_usize(i);

        // Type combo: picking another kind converts the effect (the key is kept).
        let current = EffectKind::of(effect);
        changed |= type_combo(
            ui,
            "Type##effect_type",
            current.name(),
            current,
            &EffectKind::KINDS,
            |kind| kind.name(),
            |kind| *effect = convert_effect(effect, kind),
        );

        changed |= key_editor(ui, effect_key_mut(effect));

        // The value widget per kind (none for the flag kinds).
        match effect {
            DialogueEffect::SetInteger { value, .. }
            | DialogueEffect::AddInteger { delta: value, .. } => {
                if ui.input_int("Value", value).build() {
                    changed = true;
                }
            }
            DialogueEffect::SetScalar { value, .. } => {
                if ui.input_float("Value", value).build() {
                    changed = true;
                }
            }
            DialogueEffect::SetString { value, .. } => {
                if ui.input_text("Value", value).build() {
                    changed = true;
                }
            }
            DialogueEffect::SetFlag { .. } | DialogueEffect::RemoveFlag { .. } => {}
        }

        // Up (disabled on the first) / Down (disabled on the last) / Delete on one row.
        match reorder_delete_row(ui, i, last) {
            Some(RowAction::Move(delta)) => reorder = Some((i, delta)),
            Some(RowAction::Delete) => remove = Some(i),
            None => {}
        }
    }
    if let Some((i, delta)) = reorder {
        move_item(effects, i, delta);
        changed = true;
    }
    if let Some(i) = remove {
        effects.remove(i);
        changed = true;
    }
    if ui.button("[+ Add effect]") {
        effects.push(DialogueEffect::set_flag(""));
        changed = true;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Six distinctive effects, one per kind, used as conversion sources.
    fn sources() -> Vec<(&'static str, DialogueEffect)> {
        vec![
            ("set_flag", DialogueEffect::set_flag("k")),
            ("remove_flag", DialogueEffect::remove_flag("k")),
            ("set_integer", DialogueEffect::set_integer("k", 5)),
            ("add_integer", DialogueEffect::add_integer("k", -3)),
            ("set_scalar", DialogueEffect::set_scalar("k", 1.5)),
            ("set_string", DialogueEffect::set_string("k", "v")),
        ]
    }

    #[test]
    fn effect_kind_of_and_names() {
        let cases = [
            (
                DialogueEffect::set_flag("k"),
                EffectKind::SetFlag,
                "set flag",
            ),
            (
                DialogueEffect::remove_flag("k"),
                EffectKind::RemoveFlag,
                "remove flag",
            ),
            (
                DialogueEffect::set_integer("k", 0),
                EffectKind::SetInteger,
                "set integer",
            ),
            (
                DialogueEffect::add_integer("k", 0),
                EffectKind::AddInteger,
                "add integer",
            ),
            (
                DialogueEffect::set_scalar("k", 0.0),
                EffectKind::SetScalar,
                "set scalar",
            ),
            (
                DialogueEffect::set_string("k", ""),
                EffectKind::SetString,
                "set string",
            ),
        ];
        for (effect, kind, name) in cases {
            assert_eq!(EffectKind::of(&effect), kind);
            assert_eq!(kind.name(), name);
        }
        assert_eq!(EffectKind::KINDS.len(), 6);
    }

    /// All 36 (old kind x target kind) pairs, with the expected result written out literally.
    #[test]
    fn convert_effect_table_all_36_pairs() {
        let sf = DialogueEffect::set_flag("k");
        let rf = DialogueEffect::remove_flag("k");
        let si = DialogueEffect::set_integer("k", 5);
        let ai = DialogueEffect::add_integer("k", -3);
        let sc = DialogueEffect::set_scalar("k", 1.5);
        let ss = DialogueEffect::set_string("k", "v");
        // (old, to, expected): the key is always kept; only SetInteger<->AddInteger keep the i32
        // (value<->delta); every other payload gets its default.
        let cases: Vec<(DialogueEffect, EffectKind, DialogueEffect)> = vec![
            (sf.clone(), EffectKind::SetFlag, sf.clone()),
            (sf.clone(), EffectKind::RemoveFlag, rf.clone()),
            (
                sf.clone(),
                EffectKind::SetInteger,
                DialogueEffect::set_integer("k", 0),
            ),
            (
                sf.clone(),
                EffectKind::AddInteger,
                DialogueEffect::add_integer("k", 0),
            ),
            (
                sf.clone(),
                EffectKind::SetScalar,
                DialogueEffect::set_scalar("k", 0.0),
            ),
            (
                sf.clone(),
                EffectKind::SetString,
                DialogueEffect::set_string("k", ""),
            ),
            (rf.clone(), EffectKind::SetFlag, sf.clone()),
            (rf.clone(), EffectKind::RemoveFlag, rf.clone()),
            (
                rf.clone(),
                EffectKind::SetInteger,
                DialogueEffect::set_integer("k", 0),
            ),
            (
                rf.clone(),
                EffectKind::AddInteger,
                DialogueEffect::add_integer("k", 0),
            ),
            (
                rf.clone(),
                EffectKind::SetScalar,
                DialogueEffect::set_scalar("k", 0.0),
            ),
            (
                rf.clone(),
                EffectKind::SetString,
                DialogueEffect::set_string("k", ""),
            ),
            (si.clone(), EffectKind::SetFlag, sf.clone()),
            (si.clone(), EffectKind::RemoveFlag, rf.clone()),
            (si.clone(), EffectKind::SetInteger, si.clone()),
            // set_integer -> add_integer keeps the i32 as delta
            (
                si.clone(),
                EffectKind::AddInteger,
                DialogueEffect::add_integer("k", 5),
            ),
            (
                si.clone(),
                EffectKind::SetScalar,
                DialogueEffect::set_scalar("k", 0.0),
            ),
            (
                si.clone(),
                EffectKind::SetString,
                DialogueEffect::set_string("k", ""),
            ),
            (ai.clone(), EffectKind::SetFlag, sf.clone()),
            (ai.clone(), EffectKind::RemoveFlag, rf.clone()),
            // add_integer -> set_integer keeps the i32 as value
            (
                ai.clone(),
                EffectKind::SetInteger,
                DialogueEffect::set_integer("k", -3),
            ),
            (ai.clone(), EffectKind::AddInteger, ai.clone()),
            (
                ai.clone(),
                EffectKind::SetScalar,
                DialogueEffect::set_scalar("k", 0.0),
            ),
            (
                ai.clone(),
                EffectKind::SetString,
                DialogueEffect::set_string("k", ""),
            ),
            (sc.clone(), EffectKind::SetFlag, sf.clone()),
            (sc.clone(), EffectKind::RemoveFlag, rf.clone()),
            (
                sc.clone(),
                EffectKind::SetInteger,
                DialogueEffect::set_integer("k", 0),
            ),
            (
                sc.clone(),
                EffectKind::AddInteger,
                DialogueEffect::add_integer("k", 0),
            ),
            (sc.clone(), EffectKind::SetScalar, sc.clone()),
            (
                sc.clone(),
                EffectKind::SetString,
                DialogueEffect::set_string("k", ""),
            ),
            (ss.clone(), EffectKind::SetFlag, sf.clone()),
            (ss.clone(), EffectKind::RemoveFlag, rf.clone()),
            (
                ss.clone(),
                EffectKind::SetInteger,
                DialogueEffect::set_integer("k", 0),
            ),
            (
                ss.clone(),
                EffectKind::AddInteger,
                DialogueEffect::add_integer("k", 0),
            ),
            (
                ss.clone(),
                EffectKind::SetScalar,
                DialogueEffect::set_scalar("k", 0.0),
            ),
            (ss.clone(), EffectKind::SetString, ss.clone()),
        ];
        assert_eq!(cases.len(), 36, "the table covers every 6x6 pair");
        for (old, to, expected) in cases {
            assert_eq!(
                convert_effect(&old, to),
                expected,
                "convert {old:?} -> {to:?}"
            );
        }
    }

    #[test]
    fn move_item_up_down_and_clamping() {
        let mut v = vec!['a', 'b', 'c'];
        move_item(&mut v, 0, -1);
        assert_eq!(v, ['a', 'b', 'c'], "clamped at the top");
        move_item(&mut v, 0, 1);
        assert_eq!(v, ['b', 'a', 'c'], "down by one");
        move_item(&mut v, 2, 1);
        assert_eq!(v, ['b', 'a', 'c'], "clamped at the bottom");
        move_item(&mut v, 2, -5);
        assert_eq!(
            v,
            ['c', 'b', 'a'],
            "a large negative delta clamps to the top"
        );
        move_item(&mut v, 0, 10);
        assert_eq!(
            v,
            ['b', 'a', 'c'],
            "a large positive delta clamps to the bottom"
        );
        move_item(&mut v, 1, 0);
        assert_eq!(v, ['b', 'a', 'c'], "zero delta is a no-op");
    }

    #[test]
    fn move_item_no_ops_on_empty_single_element_and_out_of_range_index() {
        let mut empty: Vec<i32> = Vec::new();
        move_item(&mut empty, 0, 1);
        assert!(empty.is_empty());

        let mut single = vec![1];
        move_item(&mut single, 0, 1);
        move_item(&mut single, 0, -1);
        assert_eq!(single, [1], "a single element cannot move");

        let mut v = vec![1, 2, 3];
        move_item(&mut v, 5, 1);
        move_item(&mut v, usize::MAX, -1);
        assert_eq!(v, [1, 2, 3], "an out-of-range index is a no-op");
    }

    #[test]
    fn every_effect_variant_serialises_to_the_engine_json_shape() {
        assert_eq!(
            serde_json::to_value(DialogueEffect::set_flag("")).unwrap(),
            json!({"type": "set_flag", "key": ""})
        );
        assert_eq!(
            serde_json::to_value(DialogueEffect::remove_flag("")).unwrap(),
            json!({"type": "remove_flag", "key": ""})
        );
        assert_eq!(
            serde_json::to_value(DialogueEffect::set_integer("", 0)).unwrap(),
            json!({"type": "set_integer", "key": "", "value": 0})
        );
        assert_eq!(
            serde_json::to_value(DialogueEffect::add_integer("", 0)).unwrap(),
            json!({"type": "add_integer", "key": "", "delta": 0})
        );
        assert_eq!(
            serde_json::to_value(DialogueEffect::set_scalar("", 0.0)).unwrap(),
            json!({"type": "set_scalar", "key": "", "value": 0.0})
        );
        assert_eq!(
            serde_json::to_value(DialogueEffect::set_string("", "")).unwrap(),
            json!({"type": "set_string", "key": "", "value": ""})
        );
    }

    #[test]
    fn every_converter_output_serialises_and_round_trips() {
        for (_name, old) in sources() {
            for to in EffectKind::KINDS {
                let converted = convert_effect(&old, to);
                let value = serde_json::to_value(&converted).expect("converter output serialises");
                assert!(
                    value.get("type").is_some_and(|t| t.is_string()),
                    "internally tagged with a string type: {value}"
                );
                let back: DialogueEffect =
                    serde_json::from_value(value).expect("converter output deserialises");
                assert_eq!(back, converted, "round trip unchanged: {to:?} of {old:?}");
            }
        }
    }
}

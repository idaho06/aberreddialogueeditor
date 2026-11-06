# Plan 04 — Inspector, condition/effect editors, validation panel, polish

**Run in:** `/home/idaho/Projects/aberreddialogueeditor`. Read `docs/plans/00-overview.md` first.
**Depends on:** plan 03 complete. **Done when:** every field of the dialogue format is editable from the
inspector, the Problems panel lists all lint findings, saving is gated by the engine, docs are written, a vibe
code-review pass is done, and the final checklist at the end is green. All steps committed by the user.

## Policies (verbatim from 00-overview.md — obey them)

- **COMMIT POLICY:** small, meaningful commits. After a verified step, stage exactly its files
  (`git add <explicit paths>`), propose a ONE-LINE commit message, NO `Co-Authored-By` trailer, and STOP. The user
  runs `git commit`; continue only after they confirm. Never run `git commit` / `git push`.
- **VIBE POLICY:** implementation, fixes, test creation, simplification, code review go to the vibe MCP
  (`vibe_submit`/`vibe_wait`/`vibe_result(diff=true)`/`vibe_apply`). Claude writes specs, reviews diffs, runs the
  acceptance recipe, debugs, stages. Claude implements directly only if the user explicitly asks. Pass the step's
  spec block verbatim as `spec`; always set `scope`. A vibe worktree starts at committed HEAD, so start a step only
  after the previous one is committed (chain with `base_job` only inside one step). If vibe is not connected,
  STOP and tell the user.
- **Acceptance** is always a `just` recipe run by Claude itself after `vibe_apply`.

## UI pattern used by every panel in this plan (put it in each spec)

The GUI runs on the render thread with a `fn(&mut GuiCtx)`. To keep locks short and avoid borrow trouble:
1. lock the `WorkspaceMutex`, CLONE what the panel needs (the selected `EdNode`, titles, problems), unlock;
2. draw widgets against the local clone, collecting an `Vec<Edit>` of requested changes (a small enum such as
   `Edit::SetText(NodeKey, String)`, `Edit::MoveChoice(NodeKey, usize, i32)`, ...);
3. lock again and apply the edits through `DialogueDoc` mutators (they bump `revision`/`dirty`).
Never hold the lock across imgui widget calls, `request_async_dialog`, or rfd. Use `ui.push_id(..)` for every
list item and recursion level. Only emit an edit when the widget reports a change (`input_text(..).build()` returns
true). Labels use the `"Visible##id"` convention. The pure edit-application function (`apply_edits(doc, edits)`)
must be unit-tested.

---

## Step 1 — Inspector: node fields and outputs (two chained vibe jobs)

### 1a — Node fields
**Executor:** vibe. **Scope:** `src/scenes/editor/inspector/{mod,node_fields}.rs`, `src/scenes/editor/gui.rs`,
`src/model/doc.rs` (only missing mutators), `tests/inspector_edits.rs`. **Acceptance:** `just check`; Claude runs
the app: select a node, edit its id (links keep pointing at it; the graph header updates), speaker, text, labels,
"Set as start". **Suggested message:** `Add inspector panel with node id, speaker, text and labels`

**Vibe spec (pass verbatim):**
```
Project: /home/idaho/Projects/aberreddialogueeditor. Read CLAUDE.md, docs/plans/00-overview.md and
docs/plans/04-editor-inspector-validation.md ("UI pattern used by every panel") first. Existing: model with
mutators (set_text, set_speaker, set_labels, rename_node, set_start, select, delete_node), projection that redraws
nodes on revision change, menus, tabs.

GOAL: an "Inspector" window (a plain imgui window, e.g. docked right by default position via
`.position([w-360, 60], Condition::FirstUseEver).size([340, h-80], Condition::FirstUseEver)`) showing the SELECTED
node of the active doc; "Nothing selected" otherwise.

CONTENT (this step: node fields only)
 - Header: node id as an editable single-line input (rename_node on every change; links hold NodeKeys, so they
   survive a rename; duplicate ids are only flagged by the linter), plus a small red "duplicate id" / "empty id"
   note when the doc's problems (doc.problems()) report it for this node.
 - "Set as start" button (disabled and labelled "Start node" when it already is).
 - Speaker: a checkbox "Has speaker" + single-line input (Some/None; unchecking sets None).
 - Text: multiline input (`input_text_multiline`, ~6 lines high, full width).
 - Labels: a list of single-line inputs with a [-] button per row and a [+ Add label] button (labels are opaque
   strings).
 - A read-only line "On enter effects: N" and "Outputs: next/choices (N)" placeholder for the next step.
Implementation: src/scenes/editor/inspector/node_fields.rs draws these widgets from a cloned `EdNode` and returns
`Vec<Edit>`; `apply_edits(doc: &mut DialogueDoc, edits: Vec<Edit>)` is a PURE function in
src/scenes/editor/inspector/mod.rs (or src/model/edits.rs), unit-tested: every Edit variant changes exactly the
intended field, bumps revision and sets dirty, and edits addressed to a vanished NodeKey are ignored (no panic).
Typing into an input must not lose focus: do not change widget ids when the text changes (ids come from the
NodeKey, never from the text).
CONSTRAINTS: no outputs/choices/conditions yet. DONE WHEN: `just check` passes. Report the first failing test.
```

### 1b — Outputs and choices
**Executor:** vibe (`base_job` = 1a or after the user committed 1a). **Scope:** `src/scenes/editor/inspector/
outputs.rs`, `src/scenes/editor/inspector/mod.rs`, `src/model/doc.rs` (missing mutators), `tests/**`.
**Acceptance:** `just check`; Claude runs the app and verifies mode switching, target combos (including the
red dangling entry), choice add/delete/up/down and that the graph updates live (pins/rows/edges follow).
**Suggested message:** `Add inspector editing for next link and choices with reordering`

**Vibe spec (pass verbatim):**
```
Project: /home/idaho/Projects/aberreddialogueeditor. Read CLAUDE.md, docs/plans/00-overview.md and
docs/plans/04-editor-inspector-validation.md ("UI pattern") first. Existing: Inspector window with node fields
(src/scenes/editor/inspector/), model mutators set_outputs, set_link(key, Slot, Link), add_choice, delete_choice,
move_choice, set_choice_text, set_choice_labels.

GOAL: edit a node's outputs in the Inspector.
 - Mode: two radio buttons "Next" / "Choices". Switching Next -> Choices creates ONE empty choice that inherits the
   current next link; Choices -> Next keeps the FIRST choice's link (or End) and DROPS the choices (ask no
   confirmation, no undo exists). Never leave `Outputs::Choices(vec![])`.
 - Next mode: one "Next" target combo.
 - Choices mode: a list; per choice a collapsing section "Choice N: <text truncated>" containing: text input,
   target combo, labels list ([+]/[-] like node labels), up / down / delete buttons (up disabled on the first,
   down on the last; delete of the LAST choice switches the node to Next(End) through the model), and a
   placeholder line for condition/effects (next step). A "[+ Add choice]" button at the end.
 - Target combo (a reusable widget `target_combo(ui, id, current: &Link, nodes: &[(NodeKey, String)]) ->
   Option<Link>`): entries "(end)" (Link::End) then every node by id; when several nodes share an id, show
   "id (#N)" with N = position in node order so entries stay distinguishable; a `Link::Dangling(s)` is shown as
   the current value in RED text "unknown: s" and still selectable-away. Selecting an entry yields the new Link.
 - All changes go through `Edit` variants + `apply_edits` (pure, tested): ChangeMode, SetLink(Slot, Link),
   AddChoice, DeleteChoice(i), MoveChoice(i, delta), SetChoiceText/Labels.
TESTS: apply_edits cases: mode switches (link inheritance, no empty Choices), delete last choice -> Next(End),
move up/down clamping, target change to End/Node, edits to a vanished node ignored; `target_combo` entry
building as a pure function `combo_entries(nodes) -> Vec<(Link, String)>` including duplicate-id disambiguation.
CONSTRAINTS: no condition/effect editors yet (placeholders only). DONE WHEN: `just check` passes.
```

**Claude review checklist (both):** ids derived from NodeKey, not text; no lock held across widgets; no empty
`Choices` can be produced; combos handle duplicate ids and Dangling.

---

## Step 2 — Condition and effect editors

**Executor:** vibe. **Scope:** `src/scenes/editor/inspector/{expr,effects}.rs`,
`src/scenes/editor/inspector/{mod,outputs,node_fields}.rs`, `tests/expr_edit.rs`.
**Acceptance:** `just check`; Claude runs the app and verifies: add a `when` to a choice (flag / integer with
op / string_equals / not / all / any, nested at least two levels deep), switch types, remove conditions; add every
effect type to a choice and to a node's on_enter; reorder/delete effects; saved JSON matches the engine format
(check the file by eye or via the existing `load_dialogue` test on a fixture saved by the ops layer).
**Suggested message:** `Add recursive condition editor and effect list editors`

**Vibe spec (pass verbatim):**
```
Project: /home/idaho/Projects/aberreddialogueeditor. Read CLAUDE.md, docs/plans/00-overview.md ("Engine facts",
"Dialogue types") and docs/plans/04-editor-inspector-validation.md ("UI pattern") first. Verify the exact enum
definitions in /home/idaho/Projects/aberredengine/crates/aberred-core/src/resources/dialogue_def.rs before use:
DialogueCondition { Flag{key}, Integer{key,op: DialogueCmp,value: i32}, StringEquals{key,value}, Not{cond:
Box<DialogueCondition>}, All{all: Vec<DialogueCondition>}, Any{any: Vec<DialogueCondition>} };
DialogueCmp { Eq, Ne, Lt, Le, Gt, Ge }; DialogueEffect { SetFlag{key}, RemoveFlag{key}, SetInteger{key,value: i32},
AddInteger{key,delta: i32}, SetScalar{key,value: f32}, SetString{key,value} }.

GOAL: editors for conditions and effects, used by every choice (`when`, `effects`) and by the node (`on_enter`).

1. src/scenes/editor/inspector/expr.rs
 - `condition_editor(ui, id: &str, cond: &mut DialogueCondition) -> bool` (returns true when something changed;
   mutates a LOCAL clone, the caller turns a change into an Edit). Renders: a type combo (flag, integer,
   string_equals, not, all, any); fields per type (key text input; for integer: an operator combo showing
   "==, !=, <, <=, >, >=" and an `input_int` value; for string_equals: value text input); `not` shows one nested
   editor indented; `all`/`any` show a list of nested editors each with a [-] button, and a [+ Add condition]
   button (adds `Flag{key:""}`). Every recursion level and list item uses `ui.push_id(..)` so widget ids never
   collide. Recursion depth is not limited, but use `ui.indent()`/`ui.unindent()` per level.
 - Switching the TYPE builds a sensible default: flag -> Flag{key:""}; integer -> Integer{key:"", op: Eq, value:
   0}; string_equals -> StringEquals{key:"", value:""}; not -> Not{cond: Flag{key:""}}; all/any -> the list
   containing ONE Flag{key:""}; when converting between all<->any keep the children; when converting a leaf to
   not/all/any wrap the old leaf as the first child (so no work is lost). Implement the conversion as a pure
   function `convert_condition(old: &DialogueCondition, to: CondKind) -> DialogueCondition` with unit tests.
 - Optional condition on a choice: a checkbox "When" that adds/removes (`Option<DialogueCondition>`), default Flag.
2. src/scenes/editor/inspector/effects.rs
 - `effects_editor(ui, id: &str, effects: &mut Vec<DialogueEffect>) -> bool`: list, each row: a type combo
   (set_flag, remove_flag, set_integer, add_integer, set_scalar, set_string), key input, value/delta widget
   (`input_int` for i32, `input_float` for f32, text for string), up / down / delete buttons, and
   [+ Add effect] (default SetFlag{key:""}). Pure `convert_effect(old, to: EffectKind)` keeps the key when
   possible (tested).
3. Wire them: each choice's section gets "When" and "Effects" sub-sections; the node gets an "On enter effects"
   section. Changes become `Edit::SetChoiceWhen(NodeKey, usize, Option<DialogueCondition>)`,
   `Edit::SetChoiceEffects(.., Vec<DialogueEffect>)`, `Edit::SetOnEnter(NodeKey, Vec<DialogueEffect>)` applied by
   the pure `apply_edits` (extend its tests).
4. Empty keys are allowed in the UI (the engine accepts them); show them with a faint red "(empty key)" hint, not
   an error.
TESTS: convert_condition/convert_effect tables, apply_edits for the new variants, a serde round trip proving every
condition/effect variant produced by the editors serialises to the engine's JSON shape (`"type": "flag"` etc.,
compare with `serde_json::to_value`).
CONSTRAINTS: scope only. DONE WHEN: `just check` passes. Report the first failing test.
```

**Claude review checklist:** every recursion level has a `push_id`; type conversion never loses children
unexpectedly; no state held across frames except in the model; f32/i32 widgets match the engine field types.

---

## Step 3 — Problems panel and save-gate UX

**Executor:** vibe. **Scope:** `src/scenes/editor/{problems,modals,gui,status}.rs`,
`src/graph/camera.rs` (center-on helper if missing), `tests/**`.
**Acceptance:** `just check`; Claude runs the app: duplicate an id → red node outline + Problems list shows both
nodes + save is refused with the engine's message and the file on disk stays untouched; fix it → save works;
unreachable node shows as a warning and does NOT block saving; clicking a problem selects the node and centres the
view on it.
**Suggested message:** `Add Problems panel, status bar and clearer save-gate errors`

**Vibe spec (pass verbatim):**
```
Project: /home/idaho/Projects/aberreddialogueeditor. Read CLAUDE.md, docs/plans/00-overview.md and
docs/plans/04-editor-inspector-validation.md ("UI pattern") first. Existing: `lint(doc)`, `doc.problems()`
(cached by revision), `engine_gate(doc)`, ops::save_doc (refuses with OpError::Invalid(engine message)), the error
modal fed by Workspace::notifications, Nodes panel, node outlines that turn red for lint errors.

GOAL: make validation visible and the save gate understandable.
 - "Problems" window: header with counts "N errors, M warnings", then two groups (Errors first, then Warnings).
   Each row: severity colour dot + message; click -> `doc.select(node)` and centre the camera on the node
   (`CamState { target: node centre, zoom: unchanged }`, via the view-only camera setter; add a pure
   `center_on(cam, point)` to src/graph/camera.rs with a test). Rows without a node (e.g. StartMissing) are not
   clickable. Also list `doc.load_warnings` in a collapsed "Load warnings (N)" section. "No problems" when empty.
   Window is open by default; View -> Problems toggles it (keep the open flag in the GUI-side state).
 - Status bar (a small bottom window or a fixed-position overlay line; keep it simple, e.g. an imgui window with
   no decoration pinned to the bottom): file path or "Untitled", dirty marker, "E errors / W warnings" in red/amber/
   green, node count, zoom percentage.
 - Save-gate modal: when ops::save_doc returns OpError::Invalid(msg) the error modal (existing notifications
   mechanism) must show a title "Cannot save: the engine would reject this dialogue", the engine message, and the
   line "See the Problems panel for all issues." Other OpErrors (Io/Json) keep their own titles. Warnings never
   block saving.
 - Edit mode shortcut: Ctrl+Shift+P toggles the Problems window (only when `!want_text_input`).
TESTS: pure helpers: `problem_counts(&[Problem]) -> (usize, usize)`, `center_on`, modal title selection
`modal_title(&OpError) -> &'static str`, ordering of grouped rows (errors before warnings, stable by node order).
CONSTRAINTS: scope only; do not change lint semantics. DONE WHEN: `just check` passes.
```

**Claude review checklist:** unreachable never blocks saving; counts agree with the linter; centre-on does not bump
`revision`.

---

## Step 4 — Docs, simplification and review passes

**Executor:** vibe for all three sub-jobs; Claude reads every diff and runs `just check` after each.
Sub-steps are separate commits, in this order.

### 4a — Documentation (vibe)
**Scope:** `README.md`, `docs/architecture.md`, `docs/gotchas.md`, `CLAUDE.md`.
**Suggested message:** `Add README and architecture docs`
```
Project: /home/idaho/Projects/aberreddialogueeditor. Read the source tree and docs/plans/*.md. Write: README.md
(what the tool is, build/run via just, controls table, file format = engine DialogueDef JSON, how to use the output
in a game: `load_dialogue(path)` / `DialogueStore::insert`, limitations: no undo, no preview, positions are not
saved, fixed-size nodes); docs/architecture.md (workspace model, thread model, projection, interaction state
machine, module map matching the real src/ tree, how to add a panel/menu action/condition type); docs/gotchas.md
(B0002, render-only resources, fn-pointer GUI, AppState Arc sharing, UiCapture because the engine does not mask
scene-GUI input, mouse_world lags the camera, lock discipline, FontMetrics async, engine facts that bit us);
update CLAUDE.md to match reality (module responsibilities where the name alone would mislead, commands, the
commit/vibe policies from docs/plans/00-overview.md). Verify every statement against the code before writing it.
Docs only. DONE WHEN: `just check` passes.
```

### 4b — Simplify (vibe skill)
Run the vibe job with `skill="simplify"`, scope `src/**`, spec: "Review the whole crate for reuse,
simplification and efficiency cleanups; behaviour must not change; keep all tests passing". Claude reads the diff
critically (reject churn), `vibe_apply`, `just check`. **Suggested message:** `Simplify editor code without behaviour changes`

### 4c — Code review (vibe skill)
Run the vibe job with `skill="code-review"`, scope `src/**` + `tests/**`, spec: "Review for correctness bugs, with
extra attention to: lock discipline (no lock across rfd/imgui calls), B0002-style system parameter conflicts,
projection leaks (entities not despawned), link integrity after delete/rename/reorder, Dangling handling, save
atomicity, panics on empty documents or vanished NodeKeys". Claude triages the findings, writes precise fix specs
and sends them to vibe as follow-up jobs (never hand-fix), then `just check`.
**Suggested messages:** one per fix batch, e.g. `Fix <what> found in code review`.

---

## FINAL CHECKLIST (whole project)

Automated
- [ ] `just check` green in `/home/idaho/Projects/aberreddialogueeditor`
- [ ] `just check` green in `/home/idaho/Projects/aberredengine` (plan 01 pushed)
- [ ] a test saves a doc via `ops::save_doc` and loads the result with `aberredengine::prelude::load_dialogue`
- [ ] lint soundness / coverage parity tests present and passing (plan 02 step 3)

Manual (Claude launches `just run`; the user confirms visually)
- [ ] open `tests/fixtures/guard.json`: auto-layout is readable; start node has the green header
- [ ] select, drag nodes; connect a choice pin to another node; drag a linked pin off to remove the link
- [ ] right-click menu on a node (Set as start, Delete node) and on the canvas (Add node here); Edit → Add node
- [ ] inspector: edit id/speaker/text/labels; switch Next ↔ Choices; reorder choices; edit a nested
      all/any/not condition; add every effect type; node `on_enter`
- [ ] pan (middle/right drag), wheel zoom at the cursor, F frame all, Re-layout; two tabs keep separate cameras
- [ ] introduce a duplicate id and a dangling reference: red outlines, Problems panel lists them, save refused with
      the engine's message, file on disk untouched; fix both, save succeeds; unreachable node = warning only
- [ ] save, reopen the saved file in a second tab: identical graph; closing a dirty tab asks for confirmation
- [ ] no panic when closing the last tab, deleting the start node, or opening an invalid JSON file (error modal)

Hand-off
- [ ] README and docs describe the real code; `CLAUDE.md` current
- [ ] the user has committed every step; Claude never committed or pushed anything

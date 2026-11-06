# Plan 02 — Editor foundation: skeleton, model, linter, workspace and files

**Run in:** `/home/idaho/Projects/aberreddialogueeditor` (currently NOT a git repo, empty except `docs/`).
Read `docs/plans/00-overview.md` first (decisions, architecture, engine facts).
**Depends on:** nothing (parallel with plan 01). **Done when:** the app opens, loads, saves and tabs dialogue JSON
files (no graph drawing yet); `just check` green; all steps committed by the user.

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

## Conventions for every vibe spec in this plan (paste into each spec)

- Rust edition 2024. Zero clippy warnings (`cargo clippy --all-targets -- -D warnings`), `cargo fmt` clean.
- Engine imports through `aberredengine::prelude::{...}`; use `aberredengine::core::...` only for items the
  prelude lacks. Do not add `bevy_ecs` as a dependency (the facade re-exports it).
- Model code (`src/model/**`) must not import anything from the engine except the dialogue types
  (`DialogueDef, NodeDef, ChoiceDef, DialogueCondition, DialogueCmp, DialogueEffect, DialogueTree, DialogueError`)
  and `Vec2` — keep it unit-testable without a window.
- Tests: plain `#[cfg(test)]` modules or `tests/*.rs`; fixtures under `tests/fixtures/`.
- Never request render-only engine resources (`TextureStore`, `FontStore`, ...) in logic systems; never request
  `ResMut<AppState>`/`ResMut<WorldSignals>` twice in one system (see 00-overview "Engine facts").

---

## Step 1 — Bootstrap

**Executor:** Claude does the repo setup (`git init` etc.), then vibe writes the skeleton.

### 1a — Claude (repo setup; ask the user before `git init`)
1. Ask the user to confirm `git init` in `/home/idaho/Projects/aberreddialogueeditor`.
2. `git init`; write `.gitignore` (`/target`, `imgui.ini`, `*.swp`, `/.vibe`); stage `.gitignore` and
   `docs/plans/*.md`; propose `Initial commit: .gitignore and implementation plans`; STOP for the user's commit.

### 1b — vibe skeleton
**Scope:** `Cargo.toml`, `Cargo.lock`, `config.ini`, `justfile`, `CLAUDE.md`, `src/**`.
**Acceptance:** `just check`, then Claude runs `just run` and confirms a window with a menu bar appears (launch it,
take no screenshots unless the `run` skill is available; at minimum confirm the process starts without a panic in
the log and exits cleanly via File → Quit).
**Stage / message:** all scope files. Suggested: `Add engine app skeleton with menu bar, justfile and CLAUDE.md`

**Vibe spec (pass verbatim):**
```
Create a new Rust binary crate named `aberreddialogueeditor` in /home/idaho/Projects/aberreddialogueeditor (the
directory already has docs/plans/ and a git repo; do not touch docs/). It is a node-graph editor for AberredEngine
dialogue JSON; this step builds ONLY the empty app shell.

REFERENCE (read-only, copy patterns, do not modify): /home/idaho/Projects/aberrededitor (same engine, working app):
Cargo.toml, config.ini, src/main.rs, src/scenes/editor/{mod.rs,lifecycle.rs,update.rs,menu.rs,ctx.rs},
src/systems/{load_assets.rs,window_resize.rs,quit.rs}, CLAUDE.md, docs/gotchas.md. Engine source (read-only):
/home/idaho/Projects/aberredengine. NOTE aberrededitor's docs are partly stale: GUI callbacks are
`fn(&mut GuiCtx)` (fields ui, signals, intents, textures, fonts, app_state), not six parameters.

FILES
1. Cargo.toml: edition 2024; dependencies: `aberredengine = { git = "https://github.com/idaho06/aberredengine.git",
   branch = "develop-0.2.0", default-features = false }`, log 0.4, env_logger 0.11, serde (derive) 1, serde_json 1.0,
   rfd 0.17, futures 0.3, crossbeam-channel 0.5, rustc-hash 2.1. (If the build can only use the local checkout,
   STOP and report instead of adding a path dependency.)
2. config.ini (compiled in with include_str!): [render] width=1280 height=720 background_color=24,26,32
   pixel_snap_camera=false; [window] width=1280 height=720 title=Aberred Dialogue Editor target_fps=60 vsync=true
   fullscreen=false; [simulation] hz=60; [audio] hz=60. Check the real key names in
   aberrededitor/config.ini and the engine's game_config.rs; mirror them exactly.
3. src/main.rs: env_logger (default filter "info"); EngineBuilder::new().config_str(include_str!("../config.ini"))
   .on_setup(setup::load_state).add_observer(window_resize observer).add_scene("editor")
   .on_scene_enter("editor", scenes::editor::enter).add_scene_gui("editor", scenes::editor::gui)
   .initial_scene("editor").try_run(). No intro scene.
4. src/state.rs: `pub struct UiCapture { pub mouse: AtomicBool, pub keyboard: AtomicBool }` (Default) and type
   aliases `pub type UiCaptureHandle = Arc<UiCapture>;`, `pub type WorkspaceMutex = Arc<Mutex<Workspace>>;` where
   `Workspace` is a placeholder `#[derive(Default)] pub struct Workspace { pub docs: Vec<()>, pub active: usize }`
   in src/model/mod.rs (the real model arrives in the next step). Both handles are inserted into `AppState` ONCE in
   the `on_setup` system (`setup::load_state(mut app_state: ResMut<AppState>)`). `AppState::insert` needs T: Clone;
   Arc<..> is Clone.
5. src/systems/window_resize.rs: copy aberrededitor's `on_window_resized` observer (calls
   `config.set_render_size(w,h)` and sets `camera.0.offset = (w/2, h/2)`); ignore sizes <= 0.
6. src/scenes/mod.rs + src/scenes/editor/{mod.rs,lifecycle.rs,gui.rs,menu.rs}:
   - `enter(_: On<SceneEntered>, mut commands: Commands, ...)`: insert
     `MainCamera(Camera2D::new(Vec2::ZERO, Vec2::new(w/2, h/2)))` using the GameConfig render size; set
     `ResMut<CameraFollowConfig>.enabled = false`; rebind `InputAction::Action2` to RIGHT mouse only via
     `ResMut<InputBindings>` (copy how aberrededitor/src/scenes/editor/lifecycle.rs rebinds Action1; read
     crates/aberred-core/src/resources/input_bindings.rs for `rebind` and the mouse-button binding variant).
   - `gui(ctx: &mut GuiCtx)`: publish capture flags every frame:
     `capture.mouse.store(ctx.ui.io().want_capture_mouse, Relaxed)` and keyboard likewise (fetch the
     UiCapture from `ctx.app_state.get::<UiCaptureHandle>()`; look at how aberrededitor reads its mutex caches).
     Then draw a main menu bar (`ui.begin_main_menu_bar()`): File (New, Open..., Save, Save As..., Close Tab,
     separator, Quit), Edit (Add node), View (Re-layout, Frame all). All items except Quit are DISABLED
     placeholders (`menu_item_config(..).enabled(false)`). Quit calls `ctx.intents.request_quit()`.
7. justfile with recipes: `check` (fmt --check, clippy --all-targets -- -D warnings, test), `test` (cargo test),
   `lint` (cargo clippy --all-targets -- -D warnings), `run` (cargo run), `fmt` (cargo fmt). Default recipe = check.
8. CLAUDE.md for this project: short. Commands (just recipes), the policies summary (commit policy, vibe policy
   from docs/plans/00-overview.md), and a gotchas list: GUI callbacks are fn pointers; GUI runs on the render
   thread, systems on the logic thread; AppState Arc<Mutex> sharing; B0002; render-only resources panic at
   startup and cargo check cannot detect it; the engine does not mask input for scene GUIs (use UiCapture); read
   the plan files in docs/plans/ before changing architecture.
CONSTRAINTS: no model, no file dialogs, no graph drawing in this step. Do not modify docs/.
DONE WHEN: `just check` passes (there may be zero tests; that is fine) and `cargo run` opens a window showing the
menu bar, File > Quit exits. Report any deviation from the file list above.
```

**Claude review checklist:** UiCapture inserted once; no `ResMut<AppState>` duplication; Quit uses
`request_quit`; Action2 rebound to right mouse only; `justfile` recipes work.

---

## Step 2 — Model and conversion

**Executor:** vibe. **Scope:** `src/model/**`, `tests/fixtures/**`, `src/main.rs` (only `mod model;`),
`src/state.rs` (replace the placeholder `Workspace` import).
**Acceptance:** `just check`. **Suggested message:** `Add dialogue document model with DialogueDef conversion`

**Vibe spec (pass verbatim):**
```
Project: /home/idaho/Projects/aberreddialogueeditor (Rust, edition 2024; engine = aberredengine git dependency,
branch develop-0.2.0, local reference checkout at /home/idaho/Projects/aberredengine - read-only). Read CLAUDE.md
and docs/plans/00-overview.md ("Architecture" and "Engine facts") first.

GOAL: the pure editor data model and its conversion to/from the engine's `DialogueDef`. No ECS, no imgui, no
window: everything must be unit-testable.

Allowed engine imports in src/model/**: `aberredengine::prelude::{DialogueDef, NodeDef, ChoiceDef,
DialogueCondition, DialogueCmp, DialogueEffect, Vec2}` (check exact paths in
/home/idaho/Projects/aberredengine/crates/aberred-core/src/resources/dialogue_def.rs and crates/aberredengine/src/
prelude.rs; Vec2 is re-exported from the prelude).

TYPES (src/model/doc.rs; derive Debug, Clone, PartialEq where possible)
 - `NodeKey(pub u64)` (Copy, Eq, Hash, Ord).
 - `Link { End, Node(NodeKey), Dangling(String) }`.
 - `EdChoice { text: String, target: Link, when: Option<DialogueCondition>, effects: Vec<DialogueEffect>,
   labels: Vec<String> }`.
 - `Outputs { Next(Link), Choices(Vec<EdChoice>) }` (Next(Link::End) is a terminal node).
 - `EdNode { key: NodeKey, id: String, speaker: Option<String>, text: String, labels: Vec<String>,
   on_enter: Vec<DialogueEffect>, out: Outputs, pos: Vec2 }`.
 - `CamState { target: Vec2, zoom: f32 }` (Default: ZERO, 1.0).
 - `DialogueDoc { nodes: Vec<EdNode>, start: Option<NodeKey>, path: Option<std::path::PathBuf>, dirty: bool,
   revision: u64, selected: Option<NodeKey>, camera: CamState, load_warnings: Vec<String>, next_key: u64 }`
   (Default; plus `DialogueDoc::new_empty()`).
 - `Workspace { docs: Vec<DialogueDoc>, active: usize }` (Default) with `active_doc()/active_doc_mut()` returning
   Option, `add_doc`, `close_doc(index)` (keeps `active` valid), `find_by_path`.

MUTATION API on DialogueDoc (EVERY mutator bumps `revision` and sets `dirty = true`; pure queries do not):
 `node(key) -> Option<&EdNode>` (read-only; there is NO public `&mut EdNode` accessor, expose the specific
 mutators below instead):
 `add_node(pos: Vec2) -> NodeKey` (unique id "node_N": smallest N >= 1 not used by any existing id; the first node
 added to a doc with no start becomes `start`), `delete_node(key)` (every Link::Node(key) in other nodes becomes
 Link::End; clears `start` if it was this node; clears `selected`), `set_start(key)`, `rename_node(key, id)` (no
 uniqueness check here - the linter reports duplicates), `set_node_pos(key, pos)` (bumps revision but does NOT set
 dirty: positions are never saved, so moving a node is not a content change),
 `set_text`, `set_speaker(Option<String>)`, `set_labels`, `set_on_enter`, `set_outputs(key, Outputs)`,
 `set_link(key, slot: Slot, Link)` where `Slot { Next, Choice(usize) }`, `add_choice(key)` (converting a
 Next node to Choices with one empty choice), `delete_choice(key, i)` (deleting the last choice converts the node
 to `Outputs::Next(Link::End)`), `move_choice(key, i, delta: i32)` (clamped, no-op at the ends),
 `set_choice_text/when/effects/labels`, `select(Option<NodeKey>)` (bumps revision, not dirty), `mark_clean()`.
 `fresh_key()` increments `next_key`.

CONVERSION (src/model/convert.rs)
 - `from_def(def: &DialogueDef) -> DialogueDoc`: tolerant, never fails.
   * one EdNode per NodeDef in order, keys assigned sequentially; a lookup map id -> key resolving duplicate ids to
     the FIRST node with that id in node order;
   * `next`/choice `next` strings become `Link::Node(key)` if resolvable, `Link::Dangling(s)` otherwise, `None` ->
     `Link::End`;
   * `start`: resolves like a link; unresolvable start -> `start = None` plus a load warning;
   * node with BOTH `next` and `choices`: keep the choices, drop `next`, add a load warning naming the node;
   * `choices: Some(vec![])`: becomes `Outputs::Next(Link::End)` plus a load warning;
   * `choices: None` -> `Outputs::Next(link from next)`;
   * positions all `Vec2::ZERO` (auto-layout comes later), `dirty = false`, `revision = 0`.
 - `to_def(doc: &DialogueDoc) -> DialogueDef`: total, exact. `start: None` -> `start: String::new()` (the linter
   flags it as StartMissing). `Link::Node(k)` -> that node's id (a key that no longer exists -> treated as End);
   `Link::Dangling(s)` -> `Some(s)` (keeps the broken reference visible to the engine's own validation);
   `Outputs::Next(l)` -> `next`, `choices: None`; `Outputs::Choices(v)` -> `choices: Some(v)` and `next: None`
   (an empty Vec never occurs because `delete_choice` converts; if one slips through, emit `Outputs::Next(End)`
   semantics, i.e. `choices: None`). The result must NEVER set both `next` and `choices`, and never emit
   `choices: Some(vec![])`.
 - Round trip semantic equality: for every valid fixture, `to_def(&from_def(&def)) == def` (serde field order and
   skipped-empty fields already normalise through DialogueDef itself; compare DialogueDef values, not text).

FIXTURES (tests/fixtures/): `guard.json` (the example from RUST-GAME-GUIDE.md section "The same conversation as
JSON": start "greet", nodes greet/friend/pass; the exact JSON is in
/home/idaho/Projects/aberredengine/RUST-GAME-GUIDE.md, the JSON block right under that heading), `loops.json` (a loop A->B->A and a
join, plus conditions of every type incl. nested all/any/not and every effect type), `invalid_dangling.json`,
`invalid_dup_id.json`, `invalid_both_next_choices.json`, `invalid_empty_choices.json`, `invalid_no_start.json`.

TESTS (write first; src/model tests or tests/model.rs):
 - round trip for guard.json and loops.json;
 - each invalid_* fixture loads without panic with the expected load_warnings / Dangling links;
 - duplicate id resolution picks the first node;
 - `add_node` id uniqueness (fills gaps), first node becomes start;
 - `delete_node` rewrites incoming links to End and clears start/selected;
 - `delete_choice` of the last choice converts to Next(End); `move_choice` clamps;
 - every mutator bumps `revision`; `set_node_pos`/`select` do not set `dirty` (all others do);
 - `to_def` never produces next+choices or Some(empty) for any generated model (a small loop over mutator
   sequences is enough).
CONSTRAINTS: no engine systems/imgui; do not touch config, justfile or docs. Keep `Workspace` re-exported from
src/model/mod.rs and update src/state.rs to use the real type (the placeholder is replaced).
DONE WHEN: `just check` passes. Report the first failing test written.
```

**Claude review checklist:** `to_def` invariants hold; duplicate-id rule is "first wins"; no engine types other than
the allowed ones in `src/model`; `revision`/`dirty` semantics match the spec; `set_node_pos` decision documented.

---

## Step 3 — Linter

**Executor:** vibe. **Scope:** `src/model/lint.rs`, `src/model/mod.rs`, `src/model/doc.rs` (only the
`lint_cache` field + accessor), `tests/lint.rs`, `tests/fixtures/**`.
**Acceptance:** `just check`. **Suggested message:** `Add dialogue linter with engine-parity tests`

**Vibe spec (pass verbatim):**
```
Project: /home/idaho/Projects/aberreddialogueeditor. Read CLAUDE.md and docs/plans/00-overview.md first. The model
(src/model/doc.rs, convert.rs) already exists and is committed.

GOAL: a live linter reporting ALL problems of a DialogueDoc, plus parity tests against the engine's own validation.

API (src/model/lint.rs)
 - `Severity { Error, Warning }`, `ProblemKind { StartMissing, EmptyId, DuplicateId, DanglingTarget,
   Unreachable }`, `Problem { severity, kind, node: Option<NodeKey>, message: String }`.
 - `lint(doc: &DialogueDoc) -> Vec<Problem>`, deterministic order (by node order, then kind).
 - Errors: StartMissing (start None, or start key no longer present); EmptyId (a node id is empty or all
   whitespace); DuplicateId (reported for EVERY node whose id is shared; message names the id); DanglingTarget
   (any `Link::Dangling`, in next or any choice; message names node and target, mirroring the engine's
   `dialogue node "{node}" links to unknown node "{target}"`).
 - Warning: Unreachable (not reachable from `start` by BFS over Link::Node edges; skipped when start is missing,
   to avoid a flood of warnings).
 - `DialogueDoc::problems(&mut self) -> &[Problem]` backed by `lint_cache: (u64, Vec<Problem>)`: recomputed only
   when `revision` differs from the cached revision. Add the field to DialogueDoc (Default (u64::MAX or an
   explicit "stale" marker so the first call computes)).
 - `has_errors(&[Problem]) -> bool`.
 - `engine_gate(doc: &DialogueDoc) -> Result<(), DialogueError>`: `DialogueTree::try_from(to_def(doc)).map(|_| ())`
   (this is the save gate; never reimplement the engine rules).

PARITY TESTS (tests/lint.rs; these guard against drift with the engine; write them FIRST)
 1. SOUNDNESS: for all fixtures plus a generated family of mutations (apply sequences of mutators: rename to
    duplicate, delete node, add dangling link, clear start, empty id, ...): if `lint` returns no Error then
    `engine_gate(doc)` is Ok. The reverse is NOT required (EmptyId is stricter than the engine; the engine accepts
    an empty id).
 2. COVERAGE: for each engine validation error reachable from the model - StartMissing, DuplicateNode,
    UnknownTarget - build a doc that triggers it and assert (a) engine_gate returns that exact `DialogueError`
    variant and (b) lint reports the matching ProblemKind as an Error.
 3. NextAndChoices and EmptyChoices are unrepresentable in the model: assert `to_def` never emits them (a test over
    fixtures + mutation sequences, checking the produced DialogueDef directly).
 4. Unreachable is a warning only: a doc with an unreachable node passes engine_gate and lint has no Error.
Other tests: BFS handles cycles/self-loops; lint cache recompute only on revision change (count via a test hook or
by mutating the cached vector and checking it survives a second call with unchanged revision).
CONSTRAINTS: no imgui/ECS. Only the listed scope. DONE WHEN: `just check` passes. Report the first failing test.
```

**Claude review checklist:** the soundness direction is one-way as stated; coverage test asserts the exact engine
variant; `engine_gate` uses the engine, not a copy.

---

## Step 4 — Workspace operations, file dialogs, tabs (two chained vibe jobs)

### 4a — Operations layer (pure, tested)
**Executor:** vibe. **Scope:** `src/ops/**`, `src/main.rs` (`mod ops;`), `tests/ops.rs`.
**Acceptance:** `just check`. **Suggested message:** `Add open/save operations with engine save gate`

**Vibe spec (pass verbatim):**
```
Project: /home/idaho/Projects/aberreddialogueeditor. Read CLAUDE.md and docs/plans/00-overview.md first. Model,
lint and `engine_gate` exist and are committed (src/model/**).

GOAL: file operations on the workspace as plain functions (no imgui, no engine ECS), fully tested.

src/ops/mod.rs:
 - `open_path(ws: &mut Workspace, path: &Path) -> Result<OpenOutcome, OpError>`: if a doc with the same canonical
   path is already open, make it active and return `OpenOutcome::Focused`; otherwise read the file, parse it with
   `serde_json::from_str::<DialogueDef>`, `from_def`, set `doc.path = Some(path)`, `dirty = false`, push it,
   activate it, return `OpenOutcome::Opened { warnings }`. Errors: `OpError::Io(String)`,
   `OpError::Json(String)` (Display text must include the path and the parse error).
 - `save_doc(doc: &mut DialogueDoc, path: &Path) -> Result<(), OpError>`: FIRST `engine_gate(doc)`; on failure
   return `OpError::Invalid(DialogueError display string)` WITHOUT writing anything; on success write
   `serde_json::to_string_pretty(&to_def(doc))` + trailing newline atomically (write to `<path>.tmp` in the same
   directory then rename), set `doc.path = Some(path)`, `dirty = false`.
 - `new_doc(ws)`: pushes a `DialogueDoc::new_empty()` and activates it. Title helper `doc_title(&DialogueDoc) ->
   String` ("Untitled" or the file name; no dirty marker here).
 - `close_doc(ws, index)` wraps the model method.
TESTS: open guard.json -> save to a temp dir (std::env::temp_dir() + unique name) -> reload -> `to_def`
equal to the original DialogueDef; the saved file passes `aberredengine::prelude::load_dialogue(path)`; saving a
doc with a dangling link / duplicate id / no start returns `OpError::Invalid` with the engine's message and leaves
no file; opening the same path twice focuses; invalid JSON yields `OpError::Json`; atomic write leaves no `.tmp`.
CONSTRAINTS: scope only; no UI. DONE WHEN: `just check` passes. Report the first failing test.
```

### 4b — File dialogs, tabs, modals, node list (UI)
**Executor:** vibe (`base_job` = 4a job, or start after the user committed 4a). **Scope:** `src/systems/
file_dialogs.rs`, `src/scenes/editor/**`, `src/state.rs`, `src/main.rs`, `Cargo.toml` (only if a dep is missing).
**Acceptance:** `just check`; then Claude runs `just run` and verifies manually: File → Open loads `guard.json` in
a tab; a second Open of the same file focuses it; New creates "Untitled"; Save As writes a file that
`load_dialogue` accepts; closing a dirty tab asks to confirm; the Nodes panel lists node ids.
**Suggested message:** `Add file dialogs, document tabs, confirm modals and node list`

**Vibe spec (pass verbatim):**
```
Project: /home/idaho/Projects/aberreddialogueeditor. Read CLAUDE.md and docs/plans/00-overview.md first. Existing:
app shell with disabled menu items (src/scenes/editor/{gui,menu,lifecycle}.rs), model (src/model/**), operations
(src/ops: open_path, save_doc, new_doc, close_doc, doc_title). WorkspaceMutex and UiCaptureHandle live in AppState.

GOAL: make File menu, tabs and modals work (no graph drawing yet).

1. src/systems/file_dialogs.rs: copy and ADAPT /home/idaho/Projects/aberrededitor/src/systems/file_dialogs.rs (read
   it fully, including its doc comments and the `drain_completed` logic). Requests: `OpenDialogue`,
   `SaveDialogueAs`. Results carry an ABSOLUTE `PathBuf` (do NOT make paths relative: delete `to_relative`).
   Keep: at most one dialog in flight; `rfd::AsyncFileDialog` built on the caller's thread, awaited with
   `futures::executor::block_on` in a spawned thread, result via `crossbeam_channel`; the receiver state lives in
   an `Arc<Mutex<..>>` inside AppState (insert once in `on_setup`). Filters: "Dialogue JSON" (json). Because the
   result must reach the model, replace the observer-event dispatch with a direct application: a logic system
   `poll_async_dialogs` (add_system) drains the receiver and calls `ops::open_path` / `ops::save_doc` on the
   locked workspace, pushing user-visible errors into `Workspace::notifications` (see below). The GUI calls
   `request_async_dialog(&AppState, req)`. Cancelling a dialog is a no-op.
2. Workspace additions (src/model/mod.rs, not the doc): `Workspace.notifications: Vec<String>` (error modal queue)
   and `pending_close: Option<usize>`, `pending_quit: bool`.
3. GUI (src/scenes/editor/{gui,menu,tabs,modals,node_list}.rs):
   - File menu: New (Ctrl+N), Open... (Ctrl+O), Save (Ctrl+S), Save As... (Ctrl+Shift+S), Close Tab (Ctrl+W),
     Quit (Ctrl+Q). Shortcuts are read with `ui.is_key_pressed`/`io().key_ctrl` and ONLY when
     `!ui.io().want_text_input`. Save with no path behaves like Save As. Save As requests the dialog; when the
     result arrives the poll system saves. A save that fails the engine gate pushes the error text
     (`dialogue node "x" links to unknown node "y"` etc.) into notifications.
   - Tab bar below the menu (`ui.tab_bar`, `tab_item_with_opened`) with the doc title and a `*` suffix when dirty;
     clicking a tab sets `workspace.active`; the tab close button sets `pending_close`.
     Use `push_id`/`##index` suffixes so equal titles do not collide.
   - Modals (modal_popup_config, opened after the menu scope like aberrededitor's menu.rs/update.rs pattern):
     "Unsaved changes" for closing a dirty tab (Save / Discard / Cancel) and for quitting with any dirty doc
     (Save all is out of scope: offer Discard and quit / Cancel); "Error" modal draining notifications one by one.
   - A "Nodes" window (dockable-free simple window): lists the active doc's node ids (selectable; clicking calls
     `doc.select(Some(key))`); shows `load_warnings` of the active doc above the list in a collapsible header.
   - Keep each lock short: lock the WorkspaceMutex, copy what you need (titles, ids), unlock before drawing
     widgets that call ops; never hold the lock while calling `request_async_dialog` or `rfd`.
4. Quit: Quit item and window-close path both go through the dirty check before `intents.request_quit()`.
CONSTRAINTS: no graph drawing, no inspector. Do not change src/model semantics (only add the Workspace fields).
Do not use `ResMut<AppState>` twice in one system. DONE WHEN: `just check` passes (add tests for the pure parts:
title formatting, the dirty-quit decision function, drain_completed with an injected channel). Report any
decision you had to make that this spec did not cover.
```

**Claude review checklist:** no `to_relative`; absolute paths kept; locks not held across rfd calls; shortcuts
gated on `!want_text_input`; quit dirty-check exists; B0002 not triggered (run the app, it panics at startup if so).

---

## End of plan 02 checklist
- [ ] `git init` done with the user's confirmation; initial commit by the user
- [ ] Steps 1b, 2, 3, 4a, 4b each committed by the user (small commits)
- [ ] `just check` green; `just run` verified manually (open/new/save/tabs/modals)
- [ ] saved files verified with `load_dialogue` (test) — plan 03 can start once plan 01 is pushed

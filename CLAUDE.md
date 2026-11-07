# AberredDialogueEditor

Node-graph editor for AberredEngine dialogue JSON. Rust binary crate on the `aberredengine` git dependency
(branch `develop-0.2.0`, `default-features = false`, pinned in `Cargo.lock`). See `README.md` for what the tool
does and how to run it; `docs/architecture.md` for the architecture and "how to add ..." recipes;
`docs/gotchas.md` for the engine/imgui/threading facts in full (symptom + rule); `docs/plans/` for the
implementation plans. This file is the fast reference — read those docs before changing architecture.

## Commands

All acceptance commands are `just` recipes (run from the repo root):

| Recipe      | What it runs                                                        |
|-------------|---------------------------------------------------------------------|
| `just check`| `cargo fmt --check` + `cargo clippy --all-targets -- -D warnings` + `cargo test` (default recipe) |
| `just test` | `cargo test`                                                        |
| `just lint` | `cargo clippy --all-targets -- -D warnings`                         |
| `just run`  | `cargo run`                                                         |
| `just fmt`  | `cargo fmt`                                                         |

Debug logging at run time: `RUST_LOG=aberreddialogueeditor=debug just run` (default level is `info`, set in
`src/main.rs`).

## Policies

- **Commit policy:** small, meaningful commits. Claude stages exactly the files that belong to a step
  (`git add <explicit paths>`, never blind `git add -A`), proposes a one-line commit message with no
  `Co-Authored-By` trailer, and stops. The user runs `git commit`. Claude never runs `git commit` or `git push`.
- **Vibe policy:** implementation, code fixes, test creation, simplification and code review go through the
  vibe MCP (`vibe_submit`/`vibe_wait`/`vibe_result(diff=true)`/`vibe_apply`/`vibe_cancel`). Claude does reasoning,
  planning, specs, diff review, running the acceptance recipes and debugging. If the vibe MCP is not connected,
  STOP and tell the user — do not silently implement.

## Gotchas

Full detail in `docs/gotchas.md`. The rules that prevent bugs:

- GUI callbacks are plain `fn` pointers (`fn(&mut GuiCtx)` with fields `ui`, `signals`, `intents`, `textures`,
  `fonts`, `app_state`) — no captures (per-frame GUI state lives in `thread_local!`s). They run on the
  **render thread**; systems/observers run on the **logic thread**. Shared state only through `AppState`
  handles (`Arc<Mutex<T>>` inserted once in `on_setup`: `WorkspaceMutex`, `UiCaptureHandle`,
  `AsyncFileDialogMutex`) — the render clone aliases the same `Arc`s.
- B0002: never request the same resource twice in one system — panics at startup, invisible to `cargo check`.
  Systems take `Res<AppState>` only, never `ResMut`, never twice.
- Render-only resources (`TextureStore`, `FontStore`, `ShaderStore`, `RaylibAccess`) panic at schedule init in
  logic systems/observers — use `ctx.textures`/`ctx.fonts` in GUI callbacks only.
- The engine does NOT mask input for scene GUIs: the GUI publishes `want_capture_mouse`/`want_capture_keyboard`
  into the shared `UiCapture` atomics every frame; logic input systems gate on them.
- `InputState::mouse_world_*` lags the camera by one tick — compute mouse world from the doc camera via
  `screen_to_world` (`graph::build_sample`), never pan with `mouse_world_*`.
- `Action2` defaults to Enter OR right mouse; the editor rebinds it to right mouse only at scene enter
  (`Action3` = middle mouse).
- Font metrics arrive asynchronously (`FontMetricsStore::resolve_key` + `PendingAssets`); `metrics_ready` is
  part of the node signature, so nodes respawn without/with texts when metrics land.
- Children of graph entities must be spawned WITH a `GlobalTransform2D` (`child_global`), or they flash at the
  origin for one frame; root entities must NOT get one.
- imgui ids never depend on edited text, labels, dirty state or shifting indices: tab ids `###doc{doc_id}`,
  choice headers `###choice{i}`, fixed `###` section suffixes, `push_id_usize` per row.
- imgui's `p_open` does not hide a window — draw nothing when hidden (`problems::should_draw`).
- On the `SET_SELECTED` frame imgui still reports the old tab as selected, and a clicked X re-selects a
  neighbour — `tabs.rs` ignores stale reports and re-asserts the real active tab.
- `set_link` marks the doc dirty even when unchanged — the link detach is deferred until the pointer moves
  > 4 px (`interaction.rs`), so a click on a linked pin mutates nothing.
- Never hold the `WorkspaceMutex` across imgui widget calls, `request_async_dialog`/`dialog_in_flight` or any
  rfd code: snapshot under a short lock, draw with the guard dropped, apply with a fresh lock.
- Never log document content (no node text/speaker/whole files) — see Logging.
- The lint cache is keyed on `content_revision`, not `revision`.
- Engine imports go through `aberredengine::prelude::{...}`; `aberredengine::core::...` only for what the
  prelude lacks (`events::window::WindowResized`, `resources::input_bindings::InputBindings`,
  `components::global_transform2d::GlobalTransform2D`, `resources::font_metrics::*`). Do not add `bevy_ecs` as a
  dependency — the facade re-exports it (`use aberredengine::bevy_ecs;` where derive macros need the path).

## Model and lint

- `src/model` is pure: plain data and functions, no imgui/ECS/window. Its only engine imports are
  `aberredengine::prelude::{DialogueDef, NodeDef, ChoiceDef, DialogueCondition, DialogueCmp, DialogueEffect,
  DialogueTree, DialogueError, Vec2}`.
- `Workspace` is the single source of truth (`docs/architecture.md` has the full shape) and also carries the
  typed modal queue `notifications: Vec<Notification>` (`Notification`/`NotificationKind` in
  `src/model/notify.rs`), `problems_hidden` (the INVERSE visibility flag of the Problems window — the derived
  `Default` = visible), `pending_close`/`pending_quit`/`show_controls`/`pending_context_menu`.
- Every mutation on `DialogueDoc` goes through a named mutator; there is no public `&mut EdNode` accessor.
  Every mutator bumps `revision`; content mutators also bump `content_revision` and set `dirty`;
  `set_node_pos`, `select` and `mark_clean` only bump `revision` (view-only, never saved); `set_camera` bumps
  nothing at all (pure view state — must not invalidate the lint cache or trigger a projection). No-op calls
  (unknown key, inapplicable slot) do not bump.
- Two revision counters: `revision` is bumped by EVERY mutator (the graph-projection key); `content_revision`
  only by content mutators — the lint cache (`DialogueDoc::problems`) is keyed on it, so
  dragging/selecting/camera moves never re-run the linter.
- `from_def`/`to_def` convert to/from the engine's `DialogueDef`, only at load/save. `from_def` is tolerant
  (dangling refs -> `Link::Dangling`, load warnings); `to_def` is total and never emits both `next` and
  `choices` or `choices: Some(vec![])`.
- `lint::lint` reports ALL problems of a doc (cached per `content_revision` via `DialogueDoc::problems`);
  `lint::engine_gate` (`DialogueTree::try_from(to_def(doc))`) is the save gate — engine rules are never
  reimplemented in the model.

## Ops

- `src/ops` is the file-operation layer: plain functions over `Workspace`/`DialogueDoc` — no imgui, no engine ECS.
- `open_path` canonicalizes both sides before matching, so any spelling of an already-open file focuses the
  existing tab (`Focused` never re-layouts; `Opened` runs `auto_layout`).
- `save_doc` runs `engine_gate` first (a rejected doc writes nothing, not even a `.tmp`) and saves atomically
  via `<path>.tmp` + rename: `serde_json::to_string_pretty(&to_def(doc))` + trailing newline.
- `check_saveable` is the pre-check run BEFORE any Save As dialog opens (and by the close modal's Save on an
  untitled doc): the same predicate and message as `save_doc`'s gate (`engine_gate` mapped to
  `OpError::Invalid`), so it can never disagree with the real save. Pure — it mutates nothing and writes
  nothing; a rejection pushes a `SaveRejected` notification instead of opening the picker. The final gate
  inside `save_doc`/the Save As result path stays (the doc can be edited while the picker is open).
- Doc paths are stored canonical/absolute by `open_path`/`save_doc`; `doc_title` is the file name or "Untitled".

## Graph

- `src/graph` is pure logic except `components.rs`, `projection.rs`, `camera_system.rs`, `input_system.rs`,
  `preview.rs` (the ECS files); engine imports in the pure files are limited to `Vec2` and `Color`.
- **Pure modules:** `style.rs` (numeric constants, palette, Z indices), `geometry.rs` (`rows`/`node_height`/
  `node_rect`, pins, `slot_at`, `hit_test` — pins beat bodies, later node in `doc.nodes` wins, `edge_curve`
  control points RELATIVE to the source pin, `arrow_tip_dir`, `graph_bounds`), `wrap.rs` (char-safe
  `wrap_text` with `"..."` truncation, `fit_line`), `camera.rs` (`screen_to_world`/`world_to_screen`,
  `zoom_at` clamp [0.1, 4.0], `pan_by_screen_delta`, `center_on`, `frame_all` clamp [0.1, 1.0], `None` ->
  ZERO/1.0), `layout.rs` (`auto_layout`: BFS-depth layers from start, back edges ignored, unreachable nodes in
  trailing columns, barycenter ordering, columns at `layer * (NODE_W + COL_GAP)` centred on y = 0; writes via
  `set_node_pos`, so it never dirties).
- **Projection:** `describe.rs` turns an `EdNode` into a `NodeVisual` (texts/pins local to the node top-left);
  `projection_plan.rs` snapshots a doc (`snapshot_of`, carrying `doc_id`/`revision`/`metrics_ready`) and diffs
  against the remembered `Snapshot` (`plan_projection` -> `ProjOp`s; edges are rewritten, never respawned, when
  a node moves). `projection.rs` `graph_projection_system` (`add_late_system`, `GraphEntities` inserted at scene
  enter) locks the workspace only to snapshot + clone spawn data, unlocks before any Commands/query call, and
  returns early when the `(doc_id, revision, metrics_ready)` triple is unchanged. Nodes spawn as a root
  (`GraphNode`, `WorldPosition`, `ZIndex`) with `GraphPart` children (body `rounded_rect` — thick outline only
  when selected/error, header `rect`, `DynamicText` items once metrics are ready, pin circles — `Shape::circle`
  is centred on its local origin); edges are bezier entities at the source pin with one arrowhead child.
  `projection: N nodes, M edges` logs at debug only for STRUCTURAL plans (`is_structural`).
- **Camera/input:** `camera_system.rs` `graph_frame_system` (`add_system`) is the single frame system, one
  workspace lock per tick, fixed order: wheel zoom (`apply_wheel` = `zoom_at` with `1.1^scroll_y` around the
  cursor) + middle-mouse pan (`Action3`, `pan_step` accumulating screen-pixel deltas in a `Local`, never
  `mouse_world_*`) -> `build_sample` (mouse world from the doc camera AFTER wheel/pan) -> `step` +
  `apply_actions` -> preview/prev-mouse bookkeeping -> `MainCamera` write LAST (`Camera2D::new(target,
  viewport/2).with_zoom(zoom)`, viewport = `GameConfig` render size). Both are ignored while
  `UiCapture::mouse` is set, except a pan already in progress. No active doc: preview cleared, interaction
  reset, camera left alone. `frame.rs`: `frame_doc` = `set_camera(frame_all(graph_bounds(doc), viewport,
  40.0))`, used by `EditorAction::FrameAll` (View > "Frame all", `F`).
- **Interaction:** `interaction.rs` (pure) holds `GraphInteraction`/`Phase` (`Idle`/`DraggingNode`/
  `DraggingLink`/`RightDown`, exclusive) and `step(&mut state, &doc, &InputSample, prev_mouse_screen) ->
  Vec<Action>` + `drag_preview`. Pin pickup radius `(PIN_R + 4.0) / zoom`; `mouse_captured_by_ui` gates gesture
  STARTS only; a node vanishing mid-gesture falls back to `Idle` (no unwrap). Left press on a body/input pin
  selects + drags; on an output pin it carries the link (detach deferred > 4 px); release over any part of a
  node reconnects (self allowed), over empty space a detached link stays `End` (never creates a node), over
  the same pin a detached drag restores. Right press: > 4 px pans (`PanByScreenDelta`), else release emits
  `OpenContextMenu` + `Select`. `apply.rs` maps `Action`s onto the doc mutators and returns the
  `ContextMenuRequest`; link changes log `link set`/`link cleared` at debug (events only). `input_system.rs`
  holds the `GraphInput` resource (inserted at scene enter); `preview.rs` keeps ONE `GraphPreview` entity while
  a link is dragged.
- **Context menu / node actions:** `EditorAction` carries `AddNodeAt(Vec2)`/`AddNode{viewport}`/
  `DeleteNode(NodeKey)`/`SetStart(NodeKey)`/`DeleteSelected`, all applied in the single `apply_action`.
  `context_menu.rs` drains `Workspace.pending_context_menu` under a short lock and opens the `graph_ctx` popup
  exactly once per request (the request lives in a `thread_local!`); `items_for` is pure: node hit -> "Set as
  start"/"Delete node"/"Add node here", empty -> only "Add node here".

## GUI

- Every user intent (menu item, Ctrl shortcut, tab close button, graph context-menu item, Problems row) is an
  `EditorAction`, applied in ONE place: `scenes/editor::actions::apply_action` (pure over `Workspace`; native
  dialog requests go through its `Option<&AppState>` parameter — the GUI passes `None` under the lock and
  dispatches the returned request after). `gui.rs` collects the frame's actions and applies them; while a
  native dialog is in flight, `CloseTab` actions are skipped (a pending Save As is keyed by the doc's unique,
  never-reused `doc_id` — closing/shifting tabs while the picker is open cannot redirect the save; the
  unsaved-changes modal's Save on an untitled doc requests the picker with `close_after`, so the tab closes
  after a successful save).
  Save As — and Save on an untitled doc, which falls back to Save As — runs `ops::check_saveable` first: a
  doc the engine would reject pushes a SaveRejected notification and `fx.dialog` stays `None`, so no picker
  opens.
- Lock discipline: lock `WorkspaceMutex` only for short snapshots and per action, never while calling
  `request_async_dialog`/`dialog_in_flight` or any rfd code; widgets are drawn with the guard dropped.
- Shortcuts (`shortcuts.rs`) are read with `ui.is_key_pressed(Key::..)` + `io().key_ctrl`/`key_shift`, only when
  `!io().want_text_input`; `F` does not fire while Ctrl is held; shortcuts are not polled while a modal is open
  (`gui.rs` skips `shortcuts::poll` for that frame when `Workspace::has_modal()` — imgui modals do not block
  io-level key polling). Modals (notification queue, unsaved-changes close/quit, Help > Controls) open from the
  frame's top-level scope after the menu, one at a time: notification first, Controls last (`modals.rs`;
  `apply_choice_ws` is the pure core).
- Tabs (`tabs.rs`): ids `###doc{doc_id}` (`actions::tab_label`) so a dirty star or a closed tab never changes a
  tab's identity; on the `SET_SELECTED` frame every tab's selected report is ignored (`clicked_focus`); a
  clicked X drops imgui's neighbour re-selection and re-asserts the real active tab next frame
  (`resolve_focus`).
- Windows: `nodes.rs` (Nodes list + load warnings), `inspector/` (see below), `problems.rs` (Problems window,
  visible by default — the flag is `problems_hidden`; errors first via `grouped_rows`, row click ->
  `FocusNode`; a hidden window is not drawn at all), `status.rs` (bottom bar: path/Untitled + `*` when dirty,
  "E errors / W warnings" coloured, node count, zoom %), plus menu/shortcuts/modals/context menu.

## Inspector

- `inspector/mod.rs` draws the "Inspector" window (plain imgui window, first-use position top-right) showing the
  SELECTED node of the ACTIVE doc ("Nothing selected"/"No document" otherwise). It holds the `Edit` enum and the
  PURE `apply_edits(doc, &edits)`, which applies edits in order through the doc mutators (a vanished NodeKey /
  out-of-range index is ignored: no panic, no bump).
- Inspector edits are applied by `apply_edits`, NOT through `apply_action` — `apply_action` logs `{action:?}`,
  which would leak node text. `apply_edits` logs only the edit KIND and the node key (`trace!` per-keystroke
  kinds, `debug!` structural), and the GUI applies collected edits only if the active doc's `doc_id` still
  matches the snapshot's.
- Per frame: lock briefly to snapshot the selected node (clone) + flags + `doc_id` + targets, unlock; draw
  widgets with the guard dropped, emitting an edit only when a widget reports a change; re-lock to apply. The
  lock is never held across widget calls.
- Widget ids come from the NodeKey (the panel body is `push_id(node{key})`) and from indices — never from the
  edited text, so typing never changes a widget id.
- `node_fields.rs`: id/speaker/text/labels/start + on-enter effects; `NodeFlags { is_start, duplicate_id,
  empty_id }` come from `doc.problems()`. `outputs.rs`: Next/Choices radios (`Edit::ChangeMode` — Next ->
  Choices builds ONE choice inheriting the next link, Choices -> Next keeps the FIRST choice's target; never
  leaves `Choices(vec![])`), per-choice sections `"Choice N: <truncated>###choice{i}"` with text/target/labels/
  up/down/delete/when/effects; `combo_entries` puts "(end)" first and disambiguates duplicate/empty ids with
  `"{id} (#{n})"`; a `Link::Dangling` preview shows red "unknown: {s}". `expr.rs`/`effects.rs`: the recursive
  condition editor and effects list editor work on a LOCAL clone and emit one whole-value edit; type switching
  is the pure `convert_condition`/`convert_effect`; every recursion level/row is `push_id_usize(..)`; empty keys
  are allowed (faint red "(empty key)" hint, not an error).

## Logging

- Default level is `info` (set in `main.rs`); configure at run time with `RUST_LOG`, e.g.
  `RUST_LOG=aberreddialogueeditor=debug just run`.
- Log on events only (an action applied, a dialog result, a file operation, a lint-cache recompute) — never
  inside per-frame GUI paths.
- What is logged where: file operations and outcomes in `ops`, dialog requests/results and the typed
  notifications they push (kind only — a SaveRejected message quotes node ids, so never its text) in
  `systems/file_dialogs.rs`, the inserted shared handles in `systems/setup.rs`, applied/ignored `EditorAction`s
  in `scenes/editor/actions.rs`, modal choices and outcomes in `scenes/editor/modals.rs`, `from_def` load
  warnings plus summary in `model/convert.rs`, the lint-cache recompute (content_revision, error/warning
  counts) in `model/doc.rs`, structural projection plans in `graph/projection.rs`, link set/cleared in
  `graph/apply.rs`.
- Never log document content: no node text, speaker text or whole file contents.

## Testing

- Tests live in `#[cfg(test)]` modules inside the source files: this is a binary crate, so `tests/` cannot
  import its modules (fixtures load via `include_str!(concat!(env!("CARGO_MANIFEST_DIR"), ...))`; shared
  helpers: `graph::fixtures`, `scenes::editor::testkit` with `fixture(...)` and `TempDir`).
- NEVER save over `tests/fixtures/*.json` — the tests load them; save tests write to a `TempDir`, and in the app
  use Save As to a copy.
- Engine-parity tests guard against drift: lint soundness (lint-clean implies `engine_gate` Ok over all fixtures
  + a generated mutation family) and coverage (each engine error variant maps to a lint error) in
  `model/lint.rs`; a save-path test proves a saved file passes `aberredengine::prelude::load_dialogue` in
  `ops/mod.rs`; the serde shape of every condition/effect variant is pinned in `inspector/` tests.

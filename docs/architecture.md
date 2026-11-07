# Architecture

AberredDialogueEditor is a single-scene AberredEngine app: a render-thread imgui GUI plus logic-thread ECS
systems, sharing one `Workspace` through an `Arc<Mutex<..>>`. Everything about dialogue content lives in a
pure Rust model that knows the engine's `DialogueDef` types and nothing else.

```
render thread (GUI callback fn(&mut GuiCtx))        shared (AppState, inserted once in on_setup)     logic thread (systems, Update / Late)
 imgui: menu, tabs, Nodes, Inspector,            ┌─ WorkspaceMutex = Arc<Mutex<Workspace>> ────────┐   graph_frame_system (camera+input, add_system)
 Problems, status bar, context menu, modals  ──► │  UiCapture = Arc<UiCapture> (AtomicBool x2)     │ ◄─►  poll_async_dialogs (add_system)
 publishes want_capture_mouse/keyboard ────────► │  AsyncFileDialogMutex = Arc<Mutex<..>>          │ ──►  graph_projection_system (add_late_system)
 every frame into UiCapture                       └─────────────────────────────────────────────────┘   graph_preview_system (add_late_system)
                                                                                                 window_resize observer
```

## Workspace model (`src/model`)

`src/model` is pure: plain data and functions, no ECS/imgui/window. Its only engine imports are
`aberredengine::prelude::{DialogueDef, NodeDef, ChoiceDef, DialogueCondition, DialogueCmp, DialogueEffect,
DialogueTree, DialogueError, Vec2}`.

- `Workspace { docs: Vec<DialogueDoc>, active: usize, next_doc_id, notifications, problems_hidden,
  pending_close, pending_quit, show_controls, pending_context_menu }` — the single source of truth.
- `DialogueDoc { doc_id, nodes: Vec<EdNode>, start: Option<NodeKey>, path, dirty, revision,
  content_revision, selected, camera: CamState, load_warnings, next_key, lint_cache }` — one tab.
  `EdNode { key: NodeKey, id, speaker, text, labels, on_enter, out: Outputs, pos: Vec2 }`;
  `Outputs::Next(Link) | Outputs::Choices(Vec<EdChoice>)`;
  `EdChoice { text, target: Link, when, effects, labels }`;
  `Link::End | Link::Node(NodeKey) | Link::Dangling(String)`.
- **Two revision counters.** Every mutator bumps `revision` (the graph-projection key). Content mutators
  additionally bump `content_revision` and set `dirty`; the view-only mutators `set_node_pos`, `select` and
  `mark_clean` bump only `revision`; `set_camera` bumps nothing at all (the camera is pure view state — it
  must not invalidate the lint cache or trigger a projection). A call that cannot apply (unknown key,
  inapplicable slot) is a no-op and does not bump. There is deliberately no public `&mut EdNode` accessor:
  every change goes through a named mutator so the bookkeeping stays in one place.
- `doc.problems()` is the lint cache: `lint_cache: Option<(content_revision, Vec<Problem>)>`, recomputed only
  when `content_revision` changed — dragging/selecting/camera moves never re-run the linter.
- `lint::lint` reports ALL problems of a doc (doc-level `StartMissing` first, then by node order, then by
  `ProblemKind`): errors `StartMissing`, `EmptyId`, `DuplicateId`, `DanglingTarget`; warning `Unreachable`
  (BFS from start; skipped when the start is missing). `lint::engine_gate(doc)` =
  `DialogueTree::try_from(to_def(doc))` is the save gate — engine rules are never reimplemented in the model.
- `from_def`/`to_def` (`src/model/convert.rs`) convert to/from the engine's `DialogueDef` and run **only at
  load/save**. `from_def` is tolerant (never fails; dangling refs become `Link::Dangling`, broken shapes get
  `load_warnings`); `to_def` is total and exact (never emits both `next` and `choices`, never
  `choices: Some(vec![])`; `start: None` becomes `""`).
- `doc_id` is unique per workspace (`Workspace::add_doc` assigns from 1; a bare doc is 0, and
  `doc_id == 0` with no content means "nothing shown" to the projection).

## Thread model

- The scene GUI is a plain `fn` pointer (`GuiCallback = fn(&mut GuiCtx)`, no captures) registered with
  `add_scene_gui("editor", scenes::editor::gui)`; it runs on the **render thread**. Systems and observers run
  on the **logic thread** (`add_system` / `add_late_system` / `add_observer` in `src/main.rs`).
- Shared state goes only through `AppState` handles inserted **once** in `on_setup`
  (`systems::setup::load_state`): `WorkspaceMutex = Arc<Mutex<Workspace>>`, `UiCaptureHandle = Arc<UiCapture>`
  (two `AtomicBool`s), `AsyncFileDialogMutex`. `AppState::insert` needs `Clone + Send + Sync`; the `Arc`s
  alias across the render thread's clone, so mutations are visible on both sides. Never re-insert per frame.
- The engine does **not** mask input for scene GUIs, so the GUI stores imgui's `io().want_capture_mouse` /
  `want_capture_keyboard` into `UiCapture` every frame (`gui.rs`), and logic input gates on them
  (`graph_frame_system` reads `UiCapture::mouse`).
- Lock discipline: lock `WorkspaceMutex` only for short snapshots and per action — never across imgui widget
  calls, `request_async_dialog`/`dialog_in_flight`, or any rfd code. Panels snapshot under a short lock, draw
  with the guard dropped, then re-lock to apply. The short-lock sites go through `state::with_workspace`
  (`src/state.rs`), whose guard lives only for the closure, so the lock cannot be held across a widget call.

## Graph pipeline (`src/graph`)

Pure modules (no ECS/imgui; engine imports limited to `Vec2`/`Color`) are `style.rs` (numeric constants,
palette, Z indices), `geometry.rs` (`rows`/`node_height`/`node_rect`, `input_pin`/`output_pin`/`slot_at`,
`hit_test` — pins beat bodies, later node in `doc.nodes` wins, `edge_curve` with control points relative to
the source pin, `arrow_tip_dir`, `graph_bounds`), `wrap.rs` (char-safe greedy `wrap_text` with `"..."`
truncation, `fit_line`), `camera.rs` (`screen_to_world`/`world_to_screen`, `zoom_at` clamped to [0.1, 4.0],
`pan_by_screen_delta`, `center_on`, `frame_all` clamped to [0.1, 1.0]), `layout.rs` (`auto_layout`: BFS-depth
layers from `start`, back edges ignored, unreachable nodes in trailing columns, barycenter ordering with two
forward sweeps, columns at `layer * (NODE_W + COL_GAP)` centred on y = 0; writes via `set_node_pos`, so it
never dirties), `describe.rs` (`describe_node` -> `NodeVisual`, texts/pins local to the node top-left),
`projection_plan.rs` (`snapshot_of` -> `Snapshot{doc_id, revision, metrics_ready, nodes, edges}`,
`plan_projection(prev, next) -> Vec<ProjOp>`, `is_structural`), `interaction.rs` (the pure state machine),
`apply.rs` (pure action application), `frame.rs` (`frame_doc`).

The only ECS-importing graph files are `components.rs` (markers `GraphNode`/`GraphEdge`/`GraphPart`/
`GraphPreview` + the `GraphEntities` resource), `projection.rs`, `camera_system.rs`, `input_system.rs` and
`preview.rs`.

Data flow per tick:

```
graph_frame_system (add_system, one workspace lock per tick, fixed order)
  (a) wheel zoom (apply_wheel = zoom_at with 1.1^scroll_y around the cursor) + middle-mouse pan
      (Action3, pan_step accumulating screen-pixel deltas in a Local<Option<Vec2>>) -> doc.set_camera
      [both ignored while UiCapture::mouse is set, except a pan already in progress]
  (b) build_sample: mouse_world computed from the doc camera AFTER (a) via screen_to_world
      (never InputState::mouse_world_*, which lags the camera)
  (c) step(&mut interaction, doc, &sample, prev_mouse) -> Vec<Action>; apply_actions(doc, &actions)
      -> mutators / doc.set_camera / Workspace.pending_context_menu; drag_preview -> GraphInput.preview
  (d) LAST: camera_update writes MainCamera(Camera2D::new(target, viewport/2).with_zoom(zoom))

graph_projection_system (add_late_system)
  cheap path: (doc_id, revision, metrics_ready) unchanged -> return without snapshotting
  else lock, snapshot_of(active doc, metrics_ready), plan_projection(shown, snapshot), clone spawn data, unlock
  execute ProjOps as entities: SpawnNode/RespawnNode -> root (GraphNode, WorldPosition, ZIndex) with
  GraphPart children spawned WITH a GlobalTransform2D (child_global); MoveNode -> rewrite root position only;
  edges spawned at the source pin as Shape::bezier (control points relative) + one arrowhead child, and
  RewriteEdge rewrites position/shape in place (never respawned for a node move); Despawn cascades via ChildOf
  debug log "projection: N nodes, M edges" only for structural plans (is_structural)

graph_preview_system (add_late_system)
  while GraphInput.preview is Some, keep ONE GraphPreview bezier entity (spawn on demand, rewrite in place,
  despawn on None)
```

`interaction.rs` state machine: `Phase::{Idle, DraggingNode, DraggingLink, RightDown}` (exclusive — the other
button's press mid-gesture is ignored). `mouse_captured_by_ui` gates gesture STARTS only; a running gesture ends
on release. Pin pickup radius is `(PIN_R + 4.0) / zoom` (screen-constant). Left press on a body/input pin
selects and drags; on an output pin it only selects and carries the link — the carried link detaches
(`SetLink .. End`) only once the pointer moves > 4 px screen from the press, so a plain click on a linked pin
emits no mutation. Release over any part of a node reconnects (self allowed); over empty space a detached link
stays `End` (dragging off removes it; never creates a node); releasing over the same pin restores the original
target after a real drag. Right press starts `RightDown`; > 4 px screen travel pans
(`PanByScreenDelta(mouse_screen - prev_mouse_screen)` per tick), otherwise release emits `OpenContextMenu`
plus `Select` for a node hit. A node vanishing mid-gesture falls back to `Idle`.

`apply.rs` maps `Action`s onto the doc mutators and turns `OpenContextMenu` into a `ContextMenuRequest { world,
node }` stored in `Workspace.pending_context_menu` (last one wins). Link changes log `link set`/`link cleared`
at debug — events only.

## GUI layer (`src/scenes/editor`)

`gui.rs` runs one frame: publish capture flags, then menu → shortcuts → tabs → Nodes → Inspector → Problems →
status bar → context menu → modals. Menu/shortcut/tab/context-menu/problem-row intents are collected as
`EditorAction`s and applied in ONE place, `actions::apply_action(ws, action, None)` under a short lock (native
dialog requests come back in `ActionEffects::dialog` and are dispatched after the guard drops; `fx.quit` calls
`intents.request_quit()`). While a native dialog is in flight, `CloseTab` actions are skipped.

- `EditorAction` (actions.rs): `New/Open/Save/SaveAs/CloseTab/Quit`, `FrameAll{viewport}` / `Relayout{viewport}`
  / `AddNode{viewport}` / `AddNodeAt(Vec2)` (viewport = `ui.io().display_size`, read by the producer),
  `DeleteNode/SetStart/DeleteSelected/FocusNode(NodeKey)`, `ShowControls`, `ToggleProblems`.
- Inspector edits are the ONE deliberate exception: they go through the `Edit` enum and the pure
  `inspector::apply_edits(doc, &edits)`, **not** `apply_action` — because `apply_action` logs `{action:?}`,
  which would leak node text into the log. `apply_edits` logs only the edit kind and the node key, and edits
  are applied only if the active doc's `doc_id` still matches the snapshot's.
- Windows: `nodes.rs` (Nodes list + load warnings), `inspector/` (Inspector: `mod.rs` window+Edit+apply_edits,
  `node_fields.rs`, `outputs.rs`, `expr.rs` recursive condition editor, `effects.rs` effects list editor,
  `widgets.rs` shared key/type-combo/Up-Down-Delete widgets),
  `problems.rs` (Problems window, errors first; row click -> `FocusNode`), `status.rs` (bottom status bar),
  `tabs.rs` (tab strip, `###doc{doc_id}` ids), `context_menu.rs` (the `graph_ctx` popup drained from
  `pending_context_menu`), `modals.rs` (notification modal with kind-based title, unsaved-changes close/quit
  confirms, Help > Controls modal), `menu.rs`, `shortcuts.rs` (imgui key state, gated on
  `!io().want_text_input`; F not while Ctrl held; Ctrl+Shift+P toggles Problems).

## Ops layer (`src/ops`)

Plain functions over `Workspace`/`DialogueDoc` — no imgui, no engine ECS. `open_path` canonicalizes both sides
when matching an already-open file (any spelling focuses the existing tab; `Focused` never re-layouts), parses
`serde_json::from_str::<DialogueDef>`, runs `auto_layout`, pushes and activates the doc. `save_doc` runs
`engine_gate` FIRST (a rejected doc writes nothing, not even a `.tmp`), then writes
`serde_json::to_string_pretty(&to_def(doc))` + `\n` atomically via `<path>.tmp` + rename, sets the canonical
path and `mark_clean`. `save_doc_at` adds a tab-collision check (`AlreadyOpen`). `error_notification` maps
`OpError::Invalid` to a `SaveRejected` notification (engine message only) and everything else to `Error`.
Doc paths are stored canonical/absolute; `doc_title` is the file name or "Untitled".

`src/systems/file_dialogs.rs` bridges rfd: the GUI calls `request_async_dialog` (at most one dialog in flight;
the future is built on the caller thread, driven on a worker via `futures::executor::block_on`, result over a
crossbeam channel in `AsyncFileDialogMutex`); `poll_async_dialogs` (logic thread) drains it once per tick and
applies it through the pure `apply_result`, pushing failures into `Workspace.notifications`.

## Module map

| File | What it is |
|---|---|
| `src/main.rs` | Entry: `env_logger` (default `info`), `EngineBuilder` wiring — `on_setup(load_state)`, `add_system(poll_async_dialogs)`, `add_system(graph_frame_system)`, `add_late_system(graph_projection_system)`, `add_late_system(graph_preview_system)`, `window_resize` observer, scene `"editor"` + enter + gui, `try_run()` |
| `src/state.rs` | `UiCapture` (mouse/keyboard `AtomicBool`s), the handle aliases `UiCaptureHandle`, `WorkspaceMutex`, and `with_workspace` (the short-lock helper: runs a closure with the workspace locked, guard dropped on return) |
| `src/model/mod.rs` | Model module root + re-exports (`from_def`/`to_def`, doc types, lint, notifications) |
| `src/model/doc.rs` | `Workspace`, `DialogueDoc` + all named mutators, `revision`/`content_revision`/`dirty` rules, lint cache, `NodeKey`, `Link`, `EdChoice`, `Outputs`, `EdNode`, `Slot`, `CamState`, `ContextMenuRequest` |
| `src/model/convert.rs` | `from_def` (tolerant) / `to_def` (total, exact) against the engine's `DialogueDef` — load/save only |
| `src/model/lint.rs` | `Severity`/`ProblemKind`/`Problem`, `lint` (all problems, deterministic order), `engine_gate` (`DialogueTree::try_from(to_def(doc))`) |
| `src/model/notify.rs` | `Notification`/`NotificationKind` (`Error`, `SaveRejected`) queue entries for the GUI modal |
| `src/ops/mod.rs` | `open_path`/`save_doc`/`save_doc_at`/`new_doc`/`close_doc`/`doc_title`, `OpenOutcome`, `OpError`, `error_notification` |
| `src/graph/mod.rs` | Graph module wiring, re-exports, `#[cfg(test)] fixtures` helper (fixture JSON + laid-out guard doc) |
| `src/graph/style.rs` | Numeric constants (node/pin/row/gap sizes, edge/outline thickness, `FONT`), palette, Z indices |
| `src/graph/geometry.rs` | `rows`/`node_height`/`node_rect`, `input_pin`/`output_pin`/`slot_at`, `hit_test`, `edge_curve`, `arrow_tip_dir`, `graph_bounds` |
| `src/graph/wrap.rs` | Char-safe greedy `wrap_text` with `"..."` truncation + `fit_line` |
| `src/graph/camera.rs` | `screen_to_world`/`world_to_screen`, `zoom_at` ([0.1, 4.0]), `pan_by_screen_delta`, `center_on`, `frame_all` ([0.1, 1.0]) |
| `src/graph/layout.rs` | `auto_layout` — layered left-to-right layout, BFS from start, deterministic, view-only |
| `src/graph/describe.rs` | `describe_node` -> `NodeVisual` (texts/pins local to the node top-left), `row_text`/`row_link` |
| `src/graph/projection_plan.rs` | `Snapshot`/`NodeSig`/`EdgeView`/`ProjOp`, `snapshot_of`, `plan_projection`, `is_structural` (pure) |
| `src/graph/components.rs` | ECS markers `GraphNode`/`GraphEdge`/`GraphPart`/`GraphPreview`, `EdgeEntry`, `GraphEntities` resource (the only graph file besides the systems importing ECS types) |
| `src/graph/projection.rs` | `graph_projection_system` — executes `ProjOp`s as entities; `spawn_node`/`spawn_edge`/`rewrite_edge`, `arrowhead_points`, `child_global` (children spawned WITH `GlobalTransform2D`) |
| `src/graph/camera_system.rs` | `graph_frame_system` — the single frame system: wheel/pan -> input sample -> `step`+`apply_actions` -> `MainCamera` write last; pure `apply_wheel`/`pan_step`/`camera_update` |
| `src/graph/input_system.rs` | `GraphInput` resource (interaction + preview + last_doc_id), `ButtonSample`, pure `build_sample` (mouse world from the doc camera) |
| `src/graph/interaction.rs` | Pure state machine: `GraphInteraction`/`Phase`/`InputSample`/`Action`, `step`, `drag_preview` |
| `src/graph/apply.rs` | `apply_actions` — maps `Action`s onto doc mutators, returns the `ContextMenuRequest` |
| `src/graph/frame.rs` | `frame_doc` (`set_camera(frame_all(graph_bounds(doc), viewport, 40.0))`) |
| `src/graph/preview.rs` | `graph_preview_system` — the single `GraphPreview` drag-link entity |
| `src/scenes/mod.rs` | `pub mod editor` |
| `src/scenes/editor/mod.rs` | Editor module wiring, `gui`/`enter` re-exports, `display_viewport`, `#[cfg(test)] testkit` (`fixture`, `app_state_with`, `ws_with_two_linked_nodes`, `TempDir`) |
| `src/scenes/editor/lifecycle.rs` | Scene enter: insert `MainCamera`, `GraphEntities`, `GraphInput`; disable camera follow; rebind `Action2` to right mouse only |
| `src/scenes/editor/gui.rs` | The scene GUI callback: capture flags, panel draw order, `EditorAction` queue application |
| `src/scenes/editor/menu.rs` | Main menu bar (File/Edit/View/Help) producing `EditorAction`s |
| `src/scenes/editor/shortcuts.rs` | imgui key polling (`plain_shortcuts`, `ctrl_shortcuts`), gated on `!want_text_input` |
| `src/scenes/editor/tabs.rs` | Tab strip (`###doc{doc_id}` ids, `LAST_ACTIVE`, `clicked_focus`/`resolve_focus` rules) |
| `src/scenes/editor/nodes.rs` | Nodes window: load warnings + selectable node-id list |
| `src/scenes/editor/inspector/mod.rs` | Inspector window, the `Edit` enum, pure `apply_edits`, `change_mode`, snapshot/`doc_id` guard |
| `src/scenes/editor/inspector/node_fields.rs` | Node field widgets (id/speaker/text/labels/start/on-enter), `NodeFlags`, `labels_editor` |
| `src/scenes/editor/inspector/outputs.rs` | Outputs editor: Next/Choices radios, per-choice sections (`###choice{i}`), `combo_entries`, `target_combo`, `truncate_chars` |
| `src/scenes/editor/inspector/expr.rs` | Recursive condition editor (`condition_editor`, `optional_condition_editor`), `CondKind`, pure `convert_condition` |
| `src/scenes/editor/inspector/effects.rs` | Effects list editor (`effects_editor`), `EffectKind`, pure `convert_effect`, `move_item` |
| `src/scenes/editor/inspector/widgets.rs` | Widgets shared by the condition/effect/choice editors: `key_editor` (with the faint-red "(empty key)" hint), `type_combo`, `reorder_delete_row`/`RowAction` |
| `src/scenes/editor/problems.rs` | Problems window: `problem_counts`/`grouped_rows`/`row_target`/`header_*`, `should_draw` (a hidden window is not drawn at all) |
| `src/scenes/editor/status.rs` | Bottom status bar + pure `file_text`/`problems_text`/`nodes_text`/`zoom_text` |
| `src/scenes/editor/context_menu.rs` | `graph_ctx` popup: drains `pending_context_menu`, `items_for`, `PENDING` thread-local |
| `src/scenes/editor/modals.rs` | Modals (notification/close/quit/Controls), the `CONTROLS` list, `apply_choice_ws`/`ChoiceEffect`, `modal_title` |
| `src/scenes/editor/actions.rs` | `EditorAction`, `ActionEffects`, the single `apply_action`, `tab_label`, `close_decision`, `quit_decision` |
| `src/systems/mod.rs` | `file_dialogs`, `setup`, `window_resize` |
| `src/systems/setup.rs` | `on_setup` — inserts `UiCaptureHandle`, `WorkspaceMutex`, `AsyncFileDialogMutex` into `AppState` once |
| `src/systems/file_dialogs.rs` | rfd bridge: `request_async_dialog`, `dialog_in_flight`, `drain_completed`, pure `apply_result`, `poll_async_dialogs` |
| `src/systems/window_resize.rs` | `WindowResized` observer: `GameConfig::set_render_size` + re-centre the camera offset (mouse coords stay 1:1) |

## How to add ...

- **A menu action:** add an `EditorAction` variant in `scenes/editor/actions.rs`, an arm in `apply_action`
  (no-ops without an active doc where applicable), a menu item in `menu.rs` (and/or a key in `shortcuts.rs`),
  plus a `#[cfg(test)]` test in `actions.rs`. Viewport-carrying actions (`FrameAll`, `Relayout`, `AddNode`) get
  the viewport from `ui.io().display_size` at the producer (`display_viewport` in `scenes/editor/mod.rs`).
- **An Inspector field / `Edit` variant:** add the variant to `Edit` in `inspector/mod.rs`, an arm in the pure
  `apply_edits` (through the existing doc mutators; log only the kind + node key — `trace!` for per-keystroke
  kinds, `debug!` for structural ones), a widget in `node_fields.rs`/`outputs.rs` that pushes the edit only when
  the widget reports a change, and tests for `apply_edits`. Widget ids come from the `NodeKey`/indices, never
  from the edited text (the panel body is wrapped in `push_id(node{key})`).
- **A new condition or effect kind** (when the engine enums gain one): extend `CondKind` (`of`/`name`/`KINDS`)
  and the pure `convert_condition` in `inspector/expr.rs`, plus the per-kind arm in `condition_editor`;
  extend `EffectKind` + `convert_effect` + the value-widget arm in `inspector/effects.rs`. Keep the serde-shape
  tests green (every producible variant must serialize to the engine's internally-tagged snake_case JSON).
- **A new lint rule:** add a `ProblemKind` variant in `model/lint.rs` (declaration order = report order within a
  node), an arm in `lint`, and engine-parity tests in `lint.rs`'s `#[cfg(test)]` module (soundness: lint-clean
  implies `engine_gate` Ok over the fixture + mutation family; coverage: each engine error maps to a lint
  error). The Problems window needs no change (rows are generic over `Severity`/`Problem`), but the row is
  clickable only when `problem.node` still exists.
- **A new window/panel:** follow the standard pattern — a `snapshot(app_state)` that locks `WorkspaceMutex`
  briefly (via `state::with_workspace`) and clones what the panel needs, `draw` that runs with the guard
  dropped and collects intents (`EditorAction`s or `Edit`s), then a fresh short lock to apply; register the
  draw call in `gui.rs`. A window
  that can be hidden must not be drawn at all when hidden (imgui's `p_open` does not hide it — see
  `problems.rs::should_draw`). Widget ids: `push_id_usize(index)` per row + fixed `###suffix` labels, never
  content text.

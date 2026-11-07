# AberredDialogueEditor

Node-graph editor for AberredEngine dialogue JSON. Rust binary crate on the `aberredengine` git dependency
(branch `develop-0.2.0`, `default-features = false`). Read `docs/plans/00-overview.md` first — the plan files in
`docs/plans/` define the architecture; read them before changing architecture.

## Commands

All acceptance commands are `just` recipes (run from the repo root):

| Recipe      | What it runs                                                        |
|-------------|---------------------------------------------------------------------|
| `just check`| `cargo fmt --check` + `cargo clippy --all-targets -- -D warnings` + `cargo test` (default recipe) |
| `just test` | `cargo test`                                                        |
| `just lint` | `cargo clippy --all-targets -- -D warnings`                         |
| `just run`  | `cargo run`                                                         |
| `just fmt`  | `cargo fmt`                                                         |

## Policies

- **Commit policy:** small, meaningful commits. Claude stages exactly the files that belong to a step
  (`git add <explicit paths>`, never blind `git add -A`), proposes a one-line commit message with no
  `Co-Authored-By` trailer, and stops. The user runs `git commit`. Claude never runs `git commit` or `git push`.
- **Vibe policy:** implementation, code fixes, test creation, simplification and code review go through the
  vibe MCP (`vibe_submit`/`vibe_wait`/`vibe_result(diff=true)`/`vibe_apply`/`vibe_cancel`). Claude does reasoning,
  planning, specs, diff review, running the acceptance recipes and debugging. If the vibe MCP is not connected,
  STOP and tell the user — do not silently implement.

## Gotchas

- GUI callbacks are plain `fn` pointers (`fn(&mut GuiCtx)` with fields `ui`, `signals`, `intents`, `textures`,
  `fonts`, `app_state`) — no captures. (`add_scene_gui` / `add_scene_world_draw`.)
- The GUI callback runs on the **render thread**; systems and observers run on the **logic thread**. They share
  state only through `AppState` (`Arc<Mutex<T>>` handles inserted once in `on_setup` alias across the clone).
- B0002: never request the same resource twice in one system (e.g. `ResMut<AppState>` plus `Res<AppState>` in
  one param list). It panics at startup and `cargo check` cannot see it.
- Render-only resources (`TextureStore`, `FontStore`, `ShaderStore`, `RaylibAccess`) panic at schedule init when
  requested from logic systems/observers — invisible to `cargo check`. Use them only in GUI callbacks
  (`ctx.textures` / `ctx.fonts`).
- The engine does NOT mask input for scene GUIs: clicks/keys reach logic even over imgui windows. The GUI
  publishes `want_capture_mouse`/`want_capture_keyboard` into the shared `UiCapture` atomics every frame; logic
  input systems must gate on them.
- Engine imports go through `aberredengine::prelude::{...}`; use `aberredengine::core::...` only for items the
  prelude lacks (e.g. `events::window::WindowResized`, `resources::input_bindings::InputBindings`).
  Do not add `bevy_ecs` as a dependency — the facade re-exports it.

## Model and lint

- `src/model` is pure: plain data and functions, no imgui/ECS/window. Its only engine imports are
  `aberredengine::prelude::{DialogueDef, NodeDef, ChoiceDef, DialogueCondition, DialogueCmp, DialogueEffect,
  DialogueTree, DialogueError, Vec2}`.
- The `Workspace` also carries the typed modal queue `notifications: Vec<Notification>` (`Notification`/
  `NotificationKind` in `src/model/notify.rs`) and `problems_hidden`, the INVERSE visibility flag of the
  Problems window (the derived `Default` = visible) — see "Problems panel, status bar, typed notifications"
  below.
- Every mutation on `DialogueDoc` goes through a named mutator that bumps `revision` and sets `dirty`;
  `set_node_pos`, `select` and `mark_clean` only bump `revision` (view-only, never saved), and
  `set_camera` bumps nothing at all — like the other view-only mutators it never sets `dirty`, and it
  must not invalidate the lint cache or trigger a projection (the camera is pure view state).
- Two revision counters: `revision` is bumped by EVERY mutator (it is the graph-projection key), while
  `content_revision` is bumped only by content mutators — the lint cache (`DialogueDoc::problems`) is
  keyed on `content_revision`, so dragging/selecting/camera moves never re-run the linter.
- `from_def`/`to_def` convert to/from the engine's `DialogueDef`, and only at load/save.
- `lint::lint` reports ALL problems of a doc (cached per `content_revision` via `DialogueDoc::problems`);
  `lint::engine_gate` (`DialogueTree::try_from(to_def(doc))`) is the save gate — engine rules are never
  reimplemented in the model.
- Tests live in `#[cfg(test)]` modules inside the source files: this is a binary crate, so `tests/`
  cannot import its modules (fixtures load via `include_str!(concat!(env!("CARGO_MANIFEST_DIR"), ...))`).

## Ops

- `src/ops` is the file-operation layer: plain functions over `Workspace`/`DialogueDoc` — no imgui, no engine ECS.
- `open_path` canonicalizes both sides before matching, so any spelling of an already-open file focuses the existing tab.
- `save_doc` runs `engine_gate` first (a rejected doc writes nothing, not even a `.tmp`) and saves atomically via `<path>.tmp` + rename.
- Doc paths are stored canonical/absolute by `open_path`/`save_doc`; `doc_title` is the file name or "Untitled".

## GUI

- Every user intent (menu item, Ctrl shortcut, tab close button, graph context-menu item) is an
  `EditorAction`, applied in ONE place:
  `scenes/editor::actions::apply_action` (pure over `Workspace`; native-dialog requests go through its
  `Option<&AppState>` parameter — the GUI passes `None` under the lock and dispatches the returned request after).
  `EditorAction` also carries the node actions (`AddNodeAt`/`AddNode`/`DeleteNode`/`SetStart`/`DeleteSelected`);
  the graph context menu is drawn by `scenes/editor/context_menu.rs`, which drains
  `Workspace.pending_context_menu` and opens the `graph_ctx` popup from the frame's top-level scope.
- Lock discipline: lock `WorkspaceMutex` only for short snapshots and per action, never while calling
  `request_async_dialog`/`dialog_in_flight` or any rfd code; widgets are drawn with the guard dropped.
- Shortcuts are read with `ui.is_key_pressed(Key::..)` + `io().key_ctrl`/`key_shift`, only when
  `!io().want_text_input`. Modals (the typed notification queue, unsaved-changes close/quit, Help > Controls) open from the frame's
  top-level scope after the menu; one at a time, the notification modal first, the Controls modal last. While a native dialog is in flight, tab closes are
  ignored (a pending Save As is applied by tab index).
- On the `SET_SELECTED` frame imgui still reports the previously selected tab as selected; `tabs::draw`
  ignores every tab's selected report on that frame (`clicked_focus`) — otherwise the active tab
  ping-pongs between the old and new tab on every frame.
- Tab ids are `###doc{doc_id}` (`actions::tab_label`): everything before `###` is display-only, so a tab's
  imgui id never depends on the title, the dirty star or the tab index — otherwise editing a doc (the
  dirty star) or closing a tab would give the tab a new id and silently switch the active tab.
- A frame with a clicked X ignores imgui's neighbour re-selection (`tabs::resolve_focus` drops the pending
  focus) and re-asserts the real active tab on the next frame (`LAST_ACTIVE` reset to `None`, so it is a
  `SET_SELECTED` frame).
- Inspector edits (the fields typed in the Inspector window) are applied by
  `scenes/editor::inspector::apply_edits` — the second deliberate place edits are applied, NOT through
  `EditorAction`, because `apply_action` logs `{action:?}` which would leak node text. They are guarded by
  `doc_id`: edits collected against a snapshot are applied only if the active doc is still the snapshotted one,
  and the logging there emits only the edit kind and the node key, never the content.

## Logging

- Default level is `info` (set in `main.rs`); configure at run time with `RUST_LOG`, e.g.
  `RUST_LOG=aberreddialogueeditor=debug just run`.
- Log on events only (an action applied, a dialog result, a file operation, a lint-cache recompute) — never
  inside per-frame GUI paths.
- What is logged where: file operations and outcomes in `ops`, dialog requests/results and the typed
  notifications they push (kind only — a SaveRejected message quotes node ids, so never its text) in
  `systems/file_dialogs.rs`, the inserted shared handles in `systems/setup.rs`, applied/ignored `EditorAction`s
  in `scenes/editor/actions.rs`, modal choices and outcomes in `scenes/editor/modals.rs`, `from_def` load
  warnings plus summary in `model/convert.rs`, and the lint-cache recompute (content_revision, error/warning counts)
  in `model/doc.rs`.
- Never log document content: no node text, speaker text or whole file contents.

## Graph (pure)

- `src/graph` is pure logic — no ECS/imgui; engine imports in the pure files are limited to `Vec2`
  and `Color`. There is no blanket module-level allow any more: the re-export list in `mod.rs` only
  re-exports what is actually consumed, and the one item kept solely for its tests
  (`camera::world_to_screen`) carries a narrow item-level `#[allow(dead_code)]` with a reason.
  `style.rs` holds the numeric constants (node/pin/row/gap sizes, edge/outline metrics, `FONT`)
  plus the palette and the Z indices.
- `geometry.rs`: `rows`/`node_height`/`node_rect`, `input_pin`/`output_pin`, `slot_at` (row index -> output
  `Slot`; shared with the projection plan), `hit_test` (pins beat
  bodies, later node in `doc.nodes` wins), `edge_curve` (control points RELATIVE to the source pin,
  endpoint `to - from`), `arrow_tip_dir`, `graph_bounds`. `wrap.rs`: char-safe greedy `wrap_text`
  with `"..."` truncation via a `measure` callback, plus `fit_line`. `camera.rs`:
  `screen_to_world`/`world_to_screen`, `zoom_at` (clamp [0.1, 4.0]), `pan_by_screen_delta`,
  `frame_all` (zoom clamped to [0.1, 1.0], `None` -> ZERO/1.0).
- Graph tests are `#[cfg(test)]` modules in each file; fixtures load via
  `include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/guard.json"))`.
- `layout.rs`: `auto_layout(doc)` is the layered left-to-right layout — BFS depth from `start` (back edges
  ignored, unreachable nodes in trailing columns), barycenter ordering with two forward sweeps, columns at
  `layer * (NODE_W + COL_GAP)` with each layer centred on y = 0. It writes via `set_node_pos`, so it bumps
  `revision` but never sets `dirty`; `ops::open_path` runs it on every newly opened doc (never on Focused).

## Graph projection prep (plan 03 step 2a)

- `Slot` derives `Hash`; `DialogueDoc.doc_id` is unique per workspace (`Workspace.next_doc_id`, assigned in
  `add_doc` starting at 1; a bare doc is 0, and `doc_id == 0` with no content means "nothing shown").
- `style.rs` now also holds the palette (`Color::new(..)` consts) and the Z constants (`f32` payloads for
  `ZIndex`: edge 0, arrowhead 1, body 10, header 11, text 12, pin 13, preview 20).
- `src/graph/components.rs` is the ONLY graph file importing ECS types (`Component`/`Resource`/`Entity` via
  the prelude, plus `use aberredengine::bevy_ecs;` so the derive macros' `bevy_ecs::..` paths resolve):
  markers `GraphNode`/`GraphEdge`/`GraphPart`/`GraphPreview` and the `GraphEntities` resource.
- `describe.rs` (pure) turns an `EdNode` into a `NodeVisual` (texts/pins local to the node top-left);
  `projection_plan.rs` (pure) snapshots a doc (`snapshot_of`, carrying `doc_id`/`revision`/`metrics_ready`)
  and diffs it against the remembered `Snapshot` (`plan_projection` -> `ProjOp`s; edges are rewritten,
  never respawned, when a node moves).

## Graph projection (plan 03 step 2b)

- `src/graph/projection.rs` holds `graph_projection_system` — registered with `add_late_system` in `main.rs`,
  with `GraphEntities::default()` inserted at editor scene enter (`lifecycle.rs`; that is enough, no `on_setup`
  insert needed). It is the only graph file besides `components.rs` importing ECS types. Per tick it locks
  `WorkspaceMutex` only to snapshot the active doc and clone what the plan needs, then unlocks before any
  `Commands`/query call; when doc id, revision and metrics readiness are unchanged it returns without
  snapshotting. `GraphEntities` remembers the last executed plan as `shown: Snapshot` — the `prev` side of
  the next diff, whose `(doc_id, revision, metrics_ready)` triple is the cheap-path key; its `nodes`/`edges`
  maps hold only the spawned entities (node key -> root `Entity`, edge key -> bezier entity + arrowhead).
- Nodes spawn as a root (`GraphNode`, `WorldPosition`, `ZIndex`) with `GraphPart`/`ChildOf` children at local
  positions: body `rounded_rect` (thick outline only when selected/error), header `rect`, `DynamicText` items
  (dropped until font metrics arrive — `metrics_ready` is in the node sig, so nodes respawn when they land),
  pin circles. `Shape::circle` is centred on its local origin, so pins need no `.with_origin`; `DynamicText`'s
  position is its top-left. Edges are bezier entities at the source pin (control points relative to it) with one
  arrowhead `GraphPart` child (`arrowhead_points`, pure + tested); `RewriteEdge` assigns position/shape only when
  different. Logs `projection: N nodes, M edges` at debug only for STRUCTURAL plans (`is_structural` in
  `projection_plan.rs`: any Spawn/Respawn/Despawn op) — MoveNode/RewriteEdge-only plans (a node drag, once per
  tick) stay silent.
- Children of graph entities (node parts, the edge arrowhead) must be spawned WITH a `GlobalTransform2D`
  (`child_global(parent_world, local)` in `projection.rs`): the engine's `propagate_transforms` inserts it via
  deferred Commands, so without it a freshly spawned child renders one frame at its raw local offset (near world
  origin). Root entities (node roots, the edge itself) must NOT get one — their own `WorldPosition` is world-space.

## Graph camera system (plan 03 step 3)

- `src/graph/camera_system.rs` holds `graph_frame_system` — registered with `add_system` in `main.rs` (Update),
  the single graph frame system: wheel/pan camera update and the interaction input run in it in a fixed order,
  with the `MainCamera` write last. Each tick it locks the workspace briefly, reads the active doc's `CamState`, and writes
  `MainCamera(Camera2D::new(target, viewport/2).with_zoom(zoom))` via the plain `camera_update` (viewport =
  `GameConfig` render size; no active doc leaves the camera alone). Wheel zoom (`apply_wheel` = `zoom_at` with
  `1.1^scroll_y` around the cursor) and middle-mouse pan (`Action3`, `pan_step` accumulating screen-pixel deltas
  in a `Local<Option<Vec2>>`, never `mouse_world_*`) are ignored while `UiCapture::mouse` is set, except a pan
  already in progress; results go back through the view-only `DialogueDoc::set_camera`.
- `src/graph/frame.rs` (pure): `frame_doc(doc, viewport)` = `set_camera(frame_all(graph_bounds(doc), viewport, 40.0))`.
  `EditorAction::FrameAll { viewport }` (actions.rs; View > "Frame all" and the `F` key, both reading the viewport
  from `ui.io().display_size`) is applied in `apply_action` via `frame_doc`; no-op without an active doc.

## Graph interaction state machine (plan 03 step 4a)

- `src/graph/interaction.rs` (pure, only `Vec2` from the prelude) holds `GraphInteraction`/`Phase`
  (`Idle`/`DraggingNode`/`DraggingLink`/`RightDown`, exclusive — the other button's press mid-gesture is ignored)
  plus `step(&mut state, &doc, &InputSample, prev_mouse_screen) -> Vec<Action>` and `drag_preview`.
- Pin pickup radius is `(PIN_R + 4.0) / zoom` (screen-constant); `mouse_captured_by_ui` gates gesture STARTS only —
  a running gesture ends on release, never on capture. A node vanishing mid-gesture falls back to `Idle` (no unwrap).
- Left press on a body/input pin selects and drags (`MoveNode` = `mouse_world - grab_offset`); on an output pin it
  only selects and carries the link. `DraggingLink` records `press_screen`, `had_link` (the pickup link was
  `Node`/`Dangling`, not `End`) and `restore: Option<NodeKey>` (the pickup-time target, Some only for a
  `Link::Node`); the carried link detaches (`SetLink .. End`) only once the pointer moves > 4 px screen from
  `press_screen` (`detached`), on drag ticks and the release tick alike — so a plain click on a linked pin emits
  no mutation at all. Release over any part of a node (body/input pin/any other output pin, self allowed)
  reconnects, over empty space a detached link stays `End` — dragging off the pin removes it and never creates a
  node. Releasing over the SAME output pin the drag started from: a detached drag with `restore` re-emits
  `SetLink .. Link::Node(r)` to restore it; anything else emits nothing (a click never self-links). Right press
  starts `RightDown`; > 4 px screen travel pans
  (`PanByScreenDelta(mouse_screen - prev_mouse_screen)` each tick), otherwise release emits
  `OpenContextMenu { world: press_world, hit: press-time hit }` plus `Select` for a node hit.
- Tests are the `#[cfg(test)]` module in `interaction.rs`: one table of scripted `InputSample` sequences over
  `guard.json`/`invalid_dangling.json` (via `graph::fixtures` + `auto_layout`), plus zoom-radius, vanished-node and
  `drag_preview` tests. The frame system in `camera_system.rs` feeds the samples and applies the returned `Action`s.

## Graph input application (plan 03 step 4b)

- `src/graph/apply.rs` (pure) maps `Action`s onto the doc mutators (`apply_actions -> Option<ContextMenuRequest>`);
  `OpenContextMenu` becomes a `ContextMenuRequest { world, node }` (last one wins), stored in
  `Workspace.pending_context_menu` for the GUI. Link changes log `link set`/`link cleared` at debug — events only.
- `src/graph/input_system.rs` holds the `GraphInput` resource (`interaction`, `preview`, `last_doc_id`; inserted at
  editor scene enter) and the pure `build_sample` (mouse world computed from the doc camera AFTER wheel/pan, never
  `mouse_world_*`). `graph_frame_system` in `camera_system.rs` (7 params, one workspace lock) does wheel+middle-pan,
  then `step` + `apply_actions` + preview/prev-mouse bookkeeping, then writes `MainCamera` last; a doc-id change
  resets the interaction, and no active doc clears the preview and leaves the camera alone.
- `src/graph/preview.rs` (`add_late_system`) keeps ONE `GraphPreview` bezier entity while `GraphInput.preview` is
  Some — spawn on demand (also when the remembered entity died, e.g. scene switch), rewrite in place, despawn on None.

## Graph context menu and node actions (plan 03 step 4c)

- `EditorAction` gained `AddNodeAt(Vec2)`, `AddNode { viewport }`, `DeleteNode(NodeKey)`, `SetStart(NodeKey)` and
  `DeleteSelected`, all applied in the single `apply_action` (no-ops without an active doc; `AddNode` places the node
  at `graph::screen_to_world(&doc.camera, viewport, viewport / 2.0)` and selects it). Edit > "Add node" pushes
  `AddNode { viewport }`; the Delete key pushes `DeleteSelected`; `F` no longer fires while Ctrl is held (Ctrl+F).
- `src/scenes/editor/context_menu.rs` (called from `gui.rs` after `nodes::draw`, before `modals::draw`, at the frame's
  top-level scope) takes `ws.pending_context_menu` under a short lock (guard dropped before any imgui call), stores it
  in a `thread_local!` `Cell` and calls `ui.open_popup("graph_ctx")` exactly once per request; `ui.popup` renders the
  pure `items_for(request)` list (node hit -> "Set as start"/"Delete node"/"Add node here", empty -> only "Add node
  here", separator before the last item when more than one) — a click pushes the action and closes the popup, and the
  stored request is cleared once the popup is no longer open.

## Re-layout action and Help > Controls modal (plan 03 step 5)

- View > "Re-layout" is `EditorAction::Relayout { viewport }` (enabled only with an active doc): `apply_action` runs
  `graph::auto_layout(doc)` then `graph::frame_doc(doc, viewport)` on the active doc and no-ops without one. Positions are
  view-only (`set_node_pos` never sets `dirty`), so a clean doc stays clean and a dirty doc stays dirty.
- Help > "Controls" is `EditorAction::ShowControls` (always enabled), setting the workspace-level `Workspace.show_controls`.
  While it is set and no error/close/quit modal is showing, modals.rs draws `CONTROLS_MODAL` (`"Controls##help"`, lowest
  priority): the pure `CONTROLS: &[(&str, &str)]` list as two aligned columns, a separator and an OK button
  (`ModalChoice::ControlsOk`). Choice application is factored into the pure `apply_choice_ws(&mut Workspace, choice)` —
  `apply_choice` only locks, calls it and dispatches the returned `ChoiceEffect` (quit / Save As dialog) after the lock drops.

## Inspector (plan 04 steps 1a/1b)

- `src/scenes/editor/inspector/mod.rs` draws the "Inspector" window (a plain imgui window; position/size set only on
  first use, top-right) showing the SELECTED node of the ACTIVE doc — "Nothing selected" (disabled text) without a
  selection, "No document" without an open doc. It also holds the `Edit` enum (`SetId`/`SetSpeaker`/`SetText`/
  `SetLabels`/`SetStart`) and the PURE `apply_edits(doc: &mut DialogueDoc, edits: &[Edit])`, which applies the edits
  in order through the doc mutators (an edit for a vanished NodeKey is ignored by the mutators: no panic, no bump)
  and logs only the edit KIND and the node key (`trace!` for the per-keystroke kinds, `debug!` for `SetStart`).
- `src/scenes/editor/inspector/node_fields.rs` draws the widgets against a cloned `EdNode` and returns the requested
  `Vec<Edit>`; `NodeFlags { is_start, duplicate_id, empty_id }` is computed in the snapshot from `doc.problems()`
  (`is_start` = `doc.start == Some(key)`).
- Per frame: lock the workspace briefly to snapshot the selected node (clone), its flags and `doc_id`, unlock; draw
  the widgets with the guard dropped, emitting an edit ONLY when a widget reports a change; if there are edits, lock
  again and apply them via `apply_edits` ONLY IF the active doc's `doc_id` still equals the snapshot's (otherwise
  drop them). The lock is never held across widget calls.
- Widget ids come from the NodeKey, never from the text: the whole panel body is wrapped in a `push_id` keyed by the
  NodeKey and each label row pushes its index, so typing never changes a widget id (the input keeps focus).
- Outputs are edited in the inspector (step 1b below), and each choice's `when`/`effects` plus the node's
  on-enter effects are edited by the condition/effect editors (step 2 below).

### Inspector step 1b — outputs and choices

- `src/scenes/editor/inspector/outputs.rs` draws the outputs editor (called from
  `node_fields::draw` in place of the old read-only "Outputs:" line); the inspector snapshot now
  also carries `targets`: the `(key, id)` of every node of the doc in node order, cloned under
  the same short lock and passed down for the target combos.
- Mode switching: two radio buttons "Next"/"Choices" emit `Edit::ChangeMode(key, OutputsMode)`.
  `apply_edits` converts purely (`change_mode`): Next -> Choices builds exactly ONE empty choice
  whose target INHERITS the current next link (a Dangling one included); Choices -> Next keeps
  the FIRST choice's target (End if none) and drops the choices (no confirmation, no undo); a
  ChangeMode to the current mode is a no-op (no bump). No edit path can ever leave
  `Outputs::Choices(vec![])` (deleting the last choice goes through `Next(End)`; `AddChoice` on a
  Next node converts ChangeMode-style so the next link is not lost).
- Next mode shows one "Next" target combo; Choices mode lists one collapsing section per choice
  labelled `"Choice N: <truncated text>###choice{i}"` — the visible part may change with the text
  but the imgui id is ONLY the `###choice{i}` part (typing never changes the widget id, so the
  section stays open and the input keeps focus); each choice is wrapped in `push_id_usize(i)`
  and holds a text input, a target combo, the shared labels editor, Up/Down/Delete buttons
  (Up disabled on the first, Down on the last) and the read-only "When: (none)/set" +
  "Effects: N" placeholders; a "[+ Add choice]" button ends the list.
- Target combos: the pure `combo_entries(nodes)` builds "(end)" (`Link::End`) first, then every
  node by id; duplicate ids (or an empty id) get `"{id} (#{n})"` with the 1-based node-order
  position (`"(empty id) (#{n})"` for an empty id). `target_combo` shows the current entry's
  label as preview, or `"unknown: {s}"` in RED (a `Text` style colour pushed around the combo)
  for a `Link::Dangling(s)`, while the dropdown still lists the normal entries; selecting a
  different entry yields the new `Link`.
- The new `Edit` variants (`ChangeMode`/`SetLink`/`AddChoice`/`DeleteChoice`/`MoveChoice`/
  `SetChoiceText`/`SetChoiceLabels`) go through the existing model mutators (only
  `ChangeMode`/`AddChoice`-on-Next use `set_outputs`); edits for a vanished node or an
  out-of-range choice index are ignored (no panic, no bump). Logging stays kind + node key +
  indices only: `trace!` for the per-keystroke kinds (`SetChoiceText`/`SetChoiceLabels`),
  `debug!` for the structural ones (`ChangeMode`/`SetLink`/`AddChoice`/`DeleteChoice`/`MoveChoice`).

### Inspector step 2 — condition and effect editors

- `src/scenes/editor/inspector/expr.rs` holds the recursive condition editor used by every choice's
  `when` (`optional_condition_editor`: a "When" checkbox that creates an empty `Flag` on check and
  clears on uncheck, with `condition_editor` below while `Some`) and `effects.rs` holds the effects
  list editor used by every choice's `effects` and by the node's `on_enter` — the old read-only
  placeholders are gone. Both editors work on a LOCAL CLONE of the value and return `true` on
  change; the caller emits ONE whole-value edit (`SetChoiceWhen`/`SetChoiceEffects`/`SetOnEnter`),
  applied by `apply_edits` through `set_choice_when`/`set_choice_effects`/`set_on_enter`
  (`trace!` kind + node key + index only, never content; vanished node / out-of-range index ignored,
  no bump). In `outputs.rs` the Effects sub-section is a collapsing header labelled
  `format!("Effects ({})###effects", n)` and in `node_fields.rs` `format!("On enter effects ({})###onenter", n)`:
  the visible count may change, the imgui id (the `###` part) never does.
- Type switching is the PURE `convert_condition(old, to: CondKind)` / `convert_effect(old, to: EffectKind)`
  (table-tested for all 6x6 pairs): same kind returns the old unchanged; converting INTO a leaf keeps the
  key of a leaf source but uses an empty key from a compound source (the Not/All/Any structure is dropped
  ON PURPOSE when the user picks a leaf type); target `Not` wraps the whole old condition; target
  `All`/`Any` keep the children (All<->Any verbatim, `Not{cond}` contributes `[*cond]`, a leaf `[old]`,
  so no work is lost). Effects always keep the key; `SetInteger`<->`AddInteger` keep the i32
  (value<->delta); every other payload gets its default (0, 0.0, ''). Effects reorder via the pure
  `move_item` (clamped, no-op out of range).
- Widget ids never depend on edited text: every condition recursion level and list item and every
  effects row is wrapped in `ui.push_id_usize(..)` (plus fixed `"##..."` label suffixes and
  `ui.indent()`/`unindent()` per level; recursion depth is unlimited), and section headers use fixed
  `###` ids as above.
- An empty key is allowed (the engine accepts it): it shows a faint red "(empty key)" hint, not an error.
- Serde shape is pinned by tests: every condition/effect variant the converters and defaults can produce
  serialises to the engine's internally-tagged snake_case JSON (`{"type":"flag","key":""}`,
  `{"type":"integer","key":"","op":"eq","value":0}`, `{"type":"not","cond":{...}}`,
  `{"type":"all","all":[...]}`, `{"type":"set_scalar","key":"","value":0.0}`, ...) and nested conditions
  round-trip through `serde_json` unchanged; a save-path test builds a doc with a 3-level nested
  condition, every effect kind and on-enter effects, and checks `engine_gate` is Ok,
  `to_def` -> `serde_json::to_string_pretty` -> `from_str::<DialogueDef>` equals the def, and
  `DialogueTree::try_from` of it succeeds.

## Problems panel, status bar, typed notifications (plan 04 step 3)

- `Workspace.notifications` is a queue of typed `Notification { kind, message }` (`src/model/notify.rs`,
  exported from `model/mod.rs`): `NotificationKind::Error` for plain failures (io/json/already-open — the
  `Display` text of the `OpError`) and `NotificationKind::SaveRejected` for `OpError::Invalid`, the engine
  save-gate rejection, carrying ONLY the engine's own message. `ops::error_notification(&OpError) ->
  Notification` builds them; the push sites are `scenes/editor/actions.rs` (Save), `scenes/editor/modals.rs`
  (CloseSave failure) and `systems/file_dialogs.rs` (open / save-as results; open failures stay
  `NotificationKind::Error` with the raw `Display` text).
- The notification modal in `modals.rs` picks its title/id by kind via the pure `modal_title(kind) ->
  &'static str`: `Error` keeps `"Error##modal"`; `SaveRejected` uses `"Cannot save: the engine would reject
  this dialogue##modal"`, shows the engine message (wrapped) plus the dimmed line "See the Problems panel
  for all issues." and an OK button. Priority order and lock discipline are unchanged; OK still removes
  the first queued notification.
- `scenes/editor/problems.rs` draws the "Problems" window (open by default — the flag is
  `Workspace.problems_hidden`, so the derived `Default` means visible): a header "N errors, M warnings"
  (red with errors, amber with warnings only, green "No problems" when clean), then an Errors group and a
  Warnings group (pure `grouped_rows`: errors first, the lint order kept inside each group;
  `problem_counts` = (errors, warnings)). Each row is a `selectable` with a coloured severity marker; row
  ids come from `push_id_usize(index)` plus a fixed `###row` suffix, never from the message. Clicking a row
  whose node still exists pushes `EditorAction::FocusNode(NodeKey)`; rows without a node or with a stale
  key (e.g. a StartMissing naming a deleted node) are shown disabled. Load warnings sit below in a
  collapsed "Load warnings (N)###loadwarn" header; "No document" (dim) without an open doc. A hidden
  window must not be drawn at all — imgui's `p_open` only shows the close button and never hides a
  window whose flag is already false, so `draw` early-returns on `!snap.visible` (`should_draw`).
- `EditorAction::FocusNode(key)` (applied in `apply_action`): with an active doc and an existing node,
  `doc.select(Some(key))` then `doc.set_camera(graph::center_on(&doc.camera, centre))` where `centre` is
  the midpoint of `graph::node_rect(node)` — view-only: `select` bumps `revision` but never sets `dirty`,
  `set_camera` bumps nothing; unknown key / no doc is a no-op. `center_on` is a pure helper in
  `graph/camera.rs` (target = point, zoom unchanged).
- Visibility of the Problems window: View > "Problems" (`menu_item_config("Problems").shortcut(
  "Ctrl+Shift+P").selected(visible)`, always enabled), the Ctrl+Shift+P shortcut in `shortcuts.rs`
  (pure `ctrl_shortcuts`, gated on `!want_text_input` like the rest) and the window's title-bar close
  button all push `EditorAction::ToggleProblems`, which flips `ws.problems_hidden`.
- `scenes/editor/status.rs` draws the status bar: a borderless window pinned to the bottom of the display
  (the tab-strip window flags, position/size with `Condition::Always`, one line + padding high) showing
  the path or "Untitled" with a `*` when dirty, "E errors / W warnings" coloured red/amber/green,
  "N nodes" and "zoom 100%"; "No document" without a doc. The strings come from the pure, unit-tested
  helpers `file_text` / `problems_text` / `nodes_text` / `zoom_text`.

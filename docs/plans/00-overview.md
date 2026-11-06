# AberredDialogueEditor — overview (READ FIRST, every session)

A visual node-graph editor for AberredEngine dialogue JSON files. Rust-only app on the `aberredengine` library
(git, branch `develop-0.2.0`; local checkout `/home/idaho/Projects/aberredengine`). imgui provides menus, tabs,
inspector and validation panel. The graph itself is drawn as world-space engine entities (`Shape` + `DynamicText`).
The editor loads any dialogue JSON, auto-lays it out, edits it, and saves JSON the engine loads unchanged.

Reference app for patterns: `/home/idaho/Projects/aberrededitor` (branch `develop-0.2.0`), a working map editor on
the same engine. Its `CLAUDE.md` and `docs/` are the best guide to engine-app conventions (some of its docs are
stale; see "Engine facts").

## Plan files

| File | Run in | Depends on |
|---|---|---|
| `00-overview.md` | — | — |
| `01-engine-shapes.md` | `/home/idaho/Projects/aberredengine` | — |
| `02-editor-foundation.md` | `/home/idaho/Projects/aberreddialogueeditor` | — (parallel with 01) |
| `03-editor-graph-view.md` | `/home/idaho/Projects/aberreddialogueeditor` | 01 pushed to GitHub + 02 done |
| `04-editor-inspector-validation.md` | `/home/idaho/Projects/aberreddialogueeditor` | 03 done |

Plan 01 lives in this repo's `docs/plans/` but is executed in the engine repo. Every plan file starts with an
absolute `Run in:` path.

## PROJECT POLICIES (non-negotiable; every plan file repeats them)

### COMMIT POLICY (applies in both repos)
Small, meaningful commits. After a step is verified, Claude stages exactly the files that belong to it
(`git add <explicit paths>`, never blind `git add -A`), then proposes a **one-line** commit message with **no
`Co-Authored-By` trailer** (this overrides any default attribution), and **STOPS**. The user runs `git commit`.
Claude continues only after the user says it is committed. Claude never runs `git commit` or `git push`.

### VIBE POLICY
Use the vibe MCP (`vibe_submit`, `vibe_wait`, `vibe_result`, `vibe_apply`, `vibe_cancel`) as much as possible for:
implementation, code fixes, test creation, code simplification (`skill="simplify"`) and code review
(`skill="code-review"`). Claude models do reasoning, planning, orchestration, debugging and direction: writing
specs, reading `vibe_result(diff=true)`, `vibe_apply` (stages, never commits), running the acceptance recipe
themselves, diagnosing failures, writing fix specs. Claude implements directly only if the user explicitly asks.

Rules for running a vibe step:
- Vibe sees neither the conversation nor any docs. Pass the step's **spec block verbatim** as `spec`, and always
  set `scope` to the step's scope.
- A vibe worktree starts at committed `HEAD`. Start a step's job only after the previous step was committed by
  the user. Chain with `base_job` only inside one step (e.g. 4a → 4b).
- Vibe's summary is unverified; the server never runs tests. Always read `vibe_result(diff=true)`, run
  `vibe_apply`, then run the acceptance recipe yourself. If it fails, write a precise fix spec and submit another
  vibe job (do not hand-edit).
- Independent steps with disjoint scopes may be submitted together, then waited on once.
- If the vibe MCP is not connected, STOP and tell the user (they will fix it). Do not silently implement.

### JUSTFILE
Every acceptance command is a `just` recipe. The editor repo gets a `justfile` in plan 02 step 1
(`check` = `cargo fmt --check` + `cargo clippy --all-targets -- -D warnings` + `cargo test`; also `test`, `lint`,
`run`). The engine already has `just check` (full, slow) and `just test-fast`.

## Decisions (fixed; do not re-litigate without asking the user)

- **Format:** the saved file is exactly the engine's `DialogueDef` serialized with
  `serde_json::to_string_pretty`. No extra fields, no position persistence. The editor depends on `serde` and
  `serde_json` itself (the facade does not re-export them).
- **Layout:** auto-layout on load; View → "Re-layout" re-runs it. Positions are never saved.
- **Node sizing ("option A"):** fixed width, fixed header and text area, `height = HEADER + TEXT_AREA +
  max(1, rows) × ROW`, where `rows` = number of choices (1 for a `next`/end node). No text-driven auto-sizing.
  Text is word-wrapped and truncated with `"..."` editor-side using `FontMetricsStore`.
- **Inspector:** a side panel does ALL editing (id, speaker, text, labels, on_enter effects, next/choices, each
  choice's text, target, `when` condition, effects, labels). Choices can be reordered (up/down) in v1.
- **Edges:** cubic bezier with an arrowhead. NO edge hit-testing. A link is removed by dragging it off its output
  pin onto empty space, or by choosing "(end)" in the inspector's target combo.
- **Start node:** "Set as start" (inspector and context menu); drawn with a distinct header colour. The first node
  created in an empty doc becomes the start.
- **Adding nodes:** right-click context menu on the canvas AND Edit → Add node. New nodes get a unique `node_N`
  id. Dropping a dragged link on empty space does NOT create a node.
- **Validation:** a live linter lists ALL problems. The engine's `DialogueTree::try_from(DialogueDef)` gates every
  save and is never reimplemented.
- **Multiple dialogues:** imgui tabs. No undo/redo. No preview/play mode.
- **File dialogs:** copy and adapt `aberrededitor/src/systems/file_dialogs.rs` (rfd 0.17 + futures +
  crossbeam-channel).
- **Engine first:** Bezier, line thickness and RoundedRect are added to the engine (plan 01) with full Lua parity
  and docs. The user pushes `develop-0.2.0`; then the editor runs `cargo update -p aberredengine`.
- **Engine types are the data model:** avoid drift. Convert to/from them only at load/save (thin wrapper).

## Architecture

```
render thread (GuiCtx fn pointer)           shared, in AppState            logic thread (systems)
 imgui: menu, tabs, inspector, ──lock──►  Arc<Mutex<Workspace>>  ◄──lock── graph_input_system (mouse gestures)
 validation, modals, shortcuts                                              graph_projection_system (entities)
 writes want_capture flags ─────────────► Arc<UiCapture>(AtomicBool×2) ───► gates world mouse/keyboard
```

- **`Workspace`** (`src/model/`) is the single source of truth: a plain Rust struct, no ECS, fully unit-testable.
  - `Workspace { docs: Vec<DialogueDoc>, active: usize }`
  - `DialogueDoc { nodes: Vec<EdNode>, start: Option<NodeKey>, path: Option<PathBuf>, dirty: bool,
    revision: u64, selected: Option<NodeKey>, camera: CamState, load_warnings: Vec<String>, next_key: u64,
    lint_cache: (u64, Vec<Problem>) }`
  - `EdNode { key: NodeKey(u64), id: String, speaker: Option<String>, text: String, labels: Vec<String>,
    on_enter: Vec<DialogueEffect>, out: Outputs, pos: Vec2 }`
  - `Outputs::Next(Link) | Outputs::Choices(Vec<EdChoice>)`;
    `EdChoice { text, target: Link, when: Option<DialogueCondition>, effects: Vec<DialogueEffect>, labels }`
  - `Link::End | Link::Node(NodeKey) | Link::Dangling(String)`
  - Every mutation goes through `DialogueDoc` methods that bump `revision` and set `dirty`.
- **GUI** (render thread) locks the workspace briefly per panel and mutates it directly. There is no
  signal/pending machinery: an `Arc<Mutex<T>>` inside `AppState` is shared (not cloned) with the render copy. The
  GUI publishes `want_capture_mouse` / `want_capture_keyboard` into `UiCapture` atomics each frame; logic input
  systems ignore the mouse/keyboard when they are set. Never hold a lock across a whole panel or while calling
  rfd.
- **`graph_input_system`** (logic): reads `InputState`, feeds a pure, tested `interaction` state machine
  (Idle / DraggingNode / DraggingLink / Panning), applies the resulting `Action`s to the workspace. A
  right-click without movement sets `pending_context_menu` (world pos + hit) for the GUI to open a popup.
- **`graph_projection_system`** (logic): compares `(active doc, revision, selection, lint revision)` with what it
  last drew and diffs entities. Each node is a root entity (`WorldPosition`, `ZIndex`) with `ChildOf` children at
  local offsets (body rect, header rect, texts, pins). Edge entities sit at the source pin; their bezier control
  points are local to it, so moving either endpoint rewrites the edge's `WorldPosition` and `Shape` in place.
- **Camera:** `MainCamera` set directly each tick (CameraFollow disabled). Each doc stores its camera.
- **Config:** `[render]` size = `[window]` size, `pixel_snap_camera=false`, `[simulation] hz=60`,
  `target_fps=60`, `vsync=true`. A `WindowResized` observer calls `GameConfig::set_render_size` and recentres the
  camera offset (copy `aberrededitor/src/systems/window_resize.rs`). Then mouse coords are 1:1.
- **Shortcuts** (Ctrl+N/O/S/Shift+S/W, Delete, F = frame all) are read in the GUI with imgui key state, gated on
  `!want_capture_keyboard`.

## Engine facts (verified against `develop-0.2.0` @ 6dd7510; paths relative to `/home/idaho/Projects/aberredengine`)

**GUI and app shell**
- `GuiCtx` (`crates/aberred-render/src/resources/scene_table.rs:34`, `#[non_exhaustive]`): `ui: &Ui, signals:
  &SignalSnapshot, intents: &mut SignalIntents, textures: &TextureStore, fonts: &FontStore, app_state:
  &AppState`. `add_scene_gui(scene, fn(&mut GuiCtx))` and `add_scene_world_draw(scene, fn(&mut WorldDrawCtx))`
  take plain `fn` pointers (no captures). `WorldDraw` offers lines only.
- `AppState` (`crates/aberred-core/src/resources/app_state.rs`): values need `Clone + Send + Sync`; the render
  thread gets a read-only clone re-made only when `generation` changes. An `Arc<Mutex<T>>` inside is shared.
  Insert once in `on_setup`; never re-insert per frame. Ask for `ResMut<AppState>` only where needed.
- The engine does NOT mask input for scene GUIs (`crates/aberred-render/src/resources/imgui_bridge.rs:241-270`):
  clicks and keys reach logic even over imgui windows. The editor must gate on its own capture flags.
- `EngineBuilder` (`crates/aberredengine/src/engine_app/builder.rs`): `new, config_str(&'static str),
  on_setup, add_system, add_observer, add_scene, on_scene_enter/exit(scene, observer), add_scene_system,
  add_scene_gui, add_scene_world_draw, initial_scene, try_run`. `add_system`/`add_scene_system` run in
  `SimSet::Update`, after GUI intents are applied. Sim is fixed-step (`hz`).
- Callback styles do not mix: setup/system/observer params are Bevy params by value; scene enter/exit take
  `On<SceneEntered>` first.
- B0002: never request the same resource twice in one system (e.g. a `SystemParam` bundle containing
  `ResMut<AppState>` plus a separate `Res<AppState>`); it panics at startup and `cargo check` cannot see it.
- Render-only resources (`TextureStore`, `FontStore`, `ShaderStore`, `RaylibAccess`) must never appear in logic
  systems/observers: panic at schedule init, invisible to `cargo check`. Use `RenderAssetCmd` messages. The GUI
  callback may use `ctx.fonts` / `ctx.textures`.
- `intents.request_quit()` quits from the GUI.

**Input and camera**
- `InputState` (`crates/aberred-core/src/resources/input.rs:162`): `mouse_x/mouse_y` (render-target px),
  `mouse_world_x/mouse_world_y` (computed with the camera as it was BEFORE this tick's systems ran),
  `scroll_y` (per-tick sum), `mouse_left_button: BoolState{active,just_pressed,just_released}` (raw; use it for
  clicks), `action(InputAction)`.
- There is no raw right/middle button: `Action2` = Enter OR right mouse; `Action3` = middle mouse
  (`input_bindings.rs:345-349`). In scene enter, rebind `Action2` to right mouse only through
  `ResMut<InputBindings>` (`rebind(InputAction::Action2, InputBinding::MouseButton(..))`; see
  `aberrededitor/src/scenes/editor/lifecycle.rs:20-54` for the exact call pattern for Action1).
- No modifier keys or Delete on the logic side: shortcuts live in the GUI (imgui key state).
- `aberredengine::core::systems::input::screen_to_world2d(pos, &Camera2D)`. `MainCamera(pub Camera2D)`;
  `Camera2D { target, offset, rotation, zoom }` (`#[non_exhaustive]`; `Camera2D::new(target, offset)`). Set
  `ResMut<CameraFollowConfig>.enabled = false` explicitly in scene enter. Pan with screen-pixel deltas ÷ zoom
  (not `mouse_world_*`, which lags the camera).
- `WindowResized { w, h }` at `aberredengine::core::events::window::WindowResized` (not in the prelude).
  `ResMut<GameConfig>::set_render_size(w, h)` rebuilds the render target.

**Fonts**
- `aberredengine::core::resources::font_metrics::{FontMetricsStore, FontMetrics, FontMetricsWarnCache}` (not in
  the prelude); `PendingAssets` is in the prelude. `FontMetricsStore(pub FxHashMap<String, FontMetrics>)`.
  `store.resolve_key(font, &pending, &mut warn_cache) -> &str`, then
  `store.0.get(key)?.measure_text(text, font_size) -> Vec2` (handles `\n`, line step = size + 2). Default font key
  `"default"`. Metrics arrive asynchronously: tolerate `None` and retry next tick.
- `DynamicText::new(content, font, font_size, color)`; `\n` is supported; there is NO word wrap and no
  clipping. World `DynamicText` ignores `Scale`/`Rotation`; `font_size` is in world units (scales with zoom).

**Entities and drawing**
- `Shape` (`crates/aberred-core/src/components/shape.rs`): `Shape::rect(size, color)`, `circle`, `line(to, c)`,
  `polygon(&[Vec2], c)`, `.filled()`, `.with_outline_color(c)`, `.with_origin(v)`. Needs `ZIndex` plus
  `WorldPosition`. Today lines/outlines are 1 px (plan 01 adds thickness, Bezier, RoundedRect).
- One sorted pass for Shape/Sprite/DynamicText by ascending `ZIndex`; text wins ties. An entity without
  `ZIndex` is not drawn.
- `ChildOf(parent)`: a child's `WorldPosition` is local to the parent; despawning the parent cascades.
- Scene switch despawns everything not `Persistent`. For manual cleanup use
  `aberred_core::components::persistent::SceneCleanup::despawn_all`, not a bare `Without<Persistent>` query
  (it also matches Bevy's internal resource entities).
- `DynamicText::size()` is 0 until `dynamic_text_size_system` runs (next tick).

**Dialogue types** (`aberredengine::prelude::{DialogueDef, NodeDef, ChoiceDef, DialogueCondition, DialogueCmp,
DialogueEffect, DialogueTree, DialogueError, load_dialogue}`)
- `DialogueDef { start: String, nodes: Vec<NodeDef> }`; `NodeDef { id, speaker: Option<String>, text, labels:
  Vec<String>, on_enter: Vec<DialogueEffect>, next: Option<String>, choices: Option<Vec<ChoiceDef>> }`;
  `ChoiceDef { text, next: Option<String>, when: Option<DialogueCondition>, effects: Vec<DialogueEffect>,
  labels: Vec<String> }`. All `Debug, Clone, PartialEq, Serialize, Deserialize`; none `Default`; no
  `deny_unknown_fields` (unknown JSON keys are ignored). `DialogueCmp` is also `Copy, Eq`.
- `DialogueCondition` (serde `tag="type"`, snake_case): `Flag{key}`, `Integer{key,op,value:i32}`,
  `StringEquals{key,value}`, `Not{cond:Box<_>}`, `All{all:Vec<_>}`, `Any{any:Vec<_>}`. `DialogueCmp`:
  `Eq Ne Lt Le Gt Ge`. `DialogueEffect` (same tagging): `SetFlag{key}`, `RemoveFlag{key}`,
  `SetInteger{key,value:i32}`, `AddInteger{key,delta:i32}`, `SetScalar{key,value:f32}`, `SetString{key,value}`.
  Verify exact field names in `crates/aberred-core/src/resources/dialogue_def.rs` before use.
- `impl TryFrom<DialogueDef> for DialogueTree` (`dialogue_tree.rs:218`) reports only the FIRST error, in order:
  duplicate node id → start missing → per node: next+choices, empty choices list (`Some(vec![])`), unknown
  `next`, then each choice's unknown `next`. Unreachable nodes are only a warning. Loops/joins are legal. It
  consumes the def. `load_dialogue(path)` = read + serde + try_from.
- `DialogueError` Display: `StartMissing` → `dialogue start "{start}" is not among the defined nodes`;
  `DuplicateNode` → `duplicate dialogue node id "{id}"`; `UnknownTarget` → `dialogue node "{node}" links to
  unknown node "{target}"`; `NextAndChoices` → `... sets both next and choices; pick one`; `EmptyChoices` →
  `... has an explicit empty choices list`; `Io`, `Json`. Not `Clone`/`PartialEq`.
- A choice whose `next` is `None` ends the conversation. A node with neither `next` nor `choices` is terminal.
- There is no reverse conversion `DialogueTree → DialogueDef`; keep the def-shaped model (`Workspace`).

## Stale-doc warnings (aberrededitor)
Its `CLAUDE.md` names branch `dev-rust-api-ecs` (real: `develop-0.2.0`); `docs/architecture.md` and
`docs/recipes/add-panel.md` show an old six-parameter `editor_gui` (real: `fn(&mut GuiCtx)`); `.serena/memories/*`
predate the three-thread engine. Trust the engine source over those docs.

## End-to-end verification (final checklist lives in plan 04)
`just check` in both repos; manual run: open `tests/fixtures/guard.json`, see the auto-layout, drag/connect/
disconnect, reorder choices, edit a condition, save, reload in a second tab; the saved file passes
`aberredengine::prelude::load_dialogue` (covered by a test); a duplicate id shows a lint error and blocks the save
with the engine's message.

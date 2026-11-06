# Plan 03 — Graph view: layout, drawing, camera, mouse interaction

**Run in:** `/home/idaho/Projects/aberreddialogueeditor`. Read `docs/plans/00-overview.md` first.
**Depends on:** plan 02 complete (done) AND plan 01 pushed (done: the editor's `Cargo.lock` pins engine `0547e57`, which
has `Shape::bezier`, `Shape::rounded_rect`, `Shape::with_thickness`, `add_late_system`, `Camera2D::with_zoom`).
**Done when:** opening `tests/fixtures/guard.json` shows an auto-laid-out graph; nodes can be selected, dragged,
connected, disconnected, added and deleted with the mouse; pan/zoom/frame-all work; `just check` green; all steps
committed by the user.

**Revision note (2026-11-06, after plan 02 was executed).** This plan was re-cut into smaller vibe jobs because
jobs of 15-20 minutes were close to the time limit. Corrections versus the first draft: step 0 (engine pickup) is
obsolete; tests live in `#[cfg(test)]` modules (the crate is a binary, `tests/*.rs` cannot import its modules);
`Workspace` lives in `src/model/doc.rs`; `Slot` must derive `Hash`; the context-menu request must not reference
graph types from the model; every spec carries the logging rules.

## Policies (verbatim from 00-overview.md — obey them)

- **COMMIT POLICY:** small, meaningful commits. After a verified step, stage exactly its files
  (`git add <explicit paths>`), propose a ONE-LINE commit message, NO `Co-Authored-By` trailer, and STOP. The user
  runs `git commit`; continue only after they confirm. Never run `git commit` / `git push`.
- **VIBE POLICY:** implementation, fixes, test creation, simplification, code review go to the vibe MCP
  (`vibe_submit`/`vibe_wait`/`vibe_result(diff=true)`/`vibe_apply`). Claude writes specs, reviews diffs, runs the
  acceptance recipe, debugs, stages. Claude implements directly only if the user explicitly asks. Pass the step's
  spec block verbatim as `spec`; always set `scope`. A vibe worktree starts at committed HEAD, so start a step only
  after the previous one is committed. If vibe is not connected, STOP and tell the user.
- **Acceptance** is always a `just` recipe run by Claude itself after `vibe_apply`.

## Lessons from plan 02 (apply to every job here)

- Vibe's sandbox CANNOT read `/home/idaho/Projects/aberrededitor`. Never tell a spec to read it; embed any reference
  code the job needs. It CAN read the engine (`/home/idaho/Projects/aberredengine` or
  `~/.cargo/git/checkouts/aberredengine-*/0547e57/`) and the cargo registry sources.
- `vibe_submit(base_job=...)` is rejected once the base job was `vibe_apply`'d. Commit first, then start the next job
  (this also applies to simplify passes: commit the step, then run simplify as its own job and commit it).
- Set `time_limit` explicitly: 2400 for normal jobs, 3000 for the jobs marked "long". Typical durations were 7-19 min.
  If a job hits its limit, split it instead of just retrying.
- Tests are `#[cfg(test)]` modules inside the source files. Fixtures load with
  `include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/NAME.json"))`. A shared test scaffold
  (`fixture()`, `TempDir`) exists in `src/scenes/editor/mod.rs` (`testkit`); pure graph tests should build docs with
  `from_def` on the fixtures instead.
- Each job appends a short section (3-6 lines) to `CLAUDE.md` (so later jobs inherit the conventions) and its scope
  must list `CLAUDE.md`.
- Claude's checks after `vibe_apply`: `just check`; for ECS/GUI jobs also `timeout 15 cargo run` (no panic, B0002 and
  render-resource panics are invisible to `cargo check`) and the manual list of the step.

## Conventions for every vibe spec in this plan (paste into each spec)

```
Project: Rust binary crate `aberreddialogueeditor` (edition 2024; engine = aberredengine git dependency, pinned in
Cargo.lock). Read CLAUDE.md and docs/plans/00-overview.md first. Zero clippy warnings (`cargo clippy --all-targets -- -D
warnings`), `cargo fmt` clean, `just check` is the acceptance recipe. Engine imports go through
`aberredengine::prelude::{...}`; `aberredengine::core::...` only for items the prelude lacks. Do not add `bevy_ecs` as a
dependency. Pure modules (src/model, src/graph pure files) must not import ECS/imgui; allowed engine types there: the
dialogue types, `Vec2`, and `Color`. Tests are #[cfg(test)] modules (binary crate). Never request render-only engine
resources (TextureStore, FontStore, ShaderStore, RaylibAccess) in logic systems, and never request ResMut<AppState> or a
second AppState param in one system (B0002): systems take `Res<AppState>` only. LOGGING: use `log::debug!/info!/warn!`
fully qualified; log on EVENTS (rebuild, action applied, link set, node deleted, menu action), never per frame, never
document text. Append a 3-6 line section about this step to CLAUDE.md.
```

## Job list (each job = one commit; order matters)

| Step | Job | Executor | Est. | Commit message |
|---|---|---|---|---|
| 1a | Pure geometry, wrap, camera math, numeric style | vibe | ~10 min | `Add pure graph geometry, text wrap and camera math` |
| 1b | Auto-layout + layout on load | vibe | ~10 min | `Add auto-layout and apply it when opening a dialogue` |
| 1s | simplify `src/graph/**` | vibe | ~8 min | `Simplify graph geometry and layout code` |
| 2a | `doc_id`, `Slot: Hash`, colours/Z, components, pure `plan_projection` + node descriptions | vibe | ~12 min | `Add pure projection planning and node descriptions` |
| 2b | ECS projection system (long) | vibe | ~15 min | `Project dialogue documents into shape and text entities` |
| 3 | Camera system, wheel zoom, pan, Frame all | vibe | ~10 min | `Add per-document camera with wheel zoom and drag pan` |
| 4a | Pure interaction state machine (long) | vibe | ~15 min | `Add pure graph interaction state machine` |
| 4b-1 | Input system, drag preview, `apply_actions` (long) | vibe | ~12 min | `Wire graph mouse input and drag preview` |
| 4b-2 | Context menu, Add node, Delete key | vibe | ~12 min | `Add graph context menu, node creation and deletion` |
| 4s | simplify projection, input, GUI additions | vibe | ~15 min | `Simplify graph projection, input and menu code` |
| 5 | Re-layout, Frame all polish, Help > Controls | vibe | ~8 min | `Enable Re-layout and add the Controls help` |

Optional small job at any point: fix the Save As / Discard index race (see "Open items" at the end).

There is NO step 0: the engine pickup is already done (`Cargo.lock` pins `0547e57`; the APIs below were verified:
`Shape::{bezier, rounded_rect, circle, rect, polygon, filled, with_outline_color, with_thickness}`,
`EngineBuilder::add_late_system`, `Camera2D::{new, with_zoom}`, `AppState::get`, `InputState::{scroll_y,
mouse_left_button, action}`, `FontMetricsStore`/`measure_text`/`resolve_key`).

## Constants (put in `src/graph/style.rs`; every spec below refers to them)

```
NODE_W = 240.0      HEADER_H = 24.0     TEXT_AREA_H = 64.0 (4 lines, FONT 12 => line step 14, 8 px padding)
ROW_H = 22.0        PAD = 8.0           PIN_R = 6.0
FONT = "default"    FONT_SIZE = 12.0    TEXT_MAX_LINES = 4
COL_GAP = 120.0     ROW_GAP = 24.0
node_height(rows) = HEADER_H + TEXT_AREA_H + max(1, rows) * ROW_H     rows = #choices (Next node: 1)
EDGE_THICKNESS = 2.0   OUTLINE_THICKNESS = 2.0   CORNER_RADIUS = 6.0
Colours (RGBA): canvas bg set in config.ini; node body (45,48,58,255); header (70,78,102); start header
(46,125,50); node outline (110,118,140); selected outline (255,217,0); error outline (220,60,60); text
(230,230,230); dim text (150,155,165); edge (160,170,200); edge selected-source (255,217,0); pin out (120,200,255);
pin in (255,170,80); dangling pin (220,60,60).
Z: edges 0, node body 10, header 11, text 12, pins 13, arrowheads 1, drag preview 20.
```
Node geometry (node top-left = `pos`, world coords, y down): body rect `(pos, (NODE_W, node_height))`; header
`(pos, (NODE_W, HEADER_H))`; text area starts at `pos.y + HEADER_H`; row `i` centre y =
`pos.y + HEADER_H + TEXT_AREA_H + (i + 0.5) * ROW_H`; **input pin** at `(pos.x, pos.y + HEADER_H/2)`; **output pin
i** at `(pos.x + NODE_W, row_i_centre_y)`.

---

## Step 1a — Pure geometry, text wrap, camera math

**Executor:** vibe. **Scope:** `src/graph/{mod,style,geometry,wrap,camera}.rs`, `src/main.rs` (`mod graph;`),
`CLAUDE.md`. **Acceptance:** `just check`. **time_limit:** 2400.
**Suggested message:** `Add pure graph geometry, text wrap and camera math`

**Vibe spec (pass verbatim, preceded by the "Conventions" block):**
```
The model (src/model: DialogueDoc, EdNode, Outputs, Link, NodeKey, Slot, CamState ...), linter and ops exist and are
committed. This step adds PURE logic: no ECS, no imgui, no engine systems; only `Vec2` from `aberredengine::prelude`.
Create the module `graph` (src/graph/mod.rs, `mod graph;` in src/main.rs; module-level
`#![allow(dead_code)] // consumed by later plan steps` in src/graph/mod.rs).

CONSTANTS (src/graph/style.rs, `pub const`): NODE_W=240.0, HEADER_H=24.0, TEXT_AREA_H=64.0, ROW_H=22.0, PAD=8.0,
PIN_R=6.0, FONT_SIZE=12.0, TEXT_MAX_LINES=4, COL_GAP=120.0, ROW_GAP=24.0, EDGE_THICKNESS=2.0, OUTLINE_THICKNESS=2.0,
CORNER_RADIUS=6.0, FONT="default". (Colours and Z constants come in a later step.)

1. src/graph/geometry.rs
 - `rows(node: &EdNode) -> usize` = number of choices for Outputs::Choices, else 1.
 - `node_height(node) = HEADER_H + TEXT_AREA_H + max(1,rows) as f32 * ROW_H`; `node_rect(node) -> (min, max)`.
 - `input_pin(node) -> Vec2` = (pos.x, pos.y + HEADER_H/2); `output_pin(node, slot_index: usize) -> Vec2` =
   (pos.x + NODE_W, pos.y + HEADER_H + TEXT_AREA_H + (slot_index as f32 + 0.5) * ROW_H).
 - `Hit { OutputPin(NodeKey, Slot), InputPin(NodeKey), Body(NodeKey), Empty }` (Debug, Clone, Copy, PartialEq) and
   `hit_test(doc: &DialogueDoc, p: Vec2, pin_hit_radius: f32) -> Hit`: pins take priority over bodies; among
   overlapping nodes the one LATER in `doc.nodes` order wins (it is drawn on top). Output pin slot for a Next node
   is `Slot::Next`, for choices `Slot::Choice(i)`. Callers pass PIN_R + 4 scaled by 1/zoom as the radius.
 - `edge_curve(from: Vec2, to: Vec2) -> (c1: Vec2, c2: Vec2, to_rel: Vec2)` returning control points RELATIVE to
   `from` (Shape::bezier is local to its entity): forward case (to.x >= from.x + 20): dx = max(60, (to.x -
   from.x)/2), c1=(dx,0), c2=(to-from)-(dx,0). Back/self case (to.x < from.x + 20): dx = 80, h = 80 + |dy|/2
   (dy = to.y - from.y): c1=(dx, h), c2=(to-from)+(-dx, h). Both return `to-from` as the endpoint.
 - `arrow_tip_dir(c2_rel: Vec2, to_rel: Vec2) -> Vec2` = normalized (to - c2) (the tangent at t=1; fall back to
   (1,0) when zero length).
 - `graph_bounds(doc) -> Option<(Vec2, Vec2)>` union of node rects.
2. src/graph/wrap.rs
 - `wrap_text(text: &str, max_width: f32, max_lines: usize, measure: &dyn Fn(&str) -> f32) -> Vec<String>`:
   greedy word wrap on spaces; explicit '\n' in the text forces a break; a word wider than max_width is
   hard-broken by characters; when the text needs more than max_lines lines, the last line is truncated and ends
   with "..." (ASCII three dots) so that it still fits max_width (drop characters until it fits). Empty text ->
   empty vec. Characters are `char`s, not bytes (no panic on non-ASCII).
 - `fit_line(text: &str, max_width: f32, measure) -> String`: single line truncated with "..." to fit.
3. src/graph/camera.rs (all pure; `CamState {target: Vec2, zoom: f32}` is in the model; viewport = window size in
   screen pixels as Vec2; the camera shows world point `target` at the screen centre; screen = (world - target) *
   zoom + viewport/2):
 - `screen_to_world(cam, viewport, p) -> Vec2` and `world_to_screen(...)` (inverses; test the round trip).
 - `zoom_at(cam, viewport, cursor_screen, factor) -> CamState`: new zoom = clamp(zoom*factor, 0.1, 4.0); the world
   point under the cursor stays under the cursor.
 - `pan_by_screen_delta(cam, delta_screen) -> CamState` (target -= delta/zoom).
 - `frame_all(bounds: Option<(Vec2,Vec2)>, viewport, margin_px: f32) -> CamState` (None -> target ZERO, zoom 1.0;
   zoom clamped to [0.1, 1.0] so small graphs are not magnified; centred on the bounds).

TESTS FIRST (#[cfg(test)] modules in geometry.rs, wrap.rs, camera.rs; build docs with `from_def` on
tests/fixtures/guard.json via include_str!): pin positions for a Next node and a 3-choice node; hit_test priority (pin
over body, topmost wins, Empty); edge_curve forward and back cases (endpoint equals `to - from`; back case control
points are below both pins); wrap: width, newline, long word, truncation "...", non-ASCII, empty; camera: round trips,
zoom_at fixed point, pan, frame_all contains the bounds.
CONSTRAINTS: no layout yet; no ECS/imgui. DONE WHEN: `just check` passes. Report the first failing test.
```

**Claude review checklist:** geometry matches the constants block exactly (it is the contract for the later steps);
`edge_curve` is relative to the source pin; wrap is `char`-safe; no engine imports besides `Vec2`.

---

## Step 1b — Auto-layout and layout on load

**Executor:** vibe (after 1a is committed). **Scope:** `src/graph/{mod,layout}.rs`, `src/ops/mod.rs`, `CLAUDE.md`.
**Acceptance:** `just check`. **time_limit:** 2400. **Suggested message:**
`Add auto-layout and apply it when opening a dialogue`

**Vibe spec (pass verbatim, preceded by the "Conventions" block):**
```
Existing and committed: model, linter, ops, and the pure graph module (src/graph/{style,geometry,wrap,camera}.rs).

1. src/graph/layout.rs: `auto_layout(doc: &mut DialogueDoc)`: layered left-to-right layout. Algorithm:
   a) layer(node) = shortest-path depth from `doc.start` (BFS over Link::Node edges); BACK edges (target depth <=
      source depth) are ignored for ordering. Nodes not reachable from start (or all nodes when start is None)
      go to a trailing set: BFS from each remaining node in node order, appended as extra layers AFTER the last
      reachable layer (so unreachable nodes are in their own columns, deterministic).
   b) within a layer order nodes by barycenter of their predecessors' order in the previous layer (two sweeps
      forward, ties broken by node order); initial order = node order.
   c) x = layer * (NODE_W + COL_GAP); y stacked within the layer with ROW_GAP between node heights (use
      geometry::node_height); each layer is vertically centred around y = 0 relative to the tallest layer.
   Write positions with `doc.set_node_pos` (it bumps `revision` but not `dirty`; layout must NOT make a doc dirty).
   Must be deterministic and must terminate on cycles/self-loops.
2. In `ops::open_path`, call `auto_layout` on the new doc right before it is pushed (Opened outcome only, never for
   Focused). Keep the ops change minimal and keep every existing test passing (62 tests in the whole crate today).
TESTS FIRST (#[cfg(test)] in layout.rs, plus one new ops test): guard.json - start in layer 0 (smallest x), no two
node rects overlap, deterministic (laying out twice gives equal positions), loops.json terminates, unreachable nodes
land right of all reachable ones, empty doc and single node do not panic, auto_layout leaves `dirty == false`;
ops: opening guard.json gives non-zero distinct positions and `dirty == false`.
CONSTRAINTS: no ECS/imgui. DONE WHEN: `just check` passes. Report the first failing test.
```

**Claude review checklist:** deterministic; unreachable nodes in trailing columns; `dirty` stays false; Focused path
does not re-layout.

## Step 1s — Simplify the pure graph module

**Executor:** vibe, skill `simplify`, after 1b is committed. **Scope:** `src/graph/**`. **time_limit:** 1800.
Spec: simplify only src/graph (and the layout hook in `ops::open_path` if redundant), no behaviour/API change, test
count not lower (record the count before). **Suggested message:** `Simplify graph geometry and layout code`

---

## Step 2a — Projection planning (pure) and node descriptions

**Executor:** vibe (after 1s is committed). **Scope:** `src/model/doc.rs`, `src/graph/{mod,style,components,
projection_plan,describe}.rs`, `CLAUDE.md`. **Acceptance:** `just check`. **time_limit:** 2400.
**Suggested message:** `Add pure projection planning and node descriptions`

**Vibe spec (pass verbatim, preceded by the "Conventions" block):**
```
Existing and committed: model, linter, ops, pure graph module (src/graph/{style,geometry,wrap,layout,camera}.rs). The
next step (not this one) adds the ECS system that spawns entities; THIS step prepares everything that can be tested
without an ECS or fonts.

1. src/model/doc.rs: derive `Hash` on `Slot`; add `doc_id: u64` to DialogueDoc (unique per doc; add
   `next_doc_id: u64` to `Workspace` and assign `doc_id` in `Workspace::add_doc`). Keep every existing test passing.
2. src/graph/style.rs: add the colour constants (engine `Color` values; check the Color type in the prelude and its
   const constructor) and Z constants from this plan's "Constants" block:
   node body (45,48,58,255); header (70,78,102); start header (46,125,50); node outline (110,118,140); selected
   outline (255,217,0); error outline (220,60,60); text (230,230,230); dim text (150,155,165); edge (160,170,200);
   edge selected-source (255,217,0); pin out (120,200,255); pin in (255,170,80); dangling pin (220,60,60).
   Z: edges 0, arrowheads 1, node body 10, header 11, text 12, pins 13, drag preview 20.
3. src/graph/components.rs: marker components `GraphNode { key: NodeKey }`, `GraphEdge { from: NodeKey, slot: Slot }`,
   `GraphPart` (all non-root visual children and arrowheads), `GraphPreview` (the drag-preview entity used later), all
   `#[derive(Component)]` (find how the prelude exports Component), and the resource `GraphEntities` (derive Resource)
   holding `shown_doc: Option<u64>`, `shown_revision: u64`, `metrics_ready: bool`, `nodes: FxHashMap<NodeKey,
   NodeEntry { root: Entity, sig: NodeSig, pos: Vec2 }>`, `edges: FxHashMap<(NodeKey, Slot), EdgeEntry { entity:
   Entity, arrow: Entity, from: Vec2, to: Vec2, selected_source: bool, target: NodeKey }>` (Default).
4. src/graph/describe.rs (PURE data, no ECS): `describe_node(node: &EdNode, is_start: bool, is_selected: bool,
   has_error: bool, measure: &dyn Fn(&str) -> f32) -> NodeVisual` where `NodeVisual { size: Vec2, body_color: Color,
   outline_color: Color, outline_thick: bool, header_color: Color, texts: Vec<TextItem { text: String, local: Vec2,
   color: Color }>, pins: Vec<PinItem { local: Vec2, color: Color, is_output: bool }> }`. Content (all local to the
   node's top-left): header text = node id (`fit_line` to NODE_W - 2*PAD) at (PAD, 4); text area = `wrap_text("<speaker>:
   <text>" when speaker is Some, else text, NODE_W - 2*PAD, TEXT_MAX_LINES, measure).join("\n")` at
   (PAD, HEADER_H + 4); rows: for row i a TextItem at (PAD, HEADER_H + TEXT_AREA_H + i*ROW_H + 4): the choice text
   `fit_line`d to NODE_W - 2*PAD - 2*PIN_R - 8 with suffix " ?" when `when` is Some and " !" when effects are
   non-empty, or "next" / "(end)" in the dim colour for a Next node; input pin at geometry::input_pin(node) - node.pos
   (pin-in colour), output pins at geometry::output_pin(node, i) - node.pos (pin-out colour, dangling colour for a
   `Link::Dangling` target). Outline: selected > error > normal; `outline_thick` only when selected or error.
5. src/graph/projection_plan.rs (PURE): `NodeSig` (Clone, PartialEq): (id, speaker, text, labels, per-row text and
   target kind, is_start, is_selected, has_error, metrics_ready); `Snapshot { doc_id: u64, metrics_ready: bool, nodes:
   Vec<NodeView { key, sig: NodeSig, pos: Vec2 }>, edges: Vec<EdgeView { from_key: NodeKey, slot: Slot, target: NodeKey,
   from: Vec2, to: Vec2, selected_source: bool }> }`; `snapshot_of(doc: &mut DialogueDoc, metrics_ready: bool) ->
   Snapshot` (uses `doc.problems()` for has_error: an Error whose `node` is that node; edges only for Link::Node
   targets that exist; from/to are WORLD positions of the output pin / target input pin); and
   `plan_projection(prev: &Snapshot-like state, next: &Snapshot) -> Vec<ProjOp>` where the previous state is what the
   ECS side remembers (design a small `Shown` struct mirroring Snapshot without Entities) and
   `ProjOp { DespawnAll, SpawnNode(NodeKey), RespawnNode(NodeKey), MoveNode(NodeKey, Vec2), DespawnNode(NodeKey),
   SpawnEdge(NodeKey, Slot), RewriteEdge(NodeKey, Slot), DespawnEdge(NodeKey, Slot) }`. Rules: different `doc_id` (or no
   previous) -> `[DespawnAll, then Spawn* for everything]`; node new -> SpawnNode; sig changed -> RespawnNode; only pos
   changed -> MoveNode; node gone -> DespawnNode and DespawnEdge for every edge touching it; edge new -> SpawnEdge;
   from/to/selected_source changed -> RewriteEdge (NEVER respawn an edge because a node moved); edge gone ->
   DespawnEdge. Identical snapshots -> empty plan. Deterministic order (nodes in doc order, then edges).
TESTS FIRST (#[cfg(test)]): describe_node for a Next node and a 3-choice node (texts, pin positions equal
geometry minus pos, dangling pin colour, suffixes, outline priority, start header colour); plan_projection: moving a
node yields only MoveNode + RewriteEdge ops (no spawns), renaming a node respawns just that node, deleting a node
despawns it and its edges, switching doc_id rebuilds all, selecting a node changes only that node's sig and its edges'
colour, identical snapshots give no ops, closing the active doc (empty next) despawns everything; Slot is Hashable.
CONSTRAINTS: no ECS systems, no imgui, no render resources. Do not change model semantics beyond `doc_id`/`Hash`.
DONE WHEN: `just check` passes. Report the first failing test and any deviation.
```

**Claude review checklist:** pure (no `Entity` in the planning types besides the components/resource file); edges are
rewritten not respawned on node moves; `describe_node` positions match the constants block.

---

## Step 2b — ECS projection system (long)

**Executor:** vibe (after 2a is committed). **Scope:** `src/graph/{mod,projection}.rs`,
`src/scenes/editor/lifecycle.rs`, `src/main.rs`, `CLAUDE.md`. **time_limit:** 3000.
**Acceptance:** `just check`; `timeout 15 cargo run` shows no panic; then manual: open `guard.json`, nodes, text,
pins and arrows appear; Open another file switches graphs with no leftover entities (debug log
`projection: N nodes, M edges` with `RUST_LOG=aberreddialogueeditor=debug`).
**Suggested message:** `Project dialogue documents into shape and text entities`

**Vibe spec (pass verbatim, preceded by the "Conventions" block):**
```
Existing and committed: model, linter, ops, pure graph module incl. src/graph/{describe,projection_plan,components}.rs
(snapshot_of, plan_projection, ProjOp, describe_node, GraphEntities, GraphNode, GraphEdge, GraphPart). The engine has
Shape::bezier(c1, c2, to, color), Shape::rounded_rect(size, radius, color), Shape::circle, Shape::polygon,
.filled(), .with_outline_color(c), .with_thickness(t) (see crates/aberred-core/src/components/shape.rs in the engine,
read the docs on thickness semantics).

GOAL: a logic system that executes the projection plan: it draws the ACTIVE document as world-space entities and
keeps them in sync.

`pub fn graph_projection_system(mut commands: Commands, mut ents: ResMut<GraphEntities>, app_state: Res<AppState>,
metrics: Res<FontMetricsStore>, pending: Res<PendingAssets>, mut warn: ResMut<FontMetricsWarnCache>, mut shapes:
Query<&mut Shape>, mut positions: Query<&mut WorldPosition>)` (FontMetricsStore, FontMetricsWarnCache live in
`aberredengine::core::resources::font_metrics`; PendingAssets is in the prelude; take ONLY `Res<AppState>`, never
ResMut). Register it with `add_late_system` in src/main.rs and insert `GraphEntities::default()` at scene enter
(lifecycle.rs). Per tick:
 - Get the workspace with `app_state.get::<WorkspaceMutex>()`, lock it, and if there is no active doc plan against an
   empty snapshot (despawn everything); otherwise `snapshot_of(doc, metrics_ready)` plus, for spawns, the
   `describe_node` data and the edge curve data you need (clone what you need), then UNLOCK before touching commands.
   Cheap path: when doc_id, revision and metrics readiness are unchanged since the last tick, return immediately.
 - `measure` = `metrics.0.get(key)?.measure_text(s, FONT_SIZE).x` with `key = metrics.resolve_key(FONT, &pending,
   &mut warn)`. If metrics are not available yet, `metrics_ready = false`: spawn nodes without texts and respawn them
   once metrics arrive (metrics_ready is part of the node sig).
 - Execute the ProjOps. Node root = `(GraphNode{key}, WorldPosition(pos), ZIndex(...))`; children use `ChildOf(root)`
   with LOCAL WorldPosition and `GraphPart`: body = `Shape::rounded_rect((NODE_W, node_height), CORNER_RADIUS,
   body_color).filled().with_outline_color(outline_color)` (+ `.with_thickness(OUTLINE_THICKNESS)` only when
   `outline_thick`), ZIndex body; header = `Shape::rect((NODE_W, HEADER_H), header_color).filled()`; every TextItem =
   `DynamicText::new(text, FONT, FONT_SIZE, color)` at its local position (text Z); every PinItem = `Shape::circle(PIN_R,
   color).filled()` (pin Z). Edge entity = `(GraphEdge{from, slot}, WorldPosition(from), ZIndex(edge Z),
   Shape::bezier(c1, c2, to_rel, color).with_thickness(EDGE_THICKNESS))` from `geometry::edge_curve`; colour = edge
   selected-source colour when `selected_source`; plus ONE arrowhead child (`GraphPart`, ChildOf(edge)):
   `Shape::polygon` triangle with the tip at `to_rel`, base 10 wide and 12 long behind the tip along
   `geometry::arrow_tip_dir`, filled, arrowhead Z. RewriteEdge rewrites the edge's WorldPosition, Shape AND arrowhead in
   place (assign only when different so change detection stays cheap); never respawn an edge for a node move.
   Despawn with `commands.entity(e).despawn()` (cascades to ChildOf children). MoveNode updates the root's WorldPosition.
 - Dangling links draw no edge (their pin is red). Update `GraphEntities` bookkeeping (Entities, sigs, positions).
 - `log::debug!("projection: {} nodes, {} edges", ..)` on each plan that changes anything.
TESTS: the planning logic is already tested; here only add tests that do not need an ECS (e.g. an edge-geometry helper
if you factor one out). Do not try to fake font metrics. The system is verified manually.
CONSTRAINTS: no input handling, no imgui, no model changes. Do not request any render-only resource.
DONE WHEN: `just check` passes AND `timeout 15 cargo run` starts without panicking. Report any deviation.
```

**Claude review checklist:** only `Res<AppState>`; edges rewritten not respawned; `add_late_system`; text and pins are
`GraphPart` children of the node root; no leftover entities after closing a tab (log count).

## Step 3 — Camera system and zoom/pan input

**Executor:** vibe (after 2b is committed). **Scope:** `src/graph/{mod,camera_system,frame}.rs`, `src/model/doc.rs`
(`set_camera`), `src/main.rs`, `src/scenes/editor/{lifecycle,gui,actions,menu,shortcuts}.rs`, `CLAUDE.md`.
**Acceptance:** `just check`; `timeout 15 cargo run`; manual: wheel-zoom at the cursor, middle-drag pan, tab switch
restores each doc's camera, F and View > Frame all fit the graph. **time_limit:** 2400.
**Suggested message:** `Add per-document camera with wheel zoom and drag pan`

**Vibe spec (pass verbatim, preceded by the "Conventions" block):**
```
Existing: pure camera math in src/graph/camera.rs (screen_to_world, zoom_at, pan_by_screen_delta, frame_all), CamState
in each DialogueDoc, UiCapture (UiCaptureHandle) in AppState, MainCamera inserted at scene enter, the projection system.

GOAL: ONE logic system `graph_frame_system` (add_system, runs in Update; later steps extend it with the input handling
so that camera update and input run in a fixed order) that makes MainCamera follow the ACTIVE doc's CamState and
handles wheel zoom and panning. Put the camera part in a plain function `camera_update(...)` called by the system.
 - Every tick: lock the workspace, read the active doc's camera (target, zoom); viewport = `Res<GameConfig>` render
   size (== window size in this app); write `MainCamera(Camera2D::new(target, viewport/2).with_zoom(zoom))` (check the
   builder in crates/aberred-core/src/resources/camera2d.rs). No active doc: leave the camera alone.
 - Ignore all mouse input when `UiCapture.mouse` is set, EXCEPT a pan gesture already in progress continues.
 - Wheel: if `input.scroll_y != 0` and the mouse is not captured: `zoom_at(cam, viewport, (mouse_x, mouse_y),
   1.1f32.powf(scroll_y))`; store back with a new `DialogueDoc::set_camera(CamState)` mutator that does NOT bump
   `revision` and does NOT set `dirty` (camera is view state; add it to the model).
 - Pan: middle mouse (`InputAction::Action3`) held -> accumulate the screen-pixel delta of `mouse_x/mouse_y` between
   ticks (previous position in a `Local<Option<Vec2>>`, reset on press) and apply `pan_by_screen_delta`. NEVER use
   mouse_world_x/y for panning (they lag the camera by one tick). Do not use Action2 (right button) here.
 - Frame all: add `EditorAction::FrameAll` (actions.rs): the menu item View > "Frame all" (enable it only with an
   active doc) and the key `F` (only when `!want_text_input`; add to shortcuts.rs) produce it; it is handled in gui.rs
   where the viewport is known (`ui.io().display_size`): `doc.set_camera(frame_all(graph_bounds(doc), viewport,
   40.0))`. Keep `apply_action` testable (the viewport-dependent part may live in a small pure helper in src/graph/
   frame.rs with a test).
TESTS: pure helpers extracted from the system: `apply_wheel(cam, viewport, cursor, scroll_y) -> CamState` and the pan
accumulation helper; `set_camera` does not bump revision/dirty; the FrameAll helper.
CONSTRAINTS: scope only; do not touch the projection system. DONE WHEN: `just check` passes and `timeout 15 cargo run`
starts without panicking.
```

**Claude review checklist:** pan uses screen deltas; `set_camera` does not bump `revision`; MainCamera offset =
viewport/2; no right-button handling; `graph_frame_system` is the single place later extended with input.

---

## Step 4a — Pure interaction state machine (long)

**Executor:** vibe (after 3 is committed). **Scope:** `src/graph/{mod,interaction}.rs`, `CLAUDE.md`.
**Acceptance:** `just check`. **time_limit:** 3000. **Suggested message:**
`Add pure graph interaction state machine`

**Vibe spec (pass verbatim, preceded by the "Conventions" block):**
```
Existing pure code: model (DialogueDoc mutators incl. set_node_pos/select/set_link/delete_node/set_start), and
src/graph/{geometry,camera,layout,style}.rs.

GOAL: the mouse-gesture state machine as pure code (no ECS/imgui). It consumes per-tick input samples and returns
Actions; a later step applies them.

TYPES (src/graph/interaction.rs)
 - `InputSample { mouse_screen: Vec2, mouse_world: Vec2, left_down: bool, left_pressed: bool, left_released: bool,
   right_down: bool, right_pressed: bool, right_released: bool, mouse_captured_by_ui: bool, zoom: f32 }`
   (`*_pressed/released` are one-tick edges; `mouse_world` is already computed with the CURRENT camera).
 - `Phase { Idle, DraggingNode { key, grab_offset: Vec2 }, DraggingLink { from: NodeKey, slot: Slot, from_pin_world:
   Vec2 }, RightDown { press_screen: Vec2, press_world: Vec2, hit: Hit, panning: bool } }`;
   `GraphInteraction { phase: Phase }` (Default Idle).
 - `Action { Select(Option<NodeKey>), MoveNode(NodeKey, Vec2 /*new pos*/), SetLink(NodeKey, Slot, Link),
   PanByScreenDelta(Vec2), OpenContextMenu { world: Vec2, hit: Hit } }`.
 - `pub fn step(state: &mut GraphInteraction, doc: &DialogueDoc, s: &InputSample, prev_mouse_screen: Vec2) ->
   Vec<Action>`.
 - `drag_preview(state, s) -> Option<(Vec2 /*from pin world*/, Vec2 /*to mouse world*/)>` for drawing the link being
   dragged.
BEHAVIOUR
 - Pin hit radius = (PIN_R + 4.0) / zoom (screen-constant pickup).
 - left_pressed while `mouse_captured_by_ui` is ignored. Gestures already in progress are NOT cancelled when the
   mouse moves over UI; they end on release.
 - Left press on Body(key): emit Select(Some(key)); phase DraggingNode with grab_offset = mouse_world - node.pos;
   while left_down emit MoveNode(key, mouse_world - grab_offset); on release -> Idle.
 - Left press on OutputPin(key, slot): emit Select(Some(key)); phase DraggingLink. If the pin's current link is
   Link::Node(_) it is detached IMMEDIATELY (emit SetLink(key, slot, Link::End)) and carried; on release: over Body or
   InputPin of node T -> SetLink(key, slot, Link::Node(T)) (a self-link is allowed); over empty space -> nothing more
   (the link stays End: this is the "drag off the pin to remove it" gesture). A Dangling link is also cleared (set to
   End) on pickup. Dropping a dragged link on empty space NEVER creates a node.
 - Left press on InputPin(key) or Body: treated like Body (select + drag). Left press on Empty: Select(None).
 - Right press (not captured): phase RightDown{ hit, panning:false }. While right_down, if the screen distance from
   press_screen exceeds 4.0 px: panning = true and emit PanByScreenDelta(mouse_screen - prev_mouse_screen) each tick.
   On right_released with panning == false: emit OpenContextMenu { world: press_world, hit } (hit computed at press
   time; for a Body/InputPin/OutputPin hit also Select that node). On release with panning -> Idle, no menu.
 - Only one phase at a time: a right press during a left drag is ignored and vice versa.
TESTS FIRST (#[cfg(test)], table-driven with scripted InputSample sequences over a doc from tests/fixtures/guard.json
(via include_str!) laid out by `auto_layout`): select+drag moves by the exact delta; drag link onto another node
connects; drag from an already-linked pin detaches (End) and re-connect works; drop on empty space leaves End and
creates nothing; Dangling pin pickup clears it; right click opens the menu with the right hit; right drag > 4 px pans
and opens no menu; UI-captured press is ignored; mid-gesture capture does not cancel; pin hit radius scales with zoom.
CONSTRAINTS: pure code only; no unwrap on missing nodes (the doc may change under a gesture: if a node vanished,
fall back to Idle). DONE WHEN: `just check` passes. Report the first failing test.
```

## Step 4b-1 — Input system, drag preview, `apply_actions` (long)

**Executor:** vibe (after 4a is committed). **Scope:** `src/graph/{mod,input_system,preview,apply}.rs`,
`src/model/doc.rs` (`pending_context_menu`), `src/scenes/editor/lifecycle.rs`, `src/main.rs`, `CLAUDE.md`.
**Acceptance:** `just check`; `timeout 15 cargo run`; manual: click selects (yellow outline), drag moves nodes, drag
from an output pin shows a preview curve and connects on release, dragging a pin off removes the link, right-drag pans.
**time_limit:** 3000. **Suggested message:** `Wire graph mouse input and drag preview`

**Vibe spec (pass verbatim, preceded by the "Conventions" block):**
```
Existing: pure interaction state machine (src/graph/interaction.rs: GraphInteraction, step, drag_preview, Action), the
projection system, `graph_frame_system` (src/graph/camera_system.rs: calls `camera_update`), model mutators,
UiCapture, Action2 rebound to the RIGHT mouse button only at scene enter.

GOAL: connect the mouse to the model.
1. src/model/doc.rs: `Workspace.pending_context_menu: Option<ContextMenuRequest>` with `ContextMenuRequest { world:
   Vec2, node: Option<NodeKey> }` defined in the MODEL (do not reference graph types from the model).
2. src/graph/apply.rs (PURE): `apply_actions(doc: &mut DialogueDoc, actions: &[Action]) -> Option<ContextMenuRequest>`
   applying Select -> `doc.select`; MoveNode -> `doc.set_node_pos`; SetLink -> `doc.set_link`; PanByScreenDelta ->
   `doc.set_camera(pan_by_screen_delta(..))` (needs no viewport); OpenContextMenu -> returned request (node = the hit's
   node for Body/pins, else None). Log link changes at debug (`link set`, `link cleared`) — events only, never the
   per-tick MoveNode.
3. src/graph/input_system.rs: extend `graph_frame_system` so the fixed order is camera update -> input. Add
   `ResMut<GraphInteraction>` (resource, inserted at scene enter) and a `Local<Vec2>` previous mouse. Build the
   `InputSample` from `Res<InputState>`: `mouse_screen = (mouse_x, mouse_y)`; `mouse_world` =
   `screen_to_world(doc.camera, viewport, mouse_screen)` using the CURRENT doc camera (not `input.mouse_world_*`,
   which lag); left from `mouse_left_button` (active/just_pressed/just_released); right from
   `input.action(InputAction::Action2)`; `mouse_captured_by_ui` from UiCapture. Lock the workspace, call `step`,
   `apply_actions`, store a returned request in `ws.pending_context_menu`, unlock.
4. src/graph/preview.rs: while `drag_preview` is Some keep ONE entity `GraphPreview` (ZIndex preview Z) with
   `Shape::bezier(...).with_thickness(EDGE_THICKNESS)` from the pin to the mouse world using `edge_curve`; rewrite it
   each tick, despawn it when the phase ends. (Plain spawn/rewrite/despawn; no workspace revision involved.)
TESTS: `apply_actions` (MoveNode/SetLink/Select effects; OpenContextMenu returns the right request; set_link on a
missing node is a no-op); the system itself is verified manually.
CONSTRAINTS: no imgui here (the menu is the next step); the system needs only Res<AppState>, Res<InputState>,
Res<GameConfig>, ResMut<GraphInteraction>, Commands and queries. DONE WHEN: `just check` passes and
`timeout 15 cargo run` starts without panicking.
```

**Claude review checklist:** one frame system orders camera -> input; input uses the doc camera for world coordinates;
the drag preview entity is cleaned up; `ContextMenuRequest` carries no graph types; no per-tick logging.

## Step 4b-2 — Context menu, Add node, Delete key

**Executor:** vibe (after 4b-1 is committed). **Scope:** `src/scenes/editor/{gui,context_menu,actions,menu,shortcuts,
mod}.rs`, `src/model/doc.rs` (only if a helper is needed), `CLAUDE.md`. **Acceptance:** `just check`;
`timeout 15 cargo run`; manual: right-click on a node -> menu (Set as start, Delete node), on canvas -> "Add node
here", Edit > Add node adds at the view centre and selects it, the Delete key removes the selected node.
**time_limit:** 2400. **Suggested message:** `Add graph context menu, node creation and deletion`

**Vibe spec (pass verbatim, preceded by the "Conventions" block):**
```
Existing: `Workspace.pending_context_menu: Option<ContextMenuRequest { world, node }>` set by the input system; the editor
GUI (src/scenes/editor: EditorAction + apply_action, menu, shortcuts, modals), model mutators (add_node, delete_node,
set_start, select).

GOAL: the context menu and node creation/deletion in the GUI (render thread).
 - gui.rs takes `pending_context_menu` each frame (short lock, `.take()`); when Some, keep the request in a GUI-side
   local (e.g. a thread_local like tabs.rs does) and `ui.open_popup("graph_ctx")` exactly once per request. The popup
   shows, for a node hit: "Set as start", "Delete node"; always: "Add node here". Items produce EditorActions
   (`SetStart(NodeKey)`, `DeleteNode(NodeKey)`, `AddNodeAt(Vec2)`), applied by the single `apply_action` function:
   `AddNodeAt(world)` = `doc.add_node(world)` then `doc.select(Some(key))`; `DeleteNode` = `doc.delete_node` (dirty
   via the mutator); `SetStart` = `doc.set_start`.
 - Edit > "Add node" (enable only with an active doc) = `EditorAction::AddNode`: adds at the world position of the
   screen centre (`screen_to_world(doc.camera, viewport, viewport/2)`; viewport = `ui.io().display_size`) and selects it.
 - The Delete key (only when `!want_text_input` and a node is selected) = `EditorAction::DeleteSelected`.
 - Log each of these at debug (events). Keep the lock discipline: short locks, never while dialog code runs.
TESTS: `apply_action` unit tests for AddNodeAt (new node selected, first node becomes start), DeleteNode (links to it
become End, start/selection cleared), SetStart, DeleteSelected with and without a selection.
CONSTRAINTS: scope only; Re-layout stays disabled. DONE WHEN: `just check` passes and `timeout 15 cargo run` starts
without panicking.
```

**Claude review checklist:** the context menu opens exactly once per request; Delete gated on `!want_text_input`;
all edits go through `apply_action`; no model semantics changed.

## Step 4s — Simplify

**Executor:** vibe, skill `simplify`, after 4b-2 is committed. **Scope:** `src/graph/**`, `src/scenes/editor/**`.
**time_limit:** 2400. Spec: simplify only; no behaviour/API change; keep lock discipline, one frame system ordering and
`add_late_system`; test count not lower (record it first). **Suggested message:**
`Simplify graph projection, input and menu code`

---

## Step 5 — Re-layout, Frame all polish, Help > Controls

**Executor:** vibe (after 4s is committed). **Scope:** `src/scenes/editor/{menu,gui,actions,modals,mod}.rs`,
`src/model/doc.rs` (only if a helper is needed), `CLAUDE.md`. **Acceptance:** `just check`; `timeout 15 cargo run`;
manual: drag nodes into a mess, View > Re-layout restores the layout and frames the graph; F fits it; a new empty doc
+ Add node works. **time_limit:** 2400. **Suggested message:** `Enable Re-layout and add the Controls help`

**Vibe spec (pass verbatim, preceded by the "Conventions" block):**
```
Existing: `graph::layout::auto_layout`, `graph::camera::frame_all`, `graph::geometry::graph_bounds`, the doc camera
setter, `EditorAction`/`apply_action`, the View menu (Re-layout still disabled, Frame all enabled).
GOAL: enable View > Re-layout = `EditorAction::Relayout` (calls `auto_layout` on the active doc, then frames all as
the Frame all action does); enabled only with an active doc. After Re-layout the doc is NOT marked dirty (positions are
not saved; `set_node_pos` does not set dirty). Add Help > Controls = a modal listing: left-drag node move, drag from a
right-side pin to connect, drag a pin off to disconnect, right-click for the menu, middle- or right-drag to pan, wheel
to zoom, F frame all, Delete removes the selected node, Ctrl+N/O/S/Shift+S/W/Q.
TESTS: re-layout does not set dirty; positions change for a messed-up doc (move nodes with set_node_pos first); the
Controls modal state toggles (pure flag on the GUI side).
CONSTRAINTS: scope only. DONE WHEN: `just check` passes and `timeout 15 cargo run` starts without panicking.
```

---

## End of plan 03 checklist
- [ ] Steps 1a, 1b, 1s, 2a, 2b, 3, 4a, 4b-1, 4b-2, 4s, 5 each committed by the user (small commits)
- [ ] `just check` green
- [ ] manual run: guard.json laid out; select/drag/connect/disconnect; context menu; Add node (menu + context);
      Delete; pan/zoom/frame-all; Re-layout; two tabs keep independent cameras
- [ ] no leftover entities after closing a tab (`RUST_LOG=aberreddialogueeditor=debug`, the `projection:` log lines)

## Open items carried from plan 02 (independent small jobs)
- **Save As / Discard index race:** a Save As dialog started while the "unsaved changes" modal is open, followed by
  Discard, can shift the tab index the dialog saves to. Fix idea: ignore Save As and Discard while a dialog is in
  flight, or key the pending Save As by `doc_id` (available from step 2a) instead of the tab index.
- **Window X button:** the engine polls raylib's `window_should_close()` directly, so closing the OS window with
  unsaved changes quits without asking. Needs an engine hook (note for the engine backlog).

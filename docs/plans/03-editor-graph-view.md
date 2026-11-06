# Plan 03 — Graph view: layout, drawing, camera, mouse interaction

**Run in:** `/home/idaho/Projects/aberreddialogueeditor`. Read `docs/plans/00-overview.md` first.
**Depends on:** plan 02 complete AND plan 01 pushed to GitHub (`develop-0.2.0` has `ShapeKind::Bezier`,
`ShapeKind::RoundedRect`, `Shape::with_thickness`). **Done when:** opening `tests/fixtures/guard.json` shows an
auto-laid-out graph; nodes can be selected, dragged, connected, disconnected, added and deleted with the mouse;
pan/zoom/frame-all work; `just check` green; all steps committed by the user.

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

## Step 0 — Pick up the engine (Claude, before any vibe job)

1. Confirm with the user that plan 01 is pushed. Run `cargo update -p aberredengine`.
2. Verify with a tiny `cargo check` that `Shape::bezier`, `Shape::rounded_rect`, `with_thickness` resolve
   (`grep` the fetched source under `~/.cargo/git/checkouts/aberredengine-*/*/crates/aberred-core/src/components/
   shape.rs`). If not, STOP: the push is missing or stale.
3. `just check`; stage `Cargo.lock`; message `Update aberredengine to the shape-features commit`; STOP for commit.

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

## Step 1 — Pure geometry, wrapping, layout, camera math (+ layout on load)

**Executor:** vibe. **Scope:** `src/graph/{mod,style,geometry,wrap,layout,camera}.rs`, `src/ops/mod.rs` (call
auto layout after load), `src/main.rs` (`mod graph;`), `tests/graph_*.rs`.
**Acceptance:** `just check`. **Suggested message:** `Add pure graph geometry, text wrap, auto-layout and camera math`

**Vibe spec (pass verbatim):**
```
Project: /home/idaho/Projects/aberreddialogueeditor. Read CLAUDE.md and docs/plans/00-overview.md (Architecture)
first. The model (src/model: DialogueDoc, EdNode, Outputs, Link, NodeKey, Slot...), linter and ops exist. This step
adds PURE logic with no ECS/imgui/engine systems; only `Vec2` from `aberredengine::prelude` may be used.

CONSTANTS (src/graph/style.rs, `pub const`): NODE_W=240.0, HEADER_H=24.0, TEXT_AREA_H=64.0, ROW_H=22.0, PAD=8.0,
PIN_R=6.0, FONT_SIZE=12.0, TEXT_MAX_LINES=4, COL_GAP=120.0, ROW_GAP=24.0. (Colours/Z come in a later step; put
only the numeric layout constants now.)

1. src/graph/geometry.rs
 - `rows(node: &EdNode) -> usize` = number of choices for Outputs::Choices, else 1.
 - `node_height(node) = HEADER_H + TEXT_AREA_H + max(1,rows) as f32 * ROW_H`; `node_rect(node) -> (min, max)`.
 - `input_pin(node) -> Vec2` = (pos.x, pos.y + HEADER_H/2); `output_pin(node, slot_index: usize) -> Vec2` =
   (pos.x + NODE_W, pos.y + HEADER_H + TEXT_AREA_H + (slot_index as f32 + 0.5) * ROW_H).
 - `Hit { OutputPin(NodeKey, Slot), InputPin(NodeKey), Body(NodeKey), Empty }` and
   `hit_test(doc: &DialogueDoc, p: Vec2, pin_hit_radius: f32) -> Hit`: pins take priority over bodies; among
   overlapping nodes the one LATER in `doc.nodes` order wins (it is drawn on top). Output pin slot for a Next node
   is `Slot::Next`, for choices `Slot::Choice(i)`. Use `pin_hit_radius` (callers pass PIN_R + 4 scaled by zoom).
 - `edge_curve(from: Vec2, to: Vec2) -> (c1: Vec2, c2: Vec2, to: Vec2)` returning control points RELATIVE to
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
3. src/graph/layout.rs
 - `auto_layout(doc: &mut DialogueDoc)`: layered left-to-right layout. Algorithm:
   a) layer(node) = shortest-path depth from `doc.start` (BFS over Link::Node edges); BACK edges (target depth <=
      source depth) are ignored for ordering. Nodes not reachable from start (or all nodes when start is None)
      go to a trailing set: BFS from each remaining node in node order, appended as extra layers AFTER the last
      reachable layer (so unreachable nodes are in their own columns, deterministic).
   b) within a layer order nodes by barycenter of their predecessors' order in the previous layer (two sweeps
      forward, ties broken by node order); initial order = node order.
   c) x = layer * (NODE_W + COL_GAP); y stacked within the layer with ROW_GAP between node heights (use
      geometry::node_height); each layer is vertically centred around y = 0 relative to the tallest layer.
   Write positions with `doc.set_node_pos`. Must be deterministic and must terminate on cycles/self-loops.
 - Call `auto_layout` at the end of `ops::open_path` for newly opened docs (not for the Focused outcome).
4. src/graph/camera.rs (all pure; `CamState {target: Vec2, zoom: f32}` is in the model; viewport = window size in
   screen pixels as Vec2; the camera shows world point `target` at the screen centre; screen = (world - target) *
   zoom + viewport/2):
 - `screen_to_world(cam, viewport, p) -> Vec2` and `world_to_screen(...)` (inverses; test the round trip).
 - `zoom_at(cam, viewport, cursor_screen, factor) -> CamState`: new zoom = clamp(zoom*factor, 0.1, 4.0); the world
   point under the cursor stays under the cursor.
 - `pan_by_screen_delta(cam, delta_screen) -> CamState` (target -= delta/zoom).
 - `frame_all(bounds: Option<(Vec2,Vec2)>, viewport, margin_px: f32) -> CamState` (None -> target ZERO, zoom 1.0;
   zoom clamped to [0.1, 1.0] so small graphs are not magnified; centred on the bounds).

TESTS FIRST (tests/graph_geometry.rs, graph_wrap.rs, graph_layout.rs, graph_camera.rs): pin positions for a
Next node and a 3-choice node; hit_test priority (pin over body, topmost wins, Empty); edge_curve forward and back
cases (endpoint equals `to - from`; back case control points are below both pins); wrap: width, newline,
long word, truncation "...", non-ASCII, empty; layout: guard.json - start in layer 0, no two node rects overlap,
deterministic (twice equal), loops.json terminates, unreachable nodes land right of all reachable ones, empty doc
no panic; camera: round trips, zoom_at fixed point, pan, frame_all contains bounds.
CONSTRAINTS: no ECS/imgui; keep `src/ops` change minimal. DONE WHEN: `just check` passes. Report the first
failing test.
```

**Claude review checklist:** geometry matches the constants block exactly (it is the contract for steps 2 and 4);
`edge_curve` is relative to the source pin; layout deterministic; wrap is `char`-safe.

---

## Step 2 — Projection (model → entities)

**Executor:** vibe. **Scope:** `src/graph/{style,projection,components}.rs`, `src/model/doc.rs` (add `doc_id`),
`src/model/mod.rs`, `src/scenes/editor/lifecycle.rs`, `src/main.rs`, `tests/projection.rs`.
**Acceptance:** `just check`; then Claude runs `just run`, opens `guard.json` and verifies nodes, text, pins and
arrows appear, and that Open on another file switches graphs with no leftover entities (use the `run` skill for
screenshots if available; otherwise rely on a log line `projection: N nodes, M edges` at debug level).
**Suggested message:** `Project dialogue documents into shape and text entities`

**Vibe spec (pass verbatim):**
```
Project: /home/idaho/Projects/aberreddialogueeditor. Read CLAUDE.md and docs/plans/00-overview.md (Architecture and
"Engine facts") and docs/plans/03-editor-graph-view.md ("Constants" block) first. Existing: model, linter, ops,
pure graph module (src/graph/{geometry,wrap,layout,camera,style}.rs). The engine now has Shape::bezier(c1,c2,to,
color), Shape::rounded_rect(size, radius, color), Shape::with_thickness(t) (see
/home/idaho/Projects/aberredengine/crates/aberred-core/src/components/shape.rs, docs on thickness semantics),
ShapeKind, Polygon via Shape::polygon(&[Vec2], color).

GOAL: a logic system that draws the ACTIVE document as world-space entities and keeps them in sync.

1. src/model: add `doc_id: u64` to DialogueDoc (unique per doc, from `Workspace::next_doc_id` when pushed; set it in
   `Workspace::add_doc`).
2. src/graph/style.rs: add the colour and Z constants from the plan's "Constants" block (RGBA as engine `Color`
   values; check the engine's Color type in the prelude).
3. src/graph/components.rs: marker components `GraphNode { key: NodeKey }`, `GraphEdge { from: NodeKey, slot:
   Slot }`, `GraphPart` (all non-root visual children and arrowheads) and a resource `GraphEntities` holding:
   `shown_doc: Option<u64>`, `shown_revision: u64`, `nodes: FxHashMap<NodeKey, NodeEntry { root: Entity, sig:
   NodeSig, pos: Vec2 }>`, `edges: FxHashMap<(NodeKey, Slot), EdgeEntry { entity: Entity, from: Vec2, to: Vec2,
   target: Option<NodeKey> }>`. Slot needs Hash+Eq (derive in the model if missing).
4. src/graph/projection.rs: `pub fn graph_projection_system(mut commands: Commands, mut ents: ResMut<GraphEntities>,
   app_state: Res<AppState>, metrics: Res<FontMetricsStore>,
   pending: Res<PendingAssets>, mut warn: ResMut<FontMetricsWarnCache>, mut shapes: Query<&mut Shape>, mut
   positions: Query<&mut WorldPosition>)`. Gets the workspace once per tick with
   `app_state.get::<WorkspaceMutex>()` (the Arc<Mutex<Workspace>> inserted at setup; check the exact `get` API in
   crates/aberred-core/src/resources/app_state.rs). Take only `Res<AppState>` here, never `ResMut`, and never
   both. Register it with
   `add_late_system` so it runs after input/camera systems. Algorithm per tick:
   - lock the workspace; if no active doc: despawn everything (via commands.entity(e).despawn(), which cascades to
     ChildOf children) and return; copy out what is needed (doc_id, revision, nodes) and unlock BEFORE spawning.
   - if `doc_id` differs from `shown_doc`: despawn all node and edge entities, clear the maps, rebuild fully.
   - else if `revision` is unchanged and font metrics availability is unchanged: return immediately (cheap path).
   - Node diff by NodeKey: compute `NodeSig` = (id, speaker, text, labels, per-row texts + target kinds, is_start,
     is_selected, has_error (from `doc.problems()` Error with that node), metrics_ready). If the node is new or its
     sig changed: despawn the old root and spawn a fresh one. If only `pos` changed: update the root's
     WorldPosition. Nodes missing from the doc: despawn.
   - Node entity layout (root = `(GraphNode{key}, WorldPosition(pos), ZIndex(...))`; children `ChildOf(root)` with
     LOCAL WorldPosition, each with `GraphPart`):
       body: Shape::rounded_rect((NODE_W, node_height), CORNER_RADIUS, body colour).filled().with_outline_color(
       outline colour: selected > error > normal) — thick outlines: also `.with_thickness(OUTLINE_THICKNESS)` only
       when selected or error;
       header: Shape::rect((NODE_W, HEADER_H), header colour or start colour).filled(); header text = node id
       (fit_line with measure) as DynamicText(FONT, FONT_SIZE, text colour) at local (PAD, 4);
       text area: DynamicText of `wrap_text("<speaker>: <text>" or just text, NODE_W - 2*PAD, TEXT_MAX_LINES,
       measure).join("\n")` at local (PAD, HEADER_H + 4), speaker prefix only when speaker is Some;
       rows: for each row i a DynamicText at local (PAD, text-area-bottom + i*ROW_H + 4): choice text via
       fit_line to NODE_W - 2*PAD - 2*PIN_R - 8, or "next" / "(end)" for a Next node (dim colour); a small
       indicator " ?" suffix when `when` is Some and " !" when effects non-empty;
       input pin: Shape::circle(PIN_R, pin-in colour).filled() at the input pin's local position
       (`geometry::input_pin(node) - node.pos`, i.e. on the left edge); output pins likewise at each
       `geometry::output_pin(node, i) - node.pos`; a Dangling target draws that pin in the dangling colour.
     `measure` = the metrics `measure_text(s, FONT_SIZE).x`; resolve the font key through
     `resolve_key(FONT, &pending, &mut warn)`. If metrics are not available yet, spawn nothing for text and retry
     next tick (include `metrics_ready` in the sig so it redraws once ready).
   - Edge diff by (from key, slot): for every Link::Node(target) compute `from = node_pos + output pin offset`
     and `to = target_pos + input pin offset` (world); edge entity = `(GraphEdge, WorldPosition(from), ZIndex(0),
     Shape::bezier(c1, c2, to_rel, edge colour).with_thickness(EDGE_THICKNESS))` using `geometry::edge_curve`.
     Colour = selected-source colour when the source node is selected. If an edge exists and only from/to/colour
     changed, REWRITE its WorldPosition and Shape in place (Query<&mut Shape>, assign only when different so
     change detection stays cheap); NEVER respawn an edge for a mere node move. Spawn an arrowhead child
     (`GraphPart`, ChildOf(edge)): Shape::polygon of a triangle with tip at `to_rel`, base 10 wide and 12 long
     behind the tip along `arrow_tip_dir`, filled, ZIndex Z_ARROW; on rewrite also rewrite the arrowhead child
     (store its Entity in EdgeEntry). Edges whose target vanished or became non-Node: despawn.
   - Dangling links draw no edge (the pin is red).
   - A debug! log line "projection: N nodes, M edges" on each rebuild.
5. Register `GraphEntities` as a resource in the scene enter (insert_resource) and add the system in main.rs.
TESTS (headless bevy World as in /home/idaho/Projects/aberrededitor/src/systems/map_ops.rs tests and
docs/ in that repo; insert the resources the system needs: FontMetricsStore with the engine's test fixture or an
inserted fake FontMetrics if the engine exposes test_support; if font metrics cannot be faked, test the diffing
by factoring the pure part into `plan_projection(prev: &Snapshot, next: &Snapshot) -> Vec<ProjOp>` and test
that function instead): moving a node yields only position updates and edge rewrites (no spawns); renaming a node
respawns just that node; deleting a node despawns it and its incoming edges; switching doc_id rebuilds all; no
entities remain after the active doc is closed. Prefer the pure `plan_projection` design so the logic is testable
without fonts.
CONSTRAINTS: do not modify model semantics beyond doc_id; no input handling here; no imgui. Do not request any
render-only resource. DONE WHEN: `just check` passes. Report the first failing test and any deviation.
```

**Claude review checklist:** no `ResMut<AppState>` next to `Res<AppState>`; edges rewritten not respawned;
`add_late_system` used; `plan_projection` is pure and tested; text children are `GraphPart` under the node root.

---

## Step 3 — Camera system and zoom/pan input

**Executor:** vibe. **Scope:** `src/graph/camera_system.rs`, `src/graph/mod.rs`, `src/main.rs`,
`src/scenes/editor/{lifecycle,gui}.rs`. **Acceptance:** `just check`; Claude runs the app and verifies
wheel-zoom at the cursor, middle-drag pan, tab switch restores each doc's camera.
**Suggested message:** `Add per-document camera with wheel zoom and drag pan`

**Vibe spec (pass verbatim):**
```
Project: /home/idaho/Projects/aberreddialogueeditor. Read CLAUDE.md, docs/plans/00-overview.md ("Engine facts"
input/camera sections) first. Existing: pure camera math in src/graph/camera.rs (screen_to_world, zoom_at,
pan_by_screen_delta, frame_all), CamState in each DialogueDoc, UiCapture in AppState, MainCamera inserted at scene
enter.

GOAL: a logic system `graph_camera_system` (add_system, runs in Update) that makes MainCamera follow the ACTIVE
doc's CamState and handles wheel zoom and panning.

 - Every tick: lock the workspace, read the active doc's camera (target, zoom); compute the viewport from
   `Res<GameConfig>` render size (== window size in this app); write `MainCamera(Camera2D::new(target, viewport/2)
   .with_zoom(zoom))` (check Camera2D builder methods in crates/aberred-core/src/resources/camera2d.rs).
 - Ignore all mouse input when `UiCapture.mouse` is set, EXCEPT a pan gesture already in progress continues.
 - Wheel: if `input.scroll_y != 0` and the mouse is not captured: `zoom_at(cam, viewport, (mouse_x, mouse_y),
   1.1f32.powf(scroll_y))`; store back into `doc.camera` (use a `set_camera` mutator that does NOT bump
   `revision`/`dirty` - the camera is view state; add it to the model if absent).
 - Pan: middle mouse (`InputAction::Action3`) held -> accumulate the screen-pixel delta of `mouse_x/mouse_y`
   between ticks (keep the previous mouse position in a `Local<Option<Vec2>>`, reset on press) and apply
   `pan_by_screen_delta`. NEVER use mouse_world_x/y for panning (it lags the camera by one tick).
   Right-button panning is handled later by the interaction state machine (it must distinguish click from drag), so
   do not use Action2 here.
 - Key `F` and View -> "Frame all" are handled in the GUI: it computes `frame_all(graph_bounds(doc), viewport,
   40.0)` and calls `doc.set_camera(...)`. Wire the existing disabled "Frame all" menu item, and the F shortcut
   (only when `!want_text_input`).
TESTS: pure parts are already tested; add a test for a small helper `apply_wheel(cam, viewport, cursor,
scroll_y)` and for the pan accumulation helper (pure functions extracted from the system).
CONSTRAINTS: scope only. DONE WHEN: `just check` passes.
```

**Claude review checklist:** pan uses screen deltas; camera setter does not bump `revision`; MainCamera offset =
viewport/2; no right-button handling here.

---

## Step 4 — Interaction (two chained vibe jobs)

### 4a — Pure interaction state machine
**Executor:** vibe. **Scope:** `src/graph/interaction.rs`, `src/graph/mod.rs`, `tests/interaction.rs`.
**Acceptance:** `just check`. **Suggested message:** `Add pure graph interaction state machine`

**Vibe spec (pass verbatim):**
```
Project: /home/idaho/Projects/aberreddialogueeditor. Read CLAUDE.md, docs/plans/00-overview.md and
docs/plans/03-editor-graph-view.md ("Constants") first. Existing pure code: model (DialogueDoc mutators incl.
set_node_pos/select/set_link/delete_node/set_start), src/graph/{geometry,camera}.rs.

GOAL: the mouse-gesture state machine as pure code (no ECS/imgui). It consumes per-tick input samples and returns
Actions; a later step applies them.

TYPES (src/graph/interaction.rs)
 - `InputSample { mouse_screen: Vec2, mouse_world: Vec2, left_down: bool, left_pressed: bool, left_released:
   bool, right_down: bool, right_pressed: bool, right_released: bool, mouse_captured_by_ui: bool, zoom: f32 }`
   (`*_pressed/released` are one-tick edges; `mouse_world` is already computed with the CURRENT camera).
 - `Phase { Idle, DraggingNode { key, grab_offset: Vec2 }, DraggingLink { from: NodeKey, slot: Slot,
   from_pin_world: Vec2 }, RightDown { press_screen: Vec2, press_world: Vec2, hit: Hit, panning: bool } }`;
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
   Link::Node(_) it is detached IMMEDIATELY (emit SetLink(key, slot, Link::End)) and carried; on release:
   over Body or InputPin of node T -> SetLink(key, slot, Link::Node(T)) (a self-link is allowed); over empty space ->
   nothing more (the link stays End: this is the "drag off the pin to remove it" gesture). A Dangling link is also
   cleared (set to End) on pickup. Dropping a dragged link on empty space NEVER creates a node.
 - Left press on InputPin(key) or Body: treated like Body (select + drag). Left press on Empty: Select(None).
 - Right press (not captured): phase RightDown{ hit, panning:false }. While right_down, if the screen distance
   from press_screen exceeds 4.0 px: panning = true and emit PanByScreenDelta(mouse_screen - prev_mouse_screen)
   each tick. On right_released with panning == false: emit OpenContextMenu { world: press_world, hit } (hit
   computed at press time; for a Body/InputPin/OutputPin hit also Select that node). On release with panning ->
   Idle, no menu.
 - Only one phase at a time: a right press during a left drag is ignored and vice versa.
TESTS FIRST (table-driven with scripted InputSample sequences over a fixture doc from tests/fixtures/guard.json
laid out by auto_layout): select+drag moves by the exact delta; drag link onto another node connects; drag from an
already-linked pin detaches (End) and re-connect works; drop on empty space leaves End and creates nothing;
Dangling pin pickup clears it; right click opens the menu with the right hit; right drag > 4 px pans and opens no
menu; UI-captured press is ignored; mid-gesture capture does not cancel; pin hit radius scales with zoom.
CONSTRAINTS: pure code only; no unwrap on missing nodes (the doc may change under a gesture: if a node vanished,
fall back to Idle). DONE WHEN: `just check` passes. Report the first failing test.
```

### 4b — System wiring, drag preview, context menu, delete
**Executor:** vibe (`base_job` = 4a, or after the user committed 4a). **Scope:** `src/graph/{input_system,
preview}.rs`, `src/graph/projection.rs` (drag preview entity only), `src/scenes/editor/{gui,context_menu}.rs`,
`src/model/mod.rs` (`pending_context_menu`), `src/main.rs`, `tests/**`.
**Acceptance:** `just check`; Claude runs the app and verifies: click selects (yellow outline), drag moves nodes,
drag from an output pin shows a preview curve and connects on release, dragging a pin off removes the link,
right-click on a node → menu (Delete node, Set as start), on canvas → "Add node here", right-drag pans, Delete
key removes the selected node, the Add-node menu item adds at the view centre.
**Suggested message:** `Wire graph mouse interaction, context menu and node deletion`

**Vibe spec (pass verbatim):**
```
Project: /home/idaho/Projects/aberreddialogueeditor. Read CLAUDE.md, docs/plans/00-overview.md ("Architecture",
"Engine facts") first. Existing: pure interaction state machine (src/graph/interaction.rs: GraphInteraction, step,
drag_preview, Action), projection system, camera system, model mutators, UiCapture.

GOAL: connect the mouse to the model and add the context menu.

1. src/graph/input_system.rs: `graph_input_system` (add_system, Update; MUST run before the camera system or be
   chained with it - simplest: call `camera_system`'s body from a single `graph_frame_system` in a fixed order:
   camera update -> input). Hold `ResMut<GraphInteraction>` (resource, inserted at scene enter) and a
   `Local<Vec2>` previous mouse. Build `InputSample` from `Res<InputState>`: `mouse_screen = (mouse_x, mouse_y)`;
   `mouse_world` = `screen_to_world(doc.camera, viewport, mouse_screen)` using the CURRENT doc camera (not
   `input.mouse_world_*`, which lags); left from `mouse_left_button` (active/just_pressed/just_released); right
   from `input.action(InputAction::Action2)` (rebound to right mouse only at scene enter);
   `mouse_captured_by_ui` from UiCapture. Lock the workspace, call `step`, apply the Actions with the doc mutators:
   Select -> `doc.select`; MoveNode -> `doc.set_node_pos`; SetLink -> `doc.set_link`; PanByScreenDelta ->
   `doc.set_camera(pan_by_screen_delta(..))`; OpenContextMenu -> `ws.pending_context_menu = Some(..)`.
2. Drag preview: while `drag_preview` is Some, keep ONE entity `GraphPreview` (ZIndex 20) with
   `Shape::bezier(...).with_thickness(EDGE_THICKNESS)` from the pin to the mouse world using `edge_curve`; despawn
   it when the phase ends. (Plain spawn/rewrite/despawn in the input system; no workspace revision involved.)
3. Context menu (src/scenes/editor/context_menu.rs, GUI/render thread): `Workspace.pending_context_menu:
   Option<ContextMenuRequest { world: Vec2, hit: Hit }>` is taken (`.take()`) by the GUI each frame; when Some,
   call `ui.open_popup("graph_ctx")` and remember the request in a GUI-side AppState mutex (or keep it in the
   workspace as `context_menu_open: Option<ContextMenuRequest>`). The popup (`ui.popup("graph_ctx", ..)`) shows:
   on a node (any Body/pin hit): "Set as start", "Delete node"; always: "Add node here" (adds a node at
   `request.world` with `doc.add_node`, then selects it). Items call the doc mutators under a short lock.
   Edit -> "Add node" menu item adds at the world position of the screen centre
   (`screen_to_world(doc.camera, viewport, viewport/2)`) and selects it. The Delete key (only when
   `!want_text_input`) deletes the selected node. Delete via `doc.delete_node`.
4. Wire View -> "Re-layout" later (a later step); leave it disabled.
TESTS: unit tests for the pure translation `apply_actions(doc, actions)` (extracted from the system) - e.g.
MoveNode/SetLink/Select effects, delete node rewrites links, add_node at a position becomes selected, the first
node becomes start. The system itself is verified manually.
CONSTRAINTS: scope only; no B0002 (do not take ResMut<AppState> and ResMut<WorldSignals> together in one system;
this system needs only Res<AppState>, Res<InputState>, Res<GameConfig>, ResMut<GraphInteraction>, Commands).
DONE WHEN: `just check` passes. Report the first failing test and any decision not covered here.
```

**Claude review checklist:** one frame system orders camera → input; input uses doc camera for world
coordinates; drag preview entity is cleaned up; context menu opens exactly once per request; Delete key gated.

---

## Step 5 — Re-layout, frame-all polish, graph shortcuts

**Executor:** vibe. **Scope:** `src/scenes/editor/{menu,gui}.rs`, `src/model/doc.rs` (only if a helper is
needed). **Acceptance:** `just check`; Claude runs the app: drag nodes into a mess, View → Re-layout restores the
layout and frames the graph; Frame all (F) fits it; new empty doc + Add node works.
**Suggested message:** `Enable Re-layout and Frame all menu actions`

**Vibe spec (pass verbatim):**
```
Project: /home/idaho/Projects/aberreddialogueeditor. Read CLAUDE.md first. Existing: `graph::layout::auto_layout`,
`graph::camera::frame_all`, `graph::geometry::graph_bounds`, doc camera setter, View menu items (Re-layout, Frame
all) that may still be disabled.
GOAL: enable View -> Re-layout (calls `auto_layout` on the active doc, then frames all) and View -> Frame all
(F). Both are enabled only when there is an active doc. After Re-layout the doc is NOT marked dirty (positions are
not saved; `set_node_pos` already does not set dirty - assert this in a test). Add a short "Controls" help modal
under Help -> Controls listing: left-drag node move, drag from a right-side pin to connect, drag a pin off to
disconnect, right-click for menu, middle- or right-drag to pan, wheel to zoom, F frame all, Delete removes the
selected node, Ctrl+N/O/S/Shift+S/W/Q.
CONSTRAINTS: scope only. DONE WHEN: `just check` passes (add a test that re-layout does not set dirty and that
positions change for a mess-ed doc).
```

---

## End of plan 03 checklist
- [ ] Step 0 and steps 1, 2, 3, 4a, 4b, 5 committed by the user (small commits)
- [ ] `just check` green
- [ ] manual run: guard.json laid out; select/drag/connect/disconnect; context menu; Add node (menu + context);
      Delete; pan/zoom/frame-all; Re-layout; two tabs keep independent cameras
- [ ] no leftover entities after closing a tab (debug log line count)

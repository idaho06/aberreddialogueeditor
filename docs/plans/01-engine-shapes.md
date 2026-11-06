# Plan 01 — Engine: thick lines, Bezier, RoundedRect (with Lua parity and docs)

**Run in:** `/home/idaho/Projects/aberredengine` (branch `develop-0.2.0`). This file lives in the dialogue
editor repo's `docs/plans/`; read `/home/idaho/Projects/aberreddialogueeditor/docs/plans/00-overview.md` first.
**Depends on:** nothing. Can run in parallel with plan 02.
**Done when:** `just check` passes in the engine, all steps are committed by the user, and the user has pushed
`develop-0.2.0` to GitHub (the editor consumes the engine as a git branch dependency; only pushed commits count).

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

## Engine conventions that apply to every step (put these in every vibe spec)

Read `AGENTS.md`, `CLAUDE.md`, `.claude/rules/tdd.md` and `.claude/skills/add-feature/SKILL.md` in the engine repo.
- **TDD is mandatory:** write the failing test first, see it fail for the expected reason, then implement. The
  report must name which test came first.
- Raylib-bound code is exempt from tests, but pure geometry lives in `aberred-core` (`shape_geometry.rs`) with
  unit tests; `draw_shape_ops` in `aberred-render` stays a thin 1:1 match.
- Test behaviour, not derives or field storage. Test style: snake_case sentence names, `Color::RED/BLUE`,
  helpers `v()`, `ops()`, `lines()`, `triangles()`, `approx_eq`/`vec2_approx_eq` from `crate::testing`.
- No `macro_rules!` outside `aberred-lua`. Intra-doc links must resolve (`just doc-links` runs
  `RUSTDOCFLAGS=-D warnings cargo doc`). Do not link to private items.
- Docs that must stay in sync: `RUST-GAME-GUIDE.md` (every new ```rust fence must be mirrored verbatim in
  `guide-check/src/*.rs`), `assets/scripts/README.md`, `.claude/rules/*.md`.
- Inner loop: `just test-fast` (= `cargo test --features test-support`). Full gate: `just check`.

## Map of the code (so specs can be precise)

- `crates/aberred-core/src/components/shape.rs`: `Shape { kind, color, filled, outline_color, origin }`
  (`#[derive(Component, Clone, Debug, PartialEq)]`), `ShapeKind { Point, Line{to}, Rect{size}, Circle{radius},
  Ellipse{radius}, Polygon(Polygon) }`; private `Shape::new(kind, color)`; constructors `point/line/rect/circle/
  ellipse/polygon`; builders `filled()/with_outline_color()/with_origin()`; tests at the bottom
  (`constructors_pick_their_kind`, `new_shapes_store_the_colour_and_start_outlined_with_a_zero_origin`,
  `builders_set_their_field_and_chain_without_losing_earlier_ones`).
- `crates/aberred-core/src/systems/shape_geometry.rs`: `DrawOp { Pixel, Line, Triangle, Circle, CircleLines,
  Ellipse, EllipseLines }` (`Copy`, no colour); `ShapeLayer { Fill, Outline }`; `shape_layers` (l.77: the
  `(Point | Line, _) | (_, false)` arm = outline only, the catch-all gives a fill layer — a new outline-only kind
  MUST be added to the first arm, the compiler will not warn); `Transform` (`point(v) = pos +
  rotor.rotate((v - origin) * scale)`, `rect_corners`, `rect_outline_edges` with inset `0.5/|scale|`);
  `shape_ops(shape, layer, pos, scale, rotation_degrees, &mut Vec<DrawOp>)` (exhaustive match; `out` is never
  cleared); helpers `push_triangle` (flips winding to CCW, skips zero area), `push_ring`, `push_round`;
  `shape_cull_bounds`; `bounding_box`.
- `crates/aberred-render/src/systems/shape.rs`: `draw_shape_ops` (1:1 DrawOp → raylib), `shape_in_view`
  (margin = `Vec2::ONE + shadow.offset.abs()`), `shape_passes`.
- `Shape` crosses threads as a whole value (`drawable_snapshot.rs`, `mirror.rs` use `Clone + PartialEq`): adding
  a field or variant needs NO change there as long as both derives stay. State hash does not include `Shape`.
- Lua: `crates/aberred-lua/src/resources/lua_runtime/entity_builder/shape.rs` (builder methods, `modify_shape`),
  `.../spawn_data.rs` (`shape: Option<Shape>`, `parse_points`, `polygon_kind`), `.../commands.rs:502-519`
  (`EntityCmd::SetShape*`), `.../engine_api/macros.rs:666-726` (`define_entity_cmds` rows `entity_set_shape_*`,
  automatic `collision_` twins), `src/systems/lua_commands/entity_cmd.rs` (routing ~l.180, `process_shape_cmd`
  ~l.570-604, tests ~l.1798 and ~l.1862), `engine_api/entity.rs:197-270` (test asserting `kinds.len() == 6`),
  `tests/stub_generator_integration.rs` (`engine_lua_stub_is_up_to_date`, `luarc_json_is_up_to_date`,
  `meta_functions_complete` list), checked-in outputs `assets/scripts/engine.lua` and `assets/scripts/.luarc.json`
  (regenerate: `cargo run -- --create-lua-stubs`, `cargo run -- --create-luarc`).

## Semantics decided for this plan (copy into specs)

- `Shape.thickness: Option<f32>`. `None` = today's 1 px hairline: all existing behaviour and tests unchanged.
- Thickness is in WORLD units × mean(|scale.x|, |scale.y|) (so it scales with camera zoom, unlike hairlines).
  For screen-space shapes (scale 1) it is render-target pixels.
- Thick segments are drawn with raylib `draw_line_ex(start, end, thick, color)` via a new
  `DrawOp::ThickLine { a, b, thickness }` belonging to the **Outline** layer. Square caps: each segment is
  extended by `thickness/2` at both ends so polygon/rect corners and Bezier joints close without gaps (overlap at
  joints is accepted; semi-transparent colours may show darker joints — document it).
- Who honours thickness: `Line`, `Bezier`, outlines of `Rect`, `Polygon`, `RoundedRect`. `Point`, `Circle`,
  `Ellipse` ignore it (document it).
- A thick **Rect/RoundedRect outline is inset by thickness/2** (stays inside the rect bounds, consistent with the
  Rect doc's inward 1 px outline). Line, Bezier and Polygon outlines straddle their path.
- Culling margin: `shape_in_view` margin becomes `1 + (thickness/2) * max(|scale.x|,|scale.y|) + |shadow offset|`
  (conservative for every kind).
- Bezier: cubic from local `(0,0)` to `to` with control points `c1`, `c2`; hairline gaps at joins of 24 segments
  are accepted (documented); the editor always uses thickness ≥ 2. Outline-only (ignores `filled` and
  `outline_color`, like Line). Cull bounds = AABB of the 4 transformed control points (+ thickness margin).
- RoundedRect: top-left at local `(0,0)` like Rect; `radius` clamped to `[0, min(|size.x|,|size.y|)/2]`; 8
  segments per quarter-circle corner. Fill = triangles via `push_triangle` (rotation-safe; raylib's own rounded
  rect is axis-aligned only, so do NOT use `draw_rectangle_rounded`). Outline = `push_ring` over the rounded
  polygon (hairline) or inset ThickLines (thickness).

---

## Step 1 — Thickness

**Executor:** vibe. **Scope:** `crates/aberred-core/src/components/shape.rs`,
`crates/aberred-core/src/systems/shape_geometry.rs`, `crates/aberred-render/src/systems/shape.rs`.
**Acceptance:** `just test-fast`, then `just check`.
**Stage / message:** the three files above. Suggested: `Add Shape thickness and DrawOp::ThickLine for lines and outlines`

**Vibe spec (pass verbatim):**
```
Repository: /home/idaho/Projects/aberredengine (Rust workspace, branch develop-0.2.0). Read AGENTS.md, CLAUDE.md and
.claude/rules/tdd.md first. TDD is mandatory: write each failing test first, watch it fail, then implement.

GOAL: add optional line thickness to Shape, drawn with a new DrawOp::ThickLine.

CHANGES
1. crates/aberred-core/src/components/shape.rs: add `pub thickness: Option<f32>` to `Shape` (default None in the
   private `Shape::new`) and a builder `pub fn with_thickness(mut self, thickness: f32) -> Self`. Update the module
   and struct docs: None = 1 px hairline; Some(t) = t world units wide; Point/Circle/Ellipse ignore it. Extend the
   existing test `new_shapes_store_the_colour_and_start_outlined_with_a_zero_origin` (default None) and
   `builders_set_their_field_and_chain_without_losing_earlier_ones`.
2. crates/aberred-core/src/systems/shape_geometry.rs: add `DrawOp::ThickLine { a: Vec2, b: Vec2, thickness: f32 }`
   (document: belongs to the Outline layer, update the ShapeLayer docs which state which ops each layer may
   emit). In `shape_ops`, when `shape.thickness` is Some(t) (t > 0):
   - Line: emit one ThickLine from point(0) to point(to); width = t * mean(|scale.x|,|scale.y|); extend both ends
     by width/2 along the segment direction (square caps). A zero-length segment emits nothing.
   - Rect outline: four ThickLines along the rect edges INSET by width/2 (so the stroke lies inside the rect
     bounds), caps extended so corners close. Hairline path (None) must stay byte-for-byte as today.
   - Polygon outline: ThickLines along every edge (closed ring), straddling the path, caps extended by width/2.
   Fill layers are unchanged. Point/Circle/Ellipse ignore thickness.
   Update `shape_cull_bounds` docs: it still excludes shadow offset and line width; the caller adds both.
3. crates/aberred-render/src/systems/shape.rs: `draw_shape_ops` gets one new arm: ThickLine ->
   `d.draw_line_ex(vec2_to_raylib(a), vec2_to_raylib(b), thickness, color)` (keep it a 1:1 match; follow the
   existing code's calling conventions for the draw handle). `shape_in_view`: margin =
   `Vec2::ONE + shadow.offset.abs() + Vec2::splat(thickness/2 * max(|scale.x|,|scale.y|))` (thickness None => 0).
   Add/adjust tests next to `a_one_pixel_line_just_outside_the_view_still_counts_as_visible`.

TESTS TO WRITE FIRST (in shape_geometry.rs tests, existing style/helpers)
 - thick line emits exactly one ThickLine with thickness == t * mean scale, endpoints extended by half width;
 - thickness None leaves every existing op sequence unchanged (existing tests must pass untouched);
 - thick rect outline: four ThickLines, all strictly inside the rect bounds expanded by 0 (stroke inset by t/2);
 - thick polygon outline: one ThickLine per edge;
 - thickness is ignored by Point/Circle/Ellipse;
 - update `each_layer_emits_only_its_own_op_kinds` so ThickLine is allowed only in the Outline layer;
 - `shape_in_view` grows with thickness.

CONSTRAINTS: do not touch aberred-lua, docs, or any file outside the scope. Do not use macro_rules. Keep
`Shape: Clone + PartialEq` (an f32 field is fine). Do not use raylib draw_rectangle_rounded.
DONE WHEN: `cargo test --features test-support` passes, `cargo clippy --workspace --all-targets` has no warnings,
`cargo fmt --all --check` is clean. In your report, name the first failing test you wrote.
```

**Claude review checklist:** hairline sequences unchanged (diff existing tests: none modified except the layer
contract test); `shape_layers` untouched; no change needed in `mirror.rs`/`drawable_snapshot.rs`; `ThickLine` only in
Outline layer; thickness scaling uses mean scale; cull margin updated.

---

## Step 2 — Bezier

**Executor:** vibe. **Scope:** `crates/aberred-core/src/components/shape.rs`,
`crates/aberred-core/src/systems/shape_geometry.rs`, `crates/aberred-render/src/systems/shape.rs` (only if the
cull margin helper needs the new kind), `crates/aberred-core/src/components/mod.rs` (doc line listing kinds).
**Acceptance:** `just test-fast`, then `just check`. **Stage / message:** those files. Suggested:
`Add ShapeKind::Bezier cubic curve shape`

**Vibe spec (pass verbatim):**
```
Repository: /home/idaho/Projects/aberredengine (branch develop-0.2.0). Read AGENTS.md, CLAUDE.md,
.claude/rules/tdd.md. TDD mandatory (failing test first).
PREREQUISITE (already committed): Shape.thickness: Option<f32>, Shape::with_thickness, DrawOp::ThickLine.

GOAL: add `ShapeKind::Bezier { c1: Vec2, c2: Vec2, to: Vec2 }`: a cubic bezier from local (0,0) to `to` with
control points c1, c2 (all local coordinates, like Line.to).

CHANGES
1. shape.rs: add the variant, `pub fn bezier(c1: Vec2, c2: Vec2, to: Vec2, color: Color) -> Self`, docs (outline
   only; `filled` and `outline_color` do not affect it, like Line; hairline join gaps between the 24 segments are
   accepted, use thickness for gap-free curves). Update module/struct docs and the kinds list in
   crates/aberred-core/src/components/mod.rs. Extend `constructors_pick_their_kind`.
2. shape_geometry.rs:
   - `shape_layers`: add `ShapeKind::Bezier { .. }` to the FIRST arm `(Point | Line, _) | (_, false)` so it is
     outline-only. The catch-all arm would otherwise give a filled Bezier a Fill layer that emits nothing.
   - `shape_ops` (exhaustive match; Outline layer): transform c1, c2, to and the origin (0,0) with the existing
     `Transform::point` (an affine transform of a cubic is the cubic of the transformed control points, so this is
     exact). Sample the curve at N = 24 equal parameter steps into N connected segments: P(t) =
     (1-t)^3 P0 + 3(1-t)^2 t P1 + 3(1-t) t^2 P2 + t^3 P3. The first point must equal point(0,0) EXACTLY and the last
     point(to) EXACTLY (compute them directly, not through the polynomial, to avoid rounding). Emit
     `DrawOp::Line` per segment (hairline, thickness None) or `DrawOp::ThickLine` per segment when thickness is
     Some(t): width = t * mean scale. ONE rule, document it in a comment and test it: every thick segment is
     extended by width/2 at BOTH ends (square caps), exactly like thick Line segments. Overlap at interior joints
     is harmless for opaque colours and closes the gaps.
   - `shape_cull_bounds`: AABB of the four transformed control points (the curve lies inside their convex hull;
     the caller adds the thickness margin).
3. Add `ShapeKind::Bezier` arms wherever the compiler demands (render crate uses only DrawOps, so likely none).

TESTS FIRST
 - first and last emitted points equal the transformed start and `to` exactly;
 - 24 segments, connected (segment i end == segment i+1 start);
 - a straight cubic (c1,c2 on the chord) yields collinear points;
 - rotated 90 degrees / scaled / negative-scale shapes: endpoints and the cull box enclose every emitted vertex
   (follow the existing `the_box_encloses_every_emitted_vertex` and `negative_scale_keeps_min_below_max` style);
 - `shape_layers` for a filled Bezier with an outline_color: exactly one Outline layer, no Fill layer
   (follow `shape_layers_follow_the_colour_model`);
 - extend `each_layer_emits_only_its_own_op_kinds` with a Bezier;
 - thickness Some(t) yields ThickLine ops only, thickness None yields Line ops only;
 - reusing the buffer does not grow capacity beyond the segment count (existing test style).

CONSTRAINTS: only the files listed; no Lua, no docs. DONE WHEN: cargo test --features test-support, cargo clippy
--workspace --all-targets (no warnings), cargo fmt --all --check. Report the first failing test written.
```

**Claude review checklist:** `shape_layers` first-arm edit present (grep it); endpoints computed exactly; the join
rule is one simple documented rule; no `unwrap` on empty ops.

---

## Step 3 — RoundedRect

**Executor:** vibe. **Scope:** `crates/aberred-core/src/components/shape.rs`,
`crates/aberred-core/src/systems/shape_geometry.rs`, `crates/aberred-core/src/components/mod.rs`.
**Acceptance:** `just test-fast`, then `just check`. **Suggested message:**
`Add ShapeKind::RoundedRect built from triangles`

**Vibe spec (pass verbatim):**
```
Repository: /home/idaho/Projects/aberredengine (branch develop-0.2.0). Read AGENTS.md, CLAUDE.md,
.claude/rules/tdd.md. TDD mandatory.
PREREQUISITE (already committed): Shape.thickness, DrawOp::ThickLine, ShapeKind::Bezier.

GOAL: add `ShapeKind::RoundedRect { size: Vec2, radius: f32 }`: top-left corner at local (0,0), like
ShapeKind::Rect.

SEMANTICS
 - Effective radius r = radius clamped to [0, min(|size.x|,|size.y|)/2]. r == 0 must behave exactly like Rect
   (same ops as the Rect arm for both layers).
 - Build ONE outline polygon (a Vec<Vec2> in local space): four corner arcs of 8 segments each (9 points per
   corner incl. both ends, shared tangent points not duplicated), clockwise or counter-clockwise consistently,
   then transform every vertex with `Transform::point` (so Scale/Rotation/origin work; unlike raylib's
   draw_rectangle_rounded this is rotation-safe).
 - Fill layer: triangle fan from the rect centre (transformed) over the outline polygon via the existing
   `push_triangle` (it fixes winding and skips zero-area triangles). Negative scale must still yield CCW
   triangles.
 - Outline layer, hairline (thickness None): `push_ring` over the transformed polygon (it handles <3 points).
   Thickness Some(t): ThickLines along the polygon edges built from the polygon INSET by width/2 (width = t *
   mean scale); simplest accepted approach: build the polygon with radius' = max(r - width/2, 0) on a rect shrunk
   by width/2 on every side, then emit ThickLines along it with caps extended by width/2.
   Document the approach in a comment.
 - `shape_layers`: normal (Fill + optional Outline when filled, Outline only when not), i.e. goes through the
   catch-all like Rect/Circle: no change needed, but add a test.
 - `shape_cull_bounds`: the rect bounds (same as Rect, transformed corners).

CHANGES: shape.rs variant + `Shape::rounded_rect(size, radius, color)` + docs + kinds list in
crates/aberred-core/src/components/mod.rs + extend `constructors_pick_their_kind`; shape_geometry.rs arms.

TESTS FIRST: r == 0 equals Rect ops; radius larger than half the short side is clamped (every vertex inside the
rect bounds); fill triangles are all CCW (see `every_triangle_is_counter_clockwise_for_raylib`) including with
negative scale; every emitted vertex lies inside the cull box; outline ring is closed and has 4*8 arcs' worth of
segments; thickness case emits ThickLine only and its stroke stays within the rect bounds; layer-contract test
`each_layer_emits_only_its_own_op_kinds` extended; rotation 90 degrees maps the polygon correctly.

CONSTRAINTS: only listed files; no Lua, no docs; do not call raylib rounded-rect functions.
DONE WHEN: cargo test --features test-support, cargo clippy --workspace --all-targets (no warnings), cargo fmt
--all --check. Report the first failing test written.
```

**Claude review checklist:** r=0 parity test exists; no raylib rounded-rect usage; vertex count per corner is
consistent; no duplicate vertices at the tangent points.

---

## Step 4 — Lua parity

**Executor:** vibe. **Scope:** `crates/aberred-lua/**`, `assets/scripts/engine.lua`, `assets/scripts/.luarc.json`.
**Acceptance:** `just test-fast`, then `just check`. **Suggested message:**
`Expose Bezier, RoundedRect and shape thickness to Lua`

**Vibe spec (pass verbatim):**
```
Repository: /home/idaho/Projects/aberredengine (branch develop-0.2.0). Read AGENTS.md, CLAUDE.md,
.claude/rules/tdd.md, .claude/rules/lua-api.md, .claude/skills/add-feature/SKILL.md. TDD mandatory.
PREREQUISITE (already committed, in aberred-core): Shape.thickness + Shape::with_thickness, ShapeKind::Bezier{c1,c2,to}
+ Shape::bezier, ShapeKind::RoundedRect{size,radius} + Shape::rounded_rect.

GOAL: Lua parity for the new shape features, following the existing shape bindings exactly.

1. Builder (crates/aberred-lua/src/resources/lua_runtime/entity_builder/shape.rs): add `with_shape_bezier(c1x, c1y,
   c2x, c2y, tox, toy, r, g, b, a)`, `with_shape_rounded_rect(w, h, radius, r, g, b, a)` next to the existing
   with_shape_line/rect (copy their signature style/arg order and `builder_method!` descriptions), and the modifier
   `with_shape_thickness(t)` next to with_shape_filled/outline_color/origin (goes through `modify_shape`; must error
   if no shape kind was set first, like the others). Update descriptions that say "1 px" to say the hairline is the
   default and thickness changes it. Also handle spawn_data.rs if shapes are stored there as Shape (they are:
   `shape: Option<Shape>`), nothing else should be needed.
2. Runtime commands: add `EntityCmd::SetShapeThickness { entity_id, thickness: Option<f32> }` in
   lua_runtime/commands.rs (near l.502-519); route it in src/systems/lua_commands/entity_cmd.rs (routing ~l.180 and
   `process_shape_cmd` ~l.570-604, keep the `unreachable!()` fallback consistent); add `define_entity_cmds` rows in
   engine_api/macros.rs (~l.666-726): `entity_set_shape_bezier`, `entity_set_shape_rounded_rect` (both produce
   `EntityCmd::SetShapeKind`) and `entity_set_shape_thickness` (a number sets Some, nil/0 or negative clears to
   None — pick: nil clears; document in `desc`). Collision twins are generated automatically.
3. Tests first (existing style in those files): builder tests (`each_kind_stores_the_matching_shape`,
   `modifiers_set_fill_outline_and_origin`, `modifiers_require_a_shape_kind_first`); command tests
   (`shape_style_cmds_change_only_their_own_field`, `set_shape_kind_replaces_only_the_geometry`); the API test in
   engine_api/entity.rs asserting the number of kinds (6 -> 8) and collision twins.
4. Regenerate checked-in stubs: `cargo run -- --create-lua-stubs` and `cargo run -- --create-luarc` (from the
   workspace as the repo's justfile/docs instruct) so `assets/scripts/engine.lua` and `assets/scripts/.luarc.json`
   are current; add the new function names to the `meta_functions_complete` list in
   crates/aberred-lua/tests/stub_generator_integration.rs. `crates/aberred-lua/tests/engine_bad_args.rs` sweeps all
   meta functions automatically; it must pass.
CONSTRAINTS: scope = crates/aberred-lua/** plus the two generated asset files. No docs (a later step does docs).
No macro_rules outside aberred-lua. DONE WHEN: `cargo test --features test-support` and
`cargo test --no-default-features --features test-support` pass, clippy clean for both feature sets, the stub drift
tests pass. Report the first failing test written.
```

**Claude review checklist:** stubs regenerated (drift tests green), twins exist, nil-clears documented,
`SetShapeThickness` handled in `process_shape_cmd`.

---

## Step 5 — Docs, final gate, push

**Executor:** vibe (docs), then Claude reviews. **Scope:** `RUST-GAME-GUIDE.md`, `guide-check/src/**`,
`assets/scripts/README.md`, `docs/lua-interface-architecture.md`, `.claude/rules/render-world.md`,
`.claude/rules/lua-api.md`, optionally `assets/scripts/scenes/shapes_showcase.lua`.
**Acceptance:** `just check` (full: fmt, tests in all configs, clippy, `check-guide`, `doc-links`).
**Suggested message:** `Document Bezier, RoundedRect and shape thickness`

**Vibe spec (pass verbatim):**
```
Repository: /home/idaho/Projects/aberredengine (branch develop-0.2.0). Documentation-only step. Features already
implemented and committed: Shape.thickness (Option<f32>, with_thickness; DrawOp::ThickLine; world units x mean
scale; Rect/RoundedRect outline inset by t/2, Line/Bezier/Polygon straddle; Point/Circle/Ellipse ignore it),
ShapeKind::Bezier{c1,c2,to} (outline only, 24 segments, hairline join gaps accepted, use thickness),
ShapeKind::RoundedRect{size,radius} (radius clamped to half the short side, rotation-safe triangles); Lua:
with_shape_bezier, with_shape_rounded_rect, with_shape_thickness, entity_set_shape_bezier,
entity_set_shape_rounded_rect, entity_set_shape_thickness (+ collision_ twins).
Read the actual code and tests to confirm every claim before writing it.

UPDATE
 - RUST-GAME-GUIDE.md "### Shapes" (~l.1504-1557): remove "lines and outlines are 1 px wide" absolutes, document
   thickness, Bezier, RoundedRect; update the Shape quick-reference row (~l.1577). Every NEW ```rust fence must be
   mirrored VERBATIM in guide-check/src/spawning.rs (existing shape fences ~l.65-102) or
   crates/aberredengine/tests/guide_snippets.rs fails.
 - assets/scripts/README.md "## Shape Component" (~l.6156-6260): builder table, modifiers ("1 px outline" text),
   "How Shapes Render", "Runtime Shape Control" table.
 - docs/lua-interface-architecture.md (~l.77, 520-521, 601-613), .claude/rules/render-world.md "### Shapes"
   (field list, thickness semantics, the rect inset rule, the ring-gap warning now relevant to Bezier),
   .claude/rules/lua-api.md (~l.246).
 - Optional: add a Bezier + RoundedRect + thick line example to assets/scripts/scenes/shapes_showcase.lua.
CONSTRAINTS: docs and the showcase script only; no Rust source changes besides guide-check snippets.
DONE WHEN: `just check` passes (this includes check-guide and doc-links). Report which fences were added.
```

**Claude review checklist:** every doc claim matches the code; fences mirrored; `just check` green.

**End of plan 01 checklist:**
- [ ] steps 1–5 committed by the user (five small commits)
- [ ] `just check` green on the committed tree
- [ ] user pushed `develop-0.2.0` (tell the user explicitly this is their action)
- [ ] note the pushed commit hash for plan 03's first step (`cargo update -p aberredengine`)

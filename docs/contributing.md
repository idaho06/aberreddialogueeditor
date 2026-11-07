# Contributing

This guide covers the development setup, the conventions the code follows, and step-by-step recipes for the
changes people usually make. Read [architecture.md](architecture.md) for the big picture first and
[gotchas.md](gotchas.md) before touching threads, imgui or the engine glue — most bugs in this codebase were
one of those rules broken.

## Setup

1. Install a stable Rust toolchain (edition 2024), [`just`](https://github.com/casey/just) and the native
   raylib build dependencies (see the [README](../README.md#build-and-run)).
2. `just run` starts the editor. `RUST_LOG=aberreddialogueeditor=debug just run` adds event logging.
3. `just check` is the gate every change must pass: `cargo fmt --check`, `cargo clippy --all-targets -- -D
   warnings` and `cargo test`. Run `just fmt` to fix formatting.

The engine is a git dependency pinned by `rev` in `Cargo.toml`. To move to a newer engine commit, change the
`rev`, run `cargo update -p aberredengine`, fix what breaks, and re-read `gotchas.md`: its engine facts are stated
against the pinned commit.

## Conventions

- **Comments and rustdoc:** present tense, explaining why (the constraint, the engine quirk, the invariant)
  rather than what the next line does, and never how the code used to be. Every module has a `//!` header and
  every `pub` item a `///` line. `cargo doc --no-deps` must stay free of warnings.
- **Layering:** `src/model` and the pure parts of `src/graph` take no imgui, ECS or window types (see their
  module headers for the exact engine imports allowed). `src/ops` is plain functions over the model. Only the
  files listed in [architecture.md](architecture.md#graph-pipeline-srcgraph) touch the ECS.
- **Mutations:** every change to a `DialogueDoc` goes through a named mutator that maintains `revision`,
  `content_revision` and `dirty`. Do not add a public `&mut` accessor to a node.
- **Locks:** lock the `WorkspaceMutex` only for short snapshots and per action. Never hold it across imgui widget
  calls, dialog requests or rfd code. `state::with_workspace` makes the right shape the easy one.
- **Logging:** log events (an action applied, a file operation, a dialog result), never per frame, and never
  document content — no node text, speakers or file contents.
- **Tests:** unit tests live in `#[cfg(test)]` modules next to the code (this is a binary crate, so `tests/`
  cannot import its modules). Shared helpers are in `src/testkit.rs`. Never save over `tests/fixtures/*.json`;
  save tests write to a `testkit::TempDir`.
- **Commits:** small and focused, with the tests for the behaviour they add. Run `just check` before pushing.

## Recipes

### A menu action

1. Add an `EditorAction` variant in `src/scenes/editor/actions.rs`.
2. Add its arm in `apply_action` (a no-op without an active doc where that makes sense) and a unit test in the
   same file. Actions are applied under a short workspace lock; an action that needs a native dialog returns it
   in `ActionEffects::dialog` instead of opening it.
3. Add the menu item in `menu.rs` and, if it has a shortcut, the key in `shortcuts.rs` (`ctrl_shortcuts` or
   `plain_shortcuts`; shortcuts must stay gated on `!want_text_input` and on no modal being open).
4. Viewport-carrying actions (`FrameAll`, `Relayout`, `AddNode`) read the viewport from `display_viewport(ui)` at
   the place that produces the action.

`apply_action` logs `{action:?}`, so an action must never carry node text or other document content.

### An Inspector field or `Edit` variant

1. Add the variant to `Edit` in `src/scenes/editor/inspector/edit.rs`, an arm in the pure `apply_edits` that goes
   through the existing doc mutators, and an arm in `Edit::log_kind`. The classification match is exhaustive, so
   the compiler forces it; log only the kind and node key (plus structural detail), at `trace!` for per-keystroke
   kinds and `debug!` for structural ones.
2. Add the widget in `node_fields.rs` or `outputs.rs`. It pushes the edit only when the widget reports a change.
   Widget ids come from the `NodeKey` and from indices, never from the edited text — the panel body is already
   wrapped in `push_id(node{key})`.
3. Add tests for `apply_edits` covering the normal case, a vanished `NodeKey` and an out-of-range index (both
   must be ignored without a panic or a revision bump).

Inspector edits deliberately bypass `apply_action` for the logging reason above.

### A new node field in the model

1. Add the field to `EdNode` (or `EdChoice`) in `src/model/doc.rs` and a named mutator that bumps `revision` and
   `content_revision` and sets `dirty` (use the existing private `edit_node`/`edit_choice` helpers).
2. Map it in `from_def` and `to_def` (`src/model/convert.rs`) if the engine's `DialogueDef` has a matching
   field. Keep `to_def` total and exact, and `from_def` tolerant.
3. If it appears on the canvas, show it in `describe_node` (`src/graph/describe.rs`) and include it in the node
   signature in `projection_plan.rs`, otherwise the node does not respawn when the field changes.
4. Add the Inspector edit (previous recipe) and tests for the mutator's bump rules and the convert round trip.

### A condition or effect kind

Needed when the engine's `DialogueCondition` or `DialogueEffect` gains a variant.

1. Extend `CondKind` (`of`, `name`, `KINDS`) and the pure `convert_condition` in `inspector/expr.rs`, plus the
   per-kind arm in `condition_editor`. For effects, extend `EffectKind`, `convert_effect` and the value-widget
   arm in `inspector/effects.rs`.
2. Keep the serde-shape tests green: every producible variant must serialize to the engine's internally tagged
   snake_case JSON. Add a case for the new variant.
3. Check `lint.rs` — a new variant that references keys or nodes may need a lint rule.

### A lint rule

1. Add a `ProblemKind` variant in `src/model/lint.rs` (declaration order is the report order within a node) and
   its detection in `lint`.
2. Add tests in the same file. Two engine-parity tests guard against drift: soundness (a doc with no lint errors
   always passes `engine_gate`, over the fixtures and a generated mutation family) and coverage (every engine
   error variant maps to a lint error). If the rule mirrors an engine rule, extend both.
3. The Problems window needs no change: rows are generic over `Severity` and `Problem`, and a row is clickable
   only while its node still exists. Never reimplement engine rules in the model beyond reporting; the save gate
   is the engine's own `DialogueTree::try_from`.

### A window or panel

1. Write a `snapshot(app_state)` that locks the workspace briefly (through `state::with_workspace`) and clones
   what the panel needs.
2. Write `draw` so that it runs with the guard dropped and collects intents (`EditorAction`s or `Edit`s), then
   takes a fresh short lock to apply them.
3. Register the draw call in `gui.rs`, in the panel order documented there.
4. A window that can be hidden must not be drawn at all while hidden: imgui's `p_open` does not hide it (see
   `problems::should_draw`).
5. Widget ids: `push_id_usize(index)` per row plus fixed `###suffix` labels, never content text.
6. Per-frame state that must survive between frames lives in a `thread_local!` (GUI callbacks are plain `fn`
   pointers and cannot capture).

### A modal

1. Add a flag to `Workspace` in `src/model/doc.rs` (like `show_controls` and `show_about`) and include it in
   `Workspace::has_modal()`. That one line also suppresses keyboard shortcuts while the modal is open.
2. Add an `EditorAction` that sets the flag (first recipe) and a Help or other menu item that emits it.
3. In `src/scenes/editor/modals.rs`, add the flag to `ModalSnap`, a `ModalChoice` for its button, a branch in
   `draw` at the right place in the priority order (notification, close, quit, Controls, About), and the
   flag-clearing arm in the pure `apply_choice_ws`, with a test like `about_ok_clears_the_show_about_flag`.
4. Modal ids must not depend on content; use a fixed `###id` suffix.

### A scene

1. Create `src/scenes/<name>.rs` with an enter observer (`On<SceneEntered>`), the scene systems and an exit
   observer (`On<SceneExited>`), and register them in `main.rs` with `add_scene`, `on_scene_enter`,
   `add_scene_system` and `on_scene_exit`. Scene entities are despawned by the engine on exit; resources are not,
   so reset what the scene changed (the intro clears the post-process chain).
2. Global systems (`add_system`, `add_late_system`) run in every scene. If they need a resource, insert it in
   `systems::setup::load_state`, not in a scene's enter observer.
3. Assets used by a scene are embedded and queued as `RenderAssetCmd`s in `load_state`; the engine waits for
   them before leaving `Setup`.

### A test fixture

1. Put the JSON in `tests/fixtures/`. Name invalid ones `invalid_<what>.json`.
2. Embed it as a constant in the `json` module of `src/testkit.rs` with `include_str!`.
3. Tests that save must write to a `TempDir`; fixtures are read-only inputs.

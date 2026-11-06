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
- Every mutation on `DialogueDoc` goes through a named mutator that bumps `revision` and sets `dirty`;
  `set_node_pos`, `select` and `mark_clean` only bump `revision` (view-only, never saved).
- `from_def`/`to_def` convert to/from the engine's `DialogueDef`, and only at load/save.
- `lint::lint` reports ALL problems of a doc (cached per `revision` via `DialogueDoc::problems`);
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

- Every user intent (menu item, Ctrl shortcut, tab close button) is an `EditorAction`, applied in ONE place:
  `scenes/editor::actions::apply_action` (pure over `Workspace`; native-dialog requests go through its
  `Option<&AppState>` parameter — the GUI passes `None` under the lock and dispatches the returned request after).
- Lock discipline: lock `WorkspaceMutex` only for short snapshots and per action, never while calling
  `request_async_dialog`/`dialog_in_flight` or any rfd code; widgets are drawn with the guard dropped.
- Shortcuts are read with `ui.is_key_pressed(Key::..)` + `io().key_ctrl`/`key_shift`, only when
  `!io().want_text_input`. Modals (error queue, unsaved-changes close/quit) open from the frame's top-level
  scope after the menu; one at a time, error modal first. While a native dialog is in flight, tab closes are
  ignored (a pending Save As is applied by tab index).

## Logging

- Default level is `info` (set in `main.rs`); configure at run time with `RUST_LOG`, e.g.
  `RUST_LOG=aberreddialogueeditor=debug just run`.
- Log on events only (an action applied, a dialog result, a file operation, a lint-cache recompute) — never
  inside per-frame GUI paths.
- What is logged where: file operations and outcomes in `ops`, dialog requests/results/notifications in
  `systems/file_dialogs.rs`, the inserted shared handles in `systems/setup.rs`, applied/ignored `EditorAction`s
  in `scenes/editor/actions.rs`, modal choices and outcomes in `scenes/editor/modals.rs`, `from_def` load
  warnings plus summary in `model/convert.rs`, and the lint-cache recompute (revision, error/warning counts)
  in `model/doc.rs`.
- Never log document content: no node text, speaker text or whole file contents.

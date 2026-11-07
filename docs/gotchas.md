# Gotchas

Hard-won engine / imgui / threading facts. Each entry: the symptom you see, then the rule. Engine facts are
verified against `aberredengine` @ `0547e57` (the `Cargo.lock` pin, branch `develop-0.2.0`); editor facts
against this repo's `src/`.

## Engine / ECS

- **B0002 — never request the same resource twice in one system.** E.g. `ResMut<AppState>` plus `Res<AppState>`
  in one param list panics at startup with `error[B0002]` (bevy_ecs `system_param.rs`); `cargo check` cannot
  see it. Rule: systems take `Res<AppState>` only (never `ResMut`, never twice); mutate shared state through
  the `Arc<Mutex<..>>` handles it carries.
- **Render-only resources panic at schedule init.** `TextureStore`, `FontStore`, `ShaderStore`,
  `RaylibAccess` exist only on the render side; requesting them from a logic system/observer panics when the
  schedule initializes — invisible to `cargo check`. Rule: use them only in GUI callbacks (`ctx.textures` /
  `ctx.fonts`); logic systems communicate with render through messages (`RenderAssetCmd`).
- **GUI callbacks are `fn` pointers, not closures.** `pub type GuiCallback = fn(&mut GuiCtx)`
  (`GuiCtx { ui, signals, intents, textures, fonts, app_state }`) — no captures. Rule: per-frame GUI state
  lives in `thread_local!`s (`tabs.rs` `LAST_ACTIVE`, `context_menu.rs` `PENDING`). The callback runs on the
  **render thread**; systems/observers run on the **logic thread**.
- **`AppState` handles are inserted once in `on_setup`.** `AppState::insert` requires `Clone + Send + Sync`;
  an `Arc<Mutex<T>>` inside aliases across the render thread's clone. Rule: insert `UiCaptureHandle`,
  `WorkspaceMutex`, `AsyncFileDialogMutex` exactly once in `systems::setup::load_state`; never re-insert per
  frame.
- **The engine does NOT mask input for scene GUIs.** imgui capture is only tracked for the F11 debug overlay;
  a scene's own `gui_callback` frame passes `debug_active = false`, so clicks/keys reach logic even over imgui
  windows. Rule: the GUI publishes `io().want_capture_mouse` / `want_capture_keyboard` into the shared
  `UiCapture` atomics every frame (`gui.rs`); logic input systems must gate on them (`graph_frame_system`).
- **`InputState::mouse_world_*` lags the camera.** The engine computes them with the camera as it was before
  this tick's systems ran. Rule: never pan or hit-test with `mouse_world_*`; compute
  `screen_to_world(&doc.camera, viewport, mouse_screen)` from the doc camera after this tick's wheel/pan
  (`build_sample` in `graph/input_system.rs`).
- **`Action2` defaults to Enter OR right mouse; `Action3` is middle mouse.** Rule: at editor scene enter,
  rebind `Action2` to right mouse only via `ResMut<InputBindings>` (`lifecycle.rs`), so the right button is a
  plain button for the interaction state machine. There is no raw right/middle button otherwise.
- **Font metrics arrive asynchronously.** `FontMetricsStore::resolve_key(font, &pending, &mut warn_cache)` then
  `store.0.get(key)?.measure_text(text, size)`; the entry may be `None` until the font loads. Rule: tolerate
  `None` (`metrics_ready = false`), spawn nodes without their text items and respawn them once metrics land —
  `metrics_ready` is part of the node signature, so the projection does this automatically.
- **Children of graph entities must be spawned WITH a `GlobalTransform2D`.** The engine's
  `propagate_transforms` inserts it via deferred commands, so without it a freshly spawned child renders one
  frame at its raw local offset (near the world origin). Rule: spawn every `GraphPart` child with
  `child_global(parent_world, local)` (`projection.rs`); root entities must NOT get one (their own
  `WorldPosition` is world-space).
- **The engine polls the window close flag itself** (`window_should_close()` in the engine's run loop), so
  closing the OS window quits without the editor's dirty-check. Rule: accept it (documented limitation); the
  unsaved-changes confirm only runs on File > Quit / Ctrl+Q.

## imgui

- **imgui ids must never depend on edited text, labels, dirty state, or indices that can shift.** Rule: tab
  ids are `###doc{doc_id}` (`actions::tab_label` — everything before `###` is display-only); choice headers
  are `"Choice N: <truncated>###choice{i}"`; section headers use fixed `###` suffixes (`###effects`,
  `###onenter`, `###loadwarn`, `###row`); list rows and recursion levels are wrapped in `push_id_usize(..)`;
  the Inspector body is wrapped in `push_id(node{key})`; Nodes rows use `{id}##{key}`. Typing must never change
  a widget id or the input loses focus / the section collapses.
- **imgui's `p_open` does not hide a window.** `Begin(name, &mut open)` only shows the close button and writes
  `false` into it on click; a window whose flag is already false is still built. Rule: a hidden window is not
  drawn at all — early-return on `!visible` (`problems.rs::should_draw`).
- **On the `SET_SELECTED` frame imgui still reports the previously selected tab as selected**, and a clicked X
  makes imgui re-select a neighbour in the same frame. Rule: `tabs::draw` ignores every tab's selected report on
  the `SET_SELECTED` frame (`clicked_focus`), and a frame with a clicked X drops the pending focus and
  re-asserts the real active tab on the next frame (`resolve_focus` + `LAST_ACTIVE` reset to `None`).
- **imgui modals do not block io-level key polling.** A modal popup masks mouse input to other windows, but
  `ui.is_key_pressed` reads io-level key state and still fires while a modal is open. Rule: gate keyboard
  shortcuts on `Workspace::has_modal()` — `gui.rs` skips `shortcuts::poll` for that frame while any modal is
  open.

## Editor internals

- **`set_link` marks the doc dirty even when the link is unchanged** (`DialogueDoc::set_link` bumps
  unconditionally). Rule: the interaction state machine defers the link detach (`SetLink .. End`) until the
  pointer actually moves > 4 px screen from the press, so a plain click on a linked pin emits no mutation at
  all (`interaction.rs` `DraggingLink.detached`).
- **Never hold the `WorkspaceMutex` across imgui/rfd calls.** Rule: lock only for short snapshots and per
  action; draw widgets with the guard dropped; `apply_action` runs with `app_state: None` under the lock and
  the returned dialog request is dispatched after the guard drops (`gui.rs`).
- **Never log document content.** Rule: inspector edits use their own `Edit` enum + pure `apply_edits` — NOT
  `EditorAction`/`apply_action`, which logs `{action:?}` and would leak node text. `apply_edits` logs only the
  edit kind and node key (`trace!` per keystroke, `debug!` structural). Likewise a `SaveRejected`
  notification's message quotes node ids, so `file_dialogs.rs` logs only its kind.
- **The lint cache is keyed on `content_revision`**, not `revision`: `doc.problems()` recomputes only when
  `content_revision` changed, so view-only changes (pos/select/camera) never re-run the linter.
- **Do NOT run manual save tests on the repo fixtures.** `tests/fixtures/*.json` are loaded by the tests
  (`include_str!` / `fixture(...)`); save tests must write to a temp dir (`testkit::TempDir`). In the app, use
  Save As to a copy — never save over a fixture.

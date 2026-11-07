# AberredDialogueEditor

A visual node-graph editor for [AberredEngine](https://github.com/idaho06/aberredengine) dialogue JSON files.
Rust binary crate on the `aberredengine` git dependency (branch `develop-0.2.0`, pinned in `Cargo.lock`).
The graph itself is drawn as world-space engine entities (`Shape` + `DynamicText`); imgui provides the menu
bar, tabs, windows, Inspector and modals.

The editor loads any dialogue JSON the engine accepts (plus some it rejects, so you can fix them), auto-lays
the graph out, lets you edit every field, shows all validation problems live, and saves JSON that the engine
loads unchanged.

## Documentation

- [docs/architecture.md](docs/architecture.md) — workspace model, thread model, graph pipeline, module map,
  and "how to add ..." recipes.
- [docs/gotchas.md](docs/gotchas.md) — the hard-won engine/imgui/threading facts, each with its symptom and rule.
- [docs/plans/](docs/plans) — the implementation plans (`00-overview.md` .. `04-editor-inspector-validation.md`).
- `CLAUDE.md` — the fast reference for working on this repo.

## Build, run, test

All acceptance commands are `just` recipes (run from the repo root):

| Recipe      | What it runs                                                        |
|-------------|---------------------------------------------------------------------|
| `just check`| `cargo fmt --check` + `cargo clippy --all-targets -- -D warnings` + `cargo test` (default recipe) |
| `just test` | `cargo test`                                                        |
| `just lint` | `cargo clippy --all-targets -- -D warnings`                         |
| `just run`  | `cargo run`                                                         |
| `just fmt`  | `cargo fmt`                                                         |

Run with debug logging (projection/lint/action events) via:

```sh
RUST_LOG=aberreddialogueeditor=debug just run
```

The default log level is `info` (set in `src/main.rs` via `env_logger`); `RUST_LOG` overrides it at run time.

The window is configured in `config.ini` (1280x720, `pixel_snap_camera = false`, 60 Hz sim/render).

## Controls

The same list is available in the app under Help > Controls (the `CONTROLS` list in
`src/scenes/editor/modals.rs` is the source of truth):

| Action | Input |
|---|---|
| Move a node | Left-drag its body or header |
| Select a node | Left-click it |
| Connect nodes | Drag from a right-side pin onto another node |
| Disconnect | Drag a linked pin off onto empty space |
| Context menu | Right-click |
| Pan | Middle-drag or Right-drag |
| Zoom | Mouse wheel |
| Frame all | F or View > Frame all |
| Re-layout | View > Re-layout |
| Add node | Right-click canvas > Add node here, or Edit > Add node |
| Delete node | Delete key or right-click > Delete node |
| New / Open / Save / Save As / Close tab / Quit | Ctrl+N / Ctrl+O / Ctrl+S / Ctrl+Shift+S / Ctrl+W / Ctrl+Q |
| Toggle the Problems window | Ctrl+Shift+P or View > Problems |

Shortcuts are read from imgui's key state and only fire when no text input is focused. Around the graph:
one tab per open document (close button on each tab; closing a dirty tab asks first), a Nodes window listing
the active doc's node ids and its load warnings, an Inspector window editing the selected node (id, speaker,
text, labels, start, next/choices with per-choice target, `when` condition, `effects`, labels, and the node's
on-enter effects), a Problems window with all lint findings (click a row to select and centre the node), and
a status bar at the bottom with the file path + dirty marker, error/warning counts, node count and zoom.

## File format

The saved file is **exactly the engine's `DialogueDef` serialized with `serde_json::to_string_pretty`** (plus
a trailing newline, written atomically: the editor writes `<path>.tmp` in the same directory, then renames it
over the target — see `ops::save_doc` in `src/ops/mod.rs`).

- No extra fields are added. Node positions are **not** saved (`pos` is editor-side view state only; every
  open re-runs the auto-layout).
- Saving is gated by the engine itself: `lint::engine_gate` runs `DialogueTree::try_from(to_def(doc))` first.
  A rejected doc writes nothing — no file, no `.tmp` — and the GUI shows the engine's own error message in a
  "Cannot save" modal. Engine validation reports the first error only: duplicate node id, missing start,
  a node with both `next` and `choices`, an explicit empty `choices` list, an unknown `next`/choice target.
  Warnings such as unreachable nodes never block saving.
- Loading is tolerant: anything the engine would reject still opens, with load warnings (kept as dangling
  links) so you can fix it in the editor.

Example (trimmed from `tests/fixtures/guard.json`):

```json
{
  "start": "greet",
  "nodes": [
    {
      "id": "greet",
      "speaker": "Guard",
      "text": "Halt! Who goes there?",
      "choices": [
        { "text": "A friend.", "next": "friend" },
        {
          "text": "Here is my pass.",
          "next": "pass",
          "when": { "type": "flag", "key": "has_pass" },
          "effects": [{ "type": "set_flag", "key": "pass_shown" }]
        }
      ]
    },
    { "id": "friend", "speaker": "Guard", "text": "Hm. Move along." },
    { "id": "pass", "speaker": "Guard", "text": "Carry on.", "next": "friend" }
  ]
}
```

A node's optional fields (`speaker`, `labels`, `on_enter`, `next`, `choices`) may be omitted; a choice's
optional fields (`next`, `when`, `effects`, `labels`) likewise. A choice whose `next` is absent ends the
conversation; a node with neither `next` nor `choices` is terminal. Conditions and effects serialize as
internally tagged enums (`"type"` discriminator, snake_case), e.g. `{"type":"flag","key":"has_pass"}`,
`{"type":"integer","key":"gold","op":"ge","value":10}`, `{"type":"not","cond":{...}}`,
`{"type":"add_integer","key":"visits","delta":1}`.

## Using the saved file in a game

In Rust (all names from `aberredengine::prelude`):

```rust
use aberredengine::prelude::*;
use std::sync::Arc;

// In the setup hook: read, parse and validate the file, store the tree under a key.
// Registered with `EngineBuilder::on_setup(setup)`: a plain fn returning (), the same style
// as this editor's `systems::setup::load_state`. on_setup systems return (), so the load
// error is handled inside.
fn setup(mut dialogues: ResMut<DialogueStore>) {
    match load_dialogue("assets/dialogue/guard.json") { // -> Result<DialogueTree, DialogueError>
        Ok(tree) => {
            dialogues.insert("guard", Arc::new(tree)); // insert(key, Arc<DialogueTree>) returns the replaced tree
        }
        Err(err) => log::error!("cannot load guard dialogue: {err}"),
    }
}

// Later: spawn a runner for a stored tree and drive it.
fn start_talk(dialogues: Res<DialogueStore>, mut commands: Commands) {
    if let Some(tree) = dialogues.get("guard") {             // -> Option<&Arc<DialogueTree>>
        commands.spawn(DialogueRunner::new(Arc::clone(tree)));
    }
}
```

- `load_dialogue(path)` reads the file, parses it as `DialogueDef` JSON and validates it with
  `DialogueTree::try_from`; I/O and JSON failures are `DialogueError::Io` / `DialogueError::Json`.
- `DialogueStore` is a pre-inserted resource mapping a key to `Arc<DialogueTree>`; `insert` returns the
  replaced tree, `get` the stored one.
- `DialogueRunner::new(tree)` is a component running one conversation on one entity; request steps with
  `advance()` / `choose(index)` / `cancel()`, read the current node with `speaker()` / `text()` /
  `has_choices()` / `choices()` (a `ChoiceView` has `index` and `available`), and listen for the
  `DialogueNodeEntered`, `DialogueChoiceSelected` and `DialogueEnded` events. The engine renders nothing —
  your game draws the conversation from the runner.
- From Lua the same loading exists as `engine.load_dialogue(key, path)` (plus `engine.register_dialogue(key,
  def_table)` and `engine.entity_insert_dialogue(entity_id, key)`), all validated through the same
  `load_dialogue` / `DialogueTree::try_from` path.

## Limitations

- No undo/redo, and no preview/play mode — the only "preview" is the edge shown while dragging a link.
- Node positions are not saved: every open re-runs the auto-layout (View > Re-layout re-runs it on demand).
- Nodes have a fixed size (240 px wide, fixed header and text area, at most 4 wrapped text lines); longer
  text is truncated with `...`.
- Closing the OS window quits immediately even with unsaved changes — the engine polls the window's close
  flag itself and offers no close-request hook, so the dirty-check only runs on File > Quit / Ctrl+Q.
- One native file dialog at a time; while one is in flight, Open/Save As requests are ignored and tab closes
  are suspended (a pending Save As result is applied by tab index).
- Edges cannot be clicked (no edge hit-testing): a link is removed by dragging its pin off onto empty space,
  or by choosing "(end)" in the Inspector's target combo. Dropping a dragged link on empty space never
  creates a node.

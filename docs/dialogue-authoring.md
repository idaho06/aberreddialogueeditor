# Authoring dialogues for AberredEngine games

This guide is for people writing conversations: how the dialogue model works, how to build one in the editor, the
exact JSON it saves, and how a Rust or Lua game loads and plays it. Engine facts here are checked against
`aberredengine` at the commit pinned in `Cargo.toml`.

## The model in one page

A dialogue is a graph of **nodes**; loops and joins are allowed. Each node shows one line and waits for the
player:

| Field | Meaning |
|---|---|
| `id` | Unique name, used by links and by `start`. |
| `speaker` | Optional; leave it out for narration. |
| `text` | The line. |
| `labels` | Optional free-form strings. The engine never interprets them; your game can (for example `"hide_when_locked"`, `"rude"`, `"lore"`). |
| `on_enter` | Optional effects applied when the node is entered. |
| `next` | Jump to another node when the player advances. |
| `choices` | Player choices, instead of `next`. A node has `next`, or `choices`, or neither — never both. |

A node with neither `next` nor `choices` is **terminal**: advancing it ends the conversation.

Each **choice** has `text`, an optional `next` (absent means the choice ends the conversation), an optional `when`
**condition**, optional `effects` applied when it is picked, and optional `labels`.

Conditions and effects read and write the game's **world signals**: named flags, integers, scalars and strings
that live in the engine's `WorldSignals`. That is how a conversation sees what the game knows (does the player
have the key?) and tells the game what happened (the guard is offended).

The conversation is data only. The engine runs it (`DialogueRunner`) and fires events; **your game draws it**.

## Placeholders

Node `text`, node `speaker` and choice `text` are **placeholder templates**. The editor shows and saves them raw;
your game fills them in when it draws the line (`runner.text()` and `runner.speaker()` return the template as
written).

| Syntax | Reads | Meaning |
|---|---|---|
| `{key}` | World signals | The signal's value as text; a set flag prints `true`. |
| `{self.key}` | The entity's `Signals` | The signals the game passes in for the running entity. |
| `{key:.N}` | World signals | A number printed with `N` digits after the point (`N` is `0`..=`6`); `{self.key:.N}` works too. |
| `{{` / `}}` | — | A literal `{` or `}`. |

Only a suffix after the LAST `:` that starts with `.` is a spec, so `{group_count:enemies}` is one key. The
malformed shapes — an unclosed `{`, a stray `}`, an empty key (`{}`, `{self.}`), a bad spec (`{k:.7}`, `{k:.}`,
`{k:.x}`) — are a lint error ("Bad placeholder" below): the Problems window shows them and saving is refused with
"Cannot save".

Your game fills a template in itself. In Rust:

```rust
let line = format_signal_text(template, &world_signals, Some(&entity_signals));
```

In Lua, from a dialogue callback:

```lua
local line = engine.format_text(dialogue.text, ctx.signals)
```

An unknown key keeps its literal token (`{key}` prints as `{key}`), and the engine logs one warning per missing key
when a node is entered — so set the signal before the node is entered: in a choice's `effects`, in a node's
`on_enter`, or before requesting the transition.

## Writing a dialogue in the editor

### Create and connect nodes

1. `File > New` (Ctrl+N), or open an existing file (Ctrl+O). A new document starts empty.
2. Right-click the canvas and choose **Add node here** (or `Edit > Add node`). A new node gets a generated id and
   a `next` of `(end)`. The first node you add becomes the start node and has a green header; change it with
   **Set as start** in the node's context menu or in the Inspector.
3. Select a node and fill it in with the Inspector: id, speaker (tick *Has speaker*), text, labels and on-enter
   effects.
4. Choose how the node continues in the Inspector's outputs section:
   - **Next** gives the node one unconditional `next` link.
   - **Choices** gives it a list of choices; use **[+ Add choice]** to grow the list. Each choice has its own
     text, target, labels, `when` condition and effects. **Up**, **Down** and **Delete** reorder and remove it.
   Switching Next to Choices turns the existing link into the first choice's target; switching back keeps the
   first choice's target.
5. Link nodes by dragging from a node's right-side pin onto another node (the target comes from the node you drop
   on), or pick the target in the Inspector's combo. The entry **(end)** means "no target": a choice with no target
   ends the conversation, a `next` of `(end)` makes the node terminal.
6. Save with Ctrl+S. Positions are not stored in the file; the layout is recomputed each time you open it.

### Conditions and effects

Both are edited with recursive editors in the Inspector. Pick the type from the combo; the fields change with it.

**Conditions** (a choice's `when`; a choice whose condition is false is *locked*):

| Type | JSON | Meaning |
|---|---|---|
| `flag` | `{"type":"flag","key":"has_pass"}` | True while the flag is set. |
| `integer` | `{"type":"integer","key":"gold","op":"ge","value":10}` | Compares an integer signal. `op` is `eq`, `ne`, `lt`, `le`, `gt` or `ge`. A missing integer reads as `0`. |
| `string_equals` | `{"type":"string_equals","key":"mood","value":"angry"}` | True when the string signal equals `value`. A missing string never equals. |
| `not` | `{"type":"not","cond":{...}}` | Negates the inner condition. |
| `all` | `{"type":"all","all":[{...},{...}]}` | True when every sub-condition is true. An empty `all` is true. |
| `any` | `{"type":"any","any":[{...},{...}]}` | True when at least one is true. An empty `any` is false. |

**Effects** (`on_enter` on a node, `effects` on a choice):

| Type | JSON | Meaning |
|---|---|---|
| `set_flag` | `{"type":"set_flag","key":"pass_shown"}` | Sets a flag. |
| `remove_flag` | `{"type":"remove_flag","key":"pass_shown"}` | Clears a flag; harmless when unset. |
| `set_integer` | `{"type":"set_integer","key":"gold","value":5}` | Sets an integer signal. |
| `add_integer` | `{"type":"add_integer","key":"visits","delta":1}` | Adds `delta` (may be negative); a missing key counts as `0`. |
| `set_scalar` | `{"type":"set_scalar","key":"anger","value":0.5}` | Sets a float signal. |
| `set_string` | `{"type":"set_string","key":"mood","value":"angry"}` | Sets a string signal. |

Keys are free text. Nothing checks that a key is used anywhere else, so keep the names consistent with what your
game reads and writes. An empty key is allowed and shown with a faint "(empty key)" hint.

### Problems, errors and the save gate

The Problems window (Ctrl+Shift+P) lists everything wrong with the active document at once; click a row to select
and centre the node.

| Problem | Severity | Meaning |
|---|---|---|
| Start missing | error | No start node is set. |
| Empty id | error | A node has an empty `id`. |
| Duplicate id | error | Two nodes share an `id`. |
| Bad placeholder | error | A text, speaker or choice text has a malformed {...} placeholder. |
| Dangling target | error | A `next` or choice points at an id that does not exist (only possible from a loaded file). |
| Unreachable | warning | No path from the start node reaches the node. It still loads and runs. |

Errors block saving. The engine's own validation (`DialogueTree::try_from`) is the gate, so a saved file always
loads in a game; if the engine would reject the document, a "Cannot save" dialog shows its message and nothing is
written. A file that already has such problems still opens, with load warnings, so you can repair it.

### Patterns that work well

- **Always leave a way out.** A choices node where no choice is available ends the conversation with the reason
  `no_choices`. Give every choices node at least one choice with no `when`, for example "Never mind." with no
  target.
- **Hub nodes.** Point several choices back at the same node (`"next": "greet"`) to let the player ask in any
  order. Add a `when` flag or integer to retire a branch once it was used.
- **Locked choices.** A false `when` does not remove the choice: the game still receives it with
  `available = false` and decides whether to grey it out or hide it. Use a label such as `hide_when_locked` to
  tell the game which locked choices to hide.
- **Availability is computed once per node.** Conditions are evaluated when the node is entered; changing a
  signal while the node is on screen does not flip a choice.
- **State carries across conversations.** Signals live in the game's world, not in the dialogue, so a flag set in
  one conversation is visible in the next. Reset them yourself when a scene restarts.

### A complete example

```json
{
  "start": "greet",
  "nodes": [
    {
      "id": "greet",
      "speaker": "Guard",
      "text": "Halt! Who goes there?",
      "on_enter": [{ "type": "add_integer", "key": "visits", "delta": 1 }],
      "choices": [
        { "text": "A friend.", "next": "friend" },
        {
          "text": "Here is my pass.",
          "next": "pass",
          "when": { "type": "flag", "key": "has_pass" },
          "effects": [{ "type": "set_flag", "key": "pass_shown" }],
          "labels": ["hide_when_locked"]
        },
        { "text": "Never mind." }
      ]
    },
    { "id": "friend", "speaker": "Guard", "text": "Hm. Move along.", "next": "greet" },
    { "id": "pass", "speaker": "Guard", "text": "Carry on." }
  ]
}
```

`friend` loops back to `greet`; `pass` is terminal; "Never mind." has no target, so it ends the conversation.
A close relative of this file is `tests/fixtures/guard.json` in the editor's repository.

## Using the file in a Rust game

All names come from `aberredengine::prelude`.

### Load it

`load_dialogue(path)` reads the file, parses it and validates it with `DialogueTree::try_from`, returning
`Result<DialogueTree, DialogueError>` (`DialogueError::Io` and `DialogueError::Json` cover read and parse
failures). Store the tree in the pre-inserted `DialogueStore` from your setup hook:

```rust
use aberredengine::prelude::*;
use std::sync::Arc;

// Registered with `EngineBuilder::on_setup(setup)`.
fn setup(mut dialogues: ResMut<DialogueStore>) {
    match load_dialogue("assets/dialogue/guard.json") {
        Ok(tree) => {
            dialogues.insert("guard", Arc::new(tree));
        }
        Err(err) => log::error!("cannot load guard dialogue: {err}"),
    }
}
```

### Start a conversation

A `DialogueRunner` component runs one conversation on one entity. Several entities can run the same tree
independently, because the tree is shared through an `Arc`.

```rust
fn start_guard_talk(dialogues: Res<DialogueStore>, mut commands: Commands) {
    if let Some(tree) = dialogues.get("guard") {
        commands.spawn(DialogueRunner::new(Arc::clone(tree)));
    }
}
```

The runner enters the start node on its first update. Add `Persistent` to keep it across a scene switch;
otherwise it despawns with its scene.

### Drive it and draw it

Request the next step from a system; read the current node to draw it:

```rust
// Registered with `EngineBuilder::add_system`.
fn drive_dialogue(mut runners: Query<&mut DialogueRunner>, input: Res<InputState>) {
    for mut runner in &mut runners {
        if !runner.is_active() || !input.action(InputAction::Action1).just_pressed {
            continue;
        }
        if runner.has_choices() {
            // `index` is the declared position, which stays stable even when earlier choices are locked.
            let first_open = runner.choices().find(|c| c.available).map(|c| c.index);
            if let Some(index) = first_open {
                runner.choose(index);
            }
        } else {
            runner.advance();
        }
    }
}

fn dialogue_lines(runner: &DialogueRunner) -> Vec<String> {
    let mut lines = Vec::new();
    if let (Some(speaker), Some(text)) = (runner.speaker(), runner.text()) {
        lines.push(format!("{speaker}: {text}"));
    }
    for choice in runner.choices() {
        let lock = if choice.available { "" } else { " (locked)" };
        lines.push(format!("{}. {}{lock}", choice.index + 1, choice.text));
    }
    lines
}
```

- `advance()` moves past a node with a `next` (or ends a terminal node); `choose(index)` picks a choice on a
  choices node; `cancel()` ends the conversation. Each only sets a pending request; the last one set before the
  engine runs wins, and it is applied on the next tick, one transition per runner per tick.
- `advance()` on a choices node and `choose()` on a node without choices are ignored. Choosing a locked or
  out-of-range index logs a warning and changes nothing.
- Read the state with `is_active()`, `is_finished()`, `node_id()`, `speaker()`, `text()`, `labels()`,
  `has_choices()` and `choices()`. Each `ChoiceView` has `index`, `text`, `labels` and `available`.

### React to events

The engine triggers three events on the runner's entity:

```rust
fn on_dialogue_choice(ev: On<DialogueChoiceSelected>, mut signals: ResMut<WorldSignals>) {
    if ev.labels.iter().any(|label| &**label == "rude") {
        signals.set_flag("guard_offended");
    }
}

fn on_dialogue_ended(ev: On<DialogueEnded>, mut commands: Commands) {
    if ev.reason != DialogueEndReason::Cancelled {
        commands.entity(ev.entity).despawn();
    }
}
```

| Event | Fields | When |
|---|---|---|
| `DialogueNodeEntered` | `entity`, `node`, `previous`, `labels` | A node was entered. `previous` is `None` for the start node, so this doubles as "dialogue started". |
| `DialogueChoiceSelected` | `entity`, `node`, `index`, `text`, `labels` | A choice was picked. It fires before the target's `DialogueNodeEntered`. |
| `DialogueEnded` | `entity`, `node`, `reason` | The conversation ended. `reason` is `Completed`, `Cancelled` or `NoChoices`; `node` is the last node shown (`None` when cancelled before the start). |

Register observers once with `EngineBuilder::add_observer`, or attach them to a single runner with `.observe(..)`.

## Using the file in a Lua game

The Lua API mirrors the Rust one: it registers trees, attaches runners to entities, requests steps, and calls your
functions on the dialogue events. The engine renders nothing, so your callbacks update whatever UI you use.

### Load it

Call `engine.load_dialogue(key, path)` while your game loads (for example from your setup script). It reads and
validates the JSON with the same loader as Rust and raises an error naming the key and path when the file is
missing or invalid. You can also define a tree inline as a Lua table in the same shape as the JSON, validated
the same way:

```lua
engine.load_dialogue("guard", "./assets/dialogue/guard.json")

engine.register_dialogue("innkeeper", {
    start = "welcome",
    nodes = {
        { id = "welcome", speaker = "Innkeeper", text = "A room for the night?",
          choices = {
              { text = "Yes.", next = "paid", effects = { { type = "add_integer", key = "coins", delta = -2 } } },
              { text = "No." },
          } },
        { id = "paid", speaker = "Innkeeper", text = "Sleep well." },
    },
})
```

### Start a conversation and listen

Spawn an entity with a dialogue and up to three callbacks, each named by a global Lua function:

```lua
engine.spawn()
    :with_dialogue("guard")
    :with_lua_dialogue_node_entered("on_dialogue_node")
    :with_lua_dialogue_choice_selected("on_dialogue_choice")
    :with_lua_dialogue_ended("on_dialogue_ended")
    :register_as("dialogue_npc")
    :build()
```

An unknown key spawns the entity without a runner. To start or restart a conversation on an existing entity,
use `engine.entity_insert_dialogue(entity_id, key)`, which replaces an existing runner; remove one with
`engine.entity_remove_dialogue(entity_id)`.

Each callback is `function(ctx, dialogue)` where `ctx` is the entity context and `dialogue` is a fresh table:

| Callback | `dialogue` fields |
|---|---|
| node entered | `node`, `previous` (nil on the start node), `labels` (1-based list), `speaker` (nil when none), `text`, `choices` |
| choice selected | `node`, `index` (0-based), `text`, `labels` |
| ended | `node` (nil when cancelled before the start), `reason`: `"completed"`, `"cancelled"` or `"no_choices"` |

`dialogue.choices` in the node-entered callback has one entry per declared choice, each with a 0-based `index`,
`text`, `labels` and `available` (false when locked). It is empty on a node without choices.

```lua
function on_dialogue_node(ctx, dialogue)
    engine.set_string("dlg_speaker", dialogue.speaker or "")
    engine.set_string("dlg_text", dialogue.text or "")
    -- Build your menu from dialogue.choices; keep each choice's declared `index`.
end

function on_dialogue_ended(ctx, dialogue)
    engine.log_info("dialogue ended: " .. dialogue.reason)
end
```

### Drive it

```lua
local npc = engine.get_entity("dialogue_npc")
engine.entity_dialogue_advance(npc)          -- past a node with `next`, or a terminal node
engine.entity_dialogue_choose(npc, index)    -- 0-based declared index on a choices node
engine.entity_dialogue_cancel(npc)           -- end the conversation now
```

These are requests, like their Rust counterparts: they do nothing without a runner, and the engine applies them on
the next tick. Pass the **declared** index (the `index` in the callback's `choices`) to `entity_dialogue_choose`,
not the position in a menu you filtered, or hidden locked choices will shift the target.

The engine repository ships a working example, `assets/scripts/scenes/dialogue_demo.lua`, with a menu-driven
conversation that hides locked choices labelled `hide_when_locked`.

## Troubleshooting

- **"Cannot save: the engine would reject this dialogue"** — fix the first problem the message names; the Problems
  window shows the rest.
- **A node is missing from the file I loaded** — it is not: an unreachable node is only a warning. Use View >
  Frame all and the Nodes window to find it.
- **A choice never appears in my game** — it is probably locked. Check the choice's `when` and the signals your
  game sets, and whether your UI hides unavailable choices.
- **The conversation ended unexpectedly with `no_choices`** — every choice on that node was locked. Add an
  unconditional fallback choice.
- **Nodes overlap after opening a file** — positions are not stored; use View > Re-layout.

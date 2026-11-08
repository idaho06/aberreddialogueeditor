# Changelog

## Unreleased

- `aberredengine` bumped to the placeholders commit.
- Malformed placeholders in a node's `text` or `speaker` or a choice's `text` are reported in the Problems window
  and no longer save.
- The dialogue authoring guide documents the placeholder syntax.

## 0.2.0

First public release.

- Node-graph editor for AberredEngine dialogue JSON: canvas with draggable nodes, pins and bezier links, pan,
  zoom, frame-all and automatic layout.
- Inspector for every field of a node and its choices, with recursive editors for conditions and effects.
- Live validation in a Problems window; saving is gated by the engine's own validation, so saved files always
  load in a game.
- Multiple documents in tabs, native open and save dialogs, atomic saves and unsaved-changes prompts.
- Intro splash scene and a Help > About dialog.
- Documentation: architecture, contributing guide and a dialogue authoring guide for Rust and Lua games.
- Built on `aberredengine` at a pinned git revision.

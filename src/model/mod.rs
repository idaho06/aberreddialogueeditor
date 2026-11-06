//! Pure editor data model: a [`Workspace`] of open [`DialogueDoc`]s plus conversion to/from the engine's
//! `DialogueDef`. No ECS, no imgui, no window — everything here is plain data and unit-testable.
//!
//! Engine imports are limited to `aberredengine::prelude::{DialogueDef, NodeDef, ChoiceDef,
//! DialogueCondition, DialogueCmp, DialogueEffect, Vec2}`.
#![allow(dead_code, unused_imports)] // consumed by later plan steps

mod convert;
mod doc;

pub use convert::{from_def, to_def};
pub use doc::{CamState, DialogueDoc, EdChoice, EdNode, Link, NodeKey, Outputs, Slot, Workspace};

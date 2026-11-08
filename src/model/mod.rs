//! Pure editor data model: a [`Workspace`] of open [`DialogueDoc`]s plus conversion to/from the engine's
//! `DialogueDef`. No ECS, no imgui, no window — everything here is plain data and unit-testable.
//!
//! Engine imports are limited to `aberredengine::prelude::{DialogueDef, NodeDef, ChoiceDef,
//! DialogueCondition, DialogueCmp, DialogueEffect, DialogueTree, DialogueError, DialogueTextField, Vec2,
//! validate_signal_text}`.

mod convert;
mod doc;
mod lint;
mod notify;

pub use convert::{from_def, to_def};
pub use doc::{
    CamState, ContextMenuRequest, DialogueDoc, EdChoice, EdNode, Link, NodeKey, Outputs, Slot,
    Workspace,
};
#[cfg(test)] // the lint function itself is only re-exported for the problems-panel tests
pub use lint::lint;
pub use lint::{Problem, ProblemKind, Severity, engine_gate};
pub use notify::{Notification, NotificationKind};

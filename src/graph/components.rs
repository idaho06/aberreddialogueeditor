//! ECS markers and projection bookkeeping for the graph view.
//!
//! This is the ONLY file of the graph module that imports ECS types: `Component`, `Resource`
//! and `Entity` come from `aberredengine::prelude` (the `bevy_ecs` re-export). Everything
//! else in `src/graph` stays pure.

// The `Component`/`Resource` derive macros expand to `bevy_ecs::...` paths; this crate does not
// depend on `bevy_ecs` directly, so the facade's re-export brings the name into scope.
use aberredengine::bevy_ecs;
use aberredengine::prelude::{Component, Entity, Resource, Vec2};
use rustc_hash::FxHashMap;

use crate::graph::projection_plan::{NodeSig, Shown};
use crate::model::{NodeKey, Slot};

/// Marker on a node's root entity (the entity carrying its `WorldPosition`).
#[derive(Component, Debug, Clone, Copy)]
pub struct GraphNode {
    pub key: NodeKey,
}

/// Marker on an edge entity (the bezier line; its arrowhead is a `GraphPart` child).
#[derive(Component, Debug, Clone, Copy)]
pub struct GraphEdge {
    pub from: NodeKey,
    pub slot: Slot,
}

/// Marker on every non-root visual child of a node or edge (body, header, texts, pins,
/// arrowheads), so they can be despawned with their parent.
#[derive(Component, Debug, Clone, Copy)]
pub struct GraphPart;

/// Marker on the drag-preview link entity shown while a link is being dragged.
#[derive(Component, Debug, Clone, Copy)]
pub struct GraphPreview;

/// One spawned node: its root entity plus the signature/position it was spawned with.
#[derive(Debug, Clone)]
pub struct NodeEntry {
    pub root: Entity,
    pub sig: NodeSig,
    pub pos: Vec2,
}

/// One spawned edge: the bezier entity, its arrowhead child and the geometry it was drawn with.
#[derive(Debug, Clone)]
pub struct EdgeEntry {
    pub entity: Entity,
    pub arrow: Entity,
    pub from: Vec2,
    pub to: Vec2,
    pub selected_source: bool,
    pub target: NodeKey,
}

/// What the projection system currently has spawned — its memory between ticks, updated after
/// each executed projection plan.
#[derive(Resource, Debug, Default)]
pub struct GraphEntities {
    pub shown_doc: Option<u64>,
    pub shown_revision: u64,
    pub metrics_ready: bool,
    /// What the last executed plan drew — the `prev` side of the next `plan_projection` diff.
    pub shown: Shown,
    pub nodes: FxHashMap<NodeKey, NodeEntry>,
    pub edges: FxHashMap<(NodeKey, Slot), EdgeEntry>,
}

//! ECS markers and projection bookkeeping for the graph view.
//!
//! This is the ONLY file of the graph module that imports ECS types: `Component`, `Resource`
//! and `Entity` come from `aberredengine::prelude` (the `bevy_ecs` re-export). Everything
//! else in `src/graph` stays pure.

// The `Component`/`Resource` derive macros expand to `bevy_ecs::...` paths; this crate does not
// depend on `bevy_ecs` directly, so the facade's re-export brings the name into scope.
use aberredengine::bevy_ecs;
use aberredengine::prelude::{Component, Entity, Resource};
use rustc_hash::FxHashMap;

use crate::graph::projection_plan::Snapshot;
use crate::model::{NodeKey, Slot};

/// Marker on a node's root entity (the entity carrying its `WorldPosition`). The node's key is
/// the key of its entry in [`GraphEntities::nodes`].
#[derive(Component, Debug, Clone, Copy)]
pub struct GraphNode;

/// Marker on an edge entity (the bezier line; its arrowhead is a `GraphPart` child). The edge's
/// `(from, slot)` is the key of its entry in [`GraphEntities::edges`].
#[derive(Component, Debug, Clone, Copy)]
pub struct GraphEdge;

/// Marker on every non-root visual child of a node or edge (body, header, texts, pins,
/// arrowheads), so they can be despawned with their parent.
#[derive(Component, Debug, Clone, Copy)]
pub struct GraphPart;

/// Marker on the drag-preview link entity shown while a link is being dragged.
#[derive(Component, Debug, Clone, Copy)]
pub struct GraphPreview;

/// One spawned edge: the bezier entity and its arrowhead child.
#[derive(Debug, Clone, Copy)]
pub struct EdgeEntry {
    pub entity: Entity,
    pub arrow: Entity,
}

/// What the projection system currently has spawned — its memory between ticks, updated after
/// each executed projection plan.
#[derive(Resource, Debug, Default)]
pub struct GraphEntities {
    /// The last executed plan's snapshot — the `prev` side of the next `plan_projection` diff;
    /// its `(doc_id, revision, metrics_ready)` triple is the cheap-path key.
    pub shown: Snapshot,
    /// Node key -> the node's root entity (the one carrying its `WorldPosition`).
    pub nodes: FxHashMap<NodeKey, Entity>,
    /// Edge key `(from_key, slot)` -> the bezier entity and its arrowhead child.
    pub edges: FxHashMap<(NodeKey, Slot), EdgeEntry>,
}

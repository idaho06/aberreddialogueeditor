//! ECS preview system: while a link is being dragged, `graph_input.preview` holds
//! `(from_pin_world, mouse_world)` and this system draws it as ONE bezier entity (`GraphPreview`)
//! — spawned on demand, rewritten in place each tick, despawned when the drag ends. If the
//! remembered entity no longer exists (query get fails, e.g. after a scene switch) a new one is
//! spawned. Plain spawn/rewrite/despawn: no workspace revision involved.

use aberredengine::prelude::*;

use super::projection::{curve_shape, set_pos_if_changed, set_shape_if_changed};
use super::{EDGE_SELECTED_COLOR, GraphInput, GraphPreview, Z_PREVIEW};

/// Keeps the drag-preview entity in sync with [`GraphInput::preview`]. Runs late in the sim tick
/// (`add_late_system`) so it sees this tick's input.
pub fn graph_preview_system(
    mut commands: Commands,
    graph_input: Res<GraphInput>,
    mut shapes: Query<&mut Shape>,
    mut positions: Query<&mut WorldPosition>,
    mut entity: Local<Option<Entity>>,
) {
    let Some((from, to)) = graph_input.preview else {
        // The drag ended: despawn the preview entity if it still exists.
        if let Some(e) = entity.take()
            && positions.get(e).is_ok()
        {
            commands.entity(e).despawn();
        }
        return;
    };
    // Reuse the remembered entity while it is alive; spawn a new one otherwise (first tick of
    // the drag, or the old entity was despawned e.g. by a scene switch).
    let alive = (*entity).filter(|&e| positions.get(e).is_ok());
    let Some(e) = alive else {
        let (curve, ..) = curve_shape(from, to, EDGE_SELECTED_COLOR);
        *entity = Some(
            commands
                .spawn((
                    GraphPreview,
                    ZIndex(Z_PREVIEW),
                    WorldPosition::from_vec(from),
                    curve,
                ))
                .id(),
        );
        return;
    };
    // Rewrite position and shape in place, assigning only when different.
    set_pos_if_changed(&mut positions, e, from);
    let (curve, ..) = curve_shape(from, to, EDGE_SELECTED_COLOR);
    set_shape_if_changed(&mut shapes, e, curve);
}

//! Pure application of interaction [`Action`]s to a [`DialogueDoc`]: the input system runs the
//! state machine ([`step`](super::step)) and hands the returned actions to [`apply_actions`],
//! which maps each one onto the doc's named mutators (unknown nodes are no-ops there) and turns
//! [`Action::OpenContextMenu`] into a [`ContextMenuRequest`] for the GUI. No ECS, no imgui —
//! only `Vec2` from the engine prelude plus the model and the other pure graph modules.
//!
//! Logging is event-only: a `SetLink` that actually changes the link logs `link set` / `link
//! cleared` at debug; the per-tick `MoveNode`/pan/select actions never log.

use super::camera::pan_by_screen_delta;
use super::geometry::Hit;
use super::interaction::{Action, link_at};
use crate::model::{ContextMenuRequest, DialogueDoc, Link};

/// Applies `actions` to `doc` in order and returns the context-menu request, if any (when several
/// `OpenContextMenu` actions arrive in one tick, the last one wins). `PanByScreenDelta` pans the
/// doc camera by the screen delta (no viewport needed); `MoveNode`/`Select`/`SetLink` go through
/// the doc's mutators, so unknown nodes are no-ops.
pub fn apply_actions(doc: &mut DialogueDoc, actions: &[Action]) -> Option<ContextMenuRequest> {
    let mut request = None;
    for action in actions {
        match action {
            Action::Select(key) => doc.select(*key),
            Action::MoveNode(key, pos) => doc.set_node_pos(*key, *pos),
            Action::SetLink(key, slot, link) => {
                let before = link_at(doc, *key, *slot);
                doc.set_link(*key, *slot, link.clone());
                let after = link_at(doc, *key, *slot);
                if before != after {
                    if matches!(after, Some(Link::End)) {
                        log::debug!("link cleared: node={key:?} slot={slot:?}");
                    } else {
                        log::debug!("link set: node={key:?} slot={slot:?}");
                    }
                }
            }
            Action::PanByScreenDelta(delta) => {
                doc.set_camera(pan_by_screen_delta(&doc.camera, *delta));
            }
            Action::OpenContextMenu { world, hit } => {
                let node = match hit {
                    Hit::Body(key) | Hit::InputPin(key) | Hit::OutputPin(key, _) => Some(*key),
                    Hit::Empty => None,
                };
                request = Some(ContextMenuRequest {
                    world: *world,
                    node,
                });
            }
        }
    }
    request
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::fixtures::{FRIEND, GREET, PASS, guard_doc};
    use crate::graph::interaction::Action;
    use crate::model::{CamState, NodeKey, Outputs, Slot};
    use aberredengine::prelude::Vec2;

    #[test]
    fn move_select_and_pan_apply_to_the_doc_without_setting_dirty() {
        let mut doc = guard_doc();
        doc.mark_clean();
        let revision = doc.revision;
        let friend_pos = doc.node(FRIEND).expect("node exists").pos;
        let target = friend_pos + Vec2::new(30.0, -12.0);
        let cam = doc.camera;
        let delta = Vec2::new(40.0, -20.0);
        let request = apply_actions(
            &mut doc,
            &[
                Action::MoveNode(FRIEND, target),
                Action::Select(Some(PASS)),
                Action::PanByScreenDelta(delta),
            ],
        );
        assert_eq!(request, None, "no context-menu action, no request");
        assert_eq!(doc.node(FRIEND).expect("node exists").pos, target);
        assert_eq!(doc.selected, Some(PASS));
        assert_eq!(
            doc.camera,
            pan_by_screen_delta(&cam, delta),
            "pan applies to the doc camera"
        );
        assert!(!doc.dirty, "MoveNode/Select/Pan are view-only: never dirty");
        assert_eq!(
            doc.revision,
            revision + 2,
            "MoveNode and Select bump the revision; the pan does not"
        );
    }

    #[test]
    fn select_none_clears_the_selection() {
        let mut doc = guard_doc();
        doc.select(Some(FRIEND));
        apply_actions(&mut doc, &[Action::Select(None)]);
        assert_eq!(doc.selected, None);
    }

    #[test]
    fn set_link_applies_and_sets_dirty() {
        let mut doc = guard_doc();
        doc.mark_clean();
        let revision = doc.revision;
        let request = apply_actions(
            &mut doc,
            &[Action::SetLink(GREET, Slot::Choice(0), Link::Node(PASS))],
        );
        assert_eq!(request, None);
        match &doc.node(GREET).expect("node exists").out {
            Outputs::Choices(choices) => {
                assert_eq!(choices[0].target, Link::Node(PASS));
            }
            Outputs::Next(_) => panic!("expected choices"),
        }
        assert!(doc.dirty, "SetLink is a content change");
        assert_eq!(doc.revision, revision + 1);

        // Clearing the link again is also a SetLink.
        let revision = doc.revision;
        apply_actions(
            &mut doc,
            &[Action::SetLink(GREET, Slot::Choice(0), Link::End)],
        );
        match &doc.node(GREET).expect("node exists").out {
            Outputs::Choices(choices) => assert_eq!(choices[0].target, Link::End),
            Outputs::Next(_) => panic!("expected choices"),
        }
        assert_eq!(doc.revision, revision + 1);
    }

    #[test]
    fn set_link_on_a_missing_node_is_a_no_op() {
        let mut doc = guard_doc();
        doc.mark_clean();
        let revision = doc.revision;
        let ghost = NodeKey(999);
        let request = apply_actions(
            &mut doc,
            &[Action::SetLink(ghost, Slot::Next, Link::Node(FRIEND))],
        );
        assert_eq!(request, None);
        assert_eq!(
            doc.revision, revision,
            "a missing node must not bump the revision"
        );
        assert!(!doc.dirty, "a no-op must not set dirty");
    }

    #[test]
    fn move_node_on_a_missing_node_is_a_no_op() {
        let mut doc = guard_doc();
        doc.mark_clean();
        let revision = doc.revision;
        apply_actions(
            &mut doc,
            &[Action::MoveNode(NodeKey(999), Vec2::new(1.0, 1.0))],
        );
        assert_eq!(doc.revision, revision);
        assert!(!doc.dirty);
    }

    #[test]
    fn open_context_menu_returns_the_request_with_the_hit_node() {
        let mut doc = guard_doc();
        let world = Vec2::new(12.0, -34.0);

        // A body hit carries the node.
        let request = apply_actions(
            &mut doc,
            &[Action::OpenContextMenu {
                world,
                hit: Hit::Body(FRIEND),
            }],
        );
        assert_eq!(
            request,
            Some(ContextMenuRequest {
                world,
                node: Some(FRIEND)
            })
        );

        // Pin hits carry their node too.
        for (hit, node) in [
            (Hit::InputPin(PASS), PASS),
            (Hit::OutputPin(GREET, Slot::Choice(1)), GREET),
        ] {
            let request = apply_actions(&mut doc, &[Action::OpenContextMenu { world, hit }]);
            assert_eq!(
                request,
                Some(ContextMenuRequest {
                    world,
                    node: Some(node)
                })
            );
        }

        // Empty space: no node.
        let request = apply_actions(
            &mut doc,
            &[Action::OpenContextMenu {
                world,
                hit: Hit::Empty,
            }],
        );
        assert_eq!(request, Some(ContextMenuRequest { world, node: None }));

        // Several menu actions in one tick: the last one wins.
        let other = Vec2::new(-1.0, -2.0);
        let request = apply_actions(
            &mut doc,
            &[
                Action::OpenContextMenu {
                    world,
                    hit: Hit::Body(FRIEND),
                },
                Action::OpenContextMenu {
                    world: other,
                    hit: Hit::Empty,
                },
            ],
        );
        assert_eq!(
            request,
            Some(ContextMenuRequest {
                world: other,
                node: None
            }),
            "the last OpenContextMenu wins"
        );
    }

    #[test]
    fn pan_uses_the_doc_camera_and_needs_no_viewport() {
        let mut doc = guard_doc();
        doc.set_camera(CamState {
            target: Vec2::new(100.0, 50.0),
            zoom: 2.0,
        });
        let before = doc.camera;
        apply_actions(&mut doc, &[Action::PanByScreenDelta(Vec2::new(10.0, -6.0))]);
        assert_eq!(
            doc.camera.target,
            before.target - Vec2::new(10.0, -6.0) / before.zoom
        );
        assert!((doc.camera.zoom - before.zoom).abs() < 1e-6);
    }
}

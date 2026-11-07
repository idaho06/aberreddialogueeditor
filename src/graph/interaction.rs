//! Pure mouse-gesture state machine for the graph view. No ECS, no imgui — only `Vec2` from the
//! engine prelude. [`step`] consumes one per-tick [`InputSample`] (button edges plus the mouse in
//! screen and world coords) and returns the [`Action`]s for the caller to apply to the doc/camera;
//! [`drag_preview`] reports the link being dragged so the view can draw it.
//!
//! Phases are exclusive: a gesture starts only from [`Phase::Idle`] and ends on its own button's
//! release, so the other button's press mid-gesture is ignored. `mouse_captured_by_ui` gates
//! gesture STARTS only — a running gesture is never cancelled by the mouse moving over UI; it ends
//! on release. If the doc changes under a gesture and a referenced node is gone, the phase falls
//! back to `Idle`.

use aberredengine::prelude::Vec2;

use crate::graph::geometry::{Hit, hit_test, output_pin};
use crate::graph::style::PIN_R;
use crate::model::{DialogueDoc, Link, NodeKey, Outputs, Slot};

/// Screen-pixel distance a press may travel before it counts as a drag instead of a click:
/// a right press pans past it, a left-drag off a linked pin detaches past it.
const CLICK_THRESHOLD_PX: f32 = 4.0;

/// One per-tick input sample. `*_pressed`/`*_released` are one-tick edges; `mouse_world` is already
/// computed with the CURRENT camera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputSample {
    pub mouse_screen: Vec2,
    pub mouse_world: Vec2,
    pub left_down: bool,
    pub left_pressed: bool,
    pub left_released: bool,
    pub right_down: bool,
    pub right_pressed: bool,
    pub right_released: bool,
    pub mouse_captured_by_ui: bool,
    pub zoom: f32,
}

/// The gesture currently in progress (exactly one at a time).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Phase {
    #[default]
    Idle,
    /// Left-dragging a node body/input pin; `grab_offset` keeps the grab point fixed under the cursor.
    DraggingNode { key: NodeKey, grab_offset: Vec2 },
    /// Left-dragging a link out of an output pin. A pin that already has a link is NOT detached
    /// at pickup — the detach (`SetLink .. End`) is deferred until the pointer actually leaves
    /// the 4 px click radius, so a plain click mutates nothing (`set_link` marks the doc dirty
    /// even when the value is unchanged).
    DraggingLink {
        from: NodeKey,
        slot: Slot,
        from_pin_world: Vec2,
        /// The node the pin pointed to at pickup if the link was `Link::Node(r)`; releasing over
        /// the SAME pin after a real drag restores it. `End`/`Dangling` have nothing to restore.
        restore: Option<NodeKey>,
        /// Where the press happened, in screen coords; the click radius is measured from it.
        press_screen: Vec2,
        /// Whether the pin's link at pickup was `Node`/`Dangling` (anything but `End`).
        had_link: bool,
        /// Set once the carried link has been detached with `SetLink .. End`.
        detached: bool,
    },
    /// Right button held; becomes a pan once the pointer leaves the 4 px click radius.
    RightDown {
        press_screen: Vec2,
        press_world: Vec2,
        hit: Hit,
        panning: bool,
    },
}

/// The interaction state (a resource on the logic side): just the current [`Phase`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GraphInteraction {
    pub phase: Phase,
}

/// What the caller should do with the doc/camera this tick.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Select(Option<NodeKey>),
    MoveNode(NodeKey, Vec2),
    SetLink(NodeKey, Slot, Link),
    PanByScreenDelta(Vec2),
    OpenContextMenu { world: Vec2, hit: Hit },
}

/// Pin pickup radius in world units: `PIN_R + 4` screen pixels, constant on screen at any zoom.
fn pin_hit_radius(zoom: f32) -> f32 {
    (PIN_R + 4.0) / if zoom > 0.0 { zoom } else { 1.0 }
}

/// Output row index a [`Slot`] addresses (`Slot::Next` is row 0 of a `Next` node).
fn slot_row(slot: Slot) -> usize {
    match slot {
        Slot::Next => 0,
        Slot::Choice(i) => i,
    }
}

/// The current [`Link`] of one output slot, if the node and slot both exist.
pub(crate) fn link_at(doc: &DialogueDoc, key: NodeKey, slot: Slot) -> Option<Link> {
    let node = doc.node(key)?;
    match (&node.out, slot) {
        (Outputs::Next(link), Slot::Next) => Some(link.clone()),
        (Outputs::Choices(choices), Slot::Choice(i)) => {
            choices.get(i).map(|choice| choice.target.clone())
        }
        _ => None,
    }
}

/// Advances the state machine by one tick and returns the actions to apply.
pub fn step(
    state: &mut GraphInteraction,
    doc: &DialogueDoc,
    s: &InputSample,
    prev_mouse_screen: Vec2,
) -> Vec<Action> {
    let mut actions = Vec::new();
    let pin_radius = pin_hit_radius(s.zoom);
    match state.phase {
        Phase::Idle => {
            if s.mouse_captured_by_ui {
                return actions; // presses over UI never start a gesture
            }
            if s.left_pressed {
                match hit_test(doc, s.mouse_world, pin_radius) {
                    Hit::Body(key) | Hit::InputPin(key) => {
                        if let Some(node) = doc.node(key) {
                            actions.push(Action::Select(Some(key)));
                            state.phase = Phase::DraggingNode {
                                key,
                                grab_offset: s.mouse_world - node.pos,
                            };
                        }
                    }
                    Hit::OutputPin(key, slot) => {
                        if let Some(node) = doc.node(key) {
                            // Pickup only selects and carries the link; the detach is deferred
                            // until the pointer moves (see the DraggingLink arm), so a plain
                            // click on a linked pin emits no mutation at all.
                            actions.push(Action::Select(Some(key)));
                            let link = link_at(doc, key, slot);
                            let had_link = matches!(&link, Some(l) if !matches!(l, Link::End));
                            let restore = match &link {
                                Some(Link::Node(target)) => Some(*target),
                                _ => None,
                            };
                            state.phase = Phase::DraggingLink {
                                from: key,
                                slot,
                                from_pin_world: output_pin(node, slot_row(slot)),
                                restore,
                                press_screen: s.mouse_screen,
                                had_link,
                                detached: false,
                            };
                        }
                    }
                    Hit::Empty => actions.push(Action::Select(None)),
                }
            } else if s.right_pressed {
                state.phase = Phase::RightDown {
                    press_screen: s.mouse_screen,
                    press_world: s.mouse_world,
                    hit: hit_test(doc, s.mouse_world, pin_radius),
                    panning: false,
                };
            }
        }
        Phase::DraggingNode { key, grab_offset } => {
            if doc.node(key).is_none() {
                state.phase = Phase::Idle; // the node vanished under the gesture
                return actions;
            }
            if s.left_released {
                state.phase = Phase::Idle;
            } else if s.left_down {
                actions.push(Action::MoveNode(key, s.mouse_world - grab_offset));
            }
        }
        Phase::DraggingLink {
            from,
            slot,
            from_pin_world,
            restore,
            press_screen,
            had_link,
            detached,
        } => {
            if doc.node(from).is_none() {
                state.phase = Phase::Idle; // the source node vanished under the gesture
                return actions;
            }
            // Detach a carried link only once the pointer has left the 4 px click radius, so a
            // plain click leaves the link untouched. This runs on drag ticks AND on the release
            // tick (before the release logic), so a single-tick flick still detaches first.
            let mut detached = detached;
            if had_link
                && !detached
                && (s.mouse_screen - press_screen).length() > CLICK_THRESHOLD_PX
            {
                actions.push(Action::SetLink(from, slot, Link::End));
                detached = true;
            }
            if s.left_released {
                // A release over a node's body/input pin — or over ANY other output pin — connects
                // to that node (self-links allowed). Releasing over the SAME pin the drag started
                // from: a detached drag restores the original target if there was one; a plain
                // click emits nothing and leaves the link untouched. Over empty space a detached
                // link stays End — dragging off removes it, and no node is ever created.
                match hit_test(doc, s.mouse_world, pin_radius) {
                    Hit::Body(t) | Hit::InputPin(t) => {
                        actions.push(Action::SetLink(from, slot, Link::Node(t)));
                    }
                    Hit::OutputPin(t, s2) => {
                        if (t, s2) == (from, slot) {
                            if detached && let Some(r) = restore {
                                actions.push(Action::SetLink(from, slot, Link::Node(r)));
                            }
                        } else {
                            actions.push(Action::SetLink(from, slot, Link::Node(t)));
                        }
                    }
                    Hit::Empty => {}
                }
                state.phase = Phase::Idle;
            } else {
                state.phase = Phase::DraggingLink {
                    from,
                    slot,
                    from_pin_world,
                    restore,
                    press_screen,
                    had_link,
                    detached,
                };
            }
        }
        Phase::RightDown {
            press_screen,
            press_world,
            hit,
            panning,
        } => {
            if s.right_released {
                if !panning {
                    actions.push(Action::OpenContextMenu {
                        world: press_world,
                        hit,
                    });
                    if let Hit::Body(k) | Hit::InputPin(k) | Hit::OutputPin(k, _) = hit {
                        actions.push(Action::Select(Some(k)));
                    }
                }
                state.phase = Phase::Idle;
            } else if s.right_down {
                let panning =
                    panning || (s.mouse_screen - press_screen).length() > CLICK_THRESHOLD_PX;
                if panning {
                    actions.push(Action::PanByScreenDelta(s.mouse_screen - prev_mouse_screen));
                    state.phase = Phase::RightDown {
                        press_screen,
                        press_world,
                        hit,
                        panning: true,
                    };
                }
            }
        }
    }
    actions
}

/// The link currently being dragged as `(from_pin_world, mouse_world)`, for the preview edge.
pub fn drag_preview(state: &GraphInteraction, s: &InputSample) -> Option<(Vec2, Vec2)> {
    match state.phase {
        Phase::DraggingLink { from_pin_world, .. } => Some((from_pin_world, s.mouse_world)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::fixtures::{DANGLING, FRIEND, GREET, PASS, doc_from, guard_doc};
    use crate::graph::geometry::input_pin;
    use crate::graph::layout::auto_layout;
    use crate::graph::style::{HEADER_H, NODE_W};

    /// invalid_dangling.json keys: a = 1 (next -> Dangling), b = 2 (choice 0 -> Dangling).
    const A: NodeKey = NodeKey(1);
    const B: NodeKey = NodeKey(2);

    fn dangling_doc() -> DialogueDoc {
        let mut doc = doc_from(DANGLING);
        auto_layout(&mut doc);
        doc
    }

    /// A point in the middle of a node's header: inside the body, far from every pin.
    fn body_point(doc: &DialogueDoc, key: NodeKey) -> Vec2 {
        let node = doc.node(key).expect("node exists");
        node.pos + Vec2::new(NODE_W / 2.0, HEADER_H / 2.0)
    }

    fn sample(screen: Vec2, world: Vec2) -> InputSample {
        InputSample {
            mouse_screen: screen,
            mouse_world: world,
            left_down: false,
            left_pressed: false,
            left_released: false,
            right_down: false,
            right_pressed: false,
            right_released: false,
            mouse_captured_by_ui: false,
            zoom: 1.0,
        }
    }

    /// A sample whose screen and world coords coincide (zoom 1, camera at the origin).
    fn at(p: Vec2) -> InputSample {
        sample(p, p)
    }

    fn left_press(mut s: InputSample) -> InputSample {
        s.left_pressed = true;
        s.left_down = true;
        s
    }

    fn left_down(mut s: InputSample) -> InputSample {
        s.left_down = true;
        s
    }

    fn left_release(mut s: InputSample) -> InputSample {
        s.left_released = true;
        s.left_down = false;
        s
    }

    fn right_press(mut s: InputSample) -> InputSample {
        s.right_pressed = true;
        s.right_down = true;
        s
    }

    fn right_down(mut s: InputSample) -> InputSample {
        s.right_down = true;
        s
    }

    fn right_release(mut s: InputSample) -> InputSample {
        s.right_released = true;
        s.right_down = false;
        s
    }

    fn captured(mut s: InputSample) -> InputSample {
        s.mouse_captured_by_ui = true;
        s
    }

    fn zoomed(mut s: InputSample, zoom: f32) -> InputSample {
        s.zoom = zoom;
        s
    }

    /// Feeds scripted samples through `step`, tracking `prev_mouse_screen` like the input system.
    fn run(doc: &DialogueDoc, samples: &[InputSample]) -> (GraphInteraction, Vec<Action>) {
        let mut state = GraphInteraction::default();
        let mut actions = Vec::new();
        let mut prev = samples.first().map_or(Vec2::ZERO, |s| s.mouse_screen);
        for s in samples {
            actions.extend(step(&mut state, doc, s, prev));
            prev = s.mouse_screen;
        }
        (state, actions)
    }

    #[test]
    fn scripted_gesture_sequences() {
        let guard = guard_doc();
        let dangling = dangling_doc();
        let friend_body = body_point(&guard, FRIEND);
        let friend_pos = guard.node(FRIEND).expect("node exists").pos;
        let greet_pin0 = output_pin(guard.node(GREET).expect("node exists"), 0);
        let greet_pin1 = output_pin(guard.node(GREET).expect("node exists"), 1);
        let greet_pin2 = output_pin(guard.node(GREET).expect("node exists"), 2);
        let pass_body = body_point(&guard, PASS);
        let pass_out = output_pin(guard.node(PASS).expect("node exists"), 0);
        let greet_in = input_pin(guard.node(GREET).expect("node exists"));
        let friend_in = input_pin(guard.node(FRIEND).expect("node exists"));
        let a_out = output_pin(dangling.node(A).expect("node exists"), 0);
        let empty = Vec2::new(-500.0, -500.0);
        let d1 = Vec2::new(37.0, -11.0);
        let d2 = Vec2::new(61.0, 3.0);

        /// One scripted sequence: (name, doc, samples, expected actions, expected final phase).
        type Case<'a> = (
            &'a str,
            &'a DialogueDoc,
            Vec<InputSample>,
            Vec<Action>,
            Phase,
        );
        let cases: Vec<Case> = vec![
            (
                "left press on body selects and drag moves by the exact delta",
                &guard,
                vec![
                    left_press(at(friend_body)),
                    left_down(at(friend_body + d1)),
                    left_down(at(friend_body + d2)),
                    left_release(at(friend_body + d2)),
                ],
                vec![
                    Action::Select(Some(FRIEND)),
                    Action::MoveNode(FRIEND, friend_pos + d1),
                    Action::MoveNode(FRIEND, friend_pos + d2),
                ],
                Phase::Idle,
            ),
            (
                "left press on empty clears the selection",
                &guard,
                vec![left_press(at(empty))],
                vec![Action::Select(None)],
                Phase::Idle,
            ),
            (
                "left press on the input pin drags like the body",
                &guard,
                vec![
                    left_press(at(friend_in)),
                    left_down(at(friend_in + d1)),
                    left_release(at(friend_in + d1)),
                ],
                vec![
                    Action::Select(Some(FRIEND)),
                    Action::MoveNode(FRIEND, friend_pos + d1),
                ],
                Phase::Idle,
            ),
            (
                "drag an already-linked pin onto another node detaches then connects",
                &guard,
                vec![
                    left_press(at(greet_pin0)), // Choice(0) -> friend
                    left_down(at(greet_pin0 + Vec2::new(30.0, 0.0))),
                    left_release(at(pass_body)),
                ],
                vec![
                    Action::Select(Some(GREET)),
                    Action::SetLink(GREET, Slot::Choice(0), Link::End),
                    Action::SetLink(GREET, Slot::Choice(0), Link::Node(PASS)),
                ],
                Phase::Idle,
            ),
            (
                "release over the target's output pin connects like its body",
                &guard,
                vec![left_press(at(greet_pin0)), left_release(at(pass_out))],
                vec![
                    Action::Select(Some(GREET)),
                    Action::SetLink(GREET, Slot::Choice(0), Link::End),
                    Action::SetLink(GREET, Slot::Choice(0), Link::Node(PASS)),
                ],
                Phase::Idle,
            ),
            (
                "a click without moving on a linked choice pin emits only Select",
                &guard,
                vec![
                    left_press(at(greet_pin0)), // Choice(0) -> friend
                    left_release(at(greet_pin0)),
                ],
                vec![Action::Select(Some(GREET))],
                Phase::Idle,
            ),
            (
                "a click without moving on a linked next pin emits only Select",
                &guard,
                vec![
                    left_press(at(pass_out)), // Next -> friend
                    left_release(at(pass_out)),
                ],
                vec![Action::Select(Some(PASS))],
                Phase::Idle,
            ),
            (
                "a click without moving on an unlinked (End) pin emits only Select",
                &guard,
                vec![
                    left_press(at(greet_pin2)), // Choice(2) -> End
                    left_release(at(greet_pin2)),
                ],
                vec![Action::Select(Some(GREET))],
                Phase::Idle,
            ),
            (
                "a click without moving on a dangling pin emits only Select",
                &dangling,
                vec![
                    left_press(at(a_out)), // Next -> Dangling
                    left_release(at(a_out)),
                ],
                vec![Action::Select(Some(A))],
                Phase::Idle,
            ),
            (
                "a self-link is allowed",
                &guard,
                vec![left_press(at(greet_pin0)), left_release(at(greet_in))],
                vec![
                    Action::Select(Some(GREET)),
                    Action::SetLink(GREET, Slot::Choice(0), Link::End),
                    Action::SetLink(GREET, Slot::Choice(0), Link::Node(GREET)),
                ],
                Phase::Idle,
            ),
            (
                "a single-tick flick off a linked pin onto empty space detaches on release",
                &guard,
                vec![
                    left_press(at(greet_pin1)), // Choice(1) -> pass
                    left_release(at(empty)),
                ],
                vec![
                    Action::Select(Some(GREET)),
                    Action::SetLink(GREET, Slot::Choice(1), Link::End),
                ],
                Phase::Idle,
            ),
            (
                "press, move beyond 4 px, release over empty space detaches at the moving tick",
                &guard,
                vec![
                    left_press(at(greet_pin1)), // Choice(1) -> pass
                    left_down(at(greet_pin1 + Vec2::new(30.0, 0.0))),
                    left_release(at(empty)),
                ],
                vec![
                    Action::Select(Some(GREET)),
                    Action::SetLink(GREET, Slot::Choice(1), Link::End),
                ],
                Phase::Idle,
            ),
            (
                "drag away then release over the SAME pin restores the original target",
                &guard,
                vec![
                    left_press(at(greet_pin0)), // Choice(0) -> friend
                    left_down(at(greet_pin0 + Vec2::new(10.0, 0.0))),
                    left_release(at(greet_pin0)),
                ],
                vec![
                    Action::Select(Some(GREET)),
                    Action::SetLink(GREET, Slot::Choice(0), Link::End),
                    Action::SetLink(GREET, Slot::Choice(0), Link::Node(FRIEND)),
                ],
                Phase::Idle,
            ),
            (
                "a dangling pin dragged away and dropped on empty space is cleared",
                &dangling,
                vec![
                    left_press(at(a_out)), // Next -> Dangling
                    left_down(at(a_out + Vec2::new(10.0, 0.0))),
                    left_release(at(empty)),
                ],
                vec![
                    Action::Select(Some(A)),
                    Action::SetLink(A, Slot::Next, Link::End),
                ],
                Phase::Idle,
            ),
            (
                "a tiny drag within 4 px released over another node connects without a detach",
                &guard,
                vec![
                    left_press(at(greet_pin0)), // Choice(0) -> friend
                    // screen moved 3 px (<= 4: no detach), world lands on pass's body
                    left_release(sample(greet_pin0 + Vec2::new(3.0, 0.0), pass_body)),
                ],
                vec![
                    Action::Select(Some(GREET)),
                    Action::SetLink(GREET, Slot::Choice(0), Link::Node(PASS)),
                ],
                Phase::Idle,
            ),
            (
                "dangling next pin pickup clears it",
                &dangling,
                vec![
                    left_press(at(output_pin(dangling.node(A).expect("node exists"), 0))),
                    left_release(at(empty)),
                ],
                vec![
                    Action::Select(Some(A)),
                    Action::SetLink(A, Slot::Next, Link::End),
                ],
                Phase::Idle,
            ),
            (
                "dangling choice pin pickup clears it",
                &dangling,
                vec![
                    left_press(at(output_pin(dangling.node(B).expect("node exists"), 0))),
                    left_release(at(empty)),
                ],
                vec![
                    Action::Select(Some(B)),
                    Action::SetLink(B, Slot::Choice(0), Link::End),
                ],
                Phase::Idle,
            ),
            (
                "right click on a body opens the menu with the press hit and selects",
                &guard,
                vec![right_press(at(friend_body)), right_release(at(friend_body))],
                vec![
                    Action::OpenContextMenu {
                        world: friend_body,
                        hit: Hit::Body(FRIEND),
                    },
                    Action::Select(Some(FRIEND)),
                ],
                Phase::Idle,
            ),
            (
                "right click on empty opens the menu only",
                &guard,
                vec![right_press(at(empty)), right_release(at(empty))],
                vec![Action::OpenContextMenu {
                    world: empty,
                    hit: Hit::Empty,
                }],
                Phase::Idle,
            ),
            (
                "right click on an output pin reports that pin as the hit",
                &guard,
                vec![right_press(at(greet_pin2)), right_release(at(greet_pin2))],
                vec![
                    Action::OpenContextMenu {
                        world: greet_pin2,
                        hit: Hit::OutputPin(GREET, Slot::Choice(2)),
                    },
                    Action::Select(Some(GREET)),
                ],
                Phase::Idle,
            ),
            (
                "right drag beyond 4 px pans and opens no menu",
                &guard,
                vec![
                    right_press(at(friend_body)),
                    right_down(at(friend_body + Vec2::new(3.0, 0.0))), // <= 4 px: still a click
                    right_down(at(friend_body + Vec2::new(10.0, 0.0))), // pans, delta = (7, 0)
                    right_down(at(friend_body + Vec2::new(14.0, 2.0))), // delta = (4, 2)
                    right_release(at(friend_body + Vec2::new(14.0, 2.0))),
                ],
                vec![
                    Action::PanByScreenDelta(Vec2::new(7.0, 0.0)),
                    Action::PanByScreenDelta(Vec2::new(4.0, 2.0)),
                ],
                Phase::Idle,
            ),
            (
                "right drag within 4 px still opens the menu",
                &guard,
                vec![
                    right_press(at(friend_body)),
                    right_down(at(friend_body + Vec2::new(3.0, 0.0))),
                    right_release(at(friend_body + Vec2::new(3.0, 0.0))),
                ],
                vec![
                    Action::OpenContextMenu {
                        world: friend_body,
                        hit: Hit::Body(FRIEND),
                    },
                    Action::Select(Some(FRIEND)),
                ],
                Phase::Idle,
            ),
            (
                "left press while the mouse is captured by UI is ignored",
                &guard,
                vec![captured(left_press(at(friend_body)))],
                vec![],
                Phase::Idle,
            ),
            (
                "right press while the mouse is captured by UI is ignored",
                &guard,
                vec![captured(right_press(at(friend_body)))],
                vec![],
                Phase::Idle,
            ),
            (
                "mid-gesture capture does not cancel a node drag",
                &guard,
                vec![
                    left_press(at(friend_body)),
                    captured(left_down(at(friend_body + d1))),
                    left_release(at(friend_body + d1)),
                ],
                vec![
                    Action::Select(Some(FRIEND)),
                    Action::MoveNode(FRIEND, friend_pos + d1),
                ],
                Phase::Idle,
            ),
            (
                "mid-gesture capture does not cancel a link drag",
                &guard,
                vec![
                    left_press(at(greet_pin0)),
                    captured(left_down(at(greet_pin0 + Vec2::new(9.0, 9.0)))),
                    captured(left_release(at(pass_body))),
                ],
                vec![
                    Action::Select(Some(GREET)),
                    Action::SetLink(GREET, Slot::Choice(0), Link::End),
                    Action::SetLink(GREET, Slot::Choice(0), Link::Node(PASS)),
                ],
                Phase::Idle,
            ),
            (
                "a right press during a left drag is ignored",
                &guard,
                vec![
                    left_press(at(friend_body)),
                    right_press(at(friend_body + Vec2::new(1.0, 1.0))),
                    left_down(at(friend_body + d1)),
                    right_release(at(friend_body + d1)),
                    left_release(at(friend_body + d1)),
                ],
                vec![
                    Action::Select(Some(FRIEND)),
                    Action::MoveNode(FRIEND, friend_pos + d1),
                ],
                Phase::Idle,
            ),
            (
                "a left press during a right pan is ignored",
                &guard,
                vec![
                    right_press(at(friend_body)),
                    left_press(at(friend_body)),
                    right_down(at(friend_body + Vec2::new(10.0, 0.0))),
                    right_release(at(friend_body + Vec2::new(10.0, 0.0))),
                ],
                vec![Action::PanByScreenDelta(Vec2::new(10.0, 0.0))],
                Phase::Idle,
            ),
        ];

        for (name, doc, samples, want_actions, want_phase) in cases {
            let (state, actions) = run(doc, &samples);
            assert_eq!(actions, want_actions, "{name}: actions");
            assert_eq!(state.phase, want_phase, "{name}: final phase");
        }
    }

    #[test]
    fn pin_hit_radius_scales_with_zoom() {
        let doc = guard_doc();
        let pin = output_pin(doc.node(GREET).expect("node exists"), 0);
        // zoom 1: radius = PIN_R + 4 = 10 world units; 9 px away picks the pin.
        let (state, actions) = run(
            &doc,
            &[zoomed(left_press(at(pin + Vec2::new(9.0, 0.0))), 1.0)],
        );
        assert!(
            matches!(state.phase, Phase::DraggingLink { .. }),
            "9 px at zoom 1 picks the pin"
        );
        assert_eq!(actions[0], Action::Select(Some(GREET)));
        // 11 px at zoom 1 misses; the point lies right of the node edge, so it is Empty.
        let (state, actions) = run(
            &doc,
            &[zoomed(left_press(at(pin + Vec2::new(11.0, 0.0))), 1.0)],
        );
        assert_eq!(state.phase, Phase::Idle, "11 px at zoom 1 misses the pin");
        assert_eq!(actions, vec![Action::Select(None)]);
        // zoom 2: radius = 5 world units.
        let (state, _) = run(
            &doc,
            &[zoomed(left_press(at(pin + Vec2::new(9.0, 0.0))), 2.0)],
        );
        assert_eq!(state.phase, Phase::Idle, "9 px at zoom 2 misses the pin");
        let (state, _) = run(
            &doc,
            &[zoomed(left_press(at(pin + Vec2::new(4.0, 0.0))), 2.0)],
        );
        assert!(
            matches!(state.phase, Phase::DraggingLink { .. }),
            "4 px at zoom 2 picks the pin"
        );
        // zoom 0.5: radius = 20 world units.
        let (state, _) = run(
            &doc,
            &[zoomed(left_press(at(pin + Vec2::new(15.0, 0.0))), 0.5)],
        );
        assert!(
            matches!(state.phase, Phase::DraggingLink { .. }),
            "15 px at zoom 0.5 picks the pin"
        );
    }

    #[test]
    fn a_node_vanishing_mid_gesture_falls_back_to_idle() {
        let doc = guard_doc();
        let friend_body = body_point(&doc, FRIEND);

        // Node drag: the dragged node is deleted between ticks.
        let mut state = GraphInteraction::default();
        let press = left_press(at(friend_body));
        let actions = step(&mut state, &doc, &press, friend_body);
        assert_eq!(actions, vec![Action::Select(Some(FRIEND))]);
        assert!(matches!(state.phase, Phase::DraggingNode { .. }));
        let mut without = doc.clone();
        without.delete_node(FRIEND);
        let drag = left_down(at(friend_body + Vec2::new(5.0, 5.0)));
        let actions = step(&mut state, &without, &drag, friend_body);
        assert!(actions.is_empty(), "no MoveNode for a vanished node");
        assert_eq!(state.phase, Phase::Idle);

        // Link drag: the source node is deleted between ticks.
        let pin = output_pin(doc.node(GREET).expect("node exists"), 0);
        let mut state = GraphInteraction::default();
        let press = left_press(at(pin));
        let actions = step(&mut state, &doc, &press, pin);
        assert_eq!(actions, vec![Action::Select(Some(GREET))]);
        assert!(matches!(state.phase, Phase::DraggingLink { .. }));
        let mut without = doc.clone();
        without.delete_node(GREET);
        let release = left_release(at(body_point(&doc, PASS)));
        let actions = step(&mut state, &without, &release, pin);
        assert!(actions.is_empty(), "no SetLink for a vanished source node");
        assert_eq!(state.phase, Phase::Idle);
    }

    #[test]
    fn drag_preview_reports_the_carried_link() {
        let doc = guard_doc();
        let pin = output_pin(doc.node(GREET).expect("node exists"), 0);
        let idle = GraphInteraction::default();
        assert_eq!(drag_preview(&idle, &at(pin)), None, "no preview while idle");

        let mut state = GraphInteraction::default();
        let press = left_press(at(pin));
        let _ = step(&mut state, &doc, &press, pin);
        assert_eq!(
            drag_preview(&state, &press),
            Some((pin, pin)),
            "the preview starts at the pin"
        );

        let drag = left_down(at(pin + Vec2::new(12.0, -6.0)));
        let _ = step(&mut state, &doc, &drag, pin);
        assert_eq!(
            drag_preview(&state, &drag),
            Some((pin, pin + Vec2::new(12.0, -6.0))),
            "the preview follows the mouse in world coords"
        );

        let release = left_release(at(Vec2::new(-500.0, -500.0)));
        let _ = step(&mut state, &doc, &release, pin + Vec2::new(12.0, -6.0));
        assert_eq!(
            drag_preview(&state, &release),
            None,
            "no preview after release"
        );
    }
}

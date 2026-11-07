//! The "Nodes" window (a plain, non-docked window): the active doc's load warnings in a
//! collapsing header, then a selectable list of its node ids. Clicking a row selects the node.
//!
//! Locking: the workspace is locked only to snapshot the panel contents and to apply a click;
//! the widgets are drawn with the guard dropped.

use aberredengine::prelude::imgui::{Condition, TreeNodeFlags};
use aberredengine::prelude::*;

use crate::model::NodeKey;
use crate::state::with_workspace;

/// Snapshot of the Nodes panel.
struct NodesSnap {
    load_warnings: Vec<String>,
    /// `(key, id)` per node, in doc order.
    nodes: Vec<(NodeKey, String)>,
    selected: Option<NodeKey>,
}

fn snapshot(app_state: &AppState) -> Option<NodesSnap> {
    with_workspace(app_state, |ws| {
        let doc = ws.active_doc()?;
        Some(NodesSnap {
            load_warnings: doc.load_warnings.clone(),
            nodes: doc
                .nodes
                .iter()
                .map(|node| (node.key, node.id.clone()))
                .collect(),
            selected: doc.selected,
        })
    })
    .flatten()
}

/// Draws the Nodes window for the active doc.
pub fn draw(ctx: &mut GuiCtx) {
    let snap = snapshot(ctx.app_state);
    let mut clicked = None;
    let ui = ctx.ui;
    ui.window("Nodes")
        .size([240.0, 320.0], Condition::FirstUseEver)
        .build(|| match &snap {
            None => ui.text_disabled("(no nodes)"),
            Some(snap) => {
                if !snap.load_warnings.is_empty()
                    && ui.collapsing_header("Load warnings", TreeNodeFlags::DEFAULT_OPEN)
                {
                    for warning in &snap.load_warnings {
                        ui.bullet_text(warning);
                    }
                }
                if snap.nodes.is_empty() {
                    ui.text_disabled("(no nodes)");
                }
                for (key, id) in &snap.nodes {
                    // Each row gets its own id scope from the NodeKey, so the selectable's
                    // label is the plain node id even when node ids duplicate.
                    let _row = ui.push_id_usize(key.0 as usize);
                    if ui
                        .selectable_config(id)
                        .selected(snap.selected == Some(*key))
                        .build()
                    {
                        clicked = Some(*key);
                    }
                }
            }
        });

    if let Some(key) = clicked {
        with_workspace(ctx.app_state, |ws| {
            if let Some(doc) = ws.active_doc_mut() {
                doc.select(Some(key));
            }
        });
    }
}

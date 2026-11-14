//! Read-only destination facts from an already resolved docking preview.

use crate::geometry::LogicalRect;
use crate::graph::command::{Edge, MovePayload};
use crate::ids::{ItemId, RootId, SurfaceId};
use crate::interaction::targeting::targets::DropTargetId;
use crate::model::DockEdge;
use crate::presentation::scene::{PaneSceneId, PresentationPlan, TabBarSceneId};

use super::{DockspaceVisualId, VisualIdentity};

#[cfg(test)]
mod tests;

/// Current presented region addressed by one resolved docking action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockspaceDropTargetScope {
    /// Merge with or split one tabs leaf.
    Pane,
    /// Split the complete root at an outer edge.
    Root,
    /// Install a main root on an empty logical surface.
    Surface,
}

/// Semantic result of the already resolved target, without command authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockspaceDropAction {
    /// Merge the payload into the target tabs stack.
    Merge,
    /// Insert at an exact gap in the presented target's tab sequence.
    InsertTab {
        /// Gap index before removing the source payload from its original location.
        index: usize,
        /// Nearest preceding target item after excluding the complete payload.
        previous: Option<ItemId>,
        /// Nearest following target item after excluding the complete payload.
        next: Option<ItemId>,
    },
    /// Split the target region; its scope distinguishes inner from outer docking.
    Split {
        /// Physical side receiving the payload.
        edge: DockEdge,
    },
    /// Install the payload as the main root of a rootless logical surface.
    InstallRoot,
}

/// Ephemeral destination feedback bound to one exact drag preview.
///
/// Bounds and selection describe the currently presented destination, before
/// source removal. The preview's separate visual describes the resulting layout.
/// This value carries no input, mutation or presentation acknowledgement authority.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DockspaceDropTargetFeedback {
    target: DropTargetId,
    selected: Option<ItemId>,
    bounds: LogicalRect,
    action: DockspaceDropAction,
}

impl DockspaceDropTargetFeedback {
    /// Returns the target's logical surface.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.target.surface()
    }

    /// Returns the existing destination root, absent for a rootless surface.
    #[must_use]
    pub const fn root(self) -> Option<RootId> {
        self.target.root()
    }

    /// Returns whether the destination is a pane, root or empty surface.
    #[must_use]
    pub const fn scope(self) -> DockspaceDropTargetScope {
        match self.target {
            DropTargetId::Center { .. }
            | DropTargetId::TabGap { .. }
            | DropTargetId::InnerEdge { .. } => DockspaceDropTargetScope::Pane,
            DropTargetId::OuterEdge { .. } => DockspaceDropTargetScope::Root,
            DropTargetId::SurfaceBackground { .. } => DockspaceDropTargetScope::Surface,
        }
    }

    /// Returns the stable opaque identity of the resolved destination.
    #[must_use]
    pub const fn visual_id(self) -> DockspaceVisualId {
        DockspaceVisualId(VisualIdentity::DropTarget(self.target))
    }

    /// Returns the item representing the presented pane, before the payload is selected.
    ///
    /// Empty central panes, complete-root edges and empty surfaces have no unique item.
    #[must_use]
    pub const fn selected_item(self) -> Option<ItemId> {
        self.selected
    }

    /// Returns the current visible pane or root bounds, not the future preview rectangle.
    #[must_use]
    pub const fn bounds(self) -> LogicalRect {
        self.bounds
    }

    /// Returns the merge, insertion, split or root-install action selected by core.
    #[must_use]
    pub const fn action(self) -> DockspaceDropAction {
        self.action
    }
}

pub(super) fn resolve(
    plan: &PresentationPlan,
    target: DropTargetId,
    payload: Option<&MovePayload>,
) -> Option<DockspaceDropTargetFeedback> {
    plan.drop_target(target)?;
    let (pane, action) = match target {
        DropTargetId::Center { root, tabs, .. } => {
            (Some(PaneSceneId { root, tabs }), DockspaceDropAction::Merge)
        }
        DropTargetId::TabGap {
            root, tabs, index, ..
        } => {
            let id = TabBarSceneId { root, tabs };
            let bars = plan.tab_bar_records();
            let bar = &bars[bars.binary_search_by_key(&id, |bar| *bar.id()).ok()?];
            let members = bar.members();
            let excludes = |item| match payload {
                Some(MovePayload::Item(source)) => source.item() == item,
                Some(MovePayload::Tabs(source) | MovePayload::Subtree(source)) => {
                    // Successful tab-gap resolution rejects a target inside a split
                    // payload. Only a same-leaf group can overlap its destination.
                    source.root() == root && source.node() == tabs
                }
                None => false,
            };
            let previous = members
                .get(..index)?
                .iter()
                .rev()
                .map(|member| member.tab().item)
                .find(|item| !excludes(*item));
            let next = members
                .get(index..)?
                .iter()
                .map(|member| member.tab().item)
                .find(|item| !excludes(*item));
            (
                Some(PaneSceneId { root, tabs }),
                DockspaceDropAction::InsertTab {
                    index,
                    previous,
                    next,
                },
            )
        }
        DropTargetId::InnerEdge {
            root, node, edge, ..
        } => (
            Some(PaneSceneId { root, tabs: node }),
            DockspaceDropAction::Split {
                edge: public_edge(edge),
            },
        ),
        DropTargetId::OuterEdge { edge, .. } => (
            None,
            DockspaceDropAction::Split {
                edge: public_edge(edge),
            },
        ),
        DropTargetId::SurfaceBackground { .. } => (None, DockspaceDropAction::InstallRoot),
    };
    let (selected, bounds) = match pane {
        Some(id) => {
            let panes = plan.pane_records();
            let pane = &panes[panes.binary_search_by_key(&id, |pane| pane.id()).ok()?];
            (plan.pane_selected(pane), pane.bounds())
        }
        None => (
            None,
            match target.root() {
                Some(root) => plan.layout_facts()?.root(root)?.bounds(),
                None => plan.bounds(),
            },
        ),
    };
    Some(DockspaceDropTargetFeedback {
        target,
        selected,
        bounds,
        action,
    })
}

const fn public_edge(edge: Edge) -> DockEdge {
    match edge {
        Edge::Left => DockEdge::Left,
        Edge::Right => DockEdge::Right,
        Edge::Top => DockEdge::Top,
        Edge::Bottom => DockEdge::Bottom,
    }
}

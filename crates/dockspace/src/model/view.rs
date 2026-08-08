use std::fmt;

use crate::geometry::LogicalRect;
use crate::graph::{Node, SplitWeight, SurfacePresentation, Workspace};
use crate::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};

use super::DockspaceAxis;

/// Read-only product view of a validated docking workspace.
#[derive(Clone, Copy)]
pub struct DockspaceView<'a> {
    workspace: &'a Workspace,
}

impl fmt::Debug for DockspaceView<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DockspaceView")
            .field("surfaces", &self.surfaces().collect::<Vec<_>>())
            .finish()
    }
}

impl<'a> DockspaceView<'a> {
    pub(crate) const fn new(workspace: &'a Workspace) -> Self {
        Self { workspace }
    }

    /// Returns a logical surface by stable identity.
    #[must_use]
    pub fn surface(self, id: SurfaceId) -> Option<DockspaceSurfaceView<'a>> {
        self.workspace
            .surface(id)
            .map(|presentation| DockspaceSurfaceView {
                workspace: self.workspace,
                id,
                presentation,
            })
    }

    /// Iterates logical surfaces in stable identity order.
    pub fn surfaces(self) -> impl Iterator<Item = DockspaceSurfaceView<'a>> + 'a {
        self.workspace
            .surfaces()
            .map(move |(id, presentation)| DockspaceSurfaceView {
                workspace: self.workspace,
                id,
                presentation,
            })
    }

    /// Returns a docking root by stable identity.
    #[must_use]
    pub fn root(self, id: RootId) -> Option<DockspaceRootView<'a>> {
        self.workspace.root(id).map(|record| DockspaceRootView {
            workspace: self.workspace,
            id,
            content: record.node,
            central: record.central,
        })
    }

    /// Returns a contained-floating presentation by stable identity.
    #[must_use]
    pub fn contained(self, id: FloatingPresentationId) -> Option<DockspaceContainedView<'a>> {
        self.workspace
            .contained_floating(id)
            .map(|contained| DockspaceContainedView {
                workspace: self.workspace,
                id,
                root: contained.root,
                rect: contained.rect,
            })
    }
}

/// Read-only product view of one logical surface.
#[derive(Clone, Copy)]
pub struct DockspaceSurfaceView<'a> {
    workspace: &'a Workspace,
    id: SurfaceId,
    presentation: &'a SurfacePresentation,
}

impl fmt::Debug for DockspaceSurfaceView<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DockspaceSurfaceView")
            .field("id", &self.id)
            .field("main_root", &self.main_root())
            .field("contained", &self.contained().collect::<Vec<_>>())
            .finish()
    }
}

impl<'a> DockspaceSurfaceView<'a> {
    /// Returns the stable surface identity.
    #[must_use]
    pub const fn id(self) -> SurfaceId {
        self.id
    }

    /// Returns whether the surface has no main root.
    #[must_use]
    pub const fn is_rootless(self) -> bool {
        self.presentation.main_root.is_none()
    }

    /// Returns the main docking root, when present.
    #[must_use]
    pub fn main_root(self) -> Option<DockspaceRootView<'a>> {
        self.presentation.main_root.and_then(|root| {
            self.workspace.root(root).map(|record| DockspaceRootView {
                workspace: self.workspace,
                id: root,
                content: record.node,
                central: record.central,
            })
        })
    }

    /// Returns the number of contained-floating roots on this surface.
    #[must_use]
    pub fn contained_count(self) -> usize {
        self.presentation.contained.len()
    }

    /// Iterates contained-floating roots in back-to-front presentation order.
    pub fn contained(self) -> impl Iterator<Item = DockspaceContainedView<'a>> + 'a {
        let workspace = self.workspace;
        self.presentation.contained.iter().filter_map(move |id| {
            workspace
                .contained_floating(*id)
                .map(|contained| DockspaceContainedView {
                    workspace,
                    id: *id,
                    root: contained.root,
                    rect: contained.rect,
                })
        })
    }
}

/// Read-only product view of one stable docking root.
#[derive(Clone, Copy)]
pub struct DockspaceRootView<'a> {
    workspace: &'a Workspace,
    id: RootId,
    content: NodeId,
    central: Option<NodeId>,
}

impl fmt::Debug for DockspaceRootView<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DockspaceRootView")
            .field("id", &self.id)
            .field("content", &self.content())
            .finish()
    }
}

impl<'a> DockspaceRootView<'a> {
    /// Returns the stable root identity.
    #[must_use]
    pub const fn id(self) -> RootId {
        self.id
    }

    /// Returns the root topology without exposing its runtime identity.
    #[must_use]
    pub fn content(self) -> Option<DockspaceNodeView<'a>> {
        self.workspace
            .node(self.content)
            .map(|_| DockspaceNodeView {
                workspace: self.workspace,
                node: self.content,
                central: self.central,
            })
    }

    /// Returns the tabs leaf that receives remaining central space, when configured.
    #[must_use]
    pub fn central(self) -> Option<DockspaceNodeView<'a>> {
        self.central.and_then(|node| {
            self.workspace.node(node).map(|_| DockspaceNodeView {
                workspace: self.workspace,
                node,
                central: self.central,
            })
        })
    }
}

/// Read-only product view of one contained-floating presentation.
#[derive(Clone, Copy)]
pub struct DockspaceContainedView<'a> {
    workspace: &'a Workspace,
    id: FloatingPresentationId,
    root: RootId,
    rect: LogicalRect,
}

impl fmt::Debug for DockspaceContainedView<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DockspaceContainedView")
            .field("id", &self.id)
            .field("rect", &self.rect)
            .field("root", &self.root())
            .finish()
    }
}

impl<'a> DockspaceContainedView<'a> {
    /// Returns the stable contained-floating identity.
    #[must_use]
    pub const fn id(self) -> FloatingPresentationId {
        self.id
    }

    /// Returns durable surface-local bounds.
    #[must_use]
    pub const fn rect(self) -> LogicalRect {
        self.rect
    }

    /// Returns the presented docking root.
    #[must_use]
    pub fn root(self) -> Option<DockspaceRootView<'a>> {
        self.workspace
            .root(self.root)
            .map(|record| DockspaceRootView {
                workspace: self.workspace,
                id: self.root,
                content: record.node,
                central: record.central,
            })
    }
}

/// Read-only product view of one tabs or split node.
#[derive(Clone, Copy)]
pub struct DockspaceNodeView<'a> {
    workspace: &'a Workspace,
    node: NodeId,
    central: Option<NodeId>,
}

impl fmt::Debug for DockspaceNodeView<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(tabs) = self.tabs() {
            return formatter
                .debug_struct("DockspaceNodeView")
                .field("tabs", &tabs)
                .finish();
        }
        if let Some(split) = self.split() {
            return formatter
                .debug_struct("DockspaceNodeView")
                .field("split", &split)
                .finish();
        }
        formatter.write_str("DockspaceNodeView(<invalid>)")
    }
}

impl<'a> DockspaceNodeView<'a> {
    /// Returns whether this node is the root's central tabs leaf.
    #[must_use]
    pub fn is_central(self) -> bool {
        self.central == Some(self.node)
    }

    /// Returns a tabs view when this node is a tabs leaf.
    #[must_use]
    pub fn tabs(self) -> Option<DockspaceTabsView<'a>> {
        match self.workspace.node(self.node) {
            Some(Node::Tabs { items, selected }) => Some(DockspaceTabsView {
                items,
                selected: *selected,
                central: self.is_central(),
            }),
            Some(Node::Split { .. }) | None => None,
        }
    }

    /// Returns a split view when this node is an N-ary split.
    #[must_use]
    pub fn split(self) -> Option<DockspaceSplitView<'a>> {
        match self.workspace.node(self.node) {
            Some(Node::Split {
                axis,
                children,
                weights,
            }) => Some(DockspaceSplitView {
                workspace: self.workspace,
                axis: (*axis).into(),
                children,
                weights,
                central: self.central,
            }),
            Some(Node::Tabs { .. }) | None => None,
        }
    }
}

/// Read-only product view of one tabs leaf.
#[derive(Debug, Clone, Copy)]
pub struct DockspaceTabsView<'a> {
    items: &'a [ItemId],
    selected: Option<ItemId>,
    central: bool,
}

impl<'a> DockspaceTabsView<'a> {
    /// Returns items in visual tab order.
    #[must_use]
    pub const fn items(self) -> &'a [ItemId] {
        self.items
    }

    /// Returns the selected item, or `None` for an empty central leaf.
    #[must_use]
    pub const fn selected(self) -> Option<ItemId> {
        self.selected
    }

    /// Returns whether this leaf receives the root's remaining central space.
    #[must_use]
    pub const fn is_central(self) -> bool {
        self.central
    }
}

/// Read-only product view of one N-ary split.
#[derive(Clone, Copy)]
pub struct DockspaceSplitView<'a> {
    workspace: &'a Workspace,
    axis: DockspaceAxis,
    children: &'a [NodeId],
    weights: &'a [SplitWeight],
    central: Option<NodeId>,
}

impl fmt::Debug for DockspaceSplitView<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DockspaceSplitView")
            .field("axis", &self.axis)
            .field("children", &self.children().collect::<Vec<_>>())
            .field("weights", &self.weights().collect::<Vec<_>>())
            .finish()
    }
}

impl<'a> DockspaceSplitView<'a> {
    /// Returns the split direction.
    #[must_use]
    pub const fn axis(self) -> DockspaceAxis {
        self.axis
    }

    /// Returns the number of ordered children.
    #[must_use]
    pub fn child_count(self) -> usize {
        self.children.len()
    }

    /// Iterates child nodes in visual order.
    pub fn children(self) -> impl Iterator<Item = DockspaceNodeView<'a>> + 'a {
        let workspace = self.workspace;
        let central = self.central;
        self.children
            .iter()
            .copied()
            .map(move |node| DockspaceNodeView {
                workspace,
                node,
                central,
            })
    }

    /// Iterates normalized child weights in visual order.
    pub fn weights(self) -> impl Iterator<Item = f32> + 'a {
        self.weights.iter().map(|weight| weight.get())
    }
}

//! Checked workspace queries used to capture mutation preconditions.

use std::collections::HashSet;
use std::sync::Arc;

use crate::command::{
    DockFraction, Edge, EdgeTarget, FingerprintNode, FingerprintPresentation, ItemSource,
    NodeFingerprint, NodeFingerprintRecord, NodeSource, RootFingerprintData, TabTarget,
};
use crate::error::{CommandError, ReferenceRole};
use crate::graph::{ContainedStackKey, Node, Workspace};
use crate::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ParentLink {
    pub(crate) parent: NodeId,
    pub(crate) index: usize,
}

/// Current presentation which exclusively owns a logical docking root.
///
/// A root keeps its identity when it moves between these presentation
/// carriers. Adapters can use this fact to preserve presentation identities
/// without inspecting workspace storage or guessing from geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootPresentationOwner {
    /// The root is the main content of a logical surface.
    Main {
        /// Surface which directly owns the root.
        surface: SurfaceId,
    },
    /// The root is presented by a contained floating on a logical surface.
    Contained {
        /// Surface which hosts the contained presentation.
        surface: SurfaceId,
        /// Stable contained presentation identity.
        floating: FloatingPresentationId,
    },
}

impl Workspace {
    /// Returns the deterministic frontmost contained presentation on a surface.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError::MissingSurface`] when `surface` is absent.
    pub fn contained_frontmost(
        &self,
        surface: SurfaceId,
    ) -> Result<Option<ContainedStackKey>, CommandError> {
        if !self.surfaces.contains_key(&surface) {
            return Err(CommandError::MissingSurface { surface });
        }
        Ok(self
            .contained_floatings
            .values()
            .filter(|entry| entry.surface == surface)
            .map(|entry| entry.stacking_key())
            .max())
    }

    /// Captures one item source and the complete state of its owning root.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] when the root, tabs node, or item is absent, or
    /// the node does not belong to the supplied root.
    pub fn capture_item_source(
        &self,
        root: RootId,
        tabs: NodeId,
        item: ItemId,
    ) -> Result<ItemSource, CommandError> {
        let fingerprint = self.capture_reference(root, tabs, ReferenceRole::Source)?;
        match self.nodes.get(tabs) {
            Some(Node::Tabs { items, .. }) if items.contains(&item) => Ok(ItemSource {
                root,
                tabs,
                item,
                fingerprint,
            }),
            Some(Node::Tabs { .. }) => Err(CommandError::ItemNotInTabs { tabs, item }),
            Some(Node::Split { .. }) => Err(CommandError::NodeIsNotTabs { node: tabs }),
            None => Err(CommandError::MissingNode {
                role: ReferenceRole::Source,
                node: tabs,
            }),
        }
    }

    /// Captures one source node and the complete state of its owning root.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] when the root or node is absent, or the node
    /// does not belong to the supplied root.
    pub fn capture_node_source(
        &self,
        root: RootId,
        node: NodeId,
    ) -> Result<NodeSource, CommandError> {
        Ok(NodeSource {
            root,
            node,
            fingerprint: self.capture_reference(root, node, ReferenceRole::Source)?,
        })
    }

    /// Captures one tabs target and the complete state of its owning root.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] when the root or node is absent, the node is
    /// outside the root, or it is not a tabs leaf.
    pub fn capture_tab_target(
        &self,
        root: RootId,
        tabs: NodeId,
    ) -> Result<TabTarget, CommandError> {
        let fingerprint = self.capture_reference(root, tabs, ReferenceRole::Target)?;
        if !matches!(self.nodes.get(tabs), Some(Node::Tabs { .. })) {
            return Err(CommandError::NodeIsNotTabs { node: tabs });
        }
        Ok(TabTarget {
            root,
            tabs,
            fingerprint,
        })
    }

    /// Captures one edge target and the complete state of its owning root.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] when the root or target node is absent, or the
    /// node does not belong to the supplied root.
    pub fn capture_edge_target(
        &self,
        root: RootId,
        node: NodeId,
        edge: Edge,
        fraction: DockFraction,
    ) -> Result<EdgeTarget, CommandError> {
        Ok(EdgeTarget {
            root,
            node,
            fingerprint: self.capture_reference(root, node, ReferenceRole::Target)?,
            edge,
            fraction,
        })
    }

    pub(crate) fn verify_reference(
        &self,
        root: RootId,
        node: NodeId,
        expected: &NodeFingerprint,
        role: ReferenceRole,
    ) -> Result<(), CommandError> {
        let actual = self.capture_reference(root, node, role)?;
        if actual == *expected {
            Ok(())
        } else {
            Err(CommandError::StaleNode {
                role,
                node,
                expected: expected.clone(),
                actual,
            })
        }
    }

    pub(crate) fn capture_reference(
        &self,
        root: RootId,
        node: NodeId,
        role: ReferenceRole,
    ) -> Result<NodeFingerprint, CommandError> {
        if !self.roots.contains_key(&root) {
            return Err(CommandError::MissingRoot { root });
        }
        if !self.nodes.contains_key(node) {
            return Err(CommandError::MissingNode { role, node });
        }
        if !self.root_contains_node(root, node) {
            return Err(CommandError::NodeOutsideRoot { role, root, node });
        }
        self.root_fingerprint(root)
    }

    pub(crate) fn root_fingerprint(&self, root: RootId) -> Result<NodeFingerprint, CommandError> {
        let record = self
            .roots
            .get(&root)
            .ok_or(CommandError::MissingRoot { root })?;
        let presentation = match self.presentation_for_root(root) {
            Some(RootPresentationOwner::Main { surface }) => {
                FingerprintPresentation::Main { surface }
            }
            Some(RootPresentationOwner::Contained { surface, floating }) => {
                FingerprintPresentation::Contained { surface, floating }
            }
            None => {
                return Err(CommandError::Invariant {
                    stage: "capture root presentation owner",
                });
            }
        };

        let mut nodes = Vec::new();
        let mut stack = vec![record.node];
        let mut visited = HashSet::new();
        while let Some(id) = stack.pop() {
            if !visited.insert(id) {
                return Err(CommandError::Invariant {
                    stage: "capture acyclic root fingerprint",
                });
            }
            let node = self.nodes.get(id).ok_or(CommandError::MissingNode {
                role: ReferenceRole::Source,
                node: id,
            })?;
            let snapshot = match node {
                Node::Tabs { items, selected } => FingerprintNode::Tabs {
                    items: items.clone(),
                    selected: *selected,
                },
                Node::Split {
                    axis,
                    children,
                    weights,
                } => {
                    stack.extend(children.iter().rev().copied());
                    FingerprintNode::Split {
                        axis: *axis,
                        children: children.clone(),
                        weight_bits: weights
                            .iter()
                            .map(|weight| weight.get().to_bits())
                            .collect(),
                    }
                }
            };
            nodes.push(NodeFingerprintRecord { id, node: snapshot });
        }

        Ok(NodeFingerprint(Arc::new(RootFingerprintData {
            root,
            root_node: record.node,
            central: record.central,
            nodes,
            presentation,
        })))
    }

    pub(crate) fn root_contains_node(&self, root: RootId, needle: NodeId) -> bool {
        let Some(record) = self.roots.get(&root) else {
            return false;
        };
        self.subtree_contains(record.node, needle)
    }

    pub(crate) fn subtree_contains(&self, root: NodeId, needle: NodeId) -> bool {
        let mut stack = vec![root];
        let mut visited = HashSet::new();
        while let Some(node_id) = stack.pop() {
            if node_id == needle {
                return true;
            }
            if !visited.insert(node_id) {
                continue;
            }
            if let Some(Node::Split { children, .. }) = self.nodes.get(node_id) {
                stack.extend(children.iter().rev().copied());
            }
        }
        false
    }

    pub(crate) fn parent_link(&self, root: RootId, needle: NodeId) -> Option<ParentLink> {
        let root_node = self.roots.get(&root)?.node;
        if root_node == needle {
            return None;
        }
        let mut stack = vec![root_node];
        let mut visited = HashSet::new();
        while let Some(node_id) = stack.pop() {
            if !visited.insert(node_id) {
                continue;
            }
            if let Some(Node::Split { children, .. }) = self.nodes.get(node_id) {
                for (index, child) in children.iter().copied().enumerate() {
                    if child == needle {
                        return Some(ParentLink {
                            parent: node_id,
                            index,
                        });
                    }
                }
                stack.extend(children.iter().rev().copied());
            }
        }
        None
    }

    pub(crate) fn collect_items_in_subtree(&self, root: NodeId) -> Vec<ItemId> {
        let mut items = Vec::new();
        let mut stack = vec![root];
        let mut visited = HashSet::new();
        while let Some(node_id) = stack.pop() {
            if !visited.insert(node_id) {
                continue;
            }
            match self.nodes.get(node_id) {
                Some(Node::Tabs {
                    items: tab_items, ..
                }) => items.extend(tab_items.iter().copied()),
                Some(Node::Split { children, .. }) => {
                    stack.extend(children.iter().rev().copied());
                }
                None => {}
            }
        }
        items
    }

    /// Returns the current presentation owner of `root`.
    ///
    /// Valid workspaces have exactly one owner for every root. `None` means the
    /// root is absent or the workspace has not passed validation.
    #[must_use]
    pub fn presentation_for_root(&self, root: RootId) -> Option<RootPresentationOwner> {
        for (surface_id, surface) in &self.surfaces {
            if surface.main_root == root {
                return Some(RootPresentationOwner::Main {
                    surface: *surface_id,
                });
            }
            for floating_id in &surface.contained {
                if self
                    .contained_floatings
                    .get(floating_id)
                    .is_some_and(|floating| floating.root == root)
                {
                    return Some(RootPresentationOwner::Contained {
                        surface: *surface_id,
                        floating: *floating_id,
                    });
                }
            }
        }
        None
    }
}

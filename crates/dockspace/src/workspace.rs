//! Checked workspace queries used to capture mutation preconditions.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

use crate::command::{
    ContainedRosterSource, DockFraction, Edge, EdgeTarget, EdgeTargetScope, FingerprintNode,
    FingerprintPresentation, ItemSource, MovePayload, NodeFingerprint, NodeFingerprintRecord,
    NodeSource, RootFingerprintData, SurfaceRosterSource, TabTarget,
};
use crate::error::{CommandError, ReferenceRole};
use crate::graph::{Node, Workspace};
use crate::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use crate::policy::DockTargetRuleKey;
use crate::transition::WorkspaceVersion;

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

/// Immutable structural authority derived for one exact workspace revision.
///
/// Scene compilation shares this index across every target captured from the
/// same requirement manifest. Replacing the manifest replaces the complete
/// index, so no entry can be reused across workspace revisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceIndex {
    version: WorkspaceVersion,
    roots: BTreeMap<RootId, WorkspaceRootIndex>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkspaceRootIndex {
    root_node: NodeId,
    central: Option<NodeId>,
    owner: RootPresentationOwner,
    members: BTreeSet<NodeId>,
    item_count: usize,
    fingerprint: NodeFingerprint,
}

impl WorkspaceIndex {
    pub(crate) fn build(
        workspace: &Workspace,
        version: WorkspaceVersion,
    ) -> Result<Self, CommandError> {
        let mut owners = BTreeMap::new();
        for (surface, presentation) in &workspace.surfaces {
            if let Some(root) = presentation.main_root
                && owners
                    .insert(root, RootPresentationOwner::Main { surface: *surface })
                    .is_some()
            {
                return Err(CommandError::Invariant {
                    stage: "index unique root presentation owner",
                });
            }
            for floating in &presentation.contained {
                let root = workspace
                    .contained_floatings
                    .get(floating)
                    .ok_or(CommandError::Invariant {
                        stage: "index contained presentation root",
                    })?
                    .root;
                if owners
                    .insert(
                        root,
                        RootPresentationOwner::Contained {
                            surface: *surface,
                            floating: *floating,
                        },
                    )
                    .is_some()
                {
                    return Err(CommandError::Invariant {
                        stage: "index unique root presentation owner",
                    });
                }
            }
        }

        let mut roots = BTreeMap::new();
        for root in workspace.roots.keys().copied() {
            let owner = owners.get(&root).copied().ok_or(CommandError::Invariant {
                stage: "index root presentation owner",
            })?;
            roots.insert(root, workspace.build_root_index(root, owner)?);
        }
        if owners.len() != roots.len() {
            return Err(CommandError::Invariant {
                stage: "index presentation owner roster",
            });
        }
        Ok(Self { version, roots })
    }

    #[cfg(test)]
    pub(crate) fn empty_for_test(version: WorkspaceVersion) -> Self {
        Self {
            version,
            roots: BTreeMap::new(),
        }
    }

    pub(crate) const fn version(&self) -> WorkspaceVersion {
        self.version
    }

    fn require_version(&self, version: WorkspaceVersion) -> Result<(), CommandError> {
        if self.version == version {
            Ok(())
        } else {
            Err(CommandError::Invariant {
                stage: "use workspace index for a different revision",
            })
        }
    }

    fn capture_reference<'index>(
        &'index self,
        workspace: &Workspace,
        version: WorkspaceVersion,
        root: RootId,
        node: NodeId,
        role: ReferenceRole,
    ) -> Result<&'index WorkspaceRootIndex, CommandError> {
        self.require_version(version)?;
        let indexed = self
            .roots
            .get(&root)
            .ok_or(CommandError::MissingRoot { root })?;
        if !workspace.nodes.contains_key(node) {
            return Err(CommandError::MissingNode { role, node });
        }
        if !indexed.members.contains(&node) {
            return Err(CommandError::NodeOutsideRoot { role, root, node });
        }
        Ok(indexed)
    }

    pub(crate) fn verify_reference(
        &self,
        workspace: &Workspace,
        version: WorkspaceVersion,
        root: RootId,
        node: NodeId,
        expected: &NodeFingerprint,
        role: ReferenceRole,
    ) -> Result<(), CommandError> {
        let indexed = self.capture_reference(workspace, version, root, node, role)?;
        if indexed.fingerprint == *expected {
            Ok(())
        } else {
            Err(CommandError::StaleNode {
                role,
                node,
                expected: expected.clone(),
                actual: indexed.fingerprint.clone(),
            })
        }
    }

    pub(crate) fn capture_complete_root_source(
        &self,
        workspace: &Workspace,
        version: WorkspaceVersion,
        payload: &MovePayload,
    ) -> Result<Option<NodeSource>, CommandError> {
        let (root, node, fingerprint) = match payload {
            MovePayload::Item(source) => (source.root(), source.tabs(), source.fingerprint()),
            MovePayload::Tabs(source) | MovePayload::Subtree(source) => {
                (source.root(), source.node(), source.fingerprint())
            }
        };
        let indexed =
            self.capture_reference(workspace, version, root, node, ReferenceRole::Source)?;
        if indexed.fingerprint != *fingerprint {
            return Err(CommandError::StaleNode {
                role: ReferenceRole::Source,
                node,
                expected: fingerprint.clone(),
                actual: indexed.fingerprint.clone(),
            });
        }

        let removes_complete_root = match payload {
            MovePayload::Item(source) => {
                indexed.central.is_none()
                    && indexed.item_count == 1
                    && matches!(
                        workspace.node(source.tabs()),
                        Some(Node::Tabs { items, .. }) if items.contains(&source.item())
                    )
            }
            MovePayload::Tabs(source) => {
                source.node() == indexed.root_node
                    && matches!(
                        workspace.node(source.node()),
                        Some(Node::Tabs { items, .. }) if !items.is_empty()
                    )
            }
            MovePayload::Subtree(source) => {
                source.node() == indexed.root_node && indexed.item_count > 0
            }
        };
        Ok(removes_complete_root.then(|| NodeSource {
            root,
            node: indexed.root_node,
            fingerprint: indexed.fingerprint.clone(),
        }))
    }

    pub(crate) fn capture_tab_target(
        &self,
        workspace: &Workspace,
        version: WorkspaceVersion,
        root: RootId,
        tabs: NodeId,
    ) -> Result<TabTarget, CommandError> {
        let indexed =
            self.capture_reference(workspace, version, root, tabs, ReferenceRole::Target)?;
        let central = indexed.central == Some(tabs);
        let rule = workspace.pane_local_target_rule(root, tabs, central)?;
        Ok(TabTarget {
            root,
            tabs,
            fingerprint: indexed.fingerprint.clone(),
            surface: indexed.owner.surface(),
            central,
            rule,
        })
    }

    pub(crate) fn capture_inner_edge_target(
        &self,
        workspace: &Workspace,
        version: WorkspaceVersion,
        root: RootId,
        tabs: NodeId,
        edge: Edge,
        fraction: DockFraction,
    ) -> Result<EdgeTarget, CommandError> {
        let indexed =
            self.capture_reference(workspace, version, root, tabs, ReferenceRole::Target)?;
        let central = indexed.central == Some(tabs);
        let rule = workspace.pane_local_target_rule(root, tabs, central)?;
        Ok(EdgeTarget {
            root,
            node: tabs,
            fingerprint: indexed.fingerprint.clone(),
            edge,
            fraction,
            surface: indexed.owner.surface(),
            central,
            rule,
            scope: EdgeTargetScope::Inner,
        })
    }

    pub(crate) fn capture_outer_edge_target(
        &self,
        workspace: &Workspace,
        version: WorkspaceVersion,
        root: RootId,
        edge: Edge,
        fraction: DockFraction,
    ) -> Result<EdgeTarget, CommandError> {
        self.require_version(version)?;
        let indexed = self
            .roots
            .get(&root)
            .ok_or(CommandError::MissingRoot { root })?;
        if workspace.root(root).is_none() {
            return Err(CommandError::MissingRoot { root });
        }
        Ok(EdgeTarget {
            root,
            node: indexed.root_node,
            fingerprint: indexed.fingerprint.clone(),
            edge,
            fraction,
            surface: indexed.owner.surface(),
            central: indexed.central == Some(indexed.root_node),
            rule: DockTargetRuleKey::Root(root),
            scope: EdgeTargetScope::Outer,
        })
    }
}

impl RootPresentationOwner {
    const fn surface(self) -> SurfaceId {
        match self {
            Self::Main { surface } | Self::Contained { surface, .. } => surface,
        }
    }
}

impl Workspace {
    pub(crate) fn matches_surface_roster_source(&self, source: &SurfaceRosterSource) -> bool {
        let Some(presentation) = self.surface(source.surface()) else {
            return false;
        };
        if presentation.main_root != source.main_root()
            || presentation.contained.len() != source.contained().len()
        {
            return false;
        }
        if let Some(main) = source.main_source() {
            let Ok(actual) = self.capture_node_source(main.root(), main.node()) else {
                return false;
            };
            if actual != *main {
                return false;
            }
        }

        presentation
            .contained
            .iter()
            .zip(source.contained())
            .all(|(floating, expected)| {
                *floating == expected.floating()
                    && self.contained_floating(*floating).is_some_and(|record| {
                        record.root == expected.root() && record.rect == expected.rect()
                    })
                    && self
                        .capture_node_source(expected.root(), expected.source().node())
                        .is_ok_and(|actual| actual == *expected.source())
            })
    }

    /// Captures the complete contained roster of one surface.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError::MissingSurface`] when `surface` is absent.
    pub fn capture_contained_roster(
        &self,
        surface: SurfaceId,
    ) -> Result<ContainedRosterSource, CommandError> {
        let presentation = self
            .surfaces
            .get(&surface)
            .ok_or(CommandError::MissingSurface { surface })?;
        Ok(ContainedRosterSource {
            surface,
            contained: Arc::from(presentation.contained.clone()),
        })
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

    /// Locates an item once and captures its owning source.
    ///
    /// Valid workspaces contain every item exactly once and every node in exactly
    /// one root. Locating the tabs node before walking root subtrees avoids the
    /// former root-by-all-nodes nested scan on pointer-click hot paths.
    pub(crate) fn capture_item_source_by_id(
        &self,
        item: ItemId,
    ) -> Result<Option<ItemSource>, CommandError> {
        let Some(tabs) = self.nodes.iter().find_map(|(tabs, node)| match node {
            Node::Tabs { items, .. } if items.contains(&item) => Some(tabs),
            Node::Tabs { .. } | Node::Split { .. } => None,
        }) else {
            return Ok(None);
        };
        let Some(root) = self
            .roots
            .iter()
            .find_map(|(root, record)| self.subtree_contains(record.node, tabs).then_some(*root))
        else {
            return Ok(None);
        };
        self.capture_item_source(root, tabs, item).map(Some)
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
        let (surface, central) = self.target_owner_facts(root, tabs)?;
        let rule = self.pane_local_target_rule(root, tabs, central)?;
        Ok(TabTarget {
            root,
            tabs,
            fingerprint,
            surface,
            central,
            rule,
        })
    }

    /// Captures one branch-local edge of a selected tabs leaf.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] when the root or tabs node is absent, the node
    /// does not belong to the supplied root, is not a tabs leaf, or has no
    /// selected item to represent its pane-local policy scope.
    pub fn capture_inner_edge_target(
        &self,
        root: RootId,
        tabs: NodeId,
        edge: Edge,
        fraction: DockFraction,
    ) -> Result<EdgeTarget, CommandError> {
        let fingerprint = self.capture_reference(root, tabs, ReferenceRole::Target)?;
        let (surface, central) = self.target_owner_facts(root, tabs)?;
        let rule = self.pane_local_target_rule(root, tabs, central)?;
        Ok(EdgeTarget {
            root,
            node: tabs,
            fingerprint,
            edge,
            fraction,
            surface,
            central,
            rule,
            scope: EdgeTargetScope::Inner,
        })
    }

    /// Captures one outer edge of a complete docking root.
    ///
    /// The target node is derived from the root record, so callers cannot
    /// relabel an arbitrary branch as the root boundary.
    ///
    /// # Errors
    ///
    /// Returns [`CommandError`] when the root or its topology node is absent or
    /// the root has no authoritative presentation owner.
    pub fn capture_outer_edge_target(
        &self,
        root: RootId,
        edge: Edge,
        fraction: DockFraction,
    ) -> Result<EdgeTarget, CommandError> {
        let node = self
            .roots
            .get(&root)
            .ok_or(CommandError::MissingRoot { root })?
            .node;
        let fingerprint = self.capture_reference(root, node, ReferenceRole::Target)?;
        let (surface, central) = self.target_owner_facts(root, node)?;
        Ok(EdgeTarget {
            root,
            node,
            fingerprint,
            edge,
            fraction,
            surface,
            central,
            rule: DockTargetRuleKey::Root(root),
            scope: EdgeTargetScope::Outer,
        })
    }

    pub(crate) fn pane_local_target_rule(
        &self,
        root: RootId,
        tabs: NodeId,
        central: bool,
    ) -> Result<DockTargetRuleKey, CommandError> {
        match self.nodes.get(tabs) {
            Some(Node::Tabs {
                selected: Some(item),
                ..
            }) => Ok(DockTargetRuleKey::Item(*item)),
            // An empty central leaf is a deliberate docking sink, not an
            // unselected pane. It has no item identity, so root scope is the
            // only stable policy identity available to the core.
            Some(Node::Tabs {
                items,
                selected: None,
            }) if central && items.is_empty() => Ok(DockTargetRuleKey::Root(root)),
            Some(Node::Tabs { selected: None, .. }) => {
                Err(CommandError::TargetTabsUnselected { tabs })
            }
            Some(Node::Split { .. }) => Err(CommandError::NodeIsNotTabs { node: tabs }),
            None => Err(CommandError::MissingNode {
                role: ReferenceRole::Target,
                node: tabs,
            }),
        }
    }

    fn target_owner_facts(
        &self,
        root: RootId,
        node: NodeId,
    ) -> Result<(SurfaceId, bool), CommandError> {
        let central = self
            .roots
            .get(&root)
            .ok_or(CommandError::MissingRoot { root })?
            .central
            == Some(node);
        let surface = match self.presentation_for_root(root) {
            Some(RootPresentationOwner::Main { surface })
            | Some(RootPresentationOwner::Contained { surface, .. }) => surface,
            None => {
                return Err(CommandError::Invariant {
                    stage: "capture target presentation owner",
                });
            }
        };
        Ok((surface, central))
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
        let owner = self
            .presentation_for_root(root)
            .ok_or(CommandError::Invariant {
                stage: "capture root presentation owner",
            })?;
        Ok(self.build_root_index(root, owner)?.fingerprint)
    }

    fn build_root_index(
        &self,
        root: RootId,
        owner: RootPresentationOwner,
    ) -> Result<WorkspaceRootIndex, CommandError> {
        let record = self
            .roots
            .get(&root)
            .ok_or(CommandError::MissingRoot { root })?;
        let presentation = match owner {
            RootPresentationOwner::Main { surface } => FingerprintPresentation::Main { surface },
            RootPresentationOwner::Contained { surface, floating } => {
                FingerprintPresentation::Contained { surface, floating }
            }
        };

        #[cfg(test)]
        crate::drop_resolver::structural_work::record_root_fingerprint_build();

        let mut nodes = Vec::new();
        let mut stack = vec![record.node];
        let mut visited = BTreeSet::new();
        let mut item_count = 0_usize;
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
            #[cfg(test)]
            crate::drop_resolver::structural_work::record_root_fingerprint_node_visit();
            let snapshot = match node {
                Node::Tabs { items, selected } => {
                    item_count =
                        item_count
                            .checked_add(items.len())
                            .ok_or(CommandError::Invariant {
                                stage: "capture root item count",
                            })?;
                    FingerprintNode::Tabs {
                        items: items.clone(),
                        selected: *selected,
                        mru: self
                            .tab_mru
                            .get(&id)
                            .ok_or(CommandError::Invariant {
                                stage: "capture tabs MRU fingerprint",
                            })?
                            .clone(),
                    }
                }
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

        let fingerprint = NodeFingerprint(Arc::new(RootFingerprintData {
            root,
            root_node: record.node,
            central: record.central,
            nodes,
            presentation,
        }));
        Ok(WorkspaceRootIndex {
            root_node: record.node,
            central: record.central,
            owner,
            members: visited,
            item_count,
            fingerprint,
        })
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
            if surface.main_root == Some(root) {
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

#[cfg(test)]
mod tests {
    use super::WorkspaceIndex;
    use crate::command::{DockFraction, Edge};
    use crate::error::{CommandError, ReferenceRole};
    use crate::graph::{Axis, Node, RootRecord, SurfacePresentation, Workspace, WorkspaceBuilder};
    use crate::ids::{ItemId, NodeId, RootId, SurfaceId, WorkspaceEpoch, WorkspaceRevision};
    use crate::policy::DockTargetRuleKey;
    use crate::transition::WorkspaceVersion;

    fn index_version(revision: u64) -> WorkspaceVersion {
        WorkspaceVersion::new(WorkspaceEpoch::new(9), WorkspaceRevision::new(revision))
    }

    fn indexed_split_fixture() -> (Workspace, RootId, NodeId, NodeId, NodeId) {
        let root = RootId::new(1);
        let mut builder = Workspace::builder();
        let left = builder.insert_node(Node::tabs([ItemId::new(1)]));
        let right = builder.insert_node(Node::tabs([ItemId::new(2)]));
        let split = builder.insert_node(
            Node::equal_split(Axis::Horizontal, [left, right])
                .expect("two children form a valid split"),
        );
        builder.set_root(root, RootRecord::new(split).with_central(left));
        builder.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(root));
        (
            builder.build().expect("fixture should validate"),
            root,
            split,
            left,
            right,
        )
    }

    #[test]
    fn workspace_index_rejects_cross_revision_reuse_after_topology_changes() {
        let (workspace, root, split, left, right) = indexed_split_fixture();
        let first_version = index_version(1);
        let next_version = index_version(2);
        let first_index =
            WorkspaceIndex::build(&workspace, first_version).expect("first index should derive");
        let old_target = first_index
            .capture_tab_target(&workspace, first_version, root, left)
            .expect("first target should capture");

        let mut changed = workspace.clone();
        *changed
            .nodes
            .get_mut(split)
            .expect("fixture split should remain live") =
            Node::equal_split(Axis::Vertical, [right, left])
                .expect("changed topology remains valid");
        changed.validate().expect("changed fixture should validate");

        assert!(matches!(
            first_index.capture_tab_target(&changed, next_version, root, left),
            Err(CommandError::Invariant {
                stage: "use workspace index for a different revision"
            })
        ));
        assert!(matches!(
            changed.verify_reference(
                root,
                left,
                &old_target.fingerprint,
                ReferenceRole::Target,
            ),
            Err(CommandError::StaleNode { node, .. }) if node == left
        ));

        let next_index =
            WorkspaceIndex::build(&changed, next_version).expect("replacement index should derive");
        let next_target = next_index
            .capture_tab_target(&changed, next_version, root, left)
            .expect("replacement target should capture");
        assert_ne!(old_target.fingerprint, next_target.fingerprint);
    }

    #[test]
    fn workspace_index_replaces_presentation_owner_facts_atomically() {
        let (workspace, root, _, left, _) = indexed_split_fixture();
        let first_version = index_version(1);
        let next_version = index_version(2);
        let first_index =
            WorkspaceIndex::build(&workspace, first_version).expect("first index should derive");
        let old_target = first_index
            .capture_tab_target(&workspace, first_version, root, left)
            .expect("first target should capture");
        assert_eq!(old_target.surface(), SurfaceId::new(1));

        let mut changed = workspace.clone();
        changed.surfaces.clear();
        changed
            .surfaces
            .insert(SurfaceId::new(2), SurfacePresentation::with_main(root));
        changed.validate().expect("rehome fixture should validate");
        let next_index =
            WorkspaceIndex::build(&changed, next_version).expect("replacement index should derive");
        let next_target = next_index
            .capture_tab_target(&changed, next_version, root, left)
            .expect("replacement target should capture");

        assert_eq!(next_target.surface(), SurfaceId::new(2));
        assert!(matches!(
            changed.verify_reference(
                root,
                left,
                &old_target.fingerprint,
                ReferenceRole::Target,
            ),
            Err(CommandError::StaleNode { node, .. }) if node == left
        ));
    }

    #[test]
    fn workspace_index_builds_every_root_once_and_visits_each_member_once() {
        let mut builder = Workspace::builder();
        for offset in 0_u64..4 {
            let left = builder.insert_node(Node::tabs([ItemId::new(offset * 2 + 1)]));
            let right = builder.insert_node(Node::tabs([ItemId::new(offset * 2 + 2)]));
            let split = builder.insert_node(
                Node::equal_split(Axis::Horizontal, [left, right])
                    .expect("two children form a valid split"),
            );
            let root = RootId::new(offset + 1);
            builder.set_root(root, RootRecord::new(split).with_central(left));
            builder.set_surface(
                SurfaceId::new(offset + 1),
                SurfacePresentation::with_main(root),
            );
        }
        let workspace = builder.build().expect("multi-root fixture should validate");
        let root_count = workspace.roots().count();
        let node_count = workspace.nodes().count();

        crate::drop_resolver::structural_work::reset();
        WorkspaceIndex::build(&workspace, index_version(1))
            .expect("multi-root index should derive");
        let work = crate::drop_resolver::structural_work::snapshot();

        assert_eq!(work.root_fingerprint_builds, root_count);
        assert_eq!(work.root_fingerprint_node_visits, node_count);
    }

    #[test]
    fn indexed_scene_targets_preserve_checked_capture_semantics() {
        let (workspace, root, _, left, _) = indexed_split_fixture();
        let version = index_version(1);
        let index = WorkspaceIndex::build(&workspace, version).expect("index should derive");
        let fraction = DockFraction::new(0.3).expect("fixture fraction should be valid");

        assert_eq!(
            index
                .capture_tab_target(&workspace, version, root, left)
                .expect("indexed tab target should capture"),
            workspace
                .capture_tab_target(root, left)
                .expect("checked tab target should capture"),
        );
        for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
            assert_eq!(
                index
                    .capture_inner_edge_target(&workspace, version, root, left, edge, fraction)
                    .expect("indexed inner target should capture"),
                workspace
                    .capture_inner_edge_target(root, left, edge, fraction)
                    .expect("checked inner target should capture"),
            );
            assert_eq!(
                index
                    .capture_outer_edge_target(&workspace, version, root, edge, fraction)
                    .expect("indexed outer target should capture"),
                workspace
                    .capture_outer_edge_target(root, edge, fraction)
                    .expect("checked outer target should capture"),
            );
        }
    }

    #[test]
    fn pane_local_target_capture_accepts_only_an_empty_central_leaf_without_selection() {
        let root = RootId::new(1);
        let surface = SurfaceId::new(1);
        let mut builder = WorkspaceBuilder::new();
        let tabs = builder.insert_node(Node::Tabs {
            items: vec![ItemId::new(1)],
            selected: None,
        });
        builder.set_root(root, RootRecord::new(tabs));
        builder.set_surface(surface, SurfacePresentation::with_main(root));
        let fraction = DockFraction::new(0.5).expect("test fraction is valid");

        assert!(matches!(
            builder.workspace.capture_tab_target(root, tabs),
            Err(CommandError::TargetTabsUnselected { tabs: actual }) if actual == tabs
        ));
        assert!(matches!(
            builder
                .workspace
                .capture_inner_edge_target(root, tabs, Edge::Left, fraction),
            Err(CommandError::TargetTabsUnselected { tabs: actual }) if actual == tabs
        ));
        assert_eq!(
            builder
                .workspace
                .capture_outer_edge_target(root, Edge::Left, fraction)
                .expect("outer root target does not need a representative item")
                .rule(),
            DockTargetRuleKey::Root(root)
        );

        let empty_central = builder.insert_node(Node::tabs([]));
        builder.set_root(
            root,
            RootRecord::new(empty_central).with_central(empty_central),
        );
        assert_eq!(
            builder
                .workspace
                .capture_tab_target(root, empty_central)
                .expect("empty central leaf is an explicit tab target")
                .rule(),
            DockTargetRuleKey::Root(root)
        );
        assert_eq!(
            builder
                .workspace
                .capture_inner_edge_target(root, empty_central, Edge::Left, fraction)
                .expect("empty central leaf is an explicit edge target")
                .rule(),
            DockTargetRuleKey::Root(root)
        );
    }

    #[test]
    fn item_source_lookup_finds_the_exact_owner_across_multiple_roots() {
        let first_root = RootId::new(1);
        let second_root = RootId::new(2);
        let mut builder = Workspace::builder();
        let first_tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
        let second_tabs = builder.insert_node(Node::tabs([ItemId::new(2), ItemId::new(3)]));
        builder.set_root(first_root, RootRecord::new(first_tabs));
        builder.set_root(second_root, RootRecord::new(second_tabs));
        builder.set_surface(
            SurfaceId::new(1),
            SurfacePresentation::with_main(first_root),
        );
        builder.set_surface(
            SurfaceId::new(2),
            SurfacePresentation::with_main(second_root),
        );
        let workspace = builder.build().expect("test workspace is valid");

        let source = workspace
            .capture_item_source_by_id(ItemId::new(3))
            .expect("source capture is valid")
            .expect("item is present");
        assert_eq!(source.root(), second_root);
        assert_eq!(source.tabs(), second_tabs);
        assert_eq!(source.item(), ItemId::new(3));
        assert_eq!(
            workspace
                .capture_item_source_by_id(ItemId::new(99))
                .expect("a missing item is not an invalid capture"),
            None
        );
    }
}

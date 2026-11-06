//! Exact root issuance and actual generational node placement owned by graph.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Weak};

use slotmap::SparseSecondaryMap;

use super::{Node, Workspace, WorkspaceData};
use crate::command::NodeFingerprint;
use crate::ids::{NodeId, RootId};
use crate::workspace::{ParentLink, RootPresentationOwner};

/// An opaque exact reference issued by one committed root owner.
///
/// Clones of an unchanged workspace share this issuance. Independent builds and
/// actual root edits issue different references, even when values later agree.
#[derive(Clone)]
pub struct RootReference(Arc<RootIssuance>);

struct RootIssuance {
    root: RootId,
    // Only the retained-selection lane consumes this concrete transition.
    selection_from: Option<(Weak<RootIssuance>, NodeId)>,
}

impl RootReference {
    /// Returns the root which issued this reference.
    pub fn root(&self) -> RootId {
        self.0.root
    }
    fn fresh(root: RootId) -> Self {
        Self(Arc::new(RootIssuance {
            root,
            selection_from: None,
        }))
    }

    pub(crate) fn follows_selection(&self, previous: &Self, tabs: NodeId) -> bool {
        self.0
            .selection_from
            .as_ref()
            .is_some_and(|(from, selected_tabs)| {
                *selected_tabs == tabs && Weak::ptr_eq(from, &Arc::downgrade(&previous.0))
            })
    }
}

impl PartialEq for RootReference {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for RootReference {}

impl fmt::Debug for RootReference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RootReference")
            .field("root", &self.root())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct NodePlacement {
    pub(crate) root: RootId,
    pub(crate) parent: Option<ParentLink>,
}

#[derive(Debug, Clone)]
struct RootOwner {
    reference: RootReference,
    presentation: RootPresentationOwner,
}

#[derive(Debug, Clone)]
pub(super) struct WorkspaceAuthority {
    roots: BTreeMap<RootId, RootOwner>,
    placements: SparseSecondaryMap<NodeId, NodePlacement>,
}

impl Default for WorkspaceAuthority {
    fn default() -> Self {
        Self {
            roots: BTreeMap::new(),
            placements: SparseSecondaryMap::new(),
        }
    }
}

/// Cold transition before-values are temporary actual geometry/semantic values,
/// never an alternative authority accepted by command validation.
pub(super) struct ColdRootTransition(Vec<(RootId, Option<NodeFingerprint>)>);

impl Workspace {
    pub(super) fn issue_storage(
        storage: WorkspaceData,
    ) -> Result<Self, crate::error::CommandError> {
        let mut workspace = Self {
            storage,
            authority: WorkspaceAuthority::default(),
        };
        let roots = workspace.storage.roots.keys().copied().collect::<Vec<_>>();
        for root in roots {
            workspace.refresh_root_placement(root)?;
        }
        Ok(workspace)
    }

    pub(crate) fn root_reference(&self, root: RootId) -> Option<&RootReference> {
        self.authority
            .roots
            .get(&root)
            .map(|owner| &owner.reference)
    }

    pub(crate) fn node_root(&self, node: NodeId) -> Option<RootId> {
        self.storage
            .nodes
            .contains_key(node)
            .then(|| {
                self.authority
                    .placements
                    .get(node)
                    .map(|placement| placement.root)
            })
            .flatten()
    }

    pub(crate) fn placement(&self, node: NodeId) -> Option<NodePlacement> {
        self.storage
            .nodes
            .contains_key(node)
            .then(|| self.authority.placements.get(node).copied())
            .flatten()
    }

    pub(crate) fn owned_presentation(&self, root: RootId) -> Option<RootPresentationOwner> {
        self.authority
            .roots
            .get(&root)
            .map(|owner| owner.presentation)
    }

    pub(super) fn forget_placement(&mut self, node: NodeId) {
        self.authority.placements.remove(node);
    }

    pub(super) fn place_node(&mut self, root: RootId, node: NodeId, parent: Option<ParentLink>) {
        if self.storage.nodes.contains_key(node) {
            self.authority
                .placements
                .insert(node, NodePlacement { root, parent });
        }
    }

    pub(super) fn refresh_child_placements(&mut self, parent: NodeId) {
        let Some(placement) = self.placement(parent) else {
            return;
        };
        let Some(Node::Split { children, .. }) = self.storage.nodes.get(parent) else {
            return;
        };
        let children = children.clone();
        for (index, child) in children.into_iter().enumerate() {
            self.place_node(placement.root, child, Some(ParentLink { parent, index }));
        }
    }

    pub(super) fn assign_subtree(
        &mut self,
        root: RootId,
        node: NodeId,
        parent: Option<ParentLink>,
    ) {
        let mut pending = vec![(node, parent)];
        while let Some((node, parent)) = pending.pop() {
            self.place_node(root, node, parent);
            if let Some(Node::Split { children, .. }) = self.storage.nodes.get(node) {
                pending.extend(children.iter().copied().enumerate().map(|(index, child)| {
                    (
                        child,
                        Some(ParentLink {
                            parent: node,
                            index,
                        }),
                    )
                }));
            }
        }
    }

    pub(super) fn unroot_subtree(&mut self, node: NodeId) {
        let mut pending = vec![node];
        while let Some(node) = pending.pop() {
            self.forget_placement(node);
            if let Some(Node::Split { children, .. }) = self.storage.nodes.get(node) {
                pending.extend(children.iter().copied());
            }
        }
    }

    pub(super) fn complete_root_change(
        &mut self,
        root: RootId,
    ) -> Result<(), crate::error::CommandError> {
        let Some(record) = self.storage.roots.get(&root).copied() else {
            self.authority.roots.remove(&root);
            return Ok(());
        };
        let presentation = self
            .owned_presentation(root)
            .or_else(|| self.presentation_in_storage(root))
            .ok_or(crate::error::CommandError::Invariant {
                stage: "complete root presentation authority",
            })?;
        self.place_node(root, record.node, None);
        self.authority.roots.insert(
            root,
            RootOwner {
                reference: RootReference::fresh(root),
                presentation,
            },
        );
        Ok(())
    }

    pub(super) fn complete_selection_change(
        &mut self,
        root: RootId,
        tabs: NodeId,
    ) -> Result<(), crate::error::CommandError> {
        let owner = self
            .authority
            .roots
            .get_mut(&root)
            .ok_or(crate::error::CommandError::MissingRoot { root })?;
        owner.reference = RootReference(Arc::new(RootIssuance {
            root,
            selection_from: Some((Arc::downgrade(&owner.reference.0), tabs)),
        }));
        Ok(())
    }

    fn refresh_root_placement(&mut self, root: RootId) -> Result<(), crate::error::CommandError> {
        let Some(record) = self.storage.roots.get(&root).copied() else {
            self.authority.roots.remove(&root);
            return Ok(());
        };
        self.assign_subtree(root, record.node, None);
        let presentation =
            self.presentation_in_storage(root)
                .ok_or(crate::error::CommandError::Invariant {
                    stage: "refresh root presentation authority",
                })?;
        self.authority
            .roots
            .entry(root)
            .and_modify(|owner| {
                owner.presentation = presentation;
            })
            .or_insert_with(|| RootOwner {
                reference: RootReference::fresh(root),
                presentation,
            });
        Ok(())
    }

    fn actual_root_value(
        &self,
        root: RootId,
    ) -> Result<Option<NodeFingerprint>, crate::error::CommandError> {
        if self.storage.roots.contains_key(&root) {
            self.root_fingerprint(root).map(Some)
        } else {
            Ok(None)
        }
    }

    pub(super) fn begin_cold_roots(
        &self,
        roots: impl IntoIterator<Item = RootId>,
    ) -> Result<ColdRootTransition, crate::error::CommandError> {
        let roots = roots.into_iter().collect::<std::collections::BTreeSet<_>>();
        roots
            .into_iter()
            .map(|root| Ok((root, self.actual_root_value(root)?)))
            .collect::<Result<Vec<_>, _>>()
            .map(ColdRootTransition)
    }

    pub(super) fn begin_cold_completion(
        &self,
    ) -> Result<ColdRootTransition, crate::error::CommandError> {
        self.begin_cold_roots(self.storage.roots.keys().copied())
    }

    pub(super) fn finish_cold_roots(
        &mut self,
        before: ColdRootTransition,
    ) -> Result<(), crate::error::CommandError> {
        for (root, previous) in before.0 {
            if let Some(previous) = &previous {
                for node in &previous.0.nodes {
                    if self
                        .authority
                        .placements
                        .get(node.id)
                        .is_some_and(|placement| placement.root == root)
                    {
                        self.forget_placement(node.id);
                    }
                }
            }
            self.refresh_root_placement(root)?;
            let current = self.actual_root_value(root)?;
            if previous != current {
                self.complete_root_change(root)?;
            }
        }
        Ok(())
    }

    pub(super) fn retain_original_issuance(&mut self, original: &Self) {
        self.authority.roots.clone_from(&original.authority.roots);
    }
}

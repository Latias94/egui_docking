//! Exact root issuance and actual generational node placement owned by graph.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Weak};

use slotmap::SparseSecondaryMap;

use super::root_structure::RootStructure;
use super::{Node, Workspace, WorkspaceData};
use crate::graph::workspace::{ParentLink, RootPresentationOwner};
use crate::ids::{NodeId, RootId};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NodePlacement {
    pub(crate) root: RootId,
    pub(crate) parent: Option<ParentLink>,
}

#[derive(Debug, Clone)]
struct RootOwner {
    reference: RootReference,
    presentation: RootPresentationOwner,
    structure: RootStructureState,
}

#[derive(Debug, Clone)]
enum RootStructureState {
    Ready(Arc<RootStructure>),
    // A node count is only an allocation hint; zero means no previous body.
    Editing { previous_nodes: usize },
}

#[derive(Debug, Clone)]
pub(super) struct WorkspaceAuthority {
    roots: BTreeMap<RootId, RootOwner>,
    placements: Arc<SparseSecondaryMap<NodeId, NodePlacement>>,
}

impl Default for WorkspaceAuthority {
    fn default() -> Self {
        Self {
            roots: BTreeMap::new(),
            placements: Arc::new(SparseSecondaryMap::new()),
        }
    }
}

impl Workspace {
    /// Borrows the current owned body; unfinished private edits have no read view.
    pub(crate) fn root_structure(&self, root: RootId) -> Option<&Arc<RootStructure>> {
        match &self.authority.roots.get(&root)?.structure {
            RootStructureState::Ready(structure) => Some(structure),
            RootStructureState::Editing { .. } => None,
        }
    }

    pub(super) fn begin_root_edit(&mut self, root: RootId) {
        if let Some(owner) = self.authority.roots.get_mut(&root)
            && let RootStructureState::Ready(structure) = &owner.structure
        {
            let previous_nodes = structure.nodes.len();
            owner.structure = RootStructureState::Editing { previous_nodes };
        }
    }

    /// A raw normalizer owns all graph writes until its actual effects return.
    pub(super) fn take_root_structures(&mut self) -> Vec<(RootId, Arc<RootStructure>)> {
        self.authority
            .roots
            .iter_mut()
            .filter_map(|(&root, owner)| {
                match std::mem::replace(
                    &mut owner.structure,
                    RootStructureState::Editing { previous_nodes: 0 },
                ) {
                    RootStructureState::Ready(structure) => Some((root, structure)),
                    RootStructureState::Editing { .. } => None,
                }
            })
            .collect()
    }

    pub(super) fn restore_root_structure(&mut self, root: RootId, structure: Arc<RootStructure>) {
        if let Some(owner) = self.authority.roots.get_mut(&root) {
            owner.structure = RootStructureState::Ready(structure);
        }
    }

    /// Completes one actual writer-owned root without scanning peer owners.
    pub(super) fn complete_root_structure(
        &mut self,
        root: RootId,
    ) -> Result<(), crate::graph::error::CommandError> {
        let Some(owner) = self.authority.roots.get_mut(&root) else {
            return Ok(());
        };
        if matches!(&owner.structure, RootStructureState::Editing { .. }) {
            owner.structure = RootStructureState::Ready(Arc::new(RootStructure::build(
                &self.storage,
                root,
                Arc::make_mut(&mut self.authority.placements),
            )?));
        }
        Ok(())
    }

    /// The checked Item writer has repaired placements before completing rows.
    pub(super) fn complete_item_move_structure(
        &mut self,
        root: RootId,
    ) -> Result<(), crate::graph::error::CommandError> {
        let Some(owner) = self.authority.roots.get_mut(&root) else {
            return Ok(());
        };
        if let RootStructureState::Editing { previous_nodes } = &owner.structure {
            owner.structure = RootStructureState::Ready(Arc::new(
                RootStructure::build_after_item_move(&self.storage, root, *previous_nodes)?,
            ));
        }
        Ok(())
    }

    /// A terminal full normalizer may finish all roots it has just visited.
    pub(super) fn complete_root_structures(
        &mut self,
    ) -> Result<(), crate::graph::error::CommandError> {
        let storage = &self.storage;
        let authority = &mut self.authority;
        for (&root, owner) in &mut authority.roots {
            if matches!(&owner.structure, RootStructureState::Editing { .. }) {
                owner.structure = RootStructureState::Ready(Arc::new(RootStructure::build(
                    storage,
                    root,
                    Arc::make_mut(&mut authority.placements),
                )?));
            }
        }
        Ok(())
    }

    pub(super) fn issue_storage(
        storage: WorkspaceData,
    ) -> Result<Self, crate::graph::error::CommandError> {
        let mut presentations = Vec::with_capacity(storage.roots.len());
        for (&surface, presentation) in &storage.surfaces {
            if let Some(root) = presentation.main_root
                && storage.roots.contains_key(&root)
            {
                presentations.push((root, RootPresentationOwner::Main { surface }));
            }
            for &floating in &presentation.contained {
                if let Some(record) = storage.contained_floatings.get(&floating)
                    && storage.roots.contains_key(&record.root)
                {
                    presentations.push((
                        record.root,
                        RootPresentationOwner::Contained { surface, floating },
                    ));
                }
            }
        }
        // Validated input has one carrier per root. Stable sorting also keeps
        // the former first-carrier diagnostic behavior for internal raw input.
        presentations.sort_by_key(|(root, _)| *root);
        presentations.dedup_by_key(|(root, _)| *root);
        if presentations.len() != storage.roots.len() {
            return Err(crate::graph::error::CommandError::Invariant {
                stage: "refresh root presentation authority",
            });
        }
        let mut workspace = Self {
            storage,
            authority: WorkspaceAuthority::default(),
        };
        for (root, presentation) in presentations {
            workspace.complete_root_birth(root, presentation)?;
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
        if self.authority.placements.contains_key(node) {
            Arc::make_mut(&mut self.authority.placements).remove(node);
        }
    }

    pub(super) fn place_node(&mut self, root: RootId, node: NodeId, parent: Option<ParentLink>) {
        if self.storage.nodes.contains_key(node) {
            let placement = NodePlacement { root, parent };
            if self.authority.placements.get(node) != Some(&placement) {
                Arc::make_mut(&mut self.authority.placements).insert(node, placement);
            }
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
    ) -> Result<(), crate::graph::error::CommandError> {
        let Some(record) = self.storage.roots.get(&root).copied() else {
            self.authority.roots.remove(&root);
            return Ok(());
        };
        let presentation = self
            .owned_presentation(root)
            .or_else(|| self.presentation_in_storage(root))
            .ok_or(crate::graph::error::CommandError::Invariant {
                stage: "complete root presentation authority",
            })?;
        self.place_node(root, record.node, None);
        match self.authority.roots.get_mut(&root) {
            Some(owner) => {
                owner.reference = RootReference::fresh(root);
                owner.presentation = presentation;
            }
            None => {
                self.authority.roots.insert(
                    root,
                    RootOwner {
                        reference: RootReference::fresh(root),
                        presentation,
                        structure: RootStructureState::Editing { previous_nodes: 0 },
                    },
                );
            }
        }
        Ok(())
    }

    pub(super) fn complete_selection_change(
        &mut self,
        root: RootId,
        tabs: NodeId,
    ) -> Result<(), crate::graph::error::CommandError> {
        let owner = self
            .authority
            .roots
            .get_mut(&root)
            .ok_or(crate::graph::error::CommandError::MissingRoot { root })?;
        owner.reference = RootReference(Arc::new(RootIssuance {
            root,
            selection_from: Some((Arc::downgrade(&owner.reference.0), tabs)),
        }));
        Ok(())
    }

    /// Completes a fresh root written by one concrete creation command.
    /// The writer has installed its actual carrier and detached any donor.
    pub(super) fn complete_root_birth(
        &mut self,
        root: RootId,
        presentation: RootPresentationOwner,
    ) -> Result<(), crate::graph::error::CommandError> {
        self.storage
            .roots
            .get(&root)
            .ok_or(crate::graph::error::CommandError::MissingRoot { root })?;
        if self.authority.roots.contains_key(&root) {
            return Err(crate::graph::error::CommandError::Invariant {
                stage: "complete fresh root birth",
            });
        }
        let structure = Arc::new(RootStructure::build(
            &self.storage,
            root,
            Arc::make_mut(&mut self.authority.placements),
        )?);
        self.authority.roots.insert(
            root,
            RootOwner {
                reference: RootReference::fresh(root),
                presentation,
                structure: RootStructureState::Ready(structure),
            },
        );
        Ok(())
    }

    /// Completes the actual carrier written by a checked rehome/promote owner.
    /// Membership is unchanged; a same-carrier no-op preserves exact issuance.
    pub(super) fn complete_root_rehome(
        &mut self,
        root: RootId,
        presentation: RootPresentationOwner,
    ) -> Result<(), crate::graph::error::CommandError> {
        let owner = self
            .authority
            .roots
            .get_mut(&root)
            .ok_or(crate::graph::error::CommandError::MissingRoot { root })?;
        if owner.presentation != presentation {
            owner.presentation = presentation;
            owner.reference = RootReference::fresh(root);
        }
        Ok(())
    }

    pub(super) fn retain_original_issuance(&mut self, original: &Self) {
        for (&root, owner) in &mut self.authority.roots {
            if let Some(previous) = original.authority.roots.get(&root) {
                owner.reference.clone_from(&previous.reference);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::command::{
        DockFraction, DockTarget, Edge, MovePayload, RootPresentationTarget, WorkspaceCommand,
    };
    use crate::graph::error::{CommandError, ReferenceRole};
    use crate::graph::transaction::WorkspaceTransaction;
    use crate::graph::{Axis, RootRecord, SurfacePresentation};
    use crate::ids::{ItemId, SurfaceId};
    use crate::policy::{DockPolicy, PolicyRevision};

    #[test]
    fn semantic_edits_share_placements_and_discarded_structure_edits_detach_them() {
        let root = RootId::new(1);
        let mut builder = Workspace::builder();
        let tabs =
            [[1, 2], [3, 4]].map(|items| builder.insert_node(Node::tabs(items.map(ItemId::new))));
        let split = builder.insert_node(Node::equal_split(Axis::Horizontal, tabs).unwrap());
        builder.set_root(root, RootRecord::new(split));
        builder.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(root));
        let mut workspace = builder.build().unwrap();
        let original = workspace.clone();
        let placements = Arc::clone(&original.authority.placements);
        let frozen = original.capture_node_source(root, split).unwrap();
        let mut policy = DockPolicy::default();
        policy.set_allow_native_surfaces(true);
        let policy = policy.snapshot(PolicyRevision::default());
        let apply_shared = |workspace: &mut Workspace, command| {
            let report = WorkspaceTransaction::from_commands([command])
                .apply(workspace, &policy)
                .unwrap();
            assert!(report.changed());
            assert!(Arc::ptr_eq(&placements, &workspace.authority.placements));
        };

        let command = WorkspaceCommand::Select {
            source: workspace
                .capture_item_source(root, tabs[0], ItemId::new(2))
                .unwrap(),
        };
        apply_shared(&mut workspace, command);
        let command = WorkspaceCommand::Move {
            payload: MovePayload::Item(
                workspace
                    .capture_item_source(root, tabs[0], ItemId::new(2))
                    .unwrap(),
            ),
            target: DockTarget::Center(workspace.capture_tab_target(root, tabs[1]).unwrap()),
        };
        apply_shared(&mut workspace, command);
        let command = WorkspaceCommand::RehomeRoot {
            source: workspace.capture_node_source(root, split).unwrap(),
            target: RootPresentationTarget::NewSurface {
                surface: SurfaceId::new(2),
            },
        };
        apply_shared(&mut workspace, command);
        original
            .verify_reference(root, split, frozen.reference(), ReferenceRole::Source)
            .unwrap();
        assert!(matches!(
            workspace.verify_reference(root, split, frozen.reference(), ReferenceRole::Source),
            Err(CommandError::StaleNode { .. })
        ));
        assert_eq!(
            original.tab_mru(tabs[0]),
            Some([ItemId::new(1), ItemId::new(2)].as_slice())
        );

        let before = workspace.clone();
        let command = WorkspaceCommand::Move {
            payload: MovePayload::Item(
                workspace
                    .capture_item_source(root, tabs[1], ItemId::new(3))
                    .unwrap(),
            ),
            target: DockTarget::InnerEdge(
                workspace
                    .capture_inner_edge_target(
                        root,
                        tabs[0],
                        Edge::Top,
                        DockFraction::new(0.5).unwrap(),
                    )
                    .unwrap(),
            ),
        };
        let prepared =
            crate::graph::operation::prepare_transaction(&workspace, &policy, &[command]).unwrap();
        assert!(!Arc::ptr_eq(
            &placements,
            &prepared.candidate().authority.placements
        ));
        assert_ne!(
            prepared.candidate().parent_link(root, tabs[0]),
            workspace.parent_link(root, tabs[0])
        );
        assert!(Arc::ptr_eq(&placements, &workspace.authority.placements));
        drop(prepared);
        assert_eq!(workspace, before);
        assert_eq!(
            workspace.parent_link(root, tabs[0]),
            Some(ParentLink {
                parent: split,
                index: 0
            })
        );
        assert!(Arc::ptr_eq(&placements, &original.authority.placements));
    }
}

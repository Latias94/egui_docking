//! Iterative validation for durable workspace state.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;

use thiserror::Error;

use crate::graph::{Axis, NORMALIZED_WEIGHT_TOLERANCE, Node, Workspace};
use crate::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};

/// The stable presentation location that owns a root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationLocation {
    /// The root is the main content of a logical surface.
    Main(SurfaceId),
    /// The root is contained-floating inside a logical surface.
    Contained(FloatingPresentationId),
}

/// One invariant violation in a workspace candidate.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum WorkspaceValidationError {
    /// A root record points at a missing runtime node.
    #[error("root {root} references missing node {node:?}")]
    MissingRootNode {
        /// Stable root identity.
        root: RootId,
        /// Missing runtime identity.
        node: NodeId,
    },
    /// A split references a missing child.
    #[error("node {parent:?} references missing child {child:?}")]
    MissingChild {
        /// Referencing split node.
        parent: NodeId,
        /// Missing child node.
        child: NodeId,
    },
    /// A graph edge closes a cycle.
    #[error("workspace graph contains a cycle at node {node:?}")]
    Cycle {
        /// Node encountered while already visiting it.
        node: NodeId,
    },
    /// A node has more than one parent or root owner.
    #[error(
        "workspace graph shares node {node:?} between roots {first_root:?} and {second_root:?}"
    )]
    SharedNode {
        /// Shared runtime node.
        node: NodeId,
        /// First root owner, if the first path was orphaned.
        first_root: Option<RootId>,
        /// Later root owner, if the later path was orphaned.
        second_root: Option<RootId>,
    },
    /// An arena node is not reachable from any root.
    #[error("workspace contains orphan node {node:?}")]
    OrphanNode {
        /// Unreachable runtime node.
        node: NodeId,
    },
    /// A non-central tabs leaf contains no items.
    #[error("non-central tabs node {tabs:?} is empty")]
    EmptyTabs {
        /// Empty tabs node.
        tabs: NodeId,
    },
    /// A tabs selection is absent or not present in its item order.
    #[error("tabs node {tabs:?} has invalid selection {selected:?}")]
    InvalidSelection {
        /// Tabs node with invalid state.
        tabs: NodeId,
        /// Invalid selected item, or `None` when a populated node has no selection.
        selected: Option<ItemId>,
    },
    /// A tabs node has no durable MRU permutation.
    #[error("tabs node {tabs:?} has no MRU permutation")]
    MissingTabMru {
        /// Tabs node without history.
        tabs: NodeId,
    },
    /// A tabs MRU permutation has a different length from its visual item order.
    #[error("tabs node {tabs:?} has {items} items but {mru} MRU entries")]
    TabMruLengthMismatch {
        /// Tabs node with invalid history.
        tabs: NodeId,
        /// Visual item count.
        items: usize,
        /// MRU entry count.
        mru: usize,
    },
    /// A tabs MRU permutation contains the same item more than once.
    #[error("tabs node {tabs:?} repeats item {item} in its MRU permutation")]
    DuplicateTabMruItem {
        /// Tabs node with invalid history.
        tabs: NodeId,
        /// Repeated item identity.
        item: ItemId,
    },
    /// A tabs MRU permutation contains an item outside its visual item order.
    #[error("tabs node {tabs:?} MRU permutation contains foreign item {item}")]
    TabMruItemNotInTabs {
        /// Tabs node with invalid history.
        tabs: NodeId,
        /// Foreign item identity.
        item: ItemId,
    },
    /// A visual tab item is absent from its MRU permutation.
    #[error("tabs node {tabs:?} item {item} is missing from its MRU permutation")]
    TabItemMissingFromMru {
        /// Tabs node with invalid history.
        tabs: NodeId,
        /// Missing item identity.
        item: ItemId,
    },
    /// The selected tab is not the most-recent MRU entry.
    #[error(
        "tabs node {tabs:?} selects {selected:?}, but its most-recent MRU entry is {most_recent:?}"
    )]
    SelectedTabNotMostRecent {
        /// Tabs node with invalid history.
        tabs: NodeId,
        /// Current selected item.
        selected: Option<ItemId>,
        /// First MRU entry.
        most_recent: Option<ItemId>,
    },
    /// Durable MRU state is attached to a missing node or split node.
    #[error("MRU state is attached to non-tabs node {node:?}")]
    UnexpectedTabMru {
        /// Missing or non-tabs runtime node identity.
        node: NodeId,
    },
    /// An item occurs more than once in the workspace forest.
    #[error(
        "item {item} appears in both tabs node {first_tabs:?} and tabs node {duplicate_tabs:?}"
    )]
    DuplicateItem {
        /// Repeated stable item identity.
        item: ItemId,
        /// First tabs node containing the item.
        first_tabs: NodeId,
        /// Later tabs node containing the item.
        duplicate_tabs: NodeId,
    },
    /// A split has fewer than two children.
    #[error("split node {split:?} has only {children} child nodes")]
    SplitTooFewChildren {
        /// Invalid split node.
        split: NodeId,
        /// Actual child count.
        children: usize,
    },
    /// Child and weight cardinalities differ.
    #[error("split node {split:?} has {children} children but {weights} weights")]
    SplitWeightCountMismatch {
        /// Invalid split node.
        split: NodeId,
        /// Child count.
        children: usize,
        /// Weight count.
        weights: usize,
    },
    /// A runtime split weight is not positive and finite.
    #[error("split node {split:?} has invalid weight {value} at index {index}")]
    InvalidSplitWeight {
        /// Invalid split node.
        split: NodeId,
        /// Weight position.
        index: usize,
        /// Invalid scalar value.
        value: f32,
    },
    /// A split's weights do not sum to one.
    #[error("split node {split:?} weights are not normalized; sum is {sum}")]
    SplitWeightsNotNormalized {
        /// Invalid split node.
        split: NodeId,
        /// Accumulated weight sum.
        sum: f64,
    },
    /// A canonical N-ary graph nests a split with the same axis as its parent.
    #[error("split node {child:?} repeats parent {parent:?} axis {axis:?}")]
    SameAxisNesting {
        /// Parent split node.
        parent: NodeId,
        /// Nested split node.
        child: NodeId,
        /// Repeated split direction.
        axis: Axis,
    },
    /// A central node is missing from the arena.
    #[error("root {root} references missing central node {central:?}")]
    MissingCentralNode {
        /// Stable root identity.
        root: RootId,
        /// Missing central runtime node.
        central: NodeId,
    },
    /// A central node does not belong to its root subtree.
    #[error("central node {central:?} is not reachable from root {root}")]
    CentralNotReachable {
        /// Stable root identity.
        root: RootId,
        /// Incorrectly owned central node.
        central: NodeId,
    },
    /// A central region points at a split instead of a tabs leaf.
    #[error("central node {central:?} for root {root} is not a tabs leaf")]
    CentralNotTabs {
        /// Stable root identity.
        root: RootId,
        /// Invalid central runtime node.
        central: NodeId,
    },
    /// A surface's main-root identity is absent from the root map.
    #[error("surface {surface} references missing main root {root}")]
    MissingSurfaceMainRoot {
        /// Logical surface identity.
        surface: SurfaceId,
        /// Missing stable root identity.
        root: RootId,
    },
    /// A logical surface has neither a main root nor a contained presentation.
    #[error("surface {surface} has no main root or contained presentation")]
    EmptySurfacePresentation {
        /// Empty logical surface identity.
        surface: SurfaceId,
    },
    /// A surface roster contains the same floating identity more than once.
    #[error("surface {surface} contains duplicate floating presentation {floating}")]
    DuplicateContainedBacklink {
        /// Logical surface identity.
        surface: SurfaceId,
        /// Repeated floating identity.
        floating: FloatingPresentationId,
    },
    /// A surface roster points at an absent contained-floating record.
    #[error("surface {surface} references missing floating presentation {floating}")]
    MissingContainedFloating {
        /// Logical surface identity.
        surface: SurfaceId,
        /// Missing floating identity.
        floating: FloatingPresentationId,
    },
    /// A contained-floating record references a missing root.
    #[error("floating presentation {floating} references missing root {root}")]
    MissingFloatingRoot {
        /// Floating presentation identity.
        floating: FloatingPresentationId,
        /// Missing stable root identity.
        root: RootId,
    },
    /// A durable contained-floating rectangle has no usable logical area.
    #[error("floating presentation {floating} has non-positive durable bounds {width} x {height}")]
    NonPositiveContainedRect {
        /// Floating presentation identity.
        floating: FloatingPresentationId,
        /// Invalid logical width.
        width: f64,
        /// Invalid logical height.
        height: f64,
    },
    /// A contained identity appears in more than one surface roster.
    #[error(
        "floating presentation {floating} is owned by both surfaces {first_surface} and {duplicate_surface}"
    )]
    ContainedPresentedMoreThanOnce {
        /// Multiply owned contained presentation.
        floating: FloatingPresentationId,
        /// First owning surface.
        first_surface: SurfaceId,
        /// Duplicate owning surface.
        duplicate_surface: SurfaceId,
    },
    /// A contained record is absent from every surface roster.
    #[error("floating presentation {floating} has no surface roster owner")]
    ContainedNotPresented {
        /// Unowned contained presentation.
        floating: FloatingPresentationId,
    },
    /// A root is presented by more than one owner.
    #[error("root {root} is presented by both {first:?} and {duplicate:?}")]
    RootPresentedMoreThanOnce {
        /// Multiply presented stable root identity.
        root: RootId,
        /// First presentation owner.
        first: PresentationLocation,
        /// Duplicate presentation owner.
        duplicate: PresentationLocation,
    },
    /// A durable root has no presentation owner.
    #[error("root {root} has no surface or contained-floating presentation")]
    RootNotPresented {
        /// Invisible stable root identity.
        root: RootId,
    },
}

/// All invariant violations found in one deterministic validation pass.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceValidationErrors {
    errors: Vec<WorkspaceValidationError>,
}

impl WorkspaceValidationErrors {
    /// Creates a collection containing one validation failure.
    pub fn one(error: WorkspaceValidationError) -> Self {
        Self {
            errors: vec![error],
        }
    }

    /// Returns every failure in deterministic discovery order.
    pub fn errors(&self) -> &[WorkspaceValidationError] {
        &self.errors
    }

    /// Consumes the collection.
    pub fn into_errors(self) -> Vec<WorkspaceValidationError> {
        self.errors
    }
}

impl fmt::Display for WorkspaceValidationErrors {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "workspace validation failed with {} error(s)",
            self.errors.len()
        )?;
        if let Some(first) = self.errors.first() {
            write!(formatter, ": {first}")?;
        }
        Ok(())
    }
}

impl Error for WorkspaceValidationErrors {}

impl From<WorkspaceValidationError> for WorkspaceValidationErrors {
    fn from(error: WorkspaceValidationError) -> Self {
        Self::one(error)
    }
}

impl Workspace {
    /// Re-runs every durable topology and presentation invariant.
    ///
    /// # Errors
    ///
    /// Returns all deterministic invariant violations found in the workspace.
    pub fn validate(&self) -> Result<(), WorkspaceValidationErrors> {
        validate_workspace(self)
    }
}

/// Validates a complete workspace without recursion.
///
/// # Errors
///
/// Returns all deterministic invariant violations found in `workspace`.
pub fn validate_workspace(workspace: &Workspace) -> Result<(), WorkspaceValidationErrors> {
    let mut validator = Validator::new(workspace);
    validator.validate_graph();
    validator.validate_central_regions();
    validator.validate_presentations();

    if validator.errors.is_empty() {
        Ok(())
    } else {
        Err(WorkspaceValidationErrors {
            errors: validator.errors,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Visiting,
    Done,
}

#[derive(Debug, Clone, Copy)]
enum Frame {
    Enter {
        node: NodeId,
        parent: Option<NodeId>,
        parent_axis: Option<Axis>,
        owner: Option<RootId>,
    },
    Exit(NodeId),
}

struct Validator<'workspace> {
    workspace: &'workspace Workspace,
    errors: Vec<WorkspaceValidationError>,
    marks: HashMap<NodeId, Mark>,
    owners: HashMap<NodeId, Option<RootId>>,
    items: HashMap<ItemId, NodeId>,
    central_nodes: HashSet<NodeId>,
}

impl<'workspace> Validator<'workspace> {
    fn new(workspace: &'workspace Workspace) -> Self {
        Self {
            workspace,
            errors: Vec::new(),
            marks: HashMap::new(),
            owners: HashMap::new(),
            items: HashMap::new(),
            central_nodes: workspace
                .roots
                .values()
                .filter_map(|root| root.central)
                .collect(),
        }
    }

    fn validate_graph(&mut self) {
        for (&root_id, record) in &self.workspace.roots {
            if self.workspace.nodes.get(record.node).is_none() {
                self.errors.push(WorkspaceValidationError::MissingRootNode {
                    root: root_id,
                    node: record.node,
                });
                continue;
            }
            self.walk(record.node, Some(root_id));
        }

        let orphans: Vec<NodeId> = self
            .workspace
            .nodes
            .keys()
            .filter(|node| !self.marks.contains_key(node))
            .collect();
        self.errors.extend(
            orphans
                .iter()
                .copied()
                .map(|node| WorkspaceValidationError::OrphanNode { node }),
        );

        let orphan_set: HashSet<NodeId> = orphans.iter().copied().collect();
        let mut has_orphan_parent = HashSet::new();
        for orphan in orphans.iter().copied() {
            if let Some(Node::Split { children, .. }) = self.workspace.nodes.get(orphan) {
                has_orphan_parent.extend(
                    children
                        .iter()
                        .copied()
                        .filter(|child| orphan_set.contains(child)),
                );
            }
        }

        for orphan in orphans
            .iter()
            .copied()
            .filter(|node| !has_orphan_parent.contains(node))
        {
            if !self.marks.contains_key(&orphan) {
                self.walk(orphan, None);
            }
        }

        // Components without a zero-in-degree node contain a cycle. Start one
        // deterministic traversal per still-unvisited component to report it.
        for orphan in orphans {
            if !self.marks.contains_key(&orphan) {
                self.walk(orphan, None);
            }
        }

        for &node in self.workspace.tab_mru.keys() {
            if !matches!(self.workspace.nodes.get(node), Some(Node::Tabs { .. })) {
                self.errors
                    .push(WorkspaceValidationError::UnexpectedTabMru { node });
            }
        }
    }

    fn walk(&mut self, root: NodeId, owner: Option<RootId>) {
        let mut stack = vec![Frame::Enter {
            node: root,
            parent: None,
            parent_axis: None,
            owner,
        }];

        while let Some(frame) = stack.pop() {
            match frame {
                Frame::Exit(node) => {
                    self.marks.insert(node, Mark::Done);
                }
                Frame::Enter {
                    node,
                    parent,
                    parent_axis,
                    owner,
                } => self.enter_node(&mut stack, node, parent, parent_axis, owner),
            }
        }
    }

    fn enter_node(
        &mut self,
        stack: &mut Vec<Frame>,
        node_id: NodeId,
        parent: Option<NodeId>,
        parent_axis: Option<Axis>,
        owner: Option<RootId>,
    ) {
        match self.marks.get(&node_id).copied() {
            Some(Mark::Visiting) => {
                self.errors
                    .push(WorkspaceValidationError::Cycle { node: node_id });
                return;
            }
            Some(Mark::Done) => {
                self.errors.push(WorkspaceValidationError::SharedNode {
                    node: node_id,
                    first_root: self.owners.get(&node_id).copied().flatten(),
                    second_root: owner,
                });
                return;
            }
            None => {}
        }

        let Some(node) = self.workspace.nodes.get(node_id).cloned() else {
            if let Some(parent) = parent {
                self.errors.push(WorkspaceValidationError::MissingChild {
                    parent,
                    child: node_id,
                });
            }
            return;
        };

        self.marks.insert(node_id, Mark::Visiting);
        self.owners.insert(node_id, owner);
        stack.push(Frame::Exit(node_id));

        match node {
            Node::Tabs { items, selected } => self.validate_tabs(node_id, &items, selected),
            Node::Split {
                axis,
                children,
                weights,
            } => {
                if let (Some(parent), Some(parent_axis)) = (parent, parent_axis)
                    && parent_axis == axis
                {
                    self.errors.push(WorkspaceValidationError::SameAxisNesting {
                        parent,
                        child: node_id,
                        axis,
                    });
                }
                self.validate_split(node_id, &children, &weights);
                stack.extend(children.into_iter().rev().map(|child| Frame::Enter {
                    node: child,
                    parent: Some(node_id),
                    parent_axis: Some(axis),
                    owner,
                }));
            }
        }
    }

    fn validate_tabs(&mut self, tabs: NodeId, items: &[ItemId], selected: Option<ItemId>) {
        if items.is_empty() {
            if !self.central_nodes.contains(&tabs) {
                self.errors
                    .push(WorkspaceValidationError::EmptyTabs { tabs });
            }
            if selected.is_some() {
                self.errors
                    .push(WorkspaceValidationError::InvalidSelection { tabs, selected });
            }
        } else if selected.is_none() || selected.is_some_and(|item| !items.contains(&item)) {
            self.errors
                .push(WorkspaceValidationError::InvalidSelection { tabs, selected });
        }

        self.validate_tab_mru(tabs, items, selected);

        for item in items {
            if let Some(first_tabs) = self.items.insert(*item, tabs) {
                self.errors.push(WorkspaceValidationError::DuplicateItem {
                    item: *item,
                    first_tabs,
                    duplicate_tabs: tabs,
                });
            }
        }
    }

    fn validate_tab_mru(&mut self, tabs: NodeId, items: &[ItemId], selected: Option<ItemId>) {
        let Some(mru) = self.workspace.tab_mru.get(&tabs) else {
            self.errors
                .push(WorkspaceValidationError::MissingTabMru { tabs });
            return;
        };
        if mru.len() != items.len() {
            self.errors
                .push(WorkspaceValidationError::TabMruLengthMismatch {
                    tabs,
                    items: items.len(),
                    mru: mru.len(),
                });
        }

        let item_set: HashSet<_> = items.iter().copied().collect();
        let mut seen = HashSet::with_capacity(mru.len());
        for &item in mru {
            if !seen.insert(item) {
                self.errors
                    .push(WorkspaceValidationError::DuplicateTabMruItem { tabs, item });
            }
            if !item_set.contains(&item) {
                self.errors
                    .push(WorkspaceValidationError::TabMruItemNotInTabs { tabs, item });
            }
        }
        for &item in items {
            if !seen.contains(&item) {
                self.errors
                    .push(WorkspaceValidationError::TabItemMissingFromMru { tabs, item });
            }
        }

        let most_recent = mru.first().copied();
        if selected != most_recent {
            self.errors
                .push(WorkspaceValidationError::SelectedTabNotMostRecent {
                    tabs,
                    selected,
                    most_recent,
                });
        }
    }

    fn validate_split(
        &mut self,
        split: NodeId,
        children: &[NodeId],
        weights: &[crate::graph::SplitWeight],
    ) {
        if children.len() < 2 {
            self.errors
                .push(WorkspaceValidationError::SplitTooFewChildren {
                    split,
                    children: children.len(),
                });
        }
        if children.len() != weights.len() {
            self.errors
                .push(WorkspaceValidationError::SplitWeightCountMismatch {
                    split,
                    children: children.len(),
                    weights: weights.len(),
                });
        }

        let mut sum = 0.0_f64;
        let mut all_valid = true;
        for (index, weight) in weights.iter().copied().enumerate() {
            let value = weight.get();
            if !value.is_finite() || value <= 0.0 {
                all_valid = false;
                self.errors
                    .push(WorkspaceValidationError::InvalidSplitWeight {
                        split,
                        index,
                        value,
                    });
            } else {
                sum += f64::from(value);
            }
        }
        if all_valid && (sum - 1.0).abs() > NORMALIZED_WEIGHT_TOLERANCE {
            self.errors
                .push(WorkspaceValidationError::SplitWeightsNotNormalized { split, sum });
        }
    }

    fn validate_central_regions(&mut self) {
        for (&root_id, record) in &self.workspace.roots {
            let Some(central) = record.central else {
                continue;
            };
            let Some(node) = self.workspace.nodes.get(central) else {
                self.errors
                    .push(WorkspaceValidationError::MissingCentralNode {
                        root: root_id,
                        central,
                    });
                continue;
            };
            if self.owners.get(&central).copied().flatten() != Some(root_id) {
                self.errors
                    .push(WorkspaceValidationError::CentralNotReachable {
                        root: root_id,
                        central,
                    });
            }
            if !matches!(node, Node::Tabs { .. }) {
                self.errors.push(WorkspaceValidationError::CentralNotTabs {
                    root: root_id,
                    central,
                });
            }
        }
    }

    fn validate_presentations(&mut self) {
        let mut presented: HashMap<RootId, PresentationLocation> = HashMap::new();
        let mut floating_owners: HashMap<FloatingPresentationId, SurfaceId> =
            HashMap::with_capacity(self.workspace.contained_floatings.len());

        for (&surface_id, surface) in &self.workspace.surfaces {
            if surface.main_root.is_none() && surface.contained.is_empty() {
                self.errors
                    .push(WorkspaceValidationError::EmptySurfacePresentation {
                        surface: surface_id,
                    });
            }
            if let Some(main_root) = surface.main_root {
                if self.workspace.roots.contains_key(&main_root) {
                    self.record_presentation(
                        &mut presented,
                        main_root,
                        PresentationLocation::Main(surface_id),
                    );
                } else {
                    self.errors
                        .push(WorkspaceValidationError::MissingSurfaceMainRoot {
                            surface: surface_id,
                            root: main_root,
                        });
                }
            }

            let mut roster = HashSet::new();
            for floating_id in surface.contained.iter().copied() {
                if !roster.insert(floating_id) {
                    self.errors
                        .push(WorkspaceValidationError::DuplicateContainedBacklink {
                            surface: surface_id,
                            floating: floating_id,
                        });
                    continue;
                }
                if let Some(&first_surface) = floating_owners.get(&floating_id) {
                    self.errors
                        .push(WorkspaceValidationError::ContainedPresentedMoreThanOnce {
                            floating: floating_id,
                            first_surface,
                            duplicate_surface: surface_id,
                        });
                } else {
                    floating_owners.insert(floating_id, surface_id);
                }
                let Some(floating) = self.workspace.contained_floatings.get(&floating_id) else {
                    self.errors
                        .push(WorkspaceValidationError::MissingContainedFloating {
                            surface: surface_id,
                            floating: floating_id,
                        });
                    continue;
                };
                if self.workspace.roots.contains_key(&floating.root) {
                    self.record_presentation(
                        &mut presented,
                        floating.root,
                        PresentationLocation::Contained(floating_id),
                    );
                }
            }
        }

        for (&floating_id, floating) in &self.workspace.contained_floatings {
            let width = floating.rect.width();
            let height = floating.rect.height();
            if width <= 0.0 || height <= 0.0 {
                self.errors
                    .push(WorkspaceValidationError::NonPositiveContainedRect {
                        floating: floating_id,
                        width,
                        height,
                    });
            }
            if !floating_owners.contains_key(&floating_id) {
                self.errors
                    .push(WorkspaceValidationError::ContainedNotPresented {
                        floating: floating_id,
                    });
            }
            if !self.workspace.roots.contains_key(&floating.root) {
                self.errors
                    .push(WorkspaceValidationError::MissingFloatingRoot {
                        floating: floating_id,
                        root: floating.root,
                    });
            }
        }

        for root in self.workspace.roots.keys().copied() {
            if !presented.contains_key(&root) {
                self.errors
                    .push(WorkspaceValidationError::RootNotPresented { root });
            }
        }
    }

    fn record_presentation(
        &mut self,
        presented: &mut HashMap<RootId, PresentationLocation>,
        root: RootId,
        location: PresentationLocation,
    ) {
        if let Some(&first) = presented.get(&root) {
            self.errors
                .push(WorkspaceValidationError::RootPresentedMoreThanOnce {
                    root,
                    first,
                    duplicate: location,
                });
        } else {
            presented.insert(root, location);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{RootRecord, SplitWeight, SurfacePresentation, WorkspaceBuilder};

    fn errors(builder: &WorkspaceBuilder) -> Vec<WorkspaceValidationError> {
        builder
            .validate()
            .expect_err("corrupted workspace must be rejected")
            .into_errors()
    }

    #[test]
    fn iterative_validation_finds_cycles_and_orphans() {
        let mut builder = Workspace::builder();
        let orphan = builder.insert_node(Node::tabs([ItemId::new(9)]));
        let left = builder.insert_node(Node::tabs([ItemId::new(1)]));
        let split = builder.insert_node(Node::Split {
            axis: Axis::Horizontal,
            children: vec![left, left],
            weights: vec![SplitWeight(0.5), SplitWeight(0.5)],
        });
        builder
            .replace_node(
                left,
                Node::Split {
                    axis: Axis::Vertical,
                    children: vec![split, orphan],
                    weights: vec![SplitWeight(0.5), SplitWeight(0.5)],
                },
            )
            .expect("test node exists");
        builder.set_root(RootId::new(1), RootRecord::new(split));
        builder.set_surface(
            SurfaceId::new(1),
            SurfacePresentation::with_main(RootId::new(1)),
        );

        let errors = errors(&builder);
        assert!(
            errors
                .iter()
                .any(|error| matches!(error, WorkspaceValidationError::Cycle { .. }))
        );
        assert!(
            errors
                .iter()
                .any(|error| matches!(error, WorkspaceValidationError::SharedNode { .. }))
        );
    }

    #[test]
    fn validation_rejects_non_finite_runtime_weight() {
        let mut builder = Workspace::builder();
        let left = builder.insert_node(Node::tabs([ItemId::new(1)]));
        let right = builder.insert_node(Node::tabs([ItemId::new(2)]));
        let split = builder.insert_node(Node::Split {
            axis: Axis::Horizontal,
            children: vec![left, right],
            weights: vec![SplitWeight(f32::NAN), SplitWeight(0.5)],
        });
        builder.set_root(RootId::new(1), RootRecord::new(split));
        builder.set_surface(
            SurfaceId::new(1),
            SurfacePresentation::with_main(RootId::new(1)),
        );

        assert!(errors(&builder).iter().any(|error| matches!(
            error,
            WorkspaceValidationError::InvalidSplitWeight { index: 0, .. }
        )));
    }

    #[test]
    fn orphan_tree_does_not_report_normal_parent_edges_as_shared() {
        let mut builder = Workspace::builder();
        let live = builder.insert_node(Node::tabs([ItemId::new(1)]));
        let first_orphan = builder.insert_node(Node::tabs([ItemId::new(2)]));
        let second_orphan = builder.insert_node(Node::tabs([ItemId::new(3)]));
        let orphan_split = builder.insert_node(
            Node::equal_split(Axis::Horizontal, [first_orphan, second_orphan])
                .expect("valid orphan split"),
        );
        builder.set_root(RootId::new(1), RootRecord::new(live));
        builder.set_surface(
            SurfaceId::new(1),
            SurfacePresentation::with_main(RootId::new(1)),
        );

        let errors = errors(&builder);
        let orphan_count = errors
            .iter()
            .filter(|error| matches!(error, WorkspaceValidationError::OrphanNode { .. }))
            .count();
        assert_eq!(orphan_count, 3);
        assert!(errors.iter().any(|error| matches!(
            error,
            WorkspaceValidationError::OrphanNode { node } if *node == orphan_split
        )));
        assert!(
            !errors
                .iter()
                .any(|error| matches!(error, WorkspaceValidationError::SharedNode { .. }))
        );
    }
}

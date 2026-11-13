//! Durable, renderer-neutral docking topology.

use std::collections::{BTreeMap, BTreeSet};

use std::sync::Arc;
use thiserror::Error;

use crate::canonical::CanonicalizationError;
use crate::geometry::LogicalRect;
use crate::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use crate::validation::WorkspaceValidationErrors;
use shared_storage::NodeStore;

pub(crate) const NORMALIZED_WEIGHT_TOLERANCE: f64 = 1.0e-5;

pub(crate) fn is_exact_tab_mru(items: &[ItemId], selected: Option<ItemId>, mru: &[ItemId]) -> bool {
    if mru.len() != items.len() || mru.first().copied() != selected {
        return false;
    }
    match (items, mru) {
        ([], []) => true,
        ([item], [most_recent]) => item == most_recent,
        ([first, second], [most_recent, previous]) => {
            first != second
                && ((first == most_recent && second == previous)
                    || (first == previous && second == most_recent))
        }
        _ => {
            let mru_items: BTreeSet<_> = mru.iter().copied().collect();
            let visual_items: BTreeSet<_> = items.iter().copied().collect();
            mru_items.len() == mru.len() && mru_items == visual_items
        }
    }
}

/// Direction in which the children of a split are laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum Axis {
    /// Children are ordered from left to right.
    Horizontal,
    /// Children are ordered from top to bottom.
    Vertical,
}

/// A positive, finite share of a split's available extent.
///
/// A collection of weights is valid only when it is normalized. Use
/// [`SplitWeight::normalize`] when converting arbitrary application values.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct SplitWeight(pub(crate) f32);

impl SplitWeight {
    /// Creates one positive, finite split weight.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidSplitWeight`] when `value` is non-finite, zero, or negative.
    pub fn new(value: f32) -> Result<Self, InvalidSplitWeight> {
        if !value.is_finite() {
            return Err(InvalidSplitWeight::NonFinite { value });
        }
        if value <= 0.0 {
            return Err(InvalidSplitWeight::NonPositive { value });
        }
        Ok(Self(value))
    }

    /// Returns the scalar value represented by this weight.
    pub fn get(self) -> f32 {
        self.0
    }

    /// Converts arbitrary positive values into normalized split weights.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidSplitWeights`] for an empty collection, an invalid scalar, an invalid
    /// accumulated sum, or a value that cannot remain positive when represented as `f32`.
    pub fn normalize(
        values: impl IntoIterator<Item = f32>,
    ) -> Result<Vec<Self>, InvalidSplitWeights> {
        let values: Vec<f32> = values.into_iter().collect();
        if values.is_empty() {
            return Err(InvalidSplitWeights::Empty);
        }

        let mut sum = 0.0_f64;
        for (index, value) in values.iter().copied().enumerate() {
            SplitWeight::new(value)
                .map_err(|source| InvalidSplitWeights::Invalid { index, source })?;
            sum += f64::from(value);
        }
        if !sum.is_finite() || sum <= 0.0 {
            return Err(InvalidSplitWeights::InvalidSum);
        }

        let mut normalized = Vec::with_capacity(values.len());
        for (index, value) in values.into_iter().enumerate() {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "the unit ratio is intentionally stored in the runtime f32 model"
            )]
            let normalized_value = (f64::from(value) / sum) as f32;
            normalized.push(
                SplitWeight::new(normalized_value)
                    .map_err(|source| InvalidSplitWeights::Invalid { index, source })?,
            );
        }

        Ok(normalized)
    }
}

/// Failure to construct one split weight.
#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum InvalidSplitWeight {
    /// The supplied weight is NaN or infinite.
    #[error("split weight must be finite, got {value}")]
    NonFinite {
        /// Rejected scalar value.
        value: f32,
    },
    /// The supplied weight is zero or negative.
    #[error("split weight must be greater than zero, got {value}")]
    NonPositive {
        /// Rejected scalar value.
        value: f32,
    },
}

/// Failure to normalize a collection of split weights.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum InvalidSplitWeights {
    /// At least one value is required.
    #[error("split weights cannot be empty")]
    Empty,
    /// One input value is not a valid runtime weight.
    #[error("invalid split weight at index {index}: {source}")]
    Invalid {
        /// Position of the rejected value.
        index: usize,
        /// Scalar validation failure.
        source: InvalidSplitWeight,
    },
    /// The accumulated weight cannot be normalized.
    #[error("split weight sum is not finite and positive")]
    InvalidSum,
}

/// Runtime node in a docking forest.
#[derive(Debug, PartialEq)]
pub enum Node {
    /// Ordered tabs sharing one content rectangle.
    Tabs {
        /// Items in visual tab order.
        items: Vec<ItemId>,
        /// Active item, or `None` for a permitted empty central leaf.
        selected: Option<ItemId>,
        /// Complete most-recent-first permutation, with the selected item first.
        mru: Vec<ItemId>,
    },
    /// Ordered N-ary split children.
    Split {
        /// Layout direction.
        axis: Axis,
        /// Child nodes in visual order.
        children: Vec<NodeId>,
        /// Normalized child shares, one per child.
        weights: Vec<SplitWeight>,
    },
}

impl Clone for Node {
    fn clone(&self) -> Self {
        #[cfg(test)]
        crate::drop_resolver::structural_work::record_node_payload_clone(self);
        match self {
            Self::Tabs {
                items,
                selected,
                mru,
            } => Self::Tabs {
                items: items.clone(),
                selected: *selected,
                mru: mru.clone(),
            },
            Self::Split {
                axis,
                children,
                weights,
            } => Self::Split {
                axis: *axis,
                children: children.clone(),
                weights: weights.clone(),
            },
        }
    }
}

impl Node {
    /// Creates a tabs node and selects its first item, if present.
    pub fn tabs(items: impl IntoIterator<Item = ItemId>) -> Self {
        let items: Vec<ItemId> = items.into_iter().collect();
        let selected = items.first().copied();
        let mru = initial_tab_mru(&items, selected);
        Self::Tabs {
            items,
            selected,
            mru,
        }
    }

    /// Creates a tabs node with an explicit selection.
    ///
    /// Full selection and duplicate-item checks run when the workspace draft is built.
    pub fn tabs_with_selection(
        items: impl IntoIterator<Item = ItemId>,
        selected: Option<ItemId>,
    ) -> Self {
        let items = items.into_iter().collect::<Vec<_>>();
        let mru = initial_tab_mru(&items, selected);
        Self::Tabs {
            items,
            selected,
            mru,
        }
    }

    /// Creates an N-ary split and normalizes its raw weights.
    ///
    /// # Errors
    ///
    /// Returns [`NodeBuildError`] when there are fewer than two children, cardinalities differ,
    /// or the supplied weights cannot be normalized.
    #[cfg(test)]
    pub fn split(
        axis: Axis,
        children: impl IntoIterator<Item = NodeId>,
        weights: impl IntoIterator<Item = f32>,
    ) -> Result<Self, NodeBuildError> {
        let children: Vec<NodeId> = children.into_iter().collect();
        if children.len() < 2 {
            return Err(NodeBuildError::SplitTooFewChildren {
                children: children.len(),
            });
        }
        let weights = SplitWeight::normalize(weights)?;
        if children.len() != weights.len() {
            return Err(NodeBuildError::SplitWeightCountMismatch {
                children: children.len(),
                weights: weights.len(),
            });
        }
        Ok(Self::Split {
            axis,
            children,
            weights,
        })
    }

    /// Creates an equal-share N-ary split.
    ///
    /// # Errors
    ///
    /// Returns [`NodeBuildError::SplitTooFewChildren`] when fewer than two children are supplied.
    #[cfg(test)]
    pub fn equal_split(
        axis: Axis,
        children: impl IntoIterator<Item = NodeId>,
    ) -> Result<Self, NodeBuildError> {
        let children: Vec<NodeId> = children.into_iter().collect();
        let weights = std::iter::repeat_n(1.0, children.len());
        Self::split(axis, children, weights)
    }
}

/// Failure to construct a node through the checked convenience API.
#[derive(Debug, Clone, PartialEq, Error)]
#[cfg(test)]
pub enum NodeBuildError {
    /// A split must have at least two children.
    #[error("a split requires at least two children, got {children}")]
    SplitTooFewChildren {
        /// Supplied child count.
        children: usize,
    },
    /// A split has a different number of children and weights.
    #[error("split has {children} children but {weights} weights")]
    SplitWeightCountMismatch {
        /// Supplied child count.
        children: usize,
        /// Supplied weight count.
        weights: usize,
    },
    /// Raw weights could not be normalized.
    #[error(transparent)]
    InvalidWeights(#[from] InvalidSplitWeights),
}

/// Durable metadata for one independently presentable dock-space root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RootRecord {
    /// Topology root node.
    pub node: NodeId,
    /// Reachable tabs leaf that receives remaining central space.
    pub central: Option<NodeId>,
}

impl RootRecord {
    /// Creates a root without central-region semantics.
    pub fn new(node: NodeId) -> Self {
        Self {
            node,
            central: None,
        }
    }

    /// Assigns the reachable tabs leaf used as the central region.
    #[must_use]
    pub fn with_central(mut self, central: NodeId) -> Self {
        self.central = Some(central);
        self
    }
}

/// Presentation roster for one logical surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfacePresentation {
    /// Root occupying the surface's main dock area, when present.
    pub main_root: Option<RootId>,
    /// Contained-floating presentation identities owned by this surface in back-to-front order.
    pub contained: Vec<FloatingPresentationId>,
}

impl SurfacePresentation {
    /// Creates a rooted surface with no contained-floating presentations.
    pub fn with_main(main_root: RootId) -> Self {
        Self {
            main_root: Some(main_root),
            contained: Vec::new(),
        }
    }

    /// Creates a rootless surface draft.
    ///
    /// A rootless surface becomes valid only after at least one contained presentation is attached.
    /// Transaction execution may use the empty form as intermediate state, but strict builder and
    /// persistence validation reject it.
    #[cfg(test)]
    pub fn rootless() -> Self {
        Self {
            main_root: None,
            contained: Vec::new(),
        }
    }
}

/// A root presented as an in-surface floating container.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContainedFloating {
    /// Presented dock-space root.
    pub root: RootId,
    /// Durable logical bounds relative to the owning surface.
    ///
    /// A validated workspace requires both dimensions to be strictly positive.
    pub rect: LogicalRect,
}

impl ContainedFloating {
    /// Creates contained-floating presentation metadata for a workspace draft.
    ///
    /// [`WorkspaceBuilder::build`] and [`Workspace::validate`] reject a rectangle
    /// without strictly positive area.
    pub const fn new(root: RootId, rect: LogicalRect) -> Self {
        Self { root, rect }
    }
}

/// Raw graph storage. Only graph-owned completion can publish it as a Workspace.
#[derive(Debug, Clone, Default)]
pub(crate) struct WorkspaceData {
    pub(crate) nodes: NodeStore,
    pub(crate) roots: Arc<BTreeMap<RootId, RootRecord>>,
    pub(crate) surfaces: BTreeMap<SurfaceId, SurfacePresentation>,
    pub(crate) contained_floatings: BTreeMap<FloatingPresentationId, ContainedFloating>,
}

impl PartialEq for WorkspaceData {
    fn eq(&self, other: &Self) -> bool {
        self.roots == other.roots
            && self.surfaces == other.surfaces
            && self.contained_floatings == other.contained_floatings
            && self.nodes.len() == other.nodes.len()
            && self
                .nodes
                .iter()
                .all(|(id, node)| other.nodes.get(id) == Some(node))
    }
}

/// A canonical workspace. Mutable storage is owned by graph's implementation.
#[derive(Debug, Clone, Default)]
pub struct Workspace {
    storage: WorkspaceData,
    authority: authority::WorkspaceAuthority,
}

impl PartialEq for Workspace {
    fn eq(&self, other: &Self) -> bool {
        self.storage == other.storage
    }
}

impl Workspace {
    /// Creates the valid empty workspace.
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts an uncommitted workspace draft.
    #[cfg(any(feature = "serde", test))]
    pub fn builder() -> WorkspaceBuilder {
        WorkspaceBuilder::new()
    }

    /// Converts this workspace into an editable draft, retaining node identities.
    #[cfg(test)]
    pub fn into_builder(self) -> WorkspaceBuilder {
        WorkspaceBuilder {
            workspace: self.storage,
        }
    }

    pub(crate) fn storage(&self) -> &WorkspaceData {
        &self.storage
    }

    /// Returns a node by runtime identity.
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.storage.node(id)
    }

    /// Iterates nodes in deterministic arena order.
    pub fn nodes(&self) -> impl Iterator<Item = (NodeId, &Node)> {
        self.storage.nodes()
    }

    /// Returns the tabs stack's selected-first MRU permutation.
    #[cfg(test)]
    pub fn tab_mru(&self, tabs: NodeId) -> Option<&[ItemId]> {
        self.storage.tab_mru(tabs)
    }

    /// Returns one root record.
    pub fn root(&self, id: RootId) -> Option<&RootRecord> {
        self.storage.root(id)
    }

    /// Iterates roots in stable identity order.
    pub fn roots(&self) -> impl Iterator<Item = (RootId, &RootRecord)> {
        self.storage.roots()
    }

    /// Returns one surface presentation.
    pub fn surface(&self, id: SurfaceId) -> Option<&SurfacePresentation> {
        self.storage.surface(id)
    }

    /// Iterates surfaces in stable identity order.
    pub fn surfaces(&self) -> impl Iterator<Item = (SurfaceId, &SurfacePresentation)> {
        self.storage.surfaces()
    }

    /// Returns one contained presentation.
    pub fn contained_floating(&self, id: FloatingPresentationId) -> Option<&ContainedFloating> {
        self.storage.contained_floating(id)
    }

    /// Iterates contained presentations in stable identity order.
    pub fn contained_floatings(
        &self,
    ) -> impl Iterator<Item = (FloatingPresentationId, &ContainedFloating)> {
        self.storage.contained_floatings()
    }

    /// Counts every owned item occurrence.
    pub fn item_multiset(&self) -> BTreeMap<ItemId, usize> {
        self.storage.item_multiset()
    }

    fn insert_runtime_node(&mut self, node: Node) -> NodeId {
        self.storage.insert_runtime_node(node)
    }

    fn remove_runtime_node(&mut self, id: NodeId) -> Option<Arc<Node>> {
        self.forget_placement(id);
        self.storage.remove_runtime_node(id)
    }
}

impl WorkspaceData {
    /// Returns a node by runtime identity.
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id)
    }

    /// Iterates runtime nodes in deterministic arena order.
    pub fn nodes(&self) -> impl Iterator<Item = (NodeId, &Node)> {
        self.nodes.iter()
    }

    /// Returns one tabs stack's complete most-recent-first item permutation.
    ///
    /// A valid populated tabs node has exactly one occurrence of every visual
    /// item here, with its selected item first. `None` means `tabs` is absent
    /// or is not a tabs node.
    #[cfg(test)]
    pub fn tab_mru(&self, tabs: NodeId) -> Option<&[ItemId]> {
        match self.nodes.get(tabs)? {
            Node::Tabs { mru, .. } => Some(mru),
            Node::Split { .. } => None,
        }
    }

    /// Returns a root record by stable identity.
    pub fn root(&self, id: RootId) -> Option<&RootRecord> {
        self.roots.get(&id)
    }

    /// Iterates roots in stable identity order.
    pub fn roots(&self) -> impl Iterator<Item = (RootId, &RootRecord)> {
        self.roots.iter().map(|(id, root)| (*id, root))
    }

    /// Returns presentation metadata for a logical surface.
    pub fn surface(&self, id: SurfaceId) -> Option<&SurfacePresentation> {
        self.surfaces.get(&id)
    }

    /// Iterates logical surfaces in stable identity order.
    pub fn surfaces(&self) -> impl Iterator<Item = (SurfaceId, &SurfacePresentation)> {
        self.surfaces.iter().map(|(id, surface)| (*id, surface))
    }

    /// Returns one contained-floating presentation.
    pub fn contained_floating(&self, id: FloatingPresentationId) -> Option<&ContainedFloating> {
        self.contained_floatings.get(&id)
    }

    /// Iterates contained-floating presentations in stable identity order.
    pub fn contained_floatings(
        &self,
    ) -> impl Iterator<Item = (FloatingPresentationId, &ContainedFloating)> {
        self.contained_floatings
            .iter()
            .map(|(id, floating)| (*id, floating))
    }

    /// Counts every item occurrence without hiding duplicate corruption.
    pub fn item_multiset(&self) -> BTreeMap<ItemId, usize> {
        let mut items = BTreeMap::new();
        #[cfg(test)]
        let mut item_visits = 0;
        for node in self.nodes.values() {
            if let Node::Tabs {
                items: tab_items, ..
            } = node
            {
                #[cfg(test)]
                {
                    item_visits += tab_items.len();
                }
                for item in tab_items {
                    *items.entry(*item).or_insert(0) += 1;
                }
            }
        }
        #[cfg(test)]
        crate::drop_resolver::structural_work::record_item_multiset_scan(item_visits);
        items
    }

    pub(crate) fn insert_runtime_node(&mut self, node: Node) -> NodeId {
        self.nodes.insert(node)
    }

    pub(crate) fn remove_runtime_node(&mut self, id: NodeId) -> Option<Arc<Node>> {
        self.nodes.remove(id)
    }

    pub(crate) fn validate_candidate(&self) -> Result<(), WorkspaceValidationErrors> {
        crate::validation::validate_workspace_data(self)
    }
}

/// Mutable assembly state for constructing and decoding a workspace.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceBuilder {
    pub(crate) workspace: WorkspaceData,
}

impl WorkspaceBuilder {
    /// Creates an empty workspace draft.
    #[cfg(any(feature = "serde", test))]
    pub fn new() -> Self {
        Self::default()
    }

    /// Read-only raw storage for graph diagnostic fixtures.
    #[cfg(test)]
    pub(crate) fn draft(&self) -> &WorkspaceData {
        &self.workspace
    }

    /// Mutable raw storage; build must still normalize and validate it.
    #[cfg(test)]
    pub(crate) fn draft_mut(&mut self) -> &mut WorkspaceData {
        &mut self.workspace
    }

    /// Inserts a runtime node and returns its generational identity.
    pub fn insert_node(&mut self, node: Node) -> NodeId {
        self.workspace.insert_runtime_node(node)
    }

    /// Replaces an existing node in the draft, including its complete tab history.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceBuildError::MissingNode`] for a stale or absent runtime identity.
    #[cfg(any(feature = "serde", test))]
    pub fn replace_node(&mut self, id: NodeId, node: Node) -> Result<Node, WorkspaceBuildError> {
        let previous = self
            .workspace
            .nodes
            .replace(id, node)
            .ok_or(WorkspaceBuildError::MissingNode { node: id })?;
        Ok(Arc::unwrap_or_clone(previous))
    }

    /// Replaces a tabs stack's complete most-recent-first item permutation.
    ///
    /// This draft API deliberately accepts malformed permutations so callers
    /// can decode untrusted state before one complete validation pass.
    /// [`Self::validate`] and [`Self::build`] reject missing, duplicate, foreign,
    /// or incorrectly ordered entries.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceBuildError`] when `tabs` is absent or is not a tabs node.
    #[cfg(test)]
    pub fn set_tab_mru(
        &mut self,
        tabs: NodeId,
        mru: impl IntoIterator<Item = ItemId>,
    ) -> Result<(), WorkspaceBuildError> {
        let node = self
            .workspace
            .nodes
            .get_mut(tabs)
            .ok_or(WorkspaceBuildError::MissingNode { node: tabs })?;
        let Node::Tabs { mru: history, .. } = node else {
            return Err(WorkspaceBuildError::NodeIsNotTabs { node: tabs });
        };
        *history = mru.into_iter().collect();
        Ok(())
    }

    /// Inserts or replaces a stable root record.
    pub fn set_root(&mut self, id: RootId, root: RootRecord) -> Option<RootRecord> {
        let previous = self.workspace.roots.get(&id).copied();
        if previous != Some(root) {
            Arc::make_mut(&mut self.workspace.roots).insert(id, root);
        }
        previous
    }

    /// Inserts or replaces a logical surface presentation.
    pub fn set_surface(
        &mut self,
        id: SurfaceId,
        surface: SurfacePresentation,
    ) -> Option<SurfacePresentation> {
        self.workspace.surfaces.insert(id, surface)
    }

    /// Inserts or replaces a contained-floating record.
    pub fn set_contained_floating(
        &mut self,
        id: FloatingPresentationId,
        floating: ContainedFloating,
    ) -> Option<ContainedFloating> {
        self.workspace.contained_floatings.insert(id, floating)
    }

    /// Adds one floating identity to its owning surface's roster.
    ///
    /// # Errors
    ///
    /// Returns [`WorkspaceBuildError::MissingSurface`] when the surface is absent.
    #[cfg(test)]
    pub fn attach_contained(
        &mut self,
        surface: SurfaceId,
        floating: FloatingPresentationId,
    ) -> Result<(), WorkspaceBuildError> {
        let presentation = self
            .workspace
            .surfaces
            .get_mut(&surface)
            .ok_or(WorkspaceBuildError::MissingSurface { surface })?;
        presentation.contained.push(floating);
        Ok(())
    }

    /// Canonicalizes and validates the complete draft, then returns it atomically.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalizationError`] when normalization would hide corruption, lose an item,
    /// or produce a workspace that fails strict validation.
    pub fn build(self) -> Result<Workspace, CanonicalizationError> {
        canonical::build_workspace(self.workspace)
    }

    /// Validates the draft without consuming or canonicalizing it.
    ///
    /// # Errors
    ///
    /// Returns every discovered [`crate::validation::WorkspaceValidationError`].
    #[cfg(any(feature = "serde", test))]
    pub fn validate(&self) -> Result<(), WorkspaceValidationErrors> {
        self.workspace.validate_candidate()
    }
}

/// Failure while editing a workspace draft.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[cfg(any(feature = "serde", test))]
pub enum WorkspaceBuildError {
    /// A node replacement referenced a stale or missing runtime identity.
    #[error("workspace draft does not contain node {node:?}")]
    MissingNode {
        /// Missing runtime node identity.
        node: NodeId,
    },
    /// A tabs-only draft edit named a split node.
    #[error("workspace draft node {node:?} is not a tabs node")]
    #[cfg(test)]
    NodeIsNotTabs {
        /// Incorrect runtime node identity.
        node: NodeId,
    },
    /// A contained presentation was attached to a missing surface.
    #[error("workspace draft does not contain surface {surface}")]
    #[cfg(test)]
    MissingSurface {
        /// Missing stable surface identity.
        surface: SurfaceId,
    },
}

fn initial_tab_mru(items: &[ItemId], selected: Option<ItemId>) -> Vec<ItemId> {
    let mut mru = Vec::with_capacity(items.len());
    if let Some(selected) = selected.filter(|selected| items.contains(selected)) {
        mru.push(selected);
    }
    mru.extend(items.iter().copied().filter(|item| Some(*item) != selected));
    mru
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_tab_mru_requires_selected_first_unique_membership() {
        let a = ItemId::new(1);
        let b = ItemId::new(2);
        let c = ItemId::new(3);
        let cases: &[(&[ItemId], Option<ItemId>, &[ItemId], bool)] = &[
            (&[], None, &[], true),
            (&[], Some(a), &[], false),
            (&[], None, &[a], false),
            (&[a], Some(a), &[a], true),
            (&[a], None, &[a], false),
            (&[a], Some(b), &[b], false),
            (&[a], Some(a), &[], false),
            (&[a, b], Some(a), &[a, b], true),
            (&[a, b], Some(b), &[b, a], true),
            (&[a, a], Some(a), &[a, a], false),
            (&[a, a], Some(a), &[a, b], false),
            (&[a, b], Some(a), &[a, a], false),
            (&[a, b], Some(a), &[a, c], false),
            (&[a, b], Some(a), &[b, a], false),
            (&[a, b, c], Some(c), &[c, a, b], true),
            (&[a, b, c], Some(c), &[c, a, a], false),
        ];
        for &(items, selected, mru, expected) in cases {
            assert_eq!(
                is_exact_tab_mru(items, selected, mru),
                expected,
                "items={items:?}, selected={selected:?}, mru={mru:?}"
            );
        }
    }
}

mod authority;
pub(crate) mod canonical;
#[cfg(test)]
mod copy_tests;
pub(crate) mod operation;
pub(crate) mod root_structure;
mod shared_storage;
pub use authority::RootReference;

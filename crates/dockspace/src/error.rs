//! Structured failures for checked workspace commands and transactions.

use std::collections::BTreeMap;

use thiserror::Error;

use crate::canonical::CanonicalizationError;
use crate::command::NodeFingerprint;
use crate::geometry::LogicalRect;
use crate::graph::{Axis, ContainedStackKey};
use crate::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use crate::policy::PolicyRejection;
use crate::validation::WorkspaceValidationErrors;

/// Whether a frozen node reference is used as a source or target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceRole {
    /// Content is detached from this node.
    Source,
    /// Content is inserted relative to this node.
    Target,
}

/// Failure to apply one checked workspace command to a candidate.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum CommandError {
    /// A stable root identity is absent.
    #[error("root {root} does not exist")]
    MissingRoot { root: RootId },
    /// A generational node identity is absent.
    #[error("{role:?} node {node:?} does not exist")]
    MissingNode { role: ReferenceRole, node: NodeId },
    /// A node is live but belongs to another root.
    #[error("{role:?} node {node:?} is not contained by root {root}")]
    NodeOutsideRoot {
        role: ReferenceRole,
        root: RootId,
        node: NodeId,
    },
    /// A frozen reference no longer describes the same subtree.
    #[error("{role:?} node {node:?} is stale: expected fingerprint {expected:?}, got {actual:?}")]
    StaleNode {
        role: ReferenceRole,
        node: NodeId,
        expected: NodeFingerprint,
        actual: NodeFingerprint,
    },
    /// A command expected a tabs leaf.
    #[error("node {node:?} is not a tabs leaf")]
    NodeIsNotTabs { node: NodeId },
    /// A command expected a split.
    #[error("node {node:?} is not a split")]
    NodeIsNotSplit { node: NodeId },
    /// A tabs node does not contain the requested item.
    #[error("tabs node {tabs:?} does not contain item {item}")]
    ItemNotInTabs { tabs: NodeId, item: ItemId },
    /// Opening an item would duplicate application ownership.
    #[error("item {item} is already open")]
    ItemAlreadyOpen { item: ItemId },
    /// A tab gap index is outside `0..=len`.
    #[error("tab insertion index {index} exceeds tabs {tabs:?} length {len}")]
    TabIndexOutOfBounds {
        tabs: NodeId,
        index: usize,
        len: usize,
    },
    /// A subtree payload cannot be merged into a tabs target.
    #[error("split subtree {node:?} cannot be inserted into a tabs stack")]
    SplitPayloadIntoTabs { node: NodeId },
    /// A tabs or subtree move contains no application-owned item.
    #[error("move payload rooted at node {node:?} contains no items")]
    EmptyPayload { node: NodeId },
    /// A subtree was targeted at itself or one of its descendants.
    #[error("target node {target:?} is inside moved subtree {source_node:?}")]
    TargetInsidePayload { source_node: NodeId, target: NodeId },
    /// Moving part of a root would detach its declared central leaf.
    #[error("moving node {source_node:?} would detach central leaf {central:?} from root {root}")]
    CentralNodeWouldDetach {
        root: RootId,
        source_node: NodeId,
        central: NodeId,
    },
    /// A split resize has the wrong number of weights.
    #[error("split {split:?} has {children} children but received {weights} weights")]
    SplitWeightCountMismatch {
        split: NodeId,
        children: usize,
        weights: usize,
    },
    /// Caller-supplied resize weights are not normalized.
    #[error("split {split:?} weights must sum to one, got {sum}")]
    SplitWeightsNotNormalized { split: NodeId, sum: f64 },
    /// A stable root identity is already owned.
    #[error("root identity {root} already exists")]
    RootIdCollision { root: RootId },
    /// A logical surface identity is already owned.
    #[error("surface identity {surface} already exists")]
    SurfaceIdCollision { surface: SurfaceId },
    /// A contained presentation identity is already owned.
    #[error("floating presentation identity {floating} already exists")]
    FloatingIdCollision { floating: FloatingPresentationId },
    /// A logical surface identity is absent.
    #[error("surface {surface} does not exist")]
    MissingSurface { surface: SurfaceId },
    /// A contained presentation identity is absent.
    #[error("floating presentation {floating} does not exist")]
    MissingFloating { floating: FloatingPresentationId },
    /// Stable presentation identities do not match the stored record.
    #[error(
        "floating presentation {floating} belongs to root {actual_root} on surface {actual_surface}, not root {expected_root} on surface {expected_surface}"
    )]
    FloatingPresentationMismatch {
        floating: FloatingPresentationId,
        expected_root: RootId,
        expected_surface: SurfaceId,
        actual_root: RootId,
        actual_surface: SurfaceId,
    },
    /// The stored contained rectangle differs from the command precondition.
    #[error(
        "floating presentation {floating} has stale rectangle: expected {expected:?}, got {actual:?}"
    )]
    StaleContainedRect {
        floating: FloatingPresentationId,
        expected: LogicalRect,
        actual: LogicalRect,
    },
    /// A main root cannot be removed while its surface still hosts contained roots.
    #[error("surface {surface} still owns contained-floating presentations")]
    SurfaceHasContainedRoots { surface: SurfaceId },
    /// Moving content would remove the main root of its contained destination surface.
    #[error(
        "moving content from main root {root} would remove contained destination surface {surface}"
    )]
    ContainedHostWouldBeRemoved { surface: SurfaceId, root: RootId },
    /// Removing an explicitly empty root found application-owned items.
    #[error("root {root} still owns {items} item(s)")]
    RootNotEmpty { root: RootId, items: usize },
    /// Closing a complete root found no application-owned item.
    #[error("root {root} is empty; use RemoveEmptyRoot")]
    RootEmpty { root: RootId },
    /// A root-level command was given an inner node instead of the root node.
    #[error("node {node:?} is not the topology root node of root {root}")]
    NodeIsNotRoot { root: RootId, node: NodeId },
    /// Creating a new root cannot silently replace the stable identity of a complete root.
    #[error("moving complete root {root} requires RehomeRoot")]
    WholeRootRequiresRehome { root: RootId },
    /// Moving between contained hosts must preserve the presentation identity.
    #[error(
        "contained root {root} must preserve floating presentation {existing}, not {requested}"
    )]
    FloatingIdentityWouldChange {
        root: RootId,
        existing: FloatingPresentationId,
        requested: FloatingPresentationId,
    },
    /// Contained geometry changes require their dedicated exact-precondition commands.
    #[error(
        "contained presentation {floating} is already the owner; use exact rectangle or stacking commands to change its metadata"
    )]
    RehomeMetadataRequiresDedicatedCommand { floating: FloatingPresentationId },
    /// The stored contained z-order differs from the command precondition.
    #[error(
        "floating presentation {floating} has stale z-order: expected {expected}, got {actual}"
    )]
    StaleZOrder {
        floating: FloatingPresentationId,
        expected: u64,
        actual: u64,
    },
    /// The surface frontmost presentation differs from the command precondition.
    #[error(
        "surface {surface} has stale frontmost presentation: expected {expected:?}, got {actual:?}"
    )]
    StaleContainedStack {
        surface: SurfaceId,
        expected: ContainedStackKey,
        actual: ContainedStackKey,
    },
    /// Raising a contained presentation would overflow its explicit order.
    #[error("cannot raise floating presentation {floating}: maximum z-order is already u64::MAX")]
    ZOrderOverflow { floating: FloatingPresentationId },
    /// Application policy rejects the mutation class.
    #[error(transparent)]
    Policy(#[from] PolicyRejection),
    /// A checked mutation encountered an impossible post-validation condition.
    #[error("workspace command invariant failed during {stage}")]
    Invariant { stage: &'static str },
    /// An edge operation could not preserve a valid split weight.
    #[error("edge insertion on axis {axis:?} produced an invalid split weight")]
    InvalidEdgeWeight { axis: Axis },
}

impl CommandError {
    pub(crate) const fn is_expected_rejection(&self) -> bool {
        match self {
            Self::Invariant { .. } => false,
            Self::MissingRoot { .. }
            | Self::MissingNode { .. }
            | Self::NodeOutsideRoot { .. }
            | Self::StaleNode { .. }
            | Self::NodeIsNotTabs { .. }
            | Self::NodeIsNotSplit { .. }
            | Self::ItemNotInTabs { .. }
            | Self::ItemAlreadyOpen { .. }
            | Self::TabIndexOutOfBounds { .. }
            | Self::SplitPayloadIntoTabs { .. }
            | Self::EmptyPayload { .. }
            | Self::TargetInsidePayload { .. }
            | Self::CentralNodeWouldDetach { .. }
            | Self::SplitWeightCountMismatch { .. }
            | Self::SplitWeightsNotNormalized { .. }
            | Self::RootIdCollision { .. }
            | Self::SurfaceIdCollision { .. }
            | Self::FloatingIdCollision { .. }
            | Self::MissingSurface { .. }
            | Self::MissingFloating { .. }
            | Self::FloatingPresentationMismatch { .. }
            | Self::StaleContainedRect { .. }
            | Self::SurfaceHasContainedRoots { .. }
            | Self::ContainedHostWouldBeRemoved { .. }
            | Self::RootNotEmpty { .. }
            | Self::RootEmpty { .. }
            | Self::NodeIsNotRoot { .. }
            | Self::WholeRootRequiresRehome { .. }
            | Self::FloatingIdentityWouldChange { .. }
            | Self::RehomeMetadataRequiresDedicatedCommand { .. }
            | Self::StaleZOrder { .. }
            | Self::StaleContainedStack { .. }
            | Self::ZOrderOverflow { .. }
            | Self::Policy(_)
            | Self::InvalidEdgeWeight { .. } => true,
        }
    }
}

/// Failure to prepare or publish an atomic command batch.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum TransactionError {
    /// One sequenced command failed; no candidate is published.
    #[error("workspace command {index} failed: {source}")]
    Command {
        /// Zero-based command index.
        index: usize,
        /// Typed command failure.
        source: CommandError,
    },
    /// Candidate canonicalization failed.
    #[error("candidate canonicalization failed: {0}")]
    Canonicalization(#[from] CanonicalizationError),
    /// Strict validation failed after canonicalization.
    #[error("candidate validation failed: {0}")]
    Validation(#[from] WorkspaceValidationErrors),
    /// The final application-item multiset differs from the command-declared delta.
    #[error("transaction item reconciliation failed")]
    ItemReconciliation {
        /// Command after which reconciliation failed, or `None` for the final
        /// canonical candidate check.
        command_index: Option<usize>,
        /// Multiset obtained by applying declared open/close deltas.
        expected: BTreeMap<ItemId, usize>,
        /// Multiset found in the final canonical candidate.
        actual: BTreeMap<ItemId, usize>,
    },
}

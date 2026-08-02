//! Structured failures for checked workspace commands and transactions.

use std::collections::BTreeMap;

use thiserror::Error;

use crate::canonical::CanonicalizationError;
use crate::command::{EdgeTargetScope, NodeFingerprint};
use crate::geometry::LogicalRect;
use crate::graph::Axis;
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
    /// A pane-local target has no selected item to represent its semantic scope.
    #[error("tabs node {tabs:?} has no selected item for a pane-local docking target")]
    TargetTabsUnselected { tabs: NodeId },
    /// An edge proof was wrapped in a command with a different semantic extent.
    #[error("edge target {node:?} was captured as {captured:?} but used as {requested:?}")]
    EdgeTargetScopeMismatch {
        node: NodeId,
        captured: EdgeTargetScope,
        requested: EdgeTargetScope,
    },
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
    /// A resize command contained no split updates.
    #[error("split resize batch must contain at least one update")]
    EmptySplitResizeBatch,
    /// A resize command named the same split more than once.
    #[error("split resize batch contains duplicate split {split:?}")]
    DuplicateSplitResize { split: NodeId },
    /// Caller-supplied resize weights are not normalized.
    #[error("split {split:?} weights must sum to one, got {sum}")]
    SplitWeightsNotNormalized { split: NodeId, sum: f64 },
    /// A stable root identity is already owned.
    #[error("root identity {root} already exists")]
    RootIdCollision { root: RootId },
    /// A root identity was retired by this engine lineage and cannot be recreated.
    #[error("root identity {root} is retired at frontier {frontier}")]
    RetiredRootId { root: RootId, frontier: u64 },
    /// A logical surface identity is already owned.
    #[error("surface identity {surface} already exists")]
    SurfaceIdCollision { surface: SurfaceId },
    /// A surface identity was retired by this engine lineage and cannot be recreated.
    #[error("surface identity {surface} is retired at frontier {frontier}")]
    RetiredSurfaceId { surface: SurfaceId, frontier: u64 },
    /// An operation requires a rootless target but the surface already has a main root.
    #[error("surface {surface} already has main root {root}")]
    SurfaceMainOccupied { surface: SurfaceId, root: RootId },
    /// A contained presentation identity is already owned.
    #[error("floating presentation identity {floating} already exists")]
    FloatingIdCollision { floating: FloatingPresentationId },
    /// A contained presentation identity was retired by this engine lineage.
    #[error("floating presentation identity {floating} is retired at frontier {frontier}")]
    RetiredFloatingId {
        floating: FloatingPresentationId,
        frontier: u64,
    },
    /// A logical surface identity is absent.
    #[error("surface {surface} does not exist")]
    MissingSurface { surface: SurfaceId },
    /// A native lifecycle decision froze the exact presentation roster.
    #[error("surface {surface} presentation roster is frozen by native lifecycle recovery")]
    SurfaceLifecycleFrozen { surface: SurfaceId },
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
    /// Same-surface contained-to-main movement requires the explicit promotion command.
    #[error(
        "floating presentation {floating} on surface {surface} requires PromoteContained to become main"
    )]
    ContainedPromotionRequiresDedicatedCommand {
        surface: SurfaceId,
        floating: FloatingPresentationId,
    },
    /// A structural position cannot anchor a presentation relative to itself.
    #[error("floating presentation {floating} cannot use itself as a roster anchor")]
    ContainedAnchorIsSelf { floating: FloatingPresentationId },
    /// A structural position references an absent contained identity.
    #[error("surface {surface} does not contain anchor presentation {anchor}")]
    MissingContainedAnchor {
        surface: SurfaceId,
        anchor: FloatingPresentationId,
    },
    /// A structural position references an identity owned by another surface.
    #[error("floating anchor {anchor} belongs to surface {actual_surface}, not {expected_surface}")]
    ContainedAnchorOnDifferentSurface {
        anchor: FloatingPresentationId,
        expected_surface: SurfaceId,
        actual_surface: SurfaceId,
    },
    /// A complete contained roster changed after capture.
    #[error("surface {surface} contained roster changed after capture")]
    StaleContainedRoster {
        surface: SurfaceId,
        expected: Vec<FloatingPresentationId>,
        actual: Vec<FloatingPresentationId>,
    },
    /// A contained roster capture belongs to a different surface.
    #[error("contained roster capture belongs to surface {captured_surface}, not {actual_surface}")]
    ContainedRosterSurfaceMismatch {
        captured_surface: SurfaceId,
        actual_surface: SurfaceId,
    },
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
            | Self::TargetTabsUnselected { .. }
            | Self::EdgeTargetScopeMismatch { .. }
            | Self::NodeIsNotSplit { .. }
            | Self::ItemNotInTabs { .. }
            | Self::ItemAlreadyOpen { .. }
            | Self::TabIndexOutOfBounds { .. }
            | Self::SplitPayloadIntoTabs { .. }
            | Self::EmptyPayload { .. }
            | Self::TargetInsidePayload { .. }
            | Self::CentralNodeWouldDetach { .. }
            | Self::SplitWeightCountMismatch { .. }
            | Self::EmptySplitResizeBatch
            | Self::DuplicateSplitResize { .. }
            | Self::SplitWeightsNotNormalized { .. }
            | Self::RootIdCollision { .. }
            | Self::RetiredRootId { .. }
            | Self::SurfaceIdCollision { .. }
            | Self::RetiredSurfaceId { .. }
            | Self::SurfaceMainOccupied { .. }
            | Self::FloatingIdCollision { .. }
            | Self::RetiredFloatingId { .. }
            | Self::MissingSurface { .. }
            | Self::SurfaceLifecycleFrozen { .. }
            | Self::MissingFloating { .. }
            | Self::FloatingPresentationMismatch { .. }
            | Self::StaleContainedRect { .. }
            | Self::RootNotEmpty { .. }
            | Self::RootEmpty { .. }
            | Self::NodeIsNotRoot { .. }
            | Self::WholeRootRequiresRehome { .. }
            | Self::FloatingIdentityWouldChange { .. }
            | Self::RehomeMetadataRequiresDedicatedCommand { .. }
            | Self::ContainedPromotionRequiresDedicatedCommand { .. }
            | Self::ContainedAnchorIsSelf { .. }
            | Self::MissingContainedAnchor { .. }
            | Self::ContainedAnchorOnDifferentSurface { .. }
            | Self::StaleContainedRoster { .. }
            | Self::ContainedRosterSurfaceMismatch { .. }
            | Self::Policy(_)
            | Self::InvalidEdgeWeight { .. } => true,
        }
    }
}

/// Failure to prepare or publish an atomic command batch.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum TransactionError {
    /// One transaction-wide precondition failed before any command was staged.
    #[error("workspace precondition {index} failed: {source}")]
    Precondition {
        /// Zero-based precondition index.
        index: usize,
        /// Typed precondition failure.
        source: TransactionPreconditionError,
    },
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

impl TransactionError {
    pub(crate) fn is_expected_rejection(&self) -> bool {
        match self {
            Self::Precondition { .. } => true,
            Self::Command { source, .. } => source.is_expected_rejection(),
            Self::Canonicalization(_) | Self::Validation(_) | Self::ItemReconciliation { .. } => {
                false
            }
        }
    }
}

/// Failure to prove a transaction-wide source snapshot at apply time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TransactionPreconditionError {
    /// A surface no longer matches its frozen optional-main, roster, roots, or geometry.
    #[error("surface {surface} no longer matches its frozen complete roster")]
    StaleSurfaceRoster {
        /// Surface whose complete roster changed.
        surface: SurfaceId,
    },
}

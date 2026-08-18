//! Typed commands and topology references for durable workspace mutation.

use std::fmt;
use std::sync::Arc;

use crate::geometry::LogicalRect;
use crate::graph::{Axis, SplitWeight};
use crate::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use crate::policy::DockTargetRuleKey;

/// Programmatic request to close one item or one complete root.
///
/// The target carries only stable identity. The core captures all graph,
/// presentation, and policy facts when it accepts the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContentCloseTarget {
    /// Close one item in its current tabs node.
    Item(ItemId),
    /// Close every item in one complete root.
    Root(RootId),
}

/// Durable result of an approved content-close transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseCommitOutcome {
    /// One item was removed from its root.
    ItemClosed { item: ItemId, root: RootId },
    /// One complete root and all of its items were removed atomically.
    RootClosed { root: RootId, items: Vec<ItemId> },
    /// One complete surface roster and all of its items were removed atomically.
    SurfaceClosed {
        surface: SurfaceId,
        items: Vec<ItemId>,
    },
}

/// Opaque, collision-free snapshot of a root and its presentation owner.
///
/// Captured references compare the complete structural snapshot rather than a
/// hash. Cloning is cheap because the immutable records use shared ownership.
#[derive(Clone, PartialEq, Eq)]
pub struct NodeFingerprint(pub(crate) Arc<RootFingerprintData>);

impl NodeFingerprint {
    /// Returns the stable root identity captured by this snapshot.
    pub fn root(&self) -> RootId {
        self.0.root
    }

    /// Compares durable topology and tab membership while deliberately ignoring
    /// selection and MRU state.
    ///
    /// Pointer-journal press handling may select a tab before a later edge from
    /// the same actually presented output resolves a drop. That mutation must
    /// refresh command preconditions without authorizing a changed graph, tab
    /// order, split ratio, central node, or presentation owner.
    pub(crate) fn presentation_structure_eq(&self, other: &Self) -> bool {
        self.0.root == other.0.root
            && self.0.root_node == other.0.root_node
            && self.0.central == other.0.central
            && self.0.presentation == other.0.presentation
            && self.0.nodes.len() == other.0.nodes.len()
            && self
                .0
                .nodes
                .iter()
                .zip(&other.0.nodes)
                .all(|(left, right)| {
                    left.id == right.id
                        && match (&left.node, &right.node) {
                            (
                                FingerprintNode::Tabs { items: left, .. },
                                FingerprintNode::Tabs { items: right, .. },
                            ) => left == right,
                            (
                                FingerprintNode::Split {
                                    axis: left_axis,
                                    children: left_children,
                                    weight_bits: left_weights,
                                },
                                FingerprintNode::Split {
                                    axis: right_axis,
                                    children: right_children,
                                    weight_bits: right_weights,
                                },
                            ) => {
                                left_axis == right_axis
                                    && left_children == right_children
                                    && left_weights == right_weights
                            }
                            (FingerprintNode::Tabs { .. }, FingerprintNode::Split { .. })
                            | (FingerprintNode::Split { .. }, FingerprintNode::Tabs { .. }) => {
                                false
                            }
                        }
                })
    }
}

impl fmt::Debug for NodeFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NodeFingerprint")
            .field("root", &self.0.root)
            .field("nodes", &self.0.nodes.len())
            .field("presentation", &self.0.presentation)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RootFingerprintData {
    pub(crate) root: RootId,
    pub(crate) root_node: NodeId,
    pub(crate) central: Option<NodeId>,
    pub(crate) nodes: Vec<NodeFingerprintRecord>,
    pub(crate) presentation: FingerprintPresentation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NodeFingerprintRecord {
    pub(crate) id: NodeId,
    pub(crate) node: FingerprintNode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FingerprintNode {
    Tabs {
        items: Vec<ItemId>,
        selected: Option<ItemId>,
        mru: Vec<ItemId>,
    },
    Split {
        axis: Axis,
        children: Vec<NodeId>,
        weight_bits: Vec<u32>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FingerprintPresentation {
    Main {
        surface: SurfaceId,
    },
    Contained {
        surface: SurfaceId,
        floating: FloatingPresentationId,
    },
}

/// A source tabs item frozen against one workspace state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemSource {
    pub(crate) root: RootId,
    pub(crate) tabs: NodeId,
    pub(crate) item: ItemId,
    pub(crate) fingerprint: NodeFingerprint,
}

impl ItemSource {
    /// Returns the source root.
    pub const fn root(&self) -> RootId {
        self.root
    }

    /// Returns the source tabs node.
    pub const fn tabs(&self) -> NodeId {
        self.tabs
    }

    /// Returns the source item.
    pub const fn item(&self) -> ItemId {
        self.item
    }

    /// Returns the source subtree fingerprint.
    pub const fn fingerprint(&self) -> &NodeFingerprint {
        &self.fingerprint
    }
}

/// A source node frozen against one workspace state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSource {
    pub(crate) root: RootId,
    pub(crate) node: NodeId,
    pub(crate) fingerprint: NodeFingerprint,
}

/// One exact split-weight replacement inside an atomic resize batch.
#[derive(Debug, Clone, PartialEq)]
pub struct SplitResize {
    split: NodeSource,
    weights: Vec<SplitWeight>,
}

impl SplitResize {
    /// Creates one resize update from a frozen split source and normalized weights.
    ///
    /// Validation against the current workspace is deferred to command execution.
    #[must_use]
    pub fn new(split: NodeSource, weights: Vec<SplitWeight>) -> Self {
        Self { split, weights }
    }

    /// Returns the frozen split source.
    #[must_use]
    pub const fn split(&self) -> &NodeSource {
        &self.split
    }

    /// Returns the complete proposed split weights.
    #[must_use]
    pub fn weights(&self) -> &[SplitWeight] {
        &self.weights
    }
}

impl NodeSource {
    /// Returns the source root.
    pub const fn root(&self) -> RootId {
        self.root
    }

    /// Returns the source node.
    pub const fn node(&self) -> NodeId {
        self.node
    }

    /// Returns the source subtree fingerprint.
    pub const fn fingerprint(&self) -> &NodeFingerprint {
        &self.fingerprint
    }
}

/// Exact source facts for one contained root in a frozen surface roster.
#[derive(Debug, Clone, PartialEq)]
pub struct ContainedRootSource {
    floating: FloatingPresentationId,
    source: NodeSource,
    rect: LogicalRect,
}

impl ContainedRootSource {
    pub(crate) const fn new(
        floating: FloatingPresentationId,
        source: NodeSource,
        rect: LogicalRect,
    ) -> Self {
        Self {
            floating,
            source,
            rect,
        }
    }

    /// Returns the stable contained-presentation identity.
    #[must_use]
    pub const fn floating(&self) -> FloatingPresentationId {
        self.floating
    }

    /// Returns the stable root presented by the contained window.
    #[must_use]
    pub fn root(&self) -> RootId {
        self.source.root()
    }

    /// Returns the source-local rectangle frozen with the roster.
    #[must_use]
    pub const fn rect(&self) -> LogicalRect {
        self.rect
    }

    pub(crate) const fn source(&self) -> &NodeSource {
        &self.source
    }
}

/// Exact optional-main plus ordered contained ownership frozen from one surface.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SurfaceRosterSource {
    surface: SurfaceId,
    main: Option<NodeSource>,
    contained: Arc<[ContainedRootSource]>,
}

impl SurfaceRosterSource {
    pub(crate) fn new(
        surface: SurfaceId,
        main: Option<NodeSource>,
        contained: Vec<ContainedRootSource>,
    ) -> Self {
        Self {
            surface,
            main,
            contained: Arc::from(contained),
        }
    }

    /// Returns the surface whose complete roster was frozen.
    #[must_use]
    pub(crate) const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Returns the stable main root frozen for the surface, when present.
    #[must_use]
    pub(crate) fn main_root(&self) -> Option<RootId> {
        self.main.as_ref().map(NodeSource::root)
    }

    /// Returns contained roots in normative back-to-front order.
    #[must_use]
    pub(crate) fn contained(&self) -> &[ContainedRootSource] {
        &self.contained
    }

    pub(crate) const fn main_source(&self) -> Option<&NodeSource> {
        self.main.as_ref()
    }
}

/// A tabs target frozen against one workspace state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabTarget {
    pub(crate) root: RootId,
    pub(crate) tabs: NodeId,
    pub(crate) fingerprint: NodeFingerprint,
    pub(crate) surface: SurfaceId,
    pub(crate) central: bool,
    pub(crate) rule: DockTargetRuleKey,
}

impl TabTarget {
    /// Returns the target root.
    pub const fn root(&self) -> RootId {
        self.root
    }

    /// Returns the target tabs node.
    pub const fn tabs(&self) -> NodeId {
        self.tabs
    }

    /// Returns the target subtree fingerprint.
    pub const fn fingerprint(&self) -> &NodeFingerprint {
        &self.fingerprint
    }

    /// Returns the logical surface that owned this target when it was captured.
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Returns whether this tabs leaf was the root's declared central node.
    pub const fn is_central(&self) -> bool {
        self.central
    }

    /// Returns the selected item that represented this pane-local target.
    pub const fn rule(&self) -> DockTargetRuleKey {
        self.rule
    }
}

/// A validated payload share used by an edge insertion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DockFraction(f32);

impl DockFraction {
    /// Creates a finite fraction strictly between zero and one.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidDockFraction`] when `value` is non-finite or outside
    /// the open interval `(0, 1)`.
    pub fn new(value: f32) -> Result<Self, InvalidDockFraction> {
        if value.is_finite() && value > 0.0 && value < 1.0 {
            Ok(Self(value))
        } else {
            Err(InvalidDockFraction { value })
        }
    }

    /// Returns the scalar fraction.
    pub const fn get(self) -> f32 {
        self.0
    }
}

/// Failure to construct a [`DockFraction`].
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
#[error("dock fraction must be finite and strictly between zero and one, got {value}")]
pub struct InvalidDockFraction {
    /// Rejected scalar.
    pub value: f32,
}

/// Physical side of a target branch receiving an edge insertion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Edge {
    /// Insert before the target on the horizontal axis.
    Left,
    /// Insert after the target on the horizontal axis.
    Right,
    /// Insert before the target on the vertical axis.
    Top,
    /// Insert after the target on the vertical axis.
    Bottom,
}

/// Semantic extent of one frozen edge target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeTargetScope {
    /// Split one selected tabs leaf at a branch-local edge.
    Inner,
    /// Split the complete docking root at its outer boundary.
    Outer,
}

/// An edge target frozen against one workspace state.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeTarget {
    pub(crate) root: RootId,
    pub(crate) node: NodeId,
    pub(crate) fingerprint: NodeFingerprint,
    pub(crate) edge: Edge,
    pub(crate) fraction: DockFraction,
    pub(crate) surface: SurfaceId,
    pub(crate) central: bool,
    pub(crate) rule: DockTargetRuleKey,
    pub(crate) scope: EdgeTargetScope,
}

impl EdgeTarget {
    /// Returns the target root.
    pub const fn root(&self) -> RootId {
        self.root
    }

    /// Returns the target branch node.
    pub const fn node(&self) -> NodeId {
        self.node
    }

    /// Returns the target subtree fingerprint.
    pub const fn fingerprint(&self) -> &NodeFingerprint {
        &self.fingerprint
    }

    /// Returns the insertion side.
    pub const fn edge(&self) -> Edge {
        self.edge
    }

    /// Returns the payload share of the target branch.
    pub const fn fraction(&self) -> DockFraction {
        self.fraction
    }

    /// Returns the logical surface that owned this target when it was captured.
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Returns whether this branch was the root's declared central node.
    pub const fn is_central(&self) -> bool {
        self.central
    }

    /// Returns the exact semantic rule represented by this edge.
    pub const fn rule(&self) -> DockTargetRuleKey {
        self.rule
    }

    /// Returns whether this proof names an inner or outer edge.
    pub const fn scope(&self) -> EdgeTargetScope {
        self.scope
    }
}

/// Existing topology location receiving an opened or moved payload.
#[derive(Debug, Clone, PartialEq)]
pub enum DockTarget {
    /// Append a payload to an existing tabs stack.
    Center(TabTarget),
    /// Insert a payload into an exact pre-removal tab gap.
    TabGap {
        /// Frozen tabs target.
        target: TabTarget,
        /// Gap index in the target's current tab sequence.
        index: usize,
    },
    /// Split a selected tabs leaf at an explicit branch-local edge and fraction.
    InnerEdge(EdgeTarget),
    /// Split the complete target root at an explicit outer edge and fraction.
    OuterEdge(EdgeTarget),
}

impl DockTarget {
    /// Returns the frozen logical target surface.
    pub const fn surface(&self) -> SurfaceId {
        match self {
            Self::Center(target) | Self::TabGap { target, .. } => target.surface(),
            Self::InnerEdge(target) | Self::OuterEdge(target) => target.surface(),
        }
    }

    /// Returns whether the exact target is the root's central node.
    pub const fn is_central(&self) -> bool {
        match self {
            Self::Center(target) | Self::TabGap { target, .. } => target.is_central(),
            Self::InnerEdge(target) | Self::OuterEdge(target) => target.is_central(),
        }
    }

    /// Returns the core-minted semantic policy rule for this target.
    pub const fn rule(&self) -> DockTargetRuleKey {
        match self {
            Self::Center(target) | Self::TabGap { target, .. } => target.rule(),
            Self::InnerEdge(target) | Self::OuterEdge(target) => target.rule(),
        }
    }
}

/// Application content detached by a move command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MovePayload {
    /// Move one item out of its tabs stack.
    Item(ItemSource),
    /// Move and merge one complete tabs stack.
    Tabs(NodeSource),
    /// Move an arbitrary complete subtree; center targets reject split payloads.
    Subtree(NodeSource),
}

/// Content used to create a new independently presented root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootContent {
    /// Open one application item, increasing the item multiset by one.
    OpenItem(ItemId),
    /// Move existing workspace content without changing item ownership.
    Move(MovePayload),
}

/// Structural insertion position in one surface's contained roster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainedPosition {
    /// Insert at the roster end, which is the frontmost presentation.
    Front,
    /// Insert immediately behind the named presentation.
    Before(FloatingPresentationId),
    /// Insert immediately in front of the named presentation.
    After(FloatingPresentationId),
}

/// Opaque snapshot of one complete contained roster used as a command precondition.
///
/// Capturing the whole back-to-front sequence prevents delayed focus input from recomputing a
/// raise against peer presentations that changed in the meantime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainedRosterSource {
    pub(crate) surface: SurfaceId,
    pub(crate) contained: Arc<[FloatingPresentationId]>,
}

impl ContainedRosterSource {
    /// Returns the surface whose roster was captured.
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Returns the captured contained identities in back-to-front order.
    pub fn contained(&self) -> &[FloatingPresentationId] {
        &self.contained
    }
}

/// Explicit presentation destination for an existing complete root.
///
/// Rehoming preserves the root and topology identities. Moving between two
/// contained hosts must also preserve the existing floating presentation
/// identity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RootPresentationTarget {
    /// Present the root as the main dock area of a newly created logical surface.
    NewSurface { surface: SurfaceId },
    /// Install the root into an existing rootless surface's main dock area.
    Main { surface: SurfaceId },
    /// Present the root as a contained floating on an existing surface.
    Contained {
        surface: SurfaceId,
        floating: FloatingPresentationId,
        rect: LogicalRect,
        position: ContainedPosition,
    },
}

/// The sole durable mutation vocabulary for a docking workspace.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkspaceCommand {
    /// Select one item in its current tabs stack.
    Select { source: ItemSource },
    /// Reorder one item to an exact pre-removal gap in the same tabs stack.
    Reorder {
        source: ItemSource,
        insertion_index: usize,
    },
    /// Open one not-yet-owned item into existing topology.
    Open { item: ItemId, target: DockTarget },
    /// Move an item, tabs stack, or complete subtree into existing topology.
    Move {
        payload: MovePayload,
        target: DockTarget,
    },
    /// Atomically replace every weight for one or more distinct splits.
    ResizeSplits { splits: Vec<SplitResize> },
    /// Create a new logical surface and its main root atomically.
    CreateSurfaceRoot {
        surface: SurfaceId,
        root: RootId,
        content: RootContent,
    },
    /// Create a contained-floating root and both presentation backlinks atomically.
    CreateContainedRoot {
        surface: SurfaceId,
        root: RootId,
        floating: FloatingPresentationId,
        rect: LogicalRect,
        position: ContainedPosition,
        content: RootContent,
    },
    /// Install a newly created root into an existing rootless surface.
    ///
    /// `root` is a caller-supplied fresh identity. Complete existing roots use
    /// [`WorkspaceCommand::RehomeRoot`], except that a contained root becoming main on its current
    /// surface uses [`WorkspaceCommand::PromoteContained`]. Neither path replaces the root identity.
    InstallMainRoot {
        surface: SurfaceId,
        root: RootId,
        content: RootContent,
    },
    /// Transfer an existing complete root to another presentation owner.
    ///
    /// Unlike `Create*Root`, this command never changes `RootId`, `NodeId`, or
    /// central-region identity.
    RehomeRoot {
        source: NodeSource,
        target: RootPresentationTarget,
    },
    /// Promote one exact contained root to the main slot of its current rootless surface.
    PromoteContained {
        source: NodeSource,
        surface: SurfaceId,
        floating: FloatingPresentationId,
    },
    /// Replace a contained rectangle only when its exact previous value still matches.
    ///
    /// This command is intended for application/offline mutation and scene-proven
    /// resize sessions that do not alter stacking. A contained title drag must use
    /// [`WorkspaceCommand::UpdateContainedPresentation`] so geometry and roster
    /// position share one exact transaction boundary.
    UpdateContainedRect {
        surface: SurfaceId,
        root: RootId,
        floating: FloatingPresentationId,
        expected_rect: LogicalRect,
        rect: LogicalRect,
    },
    /// Atomically update one existing contained presentation's rectangle and roster position.
    ///
    /// The complete root source, previous rectangle, and full surface roster are
    /// exact preconditions. Position is resolved only after removing `floating`
    /// from that frozen roster, so self anchors reject and peer anchors cannot
    /// be reinterpreted after concurrent changes.
    UpdateContainedPresentation {
        source: NodeSource,
        floating: FloatingPresentationId,
        expected_rect: LogicalRect,
        expected_roster: ContainedRosterSource,
        rect: LogicalRect,
        position: ContainedPosition,
    },
    /// Raise a contained presentation after explicit focus input.
    ///
    /// The complete captured roster is an exact precondition, so an old focus command never
    /// recomputes itself against newer peer state.
    RaiseContained {
        source: NodeSource,
        floating: FloatingPresentationId,
        expected_roster: ContainedRosterSource,
    },
    /// Remove a root whose topology contains no application item.
    RemoveEmptyRoot { source: NodeSource },
}

impl WorkspaceCommand {
    /// Returns the application item introduced by this command, if any.
    ///
    /// Move commands only transfer content already owned by the workspace. The
    /// four opening forms are therefore the complete identity-admission surface.
    pub(crate) const fn opened_item(&self) -> Option<ItemId> {
        match self {
            Self::Open { item, .. } => Some(*item),
            Self::CreateSurfaceRoot {
                content: RootContent::OpenItem(item),
                ..
            }
            | Self::CreateContainedRoot {
                content: RootContent::OpenItem(item),
                ..
            }
            | Self::InstallMainRoot {
                content: RootContent::OpenItem(item),
                ..
            } => Some(*item),
            Self::Select { .. }
            | Self::Reorder { .. }
            | Self::Move { .. }
            | Self::ResizeSplits { .. }
            | Self::CreateSurfaceRoot {
                content: RootContent::Move(_),
                ..
            }
            | Self::CreateContainedRoot {
                content: RootContent::Move(_),
                ..
            }
            | Self::InstallMainRoot {
                content: RootContent::Move(_),
                ..
            }
            | Self::RehomeRoot { .. }
            | Self::PromoteContained { .. }
            | Self::UpdateContainedRect { .. }
            | Self::UpdateContainedPresentation { .. }
            | Self::RaiseContained { .. }
            | Self::RemoveEmptyRoot { .. } => None,
        }
    }
}

/// Structured result of one successfully staged command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandOutcome {
    /// Selection was checked and optionally changed.
    Selected {
        item: ItemId,
        tabs: NodeId,
        changed: bool,
    },
    /// One tab was checked and optionally reordered.
    Reordered {
        item: ItemId,
        tabs: NodeId,
        from: usize,
        to: usize,
        changed: bool,
    },
    /// One new item was opened.
    Opened { item: ItemId, root: RootId },
    /// Existing items were moved without changing ownership.
    Moved {
        items: Vec<ItemId>,
        source_root: RootId,
        target_root: RootId,
        changed: bool,
    },
    /// One atomic split-resize batch was checked and optionally stored.
    SplitsResized { splits: Vec<NodeId>, changed: bool },
    /// A new surface and main root were created.
    SurfaceRootCreated {
        surface: SurfaceId,
        root: RootId,
        items: Vec<ItemId>,
    },
    /// A new contained-floating root was created.
    ContainedRootCreated {
        surface: SurfaceId,
        root: RootId,
        floating: FloatingPresentationId,
        items: Vec<ItemId>,
    },
    /// A new main root was installed into an existing rootless surface.
    MainRootInstalled {
        surface: SurfaceId,
        root: RootId,
        items: Vec<ItemId>,
    },
    /// An existing root was transferred without changing topology identity.
    RootRehomed {
        root: RootId,
        surface: SurfaceId,
        floating: Option<FloatingPresentationId>,
        changed: bool,
    },
    /// One exact contained presentation became its surface's main root.
    ContainedPromoted {
        surface: SurfaceId,
        root: RootId,
        floating: FloatingPresentationId,
    },
    /// A contained rectangle was checked and optionally updated.
    ContainedRectUpdated {
        floating: FloatingPresentationId,
        changed: bool,
    },
    /// An existing contained presentation's rectangle and roster position were checked together.
    ContainedPresentationUpdated {
        floating: FloatingPresentationId,
        from: usize,
        to: usize,
        rect_changed: bool,
        order_changed: bool,
    },
    /// A contained roster position was checked and optionally raised.
    ContainedRaised {
        floating: FloatingPresentationId,
        from: usize,
        to: usize,
        changed: bool,
    },
    /// An empty root and its presentation were removed.
    EmptyRootRemoved { root: RootId },
}

impl CommandOutcome {
    pub(crate) const fn changes_workspace(&self) -> bool {
        match self {
            Self::Selected { changed, .. }
            | Self::Reordered { changed, .. }
            | Self::Moved { changed, .. }
            | Self::SplitsResized { changed, .. }
            | Self::RootRehomed { changed, .. }
            | Self::ContainedRectUpdated { changed, .. }
            | Self::ContainedRaised { changed, .. } => *changed,
            Self::ContainedPresentationUpdated {
                rect_changed,
                order_changed,
                ..
            } => *rect_changed || *order_changed,
            Self::Opened { .. }
            | Self::SurfaceRootCreated { .. }
            | Self::ContainedRootCreated { .. }
            | Self::MainRootInstalled { .. }
            | Self::ContainedPromoted { .. }
            | Self::EmptyRootRemoved { .. } => true,
        }
    }
}

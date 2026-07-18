//! Typed commands and topology references for durable workspace mutation.

use std::fmt;
use std::sync::Arc;

use crate::geometry::LogicalRect;
use crate::graph::{Axis, ContainedStackKey, SplitWeight};
use crate::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};

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

/// A tabs target frozen against one workspace state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabTarget {
    pub(crate) root: RootId,
    pub(crate) tabs: NodeId,
    pub(crate) fingerprint: NodeFingerprint,
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

/// An edge target frozen against one workspace state.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeTarget {
    pub(crate) root: RootId,
    pub(crate) node: NodeId,
    pub(crate) fingerprint: NodeFingerprint,
    pub(crate) edge: Edge,
    pub(crate) fraction: DockFraction,
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
    /// Split an existing target branch at an explicit edge and fraction.
    Edge(EdgeTarget),
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

/// Explicit presentation destination for an existing complete root.
///
/// Rehoming preserves the root and topology identities. Moving between two
/// contained hosts must also preserve the existing floating presentation
/// identity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RootPresentationTarget {
    /// Present the root as the main dock area of a logical surface.
    Surface { surface: SurfaceId },
    /// Present the root as a contained floating on an existing surface.
    Contained {
        surface: SurfaceId,
        floating: FloatingPresentationId,
        rect: LogicalRect,
        z_order: u64,
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
    /// Close one existing item.
    ///
    /// Closing the selected item selects its successor at the same index, or
    /// the previous item when the closed item was last. Closing an inactive
    /// item preserves the current selection. This order rule is normative and
    /// never depends on focus history or renderer state.
    Close { source: ItemSource },
    /// Move an item, tabs stack, or complete subtree into existing topology.
    Move {
        payload: MovePayload,
        target: DockTarget,
    },
    /// Replace every weight of one split with caller-supplied normalized weights.
    ResizeSplit {
        split: NodeSource,
        weights: Vec<SplitWeight>,
    },
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
        z_order: u64,
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
    /// Replace a contained rectangle only when its exact previous value still matches.
    UpdateContainedRect {
        surface: SurfaceId,
        root: RootId,
        floating: FloatingPresentationId,
        expected_rect: LogicalRect,
        rect: LogicalRect,
    },
    /// Raise a contained presentation after explicit focus input.
    ///
    /// Both the presentation's own z-order and the surface's deterministic
    /// frontmost key are exact preconditions, so an old focus command never
    /// recomputes itself against newer peer state.
    RaiseContained {
        surface: SurfaceId,
        root: RootId,
        floating: FloatingPresentationId,
        expected_z_order: u64,
        expected_frontmost: ContainedStackKey,
    },
    /// Remove a root whose topology contains no application item.
    RemoveEmptyRoot { source: NodeSource },
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
    /// One item was closed.
    Closed { item: ItemId, root: RootId },
    /// Existing items were moved without changing ownership.
    Moved {
        items: Vec<ItemId>,
        source_root: RootId,
        target_root: RootId,
        changed: bool,
    },
    /// Caller-supplied split weights were checked and optionally stored.
    SplitResized { split: NodeId, changed: bool },
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
    /// An existing root was transferred without changing topology identity.
    RootRehomed {
        root: RootId,
        surface: SurfaceId,
        floating: Option<FloatingPresentationId>,
        changed: bool,
    },
    /// A contained rectangle was checked and optionally updated.
    ContainedRectUpdated {
        floating: FloatingPresentationId,
        changed: bool,
    },
    /// A contained z-order was checked and optionally raised.
    ContainedRaised {
        floating: FloatingPresentationId,
        previous: u64,
        current: u64,
        changed: bool,
    },
    /// An empty root and its presentation were removed.
    EmptyRootRemoved { root: RootId },
}

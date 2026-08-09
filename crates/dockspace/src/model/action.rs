use thiserror::Error;

use crate::ids::{EngineAuthorityDomainId, ItemId, RootId, SurfaceId};

use super::WorkspaceVersion;

/// Stable item or central-region anchor used by product docking actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DockAnchor {
    /// The tabs leaf which currently owns this item.
    Item(ItemId),
    /// The central tabs leaf declared by this root.
    Central(RootId),
}

/// Physical side of a stable docking anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DockEdge {
    /// Insert before the target along the horizontal axis.
    Left,
    /// Insert after the target along the horizontal axis.
    Right,
    /// Insert before the target along the vertical axis.
    Top,
    /// Insert after the target along the vertical axis.
    Bottom,
}

/// Payload share assigned by one edge docking action.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DockFraction(f32);

impl DockFraction {
    /// Creates a finite fraction strictly between zero and one.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidDockFraction`] when `value` is non-finite or outside the open unit range.
    pub fn new(value: f32) -> Result<Self, InvalidDockFraction> {
        if value.is_finite() && value > 0.0 && value < 1.0 {
            Ok(Self(value))
        } else {
            Err(InvalidDockFraction { value })
        }
    }

    /// Returns the validated scalar.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}

/// Failure to construct a [`DockFraction`].
#[derive(Debug, Clone, Copy, PartialEq, Error)]
#[error("dock fraction must be finite and strictly between zero and one, got {value}")]
pub struct InvalidDockFraction {
    /// Rejected scalar.
    pub value: f32,
}

/// Stable product placement for opening or moving one item.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DockPlacement {
    /// Merge into the tabs leaf represented by the anchor.
    ///
    /// Moving an item within its current group does not implicitly reorder it to the end.
    Center(DockAnchor),
    /// Insert immediately before the named item in its current tabs leaf.
    Before(ItemId),
    /// Insert immediately after the named item in its current tabs leaf.
    After(ItemId),
    /// Split the anchor's tabs leaf at one branch-local edge.
    InnerEdge {
        /// Stable tabs or central-region anchor.
        anchor: DockAnchor,
        /// Insertion side.
        edge: DockEdge,
        /// Share assigned to the opened or moved item.
        fraction: DockFraction,
    },
    /// Split the complete target root at its outer boundary.
    OuterEdge {
        /// Stable target root.
        root: RootId,
        /// Insertion side.
        edge: DockEdge,
        /// Share assigned to the opened or moved item.
        fraction: DockFraction,
    },
    /// Make an existing rootless surface own the item as its main presentation.
    ///
    /// Opening new content or moving part of a root allocates a new root. Moving
    /// the complete source root preserves that root's stable identity.
    Main(SurfaceId),
}

/// Product-visible result of one accepted item action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockspaceActionOutcome {
    /// Selection was checked and optionally changed.
    Selected {
        /// Selected item.
        item: ItemId,
        /// Whether durable selection or MRU state changed.
        changed: bool,
    },
    /// A new item was opened into one root.
    Opened {
        /// Opened item.
        item: ItemId,
        /// Root which now owns the item.
        root: RootId,
    },
    /// The requested item was already open and was deliberately left in place.
    Existing {
        /// Existing item.
        item: ItemId,
    },
    /// An existing item was docked into another semantic placement.
    Docked {
        /// Moved item.
        item: ItemId,
        /// Root which owned the item before the action.
        source_root: RootId,
        /// Root which owns the item after the action.
        target_root: RootId,
        /// Whether topology or tab order changed.
        changed: bool,
    },
    /// A complete root was merged into another placement or rehomed as one unit.
    RootDocked {
        /// Stable root moved as the payload authority.
        root: RootId,
        /// Root which owns the payload after the action.
        target_root: RootId,
        /// Stable items moved with the complete root in traversal order.
        items: Vec<ItemId>,
        /// Whether topology, presentation ownership, or tab order changed.
        changed: bool,
    },
}

impl DockspaceActionOutcome {
    /// Returns whether this outcome changed durable workspace state.
    #[must_use]
    pub const fn changed(&self) -> bool {
        match self {
            Self::Selected { changed, .. }
            | Self::Docked { changed, .. }
            | Self::RootDocked { changed, .. } => *changed,
            Self::Opened { .. } => true,
            Self::Existing { .. } => false,
        }
    }
}

/// Stable, actionable rejection category for one product item action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DockspaceActionRejection {
    /// The item to select or move is not currently open.
    #[error("item {item} is not open")]
    ItemUnavailable {
        /// Missing item.
        item: ItemId,
    },
    /// The stable target no longer names an available tabs or root anchor.
    #[error("dock anchor {anchor:?} is unavailable")]
    AnchorUnavailable {
        /// Missing anchor.
        anchor: DockAnchor,
    },
    /// The stable target root is unavailable.
    #[error("dock root {root} is unavailable")]
    RootUnavailable {
        /// Missing root.
        root: RootId,
    },
    /// The requested rootless main-surface destination is unavailable.
    #[error("surface {surface} cannot accept a main root")]
    MainSurfaceUnavailable {
        /// Missing or occupied surface.
        surface: SurfaceId,
    },
    /// Current policy rejects the requested source, target, or mutation class.
    #[error("current docking policy rejects the action")]
    PolicyDenied,
    /// Current topology changed or otherwise conflicts with the requested action.
    #[error("current docking topology conflicts with the action")]
    Conflict,
    /// No fresh stable presentation identity remains available.
    #[error("stable presentation identity space is exhausted")]
    IdentityExhausted,
}

/// Session- and revision-bound product docking action.
///
/// A prepared action preserves the exact published workspace version from
/// which its stable placement was derived. The core validates its opaque
/// authority domain before turning it into reducer input.
#[derive(Debug)]
#[must_use = "a prepared docking action must be submitted or deliberately discarded"]
pub struct PreparedDockAction {
    authority_domain: EngineAuthorityDomainId,
    expected: WorkspaceVersion,
    action: ProductAction,
}

impl PreparedDockAction {
    pub(crate) const fn new(
        authority_domain: EngineAuthorityDomainId,
        expected: WorkspaceVersion,
        action: ProductAction,
    ) -> Self {
        Self {
            authority_domain,
            expected,
            action,
        }
    }

    /// Returns the published workspace version from which this action was prepared.
    #[must_use]
    pub const fn expected_version(&self) -> WorkspaceVersion {
        self.expected
    }

    pub(crate) const fn into_parts(
        self,
    ) -> (EngineAuthorityDomainId, WorkspaceVersion, ProductAction) {
        (self.authority_domain, self.expected, self.action)
    }
}

/// A prepared product action was submitted to a different dockspace session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("prepared docking action belongs to another dockspace authority domain")]
pub struct PreparedDockActionAuthorityMismatch;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ProductAction {
    SelectItem {
        item: ItemId,
    },
    OpenItem {
        item: ItemId,
        placement: DockPlacement,
    },
    DockItem {
        item: ItemId,
        placement: DockPlacement,
    },
    DockRoot {
        root: RootId,
        placement: DockPlacement,
    },
}

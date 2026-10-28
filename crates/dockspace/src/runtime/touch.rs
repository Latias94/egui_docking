//! Renderer-neutral touch arbitration events and context-menu requests.

use super::{SurfacePointerId, WorkspaceVersion};
use crate::event::ReductionCause;
use crate::geometry::LogicalPoint;
use crate::ids::{ItemId, RootId, SurfaceId};

/// One core-owned touch arbitration or context-menu event.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DockspaceTouchEvent {
    /// One touch pointer was armed for core-owned interaction.
    Armed {
        /// Provider-local pointer identity.
        pointer: SurfacePointerId,
        /// Logical surface which owns the interaction.
        surface: SurfaceId,
    },
    /// One touch release activated its receiver or completed without a change.
    Activated {
        /// Provider-local pointer identity.
        pointer: SurfacePointerId,
        /// Logical surface which owns the interaction.
        surface: SurfaceId,
    },
    /// One touch pointer yielded its interaction to application content.
    YieldedToContent {
        /// Provider-local pointer identity.
        pointer: SurfacePointerId,
        /// Logical surface whose content receives the interaction.
        surface: SurfaceId,
    },
    /// One core-owned interaction requested an application context menu.
    ContextMenuRequested(DockspaceContextMenuRequest),
}

/// One context-menu request with opaque causal identity for deduplication.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DockspaceContextMenuRequest {
    cause: ReductionCause,
    version: WorkspaceVersion,
    pointer: SurfacePointerId,
    surface: SurfaceId,
    root: RootId,
    item: Option<ItemId>,
    position: LogicalPoint,
}

impl DockspaceContextMenuRequest {
    pub(super) const fn new(
        cause: ReductionCause,
        version: WorkspaceVersion,
        pointer: SurfacePointerId,
        surface: SurfaceId,
        root: RootId,
        item: Option<ItemId>,
        position: LogicalPoint,
    ) -> Self {
        Self {
            cause,
            version,
            pointer,
            surface,
            root,
            item,
            position,
        }
    }

    /// Returns the final workspace version published by the host report.
    #[must_use]
    pub const fn version(self) -> WorkspaceVersion {
        self.version
    }

    /// Returns the provider-local pointer which requested the menu.
    #[must_use]
    pub const fn pointer(self) -> SurfacePointerId {
        self.pointer
    }

    /// Returns the logical surface which owns the menu target.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    /// Returns the root targeted by the menu request.
    #[must_use]
    pub const fn root(self) -> RootId {
        self.root
    }

    /// Returns the targeted item, when the request names one.
    #[must_use]
    pub const fn item(self) -> Option<ItemId> {
        self.item
    }

    /// Returns the logical surface position at which the menu was requested.
    #[must_use]
    pub const fn position(self) -> LogicalPoint {
        self.position
    }
}

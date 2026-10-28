//! Read-only wake requests for work owned by the committed pointer gesture.

use std::time::Duration;

use crate::geometry::LogicalPoint;
use crate::ids::SurfaceId;
use crate::interaction::PointerContinuationRequest;

use super::{DockspaceSession, PointerContinuationToken, SurfacePointerEvent, SurfacePointerId};

/// One finite stationary action which the host may recheck at its deadline.
///
/// A wake is scheduling information, not permission to mutate layout. Return
/// its event through the normal ordered pointer input with explicit time,
/// capture and fresh current-output receiver facts at the requested point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DockspacePointerContinuation {
    request: PointerContinuationRequest,
}

impl DockspacePointerContinuation {
    /// Returns the provider-local pointer whose existing stream owns this wake.
    #[must_use]
    pub const fn pointer(self) -> SurfacePointerId {
        SurfacePointerId::new(self.request.token.owner.pointer().get())
    }

    /// Returns the earliest observation time which can execute this action.
    #[must_use]
    pub const fn deadline(self) -> Duration {
        self.request.token.deadline
    }

    /// Returns the logical surface whose receiver must be freshly queried.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.request.surface
    }

    /// Returns the stationary point for the fresh framework receiver query.
    #[must_use]
    pub const fn position(self) -> LogicalPoint {
        self.request.point
    }

    /// Returns the opaque, retryable token for this exact pending action.
    #[must_use]
    pub const fn token(self) -> PointerContinuationToken {
        self.request.token
    }

    /// Returns the explicit non-motion event for the normal pointer journal.
    #[must_use]
    pub const fn event(self) -> SurfacePointerEvent {
        SurfacePointerEvent::Continuation(self.request.token)
    }
}

impl DockspaceSession {
    /// Returns the current committed gesture's finite stationary wake, if any.
    ///
    /// Schedule the deadline on the input provider's clock. After waking, query
    /// this again: a changed gesture, scene, policy or stream may revoke it.
    /// Merely reading or scheduling the request does not advance input or layout.
    #[must_use]
    pub fn pointer_continuation(&self) -> Option<DockspacePointerContinuation> {
        self.engine
            .pointer_continuation()
            .map(|request| DockspacePointerContinuation { request })
    }
}

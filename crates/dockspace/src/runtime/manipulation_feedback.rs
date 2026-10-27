//! Stateless product feedback projected from committed interaction events.

use crate::event::ReductionCause;
use crate::ids::{EngineAuthorityDomainId, ReducerTickId};
use crate::interaction::{
    ContainedTransformSessionId, DragSessionId, InteractionEvent, InteractionEventKind,
    InteractionStatus, ResizeSessionId, WorkspaceDeliveryKind,
};

pub use crate::interaction::{
    InteractionCancelReason as DockspaceManipulationCancelReason,
    ManipulationRejection as DockspaceManipulationRejection,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ManipulationIdentity {
    Drag(DragSessionId),
    Resize(ResizeSessionId),
    Contained(ContainedTransformSessionId),
    Action(ReductionCause),
}

/// Opaque identity shared by pending and terminal feedback for one manipulation.
///
/// Equality is scoped to the engine authority domain, so independent sessions
/// and newly admitted gestures cannot alias an earlier manipulation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DockspaceManipulationId {
    domain: EngineAuthorityDomainId,
    identity: ManipulationIdentity,
}

impl std::fmt::Debug for DockspaceManipulationId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DockspaceManipulationId(..)")
    }
}

/// Opaque identity of one feedback publication, stable when a report is reread.
///
/// Hosts may retain these identities to suppress repeated announcements. A
/// terminal publication has a different identity from its pending publication.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct DockspaceManipulationFeedbackId {
    domain: EngineAuthorityDomainId,
    tick: ReducerTickId,
    event_index: usize,
}

impl std::fmt::Debug for DockspaceManipulationFeedbackId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DockspaceManipulationFeedbackId(..)")
    }
}

/// Why a consumed manipulation is still pending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DockspaceManipulationPendingReason {
    /// The host has not yet presented the exact release preview.
    AwaitingPresentation,
}

/// Semantic result of a successfully delivered manipulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DockspaceManipulationDelivery {
    /// Content was delivered to a docking target.
    Dock,
    /// Content was delivered to a contained presentation.
    Contained,
    /// An explicitly enabled contained fallback was delivered.
    ContainedFallback,
    /// A splitter resize committed.
    SplitterResize,
    /// A contained move or resize committed.
    ContainedTransform,
}

/// Inert product status; consuming feedback never requests a focus change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DockspaceManipulationStatus {
    /// A consumed release awaits the specified prerequisite.
    Pending(DockspaceManipulationPendingReason),
    /// The manipulation committed exactly once.
    Delivered(DockspaceManipulationDelivery),
    /// The manipulation ended without delivery for a semantic reason.
    Cancelled(DockspaceManipulationCancelReason),
    /// The terminal action was rejected without exposing private validation facts.
    Rejected(DockspaceManipulationRejection),
}

/// One committed pending, terminal, or rejection publication in core event order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DockspaceManipulationFeedback {
    id: DockspaceManipulationFeedbackId,
    manipulation: DockspaceManipulationId,
    status: DockspaceManipulationStatus,
}

impl DockspaceManipulationFeedback {
    /// Returns the identity to deduplicate this exact publication.
    #[must_use]
    pub const fn id(&self) -> DockspaceManipulationFeedbackId {
        self.id
    }

    /// Returns the identity shared across this manipulation's status publications.
    #[must_use]
    pub const fn manipulation_id(&self) -> DockspaceManipulationId {
        self.manipulation
    }

    /// Returns the inert semantic status.
    #[must_use]
    pub const fn status(&self) -> DockspaceManipulationStatus {
        self.status
    }

    pub(super) fn from_event(
        domain: EngineAuthorityDomainId,
        tick: ReducerTickId,
        event_index: usize,
        event: &InteractionEvent,
    ) -> Option<Self> {
        let identity_from_status = |status| match status {
            InteractionStatus::Armed { session } | InteractionStatus::Dragging { session } => {
                ManipulationIdentity::Drag(session)
            }
            InteractionStatus::Resizing { session } => ManipulationIdentity::Resize(session),
            InteractionStatus::ContainedTransforming { session } => {
                ManipulationIdentity::Contained(session)
            }
            InteractionStatus::Pressed { .. } | InteractionStatus::Idle => {
                ManipulationIdentity::Action(event.cause())
            }
        };
        let (identity, status) = match event.kind() {
            InteractionEventKind::ReleasePending { session } => (
                ManipulationIdentity::Drag(*session),
                DockspaceManipulationStatus::Pending(
                    DockspaceManipulationPendingReason::AwaitingPresentation,
                ),
            ),
            InteractionEventKind::ContainedTransformReleasePending { session } => (
                ManipulationIdentity::Contained(*session),
                DockspaceManipulationStatus::Pending(
                    DockspaceManipulationPendingReason::AwaitingPresentation,
                ),
            ),
            InteractionEventKind::Delivered { session, kind } => (
                ManipulationIdentity::Drag(*session),
                DockspaceManipulationStatus::Delivered(match kind {
                    WorkspaceDeliveryKind::Dock => DockspaceManipulationDelivery::Dock,
                    WorkspaceDeliveryKind::Contained => DockspaceManipulationDelivery::Contained,
                    WorkspaceDeliveryKind::ContainedFallback => {
                        DockspaceManipulationDelivery::ContainedFallback
                    }
                }),
            ),
            InteractionEventKind::ResizeDelivered { session } => (
                ManipulationIdentity::Resize(*session),
                DockspaceManipulationStatus::Delivered(
                    DockspaceManipulationDelivery::SplitterResize,
                ),
            ),
            InteractionEventKind::ContainedTransformDelivered { session } => (
                ManipulationIdentity::Contained(*session),
                DockspaceManipulationStatus::Delivered(
                    DockspaceManipulationDelivery::ContainedTransform,
                ),
            ),
            InteractionEventKind::Cancelled { status, reason } => (
                identity_from_status(*status),
                DockspaceManipulationStatus::Cancelled(*reason),
            ),
            InteractionEventKind::Rejected { status, reason } => (
                identity_from_status(*status),
                DockspaceManipulationStatus::Rejected(*reason),
            ),
            _ => return None,
        };
        Some(Self {
            id: DockspaceManipulationFeedbackId {
                domain,
                tick,
                event_index,
            },
            manipulation: DockspaceManipulationId { domain, identity },
            status,
        })
    }
}

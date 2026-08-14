//! Product-facing close-plan views and decision outcomes.
//!
//! The reducer owns exact authority, native settlement, and lifecycle proofs.
//! This module projects only the stable identities, decisions, and phases an
//! application host can act on.

use crate::close_plan::{
    CloseDecisionToken, CloseInertReason, CloseItemDecisionState, ClosePlan, ClosePlanPhase,
    ClosePlanTarget, CloseRequestId, CloseResolutionOutcome, DeferredCloseToken,
};
use crate::ids::ItemId;
use crate::policy::CloseCapability;

/// One application-facing item decision in a close plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DockspaceCloseItem {
    item: ItemId,
    capability: CloseCapability,
    token: CloseDecisionToken,
    state: CloseItemDecisionState,
}

impl DockspaceCloseItem {
    const fn from_core(item: crate::close_plan::ClosePlanItem) -> Self {
        Self {
            item: item.item(),
            capability: item.capability(),
            token: item.token(),
            state: item.state(),
        }
    }

    /// Returns the stable pane identity.
    #[must_use]
    pub const fn item(self) -> ItemId {
        self.item
    }

    /// Returns the policy capability frozen into this request.
    #[must_use]
    pub const fn capability(self) -> CloseCapability {
        self.capability
    }

    /// Returns the initial decision token.
    #[must_use]
    pub const fn token(self) -> CloseDecisionToken {
        self.token
    }

    /// Returns the current decision state.
    #[must_use]
    pub const fn state(self) -> CloseItemDecisionState {
        self.state
    }

    /// Returns the deferred continuation when one is pending.
    #[must_use]
    pub const fn deferred_token(self) -> Option<DeferredCloseToken> {
        match self.state {
            CloseItemDecisionState::Deferred { continuation } => Some(continuation),
            CloseItemDecisionState::Pending
            | CloseItemDecisionState::Allowed
            | CloseItemDecisionState::Vetoed => None,
        }
    }
}

/// Stable application-facing view of one core-owned close plan.
///
/// Native destruction proofs, cancellation state, reducer authority, and
/// internal lifecycle diagnostics deliberately remain private.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockspaceClosePlan {
    request: CloseRequestId,
    target: ClosePlanTarget,
    items: Box<[DockspaceCloseItem]>,
    phase: ClosePlanPhase,
}

impl DockspaceClosePlan {
    pub(crate) fn from_core(plan: &ClosePlan) -> Self {
        Self {
            request: plan.request(),
            target: plan.target(),
            items: plan
                .items()
                .iter()
                .copied()
                .map(DockspaceCloseItem::from_core)
                .collect(),
            phase: plan.phase(),
        }
    }

    /// Returns the unique close request identity.
    #[must_use]
    pub const fn request(&self) -> CloseRequestId {
        self.request
    }

    /// Returns the stable item, root, or surface target.
    #[must_use]
    pub const fn target(&self) -> ClosePlanTarget {
        self.target
    }

    /// Returns required pane decisions in core order.
    #[must_use]
    pub fn items(&self) -> &[DockspaceCloseItem] {
        &self.items
    }

    /// Returns the current application-visible phase.
    #[must_use]
    pub const fn phase(&self) -> ClosePlanPhase {
        self.phase
    }
}

/// Stable reason why a close decision was consumed without changing state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockspaceCloseInertReason {
    /// The request belongs to another core authority domain.
    AuthorityMismatch,
    /// The request identity was never allocated by this session.
    UnknownRequest,
    /// The request reached a terminal state and its detailed payload was compacted.
    RetiredRequest,
    /// The workspace or policy authority changed after the request was prepared.
    Stale,
    /// The supplied decision token does not belong to the requested decision.
    TokenMismatch,
    /// The same decision or deferred continuation was already consumed.
    AlreadyResolved,
    /// The frozen policy did not permit a deferred decision.
    DeferredUnsupported,
    /// The decision is not valid in the request's current phase.
    PhaseMismatch,
    /// The operation does not apply to this close target class.
    TargetMismatch,
    /// Exact native binding, effect, observation, or cancellation authority did not match.
    NativeAuthorityMismatch,
}

impl From<CloseInertReason> for DockspaceCloseInertReason {
    fn from(reason: CloseInertReason) -> Self {
        match reason {
            CloseInertReason::RequestDomainMismatch { .. }
            | CloseInertReason::AuthorityDomainMismatch { .. } => Self::AuthorityMismatch,
            CloseInertReason::UnknownRequest { .. } => Self::UnknownRequest,
            CloseInertReason::RetiredTerminal { .. } => Self::RetiredRequest,
            CloseInertReason::AuthorityStale { .. } => Self::Stale,
            CloseInertReason::UnknownDecisionToken { .. }
            | CloseInertReason::WrongDecisionToken { .. }
            | CloseInertReason::UnknownDeferredToken { .. }
            | CloseInertReason::WrongDeferredToken { .. } => Self::TokenMismatch,
            CloseInertReason::DuplicateDecision { .. }
            | CloseInertReason::DuplicateDeferredContinuation { .. }
            | CloseInertReason::Terminal { .. } => Self::AlreadyResolved,
            CloseInertReason::DeferredNotAllowed { .. } => Self::DeferredUnsupported,
            CloseInertReason::PhaseMismatch { .. } => Self::PhaseMismatch,
            CloseInertReason::TargetMismatch { .. } => Self::TargetMismatch,
            CloseInertReason::CancellationProofRequestMismatch { .. }
            | CloseInertReason::CancellationProofDomainMismatch { .. }
            | CloseInertReason::DestroyedProofRequestMismatch { .. }
            | CloseInertReason::DestroyedProofDomainMismatch { .. }
            | CloseInertReason::NativeBindingMismatch { .. }
            | CloseInertReason::NativeSurfaceMismatch { .. }
            | CloseInertReason::NativeEffectMismatch { .. }
            | CloseInertReason::NativeEdgeNotObserved { .. }
            | CloseInertReason::CloseEffectNotEmitted { .. }
            | CloseInertReason::NativeEmissionObservationPrecedesEdge { .. }
            | CloseInertReason::NativeEmissionInventoryPrecedesEdge { .. }
            | CloseInertReason::NativeCloseEffectPredecessorMismatch { .. }
            | CloseInertReason::CancellationEffectPredecessorMismatch { .. }
            | CloseInertReason::CancellationEffectMismatch { .. }
            | CloseInertReason::CancellationKnownFrontierMismatch { .. }
            | CloseInertReason::CancellationObservationNotNewer { .. }
            | CloseInertReason::CancellationInventoryNotNewer { .. }
            | CloseInertReason::DestroyedObservationNotNewer { .. }
            | CloseInertReason::DestroyedInventoryNotNewer { .. }
            | CloseInertReason::DestroyedEffectNotAcknowledged { .. } => {
                Self::NativeAuthorityMismatch
            }
        }
    }
}

/// Product-level result of consuming an initial or deferred close decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockspaceCloseResolution {
    /// One non-final allow decision was recorded.
    Recorded {
        /// Close request identity.
        request: CloseRequestId,
        /// Item whose decision was recorded.
        item: ItemId,
        /// Resulting public close phase.
        phase: ClosePlanPhase,
    },
    /// One initial token was exchanged for a deferred continuation.
    Deferred {
        /// Close request identity.
        request: CloseRequestId,
        /// Item whose decision was deferred.
        item: ItemId,
        /// Exact continuation required to finish the decision.
        continuation: DeferredCloseToken,
    },
    /// The last required allow vote made the atomic close eligible.
    Approved {
        /// Close request identity.
        request: CloseRequestId,
    },
    /// One item vetoed the complete semantic close operation.
    Vetoed {
        /// Close request identity.
        request: CloseRequestId,
        /// Item which vetoed closure.
        item: ItemId,
    },
    /// The decision was consumed inertly for a stable actionable reason.
    Inert(DockspaceCloseInertReason),
}

impl DockspaceCloseResolution {
    pub(crate) fn from_core(outcome: CloseResolutionOutcome) -> Self {
        match outcome {
            CloseResolutionOutcome::Recorded {
                request,
                item,
                phase,
            } => Self::Recorded {
                request,
                item,
                phase,
            },
            CloseResolutionOutcome::Deferred {
                request,
                item,
                continuation,
            } => Self::Deferred {
                request,
                item,
                continuation,
            },
            CloseResolutionOutcome::Approved { request } => Self::Approved { request },
            CloseResolutionOutcome::Vetoed { request, item } => Self::Vetoed { request, item },
            CloseResolutionOutcome::Inert(reason) => Self::Inert(reason.into()),
        }
    }
}

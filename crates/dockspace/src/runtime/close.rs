//! Product-facing close-plan views and decision outcomes.
//!
//! The reducer owns exact authority, native settlement, and lifecycle proofs.
//! This module projects only the stable identities, decisions, and phases an
//! application host can act on.

use std::fmt;

use thiserror::Error;

use crate::close_plan::{
    CloseDecisionToken, CloseInertReason, CloseItemDecisionState, ClosePlan,
    ClosePlanPhase as CoreClosePlanPhase, ClosePlanTarget, CloseRequestId, CloseResolutionOutcome,
    DeferredCloseToken,
};
use crate::command::ContentCloseTarget;
use crate::engine::EngineInput;
use crate::ids::{EngineAuthorityDomainId, ItemId, RootId};
use crate::model::WorkspaceVersion;
use crate::policy::CloseCapability;
use crate::transition::ContentCloseRequestRejection;

pub(crate) const fn product_close_target(target: ContentCloseTarget) -> ClosePlanTarget {
    match target {
        ContentCloseTarget::Item(item) => ClosePlanTarget::Item { item },
        ContentCloseTarget::Root(root) => ClosePlanTarget::Root { root },
    }
}

/// Stable fail-closed reason for rejecting a programmatic close request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum DockspaceCloseRequestRejection {
    /// The requested item is not currently open.
    #[error("item {item} is not open")]
    ItemUnavailable {
        /// Missing item.
        item: ItemId,
    },
    /// The requested root is not currently present.
    #[error("root {root} is unavailable")]
    RootUnavailable {
        /// Missing root.
        root: RootId,
    },
    /// The root contains no application-owned items to close.
    #[error("root {root} contains no closeable items")]
    RootEmpty {
        /// Empty root.
        root: RootId,
    },
    /// Current policy explicitly disables close for one item.
    #[error("item {item} prevents closing the requested target")]
    ItemCloseDisabled {
        /// Stable item or root requested by the caller.
        target: ClosePlanTarget,
        /// Non-closeable item.
        item: ItemId,
    },
    /// Current topology conflicts with the requested close target.
    #[error("current docking topology conflicts with the close request")]
    Conflict {
        /// Stable requested target.
        target: ClosePlanTarget,
    },
    /// Core could not capture a valid close source because an invariant failed.
    #[error("dockspace could not capture the close request")]
    Internal {
        /// Stable requested target.
        target: ClosePlanTarget,
    },
}

impl DockspaceCloseRequestRejection {
    pub(crate) const fn from_core(
        target: ContentCloseTarget,
        reason: &ContentCloseRequestRejection,
    ) -> Self {
        let target = product_close_target(target);
        match reason {
            ContentCloseRequestRejection::ItemUnavailable { item } => {
                Self::ItemUnavailable { item: *item }
            }
            ContentCloseRequestRejection::RootUnavailable { root } => {
                Self::RootUnavailable { root: *root }
            }
            ContentCloseRequestRejection::RootEmpty { root } => Self::RootEmpty { root: *root },
            ContentCloseRequestRejection::ItemCloseDisabled { item } => Self::ItemCloseDisabled {
                target,
                item: *item,
            },
            ContentCloseRequestRejection::SourceUnavailable(error) => {
                if error.is_expected_rejection() {
                    Self::Conflict { target }
                } else {
                    Self::Internal { target }
                }
            }
        }
    }

    /// Returns the stable item or root rejected by the request.
    #[must_use]
    pub const fn target(self) -> ClosePlanTarget {
        match self {
            Self::ItemUnavailable { item } => ClosePlanTarget::Item { item },
            Self::ItemCloseDisabled { target, .. } => target,
            Self::RootUnavailable { root } | Self::RootEmpty { root } => {
                ClosePlanTarget::Root { root }
            }
            Self::Conflict { target } | Self::Internal { target } => target,
        }
    }
}

/// Session- and revision-bound programmatic content-close request.
#[must_use = "a prepared close request must be submitted or deliberately discarded"]
pub struct PreparedCloseRequest {
    authority_domain: EngineAuthorityDomainId,
    expected: WorkspaceVersion,
    target: ContentCloseTarget,
}

impl PreparedCloseRequest {
    pub(crate) const fn new(
        authority_domain: EngineAuthorityDomainId,
        expected: WorkspaceVersion,
        target: ContentCloseTarget,
    ) -> Self {
        Self {
            authority_domain,
            expected,
            target,
        }
    }

    /// Returns the stable item or root requested by this action.
    #[must_use]
    pub const fn target(&self) -> ClosePlanTarget {
        product_close_target(self.target)
    }

    /// Returns the workspace revision from which this request was prepared.
    #[must_use]
    pub const fn expected_version(&self) -> WorkspaceVersion {
        self.expected
    }

    pub(crate) fn into_engine_input(
        self,
        authority_domain: EngineAuthorityDomainId,
    ) -> Result<EngineInput, PreparedCloseRequestAuthorityMismatch> {
        if self.authority_domain != authority_domain {
            return Err(PreparedCloseRequestAuthorityMismatch);
        }
        Ok(EngineInput::RequestContentClose {
            expected: self.expected,
            target: self.target,
        })
    }
}

impl fmt::Debug for PreparedCloseRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedCloseRequest")
            .field("expected", &self.expected)
            .field("target", &self.target())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("prepared close request belongs to another dockspace authority domain")]
pub(crate) struct PreparedCloseRequestAuthorityMismatch;

/// One application-facing item decision in a close plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DockspaceCloseItem {
    item: ItemId,
    capability: CloseCapability,
    token: CloseDecisionToken,
    state: CloseItemDecisionState,
}

/// Product-visible phase of a close request.
///
/// Exact native destruction, cancellation, effect, and recovery states remain
/// private to the core and native coordinator.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DockspaceClosePhase {
    /// At least one application close decision is still unresolved.
    AwaitingDecision,
    /// Core is applying or retiring the request and no application decision is pending.
    Settling,
    /// The semantic close transaction was applied.
    Applied,
    /// The request became terminal without applying its semantic close transaction.
    NotApplied,
}

impl DockspaceClosePhase {
    const fn from_core(plan: &ClosePlan) -> Self {
        Self::from_core_parts(plan.phase(), plan.target())
    }

    const fn from_core_parts(phase: CoreClosePlanPhase, target: ClosePlanTarget) -> Self {
        match phase {
            CoreClosePlanPhase::Requested
            | CoreClosePlanPhase::Resolving
            | CoreClosePlanPhase::Deferred => Self::AwaitingDecision,
            CoreClosePlanPhase::Approved
            | CoreClosePlanPhase::EffectEmitted
            | CoreClosePlanPhase::AwaitingDestroyed
            | CoreClosePlanPhase::CancelRequested
            | CoreClosePlanPhase::Indeterminate => Self::Settling,
            CoreClosePlanPhase::Applied => Self::Applied,
            CoreClosePlanPhase::Vetoed => match target {
                ClosePlanTarget::Surface { .. } => Self::Settling,
                ClosePlanTarget::Item { .. } | ClosePlanTarget::Root { .. } => Self::NotApplied,
            },
            CoreClosePlanPhase::Stale
            | CoreClosePlanPhase::Cancelled
            | CoreClosePlanPhase::ExternallyDestroyedUnproved => Self::NotApplied,
        }
    }

    /// Returns whether the application still owns an unresolved decision.
    #[must_use]
    pub const fn requires_decision(self) -> bool {
        matches!(self, Self::AwaitingDecision)
    }

    /// Returns whether core has finished the semantic close request.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Applied | Self::NotApplied)
    }
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
    phase: DockspaceClosePhase,
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
            phase: DockspaceClosePhase::from_core(plan),
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
    pub const fn phase(&self) -> DockspaceClosePhase {
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
                phase: _,
            } => Self::Recorded { request, item },
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

#[cfg(test)]
mod tests {
    use super::DockspaceClosePhase;
    use crate::close_plan::{ClosePlanPhase, ClosePlanTarget, SurfaceCloseDisposition};
    use crate::ids::{ItemId, RootId, SurfaceId};

    #[test]
    fn product_phase_hides_native_close_lifecycle_details() {
        let item = ClosePlanTarget::Item {
            item: ItemId::new(1),
        };
        let root = ClosePlanTarget::Root {
            root: RootId::new(2),
        };
        let surface = ClosePlanTarget::Surface {
            surface: SurfaceId::new(3),
            disposition: SurfaceCloseDisposition::RetainLayout,
        };

        for phase in [
            ClosePlanPhase::Requested,
            ClosePlanPhase::Resolving,
            ClosePlanPhase::Deferred,
        ] {
            assert_eq!(
                DockspaceClosePhase::from_core_parts(phase, item),
                DockspaceClosePhase::AwaitingDecision
            );
        }

        for phase in [
            ClosePlanPhase::Approved,
            ClosePlanPhase::EffectEmitted,
            ClosePlanPhase::AwaitingDestroyed,
            ClosePlanPhase::CancelRequested,
            ClosePlanPhase::Indeterminate,
        ] {
            assert_eq!(
                DockspaceClosePhase::from_core_parts(phase, surface),
                DockspaceClosePhase::Settling
            );
        }

        assert_eq!(
            DockspaceClosePhase::from_core_parts(ClosePlanPhase::Vetoed, surface),
            DockspaceClosePhase::Settling
        );
        assert_eq!(
            DockspaceClosePhase::from_core_parts(ClosePlanPhase::Vetoed, item),
            DockspaceClosePhase::NotApplied
        );
        assert_eq!(
            DockspaceClosePhase::from_core_parts(ClosePlanPhase::Vetoed, root),
            DockspaceClosePhase::NotApplied
        );
        assert_eq!(
            DockspaceClosePhase::from_core_parts(ClosePlanPhase::Applied, item),
            DockspaceClosePhase::Applied
        );

        for phase in [
            ClosePlanPhase::Stale,
            ClosePlanPhase::Cancelled,
            ClosePlanPhase::ExternallyDestroyedUnproved,
        ] {
            assert_eq!(
                DockspaceClosePhase::from_core_parts(phase, surface),
                DockspaceClosePhase::NotApplied
            );
        }
    }

    #[test]
    fn product_phase_reports_only_application_owned_decisions_and_terminality() {
        assert!(DockspaceClosePhase::AwaitingDecision.requires_decision());
        assert!(!DockspaceClosePhase::Settling.requires_decision());
        assert!(!DockspaceClosePhase::Applied.requires_decision());
        assert!(!DockspaceClosePhase::NotApplied.requires_decision());

        assert!(!DockspaceClosePhase::AwaitingDecision.is_terminal());
        assert!(!DockspaceClosePhase::Settling.is_terminal());
        assert!(DockspaceClosePhase::Applied.is_terminal());
        assert!(DockspaceClosePhase::NotApplied.is_terminal());
    }
}

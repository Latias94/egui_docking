//! Stable application close decisions, targets, and opaque decision identities.

pub(crate) mod plan;

pub use plan::{
    CloseDecision, CloseDecisionToken, CloseItemDecisionState, ClosePlanTarget,
    ClosePlanTargetKind, CloseRequestId, DeferredCloseDecision, DeferredCloseToken,
    SurfaceCloseDisposition,
};

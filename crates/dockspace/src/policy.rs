//! Declarative product policy for docking operations and presentation.

pub(crate) mod rules;

pub use rules::{
    CentralNodePolicy, CloseCapability, DockClassId, DockItemRule, DockOperation, DockPayloadKind,
    DockPolicy, DockPresentationMode, DockSourceRule, DockSurfaceRule, DockTargetRule,
    DockTargetRuleKey, TabBarInteraction, TabBarPolicy, TabBarVisibility, TearOffPresentation,
};
pub(crate) use rules::{
    DockContainedTransformPolicyRequest, DockDropOperation, DockDropTargetFacts,
    DockPayloadPolicyFacts, DockPolicyRequest, DockPolicySnapshot, DockPresentationPolicyRequest,
    DockPresentationTarget, DockResizePolicyRequest, DockSurfaceRecoveryPolicyRequest,
    DockSurfaceRecoveryRootFacts, PolicyDecision, PolicyRejection, PolicyRevision,
};
#[cfg(test)]
pub(crate) use rules::{DockDropPolicyRequest, PolicyRuleScope};

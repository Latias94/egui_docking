//! Renderer-neutral docking state and protocol engine.
//!
//! `dockspace` owns docking topology, validation, layout projection, interaction
//! generations, and native-surface lifecycle. UI adapters submit typed facts and
//! intents; they never mutate docking state directly.
//!
//! Raw reducer, scene, provider, and ingress protocols are implementation details,
//! even when every crate feature is enabled:
//!
//! ```compile_fail
//! use dockspace::backend::engine::DockEngine;
//! ```

#![forbid(unsafe_code)]

#[cfg(test)]
extern crate self as dockspace;

#[cfg(test)]
pub mod backend_ingress;
#[cfg(not(test))]
#[allow(dead_code, unused_imports)]
mod backend_ingress;
#[allow(dead_code, unused_imports)]
mod canonical;
#[cfg_attr(not(test), allow(dead_code))]
mod close_plan;
/// Stable application close decisions, targets, and opaque decision identities.
pub mod close {
    pub use crate::close_plan::{
        CloseDecision, CloseDecisionToken, CloseItemDecisionState, ClosePlanPhase, ClosePlanTarget,
        ClosePlanTargetKind, CloseRequestId, DeferredCloseDecision, DeferredCloseToken,
        SurfaceCloseDisposition, SurfaceCloseRequest, SurfaceContainedRehomeTarget,
        SurfaceMainRehomeTarget, SurfaceRehomeTarget,
    };
}
#[allow(dead_code, unused_imports)]
mod command;
#[cfg(test)]
pub mod coordinates;
#[cfg(not(test))]
#[allow(dead_code, unused_imports)]
mod coordinates;
#[cfg(feature = "serde")]
mod document;
#[allow(dead_code)]
mod drop_guide;
#[cfg(test)]
pub mod drop_resolver;
#[cfg(not(test))]
#[allow(dead_code, unused_imports)]
mod drop_resolver;
#[allow(dead_code)]
mod drop_target;
#[cfg(test)]
pub mod effect;
#[cfg(not(test))]
#[allow(dead_code, unused_imports)]
mod effect;
#[cfg(test)]
pub mod engine;
#[cfg(not(test))]
#[allow(dead_code, unused_imports)]
mod engine;
#[cfg(test)]
pub mod error;
#[cfg(not(test))]
mod error;
#[cfg(test)]
pub mod event;
#[cfg(not(test))]
#[allow(dead_code, unused_imports)]
mod event;
#[cfg(test)]
pub mod external_item_key;
#[cfg(not(test))]
#[allow(dead_code, unused_imports)]
mod external_item_key;
#[cfg(test)]
pub mod frame;
#[cfg(not(test))]
#[allow(dead_code, unused_imports)]
mod frame;
pub mod geometry;
#[allow(dead_code, unused_imports)]
mod graph;
#[cfg(test)]
pub mod hit_region;
#[cfg(not(test))]
#[allow(dead_code, unused_imports)]
mod hit_region;
mod ids;
#[allow(dead_code)]
mod intent;
#[cfg(test)]
pub mod interaction;
#[cfg(not(test))]
#[allow(dead_code, unused_imports)]
mod interaction;
mod journal_presentation;
#[cfg(test)]
pub mod layout;
#[cfg(not(test))]
#[allow(dead_code, unused_imports)]
mod layout;
pub mod model;
mod operation;
#[cfg(feature = "serde")]
mod persistence;
#[cfg(test)]
pub mod platform;
#[cfg(not(test))]
#[allow(dead_code, unused_imports)]
mod platform;
mod platform_provider;
#[cfg(test)]
pub mod pointer_journal;
#[cfg(not(test))]
#[allow(dead_code, unused_imports)]
mod pointer_journal;
#[cfg(test)]
pub mod pointer_receiver;
#[cfg(not(test))]
#[allow(dead_code, unused_imports)]
mod pointer_receiver;
#[path = "policy.rs"]
#[cfg_attr(not(test), allow(dead_code))]
mod policy_impl;
/// Declarative product policy for docking operations and presentation.
pub mod policy {
    pub use crate::policy_impl::{
        CentralNodePolicy, CloseCapability, DockClassId, DockItemRule, DockOperation,
        DockPayloadKind, DockPolicy, DockPresentationMode, DockSourceRule, DockSurfaceRule,
        DockTargetRule, DockTargetRuleKey, TabBarInteraction, TabBarPolicy, TabBarVisibility,
        TearOffPresentation,
    };
    pub(crate) use crate::policy_impl::{
        DockContainedTransformPolicyRequest, DockDropOperation, DockDropTargetFacts,
        DockPayloadPolicyFacts, DockPolicyRequest, DockPolicySnapshot,
        DockPresentationPolicyRequest, DockPresentationTarget, DockResizePolicyRequest,
        DockSurfaceRecoveryPolicyRequest, DockSurfaceRecoveryRootFacts, DockTabBarPolicyRequest,
        PolicyDecision, PolicyRejection, PolicyRevision,
    };
    #[cfg(test)]
    pub(crate) use crate::policy_impl::{DockDropPolicyRequest, PolicyRuleScope};
}
pub mod presentation_config;
#[cfg(test)]
pub mod presentation_hit;
#[cfg(not(test))]
#[allow(dead_code, unused_imports)]
mod presentation_hit;
#[cfg(test)]
pub mod presentation_observation;
#[cfg(not(test))]
#[allow(dead_code, unused_imports)]
mod presentation_observation;
#[cfg(test)]
pub mod retention;
#[cfg(not(test))]
#[allow(dead_code, unused_imports)]
mod retention;
pub mod runtime;
#[cfg(test)]
pub mod scene;
#[cfg(not(test))]
#[allow(dead_code, unused_imports)]
mod scene;
mod scene_compiler;
#[allow(dead_code)]
mod scene_manifest;
#[cfg(test)]
pub mod semantic_input;
#[cfg(not(test))]
#[allow(dead_code, unused_imports)]
mod semantic_input;
#[cfg(test)]
pub mod semantic_manifest;
#[cfg(not(test))]
#[allow(dead_code, unused_imports)]
mod semantic_manifest;
mod splitter_junction_index;
#[cfg(test)]
pub mod surface_recovery;
#[cfg(not(test))]
#[allow(dead_code, unused_imports)]
mod surface_recovery;
#[allow(dead_code)]
mod tab_strip;
#[allow(dead_code, unused_imports)]
mod transaction;
#[cfg(test)]
pub mod transition;
#[cfg(not(test))]
#[allow(dead_code, unused_imports)]
mod transition;
#[allow(dead_code, unused_imports)]
mod validation;
#[cfg(test)]
pub mod viewport;
#[cfg(not(test))]
#[allow(dead_code, unused_imports)]
mod viewport;
#[cfg(test)]
pub mod viewport_focus;
#[cfg(not(test))]
#[allow(dead_code, unused_imports)]
mod viewport_focus;
#[cfg(all(feature = "serde", test))]
pub mod viewport_persistence;
#[cfg(all(feature = "serde", not(test)))]
#[allow(dead_code)]
mod viewport_persistence;
#[cfg(test)]
pub mod viewport_registry;
#[cfg(not(test))]
#[allow(dead_code, unused_imports)]
mod viewport_registry;
mod workspace;

#[cfg(test)]
mod behavior_tests;

pub(crate) use workspace::RootPresentationOwner;

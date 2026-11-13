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
//! use dockspace::engine::DockEngine;
//! ```
//!
//! ```compile_fail
//! use dockspace::graph::Workspace;
//! ```
//!
//! ```compile_fail
//! use dockspace::ids::NodeId;
//! ```
//!
//! ```compile_fail
//! use dockspace::scene::SurfaceSceneStamp;
//! ```
//!
//! ```compile_fail
//! use dockspace::platform_provider::PlatformObservationLease;
//! ```
//!
//! ```compile_fail
//! use dockspace::persistence::WorkspaceSnapshot;
//! ```
//!
//! ```compile_fail
//! use dockspace::close::ClosePlanPhase;
//! ```

#![forbid(unsafe_code)]

#[cfg(test)]
extern crate self as dockspace;

#[cfg(test)]
pub mod backend_ingress;
#[cfg(not(test))]
mod backend_ingress;
pub(crate) use graph::canonical;
mod close_plan;
/// Stable application close decisions, targets, and opaque decision identities.
pub mod close {
    #[cfg(test)]
    pub use crate::close_plan::ClosePlanPhase;
    pub use crate::close_plan::{
        CloseDecision, CloseDecisionToken, CloseItemDecisionState, ClosePlanTarget,
        ClosePlanTargetKind, CloseRequestId, DeferredCloseDecision, DeferredCloseToken,
        SurfaceCloseDisposition,
    };
}
mod command;
#[cfg(test)]
pub mod coordinates;
#[cfg(not(test))]
mod coordinates;
#[cfg(test)]
mod docking_cost;
#[cfg(feature = "serde")]
mod document;
mod drop_guide;
#[cfg(test)]
pub mod drop_resolver;
#[cfg(not(test))]
mod drop_resolver;
mod drop_target;
#[cfg(test)]
pub mod effect;
#[cfg(not(test))]
mod effect;
#[cfg(test)]
pub mod engine;
#[cfg(not(test))]
mod engine;
#[cfg(test)]
pub mod error;
#[cfg(not(test))]
mod error;
#[cfg(test)]
pub mod event;
#[cfg(not(test))]
mod event;
#[cfg(test)]
pub mod external_item_key;
#[cfg(all(feature = "serde", not(test)))]
mod external_item_key;
#[cfg(test)]
pub mod frame;
#[cfg(not(test))]
mod frame;
pub mod geometry;
mod graph;
#[cfg(test)]
pub mod hit_region;
#[cfg(not(test))]
mod hit_region;
mod ids;
mod intent;
#[cfg(test)]
pub mod interaction;
#[cfg(not(test))]
mod interaction;
mod journal_presentation;
#[cfg(test)]
pub mod layout;
#[cfg(not(test))]
mod layout;
pub mod model;
#[cfg(feature = "motion")]
pub mod motion;
pub(crate) use graph::operation;
#[cfg(feature = "serde")]
mod persistence;
#[cfg(test)]
pub mod platform;
#[cfg(not(test))]
mod platform;
mod platform_provider;
#[cfg(test)]
pub mod pointer_journal;
#[cfg(not(test))]
mod pointer_journal;
#[cfg(test)]
pub mod pointer_receiver;
#[cfg(not(test))]
mod pointer_receiver;
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
        DockSurfaceRecoveryPolicyRequest, DockSurfaceRecoveryRootFacts, PolicyDecision,
        PolicyRejection, PolicyRevision,
    };
    #[cfg(test)]
    pub(crate) use crate::policy_impl::{DockDropPolicyRequest, PolicyRuleScope};
}
mod presentation_config;
#[cfg(test)]
pub mod presentation_hit;
#[cfg(not(test))]
mod presentation_hit;
#[cfg(test)]
pub mod presentation_observation;
#[cfg(not(test))]
mod presentation_observation;
#[cfg(test)]
pub mod retention;
#[cfg(not(test))]
mod retention;
pub mod runtime;
#[cfg(test)]
pub mod scene;
#[cfg(not(test))]
mod scene;
mod scene_compiler;
mod scene_manifest;
mod splitter_junction_index;
#[cfg(test)]
pub mod surface_recovery;
#[cfg(not(test))]
mod surface_recovery;
mod tab_strip;
mod target_field;
mod transaction;
#[cfg(test)]
pub mod transition;
#[cfg(not(test))]
mod transition;
mod validation;
#[cfg(test)]
pub mod viewport;
#[cfg(not(test))]
mod viewport;
#[cfg(test)]
pub mod viewport_focus;
#[cfg(not(test))]
mod viewport_focus;
#[cfg(all(feature = "serde", test))]
pub mod viewport_persistence;
#[cfg(all(feature = "serde", not(test)))]
mod viewport_persistence;
#[cfg(test)]
pub mod viewport_registry;
#[cfg(not(test))]
mod viewport_registry;
mod workspace;

#[cfg(test)]
mod behavior_tests;

pub(crate) use workspace::RootPresentationOwner;

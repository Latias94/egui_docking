//! Renderer-neutral docking state and protocol engine.
//!
//! `dockspace` owns docking topology, validation, layout projection, interaction
//! generations, and native-surface lifecycle. UI adapters submit typed facts and
//! intents; they never mutate docking state directly.

#![forbid(unsafe_code)]

#[cfg(test)]
extern crate self as dockspace;

#[cfg(feature = "backend")]
pub mod backend;
#[cfg(test)]
pub mod backend_ingress;
#[cfg(not(test))]
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod backend_ingress;
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod canonical;
mod close_plan;
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod command;
#[cfg(test)]
pub mod coordinates;
#[cfg(not(test))]
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod coordinates;
#[cfg(all(feature = "serde", any(feature = "backend", test)))]
pub mod document;
#[cfg(all(feature = "serde", not(any(feature = "backend", test))))]
#[allow(dead_code)]
mod document;
#[cfg(any(feature = "backend", test))]
pub mod drop_guide;
#[cfg(not(any(feature = "backend", test)))]
#[allow(dead_code)]
mod drop_guide;
#[cfg(test)]
pub mod drop_resolver;
#[cfg(not(test))]
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod drop_resolver;
#[cfg(any(feature = "backend", test))]
pub mod drop_target;
#[cfg(not(any(feature = "backend", test)))]
#[allow(dead_code)]
mod drop_target;
#[cfg(test)]
pub mod effect;
#[cfg(not(test))]
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod effect;
#[cfg(test)]
pub mod engine;
#[cfg(not(test))]
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod engine;
pub mod error;
#[cfg(test)]
pub mod event;
#[cfg(not(test))]
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod event;
pub mod external_item_key;
#[cfg(test)]
pub mod frame;
#[cfg(not(test))]
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod frame;
pub mod geometry;
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod graph;
#[cfg(test)]
pub mod hit_region;
#[cfg(not(test))]
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod hit_region;
mod ids;
#[cfg(any(feature = "backend", test))]
pub mod intent;
#[cfg(not(any(feature = "backend", test)))]
#[allow(dead_code)]
mod intent;
#[cfg(test)]
pub mod interaction;
#[cfg(not(test))]
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod interaction;
mod journal_presentation;
#[cfg(test)]
pub mod layout;
#[cfg(not(test))]
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod layout;
pub mod model;
mod operation;
#[cfg(all(feature = "serde", any(feature = "backend", test)))]
pub mod persistence;
#[cfg(all(feature = "serde", not(any(feature = "backend", test))))]
#[allow(dead_code)]
mod persistence;
#[cfg(test)]
pub mod platform;
#[cfg(not(test))]
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod platform;
mod platform_provider;
#[cfg(test)]
pub mod pointer_journal;
#[cfg(not(test))]
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod pointer_journal;
#[cfg(test)]
pub mod pointer_receiver;
#[cfg(not(test))]
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod pointer_receiver;
pub mod policy;
pub mod presentation_config;
#[cfg(test)]
pub mod presentation_hit;
#[cfg(not(test))]
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod presentation_hit;
#[cfg(test)]
pub mod presentation_observation;
#[cfg(not(test))]
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod presentation_observation;
#[cfg(test)]
pub mod retention;
#[cfg(not(test))]
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod retention;
pub mod runtime;
#[cfg(test)]
pub mod scene;
#[cfg(not(test))]
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod scene;
mod scene_compiler;
#[cfg(any(feature = "backend", test))]
pub mod scene_manifest;
#[cfg(not(any(feature = "backend", test)))]
#[allow(dead_code)]
mod scene_manifest;
#[cfg(test)]
pub mod semantic_input;
#[cfg(not(test))]
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod semantic_input;
#[cfg(test)]
pub mod semantic_manifest;
#[cfg(not(test))]
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod semantic_manifest;
mod splitter_junction_index;
#[cfg(test)]
pub mod surface_recovery;
#[cfg(not(test))]
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod surface_recovery;
#[cfg(any(feature = "backend", test))]
pub mod tab_strip;
#[cfg(not(any(feature = "backend", test)))]
#[allow(dead_code)]
mod tab_strip;
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod transaction;
#[cfg(test)]
pub mod transition;
#[cfg(not(test))]
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod transition;
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod validation;
pub mod viewport;
#[cfg(test)]
pub mod viewport_focus;
#[cfg(not(test))]
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod viewport_focus;
#[cfg(all(feature = "serde", any(feature = "backend", test)))]
pub mod viewport_persistence;
#[cfg(all(feature = "serde", not(any(feature = "backend", test))))]
#[allow(dead_code)]
mod viewport_persistence;
#[cfg(test)]
pub mod viewport_registry;
#[cfg(not(test))]
#[cfg_attr(not(feature = "backend"), allow(dead_code, unused_imports))]
mod viewport_registry;
mod workspace;

#[cfg(test)]
mod behavior_tests;

pub use close_plan::{
    CloseAdvanceOutcome, CloseAuthority, CloseCancellationProof, CloseCancellationState,
    CloseDecision, CloseDecisionToken, CloseDestroyedProof, CloseDestructionState,
    CloseInertReason, CloseItemDecisionState, CloseItemRequirement, CloseLifecycleAction,
    CloseNativeSettlement, ClosePlan, ClosePlanItem, ClosePlanLookup, ClosePlanPhase,
    ClosePlanTarget, ClosePlanTargetKind, CloseRequestId, CloseResolutionOutcome,
    DeferredCloseDecision, DeferredCloseToken, NativeCloseEdge, SurfaceCloseDisposition,
    SurfaceCloseRequest, SurfaceContainedRehomeTarget, SurfaceMainRehomeTarget,
    SurfaceRehomeTarget,
};
pub use viewport::CloseObservationGeneration;
pub(crate) use workspace::RootPresentationOwner;

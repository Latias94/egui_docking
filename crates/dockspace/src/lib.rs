//! Renderer-neutral docking state and protocol engine.
//!
//! `dockspace` owns docking topology, validation, layout projection, interaction
//! generations, and native-surface lifecycle. UI adapters submit typed facts and
//! intents; they never mutate docking state directly.

#![forbid(unsafe_code)]

pub mod backend_ingress;
pub mod canonical;
mod close_plan;
pub mod command;
pub mod coordinates;
#[cfg(feature = "serde")]
pub mod document;
pub mod drop_guide;
pub mod drop_resolver;
pub mod drop_target;
pub mod effect;
pub mod engine;
pub mod error;
pub mod event;
pub mod external_item_key;
pub mod frame;
pub mod geometry;
pub mod graph;
pub mod hit_region;
pub mod ids;
pub mod intent;
pub mod interaction;
mod journal_presentation;
pub mod layout;
mod operation;
#[cfg(feature = "serde")]
pub mod persistence;
pub mod platform;
mod platform_provider;
pub mod pointer_journal;
pub mod pointer_receiver;
pub mod policy;
pub mod presentation_config;
pub mod presentation_hit;
pub mod presentation_observation;
pub mod retention;
pub mod runtime;
pub mod scene;
mod scene_compiler;
pub mod scene_manifest;
pub mod semantic_input;
pub mod semantic_manifest;
mod splitter_junction_index;
pub mod surface_recovery;
pub mod tab_strip;
pub mod transaction;
pub mod transition;
pub mod validation;
pub mod viewport;
pub mod viewport_focus;
#[cfg(feature = "serde")]
pub mod viewport_persistence;
pub mod viewport_registry;
mod workspace;

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
pub use platform_provider::{PlatformObservationAuthorityError, PlatformObservationLease};
pub use scene_compiler::{PresentationCompilationError, SceneCompilationError};
pub use surface_recovery::{
    ConvertedMainRecovery, RootRecoveryAnchor, SurfaceRecoveryBlockedReason,
    SurfaceRecoveryBootstrap, SurfaceRecoveryError, SurfaceRecoveryTarget,
};
pub use viewport::CloseObservationGeneration;
pub use workspace::RootPresentationOwner;

//! Unstable adapter protocol for renderer and native-runtime implementations.
//!
//! This namespace is intentionally excluded from the default product API. Its
//! types may change whenever the core authority protocol changes. Applications
//! should use [`crate::runtime`] instead.

/// Canonicalization internals used by backend diagnostics and tests.
pub mod canonical {
    pub use crate::canonical::*;
}

/// Checked graph mutation commands and captured sources.
pub mod command {
    pub use crate::command::*;
}

/// Runtime workspace graph records and builders.
pub mod graph {
    pub use crate::graph::*;
}

/// Stable and runtime identity types used by adapter protocols.
pub mod ids {
    pub use crate::ids::*;
}

/// Atomic workspace transaction protocol.
pub mod transaction {
    pub use crate::transaction::*;
}

/// Strict workspace validation internals.
pub mod validation {
    pub use crate::validation::*;
}

/// Ordered backend ingress records and receipts.
pub mod ingress {
    pub use crate::backend_ingress::*;
}

/// Coordinate authority and conversion protocol.
pub mod coordinates {
    pub use crate::coordinates::*;
}

/// Core-owned drop resolution protocol.
pub mod drop_resolver {
    pub use crate::drop_resolver::*;
}

/// Docking-guide identities and structural records used by adapter backends.
pub mod drop_guide {
    pub use crate::drop_guide::*;
}

/// Core-owned drop target identities and availability records.
pub mod drop_target {
    pub use crate::drop_target::*;
}

/// Native effect ledger and typed dispatch results.
pub mod effect {
    pub use crate::effect::*;
}

/// Committed reducer events and their exact causes.
pub mod event {
    pub use crate::event::*;
}

/// Reducer and host-frame engine protocol.
pub mod engine {
    pub use crate::engine::*;
}

/// Native lifecycle frame state machines.
pub mod frame {
    pub use crate::frame::*;
}

/// Internal geometric hit regions.
pub mod hit_region {
    pub use crate::hit_region::*;
}

/// Core-owned interaction state machines and outcomes.
pub mod interaction {
    pub use crate::interaction::*;
}

/// Typed renderer intents and internal authority facts.
pub mod intent {
    pub use crate::intent::*;
}

/// Internal layout solver records.
pub mod layout {
    pub use crate::layout::*;
}

/// Typed platform observations and capability roster.
pub mod platform {
    pub use crate::platform::*;
    pub use crate::platform_provider::{
        PlatformObservationAuthorityError, PlatformObservationLease,
    };
}

/// Lossless pointer edge journal protocol.
pub mod pointer_journal {
    pub use crate::pointer_journal::*;
}

/// Presented receiver challenges and exact receipts.
pub mod pointer_receiver {
    pub use crate::pointer_receiver::*;
}

/// Core-compiled hit graph records.
pub mod presentation_hit {
    pub use crate::presentation_hit::*;
}

/// Affine presentation observation protocol.
pub mod presentation_observation {
    pub use crate::presentation_observation::*;
}

/// Runtime retention and compaction protocol.
pub mod retention {
    pub use crate::retention::*;
}

/// Compiled presentation scene internals.
pub mod scene {
    pub use crate::scene::*;
    pub use crate::scene_compiler::{PresentationCompilationError, SceneCompilationError};
    pub use crate::workspace::RootPresentationOwner;
}

/// Exact scene measurement and contribution manifests.
pub mod scene_manifest {
    pub use crate::scene_manifest::*;
}

/// Ordered semantic input protocol.
pub mod semantic_input {
    pub use crate::semantic_input::*;
}

/// Core-owned semantic receiver manifest.
pub mod semantic_manifest {
    pub use crate::semantic_manifest::*;
}

/// Native surface recovery protocol.
pub mod surface_recovery {
    pub use crate::surface_recovery::*;
}

/// Tab-strip overflow, popup, and control identities used by adapter backends.
pub mod tab_strip {
    pub use crate::tab_strip::*;
}

/// Complete reducer transition diagnostics.
pub mod transition {
    pub use crate::transition::*;
}

/// Cross-viewport focus state machine.
pub mod viewport_focus {
    pub use crate::viewport_focus::*;
}

/// Exact native binding and viewport inventory protocol.
pub mod viewport_registry {
    pub use crate::viewport_registry::*;
}

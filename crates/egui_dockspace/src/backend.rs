//! Low-level host integration protocol for native and custom egui backends.
//!
//! Application code should normally use [`crate::Dockspace`]. This module is
//! feature-gated because its types form the adapter-to-host boundary, not the
//! product-facing docking API.

pub use crate::facade::backend::EguiNativeWorkAreaState;
pub use crate::facade::{
    DockspaceHostFrame, EguiFrameScheduleKey, EguiNativeConfigurationSession,
    EguiNativeInputSession, EguiNativePresentationSession, EguiOuterFrameCommit,
    EguiOuterHostFrame, ExactNativeViewport, NativeBindingError, NativeBindingRoster,
    NativeCoreRoute, NativeViewportIncarnation, PreparedEguiOuterFrameCommit,
};
pub use crate::presentation_settlement::{
    EguiOuterOutputBatch, EguiOuterSurfaceOutput, EguiRendererOutputDisposition,
};
pub use crate::receiver::{PaintReceiverFingerprint, PaintReceiverLookup};
pub use crate::response::{
    BackendEffectReceipt, DockspaceCommandOutcome, DockspaceCommandResult, HostFrameResponse,
    PresentationObservationSummary, SurfaceCommitResponse, SurfacePaintResponse,
};

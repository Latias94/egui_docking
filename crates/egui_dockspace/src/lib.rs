//! egui renderer and viewport adapter for [`dockspace`].

#![forbid(unsafe_code)]

#[cfg(test)]
extern crate self as egui_dockspace;

pub mod pane;
pub mod style;

mod builder;
mod drop_guides;
mod error;
#[path = "dockspace.rs"]
mod facade;
mod floating;
mod hit;
#[cfg(feature = "serde")]
mod persistence;
mod pointer_input;
mod presentation_settlement;
mod projection;
mod receiver;
mod render;
mod renderer;
mod response;
mod splits;
mod tabs;

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod behavior_tests;

pub use builder::DockspaceBuilder;
pub use error::DockspaceError;
pub use facade::{
    Dockspace, DockspaceHostFrame, EguiFrameScheduleKey, EguiNativeConfigurationSession,
    EguiNativeInputSession, EguiNativePresentationSession, EguiOuterFrameCommit,
    EguiOuterHostFrame, ExactNativeViewport, NativeBindingError, NativeBindingRoster,
    NativeCoreRoute, NativeViewportIncarnation, PreparedEguiOuterFrameCommit,
};
pub use pane::{PaneFocusState, PaneView};
#[cfg(feature = "serde")]
pub use persistence::{DockspaceDocumentLoad, DockspaceDocumentPersistenceError};
pub use presentation_settlement::{
    EguiNativePresentationSettlementError, EguiOuterSurfaceOutput, EguiPresentationResult,
    EguiPresentationSettlement,
};
pub use projection::ProjectionError;
pub use receiver::{PaintReceiverFingerprint, PaintReceiverLookup};
pub use render::EguiRendererError;
pub use response::{
    DockspaceCapability, DockspaceResponse, DockspaceSurfaceStatus, DockspaceUnavailableReason,
    HostFrameResponse, SurfaceCommitResponse, SurfaceFrameDisposition, SurfacePaintResponse,
};
pub use style::{DockStyle, DockStyleError};

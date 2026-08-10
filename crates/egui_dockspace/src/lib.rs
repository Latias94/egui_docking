//! egui renderer and viewport adapter for [`dockspace`].

#![forbid(unsafe_code)]

#[cfg(test)]
extern crate self as egui_dockspace;

pub mod pane;
pub mod style;

#[cfg(any(feature = "backend", test))]
pub mod backend;

mod builder;
#[cfg(any(feature = "backend", test))]
mod drop_guides;
mod error;
#[cfg(any(feature = "backend", test))]
#[path = "error_detail.rs"]
mod error_detail;
#[cfg(not(any(feature = "backend", test)))]
#[path = "product_error_detail.rs"]
mod error_detail;
// The legacy reducer-backed adapter is intentionally isolated from the default
// product facade. This prevents two docking authorities from coexisting in a
// normal crates.io build while preserving the migration/test backend.
#[cfg(any(feature = "backend", test))]
#[path = "dockspace.rs"]
mod facade;
#[cfg(not(any(feature = "backend", test)))]
#[path = "product_dockspace.rs"]
mod facade;
#[cfg(any(feature = "backend", test))]
mod floating;
#[cfg(any(feature = "backend", test))]
mod hit;
#[cfg(any(feature = "backend", test))]
mod output_ownership;
#[cfg(all(feature = "serde", feature = "backend"))]
mod persistence;
#[cfg(any(feature = "backend", test))]
mod pointer_input;
#[cfg(any(feature = "backend", test))]
mod presentation_settlement;
#[cfg(not(any(feature = "backend", test)))]
mod product_render;
#[cfg(any(feature = "backend", test))]
mod projection;
#[cfg(any(feature = "backend", test))]
mod receiver;
#[cfg(any(feature = "backend", test))]
mod render;
#[cfg(any(feature = "backend", test))]
mod renderer;
#[cfg(any(feature = "backend", test))]
#[path = "response.rs"]
mod response;
#[cfg(not(any(feature = "backend", test)))]
#[path = "product_response.rs"]
mod response;
#[cfg(any(feature = "backend", test))]
mod splits;
#[cfg(any(feature = "backend", test))]
mod tabs;

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod behavior_tests;

pub use builder::DockspaceBuilder;
pub use dockspace::model::{
    DockAnchor, DockEdge, DockFraction, DockPlacement, DockspaceActionOutcome,
    DockspaceActionRejection, DockspaceAxis, DockspaceContainedLayout, DockspaceContainedView,
    DockspaceLayout, DockspaceLayoutError, DockspaceNode, DockspaceNodeView, DockspaceRootLayout,
    DockspaceRootView, DockspaceSplitView, DockspaceSurfaceLayout, DockspaceSurfaceView,
    DockspaceTabsView, DockspaceView, FloatingPresentationId, InvalidDockFraction, ItemId,
    PreparedDockAction, RootId, SurfaceId, WorkspaceVersion,
};
pub use dockspace::{
    CloseDecision, CloseDecisionToken, CloseRequestId, DeferredCloseDecision, DeferredCloseToken,
};
pub use error::{DockspaceError, DockspaceErrorKind};
pub use facade::Dockspace;
pub use pane::{PaneFocusState, PaneView};
#[cfg(all(feature = "serde", feature = "backend"))]
pub use persistence::{DockspaceDocumentLoad, DockspaceDocumentPersistenceError};
pub use response::{
    DockspaceActionResult, DockspaceActionStatus, DockspaceCapability, DockspaceCloseItem,
    DockspaceCloseOutcome, DockspaceClosePlan, DockspaceCloseRequest, DockspaceCloseResult,
    DockspaceInteractionCapabilities, DockspaceMutation, DockspaceResponse,
    DockspaceSurfaceCommitStatus, DockspaceSurfaceStatus, DockspaceUnavailableReason,
};
#[cfg(any(feature = "backend", test))]
#[doc(hidden)]
pub use response::{DockspaceCommandOutcome, DockspaceCommandResult};
pub use style::{DockStyle, DockStyleError};

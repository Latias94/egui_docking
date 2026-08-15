//! egui renderer and viewport adapter for [`dockspace`].

#![forbid(unsafe_code)]

pub mod pane;
pub mod style;

mod builder;
mod error;
#[path = "product_error_detail.rs"]
mod error_detail;
#[path = "product_dockspace.rs"]
mod facade;
mod guide_paint;
#[cfg(feature = "native-render-support")]
#[doc(hidden)]
pub mod native_support;
mod product_render;
#[path = "product_response.rs"]
mod response;

pub use builder::DockspaceBuilder;
pub use dockspace::close::{
    CloseDecision, CloseDecisionToken, CloseItemDecisionState, ClosePlanPhase, ClosePlanTarget,
    ClosePlanTargetKind, CloseRequestId, DeferredCloseDecision, DeferredCloseToken,
    SurfaceCloseDisposition,
};
pub use dockspace::geometry::LogicalRect;
pub use dockspace::model::{
    DockAnchor, DockEdge, DockFraction, DockPlacement, DockspaceActionOutcome,
    DockspaceActionRejection, DockspaceAxis, DockspaceContainedLayout, DockspaceContainedView,
    DockspaceLayout, DockspaceLayoutError, DockspaceNode, DockspaceNodeView, DockspaceRootLayout,
    DockspaceRootView, DockspaceSplitView, DockspaceSurfaceLayout, DockspaceSurfaceView,
    DockspaceTabsView, DockspaceView, FloatingPresentationId, InvalidDockFraction, ItemId,
    PreparedDockAction, RootId, SurfaceId, WorkspaceVersion,
};
pub use dockspace::runtime::{
    DockspaceCloseInertReason, DockspaceCloseOutcome as DockspaceAppliedClose,
    DockspaceCloseRejection as DockspaceCloseApplicationRejection, DockspaceCloseRequestRejection,
    DockspaceCloseResolution, PreparedCloseRequest,
};
#[cfg(feature = "serde")]
pub use dockspace::runtime::{
    DockspaceDocumentBootstrap, DockspaceDocumentId, DockspacePersistenceError,
    DockspacePersistenceErrorKind,
};
pub use error::{DockspaceError, DockspaceErrorKind};
pub use facade::Dockspace;
pub use pane::{PaneFocusState, PaneView};
pub use response::{
    DockspaceActionResult, DockspaceActionStatus, DockspaceCapability, DockspaceCloseItem,
    DockspaceCloseOutcome, DockspaceClosePlan, DockspaceCloseRequest, DockspaceCloseRequestResult,
    DockspaceCloseRequestStatus, DockspaceCloseResult, DockspaceInteractionCapabilities,
    DockspaceMutation, DockspaceResponse, DockspaceSurfaceCommitStatus, DockspaceSurfaceStatus,
    DockspaceUnavailableReason,
};
pub use style::{DockStyle, DockStyleError};

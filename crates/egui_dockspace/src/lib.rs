//! egui renderer and viewport adapter for [`dockspace`].
//!
//! Core-owned close lifecycle phases are intentionally not re-exported:
//!
//! ```compile_fail
//! use egui_dockspace::ClosePlanPhase;
//! ```

#![forbid(unsafe_code)]

pub mod drag_preview;
pub mod drop_preview;
pub mod pane;
pub mod style;
pub mod tab;

mod builder;
mod error;
#[path = "product_error_detail.rs"]
mod error_detail;
#[path = "product_dockspace.rs"]
mod facade;
mod guide_paint;
mod input;
#[cfg(feature = "native-render-support")]
#[doc(hidden)]
pub mod native_support;
mod output_proof;
mod pass_settlement;
mod product_focus;
mod product_pointer;
mod product_render;
#[path = "product_response.rs"]
mod response;
#[path = "product_render/touch_menu.rs"]
mod touch_menu;

pub use builder::DockspaceBuilder;
pub use dockspace::close::{
    CloseDecision, CloseDecisionToken, CloseItemDecisionState, ClosePlanTarget,
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
    PreparedDockAction, PreparedTabsGroup, RootId, SurfaceId, WorkspaceVersion,
};
pub use dockspace::runtime::SurfacePresentationResult;
pub use dockspace::runtime::{
    DockspaceCloseInertReason, DockspaceCloseOutcome as DockspaceAppliedClose, DockspaceClosePhase,
    DockspaceCloseRejection as DockspaceCloseApplicationRejection, DockspaceCloseRequestRejection,
    DockspaceCloseResolution, DockspaceContextMenuRequest, DockspaceFocusTarget,
    DockspaceManipulationCancelReason, DockspaceManipulationDelivery,
    DockspaceManipulationFeedback, DockspaceManipulationFeedbackId, DockspaceManipulationId,
    DockspaceManipulationPendingReason, DockspaceManipulationRejection,
    DockspaceManipulationStatus, DockspaceTouchEvent, PreparedCloseRequest,
};
#[cfg(feature = "serde")]
pub use dockspace::runtime::{
    DockspaceDocumentBootstrap, DockspaceDocumentId, DockspacePersistenceError,
    DockspacePersistenceErrorKind,
};
pub use drag_preview::{DockspaceDragSourceKind, DragPreview};
pub use drop_preview::{
    DockspaceDropAction, DockspaceDropTargetFeedback, DockspaceDropTargetScope, DropPreview,
    DropPreviewTab,
};
pub use error::{DockspaceError, DockspaceErrorKind};
pub use facade::Dockspace;
pub use input::{DockInputConfig, DockingActivation};
pub use pane::{PaneFocusState, PaneResponse, PaneView};
pub use pass_settlement::{DockspaceOutput, DockspaceOutputReceipt, run_ui};
pub use response::{
    DockspaceActionResult, DockspaceActionStatus, DockspaceCapability, DockspaceCloseItem,
    DockspaceCloseOutcome, DockspaceClosePlan, DockspaceCloseRequest, DockspaceCloseRequestResult,
    DockspaceCloseRequestStatus, DockspaceCloseResult, DockspaceInteractionCapabilities,
    DockspaceMutation, DockspaceResponse, DockspaceSurfaceCommitStatus, DockspaceSurfaceStatus,
    DockspaceUnavailableReason,
};
pub use style::{
    DockDensity, DockFrameVisuals, DockStyle, DockStyleError, DockVisualOverrides,
    TabCloseVisibility, TabGroupGripVisibility,
};
pub use tab::{Tab, TabBar, TabBarId, TabBarLayout, TabBarSlot, TabClose, TabVisuals};

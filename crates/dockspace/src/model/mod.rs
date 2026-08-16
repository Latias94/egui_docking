//! Product-facing docking layout and read-only query types.
//!
//! This module deliberately hides runtime graph identities. Applications describe layouts with
//! stable item, root, surface, and contained-floating identities; the core compiles that
//! declaration into its validated runtime graph.

mod action;
mod layout;
mod revision;
mod view;

pub use crate::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
pub use action::{
    DockAnchor, DockEdge, DockFraction, DockPlacement, DockspaceActionOutcome,
    DockspaceActionRejection, InvalidDockFraction, NativeWindowPlacement, PreparedDockAction,
};
pub(crate) use action::{PreparedDockActionAuthorityMismatch, ProductAction};
pub use layout::{
    DockspaceAxis, DockspaceContainedLayout, DockspaceLayout, DockspaceLayoutError, DockspaceNode,
    DockspaceRootLayout, DockspaceSurfaceLayout,
};
pub use revision::WorkspaceVersion;
pub use view::{
    DockspaceContainedView, DockspaceItemView, DockspaceNodeView, DockspaceRootView,
    DockspaceSplitView, DockspaceSurfaceView, DockspaceTabsView, DockspaceView,
};

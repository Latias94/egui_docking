//! Product-facing docking layout and read-only query types.
//!
//! This module deliberately hides runtime graph identities. Applications describe layouts with
//! stable item, root, surface, and contained-floating identities; the core compiles that
//! declaration into its validated runtime graph.

mod action;
mod layout;
mod view;

pub use crate::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
pub(crate) use action::ProductAction;
pub use action::{
    DockAnchor, DockEdge, DockFraction, DockPlacement, DockspaceActionOutcome,
    DockspaceActionRejection, InvalidDockFraction,
};
pub use layout::{
    DockspaceAxis, DockspaceContainedLayout, DockspaceLayout, DockspaceLayoutError, DockspaceNode,
    DockspaceRootLayout, DockspaceSurfaceLayout,
};
pub use view::{
    DockspaceContainedView, DockspaceNodeView, DockspaceRootView, DockspaceSplitView,
    DockspaceSurfaceView, DockspaceTabsView, DockspaceView,
};

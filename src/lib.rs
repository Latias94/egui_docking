#![forbid(unsafe_code)]

pub mod multi_viewport;
pub mod dock_builder;
pub mod workspace;

pub use multi_viewport::{DockingMultiViewport, DockingMultiViewportOptions};
pub use dock_builder::{DockBuilder, DockNodeId, DockTreeBuilder, SplitDirection};
pub use workspace::{DetachedViewportLayout, WorkspaceLayout};
pub use multi_viewport::{
    backend_monitors_outer_rects_points, backend_mouse_hovered_viewport_id,
    backend_pointer_global_points, clear_backend_monitors_outer_rects_points,
    set_backend_monitors_outer_rects_points, BACKEND_MONITORS_OUTER_RECTS_POINTS_KEY,
    BACKEND_MOUSE_HOVERED_VIEWPORT_ID_KEY, BACKEND_POINTER_GLOBAL_POINTS_KEY,
};

#[cfg(feature = "persistence")]
pub use multi_viewport::{LayoutPersistenceError, LayoutSnapshot, LAYOUT_SNAPSHOT_VERSION};

#[cfg(feature = "persistence")]
pub use multi_viewport::{PaneRegistry, SimplePaneRegistry};

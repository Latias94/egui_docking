//! egui renderer and viewport adapter for [`dockspace`].

#![forbid(unsafe_code)]

pub mod pane;
pub mod presentation;
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
#[cfg(feature = "native")]
#[allow(
    dead_code,
    reason = "the crate-private provider boundary is consumed by the next U7 native runtime unit"
)]
mod platform_adapter;
mod projection;
mod renderer;
mod response;
mod splits;
mod tabs;

pub use ::dockspace;
pub use builder::DockspaceBuilder;
pub use error::DockspaceError;
pub use facade::Dockspace;
pub use pane::{PaneCloseResponse, PaneView};
#[cfg(feature = "serde")]
pub use persistence::DockspacePersistenceError;
pub use presentation::{ContainedPresentationIds, PresentationIdSource, TearOffMode};
pub use projection::ProjectionError;
pub use response::{
    DockspaceCapability, DockspaceInputRejection, DockspaceResponse, DockspaceSurfaceStatus,
    DockspaceUnavailableReason,
};
pub use style::{DockStyle, DockStyleError};

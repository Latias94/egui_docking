//! egui renderer and viewport adapter for [`dockspace`].

#![forbid(unsafe_code)]

#[cfg(test)]
extern crate self as egui_dockspace;

pub mod pane;
pub mod style;

#[cfg(any(feature = "backend", test))]
pub mod backend;

mod builder;
mod drop_guides;
mod error;
mod error_detail;
// These modules share implementation with the opt-in host backend. Their
// backend-only branches are intentionally dormant in the default product build.
#[cfg_attr(not(any(feature = "backend", test)), allow(dead_code, unused_imports))]
#[path = "dockspace.rs"]
mod facade;
mod floating;
mod hit;
mod output_ownership;
#[cfg(feature = "serde")]
mod persistence;
#[cfg_attr(not(any(feature = "backend", test)), allow(dead_code))]
mod pointer_input;
#[cfg_attr(not(any(feature = "backend", test)), allow(dead_code))]
mod presentation_settlement;
mod projection;
#[cfg_attr(not(any(feature = "backend", test)), allow(dead_code))]
mod receiver;
#[cfg_attr(not(any(feature = "backend", test)), allow(dead_code))]
mod render;
mod renderer;
#[cfg_attr(not(any(feature = "backend", test)), allow(dead_code))]
mod response;
mod splits;
mod tabs;

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod behavior_tests;

pub use builder::DockspaceBuilder;
pub use error::{DockspaceError, DockspaceErrorKind};
pub use facade::Dockspace;
pub use pane::{PaneFocusState, PaneView};
#[cfg(feature = "serde")]
pub use persistence::{DockspaceDocumentLoad, DockspaceDocumentPersistenceError};
pub use response::{
    DockspaceCapability, DockspaceCloseItem, DockspaceCloseOutcome, DockspaceClosePlan,
    DockspaceCloseRequest, DockspaceCloseResult, DockspaceCommandOutcome, DockspaceCommandResult,
    DockspaceMutation, DockspaceResponse, DockspaceSurfaceCommitStatus, DockspaceSurfaceStatus,
    DockspaceUnavailableReason,
};
pub use style::{DockStyle, DockStyleError};

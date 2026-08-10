//! Minimal fork-backed native coordinator for `dockspace`.
//!
//! The crate deliberately owns no docking graph, renderer, platform effect
//! ledger, or presentation state machine. [`NativeCoordinator`] is a thin
//! bridge between the renderer-neutral [`dockspace::runtime::DockspaceSession`]
//! and the pinned eframe 0.36 native host seam.

#![forbid(unsafe_code)]

mod coordinator;
mod error;
mod event;
mod viewport_map;

pub use coordinator::{NativeCoordinator, NativeHostFrame};
pub use error::{
    NativeOutputBindingError, NativeOutputBindingErrorKind, NativeOutputReservationError,
    NativeOutputReservationErrorKind, NativeRuntimeError, NativeRuntimeErrorKind,
    NativeViewportBindingError,
};
pub use event::NativeWindowEventRecord;

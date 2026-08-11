//! Fork-backed native adapter for `dockspace`.
//!
//! The crate deliberately owns no docking graph, renderer, platform effect
//! ledger, or presentation state machine. The callback coordinator is an
//! implementation detail while the native product driver is being completed.
//! It bridges the renderer-neutral [`dockspace::runtime::DockspaceSession`] and
//! the pinned eframe 0.36 native host seam.

#![forbid(unsafe_code)]

mod coordinator;
mod effect_coordinator;
mod error;
mod event;
mod host_frame;
mod mailbox;
mod pointer_event;
mod viewport_map;

pub use error::{
    NativeOutputBindingError, NativeOutputBindingErrorKind, NativeRuntimeError,
    NativeRuntimeErrorKind, NativeViewportBindingError,
};
pub use event::NativeWindowEventRecord;

//! Fork-backed native adapter for `dockspace`.
//!
//! The crate deliberately owns no docking graph, renderer, platform effect
//! ledger, or presentation state machine. The callback coordinator is an
//! implementation detail while the native product driver is being completed.
//! It bridges the renderer-neutral [`dockspace::runtime::DockspaceSession`] and
//! the pinned eframe 0.36 native host seam.

#![forbid(unsafe_code)]

mod app;
mod application_action;
mod capabilities;
mod close_control;
mod coordinator;
mod deferred_viewport;
mod effect_coordinator;
mod error;
mod event;
mod focus_control;
mod host_frame;
mod input_control;
mod lifecycle_progress;
mod mailbox;
mod pass_actions;
mod pointer_event;
mod receiver;
mod retirement;
mod surface_driver;
mod viewport_callback;
mod viewport_map;
mod window_snapshot;
mod work_area;

pub use app::NativeDockspaceApp;
pub use application_action::NativeActionRequestError;
pub use close_control::NativeWindowClosePolicy;
pub use error::{NativeRuntimeError, NativeRuntimeErrorKind};

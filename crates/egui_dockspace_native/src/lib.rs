//! Fork-backed native adapter for `dockspace`.
//!
//! The crate deliberately owns no docking graph, renderer, platform effect
//! ledger, or presentation state machine. Its private callback coordinator
//! bridges the renderer-neutral [`dockspace::runtime::DockspaceSession`] and
//! the pinned eframe 0.37 native host seam. Applications use [`NativeDockspaceApp`]
//! to supply panes, window options, and application decisions.

#![forbid(unsafe_code)]

mod app;
mod application_action;
mod capabilities;
mod close_control;
mod command;
mod coordinator;
mod deferred_viewport;
mod effect_coordinator;
mod error;
mod event;
mod focus_control;
mod host_frame;
mod input_control;
mod layout_management;
mod lifecycle_progress;
mod mailbox;
mod notification;
mod pass_actions;
mod pending_work;
mod pointer_capture;
mod pointer_event;
mod pointer_observation;
mod receiver;
mod retirement;
mod style_management;
mod surface_driver;
mod viewport_callback;
mod viewport_map;
mod window_options;
mod window_snapshot;
mod window_title;
mod work_area;

pub use app::NativeDockspaceApp;
pub use application_action::{
    NativeActionFailure, NativeActionInterruption, NativeActionRequestError, NativeActionStatus,
};
pub use close_control::NativeWindowClosePolicy;
pub use command::{NativeCommand, NativeCommandRequestError};
pub use coordinator::NativeStorageLimit;
pub use dockspace::runtime::{
    DockspaceEvent, DockspaceEventId, DockspaceEventKind, DockspaceLayoutChange,
    DockspaceNotification, NativeSurfacePlacement,
};
pub use error::{NativeRuntimeError, NativeRuntimeErrorKind};
pub use layout_management::NativeLayoutRequestError;
pub use lifecycle_progress::{NativeCloseProgress, NativeOutputBacklog};
pub use notification::NativeNotificationContext;
pub use pending_work::{NativePendingWork, NativeWaitReason};
pub use style_management::NativeStyleRequestError;
pub use window_options::NativeChildWindowOptions;

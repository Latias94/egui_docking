//! Fork-backed native host runtime for `egui_dockspace`.
//!
//! This crate is intentionally unpublished. It is compiled only against the
//! release-pinned egui fork, while the base adapter remains compatible with the
//! official egui release.

#![forbid(unsafe_code)]

mod app;
mod close;
mod configuration;
mod effects;
mod error;
mod ingress;
mod persistence;
mod presentation;
mod receiver;
mod viewport;

pub use app::{NativeDockspaceApp, NativeRuntimeStatus};
pub use close::{
    AllowNativeClose, NativeCloseHandler, NativeCloseItemRequest, NativeDeferredCloseRequest,
    NativeSurfaceCloseContext, VetoNativeClose,
};
pub use error::NativeRuntimeError;
pub use persistence::{NATIVE_DOCKSPACE_DOCUMENT_STORAGE_KEY, restore_document_from_storage};
pub use viewport::{NativeSurfaceSpec, NativeViewportRoster};

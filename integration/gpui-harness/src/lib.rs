//! A retained Open GPUI adapter for the renderer-neutral docking runtime.
//!
//! Embed [`GpuiDockspace::element`] in an application's own GPUI view. The
//! standalone workbench is a consumer of the same interface.

mod config;
mod continuation;
mod element;
mod host;
mod input;
mod presentation;
mod smoke;
#[cfg(test)]
mod tests;
mod workbench;

pub use config::{DockspaceColors, DockspaceInputBindings, GpuiDockspaceConfig};
pub use host::{DockPane, GpuiDockspace};
pub use workbench::{Workbench, run, run_smoke};

pub(crate) use dockspace::model::ItemId;
pub(crate) use host::{HostState, SharedHost};
pub(crate) use workbench::CounterPanel;
#[cfg(test)]
pub(crate) use workbench::SURFACE;

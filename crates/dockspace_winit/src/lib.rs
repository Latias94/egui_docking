//! Input, scheduling and optional native-window helpers for winit-based hosts.
//!
//! The host owns its event loop, UI framework, renderer and docking session.
//! These helpers do not prove receiver delivery, pointer capture or presentation.
//! Mouse samples have no original OS timestamp and do not manufacture timed metadata.
//! Enable the `native` feature for exact-binding resource registries, window
//! observations and physical placement helpers.

mod coordinates;
mod mouse;
mod repaint;

#[cfg(any(feature = "native-windows", feature = "native-geometry"))]
pub mod native;

pub use coordinates::SurfaceCoordinates;
pub use mouse::{MouseInput, MouseSample, map_mouse_button};
pub use repaint::RepaintScheduler;
pub use winit as backend;

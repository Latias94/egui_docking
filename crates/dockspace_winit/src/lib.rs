//! Small input and scheduling helpers for winit-based dockspace hosts.
//!
//! The host owns its event loop, UI framework, renderer and docking session.
//! These helpers do not prove receiver delivery, pointer capture or presentation.
//! Mouse samples have no original OS timestamp and do not manufacture timed metadata.

mod coordinates;
mod mouse;
mod repaint;

pub use coordinates::SurfaceCoordinates;
pub use mouse::{MouseInput, MouseSample, map_mouse_button};
pub use repaint::RepaintScheduler;
pub use winit as backend;

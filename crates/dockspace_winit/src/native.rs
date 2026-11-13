//! Composable native-window integration for an application-owned winit host.
//!
//! The `native-windows` feature correlates physical resources with exact core-issued
//! window bindings, including committed Root continuations. The `native-geometry`
//! feature provides window observations and
//! placement helpers preserve the distinction between requested and observed
//! geometry. The application retains its event loop, renderer and session.
//!
//! These helpers grant no pointer capture, desktop hover, work-area, renderer
//! readiness or presentation authority. Execute and acknowledge native effects
//! through the core protocol, and report quiescence only after the application's
//! output, resource and callback owners have all retired.

#[cfg(feature = "native-geometry")]
mod observation;
#[cfg(feature = "native-geometry")]
mod placement;
#[cfg(feature = "native-windows")]
mod windows;

#[cfg(feature = "native-geometry")]
pub use observation::observe_window;
#[cfg(feature = "native-geometry")]
pub use placement::{WindowPlacementError, request_outer_bounds, window_attributes};
#[cfg(feature = "native-windows")]
pub use windows::{NativeWindowInsertError, NativeWindowRegistryError, NativeWindows};

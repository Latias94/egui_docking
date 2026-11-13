//! Bind the shared native registry to winit's existing resource identity.

use winit::window::WindowId;

/// Application resources indexed by exact native bindings and winit window IDs.
pub type NativeWindows<T> = dockspace::runtime::NativeWindows<WindowId, T>;

/// A rejected winit resource registration retaining the original resource.
pub type NativeWindowInsertError<T> = dockspace::runtime::NativeWindowInsertError<WindowId, T>;

/// A conflict between a winit ID and an exact core native-window binding.
pub type NativeWindowRegistryError = dockspace::runtime::NativeWindowRegistryError<WindowId>;

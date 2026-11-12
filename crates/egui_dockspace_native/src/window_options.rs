//! Application-owned appearance for managed child OS windows.

use std::sync::Arc;

use eframe::egui::{IconData, ViewportBuilder, WindowLevel};

/// Appearance applied to every managed child OS window, including replacements.
///
/// The external Root window keeps its application's `eframe::NativeOptions`.
/// `None` uses the backend default when creating a window and leaves an existing
/// property unchanged, following egui's viewport-builder semantics. Platform
/// support for decorations, taskbar presence, icons and stacking may differ.
/// Placement, visibility, activation and pointer passthrough remain owned by
/// Dockspace's native lifecycle.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativeChildWindowOptions {
    /// Whether the platform should draw window decorations.
    pub decorations: Option<bool>,
    /// Whether the child should appear in the Windows taskbar.
    pub taskbar: Option<bool>,
    /// Requested stacking level, such as [`WindowLevel::AlwaysOnTop`].
    pub window_level: Option<WindowLevel>,
    /// Application icon shared by managed child windows.
    pub icon: Option<Arc<IconData>>,
}

impl NativeChildWindowOptions {
    pub(crate) fn viewport_builder(&self) -> ViewportBuilder {
        ViewportBuilder {
            decorations: self.decorations,
            taskbar: self.taskbar,
            window_level: self.window_level,
            icon: self.icon.clone(),
            ..Default::default()
        }
    }
}

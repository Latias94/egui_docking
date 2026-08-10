//! Owned records copied from eframe's borrowed native event callback.

use dockspace::runtime::NativeSurfaceBinding;
use eframe::egui::ViewportId;
use winit::event::WindowEvent;
use winit::window::WindowId;

/// One immutable native event in the order observed by eframe.
///
/// The raw winit event is cloned without converting it into a second input
/// schema. Missing [`winit::event::PointerEventFacts`] remain missing; callers
/// must not reconstruct them from cached cursor or window state.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeWindowEventRecord {
    ordinal: u64,
    window_id: WindowId,
    viewport_id: Option<ViewportId>,
    binding: Option<NativeSurfaceBinding>,
    event: WindowEvent,
}

impl NativeWindowEventRecord {
    pub(crate) fn from_eframe(
        event: eframe::NativeWindowEvent<'_>,
        binding: Option<NativeSurfaceBinding>,
    ) -> Self {
        Self {
            ordinal: event.ordinal().get(),
            window_id: event.window_id(),
            viewport_id: event.viewport_id(),
            binding,
            event: event.event().clone(),
        }
    }

    /// Returns the exact eframe event ordinal.
    #[must_use]
    pub const fn ordinal(&self) -> u64 {
        self.ordinal
    }

    /// Returns the native delivery window.
    #[must_use]
    pub const fn window_id(&self) -> WindowId {
        self.window_id
    }

    /// Returns the mapped eframe viewport, when the window still belongs to eframe.
    #[must_use]
    pub const fn viewport_id(&self) -> Option<ViewportId> {
        self.viewport_id
    }

    /// Returns the exact core binding mapped at callback time, when known.
    ///
    /// `None` is an explicit authority gap. Callers must not resolve the event
    /// against a newer viewport mapping after a native window was recreated.
    #[must_use]
    pub const fn binding(&self) -> Option<NativeSurfaceBinding> {
        self.binding
    }

    /// Returns the exact cloned winit event.
    #[must_use]
    pub const fn event(&self) -> &WindowEvent {
        &self.event
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        ordinal: u64,
        window_id: WindowId,
        viewport_id: Option<ViewportId>,
        binding: Option<NativeSurfaceBinding>,
        event: WindowEvent,
    ) -> Self {
        Self {
            ordinal,
            window_id,
            viewport_id,
            binding,
            event,
        }
    }
}

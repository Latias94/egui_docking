//! Owned records copied from eframe's borrowed native event callback.

use dockspace::runtime::NativeSurfaceBinding;
use eframe::egui::ViewportId;
use winit::event::{PointerWindowRoute, WindowEvent};
use winit::window::WindowId;

use crate::viewport_map::NativeViewportMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativePointerRouteSnapshot {
    Unknown,
    None,
    Dock(NativeSurfaceBinding),
    Foreign,
}

impl NativePointerRouteSnapshot {
    fn from_winit(route: PointerWindowRoute, viewports: &NativeViewportMap) -> Self {
        match route {
            PointerWindowRoute::Unknown => Self::Unknown,
            PointerWindowRoute::None => Self::None,
            PointerWindowRoute::Window(window) => viewports
                .binding_for_window(window)
                .map_or(Self::Foreign, Self::Dock),
            PointerWindowRoute::Foreign => Self::Foreign,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativePointerRoutes {
    delivery: NativePointerRouteSnapshot,
    hover: NativePointerRouteSnapshot,
    capture: NativePointerRouteSnapshot,
}

impl NativePointerRoutes {
    pub(crate) const fn from_binding(binding: Option<NativeSurfaceBinding>) -> Self {
        let delivery = match binding {
            Some(binding) => NativePointerRouteSnapshot::Dock(binding),
            None => NativePointerRouteSnapshot::Unknown,
        };
        Self {
            delivery,
            hover: NativePointerRouteSnapshot::Unknown,
            capture: NativePointerRouteSnapshot::Unknown,
        }
    }

    pub(crate) const fn delivery(self) -> NativePointerRouteSnapshot {
        self.delivery
    }

    pub(crate) const fn hover(self) -> NativePointerRouteSnapshot {
        self.hover
    }

    pub(crate) const fn capture(self) -> NativePointerRouteSnapshot {
        self.capture
    }
}

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
    pointer_routes: Option<NativePointerRoutes>,
    event: WindowEvent,
}

impl NativeWindowEventRecord {
    pub(crate) fn from_eframe(
        event: eframe::NativeWindowEvent<'_>,
        viewports: &NativeViewportMap,
    ) -> Self {
        Self::from_parts(
            event.ordinal().get(),
            event.window_id(),
            event.viewport_id(),
            event.event().clone(),
            viewports,
        )
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

    pub(crate) const fn pointer_routes(&self) -> Option<NativePointerRoutes> {
        self.pointer_routes
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
            pointer_routes: None,
            event,
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test_snapshot(
        ordinal: u64,
        window_id: WindowId,
        viewport_id: Option<ViewportId>,
        event: WindowEvent,
        viewports: &NativeViewportMap,
    ) -> Self {
        Self::from_parts(ordinal, window_id, viewport_id, event, viewports)
    }

    fn from_parts(
        ordinal: u64,
        window_id: WindowId,
        viewport_id: Option<ViewportId>,
        event: WindowEvent,
        viewports: &NativeViewportMap,
    ) -> Self {
        let binding = viewports.binding_for_event(window_id, viewport_id);
        let delivery = match binding {
            Some(binding) => NativePointerRouteSnapshot::Dock(binding),
            None if viewport_id.is_some() => NativePointerRouteSnapshot::Unknown,
            None => NativePointerRouteSnapshot::Foreign,
        };
        let pointer_routes = match &event {
            WindowEvent::CursorMoved { facts, .. }
            | WindowEvent::MouseWheel { facts, .. }
            | WindowEvent::MouseInput { facts, .. } => Some(NativePointerRoutes {
                delivery,
                hover: NativePointerRouteSnapshot::from_winit(facts.hover, viewports),
                capture: NativePointerRouteSnapshot::from_winit(facts.capture, viewports),
            }),
            WindowEvent::PointerCaptureChanged { capture, .. } => Some(NativePointerRoutes {
                delivery,
                hover: NativePointerRouteSnapshot::Unknown,
                capture: NativePointerRouteSnapshot::from_winit(*capture, viewports),
            }),
            WindowEvent::PanGesture { .. } => Some(NativePointerRoutes {
                delivery,
                hover: NativePointerRouteSnapshot::Unknown,
                capture: NativePointerRouteSnapshot::Unknown,
            }),
            _ => None,
        };
        Self {
            ordinal,
            window_id,
            viewport_id,
            binding,
            pointer_routes,
            event,
        }
    }
}

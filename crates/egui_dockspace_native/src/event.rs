//! Owned records copied from eframe's borrowed native event callback.

use dockspace::runtime::NativeSurfaceBinding;
use eframe::egui::ViewportId;
use winit::event::{PointerWindowRoute, WindowEvent};
use winit::window::WindowId;

use crate::viewport_map::NativeViewportMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeWindowEventClass {
    Pointer,
    Lifecycle,
    Other,
}

impl NativeWindowEventClass {
    pub(crate) const fn classify(event: &WindowEvent) -> Self {
        match event {
            WindowEvent::CursorMoved { .. }
            | WindowEvent::MouseInput { .. }
            | WindowEvent::MouseWheel { .. }
            | WindowEvent::PointerCaptureChanged { .. }
            | WindowEvent::PanGesture { .. } => Self::Pointer,
            WindowEvent::CloseRequested | WindowEvent::Destroyed => Self::Lifecycle,
            _ => Self::Other,
        }
    }

    pub(crate) const fn is_dockspace_ingress(self) -> bool {
        !matches!(self, Self::Other)
    }
}

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
                .pointer_binding_for_window(window)
                .map_or(Self::Unknown, Self::Dock),
            PointerWindowRoute::Foreign => Self::Foreign,
        }
    }

    fn references_binding(self, binding: NativeSurfaceBinding) -> bool {
        matches!(self, Self::Dock(current) if current == binding)
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

    pub(crate) fn references_binding(self, binding: NativeSurfaceBinding) -> bool {
        self.delivery.references_binding(binding)
            || self.hover.references_binding(binding)
            || self.capture.references_binding(binding)
    }
}

/// One immutable native event in the order observed by eframe.
///
/// The raw winit event is cloned without converting it into a second input
/// schema. Missing [`winit::event::PointerEventFacts`] remain missing; callers
/// must not reconstruct them from cached cursor or window state.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NativeWindowEventRecord {
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
        viewports: &mut NativeViewportMap,
    ) -> Self {
        Self::from_ingress_parts(
            event.ordinal().get(),
            event.window_id(),
            event.viewport_id(),
            event.event().clone(),
            viewports,
        )
    }

    /// Returns the exact eframe event ordinal.
    #[must_use]
    pub(crate) const fn ordinal(&self) -> u64 {
        self.ordinal
    }

    /// Returns the native delivery window.
    #[must_use]
    pub(crate) const fn window_id(&self) -> WindowId {
        self.window_id
    }

    /// Returns the mapped eframe viewport, when the window still belongs to eframe.
    #[must_use]
    pub(crate) const fn viewport_id(&self) -> Option<ViewportId> {
        self.viewport_id
    }

    /// Returns the exact core binding mapped at callback time, when known.
    ///
    /// `None` is an explicit authority gap. Callers must not resolve the event
    /// against a newer viewport mapping after a native window was recreated.
    #[must_use]
    pub(crate) const fn binding(&self) -> Option<NativeSurfaceBinding> {
        self.binding
    }

    pub(crate) const fn pointer_routes(&self) -> Option<NativePointerRoutes> {
        self.pointer_routes
    }

    pub(crate) fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.binding == Some(binding)
            || self
                .pointer_routes
                .is_some_and(|routes| routes.references_binding(binding))
    }

    pub(crate) fn shares_idle_cursor_lane(&self, newer: &Self) -> bool {
        let (
            WindowEvent::CursorMoved {
                device_id: first_device,
                ..
            },
            WindowEvent::CursorMoved {
                device_id: newer_device,
                ..
            },
        ) = (&self.event, &newer.event)
        else {
            return false;
        };
        first_device == newer_device
            && self.window_id == newer.window_id
            && self.viewport_id == newer.viewport_id
            && self.binding == newer.binding
            && self.pointer_routes == newer.pointer_routes
    }

    /// Returns the exact cloned winit event.
    #[must_use]
    pub(crate) const fn event(&self) -> &WindowEvent {
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

    #[cfg(test)]
    pub(crate) fn for_test_ingress(
        ordinal: u64,
        window_id: WindowId,
        viewport_id: Option<ViewportId>,
        event: WindowEvent,
        viewports: &mut NativeViewportMap,
    ) -> Self {
        Self::from_ingress_parts(ordinal, window_id, viewport_id, event, viewports)
    }

    fn from_ingress_parts(
        ordinal: u64,
        window_id: WindowId,
        viewport_id: Option<ViewportId>,
        event: WindowEvent,
        viewports: &mut NativeViewportMap,
    ) -> Self {
        if matches!(event, WindowEvent::Destroyed)
            && let Some((viewport, binding)) = viewports.route_for_event(window_id, viewport_id)
        {
            let suppressed = viewports.suppress_pointer_route(viewport, binding);
            debug_assert!(suppressed, "destroyed callback retains one exact route");
        }
        Self::from_parts(ordinal, window_id, viewport_id, event, viewports)
    }

    fn from_parts(
        ordinal: u64,
        window_id: WindowId,
        viewport_id: Option<ViewportId>,
        event: WindowEvent,
        viewports: &NativeViewportMap,
    ) -> Self {
        let (mapped_viewport, binding) = viewports.route_for_event(window_id, viewport_id).unzip();
        let viewport_id = mapped_viewport.or(viewport_id);
        // A callback window which is not in our current map is not evidence of
        // a foreign receiver. It may be a newly-created viewport waiting for
        // enrollment, a retired window whose callback arrived late, or a
        // window owned by another host. Keep that distinction authoritative at
        // the platform route layer instead of guessing from `viewport_id`.
        let pointer_binding = viewports.pointer_binding_for_event(window_id, viewport_id);
        let delivery = pointer_binding.map_or(
            NativePointerRouteSnapshot::Unknown,
            NativePointerRouteSnapshot::Dock,
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use winit::dpi::PhysicalPosition;
    use winit::event::{DeviceId, ElementState, MouseButton, PointerEventFacts};

    #[test]
    fn unmapped_callback_window_is_not_promoted_to_foreign_delivery() {
        let window = WindowId::from(41);
        let event = WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: ElementState::Pressed,
            button: MouseButton::Left,
            facts: PointerEventFacts {
                surface_position: Some(PhysicalPosition::new(4.0, 5.0)),
                ..PointerEventFacts::default()
            },
        };
        let viewports = NativeViewportMap::default();

        let record = NativeWindowEventRecord::from_parts(1, window, None, event, &viewports);

        assert_eq!(
            record
                .pointer_routes()
                .expect("pointer event keeps route facts")
                .delivery(),
            NativePointerRouteSnapshot::Unknown
        );
    }

    #[test]
    fn unmapped_winit_route_is_not_promoted_to_foreign_hover_or_capture() {
        let window = WindowId::from(42);
        let event = WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: ElementState::Released,
            button: MouseButton::Left,
            facts: PointerEventFacts {
                hover: PointerWindowRoute::Window(window),
                capture: PointerWindowRoute::Window(window),
                ..PointerEventFacts::default()
            },
        };
        let viewports = NativeViewportMap::default();

        let record = NativeWindowEventRecord::from_parts(2, window, None, event, &viewports);
        let routes = record
            .pointer_routes()
            .expect("pointer event keeps route facts");

        assert_eq!(routes.hover(), NativePointerRouteSnapshot::Unknown);
        assert_eq!(routes.capture(), NativePointerRouteSnapshot::Unknown);
    }

    #[test]
    fn event_classification_keeps_only_ordered_dockspace_ingress() {
        let pointer = WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(1.0, 2.0),
            facts: PointerEventFacts::default(),
        };

        assert_eq!(
            NativeWindowEventClass::classify(&pointer),
            NativeWindowEventClass::Pointer
        );
        assert_eq!(
            NativeWindowEventClass::classify(&WindowEvent::CloseRequested),
            NativeWindowEventClass::Lifecycle
        );
        assert_eq!(
            NativeWindowEventClass::classify(&WindowEvent::Focused(true)),
            NativeWindowEventClass::Other
        );
    }
}

//! Owned records copied from eframe's borrowed native event callback.

use std::sync::Arc;

use dockspace::geometry::{PhysicalPoint, PhysicalSize, ScaleFactor};
use dockspace::runtime::{
    NativeLocalCoordinates, NativeLocalGeometry, NativeLocalPointerFact,
    NativeLocalPointerLocation, NativeLocalPointerSample, NativeSurfaceBinding,
};
use eframe::{NativeDeviceRemoval, egui::ViewportId};
use winit::event::{PointerEventFacts, PointerWindowRoute, WindowEvent};
use winit::window::WindowId;

use crate::viewport_map::NativeViewportMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeDeviceRemovalRecord {
    ordinal: u64,
}

impl NativeDeviceRemovalRecord {
    pub(crate) fn from_eframe(removal: NativeDeviceRemoval) -> Self {
        Self {
            ordinal: removal.ordinal().get(),
        }
    }

    #[cfg(test)]
    pub(crate) const fn for_test(ordinal: u64) -> Self {
        Self { ordinal }
    }
}

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
            | WindowEvent::PanGesture { .. }
            | WindowEvent::Touch(_) => Self::Pointer,
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

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(
    clippy::large_enum_variant,
    reason = "the callback's single Arc allocation owns the complete local sample inline"
)]
pub(crate) enum CapturedNativePointerLocation {
    Desktop,
    NativeLocal(NativeLocalPointerLocation),
    /// Exact local sample paired with the desktop facts in the same raw event.
    LocalToDesktop(NativeLocalPointerSample),
}

#[derive(Debug, PartialEq)]
struct NativePointerCallbackFacts {
    routes: NativePointerRoutes,
    location: CapturedNativePointerLocation,
}

#[derive(Debug, Clone, Copy)]
struct NativePointerLocalCapture {
    coordinates: Option<NativeLocalCoordinates>,
    geometry: Option<NativeLocalGeometry>,
}

impl NativePointerLocalCapture {
    fn capture(
        self,
        binding: Option<NativeSurfaceBinding>,
        facts: &PointerEventFacts,
    ) -> NativeLocalPointerLocation {
        let sample = (|| {
            let geometry = facts.geometry?;
            let position = facts.surface_position?;
            let event = NativeLocalPointerFact::new(
                PhysicalPoint::new(position.x, position.y).ok()?,
                PhysicalSize::new(
                    f64::from(geometry.surface_size.width),
                    f64::from(geometry.surface_size.height),
                )
                .ok()?,
                ScaleFactor::new(geometry.scale_factor).ok()?,
            );
            self.coordinates?
                .capture(binding?, event, self.geometry?)
                .ok()
        })();
        sample.map_or(
            NativeLocalPointerLocation::Unknown,
            NativeLocalPointerLocation::Exact,
        )
    }
}

fn local_geometry(geometry: eframe::NativeWindowLocalGeometry) -> Option<NativeLocalGeometry> {
    let [width, height] = geometry.content_size();
    Some(NativeLocalGeometry::new(
        PhysicalSize::new(f64::from(width), f64::from(height)).ok()?,
        ScaleFactor::new(geometry.native_scale_factor()).ok()?,
        ScaleFactor::new(geometry.presentation_scale_factor()).ok()?,
    ))
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
    pointer: Option<Arc<NativePointerCallbackFacts>>,
    close_request: Option<eframe::NativeViewportCloseRequest>,
    event: WindowEvent,
}

impl NativeWindowEventRecord {
    pub(crate) fn from_eframe(
        event: eframe::NativeWindowEvent<'_>,
        viewports: &mut NativeViewportMap,
    ) -> Self {
        let local_capture = NativePointerLocalCapture {
            coordinates: viewports
                .local_coordinates_for_event(event.window_id(), event.viewport_id()),
            geometry: event.local_geometry().and_then(local_geometry),
        };
        Self {
            close_request: event.close_request(),
            ..Self::from_ingress_parts(
                event.ordinal().get(),
                event.window_id(),
                event.viewport_id(),
                event.event().clone(),
                viewports,
                Some(local_capture),
            )
        }
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

    /// Returns only the close request retained from the original eframe callback.
    #[must_use]
    pub(crate) const fn close_request(&self) -> Option<eframe::NativeViewportCloseRequest> {
        self.close_request
    }

    pub(crate) fn pointer_routes(&self) -> Option<NativePointerRoutes> {
        self.pointer.as_ref().map(|pointer| pointer.routes)
    }

    pub(crate) fn pointer_location(&self) -> Option<CapturedNativePointerLocation> {
        self.pointer.as_ref().map(|pointer| pointer.location)
    }

    /// Counts the pointer payload's data, excluding Arc and allocator metadata.
    /// Retry clones retain the same allocation rather than allocating a new one.
    pub(crate) fn owned_descendant_capacity_bytes(&self) -> usize {
        self.pointer
            .as_ref()
            .map_or(0, |_| Self::pointer_callback_capacity_bytes())
    }

    /// Reserves one immutable pointer payload before accepting the callback.
    pub(crate) const fn pointer_callback_capacity_bytes() -> usize {
        size_of::<NativePointerCallbackFacts>()
    }

    pub(crate) fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.binding == Some(binding)
            || self
                .pointer_routes()
                .is_some_and(|routes| routes.references_binding(binding))
    }

    pub(crate) fn shares_cursor_lane(&self, newer: &Self) -> bool {
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
            && self.pointer_routes() == newer.pointer_routes()
            && match (self.pointer_location(), newer.pointer_location()) {
                (
                    Some(CapturedNativePointerLocation::NativeLocal(
                        NativeLocalPointerLocation::Exact(first),
                    )),
                    Some(CapturedNativePointerLocation::NativeLocal(
                        NativeLocalPointerLocation::Exact(newer),
                    )),
                )
                | (
                    Some(CapturedNativePointerLocation::LocalToDesktop(first)),
                    Some(CapturedNativePointerLocation::LocalToDesktop(newer)),
                ) => first.same_coordinates(newer),
                (
                    Some(CapturedNativePointerLocation::NativeLocal(
                        NativeLocalPointerLocation::Unknown,
                    )),
                    Some(CapturedNativePointerLocation::NativeLocal(
                        NativeLocalPointerLocation::Unknown,
                    )),
                ) => {
                    // Failed capture retains no usable coordinate basis. It
                    // cannot prove that two callbacks share one incarnation.
                    false
                }
                (first, newer) => first == newer,
            }
    }

    /// Returns the exact cloned winit event.
    #[must_use]
    pub(crate) const fn event(&self) -> &WindowEvent {
        &self.event
    }

    /// Freezes the sole attached-window owner claimed by the mailbox ingress.
    pub(crate) fn qualify_destroyed(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) {
        debug_assert!(matches!(self.event, WindowEvent::Destroyed));
        self.viewport_id = Some(viewport);
        self.binding = Some(binding);
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
            pointer: None,
            close_request: None,
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
        Self::from_parts(ordinal, window_id, viewport_id, event, viewports, None)
    }

    #[cfg(test)]
    pub(crate) fn for_test_ingress(
        ordinal: u64,
        window_id: WindowId,
        viewport_id: Option<ViewportId>,
        event: WindowEvent,
        viewports: &mut NativeViewportMap,
    ) -> Self {
        Self::from_ingress_parts(ordinal, window_id, viewport_id, event, viewports, None)
    }

    #[cfg(test)]
    pub(crate) fn for_test_local_capture(
        ordinal: u64,
        window_id: WindowId,
        viewport_id: Option<ViewportId>,
        event: WindowEvent,
        viewports: &NativeViewportMap,
        coordinates: Option<NativeLocalCoordinates>,
        geometry: Option<NativeLocalGeometry>,
    ) -> Self {
        Self::from_parts(
            ordinal,
            window_id,
            viewport_id,
            event,
            viewports,
            Some(NativePointerLocalCapture {
                coordinates,
                geometry,
            }),
        )
    }

    fn from_ingress_parts(
        ordinal: u64,
        window_id: WindowId,
        viewport_id: Option<ViewportId>,
        event: WindowEvent,
        viewports: &mut NativeViewportMap,
        local_capture: Option<NativePointerLocalCapture>,
    ) -> Self {
        if matches!(event, WindowEvent::Destroyed)
            && let Some((viewport, binding)) = viewports.route_for_event(window_id, viewport_id)
        {
            let suppressed = viewports.suppress_pointer_route(viewport, binding);
            debug_assert!(suppressed, "destroyed callback retains one exact route");
        }
        Self::from_parts(
            ordinal,
            window_id,
            viewport_id,
            event,
            viewports,
            local_capture,
        )
    }

    fn from_parts(
        ordinal: u64,
        window_id: WindowId,
        viewport_id: Option<ViewportId>,
        event: WindowEvent,
        viewports: &NativeViewportMap,
        local_capture: Option<NativePointerLocalCapture>,
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
            WindowEvent::PanGesture { .. } | WindowEvent::Touch(_) => Some(NativePointerRoutes {
                delivery,
                hover: NativePointerRouteSnapshot::Unknown,
                capture: NativePointerRouteSnapshot::Unknown,
            }),
            _ => None,
        };
        let pointer = pointer_routes.map(|routes| {
            let location = match (&event, local_capture) {
                (WindowEvent::CursorMoved { facts, .. }, Some(local_capture))
                    if facts.geometry.is_some()
                        && facts
                            .desktop_position
                            .is_some_and(|point| point.x.is_finite() && point.y.is_finite()) =>
                {
                    match local_capture.capture(binding, facts) {
                        NativeLocalPointerLocation::Exact(sample) => {
                            CapturedNativePointerLocation::LocalToDesktop(sample)
                        }
                        NativeLocalPointerLocation::Unknown => {
                            CapturedNativePointerLocation::Desktop
                        }
                    }
                }
                (
                    WindowEvent::CursorMoved { facts, .. }
                    | WindowEvent::MouseInput { facts, .. }
                    | WindowEvent::MouseWheel { facts, .. },
                    Some(local_capture),
                ) if facts.geometry.is_some()
                    && !facts
                        .desktop_position
                        .is_some_and(|point| point.x.is_finite() && point.y.is_finite()) =>
                {
                    CapturedNativePointerLocation::NativeLocal(
                        local_capture.capture(binding, facts),
                    )
                }
                _ => CapturedNativePointerLocation::Desktop,
            };
            Arc::new(NativePointerCallbackFacts { routes, location })
        });
        Self {
            ordinal,
            window_id,
            viewport_id,
            binding,
            pointer,
            close_request: None,
            event,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pointer_event::{NativePointerTranslation, NativePointerTranslator};
    use dockspace::geometry::LogicalPoint;
    use dockspace::model::{
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
        RootId, SurfaceId,
    };
    use dockspace::policy::DockPolicy;
    use dockspace::runtime::{
        DockspaceSession, HostWindowToken, NativeCreateStrategy, NativeDesktopPointerLocation,
        NativeDesktopPosition, NativeLocalToDesktopHandoff, NativePointerButton,
        NativePointerEvent, NativePointerHover, NativePointerId, NativePointerInput,
        NativePointerLocation, NativePointerOwner, NativePointerRoster, NativeReceiverAnswer,
        NativeScrollDelta, NativeScrollDeviceId, NativeScrollEvent, NativeScrollModifiers,
        NativeScrollMomentum, NativeScrollPhase, NativeWindowFacts, NativeWindowInputState,
        NativeWindowPresentationState, NativeWorkAreaRoster, PointerDeviceKind,
        PointerEventMetadata, SurfaceUnavailableReason,
    };
    use winit::dpi::{PhysicalPosition, PhysicalSize as WinitPhysicalSize};
    use winit::event::{
        DeviceId, ElementState, MouseButton, MouseScrollDelta, PointerEventGeometry, Touch,
        TouchPhase,
    };

    const WINDOW: u64 = 70;

    fn geometry(scale: f64) -> NativeLocalGeometry {
        NativeLocalGeometry::new(
            PhysicalSize::new(800.0, 600.0).unwrap(),
            ScaleFactor::new(scale).unwrap(),
            ScaleFactor::new(scale).unwrap(),
        )
    }

    fn commit_geometry(
        session: &mut DockspaceSession,
        binding: NativeSurfaceBinding,
        geometry: NativeLocalGeometry,
    ) {
        session
            .report_managed_native_snapshot(
                [(
                    binding,
                    NativeWindowFacts::live()
                        .with_content_size(geometry.content_size())
                        .with_native_scale_factor(geometry.native_scale_factor())
                        .with_presentation_scale_factor(geometry.presentation_scale_factor())
                        .with_input(NativeWindowInputState::ReceivesInput, None)
                        .with_presentation(NativeWindowPresentationState::Visible, None),
                )],
                NativeWorkAreaRoster::Unknown,
            )
            .unwrap();
        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        frame.commit().unwrap();
    }

    fn local_fixture() -> (DockspaceSession, NativeSurfaceBinding, NativeViewportMap) {
        let surface = SurfaceId::new(1);
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            surface,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1)]),
            ),
        )])
        .unwrap();
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
        session
            .enable_managed_native_host(
                NativeCreateStrategy::DirectPrepared,
                NativePointerRoster::Exact(Vec::new()),
            )
            .unwrap();
        session
            .configure_managed_native_capabilities(
                dockspace::runtime::NativeHostCapabilities::none_supported()
                    .with(dockspace::runtime::NativeHostCapability::AuthoritativeInventory),
            )
            .unwrap();
        session
            .register_native_root(surface, HostWindowToken::new(WINDOW))
            .unwrap();
        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let binding = frame.commit().unwrap().native_bindings()[0];
        commit_geometry(&mut session, binding, geometry(1.0));
        let mut measure = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        measure
            .measure_surface(
                surface,
                dockspace::runtime::UniformSurfaceMetrics::new(
                    dockspace::geometry::LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
                    dockspace::geometry::LogicalSize::new(32.0, 24.0).unwrap(),
                    80.0,
                )
                .unwrap(),
            )
            .unwrap();
        measure
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        measure.commit().unwrap();
        let mut paint = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        let receipt = paint
            .paint_plan(surface)
            .unwrap()
            .unwrap()
            .execute_surface_program(|_| Ok::<(), ()>(()))
            .unwrap();
        paint.confirm_surface_painted(receipt).unwrap();
        let mut report = paint.commit().unwrap();
        let [output] = report.take_painted_outputs().try_into().unwrap();
        session
            .report_surface_presentation(
                output,
                dockspace::runtime::SurfacePresentationResult::Presented,
            )
            .unwrap();
        let mut presented = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        presented
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        presented.commit().unwrap();
        let mut viewports = NativeViewportMap::default();
        viewports.reserve(ViewportId::ROOT, binding).unwrap();
        viewports
            .attach(ViewportId::ROOT, binding, WindowId::from(WINDOW))
            .unwrap();
        viewports.refresh_local_coordinates(&session);
        assert!(
            viewports
                .local_coordinates_for_event(WindowId::from(WINDOW), Some(ViewportId::ROOT))
                .is_some()
        );
        (session, binding, viewports)
    }

    fn pointer_facts(x: f64, y: f64, scale: f64) -> PointerEventFacts {
        PointerEventFacts {
            surface_position: Some(PhysicalPosition::new(x, y)),
            geometry: Some(PointerEventGeometry {
                surface_size: WinitPhysicalSize::new(800, 600),
                scale_factor: scale,
            }),
            hover: PointerWindowRoute::Window(WindowId::from(WINDOW)),
            capture: PointerWindowRoute::Window(WindowId::from(WINDOW)),
            ..PointerEventFacts::default()
        }
    }

    fn cursor(facts: PointerEventFacts) -> WindowEvent {
        WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: facts.surface_position.unwrap(),
            facts,
        }
    }

    fn captured(
        ordinal: u64,
        event: WindowEvent,
        viewports: &NativeViewportMap,
        geometry: NativeLocalGeometry,
    ) -> NativeWindowEventRecord {
        NativeWindowEventRecord::for_test_local_capture(
            ordinal,
            WindowId::from(WINDOW),
            Some(ViewportId::ROOT),
            event,
            viewports,
            viewports.local_coordinates_for_event(WindowId::from(WINDOW), Some(ViewportId::ROOT)),
            Some(geometry),
        )
    }

    #[test]
    fn synthetic_close_requests_do_not_carry_close_authority() {
        let window = WindowId::from(40);
        let viewport = Some(ViewportId::ROOT);
        let mut viewports = NativeViewportMap::default();
        let records = [
            NativeWindowEventRecord::for_test(
                1,
                window,
                viewport,
                None,
                WindowEvent::CloseRequested,
            ),
            NativeWindowEventRecord::for_test_snapshot(
                1,
                window,
                viewport,
                WindowEvent::CloseRequested,
                &viewports,
            ),
            NativeWindowEventRecord::for_test_ingress(
                1,
                window,
                viewport,
                WindowEvent::CloseRequested,
                &mut viewports,
            ),
        ];

        for record in records {
            assert!(record.close_request().is_none());
            assert_eq!(record.ordinal(), 1);
            assert_eq!(record.window_id(), window);
            assert_eq!(record.viewport_id(), viewport);
            assert_eq!(record.binding(), None);
            assert_eq!(record.event(), &WindowEvent::CloseRequested);
        }
    }

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

        let record = NativeWindowEventRecord::from_parts(1, window, None, event, &viewports, None);

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

        let record = NativeWindowEventRecord::from_parts(2, window, None, event, &viewports, None);
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

    #[test]
    fn queued_old_scale_mouse_facts_cannot_borrow_new_callback_coordinates() {
        let (mut session, binding, mut viewports) = local_fixture();
        let facts = pointer_facts(20.0, 40.0, 1.0);
        let queued = [
            (cursor(facts), NativePointerEvent::Moved),
            (
                WindowEvent::MouseInput {
                    device_id: DeviceId::dummy(),
                    state: ElementState::Released,
                    button: MouseButton::Left,
                    facts,
                },
                NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
            ),
            (
                WindowEvent::MouseWheel {
                    device_id: DeviceId::dummy(),
                    delta: MouseScrollDelta::LineDelta(0.0, 2.0),
                    phase: TouchPhase::Moved,
                    facts,
                },
                NativePointerEvent::Scrolled(NativeScrollEvent::new(
                    NativeScrollDeviceId::new(1),
                    NativeScrollPhase::Discrete {
                        delta: NativeScrollDelta::Lines { x: 0.0, y: 2.0 },
                    },
                    NativeScrollMomentum::Unknown,
                    NativeScrollModifiers::Unknown,
                )),
            ),
        ];
        commit_geometry(&mut session, binding, geometry(2.0));
        viewports.refresh_local_coordinates(&session);

        for (event, expected_event) in queued {
            let record = captured(1, event.clone(), &viewports, geometry(2.0));
            assert_eq!(record.event(), &event);
            assert_eq!(
                record.pointer_location(),
                Some(CapturedNativePointerLocation::NativeLocal(
                    NativeLocalPointerLocation::Unknown
                ))
            );
            let mut translator = NativePointerTranslator::default();
            assert_eq!(
                translator.translate(&record, |_| panic!("local input has no desktop work area")),
                NativePointerTranslation::Input(NativePointerInput::new_local(
                    NativePointerId::new(1),
                    expected_event,
                    NativeLocalPointerLocation::Unknown,
                    NativePointerOwner::Native(binding),
                    NativePointerOwner::Native(binding),
                ))
            );
        }
    }

    #[test]
    fn x11_focus_cursor_without_event_geometry_preserves_the_desktop_unknown_lane() {
        let (_session, binding, viewports) = local_fixture();
        let facts = PointerEventFacts {
            surface_position: Some(PhysicalPosition::new(20.0, 40.0)),
            geometry: None,
            desktop_position: None,
            hover: PointerWindowRoute::Unknown,
            capture: PointerWindowRoute::Unknown,
            ..PointerEventFacts::default()
        };
        let record = captured(1, cursor(facts), &viewports, geometry(1.0));
        assert_eq!(
            record.pointer_location(),
            Some(CapturedNativePointerLocation::Desktop),
            "missing desktop facts do not prove that an X11 callback changed coordinate lane",
        );
        assert_eq!(
            NativePointerTranslator::default().translate(&record, |_| {
                panic!("an unknown desktop position cannot select a work area")
            }),
            NativePointerTranslation::Input(NativePointerInput::new(
                NativePointerId::new(1),
                NativePointerEvent::Moved,
                NativeDesktopPointerLocation::new(
                    NativeDesktopPosition::Unknown,
                    NativePointerHover::Unknown,
                    None,
                ),
                NativePointerOwner::Native(binding),
                NativePointerOwner::Unknown,
            )),
        );
    }

    #[test]
    fn delayed_translation_retains_the_sample_frozen_before_route_refresh() {
        let (mut session, binding, mut viewports) = local_fixture();
        let record = captured(
            1,
            cursor(pointer_facts(20.0, 40.0, 1.0)),
            &viewports,
            geometry(1.0),
        );
        let Some(CapturedNativePointerLocation::NativeLocal(NativeLocalPointerLocation::Exact(
            original,
        ))) = record.pointer_location()
        else {
            panic!("the original callback has exact local coordinates");
        };
        assert_eq!(
            original.surface_position(),
            LogicalPoint::new(20.0, 40.0).unwrap()
        );

        commit_geometry(&mut session, binding, geometry(2.0));
        viewports.refresh_local_coordinates(&session);
        let newer = captured(
            2,
            cursor(pointer_facts(20.0, 40.0, 2.0)),
            &viewports,
            geometry(2.0),
        );
        let Some(CapturedNativePointerLocation::NativeLocal(NativeLocalPointerLocation::Exact(
            current,
        ))) = newer.pointer_location()
        else {
            panic!("the refreshed callback has exact local coordinates");
        };
        assert!(!original.same_coordinates(current));
        assert_eq!(
            current.surface_position(),
            LogicalPoint::new(10.0, 20.0).unwrap()
        );

        let captured_at = std::time::Duration::from_millis(37);
        let mut translator = NativePointerTranslator::default();
        assert_eq!(
            translator.translate_captured(&record, captured_at, |_| {
                panic!("delayed local translation must not query a desktop work area")
            }),
            NativePointerTranslation::Input(
                NativePointerInput::new_local(
                    NativePointerId::new(1),
                    NativePointerEvent::Moved,
                    NativeLocalPointerLocation::Exact(original),
                    NativePointerOwner::Native(binding),
                    NativePointerOwner::Native(binding),
                )
                .with_metadata(dockspace::runtime::PointerEventMetadata::new(
                    dockspace::runtime::PointerDeviceKind::Mouse,
                    captured_at,
                ))
            )
        );
    }

    #[test]
    fn dual_motion_translation_retains_same_callback_sample_routes_and_metadata() {
        let (mut session, binding, mut viewports) = local_fixture();
        let facts = PointerEventFacts {
            desktop_position: Some(PhysicalPosition::new(420.0, -260.0)),
            hover: PointerWindowRoute::Foreign,
            ..pointer_facts(20.0, 40.0, 1.0)
        };
        let event = cursor(facts);
        let record = captured(1, event.clone(), &viewports, geometry(1.0));
        assert_eq!(record.event(), &event);
        let Some(CapturedNativePointerLocation::LocalToDesktop(original)) =
            record.pointer_location()
        else {
            panic!("one exact cursor callback must retain both coordinate facts");
        };
        assert_eq!(
            original.client_position(),
            PhysicalPoint::new(20.0, 40.0).unwrap()
        );
        assert_eq!(
            original.surface_position(),
            LogicalPoint::new(20.0, 40.0).unwrap()
        );

        commit_geometry(&mut session, binding, geometry(2.0));
        viewports.refresh_local_coordinates(&session);
        let newer = captured(
            2,
            cursor(pointer_facts(20.0, 40.0, 2.0)),
            &viewports,
            geometry(2.0),
        );
        let Some(CapturedNativePointerLocation::NativeLocal(NativeLocalPointerLocation::Exact(
            current,
        ))) = newer.pointer_location()
        else {
            panic!("the next callback uses the refreshed coordinate authority");
        };
        assert!(!original.same_coordinates(current));
        assert_eq!(
            current.surface_position(),
            LogicalPoint::new(10.0, 20.0).unwrap()
        );
        assert!(viewports.suppress_pointer_route(ViewportId::ROOT, binding));

        let captured_at = std::time::Duration::from_millis(37);
        let expected = NativePointerTranslation::Input(
            NativePointerInput::new_handoff(
                NativePointerId::new(1),
                NativePointerEvent::Moved,
                NativeLocalToDesktopHandoff::new(
                    original,
                    NativeDesktopPointerLocation::new(
                        NativeDesktopPosition::Exact(PhysicalPoint::new(420.0, -260.0).unwrap()),
                        NativePointerHover::Foreign,
                        None,
                    ),
                ),
                NativePointerOwner::Native(binding),
                NativePointerOwner::Native(binding),
            )
            .with_metadata(PointerEventMetadata::new(
                PointerDeviceKind::Mouse,
                captured_at,
            )),
        );
        let retry = record.clone();
        let mut translator = NativePointerTranslator::default();
        for record in [&record, &retry] {
            assert_eq!(
                translator.translate_captured(record, captured_at, |_| {
                    panic!("foreign hover cannot select a work area")
                }),
                expected,
            );
        }
    }

    #[test]
    fn finite_desktop_cursor_requires_exact_event_and_callback_local_facts() {
        let (_session, binding, viewports) = local_fixture();
        let coordinates = viewports.local_coordinates_for_event(WindowId::from(WINDOW), None);
        let callback = Some(geometry(1.0));
        let facts = PointerEventFacts {
            desktop_position: Some(PhysicalPosition::new(420.0, -260.0)),
            ..pointer_facts(20.0, 40.0, 1.0)
        };
        for (facts, coordinates, callback) in [
            (facts, None, callback),
            (facts, coordinates, None),
            (facts, coordinates, Some(geometry(2.0))),
            (
                PointerEventFacts {
                    surface_position: None,
                    ..facts
                },
                coordinates,
                callback,
            ),
            (
                PointerEventFacts {
                    geometry: None,
                    ..facts
                },
                coordinates,
                callback,
            ),
            (
                PointerEventFacts {
                    geometry: Some(PointerEventGeometry {
                        surface_size: WinitPhysicalSize::new(800, 600),
                        scale_factor: 2.0,
                    }),
                    ..facts
                },
                coordinates,
                callback,
            ),
        ] {
            let record = NativeWindowEventRecord::for_test_local_capture(
                1,
                WindowId::from(WINDOW),
                Some(ViewportId::ROOT),
                WindowEvent::CursorMoved {
                    device_id: DeviceId::dummy(),
                    position: PhysicalPosition::new(20.0, 40.0),
                    facts,
                },
                &viewports,
                coordinates,
                callback,
            );
            assert_eq!(
                record.pointer_location(),
                Some(CapturedNativePointerLocation::Desktop)
            );
            assert_eq!(
                NativePointerTranslator::default().translate(&record, |_| {
                    panic!("dock hover cannot select a work area")
                }),
                NativePointerTranslation::Input(NativePointerInput::new(
                    NativePointerId::new(1),
                    NativePointerEvent::Moved,
                    NativeDesktopPointerLocation::new(
                        NativeDesktopPosition::Exact(PhysicalPoint::new(420.0, -260.0).unwrap()),
                        NativePointerHover::Dock(binding),
                        None,
                    ),
                    NativePointerOwner::Native(binding),
                    NativePointerOwner::Native(binding),
                )),
            );
        }
    }

    #[test]
    fn missing_or_non_finite_desktop_cursor_retains_the_exact_local_sample() {
        let (_session, _binding, viewports) = local_fixture();
        for desktop_position in [
            None,
            Some(PhysicalPosition::new(f64::NAN, 0.0)),
            Some(PhysicalPosition::new(0.0, f64::INFINITY)),
        ] {
            let record = captured(
                1,
                cursor(PointerEventFacts {
                    desktop_position,
                    ..pointer_facts(20.0, 40.0, 1.0)
                }),
                &viewports,
                geometry(1.0),
            );
            let Some(CapturedNativePointerLocation::NativeLocal(
                NativeLocalPointerLocation::Exact(sample),
            )) = record.pointer_location()
            else {
                panic!("unavailable desktop facts must retain the original local sample");
            };
            assert_eq!(
                sample.surface_position(),
                LogicalPoint::new(20.0, 40.0).unwrap()
            );
            let NativePointerTranslation::Input(input) = NativePointerTranslator::default()
                .translate(&record, |_| panic!("local input cannot select a work area"))
            else {
                panic!("the local motion must still translate");
            };
            assert_eq!(
                input.location(),
                NativePointerLocation::NativeLocal(NativeLocalPointerLocation::Exact(sample),)
            );
        }
    }

    #[test]
    fn finite_desktop_buttons_and_wheel_keep_their_desktop_lane() {
        let (_session, _binding, viewports) = local_fixture();
        let facts = PointerEventFacts {
            desktop_position: Some(PhysicalPosition::new(420.0, -260.0)),
            ..pointer_facts(20.0, 40.0, 1.0)
        };
        for event in [
            WindowEvent::MouseInput {
                device_id: DeviceId::dummy(),
                state: ElementState::Pressed,
                button: MouseButton::Left,
                facts,
            },
            WindowEvent::MouseInput {
                device_id: DeviceId::dummy(),
                state: ElementState::Released,
                button: MouseButton::Left,
                facts,
            },
            WindowEvent::MouseWheel {
                device_id: DeviceId::dummy(),
                delta: MouseScrollDelta::LineDelta(0.0, 2.0),
                phase: TouchPhase::Moved,
                facts,
            },
        ] {
            let record = captured(1, event.clone(), &viewports, geometry(1.0));
            assert_eq!(record.event(), &event);
            assert_eq!(
                record.pointer_location(),
                Some(CapturedNativePointerLocation::Desktop)
            );
            let NativePointerTranslation::Input(input) = NativePointerTranslator::default()
                .translate(&record, |_| panic!("dock hover cannot select a work area"))
            else {
                panic!("button and wheel callbacks must retain their existing translation");
            };
            assert!(matches!(
                input.location(),
                NativePointerLocation::Desktop(_)
            ));
        }
    }

    #[test]
    fn cursor_coalescing_compares_coordinate_authority_instead_of_points_or_arc_addresses() {
        let (mut session, binding, mut viewports) = local_fixture();
        let first = captured(
            1,
            cursor(pointer_facts(20.0, 40.0, 1.0)),
            &viewports,
            geometry(1.0),
        );
        let moved = captured(
            2,
            cursor(pointer_facts(30.0, 50.0, 1.0)),
            &viewports,
            geometry(1.0),
        );
        assert!(!Arc::ptr_eq(
            first.pointer.as_ref().unwrap(),
            moved.pointer.as_ref().unwrap()
        ));
        assert!(first.shares_cursor_lane(&moved));

        commit_geometry(&mut session, binding, geometry(2.0));
        commit_geometry(&mut session, binding, geometry(1.0));
        viewports.refresh_local_coordinates(&session);
        let renewed = captured(
            3,
            cursor(pointer_facts(30.0, 50.0, 1.0)),
            &viewports,
            geometry(1.0),
        );
        assert!(!first.shares_cursor_lane(&renewed));
        assert_eq!(first.pointer_routes(), renewed.pointer_routes());
        assert_eq!(
            viewports
                .local_coordinates_for_event(WindowId::from(WINDOW), Some(ViewportId::ROOT))
                .unwrap()
                .geometry(),
            geometry(1.0)
        );
    }

    #[test]
    fn dual_cursor_compaction_requires_same_authority_and_location_variant() {
        let (mut session, binding, mut viewports) = local_fixture();
        let facts = PointerEventFacts {
            desktop_position: Some(PhysicalPosition::new(420.0, -260.0)),
            ..pointer_facts(20.0, 40.0, 1.0)
        };
        let first = captured(1, cursor(facts), &viewports, geometry(1.0));
        let moved = captured(
            2,
            cursor(PointerEventFacts {
                desktop_position: Some(PhysicalPosition::new(430.0, -250.0)),
                ..pointer_facts(30.0, 50.0, 1.0)
            }),
            &viewports,
            geometry(1.0),
        );
        assert!(matches!(
            first.pointer_location(),
            Some(CapturedNativePointerLocation::LocalToDesktop(_))
        ));
        assert!(!Arc::ptr_eq(
            first.pointer.as_ref().unwrap(),
            moved.pointer.as_ref().unwrap()
        ));
        assert!(first.shares_cursor_lane(&moved));
        assert!(moved.shares_cursor_lane(&first));

        for boundary in [
            captured(
                3,
                cursor(pointer_facts(20.0, 40.0, 1.0)),
                &viewports,
                geometry(1.0),
            ),
            captured(
                3,
                cursor(pointer_facts(20.0, 40.0, 2.0)),
                &viewports,
                geometry(1.0),
            ),
            captured(
                3,
                cursor(PointerEventFacts {
                    geometry: None,
                    ..facts
                }),
                &viewports,
                geometry(1.0),
            ),
            captured(
                3,
                cursor(PointerEventFacts {
                    hover: PointerWindowRoute::Foreign,
                    ..facts
                }),
                &viewports,
                geometry(1.0),
            ),
        ] {
            assert!(!first.shares_cursor_lane(&boundary));
            assert!(!boundary.shares_cursor_lane(&first));
        }

        commit_geometry(&mut session, binding, geometry(2.0));
        commit_geometry(&mut session, binding, geometry(1.0));
        viewports.refresh_local_coordinates(&session);
        let renewed = captured(4, cursor(facts), &viewports, geometry(1.0));
        assert!(matches!(
            renewed.pointer_location(),
            Some(CapturedNativePointerLocation::LocalToDesktop(_))
        ));
        assert_eq!(first.pointer_routes(), renewed.pointer_routes());
        assert!(!first.shares_cursor_lane(&renewed));
        assert!(!renewed.shares_cursor_lane(&first));
    }

    #[test]
    fn unknown_local_coordinates_do_not_authorize_idle_cursor_compaction() {
        let (_session, _binding, viewports) = local_fixture();
        let first = captured(
            1,
            cursor(pointer_facts(20.0, 40.0, 2.0)),
            &viewports,
            geometry(1.0),
        );
        let moved = captured(
            2,
            cursor(pointer_facts(30.0, 50.0, 2.0)),
            &viewports,
            geometry(1.0),
        );
        let other_scale = captured(
            3,
            cursor(pointer_facts(30.0, 50.0, 3.0)),
            &viewports,
            geometry(1.0),
        );
        let mut resized = pointer_facts(30.0, 50.0, 2.0);
        resized.geometry.as_mut().unwrap().surface_size.width += 1;
        let other_size = captured(4, cursor(resized), &viewports, geometry(1.0));
        assert_eq!(
            first.pointer_location(),
            Some(CapturedNativePointerLocation::NativeLocal(
                NativeLocalPointerLocation::Unknown
            ))
        );
        assert!(!first.shares_cursor_lane(&moved));
        assert!(!first.shares_cursor_lane(&other_scale));
        assert!(!first.shares_cursor_lane(&other_size));

        let mut desktop = pointer_facts(30.0, 50.0, 2.0);
        desktop.desktop_position = Some(PhysicalPosition::new(10.0, 20.0));
        let desktop = captured(5, cursor(desktop), &viewports, geometry(1.0));
        assert_eq!(
            desktop.pointer_location(),
            Some(CapturedNativePointerLocation::Desktop)
        );
        assert!(!first.shares_cursor_lane(&desktop));
    }

    #[test]
    fn missing_event_time_facts_remain_unknown_even_when_cursor_and_callback_are_known() {
        let (_session, _binding, viewports) = local_fixture();
        for facts in [
            PointerEventFacts {
                surface_position: None,
                ..pointer_facts(20.0, 40.0, 1.0)
            },
            PointerEventFacts {
                geometry: None,
                ..pointer_facts(20.0, 40.0, 1.0)
            },
            PointerEventFacts {
                desktop_position: Some(PhysicalPosition::new(f64::NAN, 0.0)),
                geometry: None,
                ..pointer_facts(20.0, 40.0, 1.0)
            },
        ] {
            let has_local_geometry = facts.geometry.is_some();
            let record = captured(
                1,
                WindowEvent::CursorMoved {
                    device_id: DeviceId::dummy(),
                    position: PhysicalPosition::new(20.0, 40.0),
                    facts,
                },
                &viewports,
                geometry(1.0),
            );
            assert_eq!(
                record.pointer_location(),
                Some(if has_local_geometry {
                    CapturedNativePointerLocation::NativeLocal(NativeLocalPointerLocation::Unknown)
                } else {
                    CapturedNativePointerLocation::Desktop
                })
            );
        }
    }

    #[test]
    fn touch_and_pan_keep_desktop_lanes_and_existing_touch_tail_suppression() {
        let (_session, binding, viewports) = local_fixture();
        let mut translator = NativePointerTranslator::default();
        let touch = |phase| {
            captured(
                1,
                WindowEvent::Touch(Touch {
                    device_id: DeviceId::dummy(),
                    phase,
                    location: PhysicalPosition::new(20.0, 40.0),
                    force: None,
                    id: 7,
                }),
                &viewports,
                geometry(1.0),
            )
        };
        let start = touch(TouchPhase::Started);
        assert_eq!(
            start.pointer_location(),
            Some(CapturedNativePointerLocation::Desktop)
        );
        assert_eq!(
            translator.translate(&start, |_| panic!("touch has no desktop point")),
            NativePointerTranslation::Input(NativePointerInput::new(
                NativePointerId::new(1),
                NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
                NativeDesktopPointerLocation::new(
                    NativeDesktopPosition::Unknown,
                    NativePointerHover::Unknown,
                    None,
                ),
                NativePointerOwner::Native(binding),
                NativePointerOwner::Unknown,
            ))
        );
        assert_eq!(
            translator.translate(&start, |_| None),
            NativePointerTranslation::Ignored
        );
        translator.retire_binding(binding);
        for phase in [TouchPhase::Moved, TouchPhase::Ended, TouchPhase::Cancelled] {
            assert_eq!(
                translator.translate(&touch(phase), |_| None),
                NativePointerTranslation::Ignored
            );
        }

        let pan = captured(
            2,
            WindowEvent::PanGesture {
                device_id: DeviceId::dummy(),
                delta: PhysicalPosition::new(1.0, 2.0),
                phase: TouchPhase::Moved,
            },
            &viewports,
            geometry(1.0),
        );
        assert_eq!(
            pan.pointer_location(),
            Some(CapturedNativePointerLocation::Desktop)
        );
        let NativePointerTranslation::Input(input) = translator.translate(&pan, |_| None) else {
            panic!("pan retains the existing scroll translator");
        };
        assert!(matches!(
            input.location(),
            NativePointerLocation::Desktop(_)
        ));
    }

    #[test]
    fn pointer_payload_capacity_is_complete_and_retry_clones_share_one_allocation() {
        let (_session, _binding, viewports) = local_fixture();
        for desktop_position in [None, Some(PhysicalPosition::new(420.0, -260.0))] {
            let pointer = captured(
                1,
                cursor(PointerEventFacts {
                    desktop_position,
                    ..pointer_facts(20.0, 40.0, 1.0)
                }),
                &viewports,
                geometry(1.0),
            );
            assert_eq!(
                pointer.owned_descendant_capacity_bytes(),
                size_of::<NativePointerCallbackFacts>()
            );
            assert_eq!(
                pointer.owned_descendant_capacity_bytes(),
                NativeWindowEventRecord::pointer_callback_capacity_bytes()
            );
            let retry = pointer.clone();
            assert!(Arc::ptr_eq(
                pointer.pointer.as_ref().unwrap(),
                retry.pointer.as_ref().unwrap()
            ));
            assert_eq!(Arc::strong_count(pointer.pointer.as_ref().unwrap()), 2);
            drop(retry);
            assert_eq!(Arc::strong_count(pointer.pointer.as_ref().unwrap()), 1);
        }

        for event in [WindowEvent::CloseRequested, WindowEvent::Destroyed] {
            let record = captured(2, event, &viewports, geometry(1.0));
            assert!(record.pointer.is_none());
            assert_eq!(record.owned_descendant_capacity_bytes(), 0);
        }
    }
}

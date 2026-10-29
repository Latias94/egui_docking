//! Translation from callback-time winit pointer facts into core native input.

use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};

use dockspace::geometry::PhysicalPoint;
use dockspace::runtime::{
    NativeDesktopPointerLocation, NativeDesktopPosition, NativePointerButton,
    NativePointerCancelReason, NativePointerEvent, NativePointerHover, NativePointerId,
    NativePointerInput, NativePointerOwner, NativeScrollCancelReason, NativeScrollDelta,
    NativeScrollDeviceId, NativeScrollEvent, NativeScrollModifiers, NativeScrollMomentum,
    NativeScrollPhase, NativeScrollSequenceId, NativeSurfaceBinding, NativeWorkAreaBinding,
};
use winit::event::{
    DeviceId, ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent,
};

use crate::event::{
    NativePointerRouteSnapshot, NativePointerRoutes, NativeWindowEventClass,
    NativeWindowEventRecord,
};

#[derive(Debug, Clone, Default)]
pub(crate) struct NativePointerTranslator {
    devices: BTreeMap<NativeInputDeviceKey, NativePointerDeviceState>,
    removed_inputs: BTreeSet<NativeInputDeviceKey>,
    overload_scroll_tails: BTreeSet<NativeInputDeviceKey>,
    next_device_identity: u64,
    next_scroll_sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum NativeInputDeviceKey {
    Mouse(NativePhysicalDeviceKey),
    Scroll(NativePhysicalDeviceKey),
    Touch {
        device: NativePhysicalDeviceKey,
        contact: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum NativePhysicalDeviceKey {
    Winit(DeviceId),
    #[cfg(test)]
    Test(u64),
}

#[derive(Debug, Clone)]
struct NativePointerDeviceState {
    pointer: NativePointerId,
    scroll_device: NativeScrollDeviceId,
    mouse: NativeMouseState,
    scroll: NativeScrollState,
}

#[derive(Debug, Clone, Default)]
struct NativeMouseState {
    active_buttons: BTreeSet<NativePointerButton>,
    button_origins: BTreeMap<NativePointerButton, NativeSurfaceBinding>,
    cancelled_buttons: BTreeSet<NativePointerButton>,
    capture: Option<NativeSurfaceBinding>,
}

#[derive(Debug, Clone, Copy, Default)]
enum NativeScrollState {
    #[default]
    Idle,
    /// One winit phaseful sequence still maps to a live core sequence.
    Live {
        sequence: NativeScrollSequenceId,
        delivery: Option<NativeSurfaceBinding>,
    },
    /// Core closed the provider tail; ignore late phases until an explicit start.
    Retired,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum NativePointerTranslation {
    NotPointer,
    Ignored,
    Input(NativePointerInput),
}

impl NativePointerTranslator {
    /// Counts inline storage, device and mouse entry data, and both tombstone sets.
    /// B-tree node and allocator overhead are excluded.
    pub(crate) fn owned_data_capacity_bytes(&self) -> usize {
        let bytes = size_of::<Self>()
            .saturating_add(self.devices.len().saturating_mul(
                size_of::<NativeInputDeviceKey>() + size_of::<NativePointerDeviceState>(),
            ))
            .saturating_add(
                self.removed_inputs
                    .len()
                    .saturating_mul(size_of::<NativeInputDeviceKey>()),
            )
            .saturating_add(
                self.overload_scroll_tails
                    .len()
                    .saturating_mul(size_of::<NativeInputDeviceKey>()),
            );
        self.devices.values().fold(bytes, |bytes, device| {
            bytes
                .saturating_add(
                    device
                        .mouse
                        .active_buttons
                        .len()
                        .saturating_mul(size_of::<NativePointerButton>()),
                )
                .saturating_add(device.mouse.button_origins.len().saturating_mul(
                    size_of::<NativePointerButton>() + size_of::<NativeSurfaceBinding>(),
                ))
                .saturating_add(
                    device
                        .mouse
                        .cancelled_buttons
                        .len()
                        .saturating_mul(size_of::<NativePointerButton>()),
                )
        })
    }

    pub(crate) fn can_coalesce_idle_cursor_moves(&self) -> bool {
        self.devices.values().all(NativePointerDeviceState::is_idle)
    }

    /// Forgets adapter state after core overload cancellation commits.
    ///
    /// Core already terminated the semantic streams. Discard their identities
    /// without emitting terminals or claiming an exact empty pointer roster.
    /// Only phaseful scroll lanes need a tombstone until the next explicit
    /// start; idle wheel lanes must still accept Moved-only discrete samples.
    pub(crate) fn finish_overload_cancellation(&mut self) {
        self.overload_scroll_tails
            .extend(self.devices.iter().filter_map(|(key, device)| {
                (!matches!(device.scroll, NativeScrollState::Idle)).then_some(*key)
            }));
        self.devices.clear();
    }

    /// Installs local tail suppression after core records binding retirement.
    ///
    /// Mouse and scroll lanes keep their identities. Touch contacts move into
    /// the existing tombstone set so contact ID reuse allocates a fresh identity.
    pub(crate) fn retire_binding(&mut self, binding: NativeSurfaceBinding) {
        let Self {
            devices,
            removed_inputs,
            ..
        } = self;
        devices.retain(|key, device| {
            if device.retire_binding(binding) && matches!(key, NativeInputDeviceKey::Touch { .. }) {
                let _ = removed_inputs.insert(*key);
                return false;
            }
            true
        });
    }

    /// Forgets adapter identities after the provider-wide removal fact records.
    ///
    /// Winit raw-device identifiers are not correlated with the virtual
    /// identifiers carried by window input, so one raw removal cannot safely
    /// target a subset of these states. Tombstones reject their late tails
    /// until an explicit start allocates a fresh identity.
    pub(crate) fn forget_removed_devices(&mut self) {
        self.removed_inputs.extend(self.devices.keys().copied());
        self.devices.clear();
    }

    pub(crate) fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.devices
            .values()
            .any(|device| device.references_binding(binding))
    }

    pub(crate) fn translate(
        &mut self,
        record: &NativeWindowEventRecord,
        resolve_work_area: impl FnMut(PhysicalPoint) -> Option<NativeWorkAreaBinding>,
    ) -> NativePointerTranslation {
        if NativeWindowEventClass::classify(record.event()) != NativeWindowEventClass::Pointer {
            return NativePointerTranslation::NotPointer;
        }
        let device = pointer_device(record.event())
            .expect("pointer event classification and device extraction must stay aligned");
        self.translate_for_device(record, device, resolve_work_area)
    }

    #[cfg(test)]
    pub(crate) fn translate_for_test_device(
        &mut self,
        record: &NativeWindowEventRecord,
        device: u64,
        resolve_work_area: impl FnMut(PhysicalPoint) -> Option<NativeWorkAreaBinding>,
    ) -> NativePointerTranslation {
        if NativeWindowEventClass::classify(record.event()) != NativeWindowEventClass::Pointer {
            return NativePointerTranslation::NotPointer;
        }
        let device =
            pointer_device_for_physical(record.event(), NativePhysicalDeviceKey::Test(device))
                .expect(
                    "pointer event classification and test-device extraction must stay aligned",
                );
        self.translate_for_device(record, device, resolve_work_area)
    }

    fn translate_for_device(
        &mut self,
        record: &NativeWindowEventRecord,
        device: NativeInputDeviceKey,
        mut resolve_work_area: impl FnMut(PhysicalPoint) -> Option<NativeWorkAreaBinding>,
    ) -> NativePointerTranslation {
        let routes = record
            .pointer_routes()
            .unwrap_or_else(|| NativePointerRoutes::from_binding(record.binding()));
        if self.removed_inputs.contains(&device) {
            if starts_reused_device_stream(record.event()) {
                let _ = self.removed_inputs.remove(&device);
            } else {
                return NativePointerTranslation::Ignored;
            }
        }

        match record.event() {
            WindowEvent::CursorMoved { facts, .. } => self.input(
                device,
                NativePointerEvent::Moved,
                facts.desktop_position,
                routes,
                &mut resolve_work_area,
            ),
            WindowEvent::MouseInput {
                state,
                button,
                facts,
                ..
            } => {
                let button = native_button(*button);
                let event = match state {
                    ElementState::Pressed => NativePointerEvent::ButtonPressed(button),
                    ElementState::Released => NativePointerEvent::ButtonReleased(button),
                };
                self.input(
                    device,
                    event,
                    facts.desktop_position,
                    routes,
                    &mut resolve_work_area,
                )
            }
            WindowEvent::PointerCaptureChanged { .. } => self.input(
                device,
                NativePointerEvent::CaptureChanged,
                None,
                routes,
                &mut resolve_work_area,
            ),
            WindowEvent::MouseWheel {
                delta,
                phase,
                facts,
                ..
            } => {
                let Some(delta) = delta_value(delta) else {
                    return NativePointerTranslation::Ignored;
                };
                self.scroll(device, delta, *phase, facts, routes, &mut resolve_work_area)
            }
            WindowEvent::PanGesture { delta, phase, .. } => {
                let delta = NativeScrollDelta::PhysicalPixels {
                    x: f64::from(delta.x),
                    y: f64::from(delta.y),
                };
                if !scroll_delta_is_finite(delta) {
                    return NativePointerTranslation::Ignored;
                }
                let facts = winit::event::PointerEventFacts::default();
                self.scroll(
                    device,
                    delta,
                    *phase,
                    &facts,
                    routes,
                    &mut resolve_work_area,
                )
            }
            WindowEvent::Touch(touch) => self.touch(device, *touch, routes, &mut resolve_work_area),
            _ => unreachable!("pointer event classification and translation must stay aligned"),
        }
    }

    fn touch(
        &mut self,
        device: NativeInputDeviceKey,
        touch: winit::event::Touch,
        routes: NativePointerRoutes,
        resolve_work_area: &mut impl FnMut(PhysicalPoint) -> Option<NativeWorkAreaBinding>,
    ) -> NativePointerTranslation {
        let enrolled = self.devices.contains_key(&device);
        if matches!(touch.phase, TouchPhase::Started) == enrolled {
            return NativePointerTranslation::Ignored;
        }
        if matches!(touch.phase, TouchPhase::Moved)
            && self
                .devices
                .get(&device)
                .is_some_and(|state| state.mouse.active_origin_mismatch(routes))
        {
            return NativePointerTranslation::Ignored;
        }
        let routes = if matches!(touch.phase, TouchPhase::Cancelled)
            || matches!(touch.phase, TouchPhase::Ended)
                && self
                    .devices
                    .get(&device)
                    .is_some_and(|state| state.mouse.active_origin_mismatch(routes))
        {
            NativePointerRoutes::from_binding(None)
        } else {
            routes
        };
        let event = match touch.phase {
            TouchPhase::Started => NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            TouchPhase::Moved => NativePointerEvent::Moved,
            TouchPhase::Ended => NativePointerEvent::ContactEnded(NativePointerButton::Primary),
            TouchPhase::Cancelled => NativePointerEvent::StreamCancelled(
                NativePointerCancelReason::ExplicitPlatformCancellation,
            ),
        };
        let translation = self.input(device, event, None, routes, resolve_work_area);
        if matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled)
            && matches!(translation, NativePointerTranslation::Input(_))
        {
            let _ = self.devices.remove(&device);
        }
        translation
    }

    fn input(
        &mut self,
        device: NativeInputDeviceKey,
        event: NativePointerEvent,
        desktop_position: Option<winit::dpi::PhysicalPosition<f64>>,
        routes: NativePointerRoutes,
        resolve_work_area: &mut impl FnMut(PhysicalPoint) -> Option<NativeWorkAreaBinding>,
    ) -> NativePointerTranslation {
        let Some(device) = self.device_mut(device) else {
            return NativePointerTranslation::Ignored;
        };
        if !device.mouse.observe(event, routes) {
            return NativePointerTranslation::Ignored;
        }
        let position = native_position(desktop_position);
        let hover = native_hover(routes.hover());
        NativePointerTranslation::Input(NativePointerInput::new(
            device.pointer,
            event,
            NativeDesktopPointerLocation::new(
                position,
                hover,
                native_work_area(position, hover, resolve_work_area),
            ),
            native_owner(routes.delivery()),
            native_owner(routes.capture()),
        ))
    }

    fn scroll(
        &mut self,
        device: NativeInputDeviceKey,
        delta: NativeScrollDelta,
        phase: TouchPhase,
        facts: &winit::event::PointerEventFacts,
        routes: NativePointerRoutes,
        resolve_work_area: &mut impl FnMut(PhysicalPoint) -> Option<NativeWorkAreaBinding>,
    ) -> NativePointerTranslation {
        if self.overload_scroll_tails.contains(&device) {
            if !matches!(phase, TouchPhase::Started) {
                return NativePointerTranslation::Ignored;
            }
            let _ = self.overload_scroll_tails.remove(&device);
        }
        if self.device_mut(device).is_none() {
            return NativePointerTranslation::Ignored;
        }
        let Self {
            devices,
            next_scroll_sequence,
            ..
        } = self;
        let device = devices
            .get_mut(&device)
            .expect("a successfully enrolled device remains in the adapter map");
        let delivery = exact_delivery_binding(routes.delivery());
        if let NativeScrollState::Live {
            delivery: Some(active),
            ..
        } = device.scroll
            && delivery != Some(active)
        {
            // Winit does not carry a scroll sequence token. A callback routed
            // through another exact binding may be a delayed predecessor tail,
            // so it cannot mutate or terminate the current sequence.
            return NativePointerTranslation::Ignored;
        }
        let Some((phase, next_state)) =
            translate_scroll_phase(device.scroll, phase, delta, delivery, next_scroll_sequence)
        else {
            return NativePointerTranslation::Ignored;
        };
        device.scroll = next_state;

        let event = NativeScrollEvent::new(
            device.scroll_device,
            phase,
            NativeScrollMomentum::Unknown,
            native_modifiers(facts),
        );
        let position = native_position(facts.desktop_position);
        let hover = native_hover(routes.hover());
        NativePointerTranslation::Input(NativePointerInput::new(
            device.pointer,
            NativePointerEvent::Scrolled(event),
            NativeDesktopPointerLocation::new(
                position,
                hover,
                native_work_area(position, hover, resolve_work_area),
            ),
            native_owner(routes.delivery()),
            native_owner(routes.capture()),
        ))
    }

    fn device_mut(&mut self, key: NativeInputDeviceKey) -> Option<&mut NativePointerDeviceState> {
        let Self {
            devices,
            next_device_identity,
            ..
        } = self;
        match devices.entry(key) {
            Entry::Occupied(entry) => Some(entry.into_mut()),
            Entry::Vacant(entry) => {
                let next = allocate_device_identity(next_device_identity)?;
                Some(entry.insert(NativePointerDeviceState::new(next)))
            }
        }
    }
}

fn translate_scroll_phase(
    state: NativeScrollState,
    phase: TouchPhase,
    delta: NativeScrollDelta,
    event_delivery: Option<NativeSurfaceBinding>,
    next_scroll_sequence: &mut u64,
) -> Option<(NativeScrollPhase, NativeScrollState)> {
    match (state, phase) {
        (NativeScrollState::Idle | NativeScrollState::Retired, TouchPhase::Started) => {
            let sequence = allocate_scroll_sequence(next_scroll_sequence)?;
            Some((
                NativeScrollPhase::Begin {
                    sequence,
                    delta: Some(delta),
                },
                NativeScrollState::Live {
                    sequence,
                    delivery: event_delivery,
                },
            ))
        }
        (
            NativeScrollState::Live { sequence, delivery },
            TouchPhase::Started | TouchPhase::Moved,
        ) => Some((
            NativeScrollPhase::Update { sequence, delta },
            NativeScrollState::Live { sequence, delivery },
        )),
        (NativeScrollState::Live { sequence, .. }, TouchPhase::Ended) => Some((
            NativeScrollPhase::End {
                sequence,
                delta: Some(delta),
            },
            NativeScrollState::Idle,
        )),
        (NativeScrollState::Live { sequence, .. }, TouchPhase::Cancelled) => Some((
            NativeScrollPhase::Cancel {
                sequence,
                reason: NativeScrollCancelReason::PlatformCancelled,
            },
            NativeScrollState::Idle,
        )),
        (NativeScrollState::Retired, _) => None,
        (NativeScrollState::Idle, TouchPhase::Moved) => Some((
            NativeScrollPhase::Discrete { delta },
            NativeScrollState::Idle,
        )),
        (NativeScrollState::Idle, TouchPhase::Ended) => (!is_zero_delta(delta)).then_some((
            NativeScrollPhase::Discrete { delta },
            NativeScrollState::Idle,
        )),
        (NativeScrollState::Idle, TouchPhase::Cancelled) => None,
    }
}

fn allocate_device_identity(next_device_identity: &mut u64) -> Option<u64> {
    let next = next_device_identity.checked_add(1)?;
    *next_device_identity = next;
    Some(next)
}

impl NativePointerDeviceState {
    fn new(identity: u64) -> Self {
        Self {
            pointer: NativePointerId::new(identity),
            scroll_device: NativeScrollDeviceId::new(identity),
            mouse: NativeMouseState::default(),
            scroll: NativeScrollState::Idle,
        }
    }

    fn is_idle(&self) -> bool {
        self.mouse.is_idle()
            && matches!(
                self.scroll,
                NativeScrollState::Idle | NativeScrollState::Retired
            )
    }

    fn retire_binding(&mut self, binding: NativeSurfaceBinding) -> bool {
        let mouse_retired = self.mouse.retire_binding(binding);
        let scroll_retired = matches!(
            self.scroll,
            NativeScrollState::Live {
                delivery: Some(delivery),
                ..
            } if delivery == binding
        );
        if scroll_retired {
            self.scroll = NativeScrollState::Retired;
        }
        mouse_retired || scroll_retired
    }

    fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.mouse.references_binding(binding)
            || matches!(
                self.scroll,
                NativeScrollState::Live {
                    delivery: Some(delivery),
                    ..
                } if delivery == binding
            )
    }
}

fn allocate_scroll_sequence(next_scroll_sequence: &mut u64) -> Option<NativeScrollSequenceId> {
    let next = next_scroll_sequence.checked_add(1)?;
    *next_scroll_sequence = next;
    Some(NativeScrollSequenceId::new(next))
}

impl NativeMouseState {
    fn is_idle(&self) -> bool {
        self.active_buttons.is_empty()
            && self.button_origins.is_empty()
            && self.cancelled_buttons.is_empty()
            && self.capture.is_none()
    }

    fn observe(&mut self, event: NativePointerEvent, routes: NativePointerRoutes) -> bool {
        if self.should_ignore_captured_move(event, routes) {
            return false;
        }
        self.observe_routes(routes);
        if matches!(
            event,
            NativePointerEvent::ButtonReleased(button)
                | NativePointerEvent::ContactEnded(button)
                if self.cancelled_buttons.remove(&button)
        ) {
            return false;
        }
        match event {
            NativePointerEvent::ButtonPressed(button) => {
                let _ = self.cancelled_buttons.remove(&button);
                let _ = self.active_buttons.insert(button);
                if let Some(origin) = exact_delivery_binding(routes.delivery()) {
                    let _ = self.button_origins.entry(button).or_insert(origin);
                }
            }
            NativePointerEvent::ButtonReleased(button)
            | NativePointerEvent::ContactEnded(button) => {
                let _ = self.active_buttons.remove(&button);
                let _ = self.button_origins.remove(&button);
            }
            NativePointerEvent::StreamEnded | NativePointerEvent::StreamCancelled(_) => {
                self.active_buttons.clear();
                self.button_origins.clear();
                self.cancelled_buttons.clear();
                self.capture = None;
            }
            NativePointerEvent::Moved
            | NativePointerEvent::Continuation(_)
            | NativePointerEvent::CaptureChanged
            | NativePointerEvent::Scrolled(_) => {}
        }
        true
    }

    fn should_ignore_captured_move(
        &self,
        event: NativePointerEvent,
        routes: NativePointerRoutes,
    ) -> bool {
        // A backend may notify the hovered window before delivering the same
        // motion through the capture owner. Preserve both callback facts by
        // waiting for the exact capture-owner delivery instead of rewriting it.
        matches!(event, NativePointerEvent::Moved)
            && !self.active_buttons.is_empty()
            && exact_delivery_binding(routes.capture())
                .is_some_and(|capture| exact_delivery_binding(routes.delivery()) != Some(capture))
    }

    fn active_origin_mismatch(&self, routes: NativePointerRoutes) -> bool {
        self.button_origins
            .get(&NativePointerButton::Primary)
            .is_some_and(|origin| exact_delivery_binding(routes.delivery()) != Some(*origin))
    }

    fn observe_routes(&mut self, routes: NativePointerRoutes) {
        self.capture = updated_binding(self.capture, routes.capture());
    }

    fn retire_binding(&mut self, binding: NativeSurfaceBinding) -> bool {
        if !self.references_binding(binding) {
            return false;
        }
        self.cancelled_buttons
            .extend(self.active_buttons.iter().copied());
        self.active_buttons.clear();
        self.button_origins.clear();
        self.capture = None;
        true
    }

    fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.button_origins
            .values()
            .any(|origin| *origin == binding)
            || self.capture == Some(binding)
    }
}

fn updated_binding(
    current: Option<NativeSurfaceBinding>,
    route: NativePointerRouteSnapshot,
) -> Option<NativeSurfaceBinding> {
    match route {
        NativePointerRouteSnapshot::Dock(binding) => Some(binding),
        NativePointerRouteSnapshot::None | NativePointerRouteSnapshot::Foreign => None,
        NativePointerRouteSnapshot::Unknown => current,
    }
}

fn pointer_device(event: &WindowEvent) -> Option<NativeInputDeviceKey> {
    let device = match event {
        WindowEvent::CursorMoved { device_id, .. }
        | WindowEvent::MouseInput { device_id, .. }
        | WindowEvent::MouseWheel { device_id, .. }
        | WindowEvent::PointerCaptureChanged { device_id, .. }
        | WindowEvent::PanGesture { device_id, .. } => *device_id,
        WindowEvent::Touch(touch) => touch.device_id,
        _ => return None,
    };
    pointer_device_for_physical(event, NativePhysicalDeviceKey::Winit(device))
}

fn pointer_device_for_physical(
    event: &WindowEvent,
    device: NativePhysicalDeviceKey,
) -> Option<NativeInputDeviceKey> {
    match event {
        WindowEvent::CursorMoved { .. }
        | WindowEvent::MouseInput { .. }
        | WindowEvent::PointerCaptureChanged { .. } => Some(NativeInputDeviceKey::Mouse(device)),
        WindowEvent::MouseWheel { .. } | WindowEvent::PanGesture { .. } => {
            Some(NativeInputDeviceKey::Scroll(device))
        }
        WindowEvent::Touch(touch) => Some(NativeInputDeviceKey::Touch {
            device,
            contact: touch.id,
        }),
        _ => None,
    }
}

fn starts_reused_device_stream(event: &WindowEvent) -> bool {
    matches!(
        event,
        WindowEvent::MouseInput {
            state: ElementState::Pressed,
            ..
        } | WindowEvent::Touch(winit::event::Touch {
            phase: TouchPhase::Started,
            ..
        }) | WindowEvent::MouseWheel {
            phase: TouchPhase::Started,
            ..
        } | WindowEvent::PanGesture {
            phase: TouchPhase::Started,
            ..
        }
    )
}

fn exact_delivery_binding(route: NativePointerRouteSnapshot) -> Option<NativeSurfaceBinding> {
    match route {
        NativePointerRouteSnapshot::Dock(binding) => Some(binding),
        NativePointerRouteSnapshot::Unknown
        | NativePointerRouteSnapshot::None
        | NativePointerRouteSnapshot::Foreign => None,
    }
}

fn native_work_area(
    position: NativeDesktopPosition,
    hover: NativePointerHover,
    resolve: &mut impl FnMut(PhysicalPoint) -> Option<NativeWorkAreaBinding>,
) -> Option<NativeWorkAreaBinding> {
    match (position, hover) {
        (NativeDesktopPosition::Exact(point), NativePointerHover::OutsideAll) => resolve(point),
        _ => None,
    }
}

fn native_position(position: Option<winit::dpi::PhysicalPosition<f64>>) -> NativeDesktopPosition {
    position
        .and_then(|position| PhysicalPoint::new(position.x, position.y).ok())
        .map_or(NativeDesktopPosition::Unknown, NativeDesktopPosition::Exact)
}

fn native_owner(route: NativePointerRouteSnapshot) -> NativePointerOwner {
    match route {
        NativePointerRouteSnapshot::Unknown => NativePointerOwner::Unknown,
        NativePointerRouteSnapshot::None => NativePointerOwner::None,
        NativePointerRouteSnapshot::Dock(binding) => NativePointerOwner::Native(binding),
        NativePointerRouteSnapshot::Foreign => NativePointerOwner::Foreign,
    }
}

fn native_hover(route: NativePointerRouteSnapshot) -> NativePointerHover {
    match route {
        NativePointerRouteSnapshot::Unknown => NativePointerHover::Unknown,
        NativePointerRouteSnapshot::None => NativePointerHover::OutsideAll,
        NativePointerRouteSnapshot::Dock(binding) => NativePointerHover::Dock(binding),
        NativePointerRouteSnapshot::Foreign => NativePointerHover::Foreign,
    }
}

fn native_button(button: MouseButton) -> NativePointerButton {
    match button {
        MouseButton::Left => NativePointerButton::Primary,
        MouseButton::Right => NativePointerButton::Secondary,
        MouseButton::Middle => NativePointerButton::Middle,
        MouseButton::Back => NativePointerButton::Other(u16::MAX - 1),
        MouseButton::Forward => NativePointerButton::Other(u16::MAX),
        MouseButton::Other(button) => NativePointerButton::Other(button),
    }
}

fn delta_value(delta: &MouseScrollDelta) -> Option<NativeScrollDelta> {
    let delta = match delta {
        MouseScrollDelta::LineDelta(x, y) => NativeScrollDelta::Lines {
            x: f64::from(*x),
            y: f64::from(*y),
        },
        MouseScrollDelta::PixelDelta(position) => NativeScrollDelta::PhysicalPixels {
            x: position.x,
            y: position.y,
        },
    };
    scroll_delta_is_finite(delta).then_some(delta)
}

fn scroll_delta_is_finite(delta: NativeScrollDelta) -> bool {
    match delta {
        NativeScrollDelta::PhysicalPixels { x, y }
        | NativeScrollDelta::LogicalPoints { x, y }
        | NativeScrollDelta::Lines { x, y }
        | NativeScrollDelta::Pages { x, y } => x.is_finite() && y.is_finite(),
    }
}

fn is_zero_delta(delta: NativeScrollDelta) -> bool {
    match delta {
        NativeScrollDelta::PhysicalPixels { x, y }
        | NativeScrollDelta::LogicalPoints { x, y }
        | NativeScrollDelta::Lines { x, y }
        | NativeScrollDelta::Pages { x, y } => x == 0.0 && y == 0.0,
    }
}

fn native_modifiers(facts: &winit::event::PointerEventFacts) -> NativeScrollModifiers {
    facts
        .modifiers
        .map_or(NativeScrollModifiers::Unknown, |modifiers| {
            NativeScrollModifiers::Exact {
                shift: modifiers.shift_key(),
                control: modifiers.control_key(),
                alt: modifiers.alt_key(),
                command: modifiers.super_key(),
            }
        })
}

#[cfg(test)]
mod overload;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_capacity_counts_mouse_entries_and_retained_tombstones() {
        let mut translator = NativePointerTranslator::default();
        let empty = translator.owned_data_capacity_bytes();
        let mouse = NativeInputDeviceKey::Mouse(NativePhysicalDeviceKey::Test(1));
        translator.device_mut(mouse).expect("mouse enrolls");
        let idle = translator.owned_data_capacity_bytes();
        let routes = NativePointerRoutes::from_binding(None);
        assert!(
            translator
                .device_mut(mouse)
                .expect("mouse remains enrolled")
                .mouse
                .observe(
                    NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
                    routes,
                )
        );
        assert!(translator.owned_data_capacity_bytes() > idle);
        assert!(
            translator
                .device_mut(mouse)
                .expect("mouse remains enrolled")
                .mouse
                .observe(
                    NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
                    routes,
                )
        );
        assert_eq!(translator.owned_data_capacity_bytes(), idle);

        translator.forget_removed_devices();
        assert!(translator.devices.is_empty());
        assert!(translator.removed_inputs.contains(&mouse));
        let removed = translator.owned_data_capacity_bytes();
        assert!(removed > empty);
        assert!(removed < idle);

        let scroll = NativeInputDeviceKey::Scroll(NativePhysicalDeviceKey::Test(2));
        translator
            .device_mut(scroll)
            .expect("scroll device enrolls")
            .scroll = NativeScrollState::Live {
            sequence: NativeScrollSequenceId::new(1),
            delivery: None,
        };
        translator.finish_overload_cancellation();
        assert!(translator.devices.is_empty());
        assert!(translator.removed_inputs.contains(&mouse));
        assert!(translator.overload_scroll_tails.contains(&scroll));
        let both_tombstones = translator.owned_data_capacity_bytes();
        assert!(both_tombstones > removed);
        assert_eq!(both_tombstones - removed, removed - empty);

        translator.forget_removed_devices();
        translator.finish_overload_cancellation();
        assert_eq!(translator.owned_data_capacity_bytes(), both_tombstones);
    }

    #[test]
    fn idle_cursor_coalescing_stops_for_buttons_and_scroll() {
        let routes = NativePointerRoutes::from_binding(None);
        let mut translator = NativePointerTranslator::default();
        assert!(translator.can_coalesce_idle_cursor_moves());

        let device = NativeInputDeviceKey::Mouse(NativePhysicalDeviceKey::Test(1));
        assert!(
            translator
                .device_mut(device)
                .expect("test device enrolls")
                .mouse
                .observe(
                    NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
                    routes,
                )
        );
        assert!(!translator.can_coalesce_idle_cursor_moves());

        assert!(
            translator
                .device_mut(device)
                .expect("test device remains enrolled")
                .mouse
                .observe(
                    NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
                    routes,
                )
        );
        assert!(translator.can_coalesce_idle_cursor_moves());

        translator
            .device_mut(device)
            .expect("test device remains enrolled")
            .scroll = NativeScrollState::Live {
            sequence: NativeScrollSequenceId::new(1),
            delivery: None,
        };
        assert!(!translator.can_coalesce_idle_cursor_moves());
    }
}

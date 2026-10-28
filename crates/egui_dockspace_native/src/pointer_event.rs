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
    stream_live: bool,
    mouse: NativeMouseState,
    scroll: NativeScrollState,
}

#[derive(Debug, Clone, Default)]
struct NativeMouseState {
    active_buttons: BTreeSet<NativePointerButton>,
    button_origins: BTreeMap<NativePointerButton, NativeSurfaceBinding>,
    cancelled_buttons: BTreeSet<NativePointerButton>,
    capture: Option<NativeSurfaceBinding>,
    retired_tail: Option<NativeSurfaceBinding>,
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
    /// The semantic binding retired while provider tail events may still arrive.
    RetiredTail {
        sequence: NativeScrollSequenceId,
        binding: NativeSurfaceBinding,
        semantic_cancelled_by_pointer: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum NativePointerTranslation {
    NotPointer,
    Ignored,
    Input(NativePointerInput),
}

impl NativePointerTranslator {
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

    /// Terminates the semantic owner while preserving provider-tail correlation.
    ///
    /// Mouse and scroll identities retain their explicit tail state. Touch
    /// contacts instead retire into the input tombstone set because a contact
    /// has no independent reset callback after its delivery binding disappears.
    pub(crate) fn cancel_destroyed_binding(
        &mut self,
        binding: NativeSurfaceBinding,
    ) -> Vec<NativePointerInput> {
        let mut terminals = Vec::new();
        let mut retired_touches = Vec::new();
        for (key, device) in &mut self.devices {
            let Some(input) = device.cancel_destroyed_binding(binding) else {
                continue;
            };
            terminals.push((device.pointer, input));
            if matches!(key, NativeInputDeviceKey::Touch { .. }) {
                retired_touches.push(*key);
            }
        }
        for key in retired_touches {
            let removed = self.devices.remove(&key);
            debug_assert!(removed.is_some());
            let _ = self.removed_inputs.insert(key);
        }
        terminals.sort_by_key(|(pointer, _)| *pointer);
        terminals.into_iter().map(|(_, input)| input).collect()
    }

    /// Conservatively terminates every adapter-owned input identity.
    ///
    /// Winit raw-device identifiers are not correlated with the virtual
    /// identifiers carried by window input, so one raw removal cannot safely
    /// target a subset of these states.
    pub(crate) fn remove_all_devices(&mut self) -> Vec<NativePointerInput> {
        self.removed_inputs.extend(self.devices.keys().copied());
        let mut devices = std::mem::take(&mut self.devices)
            .into_values()
            .collect::<Vec<_>>();
        devices.sort_by_key(|device| device.pointer);
        devices
            .into_iter()
            .flat_map(NativePointerDeviceState::device_removed_terminals)
            .collect()
    }

    pub(crate) fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.devices
            .values()
            .any(|device| device.references_binding(binding))
    }

    pub(crate) fn has_pending_provider_tail(&self) -> bool {
        self.devices
            .values()
            .any(NativePointerDeviceState::has_pending_provider_tail)
    }

    /// Closes retired provider tails at the next causal boundary.
    pub(crate) fn reset_destroyed_binding(
        &mut self,
        binding: NativeSurfaceBinding,
    ) -> Vec<NativePointerInput> {
        let mut terminals = self
            .devices
            .values_mut()
            .filter_map(|device| {
                device
                    .reset_destroyed_binding(binding)
                    .map(|input| (device.pointer, input))
            })
            .collect::<Vec<_>>();
        terminals.sort_by_key(|(pointer, _)| *pointer);
        terminals.into_iter().map(|(_, input)| input).collect()
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
        device.stream_live = !matches!(
            event,
            NativePointerEvent::ContactEnded(_)
                | NativePointerEvent::StreamEnded
                | NativePointerEvent::StreamCancelled(_)
        );
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
        device.stream_live = true;
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
        (NativeScrollState::Idle, TouchPhase::Started) => {
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
        (
            NativeScrollState::Live { sequence, .. }
            | NativeScrollState::RetiredTail { sequence, .. },
            TouchPhase::Ended,
        ) => Some((
            NativeScrollPhase::End {
                sequence,
                delta: Some(delta),
            },
            NativeScrollState::Idle,
        )),
        (
            NativeScrollState::Live { sequence, .. }
            | NativeScrollState::RetiredTail { sequence, .. },
            TouchPhase::Cancelled,
        ) => Some((
            NativeScrollPhase::Cancel {
                sequence,
                reason: NativeScrollCancelReason::PlatformCancelled,
            },
            NativeScrollState::Idle,
        )),
        (
            NativeScrollState::RetiredTail { sequence, .. },
            TouchPhase::Started | TouchPhase::Moved,
        ) => Some((
            NativeScrollPhase::Cancel {
                sequence,
                reason: NativeScrollCancelReason::ProviderReset,
            },
            NativeScrollState::Idle,
        )),
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
            stream_live: false,
            mouse: NativeMouseState::default(),
            scroll: NativeScrollState::Idle,
        }
    }

    fn is_idle(&self) -> bool {
        self.mouse.is_idle() && matches!(self.scroll, NativeScrollState::Idle)
    }

    fn cancel_destroyed_binding(
        &mut self,
        binding: NativeSurfaceBinding,
    ) -> Option<NativePointerInput> {
        if self.mouse.cancel(binding) {
            if let NativeScrollState::Live { sequence, .. } = self.scroll {
                self.scroll = NativeScrollState::RetiredTail {
                    sequence,
                    binding,
                    semantic_cancelled_by_pointer: true,
                };
            }
            if !self.stream_live {
                return None;
            }
            self.stream_live = false;
            return Some(pointer_cancel_input(
                self.pointer,
                NativePointerCancelReason::BindingRetired,
            ));
        }
        let NativeScrollState::Live {
            sequence,
            delivery: Some(delivery),
        } = self.scroll
        else {
            return None;
        };
        if delivery != binding {
            return None;
        }
        self.scroll = NativeScrollState::RetiredTail {
            sequence,
            binding,
            semantic_cancelled_by_pointer: false,
        };
        Some(scroll_cancel_input(
            self.pointer,
            self.scroll_device,
            sequence,
            NativeScrollCancelReason::BindingRetired,
        ))
    }

    fn reset_destroyed_binding(
        &mut self,
        binding: NativeSurfaceBinding,
    ) -> Option<NativePointerInput> {
        self.mouse.reset_destroyed_binding(binding);
        let NativeScrollState::RetiredTail {
            sequence,
            binding: retired,
            semantic_cancelled_by_pointer,
        } = self.scroll
        else {
            return None;
        };
        if retired != binding {
            return None;
        }
        self.scroll = NativeScrollState::Idle;
        if semantic_cancelled_by_pointer {
            return None;
        }
        Some(scroll_cancel_input(
            self.pointer,
            self.scroll_device,
            sequence,
            NativeScrollCancelReason::ProviderReset,
        ))
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
            || matches!(
                self.scroll,
                NativeScrollState::RetiredTail {
                    binding: retired,
                    ..
                } if retired == binding
            )
    }

    const fn has_pending_provider_tail(&self) -> bool {
        self.mouse.has_pending_provider_tail()
            || matches!(self.scroll, NativeScrollState::RetiredTail { .. })
    }

    fn device_removed_terminals(self) -> Vec<NativePointerInput> {
        let mut terminals = Vec::with_capacity(2);
        if let NativeScrollState::Live { sequence, .. } = self.scroll {
            terminals.push(scroll_cancel_input(
                self.pointer,
                self.scroll_device,
                sequence,
                NativeScrollCancelReason::DeviceRemoved,
            ));
        }
        if self.stream_live {
            terminals.push(pointer_cancel_input(
                self.pointer,
                NativePointerCancelReason::DeviceRemoved,
            ));
        }
        terminals
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
            && self.retired_tail.is_none()
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
        if self.retired_tail.is_some() {
            return true;
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
        if self.retired_tail.is_none() {
            self.capture = updated_binding(self.capture, routes.capture());
        }
    }

    fn cancel(&mut self, binding: NativeSurfaceBinding) -> bool {
        if self.retired_tail.is_some() || !self.references_binding(binding) {
            return false;
        }
        self.cancelled_buttons
            .extend(self.active_buttons.iter().copied());
        self.active_buttons.clear();
        self.button_origins.clear();
        self.capture = None;
        self.retired_tail = Some(binding);
        true
    }

    fn reset_destroyed_binding(&mut self, binding: NativeSurfaceBinding) {
        if self.retired_tail == Some(binding) {
            self.retired_tail = None;
        }
    }

    fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.button_origins
            .values()
            .any(|origin| *origin == binding)
            || self.capture == Some(binding)
            || self.retired_tail == Some(binding)
    }

    const fn has_pending_provider_tail(&self) -> bool {
        self.retired_tail.is_some()
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

fn pointer_cancel_input(
    pointer: NativePointerId,
    reason: NativePointerCancelReason,
) -> NativePointerInput {
    NativePointerInput::new(
        pointer,
        NativePointerEvent::StreamCancelled(reason),
        NativeDesktopPointerLocation::new(
            NativeDesktopPosition::Unknown,
            NativePointerHover::Unknown,
            None,
        ),
        NativePointerOwner::Unknown,
        NativePointerOwner::Unknown,
    )
}

fn scroll_cancel_input(
    pointer: NativePointerId,
    device: NativeScrollDeviceId,
    sequence: NativeScrollSequenceId,
    reason: NativeScrollCancelReason,
) -> NativePointerInput {
    NativePointerInput::new(
        pointer,
        NativePointerEvent::Scrolled(NativeScrollEvent::new(
            device,
            NativeScrollPhase::Cancel { sequence, reason },
            NativeScrollMomentum::Unknown,
            NativeScrollModifiers::Unknown,
        )),
        NativeDesktopPointerLocation::new(
            NativeDesktopPosition::Unknown,
            NativePointerHover::Unknown,
            None,
        ),
        NativePointerOwner::Unknown,
        NativePointerOwner::Unknown,
    )
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

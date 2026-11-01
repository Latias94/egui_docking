//! Translation from callback-time winit pointer facts into core native input.

use std::collections::{BTreeMap, btree_map::Entry};
use std::time::Duration;

use dockspace::geometry::PhysicalPoint;
use dockspace::runtime::{
    NativeDesktopPointerLocation, NativeDesktopPosition, NativeLocalToDesktopHandoff,
    NativePointerButton, NativePointerCancelReason, NativePointerEvent, NativePointerHover,
    NativePointerId, NativePointerInput, NativePointerOwner, NativeScrollCancelReason,
    NativeScrollDelta, NativeScrollDeviceId, NativeScrollEvent, NativeScrollModifiers,
    NativeScrollMomentum, NativeScrollPhase, NativeScrollSequenceId, NativeSurfaceBinding,
    NativeWorkAreaBinding, PointerDeviceKind, PointerEventMetadata,
};
use winit::event::{
    DeviceId, ElementState, MouseButton, MouseScrollDelta, PointerPressToken, TouchPhase,
    WindowEvent,
};

use crate::event::{
    CapturedNativePointerLocation, NativePointerRouteSnapshot, NativePointerRoutes,
    NativeWindowEventClass, NativeWindowEventRecord,
};

#[derive(Debug, Clone, Default)]
pub(crate) struct NativePointerTranslator {
    devices: BTreeMap<NativeInputDeviceKey, NativePointerDeviceEntry>,
    next_device_identity: u64,
    next_scroll_sequence: u64,
    #[cfg(test)]
    translation_clock_cost: Option<(std::sync::Arc<std::sync::atomic::AtomicU64>, u64)>,
}

#[derive(Debug, Clone)]
enum NativePointerDeviceEntry {
    Live(NativePointerDeviceState),
    /// Late callbacks cannot reuse this identity without an explicit start.
    Removed,
    /// A cancelled phaseful scroll ignores tails until an explicit start.
    ScrollTail,
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
    buttons: BTreeMap<NativePointerButton, NativeMouseButtonState>,
    capture: Option<NativeSurfaceBinding>,
}

#[derive(Debug, Clone)]
enum NativeMouseButtonState {
    Active {
        origin: Option<NativeSurfaceBinding>,
        primary_press: Option<PointerPressToken>,
    },
    Cancelled,
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
    /// Charges completed callback work to the fixture's existing nanosecond clock.
    /// Clock reads remain pure, and cloned retry candidates share the same source.
    #[cfg(test)]
    pub(crate) fn set_translation_clock_cost_for_test(
        &mut self,
        ticks: std::sync::Arc<std::sync::atomic::AtomicU64>,
        cost_nanos: u64,
    ) {
        self.translation_clock_cost = Some((ticks, cost_nanos));
    }

    /// Counts inline storage, device/tombstone entries and button entries.
    /// B-tree node and allocator overhead are excluded.
    pub(crate) fn owned_data_capacity_bytes(&self) -> usize {
        let bytes = size_of::<Self>().saturating_add(self.devices.len().saturating_mul(
            size_of::<NativeInputDeviceKey>() + size_of::<NativePointerDeviceEntry>(),
        ));
        self.devices.values().fold(bytes, |bytes, entry| {
            let NativePointerDeviceEntry::Live(device) = entry else {
                return bytes;
            };
            bytes.saturating_add(device.mouse.buttons.len().saturating_mul(
                size_of::<NativePointerButton>() + size_of::<NativeMouseButtonState>(),
            ))
        })
    }

    /// Promises the current dynamic data and its overlapping retry candidate.
    /// The permanent inline translator is counted by its coordinator owner.
    pub(crate) fn reserved_translation_capacity_bytes(&self) -> usize {
        size_of::<Self>().saturating_add(
            self.owned_data_capacity_bytes()
                .saturating_sub(size_of::<Self>())
                .saturating_mul(2),
        )
    }

    /// One callback can add at most one device and one button, in both copies.
    /// Existing terminal transitions only replace entries in place.
    pub(crate) const fn callback_translation_capacity_bytes() -> usize {
        2 * (size_of::<NativeInputDeviceKey>()
            + size_of::<NativePointerDeviceEntry>()
            + size_of::<NativePointerButton>()
            + size_of::<NativeMouseButtonState>())
    }

    pub(crate) fn can_coalesce_idle_cursor_moves(&self) -> bool {
        self.devices.values().all(|entry| match entry {
            NativePointerDeviceEntry::Live(device) => device.is_idle(),
            NativePointerDeviceEntry::Removed | NativePointerDeviceEntry::ScrollTail => true,
        })
    }

    /// Finds an existing winit mouse device and its original Primary identity, if supplied.
    ///
    /// Native stationary continuations require an ongoing Primary press. This lookup
    /// never enrolls a device or revives a retired identity.
    pub(crate) fn mouse_press_for_pointer(
        &self,
        pointer: NativePointerId,
    ) -> Option<(DeviceId, Option<&PointerPressToken>)> {
        self.devices.iter().find_map(|(key, entry)| {
            let (
                NativeInputDeviceKey::Mouse(NativePhysicalDeviceKey::Winit(device)),
                NativePointerDeviceEntry::Live(state),
            ) = (key, entry)
            else {
                return None;
            };
            if state.pointer != pointer {
                return None;
            }
            let NativeMouseButtonState::Active { primary_press, .. } =
                state.mouse.buttons.get(&NativePointerButton::Primary)?
            else {
                return None;
            };
            Some((*device, primary_press.as_ref()))
        })
    }

    #[cfg(test)]
    pub(crate) fn mouse_device_for_pointer(&self, pointer: NativePointerId) -> Option<DeviceId> {
        self.mouse_press_for_pointer(pointer)
            .map(|(device, _)| device)
    }

    /// Forgets adapter state after core overload cancellation commits.
    ///
    /// Core already terminated the semantic streams. Only phaseful scroll
    /// lanes retain their existing slot until the next explicit start.
    pub(crate) fn finish_overload_cancellation(&mut self) {
        self.devices.retain(|_, entry| match entry {
            NativePointerDeviceEntry::Live(device) => {
                if matches!(device.scroll, NativeScrollState::Idle) {
                    false
                } else {
                    *entry = NativePointerDeviceEntry::ScrollTail;
                    true
                }
            }
            NativePointerDeviceEntry::Removed | NativePointerDeviceEntry::ScrollTail => true,
        });
    }

    /// Installs local tail suppression after core records binding retirement.
    ///
    /// Touch tombstones and cancelled mouse buttons reuse their original slots;
    /// destruction never needs new adapter storage after the fact is accepted.
    pub(crate) fn retire_binding(&mut self, binding: NativeSurfaceBinding) {
        for (key, entry) in &mut self.devices {
            if let NativePointerDeviceEntry::Live(device) = entry
                && device.retire_binding(binding)
                && matches!(key, NativeInputDeviceKey::Touch { .. })
            {
                *entry = NativePointerDeviceEntry::Removed;
            }
        }
    }

    /// Retains each removed identity's existing slot as a tail gate.
    ///
    /// Winit raw-device identifiers cannot select a subset of the virtual
    /// identities carried by window input. An explicit start allocates a fresh
    /// identity in the same slot; late tails cannot revive the previous one.
    pub(crate) fn forget_removed_devices(&mut self) {
        for entry in self.devices.values_mut() {
            *entry = NativePointerDeviceEntry::Removed;
        }
    }

    pub(crate) fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.devices.values().any(|entry| {
            matches!(entry, NativePointerDeviceEntry::Live(device) if device.references_binding(binding))
        })
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

    /// Retains the journal's original observation time across delayed replay.
    pub(crate) fn translate_captured(
        &mut self,
        record: &NativeWindowEventRecord,
        captured_at: Duration,
        resolve_work_area: impl FnMut(PhysicalPoint) -> Option<NativeWorkAreaBinding>,
    ) -> NativePointerTranslation {
        let translation = self.translate(record, resolve_work_area);
        #[cfg(test)]
        if let Some((ticks, cost)) = &self.translation_clock_cost {
            let _ = ticks.fetch_update(
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
                |current| Some(current.saturating_add(*cost)),
            );
        }
        let NativePointerTranslation::Input(input) = translation else {
            return translation;
        };
        let device = match record.event() {
            WindowEvent::Touch(_) => PointerDeviceKind::Touch,
            WindowEvent::CursorMoved { .. }
            | WindowEvent::MouseInput { .. }
            | WindowEvent::PointerCaptureChanged { .. } => PointerDeviceKind::Mouse,
            _ => PointerDeviceKind::Unknown,
        };
        let mut metadata = PointerEventMetadata::new(device, captured_at);
        let modifiers = match record.event() {
            WindowEvent::CursorMoved { facts, .. } | WindowEvent::MouseInput { facts, .. } => {
                facts.modifiers
            }
            WindowEvent::Touch(touch) => touch.facts.modifiers,
            _ => None,
        };
        // Missing event-time modifiers preserve the gesture's targeting intent.
        if let Some(modifiers) = modifiers {
            metadata = metadata.with_magnetic_targeting(!modifiers.shift_key());
        }
        NativePointerTranslation::Input(input.with_metadata(metadata))
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
        if matches!(
            self.devices.get(&device),
            Some(NativePointerDeviceEntry::Removed | NativePointerDeviceEntry::ScrollTail)
        ) && !starts_reused_device_stream(record.event())
        {
            return NativePointerTranslation::Ignored;
        }

        match record.event() {
            WindowEvent::CursorMoved { .. } => self.input(
                device,
                NativePointerEvent::Moved,
                record,
                routes,
                &mut resolve_work_area,
            ),
            WindowEvent::MouseInput { state, button, .. } => {
                let button = native_button(*button);
                let event = match state {
                    ElementState::Pressed => NativePointerEvent::ButtonPressed(button),
                    ElementState::Released => NativePointerEvent::ButtonReleased(button),
                };
                self.input(device, event, record, routes, &mut resolve_work_area)
            }
            WindowEvent::PointerCaptureChanged { .. } => self.input(
                device,
                NativePointerEvent::CaptureChanged,
                record,
                routes,
                &mut resolve_work_area,
            ),
            WindowEvent::MouseWheel { delta, phase, .. } => {
                let Some(delta) = delta_value(delta) else {
                    return NativePointerTranslation::Ignored;
                };
                self.scroll(
                    device,
                    delta,
                    *phase,
                    record,
                    routes,
                    &mut resolve_work_area,
                )
            }
            WindowEvent::PanGesture { delta, phase, .. } => {
                let delta = NativeScrollDelta::PhysicalPixels {
                    x: f64::from(delta.x),
                    y: f64::from(delta.y),
                };
                if !scroll_delta_is_finite(delta) {
                    return NativePointerTranslation::Ignored;
                }
                self.scroll(
                    device,
                    delta,
                    *phase,
                    record,
                    routes,
                    &mut resolve_work_area,
                )
            }
            WindowEvent::Touch(_) => self.touch(device, record, routes, &mut resolve_work_area),
            _ => unreachable!("pointer event classification and translation must stay aligned"),
        }
    }

    fn touch(
        &mut self,
        device: NativeInputDeviceKey,
        record: &NativeWindowEventRecord,
        routes: NativePointerRoutes,
        resolve_work_area: &mut impl FnMut(PhysicalPoint) -> Option<NativeWorkAreaBinding>,
    ) -> NativePointerTranslation {
        let WindowEvent::Touch(touch) = record.event() else {
            unreachable!("touch translation only receives touch records");
        };
        let enrolled = matches!(
            self.devices.get(&device),
            Some(NativePointerDeviceEntry::Live(_))
        );
        if matches!(touch.phase, TouchPhase::Started) == enrolled {
            return NativePointerTranslation::Ignored;
        }
        if matches!(touch.phase, TouchPhase::Moved)
            && self
                .devices
                .get(&device)
                .is_some_and(|entry| entry.active_touch_origin_mismatch(routes))
        {
            return NativePointerTranslation::Ignored;
        }
        let routes = if matches!(touch.phase, TouchPhase::Cancelled)
            || matches!(touch.phase, TouchPhase::Ended)
                && self
                    .devices
                    .get(&device)
                    .is_some_and(|entry| entry.active_touch_origin_mismatch(routes))
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
        let translation = self.input(device, event, record, routes, resolve_work_area);
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
        record: &NativeWindowEventRecord,
        routes: NativePointerRoutes,
        resolve_work_area: &mut impl FnMut(PhysicalPoint) -> Option<NativeWorkAreaBinding>,
    ) -> NativePointerTranslation {
        let Some(device) = self.device_mut(device) else {
            return NativePointerTranslation::Ignored;
        };
        let original_primary_press = matches!(
            event,
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary)
        ) && !matches!(
            device.mouse.buttons.get(&NativePointerButton::Primary),
            Some(NativeMouseButtonState::Active { .. })
        );
        if !device.mouse.observe(event, routes) {
            return NativePointerTranslation::Ignored;
        }
        if original_primary_press
            && let WindowEvent::MouseInput {
                primary_press: source,
                ..
            } = record.event()
            && let Some(NativeMouseButtonState::Active { primary_press, .. }) =
                device.mouse.buttons.get_mut(&NativePointerButton::Primary)
        {
            // Only the original accepted Primary edge may bind this identity. Repeated edges,
            // motion and observation results cannot supplement an absent or revoked token.
            *primary_press = source.clone();
        }
        NativePointerTranslation::Input(native_input(
            device.pointer,
            event,
            record,
            routes,
            resolve_work_area,
        ))
    }

    fn scroll(
        &mut self,
        device: NativeInputDeviceKey,
        delta: NativeScrollDelta,
        phase: TouchPhase,
        record: &NativeWindowEventRecord,
        routes: NativePointerRoutes,
        resolve_work_area: &mut impl FnMut(PhysicalPoint) -> Option<NativeWorkAreaBinding>,
    ) -> NativePointerTranslation {
        if self.device_mut(device).is_none() {
            return NativePointerTranslation::Ignored;
        }
        let Self {
            devices,
            next_scroll_sequence,
            ..
        } = self;
        let Some(NativePointerDeviceEntry::Live(device)) = devices.get_mut(&device) else {
            unreachable!("a successfully enrolled device remains live in the adapter map");
        };
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

        let modifiers = match record.event() {
            WindowEvent::MouseWheel { facts, .. } => native_modifiers(facts),
            WindowEvent::PanGesture { .. } => NativeScrollModifiers::Unknown,
            _ => unreachable!("scroll translation only receives wheel or pan records"),
        };
        let event = NativeScrollEvent::new(
            device.scroll_device,
            phase,
            NativeScrollMomentum::Unknown,
            modifiers,
        );
        NativePointerTranslation::Input(native_input(
            device.pointer,
            NativePointerEvent::Scrolled(event),
            record,
            routes,
            resolve_work_area,
        ))
    }

    fn device_mut(&mut self, key: NativeInputDeviceKey) -> Option<&mut NativePointerDeviceState> {
        let Self {
            devices,
            next_device_identity,
            ..
        } = self;
        let entry = match devices.entry(key) {
            Entry::Occupied(entry) => {
                let entry = entry.into_mut();
                if !matches!(entry, NativePointerDeviceEntry::Live(_)) {
                    let next = allocate_device_identity(next_device_identity)?;
                    *entry = NativePointerDeviceEntry::Live(NativePointerDeviceState::new(next));
                }
                entry
            }
            Entry::Vacant(entry) => {
                let next = allocate_device_identity(next_device_identity)?;
                entry.insert(NativePointerDeviceEntry::Live(
                    NativePointerDeviceState::new(next),
                ))
            }
        };
        match entry {
            NativePointerDeviceEntry::Live(device) => Some(device),
            NativePointerDeviceEntry::Removed | NativePointerDeviceEntry::ScrollTail => {
                unreachable!("device enrollment installs a live identity")
            }
        }
    }
}

fn native_input(
    pointer: NativePointerId,
    event: NativePointerEvent,
    record: &NativeWindowEventRecord,
    routes: NativePointerRoutes,
    resolve_work_area: &mut impl FnMut(PhysicalPoint) -> Option<NativeWorkAreaBinding>,
) -> NativePointerInput {
    let delivery = native_owner(routes.delivery());
    let capture = native_owner(routes.capture());
    match record
        .pointer_location()
        .unwrap_or(CapturedNativePointerLocation::Desktop)
    {
        CapturedNativePointerLocation::Desktop => NativePointerInput::new(
            pointer,
            event,
            native_desktop_location(record, routes, resolve_work_area),
            delivery,
            capture,
        ),
        CapturedNativePointerLocation::NativeLocal(location) => {
            NativePointerInput::new_local(pointer, event, location, delivery, capture)
        }
        CapturedNativePointerLocation::LocalToDesktop(sample) => NativePointerInput::new_handoff(
            pointer,
            event,
            NativeLocalToDesktopHandoff::new(
                sample,
                native_desktop_location(record, routes, resolve_work_area),
            ),
            delivery,
            capture,
        ),
    }
}

fn native_desktop_location(
    record: &NativeWindowEventRecord,
    routes: NativePointerRoutes,
    resolve_work_area: &mut impl FnMut(PhysicalPoint) -> Option<NativeWorkAreaBinding>,
) -> NativeDesktopPointerLocation {
    let desktop_position = match record.event() {
        WindowEvent::CursorMoved { facts, .. }
        | WindowEvent::MouseInput { facts, .. }
        | WindowEvent::MouseWheel { facts, .. } => facts.desktop_position,
        _ => None,
    };
    let position = native_position(desktop_position);
    let hover = native_hover(routes.hover());
    NativeDesktopPointerLocation::new(
        position,
        hover,
        native_work_area(position, hover, resolve_work_area),
    )
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

impl NativePointerDeviceEntry {
    fn active_touch_origin_mismatch(&self, routes: NativePointerRoutes) -> bool {
        matches!(self, Self::Live(device) if device.mouse.active_origin_mismatch(routes))
    }
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
        self.buttons.is_empty() && self.capture.is_none()
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
                if matches!(self.buttons.remove(&button), Some(NativeMouseButtonState::Cancelled))
        ) {
            return false;
        }
        match event {
            NativePointerEvent::ButtonPressed(button) => {
                let delivery = exact_delivery_binding(routes.delivery());
                match self.buttons.entry(button) {
                    Entry::Occupied(mut entry) => match entry.get_mut() {
                        NativeMouseButtonState::Active { origin, .. } => {
                            if origin.is_none() {
                                *origin = delivery;
                            }
                        }
                        state @ NativeMouseButtonState::Cancelled => {
                            *state = NativeMouseButtonState::Active {
                                origin: delivery,
                                primary_press: None,
                            };
                        }
                    },
                    Entry::Vacant(entry) => {
                        entry.insert(NativeMouseButtonState::Active {
                            origin: delivery,
                            primary_press: None,
                        });
                    }
                }
            }
            NativePointerEvent::ButtonReleased(button)
            | NativePointerEvent::ContactEnded(button) => {
                let _ = self.buttons.remove(&button);
            }
            NativePointerEvent::StreamEnded | NativePointerEvent::StreamCancelled(_) => {
                self.buttons.clear();
                self.capture = None;
            }
            NativePointerEvent::CaptureChanged => {
                if let Some(NativeMouseButtonState::Active { primary_press, .. }) =
                    self.buttons.get_mut(&NativePointerButton::Primary)
                {
                    *primary_press = None;
                }
            }
            NativePointerEvent::Moved
            | NativePointerEvent::Continuation(_)
            | NativePointerEvent::CaptureAcquired(_)
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
            && self
                .buttons
                .values()
                .any(|state| matches!(state, NativeMouseButtonState::Active { .. }))
            && exact_delivery_binding(routes.capture())
                .is_some_and(|capture| exact_delivery_binding(routes.delivery()) != Some(capture))
    }

    fn active_origin_mismatch(&self, routes: NativePointerRoutes) -> bool {
        self.buttons
            .get(&NativePointerButton::Primary)
            .is_some_and(|state| {
                matches!(state, NativeMouseButtonState::Active { origin: Some(origin), .. }
                    if exact_delivery_binding(routes.delivery()) != Some(*origin))
            })
    }

    fn observe_routes(&mut self, routes: NativePointerRoutes) {
        self.capture = updated_binding(self.capture, routes.capture());
    }

    fn retire_binding(&mut self, binding: NativeSurfaceBinding) -> bool {
        if !self.references_binding(binding) {
            return false;
        }
        for state in self.buttons.values_mut() {
            *state = NativeMouseButtonState::Cancelled;
        }
        self.capture = None;
        true
    }

    fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.buttons
            .values()
            .any(|state| {
                matches!(state, NativeMouseButtonState::Active { origin: Some(origin), .. } if *origin == binding)
            })
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
    mod magnetic_targeting;

    use super::*;

    fn assert_read_only_mouse_device_lookup(
        translator: &NativePointerTranslator,
        pointer: NativePointerId,
        expected: Option<DeviceId>,
    ) {
        let storage_and_watermarks = || {
            (
                translator.next_device_identity,
                translator.next_scroll_sequence,
                translator.devices.len(),
                translator.owned_data_capacity_bytes(),
                translator.reserved_translation_capacity_bytes(),
            )
        };
        let before = storage_and_watermarks();
        for _ in 0..2 {
            assert_eq!(translator.mouse_device_for_pointer(pointer), expected);
        }
        assert_eq!(storage_and_watermarks(), before);
    }

    #[test]
    fn mouse_device_lookup_is_read_only_and_requires_active_primary() {
        let mut translator = NativePointerTranslator::default();
        assert_read_only_mouse_device_lookup(&translator, NativePointerId::new(1), None);

        let device = DeviceId::dummy();
        let key = NativeInputDeviceKey::Mouse(NativePhysicalDeviceKey::Winit(device));
        let pointer = translator.device_mut(key).expect("mouse enrolls").pointer;
        assert_read_only_mouse_device_lookup(&translator, pointer, None);
        let routes = NativePointerRoutes::from_binding(None);
        assert!(translator.device_mut(key).unwrap().mouse.observe(
            NativePointerEvent::ButtonPressed(NativePointerButton::Secondary),
            routes,
        ));
        assert_read_only_mouse_device_lookup(&translator, pointer, None);
        assert!(translator.device_mut(key).unwrap().mouse.observe(
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            routes,
        ));
        assert_read_only_mouse_device_lookup(&translator, pointer, Some(device));
        assert_read_only_mouse_device_lookup(&translator, NativePointerId::new(u64::MAX), None);
        assert!(translator.device_mut(key).unwrap().mouse.observe(
            NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
            routes,
        ));
        assert!(
            translator
                .device_mut(key)
                .unwrap()
                .mouse
                .buttons
                .contains_key(&NativePointerButton::Secondary)
        );
        assert_read_only_mouse_device_lookup(&translator, pointer, None);
        assert!(translator.device_mut(key).unwrap().mouse.observe(
            NativePointerEvent::ButtonReleased(NativePointerButton::Secondary),
            routes,
        ));
        assert_read_only_mouse_device_lookup(&translator, pointer, None);
    }

    #[test]
    fn mouse_device_lookup_does_not_borrow_touch_scroll_or_test_devices() {
        let mut translator = NativePointerTranslator::default();
        let device = DeviceId::dummy();
        let physical = NativePhysicalDeviceKey::Winit(device);
        let mut other_pointers = Vec::new();
        for key in [
            NativeInputDeviceKey::Mouse(NativePhysicalDeviceKey::Test(1)),
            NativeInputDeviceKey::Touch {
                device: physical,
                contact: 7,
            },
            NativeInputDeviceKey::Scroll(physical),
        ] {
            let state = translator.device_mut(key).expect("device enrolls");
            // An active button cannot grant a different device lane mouse access.
            state.mouse.buttons.insert(
                NativePointerButton::Primary,
                NativeMouseButtonState::Active {
                    origin: None,
                    primary_press: None,
                },
            );
            other_pointers.push(state.pointer);
        }
        for pointer in &other_pointers {
            assert_read_only_mouse_device_lookup(&translator, *pointer, None);
        }

        let mouse = translator
            .device_mut(NativeInputDeviceKey::Mouse(physical))
            .expect("mouse enrolls separately");
        mouse.mouse.buttons.insert(
            NativePointerButton::Primary,
            NativeMouseButtonState::Active {
                origin: None,
                primary_press: None,
            },
        );
        let pointer = mouse.pointer;
        assert_read_only_mouse_device_lookup(&translator, pointer, Some(device));
        for pointer in other_pointers {
            assert_read_only_mouse_device_lookup(&translator, pointer, None);
        }
    }

    #[test]
    fn mouse_device_lookup_invalidates_retired_removed_and_overloaded_mouse() {
        use dockspace::model::{
            DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
            RootId, SurfaceId,
        };
        use dockspace::policy::DockPolicy;
        use dockspace::runtime::{
            DockspaceSession, HostInputOutcome, HostWindowToken, NativeCreateStrategy,
            NativePointerRoster, SurfaceUnavailableReason,
        };

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
            .enable_managed_native_host(NativeCreateStrategy::Staged, NativePointerRoster::Unknown)
            .unwrap();
        session
            .register_native_root(surface, HostWindowToken::new(1))
            .unwrap();
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let report = frame.commit().unwrap();
        let binding = report
            .inputs()
            .iter()
            .find_map(|outcome| match outcome {
                HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
                _ => None,
            })
            .expect("registration emits a binding");

        let mut translator = NativePointerTranslator::default();
        let device = DeviceId::dummy();
        let key = NativeInputDeviceKey::Mouse(NativePhysicalDeviceKey::Winit(device));
        let mouse = translator.device_mut(key).expect("mouse enrolls");
        mouse.mouse.buttons.insert(
            NativePointerButton::Primary,
            NativeMouseButtonState::Active {
                origin: Some(binding),
                primary_press: None,
            },
        );
        let retired_pointer = mouse.pointer;
        assert_read_only_mouse_device_lookup(&translator, retired_pointer, Some(device));

        translator.retire_binding(binding);
        assert!(matches!(
            translator.devices.get(&key),
            Some(NativePointerDeviceEntry::Live(state))
                if matches!(
                    state.mouse.buttons.get(&NativePointerButton::Primary),
                    Some(NativeMouseButtonState::Cancelled)
                )
        ));
        assert_read_only_mouse_device_lookup(&translator, retired_pointer, None);

        translator.forget_removed_devices();
        assert!(matches!(
            translator.devices.get(&key),
            Some(NativePointerDeviceEntry::Removed)
        ));
        assert_read_only_mouse_device_lookup(&translator, retired_pointer, None);

        let mouse = translator.device_mut(key).expect("fresh mouse enrolls");
        mouse.mouse.buttons.insert(
            NativePointerButton::Primary,
            NativeMouseButtonState::Active {
                origin: None,
                primary_press: None,
            },
        );
        let overloaded_pointer = mouse.pointer;
        assert_ne!(overloaded_pointer, retired_pointer);
        assert_read_only_mouse_device_lookup(&translator, retired_pointer, None);
        assert_read_only_mouse_device_lookup(&translator, overloaded_pointer, Some(device));

        translator.finish_overload_cancellation();
        assert!(!translator.devices.contains_key(&key));
        assert_read_only_mouse_device_lookup(&translator, overloaded_pointer, None);
        let mouse = translator
            .device_mut(key)
            .expect("post-overload mouse enrolls");
        mouse.mouse.buttons.insert(
            NativePointerButton::Primary,
            NativeMouseButtonState::Active {
                origin: None,
                primary_press: None,
            },
        );
        let fresh_pointer = mouse.pointer;
        assert_ne!(fresh_pointer, overloaded_pointer);
        assert_read_only_mouse_device_lookup(&translator, overloaded_pointer, None);
        assert_read_only_mouse_device_lookup(&translator, fresh_pointer, Some(device));

        *translator.devices.get_mut(&key).unwrap() = NativePointerDeviceEntry::ScrollTail;
        assert_read_only_mouse_device_lookup(&translator, fresh_pointer, None);
    }

    #[test]
    fn desktop_location_uses_only_same_record_desktop_facts_and_frozen_hover() {
        use crate::viewport_map::NativeViewportMap;
        use winit::dpi::PhysicalPosition;
        use winit::event::{PointerEventFacts, PointerWindowRoute};

        let point = PhysicalPoint::new(110.0, -220.0).unwrap();
        let raw_point = Some(PhysicalPosition::new(point.x(), point.y()));
        for (desktop_position, hover, expected_position, expected_hover, expected_calls) in [
            (
                raw_point,
                PointerWindowRoute::Foreign,
                NativeDesktopPosition::Exact(point),
                NativePointerHover::Foreign,
                0,
            ),
            (
                raw_point,
                PointerWindowRoute::None,
                NativeDesktopPosition::Exact(point),
                NativePointerHover::OutsideAll,
                1,
            ),
            (
                None,
                PointerWindowRoute::None,
                NativeDesktopPosition::Unknown,
                NativePointerHover::OutsideAll,
                0,
            ),
            (
                Some(PhysicalPosition::new(f64::NAN, 0.0)),
                PointerWindowRoute::None,
                NativeDesktopPosition::Unknown,
                NativePointerHover::OutsideAll,
                0,
            ),
            (
                Some(PhysicalPosition::new(0.0, f64::INFINITY)),
                PointerWindowRoute::None,
                NativeDesktopPosition::Unknown,
                NativePointerHover::OutsideAll,
                0,
            ),
        ] {
            let record = NativeWindowEventRecord::for_test_snapshot(
                1,
                winit::window::WindowId::from(1),
                None,
                WindowEvent::CursorMoved {
                    device_id: DeviceId::dummy(),
                    position: PhysicalPosition::new(900.0, 800.0),
                    facts: PointerEventFacts {
                        desktop_position,
                        hover,
                        ..PointerEventFacts::default()
                    },
                },
                &NativeViewportMap::default(),
            );
            let mut calls = 0;
            let location = native_desktop_location(
                &record,
                record.pointer_routes().unwrap(),
                &mut |observed| {
                    calls += 1;
                    assert_eq!(observed, point);
                    None
                },
            );
            assert_eq!(
                location,
                NativeDesktopPointerLocation::new(expected_position, expected_hover, None)
            );
            assert_eq!(calls, expected_calls);
        }
    }

    #[test]
    fn owned_capacity_keeps_device_slots_charged_when_they_become_tombstones() {
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
        assert_eq!(translator.devices.len(), 1);
        assert!(matches!(
            translator.devices.get(&mouse),
            Some(NativePointerDeviceEntry::Removed)
        ));
        let removed = translator.owned_data_capacity_bytes();
        assert!(removed > empty);
        assert_eq!(removed, idle);

        let scroll = NativeInputDeviceKey::Scroll(NativePhysicalDeviceKey::Test(2));
        translator
            .device_mut(scroll)
            .expect("scroll device enrolls")
            .scroll = NativeScrollState::Live {
            sequence: NativeScrollSequenceId::new(1),
            delivery: None,
        };
        translator.finish_overload_cancellation();
        assert_eq!(translator.devices.len(), 2);
        assert!(matches!(
            translator.devices.get(&mouse),
            Some(NativePointerDeviceEntry::Removed)
        ));
        assert!(matches!(
            translator.devices.get(&scroll),
            Some(NativePointerDeviceEntry::ScrollTail)
        ));
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

//! Translation from callback-time winit pointer facts into core native input.

use std::collections::BTreeMap;
use std::time::Duration;

use egui_dockspace::DockInputConfig;

use dockspace::geometry::PhysicalPoint;
use dockspace::runtime::{
    NativeDesktopPointerLocation, NativeDesktopPosition, NativeLocalToDesktopHandoff,
    NativePointerButton, NativePointerCancelReason, NativePointerEvent, NativePointerHover,
    NativePointerId, NativePointerInput, NativePointerOwner, NativeScrollDelta,
    NativeWindowBinding, NativeWorkAreaBinding, PointerDeviceKind, PointerEventMetadata,
};
use winit::event::{ElementState, MouseButton, TouchPhase, WindowEvent};
use winit::keyboard::ModifiersState;

use crate::event::{
    CapturedNativePointerLocation, NativePointerRouteSnapshot, NativePointerRoutes,
    NativeWindowEventClass, NativeWindowEventRecord,
};

use self::device::{
    NativeInputDeviceKey, NativePointerDeviceEntry, pointer_device, starts_reused_device_stream,
};
#[cfg(test)]
use self::device::{NativePhysicalDeviceKey, pointer_device_for_physical};
use self::mouse::MouseTransition;
use self::scroll::{delta_value, scroll_delta_is_finite};

mod device;
mod mouse;
mod scroll;

#[derive(Debug, Clone, Default)]
pub(crate) struct NativePointerTranslator {
    devices: BTreeMap<NativeInputDeviceKey, NativePointerDeviceEntry>,
    observed_modifiers: Option<(NativeWindowBinding, ModifiersState)>,
    next_device_identity: u64,
    next_scroll_sequence: u64,
    #[cfg(test)]
    translation_clock_cost: Option<(std::sync::Arc<std::sync::atomic::AtomicU64>, u64)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum NativePointerTranslation {
    NotPointer,
    Ignored,
    Input(NativePointerInput),
    /// One real down replaces a stream whose release never reached this host.
    Restart {
        cancelled: NativePointerInput,
        pressed: NativePointerInput,
    },
}

impl NativePointerTranslator {
    /// Retains one window's latest real modifiers without enrolling a pointer.
    ///
    /// Only a new press delivered through this exact binding may use the
    /// observation as its initial docking intent. Other windows remain unknown.
    pub(crate) fn observe_modifiers(
        &mut self,
        binding: NativeWindowBinding,
        modifiers: ModifiersState,
    ) {
        self.observed_modifiers = Some((binding, modifiers));
    }

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
    pub(crate) fn translate_captured(
        &mut self,
        record: &NativeWindowEventRecord,
        captured_at: Duration,
        resolve_work_area: impl FnMut(PhysicalPoint) -> Option<NativeWorkAreaBinding>,
    ) -> NativePointerTranslation {
        self.translate_captured_with_input(
            record,
            captured_at,
            &DockInputConfig::default(),
            resolve_work_area,
        )
    }

    /// Retains the journal's original observation time across delayed replay.
    pub(crate) fn translate_captured_with_input(
        &mut self,
        record: &NativeWindowEventRecord,
        captured_at: Duration,
        config: &DockInputConfig,
        resolve_work_area: impl FnMut(PhysicalPoint) -> Option<NativeWorkAreaBinding>,
    ) -> NativePointerTranslation {
        let starts_primary_press = self.starts_primary_press(record.event());
        let translation = self.translate(record, resolve_work_area);
        #[cfg(test)]
        if let Some((ticks, cost)) = &self.translation_clock_cost {
            let _ = ticks.try_update(
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
                |current| Some(current.saturating_add(*cost)),
            );
        }
        if matches!(
            translation,
            NativePointerTranslation::NotPointer | NativePointerTranslation::Ignored
        ) {
            return translation;
        }
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
        if let Some(modifiers) = modifiers {
            if let Some(binding) = record.binding() {
                self.observe_modifiers(binding.window(), modifiers);
            }
            metadata =
                metadata.with_docking_enabled(config.docking_enabled(egui_modifiers(modifiers)));
        } else if starts_primary_press {
            // This is an initial gesture policy, not reconstructed event facts.
            // Unknown moves and releases retain core intent.
            let observed = record
                .binding()
                .and_then(|binding| {
                    self.observed_modifiers
                        .filter(|(window, _)| *window == binding.window())
                        .map(|(_, modifiers)| modifiers)
                })
                .unwrap_or_default();
            // Core already enables docking for a new gesture without a directive.
            if !config.docking_enabled(egui_modifiers(observed)) {
                metadata = metadata.with_docking_enabled(false);
            }
        }
        match translation {
            NativePointerTranslation::Input(input) => {
                NativePointerTranslation::Input(input.with_metadata(metadata))
            }
            NativePointerTranslation::Restart { cancelled, pressed } => {
                NativePointerTranslation::Restart {
                    cancelled: cancelled
                        .with_metadata(PointerEventMetadata::new(device, captured_at)),
                    pressed: pressed.with_metadata(metadata),
                }
            }
            NativePointerTranslation::NotPointer | NativePointerTranslation::Ignored => {
                unreachable!("non-input translations returned before metadata decoration")
            }
        }
    }

    fn starts_primary_press(&self, event: &WindowEvent) -> bool {
        if !matches!(
            event,
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } | WindowEvent::Touch(winit::event::Touch {
                phase: TouchPhase::Started,
                ..
            })
        ) {
            return false;
        }
        let Some(device) = pointer_device(event) else {
            return false;
        };
        if let WindowEvent::MouseInput { primary_press, .. } = event {
            self.devices.get(&device).is_none_or(|entry| match entry {
                NativePointerDeviceEntry::Live(device) => {
                    device.mouse.starts_primary_press(primary_press.as_ref())
                }
                NativePointerDeviceEntry::Removed | NativePointerDeviceEntry::ScrollTail => true,
            })
        } else {
            !matches!(
                self.devices.get(&device),
                Some(NativePointerDeviceEntry::Live(device))
                    if device.mouse.is_pressed(NativePointerButton::Primary)
            )
        }
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
        let routes = record.pointer_routes().unwrap_or_else(|| {
            NativePointerRoutes::from_binding(record.binding().map(|binding| binding.window()))
        });
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
        let transition = match (event, record.event()) {
            (
                NativePointerEvent::ButtonPressed(button),
                WindowEvent::MouseInput { primary_press, .. },
            ) => device.mouse.press(button, routes, primary_press.as_ref()),
            _ => device.mouse.observe(event, routes),
        };
        if transition == MouseTransition::Ignore {
            return NativePointerTranslation::Ignored;
        }
        let pressed = native_input(device.pointer, event, record, routes, resolve_work_area);
        if transition == MouseTransition::CancelThenPress {
            // The same callback owns both edges. The coordinator admits them as
            // one batch before publishing this candidate translator state.
            NativePointerTranslation::Restart {
                cancelled: native_input(
                    device.pointer,
                    NativePointerEvent::StreamCancelled(
                        NativePointerCancelReason::ExplicitPlatformCancellation,
                    ),
                    record,
                    routes,
                    resolve_work_area,
                ),
                pressed,
            }
        } else {
            NativePointerTranslation::Input(pressed)
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

fn exact_delivery_binding(route: NativePointerRouteSnapshot) -> Option<NativeWindowBinding> {
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

pub(crate) fn egui_modifiers(
    modifiers: winit::keyboard::ModifiersState,
) -> eframe::egui::Modifiers {
    eframe::egui::Modifiers {
        alt: modifiers.alt_key(),
        ctrl: modifiers.control_key(),
        shift: modifiers.shift_key(),
        mac_cmd: cfg!(target_os = "macos") && modifiers.super_key(),
        command: if cfg!(target_os = "macos") {
            modifiers.super_key()
        } else {
            modifiers.control_key()
        },
    }
}

#[cfg(test)]
mod overload;

#[cfg(test)]
mod tests;

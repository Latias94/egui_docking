//! Translation from callback-time winit pointer facts into core native input.

use std::collections::{BTreeMap, BTreeSet};

use dockspace::geometry::PhysicalPoint;
use dockspace::runtime::{
    NativeDesktopPointerLocation, NativeDesktopPosition, NativePointerButton,
    NativePointerCancelReason, NativePointerEvent, NativePointerHover, NativePointerId,
    NativePointerInput, NativePointerOwner, NativeScrollCancelReason, NativeScrollDelta,
    NativeScrollDeviceId, NativeScrollEvent, NativeScrollModifiers, NativeScrollMomentum,
    NativeScrollPhase, NativeScrollSequenceId, NativeSurfaceBinding, NativeWorkAreaBinding,
};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};

use crate::event::{
    NativePointerRouteSnapshot, NativePointerRoutes, NativeWindowEventClass,
    NativeWindowEventRecord,
};

const POINTER_ID: NativePointerId = NativePointerId::new(1);
const SCROLL_DEVICE_ID: NativeScrollDeviceId = NativeScrollDeviceId::new(1);

#[derive(Debug, Clone, Default)]
pub(crate) struct NativePointerTranslator {
    mouse: NativeMouseState,
    scroll: NativeScrollState,
    next_scroll_sequence: u64,
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
        self.mouse.is_idle() && matches!(self.scroll, NativeScrollState::Idle)
    }

    /// Terminates the semantic owner while preserving provider-tail correlation.
    pub(crate) fn cancel_destroyed_binding(
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
            return Some(pointer_cancel_input(
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
            sequence,
            NativeScrollCancelReason::BindingRetired,
        ))
    }

    pub(crate) fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
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

    pub(crate) const fn has_pending_provider_tail(&self) -> bool {
        self.mouse.has_pending_provider_tail()
            || matches!(self.scroll, NativeScrollState::RetiredTail { .. })
    }

    /// Closes one retired provider tail at the next causal boundary.
    pub(crate) fn reset_destroyed_binding(
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
            sequence,
            NativeScrollCancelReason::ProviderReset,
        ))
    }

    pub(crate) fn translate(
        &mut self,
        record: &NativeWindowEventRecord,
        mut resolve_work_area: impl FnMut(PhysicalPoint) -> Option<NativeWorkAreaBinding>,
    ) -> NativePointerTranslation {
        if NativeWindowEventClass::classify(record.event()) != NativeWindowEventClass::Pointer {
            return NativePointerTranslation::NotPointer;
        }
        let routes = record
            .pointer_routes()
            .unwrap_or_else(|| NativePointerRoutes::from_binding(record.binding()));

        match record.event() {
            WindowEvent::CursorMoved { facts, .. } => self.input(
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
                    event,
                    facts.desktop_position,
                    routes,
                    &mut resolve_work_area,
                )
            }
            WindowEvent::PointerCaptureChanged { .. } => self.input(
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
                let delta = match delta_value(delta) {
                    Some(delta) => delta,
                    None => return NativePointerTranslation::Ignored,
                };
                self.scroll(delta, *phase, facts, routes, &mut resolve_work_area)
            }
            WindowEvent::PanGesture { delta, phase, .. } => {
                let delta = NativeScrollDelta::PhysicalPixels {
                    x: f64::from(delta.x),
                    y: f64::from(delta.y),
                };
                let finite = match delta {
                    NativeScrollDelta::PhysicalPixels { x, y } => x.is_finite() && y.is_finite(),
                    _ => true,
                };
                if !finite {
                    return NativePointerTranslation::Ignored;
                }
                let facts = winit::event::PointerEventFacts::default();
                self.scroll(delta, *phase, &facts, routes, &mut resolve_work_area)
            }
            _ => unreachable!("pointer event classification and translation must stay aligned"),
        }
    }

    fn input(
        &mut self,
        event: NativePointerEvent,
        desktop_position: Option<winit::dpi::PhysicalPosition<f64>>,
        routes: NativePointerRoutes,
        resolve_work_area: &mut impl FnMut(PhysicalPoint) -> Option<NativeWorkAreaBinding>,
    ) -> NativePointerTranslation {
        if !self.mouse.observe(event, routes) {
            return NativePointerTranslation::Ignored;
        }
        let position = native_position(desktop_position);
        let hover = native_hover(routes.hover());
        NativePointerTranslation::Input(NativePointerInput::new(
            POINTER_ID,
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
        delta: NativeScrollDelta,
        phase: TouchPhase,
        facts: &winit::event::PointerEventFacts,
        routes: NativePointerRoutes,
        resolve_work_area: &mut impl FnMut(PhysicalPoint) -> Option<NativeWorkAreaBinding>,
    ) -> NativePointerTranslation {
        let delivery = exact_delivery_binding(routes.delivery());
        if let NativeScrollState::Live {
            delivery: Some(active),
            ..
        } = self.scroll
            && delivery != Some(active)
        {
            // Winit does not carry a scroll sequence token. A callback routed
            // through another exact binding may be a delayed predecessor tail,
            // so it cannot mutate or terminate the current sequence.
            return NativePointerTranslation::Ignored;
        }
        self.mouse.observe_routes(routes);
        let (phase, next_state) = match (self.scroll, phase) {
            (NativeScrollState::Idle, TouchPhase::Started) => {
                let Some(sequence) = self.allocate_scroll_sequence() else {
                    return NativePointerTranslation::Ignored;
                };
                (
                    Some(NativeScrollPhase::Begin {
                        sequence,
                        delta: Some(delta),
                    }),
                    NativeScrollState::Live { sequence, delivery },
                )
            }
            (
                NativeScrollState::Live { sequence, delivery },
                TouchPhase::Started | TouchPhase::Moved,
            ) => (
                Some(NativeScrollPhase::Update { sequence, delta }),
                NativeScrollState::Live { sequence, delivery },
            ),
            (NativeScrollState::Live { sequence, .. }, TouchPhase::Ended) => (
                Some(NativeScrollPhase::End {
                    sequence,
                    delta: Some(delta),
                }),
                NativeScrollState::Idle,
            ),
            (NativeScrollState::Live { sequence, .. }, TouchPhase::Cancelled) => (
                Some(NativeScrollPhase::Cancel {
                    sequence,
                    reason: NativeScrollCancelReason::PlatformCancelled,
                }),
                NativeScrollState::Idle,
            ),
            (
                NativeScrollState::RetiredTail { sequence, .. },
                TouchPhase::Started | TouchPhase::Moved,
            ) => (
                Some(NativeScrollPhase::Cancel {
                    sequence,
                    reason: NativeScrollCancelReason::ProviderReset,
                }),
                NativeScrollState::Idle,
            ),
            (NativeScrollState::RetiredTail { sequence, .. }, TouchPhase::Ended) => (
                Some(NativeScrollPhase::End {
                    sequence,
                    delta: Some(delta),
                }),
                NativeScrollState::Idle,
            ),
            (NativeScrollState::RetiredTail { sequence, .. }, TouchPhase::Cancelled) => (
                Some(NativeScrollPhase::Cancel {
                    sequence,
                    reason: NativeScrollCancelReason::PlatformCancelled,
                }),
                NativeScrollState::Idle,
            ),
            (NativeScrollState::Idle, TouchPhase::Moved) => (
                Some(NativeScrollPhase::Discrete { delta }),
                NativeScrollState::Idle,
            ),
            (NativeScrollState::Idle, TouchPhase::Ended) => {
                if is_zero_delta(delta) {
                    (None, NativeScrollState::Idle)
                } else {
                    (
                        Some(NativeScrollPhase::Discrete { delta }),
                        NativeScrollState::Idle,
                    )
                }
            }
            (NativeScrollState::Idle, TouchPhase::Cancelled) => (None, NativeScrollState::Idle),
        };
        self.scroll = next_state;
        let Some(phase) = phase else {
            return NativePointerTranslation::Ignored;
        };

        let event = NativeScrollEvent::new(
            SCROLL_DEVICE_ID,
            phase,
            NativeScrollMomentum::Unknown,
            native_modifiers(facts),
        );
        let position = native_position(facts.desktop_position);
        let hover = native_hover(routes.hover());
        NativePointerTranslation::Input(NativePointerInput::new(
            POINTER_ID,
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

    fn allocate_scroll_sequence(&mut self) -> Option<NativeScrollSequenceId> {
        let next = self.next_scroll_sequence.checked_add(1)?;
        self.next_scroll_sequence = next;
        Some(NativeScrollSequenceId::new(next))
    }
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
            | NativePointerEvent::CaptureChanged
            | NativePointerEvent::Scrolled(_) => {}
        }
        true
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

fn pointer_cancel_input(reason: NativePointerCancelReason) -> NativePointerInput {
    NativePointerInput::new(
        POINTER_ID,
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
    sequence: NativeScrollSequenceId,
    reason: NativeScrollCancelReason,
) -> NativePointerInput {
    NativePointerInput::new(
        POINTER_ID,
        NativePointerEvent::Scrolled(NativeScrollEvent::new(
            SCROLL_DEVICE_ID,
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
    let finite = match delta {
        NativeScrollDelta::PhysicalPixels { x, y }
        | NativeScrollDelta::LogicalPoints { x, y }
        | NativeScrollDelta::Lines { x, y }
        | NativeScrollDelta::Pages { x, y } => x.is_finite() && y.is_finite(),
    };
    finite.then_some(delta)
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
mod tests {
    use super::*;

    #[test]
    fn idle_cursor_coalescing_stops_for_buttons_and_scroll() {
        let routes = NativePointerRoutes::from_binding(None);
        let mut translator = NativePointerTranslator::default();
        assert!(translator.can_coalesce_idle_cursor_moves());

        assert!(translator.mouse.observe(
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            routes
        ));
        assert!(!translator.can_coalesce_idle_cursor_moves());

        assert!(translator.mouse.observe(
            NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
            routes
        ));
        assert!(translator.can_coalesce_idle_cursor_moves());

        translator.scroll = NativeScrollState::Live {
            sequence: NativeScrollSequenceId::new(1),
            delivery: None,
        };
        assert!(!translator.can_coalesce_idle_cursor_moves());
    }
}

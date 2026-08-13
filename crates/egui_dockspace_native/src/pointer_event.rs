//! Translation from callback-time winit pointer facts into core native input.

use dockspace::geometry::PhysicalPoint;
use dockspace::runtime::{
    NativeDesktopPointerLocation, NativeDesktopPosition, NativePointerButton, NativePointerEvent,
    NativePointerHover, NativePointerId, NativePointerInput, NativePointerOwner,
    NativeScrollCancelReason, NativeScrollDelta, NativeScrollDeviceId, NativeScrollEvent,
    NativeScrollModifiers, NativeScrollMomentum, NativeScrollPhase, NativeScrollSequenceId,
    NativeWorkAreaBinding,
};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};

use crate::NativeWindowEventRecord;
use crate::event::{NativePointerRouteSnapshot, NativePointerRoutes};

const POINTER_ID: NativePointerId = NativePointerId::new(1);
const SCROLL_DEVICE_ID: NativeScrollDeviceId = NativeScrollDeviceId::new(1);

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct NativePointerTranslator {
    active_scroll: Option<NativeScrollSequenceId>,
    next_scroll_sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum NativePointerTranslation {
    NotPointer,
    Ignored,
    Input(NativePointerInput),
}

impl NativePointerTranslator {
    pub(crate) fn translate(
        &mut self,
        record: &NativeWindowEventRecord,
        mut resolve_work_area: impl FnMut(PhysicalPoint) -> Option<NativeWorkAreaBinding>,
    ) -> NativePointerTranslation {
        let routes = record
            .pointer_routes()
            .unwrap_or_else(|| NativePointerRoutes::from_binding(record.binding()));

        match record.event() {
            WindowEvent::CursorMoved { facts, .. } => {
                self.input(
                    NativePointerEvent::Moved,
                    facts.desktop_position,
                    routes,
                    &mut resolve_work_area,
                )
            }
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
                self.input(event, facts.desktop_position, routes, &mut resolve_work_area)
            }
            WindowEvent::PointerCaptureChanged { .. } => {
                self.input(
                    NativePointerEvent::CaptureChanged,
                    None,
                    routes,
                    &mut resolve_work_area,
                )
            }
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
            _ => NativePointerTranslation::NotPointer,
        }
    }

    fn input(
        &mut self,
        event: NativePointerEvent,
        desktop_position: Option<winit::dpi::PhysicalPosition<f64>>,
        routes: NativePointerRoutes,
        resolve_work_area: &mut impl FnMut(PhysicalPoint) -> Option<NativeWorkAreaBinding>,
    ) -> NativePointerTranslation {
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
        let (phase, should_clear) = match phase {
            TouchPhase::Started => match self.active_scroll {
                Some(sequence) => (NativeScrollPhase::Update { sequence, delta }, false),
                None => {
                    let Some(sequence) = self.allocate_scroll_sequence() else {
                        return NativePointerTranslation::Ignored;
                    };
                    (
                        NativeScrollPhase::Begin {
                            sequence,
                            delta: Some(delta),
                        },
                        false,
                    )
                }
            },
            TouchPhase::Moved => match self.active_scroll {
                Some(sequence) => (NativeScrollPhase::Update { sequence, delta }, false),
                None => (NativeScrollPhase::Discrete { delta }, false),
            },
            TouchPhase::Ended => match self.active_scroll {
                Some(sequence) => (
                    NativeScrollPhase::End {
                        sequence,
                        delta: Some(delta),
                    },
                    true,
                ),
                None if is_zero_delta(delta) => return NativePointerTranslation::Ignored,
                None => (NativeScrollPhase::Discrete { delta }, false),
            },
            TouchPhase::Cancelled => match self.active_scroll {
                Some(sequence) => (
                    NativeScrollPhase::Cancel {
                        sequence,
                        reason: NativeScrollCancelReason::PlatformCancelled,
                    },
                    true,
                ),
                None => return NativePointerTranslation::Ignored,
            },
        };

        let event = NativeScrollEvent::new(
            SCROLL_DEVICE_ID,
            phase,
            NativeScrollMomentum::Unknown,
            native_modifiers(facts),
        );
        if should_clear {
            self.active_scroll = None;
        }
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
        let sequence = NativeScrollSequenceId::new(next);
        self.active_scroll = Some(sequence);
        Some(sequence)
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

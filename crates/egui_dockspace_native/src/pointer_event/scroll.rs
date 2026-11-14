//! Scroll phases retain sequence identity independently of mouse buttons.

use dockspace::geometry::PhysicalPoint;
use dockspace::runtime::{
    NativePointerEvent, NativeScrollCancelReason, NativeScrollDelta, NativeScrollEvent,
    NativeScrollModifiers, NativeScrollMomentum, NativeScrollPhase, NativeScrollSequenceId,
    NativeWindowBinding, NativeWorkAreaBinding,
};
use winit::event::{MouseScrollDelta, TouchPhase, WindowEvent};

use crate::event::{NativePointerRoutes, NativeWindowEventRecord};

use super::device::{NativeInputDeviceKey, NativePointerDeviceEntry};
use super::{
    NativePointerTranslation, NativePointerTranslator, exact_delivery_binding, native_input,
};

#[derive(Debug, Clone, Copy, Default)]
pub(super) enum NativeScrollState {
    #[default]
    Idle,
    /// One winit phaseful sequence still maps to a live core sequence.
    Live {
        sequence: NativeScrollSequenceId,
        delivery: Option<NativeWindowBinding>,
    },
    /// Core closed the provider tail; ignore late phases until an explicit start.
    Retired,
}

impl NativePointerTranslator {
    pub(super) fn scroll(
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
}

fn translate_scroll_phase(
    state: NativeScrollState,
    phase: TouchPhase,
    delta: NativeScrollDelta,
    event_delivery: Option<NativeWindowBinding>,
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

fn allocate_scroll_sequence(next_scroll_sequence: &mut u64) -> Option<NativeScrollSequenceId> {
    let next = next_scroll_sequence.checked_add(1)?;
    *next_scroll_sequence = next;
    Some(NativeScrollSequenceId::new(next))
}

pub(super) fn delta_value(delta: &MouseScrollDelta) -> Option<NativeScrollDelta> {
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

pub(super) fn scroll_delta_is_finite(delta: NativeScrollDelta) -> bool {
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

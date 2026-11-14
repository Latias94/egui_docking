//! Button history normalizes raw callbacks before they become core input.

use std::collections::BTreeMap;

use dockspace::runtime::{NativePointerButton, NativePointerEvent, NativeWindowBinding};
use winit::event::PointerPressToken;

use crate::event::{NativePointerRouteSnapshot, NativePointerRoutes};

use super::exact_delivery_binding;

#[derive(Debug, Clone, Default)]
pub(super) struct NativeMouseState {
    buttons: BTreeMap<NativePointerButton, ButtonState>,
    capture: Option<NativeWindowBinding>,
}

#[derive(Debug, Clone)]
enum ButtonState {
    Active {
        origin: Option<NativeWindowBinding>,
        primary_press: Option<PointerPressToken>,
    },
    /// Release and cancellation both end the old button until a new press.
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MouseTransition {
    Ignore,
    Forward,
    CancelThenPress,
}

pub(super) fn mouse_press_transition<T: Eq>(
    active: Option<Option<&T>>,
    observed: Option<&T>,
) -> MouseTransition {
    match (active, observed) {
        (None, _) => MouseTransition::Forward,
        (Some(Some(previous)), Some(current)) if previous == current => MouseTransition::Ignore,
        (Some(_), _) => MouseTransition::CancelThenPress,
    }
}

impl NativeMouseState {
    pub(super) const fn button_slot_capacity_bytes() -> usize {
        size_of::<NativePointerButton>() + size_of::<ButtonState>()
    }

    pub(super) fn owned_button_capacity_bytes(&self) -> usize {
        self.buttons
            .len()
            .saturating_mul(Self::button_slot_capacity_bytes())
    }

    pub(super) fn is_idle(&self) -> bool {
        self.buttons
            .values()
            .all(|state| matches!(state, ButtonState::Ended))
            && self.capture.is_none()
    }

    pub(super) fn is_pressed(&self, button: NativePointerButton) -> bool {
        self.active_press(button).is_some()
    }

    pub(super) fn active_press(
        &self,
        button: NativePointerButton,
    ) -> Option<Option<&PointerPressToken>> {
        match self.buttons.get(&button) {
            Some(ButtonState::Active { primary_press, .. }) => Some(primary_press.as_ref()),
            Some(ButtonState::Ended) | None => None,
        }
    }

    pub(super) fn starts_primary_press(&self, observed: Option<&PointerPressToken>) -> bool {
        mouse_press_transition(self.active_press(NativePointerButton::Primary), observed)
            != MouseTransition::Ignore
    }

    /// A real mouse down may replace a sequence whose release never arrived.
    pub(super) fn press(
        &mut self,
        button: NativePointerButton,
        routes: NativePointerRoutes,
        observed: Option<&PointerPressToken>,
    ) -> MouseTransition {
        let transition = mouse_press_transition(self.active_press(button), observed);
        if transition == MouseTransition::Ignore {
            return transition;
        }
        if transition == MouseTransition::CancelThenPress {
            self.end_buttons();
        }
        self.observe_routes(routes);
        self.begin_button(button, routes);
        if button == NativePointerButton::Primary
            && let Some(ButtonState::Active { primary_press, .. }) = self.buttons.get_mut(&button)
        {
            *primary_press = observed.cloned();
        }
        transition
    }

    /// Contact starts have already passed their device-enrollment gate.
    pub(super) fn observe(
        &mut self,
        event: NativePointerEvent,
        routes: NativePointerRoutes,
    ) -> MouseTransition {
        if self.should_ignore_captured_move(event, routes)
            || matches!(
                event,
                NativePointerEvent::ButtonReleased(button)
                    | NativePointerEvent::ContactEnded(button)
                    if matches!(self.buttons.get(&button), Some(ButtonState::Ended))
            )
        {
            return MouseTransition::Ignore;
        }
        self.observe_routes(routes);
        match event {
            NativePointerEvent::ButtonPressed(button) => self.begin_button(button, routes),
            NativePointerEvent::ButtonReleased(button)
            | NativePointerEvent::ContactEnded(button) => {
                // No local press may mean a held button from the initial host
                // roster. Forward its first release and suppress only its tails.
                self.buttons.insert(button, ButtonState::Ended);
            }
            NativePointerEvent::StreamEnded | NativePointerEvent::StreamCancelled(_) => {
                self.end_buttons();
            }
            NativePointerEvent::CaptureChanged => {
                if let Some(ButtonState::Active { primary_press, .. }) =
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
        MouseTransition::Forward
    }

    fn begin_button(&mut self, button: NativePointerButton, routes: NativePointerRoutes) {
        let delivery = exact_delivery_binding(routes.delivery());
        match self.buttons.get_mut(&button) {
            Some(ButtonState::Active { origin, .. }) => {
                if origin.is_none() {
                    *origin = delivery;
                }
            }
            state => {
                let active = ButtonState::Active {
                    origin: delivery,
                    primary_press: None,
                };
                if let Some(state) = state {
                    *state = active;
                } else {
                    self.buttons.insert(button, active);
                }
            }
        }
    }

    fn end_buttons(&mut self) {
        for state in self.buttons.values_mut() {
            *state = ButtonState::Ended;
        }
        self.capture = None;
    }

    fn should_ignore_captured_move(
        &self,
        event: NativePointerEvent,
        routes: NativePointerRoutes,
    ) -> bool {
        // The hovered window can receive motion before the real capture owner.
        matches!(event, NativePointerEvent::Moved)
            && self
                .buttons
                .values()
                .any(|state| matches!(state, ButtonState::Active { .. }))
            && exact_delivery_binding(routes.capture())
                .is_some_and(|capture| exact_delivery_binding(routes.delivery()) != Some(capture))
    }

    pub(super) fn active_origin_mismatch(&self, routes: NativePointerRoutes) -> bool {
        self.buttons
            .get(&NativePointerButton::Primary)
            .is_some_and(|state| {
                matches!(state, ButtonState::Active { origin: Some(origin), .. }
                if exact_delivery_binding(routes.delivery()) != Some(*origin))
            })
    }

    fn observe_routes(&mut self, routes: NativePointerRoutes) {
        self.capture = match routes.capture() {
            NativePointerRouteSnapshot::Dock(binding) => Some(binding),
            NativePointerRouteSnapshot::None | NativePointerRouteSnapshot::Foreign => None,
            NativePointerRouteSnapshot::Unknown => self.capture,
        };
    }

    pub(super) fn retire_binding(&mut self, binding: NativeWindowBinding) -> bool {
        if !self.references_binding(binding) {
            return false;
        }
        self.end_buttons();
        true
    }

    pub(super) fn references_binding(&self, binding: NativeWindowBinding) -> bool {
        self.buttons.values().any(|state| {
            matches!(state, ButtonState::Active { origin: Some(origin), .. } if *origin == binding)
        }) || self.capture == Some(binding)
    }
}

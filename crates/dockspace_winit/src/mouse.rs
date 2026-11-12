use dockspace::geometry::GeometryError;
use dockspace::runtime::{
    SurfacePointerButton, SurfacePointerCancelReason, SurfacePointerEvent, SurfacePointerPosition,
};
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::keyboard::ModifiersState;

use crate::SurfaceCoordinates;

/// One mouse event with the latest client cursor position and modifier snapshot.
///
/// This sample proves neither receiver delivery nor pointer capture. Winit window
/// events provide no original OS timestamp, so this type supplies no timed metadata.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MouseSample {
    /// The core mouse edge, without routing or authority facts.
    pub event: SurfacePointerEvent,
    /// Latest observed client physical cursor position, if still known.
    ///
    /// Button events carry no position of their own. This is the preceding cursor
    /// sample, not an independently queried button-time position.
    pub position: Option<PhysicalPosition<f64>>,
    /// Most recent modifiers observed in the ordered window event stream.
    pub modifiers: ModifiersState,
}

impl MouseSample {
    /// Converts the retained cursor position, preserving an unknown position.
    ///
    /// The caller must use a mapping valid for the sampled client coordinates.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::NonFinite`] for non-finite input or arithmetic overflow.
    pub fn logical_position(
        self,
        coordinates: SurfaceCoordinates,
    ) -> Result<SurfacePointerPosition, GeometryError> {
        self.position
            .map_or(Ok(SurfacePointerPosition::Unknown), |position| {
                coordinates
                    .to_surface(position)
                    .map(SurfacePointerPosition::Known)
            })
    }
}

/// Tracks one window's mouse cursor and modifiers in event-delivery order.
///
/// Keep one instance per window and feed only that window's events. Mouse device
/// IDs are treated as one window cursor; touch and scroll events are ignored.
/// Losing focus actively cancels this host's stream. Leaving the client area
/// only forgets its cursor position and does not cancel an ongoing drag.
///
/// The host still owns pointer identity, capture, receiver routing, presentation
/// settlement and the decision to submit each returned sample to dockspace.
#[derive(Debug, Default)]
pub struct MouseInput {
    position: Option<PhysicalPosition<f64>>,
    modifiers: ModifiersState,
}

impl MouseInput {
    /// Updates mouse state and returns an edge for supported mouse/lifecycle events.
    ///
    /// Modifier changes and cursor departure update retained state without emitting
    /// a pointer edge. Keyboard, touch, wheel and unrelated window events return `None`.
    pub fn on_window_event(&mut self, event: &WindowEvent) -> Option<MouseSample> {
        let event = match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.position = Some(*position);
                SurfacePointerEvent::Moved
            }
            WindowEvent::MouseInput { state, button, .. } => match state {
                ElementState::Pressed => {
                    SurfacePointerEvent::ButtonPressed(map_mouse_button(*button))
                }
                ElementState::Released => {
                    SurfacePointerEvent::ButtonReleased(map_mouse_button(*button))
                }
            },
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
                return None;
            }
            WindowEvent::CursorLeft { .. } => {
                self.position = None;
                return None;
            }
            WindowEvent::Focused(false) => {
                self.position = None;
                self.modifiers = ModifiersState::empty();
                SurfacePointerEvent::StreamCancelled(
                    SurfacePointerCancelReason::ExplicitCancellation,
                )
            }
            WindowEvent::Destroyed => {
                self.position = None;
                self.modifiers = ModifiersState::empty();
                SurfacePointerEvent::StreamCancelled(SurfacePointerCancelReason::BindingRetired)
            }
            _ => return None,
        };
        Some(MouseSample {
            event,
            position: self.position,
            modifiers: self.modifiers,
        })
    }
}

/// Maps a winit mouse button into a core pointer button.
///
/// Back and Forward use `Other(4)` and `Other(5)` respectively. Explicit
/// `MouseButton::Other` values pass through unchanged.
#[must_use]
pub const fn map_mouse_button(button: MouseButton) -> SurfacePointerButton {
    match button {
        MouseButton::Left => SurfacePointerButton::Primary,
        MouseButton::Right => SurfacePointerButton::Secondary,
        MouseButton::Middle => SurfacePointerButton::Middle,
        MouseButton::Back => SurfacePointerButton::Other(4),
        MouseButton::Forward => SurfacePointerButton::Other(5),
        MouseButton::Other(button) => SurfacePointerButton::Other(button),
    }
}

//! Complete platform capability, window-inventory, and pointer observations.
//!
//! Adapters submit facts and execute effects. They never submit operating-system
//! handles or infer a docking target from window geometry.

use std::collections::BTreeSet;

use thiserror::Error;

use crate::geometry::{PhysicalPoint, PhysicalRect, ScaleFactor};
use crate::intent::{
    Authority, AuthorityUnavailableReason, PointerButton, PointerButtonState, PointerId,
};
use crate::viewport::WindowToken;

/// One independently degradable platform requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PlatformRequirement {
    NativeWindowLifecycle,
    AuthoritativeInventory,
    HoveredWindow,
    DesktopPointerPosition,
    AuthoritativeButtonState,
    GlobalWindowPlacement,
    WorkArea,
    PointerPassthrough,
    WindowFocus,
    CloseCancellation,
}

/// Provider-level reason one requirement is not available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlatformCapabilityReason {
    BackendUnsupported,
    NotReported,
    PermissionDenied,
    EnvironmentUnavailable,
}

/// Structured unavailable capability with the exact missing requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlatformCapabilityIssue {
    requirement: PlatformRequirement,
    reason: PlatformCapabilityReason,
}

impl PlatformCapabilityIssue {
    #[must_use]
    pub const fn new(requirement: PlatformRequirement, reason: PlatformCapabilityReason) -> Self {
        Self {
            requirement,
            reason,
        }
    }

    #[must_use]
    pub const fn requirement(self) -> PlatformRequirement {
        self.requirement
    }

    #[must_use]
    pub const fn reason(self) -> PlatformCapabilityReason {
        self.reason
    }
}

/// Authoritative capability state for one operation or primitive fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlatformCapability {
    Supported,
    Unsupported(PlatformCapabilityIssue),
    Unknown(PlatformCapabilityIssue),
}

impl PlatformCapability {
    #[must_use]
    pub const fn unsupported(
        requirement: PlatformRequirement,
        reason: PlatformCapabilityReason,
    ) -> Self {
        Self::Unsupported(PlatformCapabilityIssue::new(requirement, reason))
    }

    #[must_use]
    pub const fn unknown(
        requirement: PlatformRequirement,
        reason: PlatformCapabilityReason,
    ) -> Self {
        Self::Unknown(PlatformCapabilityIssue::new(requirement, reason))
    }

    #[must_use]
    pub const fn is_supported(self) -> bool {
        matches!(self, Self::Supported)
    }
}

/// Complete capability roster for one accepted platform snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformCapabilities {
    native_window_lifecycle: PlatformCapability,
    authoritative_inventory: PlatformCapability,
    hovered_window: PlatformCapability,
    desktop_pointer_position: PlatformCapability,
    authoritative_button_state: PlatformCapability,
    global_window_placement: PlatformCapability,
    work_area: PlatformCapability,
    pointer_passthrough: PlatformCapability,
    window_focus: PlatformCapability,
    close_cancellation: PlatformCapability,
}

macro_rules! capability_accessors {
    ($(($getter:ident, $setter:ident, $field:ident)),+ $(,)?) => {
        $(
            #[must_use]
            pub const fn $getter(&self) -> PlatformCapability {
                self.$field
            }

            pub fn $setter(&mut self, capability: PlatformCapability) {
                self.$field = capability;
            }
        )+
    };
}

impl PlatformCapabilities {
    capability_accessors!(
        (
            native_window_lifecycle,
            set_native_window_lifecycle,
            native_window_lifecycle
        ),
        (
            authoritative_inventory,
            set_authoritative_inventory,
            authoritative_inventory
        ),
        (hovered_window, set_hovered_window, hovered_window),
        (
            desktop_pointer_position,
            set_desktop_pointer_position,
            desktop_pointer_position
        ),
        (
            authoritative_button_state,
            set_authoritative_button_state,
            authoritative_button_state
        ),
        (
            global_window_placement,
            set_global_window_placement,
            global_window_placement
        ),
        (work_area, set_work_area, work_area),
        (
            pointer_passthrough,
            set_pointer_passthrough,
            pointer_passthrough
        ),
        (window_focus, set_window_focus, window_focus),
        (
            close_cancellation,
            set_close_cancellation,
            close_cancellation
        ),
    );

    /// Returns whether a native create can be observed and placed safely.
    #[must_use]
    pub fn native_tear_off(&self) -> PlatformCapability {
        combine_required([
            self.native_window_lifecycle,
            self.authoritative_inventory,
            self.global_window_placement,
            self.work_area,
        ])
    }

    /// Returns whether a pointer can be routed across native windows without inference.
    #[must_use]
    pub fn cross_surface_routing(&self) -> PlatformCapability {
        combine_required([
            self.authoritative_inventory,
            self.hovered_window,
            self.desktop_pointer_position,
            self.pointer_passthrough,
        ])
    }

    /// Returns whether the exact release button can be trusted.
    #[must_use]
    pub const fn authoritative_release(&self) -> PlatformCapability {
        self.authoritative_button_state
    }
}

impl Default for PlatformCapabilities {
    fn default() -> Self {
        Self {
            native_window_lifecycle: not_reported(PlatformRequirement::NativeWindowLifecycle),
            authoritative_inventory: not_reported(PlatformRequirement::AuthoritativeInventory),
            hovered_window: not_reported(PlatformRequirement::HoveredWindow),
            desktop_pointer_position: not_reported(PlatformRequirement::DesktopPointerPosition),
            authoritative_button_state: not_reported(PlatformRequirement::AuthoritativeButtonState),
            global_window_placement: not_reported(PlatformRequirement::GlobalWindowPlacement),
            work_area: not_reported(PlatformRequirement::WorkArea),
            pointer_passthrough: not_reported(PlatformRequirement::PointerPassthrough),
            window_focus: not_reported(PlatformRequirement::WindowFocus),
            close_cancellation: not_reported(PlatformRequirement::CloseCancellation),
        }
    }
}

const fn not_reported(requirement: PlatformRequirement) -> PlatformCapability {
    PlatformCapability::unknown(requirement, PlatformCapabilityReason::NotReported)
}

fn combine_required<const N: usize>(capabilities: [PlatformCapability; N]) -> PlatformCapability {
    let mut unknown = None;
    for capability in capabilities {
        match capability {
            PlatformCapability::Unsupported(issue) => {
                return PlatformCapability::Unsupported(issue);
            }
            PlatformCapability::Unknown(issue) if unknown.is_none() => unknown = Some(issue),
            PlatformCapability::Supported | PlatformCapability::Unknown(_) => {}
        }
    }
    unknown.map_or(PlatformCapability::Supported, PlatformCapability::Unknown)
}

/// Observed input behavior of one native window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowInputState {
    ReceivesInput,
    PassThrough,
    Minimized,
}

/// Complete provider facts for one opaque native-window token.
#[derive(Debug, Clone, PartialEq)]
pub struct ObservedWindow {
    token: WindowToken,
    content_bounds: Authority<PhysicalRect>,
    outer_bounds: Authority<PhysicalRect>,
    scale_factor: Authority<ScaleFactor>,
    work_area: Authority<Option<PhysicalRect>>,
    input_state: Authority<WindowInputState>,
    focused: Authority<bool>,
    close_requested: Authority<bool>,
}

impl ObservedWindow {
    /// Creates a token observation whose individual facts are explicitly unavailable.
    #[must_use]
    pub fn new(token: WindowToken) -> Self {
        Self {
            token,
            content_bounds: unavailable(),
            outer_bounds: unavailable(),
            scale_factor: unavailable(),
            work_area: unavailable(),
            input_state: unavailable(),
            focused: unavailable(),
            close_requested: unavailable(),
        }
    }

    #[must_use]
    pub const fn token(&self) -> WindowToken {
        self.token
    }

    #[must_use]
    pub const fn content_bounds(&self) -> &Authority<PhysicalRect> {
        &self.content_bounds
    }

    #[must_use]
    pub const fn outer_bounds(&self) -> &Authority<PhysicalRect> {
        &self.outer_bounds
    }

    #[must_use]
    pub const fn scale_factor(&self) -> &Authority<ScaleFactor> {
        &self.scale_factor
    }

    #[must_use]
    pub const fn work_area(&self) -> &Authority<Option<PhysicalRect>> {
        &self.work_area
    }

    #[must_use]
    pub const fn input_state(&self) -> &Authority<WindowInputState> {
        &self.input_state
    }

    #[must_use]
    pub const fn focused(&self) -> &Authority<bool> {
        &self.focused
    }

    #[must_use]
    pub const fn close_requested(&self) -> &Authority<bool> {
        &self.close_requested
    }

    #[must_use]
    pub fn with_content_bounds(mut self, value: Authority<PhysicalRect>) -> Self {
        self.content_bounds = value;
        self
    }

    #[must_use]
    pub fn with_outer_bounds(mut self, value: Authority<PhysicalRect>) -> Self {
        self.outer_bounds = value;
        self
    }

    #[must_use]
    pub fn with_scale_factor(mut self, value: Authority<ScaleFactor>) -> Self {
        self.scale_factor = value;
        self
    }

    #[must_use]
    pub fn with_work_area(mut self, value: Authority<Option<PhysicalRect>>) -> Self {
        self.work_area = value;
        self
    }

    #[must_use]
    pub fn with_input_state(mut self, value: Authority<WindowInputState>) -> Self {
        self.input_state = value;
        self
    }

    #[must_use]
    pub fn with_focused(mut self, value: Authority<bool>) -> Self {
        self.focused = value;
        self
    }

    #[must_use]
    pub fn with_close_requested(mut self, value: Authority<bool>) -> Self {
        self.close_requested = value;
        self
    }
}

fn unavailable<T>() -> Authority<T> {
    Authority::Unknown(AuthorityUnavailableReason::NotReported)
}

/// Authoritative platform classification of the window under one pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerWindow {
    Dock(WindowToken),
    Foreign,
    None,
}

/// One exact button state in a complete pointer observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ButtonObservation {
    button: PointerButton,
    state: PointerButtonState,
}

impl ButtonObservation {
    #[must_use]
    pub const fn new(button: PointerButton, state: PointerButtonState) -> Self {
        Self { button, state }
    }

    #[must_use]
    pub const fn button(self) -> PointerButton {
        self.button
    }

    #[must_use]
    pub const fn state(self) -> PointerButtonState {
        self.state
    }
}

/// Complete authoritative facts for one stable pointer identity.
#[derive(Debug, Clone, PartialEq)]
pub struct PointerObservation {
    pointer: PointerId,
    hovered: Authority<PointerWindow>,
    desktop_position: Authority<PhysicalPoint>,
    button_states: Authority<Vec<ButtonObservation>>,
}

impl PointerObservation {
    /// Creates and canonicalizes a pointer observation.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformSnapshotError::DuplicateButton`] when a known button roster repeats a
    /// button identity.
    pub fn new(
        pointer: PointerId,
        hovered: Authority<PointerWindow>,
        desktop_position: Authority<PhysicalPoint>,
        mut button_states: Authority<Vec<ButtonObservation>>,
    ) -> Result<Self, PlatformSnapshotError> {
        if let Authority::Known(states) = &mut button_states {
            states.sort_by_key(|state| state.button);
            for pair in states.windows(2) {
                if pair[0].button == pair[1].button {
                    return Err(PlatformSnapshotError::DuplicateButton {
                        pointer,
                        button: pair[0].button,
                    });
                }
            }
        }
        Ok(Self {
            pointer,
            hovered,
            desktop_position,
            button_states,
        })
    }

    #[must_use]
    pub const fn pointer(&self) -> PointerId {
        self.pointer
    }

    #[must_use]
    pub const fn hovered(&self) -> &Authority<PointerWindow> {
        &self.hovered
    }

    #[must_use]
    pub const fn desktop_position(&self) -> &Authority<PhysicalPoint> {
        &self.desktop_position
    }

    #[must_use]
    pub const fn button_states(&self) -> &Authority<Vec<ButtonObservation>> {
        &self.button_states
    }

    /// Returns a known exact state or an explicit unavailable observation.
    #[must_use]
    pub fn button_state(&self, button: PointerButton) -> Authority<PointerButtonState> {
        match &self.button_states {
            Authority::Unknown(reason) => Authority::Unknown(*reason),
            Authority::Known(states) => states
                .iter()
                .find(|state| state.button == button)
                .map_or_else(unavailable, |state| Authority::Known(state.state)),
        }
    }
}

/// One complete frame-before-paint platform fact snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct PlatformSnapshot {
    capabilities: PlatformCapabilities,
    windows: Vec<ObservedWindow>,
    pointers: Vec<PointerObservation>,
}

impl PlatformSnapshot {
    /// Creates a canonical complete snapshot and rejects ambiguous identities.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformSnapshotError`] for duplicate window or pointer identities.
    pub fn new(
        capabilities: PlatformCapabilities,
        mut windows: Vec<ObservedWindow>,
        mut pointers: Vec<PointerObservation>,
    ) -> Result<Self, PlatformSnapshotError> {
        windows.sort_by_key(ObservedWindow::token);
        reject_duplicate_windows(&windows)?;
        pointers.sort_by_key(PointerObservation::pointer);
        reject_duplicate_pointers(&pointers)?;
        Ok(Self {
            capabilities,
            windows,
            pointers,
        })
    }

    #[must_use]
    pub const fn capabilities(&self) -> &PlatformCapabilities {
        &self.capabilities
    }

    #[must_use]
    pub fn windows(&self) -> &[ObservedWindow] {
        &self.windows
    }

    #[must_use]
    pub fn pointers(&self) -> &[PointerObservation] {
        &self.pointers
    }
}

fn reject_duplicate_windows(windows: &[ObservedWindow]) -> Result<(), PlatformSnapshotError> {
    let mut tokens = BTreeSet::new();
    for window in windows {
        if !tokens.insert(window.token) {
            return Err(PlatformSnapshotError::DuplicateWindow {
                token: window.token,
            });
        }
    }
    Ok(())
}

fn reject_duplicate_pointers(pointers: &[PointerObservation]) -> Result<(), PlatformSnapshotError> {
    let mut identities = BTreeSet::new();
    for pointer in pointers {
        if !identities.insert(pointer.pointer) {
            return Err(PlatformSnapshotError::DuplicatePointer {
                pointer: pointer.pointer,
            });
        }
    }
    Ok(())
}

/// Malformed complete platform snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PlatformSnapshotError {
    #[error("platform snapshot repeats window token {token:?}")]
    DuplicateWindow { token: WindowToken },
    #[error("platform snapshot repeats pointer {pointer:?}")]
    DuplicatePointer { pointer: PointerId },
    #[error("pointer {pointer:?} repeats button {button:?}")]
    DuplicateButton {
        pointer: PointerId,
        button: PointerButton,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_is_stronger_than_unknown_for_a_required_operation() {
        let mut capabilities = PlatformCapabilities::default();
        capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
        capabilities.set_authoritative_inventory(PlatformCapability::unsupported(
            PlatformRequirement::AuthoritativeInventory,
            PlatformCapabilityReason::BackendUnsupported,
        ));

        assert!(matches!(
            capabilities.native_tear_off(),
            PlatformCapability::Unsupported(issue)
                if issue.requirement() == PlatformRequirement::AuthoritativeInventory
        ));
    }

    #[test]
    fn a_missing_button_is_unknown_and_never_inferred_released() {
        let observation = PointerObservation::new(
            PointerId::new(1),
            Authority::Known(PointerWindow::None),
            unavailable(),
            Authority::Known(Vec::new()),
        )
        .expect("empty button roster is unambiguous");

        assert_eq!(
            observation.button_state(PointerButton::Primary),
            Authority::Unknown(AuthorityUnavailableReason::NotReported)
        );
    }

    #[test]
    fn snapshot_rejects_duplicate_window_and_pointer_identities() {
        let window = ObservedWindow::new(WindowToken::new(7));
        assert!(matches!(
            PlatformSnapshot::new(
                PlatformCapabilities::default(),
                vec![window.clone(), window],
                Vec::new(),
            ),
            Err(PlatformSnapshotError::DuplicateWindow { .. })
        ));

        let pointer = PointerObservation::new(
            PointerId::new(2),
            unavailable(),
            unavailable(),
            unavailable(),
        )
        .expect("unknown button facts are unambiguous");
        assert!(matches!(
            PlatformSnapshot::new(
                PlatformCapabilities::default(),
                Vec::new(),
                vec![pointer.clone(), pointer],
            ),
            Err(PlatformSnapshotError::DuplicatePointer { .. })
        ));
    }
}

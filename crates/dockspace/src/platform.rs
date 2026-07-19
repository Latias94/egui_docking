//! Complete platform capability, window-inventory, and pointer observations.
//!
//! Adapters submit facts and execute effects. They never submit operating-system
//! handles or infer a docking target from window geometry.

use std::collections::BTreeSet;

use thiserror::Error;

use crate::effect::EffectId;
use crate::geometry::{PhysicalPoint, PhysicalRect, ScaleFactor};
use crate::intent::{
    Authority, AuthorityUnavailableReason, PointerButton, PointerButtonState, PointerId,
};
use crate::viewport::{InputObservationGeneration, ViewportBinding, WindowToken, WorkAreaToken};
use crate::viewport_focus::FocusObservationEnvelope;

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
    PointerHitTestObservation,
    PointerHitTestControl,
    GlobalFocusObservation,
    WindowActivationControl,
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
    pointer_hit_test_observation: PlatformCapability,
    pointer_hit_test_control: PlatformCapability,
    global_focus_observation: PlatformCapability,
    window_activation_control: PlatformCapability,
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
            pointer_hit_test_observation,
            set_pointer_hit_test_observation,
            pointer_hit_test_observation
        ),
        (
            pointer_hit_test_control,
            set_pointer_hit_test_control,
            pointer_hit_test_control
        ),
        (
            global_focus_observation,
            set_global_focus_observation,
            global_focus_observation
        ),
        (
            window_activation_control,
            set_window_activation_control,
            window_activation_control
        ),
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
            self.pointer_hit_test_observation,
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
            pointer_hit_test_observation: not_reported(
                PlatformRequirement::PointerHitTestObservation,
            ),
            pointer_hit_test_control: not_reported(PlatformRequirement::PointerHitTestControl),
            global_focus_observation: not_reported(PlatformRequirement::GlobalFocusObservation),
            window_activation_control: not_reported(PlatformRequirement::WindowActivationControl),
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
}

/// Provider authority over the exact effect reflected by an input observation.
///
/// `Known(Some(id))` means that the pointer-input property effect identified by
/// `id` was applied and that the provider serialized every earlier effect for
/// the same native-window token and property before this observation. That
/// serialization obligation crosses binding incarnation changes, so a restore
/// on a rebound binding orders any late enable for the previous incarnation.
/// This acknowledgement is stronger than dispatch success or merely sampling
/// after dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEffectAcknowledgement {
    Known(Option<EffectId>),
    Unknown(AuthorityUnavailableReason),
}

impl InputEffectAcknowledgement {
    #[must_use]
    pub const fn known(effect: Option<EffectId>) -> Self {
        Self::Known(effect)
    }

    #[must_use]
    pub const fn unknown(reason: AuthorityUnavailableReason) -> Self {
        Self::Unknown(reason)
    }

    #[must_use]
    pub fn acknowledges(self, effect: EffectId) -> bool {
        matches!(self, Self::Known(Some(actual)) if actual == effect)
    }
}

/// One provider-captured envelope for a window's pointer-input property.
///
/// The binding prevents a recycled token or a previous workspace incarnation
/// from authorizing the current window. The generation is owned by the
/// provider per native window and advances when that provider captures a fresh
/// property value; core receipt order is deliberately not a substitute. Both
/// the state and effect acknowledgement live inside this generated envelope,
/// so an unavailable state is a versioned tombstone rather than an unsequenced
/// absence. An acknowledged effect is exact to this property observation,
/// while `Known(None)` denotes a baseline observation and `Unknown` cannot
/// settle an effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowInputObservation {
    binding: ViewportBinding,
    generation: InputObservationGeneration,
    state: Authority<WindowInputState>,
    acknowledged_effect: InputEffectAcknowledgement,
}

impl WindowInputObservation {
    #[must_use]
    pub const fn new(
        binding: ViewportBinding,
        generation: InputObservationGeneration,
        state: Authority<WindowInputState>,
        acknowledged_effect: InputEffectAcknowledgement,
    ) -> Self {
        Self {
            binding,
            generation,
            state,
            acknowledged_effect,
        }
    }

    #[must_use]
    pub const fn binding(self) -> ViewportBinding {
        self.binding
    }

    #[must_use]
    pub const fn generation(self) -> InputObservationGeneration {
        self.generation
    }

    #[must_use]
    pub const fn state(self) -> Authority<WindowInputState> {
        self.state
    }

    #[must_use]
    pub const fn known_state(self) -> Option<WindowInputState> {
        match self.state {
            Authority::Known(state) => Some(state),
            Authority::Unknown(_) => None,
        }
    }

    #[must_use]
    pub const fn acknowledged_effect(self) -> InputEffectAcknowledgement {
        self.acknowledged_effect
    }

    #[must_use]
    pub fn acknowledges(self, effect: EffectId) -> bool {
        self.acknowledged_effect.acknowledges(effect)
    }
}

/// Last authoritative member of one provider-owned per-incarnation stream.
///
/// A versioned unknown state advances the stream as a tombstone. Missing,
/// wrong-incarnation, or same-generation conflicting envelopes revoke current
/// authority. After an unversioned gap, a versioned tombstone is required
/// before known state can become authoritative again. Older generations and
/// exact duplicates never advance state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct WindowInputObservationStream {
    current: Option<WindowInputObservation>,
    last_generation: Option<InputObservationGeneration>,
    last_envelope: Option<WindowInputObservation>,
    requires_tombstone: bool,
}

impl WindowInputObservationStream {
    pub(crate) fn observe(
        &mut self,
        binding: ViewportBinding,
        observation: Option<WindowInputObservation>,
    ) {
        let Some(observation) = observation else {
            self.current = None;
            self.requires_tombstone |= self.last_generation.is_some();
            return;
        };
        if observation.binding() != binding {
            self.current = None;
            self.requires_tombstone |= self.last_generation.is_some();
            return;
        }
        match self.last_generation {
            Some(generation) if observation.generation() < generation => return,
            Some(generation) if observation.generation() == generation => {
                if self.last_envelope != Some(observation) {
                    self.current = None;
                    self.requires_tombstone = true;
                }
                return;
            }
            Some(_) | None => {}
        }
        self.last_generation = Some(observation.generation());
        self.last_envelope = Some(observation);
        if self.requires_tombstone && observation.known_state().is_some() {
            self.current = None;
            return;
        }
        self.current = observation.known_state().map(|_| observation);
        self.requires_tombstone = false;
    }

    pub(crate) const fn current(self) -> Option<WindowInputObservation> {
        self.current
    }

    pub(crate) const fn generation_watermark(self) -> Option<InputObservationGeneration> {
        self.last_generation
    }
}

/// Independently observed presentation state of one native window.
///
/// This fact is deliberately separate from [`WindowInputState`]: a minimized
/// window may retain its input mode while still being ineligible for pointer
/// routing, placement, or focus-driven docking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowPresentationState {
    Visible,
    Hidden,
    Minimized,
}

/// Complete provider facts for one opaque native-window token.
#[derive(Debug, Clone, PartialEq)]
pub struct ObservedWindow {
    token: WindowToken,
    content_bounds: Authority<PhysicalRect>,
    outer_bounds: Authority<PhysicalRect>,
    scale_factor: Authority<ScaleFactor>,
    input_state: Authority<WindowInputState>,
    input_observation: Option<WindowInputObservation>,
    presentation: Authority<WindowPresentationState>,
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
            input_state: unavailable(),
            input_observation: None,
            presentation: unavailable(),
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
    pub const fn input_state(&self) -> &Authority<WindowInputState> {
        &self.input_state
    }

    #[must_use]
    pub const fn input_observation(&self) -> Option<WindowInputObservation> {
        self.input_observation
    }

    #[must_use]
    pub const fn presentation(&self) -> &Authority<WindowPresentationState> {
        &self.presentation
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
    pub fn with_input_state(mut self, value: Authority<WindowInputState>) -> Self {
        self.input_state = value;
        self
    }

    #[must_use]
    pub fn with_input_observation(mut self, value: WindowInputObservation) -> Self {
        if let Some(state) = value.known_state() {
            self.input_state = Authority::Known(state);
        }
        self.input_observation = Some(value);
        self
    }

    #[must_use]
    pub fn with_presentation(mut self, value: Authority<WindowPresentationState>) -> Self {
        self.presentation = value;
        self
    }

    #[must_use]
    pub fn with_close_requested(mut self, value: Authority<bool>) -> Self {
        self.close_requested = value;
        self
    }
}

/// One explicitly selectable monitor work area in desktop physical coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ObservedWorkArea {
    token: WorkAreaToken,
    bounds: PhysicalRect,
    scale_factor: ScaleFactor,
}

impl ObservedWorkArea {
    #[must_use]
    pub const fn new(
        token: WorkAreaToken,
        bounds: PhysicalRect,
        scale_factor: ScaleFactor,
    ) -> Self {
        Self {
            token,
            bounds,
            scale_factor,
        }
    }

    #[must_use]
    pub const fn token(self) -> WorkAreaToken {
        self.token
    }

    #[must_use]
    pub const fn bounds(self) -> PhysicalRect {
        self.bounds
    }

    #[must_use]
    pub const fn scale_factor(self) -> ScaleFactor {
        self.scale_factor
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
    focus: FocusObservationEnvelope,
    windows: Vec<ObservedWindow>,
    pointers: Vec<PointerObservation>,
    work_areas: Vec<ObservedWorkArea>,
}

impl PlatformSnapshot {
    /// Creates a canonical complete snapshot and rejects ambiguous identities.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformSnapshotError`] for duplicate identities or a work-area roster that
    /// contradicts the declared capability.
    pub fn new(
        capabilities: PlatformCapabilities,
        focus: FocusObservationEnvelope,
        mut windows: Vec<ObservedWindow>,
        mut pointers: Vec<PointerObservation>,
        mut work_areas: Vec<ObservedWorkArea>,
    ) -> Result<Self, PlatformSnapshotError> {
        windows.sort_by_key(ObservedWindow::token);
        reject_duplicate_windows(&windows)?;
        validate_input_observations(&windows)?;
        pointers.sort_by_key(PointerObservation::pointer);
        reject_duplicate_pointers(&pointers)?;
        work_areas.sort_by_key(|work_area| work_area.token);
        validate_work_areas(&work_areas)?;
        if capabilities.work_area().is_supported() && work_areas.is_empty() {
            return Err(PlatformSnapshotError::MissingWorkAreaRoster);
        }
        if !capabilities.work_area().is_supported() && !work_areas.is_empty() {
            return Err(PlatformSnapshotError::WorkAreaRosterWithoutCapability);
        }
        Ok(Self {
            capabilities,
            focus,
            windows,
            pointers,
            work_areas,
        })
    }

    #[must_use]
    pub const fn capabilities(&self) -> &PlatformCapabilities {
        &self.capabilities
    }

    /// Returns the provider-generated single global native-focus observation.
    #[must_use]
    pub const fn focus(&self) -> FocusObservationEnvelope {
        self.focus
    }

    #[must_use]
    pub fn windows(&self) -> &[ObservedWindow] {
        &self.windows
    }

    #[must_use]
    pub fn pointers(&self) -> &[PointerObservation] {
        &self.pointers
    }

    #[must_use]
    pub fn work_areas(&self) -> &[ObservedWorkArea] {
        &self.work_areas
    }
}

fn validate_work_areas(work_areas: &[ObservedWorkArea]) -> Result<(), PlatformSnapshotError> {
    let mut tokens = BTreeSet::new();
    for work_area in work_areas {
        if !tokens.insert(work_area.token) {
            return Err(PlatformSnapshotError::DuplicateWorkArea {
                token: work_area.token,
            });
        }
        if work_area.bounds.width() == 0.0 || work_area.bounds.height() == 0.0 {
            return Err(PlatformSnapshotError::EmptyWorkArea {
                token: work_area.token,
            });
        }
    }
    Ok(())
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

fn validate_input_observations(windows: &[ObservedWindow]) -> Result<(), PlatformSnapshotError> {
    for window in windows {
        let Some(observation) = window.input_observation() else {
            continue;
        };
        if observation.binding().token() != window.token() {
            return Err(PlatformSnapshotError::InputObservationTokenMismatch {
                window: window.token(),
                observation: observation.binding().token(),
            });
        }
        if let Some(state) = observation.known_state()
            && window.input_state() != &Authority::Known(state)
        {
            return Err(PlatformSnapshotError::InputObservationStateMismatch {
                token: window.token(),
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
    #[error("window {window:?} carries an input observation for a different token {observation:?}")]
    InputObservationTokenMismatch {
        window: WindowToken,
        observation: WindowToken,
    },
    #[error("window {token:?} carries conflicting input state and causal observation")]
    InputObservationStateMismatch { token: WindowToken },
    #[error("platform snapshot repeats pointer {pointer:?}")]
    DuplicatePointer { pointer: PointerId },
    #[error("platform snapshot repeats work-area token {token:?}")]
    DuplicateWorkArea { token: WorkAreaToken },
    #[error("platform snapshot contains an empty work area: {token:?}")]
    EmptyWorkArea { token: WorkAreaToken },
    #[error("supported work-area capability requires a non-empty complete roster")]
    MissingWorkAreaRoster,
    #[error("a work-area roster was supplied without supported work-area capability")]
    WorkAreaRosterWithoutCapability,
    #[error("pointer {pointer:?} repeats button {button:?}")]
    DuplicateButton {
        pointer: PointerId,
        button: PointerButton,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport_focus::{FocusObservationGeneration, unknown_focus_observation};

    fn unknown_focus(generation: u64) -> FocusObservationEnvelope {
        unknown_focus_observation(
            FocusObservationGeneration::new(generation),
            AuthorityUnavailableReason::NotReported,
        )
    }

    fn binding(incarnation: u64) -> ViewportBinding {
        ViewportBinding::new(
            crate::ids::WorkspaceEpoch::new(1),
            crate::ids::SurfaceId::new(2),
            WindowToken::new(3),
            crate::viewport::WindowIncarnation::new(incarnation),
        )
    }

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
                unknown_focus(1),
                vec![window.clone(), window],
                Vec::new(),
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
                unknown_focus(2),
                Vec::new(),
                vec![pointer.clone(), pointer],
                Vec::new(),
            ),
            Err(PlatformSnapshotError::DuplicatePointer { .. })
        ));
    }

    #[test]
    fn snapshot_requires_a_complete_unambiguous_work_area_roster() {
        let bounds = PhysicalRect::new(-1920.0, -200.0, 1920.0, 1080.0)
            .expect("test work area must be valid");
        let scale = ScaleFactor::new(1.0).expect("test scale must be valid");
        let work_area = ObservedWorkArea::new(WorkAreaToken::new(3), bounds, scale);
        let mut supported = PlatformCapabilities::default();
        supported.set_work_area(PlatformCapability::Supported);

        assert_eq!(
            PlatformSnapshot::new(
                supported.clone(),
                unknown_focus(1),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ),
            Err(PlatformSnapshotError::MissingWorkAreaRoster)
        );
        assert_eq!(
            PlatformSnapshot::new(
                PlatformCapabilities::default(),
                unknown_focus(2),
                Vec::new(),
                Vec::new(),
                vec![work_area],
            ),
            Err(PlatformSnapshotError::WorkAreaRosterWithoutCapability)
        );
        assert!(matches!(
            PlatformSnapshot::new(
                supported,
                unknown_focus(3),
                Vec::new(),
                Vec::new(),
                vec![work_area, work_area],
            ),
            Err(PlatformSnapshotError::DuplicateWorkArea { .. })
        ));
        let empty = PhysicalRect::new(0.0, 0.0, 0.0, 1080.0)
            .expect("zero-width geometry remains representable outside work-area rosters");
        assert_eq!(
            PlatformSnapshot::new(
                {
                    let mut capabilities = PlatformCapabilities::default();
                    capabilities.set_work_area(PlatformCapability::Supported);
                    capabilities
                },
                unknown_focus(4),
                Vec::new(),
                Vec::new(),
                vec![ObservedWorkArea::new(WorkAreaToken::new(4), empty, scale)],
            ),
            Err(PlatformSnapshotError::EmptyWorkArea {
                token: WorkAreaToken::new(4),
            })
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn input_observation_stream_rejects_stale_duplicate_and_wrong_binding_facts() {
        let current_binding = binding(1);
        let mut stream = WindowInputObservationStream::default();
        let observation = |binding, generation, state| {
            WindowInputObservation::new(
                binding,
                InputObservationGeneration::new(generation),
                Authority::Known(state),
                InputEffectAcknowledgement::known(None),
            )
        };
        let tombstone = |binding, generation| {
            WindowInputObservation::new(
                binding,
                InputObservationGeneration::new(generation),
                Authority::Unknown(AuthorityUnavailableReason::NotReported),
                InputEffectAcknowledgement::unknown(AuthorityUnavailableReason::NotReported),
            )
        };

        stream.observe(
            current_binding,
            Some(observation(
                current_binding,
                4,
                WindowInputState::ReceivesInput,
            )),
        );
        stream.observe(
            current_binding,
            Some(observation(
                current_binding,
                3,
                WindowInputState::PassThrough,
            )),
        );
        assert_eq!(
            stream
                .current()
                .and_then(WindowInputObservation::known_state),
            Some(WindowInputState::ReceivesInput)
        );

        stream.observe(current_binding, None);
        assert_eq!(stream.current(), None);
        assert_eq!(
            stream.generation_watermark(),
            Some(InputObservationGeneration::new(4))
        );
        stream.observe(
            current_binding,
            Some(observation(
                current_binding,
                4,
                WindowInputState::ReceivesInput,
            )),
        );
        assert_eq!(stream.current(), None);
        stream.observe(
            current_binding,
            Some(observation(binding(2), 5, WindowInputState::PassThrough)),
        );
        assert_eq!(stream.current(), None);
        stream.observe(
            current_binding,
            Some(observation(
                current_binding,
                6,
                WindowInputState::PassThrough,
            )),
        );
        assert_eq!(stream.current(), None);
        assert_eq!(
            stream.generation_watermark(),
            Some(InputObservationGeneration::new(6))
        );
        stream.observe(current_binding, Some(tombstone(current_binding, 5)));
        assert_eq!(stream.current(), None);
        assert_eq!(
            stream.generation_watermark(),
            Some(InputObservationGeneration::new(6))
        );
        stream.observe(current_binding, Some(tombstone(current_binding, 7)));
        assert_eq!(stream.current(), None);
        stream.observe(
            current_binding,
            Some(observation(
                current_binding,
                8,
                WindowInputState::PassThrough,
            )),
        );
        assert_eq!(
            stream
                .current()
                .and_then(WindowInputObservation::known_state),
            Some(WindowInputState::PassThrough)
        );

        stream.observe(
            current_binding,
            Some(observation(
                current_binding,
                8,
                WindowInputState::ReceivesInput,
            )),
        );
        assert_eq!(stream.current(), None);
        stream.observe(
            current_binding,
            Some(observation(
                current_binding,
                9,
                WindowInputState::PassThrough,
            )),
        );
        assert_eq!(stream.current(), None);
        stream.observe(current_binding, Some(tombstone(current_binding, 10)));
        stream.observe(
            current_binding,
            Some(observation(
                current_binding,
                11,
                WindowInputState::ReceivesInput,
            )),
        );
        assert_eq!(
            stream
                .current()
                .and_then(WindowInputObservation::known_state),
            Some(WindowInputState::ReceivesInput)
        );
    }

    #[test]
    fn snapshot_rejects_conflicting_causal_input_facts() {
        let binding = binding(1);
        let window = ObservedWindow::new(binding.token())
            .with_input_observation(WindowInputObservation::new(
                binding,
                InputObservationGeneration::new(1),
                Authority::Known(WindowInputState::PassThrough),
                InputEffectAcknowledgement::known(None),
            ))
            .with_input_state(Authority::Known(WindowInputState::ReceivesInput));
        assert_eq!(
            PlatformSnapshot::new(
                PlatformCapabilities::default(),
                unknown_focus(1),
                vec![window],
                Vec::new(),
                Vec::new(),
            ),
            Err(PlatformSnapshotError::InputObservationStateMismatch {
                token: binding.token(),
            })
        );
    }
}

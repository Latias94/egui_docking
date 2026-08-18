//! Complete platform capability and native-window observations.
//!
//! Adapters submit facts and execute effects. They never submit operating-system
//! handles or infer a docking target from window geometry.

use std::collections::BTreeSet;

use thiserror::Error;

use crate::effect::EffectId;
use crate::geometry::{PhysicalRect, ScaleFactor};
use crate::intent::{Authority, AuthorityUnavailableReason};
use crate::viewport::{
    CapabilityObservationGeneration, CloseObservationGeneration, CoordinateObservationGeneration,
    InputObservationGeneration, InventoryGeneration, InventoryObservationGeneration,
    PlatformSnapshotGeneration, PresentationObservationGeneration, ViewportBinding, WindowToken,
    WorkAreaObservationGeneration, WorkAreaToken,
};
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

    /// Returns whether a native window can be created at an explicit desktop rectangle.
    #[must_use]
    pub fn native_exact_placement_create(&self) -> PlatformCapability {
        combine_required([
            self.native_window_lifecycle,
            self.authoritative_inventory,
            self.global_window_placement,
        ])
    }

    /// Returns whether an outside-all pointer drop can create a native window safely.
    #[must_use]
    pub fn native_outside_all_tear_off(&self) -> PlatformCapability {
        self.physical_native_drag()
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

    /// Returns whether one physical drag can be routed and released across native windows.
    ///
    /// Placement support is intentionally absent: existing-window routing needs exact hover,
    /// desktop position, button/capture authority, rendered hit observations, and controllable
    /// pointer pass-through. Being able to position a window proves none of those interaction
    /// facts.
    #[must_use]
    pub fn physical_cross_surface_drag(&self) -> PlatformCapability {
        combine_required([
            self.cross_surface_routing(),
            self.authoritative_release(),
            self.pointer_hit_test_control,
        ])
    }

    /// Returns whether a physical outside-all drag can create a native child safely.
    ///
    /// This composes the complete cross-surface interaction contract with the independent
    /// native lifecycle, exact placement, and work-area contract. Programmatic native creation
    /// continues to use [`Self::native_exact_placement_create`] and does not require pointer
    /// capabilities.
    #[must_use]
    pub fn physical_native_drag(&self) -> PlatformCapability {
        combine_required([
            self.physical_cross_surface_drag(),
            self.native_window_lifecycle,
            self.global_window_placement,
            self.work_area,
        ])
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

/// One causally versioned complete platform-capability roster observation.
///
/// An unknown roster is an explicit versioned tombstone. Provider omission is
/// not treated as an authoritative capability fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityRosterObservation {
    generation: CapabilityObservationGeneration,
    roster: Authority<PlatformCapabilities>,
}

impl CapabilityRosterObservation {
    /// Creates one complete provider observation.
    #[must_use]
    pub const fn new(
        generation: CapabilityObservationGeneration,
        roster: Authority<PlatformCapabilities>,
    ) -> Self {
        Self { generation, roster }
    }

    /// Creates an explicit versioned unavailable roster tombstone.
    #[must_use]
    pub const fn unknown(
        generation: CapabilityObservationGeneration,
        reason: AuthorityUnavailableReason,
    ) -> Self {
        Self::new(generation, Authority::Unknown(reason))
    }

    #[must_use]
    pub const fn generation(&self) -> CapabilityObservationGeneration {
        self.generation
    }

    #[must_use]
    pub const fn roster(&self) -> &Authority<PlatformCapabilities> {
        &self.roster
    }

    #[must_use]
    pub const fn known_roster(&self) -> Option<&PlatformCapabilities> {
        self.roster.known()
    }

    #[must_use]
    pub const fn has_authority(&self) -> bool {
        matches!(self.roster, Authority::Known(_))
    }
}

/// Last authoritative member of the provider-owned capability-roster stream.
///
/// Missing, skipped-generation, or same-generation conflicting envelopes revoke current
/// authority and require an explicit versioned tombstone before a later known roster may
/// become authoritative again. Older generations and exact duplicates are inert.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CapabilityRosterObservationStream {
    current: Option<CapabilityRosterObservation>,
    last_generation: Option<CapabilityObservationGeneration>,
    last_envelope: Option<CapabilityRosterObservation>,
    requires_tombstone: bool,
}

impl CapabilityRosterObservationStream {
    /// Starts a fresh provider-local generation namespace.
    pub(crate) fn reset_for_provider_replacement(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn observe(&mut self, observation: Option<CapabilityRosterObservation>) {
        let Some(observation) = observation else {
            self.current = None;
            self.requires_tombstone |= self.last_generation.is_some();
            return;
        };
        match self.last_generation {
            Some(generation) if observation.generation() < generation => return,
            Some(generation) if observation.generation() == generation => {
                if self.last_envelope.as_ref() != Some(&observation) {
                    self.current = None;
                    self.requires_tombstone = true;
                }
                return;
            }
            Some(generation) if generation.checked_next() != Some(observation.generation()) => {
                self.last_generation = Some(observation.generation());
                self.last_envelope = Some(observation);
                self.current = None;
                self.requires_tombstone = true;
                return;
            }
            Some(_) | None => {}
        }
        self.last_generation = Some(observation.generation());
        self.last_envelope = Some(observation.clone());
        if self.requires_tombstone && observation.has_authority() {
            self.current = None;
            return;
        }
        self.current = observation.has_authority().then_some(observation);
        self.requires_tombstone = false;
    }

    pub(crate) const fn current(&self) -> Option<&CapabilityRosterObservation> {
        self.current.as_ref()
    }

    #[cfg(test)]
    pub(crate) const fn generation_watermark(&self) -> Option<CapabilityObservationGeneration> {
        self.last_generation
    }
}

/// One causally versioned complete native-window inventory observation.
///
/// A known roster is canonicalized by exact binding. A known empty roster
/// authoritatively states that no docking viewport is currently present. An
/// unknown roster is an explicit versioned tombstone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInventoryObservation {
    generation: InventoryObservationGeneration,
    roster: Authority<Vec<ViewportBinding>>,
}

impl WindowInventoryObservation {
    /// Creates an explicit versioned unavailable inventory tombstone.
    #[must_use]
    pub const fn unknown(
        generation: InventoryObservationGeneration,
        reason: AuthorityUnavailableReason,
    ) -> Self {
        Self {
            generation,
            roster: Authority::Unknown(reason),
        }
    }

    /// Creates and canonicalizes one complete provider inventory observation.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformSnapshotError`] when a known roster repeats an exact
    /// viewport binding.
    pub fn new(
        generation: InventoryObservationGeneration,
        mut roster: Authority<Vec<ViewportBinding>>,
    ) -> Result<Self, PlatformSnapshotError> {
        if let Authority::Known(bindings) = &mut roster {
            bindings.sort_unstable();
            reject_duplicate_inventory_bindings(bindings)?;
        }
        Ok(Self { generation, roster })
    }

    #[must_use]
    pub const fn generation(&self) -> InventoryObservationGeneration {
        self.generation
    }

    #[must_use]
    pub const fn roster(&self) -> &Authority<Vec<ViewportBinding>> {
        &self.roster
    }

    #[must_use]
    pub fn known_roster(&self) -> Option<&[ViewportBinding]> {
        match &self.roster {
            Authority::Known(bindings) => Some(bindings),
            Authority::Unknown(_) => None,
        }
    }

    #[must_use]
    pub const fn has_authority(&self) -> bool {
        matches!(self.roster, Authority::Known(_))
    }
}

/// Last authoritative member of the provider-owned native-window inventory stream.
///
/// Missing an authoritative envelope, skipping a generation, or reusing a generation for a
/// conflicting envelope revokes current authority and requires an explicit versioned tombstone
/// before a later known roster may become authoritative again. Older generations and exact
/// duplicates are inert.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WindowInventoryObservationStream {
    current: Option<WindowInventoryObservation>,
    last_generation: Option<InventoryObservationGeneration>,
    last_envelope: Option<WindowInventoryObservation>,
    requires_tombstone: bool,
}

impl WindowInventoryObservationStream {
    /// Starts a fresh provider-local generation namespace.
    pub(crate) fn reset_for_provider_replacement(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn observe(&mut self, observation: Option<WindowInventoryObservation>) {
        let Some(observation) = observation else {
            self.requires_tombstone |= self.current.is_some();
            self.current = None;
            return;
        };
        match self.last_generation {
            Some(generation) if observation.generation() < generation => return,
            Some(generation) if observation.generation() == generation => {
                if self.last_envelope.as_ref() != Some(&observation) {
                    self.current = None;
                    self.requires_tombstone = true;
                }
                return;
            }
            Some(generation) if generation.checked_next() != Some(observation.generation()) => {
                self.last_generation = Some(observation.generation());
                self.last_envelope = Some(observation);
                self.current = None;
                self.requires_tombstone = true;
                return;
            }
            Some(_) | None => {}
        }
        self.last_generation = Some(observation.generation());
        self.last_envelope = Some(observation.clone());
        if self.requires_tombstone && observation.has_authority() {
            self.current = None;
            return;
        }
        self.current = observation.has_authority().then_some(observation);
        self.requires_tombstone = false;
    }

    /// Returns the accepted provider watermark when this complete batch is older.
    ///
    /// Window facts in a [`PlatformSnapshot`] are captured under the inventory
    /// envelope carried by that same snapshot. They must not be reduced against
    /// a newer retained roster, because doing so would combine two causal batches.
    pub(crate) fn stale_against(
        &self,
        observation: &WindowInventoryObservation,
    ) -> Option<InventoryObservationGeneration> {
        self.last_generation
            .filter(|generation| observation.generation() < *generation)
    }

    /// Returns whether the provider reused the current generation for a
    /// different complete inventory envelope.
    ///
    /// Binding-scoped facts in the same [`PlatformSnapshot`] must not be
    /// consumed after this conflict. The viewport coordinator checks this
    /// before mutating any observation lane.
    pub(crate) fn conflicts_with(&self, observation: &WindowInventoryObservation) -> bool {
        self.last_generation == Some(observation.generation())
            && self.last_envelope.as_ref() != Some(observation)
    }

    /// Returns the accepted provider watermark when this envelope skips the
    /// next required inventory generation.
    pub(crate) fn generation_gap_before(
        &self,
        observation: &WindowInventoryObservation,
    ) -> Option<InventoryObservationGeneration> {
        self.last_generation.filter(|generation| {
            observation.generation() > *generation
                && generation.checked_next() != Some(observation.generation())
        })
    }

    pub(crate) const fn current(&self) -> Option<&WindowInventoryObservation> {
        self.current.as_ref()
    }

    #[cfg(test)]
    pub(crate) const fn generation_watermark(&self) -> Option<InventoryObservationGeneration> {
        self.last_generation
    }
}

fn reject_duplicate_inventory_bindings(
    bindings: &[ViewportBinding],
) -> Result<(), PlatformSnapshotError> {
    if let Some(binding) = bindings
        .windows(2)
        .find_map(|pair| (pair[0] == pair[1]).then_some(pair[0]))
    {
        return Err(PlatformSnapshotError::DuplicateWindowBinding { binding });
    }
    Ok(())
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
    /// Starts a fresh provider-local generation namespace.
    pub(crate) fn reset_for_provider_replacement(&mut self) {
        *self = Self::default();
    }

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

/// Provider authority over the exact effect reflected by a presentation observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PresentationEffectAcknowledgement {
    /// The named effect was applied before this observation was captured.
    Known(Option<EffectId>),
    /// The provider cannot correlate this observation with an effect.
    Unknown(AuthorityUnavailableReason),
}

impl PresentationEffectAcknowledgement {
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

/// Binding-scoped, causally versioned presentation observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowPresentationObservation {
    binding: ViewportBinding,
    generation: PresentationObservationGeneration,
    inventory_generation: InventoryGeneration,
    state: Authority<WindowPresentationState>,
    acknowledged_effect: PresentationEffectAcknowledgement,
}

impl WindowPresentationObservation {
    #[must_use]
    pub const fn new(
        binding: ViewportBinding,
        generation: PresentationObservationGeneration,
        state: Authority<WindowPresentationState>,
        acknowledged_effect: PresentationEffectAcknowledgement,
    ) -> Self {
        Self {
            binding,
            generation,
            inventory_generation: InventoryGeneration::new(0),
            state,
            acknowledged_effect,
        }
    }

    pub(crate) const fn with_inventory_generation(
        self,
        inventory_generation: InventoryGeneration,
    ) -> Self {
        Self {
            inventory_generation,
            ..self
        }
    }

    #[must_use]
    pub const fn binding(self) -> ViewportBinding {
        self.binding
    }

    #[must_use]
    pub const fn generation(self) -> PresentationObservationGeneration {
        self.generation
    }

    #[must_use]
    pub const fn inventory_generation(self) -> InventoryGeneration {
        self.inventory_generation
    }

    #[must_use]
    pub const fn state(self) -> Authority<WindowPresentationState> {
        self.state
    }

    #[must_use]
    pub const fn known_state(self) -> Option<WindowPresentationState> {
        match self.state {
            Authority::Known(state) => Some(state),
            Authority::Unknown(_) => None,
        }
    }

    #[must_use]
    pub const fn acknowledged_effect(self) -> PresentationEffectAcknowledgement {
        self.acknowledged_effect
    }

    #[must_use]
    pub fn acknowledges(self, effect: EffectId) -> bool {
        self.acknowledged_effect.acknowledges(effect)
    }

    fn same_provider_envelope(self, other: Self) -> bool {
        self.binding == other.binding
            && self.generation == other.generation
            && self.state == other.state
            && self.acknowledged_effect == other.acknowledged_effect
    }
}

/// Monotonic per-binding presentation observation stream.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct WindowPresentationObservationStream {
    current: Option<WindowPresentationObservation>,
    last_generation: Option<PresentationObservationGeneration>,
    last_envelope: Option<WindowPresentationObservation>,
    requires_tombstone: bool,
}

impl WindowPresentationObservationStream {
    pub(crate) fn observe(
        &mut self,
        binding: ViewportBinding,
        observation: Option<WindowPresentationObservation>,
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
                if !self
                    .last_envelope
                    .is_some_and(|last| last.same_provider_envelope(observation))
                {
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

    pub(crate) const fn current(self) -> Option<WindowPresentationObservation> {
        self.current
    }
}

/// One provider-captured coordinate envelope for an exact native-window binding.
///
/// The provider generation is monotonic only within the exact binding incarnation.
/// Content bounds, native scale, and presentation scale jointly authorize coordinate
/// conversion. The native scale describes operating-system logical units. The presentation
/// scale describes the surface-local coordinate system actually presented by the UI adapter.
/// Keeping them separate prevents application zoom from being mistaken for a DPI change.
/// Outer bounds remain optional, so an unavailable outer rectangle does not revoke otherwise
/// complete coordinate authority. An unavailable required fact is a versioned tombstone for
/// the whole atomic tuple.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowCoordinateObservation {
    binding: ViewportBinding,
    generation: CoordinateObservationGeneration,
    content_bounds: Authority<PhysicalRect>,
    outer_bounds: Authority<PhysicalRect>,
    native_scale_factor: Authority<ScaleFactor>,
    presentation_scale_factor: Authority<ScaleFactor>,
}

impl WindowCoordinateObservation {
    #[must_use]
    pub const fn new(
        binding: ViewportBinding,
        generation: CoordinateObservationGeneration,
        content_bounds: Authority<PhysicalRect>,
        outer_bounds: Authority<PhysicalRect>,
        native_scale_factor: Authority<ScaleFactor>,
        presentation_scale_factor: Authority<ScaleFactor>,
    ) -> Self {
        Self {
            binding,
            generation,
            content_bounds,
            outer_bounds,
            native_scale_factor,
            presentation_scale_factor,
        }
    }

    #[must_use]
    pub const fn binding(self) -> ViewportBinding {
        self.binding
    }

    #[must_use]
    pub const fn generation(self) -> CoordinateObservationGeneration {
        self.generation
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
    pub const fn native_scale_factor(&self) -> &Authority<ScaleFactor> {
        &self.native_scale_factor
    }

    /// Returns the physical-pixel scale of the surface coordinate system used for rendering.
    #[must_use]
    pub const fn presentation_scale_factor(&self) -> &Authority<ScaleFactor> {
        &self.presentation_scale_factor
    }

    #[must_use]
    pub const fn has_authority(&self) -> bool {
        matches!(self.content_bounds, Authority::Known(_))
            && matches!(self.native_scale_factor, Authority::Known(_))
            && matches!(self.presentation_scale_factor, Authority::Known(_))
    }
}

/// Last authoritative member of one provider-owned per-incarnation coordinate stream.
///
/// Missing, wrong-binding, skipped-generation, or same-generation conflicting envelopes
/// revoke current authority and require an explicit versioned tombstone before a later
/// known tuple may become authoritative again. Older generations and exact duplicates are
/// inert.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct WindowCoordinateObservationStream {
    current: Option<WindowCoordinateObservation>,
    last_generation: Option<CoordinateObservationGeneration>,
    last_envelope: Option<WindowCoordinateObservation>,
    requires_tombstone: bool,
}

impl WindowCoordinateObservationStream {
    pub(crate) fn observe(
        &mut self,
        binding: ViewportBinding,
        observation: Option<WindowCoordinateObservation>,
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
            Some(generation) if generation.checked_next() != Some(observation.generation()) => {
                self.last_generation = Some(observation.generation());
                self.last_envelope = Some(observation);
                self.current = None;
                self.requires_tombstone = true;
                return;
            }
            Some(_) | None => {}
        }
        self.last_generation = Some(observation.generation());
        self.last_envelope = Some(observation);
        if self.requires_tombstone && observation.has_authority() {
            self.current = None;
            return;
        }
        self.current = observation.has_authority().then_some(observation);
        self.requires_tombstone = false;
    }

    pub(crate) const fn current(self) -> Option<WindowCoordinateObservation> {
        self.current
    }

    #[cfg(test)]
    pub(crate) const fn generation_watermark(self) -> Option<CoordinateObservationGeneration> {
        self.last_generation
    }
}

/// Provider-observed state of one exact native close lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowCloseState {
    /// The exact binding is live without a pending native close request.
    LiveClear,
    /// The exact binding is live with a pending native close request.
    LiveRequested,
    /// The exact binding was authoritatively destroyed.
    Destroyed,
}

/// Provider authority over the close-lane effect frontier reflected by an observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CloseEffectAcknowledgement {
    /// The optional named effect is the serialized close-lane frontier.
    Known(Option<EffectId>),
    /// The provider cannot correlate this observation with a close-lane effect.
    Unknown(AuthorityUnavailableReason),
}

impl CloseEffectAcknowledgement {
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

/// One binding-scoped provider observation of native-close state.
///
/// Provider generation is monotonic only within the exact binding incarnation.
/// Core attaches the inventory generation at ingress so an observation captured
/// before effect emission cannot be made causal merely by arriving late.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowCloseObservation {
    binding: ViewportBinding,
    generation: CloseObservationGeneration,
    inventory_generation: InventoryGeneration,
    state: Authority<WindowCloseState>,
    acknowledged_effect: CloseEffectAcknowledgement,
}

impl WindowCloseObservation {
    #[must_use]
    pub const fn new(
        binding: ViewportBinding,
        generation: CloseObservationGeneration,
        state: Authority<WindowCloseState>,
        acknowledged_effect: CloseEffectAcknowledgement,
    ) -> Self {
        Self {
            binding,
            generation,
            inventory_generation: InventoryGeneration::new(0),
            state,
            acknowledged_effect,
        }
    }

    pub(crate) const fn with_inventory_generation(
        self,
        inventory_generation: InventoryGeneration,
    ) -> Self {
        Self {
            inventory_generation,
            ..self
        }
    }

    #[must_use]
    pub const fn binding(self) -> ViewportBinding {
        self.binding
    }

    #[must_use]
    pub const fn generation(self) -> CloseObservationGeneration {
        self.generation
    }

    #[must_use]
    pub const fn inventory_generation(self) -> InventoryGeneration {
        self.inventory_generation
    }

    #[must_use]
    pub const fn state(self) -> Authority<WindowCloseState> {
        self.state
    }

    #[must_use]
    pub const fn known_state(self) -> Option<WindowCloseState> {
        match self.state {
            Authority::Known(state) => Some(state),
            Authority::Unknown(_) => None,
        }
    }

    #[must_use]
    pub const fn acknowledged_effect(self) -> CloseEffectAcknowledgement {
        self.acknowledged_effect
    }

    #[must_use]
    pub fn acknowledges(self, effect: EffectId) -> bool {
        self.acknowledged_effect.acknowledges(effect)
    }

    fn same_provider_envelope(self, other: Self) -> bool {
        self.binding == other.binding
            && self.generation == other.generation
            && self.state == other.state
            && self.acknowledged_effect == other.acknowledged_effect
    }
}

/// Last authoritative member of one exact binding's provider-owned close stream.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct WindowCloseObservationStream {
    current: Option<WindowCloseObservation>,
    last_generation: Option<CloseObservationGeneration>,
    last_envelope: Option<WindowCloseObservation>,
    requires_tombstone: bool,
    destroyed: bool,
}

impl WindowCloseObservationStream {
    /// Starts a fresh provider-local generation namespace.
    ///
    /// A terminal destruction fact belongs to the provider that observed it.
    /// Completed retirements are removed from the active lifecycle before a
    /// replacement begins, so carrying that terminal fact into a successor
    /// would incorrectly attribute predecessor authority to the new provider.
    pub(crate) fn reset_for_provider_replacement(&mut self) {
        debug_assert!(
            !self.destroyed,
            "destroyed close streams must leave the active lifecycle before provider replacement"
        );
        *self = Self::default();
    }

    pub(crate) fn observe(
        &mut self,
        binding: ViewportBinding,
        observation: Option<WindowCloseObservation>,
    ) {
        if self.destroyed {
            return;
        }
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
                if !self
                    .last_envelope
                    .is_some_and(|last| last.same_provider_envelope(observation))
                {
                    self.current = None;
                    self.requires_tombstone = true;
                }
                return;
            }
            Some(_) | None => {}
        }
        self.last_generation = Some(observation.generation());
        self.last_envelope = Some(observation);
        let known_state = observation.known_state();
        if self.requires_tombstone
            && known_state.is_some()
            && known_state != Some(WindowCloseState::Destroyed)
        {
            self.current = None;
            return;
        }
        self.current = known_state.map(|_| observation);
        self.requires_tombstone = false;
        self.destroyed = known_state == Some(WindowCloseState::Destroyed);
    }

    pub(crate) const fn current(self) -> Option<WindowCloseObservation> {
        self.current
    }
}

/// Complete provider facts for one exact native-window binding.
#[derive(Debug, Clone, PartialEq)]
pub struct ObservedWindow {
    binding: ViewportBinding,
    coordinate_observation: Option<WindowCoordinateObservation>,
    input_state: Authority<WindowInputState>,
    input_observation: Option<WindowInputObservation>,
    presentation_observation: Option<WindowPresentationObservation>,
    close_requested: Authority<bool>,
}

impl ObservedWindow {
    /// Creates a binding-scoped observation whose individual facts are explicitly unavailable.
    ///
    /// The provider must retain the exact binding returned when the viewport was
    /// registered. A window token alone is deliberately insufficient because a
    /// late callback from a prior incarnation could otherwise authorize a newer
    /// binding that reused the token.
    #[must_use]
    pub fn new(binding: ViewportBinding) -> Self {
        Self {
            binding,
            coordinate_observation: None,
            input_state: unavailable(),
            input_observation: None,
            presentation_observation: None,
            close_requested: unavailable(),
        }
    }

    #[must_use]
    pub const fn binding(&self) -> ViewportBinding {
        self.binding
    }

    #[must_use]
    pub const fn coordinate_observation(&self) -> Option<WindowCoordinateObservation> {
        self.coordinate_observation
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
    pub const fn presentation_observation(&self) -> Option<WindowPresentationObservation> {
        self.presentation_observation
    }

    #[must_use]
    pub const fn close_requested(&self) -> &Authority<bool> {
        &self.close_requested
    }

    #[must_use]
    pub fn with_coordinate_observation(mut self, value: WindowCoordinateObservation) -> Self {
        self.coordinate_observation = Some(value);
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
    pub fn with_presentation_observation(mut self, value: WindowPresentationObservation) -> Self {
        self.presentation_observation = Some(value);
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

/// One causally versioned complete desktop work-area roster observation.
///
/// A known roster is canonicalized by token. An unknown roster is an explicit
/// versioned tombstone; omission never stands for unavailability.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkAreaRosterObservation {
    generation: WorkAreaObservationGeneration,
    roster: Authority<Vec<ObservedWorkArea>>,
}

impl WorkAreaRosterObservation {
    /// Creates an explicit versioned unavailable roster tombstone.
    #[must_use]
    pub const fn unknown(
        generation: WorkAreaObservationGeneration,
        reason: AuthorityUnavailableReason,
    ) -> Self {
        Self {
            generation,
            roster: Authority::Unknown(reason),
        }
    }

    /// Creates and canonicalizes one complete provider observation.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformSnapshotError`] when a known roster repeats a token or
    /// contains an empty work area.
    pub fn new(
        generation: WorkAreaObservationGeneration,
        mut roster: Authority<Vec<ObservedWorkArea>>,
    ) -> Result<Self, PlatformSnapshotError> {
        if let Authority::Known(work_areas) = &mut roster {
            work_areas.sort_by_key(|work_area| work_area.token);
            validate_work_areas(work_areas)?;
        }
        Ok(Self { generation, roster })
    }

    #[must_use]
    pub const fn generation(&self) -> WorkAreaObservationGeneration {
        self.generation
    }

    #[must_use]
    pub const fn roster(&self) -> &Authority<Vec<ObservedWorkArea>> {
        &self.roster
    }

    #[must_use]
    pub fn known_roster(&self) -> Option<&[ObservedWorkArea]> {
        match &self.roster {
            Authority::Known(work_areas) => Some(work_areas),
            Authority::Unknown(_) => None,
        }
    }

    #[must_use]
    pub const fn has_authority(&self) -> bool {
        matches!(self.roster, Authority::Known(_))
    }
}

/// Last authoritative member of the provider-owned global work-area stream.
///
/// Missing, skipped-generation, or same-generation conflicting envelopes revoke current
/// authority and require an explicit versioned tombstone before a later known roster may
/// become authoritative again. Older generations and exact duplicates are inert.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct WorkAreaRosterObservationStream {
    current: Option<WorkAreaRosterObservation>,
    last_generation: Option<WorkAreaObservationGeneration>,
    last_envelope: Option<WorkAreaRosterObservation>,
    requires_tombstone: bool,
}

impl WorkAreaRosterObservationStream {
    /// Starts a fresh provider-local generation namespace.
    pub(crate) fn reset_for_provider_replacement(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn observe(&mut self, observation: Option<WorkAreaRosterObservation>) {
        let Some(observation) = observation else {
            self.requires_tombstone |= self.current.is_some();
            self.current = None;
            return;
        };
        match self.last_generation {
            Some(generation) if observation.generation() < generation => return,
            Some(generation) if observation.generation() == generation => {
                if self.last_envelope.as_ref() != Some(&observation) {
                    self.current = None;
                    self.requires_tombstone = true;
                }
                return;
            }
            Some(generation) if generation.checked_next() != Some(observation.generation()) => {
                self.last_generation = Some(observation.generation());
                self.last_envelope = Some(observation);
                self.current = None;
                self.requires_tombstone = true;
                return;
            }
            Some(_) | None => {}
        }
        self.last_generation = Some(observation.generation());
        self.last_envelope = Some(observation.clone());
        if self.requires_tombstone && observation.has_authority() {
            self.current = None;
            return;
        }
        self.current = observation.has_authority().then_some(observation);
        self.requires_tombstone = false;
    }

    pub(crate) const fn current(&self) -> Option<&WorkAreaRosterObservation> {
        self.current.as_ref()
    }

    pub(crate) const fn generation_watermark(&self) -> Option<WorkAreaObservationGeneration> {
        self.last_generation
    }
}

fn unavailable<T>() -> Authority<T> {
    Authority::Unknown(AuthorityUnavailableReason::NotReported)
}

/// One complete frame-before-paint platform fact snapshot.
///
/// Every binding-scoped fact belongs to the same inventory envelope. A consumer
/// must reject the complete snapshot when that envelope predates its accepted
/// inventory watermark; individual window facts cannot be reduced against a
/// newer retained roster.
#[derive(Debug, Clone, PartialEq)]
pub struct PlatformSnapshot {
    generation: PlatformSnapshotGeneration,
    capability_observation: CapabilityRosterObservation,
    focus: FocusObservationEnvelope,
    inventory_observation: WindowInventoryObservation,
    window_observations: Vec<ObservedWindow>,
    close_observations: Vec<WindowCloseObservation>,
    work_area_observation: WorkAreaRosterObservation,
}

impl PlatformSnapshot {
    /// Creates a canonical complete snapshot and rejects ambiguous identities.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformSnapshotError`] for duplicate identities, a mismatched complete
    /// inventory roster, or a work-area roster that contradicts the declared capability.
    pub fn new(
        generation: PlatformSnapshotGeneration,
        capability_observation: CapabilityRosterObservation,
        focus: FocusObservationEnvelope,
        inventory_observation: WindowInventoryObservation,
        mut window_observations: Vec<ObservedWindow>,
        mut close_observations: Vec<WindowCloseObservation>,
        work_area_observation: WorkAreaRosterObservation,
    ) -> Result<Self, PlatformSnapshotError> {
        window_observations.sort_by_key(ObservedWindow::binding);
        reject_duplicate_windows(&window_observations)?;
        validate_focus_window(focus, &window_observations)?;
        validate_input_observations(&window_observations)?;
        validate_inventory_observation(
            &capability_observation,
            &inventory_observation,
            &window_observations,
        )?;
        close_observations.sort_by_key(|observation| observation.binding());
        validate_close_observations(&window_observations, &close_observations)?;
        match work_area_observation.roster() {
            Authority::Known(_)
                if !capability_observation
                    .known_roster()
                    .is_some_and(|capabilities| capabilities.work_area().is_supported()) =>
            {
                return Err(PlatformSnapshotError::WorkAreaRosterWithoutCapability);
            }
            Authority::Known(work_areas) if work_areas.is_empty() => {
                return Err(PlatformSnapshotError::MissingWorkAreaRoster);
            }
            Authority::Known(_) | Authority::Unknown(_) => {}
        }
        Ok(Self {
            generation,
            capability_observation,
            focus,
            inventory_observation,
            window_observations,
            close_observations,
            work_area_observation,
        })
    }

    /// Returns the provider-local identity of this complete atomic snapshot.
    #[must_use]
    pub const fn generation(&self) -> PlatformSnapshotGeneration {
        self.generation
    }

    #[must_use]
    pub const fn capability_observation(&self) -> &CapabilityRosterObservation {
        &self.capability_observation
    }

    /// Returns the provider-generated single global native-focus observation.
    #[must_use]
    pub const fn focus(&self) -> FocusObservationEnvelope {
        self.focus
    }

    #[must_use]
    pub const fn inventory_observation(&self) -> &WindowInventoryObservation {
        &self.inventory_observation
    }

    /// Returns binding-scoped window facts. This is not an inventory authority;
    /// only [`Self::inventory_observation`] can establish complete absence.
    #[must_use]
    pub fn window_observations(&self) -> &[ObservedWindow] {
        &self.window_observations
    }

    /// Returns binding-scoped close observations, including destroyed tombstones.
    #[must_use]
    pub fn close_observations(&self) -> &[WindowCloseObservation] {
        &self.close_observations
    }

    #[must_use]
    pub const fn work_area_observation(&self) -> &WorkAreaRosterObservation {
        &self.work_area_observation
    }
}

fn validate_inventory_observation(
    capability_observation: &CapabilityRosterObservation,
    inventory_observation: &WindowInventoryObservation,
    windows: &[ObservedWindow],
) -> Result<(), PlatformSnapshotError> {
    let Some(inventory) = inventory_observation.known_roster() else {
        return Ok(());
    };
    let Some(capabilities) = capability_observation.known_roster() else {
        return Err(PlatformSnapshotError::InventoryRosterWithoutCapability);
    };
    if !capabilities.authoritative_inventory().is_supported() {
        return Err(PlatformSnapshotError::InventoryRosterWithoutCapability);
    }
    let observed = windows
        .iter()
        .map(ObservedWindow::binding)
        .collect::<BTreeSet<_>>();
    let reported = inventory.iter().copied().collect::<BTreeSet<_>>();
    if observed != reported {
        return Err(PlatformSnapshotError::InventoryRosterMismatch);
    }
    Ok(())
}

fn validate_focus_window(
    focus: FocusObservationEnvelope,
    windows: &[ObservedWindow],
) -> Result<(), PlatformSnapshotError> {
    let Authority::Known(crate::viewport_focus::GlobalFocusedWindow::Dock(binding)) =
        focus.focused()
    else {
        return Ok(());
    };
    if windows.iter().any(|window| window.binding() == *binding) {
        Ok(())
    } else {
        Err(PlatformSnapshotError::FocusWindowWithoutObservation { binding: *binding })
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
    let mut bindings = BTreeSet::new();
    let mut tokens = BTreeSet::new();
    for window in windows {
        let binding = window.binding();
        if !bindings.insert(binding) {
            return Err(PlatformSnapshotError::DuplicateWindowBinding { binding });
        }
        if !tokens.insert(binding.token()) {
            return Err(PlatformSnapshotError::DuplicateWindowToken {
                token: binding.token(),
            });
        }
    }
    Ok(())
}

fn validate_input_observations(windows: &[ObservedWindow]) -> Result<(), PlatformSnapshotError> {
    for window in windows {
        let binding = window.binding();
        if let Some(observation) = window.coordinate_observation()
            && observation.binding() != binding
        {
            return Err(
                PlatformSnapshotError::CoordinateObservationBindingMismatch {
                    window: binding,
                    observation: observation.binding(),
                },
            );
        }
        if let Some(observation) = window.input_observation() {
            if observation.binding() != binding {
                return Err(PlatformSnapshotError::InputObservationBindingMismatch {
                    window: binding,
                    observation: observation.binding(),
                });
            }
            if let Some(state) = observation.known_state()
                && window.input_state() != &Authority::Known(state)
            {
                return Err(PlatformSnapshotError::InputObservationStateMismatch { binding });
            }
        }
        if let Some(observation) = window.presentation_observation()
            && observation.binding() != binding
        {
            return Err(
                PlatformSnapshotError::PresentationObservationBindingMismatch {
                    window: binding,
                    observation: observation.binding(),
                },
            );
        }
    }
    Ok(())
}

fn validate_close_observations(
    windows: &[ObservedWindow],
    observations: &[WindowCloseObservation],
) -> Result<(), PlatformSnapshotError> {
    let window_bindings = windows
        .iter()
        .map(ObservedWindow::binding)
        .collect::<BTreeSet<_>>();
    let mut bindings = BTreeSet::new();
    for observation in observations {
        let binding = observation.binding();
        if !bindings.insert(binding) {
            return Err(PlatformSnapshotError::DuplicateCloseObservation { binding });
        }
        match observation.known_state() {
            Some(WindowCloseState::LiveClear | WindowCloseState::LiveRequested)
                if !window_bindings.contains(&binding) =>
            {
                return Err(PlatformSnapshotError::LiveCloseObservationWithoutWindow { binding });
            }
            Some(WindowCloseState::Destroyed) if window_bindings.contains(&binding) => {
                return Err(PlatformSnapshotError::DestroyedCloseObservationWithWindow { binding });
            }
            Some(
                WindowCloseState::LiveClear
                | WindowCloseState::LiveRequested
                | WindowCloseState::Destroyed,
            )
            | None => {}
        }
    }
    Ok(())
}

/// Malformed complete platform snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PlatformSnapshotError {
    #[error(
        "a complete native-window inventory was supplied without supported inventory capability"
    )]
    InventoryRosterWithoutCapability,
    #[error(
        "the complete native-window inventory does not exactly match binding-scoped window facts"
    )]
    InventoryRosterMismatch,
    #[error("platform snapshot repeats window binding {binding:?}")]
    DuplicateWindowBinding { binding: ViewportBinding },
    #[error("platform snapshot repeats window token {token:?} across bindings")]
    DuplicateWindowToken { token: WindowToken },
    #[error(
        "window {window:?} carries a coordinate observation for a different binding {observation:?}"
    )]
    CoordinateObservationBindingMismatch {
        window: ViewportBinding,
        observation: ViewportBinding,
    },
    #[error(
        "window {window:?} carries an input observation for a different binding {observation:?}"
    )]
    InputObservationBindingMismatch {
        window: ViewportBinding,
        observation: ViewportBinding,
    },
    #[error("window {binding:?} carries conflicting input state and causal observation")]
    InputObservationStateMismatch { binding: ViewportBinding },
    #[error(
        "window {window:?} carries a presentation observation for a different binding {observation:?}"
    )]
    PresentationObservationBindingMismatch {
        window: ViewportBinding,
        observation: ViewportBinding,
    },
    #[error("platform snapshot repeats a close observation for binding {binding:?}")]
    DuplicateCloseObservation { binding: ViewportBinding },
    #[error("live close observation names a binding absent from the window roster: {binding:?}")]
    LiveCloseObservationWithoutWindow { binding: ViewportBinding },
    #[error(
        "destroyed close observation names a binding still present in the window roster: {binding:?}"
    )]
    DestroyedCloseObservationWithWindow { binding: ViewportBinding },
    #[error("global focus names an unobserved docking binding {binding:?}")]
    FocusWindowWithoutObservation { binding: ViewportBinding },
    #[error("platform snapshot repeats work-area token {token:?}")]
    DuplicateWorkArea { token: WorkAreaToken },
    #[error("platform snapshot contains an empty work area: {token:?}")]
    EmptyWorkArea { token: WorkAreaToken },
    #[error("supported work-area capability requires a non-empty complete roster")]
    MissingWorkAreaRoster,
    #[error("a work-area roster was supplied without supported work-area capability")]
    WorkAreaRosterWithoutCapability,
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

    fn unknown_work_areas(generation: u64) -> WorkAreaRosterObservation {
        WorkAreaRosterObservation::new(
            WorkAreaObservationGeneration::new(generation),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        )
        .expect("work-area tombstone must be valid")
    }

    fn snapshot_with_unknown_inventory(
        provider_generation: u64,
        capabilities: PlatformCapabilities,
        windows: Vec<ObservedWindow>,
        close_observations: Vec<WindowCloseObservation>,
        work_area_observation: WorkAreaRosterObservation,
    ) -> Result<PlatformSnapshot, PlatformSnapshotError> {
        PlatformSnapshot::new(
            PlatformSnapshotGeneration::new(provider_generation),
            CapabilityRosterObservation::new(
                CapabilityObservationGeneration::new(provider_generation),
                Authority::Known(capabilities),
            ),
            unknown_focus(provider_generation),
            WindowInventoryObservation::unknown(
                InventoryObservationGeneration::new(provider_generation),
                AuthorityUnavailableReason::NotReported,
            ),
            windows,
            close_observations,
            work_area_observation,
        )
    }

    fn binding(incarnation: u64) -> ViewportBinding {
        ViewportBinding::new(
            crate::ids::EngineAuthorityDomainId::new_for_test(1),
            crate::ids::WorkspaceEpoch::new(1),
            crate::ids::SurfaceId::new(2),
            WindowToken::new(3),
            crate::viewport::WindowIncarnation::new(incarnation),
        )
    }

    fn capabilities(
        generation: u64,
        native_window_lifecycle: PlatformCapability,
    ) -> CapabilityRosterObservation {
        let mut roster = PlatformCapabilities::default();
        roster.set_native_window_lifecycle(native_window_lifecycle);
        CapabilityRosterObservation::new(
            CapabilityObservationGeneration::new(generation),
            Authority::Known(roster),
        )
    }

    #[test]
    fn capability_observation_stream_requires_causal_tombstones() {
        let supported = PlatformCapability::Supported;
        let unsupported = PlatformCapability::unsupported(
            PlatformRequirement::NativeWindowLifecycle,
            PlatformCapabilityReason::BackendUnsupported,
        );
        let known = |generation, capability| capabilities(generation, capability);
        let tombstone = |generation| {
            CapabilityRosterObservation::unknown(
                CapabilityObservationGeneration::new(generation),
                AuthorityUnavailableReason::NotReported,
            )
        };
        let mut stream = CapabilityRosterObservationStream::default();

        let initial = known(4, supported);
        stream.observe(Some(initial.clone()));
        assert_eq!(stream.current(), Some(&initial));

        stream.observe(Some(known(3, unsupported)));
        assert_eq!(stream.current(), Some(&initial), "older rosters are inert");
        stream.observe(Some(initial.clone()));
        assert_eq!(stream.current(), Some(&initial), "duplicates are inert");

        stream.observe(Some(known(4, unsupported)));
        assert_eq!(
            stream.current(),
            None,
            "same-generation conflict revokes authority"
        );
        stream.observe(Some(known(5, supported)));
        assert_eq!(
            stream.current(),
            None,
            "known rosters cannot bridge a conflict"
        );
        stream.observe(Some(tombstone(6)));
        assert_eq!(stream.current(), None);

        let restored = known(7, unsupported);
        stream.observe(Some(restored.clone()));
        assert_eq!(stream.current(), Some(&restored));
        assert_eq!(
            stream.generation_watermark(),
            Some(CapabilityObservationGeneration::new(7))
        );

        stream.observe(Some(known(9, supported)));
        assert_eq!(
            stream.current(),
            None,
            "a skipped generation revokes authority"
        );
        stream.observe(Some(known(10, unsupported)));
        assert_eq!(
            stream.current(),
            None,
            "known rosters cannot bridge a generation gap"
        );
        stream.observe(Some(tombstone(11)));
        let post_gap = known(12, supported);
        stream.observe(Some(post_gap.clone()));
        assert_eq!(stream.current(), Some(&post_gap));
    }

    #[test]
    fn inventory_observation_canonicalizes_exact_bindings_and_accepts_empty() {
        let first = binding(1);
        let second = binding(2);
        let observation = WindowInventoryObservation::new(
            InventoryObservationGeneration::new(1),
            Authority::Known(vec![second, first]),
        )
        .expect("distinct exact bindings must form a valid inventory");

        assert_eq!(observation.known_roster(), Some([first, second].as_slice()));

        let empty = WindowInventoryObservation::new(
            InventoryObservationGeneration::new(2),
            Authority::Known(Vec::new()),
        )
        .expect("a known empty inventory is authoritative");
        assert_eq!(empty.known_roster(), Some([].as_slice()));

        assert_eq!(
            WindowInventoryObservation::new(
                InventoryObservationGeneration::new(3),
                Authority::Known(vec![first, first]),
            ),
            Err(PlatformSnapshotError::DuplicateWindowBinding { binding: first })
        );
    }

    #[test]
    fn inventory_observation_stream_requires_causal_tombstones() {
        let first = binding(1);
        let second = binding(2);
        let known = |generation, bindings| {
            WindowInventoryObservation::new(
                InventoryObservationGeneration::new(generation),
                Authority::Known(bindings),
            )
            .expect("test inventory must be valid")
        };
        let tombstone = |generation| {
            WindowInventoryObservation::unknown(
                InventoryObservationGeneration::new(generation),
                AuthorityUnavailableReason::NotReported,
            )
        };
        let mut stream = WindowInventoryObservationStream::default();

        stream.observe(Some(tombstone(1)));
        stream.observe(None);
        let first_authoritative = known(2, vec![first]);
        stream.observe(Some(first_authoritative.clone()));
        assert_eq!(
            stream.current(),
            Some(&first_authoritative),
            "missing capability must not quarantine an inventory that never held authority"
        );

        stream.reset_for_provider_replacement();

        let initial = known(11, vec![second, first]);
        stream.observe(Some(initial.clone()));
        assert_eq!(stream.current(), Some(&initial));

        stream.observe(Some(known(10, vec![first])));
        assert_eq!(stream.current(), Some(&initial), "older rosters are inert");
        stream.observe(Some(known(11, vec![first, second])));
        assert_eq!(stream.current(), Some(&initial), "duplicates are inert");

        stream.observe(Some(known(11, vec![first])));
        assert_eq!(
            stream.current(),
            None,
            "same-generation conflict revokes authority"
        );
        stream.observe(Some(known(12, vec![second])));
        assert_eq!(
            stream.current(),
            None,
            "known rosters cannot bridge a conflict"
        );
        stream.observe(Some(tombstone(13)));

        let restored = known(14, Vec::new());
        stream.observe(Some(restored.clone()));
        assert_eq!(stream.current(), Some(&restored));
        assert_eq!(
            stream.generation_watermark(),
            Some(InventoryObservationGeneration::new(14))
        );

        stream.observe(Some(known(16, vec![second])));
        assert_eq!(
            stream.current(),
            None,
            "a skipped generation revokes authority"
        );
        stream.observe(Some(known(17, vec![first])));
        assert_eq!(
            stream.current(),
            None,
            "known rosters cannot bridge a generation gap"
        );
        stream.observe(Some(tombstone(18)));
        let post_gap = known(19, vec![first]);
        stream.observe(Some(post_gap.clone()));
        assert_eq!(stream.current(), Some(&post_gap));
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn coordinate_observation_stream_requires_causal_tombstones() {
        let current_binding = binding(1);
        let other_binding = binding(2);
        let content =
            PhysicalRect::new(10.0, 20.0, 800.0, 600.0).expect("test bounds must be valid");
        let outer = PhysicalRect::new(2.0, -10.0, 816.0, 638.0).expect("test bounds must be valid");
        let scale = ScaleFactor::new(2.0).expect("test scale must be valid");
        let known = |binding, generation, outer_bounds| {
            WindowCoordinateObservation::new(
                binding,
                CoordinateObservationGeneration::new(generation),
                Authority::Known(content),
                outer_bounds,
                Authority::Known(scale),
                Authority::Known(scale),
            )
        };
        let tombstone = |generation| {
            WindowCoordinateObservation::new(
                current_binding,
                CoordinateObservationGeneration::new(generation),
                unavailable(),
                unavailable(),
                unavailable(),
                unavailable(),
            )
        };
        let mut stream = WindowCoordinateObservationStream::default();

        let initial = known(current_binding, 4, Authority::Known(outer));
        stream.observe(current_binding, Some(initial));
        assert_eq!(stream.current(), Some(initial));

        stream.observe(
            current_binding,
            Some(known(
                current_binding,
                3,
                Authority::Unknown(AuthorityUnavailableReason::NotReported),
            )),
        );
        assert_eq!(stream.current(), Some(initial), "older facts are inert");
        stream.observe(current_binding, Some(initial));
        assert_eq!(stream.current(), Some(initial), "duplicates are inert");

        let outer_unknown = known(
            current_binding,
            5,
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        );
        stream.observe(current_binding, Some(outer_unknown));
        assert_eq!(
            stream.current(),
            Some(outer_unknown),
            "outer bounds are optional inside an authoritative tuple"
        );

        stream.observe(
            current_binding,
            Some(known(current_binding, 5, Authority::Known(outer))),
        );
        assert_eq!(
            stream.current(),
            None,
            "same-generation conflict revokes authority"
        );
        stream.observe(
            current_binding,
            Some(known(current_binding, 6, Authority::Known(outer))),
        );
        assert_eq!(
            stream.current(),
            None,
            "known facts cannot bridge a conflict"
        );
        stream.observe(current_binding, Some(tombstone(7)));
        assert_eq!(stream.current(), None);
        let restored = known(current_binding, 8, Authority::Known(outer));
        stream.observe(current_binding, Some(restored));
        assert_eq!(stream.current(), Some(restored));

        stream.observe(
            current_binding,
            Some(known(other_binding, 9, Authority::Known(outer))),
        );
        assert_eq!(stream.current(), None, "wrong binding revokes authority");
        stream.observe(
            current_binding,
            Some(known(current_binding, 10, Authority::Known(outer))),
        );
        assert_eq!(
            stream.current(),
            None,
            "wrong-binding gaps require a tombstone"
        );
        stream.observe(current_binding, Some(tombstone(11)));
        stream.observe(
            current_binding,
            Some(known(current_binding, 12, Authority::Known(outer))),
        );
        assert!(stream.current().is_some());

        stream.observe(
            current_binding,
            Some(known(current_binding, 14, Authority::Known(outer))),
        );
        assert_eq!(
            stream.current(),
            None,
            "a skipped generation revokes authority"
        );
        stream.observe(
            current_binding,
            Some(known(current_binding, 15, Authority::Known(outer))),
        );
        assert_eq!(
            stream.current(),
            None,
            "known facts cannot bridge a generation gap"
        );
        stream.observe(current_binding, Some(tombstone(16)));
        let post_gap = known(current_binding, 17, Authority::Known(outer));
        stream.observe(current_binding, Some(post_gap));
        assert_eq!(stream.current(), Some(post_gap));
    }

    #[test]
    fn work_area_observation_stream_requires_causal_tombstones() {
        let bounds =
            PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).expect("test work area must be valid");
        let scale = ScaleFactor::new(1.0).expect("test scale must be valid");
        let area = |token| ObservedWorkArea::new(WorkAreaToken::new(token), bounds, scale);
        let known = |generation, roster| {
            WorkAreaRosterObservation::new(
                WorkAreaObservationGeneration::new(generation),
                Authority::Known(roster),
            )
            .expect("test work-area roster must be valid")
        };
        let tombstone = |generation| {
            WorkAreaRosterObservation::new(
                WorkAreaObservationGeneration::new(generation),
                Authority::Unknown(AuthorityUnavailableReason::NotReported),
            )
            .expect("work-area tombstone must be valid")
        };
        let mut stream = WorkAreaRosterObservationStream::default();

        stream.observe(Some(tombstone(1)));
        stream.observe(None);
        let first_known = known(2, vec![area(1)]);
        stream.observe(Some(first_known.clone()));
        assert_eq!(
            stream.current(),
            Some(&first_known),
            "an initial unavailable capability does not quarantine the first exact roster"
        );

        stream.reset_for_provider_replacement();

        let initial = known(4, vec![area(1), area(2)]);
        stream.observe(Some(initial.clone()));
        assert_eq!(stream.current(), Some(&initial));

        stream.observe(Some(known(3, vec![area(1)])));
        assert_eq!(stream.current(), Some(&initial), "older rosters are inert");
        stream.observe(Some(initial.clone()));
        assert_eq!(stream.current(), Some(&initial), "duplicates are inert");

        stream.observe(Some(known(4, vec![area(1)])));
        assert_eq!(
            stream.current(),
            None,
            "same-generation conflict revokes authority"
        );
        stream.observe(Some(known(5, vec![area(1)])));
        assert_eq!(
            stream.current(),
            None,
            "known rosters cannot bridge a conflict"
        );
        stream.observe(Some(tombstone(6)));
        assert_eq!(stream.current(), None);

        let restored = known(7, vec![area(1)]);
        stream.observe(Some(restored.clone()));
        assert_eq!(stream.current(), Some(&restored));
        assert_eq!(
            stream.generation_watermark(),
            Some(WorkAreaObservationGeneration::new(7))
        );

        stream.observe(None);
        stream.observe(Some(known(8, vec![area(1), area(2)])));
        assert_eq!(
            stream.current(),
            None,
            "an unversioned gap quarantines known facts"
        );
        stream.observe(Some(tombstone(9)));
        let final_roster = known(10, vec![area(1)]);
        stream.observe(Some(final_roster.clone()));
        assert_eq!(stream.current(), Some(&final_roster));

        stream.observe(Some(known(12, vec![area(1), area(2)])));
        assert_eq!(
            stream.current(),
            None,
            "a skipped generation revokes authority"
        );
        stream.observe(Some(known(13, vec![area(1)])));
        assert_eq!(
            stream.current(),
            None,
            "known rosters cannot bridge a generation gap"
        );
        stream.observe(Some(tombstone(14)));
        let post_gap = known(15, vec![area(1)]);
        stream.observe(Some(post_gap.clone()));
        assert_eq!(stream.current(), Some(&post_gap));
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
            capabilities.native_outside_all_tear_off(),
            PlatformCapability::Unsupported(issue)
                if issue.requirement() == PlatformRequirement::AuthoritativeInventory
        ));
    }

    #[test]
    fn placement_and_partial_routes_do_not_qualify_a_physical_drag() {
        let mut capabilities = PlatformCapabilities::default();
        capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
        capabilities.set_authoritative_inventory(PlatformCapability::Supported);
        capabilities.set_hovered_window(PlatformCapability::Supported);
        capabilities.set_desktop_pointer_position(PlatformCapability::Supported);
        capabilities.set_global_window_placement(PlatformCapability::Supported);
        capabilities.set_work_area(PlatformCapability::Supported);
        capabilities.set_pointer_hit_test_observation(PlatformCapability::Supported);
        capabilities.set_pointer_hit_test_control(PlatformCapability::Supported);

        assert!(capabilities.native_exact_placement_create().is_supported());
        assert!(capabilities.cross_surface_routing().is_supported());
        assert!(matches!(
            capabilities.physical_cross_surface_drag(),
            PlatformCapability::Unknown(issue)
                if issue.requirement() == PlatformRequirement::AuthoritativeButtonState
        ));
        assert!(matches!(
            capabilities.physical_native_drag(),
            PlatformCapability::Unknown(issue)
                if issue.requirement() == PlatformRequirement::AuthoritativeButtonState
        ));
    }

    #[test]
    fn physical_cross_surface_drag_does_not_require_window_placement() {
        let mut capabilities = PlatformCapabilities::default();
        capabilities.set_authoritative_inventory(PlatformCapability::Supported);
        capabilities.set_hovered_window(PlatformCapability::Supported);
        capabilities.set_desktop_pointer_position(PlatformCapability::Supported);
        capabilities.set_authoritative_button_state(PlatformCapability::Supported);
        capabilities.set_pointer_hit_test_observation(PlatformCapability::Supported);
        capabilities.set_pointer_hit_test_control(PlatformCapability::Supported);

        assert!(capabilities.physical_cross_surface_drag().is_supported());
        assert!(!capabilities.native_exact_placement_create().is_supported());
        assert!(!capabilities.physical_native_drag().is_supported());
    }

    #[test]
    fn snapshot_rejects_duplicate_window_identities() {
        let window = ObservedWindow::new(binding(7));
        assert!(matches!(
            snapshot_with_unknown_inventory(
                1,
                PlatformCapabilities::default(),
                vec![window.clone(), window],
                Vec::new(),
                unknown_work_areas(1),
            ),
            Err(PlatformSnapshotError::DuplicateWindowBinding { .. })
        ));
    }

    #[test]
    fn snapshot_requires_the_focused_dock_binding_to_match_an_exact_window_fact() {
        let delayed = binding(1);
        let current = binding(2);
        let focus = FocusObservationEnvelope::new(
            FocusObservationGeneration::new(1),
            Authority::Known(crate::viewport_focus::GlobalFocusedWindow::Dock(delayed)),
            Authority::Known(None),
        );

        assert_eq!(
            PlatformSnapshot::new(
                PlatformSnapshotGeneration::new(1),
                CapabilityRosterObservation::new(
                    CapabilityObservationGeneration::new(1),
                    Authority::Known(PlatformCapabilities::default()),
                ),
                focus,
                WindowInventoryObservation::unknown(
                    InventoryObservationGeneration::new(1),
                    AuthorityUnavailableReason::NotReported,
                ),
                vec![ObservedWindow::new(current)],
                Vec::new(),
                unknown_work_areas(1),
            ),
            Err(PlatformSnapshotError::FocusWindowWithoutObservation { binding: delayed })
        );
    }

    #[test]
    fn snapshot_rejects_a_same_token_observation_from_a_prior_incarnation() {
        let current = binding(2);
        let delayed = binding(1);
        let window =
            ObservedWindow::new(current).with_input_observation(WindowInputObservation::new(
                delayed,
                InputObservationGeneration::new(1),
                Authority::Known(WindowInputState::ReceivesInput),
                InputEffectAcknowledgement::known(None),
            ));

        assert_eq!(
            snapshot_with_unknown_inventory(
                1,
                PlatformCapabilities::default(),
                vec![window],
                Vec::new(),
                unknown_work_areas(1),
            ),
            Err(PlatformSnapshotError::InputObservationBindingMismatch {
                window: current,
                observation: delayed,
            })
        );
    }

    #[test]
    fn snapshot_rejects_coordinates_from_a_prior_binding_incarnation() {
        let current = binding(2);
        let delayed = binding(1);
        let window = ObservedWindow::new(current).with_coordinate_observation(
            WindowCoordinateObservation::new(
                delayed,
                CoordinateObservationGeneration::new(7),
                Authority::Known(
                    PhysicalRect::new(0.0, 0.0, 640.0, 480.0).expect("test bounds must be valid"),
                ),
                Authority::Unknown(AuthorityUnavailableReason::NotReported),
                Authority::Known(ScaleFactor::new(1.0).expect("test scale factor must be valid")),
                Authority::Known(ScaleFactor::new(1.0).expect("test scale factor must be valid")),
            ),
        );

        assert_eq!(
            snapshot_with_unknown_inventory(
                1,
                PlatformCapabilities::default(),
                vec![window],
                Vec::new(),
                unknown_work_areas(1),
            ),
            Err(
                PlatformSnapshotError::CoordinateObservationBindingMismatch {
                    window: current,
                    observation: delayed,
                }
            )
        );
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
            snapshot_with_unknown_inventory(
                1,
                supported.clone(),
                Vec::new(),
                Vec::new(),
                WorkAreaRosterObservation::new(
                    WorkAreaObservationGeneration::new(1),
                    Authority::Known(Vec::new()),
                )
                .expect("empty roster is structurally unambiguous"),
            ),
            Err(PlatformSnapshotError::MissingWorkAreaRoster)
        );
        assert_eq!(
            snapshot_with_unknown_inventory(
                2,
                PlatformCapabilities::default(),
                Vec::new(),
                Vec::new(),
                WorkAreaRosterObservation::new(
                    WorkAreaObservationGeneration::new(2),
                    Authority::Known(vec![work_area]),
                )
                .expect("test roster must be structurally valid"),
            ),
            Err(PlatformSnapshotError::WorkAreaRosterWithoutCapability)
        );
        assert!(matches!(
            WorkAreaRosterObservation::new(
                WorkAreaObservationGeneration::new(3),
                Authority::Known(vec![work_area, work_area]),
            ),
            Err(PlatformSnapshotError::DuplicateWorkArea { .. })
        ));
        let empty = PhysicalRect::new(0.0, 0.0, 0.0, 1080.0)
            .expect("zero-width geometry remains representable outside work-area rosters");
        assert_eq!(
            WorkAreaRosterObservation::new(
                WorkAreaObservationGeneration::new(4),
                Authority::Known(vec![ObservedWorkArea::new(
                    WorkAreaToken::new(4),
                    empty,
                    scale,
                )]),
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
        let window = ObservedWindow::new(binding)
            .with_input_observation(WindowInputObservation::new(
                binding,
                InputObservationGeneration::new(1),
                Authority::Known(WindowInputState::PassThrough),
                InputEffectAcknowledgement::known(None),
            ))
            .with_input_state(Authority::Known(WindowInputState::ReceivesInput));
        assert_eq!(
            snapshot_with_unknown_inventory(
                1,
                PlatformCapabilities::default(),
                vec![window],
                Vec::new(),
                unknown_work_areas(1),
            ),
            Err(PlatformSnapshotError::InputObservationStateMismatch { binding })
        );
    }

    #[test]
    fn presentation_observation_stream_requires_an_explicit_versioned_tombstone() {
        let current_binding = binding(1);
        let mut stream = WindowPresentationObservationStream::default();
        let known = |generation, state| {
            WindowPresentationObservation::new(
                current_binding,
                PresentationObservationGeneration::new(generation),
                Authority::Known(state),
                PresentationEffectAcknowledgement::known(None),
            )
        };
        let unknown = |generation| {
            WindowPresentationObservation::new(
                current_binding,
                PresentationObservationGeneration::new(generation),
                Authority::Unknown(AuthorityUnavailableReason::NotReported),
                PresentationEffectAcknowledgement::unknown(AuthorityUnavailableReason::NotReported),
            )
        };

        stream.observe(
            current_binding,
            Some(known(1, WindowPresentationState::Visible)),
        );
        assert_eq!(
            stream
                .current()
                .and_then(WindowPresentationObservation::known_state),
            Some(WindowPresentationState::Visible)
        );

        // An unversioned gap revokes authority. A later known state cannot silently
        // bridge that gap; the provider must publish an explicit versioned tombstone.
        stream.observe(current_binding, None);
        stream.observe(
            current_binding,
            Some(known(2, WindowPresentationState::Hidden)),
        );
        assert_eq!(stream.current(), None);
        stream.observe(current_binding, Some(unknown(3)));
        stream.observe(
            current_binding,
            Some(known(4, WindowPresentationState::Visible)),
        );
        assert_eq!(
            stream
                .current()
                .and_then(WindowPresentationObservation::known_state),
            Some(WindowPresentationState::Visible)
        );

        // A same-generation conflict also requires a tombstone before recovery.
        stream.observe(
            current_binding,
            Some(known(4, WindowPresentationState::Hidden)),
        );
        stream.observe(
            current_binding,
            Some(known(5, WindowPresentationState::Hidden)),
        );
        assert_eq!(stream.current(), None);
        stream.observe(current_binding, Some(unknown(6)));
        stream.observe(
            current_binding,
            Some(known(7, WindowPresentationState::Hidden)),
        );
        assert_eq!(
            stream
                .current()
                .and_then(WindowPresentationObservation::known_state),
            Some(WindowPresentationState::Hidden)
        );
    }

    #[test]
    fn repeated_presentation_envelope_keeps_its_first_inventory_ingress() {
        let binding = binding(1);
        let mut stream = WindowPresentationObservationStream::default();
        let observation = WindowPresentationObservation::new(
            binding,
            PresentationObservationGeneration::new(4),
            Authority::Known(WindowPresentationState::Visible),
            PresentationEffectAcknowledgement::known(None),
        );

        stream.observe(
            binding,
            Some(observation.with_inventory_generation(InventoryGeneration::new(7))),
        );
        stream.observe(
            binding,
            Some(observation.with_inventory_generation(InventoryGeneration::new(8))),
        );

        let current = stream
            .current()
            .expect("an exact provider duplicate must preserve authority");
        assert_eq!(
            current.known_state(),
            Some(WindowPresentationState::Visible)
        );
        assert_eq!(
            current.inventory_generation(),
            InventoryGeneration::new(7),
            "repeating an old provider envelope cannot make it causally newer"
        );
    }

    #[test]
    fn close_observation_stream_requires_tombstones_and_destroyed_is_terminal() {
        let current_binding = binding(1);
        let mut stream = WindowCloseObservationStream::default();
        let known = |binding, generation, state, effect| {
            WindowCloseObservation::new(
                binding,
                CloseObservationGeneration::new(generation),
                Authority::Known(state),
                CloseEffectAcknowledgement::known(effect),
            )
        };
        let unknown = |generation| {
            WindowCloseObservation::new(
                current_binding,
                CloseObservationGeneration::new(generation),
                Authority::Unknown(AuthorityUnavailableReason::NotReported),
                CloseEffectAcknowledgement::unknown(AuthorityUnavailableReason::NotReported),
            )
        };

        stream.observe(
            current_binding,
            Some(known(
                current_binding,
                4,
                WindowCloseState::LiveRequested,
                None,
            )),
        );
        assert_eq!(
            stream
                .current()
                .and_then(WindowCloseObservation::known_state),
            Some(WindowCloseState::LiveRequested)
        );

        stream.observe(current_binding, None);
        stream.observe(
            current_binding,
            Some(known(
                current_binding,
                5,
                WindowCloseState::LiveClear,
                Some(EffectId::new(10)),
            )),
        );
        assert_eq!(stream.current(), None);
        stream.observe(current_binding, Some(unknown(6)));
        stream.observe(
            current_binding,
            Some(known(
                current_binding,
                7,
                WindowCloseState::LiveClear,
                Some(EffectId::new(10)),
            )),
        );
        assert_eq!(
            stream
                .current()
                .and_then(WindowCloseObservation::known_state),
            Some(WindowCloseState::LiveClear)
        );

        stream.observe(
            current_binding,
            Some(known(
                current_binding,
                7,
                WindowCloseState::LiveRequested,
                Some(EffectId::new(10)),
            )),
        );
        assert_eq!(stream.current(), None);
        stream.observe(
            current_binding,
            Some(known(
                binding(2),
                8,
                WindowCloseState::LiveClear,
                Some(EffectId::new(10)),
            )),
        );
        assert_eq!(stream.current(), None);

        stream.observe(
            current_binding,
            Some(
                known(
                    current_binding,
                    9,
                    WindowCloseState::Destroyed,
                    Some(EffectId::new(11)),
                )
                .with_inventory_generation(InventoryGeneration::new(20)),
            ),
        );
        let destroyed = stream
            .current()
            .expect("an exact destroyed observation terminates the binding stream");
        assert_eq!(destroyed.known_state(), Some(WindowCloseState::Destroyed));
        assert_eq!(
            destroyed.inventory_generation(),
            InventoryGeneration::new(20)
        );
        stream.observe(
            current_binding,
            Some(known(
                current_binding,
                10,
                WindowCloseState::LiveClear,
                Some(EffectId::new(12)),
            )),
        );
        assert_eq!(stream.current(), Some(destroyed));
    }

    #[test]
    fn snapshot_rejects_ambiguous_close_observation_rosters() {
        let binding = binding(1);
        let live = WindowCloseObservation::new(
            binding,
            CloseObservationGeneration::new(1),
            Authority::Known(WindowCloseState::LiveClear),
            CloseEffectAcknowledgement::known(None),
        );
        let destroyed = WindowCloseObservation::new(
            binding,
            CloseObservationGeneration::new(2),
            Authority::Known(WindowCloseState::Destroyed),
            CloseEffectAcknowledgement::known(Some(EffectId::new(1))),
        );

        assert_eq!(
            snapshot_with_unknown_inventory(
                1,
                PlatformCapabilities::default(),
                Vec::new(),
                vec![live],
                unknown_work_areas(1),
            ),
            Err(PlatformSnapshotError::LiveCloseObservationWithoutWindow { binding })
        );
        assert_eq!(
            snapshot_with_unknown_inventory(
                2,
                PlatformCapabilities::default(),
                vec![ObservedWindow::new(binding)],
                vec![destroyed],
                unknown_work_areas(2),
            ),
            Err(PlatformSnapshotError::DestroyedCloseObservationWithWindow { binding })
        );
        assert_eq!(
            snapshot_with_unknown_inventory(
                3,
                PlatformCapabilities::default(),
                vec![ObservedWindow::new(binding)],
                vec![live, live],
                unknown_work_areas(3),
            ),
            Err(PlatformSnapshotError::DuplicateCloseObservation { binding })
        );
    }
}

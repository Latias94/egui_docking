//! Complete platform capability and native-window observations.
//!
//! Adapters submit facts and execute effects. They never submit operating-system
//! handles or infer a docking target from window geometry.

use std::mem::size_of;

use thiserror::Error;

use crate::effect::EffectId;
use crate::geometry::{PhysicalRect, PhysicalSize, ScaleFactor};
use crate::intent::{Authority, AuthorityUnavailableReason};
use crate::viewport::{
    CapabilityObservationGeneration, CloseObservationGeneration, CoordinateObservationGeneration,
    InputObservationGeneration, InventoryGeneration, InventoryObservationGeneration,
    PlatformSnapshotGeneration, PresentationObservationGeneration, ViewportBinding, WindowBinding,
    WindowToken, WorkAreaObservationGeneration, WorkAreaToken,
};
use crate::viewport_focus::FocusObservationEnvelope;

/// Host-observed creation and renderer preparation for one exact binding.
///
/// Preparation is a current fact, independent of accepting the create dispatch.
/// Hosts may report Prepared as their first observation. They must withdraw it
/// when renderer resources, visibility, or the drawable coordinate envelope become
/// invalid, then report Prepared again only after native preparation completes.
/// Prepared never transfers logical content ownership by itself: the core also
/// requires create causality and the exact semantic first-live presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeCreateReadiness {
    /// The platform window exists; renderer preparation is not yet established.
    Created,
    /// The window and renderer resources can accept a complete semantic output.
    Prepared,
    /// Renderer preparation failed after creation; ordinary cleanup owns the window.
    Failed,
}

/// Preparation facts use the existing per-window observation sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WindowCreatePreparationObservation {
    binding: ViewportBinding,
    generation: CoordinateObservationGeneration,
    inventory_generation: InventoryGeneration,
    readiness: Authority<NativeCreateReadiness>,
    acknowledged_create: Option<EffectId>,
}

impl WindowCreatePreparationObservation {
    pub(crate) const fn new(
        binding: ViewportBinding,
        generation: CoordinateObservationGeneration,
        readiness: Authority<NativeCreateReadiness>,
        acknowledged_create: Option<EffectId>,
    ) -> Self {
        Self {
            binding,
            generation,
            inventory_generation: InventoryGeneration::new(0),
            readiness,
            acknowledged_create,
        }
    }

    pub(crate) const fn with_inventory_generation(
        mut self,
        generation: InventoryGeneration,
    ) -> Self {
        self.inventory_generation = generation;
        self
    }

    pub(crate) const fn binding(self) -> ViewportBinding {
        self.binding
    }
    pub(crate) const fn generation(self) -> CoordinateObservationGeneration {
        self.generation
    }
    pub(crate) const fn inventory_generation(self) -> InventoryGeneration {
        self.inventory_generation
    }
    pub(crate) const fn acknowledged_create(self) -> Option<EffectId> {
        self.acknowledged_create
    }
    pub(crate) const fn known_readiness(self) -> Option<NativeCreateReadiness> {
        match self.readiness {
            Authority::Known(value) => Some(value),
            Authority::Unknown(_) => None,
        }
    }

    fn same_provider_envelope(self, other: Self) -> bool {
        self.binding.window() == other.binding.window()
            && self.generation == other.generation
            && self.readiness == other.readiness
            && self.acknowledged_create == other.acknowledged_create
    }
}

/// Preparation stays in the exact binding record, with no independent sequence issuer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct WindowCreatePreparationObservationStream {
    current: Option<WindowCreatePreparationObservation>,
    last_generation: Option<CoordinateObservationGeneration>,
    last_envelope: Option<WindowCreatePreparationObservation>,
    requires_tombstone: bool,
    prepared_since: Option<CoordinateObservationGeneration>,
}

impl WindowCreatePreparationObservationStream {
    pub(crate) fn observe(
        &mut self,
        binding: impl Into<WindowBinding>,
        observation: Option<WindowCreatePreparationObservation>,
    ) {
        let binding = binding.into();
        let Some(observation) =
            observation.filter(|observation| observation.binding().window() == binding)
        else {
            self.requires_tombstone |= self.current.is_some();
            self.current = None;
            self.prepared_since = None;
            return;
        };
        match self.last_generation {
            Some(generation) if observation.generation() < generation => return,
            Some(generation) if observation.generation() == generation => {
                if self
                    .last_envelope
                    .is_none_or(|last| !last.same_provider_envelope(observation))
                {
                    self.current = None;
                    self.prepared_since = None;
                    self.requires_tombstone = true;
                }
                return;
            }
            Some(generation) if generation.checked_next() != Some(observation.generation()) => {
                self.last_generation = Some(observation.generation());
                self.last_envelope = Some(observation);
                self.current = None;
                self.prepared_since = None;
                self.requires_tombstone = true;
                return;
            }
            Some(_) | None => {}
        }
        self.last_generation = Some(observation.generation());
        self.last_envelope = Some(observation);
        if self.requires_tombstone && observation.known_readiness().is_some() {
            self.current = None;
            self.prepared_since = None;
            return;
        }
        self.requires_tombstone = false;
        if observation.known_readiness() == Some(NativeCreateReadiness::Prepared) {
            self.prepared_since.get_or_insert(observation.generation());
        } else {
            self.prepared_since = None;
        }
        self.current = Some(observation);
    }

    pub(crate) const fn current(self) -> Option<WindowCreatePreparationObservation> {
        self.current
    }
    pub(crate) const fn prepared_since(self) -> Option<CoordinateObservationGeneration> {
        self.prepared_since
    }
}

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

/// Required treatment of a physical drag's source native window.
///
/// Core derives this from the source ownership and complete payload. A native
/// continuation may expose the frozen requirement; this value alone supplies
/// no input, capture, delivery or pass-through acknowledgement authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeDragSourceInput {
    /// Keep the source window's native hit regions for ordinary content dragging.
    Preserve,
    /// Exclude the complete owned source window and restore it at gesture end.
    PassThrough,
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
            #[allow(dead_code, reason = "capability fields retain symmetric diagnostic getters and setters")]
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
    /// desktop position, button/capture authority and rendered hit observations. Only a source
    /// which must exclude its whole window additionally requires pointer pass-through control.
    /// Core selects the source treatment from exact ownership and the dragged payload; choosing
    /// a query parameter does not grant a route or authorize a host to override that decision.
    #[must_use]
    pub fn physical_cross_surface_drag(
        &self,
        source_input: NativeDragSourceInput,
    ) -> PlatformCapability {
        combine_required([
            self.cross_surface_routing(),
            self.authoritative_release(),
            match source_input {
                NativeDragSourceInput::Preserve => PlatformCapability::Supported,
                NativeDragSourceInput::PassThrough => self.pointer_hit_test_control,
            },
        ])
    }

    /// Returns whether a physical outside-all drag can create a native child safely.
    ///
    /// This composes the complete cross-surface interaction contract with the independent
    /// native lifecycle, exact placement, and work-area contract. Programmatic native creation
    /// continues to use [`Self::native_exact_placement_create`] and does not require pointer
    /// capabilities.
    #[must_use]
    pub fn physical_native_drag(&self, source_input: NativeDragSourceInput) -> PlatformCapability {
        combine_required([
            self.physical_cross_surface_drag(source_input),
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
    #[cfg(test)]
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
    #[cfg(test)]
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
    #[cfg(test)]
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
    #[cfg(test)]
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
    #[cfg(test)]
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
    fn same_provider_envelope(self, other: Self) -> bool {
        self.binding.window() == other.binding.window()
            && self.generation == other.generation
            && self.state == other.state
            && self.acknowledged_effect == other.acknowledged_effect
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
    #[cfg(test)]
    pub(crate) fn reset_for_provider_replacement(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn observe(
        &mut self,
        binding: impl Into<WindowBinding>,
        observation: Option<WindowInputObservation>,
    ) {
        let binding = binding.into();
        let Some(observation) = observation else {
            self.current = None;
            self.requires_tombstone |= self.last_generation.is_some();
            return;
        };
        if observation.binding().window() != binding {
            self.current = None;
            self.requires_tombstone |= self.last_generation.is_some();
            return;
        }
        match self.last_generation {
            Some(generation) if observation.generation() < generation => return,
            Some(generation) if observation.generation() == generation => {
                if self
                    .last_envelope
                    .is_none_or(|last| !last.same_provider_envelope(observation))
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

/// Binding-scoped, causally versioned presentation observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowPresentationObservation {
    binding: ViewportBinding,
    generation: PresentationObservationGeneration,
    inventory_generation: InventoryGeneration,
    state: Authority<WindowPresentationState>,
}

impl WindowPresentationObservation {
    #[must_use]
    pub const fn new(
        binding: ViewportBinding,
        generation: PresentationObservationGeneration,
        state: Authority<WindowPresentationState>,
    ) -> Self {
        Self {
            binding,
            generation,
            inventory_generation: InventoryGeneration::new(0),
            state,
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

    fn same_provider_envelope(self, other: Self) -> bool {
        self.binding.window() == other.binding.window()
            && self.generation == other.generation
            && self.state == other.state
    }
}

/// Monotonic per-binding presentation observations with explicit conflict recovery.
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
        binding: impl Into<WindowBinding>,
        observation: Option<WindowPresentationObservation>,
    ) {
        let binding = binding.into();
        let Some(observation) = observation else {
            self.current = None;
            self.requires_tombstone |= self.last_generation.is_some();
            return;
        };
        if observation.binding().window() != binding {
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
/// Content size, native scale, and presentation scale jointly authorize local coordinates.
/// Desktop conversion additionally requires content bounds with a real desktop origin.
/// The native scale describes operating-system logical units. The presentation
/// scale describes the surface-local coordinate system actually presented by the UI adapter.
/// Keeping them separate prevents application zoom from being mistaken for a DPI change.
/// Content and outer bounds remain independently optional for local coordinate authority.
/// An unavailable required fact revokes local authority. Recovering an invalidated stream
/// requires a versioned tombstone before a complete local tuple can become authoritative.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowCoordinateObservation {
    binding: ViewportBinding,
    generation: CoordinateObservationGeneration,
    content_size: Authority<PhysicalSize>,
    content_bounds: Authority<PhysicalRect>,
    outer_bounds: Authority<PhysicalRect>,
    native_scale_factor: Authority<ScaleFactor>,
    presentation_scale_factor: Authority<ScaleFactor>,
}

impl WindowCoordinateObservation {
    #[must_use]
    pub fn new(
        binding: ViewportBinding,
        generation: CoordinateObservationGeneration,
        content_bounds: Authority<PhysicalRect>,
        outer_bounds: Authority<PhysicalRect>,
        native_scale_factor: Authority<ScaleFactor>,
        presentation_scale_factor: Authority<ScaleFactor>,
    ) -> Self {
        let content_size = match content_bounds {
            Authority::Known(bounds) => Authority::Known(bounds.size()),
            Authority::Unknown(reason) => Authority::Unknown(reason),
        };
        Self {
            binding,
            generation,
            content_size,
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

    /// Supplies content size independently of desktop-position authority.
    #[must_use]
    pub const fn with_content_size(mut self, content_size: Authority<PhysicalSize>) -> Self {
        self.content_size = content_size;
        self
    }

    /// Returns the physical content size, independent of its desktop origin.
    #[must_use]
    pub const fn content_size(&self) -> &Authority<PhysicalSize> {
        &self.content_size
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
        matches!(self.content_size, Authority::Known(_))
            && matches!(self.native_scale_factor, Authority::Known(_))
            && matches!(self.presentation_scale_factor, Authority::Known(_))
    }
    fn same_provider_envelope(self, other: Self) -> bool {
        self.binding.window() == other.binding.window()
            && self.generation == other.generation
            && self.content_size == other.content_size
            && self.content_bounds == other.content_bounds
            && self.outer_bounds == other.outer_bounds
            && self.native_scale_factor == other.native_scale_factor
            && self.presentation_scale_factor == other.presentation_scale_factor
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
        binding: impl Into<WindowBinding>,
        observation: Option<WindowCoordinateObservation>,
    ) {
        let binding = binding.into();
        let Some(observation) = observation else {
            self.current = None;
            self.requires_tombstone |= self.last_generation.is_some();
            return;
        };
        if observation.binding().window() != binding {
            self.current = None;
            self.requires_tombstone |= self.last_generation.is_some();
            return;
        }
        match self.last_generation {
            Some(generation) if observation.generation() < generation => return,
            Some(generation) if observation.generation() == generation => {
                if self
                    .last_envelope
                    .is_none_or(|last| !last.same_provider_envelope(observation))
                {
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
    #[cfg(test)]
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
        self.binding.window() == other.binding.window()
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
    #[cfg(test)]
    pub(crate) fn reset_for_provider_replacement(&mut self) {
        debug_assert!(
            !self.destroyed,
            "destroyed close streams must leave the active lifecycle before provider replacement"
        );
        *self = Self::default();
    }

    pub(crate) fn observe(
        &mut self,
        binding: impl Into<WindowBinding>,
        observation: Option<WindowCloseObservation>,
    ) {
        let binding = binding.into();
        if self.destroyed {
            return;
        }
        let Some(observation) = observation else {
            self.current = None;
            self.requires_tombstone |= self.last_generation.is_some();
            return;
        };
        if observation.binding().window() != binding {
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
    create_preparation: Option<WindowCreatePreparationObservation>,
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
            create_preparation: None,
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

    pub(crate) const fn create_preparation(&self) -> Option<WindowCreatePreparationObservation> {
        self.create_preparation
    }

    pub(crate) fn with_create_preparation(
        mut self,
        value: WindowCreatePreparationObservation,
    ) -> Self {
        self.create_preparation = Some(value);
        self
    }

    #[must_use]
    #[cfg(test)]
    pub const fn close_requested(&self) -> &Authority<bool> {
        &self.close_requested
    }

    #[must_use]
    pub fn with_coordinate_observation(mut self, value: WindowCoordinateObservation) -> Self {
        self.coordinate_observation = Some(value);
        self
    }

    #[must_use]
    #[cfg(test)]
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
    #[cfg(test)]
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
            work_areas.sort_unstable_by_key(|work_area| work_area.token);
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
    #[cfg(test)]
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

    #[cfg(test)]
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
    /// Returns the owned vector capacity, excluding inline snapshot storage.
    pub(crate) fn owned_payload_capacity_bytes(&self) -> usize {
        let inventory = match self.inventory_observation.roster() {
            Authority::Known(bindings) => bindings
                .capacity()
                .saturating_mul(size_of::<ViewportBinding>()),
            Authority::Unknown(_) => 0,
        };
        let work_areas = match self.work_area_observation.roster() {
            Authority::Known(work_areas) => work_areas
                .capacity()
                .saturating_mul(size_of::<ObservedWorkArea>()),
            Authority::Unknown(_) => 0,
        };
        self.window_observations
            .capacity()
            .saturating_mul(size_of::<ObservedWindow>())
            .saturating_add(
                self.close_observations
                    .capacity()
                    .saturating_mul(size_of::<WindowCloseObservation>()),
            )
            .saturating_add(inventory)
            .saturating_add(work_areas)
    }

    /// Creates a canonical complete snapshot and rejects ambiguous identities.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformSnapshotError`] for duplicate identities, a mismatched complete
    /// inventory roster, conflicting coordinate size and bounds, or a work-area roster
    /// that contradicts the declared capability.
    pub fn new(
        generation: PlatformSnapshotGeneration,
        capability_observation: CapabilityRosterObservation,
        focus: FocusObservationEnvelope,
        inventory_observation: WindowInventoryObservation,
        mut window_observations: Vec<ObservedWindow>,
        mut close_observations: Vec<WindowCloseObservation>,
        work_area_observation: WorkAreaRosterObservation,
    ) -> Result<Self, PlatformSnapshotError> {
        window_observations.sort_unstable_by_key(ObservedWindow::binding);
        reject_duplicate_windows(&window_observations)?;
        validate_focus_window(focus, &window_observations)?;
        validate_input_observations(&window_observations)?;
        validate_inventory_observation(
            &capability_observation,
            &inventory_observation,
            &window_observations,
        )?;
        close_observations.sort_unstable_by_key(|observation| observation.binding());
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
    // Both constructors sort by exact binding and reject duplicates first.
    if !windows
        .iter()
        .map(ObservedWindow::binding)
        .eq(inventory.iter().copied())
    {
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
    if windows
        .iter()
        .any(|window| window.binding().window() == *binding)
    {
        Ok(())
    } else {
        Err(PlatformSnapshotError::FocusWindowWithoutObservation { binding: *binding })
    }
}

fn validate_work_areas(work_areas: &[ObservedWorkArea]) -> Result<(), PlatformSnapshotError> {
    for (index, work_area) in work_areas.iter().enumerate() {
        if index != 0 && work_areas[index - 1].token == work_area.token {
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
    for (index, window) in windows.iter().enumerate() {
        let binding = window.binding();
        if index != 0 && windows[index - 1].binding() == binding {
            return Err(PlatformSnapshotError::DuplicateWindowBinding { binding });
        }
        // Window tokens are not the primary sort key. This infrequent complete
        // roster check avoids allocating another identity index during capture.
        if windows[..index]
            .iter()
            .any(|previous| previous.binding().token() == binding.token())
        {
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
        if let Some(observation) = window.coordinate_observation() {
            if observation.binding() != binding {
                return Err(
                    PlatformSnapshotError::CoordinateObservationBindingMismatch {
                        window: binding,
                        observation: observation.binding(),
                    },
                );
            }
            if let (Authority::Known(size), Authority::Known(bounds)) =
                (observation.content_size(), observation.content_bounds())
                && *size != bounds.size()
            {
                return Err(PlatformSnapshotError::CoordinateObservationSizeMismatch { binding });
            }
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
        if let Some(preparation) = window.create_preparation() {
            if preparation.binding() != binding {
                return Err(PlatformSnapshotError::CreatePreparationBindingMismatch { binding });
            }
            if window
                .coordinate_observation()
                .is_none_or(|coordinates| coordinates.generation() != preparation.generation())
            {
                return Err(PlatformSnapshotError::CreatePreparationCoordinateMismatch { binding });
            }
        }
    }
    Ok(())
}

fn validate_close_observations(
    windows: &[ObservedWindow],
    observations: &[WindowCloseObservation],
) -> Result<(), PlatformSnapshotError> {
    for (index, observation) in observations.iter().enumerate() {
        let binding = observation.binding();
        if index != 0 && observations[index - 1].binding() == binding {
            return Err(PlatformSnapshotError::DuplicateCloseObservation { binding });
        }
        let has_window = windows
            .binary_search_by_key(&binding, ObservedWindow::binding)
            .is_ok();
        match observation.known_state() {
            Some(WindowCloseState::LiveClear | WindowCloseState::LiveRequested) if !has_window => {
                return Err(PlatformSnapshotError::LiveCloseObservationWithoutWindow { binding });
            }
            Some(WindowCloseState::Destroyed) if has_window => {
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
    #[error("native create preparation does not belong to window {binding:?}")]
    CreatePreparationBindingMismatch { binding: ViewportBinding },
    #[error("native create preparation lacks its exact coordinate observation for {binding:?}")]
    CreatePreparationCoordinateMismatch { binding: ViewportBinding },
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
    #[error("window {binding:?} carries conflicting coordinate content size and bounds")]
    CoordinateObservationSizeMismatch { binding: ViewportBinding },
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
    FocusWindowWithoutObservation { binding: WindowBinding },
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
mod tests;

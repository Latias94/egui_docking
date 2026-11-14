//! Core-owned registry for stable logical surfaces and ephemeral native bindings.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::close::plan::NativeCloseEdge;
#[cfg(test)]
use crate::geometry::PhysicalPoint;
use crate::geometry::{LogicalRect, LogicalSize};
use crate::ids::{EngineAuthorityDomainId, SurfaceId, WorkspaceEpoch};
use crate::input::intent::Authority;
#[cfg(test)]
use crate::platform::ObservedWorkArea;
#[cfg(test)]
use crate::platform::coordinates::CoordinateUnavailable;
use crate::platform::coordinates::NativeSurfacePlacement;
use crate::platform::coordinates::{
    CoordinateSnapshot, RecoveryCoordinateSnapshot, SurfaceClientGeometry, ViewportPlacementProof,
    WindowCoordinateSnapshot,
};
#[cfg(test)]
use crate::platform::viewport::CloseObservationGeneration;
use crate::platform::viewport::{
    CoordinateGeneration, CoordinateObservationGeneration, InputObservationGeneration,
    InventoryGeneration, PlacementGeneration, ViewportBinding, ViewportRole, WindowBinding,
    WindowIncarnation, WindowToken, WorkAreaGeneration,
};
use crate::platform::{
    ObservedWindow, PlatformSnapshot, WindowCloseObservation, WindowCloseObservationStream,
    WindowCloseState, WindowCoordinateObservationStream, WindowCreatePreparationObservation,
    WindowCreatePreparationObservationStream, WindowInputObservation, WindowInputObservationStream,
    WindowPresentationObservation, WindowPresentationObservationStream, WindowPresentationState,
};

/// Lifecycle state derived from authoritative inventory, never from callback timing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewportLifecycle {
    AwaitingObservation,
    Ready,
    CloseRequested,
    AwaitingDestroyed,
    /// The binding is absent from an authoritative live-window inventory.
    Missing,
    /// The provider authoritatively terminated this exact binding incarnation.
    Destroyed,
}

/// Ownership of a native binding's platform resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ViewportOwnership {
    /// A window supplied and owned by the host application.
    External,
    /// A window allocated by the docking runtime.
    RuntimeOwned,
}

/// Admission of a binding into routing and focus semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ViewportAdmission {
    /// Reserved or otherwise not yet proven usable.
    Pending,
    /// Visible-proof and topology obligations have admitted this binding.
    Admitted,
    /// Destruction has begun; no new routing/focus may be granted.
    Retiring,
}

/// Typed relationship between one frozen native close edge and current registry facts.
///
/// This is a fact query, not a lifecycle decision. In particular, `LiveClear`, a missing
/// inventory entry, and unavailable authority are intentionally distinct so the engine never
/// infers a cancellation or destruction from an absence of callbacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeCloseEdgeDisposition {
    /// The exact binding, provider generation, and ingress generation remain pending.
    ExactPending,
    /// A different live close edge is pending on the same binding.
    DifferentPending { observation: WindowCloseObservation },
    /// The provider explicitly cleared close state for this binding.
    LiveClear { observation: WindowCloseObservation },
    /// The provider authoritatively destroyed this exact binding incarnation.
    Destroyed { observation: WindowCloseObservation },
    /// The binding is absent from an authoritative live-window inventory, without destruction.
    InventoryMissing,
    /// The logical surface no longer has a registry record.
    BindingMissing,
    /// The logical surface is bound to a different native window incarnation.
    BindingRebound { binding: ViewportBinding },
    /// No current authoritative close fact can prove any stronger relationship.
    AuthorityUnavailable,
}

/// Window-owned facts, stored once for each exact native incarnation.
#[derive(Debug, Clone, PartialEq)]
struct WindowRecord {
    observation_binding: ViewportBinding,
    role: ViewportRole,
    ownership: ViewportOwnership,
    lifecycle: ViewportLifecycle,
    coordinate_observations: WindowCoordinateObservationStream,
    input_observations: WindowInputObservationStream,
    presentation_observations: WindowPresentationObservationStream,
    create_preparation_observations: WindowCreatePreparationObservationStream,
    close_observations: WindowCloseObservationStream,
    ever_observed: bool,
    pending_close_request: Option<WindowCloseObservation>,
    coordinates: Option<WindowCoordinateSnapshot>,
    coordinate_generation: CoordinateGeneration,
}

/// Surface-local admission and projection authority for one native placement.
#[derive(Debug, Clone, PartialEq)]
struct SurfacePlacementRecord {
    binding: ViewportBinding,
    admission: ViewportAdmission,
    coordinates: Option<CoordinateSnapshot>,
    recovery_coordinates: Option<RecoveryCoordinateSnapshot>,
    coordinate_generation: CoordinateGeneration,
    placement: Option<NativeSurfacePlacement>,
}

/// Borrowed physical facts for one exact real-window lifetime.
#[derive(Debug, Clone, Copy)]
pub(crate) struct WindowRecordView<'registry> {
    binding: WindowBinding,
    record: &'registry WindowRecord,
}

impl WindowRecordView<'_> {
    pub(crate) const fn observation_binding(self) -> ViewportBinding {
        self.record.observation_binding
    }
    pub(crate) const fn binding(self) -> WindowBinding {
        self.binding
    }
    pub(crate) const fn role(self) -> ViewportRole {
        self.record.role
    }
    pub(crate) const fn ownership(self) -> ViewportOwnership {
        self.record.ownership
    }
    pub(crate) const fn lifecycle(self) -> ViewportLifecycle {
        self.record.lifecycle
    }
    pub(crate) const fn coordinates(self) -> Option<WindowCoordinateSnapshot> {
        self.record.coordinates
    }
    pub(crate) const fn ever_observed(self) -> bool {
        self.record.ever_observed
    }
    pub(crate) fn is_observed(self) -> bool {
        self.record.ever_observed
            && !matches!(
                self.record.lifecycle,
                ViewportLifecycle::Missing | ViewportLifecycle::Destroyed
            )
    }
    pub(crate) fn is_routeable(self) -> bool {
        self.record.lifecycle == ViewportLifecycle::Ready
            && window_has_coordinate_authority(self.record)
    }
    pub(crate) fn can_report_focus_fact(self) -> bool {
        self.record.ever_observed
            && matches!(
                self.record.lifecycle,
                ViewportLifecycle::AwaitingObservation
                    | ViewportLifecycle::Ready
                    | ViewportLifecycle::CloseRequested
            )
            && self
                .record
                .presentation_observations
                .current()
                .and_then(WindowPresentationObservation::known_state)
                == Some(WindowPresentationState::Visible)
    }
    pub(crate) const fn input_observation(self) -> Option<WindowInputObservation> {
        self.record.input_observations.current()
    }
    pub(crate) const fn input_observation_generation_watermark(
        self,
    ) -> Option<InputObservationGeneration> {
        self.record.input_observations.generation_watermark()
    }
}

/// Borrowed query joining one surface placement to its window-owned facts.
///
/// Copying this view copies references, never a second lifecycle owner. The
/// registry alone mutates either record inside its publication candidate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportRecord<'registry> {
    placement: &'registry SurfacePlacementRecord,
    window: &'registry WindowRecord,
}

impl ViewportRecord<'_> {
    #[must_use]
    pub const fn binding(self) -> ViewportBinding {
        self.placement.binding
    }

    #[must_use]
    pub const fn role(self) -> ViewportRole {
        self.window.role
    }

    #[must_use]
    pub const fn ownership(self) -> ViewportOwnership {
        self.window.ownership
    }

    #[must_use]
    pub const fn admission(self) -> ViewportAdmission {
        self.placement.admission
    }

    #[must_use]
    pub const fn lifecycle(self) -> ViewportLifecycle {
        self.window.lifecycle
    }

    #[must_use]
    pub const fn is_ready(self) -> bool {
        matches!(self.window.lifecycle, ViewportLifecycle::Ready)
    }

    /// Returns whether this live incarnation can still authorize its coordinate snapshot.
    ///
    /// A close-request edge disables new pointer routing, but the observed window and its
    /// geometry remain authoritative until close acceptance advances to `AwaitingDestroyed`.
    pub(crate) const fn has_coordinate_authority(self) -> bool {
        matches!(
            self.window.lifecycle,
            ViewportLifecycle::Ready | ViewportLifecycle::CloseRequested
        ) && self.placement.coordinates.is_some()
            && matches!(
                self.window.presentation_observations.current(),
                Some(observation)
                    if matches!(
                        observation.state(),
                        Authority::Known(WindowPresentationState::Visible)
                    )
            )
    }

    /// Returns whether the window is eligible for authoritative pointer routing.
    #[must_use]
    pub(crate) fn is_routeable(self) -> bool {
        self.placement.admission == ViewportAdmission::Admitted
            && self.is_ready()
            && self
                .window
                .presentation_observations
                .current()
                .is_some_and(|observation| {
                    observation.known_state() == Some(WindowPresentationState::Visible)
                })
    }

    /// Returns whether this exact live incarnation may report native focus.
    ///
    /// A visible staging child may be focused by the OS before its first live
    /// docking output is admitted. That fact is structurally valid, but must
    /// remain isolated from pane restoration and interaction until admission.
    pub(crate) fn can_report_focus_fact(self) -> bool {
        self.window.ever_observed
            && matches!(
                self.window.lifecycle,
                ViewportLifecycle::AwaitingObservation
                    | ViewportLifecycle::Ready
                    | ViewportLifecycle::CloseRequested
            )
            && self
                .window
                .presentation_observations
                .current()
                .is_some_and(|observation| {
                    observation.known_state() == Some(WindowPresentationState::Visible)
                })
    }

    /// Returns whether a reported focus fact may drive admitted docking semantics.
    ///
    /// Focus authority is independent of geometry authority: an admitted live
    /// window remains observable while coordinate facts are temporarily unavailable.
    pub(crate) fn can_observe_focus(self) -> bool {
        self.placement.admission == ViewportAdmission::Admitted && self.can_report_focus_fact()
    }

    /// Returns whether this exact incarnation may accept a new activation.
    ///
    /// A close-requested window may still report the focus it already owns,
    /// but new focus commands would race its terminal lifecycle.
    pub(crate) fn can_accept_activation(self) -> bool {
        self.placement.admission == ViewportAdmission::Admitted
            && self.window.ever_observed
            && matches!(
                self.window.lifecycle,
                ViewportLifecycle::AwaitingObservation | ViewportLifecycle::Ready
            )
            && self
                .window
                .presentation_observations
                .current()
                .is_some_and(|observation| {
                    observation.known_state() == Some(WindowPresentationState::Visible)
                })
    }

    pub(crate) const fn coordinates(self) -> Option<CoordinateSnapshot> {
        self.placement.coordinates
    }

    /// Returns the last exact-binding coordinates retained only for destruction recovery.
    ///
    /// This is historical placement data, not current route, hit-test, or scene authority.
    pub(crate) const fn recovery_coordinates(self) -> Option<RecoveryCoordinateSnapshot> {
        self.placement.recovery_coordinates
    }

    pub(crate) const fn coordinate_generation(self) -> CoordinateGeneration {
        self.placement.coordinate_generation
    }

    #[cfg(test)]
    pub(crate) const fn coordinate_observation_generation_watermark(
        self,
    ) -> Option<CoordinateObservationGeneration> {
        self.window.coordinate_observations.generation_watermark()
    }

    #[cfg(test)]
    pub(crate) const fn coordinate_observation_generation(
        self,
    ) -> Option<CoordinateObservationGeneration> {
        match self.placement.coordinates {
            Some(coordinates) => Some(coordinates.observation_generation()),
            None => None,
        }
    }

    #[cfg(test)]
    pub(crate) const fn input_observation(self) -> Option<WindowInputObservation> {
        self.window.input_observations.current()
    }

    pub(crate) const fn input_observations(self) -> WindowInputObservationStream {
        self.window.input_observations
    }

    #[cfg(test)]
    pub(crate) const fn input_observation_generation_watermark(
        self,
    ) -> Option<InputObservationGeneration> {
        self.window.input_observations.generation_watermark()
    }

    pub(crate) const fn presentation_observation(self) -> Option<WindowPresentationObservation> {
        self.window.presentation_observations.current()
    }

    pub(crate) const fn create_preparation(self) -> Option<WindowCreatePreparationObservation> {
        self.window.create_preparation_observations.current()
    }

    pub(crate) const fn renderer_prepared_since(self) -> Option<CoordinateObservationGeneration> {
        self.window.create_preparation_observations.prepared_since()
    }

    pub(crate) const fn close_observations(self) -> WindowCloseObservationStream {
        self.window.close_observations
    }

    pub(crate) const fn ever_observed(self) -> bool {
        self.window.ever_observed
    }

    /// Returns whether this record still owns the exact native close edge.
    ///
    /// Matching the binding alone is intentionally insufficient: a clear followed by a new
    /// request on the same native window is a different close obligation.
    #[cfg(test)]
    pub(crate) fn matches_pending_close_edge(self, edge: NativeCloseEdge) -> bool {
        self.placement.binding.window() == edge.binding().window()
            && self
                .window
                .pending_close_request
                .is_some_and(|observation| {
                    observation.binding().window() == edge.binding().window()
                        && observation.generation() == edge.observed_at()
                        && observation.inventory_generation() == edge.received_at()
                        && observation.known_state() == Some(WindowCloseState::LiveRequested)
                })
    }
}

/// One semantically relevant change from a complete inventory snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryEvent {
    Ready {
        binding: ViewportBinding,
    },
    PresentationChanged {
        binding: ViewportBinding,
    },
    FactsUnavailable {
        binding: ViewportBinding,
    },
    /// The binding is unavailable in a complete authoritative live-window roster.
    BindingMissing {
        binding: ViewportBinding,
    },
    CloseRequested {
        observation: WindowCloseObservation,
    },
    CloseRequestCleared {
        /// The exact pending request edge which this clear superseded.
        requested: WindowCloseObservation,
        /// The later authoritative live-and-clear observation.
        observation: WindowCloseObservation,
    },
    Destroyed {
        observation: WindowCloseObservation,
    },
}

/// Result of applying one complete inventory snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct RegistryTransition {
    generation: InventoryGeneration,
    native_window_placement_changed: bool,
    events: Vec<RegistryEvent>,
}

/// Exact binding facts atomically detached from the current registry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DetachedViewportFacts {
    binding: ViewportBinding,
    role: ViewportRole,
    ownership: ViewportOwnership,
    lifecycle: ViewportLifecycle,
    input_observations: WindowInputObservationStream,
    close_observations: WindowCloseObservationStream,
    ever_observed: bool,
    window_retired: bool,
}

impl DetachedViewportFacts {
    pub(crate) const fn window_retired(self) -> bool {
        self.window_retired
    }
    pub(crate) const fn binding(self) -> ViewportBinding {
        self.binding
    }

    pub(crate) const fn role(self) -> ViewportRole {
        self.role
    }

    pub(crate) const fn ownership(self) -> ViewportOwnership {
        self.ownership
    }

    pub(crate) const fn lifecycle(self) -> ViewportLifecycle {
        self.lifecycle
    }

    pub(crate) const fn input_observations(self) -> WindowInputObservationStream {
        self.input_observations
    }

    pub(crate) const fn close_observations(self) -> WindowCloseObservationStream {
        self.close_observations
    }

    pub(crate) const fn ever_observed(self) -> bool {
        self.ever_observed
    }
}

/// Atomic result of reconciling native bindings with a restored workspace.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RestoreRegistryReconciliation {
    retained: Vec<(ViewportBinding, ViewportBinding)>,
    retired: Vec<DetachedViewportFacts>,
}

impl RestoreRegistryReconciliation {
    pub(crate) fn retained(&self) -> &[(ViewportBinding, ViewportBinding)] {
        &self.retained
    }

    pub(crate) fn retired(&self) -> &[DetachedViewportFacts] {
        &self.retired
    }
}

impl RegistryTransition {
    pub(crate) const fn native_window_placement_changed(&self) -> bool {
        self.native_window_placement_changed
    }

    #[must_use]
    #[cfg(test)]
    pub const fn generation(&self) -> InventoryGeneration {
        self.generation
    }

    #[must_use]
    pub fn events(&self) -> &[RegistryEvent] {
        &self.events
    }
}

/// Binding registry with non-wrapping token, incarnation, and inventory identities.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewportRegistry {
    authority_domain: EngineAuthorityDomainId,
    records: BTreeMap<SurfaceId, SurfacePlacementRecord>,
    windows: BTreeMap<WindowBinding, WindowRecord>,
    token_to_window: BTreeMap<WindowToken, WindowBinding>,
    last_token: WindowToken,
    last_incarnation: WindowIncarnation,
    native_binding_allocation_budget: Option<usize>,
    inventory_generation: InventoryGeneration,
    last_coordinate_generation: CoordinateGeneration,
    last_placement_generation: PlacementGeneration,
    surface_authority_generations: BTreeMap<SurfaceId, CoordinateGeneration>,
}

#[cfg(test)]
impl Default for ViewportRegistry {
    fn default() -> Self {
        Self::new(EngineAuthorityDomainId::new_for_test(1))
    }
}

impl ViewportRegistry {
    pub(crate) const fn new(authority_domain: EngineAuthorityDomainId) -> Self {
        Self {
            authority_domain,
            records: BTreeMap::new(),
            windows: BTreeMap::new(),
            token_to_window: BTreeMap::new(),
            last_token: WindowToken::managed(0),
            last_incarnation: WindowIncarnation::new(0),
            native_binding_allocation_budget: None,
            inventory_generation: InventoryGeneration::new(0),
            last_coordinate_generation: CoordinateGeneration::new(0),
            last_placement_generation: PlacementGeneration::new(0),
            surface_authority_generations: BTreeMap::new(),
        }
    }

    /// Candidate-local storage admission, cleared before publication. This is
    /// a consumable allocation budget, not another binding identity registry.
    pub(crate) fn set_native_binding_allocation_budget(&mut self, remaining: Option<usize>) {
        self.native_binding_allocation_budget = remaining;
    }

    pub(crate) const fn native_binding_allocation_available(&self) -> bool {
        !matches!(self.native_binding_allocation_budget, Some(0))
    }

    fn commit_native_binding_slot(&mut self) {
        if let Some(remaining) = &mut self.native_binding_allocation_budget {
            debug_assert!(*remaining > 0, "successful binding insertion was admitted");
            *remaining = remaining.saturating_sub(1);
        }
    }

    /// Registers an adapter-owned token for an existing logical surface.
    pub(crate) fn register_existing(
        &mut self,
        epoch: WorkspaceEpoch,
        surface: SurfaceId,
        token: WindowToken,
        role: ViewportRole,
    ) -> Result<ViewportBinding, ViewportRegistryError> {
        self.register_existing_with_admission(
            epoch,
            surface,
            token,
            role,
            ViewportOwnership::External,
            ViewportAdmission::Admitted,
        )
    }

    /// Registers an existing child whose native lifetime is owned by dockspace.
    pub(crate) fn register_owned_child(
        &mut self,
        epoch: WorkspaceEpoch,
        surface: SurfaceId,
        token: WindowToken,
    ) -> Result<ViewportBinding, ViewportRegistryError> {
        self.register_existing_with_admission(
            epoch,
            surface,
            token,
            ViewportRole::Child,
            ViewportOwnership::RuntimeOwned,
            ViewportAdmission::Admitted,
        )
    }

    /// Registers an externally owned replacement that must remain non-authoritative until its
    /// exact visible presentation proof arrives.
    fn register_existing_with_admission(
        &mut self,
        epoch: WorkspaceEpoch,
        surface: SurfaceId,
        token: WindowToken,
        role: ViewportRole,
        ownership: ViewportOwnership,
        admission: ViewportAdmission,
    ) -> Result<ViewportBinding, ViewportRegistryError> {
        if !token.is_host() {
            return Err(ViewportRegistryError::ManagedTokenCannotRegister { token });
        }
        if self.records.contains_key(&surface) {
            return Err(ViewportRegistryError::SurfaceAlreadyRegistered { surface });
        }
        if self.token_to_window.contains_key(&token) {
            return Err(ViewportRegistryError::TokenAlreadyRegistered { token });
        }
        if !self.native_binding_allocation_available() {
            return Err(ViewportRegistryError::NativeBindingCapacity);
        }
        let incarnation = self.next_incarnation()?;
        let binding = self.insert_binding(
            ViewportBinding::new(self.authority_domain, epoch, surface, token, incarnation),
            role,
            ownership,
            admission,
        )?;
        self.commit_native_binding_slot();
        Ok(binding)
    }

    /// Reserves a core token for a future native child window.
    pub(crate) fn reserve(
        &mut self,
        epoch: WorkspaceEpoch,
        surface: SurfaceId,
        role: ViewportRole,
    ) -> Result<ViewportBinding, ViewportRegistryError> {
        if role == ViewportRole::Root {
            return Err(ViewportRegistryError::RuntimeOwnedRootForbidden);
        }
        if self.records.contains_key(&surface) {
            return Err(ViewportRegistryError::SurfaceAlreadyRegistered { surface });
        }
        if !self.native_binding_allocation_available() {
            return Err(ViewportRegistryError::NativeBindingCapacity);
        }
        let token = self
            .last_token
            .checked_next()
            .ok_or(ViewportRegistryError::WindowTokenExhausted)?;
        let incarnation = self.next_incarnation()?;
        let binding =
            ViewportBinding::new(self.authority_domain, epoch, surface, token, incarnation);
        self.insert_binding(
            binding,
            role,
            ViewportOwnership::RuntimeOwned,
            ViewportAdmission::Pending,
        )?;
        self.last_token = token;
        self.commit_native_binding_slot();
        Ok(binding)
    }

    fn next_incarnation(&mut self) -> Result<WindowIncarnation, ViewportRegistryError> {
        let incarnation = self
            .last_incarnation
            .checked_next()
            .ok_or(ViewportRegistryError::WindowIncarnationExhausted)?;
        self.last_incarnation = incarnation;
        Ok(incarnation)
    }

    fn insert_binding(
        &mut self,
        binding: ViewportBinding,
        role: ViewportRole,
        ownership: ViewportOwnership,
        admission: ViewportAdmission,
    ) -> Result<ViewportBinding, ViewportRegistryError> {
        if self.records.contains_key(&binding.surface()) {
            return Err(ViewportRegistryError::SurfaceAlreadyRegistered {
                surface: binding.surface(),
            });
        }
        if self.token_to_window.contains_key(&binding.token()) {
            return Err(ViewportRegistryError::TokenAlreadyRegistered {
                token: binding.token(),
            });
        }
        let coordinate_generation = self.next_surface_authority(binding.surface())?;
        self.token_to_window
            .insert(binding.token(), binding.window());
        self.windows.insert(
            binding.window(),
            WindowRecord {
                observation_binding: binding,
                role,
                ownership,
                lifecycle: ViewportLifecycle::AwaitingObservation,
                coordinate_observations: WindowCoordinateObservationStream::default(),
                input_observations: WindowInputObservationStream::default(),
                presentation_observations: WindowPresentationObservationStream::default(),
                create_preparation_observations: WindowCreatePreparationObservationStream::default(
                ),
                close_observations: WindowCloseObservationStream::default(),
                ever_observed: false,
                pending_close_request: None,
                coordinates: None,
                coordinate_generation,
            },
        );
        self.records.insert(
            binding.surface(),
            SurfacePlacementRecord {
                binding,
                admission,
                coordinates: None,
                recovery_coordinates: None,
                coordinate_generation,
                placement: None,
            },
        );
        Ok(binding)
    }

    fn next_surface_authority(
        &mut self,
        surface: SurfaceId,
    ) -> Result<CoordinateGeneration, ViewportRegistryError> {
        let generation = self
            .last_coordinate_generation
            .checked_next()
            .ok_or(ViewportRegistryError::CoordinateGenerationExhausted)?;
        self.last_coordinate_generation = generation;
        self.surface_authority_generations
            .insert(surface, generation);
        Ok(generation)
    }

    /// Test-only shorthand for a snapshot with no detached retirement bindings.
    #[cfg(test)]
    pub(crate) fn apply_snapshot_for_test(
        &mut self,
        snapshot: &PlatformSnapshot,
    ) -> Result<RegistryTransition, ViewportRegistryError> {
        self.apply_snapshot_with_retired_bindings(
            snapshot,
            snapshot.inventory_observation().known_roster(),
            &BTreeSet::new(),
            &BTreeSet::new(),
        )
    }

    /// Applies one provider snapshot while allowing exact detached bindings to
    /// remain observable exclusively by the retirement coordinator.
    ///
    /// A detached binding is never written into this registry. The allowance is
    /// only for lifecycle cleanup facts owned by `ViewportCoordinator`; every
    /// current registry record still accepts observations for its exact binding
    /// alone. A binding whose retirement already completed is narrower still:
    /// its terminal tombstone may be repeated, but no new live or unknown facts
    /// may be attributed to that old incarnation.
    pub(crate) fn apply_snapshot_with_retired_bindings(
        &mut self,
        snapshot: &PlatformSnapshot,
        inventory: Option<&[ViewportBinding]>,
        retired_bindings: &BTreeSet<ViewportBinding>,
        terminal_binding_guards: &BTreeSet<ViewportBinding>,
    ) -> Result<RegistryTransition, ViewportRegistryError> {
        let mut candidate = self.clone();
        let transition = candidate.apply_snapshot_in_place(
            snapshot,
            inventory,
            retired_bindings,
            terminal_binding_guards,
        )?;
        *self = candidate;
        Ok(transition)
    }

    /// Applies one exact live-binding close fact without replacing any other
    /// platform authority lane.
    pub(crate) fn apply_close_observation(
        &mut self,
        observation: WindowCloseObservation,
    ) -> Result<RegistryTransition, ViewportRegistryError> {
        let mut candidate = self.clone();
        let transition = candidate.apply_close_observation_in_place(observation)?;
        *self = candidate;
        Ok(transition)
    }

    fn apply_close_observation_in_place(
        &mut self,
        observation: WindowCloseObservation,
    ) -> Result<RegistryTransition, ViewportRegistryError> {
        if observation.known_state() == Some(WindowCloseState::Destroyed) {
            return Err(ViewportRegistryError::NarrowDestroyedObservationForbidden {
                binding: observation.binding(),
            });
        }
        let binding = observation.binding();
        self.validate_live_observed_binding(binding, &BTreeSet::new())?;
        let generation = self
            .inventory_generation
            .checked_next()
            .ok_or(ViewportRegistryError::InventoryGenerationExhausted)?;
        let observation = observation.with_inventory_generation(generation);
        let retiring = self.records.values().any(|record| {
            record.binding.window() == binding.window()
                && record.admission == ViewportAdmission::Retiring
        });
        let window = self.windows.get_mut(&binding.window()).ok_or(
            ViewportRegistryError::UnknownWindow {
                binding: binding.window(),
            },
        )?;
        if matches!(window.lifecycle, ViewportLifecycle::Destroyed) {
            return Err(ViewportRegistryError::BindingAlreadyDestroyed { binding });
        }

        window
            .close_observations
            .observe(binding, Some(observation));
        let mut events = Vec::new();
        let destroyed = reduce_window_close_observation(window, &mut events);
        debug_assert!(!destroyed, "narrow close ingress rejects destroyed facts");
        if window.pending_close_request.is_some() {
            window.lifecycle = ViewportLifecycle::CloseRequested;
        } else if retiring {
            window.lifecycle = ViewportLifecycle::AwaitingDestroyed;
        } else if window.coordinates.is_some() {
            window.lifecycle = ViewportLifecycle::Ready;
        } else {
            window.lifecycle = ViewportLifecycle::AwaitingObservation;
        }
        self.inventory_generation = generation;
        Ok(RegistryTransition {
            generation,
            native_window_placement_changed: false,
            events,
        })
    }

    /// Revokes every live fact owned by the current platform provider.
    ///
    /// Provider replacement is an authority boundary, not an inventory observation. It clears
    /// per-provider streams and live routing state while preserving stable bindings, ownership,
    /// admission, recovery anchors, and terminal destruction facts. Structural retirement also
    /// remains pending so a provider handoff cannot resurrect a retiring native window.
    ///
    /// The core inventory generation advances exactly once when at least one record loses live
    /// semantics. Resetting only an already-unavailable stream's provider watermark does not
    /// advance that semantic generation. Repeating the revocation after all live facts are gone
    /// is inert. The operation is applied through a candidate clone so generation exhaustion
    /// cannot partially revoke the registry.
    #[cfg(test)]
    pub(crate) fn revoke_live_provider_authority(
        &mut self,
    ) -> Result<RegistryTransition, ViewportRegistryError> {
        let mut candidate = self.clone();
        let transition = candidate.revoke_live_provider_authority_in_place()?;
        *self = candidate;
        Ok(transition)
    }

    #[cfg(test)]
    fn revoke_live_provider_authority_in_place(
        &mut self,
    ) -> Result<RegistryTransition, ViewportRegistryError> {
        let mut events = Vec::new();
        let mut authority_lost = Vec::new();
        for (&binding, window) in &mut self.windows {
            if window.lifecycle == ViewportLifecycle::Destroyed {
                continue;
            }
            let before = window.clone();
            let target = if self.records.values().any(|record| {
                record.binding.window() == binding
                    && record.admission == ViewportAdmission::Retiring
            }) {
                ViewportLifecycle::AwaitingDestroyed
            } else {
                ViewportLifecycle::AwaitingObservation
            };
            let changed = before.lifecycle != target
                || before.ever_observed
                || before.pending_close_request.is_some()
                || before.coordinate_observations.current().is_some()
                || before.input_observations.current().is_some()
                || before.presentation_observations.current().is_some()
                || before.close_observations.current().is_some();
            for (surface, record) in self
                .records
                .iter_mut()
                .filter(|(_, record)| record.binding.window() == binding)
            {
                if (ViewportRecord {
                    placement: record,
                    window: &before,
                })
                .has_coordinate_authority()
                {
                    authority_lost.push(*surface);
                }
                record.coordinates = None;
                if changed {
                    events.push(RegistryEvent::FactsUnavailable {
                        binding: record.binding,
                    });
                }
            }
            window.lifecycle = target;
            window.coordinates = None;
            window.coordinate_observations = WindowCoordinateObservationStream::default();
            window.input_observations = WindowInputObservationStream::default();
            window.presentation_observations = WindowPresentationObservationStream::default();
            window.create_preparation_observations =
                WindowCreatePreparationObservationStream::default();
            window.close_observations = WindowCloseObservationStream::default();
            window.ever_observed = false;
            window.pending_close_request = None;
        }
        if events.is_empty() {
            return Ok(RegistryTransition {
                generation: self.inventory_generation,
                native_window_placement_changed: false,
                events,
            });
        }
        for surface in authority_lost {
            let generation = self.next_surface_authority(surface)?;
            self.records
                .get_mut(&surface)
                .ok_or(ViewportRegistryError::MissingSurface { surface })?
                .coordinate_generation = generation;
        }
        let generation = self
            .inventory_generation
            .checked_next()
            .ok_or(ViewportRegistryError::InventoryGenerationExhausted)?;
        self.inventory_generation = generation;
        Ok(RegistryTransition {
            generation,
            native_window_placement_changed: false,
            events,
        })
    }

    fn apply_snapshot_in_place(
        &mut self,
        snapshot: &PlatformSnapshot,
        inventory: Option<&[ViewportBinding]>,
        retired_bindings: &BTreeSet<ViewportBinding>,
        terminal_binding_guards: &BTreeSet<ViewportBinding>,
    ) -> Result<RegistryTransition, ViewportRegistryError> {
        self.validate_snapshot_bindings(snapshot, retired_bindings, terminal_binding_guards)?;
        let generation = self
            .inventory_generation
            .checked_next()
            .ok_or(ViewportRegistryError::InventoryGenerationExhausted)?;
        let observations: BTreeMap<WindowBinding, &ObservedWindow> = snapshot
            .window_observations()
            .iter()
            .map(|window| (window.binding().window(), window))
            .collect();
        let closes: BTreeMap<_, _> = snapshot
            .close_observations()
            .iter()
            .map(|observation| (observation.binding().window(), *observation))
            .collect();
        let mut events = Vec::new();
        let mut native_window_placement_changed = false;
        let mut next_authority = self.last_coordinate_generation;
        for (&binding, window) in &mut self.windows {
            let previous = window.clone();
            let observation = observations.get(&binding).copied();
            if window.lifecycle == ViewportLifecycle::Destroyed && observation.is_some() {
                return Err(ViewportRegistryError::DestroyedBindingReappeared {
                    binding: observation.expect("present observation").binding(),
                });
            }
            window.close_observations.observe(
                binding,
                closes
                    .get(&binding)
                    .copied()
                    .map(|observation| observation.with_inventory_generation(generation)),
            );
            let destroyed = reduce_window_close_observation(window, &mut events);
            if !destroyed {
                if let Some(observation) = observation {
                    window.ever_observed = true;
                    window
                        .coordinate_observations
                        .observe(binding, observation.coordinate_observation());
                    window
                        .input_observations
                        .observe(binding, observation.input_observation());
                    window.presentation_observations.observe(
                        binding,
                        observation
                            .presentation_observation()
                            .map(|observation| observation.with_inventory_generation(generation)),
                    );
                    window.create_preparation_observations.observe(
                        binding,
                        observation
                            .create_preparation()
                            .map(|observation| observation.with_inventory_generation(generation)),
                    );
                    window.coordinates =
                        window
                            .coordinate_observations
                            .current()
                            .and_then(|observation| {
                                WindowCoordinateSnapshot::from_observation(
                                    binding,
                                    window.coordinate_generation,
                                    observation,
                                )
                            });
                    window.lifecycle = if self.records.values().any(|record| {
                        record.binding.window() == binding
                            && record.admission == ViewportAdmission::Retiring
                    }) {
                        ViewportLifecycle::AwaitingDestroyed
                    } else if window.pending_close_request.is_some() {
                        ViewportLifecycle::CloseRequested
                    } else if window.coordinates.is_some() {
                        ViewportLifecycle::Ready
                    } else {
                        ViewportLifecycle::AwaitingObservation
                    };
                } else if inventory.is_some()
                    && window.ever_observed
                    && !matches!(
                        window.lifecycle,
                        ViewportLifecycle::Missing | ViewportLifecycle::Destroyed
                    )
                {
                    window.lifecycle = ViewportLifecycle::Missing;
                }
            }
            let physical_changed = match (previous.coordinates, window.coordinates) {
                (Some(before), Some(after)) => !before.same_facts(after),
                (None, None) => false,
                _ => true,
            };
            let physical_authority_changed = window_has_coordinate_authority(&previous)
                != window_has_coordinate_authority(window);
            if physical_changed || physical_authority_changed || destroyed {
                next_authority = next_authority
                    .checked_next()
                    .ok_or(ViewportRegistryError::CoordinateGenerationExhausted)?;
                window.coordinate_generation = next_authority;
                window.coordinates = window
                    .coordinates
                    .map(|coordinates| coordinates.with_generation(next_authority));
            }
            let presentation_changed = previous.presentation_observations.current()
                != window.presentation_observations.current();
            for (surface, record) in self
                .records
                .iter_mut()
                .filter(|(_, record)| record.binding.window() == binding)
            {
                let before = record.coordinates;
                let before_authority = ViewportRecord {
                    placement: record,
                    window: &previous,
                }
                .has_coordinate_authority();
                if destroyed {
                    record.admission = ViewportAdmission::Retiring;
                }
                if observation.is_some() && !destroyed {
                    record.coordinates = window.coordinates.and_then(|coordinates| {
                        let mapping = if let Some(placement) = record.placement {
                            SurfaceClientGeometry::new(
                                placement.origin(),
                                placement.scale_factor(),
                                placement.clip(),
                            )
                        } else {
                            let scale = window
                                .coordinate_observations
                                .current()?
                                .presentation_scale_factor()
                                .known()
                                .copied()?;
                            SurfaceClientGeometry::whole_client(coordinates.content_size(), scale)
                                .ok()?
                        };
                        CoordinateSnapshot::from_window_coordinates(
                            record.binding,
                            record.coordinate_generation,
                            coordinates,
                            mapping,
                        )
                    });
                }
                let changed = match (before, record.coordinates) {
                    (Some(before), Some(after)) => !before.same_facts(after),
                    (None, None) => false,
                    _ => true,
                };
                let has_authority = ViewportRecord {
                    placement: record,
                    window,
                }
                .has_coordinate_authority();
                if (changed || !before_authority)
                    && has_authority
                    && record.admission == ViewportAdmission::Admitted
                    && let Some(after) = record.coordinates
                    && after.outer_bounds().is_some()
                {
                    // A newly authoritative placement can enter document capture
                    // even when its geometry arrived during an earlier hidden pass.
                    native_window_placement_changed |= !before_authority
                        || before.is_none_or(|before| {
                            before.outer_bounds() != after.outer_bounds()
                                || before.content_size() != after.content_size()
                                || before.native_scale_factor() != after.native_scale_factor()
                        });
                }
                if changed
                    || before_authority != has_authority
                    || destroyed
                    || previous.lifecycle != window.lifecycle
                        && window.lifecycle == ViewportLifecycle::Missing
                {
                    next_authority = next_authority
                        .checked_next()
                        .ok_or(ViewportRegistryError::CoordinateGenerationExhausted)?;
                    record.coordinate_generation = next_authority;
                    record.coordinates = record
                        .coordinates
                        .map(|coordinates| coordinates.with_generation(next_authority));
                    self.surface_authority_generations
                        .insert(*surface, next_authority);
                }
                if window.lifecycle == ViewportLifecycle::Ready
                    && (previous.lifecycle != ViewportLifecycle::Ready || changed)
                {
                    events.push(RegistryEvent::Ready {
                        binding: record.binding,
                    });
                } else if window.lifecycle == ViewportLifecycle::AwaitingObservation
                    && (previous.lifecycle == ViewportLifecycle::Ready || changed)
                {
                    events.push(RegistryEvent::FactsUnavailable {
                        binding: record.binding,
                    });
                } else if window.lifecycle == ViewportLifecycle::Missing
                    && previous.lifecycle != ViewportLifecycle::Missing
                {
                    events.push(RegistryEvent::BindingMissing {
                        binding: record.binding,
                    });
                }
                if presentation_changed {
                    events.push(RegistryEvent::PresentationChanged {
                        binding: record.binding,
                    });
                }
                if has_authority && let Some(coordinates) = record.coordinates {
                    record.recovery_coordinates = Some(record.recovery_coordinates.map_or_else(
                        || RecoveryCoordinateSnapshot::new(coordinates),
                        |recovery| recovery.observe(coordinates),
                    ));
                }
            }
        }
        self.inventory_generation = generation;
        self.last_coordinate_generation = next_authority;
        Ok(RegistryTransition {
            generation,
            native_window_placement_changed,
            events,
        })
    }

    fn validate_snapshot_bindings(
        &self,
        snapshot: &PlatformSnapshot,
        retired_bindings: &BTreeSet<ViewportBinding>,
        terminal_binding_guards: &BTreeSet<ViewportBinding>,
    ) -> Result<(), ViewportRegistryError> {
        for window in snapshot.window_observations() {
            self.validate_live_observed_binding(window.binding(), retired_bindings)?;
        }
        for observation in snapshot.close_observations() {
            self.validate_close_observed_binding(
                *observation,
                retired_bindings,
                terminal_binding_guards,
            )?;
        }
        Ok(())
    }

    fn validate_live_observed_binding(
        &self,
        binding: ViewportBinding,
        retired_bindings: &BTreeSet<ViewportBinding>,
    ) -> Result<(), ViewportRegistryError> {
        if self.windows.contains_key(&binding.window())
            || retired_bindings
                .iter()
                .any(|retired| retired.window() == binding.window())
        {
            return Ok(());
        }
        match self.records.get(&binding.surface()) {
            Some(record) if record.binding == binding => Ok(()),
            Some(_) if retired_bindings.contains(&binding) => Ok(()),
            Some(record) => Err(ViewportRegistryError::StaleObservedBinding {
                observed: binding,
                current: record.binding,
            }),
            None if retired_bindings.contains(&binding) => Ok(()),
            None => Err(ViewportRegistryError::UnknownObservedBinding { binding }),
        }
    }

    fn validate_close_observed_binding(
        &self,
        observation: WindowCloseObservation,
        retired_bindings: &BTreeSet<ViewportBinding>,
        terminal_binding_guards: &BTreeSet<ViewportBinding>,
    ) -> Result<(), ViewportRegistryError> {
        if observation.known_state() == Some(WindowCloseState::Destroyed)
            && terminal_binding_guards
                .iter()
                .any(|terminal| terminal.window() == observation.binding().window())
        {
            return Ok(());
        }
        self.validate_live_observed_binding(observation.binding(), retired_bindings)
    }

    pub(crate) fn mark_awaiting_destroyed(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<(), ViewportRegistryError> {
        let window =
            self.windows
                .get(&binding.window())
                .ok_or(ViewportRegistryError::UnknownWindow {
                    binding: binding.window(),
                })?;
        if window.lifecycle == ViewportLifecycle::Destroyed {
            return Err(ViewportRegistryError::BindingAlreadyDestroyed { binding });
        }
        let bindings: Vec<_> = self.bindings_for_window(binding.window()).collect();
        for exact in bindings {
            let generation = self.next_surface_authority(exact.surface())?;
            let (record, window) = self.current_record_mut(exact)?;
            window.lifecycle = ViewportLifecycle::AwaitingDestroyed;
            record.admission = ViewportAdmission::Retiring;
            record.coordinate_generation = generation;
            record.coordinates = record
                .coordinates
                .map(|snapshot| snapshot.with_generation(generation));
        }
        Ok(())
    }

    pub(crate) fn resume_after_failed_close(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<(), ViewportRegistryError> {
        let lifecycle = {
            let window = self.windows.get(&binding.window()).ok_or(
                ViewportRegistryError::UnknownWindow {
                    binding: binding.window(),
                },
            )?;
            if window.lifecycle == ViewportLifecycle::Destroyed {
                return Err(ViewportRegistryError::BindingAlreadyDestroyed { binding });
            }
            if window.pending_close_request.is_some() {
                ViewportLifecycle::CloseRequested
            } else if window.coordinates.is_some() {
                ViewportLifecycle::Ready
            } else {
                ViewportLifecycle::AwaitingObservation
            }
        };
        let bindings: Vec<_> = self.bindings_for_window(binding.window()).collect();
        for exact in bindings {
            let generation = self.next_surface_authority(exact.surface())?;
            let (record, window) = self.current_record_mut(exact)?;
            window.lifecycle = lifecycle;
            record.admission = ViewportAdmission::Admitted;
            record.coordinate_generation = generation;
            record.coordinates = record
                .coordinates
                .map(|snapshot| snapshot.with_generation(generation));
        }
        Ok(())
    }

    /// Restores the pre-admission state of a staging binding after its exact native close edge
    /// was authoritatively cleared.
    ///
    /// This is intentionally narrower than [`Self::resume_after_failed_close`]. A staging
    /// replacement has never owned graph, focus, or routing authority, so restoring it must
    /// retain `Pending` admission even when live coordinates are available.
    pub(crate) fn resume_pending_after_pre_admission_close_clear(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<bool, ViewportRegistryError> {
        let resumable = {
            let record = self.current_record(binding)?;
            let window = record.window;
            record.admission() == ViewportAdmission::Retiring
                && matches!(window.lifecycle, ViewportLifecycle::AwaitingDestroyed)
                && window.pending_close_request.is_none()
                && window
                    .close_observations
                    .current()
                    .is_some_and(|observation| {
                        observation.known_state() == Some(WindowCloseState::LiveClear)
                    })
        };
        if !resumable {
            return Ok(false);
        }
        let generation = self.next_surface_authority(binding.surface())?;
        let (record, window) = self.current_record_mut(binding)?;
        window.lifecycle = if record.coordinates.is_some() {
            ViewportLifecycle::Ready
        } else {
            ViewportLifecycle::AwaitingObservation
        };
        record.admission = ViewportAdmission::Pending;
        record.coordinate_generation = generation;
        record.coordinates = record
            .coordinates
            .map(|snapshot| snapshot.with_generation(generation));
        Ok(true)
    }

    /// Admits one proven runtime-owned binding after topology commit.
    #[cfg(test)]
    pub(crate) fn admit(&mut self, binding: ViewportBinding) -> Result<(), ViewportRegistryError> {
        let (record, window) = self.current_record_mut(binding)?;
        if record.admission != ViewportAdmission::Pending
            || !matches!(window.lifecycle, ViewportLifecycle::Ready)
            || window
                .presentation_observations
                .current()
                .and_then(WindowPresentationObservation::known_state)
                != Some(WindowPresentationState::Visible)
        {
            return Err(ViewportRegistryError::BindingNotAdmissible { binding });
        }
        record.admission = ViewportAdmission::Admitted;
        Ok(())
    }

    /// Detaches one exact binding from the current logical roster while preserving every
    /// causal stream required to finish its platform lifecycle independently.
    pub(crate) fn detach(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<DetachedViewportFacts, ViewportRegistryError> {
        self.current_record(binding)?;
        let surface = binding.surface();
        self.next_surface_authority(surface)?;
        let record = self
            .records
            .remove(&surface)
            .ok_or(ViewportRegistryError::MissingSurface { surface })?;
        let window = self
            .windows
            .get(&binding.window())
            .cloned()
            .ok_or(ViewportRegistryError::StaleBinding { binding })?;
        let window_retired = !self
            .records
            .values()
            .any(|record| record.binding.window() == binding.window());
        if window_retired {
            self.windows.remove(&binding.window());
            self.token_to_window.remove(&binding.token());
        }
        Ok(DetachedViewportFacts {
            binding: record.binding,
            role: window.role,
            ownership: window.ownership,
            lifecycle: window.lifecycle,
            input_observations: window.input_observations,
            close_observations: window.close_observations,
            ever_observed: window.ever_observed,
            window_retired,
        })
    }

    pub(crate) fn remove_destroyed(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<(), ViewportRegistryError> {
        let record = self.current_record(binding)?;
        let window = record.window;
        if !matches!(window.lifecycle, ViewportLifecycle::Destroyed) {
            return Err(ViewportRegistryError::BindingNotDestroyed { binding });
        }
        self.next_surface_authority(binding.surface())?;
        self.records.remove(&binding.surface());
        if !self
            .records
            .values()
            .any(|record| record.binding.window() == binding.window())
        {
            self.windows.remove(&binding.window());
            self.token_to_window.remove(&binding.token());
        }
        Ok(())
    }

    /// Drops a future-window reservation only when no matching window was ever observed.
    ///
    /// The caller must first establish absence from an authoritative complete inventory.
    pub(crate) fn discard_unobserved(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<(), ViewportRegistryError> {
        let record = self.current_record(binding)?;
        let window = record.window;
        if window.ever_observed {
            return Err(ViewportRegistryError::BindingAlreadyObserved { binding });
        }
        self.next_surface_authority(binding.surface())?;
        self.records.remove(&binding.surface());
        if !self
            .records
            .values()
            .any(|record| record.binding.window() == binding.window())
        {
            self.windows.remove(&binding.window());
            self.token_to_window.remove(&binding.token());
        }
        Ok(())
    }

    /// Rebinds exact retained windows to unique targets without replacing physical windows.
    pub(crate) fn reconcile_restore(
        &mut self,
        new_epoch: WorkspaceEpoch,
        retained_bindings: &BTreeMap<ViewportBinding, SurfaceId>,
    ) -> Result<RestoreRegistryReconciliation, ViewportRegistryError> {
        let mut candidate = self.clone();
        let reconciliation = candidate.reconcile_restore_in_place(new_epoch, retained_bindings)?;
        *self = candidate;
        Ok(reconciliation)
    }

    fn reconcile_restore_in_place(
        &mut self,
        new_epoch: WorkspaceEpoch,
        retained_bindings: &BTreeMap<ViewportBinding, SurfaceId>,
    ) -> Result<RestoreRegistryReconciliation, ViewportRegistryError> {
        let mut targets = BTreeSet::new();
        for (binding, target) in retained_bindings {
            self.current_record(*binding)?;
            if !targets.insert(*target)
                || self.records.get(target).is_some_and(|record| {
                    record.binding != *binding && !retained_bindings.contains_key(&record.binding)
                })
            {
                return Err(ViewportRegistryError::SurfaceAlreadyRegistered { surface: *target });
            }
        }
        let bindings: Vec<_> = self.records.values().map(|record| record.binding).collect();
        let mut retired = Vec::new();
        let mut continuing = Vec::new();
        for &before in &bindings {
            if !retained_bindings.contains_key(&before) {
                retired.push(self.detach(before)?);
            }
        }
        for before in bindings {
            if let Some(&target) = retained_bindings.get(&before) {
                let record = self.records.remove(&before.surface()).ok_or(
                    ViewportRegistryError::MissingSurface {
                        surface: before.surface(),
                    },
                )?;
                if before.surface() != target {
                    self.next_surface_authority(before.surface())?;
                }
                continuing.push((before, target, record));
            }
        }
        let mut window_successors = BTreeMap::new();
        let mut retained = Vec::new();
        // All predecessor placements are vacant before any target is installed.
        for (before, target, mut record) in continuing {
            let after_window = if let Some(&window) = window_successors.get(&before.window()) {
                window
            } else {
                let mut window = self
                    .windows
                    .remove(&before.window())
                    .ok_or(ViewportRegistryError::StaleBinding { binding: before })?;
                let after = WindowBinding::new(
                    self.authority_domain,
                    new_epoch,
                    before.token(),
                    self.next_incarnation()?,
                );
                window.observation_binding = ViewportBinding::with_placement_generation(
                    after,
                    target,
                    before.placement_generation(),
                );
                let generation = self
                    .last_coordinate_generation
                    .checked_next()
                    .ok_or(ViewportRegistryError::CoordinateGenerationExhausted)?;
                self.last_coordinate_generation = generation;
                window.lifecycle = ViewportLifecycle::AwaitingObservation;
                window.coordinates = None;
                window.coordinate_generation = generation;
                window.coordinate_observations = WindowCoordinateObservationStream::default();
                window.input_observations = WindowInputObservationStream::default();
                window.presentation_observations = WindowPresentationObservationStream::default();
                window.create_preparation_observations =
                    WindowCreatePreparationObservationStream::default();
                window.close_observations = WindowCloseObservationStream::default();
                window.ever_observed = false;
                window.pending_close_request = None;
                self.windows.insert(after, window);
                self.token_to_window.insert(before.token(), after);
                window_successors.insert(before.window(), after);
                after
            };
            let after = ViewportBinding::with_placement_generation(
                after_window,
                target,
                before.placement_generation(),
            );
            record.binding = after;
            record.coordinates = None;
            record.recovery_coordinates = None;
            record.coordinate_generation = self.next_surface_authority(target)?;
            record.placement = record.placement.map(|placement| {
                NativeSurfacePlacement::new(
                    target,
                    placement.size(),
                    placement.origin(),
                    placement.scale_factor(),
                    placement.clip(),
                )
            });
            self.records.insert(target, record);
            retained.push((before, after));
        }
        Ok(RestoreRegistryReconciliation { retained, retired })
    }

    #[must_use]
    pub const fn inventory_generation(&self) -> InventoryGeneration {
        self.inventory_generation
    }

    #[must_use]
    pub fn record(&self, surface: SurfaceId) -> Option<ViewportRecord<'_>> {
        let placement = self.records.get(&surface)?;
        let window = self.windows.get(&placement.binding.window())?;
        Some(ViewportRecord { placement, window })
    }

    pub(crate) fn placement_record(&self, binding: ViewportBinding) -> Option<ViewportRecord<'_>> {
        self.record(binding.surface())
            .filter(|record| record.binding() == binding)
    }

    pub(crate) fn window(&self, binding: WindowBinding) -> Option<WindowRecordView<'_>> {
        Some(WindowRecordView {
            binding,
            record: self.windows.get(&binding)?,
        })
    }

    pub(crate) fn windows(&self) -> impl Iterator<Item = WindowRecordView<'_>> {
        self.windows
            .iter()
            .map(|(&binding, record)| WindowRecordView { binding, record })
    }

    pub(crate) fn window_coordinates(
        &self,
        binding: WindowBinding,
    ) -> Option<WindowCoordinateSnapshot> {
        self.windows.get(&binding)?.coordinates
    }

    pub(crate) fn bindings_for_window(
        &self,
        binding: WindowBinding,
    ) -> impl Iterator<Item = ViewportBinding> + '_ {
        self.records
            .values()
            .filter(move |record| record.binding.window() == binding)
            .map(|record| record.binding)
    }

    pub(crate) fn placements_in_window(
        &self,
        binding: WindowBinding,
    ) -> impl Iterator<Item = ViewportRecord<'_>> {
        self.records()
            .filter_map(move |(_, record)| (record.binding().window() == binding).then_some(record))
    }

    pub(crate) fn surface_placement(
        &self,
        binding: ViewportBinding,
    ) -> Option<NativeSurfacePlacement> {
        let record = self.placement_record(binding)?;
        if let Some(placement) = record.placement.placement {
            return Some(placement);
        }
        let coordinates = record.coordinates()?;
        let mapping = coordinates.surface_client_geometry();
        let size = LogicalSize::new(
            coordinates.content_size().width() / mapping.scale_factor().get(),
            coordinates.content_size().height() / mapping.scale_factor().get(),
        )
        .ok()?;
        Some(NativeSurfacePlacement::new(
            binding.surface(),
            size,
            mapping.origin(),
            mapping.scale_factor(),
            mapping.clip(),
        ))
    }

    /// Replaces this window's roster and returns the surfaces whose exact bindings changed.
    pub(crate) fn set_surface_placements(
        &mut self,
        binding: WindowBinding,
        placements: &[NativeSurfacePlacement],
        retains_binding: impl Fn(ViewportBinding) -> bool,
    ) -> Result<Vec<SurfaceId>, ViewportRegistryError> {
        let window = self
            .windows
            .get(&binding)
            .ok_or(ViewportRegistryError::UnknownWindow { binding })?;
        if placements.is_empty() {
            return Err(ViewportRegistryError::WindowPlacementUnavailable { binding });
        }
        let frozen = matches!(
            window.lifecycle,
            ViewportLifecycle::AwaitingDestroyed | ViewportLifecycle::Destroyed
        ) || window.pending_close_request.is_some();

        let mut surfaces = BTreeSet::new();
        let mut replacements = Vec::new();
        let mut declarations = Vec::new();
        for &placement in placements {
            let surface = placement.surface();
            let existing = self.records.get(&surface);
            if !surfaces.insert(surface)
                || existing.is_some_and(|record| record.binding.window() != binding)
            {
                return Err(if frozen {
                    ViewportRegistryError::WindowPlacementUnavailable { binding }
                } else {
                    ViewportRegistryError::SurfaceAlreadyRegistered { surface }
                });
            }
            if let Some(existing) = existing
                && self.surface_placement(existing.binding) == Some(placement)
            {
                if existing.placement.is_none() {
                    declarations.push(placement);
                }
                continue;
            }
            if frozen {
                return Err(ViewportRegistryError::WindowPlacementUnavailable { binding });
            }

            if placement.size().width() <= 0.0
                || placement.size().height() <= 0.0
                || LogicalRect::from_min_size(
                    crate::geometry::LogicalPoint::new(0.0, 0.0).expect("finite origin"),
                    placement.size(),
                )
                .and_then(|rect| {
                    rect.to_desktop_physical(placement.origin(), placement.scale_factor())
                })
                .is_err()
            {
                return Err(ViewportRegistryError::InvalidSurfacePlacement { surface });
            }
            replacements.push(placement);
        }
        let removed: Vec<_> = self
            .bindings_for_window(binding)
            .filter(|old| !surfaces.contains(&old.surface()))
            .collect();
        if replacements.is_empty() && removed.is_empty() {
            self.declare_preserved_placements(declarations);
            return Ok(Vec::new());
        }
        if frozen {
            return Err(ViewportRegistryError::WindowPlacementUnavailable { binding });
        }

        let mut candidate = self.clone();
        if let Some(remaining) = &mut candidate.native_binding_allocation_budget {
            // The real window stays live. Placements without exact cleanup
            // obligations are removed from the producer at publication.
            *remaining += removed.iter().filter(|&&old| !retains_binding(old)).count();
        }
        let mut changed = Vec::with_capacity(removed.len() + replacements.len());
        for old in removed {
            candidate.next_surface_authority(old.surface())?;
            candidate.records.remove(&old.surface());
            changed.push(old.surface());
        }
        for placement in replacements {
            candidate.replace_surface_placement(binding, placement, &retains_binding)?;
            changed.push(placement.surface());
        }
        candidate.declare_preserved_placements(declarations);
        changed.sort_unstable();
        *self = candidate;
        Ok(changed)
    }

    fn declare_preserved_placements(&mut self, placements: Vec<NativeSurfacePlacement>) {
        // An explicit declaration may match the previous implicit mapping.
        // Keep its identity while freezing the declared geometry across resizes.
        for placement in placements {
            self.records
                .get_mut(&placement.surface())
                .expect("unchanged placements remain in the window roster")
                .placement = Some(placement);
        }
    }

    fn replace_surface_placement(
        &mut self,
        binding: WindowBinding,
        placement: NativeSurfacePlacement,
        retains_binding: impl Fn(ViewportBinding) -> bool,
    ) -> Result<(), ViewportRegistryError> {
        let surface = placement.surface();
        let previous = self.records.get(&surface);
        let previous_admission =
            previous.map_or(ViewportAdmission::Admitted, |record| record.admission);
        if previous.is_none_or(|record| retains_binding(record.binding)) {
            if !self.native_binding_allocation_available() {
                return Err(ViewportRegistryError::NativeBindingCapacity);
            }
            self.commit_native_binding_slot();
        }
        let placement_generation = self
            .last_placement_generation
            .checked_next()
            .ok_or(ViewportRegistryError::PlacementGenerationExhausted)?;
        self.last_placement_generation = placement_generation;
        let exact =
            ViewportBinding::with_placement_generation(binding, surface, placement_generation);
        let generation = self.next_surface_authority(surface)?;
        let coordinates = self
            .windows
            .get(&binding)
            .and_then(|window| window.coordinates)
            .and_then(|window| {
                CoordinateSnapshot::from_window_coordinates(
                    exact,
                    generation,
                    window,
                    SurfaceClientGeometry::new(
                        placement.origin(),
                        placement.scale_factor(),
                        placement.clip(),
                    ),
                )
            });
        self.records.insert(
            surface,
            SurfacePlacementRecord {
                binding: exact,
                admission: previous_admission,
                coordinates,
                recovery_coordinates: coordinates.map(RecoveryCoordinateSnapshot::new),
                coordinate_generation: generation,
                placement: Some(placement),
            },
        );
        Ok(())
    }

    /// Validates the complete ready roster before admitting any member.
    pub(crate) fn admit_window(
        &mut self,
        bindings: &[ViewportBinding],
    ) -> Result<(), ViewportRegistryError> {
        let Some(first) = bindings.first() else {
            return Ok(());
        };
        if self.bindings_for_window(first.window()).collect::<Vec<_>>() != bindings {
            return Err(ViewportRegistryError::BindingNotAdmissible { binding: *first });
        }
        for &binding in bindings {
            let record = self.current_record(binding)?;
            if record.admission() != ViewportAdmission::Pending
                || !record.is_ready()
                || record
                    .presentation_observation()
                    .and_then(WindowPresentationObservation::known_state)
                    != Some(WindowPresentationState::Visible)
            {
                return Err(ViewportRegistryError::BindingNotAdmissible { binding });
            }
        }
        for binding in bindings {
            self.records
                .get_mut(&binding.surface())
                .expect("validated complete roster")
                .admission = ViewportAdmission::Admitted;
        }
        Ok(())
    }

    /// Classifies current registry facts against one frozen native close edge.
    #[must_use]
    pub(crate) fn native_close_edge_disposition(
        &self,
        edge: NativeCloseEdge,
    ) -> NativeCloseEdgeDisposition {
        let Some(window) = self.windows.get(&edge.binding().window()) else {
            return self
                .token_to_window
                .get(&edge.binding().token())
                .and_then(|window| self.windows.get(window))
                .map_or(NativeCloseEdgeDisposition::BindingMissing, |window| {
                    NativeCloseEdgeDisposition::BindingRebound {
                        binding: window.observation_binding,
                    }
                });
        };
        if window.pending_close_request.is_some_and(|observation| {
            observation.binding().window() == edge.binding().window()
                && observation.generation() == edge.observed_at()
                && observation.inventory_generation() == edge.received_at()
                && observation.known_state() == Some(WindowCloseState::LiveRequested)
        }) {
            return NativeCloseEdgeDisposition::ExactPending;
        }
        if window.lifecycle == ViewportLifecycle::Missing {
            return NativeCloseEdgeDisposition::InventoryMissing;
        }
        match window.close_observations.current() {
            Some(observation) => match observation.known_state() {
                Some(WindowCloseState::LiveRequested) => {
                    NativeCloseEdgeDisposition::DifferentPending { observation }
                }
                Some(WindowCloseState::LiveClear) => {
                    NativeCloseEdgeDisposition::LiveClear { observation }
                }
                Some(WindowCloseState::Destroyed) => {
                    NativeCloseEdgeDisposition::Destroyed { observation }
                }
                None => NativeCloseEdgeDisposition::AuthorityUnavailable,
            },
            None => NativeCloseEdgeDisposition::AuthorityUnavailable,
        }
    }

    /// Returns the local association/coordinate authority watermark even while unbound.
    #[must_use]
    pub(crate) fn surface_authority_generation(&self, surface: SurfaceId) -> CoordinateGeneration {
        self.surface_authority_generations
            .get(&surface)
            .copied()
            .unwrap_or_default()
    }

    pub(crate) fn compact_surface_authority_generations(
        &mut self,
        retained_surfaces: &BTreeSet<SurfaceId>,
    ) -> usize {
        let before = self.surface_authority_generations.len();
        self.surface_authority_generations
            .retain(|surface, _| retained_surfaces.contains(surface));
        before - self.surface_authority_generations.len()
    }

    #[cfg(test)]
    pub(crate) fn surface_authority_generation_count(&self) -> usize {
        self.surface_authority_generations.len()
    }

    pub fn records(&self) -> impl Iterator<Item = (SurfaceId, ViewportRecord<'_>)> {
        self.records.iter().filter_map(|(surface, placement)| {
            let window = self.windows.get(&placement.binding.window())?;
            Some((*surface, ViewportRecord { placement, window }))
        })
    }

    #[cfg(test)]
    pub(crate) fn placement(
        &self,
        surface: SurfaceId,
        logical_rect: LogicalRect,
        work_area: ObservedWorkArea,
        work_area_generation: WorkAreaGeneration,
    ) -> Result<ViewportPlacementProof, CoordinateUnavailable> {
        let record = self
            .record(surface)
            .ok_or(CoordinateUnavailable::FactUnavailable {
                fact: crate::platform::coordinates::CoordinateFact::ContentBounds,
                reason: crate::input::intent::AuthorityUnavailableReason::SurfaceUnavailable,
            })?;
        if !record.has_coordinate_authority() {
            return Err(CoordinateUnavailable::FactUnavailable {
                fact: crate::platform::coordinates::CoordinateFact::ContentBounds,
                reason: crate::input::intent::AuthorityUnavailableReason::SurfaceUnavailable,
            });
        }
        let coordinates = record
            .coordinates()
            .ok_or(CoordinateUnavailable::FactUnavailable {
                fact: crate::platform::coordinates::CoordinateFact::ContentBounds,
                reason: crate::input::intent::AuthorityUnavailableReason::SurfaceUnavailable,
            })?;
        coordinates.placement(logical_rect, work_area, work_area_generation)
    }

    #[must_use]
    pub(crate) fn proof_is_current(
        &self,
        proof: &ViewportPlacementProof,
        work_area_generation: WorkAreaGeneration,
    ) -> bool {
        self.record(proof.binding().surface())
            .is_some_and(|record| {
                record.binding() == proof.binding()
                    && proof.is_current(
                        record.binding(),
                        record.coordinate_generation(),
                        work_area_generation,
                    )
                    && record.has_coordinate_authority()
            })
    }

    fn current_record(
        &self,
        binding: ViewportBinding,
    ) -> Result<ViewportRecord<'_>, ViewportRegistryError> {
        let record =
            self.record(binding.surface())
                .ok_or(ViewportRegistryError::MissingSurface {
                    surface: binding.surface(),
                })?;
        if record.binding() != binding {
            return Err(ViewportRegistryError::StaleBinding { binding });
        }
        Ok(record)
    }

    fn current_record_mut(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<(&mut SurfacePlacementRecord, &mut WindowRecord), ViewportRegistryError> {
        let record = self.records.get_mut(&binding.surface()).ok_or(
            ViewportRegistryError::MissingSurface {
                surface: binding.surface(),
            },
        )?;
        if record.binding != binding {
            return Err(ViewportRegistryError::StaleBinding { binding });
        }
        let window = self
            .windows
            .get_mut(&binding.window())
            .ok_or(ViewportRegistryError::StaleBinding { binding })?;
        Ok((record, window))
    }

    #[cfg(test)]
    pub(crate) fn exhaust_inventory_generation(&mut self) {
        self.inventory_generation = InventoryGeneration::new(u64::MAX);
    }

    #[cfg(test)]
    pub(crate) fn exhaust_coordinate_generation(&mut self) {
        self.last_coordinate_generation = CoordinateGeneration::new(u64::MAX);
    }
}

/// Reduces the current authoritative close fact into one edge event.
///
/// The pending request stores the provider observation that opened the edge. Authority gaps,
/// stale envelopes, and repeated generations therefore cannot synthesize a second request.
fn window_has_coordinate_authority(window: &WindowRecord) -> bool {
    matches!(
        window.lifecycle,
        ViewportLifecycle::Ready | ViewportLifecycle::CloseRequested
    ) && window.coordinates.is_some()
        && window
            .presentation_observations
            .current()
            .and_then(WindowPresentationObservation::known_state)
            == Some(WindowPresentationState::Visible)
}

fn reduce_window_close_observation(
    window: &mut WindowRecord,
    events: &mut Vec<RegistryEvent>,
) -> bool {
    let Some(observation) = window.close_observations.current() else {
        return false;
    };
    match observation.known_state() {
        Some(WindowCloseState::LiveRequested) if window.pending_close_request.is_none() => {
            window.pending_close_request = Some(observation);
            events.push(RegistryEvent::CloseRequested { observation });
        }
        Some(WindowCloseState::LiveClear) if window.pending_close_request.is_some() => {
            let requested = window
                .pending_close_request
                .take()
                .expect("guarded pending close request must exist");
            events.push(RegistryEvent::CloseRequestCleared {
                requested,
                observation,
            });
        }
        Some(WindowCloseState::Destroyed)
            if !matches!(window.lifecycle, ViewportLifecycle::Destroyed) =>
        {
            window.pending_close_request = None;
            window.lifecycle = ViewportLifecycle::Destroyed;
            window.ever_observed = true;
            events.push(RegistryEvent::Destroyed { observation });
            return true;
        }
        Some(
            WindowCloseState::LiveClear
            | WindowCloseState::LiveRequested
            | WindowCloseState::Destroyed,
        )
        | None => {}
    }
    false
}

/// Typed registry allocation, identity, or lifecycle failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ViewportRegistryError {
    #[error("native window binding is unknown: {binding:?}")]
    UnknownWindow { binding: WindowBinding },
    #[error(
        "native window cannot change placements during its terminal or close lifecycle: {binding:?}"
    )]
    WindowPlacementUnavailable { binding: WindowBinding },
    #[error("surface placement has invalid transformed layout geometry: {surface:?}")]
    InvalidSurfacePlacement { surface: SurfaceId },
    #[error("surface placement generation is exhausted")]
    PlacementGenerationExhausted,
    #[error("native binding lifetime storage has no available slot")]
    NativeBindingCapacity,
    #[error("logical surface {surface:?} is already registered")]
    SurfaceAlreadyRegistered { surface: SurfaceId },
    #[error("window token {token:?} is already registered")]
    TokenAlreadyRegistered { token: WindowToken },
    #[error("managed window token {token:?} cannot register an external resource")]
    ManagedTokenCannotRegister { token: WindowToken },
    #[error("logical surface {surface:?} is not registered")]
    MissingSurface { surface: SurfaceId },
    #[error("viewport binding is stale: {binding:?}")]
    StaleBinding { binding: ViewportBinding },
    #[error("platform snapshot names an unknown viewport binding: {binding:?}")]
    UnknownObservedBinding { binding: ViewportBinding },
    #[error(
        "platform snapshot names stale viewport binding {observed:?}; current binding is {current:?}"
    )]
    StaleObservedBinding {
        observed: ViewportBinding,
        current: ViewportBinding,
    },
    #[error("viewport binding is not authoritatively destroyed: {binding:?}")]
    BindingNotDestroyed { binding: ViewportBinding },
    #[error("authoritatively destroyed viewport binding reappeared: {binding:?}")]
    DestroyedBindingReappeared { binding: ViewportBinding },
    #[error("viewport binding is already authoritatively destroyed: {binding:?}")]
    BindingAlreadyDestroyed { binding: ViewportBinding },
    #[error("narrow close ingress cannot publish destruction for {binding:?}")]
    NarrowDestroyedObservationForbidden { binding: ViewportBinding },
    #[error("viewport binding was already observed and cannot be discarded: {binding:?}")]
    BindingAlreadyObserved { binding: ViewportBinding },
    #[error("viewport binding is not ready for admission: {binding:?}")]
    BindingNotAdmissible { binding: ViewportBinding },
    #[error("runtime-owned root viewport is forbidden")]
    RuntimeOwnedRootForbidden,
    #[error("window token identity is exhausted")]
    WindowTokenExhausted,
    #[error("window incarnation identity is exhausted")]
    WindowIncarnationExhausted,
    #[error("inventory generation is exhausted")]
    InventoryGenerationExhausted,
    #[error("coordinate generation is exhausted")]
    CoordinateGenerationExhausted,
}

#[cfg(test)]
mod tests;

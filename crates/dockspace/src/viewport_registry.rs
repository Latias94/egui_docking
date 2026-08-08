//! Core-owned registry for stable logical surfaces and ephemeral native bindings.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::close_plan::NativeCloseEdge;
use crate::coordinates::{
    CoordinateSnapshot, CoordinateUnavailable, RecoveryCoordinateSnapshot, ViewportPlacementProof,
};
use crate::geometry::LogicalRect;
use crate::ids::{EngineAuthorityDomainId, SurfaceId, WorkspaceEpoch};
use crate::intent::Authority;
use crate::platform::{
    ObservedWindow, ObservedWorkArea, PlatformSnapshot, WindowCloseObservation,
    WindowCloseObservationStream, WindowCloseState, WindowCoordinateObservationStream,
    WindowInputObservation, WindowInputObservationStream, WindowPresentationObservation,
    WindowPresentationObservationStream, WindowPresentationState,
};
#[cfg(test)]
use crate::viewport::CloseObservationGeneration;
use crate::viewport::{
    CoordinateGeneration, CoordinateObservationGeneration, InputObservationGeneration,
    InventoryGeneration, ViewportBinding, ViewportRole, WindowIncarnation, WindowToken,
    WorkAreaGeneration,
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

/// Queryable registry record for one logical surface.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewportRecord {
    binding: ViewportBinding,
    role: ViewportRole,
    ownership: ViewportOwnership,
    admission: ViewportAdmission,
    lifecycle: ViewportLifecycle,
    coordinates: Option<CoordinateSnapshot>,
    recovery_coordinates: Option<RecoveryCoordinateSnapshot>,
    coordinate_observations: WindowCoordinateObservationStream,
    input_observations: WindowInputObservationStream,
    presentation_observations: WindowPresentationObservationStream,
    close_observations: WindowCloseObservationStream,
    coordinate_generation: CoordinateGeneration,
    ever_observed: bool,
    pending_close_request: Option<WindowCloseObservation>,
}

impl ViewportRecord {
    #[must_use]
    pub const fn binding(&self) -> ViewportBinding {
        self.binding
    }

    #[must_use]
    pub const fn role(&self) -> ViewportRole {
        self.role
    }

    #[must_use]
    pub const fn ownership(&self) -> ViewportOwnership {
        self.ownership
    }

    #[must_use]
    pub const fn admission(&self) -> ViewportAdmission {
        self.admission
    }

    #[must_use]
    pub const fn lifecycle(&self) -> ViewportLifecycle {
        self.lifecycle
    }

    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(self.lifecycle, ViewportLifecycle::Ready)
    }

    /// Returns whether this live incarnation can still authorize its coordinate snapshot.
    ///
    /// A close-request edge disables new pointer routing, but the observed window and its
    /// geometry remain authoritative until close acceptance advances to `AwaitingDestroyed`.
    pub(crate) const fn has_coordinate_authority(&self) -> bool {
        matches!(
            self.lifecycle,
            ViewportLifecycle::Ready | ViewportLifecycle::CloseRequested
        ) && self.coordinates.is_some()
            && matches!(
                self.presentation_observations.current(),
                Some(observation)
                    if matches!(
                        observation.state(),
                        Authority::Known(WindowPresentationState::Visible)
                    )
            )
    }

    /// Returns whether the window is eligible for authoritative pointer routing.
    #[must_use]
    pub(crate) fn is_routeable(&self) -> bool {
        self.admission == ViewportAdmission::Admitted
            && self.is_ready()
            && self
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
    pub(crate) fn can_report_focus_fact(&self) -> bool {
        self.ever_observed
            && matches!(
                self.lifecycle,
                ViewportLifecycle::AwaitingObservation
                    | ViewportLifecycle::Ready
                    | ViewportLifecycle::CloseRequested
            )
            && self
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
    pub(crate) fn can_observe_focus(&self) -> bool {
        self.admission == ViewportAdmission::Admitted && self.can_report_focus_fact()
    }

    /// Returns whether this exact incarnation may accept a new activation.
    ///
    /// A close-requested window may still report the focus it already owns,
    /// but new focus commands would race its terminal lifecycle.
    pub(crate) fn can_accept_activation(&self) -> bool {
        self.admission == ViewportAdmission::Admitted
            && self.ever_observed
            && matches!(
                self.lifecycle,
                ViewportLifecycle::AwaitingObservation | ViewportLifecycle::Ready
            )
            && self
                .presentation_observations
                .current()
                .is_some_and(|observation| {
                    observation.known_state() == Some(WindowPresentationState::Visible)
                })
    }

    pub(crate) const fn coordinates(&self) -> Option<CoordinateSnapshot> {
        self.coordinates
    }

    /// Returns the last exact-binding coordinates retained only for destruction recovery.
    ///
    /// This is historical placement data, not current route, hit-test, or scene authority.
    pub(crate) const fn recovery_coordinates(&self) -> Option<RecoveryCoordinateSnapshot> {
        self.recovery_coordinates
    }

    pub(crate) const fn coordinate_generation(&self) -> CoordinateGeneration {
        self.coordinate_generation
    }

    #[cfg(test)]
    pub(crate) const fn coordinate_observation_generation_watermark(
        &self,
    ) -> Option<CoordinateObservationGeneration> {
        self.coordinate_observations.generation_watermark()
    }

    pub(crate) const fn coordinate_observation_generation(
        &self,
    ) -> Option<CoordinateObservationGeneration> {
        match self.coordinates {
            Some(coordinates) => Some(coordinates.observation_generation()),
            None => None,
        }
    }

    pub(crate) const fn input_observation(&self) -> Option<WindowInputObservation> {
        self.input_observations.current()
    }

    pub(crate) const fn input_observations(&self) -> WindowInputObservationStream {
        self.input_observations
    }

    pub(crate) const fn input_observation_generation_watermark(
        &self,
    ) -> Option<InputObservationGeneration> {
        self.input_observations.generation_watermark()
    }

    pub(crate) const fn presentation_observation(&self) -> Option<WindowPresentationObservation> {
        self.presentation_observations.current()
    }

    pub(crate) const fn close_observations(&self) -> WindowCloseObservationStream {
        self.close_observations
    }

    pub(crate) const fn ever_observed(&self) -> bool {
        self.ever_observed
    }

    pub(crate) const fn is_observed(&self) -> bool {
        self.ever_observed
            && !matches!(
                self.lifecycle,
                ViewportLifecycle::Missing | ViewportLifecycle::Destroyed
            )
    }

    /// Returns whether this record still owns the exact native close edge.
    ///
    /// Matching the binding alone is intentionally insufficient: a clear followed by a new
    /// request on the same native window is a different close obligation.
    pub(crate) fn matches_pending_close_edge(&self, edge: NativeCloseEdge) -> bool {
        self.binding == edge.binding()
            && self.pending_close_request.is_some_and(|observation| {
                observation.binding() == edge.binding()
                    && observation.generation() == edge.observed_at()
                    && observation.inventory_generation() == edge.received_at()
                    && observation.known_state() == Some(WindowCloseState::LiveRequested)
            })
    }

    /// Classifies current close facts against one frozen native close edge.
    pub(crate) fn native_close_edge_disposition(
        &self,
        edge: NativeCloseEdge,
    ) -> NativeCloseEdgeDisposition {
        if self.binding != edge.binding() {
            return NativeCloseEdgeDisposition::BindingRebound {
                binding: self.binding,
            };
        }
        if self.matches_pending_close_edge(edge) {
            return NativeCloseEdgeDisposition::ExactPending;
        }
        if matches!(self.lifecycle, ViewportLifecycle::Missing) {
            return NativeCloseEdgeDisposition::InventoryMissing;
        }
        let Some(observation) = self.close_observations.current() else {
            return NativeCloseEdgeDisposition::AuthorityUnavailable;
        };
        match observation.known_state() {
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
        }
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
}

impl DetachedViewportFacts {
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
    #[must_use]
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
    records: BTreeMap<SurfaceId, ViewportRecord>,
    token_to_surface: BTreeMap<WindowToken, SurfaceId>,
    last_token: WindowToken,
    last_incarnation: WindowIncarnation,
    inventory_generation: InventoryGeneration,
    last_coordinate_generation: CoordinateGeneration,
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
            token_to_surface: BTreeMap::new(),
            last_token: WindowToken::new(0),
            last_incarnation: WindowIncarnation::new(0),
            inventory_generation: InventoryGeneration::new(0),
            last_coordinate_generation: CoordinateGeneration::new(0),
            surface_authority_generations: BTreeMap::new(),
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
        admission: ViewportAdmission,
    ) -> Result<ViewportBinding, ViewportRegistryError> {
        if self.records.contains_key(&surface) {
            return Err(ViewportRegistryError::SurfaceAlreadyRegistered { surface });
        }
        if self.token_to_surface.contains_key(&token) {
            return Err(ViewportRegistryError::TokenAlreadyRegistered { token });
        }
        let incarnation = self.next_incarnation()?;
        self.last_token = WindowToken::new(self.last_token.get().max(token.get()));
        self.insert_binding(
            ViewportBinding::new(self.authority_domain, epoch, surface, token, incarnation),
            role,
            ViewportOwnership::External,
            admission,
        )
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
        if self.token_to_surface.contains_key(&binding.token()) {
            return Err(ViewportRegistryError::TokenAlreadyRegistered {
                token: binding.token(),
            });
        }
        let coordinate_generation = self.next_surface_authority(binding.surface())?;
        self.token_to_surface
            .insert(binding.token(), binding.surface());
        self.records.insert(
            binding.surface(),
            ViewportRecord {
                binding,
                role,
                ownership,
                admission,
                lifecycle: ViewportLifecycle::AwaitingObservation,
                coordinates: None,
                recovery_coordinates: None,
                coordinate_observations: WindowCoordinateObservationStream::default(),
                input_observations: WindowInputObservationStream::default(),
                presentation_observations: WindowPresentationObservationStream::default(),
                close_observations: WindowCloseObservationStream::default(),
                coordinate_generation,
                ever_observed: false,
                pending_close_request: None,
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
        destroyed_tombstones: &BTreeSet<ViewportBinding>,
    ) -> Result<RegistryTransition, ViewportRegistryError> {
        let mut candidate = self.clone();
        let transition = candidate.apply_snapshot_in_place(
            snapshot,
            inventory,
            retired_bindings,
            destroyed_tombstones,
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
        let record = self.current_record_mut(binding)?;
        if matches!(record.lifecycle, ViewportLifecycle::Destroyed) {
            return Err(ViewportRegistryError::BindingAlreadyDestroyed { binding });
        }

        record
            .close_observations
            .observe(binding, Some(observation));
        let mut events = Vec::new();
        let destroyed = reduce_close_observation(record, &mut events);
        debug_assert!(!destroyed, "narrow close ingress rejects destroyed facts");
        if record.pending_close_request.is_some() {
            record.lifecycle = ViewportLifecycle::CloseRequested;
        } else if record.admission == ViewportAdmission::Retiring {
            record.lifecycle = ViewportLifecycle::AwaitingDestroyed;
        } else if record.coordinates.is_some() {
            record.lifecycle = ViewportLifecycle::Ready;
        } else {
            record.lifecycle = ViewportLifecycle::AwaitingObservation;
        }
        self.inventory_generation = generation;
        Ok(RegistryTransition { generation, events })
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
    pub(crate) fn revoke_live_provider_authority(
        &mut self,
    ) -> Result<RegistryTransition, ViewportRegistryError> {
        let mut candidate = self.clone();
        let transition = candidate.revoke_live_provider_authority_in_place()?;
        *self = candidate;
        Ok(transition)
    }

    fn revoke_live_provider_authority_in_place(
        &mut self,
    ) -> Result<RegistryTransition, ViewportRegistryError> {
        let mut events = Vec::new();
        let mut coordinate_authority_lost = Vec::new();

        for (surface, record) in &mut self.records {
            if matches!(record.lifecycle, ViewportLifecycle::Destroyed) {
                continue;
            }

            let target_lifecycle = if record.admission == ViewportAdmission::Retiring {
                ViewportLifecycle::AwaitingDestroyed
            } else {
                ViewportLifecycle::AwaitingObservation
            };
            let record_changed = record.lifecycle != target_lifecycle
                || record.coordinates.is_some()
                || record.coordinate_observations != WindowCoordinateObservationStream::default()
                || record.input_observations != WindowInputObservationStream::default()
                || record.presentation_observations
                    != WindowPresentationObservationStream::default()
                || record.close_observations != WindowCloseObservationStream::default()
                || record.ever_observed
                || record.pending_close_request.is_some();
            if !record_changed {
                continue;
            }

            let coordinate_authority_was_live = record.has_coordinate_authority();
            let live_semantics_changed = record.lifecycle != target_lifecycle
                || coordinate_authority_was_live
                || record.ever_observed
                || record.pending_close_request.is_some()
                || record.coordinate_observations.current().is_some()
                || record.input_observations.current().is_some()
                || record.presentation_observations.current().is_some()
                || record.close_observations.current().is_some();
            if coordinate_authority_was_live {
                coordinate_authority_lost.push(*surface);
            }
            record.lifecycle = target_lifecycle;
            record.coordinates = None;
            record.coordinate_observations = WindowCoordinateObservationStream::default();
            record.input_observations = WindowInputObservationStream::default();
            record.presentation_observations = WindowPresentationObservationStream::default();
            record.close_observations = WindowCloseObservationStream::default();
            record.ever_observed = false;
            record.pending_close_request = None;
            if live_semantics_changed {
                events.push(RegistryEvent::FactsUnavailable {
                    binding: record.binding,
                });
            }
        }

        if events.is_empty() {
            return Ok(RegistryTransition {
                generation: self.inventory_generation,
                events,
            });
        }

        for surface in coordinate_authority_lost {
            let generation = self.next_surface_authority(surface)?;
            let record = self
                .records
                .get_mut(&surface)
                .ok_or(ViewportRegistryError::MissingSurface { surface })?;
            record.coordinate_generation = generation;
        }
        let generation = self
            .inventory_generation
            .checked_next()
            .ok_or(ViewportRegistryError::InventoryGenerationExhausted)?;
        self.inventory_generation = generation;

        Ok(RegistryTransition { generation, events })
    }

    fn apply_snapshot_in_place(
        &mut self,
        snapshot: &PlatformSnapshot,
        inventory: Option<&[ViewportBinding]>,
        retired_bindings: &BTreeSet<ViewportBinding>,
        destroyed_tombstones: &BTreeSet<ViewportBinding>,
    ) -> Result<RegistryTransition, ViewportRegistryError> {
        self.validate_snapshot_bindings(snapshot, retired_bindings, destroyed_tombstones)?;
        let generation = self
            .inventory_generation
            .checked_next()
            .ok_or(ViewportRegistryError::InventoryGenerationExhausted)?;
        let authoritative = inventory.is_some();
        let observations: BTreeMap<ViewportBinding, &ObservedWindow> = snapshot
            .window_observations()
            .iter()
            .map(|window| (window.binding(), window))
            .collect();
        let close_observations = snapshot
            .close_observations()
            .iter()
            .map(|observation| (observation.binding(), *observation))
            .collect::<BTreeMap<_, _>>();
        let mut events = Vec::new();
        let mut last_coordinate_generation = self.last_coordinate_generation;
        let mut surface_authority_generations = self.surface_authority_generations.clone();

        for (surface, record) in &mut self.records {
            let previous_lifecycle = record.lifecycle;
            let window_observation = observations.get(&record.binding).copied();
            if matches!(record.lifecycle, ViewportLifecycle::Destroyed)
                && window_observation.is_some()
            {
                return Err(ViewportRegistryError::DestroyedBindingReappeared {
                    binding: record.binding,
                });
            }

            let close_observation = close_observations
                .get(&record.binding)
                .copied()
                .map(|observation| observation.with_inventory_generation(generation));
            record
                .close_observations
                .observe(record.binding, close_observation);
            if reduce_close_observation(record, &mut events) {
                let authority = last_coordinate_generation
                    .checked_next()
                    .ok_or(ViewportRegistryError::CoordinateGenerationExhausted)?;
                last_coordinate_generation = authority;
                record.coordinate_generation = authority;
                surface_authority_generations.insert(*surface, authority);
                continue;
            }

            let Some(observation) = window_observation else {
                if authoritative
                    && record.ever_observed
                    && !matches!(
                        record.lifecycle,
                        ViewportLifecycle::Missing | ViewportLifecycle::Destroyed
                    )
                {
                    record.lifecycle = ViewportLifecycle::Missing;
                    events.push(RegistryEvent::BindingMissing {
                        binding: record.binding,
                    });
                    let authority = last_coordinate_generation
                        .checked_next()
                        .ok_or(ViewportRegistryError::CoordinateGenerationExhausted)?;
                    last_coordinate_generation = authority;
                    record.coordinate_generation = authority;
                    surface_authority_generations.insert(*surface, authority);
                }
                continue;
            };

            record.ever_observed = true;
            let awaiting_destroyed = record.admission == ViewportAdmission::Retiring;
            let was_ready = matches!(previous_lifecycle, ViewportLifecycle::Ready);
            let previous_coordinates = record.coordinates;
            let previous_presentation = record.presentation_observations.current();
            record
                .coordinate_observations
                .observe(record.binding, observation.coordinate_observation());
            record
                .input_observations
                .observe(record.binding, observation.input_observation());
            let presentation_observation = observation
                .presentation_observation()
                .map(|observation| observation.with_inventory_generation(generation));
            record
                .presentation_observations
                .observe(record.binding, presentation_observation);
            let presentation_changed =
                previous_presentation != record.presentation_observations.current();
            let coordinates = record
                .coordinate_observations
                .current()
                .and_then(|observation| {
                    CoordinateSnapshot::from_observation(
                        record.binding,
                        record.coordinate_generation,
                        observation,
                    )
                    .ok()
                });
            let facts_changed = match (previous_coordinates, coordinates) {
                (Some(previous), Some(current)) => !previous.same_facts(current),
                (None, None) => false,
                (Some(_), None) | (None, Some(_)) => true,
            };
            record.coordinates = coordinates;
            if awaiting_destroyed {
                record.lifecycle = ViewportLifecycle::AwaitingDestroyed;
            } else {
                if record.pending_close_request.is_some() {
                    record.lifecycle = ViewportLifecycle::CloseRequested;
                } else if record.coordinates.is_some() {
                    record.lifecycle = ViewportLifecycle::Ready;
                    if !was_ready || facts_changed {
                        events.push(RegistryEvent::Ready {
                            binding: record.binding,
                        });
                    }
                } else {
                    record.lifecycle = ViewportLifecycle::AwaitingObservation;
                    if was_ready || facts_changed {
                        events.push(RegistryEvent::FactsUnavailable {
                            binding: record.binding,
                        });
                    }
                }
            }
            let previous_coordinate_authority = matches!(
                previous_lifecycle,
                ViewportLifecycle::Ready | ViewportLifecycle::CloseRequested
            ) && previous_coordinates.is_some()
                && previous_presentation.is_some_and(|observation| {
                    matches!(
                        observation.state(),
                        Authority::Known(WindowPresentationState::Visible)
                    )
                });
            let current_coordinate_authority = record.has_coordinate_authority();
            if facts_changed || previous_coordinate_authority != current_coordinate_authority {
                let authority = last_coordinate_generation
                    .checked_next()
                    .ok_or(ViewportRegistryError::CoordinateGenerationExhausted)?;
                last_coordinate_generation = authority;
                record.coordinate_generation = authority;
                record.coordinates = record
                    .coordinates
                    .map(|snapshot| snapshot.with_generation(authority));
                surface_authority_generations.insert(*surface, authority);
            }
            if presentation_changed {
                events.push(RegistryEvent::PresentationChanged {
                    binding: record.binding,
                });
            }
            if record.has_coordinate_authority()
                && let Some(coordinates) = record.coordinates
            {
                record.recovery_coordinates = Some(record.recovery_coordinates.map_or_else(
                    || RecoveryCoordinateSnapshot::new(coordinates),
                    |recovery| recovery.observe(coordinates),
                ));
            }
        }

        self.inventory_generation = generation;
        self.last_coordinate_generation = last_coordinate_generation;
        self.surface_authority_generations = surface_authority_generations;
        Ok(RegistryTransition { generation, events })
    }

    fn validate_snapshot_bindings(
        &self,
        snapshot: &PlatformSnapshot,
        retired_bindings: &BTreeSet<ViewportBinding>,
        destroyed_tombstones: &BTreeSet<ViewportBinding>,
    ) -> Result<(), ViewportRegistryError> {
        for window in snapshot.window_observations() {
            self.validate_live_observed_binding(window.binding(), retired_bindings)?;
        }
        for observation in snapshot.close_observations() {
            self.validate_close_observed_binding(
                *observation,
                retired_bindings,
                destroyed_tombstones,
            )?;
        }
        Ok(())
    }

    fn validate_live_observed_binding(
        &self,
        binding: ViewportBinding,
        retired_bindings: &BTreeSet<ViewportBinding>,
    ) -> Result<(), ViewportRegistryError> {
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
        destroyed_tombstones: &BTreeSet<ViewportBinding>,
    ) -> Result<(), ViewportRegistryError> {
        if observation.known_state() == Some(WindowCloseState::Destroyed)
            && destroyed_tombstones.contains(&observation.binding())
        {
            return Ok(());
        }
        self.validate_live_observed_binding(observation.binding(), retired_bindings)
    }

    pub(crate) fn mark_awaiting_destroyed(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<(), ViewportRegistryError> {
        if matches!(
            self.current_record(binding)?.lifecycle,
            ViewportLifecycle::Destroyed
        ) {
            return Err(ViewportRegistryError::BindingAlreadyDestroyed { binding });
        }
        let generation = self.next_surface_authority(binding.surface())?;
        let record = self.current_record_mut(binding)?;
        record.lifecycle = ViewportLifecycle::AwaitingDestroyed;
        record.admission = ViewportAdmission::Retiring;
        record.coordinate_generation = generation;
        record.coordinates = record
            .coordinates
            .map(|snapshot| snapshot.with_generation(generation));
        Ok(())
    }

    pub(crate) fn resume_after_failed_close(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<(), ViewportRegistryError> {
        let lifecycle = {
            let record = self.current_record(binding)?;
            if matches!(record.lifecycle, ViewportLifecycle::Destroyed) {
                return Err(ViewportRegistryError::BindingAlreadyDestroyed { binding });
            }
            if record.pending_close_request.is_some() {
                ViewportLifecycle::CloseRequested
            } else if record.coordinates.is_some() {
                ViewportLifecycle::Ready
            } else {
                ViewportLifecycle::AwaitingObservation
            }
        };
        let generation = self.next_surface_authority(binding.surface())?;
        let record = self.current_record_mut(binding)?;
        record.lifecycle = lifecycle;
        record.admission = ViewportAdmission::Admitted;
        record.coordinate_generation = generation;
        record.coordinates = record
            .coordinates
            .map(|snapshot| snapshot.with_generation(generation));
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
            record.admission == ViewportAdmission::Retiring
                && matches!(record.lifecycle, ViewportLifecycle::AwaitingDestroyed)
                && record.pending_close_request.is_none()
                && record
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
        let record = self.current_record_mut(binding)?;
        record.lifecycle = if record.coordinates.is_some() {
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
    pub(crate) fn admit(&mut self, binding: ViewportBinding) -> Result<(), ViewportRegistryError> {
        let record = self.current_record_mut(binding)?;
        if record.admission != ViewportAdmission::Pending
            || !matches!(record.lifecycle, ViewportLifecycle::Ready)
            || record
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
        self.token_to_surface.remove(&binding.token());
        Ok(DetachedViewportFacts {
            binding: record.binding,
            role: record.role,
            ownership: record.ownership,
            lifecycle: record.lifecycle,
            input_observations: record.input_observations,
            close_observations: record.close_observations,
            ever_observed: record.ever_observed,
        })
    }

    pub(crate) fn remove_destroyed(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<(), ViewportRegistryError> {
        let record = self.current_record(binding)?;
        if !matches!(record.lifecycle, ViewportLifecycle::Destroyed) {
            return Err(ViewportRegistryError::BindingNotDestroyed { binding });
        }
        self.next_surface_authority(binding.surface())?;
        self.records.remove(&binding.surface());
        self.token_to_surface.remove(&binding.token());
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
        if record.ever_observed {
            return Err(ViewportRegistryError::BindingAlreadyObserved { binding });
        }
        self.next_surface_authority(binding.surface())?;
        self.records.remove(&binding.surface());
        self.token_to_surface.remove(&binding.token());
        Ok(())
    }

    pub(crate) fn reconcile_restore(
        &mut self,
        new_epoch: WorkspaceEpoch,
        retained_bindings: &BTreeSet<ViewportBinding>,
    ) -> Result<RestoreRegistryReconciliation, ViewportRegistryError> {
        let mut candidate = self.clone();
        let reconciliation = candidate.reconcile_restore_in_place(new_epoch, retained_bindings)?;
        *self = candidate;
        Ok(reconciliation)
    }

    fn reconcile_restore_in_place(
        &mut self,
        new_epoch: WorkspaceEpoch,
        retained_bindings: &BTreeSet<ViewportBinding>,
    ) -> Result<RestoreRegistryReconciliation, ViewportRegistryError> {
        for binding in retained_bindings {
            self.current_record(*binding)?;
        }

        let surfaces: Vec<SurfaceId> = self.records.keys().copied().collect();
        let mut retained = Vec::new();
        let mut retired = Vec::new();
        for surface in surfaces {
            let before = self
                .records
                .get(&surface)
                .ok_or(ViewportRegistryError::MissingSurface { surface })?
                .binding;
            if !retained_bindings.contains(&before) {
                retired.push(self.detach(before)?);
                continue;
            }
            let incarnation = self.next_incarnation()?;
            let generation = self.next_surface_authority(surface)?;
            let record = self
                .records
                .get_mut(&surface)
                .ok_or(ViewportRegistryError::MissingSurface { surface })?;
            let after = ViewportBinding::new(
                self.authority_domain,
                new_epoch,
                surface,
                before.token(),
                incarnation,
            );
            record.binding = after;
            record.lifecycle = ViewportLifecycle::AwaitingObservation;
            record.coordinates = None;
            record.recovery_coordinates = None;
            record.coordinate_observations = WindowCoordinateObservationStream::default();
            record.coordinate_generation = generation;
            record.input_observations = WindowInputObservationStream::default();
            record.presentation_observations = WindowPresentationObservationStream::default();
            record.close_observations = WindowCloseObservationStream::default();
            record.ever_observed = false;
            record.pending_close_request = None;
            retained.push((before, after));
        }
        Ok(RestoreRegistryReconciliation { retained, retired })
    }

    #[must_use]
    pub const fn inventory_generation(&self) -> InventoryGeneration {
        self.inventory_generation
    }

    #[must_use]
    pub fn record(&self, surface: SurfaceId) -> Option<&ViewportRecord> {
        self.records.get(&surface)
    }

    /// Classifies current registry facts against one frozen native close edge.
    #[must_use]
    pub(crate) fn native_close_edge_disposition(
        &self,
        edge: NativeCloseEdge,
    ) -> NativeCloseEdgeDisposition {
        self.record(edge.binding().surface())
            .map_or(NativeCloseEdgeDisposition::BindingMissing, |record| {
                record.native_close_edge_disposition(edge)
            })
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

    pub(crate) fn surface_authority_generation_count(&self) -> usize {
        self.surface_authority_generations.len()
    }

    pub fn records(&self) -> impl Iterator<Item = (SurfaceId, &ViewportRecord)> {
        self.records
            .iter()
            .map(|(surface, record)| (*surface, record))
    }

    pub(crate) fn placement(
        &self,
        surface: SurfaceId,
        logical_rect: LogicalRect,
        work_area: ObservedWorkArea,
        work_area_generation: WorkAreaGeneration,
    ) -> Result<ViewportPlacementProof, CoordinateUnavailable> {
        let record = self
            .records
            .get(&surface)
            .ok_or(CoordinateUnavailable::FactUnavailable {
                fact: crate::coordinates::CoordinateFact::ContentBounds,
                reason: crate::intent::AuthorityUnavailableReason::SurfaceUnavailable,
            })?;
        if !record.has_coordinate_authority() {
            return Err(CoordinateUnavailable::FactUnavailable {
                fact: crate::coordinates::CoordinateFact::ContentBounds,
                reason: crate::intent::AuthorityUnavailableReason::SurfaceUnavailable,
            });
        }
        let coordinates = record
            .coordinates()
            .ok_or(CoordinateUnavailable::FactUnavailable {
                fact: crate::coordinates::CoordinateFact::ContentBounds,
                reason: crate::intent::AuthorityUnavailableReason::SurfaceUnavailable,
            })?;
        coordinates.placement(logical_rect, work_area, work_area_generation)
    }

    #[must_use]
    pub(crate) fn proof_is_current(
        &self,
        proof: &ViewportPlacementProof,
        work_area_generation: WorkAreaGeneration,
    ) -> bool {
        self.records
            .get(&proof.binding().surface())
            .is_some_and(|record| {
                record.binding == proof.binding()
                    && proof.is_current(
                        record.binding,
                        record.coordinate_generation,
                        work_area_generation,
                    )
                    && record.has_coordinate_authority()
            })
    }

    fn current_record(
        &self,
        binding: ViewportBinding,
    ) -> Result<&ViewportRecord, ViewportRegistryError> {
        let record =
            self.records
                .get(&binding.surface())
                .ok_or(ViewportRegistryError::MissingSurface {
                    surface: binding.surface(),
                })?;
        if record.binding != binding {
            return Err(ViewportRegistryError::StaleBinding { binding });
        }
        Ok(record)
    }

    fn current_record_mut(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<&mut ViewportRecord, ViewportRegistryError> {
        let record = self.records.get_mut(&binding.surface()).ok_or(
            ViewportRegistryError::MissingSurface {
                surface: binding.surface(),
            },
        )?;
        if record.binding != binding {
            return Err(ViewportRegistryError::StaleBinding { binding });
        }
        Ok(record)
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
fn reduce_close_observation(record: &mut ViewportRecord, events: &mut Vec<RegistryEvent>) -> bool {
    let Some(observation) = record.close_observations.current() else {
        return false;
    };
    match observation.known_state() {
        Some(WindowCloseState::LiveRequested) if record.pending_close_request.is_none() => {
            record.pending_close_request = Some(observation);
            events.push(RegistryEvent::CloseRequested { observation });
        }
        Some(WindowCloseState::LiveClear) if record.pending_close_request.is_some() => {
            let requested = record
                .pending_close_request
                .take()
                .expect("guarded pending close request must exist");
            events.push(RegistryEvent::CloseRequestCleared {
                requested,
                observation,
            });
        }
        Some(WindowCloseState::Destroyed)
            if !matches!(record.lifecycle, ViewportLifecycle::Destroyed) =>
        {
            record.pending_close_request = None;
            record.lifecycle = ViewportLifecycle::Destroyed;
            record.admission = ViewportAdmission::Retiring;
            record.ever_observed = true;
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
    #[error("logical surface {surface:?} is already registered")]
    SurfaceAlreadyRegistered { surface: SurfaceId },
    #[error("window token {token:?} is already registered")]
    TokenAlreadyRegistered { token: WindowToken },
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
mod tests {
    use super::*;
    use crate::geometry::{LogicalRect, PhysicalRect, ScaleFactor};
    use crate::intent::AuthorityUnavailableReason;
    use crate::platform::{
        CapabilityRosterObservation, CloseEffectAcknowledgement, ObservedWorkArea,
        PlatformCapabilities, PlatformCapability, PresentationEffectAcknowledgement,
        WindowCloseObservation, WindowCloseState, WindowCoordinateObservation, WindowInputState,
        WindowInventoryObservation, WindowPresentationObservation, WindowPresentationState,
        WorkAreaRosterObservation,
    };
    use crate::viewport::{
        CapabilityObservationGeneration, CoordinateObservationGeneration,
        InventoryObservationGeneration, WorkAreaGeneration, WorkAreaObservationGeneration,
        WorkAreaToken,
    };
    use crate::viewport_focus::{FocusObservationGeneration, unknown_focus_observation};

    fn ready_window(binding: ViewportBinding, generation: u64) -> ObservedWindow {
        ObservedWindow::new(binding)
            .with_coordinate_observation(WindowCoordinateObservation::new(
                binding,
                CoordinateObservationGeneration::new(generation),
                Authority::Known(
                    PhysicalRect::new(0.0, 0.0, 100.0, 100.0).expect("test bounds must be valid"),
                ),
                Authority::Known(
                    PhysicalRect::new(-5.0, -25.0, 110.0, 130.0)
                        .expect("test bounds must be valid"),
                ),
                Authority::Known(ScaleFactor::new(1.0).expect("test scale must be valid")),
                Authority::Known(ScaleFactor::new(1.0).expect("test scale must be valid")),
            ))
            .with_input_state(Authority::Known(WindowInputState::ReceivesInput))
    }

    fn close_observation(
        binding: ViewportBinding,
        generation: u64,
        state: WindowCloseState,
    ) -> WindowCloseObservation {
        WindowCloseObservation::new(
            binding,
            CloseObservationGeneration::new(generation),
            Authority::Known(state),
            CloseEffectAcknowledgement::known(None),
        )
    }

    fn ready_window_with_presentation(
        binding: ViewportBinding,
        generation: u64,
        state: WindowPresentationState,
    ) -> ObservedWindow {
        ready_window(binding, generation).with_presentation_observation(presentation_observation(
            binding,
            generation,
            Authority::Known(state),
        ))
    }

    fn presentation_observation(
        binding: ViewportBinding,
        generation: u64,
        state: Authority<WindowPresentationState>,
    ) -> WindowPresentationObservation {
        WindowPresentationObservation::new(
            binding,
            crate::viewport::PresentationObservationGeneration::new(generation),
            state,
            PresentationEffectAcknowledgement::known(None),
        )
    }

    fn snapshot(provider_generation: u64, windows: Vec<ObservedWindow>) -> PlatformSnapshot {
        snapshot_with_close(provider_generation, windows, Vec::new())
    }

    fn snapshot_with_close(
        provider_generation: u64,
        windows: Vec<ObservedWindow>,
        close_observations: Vec<WindowCloseObservation>,
    ) -> PlatformSnapshot {
        let mut capabilities = PlatformCapabilities::default();
        capabilities.set_authoritative_inventory(PlatformCapability::Supported);
        let inventory = windows.iter().map(ObservedWindow::binding).collect();
        PlatformSnapshot::new(
            crate::viewport::PlatformSnapshotGeneration::new(provider_generation),
            CapabilityRosterObservation::new(
                CapabilityObservationGeneration::new(provider_generation),
                Authority::Known(capabilities),
            ),
            unknown_focus_observation(
                FocusObservationGeneration::new(provider_generation),
                AuthorityUnavailableReason::NotReported,
            ),
            WindowInventoryObservation::new(
                InventoryObservationGeneration::new(provider_generation),
                Authority::Known(inventory),
            )
            .expect("test inventory must be canonical"),
            windows,
            close_observations,
            WorkAreaRosterObservation::new(
                WorkAreaObservationGeneration::new(provider_generation),
                Authority::Unknown(AuthorityUnavailableReason::NotReported),
            )
            .expect("test work-area tombstone must be canonical"),
        )
        .expect("test snapshot must be valid")
    }

    fn apply_snapshot_with_retired_bindings_for_test(
        registry: &mut ViewportRegistry,
        snapshot: &PlatformSnapshot,
        retired_bindings: &BTreeSet<ViewportBinding>,
        destroyed_tombstones: &BTreeSet<ViewportBinding>,
    ) -> Result<RegistryTransition, ViewportRegistryError> {
        registry.apply_snapshot_with_retired_bindings(
            snapshot,
            snapshot.inventory_observation().known_roster(),
            retired_bindings,
            destroyed_tombstones,
        )
    }

    fn assert_live_provider_streams_are_revoked(record: &ViewportRecord) {
        assert!(record.coordinates.is_none());
        assert_eq!(
            record.coordinate_observations,
            WindowCoordinateObservationStream::default()
        );
        assert_eq!(
            record.input_observations,
            WindowInputObservationStream::default()
        );
        assert_eq!(
            record.presentation_observations,
            WindowPresentationObservationStream::default()
        );
        assert_eq!(
            record.close_observations,
            WindowCloseObservationStream::default()
        );
        assert!(!record.ever_observed);
        assert!(record.pending_close_request.is_none());
    }

    #[test]
    fn token_reuse_requires_a_new_incarnation() {
        let mut registry = ViewportRegistry::default();
        let token = WindowToken::new(8);
        let first = registry
            .register_existing(
                WorkspaceEpoch::new(0),
                SurfaceId::new(1),
                token,
                ViewportRole::Root,
            )
            .expect("first binding must register");
        registry
            .apply_snapshot_for_test(&snapshot(1, vec![ready_window(first, 1)]))
            .expect("ready snapshot must apply");
        registry
            .apply_snapshot_for_test(&snapshot(2, Vec::new()))
            .expect("missing snapshot must apply");
        assert_eq!(
            registry.remove_destroyed(first),
            Err(ViewportRegistryError::BindingNotDestroyed { binding: first })
        );
        registry
            .apply_snapshot_for_test(&snapshot_with_close(
                3,
                Vec::new(),
                vec![close_observation(first, 1, WindowCloseState::Destroyed)],
            ))
            .expect("typed destruction must apply");
        registry
            .remove_destroyed(first)
            .expect("destroyed binding can be removed");
        let second = registry
            .register_existing(
                WorkspaceEpoch::new(0),
                SurfaceId::new(1),
                token,
                ViewportRole::Child,
            )
            .expect("reused token must register after destruction");

        assert_ne!(first.incarnation(), second.incarnation());
    }

    #[test]
    fn foreign_authority_domain_observation_is_rejected_atomically() {
        let first_domain = EngineAuthorityDomainId::new_for_test(41);
        let second_domain = EngineAuthorityDomainId::new_for_test(42);
        let mut first = ViewportRegistry::new(first_domain);
        let mut second = ViewportRegistry::new(second_domain);
        let epoch = WorkspaceEpoch::new(3);
        let surface = SurfaceId::new(5);
        let token = WindowToken::new(7);
        let first_binding = first
            .register_existing(epoch, surface, token, ViewportRole::Root)
            .expect("first binding must register");
        let second_binding = second
            .register_existing(epoch, surface, token, ViewportRole::Root)
            .expect("second binding must register");

        assert_eq!(first_binding.epoch(), second_binding.epoch());
        assert_eq!(first_binding.surface(), second_binding.surface());
        assert_eq!(first_binding.token(), second_binding.token());
        assert_eq!(first_binding.incarnation(), second_binding.incarnation());
        assert_ne!(first_binding, second_binding);

        let before = second.clone();
        assert_eq!(
            second.apply_snapshot_for_test(&snapshot(1, vec![ready_window(first_binding, 1)])),
            Err(ViewportRegistryError::StaleObservedBinding {
                observed: first_binding,
                current: second_binding,
            })
        );
        assert_eq!(second, before);
    }

    #[test]
    fn delayed_geometry_from_a_reused_token_cannot_mutate_the_new_binding() {
        let mut registry = ViewportRegistry::default();
        let token = WindowToken::new(8);
        let first = registry
            .register_existing(
                WorkspaceEpoch::new(0),
                SurfaceId::new(1),
                token,
                ViewportRole::Root,
            )
            .expect("first binding must register");
        registry
            .apply_snapshot_for_test(&snapshot(1, vec![ready_window(first, 1)]))
            .expect("first binding must become ready");
        registry
            .apply_snapshot_for_test(&snapshot_with_close(
                2,
                Vec::new(),
                vec![close_observation(first, 1, WindowCloseState::Destroyed)],
            ))
            .expect("first binding must be destroyed");
        registry
            .remove_destroyed(first)
            .expect("destroyed binding must be removable");
        let second = registry
            .register_existing(
                WorkspaceEpoch::new(1),
                SurfaceId::new(1),
                token,
                ViewportRole::Child,
            )
            .expect("reused token must allocate a new binding");
        let before = registry.clone();

        assert_eq!(
            registry.apply_snapshot_for_test(&snapshot(3, vec![ready_window(first, 3)])),
            Err(ViewportRegistryError::StaleObservedBinding {
                observed: first,
                current: second,
            })
        );
        assert_eq!(registry, before);
        assert!(
            registry
                .record(second.surface())
                .expect("new binding must remain registered")
                .coordinates()
                .is_none(),
            "late geometry must not become coordinate authority for the new binding"
        );

        registry
            .apply_snapshot_for_test(&snapshot(4, vec![ready_window(second, 4)]))
            .expect("the exact new binding must remain observable");
    }

    #[test]
    fn terminal_tombstone_is_the_only_old_binding_fact_allowed_after_token_aba() {
        let mut registry = ViewportRegistry::default();
        let token = WindowToken::new(8);
        let first = registry
            .register_existing(
                WorkspaceEpoch::new(0),
                SurfaceId::new(1),
                token,
                ViewportRole::Root,
            )
            .expect("first binding must register");
        registry
            .apply_snapshot_for_test(&snapshot_with_close(
                1,
                Vec::new(),
                vec![close_observation(first, 1, WindowCloseState::Destroyed)],
            ))
            .expect("first binding must be destroyed");
        registry
            .remove_destroyed(first)
            .expect("destroyed binding must be removable");
        let second = registry
            .register_existing(
                WorkspaceEpoch::new(1),
                SurfaceId::new(1),
                token,
                ViewportRole::Child,
            )
            .expect("the exact destroyed token may receive a new incarnation");
        let terminal_tombstones = BTreeSet::from([first]);

        let repeated_tombstone = snapshot_with_close(
            2,
            vec![ready_window(second, 2)],
            vec![close_observation(first, 2, WindowCloseState::Destroyed)],
        );
        apply_snapshot_with_retired_bindings_for_test(
            &mut registry,
            &repeated_tombstone,
            &BTreeSet::new(),
            &terminal_tombstones,
        )
        .expect("a repeated exact terminal tombstone must be inert");
        assert_eq!(
            registry
                .record(second.surface())
                .map(ViewportRecord::binding),
            Some(second)
        );

        let unknown_old_close = WindowCloseObservation::new(
            first,
            CloseObservationGeneration::new(3),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            CloseEffectAcknowledgement::known(None),
        );
        let before_unknown_close = registry.clone();
        assert_eq!(
            apply_snapshot_with_retired_bindings_for_test(
                &mut registry,
                &snapshot_with_close(3, vec![ready_window(second, 3)], vec![unknown_old_close]),
                &BTreeSet::new(),
                &terminal_tombstones,
            ),
            Err(ViewportRegistryError::StaleObservedBinding {
                observed: first,
                current: second,
            })
        );
        assert_eq!(registry, before_unknown_close);

        let before_old_window = registry.clone();
        assert_eq!(
            apply_snapshot_with_retired_bindings_for_test(
                &mut registry,
                &snapshot(4, vec![ready_window(first, 4)]),
                &BTreeSet::new(),
                &terminal_tombstones,
            ),
            Err(ViewportRegistryError::StaleObservedBinding {
                observed: first,
                current: second,
            })
        );
        assert_eq!(registry, before_old_window);
    }

    #[test]
    fn restore_rebind_resets_old_presentation_stream_authority() {
        let mut registry = ViewportRegistry::default();
        let surface = SurfaceId::new(9);
        let token = WindowToken::new(12);
        let before = registry
            .register_existing(WorkspaceEpoch::new(0), surface, token, ViewportRole::Root)
            .expect("initial binding must register");
        registry
            .apply_snapshot_for_test(&snapshot(
                1,
                vec![ready_window_with_presentation(
                    before,
                    7,
                    WindowPresentationState::Visible,
                )],
            ))
            .expect("old presentation must become authoritative");

        let reconciliation = registry
            .reconcile_restore(WorkspaceEpoch::new(1), &BTreeSet::from([before]))
            .expect("retained binding must rebind");
        let [(_, after)] = reconciliation.retained() else {
            panic!("exactly one retained binding must be returned");
        };

        // A delayed envelope from the old incarnation is rejected, but it must
        // not force a tombstone for the newly bound incarnation.
        let before_delayed_old = registry.clone();
        assert_eq!(
            registry.apply_snapshot_for_test(&snapshot(
                2,
                vec![ready_window_with_presentation(
                    before,
                    8,
                    WindowPresentationState::Hidden,
                )],
            )),
            Err(ViewportRegistryError::StaleObservedBinding {
                observed: before,
                current: *after,
            })
        );
        assert_eq!(registry, before_delayed_old);
        assert_eq!(
            registry
                .record(surface)
                .expect("rebound record must remain")
                .presentation_observation(),
            None
        );

        let first_new = registry
            .apply_snapshot_for_test(&snapshot(
                3,
                vec![ready_window_with_presentation(
                    *after,
                    1,
                    WindowPresentationState::Hidden,
                )],
            ))
            .expect("first observation for the new incarnation must recover authority");
        assert!(first_new.events().iter().any(|event| {
            matches!(event, RegistryEvent::PresentationChanged { binding } if *binding == *after)
        }));
        assert_eq!(
            registry
                .record(surface)
                .expect("rebound record must remain")
                .presentation_observation()
                .and_then(WindowPresentationObservation::known_state),
            Some(WindowPresentationState::Hidden)
        );
    }

    #[test]
    fn close_events_preserve_provider_and_ingress_generations_without_reemitting() {
        let mut registry = ViewportRegistry::default();
        let token = WindowToken::new(3);
        let binding = registry
            .register_existing(
                WorkspaceEpoch::new(0),
                SurfaceId::new(1),
                token,
                ViewportRole::Child,
            )
            .expect("binding must register");
        let requested = close_observation(binding, 2, WindowCloseState::LiveRequested);
        let first = registry
            .apply_snapshot_for_test(&snapshot_with_close(
                1,
                vec![ready_window(binding, 1)],
                vec![requested],
            ))
            .expect("close snapshot must apply");
        assert_eq!(
            first.events(),
            &[RegistryEvent::CloseRequested {
                observation: requested.with_inventory_generation(InventoryGeneration::new(1)),
            }]
        );

        let repeated = registry
            .apply_snapshot_for_test(&snapshot_with_close(
                2,
                vec![ready_window(binding, 2)],
                vec![requested],
            ))
            .expect("repeated snapshot must apply");
        assert!(repeated.events().is_empty());

        let stale_clear = close_observation(binding, 1, WindowCloseState::LiveClear);
        let stale = registry
            .apply_snapshot_for_test(&snapshot_with_close(
                3,
                vec![ready_window(binding, 3)],
                vec![stale_clear],
            ))
            .expect("stale observation must be ignored");
        assert!(stale.events().is_empty());

        let clear = close_observation(binding, 3, WindowCloseState::LiveClear);
        let cleared = registry
            .apply_snapshot_for_test(&snapshot_with_close(
                4,
                vec![ready_window(binding, 4)],
                vec![clear],
            ))
            .expect("new clear observation must apply");
        assert!(cleared.events().iter().any(|event| matches!(
            event,
            RegistryEvent::CloseRequestCleared {
                requested: actual_requested,
                observation,
            }
                if *actual_requested
                    == requested.with_inventory_generation(InventoryGeneration::new(1))
                    && *observation == clear.with_inventory_generation(InventoryGeneration::new(4))
        )));

        let duplicate_clear = registry
            .apply_snapshot_for_test(&snapshot_with_close(
                5,
                vec![ready_window(binding, 5)],
                vec![clear],
            ))
            .expect("duplicate clear must apply without another edge");
        assert!(duplicate_clear.events().is_empty());
    }

    #[test]
    fn narrow_close_observations_preserve_exact_edge_and_clear_order() {
        let mut registry = ViewportRegistry::default();
        let binding = registry
            .register_existing(
                WorkspaceEpoch::new(0),
                SurfaceId::new(1),
                WindowToken::new(3),
                ViewportRole::Child,
            )
            .expect("binding must register");
        registry
            .apply_snapshot_for_test(&snapshot(1, vec![ready_window(binding, 1)]))
            .expect("baseline snapshot must apply");

        let requested = close_observation(binding, 1, WindowCloseState::LiveRequested);
        let request = registry
            .apply_close_observation(requested)
            .expect("exact close request must apply");
        assert_eq!(request.generation(), InventoryGeneration::new(2));
        assert_eq!(
            request.events(),
            &[RegistryEvent::CloseRequested {
                observation: requested.with_inventory_generation(InventoryGeneration::new(2)),
            }]
        );

        let clear = close_observation(binding, 2, WindowCloseState::LiveClear);
        let cleared = registry
            .apply_close_observation(clear)
            .expect("later exact clear must apply");
        assert_eq!(cleared.generation(), InventoryGeneration::new(3));
        assert_eq!(
            cleared.events(),
            &[RegistryEvent::CloseRequestCleared {
                requested: requested.with_inventory_generation(InventoryGeneration::new(2)),
                observation: clear.with_inventory_generation(InventoryGeneration::new(3)),
            }]
        );
    }

    #[test]
    fn narrow_close_observation_rejects_stale_incarnation_atomically() {
        let mut registry = ViewportRegistry::default();
        let surface = SurfaceId::new(1);
        let before = registry
            .register_existing(
                WorkspaceEpoch::new(0),
                surface,
                WindowToken::new(3),
                ViewportRole::Child,
            )
            .expect("binding must register");
        let reconciliation = registry
            .reconcile_restore(WorkspaceEpoch::new(1), &BTreeSet::from([before]))
            .expect("retained binding must rebind");
        let [(_, after)] = reconciliation.retained() else {
            panic!("one retained binding must be rebound");
        };
        let unchanged = registry.clone();
        assert_eq!(
            registry.apply_close_observation(close_observation(
                before,
                1,
                WindowCloseState::LiveRequested,
            )),
            Err(ViewportRegistryError::StaleObservedBinding {
                observed: before,
                current: *after,
            })
        );
        assert_eq!(registry, unchanged);
    }

    #[test]
    fn narrow_close_observation_cannot_bypass_retirement_for_destruction() {
        let mut registry = ViewportRegistry::default();
        let binding = registry
            .register_existing(
                WorkspaceEpoch::new(0),
                SurfaceId::new(1),
                WindowToken::new(3),
                ViewportRole::Child,
            )
            .expect("binding must register");
        let unchanged = registry.clone();
        assert_eq!(
            registry.apply_close_observation(close_observation(
                binding,
                1,
                WindowCloseState::Destroyed,
            )),
            Err(ViewportRegistryError::NarrowDestroyedObservationForbidden { binding })
        );
        assert_eq!(registry, unchanged);
    }

    #[test]
    fn pending_close_edge_requires_exact_provider_and_ingress_generations() {
        let mut registry = ViewportRegistry::default();
        let surface = SurfaceId::new(1);
        let token = WindowToken::new(3);
        let binding = registry
            .register_existing(WorkspaceEpoch::new(0), surface, token, ViewportRole::Child)
            .expect("binding must register");
        registry
            .apply_snapshot_for_test(&snapshot(1, vec![ready_window(binding, 1)]))
            .expect("ready snapshot must apply");

        let requested = close_observation(binding, 5, WindowCloseState::LiveRequested);
        registry
            .apply_snapshot_for_test(&snapshot_with_close(
                2,
                vec![ready_window(binding, 2)],
                vec![requested],
            ))
            .expect("close request must apply");
        let record = registry
            .record(surface)
            .expect("record must remain present");
        let domain = EngineAuthorityDomainId::new_for_test(7);
        let exact = NativeCloseEdge::from_authoritative_requested(
            domain,
            binding,
            CloseObservationGeneration::new(5),
            InventoryGeneration::new(2),
        );
        let different_provider_generation = NativeCloseEdge::from_authoritative_requested(
            domain,
            binding,
            CloseObservationGeneration::new(6),
            InventoryGeneration::new(2),
        );
        let different_ingress_generation = NativeCloseEdge::from_authoritative_requested(
            domain,
            binding,
            CloseObservationGeneration::new(5),
            InventoryGeneration::new(3),
        );
        assert!(record.matches_pending_close_edge(exact));
        assert!(!record.matches_pending_close_edge(different_provider_generation));
        assert!(!record.matches_pending_close_edge(different_ingress_generation));

        let cleared = close_observation(binding, 6, WindowCloseState::LiveClear);
        registry
            .apply_snapshot_for_test(&snapshot_with_close(
                3,
                vec![ready_window(binding, 3)],
                vec![cleared],
            ))
            .expect("close clear must apply");
        let reissued = close_observation(binding, 7, WindowCloseState::LiveRequested);
        registry
            .apply_snapshot_for_test(&snapshot_with_close(
                4,
                vec![ready_window(binding, 4)],
                vec![reissued],
            ))
            .expect("reissued close request must apply");
        let record = registry
            .record(surface)
            .expect("record must remain present");
        let current = NativeCloseEdge::from_authoritative_requested(
            domain,
            binding,
            CloseObservationGeneration::new(7),
            InventoryGeneration::new(4),
        );
        assert!(!record.matches_pending_close_edge(exact));
        assert!(record.matches_pending_close_edge(current));
        assert_eq!(
            registry.native_close_edge_disposition(exact),
            NativeCloseEdgeDisposition::DifferentPending {
                observation: reissued.with_inventory_generation(InventoryGeneration::new(4)),
            }
        );

        let tombstone = close_observation(binding, 8, WindowCloseState::Destroyed);
        registry
            .apply_snapshot_for_test(&snapshot_with_close(5, Vec::new(), vec![tombstone]))
            .expect("typed destruction must apply");
        assert_eq!(
            registry.native_close_edge_disposition(exact),
            NativeCloseEdgeDisposition::Destroyed {
                observation: tombstone.with_inventory_generation(InventoryGeneration::new(5)),
            }
        );
    }

    #[test]
    fn authoritative_absence_is_missing_and_never_synthesizes_destruction() {
        let mut registry = ViewportRegistry::default();
        let surface = SurfaceId::new(1);
        let token = WindowToken::new(3);
        let binding = registry
            .register_existing(WorkspaceEpoch::new(0), surface, token, ViewportRole::Child)
            .expect("binding must register");
        registry
            .apply_snapshot_for_test(&snapshot(1, vec![ready_window(binding, 1)]))
            .expect("live window must apply");

        let missing = registry
            .apply_snapshot_for_test(&snapshot(2, Vec::new()))
            .expect("authoritative absence must apply");
        assert_eq!(
            missing.events(),
            &[RegistryEvent::BindingMissing { binding }]
        );
        assert_eq!(
            registry
                .record(surface)
                .expect("missing binding remains registered")
                .lifecycle(),
            ViewportLifecycle::Missing
        );
        assert!(
            !missing
                .events()
                .iter()
                .any(|event| matches!(event, RegistryEvent::Destroyed { .. }))
        );

        let repeated = registry
            .apply_snapshot_for_test(&snapshot(3, Vec::new()))
            .expect("repeated absence must apply");
        assert!(repeated.events().is_empty());
    }

    #[test]
    fn typed_tombstone_after_missing_destroys_once() {
        let mut registry = ViewportRegistry::default();
        let surface = SurfaceId::new(1);
        let token = WindowToken::new(3);
        let binding = registry
            .register_existing(WorkspaceEpoch::new(0), surface, token, ViewportRole::Child)
            .expect("binding must register");
        registry
            .apply_snapshot_for_test(&snapshot(1, vec![ready_window(binding, 1)]))
            .expect("live window must apply");
        registry
            .apply_snapshot_for_test(&snapshot(2, Vec::new()))
            .expect("missing inventory must apply");

        let tombstone = close_observation(binding, 7, WindowCloseState::Destroyed);
        let destroyed = registry
            .apply_snapshot_for_test(&snapshot_with_close(3, Vec::new(), vec![tombstone]))
            .expect("typed tombstone must apply after absence");
        assert_eq!(
            destroyed.events(),
            &[RegistryEvent::Destroyed {
                observation: tombstone.with_inventory_generation(InventoryGeneration::new(3)),
            }]
        );
        assert_eq!(
            registry
                .record(surface)
                .expect("destroyed binding remains queryable")
                .lifecycle(),
            ViewportLifecycle::Destroyed
        );

        let duplicate = registry
            .apply_snapshot_for_test(&snapshot_with_close(4, Vec::new(), vec![tombstone]))
            .expect("duplicate tombstone must be idempotent");
        assert!(duplicate.events().is_empty());
    }

    #[test]
    fn destroyed_binding_cannot_reappear_and_rejection_is_atomic() {
        let mut registry = ViewportRegistry::default();
        let surface = SurfaceId::new(1);
        let token = WindowToken::new(3);
        let binding = registry
            .register_existing(WorkspaceEpoch::new(0), surface, token, ViewportRole::Child)
            .expect("binding must register");
        registry
            .apply_snapshot_for_test(&snapshot_with_close(
                1,
                Vec::new(),
                vec![close_observation(binding, 1, WindowCloseState::Destroyed)],
            ))
            .expect("typed tombstone must apply");
        let before = registry.clone();

        assert_eq!(
            registry.apply_snapshot_for_test(&snapshot(2, vec![ready_window(binding, 2)])),
            Err(ViewportRegistryError::DestroyedBindingReappeared { binding })
        );
        assert_eq!(registry, before);
    }

    #[test]
    fn inventory_generation_exhaustion_is_atomic() {
        let mut registry = ViewportRegistry::default();
        registry.exhaust_inventory_generation();
        let before = registry.clone();
        assert_eq!(
            registry.apply_snapshot_for_test(&snapshot(1, Vec::new())),
            Err(ViewportRegistryError::InventoryGenerationExhausted)
        );
        assert_eq!(registry, before);
    }

    #[test]
    fn coordinate_generation_exhaustion_is_atomic() {
        let mut registry = ViewportRegistry::default();
        let token = WindowToken::new(3);
        let binding = registry
            .register_existing(
                WorkspaceEpoch::new(0),
                SurfaceId::new(1),
                token,
                ViewportRole::Child,
            )
            .expect("binding must register");
        registry.exhaust_coordinate_generation();
        let before = registry.clone();

        assert_eq!(
            registry.apply_snapshot_for_test(&snapshot(1, vec![ready_window(binding, 1)])),
            Err(ViewportRegistryError::CoordinateGenerationExhausted)
        );
        assert_eq!(registry, before);
    }

    #[test]
    fn provider_revocation_clears_live_streams_and_preserves_structural_state() {
        let mut registry = ViewportRegistry::default();
        let live_surface = SurfaceId::new(1);
        let retiring_surface = SurfaceId::new(2);
        let live = registry
            .register_existing(
                WorkspaceEpoch::new(0),
                live_surface,
                WindowToken::new(3),
                ViewportRole::Root,
            )
            .expect("live binding must register");
        let retiring = registry
            .register_existing(
                WorkspaceEpoch::new(0),
                retiring_surface,
                WindowToken::new(4),
                ViewportRole::Child,
            )
            .expect("retiring binding must register");
        let requested = close_observation(live, 1, WindowCloseState::LiveRequested);
        registry
            .apply_snapshot_for_test(&snapshot_with_close(
                1,
                vec![
                    ready_window_with_presentation(live, 1, WindowPresentationState::Visible),
                    ready_window_with_presentation(retiring, 1, WindowPresentationState::Visible),
                ],
                vec![requested],
            ))
            .expect("live provider facts must apply");
        registry
            .mark_awaiting_destroyed(retiring)
            .expect("retirement obligation must be established");

        let live_before = registry
            .record(live_surface)
            .expect("live record must exist")
            .clone();
        let retiring_before = registry
            .record(retiring_surface)
            .expect("retiring record must exist")
            .clone();
        let inventory_before = registry.inventory_generation();

        let transition = registry
            .revoke_live_provider_authority()
            .expect("provider authority must revoke atomically");
        assert!(transition.generation() > inventory_before);
        assert_eq!(
            transition.events(),
            &[
                RegistryEvent::FactsUnavailable { binding: live },
                RegistryEvent::FactsUnavailable { binding: retiring },
            ]
        );

        let live_after = registry
            .record(live_surface)
            .expect("live record must remain registered");
        assert_eq!(live_after.binding(), live_before.binding());
        assert_eq!(live_after.role(), live_before.role());
        assert_eq!(live_after.ownership(), live_before.ownership());
        assert_eq!(live_after.admission(), live_before.admission());
        assert_eq!(
            live_after.lifecycle(),
            ViewportLifecycle::AwaitingObservation
        );
        assert_eq!(
            live_after.recovery_coordinates(),
            live_before.recovery_coordinates()
        );
        assert!(live_after.coordinate_generation() > live_before.coordinate_generation());
        assert_live_provider_streams_are_revoked(live_after);

        let retiring_after = registry
            .record(retiring_surface)
            .expect("retiring record must remain registered");
        assert_eq!(retiring_after.binding(), retiring_before.binding());
        assert_eq!(retiring_after.ownership(), retiring_before.ownership());
        assert_eq!(retiring_after.admission(), ViewportAdmission::Retiring);
        assert_eq!(
            retiring_after.lifecycle(),
            ViewportLifecycle::AwaitingDestroyed
        );
        assert_eq!(
            retiring_after.recovery_coordinates(),
            retiring_before.recovery_coordinates()
        );
        assert_eq!(
            retiring_after.coordinate_generation(),
            retiring_before.coordinate_generation(),
            "retirement already revoked live coordinate authority"
        );
        assert_live_provider_streams_are_revoked(retiring_after);

        let after_first = registry.clone();
        let repeated = registry
            .revoke_live_provider_authority()
            .expect("repeated provider revocation must be inert");
        assert_eq!(repeated.generation(), transition.generation());
        assert!(repeated.events().is_empty());
        assert_eq!(registry, after_first);
    }

    #[test]
    fn provider_revocation_does_not_roll_back_destroyed_records() {
        let mut registry = ViewportRegistry::default();
        let surface = SurfaceId::new(1);
        let binding = registry
            .register_existing(
                WorkspaceEpoch::new(0),
                surface,
                WindowToken::new(3),
                ViewportRole::Child,
            )
            .expect("binding must register");
        registry
            .apply_snapshot_for_test(&snapshot_with_close(
                1,
                Vec::new(),
                vec![close_observation(binding, 1, WindowCloseState::Destroyed)],
            ))
            .expect("terminal destruction must apply");
        let before = registry.clone();

        let transition = registry
            .revoke_live_provider_authority()
            .expect("destroyed records must be inert during provider revocation");

        assert!(transition.events().is_empty());
        assert_eq!(transition.generation(), before.inventory_generation());
        assert_eq!(registry, before);
    }

    #[test]
    fn provider_revocation_resets_only_stale_watermarks_without_advancing_inventory() {
        let mut registry = ViewportRegistry::default();
        let surface = SurfaceId::new(1);
        let binding = registry
            .register_existing(
                WorkspaceEpoch::new(0),
                surface,
                WindowToken::new(3),
                ViewportRole::Child,
            )
            .expect("binding must register");
        let coordinate_tombstone = WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(1),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        );
        let record = registry
            .records
            .get_mut(&surface)
            .expect("record must remain registered");
        record
            .coordinate_observations
            .observe(binding, Some(coordinate_tombstone));
        assert!(record.coordinate_observations.current().is_none());
        assert_ne!(
            record.coordinate_observations,
            WindowCoordinateObservationStream::default()
        );
        let inventory_before = registry.inventory_generation();
        let coordinate_before = registry
            .record(surface)
            .expect("record must exist")
            .coordinate_generation();

        let transition = registry
            .revoke_live_provider_authority()
            .expect("stale provider watermarks must reset");

        assert!(transition.events().is_empty());
        assert_eq!(transition.generation(), inventory_before);
        let record = registry
            .record(surface)
            .expect("record must remain registered");
        assert_eq!(
            record.coordinate_observations,
            WindowCoordinateObservationStream::default()
        );
        assert_eq!(record.coordinate_generation(), coordinate_before);
    }

    #[test]
    fn provider_revocation_coordinate_generation_exhaustion_is_atomic() {
        let mut registry = ViewportRegistry::default();
        let binding = registry
            .register_existing(
                WorkspaceEpoch::new(0),
                SurfaceId::new(1),
                WindowToken::new(3),
                ViewportRole::Child,
            )
            .expect("binding must register");
        registry
            .apply_snapshot_for_test(&snapshot(
                1,
                vec![ready_window_with_presentation(
                    binding,
                    1,
                    WindowPresentationState::Visible,
                )],
            ))
            .expect("coordinate authority must apply");
        registry.exhaust_coordinate_generation();
        let before = registry.clone();

        assert_eq!(
            registry.revoke_live_provider_authority(),
            Err(ViewportRegistryError::CoordinateGenerationExhausted)
        );
        assert_eq!(registry, before);
    }

    #[test]
    fn provider_revocation_inventory_generation_exhaustion_is_atomic() {
        let mut registry = ViewportRegistry::default();
        let binding = registry
            .register_existing(
                WorkspaceEpoch::new(0),
                SurfaceId::new(1),
                WindowToken::new(3),
                ViewportRole::Child,
            )
            .expect("binding must register");
        registry
            .apply_snapshot_for_test(&snapshot(1, vec![ObservedWindow::new(binding)]))
            .expect("an observed live binding must apply");
        registry.exhaust_inventory_generation();
        let before = registry.clone();

        assert_eq!(
            registry.revoke_live_provider_authority(),
            Err(ViewportRegistryError::InventoryGenerationExhausted)
        );
        assert_eq!(registry, before);
    }

    #[test]
    fn coordinate_provider_stream_separates_watermark_from_semantic_authority() {
        let mut registry = ViewportRegistry::default();
        let surface = SurfaceId::new(1);
        let binding = registry
            .register_existing(
                WorkspaceEpoch::new(0),
                surface,
                WindowToken::new(3),
                ViewportRole::Child,
            )
            .expect("binding must register");

        registry
            .apply_snapshot_for_test(&snapshot(
                1,
                vec![ready_window_with_presentation(
                    binding,
                    1,
                    WindowPresentationState::Visible,
                )],
            ))
            .expect("initial coordinates must apply");
        let initial = registry.record(surface).expect("record must exist");
        let initial_core_generation = initial.coordinate_generation();
        assert_eq!(
            initial.coordinate_observation_generation(),
            Some(CoordinateObservationGeneration::new(1))
        );

        registry
            .apply_snapshot_for_test(&snapshot(
                2,
                vec![ready_window_with_presentation(
                    binding,
                    2,
                    WindowPresentationState::Visible,
                )],
            ))
            .expect("same coordinate facts with a newer provider generation must apply");
        let repeated = registry.record(surface).expect("record must exist");
        assert_eq!(repeated.coordinate_generation(), initial_core_generation);
        assert_eq!(
            repeated.coordinate_observation_generation_watermark(),
            Some(CoordinateObservationGeneration::new(2))
        );
        assert_eq!(
            repeated.coordinate_observation_generation(),
            Some(CoordinateObservationGeneration::new(2))
        );

        registry
            .apply_snapshot_for_test(&snapshot(
                3,
                vec![ObservedWindow::new(binding).with_presentation_observation(
                    presentation_observation(
                        binding,
                        3,
                        Authority::Known(WindowPresentationState::Visible),
                    ),
                )],
            ))
            .expect("an unversioned coordinate gap must apply fail-closed");
        let gap = registry.record(surface).expect("record must exist");
        assert!(gap.coordinates().is_none());
        assert_eq!(
            gap.recovery_coordinates()
                .map(|coordinates| coordinates.content().observation_generation()),
            Some(CoordinateObservationGeneration::new(2)),
            "a temporary gap must not erase the last trusted recovery anchor"
        );

        registry
            .apply_snapshot_for_test(&snapshot(
                4,
                vec![ready_window_with_presentation(
                    binding,
                    3,
                    WindowPresentationState::Visible,
                )],
            ))
            .expect("known coordinates after a gap must remain quarantined");
        let quarantined = registry.record(surface).expect("record must exist");
        assert!(quarantined.coordinates().is_none());
        assert_eq!(
            quarantined.coordinate_observation_generation_watermark(),
            Some(CoordinateObservationGeneration::new(3))
        );
        assert_eq!(
            quarantined
                .recovery_coordinates()
                .map(|coordinates| coordinates.content().observation_generation()),
            Some(CoordinateObservationGeneration::new(2))
        );

        let tombstone = WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(4),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        );
        registry
            .apply_snapshot_for_test(&snapshot(
                5,
                vec![
                    ObservedWindow::new(binding)
                        .with_coordinate_observation(tombstone)
                        .with_presentation_observation(presentation_observation(
                            binding,
                            4,
                            Authority::Known(WindowPresentationState::Visible),
                        )),
                ],
            ))
            .expect("an explicit tombstone must clear quarantine");
        registry
            .apply_snapshot_for_test(&snapshot(
                6,
                vec![ready_window_with_presentation(
                    binding,
                    5,
                    WindowPresentationState::Visible,
                )],
            ))
            .expect("known coordinates after a tombstone must restore authority");
        let restored = registry.record(surface).expect("record must exist");
        assert_eq!(
            restored.coordinate_observation_generation(),
            Some(CoordinateObservationGeneration::new(5))
        );
        let exact_outer_anchor = restored
            .recovery_coordinates()
            .and_then(RecoveryCoordinateSnapshot::outer_bounds)
            .expect("ready coordinates must retain one exact outer anchor");

        let outer_unknown = WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(6),
            Authority::Known(
                PhysicalRect::new(0.0, 0.0, 100.0, 100.0).expect("test bounds must be valid"),
            ),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            Authority::Known(ScaleFactor::new(1.0).expect("test scale must be valid")),
            Authority::Known(ScaleFactor::new(1.0).expect("test scale must be valid")),
        );
        let before_outer_change = restored.coordinate_generation();
        registry
            .apply_snapshot_for_test(&snapshot(
                7,
                vec![
                    ObservedWindow::new(binding)
                        .with_coordinate_observation(outer_unknown)
                        .with_presentation_observation(presentation_observation(
                            binding,
                            6,
                            Authority::Known(WindowPresentationState::Visible),
                        )),
                ],
            ))
            .expect("optional outer bounds may become unavailable authoritatively");
        let outer_changed = registry.record(surface).expect("record must exist");
        assert!(outer_changed.has_coordinate_authority());
        assert!(
            outer_changed
                .coordinates()
                .is_some_and(|coordinates| coordinates.outer_bounds().is_none())
        );
        assert!(outer_changed.coordinate_generation() > before_outer_change);
        assert_eq!(
            outer_changed
                .recovery_coordinates()
                .and_then(RecoveryCoordinateSnapshot::outer_bounds),
            Some(exact_outer_anchor),
            "an unavailable outer fact must not erase or replace the last exact anchor"
        );
        assert_eq!(
            outer_changed
                .recovery_coordinates()
                .map(RecoveryCoordinateSnapshot::content),
            outer_changed.coordinates(),
            "recovery retains the newest exact content projection independently"
        );
    }

    #[test]
    fn hidden_and_minimized_presentations_revoke_placement_authority() {
        for presentation in [
            WindowPresentationState::Hidden,
            WindowPresentationState::Minimized,
        ] {
            let mut registry = ViewportRegistry::default();
            let surface = SurfaceId::new(1);
            let binding = registry
                .register_existing(
                    WorkspaceEpoch::new(0),
                    surface,
                    WindowToken::new(3),
                    ViewportRole::Root,
                )
                .expect("binding must register");
            registry
                .apply_snapshot_for_test(&snapshot(
                    1,
                    vec![ready_window_with_presentation(
                        binding,
                        1,
                        WindowPresentationState::Visible,
                    )],
                ))
                .expect("visible coordinates must become authoritative");

            let work_area = ObservedWorkArea::new(
                WorkAreaToken::new(1),
                PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).expect("test work area must be valid"),
                ScaleFactor::new(1.0).expect("test scale must be valid"),
            );
            let logical_rect =
                LogicalRect::new(10.0, 10.0, 100.0, 80.0).expect("test rect must be valid");
            let proof = registry
                .placement(surface, logical_rect, work_area, WorkAreaGeneration::new(1))
                .expect("visible binding must authorize placement");
            let visible = registry.record(surface).expect("visible record must exist");
            let visible_generation = visible.coordinate_generation();
            let recovery_anchor = visible.recovery_coordinates();

            registry
                .apply_snapshot_for_test(&snapshot(
                    2,
                    vec![ready_window_with_presentation(binding, 2, presentation)],
                ))
                .expect("non-visible presentation must apply");

            let unavailable = registry
                .record(surface)
                .expect("non-visible record must remain registered");
            assert!(!unavailable.has_coordinate_authority());
            assert!(unavailable.coordinate_generation() > visible_generation);
            assert_eq!(unavailable.recovery_coordinates(), recovery_anchor);
            assert!(!registry.proof_is_current(&proof, WorkAreaGeneration::new(1)));
            assert!(matches!(
                registry.placement(surface, logical_rect, work_area, WorkAreaGeneration::new(1),),
                Err(CoordinateUnavailable::FactUnavailable {
                    reason: AuthorityUnavailableReason::SurfaceUnavailable,
                    ..
                })
            ));
        }
    }

    #[test]
    fn coordinate_provider_generation_restarts_after_exact_rebind() {
        let mut registry = ViewportRegistry::default();
        let surface = SurfaceId::new(1);
        let before = registry
            .register_existing(
                WorkspaceEpoch::new(0),
                surface,
                WindowToken::new(3),
                ViewportRole::Child,
            )
            .expect("binding must register");
        registry
            .apply_snapshot_for_test(&snapshot(1, vec![ready_window(before, 99)]))
            .expect("high provider generation must apply to the original incarnation");

        let retained = BTreeSet::from([before]);
        let reconciliation = registry
            .reconcile_restore(WorkspaceEpoch::new(1), &retained)
            .expect("retained binding must rebind");
        let after = reconciliation.retained()[0].1;
        assert_ne!(before, after);
        assert_eq!(
            registry
                .record(surface)
                .expect("rebound record must exist")
                .coordinate_observation_generation_watermark(),
            None
        );

        registry
            .apply_snapshot_for_test(&snapshot(2, vec![ready_window(after, 1)]))
            .expect("a new exact incarnation may restart its provider generation");
        assert_eq!(
            registry
                .record(surface)
                .expect("rebound record must exist")
                .coordinate_observation_generation(),
            Some(CoordinateObservationGeneration::new(1))
        );
    }

    #[test]
    fn only_a_never_observed_reservation_can_be_discarded() {
        let mut registry = ViewportRegistry::default();
        let unobserved = registry
            .reserve(
                WorkspaceEpoch::new(0),
                SurfaceId::new(1),
                ViewportRole::Child,
            )
            .expect("future binding must reserve");
        registry
            .discard_unobserved(unobserved)
            .expect("unobserved reservation must be discardable");
        assert!(registry.record(unobserved.surface()).is_none());

        let observed = registry
            .reserve(
                WorkspaceEpoch::new(0),
                SurfaceId::new(2),
                ViewportRole::Child,
            )
            .expect("observed binding must reserve");
        registry
            .apply_snapshot_for_test(&snapshot(1, vec![ready_window(observed, 1)]))
            .expect("window must become observed");
        assert_eq!(
            registry.discard_unobserved(observed),
            Err(ViewportRegistryError::BindingAlreadyObserved { binding: observed })
        );
    }

    #[test]
    fn restore_reconciliation_incarnation_exhaustion_is_atomic() {
        let mut registry = ViewportRegistry::default();
        let first_surface = SurfaceId::new(1);
        let second_surface = SurfaceId::new(2);
        let first = registry
            .register_existing(
                WorkspaceEpoch::new(0),
                first_surface,
                WindowToken::new(1),
                ViewportRole::Root,
            )
            .expect("first binding must register");
        let second = registry
            .register_existing(
                WorkspaceEpoch::new(0),
                second_surface,
                WindowToken::new(2),
                ViewportRole::Child,
            )
            .expect("second binding must register");
        registry.last_incarnation = WindowIncarnation::new(u64::MAX - 1);
        let retained_bindings = BTreeSet::from([first, second]);
        let before = registry.clone();

        assert_eq!(
            registry.reconcile_restore(WorkspaceEpoch::new(1), &retained_bindings),
            Err(ViewportRegistryError::WindowIncarnationExhausted)
        );
        assert_eq!(registry, before);
    }

    #[test]
    fn restore_rebind_requires_a_new_exact_binding_close_observation() {
        let mut registry = ViewportRegistry::default();
        let surface = SurfaceId::new(1);
        let token = WindowToken::new(3);
        let before = registry
            .register_existing(WorkspaceEpoch::new(0), surface, token, ViewportRole::Root)
            .expect("binding must register");
        registry
            .apply_snapshot_for_test(&snapshot_with_close(
                1,
                vec![ready_window(before, 1)],
                vec![close_observation(
                    before,
                    8,
                    WindowCloseState::LiveRequested,
                )],
            ))
            .expect("close-request snapshot must apply");

        let reconciliation = registry
            .reconcile_restore(WorkspaceEpoch::new(1), &BTreeSet::from([before]))
            .expect("retained binding must reconcile");
        let [(retained_before, retained_after)] = reconciliation.retained() else {
            panic!("exactly one retained binding must be returned");
        };
        assert_eq!(*retained_before, before);
        assert_eq!(retained_after.epoch(), WorkspaceEpoch::new(1));
        assert_eq!(retained_after.surface(), before.surface());
        assert_eq!(retained_after.token(), before.token());
        assert_ne!(retained_after.incarnation(), before.incarnation());
        assert!(reconciliation.retired().is_empty());

        let record = registry
            .record(surface)
            .expect("retained record must remain current");
        assert_eq!(record.binding(), *retained_after);
        assert_eq!(record.lifecycle, ViewportLifecycle::AwaitingObservation);
        assert!(record.coordinates.is_none());
        assert!(record.recovery_coordinates.is_none());
        assert!(!record.ever_observed);

        let before_delayed_old = registry.clone();
        assert_eq!(
            registry.apply_snapshot_for_test(&snapshot_with_close(
                2,
                vec![ready_window(before, 2)],
                vec![close_observation(
                    before,
                    9,
                    WindowCloseState::LiveRequested,
                )],
            )),
            Err(ViewportRegistryError::StaleObservedBinding {
                observed: before,
                current: *retained_after,
            })
        );
        assert_eq!(registry, before_delayed_old);

        let requested = close_observation(*retained_after, 1, WindowCloseState::LiveRequested);
        let rebound_close = registry
            .apply_snapshot_for_test(&snapshot_with_close(
                3,
                vec![ready_window(*retained_after, 3)],
                vec![requested],
            ))
            .expect("new-incarnation close observation must apply");
        assert!(rebound_close.events().iter().any(|event| matches!(
            event,
            RegistryEvent::CloseRequested { observation }
                if observation.binding() == *retained_after
                    && observation.inventory_generation() == InventoryGeneration::new(2)
        )));
        assert_eq!(
            registry
                .record(surface)
                .expect("record must remain current")
                .lifecycle(),
            ViewportLifecycle::CloseRequested
        );
    }

    #[test]
    fn restore_reconciliation_removes_and_returns_complete_retired_facts() {
        let mut registry = ViewportRegistry::default();
        let surface = SurfaceId::new(7);
        let token = WindowToken::new(11);
        let binding = registry
            .register_existing(WorkspaceEpoch::new(0), surface, token, ViewportRole::Child)
            .expect("binding must register");
        registry
            .apply_snapshot_for_test(&snapshot(1, vec![ready_window(binding, 1)]))
            .expect("ready snapshot must apply");
        registry
            .mark_awaiting_destroyed(binding)
            .expect("binding must enter its terminal close state");
        let reconciliation = registry
            .reconcile_restore(WorkspaceEpoch::new(1), &BTreeSet::new())
            .expect("unretained binding must retire");

        assert!(reconciliation.retained().is_empty());
        let [retired] = reconciliation.retired() else {
            panic!("exactly one retired record must be returned");
        };
        assert_eq!(retired.binding(), binding);
        assert_eq!(retired.role(), ViewportRole::Child);
        assert_eq!(retired.lifecycle(), ViewportLifecycle::AwaitingDestroyed);
        assert!(retired.ever_observed());
        assert!(registry.record(surface).is_none());
        assert!(registry.records().next().is_none());
    }

    #[test]
    fn awaiting_destroyed_requires_a_typed_tombstone() {
        let mut registry = ViewportRegistry::default();
        let surface = SurfaceId::new(1);
        let token = WindowToken::new(3);
        let binding = registry
            .register_existing(WorkspaceEpoch::new(0), surface, token, ViewportRole::Child)
            .expect("binding must register");
        registry
            .apply_snapshot_for_test(&snapshot(1, vec![ready_window(binding, 1)]))
            .expect("ready snapshot must apply");
        registry
            .mark_awaiting_destroyed(binding)
            .expect("current binding can await destruction");

        let still_observed = registry
            .apply_snapshot_for_test(&snapshot(2, vec![ready_window(binding, 2)]))
            .expect("observed snapshot must apply");
        assert_eq!(
            registry
                .record(surface)
                .expect("record must remain registered")
                .lifecycle(),
            ViewportLifecycle::AwaitingDestroyed
        );
        assert!(still_observed.events().is_empty());

        let facts_unavailable = registry
            .apply_snapshot_for_test(&snapshot(3, vec![ObservedWindow::new(binding)]))
            .expect("incomplete observed snapshot must apply");
        assert_eq!(
            registry
                .record(surface)
                .expect("record must remain registered")
                .lifecycle(),
            ViewportLifecycle::AwaitingDestroyed
        );
        assert!(facts_unavailable.events().is_empty());

        let missing = registry
            .apply_snapshot_for_test(&snapshot(4, Vec::new()))
            .expect("authoritative absence must apply");
        assert_eq!(
            registry
                .record(surface)
                .expect("missing record remains queryable")
                .lifecycle(),
            ViewportLifecycle::Missing
        );
        assert_eq!(
            missing.events(),
            &[RegistryEvent::BindingMissing { binding }]
        );

        let tombstone = close_observation(binding, 4, WindowCloseState::Destroyed);
        let destroyed = registry
            .apply_snapshot_for_test(&snapshot_with_close(5, Vec::new(), vec![tombstone]))
            .expect("typed tombstone must finish destruction");
        assert_eq!(
            destroyed.events(),
            &[RegistryEvent::Destroyed {
                observation: tombstone.with_inventory_generation(InventoryGeneration::new(5)),
            }]
        );
        assert_eq!(
            registry
                .record(surface)
                .expect("destroyed record remains queryable")
                .lifecycle(),
            ViewportLifecycle::Destroyed
        );
    }

    #[test]
    fn focus_authority_requires_visible_presentation_but_survives_coordinate_gaps() {
        let mut registry = ViewportRegistry::default();
        let surface = SurfaceId::new(1);
        let token = WindowToken::new(3);
        let binding = registry
            .register_existing(WorkspaceEpoch::new(0), surface, token, ViewportRole::Child)
            .expect("binding must register");
        assert!(
            !registry
                .record(surface)
                .expect("reservation must exist")
                .can_observe_focus(),
            "an unobserved reservation cannot own provider focus"
        );

        registry
            .apply_snapshot_for_test(&snapshot(
                1,
                vec![ready_window_with_presentation(
                    binding,
                    1,
                    WindowPresentationState::Visible,
                )],
            ))
            .expect("visible ready snapshot must apply");
        assert!(
            registry
                .record(surface)
                .expect("ready record must exist")
                .can_observe_focus()
        );

        registry
            .apply_snapshot_for_test(&snapshot(
                2,
                vec![ready_window_with_presentation(
                    binding,
                    2,
                    WindowPresentationState::Hidden,
                )],
            ))
            .expect("hidden snapshot must apply");
        assert!(
            !registry
                .record(surface)
                .expect("hidden record must remain registered")
                .can_observe_focus()
        );

        registry
            .apply_snapshot_for_test(&snapshot(
                3,
                vec![ready_window_with_presentation(
                    binding,
                    3,
                    WindowPresentationState::Minimized,
                )],
            ))
            .expect("minimized snapshot must apply");
        assert!(
            !registry
                .record(surface)
                .expect("minimized record must remain registered")
                .can_observe_focus()
        );

        registry
            .apply_snapshot_for_test(&snapshot(
                4,
                vec![ready_window(binding, 4).with_presentation_observation(
                    presentation_observation(
                        binding,
                        4,
                        Authority::Unknown(AuthorityUnavailableReason::NotReported),
                    ),
                )],
            ))
            .expect("unknown presentation snapshot must apply");
        assert!(
            !registry
                .record(surface)
                .expect("unknown presentation record must remain registered")
                .can_observe_focus()
        );

        registry
            .apply_snapshot_for_test(&snapshot(
                5,
                vec![ObservedWindow::new(binding).with_presentation_observation(
                    presentation_observation(
                        binding,
                        5,
                        Authority::Known(WindowPresentationState::Visible),
                    ),
                )],
            ))
            .expect("visible coordinate gap must apply");
        let unavailable = registry
            .record(surface)
            .expect("observed record must remain registered");
        assert_eq!(
            unavailable.lifecycle(),
            ViewportLifecycle::AwaitingObservation
        );
        assert!(unavailable.can_observe_focus());
        assert!(unavailable.can_accept_activation());
        assert!(!unavailable.is_routeable());

        registry
            .apply_snapshot_for_test(&snapshot_with_close(
                6,
                vec![ready_window_with_presentation(
                    binding,
                    6,
                    WindowPresentationState::Visible,
                )],
                vec![close_observation(
                    binding,
                    1,
                    WindowCloseState::LiveRequested,
                )],
            ))
            .expect("close-requested snapshot must apply");
        let closing = registry
            .record(surface)
            .expect("close-requested record must remain registered");
        assert!(closing.can_observe_focus());
        assert!(!closing.can_accept_activation());

        registry
            .mark_awaiting_destroyed(binding)
            .expect("binding must enter terminal close state");
        assert!(
            !registry
                .record(surface)
                .expect("closing record must remain queryable")
                .can_observe_focus()
        );

        registry
            .apply_snapshot_for_test(&snapshot(7, Vec::new()))
            .expect("destroyed snapshot must apply");
        assert!(
            !registry
                .record(surface)
                .expect("missing record remains queryable")
                .can_observe_focus()
        );
    }

    #[test]
    fn visible_staging_binding_can_report_focus_without_semantic_admission() {
        let mut registry = ViewportRegistry::default();
        let surface = SurfaceId::new(9);
        let binding = registry
            .reserve(WorkspaceEpoch::new(0), surface, ViewportRole::Child)
            .expect("native staging binding must reserve");
        registry
            .apply_snapshot_for_test(&snapshot(
                1,
                vec![ready_window_with_presentation(
                    binding,
                    1,
                    WindowPresentationState::Visible,
                )],
            ))
            .expect("visible staging snapshot must apply");

        let staging = registry
            .record(surface)
            .expect("staging binding must remain registered");
        assert_eq!(staging.admission(), ViewportAdmission::Pending);
        assert!(staging.can_report_focus_fact());
        assert!(!staging.can_observe_focus());
        assert!(!staging.can_accept_activation());
    }

    #[test]
    fn retired_surface_authority_history_compacts_without_resetting_live_headless_aba() {
        let mut registry = ViewportRegistry::default();
        let epoch = WorkspaceEpoch::new(1);
        let retained_surface = SurfaceId::new(1);

        for index in 1_u64..=10_001 {
            let surface = SurfaceId::new(index);
            let binding = registry
                .register_existing(epoch, surface, WindowToken::new(index), ViewportRole::Root)
                .expect("unique test binding should register");
            registry
                .detach(binding)
                .expect("registered binding should detach");
        }
        let retained_generation = registry.surface_authority_generation(retained_surface);
        assert_ne!(retained_generation, CoordinateGeneration::default());
        assert_eq!(registry.surface_authority_generation_count(), 10_001);

        let removed =
            registry.compact_surface_authority_generations(&BTreeSet::from([retained_surface]));
        assert_eq!(removed, 10_000);
        assert_eq!(registry.surface_authority_generation_count(), 1);
        assert_eq!(
            registry.surface_authority_generation(retained_surface),
            retained_generation,
            "a still-live headless surface must not regain generation zero"
        );

        assert_eq!(
            registry.compact_surface_authority_generations(&BTreeSet::new()),
            1
        );
        assert_eq!(registry.surface_authority_generation_count(), 0);
    }
}

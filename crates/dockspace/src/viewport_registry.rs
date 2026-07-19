//! Core-owned registry for stable logical surfaces and ephemeral native bindings.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::coordinates::{CoordinateSnapshot, CoordinateUnavailable, ViewportPlacementProof};
use crate::geometry::LogicalRect;
use crate::ids::{SurfaceId, WorkspaceEpoch};
use crate::intent::Authority;
use crate::platform::{
    ObservedWindow, ObservedWorkArea, PlatformSnapshot, WindowInputObservation,
    WindowInputObservationStream, WindowPresentationState,
};
use crate::viewport::{
    CoordinateGeneration, InputObservationGeneration, InventoryGeneration, ViewportBinding,
    ViewportRole, WindowIncarnation, WindowToken, WorkAreaGeneration,
};

/// Lifecycle state derived from authoritative inventory, never from callback timing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewportLifecycle {
    AwaitingObservation,
    Ready,
    CloseRequested,
    AwaitingDestroyed,
    Missing,
}

/// Queryable registry record for one logical surface.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewportRecord {
    binding: ViewportBinding,
    role: ViewportRole,
    lifecycle: ViewportLifecycle,
    coordinates: Option<CoordinateSnapshot>,
    input_observations: WindowInputObservationStream,
    coordinate_generation: CoordinateGeneration,
    ever_observed: bool,
    close_request_latched: bool,
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
    pub const fn lifecycle(&self) -> ViewportLifecycle {
        self.lifecycle
    }

    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(self.lifecycle, ViewportLifecycle::Ready)
    }

    /// Returns whether the window is eligible for authoritative pointer routing.
    #[must_use]
    pub(crate) fn is_routeable(&self) -> bool {
        self.is_ready()
            && self.coordinates.is_some_and(|coordinates| {
                coordinates.presentation() == Some(WindowPresentationState::Visible)
            })
    }

    /// Returns whether this exact observed incarnation may own native focus.
    ///
    /// Focus authority is independent of geometry authority: a live window remains
    /// focusable while coordinate facts are temporarily unavailable.
    pub(crate) const fn is_focusable(&self) -> bool {
        self.ever_observed
            && matches!(
                self.lifecycle,
                ViewportLifecycle::AwaitingObservation
                    | ViewportLifecycle::Ready
                    | ViewportLifecycle::CloseRequested
            )
    }

    pub(crate) const fn coordinates(&self) -> Option<CoordinateSnapshot> {
        self.coordinates
    }

    pub(crate) const fn input_observation(&self) -> Option<WindowInputObservation> {
        self.input_observations.current()
    }

    pub(crate) const fn input_observation_generation_watermark(
        &self,
    ) -> Option<InputObservationGeneration> {
        self.input_observations.generation_watermark()
    }

    pub(crate) const fn is_observed(&self) -> bool {
        self.ever_observed && !matches!(self.lifecycle, ViewportLifecycle::Missing)
    }
}

/// One semantically relevant change from a complete inventory snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegistryEvent {
    Ready { binding: ViewportBinding },
    FactsUnavailable { binding: ViewportBinding },
    CloseRequested { binding: ViewportBinding },
    CloseRequestCleared { binding: ViewportBinding },
    Destroyed { binding: ViewportBinding },
}

/// Result of applying one complete inventory snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct RegistryTransition {
    generation: InventoryGeneration,
    events: Vec<RegistryEvent>,
}

/// Previous-epoch facts removed from the current registry during restore.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RetiredViewportFacts {
    binding: ViewportBinding,
    role: ViewportRole,
    lifecycle: ViewportLifecycle,
    last_coordinates: Option<CoordinateSnapshot>,
    input_observations: WindowInputObservationStream,
    ever_observed: bool,
    close_request_latched: bool,
}

impl RetiredViewportFacts {
    pub(crate) const fn binding(self) -> ViewportBinding {
        self.binding
    }

    pub(crate) const fn role(self) -> ViewportRole {
        self.role
    }

    pub(crate) const fn lifecycle(self) -> ViewportLifecycle {
        self.lifecycle
    }

    pub(crate) const fn last_coordinates(self) -> Option<CoordinateSnapshot> {
        self.last_coordinates
    }

    pub(crate) const fn input_observations(self) -> WindowInputObservationStream {
        self.input_observations
    }

    pub(crate) const fn ever_observed(self) -> bool {
        self.ever_observed
    }
}

/// Atomic result of reconciling native bindings with a restored workspace.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RestoreRegistryReconciliation {
    retained: Vec<(ViewportBinding, ViewportBinding)>,
    retired: Vec<RetiredViewportFacts>,
}

impl RestoreRegistryReconciliation {
    pub(crate) fn retained(&self) -> &[(ViewportBinding, ViewportBinding)] {
        &self.retained
    }

    pub(crate) fn retired(&self) -> &[RetiredViewportFacts] {
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
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ViewportRegistry {
    records: BTreeMap<SurfaceId, ViewportRecord>,
    token_to_surface: BTreeMap<WindowToken, SurfaceId>,
    last_token: WindowToken,
    last_incarnation: WindowIncarnation,
    inventory_generation: InventoryGeneration,
    last_coordinate_generation: CoordinateGeneration,
}

impl ViewportRegistry {
    /// Registers an adapter-owned token for an existing logical surface.
    pub(crate) fn register_existing(
        &mut self,
        epoch: WorkspaceEpoch,
        surface: SurfaceId,
        token: WindowToken,
        role: ViewportRole,
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
            ViewportBinding::new(epoch, surface, token, incarnation),
            role,
        )
    }

    /// Reserves a core token for a future native child window.
    pub(crate) fn reserve(
        &mut self,
        epoch: WorkspaceEpoch,
        surface: SurfaceId,
        role: ViewportRole,
    ) -> Result<ViewportBinding, ViewportRegistryError> {
        if self.records.contains_key(&surface) {
            return Err(ViewportRegistryError::SurfaceAlreadyRegistered { surface });
        }
        let token = self
            .last_token
            .checked_next()
            .ok_or(ViewportRegistryError::WindowTokenExhausted)?;
        let incarnation = self.next_incarnation()?;
        let binding = ViewportBinding::new(epoch, surface, token, incarnation);
        self.insert_binding(binding, role)?;
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
        self.token_to_surface
            .insert(binding.token(), binding.surface());
        self.records.insert(
            binding.surface(),
            ViewportRecord {
                binding,
                role,
                lifecycle: ViewportLifecycle::AwaitingObservation,
                coordinates: None,
                input_observations: WindowInputObservationStream::default(),
                coordinate_generation: CoordinateGeneration::default(),
                ever_observed: false,
                close_request_latched: false,
            },
        );
        Ok(binding)
    }

    /// Applies a complete provider window roster.
    ///
    /// Absence proves destruction only when authoritative inventory capability is supported.
    pub(crate) fn apply_snapshot(
        &mut self,
        snapshot: &PlatformSnapshot,
    ) -> Result<RegistryTransition, ViewportRegistryError> {
        let mut candidate = self.clone();
        let transition = candidate.apply_snapshot_in_place(snapshot)?;
        *self = candidate;
        Ok(transition)
    }

    fn apply_snapshot_in_place(
        &mut self,
        snapshot: &PlatformSnapshot,
    ) -> Result<RegistryTransition, ViewportRegistryError> {
        let generation = self
            .inventory_generation
            .checked_next()
            .ok_or(ViewportRegistryError::InventoryGenerationExhausted)?;
        let authoritative = snapshot
            .capabilities()
            .authoritative_inventory()
            .is_supported();
        let observations: BTreeMap<WindowToken, &ObservedWindow> = snapshot
            .windows()
            .iter()
            .map(|window| (window.token(), window))
            .collect();
        let mut events = Vec::new();
        let mut last_coordinate_generation = self.last_coordinate_generation;

        for record in self.records.values_mut() {
            let Some(observation) = observations.get(&record.binding.token()).copied() else {
                if authoritative
                    && record.ever_observed
                    && !matches!(record.lifecycle, ViewportLifecycle::Missing)
                {
                    record.lifecycle = ViewportLifecycle::Missing;
                    record.close_request_latched = false;
                    events.push(RegistryEvent::Destroyed {
                        binding: record.binding,
                    });
                }
                continue;
            };

            record.ever_observed = true;
            let awaiting_destroyed =
                matches!(record.lifecycle, ViewportLifecycle::AwaitingDestroyed);
            let was_ready = record.is_ready();
            let previous_coordinates = record.coordinates;
            record
                .input_observations
                .observe(record.binding, observation.input_observation());
            let mut coordinates = CoordinateSnapshot::from_observation(
                record.binding,
                record.coordinate_generation,
                observation,
            )
            .ok();
            let facts_changed = match (previous_coordinates, coordinates) {
                (Some(previous), Some(current)) => !previous.same_facts(current),
                (None, None) => false,
                (Some(_), None) | (None, Some(_)) => true,
            };
            let placement_facts_changed = match (previous_coordinates, coordinates) {
                (Some(previous), Some(current)) => !previous.same_placement_facts(current),
                (None, None) => false,
                (Some(_), None) | (None, Some(_)) => true,
            };
            if placement_facts_changed {
                let generation = last_coordinate_generation
                    .checked_next()
                    .ok_or(ViewportRegistryError::CoordinateGenerationExhausted)?;
                last_coordinate_generation = generation;
                record.coordinate_generation = generation;
                coordinates = coordinates.map(|snapshot| snapshot.with_generation(generation));
            }
            record.coordinates = coordinates;
            if awaiting_destroyed {
                continue;
            }
            update_close_request(record, observation, &mut events);
            if record.close_request_latched {
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

        self.inventory_generation = generation;
        self.last_coordinate_generation = last_coordinate_generation;
        Ok(RegistryTransition { generation, events })
    }

    pub(crate) fn mark_awaiting_destroyed(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<(), ViewportRegistryError> {
        let record = self.current_record_mut(binding)?;
        record.lifecycle = ViewportLifecycle::AwaitingDestroyed;
        Ok(())
    }

    pub(crate) fn resume_after_failed_close(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<(), ViewportRegistryError> {
        let record = self.current_record_mut(binding)?;
        record.lifecycle = if record.close_request_latched {
            ViewportLifecycle::CloseRequested
        } else if record.coordinates.is_some() {
            ViewportLifecycle::Ready
        } else {
            ViewportLifecycle::AwaitingObservation
        };
        Ok(())
    }

    pub(crate) fn remove_missing(
        &mut self,
        binding: ViewportBinding,
    ) -> Result<(), ViewportRegistryError> {
        let record = self.current_record(binding)?;
        if !matches!(record.lifecycle, ViewportLifecycle::Missing) {
            return Err(ViewportRegistryError::BindingStillObserved { binding });
        }
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
                let record = self
                    .records
                    .remove(&surface)
                    .ok_or(ViewportRegistryError::MissingSurface { surface })?;
                self.token_to_surface.remove(&record.binding.token());
                retired.push(RetiredViewportFacts {
                    binding: record.binding,
                    role: record.role,
                    lifecycle: record.lifecycle,
                    last_coordinates: record.coordinates,
                    input_observations: record.input_observations,
                    ever_observed: record.ever_observed,
                    close_request_latched: record.close_request_latched,
                });
                continue;
            }
            let incarnation = self.next_incarnation()?;
            let record = self
                .records
                .get_mut(&surface)
                .ok_or(ViewportRegistryError::MissingSurface { surface })?;
            let after = ViewportBinding::new(new_epoch, surface, before.token(), incarnation);
            record.binding = after;
            record.lifecycle = ViewportLifecycle::AwaitingObservation;
            record.coordinates = None;
            record.input_observations = WindowInputObservationStream::default();
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

    #[must_use]
    pub fn binding_for_token(&self, token: WindowToken) -> Option<ViewportBinding> {
        self.token_to_surface
            .get(&token)
            .and_then(|surface| self.records.get(surface))
            .map(ViewportRecord::binding)
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
        let coordinates = self
            .records
            .get(&surface)
            .and_then(ViewportRecord::coordinates)
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
                    && record.is_ready()
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

fn update_close_request(
    record: &mut ViewportRecord,
    observation: &ObservedWindow,
    events: &mut Vec<RegistryEvent>,
) {
    let Authority::Known(requested) = observation.close_requested() else {
        return;
    };
    if *requested && !record.close_request_latched {
        record.close_request_latched = true;
        events.push(RegistryEvent::CloseRequested {
            binding: record.binding,
        });
    } else if !requested && record.close_request_latched {
        record.close_request_latched = false;
        events.push(RegistryEvent::CloseRequestCleared {
            binding: record.binding,
        });
    }
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
    #[error("viewport binding remains observed and cannot be removed: {binding:?}")]
    BindingStillObserved { binding: ViewportBinding },
    #[error("viewport binding was already observed and cannot be discarded: {binding:?}")]
    BindingAlreadyObserved { binding: ViewportBinding },
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
    use crate::geometry::{PhysicalRect, ScaleFactor};
    use crate::intent::AuthorityUnavailableReason;
    use crate::platform::{
        PlatformCapabilities, PlatformCapability, WindowInputState, WindowPresentationState,
    };
    use crate::viewport_focus::{FocusObservationGeneration, unknown_focus_observation};

    fn ready_window(token: WindowToken, close_requested: bool) -> ObservedWindow {
        ObservedWindow::new(token)
            .with_content_bounds(Authority::Known(
                PhysicalRect::new(0.0, 0.0, 100.0, 100.0).expect("test bounds must be valid"),
            ))
            .with_outer_bounds(Authority::Known(
                PhysicalRect::new(-5.0, -25.0, 110.0, 130.0).expect("test bounds must be valid"),
            ))
            .with_scale_factor(Authority::Known(
                ScaleFactor::new(1.0).expect("test scale must be valid"),
            ))
            .with_input_state(Authority::Known(WindowInputState::ReceivesInput))
            .with_presentation(Authority::Known(WindowPresentationState::Visible))
            .with_close_requested(Authority::Known(close_requested))
    }

    fn snapshot(generation: u64, windows: Vec<ObservedWindow>) -> PlatformSnapshot {
        let mut capabilities = PlatformCapabilities::default();
        capabilities.set_authoritative_inventory(PlatformCapability::Supported);
        PlatformSnapshot::new(
            capabilities,
            unknown_focus_observation(
                FocusObservationGeneration::new(generation),
                AuthorityUnavailableReason::NotReported,
            ),
            windows,
            Vec::new(),
            Vec::new(),
        )
        .expect("test snapshot must be valid")
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
            .apply_snapshot(&snapshot(1, vec![ready_window(token, false)]))
            .expect("ready snapshot must apply");
        registry
            .apply_snapshot(&snapshot(2, Vec::new()))
            .expect("destroyed snapshot must apply");
        registry
            .remove_missing(first)
            .expect("missing binding can be removed");
        let second = registry
            .register_existing(
                WorkspaceEpoch::new(0),
                SurfaceId::new(2),
                token,
                ViewportRole::Child,
            )
            .expect("reused token must register after destruction");

        assert_ne!(first.incarnation(), second.incarnation());
    }

    #[test]
    fn close_request_is_edge_triggered_and_destruction_requires_inventory_authority() {
        let mut registry = ViewportRegistry::default();
        let token = WindowToken::new(3);
        registry
            .register_existing(
                WorkspaceEpoch::new(0),
                SurfaceId::new(1),
                token,
                ViewportRole::Child,
            )
            .expect("binding must register");
        let first = registry
            .apply_snapshot(&snapshot(1, vec![ready_window(token, true)]))
            .expect("close snapshot must apply");
        assert_eq!(
            first
                .events()
                .iter()
                .filter(|event| matches!(event, RegistryEvent::CloseRequested { .. }))
                .count(),
            1
        );
        let repeated = registry
            .apply_snapshot(&snapshot(2, vec![ready_window(token, true)]))
            .expect("repeated snapshot must apply");
        assert!(
            !repeated
                .events()
                .iter()
                .any(|event| matches!(event, RegistryEvent::CloseRequested { .. }))
        );

        let destroyed = registry
            .apply_snapshot(&snapshot(3, Vec::new()))
            .expect("destroyed snapshot must apply");
        assert!(
            destroyed
                .events()
                .iter()
                .any(|event| matches!(event, RegistryEvent::Destroyed { .. }))
        );
    }

    #[test]
    fn inventory_generation_exhaustion_is_atomic() {
        let mut registry = ViewportRegistry::default();
        registry.exhaust_inventory_generation();
        let before = registry.clone();
        assert_eq!(
            registry.apply_snapshot(&snapshot(1, Vec::new())),
            Err(ViewportRegistryError::InventoryGenerationExhausted)
        );
        assert_eq!(registry, before);
    }

    #[test]
    fn coordinate_generation_exhaustion_is_atomic() {
        let mut registry = ViewportRegistry::default();
        let token = WindowToken::new(3);
        registry
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
            registry.apply_snapshot(&snapshot(1, vec![ready_window(token, false)])),
            Err(ViewportRegistryError::CoordinateGenerationExhausted)
        );
        assert_eq!(registry, before);
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
            .apply_snapshot(&snapshot(1, vec![ready_window(observed.token(), false)]))
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
    fn restore_reconciliation_preserves_retained_close_latch() {
        let mut registry = ViewportRegistry::default();
        let surface = SurfaceId::new(1);
        let token = WindowToken::new(3);
        let before = registry
            .register_existing(WorkspaceEpoch::new(0), surface, token, ViewportRole::Root)
            .expect("binding must register");
        registry
            .apply_snapshot(&snapshot(1, vec![ready_window(token, true)]))
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
        assert!(record.ever_observed);
        assert!(record.close_request_latched);

        let repeated_close = registry
            .apply_snapshot(&snapshot(2, vec![ready_window(token, true)]))
            .expect("same close request must apply to the rebound window");
        assert!(repeated_close.events().is_empty());
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
            .apply_snapshot(&snapshot(1, vec![ready_window(token, false)]))
            .expect("ready snapshot must apply");
        registry
            .mark_awaiting_destroyed(binding)
            .expect("binding must enter its terminal close state");
        let last_coordinates = registry
            .record(surface)
            .expect("record must remain current before restore")
            .coordinates();

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
        assert_eq!(retired.last_coordinates(), last_coordinates);
        assert!(retired.ever_observed());
        assert!(!retired.close_request_latched);
        assert!(registry.record(surface).is_none());
        assert!(registry.binding_for_token(token).is_none());
        assert!(registry.records().next().is_none());
    }

    #[test]
    fn awaiting_destroyed_cannot_return_to_ready_while_window_is_observed() {
        let mut registry = ViewportRegistry::default();
        let surface = SurfaceId::new(1);
        let token = WindowToken::new(3);
        let binding = registry
            .register_existing(WorkspaceEpoch::new(0), surface, token, ViewportRole::Child)
            .expect("binding must register");
        registry
            .apply_snapshot(&snapshot(1, vec![ready_window(token, false)]))
            .expect("ready snapshot must apply");
        registry
            .mark_awaiting_destroyed(binding)
            .expect("current binding can await destruction");

        let still_observed = registry
            .apply_snapshot(&snapshot(2, vec![ready_window(token, true)]))
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
            .apply_snapshot(&snapshot(3, vec![ObservedWindow::new(token)]))
            .expect("incomplete observed snapshot must apply");
        assert_eq!(
            registry
                .record(surface)
                .expect("record must remain registered")
                .lifecycle(),
            ViewportLifecycle::AwaitingDestroyed
        );
        assert!(facts_unavailable.events().is_empty());

        let destroyed = registry
            .apply_snapshot(&snapshot(4, Vec::new()))
            .expect("authoritative absence must apply");
        assert_eq!(
            registry
                .record(surface)
                .expect("missing record remains queryable")
                .lifecycle(),
            ViewportLifecycle::Missing
        );
        assert_eq!(destroyed.events(), &[RegistryEvent::Destroyed { binding }]);
    }

    #[test]
    fn focus_authority_survives_coordinate_gaps_but_not_terminal_lifecycle() {
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
                .is_focusable(),
            "an unobserved reservation cannot own provider focus"
        );

        registry
            .apply_snapshot(&snapshot(1, vec![ready_window(token, false)]))
            .expect("ready snapshot must apply");
        assert!(
            registry
                .record(surface)
                .expect("ready record must exist")
                .is_focusable()
        );

        registry
            .apply_snapshot(&snapshot(2, vec![ObservedWindow::new(token)]))
            .expect("coordinate gap must apply");
        let unavailable = registry
            .record(surface)
            .expect("observed record must remain registered");
        assert_eq!(
            unavailable.lifecycle(),
            ViewportLifecycle::AwaitingObservation
        );
        assert!(unavailable.is_focusable());
        assert!(!unavailable.is_routeable());

        registry
            .mark_awaiting_destroyed(binding)
            .expect("binding must enter terminal close state");
        assert!(
            !registry
                .record(surface)
                .expect("closing record must remain queryable")
                .is_focusable()
        );

        registry
            .apply_snapshot(&snapshot(3, Vec::new()))
            .expect("destroyed snapshot must apply");
        assert!(
            !registry
                .record(surface)
                .expect("missing record remains queryable")
                .is_focusable()
        );
    }
}

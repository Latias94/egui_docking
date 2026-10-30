use std::collections::{BTreeMap, BTreeSet};

use crate::frame::{SurfaceVacancyAuthority, ViewportCoordinator};
use crate::graph::Workspace;
use crate::ids::{NativeCreateSagaId, SurfaceId};
use crate::presentation_observation::{
    HostPresentationEndpoint, NativeStagingResourceId, PresentationOutputSerial,
    SurfacePresentationOutputTicket,
};
use crate::scene::SurfaceCoordinateCapture;
use crate::surface_recovery::SurfaceRecoveryObligationId;
use crate::viewport::ViewportBinding;
use crate::viewport_focus::{FocusCausalStamp, PaneFocusDisposition};
use crate::viewport_registry::ViewportAdmission;

use super::{DockEngine, EngineError};

/// Engine-owned barrier before native topology transfer and live admission.
///
/// The viewport coordinator owns the platform lifecycle saga. This module owns
/// the exact first-live output floor. Source vacancies are recorded only when
/// an accepted first-live output atomically commits topology and admission;
/// their physical retirement remains a tick-final operation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct NativeAdmissionState {
    barriers: BTreeMap<ViewportBinding, FirstLiveBarrier>,
    committed_vacancies: BTreeMap<NativeCreateSagaId, NativeCreateVacancies>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeCreateVacancies {
    source: Option<SurfaceVacancyAuthority>,
    target: SurfaceVacancyAuthority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FirstLiveBarrier {
    owner: FirstLiveOwner,
    resource: Option<NativeStagingResourceId>,
    output_floor: PresentationOutputSerial,
    recovery_activation: Option<(FocusCausalStamp, PaneFocusDisposition)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FirstLiveOwner {
    NativeCreate(NativeCreateSagaId),
    RecoveryReplacement {
        destroyed_binding: ViewportBinding,
        recovery_obligation: SurfaceRecoveryObligationId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NativeAdmissionError {
    DuplicateBarrier,
    DuplicateCommittedVacancies,
    MissingBarrier,
    OwnerMismatch,
    ResourceMismatch,
}

impl NativeAdmissionError {
    pub(super) const fn detail(self) -> &'static str {
        match self {
            Self::DuplicateBarrier => "native first-live barrier was already pending",
            Self::DuplicateCommittedVacancies => "native create vacancies were already committed",
            Self::MissingBarrier => "pending native create had no first-live barrier",
            Self::OwnerMismatch => "first-live barrier did not match its lifecycle owner",
            Self::ResourceMismatch => {
                "pending native create did not match its retained staging resource"
            }
        }
    }
}

impl NativeAdmissionState {
    pub(super) fn reconcile_pending_creates(&mut self, viewport: &ViewportCoordinator) {
        self.barriers
            .retain(|binding, barrier| match barrier.owner {
                FirstLiveOwner::NativeCreate(saga) => viewport
                    .native_create_saga(saga)
                    .is_some_and(|create| create.binding() == *binding),
                FirstLiveOwner::RecoveryReplacement { .. } => true,
            });
    }

    pub(super) fn finish_native_create_first_live(
        &mut self,
        saga: NativeCreateSagaId,
        binding: ViewportBinding,
        resource: NativeStagingResourceId,
        vacancy: Option<SurfaceVacancyAuthority>,
        target_vacancy: SurfaceVacancyAuthority,
    ) -> Result<(), NativeAdmissionError> {
        if self.committed_vacancies.contains_key(&saga) {
            return Err(NativeAdmissionError::DuplicateCommittedVacancies);
        }
        self.admit_first_live(binding, FirstLiveOwner::NativeCreate(saga), Some(resource))?;
        self.committed_vacancies.insert(
            saga,
            NativeCreateVacancies {
                source: vacancy,
                target: target_vacancy,
            },
        );
        Ok(())
    }

    pub(super) fn owns_native_create_barrier(
        &self,
        saga: NativeCreateSagaId,
        binding: ViewportBinding,
        resource: NativeStagingResourceId,
    ) -> bool {
        self.barriers.get(&binding).is_some_and(|barrier| {
            barrier.owner == FirstLiveOwner::NativeCreate(saga)
                && barrier.resource == Some(resource)
        })
    }

    fn register_barrier(
        &mut self,
        target: ViewportBinding,
        owner: FirstLiveOwner,
        resource: Option<NativeStagingResourceId>,
        output_floor: PresentationOutputSerial,
        recovery_activation: Option<(FocusCausalStamp, PaneFocusDisposition)>,
    ) -> Result<(), NativeAdmissionError> {
        if self.barriers.contains_key(&target)
            || self.barriers.values().any(|barrier| barrier.owner == owner)
        {
            return Err(NativeAdmissionError::DuplicateBarrier);
        }
        self.barriers.insert(
            target,
            FirstLiveBarrier {
                owner,
                resource,
                output_floor,
                recovery_activation,
            },
        );
        Ok(())
    }

    pub(super) fn register_native_create(
        &mut self,
        saga: NativeCreateSagaId,
        target: ViewportBinding,
        resource: NativeStagingResourceId,
        output_floor: PresentationOutputSerial,
    ) -> Result<(), NativeAdmissionError> {
        self.register_barrier(
            target,
            FirstLiveOwner::NativeCreate(saga),
            Some(resource),
            output_floor,
            None,
        )
    }

    pub(super) fn register_recovery_replacement(
        &mut self,
        destroyed_binding: ViewportBinding,
        target: ViewportBinding,
        recovery_obligation: SurfaceRecoveryObligationId,
        output_floor: PresentationOutputSerial,
        recovery_activation: Option<(FocusCausalStamp, PaneFocusDisposition)>,
    ) -> Result<(), NativeAdmissionError> {
        self.register_barrier(
            target,
            FirstLiveOwner::RecoveryReplacement {
                destroyed_binding,
                recovery_obligation,
            },
            None,
            output_floor,
            recovery_activation,
        )
    }

    fn barrier_authorized_by(
        &self,
        target: ViewportBinding,
        ticket: SurfacePresentationOutputTicket,
    ) -> Option<FirstLiveBarrier> {
        self.barriers
            .get(&target)
            .copied()
            .filter(|barrier| ticket.was_issued_after(barrier.output_floor))
    }

    fn admit_first_live(
        &mut self,
        target: ViewportBinding,
        owner: FirstLiveOwner,
        resource: Option<NativeStagingResourceId>,
    ) -> Result<(), NativeAdmissionError> {
        let barrier = self
            .barriers
            .remove(&target)
            .ok_or(NativeAdmissionError::MissingBarrier)?;
        if barrier.owner != owner {
            self.barriers.insert(target, barrier);
            return Err(NativeAdmissionError::OwnerMismatch);
        }
        if barrier.resource != resource {
            self.barriers.insert(target, barrier);
            return Err(NativeAdmissionError::ResourceMismatch);
        }
        Ok(())
    }

    pub(super) fn target_destroyed(&mut self, target: ViewportBinding) {
        self.barriers.remove(&target);
    }

    pub(super) fn cancel_recovery_replacement(
        &mut self,
        destroyed_binding: ViewportBinding,
        target: ViewportBinding,
        recovery_obligation: SurfaceRecoveryObligationId,
    ) -> Result<(), NativeAdmissionError> {
        let barrier = self
            .barriers
            .get(&target)
            .copied()
            .ok_or(NativeAdmissionError::MissingBarrier)?;
        let expected = FirstLiveOwner::RecoveryReplacement {
            destroyed_binding,
            recovery_obligation,
        };
        if barrier.owner != expected {
            return Err(NativeAdmissionError::OwnerMismatch);
        }
        self.barriers.remove(&target);
        Ok(())
    }

    pub(super) fn source_surface_is_held(&self, surface: SurfaceId) -> bool {
        self.committed_vacancies.values().any(|vacancies| {
            vacancies
                .source
                .is_some_and(|vacancy| vacancy.surface() == surface)
        })
    }

    pub(super) fn held_source_surfaces(&self) -> BTreeSet<SurfaceId> {
        self.committed_vacancies
            .values()
            .filter_map(|vacancies| vacancies.source.map(SurfaceVacancyAuthority::surface))
            .collect()
    }

    pub(super) fn take_committed_vacancies(
        &mut self,
        workspace: &Workspace,
    ) -> Vec<SurfaceVacancyAuthority> {
        std::mem::take(&mut self.committed_vacancies)
            .into_values()
            .flat_map(|vacancies| [vacancies.source, Some(vacancies.target)])
            .flatten()
            .filter(|vacancy| workspace.surface(vacancy.surface()).is_none())
            .collect()
    }

    pub(super) fn clear(&mut self) {
        self.barriers.clear();
        self.committed_vacancies.clear();
    }
}

impl DockEngine {
    pub(super) fn native_source_surface_is_held(&self, surface: SurfaceId) -> bool {
        self.native_admission.source_surface_is_held(surface)
    }

    pub(super) fn native_first_live_output_requires_follow_up(
        &self,
        ticket: SurfacePresentationOutputTicket,
        endpoint: HostPresentationEndpoint,
    ) -> Result<bool, EngineError> {
        let HostPresentationEndpoint::Native(binding) = endpoint else {
            return Ok(false);
        };
        Ok(self
            .actionable_first_live_barrier(ticket, binding)?
            .is_some())
    }

    fn actionable_first_live_barrier(
        &self,
        ticket: SurfacePresentationOutputTicket,
        binding: ViewportBinding,
    ) -> Result<Option<FirstLiveBarrier>, EngineError> {
        let surface = ticket.surface();
        if binding.surface() != surface {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "first-live native output binding does not match its logical surface",
            });
        }
        let Some(barrier) = self.native_admission.barrier_authorized_by(binding, ticket) else {
            return Ok(None);
        };
        let binding_is_admissible = self.viewport.viewport(surface).is_some_and(|record| {
            record.binding() == binding
                && record.admission() == ViewportAdmission::Pending
                && record.is_ready()
                && record
                    .presentation_observation()
                    .and_then(crate::platform::WindowPresentationObservation::known_state)
                    == Some(crate::platform::WindowPresentationState::Visible)
        });
        Ok(binding_is_admissible.then_some(barrier))
    }

    pub(super) fn admit_first_live_native_output(
        &mut self,
        ticket: SurfacePresentationOutputTicket,
        capture: SurfaceCoordinateCapture,
        events: &mut Vec<crate::event::WorkspaceEvent>,
    ) -> Result<(), EngineError> {
        let surface = ticket.surface();
        let Some(binding) = Self::coordinate_capture_binding(capture) else {
            return Ok(());
        };
        let Some(barrier) = self.actionable_first_live_barrier(ticket, binding)? else {
            return Ok(());
        };
        match barrier.owner {
            FirstLiveOwner::NativeCreate(_) => {
                return Err(EngineError::ReductionCauseInvariant {
                    detail: "a native create can be admitted only by its exact first-live projection",
                });
            }
            FirstLiveOwner::RecoveryReplacement {
                destroyed_binding,
                recovery_obligation,
            } => {
                if !self
                    .bound_surface_recoveries
                    .get(&surface)
                    .is_some_and(|bound| {
                        bound.binding == destroyed_binding
                            && bound.obligation.id() == recovery_obligation
                    })
                {
                    return Err(EngineError::ReductionCauseInvariant {
                        detail: "recovery first-live barrier lost its recovery authority",
                    });
                }
                self.viewport
                    .admit_recovery_replacement_first_live(
                        destroyed_binding,
                        binding,
                        recovery_obligation,
                    )
                    .map_err(|source| EngineError::Viewport {
                        input: self.last_input,
                        source,
                    })?;
                self.native_admission
                    .admit_first_live(
                        binding,
                        FirstLiveOwner::RecoveryReplacement {
                            destroyed_binding,
                            recovery_obligation,
                        },
                        None,
                    )
                    .map_err(|source| EngineError::ReductionCauseInvariant {
                        detail: source.detail(),
                    })?;
                let bound = self
                    .bound_surface_recoveries
                    .get_mut(&surface)
                    .expect("the recovery authority was validated above");
                bound.binding = binding;
                self.surface_recovery.complete_pending(surface);
                self.reconcile_viewport_focus_authority();
                if let Some((focus_causal, pane)) = barrier.recovery_activation {
                    let _ = self.start_viewport_activation(
                        crate::viewport_focus::ViewportActivationRequest::recovery_replacement(
                            binding, pane,
                        ),
                        focus_causal,
                        events,
                    )?;
                }
            }
        }
        self.reconcile_viewport_focus_authority();
        self.viewport
            .validate_native_staging_resource_conservation()
            .map_err(|source| EngineError::Viewport {
                input: self.last_input,
                source,
            })?;
        Ok(())
    }

    pub(super) fn discard_native_first_live_barrier(&mut self, binding: ViewportBinding) {
        self.native_admission.target_destroyed(binding);
    }

    pub(super) fn take_native_create_vacancies(&mut self) -> Vec<SurfaceVacancyAuthority> {
        self.native_admission
            .take_committed_vacancies(&self.workspace)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{EngineAuthorityDomainId, WorkspaceEpoch};
    use crate::viewport::{WindowIncarnation, WindowToken};

    fn binding(surface: u64, incarnation: u64) -> ViewportBinding {
        ViewportBinding::new(
            EngineAuthorityDomainId::new_for_test(1),
            WorkspaceEpoch::new(1),
            SurfaceId::new(surface),
            WindowToken::new(surface),
            WindowIncarnation::new(incarnation),
        )
    }

    fn resource(saga: NativeCreateSagaId) -> NativeStagingResourceId {
        NativeStagingResourceId::mint(EngineAuthorityDomainId::new_for_test(1), saga)
    }

    #[test]
    fn native_create_barrier_is_unique_per_saga() {
        let mut state = NativeAdmissionState::default();
        let saga = NativeCreateSagaId::new(1);
        let target = binding(1, 1);

        state
            .register_native_create(
                saga,
                target,
                resource(saga),
                PresentationOutputSerial::default(),
            )
            .expect("the first native create barrier registers");
        assert_eq!(
            state.register_native_create(
                saga,
                target,
                resource(saga),
                PresentationOutputSerial::default(),
            ),
            Err(NativeAdmissionError::DuplicateBarrier)
        );
        assert_eq!(
            state.finish_native_create_first_live(
                saga,
                target,
                resource(saga),
                None,
                SurfaceVacancyAuthority::capture(target.surface(), None),
            ),
            Ok(())
        );
    }

    #[test]
    fn mismatched_binding_cannot_consume_the_current_barrier() {
        let mut state = NativeAdmissionState::default();
        let saga = NativeCreateSagaId::new(2);
        let target = binding(2, 1);
        state
            .register_native_create(
                saga,
                target,
                resource(saga),
                PresentationOutputSerial::default(),
            )
            .expect("the native create barrier registers");

        assert_eq!(
            state.finish_native_create_first_live(
                saga,
                binding(2, 2),
                resource(saga),
                None,
                SurfaceVacancyAuthority::capture(target.surface(), None),
            ),
            Err(NativeAdmissionError::MissingBarrier)
        );
        assert_eq!(
            state.finish_native_create_first_live(
                saga,
                target,
                resource(saga),
                None,
                SurfaceVacancyAuthority::capture(target.surface(), None),
            ),
            Ok(())
        );
    }

    #[test]
    fn mismatched_resource_cannot_consume_the_current_barrier() {
        let mut state = NativeAdmissionState::default();
        let saga = NativeCreateSagaId::new(3);
        let target = binding(3, 1);
        let expected_resource = resource(saga);
        state
            .register_native_create(
                saga,
                target,
                expected_resource,
                PresentationOutputSerial::default(),
            )
            .expect("the native create barrier registers");

        assert_eq!(
            state.admit_first_live(
                target,
                FirstLiveOwner::NativeCreate(saga),
                Some(resource(NativeCreateSagaId::new(4))),
            ),
            Err(NativeAdmissionError::ResourceMismatch)
        );
        assert_eq!(
            state.admit_first_live(
                target,
                FirstLiveOwner::NativeCreate(saga),
                Some(expected_resource),
            ),
            Ok(())
        );
    }
}

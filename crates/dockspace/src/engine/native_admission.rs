use std::collections::{BTreeMap, BTreeSet};

use crate::frame::{NativeCreatePhase, SurfaceVacancyAuthority, ViewportCoordinator};
use crate::graph::Workspace;
use crate::ids::{NativeCreateSagaId, SurfaceId};
use crate::presentation_observation::{
    NativeStagingResourceId, PresentationOutputSerial, SurfacePresentationOutputTicket,
};
use crate::scene::SurfaceCoordinateCapture;
use crate::surface_recovery::SurfaceRecoveryObligationId;
use crate::viewport::ViewportBinding;
use crate::viewport_focus::{FocusCausalStamp, PaneFocusDisposition};
use crate::viewport_registry::ViewportAdmission;

use super::{DockEngine, EngineError};

/// Engine-owned barrier between native topology transfer and live admission.
///
/// The viewport coordinator owns the platform lifecycle saga. This module owns
/// only the presentation-dependent obligations created by an atomic topology
/// transfer: the exact first-live output floor and any source binding whose
/// retirement must wait for a terminal target outcome.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct NativeAdmissionState {
    barriers: BTreeMap<ViewportBinding, FirstLiveBarrier>,
    source_transitions: BTreeMap<NativeCreateSagaId, PendingSourceTransition>,
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
struct PendingSourceTransition {
    target: ViewportBinding,
    vacancy: SurfaceVacancyAuthority,
    status: SourceTransitionStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceTransitionStatus {
    AwaitingFirstLive,
    SettleAtTickFinal(TargetTerminal),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetTerminal {
    FirstLivePresented,
    Destroyed,
    Vacated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NativeAdmissionError {
    DuplicateBarrier,
    DuplicateSourceTransition,
    MissingBarrier,
    TargetMismatch,
    OwnerMismatch,
    ResourceMismatch,
}

impl NativeAdmissionError {
    pub(super) const fn detail(self) -> &'static str {
        match self {
            Self::DuplicateBarrier => "native first-live barrier was already pending",
            Self::DuplicateSourceTransition => "native source transition was already pending",
            Self::MissingBarrier => "pending native create had no first-live barrier",
            Self::TargetMismatch => "pending native create did not match its first-live target",
            Self::OwnerMismatch => "first-live barrier did not match its lifecycle owner",
            Self::ResourceMismatch => {
                "pending native create did not match its retained staging resource"
            }
        }
    }
}

impl NativeAdmissionState {
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

    pub(super) fn register_transfer(
        &mut self,
        saga: NativeCreateSagaId,
        target: ViewportBinding,
        resource: NativeStagingResourceId,
        output_floor: PresentationOutputSerial,
        source_vacancy: Option<SurfaceVacancyAuthority>,
    ) -> Result<(), NativeAdmissionError> {
        if source_vacancy.is_some() && self.source_transitions.contains_key(&saga) {
            return Err(NativeAdmissionError::DuplicateSourceTransition);
        }
        self.register_barrier(
            target,
            FirstLiveOwner::NativeCreate(saga),
            Some(resource),
            output_floor,
            None,
        )?;
        if let Some(vacancy) = source_vacancy {
            self.source_transitions.insert(
                saga,
                PendingSourceTransition {
                    target,
                    vacancy,
                    status: SourceTransitionStatus::AwaitingFirstLive,
                },
            );
        }
        Ok(())
    }

    pub(super) fn register_recovery_replacement(
        &mut self,
        destroyed_binding: ViewportBinding,
        target: ViewportBinding,
        recovery_obligation: SurfaceRecoveryObligationId,
        resource: Option<NativeStagingResourceId>,
        output_floor: PresentationOutputSerial,
        recovery_activation: Option<(FocusCausalStamp, PaneFocusDisposition)>,
    ) -> Result<(), NativeAdmissionError> {
        self.register_barrier(
            target,
            FirstLiveOwner::RecoveryReplacement {
                destroyed_binding,
                recovery_obligation,
            },
            resource,
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
        if let FirstLiveOwner::NativeCreate(saga) = owner {
            if let Some(transition) = self
                .source_transitions
                .get_mut(&saga)
                .filter(|transition| transition.target == target)
            {
                transition.status =
                    SourceTransitionStatus::SettleAtTickFinal(TargetTerminal::FirstLivePresented);
            }
        }
        Ok(())
    }

    pub(super) fn target_destroyed(&mut self, target: ViewportBinding) {
        let Some(barrier) = self.barriers.remove(&target) else {
            return;
        };
        let FirstLiveOwner::NativeCreate(saga) = barrier.owner else {
            return;
        };
        if let Some(transition) = self
            .source_transitions
            .get_mut(&saga)
            .filter(|transition| transition.target == target)
        {
            transition.status =
                SourceTransitionStatus::SettleAtTickFinal(TargetTerminal::Destroyed);
        }
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

    pub(super) fn targets_vacated(
        &mut self,
        retired: &[(NativeCreateSagaId, ViewportBinding)],
    ) -> Result<Vec<NativeCreateSagaId>, NativeAdmissionError> {
        for (saga, target) in retired {
            let Some(barrier) = self.barriers.get(target) else {
                return Err(
                    if self
                        .barriers
                        .values()
                        .any(|barrier| barrier.owner == FirstLiveOwner::NativeCreate(*saga))
                    {
                        NativeAdmissionError::TargetMismatch
                    } else {
                        NativeAdmissionError::MissingBarrier
                    },
                );
            };
            if barrier.owner != FirstLiveOwner::NativeCreate(*saga) {
                return Err(NativeAdmissionError::OwnerMismatch);
            }
        }
        let mut settled = Vec::with_capacity(retired.len());
        for (saga, target) in retired {
            self.barriers
                .remove(target)
                .expect("the complete vacancy batch was prevalidated");
            if let Some(transition) = self
                .source_transitions
                .get_mut(saga)
                .filter(|transition| transition.target == *target)
            {
                transition.status =
                    SourceTransitionStatus::SettleAtTickFinal(TargetTerminal::Vacated);
            }
            settled.push(*saga);
        }
        Ok(settled)
    }

    pub(super) fn source_surface_is_held(&self, surface: SurfaceId) -> bool {
        self.source_transitions
            .values()
            .any(|transition| transition.vacancy.surface() == surface)
    }

    pub(super) fn held_source_surfaces(&self) -> BTreeSet<SurfaceId> {
        self.source_transitions
            .values()
            .map(|transition| transition.vacancy.surface())
            .collect()
    }

    pub(super) fn take_settleable_source_vacancies(
        &mut self,
        viewport: &ViewportCoordinator,
        workspace: &Workspace,
    ) -> Vec<SurfaceVacancyAuthority> {
        let completed = self
            .source_transitions
            .iter()
            .filter_map(|(saga, transition)| {
                let SourceTransitionStatus::SettleAtTickFinal(_) = transition.status else {
                    return None;
                };
                let target_settled =
                    viewport
                        .viewport(transition.target.surface())
                        .is_none_or(|record| {
                            record.binding() != transition.target
                                || record.admission() == ViewportAdmission::Admitted
                        });
                target_settled.then_some((*saga, *transition))
            })
            .collect::<Vec<_>>();
        completed
            .into_iter()
            .filter_map(|(saga, transition)| {
                self.source_transitions.remove(&saga);
                workspace
                    .surface(transition.vacancy.surface())
                    .is_none()
                    .then_some(transition.vacancy)
            })
            .collect()
    }

    pub(super) fn clear(&mut self) {
        self.barriers.clear();
        self.source_transitions.clear();
    }
}

impl DockEngine {
    pub(super) fn native_source_surface_is_held(&self, surface: SurfaceId) -> bool {
        self.native_admission.source_surface_is_held(surface)
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
        if binding.surface() != surface {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "presented native output binding does not match its logical surface",
            });
        }
        let Some(barrier) = self.native_admission.barrier_authorized_by(binding, ticket) else {
            return Ok(());
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
        if !binding_is_admissible {
            return Ok(());
        }
        match barrier.owner {
            FirstLiveOwner::NativeCreate(saga) => {
                let Some(resource) = barrier.resource else {
                    return Err(EngineError::ReductionCauseInvariant {
                        detail: "first-live native create lost its retained staging resource",
                    });
                };
                let create_resource =
                    self.viewport
                        .native_create_saga(saga)
                        .and_then(|saga| match saga.phase() {
                            NativeCreatePhase::AwaitingFirstLivePresentation { proof } => {
                                proof.retained_resource()
                            }
                            NativeCreatePhase::AwaitingHidden { .. }
                            | NativeCreatePhase::AwaitingPreShowPresentation { .. }
                            | NativeCreatePhase::AwaitingShowAcknowledgement { .. }
                            | NativeCreatePhase::AwaitingVisible { .. }
                            | NativeCreatePhase::AwaitingPostShowPresentation { .. }
                            | NativeCreatePhase::AwaitingOwnershipTransfer { .. } => None,
                        });
                if create_resource != Some(resource)
                    || !self
                        .bound_surface_recoveries
                        .get(&surface)
                        .is_some_and(|bound| {
                            bound.binding == binding
                                && bound.retained_staging_resource == Some(resource)
                        })
                {
                    return Err(EngineError::ReductionCauseInvariant {
                        detail: "first-live barrier lost its bound retained staging resource",
                    });
                }
                self.viewport
                    .admit_native_create_first_live(saga, binding)
                    .map_err(|source| EngineError::Viewport {
                        input: self.last_input,
                        source,
                    })?;
                self.native_admission
                    .admit_first_live(binding, FirstLiveOwner::NativeCreate(saga), Some(resource))
                    .map_err(|source| EngineError::ReductionCauseInvariant {
                        detail: source.detail(),
                    })?;
                self.bound_surface_recoveries
                    .get_mut(&surface)
                    .expect("the retained staging resource was validated above")
                    .retained_staging_resource = None;
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
                            && bound.retained_staging_resource == barrier.resource
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
                        barrier.resource,
                    )
                    .map_err(|source| EngineError::ReductionCauseInvariant {
                        detail: source.detail(),
                    })?;
                let bound = self
                    .bound_surface_recoveries
                    .get_mut(&surface)
                    .expect("the recovery authority was validated above");
                bound.binding = binding;
                bound.retained_staging_resource = None;
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

    pub(super) fn mark_native_source_transition_target_destroyed(
        &mut self,
        binding: ViewportBinding,
    ) {
        self.native_admission.target_destroyed(binding);
    }

    pub(super) fn settle_vacated_native_first_live_barriers(
        &mut self,
        retired: &[(NativeCreateSagaId, ViewportBinding)],
    ) -> Result<(), EngineError> {
        let settled = self
            .native_admission
            .targets_vacated(retired)
            .map_err(|source| EngineError::ReductionCauseInvariant {
                detail: source.detail(),
            })?;
        for saga in settled {
            let _ = self.viewport_focus.cancel_activation_reservation(saga);
        }
        Ok(())
    }

    pub(super) fn take_settleable_native_source_vacancies(
        &mut self,
    ) -> Vec<SurfaceVacancyAuthority> {
        self.native_admission
            .take_settleable_source_vacancies(&self.viewport, &self.workspace)
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
    fn transfer_registration_is_atomic_and_unique_per_saga() {
        let mut state = NativeAdmissionState::default();
        let saga = NativeCreateSagaId::new(1);
        let target = binding(1, 1);

        state
            .register_transfer(
                saga,
                target,
                resource(saga),
                PresentationOutputSerial::default(),
                None,
            )
            .expect("first transfer must register");
        assert_eq!(
            state.register_transfer(
                saga,
                target,
                resource(saga),
                PresentationOutputSerial::default(),
                None,
            ),
            Err(NativeAdmissionError::DuplicateBarrier)
        );
        assert_eq!(state.targets_vacated(&[(saga, target)]), Ok(vec![saga]));
    }

    #[test]
    fn mismatched_vacancy_cannot_consume_the_current_barrier() {
        let mut state = NativeAdmissionState::default();
        let saga = NativeCreateSagaId::new(2);
        let target = binding(2, 1);
        state
            .register_transfer(
                saga,
                target,
                resource(saga),
                PresentationOutputSerial::default(),
                None,
            )
            .expect("transfer must register");

        assert_eq!(
            state.targets_vacated(&[(saga, binding(2, 2))]),
            Err(NativeAdmissionError::TargetMismatch)
        );
        assert_eq!(state.targets_vacated(&[(saga, target)]), Ok(vec![saga]));
    }

    #[test]
    fn mismatched_resource_cannot_consume_the_current_barrier() {
        let mut state = NativeAdmissionState::default();
        let saga = NativeCreateSagaId::new(3);
        let target = binding(3, 1);
        let expected_resource = resource(saga);
        state
            .register_transfer(
                saga,
                target,
                expected_resource,
                PresentationOutputSerial::default(),
                None,
            )
            .expect("transfer must register");

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

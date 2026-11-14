use std::collections::{BTreeMap, BTreeSet};

use crate::engine::surface_recovery::SurfaceRecoveryObligationId;
use crate::graph::Workspace;
use crate::ids::{InputSequence, NativeCreateSagaId, SurfaceId};
use crate::platform::focus::{FocusCausalStamp, PaneFocusDisposition};
use crate::platform::frame::{SurfaceVacancyAuthority, ViewportCoordinator};
use crate::platform::registry::ViewportAdmission;
use crate::platform::viewport::{NativeSurfaceEstablishmentOrigin, ViewportBinding, WindowBinding};
use crate::presentation::observation::{
    HostPresentationEndpoint, NativeStagingResourceId, PresentationOutputSerial,
    PresentedSurfaceAuthority, SurfacePresentationOutputTicket,
};
use crate::presentation::scene::SurfaceCoordinateCapture;

use super::{DockEngine, EngineError};

/// Engine-owned barrier before native topology transfer and live admission.
///
/// The viewport coordinator owns the platform lifecycle saga. This module owns
/// the exact first-live output floor. Source vacancies are recorded only when
/// an accepted first-live output atomically commits topology and admission;
/// their physical retirement remains a tick-final operation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct NativeAdmissionState {
    barriers: BTreeMap<WindowBinding, FirstLiveWindow>,
    committed_vacancies: BTreeMap<NativeCreateSagaId, NativeCreateVacancies>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeCreateVacancies {
    source: Option<SurfaceVacancyAuthority>,
    target: SurfaceVacancyAuthority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FirstLiveBarrier {
    ready_basis: crate::platform::frame::NativeReadyBasis,
    owner: FirstLiveOwner,
    resource: Option<NativeStagingResourceId>,
    recovery_activation: Option<(FocusCausalStamp, PaneFocusDisposition)>,
}

/// One readiness period owns the complete exact placement roster. Individual
/// contributions cannot transfer content or admit a sibling on their own.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FirstLiveWindow {
    members: Vec<FirstLiveMember>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FirstLiveMember {
    binding: ViewportBinding,
    output_floor: PresentationOutputSerial,
    owner: Option<FirstLiveBarrier>,
    presented: Option<PresentedSurfaceAuthority>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FirstLiveOwner {
    NativeCreate(NativeCreateSagaId),
    NativeSurface {
        origin: NativeSurfaceEstablishmentOrigin,
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
            .retain(|window, roster| roster.members.iter().map(|member| member.binding).eq(viewport.registry().bindings_for_window(*window)) && roster.members.iter().all(|member| match member.owner {
                Some(barrier) => viewport.native_ready_basis(member.binding) == Some(barrier.ready_basis) && match barrier.owner {
                    FirstLiveOwner::NativeCreate(saga) => viewport.native_create_saga(saga).is_some_and(|create| create.binding() == member.binding && matches!(create.phase(), crate::platform::frame::NativeCreatePhase::AwaitingFirstLivePresentation { .. })),
                    FirstLiveOwner::NativeSurface { .. } => true,
                },
                None => true,
            }));
    }

    pub(super) fn prepare_window_roster(
        &mut self,
        bindings: &[ViewportBinding],
        output_floor: PresentationOutputSerial,
    ) -> Result<(), NativeAdmissionError> {
        let Some(first) = bindings.first() else {
            return Err(NativeAdmissionError::MissingBarrier);
        };
        if bindings
            .iter()
            .any(|binding| binding.window() != first.window())
            || bindings
                .windows(2)
                .any(|pair| pair[0].surface() >= pair[1].surface())
        {
            return Err(NativeAdmissionError::OwnerMismatch);
        }
        if let Some(roster) = self.barriers.get(&first.window()) {
            return if roster
                .members
                .iter()
                .map(|member| member.binding)
                .eq(bindings.iter().copied())
            {
                Ok(())
            } else {
                Err(NativeAdmissionError::OwnerMismatch)
            };
        }
        self.barriers.insert(
            first.window(),
            FirstLiveWindow {
                members: bindings
                    .iter()
                    .map(|binding| FirstLiveMember {
                        binding: *binding,
                        output_floor,
                        owner: None,
                        presented: None,
                    })
                    .collect(),
            },
        );
        Ok(())
    }

    fn member(&self, binding: ViewportBinding) -> Option<&FirstLiveMember> {
        self.barriers
            .get(&binding.window())?
            .members
            .iter()
            .find(|member| member.binding == binding)
    }

    fn observe(&mut self, authority: PresentedSurfaceAuthority) -> bool {
        let HostPresentationEndpoint::Native(binding) = authority.endpoint() else {
            return false;
        };
        let Some(roster) = self.barriers.get_mut(&binding.window()) else {
            return false;
        };
        let Some(member) = roster
            .members
            .iter_mut()
            .find(|member| member.binding == binding)
        else {
            return false;
        };
        if !authority.ticket().was_issued_after(member.output_floor) {
            return false;
        }
        member.presented = Some(authority);
        roster
            .members
            .iter()
            .all(|member| member.presented.is_some())
    }

    fn presented_roster(&self, binding: ViewportBinding) -> Option<Vec<PresentedSurfaceAuthority>> {
        self.barriers
            .get(&binding.window())?
            .members
            .iter()
            .map(|member| member.presented)
            .collect()
    }

    fn owns_required_output(
        &self,
        target: ViewportBinding,
        ticket: SurfacePresentationOutputTicket,
    ) -> bool {
        self.member(target)
            .is_some_and(|member| ticket.was_issued_after(member.output_floor))
    }

    pub(super) fn revoke_observed_streams(
        &mut self,
        streams: &BTreeSet<crate::presentation::observation::HostPresentationStreamId>,
    ) {
        for member in self
            .barriers
            .values_mut()
            .flat_map(|roster| &mut roster.members)
        {
            if member
                .presented
                .is_some_and(|authority| streams.contains(&authority.stream()))
            {
                member.presented = None;
            }
        }
    }

    fn window_members(&self, binding: ViewportBinding) -> Option<&[FirstLiveMember]> {
        Some(&self.barriers.get(&binding.window())?.members)
    }

    fn remove_owner(&mut self, target: ViewportBinding) {
        if let Some(roster) = self.barriers.get_mut(&target.window()) {
            if let Some(member) = roster
                .members
                .iter_mut()
                .find(|member| member.binding == target)
            {
                member.owner = None;
            }
            if roster.members.iter().all(|member| member.owner.is_none()) {
                self.barriers.remove(&target.window());
            }
        }
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
        self.member(binding)
            .and_then(|member| member.owner)
            .is_some_and(|barrier| {
                barrier.owner == FirstLiveOwner::NativeCreate(saga)
                    && barrier.resource == Some(resource)
            })
    }

    fn register_barrier(
        &mut self,
        target: ViewportBinding,
        ready_basis: crate::platform::frame::NativeReadyBasis,
        owner: FirstLiveOwner,
        resource: Option<NativeStagingResourceId>,
        output_floor: PresentationOutputSerial,
        recovery_activation: Option<(FocusCausalStamp, PaneFocusDisposition)>,
    ) -> Result<(), NativeAdmissionError> {
        if self
            .member(target)
            .is_some_and(|member| member.owner.is_some())
            || self
                .barriers
                .values()
                .flat_map(|roster| &roster.members)
                .any(|member| member.owner.is_some_and(|barrier| barrier.owner == owner))
        {
            return Err(NativeAdmissionError::DuplicateBarrier);
        }
        if !self.barriers.contains_key(&target.window()) {
            self.prepare_window_roster(&[target], output_floor)?;
        }
        let member = self
            .barriers
            .get_mut(&target.window())
            .and_then(|roster| {
                roster
                    .members
                    .iter_mut()
                    .find(|member| member.binding == target)
            })
            .ok_or(NativeAdmissionError::OwnerMismatch)?;
        member.output_floor = output_floor;
        member.presented = None;
        member.owner = Some(FirstLiveBarrier {
            ready_basis,
            owner,
            resource,
            recovery_activation,
        });
        Ok(())
    }

    pub(super) fn register_native_create(
        &mut self,
        saga: NativeCreateSagaId,
        target: ViewportBinding,
        resource: NativeStagingResourceId,
        output_floor: PresentationOutputSerial,
        ready_basis: crate::platform::frame::NativeReadyBasis,
    ) -> Result<(), NativeAdmissionError> {
        self.register_barrier(
            target,
            ready_basis,
            FirstLiveOwner::NativeCreate(saga),
            Some(resource),
            output_floor,
            None,
        )
    }

    pub(super) fn register_native_surface(
        &mut self,
        origin: NativeSurfaceEstablishmentOrigin,
        target: ViewportBinding,
        recovery_obligation: SurfaceRecoveryObligationId,
        output_floor: PresentationOutputSerial,
        recovery_activation: Option<(FocusCausalStamp, PaneFocusDisposition)>,
        ready_basis: crate::platform::frame::NativeReadyBasis,
    ) -> Result<(), NativeAdmissionError> {
        self.register_barrier(
            target,
            ready_basis,
            FirstLiveOwner::NativeSurface {
                origin,
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
        self.member(target)
            .filter(|member| ticket.was_issued_after(member.output_floor))
            .and_then(|member| member.owner)
    }

    fn admit_first_live(
        &mut self,
        target: ViewportBinding,
        owner: FirstLiveOwner,
        resource: Option<NativeStagingResourceId>,
    ) -> Result<(), NativeAdmissionError> {
        let barrier = self
            .member(target)
            .and_then(|member| member.owner)
            .ok_or(NativeAdmissionError::MissingBarrier)?;
        if barrier.owner != owner {
            return Err(NativeAdmissionError::OwnerMismatch);
        }
        if barrier.resource != resource {
            return Err(NativeAdmissionError::ResourceMismatch);
        }
        self.remove_owner(target);
        Ok(())
    }

    pub(super) fn target_destroyed(&mut self, target: ViewportBinding) {
        self.barriers.remove(&target.window());
    }

    pub(super) fn cancel_native_surface(
        &mut self,
        origin: NativeSurfaceEstablishmentOrigin,
        target: ViewportBinding,
        recovery_obligation: SurfaceRecoveryObligationId,
    ) -> Result<(), NativeAdmissionError> {
        // Readiness withdrawal may already have revoked this output barrier.
        // A remaining barrier still requires the exact original recovery owner.
        let Some(barrier) = self.member(target).and_then(|member| member.owner) else {
            return Ok(());
        };
        let expected = FirstLiveOwner::NativeSurface {
            origin,
            recovery_obligation,
        };
        if barrier.owner != expected {
            return Err(NativeAdmissionError::OwnerMismatch);
        }
        self.barriers.remove(&target.window());
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
    /// Reuses valid measurements while putting a replacement's next output above its new floor.
    pub(super) fn renew_recovery_first_live_scene(
        &mut self,
        input: InputSequence,
        binding: ViewportBinding,
    ) -> Result<(), EngineError> {
        let surface = binding.surface();
        let Some(record) = self
            .viewport
            .viewport(surface)
            .filter(|record| record.binding() == binding)
        else {
            return Ok(());
        };
        let Some(candidate) = self.presentation_authority.scene.ready_candidate(surface) else {
            return Ok(());
        };
        if self
            .presentation_requirements()
            .surface(surface)
            .is_none_or(|requirements| candidate.stamp().requirement() != requirements.ticket())
            || !Self::coordinate_capture_matches_current(
                candidate.coordinate_capture(),
                Some(record),
                self.viewport.surface_coordinate_authority(surface),
            )
        {
            return Ok(());
        }
        let plan = candidate.plan().clone();
        let coordinates = candidate.coordinate_capture();
        let serial = self.presentation_authority.issue_scene_output()?;
        self.presentation_authority
            .scene
            .invalidate_presentation_input(surface)
            .map_err(|source| Self::scene_revision_error(input, source))?;
        self.presentation_authority
            .scene
            .install_ready(plan, coordinates, self.authority_domain, serial)
            .map_err(|source| Self::scene_revision_error(input, source))?;
        self.presentation_authority.last_presentation_output_serial = serial;
        Ok(())
    }

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
        if ticket.surface() != binding.surface() {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "first-live native output binding does not match its logical surface",
            });
        }
        Ok(self.native_admission.owns_required_output(binding, ticket)
            && self
                .viewport
                .registry()
                .placement_record(binding)
                .is_some_and(|record| {
                    record.admission() == ViewportAdmission::Pending && record.is_ready()
                })
            && self
                .native_admission
                .member(binding)
                .and_then(|member| member.owner)
                .is_none_or(|barrier| {
                    self.viewport.native_ready_basis(binding) == Some(barrier.ready_basis)
                }))
    }

    /// Retains exact presented evidence until every required placement is ready.
    /// The caller has already checked the incoming scene and coordinate capture.
    pub(super) fn observe_native_first_live_window(
        &mut self,
        authority: PresentedSurfaceAuthority,
    ) -> Option<Vec<PresentedSurfaceAuthority>> {
        let HostPresentationEndpoint::Native(binding) = authority.endpoint() else {
            return None;
        };
        if !self.native_admission.observe(authority) {
            return None;
        }
        let members = self.native_admission.window_members(binding)?;
        if !members.iter().map(|member| member.binding).eq(self
            .viewport
            .registry()
            .bindings_for_window(binding.window()))
            || !members.iter().all(|member| {
                self.viewport
                    .registry()
                    .placement_record(member.binding)
                    .is_some_and(|record| {
                        record.admission() == ViewportAdmission::Pending && record.is_ready()
                    })
                    && member.owner.is_none_or(|barrier| {
                        self.viewport.native_ready_basis(member.binding)
                            == Some(barrier.ready_basis)
                    })
                    && member.presented.is_some_and(|observed| {
                        self.scene().validate_observed_authority(observed).is_ok()
                            && self
                                .scene()
                                .retained_output_capture(observed.ticket())
                                .is_ok_and(|capture| {
                                    Self::coordinate_capture_matches_current(
                                        capture,
                                        self.viewport.viewport(member.binding.surface()),
                                        self.viewport
                                            .surface_coordinate_authority(member.binding.surface()),
                                    )
                                })
                    })
            })
        {
            return None;
        }
        self.native_admission.presented_roster(binding)
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
        if self.viewport.native_ready_basis(binding) != Some(barrier.ready_basis) {
            return Ok(None);
        }
        let binding_is_admissible = self.viewport.viewport(surface).is_some_and(|record| {
            record.binding() == binding
                && matches!(
                    record.admission(),
                    ViewportAdmission::Pending | ViewportAdmission::Admitted
                )
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
        events: &mut Vec<crate::engine::event::WorkspaceEvent>,
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
            FirstLiveOwner::NativeSurface {
                origin,
                recovery_obligation,
            } => {
                if !self
                    .bound_surface_recoveries
                    .get(&surface)
                    .is_some_and(|bound| {
                        bound.binding == origin.binding()
                            && bound.obligation.id() == recovery_obligation
                    })
                {
                    return Err(EngineError::ReductionCauseInvariant {
                        detail: "recovery first-live barrier lost its recovery authority",
                    });
                }
                self.viewport
                    .admit_native_surface_first_live(origin, binding, recovery_obligation)
                    .map_err(|source| EngineError::Viewport {
                        input: self.last_input,
                        source,
                    })?;
                self.native_admission
                    .admit_first_live(
                        binding,
                        FirstLiveOwner::NativeSurface {
                            origin,
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
                        crate::platform::focus::ViewportActivationRequest::recovery_replacement(
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
    use crate::platform::viewport::{WindowIncarnation, WindowToken};

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

    fn authority(binding: ViewportBinding, serial: u64) -> PresentedSurfaceAuthority {
        let requirement = crate::presentation::manifest::SurfaceMeasurementTicket::new(
            binding.authority_domain(),
            binding.epoch(),
            crate::presentation::config::PresentationConfigRevision::new(1),
            crate::policy::PolicyRevision::new(1),
            crate::presentation::manifest::SurfaceRequirementRevision::new(1),
            binding.surface(),
        );
        let ticket = SurfacePresentationOutputTicket::mint(
            binding.authority_domain(),
            PresentationOutputSerial::new_for_test(serial),
            crate::presentation::scene::SurfaceSceneStamp::new(
                requirement,
                crate::presentation::manifest::SurfaceSceneRevision::new(1),
            ),
        );
        let observed = PresentedSurfaceAuthority::mint_observed_for_test(
            ticket,
            crate::platform::viewport::CoordinateGeneration::new(1),
        );
        PresentedSurfaceAuthority::mint_observed(
            ticket,
            observed.stream(),
            observed.emission(),
            HostPresentationEndpoint::Native(binding),
            observed.coordinate_generation(),
        )
    }

    #[test]
    fn window_first_live_requires_each_exact_member_above_its_own_floor() {
        let first = binding(1, 1);
        let second = ViewportBinding::from_window(first.window(), SurfaceId::new(2));
        let mut state = NativeAdmissionState::default();
        state
            .prepare_window_roster(&[first, second], PresentationOutputSerial::new_for_test(3))
            .unwrap();
        assert!(!state.observe(authority(first, 4)));
        assert!(
            !state.observe(authority(first, 5)),
            "a duplicate surface cannot satisfy its sibling"
        );
        assert!(
            !state.observe(authority(second, 3)),
            "the exact floor is not fresh output"
        );
        assert!(
            !state.observe(authority(binding(2, 2), 6)),
            "another window cannot complete the roster"
        );
        let different_placement = ViewportBinding::with_placement_generation(
            second.window(),
            second.surface(),
            crate::platform::viewport::PlacementGeneration::new(1),
        );
        assert!(
            !state.observe(authority(different_placement, 6)),
            "another placement in the same window cannot complete the roster"
        );
        assert!(state.presented_roster(first).is_none());
        assert!(state.observe(authority(second, 6)));
        assert_eq!(
            state.presented_roster(first).unwrap(),
            [authority(first, 5), authority(second, 6)]
        );
    }

    #[test]
    fn first_live_revocation_clears_the_whole_window_and_rejects_prior_output() {
        let first = binding(1, 1);
        let second = ViewportBinding::from_window(first.window(), SurfaceId::new(2));
        let mut state = NativeAdmissionState::default();
        state
            .prepare_window_roster(&[first, second], PresentationOutputSerial::new_for_test(3))
            .unwrap();
        assert!(!state.observe(authority(first, 4)));
        state.target_destroyed(second);
        assert!(state.barriers.is_empty());
        state
            .prepare_window_roster(&[first, second], PresentationOutputSerial::new_for_test(5))
            .unwrap();
        assert!(!state.observe(authority(first, 4)));
        assert!(!state.observe(authority(second, 6)));
        assert!(state.observe(authority(first, 6)));
        state.revoke_observed_streams(&BTreeSet::from([authority(first, 6).stream()]));
        assert!(state.presented_roster(first).is_none());
    }

    #[test]
    fn first_live_roster_rejects_partial_or_foreign_replacement_without_mutation() {
        let first = binding(1, 1);
        let second = ViewportBinding::from_window(first.window(), SurfaceId::new(2));
        let mut state = NativeAdmissionState::default();
        state
            .prepare_window_roster(&[first, second], PresentationOutputSerial::new_for_test(3))
            .unwrap();
        let before = state.clone();
        for bindings in [
            vec![first],
            vec![first, first],
            vec![first, binding(2, 2)],
            vec![second, first],
        ] {
            assert_eq!(
                state.prepare_window_roster(&bindings, PresentationOutputSerial::new_for_test(3)),
                Err(NativeAdmissionError::OwnerMismatch)
            );
            assert_eq!(state, before);
        }
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
                crate::platform::frame::NativeReadyBasis::for_test(),
            )
            .expect("the first native create barrier registers");
        assert_eq!(
            state.register_native_create(
                saga,
                target,
                resource(saga),
                PresentationOutputSerial::default(),
                crate::platform::frame::NativeReadyBasis::for_test(),
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
                crate::platform::frame::NativeReadyBasis::for_test(),
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
                crate::platform::frame::NativeReadyBasis::for_test(),
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

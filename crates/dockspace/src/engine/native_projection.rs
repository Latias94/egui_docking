//! Counterfactual native child layout prepared without publishing source ownership.

use super::presentation_authority::PresentationRequirementFailure;
use super::*;
use crate::tab_strip::PopupPlaneRequirement;

/// Provenance of a first-live layout, not permission to publish its workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeFirstLiveProjectionStamp {
    pub(super) saga: NativeCreateSagaId,
    pub(super) binding: crate::viewport::ViewportBinding,
    pub(super) resource: NativeStagingResourceId,
    ready_basis: crate::frame::NativeReadyBasis,
    base: WorkspaceVersion,
    config: crate::presentation_config::PresentationConfigRevision,
    policy: crate::scene_manifest::PolicyRevision,
}

/// A short-lived compiler input. It is never installed as a topology owner.
///
/// Only the derived requirements and compiled scene may be retained. A later
/// first-live observation must revalidate the provenance and execute the
/// original checked command in the ordinary engine transaction.
#[derive(Debug)]
pub(crate) struct NativeFirstLiveProjection {
    stamp: NativeFirstLiveProjectionStamp,
    workspace: Workspace,
    version: WorkspaceVersion,
    policy: DockPolicySnapshot,
    config: DockPresentationConfig,
    manifest: SceneRequirementManifest,
    #[cfg(test)]
    tab_strip_states: TabStripStateStore,
}

impl NativeFirstLiveProjection {
    pub(crate) const fn stamp(&self) -> NativeFirstLiveProjectionStamp {
        self.stamp
    }

    pub(crate) const fn surface(&self) -> crate::ids::SurfaceId {
        self.stamp.binding.surface()
    }

    #[cfg(test)]
    pub(crate) const fn version(&self) -> WorkspaceVersion {
        self.version
    }

    pub(crate) fn requirements(&self) -> &crate::scene_manifest::SurfaceRequirements {
        self.manifest
            .surface(self.surface())
            .expect("the checked native projection contains its target surface")
    }

    pub(super) fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    #[cfg(test)]
    pub(crate) fn compile(
        &self,
        measurements: &crate::scene_manifest::SurfaceMeasurements,
    ) -> Result<PresentationPlan, SurfaceContributionPrepareError> {
        self.compile_with_state(measurements, self.manifest.popup(), &self.tab_strip_states)
    }

    pub(crate) fn compile_with_state(
        &self,
        measurements: &crate::scene_manifest::SurfaceMeasurements,
        popup: PopupPlaneRequirement,
        tab_strip_states: &TabStripStateStore,
    ) -> Result<PresentationPlan, SurfaceContributionPrepareError> {
        let surface = self.surface();
        if measurements.ticket().surface() != surface {
            return Err(SurfaceContributionPrepareError::SurfaceOutsideRoster { surface });
        }
        let manifest = self.manifest.clone().with_popup(popup);
        compile_surface_plan(
            &self.workspace,
            self.version,
            &self.policy,
            &self.config,
            &manifest,
            measurements,
            tab_strip_states,
            &[],
            None,
        )
        .map_err(SurfaceContributionPrepareError::Compilation)?
        .finalize()
        .map_err(SurfaceContributionPrepareError::Validation)
    }
}

impl DockEngine {
    pub(super) fn commit_native_first_live_projection(
        &mut self,
        stamp: NativeFirstLiveProjectionStamp,
        authority: PresentedSurfaceAuthority,
        cause: ReductionCause,
        events: &mut Vec<WorkspaceEvent>,
        changed_surfaces: &mut BTreeSet<SurfaceId>,
    ) -> Result<bool, EngineError> {
        let ticket = authority.ticket();
        let surface = stamp.binding.surface();
        if !self.native_first_live_projection_is_current(stamp)
            || self
                .scene()
                .ready_candidate(surface)
                .is_none_or(|candidate| candidate.output_ticket() != ticket)
            || !self.native_first_live_output_requires_follow_up(
                ticket,
                HostPresentationEndpoint::Native(stamp.binding),
            )?
            || !self.native_admission.owns_native_create_barrier(
                stamp.saga,
                stamp.binding,
                stamp.resource,
            )
        {
            return Ok(false);
        }
        if let Some(session) = self
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .map(|menu| menu.session())
        {
            // A workspace popup blocks the prospective child's interaction.
            // Dismiss it before transfer and require a new exact content output
            // for the resulting popup plane, retaining durable source ownership.
            self.presentation_authority
                .tab_strip_states
                .close_tab_list_menu(session)
                .map_err(|source| EngineError::PresentationRequirementInvariant {
                    input: Some(self.last_input),
                    detail: source.to_string(),
                })?;
            self.rebuild_presentation_requirements(self.last_input)?;
            changed_surfaces.extend(self.scene().surfaces().map(|(surface, _)| *surface));
            return Ok(false);
        }
        let Some(prepared) = self
            .viewport
            .native_create_saga(stamp.saga)
            .map(|create| create.prepared().clone())
        else {
            return Ok(false);
        };
        let (publication, application) = self.stage_workspace_command_for_native_commit(
            self.last_input,
            &self.policy,
            prepared.command(),
            None,
            stamp.saga,
        )?;
        if !matches!(
            application,
            CommandApplication::Applied { changed: true, .. }
        ) {
            return Ok(false);
        }
        let Some(mut publication) = publication else {
            return Err(EngineError::MissingCommandOutcome {
                input: self.last_input,
            });
        };
        let obligation = prepared.recovery_obligation();
        if obligation.authority_domain() != self.authority_domain
            || obligation.request().source_surface() != surface
            || self.bound_surface_recoveries.contains_key(&surface)
            || Self::recovery_policy_request(
                &publication.workspace,
                surface,
                obligation.request().host_surface(),
            ) != Ok(obligation.request().clone())
        {
            return Ok(false);
        }
        let source_surface = prepared.source_surface();
        let target_vacancy = self.viewport.capture_surface_vacancy_authority(surface);
        let vacancy = publication
            .workspace
            .surface(source_surface)
            .is_none()
            .then(|| {
                self.viewport
                    .capture_surface_vacancy_authority(source_surface)
            });
        self.viewport
            .admit_native_create_first_live(stamp.saga, stamp.binding)
            .map_err(|source| EngineError::Viewport {
                input: self.last_input,
                source,
            })?;
        self.native_admission
            .finish_native_create_first_live(
                stamp.saga,
                stamp.binding,
                stamp.resource,
                vacancy,
                target_vacancy,
            )
            .map_err(|source| EngineError::ReductionCauseInvariant {
                detail: source.detail(),
            })?;
        publication.bound_surface_recoveries.insert(
            surface,
            BoundSurfaceRecovery::new(stamp.binding, obligation.clone()),
        );
        self.publish_workspace(publication);
        self.reconcile_viewport_focus_authority();
        let CommandApplication::Applied { outcome, .. } = application else {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "a checked native first-live command lost its application outcome",
            });
        };
        self.advance_revision_caused(cause)?;
        events.push(WorkspaceEvent::new_caused(
            cause,
            self.version,
            WorkspaceEventKind::CommandCommitted(outcome),
        ));
        let accepted = self
            .presentation_authority
            .scene
            .accept_observed_authority(authority)
            .map_err(|_| EngineError::ReductionCauseInvariant {
                detail: "native first-live transfer invalidated its exact presented output",
            })?;
        changed_surfaces.extend(accepted);
        Ok(true)
    }

    pub(crate) fn native_first_live_projection_is_current(
        &self,
        stamp: NativeFirstLiveProjectionStamp,
    ) -> bool {
        self.version == stamp.base
            && self.viewport.native_ready_basis(stamp.binding) == Some(stamp.ready_basis)
            && self.presentation_authority.presentation_config_revision == stamp.config
            && self.policy.revision() == stamp.policy
            && self.workspace.surface(stamp.binding.surface()).is_none()
            && self
                .viewport
                .native_create_saga(stamp.saga)
                .is_some_and(|saga| {
                    saga.binding() == stamp.binding
                        && saga.resource() == stamp.resource
                        && matches!(
                            saga.phase(),
                            crate::frame::NativeCreatePhase::AwaitingFirstLivePresentation { .. }
                        )
                })
            && self
                .viewport
                .viewport(stamp.binding.surface())
                .is_some_and(|record| {
                    record.binding() == stamp.binding
                        && record.admission()
                            == crate::viewport_registry::ViewportAdmission::Pending
                })
    }

    /// Materializes only a temporary transaction candidate for scene compilation.
    /// This method cannot advance workspace, source sequence, saga or admission.
    pub(crate) fn native_first_live_projection(
        &self,
        saga: NativeCreateSagaId,
    ) -> Result<Option<NativeFirstLiveProjection>, EngineError> {
        let Some(create) = self.viewport.native_create_saga(saga) else {
            return Ok(None);
        };
        let Some(ready_basis) = self.viewport.native_ready_basis(create.binding()) else {
            return Ok(None);
        };
        let stamp = NativeFirstLiveProjectionStamp {
            saga,
            binding: create.binding(),
            resource: create.resource(),
            ready_basis,
            base: self.version,
            config: self.presentation_authority.presentation_config_revision,
            policy: self.policy.revision(),
        };
        if !self.native_first_live_projection_is_current(stamp) {
            return Ok(None);
        }
        let (publication, application) = self.stage_workspace_command_for_native_commit(
            self.last_input,
            &self.policy,
            create.prepared().command(),
            None,
            saga,
        )?;
        if !matches!(
            application,
            CommandApplication::Applied { changed: true, .. }
        ) {
            return Ok(None);
        }
        let Some(publication) = publication else {
            return Err(EngineError::MissingCommandOutcome {
                input: self.last_input,
            });
        };
        let version = WorkspaceVersion::new(
            self.version.epoch(),
            self.version.revision().checked_next().ok_or(
                EngineError::WorkspaceRevisionExhausted {
                    input: self.last_input,
                },
            )?,
        );
        let surface = stamp.binding.surface();
        if publication.workspace.surface(surface).is_none() {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "the checked native projection omitted its target surface",
            });
        }
        let requirement_failure =
            |failure| Self::input_requirement_failure(self.last_input, failure);
        let requirement_revision = self
            .presentation_requirements()
            .revision()
            .checked_next()
            .ok_or_else(|| {
                requirement_failure(PresentationRequirementFailure::RequirementRevisionExhausted)
            })?;
        let mut surface_revisions = self
            .presentation_authority
            .surface_requirement_revisions
            .clone();
        let surface_revision = if self.scene().native_first_live_projection(surface) == Some(stamp)
        {
            self.scene()
                .surface(surface)
                .unwrap()
                .stamp()
                .requirement()
                .surface_requirement()
        } else {
            SurfaceRequirementRevision::new(requirement_revision.get())
        };
        surface_revisions.insert(surface, surface_revision);
        let mut draft = derive_scene_requirement_draft(
            self.authority_domain,
            &publication.workspace,
            version,
            self.presentation_authority.presentation_config_revision,
            &self.policy,
            requirement_revision,
            &surface_revisions,
        )
        .map_err(|source| {
            requirement_failure(PresentationRequirementFailure::Invariant(
                source.to_string(),
            ))
        })?;
        draft.retain_projection_context(surface, self.presentation_requirements());
        let surface_roster_changed = draft.surfaces().map(|(surface, _)| surface).ne(self
            .presentation_requirements()
            .surfaces()
            .map(|(surface, _)| surface));
        let mut tab_strip_states = self.presentation_authority.tab_strip_states.clone();
        Self::reconcile_tab_strip_state_store_with_sources(
            &draft,
            &mut tab_strip_states,
            surface_roster_changed,
            |current| {
                if current == surface {
                    &publication.workspace
                } else {
                    &self.workspace
                }
            },
        )
        .map_err(|source| {
            requirement_failure(PresentationRequirementFailure::Invariant(
                source.to_string(),
            ))
        })?;
        let manifest = draft
            .finalize(tab_strip_states.popup_requirement())
            .map_err(|source| {
                requirement_failure(PresentationRequirementFailure::Invariant(
                    source.to_string(),
                ))
            })?;
        Ok(Some(NativeFirstLiveProjection {
            stamp,
            workspace: publication.workspace.into_workspace(),
            version,
            policy: self.policy.clone(),
            config: self.presentation_authority.presentation_config.clone(),
            manifest,
            #[cfg(test)]
            tab_strip_states,
        }))
    }
}

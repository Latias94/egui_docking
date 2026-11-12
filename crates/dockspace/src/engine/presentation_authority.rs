//! Presentation requirements, scene authority, and output reconciliation.

mod continuation;

use super::native_projection::NativeFirstLiveProjection;
use super::presentation_roster::PresentationAllocationIssuer;
use super::*;

#[cfg(test)]
mod snapshot_tests;

/// Sole owner of renderer-neutral presentation state for one engine domain.
///
/// Keeping the manifest, transient tab-strip state, compiled scene, and
/// presentation ledger together prevents callers from publishing one of these
/// authorities without advancing the others through the engine reducer.
#[derive(Debug, Clone)]
pub(super) struct PresentationAuthorityState {
    /// Non-rollbackable attempt and output allocation shared by all candidates.
    allocation_issuer: Arc<PresentationAllocationIssuer>,
    pub(super) presentation_config: DockPresentationConfig,
    pub(super) presentation_config_revision: PresentationConfigRevision,
    pub(super) presentation_requirements: SceneRequirementManifest,
    /// Weak provenance of the index and live semantic snapshots; never retains
    /// an old complete workspace. Prospective native snapshots are separate inputs.
    workspace_index_source: std::sync::Weak<Workspace>,
    pub(super) tab_strip_states: TabStripStateStore,
    pub(super) surface_semantic_snapshots: BTreeMap<crate::ids::SurfaceId, SurfaceSemanticSnapshot>,
    /// Current per-surface revisions. Changed/new surfaces use the existing
    /// aggregate requirement revision, so reattachment needs no history map.
    pub(super) surface_requirement_revisions:
        BTreeMap<crate::ids::SurfaceId, SurfaceRequirementRevision>,
    pub(super) scene: SurfaceSceneSet,
    pub(super) presentation: PresentationLedger,
    pub(super) last_presentation_output_serial: PresentationOutputSerial,
    pub(super) selection_rebind: Option<SelectionRebind>,
}

/// A canonical Select preserves structural inventories; geometry still needs current input checks.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SelectionRebind {
    pub(super) surface: SurfaceId,
    pub(super) bar: crate::scene::TabBarSceneId,
    pub(super) version: WorkspaceVersion,
    pub(super) coordinates: SurfaceCoordinateCapture,
    pub(super) plan: Arc<PresentationPlan>,
}

impl PartialEq for PresentationAuthorityState {
    fn eq(&self, other: &Self) -> bool {
        // Reuse preparation and index provenance are not published semantic authority.
        let Self {
            allocation_issuer,
            presentation_config,
            presentation_config_revision,
            presentation_requirements,
            workspace_index_source: _,
            tab_strip_states,
            surface_semantic_snapshots,
            surface_requirement_revisions,
            scene,
            presentation,
            last_presentation_output_serial,
            selection_rebind: _,
        } = self;
        let Self {
            allocation_issuer: other_allocation_issuer,
            presentation_config: other_presentation_config,
            presentation_config_revision: other_presentation_config_revision,
            presentation_requirements: other_presentation_requirements,
            workspace_index_source: _,
            tab_strip_states: other_tab_strip_states,
            surface_semantic_snapshots: other_surface_semantic_snapshots,
            surface_requirement_revisions: other_surface_requirement_revisions,
            scene: other_scene,
            presentation: other_presentation,
            last_presentation_output_serial: other_last_presentation_output_serial,
            selection_rebind: _,
        } = other;
        allocation_issuer == other_allocation_issuer
            && presentation_config == other_presentation_config
            && presentation_config_revision == other_presentation_config_revision
            && presentation_requirements == other_presentation_requirements
            && tab_strip_states == other_tab_strip_states
            && surface_semantic_snapshots == other_surface_semantic_snapshots
            && surface_requirement_revisions == other_surface_requirement_revisions
            && scene == other_scene
            && presentation == other_presentation
            && last_presentation_output_serial == other_last_presentation_output_serial
    }
}

impl PresentationAuthorityState {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        authority_domain: EngineAuthorityDomainId,
        workspace_index_source: &Arc<Workspace>,
        presentation_config: DockPresentationConfig,
        presentation_config_revision: PresentationConfigRevision,
        presentation_requirements: SceneRequirementManifest,
        tab_strip_states: TabStripStateStore,
        surface_semantic_snapshots: BTreeMap<crate::ids::SurfaceId, SurfaceSemanticSnapshot>,
        surface_requirement_revisions: BTreeMap<crate::ids::SurfaceId, SurfaceRequirementRevision>,
        scene: SurfaceSceneSet,
        presentation: PresentationLedger,
    ) -> Self {
        Self {
            allocation_issuer: Arc::new(PresentationAllocationIssuer::new(authority_domain)),
            presentation_config,
            presentation_config_revision,
            presentation_requirements,
            workspace_index_source: Arc::downgrade(workspace_index_source),
            tab_strip_states,
            surface_semantic_snapshots,
            surface_requirement_revisions,
            scene,
            presentation,
            last_presentation_output_serial: PresentationOutputSerial::default(),
            selection_rebind: None,
        }
    }

    pub(super) fn workspace_index_for(
        &self,
        workspace: &Arc<Workspace>,
        version: WorkspaceVersion,
    ) -> Option<&crate::workspace::WorkspaceIndex> {
        let index = self.presentation_requirements.workspace_index();
        // A new publication or Arc::make_mut dissociates this allocation even
        // before its revision advances. A miss must retain live validation.
        (self
            .workspace_index_source
            .ptr_eq(&Arc::downgrade(workspace))
            && index.version() == version)
            .then_some(index)
    }

    fn surface_semantic_snapshots_for(
        &self,
        workspace: &Arc<Workspace>,
        version: WorkspaceVersion,
    ) -> Option<BTreeMap<SurfaceId, SurfaceSemanticSnapshot>> {
        self.workspace_index_for(workspace, version)?;
        // The owner publishes these snapshots with the indexed allocation.
        // Exclude prospective native surfaces: their provenance must still be
        // checked against the current saga before they join the live roster.
        workspace
            .surfaces()
            .map(|(surface, _)| {
                self.surface_semantic_snapshots
                    .get(&surface)
                    .cloned()
                    .map(|snapshot| (surface, snapshot))
            })
            .collect()
    }

    pub(super) fn issue_host_presentation_attempt(
        &self,
    ) -> Result<HostPresentationAttemptId, CoreHostFrameError> {
        self.allocation_issuer.issue()
    }

    pub(super) fn issue_scene_output(&self) -> Result<PresentationOutputSerial, EngineError> {
        self.allocation_issuer.issue_output()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SurfaceSemanticSnapshot {
    main: Option<NodeFingerprint>,
    contained: Vec<ContainedSemanticSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ContainedSemanticSnapshot {
    floating: crate::ids::FloatingPresentationId,
    root: NodeFingerprint,
    rect_bits: [u64; 4],
}

#[derive(Debug, Error)]
pub(super) enum PresentationRequirementFailure {
    #[error("aggregate requirement revision is exhausted")]
    RequirementRevisionExhausted,
    #[error("presentation requirement invariant failed: {0}")]
    Invariant(String),
    #[error(transparent)]
    Scene(#[from] SceneBuildError),
}

impl DockEngine {
    pub(super) fn advance_revision(&mut self, input: InputSequence) -> Result<(), EngineError> {
        self.presentation_authority.selection_rebind = None;
        let revision = self
            .version
            .revision()
            .checked_next()
            .ok_or(EngineError::WorkspaceRevisionExhausted { input })?;
        self.version = WorkspaceVersion::new(self.version.epoch(), revision);
        self.reconcile_close_scopes();
        self.rebuild_presentation_requirements(input)?;
        Ok(())
    }

    pub(super) fn advance_selection_revision(
        &mut self,
        input: InputSequence,
        source: &crate::command::ItemSource,
    ) -> Result<(), EngineError> {
        self.presentation_authority.selection_rebind = None;
        let revision = self
            .version
            .revision()
            .checked_next()
            .ok_or(EngineError::WorkspaceRevisionExhausted { input })?;
        self.version = WorkspaceVersion::new(self.version.epoch(), revision);
        self.reconcile_close_scopes();
        let eligible = self.viewport.native_create_sagas().next().is_none()
            && self.surface_recovery.is_empty()
            && self.bound_surface_recoveries.is_empty()
            && self.presentation_requirements().config()
                == self.presentation_authority.presentation_config_revision
            && self.presentation_requirements().popup()
                == self
                    .presentation_authority
                    .tab_strip_states
                    .popup_requirement();
        if eligible {
            let update = self
                .presentation_requirements()
                .after_selection(
                    &self.workspace,
                    self.version,
                    &self.policy,
                    source.root(),
                    source.tabs(),
                )
                .map_err(|source| {
                    Self::input_requirement_failure(
                        input,
                        PresentationRequirementFailure::Invariant(source.to_string()),
                    )
                })?;
            if let Some((surface, requirements)) = update {
                let snapshot = self
                    .presentation_authority
                    .surface_semantic_snapshots
                    .get(&surface);
                let basis = requirements
                    .workspace_index()
                    .root_fingerprint(source.root());
                if snapshot.is_some_and(|snapshot| {
                    snapshot.main.is_some() && snapshot.contained.is_empty()
                }) && basis.is_some()
                {
                    let previous = self
                        .scene()
                        .surface(surface)
                        .and_then(SurfaceScene::ready)
                        .and_then(|ready| {
                            ready.paint_fallback().filter(|fallback| {
                                fallback.output_ticket() == ready.output_ticket()
                            })
                        })
                        .filter(|previous| {
                            previous.plan().contained_projection().is_none()
                                && previous.plan().resize_projection().is_empty()
                        })
                        .map(|previous| SelectionRebind {
                            surface,
                            bar: crate::scene::TabBarSceneId {
                                root: source.root(),
                                tabs: source.tabs(),
                            },
                            version: self.version,
                            coordinates: previous.coordinate_capture(),
                            plan: Arc::clone(previous.shared_plan()),
                        });
                    let basis = basis.cloned();
                    self.presentation_authority
                        .surface_semantic_snapshots
                        .get_mut(&surface)
                        .expect("the selected surface snapshot was checked")
                        .main = basis;
                    self.presentation_authority
                        .surface_requirement_revisions
                        .insert(
                            surface,
                            requirements
                                .surface(surface)
                                .expect("the selected surface is retained")
                                .ticket()
                                .surface_requirement(),
                        );
                    self.presentation_authority.presentation_requirements = requirements;
                    self.presentation_authority.workspace_index_source =
                        Arc::downgrade(self.workspace.as_arc());
                    self.presentation_authority
                        .scene
                        .reconcile_manifest(&self.presentation_authority.presentation_requirements)
                        .map_err(|source| {
                            Self::input_requirement_failure(
                                input,
                                PresentationRequirementFailure::Scene(source),
                            )
                        })?;
                    self.presentation_authority.selection_rebind = previous;
                    return Ok(());
                }
            }
        }
        self.rebuild_presentation_requirements(input)
    }

    pub(super) fn advance_revision_caused(
        &mut self,
        cause: ReductionCause,
    ) -> Result<(), EngineError> {
        self.advance_revision_caused_with_influence(cause, None)
    }

    pub(super) fn advance_selection_revision_caused(
        &mut self,
        cause: ReductionCause,
        source: &crate::command::ItemSource,
    ) -> Result<(), EngineError> {
        self.advance_selection_revision(self.last_input, source)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: source.to_string(),
            })
    }

    pub(super) fn advance_revision_caused_with_tab_strip_delta(
        &mut self,
        cause: ReductionCause,
        delta: &TabStripStateDelta,
    ) -> Result<(), EngineError> {
        self.advance_revision_caused_with_influence(cause, Some(delta.influence()))
    }

    fn advance_revision_caused_with_influence(
        &mut self,
        cause: ReductionCause,
        influence: Option<&TabStripInfluenceDomain>,
    ) -> Result<(), EngineError> {
        self.presentation_authority.selection_rebind = None;
        let revision = self.version.revision().checked_next().ok_or_else(|| {
            EngineError::PointerInteractionInvariant {
                cause,
                detail: "workspace revision is exhausted".to_owned(),
            }
        })?;
        self.version = WorkspaceVersion::new(self.version.epoch(), revision);
        self.reconcile_close_scopes();
        self.try_rebuild_presentation_requirements(influence)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: source.to_string(),
            })?;
        Ok(())
    }

    pub(super) fn rebuild_presentation_requirements(
        &mut self,
        input: InputSequence,
    ) -> Result<(), EngineError> {
        self.try_rebuild_presentation_requirements(None)
            .map_err(|failure| Self::input_requirement_failure(input, failure))
    }

    pub(super) fn try_rebuild_presentation_requirements(
        &mut self,
        requested_influence: Option<&TabStripInfluenceDomain>,
    ) -> Result<(), PresentationRequirementFailure> {
        let recovering_sources = self
            .viewport
            .native_create_sagas()
            .filter_map(|(saga, create)| {
                let source = create.prepared().source_surface();
                self.surface_recovery
                    .pending(source)
                    .is_some()
                    .then_some(saga)
            })
            .collect::<Vec<_>>();
        for saga in recovering_sources {
            // Source recovery revokes every preparation phase, including one
            // which lost readiness before its former first-live scene rebuilt.
            self.viewport
                .reject_native_create(saga)
                .map_err(|source| PresentationRequirementFailure::Invariant(source.to_string()))?;
            let _ = self.viewport_focus.cancel_activation_reservation(saga);
        }
        self.native_admission
            .reconcile_pending_creates(&self.viewport);
        let workspace_index = if self
            .presentation_authority
            .workspace_index_for(self.workspace.as_arc(), self.version)
            .is_some()
        {
            self.presentation_requirements().shared_workspace_index()
        } else {
            Arc::new(
                crate::workspace::WorkspaceIndex::build(&self.workspace, self.version).map_err(
                    |source| {
                        let error =
                            crate::scene_compiler::SceneRequirementDerivationError::WorkspaceIndex {
                                source,
                            };
                        PresentationRequirementFailure::Invariant(error.to_string())
                    },
                )?,
            )
        };
        let mut surface_semantic_snapshots = match self
            .presentation_authority
            .surface_semantic_snapshots_for(self.workspace.as_arc(), self.version)
        {
            Some(snapshots) => snapshots,
            None => Self::capture_surface_semantic_snapshots(&self.workspace, &workspace_index)
                .map_err(|source| PresentationRequirementFailure::Invariant(source.to_string()))?,
        };
        let mut native_projections = BTreeMap::new();
        let pending_creates = self
            .viewport
            .native_create_sagas()
            .filter_map(|(saga, create)| {
                (matches!(
                    create.phase(),
                    crate::frame::NativeCreatePhase::AwaitingFirstLivePresentation { .. }
                ) && self.workspace.surface(create.binding().surface()).is_none())
                .then_some(saga)
            })
            .collect::<Vec<_>>();
        let requirements = self.presentation_requirements();
        let unchanged_roster = surface_semantic_snapshots.len() + pending_creates.len()
            == self.presentation_authority.surface_semantic_snapshots.len()
            && surface_semantic_snapshots
                .iter()
                .all(|(surface, snapshot)| {
                    self.presentation_authority
                        .surface_semantic_snapshots
                        .get(surface)
                        == Some(snapshot)
                })
            && pending_creates.iter().all(|saga| {
                self.viewport
                    .native_create_saga(*saga)
                    .and_then(|create| {
                        self.scene()
                            .native_first_live_projection(create.binding().surface())
                    })
                    .is_some_and(|stamp| self.native_first_live_projection_is_current(stamp))
            });
        if unchanged_roster
            && self.surface_recovery.is_empty()
            && self.bound_surface_recoveries.is_empty()
            && requested_influence.is_none_or(TabStripInfluenceDomain::is_empty)
            && requirements.workspace() == self.version
            && requirements.config() == self.presentation_authority.presentation_config_revision
            && requirements.policy() == self.policy.revision()
            && requirements.popup()
                == self
                    .presentation_authority
                    .tab_strip_states
                    .popup_requirement()
        {
            // Stable pending children retain only their scene provenance.
            // Materialize a counterfactual workspace only when inputs changed.
            return Ok(());
        }
        for saga in pending_creates {
            let Some(projection) = self
                .native_first_live_projection(saga)
                .map_err(|source| PresentationRequirementFailure::Invariant(source.to_string()))?
            else {
                // A new policy may revoke the prospective command before its
                // first content output. Cancel the original lifecycle owner;
                // source topology has never moved and needs no recovery plan.
                self.viewport.reject_native_create(saga).map_err(|source| {
                    PresentationRequirementFailure::Invariant(source.to_string())
                })?;
                let _ = self.viewport_focus.cancel_activation_reservation(saga);
                continue;
            };
            // A prospective workspace has independent topology and issuance.
            // Its roots must never borrow the live index by matching RootId.
            let mut snapshots =
                Self::capture_surface_semantic_snapshots_with(projection.workspace(), |root| {
                    projection.workspace().root_fingerprint(root)
                })
                .map_err(|source| PresentationRequirementFailure::Invariant(source.to_string()))?;
            let surface = projection.surface();
            let snapshot = snapshots.remove(&surface).ok_or_else(|| {
                PresentationRequirementFailure::Invariant(
                    "native content projection omitted its surface snapshot".to_owned(),
                )
            })?;
            surface_semantic_snapshots.insert(surface, snapshot);
            native_projections.insert(surface, projection);
        }
        self.native_admission
            .reconcile_pending_creates(&self.viewport);
        let previous_surface_roster = self
            .presentation_authority
            .presentation_requirements
            .surfaces()
            .map(|(surface, _)| surface)
            .collect::<BTreeSet<_>>();
        let current_surface_roster = surface_semantic_snapshots
            .keys()
            .copied()
            .collect::<BTreeSet<_>>();
        let native_origins_changed = native_projections.iter().any(|(surface, projection)| {
            self.scene().native_first_live_projection(*surface) != Some(projection.stamp())
        });
        let surface_roster_changed = previous_surface_roster != current_surface_roster;
        let popup_requirement_changed = self
            .presentation_authority
            .presentation_requirements
            .popup()
            != self
                .presentation_authority
                .tab_strip_states
                .popup_requirement();
        let manifest_identity_changed = self
            .presentation_authority
            .presentation_requirements
            .workspace()
            != self.version
            || self
                .presentation_authority
                .presentation_requirements
                .config()
                != self.presentation_authority.presentation_config_revision
            || self
                .presentation_authority
                .presentation_requirements
                .policy()
                != self.policy.revision();
        let requested_influence = requested_influence
            .cloned()
            .unwrap_or_else(|| TabStripInfluenceDomain::Local(BTreeSet::new()));
        if !manifest_identity_changed
            && surface_semantic_snapshots == self.presentation_authority.surface_semantic_snapshots
            && !popup_requirement_changed
            && requested_influence.is_empty()
            && !native_origins_changed
        {
            return Ok(());
        }

        self.presentation_authority.selection_rebind = None;
        let requirement_revision = self
            .presentation_authority
            .presentation_requirements
            .revision()
            .checked_next()
            .ok_or(PresentationRequirementFailure::RequirementRevisionExhausted)?;
        let mut surface_requirement_revisions = self
            .presentation_authority
            .surface_requirement_revisions
            .clone();
        surface_requirement_revisions.retain(|surface, _| current_surface_roster.contains(surface));
        let mut revised_surfaces = BTreeSet::new();
        for surface in current_surface_roster.iter().copied() {
            let semantics_changed = self
                .presentation_authority
                .surface_semantic_snapshots
                .get(&surface)
                != surface_semantic_snapshots.get(&surface)
                || native_projections.get(&surface).is_some_and(|projection| {
                    self.scene().native_first_live_projection(surface) != Some(projection.stamp())
                });
            if semantics_changed {
                let revision = SurfaceRequirementRevision::new(requirement_revision.get());
                surface_requirement_revisions.insert(surface, revision);
                revised_surfaces.insert(surface);
            }
        }
        let mut draft = derive_scene_requirement_draft_with_index(
            self.authority_domain,
            &self.workspace,
            self.version,
            workspace_index,
            self.presentation_authority.presentation_config_revision,
            &self.policy,
            requirement_revision,
            &surface_requirement_revisions,
        )
        .map_err(|source| PresentationRequirementFailure::Invariant(source.to_string()))?;
        for (surface, projection) in &native_projections {
            draft.insert_native_projection(
                projection
                    .requirements()
                    .clone()
                    .with_surface_requirement(surface_requirement_revisions[surface]),
            );
        }
        let mut tab_strip_states = self.presentation_authority.tab_strip_states.clone();
        let tab_strip_delta = Self::reconcile_tab_strip_state_store_with_sources(
            &draft,
            &mut tab_strip_states,
            surface_roster_changed,
            |surface| {
                native_projections
                    .get(&surface)
                    .map_or(&self.workspace, NativeFirstLiveProjection::workspace)
            },
        )
        .map_err(|source| PresentationRequirementFailure::Invariant(source.to_string()))?;
        let popup_mismatch_influence = if popup_requirement_changed {
            TabStripInfluenceDomain::Popup(BTreeSet::new())
        } else {
            TabStripInfluenceDomain::Local(BTreeSet::new())
        };
        let influence = requested_influence
            .merge(popup_mismatch_influence)
            .merge(tab_strip_delta.influence().clone());
        let affected_surfaces = match influence {
            TabStripInfluenceDomain::Local(surfaces) => surfaces,
            TabStripInfluenceDomain::Popup(mut surfaces) => {
                surfaces.extend(
                    self.presentation_requirements()
                        .popup()
                        .owner()
                        .into_iter()
                        .chain(tab_strip_states.popup_requirement().owner())
                        .map(|owner| owner.surface()),
                );
                // Prospective children must compile the current routing context
                // before their exact first-live output may transfer ownership.
                surfaces.extend(native_projections.keys().copied());
                surfaces
            }
        };
        let mut draft_revisions_changed = false;
        for surface in affected_surfaces {
            if !current_surface_roster.contains(&surface) || revised_surfaces.contains(&surface) {
                continue;
            }
            let revision = SurfaceRequirementRevision::new(requirement_revision.get());
            surface_requirement_revisions.insert(surface, revision);
            revised_surfaces.insert(surface);
            draft_revisions_changed = true;
        }
        if draft_revisions_changed {
            let workspace_index = draft.shared_workspace_index();
            draft = derive_scene_requirement_draft_with_index(
                self.authority_domain,
                &self.workspace,
                self.version,
                workspace_index,
                self.presentation_authority.presentation_config_revision,
                &self.policy,
                requirement_revision,
                &surface_requirement_revisions,
            )
            .map_err(|source| PresentationRequirementFailure::Invariant(source.to_string()))?;
            for (surface, projection) in &native_projections {
                draft.insert_native_projection(
                    projection
                        .requirements()
                        .clone()
                        .with_surface_requirement(surface_requirement_revisions[surface]),
                );
            }
        }
        let requirements = draft
            .finalize(tab_strip_states.popup_requirement())
            .map_err(|source| PresentationRequirementFailure::Invariant(source.to_string()))?;
        self.presentation_authority.presentation_requirements = requirements;
        self.presentation_authority.workspace_index_source =
            Arc::downgrade(self.workspace.as_arc());
        self.presentation_authority.tab_strip_states = tab_strip_states;
        let changed_stacking = surface_semantic_snapshots
            .iter()
            .filter_map(|(surface, current)| {
                self.presentation_authority
                    .surface_semantic_snapshots
                    .get(surface)
                    .filter(|previous| {
                        !previous
                            .contained
                            .iter()
                            .map(|record| record.floating)
                            .eq(current.contained.iter().map(|record| record.floating))
                    })
                    .map(|_| *surface)
            })
            .collect::<BTreeSet<_>>();
        self.presentation_authority.surface_semantic_snapshots = surface_semantic_snapshots;
        self.presentation_authority.surface_requirement_revisions = surface_requirement_revisions;
        self.presentation_authority
            .scene
            .reconcile_manifest(&self.presentation_authority.presentation_requirements)
            .map_err(PresentationRequirementFailure::Scene)?;
        self.presentation_authority
            .scene
            .revoke_surface_input(&changed_stacking);
        for surface in current_surface_roster {
            self.presentation_authority
                .scene
                .set_native_first_live_projection(
                    surface,
                    native_projections
                        .get(&surface)
                        .map(|projection| projection.stamp()),
                );
        }
        Ok(())
    }

    pub(super) fn reconcile_tab_strip_state_store(
        workspace: &Workspace,
        requirements: &SceneRequirementDraft,
        states: &mut TabStripStateStore,
        surface_roster_changed: bool,
    ) -> Result<TabStripStateDelta, TabStripStateError> {
        Self::reconcile_tab_strip_state_store_with_sources(
            requirements,
            states,
            surface_roster_changed,
            |_| workspace,
        )
    }

    pub(super) fn reconcile_tab_strip_state_store_with_sources<'workspace>(
        requirements: &SceneRequirementDraft,
        states: &mut TabStripStateStore,
        surface_roster_changed: bool,
        workspace_for_surface: impl Fn(SurfaceId) -> &'workspace Workspace,
    ) -> Result<TabStripStateDelta, TabStripStateError> {
        let live = requirements
            .surfaces()
            .flat_map(|(surface, requirements)| {
                let workspace = workspace_for_surface(surface);
                requirements.tab_strips().filter_map(move |strip| {
                    let bar = strip.bar();
                    match workspace.node(bar.tabs) {
                        Some(Node::Tabs { items, .. }) => {
                            Some((TabStripStateKey::new(surface, bar), items.as_slice()))
                        }
                        Some(Node::Split { .. }) | None => None,
                    }
                })
            })
            .collect::<Vec<_>>();
        let mut delta = states.reconcile_exact_for_surface_roster(live, surface_roster_changed)?;

        let active = states.active_menu().map(|menu| menu.session());
        let active_is_enabled = active.is_some_and(|session| {
            requirements
                .surface(session.key().surface())
                .and_then(|surface| surface.tab_bar(session.key().bar()))
                .is_some_and(|bar| bar.policy().interaction() == TabBarInteraction::Enabled)
        });
        if let Some(session) = active
            && !active_is_enabled
        {
            delta = delta.merge(states.close_tab_list_menu(session)?);
        }
        Ok(delta)
    }

    pub(super) fn input_requirement_failure(
        input: InputSequence,
        failure: PresentationRequirementFailure,
    ) -> EngineError {
        match failure {
            PresentationRequirementFailure::RequirementRevisionExhausted => {
                EngineError::PresentationRequirementRevisionExhausted { input }
            }
            PresentationRequirementFailure::Invariant(detail) => {
                EngineError::PresentationRequirementInvariant {
                    input: Some(input),
                    detail,
                }
            }
            PresentationRequirementFailure::Scene(source) => {
                Self::scene_revision_error(input, source)
            }
        }
    }

    pub(super) fn capture_surface_semantic_snapshots(
        workspace: &Workspace,
        workspace_index: &crate::workspace::WorkspaceIndex,
    ) -> Result<BTreeMap<crate::ids::SurfaceId, SurfaceSemanticSnapshot>, crate::error::CommandError>
    {
        Self::capture_surface_semantic_snapshots_with(workspace, |root| {
            workspace_index
                .root_fingerprint(root)
                .cloned()
                .ok_or(crate::error::CommandError::MissingRoot { root })
        })
    }

    fn capture_surface_semantic_snapshots_with(
        workspace: &Workspace,
        mut root_fingerprint: impl FnMut(RootId) -> Result<NodeFingerprint, crate::error::CommandError>,
    ) -> Result<BTreeMap<crate::ids::SurfaceId, SurfaceSemanticSnapshot>, crate::error::CommandError>
    {
        let mut snapshots = BTreeMap::new();
        for (surface, presentation) in workspace.surfaces() {
            let main = presentation
                .main_root
                .map(&mut root_fingerprint)
                .transpose()?;
            let mut contained = Vec::with_capacity(presentation.contained.len());
            for floating in presentation.contained.iter().copied() {
                let record = workspace.contained_floating(floating).ok_or(
                    crate::error::CommandError::Invariant {
                        stage: "capture contained surface semantics",
                    },
                )?;
                let rect = record.rect;
                contained.push(ContainedSemanticSnapshot {
                    floating,
                    root: root_fingerprint(record.root)?,
                    rect_bits: [
                        rect.min().x().to_bits(),
                        rect.min().y().to_bits(),
                        rect.max().x().to_bits(),
                        rect.max().y().to_bits(),
                    ],
                });
            }
            snapshots.insert(surface, SurfaceSemanticSnapshot { main, contained });
        }
        Ok(snapshots)
    }

    pub(super) fn scene_revision_error(
        input: InputSequence,
        source: SceneBuildError,
    ) -> EngineError {
        match source {
            SceneBuildError::SurfaceSceneRevisionExhausted { surface } => {
                EngineError::SurfaceSceneRevisionExhausted { input, surface }
            }
            other => EngineError::PresentationRequirementInvariant {
                input: Some(input),
                detail: other.to_string(),
            },
        }
    }

    pub(super) fn invalidate_uncovered_changed_resize_presentation(
        &mut self,
        changed_surfaces: &BTreeSet<crate::ids::SurfaceId>,
        input: InputSequence,
        covered_surfaces: &BTreeSet<crate::ids::SurfaceId>,
    ) -> Result<BTreeSet<crate::ids::SurfaceId>, EngineError> {
        let surfaces = changed_surfaces
            .iter()
            .copied()
            .filter(|surface| !covered_surfaces.contains(surface))
            .collect::<BTreeSet<_>>();
        let mut invalidated = BTreeSet::new();
        for surface in &surfaces {
            if !matches!(
                self.presentation_authority.scene.surface(*surface),
                Some(SurfaceScene::Ready(_))
            ) {
                continue;
            }
            self.presentation_authority
                .scene
                .invalidate_presentation_input(*surface)
                .map_err(|source| Self::scene_revision_error(input, source))?;
            invalidated.insert(*surface);
        }
        Ok(invalidated)
    }

    pub(super) fn changed_resize_presentation_surfaces(
        &self,
        before: &InteractionState,
    ) -> BTreeSet<SurfaceId> {
        let before_resize = before
            .active_resize_view()
            .filter(|resize| !resize.updates().is_empty());
        let current_resize = self
            .interaction
            .active_resize_view()
            .filter(|resize| !resize.updates().is_empty());
        let unchanged = match (before_resize, current_resize) {
            (None, None) => true,
            (Some(before), Some(current)) => {
                before.surface() == current.surface() && before.updates() == current.updates()
            }
            _ => false,
        };
        if unchanged {
            return BTreeSet::new();
        }

        before_resize
            .iter()
            .map(|resize| resize.surface())
            .chain(current_resize.iter().map(|resize| resize.surface()))
            .collect()
    }

    pub(super) fn prepared_surface_matches_resize_projection(
        &self,
        before: &InteractionState,
        surface: SurfaceId,
        contribution: &PreparedSurfaceContribution,
    ) -> bool {
        match &contribution.state {
            PreparedSurfaceContributionState::Ready { plan } => {
                let before_updates = before
                    .active_resize_view()
                    .filter(|resize| resize.surface() == surface)
                    .map(|resize| resize.updates())
                    .unwrap_or_default();
                let current_updates = self
                    .interaction
                    .active_resize_view()
                    .filter(|resize| resize.surface() == surface)
                    .map(|resize| resize.updates())
                    .unwrap_or_default();
                let affected = before_updates
                    .iter()
                    .chain(current_updates)
                    .map(|update| (update.split().root(), update.split().node()))
                    .collect::<BTreeSet<_>>();
                affected.into_iter().all(|(root, split)| {
                    let expected = current_updates
                        .iter()
                        .find(|update| {
                            update.split().root() == root && update.split().node() == split
                        })
                        .map(SplitResize::weights)
                        .or_else(|| match self.workspace.node(split) {
                            Some(Node::Split { weights, .. }) => Some(weights.as_slice()),
                            Some(Node::Tabs { .. }) | None => None,
                        });
                    expected.is_some_and(|expected| {
                        plan.splitter_records().iter().any(|record| {
                            let id = record.id();
                            id.root == root && id.split == split && record.weights() == expected
                        })
                    })
                })
            }
            PreparedSurfaceContributionState::Unavailable(_) => true,
            PreparedSurfaceContributionState::Retained { .. } => false,
        }
    }

    pub(super) fn invalidate_transient(
        &mut self,
        input: InputSequence,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<(), EngineError> {
        let scroll_reason = match reason {
            InteractionCancelReason::PolicyChanged => ScrollTerminationReason::PolicyChanged,
            InteractionCancelReason::PointerProviderRetired => {
                ScrollTerminationReason::ProviderRetired
            }
            InteractionCancelReason::PointerStreamCancelled => {
                ScrollTerminationReason::StreamCancelled
            }
            InteractionCancelReason::PointerStreamEnded => ScrollTerminationReason::StreamEnded,
            InteractionCancelReason::SurfaceClosed
            | InteractionCancelReason::SourceVanished
            | InteractionCancelReason::WorkspaceRestored => ScrollTerminationReason::SurfaceRemoved,
            _ => ScrollTerminationReason::ReceiverLost,
        };
        self.terminate_all_scroll_sessions_input(input, scroll_reason, interaction_events);
        self.viewport
            .end_all_drag_routing()
            .map_err(|source| EngineError::Viewport { input, source })?;
        if let Some(status) = self.interaction.cancel_active() {
            interaction_events.push(InteractionEvent::new(
                input,
                self.version,
                InteractionEventKind::Cancelled { status, reason },
            ));
        }
        self.cancel_pending_release_obligations_input(input, reason, interaction_events);
        Ok(())
    }

    pub(super) fn cancel_active_interaction_preserving_scene(
        &mut self,
        input: InputSequence,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<(), EngineError> {
        self.viewport
            .end_all_drag_routing()
            .map_err(|source| EngineError::Viewport { input, source })?;
        if let Some(status) = self.interaction.cancel_active() {
            interaction_events.push(InteractionEvent::new(
                input,
                self.version,
                InteractionEventKind::Cancelled { status, reason },
            ));
        }
        self.cancel_pending_release_obligations_input(input, reason, interaction_events);
        Ok(())
    }

    pub(super) fn platform_interaction_dependencies(&self) -> PlatformInteractionDependencies {
        let mut dependencies = PlatformInteractionDependencies::default();
        match self.interaction.status() {
            InteractionStatus::Idle => {}
            InteractionStatus::Pressed { session } => {
                if let Ok(click) = self.interaction.active_click(session) {
                    self.insert_current_viewport_binding(
                        &mut dependencies.owner_bindings,
                        click.region.surface(),
                    );
                }
            }
            InteractionStatus::Armed { session } => {
                self.collect_armed_platform_dependencies(session, &mut dependencies);
            }
            InteractionStatus::Dragging { session } => {
                self.collect_dragging_platform_dependencies(session, &mut dependencies);
            }
            InteractionStatus::Resizing { session } => {
                if let Ok(resize) = self.interaction.active_resize(session) {
                    self.insert_current_viewport_binding(
                        &mut dependencies.owner_bindings,
                        resize.surface,
                    );
                }
            }
            InteractionStatus::ContainedTransforming { session } => {
                if let Ok(transform) = self.interaction.active_contained_transform(session) {
                    self.insert_current_viewport_binding(
                        &mut dependencies.owner_bindings,
                        transform.surface,
                    );
                }
            }
        }
        if let Some(pending) = self
            .pending_mutation
            .as_ref()
            .and_then(PendingMutation::drag_release)
        {
            self.collect_pending_drag_release_platform_dependencies(pending, &mut dependencies);
        }
        dependencies
    }

    fn collect_pending_drag_release_platform_dependencies(
        &self,
        pending: &PendingDragRelease,
        dependencies: &mut PlatformInteractionDependencies,
    ) {
        if let Some((binding, input)) = pending.physical_route {
            dependencies.owner_bindings.insert(binding);
            dependencies.route_input = Some(input);
        } else {
            self.insert_current_viewport_binding(
                &mut dependencies.owner_bindings,
                pending.drag.source_surface,
            );
        }
        if let FrozenDragOrigin::Contained(origin) = &pending.drag.origin {
            self.insert_current_viewport_binding(&mut dependencies.owner_bindings, origin.surface);
        }
        if let PreviewDecision::Publish { visual, .. } = &pending.release_decision {
            match visual {
                PreviewVisual::Dock { surface, .. } | PreviewVisual::Contained { surface, .. } => {
                    self.insert_current_viewport_binding(
                        &mut dependencies.target_bindings,
                        *surface,
                    );
                }
                PreviewVisual::Native { .. } => dependencies.native = true,
            }
        }
    }

    fn collect_armed_platform_dependencies(
        &self,
        session: crate::interaction::DragSessionId,
        dependencies: &mut PlatformInteractionDependencies,
    ) {
        let Ok(drag) = self.interaction.armed_drag(session) else {
            return;
        };
        if let Some(surface) = self.payload_surface(&drag.payload) {
            self.insert_current_viewport_binding(&mut dependencies.owner_bindings, surface);
        }
        if let FrozenDragOrigin::Contained(origin) = &drag.origin {
            self.insert_current_viewport_binding(&mut dependencies.owner_bindings, origin.surface);
        }
    }

    fn collect_dragging_platform_dependencies(
        &self,
        session: crate::interaction::DragSessionId,
        dependencies: &mut PlatformInteractionDependencies,
    ) {
        let Ok(drag) = self.interaction.active_drag(session) else {
            return;
        };
        if let Some(binding) = self.viewport.drag_source(drag.owner.pointer()) {
            dependencies.owner_bindings.insert(binding);
            dependencies.route_input = self.viewport.drag_source_input(drag.owner.pointer());
        } else if let Some(surface) = self.payload_surface(&drag.payload) {
            self.insert_current_viewport_binding(&mut dependencies.owner_bindings, surface);
        }
        if let FrozenDragOrigin::Contained(origin) = &drag.origin {
            self.insert_current_viewport_binding(&mut dependencies.owner_bindings, origin.surface);
        }
        if let Some(offer) = drag.contained_offer {
            self.insert_current_viewport_binding(
                &mut dependencies.target_bindings,
                offer.anchor().surface(),
            );
        }
        if let Some(preview) = drag.preview() {
            match preview.public().visual() {
                PreviewVisual::Dock { surface, .. } | PreviewVisual::Contained { surface, .. } => {
                    self.insert_current_viewport_binding(
                        &mut dependencies.target_bindings,
                        *surface,
                    );
                }
                PreviewVisual::Native { .. } => dependencies.native = true,
            }
        } else if drag.native_offer.is_some()
            && self
                .viewport
                .physical_native_drag_capability(self.native_drag_source_input(drag))
                .is_supported()
        {
            dependencies.native = true;
        }
    }

    fn insert_current_viewport_binding(
        &self,
        bindings: &mut BTreeSet<crate::viewport::ViewportBinding>,
        surface: crate::ids::SurfaceId,
    ) {
        if let Some(binding) = self
            .viewport
            .viewport(surface)
            .map(crate::viewport_registry::ViewportRecord::binding)
        {
            bindings.insert(binding);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn platform_interaction_reconciliation(
        &self,
        dependencies: &PlatformInteractionDependencies,
        version_before_actions: WorkspaceVersion,
        transition: &crate::frame::ViewportFrameTransition,
        previous_physical_native_drag: PlatformCapability,
        previous_physical_cross_surface_drag: PlatformCapability,
        invalidated_scene_bindings: &BTreeSet<crate::viewport::ViewportBinding>,
    ) -> PlatformInteractionReconciliation {
        let mut target_authority_lost = false;
        let mut end_routing = false;
        for event in transition.registry_events() {
            let (binding, destroyed) = match event {
                crate::viewport_registry::RegistryEvent::Destroyed { observation } => {
                    (observation.binding(), true)
                }
                crate::viewport_registry::RegistryEvent::FactsUnavailable { binding } => {
                    (*binding, false)
                }
                crate::viewport_registry::RegistryEvent::Ready { .. }
                | crate::viewport_registry::RegistryEvent::PresentationChanged { .. }
                | crate::viewport_registry::RegistryEvent::CloseRequested { .. }
                | crate::viewport_registry::RegistryEvent::CloseRequestCleared { .. }
                | crate::viewport_registry::RegistryEvent::BindingMissing { .. } => continue,
            };
            if dependencies.owner_bindings.contains(&binding) {
                if destroyed {
                    return PlatformInteractionReconciliation::Cancel(
                        InteractionCancelReason::SurfaceClosed,
                    );
                }
                target_authority_lost = true;
                end_routing = true;
            }
            if dependencies.target_bindings.contains(&binding) {
                target_authority_lost = true;
            }
        }
        let owner_scene_authority_lost = invalidated_scene_bindings
            .iter()
            .any(|binding| dependencies.owner_bindings.contains(binding));
        target_authority_lost |= owner_scene_authority_lost;
        target_authority_lost |= invalidated_scene_bindings
            .iter()
            .any(|binding| dependencies.target_bindings.contains(binding));
        let source_input = dependencies
            .route_input
            .unwrap_or(crate::platform::NativeDragSourceInput::Preserve);
        let current_physical_native_drag =
            self.viewport.physical_native_drag_capability(source_input);
        if dependencies.native
            && previous_physical_native_drag.is_supported()
            && !current_physical_native_drag.is_supported()
        {
            if let Some(reason) =
                Self::physical_drag_capability_loss_reason(current_physical_native_drag)
            {
                return PlatformInteractionReconciliation::Cancel(reason);
            }
        }
        if dependencies.native && transition.work_areas_changed() {
            target_authority_lost = true;
        }
        let current_physical_cross_surface_drag = self
            .viewport
            .physical_cross_surface_drag_capability(source_input);
        if dependencies.route_input.is_some()
            && previous_physical_cross_surface_drag.is_supported()
            && !current_physical_cross_surface_drag.is_supported()
        {
            if let Some(reason) =
                Self::physical_drag_capability_loss_reason(current_physical_cross_surface_drag)
            {
                return PlatformInteractionReconciliation::Cancel(reason);
            }
        }
        if target_authority_lost {
            PlatformInteractionReconciliation::ClearDragFeedback {
                workspace_changed: self.version != version_before_actions,
                end_routing,
            }
        } else if self.version != version_before_actions {
            PlatformInteractionReconciliation::Cancel(InteractionCancelReason::WorkspaceChanged)
        } else {
            PlatformInteractionReconciliation::Preserve
        }
    }

    fn physical_drag_capability_loss_reason(
        capability: PlatformCapability,
    ) -> Option<InteractionCancelReason> {
        match capability {
            PlatformCapability::Unknown(_) => {
                Some(InteractionCancelReason::NativeCapabilityUnknown)
            }
            PlatformCapability::Unsupported(_) => {
                Some(InteractionCancelReason::NativeCapabilityUnavailable)
            }
            PlatformCapability::Supported => None,
        }
    }

    pub(super) fn clear_platform_drag_feedback(
        &mut self,
        input: InputSequence,
        workspace_changed: bool,
        end_routing: bool,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<(), EngineError> {
        let InteractionStatus::Dragging { session } = self.interaction.status() else {
            if workspace_changed {
                self.invalidate_transient(
                    input,
                    InteractionCancelReason::WorkspaceChanged,
                    interaction_events,
                )?;
            }
            return Ok(());
        };
        if workspace_changed && !self.drag_source_is_current(session, input)? {
            let _ = self.cancel_drag(
                input,
                session,
                InteractionCancelReason::SourceVanished,
                interaction_events,
            )?;
            return Ok(());
        }
        let preview_was_present = self.interaction.preview().is_some();
        self.interaction
            .clear_drag_feedback(session)
            .map_err(|_| EngineError::Interaction {
                input,
                source: InteractionCounterError::StateInvariant,
            })?;
        if preview_was_present {
            interaction_events.push(InteractionEvent::new(
                input,
                self.version,
                InteractionEventKind::PreviewCleared { session },
            ));
        }
        if end_routing {
            self.viewport
                .end_all_drag_routing()
                .map_err(|source| EngineError::Viewport { input, source })?;
        }
        Ok(())
    }

    pub(super) fn reduce_surface_contribution(
        &mut self,
        tick: ReducerTickId,
        contribution: PreparedSurfaceContribution,
        policy_revision: PolicyRevision,
    ) -> Result<SurfaceContributionOutcome, EngineError> {
        let token = contribution.token;
        let surface = token.surface();
        let cause = ReductionCause::SurfaceContribution {
            tick,
            surface,
            base: token.base(),
        };
        let current = self
            .presentation_authority
            .scene
            .surface(surface)
            .map(SurfaceScene::stamp);
        if current != Some(token.base()) {
            return Ok(SurfaceContributionOutcome::Rejected {
                surface,
                reason: SurfaceContributionRejection::StaleBase {
                    submitted: token.base(),
                    current,
                },
            });
        }
        if contribution.policy_revision != policy_revision {
            return Ok(SurfaceContributionOutcome::Rejected {
                surface,
                reason: SurfaceContributionRejection::PolicyAuthorityChanged {
                    submitted: contribution.policy_revision,
                    current: policy_revision,
                },
            });
        }
        if !Self::coordinate_capture_matches_current(
            token.coordinates,
            self.viewport.viewport(surface),
            self.viewport.surface_coordinate_authority(surface),
        ) {
            return Ok(SurfaceContributionOutcome::Rejected {
                surface,
                reason: SurfaceContributionRejection::CoordinateAuthorityChanged { surface },
            });
        }

        match contribution.state {
            PreparedSurfaceContributionState::Ready { plan } => {
                let geometry = |projection: Option<crate::scene::ContainedLayoutProjection>| {
                    projection.map(|mut projection| {
                        projection.target_basis = None;
                        projection
                    })
                };
                if geometry(plan.contained_projection())
                    != geometry(self.contained_layout_projection(surface))
                {
                    return Ok(SurfaceContributionOutcome::Rejected {
                        surface,
                        reason: SurfaceContributionRejection::ContainedProjectionChanged {
                            surface,
                        },
                    });
                }
                let resolved_tab_offsets = plan
                    .tab_bar_records()
                    .iter()
                    .map(|record| {
                        #[cfg(test)]
                        crate::drop_resolver::structural_work::record_selection_lookup();
                        let id = *record.id();
                        let selected = plan.selected_in_tabs(id.tabs);
                        (
                            id,
                            record.scroll_offset(),
                            selected,
                            record.viewport().width(),
                        )
                    })
                    .collect::<Vec<_>>();
                let output_serial = self.presentation_authority.issue_scene_output()?;
                let (stamp, ticket) = self
                    .presentation_authority
                    .scene
                    .install_ready(
                        plan,
                        token.coordinates,
                        self.authority_domain,
                        output_serial,
                    )
                    .map_err(|source| Self::contribution_invariant(cause, source.to_string()))?;
                self.presentation_authority
                    .tab_strip_states
                    .adopt_resolved_scroll_offsets(surface, resolved_tab_offsets)
                    .map_err(|source| Self::contribution_invariant(cause, source.to_string()))?;
                self.presentation_authority.last_presentation_output_serial = output_serial;
                if self
                    .presentation_authority
                    .selection_rebind
                    .as_ref()
                    .is_some_and(|rebind| rebind.surface == surface)
                {
                    self.presentation_authority.selection_rebind = None;
                }
                Ok(SurfaceContributionOutcome::Ready {
                    surface,
                    stamp,
                    ticket,
                })
            }
            PreparedSurfaceContributionState::Retained { ticket } => {
                let retained = self
                    .presentation_authority
                    .scene
                    .surface(surface)
                    .and_then(SurfaceScene::ready)
                    .filter(|ready| {
                        ready.output_ticket() == ticket
                            && ready
                                .coordinate_capture()
                                .same_projection_authority(token.coordinates)
                    })
                    .map(|ready| (ready.stamp(), ready.output_ticket()));
                match retained {
                    Some((stamp, ticket)) => Ok(SurfaceContributionOutcome::Retained {
                        surface,
                        stamp,
                        ticket,
                    }),
                    None => Ok(SurfaceContributionOutcome::Rejected {
                        surface,
                        reason: SurfaceContributionRejection::StaleBase {
                            submitted: token.base(),
                            current: self
                                .presentation_authority
                                .scene
                                .surface(surface)
                                .map(SurfaceScene::stamp),
                        },
                    }),
                }
            }
            PreparedSurfaceContributionState::Unavailable(reason) => {
                self.publish_unavailable_contribution(cause, surface, reason)
            }
        }
    }

    fn publish_unavailable_contribution(
        &mut self,
        cause: ReductionCause,
        surface: crate::ids::SurfaceId,
        reason: SurfaceContributionUnavailableReason,
    ) -> Result<SurfaceContributionOutcome, EngineError> {
        let has_paint_fallback = matches!(
            self.presentation_authority.scene.surface(surface),
            Some(SurfaceScene::Stale(_))
        ) || matches!(
            self.presentation_authority.scene.surface(surface),
            Some(SurfaceScene::Ready(ready)) if ready.paint_fallback().is_some()
        );
        let stamp = match (reason, has_paint_fallback) {
            (SurfaceContributionUnavailableReason::EmptyBounds, _) => self
                .presentation_authority
                .scene
                .replace_with_bootstrap(surface, BootstrapSurfaceSceneReason::EmptyBounds),
            (SurfaceContributionUnavailableReason::MeasurementsUnavailable(authority), true) => {
                self.presentation_authority.scene.demote_to_stale(
                    surface,
                    StaleSurfaceSceneReason::MeasurementsUnavailable(authority),
                )
            }
            (SurfaceContributionUnavailableReason::MeasurementsUnavailable(authority), false) => {
                self.presentation_authority.scene.replace_with_bootstrap(
                    surface,
                    BootstrapSurfaceSceneReason::MeasurementsUnavailable(authority),
                )
            }
            (SurfaceContributionUnavailableReason::CoordinateAuthorityUnavailable, true) => {
                self.presentation_authority.scene.demote_to_stale(
                    surface,
                    StaleSurfaceSceneReason::CoordinateAuthorityUnavailable,
                )
            }
            (SurfaceContributionUnavailableReason::CoordinateAuthorityUnavailable, false) => {
                self.presentation_authority.scene.replace_with_bootstrap(
                    surface,
                    BootstrapSurfaceSceneReason::CoordinateAuthorityUnavailable,
                )
            }
            (
                SurfaceContributionUnavailableReason::PopupGeometryUnavailable { reason, .. },
                true,
            ) => self.presentation_authority.scene.demote_to_stale(
                surface,
                StaleSurfaceSceneReason::PopupGeometryUnavailable(reason),
            ),
            (
                SurfaceContributionUnavailableReason::PopupGeometryUnavailable { reason, .. },
                false,
            ) => self.presentation_authority.scene.replace_with_bootstrap(
                surface,
                BootstrapSurfaceSceneReason::PopupGeometryUnavailable(reason),
            ),
        }
        .map_err(|source| Self::contribution_invariant(cause, source.to_string()))?;
        Ok(SurfaceContributionOutcome::Unavailable {
            surface,
            stamp,
            reason,
        })
    }

    pub(super) fn settle_popup_geometry_unavailability_after_surface_contributions(
        &mut self,
        tick: ReducerTickId,
        outcomes: &[SurfaceContributionOutcome],
        changed_surfaces: &mut BTreeSet<SurfaceId>,
    ) -> Result<(), EngineError> {
        let Some(session) = self
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .map(|menu| menu.session())
        else {
            return Ok(());
        };
        let popup = self
            .presentation_authority
            .presentation_requirements
            .popup();
        if popup.session() != Some(session) || popup.owner() != Some(session.key()) {
            return Ok(());
        }
        let owner_unavailable = outcomes.iter().any(|outcome| {
            matches!(
                outcome,
                SurfaceContributionOutcome::Unavailable {
                    surface,
                    reason:
                        SurfaceContributionUnavailableReason::PopupGeometryUnavailable {
                            session: actual,
                            ..
                        },
                    ..
                } if *surface == session.key().surface() && *actual == session
            )
        });
        if !owner_unavailable {
            return Ok(());
        }

        let cause = ReductionCause::SurfaceContributionBatch { tick };
        let before_authorities = self.presentation_authority.scene.interaction_authorities();
        let mut tab_strip_states = self.presentation_authority.tab_strip_states.clone();
        let delta = tab_strip_states
            .close_tab_list_menu(session)
            .map_err(|source| Self::tab_strip_state_invariant(cause, source))?;
        self.presentation_authority.tab_strip_states = tab_strip_states;
        self.consume_tab_strip_state_delta(cause, &delta)?;
        let after_authorities = self.presentation_authority.scene.interaction_authorities();
        changed_surfaces.extend(
            before_authorities
                .keys()
                .chain(after_authorities.keys())
                .copied()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .filter(|surface| {
                    before_authorities.get(surface) != after_authorities.get(surface)
                }),
        );
        Ok(())
    }

    fn contribution_invariant(cause: ReductionCause, detail: String) -> EngineError {
        match cause {
            ReductionCause::SurfaceContribution { surface, base, .. } => {
                EngineError::SurfaceContributionInvariant {
                    surface,
                    base,
                    detail,
                }
            }
            ReductionCause::Input { .. } => EngineError::ReductionCauseInvariant {
                detail: "surface contribution helper received an input cause",
            },
            ReductionCause::PointerEdge { .. } => EngineError::ReductionCauseInvariant {
                detail: "surface contribution helper received a pointer-edge cause",
            },
            ReductionCause::PointerProviderRetirement { .. } => {
                EngineError::ReductionCauseInvariant {
                    detail: "surface contribution helper received a pointer-provider retirement cause",
                }
            }
            ReductionCause::PlatformProviderReplacement { .. } => {
                EngineError::ReductionCauseInvariant {
                    detail: "surface contribution helper received a platform-provider replacement cause",
                }
            }
            ReductionCause::SurfacePresentationObservationBatch { tick } => {
                EngineError::SurfacePresentationObservationBatchInvariant { tick, detail }
            }
            ReductionCause::PresentationObservation { .. } => {
                EngineError::ReductionCauseInvariant {
                    detail: "surface contribution helper received a presentation-observation cause",
                }
            }
            ReductionCause::SurfaceContributionBatch { tick } => {
                EngineError::SurfaceContributionBatchInvariant { tick, detail }
            }
            ReductionCause::PresentationHostRetirement { tick, .. } => {
                EngineError::PresentationHostRetirementInvariant { tick, detail }
            }
        }
    }

    #[cfg(test)]
    pub(super) fn reconcile_interaction_after_host_retirement(
        &mut self,
        tick: ReducerTickId,
        host: PresentationHostLease,
        affected_surfaces: &BTreeSet<SurfaceId>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<(), EngineError> {
        let cause = ReductionCause::PresentationHostRetirement { tick, host };
        self.reconcile_scroll_lifecycle(cause, interaction_events);
        if self
            .cancel_revoked_presentation_caused(cause, interaction_events)?
            .is_some()
        {
            return Ok(());
        }
        match self.presentation_host_retirement_interaction_impact(affected_surfaces) {
            PresentationHostRetirementInteractionImpact::None => {}
            PresentationHostRetirementInteractionImpact::ClearFeedback { end_routing } => {
                let InteractionStatus::Dragging { session } = self.interaction.status() else {
                    return Err(Self::contribution_invariant(
                        cause,
                        "target-only retirement impact requires an active drag".to_owned(),
                    ));
                };
                let preview_was_present = self.interaction.preview().is_some();
                self.interaction
                    .clear_drag_feedback(session)
                    .map_err(|source| Self::contribution_invariant(cause, format!("{source:?}")))?;
                if preview_was_present {
                    interaction_events.push(InteractionEvent::new_caused(
                        cause,
                        self.version,
                        InteractionEventKind::PreviewCleared { session },
                    ));
                }
                if end_routing {
                    self.viewport.end_all_drag_routing().map_err(|source| {
                        EngineError::Viewport {
                            input: self.last_input,
                            source,
                        }
                    })?;
                }
            }
            PresentationHostRetirementInteractionImpact::CancelOwner => {
                self.viewport
                    .end_all_drag_routing()
                    .map_err(|source| EngineError::Viewport {
                        input: self.last_input,
                        source,
                    })?;
                if let Some(status) = self.interaction.cancel_active() {
                    interaction_events.push(InteractionEvent::new_caused(
                        cause,
                        self.version,
                        InteractionEventKind::Cancelled {
                            status,
                            reason: InteractionCancelReason::SceneUnavailable,
                        },
                    ));
                }
            }
        }
        Ok(())
    }

    #[cfg(test)]
    fn presentation_host_retirement_interaction_impact(
        &self,
        surfaces: &BTreeSet<SurfaceId>,
    ) -> PresentationHostRetirementInteractionImpact {
        if surfaces.is_empty() {
            return PresentationHostRetirementInteractionImpact::None;
        }
        match self.interaction.status() {
            InteractionStatus::Idle => PresentationHostRetirementInteractionImpact::None,
            InteractionStatus::Pressed { session } => {
                match self.interaction.active_click(session) {
                    Ok(click) if surfaces.contains(&click.region.surface()) => {
                        PresentationHostRetirementInteractionImpact::CancelOwner
                    }
                    Ok(_) => PresentationHostRetirementInteractionImpact::None,
                    Err(_) => PresentationHostRetirementInteractionImpact::CancelOwner,
                }
            }
            InteractionStatus::Armed { session } => match self.interaction.armed_drag(session) {
                Ok(drag) if surfaces.contains(&drag.source_surface) => {
                    PresentationHostRetirementInteractionImpact::CancelOwner
                }
                Ok(_) => PresentationHostRetirementInteractionImpact::None,
                Err(_) => PresentationHostRetirementInteractionImpact::CancelOwner,
            },
            InteractionStatus::Dragging { session } => {
                match self.interaction.active_drag(session) {
                    Ok(drag)
                        if surfaces.contains(&drag.source_surface)
                            || matches!(
                                &drag.origin,
                                FrozenDragOrigin::Contained(origin)
                                    if surfaces.contains(&origin.surface)
                            ) =>
                    {
                        PresentationHostRetirementInteractionImpact::CancelOwner
                    }
                    Ok(drag) => {
                        let end_routing = false;
                        let target_resource_affected = drag
                            .contained_offer
                            .is_some_and(|offer| surfaces.contains(&offer.anchor().surface()))
                            || drag
                                .drop_affordance()
                                .is_some_and(|affordance| surfaces.contains(&affordance.surface()))
                            || drag.preview().is_some_and(|preview| {
                                surfaces.contains(&preview.public().visual().surface())
                            });
                        if target_resource_affected {
                            PresentationHostRetirementInteractionImpact::ClearFeedback {
                                end_routing,
                            }
                        } else {
                            PresentationHostRetirementInteractionImpact::None
                        }
                    }
                    Err(_) => PresentationHostRetirementInteractionImpact::CancelOwner,
                }
            }
            InteractionStatus::Resizing { session } => {
                match self.interaction.active_resize(session) {
                    Ok(resize) if surfaces.contains(&resize.surface) => {
                        PresentationHostRetirementInteractionImpact::CancelOwner
                    }
                    Ok(_) => PresentationHostRetirementInteractionImpact::None,
                    Err(_) => PresentationHostRetirementInteractionImpact::CancelOwner,
                }
            }
            InteractionStatus::ContainedTransforming { session } => {
                match self.interaction.active_contained_transform(session) {
                    Ok(transform) if surfaces.contains(&transform.surface) => {
                        PresentationHostRetirementInteractionImpact::CancelOwner
                    }
                    Ok(_) => PresentationHostRetirementInteractionImpact::None,
                    Err(_) => PresentationHostRetirementInteractionImpact::CancelOwner,
                }
            }
        }
    }

    pub(super) fn reconcile_interaction_after_presentation_observations(
        &mut self,
        tick: ReducerTickId,
        changed_surfaces: &BTreeSet<crate::ids::SurfaceId>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<(), EngineError> {
        self.reconcile_interaction_after_scene_authority_changes(
            ReductionCause::SurfacePresentationObservationBatch { tick },
            changed_surfaces,
            interaction_events,
        )
    }

    pub(super) fn reconcile_interaction_after_ordered_presentation_observation(
        &mut self,
        cause: ReductionCause,
        changed_surfaces: &BTreeSet<crate::ids::SurfaceId>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<(), EngineError> {
        debug_assert!(matches!(
            cause,
            ReductionCause::PresentationObservation { .. }
        ));
        self.reconcile_interaction_after_scene_authority_changes(
            cause,
            changed_surfaces,
            interaction_events,
        )
    }

    pub(super) fn reconcile_interaction_after_surface_contributions(
        &mut self,
        tick: ReducerTickId,
        changed_surfaces: &BTreeSet<crate::ids::SurfaceId>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<(), EngineError> {
        self.reconcile_interaction_after_scene_authority_changes(
            ReductionCause::SurfaceContributionBatch { tick },
            changed_surfaces,
            interaction_events,
        )
    }

    pub(super) fn reduce_presentation_observation(
        &mut self,
        cause: ReductionCause,
        presentation_host: PresentationHostLease,
        presentation_scope: &BTreeSet<HostPresentationStreamId>,
        observation: HostPresentationObservation,
    ) -> Result<
        (
            Vec<HostPresentationObservationOutcome>,
            BTreeSet<crate::ids::SurfaceId>,
            Vec<crate::event::WorkspaceEvent>,
        ),
        EngineError,
    > {
        let reduction = self
            .presentation_authority
            .presentation
            .reduce_observation(presentation_host, presentation_scope, observation)
            .map_err(presentation_ledger_error)?;
        self.apply_presentation_observation_reduction(cause, reduction)
    }

    pub(super) fn reduce_presentation_observation_entry(
        &mut self,
        cause: ReductionCause,
        presentation_host: PresentationHostLease,
        entry: HostPresentationObservationEntry,
    ) -> Result<
        (
            Vec<HostPresentationObservationOutcome>,
            BTreeSet<crate::ids::SurfaceId>,
            Vec<crate::event::WorkspaceEvent>,
        ),
        EngineError,
    > {
        let reduction = self
            .presentation_authority
            .presentation
            .reduce_observation_entry(presentation_host, entry)
            .map_err(presentation_ledger_error)?;
        self.apply_presentation_observation_reduction(cause, reduction)
    }

    fn apply_presentation_observation_reduction(
        &mut self,
        cause: ReductionCause,
        reduction: PresentationObservationReduction,
    ) -> Result<
        (
            Vec<HostPresentationObservationOutcome>,
            BTreeSet<crate::ids::SurfaceId>,
            Vec<crate::event::WorkspaceEvent>,
        ),
        EngineError,
    > {
        let outcomes = reduction.outcomes().to_vec();
        let mut changed_surfaces = BTreeSet::new();
        let mut lifecycle_events = Vec::new();
        let invalidated_streams = outcomes
            .iter()
            .filter_map(|outcome| match outcome {
                HostPresentationObservationOutcome::CapturedUnknown { stream, .. }
                | HostPresentationObservationOutcome::Retired {
                    stream,
                    promotion_eligible: false,
                    presented: Authority::Known(Some(_)) | Authority::Unknown(_),
                    ..
                } => Some(*stream),
                HostPresentationObservationOutcome::NoUpdate { .. }
                | HostPresentationObservationOutcome::Retired {
                    promotion_eligible: true,
                    ..
                }
                | HostPresentationObservationOutcome::Retired {
                    presented: Authority::Known(None),
                    ..
                }
                | HostPresentationObservationOutcome::Rejected { .. } => None,
            })
            .collect::<BTreeSet<_>>();
        let withdrawn_first_live_streams = outcomes
            .iter()
            .filter_map(|outcome| match outcome {
                HostPresentationObservationOutcome::CapturedUnknown { stream, .. }
                | HostPresentationObservationOutcome::Retired {
                    stream,
                    promotion_eligible: false,
                    ..
                }
                | HostPresentationObservationOutcome::Retired {
                    stream,
                    presented: Authority::Known(None),
                    ..
                } => Some(*stream),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        self.native_admission
            .revoke_observed_streams(&withdrawn_first_live_streams);
        changed_surfaces.extend(
            self.invalidate_interaction_authority_for_current_streams(&invalidated_streams),
        );
        for promotion in reduction.promotions() {
            let HostPresentationOutputPayload::Paint {
                scene: ticket,
                interaction,
                coordinate_generation,
            } = promotion.payload()
            else {
                continue;
            };
            let promotion_stream = BTreeSet::from([promotion.stream()]);
            let exact_first_live_projection = self
                .scene()
                .native_first_live_projection(ticket.surface())
                .is_some_and(|stamp| {
                    promotion.endpoint() == HostPresentationEndpoint::Native(stamp.binding)
                        && self.native_first_live_projection_is_current(stamp)
                        && self
                            .scene()
                            .ready_candidate(ticket.surface())
                            .is_some_and(|candidate| candidate.output_ticket() == ticket)
                });
            if !exact_first_live_projection
                && promotion.scene().is_some_and(|output| {
                    !self.presented_layering_is_current(output.plan())
                        || !self.presented_transient_geometry_is_current(output.plan())
                })
            {
                changed_surfaces.extend(
                    self.invalidate_interaction_authority_for_current_streams(&promotion_stream),
                );
                continue;
            }
            let capture = promotion
                .scene()
                .map(|scene| scene.coordinate_capture())
                .or_else(|| {
                    self.presentation_authority
                        .scene
                        .retained_output_capture(ticket)
                        .ok()
                });
            let Some(capture) = capture else {
                changed_surfaces.extend(
                    self.invalidate_interaction_authority_for_current_streams(&promotion_stream),
                );
                continue;
            };
            if presentation_endpoint_from_capture(capture) != promotion.endpoint()
                || !Self::coordinate_capture_matches_current(
                    capture,
                    self.viewport.viewport(ticket.surface()),
                    self.viewport.surface_coordinate_authority(ticket.surface()),
                )
            {
                changed_surfaces.extend(
                    self.invalidate_interaction_authority_for_current_streams(&promotion_stream),
                );
                continue;
            }
            if capture.authority_generation() != coordinate_generation {
                changed_surfaces.extend(
                    self.invalidate_interaction_authority_for_current_streams(&promotion_stream),
                );
                continue;
            }
            if let Some(output) = promotion.scene()
                && self
                    .presentation_authority
                    .scene
                    .retain_emitted_scene(output)
                    .is_err()
            {
                changed_surfaces.extend(
                    self.invalidate_interaction_authority_for_current_streams(&promotion_stream),
                );
                continue;
            }
            let authority = PresentedSurfaceAuthority::mint_observed(
                ticket,
                promotion.stream(),
                promotion.key(),
                promotion.endpoint(),
                coordinate_generation,
            );
            let awaiting_window_first_live = if let HostPresentationEndpoint::Native(binding) =
                promotion.endpoint()
            {
                self.viewport
                    .viewport(binding.surface())
                    .is_some_and(|record| {
                        record.admission() == crate::viewport_registry::ViewportAdmission::Pending
                    })
            } else {
                false
            };
            if awaiting_window_first_live
                && !self
                    .native_first_live_output_requires_follow_up(ticket, promotion.endpoint())?
            {
                continue;
            }
            let mut authorities = if awaiting_window_first_live {
                if self.scene().validate_observed_authority(authority).is_err() {
                    continue;
                }
                let Some(authorities) = self.observe_native_first_live_window(authority) else {
                    continue;
                };
                authorities
            } else {
                vec![authority]
            };
            // A prospective topology owner goes first. Its popup/source checks
            // must finish before any sibling authority is accepted.
            authorities.sort_by_key(|authority| {
                self.scene()
                    .native_first_live_projection(authority.surface())
                    .is_none()
            });
            let mut accepted_member = false;
            for authority in authorities {
                let member_ticket = authority.ticket();
                if let Some(stamp) = self
                    .scene()
                    .native_first_live_projection(member_ticket.surface())
                {
                    if self.scene().validate_observed_authority(authority).is_err()
                        || !self.commit_native_first_live_projection(
                            stamp,
                            authority,
                            cause,
                            &mut lifecycle_events,
                            &mut changed_surfaces,
                        )?
                    {
                        if accepted_member {
                            return Err(EngineError::ReductionCauseInvariant {
                                detail: "first-live window roster changed after committing a member",
                            });
                        }
                        break;
                    }
                    accepted_member = true;
                } else {
                    let member_capture = if member_ticket == ticket {
                        capture
                    } else {
                        self.scene().retained_output_capture(member_ticket).map_err(|_| {
                            EngineError::ReductionCauseInvariant {
                                detail: "validated first-live roster lost its retained coordinate capture",
                            }
                        })?
                    };
                    match self
                        .presentation_authority
                        .scene
                        .accept_observed_authority(authority)
                    {
                        Ok(changed) => {
                            self.admit_first_live_native_output(
                                member_ticket,
                                member_capture,
                                &mut lifecycle_events,
                            )?;
                            changed_surfaces.extend(changed);
                            accepted_member = true;
                        }
                        Err(_) => {
                            if awaiting_window_first_live {
                                return Err(EngineError::ReductionCauseInvariant {
                                    detail: "a validated first-live window roster lost a member before publication",
                                });
                            }
                            changed_surfaces.extend(
                                self.invalidate_interaction_authority_for_current_streams(
                                    &promotion_stream,
                                ),
                            );
                            break;
                        }
                    }
                }
            }
            // A matching output may precede the release edge in the same
            // ordered batch. Its core-minted preview token preserves that causality.
            if let Some(pending) = self.pending_mutation.as_mut().filter(|pending| {
                pending.surface() == ticket.surface() && pending.matches_interaction(interaction)
            }) {
                pending.presentation_mut().presented = Some(promotion.key());
            }
            if let Some(token) = interaction.drag_preview() {
                self.interaction
                    .observe_presented_preview(ticket.surface(), token);
            }
            if let Some(token) = interaction.contained_transform_preview() {
                self.interaction
                    .observe_presented_contained_transform_preview(ticket.surface(), token);
            }
        }
        self.observe_pending_mutation_retirements(&outcomes);
        Ok((outcomes, changed_surfaces, lifecycle_events))
    }

    fn observe_pending_mutation_retirements(
        &mut self,
        outcomes: &[HostPresentationObservationOutcome],
    ) {
        if let Some(pending) = self.pending_mutation.as_mut() {
            pending.presentation_mut().retire_prefixes(outcomes);
        }
    }

    #[cfg(test)]
    pub(super) fn observe_pending_mutation_host_retirement(
        &mut self,
        retired_outputs: &BTreeSet<HostFrameKey>,
    ) {
        if let Some(pending) = self.pending_mutation.as_mut() {
            pending.presentation_mut().retire_keys(retired_outputs);
        }
    }

    fn invalidate_interaction_authority_for_current_streams(
        &mut self,
        streams: &BTreeSet<HostPresentationStreamId>,
    ) -> BTreeSet<SurfaceId> {
        let current_surfaces = streams
            .iter()
            .filter_map(|stream| {
                self.presentation_authority
                    .presentation
                    .active_surface_for_stream(*stream)
            })
            .collect::<BTreeSet<_>>();
        self.presentation_authority
            .scene
            .invalidate_interaction_authority_for_current_streams(streams, &current_surfaces)
    }

    fn reconcile_interaction_after_scene_authority_changes(
        &mut self,
        cause: ReductionCause,
        changed_surfaces: &BTreeSet<crate::ids::SurfaceId>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<(), EngineError> {
        self.reconcile_scroll_lifecycle(cause, interaction_events);
        // A continuation preserves source coordinates, never a revoked output.
        // In particular a dropped/captured-unknown result must not be hidden by
        // a live layout compiled for the same gesture.
        if matches!(
            cause,
            ReductionCause::SurfacePresentationObservationBatch { .. }
                | ReductionCause::PresentationObservation { .. }
        ) && let Some(authority) = self.interaction.active_presentation_authority()
            && changed_surfaces.contains(&authority.surface())
            && self.interaction_projection(authority.surface()).is_none()
            && !self
                .interaction
                .active_scene_gesture_continuation()
                .is_some_and(|continuation| {
                    self.scene_gesture_continuation_is_current(continuation)
                })
            && !self.resize_continuation_is_current(authority)
            && let Some(owner) = self.interaction.active_owner()
        {
            self.cancel_gesture_owner(
                cause,
                owner,
                InteractionCancelReason::SceneUnavailable,
                interaction_events,
            )?;
            return Ok(());
        }
        if self
            .cancel_revoked_presentation_caused(cause, interaction_events)?
            .is_some()
        {
            return Ok(());
        }
        if changed_surfaces.is_empty() {
            return Ok(());
        }
        let pending_projection_lost = self
            .pending_mutation
            .as_ref()
            .and_then(PendingMutation::drag_release)
            .is_some_and(|pending| {
                changed_surfaces.contains(&pending.preview.scene().surface())
                    && !self.drag_preview_authority_is_current(&pending.drag)
            });
        if pending_projection_lost {
            self.cancel_pending_release_obligations_caused(
                cause,
                InteractionCancelReason::SceneUnavailable,
                interaction_events,
            );
        }
        match self.interaction.status() {
            InteractionStatus::Pressed { session } => {
                let click = self
                    .interaction
                    .active_click(session)
                    .map_err(|source| Self::contribution_invariant(cause, format!("{source:?}")))?
                    .clone();
                if changed_surfaces.contains(&click.region.surface()) {
                    let current = self
                        .presentation_authority
                        .scene
                        .ready_surface(click.region.surface());
                    let lineage_matches = current.is_some_and(|current| {
                        current.stamp().requirement() == click.measurement
                            && current
                                .coordinate_capture()
                                .same_projection_authority(click.coordinates)
                    });
                    if !lineage_matches {
                        let status = self.interaction.cancel_click(session).map_err(|source| {
                            Self::contribution_invariant(cause, format!("{source:?}"))
                        })?;
                        interaction_events.push(InteractionEvent::new_caused(
                            cause,
                            self.version,
                            InteractionEventKind::Cancelled {
                                status,
                                reason: InteractionCancelReason::SceneUnavailable,
                            },
                        ));
                    }
                }
            }
            InteractionStatus::Dragging { session } => {
                let drag = self
                    .interaction
                    .active_drag(session)
                    .map_err(|source| Self::contribution_invariant(cause, format!("{source:?}")))?
                    .clone();
                if matches!(cause, ReductionCause::SurfaceContributionBatch { .. })
                    && drag.preview().is_some_and(|preview| {
                        !matches!(preview.proof(), PreviewProof::Native { .. })
                            && self.drag_preview_authority_is_current(&drag)
                    })
                {
                    // Compiling a successor does not revoke a proposal resolved
                    // against the exact output which is still actually visible.
                    return Ok(());
                }
                // Promoting the exact preview is an authority change, not a
                // loss of its target. Retirements still remove ready_surface.
                if matches!(
                    cause,
                    ReductionCause::SurfacePresentationObservationBatch { .. }
                        | ReductionCause::PresentationObservation { .. }
                ) && drag.preview().is_some_and(|preview| {
                    preview.painted()
                        && self
                            .presentation_authority
                            .scene
                            .ready_surface(preview.public().token().scene().surface())
                            .is_some_and(|_| self.drag_preview_authority_is_current(&drag))
                }) {
                    return Ok(());
                }
                let affected = drag.preview().is_some_and(|preview| {
                    changed_surfaces.contains(&preview.public().token().scene().surface())
                });
                let position_affected = drag
                    .last_advanced_position()
                    .and_then(ManipulationPositionKey::surface)
                    .is_some_and(|surface| changed_surfaces.contains(&surface));
                let source_projection_owned =
                    self.current_contained_projection_is_owned(drag.source_surface);
                let source_projection_only = source_projection_owned
                    && (drag.preview().is_none() || self.drag_preview_authority_is_current(&drag));
                if (affected || position_affected) && !source_projection_only {
                    let changed = if source_projection_owned {
                        // A new destination layout can retire the old target
                        // without revoking the source's validated movement.
                        self.interaction.clear_drag_target_feedback(session)
                    } else {
                        self.interaction.clear_drag_feedback(session)
                    }
                    .map_err(|source| Self::contribution_invariant(cause, format!("{source:?}")))?;
                    if changed {
                        interaction_events.push(InteractionEvent::new_caused(
                            cause,
                            self.version,
                            InteractionEventKind::PreviewCleared { session },
                        ));
                    }
                }
            }
            InteractionStatus::ContainedTransforming { session } => {
                let transform = self
                    .interaction
                    .active_contained_transform(session)
                    .map_err(|source| Self::contribution_invariant(cause, format!("{source:?}")))?;
                let affected = changed_surfaces.contains(&transform.surface)
                    && (transform.preview.is_some()
                        || self.scene().ready_candidate(transform.surface).is_some());
                let compatible = self.contained_transform_layout_is_compatible(transform)
                    && (transform.preview.is_none()
                        || self.current_contained_projection_is_owned(transform.surface));
                if affected && !compatible {
                    let status = self
                        .interaction
                        .cancel_contained_transform(session)
                        .map_err(|source| {
                            Self::contribution_invariant(cause, format!("{source:?}"))
                        })?;
                    interaction_events.push(InteractionEvent::new_caused(
                        cause,
                        self.version,
                        InteractionEventKind::Cancelled {
                            status,
                            reason: InteractionCancelReason::SceneUnavailable,
                        },
                    ));
                }
            }
            InteractionStatus::Idle
            | InteractionStatus::Armed { .. }
            | InteractionStatus::Resizing { .. } => {}
        }
        Ok(())
    }

    pub(super) fn surface_scene_deltas(
        before: &SurfaceSceneSet,
        after: &SurfaceSceneSet,
    ) -> Vec<SurfaceSceneDelta> {
        let surfaces = before
            .surfaces()
            .map(|(surface, _)| *surface)
            .chain(after.surfaces().map(|(surface, _)| *surface))
            .collect::<BTreeSet<_>>();
        surfaces
            .into_iter()
            .filter_map(|surface| {
                let before_authority = before.interaction_authority(surface);
                let after_authority = after.interaction_authority(surface);
                let before = before
                    .surface(surface)
                    .map(|scene| Self::surface_scene_authority(scene, before_authority));
                let after = after
                    .surface(surface)
                    .map(|scene| Self::surface_scene_authority(scene, after_authority));
                (before != after).then(|| SurfaceSceneDelta::new(surface, before, after))
            })
            .collect()
    }

    fn surface_scene_authority(
        scene: &SurfaceScene,
        interaction: Option<PresentedSurfaceAuthority>,
    ) -> (
        SurfaceSceneStamp,
        SurfaceSceneStateKind,
        Option<PresentedSurfaceAuthority>,
    ) {
        let kind = match scene {
            SurfaceScene::Ready(_) => SurfaceSceneStateKind::Ready,
            SurfaceScene::Stale(_) => SurfaceSceneStateKind::Stale,
            SurfaceScene::Bootstrap(_) => SurfaceSceneStateKind::Bootstrap,
        };
        (scene.stamp(), kind, interaction)
    }
}

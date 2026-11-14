//! Core-owned host-frame capabilities and their atomic phase transitions.

use super::*;

#[cfg(test)]
mod error_tests;

/// Derived encoding state retained while one host run tries its actual passes.
/// The semantic workspace and input journals stay in their single candidate.
#[derive(Debug)]
pub(crate) struct CoreEncodingCheckpoint {
    presentation: PresentationAuthorityState,
    interaction: InteractionState,
    roster: HostPresentationRoster,
    snapshot_changed: bool,
    projection_changed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PointerJournalSubmissionAuthority {
    #[cfg(test)]
    RawLease,
    SurfaceLocalProducer,
    BackendIngress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PointerPresentationFence {
    workspace: WorkspaceVersion,
    requirements: RequirementRevision,
    drag_preview_surface: Option<SurfaceId>,
    contained_preview_surface: Option<SurfaceId>,
}

impl PointerPresentationFence {
    fn capture(engine: &DockEngine) -> Self {
        Self {
            workspace: engine.version,
            requirements: engine
                .presentation_authority
                .presentation_requirements
                .revision(),
            drag_preview_surface: engine
                .presentation_preview()
                .map(|preview| preview.visual().surface()),
            contained_preview_surface: engine
                .presentation_contained_transform_preview()
                .map(|preview| preview.surface()),
        }
    }
}

const fn pointer_move_outcome_allows_local_presentation_refresh(
    outcome: &InteractionOutcome,
) -> bool {
    match outcome {
        InteractionOutcome::DragBegan { .. }
        | InteractionOutcome::PreviewUpdated { .. }
        | InteractionOutcome::Cancelled { .. }
        | InteractionOutcome::ResizeUpdated { .. }
        | InteractionOutcome::ContainedTransformPreviewUpdated { .. }
        | InteractionOutcome::Rejected(_) => true,
        InteractionOutcome::TouchArmed { .. }
        | InteractionOutcome::TouchActivated { .. }
        | InteractionOutcome::TouchYieldedToContent { .. }
        | InteractionOutcome::ContextMenuRequested { .. }
        | InteractionOutcome::Scroll(_)
        | InteractionOutcome::CloseRequested { .. }
        | InteractionOutcome::ProductActionApplied(_)
        | InteractionOutcome::ProductActionRejected(_)
        | InteractionOutcome::TabStripControlActivated { .. }
        | InteractionOutcome::TabListMenuItemSelected { .. }
        | InteractionOutcome::TabListMenuDismissed { .. }
        | InteractionOutcome::TabListMenuFrameConsumed { .. }
        | InteractionOutcome::TabStripScrolled { .. }
        | InteractionOutcome::TabListMenuScrolled { .. }
        | InteractionOutcome::TabListMenuFocusMoved { .. }
        | InteractionOutcome::DragArmed { .. }
        | InteractionOutcome::DragDelivered { .. }
        | InteractionOutcome::ReleasePending { .. }
        | InteractionOutcome::ResizeBegan { .. }
        | InteractionOutcome::SplitterAdjusted { .. }
        | InteractionOutcome::ResizeDelivered { .. }
        | InteractionOutcome::ContainedPlacementApplied { .. }
        | InteractionOutcome::ContainedTransformBegan { .. }
        | InteractionOutcome::ContainedTransformDelivered { .. } => false,
    }
}

fn record_pointer_scroll_outcome_for_local_presentation_refresh(
    outcome: &InteractionOutcome,
    dirty_surfaces: &mut Vec<SurfaceId>,
) -> bool {
    match outcome {
        InteractionOutcome::Scroll(ScrollReductionOutcome::Applied(application)) => {
            if application.applied_delta() != 0.0 {
                dirty_surfaces.push(application.receiver().surface());
            }
            true
        }
        InteractionOutcome::Scroll(
            ScrollReductionOutcome::Began { .. }
            | ScrollReductionOutcome::Suppressed { .. }
            | ScrollReductionOutcome::Terminated { .. },
        ) => true,
        _ => false,
    }
}

impl CoreHostFramePrelude {
    pub(super) fn new(
        engine: &DockEngine,
        presentation_host: PresentationHostLease,
        presentation_observers: BTreeSet<PresentationHostLease>,
    ) -> Result<Self, EngineError> {
        engine
            .presentation_authority
            .presentation
            .validate_lease(presentation_host)
            .map_err(presentation_ledger_error)?;
        let mut presentation_scopes = BTreeMap::new();
        for observer in presentation_observers {
            let scope = engine
                .presentation_authority
                .presentation
                .pending_stream_scope(observer)
                .map_err(presentation_ledger_error)?;
            presentation_scopes.insert(observer, scope);
        }
        Ok(Self {
            authority_domain: engine.authority_domain,
            presentation_host,
            predecessor_tick: engine.last_reducer_tick,
            presentation_host_frontier: engine.presentation_authority.presentation.host_frontier(),
            platform_provider_frontier: engine.viewport.platform_provider_frontier(),
            presentation_scopes,
            presentation_observations: BTreeMap::new(),
            item_identity_scope: None,
            native_binding_allocation_budget: None,
            poison: None,
        })
    }

    /// Limits new native lifetimes throughout this unpublished candidate,
    /// including observation-driven recovery and accepted pointer releases.
    pub(crate) fn set_native_binding_allocation_budget(&mut self, remaining: Option<usize>) {
        self.native_binding_allocation_budget = remaining;
    }

    /// Freezes the application identities admitted by a durable document session.
    #[cfg(feature = "serde")]
    pub(crate) fn restrict_item_identity_scope(&mut self, items: Arc<BTreeSet<ItemId>>) {
        self.item_identity_scope = Some(items);
    }

    /// Returns the exact pending presentation streams frozen for the rendering host.
    #[must_use]
    pub fn pending_presentation_streams(
        &self,
    ) -> impl ExactSizeIterator<Item = HostPresentationStreamId> + '_ {
        self.presentation_scopes[&self.presentation_host]
            .iter()
            .copied()
    }

    /// Returns the exact pending presentation streams for one enrolled observer.
    #[cfg(test)]
    pub fn pending_presentation_streams_for(
        &self,
        presentation_host: PresentationHostLease,
    ) -> Result<Vec<HostPresentationStreamId>, CoreHostFrameError> {
        self.presentation_scopes
            .get(&presentation_host)
            .map(|scope| scope.iter().copied().collect())
            .ok_or(
                CoreHostFrameError::PresentationObservationHostOutsideScope {
                    host: presentation_host,
                },
            )
    }

    /// Submits the rendering host's exact presentation observation batch.
    pub fn submit_presentation_observation(
        &mut self,
        observation: HostPresentationObservation,
    ) -> Result<(), CoreHostFrameError> {
        self.submit_presentation_observation_for(self.presentation_host, observation)
    }

    /// Submits one exact observation batch for an explicitly enrolled host.
    pub fn submit_presentation_observation_for(
        &mut self,
        presentation_host: PresentationHostLease,
        observation: HostPresentationObservation,
    ) -> Result<(), CoreHostFrameError> {
        if let Some(error) = &self.poison {
            return Err(error.clone());
        }
        if self
            .presentation_observations
            .contains_key(&presentation_host)
        {
            let error = if presentation_host == self.presentation_host {
                CoreHostFrameError::DuplicatePresentationObservation
            } else {
                CoreHostFrameError::DuplicatePresentationObservationForHost {
                    host: presentation_host,
                }
            };
            return self.reject(error);
        }
        let Some(scope) = self.presentation_scopes.get(&presentation_host).cloned() else {
            return self.reject(
                CoreHostFrameError::PresentationObservationHostOutsideScope {
                    host: presentation_host,
                },
            );
        };
        if let HostPresentationObservation::Batch(entries) = &observation {
            let mut submitted = BTreeSet::new();
            for entry in entries {
                if !submitted.insert(entry.stream()) {
                    return self.reject(
                        CoreHostFrameError::DuplicatePresentationObservationStream {
                            stream: entry.stream(),
                        },
                    );
                }
            }
            if let Some(stream) = scope
                .iter()
                .copied()
                .find(|stream| !submitted.contains(stream))
            {
                return self
                    .reject(CoreHostFrameError::MissingPresentationObservationStream { stream });
            }
            if let Some(stream) = submitted
                .iter()
                .copied()
                .find(|stream| !scope.contains(stream))
            {
                return self.reject(
                    CoreHostFrameError::PresentationObservationStreamOutsideScope { stream },
                );
            }
        }
        self.presentation_observations
            .insert(presentation_host, observation);
        Ok(())
    }

    /// Reduces all observations into one private rollback candidate and freezes
    /// the post-observation host-frame authority.
    ///
    /// # Errors
    ///
    /// Returns an error when the prelude is incomplete or stale, observation
    /// reduction fails, or the resulting pointer provider cannot be admitted
    /// against the sealed host, surface, and binding scope.
    pub fn seal(self, engine: &DockEngine) -> Result<CoreHostFrame, EngineError> {
        engine.seal_host_frame(self)
    }

    fn reject(&mut self, error: CoreHostFrameError) -> Result<(), CoreHostFrameError> {
        let retained = self.poison.get_or_insert(error).clone();
        Err(retained)
    }
}

impl DockEngine {
    /// Begins the observation-only prelude of one core-owned host frame.
    ///
    /// The returned capability freezes only domain, predecessor, host identity,
    /// platform-provider authority frontier, and pending presentation-stream
    /// scope. The caller must submit every enrolled observation and call
    /// [`CoreHostFramePrelude::seal`] before any scene, pointer, contribution,
    /// or semantic-input authority exists.
    ///
    /// # Errors
    ///
    /// Returns an error when `presentation_host` was not minted by this exact
    /// engine authority domain.
    pub fn begin_host_frame(
        &self,
        presentation_host: PresentationHostLease,
    ) -> Result<CoreHostFramePrelude, EngineError> {
        self.begin_host_frame_with_observers(presentation_host, [])
    }

    /// Begins one core-owned host frame with an explicit rendering host and
    /// supplementary presentation observers.
    ///
    /// The rendering host is the only participant permitted to submit the
    /// complete contribution roster or stage current output. Each observer is
    /// frozen into the same reducer boundary solely so it can settle its own
    /// delayed presentation stream. This prevents a superseded host from
    /// fabricating a second render pass merely to retire its old output.
    ///
    /// # Errors
    ///
    /// Returns an error when an observer repeats, names the rendering host, or
    /// was not minted by this exact engine authority domain.
    pub fn begin_host_frame_with_observers(
        &self,
        presentation_host: PresentationHostLease,
        observers: impl IntoIterator<Item = PresentationHostLease>,
    ) -> Result<CoreHostFramePrelude, EngineError> {
        if let Some(provider) = self.pointer_journal.abandoned_surface_local_provider() {
            return Err(EngineError::SurfaceLocalPointerProviderAbandoned { provider });
        }
        let mut presentation_observers = BTreeSet::from([presentation_host]);
        for observer in observers {
            if observer == presentation_host {
                return Err(EngineError::HostFrameObserverIsRenderingHost { host: observer });
            }
            if !presentation_observers.insert(observer) {
                return Err(EngineError::HostFrameObserverDuplicate { host: observer });
            }
        }
        CoreHostFramePrelude::new(self, presentation_host, presentation_observers)
    }

    /// Seals an observation-only prelude into one rollbackable host-frame candidate.
    fn seal_host_frame(&self, prelude: CoreHostFramePrelude) -> Result<CoreHostFrame, EngineError> {
        if let Some(source) = prelude.poison {
            return Err(EngineError::HostFramePoisoned { source });
        }
        if prelude.authority_domain != self.authority_domain {
            return Err(EngineError::HostFrameAuthorityDomainMismatch {
                expected: self.authority_domain,
                submitted: prelude.authority_domain,
            });
        }
        let current_host_frontier = self.presentation_authority.presentation.host_frontier();
        if prelude.presentation_host_frontier != current_host_frontier {
            return Err(EngineError::HostFramePresentationHostFrontierStale {
                submitted: prelude.presentation_host_frontier,
                current: current_host_frontier,
            });
        }
        let current_platform_provider_frontier = self.viewport.platform_provider_frontier();
        if prelude.platform_provider_frontier != current_platform_provider_frontier {
            return Err(EngineError::HostFramePlatformProviderFrontierStale {
                submitted: prelude.platform_provider_frontier.get(),
                current: current_platform_provider_frontier.get(),
            });
        }
        for host in prelude.presentation_scopes.keys().copied() {
            self.presentation_authority
                .presentation
                .validate_lease(host)
                .map_err(presentation_ledger_error)?;
        }
        if prelude.predecessor_tick != self.last_reducer_tick {
            return Err(EngineError::HostFramePredecessorStale {
                submitted: prelude.predecessor_tick,
                current: self.last_reducer_tick,
            });
        }

        let mut observation_batches = Vec::with_capacity(prelude.presentation_scopes.len());
        for (host, scope) in &prelude.presentation_scopes {
            let current_scope = self
                .presentation_authority
                .presentation
                .pending_stream_scope(*host)
                .map_err(presentation_ledger_error)?;
            if *scope != current_scope {
                if *host == prelude.presentation_host {
                    return Err(EngineError::HostFramePresentationScopeStale {
                        submitted: scope.iter().copied().collect(),
                        current: current_scope.into_iter().collect(),
                    });
                }
                return Err(EngineError::HostFrameSupplementaryPresentationScopeStale {
                    host: *host,
                    submitted: scope.iter().copied().collect(),
                    current: current_scope.into_iter().collect(),
                });
            }
            let observation = prelude
                .presentation_observations
                .get(host)
                .cloned()
                .ok_or_else(|| {
                    if *host == prelude.presentation_host {
                        EngineError::HostFramePresentationObservationMissing
                    } else {
                        EngineError::HostFrameSupplementaryPresentationObservationMissing {
                            host: *host,
                        }
                    }
                })?;
            observation_batches.push((*host, scope.clone(), observation));
        }

        let admission_workspace = self.version;
        let admission_requirements = self
            .presentation_authority
            .presentation_requirements
            .revision();
        let admission_surface_scope = self
            .presentation_authority
            .presentation_requirements
            .surfaces()
            .map(|(surface, _)| surface)
            .collect::<BTreeSet<_>>();
        let runtime_retention_revision = self.runtime_retention_revision;
        let mut candidate = self.candidate();
        candidate
            .viewport
            .set_native_binding_allocation_budget(prelude.native_binding_allocation_budget);
        let vacancy_ledger = TickVacancyLedger::capture(&candidate);
        let tick = candidate
            .last_reducer_tick
            .checked_next()
            .ok_or(EngineError::ReducerTickExhausted)?;
        candidate.last_reducer_tick = tick;
        let mut presentation_observation_outcomes = Vec::new();
        let mut changed_surfaces = BTreeSet::new();
        let mut observation_events = Vec::new();
        let mut observation_interaction_events = Vec::new();
        for (host, scope, observation) in observation_batches {
            let (mut outcomes, changed, mut lifecycle_events) = candidate
                .reduce_presentation_observation(
                    ReductionCause::SurfacePresentationObservationBatch { tick },
                    host,
                    &scope,
                    observation,
                )?;
            presentation_observation_outcomes.append(&mut outcomes);
            changed_surfaces.extend(changed);
            observation_events.append(&mut lifecycle_events);
        }
        if candidate.settle_presented_pending_mutation(
            &mut observation_events,
            &mut observation_interaction_events,
        )? {
            candidate.rebuild_presentation_requirements(candidate.last_input)?;
        }
        candidate.reconcile_interaction_after_presentation_observations(
            tick,
            &changed_surfaces,
            &mut observation_interaction_events,
        )?;

        CoreHostFrame::from_observed_candidate(
            prelude,
            candidate,
            tick,
            runtime_retention_revision,
            admission_workspace,
            admission_requirements,
            admission_surface_scope,
            vacancy_ledger,
            presentation_observation_outcomes,
            observation_events,
            observation_interaction_events,
        )
    }
}

impl<'frame> HostFrameView<'frame> {
    pub(crate) fn splitter_resize_delta_interval(
        &self,
        plan: &PresentationPlan,
        target: crate::scene::SplitterResizeTarget,
        axis: Axis,
    ) -> Option<crate::interaction::ResizeDeltaInterval> {
        self.engine
            .splitter_resize_delta_interval(plan, target, axis)
    }

    /// Returns the post-observation workspace visible to this frame.
    #[must_use]
    pub fn workspace(&self) -> &'frame Workspace {
        self.engine.workspace()
    }

    /// Returns the post-observation workspace version visible to this frame.
    #[must_use]
    pub const fn version(&self) -> WorkspaceVersion {
        self.engine.version()
    }

    pub(crate) const fn authority_domain(&self) -> EngineAuthorityDomainId {
        self.engine.authority_domain()
    }

    #[cfg(feature = "serde")]
    pub(crate) const fn presentation_identity_frontier(&self) -> PresentationIdentityFrontier {
        self.engine.presentation_identity_frontier()
    }

    /// Returns the exact policy used by this frame's candidate.
    #[must_use]
    #[cfg(test)]
    pub const fn policy(&self) -> &'frame DockPolicy {
        self.engine.policy()
    }

    /// Returns the validated presentation configuration used by this frame.
    #[must_use]
    pub const fn presentation_config(&self) -> &'frame DockPresentationConfig {
        self.engine.presentation_config()
    }

    pub(crate) fn prepare_product_action_if_available(
        &self,
        action: crate::model::ProductAction,
    ) -> Result<
        Result<crate::model::PreparedDockAction, crate::model::DockspaceActionRejection>,
        EngineError,
    > {
        self.engine.prepare_product_action_if_available(action)
    }

    pub(crate) fn root_close_is_available(&self, root: RootId) -> bool {
        self.engine
            .capture_content_close(
                crate::command::ContentCloseTarget::Root(root),
                self.engine.policy_snapshot(),
            )
            .is_ok()
    }

    pub(crate) fn default_contained_float_rect(
        &self,
        root: RootId,
        surface: SurfaceId,
        plan: &crate::scene::PresentationPlan,
    ) -> Result<crate::geometry::LogicalRect, crate::model::DockspaceActionRejection> {
        self.engine
            .derive_product_root_float_rect_from_plan(root, surface, plan)
    }

    pub(crate) fn root_is_bound_native_main(&self, root: RootId) -> bool {
        self.engine.root_is_bound_native_main(root)
    }

    pub(crate) fn native_main_recovery_surface(
        &self,
        root: RootId,
    ) -> Result<Option<SurfaceId>, crate::model::DockspaceActionRejection> {
        self.engine.native_main_float_target(root)
    }

    pub(crate) fn directional_dock_placement(
        &self,
        root: RootId,
        edge: Option<crate::model::DockEdge>,
    ) -> Result<crate::model::DockPlacement, crate::model::DockspaceActionRejection> {
        self.engine
            .derive_product_directional_dock_placement(root, edge)
    }

    /// Returns the exact semantic requirement manifest visible to this frame.
    #[must_use]
    pub const fn presentation_requirements(&self) -> &'frame SceneRequirementManifest {
        self.engine.presentation_requirements()
    }

    /// Returns the exact immutable source resources retained for native admission.
    ///
    /// This roster is independent of output slots. Resources stay pinned until
    /// exact first-live ownership transfer or the completion of native cleanup.
    #[cfg(test)]
    pub fn retained_native_staging_resources(
        &self,
    ) -> impl ExactSizeIterator<Item = &NativeStagingResourceDescriptor> + '_ {
        self.engine.viewport.retained_native_staging_resources()
    }

    /// Returns the post-observation authoritative scene state.
    #[must_use]
    pub const fn scene(&self) -> &'frame SurfaceSceneSet {
        self.engine.scene()
    }

    /// Returns the post-observation interaction state.
    #[must_use]
    pub const fn interaction(&self) -> &'frame InteractionState {
        self.engine.interaction()
    }

    /// Returns the transient interaction tokens frozen for one surface paint.
    #[must_use]
    pub fn presentation_interaction(
        &self,
        surface: SurfaceId,
    ) -> Option<HostInteractionPresentation> {
        self.presentation_roster
            .surface_output(surface)
            .and_then(|output| output.payload().interaction())
    }

    /// Returns the exact frozen drag preview geometry which this surface must paint.
    #[must_use]
    pub fn presentation_drag_preview(
        &self,
        surface: SurfaceId,
    ) -> Option<&'frame crate::interaction::InteractionPreview> {
        let expected = self.presentation_interaction(surface)?.drag_preview()?;
        self.engine
            .presentation_preview()
            .filter(|preview| preview.token() == expected && preview.visual().surface() == surface)
    }

    /// Reads the presented target and captured payload of an existing preview.
    /// No candidate transaction or source-membership reconstruction is performed.
    pub(crate) fn presentation_dock_target_context(
        &self,
        preview: &crate::interaction::InteractionPreview,
    ) -> Option<(
        &'frame PresentationPlan,
        crate::drop_target::DropTargetId,
        Option<&'frame MovePayload>,
    )> {
        let PreviewVisual::Dock {
            target, surface, ..
        } = *preview.visual()
        else {
            return None;
        };
        let token = preview.token();
        let drag = self
            .engine
            .interaction
            .active_drag(token.session())
            .ok()
            .or_else(|| {
                self.engine
                    .pending_mutation
                    .as_ref()
                    .and_then(PendingMutation::drag_release)
                    .filter(|pending| pending.preview == token)
                    .map(|pending| &pending.drag)
            });
        let (published, payload) = match drag {
            Some(drag) => (drag.preview()?, Some(&drag.payload)),
            None => {
                let pending = self.engine.pending_mutation.as_ref()?.rehome()?;
                let payload = match pending.preview.proof() {
                    PreviewProof::PresentationRehome {
                        command: WorkspaceCommand::Move { payload, .. },
                    } => Some(payload),
                    _ => None,
                };
                (&pending.preview, payload)
            }
        };
        if published.public().token() != token {
            return None;
        }
        let presented = self.engine.scene().interaction_projection(surface)?;
        let plan = presented.plan();
        if presented.output().stamp() != token.scene() {
            let projection = plan.contained_projection()?;
            if projection.owner != crate::scene::ContainedLayoutOwner::Drag(token.session())
                || projection.target_basis.is_none()
                || projection.target_basis != published.target_layout_basis()
                || target.root() == Some(projection.root)
            {
                return None;
            }
        }
        Some((plan, target, payload))
    }

    /// Returns the exact frozen contained-transform preview which this surface must paint.
    #[must_use]
    pub fn presentation_contained_transform_preview(
        &self,
        surface: SurfaceId,
    ) -> Option<&'frame ContainedTransformPreview> {
        let expected = self
            .presentation_interaction(surface)?
            .contained_transform_preview()?;
        self.engine
            .presentation_contained_transform_preview()
            .filter(|preview| preview.token() == expected && preview.surface() == surface)
    }

    /// Returns the current receiver-authoritative projection for one surface.
    #[must_use]
    pub fn interaction_projection(
        &self,
        surface: SurfaceId,
    ) -> Option<SurfaceInteractionProjection<'frame>> {
        self.receiver_presentations
            .get(&surface)
            .map(JournalSurfacePresentation::projection)
    }

    /// Returns the exact prepared-action projection for the test host roster.
    /// host-frame roster. Unlike pointer receiver authority, semantic actions
    /// are not scoped to the surface that supplied a local pointer provider.
    #[must_use]
    #[cfg(test)]
    pub fn prepared_action_projection(
        &self,
        surface: SurfaceId,
    ) -> Option<SurfaceInteractionProjection<'frame>> {
        self.prepared_presentations
            .get(&surface)
            .map(JournalSurfacePresentation::projection)
    }

    /// Resolves the unique core-owned HoverDrop receiver at one logical point.
    ///
    /// The returned receipt fact owns its winner or `NoReceiver` disposition
    /// and is atomically bound to this sealed frame's exact output, presented
    /// authority, and point. Framework occlusion remains an adapter fact: call
    /// this only after the framework authoritatively proves that no foreign
    /// receiver blocks docking at the point.
    pub fn resolve_hover_drop_receiver(
        &self,
        surface: SurfaceId,
        point: crate::geometry::LogicalPoint,
    ) -> Result<PointerReceiverHoverHit, HostFrameHoverDropResolutionError> {
        let projection = self.interaction_projection(surface).ok_or(
            HostFrameHoverDropResolutionError::InteractionProjectionUnavailable { surface },
        )?;
        let disposition = match projection
            .hit_manifest()
            .resolve_exclusive_with_suppression(
                PresentationPointerLane::HoverDrop,
                point,
                self.engine.hover_drop_suppression(),
            ) {
            Ok(Some(region)) => PointerReceiverHoverHitDisposition::Dock(region.id()),
            Ok(None) => PointerReceiverHoverHitDisposition::NoReceiver,
            Err(PresentationHitResolutionError::Ambiguous { first, second, .. }) => {
                return Err(HostFrameHoverDropResolutionError::Ambiguous { first, second });
            }
        };
        PointerReceiverHoverHit::new(projection, point, disposition)
            .map_err(|source| HostFrameHoverDropResolutionError::ObservationBinding { source })
    }

    /// Prepares one keyboard, accessibility, or other non-pointer tab-strip control activation.
    ///
    /// The control must exist and be enabled in this sealed frame's exact
    /// receiver-authoritative output. The resulting value is opaque and must be
    /// submitted as [`EngineInput::ActivateTabStripControl`]; reduction rechecks
    /// the same presented authority, popup revision, and control record.
    #[cfg(test)]
    pub fn prepare_tab_strip_control_activation(
        &self,
        surface: SurfaceId,
        control: TabStripControlId,
    ) -> Result<PreparedTabStripControlActivation, InteractionRejection> {
        let key = TabStripStateKey::new(surface, control.bar());
        let projection = self
            .prepared_action_projection(surface)
            .ok_or(InteractionRejection::TabStripSourceUnavailable { key })?;
        self.engine
            .prepare_presented_tab_strip_control(projection, control)
    }

    /// Prepares one keyboard or accessibility activation of an exact active menu row.
    ///
    /// The row is resolved against the sole active popup session in this sealed
    /// output. The opaque result cannot be retargeted to another item, session,
    /// popup revision, or presented scene before reduction.
    #[cfg(test)]
    pub fn prepare_tab_list_menu_row_activation(
        &self,
        surface: SurfaceId,
        session: crate::tab_strip::TabListMenuSessionId,
        tab: crate::scene::TabSceneId,
    ) -> Result<PreparedTabListMenuRowActivation, InteractionRejection> {
        let projection = self
            .prepared_action_projection(surface)
            .ok_or(InteractionRejection::TabListMenuSessionUnavailable { session })?;
        self.engine
            .prepare_presented_tab_list_menu_row(projection, session, tab)
    }

    /// Prepares one Escape, keyboard, or programmatic dismissal of the exact
    /// active tab-list menu.
    #[cfg(test)]
    pub fn prepare_tab_list_menu_dismiss(
        &self,
        surface: SurfaceId,
        session: crate::tab_strip::TabListMenuSessionId,
    ) -> Result<PreparedTabListMenuDismiss, InteractionRejection> {
        let projection = self
            .prepared_action_projection(surface)
            .ok_or(InteractionRejection::TabListMenuSessionUnavailable { session })?;
        let plan = projection.plan();
        let revision = plan.popup().revision();
        self.engine
            .validate_tab_list_menu_popup(plan, session, revision)?;
        let record = plan
            .tab_list_menu_records()
            .iter()
            .find(|record| record.session() == session)
            .cloned()
            .ok_or(InteractionRejection::TabListMenuSessionUnavailable { session })?;
        let backdrop = plan
            .tab_list_menu_backdrop_records()
            .iter()
            .copied()
            .find(|record| record.session() == session && record.revision() == revision)
            .ok_or(InteractionRejection::TabListMenuBackdropUnavailable { session })?;
        if !self
            .engine
            .presentation_authority
            .tab_strip_states
            .active_menu_for(session.key())
            .is_some_and(|active| active.session() == session)
        {
            return Err(InteractionRejection::TabListMenuSessionUnavailable { session });
        }
        Ok(PreparedTabListMenuDismiss {
            presentation: DockEngine::freeze_interaction_projection(projection),
            session,
            revision,
            record,
            backdrop,
        })
    }

    /// Prepares one exact tab-strip wheel, edge-drag, or reveal adjustment.
    ///
    /// The bar must belong to `surface`, remain interaction-enabled, and exist
    /// in this sealed frame's receiver-authoritative projection.
    #[cfg(test)]
    pub fn prepare_tab_strip_scroll(
        &self,
        surface: SurfaceId,
        bar: crate::scene::TabBarSceneId,
        adjustment: TabScrollAdjustment,
    ) -> Result<PreparedTabStripScroll, InteractionRejection> {
        let key = TabStripStateKey::new(surface, bar);
        let projection = self
            .prepared_action_projection(surface)
            .ok_or(InteractionRejection::TabStripSourceUnavailable { key })?;
        let record = projection
            .plan()
            .tab_bar_records()
            .iter()
            .find(|record| *record.id() == bar)
            .cloned()
            .ok_or(InteractionRejection::TabStripSourceUnavailable { key })?;
        if record.interaction() != TabBarInteraction::Enabled {
            return Err(InteractionRejection::TabStripScrollDisabled { key });
        }
        if self
            .engine
            .presentation_authority
            .tab_strip_states
            .state(key)
            .is_none()
        {
            return Err(InteractionRejection::TabStripSourceUnavailable { key });
        }
        let requested_items: &[ItemId] = match &adjustment.0 {
            TabScrollAdjustmentKind::ScrollByPreserving { keep_visible, .. } => keep_visible,
            TabScrollAdjustmentKind::RevealItem(item) => std::slice::from_ref(item),
        };
        if let Some(item) = requested_items.iter().copied().find(|item| {
            !record
                .members()
                .iter()
                .any(|member| member.tab().item == *item)
        }) {
            return Err(InteractionRejection::TabStripScrollItemUnavailable { key, item });
        }
        Ok(PreparedTabStripScroll {
            presentation: DockEngine::freeze_interaction_projection(projection),
            key,
            record,
            adjustment,
        })
    }

    /// Prepares one exact active tab-list menu wheel or reveal adjustment.
    ///
    /// The session, popup routing revision, complete menu geometry, and target
    /// item membership are all frozen from this sealed frame.
    #[cfg(test)]
    pub fn prepare_tab_list_menu_scroll(
        &self,
        surface: SurfaceId,
        session: crate::tab_strip::TabListMenuSessionId,
        adjustment: TabScrollAdjustment,
    ) -> Result<PreparedTabListMenuScroll, InteractionRejection> {
        let projection = self
            .prepared_action_projection(surface)
            .ok_or(InteractionRejection::TabListMenuSessionUnavailable { session })?;
        self.engine
            .prepare_presented_tab_list_menu_scroll(projection, session, adjustment)
    }

    /// Prepares one exact keyboard or accessibility focus move in the active menu.
    ///
    /// Relative moves are resolved against the sealed ordered roster and clamp
    /// at its boundaries. The prepared proof binds the resolved target to the
    /// exact popup revision and complete menu geometry used for reveal.
    #[cfg(test)]
    pub fn prepare_tab_list_menu_navigation(
        &self,
        surface: SurfaceId,
        session: crate::tab_strip::TabListMenuSessionId,
        navigation: TabListMenuNavigation,
    ) -> Result<PreparedTabListMenuNavigation, InteractionRejection> {
        let projection = self
            .prepared_action_projection(surface)
            .ok_or(InteractionRejection::TabListMenuSessionUnavailable { session })?;
        self.engine
            .prepare_presented_tab_list_menu_navigation(projection, session, navigation)
    }

    /// Returns this frame's read-only viewport coordinator.
    #[must_use]
    pub const fn viewport(&self) -> &'frame ViewportCoordinator {
        self.engine.viewport()
    }

    /// Returns this frame's adapter-neutral viewport focus state.
    #[must_use]
    pub const fn viewport_focus(&self) -> &'frame ViewportFocusCoordinator {
        self.engine.viewport_focus()
    }

    pub(crate) fn close_plan(&self, request: CloseRequestId) -> Option<&'frame ClosePlan> {
        self.engine.close_plan(request)
    }

    /// Returns the pane-focus request admitted at this committed boundary.
    ///
    /// The request becomes observable to an adapter only after the boundary
    /// that publishes its obligation.
    #[must_use]
    pub fn published_pane_focus_intent(&self) -> Option<PaneFocusIntent> {
        self.engine.viewport_focus.published_pane_intent()
    }

    /// Freezes one contribution token against this frame's exact authority.
    pub fn begin_surface_contribution(
        &self,
        surface: SurfaceId,
    ) -> Result<SurfaceContributionToken, SurfaceContributionBeginError> {
        if !self.presentation_roster.contains_surface(surface) {
            return Err(SurfaceContributionBeginError::SurfaceOutsideRoster { surface });
        }
        self.engine.begin_surface_contribution(surface)
    }

    /// Compiles one exact measurement answer against this frame's authority.
    pub fn prepare_surface_contribution(
        &self,
        token: SurfaceContributionToken,
        measurements: SurfaceMeasurements,
    ) -> Result<PreparedSurfaceContribution, SurfaceContributionPrepareError> {
        if !self.presentation_roster.contains_surface(token.surface()) {
            return Err(SurfaceContributionPrepareError::SurfaceOutsideRoster {
                surface: token.surface(),
            });
        }
        self.engine
            .prepare_surface_contribution(token, measurements)
    }

    /// Prepares one explicit unavailable answer against this frame's authority.
    pub fn prepare_surface_unavailable_contribution(
        &self,
        token: SurfaceContributionToken,
        reason: MeasurementUnavailableReason,
    ) -> Result<PreparedSurfaceContribution, SurfaceContributionPrepareError> {
        if !self.presentation_roster.contains_surface(token.surface()) {
            return Err(SurfaceContributionPrepareError::SurfaceOutsideRoster {
                surface: token.surface(),
            });
        }
        self.engine
            .prepare_surface_unavailable_contribution(token, reason)
    }

    /// Retains one exact Ready candidate visible to this frame.
    pub fn prepare_surface_retained_contribution(
        &self,
        token: SurfaceContributionToken,
    ) -> Result<PreparedSurfaceContribution, SurfaceContributionPrepareError> {
        if !self.presentation_roster.contains_surface(token.surface()) {
            return Err(SurfaceContributionPrepareError::SurfaceOutsideRoster {
                surface: token.surface(),
            });
        }
        self.engine.prepare_surface_retained_contribution(token)
    }
}

impl CoreHostFrame {
    pub(crate) fn candidate_engine(&self) -> &DockEngine {
        &self.candidate
    }

    pub(crate) fn capture_encoding_checkpoint(
        &mut self,
    ) -> Result<CoreEncodingCheckpoint, CoreHostFrameError> {
        self.begin_presentation_phase()?;
        Ok(CoreEncodingCheckpoint {
            presentation: self.candidate.presentation_authority.clone(),
            interaction: self.candidate.interaction.clone(),
            roster: self.frozen_presentation_roster.clone(),
            snapshot_changed: self.presentation_snapshot_changed,
            projection_changed: self.presentation_projection_changed,
        })
    }

    pub(crate) fn restart_encoding_from(
        &mut self,
        checkpoint: &CoreEncodingCheckpoint,
    ) -> Result<(), CoreHostFrameError> {
        if let Some(error) = &self.poison {
            return Err(error.clone());
        }
        let mut presentation = checkpoint.presentation.clone();
        presentation
            .scene
            .preserve_revision_frontier(&self.candidate.presentation_authority.scene);
        presentation.last_presentation_output_serial =
            presentation.last_presentation_output_serial.max(
                self.candidate
                    .presentation_authority
                    .last_presentation_output_serial,
            );
        self.candidate.presentation_authority = presentation;
        self.candidate.interaction = checkpoint.interaction.clone();
        self.frozen_presentation_roster = checkpoint.roster.clone();
        self.presentation_snapshot_changed = checkpoint.snapshot_changed;
        self.presentation_projection_changed = checkpoint.projection_changed;
        self.surface_contributions.clear();
        self.compiled_surface_outcomes.clear();
        self.staged_presentation_outputs.clear();
        self.staged_presentation_output_surfaces.clear();
        Ok(())
    }

    pub(crate) fn capture_host_focus_observation(
        &self,
        surface: SurfaceId,
        generation: crate::viewport_focus::PaneFocusObservationGeneration,
        state: crate::viewport_focus::HostFocusState,
    ) -> Option<crate::viewport_focus::HostFocusObservation> {
        let (stream, host, _) = self
            .candidate
            .presentation_authority
            .presentation
            .active_surface_scope(surface)?;
        (host == self.presentation_host && self.candidate.workspace.surface(surface).is_some())
            .then(|| {
                crate::viewport_focus::HostFocusObservation::new(
                    generation, host, stream, surface, state,
                )
            })
    }

    pub(crate) const fn presentation_attempt(&self) -> HostPresentationAttemptId {
        self.presentation_obligations.attempt()
    }

    pub(super) fn from_observed_candidate(
        prelude: CoreHostFramePrelude,
        candidate: DockEngine,
        tick: ReducerTickId,
        runtime_retention_revision: u64,
        admission_workspace: WorkspaceVersion,
        admission_requirements: RequirementRevision,
        admission_surface_scope: BTreeSet<SurfaceId>,
        vacancy_ledger: TickVacancyLedger,
        presentation_observation_outcomes: Vec<HostPresentationObservationOutcome>,
        observation_events: Vec<WorkspaceEvent>,
        observation_interaction_events: Vec<InteractionEvent>,
    ) -> Result<Self, EngineError> {
        let CoreHostFramePrelude {
            authority_domain,
            presentation_host,
            predecessor_tick,
            presentation_host_frontier,
            platform_provider_frontier,
            presentation_scopes,
            presentation_observations: _,
            item_identity_scope,
            native_binding_allocation_budget: _,
            poison: _,
        } = prelude;
        let frozen_presentation_roster = HostPresentationRoster::capture(&candidate)?;
        let pointer_provider = candidate.pointer_journal.active_lease();
        if let Some(provider) = pointer_provider {
            candidate.validate_pointer_provider_scope(provider.scope())?;
        }
        if let Some(host) = pointer_provider.and_then(|lease| lease.scope().local_host())
            && host != presentation_host
        {
            return Err(EngineError::PointerProviderHostOutsideFrameScope {
                provider: pointer_provider.expect("surface-local provider was present"),
                host,
            });
        }
        let backend_ingress = candidate.backend_ingress.active();
        if let Some(ingress) = backend_ingress {
            if ingress.presentation_host() != presentation_host {
                return Err(EngineError::BackendIngress {
                    source: BackendIngressError::PresentationHostLeaseMismatch {
                        expected: ingress.presentation_host(),
                        submitted: presentation_host,
                    },
                });
            }
            let platform = candidate.platform_provider();
            if platform != Some(ingress.platform_provider())
                || pointer_provider != Some(ingress.pointer_provider())
            {
                return Err(EngineError::BackendIngressProviderMismatch {
                    ingress,
                    platform,
                    pointer: pointer_provider,
                });
            }
        }
        let presentation_attempt = candidate
            .presentation_authority
            .issue_host_presentation_attempt()
            .map_err(|source| EngineError::HostFramePoisoned { source })?;
        let presentation_obligations = HostPresentationObligationSet::new(
            presentation_attempt,
            frozen_presentation_roster.clone(),
        );
        let tick_policy = candidate.policy.clone();
        let application_base = candidate.version;
        let mut frame = Self {
            platform_effect_delivery_limit: None,
            authority_domain,
            presentation_host,
            workspace: admission_workspace,
            requirements: admission_requirements,
            predecessor_tick,
            presentation_host_frontier,
            platform_provider_frontier,
            runtime_retention_revision,
            tick,
            admission_surface_scope,
            frozen_presentation_roster,
            presentation_obligations,
            backend_ingress,
            backend_ingress_batch_submitted: false,
            backend_ingress_complete: false,
            backend_ingress_commit_guard: None,
            pending_backend_ingress: None,
            pointer_provider,
            staged_pointer_journal: candidate.pointer_journal.clone(),
            surface_pointer_commit: None,
            frozen_pointer_outputs: BTreeMap::new(),
            frozen_pointer_presentations: BTreeMap::new(),
            #[cfg(test)]
            frozen_prepared_presentations: BTreeMap::new(),
            pointer_receiver_attempt_issuer: pointer_provider
                .map(|_| Arc::clone(&candidate.pointer_receiver_attempt_issuer)),
            pending_pointer_segment: None,
            pointer_segment_submitted: false,
            presentation_scopes,
            item_identity_scope,
            presentation_snapshot_changed: false,
            presentation_projection_changed: false,
            candidate,
            tick_policy,
            vacancy_ledger,
            application_base,
            presentation_observation_outcomes,
            reduced_inputs: Vec::new(),
            reduced_pointer_edges: Vec::new(),
            events: observation_events,
            interaction_events: observation_interaction_events,
            last_reduced_input: None,
            last_causal_cause: None,
            staged_presentation_outputs: Vec::new(),
            staged_presentation_output_surfaces: BTreeSet::new(),
            surface_contributions: BTreeMap::new(),
            compiled_surface_outcomes: BTreeMap::new(),
            presentation_phase_started: false,
            configuration_phase_started: false,
            input_terminated_for_overload: false,
            next_causal_ordinal: 0,
            poison: None,
        };
        frame.refresh_presented_interaction_authority();
        Ok(frame)
    }

    fn refresh_presentation_snapshot(&mut self) -> Result<(), EngineError> {
        let roster = HostPresentationRoster::capture(&self.candidate)?;
        self.presentation_projection_changed |= !self
            .frozen_presentation_roster
            .same_semantic_projection(&roster);
        self.presentation_snapshot_changed |= self.frozen_presentation_roster != roster;
        self.frozen_presentation_roster = roster;
        self.retain_presented_interaction_authority_in_roster();
        Ok(())
    }

    pub(crate) fn refresh_presentation_layout(&mut self) -> Result<(), CoreHostFrameError> {
        self.refresh_presentation_layout_except(None)
    }

    pub(crate) fn refresh_presentation_layout_except(
        &mut self,
        freshly_measured: Option<SurfaceId>,
    ) -> Result<(), CoreHostFrameError> {
        if let Some(error) = &self.poison {
            return Err(error.clone());
        }
        if self.presentation_phase_started {
            return Ok(());
        }
        let refreshed = self
            .candidate
            .refresh_presentation_layouts(self.tick, freshly_measured);
        match refreshed {
            Ok(false) => Ok(()),
            Ok(true) => match self.refresh_presentation_snapshot() {
                Ok(()) => Ok(()),
                Err(error) => self.reject_input_prefix(error),
            },
            Err(error) => self.reject_input_prefix(error),
        }
    }

    fn try_refresh_presentation_surfaces(
        &mut self,
        dirty_surfaces: impl IntoIterator<Item = SurfaceId>,
    ) -> bool {
        let Some((snapshot_changed, projection_changed)) = self
            .frozen_presentation_roster
            .try_refresh_surfaces(&self.candidate, dirty_surfaces)
        else {
            return false;
        };
        self.presentation_snapshot_changed |= snapshot_changed;
        self.presentation_projection_changed |= projection_changed;
        true
    }

    fn refresh_presentation_after_pointer_segment(
        &mut self,
        before: PointerPresentationFence,
        reduced: &[crate::transition::ReducedPointerEdge],
    ) -> Result<(), EngineError> {
        let after = PointerPresentationFence::capture(&self.candidate);
        if before.workspace == after.workspace && before.requirements == after.requirements {
            if reduced.is_empty() && self.try_refresh_presentation_surfaces([]) {
                return Ok(());
            }

            let moved_only = !reduced.is_empty()
                && reduced.iter().all(|edge| {
                    edge.edge().kind() == PointerEdgeKind::Moved
                        && edge
                            .interaction_outcomes()
                            .iter()
                            .all(pointer_move_outcome_allows_local_presentation_refresh)
                });
            if moved_only
                && self.try_refresh_presentation_surfaces(
                    [
                        before.drag_preview_surface,
                        after.drag_preview_surface,
                        before.contained_preview_surface,
                        after.contained_preview_surface,
                    ]
                    .into_iter()
                    .flatten(),
                )
            {
                return Ok(());
            }

            let mut dirty_surfaces = Vec::new();
            let scrolled_only = !reduced.is_empty()
                && reduced.iter().all(|edge| {
                    matches!(edge.edge().kind(), PointerEdgeKind::Scrolled(_))
                        && edge.interaction_outcomes().iter().all(|outcome| {
                            record_pointer_scroll_outcome_for_local_presentation_refresh(
                                outcome,
                                &mut dirty_surfaces,
                            )
                        })
                });
            dirty_surfaces.extend(
                [
                    before.drag_preview_surface,
                    after.drag_preview_surface,
                    before.contained_preview_surface,
                    after.contained_preview_surface,
                ]
                .into_iter()
                .flatten(),
            );
            if scrolled_only && self.try_refresh_presentation_surfaces(dirty_surfaces) {
                return Ok(());
            }
        }

        self.refresh_presentation_snapshot()
    }

    fn retain_presented_interaction_authority_in_roster(&mut self) {
        let roster = &self.frozen_presentation_roster;
        self.frozen_pointer_outputs
            .retain(|surface, _| roster.contains_surface(*surface));
        self.frozen_pointer_presentations
            .retain(|surface, _| roster.contains_surface(*surface));
        #[cfg(test)]
        self.frozen_prepared_presentations
            .retain(|surface, _| roster.contains_surface(*surface));
    }

    fn refresh_presented_interaction_authority(&mut self) {
        self.frozen_pointer_outputs.clear();
        self.frozen_pointer_presentations.clear();
        #[cfg(test)]
        self.frozen_prepared_presentations.clear();
        let pointer_surface = self
            .pointer_provider
            .and_then(|lease| lease.scope().surface_local())
            .map(|scope| scope.surface());
        for surface in self.frozen_presentation_roster.surfaces() {
            let Some(projection) = self
                .candidate
                .presentation_authority
                .scene
                .interaction_projection(surface)
            else {
                continue;
            };
            #[cfg(test)]
            self.frozen_prepared_presentations.insert(
                surface,
                JournalSurfacePresentation::from_interaction(projection),
            );
            if pointer_surface.is_none_or(|expected| expected == surface) {
                self.frozen_pointer_outputs.insert(
                    surface,
                    PointerReceiverPresentedOutput::from_interaction(projection),
                );
                self.frozen_pointer_presentations.insert(
                    surface,
                    JournalSurfacePresentation::from_interaction(projection),
                );
            }
        }
    }

    /// Returns the sole post-observation authority from which adapters may
    /// prepare measurements, output, and receiver facts for this frame.
    #[must_use]
    pub const fn view(&self) -> HostFrameView<'_> {
        HostFrameView {
            engine: &self.candidate,
            presentation_roster: &self.frozen_presentation_roster,
            receiver_presentations: &self.frozen_pointer_presentations,
            #[cfg(test)]
            prepared_presentations: &self.frozen_prepared_presentations,
        }
    }

    /// Returns the exact post-observation surface roster frozen at seal.
    #[must_use]
    pub fn surfaces(&self) -> impl ExactSizeIterator<Item = SurfaceId> + '_ {
        self.frozen_presentation_roster.surfaces()
    }

    /// Returns whether the private input prefix changed any presentation fact
    /// after receiver authority was sealed for this frame.
    #[must_use]
    #[cfg(test)]
    pub const fn presentation_changed_since_seal(&self) -> bool {
        self.presentation_snapshot_changed
    }

    /// Returns whether input changed scene, coordinate, or surface projection
    /// authority rather than only transient interaction visuals.
    #[must_use]
    #[cfg(test)]
    pub const fn presentation_projection_changed_since_seal(&self) -> bool {
        self.presentation_projection_changed
    }

    /// Returns the exact joined backend provider frozen for this frame.
    #[must_use]
    pub const fn backend_ingress_provider(&self) -> Option<BackendIngressLease> {
        self.backend_ingress
    }

    pub(crate) const fn input_terminated_for_overload(&self) -> bool {
        self.input_terminated_for_overload
    }

    /// Admits reconciled keys only for this final document-replacement input.
    #[cfg(feature = "serde")]
    pub(crate) fn append_document_replacement(
        &mut self,
        source: StableInputSourceId,
        sequence: SourceSequence,
        input: EngineInput,
        source_scope: &BTreeSet<ItemId>,
        target_scope: Arc<BTreeSet<ItemId>>,
    ) -> Result<(), CoreHostFrameError> {
        if let Some(error) = &self.poison {
            return Err(error.clone());
        }
        self.ensure_input_prefix_complete()?;
        if self.configuration_phase_started {
            return self.reject(CoreHostFrameError::SemanticAfterConfiguration);
        }
        if self.presentation_phase_started {
            return self.reject(CoreHostFrameError::InputAfterPresentation);
        }
        if !matches!(input, EngineInput::RestoreWorkspace(_)) {
            return self.reject(CoreHostFrameError::InputPhaseMismatch {
                phase: HostFrameInputPhase::Semantic,
                actual: input.priority(),
            });
        }
        if self.item_identity_scope.as_deref() != Some(source_scope)
            || !target_scope.is_superset(source_scope)
        {
            return self.reject(CoreHostFrameError::ItemIdentityScopeMismatch);
        }
        self.item_identity_scope = Some(target_scope);
        self.append_input(source, sequence, input)
    }

    pub(crate) const fn backend_ingress_through(&self) -> BackendIngressOrdinal {
        self.candidate.backend_ingress.committed_through()
    }

    /// Reserves only inline ingress cursor and commit-guard transport data.
    pub(crate) const fn backend_ingress_inline_capacity_bytes() -> usize {
        size_of::<HostBackendIngressCursor>() + size_of::<BackendIngressCommitGuard>()
    }

    /// Counts this frame's ingress buffers, whose payloads remain in its recorder.
    pub(crate) fn backend_ingress_buffer_capacity_bytes(&self) -> usize {
        let cursor = self.pending_backend_ingress.as_ref().map_or(0, |cursor| {
            size_of::<HostBackendIngressCursor>()
                .saturating_add(cursor.record_buffer_capacity_bytes())
        });
        let guard = self
            .backend_ingress_commit_guard
            .as_ref()
            .map_or(0, |guard| {
                size_of::<BackendIngressCommitGuard>().saturating_add(guard.owned_capacity_bytes())
            });
        cursor.saturating_add(guard)
    }

    /// Returns the typed reduction failure retained after
    /// [`CoreHostFrameError::InputPrefixReductionFailed`].
    ///
    /// The value is diagnostic-only: the host frame remains poisoned and must
    /// be dropped or finished through its normal rollback path.
    #[must_use]
    #[cfg(test)]
    pub fn input_prefix_error(&self) -> Option<&EngineError> {
        match &self.poison {
            Some(CoreHostFrameError::InputPrefixReductionFailed { source }) => Some(source),
            _ => None,
        }
    }

    pub(crate) fn proposed_workspace_root_rebounds(
        &self,
    ) -> impl Iterator<
        Item = (
            crate::viewport::ViewportBinding,
            crate::viewport::ViewportBinding,
        ),
    > + '_ {
        self.reduced_inputs
            .iter()
            .filter_map(|reduced| match reduced.outcome() {
                crate::transition::InputOutcome::WorkspaceReplaced { reconciliation, .. } => {
                    Some(reconciliation.rebound())
                }
                _ => None,
            })
            .flat_map(|pairs| pairs.iter().copied())
    }

    pub(crate) fn has_native_placement_submission(&self) -> bool {
        self.reduced_inputs.iter().any(|reduced| {
            matches!(
                reduced.outcome(),
                crate::transition::InputOutcome::NativeSurfacePlacementsChanged {
                    changed: true,
                    ..
                }
            )
        })
    }

    /// Starts one immutable backend interval and reduces at most its first record.
    ///
    /// Platform records reduce immediately. A pointer record pauses replay after
    /// freezing its receiver challenge; the caller must answer it through
    /// [`Self::submit_backend_pointer_receiver_receipts`]. At a complete record
    /// boundary the caller explicitly resumes through [`Self::resume_backend_ingress`]
    /// or seals the processed prefix through [`Self::seal_backend_ingress_prefix`].
    /// The candidate watermark advances only at seal or batch completion and
    /// remains rollbackable until this host frame commits.
    ///
    /// # Errors
    ///
    /// Returns an error for an unavailable provider, a second batch, invalid
    /// interval or append authority, a poisoned frame, or failed record reduction.
    pub fn submit_backend_ingress(
        &mut self,
        batch: BackendIngressBatch,
    ) -> Result<BackendIngressProgress, CoreHostFrameError> {
        self.prepare_backend_ingress(batch)?;
        self.advance_backend_ingress()
    }

    pub(crate) fn prepare_backend_ingress(
        &mut self,
        batch: BackendIngressBatch,
    ) -> Result<(), CoreHostFrameError> {
        if let Some(error) = &self.poison {
            return Err(error.clone());
        }
        if self.backend_ingress.is_none() {
            return self.reject_value(CoreHostFrameError::BackendIngressUnexpected);
        }
        if self.backend_ingress_batch_submitted {
            return self.reject_value(CoreHostFrameError::DuplicateBackendIngressBatch);
        }
        if let Err(source) = self.candidate.backend_ingress.validate_batch(&batch) {
            return self.reject_value(CoreHostFrameError::BackendIngressRejected { source });
        }
        self.backend_ingress_batch_submitted = true;
        self.pending_backend_ingress = Some(HostBackendIngressCursor::new(batch));
        Ok(())
    }

    /// Answers and completes the paused pointer record without starting its successor.
    ///
    /// # Errors
    ///
    /// Returns an error without a pending pointer challenge, for inexact receipts,
    /// or when the frame or pointer reduction rejects the submitted evidence.
    pub fn submit_backend_pointer_receiver_receipts(
        &mut self,
        receipts: PointerReceiverReceiptBatch,
    ) -> Result<BackendIngressProgress, CoreHostFrameError> {
        if self.pending_backend_ingress.is_none() || self.pending_pointer_segment.is_none() {
            return self.reject_value(CoreHostFrameError::BackendIngressReceiptsUnexpected);
        }
        self.submit_pointer_receiver_receipts_inner(receipts, true)?;
        self.complete_backend_ingress_record()
    }

    /// Reduces at most the next record after a complete record boundary.
    ///
    /// A pointer record remains incomplete until its exact receiver receipts
    /// reduce successfully. No successor starts while that challenge is pending.
    ///
    /// # Errors
    ///
    /// Returns an error without a live complete-record boundary, for a poisoned
    /// frame, or when reduction of the next record fails.
    pub fn resume_backend_ingress(&mut self) -> Result<BackendIngressProgress, CoreHostFrameError> {
        self.ensure_backend_ingress_boundary()?;
        self.advance_backend_ingress()
    }

    /// Seals only the fully reduced prefix of the submitted immutable batch.
    ///
    /// The suffix never enters the commit guard and remains recorder-owned for
    /// retry. Sealing is forbidden while a pointer receiver challenge is pending.
    /// An indivisible append group must also reach its final record before seal.
    /// The returned watermark remains private until the whole host frame commits.
    ///
    /// # Errors
    ///
    /// Returns an error for a pending pointer challenge, an incomplete atomic
    /// group, a missing cursor, a poisoned frame, or revoked prefix authority.
    pub fn seal_backend_ingress_prefix(
        &mut self,
    ) -> Result<BackendIngressOrdinal, CoreHostFrameError> {
        self.ensure_backend_ingress_boundary()?;
        let cursor = self
            .pending_backend_ingress
            .take()
            .expect("validated backend ingress boundary retains its cursor");
        let batch = match cursor.into_completed_prefix() {
            Ok(batch) => batch,
            Err(source) => {
                return self.reject_value(CoreHostFrameError::BackendIngressRejected { source });
            }
        };
        self.finish_backend_ingress(batch)
    }

    fn ensure_backend_ingress_boundary(&mut self) -> Result<(), CoreHostFrameError> {
        if let Some(error) = &self.poison {
            return Err(error.clone());
        }
        if self.pending_pointer_segment.is_some()
            || !self
                .pending_backend_ingress
                .as_ref()
                .is_some_and(HostBackendIngressCursor::at_record_boundary)
        {
            return self.reject(CoreHostFrameError::BackendIngressBoundaryRequired);
        }
        Ok(())
    }

    fn complete_backend_ingress_record(
        &mut self,
    ) -> Result<BackendIngressProgress, CoreHostFrameError> {
        let Some(cursor) = self.pending_backend_ingress.as_mut() else {
            return self.reject_value(CoreHostFrameError::BackendIngressIncomplete);
        };
        cursor.complete_record();
        if cursor.is_complete() {
            let cursor = self
                .pending_backend_ingress
                .take()
                .expect("completed backend ingress retains its cursor");
            self.finish_backend_ingress(cursor.into_batch())?;
            Ok(BackendIngressProgress::Complete)
        } else {
            Ok(BackendIngressProgress::RecordBoundary {
                completed_records: cursor.completed_records(),
                can_seal: cursor.can_seal(),
            })
        }
    }

    fn finish_backend_ingress(
        &mut self,
        batch: BackendIngressBatch,
    ) -> Result<BackendIngressOrdinal, CoreHostFrameError> {
        if let Err(source) = self.candidate.backend_ingress.commit_batch(&batch) {
            return self.reject_value(CoreHostFrameError::BackendIngressRejected { source });
        }
        let through = batch.through();
        self.backend_ingress_commit_guard = Some(batch.commit_guard());
        self.backend_ingress_complete = true;
        Ok(through)
    }

    fn advance_backend_ingress(&mut self) -> Result<BackendIngressProgress, CoreHostFrameError> {
        let next = {
            let Some(cursor) = self.pending_backend_ingress.as_mut() else {
                return self.reject_value(CoreHostFrameError::BackendIngressIncomplete);
            };
            let Some(next) = cursor.next() else {
                let cursor = self
                    .pending_backend_ingress
                    .take()
                    .expect("checked backend ingress cursor remains present");
                let batch = cursor.into_batch();
                self.finish_backend_ingress(batch)?;
                return Ok(BackendIngressProgress::Complete);
            };
            next
        };

        let (ordinal, payload) = next;
        match payload {
            BackendIngressPayload::PlatformSnapshot {
                expected_epoch,
                snapshot,
            } => {
                let provider = self
                    .backend_ingress
                    .expect("validated backend ingress remains frozen")
                    .platform_provider();
                self.append_backend_input(
                    ordinal,
                    EngineInput::PublishPlatformSnapshot {
                        provider,
                        expected_epoch,
                        snapshot,
                    },
                )?;
            }
            BackendIngressPayload::PlatformBindingQuiesced { .. } => {
                // This is an adapter-retention fact, not a semantic mutation. The recorder
                // turns it into an affine proof only after this complete batch commits.
            }
            BackendIngressPayload::NativeCloseObservation {
                expected_epoch,
                observation,
            } => {
                let provider = self
                    .backend_ingress
                    .expect("validated backend ingress remains frozen")
                    .platform_provider();
                self.append_backend_input(
                    ordinal,
                    EngineInput::PublishNativeCloseObservation {
                        provider,
                        expected_epoch,
                        observation,
                    },
                )?;
            }
            BackendIngressPayload::GlobalFocusObservation {
                expected_epoch,
                observation,
            } => {
                let provider = self
                    .backend_ingress
                    .expect("validated backend ingress remains frozen")
                    .platform_provider();
                self.append_backend_input(
                    ordinal,
                    EngineInput::PublishGlobalFocusObservation {
                        provider,
                        expected_epoch,
                        observation,
                    },
                )?;
            }
            BackendIngressPayload::PlatformEffectResult(result) => {
                let provider = self
                    .backend_ingress
                    .expect("validated backend ingress remains frozen")
                    .platform_provider();
                self.append_backend_input(
                    ordinal,
                    EngineInput::ReportPlatformEffect {
                        provider,
                        expected_epoch: result.receipt_epoch(),
                        result,
                    },
                )?;
            }
            BackendIngressPayload::NativeInputDeviceRemoval => {
                let provider = self
                    .backend_ingress
                    .expect("validated backend ingress remains frozen");
                self.append_backend_input(
                    ordinal,
                    EngineInput::ObserveNativeInputDeviceRemoval { provider },
                )?;
            }
            BackendIngressPayload::NativeInputBindingRetired { binding } => {
                let provider = self
                    .backend_ingress
                    .expect("validated backend ingress remains frozen");
                self.append_backend_input(
                    ordinal,
                    EngineInput::ObserveNativeInputBindingRetired { provider, binding },
                )?;
            }
            BackendIngressPayload::SemanticInput(input) => {
                self.append_backend_semantic_input(ordinal, *input)?;
            }
            BackendIngressPayload::PresentationObservation { host, entry } => {
                self.reduce_backend_presentation_observation(ordinal, host, entry)?;
            }
            BackendIngressPayload::PointerSegment(segment) => {
                let provider = self
                    .backend_ingress
                    .expect("validated backend ingress remains frozen")
                    .pointer_provider();
                self.submit_pointer_journal_inner(
                    provider,
                    segment,
                    PointerJournalSubmissionAuthority::BackendIngress,
                    None,
                )?;
                return Ok(BackendIngressProgress::ReceiverReceiptsRequired);
            }
        }
        self.complete_backend_ingress_record()
    }

    fn reduce_backend_presentation_observation(
        &mut self,
        _ordinal: BackendIngressOrdinal,
        host: PresentationHostLease,
        entry: HostPresentationObservationEntry,
    ) -> Result<(), CoreHostFrameError> {
        let expected_host = self
            .backend_ingress
            .expect("validated backend ingress remains frozen")
            .presentation_host();
        if host != expected_host {
            return self.reject(CoreHostFrameError::BackendIngressRejected {
                source: BackendIngressError::PresentationHostLeaseMismatch {
                    expected: expected_host,
                    submitted: host,
                },
            });
        }
        if self.configuration_phase_started {
            return self.reject(CoreHostFrameError::SemanticAfterConfiguration);
        }
        if self.presentation_phase_started {
            return self.reject(CoreHostFrameError::InputAfterPresentation);
        }
        if self.pending_pointer_segment.is_some() {
            return self.reject(CoreHostFrameError::PointerReceiverReceiptsMissingBeforeInput);
        }

        let stamp = CoreCausalStamp(self.next_causal_ordinal);
        let Some(next_causal_ordinal) = self.next_causal_ordinal.checked_add(1) else {
            return self.reject(CoreHostFrameError::CausalOrdinalExhausted);
        };
        self.next_causal_ordinal = next_causal_ordinal;
        let cause = ReductionCause::PresentationObservation {
            tick: self.tick,
            ordinal: stamp.ordinal(),
            host,
            stream: entry.stream(),
        };

        let reduction = self
            .candidate
            .reduce_presentation_observation_entry(cause, host, entry);
        let (mut outcomes, changed_surfaces, mut lifecycle_events) = match reduction {
            Ok(reduction) => reduction,
            Err(error) => {
                return self.reject_input_prefix(error);
            }
        };

        self.events.append(&mut lifecycle_events);
        let pending_settled = match self
            .candidate
            .settle_presented_pending_mutation(&mut self.events, &mut self.interaction_events)
        {
            Ok(settled) => settled,
            Err(error) => {
                return self.reject_input_prefix(error);
            }
        };
        if pending_settled
            && let Err(error) = self
                .candidate
                .rebuild_presentation_requirements(self.candidate.last_input)
        {
            return self.reject_input_prefix(error);
        }
        if let Err(error) = self
            .candidate
            .reconcile_interaction_after_ordered_presentation_observation(
                cause,
                &changed_surfaces,
                &mut self.interaction_events,
            )
        {
            return self.reject_input_prefix(error);
        }
        self.presentation_observation_outcomes.append(&mut outcomes);
        self.last_causal_cause = Some(cause);
        if let Err(error) = self.refresh_presentation_snapshot() {
            return self.reject_input_prefix(error);
        }
        self.refresh_presented_interaction_authority();
        self.application_base = self.candidate.version;
        Ok(())
    }

    /// Returns the sole pointer-provider incarnation frozen at seal. `None`
    /// means this frame uses no journal-based pointer protocol.
    #[must_use]
    pub const fn pointer_provider(&self) -> Option<PointerInputLease> {
        self.pointer_provider
    }

    /// Stages one contiguous desktop-global pointer journal segment and
    /// freezes the exact receiver candidate roster adapters must answer.
    ///
    /// The journal is validated against a frame-local speculative ledger. Its
    /// watermark and edge tickets are not published until `finish` has also
    /// validated every exact receiver receipt against the outputs current at
    /// this segment's causal reduction position. Callers may submit another
    /// segment after staging this segment's receipts, with semantic inputs in
    /// between; the core assigns each edge-bearing segment one ordinal in that
    /// shared lane. An empty provider checkpoint advances only provider-owned
    /// authority and consumes no synthetic raw-event ordinal.
    ///
    /// # Errors
    ///
    /// Returns a structural error and poisons this frame when the provider,
    /// watermark, edge scope, or candidate roster is invalid. A surface-local
    /// lease is rejected even when it was copied from a valid affine producer.
    #[doc(hidden)]
    #[cfg(test)]
    pub fn submit_pointer_journal(
        &mut self,
        provider: PointerInputLease,
        journal: PointerEdgeJournal,
    ) -> Result<(), CoreHostFrameError> {
        self.submit_pointer_journal_inner(
            provider,
            journal,
            PointerJournalSubmissionAuthority::RawLease,
            None,
        )
    }

    /// Stages one surface-local journal through its affine producer capability.
    ///
    /// Renderer adapters should prefer this boundary over transporting a copied
    /// lease directly. Consuming the producer into a drain receipt then makes
    /// additional surface-local submission impossible by construction.
    ///
    /// # Errors
    ///
    /// Returns an error when the producer, journal ordering, or complete
    /// frame checkpoint does not match this frame's frozen pointer authority.
    pub fn submit_surface_pointer_journal(
        &mut self,
        provider: &SurfaceLocalPointerProvider,
        journal: PointerEdgeJournal,
    ) -> Result<(), CoreHostFrameError> {
        self.submit_pointer_journal_inner(
            provider.lease(),
            journal,
            PointerJournalSubmissionAuthority::SurfaceLocalProducer,
            Some(provider),
        )
    }

    fn submit_pointer_journal_inner(
        &mut self,
        provider: PointerInputLease,
        journal: PointerEdgeJournal,
        authority: PointerJournalSubmissionAuthority,
        surface_provider: Option<&SurfaceLocalPointerProvider>,
    ) -> Result<(), CoreHostFrameError> {
        if let Some(error) = &self.poison {
            return Err(error.clone());
        }
        if self.input_terminated_for_overload {
            return self.reject(CoreHostFrameError::InputAfterOverloadCancellation);
        }
        if self.backend_ingress.is_some()
            && authority != PointerJournalSubmissionAuthority::BackendIngress
        {
            return self.reject(CoreHostFrameError::BackendIngressRequired);
        }
        if provider.scope().is_local()
            && authority != PointerJournalSubmissionAuthority::SurfaceLocalProducer
        {
            return self
                .reject(CoreHostFrameError::SurfaceLocalPointerProducerRequired { provider });
        }
        if self.configuration_phase_started {
            return self.reject(CoreHostFrameError::SemanticAfterConfiguration);
        }
        if self.presentation_phase_started {
            return self.reject(CoreHostFrameError::InputAfterPresentation);
        }
        let Some(expected) = self.pointer_provider else {
            return self.reject(CoreHostFrameError::PointerJournalUnexpected);
        };
        if provider != expected {
            return self.reject(CoreHostFrameError::PointerJournalProviderMismatch {
                expected,
                submitted: provider,
            });
        }
        if self.pending_pointer_segment.is_some() {
            return self
                .reject(CoreHostFrameError::PointerReceiverReceiptsMissingBeforeNextSegment);
        }
        if journal.edges().len() > 1 {
            return self.reject(CoreHostFrameError::PointerJournalMustBeEdgewise);
        }
        let mut pending_surface_commit = None;
        if authority == PointerJournalSubmissionAuthority::SurfaceLocalProducer {
            let surface_provider =
                surface_provider.expect("surface-local submission authority retains its producer");
            if let Some(commit) = self.surface_pointer_commit.as_mut() {
                if let Err(source) =
                    commit.extend(surface_provider, journal.previous(), journal.through())
                {
                    return self.reject(CoreHostFrameError::SurfaceLocalPointerProducerRejected {
                        source,
                    });
                }
            } else {
                pending_surface_commit = match surface_provider
                    .begin_frame_submission(journal.previous(), journal.through())
                {
                    Ok(commit) => Some(commit),
                    Err(source) => {
                        return self.reject(
                            CoreHostFrameError::SurfaceLocalPointerProducerRejected { source },
                        );
                    }
                };
            }
        }
        let prepared = match self
            .staged_pointer_journal
            .prepare_candidate(provider, journal.clone())
        {
            Ok(prepared) => prepared,
            Err(source) => {
                return self.reject(CoreHostFrameError::PointerJournalRejected { source });
            }
        };
        let commit = match self.staged_pointer_journal.commit_prepared(prepared) {
            Ok(commit) => commit,
            Err(source) => {
                return self.reject(CoreHostFrameError::PointerJournalRejected { source });
            }
        };
        debug_assert_eq!(journal.edges().len(), commit.accepted_edges().len());
        let mut specs = Vec::with_capacity(journal.edges().len());
        let mut handoff = None;
        for (edge, accepted) in journal.edges().iter().zip(commit.accepted_edges()) {
            handoff = self
                .candidate
                .prepare_native_handoff_projection(accepted.stream(), edge);
            let edge_view = handoff.map(|projection| projection.edge_view(edge));
            let effective = edge_view.as_ref().unwrap_or(edge);
            let spec = if let Some(projection) =
                handoff.filter(|projection| projection.upgrades_owner())
            {
                self.candidate
                    .native_handoff_candidate_spec(projection, edge)
            } else {
                pointer_receiver_candidate_spec(&self.candidate, effective, accepted.stream())
            };
            specs.push(spec.with_metadata(edge.metadata()));
        }
        let attempt = match self
            .pointer_receiver_attempt_issuer
            .as_ref()
            .expect("a live pointer provider retains its receiver attempt issuer")
            .issue(provider)
        {
            Ok(attempt) => attempt,
            Err(source) => {
                return self.reject(CoreHostFrameError::PointerReceiverAttempt { source });
            }
        };
        let candidates = match PointerReceiverCandidateRoster::freeze_referenced_outputs(
            attempt,
            specs,
            &self.frozen_pointer_outputs,
        ) {
            Ok(candidates) => candidates,
            Err(source) => {
                return self.reject(CoreHostFrameError::PointerReceiverRoster { source });
            }
        };
        let stamp = CoreCausalStamp(self.next_causal_ordinal);
        if !journal.edges().is_empty() {
            let Some(next_causal_ordinal) = self.next_causal_ordinal.checked_add(1) else {
                return self.reject(CoreHostFrameError::CausalOrdinalExhausted);
            };
            self.next_causal_ordinal = next_causal_ordinal;
        }
        self.pending_pointer_segment = Some(HostPointerProtocolSegment::new(
            stamp, provider, journal, candidates, handoff,
        ));
        if let Some(commit) = pending_surface_commit {
            self.surface_pointer_commit = Some(commit);
        }
        self.pointer_segment_submitted = true;
        Ok(())
    }

    /// Returns the exact candidate roster for the most recently staged segment.
    ///
    /// The returned IDs are opaque and are valid only for this one host-frame
    /// attempt. An adapter must echo every candidate once in its receipt batch.
    #[must_use]
    pub fn pointer_receiver_candidates(&self) -> Option<&PointerReceiverCandidateRoster> {
        self.pending_pointer_segment
            .as_ref()
            .map(HostPointerProtocolSegment::candidates)
    }

    /// Stages the complete exact receiver receipt batch for the most recently
    /// frozen pointer segment.
    ///
    /// Presentation observations have already reduced before this method is
    /// available. Final output-authority validation still occurs in `finish`
    /// at the segment's causal position, so a subsequently retired output
    /// fails the whole frame.
    pub fn submit_pointer_receiver_receipts(
        &mut self,
        receipts: PointerReceiverReceiptBatch,
    ) -> Result<(), CoreHostFrameError> {
        self.submit_pointer_receiver_receipts_inner(receipts, false)
    }

    fn submit_pointer_receiver_receipts_inner(
        &mut self,
        receipts: PointerReceiverReceiptBatch,
        from_backend_ingress: bool,
    ) -> Result<(), CoreHostFrameError> {
        if let Some(error) = &self.poison {
            return Err(error.clone());
        }
        if self.backend_ingress.is_some() && !from_backend_ingress {
            return self.reject(CoreHostFrameError::BackendIngressRequired);
        }
        if self.pointer_provider.is_none() {
            return self.reject(CoreHostFrameError::PointerReceiverReceiptsUnexpected);
        }
        if self.presentation_phase_started {
            return self.reject(CoreHostFrameError::InputAfterPresentation);
        }
        let Some(segment) = self.pending_pointer_segment.take() else {
            return self.reject(CoreHostFrameError::PointerReceiverReceiptBeforeJournal);
        };
        let presentation_before = PointerPresentationFence::capture(&self.candidate);
        let protocol = segment.into_prepared(receipts);
        let reduction = self.candidate.reduce_host_pointer_protocol(
            self.tick,
            protocol,
            &self.frozen_pointer_outputs,
            &self.frozen_pointer_presentations,
            &self.tick_policy,
            &mut self.events,
            &mut self.interaction_events,
        );
        let reduced = match reduction {
            Ok(reduced) => reduced,
            Err(error) => {
                return self.reject_input_prefix(error);
            }
        };
        let segment_cause = reduced
            .last()
            .map(crate::transition::ReducedPointerEdge::cause);
        if let Some(cause) = segment_cause
            && let Err(error) = self
                .candidate
                .reconcile_pointer_provider_scope(cause, &mut self.interaction_events)
        {
            return self.reject_input_prefix(error);
        }
        self.last_causal_cause = segment_cause.or(self.last_causal_cause);
        self.vacancy_ledger.observe_bindings(&self.candidate);
        if let Err(error) =
            self.refresh_presentation_after_pointer_segment(presentation_before, &reduced)
        {
            return self.reject_input_prefix(error);
        }
        self.reduced_pointer_edges.extend(reduced);
        Ok(())
    }

    /// Appends one semantic input in its actual provider/host arrival order.
    ///
    /// `source_sequence` advances the session-owned semantic writer watermark.
    /// `source` remains diagnostic provenance; it neither partitions replay
    /// authority nor reorders this frame.
    pub fn append_input(
        &mut self,
        source: StableInputSourceId,
        source_sequence: SourceSequence,
        input: EngineInput,
    ) -> Result<(), CoreHostFrameError> {
        self.append(
            HostFrameInputPhase::Semantic,
            source,
            source_sequence,
            input,
            false,
        )
    }

    /// Appends one explicit configuration input.
    ///
    /// Configuration closes semantic input and immediately updates this frame's
    /// private candidate. Measurements and paint use the resulting configuration;
    /// publication still waits for the complete frame to commit.
    pub fn append_configuration(
        &mut self,
        source: StableInputSourceId,
        source_sequence: SourceSequence,
        input: EngineInput,
    ) -> Result<(), CoreHostFrameError> {
        self.begin_configuration_phase()?;
        self.append(
            HostFrameInputPhase::Configuration,
            source,
            source_sequence,
            input,
            false,
        )
    }

    pub(crate) fn append_policy_replacement(
        &mut self,
        source: StableInputSourceId,
        source_sequence: SourceSequence,
        policy: DockPolicy,
    ) -> Result<(), CoreHostFrameError> {
        self.append_configuration(
            source,
            source_sequence,
            EngineInput::ReplacePolicy {
                expected: self.application_base,
                policy,
            },
        )
    }

    pub(crate) fn append_presentation_config_replacement(
        &mut self,
        source: StableInputSourceId,
        source_sequence: SourceSequence,
        config: DockPresentationConfig,
    ) -> Result<(), CoreHostFrameError> {
        self.append_configuration(
            source,
            source_sequence,
            EngineInput::ReplacePresentationConfig {
                expected: self.application_base,
                config,
            },
        )
    }

    /// Closes provider and semantic ingress before configuration begins.
    ///
    /// Calling this method is an irreversible type-state transition for this
    /// frame candidate: later semantic or pointer input is rejected. The
    /// configuration itself remains speculative until the complete host frame
    /// commits.
    pub fn begin_configuration_phase(&mut self) -> Result<(), CoreHostFrameError> {
        if let Some(error) = &self.poison {
            return Err(error.clone());
        }
        if self.input_terminated_for_overload {
            return self.reject(CoreHostFrameError::InputAfterOverloadCancellation);
        }
        if self.presentation_phase_started {
            return self.reject(CoreHostFrameError::InputAfterPresentation);
        }
        self.ensure_input_prefix_complete()?;
        self.configuration_phase_started = true;
        Ok(())
    }

    fn append_backend_input(
        &mut self,
        ordinal: BackendIngressOrdinal,
        input: EngineInput,
    ) -> Result<(), CoreHostFrameError> {
        debug_assert!(input.is_backend_ingress_fact());
        self.append(
            HostFrameInputPhase::Semantic,
            BACKEND_INGRESS_INPUT_SOURCE,
            SourceSequence::new(ordinal.get()),
            input,
            true,
        )
    }

    fn append_backend_semantic_input(
        &mut self,
        ordinal: BackendIngressOrdinal,
        input: EngineInput,
    ) -> Result<(), CoreHostFrameError> {
        debug_assert!(!input.is_backend_ingress_fact());
        debug_assert!(!input.is_configuration_commit());
        self.append(
            HostFrameInputPhase::Semantic,
            BACKEND_INGRESS_INPUT_SOURCE,
            SourceSequence::new(ordinal.get()),
            input,
            true,
        )
    }

    fn append(
        &mut self,
        phase: HostFrameInputPhase,
        source: StableInputSourceId,
        source_sequence: SourceSequence,
        input: EngineInput,
        from_backend_ingress: bool,
    ) -> Result<(), CoreHostFrameError> {
        if let Some(error) = &self.poison {
            return Err(error.clone());
        }
        if let EngineInput::PublishHostFocusObservation { observation, .. } = &input
            && observation.host() != self.presentation_host
        {
            return self.reject(CoreHostFrameError::HostFocusOwnerMismatch {
                expected: self.presentation_host,
                submitted: observation.host(),
            });
        }
        if self.input_terminated_for_overload {
            return self.reject(CoreHostFrameError::InputAfterOverloadCancellation);
        }
        let overload_cancellation =
            if let EngineInput::CancelNativeInputForOverload { provider } = &input {
                self.ensure_backend_ingress_complete()?;
                let Some(expected) = self.backend_ingress else {
                    return self.reject(CoreHostFrameError::BackendIngressUnexpected);
                };
                if from_backend_ingress
                    || *provider != expected
                    || provider.presentation_host() != self.presentation_host
                {
                    return self.reject(CoreHostFrameError::BackendIngressRejected {
                        source: BackendIngressError::ProviderLeaseMismatch {
                            expected,
                            submitted: *provider,
                        },
                    });
                }
                true
            } else {
                false
            };
        let native_input_reset = matches!(
            &input,
            EngineInput::ObserveNativeInputDeviceRemoval { .. }
                | EngineInput::ObserveNativeInputBindingRetired { .. }
        );
        if self.pending_pointer_segment.is_some() {
            return self.reject(CoreHostFrameError::PointerReceiverReceiptsMissingBeforeInput);
        }
        if matches!(phase, HostFrameInputPhase::Configuration) {
            self.ensure_backend_ingress_complete()?;
        }
        if source == BACKEND_INGRESS_INPUT_SOURCE && !from_backend_ingress {
            return self.reject(CoreHostFrameError::ReservedInputSource {
                input_source: source,
            });
        }
        if self.backend_ingress.is_some()
            && input.is_backend_ingress_fact()
            && !from_backend_ingress
        {
            return self.reject(CoreHostFrameError::BackendIngressRequired);
        }
        let configuration = input.is_configuration_commit();
        if configuration != matches!(phase, HostFrameInputPhase::Configuration) {
            return self.reject(CoreHostFrameError::InputPhaseMismatch {
                phase,
                actual: input.priority(),
            });
        }
        if matches!(phase, HostFrameInputPhase::Semantic) && self.configuration_phase_started {
            return self.reject(CoreHostFrameError::SemanticAfterConfiguration);
        }
        if matches!(phase, HostFrameInputPhase::Semantic) && self.presentation_phase_started {
            return self.reject(CoreHostFrameError::InputAfterPresentation);
        }
        if let Some(item) = self.first_unadmitted_item(&input) {
            return self.reject(CoreHostFrameError::ItemIdentityOutsideScope { item });
        }
        let stamp = CoreCausalStamp(self.next_causal_ordinal);
        let Some(next_causal_ordinal) = self.next_causal_ordinal.checked_add(1) else {
            return self.reject(CoreHostFrameError::CausalOrdinalExhausted);
        };
        self.next_causal_ordinal = next_causal_ordinal;
        if !from_backend_ingress
            && let Err(error) = self
                .candidate
                .validate_and_advance_semantic_input_watermark([(source, source_sequence)])
        {
            return self.reject_input_prefix(error);
        }
        let Some(sequence) = self.candidate.last_input.checked_next() else {
            return self.reject_input_prefix(EngineError::InputSequenceExhausted);
        };
        self.candidate.last_input = sequence;
        let refresh_presented_interaction_authority = matches!(
            &input,
            EngineInput::SetNativeSurfacePlacements { .. }
                | EngineInput::RegisterViewport { .. }
                | EngineInput::BootstrapChildViewport { .. }
                | EngineInput::CreateNativeSurface { .. }
                | EngineInput::AdvanceNativeSurfaceCreate { .. }
                | EngineInput::PublishPlatformSnapshot { .. }
                | EngineInput::PublishNativeCloseObservation { .. }
                | EngineInput::PublishGlobalFocusObservation { .. }
                | EngineInput::ObserveNativeInputDeviceRemoval { .. }
                | EngineInput::ObserveNativeInputBindingRetired { .. }
                | EngineInput::ReplaceWorkspace(_)
                | EngineInput::ReplaceWorkspaceContinuingRoots { .. }
                | EngineInput::RestoreWorkspace(_)
        );
        let input = SequencedInput::new(stamp, sequence, source, source_sequence, input);
        let cause = input.cause(self.tick);
        let before_configuration_resize_surface = configuration
            .then(|| {
                self.candidate
                    .interaction
                    .active_resize_view()
                    .filter(|resize| !resize.updates().is_empty())
                    .map(|resize| resize.surface())
            })
            .flatten();
        let outcome = self.candidate.reduce_sequenced_input(
            input,
            self.tick,
            TickStartAuthority {
                policy: &self.tick_policy,
            },
            &mut self.application_base,
            &mut self.events,
            &mut self.interaction_events,
        );
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => {
                return self.reject_input_prefix(error);
            }
        };
        if let Err(error) = self
            .candidate
            .reconcile_pointer_provider_scope(cause, &mut self.interaction_events)
        {
            return self.reject_input_prefix(error);
        }
        if configuration {
            // Configuration may cancel an existing resize, but cannot edit its
            // weights or start a new one. Retain only the surface needed to
            // revoke that preview before compiling new-configuration geometry.
            let after_resize_surface = self
                .candidate
                .interaction
                .active_resize_view()
                .filter(|resize| !resize.updates().is_empty())
                .map(|resize| resize.surface());
            let changed_surfaces = if before_configuration_resize_surface == after_resize_surface {
                BTreeSet::new()
            } else {
                before_configuration_resize_surface
                    .into_iter()
                    .chain(after_resize_surface)
                    .collect()
            };
            let result = self
                .candidate
                .invalidate_uncovered_changed_resize_presentation(
                    &changed_surfaces,
                    sequence,
                    &BTreeSet::new(),
                )
                .and_then(|_| {
                    self.candidate
                        .cancel_revoked_presentation_caused(cause, &mut self.interaction_events)
                });
            if let Err(error) = result {
                return self.reject_input_prefix(error);
            }
        }
        self.vacancy_ledger
            .observe_workspace_reconciliation(outcome.outcome());
        self.vacancy_ledger.observe_bindings(&self.candidate);
        self.last_reduced_input = Some(sequence);
        self.last_causal_cause = Some(cause);
        self.reduced_inputs.push(outcome);
        if let Err(error) = self.candidate.rebuild_presentation_requirements(sequence) {
            return self.reject_input_prefix(error);
        }
        if let Err(error) = self.refresh_presentation_snapshot() {
            return self.reject_input_prefix(error);
        }
        if refresh_presented_interaction_authority {
            self.refresh_presented_interaction_authority();
        }
        if overload_cancellation {
            self.input_terminated_for_overload = true;
        }
        if overload_cancellation || native_input_reset {
            self.staged_pointer_journal = self.candidate.pointer_journal.clone();
        }
        Ok(())
    }

    fn first_unadmitted_item(&self, input: &EngineInput) -> Option<ItemId> {
        let scope = self.item_identity_scope.as_ref()?;
        match input {
            EngineInput::ReplaceWorkspace(workspace)
            | EngineInput::ReplaceWorkspaceContinuingRoots { workspace, .. } => workspace
                .item_multiset()
                .into_keys()
                .find(|item| !scope.contains(item)),
            EngineInput::RestoreWorkspace(restore) => restore
                .workspace()
                .item_multiset()
                .into_keys()
                .find(|item| !scope.contains(item)),
            EngineInput::WorkspaceCommand { command, .. } => {
                command.opened_item().filter(|item| !scope.contains(item))
            }
            EngineInput::OpenItem { item, .. } => (!scope.contains(item)).then_some(*item),
            _ => None,
        }
    }

    fn reject(&mut self, error: CoreHostFrameError) -> Result<(), CoreHostFrameError> {
        let retained = self.poison.get_or_insert(error).clone();
        Err(retained)
    }

    fn reject_input_prefix(&mut self, source: EngineError) -> Result<(), CoreHostFrameError> {
        self.reject(CoreHostFrameError::InputPrefixReductionFailed {
            source: Box::new(source),
        })
    }

    fn reject_value<T>(&mut self, error: CoreHostFrameError) -> Result<T, CoreHostFrameError> {
        let retained = self.poison.get_or_insert(error).clone();
        Err(retained)
    }

    /// Issues the complete affine physical-presentation roster for this frame.
    ///
    /// Every returned ticket must be consumed exactly once by the
    /// presentation capability or a paired retained contribution. Issuance
    /// closes semantic input for this host frame.
    pub(super) fn issue_presentation_obligations(
        &mut self,
    ) -> Result<Vec<HostPresentationObligation>, CoreHostFrameError> {
        if let Some(error) = &self.poison {
            return Err(error.clone());
        }
        self.ensure_backend_ingress_complete()?;
        self.presentation_phase_started = true;
        if let Err(error) = self
            .presentation_obligations
            .replace_unissued_roster(self.frozen_presentation_roster.clone())
        {
            return self.reject_value(error);
        }
        match self.presentation_obligations.issue() {
            Ok(obligations) => Ok(obligations),
            Err(error) => self.reject_value(error),
        }
    }

    /// Consumes one exact physical presentation ticket.
    ///
    /// `Painted` stages a core presentation emission. `Unavailable` explicitly
    /// settles the slot without creating presentation authority.
    pub(super) fn settle_presentation_obligation(
        &mut self,
        obligation: HostPresentationObligation,
        disposition: HostPresentationDisposition,
    ) -> Result<Option<HostPresentationEmissionRequest>, CoreHostFrameError> {
        if let Some(error) = &self.poison {
            return Err(error.clone());
        }
        let request = match disposition {
            HostPresentationDisposition::Painted(submitted) => {
                let frozen = match self.presentation_obligations.frozen_output(&obligation) {
                    Ok(frozen) => frozen,
                    Err(error) => return self.reject_value(error),
                };
                let expected = frozen.interaction();
                if submitted != expected {
                    return self.reject_value(
                        CoreHostFrameError::PresentationInteractionMismatch {
                            surface: obligation.slot().surface(),
                            expected,
                            submitted,
                        },
                    );
                }
                let (surface, endpoint, payload) = frozen.into_parts();
                Some(self.stage_presentation_payload(
                    surface,
                    endpoint,
                    payload,
                    obligation.attempt(),
                    obligation.ordinal(),
                )?)
            }
            HostPresentationDisposition::Unavailable(_) => {
                if let Err(error) = self.presentation_obligations.validate(&obligation) {
                    return self.reject_value(error);
                }
                None
            }
        };
        self.presentation_obligations
            .resolve(obligation, disposition);
        Ok(request)
    }

    fn stage_presentation_output(
        &mut self,
        surface: SurfaceId,
        frozen: FrozenSurfacePresentationOutput,
        obligation: &HostPresentationObligation,
    ) -> Result<HostPresentationEmissionRequest, CoreHostFrameError> {
        self.stage_presentation_payload(
            surface,
            frozen.endpoint(),
            frozen.payload(),
            obligation.attempt(),
            obligation.ordinal(),
        )
    }

    fn stage_presentation_payload(
        &mut self,
        surface: SurfaceId,
        endpoint: HostPresentationEndpoint,
        payload: HostPresentationOutputPayload,
        attempt: HostPresentationAttemptId,
        ordinal: u64,
    ) -> Result<HostPresentationEmissionRequest, CoreHostFrameError> {
        self.ensure_backend_ingress_complete()?;
        self.presentation_phase_started = true;
        if !self.staged_presentation_output_surfaces.insert(surface) {
            return self.reject_value(CoreHostFrameError::DuplicatePresentationOutput { surface });
        }
        let request = HostPresentationEmissionRequest::new(
            self.presentation_host,
            self.predecessor_tick,
            attempt,
            ordinal,
        );
        let scene = if let HostPresentationOutputPayload::Paint { scene: ticket, .. } = payload {
            let Some(scene) = self.candidate.scene().emitted_scene(ticket) else {
                return self
                    .reject_value(CoreHostFrameError::PresentationOutputUnavailable { surface });
            };
            Some(Arc::new(scene.clone()))
        } else {
            None
        };
        self.staged_presentation_outputs
            .push(StagedPresentationOutput {
                request,
                surface,
                endpoint,
                payload,
                scene,
            });
        Ok(request)
    }

    /// Records one actual paint of the frozen Ready candidate and contributes
    /// that exact same candidate without recomputing measurements.
    ///
    /// This is the only retained-contribution path. Pairing the contribution
    /// with a frame-local output request prevents a semantic ticket observed
    /// before paint from being misrepresented as an actual host output. The
    /// request becomes a concrete core emission only after the prepared candidate commits
    /// commits the complete host frame.
    ///
    /// # Errors
    ///
    /// Returns [`CoreHostFrameError`] when the token is not for the exact
    /// frozen Ready candidate or when another contribution already answers the
    /// surface slot. Any such error poisons the complete host frame.
    pub(super) fn record_painted_surface_contribution(
        &mut self,
        obligation: HostPresentationObligation,
        token: SurfaceContributionToken,
        interaction: HostInteractionPresentation,
    ) -> Result<HostPresentationEmissionRequest, CoreHostFrameError> {
        if let Some(error) = &self.poison {
            return Err(error.clone());
        }
        let surface = token.surface();
        if obligation.slot() != (HostPresentationSlot::Surface { surface }) {
            return self.reject_value(CoreHostFrameError::PresentationObligationSurfaceMismatch {
                slot: obligation.slot(),
                surface,
            });
        }
        if !self.frozen_presentation_roster.contains_surface(surface) {
            return self.reject_value(CoreHostFrameError::SurfaceOutsideRoster { surface });
        }
        let frozen = match self.presentation_obligations.frozen_output(&obligation) {
            Ok(frozen) => frozen,
            Err(error) => return self.reject_value(error),
        };
        let HostPresentationOutputPayload::Paint { scene: ticket, .. } = frozen.payload() else {
            return self
                .reject_value(CoreHostFrameError::RetainedContributionNotPaintable { surface });
        };
        let expected_interaction = frozen.payload().interaction().unwrap_or_default();
        if interaction != expected_interaction {
            return self.reject_value(CoreHostFrameError::PresentationInteractionMismatch {
                surface,
                expected: expected_interaction,
                submitted: interaction,
            });
        }
        if token.base() != frozen.stamp() {
            return self.reject_value(CoreHostFrameError::RetainedContributionStampMismatch {
                surface,
                submitted: token.base(),
                expected: frozen.stamp(),
            });
        }
        if !token
            .coordinates
            .same_projection_authority(frozen.coordinates())
        {
            return self.reject_value(CoreHostFrameError::RetainedContributionCoordinateMismatch {
                surface,
            });
        }

        let request = self.stage_presentation_output(surface, frozen, &obligation)?;
        let contribution = PreparedSurfaceContribution {
            token,
            #[cfg(feature = "motion")]
            next_motion: None,
            policy_revision: token.ticket().policy(),
            state: PreparedSurfaceContributionState::Retained { ticket },
        };
        self.push_surface_contribution(contribution)?;
        self.presentation_obligations.resolve(
            obligation,
            HostPresentationDisposition::Painted(interaction),
        );
        Ok(request)
    }

    /// Adds one surface contribution reduced after the complete input prefix.
    ///
    /// Contributions are retained in canonical [`crate::ids::SurfaceId`] order,
    /// independent of adapter callback arrival order. A tick may include at most
    /// one contribution for each logical surface.
    ///
    /// # Errors
    ///
    /// Returns [`CoreHostFrameError::DuplicateSurfaceContribution`] without
    /// replacing the already-installed contribution for that surface.
    pub fn push_surface_contribution(
        &mut self,
        contribution: PreparedSurfaceContribution,
    ) -> Result<(), CoreHostFrameError> {
        if let Some(error) = &self.poison {
            return Err(error.clone());
        }
        self.ensure_backend_ingress_complete()?;
        let surface = contribution.surface();
        if self.compiled_surface_outcomes.contains_key(&surface)
            && !matches!(&contribution.state, PreparedSurfaceContributionState::Retained { ticket }
                if self.candidate.scene().ready_candidate(surface).is_some_and(|candidate| candidate.output_ticket() == *ticket))
        {
            return self.reject(CoreHostFrameError::DuplicateSurfaceContribution { surface });
        }
        if !self.frozen_presentation_roster.contains_surface(surface) {
            return self.reject(CoreHostFrameError::SurfaceOutsideRoster { surface });
        }
        self.presentation_phase_started = true;
        let std::collections::btree_map::Entry::Vacant(slot) =
            self.surface_contributions.entry(surface)
        else {
            let error = CoreHostFrameError::DuplicateSurfaceContribution { surface };
            let retained = self.poison.get_or_insert(error).clone();
            return Err(retained);
        };
        slot.insert(contribution);
        Ok(())
    }

    /// Compiles and installs measured geometry in the private update, before encoding.
    /// Publication still happens only when the complete host frame commits.
    pub(crate) fn compile_surface_contribution(
        &mut self,
        contribution: PreparedSurfaceContribution,
    ) -> Result<(), CoreHostFrameError> {
        self.ensure_input_prefix_complete()?;
        self.presentation_phase_started = true;
        let surface = contribution.surface();
        if self.compiled_surface_outcomes.contains_key(&surface)
            || self.surface_contributions.contains_key(&surface)
        {
            return self.reject(CoreHostFrameError::DuplicateSurfaceContribution { surface });
        }
        let outcome = match self.candidate.reduce_surface_contribution(
            self.tick,
            contribution,
            self.candidate.policy.revision(),
        ) {
            Ok(outcome) => outcome,
            Err(error) => {
                return self.reject_input_prefix(error);
            }
        };
        self.compiled_surface_outcomes.insert(surface, outcome);
        if let Err(error) = self.refresh_presentation_snapshot() {
            return self.reject_input_prefix(error);
        }
        Ok(())
    }

    pub(crate) fn surface_is_measured(&self, surface: SurfaceId) -> bool {
        self.compiled_surface_outcomes.contains_key(&surface)
    }

    pub(crate) fn output_admission_facts(
        &self,
        surface: SurfaceId,
    ) -> (
        &[HostPresentationObservationOutcome],
        crate::presentation_observation::PresentationStreamAdmission,
    ) {
        let endpoint = self
            .candidate
            .scene()
            .ready_candidate(surface)
            .map(|candidate| presentation_endpoint_from_capture(candidate.coordinate_capture()));
        (
            &self.presentation_observation_outcomes,
            endpoint.map_or(
                crate::presentation_observation::PresentationStreamAdmission::New,
                |endpoint| {
                    self.candidate
                        .presentation_authority
                        .presentation
                        .stream_admission(self.presentation_host, surface, endpoint)
                },
            ),
        )
    }

    fn ensure_backend_ingress_complete(&mut self) -> Result<(), CoreHostFrameError> {
        if self.backend_ingress.is_some()
            && (!self.backend_ingress_batch_submitted
                || !self.backend_ingress_complete
                || self.pending_backend_ingress.is_some())
        {
            return self.reject(CoreHostFrameError::BackendIngressIncomplete);
        }
        Ok(())
    }

    fn ensure_input_prefix_complete(&mut self) -> Result<(), CoreHostFrameError> {
        self.ensure_backend_ingress_complete()?;
        if let Some(provider) = self.pointer_provider {
            if self.backend_ingress.is_none() && !self.pointer_segment_submitted {
                return self.reject_value(
                    CoreHostFrameError::PointerJournalMissingBeforePresentation { provider },
                );
            }
            if self.pending_pointer_segment.is_some() {
                return self.reject_value(
                    CoreHostFrameError::PointerReceiverReceiptsMissingBeforePresentation,
                );
            }
        }
        Ok(())
    }

    /// Returns all surface contributions in canonical surface order.
    #[must_use]
    pub fn surface_contributions(
        &self,
    ) -> impl ExactSizeIterator<Item = &PreparedSurfaceContribution> {
        self.surface_contributions.values()
    }

    /// Closes this capability's semantic input prefix in place.
    pub(crate) fn begin_presentation_phase(&mut self) -> Result<(), CoreHostFrameError> {
        self.ensure_input_prefix_complete()?;
        self.refresh_presentation_layout()?;
        // Semantic input in this same frame may have retired the frozen
        // provider after its final segment was reduced. The reducer owns that
        // transition; presentation only requires the frozen segment and its
        // receipts to be complete.
        self.presentation_phase_started = true;
        Ok(())
    }

    /// Closes the semantic input prefix and returns a presentation-only capability.
    pub fn into_presentation(mut self) -> Result<CoreHostPresentationFrame, CoreHostFrameError> {
        self.begin_presentation_phase()?;
        if !self.presentation_obligations.is_issued() {
            if let Err(error) = self
                .presentation_obligations
                .replace_unissued_roster(self.frozen_presentation_roster.clone())
            {
                return self.reject_value(error);
            }
        }
        Ok(CoreHostPresentationFrame { frame: self })
    }

    /// Prepares this capability without publishing the candidate engine state.
    ///
    /// The returned commit capability keeps the original engine exclusively
    /// borrowed. Dropping it rolls back the complete frame. Commit still
    /// validates the affine surface-local producer attempt before publishing.
    #[cfg(test)]
    pub fn prepare<'a>(
        self,
        engine: &'a mut DockEngine,
    ) -> Result<PreparedHostFrameCommit<'a>, EngineError> {
        engine.prepare_host_frame(self)
    }

    /// Prepares this capability without borrowing its destination engine.
    pub fn prepare_owned(
        self,
        engine: &DockEngine,
    ) -> Result<OwnedPreparedHostFrameCommit, EngineError> {
        engine.prepare_host_frame_owned(self)
    }

    /// Atomically reduces and publishes this capability through its engine.
    #[cfg(test)]
    pub fn finish(self, engine: &mut DockEngine) -> Result<EngineTransition, EngineError> {
        self.prepare(engine)?.commit()
    }
}

impl CoreHostPresentationFrame {
    pub(crate) fn with_platform_effect_delivery_limit(mut self, limit: Option<usize>) -> Self {
        self.frame.platform_effect_delivery_limit = limit;
        self
    }
    #[cfg(feature = "serde")]
    pub(crate) fn item_identity_scope_matches(&self, expected: &BTreeSet<ItemId>) -> bool {
        self.frame.item_identity_scope.as_deref() == Some(expected)
    }

    /// Returns the exact post-input presentation authority.
    #[must_use]
    #[cfg(test)]
    pub const fn view(&self) -> HostFrameView<'_> {
        self.frame.view()
    }

    /// Returns the complete post-input logical surface roster.
    #[must_use]
    #[cfg(test)]
    pub fn surfaces(&self) -> impl ExactSizeIterator<Item = SurfaceId> + '_ {
        self.frame.surfaces()
    }

    /// Returns whether the input prefix changed any presentation fact.
    #[must_use]
    #[cfg(test)]
    pub const fn presentation_changed_since_seal(&self) -> bool {
        self.frame.presentation_changed_since_seal()
    }

    /// Returns whether input changed geometry or surface-roster authority.
    #[must_use]
    #[cfg(test)]
    pub const fn presentation_projection_changed_since_seal(&self) -> bool {
        self.frame.presentation_projection_changed_since_seal()
    }

    /// Issues every post-input physical presentation obligation exactly once.
    pub fn take_presentation_obligations(
        &mut self,
    ) -> Result<Vec<HostPresentationObligation>, CoreHostFrameError> {
        self.frame.issue_presentation_obligations()
    }

    /// Consumes one post-input physical presentation obligation.
    pub fn resolve_presentation_obligation(
        &mut self,
        obligation: HostPresentationObligation,
        disposition: HostPresentationDisposition,
    ) -> Result<Option<HostPresentationEmissionRequest>, CoreHostFrameError> {
        self.frame
            .settle_presentation_obligation(obligation, disposition)
    }

    /// Explicitly declares every unissued physical output unavailable.
    ///
    /// This is an atomic convenience for hosts which know that this entire
    /// presentation pass cannot produce authoritative output. It never
    /// recovers partially checked-out tickets.
    #[cfg(test)]
    pub fn resolve_all_presentation_obligations_unavailable(
        &mut self,
        reason: HostPresentationUnavailableReason,
    ) -> Result<(), CoreHostFrameError> {
        if let Some(error) = &self.frame.poison {
            return Err(error.clone());
        }
        match self
            .frame
            .presentation_obligations
            .resolve_all_unissued(HostPresentationDisposition::Unavailable(reason))
        {
            Ok(()) => Ok(()),
            Err(error) => self.frame.reject(error),
        }
    }

    /// Pairs one actual post-input paint with its exact retained contribution.
    pub fn record_painted_surface_contribution(
        &mut self,
        obligation: HostPresentationObligation,
        token: SurfaceContributionToken,
        interaction: HostInteractionPresentation,
    ) -> Result<HostPresentationEmissionRequest, CoreHostFrameError> {
        self.frame
            .record_painted_surface_contribution(obligation, token, interaction)
    }

    /// Adds one post-input surface contribution.
    #[cfg(test)]
    pub fn push_surface_contribution(
        &mut self,
        contribution: PreparedSurfaceContribution,
    ) -> Result<(), CoreHostFrameError> {
        self.frame.push_surface_contribution(contribution)
    }

    /// Returns contributions staged in canonical surface order.
    #[must_use]
    #[cfg(test)]
    pub fn surface_contributions(
        &self,
    ) -> impl ExactSizeIterator<Item = &PreparedSurfaceContribution> {
        self.frame.surface_contributions()
    }

    /// Prepares the complete candidate without publishing it.
    #[cfg(test)]
    pub fn prepare<'a>(
        self,
        engine: &'a mut DockEngine,
    ) -> Result<PreparedHostFrameCommit<'a>, EngineError> {
        self.frame.prepare(engine)
    }

    /// Prepares the complete candidate without borrowing its destination engine.
    ///
    /// The returned affine capability may cross an enclosing host transaction's
    /// seal boundary. Its later commit revalidates the engine fence captured here.
    pub fn prepare_owned(
        self,
        engine: &DockEngine,
    ) -> Result<OwnedPreparedHostFrameCommit, EngineError> {
        self.frame.prepare_owned(engine)
    }

    /// Atomically publishes the complete candidate.
    #[cfg(test)]
    pub fn finish(self, engine: &mut DockEngine) -> Result<EngineTransition, EngineError> {
        self.frame.finish(engine)
    }
}

#[cfg(test)]
impl PreparedHostFrameCommit<'_> {
    /// Returns the exact transition which will be published on commit.
    #[must_use]
    pub const fn transition(&self) -> &EngineTransition {
        &self.transition
    }

    /// Publishes the already validated candidate.
    ///
    /// # Errors
    ///
    /// Returns a typed producer error without changing the destination engine
    /// if the affine surface-local frame attempt is no longer exact.
    pub fn commit(self) -> Result<EngineTransition, EngineError> {
        let Self {
            engine,
            candidate,
            transition,
            backend_ingress_commit_guard,
            surface_pointer_commit,
        } = self;
        match backend_ingress_commit_guard {
            Some(guard) => guard
                .publish_with(|| {
                    publish_prepared_host_candidate(
                        engine,
                        candidate,
                        transition,
                        surface_pointer_commit,
                    )
                })
                .map_err(|source| EngineError::BackendIngress { source })?,
            None => publish_prepared_host_candidate(
                engine,
                candidate,
                transition,
                surface_pointer_commit,
            ),
        }
    }
}

impl OwnedPreparedHostFrameCommit {
    /// Borrows the final unpublished candidate for runtime storage admission.
    pub(crate) const fn candidate_engine(&self) -> &DockEngine {
        &self.candidate
    }

    pub(crate) fn has_new_undelivered_platform_effects(&self, engine: &DockEngine) -> bool {
        self.candidate
            .viewport
            .effects()
            .has_undelivered_request_after(engine.viewport.latest_effect_id())
    }
    /// Returns the exact transition which will be published on commit.
    #[must_use]
    pub const fn transition(&self) -> &EngineTransition {
        &self.transition
    }

    #[cfg(feature = "serde")]
    pub(crate) fn candidate_workspace(&self) -> &Workspace {
        self.candidate.workspace()
    }

    #[cfg(feature = "serde")]
    pub(crate) const fn candidate_presentation_identity_frontier(
        &self,
    ) -> PresentationIdentityFrontier {
        self.candidate.presentation_identity_frontier()
    }

    /// Publishes the candidate if its destination still has the frozen authority.
    ///
    /// # Errors
    ///
    /// Returns the same typed staleness errors as ordinary host-frame preparation
    /// when any reducer, provider, or presentation frontier changed after prepare.
    pub fn commit(self, engine: &mut DockEngine) -> Result<EngineTransition, EngineError> {
        engine.validate_host_frame_commit_fence(self.fence)?;
        let Self {
            fence: _,
            candidate,
            transition,
            backend_ingress_commit_guard,
            surface_pointer_commit,
        } = self;
        match backend_ingress_commit_guard {
            Some(guard) => guard
                .publish_with(|| {
                    publish_prepared_host_candidate(
                        engine,
                        candidate,
                        transition,
                        surface_pointer_commit,
                    )
                })
                .map_err(|source| EngineError::BackendIngress { source })?,
            None => publish_prepared_host_candidate(
                engine,
                candidate,
                transition,
                surface_pointer_commit,
            ),
        }
    }
}

fn publish_prepared_host_candidate(
    engine: &mut DockEngine,
    mut candidate: DockEngine,
    transition: EngineTransition,
    surface_pointer_commit: Option<SurfaceLocalPointerFrameCommit>,
) -> Result<EngineTransition, EngineError> {
    candidate.mark_runtime_boundary_published();
    if let Some(mut commit) = surface_pointer_commit {
        let previous = commit
            .publish_with(|| std::mem::replace(engine, candidate))
            .map_err(|source| EngineError::SurfaceLocalPointerCommit { source })?;
        drop(previous);
    } else {
        *engine = candidate;
    }
    Ok(transition)
}

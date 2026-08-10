//! Presentation requirements, scene authority, and output reconciliation.

use super::presentation_roster::HostPresentationObligationIssuer;
use super::*;

/// Sole owner of renderer-neutral presentation state for one engine domain.
///
/// Keeping the manifest, transient tab-strip state, compiled scene, and
/// presentation ledger together prevents callers from publishing one of these
/// authorities without advancing the others through the engine reducer.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct PresentationAuthorityState {
    /// Non-rollbackable physical-presentation attempt authority shared by all
    /// speculative candidates.
    host_obligation_issuer: Arc<HostPresentationObligationIssuer>,
    pub(super) presentation_config: DockPresentationConfig,
    pub(super) presentation_config_revision: PresentationConfigRevision,
    pub(super) presentation_requirements: SceneRequirementManifest,
    pub(super) tab_strip_states: TabStripStateStore,
    pub(super) surface_semantic_snapshots: BTreeMap<crate::ids::SurfaceId, SurfaceSemanticSnapshot>,
    /// Monotonic watermarks retained across removal and re-addition in one engine domain.
    pub(super) surface_requirement_revisions:
        BTreeMap<crate::ids::SurfaceId, SurfaceRequirementRevision>,
    pub(super) scene: SurfaceSceneSet,
    pub(super) presentation: PresentationLedger,
    pub(super) last_presentation_output_serial: PresentationOutputSerial,
}

impl PresentationAuthorityState {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        authority_domain: EngineAuthorityDomainId,
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
            host_obligation_issuer: Arc::new(HostPresentationObligationIssuer::new(
                authority_domain,
            )),
            presentation_config,
            presentation_config_revision,
            presentation_requirements,
            tab_strip_states,
            surface_semantic_snapshots,
            surface_requirement_revisions,
            scene,
            presentation,
            last_presentation_output_serial: PresentationOutputSerial::default(),
        }
    }

    pub(super) fn issue_host_presentation_attempt(
        &self,
    ) -> Result<HostPresentationAttemptId, CoreHostFrameError> {
        self.host_obligation_issuer.issue()
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
    #[error("surface {surface} requirement revision is exhausted")]
    SurfaceRequirementRevisionExhausted { surface: crate::ids::SurfaceId },
    #[error("presentation requirement invariant failed: {0}")]
    Invariant(String),
    #[error(transparent)]
    Scene(#[from] SceneBuildError),
}

impl DockEngine {
    pub(super) fn advance_revision(&mut self, input: InputSequence) -> Result<(), EngineError> {
        let revision = self
            .version
            .revision()
            .checked_next()
            .ok_or(EngineError::WorkspaceRevisionExhausted { input })?;
        self.version = WorkspaceVersion::new(self.version.epoch(), revision);
        self.close.invalidate_stale(self.close_authority());
        self.rebuild_presentation_requirements(input)?;
        Ok(())
    }

    pub(super) fn advance_revision_caused(
        &mut self,
        cause: ReductionCause,
    ) -> Result<(), EngineError> {
        self.advance_revision_caused_with_influence(cause, None)
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
        let revision = self.version.revision().checked_next().ok_or_else(|| {
            EngineError::PointerInteractionInvariant {
                cause,
                detail: "workspace revision is exhausted".to_owned(),
            }
        })?;
        self.version = WorkspaceVersion::new(self.version.epoch(), revision);
        self.close.invalidate_stale(self.close_authority());
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
        let surface_semantic_snapshots = Self::capture_surface_semantic_snapshots(&self.workspace)
            .map_err(|source| PresentationRequirementFailure::Invariant(source.to_string()))?;
        let previous_surface_roster = self
            .presentation_authority
            .presentation_requirements
            .surfaces()
            .map(|(surface, _)| surface)
            .collect::<BTreeSet<_>>();
        let current_surface_roster = self
            .workspace
            .surfaces()
            .map(|(surface, _)| surface)
            .collect::<BTreeSet<_>>();
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
        {
            return Ok(());
        }

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
        for (surface, _) in self.workspace.surfaces() {
            let semantics_changed = self
                .presentation_authority
                .surface_semantic_snapshots
                .get(&surface)
                != surface_semantic_snapshots.get(&surface);
            if semantics_changed {
                let revision = surface_requirement_revisions
                    .get(&surface)
                    .copied()
                    .unwrap_or_default()
                    .checked_next()
                    .ok_or(
                        PresentationRequirementFailure::SurfaceRequirementRevisionExhausted {
                            surface,
                        },
                    )?;
                surface_requirement_revisions.insert(surface, revision);
                revised_surfaces.insert(surface);
            }
        }
        let mut draft = derive_scene_requirement_draft(
            self.authority_domain,
            &self.workspace,
            self.version,
            self.presentation_authority.presentation_config_revision,
            &self.policy,
            requirement_revision,
            &surface_requirement_revisions,
        )
        .map_err(|source| PresentationRequirementFailure::Invariant(source.to_string()))?;
        let mut tab_strip_states = self.presentation_authority.tab_strip_states.clone();
        let tab_strip_delta = Self::reconcile_tab_strip_state_store(
            &self.workspace,
            &draft,
            &mut tab_strip_states,
            surface_roster_changed,
        )
        .map_err(|source| PresentationRequirementFailure::Invariant(source.to_string()))?;
        let popup_mismatch_influence = if popup_requirement_changed {
            TabStripInfluenceDomain::PopupRoster
        } else {
            TabStripInfluenceDomain::Local(BTreeSet::new())
        };
        let influence = requested_influence
            .merge(popup_mismatch_influence)
            .merge(tab_strip_delta.influence().clone());
        let affected_surfaces = match influence {
            TabStripInfluenceDomain::Local(surfaces) => surfaces,
            TabStripInfluenceDomain::PopupRoster => current_surface_roster.clone(),
        };
        let mut draft_revisions_changed = false;
        for surface in affected_surfaces {
            if !current_surface_roster.contains(&surface) || revised_surfaces.contains(&surface) {
                continue;
            }
            let revision = surface_requirement_revisions
                .get(&surface)
                .copied()
                .unwrap_or_default()
                .checked_next()
                .ok_or(
                    PresentationRequirementFailure::SurfaceRequirementRevisionExhausted { surface },
                )?;
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
        }
        let requirements = draft
            .finalize(tab_strip_states.popup_requirement())
            .map_err(|source| PresentationRequirementFailure::Invariant(source.to_string()))?;
        self.presentation_authority.presentation_requirements = requirements;
        self.presentation_authority.tab_strip_states = tab_strip_states;
        self.presentation_authority.surface_semantic_snapshots = surface_semantic_snapshots;
        self.presentation_authority.surface_requirement_revisions = surface_requirement_revisions;
        self.presentation_authority
            .scene
            .reconcile_manifest(&self.presentation_authority.presentation_requirements)
            .map_err(PresentationRequirementFailure::Scene)?;
        Ok(())
    }

    pub(super) fn reconcile_tab_strip_state_store(
        workspace: &Workspace,
        requirements: &SceneRequirementDraft,
        states: &mut TabStripStateStore,
        surface_roster_changed: bool,
    ) -> Result<TabStripStateDelta, TabStripStateError> {
        let live = requirements
            .surfaces()
            .flat_map(|(surface, requirements)| {
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

    fn input_requirement_failure(
        input: InputSequence,
        failure: PresentationRequirementFailure,
    ) -> EngineError {
        match failure {
            PresentationRequirementFailure::RequirementRevisionExhausted => {
                EngineError::PresentationRequirementRevisionExhausted { input }
            }
            PresentationRequirementFailure::SurfaceRequirementRevisionExhausted { surface } => {
                EngineError::SurfaceRequirementRevisionExhausted { input, surface }
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
    ) -> Result<BTreeMap<crate::ids::SurfaceId, SurfaceSemanticSnapshot>, crate::error::CommandError>
    {
        let mut snapshots = BTreeMap::new();
        for (surface, presentation) in workspace.surfaces() {
            let main = presentation
                .main_root
                .map(|root| workspace.root_fingerprint(root))
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
                    root: workspace.root_fingerprint(record.root)?,
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
                    // A framework-local response is already bound to the
                    // current Ready scene and has no native provider whose
                    // binding needs to be kept alive. Only journal-owned
                    // resizes participate in platform reconciliation.
                    if resize.authority.presented().is_some() {
                        self.insert_current_viewport_binding(
                            &mut dependencies.owner_bindings,
                            resize.surface,
                        );
                    }
                }
            }
            InteractionStatus::ContainedTransforming { session } => {
                if let Ok(transform) = self.interaction.active_contained_transform(session)
                    && transform.presentation.presented().is_some()
                {
                    self.insert_current_viewport_binding(
                        &mut dependencies.owner_bindings,
                        transform.surface,
                    );
                }
            }
        }
        dependencies
    }

    fn collect_armed_platform_dependencies(
        &self,
        session: crate::interaction::DragSessionId,
        dependencies: &mut PlatformInteractionDependencies,
    ) {
        let Ok(drag) = self.interaction.armed_drag(session) else {
            return;
        };
        if drag.presentation.presented().is_none() {
            return;
        }
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
        if drag.presentation.presented().is_none() {
            return;
        }
        if let Some(binding) = drag
            .owner
            .pointer_if_physical()
            .and_then(|pointer| self.viewport.drag_source(pointer))
        {
            dependencies.owner_bindings.insert(binding);
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
        if let Some(preview) = &drag.preview {
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
            && self.viewport.native_tear_off_capability().is_supported()
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
        previous_native: PlatformCapability,
        previous_routing: PlatformCapability,
        previous_release: PlatformCapability,
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
        end_routing |= owner_scene_authority_lost;
        target_authority_lost |= invalidated_scene_bindings
            .iter()
            .any(|binding| dependencies.target_bindings.contains(binding));
        let current_native = self.viewport.native_tear_off_capability();
        if dependencies.native && previous_native.is_supported() && !current_native.is_supported() {
            target_authority_lost = true;
        }
        if dependencies.native && transition.work_areas_changed() {
            target_authority_lost = true;
        }
        let current_routing = self.viewport.capabilities().cross_surface_routing();
        if dependencies.routed && previous_routing.is_supported() && !current_routing.is_supported()
        {
            target_authority_lost = true;
            end_routing = true;
        }
        let current_release = self.viewport.capabilities().authoritative_release();
        if dependencies.routed && previous_release.is_supported() && !current_release.is_supported()
        {
            target_authority_lost = true;
            end_routing = true;
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
        policy: &DockPolicySnapshot,
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
        if contribution.policy_revision != policy.revision() {
            return Ok(SurfaceContributionOutcome::Rejected {
                surface,
                reason: SurfaceContributionRejection::PolicyAuthorityChanged {
                    submitted: contribution.policy_revision,
                    current: policy.revision(),
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
                let resolved_tab_offsets = plan
                    .tab_bar_records()
                    .iter()
                    .map(|record| {
                        let id = *record.id();
                        let selected = plan
                            .pane_records()
                            .iter()
                            .find(|pane| {
                                let pane = pane.id();
                                pane.root == id.root && pane.tabs == id.tabs
                            })
                            .and_then(|pane| pane.selected());
                        (id, record.scroll_offset(), selected)
                    })
                    .collect::<Vec<_>>();
                let output_serial = self
                    .presentation_authority
                    .last_presentation_output_serial
                    .checked_next()
                    .ok_or(EngineError::PresentationOutputSerialExhausted)?;
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
                        let target_resource_affected =
                            drag.contained_offer
                                .is_some_and(|offer| surfaces.contains(&offer.anchor().surface()))
                                || drag.affordance.as_ref().is_some_and(|affordance| {
                                    surfaces.contains(&affordance.surface())
                                })
                                || drag.preview.as_ref().is_some_and(|preview| {
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
        presentation_host: PresentationHostLease,
        presentation_scope: &BTreeSet<HostPresentationStreamId>,
        observation: HostPresentationObservation,
    ) -> Result<
        (
            Vec<HostPresentationObservationOutcome>,
            BTreeSet<crate::ids::SurfaceId>,
            Vec<crate::frame::ViewportLifecycleAction>,
            Vec<crate::event::WorkspaceEvent>,
        ),
        EngineError,
    > {
        let reduction = self
            .presentation_authority
            .presentation
            .reduce_observation(presentation_host, presentation_scope, observation)
            .map_err(presentation_ledger_error)?;
        self.apply_presentation_observation_reduction(reduction)
    }

    pub(super) fn reduce_presentation_observation_entry(
        &mut self,
        presentation_host: PresentationHostLease,
        entry: HostPresentationObservationEntry,
    ) -> Result<
        (
            Vec<HostPresentationObservationOutcome>,
            BTreeSet<crate::ids::SurfaceId>,
            Vec<crate::frame::ViewportLifecycleAction>,
            Vec<crate::event::WorkspaceEvent>,
        ),
        EngineError,
    > {
        let reduction = self
            .presentation_authority
            .presentation
            .reduce_observation_entry(presentation_host, entry)
            .map_err(presentation_ledger_error)?;
        self.apply_presentation_observation_reduction(reduction)
    }

    fn apply_presentation_observation_reduction(
        &mut self,
        reduction: PresentationObservationReduction,
    ) -> Result<
        (
            Vec<HostPresentationObservationOutcome>,
            BTreeSet<crate::ids::SurfaceId>,
            Vec<crate::frame::ViewportLifecycleAction>,
            Vec<crate::event::WorkspaceEvent>,
        ),
        EngineError,
    > {
        let outcomes = reduction.outcomes().to_vec();
        let mut changed_surfaces = BTreeSet::new();
        let mut lifecycle_actions = Vec::new();
        let mut lifecycle_events = Vec::new();
        let invalidated_streams = outcomes
            .iter()
            .filter_map(|outcome| match outcome {
                HostPresentationObservationOutcome::CapturedUnknown { stream, .. }
                | HostPresentationObservationOutcome::Retired {
                    stream,
                    promotion_eligible: false,
                    ..
                } => Some(*stream),
                HostPresentationObservationOutcome::NoUpdate { .. }
                | HostPresentationObservationOutcome::Retired {
                    promotion_eligible: true,
                    ..
                }
                | HostPresentationObservationOutcome::Rejected { .. } => None,
            })
            .collect::<BTreeSet<_>>();
        changed_surfaces.extend(
            self.invalidate_interaction_authority_for_current_streams(&invalidated_streams),
        );
        for promotion in reduction.promotions() {
            if let HostPresentationOutputPayload::NativeStaging { presentation } =
                promotion.payload()
            {
                if promotion.endpoint() == HostPresentationEndpoint::Native(presentation.binding())
                    && let Some(action) = self
                        .viewport
                        .observe_native_staging_presentation(
                            PresentedNativeStagingPresentation::mint_observed(
                                presentation,
                                promotion.stream(),
                                promotion.key(),
                            ),
                        )
                        .map_err(|source| EngineError::Viewport {
                            input: self.last_input,
                            source,
                        })?
                {
                    lifecycle_actions.push(action);
                }
                continue;
            }
            let HostPresentationOutputPayload::Paint {
                scene: ticket,
                interaction,
                coordinate_generation,
            } = promotion.payload()
            else {
                continue;
            };
            let promotion_stream = BTreeSet::from([promotion.stream()]);
            let Ok(capture) = self
                .presentation_authority
                .scene
                .retained_output_capture(ticket)
            else {
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
            let authority = PresentedSurfaceAuthority::mint_observed(
                ticket,
                promotion.stream(),
                promotion.key(),
                promotion.endpoint(),
                coordinate_generation,
            );
            match self
                .presentation_authority
                .scene
                .accept_observed_authority(authority)
            {
                Ok(changed) => {
                    self.admit_first_live_native_output(ticket, capture, &mut lifecycle_events)?;
                    self.observe_pending_drag_release_presentation(
                        promotion.key(),
                        ticket.surface(),
                        interaction,
                    );
                    self.observe_pending_contained_transform_release_presentation(
                        promotion.key(),
                        ticket.surface(),
                        interaction,
                    );
                    if let Some(token) = interaction.drag_preview() {
                        self.interaction
                            .observe_presented_preview(ticket.surface(), token);
                    }
                    if let Some(token) = interaction.contained_transform_preview() {
                        self.interaction
                            .observe_presented_contained_transform_preview(ticket.surface(), token);
                    }
                    changed_surfaces.extend(changed);
                }
                Err(_) => {
                    changed_surfaces.extend(
                        self.invalidate_interaction_authority_for_current_streams(
                            &promotion_stream,
                        ),
                    );
                }
            }
        }
        self.observe_pending_release_retirements(&outcomes);
        Ok((
            outcomes,
            changed_surfaces,
            lifecycle_actions,
            lifecycle_events,
        ))
    }

    fn observe_pending_release_retirements(
        &mut self,
        outcomes: &[HostPresentationObservationOutcome],
    ) {
        if let Some(pending) = self.pending_drag_release.as_mut() {
            Self::retire_pending_release_outputs(
                &mut pending.presentation_outputs,
                pending.presented_output,
                &mut pending.presentation_failed,
                outcomes,
            );
        }
        if let Some(pending) = self.pending_contained_transform_release.as_mut() {
            Self::retire_pending_release_outputs(
                &mut pending.presentation_outputs,
                pending.presented_output,
                &mut pending.presentation_failed,
                outcomes,
            );
        }
    }

    pub(super) fn observe_pending_release_host_retirement(
        &mut self,
        retired_outputs: &BTreeSet<HostFrameKey>,
    ) {
        if let Some(pending) = self.pending_drag_release.as_mut() {
            Self::retire_pending_release_output_keys(
                &mut pending.presentation_outputs,
                pending.presented_output,
                &mut pending.presentation_failed,
                retired_outputs,
            );
        }
        if let Some(pending) = self.pending_contained_transform_release.as_mut() {
            Self::retire_pending_release_output_keys(
                &mut pending.presentation_outputs,
                pending.presented_output,
                &mut pending.presentation_failed,
                retired_outputs,
            );
        }
    }

    fn retire_pending_release_output_keys(
        outputs: &mut BTreeSet<HostFrameKey>,
        presented_output: Option<HostFrameKey>,
        presentation_failed: &mut bool,
        retired_outputs: &BTreeSet<HostFrameKey>,
    ) {
        if presented_output.is_some() || outputs.is_empty() {
            return;
        }

        let before = outputs.len();
        outputs.retain(|output| !retired_outputs.contains(output));
        if outputs.len() < before && outputs.is_empty() {
            *presentation_failed = true;
        }
    }

    fn retire_pending_release_outputs(
        outputs: &mut BTreeSet<HostFrameKey>,
        presented_output: Option<HostFrameKey>,
        presentation_failed: &mut bool,
        outcomes: &[HostPresentationObservationOutcome],
    ) {
        if presented_output.is_some() || outputs.is_empty() {
            return;
        }

        let mut retired_bound_output = false;
        for outcome in outcomes {
            let HostPresentationObservationOutcome::Retired {
                stream,
                settled_through,
                ..
            } = outcome
            else {
                continue;
            };
            outputs.retain(|output| {
                let retained = output.stream() != *stream || *output > *settled_through;
                retired_bound_output |= !retained;
                retained
            });
        }
        if retired_bound_output && outputs.is_empty() {
            *presentation_failed = true;
        }
    }

    fn observe_pending_drag_release_presentation(
        &mut self,
        output: HostFrameKey,
        surface: SurfaceId,
        interaction: HostInteractionPresentation,
    ) {
        let Some(pending) = self.pending_drag_release.as_mut() else {
            return;
        };
        let preview_surface = pending
            .drag
            .preview
            .as_ref()
            .expect("pending release retains its exact preview")
            .public()
            .visual()
            .surface();
        // A matching observed output can predate the release edge in the same
        // ordered ingress batch. Its token is core-minted and unique to this
        // preview, so requiring a post-release emission would reverse valid
        // presentation-before-release causality.
        if interaction.drag_preview() == Some(pending.preview) && surface == preview_surface {
            pending.presented_output = Some(output);
        }
    }

    fn observe_pending_contained_transform_release_presentation(
        &mut self,
        output: HostFrameKey,
        surface: SurfaceId,
        interaction: HostInteractionPresentation,
    ) {
        let Some(pending) = self.pending_contained_transform_release.as_mut() else {
            return;
        };
        let preview_surface = pending
            .transform
            .preview
            .as_ref()
            .expect("pending contained release retains its exact preview")
            .public()
            .surface();
        if interaction.contained_transform_preview() == Some(pending.preview)
            && surface == preview_surface
        {
            pending.presented_output = Some(output);
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
        if self
            .cancel_revoked_presentation_caused(cause, interaction_events)?
            .is_some()
        {
            return Ok(());
        }
        if changed_surfaces.is_empty() {
            return Ok(());
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
                let affected = self
                    .interaction
                    .active_drag(session)
                    .map_err(|source| Self::contribution_invariant(cause, format!("{source:?}")))?
                    .preview
                    .as_ref()
                    .is_some_and(|preview| {
                        changed_surfaces.contains(&preview.public().token().scene().surface())
                    });
                if affected {
                    let changed =
                        self.interaction
                            .clear_drag_feedback(session)
                            .map_err(|source| {
                                Self::contribution_invariant(cause, format!("{source:?}"))
                            })?;
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
                let local_owner = matches!(
                    transform.owner,
                    GestureOwner::LocalResponse { surface } if surface == transform.surface
                );
                let local_coordinates = transform.presentation.local_coordinates()
                    == Some(transform.coordinate_capture);
                let owner_current = self.workspace.presentation_for_root(transform.root)
                    == Some(crate::RootPresentationOwner::Contained {
                        surface: transform.surface,
                        floating: transform.floating,
                    });
                let current = self
                    .presentation_authority
                    .scene
                    .ready_candidate(transform.surface);
                let scene_coordinates = current.is_some_and(|current| {
                    current.coordinate_capture() == transform.coordinate_capture
                });
                let scene_bounds = current
                    .is_some_and(|current| current.plan().bounds() == transform.surface_bounds);
                let scene_record = current.is_some_and(|current| {
                    current
                        .plan()
                        .contained_record(transform.floating)
                        .is_some_and(|record| {
                            record.root() == transform.root
                                && record.minimum_size() == transform.minimum_size
                        })
                });
                let scene_current = scene_coordinates && scene_bounds && scene_record;
                let local_response_remains_valid =
                    local_owner && local_coordinates && owner_current && scene_current;
                let affected = changed_surfaces.contains(&transform.surface)
                    && transform.preview.is_some()
                    && !local_response_remains_valid;
                if affected {
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

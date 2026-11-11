use super::*;

impl ViewportFocusCoordinator {
    pub(super) fn record_current_ui_focus(
        &mut self,
        surface: SurfaceId,
        state: HostFocusState,
        native_guard: Option<ViewportBinding>,
    ) -> bool {
        if self.focus_observations.last_generation.is_some()
            && !self.focus_observations.current().is_some_and(|observation| {
                matches!(observation.focused, Authority::Known(GlobalFocusedWindow::Dock(binding))
                    if Some(binding) == native_guard.map(ViewportBinding::window))
            })
        {
            return false;
        }
        let owns_focus = matches!(state, HostFocusState::Pane(_) | HostFocusState::Workspace);
        if !owns_focus
            && self.current_ui_focus.is_some_and(|record| {
                record
                    .scope
                    .is_some_and(|(current, guard)| current != surface || guard != native_guard)
            })
        {
            return false;
        }
        self.current_ui_focus = Some(CurrentUiFocusRecord {
            focus: CurrentUiFocus::from_host(surface, state),
            scope: Some((surface, native_guard)),
            host_scope: None,
        });
        true
    }

    pub(super) fn record_platform_ui_focus(&mut self, focused: Option<GlobalFocusedWindow>) {
        let (focus, scope) = match focused {
            Some(GlobalFocusedWindow::Dock(window)) => {
                if self.current_ui_focus.is_some_and(|record| {
                    record.scope.is_some_and(|(_, binding)| {
                        binding.is_some_and(|binding| binding.window() == window)
                    })
                }) {
                    return;
                }
                (CurrentUiFocus::Unknown, None)
            }
            Some(GlobalFocusedWindow::Foreign) => (CurrentUiFocus::External, None),
            Some(GlobalFocusedWindow::None) => (CurrentUiFocus::None, None),
            None => (CurrentUiFocus::Unknown, None),
        };
        self.current_ui_focus = Some(CurrentUiFocusRecord {
            focus,
            scope,
            host_scope: None,
        });
    }

    pub(super) fn invalidate_current_ui_focus(&mut self) -> bool {
        let unknown = CurrentUiFocusRecord {
            focus: CurrentUiFocus::Unknown,
            scope: None,
            host_scope: None,
        };
        if self.current_ui_focus.is_none() || self.current_ui_focus == Some(unknown) {
            return false;
        }
        self.current_ui_focus = Some(unknown);
        true
    }

    pub(crate) fn bind_current_host_focus_scope(
        &mut self,
        surface: SurfaceId,
        native_guard: Option<ViewportBinding>,
        host: PresentationHostLease,
        stream: HostPresentationStreamId,
    ) {
        if let Some(current) = self.current_ui_focus.as_mut()
            && current.scope == Some((surface, native_guard))
        {
            current.host_scope = Some((host, stream));
        }
    }

    /// Records an independent UI focus fact within one exact, current host scope.
    ///
    /// This does not acknowledge a pending request or create native activation
    /// authority. A fact for another native window may update its restoration
    /// history, but cannot replace the provider's global focus owner.
    pub fn publish_host_focus_observation(
        &mut self,
        observation: HostFocusObservation,
        is_current_scope: impl FnOnce(HostFocusObservation) -> bool,
        native_guard: Option<ViewportBinding>,
        item_is_on_surface: impl FnOnce(SurfaceId, ItemId) -> bool,
    ) -> PaneFocusObservationTransition {
        if !is_current_scope(observation) {
            return PaneFocusObservationTransition::Rejected(
                PaneFocusObservationRejection::StaleHostScope,
            );
        }
        if native_guard.is_some_and(|binding| binding.surface() != observation.surface) {
            return PaneFocusObservationTransition::Rejected(
                PaneFocusObservationRejection::StaleBinding,
            );
        }
        if let HostFocusState::Pane(item) = observation.state
            && !item_is_on_surface(observation.surface, item)
        {
            return PaneFocusObservationTransition::Rejected(
                PaneFocusObservationRejection::ItemUnavailable { item },
            );
        }
        if let Some(current) = self.pane_observation_generation(observation.surface) {
            if observation.generation < current {
                return PaneFocusObservationTransition::Stale { current };
            }
            if observation.generation == current {
                return if self
                    .host_focus_observation_by_surface
                    .get(&observation.surface)
                    == Some(&observation)
                {
                    PaneFocusObservationTransition::Duplicate
                } else {
                    PaneFocusObservationTransition::EqualGenerationConflict {
                        generation: current,
                    }
                };
            }
        }
        match observation.state {
            HostFocusState::Pane(item) => {
                self.panel_focus_by_surface
                    .insert(observation.surface, PanelFocusRecord::Item(item));
            }
            HostFocusState::Workspace => {
                self.panel_focus_by_surface
                    .insert(observation.surface, PanelFocusRecord::None);
            }
            HostFocusState::External | HostFocusState::None | HostFocusState::Unknown => {}
        }
        if self.record_current_ui_focus(observation.surface, observation.state, native_guard) {
            self.bind_current_host_focus_scope(
                observation.surface,
                native_guard,
                observation.host,
                observation.stream,
            );
        }
        self.host_focus_observation_by_surface
            .insert(observation.surface, observation);
        PaneFocusObservationTransition::Applied {
            cleared_intent: None,
        }
    }

    pub(super) fn host_focus_generation_conflict(
        &self,
        surface: SurfaceId,
        generation: PaneFocusObservationGeneration,
    ) -> Option<PaneFocusObservationTransition> {
        let current = self
            .host_focus_observation_by_surface
            .get(&surface)?
            .generation;
        if generation < current {
            Some(PaneFocusObservationTransition::Stale { current })
        } else if generation == current {
            Some(PaneFocusObservationTransition::EqualGenerationConflict { generation })
        } else {
            None
        }
    }

    /// Records an exact pane-focus fact and consumes only its matching pending intent.
    pub fn publish_pane_focus_observation(
        &mut self,
        observation: PaneFocusObservation,
        is_current_binding: impl FnOnce(ViewportBinding) -> bool,
        item_is_on_surface: impl FnOnce(SurfaceId, ItemId) -> bool,
    ) -> PaneFocusObservationTransition {
        if !is_current_binding(observation.binding) {
            return PaneFocusObservationTransition::Rejected(
                PaneFocusObservationRejection::StaleBinding,
            );
        }
        if let PanelFocus::Item(item) = observation.focus
            && !item_is_on_surface(observation.binding.surface(), item)
        {
            return PaneFocusObservationTransition::Rejected(
                PaneFocusObservationRejection::ItemUnavailable { item },
            );
        }

        let surface = observation.binding.surface();
        if let Some(transition) =
            self.host_focus_generation_conflict(surface, observation.generation)
        {
            return transition;
        }
        if let Some(current) = self
            .pane_request_observation_by_surface
            .get(&surface)
            .copied()
        {
            let current_generation = current.observation.generation;
            if observation.generation < current_generation {
                return PaneFocusObservationTransition::Stale {
                    current: current_generation,
                };
            }
            if observation.generation == current_generation {
                return PaneFocusObservationTransition::EqualGenerationConflict {
                    generation: observation.generation,
                };
            }
        }
        if let Some(current) = self.pane_observation_by_surface.get(&surface).copied() {
            let current_generation = current.observation.generation;
            if observation.generation < current_generation {
                return PaneFocusObservationTransition::Stale {
                    current: current_generation,
                };
            }
            if observation.generation == current_generation {
                return if observation == current.observation {
                    PaneFocusObservationTransition::Duplicate
                } else {
                    PaneFocusObservationTransition::EqualGenerationConflict {
                        generation: observation.generation,
                    }
                };
            }
        }

        let cleared_intent = if let Some(intent_id) = observation.acknowledges {
            let Some(intent) = self.pending_pane_intent else {
                return PaneFocusObservationTransition::Rejected(
                    PaneFocusObservationRejection::UnknownIntent { intent: intent_id },
                );
            };
            if intent.id != intent_id {
                return PaneFocusObservationTransition::Rejected(
                    PaneFocusObservationRejection::UnknownIntent { intent: intent_id },
                );
            }
            if self.admitted_pane_intent != Some(intent_id) {
                return PaneFocusObservationTransition::Rejected(
                    PaneFocusObservationRejection::IntentNotPublished { intent: intent_id },
                );
            }
            if intent.native_guard() != Some(observation.binding) {
                return PaneFocusObservationTransition::Rejected(
                    PaneFocusObservationRejection::IntentBindingMismatch,
                );
            }
            if intent.focus() != observation.focus {
                return PaneFocusObservationTransition::Rejected(
                    PaneFocusObservationRejection::IntentFocusMismatch,
                );
            }
            if intent
                .pane_observation_baseline
                .is_some_and(|baseline| observation.generation <= baseline)
            {
                return PaneFocusObservationTransition::Rejected(
                    PaneFocusObservationRejection::ObservationPrecedesIntent,
                );
            }
            self.pending_pane_intent = None;
            self.clear_admitted_pane_intent(intent_id);
            Some(intent_id)
        } else {
            None
        };

        self.panel_focus_by_surface
            .insert(surface, PanelFocusRecord::from_focus(observation.focus));
        self.record_current_ui_focus(
            surface,
            match observation.focus {
                PanelFocus::Item(item) => HostFocusState::Pane(item),
                PanelFocus::None => HostFocusState::Workspace,
            },
            Some(observation.binding),
        );
        self.pane_observation_by_surface
            .insert(surface, PanelFocusObservationRecord { observation });
        PaneFocusObservationTransition::Applied { cleared_intent }
    }

    /// Resolves one exact request-bound pane-focus observation.
    pub fn publish_pane_focus_request_observation(
        &mut self,
        observation: PaneFocusRequestObservation,
        is_current_binding: impl FnOnce(ViewportBinding) -> bool,
        item_is_on_surface: impl FnOnce(SurfaceId, ItemId) -> bool,
    ) -> PaneFocusObservationTransition {
        let Some(intent) = self.pending_pane_intent else {
            return PaneFocusObservationTransition::Rejected(
                PaneFocusObservationRejection::UnknownIntent {
                    intent: observation.intent,
                },
            );
        };
        if intent.id != observation.intent {
            return PaneFocusObservationTransition::Rejected(
                PaneFocusObservationRejection::UnknownIntent {
                    intent: observation.intent,
                },
            );
        }
        if self.admitted_pane_intent != Some(intent.id) {
            return PaneFocusObservationTransition::Rejected(
                PaneFocusObservationRejection::IntentNotPublished { intent: intent.id },
            );
        }
        if intent.surface != observation.surface
            || intent.item != observation.item
            || intent.native_guard() != observation.native_guard
        {
            return PaneFocusObservationTransition::Rejected(
                PaneFocusObservationRejection::IntentTargetMismatch,
            );
        }
        if observation
            .native_guard
            .is_some_and(|binding| !is_current_binding(binding))
        {
            return PaneFocusObservationTransition::Rejected(
                PaneFocusObservationRejection::StaleBinding,
            );
        }
        if let Some(item) = observation.item
            && !item_is_on_surface(observation.surface, item)
        {
            return PaneFocusObservationTransition::Rejected(
                PaneFocusObservationRejection::ItemUnavailable { item },
            );
        }
        if let Some(transition) =
            self.host_focus_generation_conflict(observation.surface, observation.generation)
        {
            return transition;
        }
        if intent
            .pane_observation_baseline
            .is_some_and(|baseline| observation.generation <= baseline)
        {
            return PaneFocusObservationTransition::Rejected(
                PaneFocusObservationRejection::ObservationPrecedesIntent,
            );
        }
        if let Some(current) = self
            .pane_observation_by_surface
            .get(&observation.surface)
            .copied()
        {
            let current_generation = current.observation.generation;
            if observation.generation < current_generation {
                return PaneFocusObservationTransition::Stale {
                    current: current_generation,
                };
            }
            if observation.generation == current_generation {
                return PaneFocusObservationTransition::EqualGenerationConflict {
                    generation: observation.generation,
                };
            }
        }
        if let Some(current) = self
            .pane_request_observation_by_surface
            .get(&observation.surface)
            .copied()
        {
            let current_generation = current.observation.generation;
            if observation.generation < current_generation {
                return PaneFocusObservationTransition::Stale {
                    current: current_generation,
                };
            }
            if observation.generation == current_generation {
                return if observation == current.observation {
                    PaneFocusObservationTransition::Duplicate
                } else {
                    PaneFocusObservationTransition::EqualGenerationConflict {
                        generation: observation.generation,
                    }
                };
            }
        }

        self.pane_request_observation_by_surface.insert(
            observation.surface,
            PaneFocusRequestObservationRecord { observation },
        );
        match observation.state {
            PaneFocusRequestObservationState::NotFocused => {
                PaneFocusObservationTransition::NotFocused { intent: intent.id }
            }
            PaneFocusRequestObservationState::Focused => {
                self.pending_pane_intent = None;
                self.clear_admitted_pane_intent(intent.id);
                self.panel_focus_by_surface
                    .insert(intent.surface, PanelFocusRecord::from_focus(intent.focus()));
                self.record_current_ui_focus(
                    intent.surface,
                    intent
                        .item
                        .map_or(HostFocusState::Workspace, HostFocusState::Pane),
                    intent.native_guard(),
                );
                PaneFocusObservationTransition::Focused {
                    cleared_intent: intent.id,
                }
            }
            PaneFocusRequestObservationState::Unavailable => {
                self.pending_pane_intent = None;
                self.clear_admitted_pane_intent(intent.id);
                PaneFocusObservationTransition::Unavailable { intent: intent.id }
            }
        }
    }

    /// Preserves framework focus while superseding older content-focus requests.
    pub(crate) fn preserve_local_focus(
        &mut self,
        surface: SurfaceId,
        native_guard: Option<ViewportBinding>,
        causal: FocusCausalStamp,
    ) -> bool {
        self.admit_pane_causal_claim(PaneCausalClaim {
            causal,
            source: PaneFocusIntentSource::PointerTabGesture,
            target: native_guard.map_or(FocusClaimTarget::Surface(surface), FocusClaimTarget::Dock),
            pane: PaneFocusDisposition::Preserve,
            cause: ViewportActivationCause::PointerTabGesture,
        })
    }

    /// Installs one presentation-neutral item focus request.
    pub(crate) fn request_local_pane_focus(
        &mut self,
        surface: SurfaceId,
        item: ItemId,
        native_guard: Option<ViewportBinding>,
        causal: FocusCausalStamp,
    ) -> Result<Option<PaneFocusIntent>, ViewportFocusError> {
        self.request_local_focus(surface, PanelFocus::Item(item), native_guard, causal)
    }

    /// Requests the logical workspace without substituting a selected pane.
    pub(crate) fn request_local_workspace_focus(
        &mut self,
        surface: SurfaceId,
        native_guard: Option<ViewportBinding>,
        causal: FocusCausalStamp,
    ) -> Result<Option<PaneFocusIntent>, ViewportFocusError> {
        self.request_local_focus(surface, PanelFocus::None, native_guard, causal)
    }

    pub(super) fn request_local_focus(
        &mut self,
        surface: SurfaceId,
        focus: PanelFocus,
        native_guard: Option<ViewportBinding>,
        causal: FocusCausalStamp,
    ) -> Result<Option<PaneFocusIntent>, ViewportFocusError> {
        let focus_observation_baseline = self
            .focus_observations
            .current()
            .map_or_else(FocusObservationGeneration::default, |observation| {
                observation.generation
            });
        self.install_pane_intent(PaneIntentInstall {
            causal,
            activation: None,
            surface,
            native_guard,
            focus,
            source: PaneFocusIntentSource::PointerTabGesture,
            cause: Some(ViewportActivationCause::PointerTabGesture),
            focus_observation_baseline,
        })
    }

    pub(crate) fn pane_observation_generation(
        &self,
        surface: SurfaceId,
    ) -> Option<PaneFocusObservationGeneration> {
        self.pane_observation_by_surface
            .get(&surface)
            .map(|record| record.observation.generation)
            .into_iter()
            .chain(
                self.pane_request_observation_by_surface
                    .get(&surface)
                    .map(|record| record.observation.generation),
            )
            .chain(
                self.host_focus_observation_by_surface
                    .get(&surface)
                    .map(|record| record.generation),
            )
            .max()
    }

    pub(super) fn install_pane_intent(
        &mut self,
        install: PaneIntentInstall,
    ) -> Result<Option<PaneFocusIntent>, ViewportFocusError> {
        let PaneIntentInstall {
            causal,
            activation,
            surface,
            native_guard,
            focus,
            source,
            cause,
            focus_observation_baseline,
        } = install;
        if !self.admit_pane_causal_claim(PaneCausalClaim::from_install(install)) {
            return Ok(None);
        }
        let item = match focus {
            PanelFocus::Item(item) => Some(item),
            PanelFocus::None => None,
        };
        if let Some(watermark) = self.pane_intent_watermark {
            if causal.generation() < watermark.causal.generation()
                || (causal.generation() == watermark.causal.generation()
                    && source.priority() < watermark.source.priority())
            {
                return Ok(None);
            }
            if causal.generation() == watermark.causal.generation()
                && source == watermark.source
                && (surface != watermark.surface
                    || item != watermark.item
                    || native_guard != watermark.native_guard)
            {
                return Ok(None);
            }
            if causal.generation() == watermark.causal.generation()
                && source == watermark.source
                && surface == watermark.surface
                && item == watermark.item
                && native_guard == watermark.native_guard
            {
                return Ok(self.pending_pane_intent);
            }
        }

        let id = self
            .last_pane_intent
            .checked_next()
            .ok_or(ViewportFocusError::PaneFocusIntentIdExhausted)?;
        let pane_observation_baseline = self.pane_observation_generation(surface);
        let intent = PaneFocusIntent {
            id,
            causal,
            activation,
            surface,
            item,
            native: native_guard.map_or(PaneFocusNativeTarget::Local, PaneFocusNativeTarget::Bound),
            source,
            cause,
            focus_observation_baseline,
            pane_observation_baseline,
        };
        self.last_pane_intent = id;
        self.pane_intent_watermark = Some(PaneIntentWatermark {
            causal,
            source,
            surface,
            item,
            native_guard,
        });
        self.admitted_pane_intent = None;
        self.pending_pane_intent = Some(intent);
        Ok(Some(intent))
    }
}

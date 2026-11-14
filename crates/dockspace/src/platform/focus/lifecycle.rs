use super::*;

impl ViewportFocusCoordinator {
    pub(crate) fn reconcile_host_focus_scopes(
        &mut self,
        is_current_scope: impl Fn(SurfaceId, PresentationHostLease, HostPresentationStreamId) -> bool,
    ) -> ViewportFocusCleanup {
        let mut cleanup = ViewportFocusCleanup::default();
        if let Some(current) = self.current_ui_focus
            && let Some((surface, _)) = current.scope
            && let Some((host, stream)) = current.host_scope
            && !is_current_scope(surface, host, stream)
        {
            cleanup.current_focus_changed = self.invalidate_current_ui_focus();
        }
        self.host_focus_observation_by_surface
            .retain(|surface, observation| {
                let keep = is_current_scope(*surface, observation.host, observation.stream);
                cleanup.pane_observation_removed |= !keep;
                keep
            });
        cleanup
    }

    /// Captures semantic focus without choosing among unrelated surface histories.
    pub(crate) fn workspace_replacement_focus(&self) -> WorkspaceReplacementFocus {
        if let Some(intent) = self.pending_pane_intent {
            return WorkspaceReplacementFocus::Dock {
                surface: intent.surface(),
                item: intent.item(),
            };
        }
        if let Some(current) = self.current_focus() {
            return match current {
                CurrentUiFocus::Pane { surface, item } => WorkspaceReplacementFocus::Dock {
                    surface,
                    item: Some(item),
                },
                CurrentUiFocus::Workspace { surface } => WorkspaceReplacementFocus::Dock {
                    surface,
                    item: None,
                },
                CurrentUiFocus::External | CurrentUiFocus::None | CurrentUiFocus::Unknown => {
                    WorkspaceReplacementFocus::OutsideDock
                }
            };
        }
        let Some(target) = self
            .focus_lane
            .winner
            .map(|claim| claim.target)
            .or_else(|| {
                self.focus_observations
                    .current()
                    .and_then(Self::known_focus_target)
            })
        else {
            return WorkspaceReplacementFocus::NoHistory;
        };
        let surface = match target {
            FocusClaimTarget::Dock(binding) => binding.surface(),
            FocusClaimTarget::Surface(surface) => surface,
            FocusClaimTarget::Window(_) | FocusClaimTarget::Foreign | FocusClaimTarget::None => {
                return WorkspaceReplacementFocus::OutsideDock;
            }
        };
        let item = match self.panel_focus(surface) {
            PanelFocusRecord::Item(item) => Some(item),
            PanelFocusRecord::NoHistory | PanelFocusRecord::None => None,
        };
        WorkspaceReplacementFocus::Dock { surface, item }
    }

    /// Revokes all authority derived from the current platform-focus provider.
    ///
    /// Pane-focus history remains available for restoration after a replacement
    /// provider establishes a fresh global-focus baseline.
    #[cfg(test)]
    pub(crate) fn revoke_platform_provider_authority(&mut self) -> ViewportFocusCleanup {
        let had_global_authority = self.focus_observations.current().is_some();
        self.focus_observations.reset_for_provider_replacement();
        self.clear_provider_bound_state(had_global_authority)
    }

    /// Removes binding-scoped pending state while preserving per-surface focus history.
    pub fn clear_binding(&mut self, binding: ViewportBinding) -> ViewportFocusCleanup {
        let mut cleanup = ViewportFocusCleanup::default();
        if self.current_ui_focus.is_some_and(|record| {
            record
                .scope
                .is_some_and(|(_, guard)| guard == Some(binding))
        }) {
            cleanup.current_focus_changed = self.invalidate_current_ui_focus();
        }
        let reservation_count = self.focus_lane.native_reservations.len();
        self.focus_lane
            .native_reservations
            .retain(|_, reservation| reservation.request.target != binding);
        if self.focus_lane.native_reservations.len() != reservation_count {
            self.recompute_focus_winner();
        }
        self.focus_lane.quarantined.remove(&binding);
        self.focus_lane
            .hazards
            .retain(|_, attempt| attempt.request.target != binding);
        if self
            .focus_lane
            .driving
            .is_some_and(|pending| pending.request.target == binding)
            && let Some(pending) = self.focus_lane.driving.take()
        {
            self.cancel_lifecycle_focus_barrier(pending.generation);
            cleanup.pending_activation_cancelled =
                Some((pending.generation, ActivationCancellation::StaleBinding));
        }
        if self
            .pending_pane_intent
            .is_some_and(|intent| intent.native_guard() == Some(binding))
        {
            cleanup.pending_intent_removed = self.pending_pane_intent.take().map(|intent| {
                self.clear_admitted_pane_intent(intent.id);
                intent.id
            });
        }
        if self
            .recorded_observe_only_activation
            .is_some_and(|record| record.request.target == binding)
        {
            cleanup.observe_only_activation_removed =
                self.recorded_observe_only_activation.take().map(|record| {
                    self.clear_admitted_observe_only(record.generation);
                    record.generation
                });
        }
        if self
            .pane_observation_by_surface
            .get(&binding.surface())
            .is_some_and(|record| record.observation.binding == binding)
        {
            self.pane_observation_by_surface.remove(&binding.surface());
            cleanup.pane_observation_removed = true;
        }
        if self
            .pane_request_observation_by_surface
            .get(&binding.surface())
            .is_some_and(|record| record.observation.native_guard == Some(binding))
        {
            self.pane_request_observation_by_surface
                .remove(&binding.surface());
            cleanup.pane_observation_removed = true;
        }
        cleanup
    }

    /// Removes every focus fact owned by a retired logical surface.
    pub fn clear_surface(&mut self, surface: SurfaceId) -> ViewportFocusCleanup {
        let mut cleanup = ViewportFocusCleanup {
            panel_record_removed: self.panel_focus_by_surface.remove(&surface).is_some(),
            pane_observation_removed: self.pane_observation_by_surface.remove(&surface).is_some()
                | self
                    .pane_request_observation_by_surface
                    .remove(&surface)
                    .is_some(),
            ..ViewportFocusCleanup::default()
        };
        cleanup.pane_observation_removed |= self
            .host_focus_observation_by_surface
            .remove(&surface)
            .is_some();
        if self
            .current_ui_focus
            .is_some_and(|record| record.scope.is_some_and(|(current, _)| current == surface))
        {
            cleanup.current_focus_changed = self.invalidate_current_ui_focus();
        }
        let reservation_count = self.focus_lane.native_reservations.len();
        self.focus_lane
            .native_reservations
            .retain(|_, reservation| reservation.request.target.surface() != surface);
        if self.focus_lane.native_reservations.len() != reservation_count {
            self.recompute_focus_winner();
        }
        self.focus_lane
            .quarantined
            .retain(|binding| binding.surface() != surface);
        self.focus_lane
            .hazards
            .retain(|_, attempt| attempt.request.target.surface() != surface);
        if self
            .focus_lane
            .driving
            .is_some_and(|pending| pending.request.target.surface() == surface)
            && let Some(pending) = self.focus_lane.driving.take()
        {
            self.cancel_lifecycle_focus_barrier(pending.generation);
            cleanup.pending_activation_cancelled =
                Some((pending.generation, ActivationCancellation::SurfaceRemoved));
        }
        if self
            .pending_pane_intent
            .is_some_and(|intent| intent.surface == surface)
        {
            cleanup.pending_intent_removed = self.pending_pane_intent.take().map(|intent| {
                self.clear_admitted_pane_intent(intent.id);
                intent.id
            });
        }
        if self
            .recorded_observe_only_activation
            .is_some_and(|record| record.request.target.surface() == surface)
        {
            cleanup.observe_only_activation_removed =
                self.recorded_observe_only_activation.take().map(|record| {
                    self.clear_admitted_observe_only(record.generation);
                    record.generation
                });
        }
        cleanup
    }

    /// Removes focus history and commands for an item which no longer exists on its surface.
    #[cfg(test)]
    pub fn clear_item(&mut self, item: ItemId) -> ViewportFocusCleanup {
        let mut cleanup = ViewportFocusCleanup::default();
        if matches!(self.current_focus(), Some(CurrentUiFocus::Pane { item: focused, .. }) if focused == item)
        {
            cleanup.current_focus_changed = self.invalidate_current_ui_focus();
        }
        let reservation_count = self.focus_lane.native_reservations.len();
        self.focus_lane
            .native_reservations
            .retain(|_, reservation| reservation.request.pane != PaneFocusDisposition::Set(item));
        if self.focus_lane.native_reservations.len() != reservation_count {
            self.recompute_focus_winner();
        }
        let recorded_surfaces = self
            .panel_focus_by_surface
            .iter()
            .filter_map(|(surface, focus)| {
                (*focus == PanelFocusRecord::Item(item)).then_some(*surface)
            })
            .collect::<Vec<_>>();
        for surface in recorded_surfaces {
            self.panel_focus_by_surface.remove(&surface);
            self.pane_observation_by_surface.remove(&surface);
            self.pane_request_observation_by_surface.remove(&surface);
            cleanup.panel_record_removed = true;
            cleanup.pane_observation_removed = true;
        }
        let request_observation_count = self.pane_request_observation_by_surface.len();
        self.pane_request_observation_by_surface
            .retain(|_, record| record.observation.item != Some(item));
        cleanup.pane_observation_removed |=
            self.pane_request_observation_by_surface.len() != request_observation_count;
        if self
            .focus_lane
            .driving
            .is_some_and(|pending| pending.request.pane == PaneFocusDisposition::Set(item))
            && let Some(pending) = self.focus_lane.driving.take()
        {
            self.cancel_lifecycle_focus_barrier(pending.generation);
            cleanup.pending_activation_cancelled = Some((
                pending.generation,
                ActivationCancellation::ItemUnavailable { item },
            ));
        }
        if self
            .pending_pane_intent
            .is_some_and(|intent| intent.item == Some(item))
        {
            cleanup.pending_intent_removed = self.pending_pane_intent.take().map(|intent| {
                self.clear_admitted_pane_intent(intent.id);
                intent.id
            });
        }
        if self
            .recorded_observe_only_activation
            .is_some_and(|record| record.request.pane == PaneFocusDisposition::Set(item))
        {
            cleanup.observe_only_activation_removed =
                self.recorded_observe_only_activation.take().map(|record| {
                    self.clear_admitted_observe_only(record.generation);
                    record.generation
                });
        }
        cleanup
    }

    pub(crate) fn reject_pane_reveal(&mut self, intent: PaneFocusIntentId) -> bool {
        if self
            .pending_pane_intent
            .is_none_or(|pending| pending.id != intent)
        {
            return false;
        }
        self.pending_pane_intent = None;
        self.clear_admitted_pane_intent(intent);
        true
    }

    pub(crate) fn reconcile_authority(
        &mut self,
        surface_exists: impl Fn(SurfaceId) -> bool + Copy,
        can_observe_binding: impl Fn(ViewportBinding) -> bool + Copy,
        can_accept_activation: impl Fn(ViewportBinding) -> bool + Copy,
        item_is_on_surface: impl Fn(SurfaceId, ItemId) -> bool + Copy,
    ) -> ViewportFocusCleanup {
        let mut cleanup = ViewportFocusCleanup::default();

        if let Some(record) = self.current_ui_focus
            && let Some((surface, guard)) = record.scope
            && (!surface_exists(surface)
                || guard.is_some_and(|binding| !can_observe_binding(binding))
                || matches!(record.focus, CurrentUiFocus::Pane { item, .. } if !item_is_on_surface(surface, item)))
        {
            cleanup.current_focus_changed = self.invalidate_current_ui_focus();
        }
        self.host_focus_observation_by_surface
            .retain(|surface, _| surface_exists(*surface));

        self.focus_lane
            .quarantined
            .retain(|binding| surface_exists(binding.surface()));
        self.focus_lane.hazards.retain(|_, attempt| {
            surface_exists(attempt.request.target.surface())
                && can_observe_binding(attempt.request.target)
        });

        self.panel_focus_by_surface.retain(|surface, focus| {
            let keep = surface_exists(*surface)
                && match focus {
                    PanelFocusRecord::Item(item) => item_is_on_surface(*surface, *item),
                    PanelFocusRecord::NoHistory | PanelFocusRecord::None => true,
                };
            cleanup.panel_record_removed |= !keep;
            keep
        });
        self.pane_observation_by_surface.retain(|surface, record| {
            let observation = record.observation;
            let keep = surface_exists(*surface)
                && can_observe_binding(observation.binding)
                && match observation.focus {
                    PanelFocus::Item(item) => item_is_on_surface(*surface, item),
                    PanelFocus::None => true,
                };
            cleanup.pane_observation_removed |= !keep;
            keep
        });
        self.pane_request_observation_by_surface
            .retain(|surface, record| {
                let observation = record.observation;
                let keep = surface_exists(*surface)
                    && observation.native_guard.is_none_or(can_observe_binding)
                    && observation
                        .item
                        .is_none_or(|item| item_is_on_surface(*surface, item));
                cleanup.pane_observation_removed |= !keep;
                keep
            });

        if let Some(pending) = self.focus_lane.driving {
            let binding_is_current = match pending.platform_focus {
                PendingPlatformFocus::EffectRequired => {
                    can_accept_activation(pending.request.target)
                }
                PendingPlatformFocus::Requested { .. }
                | PendingPlatformFocus::Indeterminate { .. }
                | PendingPlatformFocus::ObservedAwaitingTarget { .. } => {
                    can_observe_binding(pending.request.target)
                }
            };
            if let Some(reason) = Self::stale_activation_reason(
                pending.request,
                surface_exists,
                |binding| binding == pending.request.target && binding_is_current,
                item_is_on_surface,
            ) {
                self.focus_lane.driving = None;
                self.cancel_lifecycle_focus_barrier(pending.generation);
                cleanup.pending_activation_cancelled = Some((pending.generation, reason));
            }
        }
        if let Some(intent) = self.pending_pane_intent {
            let binding_is_current = match intent.native {
                PaneFocusNativeTarget::Bound(binding)
                    if self.admitted_pane_intent == Some(intent.id) =>
                {
                    can_observe_binding(binding)
                }
                PaneFocusNativeTarget::Bound(binding) => can_accept_activation(binding),
                PaneFocusNativeTarget::Local | PaneFocusNativeTarget::Waiting => true,
            };
            if !surface_exists(intent.surface)
                || !binding_is_current
                || intent
                    .item
                    .is_some_and(|item| !item_is_on_surface(intent.surface, item))
            {
                self.pending_pane_intent = None;
                self.clear_admitted_pane_intent(intent.id);
                cleanup.pending_intent_removed = Some(intent.id);
            }
        }
        if let Some(record) = self.recorded_observe_only_activation {
            let binding_is_current =
                if self.admitted_observe_only_activation == Some(record.generation) {
                    can_observe_binding(record.request.target)
                } else {
                    can_accept_activation(record.request.target)
                };
            if Self::stale_activation_reason(
                record.request,
                surface_exists,
                |binding| binding == record.request.target && binding_is_current,
                item_is_on_surface,
            )
            .is_some()
            {
                self.recorded_observe_only_activation = None;
                self.clear_admitted_observe_only(record.generation);
                cleanup.observe_only_activation_removed = Some(record.generation);
            }
        }

        cleanup
    }

    pub(super) fn clear_focus_stream_authority(
        &mut self,
        global_authority_removed: bool,
    ) -> ViewportFocusCleanup {
        let native_reservations = std::mem::take(&mut self.focus_lane.native_reservations);
        let settled_winner = self.focus_lane.settled_winner;
        let cleanup = self.clear_provider_bound_state(global_authority_removed);

        // Stream faults revoke provider-derived observations and in-flight effects,
        // but the native lifecycle still owns its release-time causal claims.
        self.focus_lane.settled_winner = settled_winner;
        self.focus_lane.native_reservations = native_reservations;
        self.recompute_focus_winner();
        cleanup
    }

    pub(super) fn clear_provider_bound_state(
        &mut self,
        global_authority_removed: bool,
    ) -> ViewportFocusCleanup {
        let current_focus_changed = if self
            .current_ui_focus
            .is_some_and(|record| record.scope.is_none_or(|(_, guard)| guard.is_some()))
        {
            self.invalidate_current_ui_focus()
        } else {
            false
        };
        let pending_activation_cancelled = self.focus_lane.driving.take().map(|pending| {
            self.cancel_lifecycle_focus_barrier(pending.generation);
            (
                pending.generation,
                ActivationCancellation::PlatformAuthorityRevoked,
            )
        });
        let pending_intent = self.pending_pane_intent.take();
        let retained_pane_intent = pending_intent.filter(|intent| intent.native_guard().is_none());
        let pending_intent_removed = pending_intent
            .filter(|intent| intent.native_guard().is_some())
            .map(|intent| intent.id);
        let admitted_pane_intent = self
            .admitted_pane_intent
            .filter(|intent| retained_pane_intent.is_some_and(|retained| retained.id == *intent));
        let observe_only_activation_removed = self
            .recorded_observe_only_activation
            .take()
            .map(|record| record.generation);
        self.admitted_observe_only_activation = None;
        self.focus_lane = FocusLane::default();
        self.pending_pane_intent = retained_pane_intent;
        self.admitted_pane_intent = admitted_pane_intent;
        if let Some(intent) = retained_pane_intent {
            let claim = Self::pending_intent_claim(intent);
            self.focus_lane.settled_winner = Some(claim);
            self.focus_lane.winner = Some(claim);
        }
        ViewportFocusCleanup {
            global_authority_removed,
            current_focus_changed,
            pending_activation_cancelled,
            pending_intent_removed,
            observe_only_activation_removed,
            ..ViewportFocusCleanup::default()
        }
    }

    /// Clears workspace-bound focus authority while preserving monotonic identities.
    pub(crate) fn reconcile_workspace_replacement(
        &mut self,
        root_continuations: &[(ViewportBinding, ViewportBinding)],
    ) {
        let had_focus_fact = self.current_ui_focus.is_some()
            || self.focus_observations.last_generation.is_some()
            || self.focus_observations.requires_tombstone;
        let continued_focus = self.focus_observations.current().and_then(|observed| {
            let Authority::Known(GlobalFocusedWindow::Dock(focused)) = observed.focused else {
                return None;
            };
            let successor = root_continuations.iter().find_map(|&(before, after)| {
                (before.window() == focused).then_some(after.window())
            })?;
            Some(FocusObservationEnvelope::new(
                observed.generation,
                Authority::Known(GlobalFocusedWindow::Dock(successor)),
                Authority::Known(None),
            ))
        });
        if let Some(observation) = continued_focus {
            // The exact resource continuation preserves physical window focus.
            // Old pane state and focus-effect acknowledgements do not transfer.
            self.focus_observations.current = Some(observation);
            self.focus_observations.last_envelope = Some(observation);
        } else if self
            .focus_observations
            .current()
            .is_some_and(|observation| {
                matches!(
                    observation.focused,
                    Authority::Known(GlobalFocusedWindow::Dock(_))
                )
            })
        {
            self.focus_observations.current = None;
        }
        // Layout replacement retains the provider and its replay watermark.
        // Foreign/absent focus and transport revocation must survive it too.
        self.focus_lane = FocusLane::default();
        self.pane_intent_watermark = None;
        self.recorded_observe_only_activation = None;
        self.pending_pane_intent = None;
        self.admitted_observe_only_activation = None;
        self.admitted_pane_intent = None;
        self.current_ui_focus = None;
        if had_focus_fact {
            let focused =
                self.focus_observations.current().and_then(|observation| {
                    match observation.focused {
                        Authority::Known(focused) => Some(focused),
                        Authority::Unknown(_) => None,
                    }
                });
            self.record_platform_ui_focus(focused);
        }
        self.panel_focus_by_surface.clear();
        self.host_focus_observation_by_surface.clear();
        self.pane_observation_by_surface.clear();
        self.pane_request_observation_by_surface.clear();
    }

    /// Mints the replacement's sole successor within the rollbackable candidate.
    pub(crate) fn request_workspace_replacement_focus(
        &mut self,
        surface: SurfaceId,
        focus: PanelFocus,
        requires_native_binding: bool,
        causal: FocusCausalStamp,
    ) -> Result<Option<PaneFocusIntent>, ViewportFocusError> {
        let Some(mut intent) = self.install_pane_intent(PaneIntentInstall {
            causal,
            activation: None,
            surface,
            native_guard: None,
            focus,
            source: PaneFocusIntentSource::WorkspaceReplacement,
            cause: None,
            focus_observation_baseline: FocusObservationGeneration::default(),
        })?
        else {
            return Ok(None);
        };
        if requires_native_binding {
            intent.native = PaneFocusNativeTarget::Waiting;
            self.pending_pane_intent = Some(intent);
        }
        Ok(Some(intent))
    }

    /// Resolves only the existing waiting intent, using current activation eligibility.
    pub(crate) fn resume_workspace_replacement_focus(
        &mut self,
        eligible_binding: impl FnOnce(SurfaceId) -> Option<ViewportBinding>,
    ) {
        let Some(mut intent) = self
            .pending_pane_intent
            .filter(|intent| intent.awaiting_native_binding())
        else {
            return;
        };
        let Some(binding) = eligible_binding(intent.surface) else {
            return;
        };
        let previous_claim = Self::pending_intent_claim(intent);
        intent.native = PaneFocusNativeTarget::Bound(binding);
        intent.focus_observation_baseline = self
            .focus_observations
            .current()
            .map_or_else(FocusObservationGeneration::default, |observation| {
                observation.generation()
            });
        intent.pane_observation_baseline = self.pane_observation_generation(intent.surface);
        self.pending_pane_intent = Some(intent);
        if let Some(watermark) = self.pane_intent_watermark.as_mut() {
            watermark.native_guard = Some(binding);
        }
        if self.focus_lane.settled_winner == Some(previous_claim) {
            self.focus_lane.settled_winner = Some(Self::pending_intent_claim(intent));
            self.recompute_focus_winner();
        }
    }
}

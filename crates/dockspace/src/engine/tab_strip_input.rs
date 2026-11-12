//! Tab-strip and tab-list popup input reduction.

use super::*;

pub(super) enum TabListMenuActivation {
    /// Current-UI semantic input can request content focus without global desktop facts.
    Local(FocusCausalStamp),
    /// Native pointer input retains ordered activation and compensation handling.
    Journal(FocusCausalStamp),
}

pub(super) fn aligned_tab_strip_scroll_offset(
    plan: &PresentationPlan,
    key: TabStripStateKey,
    control: TabStripControlId,
) -> Option<f64> {
    let bar = plan
        .tab_bar_records()
        .iter()
        .find(|record| *record.id() == key.bar())?;
    let viewport = bar.viewport();
    let current = bar.scroll_offset();
    let maximum = bar.maximum_scroll_offset();
    let scrolling_bounds = || {
        bar.members()
            .iter()
            .map(|member| member.full_bounds())
            .chain(bar.after_tabs_bounds())
    };
    match control {
        TabStripControlId::ScrollBackward(control_bar) if control_bar == key.bar() => {
            scrolling_bounds()
                .filter(|bounds| bounds.x() < viewport.x())
                .max_by(|left, right| left.x().total_cmp(&right.x()))
                .map(|bounds| (current + bounds.x() - viewport.x()).clamp(0.0, maximum))
        }
        TabStripControlId::ScrollForward(control_bar) if control_bar == key.bar() => {
            scrolling_bounds()
                .filter(|bounds| bounds.max().x() > viewport.max().x())
                .min_by(|left, right| left.max().x().total_cmp(&right.max().x()))
                .map(|bounds| (current + bounds.max().x() - viewport.max().x()).clamp(0.0, maximum))
        }
        TabStripControlId::ScrollBackward(_)
        | TabStripControlId::ScrollForward(_)
        | TabStripControlId::TabListMenu(_)
        | TabStripControlId::RevealTabBar(_) => None,
    }
}

fn reveal_scroll_offset(
    current: f64,
    maximum: f64,
    item_start: f64,
    item_end: f64,
    viewport_start: f64,
    viewport_end: f64,
) -> f64 {
    let requested = if item_start < viewport_start {
        current + item_start - viewport_start
    } else if item_end > viewport_end {
        current + item_end - viewport_end
    } else {
        current
    };
    requested.clamp(0.0, maximum)
}

fn scroll_offset_preserving_ranges(
    requested: f64,
    maximum: f64,
    ranges: impl IntoIterator<Item = (f64, f64)>,
) -> f64 {
    let mut minimum: f64 = 0.0;
    let mut upper = maximum;
    for (item_minimum, item_maximum) in ranges {
        let candidate_minimum = minimum.max(item_minimum);
        let candidate_maximum = upper.min(item_maximum);
        if candidate_minimum <= candidate_maximum {
            minimum = candidate_minimum;
            upper = candidate_maximum;
        }
    }
    requested.clamp(minimum, upper)
}

fn fully_visible_scroll_range(
    start: f64,
    end: f64,
    viewport_extent: f64,
    maximum: f64,
) -> (f64, f64) {
    if end - start <= viewport_extent {
        (
            (end - viewport_extent).clamp(0.0, maximum),
            start.clamp(0.0, maximum),
        )
    } else {
        let aligned = start.clamp(0.0, maximum);
        (aligned, aligned)
    }
}

fn operable_tab_scroll_range(
    member: TabStripMemberRecord,
    bar: &TabBarRecord,
    close_allowed: bool,
    config: &DockPresentationConfig,
) -> (f64, f64) {
    let full = member.full_bounds();
    let viewport = bar.viewport();
    let start = full.x() - viewport.x() + bar.scroll_offset();
    let end = start + full.width();
    let operable = config
        .tab_close_extent()
        .min(config.tab_min_width())
        .min(full.width());
    let (minimum, maximum) = if close_allowed {
        let close_extent = config
            .tab_close_extent()
            .min(full.height())
            .min(full.width());
        let padding = config
            .tab_horizontal_padding()
            .min((full.width() - close_extent).max(0.0));
        let close_end = end - padding;
        let close_start = close_end - close_extent;
        (close_end - viewport.width(), close_start - operable)
    } else {
        (start + operable - viewport.width(), end - operable)
    };
    let minimum = minimum.clamp(0.0, bar.maximum_scroll_offset());
    let maximum = maximum.clamp(0.0, bar.maximum_scroll_offset());
    if minimum <= maximum {
        (minimum, maximum)
    } else {
        let aligned = start.clamp(0.0, bar.maximum_scroll_offset());
        (aligned, aligned)
    }
}

impl DockEngine {
    pub(super) fn dismiss_popup_from_other_surface_press(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        capture: Authority<PointerCaptureOwner>,
        receipt: &ValidatedPointerReceiverReceipt,
        snapshot: &JournalPresentationSnapshot,
    ) -> Result<Option<InteractionOutcome>, EngineError> {
        let Some(session) = self
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .map(|menu| menu.session())
        else {
            return Ok(None);
        };
        let PointerReceiverObservation::Presented(observation) = receipt.observation() else {
            return Ok(None);
        };
        let Some(delivery) = observation.probes().iter().find_map(|probe| match probe {
            PointerReceiverProbeReceipt::Delivery(delivery) => Some(delivery),
            PointerReceiverProbeReceipt::HoverHit(_) => None,
        }) else {
            return Ok(None);
        };
        if ![delivery.click(), delivery.drag()].into_iter().any(|lane| {
            matches!(
                lane,
                PointerReceiverDeliveryDisposition::Dock(_)
                    | PointerReceiverDeliveryDisposition::DockCanvas
            )
        }) {
            return Ok(None);
        }
        let (Some(output), Some(authority)) = (delivery.output(), delivery.authority()) else {
            return Ok(None);
        };
        if output.surface() == session.key().surface()
            || self
                .interaction_projection(output.surface())
                .is_none_or(|projection| projection.authority() != authority)
        {
            return Ok(None);
        }
        let presentation = snapshot
            .presentation(output, authority)
            .map_err(|source| Self::tab_strip_state_invariant(cause, source))?;
        if let Some(rejection) =
            self.journal_press_capture_rejection(owner, capture, presentation)?
        {
            return Ok(Some(InteractionOutcome::Rejected(rejection)));
        }
        self.close_tab_list_menu(cause, session).map(Some)
    }

    /// Verifies the geometry portion of one known docking receiver claim before
    /// the journal watermark advances.
    ///
    /// Exact output authority alone is insufficient: an adapter could otherwise
    /// name an unrelated valid region from the same retained presentation. The
    /// point belongs to the provider edge, never to the adapter receipt, and
    /// the delivery winner comes from the core-owned hit manifest. Hover-drop
    /// winner validation deliberately remains in the journal interaction FSM,
    /// where complete-root source suppression is known.
    pub(super) fn finish_journal_tab_strip_control(
        &mut self,
        cause: ReductionCause,
        frozen: &FrozenTabStripControlClick,
        plan: &PresentationPlan,
    ) -> Result<InteractionOutcome, EngineError> {
        let control = frozen.record.id();
        let Some(current) = plan
            .tab_strip_control_records()
            .iter()
            .copied()
            .find(|record| record.id() == control)
        else {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::TabStripControlUnavailable { control },
            ));
        };
        if current != frozen.record {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::TabStripControlRecordChanged { control },
            ));
        }
        if !current.enabled() {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::TabStripControlDisabled { control },
            ));
        }
        if frozen.key.surface() != plan.surface()
            || frozen.key.bar() != control.bar()
            || self
                .presentation_authority
                .tab_strip_states
                .state(frozen.key)
                .is_none()
        {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::TabStripSourceUnavailable { key: frozen.key },
            ));
        }

        let mut tab_strip_states = self.presentation_authority.tab_strip_states.clone();
        let (menu, delta) = match control {
            TabStripControlId::RevealTabBar(_) => {
                let delta = tab_strip_states
                    .reveal_single_tab_bar(frozen.key)
                    .map_err(|source| Self::tab_strip_state_invariant(cause, source))?;
                (None, delta)
            }
            TabStripControlId::ScrollBackward(_) | TabStripControlId::ScrollForward(_) => {
                let Some(offset) = aligned_tab_strip_scroll_offset(plan, frozen.key, control)
                else {
                    return Ok(InteractionOutcome::TabStripControlActivated {
                        control,
                        changed: false,
                        menu: None,
                    });
                };
                let delta = tab_strip_states
                    .set_tab_strip_scroll_offset(frozen.key, offset)
                    .map_err(|source| Self::tab_strip_state_invariant(cause, source))?;
                (None, delta)
            }
            TabStripControlId::TabListMenu(_) => {
                let previous = tab_strip_states.active_menu().map(|menu| menu.session());
                let (resulting, delta) = if previous
                    .is_some_and(|session| session.key() == frozen.key)
                {
                    let delta = tab_strip_states
                        .close_tab_list_menu(previous.expect("matching menu session exists"))
                        .map_err(|source| Self::tab_strip_state_invariant(cause, source))?;
                    (None, delta)
                } else {
                    let preferred = match self.workspace.node(frozen.key.bar().tabs) {
                        Some(Node::Tabs { selected, .. }) => *selected,
                        _ => {
                            return Ok(InteractionOutcome::Rejected(
                                InteractionRejection::TabStripSourceUnavailable { key: frozen.key },
                            ));
                        }
                    };
                    match tab_strip_states.open_tab_list_menu(frozen.key, preferred) {
                        Ok((session, delta)) => (Some(session), delta),
                        Err(
                            TabStripStateError::UnknownTabStrip { .. }
                            | TabStripStateError::EmptyTabListMenu { .. }
                            | TabStripStateError::DuplicateTabListMenuItem { .. },
                        ) => {
                            return Ok(InteractionOutcome::Rejected(
                                InteractionRejection::TabStripSourceUnavailable { key: frozen.key },
                            ));
                        }
                        Err(source) => {
                            return Err(Self::tab_strip_state_invariant(cause, source));
                        }
                    }
                };
                (resulting, delta)
            }
        };
        let changed = delta.state_changed();
        if changed {
            self.presentation_authority.tab_strip_states = tab_strip_states;
            self.consume_tab_strip_state_delta(cause, &delta)?;
        }
        Ok(InteractionOutcome::TabStripControlActivated {
            control,
            changed,
            menu,
        })
    }

    pub(super) fn finish_tab_list_menu_row(
        &mut self,
        cause: ReductionCause,
        activation: TabListMenuActivation,
        frozen: &FrozenTabListMenuRowClick,
        plan: &PresentationPlan,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let session = frozen.session;
        let tab = frozen.record.tab();
        if let Err(rejection) = self.validate_tab_list_menu_popup(plan, session, frozen.revision) {
            return Ok(InteractionOutcome::Rejected(rejection));
        }
        let Some(menu) = plan
            .tab_list_menu_records()
            .iter()
            .find(|record| record.session() == session)
        else {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::TabListMenuSessionUnavailable { session },
            ));
        };
        let Some(current) = menu
            .rows()
            .iter()
            .copied()
            .find(|record| record.tab() == tab)
        else {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::TabListMenuRowUnavailable { session, tab },
            ));
        };
        if current != frozen.record {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::TabListMenuRowRecordChanged { session, tab },
            ));
        }
        if !self
            .presentation_authority
            .tab_strip_states
            .active_menu_for(session.key())
            .is_some_and(|active| active.session() == session && active.items().contains(&tab.item))
        {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::TabListMenuSessionUnavailable { session },
            ));
        }

        let source = match self
            .workspace
            .capture_item_source(tab.root, tab.tabs, tab.item)
        {
            Ok(source) => source,
            Err(source) => {
                return Ok(InteractionOutcome::Rejected(
                    InteractionRejection::CommandRejected(source),
                ));
            }
        };
        let command = WorkspaceCommand::Select { source };
        let staged = match self.stage_journal_workspace_command(cause, &command, policy)? {
            Ok(staged) => staged,
            Err(source) => {
                return Ok(InteractionOutcome::Rejected(
                    InteractionRejection::CommandRejected(source),
                ));
            }
        };
        let mut tab_strip_states = self.presentation_authority.tab_strip_states.clone();
        let close_delta = tab_strip_states
            .close_tab_list_menu(session)
            .map_err(|source| Self::tab_strip_state_invariant(cause, source))?;
        let changed = staged.changed;
        let outcome = staged.outcome;
        self.presentation_authority.tab_strip_states = tab_strip_states;
        self.publish_workspace(staged.publication);
        self.reconcile_viewport_focus_authority();
        if changed {
            self.advance_revision_caused_with_tab_strip_delta(cause, &close_delta)?;
            events.push(WorkspaceEvent::new_caused(
                cause,
                self.version,
                WorkspaceEventKind::CommandCommitted(outcome.clone()),
            ));
        } else {
            self.consume_tab_strip_state_delta(cause, &close_delta)?;
        }
        let surface = session.key().surface();
        let (focus_causal, native_activation) = match activation {
            TabListMenuActivation::Local(causal) => (causal, None),
            TabListMenuActivation::Journal(causal) => (
                causal,
                self.viewport
                    .viewport(surface)
                    .filter(|record| record.can_accept_activation())
                    .map(crate::viewport_registry::ViewportRecord::binding),
            ),
        };
        if let Some(binding) = native_activation {
            let _ = self.start_viewport_activation(
                ViewportActivationRequest::pointer_tab_gesture(binding, PanelFocus::Item(tab.item)),
                focus_causal,
                events,
            )?;
        } else {
            let native_guard = self.viewport_focus_binding(surface);
            self.viewport_focus
                .request_local_pane_focus(surface, tab.item, native_guard, focus_causal)
                .map_err(|source| EngineError::CausedViewportFocus {
                    cause: focus_causal.cause(),
                    source,
                })?;
        }
        Ok(InteractionOutcome::TabListMenuItemSelected {
            session,
            tab,
            outcome,
            changed,
        })
    }

    pub(super) fn validate_tab_list_menu_popup(
        &self,
        plan: &PresentationPlan,
        session: crate::tab_strip::TabListMenuSessionId,
        expected_revision: PopupRoutingRevision,
    ) -> Result<(), InteractionRejection> {
        let projected = plan.popup();
        if projected.session() != Some(session) || projected.owner() != Some(session.key()) {
            return Err(InteractionRejection::TabListMenuSessionUnavailable { session });
        }
        if projected.revision() != expected_revision {
            return Err(InteractionRejection::TabListMenuPopupRevisionChanged {
                session,
                expected: expected_revision,
                actual: projected.revision(),
            });
        }

        let current = self
            .presentation_authority
            .tab_strip_states
            .popup_requirement();
        if current.session() != Some(session)
            || current.owner() != Some(session.key())
            || !self
                .presentation_authority
                .tab_strip_states
                .active_menu_for(session.key())
                .is_some_and(|active| active.session() == session)
        {
            return Err(InteractionRejection::TabListMenuSessionUnavailable { session });
        }
        if current.revision() != expected_revision {
            return Err(InteractionRejection::TabListMenuPopupRevisionChanged {
                session,
                expected: expected_revision,
                actual: current.revision(),
            });
        }
        Ok(())
    }

    pub(super) fn tab_strip_state_invariant(
        cause: ReductionCause,
        source: impl std::fmt::Display,
    ) -> EngineError {
        EngineError::PointerInteractionInvariant {
            cause,
            detail: source.to_string(),
        }
    }

    pub(super) fn consume_tab_strip_state_delta(
        &mut self,
        cause: ReductionCause,
        delta: &TabStripStateDelta,
    ) -> Result<(), EngineError> {
        if !delta.state_changed() {
            return Ok(());
        }
        let dragging = if let InteractionStatus::Dragging { session } = self.interaction.status() {
            self.interaction
                .clear_preview(session, PreviewResolutionStatus::Unavailable)
                .map_err(|error| Self::tab_strip_state_invariant(cause, format!("{error:?}")))?;
            self.interaction
                .set_drop_affordance(session, None)
                .map_err(|error| Self::tab_strip_state_invariant(cause, format!("{error:?}")))?;
            true
        } else {
            false
        };
        match delta.influence() {
            TabStripInfluenceDomain::Local(surfaces) => {
                // Scrolling during a drag changes only presentation state.
                // Reuse current measurements to compile the new strip before
                // the host completes this frame; input still needs its exact
                // newly presented output rather than the predecessor geometry.
                let cached = if dragging {
                    surfaces
                        .iter()
                        .filter_map(|surface| {
                            let measurements = self
                                .scene()
                                .ready_candidate(*surface)?
                                .plan()
                                .layout_facts()?
                                .measurements();
                            Some((*surface, Arc::clone(measurements)))
                        })
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                self.invalidate_tab_strip_presentation(cause, surfaces)?;
                for (surface, measurements) in cached {
                    let token = self
                        .begin_surface_contribution(surface)
                        .map_err(|error| Self::tab_strip_state_invariant(cause, error))?;
                    if measurements.ticket() != token.ticket() {
                        continue;
                    }
                    let contribution = self
                        .prepare_surface_contribution(token, (*measurements).clone())
                        .map_err(|error| Self::tab_strip_state_invariant(cause, error))?;
                    self.reduce_surface_contribution(
                        cause.tick(),
                        contribution,
                        self.policy.revision(),
                    )?;
                }
                self.rebind_edge_scroll_after_local_recompile(cause, surfaces)?;
                Ok(())
            }
            TabStripInfluenceDomain::Popup(_) => self
                .try_rebuild_presentation_requirements(Some(delta.influence()))
                .map_err(|source| EngineError::PointerInteractionInvariant {
                    cause,
                    detail: source.to_string(),
                }),
        }
    }

    fn invalidate_tab_strip_presentation(
        &mut self,
        cause: ReductionCause,
        surfaces: &BTreeSet<SurfaceId>,
    ) -> Result<(), EngineError> {
        for surface in surfaces {
            if self
                .presentation_authority
                .scene
                .surface(*surface)
                .is_none()
            {
                continue;
            }
            self.presentation_authority
                .scene
                .invalidate_presentation_input(*surface)
                .map_err(|source| EngineError::PointerInteractionInvariant {
                    cause,
                    detail: source.to_string(),
                })?;
        }
        Ok(())
    }

    fn prepared_tab_strip_plan(
        &self,
        presentation: FrozenPresentationAuthority,
    ) -> Result<PresentationPlan, InteractionRejection> {
        self.prepared_tab_strip_plan_ref(presentation).cloned()
    }

    fn prepared_tab_strip_plan_ref(
        &self,
        presentation: FrozenPresentationAuthority,
    ) -> Result<&PresentationPlan, InteractionRejection> {
        let projection = self
            .presentation_authority
            .scene
            .interaction_projection(presentation.surface())
            .ok_or(InteractionRejection::StaleScene)?;
        if projection.authority() != presentation.presented
            || projection.popup_revision() != presentation.popup_revision
        {
            return Err(InteractionRejection::StaleScene);
        }
        Ok(projection.plan())
    }

    pub(super) fn reduce_prepared_tab_strip_control(
        &mut self,
        cause: ReductionCause,
        prepared: &PreparedTabStripControlActivation,
    ) -> Result<InputOutcome, EngineError> {
        let outcome = match self.prepared_tab_strip_plan(prepared.presentation) {
            Ok(plan) => self.finish_journal_tab_strip_control(cause, &prepared.control, &plan)?,
            Err(rejection) => InteractionOutcome::Rejected(rejection),
        };
        Ok(InputOutcome::InteractionProcessed {
            outcome,
            version: self.version,
        })
    }

    pub(super) fn reduce_prepared_tab_list_menu_row(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        prepared: &PreparedTabListMenuRowActivation,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<InputOutcome, EngineError> {
        let outcome = match self.prepared_tab_strip_plan(prepared.presentation) {
            Ok(plan) => self.finish_tab_list_menu_row(
                cause,
                TabListMenuActivation::Local(focus_causal),
                &prepared.row,
                &plan,
                policy,
                events,
            )?,
            Err(rejection) => InteractionOutcome::Rejected(rejection),
        };
        Ok(InputOutcome::InteractionProcessed {
            outcome,
            version: self.version,
        })
    }

    pub(super) fn reduce_prepared_tab_list_menu_dismiss(
        &mut self,
        cause: ReductionCause,
        prepared: &PreparedTabListMenuDismiss,
    ) -> Result<InputOutcome, EngineError> {
        let outcome = match self.prepared_tab_strip_plan(prepared.presentation) {
            Err(rejection) => InteractionOutcome::Rejected(rejection),
            Ok(plan) => self.finish_tab_list_menu_dismiss(
                cause,
                &plan,
                prepared.session,
                prepared.revision,
                &prepared.record,
                prepared.backdrop,
            )?,
        };
        Ok(InputOutcome::InteractionProcessed {
            outcome,
            version: self.version,
        })
    }

    pub(super) fn reduce_prepared_tab_strip_scroll(
        &mut self,
        cause: ReductionCause,
        prepared: &PreparedTabStripScroll,
    ) -> Result<InputOutcome, EngineError> {
        let outcome = match self.resolve_prepared_tab_strip_scroll(prepared) {
            Err(rejection) => InteractionOutcome::Rejected(rejection),
            Ok(offset) => self.commit_tab_strip_scroll(cause, prepared.key, offset)?,
        };
        Ok(InputOutcome::InteractionProcessed {
            outcome,
            version: self.version,
        })
    }

    pub(super) fn reduce_prepared_tab_list_menu_scroll(
        &mut self,
        cause: ReductionCause,
        prepared: &PreparedTabListMenuScroll,
    ) -> Result<InputOutcome, EngineError> {
        let outcome = match self.prepared_tab_strip_plan(prepared.presentation) {
            Err(rejection) => InteractionOutcome::Rejected(rejection),
            Ok(plan) => self.finish_tab_list_menu_scroll(
                cause,
                &plan,
                prepared.session,
                prepared.revision,
                &prepared.record,
                &prepared.adjustment,
            )?,
        };
        Ok(InputOutcome::InteractionProcessed {
            outcome,
            version: self.version,
        })
    }

    pub(super) fn reduce_prepared_tab_list_menu_navigation(
        &mut self,
        cause: ReductionCause,
        prepared: &PreparedTabListMenuNavigation,
    ) -> Result<InputOutcome, EngineError> {
        let outcome = match self.prepared_tab_strip_plan(prepared.presentation) {
            Err(rejection) => InteractionOutcome::Rejected(rejection),
            Ok(plan) => self.finish_tab_list_menu_navigation(
                cause,
                &plan,
                prepared.session,
                prepared.revision,
                &prepared.record,
                prepared.target,
            )?,
        };
        Ok(InputOutcome::InteractionProcessed {
            outcome,
            version: self.version,
        })
    }

    pub(super) fn finish_tab_list_menu_dismiss(
        &mut self,
        cause: ReductionCause,
        plan: &PresentationPlan,
        session: crate::tab_strip::TabListMenuSessionId,
        revision: PopupRoutingRevision,
        expected_record: &TabListMenuRecord,
        expected_backdrop: TabListMenuBackdropRecord,
    ) -> Result<InteractionOutcome, EngineError> {
        if let Err(rejection) = self.validate_tab_list_menu_popup(plan, session, revision) {
            return Ok(InteractionOutcome::Rejected(rejection));
        }
        let menu = plan
            .tab_list_menu_records()
            .iter()
            .find(|record| record.session() == session);
        let backdrop = plan
            .tab_list_menu_backdrop_records()
            .iter()
            .find(|record| record.session() == session);
        if menu != Some(expected_record) {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::TabListMenuRecordChanged { session },
            ));
        }
        if backdrop.copied() != Some(expected_backdrop) {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::TabListMenuBackdropRecordChanged { session },
            ));
        }
        if !self
            .presentation_authority
            .tab_strip_states
            .active_menu_for(session.key())
            .is_some_and(|active| active.session() == session)
        {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::TabListMenuSessionUnavailable { session },
            ));
        }

        self.close_tab_list_menu(cause, session)
    }

    fn close_tab_list_menu(
        &mut self,
        cause: ReductionCause,
        session: crate::tab_strip::TabListMenuSessionId,
    ) -> Result<InteractionOutcome, EngineError> {
        let mut states = self.presentation_authority.tab_strip_states.clone();
        let delta = states
            .close_tab_list_menu(session)
            .map_err(|source| Self::tab_strip_state_invariant(cause, source))?;
        self.presentation_authority.tab_strip_states = states;
        self.consume_tab_strip_state_delta(cause, &delta)?;
        Ok(InteractionOutcome::TabListMenuDismissed { session })
    }

    pub(super) fn finish_tab_list_menu_scroll(
        &mut self,
        cause: ReductionCause,
        plan: &PresentationPlan,
        session: crate::tab_strip::TabListMenuSessionId,
        revision: PopupRoutingRevision,
        expected_record: &TabListMenuRecord,
        adjustment: &TabScrollAdjustment,
    ) -> Result<InteractionOutcome, EngineError> {
        let offset = match self.resolve_tab_list_menu_scroll(
            plan,
            session,
            revision,
            expected_record,
            adjustment,
        ) {
            Ok(offset) => offset,
            Err(rejection) => return Ok(InteractionOutcome::Rejected(rejection)),
        };
        let mut states = self.presentation_authority.tab_strip_states.clone();
        let delta = states
            .set_active_menu_scroll_offset(session, offset)
            .map_err(|source| Self::tab_strip_state_invariant(cause, source))?;
        let changed = delta.state_changed();
        if changed {
            self.presentation_authority.tab_strip_states = states;
            self.consume_tab_strip_state_delta(cause, &delta)?;
        }
        Ok(InteractionOutcome::TabListMenuScrolled {
            session,
            offset,
            changed,
        })
    }

    pub(super) fn finish_tab_list_menu_navigation(
        &mut self,
        cause: ReductionCause,
        plan: &PresentationPlan,
        session: crate::tab_strip::TabListMenuSessionId,
        revision: PopupRoutingRevision,
        expected_record: &TabListMenuRecord,
        target: ItemId,
    ) -> Result<InteractionOutcome, EngineError> {
        let offset = match self.resolve_tab_list_menu_navigation(
            plan,
            session,
            revision,
            expected_record,
            target,
        ) {
            Ok(offset) => offset,
            Err(rejection) => return Ok(InteractionOutcome::Rejected(rejection)),
        };
        let mut states = self.presentation_authority.tab_strip_states.clone();
        let focus_delta = states
            .set_active_menu_focus(session, target)
            .map_err(|source| Self::tab_strip_state_invariant(cause, source))?;
        let scroll_delta = states
            .set_active_menu_scroll_offset(session, offset)
            .map_err(|source| Self::tab_strip_state_invariant(cause, source))?;
        let delta = focus_delta.merge(scroll_delta);
        let changed = delta.state_changed();
        if changed {
            self.presentation_authority.tab_strip_states = states;
            self.consume_tab_strip_state_delta(cause, &delta)?;
        }
        Ok(InteractionOutcome::TabListMenuFocusMoved {
            session,
            item: target,
            offset,
            changed,
        })
    }

    pub(super) fn finish_tab_strip_scroll(
        &mut self,
        cause: ReductionCause,
        plan: &PresentationPlan,
        key: TabStripStateKey,
        expected_record: &TabBarRecord,
        adjustment: &TabScrollAdjustment,
    ) -> Result<InteractionOutcome, EngineError> {
        let offset = match self.resolve_tab_strip_scroll(plan, key, expected_record, adjustment) {
            Ok(offset) => offset,
            Err(rejection) => return Ok(InteractionOutcome::Rejected(rejection)),
        };
        self.commit_tab_strip_scroll(cause, key, offset)
    }

    pub(super) fn commit_tab_strip_scroll(
        &mut self,
        cause: ReductionCause,
        key: TabStripStateKey,
        offset: f64,
    ) -> Result<InteractionOutcome, EngineError> {
        let mut states = self.presentation_authority.tab_strip_states.clone();
        let delta = states
            .set_tab_strip_scroll_offset(key, offset)
            .map_err(|source| Self::tab_strip_state_invariant(cause, source))?;
        let changed = delta.state_changed();
        if changed {
            self.presentation_authority.tab_strip_states = states;
            self.consume_tab_strip_state_delta(cause, &delta)?;
        }
        Ok(InteractionOutcome::TabStripScrolled {
            bar: key.bar(),
            offset,
            changed,
        })
    }

    fn resolve_prepared_tab_strip_scroll(
        &self,
        prepared: &PreparedTabStripScroll,
    ) -> Result<f64, InteractionRejection> {
        let plan = self.prepared_tab_strip_plan_ref(prepared.presentation)?;
        self.resolve_tab_strip_scroll(plan, prepared.key, &prepared.record, &prepared.adjustment)
    }

    fn resolve_tab_strip_scroll(
        &self,
        plan: &PresentationPlan,
        key: TabStripStateKey,
        expected_record: &TabBarRecord,
        adjustment: &TabScrollAdjustment,
    ) -> Result<f64, InteractionRejection> {
        let record = plan
            .tab_bar_records()
            .iter()
            .find(|record| *record.id() == key.bar())
            .ok_or(InteractionRejection::TabStripSourceUnavailable { key })?;
        if record != expected_record {
            return Err(InteractionRejection::TabStripRecordChanged { key });
        }
        if record.interaction() != TabBarInteraction::Enabled {
            return Err(InteractionRejection::TabStripScrollDisabled { key });
        }
        if self
            .presentation_authority
            .tab_strip_states
            .state(key)
            .is_none()
        {
            return Err(InteractionRejection::TabStripSourceUnavailable { key });
        }
        let member = |item: ItemId| {
            record
                .members()
                .iter()
                .find(|member| member.tab().item == item)
                .copied()
                .ok_or(InteractionRejection::TabStripScrollItemUnavailable { key, item })
        };
        match &adjustment.0 {
            TabScrollAdjustmentKind::RevealItem(item) => {
                let member = member(*item)?;
                Ok(reveal_scroll_offset(
                    record.scroll_offset(),
                    record.maximum_scroll_offset(),
                    member.full_bounds().x(),
                    member.full_bounds().max().x(),
                    record.viewport().x(),
                    record.viewport().max().x(),
                ))
            }
            TabScrollAdjustmentKind::ScrollByPreserving {
                delta,
                keep_visible,
            } => {
                let ranges = keep_visible
                    .iter()
                    .copied()
                    .map(member)
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .map(|member| {
                        operable_tab_scroll_range(
                            member,
                            record,
                            self.policy
                                .pane_close_capability(member.tab().item)
                                .allows_close(),
                            &self.presentation_authority.presentation_config,
                        )
                    });
                Ok(scroll_offset_preserving_ranges(
                    (record.scroll_offset() + *delta).clamp(0.0, record.maximum_scroll_offset()),
                    record.maximum_scroll_offset(),
                    ranges,
                ))
            }
        }
    }

    fn resolve_tab_list_menu_scroll(
        &self,
        plan: &PresentationPlan,
        session: crate::tab_strip::TabListMenuSessionId,
        revision: PopupRoutingRevision,
        expected_record: &TabListMenuRecord,
        adjustment: &TabScrollAdjustment,
    ) -> Result<f64, InteractionRejection> {
        self.validate_tab_list_menu_popup(plan, session, revision)?;
        let record = plan
            .tab_list_menu_records()
            .iter()
            .find(|record| record.session() == session)
            .ok_or(InteractionRejection::TabListMenuSessionUnavailable { session })?;
        if record != expected_record {
            return Err(InteractionRejection::TabListMenuRecordChanged { session });
        }
        if !self
            .presentation_authority
            .tab_strip_states
            .active_menu_for(session.key())
            .is_some_and(|active| active.session() == session)
        {
            return Err(InteractionRejection::TabListMenuSessionUnavailable { session });
        }
        let row = |item: ItemId| {
            record
                .rows()
                .iter()
                .find(|row| row.tab().item == item)
                .copied()
                .ok_or(InteractionRejection::TabListMenuScrollItemUnavailable { session, item })
        };
        match &adjustment.0 {
            TabScrollAdjustmentKind::RevealItem(item) => {
                let row = row(*item)?;
                Ok(reveal_scroll_offset(
                    record.scroll_offset(),
                    record.maximum_scroll_offset(),
                    row.bounds().y(),
                    row.bounds().max().y(),
                    record.viewport().y(),
                    record.viewport().max().y(),
                ))
            }
            TabScrollAdjustmentKind::ScrollByPreserving {
                delta,
                keep_visible,
            } => {
                let ranges = keep_visible
                    .iter()
                    .copied()
                    .map(row)
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .map(|row| {
                        let bounds = row.bounds();
                        fully_visible_scroll_range(
                            bounds.y() - record.viewport().y() + record.scroll_offset(),
                            bounds.max().y() - record.viewport().y() + record.scroll_offset(),
                            record.viewport().height(),
                            record.maximum_scroll_offset(),
                        )
                    });
                Ok(scroll_offset_preserving_ranges(
                    (record.scroll_offset() + *delta).clamp(0.0, record.maximum_scroll_offset()),
                    record.maximum_scroll_offset(),
                    ranges,
                ))
            }
        }
    }

    fn resolve_tab_list_menu_navigation(
        &self,
        plan: &PresentationPlan,
        session: crate::tab_strip::TabListMenuSessionId,
        revision: PopupRoutingRevision,
        expected_record: &TabListMenuRecord,
        target: ItemId,
    ) -> Result<f64, InteractionRejection> {
        self.validate_tab_list_menu_popup(plan, session, revision)?;
        let record = plan
            .tab_list_menu_records()
            .iter()
            .find(|record| record.session() == session)
            .ok_or(InteractionRejection::TabListMenuSessionUnavailable { session })?;
        if record != expected_record {
            return Err(InteractionRejection::TabListMenuRecordChanged { session });
        }
        let active = self
            .presentation_authority
            .tab_strip_states
            .active_menu_for(session.key())
            .filter(|active| active.session() == session)
            .ok_or(InteractionRejection::TabListMenuSessionUnavailable { session })?;
        if !active.items().contains(&target) {
            return Err(InteractionRejection::TabListMenuFocusItemUnavailable {
                session,
                item: target,
            });
        }
        let row = record
            .rows()
            .iter()
            .find(|row| row.tab().item == target)
            .ok_or(InteractionRejection::TabListMenuFocusItemUnavailable {
                session,
                item: target,
            })?;
        Ok(reveal_scroll_offset(
            record.scroll_offset(),
            record.maximum_scroll_offset(),
            row.bounds().y(),
            row.bounds().max().y(),
            record.viewport().y(),
            record.viewport().max().y(),
        ))
    }
}

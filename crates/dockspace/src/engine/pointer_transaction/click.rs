use super::*;

impl DockEngine {
    pub(super) fn finish_journal_click_release(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        owner: GestureOwner,
        session: ClickSessionId,
        edge: &PointerEdge,
        receipt: &ValidatedPointerReceiverReceipt,
        snapshot: &JournalPresentationSnapshot,
        desktop_route: Option<DesktopRouteValidation>,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        if self
            .interaction
            .active_click(session)
            .is_ok_and(|click| matches!(click.action.as_ref(), FrozenClickAction::Touch))
        {
            return self.finish_journal_touch_release(
                cause,
                focus_causal,
                owner,
                session,
                edge,
                receipt,
                snapshot,
                desktop_route,
                policy,
                events,
                interaction_events,
            );
        }
        let click =
            match self
                .interaction
                .take_click_for_release(session, owner, PointerButton::Primary)
            {
                Ok(click) => click,
                Err(rejection) => return Ok(InteractionOutcome::Rejected(rejection)),
            };
        let pressed = InteractionStatus::Pressed { session };
        let (region, presentation) =
            match Self::journal_click_delivery(cause, edge, receipt, snapshot, desktop_route)? {
                JournalClickDelivery::Dock(region, presentation) => (region, presentation),
                JournalClickDelivery::KnownMismatch => {
                    return Ok(Self::record_terminal_click_cancellation(
                        cause,
                        self.version,
                        pressed,
                        InteractionCancelReason::ClickReceiverMismatch,
                        interaction_events,
                    ));
                }
                JournalClickDelivery::Blocked => {
                    return Ok(Self::record_terminal_click_cancellation(
                        cause,
                        self.version,
                        pressed,
                        InteractionCancelReason::OpaquePointerBlocker,
                        interaction_events,
                    ));
                }
                JournalClickDelivery::Unknown => {
                    return Ok(Self::record_terminal_click_cancellation(
                        cause,
                        self.version,
                        pressed,
                        InteractionCancelReason::UnknownTargetAuthority,
                        interaction_events,
                    ));
                }
            };
        if region != click.region
            || presentation.scene().requirement() != click.measurement
            || !presentation
                .coordinate_capture()
                .same_projection_authority(click.coordinates)
        {
            return Ok(Self::record_terminal_click_cancellation(
                cause,
                self.version,
                pressed,
                InteractionCancelReason::ClickReceiverMismatch,
                interaction_events,
            ));
        }
        if let Some(touch) = click.touch.as_ref()
            && self
                .journal_logical_point(edge, desktop_route)
                .is_some_and(|point| {
                    crate::engine::pointer_touch::exceeds_touch_slop(
                        touch.initial_pointer,
                        point,
                        self.presentation_authority
                            .presentation_config
                            .touch_press_slop_distance(),
                    )
                })
        {
            return Ok(InteractionOutcome::TouchYieldedToContent {
                pointer: owner.pointer(),
                surface: click.region.surface(),
            });
        }
        if matches!(
            click.action.as_ref(),
            FrozenClickAction::TabListMenuRow(_)
                | FrozenClickAction::TabListMenuBlocker(_)
                | FrozenClickAction::TabListMenuBackdrop(_)
        ) && (!click
            .presentation
            .presented
            .same_interaction_semantics(presentation.authority())
            || click.presentation.popup_revision != presentation.popup_revision()
            || !self.journal_popup_presentation_is_exact_current(presentation))
        {
            return Ok(Self::record_terminal_click_cancellation(
                cause,
                self.version,
                pressed,
                InteractionCancelReason::ClickReceiverMismatch,
                interaction_events,
            ));
        }

        match click.action.as_ref() {
            FrozenClickAction::Touch => Err(EngineError::ReductionCauseInvariant {
                detail: "touch release bypassed its exact pressed owner",
            }),
            FrozenClickAction::Close(close) => {
                self.finish_journal_close_click(cause, pressed, close, policy, interaction_events)
            }
            FrozenClickAction::TabStripControl(control) => {
                self.finish_journal_tab_strip_control(cause, control, presentation.plan())
            }
            FrozenClickAction::TabListMenuRow(row) => self.finish_tab_list_menu_row(
                cause,
                TabListMenuActivation::Journal(focus_causal),
                row,
                presentation.plan(),
                policy,
                events,
            ),
            FrozenClickAction::TabListMenuBlocker(blocker) => {
                self.finish_journal_tab_list_menu_blocker(blocker, presentation.plan())
            }
            FrozenClickAction::TabListMenuBackdrop(backdrop) => {
                self.finish_journal_tab_list_menu_backdrop(cause, backdrop, presentation.plan())
            }
        }
    }

    fn finish_journal_close_click(
        &mut self,
        cause: ReductionCause,
        pressed: InteractionStatus,
        close: &FrozenCloseClick,
        policy: &DockPolicySnapshot,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let current_target = match close.target {
            ClosePlanTarget::Item { item } => ContentCloseTarget::Item(item),
            ClosePlanTarget::Root { root } => ContentCloseTarget::Root(root),
            ClosePlanTarget::Surface { .. } => {
                return Err(EngineError::ReductionCauseInvariant {
                    detail: "journal content click froze a surface close target",
                });
            }
        };
        let current = match self.capture_content_close(current_target, policy) {
            Ok(current) => current,
            Err(_) => {
                return Ok(Self::record_terminal_click_cancellation(
                    cause,
                    self.version,
                    pressed,
                    InteractionCancelReason::SourceVanished,
                    interaction_events,
                ));
            }
        };
        if current.target != close.target
            || current.requirements != close.requirements
            || current.prepared != close.prepared
            || prepare_content_close(&self.workspace, policy, &close.prepared).is_err()
        {
            return Ok(Self::record_terminal_click_cancellation(
                cause,
                self.version,
                pressed,
                InteractionCancelReason::SourceVanished,
                interaction_events,
            ));
        }

        let authority = self.close_authority();
        if let Some(existing) = self
            .close
            .active_plan_for_target(close.target, authority)
            .cloned()
        {
            return Ok(InteractionOutcome::CloseRequested {
                plan: existing,
                reused: true,
            });
        }
        let plan = self
            .close
            .open(
                authority,
                close.target,
                close.requirements.clone(),
                PreparedCloseOperation::Content(close.prepared.clone()),
            )
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: source.to_string(),
            })?;
        Ok(InteractionOutcome::CloseRequested {
            plan,
            reused: false,
        })
    }

    fn finish_journal_tab_list_menu_blocker(
        &self,
        frozen: &FrozenTabListMenuBlockerClick,
        plan: &PresentationPlan,
    ) -> Result<InteractionOutcome, EngineError> {
        let session = frozen.session;
        if let Err(rejection) = self.validate_tab_list_menu_popup(plan, session, frozen.revision) {
            return Ok(InteractionOutcome::Rejected(rejection));
        }
        let Some(current) = plan
            .tab_list_menu_records()
            .iter()
            .find(|record| record.session() == session)
        else {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::TabListMenuBlockerUnavailable { session },
            ));
        };
        if current != &frozen.record {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::TabListMenuBlockerRecordChanged { session },
            ));
        }
        Ok(InteractionOutcome::TabListMenuFrameConsumed { session })
    }

    pub(in crate::engine) fn finish_journal_tab_list_menu_backdrop(
        &mut self,
        cause: ReductionCause,
        frozen: &FrozenTabListMenuBackdropClick,
        plan: &PresentationPlan,
    ) -> Result<InteractionOutcome, EngineError> {
        let session = frozen.session;
        if let Err(rejection) = self.validate_tab_list_menu_popup(plan, session, frozen.revision) {
            return Ok(InteractionOutcome::Rejected(rejection));
        }
        let Some(current) = plan
            .tab_list_menu_backdrop_records()
            .iter()
            .copied()
            .find(|record| record.session() == session)
        else {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::TabListMenuBackdropUnavailable { session },
            ));
        };
        if current != frozen.record {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::TabListMenuBackdropRecordChanged { session },
            ));
        }

        let mut tab_strip_states = self.presentation_authority.tab_strip_states.clone();
        let delta = tab_strip_states
            .close_tab_list_menu(session)
            .map_err(|source| Self::tab_strip_state_invariant(cause, source))?;
        self.presentation_authority.tab_strip_states = tab_strip_states;
        self.consume_tab_strip_state_delta(cause, &delta)?;
        Ok(InteractionOutcome::TabListMenuDismissed { session })
    }

    fn record_terminal_click_cancellation(
        cause: ReductionCause,
        version: WorkspaceVersion,
        status: InteractionStatus,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> InteractionOutcome {
        interaction_events.push(InteractionEvent::new_caused(
            cause,
            version,
            InteractionEventKind::Cancelled { status, reason },
        ));
        InteractionOutcome::Cancelled { status, reason }
    }

    pub(super) fn arm_journal_click(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        capture_authority: Authority<PointerCaptureOwner>,
        presentation: &JournalSurfacePresentation,
        region: PresentationHitRegionId,
        point: crate::geometry::LogicalPoint,
        policy: &DockPolicySnapshot,
    ) -> Result<Option<InteractionOutcome>, EngineError> {
        if !self.presented_receiver_is_current(presentation.plan(), region) {
            return Ok(Some(InteractionOutcome::Rejected(
                InteractionRejection::StaleScene,
            )));
        }
        if presentation.scene().requirement().workspace_epoch() != self.version.epoch() {
            return Ok(Some(InteractionOutcome::Rejected(
                InteractionRejection::StaleScene,
            )));
        }
        if is_tab_list_menu_click(region.kind())
            && !self.journal_popup_presentation_is_exact_current(presentation)
        {
            return Ok(Some(InteractionOutcome::Rejected(
                InteractionRejection::StaleScene,
            )));
        }
        let action = match self.freeze_journal_click_action(
            presentation.plan(),
            presentation.surface(),
            region,
            point,
            policy,
        ) {
            Ok(action) => action,
            Err(rejection) => return Ok(Some(InteractionOutcome::Rejected(rejection))),
        };
        let (session, replaced) = self
            .interaction
            .begin_click(ClickStart {
                epoch: self.version.epoch(),
                owner,
                capture_authority,
                button: PointerButton::Primary,
                region,
                measurement: presentation.scene().requirement(),
                coordinates: presentation.coordinate_capture(),
                presentation: Self::freeze_journal_presentation(presentation),
                action: Arc::new(action),
            })
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: source.to_string(),
            })?;
        if replaced.is_some() {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "journal click press replaced an active gesture after idle validation",
            });
        }
        debug_assert_eq!(
            self.interaction.status(),
            InteractionStatus::Pressed { session }
        );
        Ok(None)
    }

    pub(in crate::engine) fn freeze_journal_click_action(
        &self,
        plan: &PresentationPlan,
        surface: SurfaceId,
        region: PresentationHitRegionId,
        point: crate::geometry::LogicalPoint,
        policy: &DockPolicySnapshot,
    ) -> Result<FrozenClickAction, InteractionRejection> {
        match region.kind() {
            PresentationHitRegionKind::TabClose(tab) => self
                .freeze_journal_close_click(plan, point, policy, CloseSceneTarget::Tab(tab))
                .map(FrozenClickAction::Close),
            PresentationHitRegionKind::ContainedClose(floating) => self
                .freeze_journal_close_click(
                    plan,
                    point,
                    policy,
                    CloseSceneTarget::Contained(floating),
                )
                .map(FrozenClickAction::Close),
            PresentationHitRegionKind::TabStripControl(control) => {
                let record = plan
                    .tab_strip_control_records()
                    .iter()
                    .copied()
                    .find(|record| record.id() == control)
                    .ok_or(InteractionRejection::TabStripControlUnavailable { control })?;
                if !record.hit().contains(point) {
                    return Err(InteractionRejection::TabStripControlHitMismatch { control });
                }
                if !record.enabled() {
                    return Err(InteractionRejection::TabStripControlDisabled { control });
                }
                let key = TabStripStateKey::new(surface, control.bar());
                if self
                    .presentation_authority
                    .tab_strip_states
                    .state(key)
                    .is_none()
                    || !control.has_source_in(plan)
                {
                    return Err(InteractionRejection::TabStripSourceUnavailable { key });
                }
                Ok(FrozenClickAction::TabStripControl(
                    FrozenTabStripControlClick { key, record },
                ))
            }
            PresentationHitRegionKind::TabListMenuRow { menu, tab } => {
                let revision = plan.popup().revision();
                self.validate_tab_list_menu_popup(plan, menu, revision)?;
                let menu_record = plan
                    .tab_list_menu_records()
                    .iter()
                    .find(|record| record.session() == menu)
                    .ok_or(InteractionRejection::TabListMenuSessionUnavailable { session: menu })?;
                let record = menu_record
                    .rows()
                    .iter()
                    .copied()
                    .find(|record| record.tab() == tab)
                    .ok_or(InteractionRejection::TabListMenuRowUnavailable {
                        session: menu,
                        tab,
                    })?;
                if !record.hit().is_some_and(|hit| hit.contains(point)) {
                    return Err(InteractionRejection::TabListMenuRowHitMismatch {
                        session: menu,
                        tab,
                    });
                }
                let active = self
                    .presentation_authority
                    .tab_strip_states
                    .active_menu_for(menu.key())
                    .ok_or(InteractionRejection::TabListMenuSessionUnavailable { session: menu })?;
                if active.session() != menu || !active.items().contains(&tab.item) {
                    return Err(InteractionRejection::TabListMenuSessionUnavailable {
                        session: menu,
                    });
                }
                self.workspace
                    .capture_item_source(tab.root, tab.tabs, tab.item)
                    .map_err(InteractionRejection::CommandRejected)?;
                Ok(FrozenClickAction::TabListMenuRow(
                    FrozenTabListMenuRowClick {
                        session: menu,
                        record,
                        revision,
                    },
                ))
            }
            PresentationHitRegionKind::TabListMenuBlocker(session) => {
                let revision = plan.popup().revision();
                self.validate_tab_list_menu_popup(plan, session, revision)?;
                let record = plan
                    .tab_list_menu_records()
                    .iter()
                    .find(|record| record.session() == session)
                    .cloned()
                    .ok_or(InteractionRejection::TabListMenuBlockerUnavailable { session })?;
                if !record.bounds().contains(point) {
                    return Err(InteractionRejection::TabListMenuBlockerHitMismatch { session });
                }
                Ok(FrozenClickAction::TabListMenuBlocker(
                    FrozenTabListMenuBlockerClick {
                        session,
                        record,
                        revision,
                    },
                ))
            }
            PresentationHitRegionKind::TabListMenuBackdrop(session) => {
                let revision = plan.popup().revision();
                self.validate_tab_list_menu_popup(plan, session, revision)?;
                let record = plan
                    .tab_list_menu_backdrop_records()
                    .iter()
                    .copied()
                    .find(|record| record.session() == session)
                    .ok_or(InteractionRejection::TabListMenuBackdropUnavailable { session })?;
                if record.revision() != revision {
                    return Err(InteractionRejection::TabListMenuBackdropRecordChanged { session });
                }
                if !record.bounds().contains(point) {
                    return Err(InteractionRejection::TabListMenuBackdropHitMismatch { session });
                }
                Ok(FrozenClickAction::TabListMenuBackdrop(
                    FrozenTabListMenuBackdropClick {
                        session,
                        record,
                        revision,
                    },
                ))
            }
            _ => Err(InteractionRejection::TargetAuthorityInvalid),
        }
    }

    fn journal_popup_presentation_is_exact_current(
        &self,
        presentation: &JournalSurfacePresentation,
    ) -> bool {
        self.presentation_authority
            .scene
            .interaction_projection(presentation.surface())
            .is_some_and(|current| {
                current.plan_stamp() == presentation.scene()
                    && current.output_ticket() == presentation.output_ticket()
                    && current.authority() == presentation.authority()
                    && current.popup_revision() == presentation.popup_revision()
                    && current
                        .output()
                        .coordinate_capture()
                        .same_projection_authority(presentation.coordinate_capture())
                    && Self::coordinate_capture_matches_current(
                        presentation.coordinate_capture(),
                        self.viewport.viewport(presentation.surface()),
                        self.viewport
                            .surface_coordinate_authority(presentation.surface()),
                    )
            })
    }

    fn freeze_journal_close_click(
        &self,
        plan: &PresentationPlan,
        point: crate::geometry::LogicalPoint,
        policy: &DockPolicySnapshot,
        target: CloseSceneTarget,
    ) -> Result<FrozenCloseClick, InteractionRejection> {
        let (close_bounds, layer, content_target) = match target {
            CloseSceneTarget::Tab(id) => {
                let Some(record) = plan.tab_records().iter().find(|record| *record.id() == id)
                else {
                    return Err(InteractionRejection::CloseSceneTargetUnavailable { target });
                };
                let Some(close_bounds) = record.close_bounds() else {
                    return Err(InteractionRejection::CloseControlUnavailable { target });
                };
                (
                    close_bounds,
                    record.layer(),
                    ContentCloseTarget::Item(id.item),
                )
            }
            CloseSceneTarget::Contained(floating) => {
                let Some(record) = plan.contained_record(floating) else {
                    return Err(InteractionRejection::CloseSceneTargetUnavailable { target });
                };
                let Some(close_bounds) = record.close_bounds() else {
                    return Err(InteractionRejection::CloseControlUnavailable { target });
                };
                (
                    close_bounds,
                    record.layer(),
                    ContentCloseTarget::Root(record.root()),
                )
            }
        };
        if !close_bounds.contains(point) {
            return Err(InteractionRejection::CloseActivationHitMismatch { target });
        }
        if !plan.point_is_on_authoritative_layer(point, layer) {
            return Err(InteractionRejection::CloseActivationOccluded { target });
        }
        let capture = match self.capture_content_close(content_target, policy) {
            Ok(capture) => capture,
            Err(rejection) => {
                return Err(Self::content_close_rejection_to_interaction(
                    target, rejection,
                ));
            }
        };
        Ok(capture)
    }
}

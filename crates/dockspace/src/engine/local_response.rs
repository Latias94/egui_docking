//! Current-frame framework response actions over an exact Ready candidate.

use super::*;

impl DockEngine {
    pub(super) fn reduce_pane_focus_request_observation(
        &mut self,
        expected_epoch: crate::ids::WorkspaceEpoch,
        observation: PaneFocusRequestObservation,
    ) -> InputOutcome {
        if expected_epoch != self.version.epoch() {
            return InputOutcome::PaneFocusObservationStale {
                expected_epoch,
                current_epoch: self.version.epoch(),
            };
        }
        let current_binding = self.viewport_focus_binding(observation.surface());
        let items = self.surface_items(observation.surface());
        let transition = self.viewport_focus.publish_pane_focus_request_observation(
            observation,
            |binding| current_binding == Some(binding),
            |surface, item| surface == observation.surface() && items.contains(&item),
        );
        InputOutcome::PaneFocusObservationPublished { transition }
    }

    pub(super) fn local_response_candidate(
        &self,
        scene: SurfaceSceneStamp,
    ) -> Result<&crate::scene::SurfacePlanScene, InteractionRejection> {
        let candidate = self
            .presentation_authority
            .scene
            .surface(scene.surface())
            .and_then(SurfaceScene::ready)
            .map(crate::scene::ReadySurfaceScene::candidate)
            .filter(|candidate| candidate.stamp() == scene)
            .filter(|candidate| {
                candidate.stamp().requirement().workspace_epoch() == self.version.epoch()
            })
            .ok_or(InteractionRejection::StaleScene)?;
        Ok(candidate)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn reduce_local_contained_activation(
        &mut self,
        input: InputSequence,
        focus_causal: FocusCausalStamp,
        expected: WorkspaceVersion,
        application_base: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        floating: crate::ids::FloatingPresentationId,
        point: LogicalPoint,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != application_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: application_base,
            });
        }

        let (source, expected_roster, selection, focus_item) = {
            let candidate = match self.local_response_candidate(scene) {
                Ok(candidate) => candidate,
                Err(error) => return Ok(self.local_response_rejection(error)),
            };
            let Some(contained) = candidate.plan().contained_record(floating) else {
                return Ok(self.local_response_rejection(
                    InteractionRejection::ContainedTransformPresentationUnavailable {
                        surface: scene.surface(),
                        floating,
                    },
                ));
            };
            if contained.is_frontmost()
                || !contained.outer_bounds().contains(point)
                || !candidate
                    .plan()
                    .point_is_on_authoritative_layer(point, contained.layer())
            {
                return Ok(self.local_response_rejection(
                    InteractionRejection::SemanticReceiverUnavailable {
                        target: PresentationHitRegionKind::ContainedFrameBlocker(floating),
                    },
                ));
            }
            let root = contained.root();
            if self.root_has_pending_presentation_transition(root) {
                return Ok(self.local_response_rejection(
                    InteractionRejection::PresentationTransitionPending,
                ));
            }
            if self.workspace.presentation_for_root(root)
                != Some(crate::RootPresentationOwner::Contained {
                    surface: scene.surface(),
                    floating,
                })
            {
                return Ok(self.local_response_rejection(
                    InteractionRejection::ContainedTransformPresentationUnavailable {
                        surface: scene.surface(),
                        floating,
                    },
                ));
            }
            let Some(root_record) = self.workspace.root(root) else {
                return Ok(self.local_response_rejection(
                    InteractionRejection::ContainedTransformPresentationUnavailable {
                        surface: scene.surface(),
                        floating,
                    },
                ));
            };
            let source = match self.workspace.capture_node_source(root, root_record.node) {
                Ok(source) => source,
                Err(error) => {
                    return Ok(
                        self.local_response_rejection(InteractionRejection::CommandRejected(error))
                    );
                }
            };
            let expected_roster = match self.workspace.capture_contained_roster(scene.surface()) {
                Ok(roster) => roster,
                Err(error) => {
                    return Ok(
                        self.local_response_rejection(InteractionRejection::CommandRejected(error))
                    );
                }
            };
            let hit = match candidate
                .hit_manifest()
                .resolve_exclusive(PresentationPointerLane::Click, point)
            {
                Ok(hit) => hit.map(|hit| hit.id().kind()),
                Err(_) => {
                    return Ok(self.local_response_rejection(
                        InteractionRejection::SemanticReceiverUnavailable {
                            target: PresentationHitRegionKind::ContainedFrameBlocker(floating),
                        },
                    ));
                }
            };
            let (selection, focus_item) = match hit {
                Some(PresentationHitRegionKind::TabBody(tab)) if tab.root == root => {
                    let selection = match self
                        .workspace
                        .capture_item_source(tab.root, tab.tabs, tab.item)
                    {
                        Ok(source) => Some(source),
                        Err(error) => {
                            return Ok(self.local_response_rejection(
                                InteractionRejection::CommandRejected(error),
                            ));
                        }
                    };
                    (selection, Some(tab.item))
                }
                Some(PresentationHitRegionKind::PaneBody(pane)) if pane.root == root => {
                    let item = candidate
                        .plan()
                        .pane_records()
                        .iter()
                        .find(|record| record.id() == pane)
                        .and_then(crate::scene::PaneRecord::selected);
                    (None, item)
                }
                _ => (None, None),
            };
            (source, expected_roster, selection, focus_item)
        };

        let raise = WorkspaceCommand::RaiseContained {
            source,
            floating,
            expected_roster,
        };
        let outcome = if let Some(selection) = selection {
            self.reduce_local_contained_tab_select(
                input,
                raise,
                WorkspaceCommand::Select { source: selection },
                policy,
                events,
                interaction_events,
            )?
        } else {
            self.reduce_workspace_command(
                input,
                expected,
                application_base,
                &raise,
                policy,
                events,
                interaction_events,
            )?
        };
        if matches!(&outcome, InputOutcome::CommandProcessed { .. })
            && let Some(item) = focus_item
        {
            let native_guard = self.viewport_focus_binding(scene.surface());
            let _ = self
                .viewport_focus
                .request_local_pane_focus(scene.surface(), item, native_guard, focus_causal)
                .map_err(|source| EngineError::CausedViewportFocus {
                    cause: focus_causal.cause(),
                    source,
                })?;
        }
        Ok(outcome)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn reduce_local_scene_tab_select(
        &mut self,
        input: InputSequence,
        focus_causal: FocusCausalStamp,
        expected: WorkspaceVersion,
        application_base: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        tab: crate::scene::TabSceneId,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != application_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: application_base,
            });
        }

        let (source, raise) = {
            let candidate = match self.local_response_candidate(scene) {
                Ok(candidate) => candidate,
                Err(error) => return Ok(self.local_response_rejection(error)),
            };
            let Some(record) = candidate
                .plan()
                .tab_records()
                .iter()
                .find(|record| *record.id() == tab)
            else {
                return Ok(self.local_response_rejection(
                    InteractionRejection::TabGestureSourceUnavailable {
                        source: TabGestureSource::Item(tab),
                    },
                ));
            };
            if !candidate
                .plan()
                .region_is_operable(record.drag_hit().rect(), record.layer())
            {
                return Ok(self.local_response_rejection(
                    InteractionRejection::SemanticReceiverUnavailable {
                        target: PresentationHitRegionKind::TabBody(tab),
                    },
                ));
            }
            let source = match self
                .workspace
                .capture_item_source(tab.root, tab.tabs, tab.item)
            {
                Ok(source) => source,
                Err(_) => {
                    return Ok(self.local_response_rejection(
                        InteractionRejection::TabGestureSourceUnavailable {
                            source: TabGestureSource::Item(tab),
                        },
                    ));
                }
            };
            if self.root_has_pending_presentation_transition(tab.root) {
                return Ok(self.local_response_rejection(
                    InteractionRejection::PresentationTransitionPending,
                ));
            }
            let raise = match self.workspace.presentation_for_root(tab.root) {
                Some(crate::RootPresentationOwner::Main { surface })
                    if surface == scene.surface() =>
                {
                    None
                }
                Some(crate::RootPresentationOwner::Contained { surface, floating })
                    if surface == scene.surface() =>
                {
                    let Some(contained) = candidate.plan().contained_record(floating) else {
                        return Ok(self.local_response_rejection(
                            InteractionRejection::TabGestureSourceUnavailable {
                                source: TabGestureSource::Item(tab),
                            },
                        ));
                    };
                    if contained.root() != tab.root || contained.layer() != record.layer() {
                        return Ok(self.local_response_rejection(
                            InteractionRejection::TabGestureSourceUnavailable {
                                source: TabGestureSource::Item(tab),
                            },
                        ));
                    }
                    if contained.is_frontmost() {
                        None
                    } else {
                        let Some(root) = self.workspace.root(tab.root) else {
                            return Ok(self.local_response_rejection(
                                InteractionRejection::TabGestureSourceUnavailable {
                                    source: TabGestureSource::Item(tab),
                                },
                            ));
                        };
                        let source = match self.workspace.capture_node_source(tab.root, root.node) {
                            Ok(source) => source,
                            Err(error) => {
                                return Ok(self.local_response_rejection(
                                    InteractionRejection::CommandRejected(error),
                                ));
                            }
                        };
                        let expected_roster = match self.workspace.capture_contained_roster(surface)
                        {
                            Ok(roster) => roster,
                            Err(error) => {
                                return Ok(self.local_response_rejection(
                                    InteractionRejection::CommandRejected(error),
                                ));
                            }
                        };
                        Some(WorkspaceCommand::RaiseContained {
                            source,
                            floating,
                            expected_roster,
                        })
                    }
                }
                _ => {
                    return Ok(self.local_response_rejection(
                        InteractionRejection::TabGestureSourceUnavailable {
                            source: TabGestureSource::Item(tab),
                        },
                    ));
                }
            };
            (source, raise)
        };

        let selection = WorkspaceCommand::Select { source };
        let outcome = if let Some(raise) = raise {
            self.reduce_local_contained_tab_select(
                input,
                raise,
                selection,
                policy,
                events,
                interaction_events,
            )?
        } else {
            self.reduce_workspace_command(
                input,
                expected,
                application_base,
                &selection,
                policy,
                events,
                interaction_events,
            )?
        };
        if matches!(&outcome, InputOutcome::CommandProcessed { .. }) {
            let native_guard = self.viewport_focus_binding(scene.surface());
            let _ = self
                .viewport_focus
                .request_local_pane_focus(scene.surface(), tab.item, native_guard, focus_causal)
                .map_err(|source| EngineError::CausedViewportFocus {
                    cause: focus_causal.cause(),
                    source,
                })?;
        }
        Ok(outcome)
    }

    #[allow(clippy::too_many_arguments)]
    fn reduce_local_contained_tab_select(
        &mut self,
        input: InputSequence,
        raise: WorkspaceCommand,
        selection: WorkspaceCommand,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        let mut workspace = self.clone_workspace_candidate();
        let report = match WorkspaceTransaction::from_commands([raise, selection])
            .apply(&mut workspace, policy)
        {
            Ok(report) => report,
            Err(TransactionError::Command { source, .. }) if source.is_expected_rejection() => {
                return Ok(InputOutcome::CommandRejected {
                    error: source,
                    version: self.version,
                });
            }
            Err(source) => return Err(EngineError::Command { input, source }),
        };
        if let Some(surface) = self.first_pending_presentation_source_mismatch(
            &workspace,
            WorkspacePublicationAuthority::Ordinary,
        ) {
            return Ok(InputOutcome::CommandRejected {
                error: CommandError::SurfaceLifecycleFrozen { surface },
                version: self.version,
            });
        }
        if let Some(surface) = self.first_workspace_publication_mismatch(&workspace, None, None) {
            return Ok(InputOutcome::CommandRejected {
                error: CommandError::SurfaceLifecycleFrozen { surface },
                version: self.version,
            });
        }
        let changed = report.changed();
        let outcomes = report.into_outcomes();
        let selection = outcomes
            .iter()
            .find(|outcome| matches!(outcome, CommandOutcome::Selected { .. }))
            .cloned()
            .ok_or(EngineError::MissingCommandOutcome { input })?;
        let publication = match self.stage_workspace_publication(workspace, policy) {
            Ok(publication) => publication,
            Err(source) if source.is_expected_rejection() => {
                return Ok(InputOutcome::CommandRejected {
                    error: source,
                    version: self.version,
                });
            }
            Err(source) => {
                return Err(EngineError::Command {
                    input,
                    source: TransactionError::Command { index: 0, source },
                });
            }
        };
        self.publish_workspace(publication);
        self.reconcile_viewport_focus_authority();
        if changed {
            self.advance_revision(input)?;
            self.invalidate_transient(
                input,
                InteractionCancelReason::WorkspaceChanged,
                interaction_events,
            )?;
            events.extend(
                outcomes
                    .into_iter()
                    .filter(CommandOutcome::changes_workspace)
                    .map(|outcome| {
                        WorkspaceEvent::new(
                            input,
                            self.version,
                            WorkspaceEventKind::CommandCommitted(outcome),
                        )
                    }),
            );
        }
        Ok(InputOutcome::CommandProcessed {
            outcome: selection,
            changed,
            version: self.version,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn reduce_local_tab_chrome_action(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        expected: WorkspaceVersion,
        application_base: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        action: &LocalTabChromeAction,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != application_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: application_base,
            });
        }
        let plan = match self.local_response_candidate(scene) {
            Ok(candidate) => candidate.plan().clone(),
            Err(error) => return Ok(self.local_response_rejection(error)),
        };
        let outcome = match action {
            LocalTabChromeAction::ActivateControl(control) => {
                self.finish_journal_tab_strip_control(cause, control, &plan)?
            }
            LocalTabChromeAction::ScrollStrip {
                key,
                record,
                adjustment,
            } => self.finish_tab_strip_scroll(cause, &plan, *key, record, adjustment)?,
            LocalTabChromeAction::ActivateMenuRow(row) => self.finish_journal_tab_list_menu_row(
                cause,
                focus_causal,
                row,
                &plan,
                policy,
                events,
            )?,
            LocalTabChromeAction::DismissMenu {
                session,
                revision,
                record,
                backdrop,
            } => self.finish_tab_list_menu_dismiss(
                cause, &plan, *session, *revision, record, *backdrop,
            )?,
            LocalTabChromeAction::ScrollMenu {
                session,
                revision,
                record,
                adjustment,
            } => self.finish_tab_list_menu_scroll(
                cause, &plan, *session, *revision, record, adjustment,
            )?,
            LocalTabChromeAction::NavigateMenu {
                session,
                revision,
                record,
                target,
            } => self.finish_tab_list_menu_navigation(
                cause, &plan, *session, *revision, record, *target,
            )?,
        };
        Ok(InputOutcome::InteractionProcessed {
            outcome,
            version: self.version,
        })
    }

    pub(super) fn reduce_local_scene_close_input(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        target: CloseSceneTarget,
        policy: &DockPolicySnapshot,
    ) -> Result<InputOutcome, EngineError> {
        self.reduce_versioned_interaction(expected, |engine| {
            engine.request_close_plan(input, scene, target, CloseActivation::LocalResponse, policy)
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn reduce_local_contained_dock_back_input(
        &mut self,
        input: InputSequence,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        expected: WorkspaceVersion,
        application_base: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        floating: FloatingPresentationId,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != application_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: application_base,
            });
        }
        let root = {
            let candidate = match self.local_response_candidate(scene) {
                Ok(candidate) => candidate,
                Err(error) => return Ok(self.local_response_rejection(error)),
            };
            let target = CloseSceneTarget::Contained(floating);
            let Some(record) = candidate.plan().contained_record(floating) else {
                return Ok(self.local_response_rejection(
                    InteractionRejection::CloseSceneTargetUnavailable { target },
                ));
            };
            let Some(close_bounds) = record.close_bounds() else {
                return Ok(self.local_response_rejection(
                    InteractionRejection::CloseControlUnavailable { target },
                ));
            };
            if !candidate
                .plan()
                .region_is_operable(close_bounds, record.layer())
            {
                return Ok(self.local_response_rejection(
                    InteractionRejection::CloseActivationOccluded { target },
                ));
            }
            record.root()
        };
        self.reduce_product_action(
            input,
            cause,
            focus_causal,
            expected,
            ProductAction::DockBackRoot { root },
            policy,
            events,
            interaction_events,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn reduce_local_splitter_adjustment_input(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        application_base: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        splitter: SplitterSceneId,
        delta: f64,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != application_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: application_base,
            });
        }
        let outcome = self.adjust_splitter_resize(
            input,
            scene,
            splitter,
            delta,
            SplitterAdjustmentAuthority::LocalReady,
            policy,
            events,
            interaction_events,
        )?;
        Ok(InputOutcome::InteractionProcessed {
            outcome,
            version: self.version,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn reduce_local_splitter_junction_adjustment_input(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        application_base: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        junction: crate::scene::SplitterJunctionId,
        axis: crate::model::DockspaceAxis,
        delta: f64,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != application_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: application_base,
            });
        }
        let outcome = self.adjust_splitter_junction_resize(
            input,
            scene,
            junction,
            axis,
            delta,
            policy,
            events,
            interaction_events,
        )?;
        Ok(InputOutcome::InteractionProcessed {
            outcome,
            version: self.version,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn reduce_local_tab_gesture(
        &mut self,
        input: InputSequence,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        expected: WorkspaceVersion,
        application_base: WorkspaceVersion,
        surface: SurfaceId,
        source: TabGestureSource,
        phase: LocalTabGesturePhase,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != application_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: application_base,
            });
        }
        let owner = GestureOwner::LocalResponse { surface };
        let outcome = match phase {
            LocalTabGesturePhase::Begin {
                scene,
                initial,
                current,
            } => self.begin_local_tab_gesture(
                cause,
                focus_causal,
                owner,
                source,
                scene,
                initial,
                current,
                policy,
                events,
                interaction_events,
            )?,
            LocalTabGesturePhase::Move { scene, current } => self.update_local_tab_gesture(
                cause,
                owner,
                source,
                scene,
                current,
                policy,
                interaction_events,
            )?,
            LocalTabGesturePhase::Release { scene, current } => self.release_local_tab_gesture(
                cause,
                focus_causal,
                owner,
                source,
                scene,
                current,
                policy,
                events,
                interaction_events,
            )?,
            LocalTabGesturePhase::Cancel => {
                self.cancel_local_tab_gesture(input, owner, source, interaction_events)
            }
        };
        Ok(InputOutcome::InteractionProcessed {
            outcome,
            version: self.version,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn begin_local_tab_gesture(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        owner: GestureOwner,
        source: TabGestureSource,
        scene: SurfaceSceneStamp,
        initial: LogicalPoint,
        current: LogicalPoint,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        if let Some(rejection) = self.pending_presentation_transition_gesture_rejection() {
            return Ok(InteractionOutcome::Rejected(rejection));
        }
        if self.interaction.status() != InteractionStatus::Idle {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::SessionMismatch,
            ));
        }
        let prepared = match self
            .local_response_candidate(scene)
            .and_then(|candidate| self.prepare_local_tab_gesture(candidate, source, initial))
        {
            Ok(prepared) => prepared,
            Err(rejection) => return Ok(InteractionOutcome::Rejected(rejection)),
        };
        let armed = self.activate_prepared_tab_gesture(
            cause,
            focus_causal,
            owner,
            None,
            None,
            prepared,
            policy,
            events,
        )?;
        let InteractionOutcome::DragArmed { session, .. } = armed else {
            return Ok(armed);
        };
        if let Err(rejection) = self
            .interaction
            .begin_drag(session, owner, PointerButton::Primary)
        {
            return Ok(InteractionOutcome::Rejected(rejection));
        }
        self.interaction
            .set_drag_current_pointer(session, current)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("local tab drag lost its current pointer after begin: {source:?}"),
            })?;
        if current != initial {
            let drag = self
                .interaction
                .active_drag(session)
                .map_err(|source| EngineError::PointerInteractionInvariant {
                    cause,
                    detail: format!("local tab drag session disappeared after begin: {source:?}"),
                })?
                .clone();
            let evaluation =
                self.resolve_local_tab_preview(cause, &drag, scene, current, policy)?;
            let _ = self.apply_preview_evaluation(
                cause,
                owner,
                session,
                evaluation,
                interaction_events,
            )?;
        }
        Ok(InteractionOutcome::DragBegan { session })
    }

    fn update_local_tab_gesture(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        source: TabGestureSource,
        scene: SurfaceSceneStamp,
        current: LogicalPoint,
        policy: &DockPolicySnapshot,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let session = match self.local_tab_session(owner, source) {
            Ok(session) => session,
            Err(rejection) => return Ok(InteractionOutcome::Rejected(rejection)),
        };
        let drag = self
            .interaction
            .active_drag(session)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("local tab drag session disappeared: {source:?}"),
            })?
            .clone();
        let evaluation = self.resolve_local_tab_preview(cause, &drag, scene, current, policy)?;
        self.interaction
            .set_drag_current_pointer(session, current)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("local tab drag lost its current pointer: {source:?}"),
            })?;
        self.apply_preview_evaluation(cause, owner, session, evaluation, interaction_events)?
            .ok_or(EngineError::ReductionCauseInvariant {
                detail: "local tab preview evaluation produced no outcome",
            })
    }

    #[allow(clippy::too_many_arguments)]
    fn release_local_tab_gesture(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        owner: GestureOwner,
        source: TabGestureSource,
        scene: SurfaceSceneStamp,
        current: LogicalPoint,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let session = match self.local_tab_session(owner, source) {
            Ok(session) => session,
            Err(rejection) => return Ok(InteractionOutcome::Rejected(rejection)),
        };
        let drag = self
            .interaction
            .active_drag(session)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("local tab release session disappeared: {source:?}"),
            })?
            .clone();
        let evaluation = self.resolve_local_tab_preview(cause, &drag, scene, current, policy)?;
        self.interaction
            .set_drag_current_pointer(session, current)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("local tab release lost its current pointer: {source:?}"),
            })?;
        if !matches!(evaluation.decision, PreviewDecision::Publish { .. }) {
            return self.cancel_local_tab_release(cause, session, interaction_events);
        }
        let release_decision = evaluation.decision.clone();
        let _ =
            self.apply_preview_evaluation(cause, owner, session, evaluation, interaction_events)?;
        let drag = self
            .interaction
            .take_drag_for_release(session, owner, PointerButton::Primary)
            .map_err(|source| EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("local tab release could not consume drag: {source:?}"),
            })?;
        if drag.preview.as_ref().is_some_and(|published| {
            !published.painted()
                && Self::release_decision_matches_preview(published, &release_decision)
        }) {
            return self.defer_drag_release(
                cause,
                focus_causal,
                session,
                drag,
                None,
                release_decision,
                policy,
            );
        }
        self.finish_drag_release(
            cause,
            focus_causal,
            session,
            drag,
            release_decision,
            policy,
            events,
            interaction_events,
        )
    }

    fn cancel_local_tab_release(
        &mut self,
        cause: ReductionCause,
        session: crate::interaction::DragSessionId,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let status = self.interaction.cancel_drag(session).map_err(|source| {
            EngineError::PointerInteractionInvariant {
                cause,
                detail: format!("local tab release cancellation failed: {source:?}"),
            }
        })?;
        let reason = InteractionCancelReason::LocalResponseCancelled;
        interaction_events.push(InteractionEvent::new_caused(
            cause,
            self.version,
            InteractionEventKind::Cancelled { status, reason },
        ));
        Ok(InteractionOutcome::Cancelled { status, reason })
    }

    fn cancel_local_tab_gesture(
        &mut self,
        input: InputSequence,
        owner: GestureOwner,
        source: TabGestureSource,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> InteractionOutcome {
        let Ok(session) = self.local_tab_session(owner, source) else {
            return InteractionOutcome::Rejected(InteractionRejection::NoActiveGesture);
        };
        match self.interaction.cancel_drag(session) {
            Ok(status) => {
                let reason = InteractionCancelReason::LocalResponseCancelled;
                interaction_events.push(InteractionEvent::new(
                    input,
                    self.version,
                    InteractionEventKind::Cancelled { status, reason },
                ));
                InteractionOutcome::Cancelled { status, reason }
            }
            Err(error) => InteractionOutcome::Rejected(error),
        }
    }

    fn local_tab_session(
        &self,
        owner: GestureOwner,
        source: TabGestureSource,
    ) -> Result<crate::interaction::DragSessionId, InteractionRejection> {
        let InteractionStatus::Dragging { session } = self.interaction.status() else {
            return Err(InteractionRejection::NoActiveGesture);
        };
        let drag = self.interaction.active_drag(session)?;
        if drag.owner != owner {
            return Err(InteractionRejection::SessionMismatch);
        }
        let GestureOwner::LocalResponse { surface } = owner else {
            return Err(InteractionRejection::SessionMismatch);
        };
        match (source, &drag.payload) {
            (TabGestureSource::Item(source), MovePayload::Item(item))
                if item.root() == source.root
                    && item.tabs() == source.tabs
                    && item.item() == source.item => {}
            (TabGestureSource::Group(source), MovePayload::Tabs(tabs))
                if tabs.root() == source.root && tabs.node() == source.tabs => {}
            (
                TabGestureSource::ContainedTitle { root, floating },
                MovePayload::Subtree(subtree),
            ) if subtree.root() == root
                && self.workspace.presentation_for_root(root)
                    == Some(crate::RootPresentationOwner::Contained { surface, floating }) => {}
            _ => return Err(InteractionRejection::SessionMismatch),
        }
        Ok(session)
    }

    fn resolve_local_tab_preview(
        &self,
        cause: ReductionCause,
        drag: &crate::interaction::ActiveDrag,
        scene: SurfaceSceneStamp,
        point: LogicalPoint,
        policy: &DockPolicySnapshot,
    ) -> Result<PreviewEvaluation, EngineError> {
        if !self.drag_source_presentation_allows_targeting(drag) {
            return Ok(PreviewEvaluation::without_affordance(
                PreviewDecision::Clear(PreviewResolutionStatus::Unavailable),
            ));
        }
        let candidate = match self.local_response_candidate(scene) {
            Ok(candidate) if candidate.stamp().surface() == drag.source_surface => candidate,
            Ok(_) | Err(_) => {
                return Ok(PreviewEvaluation::without_affordance(
                    PreviewDecision::Clear(PreviewResolutionStatus::Unavailable),
                ));
            }
        };
        self.resolve_local_tab_preview_in_candidate(cause, drag, candidate, point, policy)
    }

    pub(super) fn resolve_local_tab_preview_against_ready_candidate(
        &self,
        cause: ReductionCause,
        drag: &crate::interaction::ActiveDrag,
        surface: SurfaceId,
        point: LogicalPoint,
        policy: &DockPolicySnapshot,
    ) -> Result<PreviewEvaluation, EngineError> {
        let scene = self
            .presentation_authority
            .scene
            .ready_candidate(surface)
            .map(crate::scene::SurfacePlanScene::stamp);
        let Some(scene) = scene else {
            return Ok(PreviewEvaluation::without_affordance(
                PreviewDecision::Clear(PreviewResolutionStatus::Unavailable),
            ));
        };
        self.resolve_local_tab_preview(cause, drag, scene, point, policy)
    }

    fn resolve_local_tab_preview_in_candidate(
        &self,
        cause: ReductionCause,
        drag: &crate::interaction::ActiveDrag,
        candidate: &crate::scene::SurfacePlanScene,
        point: LogicalPoint,
        policy: &DockPolicySnapshot,
    ) -> Result<PreviewEvaluation, EngineError> {
        let query = resolve_presented_drop(
            candidate.stamp(),
            candidate.plan(),
            drag.source_layout_facts.as_deref(),
            &self.workspace,
            self.version,
            self.presentation_authority
                .presentation_requirements
                .workspace_index(),
            policy,
            drag.session,
            drag.payload.clone(),
            drag.surface_background_offer,
            point,
        )
        .map_err(|source| EngineError::PointerInteractionInvariant {
            cause,
            detail: source.to_string(),
        })?;
        let (resolution, affordance) = query.into_parts();
        let decision = match resolution {
            DropResolution::Resolved(resolved) => {
                if resolved.session() != drag.session || resolved.source() != &drag.payload {
                    return Err(EngineError::PointerInteractionInvariant {
                        cause,
                        detail: "local tab resolver changed the frozen drag identity".to_owned(),
                    });
                }
                let target = resolved.target_id();
                PreviewDecision::Publish {
                    scene: resolved.scene_stamp(),
                    visual: PreviewVisual::Dock {
                        surface: candidate.stamp().surface(),
                        target,
                        rect: resolved.visual().rect(),
                    },
                    proof: Box::new(PreviewProof::Dock {
                        target,
                        command: resolved.into_command(),
                    }),
                }
            }
            DropResolution::KnownNone(_) => self
                .resolve_local_contained_move_preview(cause, drag, candidate, point, policy)?
                .unwrap_or(PreviewDecision::Clear(PreviewResolutionStatus::KnownNone)),
            DropResolution::Rejected(_) => {
                PreviewDecision::Clear(PreviewResolutionStatus::Rejected)
            }
            DropResolution::Unavailable(_) => {
                PreviewDecision::Clear(PreviewResolutionStatus::Unavailable)
            }
        };
        Ok(PreviewEvaluation::new(decision, affordance))
    }

    fn resolve_local_contained_move_preview(
        &self,
        cause: ReductionCause,
        drag: &crate::interaction::ActiveDrag,
        candidate: &crate::scene::SurfacePlanScene,
        point: LogicalPoint,
        policy: &DockPolicySnapshot,
    ) -> Result<Option<PreviewDecision>, EngineError> {
        let FrozenDragOrigin::Contained(origin) = &drag.origin else {
            return Ok(None);
        };
        let DragGestureAuthority::LocalReady {
            surface,
            coordinates,
        } = drag.presentation
        else {
            return Ok(Some(PreviewDecision::Clear(
                PreviewResolutionStatus::Unavailable,
            )));
        };
        if surface != drag.source_surface
            || surface != origin.surface
            || surface != candidate.stamp().surface()
            || coordinates != candidate.coordinate_capture()
        {
            return Ok(Some(PreviewDecision::Clear(
                PreviewResolutionStatus::Unavailable,
            )));
        }

        let requested =
            match translated_contained_rect(origin.source_rect, origin.initial_pointer, point) {
                Ok(requested) => requested,
                Err(()) => {
                    return Ok(Some(PreviewDecision::Clear(
                        PreviewResolutionStatus::Rejected,
                    )));
                }
            };
        let bounds = candidate.plan().bounds();
        let clamped = match clamp_contained_rect(surface, bounds, requested, origin.minimum_size) {
            Ok(clamped) => clamped,
            Err(_) => {
                return Ok(Some(PreviewDecision::Clear(
                    PreviewResolutionStatus::Rejected,
                )));
            }
        };
        let placement = ContainedPlacementProof::new(
            candidate.stamp(),
            surface,
            requested,
            origin.minimum_size,
            bounds,
            clamped,
        );
        let proposal = crate::intent::ContainedTearOffProposal::new(
            origin.root,
            origin.floating,
            placement,
            ContainedPosition::Front,
        );
        let Some(command) = self.contained_presentation_command(
            &drag.payload,
            proposal,
            drag.complete_root.clone(),
            Some(origin),
        ) else {
            return Ok(Some(PreviewDecision::Clear(
                PreviewResolutionStatus::Rejected,
            )));
        };
        if !self.valid_contained_command_structure(drag, proposal, &command)
            || self
                .stage_journal_workspace_command(cause, &command, policy)?
                .is_err()
        {
            return Ok(Some(PreviewDecision::Clear(
                PreviewResolutionStatus::Rejected,
            )));
        }

        Ok(Some(PreviewDecision::Publish {
            scene: placement.scene(),
            visual: PreviewVisual::Contained {
                surface,
                rect: proposal.rect(),
                fallback: false,
            },
            proof: Box::new(PreviewProof::Contained {
                command,
                proposal,
                fallback: false,
            }),
        }))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn reduce_local_splitter_gesture(
        &mut self,
        input: InputSequence,
        cause: ReductionCause,
        expected: WorkspaceVersion,
        application_base: WorkspaceVersion,
        surface: SurfaceId,
        target: SplitterResizeTarget,
        phase: LocalSplitterGesturePhase,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != application_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: application_base,
            });
        }

        let owner = GestureOwner::LocalResponse { surface };
        let outcome = match phase {
            LocalSplitterGesturePhase::Press {
                scene,
                initial,
                current,
            } => {
                if let Some(rejection) = self.pending_presentation_transition_gesture_rejection() {
                    InteractionOutcome::Rejected(rejection)
                } else if self.interaction.status() != InteractionStatus::Idle {
                    InteractionOutcome::Rejected(InteractionRejection::SessionMismatch)
                } else {
                    let candidate = match self.local_response_candidate(scene) {
                        Ok(candidate) if candidate.plan().surface() == surface => candidate,
                        Ok(_) => {
                            return Ok(self.local_response_rejection(
                                InteractionRejection::SplitterGestureSurfaceMismatch {
                                    expected: scene.surface(),
                                    actual: surface,
                                },
                            ));
                        }
                        Err(error) => return Ok(self.local_response_rejection(error)),
                    };
                    if !Self::coordinate_capture_matches_current(
                        candidate.coordinate_capture(),
                        self.viewport.viewport(surface),
                        self.viewport.surface_coordinate_authority(surface),
                    ) {
                        return Ok(self.local_response_rejection(
                            InteractionRejection::SplitterGestureCoordinateAuthorityUnavailable {
                                surface,
                            },
                        ));
                    }
                    let start = match self.prepare_splitter_resize(
                        candidate.plan(),
                        scene,
                        candidate.coordinate_capture(),
                        target,
                        owner,
                        None,
                        ResizeGestureAuthority::LocalReady,
                        initial,
                        policy,
                    ) {
                        Ok(start) => start,
                        Err(error) => return Ok(self.local_response_rejection(error)),
                    };
                    let (session, replaced) = self
                        .interaction
                        .begin_resize(self.version.epoch(), start)
                        .map_err(|source| EngineError::PointerInteractionInvariant {
                            cause,
                            detail: source.to_string(),
                        })?;
                    if replaced.is_some() {
                        return Err(EngineError::ReductionCauseInvariant {
                            detail: "local splitter press replaced an active gesture after idle validation",
                        });
                    }
                    if current != initial {
                        let update = self.update_resize_at_point(
                            cause, owner, session, target, current, policy,
                        )?;
                        if let InteractionOutcome::Rejected(rejection) = update {
                            let _ = self.interaction.cancel_resize(session);
                            return Ok(self.local_response_rejection(rejection));
                        }
                    }
                    InteractionOutcome::ResizeBegan { session, replaced }
                }
            }
            LocalSplitterGesturePhase::Move { current } => {
                let session = match self.local_resize_session(surface, target) {
                    Ok(session) => session,
                    Err(error) => return Ok(self.local_response_rejection(error)),
                };
                self.update_resize_at_point(cause, owner, session, target, current, policy)?
            }
            LocalSplitterGesturePhase::Release { current } => {
                let session = match self.local_resize_session(surface, target) {
                    Ok(session) => session,
                    Err(error) => return Ok(self.local_response_rejection(error)),
                };
                self.finish_resize_at_point(
                    cause,
                    owner,
                    session,
                    target,
                    current,
                    policy,
                    events,
                    interaction_events,
                )?
            }
            LocalSplitterGesturePhase::Cancel => {
                let session = match self.local_resize_session(surface, target) {
                    Ok(session) => session,
                    Err(error) => return Ok(self.local_response_rejection(error)),
                };
                self.cancel_resize(
                    input,
                    session,
                    InteractionCancelReason::LocalResponseCancelled,
                    interaction_events,
                )
            }
        };
        Ok(InputOutcome::InteractionProcessed {
            outcome,
            version: self.version,
        })
    }

    fn local_resize_session(
        &self,
        surface: SurfaceId,
        target: SplitterResizeTarget,
    ) -> Result<crate::interaction::ResizeSessionId, InteractionRejection> {
        let resize = self
            .interaction
            .active_resize_view()
            .ok_or(InteractionRejection::NoActiveGesture)?;
        if resize.local_response_surface() != Some(surface) || resize.target() != target {
            return Err(InteractionRejection::SessionMismatch);
        }
        Ok(resize.session())
    }

    pub(super) fn local_response_rejection(&self, outcome: InteractionRejection) -> InputOutcome {
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(outcome),
            version: self.version,
        }
    }
}

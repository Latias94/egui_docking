//! Non-pointer semantic commands prepared from an exact current candidate.

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

    pub(super) fn semantic_candidate(
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
            let candidate = match self.semantic_candidate(scene) {
                Ok(candidate) => candidate,
                Err(error) => return Ok(self.candidate_action_rejection(error)),
            };
            let Some(record) = candidate
                .plan()
                .tab_records()
                .iter()
                .find(|record| *record.id() == tab)
            else {
                return Ok(self.candidate_action_rejection(
                    InteractionRejection::TabGestureSourceUnavailable {
                        source: TabGestureSource::Item(tab),
                    },
                ));
            };
            if !candidate
                .plan()
                .region_is_operable(record.drag_hit().rect(), record.layer())
            {
                return Ok(self.candidate_action_rejection(
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
                    return Ok(self.candidate_action_rejection(
                        InteractionRejection::TabGestureSourceUnavailable {
                            source: TabGestureSource::Item(tab),
                        },
                    ));
                }
            };
            if self.root_has_pending_presentation_transition(tab.root) {
                return Ok(self.candidate_action_rejection(
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
                        return Ok(self.candidate_action_rejection(
                            InteractionRejection::TabGestureSourceUnavailable {
                                source: TabGestureSource::Item(tab),
                            },
                        ));
                    };
                    if contained.root() != tab.root || contained.layer() != record.layer() {
                        return Ok(self.candidate_action_rejection(
                            InteractionRejection::TabGestureSourceUnavailable {
                                source: TabGestureSource::Item(tab),
                            },
                        ));
                    }
                    if contained.is_frontmost() {
                        None
                    } else {
                        let Some(root) = self.workspace.root(tab.root) else {
                            return Ok(self.candidate_action_rejection(
                                InteractionRejection::TabGestureSourceUnavailable {
                                    source: TabGestureSource::Item(tab),
                                },
                            ));
                        };
                        let source = match self.workspace.capture_node_source(tab.root, root.node) {
                            Ok(source) => source,
                            Err(error) => {
                                return Ok(self.candidate_action_rejection(
                                    InteractionRejection::CommandRejected(error),
                                ));
                            }
                        };
                        let expected_roster = match self.workspace.capture_contained_roster(surface)
                        {
                            Ok(roster) => roster,
                            Err(error) => {
                                return Ok(self.candidate_action_rejection(
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
                    return Ok(self.candidate_action_rejection(
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
        let plan = match self.semantic_candidate(scene) {
            Ok(candidate) => candidate.plan().clone(),
            Err(error) => return Ok(self.candidate_action_rejection(error)),
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
            LocalTabChromeAction::DismissMenuFromBackdrop(backdrop) => {
                self.finish_journal_tab_list_menu_backdrop(cause, backdrop, &plan)?
            }
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
            engine.request_close_plan(
                input,
                scene,
                target,
                CloseActivation::CurrentCandidate,
                policy,
            )
        })
    }

    pub(super) fn reduce_local_root_close_input(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        root: RootId,
        policy: &DockPolicySnapshot,
    ) -> Result<InputOutcome, EngineError> {
        let candidate = match self.semantic_candidate(scene) {
            Ok(candidate) => candidate,
            Err(reason) => return Ok(self.candidate_action_rejection(reason)),
        };
        let operable = candidate
            .plan()
            .presentation_menu_anchor_records()
            .iter()
            .any(|anchor| {
                anchor.root() == root
                    && anchor.ready_bounds().is_some_and(|bounds| {
                        candidate.plan().region_is_operable(bounds, anchor.layer())
                    })
            });
        if !operable {
            return Ok(InputOutcome::ContentCloseRejected {
                target: ContentCloseTarget::Root(root),
                reason: ContentCloseRequestRejection::RootUnavailable { root },
                version: self.version,
            });
        }
        let outcome = self.reduce_content_close_request(
            input,
            expected,
            ContentCloseTarget::Root(root),
            policy,
        )?;
        Ok(match outcome {
            InputOutcome::ContentCloseRequested {
                plan,
                reused,
                version,
                ..
            } => InputOutcome::InteractionProcessed {
                outcome: InteractionOutcome::CloseRequested { plan, reused },
                version,
            },
            other => other,
        })
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
            SplitterAdjustmentAuthority::CurrentCandidate,
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

    pub(super) fn candidate_action_rejection(&self, outcome: InteractionRejection) -> InputOutcome {
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(outcome),
            version: self.version,
        }
    }
}

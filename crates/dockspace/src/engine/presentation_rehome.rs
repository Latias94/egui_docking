//! Presentation-gated root rehome transitions.

use super::*;

use super::product_action::{ProductActionPlan, ProductCommandContext};
use crate::interaction::{PreviewProof, PreviewVisual};
use crate::model::{DockspaceActionOutcome, DockspaceActionRejection, ProductAction};

#[derive(Debug, Clone, Copy)]
struct NativePresentationRehomeSource {
    owner: crate::RootPresentationOwner,
    surface: SurfaceId,
    binding: crate::viewport::ViewportBinding,
    recovery: SurfaceRecoveryObligationId,
    presentation: PresentedSurfaceAuthority,
}

struct PreparedPresentationRehome {
    command: WorkspaceCommand,
    root: RootId,
    source: crate::command::NodeSource,
    target_surface: SurfaceId,
    floating: Option<FloatingPresentationId>,
    visual: PreviewVisual,
    items: Vec<ItemId>,
    context: ProductCommandContext,
    commit_authority: PresentationRehomeCommitAuthority,
}

impl DockEngine {
    pub(super) fn validate_product_root_float_availability(
        &self,
        root: RootId,
        target_surface: SurfaceId,
    ) -> Result<(), DockspaceActionRejection> {
        if self.root_has_pending_presentation_transition(root) {
            return Err(DockspaceActionRejection::Conflict);
        }
        let Some(_) = self.native_presentation_rehome_source(root, target_surface)? else {
            return Ok(());
        };
        if self.pending_drag_release.is_some()
            || self.pending_contained_transform_release.is_some()
            || self.interaction.status() != InteractionStatus::Idle
        {
            return Err(DockspaceActionRejection::Conflict);
        }
        if self.interaction_projection(target_surface).is_none() {
            return Err(DockspaceActionRejection::PresentationUnavailable {
                surface: target_surface,
            });
        }
        Ok(())
    }

    pub(super) fn native_main_dock_back_target(
        &self,
        root: RootId,
    ) -> Result<Option<SurfaceId>, DockspaceActionRejection> {
        let owner = self
            .workspace
            .presentation_for_root(root)
            .ok_or(DockspaceActionRejection::RootUnavailable { root })?;
        let crate::RootPresentationOwner::Main { surface } = owner else {
            return Ok(None);
        };
        let Some(bound) = self.bound_surface_recoveries.get(&surface) else {
            return Ok(None);
        };
        let target = bound.obligation.target();
        if !target
            .converted_main()
            .is_some_and(|converted| converted.source_root() == root)
            || target.host_surface() == surface
            || self.workspace.surface(target.host_surface()).is_none()
        {
            return Err(DockspaceActionRejection::DockBackUnavailable { root });
        }
        Ok(Some(target.host_surface()))
    }

    fn native_presentation_rehome_source(
        &self,
        root: RootId,
        target_surface: SurfaceId,
    ) -> Result<Option<NativePresentationRehomeSource>, DockspaceActionRejection> {
        let owner = self
            .workspace
            .presentation_for_root(root)
            .ok_or(DockspaceActionRejection::RootUnavailable { root })?;
        let source_surface = match owner {
            crate::RootPresentationOwner::Main { surface }
            | crate::RootPresentationOwner::Contained { surface, .. } => surface,
        };
        let Some(source_recovery) = self.bound_surface_recoveries.get(&source_surface) else {
            return Ok(None);
        };
        match owner {
            crate::RootPresentationOwner::Contained { .. } if source_surface == target_surface => {
                return Ok(None);
            }
            crate::RootPresentationOwner::Main { .. } if source_surface == target_surface => {
                return Err(DockspaceActionRejection::Conflict);
            }
            crate::RootPresentationOwner::Main { .. }
            | crate::RootPresentationOwner::Contained { .. } => {}
        }
        let source_binding = source_recovery.binding;
        let source_is_live = self
            .viewport
            .viewport(source_surface)
            .is_some_and(|record| {
                record.binding() == source_binding
                    && record.admission() == ViewportAdmission::Admitted
            });
        let source_presentation = self
            .interaction_authority(source_surface)
            .filter(|authority| authority.binding() == Some(source_binding));
        let Some(presentation) = source_presentation.filter(|_| source_is_live) else {
            return Err(DockspaceActionRejection::PresentationUnavailable {
                surface: source_surface,
            });
        };
        Ok(Some(NativePresentationRehomeSource {
            owner,
            surface: source_surface,
            binding: source_binding,
            recovery: source_recovery.obligation.id(),
            presentation,
        }))
    }

    fn prepare_bound_native_main_dock_back(
        &self,
        root: RootId,
        source: NativePresentationRehomeSource,
    ) -> Result<PreparedPresentationRehome, DockspaceActionRejection> {
        if !matches!(source.owner, crate::RootPresentationOwner::Main { .. }) {
            return Err(DockspaceActionRejection::DockBackUnavailable { root });
        }
        let bound = self
            .bound_surface_recoveries
            .get(&source.surface)
            .filter(|bound| {
                bound.binding == source.binding && bound.obligation.id() == source.recovery
            })
            .ok_or(DockspaceActionRejection::DockBackUnavailable { root })?;
        let target = bound.obligation.target();
        let host = self
            .freeze_surface_recovery_target(target.host_surface())
            .ok_or(DockspaceActionRejection::PresentationUnavailable {
                surface: target.host_surface(),
            })?;
        let (root_source, _, items) = self.capture_product_root_payload(root)?;
        let roster = self.capture_surface_roster(source.surface).map_err(|_| {
            DockspaceActionRejection::PresentationUnavailable {
                surface: source.surface,
            }
        })?;
        let program = roster
            .compile_converted_main_recovery(&self.workspace, target, host)
            .map_err(|_| DockspaceActionRejection::DockBackUnavailable { root })?;
        let (command, target_surface, floating, rect) = program.into_parts();
        Ok(PreparedPresentationRehome {
            command,
            root,
            source: root_source,
            target_surface,
            floating: Some(floating),
            visual: PreviewVisual::Contained {
                surface: target_surface,
                rect,
                fallback: false,
            },
            context: ProductCommandContext::FloatRootRehome {
                root,
                surface: target_surface,
                floating,
                items: items.clone(),
            },
            items,
            commit_authority: PresentationRehomeCommitAuthority::BoundConvertedMain,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn reduce_native_product_rehome(
        &mut self,
        input: InputSequence,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        expected: WorkspaceVersion,
        action: ProductAction,
        command: WorkspaceCommand,
        context: ProductCommandContext,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<Option<InputOutcome>, EngineError> {
        let Some((candidate_root, candidate_target_surface)) =
            self.native_product_rehome_route(&command)?
        else {
            return Ok(None);
        };
        let source_native = match self
            .native_presentation_rehome_source(candidate_root, candidate_target_surface)
        {
            Ok(Some(source_native)) => source_native,
            Ok(None) => return Ok(None),
            Err(reason) => {
                return Ok(Some(InputOutcome::ProductActionRejected {
                    reason,
                    version: self.version,
                }));
            }
        };
        let (source, target_surface, floating, visual, target_root) =
            match self.prepare_native_product_rehome_target(&command) {
                Ok(Some(prepared)) => prepared,
                Ok(None) => return Ok(None),
                Err(reason) => {
                    return Ok(Some(InputOutcome::ProductActionRejected {
                        reason,
                        version: self.version,
                    }));
                }
            };
        let root = source.root();
        debug_assert_eq!(root, candidate_root);
        debug_assert_eq!(target_surface, candidate_target_surface);
        if self.pending_presentation_rehome.is_some()
            || self.pending_drag_release.is_some()
            || self.pending_contained_transform_release.is_some()
            || self.interaction.status() != InteractionStatus::Idle
        {
            return Ok(Some(InputOutcome::ProductActionRejected {
                reason: DockspaceActionRejection::Conflict,
                version: self.version,
            }));
        }
        let projection = match self.interaction_projection(target_surface) {
            Some(projection) => projection,
            None => {
                return Ok(Some(InputOutcome::ProductActionRejected {
                    reason: DockspaceActionRejection::PresentationUnavailable {
                        surface: target_surface,
                    },
                    version: self.version,
                }));
            }
        };
        let payload = match &command {
            WorkspaceCommand::Move { payload, .. } => payload.clone(),
            WorkspaceCommand::RehomeRoot { .. } => match self.capture_product_root_payload(root) {
                Ok((_, payload, _)) => payload,
                Err(reason) => {
                    return Ok(Some(InputOutcome::ProductActionRejected {
                        reason,
                        version: self.version,
                    }));
                }
            },
            _ => return Ok(None),
        };
        let pane_focus = match self.freeze_payload_focus(&payload) {
            Ok(focus) => focus,
            Err(error) => {
                return Ok(Some(InputOutcome::ProductActionRejected {
                    reason: super::product_action::product_command_rejection(action, &error)?,
                    version: self.version,
                }));
            }
        };
        match self.stage_journal_workspace_command_with_authority(
            cause,
            &command,
            policy,
            WorkspacePublicationAuthority::Ordinary,
        )? {
            Ok(_) => {}
            Err(error) => {
                return Ok(Some(InputOutcome::ProductActionRejected {
                    reason: super::product_action::product_command_rejection(action, &error)?,
                    version: self.version,
                }));
            }
        }
        let preview = self
            .interaction
            .prepare_presentation_rehome_preview(
                self.version.epoch(),
                projection.plan_stamp(),
                visual,
                PreviewProof::PresentationRehome {
                    command: command.clone(),
                },
            )
            .map_err(|source| EngineError::Interaction { input, source })?;
        let items = self.workspace.collect_items_in_subtree(source.node());
        let outcome = match action {
            ProductAction::FloatItem { .. } | ProductAction::FloatRoot { .. } => {
                DockspaceActionOutcome::RootFloatRequested {
                    root,
                    source_surface: source_native.surface,
                    target_surface,
                    items: items.clone(),
                }
            }
            ProductAction::DockItem { .. } | ProductAction::DockRoot { .. } => {
                DockspaceActionOutcome::RootDockRequested {
                    root,
                    source_surface: source_native.surface,
                    target_root,
                    items: items.clone(),
                }
            }
            _ => return Ok(None),
        };
        self.pending_presentation_rehome = Some(PendingPresentationRehome {
            source_version: expected,
            policy_revision: policy.revision(),
            cause,
            focus_causal,
            root,
            source,
            source_owner: source_native.owner,
            source_surface: source_native.surface,
            source_binding: source_native.binding,
            source_recovery: source_native.recovery,
            source_presentation: source_native.presentation,
            commit_authority: PresentationRehomeCommitAuthority::Ordinary,
            target_surface,
            floating,
            context,
            pane_focus,
            preview,
            presentation_outputs: BTreeSet::new(),
            presented_output: None,
            presentation_failed: false,
        });
        let _ = events;
        Ok(Some(InputOutcome::ProductActionProcessed {
            outcome,
            version: self.version,
        }))
    }

    fn native_product_rehome_route(
        &self,
        command: &WorkspaceCommand,
    ) -> Result<Option<(RootId, SurfaceId)>, EngineError> {
        match command {
            WorkspaceCommand::Move { payload, target } => self
                .capture_complete_root_payload(payload)
                .map(|source| source.map(|source| (source.root(), target.surface())))
                .map_err(|_| EngineError::ReductionCauseInvariant {
                    detail: "compiled product move lost its complete-root source",
                }),
            WorkspaceCommand::RehomeRoot { source, target } => match target {
                RootPresentationTarget::Main { surface }
                | RootPresentationTarget::Contained { surface, .. } => {
                    Ok(Some((source.root(), *surface)))
                }
                RootPresentationTarget::NewSurface { .. } => Ok(None),
            },
            _ => Ok(None),
        }
    }

    fn prepare_native_product_rehome_target(
        &self,
        command: &WorkspaceCommand,
    ) -> Result<
        Option<(
            crate::command::NodeSource,
            SurfaceId,
            Option<FloatingPresentationId>,
            PreviewVisual,
            RootId,
        )>,
        DockspaceActionRejection,
    > {
        match command {
            WorkspaceCommand::Move { payload, target } => {
                let Some(source) = self.capture_complete_root_payload(payload)? else {
                    return Ok(None);
                };
                let surface = target.surface();
                let projection = self
                    .interaction_projection(surface)
                    .ok_or(DockspaceActionRejection::PresentationUnavailable { surface })?;
                let record = projection
                    .plan()
                    .drop_target(product_drop_target_id(target))
                    .ok_or(DockspaceActionRejection::PresentationUnavailable { surface })?;
                Ok(Some((
                    source,
                    surface,
                    None,
                    PreviewVisual::Dock {
                        surface,
                        target: record.id(),
                        rect: record.visual().rect(),
                    },
                    product_dock_target_root(target),
                )))
            }
            WorkspaceCommand::RehomeRoot { source, target } => match *target {
                RootPresentationTarget::Main { surface } => {
                    let projection = self
                        .interaction_projection(surface)
                        .ok_or(DockspaceActionRejection::PresentationUnavailable { surface })?;
                    let record = projection
                        .plan()
                        .drop_targets()
                        .iter()
                        .find(|record| {
                            record.id()
                                == crate::drop_target::DropTargetId::SurfaceBackground { surface }
                        })
                        .ok_or(DockspaceActionRejection::PresentationUnavailable { surface })?;
                    Ok(Some((
                        source.clone(),
                        surface,
                        None,
                        PreviewVisual::Dock {
                            surface,
                            target: record.id(),
                            rect: record.visual().rect(),
                        },
                        source.root(),
                    )))
                }
                RootPresentationTarget::Contained {
                    surface,
                    floating,
                    rect,
                    ..
                } => Ok(Some((
                    source.clone(),
                    surface,
                    Some(floating),
                    PreviewVisual::Contained {
                        surface,
                        rect,
                        fallback: false,
                    },
                    source.root(),
                ))),
                RootPresentationTarget::NewSurface { .. } => Ok(None),
            },
            _ => Ok(None),
        }
    }

    pub(super) fn pending_presentation_transition_gesture_rejection(
        &self,
    ) -> Option<InteractionRejection> {
        (self.pending_presentation_rehome.is_some()
            || self.viewport.native_create_sagas().next().is_some())
        .then_some(InteractionRejection::PresentationTransitionPending)
    }

    pub(super) fn invalidate_pending_presentation_rehome(&mut self) {
        if let Some(pending) = self.pending_presentation_rehome.as_mut()
            && (pending.source_version != self.version
                || pending.policy_revision != self.policy.revision())
        {
            pending.presentation_failed = true;
        }
    }

    pub(super) fn fail_pending_presentation_rehome(&mut self) {
        if let Some(pending) = self.pending_presentation_rehome.as_mut() {
            pending.presentation_failed = true;
        }
    }

    pub(super) fn observe_pending_presentation_rehome_dispositions(
        &mut self,
        dispositions: &[HostPresentationDispositionOutcome],
    ) -> bool {
        let Some(pending) = self.pending_presentation_rehome.as_mut() else {
            return false;
        };
        let failed = dispositions.iter().any(|outcome| {
            outcome.slot().surface() == pending.target_surface
                && matches!(
                    outcome.disposition(),
                    HostPresentationDisposition::Unavailable(reason)
                        if presentation_unavailability_is_terminal(reason)
                )
        });
        if failed {
            pending.presentation_failed = true;
        }
        failed
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn reduce_product_root_float(
        &mut self,
        input: InputSequence,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        expected: WorkspaceVersion,
        root: RootId,
        surface: SurfaceId,
        rect: Option<LogicalRect>,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        self.reduce_product_root_float_action(
            input,
            cause,
            focus_causal,
            expected,
            ProductAction::FloatRoot {
                root,
                surface,
                rect,
            },
            root,
            surface,
            rect,
            policy,
            events,
            interaction_events,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn reduce_product_dock_back(
        &mut self,
        input: InputSequence,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        expected: WorkspaceVersion,
        root: RootId,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        let target_surface = match self.native_main_dock_back_target(root) {
            Ok(Some(surface)) => surface,
            Ok(None) => {
                return self.reduce_product_action(
                    input,
                    cause,
                    focus_causal,
                    expected,
                    ProductAction::DockBackRoot { root },
                    policy,
                    events,
                    interaction_events,
                );
            }
            Err(reason) => {
                return Ok(InputOutcome::ProductActionRejected {
                    reason,
                    version: self.version,
                });
            }
        };
        self.reduce_product_root_float_action(
            input,
            cause,
            focus_causal,
            expected,
            ProductAction::DockBackRoot { root },
            root,
            target_surface,
            None,
            policy,
            events,
            interaction_events,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn reduce_product_root_float_action(
        &mut self,
        input: InputSequence,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        expected: WorkspaceVersion,
        action: ProductAction,
        root: RootId,
        surface: SurfaceId,
        rect: Option<LogicalRect>,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != self.version {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: self.version,
            });
        }

        if self.root_has_pending_presentation_transition(root) {
            return Ok(InputOutcome::ProductActionRejected {
                reason: DockspaceActionRejection::Conflict,
                version: self.version,
            });
        }
        let source = match self.native_presentation_rehome_source(root, surface) {
            Ok(Some(source)) => source,
            Ok(None) => {
                return self.reduce_product_action(
                    input,
                    cause,
                    focus_causal,
                    expected,
                    action,
                    policy,
                    events,
                    interaction_events,
                );
            }
            Err(reason) => {
                return Ok(InputOutcome::ProductActionRejected {
                    reason,
                    version: self.version,
                });
            }
        };
        if self.pending_drag_release.is_some()
            || self.pending_contained_transform_release.is_some()
            || self.interaction.status() != InteractionStatus::Idle
        {
            return Ok(InputOutcome::ProductActionRejected {
                reason: DockspaceActionRejection::Conflict,
                version: self.version,
            });
        }
        let projection = match self.interaction_projection(surface) {
            Some(projection) => projection,
            None => {
                return Ok(InputOutcome::ProductActionRejected {
                    reason: DockspaceActionRejection::PresentationUnavailable { surface },
                    version: self.version,
                });
            }
        };
        let (root_source, payload, root_items) =
            self.capture_product_root_payload(root).map_err(|_| {
                EngineError::ReductionCauseInvariant {
                    detail: "validated native root payload disappeared during rehome preparation",
                }
            })?;
        let prepared = if matches!(action, ProductAction::DockBackRoot { .. }) {
            match self.prepare_bound_native_main_dock_back(root, source) {
                Ok(prepared) => prepared,
                Err(reason) => {
                    return Ok(InputOutcome::ProductActionRejected {
                        reason,
                        version: self.version,
                    });
                }
            }
        } else {
            let requested = match rect {
                Some(rect) => rect,
                None => match self.derive_product_root_float_rect(root, surface) {
                    Ok(rect) => rect,
                    Err(reason) => {
                        return Ok(InputOutcome::ProductActionRejected {
                            reason,
                            version: self.version,
                        });
                    }
                },
            };
            let rect = match clamp_contained_rect(
                surface,
                projection.plan().bounds(),
                requested,
                self.presentation_config().minimum_floating_size(),
            ) {
                Ok(rect) => rect,
                Err(_) => {
                    return Ok(InputOutcome::ProductActionRejected {
                        reason: DockspaceActionRejection::Conflict,
                        version: self.version,
                    });
                }
            };
            let plan = match self.compile_product_root_float(root, surface, rect) {
                Ok(plan) => plan,
                Err(reason) => {
                    return Ok(InputOutcome::ProductActionRejected {
                        reason,
                        version: self.version,
                    });
                }
            };
            let ProductActionPlan::Command { command, context } = plan else {
                return Err(EngineError::ReductionCauseInvariant {
                    detail: "native root float unexpectedly compiled as a no-op",
                });
            };
            let ProductCommandContext::FloatRootRehome {
                root,
                surface,
                floating,
                items,
            } = context
            else {
                return Err(EngineError::ReductionCauseInvariant {
                    detail: "native root float compiled an unrelated product context",
                });
            };
            PreparedPresentationRehome {
                command,
                root,
                source: root_source,
                target_surface: surface,
                floating: Some(floating),
                visual: PreviewVisual::Contained {
                    surface,
                    rect,
                    fallback: false,
                },
                items,
                context: ProductCommandContext::FloatRootRehome {
                    root,
                    surface,
                    floating,
                    items: root_items,
                },
                commit_authority: PresentationRehomeCommitAuthority::Ordinary,
            }
        };
        let PreparedPresentationRehome {
            command,
            root: planned_root,
            source: planned_source,
            target_surface,
            floating,
            visual,
            items,
            context,
            commit_authority,
        } = prepared;
        if target_surface != surface {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "prepared presentation rehome targeted an unrelated surface",
            });
        }
        let pane_focus = match self.freeze_payload_focus(&payload) {
            Ok(focus) => focus,
            Err(error) => {
                return Ok(InputOutcome::ProductActionRejected {
                    reason: super::product_action::product_command_rejection(action, &error)?,
                    version: self.version,
                });
            }
        };
        let publication_authority = match commit_authority {
            PresentationRehomeCommitAuthority::Ordinary => WorkspacePublicationAuthority::Ordinary,
            PresentationRehomeCommitAuthority::BoundConvertedMain => {
                WorkspacePublicationAuthority::PresentationRehome {
                    source_surface: source.surface,
                    obligation: source.recovery,
                    root: planned_root,
                    floating: floating.ok_or(EngineError::ReductionCauseInvariant {
                        detail: "bound recovery rehome lost its contained identity",
                    })?,
                }
            }
        };
        match self.stage_journal_workspace_command_with_authority(
            cause,
            &command,
            policy,
            publication_authority,
        )? {
            Ok(_) => {}
            Err(error) => {
                return Ok(InputOutcome::ProductActionRejected {
                    reason: super::product_action::product_command_rejection(action, &error)?,
                    version: self.version,
                });
            }
        }
        let preview = self
            .interaction
            .prepare_presentation_rehome_preview(
                self.version.epoch(),
                projection.plan_stamp(),
                visual,
                PreviewProof::PresentationRehome { command },
            )
            .map_err(|source| EngineError::Interaction { input, source })?;
        self.pending_presentation_rehome = Some(PendingPresentationRehome {
            source_version: self.version,
            policy_revision: policy.revision(),
            cause,
            focus_causal,
            root: planned_root,
            source: planned_source,
            source_owner: source.owner,
            source_surface: source.surface,
            source_binding: source.binding,
            source_recovery: source.recovery,
            source_presentation: source.presentation,
            commit_authority,
            target_surface,
            floating,
            context,
            pane_focus,
            preview,
            presentation_outputs: BTreeSet::new(),
            presented_output: None,
            presentation_failed: false,
        });
        Ok(InputOutcome::ProductActionProcessed {
            outcome: DockspaceActionOutcome::RootFloatRequested {
                root: planned_root,
                source_surface: source.surface,
                target_surface,
                items,
            },
            version: self.version,
        })
    }

    pub(super) fn settle_presented_pending_presentation_rehome(
        &mut self,
        events: &mut Vec<WorkspaceEvent>,
    ) -> Result<bool, EngineError> {
        let Some(pending) = self.pending_presentation_rehome.as_ref() else {
            return Ok(false);
        };
        let source_is_current = self
            .bound_surface_recoveries
            .get(&pending.source_surface)
            .is_some_and(|bound| {
                bound.binding == pending.source_binding
                    && bound.obligation.id() == pending.source_recovery
            })
            && self
                .viewport
                .viewport(pending.source_surface)
                .is_some_and(|record| {
                    record.binding() == pending.source_binding
                        && record.admission() == ViewportAdmission::Admitted
                })
            && self
                .interaction_authority(pending.source_surface)
                .is_some_and(|authority| {
                    authority.same_interaction_semantics(pending.source_presentation)
                });
        let rejection = if self.version != pending.source_version {
            Some(crate::event::PresentationRehomeResult::WorkspaceChanged)
        } else if self.policy.revision() != pending.policy_revision {
            Some(crate::event::PresentationRehomeResult::PolicyChanged)
        } else if pending.presentation_failed {
            Some(crate::event::PresentationRehomeResult::TargetNotPresented)
        } else if !source_is_current
            || self.workspace.presentation_for_root(pending.root) != Some(pending.source_owner)
        {
            Some(crate::event::PresentationRehomeResult::SourceUnavailable)
        } else {
            None
        };
        if rejection.is_none() && pending.presented_output.is_none() {
            return Ok(false);
        }
        let pending = self
            .pending_presentation_rehome
            .take()
            .expect("checked pending presentation rehome remains present");
        if let Some(result) = rejection {
            Self::record_presentation_rehome_settlement(&pending, result, self.version, events);
            return Ok(true);
        }
        let PreviewProof::PresentationRehome { command } = pending.preview.proof() else {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "pending presentation rehome lost its frozen command",
            });
        };
        let publication_authority = match pending.commit_authority {
            PresentationRehomeCommitAuthority::Ordinary => {
                WorkspacePublicationAuthority::PendingPresentationRehome { root: pending.root }
            }
            PresentationRehomeCommitAuthority::BoundConvertedMain => {
                WorkspacePublicationAuthority::PresentationRehome {
                    source_surface: pending.source_surface,
                    obligation: pending.source_recovery,
                    root: pending.root,
                    floating: pending
                        .floating
                        .ok_or(EngineError::ReductionCauseInvariant {
                            detail: "bound recovery rehome lost its contained identity",
                        })?,
                }
            }
        };
        let (outcome, changed) = match self.apply_journal_workspace_command_with_authority(
            pending.cause,
            command,
            &self.policy.clone(),
            publication_authority,
            events,
        )? {
            Ok(result) => result,
            Err(_) => {
                Self::record_presentation_rehome_settlement(
                    &pending,
                    crate::event::PresentationRehomeResult::CommandRejected,
                    self.version,
                    events,
                );
                return Ok(true);
            }
        };
        if pending
            .context
            .clone()
            .map_outcome(outcome, changed)
            .is_err()
        {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "presented product rehome produced an unrelated command outcome",
            });
        }
        if let Some(binding) = self
            .viewport
            .viewport(pending.target_surface)
            .filter(|record| record.can_accept_activation())
            .map(crate::viewport_registry::ViewportRecord::binding)
        {
            let _ = self.start_viewport_activation(
                ViewportActivationRequest::drop_committed(binding, pending.pane_focus),
                pending.focus_causal,
                events,
            )?;
        }
        Self::record_presentation_rehome_settlement(
            &pending,
            crate::event::PresentationRehomeResult::Applied,
            self.version,
            events,
        );
        Ok(true)
    }

    fn record_presentation_rehome_settlement(
        pending: &PendingPresentationRehome,
        result: crate::event::PresentationRehomeResult,
        version: WorkspaceVersion,
        events: &mut Vec<WorkspaceEvent>,
    ) {
        events.push(WorkspaceEvent::new_caused(
            pending.cause,
            version,
            WorkspaceEventKind::PresentationRehomeSettled {
                root: pending.root,
                source_surface: pending.source_surface,
                target_surface: pending.target_surface,
                result,
            },
        ));
    }
}

fn product_dock_target_root(target: &crate::command::DockTarget) -> RootId {
    match target {
        crate::command::DockTarget::Center(target)
        | crate::command::DockTarget::TabGap { target, .. } => target.root(),
        crate::command::DockTarget::InnerEdge(target)
        | crate::command::DockTarget::OuterEdge(target) => target.root(),
    }
}

fn product_drop_target_id(target: &crate::command::DockTarget) -> crate::drop_target::DropTargetId {
    match target {
        crate::command::DockTarget::Center(target) => crate::drop_target::DropTargetId::Center {
            surface: target.surface(),
            root: target.root(),
            tabs: target.tabs(),
        },
        crate::command::DockTarget::TabGap { target, index } => {
            crate::drop_target::DropTargetId::TabGap {
                surface: target.surface(),
                root: target.root(),
                tabs: target.tabs(),
                index: *index,
            }
        }
        crate::command::DockTarget::InnerEdge(target) => {
            crate::drop_target::DropTargetId::InnerEdge {
                surface: target.surface(),
                root: target.root(),
                node: target.node(),
                edge: target.edge(),
            }
        }
        crate::command::DockTarget::OuterEdge(target) => {
            crate::drop_target::DropTargetId::OuterEdge {
                surface: target.surface(),
                root: target.root(),
                node: target.node(),
                edge: target.edge(),
            }
        }
    }
}

const fn presentation_unavailability_is_terminal(
    reason: HostPresentationUnavailableReason,
) -> bool {
    match reason {
        HostPresentationUnavailableReason::OutputNotProduced
        | HostPresentationUnavailableReason::SupersededBeforePublication => false,
        HostPresentationUnavailableReason::FinalPresentationUnobservable
        | HostPresentationUnavailableReason::RetainedResourceUnavailable
        | HostPresentationUnavailableReason::TransientVisualNotPainted
        | HostPresentationUnavailableReason::BackendFailure => true,
    }
}

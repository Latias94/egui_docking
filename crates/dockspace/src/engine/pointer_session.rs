//! Pointer ownership gates and interaction-session retirement.

use super::*;

impl DockEngine {
    fn native_capture_binding_is_current(&self, binding: ViewportBinding) -> bool {
        self.workspace.surface(binding.surface()).is_some()
            && self
                .viewport
                .viewport(binding.surface())
                .is_some_and(|record| {
                    record.binding() == binding
                        && record.admission() == ViewportAdmission::Admitted
                        && record.is_observed()
                })
    }

    fn active_journal_required_capture_owner(
        &self,
        owner: GestureOwner,
    ) -> Result<PointerCaptureOwner, EngineError> {
        let stream = owner.stream().ok_or(EngineError::ReductionCauseInvariant {
            detail: "journal capture authority was requested for a legacy owner",
        })?;
        if self.interaction.active_stream() != Some(stream) {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "journal capture authority was requested for an inactive stream",
            });
        }
        match stream.lease().scope() {
            PointerProviderScope::HostLocal(_) | PointerProviderScope::SurfaceLocal(_) => {
                Ok(PointerCaptureOwner::ProviderEndpoint)
            }
            PointerProviderScope::DesktopGlobal => self
                .interaction
                .active_presentation_authority()
                .and_then(|presentation| presentation.presented.binding())
                .map(PointerCaptureOwner::Native)
                .ok_or(EngineError::ReductionCauseInvariant {
                    detail: "desktop journal gesture has no press-bound native capture owner",
                }),
        }
    }

    fn active_journal_required_delivery_owner(
        &self,
        owner: GestureOwner,
    ) -> Result<PointerEventDeliveryOwner, EngineError> {
        let stream = owner.stream().ok_or(EngineError::ReductionCauseInvariant {
            detail: "journal delivery authority was requested for a legacy owner",
        })?;
        if self.interaction.active_stream() != Some(stream) {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "journal delivery authority was requested for an inactive stream",
            });
        }
        match stream.lease().scope() {
            PointerProviderScope::HostLocal(_) | PointerProviderScope::SurfaceLocal(_) => {
                Ok(PointerEventDeliveryOwner::ProviderEndpoint)
            }
            PointerProviderScope::DesktopGlobal => self
                .interaction
                .active_presentation_authority()
                .and_then(|presentation| presentation.presented.binding())
                .map(PointerEventDeliveryOwner::Native)
                .ok_or(EngineError::ReductionCauseInvariant {
                    detail: "desktop journal gesture has no press-bound native delivery owner",
                }),
        }
    }

    fn journal_delivery_action_gate(
        &self,
        owner: GestureOwner,
        observed: Authority<PointerEventDeliveryOwner>,
    ) -> Result<JournalCaptureActionGate, EngineError> {
        let required = self.active_journal_required_delivery_owner(owner)?;
        let Authority::Known(observed) = observed else {
            return Ok(JournalCaptureActionGate::Unavailable);
        };
        if observed != required {
            return Ok(JournalCaptureActionGate::Lost);
        }
        if let PointerEventDeliveryOwner::Native(binding) = observed
            && !self.native_capture_binding_is_current(binding)
        {
            return Ok(JournalCaptureActionGate::Lost);
        }
        Ok(JournalCaptureActionGate::Authorized)
    }

    pub(super) fn gate_journal_delivery_action(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        observed: Authority<PointerEventDeliveryOwner>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<(bool, Option<InteractionOutcome>), EngineError> {
        match self.journal_delivery_action_gate(owner, observed)? {
            JournalCaptureActionGate::Authorized => Ok((true, None)),
            JournalCaptureActionGate::Unavailable => Ok((
                false,
                self.cancel_gesture_owner(
                    cause,
                    owner,
                    InteractionCancelReason::DeliveryAuthorityUnavailable,
                    interaction_events,
                )?,
            )),
            JournalCaptureActionGate::Lost => Ok((
                false,
                self.cancel_gesture_owner(
                    cause,
                    owner,
                    InteractionCancelReason::DeliveryOwnerLost,
                    interaction_events,
                )?,
            )),
        }
    }

    fn journal_capture_action_gate(
        &self,
        owner: GestureOwner,
        observed: Authority<PointerCaptureOwner>,
        release_edge: bool,
    ) -> Result<JournalCaptureActionGate, EngineError> {
        let stream = owner.stream().ok_or(EngineError::ReductionCauseInvariant {
            detail: "journal capture action was reduced for a legacy owner",
        })?;
        if matches!(
            stream.lease().scope(),
            PointerProviderScope::HostLocal(_) | PointerProviderScope::SurfaceLocal(_)
        ) {
            // The immutable local lease already confines delivery to one
            // endpoint. `None` means that endpoint delivered without platform
            // capture; only `Foreign` or unavailable authority blocks it.
            return Ok(match observed {
                Authority::Unknown(_) => JournalCaptureActionGate::Unavailable,
                Authority::Known(
                    PointerCaptureOwner::ProviderEndpoint | PointerCaptureOwner::None,
                ) => JournalCaptureActionGate::Authorized,
                Authority::Known(PointerCaptureOwner::Foreign | PointerCaptureOwner::Native(_)) => {
                    JournalCaptureActionGate::Lost
                }
            });
        }
        let required = self.active_journal_required_capture_owner(owner)?;
        let Authority::Known(observed) = observed else {
            // A desktop provider may lack persistent capture authority while
            // the native event source still proves exact edge delivery. The
            // caller validates that independent delivery fact first.
            return Ok(JournalCaptureActionGate::Authorized);
        };
        if release_edge && observed == PointerCaptureOwner::None {
            // Release carries post-edge capture authority. Relinquishing an
            // exact pre-edge owner is valid; an unknown or foreign predecessor
            // cannot authorize the terminal action.
            let current = self.interaction.active_journal_capture_authority().ok_or(
                EngineError::ReductionCauseInvariant {
                    detail: "journal release has no pre-edge capture observation",
                },
            )?;
            return Ok(match current {
                Authority::Known(current)
                    if current == required
                        && !matches!(current, PointerCaptureOwner::Native(binding)
                            if !self.native_capture_binding_is_current(binding)) =>
                {
                    JournalCaptureActionGate::Authorized
                }
                Authority::Unknown(_) => JournalCaptureActionGate::Unavailable,
                Authority::Known(_) => JournalCaptureActionGate::Lost,
            });
        }
        if observed != required {
            return Ok(JournalCaptureActionGate::Lost);
        }
        if let PointerCaptureOwner::Native(binding) = observed
            && !self.native_capture_binding_is_current(binding)
        {
            return Ok(JournalCaptureActionGate::Lost);
        }
        Ok(JournalCaptureActionGate::Authorized)
    }

    pub(super) fn gate_journal_capture_action(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        observed: Authority<PointerCaptureOwner>,
        release_edge: bool,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<(bool, Option<InteractionOutcome>), EngineError> {
        let gate = self.journal_capture_action_gate(owner, observed, release_edge)?;
        if !release_edge || !matches!(observed, Authority::Known(PointerCaptureOwner::None)) {
            let stream = owner.stream().ok_or(EngineError::ReductionCauseInvariant {
                detail: "journal capture action was reduced for a legacy owner",
            })?;
            if !matches!(gate, JournalCaptureActionGate::Lost)
                && !self
                    .interaction
                    .update_journal_capture_authority(stream, observed)
            {
                return Err(EngineError::ReductionCauseInvariant {
                    detail: "journal capture action lost its active gesture",
                });
            }
        }
        match gate {
            JournalCaptureActionGate::Authorized => Ok((true, None)),
            JournalCaptureActionGate::Unavailable => Ok((false, None)),
            JournalCaptureActionGate::Lost => Ok((
                false,
                self.cancel_gesture_owner(
                    cause,
                    owner,
                    InteractionCancelReason::CaptureLost,
                    interaction_events,
                )?,
            )),
        }
    }

    pub(super) fn reduce_journal_capture_change(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        observed: Authority<PointerCaptureOwner>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<Vec<InteractionOutcome>, EngineError> {
        let Some(stream) = owner.stream() else {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "journal capture change was reduced for a legacy owner",
            });
        };
        if self.interaction.active_stream() != Some(stream) {
            return Ok(Vec::new());
        }
        let required = self.active_journal_required_capture_owner(owner)?;
        let capture_lost = match observed {
            Authority::Unknown(_) => false,
            Authority::Known(actual) => {
                actual != required
                    || matches!(actual, PointerCaptureOwner::Native(binding)
                        if !self.native_capture_binding_is_current(binding))
            }
        };
        if capture_lost {
            return Ok(self
                .cancel_gesture_owner(
                    cause,
                    owner,
                    InteractionCancelReason::CaptureLost,
                    interaction_events,
                )?
                .into_iter()
                .collect());
        }
        if !self
            .interaction
            .update_journal_capture_authority(stream, observed)
        {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "journal capture observation lost its active gesture",
            });
        }
        Ok(Vec::new())
    }

    pub(super) fn journal_press_delivery_rejection(
        &self,
        observed: Authority<PointerEventDeliveryOwner>,
        expected: PointerEventDeliveryOwner,
    ) -> Result<Option<InteractionRejection>, EngineError> {
        let Authority::Known(actual) = observed else {
            return Ok(Some(InteractionRejection::DeliveryAuthorityUnavailable));
        };
        let compatible = match expected {
            PointerEventDeliveryOwner::Native(binding) => {
                actual == expected && self.native_capture_binding_is_current(binding)
            }
            PointerEventDeliveryOwner::ProviderEndpoint
            | PointerEventDeliveryOwner::Foreign
            | PointerEventDeliveryOwner::None => actual == expected,
        };
        Ok((!compatible)
            .then_some(InteractionRejection::DeliveryOwnerMismatch { expected, actual }))
    }

    pub(super) fn journal_press_capture_rejection(
        &self,
        owner: GestureOwner,
        observed: Authority<PointerCaptureOwner>,
        presentation: &JournalSurfacePresentation,
    ) -> Result<Option<InteractionRejection>, EngineError> {
        let stream = owner.stream().ok_or(EngineError::ReductionCauseInvariant {
            detail: "journal primary press was reduced for a legacy owner",
        })?;
        let Authority::Known(actual) = observed else {
            return Ok(None);
        };

        let (expected, compatible) = match stream.lease().scope() {
            PointerProviderScope::HostLocal(_) | PointerProviderScope::SurfaceLocal(_) => (
                PointerCaptureOwner::ProviderEndpoint,
                matches!(
                    actual,
                    PointerCaptureOwner::ProviderEndpoint | PointerCaptureOwner::None
                ),
            ),
            PointerProviderScope::DesktopGlobal => {
                let Some(binding) = presentation.authority().binding() else {
                    return Ok(Some(InteractionRejection::TargetAuthorityInvalid));
                };
                let expected = PointerCaptureOwner::Native(binding);
                (
                    expected,
                    actual == expected && self.native_capture_binding_is_current(binding),
                )
            }
        };
        Ok(
            (!compatible)
                .then_some(InteractionRejection::CaptureOwnerMismatch { expected, actual }),
        )
    }

    pub(super) fn cancel_pending_release_obligations_caused(
        &mut self,
        cause: ReductionCause,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Vec<InteractionOutcome> {
        let cancelled = usize::from(self.pending_drag_release.take().is_some())
            + usize::from(self.pending_contained_transform_release.take().is_some());
        (0..cancelled)
            .map(|_| {
                let status = InteractionStatus::Idle;
                interaction_events.push(InteractionEvent::new_caused(
                    cause,
                    self.version,
                    InteractionEventKind::Cancelled { status, reason },
                ));
                InteractionOutcome::Cancelled { status, reason }
            })
            .collect()
    }

    pub(super) fn cancel_pending_release_obligations_input(
        &mut self,
        input: InputSequence,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) {
        let cancelled = usize::from(self.pending_drag_release.take().is_some())
            + usize::from(self.pending_contained_transform_release.take().is_some());
        interaction_events.extend((0..cancelled).map(|_| {
            InteractionEvent::new(
                input,
                self.version,
                InteractionEventKind::Cancelled {
                    status: InteractionStatus::Idle,
                    reason,
                },
            )
        }));
    }

    pub(super) fn cancel_invalid_pending_release_obligations_caused(
        &mut self,
        cause: ReductionCause,
        interaction_events: &mut Vec<InteractionEvent>,
    ) {
        let reason = self
            .pending_drag_release
            .as_ref()
            .map(|pending| (pending.source_version, pending.policy_revision))
            .or_else(|| {
                self.pending_contained_transform_release
                    .as_ref()
                    .map(|pending| (pending.source_version, pending.policy_revision))
            })
            .and_then(|(source_version, policy_revision)| {
                if self.version != source_version {
                    Some(InteractionCancelReason::WorkspaceChanged)
                } else if self.policy.revision() != policy_revision {
                    Some(InteractionCancelReason::PolicyChanged)
                } else {
                    None
                }
            });
        if let Some(reason) = reason {
            let _ =
                self.cancel_pending_release_obligations_caused(cause, reason, interaction_events);
        }
    }

    /// Cancels an exact journal or local-response owner through one terminal path.
    pub(super) fn cancel_gesture_owner(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<Option<InteractionOutcome>, EngineError> {
        if self.interaction.active_owner() != Some(owner) {
            return Ok(None);
        }
        if let Some(pointer) = owner.pointer_if_physical() {
            self.viewport
                .end_drag_routing(pointer)
                .map_err(|source| EngineError::Viewport {
                    input: self.last_input,
                    source,
                })?;
        }
        let status =
            self.interaction
                .cancel_owner(owner)
                .ok_or(EngineError::ReductionCauseInvariant {
                    detail: "active gesture could not cancel its exact owner",
                })?;
        interaction_events.push(InteractionEvent::new_caused(
            cause,
            self.version,
            InteractionEventKind::Cancelled { status, reason },
        ));
        Ok(Some(InteractionOutcome::Cancelled { status, reason }))
    }

    pub(super) fn cancel_retired_pointer_owner(
        &mut self,
        cause: ReductionCause,
        provider: PointerInputLease,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<(), EngineError> {
        for (session, receiver) in self.scroll_interaction.retire_provider(provider) {
            interaction_events.push(InteractionEvent::new_caused(
                cause,
                self.version,
                InteractionEventKind::ScrollTerminated {
                    session,
                    receiver,
                    reason: ScrollTerminationReason::ProviderRetired,
                },
            ));
        }
        let Some(stream) = self
            .interaction
            .active_stream()
            .filter(|stream| stream.lease() == provider)
        else {
            return Ok(());
        };
        self.viewport
            .end_all_drag_routing()
            .map_err(|source| EngineError::Viewport {
                input: self.last_input,
                source,
            })?;
        if let Some(status) = self.interaction.cancel_owner(GestureOwner::Stream(stream)) {
            interaction_events.push(InteractionEvent::new_caused(
                cause,
                self.version,
                InteractionEventKind::Cancelled { status, reason },
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Node, RootRecord, SurfacePresentation};
    use crate::ids::{ItemId, ReducerTickId};
    use crate::interaction::DragSessionId;

    fn local_drag() -> (DockEngine, GestureOwner, DragSessionId) {
        let surface = SurfaceId::new(1);
        let root = RootId::new(1);
        let item = ItemId::new(1);
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([item]));
        builder.set_root(root, RootRecord::new(tabs));
        builder.set_surface(surface, SurfacePresentation::with_main(root));
        let workspace = builder.build().expect("test workspace is valid");
        let source = workspace
            .capture_item_source(root, tabs, item)
            .expect("test drag source exists");
        let mut engine =
            DockEngine::new(workspace, DockPolicy::default()).expect("test engine is valid");
        let owner = GestureOwner::LocalResponse { surface };
        let start = DragArmStart {
            epoch: engine.version.epoch(),
            owner,
            journal_capture_authority: None,
            button: PointerButton::Primary,
            payload: MovePayload::Item(source),
            source_surface: surface,
            initial_pointer: None,
            journal_threshold_origin: None,
            complete_root: None,
            partial_detachable: false,
            origin: FrozenDragOrigin::Workspace,
            source_validated_at: engine.version,
            presentation: DragGestureAuthority::LocalReady {
                surface,
                coordinates: engine.capture_surface_coordinates(surface),
            },
            source_layout_facts: None,
            journal_source_geometry: None,
            continuation: None,
        };
        let (session, _) = engine.interaction.arm_drag(start).expect("test drag arms");
        engine
            .interaction
            .begin_drag(session, owner, PointerButton::Primary)
            .expect("test drag begins");
        (engine, owner, session)
    }

    #[test]
    fn local_preview_cancellation_preserves_cause_and_terminates_once() {
        let (mut engine, owner, session) = local_drag();
        let cause = ReductionCause::SurfacePresentationObservationBatch {
            tick: ReducerTickId::new(1),
        };
        let reason = InteractionCancelReason::SceneUnavailable;
        let mut events = Vec::new();
        let outcome = engine
            .apply_preview_evaluation(
                cause,
                owner,
                session,
                PreviewEvaluation::without_affordance(PreviewDecision::Cancel(reason)),
                &mut events,
            )
            .expect("local preview cancellation must not require a physical pointer");
        assert_eq!(
            outcome,
            Some(InteractionOutcome::Cancelled {
                status: InteractionStatus::Dragging { session },
                reason,
            })
        );
        assert_eq!(engine.interaction.status(), InteractionStatus::Idle);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].cause(), cause);
        assert_eq!(
            engine
                .cancel_gesture_owner(cause, owner, reason, &mut events)
                .expect("repeated cancellation is harmless"),
            None
        );
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn cancelling_another_local_surface_preserves_the_active_drag() {
        let (mut engine, _, session) = local_drag();
        let cause = ReductionCause::SurfacePresentationObservationBatch {
            tick: ReducerTickId::new(1),
        };
        let mut events = Vec::new();
        assert_eq!(
            engine
                .cancel_gesture_owner(
                    cause,
                    GestureOwner::LocalResponse {
                        surface: SurfaceId::new(2),
                    },
                    InteractionCancelReason::LocalResponseCancelled,
                    &mut events,
                )
                .expect("another local owner cannot cancel this gesture"),
            None
        );
        assert_eq!(
            engine.interaction.status(),
            InteractionStatus::Dragging { session }
        );
        assert!(events.is_empty());
    }
}

//! Freezes a released drag and settles its exact presentation obligation.

use super::*;

impl DockEngine {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::engine) fn defer_drag_release(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        session: crate::interaction::DragSessionId,
        drag: crate::interaction::ActiveDrag,
        physical_route: Option<(
            crate::viewport::ViewportBinding,
            crate::platform::NativeDragSourceInput,
        )>,
        release_decision: PreviewDecision,
        policy: &DockPolicySnapshot,
    ) -> Result<InteractionOutcome, EngineError> {
        if self.pending_mutation.is_some() {
            return Err(EngineError::PointerInteractionInvariant {
                cause,
                detail: "a drag release presentation obligation is already pending".to_owned(),
            });
        }
        let preview = drag
            .preview()
            .expect("deferred release has an exact matching preview")
            .public()
            .token();
        let presentation = PresentationProgress::waiting(
            self.presentation_authority
                .presentation
                .pending_drag_preview_output_keys(preview),
        );
        self.pending_mutation = Some(PendingMutation::DragRelease(PendingDragRelease {
            source_version: self.version,
            policy_revision: policy.revision(),
            cause,
            focus_causal,
            session,
            drag,
            physical_route,
            release_decision,
            preview,
            presentation,
        }));
        Ok(InteractionOutcome::ReleasePending { session, preview })
    }

    pub(in crate::engine) fn settle_presented_pending_drag_release(
        &mut self,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<bool, EngineError> {
        let Some(pending) = self
            .pending_mutation
            .as_ref()
            .and_then(PendingMutation::drag_release)
        else {
            return Ok(false);
        };
        if pending.presentation.awaiting_presentation() {
            return Ok(false);
        }
        let Some(PendingMutation::DragRelease(pending)) = self.pending_mutation.take() else {
            unreachable!("checked pending release remains present");
        };
        let cancellation = if self.version != pending.source_version {
            Some(InteractionCancelReason::WorkspaceChanged)
        } else if self.policy.revision() != pending.policy_revision {
            Some(InteractionCancelReason::PolicyChanged)
        } else if pending.presentation.is_failed() {
            Some(InteractionCancelReason::SceneUnavailable)
        } else {
            None
        };
        if let Some(reason) = cancellation {
            interaction_events.push(InteractionEvent::new_caused(
                pending.cause,
                self.version,
                InteractionEventKind::Cancelled {
                    status: InteractionStatus::Dragging {
                        session: pending.session,
                    },
                    reason,
                },
            ));
            return Ok(true);
        }

        if !self.viewport.native_binding_allocation_available()
            && matches!(&pending.release_decision,
                PreviewDecision::Publish { proof, .. } if matches!(proof.as_ref(), PreviewProof::Native { .. }))
        {
            // A presented preview reserves its exact release, not a future
            // native lifetime. Capacity rejection is one terminal outcome;
            // replaying the accepted release cannot create more storage.
            let rejection = InteractionRejection::CommandRejected(
                crate::error::CommandError::SurfaceLifecycleFrozen {
                    surface: pending.drag.source_surface,
                },
            );
            interaction_events.push(InteractionEvent::new_caused(
                pending.cause,
                self.version,
                InteractionEventKind::Rejected {
                    status: InteractionStatus::Dragging {
                        session: pending.session,
                    },
                    reason: crate::interaction::ManipulationRejection::from_interaction(&rejection),
                },
            ));
            return Ok(true);
        }

        let cause = pending.cause;
        let policy = self.policy.clone();
        let outcome = self.deliver_drag_release(
            cause,
            pending.focus_causal,
            pending.session,
            pending.drag,
            pending.release_decision,
            None,
            &policy,
            events,
            interaction_events,
        )?;
        match outcome {
            InteractionOutcome::DragDelivered { .. } => Ok(true),
            InteractionOutcome::Rejected(rejection) => {
                interaction_events.push(InteractionEvent::new_caused(
                    cause,
                    self.version,
                    InteractionEventKind::Rejected {
                        status: InteractionStatus::Dragging {
                            session: pending.session,
                        },
                        reason: crate::interaction::ManipulationRejection::from_interaction(
                            &rejection,
                        ),
                    },
                ));
                Ok(true)
            }
            outcome => Err(EngineError::PointerInteractionInvariant {
                cause,
                detail: format!(
                    "presented release obligation ended without delivery or rejection: {outcome:?}"
                ),
            }),
        }
    }
}

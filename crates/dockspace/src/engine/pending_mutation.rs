//! One pending durable mutation and its exact presentation settlement.

use super::*;

mod drag_release;
mod presentation;
mod termination;
#[cfg(test)]
mod tests;

pub(super) use presentation::PresentationProgress;

#[derive(Debug, Clone, PartialEq)]
pub(super) enum PendingMutation {
    DragRelease(PendingDragRelease),
    Rehome(PendingPresentationRehome),
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct PendingDragRelease {
    pub(super) source_version: WorkspaceVersion,
    pub(super) policy_revision: PolicyRevision,
    pub(super) cause: ReductionCause,
    pub(super) focus_causal: FocusCausalStamp,
    pub(super) session: crate::interaction::DragSessionId,
    pub(super) drag: crate::interaction::ActiveDrag,
    pub(super) physical_route: Option<(
        crate::platform::viewport::ViewportBinding,
        crate::platform::NativeDragSourceInput,
    )>,
    pub(super) release_decision: PreviewDecision,
    pub(super) preview: crate::interaction::PreviewToken,
    pub(super) presentation: PresentationProgress,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct PendingPresentationRehome {
    pub(super) source_version: WorkspaceVersion,
    pub(super) policy_revision: PolicyRevision,
    pub(super) cause: ReductionCause,
    pub(super) focus_causal: FocusCausalStamp,
    pub(super) root: RootId,
    pub(super) source: crate::graph::command::NodeSource,
    pub(super) source_owner: crate::graph::workspace::RootPresentationOwner,
    pub(super) source_surface: SurfaceId,
    pub(super) source_binding: crate::platform::viewport::ViewportBinding,
    pub(super) source_recovery: SurfaceRecoveryObligationId,
    pub(super) source_presentation: PresentedSurfaceAuthority,
    pub(super) commit_authority: PresentationRehomeCommitAuthority,
    pub(super) target_surface: SurfaceId,
    pub(super) floating: Option<FloatingPresentationId>,
    pub(super) context: product_action::ProductCommandContext,
    pub(super) pane_focus: PaneFocusDisposition,
    pub(super) preview: crate::interaction::PublishedPreview,
    pub(super) presentation: PresentationProgress,
}

impl PendingMutation {
    pub(super) fn drag_release(&self) -> Option<&PendingDragRelease> {
        match self {
            Self::DragRelease(pending) => Some(pending),
            Self::Rehome(_) => None,
        }
    }

    pub(super) fn rehome(&self) -> Option<&PendingPresentationRehome> {
        match self {
            Self::Rehome(pending) => Some(pending),
            Self::DragRelease(_) => None,
        }
    }

    pub(super) fn rehome_mut(&mut self) -> Option<&mut PendingPresentationRehome> {
        match self {
            Self::Rehome(pending) => Some(pending),
            Self::DragRelease(_) => None,
        }
    }

    pub(super) fn presentation(&self) -> &PresentationProgress {
        match self {
            Self::DragRelease(pending) => &pending.presentation,
            Self::Rehome(pending) => &pending.presentation,
        }
    }

    pub(super) fn presentation_mut(&mut self) -> &mut PresentationProgress {
        match self {
            Self::DragRelease(pending) => &mut pending.presentation,
            Self::Rehome(pending) => &mut pending.presentation,
        }
    }

    pub(super) fn surface(&self) -> SurfaceId {
        match self {
            Self::DragRelease(pending) => pending
                .drag
                .preview()
                .expect("pending release retains its exact preview")
                .public()
                .visual()
                .surface(),
            Self::Rehome(pending) => pending.target_surface,
        }
    }

    pub(super) fn matches_interaction(&self, interaction: HostInteractionPresentation) -> bool {
        match self {
            Self::DragRelease(pending) => interaction.drag_preview() == Some(pending.preview),
            Self::Rehome(pending) => {
                interaction.drag_preview() == Some(pending.preview.public().token())
            }
        }
    }

    pub(super) fn gesture(&self) -> Option<(PointerInputLease, InteractionStatus)> {
        match self {
            Self::DragRelease(pending) => Some((
                pending.drag.owner.lease(),
                InteractionStatus::Dragging {
                    session: pending.session,
                },
            )),
            Self::Rehome(_) => None,
        }
    }

    pub(super) fn source_presentation(&self) -> HostFrameKey {
        match self {
            Self::DragRelease(pending) => pending.drag.presentation.presented.emission(),
            Self::Rehome(pending) => pending.source_presentation.emission(),
        }
    }
}

impl DockEngine {
    pub(super) fn pending_mutation_output_matches(
        &self,
        surface: SurfaceId,
        interaction: HostInteractionPresentation,
    ) -> Result<bool, EngineError> {
        let Some(pending) = self
            .pending_mutation
            .as_ref()
            .filter(|pending| pending.matches_interaction(interaction))
        else {
            return Ok(false);
        };
        if pending.surface() != surface {
            return Err(EngineError::PresentationLedger {
                detail: "pending mutation preview was emitted on the wrong surface".to_owned(),
            });
        }
        Ok(true)
    }

    pub(super) fn settle_presented_pending_mutation(
        &mut self,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<bool, EngineError> {
        match self.pending_mutation.as_ref() {
            Some(PendingMutation::DragRelease(_)) => {
                self.settle_presented_pending_drag_release(events, interaction_events)
            }
            Some(PendingMutation::Rehome(_)) => {
                self.settle_presented_pending_presentation_rehome(events)
            }
            None => Ok(false),
        }
    }
}

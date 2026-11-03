//! One pending durable mutation and its exact presentation settlement.

use super::*;

#[derive(Debug, Clone, PartialEq)]
pub(super) enum PendingMutation {
    DragRelease(PendingDragRelease),
    Rehome(PendingPresentationRehome),
}

/// Shared progress of the one presentation-gated mutation. The semantic
/// command and terminal outcome remain specific to each mutation variant.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct PendingPresentationTracker {
    pub(super) outputs: BTreeSet<HostFrameKey>,
    pub(super) presented: Option<HostFrameKey>,
    pub(super) failed: bool,
}

impl PendingPresentationTracker {
    pub(super) fn awaiting_presentation(&self) -> bool {
        self.presented.is_none() && !self.failed
    }

    pub(super) fn retire_keys(&mut self, retired: &BTreeSet<HostFrameKey>) {
        if self.presented.is_some() || self.outputs.is_empty() {
            return;
        }
        let before = self.outputs.len();
        self.outputs.retain(|output| !retired.contains(output));
        self.failed |= self.outputs.len() < before && self.outputs.is_empty();
    }

    pub(super) fn retire_prefixes(&mut self, outcomes: &[HostPresentationObservationOutcome]) {
        if self.presented.is_some() || self.outputs.is_empty() {
            return;
        }
        let before = self.outputs.len();
        self.outputs.retain(|output| {
            !outcomes.iter().any(|outcome| {
                matches!(outcome, HostPresentationObservationOutcome::Retired {
                    stream, settled_through, ..
                } if output.stream() == *stream && *output <= *settled_through)
            })
        });
        self.failed |= self.outputs.len() < before && self.outputs.is_empty();
    }

    pub(super) fn retain_emissions(&self, emissions: &mut BTreeSet<HostFrameKey>) {
        emissions.extend(self.outputs.iter().copied());
        emissions.extend(self.presented);
    }
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

    pub(super) fn presentation(&self) -> &PendingPresentationTracker {
        match self {
            Self::DragRelease(pending) => &pending.presentation,
            Self::Rehome(pending) => &pending.presentation,
        }
    }

    pub(super) fn presentation_mut(&mut self) -> &mut PendingPresentationTracker {
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

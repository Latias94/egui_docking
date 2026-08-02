//! Device-independent keyboard interaction reduction.

use crate::ids::{InputSequence, SurfaceId};
use crate::interaction::{
    EscapeDelivery, InteractionCancelReason, InteractionEvent, InteractionOutcome,
    InteractionRejection, InteractionStatus,
};
use crate::transition::{InputOutcome, WorkspaceVersion};

use super::{DockEngine, EngineError};

impl DockEngine {
    pub(super) fn reduce_escape_input(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        delivery: EscapeDelivery,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        self.reduce_versioned_interaction(expected, |engine| {
            engine.cancel_active_interaction_with_escape(input, delivery, interaction_events)
        })
    }

    fn cancel_active_interaction_with_escape(
        &mut self,
        input: InputSequence,
        delivery: EscapeDelivery,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let status = self.interaction.status();
        if status == InteractionStatus::Idle {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::NoActiveGesture,
            ));
        }
        if !self.escape_delivery_is_current(delivery) {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::EscapeDeliveryUnavailable { delivery },
            ));
        }

        match status {
            InteractionStatus::Idle => unreachable!("idle was rejected above"),
            InteractionStatus::Pressed { session } => Ok(self.cancel_click(
                input,
                session,
                InteractionCancelReason::Escape,
                interaction_events,
            )),
            InteractionStatus::Armed { session } | InteractionStatus::Dragging { session } => self
                .cancel_drag(
                    input,
                    session,
                    InteractionCancelReason::Escape,
                    interaction_events,
                ),
            InteractionStatus::Resizing { session } => Ok(self.cancel_resize(
                input,
                session,
                InteractionCancelReason::Escape,
                interaction_events,
            )),
            InteractionStatus::ContainedTransforming { session } => Ok(self
                .cancel_contained_transform(
                    input,
                    session,
                    InteractionCancelReason::Escape,
                    interaction_events,
                )),
        }
    }

    fn escape_delivery_is_current(&self, delivery: EscapeDelivery) -> bool {
        match delivery {
            EscapeDelivery::Surface(surface) => {
                self.active_interaction_source_surface() == Some(surface)
            }
            EscapeDelivery::NativeBinding(binding) => self
                .viewport
                .viewport(binding.surface())
                .is_some_and(|record| record.binding() == binding),
        }
    }

    fn active_interaction_source_surface(&self) -> Option<SurfaceId> {
        match self.interaction.status() {
            InteractionStatus::Idle => None,
            InteractionStatus::Pressed { .. } => self
                .interaction
                .active_click_view()
                .map(|click| click.surface()),
            InteractionStatus::Armed { .. } | InteractionStatus::Dragging { .. } => self
                .interaction
                .active_drag_view()
                .and_then(|drag| self.payload_surface(drag.payload())),
            InteractionStatus::Resizing { .. } => self
                .interaction
                .active_resize_view()
                .map(|resize| resize.surface()),
            InteractionStatus::ContainedTransforming { .. } => self
                .interaction
                .active_contained_transform_view()
                .map(|transform| transform.surface()),
        }
    }
}

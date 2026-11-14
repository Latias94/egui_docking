//! Device-independent gesture cancellation and immediate resize recovery.

use crate::engine::transition::{InputOutcome, WorkspaceVersion};
use crate::ids::{InputSequence, SurfaceId};
#[cfg(feature = "motion")]
use crate::interaction::ActiveResizeView;
use crate::interaction::{
    GestureCancelDelivery, GestureOwner, InteractionCancelReason, InteractionEvent,
    InteractionOutcome, InteractionRejection, InteractionStatus,
};

use super::{DockEngine, EngineError};

impl DockEngine {
    /// Cancels state after the caller has qualified its delivery or owner.
    /// Resize rollback remains immediate even if the last preview was never presented.
    pub(super) fn cancel_interaction_state(
        &mut self,
        owner: Option<GestureOwner>,
    ) -> Option<InteractionStatus> {
        #[cfg(feature = "motion")]
        let resize_surface = self
            .interaction
            .active_resize_view()
            .map(ActiveResizeView::surface);
        let status = match owner {
            Some(owner) => self.interaction.cancel_owner(owner),
            None => self.interaction.cancel_active(),
        }?;
        #[cfg(feature = "motion")]
        if let Some(surface) = resize_surface {
            self.settle_resize_motion(surface);
        }
        Some(status)
    }

    pub(super) fn reduce_gesture_cancel_input(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        delivery: GestureCancelDelivery,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        self.reduce_versioned_interaction(expected, |engine| {
            engine.cancel_active_interaction(input, delivery, interaction_events)
        })
    }

    fn cancel_active_interaction(
        &mut self,
        input: InputSequence,
        delivery: GestureCancelDelivery,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let status = self.interaction.status();
        if status == InteractionStatus::Idle {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::NoActiveGesture,
            ));
        }
        if !self.gesture_cancel_delivery_is_current(delivery) {
            return Ok(InteractionOutcome::Rejected(
                InteractionRejection::GestureCancelDeliveryUnavailable { delivery },
            ));
        }

        match status {
            InteractionStatus::Idle => unreachable!("idle was rejected above"),
            InteractionStatus::Pressed { session } => Ok(self.cancel_click(
                input,
                session,
                InteractionCancelReason::ExplicitCancellation,
                interaction_events,
            )),
            InteractionStatus::Armed { session } | InteractionStatus::Dragging { session } => self
                .cancel_drag(
                    input,
                    session,
                    InteractionCancelReason::ExplicitCancellation,
                    interaction_events,
                ),
            InteractionStatus::Resizing { session } => Ok(self.cancel_resize(
                input,
                session,
                InteractionCancelReason::ExplicitCancellation,
                interaction_events,
            )),
            InteractionStatus::ContainedTransforming { session } => Ok(self
                .cancel_contained_transform(
                    input,
                    session,
                    InteractionCancelReason::ExplicitCancellation,
                    interaction_events,
                )),
        }
    }

    fn gesture_cancel_delivery_is_current(&self, delivery: GestureCancelDelivery) -> bool {
        match delivery {
            GestureCancelDelivery::Surface(surface) => {
                self.active_interaction_source_surface() == Some(surface)
            }
            GestureCancelDelivery::NativeBinding(binding) => self
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

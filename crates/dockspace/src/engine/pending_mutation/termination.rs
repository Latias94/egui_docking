//! One ownership-consuming cancellation path for released pointer transactions.

use super::*;
use crate::event::PendingReductionCause;

impl DockEngine {
    pub(in crate::engine) fn cancel_pending_release_obligations_caused(
        &mut self,
        cause: ReductionCause,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Vec<InteractionOutcome> {
        self.cancel_pending_release(
            PendingReductionCause::bound(cause),
            None,
            reason,
            interaction_events,
        )
        .into_iter()
        .collect()
    }

    pub(in crate::engine) fn cancel_pending_release_obligations_input(
        &mut self,
        input: InputSequence,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) {
        let _ = self.cancel_pending_release(
            PendingReductionCause::input(input),
            None,
            reason,
            interaction_events,
        );
    }

    pub(in crate::engine) fn cancel_pending_release_for_provider(
        &mut self,
        cause: ReductionCause,
        provider: PointerInputLease,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) {
        let _ = self.cancel_pending_release(
            PendingReductionCause::bound(cause),
            Some(provider),
            reason,
            interaction_events,
        );
    }

    fn cancel_pending_release(
        &mut self,
        cause: PendingReductionCause,
        provider: Option<PointerInputLease>,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Option<InteractionOutcome> {
        let pending = self.pending_mutation.take_if(|pending| {
            pending
                .gesture()
                .is_some_and(|(owner, _)| provider.is_none_or(|provider| owner == provider))
        })?;
        let (_, status) = pending
            .gesture()
            .expect("only a matching released gesture is consumed");
        let kind = InteractionEventKind::Cancelled { status, reason };
        let event = match cause {
            PendingReductionCause::Input(input) => InteractionEvent::new(input, self.version, kind),
            PendingReductionCause::Bound(cause) => {
                InteractionEvent::new_caused(cause, self.version, kind)
            }
        };
        interaction_events.push(event);
        Some(InteractionOutcome::Cancelled { status, reason })
    }

    pub(in crate::engine) fn cancel_invalid_pending_release_obligations_caused(
        &mut self,
        cause: ReductionCause,
        interaction_events: &mut Vec<InteractionEvent>,
    ) {
        let reason = self
            .pending_mutation
            .as_ref()
            .and_then(PendingMutation::drag_release)
            .and_then(|pending| {
                if self.version != pending.source_version {
                    Some(InteractionCancelReason::WorkspaceChanged)
                } else if self.policy.revision() != pending.policy_revision {
                    Some(InteractionCancelReason::PolicyChanged)
                } else {
                    None
                }
            });
        if let Some(reason) = reason {
            let _ = self.cancel_pending_release(
                PendingReductionCause::bound(cause),
                None,
                reason,
                interaction_events,
            );
        }
    }
}

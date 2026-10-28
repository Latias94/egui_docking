//! Managed live-input cancellation without discarding accepted obligations.

use super::*;
use crate::pointer_journal::ScrollCancelReason;

impl DockEngine {
    pub(crate) fn validate_native_pointer_roster_resync(
        &self,
        provider: BackendIngressLease,
        checkpoint: &crate::pointer_journal::PointerAuthorityCheckpoint,
    ) -> Result<(), EngineError> {
        if self.backend_ingress.active() != Some(provider)
            || self.pointer_provider() != Some(provider.pointer_provider())
            || self.platform_provider() != Some(provider.platform_provider())
        {
            return Err(EngineError::BackendIngressProviderMismatch {
                ingress: provider,
                platform: self.platform_provider(),
                pointer: self.pointer_provider(),
            });
        }
        self.pointer_journal
            .validate_reset_authority_resync(
                provider.pointer_provider(),
                checkpoint.observed_through(),
            )
            .map_err(|source| EngineError::PointerJournal { source })
    }

    pub(super) fn cancel_native_input_for_overload(
        &mut self,
        input: InputSequence,
        cause: ReductionCause,
        provider: BackendIngressLease,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        self.cancel_managed_live_input(
            input,
            cause,
            provider,
            (
                InteractionCancelReason::InputOverload,
                ScrollTerminationReason::InputOverload,
            ),
            interaction_events,
        )?;
        Ok(InputOutcome::NativeInputCancelledForOverload)
    }

    pub(super) fn observe_native_input_device_removal(
        &mut self,
        input: InputSequence,
        cause: ReductionCause,
        provider: BackendIngressLease,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        self.cancel_managed_live_input(
            input,
            cause,
            provider,
            (
                InteractionCancelReason::PointerStreamCancelled,
                ScrollTerminationReason::Cancelled(ScrollCancelReason::DeviceRemoved),
            ),
            interaction_events,
        )?;
        Ok(InputOutcome::NativeInputDeviceRemoval)
    }

    fn cancel_managed_live_input(
        &mut self,
        input: InputSequence,
        cause: ReductionCause,
        provider: BackendIngressLease,
        reasons: (InteractionCancelReason, ScrollTerminationReason),
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<(), EngineError> {
        if self.backend_ingress.active() != Some(provider)
            || self.pointer_provider() != Some(provider.pointer_provider())
            || self.platform_provider() != Some(provider.platform_provider())
        {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "managed input cancellation requires the exact current managed provider",
            });
        }

        self.pointer_journal
            .cancel_live_streams_for_reset(provider.pointer_provider())
            .map_err(|source| EngineError::PointerJournal { source })?;
        let (interaction_reason, scroll_reason) = reasons;
        if let Some(owner) = self.interaction.active_owner() {
            let _ =
                self.cancel_gesture_owner(cause, owner, interaction_reason, interaction_events)?;
        }
        self.viewport
            .end_all_drag_routing()
            .map_err(|source| EngineError::Viewport { input, source })?;
        self.terminate_all_scroll_sessions_input(input, scroll_reason, interaction_events);
        // Pending releases own their previews and outputs independently of live input.
        // Normal input-close projection refresh restores only an unconsumed gesture.
        Ok(())
    }
}

//! Callback reduction consumes one typed head, then acknowledges its exact receipt.

use super::*;
use crate::mailbox::NativeCallbackHead;

impl NativeCoordinator {
    pub(super) fn reduce_callback(
        &mut self,
        callback: NativeCallbackHead,
    ) -> Result<(), NativeRuntimeError> {
        match callback {
            NativeCallbackHead::ViewportCloseCancelled(record) => {
                self.reduce_viewport_close_cancelled(record)
            }
            NativeCallbackHead::ViewportRoster(record) => self.reduce_viewport_roster(record),
            NativeCallbackHead::ViewportCreateFailed(record) => {
                self.reduce_viewport_create_failure(record)
            }
            NativeCallbackHead::ViewportVisibility(record) => {
                self.reduce_viewport_visibility(record)
            }
            NativeCallbackHead::ViewportCreated(record) => self.reduce_viewport_created(record),
            NativeCallbackHead::ViewportPointerPassthrough(record) => {
                self.reduce_viewport_pointer_passthrough(record)
            }
            NativeCallbackHead::PointerObservation(record, captured_at) => {
                self.reduce_pointer_observation(record, captured_at)
            }
            NativeCallbackHead::PointerCapture(record, _captured_at) => {
                self.reduce_pointer_capture(record)
            }
            NativeCallbackHead::GlobalFocus(record) => self.reduce_global_focus(record),
            NativeCallbackHead::ViewportFocus(record) => self.reduce_viewport_focus(record),
            NativeCallbackHead::DeviceRemoval(record) => {
                self.session.report_native_input_device_removal()?;
                self.pointer_translator.forget_removed_devices();
                self.bridge
                    .release_pointer_translation_capacity(&self.pointer_translator);
                self.acknowledge_device_removal(record)
            }
            NativeCallbackHead::WindowEvent | NativeCallbackHead::Output => {
                unreachable!("raw and output boundaries stay in their existing lanes")
            }
        }
    }

    fn reduce_viewport_create_failure(
        &mut self,
        failure: NativeViewportCreateFailureRecord,
    ) -> Result<(), NativeRuntimeError> {
        let Some(result) = self.effects.take_failure_result(failure) else {
            let failure_is_stale = self.viewport_binding(failure.viewport())
                != Some(failure.binding())
                || !self.session.is_current_native_binding(failure.binding());
            if failure_is_stale {
                self.acknowledge_viewport_create_failure(failure, true)?;
                return Ok(());
            }
            return Err(NativeHostProtocolError::ViewportCreateFailureWithoutEffect.into());
        };
        let (result_recorded, callback_error) =
            match self.session.report_native_effect_result(result) {
                Ok(()) => (true, None),
                Err(error) => {
                    let (kind, result) = error.into_parts();
                    (
                        kind == NativeHostErrorKind::StaleBinding,
                        self.retain_rejected_effect_result(kind, result).err(),
                    )
                }
            };
        self.acknowledge_viewport_create_failure(failure, result_recorded)?;
        if let Some(error) = callback_error {
            self.callback_errors.push_back(error);
        }
        Ok(())
    }

    fn reduce_viewport_pointer_passthrough(
        &mut self,
        record: crate::input_control::NativePointerPassthroughRecord,
    ) -> Result<(), NativeRuntimeError> {
        let disposition = self.input_control.classify(record);
        let mut callback_error = None;
        match disposition {
            NativeInputResultDisposition::Unrelated => {
                if record.status() == NativeViewportPointerPassthroughStatus::Applied
                    && let Some(binding) = record.binding()
                {
                    let route = self.current_window_route_for_binding(binding);
                    if route == Some((record.viewport(), record.window()))
                        && self.input_control.observe_unowned_applied(record)?
                    {
                        self.bridge.invalidate_viewport_roster();
                    }
                }
            }
            NativeInputResultDisposition::Current { binding, status }
                if self.current_window_route_for_binding(binding)
                    == Some((record.viewport(), record.window())) =>
            {
                match status {
                    NativeViewportPointerPassthroughStatus::Applied => {
                        if !self.input_control.accept_applied(record)? {
                            return Err(
                                NativeHostProtocolError::NativeInputCorrelationChanged.into()
                            );
                        }
                        self.bridge.invalidate_viewport_roster();
                    }
                    NativeViewportPointerPassthroughStatus::Unsupported => {
                        let result = self
                            .input_control
                            .take_unsupported_result(record)
                            .ok_or(NativeHostProtocolError::NativeInputCorrelationChanged)?;
                        callback_error = self.report_callback_terminal_result(result);
                    }
                    NativeViewportPointerPassthroughStatus::Failed
                    | NativeViewportPointerPassthroughStatus::Retired => {
                        let result = self
                            .input_control
                            .take_dispatch_failure_result(
                                record,
                                NativeDispatchFailure::AdapterRejected,
                            )
                            .ok_or(NativeHostProtocolError::NativeInputCorrelationChanged)?;
                        callback_error = self.report_callback_terminal_result(result);
                    }
                }
            }
            NativeInputResultDisposition::Current { binding, .. }
            | NativeInputResultDisposition::Stale { binding } => {
                let result = self
                    .input_control
                    .take_indeterminate_result(record)
                    .ok_or(NativeHostProtocolError::NativeInputCorrelationChanged)?;
                callback_error = self.report_callback_terminal_result(result);
                debug_assert!(
                    matches!(disposition, NativeInputResultDisposition::Stale { .. })
                        || self.current_window_route_for_binding(binding)
                            != Some((record.viewport(), record.window())),
                    "a current input callback must take the exact route branch"
                );
            }
        }
        if !self.bridge.acknowledge_viewport_pointer_passthrough(record) {
            return Err(
                NativeHostProtocolError::ViewportPointerPassthroughAcknowledgementMismatch.into(),
            );
        }
        if let Some(error) = callback_error {
            self.callback_errors.push_back(error);
        }
        Ok(())
    }

    fn reduce_global_focus(
        &mut self,
        record: crate::focus_control::NativeGlobalFocusRecord,
    ) -> Result<(), NativeRuntimeError> {
        debug_assert_ne!(record.event(), 0, "native event ordinals are non-zero");
        // Lifecycle commits may revoke the proof captured by this callback.
        let focus = match record.focus() {
            NativeGlobalFocus::Dock(binding)
                if !self.session.native_binding_can_report_focus(binding) =>
            {
                NativeGlobalFocus::Unknown
            }
            focus => focus,
        };
        self.session.report_native_global_focus(focus)?;
        if !self.bridge.acknowledge_global_focus(record) {
            return Err(NativeHostProtocolError::GlobalFocusAcknowledgementMismatch.into());
        }
        Ok(())
    }

    fn reduce_viewport_focus(
        &mut self,
        record: crate::focus_control::NativeViewportFocusRecord,
    ) -> Result<(), NativeRuntimeError> {
        let disposition = self.focus_control.classify(record);
        let mut callback_error = None;
        match disposition {
            NativeFocusResultDisposition::Unrelated => {}
            NativeFocusResultDisposition::Current { binding, status }
                if self.session.is_current_native_binding(binding)
                    && self.bridge.with_viewports(|viewports| {
                        viewports.focus_binding_for_event(record.window(), Some(record.viewport()))
                    }) == Some(binding.window()) =>
            {
                match status {
                    NativeViewportFocusStatus::AlreadyFocused => {
                        self.session
                            .report_native_global_focus(NativeGlobalFocus::Dock(
                                binding.window(),
                            ))?;
                        let request = self
                            .focus_control
                            .take_callback(record)
                            .ok_or(NativeHostProtocolError::NativeFocusCorrelationChanged)?;
                        if request.accepted().is_some() {
                            return Err(
                                NativeHostProtocolError::UnexpectedViewportEffectAcknowledgement
                                    .into(),
                            );
                        }
                    }
                    NativeViewportFocusStatus::Requested => {
                        let request = self
                            .focus_control
                            .take_callback(record)
                            .ok_or(NativeHostProtocolError::NativeFocusCorrelationChanged)?;
                        // The exact callback completes dispatch. Core owns the
                        // pending semantic focus and any superseded hazards;
                        // only a real focus observation can settle that state.
                        if request.accepted().is_some() {
                            return Err(
                                NativeHostProtocolError::UnexpectedViewportEffectAcknowledgement
                                    .into(),
                            );
                        }
                    }
                    NativeViewportFocusStatus::Retired => {
                        let request = self
                            .focus_control
                            .take_callback(record)
                            .ok_or(NativeHostProtocolError::NativeFocusCorrelationChanged)?;
                        callback_error = self.report_callback_terminal_result(
                            request.not_applied(NativeDispatchFailure::AdapterRejected),
                        );
                    }
                }
            }
            NativeFocusResultDisposition::Current { binding, .. }
            | NativeFocusResultDisposition::Stale { binding } => {
                let request = self
                    .focus_control
                    .take_callback(record)
                    .ok_or(NativeHostProtocolError::NativeFocusCorrelationChanged)?;
                let result = request.indeterminate(NativeIndeterminateReason::AcknowledgementLost);
                callback_error = self.report_callback_terminal_result(result);
                debug_assert!(
                    !self.session.is_current_native_binding(binding)
                        || self.bridge.with_viewports(|viewports| {
                            viewports
                                .focus_binding_for_event(record.window(), Some(record.viewport()))
                        }) != Some(binding.window()),
                    "a current focus callback must take the exact route branch"
                );
            }
        }
        if !self.bridge.acknowledge_viewport_focus(record) {
            return Err(NativeHostProtocolError::ViewportFocusAcknowledgementMismatch.into());
        }
        if let Some(error) = callback_error {
            self.callback_errors.push_back(error);
        }
        Ok(())
    }

    fn reduce_viewport_close_cancelled(
        &mut self,
        record: crate::close_control::NativeViewportCloseCancellationRecord,
    ) -> Result<(), NativeRuntimeError> {
        let Some((binding, acknowledgement)) = self
            .bridge
            .with_close_control(|control| control.cancellation_observation(record))
        else {
            if !self.bridge.acknowledge_viewport_close_cancelled(record) {
                return Err(NativeHostProtocolError::NativeCloseCorrelationChanged.into());
            }
            return Ok(());
        };
        self.publish_close(binding, NativeCloseState::Clear, Some(acknowledgement))?;
        if !self.bridge.acknowledge_viewport_close_cancelled(record)
            || !self
                .bridge
                .with_close_control(|control| control.mark_clear_recorded(binding))
        {
            return Err(NativeHostProtocolError::NativeCloseCorrelationChanged.into());
        }
        Ok(())
    }

    fn reduce_viewport_visibility(
        &mut self,
        record: NativeViewportVisibilityRecord,
    ) -> Result<(), NativeRuntimeError> {
        self.bridge.reduce_preparation_visibility(record);
        self.bridge.invalidate_viewport_roster();
        // Dispatch completion never substitutes for a later real visible snapshot.
        self.acknowledge_viewport_visibility(record)?;
        Ok(())
    }

    fn reduce_viewport_created(
        &mut self,
        created: crate::mailbox::NativeViewportCreatedRecord,
    ) -> Result<(), NativeRuntimeError> {
        if !self
            .bridge
            .can_retain_create_ack(created.token().viewport_id(), created.binding())
        {
            return Err(NativeHostProtocolError::CreateAcknowledgementAlreadyPending.into());
        }
        let attach = self.bridge.with_viewports_mut(|viewports| {
            viewports.attach_output(
                created.token().viewport_id(),
                created.binding(),
                created.token().window_id(),
                created.token().create_attempt(),
            )
        });
        if attach.is_err() {
            return Err(NativeHostProtocolError::OutputRouteAttachmentFailed.into());
        }
        let request = self
            .effects
            .take_for_output(created.token().viewport_id(), created.binding())
            .ok_or(NativeHostProtocolError::UnexpectedViewportEffectAcknowledgement)?;
        let Some(NativeEffectAcknowledgement::Create(acknowledgement)) = request.accepted() else {
            return Err(NativeHostProtocolError::UnexpectedViewportEffectAcknowledgement.into());
        };
        self.bridge
            .retain_create_ack(
                created.token().viewport_id(),
                created.binding(),
                acknowledgement,
            )
            .expect("the exact create callback retained its prepaid creation observation");
        self.bridge.begin_window_preparation(
            created.token().viewport_id(),
            created.binding(),
            created.token().window_id(),
        );
        self.bridge.invalidate_viewport_roster();
        if !self.bridge.acknowledge_viewport_created(created) {
            return Err(NativeHostProtocolError::OutputRouteAttachmentFailed.into());
        }
        assert!(
            self.bridge
                .clear_create(created.token().viewport_id(), created.binding())
        );
        Ok(())
    }

    fn reduce_viewport_roster(
        &mut self,
        roster: NativeViewportRosterEnvelope,
    ) -> Result<(), NativeRuntimeError> {
        self.record_viewport_roster(
            roster.roster(),
            roster.clone(),
            roster.roster().work_areas(),
        )?;
        if !self.bridge.acknowledge_viewport_roster(&roster) {
            return Err(NativeHostProtocolError::ViewportRosterAcknowledgementMismatch.into());
        }
        Ok(())
    }
}

#[cfg(test)]
impl NativeCoordinator {
    pub(crate) fn reduce_next_viewport_roster(&mut self) -> Result<bool, NativeRuntimeError> {
        let Some(NativeCallbackHead::ViewportRoster(record)) = self.bridge.next_callback() else {
            return Ok(false);
        };
        self.reduce_viewport_roster(record)?;
        Ok(true)
    }
    pub(crate) fn reduce_next_global_focus(&mut self) -> Result<bool, NativeRuntimeError> {
        let Some(NativeCallbackHead::GlobalFocus(record)) = self.bridge.next_callback() else {
            return Ok(false);
        };
        self.reduce_global_focus(record)?;
        Ok(true)
    }
    pub(crate) fn reduce_next_viewport_focus(&mut self) -> Result<bool, NativeRuntimeError> {
        let Some(NativeCallbackHead::ViewportFocus(record)) = self.bridge.next_callback() else {
            return Ok(false);
        };
        self.reduce_viewport_focus(record)?;
        Ok(true)
    }
    pub(crate) fn reduce_next_viewport_visibility(&mut self) -> Result<bool, NativeRuntimeError> {
        let Some(NativeCallbackHead::ViewportVisibility(record)) = self.bridge.next_callback()
        else {
            return Ok(false);
        };
        self.reduce_viewport_visibility(record)?;
        Ok(true)
    }
}

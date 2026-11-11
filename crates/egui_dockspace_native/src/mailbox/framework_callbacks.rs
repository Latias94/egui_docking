//! Framework callback implementation for the shared native host bridge.

use super::*;

impl NativeHostHandler for NativeHostBridge {
    fn on_service(&self, context: &eframe::egui::Context) -> NativeHostWake {
        self.output_pass_finalizer
            .get()
            .map_or(NativeHostWake::Wait, |finalizer| {
                finalizer.on_service(context)
            })
    }

    fn before_output_input(
        &self,
        context: &eframe::egui::Context,
        token: NativeOutputToken,
        has_input: bool,
    ) -> eframe::NativeOutputInputAdmission {
        self.output_pass_finalizer
            .get()
            .map_or(eframe::NativeOutputInputAdmission::Proceed, |finalizer| {
                finalizer.before_output_input(context, token, has_input)
            })
    }

    fn on_viewport_pointer_observation(
        &self,
        result: eframe::NativeViewportPointerObservationResult,
    ) -> NativeHostWake {
        let mut records = self.lock();
        if !records.mode.accepts_terminal_callbacks() {
            return NativeHostWake::Wait;
        }
        let NativeHostState {
            window_resources,
            viewports,
            journal,
            ..
        } = &mut *records;
        if window_resources.accept_observation(result, viewports, |record| journal.reserve(record))
            == CommandCallbackAdmission::First
        {
            NativeHostWake::RepaintRootAfterCurrent
        } else {
            NativeHostWake::Wait
        }
    }

    fn on_viewport_pointer_capture(
        &self,
        result: eframe::NativeViewportPointerCaptureResult,
    ) -> NativeHostWake {
        let mut records = self.lock();
        if !records.mode.accepts_terminal_callbacks() {
            return NativeHostWake::Wait;
        }
        let NativeHostState {
            window_resources,
            journal,
            ..
        } = &mut *records;
        if window_resources.accept_capture(result, |record| journal.reserve(record))
            == CommandCallbackAdmission::First
        {
            NativeHostWake::RepaintRootAfterCurrent
        } else {
            NativeHostWake::Wait
        }
    }
    fn on_viewport_property_command_retired(
        &self,
        retirement: eframe::NativeViewportPropertyCommandRetirement,
    ) -> NativeHostWake {
        if self.lock().record_property_retirement(retirement) {
            NativeHostWake::RepaintRootAfterCurrent
        } else {
            NativeHostWake::Wait
        }
    }

    fn install_output_pass_finalizer(&self, finalizer: Arc<dyn NativeOutputPassFinalizer>) -> bool {
        let installed = {
            let records = self.lock();
            if records.attachment != HostAttachmentState::Available {
                return false;
            }
            self.output_pass_finalizer.set(finalizer)
        };
        installed.is_ok()
    }

    fn try_attach(&self, _attachment: NativeHostAttachment) -> bool {
        self.lock().attachment.try_attach()
    }

    fn detach(&self, _attachment: NativeHostAttachment) {
        let mut records = self.lock();
        records.attachment.detach();
        let NativeHostState {
            window_resources,
            journal,
            ..
        } = &mut *records;
        window_resources.detach_observation(|record| journal.reserve(record));
        window_resources.detach_capture(|record| journal.reserve(record));
    }

    fn begin_deferred_viewport_create(
        &self,
        attempt: NativeViewportCreateAttempt,
    ) -> NativeViewportCreateAdmission {
        self.admit_viewport_create_attempt(attempt)
    }

    fn render_hidden_deferred_viewport(&self, viewport_id: ViewportId) -> bool {
        self.hidden_render_enabled(viewport_id)
    }

    fn on_window_event(&self, event: NativeWindowEvent<'_>) -> NativeWindowEventDisposition {
        // Freeze the route and publish the event atomically with respect to
        // route retirement. Otherwise retirement could report quiescence
        // after route lookup but before this event became visible.
        let mut records = self.lock();
        if !records.accepts_window_event(event.event()) {
            return NativeWindowEventDisposition::Continue;
        }
        if NativeWindowEventClass::classify(event.event()) == NativeWindowEventClass::Pointer {
            // The lock keeps this one complete admission valid through capture.
            // No pointer payload is allocated before its growth is prepaid.
            if records
                .admit_pointer_callback(NativeWindowEventRecord::pointer_callback_capacity_bytes())
            {
                let record = NativeWindowEventRecord::from_eframe(event, &mut records.viewports);
                debug_assert!(
                    record.owned_descendant_capacity_bytes()
                        <= NativeWindowEventRecord::pointer_callback_capacity_bytes()
                );
                let recorded = records.record_window_event(record);
                debug_assert!(recorded, "the admitted pointer retains its raw slot");
            }
            return NativeWindowEventDisposition::Continue;
        }
        let record = NativeWindowEventRecord::from_eframe(event, &mut records.viewports);
        let disposition = match (record.binding(), record.close_request()) {
            (Some(_), Some(request)) => NativeWindowEventDisposition::HoldClose(request),
            _ => NativeWindowEventDisposition::Continue,
        };
        if records.capture_window_event(record) {
            disposition
        } else {
            NativeWindowEventDisposition::Continue
        }
    }

    fn on_device_removed(&self, removal: NativeDeviceRemoval) {
        let mut records = self.lock();
        if !records.mode.accepts_device_removal() {
            return;
        }
        records.record_device_removal(NativeDeviceRemovalRecord::from_eframe(removal));
    }

    fn on_global_focus(&self, observation: NativeGlobalFocusObservation) -> NativeHostWake {
        let mut records = self.lock();
        let record = NativeGlobalFocusRecord::capture(observation, &records.viewports);
        if records.record_global_focus(record) {
            NativeHostWake::RepaintRoot
        } else if records.global_focus_refresh_pending && records.mode.accepts_new_work() {
            NativeHostWake::RepaintRootAfterCurrent
        } else {
            NativeHostWake::Wait
        }
    }

    fn on_viewport_focus(&self, result: NativeViewportFocusResult) -> NativeHostWake {
        let mut records = self.lock();
        let record = NativeViewportFocusRecord::capture(result, &records.viewports);
        if records.record_viewport_focus(record) {
            NativeHostWake::RepaintRoot
        } else {
            NativeHostWake::Wait
        }
    }

    fn on_viewport_pointer_passthrough(
        &self,
        result: NativeViewportPointerPassthroughResult,
    ) -> NativeHostWake {
        let mut records = self.lock();
        let record = NativePointerPassthroughRecord::capture(result, &records.viewports);
        if records.record_viewport_pointer_passthrough(record) {
            NativeHostWake::RepaintRoot
        } else {
            NativeHostWake::Wait
        }
    }

    fn on_viewport_close_cancelled(&self, request: NativeViewportCloseRequest) -> NativeHostWake {
        let mut records = self.lock();
        if records.record_viewport_close_cancelled(
            NativeViewportCloseCancellationRecord::from_eframe(request),
        ) {
            NativeHostWake::RepaintRoot
        } else {
            NativeHostWake::Wait
        }
    }

    fn on_output_begin(
        &self,
        token: NativeOutputToken,
        window: NativeWindowSnapshot,
        root_roster: Option<NativeViewportRoster<'_>>,
    ) -> NativeOutputAdmission {
        if self.reserve_output(token, window, root_roster) {
            NativeOutputAdmission::Proceed
        } else {
            NativeOutputAdmission::Defer
        }
    }

    fn on_output_pass_finalized(
        &self,
        finalization: NativeOutputPassFinalization<'_>,
    ) -> NativeHostWake {
        self.output_pass_finalizer
            .get()
            .map_or(NativeHostWake::Wait, |finalizer| {
                finalizer.finalize(finalization)
            })
    }

    fn on_output(&self, result: NativeOutputResult) -> NativeHostWake {
        let disposition = self.lock().record_output(result);
        match disposition {
            OutputRecordDisposition::Recorded => NativeHostWake::RepaintRoot,
            OutputRecordDisposition::RecordedAfterCurrent
            | OutputRecordDisposition::IgnoredAndWakeAfterCurrent
            | OutputRecordDisposition::ProtocolViolation => NativeHostWake::RepaintRootAfterCurrent,
            OutputRecordDisposition::IgnoredAndWake => NativeHostWake::RepaintRoot,
            OutputRecordDisposition::RecordedService => NativeHostWake::ServiceAfterCurrent,
            OutputRecordDisposition::Ignored | OutputRecordDisposition::Inactive => {
                NativeHostWake::Wait
            }
        }
    }

    fn on_viewport_create_failed(&self, failure: NativeViewportCreateFailure) -> NativeHostWake {
        self.record_eframe_viewport_create_failure(failure)
    }

    fn on_viewport_visibility(&self, result: NativeViewportVisibilityResult) -> NativeHostWake {
        let mut records = self.lock();
        let Some(binding) = records
            .window_resources
            .visibility_callback_binding(
                result.viewport_id(),
                result.token(),
                result.window_id(),
                result.visible(),
            )
            .or_else(|| {
                records
                    .viewports
                    .binding_for_event(result.window_id(), Some(result.viewport_id()))
            })
        else {
            return NativeHostWake::Wait;
        };
        if records.record_viewport_visibility(NativeViewportVisibilityRecord::from_eframe(
            result, binding,
        )) {
            NativeHostWake::RepaintRoot
        } else {
            NativeHostWake::Wait
        }
    }
}

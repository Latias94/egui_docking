//! Lifecycle and property callback receipts with their reserved ownership.

use super::*;

impl NativeHostState {
    pub(super) fn record_viewport_create_failure(
        &mut self,
        failure: NativeViewportCreateFailureRecord,
    ) -> bool {
        if !self.mode.accepts_terminal_callbacks() {
            return false;
        }
        if self.viewports.receive_create_terminal(
            failure.viewport(),
            failure.binding(),
            failure.attempt(),
            CreateTerminal::Failed(failure.kind()),
            || {
                self.journal
                    .reserve(HostRecord::ViewportCreateFailed(failure))
            },
        ) != CreateAdmission::First
        {
            return false;
        }
        true
    }

    pub(super) fn record_viewport_visibility(
        &mut self,
        record: NativeViewportVisibilityRecord,
    ) -> bool {
        if !self.mode.accepts_terminal_callbacks() {
            return false;
        }
        let admission = self
            .window_resources
            .accept_visibility_callback(record, |binding| {
                self.journal.reserve(HostRecord::ViewportVisibility(
                    record.qualified_for_binding(binding),
                ))
            });
        if admission == CommandCallbackAdmission::Duplicate {
            return false;
        }
        if admission == CommandCallbackAdmission::Unreserved && !self.admit_ordinary() {
            return false;
        }
        if matches!(self.mode, HostIngressMode::Quarantined)
            || !matches!(self.admission, NativeAdmission::Running)
        {
            self.terminal_roster.require();
        }
        if admission == CommandCallbackAdmission::Unreserved {
            return self
                .journal
                .push_back(HostRecord::ViewportVisibility(record));
        }
        true
    }

    pub(super) fn record_global_focus(&mut self, record: NativeGlobalFocusRecord) -> bool {
        let admitted =
            self.admit_ordinary() && self.journal.push_back(HostRecord::GlobalFocus(record));
        if admitted {
            self.global_focus_refresh_pending = false;
        } else if self.mode.accepts_new_work() {
            self.global_focus_refresh_pending = true;
        }
        admitted
    }

    pub(super) fn record_viewport_focus(&mut self, record: NativeViewportFocusRecord) -> bool {
        if !self.mode.accepts_terminal_callbacks() {
            return false;
        }
        let admission = self
            .window_resources
            .accept_focus_callback(record, |binding| {
                self.journal.reserve(HostRecord::ViewportFocus(
                    record.qualified_for_binding(binding),
                ))
            });
        if admission == CommandCallbackAdmission::Duplicate {
            return false;
        }
        if admission == CommandCallbackAdmission::Unreserved {
            if !self.admit_ordinary() {
                return false;
            }
            return self.journal.push_back(HostRecord::ViewportFocus(record));
        }
        true
    }

    pub(super) fn record_viewport_pointer_passthrough(
        &mut self,
        record: NativePointerPassthroughRecord,
    ) -> bool {
        if !self.mode.accepts_terminal_callbacks() {
            return false;
        }
        let admission = self
            .window_resources
            .accept_pointer_callback(record, |binding| {
                self.journal.reserve(HostRecord::ViewportPointerPassthrough(
                    record.qualified_for_binding(binding),
                ))
            });
        if admission == CommandCallbackAdmission::Duplicate {
            return false;
        }
        if admission == CommandCallbackAdmission::Unreserved && !self.admit_ordinary() {
            return false;
        }
        if matches!(self.mode, HostIngressMode::Quarantined)
            || !matches!(self.admission, NativeAdmission::Running)
        {
            self.terminal_roster.require();
        }
        if admission == CommandCallbackAdmission::Unreserved {
            return self
                .journal
                .push_back(HostRecord::ViewportPointerPassthrough(record));
        }
        true
    }

    pub(super) fn record_property_retirement(
        &mut self,
        retirement: eframe::NativeViewportPropertyCommandRetirement,
    ) -> bool {
        if !self.mode.accepts_terminal_callbacks() {
            return false;
        }
        self.window_resources
            .retire_property_command(retirement, |record| self.journal.reserve(record))
            == CommandCallbackAdmission::First
    }

    pub(super) fn record_viewport_created(&mut self, record: NativeViewportCreatedRecord) -> bool {
        if !self.mode.accepts_terminal_callbacks() {
            return false;
        }
        // This callback proves a real window before the root consumes its
        // create acknowledgement. Reserve its terminal owner immediately.
        if !self
            .retirements
            .can_retain_window(record.binding, record.token.window_id())
        {
            return false;
        }
        if self
            .viewports
            .capture_output_window(
                record.token.viewport_id(),
                record.binding,
                record.token.window_id(),
                record.token.create_attempt(),
            )
            .is_err()
        {
            return false;
        }
        if self.viewports.receive_create_terminal(
            record.token.viewport_id(),
            record.binding,
            record.token.create_attempt(),
            CreateTerminal::Created(record.token.window_id()),
            || self.journal.reserve(HostRecord::ViewportCreated(record)),
        ) != CreateAdmission::First
        {
            return false;
        }
        if matches!(self.mode, HostIngressMode::Quarantined)
            || !matches!(self.admission, NativeAdmission::Running)
        {
            self.terminal_roster.require();
        }
        true
    }

    pub(super) fn semantic_prelude_ready(&self) -> bool {
        !self.event_boundary_pending
            && deferred_semantic_prelude_ready(self.front_record().into_iter())
    }
}

impl NativeHostState {
    pub(super) fn record_viewport_roster(
        &mut self,
        roster: impl Into<Arc<NativeViewportRosterRecord>>,
    ) -> bool {
        let roster = roster.into();
        if roster.observation_count() > self.terminal_roster.window_capacity() {
            self.freeze_ordinary();
            return false;
        }
        if !self.admit_ordinary() {
            return false;
        }
        let prepaid_payload = self.terminal_roster.owns_payload(&roster);
        let mut envelope = NativeViewportRosterEnvelope::from_shared(
            self.roster_context,
            roster,
            matches!(self.mode, HostIngressMode::Active),
            None,
        );
        envelope.prepaid_payload = prepaid_payload;
        let already_known = self.committed_viewport_roster.as_ref() == Some(&envelope.key)
            || self.in_flight_viewport_roster.as_ref() == Some(&envelope.key)
            || self.journal.iter().any(
                |record| matches!(record, HostRecord::ViewportRoster(queued) if queued.key == envelope.key),
            );
        if already_known {
            return false;
        }
        self.journal.push_back(HostRecord::ViewportRoster(envelope))
    }

    pub(super) fn record_terminal_viewport_roster(
        &mut self,
        roster: impl Into<Arc<NativeViewportRosterRecord>>,
    ) -> bool {
        let roster = roster.into();
        if roster.observation_count() > self.terminal_roster.window_capacity() {
            return false;
        }
        let prepaid_payload = self.terminal_roster.owns_payload(&roster);
        self.terminal_roster.capture(|generation| {
            let mut envelope = NativeViewportRosterEnvelope::from_shared(
                self.roster_context,
                roster,
                true,
                Some(generation),
            );
            envelope.prepaid_payload = prepaid_payload;
            self.journal.reserve(HostRecord::ViewportRoster(envelope))
        })
    }

    pub(super) fn front_viewport_roster(&self) -> Option<NativeViewportRosterEnvelope> {
        let HostRecord::ViewportRoster(envelope) = self.front_dispatch_record()? else {
            return None;
        };
        Some(envelope.clone())
    }

    pub(super) fn acknowledge_viewport_roster(
        &mut self,
        expected: &NativeViewportRosterEnvelope,
    ) -> bool {
        let matches = self.front_viewport_roster().is_some_and(|front| {
            front.key == expected.key
                && front.commit_baseline == expected.commit_baseline
                && front.terminal_generation == expected.terminal_generation
        });
        if !matches {
            return false;
        }
        self.transfer_front_record();
        self.event_boundary_pending = true;
        if expected.commit_baseline {
            debug_assert!(self.in_flight_viewport_roster.is_none());
            self.in_flight_viewport_roster = Some(expected.key.clone());
        }
        true
    }

    pub(super) fn settle_viewport_roster(
        &mut self,
        expected: &NativeViewportRosterEnvelope,
        applied: bool,
    ) -> bool {
        if expected.commit_baseline
            && self.in_flight_viewport_roster.as_ref() != Some(&expected.key)
        {
            return false;
        }
        if let Some(generation) = expected.terminal_generation
            && !self.terminal_roster.can_settle(generation)
        {
            return false;
        }
        if expected.commit_baseline {
            let key = self
                .in_flight_viewport_roster
                .take()
                .expect("the exact in-flight roster was checked");
            if applied {
                self.committed_viewport_roster = Some(key);
            }
        }
        if let Some(generation) = expected.terminal_generation {
            let settled = self.terminal_roster.settle(generation, applied);
            debug_assert!(
                settled,
                "the checked terminal roster generation remains current"
            );
        }
        true
    }

    pub(super) fn viewport_roster_dirty(&self) -> bool {
        self.committed_viewport_roster.as_ref().map_or_else(
            || self.roster_context != 0,
            |committed| committed.context != self.roster_context,
        )
    }

    pub(super) fn record_viewport_close_cancelled(
        &mut self,
        record: NativeViewportCloseCancellationRecord,
    ) -> bool {
        if !self.mode.accepts_terminal_callbacks() {
            return false;
        }
        if self
            .close_control
            .receive_cancellation_callback(record, || {
                self.journal
                    .reserve(HostRecord::ViewportCloseCancelled(record))
            })
            != NativeCloseCallbackAdmission::First
        {
            return false;
        }
        true
    }
}

impl NativeHostBridge {
    pub(crate) fn acknowledge_viewport_create_failure(
        &self,
        expected: NativeViewportCreateFailureRecord,
        result_recorded: bool,
    ) -> bool {
        let mut records = self.lock();
        let matches = matches!(
            records.front_dispatch_record(),
            Some(HostRecord::ViewportCreateFailed(failure)) if *failure == expected
        );
        if matches {
            if records.retirements.has_quiescence_owner(expected.binding())
                || !records.viewports.transfer_create_terminal(
                    expected.viewport(),
                    expected.binding(),
                    expected.attempt(),
                    CreateTerminal::Failed(expected.kind()),
                )
            {
                return false;
            }
            let Some(mut receipt) = records
                .viewports
                .take_failed_create(expected.viewport(), expected.binding())
            else {
                return false;
            };
            if result_recorded {
                receipt.mark_failure_recorded();
            }
            records
                .retirements
                .begin_unmaterialized_quiescence(expected.binding(), receipt)
                .expect("preflighted failed create transfers its sole receipt");
            records.transfer_front_record();
            records.event_boundary_pending = true;
        }
        matches
    }

    pub(crate) fn acknowledge_viewport_visibility(
        &self,
        expected: NativeViewportVisibilityRecord,
    ) -> bool {
        let mut records = self.lock();
        let matches = matches!(
            records.front_dispatch_record(),
            Some(HostRecord::ViewportVisibility(record)) if *record == expected
        );
        if matches {
            records.transfer_front_record();
            records.event_boundary_pending = true;
        }
        matches
    }

    pub(crate) fn acknowledge_viewport_roster(
        &self,
        expected: &NativeViewportRosterEnvelope,
    ) -> bool {
        self.lock().acknowledge_viewport_roster(expected)
    }

    pub(crate) fn settle_viewport_roster(
        &self,
        expected: &NativeViewportRosterEnvelope,
        applied: bool,
    ) -> bool {
        self.lock().settle_viewport_roster(expected, applied)
    }

    pub(crate) fn acknowledge_viewport_created(
        &self,
        expected: NativeViewportCreatedRecord,
    ) -> bool {
        let mut records = self.lock();
        let matches = matches!(
            records.front_dispatch_record(),
            Some(HostRecord::ViewportCreated(created)) if *created == expected
        );
        if matches {
            if !records.viewports.transfer_create_terminal(
                expected.token.viewport_id(),
                expected.binding,
                expected.token.create_attempt(),
                CreateTerminal::Created(expected.token.window_id()),
            ) {
                return false;
            }
            records.transfer_front_record();
            records.event_boundary_pending = true;
        }
        matches
    }
}

impl NativeHostBridge {
    pub(crate) fn reserve_pointer_callback(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        window: winit::window::WindowId,
        token: eframe::NativeViewportPointerPassthroughCommandToken,
        enabled: bool,
    ) -> bool {
        let mut records = self.lock();
        if records
            .viewports
            .binding_for_event(window, Some(viewport))
            .is_none_or(|route| route.window() != binding.window())
        {
            return false;
        }
        (matches!(records.mode, HostIngressMode::Quarantined) || records.admit_ordinary())
            && records
                .window_resources
                .reserve_pointer_callback(viewport, binding, window, token, enabled)
    }

    pub(crate) fn reserve_focus_callback(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        window: winit::window::WindowId,
        token: eframe::NativeViewportFocusCommandToken,
    ) -> bool {
        let mut records = self.lock();
        if records
            .viewports
            .binding_for_event(window, Some(viewport))
            .is_none_or(|route| route.window() != binding.window())
        {
            return false;
        }
        (matches!(records.mode, HostIngressMode::Quarantined) || records.admit_ordinary())
            && records
                .window_resources
                .reserve_focus_callback(viewport, binding, window, token)
    }

    #[cfg(test)]
    pub(crate) fn reserve_visibility_callback(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        window: winit::window::WindowId,
        token: eframe::NativeViewportVisibilityCommandToken,
        visible: bool,
    ) -> bool {
        let mut records = self.lock();
        if records.viewports.binding_for_event(window, Some(viewport)) != Some(binding) {
            return false;
        }
        // An emitted lifecycle effect owns this prepaid terminal slot. Ordinary
        // output pressure must not turn its pending Show into a dispatch failure.
        records.mode.accepts_terminal_callbacks()
            && records
                .window_resources
                .reserve_visibility_callback(viewport, binding, window, token, visible)
    }

    pub(crate) fn reserve_close_callback(
        &self,
        binding: NativeSurfaceBinding,
        request: NativeViewportCloseRequest,
    ) -> bool {
        let mut records = self.lock();
        if records
            .viewports
            .binding_for_event(request.window_id(), Some(request.viewport_id()))
            != Some(binding)
        {
            return false;
        }
        records.mode.accepts_terminal_callbacks()
            && records
                .close_control
                .reserve_cancellation_callback(binding, request)
    }

    pub(crate) fn cancel_close_callback_reservation(
        &self,
        binding: NativeSurfaceBinding,
        request: NativeViewportCloseRequest,
    ) {
        self.lock()
            .close_control
            .cancel_cancellation_callback_reservation(binding, request);
    }
}

impl NativeHostBridge {
    pub(super) fn admit_viewport_create_attempt(
        &self,
        attempt: NativeViewportCreateAttempt,
    ) -> NativeViewportCreateAdmission {
        let viewport = attempt.viewport_id();
        let mut records = self.lock();
        let Some(reservation) = records.window_resources.create(viewport) else {
            return NativeViewportCreateAdmission::Defer;
        };
        if !records.admit_ordinary()
            || records.window_resources.hidden_binding(viewport) != Some(reservation.binding)
            || !records
                .viewports
                .admit_create_attempt(viewport, reservation.binding, attempt)
        {
            return NativeViewportCreateAdmission::Defer;
        }
        NativeViewportCreateAdmission::Proceed {
            undecorated_outer_rect: Some(reservation.rect),
        }
    }

    pub(super) fn record_eframe_viewport_create_failure(
        &self,
        failure: NativeViewportCreateFailure,
    ) -> NativeHostWake {
        let attempt = failure.create_attempt();
        let viewport = attempt.viewport_id();
        let mut records = self.lock();
        let Some(binding) = records.viewports.binding_for_create_attempt(attempt) else {
            return NativeHostWake::Wait;
        };
        if records
            .window_resources
            .create(viewport)
            .is_none_or(|reservation| reservation.binding != binding)
        {
            return NativeHostWake::Wait;
        }
        if !records.record_viewport_create_failure(NativeViewportCreateFailureRecord::from_attempt(
            attempt,
            binding,
            failure.kind(),
        )) {
            return NativeHostWake::Wait;
        }
        NativeHostWake::RepaintRoot
    }

    #[cfg(test)]
    pub(super) fn record_reserved_viewport_create_failure_for_test(
        &self,
        viewport: ViewportId,
        kind: NativeViewportCreateFailureKind,
    ) -> NativeHostWake {
        let mut records = self.lock();
        let Some(reservation) = records.window_resources.create(viewport) else {
            return NativeHostWake::Wait;
        };
        if !records
            .viewports
            .admit_test_create(viewport, reservation.binding)
        {
            return NativeHostWake::Wait;
        }
        if !records.record_viewport_create_failure(NativeViewportCreateFailureRecord::new(
            viewport,
            reservation.binding,
            kind,
        )) {
            return NativeHostWake::Wait;
        }
        NativeHostWake::RepaintRoot
    }

    #[cfg(test)]
    pub(crate) fn record_viewport_create_failure_for_test(
        &self,
        viewport: eframe::egui::ViewportId,
    ) -> NativeHostWake {
        self.record_reserved_viewport_create_failure_for_test(
            viewport,
            NativeViewportCreateFailureKind::WindowUnavailable,
        )
    }

    #[cfg(test)]
    pub(crate) fn reserve_create_for_test(
        &self,
        viewport: eframe::egui::ViewportId,
        binding: NativeSurfaceBinding,
        rect: NativePhysicalRect,
    ) -> bool {
        self.reserve_create(viewport, binding, rect)
    }

    #[cfg(test)]
    pub(crate) fn push_record(&self, record: HostRecord) {
        let mut records = self.lock();
        if let HostRecord::WindowEvent(record) = record {
            records.capture_window_event(record);
            return;
        }
        match record {
            HostRecord::DeviceRemoval(record) => {
                records.record_device_removal(record);
            }
            HostRecord::ViewportFocus(record) => {
                records.record_viewport_focus(record);
            }
            HostRecord::ViewportVisibility(record) => {
                records.record_viewport_visibility(record);
            }
            HostRecord::ViewportPointerPassthrough(record) => {
                records.record_viewport_pointer_passthrough(record);
            }
            HostRecord::ViewportCloseCancelled(record) => {
                records.record_viewport_close_cancelled(record);
            }
            record => {
                records.journal.push_back(record);
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn push_viewport_roster(&self, roster: NativeViewportRosterRecord) {
        assert!(self.lock().record_viewport_roster(roster));
    }

    #[cfg(test)]
    pub(crate) fn push_terminal_viewport_roster(&self, roster: NativeViewportRosterRecord) {
        assert!(self.lock().record_terminal_viewport_roster(roster));
    }
}

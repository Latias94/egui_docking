//! Ordered input admission, prefix claims and exact callback acknowledgements.

use super::*;

impl NativeHostState {
    pub(super) fn retained_record_count(&self) -> usize {
        self.journal
            .retained_record_count()
            .saturating_add(self.close_control.parked_ingress().count())
            .saturating_add(self.owner_ingress().count())
    }

    pub(super) fn freeze_ordinary(&mut self) -> u64 {
        let was_running = matches!(self.admission, NativeAdmission::Running);
        let boundary = self.admission.freeze(self.journal.captured_through());
        if was_running {
            // Core may already own effect, presentation or atomic replay input
            // which is not represented by a ready raw callback.
            self.event_boundary_pending = true;
            self.terminal_roster.require();
        }
        boundary
    }

    pub(super) fn admit_ordinary(&mut self) -> bool {
        self.admit_ordinary_with_data(0)
    }

    pub(super) fn admit_ordinary_with_data(&mut self, additional: usize) -> bool {
        if !self.journal.can_push_raw()
            || !self.output_resources.can_reserve_ordinary()
            || !self.can_reserve_input_data(additional)
            || self.retained_record_count() >= admission::RECORD_LIMIT
            || self
                .oldest_record_age()
                .is_some_and(|age| age >= admission::AGE_LIMIT)
        {
            self.freeze_ordinary();
            return false;
        }
        self.mode.accepts_new_work() && matches!(self.admission, NativeAdmission::Running)
    }

    pub(super) fn admit_pointer_callback(&mut self, payload_bytes: usize) -> bool {
        let Some(additional) = payload_bytes
            .checked_add(NativePointerTranslator::callback_translation_capacity_bytes())
        else {
            self.freeze_ordinary();
            return false;
        };
        self.admit_ordinary_with_data(additional)
    }

    pub(super) fn dispatch(&self) -> NativeDispatch {
        self.admission.dispatch(
            self.event_boundary_pending
                || self.in_flight_viewport_roster.is_some()
                || self.close_control.has_unsettled_clear(),
            self.front_ingress().map(ReservedHostRecord::ordinal),
        )
    }

    pub(super) fn dispatch_cutoff(&self) -> Option<u64> {
        match self.admission {
            NativeAdmission::Draining { boundary } => Some(boundary),
            NativeAdmission::Running | NativeAdmission::Recovering { .. } => None,
        }
    }

    pub(super) fn front_dispatchable(&self) -> bool {
        self.front_ingress().is_some_and(|record| {
            self.dispatch_cutoff()
                .is_none_or(|cutoff| record.ordinal() <= cutoff)
        })
    }

    pub(super) fn oldest_record_age(&self) -> Option<std::time::Duration> {
        let deferred = self
            .close_control
            .parked_ingress()
            .map(|record| self.journal.age_since(record.captured_at()))
            .max();
        let callback = self
            .owner_ingress()
            .map(|record| self.journal.age_since(record.captured_at()))
            .max();
        self.journal
            .oldest_age()
            .into_iter()
            .chain(deferred)
            .chain(callback)
            .max()
    }

    pub(super) fn owner_ingress(&self) -> impl Iterator<Item = &ReservedHostRecord> {
        self.window_resources
            .callback_ingress()
            .chain(self.close_control.callback_ingress())
            .chain(self.viewports.create_ingress())
            .chain(self.viewports.window_ingress())
            .chain(self.retirements.create_ingress())
            .chain(self.retirements.window_ingress())
            .chain(self.terminal_roster.ingress())
            .chain(
                self.output_resources
                    .values()
                    .filter_map(NativeOutputResource::terminal_ingress),
            )
    }

    pub(super) fn first_owner_ingress(&self) -> Option<&ReservedHostRecord> {
        self.owner_ingress().min_by_key(|record| record.ordinal())
    }

    pub(super) fn front_ingress(&self) -> Option<&ReservedHostRecord> {
        match (self.journal.front_reserved(), self.first_owner_ingress()) {
            (Some(raw), Some(callback)) if callback.ordinal() < raw.ordinal() => Some(callback),
            (Some(raw), _) => Some(raw),
            (None, callback) => callback,
        }
    }

    pub(super) fn front_record(&self) -> Option<&HostRecord> {
        self.front_ingress().map(ReservedHostRecord::record)
    }

    pub(super) fn front_dispatch_record(&self) -> Option<&HostRecord> {
        self.front_dispatchable()
            .then(|| self.front_record())
            .flatten()
    }

    pub(super) fn raw_prefix(&self) -> impl Iterator<Item = &HostRecord> {
        let owner_cutoff = self.first_owner_ingress().map(ReservedHostRecord::ordinal);
        let admission_cutoff = self.dispatch_cutoff();
        self.journal
            .reserved_iter()
            .take_while(move |record| {
                owner_cutoff.is_none_or(|cutoff| record.ordinal() < cutoff)
                    && admission_cutoff.is_none_or(|cutoff| record.ordinal() <= cutoff)
            })
            .map(ReservedHostRecord::record)
    }

    /// Resource slots share the journal clock without allocating another queue.
    #[cfg(test)]
    pub(super) fn ordered_ingress(&self) -> impl Iterator<Item = &ReservedHostRecord> {
        let mut raw = self.journal.reserved_iter().peekable();
        let mut after = 0;
        std::iter::from_fn(move || {
            let owned = self
                .owner_ingress()
                .filter(|record| record.ordinal() > after)
                .min_by_key(|record| record.ordinal());
            let next = match (raw.peek().copied(), owned) {
                (Some(raw), Some(owned)) if owned.ordinal() < raw.ordinal() => owned,
                (Some(_), _) => raw.next().expect("the peeked raw ingress remains present"),
                (None, Some(owned)) => owned,
                (None, None) => return None,
            };
            after = next.ordinal();
            Some(next)
        })
    }

    pub(super) fn first_output_barrier(&self, cutoff: Option<u64>) -> Option<u64> {
        self.journal
            .reserved_iter()
            .chain(self.owner_ingress())
            .filter(|ingress| {
                cutoff.is_none_or(|cutoff| ingress.ordinal() <= cutoff)
                    && !matches!(ingress.record(), HostRecord::Output { .. })
            })
            .map(ReservedHostRecord::ordinal)
            .min()
    }

    pub(super) fn output_prefix_range(&self, limit: usize) -> NativeOutputPrefix {
        let limit = limit.min(OUTPUT_PREFIX_CAPACITY);
        let mut prefix = NativeOutputPrefix::default();
        if limit == 0 {
            return prefix;
        }
        let cutoff = self.dispatch_cutoff();
        let barrier = self.first_output_barrier(cutoff);
        let mut first = None;
        let mut ready = 0usize;
        let mut submitted = 0;
        let mut submitted_through = 0;
        for (resource, ingress) in self.output_resources.terminal_ingress() {
            let ordinal = ingress.ordinal();
            if cutoff.is_some_and(|cutoff| ordinal > cutoff)
                || barrier.is_some_and(|barrier| ordinal >= barrier)
            {
                continue;
            }
            if resource.submitted {
                submitted += 1;
                submitted_through = submitted_through.max(ordinal);
            } else {
                first = Some(first.map_or(ordinal, |first: u64| first.min(ordinal)));
                prefix.through = prefix.through.max(ordinal);
                ready += 1;
            }
        }
        // Submitted owners remain the exact leading capture prefix until commit.
        let Some(first) = first else {
            return prefix;
        };
        if submitted != self.submitted_output_prefix || submitted_through >= first {
            return NativeOutputPrefix::default();
        }

        if ready <= limit && prefix.through - first + 1 == ready as u64 {
            // Output-only ingress has contiguous capture ordinals. Use those
            // ordinals to place the common complete prefix without sorting.
            for (resource, ingress) in self.output_resources.terminal_ingress() {
                let ordinal = ingress.ordinal();
                if resource.submitted
                    || ordinal < first
                    || ordinal > prefix.through
                    || cutoff.is_some_and(|cutoff| ordinal > cutoff)
                    || barrier.is_some_and(|barrier| ordinal >= barrier)
                {
                    continue;
                }
                let HostRecord::Output { token } = ingress.record() else {
                    return NativeOutputPrefix::default();
                };
                let index = usize::try_from(ordinal - first)
                    .expect("a bounded output ordinal difference fits usize");
                if index >= ready || prefix.outputs[index].replace((ordinal, *token)).is_some() {
                    return NativeOutputPrefix::default();
                }
            }
            if prefix.outputs[..ready].iter().any(Option::is_none) {
                return NativeOutputPrefix::default();
            }
            prefix.remaining = ready;
            return prefix;
        }

        // A shorter budget or a non-contiguous stream selects only its first
        // bounded ordinals. This avoids building a heap index or sorting every
        // retained output when a caller needs only a small dispatch slice.
        let mut after = first.saturating_sub(1);
        let selected = limit.min(ready);
        for index in 0..selected {
            let Some((_, ingress)) = self
                .output_resources
                .terminal_ingress()
                .filter(|(resource, ingress)| {
                    !resource.submitted
                        && ingress.ordinal() > after
                        && ingress.ordinal() <= prefix.through
                        && cutoff.is_none_or(|cutoff| ingress.ordinal() <= cutoff)
                        && barrier.is_none_or(|barrier| ingress.ordinal() < barrier)
                })
                .min_by_key(|(_, ingress)| ingress.ordinal())
            else {
                return NativeOutputPrefix::default();
            };
            let HostRecord::Output { token } = ingress.record() else {
                return NativeOutputPrefix::default();
            };
            after = ingress.ordinal();
            prefix.outputs[index] = Some((after, *token));
        }
        prefix.remaining = selected;
        prefix
    }

    pub(super) fn transfer_front_record(&mut self) {
        let ordinal = self
            .front_ingress()
            .expect("the exact front record was checked before transfer")
            .ordinal();
        if let Some(record) = self
            .window_resources
            .take_callback_ingress(ordinal)
            .or_else(|| self.close_control.take_callback_ingress(ordinal))
            .or_else(|| self.viewports.take_create_ingress(ordinal))
            .or_else(|| self.viewports.take_window_ingress(ordinal))
            .or_else(|| self.retirements.take_create_ingress(ordinal))
            .or_else(|| self.retirements.take_window_ingress(ordinal))
            .or_else(|| self.terminal_roster.take_ingress(ordinal))
        {
            self.journal.transfer_reserved(record);
        } else {
            assert_eq!(
                self.journal.front_ordinal(),
                Some(ordinal),
                "only the exact raw or resource-owned head can transfer"
            );
            self.journal.transfer_front(1);
        }
    }

    pub(super) fn can_reduce_output_prefix(&self) -> bool {
        // Only terminals validated by accept_output_order reach output ingress.
        // Quarantine drains that FIFO without clearing the original violation.
        !self.output_order_invalid || matches!(self.mode, HostIngressMode::Quarantined)
    }

    pub(super) fn has_pending_coordinator_work(&self) -> bool {
        !matches!(self.admission, NativeAdmission::Running)
            || self.retained_record_count() != 0
            || self.close_control.has_pending_work()
            || self.window_resources.has_pending_work()
            || self.retirements.has_pending_work()
            || self.in_flight_viewport_roster.is_some()
            || !self.can_reduce_output_prefix()
            || self.event_boundary_pending
            || self.viewports.has_semantic_wake()
            || self.terminal_roster.has_pending()
    }
}

impl NativeHostState {
    pub(super) fn record_window_event(&mut self, record: NativeWindowEventRecord) -> bool {
        if matches!(
            (self.mode, record.event()),
            (
                HostIngressMode::Quarantined,
                winit::event::WindowEvent::Destroyed
            )
        ) {
            self.terminal_roster.require();
        }
        self.journal.push_back(HostRecord::WindowEvent(record))
    }

    pub(super) fn capture_window_event(&mut self, mut record: NativeWindowEventRecord) -> bool {
        if NativeWindowEventClass::classify(record.event()) == NativeWindowEventClass::Pointer {
            return self.admit_pointer_callback(record.owned_descendant_capacity_bytes())
                && self.record_window_event(record);
        }
        if matches!(record.event(), winit::event::WindowEvent::CloseRequested)
            && record
                .binding()
                .is_some_and(|binding| self.close_control.retains_request(&record, binding))
        {
            return true;
        }
        let reserved_close = matches!(record.event(), winit::event::WindowEvent::CloseRequested)
            && record
                .binding()
                .is_some_and(|binding| self.close_control.references_binding(binding));
        if !matches!(record.event(), winit::event::WindowEvent::Destroyed)
            && !reserved_close
            && !self.admit_ordinary()
        {
            return false;
        }
        if matches!(record.event(), winit::event::WindowEvent::CloseRequested)
            && let Some(binding) = record.binding()
        {
            match self.close_control.capture_request(&record, binding) {
                NativeCloseCaptureAdmission::Conflict => return false,
                NativeCloseCaptureAdmission::Duplicate => return true,
                NativeCloseCaptureAdmission::First => {
                    if self.close_control.needs_request_recording(&record, binding) {
                        return self.record_window_event(record);
                    }
                    let ingress = self.journal.reserve(HostRecord::WindowEvent(record));
                    self.close_control
                        .retain_successor_ingress(binding, ingress)
                        .expect("a first successor capture owns its sole ingress reservation");
                    return true;
                }
            }
        }
        if matches!(record.event(), winit::event::WindowEvent::Destroyed) {
            let window = record.window_id();
            let viewport = record.viewport_id();
            let ordinal = record.ordinal();
            let reserve = |owner_viewport, binding| {
                record.qualify_destroyed(owner_viewport, binding);
                self.journal.reserve(HostRecord::WindowEvent(record))
            };
            let received = if self.retirements.owns_window(window, viewport) {
                self.retirements
                    .receive_destroyed(window, viewport, ordinal, reserve)
            } else {
                self.viewports
                    .receive_window_destroyed(window, viewport, ordinal, reserve)
            };
            let Some((viewport, binding)) = received else {
                return false;
            };
            self.viewports.suppress_pointer_route(viewport, binding);
            if matches!(self.mode, HostIngressMode::Quarantined)
                || !matches!(self.admission, NativeAdmission::Running)
            {
                self.terminal_roster.require();
            }
            return true;
        }
        self.record_window_event(record)
    }

    pub(super) fn record_device_removal(&mut self, record: NativeDeviceRemovalRecord) -> bool {
        let ordinary = self.admit_ordinary();
        // Quarantine may still terminate an existing provider tail, within the
        // same raw capacity and fixed admission boundary as every other source.
        let terminal = matches!(self.mode, HostIngressMode::Quarantined)
            && matches!(self.admission, NativeAdmission::Running);
        (ordinary || terminal) && self.journal.push_back(HostRecord::DeviceRemoval(record))
    }

    pub(super) fn coalesce_idle_cursor_prefix(&mut self, budget: &mut NativeInputBudget) -> bool {
        let Some(HostRecord::WindowEvent(first)) = self.front_record() else {
            return false;
        };
        let Some(latest_cursor_index) = idle_cursor_prefix_latest_index(
            self.raw_prefix()
                .take(budget.remaining_records())
                .take_while(|_| !budget.is_exhausted())
                .map(|record| match record {
                    HostRecord::WindowEvent(event) if first.shares_cursor_lane(event) => {
                        IdleCursorPrefixEntry::Cursor
                    }
                    HostRecord::Output { .. } => IdleCursorPrefixEntry::Output,
                    HostRecord::WindowEvent(_)
                    | HostRecord::DeviceRemoval(_)
                    | HostRecord::GlobalFocus(_)
                    | HostRecord::ViewportFocus(_)
                    | HostRecord::ViewportPointerPassthrough(_)
                    | HostRecord::PointerObservation(_)
                    | HostRecord::PointerCapture(_)
                    | HostRecord::ViewportCloseCancelled(_)
                    | HostRecord::ViewportRoster(_)
                    | HostRecord::ViewportCreateFailed(_)
                    | HostRecord::ViewportVisibility(_)
                    | HostRecord::ViewportCreated(_) => IdleCursorPrefixEntry::Boundary,
                }),
        ) else {
            return false;
        };

        // Renderer terminals are causal boundaries. Only obsolete idle hover
        // observations before the first output or other callback may
        // disappear; output-separated cursor observations remain lossless.
        for _ in 0..latest_cursor_index {
            self.journal
                .pop_front()
                .expect("the scanned cursor prefix remains present");
        }
        budget.complete_unsequenced_records(latest_cursor_index);
        true
    }
}

impl NativeHostBridge {
    pub(crate) fn reserve_pointer_observation(
        &self,
        key: NativePointerObservationKey,
        binding: NativeSurfaceBinding,
    ) -> bool {
        let mut records = self.lock();
        if !records.mode.accepts_new_work()
            || records.attachment != HostAttachmentState::Attached
            || !matches!(records.admission, NativeAdmission::Running)
            || records.event_boundary_pending
            || records.front_record().is_some()
            || records
                .viewports
                .binding_for_event(key.window, Some(key.viewport))
                .is_none_or(|route| route.window() != binding.window())
            || !records.admit_ordinary()
        {
            return false;
        }
        records.window_resources.reserve_observation(key, binding)
    }

    pub(crate) fn pointer_observation_pending(&self, key: NativePointerObservationKey) -> bool {
        self.lock().window_resources.observation_pending(key)
    }

    pub(crate) fn acknowledge_pointer_observation(&self, key: NativePointerObservationKey) -> bool {
        let mut records = self.lock();
        if !matches!(records.front_dispatch_record(), Some(HostRecord::PointerObservation(current)) if *current == key)
        {
            return false;
        }
        records.transfer_front_record();
        records.event_boundary_pending = true;
        true
    }

    pub(crate) fn reserve_pointer_capture(
        &self,
        key: NativePointerCaptureKey,
        binding: NativeSurfaceBinding,
    ) -> bool {
        let mut records = self.lock();
        if records.attachment != HostAttachmentState::Attached
            || !records.mode.accepts_terminal_callbacks()
        {
            return false;
        }
        if key.action == eframe::NativeViewportPointerCaptureAction::Acquire
            && (!records.mode.accepts_new_work()
                || !matches!(records.admission, NativeAdmission::Running)
                || records.event_boundary_pending
                || records.front_record().is_some()
                || records
                    .viewports
                    .binding_for_event(key.window, Some(key.viewport))
                    .is_none_or(|route| route.window() != binding.window())
                || !records.admit_ordinary())
        {
            return false;
        }
        // Release spends the original prepaid resource receipt even during
        // overload or quarantine. It cannot authorize a new native acquisition.
        records.window_resources.reserve_capture(key, binding)
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn pointer_capture_pending(&self, key: NativePointerCaptureKey) -> bool {
        self.lock().window_resources.capture_pending(key)
    }

    pub(crate) fn release_capture_binding(&self, binding: NativeSurfaceBinding) {
        self.lock()
            .window_resources
            .release_capture_binding(binding);
    }

    pub(crate) fn acknowledge_pointer_capture(&self, key: NativePointerCaptureKey) -> bool {
        let mut records = self.lock();
        if !matches!(records.front_dispatch_record(), Some(HostRecord::PointerCapture(current)) if *current == key)
        {
            return false;
        }
        records.transfer_front_record();
        records.event_boundary_pending = true;
        true
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn record_pointer_capture_for_test(
        &self,
        record: NativePointerCaptureRecord,
    ) -> bool {
        let mut records = self.lock();
        let NativeHostState {
            window_resources,
            journal,
            ..
        } = &mut *records;
        window_resources.receive_capture_record(record, |record| journal.reserve(record))
            == CommandCallbackAdmission::First
    }
}

impl NativeHostBridge {
    #[cfg(test)]
    pub(crate) fn front_event(&self) -> Option<NativeWindowEventRecord> {
        self.front_captured_event().map(|(event, _)| event)
    }

    pub(crate) fn front_captured_event(
        &self,
    ) -> Option<(NativeWindowEventRecord, std::time::Duration)> {
        let records = self.lock();
        if !records.front_dispatchable() {
            return None;
        }
        let record = records.front_ingress()?;
        let HostRecord::WindowEvent(event) = record.record() else {
            return None;
        };
        Some((event.clone(), record.captured_at()))
    }

    pub(crate) fn has_pending_input(&self) -> bool {
        self.lock()
            .front_dispatch_record()
            .is_some_and(|record| !matches!(record, HostRecord::Output { .. }))
    }

    pub(crate) fn has_pending_callback_records(&self) -> bool {
        self.lock().front_record().is_some()
    }

    pub(crate) fn acknowledge_event(&self, ordinal: u64) -> bool {
        let mut records = self.lock();
        let matches = matches!(
            records.front_dispatch_record(),
            Some(HostRecord::WindowEvent(event)) if event.ordinal() == ordinal
        );
        if matches {
            if let Some(HostRecord::WindowEvent(event)) = records.front_dispatch_record()
                && matches!(event.event(), winit::event::WindowEvent::Destroyed)
            {
                let Some(binding) = event.binding() else {
                    return false;
                };
                let window = event.window_id();
                if !records
                    .retirements
                    .transfer_destroyed(binding, window, ordinal)
                {
                    return false;
                }
            }
            records.transfer_front_record();
            records.event_boundary_pending = true;
        }
        matches
    }

    /// Trades the accepted source's growth promise for the installed state.
    /// The old translator and retry candidate fit before this atomic transfer.
    pub(crate) fn acknowledge_translated_event(
        &self,
        ordinal: u64,
        translator: &mut NativePointerTranslator,
        candidate: NativePointerTranslator,
    ) -> bool {
        let mut records = self.lock();
        if !matches!(
            records.front_dispatch_record(),
            Some(HostRecord::WindowEvent(event))
                if event.ordinal() == ordinal
                    && NativeWindowEventClass::classify(event.event()) == NativeWindowEventClass::Pointer
        ) {
            return false;
        }
        *translator = candidate;
        records.pointer_translation_reserved_bytes =
            translator.reserved_translation_capacity_bytes();
        records.transfer_front_record();
        records.event_boundary_pending = true;
        true
    }

    /// Terminal transformations only shrink or replace existing entries.
    pub(crate) fn release_pointer_translation_capacity(
        &self,
        translator: &NativePointerTranslator,
    ) {
        let mut records = self.lock();
        records.pointer_translation_reserved_bytes =
            translator.reserved_translation_capacity_bytes();
    }

    pub(crate) fn acknowledge_device_removal(&self, expected: NativeDeviceRemovalRecord) -> bool {
        let mut records = self.lock();
        let matches = matches!(
            records.front_dispatch_record(),
            Some(HostRecord::DeviceRemoval(removal)) if *removal == expected
        );
        if matches {
            records.transfer_front_record();
            records.event_boundary_pending = true;
        }
        matches
    }

    pub(crate) fn acknowledge_global_focus(&self, expected: NativeGlobalFocusRecord) -> bool {
        let mut records = self.lock();
        let matches = matches!(
            records.front_dispatch_record(),
            Some(HostRecord::GlobalFocus(record)) if *record == expected
        );
        if matches {
            records.transfer_front_record();
            records.event_boundary_pending = true;
        }
        matches
    }

    pub(crate) fn acknowledge_viewport_focus(&self, expected: NativeViewportFocusRecord) -> bool {
        let mut records = self.lock();
        let matches = matches!(
            records.front_dispatch_record(),
            Some(HostRecord::ViewportFocus(record)) if *record == expected
        );
        if matches {
            records.transfer_front_record();
            records.event_boundary_pending = true;
        }
        matches
    }

    pub(crate) fn acknowledge_viewport_pointer_passthrough(
        &self,
        expected: NativePointerPassthroughRecord,
    ) -> bool {
        let mut records = self.lock();
        let matches = matches!(
            records.front_dispatch_record(),
            Some(HostRecord::ViewportPointerPassthrough(record)) if *record == expected
        );
        if matches {
            records.transfer_front_record();
            records.event_boundary_pending = true;
        }
        matches
    }

    pub(crate) fn acknowledge_viewport_close_cancelled(
        &self,
        expected: NativeViewportCloseCancellationRecord,
    ) -> bool {
        let mut records = self.lock();
        let matches = matches!(
            records.front_dispatch_record(),
            Some(HostRecord::ViewportCloseCancelled(record)) if *record == expected
        );
        if matches {
            records.transfer_front_record();
            records.event_boundary_pending = true;
        }
        matches
    }
}

impl NativeHostBridge {
    pub(crate) fn callback_boundary_pending(&self) -> bool {
        self.lock().event_boundary_pending
    }

    pub(crate) fn hold_native_boundary(&self) {
        self.lock().event_boundary_pending = true;
    }

    pub(crate) fn coalesce_idle_cursor_prefix(&self, budget: &mut NativeInputBudget) -> bool {
        self.lock().coalesce_idle_cursor_prefix(budget)
    }

    pub(crate) fn claim_cursor_prefix(&self, limit: usize) -> Option<CursorPrefixClaim<'_>> {
        let records = self.lock();
        if records.event_boundary_pending {
            return None;
        }
        let HostRecord::WindowEvent(first) = records.front_record()? else {
            return None;
        };
        let len = records.raw_prefix().take(limit).take_while(|record| {
            matches!(record, HostRecord::WindowEvent(event) if first.shares_cursor_lane(event))
        }).count();
        std::num::NonZeroUsize::new(len).map(|len| CursorPrefixClaim { records, len })
    }
}

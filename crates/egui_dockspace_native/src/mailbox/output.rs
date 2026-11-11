//! Output reservation, dispatch and terminal settlement on the shared journal.

use super::*;

impl NativeHostState {
    pub(super) fn reserve_output(
        &mut self,
        token: NativeOutputToken,
        binding: Option<NativeSurfaceBinding>,
        budget: NativeInputBudget,
    ) -> bool {
        if matches!(self.mode, HostIngressMode::Frozen)
            || self.output_resources.contains_key(&token)
        {
            return false;
        }
        let output_capacity_available = self.can_reserve_output_data();
        if !output_capacity_available {
            self.freeze_ordinary();
        }
        let wake_after_current = binding.is_some_and(|binding| {
            self.window_resources.hidden_binding(token.viewport_id()) == Some(binding)
        });
        let prepaid_output = !output_capacity_available
            || matches!(self.admission, NativeAdmission::Draining { .. })
            || self.terminal_roster.has_pending();
        let mut reservation = if prepaid_output {
            let promised_staging = binding.filter(|binding| {
                token.viewport_id() != ViewportId::ROOT
                    && self.mode.accepts_new_work()
                    && self
                        .window_resources
                        .staging(token.viewport_id())
                        .is_some_and(|request| request.binding() == *binding)
            });
            promised_staging.map_or_else(
                || OutputReservation::diagnostic(binding),
                OutputReservation::bound,
            )
        } else {
            match self.mode {
                HostIngressMode::Active => {
                    binding.map_or_else(OutputReservation::unbound, OutputReservation::bound)
                }
                HostIngressMode::Quarantined => {
                    let terminal_create = binding.filter(|binding| {
                        token.create_attempt().is_some()
                            && self
                                .window_resources
                                .create(token.viewport_id())
                                .is_some_and(|reservation| reservation.binding == *binding)
                    });
                    terminal_create.map_or_else(
                        || OutputReservation::diagnostic(binding),
                        OutputReservation::terminal_create,
                    )
                }
                HostIngressMode::Frozen => unreachable!("frozen outputs return before reservation"),
            }
        };
        if wake_after_current {
            let armed = reservation.require_wake_after_current();
            debug_assert!(armed, "a new output can retain its wake obligation");
        }
        let resource = NativeOutputResource::new(reservation, budget);
        if prepaid_output || resource.reservation.abandoned {
            return self.output_resources.insert_control(token, resource);
        }
        self.output_resources.insert(token, resource).is_ok()
    }

    pub(super) fn attach_output_binding(
        &mut self,
        token: NativeOutputToken,
        binding: NativeSurfaceBinding,
    ) -> bool {
        if self
            .prepare_window_resource_reference(token.viewport_id(), binding)
            .is_err()
        {
            return false;
        }
        let Some(reservation) = self.output_resources.get_mut(&token) else {
            return false;
        };
        if !reservation.reservation.attach(binding) {
            return false;
        }
        self.window_resources
            .commit_host_reference(token.viewport_id(), binding);
        true
    }

    pub(super) fn require_output_wake_after_current(&mut self, token: NativeOutputToken) -> bool {
        let Some(reservation) = self.output_resources.get_mut(&token) else {
            return false;
        };
        reservation.reservation.require_wake_after_current()
    }

    pub(super) fn record_output(&mut self, result: NativeOutputResult) -> OutputRecordDisposition {
        if !self.mode.accepts_terminal_callbacks() {
            return OutputRecordDisposition::Inactive;
        }
        let Some(reservation) = self.output_resources.get(&result.token()) else {
            self.output_order_invalid = true;
            return OutputRecordDisposition::ProtocolViolation;
        };
        if reservation.reservation.terminal_recorded || !self.accept_output_order(result) {
            self.output_order_invalid = true;
            return OutputRecordDisposition::ProtocolViolation;
        }
        let reservation = self
            .output_resources
            .get_mut(&result.token())
            .expect("validated output reservation remains present");
        let requires_service = reservation.terminal_requires_repaint(result.status());
        reservation.record_terminal(result);
        let metadata = reservation.reservation;
        if metadata.abandoned {
            let wake = metadata.wakes_on_terminal();
            let wake_after_current = metadata.wakes_after_current();
            self.output_resources.remove(&result.token());
            self.refresh_window_resource_references();
            return match (wake, wake_after_current) {
                (true, true) => OutputRecordDisposition::IgnoredAndWakeAfterCurrent,
                (true, false) => OutputRecordDisposition::IgnoredAndWake,
                (false, true) => OutputRecordDisposition::IgnoredAndWakeAfterCurrent,
                (false, false) => OutputRecordDisposition::Ignored,
            };
        }
        let wake_after_current = metadata.wakes_after_current();
        let ingress = self.journal.reserve(HostRecord::Output {
            token: result.token(),
        });
        assert!(
            reservation.retain_terminal_ingress(ingress),
            "the validated output retains its exact terminal reservation"
        );
        if wake_after_current {
            OutputRecordDisposition::RecordedAfterCurrent
        } else if !requires_service {
            // Ordinary successful paint settles in a host-service event.
            // It must not schedule another application paint or leave its
            // original terminal envelope parked until future user input.
            OutputRecordDisposition::RecordedService
        } else {
            OutputRecordDisposition::Recorded
        }
    }

    pub(super) fn accept_output_order(&mut self, result: NativeOutputResult) -> bool {
        match self.output_context {
            Some(context) if !context.same_context(result.token()) => return false,
            Some(_) => {}
            None => self.output_context = Some(result.token()),
        }
        let ordinal = result.ordinal().get();
        if !is_next_output_ordinal(self.last_output_ordinal, ordinal) {
            return false;
        }
        self.last_output_ordinal = ordinal;
        true
    }
}

impl NativeHostState {
    pub(super) fn abandon_output(&mut self, token: NativeOutputToken) -> bool {
        self.abandon_output_with_terminal_wake(token, true)
    }

    pub(super) fn abandon_output_with_terminal_wake(
        &mut self,
        token: NativeOutputToken,
        wake_on_terminal: bool,
    ) -> bool {
        if self
            .output_resources
            .get(&token)
            .is_some_and(|resource| resource.submitted)
        {
            return false;
        }
        if let Some(PendingNativeOutput::Staging(request)) = self
            .output_resources
            .get(&token)
            .and_then(|resource| resource.paint.as_ref())
        {
            let request = *request;
            self.window_resources
                .finish_preparation_paint(token.viewport_id(), request, false);
            self.output_resources
                .get_mut(&token)
                .expect("the output is retained")
                .paint
                .take();
            self.invalidate_viewport_roster();
        }
        if self
            .output_resources
            .get(&token)
            .is_some_and(|resource| resource.terminal_ingress().is_some())
        {
            self.output_resources.remove(&token);
            self.refresh_window_resource_references();
            return true;
        }
        let Some(resource) = self.output_resources.get_mut(&token) else {
            return false;
        };
        if wake_on_terminal {
            resource.reservation.abandon()
        } else {
            resource.reservation.abandon_silently()
        }
    }

    pub(super) fn settle_output_abandonment(
        &mut self,
        token: NativeOutputToken,
        wake_on_terminal: bool,
    ) -> OutputAbandonment {
        let Some(resource) = self.output_resources.get_mut(&token) else {
            return OutputAbandonment::MissingOrTerminal;
        };
        resource.reservation.settle_abandonment(wake_on_terminal)
    }

    pub(super) const fn accepts_no_frame_invalidation(&self) -> bool {
        !matches!(self.mode, HostIngressMode::Frozen)
    }
}

impl NativeHostState {
    pub(super) fn claim_quarantine_output(
        &mut self,
    ) -> Option<(NativeOutputToken, Option<PendingNativeOutput>)> {
        let token = self
            .output_resources
            .iter()
            .filter(|(_, resource)| {
                (!resource.reservation.terminal_recorded && !resource.reservation.abandoned)
                    || resource.terminal.is_some_and(|terminal| {
                        output_terminal_requires_adapter_abandonment(
                            terminal.status(),
                            resource.submitted,
                            resource.paint.is_some(),
                        )
                    })
            })
            .map(|(token, _)| *token)
            .min()?;
        let terminal = self
            .output_resources
            .get(&token)
            .expect("the selected exact output remains present")
            .reservation
            .terminal_recorded;
        if terminal {
            assert!(
                self.abandon_output(token),
                "an unattached negative terminal retains its original ingress owner"
            );
            return Some((token, None));
        }
        let resource = self
            .output_resources
            .get_mut(&token)
            .expect("the selected exact output remains present");
        let abandoned = resource.reservation.abandon();
        assert!(
            abandoned,
            "one quarantine claim owns the output abandonment"
        );
        Some((token, resource.paint.take()))
    }

    #[cfg(test)]
    pub(super) fn quarantine_after_fatal(&mut self) -> Vec<NativeOutputToken> {
        self.enter_quarantine();
        std::iter::from_fn(|| self.claim_quarantine_output().map(|(token, _)| token)).collect()
    }
}

impl NativeHostBridge {
    pub(crate) fn output_prefix(&self, limit: usize) -> NativeOutputPrefix {
        let records = self.lock();
        if !matches!(records.dispatch(), NativeDispatch::Replay { .. }) {
            return NativeOutputPrefix::default();
        }
        records.output_prefix_range(limit)
    }

    pub(crate) fn next_output_prefix(
        &self,
        prefix: &mut NativeOutputPrefix,
    ) -> Result<Option<NativeOutputPrefixEntry>, NativeHostProtocolError> {
        if prefix.remaining == 0 {
            return Ok(None);
        }
        let records = self.lock();
        // Submitted heads change dispatch to ContinueBoundary. Membership is
        // already frozen by capture ordinal; never reapply Replay or its skip.
        let (ordinal, token) =
            prefix.outputs[prefix.next].ok_or(NativeHostProtocolError::OutputOrderViolation)?;
        let resource = records
            .output_resources
            .get(&token)
            .ok_or(NativeHostProtocolError::OutputOrderViolation)?;
        let ingress = resource
            .terminal_ingress()
            .filter(|ingress| ingress.ordinal() == ordinal)
            .ok_or(NativeHostProtocolError::OutputOrderViolation)?;
        let HostRecord::Output {
            token: ingress_token,
        } = ingress.record()
        else {
            return Err(NativeHostProtocolError::OutputOrderViolation);
        };
        if *ingress_token != token {
            return Err(NativeHostProtocolError::OutputOrderViolation);
        }
        let entry = NativeOutputPrefixEntry {
            result: resource
                .terminal
                .ok_or(NativeHostProtocolError::OutputOrderViolation)?,
            submitted: resource.submitted,
            attached: resource.paint.is_some(),
            binding: resource.reservation.binding(),
        };
        prefix.next += 1;
        prefix.remaining -= 1;
        Ok(Some(entry))
    }

    pub(crate) fn has_pending_output_prefix(&self) -> bool {
        let records = self.lock();
        matches!(records.dispatch(), NativeDispatch::Replay { .. })
            && matches!(
                records.front_dispatch_record(),
                Some(HostRecord::Output { .. })
            )
    }

    pub(crate) fn has_unsubmitted_output_prefix(&self) -> bool {
        self.lock().output_prefix_range(1).remaining != 0
    }

    pub(crate) fn can_reduce_output_prefix(&self) -> bool {
        self.lock().can_reduce_output_prefix()
    }

    #[cfg(test)]
    pub(crate) fn output_order_is_valid(&self) -> bool {
        !self.lock().output_order_invalid
    }
}

impl NativeHostBridge {
    pub(crate) fn reserve_output(
        &self,
        token: NativeOutputToken,
        window: NativeWindowSnapshot,
        root_roster: Option<NativeViewportRoster<'_>>,
    ) -> bool {
        let budget = NativeInputBudget::new(&self.clock);
        // Route lookup and reservation share the host-state lock, so retirement
        // cannot report quiescence before this exact binding reference is visible.
        let mut records = self.lock();
        let binding = records.viewports.binding_for_output(
            token.viewport_id(),
            token.window_id(),
            token.create_attempt(),
        );
        if let Some(binding) = binding {
            if !records.window_resources.validate_preparation_output(
                token.viewport_id(),
                binding,
                window,
            ) {
                records.invalidate_viewport_roster();
            }
        }
        if let Some(binding) = binding
            && records
                .prepare_window_resource_reference(token.viewport_id(), binding)
                .is_err()
        {
            records.freeze_ordinary();
            return false;
        }
        let ordinary_roster = root_roster.is_some() && records.admit_ordinary();
        if !records.reserve_output(token, binding, budget) {
            if token.viewport_id() != ViewportId::ROOT
                && binding.is_some()
                && records.mode.accepts_terminal_callbacks()
            {
                // Deferred output runs no egui pass, so its original redraw
                // cannot be relied on to wake again. Reuse the existing child
                // continuation released by the next complete root boundary.
                records.block_semantic_viewport(token.viewport_id());
            }
            return false;
        }
        if let Some(binding) = binding {
            records
                .window_resources
                .commit_host_reference(token.viewport_id(), binding);
        }
        if matches!(records.mode, HostIngressMode::Quarantined)
            && let Some(binding) = binding
            && token.create_attempt().is_some()
            && records
                .window_resources
                .create(token.viewport_id())
                .is_some_and(|create| create.binding == binding)
        {
            records.record_viewport_created(NativeViewportCreatedRecord::new(token, binding));
        }
        if let Some(roster) = root_roster {
            match records.mode {
                HostIngressMode::Active | HostIngressMode::Quarantined
                    if records.terminal_roster.needs_capture() =>
                {
                    let NativeHostState {
                        terminal_roster,
                        viewports,
                        ..
                    } = &mut *records;
                    let payload = terminal_roster.capture_roster(roster, viewports);
                    if let Some(payload) = payload {
                        records.record_terminal_viewport_roster(payload);
                    } else {
                        // Both preallocated payloads may still be retained by a
                        // baseline or settling snapshot. Keep the obligation and
                        // retry after this output without allocating a third one.
                        records.require_output_wake_after_current(token);
                    }
                }
                HostIngressMode::Active if ordinary_roster => {
                    if !records.viewport_roster_known_borrowed(roster) {
                        let NativeHostState {
                            terminal_roster,
                            viewports,
                            ..
                        } = &mut *records;
                        let payload = terminal_roster.capture_ordinary_roster(roster, viewports);
                        if let Some(payload) = payload {
                            records.record_viewport_roster(payload);
                        } else {
                            records.defer_ordinary_roster_capture();
                            records
                                .output_resources
                                .get_mut(&token)
                                .expect("the current output was reserved before roster capture")
                                .reservation
                                .abandon_silently();
                            records.require_output_wake_after_current(token);
                        }
                    }
                }
                HostIngressMode::Active
                | HostIngressMode::Quarantined
                | HostIngressMode::Frozen => {}
            }
        }
        true
    }

    pub(crate) fn invalidate_viewport_roster(&self) {
        self.lock().invalidate_viewport_roster();
    }

    pub(crate) fn viewport_roster_dirty(&self) -> bool {
        self.lock().viewport_roster_dirty()
    }

    pub(crate) fn output_reservation(&self, token: NativeOutputToken) -> Option<OutputReservation> {
        self.lock()
            .output_resources
            .get(&token)
            .map(|resource| resource.reservation)
    }

    pub(crate) fn is_control_output(&self, token: NativeOutputToken) -> bool {
        self.lock().output_resources.is_control(token)
    }

    pub(crate) fn has_pending_output(&self, token: NativeOutputToken) -> bool {
        self.lock()
            .output_resources
            .get(&token)
            .is_some_and(|resource| resource.paint.is_some())
    }

    pub(crate) fn output_has_staging_request(&self, token: NativeOutputToken) -> bool {
        let records = self.lock();
        records
            .window_resources
            .staging(token.viewport_id())
            .is_some_and(|request| {
                records
                    .output_resources
                    .get(&token)
                    .is_some_and(|resource| {
                        resource.reservation.binding() == Some(request.binding())
                    })
            })
    }

    pub(crate) fn output_backlog(&self) -> NativeOutputBacklog {
        let records = self.lock();
        records
            .output_resources
            .output_backlog(records.window_resources.awaiting_staging_count())
    }

    pub(crate) fn pending_output_count(&self) -> usize {
        self.lock()
            .output_resources
            .values()
            .filter(|resource| resource.paint.is_some())
            .count()
    }

    pub(crate) fn has_pending_lifecycle_output(
        &self,
        session: &dockspace::runtime::DockspaceSession,
    ) -> bool {
        self.lock().output_resources.values().any(|resource| {
            resource
                .paint
                .as_ref()
                .is_some_and(|paint| paint.requires_lifecycle_settlement(session))
        })
    }

    pub(crate) fn take_pending_output(
        &self,
        token: NativeOutputToken,
    ) -> Option<PendingNativeOutput> {
        self.lock().output_resources.get_mut(&token)?.paint.take()
    }

    /// Prepays the complete semantic collection before any pane is painted.
    /// Failed admission leaves the previous backing and token owner intact.
    pub(crate) fn prepare_surface_outputs(
        &self,
        token: NativeOutputToken,
        surfaces: usize,
    ) -> bool {
        let mut records = self.lock();
        let Some(additional) = records
            .output_resources
            .get(&token)
            .and_then(|resource| resource.output_capacity_growth(surfaces))
        else {
            return false;
        };
        if !records.can_reserve_input_data(additional) {
            return false;
        }
        records
            .output_resources
            .get_mut(&token)
            .is_some_and(|resource| resource.prepare_surface_capacity(surfaces))
    }

    pub(crate) fn bind_surface_outputs(
        &self,
        token: NativeOutputToken,
        outputs: Vec<dockspace::runtime::PaintedSurfaceOutput>,
    ) -> Result<(), Vec<dockspace::runtime::PaintedSurfaceOutput>> {
        let mut records = self.lock();
        let Some(resource) = records.output_resources.get_mut(&token) else {
            return Err(outputs);
        };
        resource.bind_surfaces(outputs)
    }

    /// Attaches the real callback window without selecting a semantic placement.
    /// Each contribution was checked against its own exact endpoint by the coordinator.
    pub(crate) fn attach_output_window_binding(
        &self,
        token: NativeOutputToken,
        window: NativeWindowBinding,
    ) -> bool {
        let mut records = self.lock();
        let Some(binding) = records
            .viewports
            .binding_for_output(
                token.viewport_id(),
                token.window_id(),
                token.create_attempt(),
            )
            .filter(|binding| binding.window() == window)
        else {
            return false;
        };
        if records.output_resources.get(&token).is_none_or(|resource| {
            resource
                .reservation
                .binding()
                .is_some_and(|binding| binding.window() != window)
        }) {
            return false;
        }
        records.attach_output_binding(token, binding)
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn attach_output_binding(
        &self,
        token: NativeOutputToken,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.lock().attach_output_binding(token, binding)
    }

    pub(crate) fn require_output_wake_after_current(&self, token: NativeOutputToken) -> bool {
        self.lock().require_output_wake_after_current(token)
    }

    pub(crate) fn mark_output_submitted(&self, token: NativeOutputToken) -> bool {
        let mut records = self.lock();
        let prefix = records.output_prefix_range(1);
        if prefix.remaining == 0 {
            return false;
        }
        let Some(resource) = records.output_resources.get_mut(&token) else {
            return false;
        };
        let Some((ordinal, expected_token)) = prefix.outputs[0] else {
            return false;
        };
        if resource.submitted
            || expected_token != token
            || resource.terminal.is_none()
            || !resource
                .terminal_ingress()
                .is_some_and(|ingress| ingress.ordinal() == ordinal)
        {
            return false;
        }
        resource.submitted = true;
        records.submitted_output_prefix += 1;
        records.event_boundary_pending = true;
        true
    }

    pub(crate) fn abandon_output(&self, token: NativeOutputToken) -> bool {
        self.lock().abandon_output(token)
    }

    pub(crate) fn defer_output_input(&self, token: NativeOutputToken) -> bool {
        let mut records = self.lock();
        let Some(resource) = records.output_resources.get_mut(&token) else {
            return false;
        };
        if !resource.defer_before_input() {
            return false;
        }
        let binding = resource.reservation.binding();
        if token.viewport_id() != ViewportId::ROOT
            && binding.is_some()
            && records.viewports.binding(token.viewport_id()) == binding
        {
            records.block_semantic_viewport(token.viewport_id());
        }
        true
    }

    pub(crate) fn settle_output_abandonment(
        &self,
        token: NativeOutputToken,
        wake_on_terminal: bool,
        no_frame: Option<PendingNoFrameInvalidation>,
    ) -> OutputAbandonment {
        let mut records = self.lock();
        let disposition = records.settle_output_abandonment(token, wake_on_terminal);
        if matches!(
            disposition,
            OutputAbandonment::NewlyAbandoned | OutputAbandonment::AlreadyAbandonedAwaitingTerminal
        ) && let Some(invalidation) = no_frame
        {
            records
                .window_resources
                .retain_no_frame(token.viewport_id(), invalidation.binding, invalidation)
                .expect("an announced output prepays its exact no-frame terminal slot");
        }
        disposition
    }
}

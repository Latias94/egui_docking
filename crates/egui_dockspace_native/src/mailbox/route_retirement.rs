//! Route retirement, detached-placement retention and cleanup observations.

use super::*;

impl NativeHostState {
    #[cfg(test)]
    pub(super) fn retire_deferred_binding(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Vec<NativeOutputToken> {
        let mut abandoned = Vec::new();
        self.retire_deferred_binding_into(viewport, binding, |token| abandoned.push(token));
        abandoned
    }

    pub(super) fn retire_deferred_binding_into(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        mut abandoned: impl FnMut(NativeOutputToken),
    ) {
        self.retire_deferred_binding_resources(viewport, binding);
        for (token, reservation) in self.output_resources.iter_mut() {
            if reservation.reservation.binding() == Some(binding)
                && !reservation.reservation.terminal_recorded
                && reservation.reservation.abandon()
            {
                abandoned(*token);
            }
        }
    }

    pub(super) fn retire_deferred_binding_resources(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) {
        self.viewports.clear_semantic_wake(viewport, binding);
        self.window_resources.retire(viewport, binding);
    }

    pub(super) fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.journal
            .iter()
            .chain(self.owner_ingress().map(ReservedHostRecord::record))
            .any(|record| match record {
                HostRecord::WindowEvent(event) => event.references_binding(binding),
                HostRecord::DeviceRemoval(_) => false,
                HostRecord::GlobalFocus(focus) => focus.references_binding(binding),
                HostRecord::ViewportFocus(focus) => focus.references_binding(binding),
                HostRecord::ViewportPointerPassthrough(input) => input.references_binding(binding),
                // Binding retention belongs to the exact pending close, not a
                // callback-time route lookup which may now name its successor.
                HostRecord::ViewportCloseCancelled(_)
                | HostRecord::PointerObservation(_)
                | HostRecord::PointerCapture(_) => false,
                HostRecord::ViewportRoster(roster) => roster
                    .roster()
                    .observations()
                    .iter()
                    .any(|observation| observation.binding() == binding),
                HostRecord::ViewportCreateFailed(failure) => failure.binding() == binding,
                HostRecord::ViewportVisibility(visibility) => visibility.binding() == binding,
                HostRecord::ViewportCreated(created) => created.binding() == binding,
                HostRecord::Output { .. } => false,
            })
            || self
                .output_resources
                .values()
                .any(|resource| resource.reservation.binding() == Some(binding))
            || self.window_resources.references_binding(binding)
            || self.close_control.references_binding(binding)
    }

    pub(super) fn references_detached_placement(&self, binding: NativeSurfaceBinding) -> bool {
        self.journal
            .iter()
            .chain(self.owner_ingress().map(ReservedHostRecord::record))
            .any(|record| match record {
                HostRecord::WindowEvent(event) => event.binding() == Some(binding),
                HostRecord::ViewportPointerPassthrough(input) => input.binding() == Some(binding),
                HostRecord::ViewportRoster(roster) => roster
                    .roster()
                    .observations()
                    .iter()
                    .any(|observation| observation.binding() == binding),
                HostRecord::ViewportCreateFailed(failure) => failure.binding() == binding,
                HostRecord::ViewportVisibility(visibility) => visibility.binding() == binding,
                HostRecord::ViewportCreated(created) => created.binding() == binding,
                HostRecord::DeviceRemoval(_)
                | HostRecord::GlobalFocus(_)
                | HostRecord::ViewportFocus(_)
                | HostRecord::ViewportCloseCancelled(_)
                | HostRecord::PointerObservation(_)
                | HostRecord::PointerCapture(_)
                | HostRecord::Output { .. } => false,
            })
            || self.output_resources.values().any(|resource| {
                resource.reservation.binding() == Some(binding)
                    || match &resource.paint {
                        Some(PendingNativeOutput::Surfaces(outputs)) => outputs
                            .iter()
                            .any(|output| output.matches_native_binding(binding)),
                        Some(PendingNativeOutput::Staging(output)) => output.binding() == binding,
                        // An admitted pass or an in-flight terminal temporarily owns
                        // its contribution collection outside this lock.
                        None => {
                            resource.retains_detached_placements
                                && resource
                                    .reservation
                                    .binding()
                                    .is_some_and(|owner| owner.window() == binding.window())
                        }
                    }
            })
            || self.window_resources.references_placement(binding)
            || self.close_control.references_binding(binding)
    }
}

impl NativeHostBridge {
    /// Receiver retirement already removes every exact binding artifact. Drop
    /// affine paints in their original output slots without a token copy.
    pub(crate) fn retire_deferred_binding_paints(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) {
        let mut records = self.lock();
        records.retire_deferred_binding_resources(viewport, binding);
        for (_, resource) in records.output_resources.iter_mut() {
            if resource.reservation.binding() == Some(binding)
                && !resource.reservation.terminal_recorded
                && resource.reservation.abandon()
            {
                resource.paint.take();
            }
        }
    }

    pub(crate) fn prepare_committed_route_retirements(
        self: &Arc<Self>,
    ) -> Result<Option<PreparedRouteRetirements>, NativeRuntimeError> {
        let mut records = self.lock();
        let count = records.retirements.committed_routes().count();
        if count == 0 {
            return Ok(None);
        }
        let slots = records.retirements.capacity_limit().unwrap_or(count);
        let mut routes = Vec::new();
        routes
            .try_reserve_exact(slots)
            .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
        if count > slots || routes.capacity() > slots {
            return Err(NativeHostProtocolError::NativeReportStorageUnavailable.into());
        }
        routes.extend(records.retirements.committed_routes());
        self.prepare_route_retirements_locked(&mut records, routes)
            .map(Some)
    }

    pub(super) fn prepare_route_retirements_locked(
        self: &Arc<Self>,
        records: &mut NativeHostState,
        routes: Vec<CommittedRetirement>,
    ) -> Result<PreparedRouteRetirements, NativeRuntimeError> {
        for retirement in &routes {
            if !records
                .retirements
                .can_retire_committed_route_for_replacement(
                    retirement.viewport(),
                    retirement.binding(),
                )
            {
                return Err(NativeHostProtocolError::RetiredViewportRouteChanged(
                    retirement.binding().surface(),
                )
                .into());
            }
        }
        let token_slots = records.output_resources.maximum_token_count();
        let mut abandoned = Vec::new();
        abandoned
            .try_reserve_exact(token_slots)
            .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
        if abandoned.capacity() > token_slots {
            return Err(NativeHostProtocolError::NativeReportStorageUnavailable.into());
        }
        records
            .viewports
            .prepare_retirements(&routes)
            .map_err(|binding| {
                NativeHostProtocolError::RetiredViewportRouteChanged(binding.surface())
            })?;
        Ok(PreparedRouteRetirements {
            bridge: self.clone(),
            routes,
            abandoned,
            active: true,
        })
    }

    #[cfg(test)]
    pub(crate) fn prepare_route_retirements(
        self: &Arc<Self>,
        retirements: &[CommittedRetirement],
    ) -> Result<PreparedRouteRetirements, NativeRuntimeError> {
        let mut routes = Vec::new();
        routes
            .try_reserve_exact(retirements.len())
            .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
        routes.extend_from_slice(retirements);
        self.prepare_route_retirements_locked(&mut self.lock(), routes)
    }

    pub(crate) fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.lock().references_binding(binding)
    }

    pub(crate) fn references_detached_placement(&self, binding: NativeSurfaceBinding) -> bool {
        self.lock().references_detached_placement(binding)
    }
}

impl NativeHostBridge {
    pub(crate) fn retirement_has_pending_work(&self) -> bool {
        self.lock().retirements.has_pending_work()
    }

    #[cfg(test)]
    pub(crate) fn retirement_committed_routes(&self) -> Vec<CommittedRetirement> {
        self.lock().retirements.committed_routes().collect()
    }

    pub(crate) fn retirement_commit_routes(&self, committed: &[CommittedRetirement]) {
        self.lock().retirements.commit_routes(committed)
    }

    pub(crate) fn retirement_settle_snapshot(&self, applied: bool) {
        self.lock().retirements.settle_snapshot(applied)
    }

    #[cfg(test)]
    pub(crate) fn retirement_quiescence_candidates(&self) -> Vec<NativeSurfaceBinding> {
        self.lock().retirements.quiescence_candidates().collect()
    }

    pub(crate) fn next_retirement_quiescence_candidate(
        &self,
        after: Option<NativeSurfaceBinding>,
    ) -> Option<NativeSurfaceBinding> {
        self.lock().retirements.next_quiescence_candidate(after)
    }

    pub(crate) fn retirement_references_cleanup(&self, binding: NativeSurfaceBinding) -> bool {
        self.lock().retirements.references_cleanup(binding)
    }

    pub(crate) fn retirement_finish_quiescence(&self, binding: NativeSurfaceBinding) -> bool {
        self.lock().retirements.finish_quiescence(binding)
    }

    pub(crate) fn retirement_is_quiescence_candidate(&self, binding: NativeSurfaceBinding) -> bool {
        self.lock().retirements.is_quiescence_candidate(binding)
    }

    pub(crate) fn retirement_is_detached_placement(&self, binding: NativeSurfaceBinding) -> bool {
        self.lock().retirements.is_detached_placement(binding)
    }

    pub(crate) fn retirement_blocks_quiescence(&self, binding: NativeSurfaceBinding) -> bool {
        let records = self.lock();
        records
            .retirements
            .bindings()
            .any(|retained| retained == binding)
            && !records.retirements.is_quiescence_candidate(binding)
    }

    /// Records input retirement once under the exact physical callback owner.
    pub(crate) fn retire_destroyed_input(
        &self,
        record: &NativeWindowEventRecord,
        retire: impl FnOnce() -> Result<(), NativeRuntimeError>,
    ) -> Result<(), NativeRuntimeError> {
        let mut records = self.lock();
        let (Some(viewport), Some(binding)) = (record.viewport_id(), record.binding()) else {
            return Err(NativeHostProtocolError::WindowEventAcknowledgementMismatch.into());
        };
        if !matches!(record.event(), winit::event::WindowEvent::Destroyed) {
            return Err(NativeHostProtocolError::WindowEventAcknowledgementMismatch.into());
        }
        let window = record.window_id();
        let ordinal = record.ordinal();
        let receipt = if records.retirements.owns_window(window, Some(viewport)) {
            records
                .retirements
                .received_destroyed_receipt_mut(viewport, binding, window, ordinal)
        } else {
            records
                .viewports
                .received_destroyed_receipt_mut(viewport, binding, window, ordinal)
        }
        .ok_or(NativeHostProtocolError::WindowEventAcknowledgementMismatch)?;
        if receipt.retire_input(retire)? {
            // A later lifecycle failure retains the raw callback, but the new
            // core fact must still be allowed to reach its commit boundary.
            records.event_boundary_pending = true;
        }
        Ok(())
    }

    pub(crate) fn retirement_observe_destroyed(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        window: winit::window::WindowId,
        ordinal: u64,
    ) -> DestroyedObservation {
        let mut records = self.lock();
        if !records.retirements.owns_window(window, Some(viewport)) {
            if !records
                .retirements
                .can_retain_window_receipt(binding, window)
                || !records
                    .viewports
                    .has_received_destroyed(viewport, binding, window, ordinal)
            {
                return DestroyedObservation::Rejected;
            }
            let Some(receipt) = records.viewports.take_window_receipt(viewport, binding) else {
                return DestroyedObservation::Rejected;
            };
            records
                .retirements
                .retain_window(receipt)
                .expect("the reducer moves the exact captured window owner into retirement");
        }
        records
            .retirements
            .observe_destroyed(viewport, binding, window, ordinal)
    }

    pub(crate) fn retirement_retire_unrecognized_destroyed(&self, binding: NativeSurfaceBinding) {
        self.lock()
            .retirements
            .retire_unrecognized_destroyed(binding);
    }

    #[cfg(test)]
    pub(crate) fn retirement_destroyed_observations(
        &self,
    ) -> Vec<(NativeSurfaceBinding, NativeWindowFacts)> {
        self.lock().retirements.destroyed_observations().collect()
    }

    pub(crate) fn with_retirement_destroyed_observations<R>(
        &self,
        action: impl FnOnce(
            usize,
            &mut dyn Iterator<Item = (NativeSurfaceBinding, NativeWindowFacts)>,
            &dyn Fn(NativeSurfaceBinding) -> Option<Option<NativeCreateEffectAcknowledgement>>,
            &dyn Fn(NativeSurfaceBinding) -> Option<NativePreparationObservation>,
        ) -> R,
    ) -> R {
        let state = self.lock();
        let count = state.retirements.destroyed_observations().count();
        action(
            count,
            &mut state.retirements.destroyed_observations(),
            &|binding| state.window_resources.created_ack(binding),
            &|binding| state.window_resources.preparation_observation(binding),
        )
    }

    pub(crate) fn retirement_mark_snapshot_queued(&self) {
        self.lock().retirements.mark_snapshot_queued()
    }

    pub(crate) fn retirement_requires_snapshot(&self) -> bool {
        self.lock().retirements.requires_snapshot()
    }

    pub(crate) fn retirement_can_begin_release(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        let records = self.lock();
        (records.viewports.has_window_receipt(viewport, binding)
            || records.retirements.has_window(viewport, binding))
            && records.retirements.can_begin_release(viewport, binding)
    }

    pub(crate) fn retirement_begin_release(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        acknowledgement: NativeCloseEffectAcknowledgement,
    ) -> bool {
        let mut records = self.lock();
        if !records.retirements.can_begin_release(viewport, binding) {
            return false;
        }
        if !records.viewports.has_window_receipt(viewport, binding)
            && !records.retirements.has_window(viewport, binding)
        {
            return false;
        }
        let window = records.viewports.take_window_receipt(viewport, binding);
        records
            .retirements
            .begin_release(viewport, binding, acknowledgement, window)
            .expect("admitted lifetime retains prepaid retirement storage");
        true
    }

    pub(crate) fn retirement_can_retire_committed_route_for_replacement(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.lock()
            .retirements
            .can_retire_committed_route_for_replacement(viewport, binding)
    }

    pub(crate) fn retirement_retire_committed_route_for_replacement(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.lock()
            .retirements
            .retire_committed_route_for_replacement(viewport, binding)
    }

    pub(crate) fn retirement_can_accept_cleanup_observation(
        &self,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.lock()
            .retirements
            .can_accept_cleanup_observation(binding)
    }

    pub(crate) fn retirement_accept_cleanup_observation(
        &self,
        binding: NativeSurfaceBinding,
        observation: NativeCleanupObservation,
    ) -> Result<Option<NativeEffectResult>, ()> {
        self.lock()
            .retirements
            .accept_cleanup_observation(binding, observation)
    }

    pub(crate) fn retirement_cleanup_is_terminal(&self, binding: NativeSurfaceBinding) -> bool {
        self.lock().retirements.cleanup_is_terminal(binding)
    }

    pub(crate) fn retirement_can_accept_cleanup_result(
        &self,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.lock().retirements.can_accept_cleanup_result(binding)
    }

    pub(crate) fn retirement_retain_cleanup_result(
        &self,
        binding: NativeSurfaceBinding,
        result: NativeEffectResult,
    ) -> Result<Option<NativeEffectResult>, CleanupResultRetentionError> {
        self.lock()
            .retirements
            .retain_cleanup_result(binding, result)
    }

    pub(crate) fn retirement_mark_create_failure_recorded(&self, binding: NativeSurfaceBinding) {
        self.lock()
            .retirements
            .mark_create_failure_recorded(binding)
    }
}

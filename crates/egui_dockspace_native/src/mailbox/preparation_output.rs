//! Window preparation output transitions on the existing native host bridge.

use super::*;

impl NativeHostBridge {
    pub(crate) fn record_deferred_viewport_paint(
        &self,
        token: NativeOutputToken,
    ) -> DeferredViewportPaint {
        let mut records = self.lock();
        let Some(reservation) = records
            .output_resources
            .get(&token)
            .map(|resource| resource.reservation)
        else {
            return DeferredViewportPaint::Waiting;
        };
        if reservation.is_abandoned() {
            return DeferredViewportPaint::Waiting;
        }
        if let Some(PendingNativeOutput::Staging(request)) = records
            .output_resources
            .get(&token)
            .and_then(|resource| resource.paint.as_ref())
        {
            // Multipass callbacks reuse the original output owner. They repaint
            // the placeholder without consuming or abandoning its preparation.
            return DeferredViewportPaint::Staging(*request);
        }
        if let Some(create) = records.window_resources.create(token.viewport_id())
            && reservation.binding() == Some(create.binding)
        {
            let created = NativeViewportCreatedRecord::new(token, create.binding);
            let recorded = records.record_viewport_created(created);
            records
                .output_resources
                .get_mut(&token)
                .expect("the deferred output reservation remains present")
                .reservation
                .abandon();
            return if recorded {
                DeferredViewportPaint::Created
            } else {
                DeferredViewportPaint::Waiting
            };
        }
        let Some(request) = records.window_resources.staging(token.viewport_id()) else {
            if !records
                .window_resources
                .preparation_allows_semantic(token.viewport_id())
            {
                records
                    .output_resources
                    .get_mut(&token)
                    .expect("the current output remains reserved")
                    .reservation
                    .abandon_silently();
                return DeferredViewportPaint::Waiting;
            }
            if let Some(binding) = reservation.binding() {
                if records.output_resources.is_control(token) {
                    records.block_semantic_viewport(token.viewport_id());
                    return DeferredViewportPaint::Retain(binding);
                }
                if records.semantic_prelude_ready() {
                    records
                        .viewports
                        .clear_semantic_wake(token.viewport_id(), binding);
                    return DeferredViewportPaint::Semantic(binding);
                }
                records.block_semantic_viewport(token.viewport_id());
                return DeferredViewportPaint::Retain(binding);
            }
            records
                .output_resources
                .get_mut(&token)
                .expect("the deferred output reservation remains present")
                .reservation
                .abandon();
            return DeferredViewportPaint::Waiting;
        };
        // A retained predecessor output cannot begin or roll back its successor's paint.
        if reservation.binding() == Some(request.binding())
            && records
                .window_resources
                .begin_preparation_paint(token.viewport_id(), request)
        {
            let resource = records
                .output_resources
                .get_mut(&token)
                .expect("the deferred output reservation remains present");
            if resource
                .bind_paint(PendingNativeOutput::Staging(request))
                .is_ok()
            {
                resource.reservation.require_wake_after_current();
                return DeferredViewportPaint::Staging(request);
            }
            records
                .window_resources
                .finish_preparation_paint(token.viewport_id(), request, false);
        }
        records
            .output_resources
            .get_mut(&token)
            .expect("the deferred output reservation remains present")
            .reservation
            .abandon();
        DeferredViewportPaint::Waiting
    }

    pub(crate) fn begin_window_preparation(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        window: WindowId,
    ) {
        self.lock()
            .window_resources
            .begin_preparation(viewport, binding, window);
    }

    #[cfg(test)]
    pub(crate) fn ready_window_preparation_for_test(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        window: WindowId,
    ) {
        self.lock()
            .window_resources
            .ready_preparation_for_test(viewport, binding, window);
    }

    #[cfg(test)]
    pub(crate) fn lose_preparation_renderer_for_test(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) {
        let mut records = self.lock();
        records.window_resources.renderer_lost(viewport, binding);
        records.invalidate_viewport_roster();
    }

    #[cfg(test)]
    pub(crate) fn present_window_preparation_for_test(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) {
        let mut records = self.lock();
        records
            .window_resources
            .present_preparation_for_test(viewport, binding);
        records.invalidate_viewport_roster();
    }

    pub(crate) fn observe_window_preparations(&self, roster: &NativeViewportRosterRecord) {
        let mut records = self.lock();
        let Some(ordinal) = records.front_ingress().map(ReservedHostRecord::ordinal) else {
            return;
        };
        records
            .window_resources
            .observe_preparations(roster, ordinal);
    }

    pub(crate) fn revoke_preparation_inventory(&self) {
        let mut records = self.lock();
        records.window_resources.revoke_preparation_inventory();
        records.invalidate_viewport_roster();
    }

    pub(crate) fn settle_window_preparation(
        &self,
        binding: NativeSurfaceBinding,
        observation: NativePreparationObservation,
    ) {
        let mut records = self.lock();
        if records
            .window_resources
            .settle_preparation(binding, observation)
        {
            records.invalidate_viewport_roster();
        }
    }

    pub(crate) fn next_preparation_show(
        &self,
    ) -> Option<(ViewportId, NativeSurfaceBinding, WindowId)> {
        let records = self.lock();
        records
            .mode
            .accepts_new_work()
            .then(|| records.window_resources.next_preparation_show())
            .flatten()
    }

    pub(crate) fn dispatch_preparation_show(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        window: WindowId,
        token: eframe::NativeViewportVisibilityCommandToken,
    ) -> bool {
        let mut records = self.lock();
        if !records.mode.accepts_new_work()
            || records.window_resources.next_preparation_show() != Some((viewport, binding, window))
            || !records
                .window_resources
                .reserve_visibility_callback(viewport, binding, window, token, true)
        {
            return false;
        }
        records
            .window_resources
            .dispatch_preparation_show(viewport, binding, token)
    }

    pub(crate) fn reduce_preparation_visibility(
        &self,
        record: NativeViewportVisibilityRecord,
    ) -> bool {
        let mut records = self.lock();
        let Some(ordinal) = records.front_ingress().map(ReservedHostRecord::ordinal) else {
            return false;
        };
        let changed = records
            .window_resources
            .preparation_visibility(record, ordinal);
        if changed {
            records.invalidate_viewport_roster();
        }
        changed
    }

    pub(crate) fn finish_preparation_output(
        &self,
        token: NativeOutputToken,
        request: NativePreparationPaint,
        presented: bool,
    ) {
        let mut records = self.lock();
        if records.window_resources.finish_preparation_paint(
            token.viewport_id(),
            request,
            presented,
        ) {
            records.invalidate_viewport_roster();
        }
    }

    pub(crate) fn preparation_renderer_lost(
        &self,
        token: NativeOutputToken,
        binding: NativeSurfaceBinding,
    ) {
        let mut records = self.lock();
        records
            .window_resources
            .renderer_lost(token.viewport_id(), binding);
        records.invalidate_viewport_roster();
    }
}

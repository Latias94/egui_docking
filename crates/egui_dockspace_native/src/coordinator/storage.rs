//! Owned input, report and native-lifecycle transport data across existing owners.

use super::*;

/// One complete roster conversion and its retained settlement sidecars.
/// Core producer/replay storage and frozen source payloads have separate owners.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NativeSnapshotStorageLimit {
    pub(crate) window_slots: usize,
    pub(crate) work_area_slots: usize,
    pub(crate) byte_limit: usize,
}

impl NativeSnapshotStorageLimit {
    pub(crate) fn required_bytes(window_slots: usize, work_area_slots: usize) -> Option<usize> {
        let observations = window_slots.checked_mul(2)?;
        size_of::<QueuedNativeSnapshot>()
            .checked_add(size_of::<Vec<CompiledWindowObservation>>())?
            .checked_add(observations.checked_mul(size_of::<CompiledWindowObservation>())?)?
            .checked_add(observations.checked_mul(2 * size_of::<NativeSurfaceBinding>())?)?
            .checked_add(NativeWorkAreaState::construction_reservation_bytes(
                work_area_slots,
            )?)
    }

    fn validate(self) -> Result<(), NativeRuntimeError> {
        if self.window_slots == 0
            || self.work_area_slots == 0
            || Self::required_bytes(self.window_slots, self.work_area_slots)
                .is_none_or(|required| required > self.byte_limit)
        {
            return Err(NativeHostProtocolError::NativeReportStorageUnavailable.into());
        }
        Ok(())
    }
}

impl QueuedNativeSnapshot {
    fn owned_descendant_capacity_bytes(&self) -> usize {
        self.presentation_acknowledgements
            .capacity()
            .saturating_mul(size_of::<NativeSurfaceBinding>())
            .saturating_add(
                self.focus_reportable
                    .capacity()
                    .saturating_mul(size_of::<NativeSurfaceBinding>()),
            )
            .saturating_add(
                self.work_areas
                    .owned_data_capacity_bytes()
                    .saturating_sub(size_of::<PreparedWorkAreaRoster>()),
            )
    }
}

impl NativeCoordinator {
    /// Select before the first snapshot; exact terminal owners never compete
    /// for new conversion space after their original obligation is admitted.
    pub(crate) fn configure_snapshot_storage_limit(
        &mut self,
        limit: NativeSnapshotStorageLimit,
    ) -> Result<(), NativeRuntimeError> {
        limit.validate()?;
        if self.snapshot_storage.is_some()
            || self.queued_snapshot.is_some()
            || !self.work_areas.can_configure_capacity_limit()
        {
            return Err(NativeHostProtocolError::NativeReportStorageUnavailable.into());
        }
        let work_areas = NativeWorkAreaState::with_capacity_limit(limit.work_area_slots)?;
        if !self.bridge.configure_snapshot_storage_allowance(
            limit.byte_limit,
            limit.window_slots,
            limit.work_area_slots,
        ) {
            return Err(NativeHostProtocolError::NativeReportStorageUnavailable.into());
        }
        self.work_areas = work_areas;
        self.snapshot_storage = Some(limit);
        Ok(())
    }

    pub(super) fn preflight_snapshot_storage(
        &self,
        roster: &NativeViewportRosterRecord,
        destroyed_count: usize,
    ) -> Result<(), NativeRuntimeError> {
        let Some(limit) = self.snapshot_storage else {
            return Ok(());
        };
        let area_count = match roster.work_areas() {
            Ok(FrozenWorkAreaRoster::Exact(records)) => records.len(),
            _ => 0,
        };
        if roster.observation_count() > limit.window_slots
            || destroyed_count > limit.window_slots
            || area_count > limit.work_area_slots
            || self.queued_snapshot.is_some()
        {
            return Err(NativeHostProtocolError::NativeReportStorageUnavailable.into());
        }
        Ok(())
    }

    /// Measures current owned data, rather than promised capacity for future terminals.
    pub(crate) fn owned_input_capacity_bytes(&self) -> Result<usize, NativeRuntimeError> {
        // Renderer/receiver authority, application-facing transition outputs and
        // diagnostic errors are outside the native input transport boundary.
        let inline = size_of::<Self>()
            - size_of::<DockspaceSession>()
            - size_of::<NativeReceiverStore>()
            - size_of::<Vec<DockspacePresentationTransition>>()
            - size_of::<VecDeque<NativeRuntimeError>>();
        let external_roster = self
            .queued_snapshot
            .as_ref()
            .map(|snapshot| snapshot.roster.shared_roster());
        Ok(inline
            .saturating_add(self.bridge.owned_input_capacity_bytes(external_roster))
            .saturating_add(self.session.native_input_owned_capacity_bytes()?)
            .saturating_add(self.session.native_input_producer_storage_capacity_bytes())
            .saturating_add(self.session.native_input_pending_report_capacity_bytes())
            .saturating_add(self.pending_no_frame_invalidations.len().saturating_mul(
                size_of::<NativeSurfaceBinding>() + size_of::<PendingNoFrameInvalidation>(),
            ))
            .saturating_add(
                self.pending_presentation_acknowledgements
                    .len()
                    .saturating_mul(
                        size_of::<NativeSurfaceBinding>()
                            + size_of::<NativePresentationEffectAcknowledgement>(),
                    ),
            )
            .saturating_add(
                self.queued_snapshot
                    .as_ref()
                    .map_or(0, QueuedNativeSnapshot::owned_descendant_capacity_bytes),
            )
            .saturating_add(
                self.deferred_viewports
                    .owned_data_capacity_bytes()
                    .saturating_sub(size_of::<DeferredViewportDriver>()),
            )
            .saturating_add(
                self.effects
                    .owned_data_capacity_bytes()
                    .saturating_sub(size_of::<NativeEffectCoordinator>()),
            )
            .saturating_add(
                self.input_control
                    .owned_data_capacity_bytes()
                    .saturating_sub(size_of::<NativeInputControl>()),
            )
            .saturating_add(
                self.pointer_translator
                    .owned_data_capacity_bytes()
                    .saturating_sub(size_of::<NativePointerTranslator>()),
            )
            .saturating_add(
                self.work_areas
                    .owned_data_capacity_bytes()
                    .saturating_sub(size_of::<NativeWorkAreaState>()),
            )
            .saturating_add(
                self.pending_effect_results
                    .capacity()
                    .saturating_mul(size_of::<PendingNativeEffectResult>()),
            ))
    }
}

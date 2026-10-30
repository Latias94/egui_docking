//! Owned input, report and native-lifecycle transport data across existing owners.

use super::*;

/// Complete storage limits for the fork-backed native input/lifecycle host.
///
/// Limits cover owned input data and promised construction capacity, excluding
/// allocator metadata, core workspace/reducer state, renderer/receiver data,
/// application actions and diagnostics. Generic hosting remains unlimited unless
/// this value is supplied at application construction.
#[derive(Debug, Clone, Copy)]
pub struct NativeStorageLimit {
    pub(crate) combined: NativeCombinedStorageLimit,
}

impl NativeStorageLimit {
    /// Selects a combined byte allowance and independent host-owner bounds.
    ///
    /// `input` must select producer and report storage limits. Resource entries
    /// are keyed by exact viewport/binding, ordinary commands and native closes
    /// have independent limits, and retired obligations keep their original
    /// slots until settlement. These counts are not inferred from window count.
    ///
    /// # Errors
    ///
    /// Rejects missing subprofile bounds, overflow, or a total allowance too
    /// small for complete initial native backing and future owner promises.
    pub fn new(
        input: NativeInputStorageLimit,
        byte_limit: usize,
        resource_slots: usize,
        command_slots: usize,
        close_slots: usize,
    ) -> Result<Self, NativeRuntimeError> {
        let producer = input
            .producer_storage_limit()
            .ok_or(NativeHostProtocolError::NativeOwnerStorageConfiguration)?;
        let reports = input
            .report_storage_limit()
            .ok_or(NativeHostProtocolError::NativeOwnerStorageConfiguration)?;
        let owner_bytes = NativeOwnerStorageLimit::required_bytes(
            producer.binding_slots().get(),
            reports.effect_slots().get(),
            resource_slots,
            command_slots,
            close_slots,
        )
        .ok_or(NativeHostProtocolError::NativeReportStorageUnavailable)?;
        let combined = NativeCombinedStorageLimit {
            input,
            byte_limit,
            owners: NativeOwnerStorageLimit {
                resource_slots,
                command_slots,
                close_slots,
                byte_limit: owner_bytes,
            },
        };
        combined.validate()?;
        Ok(Self { combined })
    }

    /// Returns the complete selected native input-data byte allowance.
    #[must_use]
    pub const fn byte_limit(self) -> usize {
        self.combined.byte_limit
    }

    /// Returns the checked initial backing and future-work reservation.
    #[must_use]
    pub fn initial_reserved_bytes(self) -> usize {
        self.combined
            .initial_required_bytes()
            .expect("validated native storage limits retain their initial promise")
    }
}

/// A separately selected total native input-data promise. Production enablement
/// additionally requires configured-cap load and lifecycle qualification.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NativeCombinedStorageLimit {
    pub(crate) input: NativeInputStorageLimit,
    pub(crate) owners: NativeOwnerStorageLimit,
    pub(crate) byte_limit: usize,
}

impl NativeCombinedStorageLimit {
    fn dimensions(self) -> Option<(usize, usize)> {
        self.input.producer_storage_limit().map(|producer| {
            (
                producer.binding_slots().get(),
                producer.work_area_slots().get(),
            )
        })
    }

    pub(super) fn external_reserved_bytes(self) -> Option<usize> {
        Self::coordinator_inline_bytes()?
            .checked_add(self.input.byte_limit())?
            .checked_add(self.input.producer_storage_limit()?.byte_limit())?
            .checked_add(self.input.report_storage_limit()?.byte_limit())
    }

    /// The complete fresh bridge and all selected future owner promises coexist
    /// during initial construction. Keeping the old empty inline lifecycle and
    /// resource headers also covers their replacement-construction overlap.
    pub(crate) fn initial_required_bytes(self) -> Option<usize> {
        let (windows, areas) = self.dimensions()?;
        NativeHostBridge::initial_storage_bytes(windows, areas)?
            .checked_add(NativePointerTranslator::default().reserved_translation_capacity_bytes())?
            .checked_add(NativeSnapshotStorageLimit::required_bytes(windows, areas)?)?
            .checked_add(self.owners.byte_limit)?
            .checked_add(self.external_reserved_bytes()?)
    }

    fn coordinator_inline_bytes() -> Option<usize> {
        // The selected O/S/R promises already own these headers. Preserve all
        // other inline fields and padding, including the permanent translator;
        // its separate bridge promise holds the overlapping retry copy.
        size_of::<NativeCoordinator>()
            .checked_sub(size_of::<DockspaceSession>())?
            .checked_sub(size_of::<NativeReceiverStore>())?
            .checked_sub(size_of::<Vec<DockspacePresentationTransition>>())?
            .checked_sub(size_of::<VecDeque<NativeRuntimeError>>())?
            .checked_sub(size_of::<Option<NativeOwnerStorageLimit>>())?
            .checked_sub(size_of::<NativeInputControl>())?
            .checked_sub(size_of::<NativeEffectCoordinator>())?
            .checked_sub(size_of::<DeferredViewportDriver>())?
            .checked_sub(size_of::<Vec<dockspace::close::CloseRequestId>>())?
            .checked_sub(size_of::<NativeWorkAreaState>())?
            .checked_sub(size_of::<QueuedNativeSnapshot>())?
            .checked_sub(size_of::<VecDeque<PendingNativeEffectResult>>())
    }

    pub(super) fn validate(self) -> Result<(), NativeRuntimeError> {
        let (windows, _) = self
            .dimensions()
            .ok_or(NativeHostProtocolError::NativeOwnerStorageConfiguration)?;
        let reports = self
            .input
            .report_storage_limit()
            .ok_or(NativeHostProtocolError::NativeOwnerStorageConfiguration)?;
        self.owners
            .validate(Some(windows), Some(reports.effect_slots().get()))?;
        if self
            .initial_required_bytes()
            .is_none_or(|bytes| bytes > self.byte_limit)
        {
            return Err(NativeHostProtocolError::NativeReportStorageUnavailable.into());
        }
        Ok(())
    }
}

/// Independent host-owner bounds; window lifetimes and effect leases do not
/// imply a fixed number of retained resource or ordinary command entries.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NativeOwnerStorageLimit {
    pub(crate) resource_slots: usize,
    pub(crate) command_slots: usize,
    pub(crate) close_slots: usize,
    pub(crate) byte_limit: usize,
}

impl NativeOwnerStorageLimit {
    pub(super) fn external_reserved_bytes(self, bindings: usize) -> Option<usize> {
        self.byte_limit
            .checked_sub(NativeHostBridge::window_resource_storage_bytes(
                self.resource_slots,
            )?)?
            .checked_sub(NativeHostBridge::window_resource_staging_bytes(
                self.resource_slots,
            )?)?
            .checked_sub(NativeHostBridge::lifecycle_storage_bytes(
                bindings,
                self.close_slots,
            )?)
    }

    pub(crate) fn required_bytes(
        bindings: usize,
        effects: usize,
        resources: usize,
        commands: usize,
        closes: usize,
    ) -> Option<usize> {
        size_of::<Option<Self>>()
            // One frozen dispatch prefix can coexist with the single-entry
            // submission check. Both bounded stack buffers belong to the host.
            .checked_add(size_of::<crate::mailbox::NativeOutputPrefix>().checked_mul(2)?)?
            .checked_add(NativeHostBridge::window_resource_storage_bytes(resources)?)?
            .checked_add(NativeHostBridge::window_resource_staging_bytes(resources)?)?
            .checked_add(NativeHostBridge::lifecycle_storage_bytes(bindings, closes)?)?
            .checked_add(NativeHostBridge::retirement_preparation_storage_bytes(
                bindings,
            )?)?
            .checked_add(size_of::<PreparedNoFrameInvalidations>())?
            .checked_add(
                resources.checked_mul(
                    size_of::<PendingNoFrameInvalidation>() + size_of::<ViewportId>(),
                )?,
            )?
            .checked_add(size_of::<Vec<dockspace::close::CloseRequestId>>())?
            .checked_add(closes.checked_mul(size_of::<dockspace::close::CloseRequestId>())?)?
            .checked_add(NativeInputControl::reservation_bytes(
                Some(bindings),
                Some(effects),
            )?)?
            .checked_add(NativeEffectCoordinator::reservation_bytes(
                Some(effects),
                Some(commands),
            )?)?
            .checked_add(DeferredViewportDriver::reservation_bytes(bindings)?)?
            .checked_add(size_of::<Vec<DeferredViewportSpec>>())?
            .checked_add(bindings.checked_mul(size_of::<DeferredViewportSpec>())?)
    }

    pub(super) fn validate(
        self,
        bindings: Option<usize>,
        effects: Option<usize>,
    ) -> Result<(), NativeRuntimeError> {
        let (Some(bindings), Some(effects)) = (bindings, effects) else {
            return Err(NativeHostProtocolError::NativeOwnerStorageConfiguration.into());
        };
        if Self::required_bytes(
            bindings,
            effects,
            self.resource_slots,
            self.command_slots,
            self.close_slots,
        )
        .is_none_or(|required| required > self.byte_limit)
        {
            return Err(NativeHostProtocolError::NativeReportStorageUnavailable.into());
        }
        Ok(())
    }
}

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
            .checked_add(observations.checked_mul(
                size_of::<NativeSnapshotAcknowledgements>() + size_of::<NativeSurfaceBinding>(),
            )?)?
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
        self.acknowledgements
            .capacity()
            .saturating_mul(size_of::<NativeSnapshotAcknowledgements>())
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
    pub(crate) fn combined_storage_reserved_bytes(&self) -> Option<usize> {
        self.combined_storage_limit?;
        self.bridge.selected_storage_reserved_bytes()
    }

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
            )
            .saturating_add(
                self.close_decision_scratch
                    .capacity()
                    .saturating_mul(size_of::<dockspace::close::CloseRequestId>()),
            ))
    }
}

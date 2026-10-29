//! Owned input, report and native-lifecycle transport data across existing owners.

use super::*;

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

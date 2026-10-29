//! Complete-roster storage is promised before a new native lifetime is admitted.

use super::journal::HostIngressJournal;
use super::output_resource::NativeOutputResources;
use super::roster::NativeViewportRosterRecord;
use super::window_resource::NativeWindowResources;
use super::{HostRecord, NativeHostBridge, NativeHostState, NativeSurfaceBinding};
use crate::close_control::NativeCloseControl;
use crate::retirement::NativeRetirementState;
use crate::viewport_map::NativeViewportMap;
use eframe::egui::ViewportId;
use std::sync::Arc;

/// A host share of one selected input budget, including promised external work.
/// Existing owners remain responsible for their exact protocol and lifetime.
#[derive(Debug, Clone, Copy)]
pub(super) struct NativeHostInputAllowance {
    byte_limit: usize,
    external_reserved_bytes: usize,
}

impl NativeHostState {
    fn roster_payloads<'state>(
        &'state self,
        external: Option<&'state Arc<NativeViewportRosterRecord>>,
    ) -> impl Iterator<Item = &'state Arc<NativeViewportRosterRecord>> {
        self.committed_viewport_roster
            .iter()
            .map(|key| &key.roster)
            .chain(self.in_flight_viewport_roster.iter().map(|key| &key.roster))
            .chain(
                self.journal
                    .iter()
                    .chain(self.owner_ingress().map(|ingress| ingress.record()))
                    .filter_map(|record| match record {
                        HostRecord::ViewportRoster(envelope) => Some(&envelope.key.roster),
                        _ => None,
                    }),
            )
            .chain(external)
    }

    /// Counts entry data and backing capacities, without allocator or Arc metadata.
    fn owned_input_capacity_bytes(
        &self,
        external: Option<&Arc<NativeViewportRosterRecord>>,
    ) -> usize {
        let mut bytes = size_of::<Self>()
            .saturating_add(
                self.journal.owned_buffer_capacity_bytes() - size_of::<HostIngressJournal>(),
            )
            .saturating_add(
                self.viewports.owned_data_capacity_bytes() - size_of::<NativeViewportMap>(),
            )
            .saturating_add(
                self.window_resources.owned_data_capacity_bytes()
                    - size_of::<NativeWindowResources>(),
            )
            .saturating_add(
                self.retirements.owned_data_capacity_bytes() - size_of::<NativeRetirementState>(),
            )
            .saturating_add(
                self.close_control.owned_data_capacity_bytes() - size_of::<NativeCloseControl>(),
            )
            .saturating_add(self.output_resources.owned_descendant_capacity_bytes())
            .saturating_add(
                self.blocked_semantic_viewports
                    .len()
                    .saturating_mul(size_of::<ViewportId>()),
            )
            .saturating_add(
                self.ready_semantic_viewports
                    .len()
                    .saturating_mul(size_of::<ViewportId>()),
            )
            .saturating_add(self.terminal_roster.reserved_payload_capacity_bytes());
        // Production rosters always belong to the two backing owners. Tests may
        // inject external allocations; compare the existing handles without
        // keeping a persistent allocation-address registry.
        for (index, payload) in self.roster_payloads(external).enumerate() {
            if !self.terminal_roster.owns_payload(payload)
                && !self
                    .roster_payloads(external)
                    .take(index)
                    .any(|previous| Arc::ptr_eq(previous, payload))
            {
                bytes = bytes.saturating_add(payload.owned_payload_capacity_bytes());
            }
        }
        bytes
    }

    pub(super) fn can_reserve_input_data(&self, additional: usize) -> bool {
        self.input_storage_allowance.is_none_or(|allowance| {
            self.owned_input_capacity_bytes(None)
                .checked_add(size_of::<NativeHostBridge>() - size_of::<NativeHostState>())
                .and_then(|bytes| bytes.checked_add(allowance.external_reserved_bytes))
                .and_then(|bytes| bytes.checked_add(additional))
                .is_some_and(|bytes| bytes <= allowance.byte_limit)
        })
    }

    pub(super) fn can_reserve_output_data(&self) -> bool {
        self.output_resources.can_reserve_ordinary()
            && self.can_reserve_input_data(NativeOutputResources::ordinary_entry_data_bytes())
    }

    /// Existing route, resource and retirement roots share one lifetime promise.
    /// A route-to-retirement move cannot return capacity before quiescence.
    pub(super) fn can_admit_binding(&self, incoming: NativeSurfaceBinding) -> bool {
        let bindings = || {
            self.viewports
                .bindings()
                .chain(self.retirements.bindings())
                .chain(self.window_resources.bindings())
        };
        if bindings().any(|binding| binding.same_window_lifetime(incoming)) {
            return true;
        }
        let retained = bindings()
            .enumerate()
            .filter(|(index, binding)| {
                !bindings()
                    .take(*index)
                    .any(|previous| previous.same_window_lifetime(*binding))
            })
            .count();
        retained < self.terminal_roster.window_capacity()
    }
}

impl NativeHostBridge {
    /// Configures one combined host allowance with room for initial output progress.
    ///
    /// The caller promises all non-bridge data and constructor/terminal work in
    /// `external_reserved_bytes`; generic/default hosting remains unlimited.
    /// Other growth boundaries must use the same allowance before product enablement.
    pub(crate) fn configure_input_storage_allowance(
        &self,
        byte_limit: usize,
        external_reserved_bytes: usize,
    ) -> bool {
        let mut state = self.lock();
        let existing = state.input_storage_allowance;
        state.input_storage_allowance = Some(NativeHostInputAllowance {
            byte_limit,
            external_reserved_bytes,
        });
        let fits = if existing.is_some() {
            state.can_reserve_input_data(0)
        } else {
            state.can_reserve_output_data()
        };
        if !fits {
            state.input_storage_allowance = existing;
            return false;
        }
        true
    }

    pub(crate) fn owned_input_capacity_bytes(
        &self,
        external_roster: Option<&Arc<NativeViewportRosterRecord>>,
    ) -> usize {
        size_of::<Self>().saturating_add(
            self.lock()
                .owned_input_capacity_bytes(external_roster)
                .saturating_sub(size_of::<NativeHostState>()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::roster::{NativeViewportRosterEnvelope, NativeViewportRosterKey};
    use super::*;
    use crate::event::NativeDeviceRemovalRecord;

    #[test]
    fn host_allowance_reserves_output_progress_and_rejection_preserves_existing_configuration() {
        let bridge = NativeHostBridge::new(NativeViewportMap::default());
        let baseline = bridge.owned_input_capacity_bytes(None);
        let output = NativeOutputResources::ordinary_entry_data_bytes();
        assert!(!bridge.configure_input_storage_allowance(baseline, 0));
        assert!(bridge.lock().input_storage_allowance.is_none());
        assert!(bridge.configure_input_storage_allowance(baseline + output, 0));
        assert!(bridge.lock().can_reserve_input_data(output));
        assert!(!bridge.lock().can_reserve_input_data(output + 1));
        assert!(!bridge.configure_input_storage_allowance(baseline + output, output + 1));
        assert!(bridge.lock().can_reserve_input_data(output));
        assert_eq!(bridge.owned_input_capacity_bytes(None), baseline);
    }

    #[test]
    fn promised_external_work_reduces_the_same_host_allowance_before_growth() {
        let bridge = NativeHostBridge::new(NativeViewportMap::default());
        let baseline = bridge.owned_input_capacity_bytes(None);
        let output = NativeOutputResources::ordinary_entry_data_bytes();
        assert!(bridge.configure_input_storage_allowance(baseline + 2 * output, output));
        assert!(bridge.lock().can_reserve_output_data());
        assert!(bridge.configure_input_storage_allowance(baseline + 2 * output, 2 * output));
        assert!(!bridge.lock().can_reserve_output_data());
        assert_eq!(bridge.owned_input_capacity_bytes(None), baseline);
        assert_eq!(
            bridge.admission_dispatch(),
            super::super::admission::NativeDispatch::ContinueBoundary
        );
        assert!(matches!(
            bridge.lock().admission,
            super::super::admission::NativeAdmission::Draining { boundary: 0 }
        ));
        assert!(bridge.finish_overload_cancellation(0));
        assert!(matches!(
            bridge.lock().admission,
            super::super::admission::NativeAdmission::Recovering { boundary: 0 }
        ));
        bridge.commit_frame_boundary(false);
        assert_eq!(
            bridge.admission_dispatch(),
            super::super::admission::NativeDispatch::Idle
        );
        assert!(matches!(
            bridge.lock().admission,
            super::super::admission::NativeAdmission::Recovering { boundary: 0 }
        ));
        assert!(bridge.configure_input_storage_allowance(baseline + 2 * output, output));
        assert_eq!(
            bridge.admission_dispatch(),
            super::super::admission::NativeDispatch::Idle
        );
        assert!(matches!(
            bridge.lock().admission,
            super::super::admission::NativeAdmission::Running
        ));
    }

    #[test]
    fn raw_transfer_and_commit_keep_the_preallocated_storage_charged() {
        let bridge = NativeHostBridge::new(NativeViewportMap::default());
        let baseline = bridge.owned_input_capacity_bytes(None);
        {
            let mut state = bridge.lock();
            for ordinal in 1..=256 {
                assert!(state.journal.push_back(HostRecord::DeviceRemoval(
                    NativeDeviceRemovalRecord::for_test(ordinal),
                )));
            }
            assert_eq!(state.journal.retained_record_count(), 256);
        }
        assert_eq!(bridge.owned_input_capacity_bytes(None), baseline);
        {
            let mut state = bridge.lock();
            state.journal.transfer_front(256);
            assert!(state.journal.is_empty());
            assert_eq!(state.journal.retained_record_count(), 256);
        }
        assert_eq!(bridge.owned_input_capacity_bytes(None), baseline);
        bridge.lock().journal.commit_transferred();
        assert_eq!(bridge.owned_input_capacity_bytes(None), baseline);
    }

    #[test]
    fn shared_roster_is_charged_once_across_raw_keys_and_queued_snapshot() {
        let bridge = NativeHostBridge::new(NativeViewportMap::default());
        let baseline = bridge.owned_input_capacity_bytes(None);
        let payload = Arc::new(NativeViewportRosterRecord::for_test([], true));
        let payload_bytes = payload.owned_payload_capacity_bytes();
        {
            let mut state = bridge.lock();
            state.committed_viewport_roster = Some(NativeViewportRosterKey {
                context: 1,
                roster: Arc::clone(&payload),
            });
            state.in_flight_viewport_roster = state.committed_viewport_roster.clone();
            assert!(state.journal.push_back(HostRecord::ViewportRoster(
                NativeViewportRosterEnvelope::from_shared(1, Arc::clone(&payload), true, None),
            )));
        }
        assert_eq!(
            bridge.owned_input_capacity_bytes(Some(&payload)),
            baseline + payload_bytes
        );
        let distinct = Arc::new(NativeViewportRosterRecord::for_test([], true));
        assert_eq!(payload.as_ref(), distinct.as_ref());
        assert_eq!(
            bridge.owned_input_capacity_bytes(Some(&distinct)),
            baseline + 2 * payload_bytes,
            "equal facts in distinct allocations remain distinct resident data"
        );
        {
            let mut state = bridge.lock();
            state.journal.transfer_front(1);
            state.committed_viewport_roster = None;
            state.in_flight_viewport_roster = None;
        }
        assert_eq!(
            bridge.owned_input_capacity_bytes(Some(&payload)),
            baseline + payload_bytes,
            "a queued snapshot can be the allocation's last transport reference"
        );
        assert_eq!(bridge.owned_input_capacity_bytes(None), baseline);
    }
}

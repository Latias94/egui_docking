//! Complete-roster storage is promised before a new native lifetime is admitted.

use super::journal::HostIngressJournal;
use super::output_resource::NativeOutputResources;
use super::roster::NativeViewportRosterRecord;
use super::window_resource::NativeWindowResources;
use super::{HostRecord, NativeHostBridge, NativeHostState, NativeSurfaceBinding};
use crate::close_control::NativeCloseControl;
use crate::error::NativeHostProtocolError;
use crate::event::NativeWindowEventClass;
use crate::pointer_event::NativePointerTranslator;
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
                .and_then(|bytes| {
                    bytes.checked_add(self.reserved_pointer_translation_capacity_bytes())
                })
                .and_then(|bytes| bytes.checked_add(self.snapshot_storage_reserved_bytes))
                .and_then(|bytes| bytes.checked_add(self.window_resource_staging_reserved_bytes))
                .and_then(|bytes| bytes.checked_add(self.host_owner_external_reserved_bytes))
                .and_then(|bytes| bytes.checked_add(allowance.external_reserved_bytes))
                .and_then(|bytes| bytes.checked_add(additional))
                .is_some_and(|bytes| bytes <= allowance.byte_limit)
        })
    }

    /// Every accepted raw pointer promises its worst-case state growth before
    /// capture. Transfer replaces that promise with two copies of current state;
    /// cancellation and binding/device retirement need no extra allocation.
    pub(super) fn reserved_pointer_translation_capacity_bytes(&self) -> usize {
        let pending = self
            .journal
            .iter()
            .filter(|record| {
                matches!(record, HostRecord::WindowEvent(event)
                    if NativeWindowEventClass::classify(event.event()) == NativeWindowEventClass::Pointer)
            })
            .count();
        self.pointer_translation_reserved_bytes.saturating_add(
            pending.saturating_mul(NativePointerTranslator::callback_translation_capacity_bytes()),
        )
    }

    pub(super) fn can_reserve_pointer_translation(&self) -> bool {
        NativePointerTranslator::callback_translation_capacity_bytes()
            .checked_add(NativeOutputResources::ordinary_entry_data_bytes())
            .is_some_and(|bytes| self.can_reserve_input_data(bytes))
    }

    pub(super) fn can_reserve_output_data(&self) -> bool {
        self.output_resources.can_reserve_ordinary()
            && self.can_reserve_input_data(NativeOutputResources::ordinary_entry_data_bytes())
    }

    /// Existing route, resource and retirement roots share one lifetime promise.
    /// A route-to-retirement move cannot return capacity before quiescence.
    fn retained_lifetime_count(&self) -> usize {
        let bindings = || {
            self.viewports
                .bindings()
                .chain(self.retirements.bindings())
                .chain(self.window_resources.bindings())
        };
        bindings()
            .enumerate()
            .filter(|(index, binding)| {
                !bindings()
                    .take(*index)
                    .any(|previous| previous.same_window_lifetime(*binding))
            })
            .count()
    }

    pub(super) fn can_admit_binding(&mut self, incoming: NativeSurfaceBinding) -> bool {
        let already_retained = self
            .viewports
            .bindings()
            .chain(self.retirements.bindings())
            .chain(self.window_resources.bindings())
            .any(|binding| binding.same_window_lifetime(incoming));
        let Some(required) = self
            .retained_lifetime_count()
            .checked_add(usize::from(!already_retained))
        else {
            return false;
        };
        let Some(growth) = NativeRetirementState::reservation_bytes(required)
            .map(|bytes| bytes.saturating_sub(self.retirements.owned_data_capacity_bytes()))
        else {
            return false;
        };
        required <= self.terminal_roster.window_capacity()
            && self.can_reserve_input_data(growth)
            && self
                .retirements
                .prepare_capacity_for_retained_lifetimes(required)
                .is_ok()
    }
}

impl NativeHostBridge {
    pub(crate) fn lifecycle_storage_bytes(bindings: usize, closes: usize) -> Option<usize> {
        NativeRetirementState::reservation_bytes(bindings)?
            .checked_add(NativeCloseControl::reservation_bytes(closes)?)?
            .checked_add(NativeViewportMap::reservation_bytes(bindings)?)
    }

    pub(crate) fn retirement_preparation_storage_bytes(bindings: usize) -> Option<usize> {
        size_of::<super::PreparedRouteRetirements>()
            .checked_add(
                bindings.checked_mul(size_of::<crate::retirement::CommittedRetirement>())?,
            )?
            .checked_add(
                NativeOutputResources::default_maximum_token_count()
                    .checked_mul(size_of::<eframe::NativeOutputToken>())?,
            )
    }

    /// Initial lifecycle backing covers every route's future terminal move.
    /// A late callback only fills its original receipt and cannot need a slot.
    pub(crate) fn configure_lifecycle_storage(
        &self,
        bindings: usize,
        closes: usize,
    ) -> Result<(), NativeHostProtocolError> {
        let mut state = self.lock();
        if !state.retirements.can_configure_capacity_limit()
            || !state.close_control.can_configure_capacity_limit()
            || !state.viewports.can_configure_capacity_limit()
            || state.journal.captured_through() != 0
            || state.output_context.is_some()
        {
            return Err(NativeHostProtocolError::NativeReportStorageUnavailable);
        }
        let additional = Self::lifecycle_storage_bytes(bindings, closes)
            .and_then(|bytes| bytes.checked_add(NativeOutputResources::ordinary_entry_data_bytes()))
            .ok_or(NativeHostProtocolError::NativeReportStorageUnavailable)?;
        // Existing backing remains live until all replacements have succeeded.
        if !state.can_reserve_input_data(additional) {
            return Err(NativeHostProtocolError::NativeReportStorageUnavailable);
        }
        let retirements = NativeRetirementState::with_capacity_limit(bindings)?;
        let close_control = NativeCloseControl::with_capacity_limit(closes)?;
        let viewports = NativeViewportMap::with_capacity_limit(bindings)?;
        state.retirements = retirements;
        state.close_control = close_control;
        state.viewports = viewports;
        Ok(())
    }

    pub(crate) fn window_resource_storage_bytes(slots: usize) -> Option<usize> {
        NativeWindowResources::reservation_bytes(slots)
    }

    pub(crate) fn window_resource_staging_bytes(slots: usize) -> Option<usize> {
        NativeWindowResources::staging_reservation_bytes(slots)
    }

    /// Initial installation reserves current entry backing plus one complete
    /// staging preparation before any terminal obligation or capture exists.
    pub(crate) fn configure_window_resource_storage(
        &self,
        slots: usize,
        external_reserved_bytes: usize,
    ) -> Result<(), NativeHostProtocolError> {
        let mut state = self.lock();
        if state.window_resource_staging_reserved_bytes != 0
            || !state.window_resources.can_configure_capacity_limit()
            || state.window_resources.owned_data_capacity_bytes()
                != size_of::<NativeWindowResources>()
            || state.journal.captured_through() != 0
            || state.output_context.is_some()
        {
            return Err(NativeHostProtocolError::NativeReportStorageUnavailable);
        }
        let staging = Self::window_resource_staging_bytes(slots)
            .ok_or(NativeHostProtocolError::NativeReportStorageUnavailable)?;
        let additional = Self::window_resource_storage_bytes(slots)
            .and_then(|bytes| bytes.checked_sub(size_of::<NativeWindowResources>()))
            .and_then(|bytes| bytes.checked_add(staging))
            .and_then(|bytes| bytes.checked_add(external_reserved_bytes))
            .and_then(|bytes| bytes.checked_add(NativeOutputResources::ordinary_entry_data_bytes()))
            .ok_or(NativeHostProtocolError::NativeReportStorageUnavailable)?;
        if !state.can_reserve_input_data(additional) {
            return Err(NativeHostProtocolError::NativeReportStorageUnavailable);
        }
        let resources = NativeWindowResources::with_capacity_limit(slots)?;
        state.window_resources = resources;
        state.window_resource_staging_reserved_bytes = staging;
        state.host_owner_external_reserved_bytes = external_reserved_bytes;
        Ok(())
    }

    /// One fixed conversion/settlement promise is paid before source capture.
    /// It is retained independently of queued snapshot occupancy.
    pub(crate) fn configure_snapshot_storage_allowance(
        &self,
        bytes: usize,
        windows: usize,
        work_areas: usize,
    ) -> bool {
        let mut state = self.lock();
        if state.retained_lifetime_count() > windows || state.journal.retained_record_count() != 0 {
            return false;
        }
        let previous = state.snapshot_storage_reserved_bytes;
        state.snapshot_storage_reserved_bytes = bytes;
        if !state.can_reserve_output_data()
            || !state.terminal_roster.configure_limits(windows, work_areas)
        {
            state.snapshot_storage_reserved_bytes = previous;
            return false;
        }
        true
    }

    /// Configures one combined host allowance with room for initial output progress.
    ///
    /// The bridge already promises pointer translation and its retry overlap.
    /// The caller promises other non-bridge data and constructor/terminal work
    /// in `external_reserved_bytes`; generic/default hosting remains unlimited.
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
    use crate::event::{NativeDeviceRemovalRecord, NativeWindowEventRecord};
    use crate::pointer_event::NativePointerTranslation;
    use winit::dpi::PhysicalPosition;
    use winit::event::{
        DeviceId, MouseScrollDelta, PointerEventFacts, Touch, TouchPhase, WindowEvent,
    };
    use winit::window::WindowId;

    #[test]
    fn lifecycle_installation_checks_shared_constructor_peak_before_replacing_any_owner() {
        let bridge = NativeHostBridge::new(NativeViewportMap::default());
        let baseline = bridge.owned_input_capacity_bytes(None);
        let pointer = bridge.lock().reserved_pointer_translation_capacity_bytes();
        let output = NativeOutputResources::ordinary_entry_data_bytes();
        let candidate = NativeHostBridge::lifecycle_storage_bytes(2, 1).unwrap();
        let required = baseline + pointer + candidate + output;
        assert!(bridge.configure_input_storage_allowance(required - 1, 0));
        assert_eq!(
            bridge.configure_lifecycle_storage(2, 1),
            Err(NativeHostProtocolError::NativeReportStorageUnavailable),
        );
        assert_eq!(bridge.owned_input_capacity_bytes(None), baseline);
        {
            let state = bridge.lock();
            assert!(state.retirements.can_configure_capacity_limit());
            assert!(state.close_control.can_configure_capacity_limit());
            assert!(state.viewports.can_configure_capacity_limit());
            assert_eq!(state.journal.captured_through(), 0);
        }
        assert!(bridge.configure_input_storage_allowance(required, 0));
        bridge.configure_lifecycle_storage(2, 1).unwrap();
        let inline = size_of::<NativeRetirementState>()
            + size_of::<NativeCloseControl>()
            + size_of::<NativeViewportMap>();
        assert_eq!(
            bridge.owned_input_capacity_bytes(None),
            baseline + candidate - inline
        );
        assert_eq!(
            bridge.configure_lifecycle_storage(2, 1),
            Err(NativeHostProtocolError::NativeReportStorageUnavailable),
        );
    }

    #[test]
    fn resource_and_external_owner_storage_share_output_headroom_before_initial_installation() {
        let bridge = NativeHostBridge::new(NativeViewportMap::default());
        let baseline = bridge.owned_input_capacity_bytes(None);
        let pointer = bridge.lock().reserved_pointer_translation_capacity_bytes();
        let output = NativeOutputResources::ordinary_entry_data_bytes();
        let external = 1234;
        let resources = NativeHostBridge::window_resource_storage_bytes(1).unwrap()
            - size_of::<NativeWindowResources>();
        let staging = NativeHostBridge::window_resource_staging_bytes(1).unwrap();
        let required = baseline + pointer + output + resources + staging + external;
        assert!(bridge.configure_input_storage_allowance(required - 1, 0));
        assert_eq!(
            bridge.configure_window_resource_storage(1, external),
            Err(NativeHostProtocolError::NativeReportStorageUnavailable)
        );
        assert_eq!(bridge.owned_input_capacity_bytes(None), baseline);
        assert_eq!(bridge.lock().window_resource_staging_reserved_bytes, 0);
        assert_eq!(bridge.lock().host_owner_external_reserved_bytes, 0);
        assert!(bridge.configure_input_storage_allowance(required, 0));
        bridge
            .configure_window_resource_storage(1, external)
            .unwrap();
        assert_eq!(
            bridge.owned_input_capacity_bytes(None),
            baseline + resources
        );
        let state = bridge.lock();
        assert_eq!(state.window_resource_staging_reserved_bytes, staging);
        assert_eq!(state.host_owner_external_reserved_bytes, external);
        assert!(state.can_reserve_input_data(output));
        assert!(!state.can_reserve_input_data(output + 1));
    }

    fn scroll(ordinal: u64, phase: TouchPhase) -> NativeWindowEventRecord {
        NativeWindowEventRecord::for_test(
            ordinal,
            WindowId::from(1),
            None,
            None,
            WindowEvent::MouseWheel {
                device_id: DeviceId::dummy(),
                delta: MouseScrollDelta::LineDelta(0.0, 1.0),
                phase,
                facts: PointerEventFacts::default(),
            },
        )
    }

    #[test]
    fn pointer_growth_cannot_spend_the_output_headroom_needed_after_a_retained_scroll_tail() {
        let bridge = NativeHostBridge::new(NativeViewportMap::default());
        let baseline = bridge.owned_input_capacity_bytes(None);
        let reserved = bridge.lock().reserved_pointer_translation_capacity_bytes();
        let growth = NativePointerTranslator::callback_translation_capacity_bytes();
        let output = NativeOutputResources::ordinary_entry_data_bytes();
        assert!(
            bridge.configure_input_storage_allowance(baseline + reserved + output.max(growth), 0)
        );
        assert!(
            !bridge
                .lock()
                .capture_window_event(scroll(1, TouchPhase::Started))
        );
        assert_eq!(bridge.lock().journal.captured_through(), 0);
        assert!(bridge.lock().journal.is_empty());
        assert!(bridge.lock().can_reserve_output_data());
    }

    #[test]
    fn scroll_tombstone_keeps_its_charge_and_recovery_reopens_output_admission() {
        use super::super::admission::{NativeAdmission, NativeDispatch};

        let growth = NativePointerTranslator::callback_translation_capacity_bytes();
        for next_callback_allowance in [0, growth] {
            let bridge = NativeHostBridge::new(NativeViewportMap::default());
            let mut translator = NativePointerTranslator::default();
            let baseline = bridge.owned_input_capacity_bytes(None);
            let limit = baseline
                + translator.reserved_translation_capacity_bytes()
                + NativeOutputResources::ordinary_entry_data_bytes()
                + growth
                + next_callback_allowance;
            assert!(bridge.configure_input_storage_allowance(limit, 0));
            assert!(
                bridge
                    .lock()
                    .capture_window_event(scroll(1, TouchPhase::Started))
            );
            if next_callback_allowance == 0 {
                assert!(
                    !bridge
                        .lock()
                        .capture_window_event(scroll(2, TouchPhase::Moved))
                );
            }
            let boundary = bridge.freeze_ordinary_for_test();
            assert_eq!(boundary, 1);
            bridge.commit_frame_boundary(false);
            let record = bridge.front_event().unwrap();
            let mut candidate = translator.clone();
            let first_translation = candidate.translate(&record, |_| None);
            assert!(matches!(
                first_translation,
                NativePointerTranslation::Input(_)
            ));
            assert!(bridge.acknowledge_translated_event(
                record.ordinal(),
                &mut translator,
                candidate
            ));
            bridge.commit_frame_boundary(false);
            assert_eq!(
                bridge.admission_dispatch(),
                NativeDispatch::Cancel { boundary }
            );
            assert!(bridge.finish_overload_cancellation(boundary));
            translator.finish_overload_cancellation();
            bridge.release_pointer_translation_capacity(&translator);
            bridge.commit_frame_boundary(false);
            assert!(translator.owned_data_capacity_bytes() > size_of::<NativePointerTranslator>());
            assert!(bridge.lock().can_reserve_output_data());
            assert_eq!(bridge.admission_dispatch(), NativeDispatch::Idle);
            assert_eq!(bridge.lock().admission, NativeAdmission::Running);
            assert!(
                baseline + bridge.lock().reserved_pointer_translation_capacity_bytes() <= limit
            );

            if next_callback_allowance != 0 {
                for (ordinal, phase) in [(3, TouchPhase::Moved), (4, TouchPhase::Started)] {
                    assert!(bridge.lock().capture_window_event(scroll(ordinal, phase)));
                    let mut candidate = translator.clone();
                    let translated = candidate.translate(&bridge.front_event().unwrap(), |_| None);
                    if phase == TouchPhase::Moved {
                        assert_eq!(translated, NativePointerTranslation::Ignored);
                    } else {
                        assert!(matches!(translated, NativePointerTranslation::Input(_)));
                        assert_ne!(translated, first_translation);
                    }
                    assert!(bridge.acknowledge_translated_event(
                        ordinal,
                        &mut translator,
                        candidate
                    ));
                    bridge.commit_frame_boundary(false);
                    assert!(bridge.lock().can_reserve_output_data());
                }
            }
        }
    }

    #[test]
    fn accepted_sources_prepay_translation_retry_overlap_and_rejection_keeps_the_first_boundary() {
        let bridge = NativeHostBridge::new(NativeViewportMap::default());
        let mut translator = NativePointerTranslator::default();
        let baseline = bridge.owned_input_capacity_bytes(None);
        let growth = NativePointerTranslator::callback_translation_capacity_bytes();
        let limit = baseline
            + translator.reserved_translation_capacity_bytes()
            + NativeOutputResources::ordinary_entry_data_bytes()
            + 5 * growth;
        assert!(bridge.configure_input_storage_allowance(limit, 0));
        let touch = |ordinal| {
            NativeWindowEventRecord::for_test(
                ordinal,
                WindowId::from(1),
                None,
                None,
                WindowEvent::Touch(Touch {
                    device_id: DeviceId::dummy(),
                    phase: TouchPhase::Started,
                    location: PhysicalPosition::new(1.0, 2.0),
                    force: None,
                    id: ordinal,
                }),
            )
        };
        let accepted = {
            let mut state = bridge.lock();
            let accepted = (1..=256)
                .take_while(|ordinal| state.capture_window_event(touch(*ordinal)))
                .count();
            assert!((1..256).contains(&accepted));
            assert_eq!(state.journal.captured_through(), accepted as u64);
            assert_eq!(
                state.reserved_pointer_translation_capacity_bytes(),
                translator.reserved_translation_capacity_bytes() + accepted * growth,
            );
            assert!(baseline + state.reserved_pointer_translation_capacity_bytes() <= limit);
            assert_eq!(
                state.admission,
                super::super::admission::NativeAdmission::Draining {
                    boundary: accepted as u64,
                }
            );
            assert!(!state.capture_window_event(touch(999)));
            assert_eq!(state.journal.captured_through(), accepted as u64);
            accepted
        };

        // A rejected core transfer drops only its retry candidate. Neither the
        // exact raw source nor the original translator returns its reservation.
        let before = bridge.lock().reserved_pointer_translation_capacity_bytes();
        let first = bridge.front_event().unwrap();
        let mut failed = translator.clone();
        assert!(matches!(
            failed.translate(&first, |_| None),
            NativePointerTranslation::Input(_)
        ));
        drop(failed);
        assert_eq!(bridge.front_event().unwrap().ordinal(), first.ordinal());
        assert_eq!(
            bridge.lock().reserved_pointer_translation_capacity_bytes(),
            before
        );
        assert_eq!(
            translator.owned_data_capacity_bytes(),
            size_of::<NativePointerTranslator>()
        );

        for ordinal in 1..=accepted {
            let record = bridge.front_event().unwrap();
            assert_eq!(record.ordinal(), ordinal as u64);
            let mut candidate = translator.clone();
            assert!(matches!(
                candidate.translate(&record, |_| None),
                NativePointerTranslation::Input(_)
            ));
            let peak = translator.owned_data_capacity_bytes()
                + candidate.owned_data_capacity_bytes()
                - size_of::<NativePointerTranslator>();
            assert!(peak <= bridge.lock().reserved_pointer_translation_capacity_bytes());
            assert!(bridge.acknowledge_translated_event(
                record.ordinal(),
                &mut translator,
                candidate
            ));
            assert!(
                baseline + bridge.lock().reserved_pointer_translation_capacity_bytes() <= limit
            );
            bridge.commit_frame_boundary(false);
        }
        assert!(bridge.front_event().is_none());
        let installed = translator.reserved_translation_capacity_bytes();
        assert_eq!(
            bridge.lock().reserved_pointer_translation_capacity_bytes(),
            installed
        );
        translator.forget_removed_devices();
        bridge.release_pointer_translation_capacity(&translator);
        assert!(bridge.lock().reserved_pointer_translation_capacity_bytes() <= installed);
        assert_eq!(
            bridge.lock().reserved_pointer_translation_capacity_bytes(),
            translator.reserved_translation_capacity_bytes()
        );
        assert!(
            translator.owned_data_capacity_bytes() > size_of::<NativePointerTranslator>(),
            "device removal retains charged tombstone slots"
        );
    }

    #[test]
    fn host_allowance_reserves_output_progress_and_rejection_preserves_existing_configuration() {
        let bridge = NativeHostBridge::new(NativeViewportMap::default());
        let baseline = bridge.owned_input_capacity_bytes(None);
        let reserved = bridge.lock().reserved_pointer_translation_capacity_bytes();
        let output = NativeOutputResources::ordinary_entry_data_bytes();
        assert!(!bridge.configure_input_storage_allowance(baseline, 0));
        assert!(bridge.lock().input_storage_allowance.is_none());
        assert!(bridge.configure_input_storage_allowance(baseline + reserved + output, 0));
        assert!(bridge.lock().can_reserve_input_data(output));
        assert!(!bridge.lock().can_reserve_input_data(output + 1));
        assert!(
            !bridge.configure_input_storage_allowance(baseline + reserved + output, output + 1)
        );
        assert!(bridge.lock().can_reserve_input_data(output));
        assert_eq!(bridge.owned_input_capacity_bytes(None), baseline);
    }

    #[test]
    fn snapshot_promise_uses_the_same_allowance_and_failed_configuration_preserves_limits() {
        let bridge = NativeHostBridge::new(NativeViewportMap::default());
        let baseline = bridge.owned_input_capacity_bytes(None);
        let reserved = bridge.lock().reserved_pointer_translation_capacity_bytes();
        let output = NativeOutputResources::ordinary_entry_data_bytes();
        let snapshot =
            crate::coordinator::NativeSnapshotStorageLimit::required_bytes(2, 1).unwrap();
        assert!(
            bridge
                .configure_input_storage_allowance(baseline + reserved + output + snapshot - 1, 0)
        );
        assert!(!bridge.configure_snapshot_storage_allowance(snapshot, 2, 1));
        {
            let state = bridge.lock();
            assert_eq!(state.snapshot_storage_reserved_bytes, 0);
            assert_eq!(
                state.terminal_roster.window_capacity(),
                super::super::terminal_roster::DEFAULT_WINDOW_CAPACITY
            );
            assert_eq!(
                state.terminal_roster.work_area_capacity(),
                super::super::terminal_roster::DEFAULT_WORK_AREA_CAPACITY
            );
            assert!(state.can_reserve_output_data());
        }

        assert!(
            bridge.configure_input_storage_allowance(baseline + reserved + output + snapshot, 0)
        );
        assert!(bridge.configure_snapshot_storage_allowance(snapshot, 2, 1));
        let state = bridge.lock();
        assert_eq!(state.snapshot_storage_reserved_bytes, snapshot);
        assert_eq!(state.terminal_roster.window_capacity(), 2);
        assert_eq!(state.terminal_roster.work_area_capacity(), 1);
        assert!(state.can_reserve_input_data(output));
        assert!(!state.can_reserve_input_data(output + 1));
    }

    #[test]
    fn promised_external_work_reduces_the_same_host_allowance_before_growth() {
        let bridge = NativeHostBridge::new(NativeViewportMap::default());
        let baseline = bridge.owned_input_capacity_bytes(None);
        let reserved = bridge.lock().reserved_pointer_translation_capacity_bytes();
        let output = NativeOutputResources::ordinary_entry_data_bytes();
        assert!(bridge.configure_input_storage_allowance(baseline + reserved + 2 * output, output));
        assert!(bridge.lock().can_reserve_output_data());
        assert!(
            bridge.configure_input_storage_allowance(baseline + reserved + 2 * output, 2 * output)
        );
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
        assert!(bridge.configure_input_storage_allowance(baseline + reserved + 2 * output, output));
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

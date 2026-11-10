use dockspace::runtime::NativeMonotonicClock;

use super::*;
use crate::event::{NativeDeviceRemovalRecord, NativeWindowEventRecord};
use crate::mailbox::journal::HostIngressJournal;
use crate::mailbox::roster::FrozenWindowObservation;
use crate::mailbox::{HostRecord, NativeHostBridge};
use crate::pointer_event::NativePointerTranslator;

#[test]
fn constructor_checks_the_complete_two_payload_peak_before_allocating() {
    for (windows, areas) in [(usize::MAX, 0), (0, usize::MAX), (usize::MAX, usize::MAX)] {
        assert_eq!(TerminalRosterState::reservation_bytes(windows, areas), None);
        assert_eq!(
            TerminalRosterState::try_with_capacity(windows, areas).unwrap_err(),
            NativeHostProtocolError::NativeReportStorageUnavailable,
        );
    }
    // Each payload formula fits, but keeping both complete backings does not.
    let windows = usize::MAX / size_of::<FrozenWindowObservation>() / 2 + 1;
    assert!(NativeViewportRosterRecord::reservation_bytes(windows, 0).is_some());
    assert_eq!(TerminalRosterState::reservation_bytes(windows, 0), None);
    assert_eq!(
        TerminalRosterState::try_with_capacity(windows, 0).unwrap_err(),
        NativeHostProtocolError::NativeReportStorageUnavailable,
    );
}

#[test]
fn empty_terminal_owner_prepays_two_distinct_complete_payloads_and_inline_state() {
    for (windows, areas) in [(0, 0), (1, 1), (7, 3), (256, 64)] {
        let state = TerminalRosterState::try_with_capacity(windows, areas).unwrap();
        let payload_bytes = NativeViewportRosterRecord::reservation_bytes(windows, areas).unwrap();
        assert!(!Arc::ptr_eq(&state.backings[0], &state.backings[1]));
        for backing in &state.backings {
            assert_eq!(Arc::strong_count(backing), 1);
            assert_eq!(backing.window_capacity(), windows);
            assert_eq!(backing.owned_payload_capacity_bytes(), payload_bytes);
        }
        assert_eq!(state.reserved_payload_capacity_bytes(), 2 * payload_bytes);
        assert_eq!(
            TerminalRosterState::reservation_bytes(windows, areas),
            Some(size_of::<TerminalRosterState>() + state.reserved_payload_capacity_bytes()),
        );
        assert_eq!(state.window_capacity(), windows);
        assert_eq!(state.work_area_capacity(), areas);
        assert!(!state.has_pending());
        assert!(!state.needs_capture());
        assert!(state.ingress().is_none());
    }
}

#[test]
fn logical_limits_and_live_receipts_cannot_replace_or_release_initial_backing() {
    let mut journal = HostIngressJournal::try_new(NativeMonotonicClock::system()).unwrap();
    let mut state = TerminalRosterState::try_with_capacity(2, 2).unwrap();
    let bytes = state.reserved_payload_capacity_bytes();
    let identities = state.backings.each_ref().map(Arc::as_ptr);
    assert!(!state.configure_limits(3, 2));
    assert!(!state.configure_limits(2, 3));
    assert!(!state.configure_limits(0, 1));
    assert!(!state.configure_limits(1, 0));
    assert!(state.configure_limits(1, 1));
    assert_eq!(state.window_capacity(), 1);
    assert_eq!(state.work_area_capacity(), 1);
    assert_eq!(state.reserved_payload_capacity_bytes(), bytes);

    state.require();
    let baseline = state.capture_with(|_| true).unwrap();
    assert!(state.capture(|_| {
        journal.reserve(HostRecord::DeviceRemoval(
            NativeDeviceRemovalRecord::for_test(1),
        ))
    }));
    let ordinal = state.ingress().unwrap().ordinal();
    assert!(!state.configure_limits(1, 1));
    assert_eq!(state.ingress().unwrap().ordinal(), ordinal);
    assert_eq!(state.required_generation, 1);
    assert_eq!(state.settled_generation, 0);
    journal.transfer_reserved(state.take_ingress(ordinal).unwrap());
    assert!(state.settle(1, true));
    journal.commit_transferred();
    state.commit_transferred();
    assert!(!state.has_pending());
    assert!(!state.configure_limits(1, 1));
    assert!(state.owns_payload(&baseline));
    drop(baseline);
    assert!(state.configure_limits(1, 1));
    assert_eq!(state.backings.each_ref().map(Arc::as_ptr), identities);
    assert_eq!(state.reserved_payload_capacity_bytes(), bytes);
    assert_eq!(
        TerminalRosterState::reservation_bytes(2, 2),
        Some(size_of::<TerminalRosterState>() + bytes),
    );
}

#[test]
fn prepaid_rosters_and_pointer_backlog_match_the_complete_capacity_recount() {
    use std::time::Duration;
    use winit::dpi::PhysicalPosition;
    use winit::event::{DeviceId, PointerEventFacts, WindowEvent};
    use winit::window::WindowId;

    let bridge = NativeHostBridge::with_clock(
        NativeViewportMap::default(),
        NativeMonotonicClock::new(|| Duration::ZERO),
    );
    let baseline = bridge.owned_input_capacity_bytes(None);
    assert!(bridge.configure_input_storage_allowance(usize::MAX, 0));
    let payload = NativeWindowEventRecord::pointer_callback_capacity_bytes();
    let translation = NativePointerTranslator::callback_translation_capacity_bytes();
    let installed_translation = bridge.lock().reserved_pointer_translation_capacity_bytes();
    {
        let mut state = bridge.lock();
        for index in 0..2 {
            let backing = Arc::clone(&state.terminal_roster.backings[index]);
            state.invalidate_viewport_roster();
            assert!(state.record_viewport_roster(backing));
        }
        assert!(!state.journal.has_unprepaid_roster());
        assert!(state.journal.iter().all(|record| {
            matches!(record, HostRecord::ViewportRoster(envelope) if envelope.prepaid_payload)
        }));
        for ordinal in 1..=128 {
            let record = NativeWindowEventRecord::for_test_snapshot(
                ordinal,
                WindowId::from(1),
                None,
                WindowEvent::CursorMoved {
                    device_id: DeviceId::dummy(),
                    position: PhysicalPosition::new(ordinal as f64, 20.0),
                    facts: PointerEventFacts::default(),
                },
                &state.viewports,
            );
            assert!(state.capture_window_event(record));
        }
        assert_eq!(state.journal.raw_pointer_count(), 128);
        assert!(!state.journal.has_unprepaid_roster());
    }
    assert_eq!(
        bridge.owned_input_capacity_bytes(None),
        baseline + 128 * payload
    );
    assert_eq!(
        bridge.selected_storage_reserved_bytes(),
        Some(baseline + installed_translation + 128 * (payload + translation)),
    );
    {
        let mut state = bridge.lock();
        state.journal.transfer_front(2);
        assert_eq!(state.journal.raw_pointer_count(), 128);
        state.journal.transfer_front(128);
        assert_eq!(state.journal.raw_pointer_count(), 0);
        assert_eq!(state.journal.raw_pointer_payload_bytes(), 0);
        assert!(!state.journal.has_unprepaid_roster());
        state.journal.commit_transferred();
    }
    assert_eq!(bridge.owned_input_capacity_bytes(None), baseline);
    assert_eq!(
        bridge.selected_storage_reserved_bytes(),
        Some(baseline + installed_translation),
    );
}

use dockspace::runtime::NativeMonotonicClock;

use super::*;
use crate::event::NativeDeviceRemovalRecord;
use crate::mailbox::HostRecord;
use crate::mailbox::journal::HostIngressJournal;
use crate::mailbox::roster::FrozenWindowObservation;

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

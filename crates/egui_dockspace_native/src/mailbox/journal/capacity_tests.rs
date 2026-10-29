use super::*;
use crate::event::NativeDeviceRemovalRecord;

fn record() -> HostRecord {
    HostRecord::DeviceRemoval(NativeDeviceRemovalRecord::for_test(1))
}

#[test]
fn fallible_initialization_prepays_exact_raw_backing_without_minting_ingress() {
    let journal = HostIngressJournal::try_new(NativeMonotonicClock::system()).unwrap();
    assert_eq!(journal.queue.capacity(), RECORD_LIMIT);
    assert_eq!(
        HostIngressJournal::reservation_bytes(),
        Some(journal.owned_buffer_capacity_bytes()),
    );
    assert_eq!(journal.len(), 0);
    assert_eq!(journal.retained_record_count(), 0);
    assert_eq!(journal.captured_through(), 0);
    assert_eq!(journal.oldest_age(), None);
    assert!(journal.can_push_raw());
}

#[test]
fn actual_spare_stays_charged_and_cannot_expand_the_logical_raw_limit() {
    let mut journal = HostIngressJournal::try_new(NativeMonotonicClock::system()).unwrap();
    // Extra backing is a private accounting fixture, not constructor admission.
    journal.queue.try_reserve_exact(RECORD_LIMIT + 1).unwrap();
    let capacity = journal.queue.capacity();
    assert!(capacity > RECORD_LIMIT);
    let bytes = journal.owned_buffer_capacity_bytes();
    assert!(bytes > HostIngressJournal::reservation_bytes().unwrap());
    assert_eq!(
        bytes,
        size_of::<HostIngressJournal>() + capacity * size_of::<ReservedHostRecord>(),
    );
    for _ in 0..RECORD_LIMIT {
        assert!(journal.push_back(record()));
    }
    let ordinal = journal.captured_through();
    assert!(!journal.push_back(record()));
    assert_eq!(journal.captured_through(), ordinal);
    assert_eq!(journal.len(), RECORD_LIMIT);
    assert_eq!(journal.queue.capacity(), capacity);

    journal.transfer_front(RECORD_LIMIT);
    assert!(journal.is_empty());
    assert_eq!(journal.retained_record_count(), RECORD_LIMIT);
    assert_eq!(journal.owned_buffer_capacity_bytes(), bytes);
    journal.commit_transferred();
    assert_eq!(journal.retained_record_count(), 0);
    assert_eq!(journal.owned_buffer_capacity_bytes(), bytes);
    assert!(journal.push_back(record()));
    assert_eq!(journal.queue.capacity(), capacity);
    assert_eq!(journal.owned_buffer_capacity_bytes(), bytes);
}

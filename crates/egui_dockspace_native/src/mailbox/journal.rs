//! Callback identity and age retained across transfer into the core recorder.

use std::collections::VecDeque;
use std::mem::size_of;
use std::time::Duration;

use dockspace::runtime::NativeMonotonicClock;

use super::{HostRecord, admission::RECORD_LIMIT};

#[derive(Debug)]
pub(crate) struct ReservedHostRecord {
    ordinal: u64,
    captured_at: Duration,
    record: HostRecord,
}

impl ReservedHostRecord {
    pub(crate) const fn ordinal(&self) -> u64 {
        self.ordinal
    }

    pub(crate) const fn record(&self) -> &HostRecord {
        &self.record
    }

    pub(crate) fn window_event(&self) -> Option<&crate::event::NativeWindowEventRecord> {
        match &self.record {
            HostRecord::WindowEvent(event) => Some(event),
            _ => None,
        }
    }

    pub(crate) const fn captured_at(&self) -> Duration {
        self.captured_at
    }
}

/// Source reservations remain live until the exact core frame boundary commits.
///
/// Transferring a callback drops its raw payload, but must not make an accepted,
/// uncommitted prefix disappear from record or oldest-age admission accounting.
#[derive(Debug)]
pub(crate) struct HostIngressJournal {
    clock: NativeMonotonicClock,
    queue: VecDeque<ReservedHostRecord>,
    last_ordinal: u64,
    transferred_count: usize,
    transferred_oldest: Option<Duration>,
}

impl HostIngressJournal {
    pub(crate) fn new(clock: NativeMonotonicClock) -> Self {
        Self {
            clock,
            queue: VecDeque::with_capacity(RECORD_LIMIT),
            last_ordinal: 0,
            transferred_count: 0,
            transferred_oldest: None,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.queue.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub(super) fn can_push_raw(&self) -> bool {
        self.queue.len() < RECORD_LIMIT
    }

    pub(super) fn retained_record_count(&self) -> usize {
        self.queue.len().saturating_add(self.transferred_count)
    }

    /// Returns inline journal storage and the raw queue's backing capacity.
    ///
    /// Excludes payload descendants and records retained by external owners.
    /// Transferred records do not add a second charge.
    pub(super) fn owned_buffer_capacity_bytes(&self) -> usize {
        size_of::<Self>().saturating_add(
            self.queue
                .capacity()
                .saturating_mul(size_of::<ReservedHostRecord>()),
        )
    }

    pub(super) fn age_since(&self, captured_at: Duration) -> Duration {
        self.clock.now().saturating_sub(captured_at)
    }

    pub(super) fn oldest_age(&self) -> Option<Duration> {
        let oldest = match (self.queue.front(), self.transferred_oldest) {
            (Some(front), Some(transferred)) => front.captured_at.min(transferred),
            (Some(front), None) => front.captured_at,
            (None, Some(transferred)) => transferred,
            (None, None) => return None,
        };
        Some(self.clock.now().saturating_sub(oldest))
    }

    pub(crate) fn captured_through(&self) -> u64 {
        self.last_ordinal
    }

    pub(super) fn front_ordinal(&self) -> Option<u64> {
        self.queue.front().map(|entry| entry.ordinal)
    }

    /// Reserves identity and age even while an existing lifecycle owner holds
    /// the record behind its predecessor's terminal commit dependency.
    pub(crate) fn reserve(&mut self, record: HostRecord) -> ReservedHostRecord {
        self.last_ordinal = self
            .last_ordinal
            .checked_add(1)
            .expect("native callback journal identity exhausted");
        ReservedHostRecord {
            ordinal: self.last_ordinal,
            captured_at: self.clock.now(),
            record,
        }
    }

    /// Rejects an unaccepted raw record before minting its ingress identity.
    pub(super) fn push_back(&mut self, record: HostRecord) -> bool {
        if !self.can_push_raw() {
            return false;
        }
        let record = self.reserve(record);
        self.queue.push_back(record);
        true
    }

    pub(super) fn front(&self) -> Option<&HostRecord> {
        self.queue.front().map(|entry| &entry.record)
    }

    pub(super) fn front_reserved(&self) -> Option<&ReservedHostRecord> {
        self.queue.front()
    }

    pub(super) fn iter(&self) -> impl ExactSizeIterator<Item = &HostRecord> {
        self.queue.iter().map(|entry| &entry.record)
    }

    pub(super) fn reserved_iter(&self) -> impl ExactSizeIterator<Item = &ReservedHostRecord> {
        self.queue.iter()
    }

    /// Removes a superseded or already committed raw record.
    pub(super) fn pop_front(&mut self) -> Option<HostRecord> {
        self.queue.pop_front().map(|entry| entry.record)
    }

    /// Moves the exact raw prefix into the still-uncommitted core boundary.
    pub(super) fn transfer_front(&mut self, count: usize) {
        assert!(count <= self.queue.len(), "a claimed prefix remains queued");
        for _ in 0..count {
            let entry = self
                .queue
                .pop_front()
                .expect("a claimed prefix remains queued");
            self.transfer_reserved(entry);
        }
    }

    /// Transfers an existing owner's reservation without re-enqueueing or recapturing it.
    pub(super) fn transfer_reserved(&mut self, record: ReservedHostRecord) {
        self.account_transfer(record.captured_at);
    }

    fn account_transfer(&mut self, captured_at: Duration) {
        self.transferred_oldest = Some(
            self.transferred_oldest
                .map_or(captured_at, |oldest| oldest.min(captured_at)),
        );
        self.transferred_count = self.transferred_count.saturating_add(1);
    }

    pub(super) fn commit_transferred(&mut self) {
        self.transferred_count = 0;
        self.transferred_oldest = None;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::event::NativeDeviceRemovalRecord;

    fn journal() -> (HostIngressJournal, Arc<AtomicU64>) {
        let now = Arc::new(AtomicU64::new(0));
        let source = Arc::clone(&now);
        (
            HostIngressJournal::new(NativeMonotonicClock::new(move || {
                Duration::from_millis(source.load(Ordering::SeqCst))
            })),
            now,
        )
    }

    fn record(ordinal: u64) -> HostRecord {
        HostRecord::DeviceRemoval(NativeDeviceRemovalRecord::for_test(ordinal))
    }

    #[test]
    fn buffer_capacity_survives_removal_without_charging_parked_or_transferred_records() {
        let (mut journal, _) = journal();
        let inline = size_of::<HostIngressJournal>();
        let capacity = journal.queue.capacity();
        assert!(capacity >= RECORD_LIMIT);
        let expected = inline + capacity * size_of::<ReservedHostRecord>();
        assert_eq!(journal.owned_buffer_capacity_bytes(), expected);
        let parked = journal.reserve(record(1));
        assert_eq!(journal.queue.capacity(), capacity);
        assert_eq!(journal.owned_buffer_capacity_bytes(), expected);

        for ordinal in 2..=4 {
            assert!(journal.push_back(record(ordinal)));
        }
        assert_eq!(journal.queue.capacity(), capacity);
        assert_eq!(journal.owned_buffer_capacity_bytes(), expected);

        journal.pop_front().expect("the first raw record is queued");
        assert_eq!(journal.owned_buffer_capacity_bytes(), expected);
        journal.transfer_front(1);
        assert_eq!(journal.retained_record_count(), 2);
        assert_eq!(journal.owned_buffer_capacity_bytes(), expected);
        journal.transfer_reserved(parked);
        assert_eq!(journal.retained_record_count(), 3);
        assert_eq!(journal.owned_buffer_capacity_bytes(), expected);
        journal.transfer_front(1);
        assert!(journal.is_empty());
        assert_eq!(journal.owned_buffer_capacity_bytes(), expected);

        journal.commit_transferred();
        assert_eq!(journal.retained_record_count(), 0);
        assert_eq!(journal.owned_buffer_capacity_bytes(), expected);
        assert!(journal.push_back(record(5)));
        journal.queue.clear();
        assert!(journal.is_empty());
        assert_eq!(journal.queue.capacity(), capacity);
        assert_eq!(journal.owned_buffer_capacity_bytes(), expected);
    }

    #[test]
    fn raw_limit_rejects_without_minting_and_reuses_backing_after_pop_and_transfer() {
        let (mut journal, _) = journal();
        let capacity = journal.queue.capacity();
        let bytes = journal.owned_buffer_capacity_bytes();
        assert!(journal.can_push_raw());
        for ordinal in 1..=RECORD_LIMIT {
            assert!(journal.push_back(record(ordinal as u64)));
        }
        let backing = journal.queue.as_slices().0.as_ptr();
        let second_slot = journal.queue.get(1).unwrap() as *const ReservedHostRecord;
        let third_slot = journal.queue.get(2).unwrap() as *const ReservedHostRecord;
        assert_eq!(journal.len(), RECORD_LIMIT);
        assert!(!journal.can_push_raw());
        assert!(!journal.push_back(record(RECORD_LIMIT as u64 + 1)));
        assert_eq!(journal.captured_through(), RECORD_LIMIT as u64);
        assert_eq!(journal.front_ordinal(), Some(1));
        assert_eq!(journal.queue.back().unwrap().ordinal(), RECORD_LIMIT as u64);
        assert_eq!(journal.queue.capacity(), capacity);
        assert_eq!(journal.owned_buffer_capacity_bytes(), bytes);

        let parked = journal.reserve(record(RECORD_LIMIT as u64 + 1));
        assert_eq!(parked.ordinal(), RECORD_LIMIT as u64 + 1);
        assert_eq!(journal.len(), RECORD_LIMIT);
        assert!(!journal.can_push_raw());
        assert_eq!(journal.queue.capacity(), capacity);
        assert_eq!(journal.owned_buffer_capacity_bytes(), bytes);

        journal.pop_front().expect("a full raw queue has a head");
        assert!(journal.can_push_raw());
        assert_eq!(
            journal.front_reserved().unwrap() as *const ReservedHostRecord,
            second_slot
        );
        assert!(journal.push_back(record(RECORD_LIMIT as u64 + 2)));
        assert_eq!(
            journal.queue.back().unwrap() as *const ReservedHostRecord,
            backing.wrapping_add(RECORD_LIMIT % capacity)
        );
        assert_eq!(journal.queue.capacity(), capacity);

        journal.transfer_front(1);
        assert_eq!(journal.retained_record_count(), RECORD_LIMIT);
        assert!(journal.can_push_raw());
        assert_eq!(
            journal.front_reserved().unwrap() as *const ReservedHostRecord,
            third_slot
        );
        assert!(journal.push_back(record(RECORD_LIMIT as u64 + 3)));
        assert_eq!(
            journal.queue.back().unwrap() as *const ReservedHostRecord,
            backing.wrapping_add((RECORD_LIMIT + 1) % capacity)
        );
        journal.transfer_reserved(parked);
        journal.commit_transferred();
        assert_eq!(journal.len(), RECORD_LIMIT);
        assert!(!journal.can_push_raw());
        assert!(!journal.push_back(record(RECORD_LIMIT as u64 + 4)));
        assert_eq!(journal.captured_through(), RECORD_LIMIT as u64 + 3);
        assert_eq!(journal.queue.capacity(), capacity);
        assert_eq!(journal.owned_buffer_capacity_bytes(), bytes);
    }

    #[test]
    fn transferred_prefix_keeps_count_and_original_age_until_commit() {
        let (mut journal, now) = journal();
        journal.push_back(record(1));
        now.store(100, Ordering::SeqCst);
        journal.push_back(record(2));
        journal.transfer_front(1);
        assert_eq!(journal.retained_record_count(), 2);
        assert_eq!(journal.front_ordinal(), Some(2));
        now.store(250, Ordering::SeqCst);
        assert_eq!(journal.oldest_age(), Some(Duration::from_millis(250)));
        journal.transfer_front(1);
        assert!(journal.is_empty());
        assert_eq!(journal.retained_record_count(), 2);
        assert_eq!(journal.oldest_age(), Some(Duration::from_millis(250)));

        journal.commit_transferred();
        assert_eq!(journal.retained_record_count(), 0);
        assert_eq!(journal.oldest_age(), None);
        journal.push_back(record(3));
        assert_eq!(journal.front_ordinal(), Some(3));
        assert_eq!(journal.captured_through(), 3);
        assert_eq!(journal.oldest_age(), Some(Duration::ZERO));
    }

    #[test]
    fn discarded_front_does_not_renew_the_surviving_record_or_its_identity() {
        let (mut journal, now) = journal();
        journal.push_back(record(1));
        now.store(25, Ordering::SeqCst);
        journal.push_back(record(2));
        now.store(100, Ordering::SeqCst);
        journal.pop_front().unwrap();
        assert_eq!(journal.front_ordinal(), Some(2));
        assert_eq!(journal.oldest_age(), Some(Duration::from_millis(75)));
        journal.pop_front().unwrap();
        assert_eq!(journal.oldest_age(), None);
        journal.push_back(record(3));
        assert_eq!(journal.front_ordinal(), Some(3));
    }

    #[test]
    fn lifecycle_reservation_transfers_without_requeueing_or_renewing_identity_or_age() {
        let (mut journal, now) = journal();
        journal.push_back(record(1));
        now.store(10, Ordering::SeqCst);
        let waiting = journal.reserve(record(2));
        now.store(20, Ordering::SeqCst);
        journal.push_back(record(3));
        assert_eq!(journal.captured_through(), 3);
        journal.transfer_front(1);
        journal.commit_transferred();
        assert_eq!(journal.front_ordinal(), Some(3));

        // An existing lifecycle owner transfers its reservation directly after
        // its predecessor commits. The later raw callback remains queued.
        now.store(250, Ordering::SeqCst);
        assert_eq!(waiting.ordinal(), 2);
        assert_eq!(
            journal.age_since(waiting.captured_at()),
            Duration::from_millis(240)
        );
        journal.transfer_reserved(waiting);
        assert_eq!(journal.captured_through(), 3);
        assert_eq!(journal.front_ordinal(), Some(3));
        assert_eq!(journal.oldest_age(), Some(Duration::from_millis(240)));
        journal.commit_transferred();
        assert_eq!(journal.oldest_age(), Some(Duration::from_millis(230)));
    }

    #[test]
    fn owner_held_transfer_preserves_capture_and_pending_queue_until_commit() {
        let (mut journal, now) = journal();
        now.store(10, Ordering::SeqCst);
        let waiting = journal.reserve(record(71));
        assert_eq!(waiting.ordinal(), 1);
        assert_eq!(waiting.captured_at(), Duration::from_millis(10));
        assert!(
            matches!(waiting.record(), HostRecord::DeviceRemoval(record) if *record == NativeDeviceRemovalRecord::for_test(71))
        );

        now.store(20, Ordering::SeqCst);
        journal.push_back(record(72));
        now.store(30, Ordering::SeqCst);
        journal.push_back(record(73));
        assert_eq!(
            journal.front_reserved().map(ReservedHostRecord::ordinal),
            Some(2)
        );
        assert_eq!(journal.reserved_iter().len(), 2);

        now.store(250, Ordering::SeqCst);
        journal.transfer_reserved(waiting);
        assert_eq!(journal.captured_through(), 3);
        assert_eq!(journal.len(), 2);
        assert_eq!(journal.retained_record_count(), 3);
        assert_eq!(journal.oldest_age(), Some(Duration::from_millis(240)));
        assert_eq!(
            journal
                .reserved_iter()
                .map(ReservedHostRecord::ordinal)
                .collect::<Vec<_>>(),
            [2, 3]
        );
        assert_eq!(
            journal.front_reserved().unwrap().captured_at(),
            Duration::from_millis(20)
        );
        assert!(
            matches!(journal.front(), Some(HostRecord::DeviceRemoval(record)) if *record == NativeDeviceRemovalRecord::for_test(72))
        );

        // Until the complete boundary commits, the original transfer age keeps growing.
        now.store(300, Ordering::SeqCst);
        assert_eq!(journal.retained_record_count(), 3);
        assert_eq!(journal.oldest_age(), Some(Duration::from_millis(290)));
        journal.transfer_front(1);
        assert_eq!(journal.retained_record_count(), 3);
        assert_eq!(journal.front_ordinal(), Some(3));
        journal.commit_transferred();
        assert_eq!(journal.retained_record_count(), 1);
        assert_eq!(journal.oldest_age(), Some(Duration::from_millis(270)));
        assert_eq!(journal.captured_through(), 3);
        assert_eq!(
            journal
                .reserved_iter()
                .map(ReservedHostRecord::ordinal)
                .collect::<Vec<_>>(),
            [3]
        );
    }

    #[test]
    fn dropping_untransferred_owner_record_does_not_change_pending_accounting() {
        let (mut journal, now) = journal();
        let waiting = journal.reserve(record(71));
        now.store(20, Ordering::SeqCst);
        journal.push_back(record(72));
        now.store(250, Ordering::SeqCst);
        drop(waiting);
        assert_eq!(journal.captured_through(), 2);
        assert_eq!(
            journal.front_reserved().map(ReservedHostRecord::ordinal),
            Some(2)
        );
        assert_eq!(journal.retained_record_count(), 1);
        assert_eq!(journal.oldest_age(), Some(Duration::from_millis(230)));
        journal.commit_transferred();
        assert_eq!(journal.retained_record_count(), 1);
        assert_eq!(journal.oldest_age(), Some(Duration::from_millis(230)));
        journal.push_back(record(73));
        assert_eq!(
            journal
                .reserved_iter()
                .map(ReservedHostRecord::ordinal)
                .collect::<Vec<_>>(),
            [2, 3]
        );
    }
}

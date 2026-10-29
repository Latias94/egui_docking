//! One terminal roster reservation survives transfer until both settlement gates.

use std::sync::Arc;

use eframe::NativeViewportRoster;

use super::journal::ReservedHostRecord;
use super::roster::NativeViewportRosterRecord;
use crate::error::NativeHostProtocolError;
use crate::viewport_map::NativeViewportMap;

pub(super) const DEFAULT_WINDOW_CAPACITY: usize = 256;
pub(super) const DEFAULT_WORK_AREA_CAPACITY: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReceiptPhase {
    Received,
    Transferred,
    Committed,
}

#[derive(Debug)]
struct TerminalRosterReceipt {
    generation: u64,
    ingress: Option<ReservedHostRecord>,
    phase: ReceiptPhase,
    settlement: Option<bool>,
}

#[derive(Debug)]
pub(super) struct TerminalRosterState {
    required_generation: u64,
    settled_generation: u64,
    receipt: Option<TerminalRosterReceipt>,
    backings: [Arc<NativeViewportRosterRecord>; 2],
    window_slots: usize,
    work_area_slots: usize,
    window_capacity_exceeded: bool,
    work_area_capacity_exceeded: bool,
}

#[cfg(test)]
impl Default for TerminalRosterState {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_WINDOW_CAPACITY, DEFAULT_WORK_AREA_CAPACITY)
    }
}

impl TerminalRosterState {
    #[cfg(test)]
    pub(super) fn with_capacity(window_slots: usize, work_area_slots: usize) -> Self {
        Self::try_with_capacity(window_slots, work_area_slots)
            .expect("native test terminal roster backing is available")
    }

    /// Reserves both complete payloads before publishing either shared owner.
    /// Vector failures return an error; Arc allocation can still abort on OOM.
    pub(super) fn try_with_capacity(
        window_slots: usize,
        work_area_slots: usize,
    ) -> Result<Self, NativeHostProtocolError> {
        Self::reservation_bytes(window_slots, work_area_slots)
            .ok_or(NativeHostProtocolError::NativeReportStorageUnavailable)?;
        let first = NativeViewportRosterRecord::try_with_capacity(window_slots, work_area_slots)?;
        let second = NativeViewportRosterRecord::try_with_capacity(window_slots, work_area_slots)?;
        Ok(Self {
            required_generation: 0,
            settled_generation: 0,
            receipt: None,
            backings: [Arc::new(first), Arc::new(second)],
            window_slots,
            work_area_slots,
            window_capacity_exceeded: false,
            work_area_capacity_exceeded: false,
        })
    }

    /// Includes inline state and both complete payloads, excluding Arc metadata.
    pub(super) fn reservation_bytes(window_slots: usize, work_area_slots: usize) -> Option<usize> {
        size_of::<Self>().checked_add(
            NativeViewportRosterRecord::reservation_bytes(window_slots, work_area_slots)?
                .checked_mul(2)?,
        )
    }

    pub(super) fn window_capacity(&self) -> usize {
        self.window_slots.min(
            self.backings[0]
                .window_capacity()
                .min(self.backings[1].window_capacity()),
        )
    }

    pub(super) const fn work_area_capacity(&self) -> usize {
        self.work_area_slots
    }

    pub(super) fn configure_limits(&mut self, windows: usize, work_areas: usize) -> bool {
        if windows == 0
            || work_areas == 0
            || self.has_pending()
            || self
                .backings
                .iter()
                .any(|backing| Arc::strong_count(backing) != 1)
            || windows > self.window_capacity()
            || work_areas > self.work_area_slots
        {
            return false;
        }
        self.window_slots = windows;
        self.work_area_slots = work_areas;
        true
    }

    /// Charges each preallocated Record and its actual vector capacities once.
    /// Arc handles are inline owner state; Arc control blocks are not estimated.
    pub(super) fn reserved_payload_capacity_bytes(&self) -> usize {
        self.backings.iter().fold(0usize, |bytes, backing| {
            bytes.saturating_add(backing.owned_payload_capacity_bytes())
        })
    }

    pub(super) fn owns_payload(&self, payload: &Arc<NativeViewportRosterRecord>) -> bool {
        self.backings
            .iter()
            .any(|backing| Arc::ptr_eq(backing, payload))
    }

    pub(super) const fn window_capacity_exceeded(&self) -> bool {
        self.window_capacity_exceeded
    }

    pub(super) const fn work_area_capacity_exceeded(&self) -> bool {
        self.work_area_capacity_exceeded
    }

    pub(super) fn capture_roster(
        &mut self,
        roster: NativeViewportRoster<'_>,
        viewports: &NativeViewportMap,
    ) -> Option<Arc<NativeViewportRosterRecord>> {
        let (windows, areas) = (self.window_slots, self.work_area_slots);
        self.capture_with(|backing| backing.capture_into(roster, viewports, windows, areas))
    }

    pub(super) fn capture_ordinary_roster(
        &mut self,
        roster: NativeViewportRoster<'_>,
        viewports: &NativeViewportMap,
    ) -> Option<Arc<NativeViewportRosterRecord>> {
        let (windows, areas) = (self.window_slots, self.work_area_slots);
        self.capture_backing_with(|backing| backing.capture_into(roster, viewports, windows, areas))
    }

    fn capture_with(
        &mut self,
        fill: impl FnOnce(&mut NativeViewportRosterRecord) -> bool,
    ) -> Option<Arc<NativeViewportRosterRecord>> {
        if !self.needs_capture() {
            return None;
        }
        self.capture_backing_with(fill)
    }

    fn capture_backing_with(
        &mut self,
        fill: impl FnOnce(&mut NativeViewportRosterRecord) -> bool,
    ) -> Option<Arc<NativeViewportRosterRecord>> {
        let index = self
            .backings
            .iter_mut()
            .position(|backing| Arc::get_mut(backing).is_some())?;
        let backing = &mut self.backings[index];
        let payload = Arc::get_mut(backing)
            .expect("the exclusively owned backing remains private during capture");
        if !fill(payload) {
            self.window_capacity_exceeded = true;
            return None;
        }
        self.window_capacity_exceeded = false;
        self.work_area_capacity_exceeded = payload.work_area_capacity_exceeded();
        Some(Arc::clone(backing))
    }

    pub(super) fn require(&mut self) {
        self.required_generation = self
            .required_generation
            .checked_add(1)
            .expect("terminal native viewport roster generation exhausted");
    }

    pub(super) const fn has_pending(&self) -> bool {
        self.receipt.is_some() || self.settled_generation < self.required_generation
    }

    pub(super) const fn needs_capture(&self) -> bool {
        self.receipt.is_none() && self.settled_generation < self.required_generation
    }

    pub(super) fn capture(&mut self, reserve: impl FnOnce(u64) -> ReservedHostRecord) -> bool {
        if !self.needs_capture() {
            return false;
        }
        let generation = self.required_generation;
        let ingress = reserve(generation);
        self.receipt = Some(TerminalRosterReceipt {
            generation,
            ingress: Some(ingress),
            phase: ReceiptPhase::Received,
            settlement: None,
        });
        true
    }

    pub(super) fn ingress(&self) -> Option<&ReservedHostRecord> {
        self.receipt.as_ref()?.ingress.as_ref()
    }

    pub(super) fn take_ingress(&mut self, ordinal: u64) -> Option<ReservedHostRecord> {
        let receipt = self.receipt.as_mut()?;
        if receipt.phase != ReceiptPhase::Received || receipt.ingress.as_ref()?.ordinal() != ordinal
        {
            return None;
        }
        let ingress = receipt.ingress.take()?;
        receipt.phase = ReceiptPhase::Transferred;
        Some(ingress)
    }

    pub(super) const fn can_settle(&self, generation: u64) -> bool {
        match &self.receipt {
            Some(receipt) => {
                generation == receipt.generation
                    && matches!(
                        receipt.phase,
                        ReceiptPhase::Transferred | ReceiptPhase::Committed
                    )
                    && receipt.settlement.is_none()
            }
            None => false,
        }
    }

    pub(super) fn settle(&mut self, generation: u64, applied: bool) -> bool {
        if !self.can_settle(generation) {
            return false;
        }
        self.receipt
            .as_mut()
            .expect("the exact unsettled roster receipt was checked")
            .settlement = Some(applied);
        self.finish_if_ready();
        true
    }

    pub(super) fn commit_transferred(&mut self) {
        if let Some(receipt) = &mut self.receipt
            && receipt.phase == ReceiptPhase::Transferred
        {
            receipt.phase = ReceiptPhase::Committed;
        }
        self.finish_if_ready();
    }

    fn finish_if_ready(&mut self) {
        let Some(receipt) = &self.receipt else {
            return;
        };
        if receipt.phase != ReceiptPhase::Committed {
            return;
        }
        let Some(applied) = receipt.settlement else {
            return;
        };
        if applied {
            self.settled_generation = receipt.generation;
        }
        // Rejection retries the outstanding requirement; it does not mint one.
        self.receipt = None;
    }
}

#[cfg(test)]
mod capacity_tests;

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use dockspace::runtime::NativeMonotonicClock;

    use super::*;
    use crate::event::NativeDeviceRemovalRecord;
    use crate::mailbox::{HostRecord, NativeHostBridge, journal::HostIngressJournal};

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

    // These records exercise only the private reservation protocol, not roster
    // payload capture or the coordinator's exact snapshot-envelope validation.
    fn protocol_record(generation: u64) -> HostRecord {
        HostRecord::DeviceRemoval(NativeDeviceRemovalRecord::for_test(generation))
    }

    // Empty private payloads qualify allocation/receipt ownership only. Real
    // borrowed fork rosters and OS window snapshots are not fabricated here.
    #[test]
    fn two_backings_wait_for_both_barriers_and_external_baseline_references() {
        let (mut journal, _) = journal();
        let mut state = TerminalRosterState::with_capacity(2, 1);
        let reserved = state.reserved_payload_capacity_bytes();
        assert_eq!(state.window_capacity(), 2);
        assert!(reserved >= 2 * std::mem::size_of::<NativeViewportRosterRecord>());

        state.require();
        let baseline = state.capture_with(|_| true).unwrap();
        assert!(state.owns_payload(&baseline));
        let first_identity = Arc::as_ptr(&baseline);
        let first = super::super::NativeViewportRosterEnvelope::from_shared(
            0,
            Arc::clone(&baseline),
            true,
            Some(1),
        );
        assert!(std::ptr::eq(first.roster(), baseline.as_ref()));
        assert!(state.capture(|_| journal.reserve(HostRecord::ViewportRoster(first))));
        let ordinal = state.ingress().unwrap().ordinal();
        journal.transfer_reserved(state.take_ingress(ordinal).unwrap());
        assert!(state.settle(1, true));
        assert!(
            state
                .capture_with(|_| panic!("settlement alone retains the receipt"))
                .is_none()
        );
        journal.commit_transferred();
        state.commit_transferred();

        state.require();
        let settling = state.capture_with(|_| true).unwrap();
        let second_identity = Arc::as_ptr(&settling);
        assert_ne!(first_identity, second_identity);
        let second = super::super::NativeViewportRosterEnvelope::from_shared(
            0,
            Arc::clone(&settling),
            true,
            Some(2),
        );
        assert!(state.capture(|_| journal.reserve(HostRecord::ViewportRoster(second))));
        let ordinal = state.ingress().unwrap().ordinal();
        journal.transfer_reserved(state.take_ingress(ordinal).unwrap());
        journal.commit_transferred();
        state.commit_transferred();
        assert!(
            state
                .capture_with(|_| panic!("full commit alone retains the receipt"))
                .is_none()
        );
        assert!(state.settle(2, false));
        assert!(state.needs_capture());
        let captured_through = journal.captured_through();
        assert!(
            state
                .capture_with(|_| panic!("both shared backings remain immutable"))
                .is_none()
        );
        assert!(state.needs_capture());
        assert_eq!(journal.captured_through(), captured_through);
        assert_eq!(state.reserved_payload_capacity_bytes(), reserved);

        drop(baseline);
        let reused = state.capture_with(|_| true).unwrap();
        assert_eq!(Arc::as_ptr(&reused), first_identity);
        assert_eq!(Arc::as_ptr(&settling), second_identity);
        assert_eq!(state.reserved_payload_capacity_bytes(), reserved);
        assert!(!state.window_capacity_exceeded());
        assert!(!state.work_area_capacity_exceeded());
    }

    #[test]
    fn failed_payload_fill_preserves_the_capture_obligation_without_reserving_ingress() {
        let (journal, _) = journal();
        let mut state = TerminalRosterState::with_capacity(2, 1);
        let reserved = state.reserved_payload_capacity_bytes();
        state.require();
        assert!(state.capture_with(|_| false).is_none());
        assert!(state.window_capacity_exceeded());
        assert!(state.needs_capture());
        assert!(state.ingress().is_none());
        assert_eq!(journal.captured_through(), 0);
        assert!(state.capture_with(|_| true).is_some());
        assert!(!state.window_capacity_exceeded());
        assert_eq!(state.reserved_payload_capacity_bytes(), reserved);
    }

    #[test]
    fn ordinary_snapshot_backpressure_recaptures_without_input_overload() {
        let bridge = NativeHostBridge::with_clock(
            NativeViewportMap::default(),
            NativeMonotonicClock::new(|| Duration::ZERO),
        );
        let first_identity = {
            let mut records = bridge.lock();
            records.terminal_roster = TerminalRosterState::with_capacity(2, 1);
            let payload = records
                .terminal_roster
                .capture_backing_with(|_| true)
                .unwrap();
            let identity = Arc::as_ptr(&payload);
            assert!(records.record_viewport_roster(payload));
            identity
        };
        let first = bridge.front_viewport_roster().unwrap();
        assert!(!first.is_terminal());
        assert!(bridge.acknowledge_viewport_roster(&first));
        bridge.commit_frame_boundary(false);
        assert!(bridge.settle_viewport_roster(&first, true));
        drop(first);

        let reserved = {
            let mut records = bridge.lock();
            records.invalidate_viewport_roster();
            let payload = records
                .terminal_roster
                .capture_backing_with(|_| true)
                .unwrap();
            assert_ne!(Arc::as_ptr(&payload), first_identity);
            assert!(records.record_viewport_roster(payload));
            let ordinal = records.journal.captured_through();
            assert!(
                records
                    .terminal_roster
                    .capture_backing_with(|_| panic!("both ordinary backings remain shared"))
                    .is_none()
            );
            for _ in 0..3 {
                records.defer_ordinary_roster_capture();
            }
            assert_eq!(records.journal.captured_through(), ordinal);
            assert_eq!(records.admission, crate::mailbox::NativeAdmission::Running);
            assert!(!records.event_boundary_pending);
            assert!(records.terminal_roster.needs_capture());
            assert_eq!(records.terminal_roster.required_generation, 1);
            records.terminal_roster.reserved_payload_capacity_bytes()
        };

        // The already accepted snapshot settles without a newer root capture.
        // Its baseline replacement releases the old backing for the obligation.
        bridge.commit_frame_boundary(false);
        let second = bridge.front_viewport_roster().unwrap();
        assert!(!second.is_terminal());
        assert!(bridge.acknowledge_viewport_roster(&second));
        bridge.commit_frame_boundary(false);
        assert!(bridge.settle_viewport_roster(&second, true));
        drop(second);
        {
            let mut records = bridge.lock();
            let recaptured = records.terminal_roster.capture_with(|_| true).unwrap();
            assert_eq!(Arc::as_ptr(&recaptured), first_identity);
            assert!(records.record_terminal_viewport_roster(recaptured));
            assert!(!records.terminal_roster.needs_capture());
            assert!(records.terminal_roster.has_pending());
            records.defer_ordinary_roster_capture();
            assert_eq!(records.terminal_roster.required_generation, 1);
        }
        let recaptured = bridge.front_viewport_roster().unwrap();
        assert!(recaptured.is_terminal());
        assert!(bridge.acknowledge_viewport_roster(&recaptured));
        assert!(bridge.settle_viewport_roster(&recaptured, true));
        assert!(bridge.lock().terminal_roster.has_pending());
        bridge.commit_frame_boundary(false);
        let records = bridge.lock();
        assert!(!records.terminal_roster.has_pending());
        assert_eq!(records.admission, crate::mailbox::NativeAdmission::Running);
        assert_eq!(
            records.terminal_roster.reserved_payload_capacity_bytes(),
            reserved
        );
    }

    #[test]
    fn ordinary_roster_window_capacity_exhaustion_still_freezes_ingress() {
        let bridge = NativeHostBridge::with_clock(
            NativeViewportMap::default(),
            NativeMonotonicClock::new(|| Duration::ZERO),
        );
        let mut records = bridge.lock();
        assert!(
            records
                .terminal_roster
                .capture_backing_with(|_| false)
                .is_none()
        );
        records.defer_ordinary_roster_capture();
        assert_eq!(
            records.admission,
            crate::mailbox::NativeAdmission::Draining { boundary: 0 },
        );
        assert!(records.event_boundary_pending);
        assert!(records.terminal_roster.needs_capture());
    }

    #[test]
    fn requirements_coalesce_without_replacing_an_occupied_receipt() {
        let (mut journal, _) = journal();
        let mut state = TerminalRosterState::default();
        assert!(!state.has_pending());
        assert!(!state.capture(|_| panic!("no requirement reserves ingress")));

        state.require();
        state.require();
        assert!(state.capture(|generation| {
            assert_eq!(generation, 2);
            journal.reserve(protocol_record(generation))
        }));
        let ordinal = state.ingress().unwrap().ordinal();
        state.require();
        state.require();
        assert!(!state.needs_capture());
        assert!(!state.capture(|_| panic!("the occupied slot cannot reserve again")));
        assert_eq!(journal.captured_through(), ordinal);
        assert_eq!(journal.len(), 0);
        assert_eq!(state.ingress().unwrap().ordinal(), ordinal);

        journal.transfer_reserved(state.take_ingress(ordinal).unwrap());
        assert!(state.settle(2, true));
        assert!(state.has_pending());
        assert!(!state.needs_capture());
        journal.commit_transferred();
        state.commit_transferred();
        assert!(state.has_pending());
        assert!(state.needs_capture());
        assert!(!state.can_settle(2));
        assert!(state.capture(|generation| {
            assert_eq!(generation, 4);
            journal.reserve(protocol_record(generation))
        }));
        assert_eq!(state.ingress().unwrap().ordinal(), ordinal + 1);
    }

    #[test]
    fn slot_waits_for_full_commit_and_exact_settlement_in_either_order() {
        for settlement_first in [false, true] {
            for applied in [false, true] {
                let (mut journal, _) = journal();
                let mut state = TerminalRosterState::default();
                state.require();
                assert!(state.capture(|generation| {
                    assert_eq!(generation, 1);
                    journal.reserve(protocol_record(generation))
                }));
                let ordinal = state.ingress().unwrap().ordinal();
                journal.transfer_reserved(state.take_ingress(ordinal).unwrap());
                assert!(state.ingress().is_none());
                assert!(state.take_ingress(ordinal).is_none());
                assert_eq!(journal.retained_record_count(), 1);

                if settlement_first {
                    assert!(state.settle(1, applied));
                    assert!(!state.settle(1, applied));
                } else {
                    journal.commit_transferred();
                    state.commit_transferred();
                    state.commit_transferred();
                }
                assert!(state.has_pending());
                assert!(!state.needs_capture());
                assert!(!state.capture(|_| panic!("one barrier cannot release the slot")));

                if settlement_first {
                    journal.commit_transferred();
                    state.commit_transferred();
                } else {
                    assert!(state.settle(1, applied));
                }
                assert!(state.ingress().is_none());
                assert!(!state.can_settle(1));
                assert!(!state.settle(1, applied));
                assert_eq!(journal.retained_record_count(), 0);
                assert_eq!(state.has_pending(), !applied);
                assert_eq!(state.needs_capture(), !applied);
                if !applied {
                    assert!(state.capture(|generation| {
                        assert_eq!(generation, 1, "rejection reuses the required generation");
                        journal.reserve(protocol_record(generation))
                    }));
                    assert_eq!(state.ingress().unwrap().ordinal(), ordinal + 1);
                }
            }
        }
    }

    #[test]
    fn received_receipt_ignores_commits_and_rejects_early_or_foreign_settlement() {
        let (mut journal, _) = journal();
        let mut state = TerminalRosterState::default();
        state.require();
        assert!(state.capture(|generation| journal.reserve(protocol_record(generation))));
        let ordinal = state.ingress().unwrap().ordinal();

        journal.commit_transferred();
        state.commit_transferred();
        assert_eq!(state.ingress().unwrap().ordinal(), ordinal);
        assert!(!state.can_settle(1));
        assert!(!state.settle(1, true));
        assert!(state.take_ingress(ordinal + 1).is_none());
        assert_eq!(state.ingress().unwrap().ordinal(), ordinal);

        journal.transfer_reserved(state.take_ingress(ordinal).unwrap());
        for foreign in [0, 2, u64::MAX] {
            assert!(!state.can_settle(foreign));
            assert!(!state.settle(foreign, true));
        }
        assert!(state.can_settle(1));
        assert!(state.settle(1, true));
        assert!(state.has_pending());
        assert!(!state.needs_capture());
        journal.commit_transferred();
        state.commit_transferred();
        assert!(!state.has_pending());
    }

    #[test]
    fn transfer_preserves_original_reservation_age_until_full_commit() {
        let (mut journal, now) = journal();
        let mut state = TerminalRosterState::default();
        journal.push_back(protocol_record(0));
        now.store(10, Ordering::SeqCst);
        state.require();
        assert!(state.capture(|generation| journal.reserve(protocol_record(generation))));
        let ordinal = state.ingress().unwrap().ordinal();
        assert_eq!(
            state.ingress().unwrap().captured_at(),
            Duration::from_millis(10)
        );
        now.store(20, Ordering::SeqCst);
        journal.push_back(protocol_record(0));

        journal.transfer_front(1);
        journal.commit_transferred();
        state.commit_transferred();
        assert_eq!(state.ingress().unwrap().ordinal(), ordinal);
        assert_eq!(journal.front_ordinal(), Some(ordinal + 1));
        assert_eq!(journal.retained_record_count(), 1);

        now.store(250, Ordering::SeqCst);
        journal.transfer_reserved(state.take_ingress(ordinal).unwrap());
        assert_eq!(journal.retained_record_count(), 2);
        assert_eq!(journal.oldest_age(), Some(Duration::from_millis(240)));
        assert!(state.settle(1, false));
        assert!(state.has_pending());
        assert!(!state.needs_capture());
        assert!(!state.capture(|_| panic!("a partial boundary retains the receipt")));
        assert_eq!(journal.oldest_age(), Some(Duration::from_millis(240)));

        journal.commit_transferred();
        state.commit_transferred();
        assert!(state.has_pending());
        assert!(state.needs_capture());
        assert_eq!(journal.retained_record_count(), 1);
        assert_eq!(journal.oldest_age(), Some(Duration::from_millis(230)));
    }
}

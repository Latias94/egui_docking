//! One terminal roster reservation survives transfer until both settlement gates.

use super::journal::ReservedHostRecord;

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

#[derive(Debug, Default)]
pub(super) struct TerminalRosterState {
    required_generation: u64,
    settled_generation: u64,
    receipt: Option<TerminalRosterReceipt>,
}

impl TerminalRosterState {
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
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use dockspace::runtime::NativeMonotonicClock;

    use super::*;
    use crate::event::NativeDeviceRemovalRecord;
    use crate::mailbox::{HostRecord, journal::HostIngressJournal};

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

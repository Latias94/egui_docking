//! One admitted create attempt owns one success or failure through commit.

use eframe::{NativeViewportCreateAttempt, NativeViewportCreateFailureKind};
use winit::window::WindowId;

use crate::mailbox::journal::ReservedHostRecord;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CreateTerminal {
    Created(WindowId),
    Failed(NativeViewportCreateFailureKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReceiptState {
    Awaiting,
    Received(CreateTerminal),
    Transferred(CreateTerminal),
    Committed(CreateTerminal),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CreateAdmission {
    First,
    Duplicate,
    Conflict,
}

/// Only the route or its existing unmaterialized retirement stores this value.
#[derive(Debug)]
pub(crate) struct NativeCreateReceipt {
    attempt: Option<NativeViewportCreateAttempt>,
    state: ReceiptState,
    result_recorded: bool,
    terminal_ingress: Option<ReservedHostRecord>,
}

impl NativeCreateReceipt {
    pub(crate) const fn new(attempt: Option<NativeViewportCreateAttempt>) -> Self {
        Self {
            attempt,
            state: ReceiptState::Awaiting,
            result_recorded: false,
            terminal_ingress: None,
        }
    }

    pub(crate) const fn attempt(&self) -> Option<NativeViewportCreateAttempt> {
        self.attempt
    }

    pub(crate) fn is_awaiting(&self) -> bool {
        self.state == ReceiptState::Awaiting
    }

    pub(crate) fn receive(
        &mut self,
        attempt: Option<NativeViewportCreateAttempt>,
        terminal: CreateTerminal,
        reserve: impl FnOnce() -> ReservedHostRecord,
    ) -> CreateAdmission {
        if self.attempt != attempt {
            return CreateAdmission::Conflict;
        }
        match self.state {
            ReceiptState::Awaiting => {
                let ingress = reserve();
                self.terminal_ingress = Some(ingress);
                self.state = ReceiptState::Received(terminal);
                CreateAdmission::First
            }
            ReceiptState::Received(current)
            | ReceiptState::Transferred(current)
            | ReceiptState::Committed(current)
                if current == terminal =>
            {
                CreateAdmission::Duplicate
            }
            _ => CreateAdmission::Conflict,
        }
    }

    pub(crate) fn transfer(
        &mut self,
        attempt: Option<NativeViewportCreateAttempt>,
        terminal: CreateTerminal,
    ) -> bool {
        if self.attempt != attempt || self.state != ReceiptState::Received(terminal) {
            return false;
        }
        self.state = ReceiptState::Transferred(terminal);
        true
    }

    pub(crate) fn ingress(&self) -> Option<&ReservedHostRecord> {
        self.terminal_ingress.as_ref()
    }

    /// Payload handoff follows the exact terminal's existing transfer acknowledgement.
    pub(crate) fn take_ingress(&mut self, ordinal: u64) -> Option<ReservedHostRecord> {
        if !matches!(self.state, ReceiptState::Transferred(_))
            || self.terminal_ingress.as_ref()?.ordinal() != ordinal
        {
            return None;
        }
        self.terminal_ingress.take()
    }

    pub(crate) fn mark_failure_recorded(&mut self) {
        if matches!(
            self.state,
            ReceiptState::Transferred(CreateTerminal::Failed(_))
        ) {
            self.result_recorded = true;
        }
    }

    pub(crate) fn commit_failure(&mut self) {
        if self.result_recorded
            && self.terminal_ingress.is_none()
            && let ReceiptState::Transferred(terminal @ CreateTerminal::Failed(_)) = self.state
        {
            self.state = ReceiptState::Committed(terminal);
        }
    }

    pub(crate) fn commit_created_boundary(&mut self) {
        if self.terminal_ingress.is_none()
            && let ReceiptState::Transferred(terminal @ CreateTerminal::Created(_)) = self.state
        {
            self.state = ReceiptState::Committed(terminal);
        }
    }

    pub(crate) fn is_transferred_failure(&self) -> bool {
        matches!(
            self.state,
            ReceiptState::Transferred(CreateTerminal::Failed(_))
        )
    }

    pub(crate) fn is_committed(&self) -> bool {
        matches!(self.state, ReceiptState::Committed(_))
    }

    pub(crate) fn permits_output(&self, window: WindowId) -> bool {
        match self.state {
            ReceiptState::Awaiting => true,
            ReceiptState::Received(CreateTerminal::Created(current))
            | ReceiptState::Transferred(CreateTerminal::Created(current))
            | ReceiptState::Committed(CreateTerminal::Created(current)) => current == window,
            _ => false,
        }
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

    #[test]
    fn one_attempt_selects_one_terminal_and_raw_transfer_never_reopens_it() {
        let now = Arc::new(AtomicU64::new(10));
        let clock = Arc::clone(&now);
        let mut journal = HostIngressJournal::new(NativeMonotonicClock::new(move || {
            Duration::from_millis(clock.load(Ordering::SeqCst))
        }));
        let mut receipt = NativeCreateReceipt::new(None);
        let created = CreateTerminal::Created(WindowId::from(11));
        let failed = CreateTerminal::Failed(NativeViewportCreateFailureKind::WindowUnavailable);
        // This tokenless receipt model uses a real journal reservation without
        // fabricating an opaque renderer token or claiming Created callback coverage.
        assert_eq!(
            receipt.receive(None, created, || {
                journal.reserve(HostRecord::DeviceRemoval(
                    NativeDeviceRemovalRecord::for_test(1),
                ))
            }),
            CreateAdmission::First
        );
        let ordinal = receipt.ingress().unwrap().ordinal();
        assert_eq!(
            receipt.ingress().unwrap().captured_at(),
            Duration::from_millis(10)
        );
        now.store(20, Ordering::SeqCst);
        assert_eq!(
            receipt.receive(None, failed, || panic!(
                "a conflicting terminal cannot reserve ingress"
            )),
            CreateAdmission::Conflict
        );
        assert!(
            receipt.take_ingress(ordinal).is_none(),
            "handoff requires semantic transfer first"
        );
        assert!(!receipt.transfer(None, CreateTerminal::Created(WindowId::from(12))));
        assert!(receipt.transfer(None, created));
        assert!(!receipt.is_committed());
        assert_eq!(
            receipt.receive(None, created, || panic!(
                "a duplicate cannot reserve ingress"
            )),
            CreateAdmission::Duplicate
        );
        assert!(!receipt.transfer(None, created));
        assert!(receipt.take_ingress(ordinal + 1).is_none());
        receipt.commit_created_boundary();
        assert!(
            !receipt.is_committed(),
            "an unhanded payload blocks completion"
        );
        let ingress = receipt
            .take_ingress(ordinal)
            .expect("exact transferred ingress hands off");
        assert_eq!(ingress.ordinal(), ordinal);
        assert_eq!(ingress.captured_at(), Duration::from_millis(10));
        assert!(receipt.take_ingress(ordinal).is_none());
        assert!(receipt.ingress().is_none());
        receipt.commit_created_boundary();
        assert!(receipt.is_committed());
        assert_eq!(
            receipt.receive(None, created, || panic!(
                "a committed duplicate cannot reserve ingress"
            )),
            CreateAdmission::Duplicate
        );
        assert_eq!(journal.captured_through(), 1);
        assert!(!receipt.permits_output(WindowId::from(12)));
    }

    #[test]
    fn failure_keeps_the_same_receipt_until_its_recorded_result_commits() {
        let mut journal = HostIngressJournal::new(NativeMonotonicClock::new(|| Duration::ZERO));
        let mut receipt = NativeCreateReceipt::new(None);
        let failed = CreateTerminal::Failed(NativeViewportCreateFailureKind::WindowUnavailable);
        assert_eq!(
            receipt.receive(None, failed, || {
                journal.reserve(HostRecord::DeviceRemoval(
                    NativeDeviceRemovalRecord::for_test(1),
                ))
            }),
            CreateAdmission::First
        );
        let ordinal = receipt.ingress().unwrap().ordinal();
        receipt.commit_failure();
        assert!(!receipt.is_committed());
        assert!(receipt.transfer(None, failed));
        receipt.commit_failure();
        assert!(
            !receipt.is_committed(),
            "an unrelated boundary cannot consume an unrecorded result"
        );
        receipt.mark_failure_recorded();
        assert!(
            !receipt.is_committed(),
            "recorder handoff alone is not publication"
        );
        receipt.commit_failure();
        assert!(
            !receipt.is_committed(),
            "a recorded result still waits for payload handoff"
        );
        let mut moved = receipt;
        assert_eq!(moved.ingress().unwrap().ordinal(), ordinal);
        assert!(moved.take_ingress(ordinal + 1).is_none());
        let ingress = moved
            .take_ingress(ordinal)
            .expect("the moved receipt retains its exact ingress");
        assert_eq!(ingress.captured_at(), Duration::ZERO);
        assert!(moved.take_ingress(ordinal).is_none());
        assert_eq!(
            moved.receive(None, failed, || panic!(
                "a duplicate cannot reserve ingress"
            )),
            CreateAdmission::Duplicate
        );
        let mut receipt = moved;
        receipt.commit_failure();
        assert!(receipt.is_committed());
        assert_eq!(receipt.attempt(), None);
        assert_eq!(
            receipt.receive(None, failed, || panic!(
                "a committed duplicate cannot reserve ingress"
            )),
            CreateAdmission::Duplicate
        );
        assert_eq!(
            receipt.receive(None, CreateTerminal::Created(WindowId::from(12)), || {
                panic!("a conflicting terminal cannot reserve ingress")
            }),
            CreateAdmission::Conflict
        );
        assert_eq!(journal.captured_through(), 1);
    }
}

//! One admitted create attempt owns one success or failure through commit.

use eframe::{NativeViewportCreateAttempt, NativeViewportCreateFailureKind};
use winit::window::WindowId;

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
}

impl NativeCreateReceipt {
    pub(crate) const fn new(attempt: Option<NativeViewportCreateAttempt>) -> Self {
        Self {
            attempt,
            state: ReceiptState::Awaiting,
            result_recorded: false,
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
    ) -> CreateAdmission {
        if self.attempt != attempt {
            return CreateAdmission::Conflict;
        }
        match self.state {
            ReceiptState::Awaiting => {
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
            && let ReceiptState::Transferred(terminal @ CreateTerminal::Failed(_)) = self.state
        {
            self.state = ReceiptState::Committed(terminal);
        }
    }

    pub(crate) fn commit_created_boundary(&mut self) {
        if let ReceiptState::Transferred(terminal @ CreateTerminal::Created(_)) = self.state {
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
    use super::*;

    #[test]
    fn one_attempt_selects_one_terminal_and_raw_transfer_never_reopens_it() {
        let mut receipt = NativeCreateReceipt::new(None);
        let created = CreateTerminal::Created(WindowId::from(11));
        let failed = CreateTerminal::Failed(NativeViewportCreateFailureKind::WindowUnavailable);
        assert_eq!(receipt.receive(None, created), CreateAdmission::First);
        assert_eq!(receipt.receive(None, failed), CreateAdmission::Conflict);
        assert!(!receipt.transfer(None, CreateTerminal::Created(WindowId::from(12))));
        assert!(receipt.transfer(None, created));
        assert!(!receipt.is_committed());
        assert_eq!(receipt.receive(None, created), CreateAdmission::Duplicate);
        assert!(!receipt.transfer(None, created));
        receipt.commit_created_boundary();
        assert!(receipt.is_committed());
        assert_eq!(receipt.receive(None, created), CreateAdmission::Duplicate);
        assert!(!receipt.permits_output(WindowId::from(12)));
    }

    #[test]
    fn failure_keeps_the_same_receipt_until_its_recorded_result_commits() {
        let mut receipt = NativeCreateReceipt::new(None);
        let failed = CreateTerminal::Failed(NativeViewportCreateFailureKind::WindowUnavailable);
        assert_eq!(receipt.receive(None, failed), CreateAdmission::First);
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
        assert_eq!(receipt.receive(None, failed), CreateAdmission::Duplicate);
        receipt.commit_failure();
        assert!(receipt.is_committed());
        assert_eq!(receipt.attempt(), None);
        assert_eq!(receipt.receive(None, failed), CreateAdmission::Duplicate);
        assert_eq!(
            receipt.receive(None, CreateTerminal::Created(WindowId::from(12))),
            CreateAdmission::Conflict
        );
    }
}

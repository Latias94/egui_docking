//! Sealed renderer-pass settlement shared by proof-strength adapters.
//!
//! The bridge owns staged artifacts. Callers only carry affine pass bindings
//! and terminal receipts, so a late or foreign proof cannot replace the
//! current artifact for a viewport. Production adapters keep those authorities
//! private; deterministic tests mint all three terminal dispositions directly.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};

use egui::ViewportId;

const TOMBSTONE_LIMIT: usize = 64;

static NEXT_ISSUER: AtomicU64 = AtomicU64::new(1);

/// One private artifact lane per viewport.
pub(super) struct RendererPassBridge<A> {
    issuer: u64,
    next_serial: u64,
    latest: HashMap<ViewportId, PassKey>,
    staged: HashMap<PassKey, A>,
    tombstones: VecDeque<PassKey>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PassKey {
    issuer: u64,
    viewport: ViewportId,
    serial: u64,
}

/// Affine authority to stage one pass artifact.
#[derive(Debug)]
pub(super) struct OpenPassBinding {
    key: PassKey,
}

/// Affine deterministic host adapter used to mint one terminal receipt.
#[derive(Debug)]
pub(super) struct RendererPassReceiptIssuer {
    key: PassKey,
}

/// Terminal proof for one exact bridge, viewport, and pass serial.
#[derive(Debug)]
pub(super) struct RendererPassReceipt {
    key: PassKey,
    disposition: OutputPassDisposition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OutputPassDisposition {
    Presented,
    Discarded,
    Unwind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BridgeOpenError {
    SerialExhausted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BridgeStageReason {
    ForeignBinding,
    Superseded,
    Terminalized,
}

#[derive(Debug)]
pub(super) struct BridgeStageError<A> {
    pub(super) artifact: A,
    pub(super) reason: BridgeStageReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BridgeSettlementReason {
    ForeignReceipt,
    Superseded,
    Terminalized,
    NotStaged,
}

#[derive(Debug)]
pub(super) struct BridgeSettlementError {
    pub(super) receipt: RendererPassReceipt,
    pub(super) reason: BridgeSettlementReason,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum RendererPassSettlement<A> {
    Presented(A),
    Discarded(A),
    Unwound(Option<A>),
}

impl<A> RendererPassBridge<A> {
    #[cfg(test)]
    pub(super) fn staged_len(&self) -> usize {
        self.staged.len()
    }

    pub(super) fn new() -> Self {
        let issuer = NEXT_ISSUER.fetch_add(1, Ordering::Relaxed);
        Self {
            issuer,
            next_serial: 0,
            latest: HashMap::new(),
            staged: HashMap::new(),
            tombstones: VecDeque::new(),
        }
    }

    pub(super) fn open_pass(
        &mut self,
        viewport: ViewportId,
    ) -> Result<(OpenPassBinding, RendererPassReceiptIssuer), BridgeOpenError> {
        let serial = self
            .next_serial
            .checked_add(1)
            .ok_or(BridgeOpenError::SerialExhausted)?;
        self.next_serial = serial;
        let key = PassKey {
            issuer: self.issuer,
            viewport,
            serial,
        };
        if let Some(previous) = self.latest.insert(viewport, key) {
            self.retire(previous);
        }
        Ok((OpenPassBinding { key }, RendererPassReceiptIssuer { key }))
    }

    pub(super) fn stage(
        &mut self,
        binding: OpenPassBinding,
        artifact: A,
    ) -> Result<(), BridgeStageError<A>> {
        let key = binding.key;
        let reason = if key.issuer != self.issuer {
            Some(BridgeStageReason::ForeignBinding)
        } else if self.tombstones.contains(&key) {
            Some(BridgeStageReason::Terminalized)
        } else if self.latest.get(&key.viewport) != Some(&key) {
            Some(BridgeStageReason::Superseded)
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(BridgeStageError { artifact, reason });
        }
        self.staged.insert(key, artifact);
        Ok(())
    }

    pub(super) fn settle(
        &mut self,
        receipt: RendererPassReceipt,
    ) -> Result<RendererPassSettlement<A>, BridgeSettlementError> {
        let key = receipt.key;
        if key.issuer != self.issuer {
            return Err(BridgeSettlementError {
                receipt,
                reason: BridgeSettlementReason::ForeignReceipt,
            });
        }
        if self.tombstones.contains(&key) {
            return Err(BridgeSettlementError {
                receipt,
                reason: BridgeSettlementReason::Terminalized,
            });
        }
        if self.latest.get(&key.viewport) != Some(&key) {
            return Err(BridgeSettlementError {
                receipt,
                reason: BridgeSettlementReason::Superseded,
            });
        }

        let artifact = self.staged.remove(&key);
        if artifact.is_none() && receipt.disposition != OutputPassDisposition::Unwind {
            self.remember(key);
            return Err(BridgeSettlementError {
                receipt,
                reason: BridgeSettlementReason::NotStaged,
            });
        }
        self.remember(key);
        Ok(match receipt.disposition {
            OutputPassDisposition::Presented => RendererPassSettlement::Presented(
                artifact.expect("presented passes require one staged artifact"),
            ),
            OutputPassDisposition::Discarded => RendererPassSettlement::Discarded(
                artifact.expect("discarded passes require one staged artifact"),
            ),
            OutputPassDisposition::Unwind => RendererPassSettlement::Unwound(artifact),
        })
    }

    fn retire(&mut self, key: PassKey) {
        self.staged.remove(&key);
        self.remember(key);
    }

    fn remember(&mut self, key: PassKey) {
        if self.tombstones.contains(&key) {
            return;
        }
        self.tombstones.push_back(key);
        while self.tombstones.len() > TOMBSTONE_LIMIT {
            self.tombstones.pop_front();
        }
    }

    #[cfg(test)]
    fn tombstone_len(&self) -> usize {
        self.tombstones.len()
    }
}

impl<A> Default for RendererPassBridge<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl RendererPassReceiptIssuer {
    pub(super) fn presented(self) -> RendererPassReceipt {
        self.receipt(OutputPassDisposition::Presented)
    }

    pub(super) fn discarded(self) -> RendererPassReceipt {
        self.receipt(OutputPassDisposition::Discarded)
    }

    pub(super) fn unwind(self) -> RendererPassReceipt {
        self.receipt(OutputPassDisposition::Unwind)
    }

    fn receipt(self, disposition: OutputPassDisposition) -> RendererPassReceipt {
        RendererPassReceipt {
            key: self.key,
            disposition,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presented_promotes_only_the_matching_staged_artifact() {
        let mut bridge = RendererPassBridge::new();
        let (binding, issuer) = bridge
            .open_pass(ViewportId::ROOT)
            .expect("pass should open");
        bridge
            .stage(binding, "artifact")
            .expect("artifact should stage");

        assert_eq!(
            bridge
                .settle(issuer.presented())
                .expect("proof should settle"),
            RendererPassSettlement::Presented("artifact")
        );
    }

    #[test]
    fn discarded_and_unwind_preserve_terminal_disposition() {
        let mut bridge = RendererPassBridge::new();
        let (binding, issuer) = bridge
            .open_pass(ViewportId::ROOT)
            .expect("pass should open");
        bridge.stage(binding, 1_u8).expect("artifact should stage");
        assert_eq!(
            bridge
                .settle(issuer.discarded())
                .expect("discard should settle"),
            RendererPassSettlement::Discarded(1)
        );

        let (binding, issuer) = bridge
            .open_pass(ViewportId::ROOT)
            .expect("pass should open");
        bridge.stage(binding, 2_u8).expect("artifact should stage");
        assert_eq!(
            bridge
                .settle(issuer.unwind())
                .expect("unwind should settle"),
            RendererPassSettlement::Unwound(Some(2))
        );
    }

    #[test]
    fn unwind_before_staging_terminalizes_the_exact_pass() {
        let mut bridge = RendererPassBridge::<()>::new();
        let (binding, issuer) = bridge
            .open_pass(ViewportId::ROOT)
            .expect("pass should open");
        assert_eq!(
            bridge
                .settle(issuer.unwind())
                .expect("unwind should create a tombstone"),
            RendererPassSettlement::Unwound(None)
        );
        let error = bridge
            .stage(binding, ())
            .expect_err("terminalized pass cannot stage later");
        assert_eq!(error.reason, BridgeStageReason::Terminalized);
    }

    #[test]
    fn late_predecessor_cannot_replace_a_newer_successor() {
        let mut bridge = RendererPassBridge::new();
        let (older, older_issuer) = bridge
            .open_pass(ViewportId::ROOT)
            .expect("pass should open");
        let (newer, newer_issuer) = bridge
            .open_pass(ViewportId::ROOT)
            .expect("pass should open");

        let error = bridge
            .stage(older, "old")
            .expect_err("predecessor must be retired");
        assert_eq!(error.reason, BridgeStageReason::Terminalized);
        assert_eq!(error.artifact, "old");
        let error = bridge
            .settle(older_issuer.presented())
            .expect_err("late predecessor proof must fail closed");
        assert_eq!(error.reason, BridgeSettlementReason::Terminalized);

        bridge.stage(newer, "new").expect("successor should stage");
        assert_eq!(
            bridge
                .settle(newer_issuer.presented())
                .expect("successor should promote"),
            RendererPassSettlement::Presented("new")
        );
    }

    #[test]
    fn foreign_binding_and_receipt_fail_closed() {
        let mut first = RendererPassBridge::<&str>::new();
        let mut second = RendererPassBridge::new();
        let (binding, _issuer) = first.open_pass(ViewportId::ROOT).expect("pass should open");
        let error = second
            .stage(binding, "foreign")
            .expect_err("foreign binding must fail closed");
        assert_eq!(error.reason, BridgeStageReason::ForeignBinding);
        assert_eq!(error.artifact, "foreign");

        let (first_binding, first_issuer) = first
            .open_pass(ViewportId::ROOT)
            .expect("first pass should open");
        first
            .stage(first_binding, "first")
            .expect("first artifact should stage");
        let (second_binding, second_issuer) = second
            .open_pass(ViewportId::ROOT)
            .expect("second pass should open");
        second
            .stage(second_binding, "second")
            .expect("second artifact should stage");

        let error = first
            .settle(second_issuer.presented())
            .expect_err("foreign receipt must fail closed");
        assert_eq!(error.reason, BridgeSettlementReason::ForeignReceipt);
        assert_eq!(
            second
                .settle(error.receipt)
                .expect("the issuing bridge should still accept the receipt"),
            RendererPassSettlement::Presented("second")
        );
        assert_eq!(
            first
                .settle(first_issuer.presented())
                .expect("foreign rejection must not disturb the first artifact"),
            RendererPassSettlement::Presented("first")
        );
    }

    #[test]
    fn presented_or_discarded_before_stage_fails_closed() {
        let mut bridge = RendererPassBridge::<()>::new();
        let (binding, issuer) = bridge
            .open_pass(ViewportId::ROOT)
            .expect("pass should open");
        let error = bridge
            .settle(issuer.presented())
            .expect_err("unstaged output cannot be presented");
        assert_eq!(error.reason, BridgeSettlementReason::NotStaged);
        let error = bridge
            .stage(binding, ())
            .expect_err("terminal receipt must retire the open binding");
        assert_eq!(error.reason, BridgeStageReason::Terminalized);

        let (binding, issuer) = bridge
            .open_pass(ViewportId::ROOT)
            .expect("pass should open");
        let error = bridge
            .settle(issuer.discarded())
            .expect_err("unstaged output cannot be discarded");
        assert_eq!(error.reason, BridgeSettlementReason::NotStaged);
        let error = bridge
            .stage(binding, ())
            .expect_err("terminal receipt must retire the open binding");
        assert_eq!(error.reason, BridgeStageReason::Terminalized);
    }

    #[test]
    fn tombstones_are_bounded() {
        let mut bridge = RendererPassBridge::<()>::new();
        for _ in 0..(TOMBSTONE_LIMIT * 4) {
            let (_binding, issuer) = bridge
                .open_pass(ViewportId::ROOT)
                .expect("pass should open");
            bridge
                .settle(issuer.unwind())
                .expect("unwind should settle");
        }
        assert!(bridge.tombstone_len() <= TOMBSTONE_LIMIT);
    }
}

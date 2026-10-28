//! Ordered native input replay and receiver settlement.

use super::receiver::resolve_receiver_observation;
use super::{
    NativeInputBudget, NativePlatformError, NativeReceiverAnswer, NativeReceiverQuery,
    RuntimeNativeState,
};
use crate::engine::{BackendIngressProgress, CoreHostFrame, DockEngine};
use crate::pointer_receiver::{PointerReceiverObservation, PointerReceiverReceiptBatch};
use crate::runtime::DockspaceRuntimeError;
use crate::runtime::presentation::SubmittedPresentationObservation;
use std::num::NonZeroUsize;

/// One unpublished candidate retained while its atomic group cannot seal.
#[derive(Debug)]
pub(in crate::runtime) struct PendingNativeReplay {
    pub(in crate::runtime) frame: CoreHostFrame,
    pub(in crate::runtime) submitted_presentation: SubmittedPresentationObservation,
    pub(in crate::runtime) prelude_pending: bool,
}

impl RuntimeNativeState {
    pub(in crate::runtime) fn overload_cancellation_input(
        &self,
        frame: &CoreHostFrame,
    ) -> Result<crate::engine::EngineInput, NativePlatformError> {
        if self.profile != super::NativeHostProfile::ManagedDesktop {
            return Err(NativePlatformError::HostProfileMismatch);
        }
        if self.input_pending_after(frame) {
            return Err(NativePlatformError::InputPending);
        }
        let provider = self.recorder.lease();
        if frame.backend_ingress_provider() != Some(provider)
            || frame.pointer_provider() != Some(provider.pointer_provider())
        {
            return Err(NativePlatformError::ProviderSuperseded);
        }
        Ok(crate::engine::EngineInput::CancelNativeInputForOverload { provider })
    }

    pub(in crate::runtime) fn prepare_pending_input(
        &self,
        engine: &DockEngine,
        frame: &mut CoreHostFrame,
        budgeted: bool,
    ) -> Result<bool, DockspaceRuntimeError> {
        let committed = engine.backend_ingress_committed_through();
        let batch = if budgeted {
            self.recorder.replay_prefix_after(
                committed,
                NonZeroUsize::new(256).expect("the native record limit is nonzero"),
            )
        } else {
            self.recorder.batch_after(committed)
        }
        .map_err(|_| NativePlatformError::ProtocolInvariant)?;
        if batch.is_empty() {
            frame.submit_backend_ingress(batch)?;
            Ok(false)
        } else {
            frame.prepare_backend_ingress(batch)?;
            Ok(true)
        }
    }

    pub(in crate::runtime) fn input_pending_after(&self, frame: &CoreHostFrame) -> bool {
        self.recorder.recorded_through() > frame.backend_ingress_through()
    }

    pub(in crate::runtime) fn take_pending_replay(&mut self) -> Option<PendingNativeReplay> {
        self.pending_replay.take()
    }

    pub(in crate::runtime) fn retain_pending_replay(&mut self, pending: PendingNativeReplay) {
        debug_assert!(self.pending_replay.is_none());
        self.pending_replay = Some(pending);
    }

    /// Drives the existing cursor until a completed-record budget boundary.
    /// False retains a private atomic group or an attempt whose budget expired
    /// before its first record. Receiver resolution uses the caller's budget.
    pub(in crate::runtime) fn reduce_pending_input(
        &mut self,
        frame: &mut CoreHostFrame,
        budget: Option<&mut NativeInputBudget>,
        mut resolver: Option<&mut dyn FnMut(NativeReceiverQuery) -> NativeReceiverAnswer>,
    ) -> Result<bool, DockspaceRuntimeError> {
        let mut budget = budget;
        if budget.as_ref().is_some_and(|budget| budget.is_exhausted()) {
            return Ok(false);
        }
        let mut progress = frame.resume_backend_ingress()?;

        loop {
            match progress {
                BackendIngressProgress::Complete => {
                    if let Some(budget) = budget.as_deref_mut() {
                        budget.complete_record();
                    }
                    return Ok(true);
                }
                BackendIngressProgress::RecordBoundary { can_seal, .. } => {
                    if let Some(budget) = budget.as_deref_mut() {
                        budget.complete_record();
                        if budget.is_exhausted() {
                            if can_seal {
                                frame.seal_backend_ingress_prefix()?;
                            }
                            return Ok(can_seal);
                        }
                    }
                    progress = frame.resume_backend_ingress()?;
                }
                BackendIngressProgress::ReceiverReceiptsRequired => {
                    let candidates = frame
                        .pointer_receiver_candidates()
                        .ok_or(NativePlatformError::ProtocolInvariant)?;
                    let receipts = candidates
                        .candidates()
                        .iter()
                        .map(|candidate| {
                            let observation = if candidate.receiver_is_applicable() {
                                let resolver = resolver
                                    .as_deref_mut()
                                    .ok_or(NativePlatformError::ReceiverResolverRequired)?;
                                resolve_receiver_observation(frame, candidate, resolver)?
                            } else {
                                PointerReceiverObservation::NotApplicable
                            };
                            Ok(candidate.receipt(observation))
                        })
                        .collect::<Result<Vec<_>, DockspaceRuntimeError>>()?;
                    let receipts = PointerReceiverReceiptBatch::new(receipts)
                        .map_err(|_| NativePlatformError::ProtocolInvariant)?;
                    progress = frame.submit_backend_pointer_receiver_receipts(receipts)?;
                }
            }
        }
    }
}

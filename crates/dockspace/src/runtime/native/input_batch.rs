//! Ordered native input replay and receiver settlement.

use super::receiver::resolve_receiver_observation;
use super::{NativePlatformError, NativeReceiverAnswer, NativeReceiverQuery, RuntimeNativeState};
use crate::engine::{BackendIngressProgress, CoreHostFrame, DockEngine};
use crate::pointer_receiver::{PointerReceiverObservation, PointerReceiverReceiptBatch};
use crate::runtime::DockspaceRuntimeError;

impl RuntimeNativeState {
    /// Replays the recorder suffix and settles every receiver challenge before
    /// returning control to the product host frame.
    pub(in crate::runtime) fn reduce_pending_input(
        &mut self,
        engine: &DockEngine,
        frame: &mut CoreHostFrame,
        mut resolver: Option<&mut dyn FnMut(NativeReceiverQuery) -> NativeReceiverAnswer>,
    ) -> Result<(), DockspaceRuntimeError> {
        let committed = engine.backend_ingress_committed_through();
        let batch = self
            .recorder
            .batch_after(committed)
            .map_err(|_| NativePlatformError::ProtocolInvariant)?;
        let mut progress = frame.submit_backend_ingress(batch)?;

        loop {
            match progress {
                BackendIngressProgress::Complete => break,
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

        Ok(())
    }
}

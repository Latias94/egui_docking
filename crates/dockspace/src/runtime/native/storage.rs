//! Capacity of the native facade's retained input transport.

use super::{NativePlatformError, RuntimeNativeState};
use crate::backend_ingress::{BackendIngressPayload, BackendIngressRecorder};
use crate::close_plan::SurfaceCloseRequest;
use crate::engine::EngineInput;
use crate::runtime::{DockspaceHostFrame, DockspaceRuntimeError, DockspaceSession};

impl RuntimeNativeState {
    fn input_owned_capacity_bytes(&self) -> Result<usize, NativePlatformError> {
        let mut bytes = size_of::<BackendIngressRecorder>()
            .saturating_add(self.recorder.record_buffer_capacity_bytes())
            .saturating_add(self.recorder.shared_fixed_data_bytes());
        for record in self.recorder.retained_records() {
            let descendants = match record.payload() {
                BackendIngressPayload::PlatformSnapshot { snapshot, .. } => {
                    snapshot.owned_payload_capacity_bytes()
                }
                BackendIngressPayload::PointerSegment(journal) => {
                    // The native facade creates new contents for every append.
                    // Its replay batches share the recorder's payload allocations.
                    journal.owned_contents_capacity_bytes()
                }
                BackendIngressPayload::SemanticInput(
                    EngineInput::RegisterViewport { .. }
                    | EngineInput::BootstrapChildViewport { .. }
                    | EngineInput::RequestSurfaceClose {
                        request:
                            SurfaceCloseRequest::RetainLayout
                            | SurfaceCloseRequest::RecoverPresentation
                            | SurfaceCloseRequest::CloseContent,
                        ..
                    }
                    | EngineInput::CancelSurfaceClose { .. },
                )
                | BackendIngressPayload::PlatformBindingQuiesced { .. }
                | BackendIngressPayload::NativeCloseObservation { .. }
                | BackendIngressPayload::GlobalFocusObservation { .. }
                | BackendIngressPayload::PlatformEffectResult(_)
                | BackendIngressPayload::PresentationObservation { .. } => 0,
                BackendIngressPayload::SemanticInput(_) => {
                    return Err(NativePlatformError::ProtocolInvariant);
                }
            };
            bytes = bytes.saturating_add(descendants);
        }
        if let Some(pending) = self.pending_replay.as_ref() {
            bytes = bytes.saturating_add(pending.frame.backend_ingress_buffer_capacity_bytes());
        }
        if let Some(receipt) = self.pending_prefix_retirement.as_ref() {
            bytes = bytes
                .saturating_add(size_of_val(receipt))
                .saturating_add(receipt.owned_capacity_bytes());
        }
        Ok(bytes)
    }
}

impl DockspaceSession {
    /// Returns the resident data capacity of recorded native input transport.
    ///
    /// Includes recorder envelopes, payload data, owned vector capacities, and
    /// the record buffers of a suspended replay. Shared payloads are charged once
    /// through their recorder. Retiring records does not release spare capacity.
    ///
    /// Counts the backend ingress recorder, replay transport buffers, and
    /// prefix-retirement receipt. Excludes pending effect/presentation report
    /// queues and reducer authority state. This is not total runtime memory:
    /// allocator overhead, Arc control blocks, workspace/candidate state,
    /// renderer state, and host-side buffers are excluded. A host with a live
    /// frame queries the corresponding frame method. Sessions without native
    /// input return zero.
    ///
    /// # Errors
    ///
    /// Returns an invariant error if an internal producer bypasses the native
    /// facade and records a semantic payload outside its finite transport contract.
    pub fn native_input_owned_capacity_bytes(&self) -> Result<usize, DockspaceRuntimeError> {
        self.native
            .as_ref()
            .map_or(Ok(0), RuntimeNativeState::input_owned_capacity_bytes)
            .map_err(Into::into)
    }
}

impl DockspaceHostFrame<'_> {
    /// Returns native input transport capacity, including this frame's buffers.
    ///
    /// Has the same measurement boundary as
    /// [`DockspaceSession::native_input_owned_capacity_bytes`]. The frame's cursor
    /// or commit-guard buffer is additional storage; its shared payloads remain
    /// charged through the session recorder.
    ///
    /// # Errors
    ///
    /// Returns the same internal-producer invariant error as the session query.
    pub fn native_input_owned_capacity_bytes(&self) -> Result<usize, DockspaceRuntimeError> {
        Ok(self
            .session
            .native_input_owned_capacity_bytes()?
            .saturating_add(self.frame.backend_ingress_buffer_capacity_bytes()))
    }
}

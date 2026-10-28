//! Capacity of the native facade's retained input transport.

use super::{NativePlatformError, RuntimeNativeState};
use crate::backend_ingress::BackendIngressRecorder;
use crate::runtime::{DockspaceHostFrame, DockspaceRuntimeError, DockspaceSession};
use std::num::NonZeroUsize;

/// Optional admission limit for a managed host's recorded input transport.
///
/// The allowance includes recorder backing, retained payload capacity, and
/// reserved space for one replay batch, its commit guard, and prefix retirement.
/// It excludes host callback buffers, temporary producer/iterator buffers,
/// pending reports, renderer state, reducer state, allocator overhead and Arc
/// control blocks. Hosts must account for
/// those owners separately when enforcing a total native-input budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeInputStorageLimit {
    record_slots: NonZeroUsize,
    byte_limit: usize,
}

impl NativeInputStorageLimit {
    /// Selects a record bound and a byte allowance for their complete transport.
    ///
    /// Enrollment rejects an allowance which cannot hold the fixed transport
    /// reservation and the initial pointer-authority record.
    #[must_use]
    pub const fn new(record_slots: NonZeroUsize, byte_limit: usize) -> Self {
        Self {
            record_slots,
            byte_limit,
        }
    }

    /// Returns the maximum number of retained input records.
    #[must_use]
    pub const fn record_slots(self) -> NonZeroUsize {
        self.record_slots
    }

    /// Returns the recorder and replay allowance in bytes.
    #[must_use]
    pub const fn byte_limit(self) -> usize {
        self.byte_limit
    }
}

impl RuntimeNativeState {
    fn input_owned_capacity_bytes(&self) -> Result<usize, NativePlatformError> {
        let mut bytes = size_of::<BackendIngressRecorder>()
            .saturating_add(self.recorder.record_buffer_capacity_bytes())
            .saturating_add(self.recorder.shared_fixed_data_bytes());
        for record in self.recorder.retained_records() {
            let descendants = record
                .payload()
                .native_owned_descendant_capacity_bytes()
                .map_err(NativePlatformError::from_ingress)?;
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

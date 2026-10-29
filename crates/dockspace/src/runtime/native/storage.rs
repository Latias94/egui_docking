//! Capacity of the native facade's retained input transport.

use super::{NativePlatformError, RuntimeNativeState};
use crate::backend_ingress::BackendIngressRecorder;
use crate::runtime::{DockspaceHostFrame, DockspaceRuntimeError, DockspaceSession};
use std::num::NonZeroUsize;

/// Optional admission limit for a managed host's recorded input transport.
///
/// The allowance includes recorder backing, retained payload capacity, and
/// reserved space for pointer preparation, one replay batch, its commit guard,
/// and prefix retirement. Preparation fits alongside still-retained payloads.
/// It excludes host callback buffers, producer maps and caller-owned iterators,
/// pending reports, renderer state, reducer state, allocator overhead and Arc
/// control blocks. Hosts must account for
/// those owners separately when enforcing a total native-input budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeInputStorageLimit {
    record_slots: NonZeroUsize,
    byte_limit: usize,
    report_storage: Option<NativeReportStorageLimit>,
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
            report_storage: None,
        }
    }

    /// Also bounds future presentation and native-effect terminal reporting.
    /// This is a separate allowance from the recorded/replay transport bytes.
    #[must_use]
    pub const fn with_report_storage_limit(mut self, limit: NativeReportStorageLimit) -> Self {
        self.report_storage = Some(limit);
        self
    }

    /// Returns the separately selected terminal-report allowance, if any.
    #[must_use]
    pub const fn report_storage_limit(self) -> Option<NativeReportStorageLimit> {
        self.report_storage
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

/// Optional storage promised before affine native output/effect delivery.
///
/// Presentation slots bound unsettled outputs and retained presentation stream
/// history; effect slots bound requests, retryable negative results and queued
/// abandoned requests. The byte allowance covers their retained backing,
/// complete preparation/settlement work and overlapping returned capabilities.
/// It excludes renderer artifacts, core authority and host lifecycle/command
/// owners, including successful-dispatch acknowledgement facts. Those owners
/// must be reserved separately by a host enforcing a combined input budget.
/// Select this profile before issuing presentation outputs; existing output
/// stream history and effect capabilities must be quiescent at enrollment.
/// Arbitrarily retained caller-owned report containers are outside the quota.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeReportStorageLimit {
    presentation_slots: NonZeroUsize,
    effect_slots: NonZeroUsize,
    byte_limit: usize,
}

impl NativeReportStorageLimit {
    /// Selects complete output/effect bounds and a prepaid byte allowance.
    /// Enrollment rejects insufficient bytes before publishing a provider.
    #[must_use]
    pub const fn new(
        presentation_slots: NonZeroUsize,
        effect_slots: NonZeroUsize,
        byte_limit: usize,
    ) -> Self {
        Self {
            presentation_slots,
            effect_slots,
            byte_limit,
        }
    }

    /// Returns the unsettled-output and retained-stream bound.
    #[must_use]
    pub const fn presentation_slots(self) -> NonZeroUsize {
        self.presentation_slots
    }

    /// Returns the outstanding native-effect failure-report bound.
    #[must_use]
    pub const fn effect_slots(self) -> NonZeroUsize {
        self.effect_slots
    }

    /// Returns the separately promised report-storage bytes.
    #[must_use]
    pub const fn byte_limit(self) -> usize {
        self.byte_limit
    }

    fn required_bytes(self) -> Option<usize> {
        super::super::presentation::RuntimePresentationState::reservation_bytes(
            self.presentation_slots.get(),
        )
        .and_then(|bytes| {
            super::super::native_effect::NativeEffectDropQueue::reservation_bytes(
                self.effect_slots.get(),
            )
            .and_then(|effects| bytes.checked_add(effects))
        })
    }

    pub(super) fn validate(self) -> Result<(), NativePlatformError> {
        self.validate_replacement(0)
    }

    pub(super) fn validate_replacement(
        self,
        old_backing: usize,
    ) -> Result<(), NativePlatformError> {
        if self
            .required_bytes()
            .and_then(|required| required.checked_add(old_backing))
            .is_none_or(|required| required > self.byte_limit)
        {
            return Err(NativePlatformError::InputCapacity);
        }
        Ok(())
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
    /// Returns the fixed allowance promised for native terminal-report storage.
    /// Its bytes are separate from the recorded/replay transport allowance and
    /// must be combined with host-side reservations before claiming a total cap.
    #[must_use]
    pub const fn native_input_report_storage_reserved_bytes(&self) -> Option<usize> {
        match self.native_report_storage_limit {
            Some(limit) => Some(limit.byte_limit()),
            None => None,
        }
    }

    /// Returns the owned data capacity of native effect and presentation reporting.
    ///
    /// Includes pending emission/result entries, retained capture generations,
    /// recorded-report metadata and shared abandoned-result queues once through
    /// their session owner. It excludes renderer artifacts, core authority,
    /// allocator metadata and Arc control blocks. The recorded transport returned
    /// by [`Self::native_input_owned_capacity_bytes`] is separate.
    /// Sessions without native input return zero.
    #[must_use]
    pub fn native_input_pending_report_capacity_bytes(&self) -> usize {
        if self.native.is_none() {
            return 0;
        }
        self.presentation
            .owned_data_capacity_bytes()
            .saturating_add(self.abandoned_native_effects.owned_data_capacity_bytes())
    }

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

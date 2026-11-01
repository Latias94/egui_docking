//! Optional bounds for the native facade's finite input transport.

use super::*;
use crate::close_plan::SurfaceCloseRequest;
use crate::engine::CoreHostFrame;

#[derive(Debug)]
pub(super) struct NativeTransportCapacity {
    record_slots: NonZeroUsize,
    byte_limit: usize,
    fixed_reserved_bytes: usize,
    pub(super) retained_data_bytes: usize,
}

impl BackendIngressPayload {
    /// Returns owned native payload descendants, excluding this enum and liveness.
    ///
    /// Native producers allocate new contents for each record. Replay batches
    /// share those contents with their recorder rather than expanding them again.
    pub(crate) fn native_owned_descendant_capacity_bytes(
        &self,
    ) -> Result<usize, BackendIngressError> {
        match self {
            Self::PlatformSnapshot { snapshot, .. } => Ok(snapshot.owned_payload_capacity_bytes()),
            Self::PointerSegment(journal) => Ok(journal.owned_contents_capacity_bytes()),
            Self::SemanticInput(input) => match input.as_ref() {
                EngineInput::RegisterViewport { .. }
                | EngineInput::UpdateNativeMagneticTargeting { .. }
                | EngineInput::BootstrapChildViewport { .. }
                | EngineInput::RequestSurfaceClose {
                    request:
                        SurfaceCloseRequest::RetainLayout
                        | SurfaceCloseRequest::RecoverPresentation
                        | SurfaceCloseRequest::CloseContent,
                    ..
                }
                | EngineInput::CancelSurfaceClose { .. } => Ok(size_of::<EngineInput>()),
                _ => Err(BackendIngressError::UnsupportedNativePayloadCapacity),
            },
            Self::NativeInputDeviceRemoval
            | Self::NativeInputBindingRetired { .. }
            | Self::PlatformBindingQuiesced { .. }
            | Self::NativeCloseObservation { .. }
            | Self::GlobalFocusObservation { .. }
            | Self::PlatformEffectResult(_)
            | Self::PresentationObservation { .. } => Ok(0),
        }
    }
}

impl BackendIngressRecorder {
    /// Checks a fresh recorder's finite transport before constructing its facts.
    /// The real configuration and append recheck actual allocation capacities.
    pub(crate) fn preflight_native_transport_configuration(
        record_slots: NonZeroUsize,
        byte_limit: usize,
        additional_records: usize,
        owned_descendant_bytes: usize,
    ) -> Result<(), BackendIngressError> {
        if additional_records > record_slots.get() {
            return Err(BackendIngressError::NativeTransportCapacityExceeded {
                limit: record_slots.get(),
                required: additional_records,
            });
        }
        let fixed = fixed_reserved_bytes(record_slots, record_slots.get(), byte_limit)?;
        let data = additional_records
            .checked_mul(size_of::<BackendIngressPayload>() + size_of::<AtomicU8>())
            .and_then(|bytes| bytes.checked_add(owned_descendant_bytes))
            .ok_or_else(|| capacity_overflow(byte_limit))?;
        check_byte_limit(fixed, data, byte_limit)
    }

    /// Bounds facade-owned transport while leaving generic recorders unlimited.
    ///
    /// Configure before exposing native replay. The fixed allowance covers one
    /// recorder buffer, one prepared-pointer handle buffer, one replay batch and
    /// its overlapping commit guard, and one pending prefix-retirement receipt.
    /// It is admission credit, not a
    /// resident-capacity diagnostic. Native replay must retain at most one
    /// candidate and must not let arbitrary batch clones escape the facade.
    /// Existing records must satisfy the same finite native payload contract.
    /// Failure preserves the recorder's records, backing and previous limit.
    pub(crate) fn configure_native_transport_capacity(
        &mut self,
        record_slots: NonZeroUsize,
        byte_limit: usize,
    ) -> Result<(), BackendIngressError> {
        if self.records.len() > record_slots.get() {
            return Err(BackendIngressError::NativeTransportCapacityExceeded {
                limit: record_slots.get(),
                required: self.records.len(),
            });
        }
        let retained_data_bytes = retained_data_bytes(&self.records, byte_limit)?;
        let minimum_backing_slots = self.records.capacity().max(record_slots.get());
        let minimum_fixed = fixed_reserved_bytes(record_slots, minimum_backing_slots, byte_limit)?;
        check_byte_limit(minimum_fixed, retained_data_bytes, byte_limit)?;

        let mut backing = if self.records.capacity() < record_slots.get() {
            let mut records = Vec::new();
            records
                .try_reserve_exact(record_slots.get())
                .map_err(|_| BackendIngressError::CapacityAllocationFailed)?;
            Some(records)
        } else {
            None
        };
        let backing_slots = backing
            .as_ref()
            .map_or(self.records.capacity(), Vec::capacity);
        let fixed_reserved_bytes = fixed_reserved_bytes(record_slots, backing_slots, byte_limit)?;
        check_byte_limit(fixed_reserved_bytes, retained_data_bytes, byte_limit)?;
        if let Some(records) = backing.as_mut() {
            records.append(&mut self.records);
        }
        if let Some(records) = backing {
            self.records = records;
        }
        self.native_transport_capacity = Some(NativeTransportCapacity {
            record_slots,
            byte_limit,
            fixed_reserved_bytes,
            retained_data_bytes,
        });
        Ok(())
    }

    /// Returns charged data plus reserved replay capacity, not resident bytes.
    #[must_use]
    pub(crate) fn native_transport_reserved_bytes(&self) -> Option<usize> {
        self.native_transport_capacity
            .as_ref()
            .map(|capacity| capacity.fixed_reserved_bytes + capacity.retained_data_bytes)
    }

    /// Bounds preparation while committed payloads remain physically retained.
    ///
    /// A committed prefix can release slots and data for the eventual append,
    /// but preparation must also fit alongside that prefix before it is retired.
    /// A suspended replay supplies no watermark and receives no retirement credit.
    pub(crate) fn native_pointer_preparation_limit(
        &self,
        reclaimable: Option<BackendIngressCommitWatermark>,
    ) -> Result<Option<usize>, BackendIngressError> {
        let Some(capacity) = self.native_transport_capacity.as_ref() else {
            return Ok(None);
        };
        let reclaimed_records = reclaimable
            .map(|watermark| self.validate_committed_prefix(watermark))
            .transpose()?
            .unwrap_or(0);
        let reclaimed_data =
            retained_data_bytes(&self.records[..reclaimed_records], capacity.byte_limit)?;
        let descendants = PointerEdgeJournal::single_edge_owned_contents_capacity_bytes();
        let per_record = descendants + size_of::<BackendIngressPayload>() + size_of::<AtomicU8>();
        let available =
            capacity.byte_limit - capacity.fixed_reserved_bytes - capacity.retained_data_bytes;
        let preparation_limit = available / descendants;
        let append_limit = available
            .checked_add(reclaimed_data)
            .ok_or_else(|| capacity_overflow(capacity.byte_limit))?
            / per_record;
        let slot_limit = capacity.record_slots.get() - (self.records.len() - reclaimed_records);
        Ok(Some(preparation_limit.min(append_limit).min(slot_limit)))
    }

    /// Checks an entire finite native append before any record identity is minted.
    ///
    /// The caller supplies the sum of owned descendants for every new record.
    /// Payload and liveness data are added here. This reserves no state: native
    /// producers hold the sole mutable recorder borrow until the append finishes.
    /// Every push still validates its actual payload against the same limit.
    pub(crate) fn preflight_native_transport_append(
        &self,
        additional_records: usize,
        owned_descendant_bytes: usize,
    ) -> Result<(), BackendIngressError> {
        let Some(capacity) = self.native_transport_capacity.as_ref() else {
            return Ok(());
        };
        let additional_data = additional_records
            .checked_mul(size_of::<BackendIngressPayload>() + size_of::<AtomicU8>())
            .and_then(|bytes| bytes.checked_add(owned_descendant_bytes))
            .ok_or_else(|| capacity_overflow(capacity.byte_limit))?;
        self.check_native_transport_data_append(additional_records, additional_data)?;
        Ok(())
    }

    pub(super) fn check_native_transport_append(
        &self,
        payload: &BackendIngressPayload,
    ) -> Result<usize, BackendIngressError> {
        let Some(capacity) = self.native_transport_capacity.as_ref() else {
            return Ok(0);
        };
        let incoming = payload_data_bytes(payload, capacity.byte_limit)?;
        let retained = self.check_native_transport_data_append(1, incoming)?;
        debug_assert!(self.records.len() < self.records.capacity());
        Ok(retained)
    }

    fn check_native_transport_data_append(
        &self,
        additional_records: usize,
        additional_data: usize,
    ) -> Result<usize, BackendIngressError> {
        let capacity = self
            .native_transport_capacity
            .as_ref()
            .expect("native append capacity is checked only for a bounded recorder");
        let required_slots = self.records.len().checked_add(additional_records);
        if required_slots.is_none_or(|required| required > capacity.record_slots.get()) {
            return Err(BackendIngressError::NativeTransportCapacityExceeded {
                limit: capacity.record_slots.get(),
                required: required_slots.unwrap_or(usize::MAX),
            });
        }
        let retained = capacity
            .retained_data_bytes
            .checked_add(additional_data)
            .ok_or_else(|| capacity_overflow(capacity.byte_limit))?;
        check_byte_limit(capacity.fixed_reserved_bytes, retained, capacity.byte_limit)?;
        Ok(retained)
    }

    pub(super) fn native_retained_data_bytes(
        &self,
        records: &[BackendIngressRecord],
    ) -> Result<usize, BackendIngressError> {
        self.native_transport_capacity
            .as_ref()
            .map_or(Ok(0), |capacity| {
                retained_data_bytes(records, capacity.byte_limit)
            })
    }
}

fn payload_data_bytes(
    payload: &BackendIngressPayload,
    limit: usize,
) -> Result<usize, BackendIngressError> {
    (size_of::<BackendIngressPayload>() + size_of::<AtomicU8>())
        .checked_add(payload.native_owned_descendant_capacity_bytes()?)
        .ok_or_else(|| capacity_overflow(limit))
}

fn retained_data_bytes(
    records: &[BackendIngressRecord],
    limit: usize,
) -> Result<usize, BackendIngressError> {
    records.iter().try_fold(0_usize, |bytes, record| {
        bytes
            .checked_add(payload_data_bytes(record.payload(), limit)?)
            .ok_or_else(|| capacity_overflow(limit))
    })
}

fn fixed_reserved_bytes(
    record_slots: NonZeroUsize,
    backing_slots: usize,
    limit: usize,
) -> Result<usize, BackendIngressError> {
    let inline = size_of::<BackendIngressRecorder>()
        + size_of::<Mutex<()>>()
        + CoreHostFrame::backend_ingress_inline_capacity_bytes()
        + size_of::<Option<BackendIngressPrefixRetirementReceipt>>();
    let per_replay_slot = size_of::<BackendIngressRecord>()
        + size_of::<(BackendIngressOrdinal, BackendIngressRecordLiveness)>()
        + size_of::<ViewportBinding>()
        + size_of::<PointerEdgeJournal>();
    backing_slots
        .checked_mul(size_of::<BackendIngressRecord>())
        .and_then(|bytes| bytes.checked_add(inline))
        .and_then(|bytes| {
            record_slots
                .get()
                .checked_mul(per_replay_slot)
                .and_then(|replay| bytes.checked_add(replay))
        })
        .ok_or_else(|| capacity_overflow(limit))
}

fn check_byte_limit(
    fixed: usize,
    retained: usize,
    limit: usize,
) -> Result<(), BackendIngressError> {
    let required = fixed
        .checked_add(retained)
        .ok_or_else(|| capacity_overflow(limit))?;
    if required > limit {
        return Err(BackendIngressError::NativeTransportCapacityExceeded { limit, required });
    }
    Ok(())
}

fn capacity_overflow(limit: usize) -> BackendIngressError {
    BackendIngressError::NativeTransportCapacityExceeded {
        limit,
        required: usize::MAX,
    }
}

#[cfg(test)]
mod tests;

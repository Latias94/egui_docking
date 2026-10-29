//! Exact native pointer pass-through command and observation ownership.

use std::collections::VecDeque;

use dockspace::runtime::{
    NativeDispatchFailure, NativeEffectAcknowledgement, NativeEffectRequest, NativeEffectResult,
    NativeIndeterminateReason, NativeInputEffectAcknowledgement, NativeSurfaceBinding,
    NativeUnsupportedReason, NativeWindowInputState,
};
use eframe::{
    NativeViewportPointerPassthroughCommandToken, NativeViewportPointerPassthroughResult,
    NativeViewportPointerPassthroughStatus, egui::ViewportId,
};
use winit::window::WindowId;

use crate::error::NativeHostProtocolError;
use crate::viewport_map::NativeViewportMap;

#[cfg(test)]
mod capacity_tests;

/// One exact result of applying a native pointer pass-through command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativePointerPassthroughRecord {
    token: NativeViewportPointerPassthroughCommandToken,
    viewport: ViewportId,
    window: WindowId,
    binding: Option<NativeSurfaceBinding>,
    enabled: bool,
    status: NativeViewportPointerPassthroughStatus,
}

impl NativePointerPassthroughRecord {
    pub(crate) const fn retired_before_dispatch(
        token: NativeViewportPointerPassthroughCommandToken,
        viewport: ViewportId,
        window: WindowId,
        binding: NativeSurfaceBinding,
        enabled: bool,
    ) -> Self {
        Self {
            token,
            viewport,
            window,
            binding: Some(binding),
            enabled,
            status: NativeViewportPointerPassthroughStatus::Retired,
        }
    }

    pub(crate) fn qualified_for_binding(mut self, binding: NativeSurfaceBinding) -> Self {
        self.binding = Some(binding);
        self
    }

    pub(crate) fn capture(
        result: NativeViewportPointerPassthroughResult,
        viewports: &NativeViewportMap,
    ) -> Self {
        Self {
            token: result.token(),
            viewport: result.viewport_id(),
            window: result.window_id(),
            binding: viewports.binding_for_event(result.window_id(), Some(result.viewport_id())),
            enabled: result.enabled(),
            status: result.status(),
        }
    }

    #[cfg(test)]
    pub(crate) const fn for_test(
        token: NativeViewportPointerPassthroughCommandToken,
        viewport: ViewportId,
        window: WindowId,
        binding: Option<NativeSurfaceBinding>,
        enabled: bool,
        status: NativeViewportPointerPassthroughStatus,
    ) -> Self {
        Self {
            token,
            viewport,
            window,
            binding,
            enabled,
            status,
        }
    }

    pub(crate) const fn viewport(self) -> ViewportId {
        self.viewport
    }

    pub(crate) const fn window(self) -> WindowId {
        self.window
    }

    pub(crate) const fn token(self) -> NativeViewportPointerPassthroughCommandToken {
        self.token
    }

    pub(crate) const fn enabled(self) -> bool {
        self.enabled
    }

    pub(crate) const fn binding(self) -> Option<NativeSurfaceBinding> {
        self.binding
    }

    pub(crate) const fn status(self) -> NativeViewportPointerPassthroughStatus {
        self.status
    }

    pub(crate) fn references_binding(self, binding: NativeSurfaceBinding) -> bool {
        self.binding == Some(binding)
    }

    const fn state(self) -> NativeWindowInputState {
        if self.enabled {
            NativeWindowInputState::PassThrough
        } else {
            NativeWindowInputState::ReceivesInput
        }
    }
}

/// One command which may be dispatched after the preceding input fact commits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativePointerPassthroughCommand {
    viewport: ViewportId,
    binding: NativeSurfaceBinding,
    enabled: bool,
}

impl NativePointerPassthroughCommand {
    pub(crate) const fn viewport(self) -> ViewportId {
        self.viewport
    }

    pub(crate) const fn enabled(self) -> bool {
        self.enabled
    }

    pub(crate) const fn binding(self) -> NativeSurfaceBinding {
        self.binding
    }
}

#[derive(Debug)]
enum PendingNativeInputState {
    Queued(NativeEffectRequest),
    AwaitingCallback {
        request: NativeEffectRequest,
        token: NativeViewportPointerPassthroughCommandToken,
    },
    AwaitingSnapshot,
}

#[derive(Debug)]
struct PendingNativeInput {
    viewport: ViewportId,
    window: WindowId,
    binding: NativeSurfaceBinding,
    enabled: bool,
    state: PendingNativeInputState,
}

impl PendingNativeInput {
    fn matches_callback(&self, record: NativePointerPassthroughRecord) -> bool {
        self.viewport == record.viewport
            && self.window == record.window
            && self.enabled == record.enabled
            && match &self.state {
                PendingNativeInputState::AwaitingCallback { token, .. } => *token == record.token,
                PendingNativeInputState::Queued(_) | PendingNativeInputState::AwaitingSnapshot => {
                    false
                }
            }
    }

    const fn command(&self) -> NativePointerPassthroughCommand {
        NativePointerPassthroughCommand {
            viewport: self.viewport,
            binding: self.binding,
            enabled: self.enabled,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeInputObservation {
    binding: NativeSurfaceBinding,
    state: NativeWindowInputState,
    acknowledgement: Option<NativeInputEffectAcknowledgement>,
}

/// Classification of one callback against the globally ordered input command queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeInputResultDisposition {
    Unrelated,
    Current {
        binding: NativeSurfaceBinding,
        status: NativeViewportPointerPassthroughStatus,
    },
    Stale {
        binding: NativeSurfaceBinding,
    },
}

/// Single ordered owner for pointer pass-through commands and exact observations.
#[derive(Debug, Default)]
pub(crate) struct NativeInputControl {
    pending: VecDeque<PendingNativeInput>,
    observations: Vec<NativeInputObservation>,
    binding_slots: Option<usize>,
    pending_slots: Option<usize>,
    quarantined: bool,
}

impl NativeInputControl {
    /// Prepays the selected dimensions before accepting any command or callback.
    /// Only the globally ordered head can await its snapshot after releasing
    /// its report lease, so E outstanding requests need at most E + 1 entries.
    pub(crate) fn with_storage_limits(
        binding_slots: Option<usize>,
        effect_slots: Option<usize>,
    ) -> Result<Self, NativeHostProtocolError> {
        Self::reservation_bytes(binding_slots, effect_slots)
            .ok_or(NativeHostProtocolError::NativeReportStorageUnavailable)?;
        let pending_slots = effect_slots.map(|slots| slots + 1);
        let mut pending = VecDeque::new();
        let mut observations = Vec::new();
        if let Some(slots) = pending_slots {
            pending
                .try_reserve_exact(slots)
                .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
            if pending.capacity() > slots {
                return Err(NativeHostProtocolError::NativeReportStorageUnavailable);
            }
        }
        if let Some(slots) = binding_slots {
            observations
                .try_reserve_exact(slots)
                .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
            if observations.capacity() > slots {
                return Err(NativeHostProtocolError::NativeReportStorageUnavailable);
            }
        }
        Ok(Self {
            pending,
            observations,
            binding_slots,
            pending_slots,
            quarantined: false,
        })
    }

    /// Counts inline state and prepaid backing only for selected dimensions.
    /// An unlimited dimension contributes no backing promise; this is not a
    /// total bound unless both the producer and report limits are selected.
    pub(crate) fn reservation_bytes(
        binding_slots: Option<usize>,
        effect_slots: Option<usize>,
    ) -> Option<usize> {
        let pending_slots = match effect_slots {
            Some(slots) => slots.checked_add(1)?,
            None => 0,
        };
        size_of::<Self>()
            .checked_add(pending_slots.checked_mul(size_of::<PendingNativeInput>())?)?
            .checked_add(
                binding_slots
                    .unwrap_or(0)
                    .checked_mul(size_of::<NativeInputObservation>())?,
            )
    }

    /// Counts inline storage and actual pending/observation backing capacity.
    /// Allocator overhead is excluded. Core counts the shared
    /// abandoned-effect queue behind the inline request handles once.
    pub(crate) fn owned_data_capacity_bytes(&self) -> usize {
        size_of::<Self>()
            .saturating_add(
                self.pending
                    .capacity()
                    .saturating_mul(size_of::<PendingNativeInput>()),
            )
            .saturating_add(
                self.observations
                    .capacity()
                    .saturating_mul(size_of::<NativeInputObservation>()),
            )
    }

    pub(crate) fn has_pending_work(&self) -> bool {
        !self.pending.is_empty()
            || self
                .observations
                .iter()
                .any(|observation| observation.acknowledgement.is_some())
    }

    pub(crate) fn requires_snapshot(&self) -> bool {
        self.observations
            .iter()
            .any(|observation| observation.acknowledgement.is_some())
            || self.pending.front().is_some_and(|pending| {
                matches!(pending.state, PendingNativeInputState::AwaitingSnapshot)
            })
    }

    pub(crate) fn retain(
        &mut self,
        viewport: ViewportId,
        window: WindowId,
        binding: NativeSurfaceBinding,
        enabled: bool,
        request: NativeEffectRequest,
    ) -> Result<(), NativeEffectRequest> {
        if self
            .pending_slots
            .is_some_and(|slots| self.pending.len() >= slots)
            || self.pending_slots.is_none() && self.pending.try_reserve(1).is_err()
        {
            return Err(request);
        }
        self.pending.push_back(PendingNativeInput {
            viewport,
            window,
            binding,
            enabled,
            state: PendingNativeInputState::Queued(request),
        });
        Ok(())
    }

    pub(crate) fn command_to_dispatch(&self) -> Option<NativePointerPassthroughCommand> {
        let pending = self.pending.front()?;
        matches!(pending.state, PendingNativeInputState::Queued(_)).then(|| pending.command())
    }

    pub(crate) fn mark_dispatched(
        &mut self,
        expected: NativePointerPassthroughCommand,
        token: NativeViewportPointerPassthroughCommandToken,
    ) -> bool {
        let Some(pending) = self.pending.front_mut() else {
            return false;
        };
        if pending.command() != expected {
            return false;
        }
        let state = std::mem::replace(
            &mut pending.state,
            PendingNativeInputState::AwaitingSnapshot,
        );
        match state {
            PendingNativeInputState::Queued(request) => {
                pending.state = PendingNativeInputState::AwaitingCallback { request, token };
                true
            }
            state => {
                pending.state = state;
                false
            }
        }
    }

    pub(crate) fn classify(
        &self,
        record: NativePointerPassthroughRecord,
    ) -> NativeInputResultDisposition {
        let Some(pending) = self.pending.front() else {
            return NativeInputResultDisposition::Unrelated;
        };
        if !matches!(
            pending.state,
            PendingNativeInputState::AwaitingCallback { .. }
        ) || !pending.matches_callback(record)
        {
            return NativeInputResultDisposition::Unrelated;
        }
        if record.binding == Some(pending.binding) {
            NativeInputResultDisposition::Current {
                binding: pending.binding,
                status: record.status,
            }
        } else {
            NativeInputResultDisposition::Stale {
                binding: pending.binding,
            }
        }
    }

    pub(crate) fn accept_applied(
        &mut self,
        record: NativePointerPassthroughRecord,
    ) -> Result<bool, NativeHostProtocolError> {
        let NativeInputResultDisposition::Current { binding, status } = self.classify(record)
        else {
            return Ok(false);
        };
        if status != NativeViewportPointerPassthroughStatus::Applied {
            return Ok(false);
        }
        // Prepare storage before accepted() releases the affine report lease.
        // A failed preflight leaves the callback correlation and request intact.
        self.preflight_observation(binding)?;
        let Some(pending) = self.pending.front_mut() else {
            return Ok(false);
        };
        let state = std::mem::replace(
            &mut pending.state,
            PendingNativeInputState::AwaitingSnapshot,
        );
        let PendingNativeInputState::AwaitingCallback { request, token } = state else {
            pending.state = state;
            return Ok(false);
        };
        debug_assert_eq!(token, record.token);
        let Some(NativeEffectAcknowledgement::Input(acknowledgement)) = request.accepted() else {
            unreachable!("pointer pass-through effects carry input acknowledgements")
        };
        self.store_observation(NativeInputObservation {
            binding,
            state: record.state(),
            acknowledgement: Some(acknowledgement),
        });
        Ok(true)
    }

    pub(crate) fn observe_unowned_applied(
        &mut self,
        record: NativePointerPassthroughRecord,
    ) -> Result<bool, NativeHostProtocolError> {
        let Some(binding) = record.binding else {
            return Ok(false);
        };
        if record.status != NativeViewportPointerPassthroughStatus::Applied
            || self
                .pending
                .iter()
                .any(|pending| pending.binding == binding)
        {
            return Ok(false);
        }
        self.preflight_observation(binding)?;
        self.store_observation(NativeInputObservation {
            binding,
            state: record.state(),
            acknowledgement: None,
        });
        Ok(true)
    }

    fn preflight_observation(
        &mut self,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativeHostProtocolError> {
        if self
            .observations
            .binary_search_by_key(&binding, |observation| observation.binding)
            .is_ok()
        {
            return Ok(());
        }
        if self
            .binding_slots
            .is_some_and(|slots| self.observations.len() >= slots)
        {
            return Err(NativeHostProtocolError::NativeReportStorageUnavailable);
        }
        if self.binding_slots.is_none() {
            self.observations
                .try_reserve(1)
                .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
        }
        Ok(())
    }

    /// The caller prepays a new entry before changing any affine owner.
    fn store_observation(&mut self, observation: NativeInputObservation) {
        match self
            .observations
            .binary_search_by_key(&observation.binding, |entry| entry.binding)
        {
            Ok(index) => self.observations[index] = observation,
            Err(index) => self.observations.insert(index, observation),
        }
    }

    pub(crate) fn take_unsupported_result(
        &mut self,
        record: NativePointerPassthroughRecord,
    ) -> Option<NativeEffectResult> {
        self.take_failure_result(record, |request| {
            request.unsupported(NativeUnsupportedReason::BackendUnsupported)
        })
    }

    pub(crate) fn take_dispatch_failure_result(
        &mut self,
        record: NativePointerPassthroughRecord,
        reason: NativeDispatchFailure,
    ) -> Option<NativeEffectResult> {
        self.take_failure_result(record, |request| request.dispatch_failed(reason))
    }

    fn take_failure_result(
        &mut self,
        record: NativePointerPassthroughRecord,
        into_result: impl FnOnce(NativeEffectRequest) -> NativeEffectResult,
    ) -> Option<NativeEffectResult> {
        if matches!(
            self.classify(record),
            NativeInputResultDisposition::Unrelated
        ) {
            return None;
        }
        let pending = self.pending.pop_front()?;
        if !pending.matches_callback(record) {
            self.pending.push_front(pending);
            return None;
        }
        match pending {
            PendingNativeInput {
                state: PendingNativeInputState::AwaitingCallback { request, .. },
                ..
            } => Some(into_result(request)),
            pending => {
                self.pending.push_front(pending);
                None
            }
        }
    }

    pub(crate) fn snapshot_settlement_matches(
        &self,
        acknowledged: Option<NativeSurfaceBinding>,
    ) -> bool {
        let Some(acknowledged) = acknowledged else {
            return true;
        };
        self.pending.front().is_some_and(|pending| {
            matches!(pending.state, PendingNativeInputState::AwaitingSnapshot)
                && acknowledged == pending.binding
        })
    }

    pub(crate) fn settle_snapshot(&mut self, acknowledged: Option<NativeSurfaceBinding>) -> bool {
        if !self.snapshot_settlement_matches(acknowledged) {
            return false;
        }
        let Some(binding) = acknowledged else {
            return false;
        };
        if let Ok(index) = self
            .observations
            .binary_search_by_key(&binding, |observation| observation.binding)
        {
            self.observations[index].acknowledgement = None;
        }
        self.pending.pop_front();
        true
    }

    pub(crate) fn snapshot_fact(
        &self,
        binding: NativeSurfaceBinding,
    ) -> Option<(
        NativeWindowInputState,
        Option<NativeInputEffectAcknowledgement>,
    )> {
        self.observations
            .binary_search_by_key(&binding, |observation| observation.binding)
            .ok()
            .and_then(|index| self.observations.get(index))
            .map(|observation| (observation.state, observation.acknowledgement))
    }

    pub(crate) fn retire_binding_into(
        &mut self,
        binding: NativeSurfaceBinding,
        mut emit: impl FnMut(NativeEffectResult),
    ) {
        self.observations
            .retain(|observation| observation.binding != binding);
        // Visit the original queue exactly once. Survivors return to the same
        // backing in order; affine results move directly into the prepaid sink.
        let count = self.pending.len();
        for _ in 0..count {
            let pending = self
                .pending
                .pop_front()
                .expect("each original input entry is visited once");
            if pending.binding != binding {
                self.pending.push_back(pending);
                continue;
            }
            match pending.state {
                PendingNativeInputState::Queued(request) => {
                    emit(request.dispatch_failed(NativeDispatchFailure::ProviderStopped));
                }
                PendingNativeInputState::AwaitingCallback { request, .. } => {
                    emit(request.indeterminate(NativeIndeterminateReason::AcknowledgementLost));
                }
                PendingNativeInputState::AwaitingSnapshot => {}
            }
        }
    }

    pub(crate) fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.observations
            .binary_search_by_key(&binding, |observation| observation.binding)
            .is_ok()
            || self
                .pending
                .iter()
                .any(|pending| pending.binding == binding)
    }

    pub(crate) fn fail_queued_dispatch(
        &mut self,
        expected: NativePointerPassthroughCommand,
    ) -> Option<NativeEffectResult> {
        let matches = self.pending.front().is_some_and(|pending| {
            matches!(pending.state, PendingNativeInputState::Queued(_))
                && pending.command() == expected
        });
        if !matches {
            return None;
        }
        let pending = self
            .pending
            .pop_front()
            .expect("the exact queued input command was inspected");
        let PendingNativeInputState::Queued(request) = pending.state else {
            unreachable!("the exact queued input command retains its affine request")
        };
        Some(request.dispatch_failed(NativeDispatchFailure::AdapterRejected))
    }

    pub(crate) fn begin_quarantine(&mut self) {
        self.quarantined = true;
    }

    pub(crate) fn cancel_quarantined_front(&mut self) -> Option<NativeEffectResult> {
        let cancellable = self.quarantined
            && self.pending.front().is_some_and(|pending| {
                pending.enabled && matches!(pending.state, PendingNativeInputState::Queued(_))
            });
        if !cancellable {
            return None;
        }
        let pending = self
            .pending
            .pop_front()
            .expect("the quarantined input command was inspected");
        let PendingNativeInputState::Queued(request) = pending.state else {
            unreachable!("the quarantined input command retains its affine request")
        };
        Some(request.dispatch_failed(NativeDispatchFailure::ProviderStopped))
    }
}

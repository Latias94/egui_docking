//! Exact native pointer pass-through command and observation ownership.

use std::collections::{BTreeMap, VecDeque};

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

use crate::viewport_map::NativeViewportMap;

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
    observations: BTreeMap<NativeSurfaceBinding, NativeInputObservation>,
    quarantined: bool,
}

impl NativeInputControl {
    pub(crate) fn has_pending_work(&self) -> bool {
        !self.pending.is_empty()
            || self
                .observations
                .values()
                .any(|observation| observation.acknowledgement.is_some())
    }

    pub(crate) fn requires_snapshot(&self) -> bool {
        self.observations
            .values()
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
    ) {
        self.pending.push_back(PendingNativeInput {
            viewport,
            window,
            binding,
            enabled,
            state: PendingNativeInputState::Queued(request),
        });
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

    pub(crate) fn accept_applied(&mut self, record: NativePointerPassthroughRecord) -> bool {
        let NativeInputResultDisposition::Current { binding, status } = self.classify(record)
        else {
            return false;
        };
        if status != NativeViewportPointerPassthroughStatus::Applied {
            return false;
        }
        let Some(pending) = self.pending.front_mut() else {
            return false;
        };
        let state = std::mem::replace(
            &mut pending.state,
            PendingNativeInputState::AwaitingSnapshot,
        );
        let PendingNativeInputState::AwaitingCallback { request, token } = state else {
            pending.state = state;
            return false;
        };
        debug_assert_eq!(token, record.token);
        let Some(NativeEffectAcknowledgement::Input(acknowledgement)) = request.accepted() else {
            unreachable!("pointer pass-through effects carry input acknowledgements")
        };
        self.observations.insert(
            binding,
            NativeInputObservation {
                state: record.state(),
                acknowledgement: Some(acknowledgement),
            },
        );
        true
    }

    pub(crate) fn observe_unowned_applied(
        &mut self,
        record: NativePointerPassthroughRecord,
    ) -> bool {
        let Some(binding) = record.binding else {
            return false;
        };
        if record.status != NativeViewportPointerPassthroughStatus::Applied
            || self
                .pending
                .iter()
                .any(|pending| pending.binding == binding)
        {
            return false;
        }
        self.observations.insert(
            binding,
            NativeInputObservation {
                state: record.state(),
                acknowledgement: None,
            },
        );
        true
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

    pub(crate) fn settle_snapshot(&mut self, acknowledged: Option<NativeSurfaceBinding>) -> bool {
        if let Some(binding) = acknowledged
            && let Some(observation) = self.observations.get_mut(&binding)
        {
            observation.acknowledgement = None;
        }
        let Some(pending) = self.pending.front() else {
            return false;
        };
        if !matches!(pending.state, PendingNativeInputState::AwaitingSnapshot)
            || acknowledged != Some(pending.binding)
        {
            return false;
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
            .get(&binding)
            .map(|observation| (observation.state, observation.acknowledgement))
    }

    pub(crate) fn retire_binding(
        &mut self,
        binding: NativeSurfaceBinding,
    ) -> Vec<NativeEffectResult> {
        self.observations.remove(&binding);
        let mut retained = VecDeque::with_capacity(self.pending.len());
        let mut results = Vec::new();
        while let Some(pending) = self.pending.pop_front() {
            if pending.binding != binding {
                retained.push_back(pending);
                continue;
            }
            match pending.state {
                PendingNativeInputState::Queued(request) => {
                    results.push(request.dispatch_failed(NativeDispatchFailure::ProviderStopped));
                }
                PendingNativeInputState::AwaitingCallback { request, .. } => results
                    .push(request.indeterminate(NativeIndeterminateReason::AcknowledgementLost)),
                PendingNativeInputState::AwaitingSnapshot => {}
            }
        }
        self.pending = retained;
        results
    }

    pub(crate) fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.observations.contains_key(&binding)
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
        Some(request.indeterminate(NativeIndeterminateReason::AcknowledgementLost))
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

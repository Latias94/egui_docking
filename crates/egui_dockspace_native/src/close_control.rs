//! Exact native close correlation owned by the eframe adapter.

use dockspace::close::CloseItemDecisionState;
use std::collections::BTreeMap;

use dockspace::runtime::{
    DockspaceClosePlan, NativeCloseEffectAcknowledgement, NativeSurfaceBinding,
    NativeSurfaceCloseAction, NativeSurfaceCloseRequest,
};
use eframe::{NativeViewportCloseRequest, egui::ViewportId};
use winit::window::WindowId;

use crate::event::NativeWindowEventRecord;
use crate::mailbox::journal::ReservedHostRecord;

#[cfg(test)]
mod tests;

/// Explicit product policy for an operating-system native close request.
///
/// The policy never guesses a rehome target or silently destroys content.
/// The framework holds the original request while ordered input or application
/// approval delays its exact decision.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NativeWindowClosePolicy {
    /// Recover a managed child through its core-owned anchor.
    ///
    /// The external application root has no predecessor presentation to
    /// recover into, so this default accepts that operating-system close and
    /// retains its logical layout for application-owned shutdown handling.
    #[default]
    RecoverPresentation,
    /// Keep the native window open.
    Cancel,
    /// Destroy the native binding while retaining its logical surface roster.
    RetainLayout,
    /// Close every pane owned by the native surface.
    CloseContent,
}

impl NativeWindowClosePolicy {
    pub(crate) const fn request(self, is_external_root: bool) -> Option<NativeSurfaceCloseAction> {
        match self {
            Self::RecoverPresentation if is_external_root => {
                Some(NativeSurfaceCloseAction::RetainLayout)
            }
            Self::RecoverPresentation => Some(NativeSurfaceCloseAction::RecoverPresentation),
            Self::Cancel => None,
            Self::RetainLayout => Some(NativeSurfaceCloseAction::RetainLayout),
            Self::CloseContent => Some(NativeSurfaceCloseAction::CloseContent),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NativeViewportCloseCancellationRecord {
    request: NativeViewportCloseRequest,
}

impl NativeViewportCloseCancellationRecord {
    pub(crate) fn from_eframe(request: NativeViewportCloseRequest) -> Self {
        Self { request }
    }

    pub(crate) const fn request(self) -> NativeViewportCloseRequest {
        self.request
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeCloseCaptureAdmission {
    First,
    Duplicate,
    Conflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeCloseCallbackAdmission {
    Unreserved,
    First,
    Duplicate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CancellationReceiptState {
    Awaiting,
    Received,
    Transferred,
    Committed,
}

/// The enclosing close capture owns the canonical key; this is never a second registry.
#[derive(Debug)]
struct CancellationReceipt {
    state: CancellationReceiptState,
    ingress: Option<ReservedHostRecord>,
}

impl CancellationReceipt {
    const fn new() -> Self {
        Self {
            state: CancellationReceiptState::Awaiting,
            ingress: None,
        }
    }

    fn receive(
        &mut self,
        reserve: impl FnOnce() -> ReservedHostRecord,
    ) -> NativeCloseCallbackAdmission {
        if self.state != CancellationReceiptState::Awaiting {
            return NativeCloseCallbackAdmission::Duplicate;
        }
        let ingress = reserve();
        self.ingress = Some(ingress);
        self.state = CancellationReceiptState::Received;
        NativeCloseCallbackAdmission::First
    }

    fn take_ingress(&mut self, ordinal: u64) -> Option<ReservedHostRecord> {
        if self.state != CancellationReceiptState::Received
            || self.ingress.as_ref()?.ordinal() != ordinal
        {
            return None;
        }
        let ingress = self.ingress.take()?;
        self.state = CancellationReceiptState::Transferred;
        Some(ingress)
    }

    fn commit_transferred(&mut self) {
        if self.state == CancellationReceiptState::Transferred {
            self.state = CancellationReceiptState::Committed;
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct CapturedNativeClose {
    held_request: Option<NativeViewportCloseRequest>,
    event_ordinal: u64,
    viewport: ViewportId,
    window: WindowId,
}

impl CapturedNativeClose {
    fn from_event(event: &NativeWindowEventRecord, binding: NativeSurfaceBinding) -> Option<Self> {
        if !matches!(event.event(), winit::event::WindowEvent::CloseRequested)
            || event.binding() != Some(binding)
            || event.ordinal() == 0
        {
            return None;
        }
        let viewport = event.viewport_id()?;
        let held_request = event.close_request();
        if held_request.is_some_and(|request| {
            request.viewport_id() != viewport
                || request.window_id() != event.window_id()
                || request.event().get() > event.ordinal()
        }) {
            return None;
        }
        Some(Self {
            held_request,
            event_ordinal: held_request.map_or(event.ordinal(), |request| request.event().get()),
            viewport,
            window: event.window_id(),
        })
    }
}

#[derive(Debug)]
struct PendingNativeClose {
    capture: CapturedNativeClose,
    request_recorded: bool,
    request: Option<NativeSurfaceCloseRequest>,
    accepted: bool,
    cancellation: Option<NativeCloseEffectAcknowledgement>,
    cancellation_receipt: Option<CancellationReceipt>,
    clear_recorded: bool,
    policy_recorded: bool,
    decision_plan: Option<DockspaceClosePlan>,
    successor: Option<CapturedNativeClose>,
    // A successor waits here, rather than at the raw journal head, until its
    // predecessor's Clear commits. Its original journal identity and age survive.
    successor_ingress: Option<ReservedHostRecord>,
    promoted_ingress: Option<ReservedHostRecord>,
}

impl PendingNativeClose {
    fn new(capture: CapturedNativeClose) -> Self {
        Self {
            capture,
            request_recorded: false,
            request: None,
            accepted: false,
            cancellation: None,
            cancellation_receipt: None,
            clear_recorded: false,
            policy_recorded: false,
            decision_plan: None,
            successor: None,
            successor_ingress: None,
            promoted_ingress: None,
        }
    }

    fn matches_cancellation(&self, record: NativeViewportCloseCancellationRecord) -> bool {
        self.capture.held_request == Some(record.request)
    }

    fn contains_held_request(&self, request: NativeViewportCloseRequest) -> bool {
        self.capture.held_request == Some(request)
            || self
                .successor
                .as_ref()
                .is_some_and(|successor| successor.held_request == Some(request))
    }

    fn clear_can_settle(&self) -> bool {
        self.clear_recorded
            && (self.capture.held_request.is_none() || self.cancellation_receipt.is_some())
            && self
                .cancellation_receipt
                .as_ref()
                .is_none_or(|receipt| receipt.state == CancellationReceiptState::Committed)
    }

    fn matches_destroyed(
        &self,
        event: &NativeWindowEventRecord,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.accepted
            && event.ordinal() > self.capture.event_ordinal
            && event.viewport_id() == Some(self.capture.viewport)
            && event.window_id() == self.capture.window
            && event.binding() == Some(binding)
    }
}

#[derive(Debug, Default)]
pub(crate) struct NativeCloseControl {
    pending: BTreeMap<NativeSurfaceBinding, PendingNativeClose>,
    progress: crate::lifecycle_progress::NativeCloseProgress,
}

impl NativeCloseControl {
    pub(crate) const fn progress(&self) -> crate::lifecycle_progress::NativeCloseProgress {
        self.progress
    }

    pub(crate) fn has_pending_work(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Reserves one exact close before ingress returns HoldClose.
    ///
    /// Only cancellation permits one newer capture on the same physical window.
    /// That successor cannot publish Requested or bind core until Clear settles.
    pub(crate) fn capture_request(
        &mut self,
        event: &NativeWindowEventRecord,
        binding: NativeSurfaceBinding,
    ) -> NativeCloseCaptureAdmission {
        let Some(capture) = CapturedNativeClose::from_event(event, binding) else {
            return NativeCloseCaptureAdmission::Conflict;
        };
        if capture.held_request.is_some_and(|request| {
            self.pending.iter().any(|(existing, pending)| {
                *existing != binding && pending.contains_held_request(request)
            })
        }) {
            return NativeCloseCaptureAdmission::Conflict;
        }
        let Some(pending) = self.pending.get_mut(&binding) else {
            self.pending
                .insert(binding, PendingNativeClose::new(capture));
            self.progress.record_request();
            return NativeCloseCaptureAdmission::First;
        };
        let matches = |existing: &CapturedNativeClose| {
            if capture.held_request.is_some() {
                existing.held_request == capture.held_request
            } else {
                *existing == capture
            }
        };
        if matches(&pending.capture) || pending.successor.as_ref().is_some_and(matches) {
            return NativeCloseCaptureAdmission::Duplicate;
        }
        if pending.accepted
            || pending.cancellation.is_none()
            || pending.successor.is_some()
            || pending.capture.viewport != capture.viewport
            || pending.capture.window != capture.window
            || pending.capture.held_request.is_some() != capture.held_request.is_some()
            || capture.event_ordinal <= pending.capture.event_ordinal
        {
            return NativeCloseCaptureAdmission::Conflict;
        }
        pending.successor = Some(capture);
        self.progress.record_request();
        NativeCloseCaptureAdmission::First
    }

    pub(crate) fn retain_successor_ingress(
        &mut self,
        binding: NativeSurfaceBinding,
        record: ReservedHostRecord,
    ) -> Result<(), ReservedHostRecord> {
        let Some(capture) = record
            .window_event()
            .and_then(|event| CapturedNativeClose::from_event(event, binding))
        else {
            return Err(record);
        };
        let Some(pending) = self.pending.get_mut(&binding) else {
            return Err(record);
        };
        if pending.successor.as_ref() != Some(&capture) || pending.successor_ingress.is_some() {
            return Err(record);
        }
        pending.successor_ingress = Some(record);
        Ok(())
    }

    pub(crate) fn parked_ingress(&self) -> impl Iterator<Item = &ReservedHostRecord> {
        self.pending
            .values()
            .filter_map(|pending| pending.successor_ingress.as_ref())
    }

    pub(crate) fn callback_ingress(&self) -> impl Iterator<Item = &ReservedHostRecord> {
        self.pending.values().flat_map(|pending| {
            pending
                .cancellation_receipt
                .as_ref()
                .and_then(|receipt| receipt.ingress.as_ref())
                .into_iter()
                .chain(pending.promoted_ingress.iter())
        })
    }

    pub(crate) fn take_callback_ingress(&mut self, ordinal: u64) -> Option<ReservedHostRecord> {
        self.pending.values_mut().find_map(|pending| {
            if let Some(ingress) = pending
                .cancellation_receipt
                .as_mut()
                .and_then(|receipt| receipt.take_ingress(ordinal))
            {
                return Some(ingress);
            }
            if pending
                .promoted_ingress
                .as_ref()
                .is_some_and(|ingress| ingress.ordinal() == ordinal)
            {
                return pending.promoted_ingress.take();
            }
            None
        })
    }

    pub(crate) fn deferred_ingress(&self) -> impl Iterator<Item = &ReservedHostRecord> {
        self.pending.values().flat_map(|pending| {
            pending
                .successor_ingress
                .iter()
                .chain(pending.promoted_ingress.iter())
        })
    }

    pub(crate) fn needs_request_recording(
        &self,
        event: &NativeWindowEventRecord,
        binding: NativeSurfaceBinding,
    ) -> bool {
        let Some(capture) = CapturedNativeClose::from_event(event, binding) else {
            return false;
        };
        self.pending.get(&binding).is_some_and(|pending| {
            pending.capture == capture
                && !pending.request_recorded
                && !pending.accepted
                && pending.cancellation.is_none()
        })
    }

    /// Called only after the exact Requested observation entered the core recorder.
    pub(crate) fn mark_request_recorded(
        &mut self,
        event: &NativeWindowEventRecord,
        binding: NativeSurfaceBinding,
    ) -> bool {
        if !self.needs_request_recording(event, binding) {
            return false;
        }
        self.pending
            .get_mut(&binding)
            .expect("the exact unrecorded capture remains present")
            .request_recorded = true;
        true
    }

    /// Tokenless unit-model shortcut; production captures before returning Hold.
    #[cfg(test)]
    pub(crate) fn observe_request(
        &mut self,
        event: &NativeWindowEventRecord,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.capture_request(event, binding) == NativeCloseCaptureAdmission::First
            && self.mark_request_recorded(event, binding)
    }

    pub(crate) fn bind_request(&mut self, request: NativeSurfaceCloseRequest) -> Result<bool, ()> {
        let Some(pending) = self.pending.get_mut(&request.binding()) else {
            return Ok(false);
        };
        if !pending.request_recorded {
            return Ok(false);
        }
        match pending.request {
            Some(existing) if existing == request => Ok(true),
            Some(_) => Err(()),
            None => {
                pending.request = Some(request);
                Ok(true)
            }
        }
    }

    pub(crate) fn unresolved_requests(
        &self,
    ) -> impl Iterator<Item = NativeSurfaceCloseRequest> + '_ {
        self.pending.values().filter_map(|pending| {
            (!pending.accepted && !pending.clear_recorded && pending.cancellation.is_none())
                .then_some(pending.request)
                .flatten()
        })
    }

    pub(crate) fn unsubmitted_requests(
        &self,
    ) -> impl Iterator<Item = NativeSurfaceCloseRequest> + '_ {
        self.unresolved_requests().filter(|request| {
            self.pending
                .get(&request.binding())
                .is_some_and(|pending| !pending.policy_recorded)
        })
    }

    pub(crate) fn mark_policy_recorded(&mut self, request: NativeSurfaceCloseRequest) {
        if let Some(pending) = self.pending.get_mut(&request.binding())
            && pending.request == Some(request)
        {
            pending.policy_recorded = true;
        }
    }

    pub(crate) fn retain_decision_plan(
        &mut self,
        request: NativeSurfaceCloseRequest,
        plan: &DockspaceClosePlan,
    ) {
        if let Some(pending) = self.pending.get_mut(&request.binding())
            && pending.request == Some(request)
            && pending.policy_recorded
            && !pending.accepted
            && pending.cancellation.is_none()
        {
            pending.decision_plan = plan
                .items()
                .iter()
                .any(|item| item.state() == CloseItemDecisionState::Pending)
                .then(|| plan.clone());
        }
    }

    pub(crate) fn decision_plans(&self) -> Vec<DockspaceClosePlan> {
        self.pending
            .values()
            .filter_map(|pending| pending.decision_plan.clone())
            .collect()
    }

    pub(crate) fn settle_decision_plans(&mut self, plans: &[DockspaceClosePlan]) {
        for pending in self.pending.values_mut() {
            if pending.decision_plan.as_ref().is_some_and(|plan| {
                plans
                    .iter()
                    .any(|submitted| submitted.request() == plan.request())
            }) {
                pending.decision_plan = None;
            }
        }
    }

    pub(crate) fn has_runnable_policy_work(&self) -> bool {
        self.unsubmitted_requests().next().is_some()
            || self
                .pending
                .values()
                .any(|pending| pending.decision_plan.is_some())
    }

    pub(crate) fn request_for_binding(
        &self,
        binding: NativeSurfaceBinding,
    ) -> Option<NativeSurfaceCloseRequest> {
        self.pending.get(&binding)?.request
    }

    pub(crate) fn held_request(
        &self,
        request: NativeSurfaceCloseRequest,
    ) -> Option<NativeViewportCloseRequest> {
        let pending = self.pending.get(&request.binding())?;
        (pending.request == Some(request)
            && !pending.accepted
            && pending.cancellation.is_none()
            && !pending.clear_recorded)
            .then_some(pending.capture.held_request)
            .flatten()
    }

    pub(crate) fn reserve_cancellation_callback(
        &mut self,
        binding: NativeSurfaceBinding,
        request: NativeViewportCloseRequest,
    ) -> bool {
        let Some(pending) = self.pending.get_mut(&binding) else {
            return false;
        };
        if pending.capture.held_request != Some(request)
            || pending.request.is_none()
            || pending.accepted
            || pending.clear_recorded
        {
            return false;
        }
        pending
            .cancellation_receipt
            .get_or_insert_with(CancellationReceipt::new);
        true
    }

    pub(crate) fn cancel_cancellation_callback_reservation(
        &mut self,
        binding: NativeSurfaceBinding,
        request: NativeViewportCloseRequest,
    ) {
        if let Some(pending) = self.pending.get_mut(&binding)
            && pending.capture.held_request == Some(request)
            && pending.cancellation.is_none()
            && pending
                .cancellation_receipt
                .as_ref()
                .is_some_and(|receipt| receipt.state == CancellationReceiptState::Awaiting)
        {
            pending.cancellation_receipt = None;
        }
    }

    pub(crate) fn receive_cancellation_callback(
        &mut self,
        record: NativeViewportCloseCancellationRecord,
        reserve: impl FnOnce() -> ReservedHostRecord,
    ) -> NativeCloseCallbackAdmission {
        self.pending
            .values_mut()
            .find(|pending| pending.matches_cancellation(record))
            .and_then(|pending| pending.cancellation_receipt.as_mut())
            .map_or(NativeCloseCallbackAdmission::Unreserved, |receipt| {
                receipt.receive(reserve)
            })
    }

    /// Full frame boundaries alone commit a transferred cancellation callback.
    /// Semantic Clear settlement remains a separate requirement for releasing it.
    pub(crate) fn commit_transferred_callbacks(&mut self) {
        for pending in self.pending.values_mut() {
            if let Some(receipt) = pending.cancellation_receipt.as_mut() {
                receipt.commit_transferred();
            }
        }
    }

    pub(crate) fn begin_cancellation(
        &mut self,
        request: NativeSurfaceCloseRequest,
        acknowledgement: NativeCloseEffectAcknowledgement,
    ) -> Option<ViewportId> {
        let pending = self.pending.get_mut(&request.binding())?;
        if pending.request != Some(request)
            || pending.accepted
            || pending.cancellation.is_some()
            || pending.clear_recorded
        {
            return None;
        }
        pending.cancellation = Some(acknowledgement);
        Some(pending.capture.viewport)
    }

    pub(crate) fn accept_close(&mut self, request: NativeSurfaceCloseRequest) -> bool {
        let Some(pending) = self.pending.get_mut(&request.binding()) else {
            return false;
        };
        if pending.request != Some(request)
            || pending.accepted
            || pending.cancellation.is_some()
            || pending.cancellation_receipt.is_some()
            || pending.clear_recorded
        {
            return false;
        }
        pending.accepted = true;
        true
    }

    pub(crate) fn accepted_destroyed_matches(
        &self,
        event: &NativeWindowEventRecord,
        binding: NativeSurfaceBinding,
    ) -> Result<bool, ()> {
        let Some(pending) = self.pending.get(&binding) else {
            return Ok(false);
        };
        if !pending.accepted {
            return Ok(false);
        }
        if pending.matches_destroyed(event, binding) {
            Ok(true)
        } else {
            Err(())
        }
    }

    pub(crate) fn settle_accepted_destroyed(
        &mut self,
        event: &NativeWindowEventRecord,
        binding: NativeSurfaceBinding,
    ) -> bool {
        if !self
            .pending
            .get(&binding)
            .is_some_and(|pending| pending.matches_destroyed(event, binding))
        {
            return false;
        }
        self.pending.remove(&binding).is_some()
    }

    pub(crate) fn cancellation_observation(
        &self,
        record: NativeViewportCloseCancellationRecord,
    ) -> Option<(NativeSurfaceBinding, NativeCloseEffectAcknowledgement)> {
        self.pending.iter().find_map(|(binding, pending)| {
            if pending.clear_recorded
                || !pending.matches_cancellation(record)
                || pending
                    .cancellation_receipt
                    .as_ref()
                    .is_some_and(|receipt| receipt.state != CancellationReceiptState::Received)
            {
                return None;
            }
            pending
                .cancellation
                .map(|acknowledgement| (*binding, acknowledgement))
        })
    }

    pub(crate) fn mark_clear_recorded(&mut self, binding: NativeSurfaceBinding) -> bool {
        let Some(pending) = self.pending.get_mut(&binding) else {
            return false;
        };
        if pending.cancellation.is_none()
            || pending.clear_recorded
            || (pending.capture.held_request.is_some() && pending.cancellation_receipt.is_none())
            || pending
                .cancellation_receipt
                .as_ref()
                .is_some_and(|receipt| receipt.state != CancellationReceiptState::Transferred)
        {
            return false;
        }
        pending.clear_recorded = true;
        true
    }

    /// Returns the number of clear observations covered by this settlement.
    /// A rejected settlement retains them for retry without counting a commit.
    pub(crate) fn settle_clear(&mut self, applied: bool) -> usize {
        if !applied {
            return self
                .pending
                .values()
                .filter(|pending| pending.clear_recorded)
                .count();
        }
        let mut count = 0;
        self.pending.retain(|_, pending| {
            if !pending.clear_can_settle() {
                return true;
            }
            count += 1;
            if let Some(successor) = pending.successor.take() {
                let ingress = pending.successor_ingress.take();
                *pending = PendingNativeClose::new(successor);
                pending.promoted_ingress = ingress;
                true
            } else {
                false
            }
        });
        self.progress.record_cancellations(count);
        count
    }

    /// Retires close correlation after the exact native route reaches its
    /// terminal destroyed state.
    pub(crate) fn retire_binding(&mut self, binding: NativeSurfaceBinding) {
        self.pending.remove(&binding);
    }

    pub(crate) fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.pending.contains_key(&binding)
    }
}

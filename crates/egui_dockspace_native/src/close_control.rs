//! Exact native close correlation owned by the eframe adapter.

use dockspace::close::{CloseItemDecisionState, CloseRequestId};
use std::mem::size_of;

use dockspace::runtime::{
    DockspaceClosePlan, NativeCloseEffectAcknowledgement, NativeSurfaceBinding,
    NativeSurfaceCloseAction, NativeSurfaceCloseRequest,
};
use eframe::{NativeViewportCloseRequest, egui::ViewportId};
use winit::window::WindowId;

use crate::error::NativeHostProtocolError;
use crate::event::NativeWindowEventRecord;
use crate::mailbox::journal::ReservedHostRecord;

#[cfg(test)]
mod storage_tests;
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

    fn matches(&self, incoming: &Self) -> bool {
        if incoming.held_request.is_some() {
            self.held_request == incoming.held_request
        } else {
            self == incoming
        }
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
    decision_request: Option<CloseRequestId>,
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
            decision_request: None,
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
    pending: Vec<(NativeSurfaceBinding, PendingNativeClose)>,
    entry_limit: Option<usize>,
    progress: crate::lifecycle_progress::NativeCloseProgress,
}

impl NativeCloseControl {
    /// Allows the first finite configuration only before any retained close.
    /// Selected limits and cumulative lifecycle progress cannot be reset.
    pub(crate) fn can_configure_capacity_limit(&self) -> bool {
        self.entry_limit.is_none()
            && self.pending.is_empty()
            && self.progress.requests_retained() == 0
    }

    /// Prepays independent exact-key slots, including retained provider aliases.
    pub(crate) fn with_capacity_limit(entries: usize) -> Result<Self, NativeHostProtocolError> {
        Self::reservation_bytes(entries)
            .ok_or(NativeHostProtocolError::NativeReportStorageUnavailable)?;
        let mut pending = Vec::new();
        pending
            .try_reserve_exact(entries)
            .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
        if pending.capacity() > entries {
            return Err(NativeHostProtocolError::NativeReportStorageUnavailable);
        }
        Ok(Self {
            pending,
            entry_limit: Some(entries),
            progress: crate::lifecycle_progress::NativeCloseProgress::default(),
        })
    }

    /// Includes inline state and selected backing, without allocator metadata.
    pub(crate) fn reservation_bytes(entries: usize) -> Option<usize> {
        size_of::<Self>().checked_add(
            entries.checked_mul(size_of::<(NativeSurfaceBinding, PendingNativeClose)>())?,
        )
    }

    /// Counts inline state and actual capacity, including vacant reusable slots.
    /// Receipts, parked ingress and decision metadata remain inline in each entry.
    pub(crate) fn owned_data_capacity_bytes(&self) -> usize {
        size_of::<Self>().saturating_add(
            self.pending
                .capacity()
                .saturating_mul(size_of::<(NativeSurfaceBinding, PendingNativeClose)>()),
        )
    }

    fn pending_for_binding(&self, binding: NativeSurfaceBinding) -> Option<&PendingNativeClose> {
        let index = self
            .pending
            .binary_search_by_key(&binding, |(binding, _)| *binding)
            .ok()?;
        Some(&self.pending[index].1)
    }

    fn pending_for_binding_mut(
        &mut self,
        binding: NativeSurfaceBinding,
    ) -> Option<&mut PendingNativeClose> {
        let index = self
            .pending
            .binary_search_by_key(&binding, |(binding, _)| *binding)
            .ok()?;
        Some(&mut self.pending[index].1)
    }

    pub(crate) const fn progress(&self) -> crate::lifecycle_progress::NativeCloseProgress {
        self.progress
    }

    pub(crate) fn has_pending_work(&self) -> bool {
        !self.pending.is_empty()
    }

    pub(crate) fn has_unsettled_clear(&self) -> bool {
        self.pending
            .iter()
            .any(|(_, pending)| pending.clear_recorded)
    }

    pub(crate) fn retains_request(
        &self,
        event: &NativeWindowEventRecord,
        binding: NativeSurfaceBinding,
    ) -> bool {
        let Some(capture) = CapturedNativeClose::from_event(event, binding) else {
            return false;
        };
        self.pending_for_binding(binding).is_some_and(|pending| {
            pending.capture.matches(&capture)
                || pending
                    .successor
                    .as_ref()
                    .is_some_and(|successor| successor.matches(&capture))
        })
    }

    /// Validates and reserves backing before ingress promises HoldClose. This
    /// does not capture the request or advance progress; the caller must keep
    /// admission and capture serialized on this owner before returning Hold.
    pub(crate) fn preflight_capture(
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
        let Some(pending) = self.pending_for_binding(binding) else {
            if self
                .entry_limit
                .is_some_and(|limit| self.pending.len() >= limit)
                || self.pending.len() == self.pending.capacity()
                    && (self.entry_limit.is_some() || self.pending.try_reserve(1).is_err())
            {
                return NativeCloseCaptureAdmission::Conflict;
            }
            return NativeCloseCaptureAdmission::First;
        };
        if pending.capture.matches(&capture)
            || pending
                .successor
                .as_ref()
                .is_some_and(|successor| successor.matches(&capture))
        {
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
        NativeCloseCaptureAdmission::First
    }

    /// Captures one exact close before ingress returns HoldClose. Full storage
    /// still admits exact duplicates and the existing owner's successor slot.
    /// Only cancellation permits that successor; Requested waits for Clear.
    pub(crate) fn capture_request(
        &mut self,
        event: &NativeWindowEventRecord,
        binding: NativeSurfaceBinding,
    ) -> NativeCloseCaptureAdmission {
        let admission = self.preflight_capture(event, binding);
        if admission != NativeCloseCaptureAdmission::First {
            return admission;
        }
        let capture = CapturedNativeClose::from_event(event, binding)
            .expect("preflight validated the exact close capture");
        match self
            .pending
            .binary_search_by_key(&binding, |(binding, _)| *binding)
        {
            Ok(index) => self.pending[index].1.successor = Some(capture),
            Err(index) => self
                .pending
                .insert(index, (binding, PendingNativeClose::new(capture))),
        }
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
        let Some(pending) = self.pending_for_binding_mut(binding) else {
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
            .iter()
            .filter_map(|(_, pending)| pending.successor_ingress.as_ref())
    }

    pub(crate) fn callback_ingress(&self) -> impl Iterator<Item = &ReservedHostRecord> {
        self.pending.iter().flat_map(|(_, pending)| {
            pending
                .cancellation_receipt
                .as_ref()
                .and_then(|receipt| receipt.ingress.as_ref())
                .into_iter()
                .chain(pending.promoted_ingress.iter())
        })
    }

    pub(crate) fn take_callback_ingress(&mut self, ordinal: u64) -> Option<ReservedHostRecord> {
        self.pending.iter_mut().find_map(|(_, pending)| {
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
        self.pending.iter().flat_map(|(_, pending)| {
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
        self.pending_for_binding(binding).is_some_and(|pending| {
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
        self.pending_for_binding_mut(binding)
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
        let Some(pending) = self.pending_for_binding_mut(request.binding()) else {
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
        self.pending.iter().filter_map(|(_, pending)| {
            (!pending.accepted && !pending.clear_recorded && pending.cancellation.is_none())
                .then_some(pending.request)
                .flatten()
        })
    }

    pub(crate) fn unsubmitted_requests(
        &self,
    ) -> impl Iterator<Item = NativeSurfaceCloseRequest> + '_ {
        self.unresolved_requests().filter(|request| {
            self.pending_for_binding(request.binding())
                .is_some_and(|pending| !pending.policy_recorded)
        })
    }

    pub(crate) fn mark_policy_recorded(&mut self, request: NativeSurfaceCloseRequest) {
        if let Some(pending) = self.pending_for_binding_mut(request.binding())
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
        if let Some(pending) = self.pending_for_binding_mut(request.binding())
            && pending.request == Some(request)
            && pending.policy_recorded
            && !pending.accepted
            && pending.cancellation.is_none()
        {
            pending.decision_request = (plan.phase().requires_decision()
                && plan
                    .items()
                    .iter()
                    .any(|item| item.state() == CloseItemDecisionState::Pending))
            .then_some(plan.request());
        }
    }

    pub(crate) fn decision_requests(&self) -> impl Iterator<Item = CloseRequestId> + '_ {
        self.pending
            .iter()
            .filter_map(|(_, pending)| pending.decision_request)
    }

    /// Counts retained decision identifiers without constructing a snapshot.
    pub(crate) fn decision_count(&self) -> usize {
        self.decision_requests().count()
    }

    /// Releases only requests proven complete by a successfully committed frame.
    pub(crate) fn settle_decision_requests(&mut self, requests: &[CloseRequestId]) {
        for (_, pending) in &mut self.pending {
            if pending
                .decision_request
                .is_some_and(|request| requests.contains(&request))
            {
                pending.decision_request = None;
            }
        }
    }

    pub(crate) fn has_runnable_policy_work(&self) -> bool {
        self.unsubmitted_requests().next().is_some()
            || self
                .pending
                .iter()
                .any(|(_, pending)| pending.decision_request.is_some())
    }

    pub(crate) fn request_for_binding(
        &self,
        binding: NativeSurfaceBinding,
    ) -> Option<NativeSurfaceCloseRequest> {
        self.pending_for_binding(binding)?.request
    }

    pub(crate) fn held_request(
        &self,
        request: NativeSurfaceCloseRequest,
    ) -> Option<NativeViewportCloseRequest> {
        let pending = self.pending_for_binding(request.binding())?;
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
        let Some(pending) = self.pending_for_binding_mut(binding) else {
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
        if let Some(pending) = self.pending_for_binding_mut(binding)
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
            .iter_mut()
            .find(|(_, pending)| pending.matches_cancellation(record))
            .and_then(|(_, pending)| pending.cancellation_receipt.as_mut())
            .map_or(NativeCloseCallbackAdmission::Unreserved, |receipt| {
                receipt.receive(reserve)
            })
    }

    /// Full frame boundaries alone commit a transferred cancellation callback.
    /// Semantic Clear settlement remains a separate requirement for releasing it.
    pub(crate) fn commit_transferred_callbacks(&mut self) {
        for (_, pending) in &mut self.pending {
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
        let pending = self.pending_for_binding_mut(request.binding())?;
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
        let Some(pending) = self.pending_for_binding_mut(request.binding()) else {
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
        let Some(pending) = self.pending_for_binding(binding) else {
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
        let Ok(index) = self
            .pending
            .binary_search_by_key(&binding, |(binding, _)| *binding)
        else {
            return false;
        };
        if !self.pending[index].1.matches_destroyed(event, binding) {
            return false;
        }
        self.pending.remove(index);
        true
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
        let Some(pending) = self.pending_for_binding_mut(binding) else {
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
                .iter()
                .filter(|(_, pending)| pending.clear_recorded)
                .count();
        }
        let mut count = 0;
        self.pending.retain_mut(|(_, pending)| {
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
        if let Ok(index) = self
            .pending
            .binary_search_by_key(&binding, |(binding, _)| *binding)
        {
            self.pending.remove(index);
        }
    }

    pub(crate) fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.pending_for_binding(binding).is_some()
    }
}

#[cfg(test)]
mod capacity_tests {
    use dockspace::model::{
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
        RootId, SurfaceId,
    };
    use dockspace::policy::DockPolicy;
    use dockspace::runtime::{
        DockspaceSession, HostFrameReport, HostInputOutcome, HostWindowToken, NativeCloseState,
        NativeEffectAcknowledgement, NativeMonotonicClock, NativePointerRoster, NativeWindowFacts,
        NativeWorkAreaRoster, SurfaceUnavailableReason,
    };
    use winit::event::WindowEvent;

    use super::*;
    use crate::mailbox::{HostRecord, journal::HostIngressJournal};

    pub(super) fn commit(session: &mut DockspaceSession) -> HostFrameReport {
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        frame.commit().unwrap()
    }

    pub(super) fn fixture(item_count: u64) -> (DockspaceSession, NativeSurfaceBinding) {
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SurfaceId::new(1),
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs((1..=item_count).map(ItemId::new)),
            ),
        )])
        .unwrap();
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
        session
            .enable_managed_native_host(
                dockspace::runtime::NativeCreateStrategy::Staged,
                NativePointerRoster::Exact(Vec::new()),
            )
            .unwrap();
        session
            .configure_managed_native_capabilities(crate::capabilities::capabilities_for_backend(
                eframe::NativeWindowingBackend::Windows,
            ))
            .unwrap();
        session
            .register_native_root(SurfaceId::new(1), HostWindowToken::new(11))
            .unwrap();
        let binding = commit(&mut session)
            .inputs()
            .iter()
            .find_map(|input| match input {
                HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
                _ => None,
            })
            .unwrap();
        session
            .report_managed_native_snapshot(
                [(binding, NativeWindowFacts::live())],
                NativeWorkAreaRoster::Unknown,
            )
            .unwrap();
        commit(&mut session);
        (session, binding)
    }

    pub(super) fn request(
        session: &mut DockspaceSession,
        binding: NativeSurfaceBinding,
    ) -> NativeSurfaceCloseRequest {
        session
            .publish_native_close(binding, NativeCloseState::Requested, None)
            .unwrap();
        commit(session)
            .inputs()
            .iter()
            .find_map(|input| match input {
                HostInputOutcome::NativeCloseObservationApplied { close_requests } => {
                    close_requests.first().copied()
                }
                _ => None,
            })
            .unwrap()
    }

    fn event(binding: NativeSurfaceBinding) -> NativeWindowEventRecord {
        NativeWindowEventRecord::for_test(
            1,
            WindowId::from(11),
            Some(ViewportId::ROOT),
            Some(binding),
            WindowEvent::CloseRequested,
        )
    }

    #[test]
    fn owned_data_capacity_keeps_close_entries_until_cancellation_commit() {
        let (mut session, binding) = fixture(1);
        let (_, other) = fixture(1);
        let close = request(&mut session, binding);
        let mut control = NativeCloseControl::default();
        let empty = control.owned_data_capacity_bytes();
        assert_eq!(empty, size_of::<NativeCloseControl>());
        assert!(control.observe_request(&event(binding), binding));
        assert_eq!(control.bind_request(close), Ok(true));
        let one = control.owned_data_capacity_bytes();
        assert!(one > empty);
        assert!(control.observe_request(&event(other), other));
        let retained = control.owned_data_capacity_bytes();
        assert!(retained >= one);
        control.retire_binding(other);
        assert_eq!(control.owned_data_capacity_bytes(), retained);

        session.cancel_native_surface_close(close).unwrap();
        let effect = commit(&mut session).take_native_effects().pop().unwrap();
        let Some(NativeEffectAcknowledgement::Close(acknowledgement)) = effect.accepted() else {
            panic!("cancellation must provide its close acknowledgement")
        };
        assert_eq!(
            control.begin_cancellation(close, acknowledgement),
            Some(ViewportId::ROOT)
        );
        let mut journal = HostIngressJournal::new(NativeMonotonicClock::system());
        let mut receipt = CancellationReceipt::new();
        // Exercise the inline receipt model without fabricating an opaque fork token.
        assert_eq!(
            receipt.receive(|| journal.reserve(HostRecord::WindowEvent(event(binding)))),
            NativeCloseCallbackAdmission::First,
        );
        control
            .pending_for_binding_mut(binding)
            .unwrap()
            .cancellation_receipt = Some(receipt);
        let ordinal = control.callback_ingress().next().unwrap().ordinal();
        assert_eq!(control.owned_data_capacity_bytes(), retained);
        control.commit_transferred_callbacks();
        assert_eq!(control.owned_data_capacity_bytes(), retained);
        assert!(control.take_callback_ingress(ordinal).is_some());
        assert!(control.mark_clear_recorded(binding));
        assert_eq!(control.settle_clear(false), 1);
        assert_eq!(control.settle_clear(true), 0);
        assert_eq!(control.owned_data_capacity_bytes(), retained);
        control.commit_transferred_callbacks();
        assert_eq!(control.owned_data_capacity_bytes(), retained);
        assert_eq!(control.settle_clear(true), 1);
        assert!(!control.has_pending_work());
        assert_eq!(control.owned_data_capacity_bytes(), retained);
    }

    #[test]
    fn owned_data_capacity_keeps_fixed_decision_metadata_until_exact_retirement() {
        let (_, foreign) = fixture(1);
        let mut metadata_bytes = None;
        for item_count in [1, 3, 1024] {
            let (mut session, binding) = fixture(item_count);
            let close = request(&mut session, binding);
            let mut control = NativeCloseControl::default();
            let empty = control.owned_data_capacity_bytes();
            assert!(control.observe_request(&event(binding), binding));
            assert_eq!(control.bind_request(close), Ok(true));
            control.mark_policy_recorded(close);
            let inline = control.owned_data_capacity_bytes();
            session
                .request_native_surface_close(close, NativeSurfaceCloseAction::CloseContent)
                .unwrap();
            let report = commit(&mut session);
            let plan = report
                .inputs()
                .iter()
                .find_map(|input| match input {
                    HostInputOutcome::NativeSurfaceCloseRequested { plan, .. } => Some(plan),
                    _ => None,
                })
                .expect("the native close produces its content decision plan");
            assert_eq!(plan.items().len(), usize::try_from(item_count).unwrap());
            control.retain_decision_plan(close, plan);
            let decision_request = plan.request();
            drop(report);
            assert_eq!(
                control.decision_requests().collect::<Vec<_>>(),
                [decision_request]
            );
            assert_eq!(control.owned_data_capacity_bytes(), inline);
            assert_eq!(
                *metadata_bytes.get_or_insert(inline - empty),
                inline - empty
            );
            control.commit_transferred_callbacks();
            control.settle_decision_requests(&[]);
            control.retire_binding(foreign);
            assert_eq!(control.owned_data_capacity_bytes(), inline);
            assert_eq!(control.request_for_binding(binding), Some(close));
            assert_eq!(
                control.decision_requests().collect::<Vec<_>>(),
                [decision_request]
            );
            assert!(control.references_binding(binding));
            control.retire_binding(binding);
            assert_eq!(control.owned_data_capacity_bytes(), inline);
            assert_eq!(control.request_for_binding(binding), None);
            assert!(control.decision_requests().next().is_none());
            assert!(!control.references_binding(binding));
        }
    }
}

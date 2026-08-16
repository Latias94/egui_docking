//! Exact native close correlation owned by the eframe adapter.

use std::collections::BTreeMap;

use dockspace::runtime::{
    NativeCloseEffectAcknowledgement, NativeSurfaceBinding, NativeSurfaceCloseAction,
    NativeSurfaceCloseRequest,
};
use eframe::{NativeViewportCloseRequest, egui::ViewportId};
use winit::window::WindowId;

use crate::event::NativeWindowEventRecord;

/// Explicit product policy for an operating-system native close request.
///
/// The policy is intentionally small and synchronous so the adapter can
/// resolve the native event before returning from the same eframe update. It
/// never guesses a rehome target or silently destroys content.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NativeWindowClosePolicy {
    /// Keep the native window open.
    #[default]
    Cancel,
    /// Destroy the native binding while retaining its logical surface roster.
    RetainLayout,
    /// Close every pane owned by the native surface.
    CloseContent,
}

impl NativeWindowClosePolicy {
    pub(crate) const fn request(self) -> Option<NativeSurfaceCloseAction> {
        match self {
            Self::Cancel => None,
            Self::RetainLayout => Some(NativeSurfaceCloseAction::RetainLayout),
            Self::CloseContent => Some(NativeSurfaceCloseAction::CloseContent),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NativeViewportCloseCancellationRecord {
    event_ordinal: u64,
    viewport: ViewportId,
    window: WindowId,
    binding: NativeSurfaceBinding,
}

impl NativeViewportCloseCancellationRecord {
    pub(crate) fn from_eframe(
        request: NativeViewportCloseRequest,
        binding: NativeSurfaceBinding,
    ) -> Self {
        Self {
            event_ordinal: request.event().get(),
            viewport: request.viewport_id(),
            window: request.window_id(),
            binding,
        }
    }

    #[cfg(test)]
    pub(crate) const fn for_test(
        event_ordinal: u64,
        viewport: ViewportId,
        window: WindowId,
        binding: NativeSurfaceBinding,
    ) -> Self {
        Self {
            event_ordinal,
            viewport,
            window,
            binding,
        }
    }

    pub(crate) const fn binding(self) -> NativeSurfaceBinding {
        self.binding
    }
}

#[derive(Debug)]
struct PendingNativeClose {
    event_ordinal: u64,
    viewport: ViewportId,
    window: WindowId,
    request: Option<NativeSurfaceCloseRequest>,
    accepted: bool,
    cancellation: Option<NativeCloseEffectAcknowledgement>,
    clear_recorded: bool,
}

impl PendingNativeClose {
    fn matches_cancellation(&self, record: NativeViewportCloseCancellationRecord) -> bool {
        self.event_ordinal == record.event_ordinal
            && self.viewport == record.viewport
            && self.window == record.window
    }

    fn matches_destroyed(
        &self,
        event: &NativeWindowEventRecord,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.accepted
            && event.ordinal() > self.event_ordinal
            && event.viewport_id() == Some(self.viewport)
            && event.window_id() == self.window
            && event.binding() == Some(binding)
    }
}

#[derive(Debug, Default)]
pub(crate) struct NativeCloseControl {
    pending: BTreeMap<NativeSurfaceBinding, PendingNativeClose>,
}

impl NativeCloseControl {
    pub(crate) fn has_pending_work(&self) -> bool {
        !self.pending.is_empty()
    }

    pub(crate) fn observe_request(
        &mut self,
        event: &NativeWindowEventRecord,
        binding: NativeSurfaceBinding,
    ) {
        self.pending.entry(binding).or_insert(PendingNativeClose {
            event_ordinal: event.ordinal(),
            viewport: event
                .viewport_id()
                .expect("mapped native close events retain their viewport"),
            window: event.window_id(),
            request: None,
            accepted: false,
            cancellation: None,
            clear_recorded: false,
        });
    }

    pub(crate) fn bind_request(&mut self, request: NativeSurfaceCloseRequest) -> Result<bool, ()> {
        let Some(pending) = self.pending.get_mut(&request.binding()) else {
            return Ok(false);
        };
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

    pub(crate) fn recognizes_request(&self, request: NativeSurfaceCloseRequest) -> bool {
        self.pending
            .get(&request.binding())
            .is_some_and(|pending| pending.request == Some(request))
    }

    pub(crate) fn request_for_binding(
        &self,
        binding: NativeSurfaceBinding,
    ) -> Option<NativeSurfaceCloseRequest> {
        self.pending.get(&binding)?.request
    }

    pub(crate) fn cancellation_viewport(
        &self,
        request: NativeSurfaceCloseRequest,
    ) -> Option<ViewportId> {
        let pending = self.pending.get(&request.binding())?;
        (pending.request == Some(request)
            && !pending.accepted
            && pending.cancellation.is_none()
            && !pending.clear_recorded)
            .then_some(pending.viewport)
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
        Some(pending.viewport)
    }

    pub(crate) fn accept_close(&mut self, request: NativeSurfaceCloseRequest) -> bool {
        let Some(pending) = self.pending.get_mut(&request.binding()) else {
            return false;
        };
        if pending.request != Some(request)
            || pending.accepted
            || pending.cancellation.is_some()
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
    ) -> Option<NativeCloseEffectAcknowledgement> {
        let pending = self.pending.get(&record.binding)?;
        if pending.clear_recorded || !pending.matches_cancellation(record) {
            return None;
        }
        pending.cancellation
    }

    pub(crate) fn mark_clear_recorded(&mut self, binding: NativeSurfaceBinding) -> bool {
        let Some(pending) = self.pending.get_mut(&binding) else {
            return false;
        };
        if pending.cancellation.is_none() || pending.clear_recorded {
            return false;
        }
        pending.clear_recorded = true;
        true
    }

    pub(crate) fn settle_clear(&mut self, applied: bool) -> bool {
        let bindings = self
            .pending
            .iter()
            .filter_map(|(binding, pending)| pending.clear_recorded.then_some(*binding))
            .collect::<Vec<_>>();
        if applied {
            for binding in &bindings {
                self.pending.remove(binding);
            }
        }
        !bindings.is_empty()
    }

    pub(crate) fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.pending.contains_key(&binding)
    }
}

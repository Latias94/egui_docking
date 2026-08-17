//! Exact native-focus command and callback correlation.

use dockspace::runtime::{NativeEffectRequest, NativeGlobalFocus, NativeSurfaceBinding};
use eframe::{
    NativeGlobalFocus as EframeGlobalFocus, NativeGlobalFocusObservation,
    NativeViewportFocusResult, NativeViewportFocusStatus, egui::ViewportId,
};
use winit::window::WindowId;

use crate::viewport_map::NativeViewportMap;

/// One globally ordered focus fact frozen against the callback-time route map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeGlobalFocusRecord {
    event: u64,
    focus: NativeGlobalFocus,
}

impl NativeGlobalFocusRecord {
    pub(crate) fn capture(
        observation: NativeGlobalFocusObservation,
        viewports: &NativeViewportMap,
    ) -> Self {
        Self::from_focus(observation.event().get(), observation.focused(), viewports)
    }

    fn from_focus(event: u64, focused: EframeGlobalFocus, viewports: &NativeViewportMap) -> Self {
        let focus = match focused {
            EframeGlobalFocus::Viewport {
                viewport_id,
                window_id,
            } => viewports
                .focus_binding_for_event(window_id, Some(viewport_id))
                .map_or(NativeGlobalFocus::Unknown, NativeGlobalFocus::Dock),
            EframeGlobalFocus::Unknown => NativeGlobalFocus::Unknown,
        };
        Self { event, focus }
    }

    #[cfg(test)]
    pub(crate) const fn for_test(event: u64, focus: NativeGlobalFocus) -> Self {
        Self { event, focus }
    }

    #[cfg(test)]
    pub(crate) fn capture_for_test(
        event: u64,
        focused: EframeGlobalFocus,
        viewports: &NativeViewportMap,
    ) -> Self {
        Self::from_focus(event, focused, viewports)
    }

    pub(crate) const fn event(self) -> u64 {
        self.event
    }

    pub(crate) const fn focus(self) -> NativeGlobalFocus {
        self.focus
    }

    pub(crate) fn references_binding(self, binding: NativeSurfaceBinding) -> bool {
        matches!(self.focus, NativeGlobalFocus::Dock(current) if current == binding)
    }
}

/// One exact result of consuming an eframe viewport-focus command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeViewportFocusRecord {
    viewport: ViewportId,
    window: WindowId,
    binding: Option<NativeSurfaceBinding>,
    status: NativeViewportFocusStatus,
}

impl NativeViewportFocusRecord {
    pub(crate) fn capture(
        result: NativeViewportFocusResult,
        viewports: &NativeViewportMap,
    ) -> Self {
        Self {
            viewport: result.viewport_id(),
            window: result.window_id(),
            binding: viewports
                .focus_binding_for_event(result.window_id(), Some(result.viewport_id())),
            status: result.status(),
        }
    }

    #[cfg(test)]
    pub(crate) const fn for_test(
        viewport: ViewportId,
        window: WindowId,
        binding: Option<NativeSurfaceBinding>,
        status: NativeViewportFocusStatus,
    ) -> Self {
        Self {
            viewport,
            window,
            binding,
            status,
        }
    }

    pub(crate) const fn viewport(self) -> ViewportId {
        self.viewport
    }

    pub(crate) const fn window(self) -> WindowId {
        self.window
    }

    pub(crate) fn references_binding(self, binding: NativeSurfaceBinding) -> bool {
        matches!(self.binding, Some(current) if current == binding)
    }
}

#[derive(Debug)]
struct PendingNativeFocus<Request> {
    viewport: ViewportId,
    window: WindowId,
    binding: NativeSurfaceBinding,
    state: PendingNativeFocusState<Request>,
}

#[derive(Debug)]
enum PendingNativeFocusState<Request> {
    Queued(Request),
    AwaitingCallback(Request),
    AwaitingGlobalFocus(Request),
}

#[derive(Debug)]
pub(crate) enum NativeFocusTermination<Request = NativeEffectRequest> {
    Queued {
        viewport: ViewportId,
        request: Request,
    },
    Dispatched(Request),
}

/// Classification of one callback against the single pending focus command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeFocusResultDisposition {
    Unrelated,
    Current {
        binding: NativeSurfaceBinding,
        status: NativeViewportFocusStatus,
    },
    Stale {
        binding: NativeSurfaceBinding,
    },
}

/// Affine ownership for the one focus command which eframe has not consumed yet.
#[derive(Debug)]
pub(crate) struct NativeFocusControl<Request = NativeEffectRequest> {
    pending: Option<PendingNativeFocus<Request>>,
}

impl<Request> Default for NativeFocusControl<Request> {
    fn default() -> Self {
        Self { pending: None }
    }
}

impl<Request> NativeFocusControl<Request> {
    pub(crate) const fn has_pending_work(&self) -> bool {
        self.pending.is_some()
    }

    pub(crate) fn retain(
        &mut self,
        viewport: ViewportId,
        window: WindowId,
        binding: NativeSurfaceBinding,
        request: Request,
    ) -> Result<(), Request> {
        if self.pending.is_some() {
            return Err(request);
        }
        self.pending = Some(PendingNativeFocus {
            viewport,
            window,
            binding,
            state: PendingNativeFocusState::Queued(request),
        });
        Ok(())
    }

    pub(crate) fn mark_dispatched(&mut self, viewport: ViewportId) -> bool {
        let Some(mut pending) = self.pending.take() else {
            return false;
        };
        if pending.viewport != viewport {
            self.pending = Some(pending);
            return false;
        }
        let changed = match pending.state {
            PendingNativeFocusState::Queued(request) => {
                pending.state = PendingNativeFocusState::AwaitingCallback(request);
                true
            }
            state => {
                pending.state = state;
                false
            }
        };
        self.pending = Some(pending);
        changed
    }

    pub(crate) fn classify(
        &self,
        record: NativeViewportFocusRecord,
    ) -> NativeFocusResultDisposition {
        let Some(pending) = &self.pending else {
            return NativeFocusResultDisposition::Unrelated;
        };
        if pending.viewport != record.viewport
            || pending.window != record.window
            || matches!(pending.state, PendingNativeFocusState::Queued(_))
        {
            return NativeFocusResultDisposition::Unrelated;
        }
        if record.binding == Some(pending.binding) {
            NativeFocusResultDisposition::Current {
                binding: pending.binding,
                status: record.status,
            }
        } else {
            NativeFocusResultDisposition::Stale {
                binding: pending.binding,
            }
        }
    }

    pub(crate) fn mark_requested(&mut self, record: NativeViewportFocusRecord) -> bool {
        if !matches!(
            self.classify(record),
            NativeFocusResultDisposition::Current {
                status: NativeViewportFocusStatus::Requested,
                ..
            }
        ) {
            return false;
        }
        let Some(mut pending) = self.pending.take() else {
            return false;
        };
        let changed = match pending.state {
            PendingNativeFocusState::AwaitingCallback(request)
            | PendingNativeFocusState::AwaitingGlobalFocus(request) => {
                pending.state = PendingNativeFocusState::AwaitingGlobalFocus(request);
                true
            }
            PendingNativeFocusState::Queued(request) => {
                pending.state = PendingNativeFocusState::Queued(request);
                false
            }
        };
        self.pending = Some(pending);
        changed
    }

    pub(crate) fn take_callback(&mut self, record: NativeViewportFocusRecord) -> Option<Request> {
        self.take_dispatched_if(
            |pending| pending.viewport == record.viewport && pending.window == record.window,
            |state| !matches!(state, PendingNativeFocusState::Queued(_)),
        )
    }

    pub(crate) fn take_observed(&mut self, binding: NativeSurfaceBinding) -> Option<Request> {
        self.take_dispatched_if(
            |pending| pending.binding == binding,
            |state| matches!(state, PendingNativeFocusState::AwaitingGlobalFocus(_)),
        )
    }

    fn take_dispatched_if(
        &mut self,
        matches_pending: impl FnOnce(&PendingNativeFocus<Request>) -> bool,
        matches_state: impl FnOnce(&PendingNativeFocusState<Request>) -> bool,
    ) -> Option<Request> {
        let pending = self.pending.as_ref()?;
        if !matches_pending(pending) || !matches_state(&pending.state) {
            return None;
        }
        let pending = self
            .pending
            .take()
            .expect("the inspected focus request remains pending");
        match pending.state {
            PendingNativeFocusState::AwaitingCallback(request)
            | PendingNativeFocusState::AwaitingGlobalFocus(request) => Some(request),
            PendingNativeFocusState::Queued(_) => {
                unreachable!("the queued focus state was rejected before extraction")
            }
        }
    }

    pub(crate) fn take_queued_terminal(
        &mut self,
        binding: NativeSurfaceBinding,
    ) -> Option<NativeFocusTermination<Request>> {
        if self.pending.as_ref().is_none_or(|pending| {
            pending.binding != binding
                || !matches!(pending.state, PendingNativeFocusState::Queued(_))
        }) {
            return None;
        }
        let pending = self.pending.take()?;
        let PendingNativeFocusState::Queued(request) = pending.state else {
            unreachable!("the queued focus state was checked before extraction")
        };
        Some(NativeFocusTermination::Queued {
            viewport: pending.viewport,
            request,
        })
    }

    pub(crate) fn take_terminal(
        &mut self,
        binding: NativeSurfaceBinding,
    ) -> Option<NativeFocusTermination<Request>> {
        if self
            .pending
            .as_ref()
            .is_none_or(|pending| pending.binding != binding)
        {
            return None;
        }
        let pending = self.pending.take()?;
        Some(match pending.state {
            PendingNativeFocusState::Queued(request) => NativeFocusTermination::Queued {
                viewport: pending.viewport,
                request,
            },
            PendingNativeFocusState::AwaitingCallback(request)
            | PendingNativeFocusState::AwaitingGlobalFocus(request) => {
                NativeFocusTermination::Dispatched(request)
            }
        })
    }

    pub(crate) fn pending_binding(&self) -> Option<NativeSurfaceBinding> {
        self.pending.as_ref().map(|pending| pending.binding)
    }

    pub(crate) fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|pending| pending.binding == binding)
    }
}

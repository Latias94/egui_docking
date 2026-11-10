//! Exact native-focus command and callback correlation.

use dockspace::runtime::{NativeEffectRequest, NativeGlobalFocus, NativeSurfaceBinding};
use eframe::{
    NativeGlobalFocus as EframeGlobalFocus, NativeGlobalFocusObservation,
    NativeViewportFocusCommandToken, NativeViewportFocusResult, NativeViewportFocusStatus,
    egui::ViewportId,
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
    token: NativeViewportFocusCommandToken,
    viewport: ViewportId,
    window: WindowId,
    binding: Option<NativeSurfaceBinding>,
    status: NativeViewportFocusStatus,
}

impl NativeViewportFocusRecord {
    pub(crate) const fn retired_before_dispatch(
        token: NativeViewportFocusCommandToken,
        viewport: ViewportId,
        window: WindowId,
        binding: NativeSurfaceBinding,
    ) -> Self {
        Self {
            token,
            viewport,
            window,
            binding: Some(binding),
            status: NativeViewportFocusStatus::Retired,
        }
    }

    pub(crate) fn qualified_for_binding(mut self, binding: NativeSurfaceBinding) -> Self {
        self.binding = Some(binding);
        self
    }

    pub(crate) fn capture(
        result: NativeViewportFocusResult,
        viewports: &NativeViewportMap,
    ) -> Self {
        Self {
            token: result.token(),
            viewport: result.viewport_id(),
            window: result.window_id(),
            binding: viewports
                .focus_binding_for_event(result.window_id(), Some(result.viewport_id())),
            status: result.status(),
        }
    }

    #[cfg(test)]
    pub(crate) const fn for_test(
        token: NativeViewportFocusCommandToken,
        viewport: ViewportId,
        window: WindowId,
        binding: Option<NativeSurfaceBinding>,
        status: NativeViewportFocusStatus,
    ) -> Self {
        Self {
            token,
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

    pub(crate) const fn token(self) -> NativeViewportFocusCommandToken {
        self.token
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
    token: Option<NativeViewportFocusCommandToken>,
    state: PendingNativeFocusState<Request>,
}

#[derive(Debug)]
enum PendingNativeFocusState<Request> {
    Queued(Request),
    AwaitingCallback(Request),
}

#[derive(Debug)]
pub(crate) enum NativeFocusTermination<Request = NativeEffectRequest> {
    Queued(Request),
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
            token: None,
            state: PendingNativeFocusState::Queued(request),
        });
        Ok(())
    }

    fn dispatch_queued(
        &mut self,
        viewport: ViewportId,
        token: NativeViewportFocusCommandToken,
        dispatch: impl FnOnce(Request) -> Request,
    ) -> bool {
        let Some(mut pending) = self.pending.take() else {
            return false;
        };
        if pending.viewport != viewport {
            self.pending = Some(pending);
            return false;
        }
        let changed = match pending.state {
            PendingNativeFocusState::Queued(request) => {
                let request = dispatch(request);
                pending.token = Some(token);
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

    #[cfg(test)]
    pub(crate) fn mark_dispatched(
        &mut self,
        viewport: ViewportId,
        token: NativeViewportFocusCommandToken,
    ) -> bool {
        self.dispatch_queued(viewport, token, |request| request)
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
            || pending.token != Some(record.token)
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

    pub(crate) fn take_callback(&mut self, record: NativeViewportFocusRecord) -> Option<Request> {
        let pending = self.pending.as_ref()?;
        if pending.viewport != record.viewport
            || pending.window != record.window
            || pending.token != Some(record.token)
            || !matches!(pending.state, PendingNativeFocusState::AwaitingCallback(_))
        {
            return None;
        }
        let pending = self
            .pending
            .take()
            .expect("the inspected focus request remains pending");
        match pending.state {
            PendingNativeFocusState::AwaitingCallback(request) => Some(request),
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
        Some(NativeFocusTermination::Queued(request))
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
            PendingNativeFocusState::Queued(request) => NativeFocusTermination::Queued(request),
            PendingNativeFocusState::AwaitingCallback(request) => {
                NativeFocusTermination::Dispatched(request)
            }
        })
    }

    pub(crate) fn pending_binding(&self) -> Option<NativeSurfaceBinding> {
        self.pending.as_ref().map(|pending| pending.binding)
    }

    #[cfg(test)]
    pub(crate) fn pending_token(&self) -> Option<NativeViewportFocusCommandToken> {
        self.pending.as_ref().and_then(|pending| pending.token)
    }

    pub(crate) fn queued_route(&self) -> Option<(ViewportId, NativeSurfaceBinding, WindowId)> {
        let pending = self.pending.as_ref()?;
        matches!(pending.state, PendingNativeFocusState::Queued(_)).then_some((
            pending.viewport,
            pending.binding,
            pending.window,
        ))
    }

    pub(crate) fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|pending| pending.binding == binding)
    }
}

impl NativeFocusControl {
    /// Keeps the affine guard on the stack until the backend enqueue returns.
    pub(crate) fn dispatch(
        &mut self,
        viewport: ViewportId,
        token: NativeViewportFocusCommandToken,
        enqueue: impl FnOnce(),
    ) -> bool {
        self.dispatch_queued(viewport, token, |request| {
            let dispatch = request.begin_dispatch();
            enqueue();
            dispatch.await_observation()
        })
    }
}

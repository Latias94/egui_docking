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
struct PendingNativeFocus {
    viewport: ViewportId,
    window: WindowId,
    binding: NativeSurfaceBinding,
    request: NativeEffectRequest,
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
#[derive(Debug, Default)]
pub(crate) struct NativeFocusControl {
    pending: Option<PendingNativeFocus>,
}

impl NativeFocusControl {
    pub(crate) fn retain(
        &mut self,
        viewport: ViewportId,
        window: WindowId,
        binding: NativeSurfaceBinding,
        request: NativeEffectRequest,
    ) -> Result<(), NativeEffectRequest> {
        if self.pending.is_some() {
            return Err(request);
        }
        self.pending = Some(PendingNativeFocus {
            viewport,
            window,
            binding,
            request,
        });
        Ok(())
    }

    pub(crate) fn classify(
        &self,
        record: NativeViewportFocusRecord,
    ) -> NativeFocusResultDisposition {
        let Some(pending) = &self.pending else {
            return NativeFocusResultDisposition::Unrelated;
        };
        if pending.viewport != record.viewport || pending.window != record.window {
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

    pub(crate) fn take(
        &mut self,
        record: NativeViewportFocusRecord,
    ) -> Option<NativeEffectRequest> {
        let pending = self.pending.as_ref()?;
        if pending.viewport != record.viewport || pending.window != record.window {
            return None;
        }
        self.pending.take().map(|pending| pending.request)
    }

    pub(crate) fn retire_binding(&mut self, binding: NativeSurfaceBinding) -> bool {
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.binding == binding)
        {
            self.pending = None;
            true
        } else {
            false
        }
    }

    pub(crate) fn pending_command(&self) -> Option<(ViewportId, NativeSurfaceBinding)> {
        self.pending
            .as_ref()
            .map(|pending| (pending.viewport, pending.binding))
    }

    pub(crate) fn cancel_pending(&mut self) {
        self.pending = None;
    }

    pub(crate) fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|pending| pending.binding == binding)
    }
}

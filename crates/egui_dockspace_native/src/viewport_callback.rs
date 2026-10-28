//! Immutable eframe viewport callback records.
//!
//! Eframe callbacks carry platform identity but deliberately know nothing
//! about dockspace bindings. The adapter freezes the exact binding at callback
//! time and journals these small records before reducing them into core facts.

use dockspace::runtime::NativeSurfaceBinding;
use eframe::{
    NativeViewportCreateAttempt, NativeViewportCreateFailureKind,
    NativeViewportVisibilityCommandToken, NativeViewportVisibilityResult,
    NativeViewportVisibilityStatus, egui::ViewportId,
};
use winit::window::WindowId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeViewportCreateFailureRecord {
    attempt: Option<NativeViewportCreateAttempt>,
    viewport: ViewportId,
    binding: NativeSurfaceBinding,
    kind: NativeViewportCreateFailureKind,
}

impl NativeViewportCreateFailureRecord {
    pub(crate) const fn new(
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        kind: NativeViewportCreateFailureKind,
    ) -> Self {
        Self {
            attempt: None,
            viewport,
            binding,
            kind,
        }
    }

    pub(crate) const fn from_attempt(
        attempt: NativeViewportCreateAttempt,
        binding: NativeSurfaceBinding,
        kind: NativeViewportCreateFailureKind,
    ) -> Self {
        Self {
            attempt: Some(attempt),
            viewport: attempt.viewport_id(),
            binding,
            kind,
        }
    }

    pub(crate) const fn attempt(self) -> Option<NativeViewportCreateAttempt> {
        self.attempt
    }

    pub(crate) const fn viewport(self) -> ViewportId {
        self.viewport
    }

    pub(crate) const fn binding(self) -> NativeSurfaceBinding {
        self.binding
    }

    pub(crate) const fn kind(self) -> NativeViewportCreateFailureKind {
        self.kind
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeViewportVisibilityRecord {
    token: NativeViewportVisibilityCommandToken,
    viewport: ViewportId,
    window: WindowId,
    binding: NativeSurfaceBinding,
    visible: bool,
    status: NativeViewportVisibilityStatus,
}

impl NativeViewportVisibilityRecord {
    pub(crate) const fn retired_before_dispatch(
        token: NativeViewportVisibilityCommandToken,
        viewport: ViewportId,
        window: WindowId,
        binding: NativeSurfaceBinding,
        visible: bool,
    ) -> Self {
        Self {
            token,
            viewport,
            window,
            binding,
            visible,
            status: NativeViewportVisibilityStatus::Retired,
        }
    }

    pub(crate) fn qualified_for_binding(mut self, binding: NativeSurfaceBinding) -> Self {
        self.binding = binding;
        self
    }

    pub(crate) const fn from_eframe(
        result: NativeViewportVisibilityResult,
        binding: NativeSurfaceBinding,
    ) -> Self {
        Self {
            token: result.token(),
            viewport: result.viewport_id(),
            window: result.window_id(),
            binding,
            visible: result.visible(),
            status: result.status(),
        }
    }

    #[cfg(test)]
    pub(crate) const fn for_test(
        token: NativeViewportVisibilityCommandToken,
        viewport: ViewportId,
        window: WindowId,
        binding: NativeSurfaceBinding,
        visible: bool,
        status: NativeViewportVisibilityStatus,
    ) -> Self {
        Self {
            token,
            viewport,
            window,
            binding,
            visible,
            status,
        }
    }

    pub(crate) const fn viewport(self) -> ViewportId {
        self.viewport
    }

    pub(crate) const fn token(self) -> NativeViewportVisibilityCommandToken {
        self.token
    }

    pub(crate) const fn window(self) -> WindowId {
        self.window
    }

    pub(crate) const fn binding(self) -> NativeSurfaceBinding {
        self.binding
    }

    pub(crate) const fn visible(self) -> bool {
        self.visible
    }

    pub(crate) const fn status(self) -> NativeViewportVisibilityStatus {
        self.status
    }
}

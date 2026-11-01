//! Exact command receipts for the existing native gesture's capture duty.

use dockspace::runtime::NativeSurfaceBinding;
use eframe::{
    NativeViewportPointerCaptureAction, NativeViewportPointerCaptureCommandToken,
    NativeViewportPointerCaptureResult, NativeViewportPointerCaptureStatus, egui::ViewportId,
};
use winit::event::DeviceId;
use winit::window::WindowId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativePointerCaptureKey {
    pub(crate) token: NativeViewportPointerCaptureCommandToken,
    pub(crate) viewport: ViewportId,
    pub(crate) window: WindowId,
    pub(crate) device: DeviceId,
    pub(crate) action: NativeViewportPointerCaptureAction,
}

impl NativePointerCaptureKey {
    pub(crate) const fn from_result(result: NativeViewportPointerCaptureResult) -> Self {
        Self {
            token: result.token(),
            viewport: result.viewport_id(),
            window: result.expected_window_id(),
            device: result.device_id(),
            action: result.action(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativePointerCaptureRecord {
    key: NativePointerCaptureKey,
    binding: NativeSurfaceBinding,
    status: NativeViewportPointerCaptureStatus,
}

impl NativePointerCaptureRecord {
    pub(crate) const fn capture(
        result: NativeViewportPointerCaptureResult,
        binding: NativeSurfaceBinding,
    ) -> Self {
        Self {
            key: NativePointerCaptureKey::from_result(result),
            binding,
            status: result.status(),
        }
    }

    pub(crate) const fn retired(
        key: NativePointerCaptureKey,
        binding: NativeSurfaceBinding,
    ) -> Self {
        Self {
            key,
            binding,
            status: NativeViewportPointerCaptureStatus::NotExecuted,
        }
    }

    pub(crate) const fn key(self) -> NativePointerCaptureKey {
        self.key
    }

    pub(crate) const fn binding(self) -> NativeSurfaceBinding {
        self.binding
    }

    pub(crate) const fn status(self) -> NativeViewportPointerCaptureStatus {
        self.status
    }

    pub(crate) fn references_binding(self, binding: NativeSurfaceBinding) -> bool {
        self.binding == binding
    }

    #[cfg(test)]
    pub(crate) const fn for_test(
        key: NativePointerCaptureKey,
        binding: NativeSurfaceBinding,
        status: NativeViewportPointerCaptureStatus,
    ) -> Self {
        Self {
            key,
            binding,
            status,
        }
    }
}

//! One attached physical window owns one Destroyed callback through commit.

use dockspace::runtime::NativeSurfaceBinding;
use eframe::{NativeViewportCreateAttempt, egui::ViewportId};
use winit::window::WindowId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DestroyedReceiptState {
    Awaiting,
    Received(u64),
    Transferred(u64),
    Committed(u64),
}

/// Created only from an attached window; the route or its retirement owns it.
#[derive(Debug)]
#[must_use = "an attached window receipt must move to its route or retirement owner"]
pub(crate) struct NativeWindowReceipt {
    viewport: ViewportId,
    window: WindowId,
    binding: NativeSurfaceBinding,
    attempt: Option<NativeViewportCreateAttempt>,
    destroyed: DestroyedReceiptState,
}

impl NativeWindowReceipt {
    pub(super) const fn attached(
        viewport: ViewportId,
        window: WindowId,
        binding: NativeSurfaceBinding,
        attempt: Option<NativeViewportCreateAttempt>,
    ) -> Self {
        Self {
            viewport,
            window,
            binding,
            attempt,
            destroyed: DestroyedReceiptState::Awaiting,
        }
    }

    pub(crate) const fn viewport(&self) -> ViewportId {
        self.viewport
    }

    pub(crate) const fn window(&self) -> WindowId {
        self.window
    }

    pub(crate) const fn binding(&self) -> NativeSurfaceBinding {
        self.binding
    }

    pub(crate) fn matches_window(&self, window: WindowId, viewport: Option<ViewportId>) -> bool {
        self.window == window && viewport.is_none_or(|viewport| viewport == self.viewport)
    }

    pub(crate) fn receive_destroyed(&mut self, ordinal: u64) -> bool {
        if ordinal == 0 || self.destroyed != DestroyedReceiptState::Awaiting {
            return false;
        }
        self.destroyed = DestroyedReceiptState::Received(ordinal);
        true
    }

    pub(crate) fn is_awaiting(&self) -> bool {
        self.destroyed == DestroyedReceiptState::Awaiting
    }

    pub(crate) fn received_destroyed(&self, ordinal: u64) -> bool {
        self.destroyed == DestroyedReceiptState::Received(ordinal)
    }

    pub(crate) fn transfer_destroyed(&mut self, ordinal: u64) -> bool {
        if !self.received_destroyed(ordinal) {
            return false;
        }
        self.destroyed = DestroyedReceiptState::Transferred(ordinal);
        true
    }

    pub(crate) fn commit_destroyed(&mut self) {
        if let DestroyedReceiptState::Transferred(ordinal) = self.destroyed {
            self.destroyed = DestroyedReceiptState::Committed(ordinal);
        }
    }

    pub(crate) fn destroyed_committed(&self) -> bool {
        matches!(self.destroyed, DestroyedReceiptState::Committed(_))
    }

    pub(crate) fn rebind(&mut self, binding: NativeSurfaceBinding) {
        self.binding = binding;
    }

    pub(super) const fn attempt(&self) -> Option<NativeViewportCreateAttempt> {
        self.attempt
    }
}

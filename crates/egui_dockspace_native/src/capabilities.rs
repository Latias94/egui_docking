//! Honest capability mapping for the attached native windowing backend.

use dockspace::runtime::{NativeHostCapabilities, NativeHostCapability};
use eframe::NativeWindowingBackend;

pub(crate) fn capabilities_for_backend(backend: NativeWindowingBackend) -> NativeHostCapabilities {
    let common = NativeHostCapabilities::none_supported()
        .with(NativeHostCapability::AuthoritativeInventory)
        .with(NativeHostCapability::PointerHitTestObservation)
        .with(NativeHostCapability::GlobalFocusObservation)
        .with(NativeHostCapability::CloseCancellation);
    let controlled_hit_test = common.with(NativeHostCapability::PointerHitTestControl);

    match backend {
        NativeWindowingBackend::Windows => controlled_hit_test
            .with(NativeHostCapability::NativeWindowLifecycle)
            .with(NativeHostCapability::HoveredWindow)
            .with(NativeHostCapability::DesktopPointerPosition)
            .with(NativeHostCapability::GlobalWindowPlacement)
            .with(NativeHostCapability::WorkArea)
            .with(NativeHostCapability::WindowActivationControl),
        NativeWindowingBackend::MacOs | NativeWindowingBackend::X11 => controlled_hit_test
            .with(NativeHostCapability::NativeWindowLifecycle)
            .with(NativeHostCapability::DesktopPointerPosition)
            .with(NativeHostCapability::GlobalWindowPlacement)
            .with(NativeHostCapability::WindowActivationControl),
        NativeWindowingBackend::Wayland => controlled_hit_test,
        NativeWindowingBackend::Other => common,
        _ => common,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_capability_matrix_is_conservative() {
        let windows = capabilities_for_backend(NativeWindowingBackend::Windows);
        assert!(windows.supports(NativeHostCapability::NativeWindowLifecycle));
        assert!(windows.supports(NativeHostCapability::HoveredWindow));
        assert!(windows.supports(NativeHostCapability::WorkArea));
        assert!(!windows.supports(NativeHostCapability::AuthoritativeButtonState));

        let x11 = capabilities_for_backend(NativeWindowingBackend::X11);
        assert!(x11.supports(NativeHostCapability::NativeWindowLifecycle));
        assert!(x11.supports(NativeHostCapability::GlobalWindowPlacement));
        assert!(!x11.supports(NativeHostCapability::HoveredWindow));
        assert!(!x11.supports(NativeHostCapability::WorkArea));

        let wayland = capabilities_for_backend(NativeWindowingBackend::Wayland);
        assert!(wayland.supports(NativeHostCapability::AuthoritativeInventory));
        assert!(wayland.supports(NativeHostCapability::PointerHitTestObservation));
        assert!(wayland.supports(NativeHostCapability::PointerHitTestControl));
        assert!(!wayland.supports(NativeHostCapability::NativeWindowLifecycle));
        assert!(!wayland.supports(NativeHostCapability::DesktopPointerPosition));
        assert!(!wayland.supports(NativeHostCapability::GlobalWindowPlacement));
        assert!(!wayland.supports(NativeHostCapability::WorkArea));
        assert!(!wayland.supports(NativeHostCapability::WindowActivationControl));

        let other = capabilities_for_backend(NativeWindowingBackend::Other);
        assert!(other.supports(NativeHostCapability::PointerHitTestObservation));
        assert!(!other.supports(NativeHostCapability::PointerHitTestControl));
    }
}

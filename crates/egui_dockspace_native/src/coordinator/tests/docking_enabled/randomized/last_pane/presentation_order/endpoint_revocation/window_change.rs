use dockspace::geometry::{PhysicalRect, PhysicalSize, ScaleFactor};
use dockspace::runtime::{NativeWindowFacts, NativeWindowPresentationState};

use super::{NativeCoordinator, NativeSurfaceBinding, ready_window_facts};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WindowChange {
    Hidden,
    PresentationScale,
    NativeDpi,
    NegativeOrigin,
    PositiveOrigin,
    ClientResize,
}

impl WindowChange {
    pub(super) fn facts(self, original_x: f64) -> NativeWindowFacts {
        let facts = ready_window_facts(original_x);
        match self {
            Self::Hidden => facts.with_presentation(NativeWindowPresentationState::Hidden),
            Self::PresentationScale => {
                facts.with_presentation_scale_factor(ScaleFactor::new(2.0).unwrap())
            }
            // OS DPI must not silently substitute the renderer's independent scale.
            Self::NativeDpi => facts.with_native_scale_factor(ScaleFactor::new(2.0).unwrap()),
            Self::NegativeOrigin | Self::PositiveOrigin => {
                let (x, y) = match self {
                    Self::NegativeOrigin => (original_x - 960.0, -360.0),
                    Self::PositiveOrigin => (original_x + 960.0, 240.0),
                    _ => unreachable!(),
                };
                facts
                    .with_content_bounds(PhysicalRect::new(x, y, 640.0, 480.0).unwrap())
                    .with_outer_bounds(PhysicalRect::new(x - 8.0, y - 30.0, 656.0, 518.0).unwrap())
            }
            Self::ClientResize => facts
                .with_content_bounds(PhysicalRect::new(original_x, 0.0, 800.0, 520.0).unwrap())
                .with_outer_bounds(
                    PhysicalRect::new(original_x - 8.0, -30.0, 816.0, 558.0).unwrap(),
                ),
        }
    }

    pub(super) fn assert_applied(self, native: &NativeCoordinator, binding: NativeSurfaceBinding) {
        if self == Self::Hidden {
            return;
        }
        let geometry = native
            .session
            .native_local_coordinates(binding)
            .unwrap()
            .geometry();
        let placement = native.session.native_surface_placement(binding).unwrap();
        let native_scale = if self == Self::NativeDpi { 2.0 } else { 1.0 };
        let presentation_scale = if self == Self::PresentationScale {
            2.0
        } else {
            1.0
        };
        let (width, height) = if self == Self::ClientResize {
            (800.0, 520.0)
        } else {
            (640.0, 480.0)
        };
        assert_eq!(
            geometry.native_scale_factor(),
            ScaleFactor::new(native_scale).unwrap(),
            "{self:?} on {binding:?}: native scale is independent"
        );
        assert_eq!(
            placement.scale_factor(),
            ScaleFactor::new(presentation_scale).unwrap(),
            "{self:?} on {binding:?}: presentation scale is independent"
        );
        assert_eq!(
            geometry.content_size(),
            PhysicalSize::new(width, height).unwrap(),
            "{self:?} on {binding:?}: only client resize changes physical size"
        );
    }
}

//! Local frame appearance without changing core-owned margins or hit geometry.

use egui::{Color32, CornerRadius, Frame, epaint::Shadow};

/// Appearance overrides applied to an egui window or popup recipe.
///
/// Unspecified properties follow the current local egui theme. Border width
/// and floating geometry remain explicit metrics on [`super::DockStyle`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DockFrameVisuals {
    /// Background fill; `None` follows the current window or popup theme.
    pub fill: Option<Color32>,
    /// Outline color; border width remains a separate docking metric.
    pub border_color: Option<Color32>,
    /// Corner rounding; `None` preserves the current egui frame recipe.
    pub corner_radius: Option<CornerRadius>,
    /// Frame shadow; use `Some(Shadow::NONE)` to remove the theme's shadow.
    pub shadow: Option<Shadow>,
}

impl DockFrameVisuals {
    pub(super) fn apply(self, mut frame: Frame) -> Frame {
        if let Some(fill) = self.fill {
            frame.fill = fill;
        }
        if let Some(color) = self.border_color {
            frame.stroke.color = color;
        }
        if let Some(radius) = self.corner_radius {
            frame.corner_radius = radius;
        }
        if let Some(shadow) = self.shadow {
            frame.shadow = shadow;
        }
        frame
    }
}

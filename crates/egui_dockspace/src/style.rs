//! Fixed geometry and color configuration for the egui adapter.

use std::{error::Error, fmt};

use dockspace::presentation_config::{DockPresentationConfig, DockPresentationConfigError};
use egui::{Color32, Vec2};

/// Configurable geometry and colors used to paint a docking scene.
///
/// All geometry is expressed in egui points. Values are fixed for a frame and
/// are never derived from viewport size, pointer velocity, elapsed time, or DPI.
#[derive(Clone, Debug, PartialEq)]
pub struct DockStyle {
    /// Height of every tab bar.
    pub tab_bar_height: f32,
    /// Nominal square extent of the whole-tab-stack grip.
    pub tab_group_grip_extent: f32,
    /// Horizontal padding on each side of tab content.
    pub tab_horizontal_padding: f32,
    /// Minimum width of a tab.
    pub tab_min_width: f32,
    /// Maximum width of a tab.
    pub tab_max_width: f32,
    /// Width and height of a tab close control.
    pub tab_close_size: f32,
    /// Visible thickness of a splitter.
    pub splitter_thickness: f32,
    /// Total interaction thickness centered on a splitter.
    pub splitter_hit_extent: f32,
    /// Logical point step used by keyboard splitter and contained-edge adjustment.
    pub splitter_keyboard_step: f32,
    /// Width and height of one explicit docking guide button.
    pub drop_guide_extent: f32,
    /// Visible gap between adjacent buttons in an inner five-way guide.
    pub drop_guide_gap: f32,
    /// Extra exact hit extent around each visible guide button.
    pub drop_guide_hit_padding: f32,
    /// Distance from a root edge to the center of its outer guide button.
    pub drop_guide_outer_inset: f32,
    /// Fraction assigned to a newly docked child.
    pub dock_fraction: f32,
    /// Height of contained-floating title bars.
    pub floating_title_height: f32,
    /// Visible border width of contained-floating surfaces.
    pub floating_border_width: f32,
    /// Total interaction thickness of contained-floating resize handles.
    pub floating_resize_extent: f32,
    /// Global minimum pane content size.
    pub minimum_pane_size: Vec2,
    /// Global minimum outer size of a contained-floating surface.
    pub minimum_floating_size: Vec2,
    /// Pointer-relative offset used to paint a drag ghost.
    pub ghost_offset: Vec2,
    /// Fill behind tiled docking content.
    pub workspace_fill: Color32,
    /// Fill behind tabs in a tab bar.
    pub tab_bar_fill: Color32,
    /// Fill of an inactive tab.
    pub tab_fill: Color32,
    /// Fill of a hovered inactive tab.
    pub tab_hover_fill: Color32,
    /// Fill of the selected tab.
    pub tab_active_fill: Color32,
    /// Text color of inactive tabs.
    pub tab_text_color: Color32,
    /// Text color of the selected tab.
    pub tab_active_text_color: Color32,
    /// Fill of an idle splitter.
    pub splitter_color: Color32,
    /// Fill of a hovered or keyboard-focused splitter.
    pub splitter_hover_color: Color32,
    /// Translucent fill of the resolved drop target.
    pub drop_fill: Color32,
    /// Border color of the resolved drop target.
    pub drop_border_color: Color32,
    /// Fill of an available passive docking guide.
    pub drop_guide_fill: Color32,
    /// Fill of the exact active docking guide.
    pub drop_guide_active_fill: Color32,
    /// Border and directional cue color of a passive docking guide.
    pub drop_guide_border_color: Color32,
    /// Border and directional cue color of the exact active docking guide.
    pub drop_guide_active_border_color: Color32,
    /// Fill of contained-floating pane content.
    pub floating_fill: Color32,
    /// Fill of an inactive contained-floating title bar.
    pub floating_title_fill: Color32,
    /// Fill of the focused contained-floating title bar.
    pub floating_title_active_fill: Color32,
    /// Border color of contained-floating surfaces.
    pub floating_border_color: Color32,
    /// Translucent fill of a drag ghost.
    pub ghost_fill: Color32,
    /// Border color of a drag ghost.
    pub ghost_border_color: Color32,
}

impl DockStyle {
    pub(crate) fn presentation_config(
        &self,
    ) -> Result<DockPresentationConfig, DockPresentationConfigError> {
        DockPresentationConfig::builder()
            .tab_bar_height(f64::from(self.tab_bar_height))
            .tab_group_grip_extent(f64::from(self.tab_group_grip_extent))
            .tab_horizontal_padding(f64::from(self.tab_horizontal_padding))
            .tab_min_width(f64::from(self.tab_min_width))
            .tab_max_width(f64::from(self.tab_max_width))
            .tab_close_extent(f64::from(self.tab_close_size))
            .splitter_thickness(f64::from(self.splitter_thickness))
            .splitter_hit_extent(f64::from(self.splitter_hit_extent))
            .splitter_keyboard_step(f64::from(self.splitter_keyboard_step))
            .guide_extent(f64::from(self.drop_guide_extent))
            .guide_gap(f64::from(self.drop_guide_gap))
            .guide_hit_padding(f64::from(self.drop_guide_hit_padding))
            .guide_outer_inset(f64::from(self.drop_guide_outer_inset))
            .dock_fraction(f64::from(self.dock_fraction))
            .floating_title_height(f64::from(self.floating_title_height))
            .floating_border_width(f64::from(self.floating_border_width))
            .floating_resize_extent(f64::from(self.floating_resize_extent))
            .minimum_pane_size(
                f64::from(self.minimum_pane_size.x),
                f64::from(self.minimum_pane_size.y),
            )
            .minimum_floating_size(
                f64::from(self.minimum_floating_size.x),
                f64::from(self.minimum_floating_size.y),
            )
            .build()
    }

    /// Validates every geometry metric and all cross-field invariants.
    ///
    /// Call this before publishing style metrics to the core scene. Validation
    /// is deterministic and does not normalize or silently repair values.
    ///
    /// # Errors
    ///
    /// Returns [`DockStyleError`] when a metric is not finite, violates its
    /// sign constraint, or conflicts with another metric.
    pub fn validate(&self) -> Result<(), DockStyleError> {
        for (field, value) in [
            ("tab_bar_height", self.tab_bar_height),
            ("tab_group_grip_extent", self.tab_group_grip_extent),
            ("tab_min_width", self.tab_min_width),
            ("tab_max_width", self.tab_max_width),
            ("tab_close_size", self.tab_close_size),
            ("splitter_thickness", self.splitter_thickness),
            ("splitter_hit_extent", self.splitter_hit_extent),
            ("splitter_keyboard_step", self.splitter_keyboard_step),
            ("drop_guide_extent", self.drop_guide_extent),
            ("drop_guide_outer_inset", self.drop_guide_outer_inset),
            ("floating_title_height", self.floating_title_height),
            ("floating_resize_extent", self.floating_resize_extent),
        ] {
            require_positive(field, value)?;
        }

        for (field, value) in [
            ("tab_horizontal_padding", self.tab_horizontal_padding),
            ("drop_guide_gap", self.drop_guide_gap),
            ("drop_guide_hit_padding", self.drop_guide_hit_padding),
            ("floating_border_width", self.floating_border_width),
            ("ghost_offset.x", self.ghost_offset.x),
            ("ghost_offset.y", self.ghost_offset.y),
        ] {
            require_non_negative(field, value)?;
        }

        for (field, value) in [
            ("minimum_pane_size.x", self.minimum_pane_size.x),
            ("minimum_pane_size.y", self.minimum_pane_size.y),
            ("minimum_floating_size.x", self.minimum_floating_size.x),
            ("minimum_floating_size.y", self.minimum_floating_size.y),
        ] {
            require_positive(field, value)?;
        }

        require_finite("dock_fraction", self.dock_fraction)?;
        if !(0.0..1.0).contains(&self.dock_fraction) || self.dock_fraction == 0.0 {
            return Err(DockStyleError::DockFractionOutOfRange);
        }

        if self.tab_min_width > self.tab_max_width {
            return Err(DockStyleError::MinimumExceedsMaximum {
                minimum: "tab_min_width",
                maximum: "tab_max_width",
            });
        }
        if self.tab_close_size > self.tab_bar_height {
            return Err(DockStyleError::ExtentExceedsContainer {
                extent: "tab_close_size",
                container: "tab_bar_height",
            });
        }
        if self.tab_group_grip_extent > self.tab_bar_height {
            return Err(DockStyleError::ExtentExceedsContainer {
                extent: "tab_group_grip_extent",
                container: "tab_bar_height",
            });
        }
        if self.splitter_hit_extent < self.splitter_thickness {
            return Err(DockStyleError::HitExtentSmallerThanVisible {
                hit_extent: "splitter_hit_extent",
                visible_extent: "splitter_thickness",
            });
        }
        if self.floating_resize_extent < self.floating_border_width {
            return Err(DockStyleError::HitExtentSmallerThanVisible {
                hit_extent: "floating_resize_extent",
                visible_extent: "floating_border_width",
            });
        }
        if self.drop_guide_gap < 2.0 * self.drop_guide_hit_padding {
            return Err(DockStyleError::GuideHitRegionsOverlap);
        }
        let guide_hit_half = self.drop_guide_extent * 0.5 + self.drop_guide_hit_padding;
        if self.drop_guide_outer_inset < guide_hit_half {
            return Err(DockStyleError::GuideOuterInsetTooSmall);
        }
        let guide_span = 3.0 * self.drop_guide_extent
            + 2.0 * self.drop_guide_gap
            + 2.0 * self.drop_guide_hit_padding;
        if guide_span > self.minimum_pane_size.x || guide_span > self.minimum_pane_size.y {
            return Err(DockStyleError::GuideClusterExceedsMinimumPane);
        }

        Ok(())
    }
}

impl Default for DockStyle {
    fn default() -> Self {
        Self {
            tab_bar_height: 28.0,
            tab_group_grip_extent: 28.0,
            tab_horizontal_padding: 10.0,
            tab_min_width: 72.0,
            tab_max_width: 220.0,
            tab_close_size: 16.0,
            splitter_thickness: 1.0,
            splitter_hit_extent: 6.0,
            splitter_keyboard_step: 16.0,
            drop_guide_extent: 16.0,
            drop_guide_gap: 4.0,
            drop_guide_hit_padding: 2.0,
            drop_guide_outer_inset: 40.0,
            dock_fraction: 0.5,
            floating_title_height: 28.0,
            floating_border_width: 1.0,
            floating_resize_extent: 7.0,
            minimum_pane_size: Vec2::new(80.0, 60.0),
            minimum_floating_size: Vec2::new(120.0, 120.0),
            ghost_offset: Vec2::new(12.0, 12.0),
            workspace_fill: Color32::from_rgb(24, 26, 29),
            tab_bar_fill: Color32::from_rgb(31, 34, 38),
            tab_fill: Color32::from_rgb(39, 43, 48),
            tab_hover_fill: Color32::from_rgb(48, 59, 68),
            tab_active_fill: Color32::from_rgb(37, 102, 112),
            tab_text_color: Color32::from_rgb(191, 197, 204),
            tab_active_text_color: Color32::from_rgb(244, 247, 248),
            splitter_color: Color32::from_rgb(69, 75, 82),
            splitter_hover_color: Color32::from_rgb(89, 157, 165),
            drop_fill: Color32::from_rgba_unmultiplied(49, 142, 154, 72),
            drop_border_color: Color32::from_rgb(76, 174, 184),
            drop_guide_fill: Color32::from_rgb(52, 57, 62),
            drop_guide_active_fill: Color32::from_rgb(49, 142, 154),
            drop_guide_border_color: Color32::from_rgb(111, 119, 127),
            drop_guide_active_border_color: Color32::from_rgb(190, 235, 239),
            floating_fill: Color32::from_rgb(29, 32, 36),
            floating_title_fill: Color32::from_rgb(47, 51, 57),
            floating_title_active_fill: Color32::from_rgb(83, 70, 119),
            floating_border_color: Color32::from_rgb(91, 98, 106),
            ghost_fill: Color32::from_rgba_unmultiplied(180, 132, 62, 64),
            ghost_border_color: Color32::from_rgb(204, 157, 83),
        }
    }
}

/// Error returned when [`DockStyle`] geometry is not internally consistent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DockStyleError {
    /// A metric is NaN or infinite.
    NonFinite { field: &'static str },
    /// A metric that may be zero is negative.
    Negative { field: &'static str },
    /// A metric that represents a size is zero or negative.
    NotPositive { field: &'static str },
    /// A configured minimum is greater than its maximum.
    MinimumExceedsMaximum {
        minimum: &'static str,
        maximum: &'static str,
    },
    /// A visible control cannot fit within its containing chrome.
    ExtentExceedsContainer {
        extent: &'static str,
        container: &'static str,
    },
    /// An interaction extent is smaller than the visible element it covers.
    HitExtentSmallerThanVisible {
        hit_extent: &'static str,
        visible_extent: &'static str,
    },
    /// The initial docking fraction is not strictly between zero and one.
    DockFractionOutOfRange,
    /// Expanded guide hit rectangles would overlap.
    GuideHitRegionsOverlap,
    /// An outer guide button and its hit padding would cross the root boundary.
    GuideOuterInsetTooSmall,
    /// The complete five-way guide cannot fit inside the configured minimum pane.
    GuideClusterExceedsMinimumPane,
}

impl fmt::Display for DockStyleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite { field } => write!(formatter, "`{field}` must be finite"),
            Self::Negative { field } => write!(formatter, "`{field}` must not be negative"),
            Self::NotPositive { field } => write!(formatter, "`{field}` must be positive"),
            Self::MinimumExceedsMaximum { minimum, maximum } => {
                write!(formatter, "`{minimum}` must not exceed `{maximum}`")
            }
            Self::ExtentExceedsContainer { extent, container } => {
                write!(formatter, "`{extent}` must not exceed `{container}`")
            }
            Self::HitExtentSmallerThanVisible {
                hit_extent,
                visible_extent,
            } => write!(
                formatter,
                "`{hit_extent}` must not be smaller than `{visible_extent}`"
            ),
            Self::DockFractionOutOfRange => {
                formatter.write_str("`dock_fraction` must be strictly between zero and one")
            }
            Self::GuideHitRegionsOverlap => formatter
                .write_str("`drop_guide_gap` must be at least twice `drop_guide_hit_padding`"),
            Self::GuideOuterInsetTooSmall => formatter.write_str(
                "`drop_guide_outer_inset` must contain half a guide plus its hit padding",
            ),
            Self::GuideClusterExceedsMinimumPane => formatter
                .write_str("the complete docking guide must fit inside `minimum_pane_size`"),
        }
    }
}

impl Error for DockStyleError {}

fn require_finite(field: &'static str, value: f32) -> Result<(), DockStyleError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(DockStyleError::NonFinite { field })
    }
}

fn require_non_negative(field: &'static str, value: f32) -> Result<(), DockStyleError> {
    require_finite(field, value)?;
    if value >= 0.0 {
        Ok(())
    } else {
        Err(DockStyleError::Negative { field })
    }
}

fn require_positive(field: &'static str, value: f32) -> Result<(), DockStyleError> {
    require_finite(field, value)?;
    if value > 0.0 {
        Ok(())
    } else {
        Err(DockStyleError::NotPositive { field })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_style_is_valid() {
        assert_eq!(DockStyle::default().validate(), Ok(()));
    }

    #[test]
    fn validation_rejects_non_finite_and_signed_metrics() {
        let mut style = DockStyle {
            splitter_thickness: f32::NAN,
            ..DockStyle::default()
        };
        assert_eq!(
            style.validate(),
            Err(DockStyleError::NonFinite {
                field: "splitter_thickness"
            })
        );

        style.splitter_thickness = 1.0;
        style.tab_horizontal_padding = -1.0;
        assert_eq!(
            style.validate(),
            Err(DockStyleError::Negative {
                field: "tab_horizontal_padding"
            })
        );

        style.tab_horizontal_padding = 0.0;
        style.drop_guide_extent = 0.0;
        assert_eq!(
            style.validate(),
            Err(DockStyleError::NotPositive {
                field: "drop_guide_extent"
            })
        );
    }

    #[test]
    fn validation_rejects_invalid_relations() {
        let mut style = DockStyle {
            tab_min_width: 221.0,
            ..DockStyle::default()
        };
        assert_eq!(
            style.validate(),
            Err(DockStyleError::MinimumExceedsMaximum {
                minimum: "tab_min_width",
                maximum: "tab_max_width"
            })
        );

        style.tab_min_width = 72.0;
        style.splitter_hit_extent = 0.5;
        assert_eq!(
            style.validate(),
            Err(DockStyleError::HitExtentSmallerThanVisible {
                hit_extent: "splitter_hit_extent",
                visible_extent: "splitter_thickness"
            })
        );

        style.splitter_hit_extent = 6.0;
        style.dock_fraction = 1.0;
        assert_eq!(
            style.validate(),
            Err(DockStyleError::DockFractionOutOfRange)
        );

        style.dock_fraction = 0.5;
        style.drop_guide_gap = 3.0;
        assert_eq!(
            style.validate(),
            Err(DockStyleError::GuideHitRegionsOverlap)
        );

        style.drop_guide_gap = 4.0;
        style.drop_guide_outer_inset = 9.0;
        assert_eq!(
            style.validate(),
            Err(DockStyleError::GuideOuterInsetTooSmall)
        );

        style.drop_guide_outer_inset = 40.0;
        style.drop_guide_extent = 18.0;
        assert_eq!(
            style.validate(),
            Err(DockStyleError::GuideClusterExceedsMinimumPane)
        );
    }

    #[test]
    fn ghost_offset_is_fixed_and_must_be_finite() {
        let mut style = DockStyle {
            ghost_offset: Vec2::new(f32::INFINITY, 12.0),
            ..DockStyle::default()
        };
        assert_eq!(
            style.validate(),
            Err(DockStyleError::NonFinite {
                field: "ghost_offset.x"
            })
        );

        style.ghost_offset = Vec2::new(-1.0, 12.0);
        assert_eq!(
            style.validate(),
            Err(DockStyleError::Negative {
                field: "ghost_offset.x"
            })
        );
    }
}

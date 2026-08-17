//! Renderer-neutral geometry configuration for semantic presentation compilation.

use thiserror::Error;

use crate::geometry::{GeometryError, LogicalSize};

/// Monotonic revision of the core-owned presentation configuration.
///
/// Revisions are advanced explicitly. They are never derived from hashes, so a
/// stale measurement can be rejected without relying on collision resistance.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct PresentationConfigRevision(u64);

impl PresentationConfigRevision {
    /// Creates a revision from its engine-local counter representation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the engine-local counter representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Advances the revision without wrapping.
    #[must_use]
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

/// Validated core-owned geometry used to compile a semantic presentation.
///
/// Every scalar uses renderer-independent logical units. Visual-only values
/// such as colors, fonts, shadows, corner radii, and drag-ghost offsets belong
/// to the UI adapter and are intentionally absent.
#[derive(Debug, Clone, PartialEq)]
pub struct DockPresentationConfig {
    tab_bar_height: f64,
    tab_strip_scroll_line_extent: f64,
    tab_group_grip_extent: f64,
    tab_horizontal_padding: f64,
    tab_min_width: f64,
    tab_max_width: f64,
    tab_close_extent: f64,
    splitter_thickness: f64,
    splitter_hit_extent: f64,
    splitter_keyboard_step: f64,
    pointer_drag_start_distance: f64,
    guide_extent: f64,
    guide_gap: f64,
    guide_hit_padding: f64,
    guide_outer_inset: f64,
    dock_fraction: f64,
    floating_title_height: f64,
    floating_border_width: f64,
    floating_resize_extent: f64,
    minimum_pane_size: LogicalSize,
    minimum_floating_size: LogicalSize,
}

impl Default for DockPresentationConfig {
    fn default() -> Self {
        Self::builder()
            .build()
            .expect("the baseline presentation configuration is valid")
    }
}

impl DockPresentationConfig {
    /// Starts a builder populated with the crate's explicit baseline geometry.
    #[must_use]
    pub fn builder() -> DockPresentationConfigBuilder {
        DockPresentationConfigBuilder::default()
    }

    /// Returns the tab-bar height.
    #[must_use]
    pub const fn tab_bar_height(&self) -> f64 {
        self.tab_bar_height
    }

    /// Returns the logical content distance represented by one tab-strip line unit.
    #[must_use]
    pub const fn tab_strip_scroll_line_extent(&self) -> f64 {
        self.tab_strip_scroll_line_extent
    }

    /// Returns the nominal square extent of a whole-tab-stack grip.
    #[must_use]
    pub const fn tab_group_grip_extent(&self) -> f64 {
        self.tab_group_grip_extent
    }

    /// Returns horizontal padding on each side of tab content.
    #[must_use]
    pub const fn tab_horizontal_padding(&self) -> f64 {
        self.tab_horizontal_padding
    }

    /// Returns the minimum allocated tab width.
    #[must_use]
    pub const fn tab_min_width(&self) -> f64 {
        self.tab_min_width
    }

    /// Returns the maximum allocated tab width.
    #[must_use]
    pub const fn tab_max_width(&self) -> f64 {
        self.tab_max_width
    }

    /// Returns the square extent reserved for a tab close control.
    #[must_use]
    pub const fn tab_close_extent(&self) -> f64 {
        self.tab_close_extent
    }

    /// Returns the visible splitter thickness.
    #[must_use]
    pub const fn splitter_thickness(&self) -> f64 {
        self.splitter_thickness
    }

    /// Returns the total splitter interaction extent.
    #[must_use]
    pub const fn splitter_hit_extent(&self) -> f64 {
        self.splitter_hit_extent
    }

    /// Returns the logical keyboard adjustment step for splitters.
    #[must_use]
    pub const fn splitter_keyboard_step(&self) -> f64 {
        self.splitter_keyboard_step
    }

    /// Returns the logical source-surface distance required to begin a pointer drag.
    ///
    /// This is an explicit core interaction policy. Adapters report ordered
    /// press and move edges; they do not decide when a drag starts.
    #[must_use]
    pub const fn pointer_drag_start_distance(&self) -> f64 {
        self.pointer_drag_start_distance
    }

    /// Returns the width and height of one docking guide button.
    #[must_use]
    pub const fn guide_extent(&self) -> f64 {
        self.guide_extent
    }

    /// Returns the visible gap between adjacent inner guide buttons.
    #[must_use]
    pub const fn guide_gap(&self) -> f64 {
        self.guide_gap
    }

    /// Returns the exact hit padding around a docking guide button.
    #[must_use]
    pub const fn guide_hit_padding(&self) -> f64 {
        self.guide_hit_padding
    }

    /// Returns the distance from a root edge to an outer guide center.
    #[must_use]
    pub const fn guide_outer_inset(&self) -> f64 {
        self.guide_outer_inset
    }

    pub(crate) fn inner_guide_reference_span(&self) -> f64 {
        3.0 * self.guide_extent + 2.0 * self.guide_gap + 2.0 * self.guide_hit_padding
    }

    pub(crate) fn outer_guide_reference_span(&self) -> f64 {
        let hit_half = self.guide_extent * 0.5 + self.guide_hit_padding;
        2.0 * (self.guide_outer_inset + 2.0 * hit_half)
    }

    /// Returns the fraction assigned to a newly docked edge child.
    #[must_use]
    pub const fn dock_fraction(&self) -> f64 {
        self.dock_fraction
    }

    /// Returns the height of a contained-floating title bar.
    #[must_use]
    pub const fn floating_title_height(&self) -> f64 {
        self.floating_title_height
    }

    /// Returns the visible contained-floating border width.
    #[must_use]
    pub const fn floating_border_width(&self) -> f64 {
        self.floating_border_width
    }

    /// Returns the total contained-floating resize interaction extent.
    #[must_use]
    pub const fn floating_resize_extent(&self) -> f64 {
        self.floating_resize_extent
    }

    /// Returns the global minimum pane content size.
    #[must_use]
    pub const fn minimum_pane_size(&self) -> LogicalSize {
        self.minimum_pane_size
    }

    /// Returns the global minimum outer size of a contained-floating surface.
    #[must_use]
    pub const fn minimum_floating_size(&self) -> LogicalSize {
        self.minimum_floating_size
    }
}

/// Fallible builder for [`DockPresentationConfig`].
#[derive(Debug, Clone, PartialEq)]
pub struct DockPresentationConfigBuilder {
    tab_bar_height: f64,
    tab_strip_scroll_line_extent: f64,
    tab_group_grip_extent: f64,
    tab_horizontal_padding: f64,
    tab_min_width: f64,
    tab_max_width: f64,
    tab_close_extent: f64,
    splitter_thickness: f64,
    splitter_hit_extent: f64,
    splitter_keyboard_step: f64,
    pointer_drag_start_distance: f64,
    guide_extent: f64,
    guide_gap: f64,
    guide_hit_padding: f64,
    guide_outer_inset: f64,
    dock_fraction: f64,
    floating_title_height: f64,
    floating_border_width: f64,
    floating_resize_extent: f64,
    minimum_pane_width: f64,
    minimum_pane_height: f64,
    minimum_floating_width: f64,
    minimum_floating_height: f64,
}

impl Default for DockPresentationConfigBuilder {
    fn default() -> Self {
        Self {
            tab_bar_height: 28.0,
            tab_strip_scroll_line_extent: 40.0,
            tab_group_grip_extent: 28.0,
            tab_horizontal_padding: 10.0,
            tab_min_width: 72.0,
            tab_max_width: 220.0,
            tab_close_extent: 16.0,
            splitter_thickness: 1.0,
            splitter_hit_extent: 6.0,
            splitter_keyboard_step: 16.0,
            pointer_drag_start_distance: 6.0,
            guide_extent: 24.0,
            guide_gap: 8.0,
            guide_hit_padding: 4.0,
            guide_outer_inset: 48.0,
            dock_fraction: 0.5,
            floating_title_height: 28.0,
            floating_border_width: 1.0,
            floating_resize_extent: 7.0,
            minimum_pane_width: 80.0,
            minimum_pane_height: 60.0,
            minimum_floating_width: 120.0,
            minimum_floating_height: 120.0,
        }
    }
}

impl DockPresentationConfigBuilder {
    /// Sets the tab-bar height.
    #[must_use]
    pub const fn tab_bar_height(mut self, value: f64) -> Self {
        self.tab_bar_height = value;
        self
    }

    /// Sets the logical content distance represented by one tab-strip line unit.
    #[must_use]
    pub const fn tab_strip_scroll_line_extent(mut self, value: f64) -> Self {
        self.tab_strip_scroll_line_extent = value;
        self
    }

    /// Sets the nominal square extent of a whole-tab-stack grip.
    #[must_use]
    pub const fn tab_group_grip_extent(mut self, value: f64) -> Self {
        self.tab_group_grip_extent = value;
        self
    }

    /// Sets horizontal padding on each side of tab content.
    #[must_use]
    pub const fn tab_horizontal_padding(mut self, value: f64) -> Self {
        self.tab_horizontal_padding = value;
        self
    }

    /// Sets the minimum allocated tab width.
    #[must_use]
    pub const fn tab_min_width(mut self, value: f64) -> Self {
        self.tab_min_width = value;
        self
    }

    /// Sets the maximum allocated tab width.
    #[must_use]
    pub const fn tab_max_width(mut self, value: f64) -> Self {
        self.tab_max_width = value;
        self
    }

    /// Sets the square extent reserved for a tab close control.
    #[must_use]
    pub const fn tab_close_extent(mut self, value: f64) -> Self {
        self.tab_close_extent = value;
        self
    }

    /// Sets the visible splitter thickness.
    #[must_use]
    pub const fn splitter_thickness(mut self, value: f64) -> Self {
        self.splitter_thickness = value;
        self
    }

    /// Sets the total splitter interaction extent.
    #[must_use]
    pub const fn splitter_hit_extent(mut self, value: f64) -> Self {
        self.splitter_hit_extent = value;
        self
    }

    /// Sets the logical keyboard adjustment step for splitters.
    #[must_use]
    pub const fn splitter_keyboard_step(mut self, value: f64) -> Self {
        self.splitter_keyboard_step = value;
        self
    }

    /// Sets the logical source-surface distance required to begin a pointer drag.
    #[must_use]
    pub const fn pointer_drag_start_distance(mut self, value: f64) -> Self {
        self.pointer_drag_start_distance = value;
        self
    }

    /// Sets the width and height of one docking guide button.
    #[must_use]
    pub const fn guide_extent(mut self, value: f64) -> Self {
        self.guide_extent = value;
        self
    }

    /// Sets the visible gap between adjacent inner guide buttons.
    #[must_use]
    pub const fn guide_gap(mut self, value: f64) -> Self {
        self.guide_gap = value;
        self
    }

    /// Sets the exact hit padding around a docking guide button.
    #[must_use]
    pub const fn guide_hit_padding(mut self, value: f64) -> Self {
        self.guide_hit_padding = value;
        self
    }

    /// Sets the distance from a root edge to an outer guide center.
    #[must_use]
    pub const fn guide_outer_inset(mut self, value: f64) -> Self {
        self.guide_outer_inset = value;
        self
    }

    /// Sets the fraction assigned to a newly docked edge child.
    #[must_use]
    pub const fn dock_fraction(mut self, value: f64) -> Self {
        self.dock_fraction = value;
        self
    }

    /// Sets the height of a contained-floating title bar.
    #[must_use]
    pub const fn floating_title_height(mut self, value: f64) -> Self {
        self.floating_title_height = value;
        self
    }

    /// Sets the visible contained-floating border width.
    #[must_use]
    pub const fn floating_border_width(mut self, value: f64) -> Self {
        self.floating_border_width = value;
        self
    }

    /// Sets the total contained-floating resize interaction extent.
    #[must_use]
    pub const fn floating_resize_extent(mut self, value: f64) -> Self {
        self.floating_resize_extent = value;
        self
    }

    /// Sets the global minimum pane content size.
    #[must_use]
    pub const fn minimum_pane_size(mut self, width: f64, height: f64) -> Self {
        self.minimum_pane_width = width;
        self.minimum_pane_height = height;
        self
    }

    /// Sets the global minimum outer size of a contained-floating surface.
    #[must_use]
    pub const fn minimum_floating_size(mut self, width: f64, height: f64) -> Self {
        self.minimum_floating_width = width;
        self.minimum_floating_height = height;
        self
    }

    /// Validates all metrics and constructs an immutable configuration.
    ///
    /// # Errors
    ///
    /// Returns [`DockPresentationConfigError`] for non-finite values, invalid
    /// signs, or a violated cross-field geometry invariant.
    pub fn build(self) -> Result<DockPresentationConfig, DockPresentationConfigError> {
        for (field, value) in [
            ("tab_bar_height", self.tab_bar_height),
            (
                "tab_strip_scroll_line_extent",
                self.tab_strip_scroll_line_extent,
            ),
            ("tab_group_grip_extent", self.tab_group_grip_extent),
            ("tab_min_width", self.tab_min_width),
            ("tab_max_width", self.tab_max_width),
            ("tab_close_extent", self.tab_close_extent),
            ("splitter_thickness", self.splitter_thickness),
            ("splitter_hit_extent", self.splitter_hit_extent),
            ("splitter_keyboard_step", self.splitter_keyboard_step),
            (
                "pointer_drag_start_distance",
                self.pointer_drag_start_distance,
            ),
            ("guide_extent", self.guide_extent),
            ("guide_outer_inset", self.guide_outer_inset),
            ("floating_title_height", self.floating_title_height),
            ("floating_resize_extent", self.floating_resize_extent),
            ("minimum_pane_size.width", self.minimum_pane_width),
            ("minimum_pane_size.height", self.minimum_pane_height),
            ("minimum_floating_size.width", self.minimum_floating_width),
            ("minimum_floating_size.height", self.minimum_floating_height),
        ] {
            require_positive(field, value)?;
        }
        for (field, value) in [
            ("tab_horizontal_padding", self.tab_horizontal_padding),
            ("guide_gap", self.guide_gap),
            ("guide_hit_padding", self.guide_hit_padding),
            ("floating_border_width", self.floating_border_width),
        ] {
            require_non_negative(field, value)?;
        }
        require_finite("dock_fraction", self.dock_fraction)?;
        if self.dock_fraction <= 0.0 || self.dock_fraction >= 1.0 {
            return Err(DockPresentationConfigError::DockFractionOutOfRange {
                value: self.dock_fraction,
            });
        }
        if self.tab_min_width > self.tab_max_width {
            return Err(DockPresentationConfigError::MinimumExceedsMaximum {
                minimum: "tab_min_width",
                maximum: "tab_max_width",
            });
        }
        if self.tab_close_extent > self.tab_bar_height {
            return Err(DockPresentationConfigError::ExtentExceedsContainer {
                extent: "tab_close_extent",
                container: "tab_bar_height",
            });
        }
        if self.tab_group_grip_extent > self.tab_bar_height {
            return Err(DockPresentationConfigError::ExtentExceedsContainer {
                extent: "tab_group_grip_extent",
                container: "tab_bar_height",
            });
        }
        if self.splitter_hit_extent < self.splitter_thickness {
            return Err(DockPresentationConfigError::HitExtentSmallerThanVisible {
                hit_extent: "splitter_hit_extent",
                visible_extent: "splitter_thickness",
            });
        }
        if self.floating_resize_extent < self.floating_border_width {
            return Err(DockPresentationConfigError::HitExtentSmallerThanVisible {
                hit_extent: "floating_resize_extent",
                visible_extent: "floating_border_width",
            });
        }
        if self.guide_gap < 2.0 * self.guide_hit_padding {
            return Err(DockPresentationConfigError::GuideHitRegionsOverlap);
        }
        let guide_hit_half = self.guide_extent * 0.5 + self.guide_hit_padding;
        if self.guide_outer_inset < guide_hit_half {
            return Err(DockPresentationConfigError::GuideOuterInsetTooSmall);
        }
        let inner_guide_span =
            3.0 * self.guide_extent + 2.0 * self.guide_gap + 2.0 * self.guide_hit_padding;
        if !inner_guide_span.is_finite() {
            return Err(DockPresentationConfigError::NonFiniteAggregate {
                aggregate: "inner_guide_span",
            });
        }
        let outer_guide_span = 2.0 * (self.guide_outer_inset + 2.0 * guide_hit_half);
        if !outer_guide_span.is_finite() {
            return Err(DockPresentationConfigError::NonFiniteAggregate {
                aggregate: "outer_guide_span",
            });
        }

        Ok(DockPresentationConfig {
            tab_bar_height: self.tab_bar_height,
            tab_strip_scroll_line_extent: self.tab_strip_scroll_line_extent,
            tab_group_grip_extent: self.tab_group_grip_extent,
            tab_horizontal_padding: self.tab_horizontal_padding,
            tab_min_width: self.tab_min_width,
            tab_max_width: self.tab_max_width,
            tab_close_extent: self.tab_close_extent,
            splitter_thickness: self.splitter_thickness,
            splitter_hit_extent: self.splitter_hit_extent,
            splitter_keyboard_step: self.splitter_keyboard_step,
            pointer_drag_start_distance: self.pointer_drag_start_distance,
            guide_extent: self.guide_extent,
            guide_gap: self.guide_gap,
            guide_hit_padding: self.guide_hit_padding,
            guide_outer_inset: self.guide_outer_inset,
            dock_fraction: self.dock_fraction,
            floating_title_height: self.floating_title_height,
            floating_border_width: self.floating_border_width,
            floating_resize_extent: self.floating_resize_extent,
            minimum_pane_size: LogicalSize::new(self.minimum_pane_width, self.minimum_pane_height)?,
            minimum_floating_size: LogicalSize::new(
                self.minimum_floating_width,
                self.minimum_floating_height,
            )?,
        })
    }
}

fn require_finite(field: &'static str, value: f64) -> Result<(), DockPresentationConfigError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(DockPresentationConfigError::NonFinite { field, value })
    }
}

fn require_positive(field: &'static str, value: f64) -> Result<(), DockPresentationConfigError> {
    require_finite(field, value)?;
    if value > 0.0 {
        Ok(())
    } else {
        Err(DockPresentationConfigError::NonPositive { field, value })
    }
}

fn require_non_negative(
    field: &'static str,
    value: f64,
) -> Result<(), DockPresentationConfigError> {
    require_finite(field, value)?;
    if value >= 0.0 {
        Ok(())
    } else {
        Err(DockPresentationConfigError::Negative { field, value })
    }
}

/// Why core presentation geometry could not be constructed.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum DockPresentationConfigError {
    /// A scalar was NaN or infinite.
    #[error("presentation metric `{field}` must be finite, got {value}")]
    NonFinite {
        /// Name of the rejected metric.
        field: &'static str,
        /// Rejected value.
        value: f64,
    },
    /// A strictly positive scalar was zero or negative.
    #[error("presentation metric `{field}` must be positive, got {value}")]
    NonPositive {
        /// Name of the rejected metric.
        field: &'static str,
        /// Rejected value.
        value: f64,
    },
    /// A non-negative scalar was negative.
    #[error("presentation metric `{field}` must be non-negative, got {value}")]
    Negative {
        /// Name of the rejected metric.
        field: &'static str,
        /// Rejected value.
        value: f64,
    },
    /// The docking fraction was outside the open interval `(0, 1)`.
    #[error("dock fraction must be strictly between zero and one, got {value}")]
    DockFractionOutOfRange {
        /// Rejected fraction.
        value: f64,
    },
    /// A configured minimum exceeded its corresponding maximum.
    #[error("presentation minimum `{minimum}` exceeds maximum `{maximum}`")]
    MinimumExceedsMaximum {
        /// Minimum field name.
        minimum: &'static str,
        /// Maximum field name.
        maximum: &'static str,
    },
    /// A control extent exceeded the container that must hold it.
    #[error("presentation extent `{extent}` exceeds container `{container}`")]
    ExtentExceedsContainer {
        /// Extent field name.
        extent: &'static str,
        /// Container field name.
        container: &'static str,
    },
    /// An interaction extent was smaller than the visible geometry it covers.
    #[error("hit extent `{hit_extent}` is smaller than visible extent `{visible_extent}`")]
    HitExtentSmallerThanVisible {
        /// Interaction extent field name.
        hit_extent: &'static str,
        /// Visible extent field name.
        visible_extent: &'static str,
    },
    /// Expanded inner-guide hit regions would overlap.
    #[error("guide gap must be at least twice guide hit padding")]
    GuideHitRegionsOverlap,
    /// An outer guide and its hit padding would cross the root boundary.
    #[error("guide outer inset must contain half a guide plus its hit padding")]
    GuideOuterInsetTooSmall,
    /// Finite inputs overflowed while an aggregate was calculated.
    #[error("presentation aggregate `{aggregate}` is non-finite")]
    NonFiniteAggregate {
        /// Aggregate that overflowed.
        aggregate: &'static str,
    },
    /// Validated size conversion unexpectedly failed.
    #[error(transparent)]
    Geometry(#[from] GeometryError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_geometry_is_valid_and_renderer_neutral() {
        let config = DockPresentationConfig::builder()
            .build()
            .expect("baseline geometry should be valid");

        assert_eq!(config, DockPresentationConfig::default());
        assert_eq!(config.tab_bar_height().to_bits(), 28.0_f64.to_bits());
        assert_eq!(config.dock_fraction().to_bits(), 0.5_f64.to_bits());
        assert_eq!(
            config.pointer_drag_start_distance().to_bits(),
            6.0_f64.to_bits()
        );
        assert_eq!(config.guide_extent().to_bits(), 24.0_f64.to_bits());
        assert_eq!(config.guide_hit_padding().to_bits(), 4.0_f64.to_bits());
        assert_eq!(
            config.minimum_pane_size().width().to_bits(),
            80.0_f64.to_bits()
        );
        assert_eq!(
            config.minimum_floating_size().height().to_bits(),
            120.0_f64.to_bits()
        );
    }

    #[test]
    fn rejects_non_finite_and_negative_metrics() {
        assert!(matches!(
            DockPresentationConfig::builder()
                .splitter_thickness(f64::NAN)
                .build(),
            Err(DockPresentationConfigError::NonFinite {
                field: "splitter_thickness",
                ..
            })
        ));
        assert!(matches!(
            DockPresentationConfig::builder()
                .guide_hit_padding(-1.0)
                .build(),
            Err(DockPresentationConfigError::Negative {
                field: "guide_hit_padding",
                ..
            })
        ));
        assert!(matches!(
            DockPresentationConfig::builder()
                .minimum_pane_size(0.0, 60.0)
                .build(),
            Err(DockPresentationConfigError::NonPositive {
                field: "minimum_pane_size.width",
                ..
            })
        ));
        assert!(matches!(
            DockPresentationConfig::builder()
                .pointer_drag_start_distance(0.0)
                .build(),
            Err(DockPresentationConfigError::NonPositive {
                field: "pointer_drag_start_distance",
                ..
            })
        ));
    }

    #[test]
    fn rejects_cross_field_geometry_conflicts() {
        assert!(matches!(
            DockPresentationConfig::builder()
                .splitter_thickness(8.0)
                .splitter_hit_extent(6.0)
                .build(),
            Err(DockPresentationConfigError::HitExtentSmallerThanVisible { .. })
        ));
        assert!(matches!(
            DockPresentationConfig::builder()
                .guide_gap(3.0)
                .guide_hit_padding(2.0)
                .build(),
            Err(DockPresentationConfigError::GuideHitRegionsOverlap)
        ));
        assert!(matches!(
            DockPresentationConfig::builder().dock_fraction(1.0).build(),
            Err(DockPresentationConfigError::DockFractionOutOfRange { value: 1.0 })
        ));
    }

    #[test]
    fn nominal_guide_size_may_exceed_the_pane_minimum_because_compilation_compacts_it() {
        let config = DockPresentationConfig::builder()
            .guide_extent(64.0)
            .guide_gap(8.0)
            .guide_hit_padding(2.0)
            .guide_outer_inset(40.0)
            .minimum_pane_size(10.0, 10.0)
            .build()
            .expect("guide footprint is compacted against actual bounds");

        assert!(config.inner_guide_reference_span() > config.minimum_pane_size().width());
        assert!(config.outer_guide_reference_span() > config.minimum_pane_size().height());
    }

    #[test]
    fn presentation_revision_never_wraps() {
        let revision = PresentationConfigRevision::new(41);
        assert_eq!(
            revision.checked_next(),
            Some(PresentationConfigRevision::new(42))
        );
        assert_eq!(
            PresentationConfigRevision::new(u64::MAX).checked_next(),
            None
        );
    }
}

//! Fixed geometry and egui-relative visual configuration for the adapter.

use std::{error::Error, fmt};

use dockspace::runtime::{DockPresentationConfig, DockPresentationConfigError};
use egui::{Color32, Rgba, Vec2, Visuals};

/// Optional visual overrides applied on top of the current egui theme.
///
/// Every field defaults to `None`, so dockspace follows the [`egui::Visuals`]
/// active on the [`egui::Ui`] that paints it. Set only the colors that belong to
/// an application-specific visual language; theme changes continue to affect
/// every color left unspecified.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DockVisualOverrides {
    /// Fill behind tiled docking content.
    pub workspace_fill: Option<Color32>,
    /// Fill behind tabs in a tab bar.
    pub tab_bar_fill: Option<Color32>,
    /// Fill of an inactive tab.
    pub tab_fill: Option<Color32>,
    /// Fill of a hovered inactive tab.
    pub tab_hover_fill: Option<Color32>,
    /// Fill of the selected tab.
    pub tab_active_fill: Option<Color32>,
    /// Text color of inactive tabs.
    pub tab_text_color: Option<Color32>,
    /// Text color of the selected tab.
    pub tab_active_text_color: Option<Color32>,
    /// Fill of an idle splitter.
    pub splitter_color: Option<Color32>,
    /// Fill of a hovered, dragged, or keyboard-focused splitter.
    pub splitter_hover_color: Option<Color32>,
    /// Translucent fill of the resolved drop target.
    pub drop_fill: Option<Color32>,
    /// Border color of the resolved drop target.
    pub drop_border_color: Option<Color32>,
    /// Fill of an available passive docking guide.
    pub drop_guide_fill: Option<Color32>,
    /// Fill of the exact active docking guide.
    pub drop_guide_active_fill: Option<Color32>,
    /// Border and directional cue color of a passive docking guide.
    pub drop_guide_border_color: Option<Color32>,
    /// Border and directional cue color of the exact active docking guide.
    pub drop_guide_active_border_color: Option<Color32>,
    /// Fill of contained-floating surfaces and menus.
    pub floating_fill: Option<Color32>,
    /// Fill of an inactive contained-floating title bar.
    pub floating_title_fill: Option<Color32>,
    /// Border color of contained-floating surfaces and menus.
    pub floating_border_color: Option<Color32>,
    /// Translucent fill of a drag ghost.
    pub ghost_fill: Option<Color32>,
    /// Border color of a drag ghost.
    pub ghost_border_color: Option<Color32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ResolvedDockVisuals {
    pub(crate) workspace_fill: Color32,
    pub(crate) tab_bar_fill: Color32,
    pub(crate) tab_fill: Color32,
    pub(crate) tab_hover_fill: Color32,
    pub(crate) tab_active_fill: Color32,
    pub(crate) tab_text_color: Color32,
    pub(crate) tab_active_text_color: Color32,
    pub(crate) splitter_color: Color32,
    pub(crate) splitter_hover_color: Color32,
    pub(crate) drop_fill: Color32,
    pub(crate) drop_border_color: Color32,
    pub(crate) drop_guide_fill: Color32,
    pub(crate) drop_guide_active_fill: Color32,
    pub(crate) drop_guide_border_color: Color32,
    pub(crate) drop_guide_active_border_color: Color32,
    pub(crate) floating_fill: Color32,
    pub(crate) floating_title_fill: Color32,
    pub(crate) floating_border_color: Color32,
    pub(crate) ghost_fill: Color32,
    pub(crate) ghost_border_color: Color32,
}

impl DockVisualOverrides {
    fn resolve(&self, visuals: &Visuals) -> ResolvedDockVisuals {
        let tab_bar_fill = if visuals.dark_mode {
            visuals.extreme_bg_color
        } else {
            (Rgba::from(visuals.panel_fill) * Rgba::from_gray(0.8)).into()
        };
        let selection = visuals.selection.bg_fill;
        ResolvedDockVisuals {
            workspace_fill: self.workspace_fill.unwrap_or(visuals.panel_fill),
            tab_bar_fill: self.tab_bar_fill.unwrap_or(tab_bar_fill),
            tab_fill: self.tab_fill.unwrap_or(Color32::TRANSPARENT),
            tab_hover_fill: self
                .tab_hover_fill
                .unwrap_or(visuals.widgets.hovered.weak_bg_fill),
            tab_active_fill: self.tab_active_fill.unwrap_or(visuals.panel_fill),
            tab_text_color: self
                .tab_text_color
                .unwrap_or(visuals.widgets.inactive.fg_stroke.color),
            tab_active_text_color: self
                .tab_active_text_color
                .unwrap_or(visuals.widgets.active.fg_stroke.color),
            splitter_color: self.splitter_color.unwrap_or(tab_bar_fill),
            splitter_hover_color: self
                .splitter_hover_color
                .unwrap_or(visuals.widgets.hovered.fg_stroke.color),
            drop_fill: self.drop_fill.unwrap_or_else(|| with_alpha(selection, 72)),
            drop_border_color: self
                .drop_border_color
                .unwrap_or(visuals.selection.stroke.color),
            drop_guide_fill: self
                .drop_guide_fill
                .unwrap_or(visuals.widgets.inactive.bg_fill),
            drop_guide_active_fill: self.drop_guide_active_fill.unwrap_or(selection),
            drop_guide_border_color: self
                .drop_guide_border_color
                .unwrap_or(visuals.widgets.inactive.fg_stroke.color),
            drop_guide_active_border_color: self
                .drop_guide_active_border_color
                .unwrap_or(visuals.selection.stroke.color),
            floating_fill: self.floating_fill.unwrap_or_else(|| visuals.window_fill()),
            floating_title_fill: self
                .floating_title_fill
                .unwrap_or(visuals.widgets.noninteractive.bg_fill),
            floating_border_color: self
                .floating_border_color
                .unwrap_or(visuals.window_stroke.color),
            ghost_fill: self.ghost_fill.unwrap_or_else(|| with_alpha(selection, 64)),
            ghost_border_color: self
                .ghost_border_color
                .unwrap_or(visuals.selection.stroke.color),
        }
    }
}

fn with_alpha(color: Color32, alpha: u8) -> Color32 {
    let [red, green, blue, _] = color.to_srgba_unmultiplied();
    Color32::from_rgba_unmultiplied(red, green, blue, alpha)
}

/// Configurable geometry and visual overrides used to paint a docking scene.
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
    /// Per-color overrides layered over the current egui theme.
    pub visuals: DockVisualOverrides,
}

impl DockStyle {
    pub(crate) fn resolved_visuals(&self, visuals: &Visuals) -> ResolvedDockVisuals {
        self.visuals.resolve(visuals)
    }

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
        Ok(())
    }
}

impl Default for DockStyle {
    fn default() -> Self {
        let geometry = DockPresentationConfig::default();
        Self {
            tab_bar_height: geometry.tab_bar_height() as f32,
            tab_group_grip_extent: geometry.tab_group_grip_extent() as f32,
            tab_horizontal_padding: geometry.tab_horizontal_padding() as f32,
            tab_min_width: geometry.tab_min_width() as f32,
            tab_max_width: geometry.tab_max_width() as f32,
            tab_close_size: geometry.tab_close_extent() as f32,
            splitter_thickness: geometry.splitter_thickness() as f32,
            splitter_hit_extent: geometry.splitter_hit_extent() as f32,
            splitter_keyboard_step: geometry.splitter_keyboard_step() as f32,
            drop_guide_extent: geometry.guide_extent() as f32,
            drop_guide_gap: geometry.guide_gap() as f32,
            drop_guide_hit_padding: geometry.guide_hit_padding() as f32,
            drop_guide_outer_inset: geometry.guide_outer_inset() as f32,
            dock_fraction: geometry.dock_fraction() as f32,
            floating_title_height: geometry.floating_title_height() as f32,
            floating_border_width: geometry.floating_border_width() as f32,
            floating_resize_extent: geometry.floating_resize_extent() as f32,
            minimum_pane_size: Vec2::new(
                geometry.minimum_pane_size().width() as f32,
                geometry.minimum_pane_size().height() as f32,
            ),
            minimum_floating_size: Vec2::new(
                geometry.minimum_floating_size().width() as f32,
                geometry.minimum_floating_size().height() as f32,
            ),
            ghost_offset: Vec2::new(12.0, 12.0),
            visuals: DockVisualOverrides::default(),
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
    fn default_visuals_follow_egui_theme_tokens() {
        let mut egui_visuals = Visuals::dark();
        egui_visuals.panel_fill = Color32::from_rgb(11, 22, 33);
        egui_visuals.extreme_bg_color = Color32::from_rgb(4, 5, 6);
        egui_visuals.selection.bg_fill = Color32::from_rgb(44, 55, 66);
        egui_visuals.window_fill = Color32::from_rgb(77, 88, 99);
        let resolved = DockStyle::default().resolved_visuals(&egui_visuals);

        assert_eq!(resolved.workspace_fill, egui_visuals.panel_fill);
        assert_eq!(resolved.tab_active_fill, egui_visuals.panel_fill);
        assert_eq!(resolved.tab_fill, Color32::TRANSPARENT);
        assert_eq!(resolved.tab_bar_fill, egui_visuals.extreme_bg_color);
        assert_eq!(resolved.floating_fill, egui_visuals.window_fill());
        assert_eq!(
            resolved.drop_guide_active_fill,
            egui_visuals.selection.bg_fill
        );

        let light = DockStyle::default().resolved_visuals(&Visuals::light());
        assert_eq!(light.tab_bar_fill.a(), 255);
    }

    #[test]
    fn translucent_theme_tokens_keep_their_unmultiplied_color_channels() {
        let mut egui_visuals = Visuals::dark();
        let selection = Color32::from_rgba_unmultiplied(160, 96, 48, 128);
        let [red, green, blue, _] = selection.to_srgba_unmultiplied();
        egui_visuals.selection.bg_fill = selection;
        let resolved = DockStyle::default().resolved_visuals(&egui_visuals);

        assert_eq!(
            resolved.drop_fill,
            Color32::from_rgba_unmultiplied(red, green, blue, 72)
        );
        assert_eq!(
            resolved.ghost_fill,
            Color32::from_rgba_unmultiplied(red, green, blue, 64)
        );
    }

    #[test]
    fn visual_override_changes_only_its_named_token() {
        let egui_visuals = Visuals::dark();
        let mut style = DockStyle::default();
        let override_fill = Color32::from_rgb(91, 73, 42);
        style.visuals.workspace_fill = Some(override_fill);
        let resolved = style.resolved_visuals(&egui_visuals);

        assert_eq!(resolved.workspace_fill, override_fill);
        assert_eq!(resolved.tab_active_fill, egui_visuals.panel_fill);
        assert_eq!(resolved.tab_bar_fill, egui_visuals.extreme_bg_color);
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

        style.drop_guide_gap = 12.0;
        style.drop_guide_outer_inset = 9.0;
        assert_eq!(
            style.validate(),
            Err(DockStyleError::GuideOuterInsetTooSmall)
        );

        style.drop_guide_outer_inset = 64.0;
        assert_eq!(style.validate(), Ok(()));
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

//! Fixed geometry and egui-relative visual configuration for the adapter.

use dockspace::runtime::{
    DockPresentationConfig, DockPresentationConfigBuilder, DockPresentationConfigError,
    PaneOverflowPolicy,
};
use egui::{Color32, CornerRadius, Frame, Margin, Rgba, Stroke, Style as EguiStyle, Vec2, Visuals};

mod frame;
pub use frame::DockFrameVisuals;

/// Optional visual overrides applied on top of the current egui theme.
///
/// Every override defaults to unset, so dockspace follows the [`egui::Visuals`]
/// active on the [`egui::Ui`] that paints it. Set only the colors that belong to
/// an application-specific visual language; theme changes continue to affect
/// every color left unspecified. Optional tab outlines default to no stroke.
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
    /// Outline of an inactive tab; defaults to no outline.
    pub tab_stroke: Option<Stroke>,
    /// Outline of the selected tab; defaults to no outline.
    pub tab_active_stroke: Option<Stroke>,
    /// Text color of inactive tabs.
    pub tab_text_color: Option<Color32>,
    /// Text color of the selected tab.
    pub tab_active_text_color: Option<Color32>,
    /// Underline of the selected tab whose pane owns observed content focus.
    ///
    /// Defaults to two points in egui's selection color; `Stroke::NONE` hides it.
    pub pane_focus_stroke: Option<Stroke>,
    /// Keyboard-focus outline, independent of drag-and-drop colors.
    pub tab_focus_stroke: Option<Stroke>,
    /// Fill of an idle splitter.
    pub splitter_color: Option<Color32>,
    /// Fill of a hovered, dragged, or keyboard-focused splitter.
    pub splitter_hover_color: Option<Color32>,
    /// Translucent fill of the resulting docking area in the candidate layout.
    pub drop_fill: Option<Color32>,
    /// Border color of the resulting docking area in the candidate layout.
    pub drop_border_color: Option<Color32>,
    /// Fill of an available passive docking guide.
    pub drop_guide_fill: Option<Color32>,
    /// Fill of the exact active docking guide.
    pub drop_guide_active_fill: Option<Color32>,
    /// Border and directional cue color of a passive docking guide.
    pub drop_guide_border_color: Option<Color32>,
    /// Border and directional cue color of the exact active docking guide.
    pub drop_guide_active_border_color: Option<Color32>,
    /// Contained-floating frames, independent of popup menus.
    pub floating: DockFrameVisuals,
    /// Tab-list and presentation-menu frames.
    pub popup: DockFrameVisuals,
    /// Fill of an inactive contained-floating title bar.
    pub floating_title_fill: Option<Color32>,
    /// Fill of the frontmost contained-floating title bar.
    pub floating_title_active_fill: Option<Color32>,
    /// Text and grip color of contained-floating title bars.
    pub floating_title_text_color: Option<Color32>,
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
    pub(crate) pane_focus_stroke: Stroke,
    pub(crate) tab_focus_stroke: Stroke,
    pub(crate) splitter_color: Color32,
    pub(crate) splitter_hover_color: Color32,
    pub(crate) drop_fill: Color32,
    pub(crate) drop_border_color: Color32,
    pub(crate) drop_guide_fill: Color32,
    pub(crate) drop_guide_active_fill: Color32,
    pub(crate) drop_guide_border_color: Color32,
    pub(crate) drop_guide_active_border_color: Color32,
    pub(crate) popup_fill: Color32,
    pub(crate) floating_title_fill: Color32,
    pub(crate) ghost_fill: Color32,
    pub(crate) ghost_border_color: Color32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ResolvedContainedWindowVisuals {
    pub(crate) frame: Frame,
    pub(crate) title_fill: Color32,
    pub(crate) title_text_color: Color32,
    pub(crate) title_margin: Margin,
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
                .or(visuals.override_text_color)
                .unwrap_or(visuals.widgets.inactive.fg_stroke.color),
            tab_active_text_color: self
                .tab_active_text_color
                .or(visuals.override_text_color)
                .unwrap_or(visuals.widgets.active.fg_stroke.color),
            pane_focus_stroke: self
                .pane_focus_stroke
                .unwrap_or(Stroke::new(2.0, visuals.selection.stroke.color)),
            tab_focus_stroke: self.tab_focus_stroke.unwrap_or(visuals.selection.stroke),
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
            popup_fill: self.popup.fill.unwrap_or_else(|| visuals.window_fill()),
            floating_title_fill: self
                .floating_title_fill
                .unwrap_or(visuals.widgets.noninteractive.bg_fill),
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

/// Initial docking chrome density relative to the current egui fonts.
///
/// This only changes docking geometry. Pane fonts, application widgets, zoom
/// and the separate touch resize target remain under their existing settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DockDensity {
    /// Reduced vertical padding, shorter group grips and narrower short tabs.
    Compact,
    /// Follow egui button spacing and interaction height.
    #[default]
    Comfortable,
}

/// When to paint a tab's close icon. Its allocated hit target remains stable.
/// Keyboard focus on the tab or close control always reveals the icon.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TabCloseVisibility {
    /// Paint every allocated close icon, including those on inactive tabs.
    Always,
    /// Paint it for selected tabs and for hovered tabs or close controls.
    #[default]
    SelectedOrHovered,
    /// Reveal it on tab or close-control hover, in addition to keyboard focus.
    Hovered,
}

/// Presentation of the whole-group grip. The trailing empty bar remains draggable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TabGroupGripVisibility {
    /// Paint every enabled group's leading grip.
    #[default]
    Always,
    /// Reserve its space, painting it while the enabled tab bar is hovered.
    Hovered,
    /// Reclaims the leading grip's space in core layout.
    Hidden,
}

/// Configurable docking geometry, interaction timing and visual overrides.
///
/// All geometry is expressed in egui points. Values are fixed for a frame and
/// are never derived from viewport size, pointer velocity, elapsed time, or DPI.
#[derive(Clone, Debug, PartialEq)]
pub struct DockStyle {
    /// Delay before an active drag selects its hovered tab; `None` disables it.
    pub hover_selection_delay: Option<std::time::Duration>,
    /// Delay before a touch press becomes a long press.
    pub touch_long_press_delay: std::time::Duration,
    /// Logical movement distance allowed before an unarmed touch yields to content.
    pub touch_press_slop_distance: f32,
    /// Requested touch resize hit extent in logical points before clipping.
    ///
    /// Must be finite and at least 44 points. Surface bounds, protected chrome,
    /// and crowding may shrink the actual hit region or leave it empty. Mouse,
    /// pen, and unknown-device hit geometry are unaffected.
    pub touch_resize_hit_extent: f32,
    /// Height of every tab bar.
    pub tab_bar_height: f32,
    /// Radius of tab backgrounds and outlines; independent of core hit geometry.
    pub tab_corner_radius: CornerRadius,
    /// Width of each tab-strip edge scroll band in egui points; zero disables it.
    pub tab_strip_edge_scroll_band: f32,
    /// Positive interval between edge scroll steps; `None` disables edge scrolling.
    pub tab_strip_edge_scroll_interval: Option<std::time::Duration>,
    /// Nominal square extent of the whole-tab-stack grip.
    pub tab_group_grip_extent: f32,
    /// Changes grip painting; `Hidden` also removes its core allocation.
    pub tab_group_grip_visibility: TabGroupGripVisibility,
    /// Horizontal padding on each side of tab content.
    pub tab_horizontal_padding: f32,
    /// Minimum width of a tab.
    pub tab_min_width: f32,
    /// Maximum width of a tab.
    pub tab_max_width: f32,
    /// Width and height of a tab close control.
    pub tab_close_size: f32,
    /// Positive visual cross size, clamped to the independent close hit target.
    pub tab_close_icon_size: f32,
    /// Controls icon painting without changing the tab's width or close target.
    pub tab_close_visibility: TabCloseVisibility,
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
    /// Layout behavior when pane minima cannot fit the available split extent.
    pub pane_overflow_policy: PaneOverflowPolicy,
    /// Global minimum outer size of a contained-floating surface.
    pub minimum_floating_size: Vec2,
    /// Pointer-relative offset used to paint a drag ghost.
    pub ghost_offset: Vec2,
    /// Visual overrides layered over the current egui theme.
    pub visuals: DockVisualOverrides,
}

impl DockStyle {
    /// Derives initial chrome geometry from the current egui fonts and spacing.
    ///
    /// Call this inside a UI pass after configuring fonts. The returned metrics
    /// are ordinary fixed docking geometry, not a live theme subscription.
    /// Reapply them explicitly with `Dockspace::set_style` after changing font
    /// size or density. A native host must construct its session from the same
    /// [`Self::presentation_config`]. Whole-UI zoom needs no geometry update.
    #[must_use]
    pub fn from_egui(ui: &egui::Ui) -> Self {
        Self::from_egui_with_density(ui, DockDensity::Comfortable)
    }

    /// Derives docking geometry with an explicit compact or comfortable density.
    ///
    /// Compact chrome reduces padding and minimum tab width without shrinking
    /// text. Both densities retain enough body width beside the close control
    /// for keyboard and pointer interaction at the current font size.
    ///
    /// Like [`Self::from_egui`], call this during a UI pass and reapply it after
    /// font/density changes. The returned style remains ordinary editable data.
    #[must_use]
    pub fn from_egui_with_density(ui: &egui::Ui, density: DockDensity) -> Self {
        let font = egui::FontSelection::Default
            .resolve_with_fallback(ui.style(), egui::TextStyle::Button.into());
        let font_height = ui.fonts_mut(|fonts| fonts.row_height(&font));
        let spacing = ui.spacing();
        let height = (font_height + spacing.button_padding.y * 2.0).max(spacing.interact_size.y);
        let compact = density == DockDensity::Compact;
        let height = if compact {
            (font_height + 4.0).min(height)
        } else {
            height
        };
        let padding = if compact {
            spacing.button_padding.x.min(6.0)
        } else {
            spacing.button_padding.x
        };
        let mut result = Self {
            tab_bar_height: height,
            floating_title_height: height,
            tab_group_grip_extent: if compact { font_height } else { height },
            tab_close_size: font_height,
            tab_close_icon_size: font_height * 0.5,
            tab_horizontal_padding: padding,
            floating_border_width: ui.visuals().window_stroke.width,
            ..Self::default()
        };
        let minimum = if compact { 48.0 } else { result.tab_min_width };
        result.tab_min_width = minimum.max(font_height * 2.0 + padding * 2.0);
        result.tab_max_width = result.tab_max_width.max(result.tab_min_width);
        result.floating_resize_extent = result
            .floating_resize_extent
            .max(result.floating_border_width);
        result
    }

    pub(crate) fn resolved_visuals(&self, visuals: &Visuals) -> ResolvedDockVisuals {
        self.visuals.resolve(visuals)
    }

    pub(crate) fn resolved_popup_frame(&self, style: &EguiStyle) -> Frame {
        self.visuals.popup.apply(Frame::popup(style))
    }

    pub(crate) fn resolved_contained_window(
        &self,
        style: &EguiStyle,
        active: bool,
    ) -> ResolvedContainedWindowVisuals {
        let mut frame = self.visuals.floating.apply(Frame::window(style));
        let title_margin = frame.inner_margin;
        frame.inner_margin = Margin::ZERO;
        frame.outer_margin = Margin::ZERO;
        frame.stroke.width = self.floating_border_width;
        let title_fill = if active {
            self.visuals
                .floating_title_active_fill
                .unwrap_or(style.visuals.widgets.open.weak_bg_fill)
        } else {
            self.visuals.floating_title_fill.unwrap_or(frame.fill)
        };
        ResolvedContainedWindowVisuals {
            frame,
            title_fill,
            title_text_color: self
                .visuals
                .floating_title_text_color
                .unwrap_or(style.visuals.widgets.noninteractive.fg_stroke.color),
            title_margin,
        }
    }

    /// Converts the geometry and interaction settings represented by this style
    /// into a validated core presentation configuration.
    ///
    /// Core settings not represented by [`DockStyle`], such as pointer drag
    /// thresholds and magnetic targeting settings, retain their core defaults.
    /// Use this conversion when creating a session; do not replace an existing
    /// session's configuration with it to synchronize a native renderer.
    /// Visual overrides and the drag-ghost offset remain adapter-local.
    ///
    /// # Errors
    ///
    /// Returns an error when adapter visuals are invalid or the represented
    /// geometry and interaction settings violate core configuration constraints.
    pub fn presentation_config(&self) -> Result<DockPresentationConfig, DockStyleError> {
        self.configure_presentation(DockPresentationConfig::builder())
    }

    /// Updates represented settings while preserving core-only configuration.
    ///
    /// Use this when changing a live session's style. Pointer thresholds,
    /// magnetic targeting and other settings absent from `DockStyle` retain
    /// their current values.
    ///
    /// # Errors
    ///
    /// Returns an error when adapter visuals, combined geometry or timing are invalid.
    pub fn presentation_config_with(
        &self,
        current: &DockPresentationConfig,
    ) -> Result<DockPresentationConfig, DockStyleError> {
        self.configure_presentation(current.to_builder())
    }

    fn configure_presentation(
        &self,
        builder: DockPresentationConfigBuilder,
    ) -> Result<DockPresentationConfig, DockStyleError> {
        self.validate_visuals()?;
        builder
            .hover_selection_delay(self.hover_selection_delay)
            .touch_long_press_delay(self.touch_long_press_delay)
            .touch_press_slop_distance(f64::from(self.touch_press_slop_distance))
            .touch_resize_hit_extent(f64::from(self.touch_resize_hit_extent))
            .tab_bar_height(f64::from(self.tab_bar_height))
            .tab_strip_edge_scroll_band(f64::from(self.tab_strip_edge_scroll_band))
            .tab_strip_edge_scroll_interval(self.tab_strip_edge_scroll_interval)
            .tab_group_grip_extent(
                if self.tab_group_grip_visibility == TabGroupGripVisibility::Hidden {
                    0.0
                } else {
                    f64::from(self.tab_group_grip_extent)
                },
            )
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
            .pane_overflow_policy(self.pane_overflow_policy)
            .minimum_floating_size(
                f64::from(self.minimum_floating_size.x),
                f64::from(self.minimum_floating_size.y),
            )
            .build()
            .map_err(DockStyleError::Presentation)
    }

    /// Validates every geometry metric and all cross-field invariants.
    ///
    /// Call this before publishing style metrics to the core scene. Validation
    /// is deterministic and does not normalize or silently repair values.
    ///
    /// # Errors
    ///
    /// Returns [`DockStyleError`] when a metric is not finite, violates its
    /// sign constraint, the touch resize hit extent is below 44 points,
    /// a scroll interval is zero, or metrics conflict.
    pub fn validate(&self) -> Result<(), DockStyleError> {
        self.presentation_config().map(|_| ())
    }

    fn validate_visuals(&self) -> Result<(), DockStyleError> {
        for (field, stroke) in [
            ("tab_stroke.width", self.visuals.tab_stroke),
            ("tab_active_stroke.width", self.visuals.tab_active_stroke),
            ("tab_focus_stroke.width", self.visuals.tab_focus_stroke),
            ("pane_focus_stroke.width", self.visuals.pane_focus_stroke),
        ] {
            if let Some(stroke) = stroke {
                require_non_negative(field, stroke.width)?;
            }
        }
        require_positive("tab_close_icon_size", self.tab_close_icon_size)?;
        require_non_negative("ghost_offset.x", self.ghost_offset.x)?;
        require_non_negative("ghost_offset.y", self.ghost_offset.y)?;
        if self.tab_group_grip_visibility == TabGroupGripVisibility::Hidden {
            require_non_negative("tab_group_grip_extent", self.tab_group_grip_extent)?;
        }
        Ok(())
    }
}

impl Default for DockStyle {
    fn default() -> Self {
        let geometry = DockPresentationConfig::default();
        Self {
            hover_selection_delay: geometry.hover_selection_delay(),
            touch_long_press_delay: geometry.touch_long_press_delay(),
            touch_press_slop_distance: geometry.touch_press_slop_distance() as f32,
            touch_resize_hit_extent: geometry.touch_resize_hit_extent() as f32,
            tab_bar_height: geometry.tab_bar_height() as f32,
            tab_corner_radius: CornerRadius::ZERO,
            tab_strip_edge_scroll_band: geometry.tab_strip_edge_scroll_band() as f32,
            tab_strip_edge_scroll_interval: geometry.tab_strip_edge_scroll_interval(),
            tab_group_grip_extent: geometry.tab_group_grip_extent() as f32,
            tab_group_grip_visibility: TabGroupGripVisibility::Always,
            tab_horizontal_padding: geometry.tab_horizontal_padding() as f32,
            tab_min_width: geometry.tab_min_width() as f32,
            tab_max_width: geometry.tab_max_width() as f32,
            tab_close_size: geometry.tab_close_extent() as f32,
            tab_close_icon_size: 8.0,
            tab_close_visibility: TabCloseVisibility::default(),
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
            pane_overflow_policy: geometry.pane_overflow_policy(),
            minimum_floating_size: Vec2::new(
                geometry.minimum_floating_size().width() as f32,
                geometry.minimum_floating_size().height() as f32,
            ),
            ghost_offset: Vec2::new(12.0, 12.0),
            visuals: DockVisualOverrides::default(),
        }
    }
}

/// Error returned when [`DockStyle`] geometry or interaction timing is invalid.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum DockStyleError {
    /// Core geometry or interaction timing is invalid.
    ///
    /// Field names use the renderer-neutral configuration's vocabulary, such as
    /// `tab_close_extent` for [`DockStyle::tab_close_size`]. The original error
    /// and rejected value remain available through the error's source chain.
    #[error("invalid docking geometry: {0}")]
    Presentation(#[from] DockPresentationConfigError),
    /// An adapter-only metric is NaN or infinite.
    #[error("`{field}` must be finite")]
    NonFinite {
        /// Name of the invalid style field.
        field: &'static str,
    },
    /// An adapter-only metric that may be zero is negative.
    #[error("`{field}` must not be negative")]
    Negative {
        /// Name of the invalid style field.
        field: &'static str,
    },
    /// An adapter-only size is zero or negative.
    #[error("`{field}` must be positive")]
    NotPositive {
        /// Name of the invalid style field.
        field: &'static str,
    },
}

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
        let style = DockStyle::default();
        assert_eq!(style.validate(), Ok(()));
        let config = style.presentation_config().unwrap();
        assert_eq!(config, DockPresentationConfig::default());
        assert_eq!(style.touch_resize_hit_extent.to_bits(), 44.0_f32.to_bits());
        assert_eq!(
            style.tab_strip_edge_scroll_band.to_bits(),
            24.0_f32.to_bits()
        );
        assert_eq!(
            style.tab_strip_edge_scroll_interval,
            Some(std::time::Duration::from_millis(50))
        );
    }

    #[test]
    fn overflow_policy_maps_through_creation_and_live_style_updates() {
        let style = DockStyle {
            pane_overflow_policy: PaneOverflowPolicy::Compress,
            ..Default::default()
        };
        let config = style.presentation_config().unwrap();
        assert_eq!(config.pane_overflow_policy(), PaneOverflowPolicy::Compress);
        assert_eq!(config.to_builder().build().unwrap(), config);
        let changed = config
            .to_builder()
            .pointer_drag_start_distance(11.0)
            .build()
            .unwrap();
        let mapped = style.presentation_config_with(&changed).unwrap();
        assert_eq!(mapped.pane_overflow_policy(), PaneOverflowPolicy::Compress);
        assert_eq!(mapped.pointer_drag_start_distance(), 11.0);
    }

    #[test]
    fn tab_outlines_are_visual_only_and_reject_invalid_widths() {
        let mut style = DockStyle {
            tab_corner_radius: CornerRadius::same(7),
            ..Default::default()
        };
        style.visuals.tab_stroke = Some(Stroke::new(1.0, Color32::RED));
        style.visuals.tab_active_stroke = Some(Stroke::new(2.0, Color32::BLUE));
        assert_eq!(style.validate(), Ok(()));
        assert_eq!(
            style.presentation_config().unwrap(),
            DockPresentationConfig::default()
        );
        for width in [-1.0, f32::NAN, f32::INFINITY] {
            for active in [false, true] {
                let mut candidate = style.clone();
                let stroke = if active {
                    &mut candidate.visuals.tab_active_stroke
                } else {
                    &mut candidate.visuals.tab_stroke
                };
                stroke.as_mut().unwrap().width = width;
                assert!(candidate.validate().is_err());
            }
        }
    }

    #[test]
    fn touch_resize_hit_extent_style_bridges_minimum_and_custom_values() {
        let baseline = DockStyle::default();
        for extent in [44.0, 64.5] {
            let style = DockStyle {
                touch_resize_hit_extent: extent,
                ..DockStyle::default()
            };
            assert_eq!(style.validate(), Ok(()));
            let config = style.presentation_config().unwrap();
            assert_eq!(
                config.touch_resize_hit_extent().to_bits(),
                f64::from(extent).to_bits()
            );
            assert_eq!(
                config.splitter_hit_extent().to_bits(),
                f64::from(baseline.splitter_hit_extent).to_bits()
            );
            assert_eq!(
                config.floating_resize_extent().to_bits(),
                f64::from(baseline.floating_resize_extent).to_bits()
            );
        }
    }

    #[test]
    fn touch_resize_hit_extent_style_rejects_non_finite_and_below_minimum_values() {
        for extent in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let style = DockStyle {
                touch_resize_hit_extent: extent,
                ..DockStyle::default()
            };
            assert!(matches!(
                style.validate(),
                Err(DockStyleError::Presentation(
                    DockPresentationConfigError::NonFinite {
                        field: "touch_resize_hit_extent",
                        ..
                    }
                ))
            ));
        }
        for extent in [-1.0, 0.0, 43.5] {
            let style = DockStyle {
                touch_resize_hit_extent: extent,
                ..DockStyle::default()
            };
            assert_eq!(
                style.validate(),
                Err(DockStyleError::Presentation(
                    DockPresentationConfigError::TouchResizeHitExtentTooSmall {
                        value: f64::from(extent),
                    }
                ))
            );
        }
    }

    #[test]
    fn tab_strip_edge_scroll_style_bridges_custom_and_disabled_values() {
        for (band, interval) in [
            (12.5, Some(std::time::Duration::from_millis(25))),
            (0.0, Some(std::time::Duration::from_nanos(1))),
            (24.0, None),
            (0.0, None),
        ] {
            let style = DockStyle {
                tab_strip_edge_scroll_band: band,
                tab_strip_edge_scroll_interval: interval,
                ..DockStyle::default()
            };
            assert_eq!(style.validate(), Ok(()));
            let config = style.presentation_config().unwrap();
            assert_eq!(
                config.tab_strip_edge_scroll_band().to_bits(),
                f64::from(band).to_bits()
            );
            assert_eq!(config.tab_strip_edge_scroll_interval(), interval);
        }
    }

    #[test]
    fn tab_strip_edge_scroll_style_rejects_zero_interval_and_invalid_band() {
        let style = DockStyle {
            tab_strip_edge_scroll_band: 0.0,
            tab_strip_edge_scroll_interval: Some(std::time::Duration::ZERO),
            ..DockStyle::default()
        };
        assert_eq!(
            style.validate(),
            Err(DockStyleError::Presentation(
                DockPresentationConfigError::ZeroEdgeScrollInterval
            ))
        );
        for band in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let style = DockStyle {
                tab_strip_edge_scroll_band: band,
                tab_strip_edge_scroll_interval: None,
                ..DockStyle::default()
            };
            assert!(matches!(
                style.validate(),
                Err(DockStyleError::Presentation(
                    DockPresentationConfigError::NonFinite {
                        field: "tab_strip_edge_scroll_band",
                        ..
                    }
                ))
            ));
        }
        let style = DockStyle {
            tab_strip_edge_scroll_band: -1.0,
            ..DockStyle::default()
        };
        assert_eq!(
            style.validate(),
            Err(DockStyleError::Presentation(
                DockPresentationConfigError::Negative {
                    field: "tab_strip_edge_scroll_band",
                    value: -1.0,
                }
            ))
        );
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
        assert_eq!(resolved.popup_fill, egui_visuals.window_fill());
        assert_eq!(
            resolved.drop_guide_active_fill,
            egui_visuals.selection.bg_fill
        );

        let light = DockStyle::default().resolved_visuals(&Visuals::light());
        assert_eq!(light.tab_bar_fill.a(), 255);
    }

    #[test]
    fn contained_window_inherits_egui_window_recipe() {
        let mut egui_style = EguiStyle::default();
        egui_style.spacing.window_margin = Margin::symmetric(7, 9);
        egui_style.visuals.window_fill = Color32::from_rgb(31, 41, 59);
        egui_style.visuals.window_stroke = egui::Stroke::new(2.0, Color32::from_rgb(148, 163, 184));
        egui_style.visuals.window_corner_radius = egui::CornerRadius::same(11);
        egui_style.visuals.window_shadow = egui::epaint::Shadow {
            offset: [2, 3],
            blur: 12,
            spread: 1,
            color: Color32::from_black_alpha(96),
        };
        let dock_style = DockStyle {
            floating_border_width: 2.0,
            ..DockStyle::default()
        };

        let inactive = dock_style.resolved_contained_window(&egui_style, false);
        let active = dock_style.resolved_contained_window(&egui_style, true);

        assert_eq!(inactive.frame.fill, egui_style.visuals.window_fill());
        assert_eq!(inactive.frame.stroke, egui_style.visuals.window_stroke());
        assert_eq!(
            inactive.frame.corner_radius,
            egui_style.visuals.window_corner_radius
        );
        assert_eq!(inactive.frame.shadow, egui_style.visuals.window_shadow);
        assert_eq!(inactive.frame.inner_margin, Margin::ZERO);
        assert_eq!(inactive.frame.outer_margin, Margin::ZERO);
        assert_eq!(inactive.title_margin, egui_style.spacing.window_margin);
        assert_eq!(inactive.title_fill, egui_style.visuals.window_fill());
        assert_eq!(
            active.title_fill,
            egui_style.visuals.widgets.open.weak_bg_fill
        );
    }

    #[test]
    fn local_window_and_popup_overrides_do_not_leak_into_each_other() {
        let theme = EguiStyle::default();
        let mut style = DockStyle::default();
        style.visuals.floating.fill = Some(Color32::RED);
        style.visuals.floating.corner_radius = Some(CornerRadius::same(9));
        style.visuals.popup.fill = Some(Color32::BLUE);
        style.visuals.popup.corner_radius = Some(CornerRadius::same(3));
        style.visuals.floating_title_fill = Some(Color32::YELLOW);
        style.visuals.floating_title_active_fill = Some(Color32::GREEN);
        style.visuals.floating_title_text_color = Some(Color32::BLACK);
        let inactive = style.resolved_contained_window(&theme, false);
        let active = style.resolved_contained_window(&theme, true);
        let popup = style.resolved_popup_frame(&theme);
        assert_eq!(inactive.frame.fill, Color32::RED);
        assert_eq!(inactive.frame.corner_radius, CornerRadius::same(9));
        assert_eq!(inactive.frame.shadow, theme.visuals.window_shadow);
        assert_eq!(inactive.title_fill, Color32::YELLOW);
        assert_eq!(active.title_fill, Color32::GREEN);
        assert_eq!(active.title_text_color, Color32::BLACK);
        assert_eq!(popup.fill, Color32::BLUE);
        assert_eq!(popup.corner_radius, CornerRadius::same(3));
        assert_eq!(popup.stroke, Frame::popup(&theme).stroke);
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
        assert!(matches!(
            style.validate(),
            Err(DockStyleError::Presentation(
                DockPresentationConfigError::NonFinite {
                    field: "splitter_thickness",
                    ..
                }
            ))
        ));

        style.splitter_thickness = 1.0;
        style.tab_horizontal_padding = -1.0;
        assert_eq!(
            style.validate(),
            Err(DockStyleError::Presentation(
                DockPresentationConfigError::Negative {
                    field: "tab_horizontal_padding",
                    value: -1.0,
                }
            ))
        );

        style.tab_horizontal_padding = 0.0;
        style.drop_guide_extent = 0.0;
        assert_eq!(
            style.validate(),
            Err(DockStyleError::Presentation(
                DockPresentationConfigError::NonPositive {
                    field: "guide_extent",
                    value: 0.0,
                }
            ))
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
            Err(DockStyleError::Presentation(
                DockPresentationConfigError::MinimumExceedsMaximum {
                    minimum: "tab_min_width",
                    maximum: "tab_max_width"
                }
            ))
        );

        style.tab_min_width = 72.0;
        style.splitter_hit_extent = 0.5;
        assert_eq!(
            style.validate(),
            Err(DockStyleError::Presentation(
                DockPresentationConfigError::HitExtentSmallerThanVisible {
                    hit_extent: "splitter_hit_extent",
                    visible_extent: "splitter_thickness"
                }
            ))
        );

        style.splitter_hit_extent = 6.0;
        style.dock_fraction = 1.0;
        assert_eq!(
            style.validate(),
            Err(DockStyleError::Presentation(
                DockPresentationConfigError::DockFractionOutOfRange { value: 1.0 }
            ))
        );

        style.dock_fraction = 0.5;
        style.drop_guide_gap = 3.0;
        assert_eq!(
            style.validate(),
            Err(DockStyleError::Presentation(
                DockPresentationConfigError::GuideHitRegionsOverlap
            ))
        );

        style.drop_guide_gap = 12.0;
        style.drop_guide_outer_inset = 9.0;
        assert_eq!(
            style.validate(),
            Err(DockStyleError::Presentation(
                DockPresentationConfigError::GuideOuterInsetTooSmall
            ))
        );

        style.drop_guide_outer_inset = 72.0;
        assert_eq!(style.validate(), Ok(()));
    }

    #[test]
    fn geometry_failures_preserve_the_core_error_and_its_field_names() {
        use std::error::Error;

        let style = DockStyle {
            tab_close_size: 40.0,
            ..DockStyle::default()
        };
        let error = style.presentation_config().unwrap_err();
        assert_eq!(
            error
                .source()
                .unwrap()
                .downcast_ref::<DockPresentationConfigError>(),
            Some(&DockPresentationConfigError::ExtentExceedsContainer {
                extent: "tab_close_extent",
                container: "tab_bar_height",
            })
        );
        assert_eq!(style.validate().unwrap_err(), error);
    }

    #[test]
    fn hidden_grip_retains_adapter_validation_before_mapping_to_zero() {
        let mut style = DockStyle {
            tab_group_grip_visibility: TabGroupGripVisibility::Hidden,
            tab_group_grip_extent: 100.0,
            ..DockStyle::default()
        };
        assert_eq!(
            style.presentation_config().unwrap().tab_group_grip_extent(),
            0.0
        );
        style.tab_group_grip_extent = -1.0;
        assert_eq!(
            style.presentation_config().unwrap_err(),
            DockStyleError::Negative {
                field: "tab_group_grip_extent"
            }
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

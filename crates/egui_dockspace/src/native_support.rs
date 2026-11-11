//! Private cross-crate rendering seam for the fork-backed native adapter.
//!
//! The native crate owns the `DockspaceSession`; this module only reuses the
//! ordinary product renderer over an externally owned host frame. Keeping this
//! seam here prevents the native crate from copying a second projection or
//! pane/tab renderer.

use std::collections::BTreeSet;

use dockspace::model::{ItemId, SurfaceId};
use dockspace::runtime::{
    DockPresentationConfig, DockspaceHostFrame, DockspaceReceiverDescriptor, DockspaceReceiverRole,
    DockspaceSemanticOutput, DockspaceVisualId, NativeSurfaceReceiverQuery,
    PreparedPaneFocusObservation, PresentedDockReceiver, PresentedDockspaceSurface,
    SurfaceProgramExecutionReceipt, SurfaceUnavailableReason,
};
use egui::{Id, Ui};

use crate::error::DockspaceError;
use crate::pane::PaneView;
use crate::product_render;
use crate::style::DockStyle;

pub use crate::product_focus::{retain_skipped_pane_focus, sample_host_focus};
pub use crate::product_render::EguiSurfaceAction;

/// Validates every surface's exact encoded spans after all renderer output hooks.
///
/// Surfaces must be supplied in their synchronous paint order. Private markers
/// are removed once, only after every proof and the complete layer order pass.
/// A rejected output is left unchanged so the caller can inspect or retry it.
pub fn validate_final_outputs<'a>(
    paints: impl IntoIterator<Item = &'a NativeSurfacePaint>,
    output: &mut egui::FullOutput,
) -> bool {
    let mut context = None;
    let mut spans = Vec::new();
    for (paint_order, paint) in paints.into_iter().enumerate() {
        if !paint.had_ready_plan {
            continue;
        }
        let Some(proof) = paint.output_proof.as_ref() else {
            return false;
        };
        match context {
            Some(previous) if previous != proof.context() => return false,
            None => context = Some(proof.context()),
            _ => {}
        }
        let Some(evidence) = proof.present_spans(output) else {
            return false;
        };
        spans.extend(
            evidence
                .spans
                .into_iter()
                .enumerate()
                .map(|(span_order, span)| (paint_order, span_order, span)),
        );
    }
    if context.is_some_and(|context| {
        !crate::output_proof::spans_follow_paint_order(
            &mut spans,
            &crate::output_proof::final_paint_layer_order(context),
        )
    }) {
        return false;
    }
    crate::output_proof::scrub_private_markers(output);
    true
}

/// Validates a native renderer style and diagnoses mismatched mapped settings.
///
/// Only fields represented by the style are compared. Core-only settings and
/// adapter-only visuals remain independently owned; neither input is changed.
/// The complete mapped configuration decides agreement. The field list only
/// names a mismatch; an unlisted mismatch uses `presentation_config`.
///
/// # Errors
///
/// Returns the original style or presentation configuration failure in the
/// adapter error's source chain.
#[doc(hidden)]
#[allow(
    clippy::float_cmp,
    clippy::too_many_lines,
    reason = "the field list diagnoses a mismatch after complete configuration comparison"
)]
pub fn style_presentation_mismatch(
    style: &DockStyle,
    accepted: &DockPresentationConfig,
) -> Result<Option<&'static str>, DockspaceError> {
    style.validate().map_err(DockspaceError::from_detail)?;
    let mapped = style
        .presentation_config_with(accepted)
        .map_err(DockspaceError::from_detail)?;
    if mapped == *accepted {
        return Ok(None);
    }
    let field = [
        (
            "hover_selection_delay",
            mapped.hover_selection_delay() != accepted.hover_selection_delay(),
        ),
        (
            "touch_long_press_delay",
            mapped.touch_long_press_delay() != accepted.touch_long_press_delay(),
        ),
        (
            "touch_press_slop_distance",
            mapped.touch_press_slop_distance() != accepted.touch_press_slop_distance(),
        ),
        (
            "touch_resize_hit_extent",
            mapped.touch_resize_hit_extent() != accepted.touch_resize_hit_extent(),
        ),
        (
            "tab_bar_height",
            mapped.tab_bar_height() != accepted.tab_bar_height(),
        ),
        (
            "tab_strip_edge_scroll_band",
            mapped.tab_strip_edge_scroll_band() != accepted.tab_strip_edge_scroll_band(),
        ),
        (
            "tab_strip_edge_scroll_interval",
            mapped.tab_strip_edge_scroll_interval() != accepted.tab_strip_edge_scroll_interval(),
        ),
        (
            "tab_group_grip_extent",
            mapped.tab_group_grip_extent() != accepted.tab_group_grip_extent(),
        ),
        (
            "tab_horizontal_padding",
            mapped.tab_horizontal_padding() != accepted.tab_horizontal_padding(),
        ),
        (
            "tab_min_width",
            mapped.tab_min_width() != accepted.tab_min_width(),
        ),
        (
            "tab_max_width",
            mapped.tab_max_width() != accepted.tab_max_width(),
        ),
        (
            "tab_close_size",
            mapped.tab_close_extent() != accepted.tab_close_extent(),
        ),
        (
            "splitter_thickness",
            mapped.splitter_thickness() != accepted.splitter_thickness(),
        ),
        (
            "splitter_hit_extent",
            mapped.splitter_hit_extent() != accepted.splitter_hit_extent(),
        ),
        (
            "splitter_keyboard_step",
            mapped.splitter_keyboard_step() != accepted.splitter_keyboard_step(),
        ),
        (
            "drop_guide_extent",
            mapped.guide_extent() != accepted.guide_extent(),
        ),
        ("drop_guide_gap", mapped.guide_gap() != accepted.guide_gap()),
        (
            "drop_guide_hit_padding",
            mapped.guide_hit_padding() != accepted.guide_hit_padding(),
        ),
        (
            "drop_guide_outer_inset",
            mapped.guide_outer_inset() != accepted.guide_outer_inset(),
        ),
        (
            "dock_fraction",
            mapped.dock_fraction() != accepted.dock_fraction(),
        ),
        (
            "floating_title_height",
            mapped.floating_title_height() != accepted.floating_title_height(),
        ),
        (
            "floating_border_width",
            mapped.floating_border_width() != accepted.floating_border_width(),
        ),
        (
            "floating_resize_extent",
            mapped.floating_resize_extent() != accepted.floating_resize_extent(),
        ),
        (
            "minimum_pane_size",
            mapped.minimum_pane_size() != accepted.minimum_pane_size(),
        ),
        (
            "minimum_floating_size",
            mapped.minimum_floating_size() != accepted.minimum_floating_size(),
        ),
    ]
    .into_iter()
    .find_map(|(field, mismatch)| mismatch.then_some(field))
    .unwrap_or("presentation_config");
    Ok(Some(field))
}

/// Result of painting one surface against a core-owned host frame.
#[derive(Debug)]
pub struct NativeSurfacePaint {
    surface: SurfaceId,
    viewport_id: egui::ViewportId,
    cumulative_pass_nr: u64,
    had_ready_plan: bool,
    deferred_measurement: bool,
    transient_visuals_complete: bool,
    missing_items: BTreeSet<ItemId>,
    receivers: Vec<NativePaintReceiver>,
    scroll_receivers: Vec<NativeScrollPaintReceiver>,
    semantic_output: Option<DockspaceSemanticOutput>,
    execution_receipt: Option<SurfaceProgramExecutionReceipt>,
    output_proof: Option<crate::output_proof::SurfaceOutputProof>,
    local_actions: Vec<EguiSurfaceAction>,
    pane_focus_observation: Option<PreparedPaneFocusObservation>,
    pane_focus_bindings: Vec<(ItemId, Id)>,
}

/// One egui widget identity bound to an exact core receiver in the same pass.
#[derive(Debug, Clone, Copy)]
pub struct NativePaintReceiver {
    viewport_id: egui::ViewportId,
    cumulative_pass_nr: u64,
    widget_id: egui::Id,
    layer_id: egui::LayerId,
    interact_rect: egui::Rect,
    receiver: DockspaceReceiverDescriptor,
}

/// One fork-owned scroll identity bound to an exact core receiver in the same pass.
#[derive(Debug, Clone, Copy)]
pub struct NativeScrollPaintReceiver {
    viewport_id: egui::ViewportId,
    cumulative_pass_nr: u64,
    widget_id: egui::Id,
    layer_id: egui::LayerId,
    receiver: DockspaceReceiverDescriptor,
}

impl NativePaintReceiver {
    /// Returns the viewport whose completed pass owns this binding.
    #[must_use]
    pub const fn viewport_id(self) -> egui::ViewportId {
        self.viewport_id
    }

    /// Returns the exact completed-pass generation owning this binding.
    #[must_use]
    pub const fn cumulative_pass_nr(self) -> u64 {
        self.cumulative_pass_nr
    }

    /// Returns the egui widget identity registered during painting.
    #[must_use]
    pub const fn widget_id(self) -> egui::Id {
        self.widget_id
    }

    /// Returns the exact egui layer containing the widget.
    #[must_use]
    pub const fn layer_id(self) -> egui::LayerId {
        self.layer_id
    }

    /// Returns the stable semantic receiver role.
    #[must_use]
    pub const fn role(self) -> DockspaceReceiverRole {
        self.receiver.role()
    }

    /// Returns the opaque visual identity of this exact receiver.
    #[must_use]
    pub const fn visual_id(self) -> DockspaceVisualId {
        self.receiver.visual_id()
    }

    /// Rebinds this paint-time identity to the exact output named by a core query.
    #[must_use]
    pub fn bind_for_query(
        self,
        query: NativeSurfaceReceiverQuery,
    ) -> Option<PresentedDockReceiver> {
        query.bind_receiver(&self.receiver)
    }

    /// Qualifies this known widget's exact output and interaction bounds for hover clearance.
    ///
    /// The native adapter must first match the unique completed-pass widget and layer.
    #[must_use]
    pub fn clear_hover_lane_for_query(
        self,
        query: NativeSurfaceReceiverQuery,
    ) -> Option<PresentedDockspaceSurface> {
        let point = query.point()?;
        let rect = self.interact_rect;
        if !rect.is_finite()
            || !rect.is_positive()
            || point.x() < f64::from(rect.min.x)
            || point.y() < f64::from(rect.min.y)
            || point.x() >= f64::from(rect.max.x)
            || point.y() >= f64::from(rect.max.y)
        {
            return None;
        }
        query.clear_hover_lane(&self.receiver)
    }
}

impl NativeScrollPaintReceiver {
    /// Returns the viewport whose completed pass owns this binding.
    #[must_use]
    pub const fn viewport_id(self) -> egui::ViewportId {
        self.viewport_id
    }

    /// Returns the exact completed-pass generation owning this binding.
    #[must_use]
    pub const fn cumulative_pass_nr(self) -> u64 {
        self.cumulative_pass_nr
    }

    /// Returns the fork-owned scroll identity.
    #[must_use]
    pub const fn widget_id(self) -> egui::Id {
        self.widget_id
    }

    /// Returns the exact egui layer containing the scroll route.
    #[must_use]
    pub const fn layer_id(self) -> egui::LayerId {
        self.layer_id
    }

    /// Returns the stable semantic receiver role.
    #[must_use]
    pub const fn role(self) -> DockspaceReceiverRole {
        self.receiver.role()
    }

    /// Rebinds this paint-time identity to the exact output named by a core query.
    #[must_use]
    pub fn bind_for_query(
        self,
        query: NativeSurfaceReceiverQuery,
    ) -> Option<PresentedDockReceiver> {
        query.bind_receiver(&self.receiver)
    }
}

impl NativeSurfacePaint {
    /// Returns exact focusable pane targets registered in this output's UI pass.
    pub fn pane_focus_bindings(&self) -> &[(ItemId, Id)] {
        &self.pane_focus_bindings
    }

    /// Validates the exact encoded span after all renderer output hooks.
    /// Private markers are removed only after a successful complete check.
    pub fn validate_final_output(&self, output: &mut egui::FullOutput) -> bool {
        validate_final_outputs(std::iter::once(self), output)
    }
    /// Returns the logical surface painted by this pass.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Returns the viewport whose current pass produced these bindings.
    #[must_use]
    pub const fn viewport_id(&self) -> egui::ViewportId {
        self.viewport_id
    }

    /// Returns the completed-pass generation expected after this pass ends.
    #[must_use]
    pub const fn cumulative_pass_nr(&self) -> u64 {
        self.cumulative_pass_nr
    }

    /// Returns whether a ready core plan was painted.
    #[must_use]
    pub const fn had_ready_plan(&self) -> bool {
        self.had_ready_plan
    }

    /// Returns whether the frame should retain the current contribution rather
    /// than publish a new measured contribution.
    #[must_use]
    pub const fn deferred_measurement(&self) -> bool {
        self.deferred_measurement
    }

    /// Returns whether every transient visual required by the ready plan was
    /// actually painted in this pass.
    #[must_use]
    pub const fn transient_visuals_complete(&self) -> bool {
        self.transient_visuals_complete
    }

    /// Returns pane identities which were missing from the application catalog.
    #[must_use]
    pub fn missing_items(&self) -> impl Iterator<Item = ItemId> + '_ {
        self.missing_items.iter().copied()
    }

    /// Returns every egui widget bound to one exact core receiver in paint order.
    pub fn receivers(&self) -> impl ExactSizeIterator<Item = NativePaintReceiver> + '_ {
        self.receivers.iter().copied()
    }

    /// Returns every fork-owned scroll identity bound in paint order.
    pub fn scroll_receivers(
        &self,
    ) -> impl ExactSizeIterator<Item = NativeScrollPaintReceiver> + '_ {
        self.scroll_receivers.iter().copied()
    }

    /// Returns the exact semantic output represented by this ready paint pass.
    #[must_use]
    pub const fn semantic_output(&self) -> Option<DockspaceSemanticOutput> {
        self.semantic_output
    }

    /// Confirms this exact completed paint against the host frame which issued
    /// its surface plan.
    ///
    /// Returns `false` when no complete ready program was painted or its affine
    /// completion capability was already consumed. The receipt itself never
    /// crosses the adapter seam.
    pub fn confirm_into(
        &mut self,
        frame: &mut DockspaceHostFrame<'_>,
    ) -> Result<bool, DockspaceError> {
        let Some(receipt) = self.execution_receipt.take() else {
            return Ok(false);
        };
        frame
            .confirm_surface_painted(receipt)
            .map_err(DockspaceError::from_detail)?;
        Ok(true)
    }

    /// Takes user actions which survive an egui discarded pass.
    pub fn take_local_actions(&mut self) -> Vec<EguiSurfaceAction> {
        std::mem::take(&mut self.local_actions)
    }

    /// Takes the pane-focus observation owned only by this exact pass.
    pub fn take_pane_focus_observation(&mut self) -> Option<PreparedPaneFocusObservation> {
        self.pane_focus_observation.take()
    }
}

/// Paints one ready surface using the same renderer as the ordinary product
/// facade. Semantic actions retain their event origins across egui discarded
/// passes. Paint completion, receiver declarations, and pane-focus observations
/// remain owned by the exact final pass.
pub fn paint_surface(
    frame: &mut DockspaceHostFrame<'_>,
    instance_id: Id,
    surface: SurfaceId,
    ui: &mut Ui,
    panes: &mut dyn PaneView,
    style: &DockStyle,
    scroll_registrar: &mut dyn FnMut(&Ui, egui::Rect, Id) -> (Id, egui::LayerId),
) -> Result<NativeSurfacePaint, DockspaceError> {
    let viewport_id = ui.ctx().viewport_id();
    let cumulative_pass_nr = ui
        .ctx()
        .cumulative_pass_nr_for(viewport_id)
        .saturating_add(1);
    let dock_rect = ui.available_rect_before_wrap();
    let Some(plan) = frame
        .paint_plan(surface)
        .map_err(DockspaceError::from_detail)?
    else {
        ui.allocate_rect(dock_rect, egui::Sense::hover());
        let visuals = style.resolved_visuals(ui.visuals());
        ui.painter()
            .rect_filled(dock_rect, 0.0, visuals.workspace_fill);
        return Ok(NativeSurfacePaint {
            surface,
            viewport_id,
            cumulative_pass_nr,
            had_ready_plan: false,
            deferred_measurement: false,
            transient_visuals_complete: true,
            missing_items: BTreeSet::new(),
            receivers: Vec::new(),
            scroll_receivers: Vec::new(),
            semantic_output: None,
            execution_receipt: None,
            output_proof: None,
            local_actions: Vec::new(),
            pane_focus_observation: None,
            pane_focus_bindings: Vec::new(),
        });
    };

    let semantic_output = plan.semantic_output();
    product_render::prepare_menu_layer(ui, instance_id, surface);
    let capture = crate::output_proof::SurfaceOutputCapture::begin_multilayer(ui.painter());
    let painted = product_render::paint_surface(
        ui,
        instance_id,
        plan,
        panes,
        style,
        product_render::PaintResources::default(),
        product_render::PointerActionAuthority::ExternalJournal,
        Some(scroll_registrar),
    )
    .map_err(DockspaceError::from_detail)?;
    Ok(NativeSurfacePaint {
        surface,
        viewport_id,
        cumulative_pass_nr,
        had_ready_plan: true,
        deferred_measurement: painted.defer_measurement,
        transient_visuals_complete: painted.transient_visuals_complete,
        missing_items: painted.missing_items,
        receivers: painted
            .receivers
            .into_iter()
            .map(|binding| NativePaintReceiver {
                viewport_id,
                cumulative_pass_nr,
                widget_id: binding.widget_id,
                layer_id: binding.layer_id,
                interact_rect: binding.interact_rect,
                receiver: binding.receiver,
            })
            .collect(),
        scroll_receivers: painted
            .scroll_receivers
            .into_iter()
            .map(|binding| NativeScrollPaintReceiver {
                viewport_id,
                cumulative_pass_nr,
                widget_id: binding.widget_id,
                layer_id: binding.layer_id,
                receiver: binding.receiver,
            })
            .collect(),
        semantic_output: Some(semantic_output),
        execution_receipt: painted.execution_receipt,
        output_proof: capture.finish(ui.painter()),
        local_actions: painted.local_actions,
        pane_focus_observation: painted.pane_focus_observation,
        pane_focus_bindings: painted.pane_focus_bindings,
    })
}

/// Supplies the egui measurements required to compile the next core surface
/// contribution. This is intentionally separate from painting: a native host
/// can paint a previously accepted plan and acknowledge its output while a
/// later frame measures the next plan.
pub fn measure_surface(
    frame: &mut DockspaceHostFrame<'_>,
    surface: SurfaceId,
    ui: &Ui,
    dock_rect: egui::Rect,
    popup_rect: egui::Rect,
    panes: &dyn PaneView,
    style: &DockStyle,
) -> Result<BTreeSet<ItemId>, DockspaceError> {
    product_render::measure_surface(
        frame, surface, ui, dock_rect, popup_rect, panes, style, None,
    )
}

/// Explicitly settles every surface not painted by the native host callback.
pub fn defer_unpainted_surfaces(frame: &mut DockspaceHostFrame<'_>) -> Result<(), DockspaceError> {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .map_err(DockspaceError::from_detail)
}

#[cfg(test)]
mod output_tests;

#[cfg(test)]
mod style_tests {
    use std::error::Error as _;
    use std::time::Duration;

    use super::{DockPresentationConfig, DockStyle, style_presentation_mismatch};
    use crate::error::{DockspaceErrorKind, DockspaceErrorSource};
    use crate::style::DockStyleError;

    #[test]
    fn complete_style_comparison_preserves_core_only_settings_and_visual_overrides() {
        let accepted = DockPresentationConfig::builder()
            .pointer_drag_start_distance(12.25)
            .magnetic_center_fraction(0.6)
            .target_release_hysteresis(8.0)
            .build()
            .unwrap();
        let mut style = DockStyle {
            ghost_offset: egui::vec2(32.0, 48.0),
            ..DockStyle::default()
        };
        style.visuals.tab_fill = Some(egui::Color32::RED);
        assert_eq!(
            style_presentation_mismatch(&style, &accepted).unwrap(),
            None
        );
        assert_eq!(style.presentation_config_with(&accepted).unwrap(), accepted);
    }

    #[test]
    fn mapped_geometry_and_timing_mismatches_still_report_their_field() {
        let accepted = DockPresentationConfig::default();
        for (field, style) in [
            (
                "tab_bar_height",
                DockStyle {
                    tab_bar_height: 44.0,
                    ..DockStyle::default()
                },
            ),
            (
                "touch_long_press_delay",
                DockStyle {
                    touch_long_press_delay: Duration::from_millis(500),
                    ..DockStyle::default()
                },
            ),
        ] {
            assert_eq!(
                style_presentation_mismatch(&style, &accepted).unwrap(),
                Some(field)
            );
        }
    }

    #[test]
    fn adapter_only_style_validation_precedes_equal_mapped_configuration() {
        let accepted = DockPresentationConfig::default();
        let style = DockStyle {
            ghost_offset: egui::vec2(f32::NAN, 0.0),
            ..DockStyle::default()
        };
        assert_eq!(style.presentation_config_with(&accepted).unwrap(), accepted);
        let error = style_presentation_mismatch(&style, &accepted).unwrap_err();
        assert_eq!(error.kind(), DockspaceErrorKind::InvalidConfiguration);
        assert!(matches!(
            error
                .source()
                .unwrap()
                .downcast_ref::<DockspaceErrorSource>(),
            Some(DockspaceErrorSource::Style(DockStyleError::NonFinite {
                field: "ghost_offset.x"
            }))
        ));
    }
}

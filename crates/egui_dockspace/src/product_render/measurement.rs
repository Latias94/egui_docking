//! Egui-owned measurement facts for one renderer-neutral host frame.

use std::cell::OnceCell;
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use dockspace::geometry::LogicalSize;
use dockspace::model::{ItemId, SurfaceId};
use dockspace::policy::TabBarVisibility;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceRuntimeError, DockspaceVisualId, SurfaceMeasurementAnswer,
    SurfaceMeasurementRequest, SurfaceUnavailableReason, TabListMenuMetrics, TabStripControlMetric,
    TabStripControlMetrics, TabStripControlPlacement, TabStripMetrics,
};
use egui::{FontSelection, Galley, TextStyle, TextWrapMode, Ui};

use crate::error::DockspaceError;
use crate::error_detail::DockspaceErrorSource;
use crate::pane::PaneView;
use crate::style::DockStyle;

use super::geometry::logical_rect;

#[derive(Clone)]
pub(crate) struct TabPaintResource {
    pub(crate) title: String,
    pub(crate) galley: Arc<Galley>,
    pub(crate) missing: bool,
}

#[derive(Default)]
pub(crate) struct PaintResources {
    tab_visuals: OnceCell<HashMap<ItemId, DockspaceVisualId>>,
    items: HashMap<ItemId, TabPaintResource>,
    missing: BTreeSet<ItemId>,
}

impl PaintResources {
    pub(crate) fn prepare_for_plan(
        &mut self,
        plan: dockspace::runtime::SurfacePaintPlan<'_>,
        ui: &Ui,
        panes: &dyn PaneView,
        style: &DockStyle,
    ) -> Result<(), DockspaceRuntimeError> {
        plan.visit_tab_resources(|_, item| {
            self.get_or_shape(item, ui, panes, style);
        })?;
        Ok(())
    }

    fn get_or_shape(
        &mut self,
        item: ItemId,
        ui: &Ui,
        panes: &dyn PaneView,
        style: &DockStyle,
    ) -> &TabPaintResource {
        self.items.entry(item).or_insert_with(|| {
            let resource = shape_tab(ui, panes, style, item);
            if resource.missing {
                self.missing.insert(item);
            }
            resource
        })
    }

    pub(crate) fn tab_visual(
        &self,
        plan: dockspace::runtime::SurfacePaintPlan<'_>,
        item: ItemId,
    ) -> Option<DockspaceVisualId> {
        if self.tab_visuals.get().is_none() {
            // Ordinary paint does not query this map. Navigation and diagnostics
            // materialize it once from this call's exact program, in program order.
            let mut visuals = HashMap::new();
            plan.visit_tab_resources(|visual, item| {
                if visual.kind() == dockspace::runtime::DockspaceVisualKind::Tab {
                    visuals.entry(item).or_insert(visual);
                }
            })
            .ok()?;
            let _ = self.tab_visuals.set(visuals);
        }
        self.tab_visuals.get()?.get(&item).copied()
    }

    pub(crate) fn missing_items(&self) -> impl Iterator<Item = ItemId> + '_ {
        self.missing.iter().copied()
    }

    pub(crate) fn item(&self, item: ItemId) -> Option<&TabPaintResource> {
        self.items.get(&item)
    }
}

pub(crate) fn measure_surface(
    frame: &mut DockspaceHostFrame<'_>,
    surface: SurfaceId,
    ui: &Ui,
    dock_rect: egui::Rect,
    popup_rect: egui::Rect,
    panes: &dyn PaneView,
    style: &DockStyle,
    mut resources: Option<&mut PaintResources>,
) -> Result<BTreeSet<ItemId>, DockspaceError> {
    let dock_bounds =
        logical_rect(dock_rect).map_err(|()| DockspaceErrorSource::InvalidMeasurement {
            what: "dock bounds",
        })?;
    let popup_bounds =
        logical_rect(popup_rect).map_err(|()| DockspaceErrorSource::InvalidMeasurement {
            what: "popup plane bounds",
        })?;
    let popup_style = ui.ctx().global_style();
    let popup_spacing = popup_style.spacing.clone();
    let popup_stroke = popup_style.visuals.window_stroke.width.max(0.0);
    let horizontal_padding = f32::from(
        i8::max(
            popup_spacing.menu_margin.left,
            popup_spacing.menu_margin.right,
        )
        .max(0),
    ) + popup_stroke;
    let vertical_padding = f32::from(
        i8::max(
            popup_spacing.menu_margin.top,
            popup_spacing.menu_margin.bottom,
        )
        .max(0),
    ) + popup_stroke;
    let mut row_heights = HashMap::<DockspaceVisualId, f64>::new();
    let mut missing = BTreeSet::new();
    let mut adapter_error = None;

    frame
        .measure_surface_with(surface, |request| {
            let answer = resolve_measurement(
                request,
                ui,
                panes,
                style,
                dock_bounds,
                popup_bounds,
                &mut row_heights,
                resources.as_deref_mut(),
                &mut missing,
                horizontal_padding,
                vertical_padding,
                popup_spacing.item_spacing.y.max(0.0),
                popup_spacing.scroll.allocated_width().max(0.0),
            );
            match answer {
                Ok(answer) => answer,
                Err(error) => {
                    adapter_error.get_or_insert(error);
                    SurfaceMeasurementAnswer::Unavailable(
                        SurfaceUnavailableReason::ContentUnavailable,
                    )
                }
            }
        })
        .map_err(DockspaceError::from_detail)?;

    if let Some(error) = adapter_error {
        return Err(error);
    }
    Ok(missing)
}

#[allow(clippy::too_many_arguments)]
fn resolve_measurement(
    request: SurfaceMeasurementRequest,
    ui: &Ui,
    panes: &dyn PaneView,
    style: &DockStyle,
    dock_bounds: dockspace::geometry::LogicalRect,
    popup_bounds: dockspace::geometry::LogicalRect,
    row_heights: &mut HashMap<DockspaceVisualId, f64>,
    resources: Option<&mut PaintResources>,
    missing: &mut BTreeSet<ItemId>,
    horizontal_padding: f32,
    vertical_padding: f32,
    row_spacing: f32,
    scrollbar_extent: f32,
) -> Result<SurfaceMeasurementAnswer, DockspaceError> {
    match request {
        SurfaceMeasurementRequest::DockBounds { .. } => {
            Ok(SurfaceMeasurementAnswer::Bounds(dock_bounds))
        }
        SurfaceMeasurementRequest::PopupPlaneBounds { .. } => {
            Ok(SurfaceMeasurementAnswer::Bounds(popup_bounds))
        }
        SurfaceMeasurementRequest::PaneMinimum { item, .. } => {
            let minimum = item.map_or(egui::Vec2::ZERO, |item| panes.minimum_size(item));
            let minimum =
                LogicalSize::new(f64::from(minimum.x), f64::from(minimum.y)).map_err(|_| {
                    DockspaceErrorSource::InvalidMeasurement {
                        what: "pane minimum",
                    }
                })?;
            Ok(SurfaceMeasurementAnswer::PaneMinimum(minimum))
        }
        SurfaceMeasurementRequest::TabIntrinsic {
            visual: _,
            bar,
            item,
        } => {
            let uncached;
            let resource = if let Some(resources) = resources {
                resources.get_or_shape(item, ui, panes, style)
            } else {
                // Independent native measurement has no same-pass paint owner.
                uncached = shape_tab(ui, panes, style, item);
                &uncached
            };
            if resource.missing {
                missing.insert(item);
            }
            let row_height = f64::from(
                ui.spacing()
                    .interact_size
                    .y
                    .max(resource.galley.size().y + style.tab_horizontal_padding),
            );
            row_heights
                .entry(bar)
                .and_modify(|current| *current = current.max(row_height))
                .or_insert(row_height);
            Ok(SurfaceMeasurementAnswer::TabIntrinsic(f64::from(
                resource.galley.size().x,
            )))
        }
        SurfaceMeasurementRequest::TabStrip { visual, visibility } => {
            let mut metrics = TabStripMetrics::new(0.0, 0.0)
                .map_err(|_| DockspaceErrorSource::InvalidMeasurement { what: "tab strip" })?;
            if visibility == TabBarVisibility::Visible {
                let extent = f64::from(style.tab_bar_height);
                let controls = TabStripControlMetrics::new(0.0)
                    .and_then(|controls| {
                        Ok(controls
                            .with_scroll_backward(TabStripControlMetric::new(
                                extent,
                                TabStripControlPlacement::OverlayLeading,
                            )?)
                            .with_scroll_forward(TabStripControlMetric::new(
                                extent,
                                TabStripControlPlacement::OverlayTrailing,
                            )?)
                            .with_tab_list_menu(TabStripControlMetric::new(
                                extent,
                                TabStripControlPlacement::ReservedTrailing,
                            )?))
                    })
                    .map_err(|_| DockspaceErrorSource::InvalidMeasurement {
                        what: "tab strip controls",
                    })?;
                let menu = TabListMenuMetrics::new(
                    row_heights.get(&visual).copied().unwrap_or_default(),
                    f64::from(horizontal_padding),
                    f64::from(vertical_padding),
                    f64::from(row_spacing),
                    popup_bounds.height(),
                    f64::from(scrollbar_extent),
                )
                .map_err(|_| DockspaceErrorSource::InvalidMeasurement {
                    what: "tab list menu",
                })?;
                metrics = metrics.with_controls(controls).with_tab_list_menu(menu);
            }
            Ok(SurfaceMeasurementAnswer::TabStrip(metrics))
        }
    }
}

fn shape_tab(ui: &Ui, panes: &dyn PaneView, style: &DockStyle, item: ItemId) -> TabPaintResource {
    let title = panes.title(item);
    let missing = title.is_none();
    let widget_text = title.unwrap_or_else(|| format!("Missing pane {}", item.get()).into());
    let title = widget_text.text().to_owned();
    let galley = widget_text.into_galley(
        ui,
        Some(TextWrapMode::Truncate),
        style.tab_max_width,
        FontSelection::Style(TextStyle::Button),
    );
    TabPaintResource {
        title,
        galley,
        missing,
    }
}

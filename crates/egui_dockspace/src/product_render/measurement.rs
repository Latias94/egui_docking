//! Egui-owned measurement facts for one renderer-neutral host frame.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use dockspace::geometry::LogicalSize;
use dockspace::model::{ItemId, SurfaceId};
use dockspace::policy::TabBarVisibility;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceVisualId, SurfaceMeasurementAnswer, SurfaceMeasurementRequest,
    SurfaceUnavailableReason, TabListMenuMetrics, TabStripControlMetric, TabStripControlMetrics,
    TabStripControlPlacement, TabStripMetrics,
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
    tabs: BTreeMap<DockspaceVisualId, TabPaintResource>,
    items: BTreeMap<ItemId, TabPaintResource>,
    missing: BTreeSet<ItemId>,
}

impl PaintResources {
    pub(crate) fn from_plan(
        plan: dockspace::runtime::SurfacePaintPlan<'_>,
        ui: &Ui,
        panes: &dyn PaneView,
        style: &DockStyle,
    ) -> Self {
        let mut resources = Self::default();
        for tab in plan.tabs() {
            resources.insert_tab(tab.visual_id(), tab.item(), ui, panes, style);
        }
        for row in plan.tab_list_menus().flat_map(|menu| menu.rows()) {
            resources.insert_tab(row.tab_visual_id(), row.item(), ui, panes, style);
        }
        if let Some(decoration) = plan.drag_decoration()
            && let Some(item) = decoration.ghost_item()
        {
            resources.insert_tab(decoration.source_visual(), item, ui, panes, style);
        }
        resources
    }

    fn insert_tab(
        &mut self,
        visual: DockspaceVisualId,
        item: ItemId,
        ui: &Ui,
        panes: &dyn PaneView,
        style: &DockStyle,
    ) {
        let resource = self.items.get(&item).cloned().unwrap_or_else(|| {
            let resource = shape_tab(ui, panes, style, item);
            if resource.missing {
                self.missing.insert(item);
            }
            self.items.insert(item, resource.clone());
            resource
        });
        self.tabs.insert(visual, resource);
    }

    pub(crate) fn tab(&self, visual: DockspaceVisualId) -> Option<&TabPaintResource> {
        self.tabs.get(&visual)
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
    let mut row_heights = BTreeMap::<DockspaceVisualId, f64>::new();
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
    row_heights: &mut BTreeMap<DockspaceVisualId, f64>,
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
            let resource = shape_tab(ui, panes, style, item);
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

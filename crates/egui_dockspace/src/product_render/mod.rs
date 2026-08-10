//! Default single-surface renderer over the headless product session.

mod actions;
mod contained;
mod geometry;
mod measurement;
mod splitters;
mod tabs;

use std::collections::BTreeSet;

use dockspace::model::ItemId;
use dockspace::runtime::{DockspacePreviewVisual, PreparedSurfaceAction, SurfacePaintPlan};
use egui::{Id, Sense, Stroke, StrokeKind, Ui};

use crate::pane::PaneView;
use crate::style::DockStyle;

use measurement::PaintResources;
pub(crate) use measurement::measure_surface;

struct RenderContext<'ui, 'plan> {
    ui: &'ui mut Ui,
    instance_id: Id,
    plan: SurfacePaintPlan<'plan>,
    panes: &'ui mut dyn PaneView,
    style: &'ui DockStyle,
    resources: &'ui PaintResources,
    actions: &'ui mut Vec<PreparedSurfaceAction>,
}

pub(crate) struct ProductPaintOutput {
    pub(crate) actions: Vec<PreparedSurfaceAction>,
    pub(crate) missing_items: BTreeSet<ItemId>,
}

pub(crate) fn paint_surface(
    ui: &mut Ui,
    instance_id: Id,
    plan: SurfacePaintPlan<'_>,
    panes: &mut dyn PaneView,
    style: &DockStyle,
) -> ProductPaintOutput {
    let resources = PaintResources::from_plan(plan, ui, panes, style);
    let mut actions = Vec::new();
    if let Some(bounds) = geometry::egui_rect(plan.bounds()) {
        ui.allocate_rect(bounds, Sense::hover());
        ui.painter().rect_filled(bounds, 0.0, style.workspace_fill);
    }

    let mut contained = plan.contained().collect::<Vec<_>>();
    contained.sort_by_key(|record| record.ordinal());
    let contained_roots = contained
        .iter()
        .map(|record| record.root())
        .collect::<BTreeSet<_>>();
    let main_roots = plan
        .panes()
        .map(dockspace::runtime::PanePaintRecord::root)
        .filter(|root| !contained_roots.contains(root))
        .collect::<BTreeSet<_>>();

    {
        let mut context = RenderContext {
            ui,
            instance_id,
            plan,
            panes,
            style,
            resources: &resources,
            actions: &mut actions,
        };
        for root in main_roots {
            tabs::paint_root(&mut context, root);
            splitters::paint_root(&mut context, root);
        }

        for record in contained.iter().copied() {
            contained::paint_background(&mut context, record);
            tabs::paint_root(&mut context, record.root());
            splitters::paint_root(&mut context, record.root());
            contained::paint_controls(&mut context, record);
        }

        paint_preview(context.ui, context.plan, context.style);
    }
    ProductPaintOutput {
        actions,
        missing_items: resources.missing_items().collect(),
    }
}

fn paint_preview(ui: &Ui, plan: SurfacePaintPlan<'_>, style: &DockStyle) {
    let Some(preview) = plan.drag_preview() else {
        return;
    };
    let rect = match preview.visual() {
        DockspacePreviewVisual::Dock { rect, .. }
        | DockspacePreviewVisual::Contained { rect, .. } => rect,
        DockspacePreviewVisual::Native { .. } => return,
    };
    let Some(rect) = geometry::egui_rect(rect) else {
        return;
    };
    ui.painter().rect_filled(rect, 2.0, style.drop_fill);
    ui.painter().rect_stroke(
        rect,
        2.0,
        Stroke::new(1.0, style.drop_border_color),
        StrokeKind::Inside,
    );
}

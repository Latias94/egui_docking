//! Default single-surface renderer over the headless product session.

mod actions;
mod contained;
mod geometry;
mod guides;
mod measurement;
mod splitters;
mod tab_chrome;
mod tabs;

use std::collections::BTreeSet;

use dockspace::model::ItemId;
use dockspace::runtime::{
    DockspacePreviewVisual, DockspaceReceiverDescriptor, PreparedSurfaceAction, SurfacePaintPlan,
};
use egui::{Id, Key, Modifiers, Sense, Stroke, StrokeKind, Ui};

use crate::pane::PaneView;
use crate::style::DockStyle;

use measurement::PaintResources;
pub(crate) use measurement::measure_surface;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PointerActionAuthority {
    LocalResponses,
    #[cfg(feature = "native-render-support")]
    ExternalJournal,
}

impl PointerActionAuthority {
    const fn accepts_local_pointer_actions(self) -> bool {
        matches!(self, Self::LocalResponses)
    }

    const fn acknowledges_previews_locally(self) -> bool {
        matches!(self, Self::LocalResponses)
    }
}

struct RenderContext<'ui, 'plan> {
    ui: &'ui mut Ui,
    instance_id: Id,
    plan: SurfacePaintPlan<'plan>,
    panes: &'ui mut dyn PaneView,
    style: &'ui DockStyle,
    resources: &'ui PaintResources,
    local_actions: &'ui mut Vec<PreparedSurfaceAction>,
    presentation_actions: &'ui mut Vec<PreparedSurfaceAction>,
    #[cfg(feature = "native-render-support")]
    receivers: &'ui mut Vec<ProductReceiverBinding>,
    defer_measurement: &'ui mut bool,
    pointer_authority: PointerActionAuthority,
}

impl RenderContext<'_, '_> {
    fn push_preview_gesture_action(&mut self, action: PreparedSurfaceAction) {
        *self.defer_measurement = true;
        self.local_actions.push(action);
    }

    fn push_local_action(&mut self, action: PreparedSurfaceAction) {
        self.local_actions.push(action);
    }

    fn push_presentation_action(&mut self, action: PreparedSurfaceAction) {
        self.presentation_actions.push(action);
    }

    fn interact_receiver(
        &mut self,
        rect: egui::Rect,
        id: Id,
        sense: Sense,
        _receiver: Option<DockspaceReceiverDescriptor>,
    ) -> egui::Response {
        let response = self.ui.interact(rect, id, sense);
        #[cfg(feature = "native-render-support")]
        if let Some(receiver) = _receiver {
            self.receivers.push(ProductReceiverBinding {
                widget_id: response.id,
                layer_id: response.layer_id,
                receiver,
            });
        }
        response
    }
}

#[cfg(feature = "native-render-support")]
#[derive(Debug, Clone, Copy)]
pub(crate) struct ProductReceiverBinding {
    pub(crate) widget_id: Id,
    pub(crate) layer_id: egui::LayerId,
    pub(crate) receiver: DockspaceReceiverDescriptor,
}

pub(crate) struct ProductPaintOutput {
    pub(crate) local_actions: Vec<PreparedSurfaceAction>,
    pub(crate) presentation_actions: Vec<PreparedSurfaceAction>,
    pub(crate) missing_items: BTreeSet<ItemId>,
    #[cfg(feature = "native-render-support")]
    pub(crate) receivers: Vec<ProductReceiverBinding>,
    pub(crate) defer_measurement: bool,
    #[cfg(feature = "native-render-support")]
    pub(crate) transient_visuals_complete: bool,
}

pub(crate) fn paint_surface(
    ui: &mut Ui,
    instance_id: Id,
    plan: SurfacePaintPlan<'_>,
    panes: &mut dyn PaneView,
    style: &DockStyle,
    pointer_authority: PointerActionAuthority,
) -> ProductPaintOutput {
    let resources = PaintResources::from_plan(plan, ui, panes, style);
    let mut local_actions = Vec::new();
    let mut presentation_actions = Vec::new();
    #[cfg(feature = "native-render-support")]
    let mut receivers = Vec::new();
    let mut defer_measurement = false;
    let mut transient_visuals_complete = true;
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
            local_actions: &mut local_actions,
            presentation_actions: &mut presentation_actions,
            #[cfg(feature = "native-render-support")]
            receivers: &mut receivers,
            defer_measurement: &mut defer_measurement,
            pointer_authority,
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

        tab_chrome::paint(&mut context);

        let drag_preview_required = context.plan.drag_preview().is_some();
        let drag_preview_painted = paint_preview(context.ui, context.plan, context.style);
        transient_visuals_complete &= !drag_preview_required || drag_preview_painted;
        if context.pointer_authority.acknowledges_previews_locally()
            && drag_preview_painted
            && let Some(action) = context.plan.prepare_drag_preview_painted()
        {
            context.push_presentation_action(action);
            *context.defer_measurement = true;
        }
        guides::paint(&mut context);
        let contained_preview_required = context.plan.contained_transform_preview().is_some();
        let contained_preview_painted =
            paint_contained_transform_preview(context.ui, context.plan, context.style);
        transient_visuals_complete &= !contained_preview_required || contained_preview_painted;
        if context.pointer_authority.acknowledges_previews_locally()
            && contained_preview_painted
            && let Some(action) = context.plan.prepare_contained_transform_preview_painted()
        {
            context.push_presentation_action(action);
        }
        if context.pointer_authority.accepts_local_pointer_actions()
            && let Some(action) = context.plan.prepare_escape_cancel()
            && context
                .ui
                .input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape))
        {
            context.push_local_action(action);
        }
    }
    #[cfg(not(feature = "native-render-support"))]
    let _ = transient_visuals_complete;
    ProductPaintOutput {
        local_actions,
        presentation_actions,
        missing_items: resources.missing_items().collect(),
        #[cfg(feature = "native-render-support")]
        receivers,
        defer_measurement,
        #[cfg(feature = "native-render-support")]
        transient_visuals_complete,
    }
}

fn paint_preview(ui: &Ui, plan: SurfacePaintPlan<'_>, style: &DockStyle) -> bool {
    let Some(preview) = plan.drag_preview() else {
        return false;
    };
    let Some(bounds) = geometry::egui_rect(plan.bounds()) else {
        return false;
    };
    let Some(rect) = preview_rect(plan.surface(), bounds, preview.visual()) else {
        return false;
    };
    let painter = ui.painter_at(bounds);
    painter.rect_filled(rect, 2.0, style.drop_fill);
    painter.rect_stroke(
        rect,
        2.0,
        Stroke::new(1.0, style.drop_border_color),
        StrokeKind::Inside,
    );
    true
}

fn preview_rect(
    host_surface: dockspace::model::SurfaceId,
    host_bounds: egui::Rect,
    visual: DockspacePreviewVisual,
) -> Option<egui::Rect> {
    match visual {
        DockspacePreviewVisual::Dock { surface, rect }
        | DockspacePreviewVisual::Contained { surface, rect, .. }
            if surface == host_surface =>
        {
            geometry::egui_rect(rect)
        }
        DockspacePreviewVisual::Native {
            host_surface: surface,
            ..
        } if surface == host_surface => Some(host_bounds.shrink(4.0)),
        DockspacePreviewVisual::Dock { .. }
        | DockspacePreviewVisual::Contained { .. }
        | DockspacePreviewVisual::Native { .. } => None,
    }
}

fn paint_contained_transform_preview(
    ui: &Ui,
    plan: SurfacePaintPlan<'_>,
    style: &DockStyle,
) -> bool {
    let Some(preview) = plan.contained_transform_preview() else {
        return false;
    };
    if preview.surface() != plan.surface() {
        return false;
    }
    let Some(rect) = geometry::egui_rect(preview.rect()) else {
        return false;
    };
    ui.painter().rect(
        rect,
        0.0,
        style.ghost_fill,
        Stroke::new(1.0, style.ghost_border_color),
        StrokeKind::Inside,
    );
    true
}

#[cfg(all(test, feature = "native-render-support"))]
mod tests {
    use dockspace::geometry::{LogicalRect, PhysicalRect};
    use dockspace::model::SurfaceId;
    use egui::{Rect, pos2, vec2};

    use super::{DockspacePreviewVisual, PointerActionAuthority, preview_rect};

    #[test]
    fn external_journal_uses_renderer_settlement_for_preview_authority() {
        assert!(
            PointerActionAuthority::LocalResponses.acknowledges_previews_locally(),
            "the official-egui product path has no renderer settlement callback"
        );
        assert!(
            !PointerActionAuthority::ExternalJournal.acknowledges_previews_locally(),
            "native preview authority must come from its Presented output"
        );
    }

    #[test]
    fn native_preview_paints_a_source_hosted_cue() {
        let source = SurfaceId::new(1);
        let target = SurfaceId::new(2);
        let bounds = Rect::from_min_size(pos2(10.0, 20.0), vec2(300.0, 200.0));
        let placement =
            PhysicalRect::new(400.0, 100.0, 300.0, 200.0).expect("native placement is valid");
        assert_eq!(
            preview_rect(
                source,
                bounds,
                DockspacePreviewVisual::Native {
                    host_surface: source,
                    target_surface: target,
                    placement,
                },
            ),
            Some(bounds.shrink(4.0))
        );
        assert_eq!(
            preview_rect(
                target,
                bounds,
                DockspacePreviewVisual::Dock {
                    surface: source,
                    rect: LogicalRect::new(10.0, 20.0, 30.0, 40.0)
                        .expect("logical preview is valid"),
                },
            ),
            None
        );
    }
}

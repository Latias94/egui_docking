//! Core-derived four-way and center docking-guide rendering.

use dockspace::runtime::{
    DockspaceDropDirection, DockspaceDropEligibility, DropAffordanceTargetPaintRecord,
};
use egui::Sense;
use egui::accesskit::Role;

use crate::guide_paint::{GuideCueDirection, describe_guide_button, paint_guide_button};

use super::RenderContext;
use super::geometry::{accesskit_bounds, egui_rect};

const CENTER_DROP_GUIDE_LABEL: &str = "Dockspace center drop guide";

pub(crate) fn paint(context: &mut RenderContext<'_, '_, '_>) {
    let Some(affordance) = context.plan.drop_affordance() else {
        return;
    };
    for cluster in affordance.clusters() {
        for target in cluster.targets() {
            paint_target(context, target);
        }
    }
}

fn paint_target(
    context: &mut RenderContext<'_, '_, '_>,
    target: DropAffordanceTargetPaintRecord<'_>,
) {
    let Some(button) = egui_rect(target.draw_bounds()) else {
        return;
    };
    let paint = describe_guide_button(
        button,
        guide_direction(target.direction()),
        target.is_active(),
        target.eligibility() != DockspaceDropEligibility::Rejected,
        context.visuals,
    );
    paint_guide_button(context.ui.painter(), paint);
    if let Some(receiver) = context.plan.receiver_for_drop_target(target)
        && let Some(hit) = egui_rect(receiver.bounds())
    {
        let id =
            context
                .ui
                .make_persistent_id((context.instance_id, "drop-target", target.visual_id()));
        let _ = context.interact_receiver(hit, id, Sense::hover(), Some(receiver));
        if target.direction() == DockspaceDropDirection::Center {
            context.ui.ctx().accesskit_node_builder(id, |node| {
                node.set_role(Role::Label);
                node.set_bounds(accesskit_bounds(hit));
                node.set_label(CENTER_DROP_GUIDE_LABEL);
            });
        }
    }
}

const fn guide_direction(direction: DockspaceDropDirection) -> GuideCueDirection {
    match direction {
        DockspaceDropDirection::Center => GuideCueDirection::Center,
        DockspaceDropDirection::Left => GuideCueDirection::Left,
        DockspaceDropDirection::Right => GuideCueDirection::Right,
        DockspaceDropDirection::Top => GuideCueDirection::Top,
        DockspaceDropDirection::Bottom => GuideCueDirection::Bottom,
    }
}

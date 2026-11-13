//! Core-derived four-way and center docking-guide rendering.

use dockspace::runtime::{
    DockspaceDropDirection, DockspaceGuideScope, DropAffordanceClusterPaintRecord,
    DropAffordanceTargetPaintRecord,
};
use egui::accesskit::Role;
use egui::{Sense, UiBuilder};

use crate::{RootId, drop_guide::describe_guide_button};

use super::RenderContext;
use super::geometry::{accesskit_bounds, egui_rect};

const CENTER_DROP_GUIDE_LABEL: &str = "Dockspace center drop guide";

pub(crate) fn paint_cluster(
    context: &mut RenderContext<'_, '_, '_>,
    cluster: DropAffordanceClusterPaintRecord<'_>,
) {
    let mut paint_audit = context.paint_audit.take();
    let painter = context.ui.painter().clone();
    let mut paint_targets = || {
        for target in cluster.targets() {
            paint_target(context, cluster.root(), cluster.scope(), target);
        }
    };
    if let Some(audit) = paint_audit.as_mut() {
        audit.check_append(&painter, paint_targets);
    } else {
        paint_targets();
    }
    context.paint_audit = paint_audit;
}

fn paint_target(
    context: &mut RenderContext<'_, '_, '_>,
    root: RootId,
    scope: DockspaceGuideScope,
    target: DropAffordanceTargetPaintRecord<'_>,
) {
    let Some(button) = egui_rect(target.draw_bounds()) else {
        return;
    };
    let id =
        context
            .ui
            .make_persistent_id((context.instance_id, "drop-target", target.visual_id()));
    let guide = describe_guide_button(id, root, scope, target, button, context.visuals);
    let mut child = context
        .ui
        .new_child(UiBuilder::new().id_salt(id).max_rect(button));
    child.set_clip_rect(context.ui.clip_rect().intersect(button));
    context.panes.drop_guide_ui(&child, &guide);
    drop(child);
    if let Some(receiver) = context.plan.receiver_for_drop_target(target)
        && let Some(hit) = egui_rect(receiver.bounds())
    {
        #[cfg(test)]
        let response = context.interact_receiver(hit, id, Sense::hover(), Some(receiver));
        #[cfg(not(test))]
        let _ = context.interact_receiver(hit, id, Sense::hover(), Some(receiver));
        #[cfg(test)]
        context.observe_drop_target(target.direction(), target.visual_id(), &response, receiver);
        context.ui.ctx().accesskit_node_builder(id, |node| {
            node.set_role(Role::Label);
            node.set_bounds(accesskit_bounds(hit));
            node.set_label(guide_label(target.direction()));
        });
    }
}

const fn guide_label(direction: DockspaceDropDirection) -> &'static str {
    match direction {
        DockspaceDropDirection::Center => CENTER_DROP_GUIDE_LABEL,
        DockspaceDropDirection::Left => "Dockspace left drop guide",
        DockspaceDropDirection::Right => "Dockspace right drop guide",
        DockspaceDropDirection::Top => "Dockspace top drop guide",
        DockspaceDropDirection::Bottom => "Dockspace bottom drop guide",
    }
}

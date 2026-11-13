//! Close-button painting over adapter-owned input and core close requests.

use dockspace::runtime::TabPaintRecord;
use egui::accesskit::{Action, Role};
use egui::{Sense, UiBuilder, vec2};

use crate::{TabClose, TabCloseVisibility, TabVisuals};

use super::RenderContext;
use super::actions::{button_activation_origin, effective_enabled};
use super::geometry::{accesskit_bounds, egui_rect};
use super::measurement::TabPaintResource;

pub(super) fn paint(
    context: &mut RenderContext<'_, '_, '_>,
    tab: TabPaintRecord<'_>,
    resource: &TabPaintResource,
    tab_response: &egui::Response,
    tab_visuals: Option<TabVisuals>,
) {
    let Some(rect) = tab.close_bounds().and_then(egui_rect) else {
        return;
    };
    let id = context
        .ui
        .make_persistent_id((context.instance_id, "tab-close", tab.item()));
    let receiver = context.plan.receiver_for_tab_close(tab);
    let enabled = effective_enabled(context.ui, receiver.is_some());
    let response = context
        .interact_receiver(
            rect,
            id,
            if enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
            enabled.then_some(receiver).flatten(),
        )
        .on_hover_text(format!("Close {}", resource.title));
    let hovered = tab_response.hovered() || response.hovered();
    let visible = tab_response.has_focus()
        || response.has_focus()
        || match context.style.tab_close_visibility {
            TabCloseVisibility::Always => true,
            TabCloseVisibility::SelectedOrHovered => tab.selected() || hovered,
            TabCloseVisibility::Hovered => hovered,
        };
    if visible && let Some(tab_visuals) = tab_visuals {
        let mut stroke =
            context.interaction_stroke(&response, context.style.visuals.tab_text_color);
        stroke.color = tab_visuals.close_color.unwrap_or(stroke.color);
        let size = context
            .style
            .tab_close_icon_size
            .min(rect.width().min(rect.height()));
        let close = TabClose {
            item: tab.item(),
            rect,
            icon_rect: egui::Rect::from_center_size(rect.center(), vec2(size, size)),
            response: &response,
            selected: tab.selected(),
            enabled,
            stroke,
        };
        let mut child = context
            .ui
            .new_child(UiBuilder::new().id_salt((id, "paint")).max_rect(rect));
        child.set_clip_rect(context.ui.clip_rect().intersect(rect));
        if !enabled && child.is_enabled() {
            child.disable();
        }
        if resource.missing {
            close.paint(&child);
        } else {
            context.panes.tab_close_ui(&child, &close);
        }
    }
    context.ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(Role::Button);
        node.set_bounds(accesskit_bounds(rect));
        node.set_label(format!("Close {}", resource.title));
        if enabled {
            node.add_action(Action::Click);
        } else {
            node.set_disabled();
        }
    });
    if enabled
        && let Some(origin) =
            button_activation_origin(context.ui, &response, context.pointer_authority)
        && let Some(action) = context.plan.prepare_tab_close(tab.item())
    {
        context.push_local_action(origin, action);
    }
}

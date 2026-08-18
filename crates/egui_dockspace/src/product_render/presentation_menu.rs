//! Root presentation commands rendered with egui's native menu primitives.

use dockspace::model::DockspaceActionRejection;
use dockspace::runtime::{
    DockspacePresentationCommandKind, DockspacePresentationCommandUnavailable,
    DockspacePresentationMenuAnchorKind, DockspaceRuntimeError, PresentationMenuAnchorPaintRecord,
};
use egui::accesskit::{Action, HasPopup, Role};
use egui::{Button, Popup, Sense, vec2};

use super::RenderContext;
use super::geometry::{accesskit_bounds, egui_rect};

const COMMANDS: [DockspacePresentationCommandKind; 3] = [
    DockspacePresentationCommandKind::Float,
    DockspacePresentationCommandKind::MoveToNewWindow,
    DockspacePresentationCommandKind::DockBack,
];

pub(super) fn paint(
    context: &mut RenderContext<'_, '_, '_>,
    anchor: PresentationMenuAnchorPaintRecord<'_>,
) -> Result<(), DockspaceRuntimeError> {
    let Some(bounds) = egui_rect(anchor.bounds()) else {
        return Ok(());
    };
    let id = context.ui.make_persistent_id((
        context.instance_id,
        "presentation-menu-anchor",
        anchor.visual_id(),
    ));
    let popup_id = id.with("popup");
    let operable = anchor.operable();
    let response = context
        .ui
        .interact(
            bounds,
            id,
            if operable {
                Sense::click()
            } else {
                Sense::hover()
            },
        )
        .on_hover_text("Window actions");
    let ui_enabled = context.ui.is_enabled() && operable;
    let menu_open = Popup::is_id_open(context.ui.ctx(), popup_id);

    let paint_omitted = context
        .plan
        .drag_decoration()
        .is_some_and(|decoration| decoration.omits_visual(anchor.visual_id()));
    if !paint_omitted {
        paint_anchor_icon(context, anchor, bounds, &response);
    }
    context.ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(Role::Button);
        node.set_bounds(accesskit_bounds(bounds));
        node.set_label("Presentation commands");
        node.set_has_popup(HasPopup::Menu);
        node.set_expanded(menu_open);
        if ui_enabled {
            node.add_action(Action::Click);
        } else {
            node.set_disabled();
            node.clear_actions();
        }
    });

    if !menu_open && (!ui_enabled || !response.clicked()) {
        return Ok(());
    }

    let commands = COMMANDS
        .into_iter()
        .map(|kind| context.plan.presentation_command(anchor.root(), kind))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let mut activated = None;
    Popup::menu(&response).id(popup_id).show(|ui| {
        ui.ctx().accesskit_node_builder(ui.unique_id(), |node| {
            node.set_role(Role::Menu);
            node.set_label("Presentation commands");
        });
        for command in commands.iter().copied() {
            let label = command_label(command.kind());
            let enabled = ui_enabled && command.is_ready();
            let response = ui.add_enabled(enabled, Button::new(label));
            ui.ctx().accesskit_node_builder(response.id, |node| {
                node.set_role(Role::MenuItem);
                node.set_label(label);
                node.clear_actions();
                if enabled {
                    node.add_action(Action::Click);
                    node.add_action(Action::Focus);
                } else {
                    node.set_disabled();
                    if let Some(reason) = command.unavailable_reason() {
                        node.set_description(unavailable_text(reason));
                    }
                }
            });
            let response = if let Some(reason) = command.unavailable_reason() {
                response.on_disabled_hover_text(unavailable_text(reason))
            } else {
                response
            };
            if response.clicked() {
                activated = Some(command);
                ui.close();
            }
        }
    });

    if let Some(command) = activated
        && let Some(action) = context.plan.prepare_presentation_command(command)
    {
        context.push_local_action(action);
    }
    Ok(())
}

fn paint_anchor_icon(
    context: &RenderContext<'_, '_, '_>,
    anchor: PresentationMenuAnchorPaintRecord<'_>,
    bounds: egui::Rect,
    response: &egui::Response,
) {
    let (emphasized_fill, idle_color) = match anchor.kind() {
        DockspacePresentationMenuAnchorKind::TabBar => (
            context.interaction_fill(response),
            context.style.visuals.tab_text_color,
        ),
        DockspacePresentationMenuAnchorKind::ContainedTitle => {
            let contained = context
                .plan
                .contained()
                .find(|contained| contained.root() == anchor.root());
            let title_text_color = contained
                .map(|contained| {
                    context
                        .style
                        .resolved_contained_window(context.ui.style(), contained.is_frontmost())
                        .title_text_color
                })
                .unwrap_or(context.ui.visuals().widgets.noninteractive.fg_stroke.color);
            (
                context.ui.style().interact(response).weak_bg_fill,
                Some(title_text_color),
            )
        }
    };
    if RenderContext::response_emphasized(response) {
        context
            .ui
            .painter()
            .rect_filled(bounds, 2.0, emphasized_fill);
    }
    let color = context.interaction_stroke(response, idle_color).color;
    let center = bounds.center();
    let spacing = bounds.height().min(bounds.width()) * 0.16;
    let radius = (bounds.height().min(bounds.width()) * 0.055).clamp(1.0, 1.8);
    for y in [-1.0, 0.0, 1.0] {
        context
            .ui
            .painter()
            .circle_filled(center + vec2(0.0, y * spacing), radius, color);
    }
}

const fn command_label(kind: DockspacePresentationCommandKind) -> &'static str {
    match kind {
        DockspacePresentationCommandKind::Float => "Float",
        DockspacePresentationCommandKind::MoveToNewWindow => "Move to New Window",
        DockspacePresentationCommandKind::DockBack => "Dock Back",
    }
}

const fn unavailable_text(reason: DockspacePresentationCommandUnavailable) -> &'static str {
    match reason {
        DockspacePresentationCommandUnavailable::AlreadyContained => {
            "This root is already in a floating egui window."
        }
        DockspacePresentationCommandUnavailable::AlreadyNative => {
            "This root is already in a native child window."
        }
        DockspacePresentationCommandUnavailable::Action(reason) => action_unavailable_text(reason),
    }
}

const fn action_unavailable_text(reason: DockspaceActionRejection) -> &'static str {
    match reason {
        DockspaceActionRejection::PolicyDenied => {
            "This command is disabled by the current docking policy."
        }
        DockspaceActionRejection::Conflict => {
            "Another docking transition is currently in progress."
        }
        DockspaceActionRejection::IdentityExhausted => {
            "No presentation identity is currently available."
        }
        DockspaceActionRejection::NativeUnavailable => {
            "Native child windows are unavailable for this host."
        }
        DockspaceActionRejection::ItemUnavailable { .. }
        | DockspaceActionRejection::ItemNotContained { .. }
        | DockspaceActionRejection::AnchorUnavailable { .. }
        | DockspaceActionRejection::RootUnavailable { .. }
        | DockspaceActionRejection::DockBackUnavailable { .. }
        | DockspaceActionRejection::MainSurfaceUnavailable { .. }
        | DockspaceActionRejection::SurfaceUnavailable { .. }
        | DockspaceActionRejection::PresentationUnavailable { .. } => {
            "The current layout cannot perform this command."
        }
    }
}

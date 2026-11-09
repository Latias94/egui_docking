//! Contained-floating chrome and semantic keyboard/accessibility actions.

use dockspace::runtime::{
    ContainedPaintRecord, ContainedResizeDirection, SurfaceContainedResizeAdjustment,
    SurfacePaintDisposition, SurfacePaintProgramRoot,
};
use egui::accesskit::{Action, Orientation, Role};
use egui::{CursorIcon, EventFilter, Key, Sense};

use crate::style::ResolvedContainedWindowVisuals;

use super::RenderContext;
use super::actions::{button_activation_origin, effective_enabled};
use super::geometry::{accesskit_bounds, egui_rect};

pub(crate) fn paint_background(
    context: &mut RenderContext<'_, '_, '_>,
    contained: ContainedPaintRecord<'_>,
    root: &SurfacePaintProgramRoot<'_>,
    visuals: ResolvedContainedWindowVisuals,
    paint: SurfacePaintDisposition,
) {
    let Some(outer) = egui_rect(contained.outer_bounds()) else {
        return;
    };
    let Some(title) = egui_rect(contained.title_bounds()) else {
        return;
    };
    let paint_omitted = context.source_gap_active(paint);
    if !paint_omitted {
        let content_rect = outer.shrink(visuals.frame.stroke.width);
        context.ui.painter().add(visuals.frame.paint(content_rect));
        let mut title_corner_radius = visuals.frame.corner_radius;
        title_corner_radius.sw = 0;
        title_corner_radius.se = 0;
        context
            .ui
            .painter()
            .rect_filled(title, title_corner_radius, visuals.title_fill);
    }

    let scroll_id = context.ui.make_persistent_id((
        context.instance_id,
        "contained-window-scroll-blocker",
        contained.visual_id(),
    ));
    let scroll_receiver = context.plan.receiver_for_contained_frame(contained);
    context.register_scroll_receiver(scroll_id, scroll_receiver);

    // A contained surface is a foreground receiver even when its pane does not
    // create widgets. Register both egui capture lanes before the pane widgets
    // so actual foreground content can supersede the blocker while background
    // widgets remain unreachable through the floating surface.
    for (lane, sense) in [("click", Sense::CLICK), ("drag", Sense::DRAG)] {
        let id = context.ui.make_persistent_id((
            context.instance_id,
            "contained-window-blocker",
            contained.visual_id(),
            lane,
        ));
        let _ = context.interact_receiver(
            outer,
            id,
            sense,
            context.plan.receiver_for_contained_frame(contained),
        );
    }

    if !paint_omitted {
        let label = title_label(context, root);
        let drag = egui_rect(contained.title_drag_bounds()).unwrap_or(title);
        let title_text_bounds = egui::Rect::from_min_max(
            egui::pos2((drag.left() + 20.0).min(drag.right()), title.top()),
            egui::pos2(drag.right(), title.bottom()),
        );
        let title_content_bounds = inset_title_rect(title_text_bounds, visuals.title_margin);
        if title_content_bounds.is_positive() {
            let galley = egui::WidgetText::from(label).into_galley(
                context.ui,
                Some(egui::TextWrapMode::Truncate),
                title_content_bounds.width(),
                egui::TextStyle::Button,
            );
            let position = title_content_bounds.center() - galley.size() * 0.5;
            context.ui.painter_at(title_content_bounds).galley(
                position,
                galley,
                visuals.title_text_color,
            );
        }
    }
}

fn inset_title_rect(rect: egui::Rect, margin: egui::Margin) -> egui::Rect {
    let min = egui::pos2(
        (rect.left() + f32::from(margin.left)).min(rect.right()),
        rect.top(),
    );
    let max = egui::pos2(
        (rect.right() - f32::from(margin.right)).max(min.x),
        rect.bottom(),
    );
    egui::Rect::from_min_max(min, max)
}

pub(crate) fn paint_controls(
    context: &mut RenderContext<'_, '_, '_>,
    contained: ContainedPaintRecord<'_>,
    root: &SurfacePaintProgramRoot<'_>,
    paint: SurfacePaintDisposition,
) {
    let title_text_color = context
        .style
        .resolved_contained_window(context.ui.style(), contained.is_frontmost())
        .title_text_color;
    let paint_omitted = context.source_gap_active(paint);
    if let Some(title) = egui_rect(contained.title_drag_bounds()) {
        let core_enabled = effective_enabled(context.ui, contained.title_operable());
        let enabled = core_enabled && context.pointer_authority.supports_preview_gestures();
        let id = context.ui.make_persistent_id((
            context.instance_id,
            "contained-title",
            contained.visual_id(),
        ));
        let response = context.interact_receiver(
            title,
            id,
            if enabled {
                Sense::click_and_drag()
            } else {
                Sense::hover()
            },
            enabled
                .then(|| context.plan.receiver_for_contained_title(contained))
                .flatten(),
        );
        let description = if enabled {
            "Drag to move floating window"
        } else if core_enabled {
            "Dragging floating windows is unavailable in this host"
        } else {
            "Floating window movement is unavailable"
        };
        let label = title_label(context, root);
        let response = response.on_hover_text(format!("{label}\n{description}"));
        context.ui.ctx().accesskit_node_builder(id, |node| {
            node.set_role(Role::TitleBar);
            node.set_bounds(accesskit_bounds(title));
            node.set_label(label);
            node.set_description(description);
            if !enabled {
                node.set_disabled();
            }
        });
        if !paint_omitted && enabled {
            paint_title_grip(
                context.ui,
                title,
                context.interaction_stroke(&response, Some(title_text_color)),
            );
        }
        if enabled && (response.hovered() || paint_omitted) {
            context.ui.ctx().set_cursor_icon(if paint_omitted {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Grab
            });
        }
    }

    if let Some(close) = contained.close_bounds().and_then(egui_rect) {
        let enabled = effective_enabled(context.ui, contained.close_operable())
            && context
                .plan
                .prepare_contained_close(contained.floating())
                .is_some();
        let id = context.ui.make_persistent_id((
            context.instance_id,
            "contained-close",
            contained.floating(),
        ));
        let response = context.interact_receiver(
            close,
            id,
            if enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
            enabled
                .then(|| context.plan.receiver_for_contained_close(contained))
                .flatten(),
        );
        let label = title_label(context, root);
        let response = response.on_hover_text(format!("Close all panes in {label}"));
        context.ui.ctx().accesskit_node_builder(id, |node| {
            node.set_role(Role::Button);
            node.set_bounds(accesskit_bounds(close));
            node.set_label(format!("Close floating {label}"));
            if !enabled {
                node.set_disabled();
            }
        });
        if !paint_omitted {
            let visuals = *context.ui.style().interact(&response);
            let close = close.shrink(2.0).expand(visuals.expansion);
            let stroke = visuals.fg_stroke;
            context
                .ui
                .painter()
                .line_segment([close.left_top(), close.right_bottom()], stroke);
            context
                .ui
                .painter()
                .line_segment([close.right_top(), close.left_bottom()], stroke);
        }
        if enabled
            && let Some(origin) =
                button_activation_origin(context.ui, &response, context.pointer_authority)
            && let Some(action) = context.plan.prepare_contained_close(contained.floating())
        {
            context.push_local_action(origin, action);
        }
    }

    for resize in contained.resize() {
        let Some(bounds) = egui_rect(resize.hit_bounds()) else {
            continue;
        };
        let id = context.ui.make_persistent_id((
            context.instance_id,
            "contained-resize",
            contained.floating(),
            resize.direction(),
        ));
        let cardinal = cardinal_resize_axis(resize.direction());
        let enabled = effective_enabled(context.ui, resize.operable());
        let drag_enabled = enabled && context.pointer_authority.supports_preview_gestures();
        let response = context.interact_receiver(
            bounds,
            id,
            match (drag_enabled, cardinal, enabled) {
                (true, Some(_), _) => Sense::drag(),
                (true, None, _) => Sense::DRAG,
                (false, Some(_), true) => Sense::focusable_noninteractive(),
                (false, _, _) => Sense::hover(),
            },
            drag_enabled
                .then(|| {
                    context
                        .plan
                        .receiver_for_contained_resize(contained, resize)
                })
                .flatten(),
        );
        let enabled = enabled && response.enabled();
        if let Some((orientation, horizontal_arrows)) = cardinal {
            configure_resize_accessibility(
                context.ui,
                &response,
                bounds,
                resize.direction(),
                orientation,
                enabled,
            );
            if enabled && response.has_focus() {
                context.ui.memory_mut(|memory| {
                    memory.set_focus_lock_filter(
                        id,
                        EventFilter {
                            horizontal_arrows,
                            vertical_arrows: !horizontal_arrows,
                            ..Default::default()
                        },
                    );
                });
            }
            if enabled
                && let Some((origin, adjustment)) = contained_resize_adjustment(
                    context.ui,
                    id,
                    resize.direction(),
                    response.has_focus(),
                )
                && let Some(action) = context.plan.prepare_contained_resize_adjustment(
                    contained.floating(),
                    resize.direction(),
                    adjustment,
                )
            {
                context.push_local_action(origin, action);
            }
        }
        if drag_enabled && response.hovered() {
            context
                .ui
                .ctx()
                .set_cursor_icon(resize_cursor(resize.direction()));
        }
    }
}

fn paint_title_grip(ui: &egui::Ui, title: egui::Rect, stroke: egui::Stroke) {
    let grip = egui::Rect::from_center_size(
        egui::pos2(title.left() + 10.0, title.center().y),
        egui::vec2(10.0, 14.0),
    );
    let spacing = 3.5_f32.min(grip.width() * 0.22).min(grip.height() * 0.22);
    let radius = 1.0_f32.min(spacing * 0.32);
    for x in [-0.5, 0.5] {
        for y in [-1.0, 0.0, 1.0] {
            ui.painter().circle_filled(
                grip.center() + egui::vec2(x * spacing, y * spacing),
                radius,
                stroke.color,
            );
        }
    }
}

fn contained_resize_adjustment(
    ui: &egui::Ui,
    id: egui::Id,
    direction: ContainedResizeDirection,
    focused: bool,
) -> Option<(egui::Id, SurfaceContainedResizeAdjustment)> {
    if focused {
        let keyboard = ui.input_mut(|input| {
            let (increment, decrement) = match direction {
                ContainedResizeDirection::North => (Key::ArrowDown, Key::ArrowUp),
                ContainedResizeDirection::East => (Key::ArrowRight, Key::ArrowLeft),
                ContainedResizeDirection::South => (Key::ArrowDown, Key::ArrowUp),
                ContainedResizeDirection::West => (Key::ArrowRight, Key::ArrowLeft),
                ContainedResizeDirection::NorthEast
                | ContainedResizeDirection::SouthEast
                | ContainedResizeDirection::SouthWest
                | ContainedResizeDirection::NorthWest => return None,
            };
            if input.consume_key(egui::Modifiers::NONE, increment) {
                Some((
                    id.with(("keyboard-resize", increment)),
                    SurfaceContainedResizeAdjustment::Increment,
                ))
            } else if input.consume_key(egui::Modifiers::NONE, decrement) {
                Some((
                    id.with(("keyboard-resize", decrement)),
                    SurfaceContainedResizeAdjustment::Decrement,
                ))
            } else {
                None
            }
        });
        if keyboard.is_some() {
            return keyboard;
        }
    }
    ui.input(|input| {
        if input.has_accesskit_action_request(id, Action::Increment) {
            Some((
                id.with("accesskit-increment"),
                SurfaceContainedResizeAdjustment::Increment,
            ))
        } else if input.has_accesskit_action_request(id, Action::Decrement) {
            Some((
                id.with("accesskit-decrement"),
                SurfaceContainedResizeAdjustment::Decrement,
            ))
        } else {
            None
        }
    })
}

fn configure_resize_accessibility(
    ui: &egui::Ui,
    response: &egui::Response,
    bounds: egui::Rect,
    direction: ContainedResizeDirection,
    orientation: Orientation,
    operable: bool,
) {
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(Role::Splitter);
        node.set_bounds(accesskit_bounds(bounds));
        node.set_label(match direction {
            ContainedResizeDirection::North => "Resize floating top edge",
            ContainedResizeDirection::East => "Resize floating right edge",
            ContainedResizeDirection::South => "Resize floating bottom edge",
            ContainedResizeDirection::West => "Resize floating left edge",
            ContainedResizeDirection::NorthEast
            | ContainedResizeDirection::SouthEast
            | ContainedResizeDirection::SouthWest
            | ContainedResizeDirection::NorthWest => {
                unreachable!("diagonal resize handles are not accessibility controls")
            }
        });
        node.set_orientation(orientation);
        if operable {
            node.add_action(Action::Increment);
            node.add_action(Action::Decrement);
        } else {
            node.set_disabled();
        }
    });
}

const fn cardinal_resize_axis(direction: ContainedResizeDirection) -> Option<(Orientation, bool)> {
    match direction {
        ContainedResizeDirection::North | ContainedResizeDirection::South => {
            Some((Orientation::Horizontal, false))
        }
        ContainedResizeDirection::East | ContainedResizeDirection::West => {
            Some((Orientation::Vertical, true))
        }
        ContainedResizeDirection::NorthEast
        | ContainedResizeDirection::SouthEast
        | ContainedResizeDirection::SouthWest
        | ContainedResizeDirection::NorthWest => None,
    }
}

fn title_label<'a>(
    context: &'a RenderContext<'_, '_, '_>,
    root: &SurfacePaintProgramRoot<'_>,
) -> &'a str {
    root.title_item()
        .and_then(|item| context.resources.item(item))
        .map_or("Floating", |resource| resource.title.as_str())
}

const fn resize_cursor(direction: ContainedResizeDirection) -> CursorIcon {
    match direction {
        ContainedResizeDirection::North | ContainedResizeDirection::South => {
            CursorIcon::ResizeVertical
        }
        ContainedResizeDirection::East | ContainedResizeDirection::West => {
            CursorIcon::ResizeHorizontal
        }
        ContainedResizeDirection::NorthEast | ContainedResizeDirection::SouthWest => {
            CursorIcon::ResizeNeSw
        }
        ContainedResizeDirection::SouthEast | ContainedResizeDirection::NorthWest => {
            CursorIcon::ResizeNwSe
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        Dockspace, DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
        DockspaceSurfaceLayout, FloatingPresentationId, ItemId, LogicalRect, PaneResponse,
        PaneView, RootId, SurfaceId,
    };
    use egui::{Context, RawInput, Rect, Shape, Ui, WidgetText, pos2, vec2};

    struct TitlePanes;

    impl PaneView for TitlePanes {
        fn title(&self, item: ItemId) -> Option<WidgetText> {
            Some(if item == ItemId::new(1) {
                "Editor".into()
            } else {
                "Inspector".into()
            })
        }

        fn ui(&mut self, _item: ItemId, _ui: &mut Ui) -> PaneResponse {
            PaneResponse::default()
        }
    }

    #[test]
    fn floating_title_glyphs_fit_without_using_vertical_window_margins() {
        for vertical_margin in [6, 24] {
            let context = Context::default();
            context.global_style_mut(|style| {
                style.spacing.window_margin = egui::Margin::symmetric(8, vertical_margin);
            });
            let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
                SurfaceId::new(1),
                DockspaceRootLayout::new(
                    RootId::new(1),
                    DockspaceNode::central_tabs([ItemId::new(1)]),
                ),
            )
            .with_contained(DockspaceContainedLayout::new(
                FloatingPresentationId::new(1),
                DockspaceRootLayout::new(RootId::new(2), DockspaceNode::tabs([ItemId::new(2)])),
                LogicalRect::new(160.0, 100.0, 320.0, 240.0).unwrap(),
            ))])
            .unwrap();
            let mut dockspace = Dockspace::builder("title-glyph-clip", layout)
                .build()
                .unwrap();
            let mut panes = TitlePanes;
            for pass in 0..2 {
                let mut output = context.run_ui(
                    RawInput {
                        screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(640.0, 480.0))),
                        ..Default::default()
                    },
                    |ui| {
                        dockspace
                            .show_single_surface(SurfaceId::new(1), ui, &mut panes)
                            .unwrap();
                    },
                );
                if pass == 0 {
                    output.textures_delta.clear();
                    continue;
                }
                let (clip, text) = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        Shape::Text(text) if text.galley.job.text == "Inspector" => {
                            Some((shape.clip_rect, text))
                        }
                        _ => None,
                    })
                    .min_by(|(_, a), (_, b)| a.pos.y.total_cmp(&b.pos.y))
                    .expect("the actual floating title was painted");
                assert!(
                    clip.contains_rect(text.visual_bounding_rect()),
                    "vertical window margin {vertical_margin} must not clip title glyphs",
                );
                assert!(clip.top() >= 100.0 && clip.bottom() < 140.0);
                assert!(
                    clip.left() > 180.0,
                    "title text leaves the grip gutter clear"
                );
                assert!(
                    clip.right() < 450.0,
                    "title text leaves trailing controls clear"
                );
                output.textures_delta.clear();
            }
        }
    }
}

//! Application tab callbacks over core-owned geometry and interaction.

use dockspace::policy::TabBarInteraction;
use dockspace::runtime::{SurfacePaintDisposition, TabBarPaintRecord, TabPaintRecord};
use egui::{Align, Layout, Response, Stroke, UiBuilder};

use crate::{Tab, TabBar, TabBarId, TabBarSlot, TabVisuals};

use super::RenderContext;
use super::actions::effective_enabled;
use super::geometry::egui_rect;
use super::measurement::TabPaintResource;

pub(super) fn paint_tab(
    context: &mut RenderContext<'_, '_, '_>,
    record: TabPaintRecord<'_>,
    resource: &TabPaintResource,
    response: &Response,
    pane_focused: bool,
) -> TabVisuals {
    let selected = record.selected();
    let text_override = if selected {
        context.style.visuals.tab_active_text_color
    } else {
        context.style.visuals.tab_text_color
    };
    let visuals = TabVisuals {
        fill: if selected {
            context.visuals.tab_active_fill
        } else if RenderContext::response_emphasized(response) {
            context.interaction_fill(response)
        } else {
            context.visuals.tab_fill
        },
        stroke: if selected {
            context.style.visuals.tab_active_stroke
        } else {
            context.style.visuals.tab_stroke
        }
        .unwrap_or(Stroke::NONE),
        corner_radius: context.style.tab_corner_radius,
        text_color: if selected {
            context.visuals.tab_active_text_color
        } else if RenderContext::response_emphasized(response) {
            context.interaction_text_color(response, text_override)
        } else {
            context.visuals.tab_text_color
        },
        text_color_override: text_override.or(context.ui.visuals().override_text_color),
        close_color: None,
        focus_stroke: context.visuals.tab_focus_stroke,
        pane_focus_stroke: context.visuals.pane_focus_stroke,
    };
    let (Some(rect), Some(visible_rect), Some(content_rect), Some(content_clip_rect)) = (
        egui_rect(record.full_bounds()),
        egui_rect(record.visible_bounds()),
        egui_rect(record.text_layout_bounds()),
        egui_rect(record.text_bounds()),
    ) else {
        return visuals;
    };
    let tab = Tab {
        item: record.item(),
        bar: TabBarId(record.tab_bar_visual_id()),
        rect,
        visible_rect,
        content_rect,
        content_clip_rect,
        title: resource.galley.clone(),
        response,
        selected,
        pane_focused,
        enabled: effective_enabled(context.ui, record.operable()),
        visuals,
    };
    let mut child = context.ui.new_child(
        UiBuilder::new()
            .id_salt((context.instance_id, "tab-ui", record.visual_id()))
            .max_rect(content_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.set_clip_rect(context.ui.clip_rect().intersect(visible_rect));
    if !tab.enabled && child.is_enabled() {
        child.disable();
    }
    if resource.missing {
        tab.paint(&child)
    } else {
        context.panes.tab_ui(&mut child, &tab)
    }
}

pub(super) fn paint_bar_slots(
    context: &mut RenderContext<'_, '_, '_>,
    record: TabBarPaintRecord<'_>,
    paint: SurfacePaintDisposition,
) {
    if paint.is_source_gap() {
        return;
    }
    for (slot, bounds) in [
        (TabBarSlot::Leading, record.leading_reserved_bounds()),
        (TabBarSlot::Trailing, record.trailing_reserved_bounds()),
        (TabBarSlot::AfterTabs, record.after_tabs_bounds()),
    ] {
        let Some(bounds) = bounds.and_then(egui_rect).filter(egui::Rect::is_positive) else {
            continue;
        };
        let clip = if slot == TabBarSlot::AfterTabs {
            let Some(viewport) = egui_rect(record.viewport()) else {
                continue;
            };
            bounds.intersect(viewport)
        } else {
            bounds
        };
        let clip = context.ui.clip_rect().intersect(clip);
        if !clip.is_positive() {
            continue;
        }
        let mut child = context.ui.new_child(
            UiBuilder::new()
                .id_salt((
                    context.instance_id,
                    "tab-bar-slot",
                    record.visual_id(),
                    slot,
                ))
                .max_rect(bounds)
                .layout(Layout::left_to_right(Align::Center)),
        );
        child.set_clip_rect(clip);
        if record.interaction() == TabBarInteraction::Disabled && child.is_enabled() {
            child.disable();
        }
        context
            .panes
            .tab_bar_ui(&mut child, TabBar { record }, slot);
    }
}

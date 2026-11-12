//! Optional application chrome. Dockspace still owns docking and close input.

use egui::{Color32, CornerRadius, Rect, Stroke, Ui, pos2, vec2};
use egui_dockspace::{Tab, TabBar, TabBarSlot};

use super::DemoPanes;

const ACCENTS: [Color32; 3] = [
    Color32::from_rgb(48, 112, 172),
    Color32::from_rgb(35, 125, 105),
    Color32::from_rgb(130, 78, 160),
];

pub(super) fn tab_ui(panes: &DemoPanes, ui: &Ui, tab: &Tab<'_>) {
    if !panes.custom_tabs {
        tab.paint(ui);
        return;
    }
    let accent = ACCENTS[panes.tab_accent];
    let mut custom = tab.clone();
    custom.visuals.corner_radius = CornerRadius {
        nw: 6,
        ne: 6,
        sw: 0,
        se: 0,
    };
    if tab.selected {
        custom.visuals.stroke = Stroke::new(1.0, accent);
    }
    custom.paint_background(ui.painter());
    let icon = Rect::from_center_size(
        pos2(tab.content_rect.left() + 5.0, tab.content_rect.center().y),
        vec2(8.0, 10.0),
    );
    ui.painter()
        .with_clip_rect(tab.content_clip_rect)
        .rect_filled(icon, 2.0, accent);
    custom.paint_title(
        ui,
        Rect::from_min_max(tab.content_rect.min + vec2(18.0, 0.0), tab.content_rect.max),
    );
}

pub(super) fn tab_bar_ui(panes: &mut DemoPanes, ui: &mut Ui, bar: TabBar<'_>, slot: TabBarSlot) {
    match slot {
        TabBarSlot::Leading => {
            ui.small(bar.items().len().to_string());
        }
        TabBarSlot::Trailing => {
            if ui.small_button("Tint").clicked() {
                panes.tab_accent = (panes.tab_accent + 1) % ACCENTS.len();
            }
        }
    }
}

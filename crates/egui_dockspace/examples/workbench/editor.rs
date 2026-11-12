//! Application content and docking layout. No window-system or renderer code.

use std::collections::BTreeMap;

use egui::{Color32, Context, FontId, Frame, Margin, TextStyle, Theme, Vec2};
use egui_dockspace::{
    CloseDecision, CloseItemDecisionState, DockStyle, DockVisualOverrides, Dockspace,
    DockspaceAxis, DockspaceContainedLayout, DockspaceError, DockspaceLayout, DockspaceNode,
    DockspaceOutput, DockspaceResponse, DockspaceRootLayout, DockspaceSurfaceLayout,
    FloatingPresentationId, ItemId, LogicalRect, PaneResponse, PaneView, RootId, SurfaceId, Tab,
    TabBar, TabBarId, TabBarLayout, TabBarSlot,
};

mod tabs;

pub(super) const SURFACE: SurfaceId = SurfaceId::new(1);
pub(super) const MAIN_ROOT: RootId = RootId::new(1);
const FLOATING_ROOT: RootId = RootId::new(2);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);

pub(super) const EDITOR: ItemId = ItemId::new(1);
pub(super) const PREVIEW: ItemId = ItemId::new(2);
pub(super) const OUTLINE: ItemId = ItemId::new(3);
pub(super) const INSPECTOR: ItemId = ItemId::new(4);
pub(super) const CONSOLE: ItemId = ItemId::new(5);
const HISTORY: ItemId = ItemId::new(6);
pub(super) const PROFILER: ItemId = ItemId::new(7);
const ASSETS: ItemId = ItemId::new(8);

pub(super) fn new_context() -> Context {
    let context = Context::default();
    context.set_theme(Theme::Light);
    context.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        for text in [TextStyle::Body, TextStyle::Button, TextStyle::Monospace] {
            style.text_styles.get_mut(&text).unwrap().size = 14.0;
        }
        let visuals = &mut style.visuals;
        if visuals.dark_mode {
            visuals.panel_fill = Color32::from_rgb(49, 63, 80);
            visuals.window_fill = Color32::from_rgb(32, 40, 52);
            visuals.extreme_bg_color = Color32::from_rgb(22, 29, 39);
        } else {
            visuals.panel_fill = Color32::from_rgb(220, 232, 244);
            visuals.window_fill = Color32::from_rgb(250, 251, 253);
            visuals.extreme_bg_color = Color32::from_rgb(236, 240, 245);
        }
    });
    context
}

pub(super) fn dock_style() -> DockStyle {
    DockStyle {
        tab_bar_height: 34.0,
        tab_min_width: 100.0,
        tab_horizontal_padding: 12.0,
        tab_close_size: 16.0,
        floating_title_height: 30.0,
        splitter_thickness: 3.0,
        visuals: DockVisualOverrides {
            tab_active_fill: Some(Color32::from_rgb(48, 112, 172)),
            tab_active_text_color: Some(Color32::WHITE),
            splitter_color: Some(Color32::from_rgb(116, 136, 158)),
            splitter_hover_color: Some(Color32::from_rgb(52, 132, 210)),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn toolbar(ui: &mut egui::Ui, restore_layout: &mut bool, custom_tabs: &mut bool) {
    egui::Panel::top("workbench-toolbar").show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.strong("Dockspace");
            ui.separator();
            *restore_layout |= ui
                .button("Restore layout")
                .on_hover_text("Reopen every pane and keep document edits.")
                .clicked();
            ui.menu_button("Help", |ui| {
                ui.label("Drag a tab into the center of a pane to merge it.");
                ui.label("Drop near an edge to split. Drag a divider to resize.");
                ui.label("Move Inspector using its dotted title grip.");
                ui.label("Close panes freely; Restore layout keeps your edits.");
            });
            ui.checkbox(custom_tabs, "Custom tabs");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let dark = ui.visuals().dark_mode;
                if ui.selectable_label(dark, "Dark").clicked() {
                    ui.ctx().set_theme(Theme::Dark);
                }
                if ui.selectable_label(!dark, "Light").clicked() {
                    ui.ctx().set_theme(Theme::Light);
                }
            });
        });
    });
}

pub(super) fn run_editor(
    dockspace: &mut Dockspace,
    context: &Context,
    input: egui::RawInput,
    panes: &mut DemoPanes,
    restore_layout: &mut bool,
) -> Result<(DockspaceOutput, Option<DockspaceResponse>), DockspaceError> {
    let mut custom_tabs = panes.custom_tabs;
    if dockspace.view().surfaces().next().is_none() {
        let output = egui_dockspace::run_ui(context, input, |ui| {
            toolbar(ui, restore_layout, &mut custom_tabs);
            ui.centered_and_justified(|ui| {
                ui.label(
                    "All panes are closed. Restore layout to reopen them and keep your edits.",
                );
            });
        });
        panes.custom_tabs = custom_tabs;
        return Ok((output, None));
    }
    let result = dockspace
        .run_single_surface_with_ui(context, input, SURFACE, panes, |ui| {
            toolbar(ui, restore_layout, &mut custom_tabs);
        })
        .map(|(output, response)| (output, Some(response)));
    panes.custom_tabs = custom_tabs;
    result
}

pub(super) fn apply_editor_commands(
    dockspace: &mut Dockspace,
    response: Option<&DockspaceResponse>,
    restore_layout: bool,
) -> Result<bool, DockspaceError> {
    if restore_layout {
        return dockspace
            .reset_layout(demo_layout())
            .map(|mutation| mutation.workspace_changed());
    }
    let mut changed = false;
    // Closing a sample pane retains its application data for Restore layout.
    for request in response
        .into_iter()
        .flat_map(DockspaceResponse::close_request_events)
        .filter(|request| !request.reused())
    {
        let plan = request.plan();
        for item in plan.items() {
            if item.state() == CloseItemDecisionState::Pending {
                changed |= dockspace
                    .resolve_close(plan.request(), item.token(), CloseDecision::Allow)?
                    .mutation()
                    .workspace_changed();
            }
        }
    }
    Ok(changed)
}

pub(super) fn demo_layout() -> DockspaceLayout {
    let top = DockspaceNode::split(
        DockspaceAxis::Horizontal,
        [
            (DockspaceNode::tabs([OUTLINE]), 0.24),
            (DockspaceNode::central_tabs([EDITOR, PREVIEW]), 0.76),
        ],
    )
    .expect("the static top split is valid");
    let bottom = DockspaceNode::split(
        DockspaceAxis::Horizontal,
        [
            (DockspaceNode::tabs([CONSOLE, HISTORY]), 0.65),
            (DockspaceNode::tabs([PROFILER, ASSETS]), 0.35),
        ],
    )
    .expect("the static bottom split is valid");
    let main = DockspaceNode::split(DockspaceAxis::Vertical, [(top, 0.7), (bottom, 0.3)])
        .expect("the editor rows are valid");
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(MAIN_ROOT, main),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs([INSPECTOR])),
        LogicalRect::new(940.0, 90.0, 300.0, 340.0).expect("the static Inspector bounds are valid"),
    ))])
    .expect("the workbench layout is valid")
}

pub(super) struct DemoPane {
    title: &'static str,
    pub(super) text: String,
    pub(super) content: Option<egui::Rect>,
    pub(super) editor: Option<egui::Rect>,
    pub(super) scroll_offset: f32,
    pub(super) scroll_bounds: Option<egui::Rect>,
}

pub(super) struct DemoPanes {
    pub(super) custom_tabs: bool,
    tab_accent: usize,
    pub(super) panes: BTreeMap<ItemId, DemoPane>,
    selection: usize,
    objects: [SceneObject; 3],
}

struct SceneObject {
    name: &'static str,
    position: [f32; 3],
    visible: bool,
}

const OBJECTS: [&str; 3] = ["Camera", "Light", "Player"];

impl DemoPanes {
    pub(super) fn new() -> Self {
        Self {
            custom_tabs: false,
            tab_accent: 0,
            panes: [
                (EDITOR, "Editor", "// src/main.rs\n\nfn main() {\n    let mut scene = Scene::new();\n    scene.add(\"Player\");\n    scene.run();\n}\n"),
                (PREVIEW, "Preview", ""),
                (OUTLINE, "Outline", ""),
                (INSPECTOR, "Inspector", "A place for scene notes."),
                (CONSOLE, "Console", ""),
                (HISTORY, "History", "Created scene\nAdded Player\nUpdated transform\nSaved scene"),
                (PROFILER, "Profiler", ""),
                (ASSETS, "Assets", "src/main.rs\nscenes/playground.scene\nmeshes/player.mesh\ntextures/grid.png"),
            ]
            .into_iter()
            .map(|(item, title, text)| (item, DemoPane {
                title,
                text: text.into(),
                content: None,
                editor: None,
                scroll_offset: 0.0,
                scroll_bounds: None,
            }))
            .collect(),
            selection: 2,
            objects: std::array::from_fn(|index| SceneObject {
                name: OBJECTS[index],
                position: [[-140.0, 60.0, 0.0], [140.0, 60.0, 0.0], [0.0, -60.0, 0.0]][index],
                visible: true,
            }),
        }
    }
}

impl PaneView for DemoPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        self.panes.get(&item).map(|pane| pane.title.into())
    }

    fn tab_content_width(&self, _item: ItemId, _ui: &egui::Ui, title: &egui::Galley) -> f32 {
        title.size().x + if self.custom_tabs { 18.0 } else { 0.0 }
    }

    fn tab_ui(&mut self, ui: &mut egui::Ui, tab: &Tab<'_>) {
        tabs::tab_ui(self, ui, tab);
    }

    fn tab_bar_layout(&self, _bar: TabBarId, _ui: &egui::Ui) -> TabBarLayout {
        if self.custom_tabs {
            TabBarLayout {
                leading_width: 24.0,
                trailing_width: 44.0,
            }
        } else {
            TabBarLayout::default()
        }
    }

    fn tab_bar_ui(&mut self, ui: &mut egui::Ui, bar: TabBar<'_>, slot: TabBarSlot) {
        tabs::tab_bar_ui(self, ui, bar, slot);
    }

    fn ui(&mut self, item: ItemId, ui: &mut egui::Ui) -> PaneResponse {
        let Some(pane) = self.panes.get_mut(&item) else {
            return PaneResponse::default();
        };
        pane.content = Some(ui.available_rect_before_wrap());
        pane.editor = None;
        let frame = Frame::new()
            .fill(ui.visuals().window_fill())
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                let mut content = PaneResponse::default();
                match item {
                    OUTLINE => {
                        content = content.with_content(&ui.strong("Playground"));
                        for (index, name) in OBJECTS.iter().enumerate() {
                            let row = ui.selectable_label(self.selection == index, *name);
                            if row.clicked() {
                                self.selection = index;
                            }
                            content = content.with_content(&row);
                        }
                    }
                    PREVIEW => {
                        let (rect, response) =
                            ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
                        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
                        let line = ui.visuals().widgets.noninteractive.bg_stroke;
                        for step in 0..=((rect.width().max(rect.height()) / 32.0) as usize) {
                            let offset = step as f32 * 32.0;
                            painter.line_segment(
                                [
                                    rect.left_top() + egui::vec2(offset, 0.0),
                                    rect.left_bottom() + egui::vec2(offset, 0.0),
                                ],
                                line,
                            );
                            painter.line_segment(
                                [
                                    rect.left_top() + egui::vec2(0.0, offset),
                                    rect.right_top() + egui::vec2(0.0, offset),
                                ],
                                line,
                            );
                        }
                        for (index, object) in self
                            .objects
                            .iter()
                            .enumerate()
                            .filter(|(_, object)| object.visible)
                        {
                            let center =
                                rect.center() + egui::vec2(object.position[0], -object.position[1]);
                            let bounds =
                                egui::Rect::from_center_size(center, egui::vec2(100.0, 70.0));
                            let fill = if index == self.selection {
                                Color32::from_rgb(48, 112, 172)
                            } else {
                                Color32::from_rgb(100, 120, 142)
                            };
                            painter.rect_filled(bounds, 8.0, fill);
                            painter.text(
                                center,
                                egui::Align2::CENTER_CENTER,
                                object.name,
                                FontId::proportional(16.0),
                                Color32::WHITE,
                            );
                        }
                        content = content.with_content(&response);
                    }
                    INSPECTOR => {
                        let object = &mut self.objects[self.selection];
                        content = content.with_content(&ui.strong(object.name));
                        content = content.with_content(&ui.separator());
                        content = content.with_content(&ui.label("Position"));
                        let row = ui.horizontal_wrapped(|ui| {
                            let mut fields = PaneResponse::default();
                            for (axis, value) in
                                ["X ", "Y ", "Z "].into_iter().zip(&mut object.position)
                            {
                                fields = fields.with_content(
                                    &ui.add(egui::DragValue::new(value).speed(1.0).prefix(axis)),
                                );
                            }
                            fields
                        });
                        content = content
                            .with_content_ids(row.inner.content_widgets())
                            .with_content(&row.response);
                        content = content
                            .with_content(&ui.checkbox(&mut object.visible, "Visible in Preview"));
                        content = content.with_content(&ui.label("Scene notes"));
                        content = content.with_content(&ui.text_edit_multiline(&mut pane.text));
                    }
                    CONSOLE => {
                        let log = egui::ScrollArea::vertical()
                            .id_salt("workbench-console")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let mut rows =
                                    PaneResponse::default().with_content_ids([ui.unique_id()]);
                                for index in 0..96 {
                                    let message = [
                                        "Loaded playground.scene",
                                        "Player added to scene",
                                        "Transform updated",
                                        "Scene saved",
                                    ][index % 4];
                                    rows =
                                        rows.with_content(&ui.monospace(format!(
                                            "10:42:{:02}  {message}",
                                            index % 60
                                        )));
                                }
                                rows
                            });
                        pane.scroll_offset = log.state.clamped_offset().y;
                        pane.scroll_bounds = Some(log.inner_rect);
                        content = log.inner;
                        for id in [log.id.with("area"), log.id.with(1_usize)] {
                            if ui.ctx().viewport(|v| v.this_pass.widgets.get(id).is_some()) {
                                content = content.with_content_ids([id]);
                            }
                        }
                    }
                    PROFILER => {
                        for (label, milliseconds) in
                            [("Simulation", 2.6), ("Rendering", 5.4), ("UI", 0.8)]
                        {
                            content = content.with_content(
                                &ui.add(
                                    egui::ProgressBar::new(milliseconds / 16.67)
                                        .text(format!("{label}   {milliseconds:.1} ms")),
                                ),
                            );
                        }
                        content = content.with_content(&ui.weak("Sample data"));
                    }
                    HISTORY | ASSETS => {
                        for line in pane.text.lines() {
                            content = content.with_content(&ui.monospace(line));
                        }
                    }
                    _ => {
                        let edit = ui.add_sized(
                            ui.available_size(),
                            egui::TextEdit::multiline(&mut pane.text)
                                .id(editor_id(item))
                                .code_editor()
                                .frame(Frame::new())
                                .desired_width(f32::INFINITY),
                        );
                        pane.editor = Some(edit.rect);
                        content = content.with_content(&edit);
                    }
                }
                content
            });
        frame.inner.with_content(&frame.response)
    }

    fn focus_target(&self, item: ItemId) -> Option<egui::Id> {
        (item == EDITOR).then(|| editor_id(item))
    }

    fn minimum_size(&self, _item: ItemId) -> Vec2 {
        egui::vec2(160.0, 100.0)
    }
}

fn editor_id(item: ItemId) -> egui::Id {
    egui::Id::unique(("workbench-editor", item))
}

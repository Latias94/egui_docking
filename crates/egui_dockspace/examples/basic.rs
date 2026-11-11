#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::BTreeMap;

use eframe::egui;
use egui_dockspace::{
    CloseDecision, CloseItemDecisionState, DockStyle, Dockspace, DockspaceAxis,
    DockspaceCapability, DockspaceContainedLayout, DockspaceLayout, DockspaceNode,
    DockspaceRootLayout, DockspaceSurfaceLayout, DockspaceUnavailableReason,
    FloatingPresentationId, ItemId, LogicalRect, PaneView, RootId, SurfaceId,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(1);
const FLOATING_ROOT: RootId = RootId::new(2);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);

const EDITOR: ItemId = ItemId::new(1);
const PREVIEW: ItemId = ItemId::new(2);
const OUTLINE: ItemId = ItemId::new(3);
const INSPECTOR: ItemId = ItemId::new(4);

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "egui_dockspace",
        options,
        Box::new(|creation| {
            creation.egui_ctx.set_theme(egui::Theme::Light);
            Ok(Box::new(DockspaceApp::new()))
        }),
    )
}

struct DockspaceApp {
    dockspace: Dockspace,
    panes: ExamplePanes,
    error: Option<String>,
    presentation_settlement: DockspaceCapability,
    font_size: f32,
    refresh_chrome: bool,
}

impl DockspaceApp {
    fn new() -> Self {
        let dockspace = Dockspace::builder("basic", example_layout())
            .build()
            .expect("the static example layout is valid");
        Self {
            dockspace,
            panes: ExamplePanes::new(),
            error: None,
            font_size: 13.0,
            refresh_chrome: true,
            presentation_settlement: DockspaceCapability::Unavailable(
                DockspaceUnavailableReason::PresentationSettlementRequired,
            ),
        }
    }
}

impl eframe::App for DockspaceApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.refresh_chrome {
            match self.dockspace.set_style(DockStyle::from_egui(ui)) {
                Ok(_) => self.refresh_chrome = false,
                Err(error) => self.error = Some(error.to_string()),
            }
        }
        if let Some(error) = &self.error {
            egui::Panel::bottom("dockspace-error").show(ui, |ui| {
                ui.colored_label(ui.visuals().error_fg_color, error);
            });
        }
        egui::Panel::top("dockspace-help").show(ui, |ui| {
            ui.horizontal(|ui| {
                egui::widgets::global_theme_preference_buttons(ui);
                ui.separator();
                if ui.add(egui::DragValue::new(&mut self.font_size).range(10.0..=36.0).prefix("Font ")).changed() {
                    ui.ctx().all_styles_mut(|style| {
                        style.override_font_id = Some(egui::FontId::proportional(self.font_size));
                    });
                    self.refresh_chrome = true;
                    ui.ctx().request_repaint();
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.strong("Pane rendering");
                ui.separator();
                match self.presentation_settlement {
                    DockspaceCapability::Supported => {
                        ui.small("Drag tabs into panes; use the dotted title grip to move Inspector.");
                    }
                    DockspaceCapability::Unavailable(_) => {
                        ui.small("Select and close tabs, or resize with the keyboard. For drag docking, run the workbench example.");
                    }
                }
            });
        });
        egui::CentralPanel::default().show(ui, |ui| {
            match self
                .dockspace
                .show_single_surface(SURFACE, ui, &mut self.panes)
            {
                Ok(response) => {
                    self.presentation_settlement = response.presentation_settlement_capability();
                    self.error = None;
                    for request in response.close_request_events() {
                        // Reused requests already went through the application decision path.
                        if request.reused() {
                            continue;
                        }
                        let plan = request.plan();
                        for item in plan.items() {
                            if item.state() != CloseItemDecisionState::Pending {
                                continue;
                            }
                            match self.dockspace.resolve_close(
                                plan.request(),
                                item.token(),
                                CloseDecision::Allow,
                            ) {
                                Ok(result) => {
                                    if result.mutation().workspace_changed() {
                                        ui.ctx().request_repaint();
                                    }
                                }
                                Err(error) => self.error = Some(error.to_string()),
                            }
                        }
                    }
                }
                Err(error) => self.error = Some(error.to_string()),
            }
        });
    }
}

fn example_layout() -> DockspaceLayout {
    let main = DockspaceNode::split(
        DockspaceAxis::Horizontal,
        [
            (DockspaceNode::tabs([OUTLINE]), 0.24),
            (DockspaceNode::central_tabs([EDITOR, PREVIEW]), 0.76),
        ],
    )
    .expect("the static split is valid");
    let contained = DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs([INSPECTOR])),
        LogicalRect::new(690.0, 90.0, 310.0, 300.0).expect("the static rect is valid"),
    );
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(MAIN_ROOT, main),
    )
    .with_contained(contained)])
    .expect("the example layout is valid")
}

struct ExamplePane {
    title: &'static str,
    text: String,
}

struct ExamplePanes {
    panes: BTreeMap<ItemId, ExamplePane>,
}

impl ExamplePanes {
    fn new() -> Self {
        Self {
            panes: BTreeMap::from([
                (
                    EDITOR,
                    ExamplePane {
                        title: "Editor",
                        text: "fn main() {\n    println!(\"dockspace\");\n}".to_owned(),
                    },
                ),
                (
                    PREVIEW,
                    ExamplePane {
                        title: "Preview",
                        text: "Rendered output".to_owned(),
                    },
                ),
                (
                    OUTLINE,
                    ExamplePane {
                        title: "Outline",
                        text: "main\n  editor\n  preview".to_owned(),
                    },
                ),
                (
                    INSPECTOR,
                    ExamplePane {
                        title: "Inspector",
                        text: "Selection: Editor".to_owned(),
                    },
                ),
            ]),
        }
    }
}

impl PaneView for ExamplePanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        self.panes.get(&item).map(|pane| pane.title.into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut egui::Ui) -> egui_dockspace::PaneResponse {
        let Some(pane) = self.panes.get_mut(&item) else {
            return egui_dockspace::PaneResponse::default();
        };
        let response = ui.add(
            egui::TextEdit::multiline(&mut pane.text)
                .id(egui::Id::unique(("basic-pane-editor", item)))
                .code_editor()
                .desired_width(f32::INFINITY),
        );
        egui_dockspace::PaneResponse::from(&response)
    }

    fn focus_target(&self, item: ItemId) -> Option<egui::Id> {
        self.panes
            .contains_key(&item)
            .then(|| egui::Id::unique(("basic-pane-editor", item)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::accesskit::{Action, ActionRequest, Role, TreeId};

    fn run_frame(
        context: &egui::Context,
        app: &mut DockspaceApp,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let mut frame = eframe::Frame::_new_kittest();
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 720.0),
                )),
                events,
                ..Default::default()
            },
            |ui| eframe::App::ui(app, ui, &mut frame),
        );
        output.textures_delta.clear();
        assert!(app.error.is_none(), "example frame failed: {:?}", app.error);
        output
    }

    #[test]
    fn close_button_allows_and_removes_the_pane() {
        let context = egui::Context::default();
        context.enable_accesskit();
        let mut app = DockspaceApp::new();
        let _ = run_frame(&context, &mut app, Vec::new());
        let ready = run_frame(&context, &mut app, Vec::new());
        let close = ready
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("the example exposes its controls")
            .nodes
            .iter()
            .find_map(|(id, node)| {
                (node.role() == Role::Button
                    && node.label() == Some("Close Editor")
                    && node.supports_action(Action::Click))
                .then_some(*id)
            })
            .expect("the editor has an enabled close button");
        assert!(app.dockspace.view().item(EDITOR).is_some());

        let _ = run_frame(
            &context,
            &mut app,
            vec![egui::Event::AccessKitActionRequest(ActionRequest {
                action: Action::Click,
                target_tree: TreeId::ROOT,
                target_node: close,
                data: None,
            })],
        );

        // Current-pass chrome actions enter the next update before encoding.
        assert!(app.dockspace.view().item(EDITOR).is_some());
        let _ = run_frame(&context, &mut app, Vec::new());
        assert!(app.dockspace.view().item(EDITOR).is_none());
        assert!(app.dockspace.view().item(PREVIEW).is_some());
        let closed_version = app.dockspace.version();
        let _ = run_frame(&context, &mut app, Vec::new());
        assert_eq!(app.dockspace.version(), closed_version);
    }
}

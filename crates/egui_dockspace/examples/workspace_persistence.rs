#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::BTreeMap;

use eframe::egui;
use egui_dockspace::{
    CloseDecision, CloseItemDecisionState, Dockspace, DockspaceAxis, DockspaceDocumentBootstrap,
    DockspaceDocumentId, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, ItemId, PaneView, RootId, SurfaceId,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const PERSISTENCE_DOCUMENT_ID: DockspaceDocumentId =
    DockspaceDocumentId::from_bytes(*b"egui-example-doc");

fn main() -> eframe::Result {
    eframe::run_native(
        "egui_dockspace persistence",
        eframe::NativeOptions::default(),
        Box::new(|_| Ok(Box::new(PersistenceApp::new()))),
    )
}

struct PersistenceApp {
    dockspace: Dockspace,
    panes: PersistencePanes,
    saved: Option<Vec<u8>>,
    status: String,
}

impl PersistenceApp {
    fn new() -> Self {
        let mut bootstrap = DockspaceDocumentBootstrap::new(PERSISTENCE_DOCUMENT_ID);
        let document = bootstrap
            .ensure_item("pane/document")
            .expect("the document identity must fit");
        let console = bootstrap
            .ensure_item("pane/console")
            .expect("the console identity must fit");
        let dockspace = Dockspace::builder("persistence", example_layout(document, console))
            .persistence(bootstrap)
            .build()
            .expect("the static example layout is valid");
        Self {
            dockspace,
            panes: PersistencePanes::new(document, console),
            saved: None,
            status: "No snapshot".to_owned(),
        }
    }

    fn save(&mut self) {
        match self.dockspace.save_document_json() {
            Ok(snapshot) => {
                self.saved = Some(snapshot);
                "Snapshot saved".clone_into(&mut self.status);
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    fn restore(&mut self) {
        let Some(snapshot) = self.saved.as_deref() else {
            "No snapshot".clone_into(&mut self.status);
            return;
        };
        match self
            .dockspace
            .restore_document_json(snapshot, |document_id, key| {
                document_id == PERSISTENCE_DOCUMENT_ID
                    && matches!(key, "pane/document" | "pane/console")
            }) {
            Ok(_) => "Snapshot restored".clone_into(&mut self.status),
            Err(error) => self.status = error.to_string(),
        }
    }
}

impl eframe::App for PersistenceApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("snapshot-controls").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Save snapshot").clicked() {
                    self.save();
                }
                if ui
                    .add_enabled(self.saved.is_some(), egui::Button::new("Restore snapshot"))
                    .clicked()
                {
                    self.restore();
                }
                ui.label(&self.status);
                ui.separator();
                ui.weak("This unmodified eframe example focuses on persistence; preview-qualified drag is intentionally unavailable.");
            });
        });
        egui::CentralPanel::default().show(ui, |ui| {
            match self
                .dockspace
                .show_single_surface(SURFACE, ui, &mut self.panes)
            {
                Ok(response) => {
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
                            // Keep application pane data so a snapshot can reopen the item.
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
                                Err(error) => self.status = error.to_string(),
                            }
                        }
                    }
                }
                Err(error) => self.status = error.to_string(),
            }
        });
    }
}

fn example_layout(document_item: ItemId, console_item: ItemId) -> DockspaceLayout {
    let root = DockspaceNode::split(
        DockspaceAxis::Vertical,
        [
            (DockspaceNode::central_tabs([document_item]), 0.72),
            (DockspaceNode::tabs([console_item]), 0.28),
        ],
    )
    .expect("the static split is valid");
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, root),
    )])
    .expect("the example layout is valid")
}

struct PersistencePanes {
    items: BTreeMap<ItemId, (&'static str, String)>,
}

impl PersistencePanes {
    fn new(document: ItemId, console: ItemId) -> Self {
        Self {
            items: BTreeMap::from([
                (
                    document,
                    ("Document", "Persistent document state".to_owned()),
                ),
                (console, ("Console", "Ready".to_owned())),
            ]),
        }
    }
}

impl PaneView for PersistencePanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        self.items.get(&item).map(|(title, _)| (*title).into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut egui::Ui) {
        if let Some((_, text)) = self.items.get_mut(&item) {
            ui.add(egui::TextEdit::multiline(text).desired_width(f32::INFINITY));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::accesskit::{Action, ActionRequest, Role, TreeId};

    fn run_frame(
        context: &egui::Context,
        app: &mut PersistenceApp,
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
        output
    }

    #[test]
    fn close_button_preserves_pane_data_for_snapshot_restore() {
        let context = egui::Context::default();
        context.enable_accesskit();
        let mut app = PersistenceApp::new();
        let document = app
            .dockspace
            .item_id_for_external_key("pane/document")
            .expect("the document has a persistent identity");
        let text = "Unsaved application text survives closing and layout restore";
        text.clone_into(&mut app.panes.items.get_mut(&document).expect("pane data").1);
        app.save();
        assert_eq!(app.status, "Snapshot saved");
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
                    && node.label() == Some("Close Document")
                    && node.supports_action(Action::Click))
                .then_some(*id)
            })
            .expect("the document has an enabled close button");
        assert!(app.dockspace.view().item(document).is_some());

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

        assert!(app.dockspace.view().item(document).is_none());
        assert_eq!(
            app.panes.items.get(&document).expect("closed pane data").1,
            text
        );
        app.restore();
        assert_eq!(app.status, "Snapshot restored");
        assert!(app.dockspace.view().item(document).is_some());
        assert_eq!(
            app.panes
                .items
                .get(&document)
                .expect("restored pane data")
                .1,
            text
        );
        let restored = run_frame(&context, &mut app, Vec::new());
        assert_eq!(app.status, "Snapshot restored");
        assert!(
            restored
                .platform_output
                .accesskit_update
                .as_ref()
                .expect("the restored example exposes its panes")
                .nodes
                .iter()
                .any(|(_, node)| node.role() == Role::Tab && node.label() == Some("Document")),
            "the restored layout renders the retained application pane"
        );
    }
}

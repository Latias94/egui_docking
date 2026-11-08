use dockspace::model::{
    DockAnchor, DockPlacement, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::DockspaceSession;
use eframe::egui;
use egui_dockspace::{DockStyle, PaneResponse, PaneView};
use egui_dockspace_native::{NativeActionRequestError, NativeCommand, NativeDockspaceApp};

const SURFACE: SurfaceId = SurfaceId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);

struct Panes(String);

impl PaneView for Panes {
    fn title(&self, _item: ItemId) -> Option<egui::WidgetText> {
        Some("Command consumer".into())
    }

    fn ui(&mut self, _item: ItemId, ui: &mut egui::Ui) -> PaneResponse {
        PaneResponse::from(&ui.text_edit_multiline(&mut self.0))
    }
}

#[test]
fn a_stored_command_shares_bounded_admission_without_mutating_application_data() {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([FIRST, SECOND])),
    )])
    .unwrap();
    let session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let app = NativeDockspaceApp::new(
        egui::Id::unique("public-native-command-consumer"),
        session,
        SURFACE,
        Panes("unsaved application text".to_owned()),
        DockStyle::default(),
    )
    .unwrap();
    let command = NativeCommand::FloatItem {
        item: FIRST,
        surface: SURFACE,
    };

    app.request_command(command).unwrap();
    assert_eq!(
        app.request_command(command),
        Err(NativeActionRequestError::Busy)
    );
    assert_eq!(
        app.request_command(NativeCommand::DockItem {
            item: FIRST,
            placement: DockPlacement::Center(DockAnchor::Item(SECOND)),
        }),
        Err(NativeActionRequestError::Busy)
    );
    assert!(app.take_action_status().is_none());
    assert_eq!(
        app.with_panes(|panes| panes.0.clone()),
        "unsaved application text"
    );
    assert_eq!(
        app.with_view(|view| view.item(FIRST).unwrap().root()),
        RootId::new(1)
    );
}

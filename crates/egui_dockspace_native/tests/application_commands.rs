use dockspace::geometry::PhysicalRect;
use dockspace::model::{
    DockAnchor, DockPlacement, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, ItemId, NativeTearOffSource, NativeWindowPlacement, RootId, SurfaceId,
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

fn app() -> NativeDockspaceApp<Panes> {
    app_with_panes(Panes("unsaved application text".to_owned()))
}

fn app_with_panes<P: PaneView + Send + 'static>(panes: P) -> NativeDockspaceApp<P> {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([FIRST, SECOND])),
    )])
    .unwrap();
    let session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    NativeDockspaceApp::new(
        egui::Id::unique("public-native-command-consumer"),
        session,
        SURFACE,
        panes,
        DockStyle::default(),
    )
    .unwrap()
}

#[test]
fn boxed_dynamic_provider_works_with_the_public_native_owner() {
    let panes: Box<dyn PaneView + Send> = Box::new(Panes("dynamic provider".to_owned()));
    let app = app_with_panes(panes);
    assert_eq!(
        app.with_panes(|panes| panes.title(FIRST).unwrap().text().to_owned()),
        "Command consumer"
    );
    app.request_command(NativeCommand::SelectItem { item: SECOND })
        .unwrap();
    assert_eq!(
        app.request_command(NativeCommand::SelectItem { item: FIRST })
            .map_err(|error| error.reason()),
        Err(NativeActionRequestError::Busy)
    );
}

#[test]
fn a_stored_command_shares_bounded_admission_without_mutating_application_data() {
    use std::error::Error;

    let app = app();
    let command = NativeCommand::FloatItem {
        item: FIRST,
        surface: SURFACE,
    };

    app.request_command(command.clone()).unwrap();
    let error = app.request_command(command.clone()).unwrap_err();
    assert_eq!(error.reason(), NativeActionRequestError::Busy);
    assert_eq!(
        error
            .source()
            .unwrap()
            .downcast_ref::<NativeActionRequestError>(),
        Some(&NativeActionRequestError::Busy)
    );
    assert_eq!(error.into_command(), command);
    assert_eq!(
        app.request_command(NativeCommand::DockItem {
            item: FIRST,
            placement: DockPlacement::Center(DockAnchor::Item(SECOND)),
        })
        .map_err(|error| error.reason()),
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

#[test]
fn stored_native_payload_commands_and_group_capture_share_the_public_slot() {
    let placement =
        NativeWindowPlacement::new(PhysicalRect::new(900.0, 100.0, 480.0, 320.0).unwrap());
    for kind in 0..6 {
        let app = app();
        let group = app.capture_tabs_group(DockAnchor::Item(FIRST)).unwrap();
        let source = match kind {
            0 | 3 => NativeTearOffSource::Item(SECOND),
            1 | 4 => NativeTearOffSource::Tabs(group.clone()),
            _ => NativeTearOffSource::Root(RootId::new(1)),
        };
        let command = match kind {
            0..=2 => NativeCommand::MoveToNewWindow { source, placement },
            3 | 4 => NativeCommand::MoveToNewWindowDefault { source },
            _ => NativeCommand::MoveRootToNewWindow {
                root: RootId::new(1),
                placement,
            },
        };
        app.request_command(command.clone()).unwrap();
        assert_eq!(
            app.request_command(command).map_err(|error| error.reason()),
            Err(NativeActionRequestError::Busy)
        );
        assert_eq!(
            app.request_command(NativeCommand::SelectItem { item: FIRST })
                .map_err(|error| error.reason()),
            Err(NativeActionRequestError::Busy)
        );
        assert_eq!(
            app.capture_tabs_group(DockAnchor::Item(SECOND)).unwrap(),
            group
        );
        assert!(
            app.capture_tabs_group(DockAnchor::Item(ItemId::new(99)))
                .is_err()
        );
        assert!(app.take_action_status().is_none());
        assert_eq!(
            app.with_view(|view| view.item(FIRST).unwrap().tabs().items().to_vec()),
            [FIRST, SECOND]
        );
        assert_eq!(
            app.with_view(|view| view.item(SECOND).unwrap().surface()),
            SURFACE
        );
        assert_eq!(
            app.with_panes(|panes| panes.0.clone()),
            "unsaved application text"
        );
    }
}

#[test]
fn another_apps_captured_group_uses_the_same_public_command_slot() {
    let source = app();
    let destination = app();
    let group = source.capture_tabs_group(DockAnchor::Item(FIRST)).unwrap();
    let original = group.clone();
    destination
        .request_command(NativeCommand::MoveToNewWindowDefault {
            source: NativeTearOffSource::Tabs(group),
        })
        .unwrap();
    assert_eq!(
        destination
            .request_command(NativeCommand::SelectItem { item: SECOND })
            .map_err(|error| error.reason()),
        Err(NativeActionRequestError::Busy)
    );
    assert_eq!(
        source.capture_tabs_group(DockAnchor::Item(FIRST)).unwrap(),
        original
    );
    assert_eq!(
        destination.with_view(|view| view.item(FIRST).unwrap().surface()),
        SURFACE
    );
    assert!(destination.take_action_status().is_none());
    assert!(destination.error_kind().is_none());
}

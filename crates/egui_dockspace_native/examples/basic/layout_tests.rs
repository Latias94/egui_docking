use super::*;

fn app(layout: DockspaceLayout, host: SurfaceId) -> NativeExampleApp {
    let session = DockspaceSession::from_layout(layout.clone(), DockPolicy::default()).unwrap();
    NativeExampleApp {
        dockspace: NativeDockspaceApp::new(
            egui::Id::unique("pane-workflow-test"),
            session,
            host,
            ExamplePanes::new(),
            DockStyle::default(),
        )
        .unwrap(),
        default_layout: layout,
        #[cfg(feature = "serde")]
        presets: BTreeMap::new(),
        #[cfg(feature = "serde")]
        preset_name: "Working".to_owned(),
        action: ExampleActionState::Idle,
        status: None,
        smoke: None,
        docking_smoke: None,
        command_smoke: None,
    }
}

#[test]
fn reopen_without_document_uses_the_current_host_after_rebinding() {
    let host = SurfaceId::new(1_000);
    for (content, expected) in [
        (
            Some(DockspaceNode::tabs([OUTLINE, ASSETS])),
            DockPlacement::Center(DockAnchor::Item(OUTLINE)),
        ),
        (
            Some(DockspaceNode::central_tabs([OUTLINE, ASSETS])),
            DockPlacement::Center(DockAnchor::Central(MAIN_ROOT)),
        ),
        (None, DockPlacement::Main(host)),
    ] {
        let surface = match content {
            Some(content) => {
                DockspaceSurfaceLayout::new(host, DockspaceRootLayout::new(MAIN_ROOT, content))
            }
            None => DockspaceSurfaceLayout::rootless(host).with_contained(
                DockspaceContainedLayout::new(
                    INSPECTOR_FLOATING,
                    DockspaceRootLayout::new(INSPECTOR_ROOT, DockspaceNode::tabs([ASSETS])),
                    LogicalRect::new(100.0, 80.0, 360.0, 300.0).unwrap(),
                ),
            ),
        };
        let mut app = app(DockspaceLayout::new([surface]).unwrap(), host);
        assert_eq!(
            app.dockspace
                .with_view(|view| main_dock_placement(view, host, None)),
            Some(expected)
        );
        app.reopen_pane(INSPECTOR);
        assert!(
            app.action.is_pending(),
            "the inventory must admit reopening on the current host: {:?}",
            app.status
        );
        assert!(
            app.dockspace
                .with_view(|view| view.item(INSPECTOR).is_none())
        );
        assert!(
            app.dockspace
                .with_panes(|panes| panes.panes[&INSPECTOR].loaded)
        );
    }
}

#[test]
fn main_docking_skips_the_moving_pane_and_finds_the_next_split_leaf() {
    let host = SurfaceId::new(1_000);
    let content = DockspaceNode::split(
        DockspaceAxis::Horizontal,
        [
            (DockspaceNode::tabs([OUTLINE]), 0.5),
            (DockspaceNode::tabs([ASSETS]), 0.5),
        ],
    )
    .unwrap();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        host,
        DockspaceRootLayout::new(MAIN_ROOT, content),
    )])
    .unwrap();
    let app = app(layout, host);
    assert_eq!(
        app.dockspace
            .with_view(|view| main_dock_placement(view, host, Some(OUTLINE))),
        Some(DockPlacement::Center(DockAnchor::Item(ASSETS)))
    );
}

fn move_controls(
    app: &mut NativeExampleApp,
    context: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            events,
            ..Default::default()
        },
        |ui| app.show_pane_move_commands(ui, INSPECTOR),
    );
    output.textures_delta.clear();
    output
}

#[test]
fn the_inventory_can_float_one_pane_out_of_a_contained_group() {
    let mut app = app(example_layout(), SURFACE);
    let context = egui::Context::default();
    context.enable_accesskit();
    let output = move_controls(&mut app, &context, Vec::new());
    let (id, button) = output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some("Float in main window"))
        .expect("the pane's Move menu exposes floating");
    assert!(
        !button.is_disabled(),
        "Inspector can leave the two-pane Inspector/Preview contained group"
    );
    let click = egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
        action: egui::accesskit::Action::Click,
        target_tree: egui::accesskit::TreeId::ROOT,
        target_node: *id,
        data: None,
    });
    move_controls(&mut app, &context, vec![click]);
    assert!(
        app.action.is_pending(),
        "the actual button must submit the command"
    );
    assert!(app.dockspace.take_action_status().is_none());
}

#[test]
fn a_standalone_contained_pane_keeps_group_recovery_controls() {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(MAIN_ROOT, DockspaceNode::central_tabs([EDITOR])),
    )
    .with_contained(DockspaceContainedLayout::new(
        INSPECTOR_FLOATING,
        DockspaceRootLayout::new(INSPECTOR_ROOT, DockspaceNode::tabs([INSPECTOR])),
        LogicalRect::new(100.0, 80.0, 360.0, 300.0).unwrap(),
    ))])
    .unwrap();
    let mut app = app(layout, SURFACE);
    let context = egui::Context::default();
    context.enable_accesskit();
    let output = move_controls(&mut app, &context, Vec::new());
    let nodes = &output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes;
    for (label, disabled) in [
        ("Float in main window", true),
        ("Bring group to front", false),
        ("Move group into view", false),
    ] {
        let (_, button) = nodes
            .iter()
            .find(|(_, node)| node.label() == Some(label))
            .unwrap();
        assert_eq!(button.is_disabled(), disabled, "{label}");
    }
}

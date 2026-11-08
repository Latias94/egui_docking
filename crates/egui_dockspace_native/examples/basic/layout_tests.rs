use super::*;

fn app(layout: DockspaceLayout, host: SurfaceId) -> NativeExampleApp {
    let session = DockspaceSession::from_layout(layout.clone(), DockPolicy::default()).unwrap();
    app_with_session(layout, host, session)
}

fn app_with_session(
    layout: DockspaceLayout,
    host: SurfaceId,
    session: DockspaceSession,
) -> NativeExampleApp {
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

fn close_app(deferred: bool) -> NativeExampleApp {
    let layout = example_layout();
    let mut policy = DockPolicy::default();
    policy.set_close_capability(CloseCapability::DeferredAllowed);
    let mut session = DockspaceSession::from_layout(layout.clone(), policy).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame.request_close_root_current(INSPECTOR_ROOT).unwrap();
    frame
        .complete_unpainted_surfaces(dockspace::runtime::SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    if deferred {
        let plan = session.pending_content_close_plans().next().unwrap();
        let item = plan
            .items()
            .iter()
            .find(|item| item.item() == INSPECTOR)
            .unwrap();
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .resolve_close(plan.request(), item.token(), CloseDecision::Deferred)
            .unwrap();
        frame
            .complete_unpainted_surfaces(dockspace::runtime::SurfaceUnavailableReason::Deferred)
            .unwrap();
        frame.commit().unwrap();
    }
    app_with_session(layout, SURFACE, session)
}

fn close_controls(
    app: &mut NativeExampleApp,
    context: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 600.0),
            )),
            events,
            ..Default::default()
        },
        |ui| app.show_close_decisions(ui),
    );
    output.textures_delta.clear();
    output
}

fn button_click(output: &egui::FullOutput, label: &str) -> egui::Event {
    let (id, button) = output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find(|(_, node)| {
            node.role() == egui::accesskit::Role::Button && node.label() == Some(label)
        })
        .unwrap_or_else(|| panic!("missing enabled button: {label}"));
    assert!(!button.is_disabled(), "{label}");
    egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
        action: egui::accesskit::Action::Click,
        target_tree: egui::accesskit::TreeId::ROOT,
        target_node: *id,
        data: None,
    })
}

#[test]
fn close_confirmations_identify_each_pane_and_disable_actions_while_pending() {
    let mut app = close_app(false);
    let context = egui::Context::default();
    context.enable_accesskit();
    let output = close_controls(&mut app, &context, Vec::new());
    for pane in ["Preview", "Inspector"] {
        for choice in ["Close", "Keep open", "Decide later"] {
            button_click(&output, &format!("{pane}: {choice}"));
        }
    }
    let postpone = button_click(&output, "Inspector: Decide later");
    close_controls(&mut app, &context, vec![postpone]);
    assert!(app.action.is_pending());
    let output = close_controls(&mut app, &context, Vec::new());
    let controls = output.platform_output.accesskit_update.as_ref().unwrap();
    let buttons = controls
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == egui::accesskit::Role::Button)
        .filter(|(_, node)| node.label().is_some_and(|label| label.contains(": ")))
        .collect::<Vec<_>>();
    assert_eq!(buttons.len(), 6);
    assert!(buttons.iter().all(|(_, node)| node.is_disabled()));
}

#[test]
fn deferred_confirmation_reuses_close_and_keep_open_without_another_postpone() {
    let mut app = close_app(true);
    let context = egui::Context::default();
    context.enable_accesskit();
    let output = close_controls(&mut app, &context, Vec::new());
    button_click(&output, "Inspector: Close");
    let keep = button_click(&output, "Inspector: Keep open");
    assert!(
        output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .all(|(_, node)| node.label() != Some("Inspector: Decide later"))
    );
    button_click(&output, "Preview: Decide later");
    close_controls(&mut app, &context, vec![keep]);
    assert!(app.action.is_pending());
}

fn inventory_controls(
    app: &mut NativeExampleApp,
    context: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(440.0, 620.0),
            )),
            events,
            ..Default::default()
        },
        |ui| app.show_layout_controls(ui),
    );
    output.textures_delta.clear();
    output
}

fn open_inventory_pane(
    app: &mut NativeExampleApp,
    context: &egui::Context,
    pane: &str,
) -> egui::FullOutput {
    let output = inventory_controls(app, context, Vec::new());
    let point = inventory_point(&output, "Pane inventory (8)");
    inventory_controls(
        app,
        context,
        vec![
            egui::Event::PointerMoved(point),
            egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    inventory_controls(
        app,
        context,
        vec![egui::Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    let output = inventory_controls(app, context, Vec::new());
    let pointer = inventory_point(&output, pane);
    inventory_controls(app, context, vec![egui::Event::PointerMoved(pointer)]);
    inventory_controls(app, context, Vec::new())
}

#[test]
fn narrow_inventory_keeps_content_actions_visible_and_targets_the_named_pane() {
    let mut app = app(example_layout(), SURFACE);
    let context = egui::Context::default();
    context.enable_accesskit();
    let output = open_inventory_pane(&mut app, &context, "Inspector — selected");
    let actions = output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .filter(|(_, node)| node.label() == Some("Unload content"))
        .collect::<Vec<_>>();
    assert!(!actions.is_empty(), "Inspector exposes its content action");
    for (_, action) in actions {
        let bounds = action.bounds().unwrap();
        assert!(
            bounds.x0 >= 0.0 && bounds.x1 <= 440.0 && bounds.y0 >= 0.0 && bounds.y1 <= 620.0,
            "the complete content action must fit the window: {bounds:?}"
        );
    }
    let unload = button_click(&output, "Unload content");
    inventory_controls(&mut app, &context, vec![unload]);
    app.dockspace.with_panes(|panes| {
        assert!(!panes.panes[&INSPECTOR].loaded);
        assert!(
            panes
                .panes
                .iter()
                .all(|(&item, pane)| item == INSPECTOR || pane.loaded)
        );
    });
}

#[test]
fn inventory_keeps_selection_and_content_available_only_when_controls_allow_it() {
    for (locked, pending) in [(false, false), (false, true), (true, false), (true, true)] {
        let layout = example_layout();
        let mut policy = DockPolicy::default();
        policy.set_arrangement_locked(locked);
        let session = DockspaceSession::from_layout(layout.clone(), policy).unwrap();
        let mut app = app_with_session(layout, SURFACE, session);
        if pending {
            let request = app
                .dockspace
                .request_command(NativeCommand::SelectItem { item: EDITOR });
            app.begin_layout_action("Select pane", request);
            assert!(app.action.is_pending());
        }
        let context = egui::Context::default();
        context.enable_accesskit();
        let output = open_inventory_pane(&mut app, &context, "Inspector — selected");
        for (label, disabled) in [
            ("Select", pending),
            ("Unload content", pending),
            ("Dock in main area", locked || pending),
            ("Float in main window", locked || pending),
            ("Bring group to front", locked || pending),
            ("Move group into view", locked || pending),
            ("Close", locked || pending),
            ("Close group", locked || pending),
        ] {
            let node = output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label))
                .unwrap_or_else(|| panic!("missing inventory action {label}"));
            assert_eq!(
                node.1.is_disabled(),
                disabled,
                "{label}, locked={locked}, pending={pending}"
            );
        }
    }
}

fn inventory_point(output: &egui::FullOutput, text: &str) -> egui::Pos2 {
    let bounds = output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find(|(_, node)| node.label() == Some(text) || node.value() == Some(text))
        .unwrap_or_else(|| panic!("the inventory exposes {text}"))
        .1
        .bounds()
        .unwrap();
    egui::pos2(
        ((bounds.x0 + bounds.x1) / 2.0) as f32,
        ((bounds.y0 + bounds.y1) / 2.0) as f32,
    )
}

#[test]
fn reloading_pane_content_hands_keyboard_focus_to_its_editor() {
    for activation in [egui::Key::Enter, egui::Key::Space] {
        let context = egui::Context::default();
        let mut panes = ExamplePanes::new();
        let retained = panes.panes[&EDITOR].text.clone();
        panes.panes.get_mut(&EDITOR).unwrap().set_loaded(false);
        let paint = |panes: &mut ExamplePanes, events| {
            let mut output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    panes.ui(EDITOR, ui);
                },
            );
            output.textures_delta.clear();
        };
        let key = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        paint(&mut panes, Vec::new());
        paint(&mut panes, vec![key(egui::Key::Tab)]);
        paint(&mut panes, vec![key(activation)]);
        assert!(panes.panes[&EDITOR].loaded, "Reload content was activated");
        paint(&mut panes, Vec::new());
        assert!(
            context.memory(|memory| memory.has_focus(pane_focus_id(EDITOR))),
            "{activation:?} on Reload content must focus the restored editor"
        );
        paint(
            &mut panes,
            vec![egui::Event::Text("~AFTER_RELOAD~".to_owned())],
        );
        assert!(panes.panes[&EDITOR].loaded);
        assert!(panes.panes[&EDITOR].text.contains(&retained));
        assert!(panes.panes[&EDITOR].text.contains("~AFTER_RELOAD~"));
        assert_eq!(
            panes.panes[&INSPECTOR].text,
            ExamplePanes::new().panes[&INSPECTOR].text,
        );
    }
}

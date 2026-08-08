use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, RootId, SurfaceId};
use egui::accesskit::{Action, ActionRequest, Role, TreeId};
use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui, vec2};
use egui_dockspace::{Dockspace, PaneView};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);

struct Panes;

impl PaneView for Panes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        match item {
            FIRST => Some("First".into()),
            SECOND => Some("Second".into()),
            _ => None,
        }
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) {
        ui.label(format!("Pane {}", item.get()));
    }
}

fn workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([FIRST, SECOND]));
    builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.build().expect("the interaction workspace is valid")
}

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
        events,
        ..RawInput::default()
    }
}

fn run_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut Panes,
    events: Vec<Event>,
) -> FrameObservation {
    let mut close_items = Vec::new();
    let mut local_actions_current = false;
    let mut output = context.run_ui(input(events), |ui| {
        let response = dockspace
            .show_single_surface(SURFACE, ui, panes)
            .expect("the official-egui frame advances");
        local_actions_current = response
            .interaction_capabilities()
            .local_actions_current();
        close_items.extend(
            response
                .close_requests()
                .flat_map(|plan| plan.items().iter().map(|item| item.item())),
        );
    });
    output.textures_delta.clear();
    FrameObservation {
        output,
        close_items,
        local_actions_current,
    }
}

fn run_frame_with_discard_after_dockspace(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut Panes,
    events: Vec<Event>,
) -> (FrameObservation, usize) {
    let mut close_items = Vec::new();
    let mut local_actions_current = false;
    let mut passes = 0;
    let mut output = context.run_ui(input(events), |ui| {
        let response = dockspace
            .show_single_surface(SURFACE, ui, panes)
            .expect("the official-egui multipass frame advances");
        local_actions_current = response
            .interaction_capabilities()
            .local_actions_current();
        close_items.extend(
            response
                .close_requests()
                .flat_map(|plan| plan.items().iter().map(|item| item.item())),
        );
        passes += 1;
        if ui.ctx().current_pass_index() == 0 {
            ui.ctx()
                .request_discard("exercise local response action retention");
        }
    });
    output.textures_delta.clear();
    (
        FrameObservation {
            output,
            close_items,
            local_actions_current,
        },
        passes,
    )
}

struct FrameObservation {
    output: egui::FullOutput,
    close_items: Vec<ItemId>,
    local_actions_current: bool,
}

fn selected_item(dockspace: &Dockspace) -> Option<ItemId> {
    let root = dockspace.workspace().root(ROOT)?;
    match dockspace.workspace().node(root.node)? {
        Node::Tabs { selected, .. } => *selected,
        Node::Split { .. } => None,
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "finite AccessKit fixture geometry is converted to egui input coordinates"
)]
fn accesskit_node<'a>(
    output: &'a egui::FullOutput,
    role: Role,
    label: &str,
) -> (egui::accesskit::NodeId, &'a egui::accesskit::Node) {
    let tree = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit output is enabled");
    tree
        .nodes
        .iter()
        .find_map(|(id, node)| {
            (node.role() == role && node.label() == Some(label)).then_some((*id, node))
        })
        .expect("the requested node is present in the accessibility tree")
}

fn node_center(node: &egui::accesskit::Node) -> Pos2 {
    let bounds = node.bounds().expect("interactive tabs expose bounds");
    Pos2::new(
        ((bounds.x0 + bounds.x1) * 0.5) as f32,
        ((bounds.y0 + bounds.y1) * 0.5) as f32,
    )
}

#[test]
fn production_single_surface_click_selects_a_tab() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("official-egui-interaction", workspace())
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(stable.local_actions_current);
    let pointer = node_center(accesskit_node(&stable.output, Role::Tab, "Second").1);

    let press = vec![
        Event::PointerMoved(pointer),
        Event::PointerButton {
            pos: pointer,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
    ];
    let _ = run_frame(&context, &mut dockspace, &mut panes, press);
    let release = vec![
        Event::PointerMoved(pointer),
        Event::PointerButton {
            pos: pointer,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        },
    ];
    let _ = run_frame(&context, &mut dockspace, &mut panes, release);

    assert_eq!(selected_item(&dockspace), Some(SECOND));
}

#[test]
fn production_single_surface_same_batch_click_selects_a_tab() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("official-egui-batched-click", workspace())
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let pointer = node_center(accesskit_node(&stable.output, Role::Tab, "Second").1);
    let click = vec![
        Event::PointerMoved(pointer),
        Event::PointerButton {
            pos: pointer,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
        Event::PointerButton {
            pos: pointer,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        },
    ];
    let _ = run_frame(&context, &mut dockspace, &mut panes, click);

    assert_eq!(selected_item(&dockspace), Some(SECOND));
}

#[test]
fn production_single_surface_preserves_a_click_across_discard_passes() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let mut dockspace = Dockspace::builder("official-egui-multipass-click", workspace())
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let pointer = node_center(accesskit_node(&stable.output, Role::Tab, "Second").1);
    let click = vec![
        Event::PointerMoved(pointer),
        Event::PointerButton {
            pos: pointer,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
        Event::PointerButton {
            pos: pointer,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        },
    ];
    let (_, passes) = run_frame_with_discard_after_dockspace(
        &context,
        &mut dockspace,
        &mut panes,
        click,
    );

    assert!(passes >= 2, "the fixture must execute a replacement pass");
    assert_eq!(selected_item(&dockspace), Some(SECOND));
}

#[test]
fn production_single_surface_accesskit_click_selects_a_tab() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("official-egui-accesskit", workspace())
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (target_node, _) = accesskit_node(&stable.output, Role::Tab, "Second");
    let event = Event::AccessKitActionRequest(ActionRequest {
        action: Action::Click,
        target_tree: TreeId::ROOT,
        target_node,
        data: None,
    });
    let _ = run_frame(&context, &mut dockspace, &mut panes, vec![event]);

    assert_eq!(selected_item(&dockspace), Some(SECOND));
}

#[test]
fn production_single_surface_close_button_requests_the_exact_tab() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("official-egui-close", workspace())
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (_, close_node) = accesskit_node(&stable.output, Role::Button, "Close Second");
    assert!(!close_node.is_disabled());
    assert!(close_node.supports_action(Action::Click));
    let pointer = node_center(close_node);
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(pointer),
            Event::PointerButton {
                pos: pointer,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    let released = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(pointer),
            Event::PointerButton {
                pos: pointer,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ],
    );

    assert!(released.local_actions_current);
    assert_eq!(selected_item(&dockspace), Some(FIRST));
    assert_eq!(released.close_items, [SECOND]);
}

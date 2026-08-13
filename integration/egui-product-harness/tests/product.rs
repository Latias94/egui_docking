use dockspace::geometry::LogicalRect;
use dockspace::model::{
    DockspaceAxis, DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId,
};
use egui::accesskit::{Action, ActionRequest, Role, TreeId};
use egui::{Context, Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui, vec2};
use egui_dockspace::{CloseDecision, DockspaceCloseRequest};
use egui_dockspace::{Dockspace, PaneView};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FLOATING_ROOT: RootId = RootId::new(2);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
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

fn layout() -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, SECOND])),
    )])
    .expect("the product fixture is valid")
}

fn split_layout() -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            ROOT,
            DockspaceNode::equal_split(
                DockspaceAxis::Horizontal,
                [DockspaceNode::tabs([FIRST]), DockspaceNode::tabs([SECOND])],
            )
            .expect("the product split fixture is valid"),
        ),
    )])
    .expect("the product split layout is valid")
}

fn contained_layout() -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs([SECOND])),
        LogicalRect::new(500.0, 260.0, 240.0, 220.0)
            .expect("the contained product fixture rectangle is valid"),
    ))])
    .expect("the contained product layout is valid")
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
) -> FrameOutput {
    let mut close_requests = Vec::new();
    let mut output = context.run_ui(input(events), |ui| {
        let response = dockspace
            .show_single_surface(SURFACE, ui, panes)
            .expect("the product frame advances");
        close_requests.extend(response.close_request_events().iter().cloned());
    });
    output.textures_delta.clear();
    FrameOutput {
        output,
        close_requests,
    }
}

fn run_frame_with_discard_after_dockspace(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut Panes,
    events: Vec<Event>,
) -> (FrameOutput, usize) {
    let mut close_requests = Vec::new();
    let mut passes = 0;
    let mut output = context.run_ui(input(events), |ui| {
        let response = dockspace
            .show_single_surface(SURFACE, ui, panes)
            .expect("the product multipass frame advances");
        close_requests.extend(response.close_request_events().iter().cloned());
        passes += 1;
        if ui.ctx().current_pass_index() == 0 {
            ui.ctx()
                .request_discard("exercise product response action retention");
        }
    });
    output.textures_delta.clear();
    (
        FrameOutput {
            output,
            close_requests,
        },
        passes,
    )
}

struct FrameOutput {
    output: egui::FullOutput,
    close_requests: Vec<DockspaceCloseRequest>,
}

fn node_center(output: &egui::FullOutput, role: Role, label: &str) -> Pos2 {
    node_rect(output, role, label).center()
}

fn node_rect(output: &egui::FullOutput, role: Role, label: &str) -> Rect {
    let (_, node) = accesskit_node(output, role, label);
    let bounds = node.bounds().expect("the control exposes bounds");
    Rect::from_min_max(
        Pos2::new(bounds.x0 as f32, bounds.y0 as f32),
        Pos2::new(bounds.x1 as f32, bounds.y1 as f32),
    )
}

fn accesskit_node<'a>(
    output: &'a egui::FullOutput,
    role: Role,
    label: &str,
) -> (egui::accesskit::NodeId, &'a egui::accesskit::Node) {
    let tree = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit is enabled");
    tree.nodes
        .iter()
        .find_map(|(id, node)| {
            (node.role() == role && node.label() == Some(label)).then_some((*id, node))
        })
        .expect("the requested accessibility node is present")
}

fn tab_center(output: &egui::FullOutput, label: &str) -> Pos2 {
    node_center(output, Role::Tab, label)
}

fn selected(dockspace: &Dockspace) -> Option<ItemId> {
    [FIRST, SECOND].into_iter().find(|item| {
        dockspace
            .view()
            .item(*item)
            .is_some_and(|view| view.is_selected())
    })
}

fn tab_items(dockspace: &Dockspace) -> Vec<ItemId> {
    dockspace
        .view()
        .root(ROOT)
        .and_then(|root| root.content())
        .and_then(|node| node.tabs())
        .map_or_else(Vec::new, |tabs| tabs.items().to_vec())
}

fn root_tabs(dockspace: &Dockspace) -> Option<Vec<ItemId>> {
    Some(
        dockspace
            .view()
            .root(ROOT)?
            .content()?
            .tabs()?
            .items()
            .to_vec(),
    )
}

fn root_split(dockspace: &Dockspace) -> Option<(DockspaceAxis, Vec<Vec<ItemId>>)> {
    let split = dockspace.view().root(ROOT)?.content()?.split()?;
    Some((
        split.axis(),
        split
            .children()
            .map(|child| {
                child
                    .tabs()
                    .map_or_else(Vec::new, |tabs| tabs.items().to_vec())
            })
            .collect(),
    ))
}

fn root_weights(dockspace: &Dockspace) -> Option<Vec<f32>> {
    Some(
        dockspace
            .view()
            .root(ROOT)?
            .content()?
            .split()?
            .weights()
            .collect(),
    )
}

fn contained_title_point(dockspace: &Dockspace) -> Pos2 {
    let rect = dockspace
        .view()
        .contained(FLOATING)
        .expect("the contained fixture is present")
        .rect();
    let style = dockspace.style();
    let title_inset = style
        .floating_resize_extent
        .min(style.floating_title_height * 0.5);
    Pos2::new(
        rect.min().x() as f32
            + style.floating_border_width
            + title_inset
            + style.tab_horizontal_padding,
        rect.min().y() as f32 + style.floating_border_width + style.floating_title_height * 0.5,
    )
}

fn pointer_button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

fn key_press(key: Key) -> Vec<Event> {
    [true, false]
        .into_iter()
        .map(|pressed| Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        })
        .collect()
}

fn accesskit_action(target_node: egui::accesskit::NodeId, action: Action) -> Event {
    Event::AccessKitActionRequest(ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node,
        data: None,
    })
}

fn guide_button_count(
    output: &egui::FullOutput,
    passive: egui::Color32,
    active: egui::Color32,
) -> usize {
    output
        .shapes
        .iter()
        .filter(|shape| {
            matches!(
                &shape.shape,
                egui::Shape::Rect(rect) if rect.fill == passive || rect.fill == active
            )
        })
        .count()
}

fn nearest_guide_center(
    output: &egui::FullOutput,
    passive: egui::Color32,
    active: egui::Color32,
    expected: Pos2,
) -> Pos2 {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect) if rect.fill == passive || rect.fill == active => {
                Some(rect.rect.center())
            }
            _ => None,
        })
        .min_by(|left, right| {
            left.distance_sq(expected)
                .total_cmp(&right.distance_sq(expected))
        })
        .expect("an active drag paints the requested guide cluster")
}

fn rect_fill_count(output: &egui::FullOutput, fill: egui::Color32) -> usize {
    output
        .shapes
        .iter()
        .filter(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.fill == fill))
        .count()
}

fn splitter_rect(output: &egui::FullOutput, idle: egui::Color32, active: egui::Color32) -> Rect {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect)
                if (rect.fill == idle || rect.fill == active)
                    && (rect.rect.width() <= 12.0 || rect.rect.height() <= 12.0) =>
            {
                Some(rect.rect)
            }
            _ => None,
        })
        .expect("the split fixture paints one splitter")
}

#[test]
fn default_features_render_a_ready_product_surface() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-ready", layout())
        .build()
        .expect("the product facade initializes");
    let mut panes = Panes;

    let first = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        first.output.platform_output.accesskit_update.is_some(),
        "the bootstrap frame still publishes a valid accessibility tree",
    );
    let ready = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = tab_center(&ready.output, "First");
    let _ = tab_center(&ready.output, "Second");
}

#[test]
fn default_features_click_selects_a_tab() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-click", layout())
        .build()
        .expect("the product facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let ready = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let pointer = tab_center(&ready.output, "Second");
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
            Event::PointerButton {
                pos: pointer,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ],
    );

    assert_eq!(selected(&dockspace), Some(SECOND));
}

#[test]
fn default_features_keyboard_navigation_keeps_tab_focus_with_selection() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-keyboard-navigation", layout())
        .build()
        .expect("the product facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let first = tab_center(&stable.output, "First");
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(first),
            pointer_button(first, true),
            pointer_button(first, false),
        ],
    );

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        key_press(Key::ArrowRight),
    );
    assert_eq!(selected(&dockspace), Some(SECOND));

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_frame(&context, &mut dockspace, &mut panes, key_press(Key::Home));
    assert_eq!(selected(&dockspace), Some(FIRST));

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_frame(&context, &mut dockspace, &mut panes, key_press(Key::End));
    assert_eq!(selected(&dockspace), Some(SECOND));
}

#[test]
fn default_features_tab_drag_reorders_across_multipass_discard() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let mut dockspace = Dockspace::builder("product-tab-reorder", layout())
        .build()
        .expect("the product facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let first_rect = node_rect(&stable.output, Role::Tab, "First");
    let first = Pos2::new(
        first_rect.min.x + first_rect.width() * 0.25,
        first_rect.center().y,
    );
    let second = tab_center(&stable.output, "Second");

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(second), pointer_button(second, true)],
    );
    let (_, passes) = run_frame_with_discard_after_dockspace(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(first)],
    );
    assert!(
        passes > 1,
        "the fixture must execute an actual egui multipass"
    );
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(first), pointer_button(first, false)],
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());

    assert_eq!(tab_items(&dockspace), vec![SECOND, FIRST]);
}

#[test]
fn default_features_close_request_can_be_resolved() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-close", layout())
        .build()
        .expect("the product facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let ready = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let pointer = node_center(&ready.output, Role::Button, "Close Second");
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

    let request = released
        .close_requests
        .first()
        .expect("the close button opens one plan");
    let item = request.plan().items()[0];
    assert_eq!(item.item(), SECOND);
    dockspace
        .resolve_close(request.plan().request(), item.token(), CloseDecision::Allow)
        .expect("the close decision commits");
    assert!(dockspace.view().item(SECOND).is_none());
}

#[test]
fn default_features_paint_outer_guides_and_dock_each_edge() {
    for (name, target, expected_axis, expected_items) in [
        (
            "left",
            Pos2::new(40.0, 300.0),
            DockspaceAxis::Horizontal,
            vec![vec![SECOND], vec![FIRST]],
        ),
        (
            "right",
            Pos2::new(760.0, 300.0),
            DockspaceAxis::Horizontal,
            vec![vec![FIRST], vec![SECOND]],
        ),
        (
            "top",
            Pos2::new(400.0, 40.0),
            DockspaceAxis::Vertical,
            vec![vec![SECOND], vec![FIRST]],
        ),
        (
            "bottom",
            Pos2::new(400.0, 560.0),
            DockspaceAxis::Vertical,
            vec![vec![FIRST], vec![SECOND]],
        ),
    ] {
        let context = Context::default();
        context.enable_accesskit();
        let mut dockspace = Dockspace::builder(("product-guide", name), split_layout())
            .build()
            .expect("the product guide facade initializes");
        let mut panes = Panes;

        let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let source = tab_center(&stable.output, "Second");
        let passive = dockspace.style().drop_guide_fill;
        let active = dockspace.style().drop_guide_active_fill;

        let _ = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(source), pointer_button(source, true)],
        );
        let _ = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(target)],
        );
        let _ = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(target)],
        );
        let painted = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(target)],
        );
        assert!(
            guide_button_count(&painted.output, passive, active) >= 4,
            "an active drag must paint at least the complete outer four-way guide",
        );
        assert!(
            painted.output.shapes.iter().any(|shape| matches!(
                &shape.shape,
                egui::Shape::Rect(rect) if rect.fill == active
            )),
            "the exact hovered guide must use the active visual state",
        );

        let _ = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(target), pointer_button(target, false)],
        );
        assert_eq!(
            root_split(&dockspace),
            Some((expected_axis, expected_items)),
            "the {name} guide must commit the same edge target shown in preview",
        );
    }
}

#[test]
fn default_features_center_guide_merges_tabs() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-guide-center", split_layout())
        .build()
        .expect("the product guide facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = tab_center(&stable.output, "Second");
    let first = tab_center(&stable.output, "First");
    let passive = dockspace.style().drop_guide_fill;
    let active = dockspace.style().drop_guide_active_fill;

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    for _ in 0..2 {
        let _ = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(first)],
        );
    }
    let cluster = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(first)],
    );
    let target = nearest_guide_center(&cluster.output, passive, active, Pos2::new(200.0, 314.0));
    for _ in 0..2 {
        let _ = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(target)],
        );
    }
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(target), pointer_button(target, false)],
    );

    assert_eq!(root_tabs(&dockspace), Some(vec![FIRST, SECOND]));
}

#[test]
fn default_features_splitter_tracks_pointer_before_release() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("product-live-splitter", split_layout())
        .build()
        .expect("the product splitter facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let ready = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let idle = dockspace.style().splitter_color;
    let active = dockspace.style().splitter_hover_color;
    let initial_splitter = splitter_rect(&ready.output, idle, active);
    let initial_weights = root_weights(&dockspace).expect("the fixture root remains split");
    let source = initial_splitter.center();
    let moved = source + vec2(80.0, 0.0);

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(moved)],
    );
    let live = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let live_splitter = splitter_rect(&live.output, idle, active);

    assert!(
        live_splitter.center().x > initial_splitter.center().x + 40.0,
        "transient splitter geometry must follow the pointer before durable commit",
    );
    assert_eq!(
        root_weights(&dockspace),
        Some(initial_weights.clone()),
        "pointer motion must not mutate durable weights before release",
    );

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(moved), pointer_button(moved, false)],
    );
    assert_ne!(
        root_weights(&dockspace),
        Some(initial_weights),
        "release commits the transient splitter proposal",
    );
}

#[test]
fn default_features_splitter_supports_accesskit_and_keyboard_adjustment() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-splitter-semantics", split_layout())
        .build()
        .expect("the product splitter facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (splitter, node) = accesskit_node(&stable.output, Role::Splitter, "Resize panes");
    assert!(node.supports_action(Action::Focus));
    assert!(node.supports_action(Action::Increment));
    assert!(node.supports_action(Action::Decrement));
    let initial = root_weights(&dockspace).expect("the fixture root remains split");

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(splitter, Action::Increment)],
    );
    let increased = root_weights(&dockspace).expect("the fixture root remains split");
    assert!(increased[0] > initial[0]);

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(splitter, Action::Focus)],
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        key_press(Key::ArrowLeft),
    );
    let decreased = root_weights(&dockspace).expect("the fixture root remains split");
    assert!(decreased[0] < increased[0]);
}

#[test]
fn default_features_escape_cancels_one_active_drag_and_clears_preview() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-escape", split_layout())
        .build()
        .expect("the product drag facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = tab_center(&stable.output, "Second");
    let target = Pos2::new(400.0, 40.0);
    let passive = dockspace.style().drop_guide_fill;
    let active = dockspace.style().drop_guide_active_fill;

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    for _ in 0..3 {
        let _ = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(target)],
        );
    }
    let preview = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(target)],
    );
    assert!(guide_button_count(&preview.output, passive, active) >= 4);

    let _ = run_frame(&context, &mut dockspace, &mut panes, key_press(Key::Escape));
    let cancelled = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(guide_button_count(&cancelled.output, passive, active), 0);
    assert_eq!(
        root_split(&dockspace),
        Some((DockspaceAxis::Horizontal, vec![vec![FIRST], vec![SECOND]],)),
    );

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(target), pointer_button(target, false)],
    );
    assert_eq!(
        root_split(&dockspace),
        Some((DockspaceAxis::Horizontal, vec![vec![FIRST], vec![SECOND]],)),
    );
}

#[test]
fn default_features_release_waits_for_the_new_preview_to_be_painted() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-release-resample", split_layout())
        .build()
        .expect("the product release facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = tab_center(&stable.output, "Second");
    let painted_top = Pos2::new(400.0, 40.0);
    let release_bottom = Pos2::new(400.0, 560.0);

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    for _ in 0..3 {
        let _ = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(painted_top)],
        );
    }
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(release_bottom),
            pointer_button(release_bottom, false),
        ],
    );
    assert_eq!(
        root_split(&dockspace).map(|(axis, _)| axis),
        Some(DockspaceAxis::Horizontal),
        "an unpainted release target must remain pending instead of committing invisibly",
    );

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(
        root_split(&dockspace),
        Some((DockspaceAxis::Vertical, vec![vec![FIRST], vec![SECOND]],)),
        "the pending release commits after the exact bottom preview is painted",
    );
}

#[test]
fn default_features_contained_resize_commits_a_painted_preview() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("product-contained-resize", contained_layout())
        .build()
        .expect("the product contained facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let initial = dockspace
        .view()
        .contained(FLOATING)
        .expect("the contained fixture is present")
        .rect();
    let source = Pos2::new(
        initial.max().x() as f32 - 1.0,
        initial.min().y() as f32 + 100.0,
    );
    let moved = source + vec2(30.0, 0.0);

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(moved)],
    );
    assert_eq!(
        dockspace
            .view()
            .contained(FLOATING)
            .expect("the contained fixture remains present")
            .rect(),
        initial,
        "resize motion remains transient until release",
    );

    let released = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(moved), pointer_button(moved, false)],
    );
    assert!(
        rect_fill_count(&released.output, dockspace.style().ghost_fill) > 0,
        "the exact contained transform preview must be painted before release settles",
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let committed = dockspace
        .view()
        .contained(FLOATING)
        .expect("the contained fixture remains present")
        .rect();
    assert_eq!(committed.min(), initial.min());
    assert!(committed.max().x() > initial.max().x());
}

#[test]
fn default_features_contained_title_moves_without_a_dock_target() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("product-contained-move", contained_layout())
        .build()
        .expect("the product contained facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let initial = dockspace
        .view()
        .contained(FLOATING)
        .expect("the contained fixture is present")
        .rect();
    let source = contained_title_point(&dockspace);
    let destination = source + vec2(-120.0, 80.0);

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(destination)],
    );
    assert_eq!(
        dockspace
            .view()
            .contained(FLOATING)
            .expect("the contained fixture remains present")
            .rect(),
        initial,
        "contained motion remains transient until release",
    );

    let released = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(destination),
            pointer_button(destination, false),
        ],
    );
    assert!(
        rect_fill_count(&released.output, dockspace.style().drop_fill) > 0,
        "the contained move preview must be painted before release settles",
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let committed = dockspace
        .view()
        .contained(FLOATING)
        .expect("the contained fixture remains present")
        .rect();
    assert_eq!(committed.width(), initial.width());
    assert_eq!(committed.height(), initial.height());
    assert_eq!(committed.min().x(), initial.min().x() - 120.0);
    assert_eq!(committed.min().y(), initial.min().y() + 80.0);
}

#[test]
fn default_features_contained_title_redocks_through_the_canonical_guide() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("product-contained-redock", contained_layout())
        .build()
        .expect("the product contained facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = contained_title_point(&dockspace);
    let target = Pos2::new(400.0, 40.0);

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    for _ in 0..3 {
        let _ = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(target)],
        );
    }
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(target), pointer_button(target, false)],
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());

    assert_eq!(
        root_split(&dockspace),
        Some((DockspaceAxis::Vertical, vec![vec![SECOND], vec![FIRST]],)),
    );
    assert!(dockspace.view().contained(FLOATING).is_none());
}

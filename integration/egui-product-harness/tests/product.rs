use egui::accesskit::{Action, ActionRequest, Role, TreeId};
use egui::{
    Context, Event, FullOutput, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, RepaintCause,
    Ui, vec2,
};
use egui_dockspace::{
    CloseDecision, ClosePlanTarget, DockStyle, Dockspace, DockspaceActionOutcome,
    DockspaceActionStatus, DockspaceAxis, DockspaceCloseRequest, DockspaceCloseRequestRejection,
    DockspaceCloseRequestStatus, DockspaceContainedLayout, DockspaceLayout, DockspaceNode,
    DockspaceRootLayout, DockspaceSurfaceLayout, FloatingPresentationId, ItemId, LogicalRect,
    PaneView, RootId, SurfaceId,
};
use std::panic::{AssertUnwindSafe, catch_unwind};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FLOATING_ROOT: RootId = RootId::new(2);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);
const FOURTH: ItemId = ItemId::new(4);

struct Panes;

#[derive(Default)]
struct LateDiscardPlugin {
    remaining: usize,
    panic_once: bool,
}

impl egui::plugin::Plugin for LateDiscardPlugin {
    fn debug_name(&self) -> &'static str {
        "egui_dockspace_test::late_discard"
    }

    fn output_hook(&mut self, _context: &Context, output: &mut FullOutput) {
        if self.panic_once {
            self.panic_once = false;
            panic!("late output hook interrupted the egui run");
        }
        if self.remaining == 0 {
            return;
        }
        self.remaining -= 1;
        output
            .platform_output
            .request_discard_reasons
            .push(RepaintCause::new_reason(
                "discard after dockspace final-pass settlement runs",
            ));
    }
}

impl PaneView for Panes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        match item {
            FIRST => Some("First".into()),
            SECOND => Some("Second".into()),
            THIRD => Some("Third".into()),
            FOURTH => Some("Fourth".into()),
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

fn overflow_layout() -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            ROOT,
            DockspaceNode::central_tabs([FIRST, SECOND, THIRD, FOURTH]),
        ),
    )])
    .expect("the overflow product fixture is valid")
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

fn splitter_junction_layout() -> DockspaceLayout {
    let column = |top, bottom| {
        DockspaceNode::equal_split(
            DockspaceAxis::Vertical,
            [DockspaceNode::tabs([top]), DockspaceNode::tabs([bottom])],
        )
        .expect("the product splitter column is valid")
    };
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            ROOT,
            DockspaceNode::equal_split(
                DockspaceAxis::Horizontal,
                [column(FIRST, SECOND), column(THIRD, FOURTH)],
            )
            .expect("the product splitter junction fixture is valid"),
        ),
    )])
    .expect("the product splitter junction layout is valid")
}

fn contained_layout() -> DockspaceLayout {
    contained_layout_at(
        LogicalRect::new(500.0, 260.0, 240.0, 220.0)
            .expect("the contained product fixture rectangle is valid"),
    )
}

fn contained_layout_at(rect: LogicalRect) -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs([SECOND])),
        rect,
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

fn run_disabled_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut Panes,
    events: Vec<Event>,
) -> FrameOutput {
    let mut close_requests = Vec::new();
    let mut output = context.run_ui(input(events), |ui| {
        ui.add_enabled_ui(false, |ui| {
            let response = dockspace
                .show_single_surface(SURFACE, ui, panes)
                .expect("the disabled product frame advances");
            close_requests.extend(response.close_request_events().iter().cloned());
        });
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

fn run_two_discarded_dockspace_passes(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut Panes,
    events: Vec<Event>,
) -> usize {
    let mut passes = 0;
    let mut output = context.run_ui(input(events), |ui| {
        passes += 1;
        if ui.ctx().current_pass_index() < 2 {
            dockspace
                .show_single_surface(SURFACE, ui, panes)
                .expect("the discarded product pass advances");
        } else {
            ui.label("terminal pass intentionally omits the dockspace");
        }
    });
    output.textures_delta.clear();
    passes
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
        .unwrap_or_else(|| {
            let nodes = tree
                .nodes
                .iter()
                .map(|(_, node)| (node.role(), node.label().map(str::to_owned)))
                .collect::<Vec<_>>();
            panic!("missing accessibility node {role:?} {label:?}; nodes={nodes:?}")
        })
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

fn contained_rect(dockspace: &Dockspace) -> LogicalRect {
    dockspace
        .view()
        .contained(FLOATING)
        .expect("the contained fixture is present")
        .rect()
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
fn default_features_expose_item_centric_contained_actions() {
    let first_rect =
        LogicalRect::new(40.0, 50.0, 260.0, 180.0).expect("the first contained rectangle is valid");
    let second_rect = LogicalRect::new(340.0, 80.0, 280.0, 200.0)
        .expect("the second contained rectangle is valid");
    let updated_rect = LogicalRect::new(90.0, 120.0, 300.0, 240.0)
        .expect("the updated contained rectangle is valid");
    let mut dockspace = Dockspace::builder("product-contained-actions", layout())
        .build()
        .expect("the product facade initializes");

    dockspace
        .float_item_current(SECOND, SURFACE, first_rect)
        .expect("the first item floats through the product facade");
    let second_floating = dockspace
        .view()
        .item(SECOND)
        .and_then(|item| item.contained())
        .expect("the floated item owns a contained presentation");

    dockspace
        .float_item_current(FIRST, SURFACE, second_rect)
        .expect("the singleton main root floats through the product facade");
    let first_floating = dockspace
        .view()
        .item(FIRST)
        .and_then(|item| item.contained())
        .expect("the second floated item owns a contained presentation");
    assert_eq!(
        dockspace
            .view()
            .surface(SURFACE)
            .expect("the surface remains available")
            .contained()
            .map(|contained| contained.id())
            .collect::<Vec<_>>(),
        vec![second_floating, first_floating],
    );

    dockspace
        .set_contained_rect_current(SECOND, updated_rect)
        .expect("contained bounds update through the product facade");
    assert_eq!(
        dockspace
            .view()
            .contained(second_floating)
            .expect("the updated contained presentation remains available")
            .rect(),
        updated_rect,
    );

    dockspace
        .raise_contained_current(SECOND)
        .expect("contained raise through the product facade");
    assert_eq!(
        dockspace
            .view()
            .surface(SURFACE)
            .expect("the surface remains available")
            .contained()
            .map(|contained| contained.id())
            .collect::<Vec<_>>(),
        vec![first_floating, second_floating],
    );
}

#[test]
fn default_features_bring_contained_content_into_current_ready_bounds() {
    let context = Context::default();
    let offscreen = LogicalRect::new(900.0, 700.0, 240.0, 220.0)
        .expect("the offscreen contained rectangle is valid");
    let mut dockspace = Dockspace::builder(
        "product-contained-bring-into-view",
        contained_layout_at(offscreen),
    )
    .build()
    .expect("the product contained facade initializes");
    let mut panes = Panes;
    let roster = dockspace
        .view()
        .surface(SURFACE)
        .expect("the surface is present")
        .contained()
        .map(|contained| contained.id())
        .collect::<Vec<_>>();

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let result = dockspace
        .bring_contained_into_view_current(SECOND)
        .expect("the public product action uses current ready bounds");
    assert!(
        matches!(
            result.status(),
            DockspaceActionStatus::Applied(DockspaceActionOutcome::ContainedBoundsUpdated {
                root: FLOATING_ROOT,
                surface: SURFACE,
                floating: FLOATING,
                changed: true,
            })
        ),
        "unexpected bring-into-view status: {:?}",
        result.status(),
    );

    let rect = dockspace
        .view()
        .contained(FLOATING)
        .expect("the contained presentation remains available")
        .rect();
    assert_eq!(rect.size(), offscreen.size());
    assert!(rect.min().x() >= 0.0 && rect.min().y() >= 0.0);
    assert!(rect.max().x() <= 800.0 && rect.max().y() <= 600.0);
    assert_eq!(
        dockspace
            .view()
            .surface(SURFACE)
            .expect("the surface remains present")
            .contained()
            .map(|contained| contained.id())
            .collect::<Vec<_>>(),
        roster,
        "bring-into-view must not change contained stacking order",
    );
}

#[test]
fn default_features_contained_chrome_exposes_focusable_accessibility_controls() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-contained-accessibility", contained_layout())
        .build()
        .expect("the product contained facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (title, title_node) = accesskit_node(&stable.output, Role::TitleBar, "Second");
    assert!(title_node.supports_action(Action::Focus));
    let (_, close_node) = accesskit_node(&stable.output, Role::Button, "Close floating Second");
    assert!(close_node.supports_action(Action::Focus));
    assert!(close_node.supports_action(Action::Click));

    let focused = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(title, Action::Focus)],
    );
    assert_eq!(
        focused
            .output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("AccessKit remains enabled")
            .focus,
        title,
    );
}

#[test]
fn default_features_contained_close_supports_pointer_accesskit_and_keyboard() {
    #[derive(Clone, Copy)]
    enum Activation {
        Pointer,
        AccessKit,
        Keyboard(Key),
    }

    for (name, activation) in [
        ("pointer", Activation::Pointer),
        ("accesskit", Activation::AccessKit),
        ("enter", Activation::Keyboard(Key::Enter)),
        ("space", Activation::Keyboard(Key::Space)),
    ] {
        let context = Context::default();
        context.enable_accesskit();
        let mut dockspace = Dockspace::builder(
            format!("product-contained-close-{name}"),
            contained_layout(),
        )
        .build()
        .expect("the product contained facade initializes");
        let mut panes = Panes;

        let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let (close, _) = accesskit_node(&stable.output, Role::Button, "Close floating Second");
        let point = node_center(&stable.output, Role::Button, "Close floating Second");
        let activated = match activation {
            Activation::Pointer => {
                let _ = run_frame(
                    &context,
                    &mut dockspace,
                    &mut panes,
                    vec![Event::PointerMoved(point), pointer_button(point, true)],
                );
                run_frame(
                    &context,
                    &mut dockspace,
                    &mut panes,
                    vec![Event::PointerMoved(point), pointer_button(point, false)],
                )
            }
            Activation::AccessKit => run_frame(
                &context,
                &mut dockspace,
                &mut panes,
                vec![accesskit_action(close, Action::Click)],
            ),
            Activation::Keyboard(key) => {
                let _ = run_frame(
                    &context,
                    &mut dockspace,
                    &mut panes,
                    vec![accesskit_action(close, Action::Focus)],
                );
                run_frame(&context, &mut dockspace, &mut panes, key_press(key))
            }
        };

        assert_eq!(activated.close_requests.len(), 1, "{name}");
        let request = &activated.close_requests[0];
        assert!(!request.reused(), "{name}");
        assert_eq!(
            request.plan().target(),
            ClosePlanTarget::Root {
                root: FLOATING_ROOT,
            },
            "{name}",
        );
        assert_eq!(request.plan().items().len(), 1);
        assert_eq!(request.plan().items()[0].item(), SECOND);
    }
}

#[test]
fn disabled_ui_ignores_delayed_contained_accesskit_actions() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-disabled-contained-close", contained_layout())
        .build()
        .expect("the product contained facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (close, _) = accesskit_node(&stable.output, Role::Button, "Close floating Second");
    let (right, _) = accesskit_node(&stable.output, Role::Splitter, "Resize floating right edge");
    let before = contained_rect(&dockspace);
    let disabled = run_disabled_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            accesskit_action(close, Action::Click),
            accesskit_action(right, Action::Increment),
        ],
    );

    assert!(disabled.close_requests.is_empty());
    assert_eq!(contained_rect(&dockspace), before);
    let (_, resize_node) = accesskit_node(
        &disabled.output,
        Role::Splitter,
        "Resize floating right edge",
    );
    assert!(!resize_node.supports_action(Action::Focus));
    assert!(!resize_node.supports_action(Action::Increment));
    assert!(!resize_node.supports_action(Action::Decrement));
}

#[test]
fn default_features_contained_cardinal_resize_supports_accesskit_and_keyboard() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace =
        Dockspace::builder("product-contained-resize-semantics", contained_layout())
            .build()
            .expect("the product contained facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let tree = stable
        .output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit is enabled");
    let resize_nodes = tree
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == Role::Splitter)
        .collect::<Vec<_>>();
    assert_eq!(
        resize_nodes.len(),
        4,
        "only cardinal contained edges are focusable controls",
    );
    for (_, node) in resize_nodes {
        assert!(node.supports_action(Action::Focus));
        assert!(node.supports_action(Action::Increment));
        assert!(node.supports_action(Action::Decrement));
    }

    let (right, _) = accesskit_node(&stable.output, Role::Splitter, "Resize floating right edge");
    let (top, _) = accesskit_node(&stable.output, Role::Splitter, "Resize floating top edge");
    let initial = contained_rect(&dockspace);
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(right, Action::Increment)],
    );
    let widened = contained_rect(&dockspace);
    assert_eq!(widened.min(), initial.min());
    assert!(widened.width() > initial.width());

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(top, Action::Focus)],
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        key_press(Key::ArrowUp),
    );
    let raised = contained_rect(&dockspace);
    assert!(raised.min().y() < widened.min().y());
    assert_eq!(raised.max().y(), widened.max().y());
}

#[test]
fn contained_resize_policy_denial_advertises_no_adjustment_actions() {
    let context = Context::default();
    context.enable_accesskit();
    let mut policy = dockspace::policy::DockPolicy::default();
    policy.set_allow_contained_transform(false);
    let mut dockspace = Dockspace::builder("product-contained-resize-disabled", contained_layout())
        .policy(policy)
        .build()
        .expect("the policy-disabled contained facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let tree = stable
        .output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit is enabled");
    let resize_nodes = tree
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == Role::Splitter)
        .collect::<Vec<_>>();
    assert_eq!(resize_nodes.len(), 4);
    for (_, node) in &resize_nodes {
        assert!(!node.supports_action(Action::Focus));
        assert!(!node.supports_action(Action::Increment));
        assert!(!node.supports_action(Action::Decrement));
    }

    let before = contained_rect(&dockspace);
    let right = resize_nodes
        .iter()
        .find_map(|(id, node)| (node.label() == Some("Resize floating right edge")).then_some(*id))
        .expect("the disabled right edge remains described");
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(right, Action::Increment)],
    );
    assert_eq!(contained_rect(&dockspace), before);
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
fn default_features_overflow_menu_opens_and_selects_hidden_tab() {
    let context = Context::default();
    context.enable_accesskit();
    let mut style = DockStyle::default();
    style.tab_min_width = 220.0;
    style.tab_max_width = 220.0;
    let mut dockspace = Dockspace::builder("product-overflow-menu", overflow_layout())
        .style(style)
        .build()
        .expect("the overflow product facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (control, control_node) = accesskit_node(&stable.output, Role::Button, "Show hidden tabs");
    assert!(control_node.supports_action(Action::Click));
    let control_point = node_center(&stable.output, Role::Button, "Show hidden tabs");

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(control_point),
            pointer_button(control_point, true),
            pointer_button(control_point, false),
        ],
    );
    let menu = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (row, row_node) = accesskit_node(&menu.output, Role::MenuItem, "Fourth");
    assert!(row_node.supports_action(Action::Click));
    assert!(row_node.supports_action(Action::ScrollIntoView));
    let row_point = node_center(&menu.output, Role::MenuItem, "Fourth");
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(row_point),
            pointer_button(row_point, true),
            pointer_button(row_point, false),
        ],
    );

    assert!(
        dockspace
            .view()
            .item(FOURTH)
            .expect("fourth item remains open")
            .is_selected(),
        "activating a menu row selects the hidden item"
    );
    let _ = (control, row);
}

#[test]
fn default_features_overflow_menu_supports_accesskit_and_keyboard() {
    let context = Context::default();
    context.enable_accesskit();
    let mut style = DockStyle::default();
    style.tab_min_width = 220.0;
    style.tab_max_width = 220.0;
    let mut dockspace = Dockspace::builder("product-overflow-menu-inputs", overflow_layout())
        .style(style)
        .build()
        .expect("the overflow product facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (control, control_node) = accesskit_node(&stable.output, Role::Button, "Show hidden tabs");
    assert!(control_node.supports_action(Action::Click));
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(control, Action::Click)],
    );

    let menu = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (_, row_node) = accesskit_node(&menu.output, Role::MenuItem, "Fourth");
    assert!(row_node.supports_action(Action::Focus));
    assert!(row_node.supports_action(Action::ScrollIntoView));

    let _ = run_frame(&context, &mut dockspace, &mut panes, key_press(Key::End));
    let _ = run_frame(&context, &mut dockspace, &mut panes, key_press(Key::Enter));

    assert!(
        dockspace
            .view()
            .item(FOURTH)
            .expect("fourth item remains open")
            .is_selected(),
        "keyboard activation selects the focused hidden item"
    );
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
    assert_eq!(
        tab_items(&dockspace),
        vec![FIRST, SECOND],
        "the first follow-up only publishes the release-resampled preview",
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
fn default_features_programmatic_root_close_uses_the_product_facade() {
    let mut root_dockspace = Dockspace::builder("product-programmatic-root-close", layout())
        .build()
        .expect("the root-close product facade initializes");
    let root_request = root_dockspace
        .request_close_root_current(ROOT)
        .expect("the root close request is reduced");
    let DockspaceCloseRequestStatus::Requested(request) = root_request.status() else {
        panic!("the root close request must open a plan")
    };
    assert_eq!(
        request.plan().target(),
        egui_dockspace::ClosePlanTarget::Root { root: ROOT }
    );
    for item in request.plan().items() {
        root_dockspace
            .resolve_close(request.plan().request(), item.token(), CloseDecision::Allow)
            .expect("each root close decision commits");
    }
    assert!(root_dockspace.view().root(ROOT).is_none());
}

#[test]
fn default_features_programmatic_close_reports_stale_and_root_rejection() {
    let mut stale_dockspace = Dockspace::builder("product-programmatic-close-stale", layout())
        .build()
        .expect("the stale-close product facade initializes");
    let prepared = stale_dockspace.prepare_close_item(FIRST);
    let expected = prepared.expected_version();
    let selection = stale_dockspace
        .select_item_current(SECOND)
        .expect("selection advances the product revision");
    assert!(matches!(
        selection.status(),
        DockspaceActionStatus::Applied(DockspaceActionOutcome::Selected {
            item: SECOND,
            changed: true,
        })
    ));
    let stale = stale_dockspace
        .submit_prepared_close_request(prepared)
        .expect("the stale close request is reduced");
    assert!(matches!(
        stale.status(),
        DockspaceCloseRequestStatus::Stale {
            expected: actual_expected,
            accepted,
        } if *actual_expected == expected && *accepted == stale.mutation().after()
    ));

    let mut policy = dockspace::policy::DockPolicy::default();
    policy.set_close_capability(dockspace::policy::CloseCapability::Disabled);
    let mut disabled_dockspace =
        Dockspace::builder("product-programmatic-root-close-disabled", layout())
            .policy(policy)
            .build()
            .expect("the disabled-close product facade initializes");
    let rejected = disabled_dockspace
        .request_close_root_current(ROOT)
        .expect("the disabled root close request is reduced");
    assert_eq!(
        rejected.status(),
        &DockspaceCloseRequestStatus::Rejected(DockspaceCloseRequestRejection::ItemCloseDisabled {
            target: ClosePlanTarget::Root { root: ROOT },
            item: FIRST,
        })
    );
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
fn default_features_splitter_junction_tracks_both_axes_and_commits_once() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace =
        Dockspace::builder("product-live-splitter-junction", splitter_junction_layout())
            .build()
            .expect("the product splitter junction facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let ready = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let initial = node_rect(&ready.output, Role::Splitter, "Resize pane grid");
    let source = initial.center();
    let moved = source + vec2(70.0, 50.0);
    let before_version = dockspace.version();

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
    let live_junction = node_rect(&live.output, Role::Splitter, "Resize pane grid");

    assert!(
        live_junction.center().x > initial.center().x + 35.0
            && live_junction.center().y > initial.center().y + 25.0,
        "the atomic junction preview must follow both pointer axes",
    );
    assert_eq!(
        dockspace.version(),
        before_version,
        "junction motion remains transient until release",
    );

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(moved), pointer_button(moved, false)],
    );
    assert_ne!(
        dockspace.version(),
        before_version,
        "one junction release commits both axes atomically",
    );
    let committed_version = dockspace.version();
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(dockspace.version(), committed_version);
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
        root_split(&dockspace).map(|(axis, _)| axis),
        Some(DockspaceAxis::Horizontal),
        "the first follow-up only publishes the newly painted terminal pass",
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(
        root_split(&dockspace),
        Some((DockspaceAxis::Vertical, vec![vec![FIRST], vec![SECOND]],)),
        "the pending release commits after the terminal preview is settled",
    );
}

#[test]
fn default_features_discarded_preview_cannot_release_a_drag() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let mut dockspace = Dockspace::builder("product-discarded-preview", split_layout())
        .build()
        .expect("the product discarded-preview facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = tab_center(&stable.output, "Second");
    let top = Pos2::new(400.0, 40.0);
    let bottom = Pos2::new(400.0, 560.0);

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
        vec![Event::PointerMoved(top)],
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());

    context
        .plugin_or_default::<LateDiscardPlugin>()
        .lock()
        .remaining = 2;

    let passes = run_two_discarded_dockspace_passes(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(bottom)],
    );
    assert_eq!(
        passes, 3,
        "the fixture must omit dockspace from the final pass"
    );

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(bottom), pointer_button(bottom, false)],
    );
    assert_eq!(
        root_split(&dockspace).map(|(axis, _)| axis),
        Some(DockspaceAxis::Horizontal),
        "a preview painted only by discarded passes cannot release the drag",
    );

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(
        root_split(&dockspace),
        Some((DockspaceAxis::Vertical, vec![vec![FIRST], vec![SECOND]],)),
        "the release settles after a terminal pass paints the exact preview",
    );
}

#[test]
fn default_features_unfinished_egui_run_cannot_settle_preview() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-interrupted-preview", split_layout())
        .build()
        .expect("the product interrupted-preview facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = tab_center(&stable.output, "Second");
    let top = Pos2::new(400.0, 40.0);
    let bottom = Pos2::new(400.0, 560.0);

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
        vec![Event::PointerMoved(top)],
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());

    context
        .plugin_or_default::<LateDiscardPlugin>()
        .lock()
        .panic_once = true;
    let interrupted = catch_unwind(AssertUnwindSafe(|| {
        let mut output = context.run_ui(input(vec![Event::PointerMoved(bottom)]), |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the interrupted product pass advances");
        });
        output.textures_delta.clear();
    }));
    assert!(
        interrupted.is_err(),
        "the late output hook must interrupt the run"
    );

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(bottom), pointer_button(bottom, false)],
    );
    assert_eq!(
        root_split(&dockspace).map(|(axis, _)| axis),
        Some(DockspaceAxis::Horizontal),
        "a preview from an unfinished egui run cannot release the drag",
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

#[test]
fn default_features_replace_policy_and_style_atomically() {
    let mut dockspace = Dockspace::builder("product-online-configuration", layout())
        .build()
        .expect("the product configuration facade initializes");
    let initial_style = dockspace.style().clone();
    let mut replacement_style = initial_style.clone();
    replacement_style.tab_bar_height += 8.0;
    replacement_style.tab_active_fill = egui::Color32::from_rgb(12, 34, 56);

    let style_mutation = dockspace
        .set_style(replacement_style.clone())
        .expect("valid style replacement commits");
    assert_eq!(dockspace.style(), &replacement_style);
    assert!(!style_mutation.workspace_changed());
    assert!(style_mutation.published_state_changed());
    assert_eq!(style_mutation.affected_surfaces(), &[SURFACE]);

    let version_before_visual_only_style = dockspace.version();
    let mut visual_only_style = replacement_style.clone();
    visual_only_style.tab_active_fill = egui::Color32::from_rgb(78, 90, 123);
    let visual_only_mutation = dockspace
        .set_style(visual_only_style.clone())
        .expect("visual-only style replacement commits");
    assert_eq!(dockspace.version(), version_before_visual_only_style);
    assert_eq!(dockspace.style(), &visual_only_style);
    assert!(!visual_only_mutation.workspace_changed());
    assert!(visual_only_mutation.published_state_changed());
    assert_eq!(visual_only_mutation.affected_surfaces(), &[SURFACE]);

    let mut replacement_policy = dockspace.policy().clone();
    replacement_policy.set_allow_tab_merge(false);
    let policy_mutation = dockspace
        .set_policy(replacement_policy.clone())
        .expect("valid policy replacement commits");
    assert_eq!(dockspace.policy(), &replacement_policy);
    assert!(policy_mutation.published_state_changed());

    let version_before_invalid_style = dockspace.version();
    let mut invalid_style = visual_only_style.clone();
    invalid_style.tab_min_width = invalid_style.tab_max_width + 1.0;
    assert!(dockspace.set_style(invalid_style).is_err());
    assert_eq!(dockspace.version(), version_before_invalid_style);
    assert_eq!(dockspace.style(), &visual_only_style);
}

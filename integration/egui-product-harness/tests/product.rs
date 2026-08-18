use egui::accesskit::{Action, ActionRequest, Orientation, Role, TreeId};
use egui::{
    Context, Event, FullOutput, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, RepaintCause,
    Ui, vec2,
};
use egui_dockspace::{
    CloseDecision, ClosePlanTarget, DockStyle, Dockspace, DockspaceActionOutcome,
    DockspaceActionStatus, DockspaceAxis, DockspaceCapability, DockspaceCloseRequest,
    DockspaceCloseRequestRejection, DockspaceCloseRequestStatus, DockspaceContainedLayout,
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
    DockspaceUnavailableReason, FloatingPresentationId, ItemId, LogicalRect, PaneFocusState,
    PaneView, RootId, SurfaceId,
};
use egui_kittest::{Harness, kittest::Queryable as _};
use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FLOATING_ROOT: RootId = RootId::new(2);
const FRONT_FLOATING_ROOT: RootId = RootId::new(3);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const FRONT_FLOATING: FloatingPresentationId = FloatingPresentationId::new(2);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);
const FOURTH: ItemId = ItemId::new(4);

fn instrumented_style() -> DockStyle {
    let mut style = DockStyle::default();
    style.visuals.drop_guide_fill = Some(egui::Color32::from_rgb(52, 57, 62));
    style.visuals.drop_guide_active_fill = Some(egui::Color32::from_rgb(49, 142, 154));
    style.visuals.splitter_color = Some(egui::Color32::from_rgb(69, 75, 82));
    style.visuals.splitter_hover_color = Some(egui::Color32::from_rgb(89, 157, 165));
    style.visuals.drop_fill = Some(egui::Color32::from_rgba_unmultiplied(49, 142, 154, 72));
    style.visuals.ghost_fill = Some(egui::Color32::from_rgba_unmultiplied(180, 132, 62, 64));
    style
}

struct Panes;

struct FocusPanes {
    target: egui::Id,
    target_rect: Cell<Option<Rect>>,
    expose_target: bool,
    target_requests: Cell<usize>,
}

impl FocusPanes {
    fn new(id_salt: &'static str, expose_target: bool) -> Self {
        Self {
            target: egui::Id::new((id_salt, "pane-focus-target")),
            target_rect: Cell::new(None),
            expose_target,
            target_requests: Cell::new(0),
        }
    }

    fn target_requests(&self) -> usize {
        self.target_requests.get()
    }

    fn target_rect(&self) -> Rect {
        self.target_rect
            .get()
            .expect("the selected pane published its focus target rectangle")
    }
}

struct KittestProductState {
    dockspace: Dockspace,
    panes: Panes,
    close_requests: Vec<DockspaceCloseRequest>,
}

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

impl PaneView for FocusPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Panes.title(item)
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) {
        if item == SECOND {
            let rect = Rect::from_min_size(ui.available_rect_before_wrap().min, vec2(120.0, 24.0));
            self.target_rect.set(Some(rect));
            let _ = ui.interact(rect, self.target, egui::Sense::click_and_drag());
        }
        ui.label(format!("Pane {}", item.get()));
    }

    fn focus_target(&self, item: ItemId) -> Option<egui::Id> {
        if item != SECOND {
            return None;
        }
        self.target_requests
            .set(self.target_requests.get().saturating_add(1));
        self.expose_target.then_some(self.target)
    }

    fn focus_state(&self, item: ItemId, context: &Context) -> PaneFocusState {
        if item != SECOND || !self.expose_target {
            return PaneFocusState::Unknown;
        }
        if context.input(|input| input.focused)
            && context.memory(|memory| memory.has_focus(self.target))
        {
            PaneFocusState::Focused
        } else {
            PaneFocusState::Unfocused
        }
    }
}

fn kittest_harness(
    id: &'static str,
    layout: DockspaceLayout,
) -> Harness<'static, KittestProductState> {
    let dockspace = Dockspace::builder(id, layout)
        .build()
        .expect("the kittest product facade initializes");
    Harness::builder()
        .with_size(vec2(800.0, 600.0))
        .with_max_steps(8)
        .build_ui_state(
            |ui, state: &mut KittestProductState| {
                let response = state
                    .dockspace
                    .show_single_surface(SURFACE, ui, &mut state.panes)
                    .expect("the kittest product frame advances");
                state
                    .close_requests
                    .extend(response.close_request_events().iter().cloned());
            },
            KittestProductState {
                dockspace,
                panes: Panes,
                close_requests: Vec::new(),
            },
        )
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

fn stacked_contained_layout() -> DockspaceLayout {
    let rear =
        LogicalRect::new(80.0, 80.0, 300.0, 220.0).expect("the rear contained rectangle is valid");
    let front = LogicalRect::new(300.0, 240.0, 300.0, 220.0)
        .expect("the front contained rectangle is valid");
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, THIRD])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs([SECOND])),
        rear,
    ))
    .with_contained(DockspaceContainedLayout::new(
        FRONT_FLOATING,
        DockspaceRootLayout::new(FRONT_FLOATING_ROOT, DockspaceNode::tabs([FOURTH])),
        front,
    ))])
    .expect("the stacked contained product layout is valid")
}

fn fully_occluded_stacked_contained_layout() -> DockspaceLayout {
    let rear = LogicalRect::new(220.0, 180.0, 240.0, 180.0)
        .expect("the rear contained rectangle is valid");
    let front = LogicalRect::new(180.0, 140.0, 320.0, 260.0)
        .expect("the front contained rectangle is valid");
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, THIRD])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs([SECOND])),
        rear,
    ))
    .with_contained(DockspaceContainedLayout::new(
        FRONT_FLOATING,
        DockspaceRootLayout::new(FRONT_FLOATING_ROOT, DockspaceNode::tabs([FOURTH])),
        front,
    ))])
    .expect("the fully occluded contained product layout is valid")
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
    panes: &mut dyn PaneView,
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

fn run_focus_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut FocusPanes,
    events: Vec<Event>,
) -> (FrameOutput, DockspaceCapability) {
    let mut close_requests = Vec::new();
    let mut pane_focus_capability = None;
    let mut output = context.run_ui(input(events), |ui| {
        let response = dockspace
            .show_single_surface(SURFACE, ui, panes)
            .expect("the pane-focus product frame advances");
        close_requests.extend(response.close_request_events().iter().cloned());
        pane_focus_capability = Some(response.pane_focus_capability());
    });
    output.textures_delta.clear();
    (
        FrameOutput {
            output,
            close_requests,
        },
        pane_focus_capability.expect("the pane-focus frame returns one capability"),
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

fn splitter_junction_weights(dockspace: &Dockspace) -> Option<(Vec<f32>, Vec<Vec<f32>>)> {
    let root = dockspace.view().root(ROOT)?.content()?.split()?;
    let root_weights = root.weights().collect();
    let column_weights = root
        .children()
        .map(|child| child.split().map(|split| split.weights().collect()))
        .collect::<Option<Vec<_>>>()?;
    Some((root_weights, column_weights))
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

fn rect_with_fill(output: &egui::FullOutput, fill: egui::Color32) -> Option<Rect> {
    output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Rect(rect) if rect.fill == fill => Some(rect.rect),
        _ => None,
    })
}

fn last_rect_fill_index(output: &egui::FullOutput, fills: &[egui::Color32]) -> Option<usize> {
    output
        .shapes
        .iter()
        .enumerate()
        .filter_map(|(index, shape)| match &shape.shape {
            egui::Shape::Rect(rect) if fills.contains(&rect.fill) => Some(index),
            _ => None,
        })
        .next_back()
}

fn rect_fill_count_in(output: &egui::FullOutput, fill: egui::Color32, bounds: Rect) -> usize {
    output
        .shapes
        .iter()
        .filter(|shape| {
            matches!(
                &shape.shape,
                egui::Shape::Rect(rect)
                    if rect.fill == fill && bounds.contains(rect.rect.center())
            )
        })
        .count()
}

fn line_segment_count_in(output: &egui::FullOutput, color: egui::Color32, bounds: Rect) -> usize {
    output
        .shapes
        .iter()
        .filter(|shape| {
            matches!(
                &shape.shape,
                egui::Shape::LineSegment { points, stroke }
                    if stroke.color == color
                        && points.iter().all(|point| bounds.contains(*point))
            )
        })
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
fn default_visuals_follow_the_current_egui_theme() {
    let context = Context::default();
    let dark_panel = egui::Color32::from_rgb(17, 23, 31);
    let dark_bar = egui::Color32::from_rgb(5, 7, 11);
    let mut dark = egui::Visuals::dark();
    dark.panel_fill = dark_panel;
    dark.extreme_bg_color = dark_bar;
    context.set_visuals(dark);

    let mut dockspace = Dockspace::builder("product-theme-following", layout())
        .build()
        .expect("the themed product facade initializes");
    let mut panes = Panes;
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let dark_output = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(rect_fill_count(&dark_output.output, dark_panel) > 0);
    assert!(rect_fill_count(&dark_output.output, dark_bar) > 0);

    let light_panel = egui::Color32::from_rgb(241, 244, 248);
    let light_bar = egui::Color32::from_rgb(211, 217, 225);
    let mut light = egui::Visuals::light();
    light.panel_fill = light_panel;
    light.extreme_bg_color = light_bar;
    let expected_light_bar: egui::Color32 =
        (egui::Rgba::from(light_panel) * egui::Rgba::from_gray(0.8)).into();
    context.set_visuals(light);
    let light_output = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(rect_fill_count(&light_output.output, light_panel) > 0);
    assert!(rect_fill_count(&light_output.output, expected_light_bar) > 0);
    assert_eq!(rect_fill_count(&light_output.output, dark_panel), 0);
    assert_eq!(rect_fill_count(&light_output.output, dark_bar), 0);
}

#[test]
fn contained_panes_use_the_egui_window_fill() {
    let context = Context::default();
    let panel_fill = egui::Color32::from_rgb(23, 31, 43);
    let window_fill = egui::Color32::from_rgb(67, 79, 101);
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = panel_fill;
    visuals.window_fill = window_fill;
    context.set_visuals(visuals);
    let mut dockspace = Dockspace::builder("product-contained-window-fill", contained_layout())
        .build()
        .expect("the themed contained facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let ready = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let contained_content = |rect: &egui::epaint::RectShape| {
        rect.rect.min.x >= 500.0 && rect.rect.min.y > 280.0 && rect.rect.width() > 200.0
    };

    assert!(ready.output.shapes.iter().any(|shape| {
        matches!(
            &shape.shape,
            egui::Shape::Rect(rect) if rect.fill == window_fill && contained_content(rect)
        )
    }));
    assert!(!ready.output.shapes.iter().any(|shape| {
        matches!(
            &shape.shape,
            egui::Shape::Rect(rect) if rect.fill == panel_fill && contained_content(rect)
        )
    }));
}

#[test]
fn frontmost_contained_title_uses_the_egui_open_window_visual() {
    let context = Context::default();
    let window_fill = egui::Color32::from_rgb(45, 55, 72);
    let active_title_fill = egui::Color32::from_rgb(121, 77, 148);
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = window_fill;
    visuals.widgets.open.weak_bg_fill = active_title_fill;
    context.set_visuals(visuals);
    let mut dockspace =
        Dockspace::builder("product-contained-active-title", stacked_contained_layout())
            .build()
            .expect("the stacked contained facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let ready = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let front = dockspace
        .view()
        .contained(FRONT_FLOATING)
        .expect("the front contained window remains present")
        .rect();
    let border = dockspace.style().floating_border_width;
    let title = Rect::from_min_size(
        Pos2::new(
            front.min().x() as f32 + border,
            front.min().y() as f32 + border,
        ),
        vec2(
            front.width() as f32 - 2.0 * border,
            dockspace.style().floating_title_height,
        ),
    );

    assert_eq!(rect_fill_count(&ready.output, active_title_fill), 1);
    assert!(
        rect_with_fill(&ready.output, active_title_fill)
            .is_some_and(|rect| title.contains(rect.center())),
        "only the core-frontmost contained title receives egui's open-window fill",
    );
}

#[test]
fn visual_override_wins_without_detaching_unspecified_theme_tokens() {
    let context = Context::default();
    let panel = egui::Color32::from_rgb(36, 41, 47);
    let bar = egui::Color32::from_rgb(9, 13, 17);
    let workspace_override = egui::Color32::from_rgb(71, 37, 91);
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = panel;
    visuals.extreme_bg_color = bar;
    context.set_visuals(visuals);
    let mut style = DockStyle::default();
    style.visuals.workspace_fill = Some(workspace_override);
    let mut dockspace = Dockspace::builder("product-theme-override", layout())
        .style(style)
        .build()
        .expect("the overridden product facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let ready = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(rect_fill_count(&ready.output, workspace_override) > 0);
    assert!(rect_fill_count(&ready.output, bar) > 0);
    assert!(rect_fill_count(&ready.output, panel) > 0);
}

#[test]
fn default_splitter_visuals_follow_egui_response_state() {
    let context = Context::default();
    let idle = egui::Color32::from_rgb(19, 29, 39);
    let hovered = egui::Color32::from_rgb(61, 127, 149);
    let active = egui::Color32::from_rgb(191, 97, 53);
    let mut visuals = egui::Visuals::dark();
    visuals.extreme_bg_color = idle;
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(2.0, hovered);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(3.0, active);
    context.set_visuals(visuals);
    let mut dockspace = Dockspace::builder("product-response-splitter", split_layout())
        .build()
        .expect("the response-driven splitter facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let splitter = splitter_rect(&stable.output, idle, hovered);
    let point = splitter.center();
    let hovered_output = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(point)],
    );
    assert!(rect_fill_count(&hovered_output.output, hovered) > 0);

    let active_output = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![pointer_button(point, true)],
    );
    assert!(rect_fill_count(&active_output.output, active) > 0);
}

#[test]
fn default_close_visuals_use_egui_interaction_strokes() {
    let context = Context::default();
    context.enable_accesskit();
    let idle = egui::Color32::from_rgb(31, 47, 59);
    let hovered = egui::Color32::from_rgb(79, 143, 167);
    let active = egui::Color32::from_rgb(211, 101, 67);
    let mut visuals = egui::Visuals::dark();
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, idle);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(2.0, hovered);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(3.0, active);
    context.set_visuals(visuals);
    let mut dockspace = Dockspace::builder("product-response-close", layout())
        .build()
        .expect("the response-driven close facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let close = node_rect(&stable.output, Role::Button, "Close First");
    assert!(line_segment_count_in(&stable.output, idle, close.expand(4.0)) >= 2);

    let hovered_output = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(close.center())],
    );
    assert!(line_segment_count_in(&hovered_output.output, hovered, close.expand(4.0)) >= 2);

    let active_output = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![pointer_button(close.center(), true)],
    );
    assert!(line_segment_count_in(&active_output.output, active, close.expand(4.0)) >= 2);
}

#[test]
fn selected_tab_text_override_does_not_leak_into_close_interaction() {
    let context = Context::default();
    context.enable_accesskit();
    let tab_text = egui::Color32::from_rgb(43, 157, 181);
    let selected_text = egui::Color32::from_rgb(229, 83, 61);
    let hovered_text = egui::Color32::from_rgb(103, 211, 139);
    let mut visuals = egui::Visuals::dark();
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(2.0, hovered_text);
    context.set_visuals(visuals);
    let mut style = DockStyle::default();
    style.visuals.tab_text_color = Some(tab_text);
    style.visuals.tab_active_text_color = Some(selected_text);
    let mut dockspace = Dockspace::builder("product-close-color-roles", layout())
        .style(style)
        .build()
        .expect("the role-specific close facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let close = node_rect(&stable.output, Role::Button, "Close First");
    let hovered = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(close.center())],
    );

    assert!(line_segment_count_in(&hovered.output, hovered_text, close.expand(4.0)) >= 2);
    assert_eq!(
        line_segment_count_in(&hovered.output, tab_text, close.expand(4.0)),
        0,
        "the idle tab text override must not replace egui's hovered control stroke",
    );
    assert_eq!(
        line_segment_count_in(&hovered.output, selected_text, close.expand(4.0)),
        0,
        "selected-tab text color is not a generic hovered-control color",
    );
}

#[test]
fn inactive_tab_close_hides_only_its_paint_until_emphasized() {
    let context = Context::default();
    context.enable_accesskit();
    let idle = egui::Color32::from_rgb(31, 47, 59);
    let hovered = egui::Color32::from_rgb(79, 143, 167);
    let mut visuals = egui::Visuals::dark();
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, idle);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(2.0, hovered);
    context.set_visuals(visuals);
    let mut dockspace = Dockspace::builder("product-inactive-close-visibility", layout())
        .build()
        .expect("the close visibility facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let close = node_rect(&stable.output, Role::Button, "Close Second");
    assert_eq!(
        line_segment_count_in(&stable.output, idle, close.expand(4.0)),
        0,
        "the inactive close keeps its exact response bounds without idle icon noise"
    );

    let hover = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(close.center())],
    );
    assert!(line_segment_count_in(&hover.output, hovered, close.expand(4.0)) >= 2);
    assert_eq!(
        node_rect(&hover.output, Role::Button, "Close Second"),
        close,
        "paint visibility must not change the reserved close geometry"
    );
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
fn default_features_contained_dock_back_supports_pointer_accesskit_and_keyboard() {
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

        assert!(activated.close_requests.is_empty(), "{name}");
        assert!(dockspace.view().contained(FLOATING).is_none(), "{name}");
        assert!(dockspace.view().root(FLOATING_ROOT).is_none(), "{name}");
        let tabs = dockspace
            .view()
            .root(ROOT)
            .and_then(|root| root.content())
            .and_then(|content| content.tabs())
            .expect("the contained pane docks into the current main root");
        assert_eq!(tabs.items(), &[FIRST, SECOND], "{name}");
        assert_eq!(tabs.selected(), Some(SECOND), "{name}");
    }
}

#[test]
fn presentation_menu_floats_and_docks_back_with_truthful_native_availability() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-presentation-menu", layout())
        .build()
        .expect("the presentation-menu facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (menu, menu_node) = accesskit_node(&stable.output, Role::Button, "Presentation commands");
    assert!(!menu_node.is_disabled());

    let _sizing = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(menu, Action::Click)],
    );
    let opened = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (float, float_node) = accesskit_node(&opened.output, Role::MenuItem, "Float");
    let (_, native_node) = accesskit_node(&opened.output, Role::MenuItem, "Move to New Window");
    let (_, dock_back_node) = accesskit_node(&opened.output, Role::MenuItem, "Dock Back");
    assert!(
        !float_node.is_disabled(),
        "{float_node:#?}; description={:?}",
        float_node.description()
    );
    assert!(native_node.is_disabled());
    assert!(dock_back_node.is_disabled());

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(float, Action::Click)],
    );
    assert!(
        dockspace
            .view()
            .item(SECOND)
            .and_then(|item| item.contained())
            .is_some(),
        "Float must move the complete root into an egui-native contained window",
    );

    let floating = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (menu, _) = accesskit_node(&floating.output, Role::Button, "Presentation commands");
    let _sizing = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(menu, Action::Click)],
    );
    let opened = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (_, float_node) = accesskit_node(&opened.output, Role::MenuItem, "Float");
    let (dock_back, dock_back_node) = accesskit_node(&opened.output, Role::MenuItem, "Dock Back");
    assert!(float_node.is_disabled());
    assert!(!dock_back_node.is_disabled());

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(dock_back, Action::Click)],
    );
    assert!(
        dockspace
            .view()
            .item(SECOND)
            .and_then(|item| item.contained())
            .is_none(),
        "Dock Back must recover the complete root into the dockspace",
    );
}

#[test]
fn open_presentation_menu_closes_immediately_when_ui_becomes_disabled() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-open-disabled-presentation-menu", layout())
        .build()
        .expect("the presentation-menu facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (menu, _) = accesskit_node(&stable.output, Role::Button, "Presentation commands");
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(menu, Action::Click)],
    );
    let opened = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (_, opened_anchor) = accesskit_node(&opened.output, Role::Button, "Presentation commands");
    assert_eq!(opened_anchor.is_expanded(), Some(true));
    let _ = accesskit_node(&opened.output, Role::MenuItem, "Float");

    let disabled = run_disabled_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (_, disabled_anchor) =
        accesskit_node(&disabled.output, Role::Button, "Presentation commands");
    assert!(disabled_anchor.is_disabled());
    assert_eq!(
        disabled_anchor.is_expanded(),
        Some(false),
        "the disabled anchor must synchronously report its popup as collapsed",
    );
    let tree = disabled
        .output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit is enabled");
    assert!(
        tree.nodes
            .iter()
            .all(|(_, node)| node.role() != Role::MenuItem),
        "the popup must stop rendering as soon as its anchor becomes disabled",
    );
}

#[test]
fn open_main_presentation_menu_closes_immediately_when_anchor_becomes_occluded() {
    let context = Context::default();
    context.enable_accesskit();
    let initial_rect = LogicalRect::new(40.0, 300.0, 180.0, 180.0)
        .expect("the initial contained rectangle is valid");
    let mut dockspace = Dockspace::builder(
        "product-open-occluded-presentation-menu",
        contained_layout_at(initial_rect),
    )
    .build()
    .expect("the presentation-menu occlusion facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable_tree = stable
        .output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit is enabled");
    let (main_menu, main_anchor) = stable_tree
        .nodes
        .iter()
        .find_map(|(id, node)| {
            let is_main_anchor = node.role() == Role::Button
                && node.label() == Some("Presentation commands")
                && node.bounds().is_some_and(|bounds| bounds.x0 > 600.0);
            is_main_anchor.then_some((*id, node))
        })
        .expect("the unobscured main root exposes its presentation menu");
    assert!(!main_anchor.is_disabled());

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(main_menu, Action::Click)],
    );
    let opened = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let opened_tree = opened
        .output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit is enabled");
    let opened_anchor = opened_tree
        .nodes
        .iter()
        .find_map(|(id, node)| (*id == main_menu).then_some(node))
        .expect("the opened main presentation anchor remains stable");
    assert_eq!(opened_anchor.is_expanded(), Some(true));
    let _ = accesskit_node(&opened.output, Role::MenuItem, "Float");

    dockspace
        .set_contained_rect_current(
            SECOND,
            LogicalRect::new(620.0, 0.0, 180.0, 180.0)
                .expect("the occluding contained rectangle is valid"),
        )
        .expect("the contained window moves over the main presentation anchor");
    let occluded = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let occluded_tree = occluded
        .output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit is enabled");
    let occluded_anchor = occluded_tree
        .nodes
        .iter()
        .find_map(|(id, node)| (*id == main_menu).then_some(node))
        .expect("the occluded main presentation anchor remains described");
    assert!(occluded_anchor.is_disabled());
    assert_eq!(
        occluded_anchor.is_expanded(),
        Some(false),
        "the occluded anchor must synchronously report its popup as collapsed",
    );
    assert!(
        occluded_tree
            .nodes
            .iter()
            .all(|(_, node)| node.role() != Role::MenuItem),
        "the stale popup must not remain above or intercept the front contained window",
    );
    assert!(
        occluded_tree.nodes.iter().any(|(id, node)| {
            *id != main_menu
                && node.role() == Role::Button
                && node.label() == Some("Presentation commands")
                && !node.is_disabled()
        }),
        "the front contained window keeps its own presentation anchor operable",
    );
}

#[test]
fn discarded_presentation_menu_activation_commits_exactly_once() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let mut dockspace = Dockspace::builder("product-discarded-presentation-menu", layout())
        .build()
        .expect("the multipass presentation-menu facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (menu, _) = accesskit_node(&stable.output, Role::Button, "Presentation commands");
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(menu, Action::Click)],
    );
    let opened = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (float, float_node) = accesskit_node(&opened.output, Role::MenuItem, "Float");
    assert!(!float_node.is_disabled());

    context
        .plugin_or_default::<LateDiscardPlugin>()
        .lock()
        .remaining = 2;
    let passes = run_two_discarded_dockspace_passes(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(float, Action::Click)],
    );
    assert_eq!(passes, 3, "the terminal pass intentionally omits dockspace");
    let surface = dockspace
        .view()
        .surface(SURFACE)
        .expect("the logical surface remains present");
    assert_eq!(
        surface.contained().count(),
        1,
        "discarded passes must retain the local command once without replaying it"
    );
    assert!(surface.main_root().is_none());
}

#[test]
fn disabled_ui_does_not_open_or_activate_presentation_commands() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-disabled-presentation-menu", layout())
        .build()
        .expect("the disabled presentation-menu facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (menu, _) = accesskit_node(&stable.output, Role::Button, "Presentation commands");
    let disabled = run_disabled_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(menu, Action::Click)],
    );
    let (_, node) = accesskit_node(&disabled.output, Role::Button, "Presentation commands");
    assert!(node.is_disabled());
    assert!(!node.supports_action(Action::Click));
    assert!(
        dockspace
            .view()
            .item(SECOND)
            .and_then(|item| item.contained())
            .is_none(),
        "a delayed accessibility click must not float content through a disabled Ui",
    );

    let after = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let tree = after
        .output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit is enabled");
    assert!(
        tree.nodes
            .iter()
            .all(|(_, node)| node.role() != Role::MenuItem),
        "the disabled anchor must not leave its popup open",
    );
}

#[test]
fn fully_occluded_main_presentation_menu_is_accessibility_disabled() {
    let context = Context::default();
    context.enable_accesskit();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs([SECOND])),
        LogicalRect::new(620.0, 0.0, 180.0, 180.0)
            .expect("the occluding contained rectangle is valid"),
    ))])
    .expect("the presentation-menu occlusion layout is valid");
    let mut dockspace = Dockspace::builder("product-occluded-presentation-menu", layout)
        .build()
        .expect("the occluded presentation-menu facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let tree = stable
        .output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit is enabled");
    let anchors = tree
        .nodes
        .iter()
        .filter(|(_, node)| {
            node.role() == Role::Button && node.label() == Some("Presentation commands")
        })
        .collect::<Vec<_>>();
    assert_eq!(anchors.len(), 2);
    let (occluded_id, occluded) = anchors
        .iter()
        .copied()
        .find(|(_, node)| node.is_disabled())
        .expect("the main anchor under the front contained window is disabled");
    assert!(!occluded.supports_action(Action::Click));
    assert!(
        anchors.iter().any(|(_, node)| !node.is_disabled()),
        "the front contained title keeps its own command menu operable",
    );

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(*occluded_id, Action::Click)],
    );
    assert!(
        dockspace
            .view()
            .item(FIRST)
            .and_then(|item| item.contained())
            .is_none(),
        "an occluded main command anchor must not execute through the front window",
    );
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
fn fully_occluded_rear_contained_chrome_is_accessibility_disabled() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder(
        "product-fully-occluded-contained-chrome",
        fully_occluded_stacked_contained_layout(),
    )
    .build()
    .expect("the fully occluded contained facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let tree = stable
        .output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit is enabled");
    let (rear_title_id, rear_title) = accesskit_node(&stable.output, Role::TitleBar, "Second");
    let (rear_close_id, rear_close) =
        accesskit_node(&stable.output, Role::Button, "Close floating Second");
    let (_, front_title) = accesskit_node(&stable.output, Role::TitleBar, "Fourth");
    let (_, front_close) = accesskit_node(&stable.output, Role::Button, "Close floating Fourth");
    let right_edges = tree
        .nodes
        .iter()
        .filter(|(_, node)| {
            node.role() == Role::Splitter && node.label() == Some("Resize floating right edge")
        })
        .collect::<Vec<_>>();
    assert_eq!(right_edges.len(), 2);
    let (rear_right_id, rear_right) = right_edges
        .iter()
        .copied()
        .min_by(|(_, left), (_, right)| {
            left.bounds()
                .expect("the rear resize edge has bounds")
                .x1
                .total_cmp(&right.bounds().expect("the front resize edge has bounds").x1)
        })
        .expect("the rear resize edge is described");
    let (_, front_right) = right_edges
        .iter()
        .copied()
        .max_by(|(_, left), (_, right)| {
            left.bounds()
                .expect("the rear resize edge has bounds")
                .x1
                .total_cmp(&right.bounds().expect("the front resize edge has bounds").x1)
        })
        .expect("the front resize edge is described");

    for node in [rear_title, rear_close, rear_right] {
        assert!(node.is_disabled());
        assert!(!node.supports_action(Action::Focus));
    }
    assert!(!rear_close.supports_action(Action::Click));
    assert!(!rear_right.supports_action(Action::Increment));
    assert!(!rear_right.supports_action(Action::Decrement));
    assert!(!front_title.is_disabled());
    assert!(!front_close.is_disabled());
    assert!(front_close.supports_action(Action::Click));
    assert!(front_right.supports_action(Action::Increment));
    assert!(front_right.supports_action(Action::Decrement));

    let before_rect = contained_rect(&dockspace);
    let before_roster = dockspace
        .view()
        .surface(SURFACE)
        .expect("the stacked surface remains present")
        .contained()
        .map(|contained| contained.id())
        .collect::<Vec<_>>();
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            accesskit_action(*rear_right_id, Action::Increment),
            accesskit_action(rear_title_id, Action::Focus),
        ],
    );
    assert_eq!(contained_rect(&dockspace), before_rect);
    assert_eq!(
        dockspace
            .view()
            .surface(SURFACE)
            .expect("the stacked surface remains present")
            .contained()
            .map(|contained| contained.id())
            .collect::<Vec<_>>(),
        before_roster,
    );
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(rear_close_id, Action::Click)],
    );
    assert!(dockspace.view().contained(FLOATING).is_some());
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
    let style = DockStyle {
        tab_min_width: 220.0,
        tab_max_width: 220.0,
        ..DockStyle::default()
    };
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
    let style = DockStyle {
        tab_min_width: 220.0,
        tab_max_width: 220.0,
        ..DockStyle::default()
    };
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
fn tab_drag_uses_egui_grab_cursors() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-tab-drag-cursors", layout())
        .build()
        .expect("the product facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let tab = node_rect(&stable.output, Role::Tab, "First");
    let source = tab.center();
    let hovered = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source)],
    );
    assert_eq!(
        hovered.output.platform_output.cursor_icon,
        egui::CursorIcon::Grab
    );

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![pointer_button(source, true)],
    );
    let dragging = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source + vec2(12.0, 0.0))],
    );
    assert_eq!(
        dragging.output.platform_output.cursor_icon,
        egui::CursorIcon::Grabbing
    );
}

#[test]
fn tab_accessibility_bounds_match_the_exact_drag_receiver() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-tab-accessibility-drag-bounds", layout())
        .build()
        .expect("the product facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let tab = node_rect(&stable.output, Role::Tab, "First");
    let close = node_rect(&stable.output, Role::Button, "Close First");

    assert!(tab.is_positive());
    assert!(close.is_positive());
    assert!(
        !tab.intersect(close).is_positive(),
        "the Tab semantic hit bounds must exclude its disjoint close receiver; tab={tab:?}, close={close:?}"
    );
}

#[test]
fn active_tab_drag_leaves_an_accessible_gap_and_paints_one_pointer_ghost() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let tab_fill = egui::Color32::from_rgb(37, 73, 109);
    let active_fill = egui::Color32::from_rgb(53, 101, 149);
    let ghost_fill = egui::Color32::from_rgba_unmultiplied(219, 139, 57, 96);
    let ghost_offset = vec2(17.0, 11.0);
    let mut style = DockStyle::default();
    style.visuals.tab_fill = Some(tab_fill);
    style.visuals.tab_active_fill = Some(active_fill);
    style.visuals.ghost_fill = Some(ghost_fill);
    style.ghost_offset = ghost_offset;
    let mut dockspace = Dockspace::builder("product-tab-drag-decoration", layout())
        .style(style)
        .build()
        .expect("the decorated drag facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source_rect = node_rect(&stable.output, Role::Tab, "First");
    let source = source_rect.center();
    assert!(rect_fill_count_in(&stable.output, active_fill, source_rect) > 0);

    let armed = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    assert!(
        rect_fill_count_in(&armed.output, active_fill, source_rect) > 0,
        "an armed press must not hide the source tab"
    );
    assert_eq!(rect_fill_count(&armed.output, ghost_fill), 0);

    let dragged = source + vec2(24.0, 8.0);
    let dragging = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(dragged)],
    );
    assert_eq!(
        dragging.output.platform_output.num_completed_passes, 2,
        "the threshold-crossing pass is discarded so the final output uses core drag state"
    );
    assert_eq!(
        rect_fill_count_in(&dragging.output, active_fill, source_rect),
        0,
        "the active source tab must become a real visual gap"
    );
    assert_eq!(
        rect_fill_count(&dragging.output, ghost_fill),
        1,
        "one non-interactive pointer ghost represents the dragged tab"
    );
    let ghost = rect_with_fill(&dragging.output, ghost_fill).expect("the tab ghost is painted");
    assert!(
        ghost.min.distance(dragged + ghost_offset) <= 0.01,
        "the pointer ghost follows the current pointer plus the configured style offset"
    );
    let _ = accesskit_node(&dragging.output, Role::Tab, "First");
    let _ = accesskit_node(&dragging.output, Role::Button, "Close First");
    assert_eq!(
        dragging.output.platform_output.cursor_icon,
        egui::CursorIcon::Grabbing
    );

    let _ = run_frame(&context, &mut dockspace, &mut panes, key_press(Key::Escape));
    let cancelled = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(rect_fill_count_in(&cancelled.output, active_fill, source_rect) > 0);
    assert_eq!(rect_fill_count(&cancelled.output, ghost_fill), 0);
}

#[test]
fn rejected_drag_begin_requests_only_one_settlement_pass() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let active_fill = egui::Color32::from_rgb(61, 109, 157);
    let ghost_fill = egui::Color32::from_rgba_unmultiplied(231, 145, 53, 96);
    let mut style = DockStyle::default();
    style.visuals.tab_fill = Some(active_fill);
    style.visuals.tab_active_fill = Some(active_fill);
    style.visuals.tab_hover_fill = Some(active_fill);
    style.visuals.ghost_fill = Some(ghost_fill);
    let mut policy = dockspace::policy::DockPolicy::default();
    let mut first = dockspace::policy::DockItemRule::new();
    first.set_source_enabled(false);
    policy.set_item_rule(FIRST, first);
    let mut dockspace = Dockspace::builder("product-rejected-tab-drag-decoration", layout())
        .style(style)
        .policy(policy)
        .build()
        .expect("the source-disabled drag facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source_rect = node_rect(&stable.output, Role::Tab, "First");
    let source = source_rect.center();
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    let rejected = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source + vec2(24.0, 8.0))],
    );

    assert_eq!(
        rejected.output.platform_output.num_completed_passes, 2,
        "a rejected Begin is marked per logical frame instead of discarding until max_passes"
    );
    assert!(rect_fill_count_in(&rejected.output, active_fill, source_rect) > 0);
    assert_eq!(rect_fill_count(&rejected.output, ghost_fill), 0);
}

#[test]
fn unused_tab_bar_space_starts_the_exact_group_drag() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let group_fill = egui::Color32::from_rgb(71, 107, 157);
    let ghost_fill = egui::Color32::from_rgba_unmultiplied(227, 151, 61, 96);
    let mut style = instrumented_style();
    style.visuals.tab_active_fill = Some(group_fill);
    style.visuals.ghost_fill = Some(ghost_fill);
    let mut dockspace = Dockspace::builder("product-tab-group-trailing-drag", contained_layout())
        .style(style)
        .build()
        .expect("the product facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let second = node_rect(&stable.output, Role::Tab, "Second");
    let source = Pos2::new((second.max.x + 80.0).min(760.0), second.center().y);
    assert!(
        source.x > second.max.x,
        "the fixture must leave a positive trailing tab-bar region"
    );

    let hovered = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source)],
    );
    assert_eq!(
        hovered.output.platform_output.cursor_icon,
        egui::CursorIcon::Grab,
        "unused tab-bar space must expose the same group drag affordance as the leading grip"
    );

    let armed = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![pointer_button(source, true)],
    );
    assert!(rect_fill_count_in(&armed.output, group_fill, second) > 0);
    assert_eq!(rect_fill_count(&armed.output, ghost_fill), 0);
    let dragging = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source + vec2(12.0, 0.0))],
    );
    assert_eq!(
        dragging.output.platform_output.cursor_icon,
        egui::CursorIcon::Grabbing
    );
    assert_eq!(
        dragging.output.platform_output.num_completed_passes, 2,
        "the threshold-crossing pass is discarded so the final output uses core group state"
    );
    assert_eq!(
        rect_fill_count_in(&dragging.output, group_fill, second),
        0,
        "the terminal threshold-crossing output already contains the group source gap"
    );
    assert_eq!(rect_fill_count(&dragging.output, ghost_fill), 1);

    let passive = dockspace
        .style()
        .visuals
        .drop_guide_fill
        .expect("the fixture fixes its passive guide color");
    let active = dockspace
        .style()
        .visuals
        .drop_guide_active_fill
        .expect("the fixture fixes its active guide color");
    let preview = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(Pos2::new(400.0, 528.0))],
    );
    assert!(
        guide_button_count(&preview.output, passive, active) >= 4,
        "the trailing region must enter the same core-owned group drag as the leading grip"
    );
    assert_eq!(
        rect_fill_count_in(&preview.output, group_fill, second),
        0,
        "an active group drag must omit the source tab-stack paint"
    );
    assert_eq!(
        rect_fill_count(&preview.output, ghost_fill),
        1,
        "one non-interactive pointer ghost represents the dragged group"
    );
    let ghost = rect_with_fill(&preview.output, ghost_fill).expect("the group ghost is painted");
    assert!(ghost.width() <= 280.0);
    assert!(
        ghost.height() <= 64.0,
        "a group ghost stays header-sized rather than copying the whole pane"
    );
    let guide_index = last_rect_fill_index(&preview.output, &[passive, active])
        .expect("the active group drag paints guide shapes");
    let ghost_index = last_rect_fill_index(&preview.output, &[ghost_fill])
        .expect("the active group drag paints a ghost shape");
    assert!(
        ghost_index > guide_index,
        "the pointer ghost remains above every target preview and guide"
    );
    let _ = accesskit_node(&preview.output, Role::Tab, "Second");
    let _ = accesskit_node(&preview.output, Role::Button, "Close Second");
    let tree = preview
        .output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("AccessKit is enabled");
    let pane_label = tree
        .nodes
        .iter()
        .find_map(|(_, node)| {
            (node.role() == Role::Label && node.value() == Some("Pane 2")).then_some(node)
        })
        .expect("the transparent pane keeps its application accessibility node");
    assert!(
        !pane_label.is_disabled(),
        "paint omission must not disable application controls or surrender pane focus"
    );
}

#[test]
fn contained_title_drag_settles_root_gap_and_compact_ghost_in_the_same_run() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let source_fill = egui::Color32::from_rgb(83, 59, 137);
    let ghost_fill = egui::Color32::from_rgba_unmultiplied(235, 157, 67, 96);
    let mut style = DockStyle::default();
    style.visuals.floating_fill = Some(source_fill);
    style.visuals.ghost_fill = Some(ghost_fill);
    let mut dockspace = Dockspace::builder("product-contained-drag-decoration", contained_layout())
        .style(style)
        .build()
        .expect("the decorated contained facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source_bounds = contained_rect(&dockspace);
    let source_rect = Rect::from_min_max(
        Pos2::new(
            source_bounds.min().x() as f32,
            source_bounds.min().y() as f32,
        ),
        Pos2::new(
            source_bounds.max().x() as f32,
            source_bounds.max().y() as f32,
        ),
    );
    let source = contained_title_point(&dockspace);
    assert!(rect_fill_count_in(&stable.output, source_fill, source_rect) > 0);

    let armed = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    assert!(rect_fill_count_in(&armed.output, source_fill, source_rect) > 0);
    assert_eq!(rect_fill_count(&armed.output, ghost_fill), 0);

    let dragging = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source + vec2(24.0, 8.0))],
    );
    assert_eq!(dragging.output.platform_output.num_completed_passes, 2);
    assert_eq!(
        rect_fill_count_in(&dragging.output, source_fill, source_rect),
        0,
        "the terminal threshold-crossing output contains a real contained-root gap"
    );
    assert_eq!(rect_fill_count(&dragging.output, ghost_fill), 1);
    let ghost = rect_with_fill(&dragging.output, ghost_fill).expect("the root ghost is painted");
    assert!(ghost.width() <= 280.0);
    assert!(ghost.height() <= 64.0);
    let _ = accesskit_node(&dragging.output, Role::TitleBar, "Second");
    let _ = accesskit_node(&dragging.output, Role::Button, "Close floating Second");
    assert_eq!(
        dragging.output.platform_output.cursor_icon,
        egui::CursorIcon::Grabbing
    );
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
    let released = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(first), pointer_button(first, false)],
    );
    let _ = accesskit_node(&released.output, Role::Tab, "First");
    let _ = accesskit_node(&released.output, Role::Tab, "Second");
    assert_eq!(
        tab_items(&dockspace),
        vec![SECOND, FIRST],
        "release commits the preview painted by the preceding terminal pass",
    );
    let settled = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = accesskit_node(&settled.output, Role::Tab, "First");
    let _ = accesskit_node(&settled.output, Role::Tab, "Second");
    assert!(settled.output.platform_output.num_completed_passes >= 2);
    assert_eq!(
        tab_items(&dockspace),
        vec![SECOND, FIRST],
        "the follow-up preserves the already committed exact preview result",
    );
}

#[test]
fn rear_contained_selected_tab_press_raises_before_any_drag_motion() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let mut dockspace = Dockspace::builder(
        "product-contained-first-effective-press",
        stacked_contained_layout(),
    )
    .build()
    .expect("the stacked contained facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = node_center(&stable.output, Role::Tab, "Second");
    let pressed = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );

    assert!(pressed.output.platform_output.num_completed_passes >= 2);
    let _ = accesskit_node(&pressed.output, Role::TitleBar, "Second");
    let _ = accesskit_node(&pressed.output, Role::TitleBar, "Fourth");
    assert_eq!(
        dockspace
            .view()
            .surface(SURFACE)
            .expect("the stacked surface remains present")
            .contained()
            .map(|contained| contained.id())
            .collect::<Vec<_>>(),
        [FRONT_FLOATING, FLOATING],
        "the selected rear tab raises its complete contained root on the initial press",
    );
}

#[test]
fn rear_contained_child_press_uses_event_time_position_even_when_released_in_the_same_frame() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let mut dockspace = Dockspace::builder(
        "product-contained-child-first-effective-press",
        stacked_contained_layout(),
    )
    .build()
    .expect("the stacked contained facade initializes");
    let mut panes = FocusPanes::new("contained-child-first-effective-press", true);

    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = panes.target_rect().center();
    let released_at = contained_title_point(&dockspace);
    let (pressed, _) = run_focus_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(source),
            pointer_button(source, true),
            Event::PointerMoved(released_at),
            pointer_button(released_at, false),
        ],
    );

    assert!(pressed.output.platform_output.num_completed_passes >= 2);
    assert_eq!(
        dockspace
            .view()
            .surface(SURFACE)
            .expect("the stacked surface remains present")
            .contained()
            .map(|contained| contained.id())
            .collect::<Vec<_>>(),
        [FRONT_FLOATING, FLOATING],
        "an application-owned child response still raises its containing presentation",
    );
    assert!(
        panes.target_requests() > 0,
        "the event-time pane hit publishes the exact pane focus request"
    );
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
            Pos2::new(72.0, 300.0),
            DockspaceAxis::Horizontal,
            vec![vec![SECOND], vec![FIRST]],
        ),
        (
            "right",
            Pos2::new(728.0, 300.0),
            DockspaceAxis::Horizontal,
            vec![vec![FIRST], vec![SECOND]],
        ),
        (
            "top",
            Pos2::new(400.0, 72.0),
            DockspaceAxis::Vertical,
            vec![vec![SECOND], vec![FIRST]],
        ),
        (
            "bottom",
            Pos2::new(400.0, 528.0),
            DockspaceAxis::Vertical,
            vec![vec![FIRST], vec![SECOND]],
        ),
    ] {
        let context = Context::default();
        context.enable_accesskit();
        let mut dockspace = Dockspace::builder(("product-guide", name), split_layout())
            .style(instrumented_style())
            .build()
            .expect("the product guide facade initializes");
        let mut panes = Panes;

        let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let source = tab_center(&stable.output, "Second");
        let passive = dockspace
            .style()
            .visuals
            .drop_guide_fill
            .expect("the fixture fixes its passive guide color");
        let active = dockspace
            .style()
            .visuals
            .drop_guide_active_fill
            .expect("the fixture fixes its active guide color");

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
        .style(instrumented_style())
        .build()
        .expect("the product guide facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = tab_center(&stable.output, "Second");
    let first = tab_center(&stable.output, "First");
    let passive = dockspace
        .style()
        .visuals
        .drop_guide_fill
        .expect("the fixture fixes its passive guide color");
    let active = dockspace
        .style()
        .visuals
        .drop_guide_active_fill
        .expect("the fixture fixes its active guide color");

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
    let (_, guide_node) =
        accesskit_node(&cluster.output, Role::Label, "Dockspace center drop guide");
    assert!(!guide_node.supports_action(Action::Click));
    assert!(!guide_node.supports_action(Action::Focus));
    let target = node_rect(&cluster.output, Role::Label, "Dockspace center drop guide").center();
    let painted_center =
        nearest_guide_center(&cluster.output, passive, active, Pos2::new(200.0, 314.0));
    assert_eq!(target, painted_center);
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
        .style(instrumented_style())
        .build()
        .expect("the product splitter facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let ready = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let idle = dockspace
        .style()
        .visuals
        .splitter_color
        .expect("the fixture fixes its idle splitter color");
    let active = dockspace
        .style()
        .visuals
        .splitter_hover_color
        .expect("the fixture fixes its active splitter color");
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
    let initial = node_rect(&ready.output, Role::Group, "Resize pane grid");
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
    let live_junction = node_rect(&live.output, Role::Group, "Resize pane grid");

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
fn default_features_splitter_junction_exposes_independent_axis_semantics() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder(
        "product-splitter-junction-semantics",
        splitter_junction_layout(),
    )
    .build()
    .expect("the product splitter junction facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (_, group_node) = accesskit_node(&stable.output, Role::Group, "Resize pane grid");
    let (horizontal, horizontal_node) = accesskit_node(
        &stable.output,
        Role::Splitter,
        "Resize pane grid horizontally",
    );
    let (vertical, vertical_node) = accesskit_node(
        &stable.output,
        Role::Splitter,
        "Resize pane grid vertically",
    );
    assert_eq!(horizontal_node.orientation(), Some(Orientation::Vertical));
    assert_eq!(vertical_node.orientation(), Some(Orientation::Horizontal));
    assert!(group_node.children().contains(&horizontal));
    assert!(group_node.children().contains(&vertical));
    for node in [horizontal_node, vertical_node] {
        assert!(node.supports_action(Action::Focus));
        assert!(node.supports_action(Action::Increment));
        assert!(node.supports_action(Action::Decrement));
    }

    let (before_root, before_columns) =
        splitter_junction_weights(&dockspace).expect("the product junction fixture remains split");
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(horizontal, Action::Increment)],
    );
    let (horizontal_root, horizontal_columns) = splitter_junction_weights(&dockspace)
        .expect("the horizontally adjusted junction remains split");
    assert!(horizontal_root[0] > before_root[0]);
    assert_eq!(horizontal_columns, before_columns);

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(vertical, Action::Focus)],
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        key_press(Key::ArrowDown),
    );
    let (vertical_root, vertical_columns) = splitter_junction_weights(&dockspace)
        .expect("the vertically adjusted junction remains split");
    assert_eq!(vertical_root, horizontal_root);
    assert!(
        vertical_columns
            .iter()
            .zip(horizontal_columns.iter())
            .all(|(after, before)| after[0] > before[0]),
        "Down Arrow advances every vertical-axis incident splitter together"
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
        .style(instrumented_style())
        .build()
        .expect("the product drag facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = tab_center(&stable.output, "Second");
    let target = Pos2::new(400.0, 72.0);
    let passive = dockspace
        .style()
        .visuals
        .drop_guide_fill
        .expect("the fixture fixes its passive guide color");
    let active = dockspace
        .style()
        .visuals
        .drop_guide_active_fill
        .expect("the fixture fixes its active guide color");

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
        context.dragged_id(),
        None,
        "Escape must clear egui's local drag owner together with the core gesture"
    );
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
    let painted_top = Pos2::new(400.0, 72.0);
    let release_bottom = Pos2::new(400.0, 528.0);

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
    let version_before_visual_update = dockspace.version();
    let mut visual_only_style = dockspace.style().clone();
    visual_only_style.visuals.tab_active_fill = Some(egui::Color32::from_rgb(41, 83, 109));
    let visual_mutation = dockspace
        .set_style(visual_only_style)
        .expect("visual-only styling does not disturb the pending release");
    assert_eq!(dockspace.version(), version_before_visual_update);
    assert!(visual_mutation.published_state_changed());
    assert_eq!(visual_mutation.affected_surfaces(), &[SURFACE]);
    assert_eq!(
        root_split(&dockspace).map(|(axis, _)| axis),
        Some(DockspaceAxis::Horizontal),
        "adapter-only styling must not cancel the pending release",
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
    let top = Pos2::new(400.0, 72.0);
    let bottom = Pos2::new(400.0, 528.0);

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
fn pane_focus_observation_settles_only_after_the_terminal_egui_pass() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-pane-focus", layout())
        .build()
        .expect("the pane-focus product facade initializes");
    let mut panes = FocusPanes::new("product-pane-focus", true);

    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (stable, _) = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (second, _) = accesskit_node(&stable.output, Role::Tab, "Second");
    let _ = run_focus_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(second, Action::Click)],
    );
    assert_eq!(selected(&dockspace), Some(SECOND));
    assert_eq!(panes.target_requests(), 0);

    let (_, capability) = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(panes.target_requests(), 1);
    assert_eq!(capability, DockspaceCapability::Supported);
    assert!(context.memory(|memory| memory.has_focus(panes.target)));

    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(
        panes.target_requests(),
        1,
        "the terminal Focused observation must complete the exact request",
    );
}

#[test]
fn unavailable_pane_focus_binding_terminates_without_guessing_a_target() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-pane-focus-unavailable", layout())
        .build()
        .expect("the unavailable-focus product facade initializes");
    let mut panes = FocusPanes::new("product-pane-focus-unavailable", false);

    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (stable, _) = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (second, _) = accesskit_node(&stable.output, Role::Tab, "Second");
    let _ = run_focus_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(second, Action::Click)],
    );

    let (_, capability) = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(panes.target_requests(), 1);
    assert_eq!(
        capability,
        DockspaceCapability::Unavailable(DockspaceUnavailableReason::PaneFocusBindingUnavailable,),
    );
    assert!(!context.memory(|memory| memory.has_focus(panes.target)));

    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(
        panes.target_requests(),
        1,
        "an unavailable terminal observation must not replay or guess a focus target",
    );
}

#[test]
fn discarded_pane_focus_observation_cannot_complete_the_request() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let mut dockspace = Dockspace::builder("product-discarded-pane-focus", layout())
        .build()
        .expect("the discarded-focus product facade initializes");
    let mut panes = FocusPanes::new("product-discarded-pane-focus", true);

    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (stable, _) = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (second, _) = accesskit_node(&stable.output, Role::Tab, "Second");
    let _ = run_focus_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(second, Action::Click)],
    );

    context
        .plugin_or_default::<LateDiscardPlugin>()
        .lock()
        .remaining = 2;
    let passes =
        run_two_discarded_dockspace_passes(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(passes, 3);
    assert_eq!(panes.target_requests(), 2);

    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(
        panes.target_requests(),
        3,
        "discarded Focused observations must leave the exact request pending",
    );
    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(panes.target_requests(), 3);
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
    let top = Pos2::new(400.0, 72.0);
    let bottom = Pos2::new(400.0, 528.0);

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
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-contained-resize", contained_layout())
        .style(instrumented_style())
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
        rect_fill_count(
            &released.output,
            dockspace
                .style()
                .visuals
                .ghost_fill
                .expect("the fixture fixes its ghost fill"),
        ) > 0,
        "the exact contained transform preview must be painted before release settles",
    );
    let settled = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = accesskit_node(&settled.output, Role::TitleBar, "Second");
    assert!(settled.output.platform_output.num_completed_passes >= 2);
    let committed = dockspace
        .view()
        .contained(FLOATING)
        .expect("the contained fixture remains present")
        .rect();
    assert_eq!(committed.min(), initial.min());
    assert!(committed.max().x() > initial.max().x());
}

#[test]
fn rear_contained_title_drag_raises_without_a_bootstrap_mask() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder(
        "product-contained-activation-continuity",
        stacked_contained_layout(),
    )
    .build()
    .expect("the stacked contained facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = node_center(&stable.output, Role::TitleBar, "Second");
    let moved = source + vec2(24.0, 0.0);

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    let dragged = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(moved)],
    );

    let _ = accesskit_node(&dragged.output, Role::TitleBar, "Second");
    let _ = accesskit_node(&dragged.output, Role::TitleBar, "Fourth");
    assert_eq!(
        dockspace
            .view()
            .surface(SURFACE)
            .expect("the stacked surface remains present")
            .contained()
            .map(|contained| contained.id())
            .collect::<Vec<_>>(),
        [FRONT_FLOATING, FLOATING],
        "the rear contained root raises without replacing the terminal pass with bootstrap paint",
    );
}

#[test]
fn default_features_contained_title_moves_without_a_dock_target() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("product-contained-move", contained_layout())
        .style(instrumented_style())
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
        rect_fill_count(
            &released.output,
            dockspace
                .style()
                .visuals
                .drop_fill
                .expect("the fixture fixes its drop fill"),
        ) > 0,
        "the contained move preview must be painted before release settles",
    );
    let settled = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let _ = accesskit_node(&settled.output, Role::TitleBar, "Second");
    assert!(settled.output.platform_output.num_completed_passes >= 2);
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
    let target = Pos2::new(400.0, 72.0);

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
    replacement_style.visuals.tab_active_fill = Some(egui::Color32::from_rgb(12, 34, 56));

    let style_mutation = dockspace
        .set_style(replacement_style.clone())
        .expect("valid style replacement commits");
    assert_eq!(dockspace.style(), &replacement_style);
    assert!(!style_mutation.workspace_changed());
    assert!(style_mutation.published_state_changed());
    assert_eq!(style_mutation.affected_surfaces(), &[SURFACE]);

    let version_before_visual_only_style = dockspace.version();
    let mut visual_only_style = replacement_style.clone();
    visual_only_style.visuals.tab_active_fill = Some(egui::Color32::from_rgb(78, 90, 123));
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

#[test]
fn kittest_selects_and_closes_tabs_through_accessible_controls() {
    let mut harness = kittest_harness("product-kittest-tabs", layout());

    harness
        .get_by_role_and_label(Role::Tab, "Second")
        .click_accesskit();
    harness.run_steps(2);
    assert_eq!(selected(&harness.state().dockspace), Some(SECOND));

    harness
        .get_by_role_and_label(Role::Button, "Close Second")
        .click_accesskit();
    harness.run_steps(2);
    let request = harness
        .state_mut()
        .close_requests
        .pop()
        .expect("the accessible close control opens one close plan");
    let item = request.plan().items()[0];
    assert_eq!(item.item(), SECOND);
    harness
        .state_mut()
        .dockspace
        .resolve_close(request.plan().request(), item.token(), CloseDecision::Allow)
        .expect("the kittest close decision commits");
    harness.run_steps(2);

    assert!(harness.state().dockspace.view().item(SECOND).is_none());
    assert_eq!(tab_items(&harness.state().dockspace), vec![FIRST]);
}

#[test]
fn disabled_tab_bar_is_paint_only_for_pointer_keyboard_and_accessibility() {
    let context = Context::default();
    context.enable_accesskit();
    let mut policy = dockspace::policy::DockPolicy::default();
    let mut target = dockspace::policy::DockTargetRule::default();
    target.set_tab_bar(dockspace::policy::TabBarPolicy::new(
        dockspace::policy::TabBarVisibility::Visible,
        dockspace::policy::TabBarInteraction::Disabled,
    ));
    policy.set_target_rule(dockspace::policy::DockTargetRuleKey::Item(FIRST), target);
    let mut dockspace = Dockspace::builder("product-disabled-tab-bar", layout())
        .policy(policy)
        .build()
        .expect("the paint-only tab-bar facade initializes");
    let mut panes = Panes;

    let _ = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(selected(&dockspace), Some(FIRST));
    let (second_id, second_node) = accesskit_node(&stable.output, Role::Tab, "Second");
    assert!(second_node.is_disabled());
    assert!(!second_node.supports_action(Action::Focus));
    assert!(!second_node.supports_action(Action::Click));
    let second = node_rect(&stable.output, Role::Tab, "Second").center();

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(second),
            pointer_button(second, true),
            pointer_button(second, false),
        ],
    );
    assert_eq!(selected(&dockspace), Some(FIRST));

    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            accesskit_action(second_id, Action::Focus),
            accesskit_action(second_id, Action::Click),
        ],
    );
    let _ = run_frame(&context, &mut dockspace, &mut panes, key_press(Key::Enter));
    assert_eq!(selected(&dockspace), Some(FIRST));
}

#[test]
fn kittest_moves_and_resizes_contained_content_semantically() {
    let mut harness = kittest_harness("product-kittest-contained", contained_layout());
    let initial = contained_rect(&harness.state().dockspace);
    let source = harness
        .get_by_role_and_label(Role::TitleBar, "Second")
        .rect()
        .center();
    let destination = source + vec2(-80.0, 60.0);

    harness.hover_at(source);
    harness.step();
    harness.drag_at(source);
    harness.step();
    harness.hover_at(destination);
    harness.step();
    harness.drop_at(destination);
    harness.run_steps(3);

    let moved = contained_rect(&harness.state().dockspace);
    assert_eq!(moved.size(), initial.size());
    assert!((moved.min().x() - (initial.min().x() - 80.0)).abs() < 0.5);
    assert!((moved.min().y() - (initial.min().y() + 60.0)).abs() < 0.5);

    harness
        .get_by_role_and_label(Role::Splitter, "Resize floating right edge")
        .focus();
    harness.run_steps(2);
    harness.key_press(Key::ArrowRight);
    harness.run_steps(3);

    let resized = contained_rect(&harness.state().dockspace);
    assert_eq!(resized.min(), moved.min());
    assert!(resized.width() > moved.width());
    assert_eq!(resized.height(), moved.height());
}

#[test]
fn kittest_adjusts_splitter_junction_axes_independently() {
    let mut harness = kittest_harness("product-kittest-junction", splitter_junction_layout());
    let (initial_root, initial_columns) = splitter_junction_weights(&harness.state().dockspace)
        .expect("the kittest junction fixture remains split");

    harness
        .get_by_role_and_label(Role::Splitter, "Resize pane grid horizontally")
        .focus();
    harness.run_steps(2);
    harness.key_press(Key::ArrowRight);
    harness.run_steps(3);

    let (horizontal_root, horizontal_columns) =
        splitter_junction_weights(&harness.state().dockspace)
            .expect("the horizontally adjusted kittest junction remains split");
    assert!(horizontal_root[0] > initial_root[0]);
    assert_eq!(horizontal_columns, initial_columns);

    harness
        .get_by_role_and_label(Role::Splitter, "Resize pane grid vertically")
        .focus();
    harness.run_steps(2);
    harness.key_press(Key::ArrowDown);
    harness.run_steps(3);

    let (vertical_root, vertical_columns) = splitter_junction_weights(&harness.state().dockspace)
        .expect("the vertically adjusted kittest junction remains split");
    assert_eq!(vertical_root, horizontal_root);
    assert!(
        vertical_columns
            .iter()
            .zip(horizontal_columns.iter())
            .all(|(after, before)| after[0] > before[0])
    );
}

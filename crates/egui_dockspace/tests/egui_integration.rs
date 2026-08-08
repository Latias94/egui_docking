use std::collections::BTreeMap;

use dockspace::backend::interaction::{InteractionStatus, PreviewVisual};
use dockspace::backend::scene::{SplitterGapPresentation, SurfaceScene};
use dockspace::backend::transition::WorkspaceVersion;
use dockspace::drop_target::DropTargetId;
use dockspace::geometry::LogicalRect;
use dockspace::graph::{Axis, ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use dockspace::policy::{
    CloseCapability, DockItemRule, DockPolicy, DockTargetRule, DockTargetRuleKey,
    TabBarInteraction, TabBarPolicy, TabBarVisibility,
};
use dockspace::tab_strip::TabStripControlId;
use dockspace::{CloseDecision, ClosePlanTarget};
use egui::accesskit::{
    Action, ActionRequest, NodeId as AccessKitNodeId, Orientation, Role, TreeUpdate,
};
use egui::{
    Context, Event, Frame, Id, Key, Modifiers, MouseWheelUnit, PointerButton, Pos2, RawInput, Rect,
    Sense, TouchPhase, Ui, UiBuilder, vec2,
};
use egui_dockspace::backend::{EguiFrameScheduleKey, EguiRendererOutputDisposition};
use egui_dockspace::{
    Dockspace, DockspaceCloseOutcome, DockspaceClosePlan, DockspaceCommandOutcome,
    DockspaceSurfaceStatus, PaneView,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(10);
const FLOATING_ROOT: RootId = RootId::new(11);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(20);
const ITEM_A: ItemId = ItemId::new(100);
const ITEM_B: ItemId = ItemId::new(101);
const ITEM_C: ItemId = ItemId::new(102);
const ITEM_D: ItemId = ItemId::new(103);
const ITEM_E: ItemId = ItemId::new(104);
const ITEM_F: ItemId = ItemId::new(105);
const ITEM_G: ItemId = ItemId::new(106);

#[derive(Default)]
struct TestPanes {
    titles: BTreeMap<ItemId, String>,
    monospace_titles: bool,
    minimum_sizes: BTreeMap<ItemId, egui::Vec2>,
    ui_calls: BTreeMap<ItemId, usize>,
    disabled_ui_calls: BTreeMap<ItemId, usize>,
    pane_clicks: BTreeMap<ItemId, usize>,
    last_ui_rects: BTreeMap<ItemId, Rect>,
}

impl TestPanes {
    fn with_items(items: impl IntoIterator<Item = ItemId>) -> Self {
        Self {
            titles: items
                .into_iter()
                .map(|item| (item, format!("Pane {}", item.get())))
                .collect(),
            ..Self::default()
        }
    }

    fn ui_calls(&self, item: ItemId) -> usize {
        self.ui_calls.get(&item).copied().unwrap_or_default()
    }

    fn disabled_ui_calls(&self, item: ItemId) -> usize {
        self.disabled_ui_calls
            .get(&item)
            .copied()
            .unwrap_or_default()
    }

    fn pane_clicks(&self, item: ItemId) -> usize {
        self.pane_clicks.get(&item).copied().unwrap_or_default()
    }

    fn last_ui_rect(&self, item: ItemId) -> Option<Rect> {
        self.last_ui_rects.get(&item).copied()
    }
}

impl PaneView for TestPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        let title = self.titles.get(&item)?.clone();
        Some(if self.monospace_titles {
            egui::RichText::new(title).monospace().into()
        } else {
            title.into()
        })
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) {
        *self.ui_calls.entry(item).or_default() += 1;
        self.last_ui_rects.insert(item, ui.max_rect());
        if !ui.is_enabled() {
            *self.disabled_ui_calls.entry(item).or_default() += 1;
        }
        if ui.allocate_rect(ui.max_rect(), Sense::click()).clicked() {
            *self.pane_clicks.entry(item).or_default() += 1;
        }
    }

    fn minimum_size(&self, item: ItemId) -> egui::Vec2 {
        self.minimum_sizes.get(&item).copied().unwrap_or_default()
    }
}

#[derive(Debug)]
struct Observation {
    pass: usize,
    local_actions_current: bool,
    retained_presentation_current: bool,
    pointer_receivers_current: bool,
    surface_status: DockspaceSurfaceStatus,
    missing: Vec<ItemId>,
    close_requests: Vec<DockspaceClosePlan>,
    version: WorkspaceVersion,
    workspace: Workspace,
}

fn single_workspace(items: impl IntoIterator<Item = ItemId>) -> Workspace {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs(items));
    builder.set_root(MAIN_ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(MAIN_ROOT));
    builder.build().expect("single-surface fixture must build")
}

fn contained_workspace(rect: LogicalRect) -> Workspace {
    let mut builder = Workspace::builder();
    let main = builder.insert_node(Node::tabs([ITEM_A]));
    let floating = builder.insert_node(Node::tabs([ITEM_B]));
    builder.set_root(MAIN_ROOT, RootRecord::new(main));
    builder.set_root(FLOATING_ROOT, RootRecord::new(floating));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(MAIN_ROOT));
    builder.set_contained_floating(FLOATING, ContainedFloating::new(FLOATING_ROOT, rect));
    builder
        .attach_contained(SURFACE, FLOATING)
        .expect("surface exists");
    builder.build().expect("contained fixture must build")
}

fn overlapping_overflow_workspace(rect: LogicalRect) -> Workspace {
    let mut builder = Workspace::builder();
    let top = builder.insert_node(Node::tabs([ITEM_G]));
    let main_overflow = builder.insert_node(Node::tabs([ITEM_A, ITEM_B, ITEM_C]));
    let main = builder.insert_node(
        Node::split(Axis::Vertical, [top, main_overflow], [0.01, 0.99])
            .expect("weighted vertical fixture split is valid"),
    );
    let floating = builder.insert_node(Node::tabs([ITEM_D, ITEM_E, ITEM_F]));
    builder.set_root(MAIN_ROOT, RootRecord::new(main));
    builder.set_root(FLOATING_ROOT, RootRecord::new(floating));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(MAIN_ROOT));
    builder.set_contained_floating(FLOATING, ContainedFloating::new(FLOATING_ROOT, rect));
    builder
        .attach_contained(SURFACE, FLOATING)
        .expect("surface exists");
    builder.build().expect("overlap fixture must build")
}

fn popup_overlapping_tab_strip_workspace(
    popup_items: &[ItemId],
    underlay_items: &[ItemId],
    floating_underlay: bool,
) -> Workspace {
    let mut builder = Workspace::builder();
    let popup = builder.insert_node(Node::tabs(popup_items.iter().copied()));
    if floating_underlay {
        let underlay = builder.insert_node(Node::tabs(underlay_items.iter().copied()));
        builder.set_root(MAIN_ROOT, RootRecord::new(popup));
        builder.set_root(FLOATING_ROOT, RootRecord::new(underlay));
        builder.set_surface(SURFACE, SurfacePresentation::with_main(MAIN_ROOT));
        let rect =
            LogicalRect::new(20.0, 50.0, 200.0, 150.0).expect("floating underlay rect is valid");
        builder.set_contained_floating(FLOATING, ContainedFloating::new(FLOATING_ROOT, rect));
        builder
            .attach_contained(SURFACE, FLOATING)
            .expect("surface exists");
    } else {
        let underlay = builder.insert_node(Node::tabs(underlay_items.iter().copied()));
        let split = builder.insert_node(
            Node::split(Axis::Vertical, [popup, underlay], [0.45, 0.55])
                .expect("vertical popup overlap fixture is valid"),
        );
        builder.set_root(MAIN_ROOT, RootRecord::new(split));
        builder.set_surface(SURFACE, SurfacePresentation::with_main(MAIN_ROOT));
    }
    builder.build().expect("popup overlap fixture must build")
}

fn split_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let left = builder.insert_node(Node::tabs([ITEM_A]));
    let right = builder.insert_node(Node::tabs([ITEM_B]));
    let split = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [left, right]).expect("two children form a split"),
    );
    builder.set_root(MAIN_ROOT, RootRecord::new(split));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(MAIN_ROOT));
    builder.build().expect("split fixture must build")
}

fn input(events: Vec<Event>) -> RawInput {
    input_with_size(events, vec2(600.0, 400.0))
}

fn input_with_size(events: Vec<Event>, size: egui::Vec2) -> RawInput {
    input_with_rect(events, Rect::from_min_size(Pos2::ZERO, size))
}

fn input_with_rect(events: Vec<Event>, screen_rect: Rect) -> RawInput {
    RawInput {
        screen_rect: Some(screen_rect),
        events: events.into_iter().map(Into::into).collect(),
        ..RawInput::default()
    }
}

fn run_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    events: Vec<Event>,
) -> Vec<Observation> {
    run_frame_with_size(context, dockspace, panes, vec2(600.0, 400.0), events)
}

fn run_frame_with_size(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    size: egui::Vec2,
    events: Vec<Event>,
) -> Vec<Observation> {
    let mut observations = Vec::new();
    let _ = crate::test_support::run_ui(context, input_with_size(events, size), |ui| {
        let response = dockspace
            .show_single_surface(SURFACE, ui, panes)
            .expect("egui frame must advance");
        let capabilities = response.interaction_capabilities();
        observations.push(Observation {
            pass: ui.ctx().current_pass_index(),
            local_actions_current: capabilities.local_actions_current(),
            retained_presentation_current: capabilities.retained_presentation_current(),
            pointer_receivers_current: capabilities.pointer_receivers_current(),
            surface_status: response.surface_status(),
            missing: response.missing_panes().to_vec(),
            close_requests: response.close_requests().cloned().collect(),
            version: dockspace.core_engine().version(),
            workspace: dockspace.core_engine().workspace().clone(),
        });
    });
    observations
}

fn run_outer_frame_with_size(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    size: egui::Vec2,
    events: Vec<Event>,
) -> Observation {
    let sequence = dockspace.last_egui_frame_schedule_key().map_or(1, |key| {
        key.sequence()
            .checked_add(1)
            .expect("test host sequence must remain representable")
    });
    let mut frame = dockspace
        .begin_outer_frame(EguiFrameScheduleKey::new(sequence, 0))
        .expect("outer frame must begin");
    let paint = frame
        .run_surface(SURFACE, context, input_with_size(events, size), panes)
        .expect("outer host must paint the surface");
    let capabilities = paint.interaction_capabilities();
    let (host, outputs) = frame
        .finish()
        .expect("outer frame must commit atomically")
        .into_parts();
    outputs.submit_with(|_, _, _| EguiRendererOutputDisposition::Accepted);
    Observation {
        pass: context.current_pass_index(),
        local_actions_current: capabilities.local_actions_current(),
        retained_presentation_current: capabilities.retained_presentation_current(),
        pointer_receivers_current: capabilities.pointer_receivers_current(),
        surface_status: paint.surface_status(),
        missing: paint.missing_panes().to_vec(),
        close_requests: host.close_requests().cloned().collect(),
        version: dockspace.core_engine().version(),
        workspace: dockspace.core_engine().workspace().clone(),
    }
}

fn warm_outer_with_size(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    size: egui::Vec2,
) {
    for _ in 0..8 {
        let observation = run_outer_frame_with_size(context, dockspace, panes, size, Vec::new());
        if observation.pointer_receivers_current && paint_projection_is_authoritative(dockspace) {
            return;
        }
    }
    panic!("a later outer-host frame must acknowledge the rendered projection");
}

fn run_authoritative_frame_with_size(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    size: egui::Vec2,
) -> Vec<Observation> {
    for _ in 0..8 {
        let observations = run_frame_with_size(context, dockspace, panes, size, Vec::new());
        if observations
            .last()
            .is_some_and(|observation| observation.pointer_receivers_current)
            && paint_projection_is_authoritative(dockspace)
        {
            return observations;
        }
    }
    panic!("a later host sequence must acknowledge the rendered projection");
}

fn only_close_request(observations: &[Observation]) -> DockspaceClosePlan {
    let mut requests = observations
        .iter()
        .flat_map(|observation| observation.close_requests.iter());
    let request = requests
        .next()
        .expect("the semantic close activation must publish one close plan")
        .clone();
    assert!(
        requests.next().is_none(),
        "one activation must publish exactly one close plan"
    );
    request
}

fn resolve_close_plan(
    dockspace: &mut Dockspace,
    plan: &DockspaceClosePlan,
    decision_for: impl Fn(ItemId) -> CloseDecision,
) {
    for item in plan.items() {
        let result = dockspace
            .resolve_close(plan.request(), item.token(), decision_for(item.item()))
            .expect("an exact close decision must commit immediately");
        assert!(matches!(
            result.outcome(),
            DockspaceCloseOutcome::Processed { .. }
        ));
    }
}

fn run_accesskit_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    size: egui::Vec2,
    events: Vec<Event>,
) -> TreeUpdate {
    run_accesskit_frame_with_authority(context, dockspace, panes, size, events).0
}

fn run_accesskit_frame_with_authority(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    size: egui::Vec2,
    events: Vec<Event>,
) -> (TreeUpdate, bool) {
    let mut retained_presentation_current = false;
    let output = crate::test_support::run_ui(context, input_with_size(events, size), |ui| {
        let response = dockspace
            .show_single_surface(SURFACE, ui, panes)
            .expect("AccessKit frame must advance");
        retained_presentation_current = response
            .interaction_capabilities()
            .retained_presentation_current();
    });
    let update = output
        .platform_output
        .accesskit_update
        .expect("AccessKit output is enabled");
    (update, retained_presentation_current)
}

fn run_authoritative_accesskit_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    size: egui::Vec2,
) -> TreeUpdate {
    for _ in 0..4 {
        let (update, retained_presentation_current) =
            run_accesskit_frame_with_authority(context, dockspace, panes, size, Vec::new());
        if retained_presentation_current {
            return update;
        }
    }
    panic!("a later host sequence must acknowledge the rendered projection");
}

fn run_accesskit_frame_in_rect(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    screen_rect: Rect,
    dock_rect: Rect,
    events: Vec<Event>,
) -> TreeUpdate {
    run_accesskit_frame_in_rect_with_authority(
        context,
        dockspace,
        panes,
        screen_rect,
        dock_rect,
        events,
    )
    .0
}

fn run_accesskit_frame_in_rect_with_authority(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    screen_rect: Rect,
    dock_rect: Rect,
    events: Vec<Event>,
) -> (TreeUpdate, bool) {
    let mut retained_presentation_current = false;
    let output = crate::test_support::run_ui(context, input_with_rect(events, screen_rect), |ui| {
        let mut child = ui.new_child(UiBuilder::new().max_rect(dock_rect));
        let response = dockspace
            .show_single_surface(SURFACE, &mut child, panes)
            .expect("positioned AccessKit frame must advance");
        retained_presentation_current = response
            .interaction_capabilities()
            .retained_presentation_current();
    });
    let update = output
        .platform_output
        .accesskit_update
        .expect("AccessKit output is enabled");
    (update, retained_presentation_current)
}

fn run_authoritative_accesskit_frame_in_rect(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    screen_rect: Rect,
    dock_rect: Rect,
) -> TreeUpdate {
    for _ in 0..4 {
        let (update, retained_presentation_current) = run_accesskit_frame_in_rect_with_authority(
            context,
            dockspace,
            panes,
            screen_rect,
            dock_rect,
            Vec::new(),
        );
        if retained_presentation_current {
            return update;
        }
    }
    panic!("a later positioned host sequence must acknowledge the rendered projection");
}

fn run_accesskit_frame_with_competing_popup(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    size: egui::Vec2,
    competing_id: Id,
) -> TreeUpdate {
    let output = crate::test_support::run_ui(context, input_with_size(Vec::new(), size), |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, panes)
            .expect("AccessKit frame must advance");
        egui::Popup::open_id(ui.ctx(), competing_id);
    });
    output
        .platform_output
        .accesskit_update
        .expect("AccessKit output is enabled")
}

fn run_accesskit_frame_in_ancestor_scroll_area(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    salt: &str,
    size: egui::Vec2,
    forced_outer_offset: Option<f32>,
    events: Vec<Event>,
) -> (TreeUpdate, f32, bool) {
    let mut outer_offset = None;
    let mut wheel_available_after_dockspace = false;
    let output = crate::test_support::run_ui(context, input_with_size(events, size), |ui| {
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt(Id::new(("ancestor-scroll", salt)))
            .auto_shrink([false, false]);
        if let Some(offset) = forced_outer_offset {
            scroll = scroll.vertical_scroll_offset(offset);
        }
        let result = scroll.show(ui, |outer_ui| {
            outer_ui.add_space(100.0);
            outer_ui.allocate_ui_with_layout(
                vec2(outer_ui.available_width(), size.y),
                egui::Layout::top_down(egui::Align::Min),
                |dock_ui| {
                    dockspace
                        .show_single_surface(SURFACE, dock_ui, panes)
                        .expect("nested AccessKit frame must advance");
                    wheel_available_after_dockspace |= dock_ui.input(|input| {
                        input
                            .events
                            .iter()
                            .any(|event| matches!(event, Event::MouseWheel { .. }))
                    });
                },
            );
            outer_ui.add_space(800.0);
        });
        outer_offset = Some(result.state.offset.y);
    });
    (
        output
            .platform_output
            .accesskit_update
            .expect("AccessKit output is enabled"),
        outer_offset.expect("ancestor scroll area paints in every pass"),
        wheel_available_after_dockspace,
    )
}

fn accesskit_node_by_label<'a>(
    update: &'a TreeUpdate,
    role: Role,
    label: &str,
) -> (AccessKitNodeId, &'a egui::accesskit::Node) {
    let mut matches = update
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == role && node.label() == Some(label));
    let (id, node) = matches.next().expect("labeled AccessKit node exists");
    assert!(
        matches.next().is_none(),
        "fixture AccessKit label and role must be unique"
    );
    (*id, node)
}

fn accesskit_node_by_role(
    update: &TreeUpdate,
    role: Role,
) -> (AccessKitNodeId, &egui::accesskit::Node) {
    let mut matches = update.nodes.iter().filter(|(_, node)| node.role() == role);
    let (id, node) = matches.next().expect("AccessKit node with role exists");
    assert!(
        matches.next().is_none(),
        "fixture AccessKit role must be unique"
    );
    (*id, node)
}

fn accesskit_node_by_id(update: &TreeUpdate, id: AccessKitNodeId) -> &egui::accesskit::Node {
    update
        .nodes
        .iter()
        .find_map(|(candidate, node)| (*candidate == id).then_some(node))
        .expect("AccessKit node id exists in the complete fixture update")
}

fn accesskit_action(target_node: AccessKitNodeId, action: Action) -> Event {
    Event::AccessKitActionRequest(ActionRequest {
        action,
        target_tree: egui::accesskit::TreeId::ROOT,
        target_node,
        data: None,
    })
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "finite AccessKit fixture geometry is converted to egui input coordinates"
)]
fn accesskit_node_center(node: &egui::accesskit::Node) -> Pos2 {
    let bounds = node
        .bounds()
        .expect("interactive AccessKit node has bounds");
    Pos2::new(
        ((bounds.x0 + bounds.x1) * 0.5) as f32,
        ((bounds.y0 + bounds.y1) * 0.5) as f32,
    )
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "finite AccessKit fixture geometry is converted to egui input coordinates"
)]
fn accesskit_node_rect(node: &egui::accesskit::Node) -> Rect {
    let bounds = node
        .bounds()
        .expect("interactive AccessKit node has bounds");
    Rect::from_min_max(
        Pos2::new(bounds.x0 as f32, bounds.y0 as f32),
        Pos2::new(bounds.x1 as f32, bounds.y1 as f32),
    )
}

fn numbered_items(start: u64, count: usize) -> Vec<ItemId> {
    (0..count)
        .map(|index| {
            ItemId::new(start + u64::try_from(index).expect("fixture item index fits in u64"))
        })
        .collect()
}

fn warm(context: &Context, dockspace: &mut Dockspace, panes: &mut TestPanes) -> Observation {
    let first = run_frame(context, dockspace, panes, Vec::new());
    assert!(
        first
            .iter()
            .all(|observation| !observation.retained_presentation_current),
        "no pass may acknowledge output from its own host sequence"
    );
    let second = run_frame(context, dockspace, panes, Vec::new());
    let stable = second.into_iter().last().expect("one authoritative pass");
    assert!(stable.retained_presentation_current);
    stable
}

#[test]
fn equal_width_title_changes_refresh_paint_resources_without_changing_scene_identity() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("equal-width-title-refresh", single_workspace([ITEM_A]))
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A]);
    panes.monospace_titles = true;
    panes.titles.insert(ITEM_A, "AAAA".to_owned());
    let _ = warm(&context, &mut dockspace, &mut panes);
    let before = dockspace
        .core_engine()
        .scene()
        .surface(SURFACE)
        .and_then(SurfaceScene::ready)
        .expect("surface is ready")
        .candidate()
        .stamp();

    panes.titles.insert(ITEM_A, "BBBB".to_owned());
    let refreshed = run_accesskit_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec2(600.0, 400.0),
        Vec::new(),
    );
    let after = dockspace
        .core_engine()
        .scene()
        .surface(SURFACE)
        .and_then(SurfaceScene::ready)
        .expect("surface remains ready")
        .candidate()
        .stamp();

    assert_eq!(
        after, before,
        "paint-only changes must not mint a scene stamp"
    );
    accesskit_node_by_label(&refreshed, Role::Tab, "BBBB");
}

fn contained_close_id(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    salt: &'static str,
) -> Id {
    let mut close_id = None;
    let _ = crate::test_support::run_ui(context, input(Vec::new()), |ui| {
        let instance_id = Id::new(("egui_dockspace", salt));
        close_id = Some(ui.make_persistent_id((
            "egui_dockspace",
            instance_id,
            SURFACE,
            FLOATING_ROOT,
            FLOATING,
            "contained-close",
        )));
        dockspace
            .show_single_surface(SURFACE, ui, panes)
            .expect("contained close frame must advance");
    });
    close_id.expect("contained close widget must be registered")
}

fn pointer_button(position: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos: position,
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

fn translated_rect(rect: LogicalRect, origin: Pos2, current: Pos2) -> LogicalRect {
    LogicalRect::new(
        rect.x() + f64::from(current.x) - f64::from(origin.x),
        rect.y() + f64::from(current.y) - f64::from(origin.y),
        rect.width(),
        rect.height(),
    )
    .expect("translated fixture rect is finite")
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "fixture geometry is finite and deliberately converted into egui's f32 input space"
)]
fn contained_title_point(dockspace: &Dockspace) -> Pos2 {
    let floating = dockspace
        .core_engine()
        .workspace()
        .contained_floating(FLOATING)
        .expect("fixture has one contained floating");
    let style = dockspace.style();
    let title_inset = style
        .floating_resize_extent
        .min(style.floating_title_height * 0.5);
    Pos2::new(
        floating.rect.min().x() as f32
            + style.floating_border_width
            + title_inset
            + style.tab_horizontal_padding,
        floating.rect.min().y() as f32
            + style.floating_border_width
            + style.floating_title_height * 0.5,
    )
}

fn contained_rect(dockspace: &Dockspace) -> LogicalRect {
    dockspace
        .core_engine()
        .workspace()
        .contained_floating(FLOATING)
        .expect("fixture has one contained floating")
        .rect
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "fixture geometry is finite and deliberately converted into egui's f32 input space"
)]
fn contained_north_west_resize_point(dockspace: &Dockspace) -> Pos2 {
    let floating = dockspace
        .core_engine()
        .workspace()
        .contained_floating(FLOATING)
        .expect("fixture has one contained floating");
    let half_extent = dockspace.style().floating_resize_extent * 0.5;
    Pos2::new(
        floating.rect.min().x() as f32 + half_extent,
        floating.rect.min().y() as f32 + half_extent,
    )
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "the published finite splitter rectangle is converted back to egui's f32 input space"
)]
fn splitter_center(dockspace: &Dockspace) -> Pos2 {
    splitter_hit_rect(dockspace).center()
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "the published finite splitter rectangle is converted back to egui's f32 input space"
)]
fn splitter_hit_rect(dockspace: &Dockspace) -> Rect {
    let painted = dockspace
        .core_engine()
        .interaction_projection(SURFACE)
        .expect("surface must have an acknowledged paint");
    let splitter = painted
        .plan()
        .splitter_records()
        .first()
        .expect("split fixture has one splitter")
        .hit()
        .rect();
    Rect::from_min_max(
        Pos2::new(splitter.min().x() as f32, splitter.min().y() as f32),
        Pos2::new(splitter.max().x() as f32, splitter.max().y() as f32),
    )
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "the published finite scene rectangle is converted back to egui's f32 input space"
)]
fn tab_close_rect(dockspace: &Dockspace) -> Rect {
    let painted = dockspace
        .core_engine()
        .interaction_projection(SURFACE)
        .expect("surface must have an acknowledged paint");
    let tab = painted
        .plan()
        .tab_records()
        .first()
        .and_then(|tab| tab.close_bounds())
        .expect("fixture has one closeable tab");
    Rect::from_min_max(
        Pos2::new(tab.min().x() as f32, tab.min().y() as f32),
        Pos2::new(tab.max().x() as f32, tab.max().y() as f32),
    )
}

fn published_tab_rect(dockspace: &Dockspace, item: ItemId) -> Option<LogicalRect> {
    dockspace
        .core_engine()
        .interaction_projection(SURFACE)?
        .plan()
        .tab_records()
        .iter()
        .find(|tab| tab.id().item == item)
        .map(dockspace::backend::scene::TabRecord::visible_bounds)
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "finite published fixture geometry is converted to egui input coordinates"
)]
fn logical_rect_center(rect: LogicalRect) -> Pos2 {
    Pos2::new(
        ((rect.min().x() + rect.max().x()) * 0.5) as f32,
        ((rect.min().y() + rect.max().y()) * 0.5) as f32,
    )
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "finite published fixture geometry is converted to egui input coordinates"
)]
fn egui_rect(rect: LogicalRect) -> Rect {
    Rect::from_min_max(
        Pos2::new(rect.min().x() as f32, rect.min().y() as f32),
        Pos2::new(rect.max().x() as f32, rect.max().y() as f32),
    )
}

fn logical_rect_intersection(first: LogicalRect, second: LogicalRect) -> Option<LogicalRect> {
    let min_x = first.min().x().max(second.min().x());
    let min_y = first.min().y().max(second.min().y());
    let max_x = first.max().x().min(second.max().x());
    let max_y = first.max().y().min(second.max().y());
    (max_x > min_x && max_y > min_y)
        .then(|| LogicalRect::new(min_x, min_y, max_x - min_x, max_y - min_y))
        .transpose()
        .expect("finite scene intersections remain valid")
}

fn selected_item(dockspace: &Dockspace, root: RootId) -> Option<ItemId> {
    let root = dockspace.core_engine().workspace().root(root)?;
    match dockspace.core_engine().workspace().node(root.node)? {
        Node::Tabs { selected, .. } => *selected,
        Node::Split { .. } => None,
    }
}

fn select_item(dockspace: &mut Dockspace, item: ItemId) {
    let source = dockspace
        .core_engine()
        .workspace()
        .roots()
        .find_map(|(root, record)| {
            dockspace
                .core_engine()
                .workspace()
                .capture_item_source(root, record.node, item)
                .ok()
        })
        .expect("selected pane source must capture");
    let result = dockspace
        .submit_command(dockspace::command::WorkspaceCommand::Select { source })
        .expect("selection command must commit immediately");
    assert!(matches!(
        result.outcome(),
        DockspaceCommandOutcome::Applied(_)
    ));
}

fn selected_in_group_containing(dockspace: &Dockspace, item: ItemId) -> Option<ItemId> {
    dockspace
        .core_engine()
        .workspace()
        .nodes()
        .find_map(|(_, node)| match node {
            Node::Tabs { items, selected } if items.contains(&item) => *selected,
            Node::Tabs { .. } | Node::Split { .. } => None,
        })
}

fn paint_projection_is_authoritative(dockspace: &Dockspace) -> bool {
    let scene = dockspace.core_engine().scene().surface(SURFACE);
    scene
        .and_then(SurfaceScene::paint_projection)
        .zip(dockspace.core_engine().interaction_projection(SURFACE))
        .is_some_and(|(paint, interaction)| {
            paint.output_ticket() == interaction.output_ticket()
                && interaction
                    .authority()
                    .matches_output(paint.output_ticket())
        })
}

#[test]
fn inactive_tab_pointer_down_selects_and_arms_in_one_core_revision() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder(
        "inactive-tab-atomic-pointer-activation",
        single_workspace([ITEM_A, ITEM_B]),
    )
    .build()
    .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    warm_outer_with_size(&context, &mut dockspace, &mut panes, vec2(600.0, 400.0));

    let press = logical_rect_center(
        published_tab_rect(&dockspace, ITEM_B).expect("inactive tab is visible"),
    );
    let before = dockspace.core_engine().version();

    run_outer_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        vec2(600.0, 400.0),
        vec![Event::PointerMoved(press), pointer_button(press, true)],
    );

    assert_eq!(selected_item(&dockspace, MAIN_ROOT), Some(ITEM_B));
    assert!(matches!(
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Armed { .. }
    ));
    assert_eq!(dockspace.core_engine().version().epoch(), before.epoch());
    assert_eq!(
        dockspace.core_engine().version().revision().get(),
        before.revision().get() + 1,
        "selection and gesture activation must publish one atomic core revision"
    );
}

#[test]
fn same_frame_press_and_release_still_activate_an_inactive_tab() {
    let context = Context::default();
    let mut dockspace =
        Dockspace::builder("same-frame-tab-click", single_workspace([ITEM_A, ITEM_B]))
            .build()
            .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    warm_outer_with_size(&context, &mut dockspace, &mut panes, vec2(600.0, 400.0));

    let click = logical_rect_center(
        published_tab_rect(&dockspace, ITEM_B).expect("inactive tab is visible"),
    );
    run_outer_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        vec2(600.0, 400.0),
        vec![
            Event::PointerMoved(click),
            pointer_button(click, true),
            pointer_button(click, false),
        ],
    );

    assert_eq!(selected_item(&dockspace, MAIN_ROOT), Some(ITEM_B));
    assert_eq!(
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Idle,
        "a complete click must not leave a gesture owner behind"
    );
}

#[test]
fn same_frame_press_and_release_requests_one_exact_tab_close() {
    let context = Context::default();
    let mut dockspace =
        Dockspace::builder("same-frame-tab-close", single_workspace([ITEM_A, ITEM_B]))
            .build()
            .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    warm_outer_with_size(&context, &mut dockspace, &mut panes, vec2(600.0, 400.0));

    let close = dockspace
        .core_engine()
        .interaction_projection(SURFACE)
        .expect("the tab strip is authoritative")
        .plan()
        .tab_records()
        .iter()
        .find(|tab| tab.id().item == ITEM_B)
        .and_then(|tab| tab.close_bounds())
        .map(logical_rect_center)
        .expect("the inactive tab exposes an operable close affordance");
    let observation = run_outer_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        vec2(600.0, 400.0),
        vec![
            Event::PointerMoved(close),
            pointer_button(close, true),
            pointer_button(close, false),
        ],
    );

    assert_eq!(observation.close_requests.len(), 1);
    assert_eq!(observation.close_requests[0].items().len(), 1);
    assert_eq!(observation.close_requests[0].items()[0].item(), ITEM_B);
    assert_eq!(
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Idle,
        "a complete close click must not leave a gesture owner behind"
    );
}

#[test]
fn painted_prepared_contribution_requires_a_later_host_sequence_acknowledgement() {
    let context = Context::default();
    let workspace = single_workspace([ITEM_A]);
    let mut dockspace = Dockspace::builder("ready-scene", workspace.clone())
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A]);

    let observations = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let painted = observations.last().expect("frame paints");

    assert_eq!(painted.workspace, workspace);
    assert_eq!(painted.version, WorkspaceVersion::default());
    assert_eq!(
        observations
            .iter()
            .map(|observation| observation.retained_presentation_current)
            .collect::<Vec<_>>(),
        vec![false, false]
    );
    assert_eq!(
        observations
            .iter()
            .map(|observation| observation.surface_status)
            .collect::<Vec<_>>(),
        vec![
            DockspaceSurfaceStatus::Bootstrap,
            DockspaceSurfaceStatus::Ready
        ]
    );
    assert_eq!(painted.pass, 1);
    assert!(matches!(
        dockspace.core_engine().scene().surface(SURFACE),
        Some(SurfaceScene::Ready(_))
    ));
    assert!(!paint_projection_is_authoritative(&dockspace));

    let current = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        current
            .last()
            .expect("next host sequence acknowledges the final painted pass")
            .retained_presentation_current
    );
    assert!(paint_projection_is_authoritative(&dockspace));
    assert_eq!(panes.ui_calls(ITEM_A), observations.len() + current.len());
}

#[test]
fn external_discards_before_or_after_dockspace_never_acknowledge_an_earlier_pass() {
    for (salt, discard_before_dockspace) in [
        ("external-discard-before", true),
        ("external-discard-after", false),
    ] {
        let context = Context::default();
        context.options_mut(|options| {
            options.max_passes = 4.try_into().expect("four is non-zero");
        });
        let mut dockspace = Dockspace::builder(salt, single_workspace([ITEM_A]))
            .build()
            .expect("facade must build");
        let mut panes = TestPanes::with_items([ITEM_A]);
        let mut sequence_passes = Vec::new();

        let _ = crate::test_support::run_ui(&context, input(Vec::new()), |ui| {
            let pass = ui.ctx().current_pass_index();
            if pass == 1 && discard_before_dockspace {
                ui.ctx()
                    .request_discard("external widget changed before dockspace");
            }
            let response = dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("multipass dockspace frame must advance");
            sequence_passes.push((
                pass,
                response
                    .interaction_capabilities()
                    .retained_presentation_current(),
            ));
            if pass == 1 && !discard_before_dockspace {
                ui.ctx()
                    .request_discard("external widget changed after dockspace");
            }
        });

        assert_eq!(
            sequence_passes,
            [(0, false), (1, false), (2, false)],
            "no pass may acknowledge presentation from its own host sequence"
        );
        let candidate = dockspace
            .core_engine()
            .scene()
            .surface(SURFACE)
            .and_then(SurfaceScene::ready)
            .expect("the final pass paints a ready candidate")
            .candidate()
            .stamp();
        assert!(!paint_projection_is_authoritative(&dockspace));

        let accepted = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        assert!(
            accepted
                .last()
                .expect("the next host sequence observes the acknowledgement")
                .retained_presentation_current
        );
        assert_eq!(
            dockspace
                .core_engine()
                .interaction_projection(SURFACE)
                .map(dockspace::backend::scene::SurfaceInteractionProjection::plan_stamp),
            Some(candidate)
        );
    }
}

#[test]
fn omitted_final_pass_cannot_acknowledge_an_earlier_dockspace_pass() {
    let context = Context::default();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let mut dockspace = Dockspace::builder("omitted-final-pass", single_workspace([ITEM_A]))
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A]);

    let mut bootstrap_passes = Vec::new();
    let _ = crate::test_support::run_ui(&context, input(Vec::new()), |ui| {
        let pass = ui.ctx().current_pass_index();
        bootstrap_passes.push(pass);
        if pass == 0 {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("bootstrap pass commits its candidate");
        }
    });
    assert_eq!(bootstrap_passes, [0, 1]);
    assert!(!paint_projection_is_authoritative(&dockspace));

    let mut discarded_passes = Vec::new();
    let _ = crate::test_support::run_ui(&context, input(Vec::new()), |ui| {
        let pass = ui.ctx().current_pass_index();
        discarded_passes.push(pass);
        if pass == 0 {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("candidate paints in the pass that will be discarded");
            ui.ctx()
                .request_discard("the final pass deliberately omits dockspace");
        }
    });
    assert_eq!(discarded_passes, [0, 1]);
    assert!(!paint_projection_is_authoritative(&dockspace));

    let unconfirmed = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        unconfirmed
            .last()
            .is_some_and(|observation| !observation.retained_presentation_current)
    );
    assert!(
        !paint_projection_is_authoritative(&dockspace),
        "a pass omitted from the final egui output cannot become presentation authority"
    );

    run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        paint_projection_is_authoritative(&dockspace),
        "the later pass that really was final remains acknowledgeable"
    );
}

#[test]
fn authority_incomplete_wheel_does_not_mutate_overflowing_tabs() {
    let context = Context::default();
    let workspace = single_workspace([ITEM_A, ITEM_B, ITEM_C]);
    let mut dockspace = Dockspace::builder("tab-overflow", workspace)
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B, ITEM_C]);
    let size = vec2(180.0, 200.0);

    run_authoritative_frame_with_size(&context, &mut dockspace, &mut panes, size);
    let selected_before = published_tab_rect(&dockspace, ITEM_A);
    let neighbor_before = published_tab_rect(&dockspace, ITEM_B);
    let scroll_before = dockspace
        .core_engine()
        .interaction_projection(SURFACE)
        .and_then(|projection| projection.plan().tab_bar_records().first())
        .map(dockspace::backend::scene::TabBarRecord::scroll_offset)
        .expect("the overflow fixture publishes one tab bar");
    let version_before = dockspace.core_engine().version();
    assert!(selected_before.is_some());
    assert!(published_tab_rect(&dockspace, ITEM_C).is_none());

    run_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![
            Event::PointerMoved(Pos2::new(90.0, 14.0)),
            Event::MouseWheel {
                unit: MouseWheelUnit::Point,
                delta: vec2(0.0, -72.0),
                phase: TouchPhase::Move,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    run_authoritative_frame_with_size(&context, &mut dockspace, &mut panes, size);
    assert_eq!(selected_item(&dockspace, MAIN_ROOT), Some(ITEM_A));
    assert_eq!(dockspace.core_engine().version(), version_before);
    assert_eq!(
        dockspace
            .core_engine()
            .interaction_projection(SURFACE)
            .and_then(|projection| projection.plan().tab_bar_records().first())
            .map(dockspace::backend::scene::TabBarRecord::scroll_offset),
        Some(scroll_before),
        "official egui wheel facts cannot authorize docking scroll"
    );
    let selected_after = published_tab_rect(&dockspace, ITEM_A)
        .expect("the selected tab must retain enough visible chrome to remain operable");
    let neighbor_after = published_tab_rect(&dockspace, ITEM_B);
    assert_eq!(
        (Some(selected_after), neighbor_after),
        (selected_before, neighbor_before),
        "final-frame hover must not be promoted into wheel receiver authority"
    );

    let painted = dockspace
        .core_engine()
        .interaction_projection(SURFACE)
        .expect("overflow fixture publishes an acknowledged scene");
    assert!(
        selected_after.width()
            >= dockspace
                .core_engine()
                .presentation_config()
                .tab_close_extent(),
        "the selected closeable tab must retain its close and drag affordances"
    );
    assert!(painted.plan().tab_records().iter().all(|tab| {
        tab.visible_bounds().min().x() >= 28.0 && tab.visible_bounds().max().x() <= 152.0
    }));
    assert!(painted.plan().drop_targets().iter().all(|target| {
        !matches!(target.id(), DropTargetId::TabGap { .. })
            || target.region().rect().min().x() >= 28.0 && target.region().rect().max().x() <= 152.0
    }));

    let tabs = dockspace
        .core_engine()
        .workspace()
        .root(MAIN_ROOT)
        .expect("fixture root exists")
        .node;
    let source = dockspace
        .core_engine()
        .workspace()
        .capture_item_source(MAIN_ROOT, tabs, ITEM_C)
        .expect("hidden item source captures");
    dockspace
        .submit_command(dockspace::command::WorkspaceCommand::Select { source })
        .expect("selection command commits immediately");
    assert_eq!(selected_item(&dockspace, MAIN_ROOT), Some(ITEM_C));
    let painted_selection =
        run_frame_with_size(&context, &mut dockspace, &mut panes, size, Vec::new());
    assert!(
        !painted_selection
            .last()
            .expect("the selected candidate paints")
            .retained_presentation_current
    );
    let acknowledged_selection =
        run_frame_with_size(&context, &mut dockspace, &mut panes, size, Vec::new());
    assert!(
        acknowledged_selection
            .last()
            .expect("the next host sequence acknowledges the selected candidate")
            .retained_presentation_current
    );
    assert_eq!(
        published_tab_rect(&dockspace, ITEM_C).map(LogicalRect::width),
        Some(72.0)
    );
}

#[test]
fn authority_incomplete_wheel_does_not_disturb_keyboard_focus() {
    let context = Context::default();
    context.enable_accesskit();
    let size = vec2(220.0, 200.0);
    let mut dockspace = Dockspace::builder(
        "focused-tab-wheel-guard",
        single_workspace([ITEM_A, ITEM_B, ITEM_C, ITEM_D, ITEM_E]),
    )
    .build()
    .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B, ITEM_C, ITEM_D, ITEM_E]);

    let _ = run_authoritative_accesskit_frame(&context, &mut dockspace, &mut panes, size);
    let stable = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
    let (first, _) = accesskit_node_by_label(&stable, Role::Tab, &format!("Pane {}", ITEM_A.get()));
    run_accesskit_frame(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![accesskit_action(first, Action::Focus)],
    );
    let _ = run_authoritative_accesskit_frame(&context, &mut dockspace, &mut panes, size);
    run_accesskit_frame(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![Event::Key {
            key: Key::End,
            physical_key: Some(Key::End),
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }],
    );
    let focused = run_authoritative_accesskit_frame(&context, &mut dockspace, &mut panes, size);
    let last_label = format!("Pane {}", ITEM_E.get());
    let (last_id, _) = accesskit_node_by_label(&focused, Role::Tab, &last_label);
    assert_eq!(focused.focus, last_id);
    assert_eq!(selected_item(&dockspace, MAIN_ROOT), Some(ITEM_E));
    let last_before = published_tab_rect(&dockspace, ITEM_E).expect("focused last tab is revealed");
    let pointer = logical_rect_center(last_before);

    run_accesskit_frame(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![
            Event::PointerMoved(pointer),
            Event::MouseWheel {
                unit: MouseWheelUnit::Point,
                delta: vec2(0.0, 10_000.0),
                phase: TouchPhase::Move,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
    let guarded = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
    let (last_id, _) = accesskit_node_by_label(&guarded, Role::Tab, &last_label);

    assert_eq!(guarded.focus, last_id);
    assert!(published_tab_rect(&dockspace, ITEM_E).is_some());
}

#[test]
fn authority_incomplete_wheel_does_not_disturb_an_active_drag() {
    let context = Context::default();
    let size = vec2(220.0, 200.0);
    let mut dockspace = Dockspace::builder(
        "dragged-tab-wheel-guard",
        single_workspace([ITEM_A, ITEM_B, ITEM_C, ITEM_D, ITEM_E]),
    )
    .build()
    .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B, ITEM_C, ITEM_D, ITEM_E]);
    warm_outer_with_size(&context, &mut dockspace, &mut panes, size);

    let dragged_before = published_tab_rect(&dockspace, ITEM_B).expect("second tab is visible");
    let press = logical_rect_center(dragged_before);
    let current = press + vec2(20.0, 0.0);
    run_outer_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![Event::PointerMoved(press), pointer_button(press, true)],
    );
    run_outer_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![Event::PointerMoved(current)],
    );
    run_outer_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![Event::PointerMoved(current)],
    );
    assert!(matches!(
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));

    run_outer_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![
            Event::PointerMoved(current),
            Event::MouseWheel {
                unit: MouseWheelUnit::Point,
                delta: vec2(0.0, -10_000.0),
                phase: TouchPhase::Move,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    warm_outer_with_size(&context, &mut dockspace, &mut panes, size);

    assert!(matches!(
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    assert!(
        published_tab_rect(&dockspace, ITEM_B).is_some(),
        "active drag source remains visible"
    );
    assert!(
        dockspace
            .core_engine()
            .interaction_projection(SURFACE)
            .and_then(|projection| projection
                .plan()
                .tab_records()
                .iter()
                .find(|tab| tab.id().item == ITEM_B)
                .map(dockspace::backend::scene::TabRecord::selected))
            == Some(true),
        "the atomically selected drag source remains selected after scrolling",
    );
}

#[test]
fn authority_incomplete_wheel_does_not_choose_an_overlapping_tab_strip() {
    let context = Context::default();
    let size = vec2(220.0, 250.0);
    let floating_rect =
        LogicalRect::new(0.0, 60.0, 180.0, 160.0).expect("floating fixture rect is valid");
    let workspace = overlapping_overflow_workspace(floating_rect);
    let mut dockspace = Dockspace::builder("floating-wheel-owner", workspace)
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B, ITEM_C, ITEM_D, ITEM_E, ITEM_F, ITEM_G]);
    run_authoritative_frame_with_size(&context, &mut dockspace, &mut panes, size);
    select_item(&mut dockspace, ITEM_E);
    run_authoritative_frame_with_size(&context, &mut dockspace, &mut panes, size);

    let background_before = published_tab_rect(&dockspace, ITEM_A).expect("main tab is visible");
    let foreground_before =
        published_tab_rect(&dockspace, ITEM_D).expect("floating tab is visible");
    let overlap = logical_rect_intersection(background_before, foreground_before)
        .expect("fixture tab strips overlap");
    let pointer = logical_rect_center(overlap);
    let version_before = dockspace.core_engine().version();
    run_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![
            Event::PointerMoved(pointer),
            Event::MouseWheel {
                unit: MouseWheelUnit::Point,
                delta: vec2(0.0, -72.0),
                phase: TouchPhase::Move,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    run_authoritative_frame_with_size(&context, &mut dockspace, &mut panes, size);

    assert_eq!(dockspace.core_engine().version(), version_before);
    assert_eq!(
        published_tab_rect(&dockspace, ITEM_A),
        Some(background_before)
    );
    assert_eq!(
        published_tab_rect(&dockspace, ITEM_D),
        Some(foreground_before),
        "visual stacking plus final hover cannot prove wheel delivery"
    );
}

#[test]
fn frontmost_floating_tab_strip_exclusively_owns_overlapping_drag_edge_scroll() {
    let context = Context::default();
    let size = vec2(220.0, 250.0);
    let floating_rect =
        LogicalRect::new(0.0, 60.0, 180.0, 160.0).expect("floating fixture rect is valid");
    let workspace = overlapping_overflow_workspace(floating_rect);
    let mut dockspace = Dockspace::builder("floating-edge-owner", workspace)
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B, ITEM_C, ITEM_D, ITEM_E, ITEM_F, ITEM_G]);
    warm_outer_with_size(&context, &mut dockspace, &mut panes, size);
    select_item(&mut dockspace, ITEM_E);
    warm_outer_with_size(&context, &mut dockspace, &mut panes, size);

    let background_before = published_tab_rect(&dockspace, ITEM_A).expect("main tab is visible");
    let foreground_before =
        published_tab_rect(&dockspace, ITEM_D).expect("floating tab is visible");
    let press = logical_rect_center(
        published_tab_rect(&dockspace, ITEM_E).expect("middle floating tab is visible"),
    );
    let edge = logical_rect_center(
        dockspace
            .core_engine()
            .interaction_projection(SURFACE)
            .expect("the floating strip has authoritative controls")
            .plan()
            .tab_strip_control_records()
            .iter()
            .find_map(|control| match control.id() {
                TabStripControlId::ScrollForward(bar) if bar.root == FLOATING_ROOT => {
                    Some(control.hit().rect())
                }
                TabStripControlId::ScrollBackward(_) | TabStripControlId::TabListMenu(_) => None,
                TabStripControlId::ScrollForward(_) => None,
            })
            .expect("the overflowing floating strip exposes its forward control"),
    );

    run_outer_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![Event::PointerMoved(press), pointer_button(press, true)],
    );
    run_outer_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![Event::PointerMoved(press - vec2(12.0, 0.0))],
    );
    run_outer_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![Event::PointerMoved(press - vec2(12.0, 0.0))],
    );
    assert!(matches!(
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    let drag_pointer = press - vec2(12.0, 0.0);
    run_outer_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![Event::PointerMoved(drag_pointer)],
    );
    warm_outer_with_size(&context, &mut dockspace, &mut panes, size);

    let edge_scroll = run_outer_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![Event::PointerMoved(edge)],
    );
    assert!(
        edge_scroll.pointer_receivers_current,
        "the edge action is reduced against the prior authoritative receiver view"
    );
    let projected =
        run_outer_frame_with_size(&context, &mut dockspace, &mut panes, size, Vec::new());
    assert!(
        !projected.pointer_receivers_current,
        "the changed scroll projection remains fail-closed until it is presented"
    );
    warm_outer_with_size(&context, &mut dockspace, &mut panes, size);

    assert!(matches!(
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    assert_eq!(
        published_tab_rect(&dockspace, ITEM_A),
        Some(background_before)
    );
    assert_ne!(
        published_tab_rect(&dockspace, ITEM_D),
        Some(foreground_before)
    );
}

#[test]
fn fully_hidden_overflow_item_addition_and_removal_force_a_fresh_multipass() {
    let context = Context::default();
    let size = vec2(180.0, 200.0);
    let mut dockspace = Dockspace::builder(
        "overflow-fingerprint",
        single_workspace([ITEM_A, ITEM_B, ITEM_C]),
    )
    .build()
    .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B, ITEM_C, ITEM_D]);
    run_frame_with_size(&context, &mut dockspace, &mut panes, size, Vec::new());
    let stable = run_frame_with_size(&context, &mut dockspace, &mut panes, size, Vec::new());
    assert!(
        stable
            .last()
            .expect("stable frame paints")
            .retained_presentation_current
    );
    assert!(published_tab_rect(&dockspace, ITEM_C).is_none());

    let added_workspace = single_workspace([ITEM_A, ITEM_B, ITEM_C, ITEM_D]);
    dockspace
        .replace_workspace(added_workspace.clone())
        .expect("hidden-item addition commits immediately");
    assert_eq!(dockspace.core_engine().workspace(), &added_workspace);
    let added = run_frame_with_size(&context, &mut dockspace, &mut panes, size, Vec::new());
    assert!(
        !added
            .first()
            .expect("addition paints one stale pass")
            .retained_presentation_current
    );
    assert!(
        !added
            .last()
            .expect("addition paints its candidate without same-sequence authority")
            .retained_presentation_current
    );
    let added_current = run_frame_with_size(&context, &mut dockspace, &mut panes, size, Vec::new());
    assert!(
        added_current
            .last()
            .expect("addition reaches a later authoritative frame")
            .retained_presentation_current
    );
    assert!(published_tab_rect(&dockspace, ITEM_D).is_none());

    let removed_workspace = single_workspace([ITEM_A, ITEM_B, ITEM_C]);
    dockspace
        .replace_workspace(removed_workspace.clone())
        .expect("hidden-item removal commits immediately");
    assert_eq!(dockspace.core_engine().workspace(), &removed_workspace);
    let removed = run_frame_with_size(&context, &mut dockspace, &mut panes, size, Vec::new());
    assert!(
        !removed
            .first()
            .expect("removal paints one stale pass")
            .retained_presentation_current
    );
    assert!(
        !removed
            .last()
            .expect("removal paints its candidate without same-sequence authority")
            .retained_presentation_current
    );
    let removed_current =
        run_frame_with_size(&context, &mut dockspace, &mut panes, size, Vec::new());
    assert!(
        removed_current
            .last()
            .expect("removal reaches a later authoritative frame")
            .retained_presentation_current
    );
}

#[test]
fn overflow_menu_accesskit_and_keyboard_selection_reveal_hidden_tabs() {
    for (salt, keyboard) in [
        ("overflow-menu-accesskit", false),
        ("overflow-menu-keyboard", true),
    ] {
        let context = Context::default();
        context.enable_accesskit();
        let size = vec2(180.0, 200.0);
        let mut dockspace = Dockspace::builder(salt, single_workspace([ITEM_A, ITEM_B, ITEM_C]))
            .build()
            .expect("facade must build");
        let mut panes = TestPanes::with_items([ITEM_A, ITEM_B, ITEM_C]);
        run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        let stable = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        let (_, overflow_node) = accesskit_node_by_label(&stable, Role::Button, "Show hidden tabs");
        assert!(overflow_node.supports_action(Action::Click));

        let mut pre_open_style = dockspace.style().clone();
        pre_open_style.tab_horizontal_padding += 1.0;
        dockspace
            .set_style(pre_open_style)
            .expect("pre-open overflow style remains valid");

        let pre_open_painted =
            run_frame_with_size(&context, &mut dockspace, &mut panes, size, Vec::new());
        assert!(
            !pre_open_painted
                .last()
                .expect("the changed pre-open projection paints")
                .local_actions_current
        );
        let pre_open_current =
            run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        assert!(paint_projection_is_authoritative(&dockspace));
        let (overflow, _) =
            accesskit_node_by_label(&pre_open_current, Role::Button, "Show hidden tabs");

        run_accesskit_frame(
            &context,
            &mut dockspace,
            &mut panes,
            size,
            vec![accesskit_action(overflow, Action::Click)],
        );
        let open = run_authoritative_accesskit_frame(&context, &mut dockspace, &mut panes, size);
        let hidden_label = format!("Pane {}", ITEM_C.get());
        let (hidden_before_stale_pass, hidden_node) =
            accesskit_node_by_label(&open, Role::MenuItem, &hidden_label);
        assert!(hidden_node.supports_action(Action::Focus));
        assert!(hidden_node.supports_action(Action::Click));
        assert!(hidden_node.author_id().is_some());

        let mut changed_style = dockspace.style().clone();
        changed_style.tab_bar_height += 1.0;
        dockspace
            .set_style(changed_style)
            .expect("changed overflow style remains valid");
        let after_stale_pass =
            run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        assert!(
            after_stale_pass.nodes.iter().all(|(_, node)| {
                !matches!(node.role(), Role::Menu | Role::MenuItem | Role::ScrollBar)
                    || node.is_disabled()
            }),
            "a stale projection may paint popup chrome but must not publish actionable controls"
        );
        assert!(!paint_projection_is_authoritative(&dockspace));
        let after_current =
            run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        assert!(paint_projection_is_authoritative(&dockspace));
        let (_, overflow_after_style) =
            accesskit_node_by_label(&after_current, Role::Button, "Show hidden tabs");
        assert_eq!(overflow_after_style.is_expanded(), Some(true));
        let (hidden, _) = accesskit_node_by_label(&after_current, Role::MenuItem, &hidden_label);
        assert_eq!(hidden, hidden_before_stale_pass);

        if keyboard {
            run_accesskit_frame(
                &context,
                &mut dockspace,
                &mut panes,
                size,
                vec![accesskit_action(hidden, Action::Focus)],
            );
            let _ = run_authoritative_accesskit_frame(&context, &mut dockspace, &mut panes, size);
            run_accesskit_frame(
                &context,
                &mut dockspace,
                &mut panes,
                size,
                vec![Event::Key {
                    key: Key::Enter,
                    physical_key: Some(Key::Enter),
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                }],
            );
        } else {
            run_accesskit_frame(
                &context,
                &mut dockspace,
                &mut panes,
                size,
                vec![accesskit_action(hidden, Action::Click)],
            );
        }
        run_frame_with_size(&context, &mut dockspace, &mut panes, size, Vec::new());
        run_frame_with_size(&context, &mut dockspace, &mut panes, size, Vec::new());

        assert_eq!(
            selected_item(&dockspace, MAIN_ROOT),
            Some(ITEM_C),
            "overflow menu activation failed for keyboard={keyboard}"
        );
        assert_eq!(
            published_tab_rect(&dockspace, ITEM_C).map(LogicalRect::width),
            Some(72.0)
        );
    }
}

#[test]
fn keyboard_opening_overflow_menu_does_not_activate_its_first_item() {
    for (salt, key) in [
        ("overflow-open-enter", Key::Enter),
        ("overflow-open-space", Key::Space),
    ] {
        let context = Context::default();
        context.enable_accesskit();
        let size = vec2(180.0, 200.0);
        let mut dockspace = Dockspace::builder(salt, single_workspace([ITEM_A, ITEM_B, ITEM_C]))
            .build()
            .expect("facade must build");
        let mut panes = TestPanes::with_items([ITEM_A, ITEM_B, ITEM_C]);

        run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        let stable = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        let (overflow, _) = accesskit_node_by_label(&stable, Role::Button, "Show hidden tabs");
        run_accesskit_frame(
            &context,
            &mut dockspace,
            &mut panes,
            size,
            vec![accesskit_action(overflow, Action::Focus)],
        );
        let focused = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        assert_eq!(focused.focus, overflow);

        let action_frame =
            run_accesskit_frame(&context, &mut dockspace, &mut panes, size, key_press(key));
        let (_, overflow_node) =
            accesskit_node_by_label(&action_frame, Role::Button, "Show hidden tabs");
        assert_eq!(overflow_node.is_expanded(), Some(false));
        assert!(
            action_frame
                .nodes
                .iter()
                .all(|(_, node)| node.role() != Role::MenuItem)
        );

        let (painted, local_actions_current) = run_accesskit_frame_with_authority(
            &context,
            &mut dockspace,
            &mut panes,
            size,
            Vec::new(),
        );
        assert!(!local_actions_current);
        let (_, painted_overflow) =
            accesskit_node_by_label(&painted, Role::Button, "Show hidden tabs");
        assert_eq!(painted_overflow.is_expanded(), Some(true));
        accesskit_node_by_label(&painted, Role::MenuItem, &format!("Pane {}", ITEM_B.get()));

        let settled = run_authoritative_accesskit_frame(&context, &mut dockspace, &mut panes, size);
        assert_eq!(selected_item(&dockspace, MAIN_ROOT), Some(ITEM_A));
        assert_eq!(
            accesskit_node_by_id(&settled, overflow).is_expanded(),
            Some(true)
        );
    }
}

#[test]
fn external_popup_state_does_not_override_the_authoritative_overflow_menu() {
    for (salt, competing_popup) in [
        ("overflow-external-close-all", false),
        ("overflow-competing-popup", true),
    ] {
        let context = Context::default();
        context.enable_accesskit();
        let size = vec2(180.0, 200.0);
        let mut dockspace = Dockspace::builder(salt, single_workspace([ITEM_A, ITEM_B, ITEM_C]))
            .build()
            .expect("facade must build");
        let mut panes = TestPanes::with_items([ITEM_A, ITEM_B, ITEM_C]);

        run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        let stable = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        let (overflow, _) = accesskit_node_by_label(&stable, Role::Button, "Show hidden tabs");
        run_accesskit_frame(
            &context,
            &mut dockspace,
            &mut panes,
            size,
            vec![accesskit_action(overflow, Action::Click)],
        );
        let opened = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        assert_eq!(
            accesskit_node_by_id(&opened, overflow).is_expanded(),
            Some(true)
        );

        let competing_id = Id::new((salt, "competing-popup"));
        let observed = if competing_popup {
            run_accesskit_frame_with_competing_popup(
                &context,
                &mut dockspace,
                &mut panes,
                size,
                competing_id,
            );
            run_accesskit_frame_with_competing_popup(
                &context,
                &mut dockspace,
                &mut panes,
                size,
                competing_id,
            )
        } else {
            egui::Popup::close_all(&context);
            run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new())
        };
        assert_eq!(
            accesskit_node_by_id(&observed, overflow).is_expanded(),
            Some(true),
            "external egui popup memory cannot mutate the core-owned menu session"
        );
        assert_eq!(
            egui::Popup::is_id_open(&context, competing_id),
            competing_popup,
            "the core-owned menu must not disturb a competing egui popup"
        );
        let still_open =
            run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        assert_eq!(
            accesskit_node_by_id(&still_open, overflow).is_expanded(),
            Some(true)
        );
    }
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "the same popup must prove all four host edges across multiple physical scales"
)]
fn overflow_popup_rounding_stays_inside_a_non_grid_host_when_opening_above() {
    for pixels_per_point in [0.75, 1.0, 1.25, 1.5, 2.0, 3.0] {
        let context = Context::default();
        context.enable_accesskit();
        context.set_pixels_per_point(pixels_per_point);
        let screen = Rect::from_min_size(Pos2::new(10.17, 20.23), vec2(180.03, 140.03));
        let dock = Rect::from_min_size(
            Pos2::new(screen.min.x, screen.max.y - 32.0),
            vec2(screen.width(), 32.0),
        );
        let items = numbered_items(2_000, 32);
        let mut dockspace = Dockspace::builder(
            ("strict-popup-rounding", pixels_per_point.to_bits()),
            single_workspace(items.iter().copied()),
        )
        .build()
        .expect("facade must build");
        let mut panes = TestPanes::with_items(items.iter().copied());
        for item in &items {
            panes.titles.insert(
                *item,
                format!(
                    "An intentionally wide overflow entry for pane {}",
                    item.get()
                ),
            );
        }

        run_accesskit_frame_in_rect(
            &context,
            &mut dockspace,
            &mut panes,
            screen,
            dock,
            Vec::new(),
        );
        let stable = run_accesskit_frame_in_rect(
            &context,
            &mut dockspace,
            &mut panes,
            screen,
            dock,
            Vec::new(),
        );
        let (overflow, _) = accesskit_node_by_label(&stable, Role::Button, "Show hidden tabs");
        run_accesskit_frame_in_rect(
            &context,
            &mut dockspace,
            &mut panes,
            screen,
            dock,
            vec![accesskit_action(overflow, Action::Click)],
        );
        let (opening, opening_current) = run_accesskit_frame_in_rect_with_authority(
            &context,
            &mut dockspace,
            &mut panes,
            screen,
            dock,
            Vec::new(),
        );
        let opening_menu = accesskit_node_rect(accesskit_node_by_role(&opening, Role::Menu).1);
        assert!(
            !opening_current,
            "the first painted popup candidate cannot acknowledge itself"
        );
        let opened = run_authoritative_accesskit_frame_in_rect(
            &context,
            &mut dockspace,
            &mut panes,
            screen,
            dock,
        );
        let (_, menu) = accesskit_node_by_role(&opened, Role::Menu);
        let menu = accesskit_node_rect(menu);
        let overflow_rect = accesskit_node_rect(accesskit_node_by_id(&opened, overflow));

        assert_eq!(
            menu, opening_menu,
            "popup geometry shifted after its opening frame at {pixels_per_point} PPP"
        );
        let physical_rounding_tolerance = 0.5 / pixels_per_point + egui::emath::GUI_ROUNDING;
        assert!(
            menu.min.x + physical_rounding_tolerance >= screen.min.x,
            "left edge of {menu:?} escaped {screen:?} at {pixels_per_point} PPP"
        );
        assert!(
            menu.min.y + physical_rounding_tolerance >= screen.min.y,
            "top edge of {menu:?} escaped {screen:?} at {pixels_per_point} PPP"
        );
        assert!(
            menu.max.x <= screen.max.x + physical_rounding_tolerance,
            "right edge of {menu:?} escaped {screen:?} at {pixels_per_point} PPP"
        );
        assert!(
            menu.max.y <= screen.max.y + physical_rounding_tolerance,
            "bottom edge of {menu:?} escaped {screen:?} at {pixels_per_point} PPP"
        );
        assert!(
            menu.max.y <= overflow_rect.min.y + 0.5 / pixels_per_point + egui::emath::GUI_ROUNDING,
            "the fixture popup {menu:?} must open above {overflow_rect:?} at {pixels_per_point} PPP"
        );
        assert!(
            menu.min.x - screen.min.x < 2.0 / pixels_per_point + 1.0,
            "the wide popup {menu:?} must exercise the left host budget {screen:?} at {pixels_per_point} PPP"
        );
    }
}

#[test]
fn popup_smaller_than_its_frame_closes_without_resurrecting_adapter_state() {
    let context = Context::default();
    context.enable_accesskit();
    context.set_pixels_per_point(0.75);
    let screen = Rect::from_min_size(Pos2::new(10.17, 20.23), vec2(180.03, 140.03));
    let dock = Rect::from_min_size(screen.min, screen.size());
    let items = numbered_items(2_100, 16);
    let mut dockspace = Dockspace::builder(
        "overflow-popup-smaller-than-frame",
        single_workspace(items.iter().copied()),
    )
    .build()
    .expect("facade must build");
    let mut panes = TestPanes::with_items(items.iter().copied());

    run_accesskit_frame_in_rect(
        &context,
        &mut dockspace,
        &mut panes,
        screen,
        dock,
        Vec::new(),
    );
    let stable = run_accesskit_frame_in_rect(
        &context,
        &mut dockspace,
        &mut panes,
        screen,
        dock,
        Vec::new(),
    );
    let (overflow, _) = accesskit_node_by_label(&stable, Role::Button, "Show hidden tabs");
    run_accesskit_frame_in_rect(
        &context,
        &mut dockspace,
        &mut panes,
        screen,
        dock,
        vec![accesskit_action(overflow, Action::Click)],
    );
    let opened = run_authoritative_accesskit_frame_in_rect(
        &context,
        &mut dockspace,
        &mut panes,
        screen,
        dock,
    );
    assert_eq!(
        accesskit_node_by_id(&opened, overflow).is_expanded(),
        Some(true)
    );

    let tiny_screen = Rect::from_min_size(screen.min, vec2(3.0, 3.0));
    let tiny_dock = Rect::from_min_size(
        Pos2::new(tiny_screen.max.x - dock.width(), tiny_screen.min.y),
        dock.size(),
    );
    let (tiny, local_actions_current) = run_accesskit_frame_in_rect_with_authority(
        &context,
        &mut dockspace,
        &mut panes,
        tiny_screen,
        tiny_dock,
        Vec::new(),
    );
    assert!(!local_actions_current);
    assert!(
        tiny.nodes
            .iter()
            .filter(|(_, node)| matches!(
                node.role(),
                Role::Menu | Role::MenuItem | Role::ScrollBar
            ))
            .all(|(_, node)| node.is_disabled()),
        "the prior popup may paint during settlement but cannot remain interactive"
    );

    let closed = run_authoritative_accesskit_frame_in_rect(
        &context,
        &mut dockspace,
        &mut panes,
        tiny_screen,
        tiny_dock,
    );
    assert!(
        closed
            .nodes
            .iter()
            .all(|(_, node)| node.role() != Role::Menu)
    );

    let restored = run_authoritative_accesskit_frame_in_rect(
        &context,
        &mut dockspace,
        &mut panes,
        screen,
        dock,
    );
    let (_, restored_overflow) =
        accesskit_node_by_label(&restored, Role::Button, "Show hidden tabs");
    assert_eq!(restored_overflow.is_expanded(), Some(false));
    assert!(
        restored
            .nodes
            .iter()
            .all(|(_, node)| node.role() != Role::Menu)
    );
}

#[test]
fn popup_closes_when_a_solid_scrollbar_cannot_fit_the_host_width() {
    let context = Context::default();
    context.enable_accesskit();
    context.set_pixels_per_point(0.75);
    context.all_styles_mut(|style| {
        style.spacing.scroll.floating = false;
        style.spacing.scroll.bar_width = 18.17;
        style.spacing.scroll.bar_inner_margin = 4.13;
        style.spacing.scroll.bar_outer_margin = 4.07;
    });
    let screen = Rect::from_min_size(Pos2::new(10.17, 20.23), vec2(180.0, 140.0));
    let dock = screen;
    let items = numbered_items(2_200, 32);
    let mut dockspace = Dockspace::builder(
        "overflow-scrollbar-wider-than-host",
        single_workspace(items.iter().copied()),
    )
    .build()
    .expect("facade must build");
    let mut panes = TestPanes::with_items(items.iter().copied());

    run_accesskit_frame_in_rect(
        &context,
        &mut dockspace,
        &mut panes,
        screen,
        dock,
        Vec::new(),
    );
    let stable = run_accesskit_frame_in_rect(
        &context,
        &mut dockspace,
        &mut panes,
        screen,
        dock,
        Vec::new(),
    );
    let (overflow, _) = accesskit_node_by_label(&stable, Role::Button, "Show hidden tabs");
    run_accesskit_frame_in_rect(
        &context,
        &mut dockspace,
        &mut panes,
        screen,
        dock,
        vec![accesskit_action(overflow, Action::Click)],
    );
    let opened = run_authoritative_accesskit_frame_in_rect(
        &context,
        &mut dockspace,
        &mut panes,
        screen,
        dock,
    );
    assert_eq!(
        accesskit_node_by_id(&opened, overflow).is_expanded(),
        Some(true)
    );

    let narrow_screen = Rect::from_min_size(screen.min, vec2(35.0, 60.0));
    let narrow_dock = Rect::from_min_size(
        Pos2::new(narrow_screen.max.x - dock.width(), narrow_screen.min.y),
        dock.size(),
    );
    let popup_style = context.global_style();
    let frame_margin = Frame::popup(&popup_style).total_margin();
    let frame_width = frame_margin.left + frame_margin.right;
    let scrollbar_width = popup_style.spacing.scroll.allocated_width();
    assert!(narrow_screen.width() > frame_width);
    assert!(narrow_screen.width() < frame_width + scrollbar_width);

    let (narrow, local_actions_current) = run_accesskit_frame_in_rect_with_authority(
        &context,
        &mut dockspace,
        &mut panes,
        narrow_screen,
        narrow_dock,
        Vec::new(),
    );
    assert!(!local_actions_current);
    assert!(
        narrow
            .nodes
            .iter()
            .filter(|(_, node)| matches!(
                node.role(),
                Role::Menu | Role::MenuItem | Role::ScrollBar
            ))
            .all(|(_, node)| node.is_disabled()),
        "the prior popup may paint during settlement but cannot remain interactive"
    );

    let closed = run_authoritative_accesskit_frame_in_rect(
        &context,
        &mut dockspace,
        &mut panes,
        narrow_screen,
        narrow_dock,
    );
    assert!(
        closed
            .nodes
            .iter()
            .all(|(_, node)| node.role() != Role::Menu)
    );

    let restored = run_authoritative_accesskit_frame_in_rect(
        &context,
        &mut dockspace,
        &mut panes,
        screen,
        dock,
    );
    let (_, overflow) = accesskit_node_by_label(&restored, Role::Button, "Show hidden tabs");
    assert_eq!(overflow.is_expanded(), Some(false));
    assert!(
        restored
            .nodes
            .iter()
            .all(|(_, node)| node.role() != Role::Menu)
    );
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "the scrollbar transition must compare opening and resize allocation in one persistent popup"
)]
fn overflow_scrollbar_and_rows_keep_their_first_frame_width_allocation() {
    let context = Context::default();
    context.enable_accesskit();
    context.all_styles_mut(|style| {
        style.spacing.scroll.floating = false;
        style.spacing.scroll.bar_width = 15.17;
        style.spacing.scroll.bar_inner_margin = 1.13;
        style.spacing.scroll.bar_outer_margin = 2.07;
    });
    let size = vec2(180.03, 140.03);
    let items = numbered_items(1_500, 32);
    let mut dockspace = Dockspace::builder(
        "overflow-scrollbar-allocation",
        single_workspace(items.iter().copied()),
    )
    .build()
    .expect("facade must build");
    let mut panes = TestPanes::with_items(items.iter().copied());
    for item in &items {
        panes.titles.insert(
            *item,
            format!(
                "An intentionally wide overflow entry for pane {}",
                item.get()
            ),
        );
    }

    run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
    let stable = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
    let (overflow, _) = accesskit_node_by_label(&stable, Role::Button, "Show hidden tabs");
    run_accesskit_frame(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![accesskit_action(overflow, Action::Click)],
    );
    let opened = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
    let (_, opened_menu) = accesskit_node_by_role(&opened, Role::Menu);
    let opened_menu_bounds = opened_menu.bounds().expect("menu has bounds");
    let (_, opened_scrollbar) = accesskit_node_by_role(&opened, Role::ScrollBar);
    let opened_scrollbar_bounds = opened_scrollbar.bounds().expect("scrollbar has bounds");
    let first_hidden_label = format!(
        "An intentionally wide overflow entry for pane {}",
        items[1].get()
    );
    let (_, opened_first_hidden) =
        accesskit_node_by_label(&opened, Role::MenuItem, &first_hidden_label);
    let opened_first_hidden_bounds = opened_first_hidden
        .bounds()
        .expect("overflow row has bounds");
    for (_, node) in opened
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == Role::MenuItem)
    {
        let bounds = node.bounds().expect("overflow row has bounds");
        assert!(bounds.x0 >= opened_menu_bounds.x0);
        assert!(bounds.x1 <= opened_menu_bounds.x1);
    }

    let settled = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
    let (_, settled_menu) = accesskit_node_by_role(&settled, Role::Menu);
    let (_, settled_scrollbar) = accesskit_node_by_role(&settled, Role::ScrollBar);
    let (_, settled_first_hidden) =
        accesskit_node_by_label(&settled, Role::MenuItem, &first_hidden_label);
    assert_eq!(
        settled_scrollbar.bounds(),
        Some(opened_scrollbar_bounds),
        "scrollbar allocation must not animate after opening; popup changed from {opened_menu_bounds:?} to {:?}",
        settled_menu.bounds()
    );
    assert_eq!(
        settled_first_hidden.bounds(),
        Some(opened_first_hidden_bounds),
        "row allocation must agree with the scrollbar width on the opening frame"
    );

    let tall_size = vec2(size.x, 1_200.0);
    let mut tall = None;
    for _ in 0..20 {
        tall = Some(run_accesskit_frame(
            &context,
            &mut dockspace,
            &mut panes,
            tall_size,
            Vec::new(),
        ));
    }
    let tall = tall.expect("resize sequence paints at least one frame");
    assert!(
        tall.nodes
            .iter()
            .all(|(_, node)| node.role() != Role::ScrollBar),
        "the tall popup establishes a hidden-scrollbar animation state"
    );

    let stale_resize = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
    assert!(
        stale_resize
            .nodes
            .iter()
            .filter(|(_, node)| node.role() == Role::ScrollBar)
            .all(|(_, node)| node.is_disabled()),
        "the replacement scrollbar may paint immediately but cannot interact before acknowledgement"
    );
    assert!(!paint_projection_is_authoritative(&dockspace));
    let (_, stale_scrollbar) = accesskit_node_by_role(&stale_resize, Role::ScrollBar);
    let stale_scrollbar_bounds = stale_scrollbar.bounds();
    let (_, stale_first_hidden) =
        accesskit_node_by_label(&stale_resize, Role::MenuItem, &first_hidden_label);
    let stale_first_hidden_bounds = stale_first_hidden.bounds();
    let resized = run_authoritative_accesskit_frame(&context, &mut dockspace, &mut panes, size);
    assert!(paint_projection_is_authoritative(&dockspace));
    let (_, overflow_after_resize) =
        accesskit_node_by_label(&resized, Role::Button, "Show hidden tabs");
    assert_eq!(overflow_after_resize.is_expanded(), Some(true));
    let (_, resized_scrollbar) = accesskit_node_by_role(&resized, Role::ScrollBar);
    let resized_scrollbar_bounds = resized_scrollbar.bounds();
    let (_, resized_first_hidden) =
        accesskit_node_by_label(&resized, Role::MenuItem, &first_hidden_label);
    let resized_first_hidden_bounds = resized_first_hidden.bounds();
    assert_eq!(resized_scrollbar_bounds, stale_scrollbar_bounds);
    assert_eq!(resized_first_hidden_bounds, stale_first_hidden_bounds);
    let mut resized_settled = None;
    for _ in 0..20 {
        resized_settled = Some(run_accesskit_frame(
            &context,
            &mut dockspace,
            &mut panes,
            size,
            Vec::new(),
        ));
    }
    let resized_settled = resized_settled.expect("resize settlement paints at least one frame");
    let (_, final_scrollbar) = accesskit_node_by_role(&resized_settled, Role::ScrollBar);
    let (_, final_first_hidden) =
        accesskit_node_by_label(&resized_settled, Role::MenuItem, &first_hidden_label);
    assert_eq!(
        final_scrollbar.bounds(),
        resized_scrollbar_bounds,
        "solid scrollbar allocation must jump to its final width on the resize frame"
    );
    assert_eq!(
        final_first_hidden.bounds(),
        resized_first_hidden_bounds,
        "row width must not animate while solid scrollbar space is reserved"
    );
}

#[test]
fn overflow_scrollbar_accesskit_adjustments_commit_one_row_steps() {
    let context = Context::default();
    context.enable_accesskit();
    let size = vec2(180.0, 140.0);
    let items = numbered_items(1_700, 32);
    let mut dockspace = Dockspace::builder(
        "overflow-scrollbar-accesskit-adjustment",
        single_workspace(items.iter().copied()),
    )
    .build()
    .expect("facade must build");
    let mut panes = TestPanes::with_items(items);

    run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
    let stable = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
    let (overflow, _) = accesskit_node_by_label(&stable, Role::Button, "Show hidden tabs");
    run_accesskit_frame(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![accesskit_action(overflow, Action::Click)],
    );
    let opened = run_authoritative_accesskit_frame(&context, &mut dockspace, &mut panes, size);
    let (scrollbar, scrollbar_node) = accesskit_node_by_role(&opened, Role::ScrollBar);
    assert!(scrollbar_node.supports_action(Action::Increment));
    assert!(scrollbar_node.supports_action(Action::Decrement));
    let initial_offset = scrollbar_node
        .numeric_value()
        .expect("the scrollbar exposes its authoritative offset");
    let row_height = opened
        .nodes
        .iter()
        .find_map(|(_, node)| {
            (node.role() == Role::MenuItem)
                .then(|| node.bounds())
                .flatten()
                .map(|bounds| bounds.y1 - bounds.y0)
        })
        .expect("the menu exposes at least one measured row");

    run_accesskit_frame(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![accesskit_action(scrollbar, Action::Increment)],
    );
    let incremented = run_authoritative_accesskit_frame(&context, &mut dockspace, &mut panes, size);
    let (scrollbar, scrollbar_node) = accesskit_node_by_role(&incremented, Role::ScrollBar);
    let incremented_offset = scrollbar_node
        .numeric_value()
        .expect("the incremented scrollbar exposes its offset");
    assert!((incremented_offset - initial_offset - row_height).abs() <= f64::EPSILON);

    run_accesskit_frame(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![accesskit_action(scrollbar, Action::Decrement)],
    );
    let decremented = run_authoritative_accesskit_frame(&context, &mut dockspace, &mut panes, size);
    let (_, scrollbar_node) = accesskit_node_by_role(&decremented, Role::ScrollBar);
    assert_eq!(scrollbar_node.numeric_value(), Some(initial_offset));
}

#[test]
fn authority_incomplete_wheel_is_not_consumed_as_docking_input() {
    let context = Context::default();
    context.enable_accesskit();
    let salt = "overflow-ancestor-scroll";
    let size = vec2(220.0, 180.0);
    let items = numbered_items(1_600, 64);
    let mut dockspace = Dockspace::builder(salt, single_workspace(items.iter().copied()))
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items(items.iter().copied());

    run_accesskit_frame_in_ancestor_scroll_area(
        &context,
        &mut dockspace,
        &mut panes,
        salt,
        size,
        Some(100.0),
        Vec::new(),
    );
    let (stable, baseline_offset, _) = run_accesskit_frame_in_ancestor_scroll_area(
        &context,
        &mut dockspace,
        &mut panes,
        salt,
        size,
        None,
        Vec::new(),
    );
    assert!(
        baseline_offset > 50.0,
        "fixture starts away from both boundaries"
    );
    let (overflow, _) = accesskit_node_by_label(&stable, Role::Button, "Show hidden tabs");
    run_accesskit_frame_in_ancestor_scroll_area(
        &context,
        &mut dockspace,
        &mut panes,
        salt,
        size,
        None,
        vec![accesskit_action(overflow, Action::Click)],
    );
    let (mut opened, mut opened_outer_offset, _) = run_accesskit_frame_in_ancestor_scroll_area(
        &context,
        &mut dockspace,
        &mut panes,
        salt,
        size,
        None,
        Vec::new(),
    );
    for _ in 0..8 {
        if dockspace
            .core_engine()
            .interaction_projection(SURFACE)
            .is_some_and(|projection| !projection.plan().tab_list_menu_records().is_empty())
        {
            break;
        }
        (opened, opened_outer_offset, _) = run_accesskit_frame_in_ancestor_scroll_area(
            &context,
            &mut dockspace,
            &mut panes,
            salt,
            size,
            None,
            Vec::new(),
        );
    }
    assert!((opened_outer_offset - baseline_offset).abs() < 0.01);
    let (_, menu) = accesskit_node_by_role(&opened, Role::Menu);
    let pointer = accesskit_node_rect(menu).center();
    let menu_offset_before = dockspace
        .core_engine()
        .interaction_projection(SURFACE)
        .and_then(|projection| projection.plan().tab_list_menu_records().first())
        .map(dockspace::backend::scene::TabListMenuRecord::scroll_offset)
        .expect("the popup is present in the authoritative projection");

    let (_, mut latest_outer_offset, wheel_available_after_dockspace) =
        run_accesskit_frame_in_ancestor_scroll_area(
            &context,
            &mut dockspace,
            &mut panes,
            salt,
            size,
            None,
            vec![
                Event::PointerMoved(pointer),
                Event::MouseWheel {
                    unit: MouseWheelUnit::Point,
                    delta: vec2(0.0, -10_000.0),
                    phase: TouchPhase::Move,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
    for _ in 0..8 {
        let (_, offset, _) = run_accesskit_frame_in_ancestor_scroll_area(
            &context,
            &mut dockspace,
            &mut panes,
            salt,
            size,
            None,
            Vec::new(),
        );
        latest_outer_offset = offset;
    }
    assert!(
        wheel_available_after_dockspace,
        "dockspace must leave an authority-incomplete wheel available to framework routing"
    );
    assert!(
        (latest_outer_offset - baseline_offset).abs() < 0.01,
        "egui's top-layer routing still prevents the popup wheel from scrolling its ancestor"
    );
    assert_eq!(
        dockspace
            .core_engine()
            .interaction_projection(SURFACE)
            .and_then(|projection| projection.plan().tab_list_menu_records().first())
            .map(dockspace::backend::scene::TabListMenuRecord::scroll_offset),
        Some(menu_offset_before),
        "the same wheel cannot mutate docking state without a conforming receiver receipt"
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn authority_incomplete_popup_wheel_never_mutates_a_docking_scroll_owner() {
    for (salt, floating_underlay) in [
        ("popup-over-tiled-strip", false),
        ("popup-over-floating-strip", true),
    ] {
        let context = Context::default();
        context.enable_accesskit();
        let size = vec2(220.0, 220.0);
        let popup_items = numbered_items(2_000, 64);
        let underlay_items = numbered_items(3_000, 4);
        let workspace =
            popup_overlapping_tab_strip_workspace(&popup_items, &underlay_items, floating_underlay);
        let mut dockspace = Dockspace::builder(salt, workspace)
            .build()
            .expect("facade must build");
        let mut panes = TestPanes::with_items(popup_items.iter().chain(&underlay_items).copied());
        for item in &popup_items {
            panes.titles.insert(
                *item,
                format!("Wide overflow menu entry for pane {}", item.get()),
            );
        }

        run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        let stable = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        let (overflow, _) = stable
            .nodes
            .iter()
            .filter(|(_, node)| {
                node.role() == Role::Button && node.label() == Some("Show hidden tabs")
            })
            .min_by(|(_, left), (_, right)| {
                left.bounds()
                    .expect("overflow button has bounds")
                    .y0
                    .total_cmp(&right.bounds().expect("overflow button has bounds").y0)
            })
            .map(|(id, node)| (*id, node))
            .expect("topmost overflow button exists");
        run_accesskit_frame(
            &context,
            &mut dockspace,
            &mut panes,
            size,
            vec![accesskit_action(overflow, Action::Click)],
        );
        run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        let menu = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        let (_, menu_node) = accesskit_node_by_role(&menu, Role::Menu);
        let menu_rect = accesskit_node_rect(menu_node);
        let popup_tab_before =
            published_tab_rect(&dockspace, popup_items[0]).expect("popup source tab is visible");
        let underlay_before = published_tab_rect(&dockspace, underlay_items[1])
            .expect("underlay overflow tab is visible");
        let overlap = menu_rect.intersect(egui_rect(underlay_before));
        assert!(
            overlap.is_positive(),
            "popup viewport covers underlay tab strip"
        );
        let pointer = overlap.center();
        let last_label = format!(
            "Wide overflow menu entry for pane {}",
            popup_items.last().expect("popup fixture has items").get()
        );
        let (_, last_before) = accesskit_node_by_label(&menu, Role::MenuItem, &last_label);
        let last_y_before = accesskit_node_center(last_before).y;
        let menu_offset_before = dockspace
            .core_engine()
            .interaction_projection(SURFACE)
            .and_then(|projection| projection.plan().tab_list_menu_records().first())
            .map(dockspace::backend::scene::TabListMenuRecord::scroll_offset)
            .expect("the popup is present in the authoritative projection");

        run_accesskit_frame(
            &context,
            &mut dockspace,
            &mut panes,
            size,
            vec![
                Event::PointerMoved(pointer),
                Event::MouseWheel {
                    unit: MouseWheelUnit::Point,
                    delta: vec2(0.0, -10_000.0),
                    phase: TouchPhase::Move,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
        run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        let scrolled = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        let (_, last_after) = accesskit_node_by_label(&scrolled, Role::MenuItem, &last_label);
        assert_eq!(accesskit_node_center(last_after).y, last_y_before);
        assert_eq!(
            dockspace
                .core_engine()
                .interaction_projection(SURFACE)
                .and_then(|projection| projection.plan().tab_list_menu_records().first())
                .map(dockspace::backend::scene::TabListMenuRecord::scroll_offset),
            Some(menu_offset_before),
            "neither the popup nor an overlapping strip may claim an authority-incomplete wheel"
        );
        assert_eq!(
            published_tab_rect(&dockspace, popup_items[0]),
            Some(popup_tab_before)
        );
        assert_eq!(
            published_tab_rect(&dockspace, underlay_items[1]),
            Some(underlay_before)
        );
    }
}

#[test]
fn stale_overflow_clicks_cannot_activate_or_dismiss_a_remeasured_popup() {
    for (salt, click_item) in [
        ("stale-overflow-item-click", true),
        ("stale-overflow-outside-click", false),
    ] {
        let context = Context::default();
        context.enable_accesskit();
        let size = vec2(180.0, 200.0);
        let mut dockspace = Dockspace::builder(salt, single_workspace([ITEM_A, ITEM_B, ITEM_C]))
            .build()
            .expect("facade must build");
        let mut panes = TestPanes::with_items([ITEM_A, ITEM_B, ITEM_C]);

        run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        let stable = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        let (overflow, _) = accesskit_node_by_label(&stable, Role::Button, "Show hidden tabs");
        run_accesskit_frame(
            &context,
            &mut dockspace,
            &mut panes,
            size,
            vec![accesskit_action(overflow, Action::Click)],
        );
        run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        let authoritative_menu =
            run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        assert!(paint_projection_is_authoritative(&dockspace));
        let old_label = format!("Pane {}", ITEM_C.get());
        let (_, hidden) = accesskit_node_by_label(&authoritative_menu, Role::MenuItem, &old_label);
        let stale_click = if click_item {
            accesskit_node_center(hidden)
        } else {
            Pos2::new(4.0, size.y - 4.0)
        };

        panes.titles.insert(
            ITEM_C,
            format!("Pane {} with a much wider overflow label", ITEM_C.get()),
        );
        context.all_styles_mut(|style| {
            style.spacing.item_spacing.y += 3.0;
            style.spacing.default_area_size += vec2(7.0, 11.0);
            style.visuals.window_stroke.width += 1.0;
        });
        context.options_mut(|options| {
            options.max_passes = 1.try_into().expect("one is non-zero");
        });
        let stale_frame = run_frame_with_size(
            &context,
            &mut dockspace,
            &mut panes,
            size,
            vec![
                Event::PointerMoved(stale_click),
                pointer_button(stale_click, true),
                pointer_button(stale_click, false),
            ],
        );

        assert_eq!(stale_frame.len(), 1, "the one-pass budget must fail closed");
        assert!(!stale_frame[0].local_actions_current);
        assert_eq!(selected_item(&dockspace, MAIN_ROOT), Some(ITEM_A));

        context.options_mut(|options| {
            options.max_passes = 4.try_into().expect("four is non-zero");
        });
        let recovered = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        assert!(
            recovered.nodes.iter().all(|(_, node)| {
                !matches!(node.role(), Role::Menu | Role::MenuItem | Role::ScrollBar)
                    || node.is_disabled()
            }),
            "the first recovery sequence may paint replacement popup chrome but cannot authorize it"
        );
        assert!(!paint_projection_is_authoritative(&dockspace));
        let authoritative =
            run_authoritative_accesskit_frame(&context, &mut dockspace, &mut panes, size);
        assert!(paint_projection_is_authoritative(&dockspace));
        let (_, overflow_after_recovery) =
            accesskit_node_by_label(&authoritative, Role::Button, "Show hidden tabs");
        assert_eq!(overflow_after_recovery.is_expanded(), Some(true));
        let (_, remeasured_hidden) = accesskit_node_by_label(
            &authoritative,
            Role::MenuItem,
            &format!("Pane {} with a much wider overflow label", ITEM_C.get()),
        );
        assert!(!remeasured_hidden.is_disabled());
        assert_eq!(selected_item(&dockspace, MAIN_ROOT), Some(ITEM_A));
    }
}

#[test]
fn authoritative_overflow_gap_click_keeps_the_popup_open() {
    let context = Context::default();
    context.enable_accesskit();
    context.all_styles_mut(|style| {
        style.spacing.item_spacing.y = 12.0;
    });
    let size = vec2(180.0, 200.0);
    let mut dockspace = Dockspace::builder(
        "authoritative-overflow-gap-click",
        single_workspace([ITEM_A, ITEM_B, ITEM_C]),
    )
    .build()
    .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B, ITEM_C]);

    run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
    let stable = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
    let (overflow, _) = accesskit_node_by_label(&stable, Role::Button, "Show hidden tabs");
    run_accesskit_frame(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![accesskit_action(overflow, Action::Click)],
    );
    let opened = run_authoritative_accesskit_frame(&context, &mut dockspace, &mut panes, size);
    let first = accesskit_node_rect(
        accesskit_node_by_label(&opened, Role::MenuItem, &format!("Pane {}", ITEM_B.get())).1,
    );
    let second = accesskit_node_rect(
        accesskit_node_by_label(&opened, Role::MenuItem, &format!("Pane {}", ITEM_C.get())).1,
    );
    assert!(second.min.y > first.max.y, "fixture must expose a menu gap");
    let shared_min_x = first.min.x.max(second.min.x);
    let shared_max_x = first.max.x.min(second.max.x);
    assert!(
        shared_max_x > shared_min_x,
        "fixture menu items must share horizontal space"
    );
    let gap = Pos2::new(
        (shared_min_x + shared_max_x) * 0.5,
        (first.max.y + second.min.y) * 0.5,
    );

    run_accesskit_frame(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![
            Event::PointerMoved(gap),
            pointer_button(gap, true),
            pointer_button(gap, false),
        ],
    );
    let still_open = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
    let (_, overflow) = accesskit_node_by_label(&still_open, Role::Button, "Show hidden tabs");

    assert_eq!(overflow.is_expanded(), Some(true));
    assert_eq!(selected_item(&dockspace, MAIN_ROOT), Some(ITEM_A));
}

#[test]
fn stale_style_projection_disables_pane_widgets_until_the_plan_is_current() {
    let context = Context::default();
    let workspace = single_workspace([ITEM_A]);
    let mut dockspace = Dockspace::builder("stale-pane-fail-closed", workspace)
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A]);
    warm(&context, &mut dockspace, &mut panes);

    context.options_mut(|options| {
        options.max_passes = 1.try_into().expect("one is non-zero");
    });
    let ui_calls_before = panes.ui_calls(ITEM_A);
    let disabled_calls_before = panes.disabled_ui_calls(ITEM_A);
    let clicks_before = panes.pane_clicks(ITEM_A);
    let mut adjusted_style = dockspace.style().clone();
    adjusted_style.tab_bar_height += 8.0;
    dockspace
        .set_style(adjusted_style)
        .expect("adjusted fixture style remains valid");

    let press = Pos2::new(300.0, 200.0);
    let stale_observations = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(press),
            pointer_button(press, true),
            pointer_button(press, false),
        ],
    );
    assert_eq!(
        stale_observations.len(),
        1,
        "the configured pass budget rejects discard"
    );
    assert_eq!(
        stale_observations[0].surface_status,
        DockspaceSurfaceStatus::Stale
    );
    assert!(!stale_observations[0].retained_presentation_current);
    assert_eq!(panes.ui_calls(ITEM_A), ui_calls_before + 1);
    assert_eq!(panes.disabled_ui_calls(ITEM_A), disabled_calls_before + 1);
    assert_eq!(panes.pane_clicks(ITEM_A), clicks_before);

    let recovered_observations = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        !recovered_observations
            .last()
            .expect("the recovered candidate paints without same-sequence authority")
            .retained_presentation_current
    );
    assert_eq!(panes.ui_calls(ITEM_A), ui_calls_before + 2);
    assert_eq!(panes.disabled_ui_calls(ITEM_A), disabled_calls_before + 1);

    let current_observations = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        current_observations
            .last()
            .expect("the sealed frame observes presentation authority before painting")
            .retained_presentation_current
    );
    assert_eq!(panes.ui_calls(ITEM_A), ui_calls_before + 3);
    assert_eq!(panes.disabled_ui_calls(ITEM_A), disabled_calls_before + 1);

    let authoritative_observations = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        authoritative_observations
            .last()
            .expect("the following host sequence observes accepted authority")
            .retained_presentation_current
    );
    assert_eq!(panes.ui_calls(ITEM_A), ui_calls_before + 4);
    assert_eq!(panes.disabled_ui_calls(ITEM_A), disabled_calls_before + 1);
    assert_eq!(panes.pane_clicks(ITEM_A), clicks_before);

    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(press),
            pointer_button(press, true),
            pointer_button(press, false),
        ],
    );
    assert_eq!(panes.pane_clicks(ITEM_A), clicks_before + 1);
}

#[test]
fn stale_selection_paints_current_pane_without_running_superseded_callback() {
    let context = Context::default();
    let workspace = single_workspace([ITEM_A, ITEM_B]);
    let mut dockspace = Dockspace::builder("selected-pane-fingerprint", workspace.clone())
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    warm(&context, &mut dockspace, &mut panes);

    select_item(&mut dockspace, ITEM_B);
    assert_eq!(selected_item(&dockspace, MAIN_ROOT), Some(ITEM_B));
    context.options_mut(|options| {
        options.max_passes = 1.try_into().expect("one is non-zero");
    });

    let b_ui_before = panes.ui_calls(ITEM_B);
    let b_disabled_before = panes.disabled_ui_calls(ITEM_B);
    let b_clicks_before = panes.pane_clicks(ITEM_B);
    let a_ui_before = panes.ui_calls(ITEM_A);
    let a_disabled_before = panes.disabled_ui_calls(ITEM_A);
    let a_clicks_before = panes.pane_clicks(ITEM_A);
    let press = Pos2::new(300.0, 200.0);
    let stale_frame = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(press),
            pointer_button(press, true),
            pointer_button(press, false),
        ],
    );
    assert_eq!(stale_frame.len(), 1, "the one-pass budget must fail closed");
    assert!(!stale_frame[0].retained_presentation_current);
    assert_eq!(panes.ui_calls(ITEM_A), a_ui_before);
    assert_eq!(panes.disabled_ui_calls(ITEM_A), a_disabled_before);
    assert_eq!(panes.ui_calls(ITEM_B), b_ui_before + 1);
    assert_eq!(panes.disabled_ui_calls(ITEM_B), b_disabled_before + 1);
    assert_eq!(panes.pane_clicks(ITEM_A), a_clicks_before);
    assert_eq!(panes.pane_clicks(ITEM_B), b_clicks_before);

    let recovered_frame = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        !recovered_frame
            .last()
            .expect("the selected candidate paints without same-sequence authority")
            .retained_presentation_current
    );
    assert_eq!(panes.ui_calls(ITEM_B), b_ui_before + 2);
    assert_eq!(panes.disabled_ui_calls(ITEM_B), b_disabled_before + 1);
    assert_eq!(panes.ui_calls(ITEM_A), a_ui_before);

    let stable_frame = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(press),
            pointer_button(press, true),
            pointer_button(press, false),
        ],
    );
    assert!(
        stable_frame
            .last()
            .expect("the sealed frame observes selection authority before input")
            .retained_presentation_current
    );
    assert_eq!(panes.ui_calls(ITEM_B), b_ui_before + 3);
    assert_eq!(panes.disabled_ui_calls(ITEM_B), b_disabled_before + 1);
    assert_eq!(panes.pane_clicks(ITEM_B), b_clicks_before + 1);
    assert_eq!(panes.ui_calls(ITEM_A), a_ui_before);

    let authoritative_frame = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        authoritative_frame
            .last()
            .expect("the following host sequence observes accepted selection authority")
            .retained_presentation_current
    );
    assert_eq!(panes.ui_calls(ITEM_B), b_ui_before + 4);
    assert_eq!(panes.disabled_ui_calls(ITEM_B), b_disabled_before + 1);
    assert_eq!(panes.pane_clicks(ITEM_B), b_clicks_before + 1);
    assert_eq!(panes.ui_calls(ITEM_A), a_ui_before);
    assert_eq!(
        dockspace
            .core_engine()
            .workspace()
            .roots()
            .next()
            .and_then(|(_, root)| dockspace.core_engine().workspace().node(root.node))
            .and_then(|node| match node {
                Node::Tabs { selected, .. } => *selected,
                Node::Split { .. } => None,
            }),
        Some(ITEM_B)
    );
}

#[test]
fn workspace_replacement_bootstrap_paints_only_the_current_pane() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("replacement-pane-identity", single_workspace([ITEM_A]))
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    warm(&context, &mut dockspace, &mut panes);

    let a_ui_before = panes.ui_calls(ITEM_A);
    let b_ui_before = panes.ui_calls(ITEM_B);
    dockspace
        .replace_workspace(single_workspace([ITEM_B]))
        .expect("replacement must commit immediately");
    context.options_mut(|options| {
        options.max_passes = 1.try_into().expect("one is non-zero");
    });

    let bootstrap = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(bootstrap.len(), 1, "the one-pass budget must fail closed");
    assert_eq!(
        bootstrap[0].surface_status,
        DockspaceSurfaceStatus::Bootstrap
    );
    assert_eq!(panes.ui_calls(ITEM_A), a_ui_before);
    assert_eq!(
        panes.ui_calls(ITEM_B),
        b_ui_before + 1,
        "the current replacement plan proves B independently of presentation authority"
    );

    for _ in 0..3 {
        run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        assert_eq!(panes.ui_calls(ITEM_A), a_ui_before);
    }
    assert!(panes.ui_calls(ITEM_B) > b_ui_before + 1);
}

#[test]
fn relocated_pane_retargets_to_its_stable_tabs_retained_geometry() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("relocated-pane-identity", split_workspace())
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    warm(&context, &mut dockspace, &mut panes);

    let (source_tabs, target_tabs) = dockspace.core_engine().workspace().nodes().fold(
        (None, None),
        |(source, target), (node, record)| match record {
            Node::Tabs { items, .. } if items.contains(&ITEM_A) => (Some(node), target),
            Node::Tabs { items, .. } if items.contains(&ITEM_B) => (source, Some(node)),
            Node::Tabs { .. } | Node::Split { .. } => (source, target),
        },
    );
    let source_tabs = source_tabs.expect("source tabs exist");
    let target_tabs = target_tabs.expect("target tabs exist");
    let source = dockspace
        .core_engine()
        .workspace()
        .capture_item_source(MAIN_ROOT, source_tabs, ITEM_A)
        .expect("source item must capture");
    let target = dockspace
        .core_engine()
        .workspace()
        .capture_tab_target(MAIN_ROOT, target_tabs)
        .expect("target tabs must capture");
    let a_ui_before = panes.ui_calls(ITEM_A);
    let b_ui_before = panes.ui_calls(ITEM_B);
    let source_rect = panes.last_ui_rect(ITEM_A).expect("source pane was painted");
    let target_rect = panes.last_ui_rect(ITEM_B).expect("target pane was painted");
    assert_ne!(source_rect, target_rect);
    dockspace
        .submit_command(dockspace::command::WorkspaceCommand::Move {
            payload: dockspace::command::MovePayload::Item(source),
            target: dockspace::command::DockTarget::Center(target),
        })
        .expect("move must commit immediately");
    assert_eq!(
        dockspace
            .core_engine()
            .workspace()
            .root(MAIN_ROOT)
            .map(|root| root.node),
        Some(target_tabs),
        "removing the source leaf collapses the split and relocates the target tabs"
    );
    assert_eq!(
        selected_in_group_containing(&dockspace, ITEM_A),
        Some(ITEM_A)
    );
    context.options_mut(|options| {
        options.max_passes = 1.try_into().expect("one is non-zero");
    });

    let stale = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(stale.len(), 1, "the one-pass budget must fail closed");
    assert_eq!(stale[0].surface_status, DockspaceSurfaceStatus::Stale);
    assert_eq!(panes.ui_calls(ITEM_A), a_ui_before + 1);
    assert_eq!(panes.ui_calls(ITEM_B), b_ui_before);
    assert_eq!(
        panes.last_ui_rect(ITEM_A),
        Some(target_rect),
        "the moved item must paint once through the stable target node, not its removed source slot"
    );

    for _ in 0..3 {
        run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        assert_eq!(panes.ui_calls(ITEM_B), b_ui_before);
    }
    assert!(panes.ui_calls(ITEM_A) > a_ui_before + 1);
}

#[test]
fn presentation_owner_change_does_not_run_pane_through_old_contained_chrome() {
    let context = Context::default();
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM_A]));
    builder.set_root(FLOATING_ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::rootless());
    let contained_rect =
        LogicalRect::new(80.0, 60.0, 260.0, 220.0).expect("contained fixture rect is valid");
    builder.set_contained_floating(
        FLOATING,
        ContainedFloating::new(FLOATING_ROOT, contained_rect),
    );
    builder
        .attach_contained(SURFACE, FLOATING)
        .expect("surface exists");
    let workspace = builder
        .build()
        .expect("rootless contained fixture is valid");
    let mut dockspace = Dockspace::builder("pane-owner-change", workspace)
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A]);
    warm(&context, &mut dockspace, &mut panes);

    let source = dockspace
        .core_engine()
        .workspace()
        .capture_node_source(FLOATING_ROOT, tabs)
        .expect("contained root source must capture");
    let ui_calls_before = panes.ui_calls(ITEM_A);
    let contained_content_rect = panes
        .last_ui_rect(ITEM_A)
        .expect("contained pane was painted");
    dockspace
        .submit_command(dockspace::command::WorkspaceCommand::PromoteContained {
            source,
            surface: SURFACE,
            floating: FLOATING,
        })
        .expect("contained root promotion must commit immediately");
    assert_eq!(
        dockspace
            .core_engine()
            .workspace()
            .surface(SURFACE)
            .and_then(|surface| surface.main_root),
        Some(FLOATING_ROOT)
    );
    context.options_mut(|options| {
        options.max_passes = 1.try_into().expect("one is non-zero");
    });

    let stale = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(stale.len(), 1, "the one-pass budget must fail closed");
    assert_eq!(stale[0].surface_status, DockspaceSurfaceStatus::Stale);
    assert_eq!(panes.ui_calls(ITEM_A), ui_calls_before);

    for _ in 0..3 {
        run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    }
    assert!(panes.ui_calls(ITEM_A) > ui_calls_before);
    assert_ne!(panes.last_ui_rect(ITEM_A), Some(contained_content_rect));
}

#[test]
fn stale_selection_uses_current_missing_pane_state() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder(
        "stale-selection-current-missing",
        single_workspace([ITEM_A, ITEM_B]),
    )
    .build()
    .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    warm(&context, &mut dockspace, &mut panes);

    let a_ui_before = panes.ui_calls(ITEM_A);
    let b_ui_before = panes.ui_calls(ITEM_B);
    panes.titles.remove(&ITEM_B);
    select_item(&mut dockspace, ITEM_B);
    context.options_mut(|options| {
        options.max_passes = 1.try_into().expect("one is non-zero");
    });

    let stale = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(stale.len(), 1, "the one-pass budget must fail closed");
    assert_eq!(stale[0].surface_status, DockspaceSurfaceStatus::Stale);
    assert_eq!(stale[0].missing, [ITEM_B]);
    assert_eq!(panes.ui_calls(ITEM_A), a_ui_before);
    assert_eq!(panes.ui_calls(ITEM_B), b_ui_before);
}

#[test]
fn stale_selection_uses_currently_recovered_pane_state() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder(
        "stale-selection-current-recovered",
        single_workspace([ITEM_A, ITEM_B]),
    )
    .build()
    .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A]);
    warm(&context, &mut dockspace, &mut panes);

    let a_ui_before = panes.ui_calls(ITEM_A);
    let b_ui_before = panes.ui_calls(ITEM_B);
    let b_disabled_before = panes.disabled_ui_calls(ITEM_B);
    panes.titles.insert(ITEM_B, "Recovered B".to_owned());
    select_item(&mut dockspace, ITEM_B);
    context.options_mut(|options| {
        options.max_passes = 1.try_into().expect("one is non-zero");
    });

    let stale = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(stale.len(), 1, "the one-pass budget must fail closed");
    assert_eq!(stale[0].surface_status, DockspaceSurfaceStatus::Stale);
    assert!(stale[0].missing.is_empty());
    assert_eq!(panes.ui_calls(ITEM_A), a_ui_before);
    assert_eq!(panes.ui_calls(ITEM_B), b_ui_before + 1);
    assert_eq!(panes.disabled_ui_calls(ITEM_B), b_disabled_before + 1);
}

#[test]
fn stale_selection_accessibility_uses_current_pane_identity() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder(
        "stale-selection-current-accessibility",
        single_workspace([ITEM_A, ITEM_B]),
    )
    .build()
    .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    let _ =
        run_authoritative_accesskit_frame(&context, &mut dockspace, &mut panes, vec2(600.0, 400.0));

    select_item(&mut dockspace, ITEM_B);
    context.options_mut(|options| {
        options.max_passes = 1.try_into().expect("one is non-zero");
    });
    let (tree, local_actions_current) = run_accesskit_frame_with_authority(
        &context,
        &mut dockspace,
        &mut panes,
        vec2(600.0, 400.0),
        Vec::new(),
    );

    assert!(!local_actions_current);
    let a_label = format!("Pane {}", ITEM_A.get());
    let b_label = format!("Pane {}", ITEM_B.get());
    let (_, tab_a) = accesskit_node_by_label(&tree, Role::Tab, &a_label);
    let (tab_b_id, tab_b) = accesskit_node_by_label(&tree, Role::Tab, &b_label);
    assert_ne!(tab_a.is_selected(), Some(true));
    assert!(tab_a.is_disabled());
    assert_eq!(tab_b.is_selected(), Some(true));
    assert!(tab_b.is_disabled());
    let (_, tab_list) = accesskit_node_by_role(&tree, Role::TabList);
    assert_eq!(tab_list.active_descendant(), Some(tab_b_id));
    let (_, panel_b) = accesskit_node_by_label(&tree, Role::TabPanel, &b_label);
    assert!(panel_b.is_disabled());
    assert!(
        !tree.nodes.iter().any(
            |(_, node)| node.role() == Role::TabPanel && node.label() == Some(a_label.as_str())
        )
    );
}

#[test]
fn missing_pane_is_reported_and_recovers_without_topology_changes() {
    let context = Context::default();
    let workspace = single_workspace([ITEM_A]);
    let mut dockspace = Dockspace::builder("missing-pane", workspace.clone())
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::default();

    let missing = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(missing.last().expect("frame paints").missing, [ITEM_A]);
    assert_eq!(panes.ui_calls(ITEM_A), 0);

    panes.titles.insert(ITEM_A, "Recovered".to_owned());
    let recovered = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(recovered.last().expect("frame paints").missing.is_empty());
    assert_eq!(panes.ui_calls(ITEM_A), 2);
    assert_eq!(panes.disabled_ui_calls(ITEM_A), 1);
    assert!(
        !recovered
            .last()
            .expect("the recovered pane candidate paints")
            .retained_presentation_current
    );
    let acknowledged = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        acknowledged
            .last()
            .expect("the next host sequence acknowledges the recovered pane")
            .retained_presentation_current
    );
    assert_eq!(panes.ui_calls(ITEM_A), 4);
    assert_eq!(panes.disabled_ui_calls(ITEM_A), 1);
    assert_eq!(dockspace.core_engine().workspace(), &workspace);
}

#[test]
fn disabled_close_policy_removes_the_egui_affordance() {
    let context = Context::default();
    let workspace = single_workspace([ITEM_A]);
    let mut policy = DockPolicy::default();
    let mut item_rule = DockItemRule::default();
    item_rule.set_close_capability(Some(CloseCapability::Disabled));
    policy.set_item_rule(ITEM_A, item_rule);
    let mut dockspace = Dockspace::builder("disabled-close", workspace.clone())
        .policy(policy)
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A]);
    warm(&context, &mut dockspace, &mut panes);

    let painted = dockspace
        .core_engine()
        .interaction_projection(SURFACE)
        .expect("disabled fixture must have an acknowledged paint");
    let tab = painted
        .plan()
        .tab_records()
        .iter()
        .find(|tab| tab.id().item == ITEM_A)
        .expect("fixture tab is painted");
    assert!(tab.close_bounds().is_none());

    assert_eq!(dockspace.core_engine().workspace(), &workspace);
    assert_eq!(
        dockspace.core_engine().version(),
        WorkspaceVersion::default()
    );
}

#[test]
fn hidden_tab_bar_gives_the_full_leaf_to_content_without_tab_accessibility() {
    let context = Context::default();
    context.enable_accesskit();
    let workspace = single_workspace([ITEM_A, ITEM_B]);
    let mut policy = DockPolicy::default();
    policy.set_tab_bar(TabBarPolicy::new(
        TabBarVisibility::Hidden,
        TabBarInteraction::Enabled,
    ));
    let mut dockspace = Dockspace::builder("hidden-tab-bar", workspace)
        .policy(policy)
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    warm(&context, &mut dockspace, &mut panes);
    let tree = run_accesskit_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec2(600.0, 400.0),
        Vec::new(),
    );

    let ready = dockspace
        .core_engine()
        .interaction_projection(SURFACE)
        .expect("hidden fixture has an acknowledged scene")
        .plan();
    let pane = ready
        .pane_records()
        .first()
        .expect("hidden bar keeps its pane record");
    assert_eq!(pane.content_bounds(), pane.bounds());
    assert!(ready.tab_bar_records().is_empty());
    assert!(ready.tab_records().is_empty());
    assert!(tree.nodes.iter().all(|(_, node)| {
        !matches!(node.role(), Role::Tab | Role::TabList)
            && node.label().is_none_or(|label| {
                label != "Drag tab group"
                    && label != "Show hidden tabs"
                    && !label.starts_with("Close Pane ")
            })
    }));
    accesskit_node_by_label(&tree, Role::TabPanel, &format!("Pane {}", ITEM_A.get()));
    assert!(panes.ui_calls(ITEM_A) > 0);
}

#[test]
fn disabled_tab_bar_paints_static_chrome_without_input_or_accessibility_actions() {
    let context = Context::default();
    context.enable_accesskit();
    let workspace = single_workspace([ITEM_A, ITEM_B]);
    let mut target_rule = DockTargetRule::default();
    target_rule.set_tab_bar(TabBarPolicy::new(
        TabBarVisibility::Visible,
        TabBarInteraction::Disabled,
    ));
    let mut policy = DockPolicy::default();
    policy.set_target_rule(DockTargetRuleKey::Item(ITEM_A), target_rule);
    let mut dockspace = Dockspace::builder("disabled-tab-bar", workspace.clone())
        .policy(policy)
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    warm(&context, &mut dockspace, &mut panes);
    let tree = run_accesskit_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec2(600.0, 400.0),
        Vec::new(),
    );

    let ready = dockspace
        .core_engine()
        .interaction_projection(SURFACE)
        .expect("paint-only fixture has an acknowledged scene")
        .plan();
    let second = ready
        .tab_records()
        .iter()
        .find(|tab| tab.id().item == ITEM_B)
        .expect("paint-only chrome retains the second visual tab");
    let visible = second.visible_bounds();
    let tab_center = Pos2::new(
        (visible.x() + visible.width() * 0.5) as f32,
        (visible.y() + visible.height() * 0.5) as f32,
    );
    assert!(second.close_visual_bounds().is_some());
    assert!(second.close_bounds().is_none());
    assert!(second.drag_hit().rect().width() <= 0.0 || second.drag_hit().rect().height() <= 0.0);
    assert!(tree.nodes.iter().all(|(_, node)| {
        !matches!(node.role(), Role::Tab | Role::TabList)
            && node.label().is_none_or(|label| {
                label != "Drag tab group"
                    && label != "Show hidden tabs"
                    && !label.starts_with("Close Pane ")
            })
    }));
    accesskit_node_by_label(&tree, Role::TabPanel, &format!("Pane {}", ITEM_A.get()));

    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(tab_center),
            pointer_button(tab_center, true),
        ],
    );
    let released = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(tab_center),
            pointer_button(tab_center, false),
        ],
    );
    assert!(
        released
            .iter()
            .all(|observation| observation.close_requests.is_empty())
    );
    assert_eq!(dockspace.core_engine().workspace(), &workspace);
}

#[test]
fn contained_close_accepts_keyboard_and_accesskit_activation_without_pointer_geometry() {
    for (salt, activation) in [
        (
            "contained-close-enter",
            Event::Key {
                key: Key::Enter,
                physical_key: Some(Key::Enter),
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            },
        ),
        (
            "contained-close-accesskit",
            Event::AccessKitActionRequest(ActionRequest {
                action: Action::Click,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: Id::NULL.accesskit_id(),
                data: None,
            }),
        ),
    ] {
        let context = Context::default();
        let workspace = contained_workspace(
            LogicalRect::new(100.0, 80.0, 260.0, 190.0).expect("contained rect is valid"),
        );
        let mut dockspace = Dockspace::builder(salt, workspace.clone())
            .build()
            .expect("facade must build");
        let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
        warm(&context, &mut dockspace, &mut panes);
        let close_id = contained_close_id(&context, &mut dockspace, &mut panes, salt);
        let event = match activation {
            Event::AccessKitActionRequest(mut request) => {
                request.target_node = close_id.accesskit_id();
                Event::AccessKitActionRequest(request)
            }
            event => event,
        };

        if matches!(&event, Event::Key { .. }) {
            context.memory_mut(|memory| memory.request_focus(close_id));
            run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        }
        let requested = run_frame(&context, &mut dockspace, &mut panes, vec![event]);
        let plan = only_close_request(&requested);
        assert_eq!(
            plan.target(),
            ClosePlanTarget::Root {
                root: FLOATING_ROOT,
            }
        );
        assert_eq!(
            plan.items()
                .iter()
                .map(|item| item.item())
                .collect::<Vec<_>>(),
            [ITEM_B]
        );
        assert_eq!(dockspace.core_engine().workspace(), &workspace);

        resolve_close_plan(&mut dockspace, &plan, |_| CloseDecision::Allow);
        assert!(
            dockspace
                .core_engine()
                .workspace()
                .contained_floating(FLOATING)
                .is_none()
        );
        assert!(
            dockspace
                .core_engine()
                .workspace()
                .root(FLOATING_ROOT)
                .is_none()
        );
        run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        assert!(
            dockspace
                .core_engine()
                .workspace()
                .contained_floating(FLOATING)
                .is_none()
        );
        assert!(
            dockspace
                .core_engine()
                .workspace()
                .root(FLOATING_ROOT)
                .is_none()
        );

        run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        assert!(
            dockspace
                .core_engine()
                .workspace()
                .contained_floating(FLOATING)
                .is_none()
        );
        assert!(
            dockspace
                .core_engine()
                .workspace()
                .root(FLOATING_ROOT)
                .is_none()
        );
    }
}

#[test]
fn contained_resize_edges_expose_truthful_one_axis_accessibility_and_keyboard_equivalence() {
    fn adjusted_right_edge(salt: &'static str, keyboard: bool) -> LogicalRect {
        let context = Context::default();
        context.enable_accesskit();
        let original =
            LogicalRect::new(100.0, 80.0, 240.0, 180.0).expect("contained rect is valid");
        let mut dockspace = Dockspace::builder(salt, contained_workspace(original))
            .build()
            .expect("facade must build");
        let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
        let tree = run_authoritative_accesskit_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec2(600.0, 400.0),
        );
        let (right, _) = accesskit_node_by_label(&tree, Role::Splitter, "Resize right edge");

        if keyboard {
            run_accesskit_frame(
                &context,
                &mut dockspace,
                &mut panes,
                vec2(600.0, 400.0),
                vec![accesskit_action(right, Action::Focus)],
            );
            assert_eq!(
                context
                    .memory(egui::Memory::focused)
                    .map(|id| id.accesskit_id()),
                Some(right),
                "the edge splitter must accept semantic focus"
            );
            run_accesskit_frame(
                &context,
                &mut dockspace,
                &mut panes,
                vec2(600.0, 400.0),
                key_press(Key::ArrowRight),
            );
        } else {
            run_accesskit_frame(
                &context,
                &mut dockspace,
                &mut panes,
                vec2(600.0, 400.0),
                vec![accesskit_action(right, Action::Increment)],
            );
        }
        contained_rect(&dockspace)
    }

    let context = Context::default();
    context.enable_accesskit();
    let original = LogicalRect::new(100.0, 80.0, 240.0, 180.0).expect("contained rect is valid");
    let mut dockspace = Dockspace::builder(
        "contained-resize-accessibility-tree",
        contained_workspace(original),
    )
    .build()
    .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    let tree =
        run_authoritative_accesskit_frame(&context, &mut dockspace, &mut panes, vec2(600.0, 400.0));
    let expected = [
        (
            "Resize top edge",
            Orientation::Horizontal,
            original.min().y(),
        ),
        (
            "Resize right edge",
            Orientation::Vertical,
            original.max().x(),
        ),
        (
            "Resize bottom edge",
            Orientation::Horizontal,
            original.max().y(),
        ),
        (
            "Resize left edge",
            Orientation::Vertical,
            original.min().x(),
        ),
    ];
    for (label, orientation, value) in expected {
        let (_, node) = accesskit_node_by_label(&tree, Role::Splitter, label);
        assert_eq!(node.orientation(), Some(orientation));
        assert_eq!(node.numeric_value(), Some(value));
        assert!(node.supports_action(Action::Focus));
        assert!(node.supports_action(Action::Increment));
        assert!(node.supports_action(Action::Decrement));
    }
    assert_eq!(
        tree.nodes
            .iter()
            .filter(|(_, node)| node.role() == Role::Splitter)
            .count(),
        4,
        "the four diagonal pointer grips must not masquerade as one-axis splitters"
    );

    let keyboard = adjusted_right_edge("contained-resize-keyboard", true);
    let accesskit = adjusted_right_edge("contained-resize-accesskit", false);
    assert_eq!(keyboard, accesskit);
    assert_eq!(keyboard.min(), original.min());
    assert_eq!(keyboard.max().y(), original.max().y());
    assert!(keyboard.max().x() > original.max().x());
}

#[test]
fn contained_edge_adjustment_clamps_at_minimum_without_moving_the_opposite_anchor() {
    let context = Context::default();
    context.enable_accesskit();
    let original = LogicalRect::new(100.0, 80.0, 125.0, 180.0).expect("contained rect is valid");
    let mut dockspace = Dockspace::builder(
        "contained-resize-minimum-clamp",
        contained_workspace(original),
    )
    .build()
    .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    let tree =
        run_authoritative_accesskit_frame(&context, &mut dockspace, &mut panes, vec2(600.0, 400.0));
    let (left, _) = accesskit_node_by_label(&tree, Role::Splitter, "Resize left edge");

    run_accesskit_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec2(600.0, 400.0),
        vec![accesskit_action(left, Action::Increment)],
    );
    let resized = contained_rect(&dockspace);

    assert_eq!(resized.max(), original.max());
    assert_eq!(
        resized.width(),
        f64::from(dockspace.style().minimum_floating_size.x)
    );
}

#[test]
fn contained_edge_adjustment_obeys_policy_and_stale_projection_authority() {
    let original = LogicalRect::new(100.0, 80.0, 240.0, 180.0).expect("contained rect is valid");

    let policy_context = Context::default();
    policy_context.enable_accesskit();
    let mut policy = DockPolicy::default();
    policy.set_allow_contained_transform(false);
    let mut policy_dockspace = Dockspace::builder(
        "contained-resize-policy-disabled",
        contained_workspace(original),
    )
    .policy(policy)
    .build()
    .expect("facade must build");
    let mut policy_panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    let policy_tree = run_authoritative_accesskit_frame(
        &policy_context,
        &mut policy_dockspace,
        &mut policy_panes,
        vec2(600.0, 400.0),
    );
    let (policy_right, _) =
        accesskit_node_by_label(&policy_tree, Role::Splitter, "Resize right edge");
    let policy_version = policy_dockspace.core_engine().version();
    run_accesskit_frame(
        &policy_context,
        &mut policy_dockspace,
        &mut policy_panes,
        vec2(600.0, 400.0),
        vec![accesskit_action(policy_right, Action::Increment)],
    );
    assert_eq!(contained_rect(&policy_dockspace), original);
    assert_eq!(policy_dockspace.core_engine().version(), policy_version);

    let stale_context = Context::default();
    stale_context.enable_accesskit();
    let mut stale_dockspace = Dockspace::builder(
        "contained-resize-stale-projection",
        contained_workspace(original),
    )
    .build()
    .expect("facade must build");
    let mut stale_panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    let current_tree = run_authoritative_accesskit_frame(
        &stale_context,
        &mut stale_dockspace,
        &mut stale_panes,
        vec2(600.0, 400.0),
    );
    let (stale_right, _) =
        accesskit_node_by_label(&current_tree, Role::Splitter, "Resize right edge");
    let replacement =
        LogicalRect::new(130.0, 100.0, 210.0, 160.0).expect("replacement rect is valid");
    stale_dockspace
        .replace_workspace(contained_workspace(replacement))
        .expect("replacement must commit before the stale callback");
    stale_context.options_mut(|options| {
        options.max_passes = 1.try_into().expect("one is non-zero");
    });
    let before_stale_action = stale_dockspace.core_engine().version();
    let (stale_tree, local_actions_current) = run_accesskit_frame_with_authority(
        &stale_context,
        &mut stale_dockspace,
        &mut stale_panes,
        vec2(600.0, 400.0),
        vec![accesskit_action(stale_right, Action::Increment)],
    );

    assert!(!local_actions_current);
    assert_eq!(contained_rect(&stale_dockspace), replacement);
    assert_eq!(stale_dockspace.core_engine().version(), before_stale_action);
    let stale_node = stale_tree
        .nodes
        .iter()
        .find_map(|(id, node)| (*id == stale_right).then_some(node));
    assert!(stale_node.is_none_or(|node| {
        node.role() != Role::Splitter
            && !node.supports_action(Action::Increment)
            && !node.supports_action(Action::Decrement)
    }));
}

#[test]
fn tab_close_and_splitter_max_edges_are_not_interaction_owned() {
    let context = Context::default();
    let original = single_workspace([ITEM_A]);
    let mut dockspace = Dockspace::builder("tab-close-max-edge", original.clone())
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A]);
    warm(&context, &mut dockspace, &mut panes);
    let close = tab_close_rect(&dockspace);
    let close_max_edge = Pos2::new(close.max.x, close.center().y);

    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(close_max_edge),
            pointer_button(close_max_edge, true),
        ],
    );
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(close_max_edge),
            pointer_button(close_max_edge, false),
        ],
    );
    run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(dockspace.core_engine().workspace(), &original);

    let context = Context::default();
    let original = split_workspace();
    let mut dockspace = Dockspace::builder("splitter-max-edge", original.clone())
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    warm(&context, &mut dockspace, &mut panes);
    let splitter = splitter_hit_rect(&dockspace);
    let press = Pos2::new(splitter.max.x, splitter.center().y);
    let current = press + vec2(24.0, 0.0);

    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(press), pointer_button(press, true)],
    );
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(current)],
    );
    let released = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(current), pointer_button(current, false)],
    );
    assert_eq!(
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Idle
    );
    assert!(
        released
            .iter()
            .all(|observation| observation.workspace == original)
    );
}

#[test]
fn multipass_observes_a_command_submitted_between_passes() {
    let context = Context::default();
    let workspace = single_workspace([ITEM_A, ITEM_B]);
    let mut dockspace = Dockspace::builder("multipass-boundary", workspace.clone())
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    warm(&context, &mut dockspace, &mut panes);
    let source = dockspace
        .core_engine()
        .workspace()
        .roots()
        .next()
        .and_then(|(root, record)| {
            dockspace
                .core_engine()
                .workspace()
                .capture_item_source(root, record.node, ITEM_B)
                .ok()
        })
        .expect("item source must capture");
    let mut command = Some(dockspace::command::WorkspaceCommand::Select { source });
    let mut versions = Vec::new();
    let mut workspaces = Vec::new();
    let mut submitted_workspace = None;

    let _ = crate::test_support::run_ui(&context, input(Vec::new()), |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("multipass show must succeed");
        versions.push(dockspace.core_engine().version());
        workspaces.push(dockspace.core_engine().workspace().clone());
        if ui.ctx().current_pass_index() == 0 {
            dockspace
                .submit_command(command.take().expect("submitted only in pass zero"))
                .expect("command must commit before the next pass");
            submitted_workspace = Some(dockspace.core_engine().workspace().clone());
            ui.ctx()
                .request_discard("exercise dockspace multipass gate");
        }
    });

    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0], WorkspaceVersion::default());
    assert_eq!(workspaces[0], workspace);
    let submitted_workspace = submitted_workspace.expect("pass zero must submit one command");
    assert_ne!(versions[1], WorkspaceVersion::default());
    assert_eq!(workspaces[1], submitted_workspace);
    let selected = workspaces[1]
        .roots()
        .next()
        .and_then(|(_, root)| workspaces[1].node(root.node))
        .and_then(|node| match node {
            Node::Tabs { selected, .. } => *selected,
            Node::Split { .. } => None,
        });
    assert_eq!(selected, Some(ITEM_B));
    assert_eq!(dockspace.core_engine().workspace(), &submitted_workspace);
}

#[test]
fn host_smaller_than_splitter_thickness_publishes_a_ready_degraded_scene() {
    let context = Context::default();
    let original = split_workspace();
    let mut dockspace = Dockspace::builder("collapsed-host", original.clone())
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    let tiny = Rect::from_min_size(Pos2::new(20.0, 20.0), vec2(2.0, 160.0));

    let mut statuses = Vec::new();
    let _ = crate::test_support::run_ui(&context, input(Vec::new()), |ui| {
        let mut child = ui.new_child(UiBuilder::new().max_rect(tiny));
        let response = dockspace
            .show_single_surface(SURFACE, &mut child, &mut panes)
            .expect("collapsed host remains projectable");
        statuses.push((
            response.surface_status(),
            response.interaction_capabilities().local_actions_current(),
        ));
    });
    assert_eq!(
        statuses,
        vec![
            (DockspaceSurfaceStatus::Bootstrap, false),
            (DockspaceSurfaceStatus::Ready, false)
        ]
    );

    let mut current = None;
    let _ = crate::test_support::run_ui(&context, input(Vec::new()), |ui| {
        let mut child = ui.new_child(UiBuilder::new().max_rect(tiny));
        let response = dockspace
            .show_single_surface(SURFACE, &mut child, &mut panes)
            .expect("collapsed host remains projectable");
        current = Some((
            response.surface_status(),
            response.interaction_capabilities(),
        ));
    });
    let (status, capabilities) = current.expect("the ready pass must return capabilities");
    assert_eq!(status, DockspaceSurfaceStatus::Ready);
    assert!(!capabilities.local_actions_current());
    assert!(capabilities.retained_presentation_current());
    assert!(capabilities.pointer_receivers_current());

    let degraded_plan = dockspace
        .core_engine()
        .scene()
        .surface(SURFACE)
        .and_then(SurfaceScene::paint_projection)
        .expect("ready degraded surface remains paintable")
        .plan();
    assert_eq!(degraded_plan.splitter_gap_records().len(), 1);
    assert_eq!(
        degraded_plan.splitter_gap_records()[0].presentation(),
        SplitterGapPresentation::Collapsed
    );
    assert!(
        degraded_plan.splitter_records().is_empty(),
        "a collapsed gap has no paint or interaction record"
    );

    assert_eq!(dockspace.core_engine().workspace(), &original);
    assert_eq!(
        dockspace.core_engine().workspace().item_multiset(),
        BTreeMap::from([(ITEM_A, 1), (ITEM_B, 1)])
    );
    assert!(matches!(
        dockspace.core_engine().scene().surface(SURFACE),
        Some(SurfaceScene::Ready(_))
    ));
}

#[test]
fn contained_measurements_clip_presentation_without_rewriting_durable_bounds() {
    let context = Context::default();
    let original = LogicalRect::new(550.0, 330.0, 200.0, 160.0).expect("finite rect");
    let workspace = contained_workspace(original);
    let mut dockspace = Dockspace::builder("contained-clamp", workspace)
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    panes.minimum_sizes.insert(ITEM_B, vec2(420.0, 280.0));
    let before_version = dockspace.core_engine().version();

    run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let floating = dockspace
        .core_engine()
        .workspace()
        .contained_floating(FLOATING)
        .expect("floating remains presented");

    assert_eq!(floating.rect, original);
    assert_eq!(dockspace.core_engine().version(), before_version);
    assert_eq!(
        dockspace.core_engine().workspace().item_multiset(),
        BTreeMap::from([(ITEM_A, 1), (ITEM_B, 1)])
    );
    let ready = dockspace
        .core_engine()
        .scene()
        .surface(SURFACE)
        .and_then(SurfaceScene::ready)
        .expect("the measured surface must publish a ready plan");
    let occlusion = ready
        .plan()
        .drop_occlusions()
        .iter()
        .find(|occlusion| occlusion.floating() == FLOATING)
        .expect("the durable contained root owns one raw occlusion");
    assert_eq!(occlusion.region().rect(), original);
    let presented = ready
        .plan()
        .contained_records()
        .iter()
        .find(|record| record.floating() == FLOATING)
        .expect("the visible corner remains presentable");
    let surface_bounds = ready.plan().bounds();
    assert!(presented.outer_bounds().min().x() >= surface_bounds.min().x());
    assert!(presented.outer_bounds().min().y() >= surface_bounds.min().y());
    assert!(presented.outer_bounds().max().x() <= surface_bounds.max().x());
    assert!(presented.outer_bounds().max().y() <= surface_bounds.max().y());
    assert_ne!(presented.outer_bounds(), original);
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "the linear frame sequence proves that release cannot replay a stale painted preview"
)]
fn contained_move_waits_for_a_release_beyond_the_last_painted_pointer_preview() {
    let context = Context::default();
    let original = LogicalRect::new(140.0, 90.0, 220.0, 160.0).expect("finite rect");
    let mut dockspace = Dockspace::builder("contained-move", contained_workspace(original))
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    warm_outer_with_size(&context, &mut dockspace, &mut panes, vec2(600.0, 400.0));

    let original_floating = *dockspace
        .core_engine()
        .workspace()
        .contained_floating(FLOATING)
        .expect("floating exists");
    let press_origin = contained_title_point(&dockspace);
    let painted_pointer = press_origin + vec2(48.0, 36.0);
    let release_pointer = press_origin + vec2(72.0, 54.0);

    run_outer_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        vec2(600.0, 400.0),
        vec![
            Event::PointerMoved(press_origin),
            pointer_button(press_origin, true),
        ],
    );
    assert_eq!(contained_rect(&dockspace), original);

    run_outer_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        vec2(600.0, 400.0),
        vec![Event::PointerMoved(painted_pointer)],
    );
    assert!(matches!(
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    assert_eq!(contained_rect(&dockspace), original);

    run_outer_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        vec2(600.0, 400.0),
        vec![Event::PointerMoved(painted_pointer)],
    );
    let expected = translated_rect(original, press_origin, painted_pointer);
    assert!(matches!(
        dockspace
            .core_engine()
            .interaction()
            .preview()
            .expect("last painted move candidate remains authoritative")
            .visual(),
        PreviewVisual::Contained {
            surface: SURFACE,
            rect,
            fallback: false,
        } if *rect == expected
    ));

    let sequence = dockspace
        .last_egui_frame_schedule_key()
        .expect("the warmed facade has a schedule key")
        .sequence()
        .checked_add(1)
        .expect("the test schedule remains representable");
    let mut frame = dockspace
        .begin_outer_frame(EguiFrameScheduleKey::new(sequence, 0))
        .expect("release frame begins");
    frame
        .run_surface(
            SURFACE,
            &context,
            input_with_size(
                vec![
                    Event::PointerMoved(release_pointer),
                    pointer_button(release_pointer, false),
                ],
                vec2(600.0, 400.0),
            ),
            &mut panes,
        )
        .expect("release frame paints");
    let (host, outputs) = frame
        .finish()
        .expect("release frame commits atomically")
        .into_parts();
    let _ = host;
    let pending = dockspace
        .core_engine()
        .pending_release_preview()
        .expect("release retains the newly sampled preview");
    let release_output = outputs
        .iter()
        .find(|output| output.surface() == SURFACE)
        .expect("release paint produces the source surface output");
    assert_ne!(
        release_output
            .presentation_output()
            .and_then(|output| output.payload().interaction())
            .and_then(|interaction| interaction.drag_preview()),
        Some(pending.1),
        "the pre-input paint must not claim the preview sampled later in the frame"
    );
    let expected_release_rect = egui_rect(translated_rect(original, press_origin, release_pointer));
    assert!(
        !release_output
            .full_output()
            .shapes
            .iter()
            .any(|clipped| matches!(
                &clipped.shape,
                egui::Shape::Rect(shape)
                    if shape.rect == expected_release_rect
                        && shape.fill == dockspace.style().drop_fill
            )),
        "the release position was sampled after this paint and must not be claimed as drawn"
    );
    outputs.submit_with(|_, _, _| EguiRendererOutputDisposition::Accepted);
    let floating = dockspace
        .core_engine()
        .workspace()
        .contained_floating(FLOATING)
        .expect("floating remains");
    assert_eq!(floating.rect, original);
    assert_eq!(
        dockspace
            .core_engine()
            .workspace()
            .surface(SURFACE)
            .expect("surface remains present")
            .contained,
        vec![FLOATING]
    );
    assert_eq!(floating.root, original_floating.root);
    assert_eq!(
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Idle
    );

    let sequence = dockspace
        .last_egui_frame_schedule_key()
        .expect("release frame established a schedule key")
        .sequence()
        .checked_add(1)
        .expect("preview frame sequence remains representable");
    let mut frame = dockspace
        .begin_outer_frame(EguiFrameScheduleKey::new(sequence, 0))
        .expect("preview frame begins");
    frame
        .run_surface(
            SURFACE,
            &context,
            input_with_size(Vec::new(), vec2(600.0, 400.0)),
            &mut panes,
        )
        .expect("preview frame paints");
    let (_, outputs) = frame
        .finish()
        .expect("preview frame commits atomically")
        .into_parts();
    let preview_output = outputs
        .iter()
        .find(|output| output.surface() == SURFACE)
        .expect("preview frame produces the source surface output");
    assert_eq!(
        preview_output
            .presentation_output()
            .and_then(|output| output.payload().interaction())
            .and_then(|interaction| interaction.drag_preview()),
        Some(pending.1),
        "the next output must name the exact pending preview it painted"
    );
    assert_eq!(
        preview_output
            .presentation_output()
            .expect("pending release preview must carry an exact core output")
            .continuation(),
        dockspace::backend::presentation_observation::HostPresentationContinuation::Terminal,
        "a dropped renderer result must still settle the pending release",
    );
    assert!(
        preview_output
            .full_output()
            .shapes
            .iter()
            .any(|clipped| matches!(
                &clipped.shape,
                egui::Shape::Rect(shape)
                    if shape.rect == expected_release_rect
                        && shape.fill == dockspace.style().drop_fill
            )),
        "the output carrying the pending token must contain its exact preview rectangle"
    );
    outputs.submit_with(|_, _, _| EguiRendererOutputDisposition::Accepted);
    assert!(dockspace.core_engine().pending_release_preview().is_some());
    run_outer_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        vec2(600.0, 400.0),
        Vec::new(),
    );
    assert_eq!(dockspace.core_engine().pending_release_preview(), None);
    assert_eq!(
        contained_rect(&dockspace),
        translated_rect(original, press_origin, release_pointer)
    );
}

#[test]
fn contained_north_west_resize_preserves_opposite_anchor_and_clamps_constraints() {
    let context = Context::default();
    let original = LogicalRect::new(120.0, 80.0, 240.0, 180.0).expect("finite rect");
    let mut dockspace = Dockspace::builder("contained-resize", contained_workspace(original))
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    warm_outer_with_size(&context, &mut dockspace, &mut panes, vec2(600.0, 400.0));

    let press_origin = contained_north_west_resize_point(&dockspace);
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the finite fixture edge is deliberately converted to egui's f32 input space"
    )]
    let current = Pos2::new(original.max().x() as f32, -40.0);

    run_outer_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        vec2(600.0, 400.0),
        vec![
            Event::PointerMoved(press_origin),
            pointer_button(press_origin, true),
        ],
    );
    run_outer_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        vec2(600.0, 400.0),
        vec![Event::PointerMoved(current)],
    );
    run_outer_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        vec2(600.0, 400.0),
        vec![Event::PointerMoved(current)],
    );
    run_outer_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        vec2(600.0, 400.0),
        vec![Event::PointerMoved(current), pointer_button(current, false)],
    );
    let resized = dockspace
        .core_engine()
        .workspace()
        .contained_floating(FLOATING)
        .expect("floating remains")
        .rect;
    let minimum = dockspace.style().minimum_floating_size;

    assert_eq!(resized.max(), original.max(), "opposite corner is frozen");
    assert!(
        resized.min().y().abs() <= f64::EPSILON,
        "north edge is clamped to bounds"
    );
    assert!((resized.width() - f64::from(minimum.x)).abs() <= f64::EPSILON);
    assert!(resized.height() >= f64::from(minimum.y));
    assert_eq!(
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Idle
    );
}

#[test]
fn stale_projection_release_uses_current_unknown_target_and_cancels_drag() {
    let context = Context::default();
    let original = LogicalRect::new(120.0, 80.0, 220.0, 150.0).expect("finite rect");
    let mut dockspace =
        Dockspace::builder("stale-contained-release", contained_workspace(original))
            .build()
            .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    warm_outer_with_size(&context, &mut dockspace, &mut panes, vec2(600.0, 400.0));

    let press_origin = contained_title_point(&dockspace);
    let current = press_origin + vec2(30.0, 20.0);
    run_outer_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        vec2(600.0, 400.0),
        vec![
            Event::PointerMoved(press_origin),
            pointer_button(press_origin, true),
        ],
    );
    run_outer_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        vec2(600.0, 400.0),
        vec![Event::PointerMoved(current)],
    );
    run_outer_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        vec2(600.0, 400.0),
        vec![Event::PointerMoved(current)],
    );
    assert!(matches!(
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    let before_release = dockspace.core_engine().workspace().clone();

    let released = run_outer_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        vec2(560.0, 360.0),
        vec![Event::PointerMoved(current), pointer_button(current, false)],
    );
    assert!(!released.pointer_receivers_current);
    assert_eq!(
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Idle
    );
    assert_eq!(dockspace.core_engine().workspace(), &before_release);
    let projection = dockspace
        .core_engine()
        .scene()
        .surface(SURFACE)
        .and_then(SurfaceScene::paint_projection)
        .expect("the resized surface projection is painted");
    let interaction = dockspace.core_engine().interaction_projection(SURFACE);
    assert!(
        interaction.is_none_or(|interaction| {
            interaction.output_ticket() != projection.output_ticket()
        }),
        "the release callback cannot acknowledge its replacement candidate"
    );
    warm_outer_with_size(&context, &mut dockspace, &mut panes, vec2(560.0, 360.0));
    assert!(paint_projection_is_authoritative(&dockspace));
}

#[test]
fn stale_projection_still_releases_active_split_resize() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("stale-resize-release", split_workspace())
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    warm_outer_with_size(&context, &mut dockspace, &mut panes, vec2(600.0, 400.0));

    let press_origin = splitter_center(&dockspace);
    let current = press_origin + vec2(60.0, 0.0);
    run_outer_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        vec2(600.0, 400.0),
        vec![
            Event::PointerMoved(press_origin),
            pointer_button(press_origin, true),
        ],
    );
    run_outer_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        vec2(600.0, 400.0),
        vec![Event::PointerMoved(current)],
    );
    assert!(matches!(
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Resizing { .. }
    ));
    let version_before_release = dockspace.core_engine().version();

    let released = run_outer_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        vec2(600.0, 400.0),
        vec![Event::PointerMoved(current), pointer_button(current, false)],
    );
    assert!(!released.pointer_receivers_current);
    assert_ne!(dockspace.core_engine().version(), version_before_release);
    assert_eq!(
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Idle
    );
    assert!(
        dockspace
            .core_engine()
            .scene()
            .surface(SURFACE)
            .and_then(SurfaceScene::paint_projection)
            .is_some(),
        "the resized split candidate is published"
    );
    assert!(
        dockspace
            .core_engine()
            .interaction_projection(SURFACE)
            .is_none(),
        "the resize release cannot acknowledge its replacement candidate"
    );

    let settled = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        !settled
            .last()
            .expect("the next host sequence paints the replacement candidate")
            .retained_presentation_current
    );
    let acknowledged = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        acknowledged
            .last()
            .expect("the later host sequence acknowledges the replacement candidate")
            .retained_presentation_current
    );
    assert!(paint_projection_is_authoritative(&dockspace));
}

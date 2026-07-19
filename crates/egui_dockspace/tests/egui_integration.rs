use std::collections::BTreeMap;

use dockspace::drop_target::DropTargetId;
use dockspace::geometry::LogicalRect;
use dockspace::graph::{Axis, ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use dockspace::interaction::{InteractionStatus, PreviewVisual};
use dockspace::scene::SurfaceScene;
use dockspace::transition::WorkspaceVersion;
use egui::accesskit::{Action, ActionRequest, NodeId as AccessKitNodeId, Role, TreeUpdate};
use egui::{
    Context, Event, Frame, Id, Key, Modifiers, MouseWheelUnit, PointerButton, Pos2, RawInput, Rect,
    Sense, TouchPhase, Ui, UiBuilder, vec2,
};
use egui_dockspace::{Dockspace, DockspaceSurfaceStatus, PaneCloseResponse, PaneView};

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
    close_response: BTreeMap<ItemId, PaneCloseResponse>,
    minimum_sizes: BTreeMap<ItemId, egui::Vec2>,
    ui_calls: BTreeMap<ItemId, usize>,
    disabled_ui_calls: BTreeMap<ItemId, usize>,
    pane_clicks: BTreeMap<ItemId, usize>,
    close_calls: BTreeMap<ItemId, usize>,
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

    fn close_calls(&self, item: ItemId) -> usize {
        self.close_calls.get(&item).copied().unwrap_or_default()
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
}

impl PaneView for TestPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        self.titles.get(&item).cloned().map(Into::into)
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) {
        *self.ui_calls.entry(item).or_default() += 1;
        if !ui.is_enabled() {
            *self.disabled_ui_calls.entry(item).or_default() += 1;
        }
        if ui.allocate_rect(ui.max_rect(), Sense::click()).clicked() {
            *self.pane_clicks.entry(item).or_default() += 1;
        }
    }

    fn close(&mut self, item: ItemId) -> PaneCloseResponse {
        *self.close_calls.entry(item).or_default() += 1;
        self.close_response
            .get(&item)
            .copied()
            .unwrap_or(PaneCloseResponse::Allow)
    }

    fn minimum_size(&self, item: ItemId) -> egui::Vec2 {
        self.minimum_sizes.get(&item).copied().unwrap_or_default()
    }
}

#[derive(Debug)]
struct Observation {
    pass: usize,
    interactions_current: bool,
    missing: Vec<ItemId>,
    version: WorkspaceVersion,
    workspace: Workspace,
}

fn single_workspace(items: impl IntoIterator<Item = ItemId>) -> Workspace {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs(items));
    builder.set_root(MAIN_ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::new(MAIN_ROOT));
    builder.build().expect("single-surface fixture must build")
}

fn contained_workspace(rect: LogicalRect) -> Workspace {
    let mut builder = Workspace::builder();
    let main = builder.insert_node(Node::tabs([ITEM_A]));
    let floating = builder.insert_node(Node::tabs([ITEM_B]));
    builder.set_root(MAIN_ROOT, RootRecord::new(main));
    builder.set_root(FLOATING_ROOT, RootRecord::new(floating));
    builder.set_surface(SURFACE, SurfacePresentation::new(MAIN_ROOT));
    builder.set_contained_floating(ContainedFloating::new(
        FLOATING,
        FLOATING_ROOT,
        SURFACE,
        rect,
        1,
    ));
    builder
        .attach_contained(SURFACE, FLOATING)
        .expect("surface exists");
    builder.build().expect("contained fixture must build")
}

fn contained_group_workspace(rect: LogicalRect) -> Workspace {
    let mut builder = Workspace::builder();
    let main = builder.insert_node(Node::tabs([ITEM_A]));
    let floating = builder.insert_node(Node::tabs([ITEM_B, ITEM_C]));
    builder.set_root(MAIN_ROOT, RootRecord::new(main));
    builder.set_root(FLOATING_ROOT, RootRecord::new(floating));
    builder.set_surface(SURFACE, SurfacePresentation::new(MAIN_ROOT));
    builder.set_contained_floating(ContainedFloating::new(
        FLOATING,
        FLOATING_ROOT,
        SURFACE,
        rect,
        1,
    ));
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
    builder.set_surface(SURFACE, SurfacePresentation::new(MAIN_ROOT));
    builder.set_contained_floating(ContainedFloating::new(
        FLOATING,
        FLOATING_ROOT,
        SURFACE,
        rect,
        1,
    ));
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
        builder.set_surface(SURFACE, SurfacePresentation::new(MAIN_ROOT));
        let rect =
            LogicalRect::new(20.0, 50.0, 200.0, 150.0).expect("floating underlay rect is valid");
        builder.set_contained_floating(ContainedFloating::new(
            FLOATING,
            FLOATING_ROOT,
            SURFACE,
            rect,
            1,
        ));
        builder
            .attach_contained(SURFACE, FLOATING)
            .expect("surface exists");
    } else {
        let underlay = builder.insert_node(Node::tabs(underlay_items.iter().copied()));
        let split = builder.insert_node(
            Node::split(Axis::Vertical, [popup, underlay], [0.35, 0.65])
                .expect("vertical popup overlap fixture is valid"),
        );
        builder.set_root(MAIN_ROOT, RootRecord::new(split));
        builder.set_surface(SURFACE, SurfacePresentation::new(MAIN_ROOT));
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
    builder.set_surface(SURFACE, SurfacePresentation::new(MAIN_ROOT));
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
        events,
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
    let _ = context.run_ui(input_with_size(events, size), |ui| {
        let response = dockspace
            .show(SURFACE, ui, panes)
            .expect("egui frame must advance");
        observations.push(Observation {
            pass: ui.ctx().current_pass_index(),
            interactions_current: response.interactions_current(),
            missing: response.missing_panes().to_vec(),
            version: dockspace.engine().version(),
            workspace: dockspace.engine().workspace().clone(),
        });
    });
    observations
}

fn run_accesskit_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    size: egui::Vec2,
    events: Vec<Event>,
) -> TreeUpdate {
    let output = context.run_ui(input_with_size(events, size), |ui| {
        dockspace
            .show(SURFACE, ui, panes)
            .expect("AccessKit frame must advance");
    });
    output
        .platform_output
        .accesskit_update
        .expect("AccessKit output is enabled")
}

fn run_accesskit_frame_in_rect(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    screen_rect: Rect,
    dock_rect: Rect,
    events: Vec<Event>,
) -> TreeUpdate {
    let output = context.run_ui(input_with_rect(events, screen_rect), |ui| {
        let mut child = ui.new_child(UiBuilder::new().max_rect(dock_rect));
        dockspace
            .show(SURFACE, &mut child, panes)
            .expect("positioned AccessKit frame must advance");
    });
    output
        .platform_output
        .accesskit_update
        .expect("AccessKit output is enabled")
}

fn run_accesskit_frame_with_competing_popup(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    size: egui::Vec2,
    competing_id: Id,
) -> TreeUpdate {
    let output = context.run_ui(input_with_size(Vec::new(), size), |ui| {
        dockspace
            .show(SURFACE, ui, panes)
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
) -> (TreeUpdate, f32) {
    let mut outer_offset = None;
    let output = context.run_ui(input_with_size(events, size), |ui| {
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
                        .show(SURFACE, dock_ui, panes)
                        .expect("nested AccessKit frame must advance");
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
            .any(|observation| !observation.interactions_current),
        "the initial projection must have an observation-only pass"
    );
    assert!(first.last().expect("one pass").interactions_current);
    let second = run_frame(context, dockspace, panes, Vec::new());
    let stable = second.into_iter().last().expect("one stable pass");
    assert!(stable.interactions_current);
    stable
}

fn contained_close_id(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    salt: &'static str,
) -> Id {
    let mut close_id = None;
    let _ = context.run_ui(input(Vec::new()), |ui| {
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
            .show(SURFACE, ui, panes)
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
        .engine()
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
        .engine()
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
        .engine()
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
    reason = "fixture geometry is finite and deliberately converted into egui's f32 input space"
)]
fn contained_close_point(dockspace: &Dockspace) -> Pos2 {
    let floating = dockspace
        .engine()
        .workspace()
        .contained_floating(FLOATING)
        .expect("fixture has one contained floating");
    let style = dockspace.style();
    let border = style.floating_border_width;
    let title_height = style.floating_title_height;
    let resize = style.floating_resize_extent;
    let inner_title_max_x = floating.rect.max().x() as f32 - border - resize;
    let close_size = style.tab_close_size.min(title_height - 2.0 * resize);
    Pos2::new(
        inner_title_max_x - style.tab_horizontal_padding - close_size * 0.5,
        floating.rect.min().y() as f32 + border + title_height * 0.5,
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
    let scene = dockspace.engine().scene().expect("scene was published");
    let SurfaceScene::Ready(ready) = scene.surface(SURFACE).expect("surface is in roster") else {
        panic!("surface must be ready");
    };
    let splitter = ready
        .splitters()
        .first()
        .expect("split fixture has one splitter")
        .rect();
    let visible = Rect::from_min_max(
        Pos2::new(splitter.min().x() as f32, splitter.min().y() as f32),
        Pos2::new(splitter.max().x() as f32, splitter.max().y() as f32),
    );
    Rect::from_center_size(
        visible.center(),
        vec2(dockspace.style().splitter_hit_extent, visible.height()),
    )
    .intersect(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0)))
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "the published finite scene rectangle is converted back to egui's f32 input space"
)]
fn tab_close_center(dockspace: &Dockspace) -> Pos2 {
    tab_close_rect(dockspace).center()
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "the published finite scene rectangle is converted back to egui's f32 input space"
)]
fn tab_close_rect(dockspace: &Dockspace) -> Rect {
    let scene = dockspace.engine().scene().expect("scene was published");
    let SurfaceScene::Ready(ready) = scene.surface(SURFACE).expect("surface is in roster") else {
        panic!("surface must be ready");
    };
    let tab = ready.tabs().first().expect("fixture has one tab").rect();
    let style = dockspace.style();
    let width = tab.width();
    let height = tab.height();
    let close_size = f64::from(style.tab_close_size).min(width).min(height);
    Rect::from_center_size(
        Pos2::new(
            (tab.max().x() - f64::from(style.tab_horizontal_padding) - close_size * 0.5) as f32,
            ((tab.min().y() + tab.max().y()) * 0.5) as f32,
        ),
        egui::Vec2::splat(close_size as f32),
    )
    .intersect(Rect::from_min_max(
        Pos2::new(tab.min().x() as f32, tab.min().y() as f32),
        Pos2::new(tab.max().x() as f32, tab.max().y() as f32),
    ))
}

fn published_tab_rect(dockspace: &Dockspace, item: ItemId) -> Option<LogicalRect> {
    let scene = dockspace.engine().scene()?;
    let SurfaceScene::Ready(ready) = scene.surface(SURFACE)? else {
        return None;
    };
    ready
        .tabs()
        .iter()
        .find(|tab| tab.id().item == item)
        .map(dockspace::scene::SemanticRect::rect)
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
    let root = dockspace.engine().workspace().root(root)?;
    match dockspace.engine().workspace().node(root.node)? {
        Node::Tabs { selected, .. } => *selected,
        Node::Split { .. } => None,
    }
}

fn selected_in_group_containing(dockspace: &Dockspace, item: ItemId) -> Option<ItemId> {
    dockspace
        .engine()
        .workspace()
        .nodes()
        .find_map(|(_, node)| match node {
            Node::Tabs { items, selected } if items.contains(&item) => *selected,
            Node::Tabs { .. } | Node::Split { .. } => None,
        })
}

#[test]
fn scene_is_ready_before_paint_and_paint_does_not_mutate_workspace() {
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
    assert!(
        observations
            .iter()
            .any(|observation| !observation.interactions_current)
    );
    assert!(painted.interactions_current);
    assert_eq!(painted.pass, 1);
    assert!(matches!(
        dockspace
            .engine()
            .scene()
            .and_then(|scene| scene.surface(SURFACE)),
        Some(SurfaceScene::Ready(_))
    ));
    assert_eq!(panes.ui_calls(ITEM_A), observations.len());
}

#[test]
fn overflowing_tabs_preserve_selected_identity_without_publishing_hidden_hits() {
    let context = Context::default();
    let workspace = single_workspace([ITEM_A, ITEM_B, ITEM_C]);
    let mut dockspace = Dockspace::builder("tab-overflow", workspace)
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B, ITEM_C]);
    let size = vec2(180.0, 200.0);

    run_frame_with_size(&context, &mut dockspace, &mut panes, size, Vec::new());
    run_frame_with_size(&context, &mut dockspace, &mut panes, size, Vec::new());
    assert!(published_tab_rect(&dockspace, ITEM_A).is_some());
    assert!(published_tab_rect(&dockspace, ITEM_C).is_none());

    run_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![
            Event::PointerMoved(Pos2::new(100.0, 14.0)),
            Event::MouseWheel {
                unit: MouseWheelUnit::Point,
                delta: vec2(0.0, -72.0),
                phase: TouchPhase::Move,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    run_frame_with_size(&context, &mut dockspace, &mut panes, size, Vec::new());
    assert!(
        published_tab_rect(&dockspace, ITEM_A).is_some(),
        "ordinary wheel scrolling must preserve selected tab chrome"
    );
    assert_eq!(
        published_tab_rect(&dockspace, ITEM_B).map(LogicalRect::width),
        Some(72.0)
    );
    assert!(published_tab_rect(&dockspace, ITEM_C).is_none());

    let SurfaceScene::Ready(ready) = dockspace
        .engine()
        .scene()
        .and_then(|scene| scene.surface(SURFACE))
        .expect("overflow fixture publishes a ready scene")
    else {
        panic!("overflow fixture surface must be ready");
    };
    assert!(
        ready
            .tabs()
            .iter()
            .all(|tab| { tab.rect().min().x() >= 28.0 && tab.rect().max().x() <= 152.0 })
    );
    assert!(ready.drop_targets().iter().all(|target| {
        !matches!(target.id(), DropTargetId::TabGap { .. })
            || target.region().rect().min().x() >= 28.0 && target.region().rect().max().x() <= 152.0
    }));

    let tabs = dockspace
        .engine()
        .workspace()
        .root(MAIN_ROOT)
        .expect("fixture root exists")
        .node;
    let source = dockspace
        .engine()
        .workspace()
        .capture_item_source(MAIN_ROOT, tabs, ITEM_C)
        .expect("hidden item source captures");
    dockspace
        .enqueue_command(dockspace::command::WorkspaceCommand::Select { source })
        .expect("selection command enqueues");
    run_frame_with_size(&context, &mut dockspace, &mut panes, size, Vec::new());
    assert_eq!(
        published_tab_rect(&dockspace, ITEM_C).map(LogicalRect::width),
        Some(72.0)
    );
}

#[test]
fn keyboard_focused_tab_survives_extreme_wheel_scroll() {
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

    run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
    let stable = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
    let (first, _) = accesskit_node_by_label(&stable, Role::Tab, &format!("Pane {}", ITEM_A.get()));
    run_accesskit_frame(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![accesskit_action(first, Action::Focus)],
    );
    run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
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
    run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
    let focused = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
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
fn active_dragged_tab_survives_extreme_wheel_scroll() {
    let context = Context::default();
    let size = vec2(220.0, 200.0);
    let mut dockspace = Dockspace::builder(
        "dragged-tab-wheel-guard",
        single_workspace([ITEM_A, ITEM_B, ITEM_C, ITEM_D, ITEM_E]),
    )
    .build()
    .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B, ITEM_C, ITEM_D, ITEM_E]);
    run_frame_with_size(&context, &mut dockspace, &mut panes, size, Vec::new());
    run_frame_with_size(&context, &mut dockspace, &mut panes, size, Vec::new());

    let dragged_before = published_tab_rect(&dockspace, ITEM_B).expect("second tab is visible");
    let press = logical_rect_center(dragged_before);
    let current = press + vec2(20.0, 0.0);
    run_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![Event::PointerMoved(press), pointer_button(press, true)],
    );
    run_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![Event::PointerMoved(current)],
    );
    run_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![Event::PointerMoved(current)],
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));

    run_frame_with_size(
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
    run_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![Event::PointerMoved(current)],
    );

    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    assert!(
        published_tab_rect(&dockspace, ITEM_A).is_some(),
        "feasible selected identity remains visible"
    );
    assert!(
        published_tab_rect(&dockspace, ITEM_B).is_some(),
        "active drag source remains visible"
    );
}

#[test]
fn frontmost_floating_tab_strip_exclusively_owns_overlapping_wheel_scroll() {
    let context = Context::default();
    let size = vec2(220.0, 250.0);
    let floating_rect =
        LogicalRect::new(0.0, 60.0, 180.0, 160.0).expect("floating fixture rect is valid");
    let workspace = overlapping_overflow_workspace(floating_rect);
    let mut dockspace = Dockspace::builder("floating-wheel-owner", workspace)
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B, ITEM_C, ITEM_D, ITEM_E, ITEM_F, ITEM_G]);
    run_frame_with_size(&context, &mut dockspace, &mut panes, size, Vec::new());
    run_frame_with_size(&context, &mut dockspace, &mut panes, size, Vec::new());

    let background_before = published_tab_rect(&dockspace, ITEM_A).expect("main tab is visible");
    let foreground_before =
        published_tab_rect(&dockspace, ITEM_D).expect("floating tab is visible");
    let overlap = logical_rect_intersection(background_before, foreground_before)
        .expect("fixture tab strips overlap");
    let pointer = logical_rect_center(overlap);
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
    run_frame_with_size(&context, &mut dockspace, &mut panes, size, Vec::new());

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
    run_frame_with_size(&context, &mut dockspace, &mut panes, size, Vec::new());
    run_frame_with_size(&context, &mut dockspace, &mut panes, size, Vec::new());

    let background_before = published_tab_rect(&dockspace, ITEM_A).expect("main tab is visible");
    let foreground_before =
        published_tab_rect(&dockspace, ITEM_D).expect("floating tab is visible");
    let overlap = logical_rect_intersection(background_before, foreground_before)
        .expect("fixture tab strips overlap");
    let press = logical_rect_center(foreground_before);
    #[allow(
        clippy::cast_possible_truncation,
        reason = "finite fixture geometry is converted to egui input coordinates"
    )]
    let edge = Pos2::new(
        floating_rect.max().x() as f32
            - dockspace.style().floating_border_width
            - dockspace.style().tab_bar_height
            - 4.0,
        logical_rect_center(overlap).y,
    );

    run_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![Event::PointerMoved(press), pointer_button(press, true)],
    );
    run_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![Event::PointerMoved(edge)],
    );
    run_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![Event::PointerMoved(edge)],
    );
    run_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![Event::PointerMoved(logical_rect_center(overlap))],
    );

    assert!(matches!(
        dockspace.engine().interaction().status(),
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
            .interactions_current
    );
    assert!(published_tab_rect(&dockspace, ITEM_C).is_none());

    dockspace
        .replace_workspace(single_workspace([ITEM_A, ITEM_B, ITEM_C, ITEM_D]))
        .expect("hidden-item addition queues");
    let added = run_frame_with_size(&context, &mut dockspace, &mut panes, size, Vec::new());
    assert!(
        added
            .iter()
            .any(|observation| !observation.interactions_current)
    );
    assert!(
        added
            .last()
            .expect("addition repaints")
            .interactions_current
    );
    assert!(published_tab_rect(&dockspace, ITEM_D).is_none());

    dockspace
        .replace_workspace(single_workspace([ITEM_A, ITEM_B, ITEM_C]))
        .expect("hidden-item removal queues");
    let removed = run_frame_with_size(&context, &mut dockspace, &mut panes, size, Vec::new());
    assert!(
        removed
            .iter()
            .any(|observation| !observation.interactions_current)
    );
    assert!(
        removed
            .last()
            .expect("removal repaints")
            .interactions_current
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
        let (overflow, overflow_node) =
            accesskit_node_by_label(&stable, Role::Button, "Show hidden tabs");
        assert!(overflow_node.supports_action(Action::Click));

        let mut pre_open_style = dockspace.style().clone();
        pre_open_style.tab_horizontal_padding += 1.0;
        dockspace
            .set_style(pre_open_style)
            .expect("pre-open overflow style remains valid");

        let open = run_accesskit_frame(
            &context,
            &mut dockspace,
            &mut panes,
            size,
            vec![accesskit_action(overflow, Action::Click)],
        );
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
        let (hidden, _) = accesskit_node_by_label(&after_stale_pass, Role::MenuItem, &hidden_label);
        assert_eq!(hidden, hidden_before_stale_pass);

        if keyboard {
            run_accesskit_frame(
                &context,
                &mut dockspace,
                &mut panes,
                size,
                vec![accesskit_action(hidden, Action::Focus)],
            );
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

        assert_eq!(selected_item(&dockspace, MAIN_ROOT), Some(ITEM_C));
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

        let opened =
            run_accesskit_frame(&context, &mut dockspace, &mut panes, size, key_press(key));
        let (_, overflow_node) = accesskit_node_by_label(&opened, Role::Button, "Show hidden tabs");
        assert_eq!(overflow_node.is_expanded(), Some(true));
        accesskit_node_by_label(&opened, Role::MenuItem, &format!("Pane {}", ITEM_B.get()));

        let settled = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        assert_eq!(selected_item(&dockspace, MAIN_ROOT), Some(ITEM_A));
        assert_eq!(
            accesskit_node_by_id(&settled, overflow).is_expanded(),
            Some(true)
        );
    }
}

#[test]
fn external_popup_closure_is_not_resurrected_by_overflow_adapter_state() {
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
        let closed = if competing_popup {
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
            accesskit_node_by_id(&closed, overflow).is_expanded(),
            Some(false)
        );
        assert_eq!(
            egui::Popup::is_id_open(&context, competing_id),
            competing_popup,
            "closing overflow state must not disturb a competing popup"
        );
        let still_closed =
            run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        assert_eq!(
            accesskit_node_by_id(&still_closed, overflow).is_expanded(),
            Some(false)
        );
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn large_overflow_menu_reaches_the_last_item_by_pointer_keyboard_and_accesskit() {
    for mode in ["pointer", "keyboard", "accesskit"] {
        let context = Context::default();
        context.enable_accesskit();
        let size = vec2(180.0, 140.0);
        let items = numbered_items(1_000, 64);
        let last = *items.last().expect("large fixture has a last item");
        let mut dockspace = Dockspace::builder(mode, single_workspace(items.iter().copied()))
            .build()
            .expect("facade must build");
        let mut panes = TestPanes::with_items(items.iter().copied());

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
        let menu = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        let (_, menu_node) = accesskit_node_by_role(&menu, Role::Menu);
        let menu_rect = accesskit_node_rect(menu_node);
        assert!(menu_node.clips_children());
        assert!(menu_node.child_supports_action(Action::ScrollIntoView));
        assert!(menu_rect.min.x >= 0.0 && menu_rect.min.y >= 0.0);
        assert!(
            menu_rect.max.x <= size.x && menu_rect.max.y <= size.y,
            "bounded menu {menu_rect:?} must fit host size {size:?}"
        );
        assert!(
            menu.nodes
                .iter()
                .filter(|(_, node)| node.role() == Role::MenuItem)
                .count()
                >= 60,
            "the bounded popup must retain the complete stable accessibility roster"
        );
        let last_label = format!("Pane {}", last.get());
        let (last_node_id, last_node) = accesskit_node_by_label(&menu, Role::MenuItem, &last_label);
        assert!(last_node.supports_action(Action::Focus));
        assert!(last_node.supports_action(Action::Click));
        assert!(last_node.supports_action(Action::ScrollIntoView));

        match mode {
            "pointer" => {
                let pointer = menu_rect.center();
                let mut scrolled = run_accesskit_frame(
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
                for _ in 0..32 {
                    scrolled =
                        run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
                }
                let (_, last_node) =
                    accesskit_node_by_label(&scrolled, Role::MenuItem, &last_label);
                let last_center = accesskit_node_center(last_node);
                assert!(
                    menu_rect.contains(last_center),
                    "scrolled last item center {last_center:?} must enter menu {menu_rect:?}"
                );
                run_accesskit_frame(
                    &context,
                    &mut dockspace,
                    &mut panes,
                    size,
                    vec![
                        Event::PointerMoved(last_center),
                        pointer_button(last_center, true),
                        pointer_button(last_center, false),
                    ],
                );
            }
            "keyboard" => {
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
                run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
                let scrolled =
                    run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
                let (_, last_node) =
                    accesskit_node_by_label(&scrolled, Role::MenuItem, &last_label);
                assert!(menu_rect.contains(accesskit_node_center(last_node)));
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
            }
            "accesskit" => {
                run_accesskit_frame(
                    &context,
                    &mut dockspace,
                    &mut panes,
                    size,
                    vec![accesskit_action(last_node_id, Action::Click)],
                );
            }
            _ => unreachable!("fixture mode is exhaustive"),
        }
        run_frame_with_size(&context, &mut dockspace, &mut panes, size, Vec::new());
        run_frame_with_size(&context, &mut dockspace, &mut panes, size, Vec::new());
        assert_eq!(
            selected_item(&dockspace, MAIN_ROOT),
            Some(last),
            "large overflow activation failed in {mode} mode"
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
        let opening = run_accesskit_frame_in_rect(
            &context,
            &mut dockspace,
            &mut panes,
            screen,
            dock,
            vec![accesskit_action(overflow, Action::Click)],
        );
        let opening_menu = accesskit_node_rect(accesskit_node_by_role(&opening, Role::Menu).1);
        run_accesskit_frame_in_rect(
            &context,
            &mut dockspace,
            &mut panes,
            screen,
            dock,
            Vec::new(),
        );
        let opened = run_accesskit_frame_in_rect(
            &context,
            &mut dockspace,
            &mut panes,
            screen,
            dock,
            Vec::new(),
        );
        let (_, menu) = accesskit_node_by_role(&opened, Role::Menu);
        let menu = accesskit_node_rect(menu);
        let overflow_rect = accesskit_node_rect(accesskit_node_by_id(&opened, overflow));

        assert_eq!(
            menu, opening_menu,
            "popup geometry shifted after its opening frame at {pixels_per_point} PPP"
        );
        assert!(
            menu.min.x > screen.min.x,
            "left edge escaped at {pixels_per_point} PPP"
        );
        assert!(
            menu.min.y > screen.min.y,
            "top edge escaped at {pixels_per_point} PPP"
        );
        assert!(
            menu.max.x < screen.max.x,
            "right edge escaped at {pixels_per_point} PPP"
        );
        assert!(
            menu.max.y < screen.max.y,
            "bottom edge escaped at {pixels_per_point} PPP"
        );
        assert!(
            menu.max.y <= overflow_rect.min.y + 0.5 / pixels_per_point + egui::emath::GUI_ROUNDING,
            "the fixture popup {menu:?} must open above {overflow_rect:?} at {pixels_per_point} PPP"
        );
        assert!(
            menu.min.x - screen.min.x < 2.0 / pixels_per_point + 1.0,
            "the wide popup must exercise the left host budget at {pixels_per_point} PPP"
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
    let opened = run_accesskit_frame_in_rect(
        &context,
        &mut dockspace,
        &mut panes,
        screen,
        dock,
        vec![accesskit_action(overflow, Action::Click)],
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
    let tiny = run_accesskit_frame_in_rect(
        &context,
        &mut dockspace,
        &mut panes,
        tiny_screen,
        tiny_dock,
        Vec::new(),
    );
    assert!(tiny.nodes.iter().all(|(_, node)| node.role() != Role::Menu));

    let restored = run_accesskit_frame_in_rect(
        &context,
        &mut dockspace,
        &mut panes,
        screen,
        dock,
        Vec::new(),
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
fn overflow_menu_short_items_own_the_full_clickable_row() {
    let context = Context::default();
    context.enable_accesskit();
    let size = vec2(180.0, 200.0);
    let mut dockspace = Dockspace::builder(
        "overflow-full-width-short-row",
        single_workspace([ITEM_A, ITEM_B, ITEM_C]),
    )
    .build()
    .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B, ITEM_C]);
    let short_label = "B".to_owned();
    let long_label = "An intentionally wide overflow menu item".to_owned();
    panes.titles.insert(ITEM_B, short_label.clone());
    panes.titles.insert(ITEM_C, long_label.clone());

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
    let short =
        accesskit_node_rect(accesskit_node_by_label(&opened, Role::MenuItem, &short_label).1);
    let long = accesskit_node_rect(accesskit_node_by_label(&opened, Role::MenuItem, &long_label).1);

    assert_eq!(short.x_range(), long.x_range());
    let row_end = Pos2::new(long.max.x - 1.0, short.center().y);
    run_accesskit_frame(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![
            Event::PointerMoved(row_end),
            pointer_button(row_end, true),
            pointer_button(row_end, false),
        ],
    );
    run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());

    assert_eq!(selected_item(&dockspace, MAIN_ROOT), Some(ITEM_B));
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
    let opened = run_accesskit_frame_in_rect(
        &context,
        &mut dockspace,
        &mut panes,
        screen,
        dock,
        vec![accesskit_action(overflow, Action::Click)],
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

    let narrow = run_accesskit_frame_in_rect(
        &context,
        &mut dockspace,
        &mut panes,
        narrow_screen,
        narrow_dock,
        Vec::new(),
    );
    assert!(
        narrow
            .nodes
            .iter()
            .all(|(_, node)| node.role() != Role::Menu)
    );

    let restored = run_accesskit_frame_in_rect(
        &context,
        &mut dockspace,
        &mut panes,
        screen,
        dock,
        Vec::new(),
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
    let opened = run_accesskit_frame(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![accesskit_action(overflow, Action::Click)],
    );
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

    let resized = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
    let (_, resized_scrollbar) = accesskit_node_by_role(&resized, Role::ScrollBar);
    let resized_scrollbar_bounds = resized_scrollbar.bounds();
    let (_, resized_first_hidden) =
        accesskit_node_by_label(&resized, Role::MenuItem, &first_hidden_label);
    let resized_first_hidden_bounds = resized_first_hidden.bounds();
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
#[allow(
    clippy::too_many_lines,
    reason = "the full nested-scroll sequence verifies both boundaries and smooth follow-up frames"
)]
fn overflow_popup_owns_wheel_residue_inside_an_ancestor_scroll_area() {
    let context = Context::default();
    context.enable_accesskit();
    let salt = "overflow-ancestor-scroll";
    let size = vec2(220.0, 180.0);
    let items = numbered_items(1_600, 64);
    let last = *items.last().expect("large fixture has a last item");
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
    let (stable, baseline_offset) = run_accesskit_frame_in_ancestor_scroll_area(
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
    let (opened, opened_outer_offset) = run_accesskit_frame_in_ancestor_scroll_area(
        &context,
        &mut dockspace,
        &mut panes,
        salt,
        size,
        None,
        Vec::new(),
    );
    assert!((opened_outer_offset - baseline_offset).abs() < 0.01);
    let (_, menu) = accesskit_node_by_role(&opened, Role::Menu);
    let pointer = accesskit_node_rect(menu).center();

    let (_, top_boundary_offset) = run_accesskit_frame_in_ancestor_scroll_area(
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
                delta: vec2(0.0, 10_000.0),
                phase: TouchPhase::Move,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    assert!(
        (top_boundary_offset - baseline_offset).abs() < 0.01,
        "unconsumed wheel input at the popup top must not scroll its ancestor"
    );
    for _ in 0..8 {
        let (_, offset) = run_accesskit_frame_in_ancestor_scroll_area(
            &context,
            &mut dockspace,
            &mut panes,
            salt,
            size,
            None,
            Vec::new(),
        );
        assert!(
            (offset - baseline_offset).abs() < 0.01,
            "smooth follow-up frames remain owned by the popup"
        );
    }

    let (_, downward_offset) = run_accesskit_frame_in_ancestor_scroll_area(
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
    assert!((downward_offset - baseline_offset).abs() < 0.01);
    let mut bottom = None;
    for _ in 0..16 {
        let (update, offset) = run_accesskit_frame_in_ancestor_scroll_area(
            &context,
            &mut dockspace,
            &mut panes,
            salt,
            size,
            None,
            Vec::new(),
        );
        assert!((offset - baseline_offset).abs() < 0.01);
        bottom = Some(update);
    }
    let bottom = bottom.expect("smooth scrolling produces follow-up frames");
    let (_, bottom_menu) = accesskit_node_by_role(&bottom, Role::Menu);
    let (_, last_node) =
        accesskit_node_by_label(&bottom, Role::MenuItem, &format!("Pane {}", last.get()));
    assert!(
        accesskit_node_rect(bottom_menu).contains(accesskit_node_center(last_node)),
        "fixture reaches the popup bottom before testing boundary ownership"
    );

    let (_, bottom_boundary_offset) = run_accesskit_frame_in_ancestor_scroll_area(
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
    assert!(
        (bottom_boundary_offset - baseline_offset).abs() < 0.01,
        "unconsumed wheel input at the popup bottom must not scroll its ancestor"
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn overflow_popup_wheel_never_falls_through_to_tiled_or_floating_tab_strips() {
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
        assert!((accesskit_node_center(last_after).y - last_y_before).abs() > f32::EPSILON);
        assert_eq!(
            published_tab_rect(&dockspace, popup_items[0]),
            Some(popup_tab_before)
        );
        assert_eq!(
            published_tab_rect(&dockspace, underlay_items[1]),
            Some(underlay_before)
        );

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
#[allow(
    clippy::too_many_lines,
    reason = "one parameterized ownership scenario keeps identical pointer assertions across layers"
)]
fn overflow_popup_pointer_ownership_blocks_tiled_and_floating_underlays() {
    for floating_underlay in [false, true] {
        for interaction in ["row", "gap", "scrollbar"] {
            let context = Context::default();
            context.enable_accesskit();
            context.all_styles_mut(|style| {
                style.spacing.item_spacing.y = 10.0;
                style.spacing.scroll.floating = false;
                style.spacing.scroll.bar_width = 16.0;
                style.spacing.scroll.bar_inner_margin = 2.0;
                style.spacing.scroll.bar_outer_margin = 2.0;
            });
            let size = vec2(220.0, 220.0);
            let popup_items = numbered_items(4_000, 32);
            let underlay_items = numbered_items(5_000, 4);
            let workspace = popup_overlapping_tab_strip_workspace(
                &popup_items,
                &underlay_items,
                floating_underlay,
            );
            let salt = format!("popup-pointer-{floating_underlay}-{interaction}");
            let mut dockspace = Dockspace::builder(salt, workspace)
                .build()
                .expect("facade must build");
            let mut panes =
                TestPanes::with_items(popup_items.iter().chain(&underlay_items).copied());

            run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
            let stable =
                run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
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
            let underlay_tab = egui_rect(
                published_tab_rect(&dockspace, underlay_items[1]).expect("underlay tab is visible"),
            );
            let (_, underlay_panel_node) = accesskit_node_by_label(
                &menu,
                Role::TabPanel,
                &format!("Pane {}", underlay_items[0].get()),
            );
            let underlay_panel = accesskit_node_rect(underlay_panel_node);
            let (_, scrollbar_node) = accesskit_node_by_role(&menu, Role::ScrollBar);
            let scrollbar_response = accesskit_node_rect(scrollbar_node);
            let scrollbar_overlap = scrollbar_response
                .intersect(menu_rect)
                .intersect(underlay_panel);
            assert!(
                scrollbar_overlap.is_positive(),
                "the actual popup scrollbar response overlaps the underlay pane"
            );
            let mut visible_rows = popup_items
                .iter()
                .filter_map(|item| {
                    let label = format!("Pane {}", item.get());
                    let node = menu.nodes.iter().find_map(|(_, node)| {
                        (node.role() == Role::MenuItem && node.label() == Some(label.as_str()))
                            .then_some(node)
                    })?;
                    let rect = accesskit_node_rect(node);
                    rect.intersect(menu_rect)
                        .is_positive()
                        .then_some((*item, rect))
                })
                .collect::<Vec<_>>();
            visible_rows.sort_by(|(_, left), (_, right)| left.min.y.total_cmp(&right.min.y));
            let row = visible_rows
                .iter()
                .find_map(|(item, rect)| {
                    let overlap = rect.intersect(underlay_tab).intersect(menu_rect);
                    overlap.is_positive().then_some((*item, overlap.center()))
                })
                .expect("a popup row covers the underlay tab strip");
            let gap = visible_rows
                .windows(2)
                .find_map(|rows| {
                    let left = rows[0].1.min.x.max(rows[1].1.min.x);
                    let right = rows[0].1.max.x.min(rows[1].1.max.x);
                    let gap = Rect::from_min_max(
                        Pos2::new(left, rows[0].1.max.y),
                        Pos2::new(right, rows[1].1.min.y),
                    )
                    .intersect(underlay_tab)
                    .intersect(menu_rect);
                    gap.is_positive().then_some(gap.center())
                })
                .expect("a popup row gap covers the underlay tab strip");
            let scrollbar = scrollbar_overlap.center();
            let underlay_selected = selected_in_group_containing(&dockspace, underlay_items[0]);
            let underlay_clicks = panes.pane_clicks(underlay_items[0]);
            let underlay_closes = panes.close_calls(underlay_items[0]);
            let version_before = dockspace.engine().version();
            let (pointer, drag_delta) = match interaction {
                "row" => (row.1, egui::Vec2::ZERO),
                "gap" => (gap, vec2(4.0, 0.0)),
                "scrollbar" => (scrollbar, vec2(0.0, 24.0)),
                _ => unreachable!("fixture interaction is exhaustive"),
            };
            run_accesskit_frame(
                &context,
                &mut dockspace,
                &mut panes,
                size,
                vec![Event::PointerMoved(pointer), pointer_button(pointer, true)],
            );
            if drag_delta != egui::Vec2::ZERO {
                run_accesskit_frame(
                    &context,
                    &mut dockspace,
                    &mut panes,
                    size,
                    vec![Event::PointerMoved(pointer + drag_delta)],
                );
            }
            run_accesskit_frame(
                &context,
                &mut dockspace,
                &mut panes,
                size,
                vec![
                    Event::PointerMoved(pointer + drag_delta),
                    pointer_button(pointer + drag_delta, false),
                ],
            );
            let settled =
                run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());

            assert_eq!(
                selected_in_group_containing(&dockspace, underlay_items[0]),
                underlay_selected
            );
            assert_eq!(panes.pane_clicks(underlay_items[0]), underlay_clicks);
            assert_eq!(panes.close_calls(underlay_items[0]), underlay_closes);
            assert_eq!(
                dockspace.engine().interaction().status(),
                InteractionStatus::Idle
            );
            let expanded = accesskit_node_by_id(&settled, overflow).is_expanded();
            if interaction == "row" {
                assert_eq!(
                    selected_in_group_containing(&dockspace, row.0),
                    Some(row.0),
                    "popup row activation failed for floating_underlay={floating_underlay}"
                );
                assert_eq!(expanded, Some(false));
            } else {
                assert_eq!(dockspace.engine().version(), version_before);
                assert_eq!(expanded, Some(true));
            }
        }
    }
}

#[test]
fn stale_overflow_clicks_cannot_activate_or_close_the_popup() {
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
        let authoritative_menu =
            run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        let old_label = format!("Pane {}", ITEM_C.get());
        let (_, hidden) = accesskit_node_by_label(&authoritative_menu, Role::MenuItem, &old_label);
        let stale_click = if click_item {
            accesskit_node_center(hidden)
        } else {
            Pos2::new(4.0, size.y - 4.0)
        };

        let changed_label = format!("Pane {} with a much wider overflow label", ITEM_C.get());
        panes.titles.insert(ITEM_C, changed_label.clone());
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
        assert!(!stale_frame[0].interactions_current);
        assert_eq!(selected_item(&dockspace, MAIN_ROOT), Some(ITEM_A));

        context.options_mut(|options| {
            options.max_passes = 4.try_into().expect("four is non-zero");
        });
        let recovered = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
        accesskit_node_by_label(&recovered, Role::MenuItem, &changed_label);
        assert_eq!(selected_item(&dockspace, MAIN_ROOT), Some(ITEM_A));
    }
}

#[test]
fn opening_overflow_click_survives_geometry_discard_in_the_same_frame() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let size = vec2(180.0, 200.0);
    let mut dockspace = Dockspace::builder(
        "overflow-open-multipass",
        single_workspace([ITEM_A, ITEM_B, ITEM_C]),
    )
    .build()
    .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B, ITEM_C]);

    run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
    let stable = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
    let (_, overflow) = accesskit_node_by_label(&stable, Role::Button, "Show hidden tabs");
    let open = accesskit_node_center(overflow);
    let opened = run_accesskit_frame(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![
            Event::PointerMoved(open),
            pointer_button(open, true),
            pointer_button(open, false),
        ],
    );

    let (_, overflow) = accesskit_node_by_label(&opened, Role::Button, "Show hidden tabs");
    assert_eq!(overflow.is_expanded(), Some(true));
    accesskit_node_by_label(&opened, Role::MenuItem, &format!("Pane {}", ITEM_C.get()));
}

#[test]
fn authoritative_overflow_outside_click_closes_the_popup() {
    let context = Context::default();
    context.enable_accesskit();
    let size = vec2(180.0, 200.0);
    let mut dockspace = Dockspace::builder(
        "authoritative-overflow-outside-click",
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
    run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());

    let outside = Pos2::new(4.0, size.y - 4.0);
    run_accesskit_frame(
        &context,
        &mut dockspace,
        &mut panes,
        size,
        vec![
            Event::PointerMoved(outside),
            pointer_button(outside, true),
            pointer_button(outside, false),
        ],
    );
    let closed = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
    let (_, overflow_node) = accesskit_node_by_label(&closed, Role::Button, "Show hidden tabs");

    assert_eq!(overflow_node.is_expanded(), Some(false));
    assert_eq!(selected_item(&dockspace, MAIN_ROOT), Some(ITEM_A));
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
    let opened = run_accesskit_frame(&context, &mut dockspace, &mut panes, size, Vec::new());
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
fn stale_projection_paints_pane_but_disables_widget_interaction() {
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
    assert!(!stale_observations[0].interactions_current);
    assert_eq!(panes.ui_calls(ITEM_A), ui_calls_before + 1);
    assert_eq!(panes.disabled_ui_calls(ITEM_A), disabled_calls_before + 1);
    assert_eq!(panes.pane_clicks(ITEM_A), clicks_before);

    let current_observations = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        current_observations
            .last()
            .expect("stable frame paints")
            .interactions_current
    );
    assert_eq!(panes.ui_calls(ITEM_A), ui_calls_before + 2);
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
fn selected_pane_is_part_of_projection_authority_before_pointer_delivery() {
    let context = Context::default();
    let workspace = single_workspace([ITEM_A, ITEM_B]);
    let mut dockspace = Dockspace::builder("selected-pane-fingerprint", workspace.clone())
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    warm(&context, &mut dockspace, &mut panes);

    let source = dockspace
        .engine()
        .workspace()
        .roots()
        .next()
        .and_then(|(root, record)| {
            dockspace
                .engine()
                .workspace()
                .capture_item_source(root, record.node, ITEM_B)
                .ok()
        })
        .expect("selected pane source must capture");
    dockspace
        .enqueue_command(dockspace::command::WorkspaceCommand::Select { source })
        .expect("selection command must enqueue");
    context.options_mut(|options| {
        options.max_passes = 1.try_into().expect("one is non-zero");
    });

    let b_ui_before = panes.ui_calls(ITEM_B);
    let b_disabled_before = panes.disabled_ui_calls(ITEM_B);
    let b_clicks_before = panes.pane_clicks(ITEM_B);
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
    assert!(!stale_frame[0].interactions_current);
    assert_eq!(panes.ui_calls(ITEM_B), b_ui_before + 1);
    assert_eq!(panes.disabled_ui_calls(ITEM_B), b_disabled_before + 1);
    assert_eq!(panes.pane_clicks(ITEM_B), b_clicks_before);

    let recovered_frame = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        recovered_frame
            .last()
            .expect("stable frame paints")
            .interactions_current
    );
    assert_eq!(panes.ui_calls(ITEM_B), b_ui_before + 2);
    assert_eq!(panes.disabled_ui_calls(ITEM_B), b_disabled_before + 1);
    assert_eq!(panes.pane_clicks(ITEM_B), b_clicks_before);
    assert_eq!(
        dockspace
            .engine()
            .workspace()
            .roots()
            .next()
            .and_then(|(_, root)| dockspace.engine().workspace().node(root.node))
            .and_then(|node| match node {
                Node::Tabs { selected, .. } => *selected,
                Node::Split { .. } => None,
            }),
        Some(ITEM_B)
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
    assert_eq!(panes.ui_calls(ITEM_A), recovered.len());
    assert_eq!(dockspace.engine().workspace(), &workspace);
}

#[test]
fn close_veto_runs_once_at_the_next_frame_boundary() {
    let context = Context::default();
    let workspace = single_workspace([ITEM_A]);
    let mut dockspace = Dockspace::builder("close-veto", workspace.clone())
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A]);
    panes.close_response.insert(ITEM_A, PaneCloseResponse::Veto);
    warm(&context, &mut dockspace, &mut panes);
    let close = tab_close_center(&dockspace);

    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(close), pointer_button(close, true)],
    );
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(close), pointer_button(close, false)],
    );
    assert_eq!(panes.close_calls(ITEM_A), 0);
    assert_eq!(dockspace.engine().workspace(), &workspace);

    run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(panes.close_calls(ITEM_A), 1);
    assert_eq!(dockspace.engine().workspace(), &workspace);
    assert_eq!(dockspace.engine().version(), WorkspaceVersion::default());
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
        let mut dockspace = Dockspace::builder(salt, workspace)
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
        run_frame(&context, &mut dockspace, &mut panes, vec![event]);
        assert!(
            dockspace
                .engine()
                .workspace()
                .contained_floating(FLOATING)
                .is_some()
        );
        assert_eq!(panes.close_calls(ITEM_B), 0);

        run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        assert!(
            dockspace
                .engine()
                .workspace()
                .contained_floating(FLOATING)
                .is_none()
        );
        assert!(dockspace.engine().workspace().root(FLOATING_ROOT).is_none());
        assert_eq!(panes.close_calls(ITEM_B), 1);
    }
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
    assert_eq!(panes.close_calls(ITEM_A), 0);
    assert_eq!(dockspace.engine().workspace(), &original);

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
        dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    );
    assert!(
        released
            .iter()
            .all(|observation| observation.workspace == original)
    );
}

#[test]
fn root_close_queries_every_pane_and_commits_atomically() {
    let context = Context::default();
    let rect = LogicalRect::new(120.0, 80.0, 300.0, 220.0).expect("valid fixture rect");
    let workspace = contained_group_workspace(rect);
    let mut dockspace = Dockspace::builder("root-close", workspace.clone())
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B, ITEM_C]);
    panes.close_response.insert(ITEM_B, PaneCloseResponse::Veto);
    warm(&context, &mut dockspace, &mut panes);
    let close = contained_close_point(&dockspace);

    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(close), pointer_button(close, true)],
    );
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(close), pointer_button(close, false)],
    );
    assert_eq!(panes.close_calls(ITEM_B), 0);
    assert_eq!(panes.close_calls(ITEM_C), 0);

    run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(panes.close_calls(ITEM_B), 1);
    assert_eq!(panes.close_calls(ITEM_C), 1);
    assert_eq!(dockspace.engine().workspace(), &workspace);

    panes
        .close_response
        .insert(ITEM_B, PaneCloseResponse::Allow);
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(close), pointer_button(close, true)],
    );
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(close), pointer_button(close, false)],
    );
    run_frame(&context, &mut dockspace, &mut panes, Vec::new());

    assert_eq!(panes.close_calls(ITEM_B), 2);
    assert_eq!(panes.close_calls(ITEM_C), 2);
    assert!(
        dockspace
            .engine()
            .workspace()
            .contained_floating(FLOATING)
            .is_none()
    );
    assert!(dockspace.engine().workspace().root(FLOATING_ROOT).is_none());
}

#[test]
fn multipass_never_reduces_inputs_queued_during_paint() {
    let context = Context::default();
    let workspace = single_workspace([ITEM_A, ITEM_B]);
    let mut dockspace = Dockspace::builder("multipass-boundary", workspace.clone())
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    warm(&context, &mut dockspace, &mut panes);
    let source = dockspace
        .engine()
        .workspace()
        .roots()
        .next()
        .and_then(|(root, record)| {
            dockspace
                .engine()
                .workspace()
                .capture_item_source(root, record.node, ITEM_B)
                .ok()
        })
        .expect("item source must capture");
    let mut command = Some(dockspace::command::WorkspaceCommand::Select { source });
    let mut versions = Vec::new();
    let mut workspaces = Vec::new();

    let _ = context.run_ui(input(Vec::new()), |ui| {
        dockspace
            .show(SURFACE, ui, &mut panes)
            .expect("multipass show must succeed");
        versions.push(dockspace.engine().version());
        workspaces.push(dockspace.engine().workspace().clone());
        if ui.ctx().current_pass_index() == 0 {
            dockspace
                .enqueue_command(command.take().expect("queued only in pass zero"))
                .expect("input sequence must advance");
            ui.ctx()
                .request_discard("exercise dockspace multipass gate");
        }
    });

    assert_eq!(versions.len(), 2);
    assert!(versions.iter().all(|version| *version == versions[0]));
    assert!(workspaces.iter().all(|candidate| candidate == &workspace));
    assert_eq!(dockspace.engine().pending_inputs().len(), 1);

    run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let selected = dockspace
        .engine()
        .workspace()
        .roots()
        .next()
        .and_then(|(_, root)| dockspace.engine().workspace().node(root.node))
        .and_then(|node| match node {
            Node::Tabs { selected, .. } => *selected,
            Node::Split { .. } => None,
        });
    assert_eq!(selected, Some(ITEM_B));
    assert_ne!(dockspace.engine().version(), WorkspaceVersion::default());
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

    for _ in 0..2 {
        let _ = context.run_ui(input(Vec::new()), |ui| {
            let mut child = ui.new_child(UiBuilder::new().max_rect(tiny));
            let response = dockspace
                .show(SURFACE, &mut child, &mut panes)
                .expect("collapsed host remains projectable");
            assert_eq!(response.surface_status(), DockspaceSurfaceStatus::Ready);
        });
    }

    assert_eq!(dockspace.engine().workspace(), &original);
    assert_eq!(
        dockspace.engine().workspace().item_multiset(),
        BTreeMap::from([(ITEM_A, 1), (ITEM_B, 1)])
    );
    assert!(matches!(
        dockspace
            .engine()
            .scene()
            .and_then(|scene| scene.surface(SURFACE)),
        Some(SurfaceScene::Ready(_))
    ));
}

#[test]
fn contained_bounds_reconciliation_is_proof_driven_and_conserves_items() {
    let context = Context::default();
    let original = LogicalRect::new(550.0, 330.0, 200.0, 160.0).expect("finite rect");
    let workspace = contained_workspace(original);
    let mut dockspace = Dockspace::builder("contained-clamp", workspace)
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    panes.minimum_sizes.insert(ITEM_B, vec2(420.0, 280.0));

    run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let floating = dockspace
        .engine()
        .workspace()
        .contained_floating(FLOATING)
        .expect("floating remains presented");

    assert_ne!(floating.rect, original);
    assert!(floating.rect.min().x() >= 0.0);
    assert!(floating.rect.min().y() >= 0.0);
    assert!(floating.rect.max().x() <= 600.0);
    assert!(floating.rect.max().y() <= 400.0);
    let style = dockspace.style();
    assert!(floating.rect.width() >= 420.0 + 2.0 * f64::from(style.floating_border_width));
    assert!(
        floating.rect.height()
            >= f64::from(280.0 + style.tab_bar_height + style.floating_title_height)
                + 2.0 * f64::from(style.floating_border_width)
    );
    assert_eq!(
        dockspace.engine().workspace().item_multiset(),
        BTreeMap::from([(ITEM_A, 1), (ITEM_B, 1)])
    );
    assert!(matches!(
        dockspace
            .engine()
            .scene()
            .and_then(|scene| scene.surface(SURFACE)),
        Some(SurfaceScene::Ready(_))
    ));
}

#[test]
fn contained_move_commits_only_the_last_painted_absolute_pointer_preview() {
    let context = Context::default();
    let original = LogicalRect::new(140.0, 90.0, 220.0, 160.0).expect("finite rect");
    let mut dockspace = Dockspace::builder("contained-move", contained_workspace(original))
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    warm(&context, &mut dockspace, &mut panes);

    let original_floating = *dockspace
        .engine()
        .workspace()
        .contained_floating(FLOATING)
        .expect("floating exists");
    let press_origin = contained_title_point(&dockspace);
    let painted_pointer = press_origin + vec2(48.0, 36.0);
    let release_pointer = press_origin + vec2(72.0, 54.0);

    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(press_origin),
            pointer_button(press_origin, true),
        ],
    );
    assert_eq!(contained_rect(&dockspace), original);

    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(painted_pointer)],
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Armed { .. }
    ));

    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(painted_pointer)],
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    assert_eq!(contained_rect(&dockspace), original);

    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(painted_pointer)],
    );
    let expected = translated_rect(original, press_origin, painted_pointer);
    assert!(matches!(
        dockspace
            .engine()
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

    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(release_pointer),
            pointer_button(release_pointer, false),
        ],
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    assert_eq!(contained_rect(&dockspace), original);

    run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let floating = dockspace
        .engine()
        .workspace()
        .contained_floating(FLOATING)
        .expect("floating remains");
    assert_eq!(floating.rect, expected);
    assert_eq!(floating.z_order, original_floating.z_order);
    assert_eq!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Idle
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
    warm(&context, &mut dockspace, &mut panes);

    let press_origin = contained_north_west_resize_point(&dockspace);
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the finite fixture edge is deliberately converted to egui's f32 input space"
    )]
    let current = Pos2::new(original.max().x() as f32, -40.0);

    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(press_origin),
            pointer_button(press_origin, true),
        ],
    );
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(current)],
    );
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(current)],
    );
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(current), pointer_button(current, false)],
    );
    assert_eq!(
        dockspace
            .engine()
            .workspace()
            .contained_floating(FLOATING)
            .expect("floating remains")
            .rect,
        original,
        "release is reduced only at the next complete frame boundary"
    );

    run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let resized = dockspace
        .engine()
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
        dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    );
}

#[test]
fn stale_projection_still_releases_active_contained_title_drag() {
    let context = Context::default();
    let original = LogicalRect::new(120.0, 80.0, 220.0, 150.0).expect("finite rect");
    let mut dockspace =
        Dockspace::builder("stale-contained-release", contained_workspace(original))
            .build()
            .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    warm(&context, &mut dockspace, &mut panes);

    let press_origin = contained_title_point(&dockspace);
    let current = press_origin + vec2(30.0, 20.0);
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(press_origin),
            pointer_button(press_origin, true),
        ],
    );
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(current)],
    );
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(current)],
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));

    let released = run_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        vec2(560.0, 360.0),
        vec![Event::PointerMoved(current), pointer_button(current, false)],
    );
    assert!(
        released
            .iter()
            .any(|observation| !observation.interactions_current)
    );
    assert!(
        released
            .last()
            .expect("release frame paints")
            .interactions_current
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));

    run_frame_with_size(
        &context,
        &mut dockspace,
        &mut panes,
        vec2(560.0, 360.0),
        Vec::new(),
    );
    assert_eq!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    );
}

#[test]
fn stale_projection_still_releases_active_split_resize() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("stale-resize-release", split_workspace())
        .build()
        .expect("facade must build");
    let mut panes = TestPanes::with_items([ITEM_A, ITEM_B]);
    warm(&context, &mut dockspace, &mut panes);

    let press_origin = splitter_center(&dockspace);
    let current = press_origin + vec2(60.0, 0.0);
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(press_origin),
            pointer_button(press_origin, true),
        ],
    );
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(current)],
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Resizing { .. }
    ));

    let released = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(current), pointer_button(current, false)],
    );
    assert!(
        released
            .iter()
            .any(|observation| !observation.interactions_current)
    );
    assert!(
        released
            .last()
            .expect("release frame paints")
            .interactions_current
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Resizing { .. }
    ));

    run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    );
}

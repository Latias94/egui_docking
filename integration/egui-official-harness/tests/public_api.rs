use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use dockspace::command::{CommandOutcome, Edge, MovePayload};
use dockspace::drop_guide::{DropGuideClusterId, DropGuideSlot};
use dockspace::drop_resolver::DropAffordanceTarget;
use dockspace::drop_target::DropTargetId;
use dockspace::geometry::{LogicalPoint, LogicalRect};
use dockspace::graph::{Axis, ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use dockspace::interaction::{
    InteractionDelivery, InteractionOutcome, InteractionStatus, PreviewResolutionStatus,
    PreviewVisual, WorkspaceDeliveryKind,
};
use dockspace::policy::DockPolicy;
use dockspace::presentation_hit::PresentationHitRegionKind;
use dockspace::presentation_observation::HostPresentationObservationOutcome;
use dockspace::scene::ContainedResizeDirection;
use dockspace::tab_strip::TabStripControlId;
use dockspace::{CloseDecision, ClosePlanLookup, ClosePlanPhase, ClosePlanTarget};
use egui::accesskit::{Action, ActionRequest, NodeId as AccessKitNodeId, Role, TreeId, TreeUpdate};
use egui::{
    Context, Event, Id, Key, Modifiers, MouseWheelUnit, Order, PointerButton, Pos2, RawInput, Rect,
    Sense, TouchPhase, Ui, ViewportId, vec2,
};
use egui_dockspace::HostFrameResponse;
use egui_dockspace::{
    Dockspace, DockspaceCapability, DockspaceUnavailableReason, EguiFrameScheduleKey,
    EguiPresentationResult, PaneView,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const ITEM: ItemId = ItemId::new(1);
const SECOND_ITEM: ItemId = ItemId::new(2);
const THIRD_ITEM: ItemId = ItemId::new(3);
const FOURTH_ITEM: ItemId = ItemId::new(4);
const FIFTH_ITEM: ItemId = ItemId::new(5);
const SIXTH_ITEM: ItemId = ItemId::new(6);
const FLOATING_ROOT: RootId = RootId::new(2);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const SECOND_FLOATING_ROOT: RootId = RootId::new(3);
const SECOND_FLOATING: FloatingPresentationId = FloatingPresentationId::new(2);

#[derive(Default)]
struct SmokePanes {
    paint_count: usize,
    overlay: Option<Rect>,
}

impl PaneView for SmokePanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        (item == ITEM).then(|| "Official egui pane".into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) {
        if item == ITEM {
            self.paint_count += 1;
            ui.label("Rendered through the public egui_dockspace facade");
            if let Some(rect) = self.overlay {
                egui::Area::new(Id::new("official-egui-foreign-overlay"))
                    .order(Order::Foreground)
                    .fixed_pos(rect.min)
                    .show(ui.ctx(), |ui| {
                        let _ = ui.allocate_response(rect.size(), Sense::click());
                    });
            }
        }
    }
}

struct MissingPanes;

impl PaneView for MissingPanes {
    fn title(&self, _item: ItemId) -> Option<egui::WidgetText> {
        None
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut Ui) {
        panic!("a missing pane must never invoke the application pane callback");
    }
}

#[derive(Default)]
struct NamedPanes {
    titles: BTreeMap<ItemId, String>,
    clicks: BTreeMap<ItemId, usize>,
    minimum_sizes: BTreeMap<ItemId, egui::Vec2>,
}

impl NamedPanes {
    fn with_items(items: impl IntoIterator<Item = ItemId>) -> Self {
        Self {
            titles: items
                .into_iter()
                .map(|item| (item, format!("Pane {}", item.get())))
                .collect(),
            clicks: BTreeMap::new(),
            minimum_sizes: BTreeMap::new(),
        }
    }

    fn click_count(&self, item: ItemId) -> usize {
        self.clicks.get(&item).copied().unwrap_or_default()
    }
}

impl PaneView for NamedPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        self.titles.get(&item).cloned().map(Into::into)
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) {
        let response = ui.interact(
            ui.max_rect(),
            ui.id().with(("named-pane", item)),
            Sense::click(),
        );
        if response.clicked() {
            *self.clicks.entry(item).or_default() += 1;
        }
    }

    fn minimum_size(&self, item: ItemId) -> egui::Vec2 {
        self.minimum_sizes.get(&item).copied().unwrap_or_default()
    }
}

#[derive(Default)]
struct EscapeTrackingPanes {
    observations: usize,
    consumptions: usize,
}

impl PaneView for EscapeTrackingPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(format!("Pane {}", item.get()).into())
    }

    fn ui(&mut self, _item: ItemId, ui: &mut Ui) {
        if ui.input(|input| input.key_pressed(Key::Escape)) {
            self.observations += 1;
        }
        if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape)) {
            self.consumptions += 1;
        }
    }
}

fn workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM]));
    builder.set_root(ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.build().expect("the smoke workspace is valid")
}

fn two_tab_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM, SECOND_ITEM]));
    builder.set_root(ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.build().expect("the two-tab workspace is valid")
}

fn tab_stack_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM, SECOND_ITEM, THIRD_ITEM]));
    builder.set_root(ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.build().expect("the tab-stack workspace is valid")
}

fn tab_list_workspace(items: impl IntoIterator<Item = ItemId>) -> Workspace {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs(items));
    builder.set_root(ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.build().expect("the tab-list workspace is valid")
}

fn popup_underlay_workspace(
    popup_items: impl IntoIterator<Item = ItemId>,
    underlay: ItemId,
    floating_underlay: bool,
) -> Workspace {
    let mut builder = Workspace::builder();
    let popup = builder.insert_node(Node::tabs(popup_items));
    let underlay_tabs = builder.insert_node(Node::tabs([underlay]));
    if floating_underlay {
        builder.set_root(ROOT, RootRecord::new(popup));
        builder.set_root(FLOATING_ROOT, RootRecord::new(underlay_tabs));
        builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
        builder.set_contained_floating(
            FLOATING,
            ContainedFloating::new(
                FLOATING_ROOT,
                LogicalRect::new(70.0, 42.0, 210.0, 170.0)
                    .expect("the contained underlay rectangle is valid"),
            ),
        );
        builder
            .attach_contained(SURFACE, FLOATING)
            .expect("the fixture surface exists");
    } else {
        let split = builder.insert_node(
            Node::equal_split(Axis::Horizontal, [popup, underlay_tabs])
                .expect("the tiled underlay split is valid"),
        );
        builder.set_root(ROOT, RootRecord::new(split));
        builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    }
    builder
        .build()
        .expect("the popup underlay workspace is valid")
}

fn cross_presentation_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let main = builder.insert_node(Node::tabs_with_selection([ITEM, SECOND_ITEM], Some(ITEM)));
    let floating = builder.insert_node(Node::tabs_with_selection(
        [THIRD_ITEM, FOURTH_ITEM],
        Some(FOURTH_ITEM),
    ));
    let second_floating = builder.insert_node(Node::tabs_with_selection(
        [FIFTH_ITEM, SIXTH_ITEM],
        Some(SIXTH_ITEM),
    ));
    builder.set_root(ROOT, RootRecord::new(main));
    builder.set_root(FLOATING_ROOT, RootRecord::new(floating));
    builder.set_root(SECOND_FLOATING_ROOT, RootRecord::new(second_floating));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.set_contained_floating(
        FLOATING,
        ContainedFloating::new(
            FLOATING_ROOT,
            LogicalRect::new(40.0, 180.0, 340.0, 240.0).expect("the first contained rect is valid"),
        ),
    );
    builder.set_contained_floating(
        SECOND_FLOATING,
        ContainedFloating::new(
            SECOND_FLOATING_ROOT,
            LogicalRect::new(430.0, 180.0, 340.0, 240.0)
                .expect("the second contained rect is valid"),
        ),
    );
    builder
        .attach_contained(SURFACE, FLOATING)
        .expect("the surface exists");
    builder
        .attach_contained(SURFACE, SECOND_FLOATING)
        .expect("the surface exists");
    builder
        .build()
        .expect("the cross-presentation workspace is valid")
}

fn split_workspace() -> (Workspace, NodeId, NodeId, NodeId) {
    let mut builder = Workspace::builder();
    let left_tabs = builder.insert_node(Node::tabs([ITEM]));
    let right_tabs = builder.insert_node(Node::tabs([SECOND_ITEM]));
    let split = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [left_tabs, right_tabs])
            .expect("the split fixture is valid"),
    );
    builder.set_root(ROOT, RootRecord::new(split));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    (
        builder.build().expect("the split workspace is valid"),
        left_tabs,
        right_tabs,
        split,
    )
}

fn junction_workspace() -> (Workspace, NodeId, NodeId) {
    let mut builder = Workspace::builder();
    let left_tabs = builder.insert_node(Node::tabs([ITEM]));
    let top_right_tabs = builder.insert_node(Node::tabs([SECOND_ITEM]));
    let bottom_right_tabs = builder.insert_node(Node::tabs([ItemId::new(3)]));
    let vertical = builder.insert_node(
        Node::equal_split(Axis::Vertical, [top_right_tabs, bottom_right_tabs])
            .expect("the vertical split fixture is valid"),
    );
    let horizontal = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [left_tabs, vertical])
            .expect("the horizontal split fixture is valid"),
    );
    builder.set_root(ROOT, RootRecord::new(horizontal));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    (
        builder.build().expect("the junction workspace is valid"),
        horizontal,
        vertical,
    )
}

fn contained_workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let main_tabs = builder.insert_node(Node::tabs([ITEM]));
    let floating_tabs = builder.insert_node(Node::tabs([SECOND_ITEM]));
    builder.set_root(ROOT, RootRecord::new(main_tabs));
    builder.set_root(FLOATING_ROOT, RootRecord::new(floating_tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.set_contained_floating(
        FLOATING,
        ContainedFloating::new(
            FLOATING_ROOT,
            LogicalRect::new(120.0, 100.0, 420.0, 360.0)
                .expect("the contained fixture rectangle is valid"),
        ),
    );
    builder
        .attach_contained(SURFACE, FLOATING)
        .expect("the fixture surface exists");
    builder.build().expect("the contained workspace is valid")
}

fn contained_split_workspace() -> (Workspace, NodeId) {
    let mut builder = Workspace::builder();
    let left_tabs = builder.insert_node(Node::tabs([ITEM]));
    let right_tabs = builder.insert_node(Node::tabs([SECOND_ITEM]));
    let split = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [left_tabs, right_tabs])
            .expect("the contained split fixture is valid"),
    );
    let floating_tabs = builder.insert_node(Node::tabs_with_selection(
        [THIRD_ITEM, FOURTH_ITEM],
        Some(FOURTH_ITEM),
    ));
    builder.set_root(ROOT, RootRecord::new(split));
    builder.set_root(FLOATING_ROOT, RootRecord::new(floating_tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.set_contained_floating(
        FLOATING,
        ContainedFloating::new(
            FLOATING_ROOT,
            LogicalRect::new(520.0, 80.0, 240.0, 190.0)
                .expect("the contained split rectangle is valid"),
        ),
    );
    builder
        .attach_contained(SURFACE, FLOATING)
        .expect("the fixture surface exists");
    (
        builder
            .build()
            .expect("the contained split workspace is valid"),
        left_tabs,
    )
}

fn contained_subtree_workspace() -> (Workspace, NodeId, NodeId, NodeId, NodeId) {
    let mut builder = Workspace::builder();
    let main_tabs = builder.insert_node(Node::tabs([ITEM]));
    let source_left = builder.insert_node(Node::tabs_with_selection(
        [SECOND_ITEM, FOURTH_ITEM],
        Some(FOURTH_ITEM),
    ));
    let source_right = builder.insert_node(Node::tabs([THIRD_ITEM]));
    let source_root = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [source_left, source_right])
            .expect("the contained subtree fixture is valid"),
    );
    builder.set_root(ROOT, RootRecord::new(main_tabs));
    builder.set_root(FLOATING_ROOT, RootRecord::new(source_root));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.set_contained_floating(
        FLOATING,
        ContainedFloating::new(
            FLOATING_ROOT,
            LogicalRect::new(120.0, 80.0, 340.0, 230.0)
                .expect("the contained subtree rectangle is valid"),
        ),
    );
    builder
        .attach_contained(SURFACE, FLOATING)
        .expect("the fixture surface exists");
    (
        builder
            .build()
            .expect("the contained subtree workspace is valid"),
        main_tabs,
        source_root,
        source_left,
        source_right,
    )
}

fn contained_resize_center(dockspace: &Dockspace, direction: ContainedResizeDirection) -> Pos2 {
    let record = dockspace
        .engine()
        .interaction_projection(SURFACE)
        .expect("the surface has one acknowledged interaction projection")
        .plan()
        .contained_record(FLOATING)
        .expect("the contained floating has a compiled record")
        .resize()
        .iter()
        .find(|record| record.direction() == direction)
        .expect("the requested resize direction is present");
    let rect = record.hit().rect();
    Pos2::new(
        ((rect.min().x() + rect.max().x()) * 0.5) as f32,
        ((rect.min().y() + rect.max().y()) * 0.5) as f32,
    )
}

fn contained_title_center(dockspace: &Dockspace) -> Pos2 {
    let rect = dockspace
        .engine()
        .interaction_projection(SURFACE)
        .expect("the surface has one acknowledged interaction projection")
        .plan()
        .contained_record(FLOATING)
        .expect("the contained floating has a compiled record")
        .title_drag_hit()
        .rect();
    Pos2::new(
        ((rect.min().x() + rect.max().x()) * 0.5) as f32,
        ((rect.min().y() + rect.max().y()) * 0.5) as f32,
    )
}

fn contained_close_center(dockspace: &Dockspace) -> Pos2 {
    let rect = dockspace
        .engine()
        .interaction_projection(SURFACE)
        .expect("the surface has one acknowledged interaction projection")
        .plan()
        .contained_record(FLOATING)
        .expect("the contained floating has a compiled record")
        .close_bounds()
        .expect("the default policy exposes contained close chrome");
    Pos2::new(
        ((rect.min().x() + rect.max().x()) * 0.5) as f32,
        ((rect.min().y() + rect.max().y()) * 0.5) as f32,
    )
}

fn contained_guide_center(
    dockspace: &Dockspace,
    target_tabs: NodeId,
    slot: DropGuideSlot,
) -> (dockspace::drop_target::DropTargetId, Pos2) {
    let target = dockspace
        .engine()
        .interaction_projection(SURFACE)
        .expect("the surface has one acknowledged interaction projection")
        .plan()
        .drop_guide_clusters()
        .iter()
        .find(|cluster| cluster.id() == DropGuideClusterId::inner(SURFACE, ROOT, target_tabs))
        .and_then(|cluster| cluster.target(slot))
        .expect("the requested contained-title guide exists");
    let id = target.id();
    let rect = target.target().region().rect();
    (
        id,
        Pos2::new(
            ((rect.min().x() + rect.max().x()) * 0.5) as f32,
            ((rect.min().y() + rect.max().y()) * 0.5) as f32,
        ),
    )
}

fn contained_activation_only_point(dockspace: &Dockspace, target_tabs: NodeId) -> Pos2 {
    let projection = dockspace
        .engine()
        .interaction_projection(SURFACE)
        .expect("the surface has one acknowledged interaction projection");
    let plan = projection.plan();
    let cluster = plan
        .drop_guide_clusters()
        .iter()
        .find(|cluster| cluster.id() == DropGuideClusterId::inner(SURFACE, ROOT, target_tabs))
        .expect("the target leaf publishes its inner guide cluster");
    let activation = cluster.activation().rect();
    let source = contained_title_center(dockspace);
    let floating = dockspace
        .engine()
        .workspace()
        .contained_floating(FLOATING)
        .expect("the contained presentation remains present")
        .rect;
    let surface = plan.bounds();
    let fractions = [0.2, 0.35, 0.65, 0.8];
    let point = fractions
        .iter()
        .copied()
        .flat_map(|x| fractions.iter().copied().map(move |y| (x, y)))
        .filter_map(|(x, y)| {
            LogicalPoint::new(
                activation.x() + activation.width() * x,
                activation.y() + activation.height() * y,
            )
            .ok()
        })
        .find(|point| {
            let translated = LogicalRect::new(
                floating.x() + point.x() - f64::from(source.x),
                floating.y() + point.y() - f64::from(source.y),
                floating.width(),
                floating.height(),
            )
            .ok();
            cluster.activation().contains(*point)
                && translated.is_some_and(|translated| {
                    translated.x() >= surface.x()
                        && translated.y() >= surface.y()
                        && translated.max().x() <= surface.max().x()
                        && translated.max().y() <= surface.max().y()
                })
                && cluster
                    .targets()
                    .all(|(_, target)| !target.target().region().contains(*point))
                && plan
                    .drop_guide_clusters()
                    .iter()
                    .flat_map(|cluster| cluster.targets().map(|(_, target)| target))
                    .all(|target| !target.target().region().contains(*point))
                && plan
                    .drop_targets()
                    .iter()
                    .all(|target| !target.region().contains(*point))
        })
        .expect("the inner activation contains an in-bounds non-target point");
    Pos2::new(point.x() as f32, point.y() as f32)
}

fn inner_activation_only_point(
    dockspace: &Dockspace,
    root: RootId,
    target_tabs: NodeId,
    index: usize,
) -> Pos2 {
    let plan = current_plan(dockspace);
    let cluster = plan
        .drop_guide_clusters()
        .iter()
        .find(|cluster| cluster.id() == DropGuideClusterId::inner(SURFACE, root, target_tabs))
        .expect("the target leaf publishes its inner guide cluster");
    let activation = cluster.activation().rect();
    let fractions = [0.2, 0.35, 0.65, 0.8];
    let point = fractions
        .iter()
        .copied()
        .flat_map(|x| fractions.iter().copied().map(move |y| (x, y)))
        .filter_map(|(x, y)| {
            LogicalPoint::new(
                activation.x() + activation.width() * x,
                activation.y() + activation.height() * y,
            )
            .ok()
        })
        .filter(|point| {
            cluster.activation().contains(*point)
                && plan
                    .drop_guide_clusters()
                    .iter()
                    .flat_map(|cluster| cluster.targets().map(|(_, target)| target))
                    .all(|target| !target.target().region().contains(*point))
                && plan
                    .drop_targets()
                    .iter()
                    .all(|target| !target.region().contains(*point))
        })
        .nth(index)
        .expect("the inner activation contains the requested non-target point");
    Pos2::new(point.x() as f32, point.y() as f32)
}

fn tab_items(dockspace: &Dockspace, node: NodeId) -> (&[ItemId], Option<ItemId>) {
    match dockspace.engine().workspace().node(node) {
        Some(Node::Tabs { items, selected }) => (items, *selected),
        Some(Node::Split { .. }) | None => panic!("the matrix child must be a tabs node"),
    }
}

fn tab_drag_point(dockspace: &Dockspace, root: RootId, item: ItemId) -> Pos2 {
    let rect = dockspace
        .engine()
        .interaction_projection(SURFACE)
        .expect("the surface has one acknowledged interaction projection")
        .plan()
        .tab_records()
        .iter()
        .find(|tab| tab.id().root == root && tab.id().item == item)
        .map(|tab| tab.drag_hit().rect())
        .expect("the requested tab exposes a drag receiver");
    Pos2::new(
        (rect.min().x() + 8.0) as f32,
        ((rect.min().y() + rect.max().y()) * 0.5) as f32,
    )
}

fn tab_group_grip_center(dockspace: &Dockspace, root: RootId) -> (Pos2, NodeId) {
    let bar = dockspace
        .engine()
        .interaction_projection(SURFACE)
        .expect("the surface has one acknowledged interaction projection")
        .plan()
        .tab_bar_records()
        .iter()
        .find(|bar| bar.id().root == root)
        .expect("the requested root exposes a tab bar");
    let rect = bar
        .group_drag()
        .expect("the requested tab bar exposes a group drag receiver")
        .grip_bounds();
    (
        Pos2::new(
            ((rect.min().x() + rect.max().x()) * 0.5) as f32,
            ((rect.min().y() + rect.max().y()) * 0.5) as f32,
        ),
        bar.id().tabs,
    )
}

fn tab_gap_target(dockspace: &Dockspace, tabs: NodeId, index: usize) -> (DropTargetId, Pos2) {
    let target = dockspace
        .engine()
        .interaction_projection(SURFACE)
        .expect("the surface has one acknowledged interaction projection")
        .plan()
        .drop_targets()
        .iter()
        .find(|target| {
            target.id()
                == (DropTargetId::TabGap {
                    surface: SURFACE,
                    root: ROOT,
                    tabs,
                    index,
                })
        })
        .expect("the requested exact tab gap exists");
    let rect = target.region().rect();
    (
        target.id(),
        Pos2::new(
            ((rect.min().x() + rect.max().x()) * 0.5) as f32,
            ((rect.min().y() + rect.max().y()) * 0.5) as f32,
        ),
    )
}

fn center_guide_target(dockspace: &Dockspace, root: RootId) -> (DropTargetId, Pos2) {
    let tabs = dockspace
        .engine()
        .workspace()
        .root(root)
        .expect("the requested root remains present")
        .node;
    let target = dockspace
        .engine()
        .interaction_projection(SURFACE)
        .expect("the surface has one acknowledged interaction projection")
        .plan()
        .drop_guide_clusters()
        .iter()
        .find(|cluster| cluster.id() == DropGuideClusterId::inner(SURFACE, root, tabs))
        .and_then(|cluster| cluster.target(DropGuideSlot::Center))
        .expect("the requested root exposes an exact center guide")
        .target();
    let rect = target.region().rect();
    (
        target.id(),
        Pos2::new(
            ((rect.min().x() + rect.max().x()) * 0.5) as f32,
            ((rect.min().y() + rect.max().y()) * 0.5) as f32,
        ),
    )
}

fn root_tab_items(dockspace: &Dockspace, root: RootId) -> (&[ItemId], Option<ItemId>) {
    let node = dockspace
        .engine()
        .workspace()
        .root(root)
        .expect("the requested root remains present")
        .node;
    tab_items(dockspace, node)
}

fn subtree_items(workspace: &Workspace, root: NodeId) -> Vec<ItemId> {
    let mut stack = vec![root];
    let mut items = Vec::new();
    while let Some(node) = stack.pop() {
        match workspace.node(node).expect("the reachable node exists") {
            Node::Tabs {
                items: tab_items, ..
            } => items.extend(tab_items),
            Node::Split { children, .. } => stack.extend(children.iter().rev().copied()),
        }
    }
    items
}

fn reachable_nodes(workspace: &Workspace, root: NodeId) -> BTreeSet<NodeId> {
    let mut stack = vec![root];
    let mut reachable = BTreeSet::new();
    while let Some(node) = stack.pop() {
        if !reachable.insert(node) {
            continue;
        }
        if let Some(Node::Split { children, .. }) = workspace.node(node) {
            stack.extend(children.iter().rev().copied());
        }
    }
    reachable
}

fn deliver_official_drag(
    dockspace: &mut Dockspace,
    context: &Context,
    panes: &mut dyn PaneView,
    sequence: &mut u64,
    source: Pos2,
    target_id: DropTargetId,
    target: Pos2,
) -> HostFrameResponse {
    *sequence += 1;
    run_outer_frame(
        dockspace,
        context,
        panes,
        *sequence,
        vec![
            Event::PointerMoved(source),
            Event::PointerButton {
                pos: source,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    assert!(
        matches!(
            dockspace.engine().interaction().status(),
            InteractionStatus::Armed { .. } | InteractionStatus::Dragging { .. }
        ),
        "press at {source:?} in frame {} did not start a drag: {:?}",
        *sequence,
        dockspace.engine().interaction().status()
    );
    for _ in 0..4 {
        *sequence += 1;
        run_outer_frame(
            dockspace,
            context,
            panes,
            *sequence,
            vec![Event::PointerMoved(target)],
        );
    }
    let preview = dockspace
        .engine()
        .interaction()
        .preview()
        .expect("the exact target publishes one preview");
    assert!(matches!(
        preview.visual(),
        PreviewVisual::Dock { target, .. } if *target == target_id
    ));
    *sequence += 1;
    run_outer_frame(dockspace, context, panes, *sequence, Vec::new());
    *sequence += 1;
    run_outer_frame(
        dockspace,
        context,
        panes,
        *sequence,
        vec![
            Event::PointerMoved(target),
            Event::PointerButton {
                pos: target,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ],
    )
}

fn assert_contained_guide_topology(dockspace: &Dockspace, slot: DropGuideSlot) {
    let workspace = dockspace.engine().workspace();
    let main = workspace.root(ROOT).expect("the main root remains present");
    let Some(Node::Split {
        axis: Axis::Horizontal,
        children,
        ..
    }) = workspace.node(main.node)
    else {
        panic!("the main root remains a horizontal split");
    };
    match slot {
        DropGuideSlot::Center => {
            assert_eq!(children.len(), 2);
            assert_eq!(
                tab_items(dockspace, children[0]),
                (&[ITEM, THIRD_ITEM, FOURTH_ITEM][..], Some(FOURTH_ITEM))
            );
            assert_eq!(
                tab_items(dockspace, children[1]),
                (&[SECOND_ITEM][..], Some(SECOND_ITEM))
            );
        }
        DropGuideSlot::Edge(edge @ (Edge::Left | Edge::Right)) => {
            let actual = children
                .iter()
                .map(|node| tab_items(dockspace, *node))
                .collect::<Vec<_>>();
            let expected = match edge {
                Edge::Left => vec![
                    (&[THIRD_ITEM, FOURTH_ITEM][..], Some(FOURTH_ITEM)),
                    (&[ITEM][..], Some(ITEM)),
                    (&[SECOND_ITEM][..], Some(SECOND_ITEM)),
                ],
                Edge::Right => vec![
                    (&[ITEM][..], Some(ITEM)),
                    (&[THIRD_ITEM, FOURTH_ITEM][..], Some(FOURTH_ITEM)),
                    (&[SECOND_ITEM][..], Some(SECOND_ITEM)),
                ],
                Edge::Top | Edge::Bottom => unreachable!("matched horizontal edges only"),
            };
            assert_eq!(actual, expected);
        }
        DropGuideSlot::Edge(edge @ (Edge::Top | Edge::Bottom)) => {
            assert_eq!(children.len(), 2);
            assert_eq!(
                tab_items(dockspace, children[1]),
                (&[SECOND_ITEM][..], Some(SECOND_ITEM))
            );
            let Some(Node::Split {
                axis: Axis::Vertical,
                children,
                ..
            }) = workspace.node(children[0])
            else {
                panic!("a vertical guide wraps the target tabs in a vertical split");
            };
            let actual = children
                .iter()
                .map(|node| tab_items(dockspace, *node))
                .collect::<Vec<_>>();
            let expected = match edge {
                Edge::Top => vec![
                    (&[THIRD_ITEM, FOURTH_ITEM][..], Some(FOURTH_ITEM)),
                    (&[ITEM][..], Some(ITEM)),
                ],
                Edge::Bottom => vec![
                    (&[ITEM][..], Some(ITEM)),
                    (&[THIRD_ITEM, FOURTH_ITEM][..], Some(FOURTH_ITEM)),
                ],
                Edge::Left | Edge::Right => unreachable!("matched vertical edges only"),
            };
            assert_eq!(actual, expected);
        }
    }
}

fn assert_subtree_redock_topology(
    dockspace: &Dockspace,
    original: &Workspace,
    edge: Edge,
    source_left: NodeId,
    source_right: NodeId,
) {
    let workspace = dockspace.engine().workspace();
    assert!(workspace.contained_floating(FLOATING).is_none());
    assert!(workspace.root(FLOATING_ROOT).is_none());
    let surface = workspace
        .surface(SURFACE)
        .expect("the logical surface remains present");
    assert_eq!(surface.main_root, Some(ROOT));
    assert!(surface.contained.is_empty());
    assert_eq!(workspace.item_multiset(), original.item_multiset());
    assert!(matches!(
        workspace.node(source_left),
        Some(Node::Tabs { items, selected })
            if items == &[SECOND_ITEM, FOURTH_ITEM] && *selected == Some(FOURTH_ITEM)
    ));
    assert!(matches!(
        workspace.node(source_right),
        Some(Node::Tabs { items, selected })
            if items == &[THIRD_ITEM] && *selected == Some(THIRD_ITEM)
    ));
    let main_node = workspace
        .root(ROOT)
        .expect("the main root remains present")
        .node;
    let reachable = reachable_nodes(workspace, main_node);
    assert!(reachable.contains(&source_left));
    assert!(reachable.contains(&source_right));
    let Some(Node::Split { axis, children, .. }) = workspace.node(main_node) else {
        panic!("edge docking produces a split main root");
    };
    assert_eq!(
        *axis,
        match edge {
            Edge::Left | Edge::Right => Axis::Horizontal,
            Edge::Top | Edge::Bottom => Axis::Vertical,
        }
    );
    let branch_items = children
        .iter()
        .map(|child| subtree_items(workspace, *child))
        .collect::<Vec<_>>();
    assert_eq!(
        branch_items,
        match edge {
            Edge::Left => vec![vec![SECOND_ITEM, FOURTH_ITEM], vec![THIRD_ITEM], vec![ITEM],],
            Edge::Right => vec![vec![ITEM], vec![SECOND_ITEM, FOURTH_ITEM], vec![THIRD_ITEM],],
            Edge::Top => vec![vec![SECOND_ITEM, FOURTH_ITEM, THIRD_ITEM], vec![ITEM]],
            Edge::Bottom => vec![vec![ITEM], vec![SECOND_ITEM, FOURTH_ITEM, THIRD_ITEM]],
        }
    );
    assert_eq!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    );
}

fn split_weights(dockspace: &Dockspace, split: NodeId) -> Vec<f32> {
    match dockspace.engine().workspace().node(split) {
        Some(Node::Split { weights, .. }) => weights.iter().map(|weight| weight.get()).collect(),
        Some(Node::Tabs { .. }) | None => panic!("the fixture split must remain present"),
    }
}

fn active_resize_weights(dockspace: &Dockspace) -> BTreeMap<NodeId, Vec<f32>> {
    dockspace
        .engine()
        .interaction()
        .active_resize_view()
        .expect("splitter resize remains active")
        .updates()
        .iter()
        .map(|update| {
            (
                update.split().node(),
                update.weights().iter().map(|weight| weight.get()).collect(),
            )
        })
        .collect()
}

fn splitter_center(dockspace: &Dockspace, split: NodeId) -> Pos2 {
    let record = dockspace
        .engine()
        .interaction_projection(SURFACE)
        .expect("the surface has one acknowledged interaction projection")
        .plan()
        .splitter_records()
        .iter()
        .find(|record| record.id().split == split)
        .expect("the split exposes one rendered splitter");
    let rect = record.hit().rect();
    Pos2::new(
        ((rect.min().x() + rect.max().x()) * 0.5) as f32,
        ((rect.min().y() + rect.max().y()) * 0.5) as f32,
    )
}

fn splitter_junction_center(dockspace: &Dockspace) -> Pos2 {
    let record = dockspace
        .engine()
        .interaction_projection(SURFACE)
        .expect("the surface has one acknowledged interaction projection")
        .plan()
        .splitter_junction_records()
        .first()
        .expect("the nested split exposes one rendered junction");
    let rect = record.hit().rect();
    Pos2::new(
        ((rect.min().x() + rect.max().x()) * 0.5) as f32,
        ((rect.min().y() + rect.max().y()) * 0.5) as f32,
    )
}

fn top_guide_drag_geometry(dockspace: &Dockspace, left_tabs: NodeId) -> (Pos2, Pos2, Rect) {
    let projection = dockspace
        .engine()
        .interaction_projection(SURFACE)
        .expect("the surface has one acknowledged interaction projection");
    let source = projection
        .plan()
        .tab_records()
        .iter()
        .find(|tab| tab.id().item == SECOND_ITEM)
        .map(|tab| tab.drag_hit().rect())
        .map(|rect| {
            Pos2::new(
                (rect.min().x() + 8.0) as f32,
                ((rect.min().y() + rect.max().y()) * 0.5) as f32,
            )
        })
        .expect("the source tab exposes a drag receiver");
    let target = projection
        .plan()
        .drop_guide_clusters()
        .iter()
        .find(|cluster| cluster.id() == DropGuideClusterId::inner(SURFACE, ROOT, left_tabs))
        .and_then(|cluster| cluster.target(DropGuideSlot::Edge(Edge::Top)))
        .map(|target| target.target().region().rect())
        .expect("the left leaf exposes an exact top guide target");
    let target_rect = Rect::from_min_max(
        Pos2::new(target.min().x() as f32, target.min().y() as f32),
        Pos2::new(target.max().x() as f32, target.max().y() as f32),
    );
    (source, target_rect.center(), target_rect)
}

fn run_outer_frame(
    dockspace: &mut Dockspace,
    context: &Context,
    panes: &mut dyn PaneView,
    sequence: u64,
    events: Vec<Event>,
) -> HostFrameResponse {
    run_outer_frame_with_size(
        dockspace,
        context,
        panes,
        sequence,
        vec2(800.0, 600.0),
        events,
    )
}

fn run_outer_frame_with_size(
    dockspace: &mut Dockspace,
    context: &Context,
    panes: &mut dyn PaneView,
    sequence: u64,
    size: egui::Vec2,
    events: Vec<Event>,
) -> HostFrameResponse {
    let mut frame = dockspace
        .begin_outer_frame(EguiFrameScheduleKey::new(sequence, 0))
        .expect("outer frame begins");
    frame
        .run_surface(
            SURFACE,
            context,
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
                events,
                ..RawInput::default()
            },
            panes,
        )
        .expect("outer host paints the surface");
    let (host, outputs) = frame.finish().expect("outer frame commits").into_parts();
    for output in outputs {
        output.settle_with(|_, _| EguiPresentationResult::Presented);
    }
    host
}

fn run_outer_frame_with_accesskit(
    dockspace: &mut Dockspace,
    context: &Context,
    panes: &mut dyn PaneView,
    sequence: u64,
    size: egui::Vec2,
    events: Vec<Event>,
) -> (HostFrameResponse, TreeUpdate) {
    let mut frame = dockspace
        .begin_outer_frame(EguiFrameScheduleKey::new(sequence, 0))
        .expect("outer frame begins");
    frame
        .run_surface(
            SURFACE,
            context,
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
                events,
                ..RawInput::default()
            },
            panes,
        )
        .expect("outer host paints the surface");
    let (host, outputs) = frame.finish().expect("outer frame commits").into_parts();
    let mut tree = None;
    for output in outputs {
        output.settle_with(|surface, output| {
            if surface == SURFACE {
                tree = output.platform_output.accesskit_update;
            }
            EguiPresentationResult::Presented
        });
    }
    (
        host,
        tree.expect("AccessKit output is enabled for the outer surface"),
    )
}

#[test]
fn official_egui_reports_the_production_pane_focus_authority_gap() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("official-egui-pane-focus-authority", workspace())
        .build()
        .expect("the public facade builds");
    let mut panes = SmokePanes::default();
    let mut response = None;

    for sequence in 1..=3 {
        response = Some(run_outer_frame(
            &mut dockspace,
            &context,
            &mut panes,
            sequence,
            Vec::new(),
        ));
    }

    assert_eq!(
        response
            .expect("the stable outer frame publishes a response")
            .surface(SURFACE)
            .and_then(|surface| surface.paint())
            .expect("the stable surface paints")
            .pane_focus_capability(),
        DockspaceCapability::Unavailable(DockspaceUnavailableReason::PaneFocusBindingUnavailable,),
        "production focus reporting must run the typed authority gate",
    );
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

fn accesskit_action(target_node: AccessKitNodeId, action: Action) -> Event {
    Event::AccessKitActionRequest(ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node,
        data: None,
    })
}

fn current_plan(dockspace: &Dockspace) -> &dockspace::scene::PresentationPlan {
    dockspace
        .engine()
        .interaction_projection(SURFACE)
        .expect("the surface has an acknowledged interaction projection")
        .plan()
}

fn overflow_control_center(dockspace: &Dockspace) -> Pos2 {
    let rect = current_plan(dockspace)
        .tab_strip_control_records()
        .iter()
        .find(|record| matches!(record.id(), TabStripControlId::TabListMenu(_)))
        .map(|record| record.bounds())
        .expect("the narrow tab strip exposes an overflow control");
    Pos2::new(
        ((rect.min().x() + rect.max().x()) * 0.5) as f32,
        ((rect.min().y() + rect.max().y()) * 0.5) as f32,
    )
}

fn open_overflow_menu(
    dockspace: &mut Dockspace,
    context: &Context,
    panes: &mut dyn PaneView,
    size: egui::Vec2,
) -> u64 {
    for sequence in 1..=3 {
        run_outer_frame_with_size(dockspace, context, panes, sequence, size, Vec::new());
    }
    let control = overflow_control_center(dockspace);
    run_outer_frame_with_size(
        dockspace,
        context,
        panes,
        4,
        size,
        vec![
            Event::PointerMoved(control),
            Event::PointerButton {
                pos: control,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    run_outer_frame_with_size(
        dockspace,
        context,
        panes,
        5,
        size,
        vec![
            Event::PointerMoved(control),
            Event::PointerButton {
                pos: control,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    let mut observed = Vec::new();
    for sequence in 6..=12 {
        let host = run_outer_frame_with_size(dockspace, context, panes, sequence, size, Vec::new());
        let paint = host.surface(SURFACE).and_then(|surface| surface.paint());
        let interactions_current = paint.is_some_and(|paint| paint.interactions_current());
        let menu_count = dockspace
            .engine()
            .interaction_projection(SURFACE)
            .map(|projection| projection.plan().tab_list_menu_records().len())
            .unwrap_or_default();
        observed.push((
            sequence,
            interactions_current,
            paint.map(|paint| paint.surface_status()),
            menu_count,
        ));
        if interactions_current && menu_count == 1 {
            return sequence + 1;
        }
    }
    panic!(
        "the presented overflow menu did not become interactive within seven host frames: {observed:?}"
    );
}

fn selected_item(dockspace: &Dockspace) -> Option<ItemId> {
    dockspace
        .engine()
        .workspace()
        .root(ROOT)
        .and_then(|root| dockspace.engine().workspace().node(root.node))
        .and_then(|node| match node {
            Node::Tabs { selected, .. } => *selected,
            Node::Split { .. } => None,
        })
}

fn selected_in_group_containing(dockspace: &Dockspace, item: ItemId) -> Option<ItemId> {
    dockspace
        .engine()
        .workspace()
        .nodes()
        .find_map(|(_, node)| match node {
            Node::Tabs { items, selected } if items.contains(&item) => Some(*selected),
            Node::Tabs { .. } | Node::Split { .. } => None,
        })
        .flatten()
}

fn egui_rect(rect: LogicalRect) -> Rect {
    Rect::from_min_max(
        Pos2::new(rect.min().x() as f32, rect.min().y() as f32),
        Pos2::new(rect.max().x() as f32, rect.max().y() as f32),
    )
}

#[test]
fn official_egui_builds_and_paints_the_public_single_surface_facade() {
    let _native_options = eframe::NativeOptions::default();
    let context = Context::default();
    let mut dockspace = Dockspace::builder("official-egui-harness", workspace())
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = SmokePanes::default();

    for _ in 0..4 {
        let _ = context.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))),
                ..RawInput::default()
            },
            |ui| {
                dockspace
                    .show_single_surface(SURFACE, ui, &mut panes)
                    .expect("the public facade advances an official-egui frame");
            },
        );
    }

    assert!(
        panes.paint_count > 0,
        "the selected pane must be rendered through the public facade"
    );
}

#[test]
fn official_egui_can_drive_the_outer_presentation_protocol() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("official-egui-outer-host", workspace())
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = SmokePanes::default();

    let mut bootstrap = dockspace
        .begin_outer_frame(EguiFrameScheduleKey::new(1, 0))
        .expect("outer frame begins");
    let _ = bootstrap
        .run_surface(
            SURFACE,
            &context,
            RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))),
                ..RawInput::default()
            },
            &mut panes,
        )
        .expect("outer host owns and confirms the surface run");
    let (_, outputs) = bootstrap
        .finish()
        .expect("bootstrap frame commits")
        .into_parts();
    assert!(
        outputs
            .iter()
            .all(|output| !output.has_presentation_obligation()),
        "the prepared-only bootstrap paint must not create a presentation obligation",
    );

    let mut first = dockspace
        .begin_outer_frame(EguiFrameScheduleKey::new(2, 0))
        .expect("first ready outer frame begins");
    let _ = first
        .run_surface(
            SURFACE,
            &context,
            RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))),
                ..RawInput::default()
            },
            &mut panes,
        )
        .expect("first ready outer host owns and confirms the surface run");
    let (_, pending) = first
        .finish()
        .expect("first ready outer frame commits")
        .into_parts();
    pending
        .into_iter()
        .next()
        .expect("painted output has a settlement obligation")
        .settle_with(|_, _| EguiPresentationResult::Presented);

    let mut second = dockspace
        .begin_outer_frame(EguiFrameScheduleKey::new(3, 0))
        .expect("next outer frame begins");
    let _ = second
        .run_surface(
            SURFACE,
            &context,
            RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))),
                ..RawInput::default()
            },
            &mut panes,
        )
        .expect("next outer host owns and confirms the surface run");
    let (host, pending) = second
        .finish()
        .expect("next outer frame commits")
        .into_parts();
    assert!(
        host.transition()
            .presentation_observations()
            .iter()
            .any(|outcome| matches!(outcome, HostPresentationObservationOutcome::Retired { .. }))
    );
    pending
        .into_iter()
        .next()
        .expect("next output has a settlement obligation")
        .settle_with(|_, _| EguiPresentationResult::Dropped);
}

#[test]
fn official_egui_outer_host_selects_a_tab_through_the_pointer_journal() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("official-egui-pointer-journal", two_tab_workspace())
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = SmokePanes::default();

    run_outer_frame(&mut dockspace, &context, &mut panes, 1, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 2, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 3, Vec::new());

    let second_tab_center = dockspace
        .engine()
        .scene()
        .surface(SURFACE)
        .and_then(|surface| surface.ready())
        .and_then(|ready| {
            ready
                .candidate()
                .hit_manifest()
                .regions()
                .iter()
                .find(|region| {
                    matches!(
                        region.id().kind(),
                        PresentationHitRegionKind::TabBody(tab) if tab.item == SECOND_ITEM
                    )
                })
                .copied()
        })
        .map(|region| region.hit().rect())
        .map(|rect| {
            Pos2::new(
                ((rect.min().x() + rect.max().x()) * 0.5) as f32,
                ((rect.min().y() + rect.max().y()) * 0.5) as f32,
            )
        })
        .expect("the presented scene exposes the second tab receiver");

    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        4,
        vec![Event::PointerButton {
            pos: second_tab_center,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }],
    );
    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        5,
        vec![Event::PointerButton {
            pos: second_tab_center,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );

    let selected = match dockspace
        .engine()
        .workspace()
        .root(ROOT)
        .and_then(|root| dockspace.engine().workspace().node(root.node))
    {
        Some(Node::Tabs { selected, .. }) => *selected,
        Some(Node::Split { .. }) | None => None,
    };
    assert_eq!(selected, Some(SECOND_ITEM));
}

#[test]
fn official_egui_outer_host_requests_close_only_after_matching_release() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("official-egui-close-journal", workspace())
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = SmokePanes::default();

    run_outer_frame(&mut dockspace, &context, &mut panes, 1, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 2, Vec::new());

    let close_center = dockspace
        .engine()
        .scene()
        .surface(SURFACE)
        .and_then(|surface| surface.ready())
        .and_then(|ready| {
            ready
                .candidate()
                .hit_manifest()
                .regions()
                .iter()
                .find(|region| matches!(region.id().kind(), PresentationHitRegionKind::TabClose(_)))
                .copied()
        })
        .map(|region| region.hit().rect())
        .map(|rect| {
            Pos2::new(
                ((rect.min().x() + rect.max().x()) * 0.5) as f32,
                ((rect.min().y() + rect.max().y()) * 0.5) as f32,
            )
        })
        .expect("the presented scene exposes the close receiver");

    let pressed = run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        3,
        vec![Event::PointerButton {
            pos: close_center,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }],
    );
    assert_eq!(pressed.close_requests().count(), 0);

    let released = run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        4,
        vec![Event::PointerButton {
            pos: close_center,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );
    assert_eq!(released.close_requests().count(), 1);
    assert!(released.transition().reduced_inputs().iter().all(|input| {
        !matches!(
            input.outcome(),
            dockspace::transition::InputOutcome::InteractionProcessed {
                outcome: InteractionOutcome::CloseRequested { .. },
                ..
            }
        )
    }));
    assert!(
        released
            .transition()
            .reduced_pointer_edges()
            .iter()
            .flat_map(|edge| edge.interaction_outcomes())
            .any(|outcome| matches!(outcome, InteractionOutcome::CloseRequested { .. }))
    );
}

#[test]
fn official_egui_outer_host_never_implicitly_allows_a_missing_pane_close() {
    let context = Context::default();
    let original = workspace();
    let mut dockspace = Dockspace::builder("official-egui-missing-pane-close", original.clone())
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = MissingPanes;

    run_outer_frame(&mut dockspace, &context, &mut panes, 1, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 2, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 3, Vec::new());

    let close = dockspace
        .engine()
        .interaction_projection(SURFACE)
        .expect("the missing pane retains an acknowledged scene")
        .plan()
        .tab_records()
        .iter()
        .find(|tab| tab.id().item == ITEM)
        .and_then(|tab| tab.close_bounds())
        .map(|rect| {
            Pos2::new(
                ((rect.min().x() + rect.max().x()) * 0.5) as f32,
                ((rect.min().y() + rect.max().y()) * 0.5) as f32,
            )
        })
        .expect("the missing pane keeps its close chrome");

    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        4,
        vec![
            Event::PointerMoved(close),
            Event::PointerButton {
                pos: close,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    let released = run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        5,
        vec![
            Event::PointerMoved(close),
            Event::PointerButton {
                pos: close,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    let plan = released
        .close_requests()
        .next()
        .cloned()
        .expect("the missing pane close requires an application decision");
    assert_eq!(released.close_requests().count(), 1);
    assert_eq!(plan.target(), ClosePlanTarget::Item { item: ITEM });
    assert_eq!(plan.items().len(), 1);
    assert_eq!(plan.items()[0].item(), ITEM);
    assert_eq!(plan.phase(), ClosePlanPhase::Requested);
    assert_eq!(dockspace.engine().workspace(), &original);

    run_outer_frame(&mut dockspace, &context, &mut panes, 6, Vec::new());
    assert_eq!(dockspace.engine().workspace(), &original);
    assert_eq!(
        dockspace
            .engine()
            .close_plan(plan.request())
            .map(|plan| plan.phase()),
        Some(ClosePlanPhase::Requested)
    );

    dockspace
        .resolve_close(plan.request(), plan.items()[0].token(), CloseDecision::Veto)
        .expect("the explicit veto resolves through the public facade");
    assert_eq!(dockspace.engine().workspace(), &original);
    assert_eq!(
        dockspace
            .engine()
            .close_plan(plan.request())
            .map(|plan| plan.phase()),
        Some(ClosePlanPhase::Vetoed)
    );
    run_outer_frame(&mut dockspace, &context, &mut panes, 7, Vec::new());
    assert_eq!(dockspace.engine().workspace(), &original);
    assert!(matches!(
        dockspace.engine().lookup_close_plan(plan.request()),
        ClosePlanLookup::RetiredTerminal
    ));
}

#[test]
fn official_egui_outer_host_opens_a_presented_overflow_menu() {
    let context = Context::default();
    let size = vec2(180.0, 200.0);
    let items = [ITEM, SECOND_ITEM, THIRD_ITEM];
    let mut dockspace =
        Dockspace::builder("official-egui-overflow-open", tab_list_workspace(items))
            .build()
            .expect("the overflow fixture builds");
    let mut panes = NamedPanes::with_items(items);

    let _ = open_overflow_menu(&mut dockspace, &context, &mut panes, size);
    let menu = current_plan(&dockspace)
        .tab_list_menu_records()
        .first()
        .expect("the presented menu remains open");
    assert!(!menu.rows().is_empty());
}

#[test]
fn official_egui_outer_host_dismisses_an_overflow_menu_only_through_its_backdrop() {
    let context = Context::default();
    let size = vec2(180.0, 200.0);
    let items = [ITEM, SECOND_ITEM, THIRD_ITEM];
    let mut dockspace =
        Dockspace::builder("official-egui-overflow-backdrop", tab_list_workspace(items))
            .build()
            .expect("the overflow fixture builds");
    let mut panes = NamedPanes::with_items(items);
    let next = open_overflow_menu(&mut dockspace, &context, &mut panes, size);
    let menu = current_plan(&dockspace)
        .tab_list_menu_records()
        .first()
        .expect("the presented menu is open");
    let menu_rect = Rect::from_min_max(
        Pos2::new(
            menu.bounds().min().x() as f32,
            menu.bounds().min().y() as f32,
        ),
        Pos2::new(
            menu.bounds().max().x() as f32,
            menu.bounds().max().y() as f32,
        ),
    );
    let outside = [
        Pos2::new(4.0, 4.0),
        Pos2::new(size.x - 4.0, 4.0),
        Pos2::new(4.0, size.y - 4.0),
        Pos2::new(size.x - 4.0, size.y - 4.0),
    ]
    .into_iter()
    .find(|point| !menu_rect.contains(*point))
    .expect("the menu leaves backdrop geometry inside the host");

    run_outer_frame_with_size(
        &mut dockspace,
        &context,
        &mut panes,
        next,
        size,
        vec![
            Event::PointerMoved(outside),
            Event::PointerButton {
                pos: outside,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    run_outer_frame_with_size(
        &mut dockspace,
        &context,
        &mut panes,
        next + 1,
        size,
        vec![
            Event::PointerMoved(outside),
            Event::PointerButton {
                pos: outside,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    let mut settled = false;
    for sequence in (next + 2)..=(next + 8) {
        let host = run_outer_frame_with_size(
            &mut dockspace,
            &context,
            &mut panes,
            sequence,
            size,
            Vec::new(),
        );
        let interactions_current = host
            .surface(SURFACE)
            .and_then(|surface| surface.paint())
            .is_some_and(|paint| paint.interactions_current());
        let menu_count = dockspace
            .engine()
            .interaction_projection(SURFACE)
            .map(|projection| projection.plan().tab_list_menu_records().len());
        if interactions_current && menu_count == Some(0) {
            settled = true;
            break;
        }
    }
    assert!(
        settled,
        "the dismissed menu must settle to a current closed plan"
    );
    assert_eq!(selected_item(&dockspace), Some(ITEM));
}

#[test]
fn official_egui_overflow_short_rows_own_their_complete_width() {
    let context = Context::default();
    let size = vec2(180.0, 200.0);
    let items = [ITEM, SECOND_ITEM, THIRD_ITEM];
    let mut dockspace = Dockspace::builder(
        "official-egui-overflow-row-width",
        tab_list_workspace(items),
    )
    .build()
    .expect("the overflow fixture builds");
    let mut panes = NamedPanes::with_items(items);
    panes.titles.insert(SECOND_ITEM, "B".to_owned());
    panes.titles.insert(
        THIRD_ITEM,
        "An intentionally wide overflow menu item".to_owned(),
    );
    let next = open_overflow_menu(&mut dockspace, &context, &mut panes, size);
    let menu = current_plan(&dockspace)
        .tab_list_menu_records()
        .first()
        .expect("the presented menu is open");
    let short = menu
        .rows()
        .iter()
        .find(|row| row.tab().item == SECOND_ITEM)
        .expect("the short hidden item has a row");
    let long = menu
        .rows()
        .iter()
        .find(|row| row.tab().item == THIRD_ITEM)
        .expect("the long hidden item has a row");
    assert_eq!(short.bounds().min().x(), long.bounds().min().x());
    assert_eq!(short.bounds().max().x(), long.bounds().max().x());
    let hit = short.hit().expect("the short row is visible").rect();
    let pointer = Pos2::new(
        (hit.max().x() - 1.0) as f32,
        ((hit.min().y() + hit.max().y()) * 0.5) as f32,
    );

    let pressed = run_outer_frame_with_size(
        &mut dockspace,
        &context,
        &mut panes,
        next,
        size,
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
    let released = run_outer_frame_with_size(
        &mut dockspace,
        &context,
        &mut panes,
        next + 1,
        size,
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
    run_outer_frame_with_size(
        &mut dockspace,
        &context,
        &mut panes,
        next + 2,
        size,
        Vec::new(),
    );

    assert_eq!(
        selected_item(&dockspace),
        Some(SECOND_ITEM),
        "press={:?}; release={:?}",
        pressed.transition().reduced_pointer_edges(),
        released.transition().reduced_pointer_edges()
    );
}

#[test]
fn official_egui_overflow_popup_owns_rows_and_frame_above_tiled_and_floating_panes() {
    for floating_underlay in [false, true] {
        for interaction in ["row", "frame"] {
            let context = Context::default();
            context.all_styles_mut(|style| style.spacing.item_spacing.y = 10.0);
            let size = vec2(300.0, 230.0);
            let popup_items = (0..32)
                .map(|index| ItemId::new(4_000 + index))
                .collect::<Vec<_>>();
            let underlay = ItemId::new(5_000 + u64::from(floating_underlay));
            let workspace =
                popup_underlay_workspace(popup_items.iter().copied(), underlay, floating_underlay);
            let mut dockspace = Dockspace::builder(
                (
                    "official-egui-overflow-underlay",
                    floating_underlay,
                    interaction,
                ),
                workspace,
            )
            .build()
            .expect("the popup underlay fixture builds");
            let original_version = dockspace.engine().version();
            let mut panes = NamedPanes::with_items(
                popup_items.iter().copied().chain(std::iter::once(underlay)),
            );
            for item in &popup_items {
                panes
                    .titles
                    .insert(*item, format!("A wide popup entry for pane {}", item.get()));
            }
            let next = open_overflow_menu(&mut dockspace, &context, &mut panes, size);
            let underlay_bounds = current_plan(&dockspace)
                .pane_records()
                .iter()
                .find(|pane| pane.selected() == Some(underlay))
                .map(|pane| egui_rect(pane.content_bounds()))
                .expect("the underlay pane is rendered");
            let mut rows = current_plan(&dockspace)
                .tab_list_menu_records()
                .first()
                .expect("the popup menu is open")
                .rows()
                .iter()
                .filter_map(|row| row.hit().map(|hit| (row.tab().item, egui_rect(hit.rect()))))
                .collect::<Vec<_>>();
            rows.sort_by(|left, right| left.1.min.y.total_cmp(&right.1.min.y));
            let (target_item, pointer) = if interaction == "row" {
                rows.iter()
                    .find_map(|(item, row)| {
                        let overlap = row.intersect(underlay_bounds);
                        overlap.is_positive().then_some((*item, overlap.center()))
                    })
                    .expect("a popup row overlaps the underlay pane")
            } else {
                let gap = rows
                    .windows(2)
                    .find_map(|rows| {
                        let gap = Rect::from_min_max(
                            Pos2::new(rows[0].1.min.x.max(rows[1].1.min.x), rows[0].1.max.y),
                            Pos2::new(rows[0].1.max.x.min(rows[1].1.max.x), rows[1].1.min.y),
                        )
                        .intersect(underlay_bounds);
                        gap.is_positive().then_some(gap)
                    })
                    .expect("popup row spacing overlaps the underlay pane");
                (popup_items[0], gap.center())
            };
            let underlay_selection = selected_in_group_containing(&dockspace, underlay);
            let underlay_clicks = panes.click_count(underlay);

            run_outer_frame_with_size(
                &mut dockspace,
                &context,
                &mut panes,
                next,
                size,
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
            run_outer_frame_with_size(
                &mut dockspace,
                &context,
                &mut panes,
                next + 1,
                size,
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
            run_outer_frame_with_size(
                &mut dockspace,
                &context,
                &mut panes,
                next + 2,
                size,
                Vec::new(),
            );

            assert_eq!(panes.click_count(underlay), underlay_clicks);
            assert_eq!(
                selected_in_group_containing(&dockspace, underlay),
                underlay_selection
            );
            if interaction == "row" {
                assert_eq!(
                    selected_in_group_containing(&dockspace, target_item),
                    Some(target_item)
                );
            } else {
                assert_eq!(dockspace.engine().version(), original_version);
                assert_eq!(
                    current_plan(&dockspace).tab_list_menu_records().len(),
                    1,
                    "menu-frame input must be consumed without dismissing the popup"
                );
            }
        }
    }
}

#[test]
fn official_egui_overflow_pointer_scroll_reaches_and_activates_the_last_item() {
    let context = Context::default();
    let size = vec2(220.0, 220.0);
    let items = (0..64)
        .map(|index| ItemId::new(1_000 + index))
        .collect::<Vec<_>>();
    let last = *items.last().expect("the fixture is non-empty");
    let mut dockspace = Dockspace::builder(
        "official-egui-overflow-pointer-scroll",
        tab_list_workspace(items.iter().copied()),
    )
    .build()
    .expect("the overflow fixture builds");
    let mut panes = NamedPanes::with_items(items.iter().copied());
    let next = open_overflow_menu(&mut dockspace, &context, &mut panes, size);
    let menu_bounds = current_plan(&dockspace)
        .tab_list_menu_records()
        .first()
        .expect("the presented menu is open")
        .bounds();
    let pointer = Pos2::new(
        ((menu_bounds.min().x() + menu_bounds.max().x()) * 0.5) as f32,
        ((menu_bounds.min().y() + menu_bounds.max().y()) * 0.5) as f32,
    );

    let wheel_host = run_outer_frame_with_size(
        &mut dockspace,
        &context,
        &mut panes,
        next,
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
    let mut activation_sequence = None;
    for sequence in (next + 1)..=(next + 8) {
        let host = run_outer_frame_with_size(
            &mut dockspace,
            &context,
            &mut panes,
            sequence,
            size,
            Vec::new(),
        );
        let interactions_current = host
            .surface(SURFACE)
            .and_then(|surface| surface.paint())
            .is_some_and(|paint| paint.interactions_current());
        let scrolled = dockspace
            .engine()
            .interaction_projection(SURFACE)
            .and_then(|projection| projection.plan().tab_list_menu_records().first())
            .is_some_and(|menu| menu.scroll_offset() == menu.maximum_scroll_offset());
        if interactions_current && scrolled {
            activation_sequence = Some(sequence + 1);
            break;
        }
    }
    let activation_sequence = activation_sequence.unwrap_or_else(|| {
        panic!(
            "the committed menu scroll did not become authoritative: {:?}",
            wheel_host.transition().reduced_inputs()
        )
    });
    let menu = current_plan(&dockspace)
        .tab_list_menu_records()
        .first()
        .expect("the menu remains open after scrolling");
    let last_hit = menu
        .rows()
        .iter()
        .find(|row| row.tab().item == last)
        .and_then(|row| row.hit())
        .map(|hit| hit.rect())
        .unwrap_or_else(|| {
            panic!(
                "scrolling did not expose the last menu item: offset={} maximum={}; inputs={:?}",
                menu.scroll_offset(),
                menu.maximum_scroll_offset(),
                wheel_host.transition().reduced_inputs()
            )
        });
    let last_pointer = Pos2::new(
        ((last_hit.min().x() + last_hit.max().x()) * 0.5) as f32,
        ((last_hit.min().y() + last_hit.max().y()) * 0.5) as f32,
    );
    run_outer_frame_with_size(
        &mut dockspace,
        &context,
        &mut panes,
        activation_sequence,
        size,
        vec![
            Event::PointerMoved(last_pointer),
            Event::PointerButton {
                pos: last_pointer,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    run_outer_frame_with_size(
        &mut dockspace,
        &context,
        &mut panes,
        activation_sequence + 1,
        size,
        vec![
            Event::PointerMoved(last_pointer),
            Event::PointerButton {
                pos: last_pointer,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    run_outer_frame_with_size(
        &mut dockspace,
        &context,
        &mut panes,
        activation_sequence + 2,
        size,
        Vec::new(),
    );

    assert_eq!(selected_item(&dockspace), Some(last));
}

#[test]
fn official_egui_overflow_keyboard_and_accesskit_reach_the_last_item() {
    for mode in ["keyboard", "accesskit"] {
        let context = Context::default();
        context.enable_accesskit();
        let size = vec2(220.0, 220.0);
        let items = (0..64)
            .map(|index| ItemId::new(2_000 + index))
            .collect::<Vec<_>>();
        let last = *items.last().expect("the fixture is non-empty");
        let mut dockspace = Dockspace::builder(
            ("official-egui-overflow-semantic", mode),
            tab_list_workspace(items.iter().copied()),
        )
        .build()
        .expect("the overflow fixture builds");
        let mut panes = NamedPanes::with_items(items.iter().copied());
        let next = open_overflow_menu(&mut dockspace, &context, &mut panes, size);

        match mode {
            "keyboard" => {
                run_outer_frame_with_size(
                    &mut dockspace,
                    &context,
                    &mut panes,
                    next,
                    size,
                    vec![Event::Key {
                        key: Key::End,
                        physical_key: Some(Key::End),
                        pressed: true,
                        repeat: false,
                        modifiers: Modifiers::NONE,
                    }],
                );
                let mut activation_sequence = None;
                for sequence in (next + 1)..=(next + 8) {
                    let host = run_outer_frame_with_size(
                        &mut dockspace,
                        &context,
                        &mut panes,
                        sequence,
                        size,
                        Vec::new(),
                    );
                    let interactions_current = host
                        .surface(SURFACE)
                        .and_then(|surface| surface.paint())
                        .is_some_and(|paint| paint.interactions_current());
                    let at_last = dockspace
                        .engine()
                        .interaction_projection(SURFACE)
                        .and_then(|projection| projection.plan().tab_list_menu_records().first())
                        .is_some_and(|menu| {
                            menu.scroll_offset() == menu.maximum_scroll_offset()
                                && menu
                                    .rows()
                                    .iter()
                                    .any(|row| row.focused() && row.tab().item == last)
                        });
                    if interactions_current && at_last {
                        activation_sequence = Some(sequence + 1);
                        break;
                    }
                }
                let activation_sequence = activation_sequence
                    .expect("End must focus and reveal the final presented menu row");
                run_outer_frame_with_size(
                    &mut dockspace,
                    &context,
                    &mut panes,
                    activation_sequence,
                    size,
                    vec![Event::Key {
                        key: Key::Enter,
                        physical_key: Some(Key::Enter),
                        pressed: true,
                        repeat: false,
                        modifiers: Modifiers::NONE,
                    }],
                );
                run_outer_frame_with_size(
                    &mut dockspace,
                    &context,
                    &mut panes,
                    activation_sequence + 1,
                    size,
                    Vec::new(),
                );
            }
            "accesskit" => {
                let (_, tree) = run_outer_frame_with_accesskit(
                    &mut dockspace,
                    &context,
                    &mut panes,
                    next,
                    size,
                    Vec::new(),
                );
                let label = format!("Pane {}", last.get());
                let (last_node, node) = accesskit_node_by_label(&tree, Role::MenuItem, &label);
                assert!(node.supports_action(Action::Click));
                run_outer_frame_with_accesskit(
                    &mut dockspace,
                    &context,
                    &mut panes,
                    next + 1,
                    size,
                    vec![accesskit_action(last_node, Action::Click)],
                );
                run_outer_frame_with_accesskit(
                    &mut dockspace,
                    &context,
                    &mut panes,
                    next + 2,
                    size,
                    Vec::new(),
                );
            }
            _ => unreachable!("fixture mode is exhaustive"),
        }

        assert_eq!(
            selected_item(&dockspace),
            Some(last),
            "large overflow activation failed in {mode} mode"
        );
    }
}

fn exercise_official_same_stack_reorder(source: ItemId, gap: usize, expected: &[ItemId]) {
    let context = Context::default();
    let original = tab_stack_workspace();
    let expected_multiset = original.item_multiset();
    let mut dockspace = Dockspace::builder("official-egui-same-stack-reorder", original)
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = SmokePanes::default();

    run_outer_frame(&mut dockspace, &context, &mut panes, 1, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 2, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 3, Vec::new());

    let tabs = dockspace
        .engine()
        .workspace()
        .root(ROOT)
        .expect("the root remains present")
        .node;
    let source_point = tab_drag_point(&dockspace, ROOT, source);
    let (target_id, target_point) = tab_gap_target(&dockspace, tabs, gap);

    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        4,
        vec![
            Event::PointerMoved(source_point),
            Event::PointerButton {
                pos: source_point,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Armed { .. } | InteractionStatus::Dragging { .. }
    ));

    for sequence in 5..=8 {
        run_outer_frame(
            &mut dockspace,
            &context,
            &mut panes,
            sequence,
            vec![Event::PointerMoved(target_point)],
        );
    }
    let preview = dockspace
        .engine()
        .interaction()
        .preview()
        .expect("the exact tab gap publishes one preview");
    assert!(matches!(
        preview.visual(),
        PreviewVisual::Dock { target, .. } if *target == target_id
    ));

    run_outer_frame(&mut dockspace, &context, &mut panes, 9, Vec::new());
    let released = run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        10,
        vec![
            Event::PointerMoved(target_point),
            Event::PointerButton {
                pos: target_point,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    let expected_changed = expected != [ITEM, SECOND_ITEM, THIRD_ITEM];
    assert!(
        released
            .transition()
            .reduced_pointer_edges()
            .iter()
            .flat_map(|edge| edge.interaction_outcomes())
            .any(|outcome| matches!(
                outcome,
                InteractionOutcome::DragDelivered {
                    delivery: InteractionDelivery::Workspace {
                        kind: WorkspaceDeliveryKind::Dock,
                        outcome: CommandOutcome::Moved { changed, .. },
                        changed: delivery_changed,
                    },
                    ..
                } if *changed == expected_changed && *delivery_changed == expected_changed
            ))
    );

    let workspace = dockspace.engine().workspace();
    assert_eq!(workspace.item_multiset(), expected_multiset);
    assert_eq!(tab_items(&dockspace, tabs), (expected, Some(source)));
    let expected_mru = std::iter::once(source)
        .chain(
            [ITEM, SECOND_ITEM, THIRD_ITEM]
                .into_iter()
                .filter(|item| *item != source),
        )
        .collect::<Vec<_>>();
    assert_eq!(workspace.tab_mru(tabs), Some(expected_mru.as_slice()));
}

#[test]
fn official_egui_outer_host_reorders_same_stack_tabs_at_exact_gaps() {
    exercise_official_same_stack_reorder(ITEM, 3, &[SECOND_ITEM, THIRD_ITEM, ITEM]);
    exercise_official_same_stack_reorder(THIRD_ITEM, 0, &[THIRD_ITEM, ITEM, SECOND_ITEM]);
    exercise_official_same_stack_reorder(ITEM, 1, &[ITEM, SECOND_ITEM, THIRD_ITEM]);
}

#[test]
fn official_egui_routes_items_from_main_to_contained_and_between_contained_roots() {
    let context = Context::default();
    let original = cross_presentation_workspace();
    let expected_items = original.item_multiset();
    let mut dockspace = Dockspace::builder("official-egui-cross-presentation-routing", original)
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = SmokePanes::default();
    let mut sequence = 0;

    for _ in 0..3 {
        sequence += 1;
        run_outer_frame(&mut dockspace, &context, &mut panes, sequence, Vec::new());
    }

    let source = tab_drag_point(&dockspace, ROOT, SECOND_ITEM);
    let (target_id, target) = center_guide_target(&dockspace, FLOATING_ROOT);
    let first_delivery = deliver_official_drag(
        &mut dockspace,
        &context,
        &mut panes,
        &mut sequence,
        source,
        target_id,
        target,
    );
    assert!(
        first_delivery
            .transition()
            .reduced_pointer_edges()
            .iter()
            .flat_map(|edge| edge.interaction_outcomes())
            .any(|outcome| matches!(
                outcome,
                InteractionOutcome::DragDelivered {
                    delivery: InteractionDelivery::Workspace {
                        kind: WorkspaceDeliveryKind::Dock,
                        changed: true,
                        ..
                    },
                    ..
                }
            ))
    );
    assert_eq!(
        dockspace.engine().workspace().item_multiset(),
        expected_items
    );
    assert_eq!(root_tab_items(&dockspace, ROOT), (&[ITEM][..], Some(ITEM)));
    assert_eq!(
        root_tab_items(&dockspace, FLOATING_ROOT),
        (
            &[THIRD_ITEM, FOURTH_ITEM, SECOND_ITEM][..],
            Some(SECOND_ITEM)
        )
    );

    for _ in 0..3 {
        sequence += 1;
        run_outer_frame(&mut dockspace, &context, &mut panes, sequence, Vec::new());
    }
    let source = tab_drag_point(&dockspace, FLOATING_ROOT, SECOND_ITEM);
    let (target_id, target) = center_guide_target(&dockspace, SECOND_FLOATING_ROOT);
    let second_delivery = deliver_official_drag(
        &mut dockspace,
        &context,
        &mut panes,
        &mut sequence,
        source,
        target_id,
        target,
    );
    assert!(
        second_delivery
            .transition()
            .reduced_pointer_edges()
            .iter()
            .flat_map(|edge| edge.interaction_outcomes())
            .any(|outcome| matches!(
                outcome,
                InteractionOutcome::DragDelivered {
                    delivery: InteractionDelivery::Workspace {
                        kind: WorkspaceDeliveryKind::Dock,
                        changed: true,
                        ..
                    },
                    ..
                }
            ))
    );

    let workspace = dockspace.engine().workspace();
    assert_eq!(workspace.item_multiset(), expected_items);
    assert_eq!(
        root_tab_items(&dockspace, FLOATING_ROOT),
        (&[THIRD_ITEM, FOURTH_ITEM][..], Some(FOURTH_ITEM))
    );
    assert_eq!(
        root_tab_items(&dockspace, SECOND_FLOATING_ROOT),
        (
            &[FIFTH_ITEM, SIXTH_ITEM, SECOND_ITEM][..],
            Some(SECOND_ITEM)
        )
    );
    assert_eq!(
        workspace
            .contained_floating(FLOATING)
            .map(|record| record.root),
        Some(FLOATING_ROOT)
    );
    assert_eq!(
        workspace
            .contained_floating(SECOND_FLOATING)
            .map(|record| record.root),
        Some(SECOND_FLOATING_ROOT)
    );
    let surface = workspace
        .surface(SURFACE)
        .expect("the surface remains present");
    assert_eq!(surface.main_root, Some(ROOT));
    assert_eq!(surface.contained, vec![SECOND_FLOATING, FLOATING]);
}

#[test]
fn official_egui_close_press_cannot_upgrade_into_a_tab_drag() {
    let context = Context::default();
    let original = tab_stack_workspace();
    let mut dockspace = Dockspace::builder("official-egui-close-does-not-drag", original.clone())
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = SmokePanes::default();

    run_outer_frame(&mut dockspace, &context, &mut panes, 1, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 2, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 3, Vec::new());
    let tabs = dockspace
        .engine()
        .workspace()
        .root(ROOT)
        .expect("the root remains present")
        .node;
    let close = dockspace
        .engine()
        .interaction_projection(SURFACE)
        .expect("the surface has one acknowledged interaction projection")
        .plan()
        .tab_records()
        .iter()
        .find(|tab| tab.id().item == SECOND_ITEM)
        .and_then(|tab| tab.close_bounds())
        .map(|rect| {
            Pos2::new(
                ((rect.min().x() + rect.max().x()) * 0.5) as f32,
                ((rect.min().y() + rect.max().y()) * 0.5) as f32,
            )
        })
        .expect("the requested tab exposes a close receiver");
    let (_, gap) = tab_gap_target(&dockspace, tabs, 3);

    let pressed = run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        4,
        vec![
            Event::PointerMoved(close),
            Event::PointerButton {
                pos: close,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    assert_eq!(pressed.close_requests().count(), 0);
    assert!(!matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Armed { .. }
            | InteractionStatus::Dragging { .. }
            | InteractionStatus::Resizing { .. }
    ));

    for sequence in 5..=7 {
        run_outer_frame(
            &mut dockspace,
            &context,
            &mut panes,
            sequence,
            vec![Event::PointerMoved(gap)],
        );
        assert!(!matches!(
            dockspace.engine().interaction().status(),
            InteractionStatus::Armed { .. }
                | InteractionStatus::Dragging { .. }
                | InteractionStatus::Resizing { .. }
        ));
    }

    let released = run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        8,
        vec![Event::PointerButton {
            pos: gap,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );
    assert_eq!(released.close_requests().count(), 0);
    assert_eq!(dockspace.engine().workspace(), &original);
    assert_eq!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    );
}

#[test]
fn official_egui_outer_host_does_not_click_through_a_foreground_area() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("official-egui-occlusion", two_tab_workspace())
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = SmokePanes::default();

    run_outer_frame(&mut dockspace, &context, &mut panes, 1, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 2, Vec::new());
    let tab_rect = dockspace
        .engine()
        .scene()
        .surface(SURFACE)
        .and_then(|surface| surface.ready())
        .and_then(|ready| {
            ready
                .candidate()
                .hit_manifest()
                .regions()
                .iter()
                .find(|region| {
                    matches!(
                        region.id().kind(),
                        PresentationHitRegionKind::TabBody(tab) if tab.item == SECOND_ITEM
                    )
                })
                .copied()
        })
        .map(|region| region.hit().rect())
        .map(|rect| {
            Rect::from_min_max(
                Pos2::new(rect.min().x() as f32, rect.min().y() as f32),
                Pos2::new(rect.max().x() as f32, rect.max().y() as f32),
            )
        })
        .expect("the presented scene exposes the second tab receiver");
    panes.overlay = Some(tab_rect);
    run_outer_frame(&mut dockspace, &context, &mut panes, 3, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 4, Vec::new());

    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        6,
        vec![Event::PointerButton {
            pos: tab_rect.center(),
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }],
    );

    let selected = match dockspace
        .engine()
        .workspace()
        .root(ROOT)
        .and_then(|root| dockspace.engine().workspace().node(root.node))
    {
        Some(Node::Tabs { selected, .. }) => *selected,
        Some(Node::Split { .. }) | None => None,
    };
    assert_eq!(selected, Some(ITEM));
}

#[test]
fn official_egui_outer_host_correlates_one_complete_same_batch_click() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("official-egui-ambiguous-batch", two_tab_workspace())
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = SmokePanes::default();

    run_outer_frame(&mut dockspace, &context, &mut panes, 1, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 2, Vec::new());
    let tab_center = dockspace
        .engine()
        .scene()
        .surface(SURFACE)
        .and_then(|surface| surface.ready())
        .and_then(|ready| {
            ready
                .candidate()
                .hit_manifest()
                .regions()
                .iter()
                .find(|region| {
                    matches!(
                        region.id().kind(),
                        PresentationHitRegionKind::TabBody(tab) if tab.item == SECOND_ITEM
                    )
                })
                .copied()
        })
        .map(|region| region.hit().rect())
        .map(|rect| {
            Pos2::new(
                ((rect.min().x() + rect.max().x()) * 0.5) as f32,
                ((rect.min().y() + rect.max().y()) * 0.5) as f32,
            )
        })
        .expect("the presented scene exposes the second tab receiver");

    let click = run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        3,
        vec![
            Event::PointerButton {
                pos: tab_center,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
            Event::PointerButton {
                pos: tab_center,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    assert_eq!(click.transition().reduced_pointer_edges().len(), 2);

    let selected = match dockspace
        .engine()
        .workspace()
        .root(ROOT)
        .and_then(|root| dockspace.engine().workspace().node(root.node))
    {
        Some(Node::Tabs { selected, .. }) => *selected,
        Some(Node::Split { .. }) | None => None,
    };
    assert_eq!(selected, Some(SECOND_ITEM));
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    ));
}

#[test]
fn official_egui_outer_host_docks_a_tab_through_an_exact_top_guide() {
    let context = Context::default();
    let (workspace, left_tabs, _, _) = split_workspace();
    let mut dockspace = Dockspace::builder("official-egui-docking-journal", workspace)
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = SmokePanes::default();

    run_outer_frame(&mut dockspace, &context, &mut panes, 1, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 2, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 3, Vec::new());
    let (source, target, _) = top_guide_drag_geometry(&dockspace, left_tabs);

    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        4,
        vec![
            Event::PointerMoved(source),
            Event::PointerButton {
                pos: source,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Armed { .. }
    ));
    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        5,
        vec![Event::PointerMoved(target)],
    );
    assert!(dockspace.engine().interaction().preview().is_some());

    run_outer_frame(&mut dockspace, &context, &mut panes, 6, Vec::new());
    let released = run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        7,
        vec![
            Event::PointerMoved(target),
            Event::PointerButton {
                pos: target,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    assert!(
        released
            .transition()
            .reduced_pointer_edges()
            .iter()
            .flat_map(|edge| edge.interaction_outcomes())
            .any(|outcome| matches!(
                outcome,
                InteractionOutcome::DragDelivered {
                    delivery: InteractionDelivery::Workspace {
                        kind: WorkspaceDeliveryKind::Dock,
                        ..
                    },
                    ..
                }
            ))
    );
}

#[test]
fn official_egui_foreground_area_blocks_a_drop_guide_receiver() {
    let context = Context::default();
    let (workspace, left_tabs, right_tabs, _) = split_workspace();
    let mut dockspace = Dockspace::builder("official-egui-docking-occlusion", workspace)
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = SmokePanes::default();

    run_outer_frame(&mut dockspace, &context, &mut panes, 1, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 2, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 3, Vec::new());
    let (source, target, target_rect) = top_guide_drag_geometry(&dockspace, left_tabs);
    panes.overlay = Some(target_rect);

    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        4,
        vec![
            Event::PointerMoved(source),
            Event::PointerButton {
                pos: source,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        5,
        vec![Event::PointerMoved(target)],
    );
    assert!(dockspace.engine().interaction().preview().is_none());

    let released = run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        6,
        vec![Event::PointerButton {
            pos: target,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );
    assert!(
        released
            .transition()
            .reduced_pointer_edges()
            .iter()
            .flat_map(|edge| edge.interaction_outcomes())
            .all(|outcome| !matches!(outcome, InteractionOutcome::DragDelivered { .. }))
    );
    assert!(matches!(
        dockspace.engine().workspace().node(right_tabs),
        Some(Node::Tabs { items, .. }) if items == &[SECOND_ITEM]
    ));
}

#[test]
fn official_egui_outer_host_resizes_a_splitter_through_the_pointer_journal() {
    let context = Context::default();
    let (workspace, _, _, split) = split_workspace();
    let mut dockspace = Dockspace::builder("official-egui-splitter-journal", workspace)
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = SmokePanes::default();

    run_outer_frame(&mut dockspace, &context, &mut panes, 1, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 2, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 3, Vec::new());
    let before_version = dockspace.engine().version();
    let before_weights = split_weights(&dockspace, split);
    let press = splitter_center(&dockspace, split);

    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        4,
        vec![
            Event::PointerMoved(press),
            Event::PointerButton {
                pos: press,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Resizing { .. }
    ));

    let moved = press + vec2(80.0, 0.0);
    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        5,
        vec![Event::PointerMoved(moved)],
    );
    assert_eq!(dockspace.engine().version(), before_version);
    assert_eq!(split_weights(&dockspace, split), before_weights);

    let released = run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        6,
        vec![Event::PointerButton {
            pos: moved,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );
    let outcomes = released
        .transition()
        .reduced_pointer_edges()
        .iter()
        .flat_map(|edge| edge.interaction_outcomes())
        .collect::<Vec<_>>();
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(
                outcome,
                InteractionOutcome::ResizeDelivered { changed: true, .. }
            ))
            .count(),
        1,
        "splitter release outcomes: {outcomes:#?}; reduced edges: {:#?}; interaction: {:#?}",
        released.transition().reduced_pointer_edges(),
        dockspace.engine().interaction().status(),
    );
    assert_ne!(split_weights(&dockspace, split), before_weights);
}

#[test]
fn official_egui_outer_host_resizes_a_splitter_junction_through_the_pointer_journal() {
    let context = Context::default();
    let (workspace, horizontal, vertical) = junction_workspace();
    let mut dockspace = Dockspace::builder("official-egui-splitter-junction-journal", workspace)
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = SmokePanes::default();

    run_outer_frame(&mut dockspace, &context, &mut panes, 1, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 2, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 3, Vec::new());
    let before_version = dockspace.engine().version();
    let before_horizontal = split_weights(&dockspace, horizontal);
    let before_vertical = split_weights(&dockspace, vertical);
    let press = splitter_junction_center(&dockspace);

    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        4,
        vec![
            Event::PointerMoved(press),
            Event::PointerButton {
                pos: press,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Resizing { .. }
    ));
    assert!(
        active_resize_weights(&dockspace).is_empty(),
        "pressing a junction does not invent a transient resize proposal"
    );

    let near = press + vec2(20.0, 12.0);
    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        5,
        vec![Event::PointerMoved(near)],
    );
    let near_updates = active_resize_weights(&dockspace);
    assert_eq!(
        near_updates.len(),
        2,
        "junction resize updates both split axes"
    );
    assert_ne!(near_updates[&horizontal], before_horizontal);
    assert_ne!(near_updates[&vertical], before_vertical);
    assert_eq!(dockspace.engine().version(), before_version);
    assert_eq!(split_weights(&dockspace, horizontal), before_horizontal);
    assert_eq!(split_weights(&dockspace, vertical), before_vertical);

    let far = press + vec2(72.0, 44.0);
    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        6,
        vec![Event::PointerMoved(far), Event::PointerMoved(near)],
    );
    assert_eq!(
        active_resize_weights(&dockspace),
        near_updates,
        "the final pointer edge in one host frame is the sole absolute proposal"
    );
    assert_eq!(dockspace.engine().version(), before_version);

    let released = run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        7,
        vec![Event::PointerButton {
            pos: far,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );
    assert_eq!(
        released
            .transition()
            .reduced_pointer_edges()
            .iter()
            .flat_map(|edge| edge.interaction_outcomes())
            .filter(|outcome| matches!(
                outcome,
                InteractionOutcome::ResizeDelivered { changed: true, .. }
            ))
            .count(),
        1,
        "junction release is delivered exactly once"
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    ));
    assert_eq!(
        dockspace.engine().version().revision().get(),
        before_version.revision().get() + 1,
        "junction release commits both axes in one workspace revision"
    );
    let committed_horizontal = split_weights(&dockspace, horizontal);
    let committed_vertical = split_weights(&dockspace, vertical);
    assert!(committed_horizontal[0] > near_updates[&horizontal][0]);
    assert!(committed_vertical[0] > near_updates[&vertical][0]);
    assert_ne!(committed_horizontal, before_horizontal);
    assert_ne!(committed_vertical, before_vertical);
}

#[test]
fn official_egui_outer_host_tears_off_and_redocks_a_partial_tab_losslessly() {
    let context = Context::default();
    let size = vec2(600.0, 400.0);
    let original = two_tab_workspace();
    let original_items = original.item_multiset();
    let mut dockspace = Dockspace::builder("official-egui-contained-tear-off", original)
        .build()
        .expect("the public facade accepts the tear-off workspace");
    let mut panes = NamedPanes::with_items([ITEM, SECOND_ITEM]);
    panes.minimum_sizes.insert(SECOND_ITEM, vec2(280.0, 190.0));

    let mut last_response = None;
    for sequence in 1..=3 {
        last_response = Some(run_outer_frame_with_size(
            &mut dockspace,
            &context,
            &mut panes,
            sequence,
            size,
            Vec::new(),
        ));
    }
    assert_eq!(
        last_response
            .expect("the outer host publishes a warm surface response")
            .surface(SURFACE)
            .and_then(|surface| surface.paint())
            .expect("the outer host paints its complete surface")
            .contained_capability(),
        DockspaceCapability::Supported,
    );
    let main_tabs = dockspace
        .engine()
        .workspace()
        .root(ROOT)
        .expect("the main root remains present")
        .node;
    let source = tab_drag_point(&dockspace, ROOT, SECOND_ITEM);
    let target = inner_activation_only_point(&dockspace, ROOT, main_tabs, 0);

    run_outer_frame_with_size(
        &mut dockspace,
        &context,
        &mut panes,
        4,
        size,
        vec![
            Event::PointerMoved(source),
            Event::PointerButton {
                pos: source,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Armed { .. }
    ));
    for sequence in 5..=8 {
        run_outer_frame_with_size(
            &mut dockspace,
            &context,
            &mut panes,
            sequence,
            size,
            vec![Event::PointerMoved(target)],
        );
    }
    let active = dockspace
        .engine()
        .interaction()
        .active_drag_view()
        .expect("the partial tab drag remains active");
    let session = active.session();
    let offer = *active
        .contained_offer()
        .expect("core reserves one contained identity for the partial payload");
    let preview = dockspace
        .engine()
        .interaction()
        .preview()
        .expect("the activation-only point publishes a contained preview");
    assert_eq!(preview.token().session(), session);
    assert!(matches!(
        preview.visual(),
        PreviewVisual::Contained {
            surface: SURFACE,
            fallback: false,
            ..
        }
    ));
    assert_eq!(
        dockspace.engine().workspace().item_multiset(),
        original_items
    );

    run_outer_frame_with_size(&mut dockspace, &context, &mut panes, 9, size, Vec::new());
    let released = run_outer_frame_with_size(
        &mut dockspace,
        &context,
        &mut panes,
        10,
        size,
        vec![Event::PointerButton {
            pos: target,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );
    assert_eq!(
        released
            .transition()
            .reduced_pointer_edges()
            .iter()
            .flat_map(|edge| edge.interaction_outcomes())
            .filter(|outcome| matches!(
                outcome,
                InteractionOutcome::DragDelivered {
                    session: delivered,
                    delivery: InteractionDelivery::Workspace {
                        kind: WorkspaceDeliveryKind::Contained,
                        changed: true,
                        ..
                    },
                } if *delivered == session
            ))
            .count(),
        1
    );
    let floating = dockspace
        .engine()
        .workspace()
        .contained_floating(offer.floating())
        .expect("the reserved contained presentation is committed");
    assert_eq!(floating.root, offer.root());
    let style = dockspace.style();
    assert!(floating.rect.width() >= 280.0 + 2.0 * f64::from(style.floating_border_width));
    assert!(
        floating.rect.height()
            >= f64::from(190.0 + style.tab_bar_height + style.floating_title_height)
                + 2.0 * f64::from(style.floating_border_width)
    );
    assert_eq!(
        dockspace.engine().workspace().item_multiset(),
        original_items
    );

    for sequence in 11..=14 {
        run_outer_frame_with_size(
            &mut dockspace,
            &context,
            &mut panes,
            sequence,
            size,
            Vec::new(),
        );
    }
    let detached_source = tab_drag_point(&dockspace, offer.root(), SECOND_ITEM);
    let (center_target, center) = center_guide_target(&dockspace, ROOT);
    run_outer_frame_with_size(
        &mut dockspace,
        &context,
        &mut panes,
        15,
        size,
        vec![
            Event::PointerMoved(detached_source),
            Event::PointerButton {
                pos: detached_source,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    for sequence in 16..=19 {
        run_outer_frame_with_size(
            &mut dockspace,
            &context,
            &mut panes,
            sequence,
            size,
            vec![Event::PointerMoved(center)],
        );
    }
    let redock_preview = dockspace
        .engine()
        .interaction()
        .preview()
        .expect("the center guide publishes a redock preview");
    assert!(matches!(
        redock_preview.visual(),
        PreviewVisual::Dock { target, .. } if *target == center_target
    ));
    run_outer_frame_with_size(&mut dockspace, &context, &mut panes, 20, size, Vec::new());
    run_outer_frame_with_size(
        &mut dockspace,
        &context,
        &mut panes,
        21,
        size,
        vec![Event::PointerButton {
            pos: center,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );

    let workspace = dockspace.engine().workspace();
    assert!(workspace.contained_floating(offer.floating()).is_none());
    assert!(workspace.root(offer.root()).is_none());
    assert_eq!(workspace.item_multiset(), original_items);
    assert_eq!(
        workspace.surface(SURFACE).map(|surface| surface.main_root),
        Some(Some(ROOT))
    );
    assert_eq!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    );
}

#[test]
fn official_egui_outer_host_reuses_a_complete_main_root_for_contained_tear_off() {
    let context = Context::default();
    let size = vec2(600.0, 400.0);
    let mut dockspace = Dockspace::builder(
        "official-egui-complete-root-tear-off",
        tab_list_workspace([ITEM]),
    )
    .build()
    .expect("the complete-root tear-off fixture builds");
    let mut panes = NamedPanes::with_items([ITEM]);

    for sequence in 1..=3 {
        run_outer_frame_with_size(
            &mut dockspace,
            &context,
            &mut panes,
            sequence,
            size,
            Vec::new(),
        );
    }
    let tabs = dockspace
        .engine()
        .workspace()
        .root(ROOT)
        .expect("the complete source root remains present")
        .node;
    let source = tab_drag_point(&dockspace, ROOT, ITEM);
    let target = inner_activation_only_point(&dockspace, ROOT, tabs, 0);
    run_outer_frame_with_size(
        &mut dockspace,
        &context,
        &mut panes,
        4,
        size,
        vec![
            Event::PointerMoved(source),
            Event::PointerButton {
                pos: source,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    for sequence in 5..=8 {
        run_outer_frame_with_size(
            &mut dockspace,
            &context,
            &mut panes,
            sequence,
            size,
            vec![Event::PointerMoved(target)],
        );
    }
    let offer = *dockspace
        .engine()
        .interaction()
        .active_drag_view()
        .and_then(|drag| drag.contained_offer())
        .expect("the complete-root drag reserves a contained carrier");
    assert_eq!(offer.root(), ROOT);

    run_outer_frame_with_size(&mut dockspace, &context, &mut panes, 9, size, Vec::new());
    run_outer_frame_with_size(
        &mut dockspace,
        &context,
        &mut panes,
        10,
        size,
        vec![Event::PointerButton {
            pos: target,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );

    let workspace = dockspace.engine().workspace();
    assert_eq!(
        workspace.surface(SURFACE).map(|surface| surface.main_root),
        Some(None)
    );
    assert_eq!(
        workspace
            .contained_floating(offer.floating())
            .map(|floating| floating.root),
        Some(ROOT)
    );
    assert!(workspace.root(ROOT).is_some());
    assert_eq!(workspace.item_multiset(), BTreeMap::from([(ITEM, 1)]));
}

#[test]
fn official_egui_outer_host_rejects_partial_contained_creation_by_policy() {
    let context = Context::default();
    let size = vec2(600.0, 400.0);
    let original = two_tab_workspace();
    let mut policy = DockPolicy::default();
    policy.set_allow_contained_floating(false);
    let mut dockspace =
        Dockspace::builder("official-egui-contained-creation-policy", original.clone())
            .policy(policy)
            .build()
            .expect("the policy fixture builds");
    let mut panes = NamedPanes::with_items([ITEM, SECOND_ITEM]);

    for sequence in 1..=3 {
        run_outer_frame_with_size(
            &mut dockspace,
            &context,
            &mut panes,
            sequence,
            size,
            Vec::new(),
        );
    }
    let tabs = dockspace
        .engine()
        .workspace()
        .root(ROOT)
        .expect("the source root remains present")
        .node;
    let source = tab_drag_point(&dockspace, ROOT, SECOND_ITEM);
    let target = inner_activation_only_point(&dockspace, ROOT, tabs, 0);
    let before_frontier = dockspace.engine().presentation_identity_frontier();
    let assert_rejected_state = |dockspace: &Dockspace| {
        let workspace = dockspace.engine().workspace();
        assert_eq!(workspace.item_multiset(), original.item_multiset());
        let surface = workspace
            .surface(SURFACE)
            .expect("the source surface remains present");
        assert_eq!(surface.main_root, Some(ROOT));
        assert!(surface.contained.is_empty());
        assert_eq!(workspace.roots().count(), 1);
        assert_eq!(workspace.contained_floatings().count(), 0);
        let tabs = workspace
            .root(ROOT)
            .expect("the source root remains present")
            .node;
        assert!(matches!(
            workspace.node(tabs),
            Some(Node::Tabs { items, selected })
                if items == &[ITEM, SECOND_ITEM] && *selected == Some(SECOND_ITEM)
        ));
        assert_eq!(workspace.tab_mru(tabs), Some(&[SECOND_ITEM, ITEM][..]));
    };
    run_outer_frame_with_size(
        &mut dockspace,
        &context,
        &mut panes,
        4,
        size,
        vec![
            Event::PointerMoved(source),
            Event::PointerButton {
                pos: source,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    for sequence in 5..=8 {
        run_outer_frame_with_size(
            &mut dockspace,
            &context,
            &mut panes,
            sequence,
            size,
            vec![Event::PointerMoved(target)],
        );
    }
    assert!(
        dockspace
            .engine()
            .interaction()
            .active_drag_view()
            .and_then(|drag| drag.contained_offer())
            .is_some(),
        "activation freezes identities before policy evaluates the candidate"
    );
    assert!(dockspace.engine().interaction().preview().is_none());
    assert_rejected_state(&dockspace);
    assert!(
        dockspace
            .engine()
            .presentation_identity_frontier()
            .last_root()
            > before_frontier.last_root()
    );
    assert!(
        dockspace
            .engine()
            .presentation_identity_frontier()
            .last_floating()
            > before_frontier.last_floating()
    );

    run_outer_frame_with_size(
        &mut dockspace,
        &context,
        &mut panes,
        9,
        size,
        vec![Event::PointerButton {
            pos: target,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );
    assert_rejected_state(&dockspace);
    assert_eq!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    );
}

#[test]
fn official_egui_outer_host_reuses_one_contained_reservation_across_target_changes() {
    let context = Context::default();
    let size = vec2(600.0, 400.0);
    let original = two_tab_workspace();
    let original_items = original.item_multiset();
    let mut dockspace = Dockspace::builder("official-egui-contained-reservation", original)
        .build()
        .expect("the reservation fixture builds");
    let mut panes = NamedPanes::with_items([ITEM, SECOND_ITEM]);

    for sequence in 1..=3 {
        run_outer_frame_with_size(
            &mut dockspace,
            &context,
            &mut panes,
            sequence,
            size,
            Vec::new(),
        );
    }
    let tabs = dockspace
        .engine()
        .workspace()
        .root(ROOT)
        .expect("the source root remains present")
        .node;
    let source = tab_drag_point(&dockspace, ROOT, SECOND_ITEM);
    let first_target = inner_activation_only_point(&dockspace, ROOT, tabs, 0);
    let second_target = inner_activation_only_point(&dockspace, ROOT, tabs, 1);
    let (dock_target, dock_point) = center_guide_target(&dockspace, ROOT);
    run_outer_frame_with_size(
        &mut dockspace,
        &context,
        &mut panes,
        4,
        size,
        vec![
            Event::PointerMoved(source),
            Event::PointerButton {
                pos: source,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    for sequence in 5..=8 {
        run_outer_frame_with_size(
            &mut dockspace,
            &context,
            &mut panes,
            sequence,
            size,
            vec![Event::PointerMoved(first_target)],
        );
    }
    let first_offer = *dockspace
        .engine()
        .interaction()
        .active_drag_view()
        .and_then(|drag| drag.contained_offer())
        .expect("the first contained candidate freezes a reservation");
    let frontier = dockspace.engine().presentation_identity_frontier();

    run_outer_frame_with_size(
        &mut dockspace,
        &context,
        &mut panes,
        9,
        size,
        vec![Event::PointerGone],
    );
    assert_eq!(
        dockspace
            .engine()
            .interaction()
            .active_drag_view()
            .and_then(|drag| drag.contained_offer()),
        Some(&first_offer)
    );
    run_outer_frame_with_size(
        &mut dockspace,
        &context,
        &mut panes,
        10,
        size,
        vec![Event::PointerMoved(dock_point)],
    );
    assert!(matches!(
        dockspace
            .engine()
            .interaction()
            .preview()
            .expect("the exact center target replaces the contained preview")
            .visual(),
        PreviewVisual::Dock { target, .. } if *target == dock_target
    ));
    for sequence in 11..=13 {
        run_outer_frame_with_size(
            &mut dockspace,
            &context,
            &mut panes,
            sequence,
            size,
            vec![Event::PointerMoved(second_target)],
        );
    }
    let active = dockspace
        .engine()
        .interaction()
        .active_drag_view()
        .expect("the drag remains active after target changes");
    assert_eq!(active.contained_offer(), Some(&first_offer));
    assert_eq!(
        dockspace.engine().presentation_identity_frontier(),
        frontier
    );
    let PreviewVisual::Contained {
        rect: second_rect,
        fallback: false,
        ..
    } = dockspace
        .engine()
        .interaction()
        .preview()
        .expect("the second activation point restores the contained preview")
        .visual()
    else {
        panic!("the second candidate must be a non-fallback contained preview");
    };
    let second_rect = *second_rect;

    run_outer_frame_with_size(&mut dockspace, &context, &mut panes, 14, size, Vec::new());
    run_outer_frame_with_size(
        &mut dockspace,
        &context,
        &mut panes,
        15,
        size,
        vec![Event::PointerButton {
            pos: second_target,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );
    let stored = dockspace
        .engine()
        .workspace()
        .contained_floating(first_offer.floating())
        .expect("release commits the original reservation");
    assert_eq!(stored.root, first_offer.root());
    assert_eq!(stored.rect, second_rect);
    assert_eq!(
        dockspace.engine().presentation_identity_frontier(),
        frontier
    );
    assert_eq!(
        dockspace.engine().workspace().item_multiset(),
        original_items
    );
}

#[test]
fn official_egui_outer_host_resizes_a_contained_floating_through_the_pointer_journal() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("official-egui-contained-resize", contained_workspace())
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = SmokePanes::default();

    run_outer_frame(&mut dockspace, &context, &mut panes, 1, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 2, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 3, Vec::new());
    let before = dockspace
        .engine()
        .workspace()
        .contained_floating(FLOATING)
        .expect("the contained fixture remains present")
        .rect;
    let press = contained_resize_center(&dockspace, ContainedResizeDirection::East);

    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        4,
        vec![
            Event::PointerMoved(press),
            Event::PointerButton {
                pos: press,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::ContainedTransforming { .. }
    ));

    let moved = press + vec2(48.0, 0.0);
    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        5,
        vec![Event::PointerMoved(moved)],
    );
    assert_eq!(
        dockspace
            .engine()
            .workspace()
            .contained_floating(FLOATING)
            .expect("the contained fixture remains present")
            .rect,
        before
    );
    assert!(
        dockspace
            .engine()
            .interaction()
            .active_contained_transform_view()
            .and_then(|transform| transform.preview())
            .is_some()
    );

    run_outer_frame(&mut dockspace, &context, &mut panes, 6, Vec::new());
    let released = run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        7,
        vec![Event::PointerButton {
            pos: moved,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );
    assert_eq!(
        released
            .transition()
            .reduced_pointer_edges()
            .iter()
            .flat_map(|edge| edge.interaction_outcomes())
            .filter(|outcome| matches!(
                outcome,
                InteractionOutcome::ContainedTransformDelivered { changed: true, .. }
            ))
            .count(),
        1
    );
    let after = dockspace
        .engine()
        .workspace()
        .contained_floating(FLOATING)
        .expect("the contained fixture remains present")
        .rect;
    assert!(after.width() > before.width());
}

#[test]
fn official_egui_outer_host_moves_a_contained_floating_through_the_pointer_journal() {
    let mut disabled_creation = DockPolicy::default();
    disabled_creation.set_allow_contained_floating(false);
    for (name, policy) in [
        ("official-egui-contained-move", DockPolicy::default()),
        ("official-egui-existing-contained-move", disabled_creation),
    ] {
        let context = Context::default();
        let mut dockspace = Dockspace::builder(name, contained_workspace())
            .policy(policy)
            .build()
            .expect("the public facade accepts a valid workspace");
        let mut panes = SmokePanes::default();

        run_outer_frame(&mut dockspace, &context, &mut panes, 1, Vec::new());
        run_outer_frame(&mut dockspace, &context, &mut panes, 2, Vec::new());
        run_outer_frame(&mut dockspace, &context, &mut panes, 3, Vec::new());
        let before = dockspace
            .engine()
            .workspace()
            .contained_floating(FLOATING)
            .expect("the contained fixture remains present")
            .rect;
        let press = contained_title_center(&dockspace);

        run_outer_frame(
            &mut dockspace,
            &context,
            &mut panes,
            4,
            vec![
                Event::PointerMoved(press),
                Event::PointerButton {
                    pos: press,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
        assert!(matches!(
            dockspace.engine().interaction().status(),
            InteractionStatus::Armed { .. }
        ));

        let moved = press + vec2(64.0, 48.0);
        run_outer_frame(
            &mut dockspace,
            &context,
            &mut panes,
            5,
            vec![Event::PointerMoved(moved)],
        );
        assert!(matches!(
            dockspace.engine().interaction().status(),
            InteractionStatus::Dragging { .. }
        ));
        assert_eq!(
            dockspace
                .engine()
                .workspace()
                .contained_floating(FLOATING)
                .expect("the contained fixture remains present")
                .rect,
            before
        );
        let PreviewVisual::Contained { rect: preview, .. } = dockspace
            .engine()
            .interaction()
            .preview()
            .expect("the move publishes a contained preview")
            .visual()
        else {
            panic!("the contained title fallback must publish a contained preview");
        };
        let preview = *preview;

        run_outer_frame(&mut dockspace, &context, &mut panes, 6, Vec::new());
        let released = run_outer_frame(
            &mut dockspace,
            &context,
            &mut panes,
            7,
            vec![Event::PointerButton {
                pos: moved,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
        );
        assert_eq!(
            released
                .transition()
                .reduced_pointer_edges()
                .iter()
                .flat_map(|edge| edge.interaction_outcomes())
                .filter(|outcome| matches!(
                    outcome,
                    InteractionOutcome::DragDelivered {
                        delivery: InteractionDelivery::Workspace {
                            kind: WorkspaceDeliveryKind::Contained,
                            ..
                        },
                        ..
                    }
                ))
                .count(),
            1
        );
        let after = dockspace
            .engine()
            .workspace()
            .contained_floating(FLOATING)
            .expect("the contained fixture remains present")
            .rect;
        assert_eq!(after, preview);
        assert_eq!(after.size(), before.size());
    }
}

#[test]
fn official_egui_outer_host_keeps_group_drag_across_local_pointer_loss() {
    let context = Context::default();
    let original = two_tab_workspace();
    let mut dockspace = Dockspace::builder("official-egui-group-pointer-loss", original.clone())
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = SmokePanes::default();

    run_outer_frame(&mut dockspace, &context, &mut panes, 1, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 2, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 3, Vec::new());
    let (source, tabs) = tab_group_grip_center(&dockspace, ROOT);
    let moved = source + vec2(80.0, 40.0);

    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        4,
        vec![
            Event::PointerMoved(source),
            Event::PointerButton {
                pos: source,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Armed { .. }
    ));
    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        5,
        vec![Event::PointerMoved(moved)],
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    let MovePayload::Tabs(payload) = dockspace
        .engine()
        .interaction()
        .active_drag_view()
        .expect("the group drag remains active")
        .payload()
    else {
        panic!("the group grip must preserve a complete tabs payload");
    };
    let session = dockspace
        .engine()
        .interaction()
        .active_drag_view()
        .expect("the group drag remains active")
        .session();
    assert_eq!(payload.root(), ROOT);
    assert_eq!(payload.node(), tabs);
    assert_eq!(dockspace.engine().workspace(), &original);

    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        6,
        vec![Event::PointerGone],
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    assert_eq!(dockspace.engine().workspace(), &original);

    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        7,
        vec![Event::PointerMoved(moved + vec2(8.0, 6.0))],
    );
    let resumed = dockspace
        .engine()
        .interaction()
        .active_drag_view()
        .expect("the same group drag resumes after local pointer loss");
    assert_eq!(resumed.session(), session);
    assert!(
        matches!(resumed.payload(), MovePayload::Tabs(payload) if payload.root() == ROOT && payload.node() == tabs)
    );
    assert_eq!(dockspace.engine().workspace(), &original);

    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        8,
        vec![Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }],
    );
    assert_eq!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    );
    assert_eq!(dockspace.engine().workspace(), &original);
}

#[test]
fn official_egui_outer_host_owns_escape_before_the_source_pane() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder(
        "official-egui-gesture-escape-ownership",
        two_tab_workspace(),
    )
    .build()
    .expect("the public facade accepts a valid workspace");
    let mut panes = EscapeTrackingPanes::default();

    run_outer_frame(&mut dockspace, &context, &mut panes, 1, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 2, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 3, Vec::new());
    let (source, _) = tab_group_grip_center(&dockspace, ROOT);

    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        4,
        vec![
            Event::PointerMoved(source),
            Event::PointerButton {
                pos: source,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        5,
        vec![Event::PointerMoved(source + vec2(80.0, 40.0))],
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));

    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        6,
        vec![Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }],
    );

    assert_eq!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Idle,
        "the docking gesture owns Escape and terminates"
    );
    assert_eq!(
        (panes.observations, panes.consumptions),
        (0, 0),
        "the source pane must neither observe nor consume Escape after docking owns it"
    );
}

#[test]
fn official_egui_outer_host_keeps_split_subtree_drag_across_local_pointer_loss() {
    let context = Context::default();
    let (original, _, source_root, _, _) = contained_subtree_workspace();
    let mut dockspace = Dockspace::builder("official-egui-subtree-pointer-loss", original.clone())
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = SmokePanes::default();

    run_outer_frame(&mut dockspace, &context, &mut panes, 1, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 2, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 3, Vec::new());
    let source = contained_title_center(&dockspace);
    let moved = source + vec2(100.0, 45.0);

    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        4,
        vec![
            Event::PointerMoved(source),
            Event::PointerButton {
                pos: source,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Armed { .. }
    ));
    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        5,
        vec![Event::PointerMoved(moved)],
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    let MovePayload::Subtree(payload) = dockspace
        .engine()
        .interaction()
        .active_drag_view()
        .expect("the contained title drag remains active")
        .payload()
    else {
        panic!("the contained title must preserve the complete split subtree");
    };
    let session = dockspace
        .engine()
        .interaction()
        .active_drag_view()
        .expect("the contained title drag remains active")
        .session();
    assert_eq!(payload.root(), FLOATING_ROOT);
    assert_eq!(payload.node(), source_root);
    assert_eq!(dockspace.engine().workspace(), &original);

    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        6,
        vec![Event::PointerGone],
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    assert_eq!(dockspace.engine().workspace(), &original);

    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        7,
        vec![Event::PointerMoved(moved + vec2(8.0, 6.0))],
    );
    let resumed = dockspace
        .engine()
        .interaction()
        .active_drag_view()
        .expect("the same subtree drag resumes after local pointer loss");
    assert_eq!(resumed.session(), session);
    assert!(
        matches!(resumed.payload(), MovePayload::Subtree(payload) if payload.root() == FLOATING_ROOT && payload.node() == source_root)
    );
    assert_eq!(dockspace.engine().workspace(), &original);

    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        8,
        vec![Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }],
    );
    assert_eq!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    );
    assert_eq!(dockspace.engine().workspace(), &original);
}

#[test]
fn official_egui_outer_host_rejects_contained_title_move_when_transform_is_disabled() {
    let context = Context::default();
    let mut policy = DockPolicy::default();
    policy.set_allow_contained_transform(false);
    let mut dockspace = Dockspace::builder(
        "official-egui-contained-transform-policy",
        contained_workspace(),
    )
    .policy(policy)
    .build()
    .expect("the public facade accepts a valid workspace");
    let mut panes = SmokePanes::default();

    run_outer_frame(&mut dockspace, &context, &mut panes, 1, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 2, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 3, Vec::new());
    let original = dockspace.engine().workspace().clone();
    let press = contained_title_center(&dockspace);

    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        4,
        vec![
            Event::PointerMoved(press),
            Event::PointerButton {
                pos: press,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    let moved = run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        5,
        vec![Event::PointerMoved(press + vec2(64.0, 48.0))],
    );

    assert!(
        moved
            .transition()
            .reduced_pointer_edges()
            .iter()
            .flat_map(|edge| edge.interaction_outcomes())
            .any(|outcome| matches!(
                outcome,
                InteractionOutcome::PreviewUpdated {
                    preview: None,
                    status: PreviewResolutionStatus::Rejected,
                    ..
                }
            ))
    );
    assert!(dockspace.engine().interaction().preview().is_none());
    assert_eq!(dockspace.engine().workspace(), &original);
}

#[test]
fn official_egui_outer_host_requests_contained_close_only_after_matching_release() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("official-egui-contained-close", contained_workspace())
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = SmokePanes::default();

    run_outer_frame(&mut dockspace, &context, &mut panes, 1, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 2, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 3, Vec::new());
    let close = contained_close_center(&dockspace);

    let pressed = run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        4,
        vec![
            Event::PointerMoved(close),
            Event::PointerButton {
                pos: close,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    assert_eq!(pressed.close_requests().count(), 0);

    let released = run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        5,
        vec![Event::PointerButton {
            pos: close,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );
    assert_eq!(released.close_requests().count(), 1);
}

#[test]
fn official_egui_outer_host_redocks_a_complete_split_subtree_through_all_edge_guides() {
    for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
        let context = Context::default();
        let (original, main_tabs, source_root, source_left, source_right) =
            contained_subtree_workspace();
        let mut dockspace = Dockspace::builder(
            ("official-egui-contained-subtree-guide", edge),
            original.clone(),
        )
        .build()
        .expect("the public facade accepts a valid workspace");
        let mut panes = SmokePanes::default();

        run_outer_frame(&mut dockspace, &context, &mut panes, 1, Vec::new());
        run_outer_frame(&mut dockspace, &context, &mut panes, 2, Vec::new());
        run_outer_frame(&mut dockspace, &context, &mut panes, 3, Vec::new());
        let source = contained_title_center(&dockspace);
        let slot = DropGuideSlot::Edge(edge);
        let (target_id, target) = contained_guide_center(&dockspace, main_tabs, slot);

        run_outer_frame(
            &mut dockspace,
            &context,
            &mut panes,
            4,
            vec![
                Event::PointerMoved(source),
                Event::PointerButton {
                    pos: source,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
        for sequence in 5..=8 {
            run_outer_frame(
                &mut dockspace,
                &context,
                &mut panes,
                sequence,
                vec![Event::PointerMoved(target)],
            );
        }

        let MovePayload::Subtree(payload) = dockspace
            .engine()
            .interaction()
            .active_drag_view()
            .expect("the split subtree drag remains active")
            .payload()
        else {
            panic!("the contained title must preserve the split subtree payload");
        };
        assert_eq!(payload.root(), FLOATING_ROOT);
        assert_eq!(payload.node(), source_root);
        let preview = dockspace
            .engine()
            .interaction()
            .preview()
            .expect("the exact edge publishes a dock preview");
        assert!(matches!(
            preview.visual(),
            PreviewVisual::Dock { target, .. } if *target == target_id
        ));
        let preview_session = preview.token().session();
        assert_eq!(
            preview_session,
            dockspace
                .engine()
                .interaction()
                .active_drag_view()
                .expect("the exact edge keeps the subtree drag active")
                .session()
        );
        assert_eq!(dockspace.engine().workspace(), &original);

        run_outer_frame(&mut dockspace, &context, &mut panes, 9, Vec::new());
        let released = run_outer_frame(
            &mut dockspace,
            &context,
            &mut panes,
            10,
            vec![
                Event::PointerMoved(target),
                Event::PointerButton {
                    pos: target,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
        let deliveries = released
            .transition()
            .reduced_pointer_edges()
            .iter()
            .flat_map(|edge| edge.interaction_outcomes())
            .filter(|outcome| matches!(outcome, InteractionOutcome::DragDelivered { .. }))
            .collect::<Vec<_>>();
        assert!(
            matches!(
                deliveries.as_slice(),
                [InteractionOutcome::DragDelivered {
                    session,
                    delivery: InteractionDelivery::Workspace {
                        kind: WorkspaceDeliveryKind::Dock,
                        changed: true,
                        ..
                    },
                }] if *session == preview_session
            ),
            "exact {edge:?} delivery must commit the painted preview once: {deliveries:?}"
        );
        assert_subtree_redock_topology(&dockspace, &original, edge, source_left, source_right);
    }
}

#[test]
fn official_egui_outer_host_docks_a_contained_root_through_all_exact_inner_guides() {
    for slot in [
        DropGuideSlot::Center,
        DropGuideSlot::Edge(Edge::Left),
        DropGuideSlot::Edge(Edge::Right),
        DropGuideSlot::Edge(Edge::Top),
        DropGuideSlot::Edge(Edge::Bottom),
    ] {
        let context = Context::default();
        let (workspace, target_tabs) = contained_split_workspace();
        let mut dockspace = Dockspace::builder("official-egui-contained-guide-matrix", workspace)
            .build()
            .expect("the public facade accepts a valid workspace");
        let mut panes = SmokePanes::default();

        run_outer_frame(&mut dockspace, &context, &mut panes, 1, Vec::new());
        run_outer_frame(&mut dockspace, &context, &mut panes, 2, Vec::new());
        run_outer_frame(&mut dockspace, &context, &mut panes, 3, Vec::new());
        let source = contained_title_center(&dockspace);
        let (target_id, target) = contained_guide_center(&dockspace, target_tabs, slot);

        run_outer_frame(
            &mut dockspace,
            &context,
            &mut panes,
            4,
            vec![
                Event::PointerMoved(source),
                Event::PointerButton {
                    pos: source,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
        run_outer_frame(
            &mut dockspace,
            &context,
            &mut panes,
            5,
            vec![Event::PointerMoved(target)],
        );
        assert!(matches!(
            dockspace.engine().interaction().status(),
            InteractionStatus::Dragging { .. }
        ));
        let affordance = dockspace
            .engine()
            .interaction()
            .drop_affordance()
            .expect("an exact guide publishes its affordance");
        let active = affordance
            .active_target()
            .expect("the requested guide is active");
        assert_eq!(active.slot(), slot);
        assert_eq!(active.target_id(), target_id);
        assert!(matches!(
            dockspace
                .engine()
                .interaction()
                .preview()
                .expect("an exact guide publishes its preview")
                .visual(),
            PreviewVisual::Dock { target, .. } if *target == target_id
        ));

        run_outer_frame(&mut dockspace, &context, &mut panes, 6, Vec::new());
        let released = run_outer_frame(
            &mut dockspace,
            &context,
            &mut panes,
            7,
            vec![Event::PointerButton {
                pos: target,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
        );
        assert!(
            released
                .transition()
                .reduced_pointer_edges()
                .iter()
                .flat_map(|edge| edge.interaction_outcomes())
                .any(|outcome| matches!(
                    outcome,
                    InteractionOutcome::DragDelivered {
                        delivery: InteractionDelivery::Workspace {
                            kind: WorkspaceDeliveryKind::Dock,
                            ..
                        },
                        ..
                    }
                )),
            "exact {slot:?} guide must deliver the contained root"
        );
        assert!(
            dockspace
                .engine()
                .workspace()
                .contained_floating(FLOATING)
                .is_none(),
            "exact {slot:?} docking consumes the contained presentation"
        );
        assert!(dockspace.engine().workspace().root(FLOATING_ROOT).is_none());
        assert_eq!(
            dockspace.engine().workspace().item_multiset(),
            std::collections::BTreeMap::from([
                (ITEM, 1),
                (SECOND_ITEM, 1),
                (THIRD_ITEM, 1),
                (FOURTH_ITEM, 1),
            ])
        );
        assert_contained_guide_topology(&dockspace, slot);
    }
}

#[test]
fn official_egui_outer_host_docks_a_contained_root_at_an_exact_tab_gap() {
    let context = Context::default();
    let (workspace, target_tabs) = contained_split_workspace();
    let expected_items = workspace.item_multiset();
    let mut dockspace = Dockspace::builder("official-egui-contained-tab-gap", workspace)
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = SmokePanes::default();
    let mut sequence = 0;

    for _ in 0..3 {
        sequence += 1;
        run_outer_frame(&mut dockspace, &context, &mut panes, sequence, Vec::new());
    }
    let source = contained_title_center(&dockspace);
    let (target_id, target) = tab_gap_target(&dockspace, target_tabs, 0);
    let released = deliver_official_drag(
        &mut dockspace,
        &context,
        &mut panes,
        &mut sequence,
        source,
        target_id,
        target,
    );

    assert_eq!(
        released
            .transition()
            .reduced_pointer_edges()
            .iter()
            .flat_map(|edge| edge.interaction_outcomes())
            .filter(|outcome| matches!(
                outcome,
                InteractionOutcome::DragDelivered {
                    delivery: InteractionDelivery::Workspace {
                        kind: WorkspaceDeliveryKind::Dock,
                        ..
                    },
                    ..
                }
            ))
            .count(),
        1
    );
    let workspace = dockspace.engine().workspace();
    assert!(workspace.contained_floating(FLOATING).is_none());
    assert!(workspace.root(FLOATING_ROOT).is_none());
    assert_eq!(workspace.item_multiset(), expected_items);
    assert_eq!(
        tab_items(&dockspace, target_tabs),
        (&[THIRD_ITEM, FOURTH_ITEM, ITEM][..], Some(FOURTH_ITEM))
    );
    assert_eq!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    );
}

#[test]
fn official_egui_outer_host_keeps_inner_affordance_while_moving_between_targets() {
    let context = Context::default();
    let (workspace, target_tabs) = contained_split_workspace();
    let original = workspace.clone();
    let mut dockspace = Dockspace::builder("official-egui-contained-activation-move", workspace)
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = SmokePanes::default();

    run_outer_frame(&mut dockspace, &context, &mut panes, 1, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 2, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 3, Vec::new());
    let source = contained_title_center(&dockspace);
    let target = contained_activation_only_point(&dockspace, target_tabs);
    let before = original
        .contained_floating(FLOATING)
        .expect("the original contained presentation exists");
    let expected = LogicalRect::new(
        before.rect.x() + f64::from(target.x) - f64::from(source.x),
        before.rect.y() + f64::from(target.y) - f64::from(source.y),
        before.rect.width(),
        before.rect.height(),
    )
    .expect("the selected activation point keeps the floating in bounds");

    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        4,
        vec![
            Event::PointerMoved(source),
            Event::PointerButton {
                pos: source,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    for sequence in 5..=8 {
        run_outer_frame(
            &mut dockspace,
            &context,
            &mut panes,
            sequence,
            vec![Event::PointerMoved(target)],
        );
    }

    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    let visual = dockspace
        .engine()
        .interaction()
        .preview()
        .expect("the contained move fallback publishes a preview")
        .visual();
    let PreviewVisual::Contained {
        surface,
        rect,
        fallback,
    } = visual
    else {
        panic!("the contained move fallback published {visual:?}");
    };
    assert_eq!(*surface, SURFACE);
    assert_eq!(*rect, expected);
    assert!(!fallback);
    let affordance = dockspace
        .engine()
        .interaction()
        .drop_affordance()
        .expect("the inner activation publishes its passive affordance");
    let inner = affordance
        .clusters()
        .iter()
        .find(|cluster| cluster.id() == DropGuideClusterId::inner(SURFACE, ROOT, target_tabs))
        .expect("the target inner cluster remains visible");
    assert_eq!(
        inner
            .targets()
            .iter()
            .map(DropAffordanceTarget::slot)
            .collect::<Vec<_>>(),
        vec![
            DropGuideSlot::Center,
            DropGuideSlot::Edge(Edge::Left),
            DropGuideSlot::Edge(Edge::Right),
            DropGuideSlot::Edge(Edge::Top),
            DropGuideSlot::Edge(Edge::Bottom),
        ]
    );
    assert!(affordance.active_target().is_none());

    run_outer_frame(&mut dockspace, &context, &mut panes, 9, Vec::new());
    let released = run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        10,
        vec![Event::PointerButton {
            pos: target,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );
    assert_eq!(
        released
            .transition()
            .reduced_pointer_edges()
            .iter()
            .flat_map(|edge| edge.interaction_outcomes())
            .filter(|outcome| matches!(
                outcome,
                InteractionOutcome::DragDelivered {
                    delivery: InteractionDelivery::Workspace {
                        kind: WorkspaceDeliveryKind::Contained,
                        ..
                    },
                    ..
                }
            ))
            .count(),
        1
    );
    let moved = dockspace
        .engine()
        .workspace()
        .contained_floating(FLOATING)
        .expect("the contained presentation remains present");
    assert_eq!(moved.root, before.root);
    assert_eq!(moved.rect, expected);
    assert_eq!(moved.rect.size(), before.rect.size());
    assert_eq!(
        dockspace
            .engine()
            .workspace()
            .surface(SURFACE)
            .expect("the surface remains present")
            .contained,
        original
            .surface(SURFACE)
            .expect("the original surface exists")
            .contained
    );
    assert_eq!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    );
}

#[test]
fn official_egui_outer_host_does_not_move_through_a_foreground_activation_overlay() {
    let context = Context::default();
    let (workspace, target_tabs) = contained_split_workspace();
    let original = workspace.clone();
    let mut dockspace =
        Dockspace::builder("official-egui-contained-activation-occlusion", workspace)
            .build()
            .expect("the public facade accepts a valid workspace");
    let mut panes = SmokePanes::default();

    run_outer_frame(&mut dockspace, &context, &mut panes, 1, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 2, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 3, Vec::new());
    let source = contained_title_center(&dockspace);
    let target = contained_activation_only_point(&dockspace, target_tabs);
    panes.overlay = Some(Rect::from_center_size(target, vec2(48.0, 48.0)));
    run_outer_frame(&mut dockspace, &context, &mut panes, 4, Vec::new());
    run_outer_frame(&mut dockspace, &context, &mut panes, 5, Vec::new());

    run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        6,
        vec![
            Event::PointerMoved(source),
            Event::PointerButton {
                pos: source,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    for sequence in 7..=10 {
        run_outer_frame(
            &mut dockspace,
            &context,
            &mut panes,
            sequence,
            vec![Event::PointerMoved(target)],
        );
    }

    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    assert!(dockspace.engine().interaction().preview().is_none());
    assert!(dockspace.engine().interaction().drop_affordance().is_none());
    assert_eq!(dockspace.engine().workspace(), &original);

    run_outer_frame(&mut dockspace, &context, &mut panes, 11, Vec::new());
    let released = run_outer_frame(
        &mut dockspace,
        &context,
        &mut panes,
        12,
        vec![Event::PointerButton {
            pos: target,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );
    assert!(
        released
            .transition()
            .reduced_pointer_edges()
            .iter()
            .flat_map(|edge| edge.interaction_outcomes())
            .all(|outcome| !matches!(outcome, InteractionOutcome::DragDelivered { .. }))
    );
    assert_eq!(dockspace.engine().workspace(), &original);
    assert_eq!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    );
}

#[test]
fn stable_paint_only_facade_does_not_poll_for_unavailable_authority() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("official-egui-no-authority-poll", workspace())
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = SmokePanes::default();
    let mut last_repaint_delay = Duration::ZERO;

    for _ in 0..4 {
        let output = context.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))),
                ..RawInput::default()
            },
            |ui| {
                let response = dockspace
                    .show_single_surface(SURFACE, ui, &mut panes)
                    .expect("the public facade advances an official-egui frame");
                assert!(!response.interactions_current());
            },
        );
        last_repaint_delay = output
            .viewport_output
            .get(&ViewportId::ROOT)
            .expect("the root viewport has output")
            .repaint_delay;
    }

    assert_ne!(
        last_repaint_delay,
        Duration::ZERO,
        "paint-only mode must not spin while presentation authority is unavailable"
    );
}

#[test]
fn paint_only_chrome_does_not_advertise_unavailable_accessibility_actions() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("official-egui-accessibility", workspace())
        .build()
        .expect("the public facade accepts a valid workspace");
    let mut panes = SmokePanes::default();
    let mut tree = None;

    for _ in 0..3 {
        let output = context.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))),
                ..RawInput::default()
            },
            |ui| {
                let response = dockspace
                    .show_single_surface(SURFACE, ui, &mut panes)
                    .expect("the public facade advances an official-egui frame");
                assert!(!response.interactions_current());
            },
        );
        tree = output.platform_output.accesskit_update;
    }

    let tree = tree.expect("AccessKit output is enabled");
    let mut disabled_chrome_count = 0;
    for (_, node) in &tree.nodes {
        if node.is_disabled() && matches!(node.role(), Role::Tab | Role::TabList | Role::Button) {
            disabled_chrome_count += 1;
            for action in [
                Action::Focus,
                Action::Click,
                Action::Increment,
                Action::Decrement,
                Action::ScrollIntoView,
            ] {
                assert!(
                    !node.supports_action(action),
                    "disabled docking chrome must not advertise {action:?}"
                );
            }
        }
    }
    assert!(
        disabled_chrome_count > 0,
        "the fixture must exercise disabled docking chrome"
    );
}

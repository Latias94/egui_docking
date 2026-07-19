use std::collections::BTreeMap;

use dockspace::geometry::LogicalRect;
use dockspace::graph::{Axis, ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use dockspace::interaction::{InteractionStatus, PreviewVisual};
use dockspace::scene::SurfaceScene;
use dockspace::transition::WorkspaceVersion;
use egui::accesskit::{Action, ActionRequest};
use egui::{
    Context, Event, Id, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui, UiBuilder, vec2,
};
use egui_dockspace::{Dockspace, DockspaceSurfaceStatus, PaneCloseResponse, PaneView};

const SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(10);
const FLOATING_ROOT: RootId = RootId::new(11);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(20);
const ITEM_A: ItemId = ItemId::new(100);
const ITEM_B: ItemId = ItemId::new(101);
const ITEM_C: ItemId = ItemId::new(102);

#[derive(Default)]
struct TestPanes {
    titles: BTreeMap<ItemId, String>,
    close_response: BTreeMap<ItemId, PaneCloseResponse>,
    minimum_sizes: BTreeMap<ItemId, egui::Vec2>,
    ui_calls: BTreeMap<ItemId, usize>,
    pointer_down_ui_calls: BTreeMap<ItemId, usize>,
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

    fn pointer_down_ui_calls(&self, item: ItemId) -> usize {
        self.pointer_down_ui_calls
            .get(&item)
            .copied()
            .unwrap_or_default()
    }
}

impl PaneView for TestPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        self.titles.get(&item).cloned().map(Into::into)
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) {
        *self.ui_calls.entry(item).or_default() += 1;
        if ui.input(|input| input.pointer.primary_down()) {
            *self.pointer_down_ui_calls.entry(item).or_default() += 1;
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
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
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
    assert_eq!(panes.ui_calls(ITEM_A), 1);
}

#[test]
fn stale_projection_never_invokes_pane_when_discard_budget_is_exhausted() {
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
    let pointer_calls_before = panes.pointer_down_ui_calls(ITEM_A);
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
        vec![Event::PointerMoved(press), pointer_button(press, true)],
    );
    assert_eq!(
        stale_observations.len(),
        1,
        "the configured pass budget rejects discard"
    );
    assert!(!stale_observations[0].interactions_current);
    assert_eq!(panes.ui_calls(ITEM_A), ui_calls_before);
    assert_eq!(panes.pointer_down_ui_calls(ITEM_A), pointer_calls_before);

    let current_observations = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(press), pointer_button(press, false)],
    );
    assert!(
        current_observations
            .last()
            .expect("stable frame paints")
            .interactions_current
    );
    assert_eq!(panes.ui_calls(ITEM_A), ui_calls_before + 1);
    assert_eq!(panes.pointer_down_ui_calls(ITEM_A), pointer_calls_before);
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
    let b_pointer_before = panes.pointer_down_ui_calls(ITEM_B);
    let press = Pos2::new(300.0, 200.0);
    let stale_frame = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(press), pointer_button(press, true)],
    );
    assert_eq!(stale_frame.len(), 1, "the one-pass budget must fail closed");
    assert!(!stale_frame[0].interactions_current);
    assert_eq!(panes.ui_calls(ITEM_B), b_ui_before);
    assert_eq!(panes.pointer_down_ui_calls(ITEM_B), b_pointer_before);

    let recovered_frame = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![pointer_button(press, false)],
    );
    assert!(
        recovered_frame
            .last()
            .expect("stable frame paints")
            .interactions_current
    );
    assert_eq!(panes.ui_calls(ITEM_B), b_ui_before + 1);
    assert_eq!(panes.pointer_down_ui_calls(ITEM_B), b_pointer_before);
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
    assert_eq!(panes.ui_calls(ITEM_A), 1);
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

use std::collections::BTreeMap;

use dockspace::drop_guide::{DropGuideScope, DropGuideSlot};
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use dockspace::interaction::InteractionStatus;
use dockspace::scene::SurfaceScene;
use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui, UiBuilder, vec2};
use egui_dockspace::{ContainedPresentationIds, Dockspace, PaneView, TearOffMode};

const SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(10);
const DETACHED_ROOT: RootId = RootId::new(11);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(20);
const ITEM_A: ItemId = ItemId::new(100);
const ITEM_B: ItemId = ItemId::new(101);

#[derive(Default)]
struct TestPanes {
    detached_minimum: egui::Vec2,
}

impl PaneView for TestPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(format!("Pane {}", item.get()).into())
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut Ui) {}

    fn minimum_size(&self, item: ItemId) -> egui::Vec2 {
        if item == ITEM_B {
            self.detached_minimum
        } else {
            egui::Vec2::ZERO
        }
    }
}

fn workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM_A, ITEM_B]));
    builder.set_root(MAIN_ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::new(MAIN_ROOT));
    builder.build().expect("tear-off fixture is valid")
}

fn surface_rect() -> Rect {
    Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0))
}

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0))),
        events,
        ..RawInput::default()
    }
}

fn pointer_button(position: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos: position,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

fn run_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    events: Vec<Event>,
) {
    let _ = context.run_ui(input(events), |ui| {
        let mut surface = ui.new_child(UiBuilder::new().max_rect(surface_rect()));
        dockspace
            .show(SURFACE, &mut surface, panes)
            .expect("tear-off frame advances");
    });
}

fn warm(context: &Context, dockspace: &mut Dockspace, panes: &mut TestPanes) {
    run_frame(context, dockspace, panes, Vec::new());
    run_frame(context, dockspace, panes, Vec::new());
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "finite scene coordinates intentionally become egui f32 input coordinates"
)]
fn tab_point(dockspace: &Dockspace, item: ItemId) -> Pos2 {
    let SurfaceScene::Ready(ready) = dockspace
        .engine()
        .scene()
        .and_then(|scene| scene.surface(SURFACE))
        .expect("surface scene exists")
    else {
        panic!("surface scene is ready");
    };
    let rect = ready
        .tabs()
        .iter()
        .find(|tab| tab.id().item == item)
        .expect("requested tab is painted")
        .rect();
    Pos2::new(
        (rect.min().x() + 8.0) as f32,
        ((rect.min().y() + rect.max().y()) * 0.5) as f32,
    )
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "finite scene coordinates intentionally become egui f32 input coordinates"
)]
fn main_center_target_point(dockspace: &Dockspace) -> Pos2 {
    let SurfaceScene::Ready(ready) = dockspace
        .engine()
        .scene()
        .and_then(|scene| scene.surface(SURFACE))
        .expect("surface scene exists")
    else {
        panic!("surface scene is ready");
    };
    let rect = ready
        .drop_guide_clusters()
        .iter()
        .find(|cluster| {
            cluster.id().root == MAIN_ROOT && matches!(cluster.id().scope, DropGuideScope::Inner(_))
        })
        .and_then(|cluster| cluster.target(DropGuideSlot::Center))
        .expect("main center guide is published")
        .target()
        .region()
        .rect();
    Pos2::new(
        ((rect.min().x() + rect.max().x()) * 0.5) as f32,
        ((rect.min().y() + rect.max().y()) * 0.5) as f32,
    )
}

fn drag_to(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    source: Pos2,
    target: Pos2,
) {
    run_frame(
        context,
        dockspace,
        panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    for _ in 0..4 {
        run_frame(context, dockspace, panes, vec![Event::PointerMoved(target)]);
    }
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Dragging { .. }
    ));
    assert!(dockspace.engine().interaction().preview().is_some());
    run_frame(
        context,
        dockspace,
        panes,
        vec![Event::PointerMoved(target), pointer_button(target, false)],
    );
    run_frame(context, dockspace, panes, Vec::new());
    run_frame(context, dockspace, panes, Vec::new());
}

#[test]
fn contained_tear_off_and_redock_are_preview_acknowledged_and_lossless() {
    let context = Context::default();
    let original = workspace();
    let mut ids = Some(ContainedPresentationIds::new(DETACHED_ROOT, FLOATING));
    let mut dockspace = Dockspace::builder("contained-tear-off-delivery", original)
        .tear_off_mode(TearOffMode::Contained)
        .presentation_ids(move || ids.take())
        .build()
        .expect("dockspace builds");
    let mut panes = TestPanes {
        detached_minimum: vec2(280.0, 190.0),
    };
    warm(&context, &mut dockspace, &mut panes);

    let source = tab_point(&dockspace, ITEM_B);
    let outside_surface = Pos2::new(470.0, 210.0);
    drag_to(
        &context,
        &mut dockspace,
        &mut panes,
        source,
        outside_surface,
    );

    let floating = dockspace
        .engine()
        .workspace()
        .contained_floating(FLOATING)
        .expect("tear-off creates the contained presentation");
    assert_eq!(floating.root, DETACHED_ROOT);
    let style = dockspace.style();
    assert!(floating.rect.width() >= 280.0 + 2.0 * f64::from(style.floating_border_width));
    assert!(
        floating.rect.height()
            >= f64::from(190.0 + style.tab_bar_height + style.floating_title_height)
                + 2.0 * f64::from(style.floating_border_width)
    );
    assert_eq!(
        dockspace.engine().workspace().item_multiset(),
        BTreeMap::from([(ITEM_A, 1), (ITEM_B, 1)])
    );
    assert!(dockspace.engine().interaction().preview().is_none());

    warm(&context, &mut dockspace, &mut panes);
    let detached_source = tab_point(&dockspace, ITEM_B);
    let main_target = main_center_target_point(&dockspace);
    drag_to(
        &context,
        &mut dockspace,
        &mut panes,
        detached_source,
        main_target,
    );

    assert!(
        dockspace
            .engine()
            .workspace()
            .contained_floating(FLOATING)
            .is_none()
    );
    assert!(dockspace.engine().workspace().root(DETACHED_ROOT).is_none());
    assert_eq!(
        dockspace.engine().workspace().item_multiset(),
        BTreeMap::from([(ITEM_A, 1), (ITEM_B, 1)])
    );
    assert_eq!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    );
}

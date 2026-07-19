use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, RootId, SurfaceId};
use dockspace::scene::SurfaceScene;
use egui::{
    Context, Event, PointerButton, Pos2, RawInput, Rect, Ui, ViewportId, ViewportInfo, vec2,
};
use egui_dockspace::{Dockspace, PaneView};

const ROOT_SURFACE: SurfaceId = SurfaceId::new(1);
const CHILD_SURFACE: SurfaceId = SurfaceId::new(2);
const ROOT_ROOT: RootId = RootId::new(10);
const CHILD_ROOT: RootId = RootId::new(11);
const ROOT_ITEM_A: ItemId = ItemId::new(100);
const ROOT_ITEM_B: ItemId = ItemId::new(101);
const CHILD_ITEM: ItemId = ItemId::new(200);

struct TestPanes;

impl PaneView for TestPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(format!("Pane {}", item.get()).into())
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut Ui) {}
}

fn workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let root_tabs = builder.insert_node(Node::tabs([ROOT_ITEM_A, ROOT_ITEM_B]));
    let child_tabs = builder.insert_node(Node::tabs([CHILD_ITEM]));
    builder.set_root(ROOT_ROOT, RootRecord::new(root_tabs));
    builder.set_root(CHILD_ROOT, RootRecord::new(child_tabs));
    builder.set_surface(ROOT_SURFACE, SurfacePresentation::new(ROOT_ROOT));
    builder.set_surface(CHILD_SURFACE, SurfacePresentation::new(CHILD_ROOT));
    builder.build().expect("two-surface fixture must build")
}

fn input(viewport: ViewportId, events: Vec<Event>) -> RawInput {
    RawInput {
        viewport_id: viewport,
        viewports: [
            (ViewportId::ROOT, ViewportInfo::default()),
            (child_viewport(), ViewportInfo::default()),
        ]
        .into_iter()
        .collect(),
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0))),
        events,
        ..RawInput::default()
    }
}

fn child_viewport() -> ViewportId {
    ViewportId::from_hash_of("multi-surface-child")
}

fn run_surface(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    viewport: ViewportId,
    surface: SurfaceId,
    events: Vec<Event>,
) {
    let _ = context.run_ui(input(viewport, events), |ui| {
        dockspace
            .show(surface, ui, panes)
            .expect("surface frame must advance");
    });
}

fn selected_root_item(dockspace: &Dockspace) -> Option<ItemId> {
    let presentation = dockspace
        .engine()
        .workspace()
        .surface(ROOT_SURFACE)
        .expect("root surface exists");
    let root = dockspace
        .engine()
        .workspace()
        .root(presentation.main_root)
        .expect("root exists");
    match dockspace.engine().workspace().node(root.node) {
        Some(Node::Tabs { selected, .. }) => *selected,
        other => panic!("expected root tabs, got {other:?}"),
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "the scene was projected from finite egui f32 coordinates"
)]
fn root_item_center(dockspace: &Dockspace, item: ItemId) -> Pos2 {
    let ready = match dockspace
        .engine()
        .scene()
        .and_then(|scene| scene.surface(ROOT_SURFACE))
    {
        Some(SurfaceScene::Ready(ready)) => ready,
        other => panic!("expected ready root scene, got {other:?}"),
    };
    let rect = ready
        .tabs()
        .iter()
        .find(|tab| tab.id().item == item)
        .expect("item tab must be projected")
        .rect();
    Pos2::new(
        (rect.x() + rect.width() * 0.5) as f32,
        (rect.y() + rect.height() * 0.5) as f32,
    )
}

fn pointer_button(position: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos: position,
        button: PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn surface_callbacks_publish_one_complete_ready_scene() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("multi-ready", workspace())
        .build()
        .expect("fixture must build");
    let mut panes = TestPanes;

    run_surface(
        &context,
        &mut dockspace,
        &mut panes,
        ViewportId::ROOT,
        ROOT_SURFACE,
        Vec::new(),
    );
    let scene = dockspace.engine().scene().expect("scene must publish");
    assert!(matches!(
        scene.surface(ROOT_SURFACE),
        Some(SurfaceScene::Ready(_))
    ));
    assert!(matches!(
        scene.surface(CHILD_SURFACE),
        Some(SurfaceScene::Bootstrap(_))
    ));

    run_surface(
        &context,
        &mut dockspace,
        &mut panes,
        child_viewport(),
        CHILD_SURFACE,
        Vec::new(),
    );
    let complete_stamp = dockspace.engine().scene().expect("scene exists").stamp();
    assert!(matches!(
        dockspace
            .engine()
            .scene()
            .and_then(|scene| scene.surface(ROOT_SURFACE)),
        Some(SurfaceScene::Ready(_))
    ));
    assert!(matches!(
        dockspace
            .engine()
            .scene()
            .and_then(|scene| scene.surface(CHILD_SURFACE)),
        Some(SurfaceScene::Ready(_))
    ));

    run_surface(
        &context,
        &mut dockspace,
        &mut panes,
        ViewportId::ROOT,
        ROOT_SURFACE,
        Vec::new(),
    );
    assert_eq!(
        dockspace.engine().scene().expect("scene exists").stamp(),
        complete_stamp,
        "an unchanged callback must reuse the complete generation"
    );
}

#[test]
fn another_surface_does_not_reduce_root_render_actions() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("multi-boundary", workspace())
        .build()
        .expect("fixture must build");
    let mut panes = TestPanes;

    run_surface(
        &context,
        &mut dockspace,
        &mut panes,
        ViewportId::ROOT,
        ROOT_SURFACE,
        Vec::new(),
    );
    run_surface(
        &context,
        &mut dockspace,
        &mut panes,
        child_viewport(),
        CHILD_SURFACE,
        Vec::new(),
    );
    run_surface(
        &context,
        &mut dockspace,
        &mut panes,
        ViewportId::ROOT,
        ROOT_SURFACE,
        Vec::new(),
    );
    let target = root_item_center(&dockspace, ROOT_ITEM_B);

    run_surface(
        &context,
        &mut dockspace,
        &mut panes,
        ViewportId::ROOT,
        ROOT_SURFACE,
        vec![Event::PointerMoved(target), pointer_button(target, true)],
    );
    run_surface(
        &context,
        &mut dockspace,
        &mut panes,
        ViewportId::ROOT,
        ROOT_SURFACE,
        vec![pointer_button(target, false)],
    );
    assert_eq!(selected_root_item(&dockspace), Some(ROOT_ITEM_A));

    run_surface(
        &context,
        &mut dockspace,
        &mut panes,
        child_viewport(),
        CHILD_SURFACE,
        Vec::new(),
    );
    assert_eq!(
        selected_root_item(&dockspace),
        Some(ROOT_ITEM_A),
        "a child callback must not consume another surface's pending paint actions"
    );

    run_surface(
        &context,
        &mut dockspace,
        &mut panes,
        ViewportId::ROOT,
        ROOT_SURFACE,
        Vec::new(),
    );
    assert_eq!(selected_root_item(&dockspace), Some(ROOT_ITEM_B));
}

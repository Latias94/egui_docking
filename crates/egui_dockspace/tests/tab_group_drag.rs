use dockspace::command::MovePayload;
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, RootId, SurfaceId};
use dockspace::interaction::InteractionStatus;
use dockspace::scene::SurfaceScene;
use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui, vec2};
use egui_dockspace::{Dockspace, PaneView};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(2);
const ITEM_A: ItemId = ItemId::new(3);
const ITEM_B: ItemId = ItemId::new(4);

struct TestPanes;

impl PaneView for TestPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        match item {
            ITEM_A => Some("First".into()),
            ITEM_B => Some("Second".into()),
            _ => None,
        }
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut Ui) {}
}

fn workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM_A, ITEM_B]));
    builder.set_root(ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::new(ROOT));
    builder.build().expect("fixture workspace is valid")
}

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0))),
        events,
        ..RawInput::default()
    }
}

fn run_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    events: Vec<Event>,
) {
    let _ = context.run_ui(input(events), |ui| {
        dockspace
            .show(SURFACE, ui, panes)
            .expect("fixture frame advances");
    });
}

fn warm(context: &Context, dockspace: &mut Dockspace, panes: &mut TestPanes) {
    run_frame(context, dockspace, panes, Vec::new());
    run_frame(context, dockspace, panes, Vec::new());
}

fn pointer_button(position: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos: position,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "finite scene coordinates are converted to egui's f32 input space"
)]
fn group_grip_center(dockspace: &Dockspace) -> (Pos2, dockspace::ids::NodeId) {
    let SurfaceScene::Ready(ready) = dockspace
        .engine()
        .scene()
        .and_then(|scene| scene.surface(SURFACE))
        .expect("fixture surface is ready")
    else {
        panic!("fixture surface is bootstrapping");
    };
    let bar = ready.tab_bars().first().expect("fixture has one tab bar");
    let rect = bar.rect();
    let extent = rect.height().min(rect.width());
    (
        Pos2::new(
            (rect.min().x() + extent * 0.5) as f32,
            ((rect.min().y() + rect.max().y()) * 0.5) as f32,
        ),
        bar.id().tabs,
    )
}

#[test]
fn explicit_group_grip_arms_the_complete_tabs_stack() {
    let context = Context::default();
    let original = workspace();
    let mut dockspace = Dockspace::builder("tab-group-drag", original.clone())
        .build()
        .expect("fixture facade builds");
    let mut panes = TestPanes;
    warm(&context, &mut dockspace, &mut panes);
    let (grip, tabs) = group_grip_center(&dockspace);
    let moved = grip + vec2(80.0, 40.0);

    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(grip), pointer_button(grip, true)],
    );
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(moved)],
    );
    assert!(matches!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Armed { .. }
    ));

    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(moved)],
    );
    let active = dockspace
        .engine()
        .interaction()
        .active_drag_view()
        .expect("group drag is active");
    let MovePayload::Tabs(source) = active.payload() else {
        panic!("group grip must not degrade to an item or arbitrary-subtree drag");
    };
    assert_eq!(source.root(), ROOT);
    assert_eq!(source.node(), tabs);
    assert_eq!(dockspace.engine().workspace(), &original);

    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerGone],
    );
    run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    );
    assert_eq!(dockspace.engine().workspace(), &original);
}

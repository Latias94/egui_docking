use std::cell::Cell;
use std::rc::Rc;

use dockspace::command::MovePayload;
use dockspace::geometry::LogicalRect;
use dockspace::graph::{Axis, ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use dockspace::interaction::InteractionStatus;
use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui, UiBuilder, vec2};
use egui_dockspace::{ContainedPresentationIds, Dockspace, PaneView, TearOffMode};

const SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(2);
const FLOATING_ROOT: RootId = RootId::new(3);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(4);
const ITEM_A: ItemId = ItemId::new(10);
const ITEM_B: ItemId = ItemId::new(11);
const ITEM_C: ItemId = ItemId::new(12);
const UNUSED_ROOT: RootId = RootId::new(30);
const UNUSED_FLOATING: FloatingPresentationId = FloatingPresentationId::new(31);

struct TestPanes;

impl PaneView for TestPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        match item {
            ITEM_A => Some("Main".into()),
            ITEM_B => Some("Left".into()),
            ITEM_C => Some("Right".into()),
            _ => None,
        }
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut Ui) {}
}

fn workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let main = builder.insert_node(Node::tabs([ITEM_A]));
    let left = builder.insert_node(Node::tabs([ITEM_B]));
    let right = builder.insert_node(Node::tabs([ITEM_C]));
    let floating = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [left, right]).expect("two children form a split"),
    );
    builder.set_root(MAIN_ROOT, RootRecord::new(main));
    builder.set_root(FLOATING_ROOT, RootRecord::new(floating));
    builder.set_surface(SURFACE, SurfacePresentation::new(MAIN_ROOT));
    builder.set_contained_floating(ContainedFloating::new(
        FLOATING,
        FLOATING_ROOT,
        SURFACE,
        LogicalRect::new(120.0, 80.0, 340.0, 230.0).expect("fixture rect is valid"),
        1,
    ));
    builder
        .attach_contained(SURFACE, FLOATING)
        .expect("surface exists");
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
        let mut surface = ui.new_child(
            UiBuilder::new().max_rect(Rect::from_min_size(Pos2::ZERO, vec2(480.0, 320.0))),
        );
        dockspace
            .show(SURFACE, &mut surface, panes)
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
    reason = "finite workspace coordinates are converted to egui's f32 input space"
)]
fn floating_dock_grip_center(dockspace: &Dockspace) -> Pos2 {
    let floating = dockspace
        .engine()
        .workspace()
        .contained_floating(FLOATING)
        .expect("fixture has a contained presentation");
    let style = dockspace.style();
    let inset = style
        .floating_resize_extent
        .min(style.floating_title_height * 0.5);
    let grip_height = style.floating_title_height - 2.0 * inset;
    Pos2::new(
        floating.rect.min().x() as f32
            + style.floating_border_width
            + style.floating_resize_extent
            + grip_height * 0.5,
        floating.rect.min().y() as f32
            + style.floating_border_width
            + style.floating_title_height * 0.5,
    )
}

#[test]
fn explicit_floating_grip_arms_the_complete_split_subtree() {
    let context = Context::default();
    let original = workspace();
    let root_node = original
        .root(FLOATING_ROOT)
        .expect("fixture floating root exists")
        .node;
    let mut dockspace = Dockspace::builder("subtree-drag", original.clone())
        .build()
        .expect("fixture facade builds");
    let mut panes = TestPanes;
    warm(&context, &mut dockspace, &mut panes);
    let grip = floating_dock_grip_center(&dockspace);
    let moved = grip + vec2(100.0, 45.0);

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
        .expect("subtree drag is active");
    let MovePayload::Subtree(source) = active.payload() else {
        panic!("floating docking grip must preserve the complete split subtree");
    };
    assert_eq!(source.root(), FLOATING_ROOT);
    assert_eq!(source.node(), root_node);
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

#[test]
fn complete_contained_root_does_not_allocate_a_new_identity_in_its_current_host() {
    let context = Context::default();
    let original = workspace();
    let allocations = Rc::new(Cell::new(0));
    let observed_allocations = Rc::clone(&allocations);
    let mut dockspace = Dockspace::builder("contained-root-identity", original.clone())
        .tear_off_mode(TearOffMode::Contained)
        .presentation_ids(move || {
            observed_allocations.set(observed_allocations.get() + 1);
            Some(ContainedPresentationIds::new(UNUSED_ROOT, UNUSED_FLOATING))
        })
        .build()
        .expect("fixture facade builds");
    let mut panes = TestPanes;
    warm(&context, &mut dockspace, &mut panes);
    let grip = floating_dock_grip_center(&dockspace);
    let outside_host = Pos2::new(550.0, 210.0);

    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(grip), pointer_button(grip, true)],
    );
    for _ in 0..4 {
        run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(outside_host)],
        );
    }

    let active = dockspace
        .engine()
        .interaction()
        .active_drag_view()
        .expect("whole-root drag remains active");
    assert_eq!(active.tear_off(), None);
    assert_eq!(allocations.get(), 0);
    assert_eq!(dockspace.engine().workspace(), &original);

    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            Event::PointerMoved(outside_host),
            pointer_button(outside_host, false),
        ],
    );
    run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    run_frame(&context, &mut dockspace, &mut panes, Vec::new());

    assert_eq!(allocations.get(), 0);
    assert_eq!(dockspace.engine().workspace(), &original);
    assert_eq!(
        dockspace.engine().interaction().status(),
        InteractionStatus::Idle
    );
}

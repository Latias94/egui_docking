use dockspace::geometry::LogicalRect;
use dockspace::graph::{ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use egui::{Button, Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui, vec2};
use egui_dockspace::{Dockspace, PaneView};

const SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(10);
const FLOATING_ROOT: RootId = RootId::new(11);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(20);
const REAR_ITEM: ItemId = ItemId::new(100);
const FRONT_ITEM: ItemId = ItemId::new(101);

#[derive(Default)]
struct TestPanes {
    rear_clicks: usize,
}

impl PaneView for TestPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(format!("Pane {}", item.get()).into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) {
        if item == REAR_ITEM
            && ui
                .put(rear_button_rect(), Button::new("Rear action"))
                .clicked()
        {
            self.rear_clicks += 1;
        }
    }
}

fn workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let main = builder.insert_node(Node::tabs([REAR_ITEM]));
    let floating = builder.insert_node(Node::tabs([FRONT_ITEM]));
    builder.set_root(MAIN_ROOT, RootRecord::new(main));
    builder.set_root(FLOATING_ROOT, RootRecord::new(floating));
    builder.set_surface(SURFACE, SurfacePresentation::new(MAIN_ROOT));
    builder.set_contained_floating(ContainedFloating::new(
        FLOATING,
        FLOATING_ROOT,
        SURFACE,
        LogicalRect::new(180.0, 80.0, 240.0, 200.0).expect("floating rect is valid"),
        1,
    ));
    builder
        .attach_contained(SURFACE, FLOATING)
        .expect("surface exists");
    builder.build().expect("occlusion fixture is valid")
}

fn rear_button_rect() -> Rect {
    Rect::from_min_size(Pos2::new(250.0, 175.0), vec2(90.0, 32.0))
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
        dockspace
            .show(SURFACE, ui, panes)
            .expect("occlusion frame advances");
    });
}

#[test]
fn front_floating_blank_content_blocks_rear_pane_widgets() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("contained-widget-occlusion", workspace())
        .build()
        .expect("dockspace builds");
    let mut panes = TestPanes::default();
    run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    run_frame(&context, &mut dockspace, &mut panes, Vec::new());

    let pointer = rear_button_rect().center();
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(pointer), pointer_button(pointer, true)],
    );
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(pointer), pointer_button(pointer, false)],
    );

    assert_eq!(panes.rear_clicks, 0);
}

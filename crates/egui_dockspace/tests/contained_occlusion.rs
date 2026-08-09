use dockspace::geometry::LogicalRect;
use egui::{Button, Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui, vec2};
use egui_dockspace::{
    Dockspace, DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, FloatingPresentationId, ItemId, PaneView, RootId, SurfaceId,
};

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

fn layout() -> DockspaceLayout {
    let contained = DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs([FRONT_ITEM])),
        LogicalRect::new(180.0, 80.0, 240.0, 200.0).expect("floating rect is valid"),
    );
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(MAIN_ROOT, DockspaceNode::tabs([REAR_ITEM])),
    )
    .with_contained(contained)])
    .expect("occlusion fixture is valid")
}

fn rear_button_rect() -> Rect {
    Rect::from_min_size(Pos2::new(250.0, 175.0), vec2(90.0, 32.0))
}

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0))),
        events: events.into_iter().map(Into::into).collect(),
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

fn run_ui_without_renderer(
    context: &Context,
    input: RawInput,
    run_ui: impl FnMut(&mut Ui),
) -> egui::FullOutput {
    let mut output = context.run_ui(input, run_ui);
    output.textures_delta.clear();
    output
}

fn run_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    events: Vec<Event>,
) {
    let _ = run_ui_without_renderer(context, input(events), |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, panes)
            .expect("occlusion frame advances");
    });
}

#[test]
fn front_floating_blank_content_blocks_rear_pane_widgets() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("contained-widget-occlusion", layout())
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

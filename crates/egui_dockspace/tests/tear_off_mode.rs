use egui::{Context, RawInput, Rect, Ui, vec2};
use egui_dockspace::{
    Dockspace, DockspaceCapability, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, DockspaceUnavailableReason, ItemId, PaneView, RootId, SurfaceId,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(2);
const ITEM: ItemId = ItemId::new(3);

struct TestPane;

fn run_ui_without_renderer(
    context: &Context,
    input: RawInput,
    run_ui: impl FnMut(&mut Ui),
) -> egui::FullOutput {
    let mut output = context.run_ui(input, run_ui);
    output.textures_delta.clear();
    output
}

impl PaneView for TestPane {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        (item == ITEM).then(|| "Document".into())
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut Ui) {}
}

fn layout() -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::tabs([ITEM])),
    )])
    .expect("fixture layout is valid")
}

fn capability(dockspace: &mut Dockspace, context: &Context) -> DockspaceCapability {
    let mut pane = TestPane;
    let mut observed = None;
    let _ = run_ui_without_renderer(
        context,
        RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(600.0, 400.0))),
            ..RawInput::default()
        },
        |ui| {
            observed = Some(
                dockspace
                    .show_single_surface(SURFACE, ui, &mut pane)
                    .expect("fixture frame advances")
                    .contained_capability(),
            );
        },
    );
    observed.expect("egui executes one or more passes")
}

#[test]
fn callback_only_contained_tear_off_requires_presentation_settlement() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("contained-tear-off", layout())
        .build()
        .expect("fixture facade builds");

    assert_eq!(
        capability(&mut dockspace, &context),
        DockspaceCapability::Unavailable(
            DockspaceUnavailableReason::PresentationSettlementRequired
        )
    );
}

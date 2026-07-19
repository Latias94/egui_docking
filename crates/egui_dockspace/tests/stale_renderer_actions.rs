use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, RootId, SurfaceId};
use dockspace::scene::SurfaceScene;
use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui, vec2};
use egui_dockspace::{Dockspace, DockspaceInputRejection, PaneCloseResponse, PaneView};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(2);
const ITEM: ItemId = ItemId::new(3);

struct TestPane {
    close_calls: usize,
}

impl PaneView for TestPane {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        (item == ITEM).then(|| "Document".into())
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut Ui) {}

    fn close(&mut self, _item: ItemId) -> PaneCloseResponse {
        self.close_calls += 1;
        PaneCloseResponse::Allow
    }
}

fn workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM]));
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

fn run(
    context: &Context,
    dockspace: &mut Dockspace,
    pane: &mut TestPane,
    events: Vec<Event>,
) -> Vec<DockspaceInputRejection> {
    let mut rejections = Vec::new();
    let _ = context.run_ui(input(events), |ui| {
        let response = dockspace
            .show(SURFACE, ui, pane)
            .expect("fixture frame advances");
        rejections.extend_from_slice(response.input_rejections());
    });
    rejections
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
fn close_center(dockspace: &Dockspace) -> Pos2 {
    let SurfaceScene::Ready(ready) = dockspace
        .engine()
        .scene()
        .and_then(|scene| scene.surface(SURFACE))
        .expect("fixture surface is ready")
    else {
        panic!("fixture surface is not bootstrapping");
    };
    let tab = ready.tabs().first().expect("fixture has one tab").rect();
    let style = dockspace.style();
    let close_size = f64::from(style.tab_close_size)
        .min(tab.width())
        .min(tab.height());
    Pos2::new(
        (tab.max().x() - f64::from(style.tab_horizontal_padding) - close_size * 0.5) as f32,
        ((tab.min().y() + tab.max().y()) * 0.5) as f32,
    )
}

#[test]
fn restored_epoch_rejects_prior_paint_actions_before_close_callback() {
    let context = Context::default();
    let original = workspace();
    let mut dockspace = Dockspace::builder("stale-actions", original.clone())
        .build()
        .expect("fixture facade builds");
    let mut pane = TestPane { close_calls: 0 };

    run(&context, &mut dockspace, &mut pane, Vec::new());
    run(&context, &mut dockspace, &mut pane, Vec::new());
    let close = close_center(&dockspace);
    run(
        &context,
        &mut dockspace,
        &mut pane,
        vec![Event::PointerMoved(close), pointer_button(close, true)],
    );
    run(
        &context,
        &mut dockspace,
        &mut pane,
        vec![Event::PointerMoved(close), pointer_button(close, false)],
    );
    assert_eq!(pane.close_calls, 0);

    dockspace
        .replace_workspace(original.clone())
        .expect("replacement queues before the renderer boundary");
    let rejections = run(&context, &mut dockspace, &mut pane, Vec::new());

    assert_eq!(pane.close_calls, 0);
    assert_eq!(dockspace.engine().workspace(), &original);
    assert!(matches!(
        rejections.as_slice(),
        [DockspaceInputRejection::StaleWorkspace {
            dropped_actions: 1,
            ..
        }]
    ));
}

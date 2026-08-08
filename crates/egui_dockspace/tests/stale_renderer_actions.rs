use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, RootId, SurfaceId};
use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui, vec2};
use egui_dockspace::{Dockspace, PaneView};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(2);
const ITEM: ItemId = ItemId::new(3);

struct TestPane;

impl PaneView for TestPane {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        (item == ITEM).then(|| "Document".into())
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut Ui) {}
}

fn workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM]));
    builder.set_root(ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.build().expect("fixture workspace is valid")
}

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0))),
        events: events.into_iter().map(Into::into).collect(),
        ..RawInput::default()
    }
}

fn run(
    context: &Context,
    dockspace: &mut Dockspace,
    pane: &mut TestPane,
    events: Vec<Event>,
) -> usize {
    let mut close_requests = 0;
    let _ = crate::test_support::run_ui(context, input(events), |ui| {
        let response = dockspace
            .show_single_surface(SURFACE, ui, pane)
            .expect("fixture frame advances");
        close_requests += response.close_requests().count();
    });
    close_requests
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
    let painted = dockspace
        .core_engine()
        .interaction_projection(SURFACE)
        .expect("fixture surface must have acknowledged painted geometry");
    let tab = painted
        .plan()
        .tab_records()
        .first()
        .and_then(|tab| tab.close_bounds())
        .expect("fixture has one closeable tab");
    Pos2::new(
        ((tab.min().x() + tab.max().x()) * 0.5) as f32,
        ((tab.min().y() + tab.max().y()) * 0.5) as f32,
    )
}

#[test]
fn restored_epoch_rejects_prior_scene_close_release_without_a_request() {
    let context = Context::default();
    let original = workspace();
    let mut dockspace = Dockspace::builder("stale-actions", original.clone())
        .build()
        .expect("fixture facade builds");
    let mut pane = TestPane;

    assert_eq!(run(&context, &mut dockspace, &mut pane, Vec::new()), 0);
    assert_eq!(run(&context, &mut dockspace, &mut pane, Vec::new()), 0);
    let close = close_center(&dockspace);
    assert_eq!(
        run(
            &context,
            &mut dockspace,
            &mut pane,
            vec![Event::PointerMoved(close), pointer_button(close, true)],
        ),
        0,
        "pressing a close control must not open a close plan"
    );

    let version_before_replacement = dockspace.core_engine().version();
    dockspace
        .replace_workspace(original.clone())
        .expect("replacement commits before the release renderer boundary");
    assert_ne!(
        dockspace.core_engine().version(),
        version_before_replacement
    );
    let close_requests = run(
        &context,
        &mut dockspace,
        &mut pane,
        vec![Event::PointerMoved(close), pointer_button(close, false)],
    );

    assert_eq!(
        close_requests, 0,
        "a release captured from the prior scene must not reach application close decisions"
    );
    assert_eq!(dockspace.core_engine().workspace(), &original);
}

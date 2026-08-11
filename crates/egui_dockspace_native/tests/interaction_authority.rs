use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{DockspaceReceiverRole, DockspaceSession, UniformSurfaceMetrics};
use eframe::egui::{
    Context, Event, Id, Modifiers, PointerButton, Pos2, RawInput, Rect, Sense, Ui, vec2,
};
use egui_dockspace::{DockStyle, PaneView, native_support};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);

struct Panes;

impl PaneView for Panes {
    fn title(&self, item: ItemId) -> Option<eframe::egui::WidgetText> {
        match item {
            FIRST => Some("First".into()),
            SECOND => Some("Second".into()),
            _ => None,
        }
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) {
        ui.allocate_rect(ui.max_rect(), Sense::hover());
        ui.label(format!("Pane {}", item.get()));
    }
}

fn session() -> DockspaceSession {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, SECOND])),
    )])
    .expect("native interaction layout validates");
    DockspaceSession::from_layout(layout, DockPolicy::default())
        .expect("native interaction session initializes")
}

fn install_ready_candidate(session: &mut DockspaceSession) {
    let metrics = UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 800.0, 600.0).expect("test bounds validate"),
        LogicalSize::new(64.0, 48.0).expect("test minimum validates"),
        96.0,
    )
    .expect("test metrics validate");
    let mut frame = session
        .begin_host_frame()
        .expect("measurement frame begins");
    frame
        .measure_surface(SURFACE, metrics)
        .expect("uniform surface measurement succeeds");
    frame.commit().expect("measurement frame commits");
}

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
        events,
        ..RawInput::default()
    }
}

fn run_surface_frame(
    context: &Context,
    session: &mut DockspaceSession,
    panes: &mut Panes,
    events: Vec<Event>,
) -> (
    eframe::egui::FullOutput,
    Option<Pos2>,
    Vec<native_support::NativePaintReceiver>,
    Option<(eframe::egui::ViewportId, u64)>,
) {
    let mut second_tab_center = None;
    let mut receivers = Vec::new();
    let mut receiver_generation = None;
    let mut output = context.run_ui(input(events), |ui| {
        let mut frame = session.begin_host_frame().expect("host frame begins");
        second_tab_center = frame
            .paint_plan(SURFACE)
            .expect("paint plan lookup succeeds")
            .and_then(|plan| {
                plan.tabs()
                    .find(|tab| tab.item() == SECOND)
                    .map(|tab| logical_center(tab.drag_bounds()))
            });
        let painted = native_support::paint_surface(
            &mut frame,
            Id::new("native-interaction-authority"),
            SURFACE,
            ui,
            panes,
            &DockStyle::default(),
        )
        .expect("native surface paints");
        receivers.extend(painted.receivers());
        receiver_generation = Some((painted.viewport_id(), painted.cumulative_pass_nr()));
        if painted.deferred_measurement() {
            native_support::defer_unpainted_surfaces(&mut frame)
                .expect("deferred surface settlement succeeds");
        } else {
            native_support::measure_surface(&mut frame, SURFACE, ui, panes, &DockStyle::default())
                .expect("native surface measures");
        }
        frame.commit().expect("host frame commits");
    });
    output.textures_delta.clear();
    (output, second_tab_center, receivers, receiver_generation)
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "validated finite logical test geometry is converted into egui's f32 input space"
)]
fn logical_center(bounds: dockspace::geometry::LogicalRect) -> Pos2 {
    Pos2::new(
        (bounds.x() + bounds.width() * 0.5) as f32,
        (bounds.y() + bounds.height() * 0.5) as f32,
    )
}

#[test]
fn external_pointer_journal_suppresses_local_response_actions() {
    let context = Context::default();
    let mut session = session();
    let mut panes = Panes;
    install_ready_candidate(&mut session);

    let (_, pointer, _, _) = run_surface_frame(&context, &mut session, &mut panes, Vec::new());
    let pointer = pointer.expect("stable paint plan contains the second tab");

    let _ = run_surface_frame(
        &context,
        &mut session,
        &mut panes,
        vec![
            Event::PointerMoved(pointer),
            Event::PointerButton {
                pos: pointer,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    let _ = run_surface_frame(
        &context,
        &mut session,
        &mut panes,
        vec![
            Event::PointerMoved(pointer),
            Event::PointerButton {
                pos: pointer,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ],
    );

    assert!(
        session
            .view()
            .item(FIRST)
            .expect("first item remains open")
            .is_selected(),
        "native rendering must not submit a second local pointer action"
    );
}

#[test]
fn completed_pass_hit_maps_to_the_exact_tab_receiver() {
    let context = Context::default();
    let mut session = session();
    let mut panes = Panes;
    install_ready_candidate(&mut session);

    let (_, pointer, receivers, generation) =
        run_surface_frame(&context, &mut session, &mut panes, Vec::new());
    let pointer = pointer.expect("stable paint plan contains the second tab");
    let (viewport, expected_pass) = generation.expect("ready paint reports its pass identity");
    let hits = context
        .hit_test_last_pass(viewport, pointer)
        .expect("the viewport completed the painted pass");
    assert_eq!(hits.cumulative_pass_nr(), expected_pass);

    let drag = hits.drag().expect("the tab owns the drag lane");
    let receiver = receivers
        .iter()
        .copied()
        .find(|receiver| {
            receiver.widget_id() == drag.id() && receiver.layer_id() == drag.layer_id()
        })
        .expect("the completed-pass identity has an exact dockspace binding");
    assert_eq!(receiver.receiver().role(), DockspaceReceiverRole::TabBody);
}

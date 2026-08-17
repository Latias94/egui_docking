use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceReceiverRole, DockspaceSession, DockspaceVisualId, UniformSurfaceMetrics,
};
use eframe::egui::emath::GuiRounding;
use eframe::egui::{
    Context, Event, Id, InputState, Modifiers, PointerButton, Pos2, RawInput, Rect, Sense, Ui, vec2,
};
use eframe::egui::{
    WidgetHitIdentity, WidgetScrollDelta, WidgetScrollHit, WidgetScrollHitChallenge,
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

struct SurfaceFrameResult {
    pane_center: Option<Pos2>,
    second_tab_center: Option<Pos2>,
    second_tab_receiver: Option<DockspaceVisualId>,
    group_grip_center: Option<Pos2>,
    tab_strip_scroll_center: Option<Pos2>,
    receivers: Vec<native_support::NativePaintReceiver>,
    scroll_receivers: Vec<native_support::NativeScrollPaintReceiver>,
    scroll_identities: Vec<WidgetHitIdentity>,
    receiver_generation: Option<(eframe::egui::ViewportId, u64)>,
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

fn empty_session() -> DockspaceSession {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([])),
    )])
    .expect("empty native interaction layout validates");
    DockspaceSession::from_layout(layout, DockPolicy::default())
        .expect("empty native interaction session initializes")
}

fn overflow_session() -> DockspaceSession {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs((1..=24).map(ItemId::new))),
    )])
    .expect("overflow native interaction layout validates");
    DockspaceSession::from_layout(layout, DockPolicy::default())
        .expect("overflow native interaction session initializes")
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
) -> SurfaceFrameResult {
    let mut pane_center = None;
    let mut second_tab_center = None;
    let mut second_tab_receiver = None;
    let mut group_grip_center = None;
    let mut tab_strip_scroll_center = None;
    let mut receivers = Vec::new();
    let mut scroll_receivers = Vec::new();
    let mut scroll_identities = Vec::new();
    let mut receiver_generation = None;
    let mut output = context.run_ui(input(events), |ui| {
        let dock_rect = ui.available_rect_before_wrap();
        let popup_rect = ui.ctx().input(InputState::content_rect).round_ui();
        let mut frame = session.begin_host_frame().expect("host frame begins");
        if let Some(plan) = frame
            .paint_plan(SURFACE)
            .expect("paint plan lookup succeeds")
        {
            pane_center = plan
                .panes()
                .next()
                .map(|pane| logical_center(pane.content_bounds()));
            if let Some(tab) = plan.tabs().find(|tab| tab.item() == SECOND) {
                second_tab_center = Some(logical_center(tab.drag_bounds()));
                second_tab_receiver = plan
                    .receiver_for_tab_body(tab)
                    .map(|receiver| receiver.visual_id());
            }
            group_grip_center = plan
                .tab_bars()
                .find_map(|bar| bar.group_grip_bounds().map(logical_center));
            tab_strip_scroll_center = plan.tab_bars().find_map(|bar| {
                plan.receiver_for_tab_strip_scroll(bar)
                    .map(|receiver| logical_center(receiver.bounds()))
            });
        }
        let mut register_scroll_candidate = |ui: &Ui, rect: Rect, id: Id| {
            let identity = ui.register_scroll_hit_candidate(rect, id);
            scroll_identities.push(identity);
            (identity.id(), identity.layer_id())
        };
        let painted = native_support::paint_surface(
            &mut frame,
            Id::new("native-interaction-authority"),
            SURFACE,
            ui,
            panes,
            &DockStyle::default(),
            &mut register_scroll_candidate,
        )
        .expect("native surface paints");
        receivers = painted.receivers().collect();
        scroll_receivers = painted.scroll_receivers().collect();
        receiver_generation = painted
            .had_ready_plan()
            .then_some((painted.viewport_id(), painted.cumulative_pass_nr()));
        if painted.deferred_measurement() {
            native_support::defer_unpainted_surfaces(&mut frame)
                .expect("deferred surface settlement succeeds");
        } else {
            native_support::measure_surface(
                &mut frame,
                SURFACE,
                ui,
                dock_rect,
                popup_rect,
                panes,
                &DockStyle::default(),
            )
            .expect("native surface measures");
        }
        frame.commit().expect("host frame commits");
    });
    output.textures_delta.clear();
    SurfaceFrameResult {
        pane_center,
        second_tab_center,
        second_tab_receiver,
        group_grip_center,
        tab_strip_scroll_center,
        receivers,
        scroll_receivers,
        scroll_identities,
        receiver_generation,
    }
}

fn run_ready_surface_frame(
    context: &Context,
    session: &mut DockspaceSession,
    panes: &mut Panes,
) -> SurfaceFrameResult {
    for _ in 0..3 {
        let painted = run_surface_frame(context, session, panes, Vec::new());
        if painted.receiver_generation.is_some() {
            return painted;
        }
    }
    panic!("surface did not return to a ready paint plan");
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

    let painted = run_ready_surface_frame(&context, &mut session, &mut panes);
    let pointer = painted
        .second_tab_center
        .expect("stable paint plan contains the second tab");

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

    let painted = run_ready_surface_frame(&context, &mut session, &mut panes);
    let pointer = painted
        .second_tab_center
        .expect("stable paint plan contains the second tab");
    let (viewport, expected_pass) = painted
        .receiver_generation
        .expect("ready paint reports its pass identity");
    let hits = context
        .hit_test_last_pass(viewport, pointer)
        .expect("the viewport completed the painted pass");
    assert_eq!(hits.cumulative_pass_nr(), expected_pass);

    let drag = hits.drag().expect("the tab owns the drag lane");
    let receiver = painted
        .receivers
        .iter()
        .copied()
        .find(|receiver| {
            receiver.viewport_id() == viewport
                && receiver.cumulative_pass_nr() == expected_pass
                && receiver.widget_id() == drag.id()
                && receiver.layer_id() == drag.layer_id()
        })
        .expect("the completed-pass identity has an exact dockspace binding");
    assert_eq!(receiver.role(), DockspaceReceiverRole::TabBody);
    assert_eq!(
        Some(receiver.visual_id()),
        painted.second_tab_receiver,
        "the hit must bind the exact second tab, not merely another tab role"
    );
}

#[test]
fn detached_receiver_bindings_remain_generation_qualified() {
    let context = Context::default();
    let mut panes = Panes;
    let mut first_session = session();
    install_ready_candidate(&mut first_session);
    let first = run_ready_surface_frame(&context, &mut first_session, &mut panes);

    let mut second_session = session();
    install_ready_candidate(&mut second_session);
    let second = run_ready_surface_frame(&context, &mut second_session, &mut panes);
    let (viewport, first_pass) = first
        .receiver_generation
        .expect("the first pass reports its identity");
    let (second_viewport, second_pass) = second
        .receiver_generation
        .expect("the second pass reports its identity");
    assert_eq!(second_viewport, viewport);
    assert!(second_pass > first_pass);
    assert!(first.receivers.iter().all(|receiver| {
        receiver.viewport_id() == viewport && receiver.cumulative_pass_nr() == first_pass
    }));
    assert!(second.receivers.iter().all(|receiver| {
        receiver.viewport_id() == viewport && receiver.cumulative_pass_nr() == second_pass
    }));
}

#[test]
fn completed_pass_hit_maps_to_the_exact_tab_group_receiver() {
    let context = Context::default();
    let mut session = session();
    let mut panes = Panes;
    install_ready_candidate(&mut session);

    let painted = run_ready_surface_frame(&context, &mut session, &mut panes);
    let pointer = painted
        .group_grip_center
        .expect("the tab bar exposes a group drag grip");
    let (viewport, expected_pass) = painted
        .receiver_generation
        .expect("ready paint reports its pass identity");
    let hits = context
        .hit_test_last_pass(viewport, pointer)
        .expect("the viewport completed the painted pass");
    assert_eq!(hits.cumulative_pass_nr(), expected_pass);

    let drag = hits.drag().expect("the group grip owns the drag lane");
    let receiver = painted
        .receivers
        .iter()
        .copied()
        .find(|receiver| {
            receiver.viewport_id() == viewport
                && receiver.cumulative_pass_nr() == expected_pass
                && receiver.widget_id() == drag.id()
                && receiver.layer_id() == drag.layer_id()
        })
        .expect("the completed-pass identity has an exact group binding");
    assert_eq!(receiver.role(), DockspaceReceiverRole::TabGroupGrip);
}

#[test]
fn completed_pass_scroll_hit_maps_to_the_exact_tab_strip_receiver() {
    let context = Context::default();
    let mut session = overflow_session();
    let mut panes = Panes;
    install_ready_candidate(&mut session);

    let painted = run_ready_surface_frame(&context, &mut session, &mut panes);
    let point = painted
        .tab_strip_scroll_center
        .expect("the overflowing tab strip exposes a scroll receiver");
    let (viewport, expected_pass) = painted
        .receiver_generation
        .expect("ready paint reports its pass identity");
    assert_eq!(
        painted.scroll_receivers.len(),
        painted.scroll_identities.len(),
        "every product scroll receiver retains its fork-owned identity"
    );
    let (expected_identity, expected_receiver) = painted
        .scroll_identities
        .iter()
        .copied()
        .zip(painted.scroll_receivers.iter().copied())
        .find(|(_, receiver)| receiver.role() == DockspaceReceiverRole::TabStripScroll)
        .expect("one exact tab-strip scroll binding is painted");
    assert_eq!(expected_receiver.viewport_id(), viewport);
    assert_eq!(expected_receiver.cumulative_pass_nr(), expected_pass);
    assert_eq!(expected_receiver.widget_id(), expected_identity.id());
    assert_eq!(expected_receiver.layer_id(), expected_identity.layer_id());

    let hit = context
        .scroll_hit_test_last_pass(
            viewport,
            point,
            painted.scroll_identities.as_slice(),
            WidgetScrollHitChallenge::Spatial {
                projected_delta: Some(
                    WidgetScrollDelta::new(0.0, -1.0).expect("test delta is finite"),
                ),
            },
        )
        .expect("the viewport completed the painted pass");
    assert_eq!(hit.cumulative_pass_nr(), expected_pass);
    assert_eq!(hit.hit(), WidgetScrollHit::Candidate(expected_identity));
}

#[test]
fn empty_central_leaf_registers_its_pane_receiver() {
    let context = Context::default();
    let mut session = empty_session();
    let mut panes = Panes;
    install_ready_candidate(&mut session);

    let painted = run_ready_surface_frame(&context, &mut session, &mut panes);
    let pointer = painted
        .pane_center
        .expect("the empty central leaf still has pane geometry");
    let (viewport, expected_pass) = painted
        .receiver_generation
        .expect("ready paint reports its pass identity");
    let hits = context
        .hit_test_last_pass(viewport, pointer)
        .expect("the viewport completed the painted pass");
    let drag = hits.drag().expect("the empty pane owns the drag lane");
    let receiver = painted
        .receivers
        .iter()
        .copied()
        .find(|receiver| {
            receiver.viewport_id() == viewport
                && receiver.cumulative_pass_nr() == expected_pass
                && receiver.widget_id() == drag.id()
                && receiver.layer_id() == drag.layer_id()
        })
        .expect("the empty pane hit has an exact dockspace binding");
    assert_eq!(receiver.role(), DockspaceReceiverRole::PaneBody);
}

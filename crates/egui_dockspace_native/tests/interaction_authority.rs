use std::collections::BTreeMap;

use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceReceiverRole, DockspaceSession, DockspaceVisualId, SurfacePaintProgramOperation,
    SurfacePaintProgramStep, UniformSurfaceMetrics,
};
use eframe::egui::emath::GuiRounding;
use eframe::egui::{
    Area, Button, Color32, Context, CursorIcon, Event, Id, InputState, Modifiers, Order,
    PointerButton, Pos2, RawInput, Rect, Response, Sense, TextEdit, Ui, vec2,
};
use eframe::egui::{
    WidgetHitIdentity, WidgetScrollDelta, WidgetScrollHit, WidgetScrollHitChallenge,
};
use egui_dockspace::{DockStyle, PaneResponse, PaneView, native_support};

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

    fn ui(&mut self, item: ItemId, ui: &mut Ui) -> PaneResponse {
        ui.allocate_rect(ui.max_rect(), Sense::hover());
        ui.label(format!("Pane {}", item.get()));
        PaneResponse::default()
    }
}

#[derive(Default)]
struct ContentPane {
    text: String,
    widgets: Vec<Response>,
    declared_widgets: Vec<Id>,
}

struct ContentPanes {
    panes: BTreeMap<ItemId, ContentPane>,
    declare_widgets: bool,
}

impl ContentPanes {
    fn new(declare_widgets: bool) -> Self {
        Self {
            panes: [FIRST, SECOND]
                .into_iter()
                .map(|item| (item, ContentPane::default()))
                .collect(),
            declare_widgets,
        }
    }
}

impl PaneView for ContentPanes {
    fn title(&self, item: ItemId) -> Option<eframe::egui::WidgetText> {
        self.panes
            .contains_key(&item)
            .then(|| format!("Content {}", item.get()).into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) -> PaneResponse {
        let pane = self.panes.get_mut(&item).expect("the rendered pane exists");
        pane.widgets.clear();
        pane.declared_widgets.clear();

        let text = ui.add_sized(
            vec2(240.0, 28.0),
            TextEdit::singleline(&mut pane.text).id(ui.make_persistent_id((item, "text"))),
        );
        let button = ui.add_sized(vec2(240.0, 28.0), Button::new("Content action"));
        let (canvas_rect, canvas) = ui.allocate_exact_size(vec2(240.0, 80.0), Sense::click());
        ui.painter()
            .rect_filled(canvas_rect, 0.0, Color32::DARK_BLUE);
        ui.painter()
            .circle_filled(canvas_rect.center(), 8.0, Color32::LIGHT_BLUE);
        pane.widgets.extend([text, button, canvas]);

        let content = if self.declare_widgets {
            PaneResponse::default()
                .with_content_ids(pane.widgets.iter().map(|response| response.id))
        } else {
            PaneResponse::default()
        };
        pane.declared_widgets.extend(content.content_widgets());
        content
    }
}

struct SurfaceFrameResult {
    pane_center: Option<Pos2>,
    pane_receiver: Option<DockspaceVisualId>,
    second_tab_center: Option<Pos2>,
    second_tab_receiver: Option<DockspaceVisualId>,
    group_grip_center: Option<Pos2>,
    tab_strip_scroll_center: Option<Pos2>,
    receivers: Vec<native_support::NativePaintReceiver>,
    scroll_receivers: Vec<native_support::NativeScrollPaintReceiver>,
    scroll_identities: Vec<WidgetHitIdentity>,
    receiver_generation: Option<(eframe::egui::ViewportId, u64)>,
    cursor_icon: CursorIcon,
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

fn run_surface_frame<P: PaneView>(
    context: &Context,
    session: &mut DockspaceSession,
    panes: &mut P,
    events: Vec<Event>,
) -> SurfaceFrameResult {
    run_surface_frame_with_overlay(context, session, panes, events, |_, _| {})
}

fn run_surface_frame_with_overlay<P: PaneView>(
    context: &Context,
    session: &mut DockspaceSession,
    panes: &mut P,
    events: Vec<Event>,
    mut paint_overlay: impl FnMut(&Ui, &P),
) -> SurfaceFrameResult {
    let mut pane_center = None;
    let mut pane_receiver = None;
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
            let mut pane = None;
            let mut second_tab = None;
            let mut bars = Vec::new();
            plan.visit_surface_program(|step| {
                if let SurfacePaintProgramStep::Operation { operation, .. } = step {
                    match operation {
                        SurfacePaintProgramOperation::Pane(record) => {
                            assert!(pane.replace(record).is_none());
                        }
                        SurfacePaintProgramOperation::Tab(record) if record.item() == SECOND => {
                            assert!(second_tab.replace(record).is_none());
                        }
                        SurfacePaintProgramOperation::TabBar(record) => bars.push(record),
                        SurfacePaintProgramOperation::Tab(_)
                        | SurfacePaintProgramOperation::Splitter(_)
                        | SurfacePaintProgramOperation::SplitterJunction(_)
                        | SurfacePaintProgramOperation::TabStripControl(_)
                        | SurfacePaintProgramOperation::ContainedControls(_)
                        | SurfacePaintProgramOperation::PresentationMenuAnchor(_) => {}
                    }
                }
                Ok::<(), ()>(())
            })
            .expect("interaction authority paint program resolves");
            pane_center = pane.map(|pane| logical_center(pane.content_bounds()));
            pane_receiver = pane
                .and_then(|pane| plan.receiver_for_pane(pane))
                .map(|receiver| receiver.visual_id());
            if let Some(tab) = second_tab {
                second_tab_center = Some(logical_center(tab.drag_bounds()));
                second_tab_receiver = plan
                    .receiver_for_tab_body(tab)
                    .map(|receiver| receiver.visual_id());
            }
            group_grip_center = bars
                .iter()
                .copied()
                .find_map(|bar| bar.group_grip_bounds().map(logical_center));
            tab_strip_scroll_center = bars.into_iter().find_map(|bar| {
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
            Id::unique("native-interaction-authority"),
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
        paint_overlay(ui, panes);
    });
    let cursor_icon = output.platform_output.cursor_icon;
    output.textures_delta.clear();
    SurfaceFrameResult {
        pane_center,
        pane_receiver,
        second_tab_center,
        second_tab_receiver,
        group_grip_center,
        tab_strip_scroll_center,
        receivers,
        scroll_receivers,
        scroll_identities,
        receiver_generation,
        cursor_icon,
    }
}

fn run_ready_surface_frame<P: PaneView>(
    context: &Context,
    session: &mut DockspaceSession,
    panes: &mut P,
) -> SurfaceFrameResult {
    for _ in 0..3 {
        let painted = run_surface_frame(context, session, panes, Vec::new());
        if painted.receiver_generation.is_some() {
            return painted;
        }
    }
    panic!("surface did not return to a ready paint plan");
}

fn matching_receivers(
    painted: &SurfaceFrameResult,
    identity: WidgetHitIdentity,
) -> Vec<native_support::NativePaintReceiver> {
    let (viewport, pass) = painted
        .receiver_generation
        .expect("the ready paint reports its exact generation");
    painted
        .receivers
        .iter()
        .copied()
        .filter(|receiver| {
            receiver.viewport_id() == viewport
                && receiver.cumulative_pass_nr() == pass
                && receiver.widget_id() == identity.id()
                && receiver.layer_id() == identity.layer_id()
        })
        .collect()
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
fn external_pointer_journal_does_not_retain_an_egui_drag_owner_after_pointer_gone() {
    let context = Context::default();
    let mut session = session();
    let mut panes = Panes;
    install_ready_candidate(&mut session);

    let painted = run_ready_surface_frame(&context, &mut session, &mut panes);
    let source = painted
        .second_tab_center
        .expect("stable paint plan contains the second tab");
    let moved = source + vec2(24.0, 8.0);
    let _ = run_surface_frame(
        &context,
        &mut session,
        &mut panes,
        vec![
            Event::PointerMoved(source),
            Event::PointerButton {
                pos: source,
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
        vec![Event::PointerMoved(moved)],
    );
    assert_eq!(
        context.dragged_id(),
        None,
        "the external journal keeps the drag lane without a second egui owner"
    );

    let cancelled = run_surface_frame(&context, &mut session, &mut panes, vec![Event::PointerGone]);
    assert_eq!(context.dragged_id(), None);
    assert_ne!(
        cancelled.cursor_icon,
        CursorIcon::Grabbing,
        "capture loss must not leave stale docking drag feedback"
    );
    let (viewport, pass) = cancelled
        .receiver_generation
        .expect("the cancelled frame still publishes exact receiver identities");
    let hit = context
        .hit_test_last_pass(viewport, source)
        .expect("the cancelled viewport completed its pass");
    assert_eq!(hit.cumulative_pass_nr(), pass);
    let drag = hit
        .drag()
        .expect("the external journal retains the next drag-delivery lane");
    assert!(cancelled.receivers.iter().any(|receiver| {
        receiver.widget_id() == drag.id() && receiver.layer_id() == drag.layer_id()
    }));
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

#[test]
fn declared_content_widgets_map_completed_pass_hits_to_the_exact_pane_receiver() {
    let context = Context::default();
    let mut session = session();
    let mut panes = ContentPanes::new(true);
    install_ready_candidate(&mut session);

    let painted = run_ready_surface_frame(&context, &mut session, &mut panes);
    let (viewport, expected_pass) = painted
        .receiver_generation
        .expect("the ready paint reports its exact generation");
    let expected_pane = painted
        .pane_receiver
        .expect("the paint plan contains the selected pane receiver");
    let pane = panes.panes.get(&FIRST).expect("the selected pane exists");
    assert_eq!(pane.widgets.len(), 3);
    assert_eq!(
        pane.declared_widgets,
        pane.widgets
            .iter()
            .map(|response| response.id)
            .collect::<Vec<_>>(),
        "the declaration is refreshed from this pane's actual responses"
    );

    for (control, response) in ["TextEdit", "Button", "canvas"]
        .into_iter()
        .zip(&pane.widgets)
    {
        assert!(response.enabled(), "{control} remains an enabled control");
        assert!(response.interact_rect.is_positive());
        let hit = context
            .hit_test_last_pass(viewport, response.interact_rect.center())
            .expect("the viewport completed the content pass");
        assert_eq!(hit.cumulative_pass_nr(), expected_pass);
        let actual = hit
            .contains_pointer()
            .expect("the actual control owns the completed-pass hover hit");
        assert_eq!(actual.id(), response.id, "{control} owns its actual hit");
        assert_eq!(actual.layer_id(), response.layer_id);
        assert_eq!(
            hit.click(),
            Some(actual),
            "{control} retains its click lane"
        );

        let matches = matching_receivers(&painted, actual);
        let [receiver] = matches.as_slice() else {
            panic!("{control} must have one exact content binding, got {matches:?}");
        };
        assert_eq!(receiver.role(), DockspaceReceiverRole::PaneBody);
        assert_eq!(receiver.visual_id(), expected_pane);
    }
}

#[test]
fn undeclared_content_widgets_do_not_inherit_previous_pass_pane_receivers() {
    let context = Context::default();
    let mut session = session();
    let mut panes = ContentPanes::new(true);
    install_ready_candidate(&mut session);

    let declared = run_ready_surface_frame(&context, &mut session, &mut panes);
    let (viewport, declared_pass) = declared
        .receiver_generation
        .expect("the declared content reports its exact generation");
    for response in &panes.panes.get(&FIRST).expect("the pane exists").widgets {
        let identity = context
            .hit_test_last_pass(viewport, response.interact_rect.center())
            .and_then(|hit| hit.contains_pointer())
            .expect("the declared control owns an actual completed-pass hit");
        assert_eq!(identity.id(), response.id);
        assert_eq!(identity.layer_id(), response.layer_id);
        assert_eq!(matching_receivers(&declared, identity).len(), 1);
    }

    panes.declare_widgets = false;
    let painted = run_ready_surface_frame(&context, &mut session, &mut panes);
    let (current_viewport, expected_pass) = painted
        .receiver_generation
        .expect("the undeclared content reports its exact generation");
    assert_eq!(current_viewport, viewport);
    assert!(expected_pass > declared_pass);
    let pane = panes.panes.get(&FIRST).expect("the selected pane exists");
    assert_eq!(pane.widgets.len(), 3);
    assert!(pane.declared_widgets.is_empty());
    assert!(painted.receivers.iter().any(|receiver| {
        receiver.role() == DockspaceReceiverRole::PaneBody
            && Some(receiver.visual_id()) == painted.pane_receiver
    }));

    for (control, response) in ["TextEdit", "Button", "canvas"]
        .into_iter()
        .zip(&pane.widgets)
    {
        let hit = context
            .hit_test_last_pass(viewport, response.interact_rect.center())
            .expect("the viewport completed the undeclared content pass");
        assert_eq!(hit.cumulative_pass_nr(), expected_pass);
        let actual = hit
            .contains_pointer()
            .expect("the undeclared control remains a visible hit blocker");
        assert_eq!(actual.id(), response.id, "{control} owns its actual hit");
        assert_eq!(actual.layer_id(), response.layer_id);
        assert_eq!(hit.click(), Some(actual));
        assert!(
            matching_receivers(&painted, actual).is_empty(),
            "{control} cannot borrow the surrounding pane or an earlier pass's receiver"
        );
    }
}

#[test]
fn visible_overlay_blocks_declared_content_without_inheriting_pane_receivers() {
    let context = Context::default();
    let mut session = session();
    let mut panes = ContentPanes::new(true);
    install_ready_candidate(&mut session);
    let _ = run_ready_surface_frame(&context, &mut session, &mut panes);

    let mut overlay_response = None;
    let mut overlay_visible = false;
    let mut painted = None;
    // Area's initial sizing pass is invisible; inspect the following visible pass.
    for _ in 0..2 {
        painted = Some(run_surface_frame_with_overlay(
            &context,
            &mut session,
            &mut panes,
            Vec::new(),
            |ui, panes| {
                let pane = panes.panes.get(&FIRST).expect("the selected pane exists");
                let overlay_rect = pane
                    .widgets
                    .iter()
                    .fold(Rect::NOTHING, |rect, response| {
                        rect.union(response.interact_rect)
                    })
                    .expand(4.0);
                overlay_response = Some(
                    Area::new(Id::unique("native-content-blocking-overlay"))
                        .order(Order::Foreground)
                        .movable(false)
                        .fixed_pos(overlay_rect.min)
                        .fade_in(false)
                        .show(ui.ctx(), |ui| {
                            overlay_visible = ui.is_visible();
                            ui.put(overlay_rect, Button::new("Blocking overlay"))
                        })
                        .inner,
                );
            },
        ));
    }
    let painted = painted.expect("the visible overlay frame paints");
    let overlay = overlay_response.expect("the overlay registers a real button response");
    assert!(overlay_visible, "the sizing pass is not blocking evidence");
    assert!(overlay.enabled());
    assert_eq!(overlay.layer_id.order, Order::Foreground);
    let (viewport, expected_pass) = painted
        .receiver_generation
        .expect("the content beneath the overlay reports its exact generation");
    let expected_pane = painted
        .pane_receiver
        .expect("the paint plan still contains the selected pane receiver");
    assert!(painted.receivers.iter().all(|receiver| {
        receiver.widget_id() != overlay.id || receiver.layer_id() != overlay.layer_id
    }));

    let pane = panes.panes.get(&FIRST).expect("the selected pane exists");
    assert_eq!(pane.widgets.len(), 3);
    assert_eq!(pane.declared_widgets.len(), 3);
    for (control, response) in ["TextEdit", "Button", "canvas"]
        .into_iter()
        .zip(&pane.widgets)
    {
        let underlying = painted
            .receivers
            .iter()
            .filter(|receiver| {
                receiver.viewport_id() == viewport
                    && receiver.cumulative_pass_nr() == expected_pass
                    && receiver.widget_id() == response.id
                    && receiver.layer_id() == response.layer_id
            })
            .collect::<Vec<_>>();
        let [receiver] = underlying.as_slice() else {
            panic!("the covered {control} keeps one exact content binding, got {underlying:?}");
        };
        assert_eq!(receiver.role(), DockspaceReceiverRole::PaneBody);
        assert_eq!(receiver.visual_id(), expected_pane);
        assert_ne!(response.layer_id, overlay.layer_id);

        let point = response.interact_rect.center();
        assert!(overlay.interact_rect.contains(point));
        let hit = context
            .hit_test_last_pass(viewport, point)
            .expect("the viewport completed the visible overlay pass");
        assert_eq!(hit.cumulative_pass_nr(), expected_pass);
        let actual = hit
            .contains_pointer()
            .expect("the overlay owns the visible completed-pass hover hit");
        assert_eq!(actual.id(), overlay.id, "the overlay covers {control}");
        assert_eq!(actual.layer_id(), overlay.layer_id);
        assert_eq!(hit.click(), Some(actual));
        assert!(
            matching_receivers(&painted, actual).is_empty(),
            "the overlay cannot borrow the covered {control}'s pane binding"
        );
    }
}

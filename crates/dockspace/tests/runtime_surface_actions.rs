use crate::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use crate::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use crate::model::{
    DockspaceAxis, DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout,
};
use crate::policy::{DockItemRule, DockPolicy, DockSourceRule, DockSurfaceRule};
use crate::runtime::{
    ContainedResizeDirection, DockspaceRuntimeErrorKind, DockspaceSession, HostCloseRequestOrigin,
    HostInputOutcome, PreparedSurfaceAction, SurfaceContainedResizeAdjustment, SurfaceGesturePhase,
    SurfaceMeasurementAnswer, SurfaceMeasurementRequest, SurfaceSplitterAdjustment,
    SurfaceUnavailableReason, TabListMenuMetrics, TabStripControlKind, TabStripControlMetric,
    TabStripControlMetrics, TabStripControlPlacement, TabStripMetrics, UniformSurfaceMetrics,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FLOATING_ROOT: RootId = RootId::new(2);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);
const FOURTH: ItemId = ItemId::new(4);

fn session() -> DockspaceSession {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, SECOND])),
    )])
    .expect("surface-action test layout validates");
    DockspaceSession::from_layout(layout, DockPolicy::default())
        .expect("surface-action test session initializes")
}

fn split_session() -> DockspaceSession {
    let content = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        [
            DockspaceNode::central_tabs([FIRST]),
            DockspaceNode::tabs([SECOND]),
        ],
    )
    .expect("surface-action split validates");
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, content),
    )])
    .expect("surface-action split layout validates");
    DockspaceSession::from_layout(layout, DockPolicy::default())
        .expect("surface-action split session initializes")
}

fn overflow_session() -> DockspaceSession {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            ROOT,
            DockspaceNode::central_tabs([FIRST, SECOND, THIRD, FOURTH]),
        ),
    )])
    .expect("overflow surface-action layout validates");
    DockspaceSession::from_layout(layout, DockPolicy::default())
        .expect("overflow surface-action session initializes")
}

fn contained_session(policy: DockPolicy) -> DockspaceSession {
    let contained_rect =
        LogicalRect::new(160.0, 120.0, 320.0, 240.0).expect("contained rectangle validates");
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, THIRD])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs([SECOND])),
        contained_rect,
    ))])
    .expect("contained surface-action layout validates");
    DockspaceSession::from_layout(layout, policy)
        .expect("contained surface-action session initializes")
}

fn split_weights(session: &DockspaceSession) -> Vec<f32> {
    session
        .view()
        .surface(SURFACE)
        .and_then(|surface| surface.main_root())
        .and_then(|root| root.content())
        .and_then(|content| content.split())
        .expect("the test main root remains split")
        .weights()
        .collect()
}

fn contained_rect(session: &DockspaceSession) -> LogicalRect {
    session
        .view()
        .contained(FLOATING)
        .expect("the test contained presentation remains open")
        .rect()
}

fn metrics() -> UniformSurfaceMetrics {
    UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 800.0, 600.0).expect("test bounds validate"),
        LogicalSize::new(64.0, 48.0).expect("test minimum validates"),
        96.0,
    )
    .expect("test metrics validate")
}

fn install_ready_candidate(session: &mut DockspaceSession) {
    let mut frame = session
        .begin_host_frame()
        .expect("measurement frame begins");
    frame
        .measure_surface(SURFACE, metrics())
        .expect("surface measurement succeeds");
    frame.commit().expect("ready candidate commits");
}

fn overflow_measurement(request: SurfaceMeasurementRequest) -> SurfaceMeasurementAnswer {
    match request {
        SurfaceMeasurementRequest::DockBounds { .. }
        | SurfaceMeasurementRequest::PopupPlaneBounds { .. } => SurfaceMeasurementAnswer::Bounds(
            LogicalRect::new(0.0, 0.0, 260.0, 180.0).expect("overflow bounds validate"),
        ),
        SurfaceMeasurementRequest::PaneMinimum { .. } => SurfaceMeasurementAnswer::PaneMinimum(
            LogicalSize::new(64.0, 48.0).expect("overflow pane minimum validates"),
        ),
        SurfaceMeasurementRequest::TabIntrinsic { .. } => {
            SurfaceMeasurementAnswer::TabIntrinsic(88.0)
        }
        SurfaceMeasurementRequest::TabStrip { .. } => {
            let controls = TabStripControlMetrics::new(4.0)
                .expect("overflow control spacing validates")
                .with_scroll_backward(
                    TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayLeading)
                        .expect("backward control validates"),
                )
                .with_scroll_forward(
                    TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayTrailing)
                        .expect("forward control validates"),
                )
                .with_tab_list_menu(
                    TabStripControlMetric::new(20.0, TabStripControlPlacement::ReservedTrailing)
                        .expect("menu control validates"),
                );
            let menu = TabListMenuMetrics::new(24.0, 8.0, 8.0, 2.0, 92.0, 10.0)
                .expect("menu metrics validate");
            SurfaceMeasurementAnswer::TabStrip(
                TabStripMetrics::new(0.0, 0.0)
                    .expect("overflow strip metrics validate")
                    .with_controls(controls)
                    .with_tab_list_menu(menu),
            )
        }
    }
}

fn measure_overflow_surface(frame: &mut crate::runtime::DockspaceHostFrame<'_>) {
    frame
        .measure_surface_with(SURFACE, overflow_measurement)
        .expect("overflow surface measurement succeeds");
}

fn install_overflow_candidate(session: &mut DockspaceSession) {
    let mut frame = session
        .begin_host_frame()
        .expect("overflow measurement frame begins");
    measure_overflow_surface(&mut frame);
    frame.commit().expect("overflow candidate commits");
}

fn prepare_tab_select(session: &mut DockspaceSession, item: ItemId) -> PreparedSurfaceAction {
    let mut frame = session.begin_host_frame().expect("paint frame begins");
    let action = frame
        .paint_plan(SURFACE)
        .expect("paint plan lookup succeeds")
        .expect("ready candidate is paintable")
        .prepare_tab_select(item)
        .expect("tab exists in the exact candidate");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the unchanged candidate is retained");
    frame.commit().expect("paint pass commits");
    action
}

fn prepare_tab_close(session: &mut DockspaceSession, item: ItemId) -> PreparedSurfaceAction {
    let mut frame = session.begin_host_frame().expect("paint frame begins");
    let action = frame
        .paint_plan(SURFACE)
        .expect("paint plan lookup succeeds")
        .expect("ready candidate is paintable")
        .prepare_tab_close(item)
        .expect("tab close control exists in the exact candidate");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the unchanged candidate is retained");
    frame.commit().expect("paint pass commits");
    action
}

#[test]
fn contained_resize_operability_honors_exact_root_item_and_surface_policy() {
    let mut policies = Vec::new();

    let mut global = DockPolicy::default();
    global.set_allow_contained_transform(false);
    policies.push(("global", global));

    let mut root = DockPolicy::default();
    let mut root_rule = DockSourceRule::new();
    root_rule.set_enabled(false);
    root.set_source_rule(FLOATING_ROOT, root_rule);
    policies.push(("root", root));

    let mut item = DockPolicy::default();
    let mut item_rule = DockItemRule::new();
    item_rule.set_source_enabled(false);
    item.set_item_rule(SECOND, item_rule);
    policies.push(("item", item));

    let mut surface = DockPolicy::default();
    let mut surface_rule = DockSurfaceRule::new();
    surface_rule.set_target_enabled(false);
    surface.set_surface_rule(SURFACE, surface_rule);
    policies.push(("surface", surface));

    for (scope, policy) in policies {
        let mut session = contained_session(policy);
        install_ready_candidate(&mut session);
        let mut frame = session.begin_host_frame().expect("paint frame begins");
        let plan = frame
            .paint_plan(SURFACE)
            .expect("paint plan lookup succeeds")
            .expect("contained candidate is paintable");
        let contained = plan
            .contained()
            .next()
            .expect("the exact contained presentation is painted");
        let cardinal = contained
            .resize()
            .filter(|resize| {
                matches!(
                    resize.direction(),
                    ContainedResizeDirection::North
                        | ContainedResizeDirection::East
                        | ContainedResizeDirection::South
                        | ContainedResizeDirection::West
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(cardinal.len(), 4);
        assert!(
            cardinal.iter().all(|resize| !resize.operable()),
            "{scope} policy denial must be frozen into every cardinal resize record",
        );
        assert!(
            cardinal.iter().all(|resize| plan
                .receiver_for_contained_resize(contained, *resize)
                .is_none()),
            "{scope} policy denial must remove every cardinal resize receiver",
        );
        assert!(
            cardinal.iter().all(|resize| {
                plan.prepare_contained_resize_adjustment(
                    contained.floating(),
                    resize.direction(),
                    SurfaceContainedResizeAdjustment::Increment,
                )
                .is_none()
            }),
            "{scope} policy denial must reject every cardinal resize action",
        );
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("the unchanged candidate is retained");
        frame.commit().expect("paint pass commits");
    }
}

#[test]
fn contained_resize_adjustment_uses_current_ready_geometry_for_every_cardinal_edge() {
    let source = LogicalRect::new(160.0, 120.0, 320.0, 240.0).expect("source rectangle validates");
    let cases = [
        (
            ContainedResizeDirection::North,
            SurfaceContainedResizeAdjustment::Decrement,
            LogicalRect::new(160.0, 104.0, 320.0, 256.0).expect("north rectangle validates"),
        ),
        (
            ContainedResizeDirection::East,
            SurfaceContainedResizeAdjustment::Increment,
            LogicalRect::new(160.0, 120.0, 336.0, 240.0).expect("east rectangle validates"),
        ),
        (
            ContainedResizeDirection::South,
            SurfaceContainedResizeAdjustment::Increment,
            LogicalRect::new(160.0, 120.0, 320.0, 256.0).expect("south rectangle validates"),
        ),
        (
            ContainedResizeDirection::West,
            SurfaceContainedResizeAdjustment::Decrement,
            LogicalRect::new(144.0, 120.0, 336.0, 240.0).expect("west rectangle validates"),
        ),
    ];

    for (direction, adjustment, expected) in cases {
        let mut session = contained_session(DockPolicy::default());
        install_ready_candidate(&mut session);
        assert_eq!(contained_rect(&session), source);

        let mut frame = session.begin_host_frame().expect("adjustment frame begins");
        let plan = frame
            .paint_plan(SURFACE)
            .expect("paint plan lookup succeeds")
            .expect("contained candidate is paintable");
        let contained = plan
            .contained()
            .next()
            .expect("the contained presentation is painted");
        let resize = contained
            .resize()
            .find(|resize| resize.direction() == direction)
            .expect("the requested cardinal edge is painted");
        assert!(resize.operable());
        let action = plan
            .prepare_contained_resize_adjustment(
                contained.floating(),
                resize.direction(),
                adjustment,
            )
            .expect("the current Ready edge prepares an adjustment");
        frame
            .submit_surface_action(action)
            .expect("the same-frame action is accepted");
        frame
            .measure_surface(SURFACE, metrics())
            .expect("the adjusted surface measures");
        frame.commit().expect("the adjustment frame commits");

        assert_eq!(contained_rect(&session), expected, "{direction:?}");
    }
}

#[test]
fn stale_contained_resize_adjustment_is_inert() {
    let mut session = contained_session(DockPolicy::default());
    install_ready_candidate(&mut session);
    let before = contained_rect(&session);

    let mut prepare = session.begin_host_frame().expect("prepare frame begins");
    let plan = prepare
        .paint_plan(SURFACE)
        .expect("paint plan lookup succeeds")
        .expect("contained candidate is paintable");
    let contained = plan
        .contained()
        .next()
        .expect("the contained presentation is painted");
    let action = plan
        .prepare_contained_resize_adjustment(
            contained.floating(),
            ContainedResizeDirection::East,
            SurfaceContainedResizeAdjustment::Increment,
        )
        .expect("the current edge prepares an adjustment");
    prepare
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the candidate is retained");
    prepare.commit().expect("prepare frame commits");

    let mut mutate = session.begin_host_frame().expect("mutation frame begins");
    mutate
        .select_item_current(THIRD)
        .expect("the main-root selection changes the revision");
    mutate
        .measure_surface(SURFACE, metrics())
        .expect("the mutated surface measures");
    mutate.commit().expect("mutation frame commits");

    let mut stale = session.begin_host_frame().expect("stale frame begins");
    stale
        .submit_surface_action(action)
        .expect("same-session stale action is structurally accepted");
    stale
        .measure_surface(SURFACE, metrics())
        .expect("the stale surface measures");
    let report = stale.commit().expect("stale frame commits");

    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::StaleRejected { .. }]
    ));
    assert_eq!(contained_rect(&session), before);
}

#[test]
fn exact_surface_tab_action_selects_without_exposing_scene_identity() {
    let mut session = session();
    install_ready_candidate(&mut session);
    let action = prepare_tab_select(&mut session, SECOND);
    assert_eq!(action.surface(), SURFACE);
    assert_eq!(action.expected_version(), session.version());

    let mut frame = session.begin_host_frame().expect("action frame begins");
    frame
        .submit_surface_action(action)
        .expect("same-session action is accepted");
    frame
        .measure_surface(SURFACE, metrics())
        .expect("post-action surface measurement succeeds");
    frame.commit().expect("selection frame commits");

    assert!(
        session
            .view()
            .item(SECOND)
            .expect("second item remains open")
            .is_selected()
    );
}

#[test]
fn exact_surface_close_action_opens_the_shared_close_workflow() {
    let mut session = session();
    install_ready_candidate(&mut session);
    let action = prepare_tab_close(&mut session, SECOND);

    let mut frame = session.begin_host_frame().expect("close frame begins");
    frame
        .submit_surface_action(action)
        .expect("same-session close action is accepted");
    frame
        .measure_surface(SURFACE, metrics())
        .expect("post-close-request measurement succeeds");
    let report = frame.commit().expect("close request frame commits");

    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::CloseRequested {
            origin: HostCloseRequestOrigin::Interaction,
            ..
        }]
    ));
    assert!(session.view().item(SECOND).is_some());
}

#[test]
fn stale_surface_action_is_inert_after_a_new_workspace_revision() {
    let mut session = session();
    install_ready_candidate(&mut session);
    let stale = prepare_tab_select(&mut session, FIRST);

    let mut mutate = session.begin_host_frame().expect("mutation frame begins");
    mutate
        .select_item_current(SECOND)
        .expect("current selection action is accepted");
    mutate
        .measure_surface(SURFACE, metrics())
        .expect("mutated surface measurement succeeds");
    mutate.commit().expect("mutation frame commits");

    let mut frame = session
        .begin_host_frame()
        .expect("stale action frame begins");
    frame
        .submit_surface_action(stale)
        .expect("same-session stale action is structurally accepted");
    frame
        .measure_surface(SURFACE, metrics())
        .expect("stale action surface measurement succeeds");
    let report = frame.commit().expect("stale action frame commits");

    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::StaleRejected { .. }]
    ));
    assert!(
        session
            .view()
            .item(SECOND)
            .expect("second item remains open")
            .is_selected()
    );
}

#[test]
fn cross_session_surface_action_is_rejected_without_poisoning_the_frame() {
    let mut first = session();
    install_ready_candidate(&mut first);
    let foreign = prepare_tab_select(&mut first, SECOND);

    let mut second = session();
    install_ready_candidate(&mut second);
    let mut frame = second
        .begin_host_frame()
        .expect("second session frame begins");
    let error = frame
        .submit_surface_action(foreign)
        .expect_err("foreign surface action must be rejected");
    assert_eq!(error.kind(), DockspaceRuntimeErrorKind::OperationConflict);
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the unaffected frame remains usable");
    frame.commit().expect("the unaffected frame commits");

    assert!(
        second
            .view()
            .item(FIRST)
            .expect("first item remains open")
            .is_selected()
    );
}

#[test]
fn opaque_splitter_actions_drive_the_core_resize_session() {
    let mut session = split_session();
    install_ready_candidate(&mut session);
    let before = split_weights(&session);

    let mut prepare = session.begin_host_frame().expect("paint frame begins");
    let plan = prepare
        .paint_plan(SURFACE)
        .expect("paint plan lookup succeeds")
        .expect("split candidate is paintable");
    let splitter = plan
        .splitters()
        .next()
        .expect("the equal split has one splitter");
    let bounds = splitter.hit_bounds();
    let initial = LogicalPoint::new(
        bounds.x() + bounds.width() * 0.5,
        bounds.y() + bounds.height() * 0.5,
    )
    .expect("splitter center validates");
    let released = LogicalPoint::new(initial.x() + 48.0, initial.y())
        .expect("splitter release point validates");
    let press = plan
        .prepare_splitter_gesture(
            splitter.visual_id(),
            SurfaceGesturePhase::Begin {
                initial,
                current: initial,
            },
        )
        .expect("the exact operable splitter prepares a press");
    let release = plan
        .prepare_splitter_gesture(
            splitter.visual_id(),
            SurfaceGesturePhase::Release { current: released },
        )
        .expect("the exact operable splitter prepares a release");
    prepare
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the unchanged candidate is retained");
    prepare.commit().expect("paint pass commits");

    let mut press_frame = session.begin_host_frame().expect("press frame begins");
    press_frame
        .submit_surface_action(press)
        .expect("splitter press is accepted");
    press_frame
        .measure_surface(SURFACE, metrics())
        .expect("pressed surface measures");
    press_frame.commit().expect("splitter press commits");
    assert_eq!(split_weights(&session), before);

    let mut release_frame = session.begin_host_frame().expect("release frame begins");
    release_frame
        .submit_surface_action(release)
        .expect("splitter release is accepted");
    release_frame
        .measure_surface(SURFACE, metrics())
        .expect("released surface measures");
    release_frame.commit().expect("splitter release commits");

    assert_ne!(split_weights(&session), before);
}

#[test]
fn exact_surface_splitter_adjustment_uses_the_local_ready_candidate() {
    let mut session = split_session();
    install_ready_candidate(&mut session);
    let before = split_weights(&session);

    let mut frame = session.begin_host_frame().expect("adjustment frame begins");
    let plan = frame
        .paint_plan(SURFACE)
        .expect("paint plan lookup succeeds")
        .expect("split candidate is paintable");
    let splitter = plan
        .splitters()
        .next()
        .expect("the equal split has one splitter");
    let action = plan
        .prepare_splitter_adjustment(splitter.visual_id(), SurfaceSplitterAdjustment::Increment)
        .expect("the operable splitter prepares an adjustment");
    frame
        .submit_surface_action(action)
        .expect("the same-frame local response is accepted");
    frame
        .measure_surface(SURFACE, metrics())
        .expect("the adjusted surface measures");
    frame.commit().expect("the adjustment frame commits");

    assert!(split_weights(&session)[0] > before[0]);
}

#[test]
fn exact_surface_tab_list_actions_open_and_select_from_the_current_candidate() {
    let mut session = overflow_session();
    install_overflow_candidate(&mut session);

    let mut prepare = session
        .begin_host_frame()
        .expect("overflow control paint frame begins");
    let plan = prepare
        .paint_plan(SURFACE)
        .expect("overflow control plan lookup succeeds")
        .expect("overflow control plan is paintable");
    let control = plan
        .tab_strip_controls()
        .find(|control| control.kind() == TabStripControlKind::TabListMenu)
        .expect("overflow plan exposes the menu control");
    let open = plan
        .prepare_tab_strip_control_activation(control)
        .expect("enabled menu control prepares an action");
    prepare
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the control plan is retained");
    prepare.commit().expect("control paint frame commits");

    let mut open_frame = session
        .begin_host_frame()
        .expect("menu open action frame begins");
    open_frame
        .submit_surface_action(open)
        .expect("menu open action is accepted");
    open_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("menu open defers remeasurement");
    open_frame.commit().expect("menu open action commits");

    let mut publish_menu = session
        .begin_host_frame()
        .expect("menu publication frame begins");
    measure_overflow_surface(&mut publish_menu);
    publish_menu.commit().expect("menu candidate commits");

    let mut prepare_row = session
        .begin_host_frame()
        .expect("menu row paint frame begins");
    let plan = prepare_row
        .paint_plan(SURFACE)
        .expect("menu plan lookup succeeds")
        .expect("menu plan is paintable");
    let menu = plan
        .tab_list_menus()
        .next()
        .expect("the tab-list menu is open");
    let row = menu
        .rows()
        .find(|row| row.item() == FOURTH)
        .expect("the fourth item has a menu row");
    let select = plan
        .prepare_tab_list_menu_row_activation(row)
        .expect("the visible menu row prepares an action");
    prepare_row
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the menu plan is retained");
    prepare_row.commit().expect("menu row paint frame commits");

    let mut select_frame = session
        .begin_host_frame()
        .expect("menu selection frame begins");
    select_frame
        .submit_surface_action(select)
        .expect("menu row action is accepted");
    select_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("menu selection defers remeasurement");
    select_frame.commit().expect("menu row selection commits");

    assert!(
        session
            .view()
            .item(FOURTH)
            .expect("fourth item remains open")
            .is_selected()
    );
}

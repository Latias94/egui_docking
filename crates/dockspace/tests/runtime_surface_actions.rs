use crate::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use crate::ids::{ItemId, RootId, SurfaceId};
use crate::model::{
    DockspaceAxis, DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
};
use crate::policy::DockPolicy;
use crate::runtime::{
    DockspaceRuntimeErrorKind, DockspaceSession, HostCloseRequestOrigin, HostInputOutcome,
    PreparedSurfaceAction, SurfaceGesturePhase, SurfaceSplitterAdjustment,
    SurfaceUnavailableReason, UniformSurfaceMetrics,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);

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

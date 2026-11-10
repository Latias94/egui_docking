//! Retained pixels cannot authorize touch or scroll against a migrated receiver.

use std::time::Duration;

use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceActionOutcome, DockspaceContainedLayout, DockspaceLayout, DockspaceNode,
    DockspaceRootLayout, DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceSession, DockspaceTouchEvent, HostFrameReport, HostInputOutcome,
    HostPointerInput, HostPointerRoute, PointerDeviceKind, PointerEventMetadata,
    PresentedDockReceiver, PresentedDockspaceSurface, SurfacePaintProgramOperation as Operation,
    SurfacePaintProgramStep as Step, SurfacePointerButton as Button, SurfacePointerCapture,
    SurfacePointerEvent as Event, SurfacePointerId, SurfacePointerReceiverFacts,
    SurfacePresentationResult, SurfaceScrollDelta, SurfaceScrollDeviceId, SurfaceScrollEvent,
    SurfaceScrollModifiers, SurfaceScrollMomentum, SurfaceScrollPhase, SurfaceUnavailableReason,
    UniformSurfaceMetrics,
};
use dockspace_host_harness::RecordingRenderer;

const A: SurfaceId = SurfaceId::new(1);
const B: SurfaceId = SurfaceId::new(2);
const A_FIRST: ItemId = ItemId::new(1);
const A_SECOND: ItemId = ItemId::new(2);
const TARGET: ItemId = ItemId::new(10);
const MIGRATING_ROOT: RootId = RootId::new(2);
const POINTER: SurfacePointerId = SurfacePointerId::new(1);

fn finish(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

struct Fixture {
    session: DockspaceSession,
    surface: PresentedDockspaceSurface,
    tab: PresentedDockReceiver,
    scroll: PresentedDockReceiver,
}

fn fixture() -> Fixture {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            A,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([A_FIRST, A_SECOND]),
            ),
        ),
        DockspaceSurfaceLayout::new(
            B,
            DockspaceRootLayout::new(
                MIGRATING_ROOT,
                DockspaceNode::tabs((10..18).map(ItemId::new)),
            ),
        )
        .with_contained(DockspaceContainedLayout::new(
            FloatingPresentationId::new(1),
            DockspaceRootLayout::new(RootId::new(3), DockspaceNode::tabs([ItemId::new(3)])),
            LogicalRect::new(160.0, 300.0, 160.0, 200.0).unwrap(),
        )),
    ])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session.enable_host_pointer().unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    for surface in [A, B] {
        frame
            .measure_surface(
                surface,
                UniformSurfaceMetrics::new(
                    LogicalRect::new(0.0, 0.0, if surface == A { 900.0 } else { 360.0 }, 650.0)
                        .unwrap(),
                    LogicalSize::new(64.0, 48.0).unwrap(),
                    96.0,
                )
                .unwrap(),
            )
            .unwrap();
    }
    frame.commit().unwrap();

    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(B).unwrap().unwrap();
    let tab = plan.tab_receiver(TARGET).unwrap();
    let mut scroll = None;
    plan.visit_surface_program(|step| {
        if let Step::Operation {
            operation: Operation::TabBar(bar),
            ..
        } = step
            && bar.root() == MIGRATING_ROOT
        {
            assert!(bar.maximum_scroll_offset() > 0.0);
            scroll = plan.receiver_for_tab_strip_scroll(bar);
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let scroll = scroll.expect("the main strip overflows and exposes a scroll receiver");
    for surface in [A, B] {
        let plan = frame.paint_plan(surface).unwrap().unwrap();
        let (_, receipt) = RecordingRenderer::execute(plan).unwrap();
        frame.confirm_surface_painted(receipt).unwrap();
    }
    for output in frame.commit().unwrap().take_painted_outputs() {
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
    }
    finish(session.begin_host_frame().unwrap());
    let surface = session.presented_surface(B).unwrap();
    let tab = session.bind_presented_receiver(&tab).unwrap();
    let scroll = session.bind_presented_receiver(&scroll).unwrap();
    Fixture {
        session,
        surface,
        tab,
        scroll,
    }
}

fn assert_target_on_a(session: &DockspaceSession) {
    let target = session.view().item(TARGET).unwrap();
    assert_eq!(target.surface(), A);
    assert_eq!(target.root(), MIGRATING_ROOT);
    assert!(target.contained().is_some());
}

fn migrate(fixture: &mut Fixture) {
    let action = fixture.session.prepare_float_root(MIGRATING_ROOT, A);
    let mut frame = fixture.session.begin_host_frame().unwrap();
    frame.submit_prepared_action(action).unwrap();
    let mut report = finish(frame);
    assert!(report.workspace_changed());
    assert!(report.take_painted_outputs().is_empty());
    assert_target_on_a(&fixture.session);
    // B's main root leaves its contained stacking roster unchanged. Its old
    // pixels remain presented even though this root now belongs to A.
    assert_eq!(fixture.session.presented_surface(B), Some(fixture.surface));
    for receiver in [fixture.tab, fixture.scroll] {
        assert!(
            fixture
                .session
                .bind_presented_receiver(&receiver.descriptor())
                .is_none(),
            "the migrated receiver cannot bind again even while its old pixels remain visible"
        );
    }
}

fn touch(fixture: &mut Fixture, event: Event, millis: u64) -> HostFrameReport {
    let route = HostPointerRoute::new(fixture.surface, fixture.tab.center());
    let mut frame = fixture.session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(
            HostPointerInput::new(
                POINTER,
                event,
                route,
                route,
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::delivery(&fixture.tab),
            )
            .with_metadata(PointerEventMetadata::new(
                PointerDeviceKind::Touch,
                Duration::from_millis(millis),
            )),
        )
        .unwrap();
    finish(frame)
}

fn assert_no_context_menu(report: &HostFrameReport) {
    assert!(
        report.inputs().iter().all(|outcome| !matches!(
            outcome,
            HostInputOutcome::Touch(DockspaceTouchEvent::ContextMenuRequested(_))
        )),
        "a migrated receiver cannot request a menu on its previous surface: {report:?}"
    );
}

fn observe_long_press(fixture: &mut Fixture) {
    if let Some(request) = fixture.session.pointer_continuation() {
        assert_eq!(request.deadline(), Duration::from_millis(350));
        assert_no_context_menu(&touch(fixture, request.event(), 350));
    }
}

#[test]
fn touch_started_after_root_migration_cannot_request_a_menu_from_the_retained_receiver() {
    let mut fixture = fixture();
    migrate(&mut fixture);
    let version = fixture.session.version();
    assert_no_context_menu(&touch(
        &mut fixture,
        Event::ButtonPressed(Button::Primary),
        0,
    ));
    observe_long_press(&mut fixture);
    let report = touch(&mut fixture, Event::ContactEnded(Button::Primary), 360);
    assert_no_context_menu(&report);
    assert!(!report.workspace_changed());
    assert_eq!(fixture.session.version(), version);
    assert_target_on_a(&fixture.session);
    assert!(!fixture.session.has_active_gesture());
}

#[test]
fn waiting_or_armed_touch_cannot_request_a_menu_after_its_root_migrates() {
    for arm_before_migration in [false, true] {
        let mut fixture = fixture();
        assert_no_context_menu(&touch(
            &mut fixture,
            Event::ButtonPressed(Button::Primary),
            0,
        ));
        assert!(fixture.session.has_active_gesture());
        assert!(fixture.session.pointer_continuation().is_some());
        if arm_before_migration {
            observe_long_press(&mut fixture);
            assert!(fixture.session.has_active_gesture());
            assert!(fixture.session.pointer_continuation().is_none());
        }
        migrate(&mut fixture);
        let version = fixture.session.version();
        if !arm_before_migration {
            observe_long_press(&mut fixture);
        }
        let report = touch(&mut fixture, Event::ContactEnded(Button::Primary), 360);
        assert_no_context_menu(&report);
        assert!(!report.workspace_changed());
        assert_eq!(fixture.session.version(), version);
        assert_target_on_a(&fixture.session);
        assert!(!fixture.session.has_active_gesture());
    }
}

#[test]
fn scroll_on_a_migrated_receiver_does_not_poison_an_independent_action() {
    let mut fixture = fixture();
    migrate(&mut fixture);
    let route = HostPointerRoute::new(fixture.surface, fixture.scroll.center());
    let mut frame = fixture.session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(HostPointerInput::new(
            POINTER,
            Event::Scrolled(SurfaceScrollEvent::new(
                SurfaceScrollDeviceId::new(1),
                SurfaceScrollPhase::Discrete {
                    delta: SurfaceScrollDelta::LogicalPoints { x: -40.0, y: 0.0 },
                },
                SurfaceScrollMomentum::Direct,
                SurfaceScrollModifiers::new(false, false, false, false),
            )),
            route,
            route,
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::delivery(&fixture.scroll),
        ))
        .expect("an obsolete visible receiver must suppress scrolling without poisoning the frame");
    frame.select_item_current(A_SECOND).unwrap();
    let report = finish(frame);
    assert!(report.workspace_changed());
    assert!(fixture.session.view().item(A_SECOND).unwrap().is_selected());
    assert_target_on_a(&fixture.session);
    assert_no_context_menu(&report);
    assert!(report.inputs().iter().any(|outcome| matches!(
        outcome,
        HostInputOutcome::ProductActionApplied(DockspaceActionOutcome::Selected {
            item: A_SECOND,
            changed: true,
        })
    )));
}

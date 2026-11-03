//! Visible receivers retain their emitted meaning across semantic updates.

use dockspace::close::ClosePlanTarget;
use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::model::{
    DockPlacement, DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    ContainedResizeDirection, DockspaceHostFrame, DockspaceManipulationDelivery,
    DockspaceManipulationStatus, DockspaceReceiverDescriptor, DockspaceSession, HostFrameReport,
    HostInputOutcome, HostPointerInput, HostPointerRoute, PaintedSurfaceOutput,
    PresentedDockReceiver, PresentedDockspaceSurface, SurfacePaintProgramOperation as Operation,
    SurfacePaintProgramStep as Step, SurfacePointerButton as Button, SurfacePointerCapture,
    SurfacePointerEvent as Event, SurfacePointerId, SurfacePointerReceiverFacts,
    SurfacePresentationResult, SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use dockspace_host_harness::RecordingRenderer;

const SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN: RootId = RootId::new(1);
const REAR_ROOT: RootId = RootId::new(2);
const REAR: FloatingPresentationId = FloatingPresentationId::new(1);
const FRONT: FloatingPresentationId = FloatingPresentationId::new(2);
const BASE: ItemId = ItemId::new(1);
const FIRST: ItemId = ItemId::new(2);
const SECOND: ItemId = ItemId::new(3);
const UNSEEN: ItemId = ItemId::new(4);

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).unwrap()
}

fn session(overlap: bool) -> DockspaceSession {
    let mut surface = DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(MAIN, DockspaceNode::central_tabs([BASE])),
    )
    .with_contained(DockspaceContainedLayout::new(
        REAR,
        DockspaceRootLayout::new(REAR_ROOT, DockspaceNode::tabs([FIRST, SECOND])),
        rect(80.0, 80.0, 360.0, 260.0),
    ));
    if overlap {
        surface = surface.with_contained(DockspaceContainedLayout::new(
            FRONT,
            DockspaceRootLayout::new(RootId::new(3), DockspaceNode::tabs([ItemId::new(5)])),
            rect(240.0, 180.0, 360.0, 260.0),
        ));
    }
    let mut session = DockspaceSession::from_layout(
        DockspaceLayout::new([surface]).unwrap(),
        DockPolicy::default(),
    )
    .unwrap();
    session.enable_host_pointer().unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(
                rect(0.0, 0.0, 800.0, 600.0),
                LogicalSize::new(64.0, 48.0).unwrap(),
                96.0,
            )
            .unwrap(),
        )
        .unwrap();
    frame.commit().unwrap();
    session
}

fn commit(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

struct Receivers {
    contained_close: DockspaceReceiverDescriptor,
    tab_close: DockspaceReceiverDescriptor,
    west_resize: DockspaceReceiverDescriptor,
    hover: DockspaceReceiverDescriptor,
}

fn emit(session: &mut DockspaceSession) -> (Receivers, PaintedSurfaceOutput) {
    let mut frame = session.begin_host_frame().unwrap();
    let hover_item = if frame.view().contained(FRONT).is_some() {
        ItemId::new(5)
    } else {
        FIRST
    };
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let mut contained_close = None;
    let mut tab_close = None;
    let mut west_resize = None;
    plan.visit_surface_program(|step| {
        match step {
            Step::RootBegin { root, .. } if root.root() == REAR_ROOT => {
                let contained = root.contained().unwrap();
                contained_close = plan.receiver_for_contained_close(contained);
                west_resize = contained
                    .resize()
                    .find(|handle| handle.direction() == ContainedResizeDirection::West)
                    .and_then(|handle| plan.receiver_for_contained_resize(contained, handle));
            }
            Step::Operation {
                operation: Operation::Tab(tab),
                ..
            } if tab.item() == FIRST => tab_close = plan.receiver_for_tab_close(tab),
            _ => {}
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let receivers = Receivers {
        contained_close: contained_close.unwrap(),
        tab_close: tab_close.unwrap(),
        west_resize: west_resize.unwrap(),
        hover: plan.center_drop_receiver_for_item(hover_item).unwrap(),
    };
    let (_, receipt) = RecordingRenderer::execute(plan).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    let mut outputs = commit(frame).take_painted_outputs();
    assert_eq!(outputs.len(), 1);
    (receivers, outputs.pop().unwrap())
}

fn present(session: &mut DockspaceSession, output: PaintedSurfaceOutput) -> HostFrameReport {
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    commit(session.begin_host_frame().unwrap())
}

fn click(
    session: &mut DockspaceSession,
    surface: PresentedDockspaceSurface,
    receiver: &PresentedDockReceiver,
) -> HostFrameReport {
    let route = HostPointerRoute::new(surface, receiver.center());
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer_batch(
            [
                Event::ButtonPressed(Button::Primary),
                Event::ButtonReleased(Button::Primary),
            ]
            .map(|event| {
                HostPointerInput::new(
                    SurfacePointerId::new(1),
                    event,
                    route,
                    route,
                    SurfacePointerCapture::ProviderEndpoint,
                    SurfacePointerReceiverFacts::delivery(receiver).with_clear_hover_lane(&surface),
                )
            }),
        )
        .unwrap();
    commit(frame)
}

fn close_items(report: &HostFrameReport, target: ClosePlanTarget) -> Vec<ItemId> {
    let plans = report
        .inputs()
        .iter()
        .filter_map(|input| match input {
            HostInputOutcome::CloseRequested { plan, .. } => Some(plan),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [plan] = plans.as_slice() else {
        panic!("an unchanged visible close button must request exactly one plan: {report:?}");
    };
    assert_eq!(plan.target(), target);
    plan.items().iter().map(|item| item.item()).collect()
}

fn assert_no_close(report: &HostFrameReport) {
    assert!(!report.workspace_changed());
    assert!(
        report
            .inputs()
            .iter()
            .all(|input| !matches!(input, HostInputOutcome::CloseRequested { .. })),
        "a prebound stale close must not recapture current content: {report:?}"
    );
}

#[test]
fn prebound_contained_close_cannot_capture_a_member_absent_from_the_presented_root() {
    for add_unseen_member in [false, true] {
        let mut session = session(false);
        let (receivers, output) = emit(&mut session);
        present(&mut session, output);
        let surface = session.presented_surface(SURFACE).unwrap();
        let close = session
            .bind_presented_receiver(&receivers.contained_close)
            .unwrap();
        if add_unseen_member {
            let mut frame = session.begin_host_frame().unwrap();
            frame
                .open_item_current(UNSEEN, DockPlacement::After(SECOND))
                .unwrap();
            assert!(commit(frame).workspace_changed());
            assert_eq!(session.presented_surface(SURFACE), Some(surface));
            assert!(
                session
                    .bind_presented_receiver(&receivers.contained_close)
                    .is_none()
            );
        }
        let before = session.version();
        let report = click(&mut session, surface, &close);
        assert_eq!(session.version(), before);
        if add_unseen_member {
            assert_no_close(&report);
            assert_eq!(
                session.view().item(UNSEEN).unwrap().tabs().items(),
                &[FIRST, SECOND, UNSEEN]
            );
        } else {
            assert_eq!(
                close_items(&report, ClosePlanTarget::Root { root: REAR_ROOT }),
                [FIRST, SECOND]
            );
        }
        assert!(session.view().item(FIRST).is_some());
        assert!(session.view().item(SECOND).is_some());
        assert!(!session.has_active_gesture());
    }
}

#[test]
fn prebound_tab_close_cannot_follow_its_item_into_another_root() {
    for move_item in [false, true] {
        let mut session = session(false);
        let (receivers, output) = emit(&mut session);
        present(&mut session, output);
        let surface = session.presented_surface(SURFACE).unwrap();
        let close = session
            .bind_presented_receiver(&receivers.tab_close)
            .unwrap();
        if move_item {
            let mut frame = session.begin_host_frame().unwrap();
            frame
                .dock_item_current(FIRST, DockPlacement::After(BASE))
                .unwrap();
            assert!(commit(frame).workspace_changed());
            assert_eq!(session.view().item(FIRST).unwrap().root(), MAIN);
            assert!(
                session
                    .bind_presented_receiver(&receivers.tab_close)
                    .is_none()
            );
        }
        let before = session.version();
        let report = click(&mut session, surface, &close);
        assert_eq!(session.version(), before);
        if move_item {
            assert_no_close(&report);
            assert_eq!(
                session.view().item(FIRST).unwrap().tabs().items(),
                &[BASE, FIRST]
            );
        } else {
            assert_eq!(
                close_items(&report, ClosePlanTarget::Item { item: FIRST }),
                [FIRST]
            );
        }
        assert!(session.view().item(FIRST).is_some());
        assert!(!session.has_active_gesture());
    }
}

#[test]
fn late_presented_output_cannot_restore_old_layering_or_revoke_exact_source_continuation() {
    let mut session = session(true);
    let (receivers, output) = emit(&mut session);
    present(&mut session, output);
    let surface = session.presented_surface(SURFACE).unwrap();
    let resize = session
        .bind_presented_receiver(&receivers.west_resize)
        .unwrap();
    let old_hover = session.bind_presented_receiver(&receivers.hover).unwrap();
    let (_, late) = emit(&mut session);
    let artifact = late.presentation_artifact();
    let original = session.view().contained(REAR).unwrap().rect();
    let press = resize.center();
    let click = session.host_pointer_hit(SURFACE, press).unwrap().delivery();
    let edge = |event, point| {
        let facts = click.as_ref().map_or_else(
            || SurfacePointerReceiverFacts::unknown().with_no_delivery(&surface),
            SurfacePointerReceiverFacts::delivery,
        );
        HostPointerInput::new(
            SurfacePointerId::new(1),
            event,
            HostPointerRoute::new(surface, point),
            HostPointerRoute::new(surface, old_hover.center()),
            SurfacePointerCapture::ProviderEndpoint,
            facts.with_drag_delivery(&resize).with_hover(&old_hover),
        )
    };
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(edge(Event::ButtonPressed(Button::Primary), press))
        .unwrap();
    assert!(
        commit(frame).workspace_changed(),
        "pressing the rear resize raises its root"
    );
    assert_eq!(
        session
            .view()
            .surface(SURFACE)
            .unwrap()
            .contained()
            .map(|contained| contained.id())
            .collect::<Vec<_>>(),
        [FRONT, REAR]
    );
    assert!(session.has_active_gesture());
    assert!(session.presented_surface(SURFACE).is_none());
    assert!(session.bind_presented_receiver(&receivers.hover).is_none());
    let raised_version = session.version();

    let settled = present(&mut session, late);
    assert!(
        settled
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.retires(artifact))
    );
    assert_eq!(session.version(), raised_version);
    assert!(session.presented_surface(SURFACE).is_none());
    assert!(session.bind_presented_receiver(&receivers.hover).is_none());
    assert!(
        session
            .host_pointer_hit(SURFACE, old_hover.center())
            .is_none()
    );
    assert!(
        session.has_active_gesture(),
        "the source continuation survives an inert late output"
    );

    let moved = LogicalPoint::new(press.x() - 30.0, press.y()).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(edge(Event::Moved, moved))
        .unwrap();
    assert!(!commit(frame).workspace_changed());
    assert_eq!(session.view().contained(REAR).unwrap().rect(), original);
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(edge(Event::ButtonReleased(Button::Primary), moved))
        .unwrap();
    let released = commit(frame);
    assert!(released.workspace_changed());
    assert_eq!(
        session.view().contained(REAR).unwrap().rect(),
        rect(
            original.x() - 30.0,
            original.y(),
            original.width() + 30.0,
            original.height()
        )
    );
    assert_eq!(released.manipulation_feedback().len(), 1);
    assert_eq!(
        released.manipulation_feedback()[0].status(),
        DockspaceManipulationStatus::Delivered(DockspaceManipulationDelivery::ContainedTransform)
    );
    assert!(!session.has_active_gesture());
}

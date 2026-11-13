use super::*;

use crate::runtime::{
    HostPointerInput, HostPointerRoute, SurfacePointerButton, SurfacePointerCancelReason,
    SurfacePointerCapture, SurfacePointerEvent, SurfacePointerId, SurfacePointerReceiverFacts,
};

fn moving_session() -> DockspaceSession {
    let (mut session, _) = fixture();
    dock(&mut session, C, A);
    let (_, output) = paint(&mut session, motion(1_000));
    present(&mut session, output);
    let (moving, output) = paint(&mut session, motion(1_050));
    assert!(!moving.partition);
    assert!(session.surface_motion_is_active(SURFACE));
    present(&mut session, output);
    assert!(!session.view().item(A).unwrap().is_selected());
    session
}

fn assert_final_partition(session: &mut DockspaceSession) {
    let (settled, output) = paint(session, motion(6_000));
    assert!(
        settled.partition,
        "a selection during motion must not promote an unpartitioned presentation sample into the permanent layout target"
    );
    assert_eq!(settled.splitters, 1);
    assert!(!session.surface_motion_is_active(SURFACE));
    present(session, output);

    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let mut targets = 0;
    plan.visit_surface_program(|step| {
        if matches!(step, Step::DropTarget(_)) {
            targets += 1;
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    assert!(
        targets > 0,
        "the settled root must retain actual dock targets"
    );
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
}

#[test]
fn selecting_a_tab_mid_motion_keeps_a_complete_final_docking_partition() {
    let mut session = moving_session();
    let mut frame = session.begin_host_frame().unwrap();
    frame.select_item_current(A).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    assert!(frame.commit().unwrap().workspace_changed());
    let (_, output) = paint(&mut session, motion(1_060));
    assert!(session.view().item(A).unwrap().is_selected());
    present(&mut session, output);
    assert_final_partition(&mut session);
}

#[test]
fn cancelling_a_tab_press_mid_motion_restores_a_complete_docking_partition() {
    let mut session = moving_session();
    session.enable_host_pointer().unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    let descriptor = frame
        .paint_plan(SURFACE)
        .unwrap()
        .unwrap()
        .tab_receiver(A)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    let receiver = session.bind_presented_receiver(&descriptor).unwrap();
    let surface = session.presented_surface(SURFACE).unwrap();
    let route = HostPointerRoute::new(surface, receiver.center());
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer_batch([HostPointerInput::new(
            SurfacePointerId::new(1),
            SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
            route,
            route,
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::delivery(&receiver).with_clear_hover_lane(&surface),
        )])
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    assert!(session.has_active_gesture());
    assert!(session.view().item(A).unwrap().is_selected());
    let (_, output) = paint(&mut session, motion(1_060));
    present(&mut session, output);

    let surface = session.presented_surface(SURFACE).unwrap();
    let route = HostPointerRoute::new(surface, receiver.center());
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer_batch([HostPointerInput::new(
            SurfacePointerId::new(1),
            SurfacePointerEvent::StreamCancelled(
                SurfacePointerCancelReason::ExplicitPlatformCancellation,
            ),
            route,
            route,
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::unknown(),
        )])
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    assert!(!session.has_active_gesture());
    assert_final_partition(&mut session);
}

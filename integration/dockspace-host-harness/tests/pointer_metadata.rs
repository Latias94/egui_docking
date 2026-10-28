//! Event-time facts remain part of the existing rollbackable public host input.

use std::time::Duration;

use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceRuntimeErrorKind, DockspaceSession, HostPointerInput,
    HostPointerRoute, PointerDeviceKind, PointerEventMetadata, SurfacePointerCapture,
    SurfacePointerEvent, SurfacePointerId, SurfacePointerReceiverFacts, SurfaceUnavailableReason,
};

fn session() -> DockspaceSession {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SurfaceId::new(1),
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::central_tabs([ItemId::new(1)]),
        ),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session.enable_host_pointer().unwrap();
    session
}

fn sample(
    pointer: u64,
    event: SurfacePointerEvent,
    device: PointerDeviceKind,
    millis: u64,
) -> HostPointerInput<'static> {
    HostPointerInput::new(
        SurfacePointerId::new(pointer),
        event,
        HostPointerRoute::Unknown,
        HostPointerRoute::Unknown,
        SurfacePointerCapture::Unknown,
        SurfacePointerReceiverFacts::unknown(),
    )
    .with_metadata(PointerEventMetadata::new(
        device,
        Duration::from_millis(millis),
    ))
}

fn commit(mut frame: DockspaceHostFrame<'_>) {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
}

#[test]
fn event_metadata_rejected_suffix_and_dropped_frame_leave_time_and_sequence_retryable() {
    let mut session = session();
    let before = session.version();
    let initial = sample(1, SurfacePointerEvent::Moved, PointerDeviceKind::Mouse, 10);
    {
        let mut frame = session.begin_host_frame().unwrap();
        frame.submit_host_pointer(initial).unwrap();
    }
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_host_pointer(initial).unwrap();
    commit(frame);
    let mut invalid = session.begin_host_frame().unwrap();
    let error = invalid
        .submit_host_pointer_batch([
            sample(1, SurfacePointerEvent::Moved, PointerDeviceKind::Mouse, 30),
            sample(2, SurfacePointerEvent::Moved, PointerDeviceKind::Touch, 20),
        ])
        .unwrap_err();
    assert_eq!(error.kind(), DockspaceRuntimeErrorKind::HostProtocol);
    drop(invalid);
    assert_eq!(session.version(), before);
    let mut retry = session.begin_host_frame().unwrap();
    retry
        .submit_host_pointer_batch([
            sample(1, SurfacePointerEvent::Moved, PointerDeviceKind::Mouse, 20),
            sample(2, SurfacePointerEvent::Moved, PointerDeviceKind::Touch, 20),
        ])
        .unwrap();
    commit(retry);
    let mut old_time = session.begin_host_frame().unwrap();
    assert!(
        old_time
            .submit_host_pointer(sample(
                3,
                SurfacePointerEvent::Moved,
                PointerDeviceKind::Pen,
                19
            ))
            .is_err()
    );
}

#[test]
fn event_metadata_stream_end_allows_device_reuse_without_resetting_provider_time() {
    let mut session = session();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(sample(
            7,
            SurfacePointerEvent::Moved,
            PointerDeviceKind::Touch,
            40,
        ))
        .unwrap();
    commit(frame);
    let mut changed = session.begin_host_frame().unwrap();
    assert!(
        changed
            .submit_host_pointer(sample(
                7,
                SurfacePointerEvent::Moved,
                PointerDeviceKind::Mouse,
                41
            ))
            .is_err()
    );
    drop(changed);
    let mut terminal = session.begin_host_frame().unwrap();
    terminal
        .submit_host_pointer(sample(
            7,
            SurfacePointerEvent::StreamEnded,
            PointerDeviceKind::Touch,
            40,
        ))
        .unwrap();
    commit(terminal);
    let mut reused = session.begin_host_frame().unwrap();
    reused
        .submit_host_pointer(sample(
            7,
            SurfacePointerEvent::Moved,
            PointerDeviceKind::Mouse,
            41,
        ))
        .unwrap();
    commit(reused);
    let mut old_time = session.begin_host_frame().unwrap();
    assert!(
        old_time
            .submit_host_pointer(sample(
                8,
                SurfacePointerEvent::Moved,
                PointerDeviceKind::Pen,
                39
            ))
            .is_err()
    );
}

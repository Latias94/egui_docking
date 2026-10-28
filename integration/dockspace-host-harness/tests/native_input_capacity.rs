use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceSession, NativeDesktopPointerLocation, NativeDesktopPosition, NativeHostErrorKind,
    NativeInputStorageLimit, NativePointerEvent, NativePointerHover, NativePointerId,
    NativePointerInput, NativePointerOwner, NativePointerRoster, SurfaceUnavailableReason,
};
use std::num::NonZeroUsize;

#[test]
fn bounded_native_host_retries_failed_enrollment_and_unaccepted_input_through_public_api() {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SurfaceId::new(1),
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::central_tabs([ItemId::new(1)]),
        ),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let slots = NonZeroUsize::new(1).unwrap();
    let refused = session
        .enable_managed_native_host_with_input_storage_limit(
            NativePointerRoster::Exact(Vec::new()),
            NativeInputStorageLimit::new(slots, 0),
        )
        .unwrap_err();
    assert_eq!(
        refused.native_kind(),
        Some(NativeHostErrorKind::InputCapacity)
    );
    assert_eq!(session.native_input_recorded_count(), 0);
    assert_eq!(session.native_input_owned_capacity_bytes().unwrap(), 0);

    let limit = NativeInputStorageLimit::new(slots, 1 << 20);
    session
        .enable_managed_native_host_with_input_storage_limit(
            NativePointerRoster::Exact(Vec::new()),
            limit,
        )
        .unwrap();
    let moved = NativePointerInput::new(
        NativePointerId::new(7),
        NativePointerEvent::Moved,
        NativeDesktopPointerLocation::new(
            NativeDesktopPosition::Unknown,
            NativePointerHover::Unknown,
            None,
        ),
        NativePointerOwner::Unknown,
        NativePointerOwner::Unknown,
    );
    assert_eq!(
        session
            .record_native_pointer(moved)
            .unwrap_err()
            .native_kind(),
        Some(NativeHostErrorKind::InputCapacity)
    );
    assert_eq!(session.native_input_recorded_count(), 1);
    for expected in [1, 2] {
        for commit in [false, true] {
            let mut frame = session
                .begin_native_host_frame(|_| {
                    panic!("unknown pointer locations cannot ask for a receiver")
                })
                .unwrap();
            assert!(frame.native_input_owned_capacity_bytes().unwrap() <= limit.byte_limit());
            assert!(!frame.native_input_pending());
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            if commit {
                assert!(!frame.commit().unwrap().native_input_pending());
            }
        }
        assert_eq!(session.native_input_recorded_count(), expected);
        if expected == 1 {
            assert_eq!(
                session
                    .record_native_pointer_sequence([moved, moved])
                    .unwrap_err()
                    .native_kind(),
                Some(NativeHostErrorKind::InputCapacity),
            );
            assert_eq!(session.native_input_recorded_count(), expected);
            session.record_native_pointer(moved).unwrap();
        }
    }
}

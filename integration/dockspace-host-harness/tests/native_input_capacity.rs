use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceSession, HostInputOutcome, HostWindowToken, NativeDesktopPointerLocation,
    NativeDesktopPosition, NativeHostCapabilities, NativeHostErrorKind, NativeInputStorageLimit,
    NativePointerButton, NativePointerEvent, NativePointerHover, NativePointerId,
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
    assert_eq!(session.native_input_pending_report_capacity_bytes(), 0);
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
    assert_eq!(session.native_input_pending_report_capacity_bytes(), 0);

    let limit = NativeInputStorageLimit::new(slots, 1 << 20);
    session
        .enable_managed_native_host_with_input_storage_limit(
            NativePointerRoster::Exact(Vec::new()),
            limit,
        )
        .unwrap();
    let report_data = session.native_input_pending_report_capacity_bytes();
    assert!(report_data > 0);
    assert!(session.has_pending_native_input());
    assert_eq!(session.prepare_native_pointer_capacity().unwrap(), Some(0));
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
        assert!(!session.has_pending_native_input());
        assert_eq!(session.prepare_native_pointer_capacity().unwrap(), Some(1));
        assert_eq!(
            session.native_input_pending_report_capacity_bytes(),
            report_data,
            "pointer-only frames do not create pending renderer reports"
        );
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

#[test]
fn one_public_binding_terminal_fact_fits_one_slot_after_five_independent_pressed_streams() {
    let surface = SurfaceId::new(1);
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        surface,
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::central_tabs([ItemId::new(1)]),
        ),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session
        .enable_managed_native_host_with_input_storage_limit(
            NativePointerRoster::Exact(Vec::new()),
            NativeInputStorageLimit::new(NonZeroUsize::new(1).unwrap(), 1 << 20),
        )
        .unwrap();
    session
        .configure_managed_native_capabilities(NativeHostCapabilities::none_supported())
        .unwrap();
    let commit = |session: &mut DockspaceSession| {
        let mut frame = session
            .begin_native_host_frame(|_| {
                panic!("unknown desktop positions cannot query presentation receivers")
            })
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        frame.commit().unwrap()
    };
    commit(&mut session);
    session
        .register_native_root(surface, HostWindowToken::new(1))
        .unwrap();
    let report = commit(&mut session);
    let [HostInputOutcome::NativeSurfaceRegistered { binding }] = report.inputs() else {
        panic!("registration must return its exact binding");
    };
    let binding = *binding;
    let press = |pointer| {
        NativePointerInput::new(
            NativePointerId::new(pointer),
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            NativeDesktopPointerLocation::new(
                NativeDesktopPosition::Unknown,
                NativePointerHover::Unknown,
                None,
            ),
            NativePointerOwner::Native(binding),
            NativePointerOwner::Unknown,
        )
    };
    for pointer in 1..=5 {
        session.record_native_pointer(press(pointer)).unwrap();
        commit(&mut session);
    }
    let before = session.native_input_recorded_count();
    let version = session.version();
    session
        .report_native_input_binding_retired(binding)
        .unwrap();
    assert_eq!(session.native_input_recorded_count(), before + 1);
    assert_eq!(session.prepare_native_pointer_capacity().unwrap(), Some(0));
    drop(session.begin_native_host_frame(|_| unreachable!()).unwrap());
    assert!(session.has_pending_native_input());
    assert_eq!(session.version(), version);
    let report = commit(&mut session);
    assert_eq!(
        report.inputs(),
        [HostInputOutcome::NativeInputBindingRetired { binding }]
    );
    assert!(!session.has_pending_native_input());
    assert!(session.is_current_native_binding(binding));
    session.record_native_pointer(press(1)).unwrap();
    commit(&mut session);
    assert_eq!(session.native_input_recorded_count(), before + 2);
}

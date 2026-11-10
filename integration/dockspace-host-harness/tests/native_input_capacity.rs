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
fn public_producer_limit_keeps_accepted_registration_and_quiescence_progress_without_reconfiguration()
 {
    use dockspace::runtime::NativeProducerStorageLimit;

    let layout = |identities: &[u64]| {
        DockspaceLayout::new(identities.iter().copied().map(|identity| {
            DockspaceSurfaceLayout::new(
                SurfaceId::new(identity),
                DockspaceRootLayout::new(
                    RootId::new(identity),
                    DockspaceNode::central_tabs([ItemId::new(identity)]),
                ),
            )
        }))
        .unwrap()
    };
    let mut session =
        DockspaceSession::from_layout(layout(&[1, 2]), DockPolicy::default()).unwrap();
    let producer = NativeProducerStorageLimit::new(NonZeroUsize::MIN, NonZeroUsize::MIN, 1 << 20);
    session
        .enable_managed_native_host_with_input_storage_limit(
            NativePointerRoster::Exact(Vec::new()),
            NativeInputStorageLimit::new(NonZeroUsize::new(4).unwrap(), 1 << 20)
                .with_producer_storage_limit(producer),
        )
        .unwrap();
    assert_eq!(
        session.native_input_producer_storage_reserved_bytes(),
        Some(producer.byte_limit())
    );
    session
        .register_native_root(SurfaceId::new(1), HostWindowToken::new(1))
        .unwrap();
    let recorded = session.native_input_recorded_count();
    let version = session.version();
    let backing = session.native_input_producer_storage_capacity_bytes();
    assert_eq!(
        session
            .register_native_root(SurfaceId::new(2), HostWindowToken::new(2))
            .unwrap_err()
            .native_kind(),
        Some(NativeHostErrorKind::InputCapacity)
    );
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert_eq!(session.version(), version);

    let mut frame = session.begin_native_host_frame(|_| unreachable!()).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    drop(frame);
    assert_eq!(session.version(), version);
    assert_eq!(session.native_input_recorded_count(), recorded);
    let mut frame = session.begin_native_host_frame(|_| unreachable!()).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    let binding = report
        .inputs()
        .iter()
        .find_map(|input| match input {
            HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
            _ => None,
        })
        .unwrap();
    assert_eq!(report.native_bindings(), [binding]);
    assert!(!session.has_pending_native_input());
    assert_eq!(
        session.native_input_producer_storage_capacity_bytes(),
        backing
    );

    let reset = session.prepare_layout_reset(layout(&[2])).unwrap();
    let mut frame = session
        .begin_native_layout_reset_frame(&reset, |_| unreachable!())
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    assert!(!session.is_current_native_binding(binding));
    assert!(session.recognizes_native_binding(binding));
    assert_eq!(
        session
            .register_native_root(SurfaceId::new(2), HostWindowToken::new(2))
            .unwrap_err()
            .native_kind(),
        Some(NativeHostErrorKind::InputCapacity)
    );
    assert_eq!(session.native_input_recorded_count(), recorded);

    session.report_native_binding_quiescence(binding).unwrap();
    assert!(!session.recognizes_native_binding(binding));
    session
        .register_native_root(SurfaceId::new(2), HostWindowToken::new(2))
        .unwrap();
    let mut frame = session.begin_native_host_frame(|_| unreachable!()).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert_eq!(report.native_bindings().len(), 1);
    assert_eq!(report.native_bindings()[0].surface(), SurfaceId::new(2));
    assert_ne!(report.native_bindings()[0], binding);
    assert_eq!(session.native_input_recorded_count(), recorded + 2);
    assert_eq!(
        session.native_input_producer_storage_capacity_bytes(),
        backing
    );
    assert_eq!(
        session.native_input_producer_storage_reserved_bytes(),
        Some(producer.byte_limit())
    );
}

#[test]
fn public_report_allowance_defers_encoding_and_reuses_a_prepaid_output_slot() {
    use dockspace::geometry::{LogicalRect, LogicalSize};
    use dockspace::runtime::{
        NativeReportStorageLimit, SurfacePresentationResult, SurfaceWork, SurfaceWorkDeferred,
        UniformSurfaceMetrics,
    };

    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SurfaceId::new(1),
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::central_tabs([ItemId::new(1)]),
        ),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let one = NonZeroUsize::new(1).unwrap();
    let reports = NativeReportStorageLimit::new(one, one, 1 << 20);
    session
        .enable_managed_native_host_with_input_storage_limit(
            NativePointerRoster::Exact(Vec::new()),
            NativeInputStorageLimit::new(NonZeroUsize::new(4).unwrap(), 1 << 20)
                .with_report_storage_limit(reports),
        )
        .unwrap();
    assert_eq!(
        session.native_input_report_storage_reserved_bytes(),
        Some(reports.byte_limit())
    );
    let mut frame = session.begin_native_host_frame(|_| unreachable!()).unwrap();
    frame
        .measure_surface(
            SurfaceId::new(1),
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
                LogicalSize::new(32.0, 24.0).unwrap(),
                80.0,
            )
            .unwrap(),
        )
        .unwrap();
    let receipt = frame
        .paint_plan(SurfaceId::new(1))
        .unwrap()
        .unwrap()
        .execute_surface_program(|_| Ok::<(), ()>(()))
        .unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    let first = frame
        .commit()
        .unwrap()
        .take_painted_outputs()
        .pop()
        .unwrap();
    let first_artifact = first.presentation_artifact();
    let version = session.version();
    let recorded = session.native_input_recorded_count();
    let bytes = session.native_input_pending_report_capacity_bytes();

    session.report_native_input_device_removal().unwrap();
    assert!(session.has_pending_native_input());
    assert_eq!(session.native_input_recorded_count(), recorded + 1);
    let mut frame = session.begin_native_host_frame(|_| unreachable!()).unwrap();
    assert!(matches!(
        frame.surface_work(SurfaceId::new(1)).unwrap(),
        SurfaceWork::Deferred(SurfaceWorkDeferred::OutputCapacity)
    ));
    assert!(frame.paint_plan(SurfaceId::new(1)).unwrap().is_none());
    let mut terminal = frame.commit().unwrap();
    assert_eq!(
        terminal.inputs(),
        &[HostInputOutcome::NativeInputDeviceRemoval]
    );
    assert!(terminal.take_painted_outputs().is_empty());
    assert!(terminal.take_native_effects().is_empty());
    assert!(terminal.surface_presentation_commits().is_empty());
    assert_eq!(session.version(), version);
    assert_eq!(session.native_input_recorded_count(), recorded + 1);
    assert!(!session.has_pending_native_input());
    assert_eq!(session.native_input_pending_report_capacity_bytes(), bytes);

    let mut frame = session.begin_native_host_frame(|_| unreachable!()).unwrap();
    assert!(matches!(
        frame.surface_work(SurfaceId::new(1)).unwrap(),
        SurfaceWork::Deferred(SurfaceWorkDeferred::OutputCapacity)
    ));
    let mut idle = frame.commit().unwrap();
    assert!(idle.inputs().is_empty());
    assert!(idle.take_painted_outputs().is_empty());
    assert_eq!(session.native_input_recorded_count(), recorded + 1);

    session
        .report_surface_presentation(first, SurfacePresentationResult::Presented)
        .unwrap();
    let mut frame = session.begin_native_host_frame(|_| unreachable!()).unwrap();
    let plan = match frame.surface_work(SurfaceId::new(1)).unwrap() {
        SurfaceWork::Ready(plan) => plan,
        SurfaceWork::Deferred(reason) => {
            panic!("the reported output must release its slot in this update: {reason:?}")
        }
    };
    let receipt = plan.execute_surface_program(|_| Ok::<(), ()>(())).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    let mut report = frame.commit().unwrap();
    assert!(report.surface_presentation_commits()[0].presents(first_artifact));
    let second = report.take_painted_outputs().pop().unwrap();
    let second_artifact = second.presentation_artifact();
    let bytes = session.native_input_pending_report_capacity_bytes();
    std::thread::spawn(move || drop(second)).join().unwrap();
    assert_eq!(session.native_input_pending_report_capacity_bytes(), bytes);
    let mut frame = session.begin_native_host_frame(|_| unreachable!()).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert!(report.surface_presentation_commits()[0].retires(second_artifact));
    assert_eq!(
        session.native_input_report_storage_reserved_bytes(),
        Some(reports.byte_limit())
    );
    assert!(session.native_input_pending_report_capacity_bytes() <= reports.byte_limit());
}

#[test]
fn prepaid_stream_capacity_allows_a_drained_headless_surface_to_acquire_a_native_endpoint() {
    use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalSize, ScaleFactor};
    use dockspace::runtime::{
        NativeHostCapability, NativeReportStorageLimit, NativeWindowFacts, NativeWindowInputState,
        NativeWindowPresentationState, NativeWorkAreaRoster, SurfacePresentationResult,
        SurfaceWork, SurfaceWorkDeferred, UniformSurfaceMetrics,
    };

    let surface = SurfaceId::new(1);
    let item = ItemId::new(1);
    let metrics = || {
        UniformSurfaceMetrics::new(
            LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
            LogicalSize::new(32.0, 24.0).unwrap(),
            80.0,
        )
        .unwrap()
    };
    let settle = |session: &mut DockspaceSession| {
        let mut frame = session.begin_native_host_frame(|_| unreachable!()).unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        frame.commit().unwrap()
    };
    for settle_before_registration in [true, false] {
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            surface,
            DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([item])),
        )])
        .unwrap();
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
        let reports = NativeReportStorageLimit::new(NonZeroUsize::MIN, NonZeroUsize::MIN, 1 << 20);
        session
            .enable_managed_native_host_with_input_storage_limit(
                NativePointerRoster::Exact(Vec::new()),
                NativeInputStorageLimit::new(NonZeroUsize::new(4).unwrap(), 1 << 20)
                    .with_report_storage_limit(reports),
            )
            .unwrap();
        session
            .configure_managed_native_capabilities(
                NativeHostCapabilities::none_supported()
                    .with(NativeHostCapability::AuthoritativeInventory),
            )
            .unwrap();
        let mut frame = session.begin_native_host_frame(|_| unreachable!()).unwrap();
        frame.measure_surface(surface, metrics()).unwrap();
        let plan = frame.paint_plan(surface).unwrap().unwrap();
        let old_receiver = plan.tab_receiver(item).unwrap();
        let receipt = plan.execute_surface_program(|_| Ok::<(), ()>(())).unwrap();
        frame.confirm_surface_painted(receipt).unwrap();
        let first = frame
            .commit()
            .unwrap()
            .take_painted_outputs()
            .pop()
            .unwrap();
        let first_artifact = first.presentation_artifact();
        let mut first = Some(first);
        if settle_before_registration {
            session
                .report_surface_presentation(
                    first.take().unwrap(),
                    SurfacePresentationResult::Presented,
                )
                .unwrap();
            assert!(
                settle(&mut session).surface_presentation_commits()[0].presents(first_artifact)
            );
        }

        session
            .register_native_root(surface, HostWindowToken::new(1))
            .unwrap();
        let registration = settle(&mut session);
        let [binding] = registration.native_bindings() else {
            panic!("registration must publish the exact native root binding");
        };
        let binding = *binding;
        session
            .report_managed_native_snapshot(
                [(
                    binding,
                    NativeWindowFacts::live()
                        .with_content_size(PhysicalSize::new(640.0, 480.0).unwrap())
                        .with_native_scale_factor(ScaleFactor::new(1.0).unwrap())
                        .with_presentation_scale_factor(ScaleFactor::new(1.0).unwrap())
                        .with_input(NativeWindowInputState::ReceivesInput, None)
                        .with_presentation(NativeWindowPresentationState::Visible),
                )],
                NativeWorkAreaRoster::Unknown,
            )
            .unwrap();
        settle(&mut session);
        if let Some(first) = first {
            for _ in 0..2 {
                let mut frame = session.begin_native_host_frame(|_| unreachable!()).unwrap();
                frame.measure_surface(surface, metrics()).unwrap();
                assert!(matches!(
                    frame.surface_work(surface).unwrap(),
                    SurfaceWork::Deferred(SurfaceWorkDeferred::OutputCapacity)
                ));
                assert!(frame.commit().unwrap().take_painted_outputs().is_empty());
            }
            session
                .report_surface_presentation(first, SurfacePresentationResult::Presented)
                .unwrap();
        }

        // Both abandonment and a rejected commit must leave the handoff retryable.
        for attempt in 0..3 {
            let version = session.version();
            let presented = session.presented_surface(surface);
            let mut frame = session.begin_native_host_frame(|_| unreachable!()).unwrap();
            frame.measure_surface(surface, metrics()).unwrap();
            let plan = match frame.surface_work(surface).unwrap() {
                SurfaceWork::Ready(plan) => plan,
                SurfaceWork::Deferred(reason) => {
                    panic!("a drained predecessor must permit endpoint handoff: {reason:?}")
                }
            };
            let receipt = plan.execute_surface_program(|_| Ok::<(), ()>(())).unwrap();
            frame.confirm_surface_painted(receipt).unwrap();
            if attempt == 2 {
                let output = frame
                    .commit()
                    .unwrap()
                    .take_painted_outputs()
                    .pop()
                    .unwrap();
                assert!(output.matches_native_binding(binding));
                let artifact = output.presentation_artifact();
                session
                    .report_surface_presentation(output, SurfacePresentationResult::Presented)
                    .unwrap();
                settle(&mut session);
                assert!(
                    session
                        .presented_surface(surface)
                        .unwrap()
                        .matches_presentation_artifact(artifact)
                );
            } else if attempt == 1 {
                assert!(frame.select_item_current(item).is_err());
                assert!(
                    frame
                        .defer_surface(surface, SurfaceUnavailableReason::SurfaceUnavailable)
                        .is_err()
                );
                assert!(frame.commit().is_err());
                assert_eq!(session.presented_surface(surface), presented);
            } else {
                drop(frame);
                assert_eq!(session.presented_surface(surface), presented);
            }
            assert_eq!(session.version(), version);
            assert!(session.bind_presented_receiver(&old_receiver).is_none());
        }
        assert_eq!(
            session.native_input_report_storage_reserved_bytes(),
            Some(reports.byte_limit())
        );
        assert!(session.native_input_pending_report_capacity_bytes() <= reports.byte_limit());
    }
}

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

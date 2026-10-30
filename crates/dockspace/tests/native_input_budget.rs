use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceSession, HostFrameReport, HostInputOutcome,
    NativeDesktopPointerLocation, NativeDesktopPosition, NativeInputBudget, NativeMonotonicClock,
    NativePointerEvent, NativePointerHover, NativePointerId, NativePointerInput,
    NativePointerOwner, NativePointerRoster, NativeReceiverAnswer, SurfaceUnavailableReason,
};
use std::time::Duration;

fn commit(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the input-only frame must settle all surfaces");
    frame.commit().expect("the input prefix must commit")
}

#[test]
fn public_device_removal_records_one_retryable_fact_without_ending_the_input_phase() {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SurfaceId::new(1),
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::central_tabs([ItemId::new(1)]),
        ),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session
        .enable_managed_native_host(
            dockspace::runtime::NativeCreateStrategy::Staged,
            NativePointerRoster::Exact(Vec::new()),
        )
        .unwrap();
    commit(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    let input = |id| {
        NativePointerInput::new(
            NativePointerId::new(id),
            NativePointerEvent::Moved,
            NativeDesktopPointerLocation::new(
                NativeDesktopPosition::Unknown,
                NativePointerHover::Unknown,
                None,
            ),
            NativePointerOwner::Unknown,
            NativePointerOwner::Unknown,
        )
    };
    let before = session.native_input_recorded_count();
    session.record_native_pointer(input(1)).unwrap();
    session.report_native_input_device_removal().unwrap();
    session.record_native_pointer(input(2)).unwrap();
    assert_eq!(session.native_input_recorded_count(), before + 3);
    drop(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    assert_eq!(session.native_input_recorded_count(), before + 3);
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.select_item_current(ItemId::new(1)).unwrap();
    let report = commit(frame);
    assert_eq!(
        report
            .inputs()
            .iter()
            .filter(|outcome| **outcome == HostInputOutcome::NativeInputDeviceRemoval)
            .count(),
        1,
    );
    assert!(
        report
            .inputs()
            .iter()
            .any(|outcome| matches!(outcome, HostInputOutcome::ProductActionApplied(_)))
    );
    assert_eq!(session.native_input_recorded_count(), before + 3);
}

#[test]
fn public_budgeted_host_distinguishes_committable_prefixes_from_atomic_continuations() {
    for atomic in [false, true] {
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SurfaceId::new(1),
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1)]),
            ),
        )])
        .expect("the application layout must validate");
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default())
            .expect("the renderer-neutral session must initialize");
        assert_eq!(session.native_input_recorded_count(), 0);
        assert_eq!(session.native_input_owned_capacity_bytes().unwrap(), 0);
        session
            .enable_managed_native_host(
                dockspace::runtime::NativeCreateStrategy::Staged,
                NativePointerRoster::Exact(Vec::new()),
            )
            .expect("the native provider must enroll");
        commit(
            session
                .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
                .expect("the initial authority checkpoint must settle"),
        );
        let version = session.version();
        let recorded_before = session.native_input_recorded_count();
        let inputs = (0..257).map(|_| {
            NativePointerInput::new(
                NativePointerId::new(1),
                NativePointerEvent::Moved,
                NativeDesktopPointerLocation::new(
                    NativeDesktopPosition::Unknown,
                    NativePointerHover::Unknown,
                    None,
                ),
                NativePointerOwner::Unknown,
                NativePointerOwner::Unknown,
            )
        });
        if atomic {
            session.record_native_pointer_batch(inputs)
        } else {
            session.record_native_pointer_sequence(inputs)
        }
        .expect("the complete input transaction must record");
        assert_eq!(session.native_input_recorded_count(), recorded_before + 257);
        let recorded_bytes = session.native_input_owned_capacity_bytes().unwrap();
        assert!(recorded_bytes > 0);

        let clock = NativeMonotonicClock::new(|| Duration::ZERO);
        let mut first_budget = NativeInputBudget::new(&clock);
        let first = session
            .poll_native_host_frame(&mut first_budget, |_| {
                panic!("unknown locations cannot authorize a receiver query")
            })
            .expect("the first budgeted attempt must succeed");
        assert_eq!(first_budget.remaining_records(), 0);
        if atomic {
            assert!(first.is_none(), "an atomic interior cannot publish");
        } else {
            let first = first.expect("an independent prefix can publish");
            assert!(first.native_input_owned_capacity_bytes().unwrap() > recorded_bytes);
            assert!(first.native_input_pending());
            assert!(commit(first).native_input_pending());
        }
        if atomic {
            assert!(session.native_input_owned_capacity_bytes().unwrap() > recorded_bytes);
        }
        assert_eq!(session.version(), version);
        let mut second_budget = NativeInputBudget::new(&clock);
        let second = session
            .poll_native_host_frame(&mut second_budget, |_| NativeReceiverAnswer::Unknown)
            .expect("the next poll must resume the exact accepted suffix")
            .expect("the final record completes its commit group");
        assert_eq!(second_budget.remaining_records(), 255);
        assert!(!second.native_input_pending());
        assert!(!commit(second).native_input_pending());
        assert_eq!(session.version(), version);
        assert_eq!(session.native_input_recorded_count(), recorded_before + 257);
    }
}

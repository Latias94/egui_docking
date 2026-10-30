use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceSession, HostInputOutcome, NativeDesktopPointerLocation, NativeDesktopPosition,
    NativeInputBudget, NativeMonotonicClock, NativePointerEvent, NativePointerHover,
    NativePointerId, NativePointerInput, NativePointerOwner, NativePointerRoster,
    NativeReceiverAnswer, SurfaceUnavailableReason,
};
use std::time::Duration;

#[test]
fn overload_terminal_remains_available_after_the_entire_ordinary_input_budget_is_used() {
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
    let mut enrollment = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    enrollment
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    enrollment.commit().unwrap();
    let version = session.version();
    session
        .record_native_pointer_sequence((0..256).map(|_| {
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
        }))
        .unwrap();
    let recorded = session.native_input_recorded_count();
    let clock = NativeMonotonicClock::new(|| Duration::ZERO);
    let mut budget = NativeInputBudget::new(&clock);
    let mut frame = session
        .poll_native_host_frame(&mut budget, |_| NativeReceiverAnswer::Unknown)
        .unwrap()
        .expect("the final ordinary record seals a complete prefix");
    assert_eq!(budget.remaining_records(), 0);
    assert!(!frame.native_input_pending());
    let submitted = frame
        .cancel_native_input_for_overload()
        .expect("terminal cancellation requires no additional journal record or budget slot");
    assert!(frame.select_item_current(ItemId::new(1)).is_err());
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert_eq!(
        report.submitted_action_outcome(submitted),
        Some(&HostInputOutcome::NativeInputCancelledForOverload)
    );
    assert!(!report.workspace_changed());
    assert_eq!(session.version(), version);
    assert_eq!(session.native_input_recorded_count(), recorded);
}

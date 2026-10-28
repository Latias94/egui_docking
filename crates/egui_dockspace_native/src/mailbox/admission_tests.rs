use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceSession, HostInputOutcome, HostWindowToken, NativePointerRoster,
    SurfaceUnavailableReason,
};
use winit::dpi::PhysicalPosition;
use winit::event::{DeviceId, PointerEventFacts, WindowEvent};

use super::*;

fn bridge_with_clock() -> (
    NativeHostBridge,
    Arc<AtomicU64>,
    NativeSurfaceBinding,
    WindowId,
) {
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
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    session
        .register_native_root(SurfaceId::new(1), HostWindowToken::new(1))
        .unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let binding = frame
        .commit()
        .unwrap()
        .inputs()
        .iter()
        .find_map(|input| match input {
            HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
            _ => None,
        })
        .unwrap();
    let window = WindowId::from(11);
    let mut routes = NativeViewportMap::default();
    routes.bind(ViewportId::ROOT, window, binding).unwrap();
    let now = Arc::new(AtomicU64::new(0));
    let source = Arc::clone(&now);
    let bridge = NativeHostBridge::with_clock(
        routes,
        NativeMonotonicClock::new(move || Duration::from_millis(source.load(Ordering::SeqCst))),
    );
    (bridge, now, binding, window)
}

fn cursor(ordinal: u64, window: WindowId, binding: NativeSurfaceBinding) -> HostRecord {
    HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
        ordinal,
        window,
        Some(ViewportId::ROOT),
        Some(binding),
        WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(10.0, 20.0),
            facts: PointerEventFacts::default(),
        },
    ))
}

#[test]
fn ordinary_record_limit_freezes_before_minting_and_preserves_post_boundary_command_owner() {
    let (bridge, now, binding, window) = bridge_with_clock();
    let context = eframe::egui::Context::default();
    let token = eframe::queue_native_viewport_pointer_passthrough(&context, ViewportId::ROOT, true);
    assert!(bridge.reserve_pointer_callback(ViewportId::ROOT, binding, window, token, true));

    // Exercise capture admission rather than the journal's unguarded test lane.
    for ordinal in 1..=4_096 {
        bridge.push_record(cursor(ordinal, window, binding));
    }
    {
        let records = bridge.lock();
        assert_eq!(records.journal.captured_through(), 4_096);
        assert_eq!(records.journal.len(), 4_096);
        assert_eq!(records.retained_record_count(), 4_096);
        assert_eq!(records.admission, NativeAdmission::Running);
    }
    bridge.push_record(cursor(4_097, window, binding));
    {
        let records = bridge.lock();
        assert_eq!(records.journal.captured_through(), 4_096);
        assert_eq!(records.retained_record_count(), 4_096);
        assert_eq!(
            records.admission,
            NativeAdmission::Draining { boundary: 4_096 }
        );
    }
    assert_eq!(
        bridge.admission_dispatch(),
        NativeDispatch::ContinueBoundary
    );

    now.store(10, Ordering::SeqCst);
    let callback = NativePointerPassthroughRecord::for_test(
        token,
        ViewportId::ROOT,
        window,
        Some(binding),
        true,
        eframe::NativeViewportPointerPassthroughStatus::Applied,
    );
    bridge.push_record(HostRecord::ViewportPointerPassthrough(callback));
    // Only the already reserved command can acquire this post-B ordinal.
    bridge.push_record(cursor(4_098, window, binding));
    bridge.push_record(HostRecord::ViewportPointerPassthrough(callback));
    {
        let records = bridge.lock();
        assert_eq!(records.journal.captured_through(), 4_097);
        assert_eq!(records.journal.len(), 4_096);
        assert_eq!(records.retained_record_count(), 4_097);
        assert_eq!(
            records.admission,
            NativeAdmission::Draining { boundary: 4_096 }
        );
        let ingress = records.window_resources.callback_ingress().next().unwrap();
        assert_eq!(ingress.ordinal(), 4_097);
        assert_eq!(ingress.captured_at(), Duration::from_millis(10));
    }
    bridge.commit_frame_boundary(false);
    for _ in 0..16 {
        assert_eq!(
            bridge.admission_dispatch(),
            NativeDispatch::Replay {
                through: Some(4_096)
            },
        );
        let claim = bridge.claim_cursor_prefix(256).unwrap();
        let count = claim.record_count();
        assert_eq!(count.get(), 256);
        assert!(claim.events().all(|event| event.ordinal() <= 4_096));
        assert!(claim.retain_prefix(count));
        bridge.commit_frame_boundary(true);
        assert_eq!(
            bridge.admission_dispatch(),
            NativeDispatch::ContinueBoundary
        );
        bridge.commit_frame_boundary(false);
    }
    assert!(bridge.has_pending_callback_records());
    assert!(!bridge.has_pending_input());
    assert!(bridge.front_viewport_pointer_passthrough().is_none());
    assert!(!bridge.acknowledge_viewport_pointer_passthrough(callback));
    assert!(bridge.claim_cursor_prefix(256).is_none());
    assert_eq!(
        bridge.admission_dispatch(),
        NativeDispatch::Cancel { boundary: 4_096 },
    );
    assert!(!bridge.finish_overload_cancellation(4_097));
    assert_eq!(
        bridge.admission_dispatch(),
        NativeDispatch::Cancel { boundary: 4_096 },
    );
    assert!(bridge.finish_overload_cancellation(4_096));
    assert!(!bridge.finish_overload_cancellation(4_096));
    assert_eq!(
        bridge.admission_dispatch(),
        NativeDispatch::Replay { through: None },
    );
    assert_eq!(bridge.front_viewport_pointer_passthrough(), Some(callback));
    assert!(bridge.acknowledge_viewport_pointer_passthrough(callback));
    bridge.commit_frame_boundary(false);
    assert_eq!(bridge.lock().retained_record_count(), 0);
    assert_eq!(bridge.lock().journal.captured_through(), 4_097);
    assert!(!bridge.has_pending_callback_records());
}

#[test]
fn age_limit_freezes_at_250ms_and_preserves_the_exact_destroyed_owner_until_cancel() {
    let (bridge, now, binding, window) = bridge_with_clock();
    bridge.push_record(cursor(1, window, binding));
    now.store(249, Ordering::SeqCst);
    bridge.push_record(cursor(2, window, binding));
    assert_eq!(bridge.lock().journal.captured_through(), 2);
    assert_eq!(
        bridge.lock().oldest_record_age(),
        Some(Duration::from_millis(249)),
    );
    now.store(250, Ordering::SeqCst);
    bridge.push_record(cursor(3, window, binding));
    {
        let records = bridge.lock();
        assert_eq!(records.journal.captured_through(), 2);
        assert_eq!(records.retained_record_count(), 2);
        assert_eq!(records.admission, NativeAdmission::Draining { boundary: 2 });
    }
    assert_eq!(
        bridge.admission_dispatch(),
        NativeDispatch::ContinueBoundary
    );

    let destroyed = HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
        4,
        window,
        Some(ViewportId::ROOT),
        None,
        WindowEvent::Destroyed,
    ));
    bridge.push_record(destroyed.clone());
    bridge.push_record(destroyed);
    let original = {
        let records = bridge.lock();
        assert_eq!(records.journal.captured_through(), 3);
        assert_eq!(records.journal.len(), 2);
        assert_eq!(records.retained_record_count(), 3);
        assert_eq!(records.admission, NativeAdmission::Draining { boundary: 2 });
        let ingress = records.viewports.window_ingress().next().unwrap();
        assert!(matches!(
            ingress.record(),
            HostRecord::WindowEvent(event) if event.binding() == Some(binding)
                && event.window_id() == window && event.ordinal() == 4
        ));
        (ingress.ordinal(), ingress.captured_at())
    };
    assert_eq!(original, (3, Duration::from_millis(250)));
    bridge.commit_frame_boundary(false);
    for ordinal in [1, 2] {
        assert_eq!(
            bridge.admission_dispatch(),
            NativeDispatch::Replay { through: Some(2) },
        );
        assert_eq!(bridge.front_event().unwrap().ordinal(), ordinal);
        assert!(bridge.acknowledge_event(ordinal));
        bridge.commit_frame_boundary(true);
        assert_eq!(
            bridge.admission_dispatch(),
            NativeDispatch::ContinueBoundary
        );
        bridge.commit_frame_boundary(false);
    }
    assert!(bridge.has_pending_callback_records());
    assert!(!bridge.has_pending_input());
    assert!(bridge.front_event().is_none());
    assert!(!bridge.acknowledge_event(4));
    assert_eq!(
        bridge.admission_dispatch(),
        NativeDispatch::Cancel { boundary: 2 },
    );
    now.store(300, Ordering::SeqCst);
    assert!(bridge.finish_overload_cancellation(2));
    assert_eq!(
        bridge.admission_dispatch(),
        NativeDispatch::Replay { through: None },
    );
    assert_eq!(bridge.front_event().unwrap().ordinal(), 4);
    assert_eq!(
        bridge.retirement_observe_destroyed(ViewportId::ROOT, binding, window, 4),
        DestroyedObservation::Recorded,
    );
    {
        let records = bridge.lock();
        assert_eq!(records.viewports.window_ingress().count(), 0);
        let ingress = records.retirements.window_ingress().next().unwrap();
        assert_eq!((ingress.ordinal(), ingress.captured_at()), original);
        assert_eq!(records.oldest_record_age(), Some(Duration::from_millis(50)));
    }
    assert!(bridge.acknowledge_event(4));
    bridge.commit_frame_boundary(true);
    assert!(bridge.retirement_destroyed_observations().is_empty());
    assert_eq!(bridge.lock().retained_record_count(), 1);
    bridge.commit_frame_boundary(false);
    assert_eq!(
        bridge.retirement_destroyed_observations(),
        [(binding, NativeWindowFacts::destroyed())],
    );
    assert_eq!(bridge.lock().retained_record_count(), 0);
    assert_eq!(bridge.lock().journal.captured_through(), 3);
}

#[test]
fn in_flight_roster_settlement_keeps_cancellation_pending_after_full_commit() {
    let (bridge, now, binding, window) = bridge_with_clock();
    bridge.push_viewport_roster(NativeViewportRosterRecord::for_test([], true));
    now.store(250, Ordering::SeqCst);
    bridge.push_record(cursor(2, window, binding));
    assert_eq!(bridge.lock().journal.captured_through(), 1);
    assert_eq!(
        bridge.admission_dispatch(),
        NativeDispatch::ContinueBoundary
    );
    bridge.commit_frame_boundary(false);
    assert_eq!(
        bridge.admission_dispatch(),
        NativeDispatch::Replay { through: Some(1) },
    );
    let roster = bridge.front_viewport_roster().unwrap();
    assert!(bridge.acknowledge_viewport_roster(&roster));
    bridge.commit_frame_boundary(true);
    assert_eq!(
        bridge.admission_dispatch(),
        NativeDispatch::ContinueBoundary
    );
    bridge.commit_frame_boundary(false);
    assert!(!bridge.callback_boundary_pending());
    assert!(!bridge.has_pending_callback_records());
    assert!(!bridge.has_pending_input());
    assert!(bridge.lock().in_flight_viewport_roster.is_some());
    assert_eq!(
        bridge.admission_dispatch(),
        NativeDispatch::ContinueBoundary
    );
    assert!(bridge.settle_viewport_roster(&roster, false));
    assert_eq!(
        bridge.admission_dispatch(),
        NativeDispatch::Cancel { boundary: 1 },
    );
}

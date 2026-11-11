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

fn binding() -> NativeSurfaceBinding {
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
    frame
        .commit()
        .unwrap()
        .inputs()
        .iter()
        .find_map(|input| match input {
            HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
            _ => None,
        })
        .unwrap()
}

fn bridge_with_clock(routes: NativeViewportMap) -> (Arc<NativeHostBridge>, Arc<AtomicU64>) {
    let now = Arc::new(AtomicU64::new(0));
    let source = Arc::clone(&now);
    let bridge = Arc::new(NativeHostBridge::with_clock(
        routes,
        NativeMonotonicClock::new(move || Duration::from_millis(source.load(Ordering::SeqCst))),
    ));
    (bridge, now)
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

fn destroyed(ordinal: u64, window: WindowId) -> HostRecord {
    HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
        ordinal,
        window,
        Some(ViewportId::ROOT),
        None,
        WindowEvent::Destroyed,
    ))
}

#[test]
fn destroyed_input_retirement_requires_received_owner_and_moves_once_only_progress() {
    let binding = binding();
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(8);
    let mut routes = NativeViewportMap::default();
    routes.bind(viewport, window, binding).unwrap();
    let (bridge, _) = bridge_with_clock(routes);
    let uncaptured = NativeWindowEventRecord::for_test(
        90,
        window,
        Some(viewport),
        Some(binding),
        WindowEvent::Destroyed,
    );
    bridge
        .retire_destroyed_input(&uncaptured, || {
            panic!("a synthetic callback cannot authorize a core fact")
        })
        .unwrap_err();
    bridge.push_record(HostRecord::WindowEvent(uncaptured));
    let record = bridge.front_event().unwrap();
    for (event_window, event_viewport, event_ordinal) in [
        (WindowId::from(9), viewport, 90),
        (
            window,
            ViewportId::from_hash_of("foreign-destroyed-owner"),
            90,
        ),
        (window, viewport, 91),
    ] {
        let unrelated = NativeWindowEventRecord::for_test(
            event_ordinal,
            event_window,
            Some(event_viewport),
            Some(binding),
            WindowEvent::Destroyed,
        );
        bridge
            .retire_destroyed_input(&unrelated, || {
                panic!("only the exact received physical callback can record a fact")
            })
            .unwrap_err();
    }
    let mut attempts = 0;
    bridge
        .retire_destroyed_input(&record, || {
            attempts += 1;
            Err(NativeHostProtocolError::NativeCloseCorrelationChanged.into())
        })
        .unwrap_err();
    assert_eq!(bridge.front_event().unwrap().ordinal(), 90);
    bridge
        .retire_destroyed_input(&record, || {
            attempts += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        attempts, 2,
        "a failed record leaves the same callback retryable"
    );
    assert_eq!(
        bridge.retirement_observe_destroyed(viewport, binding, window, 90),
        DestroyedObservation::Recorded,
    );
    bridge
        .retire_destroyed_input(&record, || {
            panic!("moving the receipt into retirement must preserve recorded progress")
        })
        .unwrap();
    assert!(bridge.acknowledge_event(90));
    bridge.commit_frame_boundary(false);
    assert_eq!(
        bridge.retirement_destroyed_observations(),
        [(binding, NativeWindowFacts::destroyed())]
    );
}

#[test]
fn destroyed_capture_survives_created_window_index_publication() {
    let binding = binding();
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(9);
    let mut routes = NativeViewportMap::default();
    routes.reserve(viewport, binding).unwrap();
    assert!(routes.admit_test_create(viewport, binding));
    // This route model owns no opaque Created output token. It exercises the
    // actual callback capture between inline attachment and index publication.
    routes
        .capture_output_window(viewport, binding, window, None)
        .unwrap();
    let (bridge, now) = bridge_with_clock(routes);
    now.store(10, Ordering::SeqCst);
    bridge.push_record(destroyed(91, window));
    let original = {
        let records = bridge.lock();
        let ingress = records.viewports.window_ingress().next().unwrap();
        assert_eq!(records.journal.len(), 0);
        assert!(!records.retirements.has_pending_work());
        assert_eq!(records.viewports.pointer_binding_for_window(window), None);
        (ingress.ordinal(), ingress.captured_at())
    };
    bridge.with_viewports_mut(|routes| {
        routes
            .attach_output(viewport, binding, window, None)
            .unwrap();
        assert_eq!(
            routes.route_for_event(window, None),
            Some((viewport, binding))
        );
        assert_eq!(routes.pointer_binding_for_window(window), None);
        assert!(routes.has_received_destroyed(viewport, binding, window, 91));
        let ingress = routes.window_ingress().next().unwrap();
        assert_eq!((ingress.ordinal(), ingress.captured_at()), original);
    });
    bridge.push_record(destroyed(92, window));
    assert_eq!(bridge.lock().journal.captured_through(), original.0);
    assert_eq!(bridge.front_event().unwrap().ordinal(), 91);
    assert_eq!(
        bridge.retirement_observe_destroyed(viewport, binding, window, 91),
        DestroyedObservation::Recorded
    );
    assert!(bridge.acknowledge_event(91));
    bridge.commit_frame_boundary(true);
    assert_eq!(bridge.lock().retained_record_count(), 1);
    assert!(bridge.retirement_destroyed_observations().is_empty());
    bridge.commit_frame_boundary(false);
    assert_eq!(bridge.lock().retained_record_count(), 0);
    assert_eq!(
        bridge.retirement_destroyed_observations(),
        [(binding, NativeWindowFacts::destroyed())]
    );
}

#[test]
fn destroyed_owner_preserves_raw_order_capture_age_and_full_commit_boundary() {
    let binding = binding();
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(11);
    let mut routes = NativeViewportMap::default();
    routes.bind(viewport, window, binding).unwrap();
    let (bridge, now) = bridge_with_clock(routes);

    bridge.push_record(cursor(101, window, binding));
    now.store(10, Ordering::SeqCst);
    bridge.push_record(destroyed(202, window));
    {
        let records = bridge.lock();
        assert_eq!(records.journal.len(), 1);
        assert_eq!(records.viewports.window_ingress().count(), 1);
        assert_eq!(records.retirements.window_ingress().count(), 0);
        assert!(!records.retirements.has_pending_work());
    }
    now.store(20, Ordering::SeqCst);
    bridge.push_record(cursor(303, window, binding));
    bridge.push_record(destroyed(202, window));
    bridge.push_record(destroyed(203, window));
    now.store(50, Ordering::SeqCst);
    {
        let records = bridge.lock();
        assert_eq!(records.journal.len(), 2);
        assert_eq!(records.journal.captured_through(), 3);
        assert_eq!(records.retained_record_count(), 3);
        assert_eq!(records.oldest_record_age(), Some(Duration::from_millis(50)));
        assert_eq!(
            records
                .ordered_ingress()
                .map(ReservedHostRecord::ordinal)
                .collect::<Vec<_>>(),
            [1, 2, 3],
        );
    }
    assert_eq!(bridge.front_event().unwrap().ordinal(), 101);
    assert!(!bridge.acknowledge_event(202));
    {
        let claim = bridge.claim_cursor_prefix(256).unwrap();
        assert_eq!(claim.record_count().get(), 1);
    }
    assert_eq!(bridge.lock().retained_record_count(), 3);
    assert!(bridge.acknowledge_event(101));
    bridge.commit_frame_boundary(true);
    assert_eq!(bridge.lock().retained_record_count(), 3);
    bridge.commit_frame_boundary(false);
    assert_eq!(bridge.front_event().unwrap().ordinal(), 202);
    assert_eq!(bridge.front_event().unwrap().binding(), Some(binding));
    assert!(!bridge.acknowledge_event(202));
    assert!(!bridge.retirement_has_pending_work());

    let original = {
        let records = bridge.lock();
        let ingress = records.viewports.window_ingress().next().unwrap();
        (ingress.ordinal(), ingress.captured_at())
    };
    assert_eq!(original, (2, Duration::from_millis(10)));
    assert_eq!(
        bridge.retirement_observe_destroyed(viewport, binding, window, 202),
        DestroyedObservation::Recorded,
    );
    {
        let records = bridge.lock();
        assert_eq!(records.viewports.window_ingress().count(), 0);
        let moved = records.retirements.window_ingress().next().unwrap();
        assert_eq!((moved.ordinal(), moved.captured_at()), original);
        assert_eq!(records.front_ingress().unwrap().ordinal(), 2);
        assert_eq!(records.journal.len(), 1);
        assert_eq!(records.retained_record_count(), 2);
        assert_eq!(records.oldest_record_age(), Some(Duration::from_millis(40)));
    }
    assert_eq!(
        bridge.retirement_observe_destroyed(viewport, binding, window, 202),
        DestroyedObservation::Duplicate,
    );
    assert!(bridge.acknowledge_event(202));
    assert!(!bridge.acknowledge_event(202));
    now.store(100, Ordering::SeqCst);
    bridge.push_record(destroyed(204, window));
    {
        // No full commit follows handoff yet, as with a dropped candidate frame.
        let records = bridge.lock();
        assert_eq!(records.journal.captured_through(), 3);
        assert_eq!(records.retirements.window_ingress().count(), 0);
        assert_eq!(records.retained_record_count(), 2);
        assert_eq!(records.oldest_record_age(), Some(Duration::from_millis(90)));
    }
    bridge.commit_frame_boundary(true);
    assert!(bridge.retirement_destroyed_observations().is_empty());
    assert_eq!(bridge.lock().retained_record_count(), 2);
    bridge.commit_frame_boundary(false);
    assert_eq!(
        bridge.retirement_destroyed_observations(),
        [(binding, NativeWindowFacts::destroyed())],
    );
    assert_eq!(bridge.lock().retained_record_count(), 1);
    assert_eq!(
        bridge.lock().oldest_record_age(),
        Some(Duration::from_millis(80))
    );
    assert_eq!(bridge.front_event().unwrap().ordinal(), 303);
    assert!(bridge.acknowledge_event(303));
    bridge.commit_frame_boundary(false);
    assert_eq!(bridge.lock().retained_record_count(), 0);
    assert_eq!(bridge.lock().oldest_record_age(), None);
}

#[test]
fn retired_window_terminal_cannot_replace_or_release_the_active_successor() {
    let first = binding();
    let next = binding();
    let viewport = ViewportId::ROOT;
    let old_window = WindowId::from(21);
    let new_window = WindowId::from(22);
    let (bridge, now) = bridge_with_clock(NativeViewportMap::default());
    bridge
        .bind_viewport_route(viewport, old_window, first)
        .unwrap();
    assert!(bridge.with_viewports_mut(|routes| routes.suppress_pointer_route(viewport, first)));
    bridge
        .reserve_retired_viewport_replacement(viewport, first, next)
        .unwrap();
    bridge
        .with_viewports_mut(|routes| routes.attach_output(viewport, next, new_window, None))
        .unwrap();
    now.store(10, Ordering::SeqCst);
    bridge.push_record(destroyed(401, old_window));
    bridge.push_record(destroyed(402, old_window));
    {
        let records = bridge.lock();
        assert!(records.journal.is_empty());
        assert_eq!(records.journal.captured_through(), 1);
        assert_eq!(records.viewports.window_ingress().count(), 0);
        let ingress = records.retirements.window_ingress().next().unwrap();
        assert_eq!(ingress.ordinal(), 1);
        assert_eq!(ingress.captured_at(), Duration::from_millis(10));
    }
    assert_eq!(bridge.front_event().unwrap().binding(), Some(first));
    assert_eq!(bridge.front_event().unwrap().ordinal(), 401);
    assert_eq!(
        bridge.retirement_observe_destroyed(viewport, first, old_window, 401),
        DestroyedObservation::Recorded,
    );
    assert!(bridge.acknowledge_event(401));
    bridge.commit_frame_boundary(true);
    assert!(bridge.retirement_quiescence_candidates().is_empty());
    assert_eq!(bridge.lock().retained_record_count(), 1);
    bridge.commit_frame_boundary(false);
    assert_eq!(bridge.lock().retained_record_count(), 0);

    bridge.retirement_mark_snapshot_queued();
    bridge.retirement_settle_snapshot(true);
    let committed = bridge.retirement_committed_routes();
    assert_eq!(committed.len(), 1);
    assert_eq!(committed[0].binding(), first);
    assert!(
        bridge
            .prepare_route_retirements(&committed)
            .unwrap()
            .commit()
            .is_empty()
    );
    bridge.retirement_commit_routes(&committed);
    assert!(bridge.retirement_finish_quiescence(first));
    assert!(!bridge.retirement_finish_quiescence(first));
    bridge.push_record(destroyed(403, old_window));
    assert_eq!(bridge.lock().journal.captured_through(), 1);
    assert_eq!(bridge.lock().retained_record_count(), 0);
    assert!(!bridge.retirement_has_pending_work());
    bridge.with_viewports(|routes| {
        assert_eq!(routes.binding(viewport), Some(next));
        assert_eq!(routes.window(viewport), Some(new_window));
        assert_eq!(
            routes.pointer_binding_for_window(new_window),
            Some(next.window())
        );
        assert!(routes.has_window_receipt(viewport, next));
    });
}

#[test]
fn create_failure_keeps_one_inline_capture_through_transfer_and_partial_commit() {
    let binding = binding();
    let viewport = ViewportId::ROOT;
    for result_recorded in [false, true] {
        let mut routes = NativeViewportMap::default();
        routes.reserve(viewport, binding).unwrap();
        assert!(routes.admit_test_create(viewport, binding));
        let (bridge, now) = bridge_with_clock(routes);
        let failure = NativeViewportCreateFailureRecord::new(
            viewport,
            binding,
            NativeViewportCreateFailureKind::WindowUnavailable,
        );
        now.store(10, Ordering::SeqCst);
        {
            let mut records = bridge.lock();
            assert!(records.record_viewport_create_failure(failure));
            assert!(!records.record_viewport_create_failure(failure));
            assert!(records.journal.is_empty());
            assert_eq!(records.journal.captured_through(), 1);
            assert_eq!(records.retained_record_count(), 1);
            let ingress = records.viewports.create_ingress().next().unwrap();
            assert_eq!(ingress.ordinal(), 1);
            assert_eq!(ingress.captured_at(), Duration::from_millis(10));
            assert!(
                matches!(ingress.record(), HostRecord::ViewportCreateFailed(record) if *record == failure)
            );
            assert_eq!(records.retirements.create_ingress().count(), 0);
        }
        bridge.commit_frame_boundary(false);
        now.store(50, Ordering::SeqCst);
        assert_eq!(bridge.front_viewport_create_failure(), Some(failure));
        assert_eq!(bridge.lock().retained_record_count(), 1);
        assert_eq!(
            bridge.lock().oldest_record_age(),
            Some(Duration::from_millis(40))
        );
        assert!(bridge.acknowledge_viewport_create_failure(failure, result_recorded));
        assert!(!bridge.acknowledge_viewport_create_failure(failure, result_recorded));
        assert_eq!(
            bridge.with_viewports(|routes| routes.binding(viewport)),
            None
        );
        {
            let mut records = bridge.lock();
            assert!(!records.record_viewport_create_failure(failure));
            assert_eq!(records.journal.captured_through(), 1);
            assert!(records.journal.is_empty());
            assert_eq!(records.viewports.create_ingress().count(), 0);
            assert_eq!(records.retirements.create_ingress().count(), 0);
            assert_eq!(records.retained_record_count(), 1);
            assert_eq!(
                records.journal.oldest_age(),
                Some(Duration::from_millis(40))
            );
        }
        now.store(100, Ordering::SeqCst);
        bridge.commit_frame_boundary(true);
        assert_eq!(bridge.lock().retained_record_count(), 1);
        assert_eq!(
            bridge.lock().oldest_record_age(),
            Some(Duration::from_millis(90))
        );
        assert!(bridge.retirement_quiescence_candidates().is_empty());
        assert!(!bridge.retirement_finish_quiescence(binding));
        bridge.commit_frame_boundary(false);
        assert_eq!(bridge.lock().retained_record_count(), 0);
        assert_eq!(bridge.lock().oldest_record_age(), None);
        if !result_recorded {
            assert!(bridge.retirement_quiescence_candidates().is_empty());
            assert!(!bridge.retirement_finish_quiescence(binding));
            bridge.retirement_mark_create_failure_recorded(binding);
            bridge.commit_frame_boundary(true);
            assert!(bridge.retirement_quiescence_candidates().is_empty());
            bridge.commit_frame_boundary(false);
        }
        assert_eq!(bridge.retirement_quiescence_candidates(), [binding]);
        assert!(bridge.retirement_finish_quiescence(binding));
        assert!(!bridge.retirement_finish_quiescence(binding));
        assert!(!bridge.lock().record_viewport_create_failure(failure));
        assert_eq!(bridge.lock().journal.captured_through(), 1);
        assert!(!bridge.retirement_has_pending_work());
    }
}

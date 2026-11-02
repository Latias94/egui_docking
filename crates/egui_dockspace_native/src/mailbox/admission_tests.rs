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
        .enable_managed_native_host(
            dockspace::runtime::NativeCreateStrategy::Staged,
            NativePointerRoster::Exact(Vec::new()),
        )
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
fn selected_window_limit_rejects_whole_synthetic_rosters_before_minting() {
    let (bridge, _, binding, _) = bridge_with_clock();
    let snapshot = crate::coordinator::NativeSnapshotStorageLimit::required_bytes(1, 1).unwrap();
    assert!(bridge.configure_snapshot_storage_allowance(snapshot, 1, 1));
    let observations = [crate::window_snapshot::CompiledWindowObservation::new(
        binding,
        dockspace::runtime::NativeWindowFacts::live(),
    ); 2];
    let mut state = bridge.lock();
    let ordinal = state.journal.captured_through();
    let retained = state.retained_record_count();
    // Repeated bindings are a deliberately malformed private fixture. The
    // count guard must run before either capture identity or validation.
    assert!(
        !state.record_viewport_roster(NativeViewportRosterRecord::for_test(observations, true))
    );
    assert_eq!(state.journal.captured_through(), ordinal);
    assert_eq!(state.retained_record_count(), retained);
    state.terminal_roster.require();
    assert!(
        !state.record_terminal_viewport_roster(NativeViewportRosterRecord::for_test(
            observations,
            true
        ))
    );
    assert!(state.terminal_roster.needs_capture());
    assert!(state.terminal_roster.ingress().is_none());
    assert_eq!(state.journal.captured_through(), ordinal);
    assert!(
        state.record_terminal_viewport_roster(NativeViewportRosterRecord::for_test(
            [observations[0]],
            true
        ))
    );
    assert_eq!(state.journal.captured_through(), ordinal + 1);
}

fn three_bindings() -> [NativeSurfaceBinding; 3] {
    let layout = DockspaceLayout::new((1..=3).map(|id| {
        DockspaceSurfaceLayout::new(
            SurfaceId::new(id),
            DockspaceRootLayout::new(
                RootId::new(id),
                DockspaceNode::central_tabs([ItemId::new(id)]),
            ),
        )
    }))
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session
        .enable_managed_native_host(
            dockspace::runtime::NativeCreateStrategy::Staged,
            NativePointerRoster::Exact(Vec::new()),
        )
        .unwrap();
    for id in 1..=3 {
        session
            .register_native_root(SurfaceId::new(id), HostWindowToken::new(id))
            .unwrap();
    }
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame
        .commit()
        .unwrap()
        .inputs()
        .iter()
        .filter_map(|input| match input {
            HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
            _ => None,
        })
        .collect::<Vec<_>>()
        .try_into()
        .unwrap()
}

#[test]
fn new_window_lifetimes_require_complete_roster_capacity_before_route_or_create_admission() {
    let [first, second, third] = three_bindings();
    let bridge = NativeHostBridge::new(NativeViewportMap::default());
    bridge.lock().terminal_roster = TerminalRosterState::with_capacity(2, 1);
    let viewport = |id| ViewportId::from_hash_of(id);
    bridge
        .bind_viewport_route(viewport(1), WindowId::from(11), first)
        .unwrap();
    bridge.reserve_viewport_route(viewport(2), second).unwrap();
    // The route and create sidecar share one lifetime's existing commitment.
    assert!(bridge.reserve_create(viewport(2), second, NativePhysicalRect::new(0, 0, 320, 240)));
    assert!(matches!(
        bridge.reserve_viewport_route(viewport(3), third),
        Err(NativeViewportBindingError::CapacityExceeded { limit: 2 }),
    ));
    assert!(!bridge.reserve_create(viewport(3), third, NativePhysicalRect::new(0, 0, 320, 240)));
    assert!(
        bridge
            .with_viewports(|routes| routes.binding(viewport(3)))
            .is_none()
    );
    assert!(bridge.lock().window_resources.create(viewport(3)).is_none());

    // Moving the first window to retirement does not return its commitment.
    bridge.unbind_viewport_route(viewport(1), first).unwrap();
    assert_eq!(bridge.lock().viewports.bindings().count(), 1);
    assert_eq!(bridge.lock().retirements.bindings().count(), 1);
    assert!(matches!(
        bridge.bind_viewport_route(viewport(3), WindowId::from(33), third),
        Err(NativeViewportBindingError::CapacityExceeded { limit: 2 }),
    ));

    assert!(bridge.clear_create(viewport(2), second));
    assert!(bridge.clear_hidden_render(viewport(2), second));
    bridge.unbind_viewport_route(viewport(2), second).unwrap();
    bridge.reserve_viewport_route(viewport(3), third).unwrap();
    assert_eq!(
        bridge.lock().viewports.bindings().collect::<Vec<_>>(),
        [third]
    );
    assert_eq!(
        bridge.lock().retirements.bindings().collect::<Vec<_>>(),
        [first]
    );
}

#[test]
fn admitted_window_retains_prepaid_retirement_backing_through_terminal_retry() {
    let [first, second, _] = three_bindings();
    let bridge = Arc::new(NativeHostBridge::new(NativeViewportMap::default()));
    bridge.configure_lifecycle_storage(1, 1).unwrap();
    bridge.lock().terminal_roster = TerminalRosterState::with_capacity(1, 1);
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(11);
    let backing = bridge.lock().retirements.owned_data_capacity_bytes();
    bridge.bind_viewport_route(viewport, window, first).unwrap();
    assert_eq!(
        bridge.lock().retirements.owned_data_capacity_bytes(),
        backing
    );
    assert!(matches!(
        bridge.reserve_viewport_route(ViewportId::from_hash_of(2), second),
        Err(NativeViewportBindingError::CapacityExceeded { limit: 1 }),
    ));
    bridge.unbind_viewport_route(viewport, first).unwrap();
    assert_eq!(
        bridge.lock().retirements.owned_data_capacity_bytes(),
        backing
    );
    let captured = bridge.lock().journal.captured_through();
    bridge.push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
        41,
        window,
        Some(viewport),
        Some(first),
        WindowEvent::Destroyed,
    )));
    assert_eq!(bridge.lock().journal.captured_through(), captured + 1);
    assert_eq!(
        bridge.retirement_observe_destroyed(viewport, first, window, 41),
        DestroyedObservation::Recorded,
    );
    assert!(bridge.acknowledge_event(41));
    bridge.commit_frame_boundary(true);
    assert!(!bridge.retirement_requires_snapshot());
    assert_eq!(
        bridge.lock().retirements.owned_data_capacity_bytes(),
        backing
    );
    bridge.commit_frame_boundary(false);
    assert!(bridge.retirement_requires_snapshot());
    bridge.retirement_mark_snapshot_queued();
    bridge.retirement_settle_snapshot(false);
    assert!(bridge.retirement_requires_snapshot());
    bridge.retirement_mark_snapshot_queued();
    bridge.retirement_settle_snapshot(true);
    let prepared = bridge
        .prepare_committed_route_retirements()
        .unwrap()
        .unwrap();
    drop(prepared);
    assert_eq!(bridge.next_retirement_quiescence_candidate(None), None);
    let prepared = bridge
        .prepare_committed_route_retirements()
        .unwrap()
        .unwrap();
    let (routes, abandoned) = prepared.commit_with_routes();
    assert!(abandoned.is_empty());
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].binding(), first);
    bridge.retirement_commit_routes(&routes);
    assert_eq!(
        bridge.next_retirement_quiescence_candidate(None),
        Some(first)
    );
    assert_eq!(
        bridge.next_retirement_quiescence_candidate(Some(first)),
        None
    );
    assert!(bridge.retirement_finish_quiescence(first));
    assert_eq!(
        bridge.lock().retirements.owned_data_capacity_bytes(),
        backing
    );
    bridge.reserve_viewport_route(viewport, second).unwrap();
    assert_eq!(
        bridge.lock().retirements.owned_data_capacity_bytes(),
        backing
    );
}

#[test]
fn full_close_owner_refuses_new_capture_before_journal_identity_and_keeps_duplicates() {
    let [first, second, _] = three_bindings();
    let bridge = NativeHostBridge::new(NativeViewportMap::default());
    bridge.configure_lifecycle_storage(2, 1).unwrap();
    let first_viewport = ViewportId::ROOT;
    let second_viewport = ViewportId::from_hash_of(2);
    bridge
        .bind_viewport_route(first_viewport, WindowId::from(11), first)
        .unwrap();
    bridge
        .bind_viewport_route(second_viewport, WindowId::from(22), second)
        .unwrap();
    let close = |binding, viewport, window, ordinal| {
        NativeWindowEventRecord::for_test(
            ordinal,
            window,
            Some(viewport),
            Some(binding),
            WindowEvent::CloseRequested,
        )
    };
    let mut state = bridge.lock();
    let backing = state.close_control.owned_data_capacity_bytes();
    assert!(state.capture_window_event(close(first, first_viewport, WindowId::from(11), 1)));
    let captured = state.journal.captured_through();
    let progress = state.close_control.progress();
    assert!(!state.capture_window_event(close(second, second_viewport, WindowId::from(22), 2)));
    assert_eq!(state.journal.captured_through(), captured);
    assert_eq!(state.close_control.progress(), progress);
    assert!(!state.close_control.references_binding(second));
    assert!(state.capture_window_event(close(first, first_viewport, WindowId::from(11), 1)));
    assert_eq!(state.journal.captured_through(), captured);
    assert_eq!(state.close_control.progress(), progress);
    assert_eq!(state.close_control.owned_data_capacity_bytes(), backing);
}

#[test]
fn quarantine_cannot_grow_raw_storage_or_capture_a_new_close_owner() {
    let (bridge, _, binding, window) = bridge_with_clock();
    for ordinal in 1..=admission::RECORD_LIMIT as u64 {
        bridge.push_record(cursor(ordinal, window, binding));
    }
    let mut records = bridge.lock();
    let bytes = records.journal.owned_buffer_capacity_bytes();
    let captured = records.journal.captured_through();
    records.quarantine_after_fatal();
    for ordinal in 10_000..10_100 {
        assert!(!records.record_device_removal(NativeDeviceRemovalRecord::for_test(ordinal)));
        assert!(
            !records.capture_window_event(NativeWindowEventRecord::for_test(
                ordinal,
                window,
                Some(ViewportId::ROOT),
                Some(binding),
                WindowEvent::CloseRequested,
            ))
        );
        assert!(
            !records.capture_window_event(NativeWindowEventRecord::for_test(
                ordinal,
                WindowId::from(99),
                Some(ViewportId::from_hash_of("unmapped close")),
                None,
                WindowEvent::CloseRequested,
            ))
        );
    }
    assert_eq!(records.journal.len(), admission::RECORD_LIMIT);
    assert_eq!(records.journal.captured_through(), captured);
    assert_eq!(records.journal.owned_buffer_capacity_bytes(), bytes);
    assert_eq!(
        records.admission,
        NativeAdmission::Draining { boundary: captured },
        "fatal mode must not bypass the full source boundary cancellation",
    );
    assert!(!records.close_control.has_pending_work());
    assert_eq!(records.close_control.progress().requests_retained(), 0);
}

#[test]
fn quarantine_device_terminal_uses_capacity_and_freezes_at_the_original_age_boundary() {
    let (bridge, now, binding, window) = bridge_with_clock();
    bridge.push_record(cursor(1, window, binding));
    bridge.lock().quarantine_after_fatal();
    now.store(249, Ordering::SeqCst);
    assert!(
        bridge
            .lock()
            .record_device_removal(NativeDeviceRemovalRecord::for_test(2))
    );
    assert_eq!(bridge.lock().journal.captured_through(), 2);
    now.store(250, Ordering::SeqCst);
    assert!(
        !bridge
            .lock()
            .record_device_removal(NativeDeviceRemovalRecord::for_test(3))
    );
    let records = bridge.lock();
    assert_eq!(records.journal.captured_through(), 2);
    assert_eq!(records.journal.len(), 2);
    assert_eq!(records.admission, NativeAdmission::Draining { boundary: 2 });
    assert!(matches!(records.mode, HostIngressMode::Quarantined));
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

#[test]
fn accepted_lifecycle_show_reserves_its_terminal_while_ordinary_ingress_drains() {
    let (bridge, _, binding, window) = bridge_with_clock();
    let context = eframe::egui::Context::default();
    let token = eframe::queue_native_viewport_visibility(&context, ViewportId::ROOT, true);
    bridge.configure_window_resource_storage(1, 0).unwrap();
    bridge
        .bind_viewport_route(ViewportId::ROOT, window, binding)
        .unwrap();
    let capacity = bridge.lock().window_resources.owned_data_capacity_bytes();
    assert_eq!(bridge.lock().window_resources.bindings().count(), 1);
    let boundary = bridge.freeze_ordinary_for_test();
    let before = bridge.lock().admission;
    assert!(bridge.reserve_visibility_callback(ViewportId::ROOT, binding, window, token, true));
    assert_eq!(bridge.lock().admission, before);
    assert!(!bridge.permits_unscheduled_input());
    let record = NativeViewportVisibilityRecord::for_test(
        token,
        ViewportId::ROOT,
        window,
        binding,
        true,
        eframe::NativeViewportVisibilityStatus::Dispatched,
    );
    assert!(bridge.lock().record_viewport_visibility(record));
    assert!(!bridge.lock().record_viewport_visibility(record));
    assert_eq!(bridge.lock().journal.captured_through(), boundary + 1);
    assert_eq!(bridge.lock().window_resources.callback_ingress().count(), 1);
    assert_eq!(
        bridge.lock().window_resources.owned_data_capacity_bytes(),
        capacity
    );
    assert_eq!(bridge.lock().window_resources.bindings().count(), 1);
    assert_eq!(bridge.lock().journal.len(), 0);
    assert!(bridge.reserve_visibility_callback(ViewportId::ROOT, binding, window, token, true));
    assert!(!bridge.reserve_visibility_callback(
        ViewportId::ROOT,
        binding,
        WindowId::from(999),
        token,
        true,
    ));
    bridge.lock().freeze();
    assert!(!bridge.reserve_visibility_callback(ViewportId::ROOT, binding, window, token, true));
}

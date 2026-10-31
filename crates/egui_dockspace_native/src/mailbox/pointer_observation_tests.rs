//! Controlled backend models for mailbox receipts, without OS/window or output authority.
//! Command tokens and core bindings are real; no result or continuation is fabricated.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    HostInputOutcome, HostWindowToken, NativeCreateStrategy, NativePointerRoster,
    SurfaceUnavailableReason,
};
use eframe::{NativeViewportPointerObservation, NativeViewportPropertyCommandRetirement};
use winit::dpi::PhysicalPosition;
use winit::event::{DeviceId, PointerEventFacts, PointerWindowRoute};

use super::*;

fn registered_session() -> (DockspaceSession, [NativeSurfaceBinding; 3]) {
    let layout = DockspaceLayout::new([1, 2, 3].map(|id| {
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
            NativeCreateStrategy::Staged,
            NativePointerRoster::Exact(Vec::new()),
        )
        .unwrap();
    for id in [1, 2, 3] {
        session
            .register_native_root(SurfaceId::new(id), HostWindowToken::new(id))
            .unwrap();
    }
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    let bindings = report
        .inputs()
        .iter()
        .filter_map(|input| match input {
            HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
            _ => None,
        })
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    (session, bindings)
}

fn bridge_with_clock() -> (NativeHostBridge, Arc<AtomicU64>) {
    let now = Arc::new(AtomicU64::new(0));
    let source = Arc::clone(&now);
    let bridge = NativeHostBridge::with_clock(
        NativeViewportMap::default(),
        NativeMonotonicClock::new(move || Duration::from_millis(source.load(Ordering::SeqCst))),
    );
    (bridge, now)
}

fn key(viewport: ViewportId, window: WindowId) -> NativePointerObservationKey {
    let device = DeviceId::dummy();
    let prepared = eframe::prepare_native_viewport_pointer_observation(viewport, window, device);
    NativePointerObservationKey {
        token: prepared.token(),
        viewport,
        window,
        device,
    }
}

fn observed(window: WindowId) -> NativeViewportPointerObservation {
    NativeViewportPointerObservation::Observed {
        facts: PointerEventFacts {
            desktop_position: Some(PhysicalPosition::new(-200.75, 300.5)),
            hover: PointerWindowRoute::Window(window),
            capture: PointerWindowRoute::Unknown,
            ..Default::default()
        },
        delivery: PointerWindowRoute::Unknown,
    }
}

fn record(
    bridge: &NativeHostBridge,
    key: NativePointerObservationKey,
    binding: NativeSurfaceBinding,
    observation: NativeViewportPointerObservation,
) -> NativePointerObservationRecord {
    NativePointerObservationRecord::for_test(key, binding, observation, &bridge.lock().viewports)
}

fn receive(
    bridge: &NativeHostBridge,
    record: NativePointerObservationRecord,
) -> CommandCallbackAdmission {
    let mut records = bridge.lock();
    let NativeHostState {
        window_resources,
        journal,
        ..
    } = &mut *records;
    window_resources.receive_observation_record(record, |record| journal.reserve(record))
}

fn retirement(key: NativePointerObservationKey) -> NativeViewportPropertyCommandRetirement {
    NativeViewportPropertyCommandRetirement::PointerObservation {
        viewport_id: key.viewport,
        token: key.token,
        expected_window_id: key.window,
        device_id: key.device,
    }
}

fn front_identity(bridge: &NativeHostBridge) -> (u64, Duration) {
    let records = bridge.lock();
    let ingress = records.front_ingress().unwrap();
    (ingress.ordinal(), ingress.captured_at())
}

#[test]
fn observation_reservation_is_session_wide_and_exactly_idempotent() {
    let (_session, [source, peer, _]) = registered_session();
    let (bridge, _) = bridge_with_clock();
    let viewport = ViewportId::ROOT;
    let peer_viewport = ViewportId::from_hash_of("observation-peer");
    let window = WindowId::from(11);
    let peer_window = WindowId::from(12);
    bridge
        .bind_viewport_route(viewport, window, source)
        .unwrap();
    bridge
        .bind_viewport_route(peer_viewport, peer_window, peer)
        .unwrap();
    let first = key(viewport, window);
    let successor = key(viewport, window);
    let other = key(peer_viewport, peer_window);

    assert!(!bridge.reserve_pointer_observation(first, source));
    bridge.enable_observation_attachment_for_test();
    let raw = NativeDeviceRemovalRecord::for_test(71);
    bridge.push_record(HostRecord::DeviceRemoval(raw));
    assert!(!bridge.reserve_pointer_observation(first, source));
    assert!(bridge.acknowledge_device_removal(raw));
    assert!(!bridge.reserve_pointer_observation(first, source));
    bridge.commit_frame_boundary(false);

    assert!(bridge.reserve_pointer_observation(first, source));
    assert!(bridge.reserve_pointer_observation(first, source));
    assert!(!bridge.reserve_pointer_observation(successor, source));
    assert!(!bridge.reserve_pointer_observation(other, peer));
    assert!(!bridge.reserve_pointer_observation(first, peer));

    let wrong_window = NativePointerObservationKey {
        window: peer_window,
        ..first
    };
    let wrong_viewport = NativePointerObservationKey {
        viewport: peer_viewport,
        ..first
    };
    // Winit exposes no portable safe constructor for a second distinct DeviceId.
    for wrong in [wrong_window, wrong_viewport] {
        assert!(!bridge.reserve_pointer_observation(wrong, source));
        assert!(
            !bridge
                .lock()
                .window_resources
                .reserve_observation(wrong, source)
        );
        assert!(!bridge.pointer_observation_pending(wrong));
    }
    let mut records = bridge.lock();
    assert!(!records.window_resources.reserve_observation(first, peer));
    assert!(!records.window_resources.reserve_observation(other, peer));
    assert!(records.window_resources.observation_pending(first));
    assert_eq!(records.window_resources.observation_record(first), None);
    assert_eq!(records.window_resources.callback_ingress().count(), 0);
    assert_eq!(records.retained_record_count(), 0);
    assert_eq!(records.journal.captured_through(), 1);
}

#[test]
fn observation_first_terminal_survives_retry_until_full_commit() {
    let window = WindowId::from(21);
    for observation in [
        Some(observed(window)),
        Some(NativeViewportPointerObservation::Unavailable),
        None,
    ] {
        let (mut session, [source, peer, _]) = registered_session();
        let (bridge, now) = bridge_with_clock();
        let viewport = ViewportId::ROOT;
        let peer_viewport = ViewportId::from_hash_of("observation-retry-peer");
        let peer_window = WindowId::from(22);
        bridge
            .bind_viewport_route(viewport, window, source)
            .unwrap();
        bridge
            .bind_viewport_route(peer_viewport, peer_window, peer)
            .unwrap();
        bridge.enable_observation_attachment_for_test();
        let first = key(viewport, window);
        let successor = key(viewport, window);
        let other = key(peer_viewport, peer_window);
        assert!(bridge.reserve_pointer_observation(first, source));
        let wrong_window = NativePointerObservationKey {
            window: peer_window,
            ..first
        };
        let wrong_viewport = NativePointerObservationKey {
            viewport: peer_viewport,
            ..first
        };
        for (wrong, binding) in [
            (successor, source),
            (wrong_window, source),
            (wrong_viewport, source),
            (first, peer),
        ] {
            assert_eq!(
                receive(&bridge, record(&bridge, wrong, binding, observed(window))),
                CommandCallbackAdmission::Unreserved,
            );
        }
        for wrong in [successor, wrong_window, wrong_viewport] {
            assert_eq!(
                NativeHostHandler::on_viewport_property_command_retired(&bridge, retirement(wrong)),
                NativeHostWake::Wait,
            );
        }
        assert_eq!(bridge.lock().journal.captured_through(), 0);
        now.store(17, Ordering::SeqCst);
        let callback = record(&bridge, first, source, observed(window));
        let expected = match observation {
            Some(observation) => {
                let expected = record(&bridge, first, source, observation);
                assert_eq!(receive(&bridge, expected), CommandCallbackAdmission::First);
                expected
            }
            None => {
                assert_eq!(
                    NativeHostHandler::on_viewport_property_command_retired(
                        &bridge,
                        retirement(first),
                    ),
                    NativeHostWake::RepaintRootAfterCurrent,
                );
                NativePointerObservationRecord::retired(first, source)
            }
        };
        let identity = front_identity(&bridge);
        assert_eq!(identity, (1, Duration::from_millis(17)));
        assert_eq!(
            bridge.front_pointer_observation(),
            Some((expected, identity.1)),
        );
        for duplicate in [expected, callback] {
            assert_eq!(
                receive(&bridge, duplicate),
                CommandCallbackAdmission::Duplicate
            );
        }
        assert_eq!(
            NativeHostHandler::on_viewport_property_command_retired(&bridge, retirement(first)),
            NativeHostWake::Wait,
        );
        {
            let mut records = bridge.lock();
            assert!(records.journal.is_empty());
            assert_eq!(records.retained_record_count(), 1);
            assert!(
                records
                    .window_resources
                    .take_callback_ingress(identity.0 + 1)
                    .is_none()
            );
            records.window_resources.commit_transferred_callbacks();
            assert!(records.window_resources.observation_pending(first));
            assert!(
                !records
                    .window_resources
                    .reserve_observation(successor, source)
            );
            assert!(!records.window_resources.reserve_observation(other, peer));
        }

        // Dropping a core candidate before acknowledgement leaves the raw receipt retryable.
        drop(session.begin_host_frame().unwrap());
        now.store(60, Ordering::SeqCst);
        bridge.commit_frame_boundary(true);
        assert_eq!(front_identity(&bridge), identity);
        assert_eq!(
            bridge.front_pointer_observation(),
            Some((expected, identity.1)),
        );
        assert_eq!(bridge.lock().journal.captured_through(), identity.0);
        assert_eq!(
            bridge.lock().oldest_record_age(),
            Some(Duration::from_millis(43)),
        );
        assert!(!bridge.acknowledge_pointer_observation(successor));
        assert!(!bridge.acknowledge_pointer_observation(wrong_window));
        assert!(bridge.acknowledge_pointer_observation(first));
        assert!(!bridge.acknowledge_pointer_observation(first));
        assert!(bridge.front_pointer_observation().is_none());
        assert!(bridge.pointer_observation_pending(first));
        assert!(!bridge.reserve_pointer_observation(successor, source));

        now.store(90, Ordering::SeqCst);
        bridge.commit_frame_boundary(true);
        assert_eq!(
            receive(&bridge, callback),
            CommandCallbackAdmission::Duplicate
        );
        assert_eq!(
            NativeHostHandler::on_viewport_property_command_retired(&bridge, retirement(first)),
            NativeHostWake::Wait,
        );
        {
            let mut records = bridge.lock();
            assert_eq!(records.window_resources.callback_ingress().count(), 0);
            assert_eq!(
                records.window_resources.observation_record(first),
                Some(expected)
            );
            assert!(records.window_resources.observation_pending(first));
            assert!(
                !records
                    .window_resources
                    .reserve_observation(successor, source)
            );
            assert!(!records.window_resources.reserve_observation(other, peer));
            assert_eq!(records.retained_record_count(), 1);
            assert_eq!(records.oldest_record_age(), Some(Duration::from_millis(73)));
            assert_eq!(
                records.journal.oldest_age(),
                Some(Duration::from_millis(73))
            );
            assert_eq!(records.journal.captured_through(), identity.0);
        }

        bridge.commit_frame_boundary(false);
        assert!(!bridge.pointer_observation_pending(first));
        assert_eq!(
            bridge.lock().window_resources.observation_record(first),
            None
        );
        assert_eq!(bridge.lock().retained_record_count(), 0);
        assert_eq!(bridge.lock().oldest_record_age(), None);
        assert_eq!(
            receive(&bridge, expected),
            CommandCallbackAdmission::Duplicate
        );
        assert_eq!(
            NativeHostHandler::on_viewport_property_command_retired(&bridge, retirement(first)),
            NativeHostWake::Wait,
        );
        assert!(bridge.reserve_pointer_observation(first, source));
        assert!(!bridge.pointer_observation_pending(first));
        assert!(bridge.reserve_pointer_observation(successor, source));
        assert!(bridge.pointer_observation_pending(successor));
        assert!(!bridge.reserve_pointer_observation(other, peer));
        assert_eq!(bridge.lock().journal.captured_through(), identity.0);
    }
}

#[test]
fn retired_observation_keeps_exact_owner_and_frozen_routes() {
    #[derive(Clone, Copy)]
    enum Terminal {
        Callback,
        Retirement,
        Detach,
    }

    for terminal in [Terminal::Callback, Terminal::Retirement, Terminal::Detach] {
        let (_session, [source, target, _]) = registered_session();
        let (_successor_session, [successor, _, _]) = registered_session();
        // Rebinding one window requires distinct real bindings for the same surface.
        assert_eq!(source.surface(), successor.surface());
        assert_ne!(source, successor);
        let (bridge, now) = bridge_with_clock();
        let viewport = ViewportId::ROOT;
        let window = WindowId::from(31);
        let late_viewport = ViewportId::from_hash_of("observation-late-route");
        let late_window = WindowId::from(32);
        bridge
            .bind_viewport_route(viewport, window, source)
            .unwrap();
        bridge.enable_observation_attachment_for_test();
        let first = key(viewport, window);
        let next = key(viewport, window);
        assert!(bridge.reserve_pointer_observation(first, source));
        let observation = NativeViewportPointerObservation::Observed {
            facts: PointerEventFacts {
                hover: PointerWindowRoute::Window(late_window),
                capture: PointerWindowRoute::Window(window),
                ..Default::default()
            },
            delivery: PointerWindowRoute::Unknown,
        };
        let frozen = record(&bridge, first, source, observation);
        now.store(10, Ordering::SeqCst);
        let expected = match terminal {
            Terminal::Callback => {
                assert!(bridge.record_pointer_observation_for_test(frozen));
                frozen
            }
            Terminal::Retirement | Terminal::Detach => {
                NativePointerObservationRecord::retired(first, source)
            }
        };
        bridge
            .replace_viewport_route(
                viewport,
                source,
                successor,
                NativeViewportReplacement::Attach(window),
            )
            .unwrap();
        bridge
            .bind_viewport_route(late_viewport, late_window, target)
            .unwrap();
        let rect = NativePhysicalRect::new(0, 0, 640, 480);
        assert!(bridge.reserve_create(viewport, successor, rect));
        assert!(!bridge.reserve_pointer_observation(next, successor));

        if matches!(terminal, Terminal::Retirement) {
            assert_eq!(
                NativeHostHandler::on_viewport_property_command_retired(&bridge, retirement(first)),
                NativeHostWake::RepaintRootAfterCurrent,
            );
        }
        {
            let mut records = bridge.lock();
            records.attachment.detach();
            let NativeHostState {
                window_resources,
                journal,
                ..
            } = &mut *records;
            window_resources.detach_observation(|record| journal.reserve(record));
            window_resources.detach_observation(|_| panic!("detach cannot recapture a terminal"));
            window_resources.retire(viewport, source);
        }
        assert_eq!(
            bridge.front_pointer_observation(),
            Some((expected, Duration::from_millis(10)))
        );
        assert_eq!(front_identity(&bridge), (1, Duration::from_millis(10)));
        assert_eq!(
            receive(&bridge, frozen),
            CommandCallbackAdmission::Duplicate
        );
        assert_eq!(
            NativeHostHandler::on_viewport_property_command_retired(&bridge, retirement(first)),
            NativeHostWake::Wait,
        );
        if matches!(terminal, Terminal::Callback) {
            let recaptured = record(&bridge, first, source, observation);
            assert!(frozen.references_binding(source));
            assert!(!frozen.references_binding(target));
            assert!(!frozen.references_binding(successor));
            assert!(recaptured.references_binding(target));
            assert!(recaptured.references_binding(successor));
            assert_ne!(frozen, recaptured);
        }
        assert!(bridge.acknowledge_pointer_observation(first));
        bridge.commit_frame_boundary(true);
        {
            let mut records = bridge.lock();
            assert!(records.window_resources.references_binding(source));
            assert!(
                !records
                    .window_resources
                    .reserve_observation(next, successor)
            );
            assert_eq!(records.viewports.binding(viewport), Some(successor));
            assert_eq!(
                records.window_resources.create(viewport).unwrap().binding,
                successor
            );
            assert_eq!(
                records.window_resources.create(viewport).unwrap().rect,
                rect
            );
            assert_eq!(
                records.window_resources.hidden_binding(viewport),
                Some(successor)
            );
            assert_eq!(records.journal.captured_through(), 1);
        }
        bridge.commit_frame_boundary(false);
        let mut records = bridge.lock();
        assert!(!records.window_resources.references_binding(source));
        assert_eq!(records.viewports.binding(viewport), Some(successor));
        assert_eq!(
            records.window_resources.create(viewport).unwrap().binding,
            successor
        );
        assert_eq!(
            records.window_resources.create(viewport).unwrap().rect,
            rect
        );
        assert_eq!(
            records.window_resources.hidden_binding(viewport),
            Some(successor)
        );
        assert!(
            records
                .window_resources
                .reserve_observation(next, successor)
        );
        assert_eq!(records.journal.captured_through(), 1);
    }
}

#[test]
fn post_overload_observation_terminal_preserves_frozen_boundary_and_cancel() {
    let (mut session, [source, _, _]) = registered_session();
    let (bridge, now) = bridge_with_clock();
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(41);
    bridge
        .bind_viewport_route(viewport, window, source)
        .unwrap();
    bridge.enable_observation_attachment_for_test();
    let first = key(viewport, window);
    assert!(bridge.reserve_pointer_observation(first, source));
    let raw = NativeDeviceRemovalRecord::for_test(81);
    bridge.push_record(HostRecord::DeviceRemoval(raw));
    let boundary = bridge.lock().journal.captured_through();
    assert_eq!(boundary, 1);

    now.store(250, Ordering::SeqCst);
    assert_eq!(
        bridge.admission_dispatch(),
        NativeDispatch::ContinueBoundary
    );
    let terminal = record(&bridge, first, source, observed(window));
    assert!(bridge.record_pointer_observation_for_test(terminal));
    assert_eq!(
        receive(&bridge, terminal),
        CommandCallbackAdmission::Duplicate
    );
    {
        let records = bridge.lock();
        assert_eq!(records.admission, NativeAdmission::Draining { boundary });
        assert_eq!(records.journal.captured_through(), boundary + 1);
        let ingress = records.window_resources.callback_ingress().next().unwrap();
        assert_eq!(
            (ingress.ordinal(), ingress.captured_at()),
            (boundary + 1, Duration::from_millis(250)),
        );
    }
    bridge.commit_frame_boundary(true);
    assert_eq!(
        bridge.admission_dispatch(),
        NativeDispatch::ContinueBoundary
    );
    bridge.commit_frame_boundary(false);
    assert_eq!(
        bridge.admission_dispatch(),
        NativeDispatch::Replay {
            through: Some(boundary)
        },
    );
    assert!(bridge.acknowledge_device_removal(raw));
    drop(session.begin_host_frame().unwrap());
    bridge.commit_frame_boundary(true);
    assert_eq!(
        bridge.admission_dispatch(),
        NativeDispatch::ContinueBoundary
    );
    assert!(bridge.pointer_observation_pending(first));
    assert!(!bridge.acknowledge_pointer_observation(first));
    bridge.commit_frame_boundary(false);

    assert_eq!(
        bridge.admission_dispatch(),
        NativeDispatch::Cancel { boundary }
    );
    assert!(bridge.front_pointer_observation().is_none());
    assert!(!bridge.acknowledge_pointer_observation(first));
    assert_eq!(bridge.lock().retained_record_count(), 1);
    assert_eq!(
        front_identity(&bridge),
        (boundary + 1, Duration::from_millis(250)),
    );
    assert!(!bridge.finish_overload_cancellation(boundary + 1));
    drop(session.begin_host_frame().unwrap());
    assert_eq!(
        bridge.admission_dispatch(),
        NativeDispatch::Cancel { boundary }
    );
    assert_eq!(
        bridge.lock().admission,
        NativeAdmission::Draining { boundary }
    );

    assert!(bridge.finish_overload_cancellation(boundary));
    assert_eq!(
        bridge.lock().admission,
        NativeAdmission::Recovering { boundary }
    );
    now.store(300, Ordering::SeqCst);
    assert_eq!(
        bridge.admission_dispatch(),
        NativeDispatch::Replay { through: None }
    );
    assert_eq!(
        bridge.front_pointer_observation(),
        Some((terminal, Duration::from_millis(250))),
    );
    assert!(bridge.acknowledge_pointer_observation(first));
    bridge.commit_frame_boundary(true);
    assert!(bridge.pointer_observation_pending(first));
    assert_eq!(
        bridge.admission_dispatch(),
        NativeDispatch::ContinueBoundary
    );
    bridge.commit_frame_boundary(false);
    assert!(!bridge.pointer_observation_pending(first));
    assert_eq!(bridge.lock().retained_record_count(), 0);
    assert_eq!(bridge.lock().journal.captured_through(), boundary + 1);
    assert_eq!(bridge.admission_dispatch(), NativeDispatch::Idle);
}

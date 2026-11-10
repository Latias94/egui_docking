use std::sync::Arc;

use winit::event::WindowEvent;

use super::tests::bindings;
use super::*;
use crate::event::NativeWindowEventRecord;
use crate::mailbox::{HostRecord, NativeHostBridge};
use crate::retirement::DestroyedObservation;

/// Obtain real committed retirement capabilities through the existing bridge.
fn retirement_fixture(
    slots: usize,
    owners: &[(ViewportId, NativeSurfaceBinding)],
) -> Arc<NativeHostBridge> {
    let mut routes = NativeViewportMap::with_capacity_limit(slots).unwrap();
    for (index, &(viewport, binding)) in owners.iter().enumerate() {
        let window = WindowId::from(81 + u64::try_from(index).unwrap());
        routes.bind(viewport, window, binding).unwrap();
    }
    let bridge = Arc::new(NativeHostBridge::new(routes));
    for (index, &(viewport, binding)) in owners.iter().enumerate() {
        let ordinal = 1 + u64::try_from(index).unwrap();
        let window = WindowId::from(80 + ordinal);
        bridge.push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            ordinal,
            window,
            Some(viewport),
            Some(binding),
            WindowEvent::Destroyed,
        )));
        assert_eq!(bridge.front_event().unwrap().binding(), Some(binding));
        assert_eq!(
            bridge.retirement_observe_destroyed(viewport, binding, window, ordinal),
            DestroyedObservation::Recorded,
        );
        assert!(bridge.acknowledge_event(ordinal));
    }
    bridge.commit_frame_boundary(false);
    bridge.retirement_mark_snapshot_queued();
    bridge.retirement_settle_snapshot(true);
    assert_eq!(bridge.retirement_committed_routes().len(), owners.len());
    bridge
}

#[test]
fn finite_routes_reject_new_keys_and_reuse_removed_slots_without_losing_backing() {
    let (first, second) = bindings();
    let first_viewport = ViewportId::ROOT;
    let second_viewport = ViewportId::from_hash_of("bounded-route-peer");
    let mut zero = NativeViewportMap::with_capacity_limit(0).unwrap();
    assert_eq!(
        zero.reserve(first_viewport, first),
        Err(NativeViewportBindingError::CapacityExceeded { limit: 0 }),
    );
    for slots in [1, 2] {
        let mut routes = NativeViewportMap::with_capacity_limit(slots).unwrap();
        let bytes = routes.owned_data_capacity_bytes();
        let route_backing = routes.viewports.as_ptr();
        let marker_backing = routes.prepared_retirements.as_ptr();
        routes.reserve(first_viewport, first).unwrap();
        routes.reserve(first_viewport, first).unwrap();
        routes
            .attach(first_viewport, first, WindowId::from(11))
            .unwrap();
        if slots == 1 {
            assert_eq!(
                routes.reserve(second_viewport, second),
                Err(NativeViewportBindingError::CapacityExceeded { limit: 1 }),
            );
            assert_eq!(
                routes.bind(second_viewport, WindowId::from(12), second),
                Err(NativeViewportBindingError::CapacityExceeded { limit: 1 }),
            );
            assert_eq!(routes.binding(second_viewport), None);
            assert_eq!(routes.window(first_viewport), Some(WindowId::from(11)));
            assert!(routes.has_window_receipt(first_viewport, first));
            drop(routes.remove_viewport(first_viewport, first).unwrap());
            routes
                .bind(second_viewport, WindowId::from(12), second)
                .unwrap();
        } else {
            routes
                .bind(second_viewport, WindowId::from(12), second)
                .unwrap();
            assert_eq!(routes.bindings().count(), 2);
            assert_eq!(routes.viewport(first.surface()), Some(first_viewport));
            assert_eq!(routes.viewport(second.surface()), Some(second_viewport));
            drop(routes.remove_viewport(first_viewport, first).unwrap());
        }
        drop(routes.remove_viewport(second_viewport, second).unwrap());
        assert!(routes.bindings().next().is_none());
        assert_eq!(routes.owned_data_capacity_bytes(), bytes);
        assert_eq!(routes.viewports.as_ptr(), route_backing);
        assert_eq!(routes.prepared_retirements.as_ptr(), marker_backing);
        assert!(!routes.can_configure_capacity_limit());
    }
}

#[test]
fn full_route_store_attaches_and_replaces_before_moving_only_the_exact_receipt() {
    let (first, other_surface) = bindings();
    let second = bindings().0;
    let third = bindings().0;
    let fourth = bindings().0;
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(21);
    let mut routes = NativeViewportMap::with_capacity_limit(1).unwrap();
    let bytes = routes.owned_data_capacity_bytes();
    let backing = routes.viewports.as_ptr();
    routes.reserve(viewport, first).unwrap();
    routes.attach(viewport, first, window).unwrap();
    routes.set_focus_reportable(&[first]);
    assert_eq!(
        routes.focus_binding_for_event(window, None),
        Some(first.window())
    );
    assert!(
        routes
            .replace(viewport, first, window, other_surface)
            .is_err()
    );
    assert_eq!(routes.binding(viewport), Some(first));
    assert!(routes.has_window_receipt(viewport, first));

    assert!(
        routes
            .replace(viewport, first, window, second)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        routes
            .route(viewport)
            .unwrap()
            .terminal
            .as_ref()
            .unwrap()
            .binding(),
        second
    );
    assert_eq!(routes.focus_binding_for_event(window, None), None);
    assert!(routes.attach(viewport, first, window).is_err());
    assert!(routes.take_window_receipt(viewport, first).is_none());
    let receipt = routes
        .reserve_replacement(viewport, second, third)
        .unwrap()
        .unwrap();
    assert_eq!(receipt.binding(), second);
    assert_eq!(receipt.window(), window);
    assert_eq!(routes.route_for_event(window, None), None);
    let third_window = WindowId::from(22);
    routes.attach(viewport, third, third_window).unwrap();
    assert!(routes.suppress_pointer_route(viewport, third));
    let receipt = routes
        .reserve_retired_replacement(viewport, third, fourth)
        .unwrap()
        .unwrap();
    assert_eq!(receipt.binding(), third);
    assert_eq!(receipt.window(), third_window);
    assert_eq!(routes.binding(viewport), Some(fourth));
    assert_eq!(routes.window(viewport), None);
    assert_eq!(routes.route_for_event(third_window, None), None);
    routes.attach(viewport, fourth, WindowId::from(23)).unwrap();
    let fifth = bindings().0;
    let receipt = routes
        .replace(viewport, fourth, WindowId::from(24), fifth)
        .unwrap()
        .unwrap();
    assert_eq!(receipt.binding(), fourth);
    assert_eq!(receipt.window(), WindowId::from(23));
    assert_eq!(routes.route_for_event(WindowId::from(23), None), None);
    assert_eq!(routes.binding(viewport), Some(fifth));
    assert_eq!(routes.owned_data_capacity_bytes(), bytes);
    assert_eq!(routes.viewports.as_ptr(), backing);
}

#[test]
fn retirement_batches_reject_duplicates_and_overflow_without_partial_markers() {
    let (first, second) = bindings();
    let owners = [
        (ViewportId::ROOT, first),
        (ViewportId::from_hash_of("retirement-batch-peer"), second),
    ];
    let fixture = retirement_fixture(2, &owners);
    let committed = fixture.retirement_committed_routes();
    let mut one = NativeViewportMap::with_capacity_limit(1).unwrap();
    let bytes = one.owned_data_capacity_bytes();
    let backing = one.prepared_retirements.as_ptr();
    assert_eq!(
        one.prepare_retirements(&committed),
        Err(committed[1].binding())
    );
    assert!(one.prepared_retirements.is_empty());
    assert_eq!(one.owned_data_capacity_bytes(), bytes);
    assert_eq!(one.prepared_retirements.as_ptr(), backing);

    let mut routes = NativeViewportMap::with_capacity_limit(2).unwrap();
    let bytes = routes.owned_data_capacity_bytes();
    let backing = routes.prepared_retirements.as_ptr();
    let repeated = [committed[0], committed[0]];
    assert_eq!(
        routes.prepare_retirements(&repeated),
        Err(committed[0].binding())
    );
    assert!(routes.prepared_retirements.is_empty());
    routes.prepare_retirements(&committed[..1]).unwrap();
    let overlapping = [committed[1], committed[0]];
    assert_eq!(
        routes.prepare_retirements(&overlapping),
        Err(committed[0].binding())
    );
    assert_eq!(routes.prepared_retirements, [committed[0].binding()]);
    routes.abort_retirements(&committed[..1]);
    routes.prepare_retirements(&committed).unwrap();
    assert_eq!(routes.prepared_retirements.len(), 2);
    routes.abort_retirements(&committed);
    routes.abort_retirements(&committed);
    assert!(routes.prepared_retirements.is_empty());
    assert_eq!(routes.owned_data_capacity_bytes(), bytes);
    assert_eq!(routes.prepared_retirements.as_ptr(), backing);

    let mut awaiting = NativeViewportMap::with_capacity_limit(2).unwrap();
    for &(viewport, binding) in &owners {
        awaiting.reserve(viewport, binding).unwrap();
    }
    let pending = committed[1];
    assert!(awaiting.admit_test_create(pending.viewport(), pending.binding()));
    assert_eq!(
        awaiting.prepare_retirements(&committed),
        Err(pending.binding())
    );
    assert!(awaiting.prepared_retirements.is_empty());
    assert!(awaiting.has_pending_create_receipts());
    for &(viewport, binding) in &owners {
        assert_eq!(awaiting.binding(viewport), Some(binding));
    }
}

#[test]
fn prepared_batch_drop_releases_every_marker_and_commit_retains_empty_backing() {
    let (first, second) = bindings();
    let owners = [
        (ViewportId::ROOT, first),
        (ViewportId::from_hash_of("retirement-drop-peer"), second),
    ];
    let bridge = retirement_fixture(2, &owners);
    let committed = bridge.retirement_committed_routes();
    let (bytes, routes_backing, markers_backing) = bridge.with_viewports(|routes| {
        (
            routes.owned_data_capacity_bytes(),
            routes.viewports.as_ptr(),
            routes.prepared_retirements.as_ptr(),
        )
    });
    let prepared = bridge.prepare_route_retirements(&committed).unwrap();
    bridge.with_viewports_mut(|routes| {
        assert_eq!(routes.prepared_retirements.len(), 2);
        for &(viewport, binding) in &owners {
            assert!(routes.remove_viewport(viewport, binding).is_err());
            assert_eq!(routes.binding(viewport), Some(binding));
        }
    });
    drop(prepared);
    bridge.with_viewports(|routes| {
        assert!(routes.prepared_retirements.is_empty());
        assert_eq!(routes.bindings().count(), 2);
        assert_eq!(routes.owned_data_capacity_bytes(), bytes);
    });
    assert!(
        bridge
            .prepare_route_retirements(&committed)
            .unwrap()
            .commit()
            .is_empty()
    );
    bridge.with_viewports(|routes| {
        assert!(routes.bindings().next().is_none());
        assert!(routes.prepared_retirements.is_empty());
        assert_eq!(routes.owned_data_capacity_bytes(), bytes);
        assert_eq!(routes.viewports.as_ptr(), routes_backing);
        assert_eq!(routes.prepared_retirements.as_ptr(), markers_backing);
    });
}

#[test]
fn capture_preserves_fallback_without_publishing_or_recreating_the_window_receipt() {
    let (binding, peer) = bindings();
    let viewport = ViewportId::ROOT;
    let peer_viewport = ViewportId::from_hash_of("captured-window-peer");
    let window = WindowId::from(31);
    let mut routes = NativeViewportMap::with_capacity_limit(2).unwrap();
    routes.reserve(viewport, binding).unwrap();
    routes.reserve(peer_viewport, peer).unwrap();
    let bytes = routes.owned_data_capacity_bytes();
    let backing = routes.viewports.as_ptr();
    assert_eq!(routes.binding_for_roster(viewport, window), None);
    routes
        .capture_output_window(viewport, binding, window, None)
        .unwrap();
    assert!(!routes.route(viewport).unwrap().window_published);
    assert_eq!(routes.binding_for_event(window, None), Some(binding));
    assert_eq!(routes.binding_for_roster(viewport, window), Some(binding));
    assert_eq!(
        routes.pointer_binding_for_window(window),
        Some(binding.window())
    );
    assert_eq!(
        routes.binding_for_output(viewport, window, None),
        Some(binding)
    );
    assert!(
        routes
            .capture_output_window(peer_viewport, peer, window, None)
            .is_err()
    );
    routes.bind(viewport, window, binding).unwrap();
    assert!(!routes.route(viewport).unwrap().window_published);
    assert!(
        routes
            .replace(viewport, binding, window, binding)
            .unwrap()
            .is_none()
    );
    assert!(!routes.route(viewport).unwrap().window_published);

    let mut journal = crate::mailbox::journal::HostIngressJournal::new(
        dockspace::runtime::NativeMonotonicClock::system(),
    );
    assert_eq!(
        routes.receive_window_destroyed(window, Some(viewport), 1, |viewport, binding| {
            journal.reserve(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
                1,
                window,
                Some(viewport),
                Some(binding),
                WindowEvent::Destroyed,
            )))
        }),
        Some((viewport, binding))
    );
    let captured = routes.window_ingress().next().unwrap().ordinal();
    assert!(routes.attach_output(viewport, peer, window, None).is_err());
    assert!(!routes.route(viewport).unwrap().window_published);
    routes
        .attach_output(viewport, binding, window, None)
        .unwrap();
    assert!(routes.route(viewport).unwrap().window_published);
    assert!(routes.has_received_destroyed(viewport, binding, window, 1));
    assert_eq!(routes.window_ingress().next().unwrap().ordinal(), captured);
    routes
        .capture_output_window(viewport, binding, window, None)
        .unwrap();
    assert!(routes.route(viewport).unwrap().window_published);
    assert_eq!(routes.owned_data_capacity_bytes(), bytes);
    assert_eq!(routes.viewports.as_ptr(), backing);
}

#[test]
fn foreign_provider_handles_with_reused_public_ids_do_not_match_exact_guards() {
    let predecessor = bindings().0;
    let successor = bindings().0;
    assert_eq!(predecessor.surface(), successor.surface());
    assert_eq!(predecessor.window_token(), successor.window_token());
    assert_ne!(predecessor, successor);
    // Public sessions cannot mint a provider alias of the same lifetime. These
    // independently minted providers exercise exact-handle rejection without
    // constructing opaque bindings or claiming same-lifetime handoff coverage.
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(41);
    let fixture = retirement_fixture(1, &[(viewport, predecessor)]);
    let committed = fixture.retirement_committed_routes();
    let mut routes = NativeViewportMap::with_capacity_limit(1).unwrap();
    let bytes = routes.owned_data_capacity_bytes();
    routes.bind(viewport, window, predecessor).unwrap();
    assert!(routes.set_focus_reportable(&[predecessor]));
    assert!(
        routes
            .replace(viewport, predecessor, window, successor)
            .unwrap()
            .is_none()
    );
    assert_eq!(routes.focus_binding_for_event(window, None), None);
    assert!(!routes.set_focus_reportable(&[predecessor]));
    assert_eq!(routes.focus_binding_for_event(window, None), None);
    assert!(routes.attach(viewport, predecessor, window).is_err());
    assert!(routes.remove_viewport(viewport, predecessor).is_err());
    assert!(!routes.suppress_pointer_route(viewport, predecessor));
    assert!(routes.take_window_receipt(viewport, predecessor).is_none());
    routes.prepare_retirements(&committed).unwrap();
    assert!(routes.set_focus_reportable(&[successor]));
    assert_eq!(
        routes.focus_binding_for_event(window, None),
        Some(successor.window())
    );
    routes.commit_retirements(&committed);
    assert_eq!(routes.binding(viewport), Some(successor));
    assert!(routes.has_window_receipt(viewport, successor));
    assert!(routes.prepared_retirements.is_empty());
    assert_eq!(routes.owned_data_capacity_bytes(), bytes);
}

#[test]
fn constructor_overflow_and_reconfiguration_checks_preserve_empty_spare_owners() {
    for slots in [0, 1, 2, 7, 65] {
        let routes = NativeViewportMap::with_capacity_limit(slots).unwrap();
        assert_eq!(routes.viewports.capacity(), slots);
        assert_eq!(routes.prepared_retirements.capacity(), slots);
        assert_eq!(
            routes.owned_data_capacity_bytes(),
            NativeViewportMap::reservation_bytes(slots).unwrap()
        );
        assert!(!routes.can_configure_capacity_limit());
    }
    assert_eq!(NativeViewportMap::reservation_bytes(usize::MAX), None);
    assert_eq!(
        NativeViewportMap::with_capacity_limit(usize::MAX).unwrap_err(),
        NativeHostProtocolError::NativeReportStorageUnavailable
    );
    let (first, _) = bindings();
    let viewport = ViewportId::ROOT;
    let mut unlimited = NativeViewportMap::default();
    assert!(unlimited.can_configure_capacity_limit());
    unlimited.reserve(viewport, first).unwrap();
    assert!(!unlimited.can_configure_capacity_limit());
    let bytes = unlimited.owned_data_capacity_bytes();
    let backing = unlimited.viewports.as_ptr();
    drop(unlimited.remove_viewport(viewport, first).unwrap());
    assert!(unlimited.can_configure_capacity_limit());
    assert_eq!(unlimited.owned_data_capacity_bytes(), bytes);
    assert_eq!(unlimited.viewports.as_ptr(), backing);
    let fixture = retirement_fixture(1, &[(viewport, first)]);
    let committed = fixture.retirement_committed_routes();
    unlimited.prepare_retirements(&committed).unwrap();
    assert!(!unlimited.can_configure_capacity_limit());
    let with_markers = unlimited.owned_data_capacity_bytes();
    unlimited.abort_retirements(&committed);
    assert!(unlimited.can_configure_capacity_limit());
    assert_eq!(unlimited.owned_data_capacity_bytes(), with_markers);
}

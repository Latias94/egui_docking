use super::tests::bindings;
use super::*;
use crate::event::NativeWindowEventRecord;
use crate::mailbox::{HostRecord, journal::HostIngressJournal};
use crate::retirement::{DestroyedObservation, NativeRetirementState};
use dockspace::runtime::NativeMonotonicClock;
use winit::event::WindowEvent;

fn committed_retirement(receipt: NativeWindowReceipt) -> CommittedRetirement {
    let viewport = receipt.viewport();
    let window = receipt.window();
    let binding = receipt.binding();
    let mut retirement = NativeRetirementState::default();
    retirement.retain_window(receipt).unwrap();
    let mut journal = HostIngressJournal::new(NativeMonotonicClock::system());
    assert_eq!(
        retirement.receive_destroyed(window, Some(viewport), 1, |viewport, binding| {
            journal.reserve(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
                1,
                window,
                Some(viewport),
                Some(binding),
                WindowEvent::Destroyed,
            )))
        }),
        Some((viewport, binding)),
    );
    assert_eq!(
        retirement.observe_destroyed(viewport, binding, window, 1),
        DestroyedObservation::Recorded,
    );
    assert!(retirement.transfer_destroyed(binding, window, 1));
    let ordinal = retirement.window_ingress().next().unwrap().ordinal();
    drop(retirement.take_window_ingress(ordinal).unwrap());
    retirement.commit_destroyed_callbacks();
    retirement.mark_snapshot_queued();
    retirement.settle_snapshot(true);
    retirement.committed_routes().next().unwrap()
}

#[test]
fn only_existing_routes_retain_wakes_and_claim_each_release_once_without_growth() {
    let (first, second) = bindings();
    let child = ViewportId::from_hash_of("wake-owner-child");
    let unknown = ViewportId::from_hash_of("wake-owner-unknown");
    let window = WindowId::from(91);
    let mut routes = NativeViewportMap::with_capacity_limit(1).unwrap();
    let bytes = routes.owned_data_capacity_bytes();
    let backing = routes.viewports.as_ptr();
    assert!(!routes.block_semantic_viewport(child));
    assert!(!routes.block_semantic_viewport(ViewportId::ROOT));
    assert!(!routes.has_semantic_wake());
    routes.bind(child, window, first).unwrap();
    assert_eq!(routes.binding_for_output(child, window, None), Some(first));
    // Output admission owns its rejection policy. This route retains the
    // resulting repaint without needing another slot after the rejection.
    assert!(routes.block_semantic_viewport(child));
    assert!(routes.block_semantic_viewport(child));
    assert!(routes.semantic_viewport_blocked(child));
    assert!(!routes.semantic_viewport_ready(child));
    assert!(routes.has_semantic_wake());
    assert_eq!(routes.claim_ready_semantic_viewport(), None);
    assert_eq!(
        routes.reserve(unknown, second),
        Err(NativeViewportBindingError::CapacityExceeded { limit: 1 }),
    );
    assert!(!routes.block_semantic_viewport(unknown));
    assert!(!routes.semantic_viewport_blocked(unknown));
    assert!(!routes.semantic_viewport_ready(unknown));
    assert_eq!(routes.binding(unknown), None);
    assert_eq!(routes.binding_for_output(child, window, None), Some(first));
    routes.release_blocked_semantic_viewports();
    routes.release_blocked_semantic_viewports();
    assert!(routes.semantic_viewport_ready(child));
    assert!(!routes.semantic_viewport_blocked(child));
    assert_eq!(routes.claim_ready_semantic_viewport(), Some(child));
    assert_eq!(routes.claim_ready_semantic_viewport(), None);
    routes.release_blocked_semantic_viewports();
    assert_eq!(routes.claim_ready_semantic_viewport(), None);
    assert!(!routes.has_semantic_wake());
    assert_eq!(routes.viewports.as_ptr(), backing);
    assert_eq!(routes.owned_data_capacity_bytes(), bytes);
}

#[test]
fn retained_pass_reblocks_a_ready_or_claimed_route_until_the_next_release() {
    let (binding, _) = bindings();
    let viewport = ViewportId::ROOT;
    let mut routes = NativeViewportMap::with_capacity_limit(1).unwrap();
    routes.reserve(viewport, binding).unwrap();
    let bytes = routes.owned_data_capacity_bytes();
    assert!(routes.block_semantic_viewport(viewport));
    routes.release_blocked_semantic_viewports();
    assert!(routes.semantic_viewport_ready(viewport));
    assert!(routes.block_semantic_viewport(viewport));
    assert!(!routes.semantic_viewport_ready(viewport));
    assert_eq!(routes.claim_ready_semantic_viewport(), None);
    routes.release_blocked_semantic_viewports();
    assert_eq!(routes.claim_ready_semantic_viewport(), Some(viewport));
    assert!(!routes.has_semantic_wake());
    assert!(routes.block_semantic_viewport(viewport));
    assert_eq!(routes.claim_ready_semantic_viewport(), None);
    routes
        .attach(viewport, binding, WindowId::from(92))
        .unwrap();
    assert!(routes.semantic_viewport_blocked(viewport));
    routes.release_blocked_semantic_viewports();
    assert_eq!(routes.claim_ready_semantic_viewport(), Some(viewport));
    assert_eq!(routes.claim_ready_semantic_viewport(), None);
    assert_eq!(routes.owned_data_capacity_bytes(), bytes);
}

#[test]
fn every_same_viewport_replacement_preserves_blocked_and_ready_wakes() {
    let (predecessor, _) = bindings();
    let successor = bindings().0;
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(93);
    for ready in [false, true] {
        for replacement in 0..4 {
            let mut routes = NativeViewportMap::with_capacity_limit(1).unwrap();
            routes.bind(viewport, window, predecessor).unwrap();
            let bytes = routes.owned_data_capacity_bytes();
            let backing = routes.viewports.as_ptr();
            assert!(routes.block_semantic_viewport(viewport));
            if ready {
                routes.release_blocked_semantic_viewports();
            }
            let displaced = match replacement {
                0 => routes.reserve_replacement(viewport, predecessor, successor),
                1 => {
                    assert!(routes.suppress_pointer_route(viewport, predecessor));
                    routes.reserve_retired_replacement(viewport, predecessor, successor)
                }
                2 => routes.replace(viewport, predecessor, WindowId::from(94), successor),
                3 => routes.replace(viewport, predecessor, window, successor),
                _ => unreachable!(),
            }
            .unwrap();
            if let Some(receipt) = displaced {
                assert_eq!(receipt.binding(), predecessor);
                assert_eq!(receipt.window(), window);
            } else {
                assert_eq!(replacement, 3);
                assert!(routes.has_window_receipt(viewport, successor));
            }
            assert_eq!(routes.binding(viewport), Some(successor));
            assert!(!routes.clear_semantic_wake(viewport, predecessor));
            assert_eq!(routes.semantic_viewport_blocked(viewport), !ready);
            assert_eq!(routes.semantic_viewport_ready(viewport), ready);
            assert!(routes.has_semantic_wake());
            routes.release_blocked_semantic_viewports();
            assert_eq!(routes.claim_ready_semantic_viewport(), Some(viewport));
            assert_eq!(routes.claim_ready_semantic_viewport(), None);
            assert_eq!(routes.viewports.as_ptr(), backing);
            assert_eq!(routes.owned_data_capacity_bytes(), bytes);
        }
    }
}

#[test]
fn predecessor_retirement_cannot_clear_successor_wake_and_exact_retirement_ends_it() {
    let (predecessor, _) = bindings();
    let successor = bindings().0;
    let viewport = ViewportId::ROOT;
    let mut routes = NativeViewportMap::with_capacity_limit(1).unwrap();
    routes
        .bind(viewport, WindowId::from(95), predecessor)
        .unwrap();
    let bytes = routes.owned_data_capacity_bytes();
    assert!(routes.block_semantic_viewport(viewport));
    let predecessor_receipt = routes
        .replace(viewport, predecessor, WindowId::from(96), successor)
        .unwrap()
        .unwrap();
    let retired = committed_retirement(predecessor_receipt);
    assert!(!routes.clear_semantic_wake(viewport, predecessor));
    routes.prepare_retirements(&[retired]).unwrap();
    routes.commit_retirements(&[retired]);
    assert_eq!(routes.binding(viewport), Some(successor));
    assert!(routes.semantic_viewport_blocked(viewport));
    let successor_receipt = routes.take_window_receipt(viewport, successor).unwrap();
    let retired = committed_retirement(successor_receipt);
    routes.release_blocked_semantic_viewports();
    routes.prepare_retirements(&[retired]).unwrap();
    routes.abort_retirements(&[retired]);
    assert!(routes.semantic_viewport_ready(viewport));
    routes.prepare_retirements(&[retired]).unwrap();
    routes.commit_retirements(&[retired]);
    assert_eq!(routes.binding(viewport), None);
    assert!(!routes.has_semantic_wake());
    assert_eq!(routes.claim_ready_semantic_viewport(), None);
    assert_eq!(routes.owned_data_capacity_bytes(), bytes);
}

#[test]
fn exact_clear_and_unbind_end_only_the_matching_route_obligation() {
    let (first, second) = bindings();
    let child = ViewportId::from_hash_of("wake-owner-clear-child");
    let mut routes = NativeViewportMap::with_capacity_limit(2).unwrap();
    routes
        .bind(ViewportId::ROOT, WindowId::from(97), first)
        .unwrap();
    routes.reserve(child, second).unwrap();
    let bytes = routes.owned_data_capacity_bytes();
    assert!(routes.block_semantic_viewport(ViewportId::ROOT));
    assert!(routes.block_semantic_viewport(child));
    routes.release_blocked_semantic_viewports();
    assert!(!routes.clear_semantic_wake(child, first));
    assert!(routes.semantic_viewport_ready(child));
    assert!(routes.clear_semantic_wake(ViewportId::ROOT, first));
    assert!(routes.clear_semantic_wake(ViewportId::ROOT, first));
    assert!(!routes.semantic_viewport_ready(ViewportId::ROOT));
    assert!(routes.has_semantic_wake());
    assert_eq!(routes.claim_ready_semantic_viewport(), Some(child));
    assert!(routes.block_semantic_viewport(child));
    assert!(routes.remove_viewport(child, first).is_err());
    assert!(routes.semantic_viewport_blocked(child));
    drop(routes.remove_viewport(child, second).unwrap());
    assert!(!routes.block_semantic_viewport(child));
    assert!(!routes.has_semantic_wake());
    routes.reserve(child, second).unwrap();
    assert!(!routes.semantic_viewport_blocked(child));
    assert!(!routes.semantic_viewport_ready(child));
    assert_eq!(routes.claim_ready_semantic_viewport(), None);
    assert_eq!(routes.owned_data_capacity_bytes(), bytes);
}

#[test]
fn claims_follow_viewport_order_and_quarantine_clears_both_states_without_unbinding() {
    let (first, second) = bindings();
    let mut viewports = [
        ViewportId::from_hash_of("wake-owner-order-first"),
        ViewportId::from_hash_of("wake-owner-order-second"),
    ];
    viewports.sort_unstable();
    let mut routes = NativeViewportMap::with_capacity_limit(2).unwrap();
    routes.reserve(viewports[1], second).unwrap();
    routes.reserve(viewports[0], first).unwrap();
    let bytes = routes.owned_data_capacity_bytes();
    let backing = routes.viewports.as_ptr();
    assert!(routes.block_semantic_viewport(viewports[1]));
    assert!(routes.block_semantic_viewport(viewports[0]));
    routes.release_blocked_semantic_viewports();
    assert_eq!(routes.claim_ready_semantic_viewport(), Some(viewports[0]));
    assert_eq!(routes.claim_ready_semantic_viewport(), Some(viewports[1]));
    assert_eq!(routes.claim_ready_semantic_viewport(), None);
    assert!(routes.block_semantic_viewport(viewports[0]));
    routes.release_blocked_semantic_viewports();
    assert!(routes.block_semantic_viewport(viewports[1]));
    assert!(routes.semantic_viewport_ready(viewports[0]));
    assert!(routes.semantic_viewport_blocked(viewports[1]));
    routes.clear_semantic_wakes();
    routes.clear_semantic_wakes();
    assert!(!routes.has_semantic_wake());
    routes.release_blocked_semantic_viewports();
    assert_eq!(routes.claim_ready_semantic_viewport(), None);
    assert_eq!(routes.binding(viewports[0]), Some(first));
    assert_eq!(routes.binding(viewports[1]), Some(second));
    assert_eq!(routes.viewports.as_ptr(), backing);
    assert_eq!(routes.owned_data_capacity_bytes(), bytes);
}

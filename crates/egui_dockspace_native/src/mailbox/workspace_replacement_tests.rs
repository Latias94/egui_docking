//! Atomic native route consumption of multi-Root core continuations.

use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{HostWindowToken, NativePointerRoster, SurfaceUnavailableReason};
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;

use super::*;

const SURFACES: [SurfaceId; 2] = [SurfaceId::new(1), SurfaceId::new(2)];

fn viewports() -> [ViewportId; 2] {
    [
        ViewportId::ROOT,
        ViewportId::from_hash_of("workspace-root-peer"),
    ]
}

fn windows() -> [WindowId; 2] {
    [WindowId::from(91), WindowId::from(92)]
}

fn layout() -> DockspaceLayout {
    DockspaceLayout::new(SURFACES.map(|surface| {
        DockspaceSurfaceLayout::new(
            surface,
            DockspaceRootLayout::new(
                RootId::new(surface.get()),
                DockspaceNode::central_tabs([ItemId::new(surface.get())]),
            ),
        )
    }))
    .unwrap()
}

fn fixture(
    window_capacity: usize,
) -> (
    DockspaceSession,
    NativeHostBridge,
    [NativeSurfaceBinding; 2],
) {
    let mut session = DockspaceSession::from_layout(layout(), DockPolicy::default()).unwrap();
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    for (index, surface) in SURFACES.into_iter().enumerate() {
        session
            .register_native_root(surface, HostWindowToken::new(index as u64 + 1))
            .unwrap();
    }
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    let bindings = SURFACES.map(|surface| {
        *report
            .native_bindings()
            .iter()
            .find(|binding| binding.surface() == surface)
            .unwrap()
    });
    let bridge = NativeHostBridge::try_new_with_capacity(
        NativeViewportMap::with_capacity_limit(2).unwrap(),
        window_capacity,
        1,
    )
    .unwrap();
    for index in 0..2 {
        bridge
            .bind_viewport_route(viewports()[index], windows()[index], bindings[index])
            .unwrap();
    }
    bridge.with_viewports_mut(|routes| {
        routes.set_focus_reportable(&bindings);
        for viewport in viewports() {
            assert!(routes.block_semantic_viewport(viewport));
        }
    });
    (session, bridge, bindings)
}

fn swapping_frame(
    session: &mut DockspaceSession,
    predecessors: [NativeSurfaceBinding; 2],
) -> DockspaceHostFrame<'_> {
    let reset = session.prepare_layout_reset(layout()).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_native_layout_reset_with_roots(
            &reset,
            [
                (predecessors[0], SURFACES[1]),
                (predecessors[1], SURFACES[0]),
            ],
        )
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame
}

fn assert_predecessors(bridge: &NativeHostBridge, predecessors: [NativeSurfaceBinding; 2]) {
    bridge.with_viewports(|routes| {
        for index in 0..2 {
            assert_eq!(
                routes.binding(viewports()[index]),
                Some(predecessors[index])
            );
            assert_eq!(routes.window(viewports()[index]), Some(windows()[index]));
            assert!(routes.has_window_receipt(viewports()[index], predecessors[index]));
            assert!(routes.semantic_viewport_blocked(viewports()[index]));
        }
    });
}

#[test]
fn workspace_root_batch_swap_preserves_each_physical_receipt_repaint_and_old_callback() {
    let (mut session, bridge, predecessors) = fixture(4);
    let route_bytes = bridge.with_viewports(NativeViewportMap::owned_data_capacity_bytes);
    let frame = swapping_frame(&mut session, predecessors);
    let proposed = frame
        .proposed_native_root_continuations()
        .collect::<Vec<_>>();
    assert_eq!(proposed.len(), 2);
    let mut root_surface = SURFACES[0];
    let (mut report, _) = bridge
        .commit_host_frame_with_staging(frame, true, Some(&mut root_surface))
        .unwrap();
    assert_eq!(report.native_root_continuations(), proposed);
    assert_eq!(root_surface, SURFACES[1]);
    assert!(report.take_native_effects().is_empty());
    for index in 0..2 {
        let continuation = proposed
            .iter()
            .find(|continuation| continuation.predecessor() == predecessors[index])
            .unwrap();
        let successor = continuation.successor();
        assert_eq!(successor.surface(), SURFACES[1 - index]);
        assert!(session.is_current_native_binding(successor));
        assert!(!session.is_current_native_binding(predecessors[index]));
        assert_eq!(
            bridge.workspace_replacement_predecessor(successor),
            Some((viewports()[index], predecessors[index]))
        );
        assert!(bridge.retirement_is_quiescence_candidate(predecessors[index]));
        bridge.with_viewports(|routes| {
            assert_eq!(
                routes.viewport(successor.surface()),
                Some(viewports()[index])
            );
            assert_eq!(routes.binding(viewports()[index]), Some(successor));
            assert_eq!(routes.window(viewports()[index]), Some(windows()[index]));
            assert!(routes.has_window_receipt(viewports()[index], successor));
            assert!(!routes.has_window_receipt(viewports()[index], predecessors[index]));
            assert_eq!(
                routes.route_for_event(windows()[index], None),
                Some((viewports()[index], successor))
            );
            assert_eq!(routes.focus_binding_for_event(windows()[index], None), None);
            assert!(routes.semantic_viewport_blocked(viewports()[index]));
            assert_eq!(routes.owned_data_capacity_bytes(), route_bytes);
        });
        // A callback captured before publication keeps its old binding even
        // though that surface is now hosted by the other physical window.
        bridge.push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            index as u64 + 1,
            windows()[index],
            Some(viewports()[index]),
            Some(predecessors[index]),
            WindowEvent::Resized(PhysicalSize::new(700, 500)),
        )));
    }
    let callbacks = bridge
        .lock()
        .journal
        .iter()
        .filter_map(|record| match record {
            HostRecord::WindowEvent(event) => event.binding(),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(callbacks, predecessors);
    bridge.with_viewports_mut(|routes| {
        routes.release_blocked_semantic_viewports();
        let mut ready =
            std::iter::from_fn(|| routes.claim_ready_semantic_viewport()).collect::<Vec<_>>();
        ready.sort_unstable();
        let mut expected = viewports().to_vec();
        expected.sort_unstable();
        assert_eq!(ready, expected);
        assert_eq!(routes.claim_ready_semantic_viewport(), None);
    });
}

#[test]
fn workspace_root_batch_rejects_partial_duplicate_and_stale_rosters_without_route_changes() {
    let (mut session, bridge, predecessors) = fixture(4);
    let reset = session.prepare_layout_reset(layout()).unwrap();
    let mut identity = session.begin_host_frame().unwrap();
    identity.submit_layout_reset(&reset).unwrap();
    let identity_pairs = identity
        .proposed_native_root_continuations()
        .collect::<Vec<_>>();
    drop(identity);
    let frame = swapping_frame(&mut session, predecessors);
    let proposed = frame
        .proposed_native_root_continuations()
        .collect::<Vec<_>>();
    let first = *proposed
        .iter()
        .find(|continuation| continuation.predecessor() == predecessors[0])
        .unwrap();
    let second_identity = *identity_pairs
        .iter()
        .find(|continuation| continuation.predecessor() == predecessors[1])
        .unwrap();
    assert_eq!(
        first.successor().surface(),
        second_identity.successor().surface()
    );
    for invalid in [
        vec![first],
        vec![first, first],
        vec![first, second_identity],
    ] {
        assert!(bridge.with_viewports(|routes| {
            routes
                .validate_workspace_root_continuations(|| invalid.iter().copied())
                .is_err()
        }));
        assert_predecessors(&bridge, predecessors);
    }
    bridge
        .commit_host_frame_with_staging(frame, true, None)
        .unwrap();
    let bindings = bridge.with_viewports(|routes| routes.bindings().collect::<Vec<_>>());
    assert!(bridge.with_viewports(|routes| {
        routes
            .validate_workspace_root_continuations(|| proposed.iter().copied())
            .is_err()
    }));
    assert_eq!(
        bridge.with_viewports(|routes| routes.bindings().collect::<Vec<_>>()),
        bindings
    );
}

#[test]
fn workspace_root_batch_capacity_refusal_releases_all_speculative_successors() {
    // Two live predecessors plus one successor fit. The second successor must
    // fail admission without publishing the first half of the batch.
    let (mut session, bridge, predecessors) = fixture(3);
    let before = session.version();
    let reset = session.prepare_layout_reset(layout()).unwrap();
    let mut exact = None;
    for _ in 0..2 {
        let frame = swapping_frame(&mut session, predecessors);
        let proposed = frame
            .proposed_native_root_continuations()
            .collect::<Vec<_>>();
        assert_eq!(exact.get_or_insert_with(|| proposed.clone()), &proposed);
        let mut root_surface = SURFACES[0];
        assert!(
            bridge
                .commit_host_frame_with_staging(frame, true, Some(&mut root_surface))
                .is_err()
        );
        assert_eq!(session.version(), before);
        assert_eq!(root_surface, SURFACES[0]);
        assert_predecessors(&bridge, predecessors);
        let state = bridge.lock();
        assert_eq!(state.window_resources.bindings().count(), 2);
        assert!(!state.retirements.has_pending_work());
        for continuation in proposed {
            assert_eq!(
                state
                    .window_resources
                    .workspace_predecessor(continuation.successor()),
                None
            );
            assert!(
                !state
                    .window_resources
                    .references_binding(continuation.successor())
            );
        }
    }
    // The original preparation remains usable after failed host admission.
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_native_layout_reset_with_roots(
            &reset,
            [
                (predecessors[0], SURFACES[1]),
                (predecessors[1], SURFACES[0]),
            ],
        )
        .unwrap();
    assert_eq!(
        frame
            .proposed_native_root_continuations()
            .collect::<Vec<_>>(),
        exact.unwrap()
    );
}

#[test]
fn workspace_root_batch_rejects_one_destroyed_receipt_before_publishing_any_successor() {
    let (mut session, bridge, predecessors) = fixture(4);
    let before = session.version();
    let frame = swapping_frame(&mut session, predecessors);
    let proposed = frame
        .proposed_native_root_continuations()
        .collect::<Vec<_>>();
    bridge.push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
        1,
        windows()[1],
        Some(viewports()[1]),
        Some(predecessors[1]),
        WindowEvent::Destroyed,
    )));
    assert!(
        bridge
            .commit_host_frame_with_staging(frame, true, None)
            .is_err()
    );
    assert_eq!(session.version(), before);
    assert_predecessors(&bridge, predecessors);
    assert!(bridge.with_viewports(|routes| {
        routes.has_received_destroyed(viewports()[1], predecessors[1], windows()[1], 1)
    }));
    assert_eq!(bridge.lock().window_resources.bindings().count(), 2);
    for continuation in proposed {
        assert_eq!(
            bridge.workspace_replacement_predecessor(continuation.successor()),
            None
        );
    }
}

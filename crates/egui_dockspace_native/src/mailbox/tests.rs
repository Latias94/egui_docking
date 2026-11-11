use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::DockspaceSession;
use winit::dpi::PhysicalPosition;
use winit::event::{DeviceId, PointerEventFacts, WindowEvent};
use winit::window::WindowId;

#[test]
fn rejected_finalizer_drops_after_host_state_unlocks() {
    use std::sync::Weak;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Finalizer {
        bridge: Weak<NativeHostBridge>,
        dropped: Arc<AtomicUsize>,
    }

    impl NativeOutputPassFinalizer for Finalizer {
        fn finalize(&self, _: NativeOutputPassFinalization<'_>) -> NativeHostWake {
            NativeHostWake::Wait
        }
    }

    impl Drop for Finalizer {
        fn drop(&mut self) {
            if let Some(bridge) = self.bridge.upgrade() {
                assert!(
                    bridge.records.try_lock().is_ok(),
                    "a rejected external finalizer must drop outside the host-state lock"
                );
                assert!(!bridge.has_pending_coordinator_work());
                self.dropped.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    let bridge = Arc::new(NativeHostBridge::new(NativeViewportMap::default()));
    let dropped = Arc::new(AtomicUsize::new(0));
    assert!(bridge.install_output_pass_finalizer(Arc::new(Finalizer {
        bridge: Weak::new(),
        dropped: Arc::clone(&dropped),
    })));
    assert!(!bridge.install_output_pass_finalizer(Arc::new(Finalizer {
        bridge: Arc::downgrade(&bridge),
        dropped: Arc::clone(&dropped),
    })));
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}

#[test]
fn displaced_window_destroyed_is_once_only_and_cannot_retire_its_successor() {
    let predecessor = bindings().0;
    let successor = bindings().0;
    let viewport = ViewportId::from_hash_of("overlapping-destroyed-receipts");
    let old_window = WindowId::from(81);
    let new_window = WindowId::from(82);
    let mut routes = NativeViewportMap::default();
    routes.bind(viewport, old_window, predecessor).unwrap();
    let bridge = Arc::new(NativeHostBridge::new(routes));
    assert!(
        bridge
            .with_viewports_mut(|routes| { routes.suppress_pointer_route(viewport, predecessor) })
    );
    bridge
        .reserve_retired_viewport_replacement(viewport, predecessor, successor)
        .unwrap();
    bridge
        .with_viewports_mut(|routes| routes.attach_output(viewport, successor, new_window, None))
        .unwrap();
    let destroyed = |ordinal| {
        NativeWindowEventRecord::for_test(
            ordinal,
            old_window,
            Some(viewport),
            None,
            WindowEvent::Destroyed,
        )
    };
    bridge.push_record(HostRecord::WindowEvent(destroyed(1)));
    bridge.push_record(HostRecord::WindowEvent(destroyed(2)));
    assert_eq!(bridge.lock().journal.len(), 0);
    assert_eq!(bridge.lock().retained_record_count(), 1);
    assert_eq!(bridge.front_event().unwrap().binding(), Some(predecessor));
    assert_eq!(
        bridge.retirement_observe_destroyed(viewport, predecessor, old_window, 1),
        DestroyedObservation::Recorded
    );
    assert!(bridge.acknowledge_event(1));
    assert!(!bridge.acknowledge_event(1));
    bridge.commit_frame_boundary(true);
    assert!(bridge.retirement_destroyed_observations().is_empty());
    assert!(bridge.retirement_quiescence_candidates().is_empty());
    bridge.push_record(HostRecord::WindowEvent(destroyed(3)));
    assert_eq!(bridge.lock().journal.retained_record_count(), 1);
    bridge.commit_frame_boundary(false);
    assert_eq!(bridge.retirement_destroyed_observations().len(), 1);
    bridge.retirement_mark_snapshot_queued();
    bridge.retirement_settle_snapshot(true);
    let committed = bridge.retirement_committed_routes();
    let prepared = bridge.prepare_route_retirements(&committed).unwrap();
    drop(prepared);
    assert!(bridge.retirement_quiescence_candidates().is_empty());
    assert!(
        bridge
            .prepare_route_retirements(&committed)
            .unwrap()
            .commit()
            .is_empty()
    );
    bridge.retirement_commit_routes(&committed);
    assert_eq!(bridge.retirement_quiescence_candidates(), [predecessor]);
    assert!(bridge.retirement_finish_quiescence(predecessor));
    bridge.push_record(HostRecord::WindowEvent(destroyed(4)));
    assert!(bridge.lock().journal.is_empty());
    assert_eq!(
        bridge.with_viewports(|routes| routes.binding(viewport)),
        Some(successor)
    );
    assert_eq!(
        bridge.with_viewports(|routes| routes.window(viewport)),
        Some(new_window)
    );
    assert_eq!(
        bridge.with_viewports(|routes| routes.pointer_binding_for_window(new_window)),
        Some(successor.window())
    );
    assert!(bridge.with_viewports(|routes| routes.has_window_receipt(viewport, successor)));
}

#[test]
fn same_physical_window_rebind_rejects_a_received_destroyed_receipt() {
    let (mut session, bridge, predecessor) = workspace_root_fixture();
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(91);
    let reset = session
        .prepare_layout_reset(workspace_root_layout(predecessor.surface()))
        .unwrap();
    let version = session.version();
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_layout_reset(&reset).unwrap();
    let successor = frame
        .proposed_native_root_continuations()
        .next()
        .unwrap()
        .successor();
    frame
        .complete_unpainted_surfaces(dockspace::runtime::SurfaceUnavailableReason::Deferred)
        .unwrap();
    bridge.push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
        1,
        window,
        Some(viewport),
        Some(predecessor),
        WindowEvent::Destroyed,
    )));
    assert!(
        bridge
            .commit_host_frame_with_staging(frame, false, None)
            .is_err()
    );
    assert_eq!(session.version(), version);
    assert_eq!(bridge.front_event().unwrap().binding(), Some(predecessor));
    assert!(!bridge.with_viewports(|routes| routes.has_window_receipt(viewport, successor)));
    assert!(bridge.with_viewports(|routes| routes.has_window_receipt(viewport, predecessor)));
    assert!(!bridge.lock().retirements.has_window(viewport, predecessor));
    bridge.push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
        2,
        window,
        Some(viewport),
        Some(successor),
        WindowEvent::Destroyed,
    )));
    assert_eq!(bridge.lock().journal.len(), 0);
    assert_eq!(bridge.lock().retained_record_count(), 1);
    assert_eq!(
        bridge.retirement_observe_destroyed(viewport, predecessor, window, 1),
        DestroyedObservation::Recorded
    );
    assert!(bridge.acknowledge_event(1));
    bridge.commit_frame_boundary(false);
    assert_eq!(
        bridge.retirement_destroyed_observations(),
        [(predecessor, NativeWindowFacts::destroyed())]
    );
}

#[test]
fn retired_physical_owners_reject_binding_and_window_reuse_before_quiescence() {
    let predecessor = bindings().0;
    let successor = bindings().0;
    let viewport = ViewportId::from_hash_of("retired-window-admission");
    let old_window = WindowId::from(86);
    let bridge = NativeHostBridge::new(NativeViewportMap::default());
    bridge
        .bind_viewport_route(viewport, old_window, predecessor)
        .unwrap();
    assert!(
        bridge
            .with_viewports_mut(|routes| { routes.suppress_pointer_route(viewport, predecessor) })
    );
    bridge
        .reserve_retired_viewport_replacement(viewport, predecessor, successor)
        .unwrap();
    assert!(
        bridge
            .bind_viewport_route(viewport, WindowId::from(87), predecessor)
            .is_err()
    );
    assert!(
        bridge
            .bind_viewport_route(viewport, old_window, successor)
            .is_err()
    );
    assert_eq!(
        bridge.with_viewports(|viewports| viewports.binding(viewport)),
        Some(successor)
    );
    assert!(bridge.lock().retirements.has_window(viewport, predecessor));
    bridge
        .with_viewports_mut(|routes| {
            routes.attach_output(viewport, successor, WindowId::from(87), None)
        })
        .unwrap();
    assert_eq!(
        bridge.with_viewports(|viewports| viewports.binding(viewport)),
        Some(successor)
    );
}

#[test]
fn awaiting_physical_window_rebind_preserves_one_terminal_owner() {
    let (mut session, bridge, predecessor) = workspace_root_fixture();
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(91);
    let reset = session
        .prepare_layout_reset(workspace_root_layout(predecessor.surface()))
        .unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_layout_reset(&reset).unwrap();
    frame
        .complete_unpainted_surfaces(dockspace::runtime::SurfaceUnavailableReason::Deferred)
        .unwrap();
    let (report, _) = bridge
        .commit_host_frame_with_staging(frame, false, None)
        .unwrap();
    let successor = report.native_bindings()[0];
    bridge.push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
        1,
        window,
        Some(viewport),
        None,
        WindowEvent::Destroyed,
    )));
    assert_eq!(bridge.front_event().unwrap().binding(), Some(successor));
    assert!(!bridge.lock().retirements.has_window(viewport, predecessor));
    assert!(!bridge.lock().retirements.has_window(viewport, successor));
    assert!(bridge.with_viewports(|routes| {
        routes.has_received_destroyed(viewport, successor, window, 1)
    }));
}

#[test]
fn retired_replacement_preserves_destroyed_owner_and_unattached_create_cannot_be_discarded() {
    let predecessor = bindings().0;
    let successor = bindings().0;
    let viewport = ViewportId::from_hash_of("unbound-window-receipt");
    let window = WindowId::from(84);
    let mut routes = NativeViewportMap::default();
    routes.bind(viewport, window, predecessor).unwrap();
    let bridge = NativeHostBridge::new(routes);
    assert!(
        bridge
            .with_viewports_mut(|routes| { routes.suppress_pointer_route(viewport, predecessor) })
    );
    bridge
        .reserve_retired_viewport_replacement(viewport, predecessor, successor)
        .unwrap();
    bridge
        .with_viewports_mut(|routes| {
            routes.attach_output(viewport, successor, WindowId::from(85), None)
        })
        .unwrap();
    bridge.push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
        1,
        window,
        None,
        None,
        WindowEvent::Destroyed,
    )));
    let event = bridge.front_event().unwrap();
    assert_eq!(event.binding(), Some(predecessor));
    assert_eq!(event.viewport_id(), Some(viewport));
    assert_eq!(
        bridge.with_viewports(|routes| routes.binding(viewport)),
        Some(successor)
    );

    let pending = ViewportId::from_hash_of("unattached-create-receipt");
    let binding = bindings().1;
    bridge
        .with_viewports_mut(|routes| routes.reserve(pending, binding))
        .unwrap();
    assert!(bridge.with_viewports_mut(|routes| routes.admit_test_create(pending, binding)));
    assert!(!bridge.retire_unstarted_viewport(pending, binding));
    assert_eq!(
        bridge.with_viewports(|routes| routes.binding(pending)),
        Some(binding)
    );
    assert!(bridge.with_viewports(|routes| routes.has_pending_create_receipts()));
}

#[test]
fn failed_create_receipt_moves_once_and_waits_for_its_recorded_full_boundary() {
    let (binding, successor) = bindings();
    let viewport = ViewportId::from_hash_of("create-receipt-transfer");
    let mut routes = NativeViewportMap::default();
    routes.reserve(viewport, binding).unwrap();
    let bridge = NativeHostBridge::new(routes);
    assert!(bridge.reserve_create(viewport, binding, NativePhysicalRect::new(0, 0, 320, 240)));
    assert_eq!(
        bridge.record_viewport_create_failure_for_test(viewport),
        NativeHostWake::RepaintRoot
    );
    let failure = bridge.front_viewport_create_failure().unwrap();
    assert_eq!(
        bridge.record_viewport_create_failure_for_test(viewport),
        NativeHostWake::Wait
    );
    assert_eq!(bridge.lock().journal.len(), 0);
    assert_eq!(bridge.lock().retained_record_count(), 1);
    assert!(!bridge.acknowledge_viewport_create_failure(
        NativeViewportCreateFailureRecord::new(viewport, successor, failure.kind()),
        true
    ));
    assert!(bridge.acknowledge_viewport_create_failure(failure, false));
    assert_eq!(
        bridge.with_viewports(|routes| routes.binding(viewport)),
        None
    );
    assert!(!bridge.acknowledge_viewport_create_failure(failure, true));
    assert_eq!(
        bridge.record_viewport_create_failure_for_test(viewport),
        NativeHostWake::Wait
    );
    assert!(bridge.retirement_quiescence_candidates().is_empty());
    assert!(!bridge.retirement_finish_quiescence(binding));
    bridge.commit_frame_boundary(false);
    assert!(
        bridge.retirement_quiescence_candidates().is_empty(),
        "an unrecorded result survives an unrelated full commit"
    );
    bridge.retirement_mark_create_failure_recorded(binding);
    bridge.commit_frame_boundary(true);
    assert!(bridge.retirement_quiescence_candidates().is_empty());
    assert!(!bridge.retirement_finish_quiescence(binding));
    bridge.commit_frame_boundary(false);
    assert_eq!(bridge.retirement_quiescence_candidates(), [binding]);
    assert!(bridge.retirement_finish_quiescence(binding));
    assert!(!bridge.retirement_finish_quiescence(binding));
    assert_eq!(
        bridge.record_viewport_create_failure_for_test(viewport),
        NativeHostWake::Wait
    );
}

#[test]
fn transferred_create_success_keeps_one_owner_until_full_commit_and_preserves_successor() {
    let (binding, _) = bindings();
    let (successor, _) = bindings();
    let viewport = ViewportId::from_hash_of("create-receipt-success");
    let window = WindowId::from(81);
    let mut routes = NativeViewportMap::default();
    let mut journal = HostIngressJournal::new(NativeMonotonicClock::system());
    {
        routes.reserve(viewport, binding).unwrap();
        assert!(routes.admit_test_create(viewport, binding));
        assert_eq!(
            routes.receive_create_terminal(
                viewport,
                binding,
                None,
                CreateTerminal::Created(window),
                || journal.reserve(HostRecord::DeviceRemoval(
                    NativeDeviceRemovalRecord::for_test(1)
                )),
            ),
            CreateAdmission::First
        );
        routes
            .attach_output(viewport, binding, window, None)
            .unwrap();
        assert!(routes.transfer_create_terminal(
            viewport,
            binding,
            None,
            CreateTerminal::Created(window)
        ));
        assert_eq!(
            routes.receive_create_terminal(
                viewport,
                binding,
                None,
                CreateTerminal::Created(window),
                || panic!("a duplicate cannot recapture create ingress"),
            ),
            CreateAdmission::Duplicate
        );
        assert_eq!(
            routes.receive_create_terminal(
                viewport,
                binding,
                None,
                CreateTerminal::Failed(NativeViewportCreateFailureKind::WindowUnavailable),
                || panic!("a conflicting terminal cannot recapture create ingress"),
            ),
            CreateAdmission::Conflict
        );
        assert!(
            routes
                .reserve_retired_replacement(viewport, binding, successor)
                .is_err()
        );
    }
    // The receipt-state model transfers its real capture without fabricating
    // an opaque renderer output token for the Created payload.
    let ordinal = routes.create_ingress().next().unwrap().ordinal();
    journal.transfer_reserved(routes.take_create_ingress(ordinal).unwrap());
    let bridge = NativeHostBridge::new(routes);
    bridge.lock().journal = journal;
    bridge.commit_frame_boundary(true);
    assert!(bridge.with_viewports(|routes| routes.has_pending_create_receipts()));
    bridge.commit_frame_boundary(false);
    assert!(!bridge.with_viewports(|routes| routes.has_pending_create_receipts()));
    assert_eq!(
        bridge.with_viewports_mut(|routes| routes.receive_create_terminal(
            viewport,
            binding,
            None,
            CreateTerminal::Created(window),
            || panic!("a committed duplicate cannot recapture create ingress"),
        )),
        CreateAdmission::Duplicate
    );
    assert!(
        bridge.with_viewports_mut(|routes| { routes.suppress_pointer_route(viewport, binding) })
    );
    bridge
        .reserve_retired_viewport_replacement(viewport, binding, successor)
        .unwrap();
    assert_eq!(
        bridge.with_viewports_mut(|routes| routes.receive_create_terminal(
            viewport,
            binding,
            None,
            CreateTerminal::Created(window),
            || panic!("a successor route cannot accept the old terminal"),
        )),
        CreateAdmission::Conflict
    );
    assert_eq!(
        bridge.with_viewports(|routes| routes.binding(viewport)),
        Some(successor)
    );
}

use super::*;

fn bindings() -> (NativeSurfaceBinding, NativeSurfaceBinding) {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            SurfaceId::new(1),
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1)]),
            ),
        ),
        DockspaceSurfaceLayout::new(
            SurfaceId::new(2),
            DockspaceRootLayout::new(
                RootId::new(2),
                DockspaceNode::central_tabs([ItemId::new(2)]),
            ),
        ),
    ])
    .expect("test layout validates");
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default())
        .expect("test session initializes");
    session
        .enable_managed_native_host(dockspace::runtime::NativePointerRoster::Exact(Vec::new()))
        .expect("native host enrolls");
    session
        .register_native_root(
            SurfaceId::new(1),
            dockspace::runtime::HostWindowToken::new(1),
        )
        .expect("first root registers");
    session
        .register_native_root(
            SurfaceId::new(2),
            dockspace::runtime::HostWindowToken::new(2),
        )
        .expect("second root registers");
    let mut frame = session
        .begin_host_frame()
        .expect("registration frame begins");
    frame
        .complete_unpainted_surfaces(dockspace::runtime::SurfaceUnavailableReason::Deferred)
        .expect("registration surfaces settle");
    let report = frame.commit().expect("registration frame commits");
    let mut found = report.inputs().iter().filter_map(|input| match input {
        dockspace::runtime::HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
        _ => None,
    });
    (
        found.next().expect("first binding exists"),
        found.next().expect("second binding exists"),
    )
}

pub(super) fn workspace_root_layout(surface: SurfaceId) -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        surface,
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::central_tabs([ItemId::new(1)]),
        ),
    )])
    .unwrap()
}

pub(super) fn workspace_root_fixture() -> (DockspaceSession, NativeHostBridge, NativeSurfaceBinding)
{
    use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalRect, ScaleFactor};
    use dockspace::runtime::{
        HostInputOutcome, HostWindowToken, NativeCloseState, NativePointerRoster,
        NativeReceiverAnswer, NativeWindowInputState, NativeWindowPresentationState,
        NativeWorkAreaRoster, SurfacePresentationResult, SurfaceUnavailableReason,
        UniformSurfaceMetrics,
    };

    let mut session = DockspaceSession::from_layout(
        workspace_root_layout(SurfaceId::new(1)),
        DockPolicy::default(),
    )
    .unwrap();
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    session
        .configure_managed_native_capabilities(crate::capabilities::capabilities_for_backend(
            eframe::NativeWindowingBackend::Windows,
        ))
        .unwrap();
    session
        .register_native_root(SurfaceId::new(1), HostWindowToken::new(1))
        .unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let registration = frame.commit().unwrap();
    let predecessor = registration
        .inputs()
        .iter()
        .find_map(|input| match input {
            HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
            _ => None,
        })
        .unwrap();
    let bounds = PhysicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap();
    let scale = ScaleFactor::new(1.0).unwrap();
    let facts = NativeWindowFacts::live()
        .with_content_bounds(bounds)
        .with_outer_bounds(bounds)
        .with_native_scale_factor(scale)
        .with_presentation_scale_factor(scale)
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible)
        .with_close(NativeCloseState::Clear, None);
    session
        .report_managed_native_snapshot([(predecessor, facts)], NativeWorkAreaRoster::Unknown)
        .unwrap();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .measure_surface(
            predecessor.surface(),
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
                LogicalSize::new(32.0, 24.0).unwrap(),
                96.0,
            )
            .unwrap(),
        )
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();

    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let receipt = frame
        .paint_plan(predecessor.surface())
        .unwrap()
        .unwrap()
        .execute_surface_program(|_| Ok::<(), ()>(()))
        .unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    let output = frame
        .commit()
        .unwrap()
        .take_painted_outputs()
        .pop()
        .unwrap();
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();

    let bridge = NativeHostBridge::new(NativeViewportMap::default());
    bridge
        .bind_viewport_route(ViewportId::ROOT, WindowId::from(91), predecessor)
        .unwrap();
    bridge.with_viewports_mut(|routes| {
        routes.refresh_local_coordinates(&session);
        routes.set_focus_reportable(&[predecessor]);
        assert!(routes.block_semantic_viewport(ViewportId::ROOT));
        assert!(
            routes
                .local_coordinates_for_event(WindowId::from(91), None)
                .is_some()
        );
    });
    (session, bridge, predecessor)
}

#[test]
fn workspace_root_continuation_moves_the_route_and_preserves_only_physical_authority() {
    use dockspace::runtime::{NativeEffectOperation, SurfaceUnavailableReason};

    let (mut session, bridge, predecessor) = workspace_root_fixture();
    let reset = session
        .prepare_layout_reset(workspace_root_layout(SurfaceId::new(2)))
        .unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_native_layout_reset(&reset, predecessor, SurfaceId::new(2))
        .unwrap();
    let continuation = frame.proposed_native_root_continuations().next().unwrap();
    let successor = continuation.successor();
    assert_eq!(continuation.predecessor(), predecessor);
    assert_ne!(predecessor.surface(), successor.surface());
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut root_surface = predecessor.surface();
    let (mut report, _) = bridge
        .commit_host_frame_with_staging(frame, true, Some(&mut root_surface))
        .unwrap();

    assert_eq!(report.native_root_continuations(), &[continuation]);
    assert_eq!(root_surface, successor.surface());
    assert_eq!(session.native_local_coordinates(successor), None);
    bridge.with_viewports(|routes| {
        assert_eq!(routes.viewport(predecessor.surface()), None);
        assert_eq!(routes.viewport(successor.surface()), Some(ViewportId::ROOT));
        assert_eq!(routes.window(ViewportId::ROOT), Some(WindowId::from(91)));
        assert_eq!(
            routes.route_for_event(WindowId::from(91), None),
            Some((ViewportId::ROOT, successor))
        );
        assert!(routes.has_window_receipt(ViewportId::ROOT, successor));
        assert!(!routes.has_window_receipt(ViewportId::ROOT, predecessor));
        assert_eq!(
            routes.local_coordinates_for_event(WindowId::from(91), None),
            None
        );
        assert_eq!(
            routes.focus_binding_for_event(WindowId::from(91), None),
            None
        );
        assert!(routes.semantic_viewport_blocked(ViewportId::ROOT));
    });
    assert_eq!(
        bridge.workspace_replacement_predecessor(successor),
        Some((ViewportId::ROOT, predecessor))
    );
    assert!(bridge.retirement_is_quiescence_candidate(predecessor));
    assert!(
        !bridge
            .lock()
            .retirements
            .has_window(ViewportId::ROOT, predecessor)
    );
    assert!(bridge.retirement_destroyed_observations().is_empty());
    assert!(bridge.front_event().is_none());
    assert!(report.take_native_effects().iter().all(|effect| {
        !matches!(
            effect.operation(),
            NativeEffectOperation::CreateWindow { .. }
                | NativeEffectOperation::CompensatingClose { .. }
                | NativeEffectOperation::RequestReplacement { .. }
        )
    }));

    bridge.push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
        1,
        WindowId::from(91),
        Some(ViewportId::ROOT),
        None,
        WindowEvent::Destroyed,
    )));
    assert_eq!(bridge.front_event().unwrap().binding(), Some(successor));
    assert!(bridge.with_viewports(|routes| {
        routes.has_received_destroyed(ViewportId::ROOT, successor, WindowId::from(91), 1)
    }));
}

#[test]
fn workspace_root_continuation_preserves_bootstrap_and_rejects_invalid_publication() {
    use dockspace::runtime::{NativeReceiverAnswer, SurfaceUnavailableReason};

    for target_conflict in [true, false] {
        let (mut session, bridge, predecessor) = workspace_root_fixture();
        let before = session.version();
        let reset = session
            .prepare_layout_reset(workspace_root_layout(SurfaceId::new(2)))
            .unwrap();
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_native_layout_reset(&reset, predecessor, SurfaceId::new(2))
            .unwrap();
        let continuation = frame.proposed_native_root_continuations().next().unwrap();
        let successor = continuation.successor();
        let peer = ViewportId::from_hash_of("root-continuation-target-conflict");
        let peer_binding = bindings().1;
        let mut root_surface = if target_conflict {
            bridge
                .bind_viewport_route(peer, WindowId::from(92), peer_binding)
                .unwrap();
            assert_eq!(peer_binding.surface(), successor.surface());
            predecessor.surface()
        } else {
            SurfaceId::new(99)
        };
        let previous_root = root_surface;
        let resource_count = bridge.lock().window_resources.bindings().count();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        assert!(
            bridge
                .commit_host_frame_with_staging(frame, true, Some(&mut root_surface))
                .is_err()
        );

        assert_eq!(session.version(), before);
        assert_eq!(root_surface, previous_root);
        assert_eq!(bridge.workspace_replacement_predecessor(successor), None);
        assert_eq!(
            bridge.lock().window_resources.bindings().count(),
            resource_count
        );
        assert!(!bridge.lock().retirements.has_pending_work());
        bridge.with_viewports(|routes| {
            assert_eq!(routes.binding(ViewportId::ROOT), Some(predecessor));
            assert_eq!(routes.window(ViewportId::ROOT), Some(WindowId::from(91)));
            assert!(routes.has_window_receipt(ViewportId::ROOT, predecessor));
            assert!(!routes.has_window_receipt(ViewportId::ROOT, successor));
            assert!(
                routes
                    .local_coordinates_for_event(WindowId::from(91), None)
                    .is_some()
            );
            assert_eq!(
                routes.focus_binding_for_event(WindowId::from(91), None),
                Some(predecessor.window())
            );
            if target_conflict {
                assert_eq!(routes.binding(peer), Some(peer_binding));
                assert_eq!(routes.window(peer), Some(WindowId::from(92)));
            }
        });
    }

    // Initial registration may replace a same-surface Root before its first
    // physical route is attached. Both commit entry points keep that path.
    for publish_root in [false, true] {
        let (mut session, _, predecessor) = workspace_root_fixture();
        let bridge = NativeHostBridge::new(NativeViewportMap::default());
        let reset = session
            .prepare_layout_reset(workspace_root_layout(predecessor.surface()))
            .unwrap();
        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        frame.submit_layout_reset(&reset).unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let mut root_surface = predecessor.surface();
        let (report, _) = bridge
            .commit_host_frame_with_staging(frame, true, publish_root.then_some(&mut root_surface))
            .unwrap();
        assert_eq!(report.native_root_continuations().len(), 1);
        assert_eq!(root_surface, predecessor.surface());
        assert_eq!(
            bridge.with_viewports(|routes| routes.binding(ViewportId::ROOT)),
            None
        );
    }
}

#[test]
fn output_reservation_keeps_the_binding_seen_at_begin() {
    let (first, second) = bindings();
    let mut reservation = OutputReservation::bound_for_test(first);

    assert!(!reservation.attach(second));
    assert_eq!(reservation.binding(), Some(first));
}

#[test]
fn reserved_pointer_callback_is_once_only_across_raw_handoff_partial_commit_and_retry() {
    let (binding, _) = bindings();
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(11);
    let context = eframe::egui::Context::default();
    let first = eframe::queue_native_viewport_pointer_passthrough(&context, viewport, true);
    let next = eframe::queue_native_viewport_pointer_passthrough(&context, viewport, false);
    let bridge = NativeHostBridge::new(NativeViewportMap::default());
    let record = NativePointerPassthroughRecord::for_test(
        first,
        viewport,
        window,
        Some(binding),
        true,
        eframe::NativeViewportPointerPassthroughStatus::Applied,
    );
    {
        let mut records = bridge.lock();
        assert!(
            records
                .window_resources
                .reserve_pointer_callback(viewport, binding, window, first, true)
        );
        assert!(records.record_viewport_pointer_passthrough(record));
        assert!(!records.record_viewport_pointer_passthrough(record));
        assert_eq!(records.journal.len(), 0);
        assert_eq!(records.window_resources.callback_ingress().count(), 1);
        assert_eq!(records.retained_record_count(), 1);
    }
    bridge.commit_frame_boundary(false);
    assert!(bridge.lock().window_resources.has_pending_work());
    assert!(bridge.acknowledge_viewport_pointer_passthrough(record));
    assert!(!bridge.lock().record_viewport_pointer_passthrough(record));
    bridge.commit_frame_boundary(true);
    assert!(
        !bridge
            .lock()
            .window_resources
            .reserve_pointer_callback(viewport, binding, window, next, false)
    );
    assert!(bridge.callback_boundary_pending());
    bridge.commit_frame_boundary(false);
    assert!(!bridge.lock().window_resources.has_pending_work());
    assert!(!bridge.lock().record_viewport_pointer_passthrough(record));
    assert!(
        bridge
            .lock()
            .window_resources
            .reserve_pointer_callback(viewport, binding, window, next, false)
    );
    assert!(bridge.lock().window_resources.has_pending_work());
}

#[test]
fn durable_output_wake_is_idempotent_after_the_terminal_when_prearmed() {
    let (binding, _) = bindings();
    let mut prearmed = OutputReservation::bound_for_test(binding);
    assert!(prearmed.require_wake_after_current());
    prearmed.terminal_recorded = true;
    assert!(prearmed.require_wake_after_current());

    let mut too_late = OutputReservation::bound_for_test(binding);
    too_late.terminal_recorded = true;
    assert!(!too_late.require_wake_after_current());
}

#[test]
fn silent_output_abandonment_does_not_request_a_terminal_wake() {
    let mut reservation = OutputReservation::unbound_for_test();

    assert!(reservation.abandon_silently());
    assert!(!reservation.wakes_on_terminal());
    assert!(!reservation.abandon_silently());
}

#[test]
fn repeated_output_settlement_only_upgrades_the_terminal_wake() {
    let (binding, _) = bindings();
    let mut silent = OutputReservation::unbound_for_test();
    assert_eq!(
        silent.settle_abandonment(false),
        OutputAbandonment::NewlyAbandoned
    );
    assert_eq!(
        silent.settle_abandonment(true),
        OutputAbandonment::AlreadyAbandonedAwaitingTerminal
    );
    assert!(silent.wakes_on_terminal());

    let mut terminal_create = OutputReservation::bound_for_test(binding);
    assert!(terminal_create.abandon());
    assert_eq!(
        terminal_create.settle_abandonment(false),
        OutputAbandonment::AlreadyAbandonedAwaitingTerminal
    );
    assert!(terminal_create.wakes_on_terminal());
}

#[test]
fn output_ordinals_must_form_one_contiguous_sequence() {
    assert!(is_next_output_ordinal(0, 1));
    assert!(is_next_output_ordinal(1, 2));
    assert!(!is_next_output_ordinal(1, 1));
    assert!(!is_next_output_ordinal(1, 3));
    assert!(!is_next_output_ordinal(u64::MAX, 1));
}

#[test]
fn native_runtime_host_attachment_is_one_shot() {
    let mut attachment = HostAttachmentState::Available;

    assert!(attachment.try_attach());
    assert_eq!(attachment, HostAttachmentState::Attached);
    assert!(!attachment.try_attach());

    attachment.detach();
    assert_eq!(attachment, HostAttachmentState::Retired);
    assert!(!attachment.try_attach());
}

#[test]
fn deferred_semantic_prelude_waits_for_an_empty_journal_and_committed_input() {
    let child = ViewportId::from_hash_of("semantic-prelude-child");
    let cases = [
        ("empty", Vec::new(), true),
        ("root output", vec![Some(ViewportId::ROOT)], false),
        (
            "root outputs",
            vec![Some(ViewportId::ROOT), Some(ViewportId::ROOT)],
            false,
        ),
        ("child output", vec![Some(child)], false),
        (
            "root then child output",
            vec![Some(ViewportId::ROOT), Some(child)],
            false,
        ),
        (
            "root then callback",
            vec![Some(ViewportId::ROOT), None],
            false,
        ),
    ];

    for (name, records, expected) in cases {
        assert_eq!(
            deferred_semantic_prelude_ready(records.into_iter()),
            expected,
            "{name}"
        );
    }

    let mut records = NativeHostState::active();
    records.event_boundary_pending = true;
    assert!(!records.semantic_prelude_ready());
    records.event_boundary_pending = false;
    assert!(records.semantic_prelude_ready());
}

#[test]
fn deferred_resource_owner_rejects_foreign_cleanup_and_prunes_only_after_last_obligation() {
    let (binding, foreign) = bindings();
    let viewport = ViewportId::from_hash_of("deferred-resource-owner");
    let rect = NativePhysicalRect::new(10, 20, 800, 600);
    let mut resources = NativeWindowResources::default();
    assert!(resources.reserve_create(viewport, binding, rect));
    assert!(resources.reserve_create(viewport, binding, rect));
    assert!(!resources.reserve_create(viewport, foreign, rect));
    assert!(resources.references_binding(binding));
    assert!(!resources.clear_create(viewport, foreign));
    assert!(!resources.clear_hidden(viewport, foreign));
    resources.retire(viewport, foreign);
    assert_eq!(
        resources.create(viewport),
        Some(NativeCreateReservation { binding, rect })
    );
    assert_eq!(resources.hidden_binding(viewport), Some(binding));

    assert!(resources.clear_create(viewport, binding));
    assert!(resources.has_pending_work());
    assert_eq!(resources.hidden_binding(viewport), Some(binding));
    assert!(resources.clear_hidden(viewport, binding));
    assert!(!resources.has_pending_work());
    assert!(!resources.references_binding(binding));
    assert!(resources.reserve_create(viewport, foreign, rect));
    resources.retire(viewport, binding);
    assert_eq!(resources.hidden_binding(viewport), Some(foreign));
    resources.retire(viewport, foreign);
    assert!(!resources.has_pending_work());
}

#[test]
fn active_mailbox_accepts_only_events_consumed_by_dockspace() {
    let moved = WindowEvent::CursorMoved {
        device_id: DeviceId::dummy(),
        position: PhysicalPosition::new(10.0, 20.0),
        facts: PointerEventFacts::default(),
    };
    let records = NativeHostState::active();

    assert!(records.accepts_window_event(&moved));
    assert!(records.accepts_window_event(&WindowEvent::CloseRequested));
    assert!(records.accepts_window_event(&WindowEvent::Destroyed));
    assert!(!records.accepts_window_event(&WindowEvent::Focused(true)));
}

#[test]
fn idle_cursor_prefix_keeps_only_the_latest_same_lane_observation() {
    let (binding, _) = bindings();
    let window = WindowId::from(11);
    let mut records = NativeHostState::active();
    for ordinal in 1..=128 {
        records.record_window_event(NativeWindowEventRecord::for_test(
            ordinal,
            window,
            Some(ViewportId::ROOT),
            Some(binding),
            WindowEvent::CursorMoved {
                device_id: DeviceId::dummy(),
                position: PhysicalPosition::new(ordinal as f64, 20.0),
                facts: PointerEventFacts::default(),
            },
        ));
    }

    assert!(
        records.coalesce_idle_cursor_prefix(&mut NativeInputBudget::new(
            &NativeMonotonicClock::new(|| std::time::Duration::ZERO),
        ))
    );
    assert_eq!(records.journal.len(), 1);
    assert!(matches!(
        records.journal.front(),
        Some(HostRecord::WindowEvent(event)) if event.ordinal() == 128
    ));
}

#[test]
fn idle_cursor_prefix_stops_at_outputs_and_other_callbacks() {
    use IdleCursorPrefixEntry::{Boundary, Cursor, Output};

    assert_eq!(
        idle_cursor_prefix_latest_index(
            [Cursor, Output, Cursor, Output, Cursor, Output].into_iter()
        ),
        None
    );
    assert_eq!(
        idle_cursor_prefix_latest_index(
            [Cursor, Cursor, Output, Cursor, Boundary, Output, Cursor].into_iter()
        ),
        Some(1)
    );
    assert_eq!(
        idle_cursor_prefix_latest_index([Cursor, Output, Output].into_iter()),
        None
    );
}

#[test]
fn blocked_child_repaints_only_after_the_ordered_boundary_is_clear() {
    let (binding, _) = bindings();
    let child = ViewportId::from_hash_of("blocked-semantic-child");
    let mut records = NativeHostState::active();
    records.viewports.reserve(child, binding).unwrap();
    records.record_window_event(NativeWindowEventRecord::for_test(
        1,
        WindowId::from(22),
        Some(child),
        Some(binding),
        WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(10.0, 20.0),
            facts: PointerEventFacts::default(),
        },
    ));
    records.block_semantic_viewport(child);

    records.release_semantic_viewports_if_ready();
    assert!(records.take_ready_semantic_viewports().is_empty());

    records.journal.pop_front();
    records.release_semantic_viewports_if_ready();
    assert_eq!(records.take_ready_semantic_viewports(), vec![child]);
}

#[test]
fn claimed_native_suffix_cannot_release_a_blocked_child_with_an_empty_raw_journal() {
    let mut records = NativeHostState::active();
    let child = ViewportId::from_hash_of("claimed-native-child");
    let (binding, _) = bindings();
    records.viewports.reserve(child, binding).unwrap();
    records.block_semantic_viewport(child);
    records.event_boundary_pending = true;
    assert!(records.journal.is_empty());

    assert!(!records.semantic_prelude_ready());
    records.release_semantic_viewports_if_ready();
    assert!(records.take_ready_semantic_viewports().is_empty());
    assert!(records.viewports.semantic_viewport_blocked(child));

    records.event_boundary_pending = false;
    records.release_semantic_viewports_if_ready();
    assert_eq!(records.take_ready_semantic_viewports(), vec![child]);
}

#[test]
fn callback_age_and_admission_reservation_survive_handoff_partial_commit_and_retry() {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    let now = Arc::new(AtomicU64::new(0));
    let source = Arc::clone(&now);
    let bridge = NativeHostBridge::with_clock(
        NativeViewportMap::default(),
        NativeMonotonicClock::new(move || Duration::from_millis(source.load(Ordering::SeqCst))),
    );
    bridge.push_record(HostRecord::DeviceRemoval(
        NativeDeviceRemovalRecord::for_test(1),
    ));
    now.store(25, Ordering::SeqCst);
    assert!(bridge.acknowledge_device_removal(NativeDeviceRemovalRecord::for_test(1)));
    assert!(!bridge.has_pending_callback_records());
    assert!(bridge.has_pending_coordinator_work());
    assert_eq!(bridge.lock().journal.retained_record_count(), 1);
    bridge.commit_frame_boundary(true);
    now.store(250, Ordering::SeqCst);
    assert_eq!(
        bridge.lock().journal.oldest_age(),
        Some(Duration::from_millis(250))
    );
    assert_eq!(bridge.lock().journal.retained_record_count(), 1);
    assert!(bridge.callback_boundary_pending());

    // A yielded or dropped attempt has no mailbox commit call at all.
    assert_eq!(bridge.lock().journal.front_ordinal(), None);
    assert_eq!(bridge.lock().journal.captured_through(), 1);
    bridge.commit_frame_boundary(false);
    assert_eq!(bridge.lock().journal.retained_record_count(), 0);
    assert_eq!(bridge.lock().journal.oldest_age(), None);
    assert!(!bridge.has_pending_coordinator_work());
}

#[test]
fn cursor_claim_drop_and_idle_coalescing_preserve_original_capture_identity_and_age() {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    let (binding, _) = bindings();
    let now = Arc::new(AtomicU64::new(0));
    let source = Arc::clone(&now);
    let bridge = NativeHostBridge::with_clock(
        NativeViewportMap::default(),
        NativeMonotonicClock::new(move || Duration::from_millis(source.load(Ordering::SeqCst))),
    );
    for ordinal in 1..=3 {
        now.store((ordinal - 1) * 10, Ordering::SeqCst);
        bridge.push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            ordinal,
            WindowId::from(100),
            Some(ViewportId::ROOT),
            Some(binding),
            WindowEvent::CursorMoved {
                device_id: DeviceId::dummy(),
                position: PhysicalPosition::new(ordinal as f64, 20.0),
                facts: PointerEventFacts::default(),
            },
        )));
    }
    now.store(50, Ordering::SeqCst);
    {
        let claim = bridge.claim_cursor_prefix(256).unwrap();
        assert_eq!(claim.record_count().get(), 3);
    }
    assert_eq!(bridge.lock().journal.front_ordinal(), Some(1));
    assert_eq!(bridge.lock().journal.retained_record_count(), 3);
    assert_eq!(
        bridge.lock().journal.oldest_age(),
        Some(Duration::from_millis(50))
    );

    let mut budget = NativeInputBudget::new(&NativeMonotonicClock::new(|| Duration::ZERO));
    assert!(bridge.coalesce_idle_cursor_prefix(&mut budget));
    assert_eq!(bridge.lock().journal.front_ordinal(), Some(3));
    assert_eq!(bridge.lock().journal.captured_through(), 3);
    assert_eq!(bridge.lock().journal.retained_record_count(), 1);
    assert_eq!(
        bridge.lock().journal.oldest_age(),
        Some(Duration::from_millis(30))
    );
    assert_eq!(budget.remaining_records(), 254);
    assert_eq!(bridge.front_event().unwrap().ordinal(), 3);
}

#[test]
fn identical_viewport_rosters_wait_for_a_new_fact_context() {
    let roster =
        NativeViewportRosterRecord::for_test(std::iter::empty::<CompiledWindowObservation>(), true);
    let mut records = NativeHostState::active();

    assert!(records.record_viewport_roster(roster.clone()));
    assert!(!records.record_viewport_roster(roster.clone()));

    let first = records
        .front_viewport_roster()
        .expect("the queued roster freezes its enqueue context");
    assert!(records.acknowledge_viewport_roster(&first));
    assert!(!records.record_viewport_roster(roster.clone()));
    assert!(records.settle_viewport_roster(&first, false));

    assert!(records.record_viewport_roster(roster.clone()));
    let retry = records
        .front_viewport_roster()
        .expect("a rejected roster remains retryable");
    assert!(records.acknowledge_viewport_roster(&retry));
    assert!(records.settle_viewport_roster(&retry, true));
    assert!(!records.record_viewport_roster(roster.clone()));

    records.invalidate_viewport_roster();
    assert!(records.record_viewport_roster(roster.clone()));

    let mut queued = NativeHostState::active();
    assert!(queued.record_viewport_roster(roster.clone()));
    queued.invalidate_viewport_roster();
    assert!(queued.record_viewport_roster(roster));
    assert_eq!(
        queued
            .journal
            .iter()
            .filter(|record| matches!(record, HostRecord::ViewportRoster(_)))
            .count(),
        2
    );
}

#[test]
fn queued_inflight_and_committed_roster_share_one_immutable_owned_snapshot() {
    let roster =
        NativeViewportRosterRecord::for_test(std::iter::empty::<CompiledWindowObservation>(), true);
    let mut records = NativeHostState::active();
    assert!(records.record_viewport_roster(roster));
    let queued = records.front_viewport_roster().unwrap();
    let retry = records.front_viewport_roster().unwrap();
    assert!(
        std::ptr::eq(queued.roster(), retry.roster()),
        "reading an exact roster must not allocate another owned observation vector"
    );
    assert!(records.acknowledge_viewport_roster(&queued));
    assert!(std::ptr::eq(
        queued.roster(),
        records
            .in_flight_viewport_roster
            .as_ref()
            .unwrap()
            .roster
            .as_ref()
    ));
    assert!(records.settle_viewport_roster(&queued, true));
    assert!(std::ptr::eq(
        queued.roster(),
        records
            .committed_viewport_roster
            .as_ref()
            .unwrap()
            .roster
            .as_ref()
    ));
    assert_eq!(
        retry, queued,
        "a retained immutable receipt survives its owner's settlement"
    );
}

#[test]
fn active_destroyed_does_not_leave_a_shutdown_roster_obligation() {
    let (binding, _) = bindings();
    let mut records = NativeHostState::active();
    records.record_window_event(NativeWindowEventRecord::for_test(
        1,
        WindowId::from(11),
        Some(ViewportId::ROOT),
        Some(binding),
        WindowEvent::Destroyed,
    ));

    assert!(!records.terminal_roster.has_pending());
    assert!(records.journal.pop_front().is_some());
    assert!(!records.has_pending_coordinator_work());

    records.quarantine_after_fatal();
    records.record_window_event(NativeWindowEventRecord::for_test(
        2,
        WindowId::from(11),
        Some(ViewportId::ROOT),
        Some(binding),
        WindowEvent::Destroyed,
    ));
    assert!(records.terminal_roster.has_pending());
}

#[test]
fn quarantine_preserves_an_existing_visibility_roster_obligation() {
    let (binding, _) = bindings();
    let mut records = NativeHostState::active();
    assert!(
        records.record_viewport_visibility(NativeViewportVisibilityRecord::for_test(
            eframe::queue_native_viewport_visibility(
                &eframe::egui::Context::default(),
                ViewportId::ROOT,
                true
            ),
            ViewportId::ROOT,
            WindowId::from(11),
            binding,
            true,
            eframe::NativeViewportVisibilityStatus::Dispatched,
        ))
    );
    assert!(!records.terminal_roster.has_pending());

    records.quarantine_after_fatal();

    assert!(records.terminal_roster.has_pending());
}

#[test]
fn rejected_terminal_roster_is_rearmed_until_applied() {
    let bridge = NativeHostBridge::new(NativeViewportMap::default());
    bridge.quarantine_after_fatal();
    bridge.require_terminal_roster();
    let roster = NativeViewportRosterRecord::for_test([], true);

    bridge.push_terminal_viewport_roster(roster.clone());
    assert_eq!(bridge.lock().journal.len(), 0);
    assert!(bridge.lock().terminal_roster.has_pending());
    let rejected = bridge
        .front_viewport_roster()
        .expect("the terminal roster enters its reserved slot");
    assert!(bridge.acknowledge_viewport_roster(&rejected));
    assert!(bridge.settle_viewport_roster(&rejected, false));
    assert!(bridge.lock().terminal_roster.has_pending());
    assert!(
        !bridge
            .lock()
            .record_terminal_viewport_roster(roster.clone())
    );
    bridge.commit_frame_boundary(true);
    assert!(
        !bridge
            .lock()
            .record_terminal_viewport_roster(roster.clone())
    );
    bridge.commit_frame_boundary(false);

    bridge.push_terminal_viewport_roster(roster);
    let applied = bridge
        .front_viewport_roster()
        .expect("the rejected terminal roster remains retryable");
    assert!(bridge.acknowledge_viewport_roster(&applied));
    bridge.commit_frame_boundary(false);
    assert!(bridge.lock().terminal_roster.has_pending());
    assert!(bridge.settle_viewport_roster(&applied, true));
    assert!(!bridge.lock().terminal_roster.has_pending());
}

#[test]
fn fresh_roster_state_is_clean_until_facts_are_invalidated() {
    let mut records = NativeHostState::active();

    assert!(!records.viewport_roster_dirty());
    records.invalidate_viewport_roster();
    assert!(records.viewport_roster_dirty());
}

#[test]
fn newer_terminal_roster_obligation_survives_an_older_settlement() {
    let bridge = NativeHostBridge::new(NativeViewportMap::default());
    bridge.quarantine_after_fatal();
    bridge.require_terminal_roster();
    let roster = NativeViewportRosterRecord::for_test([], true);

    bridge.push_terminal_viewport_roster(roster.clone());
    let first = bridge
        .front_viewport_roster()
        .expect("the first terminal roster enters its reserved slot");
    assert!(bridge.acknowledge_viewport_roster(&first));

    bridge.require_terminal_roster();
    for _ in 0..32 {
        assert!(
            !bridge
                .lock()
                .record_terminal_viewport_roster(roster.clone())
        );
    }
    assert_eq!(bridge.lock().journal.captured_through(), 1);
    assert!(bridge.settle_viewport_roster(&first, true));
    assert!(bridge.lock().terminal_roster.has_pending());
    bridge.commit_frame_boundary(true);
    assert!(!bridge.lock().terminal_roster.needs_capture());
    bridge.commit_frame_boundary(false);
    assert!(bridge.lock().terminal_roster.needs_capture());

    bridge.push_terminal_viewport_roster(roster);
    let second = bridge
        .front_viewport_roster()
        .expect("the newer obligation reuses the released slot");
    assert!(bridge.acknowledge_viewport_roster(&second));
    assert!(bridge.settle_viewport_roster(&second, true));
    assert!(bridge.lock().terminal_roster.has_pending());
    bridge.commit_frame_boundary(false);
    assert!(!bridge.lock().terminal_roster.has_pending());
}

#[test]
fn freeze_preserves_owned_mailbox_prefix_and_rejects_new_callbacks() {
    let (binding, _) = bindings();
    let viewport = ViewportId::ROOT;
    let mut viewports = NativeViewportMap::default();
    viewports.reserve(viewport, binding).unwrap();
    assert!(viewports.admit_test_create(viewport, binding));
    let rect = NativePhysicalRect::new(10, 20, 800, 600);
    let pending = NativeWindowEventRecord::for_test(
        1,
        WindowId::from(11),
        Some(viewport),
        Some(binding),
        WindowEvent::Focused(true),
    );
    let mut records = NativeHostState::active();
    records.viewports = viewports;
    records
        .journal
        .push_back(HostRecord::WindowEvent(pending.clone()));
    assert!(
        records
            .window_resources
            .reserve_create(viewport, binding, rect)
    );

    records.freeze();

    assert_eq!(records.mode, HostIngressMode::Frozen);
    assert!(matches!(
        records.journal.front(),
        Some(HostRecord::WindowEvent(event)) if *event == pending
    ));
    assert_eq!(
        records.window_resources.create(viewport),
        Some(NativeCreateReservation { binding, rect })
    );
    assert_eq!(
        records.window_resources.hidden_binding(viewport),
        Some(binding)
    );
    let accepted = records.record_viewport_create_failure(NativeViewportCreateFailureRecord::new(
        viewport,
        binding,
        NativeViewportCreateFailureKind::WindowUnavailable,
    ));
    assert!(!accepted);
}

#[test]
fn quarantine_accepts_terminal_callbacks_and_close_requests() {
    let (binding, _) = bindings();
    let viewport = ViewportId::ROOT;
    let mut viewports = NativeViewportMap::default();
    viewports.reserve(viewport, binding).unwrap();
    assert!(viewports.admit_test_create(viewport, binding));
    let window = WindowId::from(11);
    let mut records = NativeHostState::active();
    records.viewports = viewports;
    records.block_semantic_viewport(viewport);
    records.viewports.release_blocked_semantic_viewports();
    let context = eframe::egui::Context::default();
    let visibility_token = eframe::queue_native_viewport_visibility(&context, viewport, false);
    let input_token = eframe::queue_native_viewport_pointer_passthrough(&context, viewport, false);
    assert!(records.window_resources.reserve_visibility_callback(
        viewport,
        binding,
        window,
        visibility_token,
        false,
    ));
    assert!(records.window_resources.reserve_pointer_callback(
        viewport,
        binding,
        window,
        input_token,
        false,
    ));

    assert!(records.quarantine_after_fatal().is_empty());

    assert_eq!(records.mode, HostIngressMode::Quarantined);
    assert!(records.accepts_no_frame_invalidation());
    assert!(!records.viewports.has_semantic_wake());
    assert!(records.accepts_window_event(&WindowEvent::CloseRequested));
    assert!(records.accepts_window_event(&WindowEvent::Destroyed));
    assert!(!records.accepts_window_event(&WindowEvent::Focused(true)));
    assert!(
        records.record_viewport_create_failure(NativeViewportCreateFailureRecord::new(
            viewport,
            binding,
            NativeViewportCreateFailureKind::WindowUnavailable,
        ),)
    );
    assert!(
        records.record_viewport_visibility(NativeViewportVisibilityRecord::for_test(
            visibility_token,
            viewport,
            window,
            binding,
            false,
            eframe::NativeViewportVisibilityStatus::Dispatched,
        ))
    );
    assert!(
        records.record_viewport_pointer_passthrough(NativePointerPassthroughRecord::for_test(
            input_token,
            viewport,
            window,
            Some(binding),
            false,
            eframe::NativeViewportPointerPassthroughStatus::Applied,
        ))
    );
    assert!(records.terminal_roster.has_pending());

    records.freeze();

    assert_eq!(records.mode, HostIngressMode::Frozen);
    assert!(!records.accepts_no_frame_invalidation());
    assert!(!records.accepts_window_event(&WindowEvent::Destroyed));
    assert!(
        !records.record_viewport_create_failure(NativeViewportCreateFailureRecord::new(
            viewport,
            binding,
            NativeViewportCreateFailureKind::WindowUnavailable,
        ),)
    );
}

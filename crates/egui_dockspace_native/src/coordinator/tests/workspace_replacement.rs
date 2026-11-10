//! Workspace replacement uses the existing exact native route and retirement owners.

use std::num::NonZeroUsize;

use dockspace::runtime::{
    NativeDesktopPointerLocation, NativeDesktopPosition, NativeInputStorageLimit,
    NativePointerEvent, NativePointerHover, NativePointerId, NativePointerOwner,
    NativeProducerStorageLimit, NativeReportStorageLimit,
};

use super::*;
use crate::focus_control::{NativeGlobalFocusRecord, NativeViewportFocusRecord};

fn layout() -> DockspaceLayout {
    DockspaceLayout::new([FIRST_SURFACE, SECOND_SURFACE].map(|surface| {
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

fn commit(native: &mut NativeCoordinator) -> HostFrameReport {
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    native.settle_host_frame_report(&report).unwrap();
    report
}

fn reset(native: &mut NativeCoordinator) -> HostFrameReport {
    let reset = native.session.prepare_layout_reset(layout()).unwrap();
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.frame.submit_layout_reset(&reset).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn successor(report: &HostFrameReport, predecessor: NativeSurfaceBinding) -> NativeSurfaceBinding {
    let successor = report
        .native_bindings()
        .iter()
        .copied()
        .find(|binding| binding.surface() == predecessor.surface())
        .unwrap();
    assert_ne!(successor, predecessor);
    assert_eq!(successor.window_token(), predecessor.window_token());
    assert!(!successor.same_window_lifetime(predecessor));
    successor
}

#[test]
fn reset_rebinds_every_retained_root_on_its_current_physical_window() {
    let (mut native, first, second) = presented_local_coordinate_routes();
    let peer = native.surface_viewport(SECOND_SURFACE).unwrap();
    native.bridge.with_viewports_mut(|routes| {
        routes.set_focus_reportable(&[first, second]);
    });
    let old_coordinates = native.session.native_local_coordinates(first).unwrap();
    let old_focus = native.bridge.with_viewports(|routes| {
        NativeGlobalFocusRecord::capture_for_test(
            1,
            eframe::NativeGlobalFocus::Viewport {
                viewport_id: ViewportId::ROOT,
                window_id: WindowId::from(11),
            },
            routes,
        )
    });
    assert_eq!(old_focus.focus(), NativeGlobalFocus::Dock(first));

    let report = reset(&mut native);
    let next_first = successor(&report, first);
    let next_second = successor(&report, second);
    assert!(native.settle_host_frame_report(&report).unwrap());
    for (viewport, window, old, next) in [
        (ViewportId::ROOT, WindowId::from(11), first, next_first),
        (peer, WindowId::from(22), second, next_second),
    ] {
        assert_eq!(native.viewport_binding(viewport), Some(next));
        native.bridge.with_viewports(|routes| {
            assert_eq!(routes.window(viewport), Some(window));
            assert!(routes.has_window_receipt(viewport, next));
            assert!(!routes.has_window_receipt(viewport, old));
            assert_eq!(routes.local_coordinates_for_event(window, None), None);
            assert_eq!(routes.focus_binding_for_event(window, None), None);
        });
        assert!(!native.bridge.references_binding(old));
        assert!(native.bridge.references_binding(next));
    }
    assert_ne!(
        native.session.native_local_coordinates(next_first),
        Some(old_coordinates)
    );
    assert_eq!(old_focus.focus(), NativeGlobalFocus::Dock(first));
    assert!(native.bridge.viewport_roster_dirty());
    assert!(native.bridge.retirement_destroyed_observations().is_empty());
    assert!(native.prepare_committed_retirements().unwrap().is_none());
    assert!(!native.settle_workspace_replacement(&report).unwrap());

    native
        .bridge
        .push_viewport_roster(NativeViewportRosterRecord::for_test(
            [(next_first, 0.0), (next_second, 720.0)].map(|(binding, x)| {
                CompiledWindowObservation::new(binding, ready_window_facts(x))
                    .with_focus_reportable_for_test()
            }),
            true,
        ));
    assert!(native.reduce_next_viewport_roster().unwrap());
    commit(&mut native);
    let presented = paint_and_present_all_surfaces(&mut native);
    native.settle_host_frame_report(&presented).unwrap();
    assert!(native.session.presented_surface(FIRST_SURFACE).is_some());
    assert!(
        native
            .session
            .native_local_coordinates(next_first)
            .is_some()
    );
    native.bridge.with_viewports(|routes| {
        assert_eq!(
            routes.focus_binding_for_event(WindowId::from(11), None),
            Some(next_first)
        );
        assert_eq!(
            routes.local_coordinates_for_event(WindowId::from(11), None),
            native.session.native_local_coordinates(next_first)
        );
    });

    let newer = reset(&mut native);
    let newest = successor(&newer, next_first);
    native.settle_host_frame_report(&newer).unwrap();
    assert!(native.settle_workspace_replacement(&report).is_err());
    assert_eq!(native.viewport_binding(ViewportId::ROOT), Some(newest));
}

#[test]
fn snapshot_prefix_acknowledges_its_exact_envelope_before_rebinding_roots() {
    let (mut native, first, second) = presented_local_coordinate_routes();
    let unrelated = commit(&mut native);
    native
        .bridge
        .push_viewport_roster(live_roster([first, second]));
    assert!(native.reduce_next_viewport_roster().unwrap());
    let envelope = native.queued_snapshot.as_ref().unwrap().roster.clone();
    let report = reset(&mut native);
    assert!(matches!(
        report.inputs().last(),
        Some(HostInputOutcome::LayoutReset { .. })
    ));
    assert!(report.inputs().iter().any(|input| matches!(
        input,
        HostInputOutcome::NativePlatformSnapshotApplied { .. }
    )));

    assert!(native.settle_host_frame_report(&unrelated).is_err());
    assert!(native.settle_workspace_replacement(&report).is_err());
    assert_eq!(
        native.viewport_binding(ViewportId::ROOT),
        Some(successor(&report, first))
    );
    assert_eq!(
        native
            .bridge
            .workspace_replacement_predecessor(successor(&report, first)),
        Some((ViewportId::ROOT, first))
    );
    assert_eq!(native.queued_snapshot.as_ref().unwrap().roster, envelope);
    assert_eq!(native.queued_snapshot.as_ref().unwrap().settlement, None);

    assert!(native.settle_host_frame_report(&report).unwrap());
    assert!(native.queued_snapshot.is_none());
    assert_eq!(
        native.viewport_binding(ViewportId::ROOT),
        Some(successor(&report, first))
    );
    assert!(!native.settle_host_frame_report(&report).unwrap());
}

#[test]
fn snapshot_ack_from_an_earlier_partial_prefix_settles_before_the_final_reset() {
    let (mut native, first, second) = presented_local_coordinate_routes();
    native
        .bridge
        .push_viewport_roster(live_roster([first, second]));
    assert!(native.reduce_next_viewport_roster().unwrap());
    let envelope = native.queued_snapshot.as_ref().unwrap().roster.clone();
    native
        .session
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
    let mut frame = native
        .poll_host_frame(&mut test_budget(), |_| NativeReceiverAnswer::Unknown)
        .unwrap()
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let prefix = frame.commit().unwrap();
    assert!(prefix.native_input_pending());
    assert!(prefix.inputs().iter().any(|input| matches!(
        input,
        HostInputOutcome::NativePlatformSnapshotApplied { .. }
    )));
    assert!(!native.settle_host_frame_report(&prefix).unwrap());
    assert_eq!(native.queued_snapshot.as_ref().unwrap().roster, envelope);
    assert_eq!(
        native.queued_snapshot.as_ref().unwrap().settlement,
        Some(true)
    );
    assert_eq!(native.viewport_binding(ViewportId::ROOT), Some(first));

    let report = reset(&mut native);
    assert!(!report.native_input_pending());
    assert!(!report.inputs().iter().any(|input| matches!(
        input,
        HostInputOutcome::NativePlatformSnapshotApplied { .. }
    )));
    assert!(native.settle_host_frame_report(&report).unwrap());
    assert!(native.queued_snapshot.is_none());
    assert_eq!(
        native.viewport_binding(ViewportId::ROOT),
        Some(successor(&report, first))
    );
}

#[test]
fn reset_retires_owned_children_through_the_existing_effect_and_destroyed_snapshot() {
    let mut native = coordinator();
    let (root, child) = register_root_and_child(&mut native);
    let viewport = ViewportId::from_hash_of("workspace-reset-owned-child");
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(11), root)
        .unwrap();
    native
        .bind_viewport(viewport, WindowId::from(22), child)
        .unwrap();
    let mut report = reset(&mut native);
    let next_root = successor(&report, root);
    assert_eq!(report.native_bindings(), [next_root]);
    assert!(native.settle_host_frame_report(&report).unwrap());
    assert_eq!(native.viewport_binding(viewport), Some(child));
    let effects = report.take_native_effects();
    assert!(effects.iter().any(|request| matches!(
        request.operation(),
        NativeEffectOperation::ReleaseChild { binding } if *binding == child
    )));
    native
        .accept_native_effects(&egui::Context::default(), effects)
        .unwrap();
    assert!(native.bridge.retirement_blocks_quiescence(child));
    assert!(!native.session.is_current_native_binding(child));
    assert!(native.session.recognizes_native_binding(child));

    let destroyed = native.bridge.with_viewports_mut(|routes| {
        NativeWindowEventRecord::for_test_ingress(
            2,
            WindowId::from(22),
            Some(viewport),
            WindowEvent::Destroyed,
            routes,
        )
    });
    native
        .bridge
        .push_record(HostRecord::WindowEvent(destroyed));
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .is_some()
    );
    commit(&mut native);
    native.bridge.push_viewport_roster(live_roster([next_root]));
    assert!(native.reduce_next_viewport_roster().unwrap());
    let mut report = commit(&mut native);
    assert!(report.take_native_effects().is_empty());
    let prepared = native.prepare_committed_retirements().unwrap().unwrap();
    assert!(native.commit_retirements(prepared).is_empty());
    assert!(native.try_report_retirement_quiescence().unwrap());
    assert_eq!(native.viewport_binding(viewport), None);
    assert_eq!(native.viewport_binding(ViewportId::ROOT), Some(next_root));
    assert!(!native.session.recognizes_native_binding(child));
    assert!(!native.bridge.references_binding(child));
    assert!(!native.bridge.retirement_has_pending_work());
}

fn bounded(resources: usize, window: Option<WindowId>) -> NativeCoordinator {
    let input = NativeInputStorageLimit::new(NonZeroUsize::new(16).unwrap(), 1 << 20)
        .with_producer_storage_limit(NativeProducerStorageLimit::new(
            NonZeroUsize::new(2).unwrap(),
            NonZeroUsize::MIN,
            1 << 20,
        ))
        .with_report_storage_limit(NativeReportStorageLimit::new(
            NonZeroUsize::new(8).unwrap(),
            NonZeroUsize::new(2).unwrap(),
            1 << 20,
        ));
    let owners = NativeOwnerStorageLimit {
        resource_slots: resources,
        close_slots: 2,
        byte_limit: NativeOwnerStorageLimit::required_bytes(2, 2, resources, 2).unwrap(),
    };
    let mut native = NativeCoordinator::new_with_storage_limits(
        DockspaceSession::from_layout(layout(), DockPolicy::default()).unwrap(),
        NativePointerRoster::Exact(Vec::new()),
        Some(input),
        Some(owners),
    )
    .unwrap();
    native
        .register_native_root(FIRST_SURFACE, HostWindowToken::new(11))
        .unwrap();
    let binding = commit(&mut native).native_bindings()[0];
    if let Some(window) = window {
        native
            .bind_viewport(ViewportId::ROOT, window, binding)
            .unwrap();
    }
    native
}

fn continuation_layout(surface: SurfaceId) -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        surface,
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::central_tabs([ItemId::new(1)]),
        ),
    )])
    .unwrap()
}

#[test]
fn root_rebinding_failure_keeps_the_application_field_route_and_workspace_atomic() {
    for destroyed in [false, true] {
        let mut native = bounded(if destroyed { 2 } else { 1 }, Some(WindowId::from(11)));
        let predecessor = native.viewport_binding(ViewportId::ROOT).unwrap();
        let before = native.session.version();
        let incoming = SurfaceId::new(8);
        let reset = native
            .session
            .prepare_layout_reset(continuation_layout(incoming))
            .unwrap();
        let bridge = native.bridge.clone();
        let mut frame = native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        frame
            .frame
            .submit_native_layout_reset(&reset, predecessor, incoming)
            .unwrap();
        let successor = frame
            .frame
            .proposed_native_root_continuations()
            .next()
            .unwrap()
            .successor();
        if destroyed {
            bridge.push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
                3,
                WindowId::from(11),
                Some(ViewportId::ROOT),
                Some(predecessor),
                WindowEvent::Destroyed,
            )));
        }
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let mut root_surface = predecessor.surface();
        let error = frame.commit_publishing_root(&mut root_surface).unwrap_err();
        if !destroyed {
            assert!(error.is_input_capacity());
        }
        assert_eq!(root_surface, predecessor.surface());
        assert_eq!(native.session.version(), before);
        assert!(native.session.view().surface(incoming).is_none());
        assert!(native.session.is_current_native_binding(predecessor));
        assert!(!native.session.recognizes_native_binding(successor));
        assert_eq!(native.viewport_binding(ViewportId::ROOT), Some(predecessor));
        assert!(
            bridge
                .with_viewports(|routes| routes.has_window_receipt(ViewportId::ROOT, predecessor))
        );
        assert_eq!(bridge.workspace_replacement_predecessor(successor), None);
    }
}

#[test]
fn bounded_root_rebinding_waits_for_late_output_then_reclaims_exact_predecessors() {
    let mut native = bounded(2, Some(WindowId::from(11)));
    let mut root_surface = FIRST_SURFACE;
    for index in 0..8 {
        let predecessor = native.viewport_binding(ViewportId::ROOT).unwrap();
        // Ready and paint the current Root through ordinary core host records.
        native
            .report_snapshot(
                [(predecessor, ready_window_facts(0.0))],
                NativeWorkAreaRoster::Unknown,
                crate::capabilities::capabilities_for_backend(
                    eframe::NativeWindowingBackend::Windows,
                ),
            )
            .unwrap();
        commit(&mut native);
        let mut frame = native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        frame
            .frame
            .measure_surface(
                root_surface,
                dockspace::runtime::UniformSurfaceMetrics::new(
                    dockspace::geometry::LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
                    dockspace::geometry::LogicalSize::new(32.0, 24.0).unwrap(),
                    96.0,
                )
                .unwrap(),
            )
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let report = frame.commit().unwrap();
        native.settle_host_frame_report(&report).unwrap();
        let mut frame = native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        let receipt = frame
            .frame
            .paint_plan(root_surface)
            .unwrap()
            .unwrap()
            .execute_surface_program(|_| Ok::<(), ()>(()))
            .unwrap();
        frame.frame.confirm_surface_painted(receipt).unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let output = frame
            .commit()
            .unwrap()
            .take_painted_outputs()
            .pop()
            .unwrap();
        assert!(output.matches_native_binding(predecessor));
        let focus = eframe::prepare_native_viewport_focus(ViewportId::ROOT);
        assert!(native.bridge.reserve_focus_callback(
            ViewportId::ROOT,
            predecessor,
            WindowId::from(11),
            focus.token(),
        ));
        let incoming = SurfaceId::new(if index % 2 == 0 { 8 } else { 1 });
        let reset = native
            .session
            .prepare_layout_reset(continuation_layout(incoming))
            .unwrap();
        let mut frame = native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        frame
            .frame
            .submit_native_layout_reset(&reset, predecessor, incoming)
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let mut report = frame.commit_publishing_root(&mut root_surface).unwrap();
        let [pair] = report.native_root_continuations() else {
            panic!("one exact Root continues");
        };
        let successor = pair.successor();
        assert_eq!(pair.predecessor(), predecessor);
        assert_eq!(root_surface, successor.surface());
        assert_ne!(root_surface, predecessor.surface());
        assert!(report.take_native_effects().is_empty());
        native.settle_host_frame_report(&report).unwrap();
        assert!(!native.try_report_retirement_quiescence().unwrap());
        native
            .session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        commit(&mut native);
        assert!(native.session.presented_surface(root_surface).is_none());
        assert!(!native.try_report_retirement_quiescence().unwrap());
        native.bridge.push_record(HostRecord::ViewportFocus(
            NativeViewportFocusRecord::for_test(
                focus.token(),
                ViewportId::ROOT,
                WindowId::from(11),
                Some(successor),
                eframe::NativeViewportFocusStatus::Requested,
            ),
        ));
        assert!(
            native
                .bridge
                .front_viewport_focus()
                .unwrap()
                .references_binding(predecessor)
        );
        assert!(native.reduce_next_viewport_focus().unwrap());
        commit(&mut native);
        assert!(native.try_report_retirement_quiescence().unwrap());
        commit(&mut native);
        assert!(!native.session.recognizes_native_binding(predecessor));
        assert!(!native.bridge.references_binding(predecessor));
        assert!(!native.bridge.retirement_has_pending_work());
        assert!(native.bridge.retirement_destroyed_observations().is_empty());
        native.bridge.with_viewports(|routes| {
            assert_eq!(routes.window(ViewportId::ROOT), Some(WindowId::from(11)));
            assert!(routes.has_window_receipt(ViewportId::ROOT, successor));
        });
    }
}

#[test]
fn successor_resource_capacity_is_checked_before_the_root_route_changes() {
    let mut native = bounded(1, Some(WindowId::from(11)));
    let predecessor = native.viewport_binding(ViewportId::ROOT).unwrap();
    let version = native.session.version();
    let reset = native.session.prepare_layout_reset(layout()).unwrap();
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.frame.submit_layout_reset(&reset).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    assert!(frame.commit().unwrap_err().is_input_capacity());
    assert_eq!(native.session.version(), version);
    assert!(native.session.is_current_native_binding(predecessor));
    assert_eq!(native.viewport_binding(ViewportId::ROOT), Some(predecessor));
    assert!(
        native
            .bridge
            .with_viewports(|routes| { routes.has_window_receipt(ViewportId::ROOT, predecessor) })
    );
}

#[test]
fn repeated_bounded_resets_reclaim_retired_producer_bindings_without_destroying_the_root() {
    let mut native = bounded(2, Some(WindowId::from(11)));
    for _ in 0..8 {
        let predecessor = native.viewport_binding(ViewportId::ROOT).unwrap();
        let report = reset(&mut native);
        let next = successor(&report, predecessor);
        native.settle_host_frame_report(&report).unwrap();
        assert!(native.session.recognizes_native_binding(predecessor));
        assert!(native.try_report_retirement_quiescence().unwrap());
        assert!(!native.session.recognizes_native_binding(predecessor));
        commit(&mut native);
        assert!(!native.bridge.retirement_has_pending_work());
        assert!(!native.bridge.references_binding(predecessor));
        assert!(native.session.is_current_native_binding(next));
        assert_eq!(native.viewport_binding(ViewportId::ROOT), Some(next));
        assert!(native.bridge.with_viewports(|routes| {
            routes.window(ViewportId::ROOT) == Some(WindowId::from(11))
                && routes.has_window_receipt(ViewportId::ROOT, next)
        }));
        assert!(native.bridge.retirement_destroyed_observations().is_empty());
    }
}

#[test]
fn late_property_receipts_keep_their_old_resource_until_their_exact_commits() {
    let mut native = bounded(2, Some(WindowId::from(11)));
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(11);
    let predecessor = native.viewport_binding(viewport).unwrap();
    let focus = eframe::prepare_native_viewport_focus(viewport);
    let show = eframe::prepare_native_viewport_visibility(viewport, true);
    assert!(
        native
            .bridge
            .reserve_focus_callback(viewport, predecessor, window, focus.token())
    );
    assert!(native.bridge.reserve_visibility_callback(
        viewport,
        predecessor,
        window,
        show.token(),
        true
    ));
    let report = reset(&mut native);
    let next = successor(&report, predecessor);
    assert!(native.settle_host_frame_report(&report).unwrap());
    assert!(native.bridge.references_binding(predecessor));
    assert!(native.bridge.references_binding(next));
    assert!(!native.try_report_retirement_quiescence().unwrap());
    let next_focus = eframe::prepare_native_viewport_focus(viewport);
    assert!(
        native
            .bridge
            .reserve_focus_callback(viewport, next, window, next_focus.token())
    );
    assert!(!native.bridge.reserve_focus_callback(
        viewport,
        predecessor,
        window,
        next_focus.token()
    ));

    // Simulate callback-time lookup seeing the successor. The old command
    // receipt must qualify this result back to its original exact owner.
    native.bridge.push_record(HostRecord::ViewportFocus(
        NativeViewportFocusRecord::for_test(
            focus.token(),
            viewport,
            window,
            Some(next),
            NativeViewportFocusStatus::Requested,
        ),
    ));
    let old_focus = native.bridge.front_viewport_focus().unwrap();
    assert!(old_focus.references_binding(predecessor));
    assert!(!old_focus.references_binding(next));
    assert!(native.reduce_next_viewport_focus().unwrap());
    commit(&mut native);
    assert!(native.bridge.references_binding(predecessor));
    native.bridge.push_record(HostRecord::ViewportVisibility(
        NativeViewportVisibilityRecord::for_test(
            show.token(),
            viewport,
            window,
            next,
            true,
            NativeViewportVisibilityStatus::Dispatched,
        ),
    ));
    assert_eq!(
        native
            .next_viewport_visibility()
            .unwrap()
            .unwrap()
            .binding(),
        predecessor
    );
    assert!(native.reduce_next_viewport_visibility().unwrap());
    commit(&mut native);
    assert!(!native.bridge.references_binding(predecessor));
    assert!(native.try_report_retirement_quiescence().unwrap());
    assert!(!native.session.recognizes_native_binding(predecessor));
    assert_eq!(native.bridge.create_ack_count(), 0);
    assert_eq!(native.viewport_binding(viewport), Some(next));
    assert!(native.bridge.with_viewports(|routes| {
        routes
            .focus_binding_for_event(window, Some(viewport))
            .is_none()
    }));
}

#[test]
fn a_received_destroyed_receipt_cannot_move_to_the_workspace_successor() {
    let (mut native, root, _) = presented_local_coordinate_routes();
    let version = native.session.version();
    let reset = native.session.prepare_layout_reset(layout()).unwrap();
    let bridge = native.bridge.clone();
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.frame.submit_layout_reset(&reset).unwrap();
    bridge.push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
        3,
        WindowId::from(11),
        Some(ViewportId::ROOT),
        Some(root),
        WindowEvent::Destroyed,
    )));
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    assert!(frame.commit().is_err());
    assert_eq!(native.session.version(), version);
    assert_eq!(native.viewport_binding(ViewportId::ROOT), Some(root));
    assert_eq!(native.bridge.front_event().unwrap().binding(), Some(root));
}

#[test]
fn late_predecessor_paint_terminal_cannot_present_the_successor() {
    let (mut native, root, _) = presented_local_coordinate_routes();
    let mut painted = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let receipt = painted
        .frame
        .paint_plan(FIRST_SURFACE)
        .unwrap()
        .unwrap()
        .execute_surface_program(|_| Ok::<(), ()>(()))
        .unwrap();
    painted.frame.confirm_surface_painted(receipt).unwrap();
    painted
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = painted.commit().unwrap();
    let [output] = report.take_painted_outputs().try_into().unwrap();
    assert!(output.matches_native_binding(root));

    let report = reset(&mut native);
    let next = successor(&report, root);
    native.settle_host_frame_report(&report).unwrap();
    assert!(!output.matches_native_binding(next));
    native
        .session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    commit(&mut native);
    assert_eq!(native.viewport_binding(ViewportId::ROOT), Some(next));
    assert!(native.session.presented_surface(FIRST_SURFACE).is_none());
    assert!(!native.receivers.references_binding(root));
    assert!(!native.receivers.references_binding(next));
}

#[cfg(feature = "serde")]
#[test]
fn document_restore_rebinds_the_same_physical_root_from_the_complete_report() {
    use dockspace::runtime::{DockspaceDocumentBootstrap, DockspaceDocumentId};

    let document = DockspaceDocumentId::from_bytes([0x43; 16]);
    let persistent = || {
        let mut bootstrap = DockspaceDocumentBootstrap::new(document);
        assert_eq!(bootstrap.ensure_item("pane:first").unwrap(), ItemId::new(1));
        assert_eq!(
            bootstrap.ensure_item("pane:second").unwrap(),
            ItemId::new(2)
        );
        DockspaceSession::from_persistent_layout(layout(), DockPolicy::default(), bootstrap)
            .unwrap()
    };
    let mut native =
        NativeCoordinator::new(persistent(), NativePointerRoster::Exact(Vec::new())).unwrap();
    let (first, second) = register_roots(&mut native);
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(11), first)
        .unwrap();
    let peer = ViewportId::from_hash_of("restored-root-peer");
    native
        .bind_viewport(peer, WindowId::from(22), second)
        .unwrap();
    let bytes = native.save_document_json().unwrap();
    let restore = native
        .session
        .prepare_document_restore_json(&bytes, |candidate, _| candidate == document)
        .unwrap();
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.frame.submit_document_restore(&restore).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert!(matches!(
        report.inputs().last(),
        Some(HostInputOutcome::DocumentRestored { .. })
    ));
    assert!(native.settle_host_frame_report(&report).unwrap());
    for (viewport, old, window) in [
        (ViewportId::ROOT, first, WindowId::from(11)),
        (peer, second, WindowId::from(22)),
    ] {
        let next = successor(&report, old);
        assert_eq!(native.viewport_binding(viewport), Some(next));
        assert!(
            native
                .bridge
                .with_viewports(|routes| routes.window(viewport) == Some(window)
                    && routes.has_window_receipt(viewport, next))
        );
    }
}

// Opaque eframe output identities can only be obtained from real native callbacks.
#[cfg(target_os = "linux")]
mod native_output {
    use std::sync::Mutex;

    use egui_dockspace::{DockStyle, PaneView};
    use winit::platform::x11::EventLoopBuilderExtX11;

    use super::*;

    struct Panes;

    impl PaneView for Panes {
        fn title(&self, _item: ItemId) -> Option<egui::WidgetText> {
            Some("Workspace replacement".into())
        }

        fn ui(&mut self, _item: ItemId, _ui: &mut egui::Ui) -> egui_dockspace::PaneResponse {
            egui_dockspace::PaneResponse::default()
        }
    }

    struct OutputFixture {
        native: NativeCoordinator,
        root: NativeSurfaceBinding,
        paint: Option<(
            dockspace::runtime::PaintedSurfaceOutput,
            crate::host_frame::NativeSurfacePaint,
        )>,
        replaced: Option<(NativeOutputToken, NativeSurfaceBinding)>,
        terminal: Option<eframe::NativeOutputResult>,
        done: bool,
    }

    struct Host(Arc<Mutex<OutputFixture>>);

    impl NativeHostHandler for Host {
        fn try_attach(&self, attachment: eframe::NativeHostAttachment) -> bool {
            self.0.lock().unwrap().native.bridge.try_attach(attachment)
        }

        fn detach(&self, attachment: eframe::NativeHostAttachment) {
            self.0.lock().unwrap().native.bridge.detach(attachment);
        }

        fn on_output_begin(
            &self,
            token: NativeOutputToken,
            window: eframe::NativeWindowSnapshot,
            _roster: Option<eframe::NativeViewportRoster<'_>>,
        ) -> eframe::NativeOutputAdmission {
            // This fixture supplies its complete core inventory before the loop;
            // only output identities and renderer terminals come from this host.
            self.0
                .lock()
                .unwrap()
                .native
                .bridge
                .on_output_begin(token, window, None)
        }

        fn on_output_pass_finalized(
            &self,
            finalization: eframe::NativeOutputPassFinalization<'_>,
        ) -> NativeHostWake {
            let (context, token, _, _, _) = finalization.into_parts();
            let mut fixture = self.0.lock().unwrap();
            let Some((output, paint)) = fixture.paint.take() else {
                return NativeHostWake::Wait;
            };
            fixture
                .native
                .bind_painted_surface(context, token, output, &paint)
                .unwrap();
            let root = fixture.root;
            assert!(fixture.native.receivers.references_binding(root));
            let report = reset(&mut fixture.native);
            let next = successor(&report, root);
            assert!(fixture.native.settle_host_frame_report(&report).unwrap());
            let reservation = fixture.native.bridge.output_reservation(token).unwrap();
            assert_eq!(reservation.binding(), Some(root));
            assert!(!reservation.is_abandoned());
            assert!(!fixture.native.bridge.attach_output_binding(token, next));
            assert!(fixture.native.bridge.has_pending_output(token));
            assert!(fixture.native.bridge.references_binding(root));
            assert!(fixture.native.bridge.references_binding(next));
            assert!(!fixture.native.try_report_retirement_quiescence().unwrap());
            assert!(!fixture.native.receivers.references_binding(root));
            assert!(!fixture.native.receivers.references_binding(next));
            assert_eq!(
                fixture
                    .native
                    .bridge
                    .with_viewports(|routes| routes.window(ViewportId::ROOT)),
                Some(token.window_id())
            );
            fixture.replaced = Some((token, next));
            NativeHostWake::RepaintRoot
        }

        fn on_output(&self, result: eframe::NativeOutputResult) -> NativeHostWake {
            let mut fixture = self.0.lock().unwrap();
            if fixture
                .replaced
                .is_some_and(|(token, _)| token == result.token())
            {
                assert!(fixture.terminal.is_none());
                fixture.terminal = Some(result);
            }
            fixture.native.bridge.on_output(result)
        }
    }

    struct App(Arc<Mutex<OutputFixture>>);

    impl eframe::App for App {
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            let token = eframe::current_native_output_token().unwrap();
            let mut fixture = self.0.lock().unwrap();
            if fixture.replaced.is_none() {
                let root = fixture.root;
                fixture
                    .native
                    .bind_viewport(ViewportId::ROOT, token.window_id(), root)
                    .unwrap();
                assert!(fixture.native.bridge.attach_output_binding(token, root));
                let mut frame = fixture
                    .native
                    .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
                    .unwrap();
                let mut paint = frame
                    .paint_surface(
                        egui::Id::unique("workspace-replacement-native-output"),
                        FIRST_SURFACE,
                        ui,
                        &mut Panes,
                        &DockStyle::default(),
                    )
                    .unwrap();
                assert!(paint.confirm_into(&mut frame.frame).unwrap());
                frame
                    .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                    .unwrap();
                let mut report = frame.commit().unwrap();
                let [output] = report.take_painted_outputs().try_into().unwrap();
                fixture.paint = Some((output, paint));
            } else if !fixture.done {
                let (old_token, next) = fixture.replaced.unwrap();
                assert_ne!(old_token, token);
                assert_eq!(old_token.window_id(), token.window_id());
                assert_eq!(fixture.terminal.unwrap().token(), old_token);
                assert_eq!(
                    fixture
                        .native
                        .bridge
                        .output_reservation(old_token)
                        .unwrap()
                        .binding(),
                    Some(fixture.root)
                );
                assert_eq!(
                    fixture
                        .native
                        .bridge
                        .output_reservation(token)
                        .unwrap()
                        .binding(),
                    Some(next)
                );
                commit(&mut fixture.native);
                assert!(
                    fixture
                        .native
                        .bridge
                        .output_reservation(old_token)
                        .is_none()
                );
                assert!(!fixture.native.bridge.references_binding(fixture.root));
                assert!(fixture.native.try_report_retirement_quiescence().unwrap());
                assert!(
                    !fixture
                        .native
                        .session
                        .recognizes_native_binding(fixture.root)
                );
                assert!(!fixture.native.receivers.references_binding(fixture.root));
                assert!(!fixture.native.receivers.references_binding(next));
                assert!(
                    fixture
                        .native
                        .session
                        .presented_surface(FIRST_SURFACE)
                        .is_none()
                );
                assert_eq!(
                    fixture.native.viewport_binding(ViewportId::ROOT),
                    Some(next)
                );
                fixture
                    .native
                    .settle_retained_output_token(token, true)
                    .unwrap();
                fixture.done = true;
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ui.ctx().request_repaint();
        }
    }

    #[test]
    #[ignore = "requires a real X11 display and renderer; output identities are minted by eframe"]
    fn real_old_output_token_terminal_releases_only_its_predecessor_resource() {
        let mut native = bounded(2, None);
        let root = commit(&mut native).native_bindings()[0];
        native
            .report_snapshot(
                [(root, ready_window_facts(0.0))],
                NativeWorkAreaRoster::Unknown,
                test_managed_capabilities(),
            )
            .unwrap();
        let mut frame = native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        frame
            .frame
            .measure_surface(
                FIRST_SURFACE,
                UniformSurfaceMetrics::new(
                    LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
                    LogicalSize::new(32.0, 24.0).unwrap(),
                    80.0,
                )
                .unwrap(),
            )
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        frame.commit().unwrap();
        let fixture = Arc::new(Mutex::new(OutputFixture {
            native,
            root,
            paint: None,
            replaced: None,
            terminal: None,
            done: false,
        }));
        let app = Arc::clone(&fixture);
        eframe::run_native(
            "Workspace replacement output regression",
            eframe::NativeOptions {
                viewport: egui::ViewportBuilder::default().with_inner_size([640.0, 480.0]),
                native_host: Some(Arc::new(Host(Arc::clone(&fixture)))),
                event_loop_builder: Some(Box::new(|builder| {
                    builder.with_any_thread(true).with_x11();
                })),
                ..Default::default()
            },
            Box::new(move |creation| {
                creation
                    .egui_ctx
                    .options_mut(|options| options.max_passes = NonZeroUsize::MIN);
                Ok(Box::new(App(app)))
            }),
        )
        .unwrap();
        assert!(fixture.lock().unwrap().done);
    }
}

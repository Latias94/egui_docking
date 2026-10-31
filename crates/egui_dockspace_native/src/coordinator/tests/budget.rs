use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use dockspace::close::{CloseDecision, CloseItemDecisionState, CloseRequestId};
use dockspace::runtime::{
    NativeDesktopPointerLocation, NativeDesktopPosition, NativeInputBudget, NativeMonotonicClock,
    NativePointerEvent, NativePointerHover, NativePointerId, NativePointerOwner,
    NativeSurfaceCloseRequest,
};

use super::*;

fn one_record_budget() -> NativeInputBudget {
    let now = Arc::new(AtomicU64::new(0));
    let source = Arc::clone(&now);
    let clock = NativeMonotonicClock::new(move || {
        if source.fetch_add(1, Ordering::SeqCst) < 2 {
            Duration::ZERO
        } else {
            Duration::from_millis(2)
        }
    });
    NativeInputBudget::new(&clock)
}

fn queued_snapshot() -> NativeCoordinator {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    native
        .session
        .record_native_pointer_sequence((0..2).map(|_| unknown_move()))
        .expect("independent pointer records precede the snapshot");
    native
        .bridge
        .push_viewport_roster(live_roster([first, second]));
    native
        .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
        .expect("the exact roster enters core ingress");
    assert!(native.queued_snapshot.is_some());
    native
}

fn unknown_move() -> NativePointerInput {
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
}

fn commit_poll(native: &mut NativeCoordinator, budget: &mut NativeInputBudget) -> HostFrameReport {
    let mut frame = native
        .poll_host_frame(budget, |_| NativeReceiverAnswer::Unknown)
        .expect("the exact input continuation polls")
        .expect("an independent prefix seals");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn prefix_before_snapshot(native: &mut NativeCoordinator) -> HostFrameReport {
    let mut budget = one_record_budget();
    let frame = native
        .session
        .poll_native_host_frame(&mut budget, |_| NativeReceiverAnswer::Unknown)
        .expect("exhaustion is not a protocol error")
        .expect("the first independent pointer record can seal");
    assert!(frame.native_input_pending());
    let mut frame = NativeHostFrame::new(frame, native.bridge.clone(), &mut native.receivers, None);
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the retained-output prefix settles without semantic paint");
    frame.commit().expect("the pointer prefix commits")
}

fn pending_content_close(
    item_count: u64,
) -> (NativeCoordinator, NativeSurfaceCloseRequest, CloseRequestId) {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            FIRST_SURFACE,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1)]),
            ),
        ),
        DockspaceSurfaceLayout::new(
            SECOND_SURFACE,
            DockspaceRootLayout::new(
                RootId::new(2),
                DockspaceNode::tabs((2..=item_count + 1).map(ItemId::new)),
            ),
        ),
    ])
    .unwrap();
    let session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let mut native =
        NativeCoordinator::new(session, NativePointerRoster::Exact(Vec::new())).unwrap();
    let (root, binding) = register_roots(&mut native);
    let close = super::close::observe_model_close(
        &mut native,
        binding,
        [root, binding],
        ViewportId::from_hash_of("budgeted-content-close"),
        WindowId::from(86),
        44,
    );
    assert!(
        native
            .drive_close_policy(
                &egui::Context::default(),
                crate::NativeWindowClosePolicy::CloseContent,
                root.surface(),
                &mut test_budget(),
            )
            .unwrap()
    );
    let request = native.bridge.with_close_control(|control| {
        let mut requests = control.decision_requests();
        let request = requests.next().unwrap();
        assert!(requests.next().is_none());
        request
    });
    (native, close, request)
}

fn pending_decision_items(native: &mut NativeCoordinator, request: CloseRequestId) -> Vec<ItemId> {
    let frame = native.session.begin_host_frame().unwrap();
    frame
        .pending_close_decisions(request)
        .map(|item| item.item())
        .collect()
}

fn clear_root_repaint(context: &egui::Context) {
    for _ in 0..8 {
        if !context.has_requested_repaint_for(&ViewportId::ROOT) {
            break;
        }
        context.begin_pass(egui::RawInput::default());
        let mut output = context.end_pass();
        output.textures_delta.clear();
    }
    assert!(!context.has_requested_repaint_for(&ViewportId::ROOT));
}

#[test]
fn exhausted_prefix_keeps_the_mailbox_boundary_until_the_native_suffix_commits() {
    let mut native = queued_snapshot();
    let report = prefix_before_snapshot(&mut native);
    assert!(report.native_input_pending());
    assert!(native.bridge.callback_boundary_pending());
}

#[test]
fn partial_report_without_snapshot_outcome_retains_its_exact_roster_envelope() {
    let mut native = queued_snapshot();
    let report = prefix_before_snapshot(&mut native);
    assert!(!report.inputs().iter().any(|outcome| matches!(
        outcome,
        HostInputOutcome::NativePlatformSnapshotApplied { .. }
    )));
    assert!(
        !native
            .settle_host_frame_report(&report)
            .expect("a pending native suffix has not settled its snapshot yet")
    );
    assert!(native.queued_snapshot.is_some());
    let final_report = commit_poll(&mut native, &mut test_budget());
    assert!(!final_report.native_input_pending());
    assert!(native.settle_host_frame_report(&final_report).unwrap());
    assert!(native.queued_snapshot.is_none());
    assert!(!native.bridge.callback_boundary_pending());
}

#[test]
fn snapshot_seen_in_an_earlier_prefix_settles_after_a_final_report_without_snapshot() {
    let mut native = queued_snapshot();
    native
        .session
        .record_native_pointer_sequence((0..256).map(|_| unknown_move()))
        .unwrap();
    let first = commit_poll(&mut native, &mut test_budget());
    assert!(first.native_input_pending());
    assert!(first.inputs().iter().any(|input| matches!(
        input,
        HostInputOutcome::NativePlatformSnapshotApplied { .. }
    )));
    assert!(!native.settle_host_frame_report(&first).unwrap());
    assert!(native.queued_snapshot.is_some());
    let last = commit_poll(&mut native, &mut test_budget());
    assert!(!last.native_input_pending());
    assert!(!last.inputs().iter().any(|input| matches!(
        input,
        HostInputOutcome::NativePlatformSnapshotApplied { .. }
    )));
    assert!(native.settle_host_frame_report(&last).unwrap());
    assert!(native.queued_snapshot.is_none());
}

#[test]
fn pending_native_suffix_does_not_delay_committed_local_coordinate_refresh() {
    let (mut native, first, second) = presented_local_coordinate_routes();
    let window = WindowId::from(11);
    let original = native.session.native_local_coordinates(first).unwrap();
    native
        .bridge
        .push_viewport_roster(NativeViewportRosterRecord::for_test(
            [
                (
                    first,
                    ready_window_facts(0.0)
                        .with_presentation_scale_factor(ScaleFactor::new(2.0).unwrap()),
                ),
                (second, ready_window_facts(720.0)),
            ]
            .map(|(binding, facts)| {
                crate::window_snapshot::CompiledWindowObservation::new(binding, facts)
            }),
            true,
        ));
    assert!(native.reduce_next_viewport_roster().unwrap());
    native
        .session
        .record_native_pointer_sequence((0..256).map(|_| unknown_move()))
        .unwrap();
    let report = commit_poll(&mut native, &mut test_budget());
    assert!(report.native_input_pending());
    assert!(report.inputs().iter().any(|input| matches!(
        input,
        HostInputOutcome::NativePlatformSnapshotApplied { .. },
    )));
    let committed = native.session.native_local_coordinates(first).unwrap();
    assert_ne!(committed, original);
    assert_eq!(
        native
            .bridge
            .with_viewports(|routes| routes.local_coordinates_for_event(window, None)),
        Some(original),
    );
    assert!(!native.settle_host_frame_report(&report).unwrap());
    assert!(native.queued_snapshot.is_some());
    assert_eq!(
        native
            .bridge
            .with_viewports(|routes| routes.local_coordinates_for_event(window, None)),
        Some(committed),
        "only the committed prefix grants capability while the snapshot envelope awaits its suffix",
    );
    let final_report = commit_poll(&mut native, &mut test_budget());
    assert!(!final_report.native_input_pending());
    assert!(native.settle_host_frame_report(&final_report).unwrap());
    assert!(native.queued_snapshot.is_none());
}

#[test]
fn atomic_yield_keeps_the_boundary_even_after_every_raw_callback_was_claimed() {
    let mut native = coordinator();
    register_roots(&mut native);
    let before = native.session.version();
    native
        .session
        .record_native_pointer_batch((0..257).map(|_| unknown_move()))
        .unwrap();
    let mut first_budget = test_budget();
    assert!(
        native
            .poll_host_frame(&mut first_budget, |_| NativeReceiverAnswer::Unknown)
            .unwrap()
            .is_none()
    );
    assert_eq!(first_budget.remaining_records(), 0);
    assert!(native.bridge.callback_boundary_pending());
    assert!(!native.has_pending_callback_records());
    assert!(!native.is_quiescent());
    assert_eq!(native.session.version(), before);
    let final_report = commit_poll(&mut native, &mut test_budget());
    assert!(!final_report.native_input_pending());
    assert!(!native.bridge.callback_boundary_pending());
    assert_eq!(native.session.version(), before);
}

#[test]
fn deferred_child_paint_survives_repeated_partial_commits_and_a_dropped_final_frame() {
    let mut native = coordinator();
    let (_, binding) = register_roots(&mut native);
    let context = egui::Context::default();
    let child = ViewportId::from_hash_of("partial-native-child");
    native.reserve_viewport(child, binding).unwrap();
    let wakes = Arc::new(AtomicU64::new(0));
    context.set_request_repaint_callback({
        let wakes = Arc::clone(&wakes);
        move |info| {
            if info.viewport_id == child {
                wakes.fetch_add(1, Ordering::SeqCst);
            }
        }
    });
    native
        .session
        .record_native_pointer_sequence((0..3).map(|_| unknown_move()))
        .unwrap();
    native.bridge.block_semantic_viewport(child);

    for _ in 0..2 {
        let mut budget = one_record_budget();
        let mut frame = native
            .poll_resolved_host_frame(&context, &mut budget)
            .unwrap()
            .unwrap();
        assert!(frame.native_input_pending());
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        assert!(frame.commit().unwrap().native_input_pending());
        assert!(native.bridge.callback_boundary_pending());
        assert_eq!(wakes.load(Ordering::SeqCst), 0);
    }
    {
        let mut budget = test_budget();
        let frame = native
            .poll_resolved_host_frame(&context, &mut budget)
            .unwrap()
            .unwrap();
        assert!(!frame.native_input_pending());
    }
    assert!(native.bridge.callback_boundary_pending());
    assert_eq!(wakes.load(Ordering::SeqCst), 0);

    let mut budget = test_budget();
    let mut frame = native
        .poll_resolved_host_frame(&context, &mut budget)
        .unwrap()
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    assert!(!frame.commit().unwrap().native_input_pending());
    assert!(!native.bridge.callback_boundary_pending());
    assert_eq!(wakes.load(Ordering::SeqCst), 1);
    assert!(native.bridge.take_ready_semantic_viewports().is_empty());
}

#[test]
fn policy_queued_before_a_yield_is_not_requeued_when_its_exact_plan_resumes() {
    let mut native = coordinator();
    let (root, binding) = register_roots(&mut native);
    let viewport = ViewportId::from_hash_of("budgeted-native-close-child");
    let close = super::close::observe_model_close(
        &mut native,
        binding,
        [root, binding],
        viewport,
        WindowId::from(81),
        40,
    );
    let calls = Arc::new(AtomicU64::new(0));
    let source = Arc::clone(&calls);
    let clock = NativeMonotonicClock::new(move || {
        if source.fetch_add(1, Ordering::SeqCst) < 3 {
            Duration::ZERO
        } else {
            Duration::from_millis(2)
        }
    });
    let context = egui::Context::default();
    let mut budget = NativeInputBudget::new(&clock);
    assert!(
        native
            .drive_close_policy(
                &context,
                crate::NativeWindowClosePolicy::CloseContent,
                root.surface(),
                &mut budget
            )
            .unwrap()
    );
    assert!(native.bridge.callback_boundary_pending());
    assert!(
        native
            .bridge
            .with_close_control(|control| control.unsubmitted_requests().next().is_none())
    );
    assert!(
        native
            .bridge
            .with_close_control(|control| control.decision_requests().next().is_none())
    );
    assert!(
        !native
            .drive_close_policy(
                &context,
                crate::NativeWindowClosePolicy::CloseContent,
                root.surface(),
                &mut test_budget()
            )
            .unwrap()
    );
    let report = commit_poll(&mut native, &mut test_budget());
    let plans = report
        .inputs()
        .iter()
        .filter_map(|outcome| match outcome {
            HostInputOutcome::NativeSurfaceCloseRequested {
                close: exact, plan, ..
            } => {
                assert_eq!(*exact, close);
                Some(plan.request())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(plans.len(), 1);
    native.settle_close_control_report(&report).unwrap();
    assert_eq!(
        native
            .bridge
            .with_close_control(|control| control.decision_requests().count()),
        1
    );
    assert!(
        native
            .drive_close_policy(
                &context,
                crate::NativeWindowClosePolicy::CloseContent,
                root.surface(),
                &mut test_budget()
            )
            .unwrap()
    );
    assert!(
        native
            .bridge
            .with_close_control(|control| control.decision_requests().next().is_none())
    );
    assert!(
        !native
            .drive_close_policy(
                &context,
                crate::NativeWindowClosePolicy::CloseContent,
                root.surface(),
                &mut test_budget()
            )
            .unwrap()
    );
    assert!(
        !native
            .dispatch_viewport_commands(&egui::Context::default())
            .expect("no native focus command is queued"),
        "tokenless close policy must not dispatch an unrelated focus command"
    );
}

#[test]
fn native_close_content_decisions_share_the_remaining_record_allowance() {
    let (mut native, close, request) = pending_content_close(259);
    assert_eq!(pending_decision_items(&mut native, request).len(), 259);
    let context = egui::Context::default();
    clear_root_repaint(&context);
    let mut budget = test_budget();
    budget.complete_unsequenced_records(budget.remaining_records() - 3);
    assert!(
        native
            .drive_close_policy(
                &context,
                crate::NativeWindowClosePolicy::CloseContent,
                FIRST_SURFACE,
                &mut budget,
            )
            .unwrap()
    );
    assert_eq!(budget.remaining_records(), 0);
    assert!(!native.bridge.callback_boundary_pending());
    native.bridge.with_close_control(|control| {
        assert_eq!(control.decision_requests().collect::<Vec<_>>(), [request]);
        assert_eq!(control.request_for_binding(close.binding()), Some(close));
        assert!(control.has_runnable_policy_work());
    });
    assert_eq!(
        pending_decision_items(&mut native, request),
        (5..=260).map(ItemId::new).collect::<Vec<_>>()
    );
    assert!(context.has_requested_repaint_for(&ViewportId::ROOT));
    clear_root_repaint(&context);

    // Reusing the exhausted budget cannot open an empty candidate or repeat votes.
    assert!(
        !native
            .drive_close_policy(
                &context,
                crate::NativeWindowClosePolicy::CloseContent,
                FIRST_SURFACE,
                &mut budget,
            )
            .unwrap()
    );
    assert!(context.has_requested_repaint_for(&ViewportId::ROOT));
    clear_root_repaint(&context);
    let mut fresh = test_budget();
    assert!(
        native
            .drive_close_policy(
                &context,
                crate::NativeWindowClosePolicy::CloseContent,
                FIRST_SURFACE,
                &mut fresh,
            )
            .unwrap()
    );
    assert_eq!(fresh.remaining_records(), 0);
    native.bridge.with_close_control(|control| {
        assert!(control.decision_requests().next().is_none());
        assert!(!control.has_runnable_policy_work());
        assert_eq!(control.request_for_binding(close.binding()), Some(close));
    });
    assert!(!context.has_requested_repaint_for(&ViewportId::ROOT));
    assert!(pending_decision_items(&mut native, request).is_empty());
    let mut idle = test_budget();
    assert!(
        !native
            .drive_close_policy(
                &context,
                crate::NativeWindowClosePolicy::CloseContent,
                FIRST_SURFACE,
                &mut idle,
            )
            .unwrap()
    );
    assert_eq!(idle.remaining_records(), 256);
    assert!(!context.has_requested_repaint_for(&ViewportId::ROOT));
}

#[test]
fn close_decision_deadline_commits_only_the_processed_prefix() {
    let (mut native, _, request) = pending_content_close(16);
    let calls = Arc::new(AtomicU64::new(0));
    let source = Arc::clone(&calls);
    let clock = NativeMonotonicClock::new(move || {
        if source.fetch_add(1, Ordering::SeqCst) < 8 {
            Duration::ZERO
        } else {
            Duration::from_millis(2)
        }
    });
    let context = egui::Context::default();
    clear_root_repaint(&context);
    let mut budget = NativeInputBudget::new(&clock);
    assert!(
        native
            .drive_close_policy(
                &context,
                crate::NativeWindowClosePolicy::CloseContent,
                FIRST_SURFACE,
                &mut budget,
            )
            .unwrap()
    );
    let completed = 256 - budget.remaining_records();
    assert!((1..16).contains(&completed));
    assert!(budget.is_exhausted());
    assert_eq!(
        pending_decision_items(&mut native, request),
        (2..=17)
            .map(ItemId::new)
            .skip(completed)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        native
            .bridge
            .with_close_control(|control| control.decision_requests().collect::<Vec<_>>()),
        [request]
    );
    assert!(context.has_requested_repaint_for(&ViewportId::ROOT));
    assert!(
        native
            .drive_close_policy(
                &context,
                crate::NativeWindowClosePolicy::CloseContent,
                FIRST_SURFACE,
                &mut test_budget(),
            )
            .unwrap()
    );
    assert!(
        native
            .bridge
            .with_close_control(|control| control.decision_requests().next().is_none())
    );
}

#[test]
fn pending_native_input_keeps_the_same_close_decision_owner() {
    let (mut native, _, request) = pending_content_close(3);
    native
        .session
        .record_native_pointer_sequence((0..2).map(|_| unknown_move()))
        .unwrap();
    let context = egui::Context::default();
    let mut budget = test_budget();
    budget.complete_unsequenced_records(255);
    assert!(
        native
            .drive_close_policy(
                &context,
                crate::NativeWindowClosePolicy::CloseContent,
                FIRST_SURFACE,
                &mut budget,
            )
            .unwrap()
    );
    assert_eq!(budget.remaining_records(), 0);
    assert!(native.bridge.callback_boundary_pending());
    assert_eq!(
        native
            .bridge
            .with_close_control(|control| control.decision_requests().collect::<Vec<_>>()),
        [request]
    );
    let report = commit_poll(&mut native, &mut test_budget());
    assert!(!report.native_input_pending());
    assert_eq!(pending_decision_items(&mut native, request).len(), 3);
    let mut fresh = test_budget();
    assert!(
        native
            .drive_close_policy(
                &context,
                crate::NativeWindowClosePolicy::CloseContent,
                FIRST_SURFACE,
                &mut fresh,
            )
            .unwrap()
    );
    assert_eq!(fresh.remaining_records(), 253);
    assert!(
        native
            .bridge
            .with_close_control(|control| control.decision_requests().next().is_none())
    );
}

#[test]
fn vetoed_close_decision_owner_clears_after_one_successful_frame() {
    let (mut native, close, request) = pending_content_close(3);
    let context = egui::Context::default();
    let mut frame = native.session.begin_host_frame().unwrap();
    let item = frame.pending_close_decisions(request).next().unwrap();
    frame
        .resolve_close(request, item.token(), CloseDecision::Veto)
        .unwrap();
    assert!(frame.pending_close_decisions(request).next().is_none());
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert!(report.inputs().iter().any(|input| matches!(
        input,
        HostInputOutcome::CloseDecisionProcessed { plan: Some(plan), .. }
            if plan.items().iter().any(|item| item.state() == CloseItemDecisionState::Pending)
    )));
    native.finish_internal_report(&context, report).unwrap();
    assert_eq!(
        native
            .bridge
            .with_close_control(|control| control.decision_requests().collect::<Vec<_>>()),
        [request]
    );
    clear_root_repaint(&context);
    assert!(
        native
            .drive_close_policy(
                &context,
                crate::NativeWindowClosePolicy::CloseContent,
                FIRST_SURFACE,
                &mut test_budget(),
            )
            .unwrap()
    );
    native.bridge.with_close_control(|control| {
        assert!(control.decision_requests().next().is_none());
        assert!(!control.has_runnable_policy_work());
        assert_eq!(control.request_for_binding(close.binding()), Some(close));
    });
    assert!(!context.has_requested_repaint_for(&ViewportId::ROOT));
    assert!(
        !native
            .drive_close_policy(
                &context,
                crate::NativeWindowClosePolicy::CloseContent,
                FIRST_SURFACE,
                &mut test_budget(),
            )
            .unwrap()
    );
}

#[test]
fn cancellation_clear_commits_once_after_its_whole_native_interval_settles() {
    for clear_in_first_prefix in [false, true] {
        let mut native = coordinator();
        let (root, binding) = register_roots(&mut native);
        let viewport = ViewportId::from_hash_of("partial-cancellation-model");
        let close = super::close::observe_model_close(
            &mut native,
            binding,
            [root, binding],
            viewport,
            WindowId::from(82),
            41,
        );
        let acknowledgement = super::close::begin_model_cancellation(&mut native, close, viewport);
        native
            .session
            .record_native_pointer_sequence((0..2).map(|_| unknown_move()))
            .unwrap();
        // This is a correlation model with a real core ACK, not a forged fork callback.
        native
            .session
            .publish_native_close(binding, NativeCloseState::Clear, Some(acknowledgement))
            .unwrap();
        assert!(
            native
                .bridge
                .with_close_control(|control| control.mark_clear_recorded(binding))
        );
        if clear_in_first_prefix {
            native
                .session
                .record_native_pointer_sequence((0..256).map(|_| unknown_move()))
                .unwrap();
        }
        let mut budget = if clear_in_first_prefix {
            test_budget()
        } else {
            one_record_budget()
        };
        let first = commit_poll(&mut native, &mut budget);
        assert!(first.native_input_pending());
        assert!(!native.settle_close_control_report(&first).unwrap());
        assert!(
            native
                .bridge
                .with_close_control(|control| control.references_binding(binding))
        );
        assert_eq!(native.close_progress().cancellations_committed(), 0);
        let last = commit_poll(&mut native, &mut test_budget());
        assert!(!last.native_input_pending());
        assert!(native.settle_close_control_report(&last).unwrap());
        assert!(
            !native
                .bridge
                .with_close_control(|control| control.references_binding(binding))
        );
        assert_eq!(native.close_progress().cancellations_committed(), 1);
        assert!(!native.settle_close_control_report(&last).unwrap());
        assert_eq!(native.close_progress().cancellations_committed(), 1);
    }
}

#[test]
fn exhausted_budget_before_claim_yields_without_inventing_a_native_boundary() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    native
        .bridge
        .push_viewport_roster(live_roster([first, second]));
    let context = egui::Context::default();
    let mut budget = test_budget();
    budget.complete_unsequenced_records(256);
    assert!(
        native
            .reduce_callback_prefix(&context, &mut budget)
            .unwrap()
            .is_none()
    );
    assert!(
        native
            .poll_host_frame(&mut budget, |_| NativeReceiverAnswer::Unknown)
            .unwrap()
            .is_none()
    );
    assert!(
        native
            .poll_resolved_host_frame(&context, &mut budget)
            .unwrap()
            .is_none()
    );
    assert!(!native.bridge.callback_boundary_pending());
    assert!(native.has_pending_callback_records());
    assert!(
        native
            .reduce_callback_prefix(&context, &mut test_budget())
            .unwrap()
            .is_some()
    );
    let report = commit_poll(&mut native, &mut test_budget());
    assert!(native.settle_host_frame_report(&report).unwrap());
    assert!(!native.has_pending_callback_records());
}

#[test]
fn host_only_visibility_callbacks_share_the_record_limit_across_passes() {
    let mut native = coordinator();
    let (_, binding) = register_roots(&mut native);
    let viewport = ViewportId::from_hash_of("unsequenced-visibility-budget");
    let window = WindowId::from(83);
    native.bind_viewport(viewport, window, binding).unwrap();
    let callback = NativeViewportVisibilityRecord::for_test(
        eframe::queue_native_viewport_visibility(
            &eframe::egui::Context::default(),
            viewport,
            false,
        ),
        viewport,
        window,
        binding,
        false,
        NativeViewportVisibilityStatus::Dispatched,
    );
    native
        .bridge
        .push_record(HostRecord::ViewportVisibility(callback));
    native
        .bridge
        .push_record(HostRecord::ViewportVisibility(callback));
    let context = egui::Context::default();
    let mut budget = test_budget();
    budget.complete_unsequenced_records(255);
    assert!(
        native
            .reduce_callback_prefix(&context, &mut budget)
            .unwrap()
            .is_some()
    );
    assert_eq!(budget.remaining_records(), 0);
    assert!(!commit_poll(&mut native, &mut budget).native_input_pending());
    assert!(
        native
            .reduce_callback_prefix(&context, &mut budget)
            .unwrap()
            .is_none()
    );
    assert!(native.bridge.front_viewport_visibility().is_some());
    assert!(
        native
            .reduce_callback_prefix(&context, &mut test_budget())
            .unwrap()
            .is_some()
    );
    assert!(native.bridge.front_viewport_visibility().is_none());
}

#[test]
fn shutdown_rearms_a_runnable_close_policy_when_its_budget_is_exhausted() {
    let mut native = coordinator();
    let (root, binding) = register_roots(&mut native);
    let _close = super::close::observe_model_close(
        &mut native,
        binding,
        [root, binding],
        ViewportId::from_hash_of("shutdown-deferred-close"),
        WindowId::from(84),
        42,
    );
    native.quarantine_after_fatal();
    let mut budget = test_budget();
    budget.complete_unsequenced_records(256);
    let advance = native.advance_shutdown_boundary_with_close_policy(
        &egui::Context::default(),
        crate::NativeWindowClosePolicy::Cancel,
        root.surface(),
        &mut budget,
    );
    assert!(advance.errors.is_empty());
    assert!(!advance.transition_source_exhausted);
    assert!(
        native
            .bridge
            .with_close_control(|control| control.unsubmitted_requests().next().is_some())
    );
    assert!(advance.repaint_viewports.contains(&ViewportId::ROOT));
}

#[test]
fn close_policy_expiring_after_entry_still_rearms_the_root() {
    let mut native = coordinator();
    let (root, binding) = register_roots(&mut native);
    let _close = super::close::observe_model_close(
        &mut native,
        binding,
        [root, binding],
        ViewportId::from_hash_of("close-expiring-after-entry"),
        WindowId::from(85),
        43,
    );
    let context = egui::Context::default();
    for _ in 0..8 {
        if !context.has_requested_repaint_for(&ViewportId::ROOT) {
            break;
        }
        context.begin_pass(egui::RawInput::default());
        let mut output = context.end_pass();
        output.textures_delta.clear();
    }
    assert!(!context.has_requested_repaint_for(&ViewportId::ROOT));
    let calls = Arc::new(AtomicU64::new(0));
    let source = Arc::clone(&calls);
    let clock = NativeMonotonicClock::new(move || {
        if source.fetch_add(1, Ordering::SeqCst) < 2 {
            Duration::ZERO
        } else {
            Duration::from_millis(2)
        }
    });
    let mut budget = NativeInputBudget::new(&clock);
    assert!(
        !native
            .drive_close_policy(
                &context,
                crate::NativeWindowClosePolicy::Cancel,
                root.surface(),
                &mut budget
            )
            .unwrap()
    );
    assert!(budget.is_exhausted());
    assert!(
        native
            .bridge
            .with_close_control(|control| control.unsubmitted_requests().next().is_some())
    );
    assert!(context.has_requested_repaint_for(&ViewportId::ROOT));
}

//! Tokenless production rejection and exact close-correlation model tests.
//!
//! These fixtures mint real core close requests, not fork authority. Successful
//! typed Allow/Veto, same-output close suppression, and opaque cancellation
//! callbacks belong to fork framework tests and future real-window smoke tests.
//! Those tests must also cover held-close delivery, exact real-window Destroyed
//! correlation, and cancellation-before-clear ordering across host updates.

use dockspace::runtime::{
    NativeCloseEffectAcknowledgement, NativeSurfaceCloseAction, NativeSurfaceCloseRequest,
};
use eframe::egui::ViewportCommand;

use super::*;
use crate::close_control::NativeWindowClosePolicy;

#[test]
fn default_close_policy_recovers_children_and_accepts_the_external_root() {
    assert_eq!(
        NativeWindowClosePolicy::default().request(false),
        Some(NativeSurfaceCloseAction::RecoverPresentation)
    );
    assert_eq!(
        NativeWindowClosePolicy::default().request(true),
        Some(NativeSurfaceCloseAction::RetainLayout)
    );
}

#[test]
fn rejected_close_fact_keeps_its_ingress_owner_and_raw_record_retryable() {
    let mut native = coordinator();
    let (current, _) = register_roots(&mut native);
    let mut foreign = coordinator();
    let (stale, _) = register_roots(&mut foreign);
    let window = WindowId::from(38);
    native
        .bind_viewport(ViewportId::ROOT, window, current)
        .expect("the successor window remains current");
    native
        .bridge
        .push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            14,
            window,
            Some(ViewportId::ROOT),
            Some(stale),
            WindowEvent::CloseRequested,
        )));

    let context = egui::Context::default();
    for _ in 0..2 {
        let error = native
            .reduce_callback_prefix(&context, &mut test_budget())
            .expect_err("a stale close fact cannot be appended by this provider");
        assert_eq!(error.kind(), crate::NativeRuntimeErrorKind::HostProtocol);
        assert!(
            native
                .bridge
                .with_close_control(|control| control.references_binding(stale))
        );
        assert!(!native.bridge.callback_boundary_pending());
        let pending = native
            .next_window_event()
            .expect("rejected recording retains the raw callback")
            .expect("the same callback remains at the FIFO head");
        assert_eq!(pending.ordinal(), 14);
        assert_eq!(pending.binding(), Some(stale));
        assert_eq!(native.close_progress().requests_retained(), 1);
        assert_eq!(native.viewport_binding(ViewportId::ROOT), Some(current));
    }
}

#[test]
fn tokenless_close_policy_rejects_dispatch_without_waiting_for_later_callbacks() {
    let mut native = coordinator();
    let (binding, survivor) = register_roots(&mut native);
    let close = observe_model_close(
        &mut native,
        binding,
        [binding, survivor],
        ViewportId::ROOT,
        WindowId::from(39),
        15,
    );

    native
        .bridge
        .push_viewport_roster(live_roster([binding, survivor]));
    assert!(
        native
            .drive_close_policy(
                &egui::Context::default(),
                NativeWindowClosePolicy::Cancel,
                survivor.surface(),
                &mut test_budget()
            )
            .expect("a later callback cannot delay the synchronous close policy"),
        "the close request is already ordered after every older callback"
    );
    assert_eq!(
        native
            .bridge
            .with_close_control(|control| control.held_request(close)),
        None
    );
    assert_eq!(
        native
            .bridge
            .with_close_control(|control| control.unresolved_requests().next()),
        Some(close)
    );
    assert!(
        native
            .take_viewport_commands(&egui::Context::default())
            .expect("queued native commands dispatch")
            .is_empty(),
        "failed typed cancellation must not fall back to ordinary Close/CancelClose"
    );

    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("the later callback record remains independently reducible")
            .is_some()
    );
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the later callback boundary begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the later callback boundary settles every surface");
    let report = frame.commit().expect("the later callback boundary commits");
    assert!(
        native
            .settle_host_frame_inputs(report.inputs())
            .expect("the exact native snapshot settles")
    );
}

/// Publishes a real core native-close fact and binds its exact window fixture.
/// `for_test` deliberately supplies no opaque fork request or host authority.
pub(super) fn observe_model_close(
    native: &mut NativeCoordinator,
    binding: NativeSurfaceBinding,
    live_bindings: impl IntoIterator<Item = NativeSurfaceBinding>,
    viewport: ViewportId,
    window: WindowId,
    ordinal: u64,
) -> NativeSurfaceCloseRequest {
    native
        .report_snapshot(
            live_bindings
                .into_iter()
                .map(|binding| (binding, NativeWindowFacts::live())),
            NativeWorkAreaRoster::Unknown,
            test_managed_capabilities(),
        )
        .expect("the managed capability roster queues");
    native
        .bind_viewport(viewport, window, binding)
        .expect("the exact viewport binds");
    native
        .bridge
        .push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            ordinal,
            window,
            Some(viewport),
            Some(binding),
            WindowEvent::CloseRequested,
        )));
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("the exact native close callback records")
            .is_some()
    );
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the native close observation frame begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the native close observation settles every surface");
    let report = frame
        .commit()
        .expect("the native close observation frame commits");
    assert!(
        native
            .settle_close_control_inputs(report.inputs())
            .expect("the exact core close request binds the callback")
    );
    let close = native.bridge.with_close_control(|control| {
        control
            .unresolved_requests()
            .find(|close| close.binding() == binding)
            .expect("the model fixture owns an unresolved core close request")
    });
    assert_eq!(
        native
            .bridge
            .with_close_control(|control| control.held_request(close)),
        None
    );
    close
}

#[test]
fn model_cancellation_retains_binding_until_clear_commits() {
    let mut native = coordinator();
    let (survivor, binding) = register_roots(&mut native);
    let viewport = ViewportId::from_hash_of("cancelled-native-child");
    let window = WindowId::from(41);
    let close = observe_model_close(
        &mut native,
        binding,
        [survivor, binding],
        viewport,
        window,
        17,
    );
    assert_eq!(native.close_progress().requests_retained(), 1);
    assert_eq!(native.close_progress().cancellations_committed(), 0);

    assert!(
        !native
            .bridge
            .with_close_control(|control| control.mark_clear_recorded(binding))
    );
    let acknowledgement = begin_model_cancellation(&mut native, close, viewport);
    assert!(
        native
            .bridge
            .with_close_control(|control| control.references_binding(binding))
    );
    assert!(
        !native
            .bridge
            .with_close_control(|control| control.accept_close(close))
    );
    assert_eq!(
        native
            .bridge
            .with_close_control(|control| control.begin_cancellation(close, acknowledgement)),
        None,
        "the model cannot begin cancellation twice"
    );
    assert_eq!(
        native
            .bridge
            .with_close_control(|control| control.settle_clear(true)),
        0
    );

    // Model-only LiveClear bookkeeping: no fork cancellation callback is forged.
    native
        .session
        .publish_native_close(binding, NativeCloseState::Clear, Some(acknowledgement))
        .expect("the real core acknowledgement authorizes the model clear fact");
    assert!(
        native
            .bridge
            .with_close_control(|control| control.mark_clear_recorded(binding))
    );
    assert!(
        !native
            .bridge
            .with_close_control(|control| control.mark_clear_recorded(binding))
    );
    assert_eq!(
        native
            .bridge
            .with_close_control(|control| control.settle_clear(false)),
        1
    );
    assert!(
        native
            .bridge
            .with_close_control(|control| control.references_binding(binding))
    );
    assert_eq!(native.close_progress().cancellations_committed(), 0);
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the cancellation observation frame begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the cancellation observation settles every surface");
    let report = frame
        .commit()
        .expect("the exact cancellation observation commits");
    assert!(
        native
            .settle_close_control_inputs(report.inputs())
            .expect("the exact LiveClear acknowledgement settles")
    );
    assert!(
        !native
            .bridge
            .with_close_control(|control| control.references_binding(binding))
    );
    assert_eq!(native.close_progress().requests_retained(), 1);
    assert_eq!(native.close_progress().cancellations_committed(), 1);
    assert!(
        !native
            .settle_close_control_inputs(report.inputs())
            .expect("duplicate report settlement does not repeat cancellation progress")
    );
    assert_eq!(native.close_progress().cancellations_committed(), 1);
    assert_eq!(
        native
            .bridge
            .with_close_control(|control| control.settle_clear(true)),
        0
    );
    assert!(native.session.is_current_native_binding(binding));
    assert!(
        native
            .take_viewport_commands(&egui::Context::default())
            .expect("queued native commands dispatch")
            .is_empty()
    );
}

#[test]
fn model_destroyed_during_cancellation_retires_close_sidecar_and_allows_quiescence() {
    let mut native = coordinator();
    let (survivor, binding) = register_roots(&mut native);
    let viewport = ViewportId::from_hash_of("destroyed-before-cancel-callback");
    let window = WindowId::from(45);
    let close = observe_model_close(
        &mut native,
        binding,
        [survivor, binding],
        viewport,
        window,
        19,
    );

    begin_model_cancellation(&mut native, close, viewport);
    assert!(
        native
            .take_viewport_commands(&egui::Context::default())
            .expect("queued native commands dispatch")
            .is_empty()
    );
    assert!(
        native
            .bridge
            .with_close_control(|control| control.references_binding(binding))
    );

    native
        .bridge
        .push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            20,
            window,
            Some(viewport),
            Some(binding),
            WindowEvent::Destroyed,
        )));
    native.bridge.push_viewport_roster(live_roster([survivor]));

    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("the exact destroyed callback records")
            .is_some()
    );
    let mut destroyed = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the destroyed callback frame begins");
    destroyed
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the destroyed callback settles every surface");
    destroyed
        .commit()
        .expect("the destroyed callback frame commits");
    assert!(
        native
            .bridge
            .with_close_control(|control| control.references_binding(binding)),
        "the close correlation remains until the destroyed tombstone commits"
    );

    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("the exact tombstone roster records")
            .is_some()
    );
    let mut roster = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the destroyed roster frame begins");
    roster
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the destroyed roster settles every surface");
    let report = roster
        .commit()
        .expect("the destroyed roster commits the exact tombstone");
    assert!(
        native
            .settle_host_frame_inputs(report.inputs())
            .expect("the exact native snapshot settles")
    );

    let prepared = native
        .prepare_committed_retirements()
        .expect("the committed route prepares atomically")
        .expect("one exact route is ready to retire");
    let mut retirement = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the route retirement frame begins");
    retirement
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the route retirement frame settles every surface");
    retirement
        .commit()
        .expect("the route retirement frame commits");
    assert!(native.commit_retirements(prepared).is_empty());

    assert!(
        !native
            .bridge
            .with_close_control(|control| control.references_binding(binding))
    );
    assert!(
        native
            .try_report_retirement_quiescence()
            .expect("terminal close correlation no longer blocks quiescence")
    );
}

#[test]
fn model_accepted_close_settles_only_once_after_exact_destroyed_observation() {
    let mut native = coordinator();
    let (binding, survivor) = register_roots(&mut native);
    let window = WindowId::from(51);
    let close = observe_model_close(
        &mut native,
        binding,
        [binding, survivor],
        ViewportId::ROOT,
        window,
        23,
    );

    // Establish only the correlation model's accepted state, not host Allow.
    assert!(
        native
            .bridge
            .with_close_control(|control| control.accept_close(close))
    );
    assert!(
        !native
            .bridge
            .with_close_control(|control| control.accept_close(close))
    );
    assert!(
        native
            .take_viewport_commands(&egui::Context::default())
            .expect("queued native commands dispatch")
            .is_empty(),
        "the model transition does not dispatch a host command"
    );
    assert!(
        native
            .bridge
            .with_close_control(|control| control.references_binding(binding)),
        "the accepted close remains correlated until the exact window is destroyed"
    );
    assert!(native.session.is_current_native_binding(binding));

    for (ordinal, event_window, viewport, event_binding) in [
        (23, window, ViewportId::ROOT, binding),
        (24, WindowId::from(52), ViewportId::ROOT, binding),
        (
            24,
            window,
            ViewportId::from_hash_of("unrelated-close"),
            binding,
        ),
        (24, window, ViewportId::ROOT, survivor),
    ] {
        let unrelated = NativeWindowEventRecord::for_test(
            ordinal,
            event_window,
            Some(viewport),
            Some(event_binding),
            WindowEvent::Destroyed,
        );
        assert_eq!(
            native.bridge.with_close_control(
                |control| control.accepted_destroyed_matches(&unrelated, binding)
            ),
            Err(())
        );
        assert!(
            !native.bridge.with_close_control(
                |control| control.settle_accepted_destroyed(&unrelated, binding)
            )
        );
        assert!(
            native
                .bridge
                .with_close_control(|control| control.references_binding(binding))
        );
    }
    let destroyed = NativeWindowEventRecord::for_test(
        24,
        window,
        Some(ViewportId::ROOT),
        Some(binding),
        WindowEvent::Destroyed,
    );
    assert_eq!(
        native
            .bridge
            .with_close_control(|control| control.accepted_destroyed_matches(&destroyed, binding)),
        Ok(true)
    );
    assert!(
        native
            .bridge
            .with_close_control(|control| control.settle_accepted_destroyed(&destroyed, binding))
    );
    assert!(
        !native
            .bridge
            .with_close_control(|control| control.references_binding(binding))
    );
    assert!(
        !native
            .bridge
            .with_close_control(|control| control.settle_accepted_destroyed(&destroyed, binding))
    );
    assert!(
        !native
            .bridge
            .with_close_control(|control| control.accept_close(close))
    );
    assert!(
        !native
            .bridge
            .with_close_control(|control| control.has_pending_work())
    );
    // Settling the model alone neither destroys nor retires a real/core window.
    assert!(native.session.is_current_native_binding(binding));
    assert!(native.session.view().surface(FIRST_SURFACE).is_some());
}

#[test]
fn quarantined_shutdown_rejects_tokenless_allow_without_marking_accepted() {
    let mut native = coordinator();
    let (binding, survivor) = register_roots(&mut native);
    let close = observe_model_close(
        &mut native,
        binding,
        [binding, survivor],
        ViewportId::ROOT,
        WindowId::from(55),
        27,
    );
    let _ = native.quarantine_after_fatal();

    let advance = native.advance_shutdown_boundary_with_close_policy(
        &egui::Context::default(),
        NativeWindowClosePolicy::RetainLayout,
        survivor.surface(),
        &mut test_budget(),
    );

    assert!(advance.progress);
    assert!(advance.errors.is_empty());
    assert_eq!(
        native
            .bridge
            .with_close_control(|control| control.held_request(close)),
        None
    );
    assert_eq!(
        native
            .bridge
            .with_close_control(|control| control.unresolved_requests().next()),
        Some(close)
    );
    assert!(!advance.commands_dispatched);
    assert!(
        native
            .take_viewport_commands(&egui::Context::default())
            .expect("queued native commands dispatch")
            .is_empty()
    );
    assert!(
        native
            .bridge
            .with_close_control(|control| control.references_binding(binding))
    );
}

#[test]
fn model_accepted_close_rejects_destroyed_from_a_different_native_window() {
    let mut native = coordinator();
    let (binding, survivor) = register_roots(&mut native);
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(61);
    let close = observe_model_close(
        &mut native,
        binding,
        [binding, survivor],
        viewport,
        window,
        31,
    );

    assert!(
        native
            .bridge
            .with_close_control(|control| control.accept_close(close))
    );
    assert!(
        !native
            .bridge
            .with_close_control(|control| control.accept_close(close))
    );
    native
        .bridge
        .push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            32,
            WindowId::from(62),
            Some(viewport),
            Some(binding),
            WindowEvent::Destroyed,
        )));

    let ignored = native
        .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
        .expect("unowned destruction is rejected before journal admission");
    assert!(ignored.is_none());
    assert!(!native.bridge.has_pending_callback_records());
    assert!(
        native
            .bridge
            .with_close_control(|control| control.references_binding(binding))
    );
}

#[test]
fn tokenless_allow_and_veto_fail_closed_without_ordinary_close_commands() {
    for (policy, child) in [
        (NativeWindowClosePolicy::RetainLayout, false),
        (NativeWindowClosePolicy::Cancel, false),
        (NativeWindowClosePolicy::RetainLayout, true),
        (NativeWindowClosePolicy::Cancel, true),
    ] {
        let mut native = coordinator();
        let (root, second) = register_roots(&mut native);
        let binding = if child { second } else { root };
        let viewport = if child {
            ViewportId::from_hash_of("tokenless-child")
        } else {
            ViewportId::ROOT
        };
        let close = observe_model_close(
            &mut native,
            binding,
            [root, second],
            viewport,
            WindowId::from(71),
            35,
        );
        let context = egui::Context::default();
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            assert!(
                native
                    .drive_close_policy(ui.ctx(), policy, root.surface(), &mut test_budget())
                    .expect("tokenless dispatch submits an explicit failure result")
            );
        });
        assert!(
            output.viewport_output.values().all(|viewport| {
                !viewport.commands.iter().any(|command| {
                    matches!(
                        command,
                        ViewportCommand::Close | ViewportCommand::CancelClose
                    )
                })
            }),
            "failed typed dispatch must not emit an ordinary egui close command"
        );
        output.textures_delta.clear();
        assert_eq!(
            native
                .bridge
                .with_close_control(|control| control.held_request(close)),
            None
        );
        assert_eq!(
            native
                .bridge
                .with_close_control(|control| control.unresolved_requests().next()),
            Some(close)
        );
        assert!(
            native
                .bridge
                .with_close_control(|control| control.references_binding(binding))
        );
        assert!(native.session.is_current_native_binding(binding));
        assert!(
            native
                .take_viewport_commands(&egui::Context::default())
                .expect("queued native commands dispatch")
                .is_empty()
        );
        assert!(
            native
                .prepare_committed_retirements()
                .expect("failed dispatch cannot prepare a retirement")
                .is_none()
        );
        assert!(
            !native
                .bridge
                .with_close_control(|control| control.mark_clear_recorded(binding)),
            "failed Veto must not begin model cancellation"
        );
        let destroyed = NativeWindowEventRecord::for_test(
            36,
            WindowId::from(71),
            Some(viewport),
            Some(binding),
            WindowEvent::Destroyed,
        );
        assert_eq!(
            native.bridge.with_close_control(
                |control| control.accepted_destroyed_matches(&destroyed, binding)
            ),
            Ok(false),
            "failed Allow must not mark the model accepted"
        );
    }
}

/// Begins only the CloseControl model using a real core cancellation effect.
/// Consuming its acknowledgement does not prove that the fork performed Veto.
pub(super) fn begin_model_cancellation(
    native: &mut NativeCoordinator,
    close: NativeSurfaceCloseRequest,
    viewport: ViewportId,
) -> NativeCloseEffectAcknowledgement {
    native
        .session
        .cancel_native_surface_close(close)
        .expect("the core model requests cancellation");
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the core cancellation model frame begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the core cancellation model settles every surface");
    let mut report = frame.commit().expect("the core cancellation model commits");
    let mut effects = report.take_native_effects().into_iter();
    let effect = effects
        .next()
        .expect("the core emits a real cancellation effect");
    assert!(effects.next().is_none());
    assert_eq!(
        effect.operation(),
        &NativeEffectOperation::ResolveNativeClose {
            close,
            resolution: NativeCloseDisposition::Cancel,
        }
    );
    let Some(NativeEffectAcknowledgement::Close(acknowledgement)) = effect.accepted() else {
        panic!("the real core cancellation effect must yield a close acknowledgement");
    };
    assert_eq!(
        native
            .bridge
            .with_close_control(|control| control.begin_cancellation(close, acknowledgement)),
        Some(viewport)
    );
    acknowledgement
}

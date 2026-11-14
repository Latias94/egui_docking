//! Deferred native close acceptance must yield to cancellation before delivery.

use super::*;
use crate::close::plan::{CloseDecision, ClosePlanPhase};
use crate::platform::effect::{EffectPhase, NativeCloseResolution};
use crate::platform::focus::ViewportActivationRequest;
use crate::platform::frame::PanelFocus;
use crate::runtime::{HostInputOutcome, NativeCloseDisposition};

#[test]
fn emitted_close_accept_revokes_the_ready_scene_before_a_later_paint_can_use_old_coordinates() {
    let (mut session, binding) = managed_tear_off_session();
    session
        .publish_native_close(binding, NativeCloseState::Requested, None)
        .unwrap();
    let observed = commit_managed_frame_report(&mut session);
    let close = match observed.inputs() {
        [HostInputOutcome::NativeCloseObservationApplied { close_requests }] => close_requests[0],
        inputs => panic!("expected one native close edge, got {inputs:?}"),
    };
    session
        .request_native_surface_close(close, NativeSurfaceCloseAction::RetainLayout)
        .unwrap();
    let requested = commit_managed_frame_report(&mut session);
    let plan = match requested.inputs() {
        [HostInputOutcome::NativeSurfaceCloseRequested { plan, .. }] => plan.clone(),
        inputs => panic!("expected one retained-layout close plan, got {inputs:?}"),
    };
    let version = session.version();
    let mut approval = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    for item in plan.items() {
        approval
            .resolve_close(plan.request(), item.token(), CloseDecision::Allow)
            .unwrap();
    }
    approval
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut approved = approval.commit().unwrap();
    let accept = take_only_native_effect(&mut approved);
    assert!(matches!(
        accept.operation(),
        NativeEffectOperation::ResolveNativeClose { .. }
    ));
    assert_eq!(
        session.version(),
        version,
        "close emission preserves layout"
    );
    assert_eq!(
        session
            .engine
            .viewport()
            .viewport(SURFACE)
            .unwrap()
            .lifecycle(),
        crate::platform::registry::ViewportLifecycle::AwaitingDestroyed
    );
    assert!(!session.native_binding_can_report_focus(binding));

    let mut next = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    assert!(
        next.paint_plan(SURFACE).unwrap().is_none(),
        "a close-accepted scene cannot mint a paint with revoked native coordinates"
    );
    next.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    assert!(next.commit().unwrap().take_painted_outputs().is_empty());
    drop(accept);
}

#[test]
fn emitted_close_accept_discards_an_unconfirmed_paint_fallback_and_bootstraps_the_scene() {
    use crate::presentation::scene::{BootstrapSurfaceSceneReason, SurfaceScene};

    let (mut session, binding) =
        native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    session
        .report_managed_native_snapshot(
            [(binding, root_window_facts())],
            NativeWorkAreaRoster::Exact(vec![work_area(1)]),
        )
        .expect("the unpresented root snapshot records");
    commit_managed_frame_report(&mut session);
    measure_all_surfaces(&mut session);
    let (first_output, requirement) = {
        let ready = session
            .engine
            .scene()
            .surface(SURFACE)
            .and_then(SurfaceScene::ready)
            .expect("the first measurement installs an unpresented candidate");
        assert!(ready.paint_fallback().is_none());
        (ready.output_ticket(), ready.stamp().requirement())
    };

    let metrics = UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
        LogicalSize::new(32.0, 24.0).unwrap(),
        120.0,
    )
    .unwrap();
    let mut measured = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the replacement measurement frame begins");
    measured
        .measure_surface(SURFACE, metrics)
        .expect("different metrics replace the candidate under the same requirements");
    assert!(
        measured
            .commit()
            .expect("the unpresented replacement commits")
            .take_painted_outputs()
            .is_empty()
    );
    let ready = session
        .engine
        .scene()
        .surface(SURFACE)
        .and_then(SurfaceScene::ready)
        .expect("the replacement candidate remains Ready before close acceptance");
    assert_eq!(ready.stamp().requirement(), requirement);
    assert_ne!(ready.output_ticket(), first_output);
    assert_eq!(
        ready
            .paint_fallback()
            .map(|fallback| fallback.output_ticket()),
        Some(first_output),
        "the old unpresented candidate is retained without a Presented observation"
    );
    assert!(session.engine.scene().ready_surface(SURFACE).is_none());

    session
        .publish_native_close(binding, NativeCloseState::Requested, None)
        .expect("the unpresented root close observation records");
    let observed = commit_managed_frame_report(&mut session);
    let close = match observed.inputs() {
        [HostInputOutcome::NativeCloseObservationApplied { close_requests }] => {
            let [close] = close_requests.as_slice() else {
                panic!("the root must publish one exact close edge");
            };
            *close
        }
        inputs => panic!("expected one native close edge, got {inputs:?}"),
    };
    session
        .request_native_surface_close(close, NativeSurfaceCloseAction::RetainLayout)
        .expect("the unpresented root enters the retained-layout close protocol");
    let requested = commit_managed_frame_report(&mut session);
    let plan = match requested.inputs() {
        [HostInputOutcome::NativeSurfaceCloseRequested { plan, .. }] => plan.clone(),
        inputs => panic!("expected one retained-layout close plan, got {inputs:?}"),
    };
    assert_eq!(plan.items().len(), 1);
    let mut approval = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the unpresented root approval frame begins");
    for item in plan.items() {
        approval
            .resolve_close(plan.request(), item.token(), CloseDecision::Allow)
            .expect("the exact root close decision is allowed");
    }
    approval
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the pending candidate settles before Accept delivery");
    let mut approved = approval
        .commit()
        .expect("emitting Accept must discard an unconfirmed fallback instead of demoting it");
    let accept = take_only_native_effect(&mut approved);
    assert!(matches!(
        accept.operation(),
        NativeEffectOperation::ResolveNativeClose {
            close: accepted_close,
            resolution: NativeCloseDisposition::Accept,
        } if *accepted_close == close
    ));
    assert_eq!(
        session
            .engine
            .viewport()
            .viewport(SURFACE)
            .unwrap()
            .lifecycle(),
        crate::platform::registry::ViewportLifecycle::AwaitingDestroyed
    );
    assert!(matches!(
        session.engine.scene().surface(SURFACE),
        Some(SurfaceScene::Bootstrap(scene))
            if scene.reason() == BootstrapSurfaceSceneReason::CoordinateAuthorityChanged
    ));

    let mut next = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the next frame begins without a new native snapshot");
    assert!(next.paint_plan(SURFACE).unwrap().is_none());
    next.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    assert!(next.commit().unwrap().take_painted_outputs().is_empty());
    drop(accept);
}

#[test]
fn cancelling_a_deferred_root_close_emits_only_cancel_once_after_report_capacity_returns() {
    let (mut session, binding) = managed_tear_off_session();
    assert!(session.abandoned_native_effects.is_quiescent());
    session.abandoned_native_effects = NativeEffectDropQueue::with_capacity_limit(1)
        .expect("the single effect-report slot preallocates");
    session
        .report_native_global_focus(NativeGlobalFocus::Foreign)
        .expect("foreign native focus records before explicit activation");
    let expected = session.version();
    session
        .native
        .as_mut()
        .expect("the managed native host exists")
        .recorder_mut()
        .record_semantic_input(EngineInput::ActivateViewport {
            expected,
            request: ViewportActivationRequest::explicit(binding.binding, PanelFocus::None),
        })
        .expect("the exact root activation records");
    let mut activation = commit_managed_frame_report(&mut session);
    let held = take_only_native_effect(&mut activation);
    assert!(matches!(
        held.operation(),
        NativeEffectOperation::RequestFocus { binding: target } if *target == binding
    ));
    assert_eq!(session.abandoned_native_effects.available_slots(), Some(0));
    let report_backing = session.abandoned_native_effects.owned_data_capacity_bytes();

    session
        .publish_native_close(binding, NativeCloseState::Requested, None)
        .expect("the ordinary root close observation records");
    let mut observed = commit_managed_frame_report(&mut session);
    assert!(observed.take_native_effects().is_empty());
    let close = match observed.inputs() {
        [HostInputOutcome::NativeCloseObservationApplied { close_requests }] => {
            let [close] = close_requests.as_slice() else {
                panic!("the ordinary root must publish one exact close edge");
            };
            *close
        }
        inputs => panic!("expected the root close observation, got {inputs:?}"),
    };
    assert_eq!(close.binding(), binding);
    session
        .request_native_surface_close(close, NativeSurfaceCloseAction::RetainLayout)
        .expect("the public close request records against that exact edge");
    let mut requested = commit_managed_frame_report(&mut session);
    assert!(requested.take_native_effects().is_empty());
    let plan = match requested.inputs() {
        [
            HostInputOutcome::NativeSurfaceCloseRequested {
                close: requested_close,
                plan,
                ..
            },
        ] if *requested_close == close => plan.clone(),
        inputs => panic!("expected the ordinary root close plan, got {inputs:?}"),
    };
    let request = plan.request();
    assert_eq!(plan.items().len(), 2);

    // A complete accepted backend prefix permits same-frame application votes
    // to commit even while their resulting native Accept cannot be delivered.
    session
        .record_native_pointer(unknown_native_pointer_input(91, NativePointerEvent::Moved))
        .expect("the accepted backend prefix records before the close votes");
    let version = session.version();
    let source_sequence = session.committed_source_sequence;
    let committed_before = session.engine.backend_ingress_committed_through();
    let approval_through = session.native_input_recorded_count();
    assert!(approval_through > committed_before.get());
    for rollback in [true, false] {
        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("the accepted prefix replays before the application votes");
        assert_eq!(
            frame.frame.backend_ingress_through().get(),
            approval_through
        );
        let decisions = frame.pending_close_decisions(request).collect::<Vec<_>>();
        assert_eq!(decisions, plan.items());
        for item in decisions {
            frame
                .resolve_close(request, item.token(), CloseDecision::Allow)
                .expect("the exact ordinary root close decision is allowed");
        }
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("the approval candidate settles the surface roster");
        if rollback {
            drop(frame);
            assert_eq!(session.version(), version);
            assert_eq!(session.committed_source_sequence, source_sequence);
            assert_eq!(
                session.engine.backend_ingress_committed_through(),
                committed_before
            );
            assert_eq!(session.native_input_recorded_count(), approval_through);
            assert_eq!(
                session.engine.close_plan(request).unwrap().phase(),
                ClosePlanPhase::Requested
            );
        } else {
            let mut report = frame
                .commit()
                .expect("the accepted prefix and approval commit under report backpressure");
            assert!(report.take_native_effects().is_empty());
        }
    }
    assert_eq!(session.version(), version);
    assert_eq!(session.committed_source_sequence, source_sequence + 2);
    assert_eq!(
        session.engine.backend_ingress_committed_through().get(),
        approval_through
    );
    assert_eq!(
        session.engine.close_plan(request).unwrap().phase(),
        ClosePlanPhase::Approved
    );
    let accept = session
        .engine
        .viewport()
        .effects()
        .records()
        .find_map(|(id, record)| {
            matches!(
                record.request().effect(),
                PlatformEffect::ResolveNativeClose {
                    request: target,
                    resolution: NativeCloseResolution::Accept,
                    ..
                } if *target == request
            )
            .then_some(id)
        })
        .expect("the approved close retains its original deferred Accept identity");
    let deferred_accept = session
        .engine
        .viewport()
        .effects()
        .record(accept)
        .unwrap()
        .clone();
    assert_eq!(deferred_accept.phase(), EffectPhase::Requested);
    assert!(deferred_accept.delivery().is_none());
    assert!(
        deferred_accept
            .request()
            .native_close_emission_fence()
            .is_none()
    );

    session
        .cancel_native_surface_close(close)
        .expect("the public API cancels the same unresolved root close");
    let cancellation_through = session.native_input_recorded_count();
    assert_eq!(cancellation_through, approval_through + 1);
    let approved_source_sequence = session.committed_source_sequence;
    for rollback in [true, false] {
        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("the original cancellation prefix remains replayable");
        assert_eq!(
            frame.frame.backend_ingress_through().get(),
            cancellation_through
        );
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("the cancellation candidate settles the surface roster");
        if rollback {
            drop(frame);
            assert_eq!(session.version(), version);
            assert_eq!(session.committed_source_sequence, approved_source_sequence);
            assert_eq!(
                session.engine.backend_ingress_committed_through().get(),
                approval_through
            );
            assert_eq!(session.native_input_recorded_count(), cancellation_through);
            assert_eq!(
                session.engine.viewport().effects().record(accept),
                Some(&deferred_accept)
            );
            assert_eq!(
                session.engine.close_plan(request).unwrap().phase(),
                ClosePlanPhase::Approved
            );
        } else {
            let mut report = frame
                .commit()
                .expect("cancellation commits without delivering the obsolete Accept");
            assert!(report.take_native_effects().is_empty());
            assert!(matches!(
                report.inputs(),
                [HostInputOutcome::NativeSurfaceCloseCancellationRequired {
                    close: cancelled_close,
                    plan,
                }] if *cancelled_close == close && plan.request() == request
            ));
        }
    }
    assert_eq!(session.committed_source_sequence, approved_source_sequence);
    assert_eq!(
        session.engine.backend_ingress_committed_through().get(),
        cancellation_through
    );
    let invalidated = session.engine.viewport().effects().record(accept).unwrap();
    assert!(matches!(
        invalidated.phase(),
        EffectPhase::Invalidated { .. }
    ));
    assert!(invalidated.delivery().is_none());
    assert!(
        invalidated
            .request()
            .native_close_emission_fence()
            .is_none()
    );
    let cancel = session
        .engine
        .viewport()
        .effects()
        .records()
        .find_map(|(id, record)| {
            matches!(
                record.request().effect(),
                PlatformEffect::ResolveNativeClose {
                    request: target,
                    resolution: NativeCloseResolution::Cancel,
                    ..
                } if *target == request
            )
            .then_some(id)
        })
        .expect("the cancellation retains one deferred Cancel for the same request");
    let deferred_cancel = session
        .engine
        .viewport()
        .effects()
        .record(cancel)
        .unwrap()
        .clone();
    assert_eq!(deferred_cancel.phase(), EffectPhase::Requested);
    assert!(deferred_cancel.delivery().is_none());
    assert!(
        deferred_cancel
            .request()
            .native_close_emission_fence()
            .is_none()
    );
    assert_eq!(session.abandoned_native_effects.available_slots(), Some(0));

    assert!(held.accepted().is_none());
    assert_eq!(session.abandoned_native_effects.available_slots(), Some(1));
    let mut dropped = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("an empty retry frame begins after the old report slot returns");
    dropped
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the unpublished delivery retry settles the surface roster");
    drop(dropped);
    assert_eq!(session.committed_source_sequence, approved_source_sequence);
    assert_eq!(
        session.engine.backend_ingress_committed_through().get(),
        cancellation_through
    );
    assert_eq!(
        session.engine.viewport().effects().record(cancel),
        Some(&deferred_cancel)
    );

    let mut delivered = commit_managed_frame_report(&mut session);
    let cancellation = take_only_native_effect(&mut delivered);
    assert!(matches!(
        cancellation.operation(),
        NativeEffectOperation::ResolveNativeClose {
            close: cancelled_close,
            resolution: NativeCloseDisposition::Cancel,
        } if *cancelled_close == close
    ));
    let delivery = session
        .engine
        .viewport()
        .effects()
        .record(cancel)
        .unwrap()
        .delivery();
    assert!(delivery.is_some());
    assert_eq!(
        session
            .engine
            .viewport()
            .effects()
            .record(cancel)
            .unwrap()
            .provider(),
        Some(binding.provider)
    );
    for _ in 0..2 {
        let mut report = commit_managed_frame_report(&mut session);
        assert!(
            report.take_native_effects().is_empty(),
            "Cancel emits exactly once"
        );
        assert_eq!(session.native_input_recorded_count(), cancellation_through);
        assert_eq!(session.committed_source_sequence, approved_source_sequence);
        assert_eq!(
            session.engine.backend_ingress_committed_through().get(),
            cancellation_through
        );
        assert_eq!(
            session
                .engine
                .viewport()
                .effects()
                .record(cancel)
                .unwrap()
                .delivery(),
            delivery
        );
    }
    assert!(matches!(
        cancellation.accepted(),
        Some(NativeEffectAcknowledgement::Close(_))
    ));
    let mut report = commit_managed_frame_report(&mut session);
    assert!(report.take_native_effects().is_empty());
    assert_eq!(session.version(), version);
    assert!(session.is_current_native_binding(binding));
    assert_eq!(session.view().item(ITEM).unwrap().surface(), SURFACE);
    assert_eq!(session.view().item(SECOND_ITEM).unwrap().surface(), SURFACE);
    assert!(session.abandoned_native_effects.is_quiescent());
    assert_eq!(
        session.abandoned_native_effects.owned_data_capacity_bytes(),
        report_backing
    );
}

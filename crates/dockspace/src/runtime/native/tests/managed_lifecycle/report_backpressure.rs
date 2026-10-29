//! Accepted native input must drain before deferred effects consume report slots.

use super::*;
use crate::effect::{DispatchFailureReason, EffectPhase};
use std::num::NonZeroUsize;

fn pending_staging_show() -> (
    DockspaceSession,
    NativeSurfaceBinding,
    crate::runtime::NativeEffectRequest,
) {
    let (mut session, root_binding) = programmatic_tear_off_session();
    assert!(session.abandoned_native_effects.is_quiescent());
    session.abandoned_native_effects =
        crate::runtime::native_effect::NativeEffectDropQueue::with_capacity_limit(1)
            .expect("the single effect-report slot preallocates before create");
    let placement =
        PhysicalRect::new(920.0, 120.0, 420.0, 320.0).expect("the requested placement validates");
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the programmatic tear-off frame begins");
    frame
        .move_root_to_new_window_current(ROOT, NativeWindowPlacement::new(placement))
        .expect("the programmatic tear-off stages");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the source presentation remains retained");
    let mut report = frame.commit().expect("the create request commits");
    let create = take_only_native_effect(&mut report);
    let binding = match create.operation() {
        NativeEffectOperation::CreateWindow { binding, .. } => *binding,
        operation => panic!("expected the create request, got {operation:?}"),
    };
    let Some(NativeEffectAcknowledgement::Presentation(acknowledgement)) = create.accepted() else {
        panic!("create must return its exact presentation acknowledgement");
    };
    assert!(session.abandoned_native_effects.is_quiescent());
    session
        .report_managed_native_snapshot(
            [
                (root_binding, root_window_facts()),
                (
                    binding,
                    NativeWindowFacts::live()
                        .with_content_bounds(placement)
                        .with_outer_bounds(placement)
                        .with_native_scale_factor(native_scale())
                        .with_presentation_scale_factor(native_scale())
                        .with_input(NativeWindowInputState::ReceivesInput, None)
                        .with_presentation(
                            NativeWindowPresentationState::Hidden,
                            Some(acknowledgement),
                        )
                        .with_close(NativeCloseState::Clear, None),
                ),
            ],
            NativeWorkAreaRoster::Exact(vec![work_area(1)]),
        )
        .expect("the exact hidden staging window records");
    commit_managed_frame(&mut session);
    let pre_show = paint_staging(
        &mut session,
        binding,
        NativeStagingPresentationPhase::PreShow,
    );
    assert_eq!(pre_show.binding(), binding);
    session
        .report_native_staging_presentation(pre_show, SurfacePresentationResult::Presented)
        .expect("the exact pre-show output records its terminal presentation");
    let mut report = commit_managed_frame_report(&mut session);
    let show = take_only_native_effect(&mut report);
    assert!(matches!(
        show.operation(),
        NativeEffectOperation::ShowWindow { binding: target } if *target == binding
    ));
    assert!(!session.abandoned_native_effects.is_quiescent());
    let native = session
        .native
        .as_mut()
        .expect("the native host is enrolled");
    native
        .reclaim_committed_prefix(&mut session.engine)
        .expect("the setup ingress prefix retires before selecting one record slot");
    native
        .recorder_mut()
        .configure_native_transport_capacity(NonZeroUsize::new(1).unwrap(), 4 << 20)
        .expect("the single input-record slot configures");
    (session, binding, show)
}

fn assert_staging_close_backpressure(abandon_show: bool) {
    let (mut session, binding, show) = pending_staging_show();
    let version = session.version();
    let source_sequence = session.committed_source_sequence;
    let committed_before = session.engine.backend_ingress_committed_through();
    let backing = session.abandoned_native_effects.owned_data_capacity_bytes();
    let (saga_id, create, resource) = session
        .engine
        .viewport()
        .native_create_sagas()
        .map(|(id, saga)| (id, saga.create(), saga.resource()))
        .next()
        .expect("the staging window retains its original create saga");
    let show_effect = session
        .engine
        .viewport()
        .effects()
        .records()
        .find_map(|(id, record)| {
            matches!(
                record.request().effect(),
                PlatformEffect::ShowWindow { binding: target, .. } if *target == binding.binding
            )
            .then_some(id)
        })
        .expect("the held show request has one core identity");
    let retained = session
        .engine
        .viewport()
        .retained_native_staging_resources()
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(retained.len(), 1);
    assert_eq!(retained[0].id(), resource);
    let assert_source_retained = |session: &DockspaceSession| {
        assert_eq!(
            session
                .engine
                .viewport()
                .retained_native_staging_resources()
                .cloned()
                .collect::<Vec<_>>(),
            retained,
            "retry must retain the exact source output authority and move payload"
        );
        session
            .engine
            .viewport()
            .validate_native_staging_resource_conservation()
            .expect("the original staging resource keeps exactly one lifecycle owner");
        assert_eq!(session.view().item(ITEM).unwrap().surface(), SURFACE);
        assert_eq!(session.view().item(SECOND_ITEM).unwrap().surface(), SURFACE);
    };

    session
        .publish_native_close(binding, NativeCloseState::Requested, None)
        .expect("the staging close is accepted before the show failure");
    let close_recorded = session.native_input_recorded_count();
    assert_eq!(close_recorded, committed_before.get() + 1);
    let mut retry = if abandon_show {
        drop(show);
        assert!(session.abandoned_native_effects.has_pending());
        None
    } else {
        let failed = show.dispatch_failed(NativeDispatchFailure::WindowUnavailable);
        let rejected = session
            .report_native_effect_result(failed)
            .expect_err("the accepted close fills the only input-record slot");
        assert_eq!(rejected.kind(), NativeHostErrorKind::InputCapacity);
        let (_, failed) = rejected.into_parts();
        assert_eq!(failed.binding(), binding);
        Some(failed)
    };

    for _ in 0..2 {
        let mut prefix = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("the full accepted close prefix remains replayable");
        assert_eq!(prefix.frame.backend_ingress_through().get(), close_recorded);
        prefix
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("the rollback candidate settles the complete surface roster");
        drop(prefix);
        assert_eq!(session.version(), version);
        assert_eq!(session.committed_source_sequence, source_sequence);
        assert_eq!(
            session.engine.backend_ingress_committed_through(),
            committed_before
        );
        assert_eq!(session.native_input_recorded_count(), close_recorded);
        let saga = session
            .engine
            .viewport()
            .native_create_saga(saga_id)
            .expect("dropping the close candidate preserves the published saga owner");
        assert_eq!(saga.binding(), binding.binding);
        assert_eq!(saga.resource(), resource);
        assert_source_retained(&session);
        if let Some(failed) = retry.take() {
            let rejected = session
                .report_native_effect_result(failed)
                .expect_err("an unpublished prefix cannot free recorder capacity");
            assert_eq!(rejected.kind(), NativeHostErrorKind::InputCapacity);
            let (_, failed) = rejected.into_parts();
            assert_eq!(failed.binding(), binding);
            assert_eq!(failed.result.effect(), show_effect);
            retry = Some(failed);
        }
    }

    let mut prefix = commit_managed_frame_report(&mut session);
    assert!(prefix.take_native_effects().is_empty());
    assert!(prefix.native_admissions().is_empty());
    assert!(prefix.take_painted_native_staging_outputs().is_empty());
    assert_eq!(
        session.engine.backend_ingress_committed_through().get(),
        close_recorded
    );
    assert_eq!(session.native_input_recorded_count(), close_recorded);
    assert!(
        session
            .engine
            .viewport()
            .native_create_saga(saga_id)
            .is_none()
    );
    let retirements = session
        .engine
        .viewport()
        .binding_retirements()
        .map(|(binding, retirement)| (binding, retirement.clone()))
        .collect::<Vec<_>>();
    let [(retired_binding, retirement)] = retirements.as_slice() else {
        panic!("the committed close must transfer to one cleanup owner: {retirements:?}");
    };
    assert_eq!(*retired_binding, binding.binding);
    assert_eq!(
        retirement.origin(),
        crate::frame::BindingRetirementOrigin::NativeCreateAborted { create }
    );
    let crate::frame::BindingRetirementStatus::CleanupRequested { effect: cleanup } =
        retirement.status()
    else {
        panic!("the accepted close must retain its exact compensating request");
    };
    let queued = session.engine.viewport().effects().record(cleanup).unwrap();
    assert_eq!(queued.phase(), EffectPhase::Requested);
    assert!(
        !queued.was_emitted(),
        "report backpressure must defer delivery"
    );
    assert!(matches!(
        queued.request().effect(),
        PlatformEffect::CompensatingClose { binding: target, compensates }
            if *target == binding.binding && *compensates == create
    ));
    assert_source_retained(&session);

    if let Some(failed) = retry {
        session
            .report_native_effect_result(failed)
            .expect("the committed close prefix frees the original failure's recorder slot");
    }
    let mut settlement = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the original show failure enters the next accepted prefix");
    assert_eq!(
        settlement.frame.backend_ingress_through().get(),
        close_recorded + 1
    );
    settlement
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the failure replay settles every surface before rollback");
    drop(settlement);
    assert_eq!(
        session.engine.backend_ingress_committed_through().get(),
        close_recorded
    );
    assert_eq!(session.native_input_recorded_count(), close_recorded + 1);
    assert!(session.abandoned_native_effects.is_quiescent());
    assert!(
        !session
            .engine
            .viewport()
            .effects()
            .record(cleanup)
            .unwrap()
            .was_emitted()
    );
    assert_source_retained(&session);

    let mut settlement = commit_managed_frame_report(&mut session);
    let compensation = take_only_native_effect(&mut settlement);
    assert!(matches!(
        compensation.operation(),
        NativeEffectOperation::CompensatingClose { binding: target } if *target == binding
    ));
    assert!(settlement.native_admissions().is_empty());
    assert!(settlement.take_painted_native_staging_outputs().is_empty());
    assert_eq!(
        session.engine.backend_ingress_committed_through().get(),
        close_recorded + 1
    );
    let failure = session
        .engine
        .viewport()
        .effects()
        .record(show_effect)
        .unwrap();
    assert_eq!(
        failure.phase(),
        EffectPhase::DispatchFailed(if abandon_show {
            DispatchFailureReason::ProviderStopped
        } else {
            DispatchFailureReason::WindowUnavailable
        })
    );
    let delivery = session
        .engine
        .viewport()
        .effects()
        .record(cleanup)
        .unwrap()
        .delivery();
    assert!(delivery.is_some());
    assert_source_retained(&session);
    for _ in 0..2 {
        let mut report = commit_managed_frame_report(&mut session);
        assert!(
            report.take_native_effects().is_empty(),
            "cleanup emits exactly once"
        );
        assert_eq!(session.native_input_recorded_count(), close_recorded + 1);
        assert_eq!(
            session
                .engine
                .viewport()
                .effects()
                .record(cleanup)
                .unwrap()
                .delivery(),
            delivery
        );
        assert_eq!(
            session.engine.viewport().binding_retirements().next(),
            Some((binding.binding, retirement))
        );
        assert_source_retained(&session);
    }
    let Some(NativeEffectAcknowledgement::Close(_acknowledgement)) = compensation.accepted() else {
        panic!("the original compensating close returns its exact close acknowledgement");
    };
    assert!(session.abandoned_native_effects.is_quiescent());
    assert_eq!(
        session.abandoned_native_effects.owned_data_capacity_bytes(),
        backing
    );
    let mut report = commit_managed_frame_report(&mut session);
    assert!(report.take_native_effects().is_empty());
    assert_source_retained(&session);
}

#[test]
fn abandoned_show_does_not_block_an_accepted_staging_close() {
    assert_staging_close_backpressure(true);
}

#[test]
fn rejected_show_result_retries_after_an_accepted_staging_close() {
    assert_staging_close_backpressure(false);
}

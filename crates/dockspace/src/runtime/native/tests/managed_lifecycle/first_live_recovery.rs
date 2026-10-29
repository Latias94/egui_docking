//! Explicit recovery of transferred children before first-live admission.

use super::*;
use crate::frame::NativeCreatePhase;
use crate::ids::SourceSequence;
use crate::runtime::{
    DockspaceSubmittedAction, HostInputOutcome, NativeCreateRecoveryBlockedReason,
    NativeCreateRecoveryStep,
};
use crate::surface_recovery::SurfaceRecoveryBlockedReason;
use crate::transition::{InputOutcome, NativeCreateFirstLiveRecoveryResult};

#[inline(never)]
fn observe_child(
    session: &mut DockspaceSession,
    root: NativeSurfaceBinding,
    child: NativeSurfaceBinding,
    facts: NativeWindowFacts,
) {
    session
        .report_managed_native_snapshot(
            [(root, root_window_facts()), (child, facts)],
            NativeWorkAreaRoster::Exact(vec![work_area(1)]),
        )
        .expect("the exact root and child facts record");
    assert!(
        commit_managed_frame_report(session)
            .native_admissions()
            .is_empty()
    );
}

#[inline(never)]
fn transferred_native_create() -> (DockspaceSession, NativeSurfaceBinding, NativeSurfaceBinding) {
    let (mut session, root) = programmatic_tear_off_session();
    let placement = PhysicalRect::new(920.0, 120.0, 420.0, 320.0).unwrap();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .move_root_to_new_window_current(ROOT, NativeWindowPlacement::new(placement))
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    let create = take_only_native_effect(&mut report);
    let child = create.operation().binding();
    let Some(NativeEffectAcknowledgement::Presentation(created)) = create.accepted() else {
        panic!("the create request must return a presentation acknowledgement");
    };
    let facts = NativeWindowFacts::live()
        .with_content_bounds(placement)
        .with_outer_bounds(placement)
        .with_native_scale_factor(native_scale())
        .with_presentation_scale_factor(native_scale())
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_close(NativeCloseState::Clear, None);
    observe_child(
        &mut session,
        root,
        child,
        facts.with_presentation(NativeWindowPresentationState::Hidden, Some(created)),
    );
    let pre_show = paint_staging(&mut session, child, NativeStagingPresentationPhase::PreShow);
    session
        .report_native_staging_presentation(pre_show, SurfacePresentationResult::Presented)
        .unwrap();
    let mut report = commit_managed_frame_report(&mut session);
    let show = take_only_native_effect(&mut report);
    assert!(matches!(
        show.operation(),
        NativeEffectOperation::ShowWindow { binding } if *binding == child
    ));
    let Some(NativeEffectAcknowledgement::Presentation(shown)) = show.accepted() else {
        panic!("the show request must return a presentation acknowledgement");
    };
    // Visibility must arrive after the exact show acknowledgement's boundary.
    for (state, acknowledgement) in [
        (NativeWindowPresentationState::Hidden, Some(shown)),
        (NativeWindowPresentationState::Visible, None),
    ] {
        observe_child(
            &mut session,
            root,
            child,
            facts.with_presentation(state, acknowledgement),
        );
    }
    let post_show = paint_staging(
        &mut session,
        child,
        NativeStagingPresentationPhase::PostShow,
    );
    session
        .report_native_staging_presentation(post_show, SurfacePresentationResult::Presented)
        .unwrap();
    let mut report = commit_managed_frame_report(&mut session);
    assert!(report.native_admissions().is_empty());
    assert!(report.take_native_effects().is_empty());
    let saga = session
        .engine
        .viewport()
        .native_create_sagas()
        .next()
        .unwrap()
        .1;
    assert_eq!(saga.binding(), child.binding);
    assert!(matches!(
        saga.phase(),
        NativeCreatePhase::AwaitingFirstLivePresentation { .. }
    ));
    assert_eq!(
        session.view().item(ITEM).unwrap().surface(),
        child.surface()
    );
    assert_eq!(
        session.view().item(SECOND_ITEM).unwrap().surface(),
        child.surface()
    );
    assert!(
        session
            .engine
            .workspace()
            .surface(child.surface())
            .unwrap()
            .contained
            .is_empty()
    );
    (session, root, child)
}

#[inline(never)]
fn measure_recovery_host(session: &mut DockspaceSession) -> SurfaceId {
    let host = session
        .engine
        .viewport()
        .native_create_sagas()
        .next()
        .unwrap()
        .1
        .prepared()
        .recovery_obligation()
        .target()
        .host_surface();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .measure_surface(
            host,
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
    let report = frame
        .commit()
        .expect("host measurements commit without presentation");
    assert!(report.native_admissions().is_empty());
    host
}

#[inline(never)]
fn recover_and_commit(
    session: &mut DockspaceSession,
) -> (NativeCreateRecoveryStep, HostFrameReport) {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let step = frame.recover_next_transferred_native_create().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    (
        step,
        frame.commit().expect("the exact recovery attempt commits"),
    )
}

fn assert_recovered_report(step: NativeCreateRecoveryStep, report: &HostFrameReport) {
    let NativeCreateRecoveryStep::Applied {
        submitted,
        surface,
        host,
    } = step
    else {
        panic!("expected an applied recovery, got {step:?}");
    };
    assert_eq!(
        report.submitted_action_outcome(submitted),
        Some(&HostInputOutcome::NativeCreateFirstLiveRecovered { surface, host })
    );
    assert!(report.native_admissions().is_empty());
}

#[test]
fn native_create_recovery_none_preserves_sequence_and_skips_untransferred_candidates() {
    let (mut session, _) = programmatic_tear_off_session();
    let sequence = session.committed_source_sequence;
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    assert_eq!(
        frame.recover_next_transferred_native_create().unwrap(),
        NativeCreateRecoveryStep::None
    );
    assert_eq!(frame.next_source_sequence, sequence);
    frame
        .move_root_to_new_window_current(
            ROOT,
            NativeWindowPlacement::new(PhysicalRect::new(920.0, 120.0, 420.0, 320.0).unwrap()),
        )
        .unwrap();
    assert_eq!(
        frame.recover_next_transferred_native_create().unwrap(),
        NativeCreateRecoveryStep::None
    );
    assert_eq!(frame.next_source_sequence, sequence + 1);
    drop(frame);
    assert_eq!(session.committed_source_sequence, sequence);
}

#[test]
fn native_create_recovery_restores_complete_roster_to_obligation_host_without_child_ready() {
    let (mut session, root, child) = transferred_native_create();
    let host = measure_recovery_host(&mut session);
    assert_eq!(host, root.surface());
    assert!(
        session
            .engine
            .scene()
            .ready_candidate(child.surface())
            .is_none()
    );
    let saga = session
        .engine
        .viewport()
        .native_create_sagas()
        .next()
        .unwrap()
        .1
        .clone();
    let converted = saga
        .prepared()
        .recovery_obligation()
        .target()
        .converted_main()
        .unwrap();
    let items = session.engine.workspace().item_multiset();
    let roots = session
        .engine
        .workspace()
        .roots()
        .map(|(id, record)| (id, record.clone()))
        .collect::<Vec<_>>();
    let retained = session
        .engine
        .viewport()
        .retained_native_staging_resources()
        .cloned()
        .collect::<Vec<_>>();
    let sequence = session.committed_source_sequence;
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let step = frame.recover_next_transferred_native_create().unwrap();
    assert!(
        matches!(step, NativeCreateRecoveryStep::Applied { surface, host: target, .. }
        if surface == child.surface() && target == host)
    );
    assert!(frame.view().surface(child.surface()).is_none());
    for item in [ITEM, SECOND_ITEM] {
        let location = frame.view().item(item).unwrap();
        assert_eq!(
            (location.surface(), location.root(), location.contained()),
            (host, ROOT, Some(converted.floating()))
        );
    }
    assert_eq!(
        frame.recover_next_transferred_native_create().unwrap(),
        NativeCreateRecoveryStep::None
    );
    assert_eq!(frame.next_source_sequence, sequence + 1);
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    assert_recovered_report(step, &report);
    assert_eq!(session.engine.workspace().item_multiset(), items);
    assert_eq!(
        session
            .engine
            .workspace()
            .roots()
            .map(|(id, record)| (id, record.clone()))
            .collect::<Vec<_>>(),
        roots
    );
    assert!(
        session
            .engine
            .viewport()
            .native_create_saga(saga.id())
            .is_none()
    );
    assert_eq!(
        session
            .engine
            .viewport()
            .binding_retirements()
            .next()
            .unwrap()
            .1
            .origin(),
        crate::frame::BindingRetirementOrigin::SurfaceVacated
    );
    assert_eq!(
        session
            .engine
            .viewport()
            .retained_native_staging_resources()
            .cloned()
            .collect::<Vec<_>>(),
        retained
    );
    session
        .engine
        .viewport()
        .validate_native_staging_resource_conservation()
        .unwrap();
    let release = take_only_native_effect(&mut report);
    assert!(
        matches!(release.operation(), NativeEffectOperation::ReleaseChild { binding } if *binding == child)
    );
    let Some(NativeEffectAcknowledgement::Close(closed)) = release.accepted() else {
        panic!("the recovered child must retain its exact destruction acknowledgement");
    };
    assert_eq!(
        session
            .engine
            .viewport()
            .retained_native_staging_resources()
            .cloned()
            .collect::<Vec<_>>(),
        retained
    );
    session
        .report_managed_native_snapshot(
            [(root, root_window_facts())],
            NativeWorkAreaRoster::Exact(vec![work_area(1)]),
        )
        .unwrap();
    assert!(
        commit_managed_frame_report(&mut session)
            .native_admissions()
            .is_empty()
    );
    assert_eq!(
        session
            .engine
            .viewport()
            .retained_native_staging_resources()
            .cloned()
            .collect::<Vec<_>>(),
        retained
    );
    observe_child(
        &mut session,
        root,
        child,
        NativeWindowFacts::destroyed_after(closed),
    );
    assert!(
        session
            .engine
            .viewport()
            .retained_native_staging_resources()
            .next()
            .is_none()
    );
    session
        .engine
        .viewport()
        .validate_native_staging_resource_conservation()
        .unwrap();
}

#[test]
fn native_create_recovery_drop_and_capacity_failure_roll_back_before_cleanup_only_retry() {
    let (mut session, _, child) = transferred_native_create();
    measure_recovery_host(&mut session);
    assert!(session.abandoned_native_effects.is_quiescent());
    session.abandoned_native_effects = NativeEffectDropQueue::with_capacity_limit(0).unwrap();
    let workspace = session.engine.workspace().clone();
    let version = session.version();
    let sequence = session.committed_source_sequence;
    let ingress = session.engine.backend_ingress_committed_through();
    let saga = session
        .engine
        .viewport()
        .native_create_sagas()
        .next()
        .unwrap()
        .1
        .clone();
    let retained = session
        .engine
        .viewport()
        .retained_native_staging_resources()
        .cloned()
        .collect::<Vec<_>>();
    let abandoned = {
        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        let NativeCreateRecoveryStep::Applied { submitted, .. } =
            frame.recover_next_transferred_native_create().unwrap()
        else {
            panic!("the private recovery must apply before rollback");
        };
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        submitted
    };
    assert_eq!(session.engine.workspace(), &workspace);
    assert_eq!(
        session.engine.viewport().native_create_saga(saga.id()),
        Some(&saga)
    );
    assert_eq!(session.committed_source_sequence, sequence);
    assert_eq!(session.version(), version);
    assert_eq!(
        session
            .engine
            .viewport()
            .retained_native_staging_resources()
            .cloned()
            .collect::<Vec<_>>(),
        retained
    );
    let mut mixed = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    assert!(matches!(
        mixed.recover_next_transferred_native_create().unwrap(),
        NativeCreateRecoveryStep::Applied { .. }
    ));
    mixed.select_item_current(SECOND_ITEM).unwrap();
    mixed
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    assert_eq!(
        mixed.commit().unwrap_err().native_kind(),
        Some(NativeHostErrorKind::InputCapacity)
    );
    assert_eq!(session.engine.workspace(), &workspace);
    assert_eq!(
        session.engine.viewport().native_create_saga(saga.id()),
        Some(&saga)
    );
    assert_eq!(session.version(), version);
    assert_eq!(session.committed_source_sequence, sequence);
    assert_eq!(session.engine.backend_ingress_committed_through(), ingress);
    let (step, mut report) = recover_and_commit(&mut session);
    assert_recovered_report(step, &report);
    assert!(report.submitted_action_outcome(abandoned).is_none());
    assert!(report.take_native_effects().is_empty());
    assert_eq!(session.committed_source_sequence, sequence + 1);
    let retirement = session
        .engine
        .viewport()
        .binding_retirements()
        .find_map(|(binding, retirement)| (binding == child.binding).then_some(retirement))
        .unwrap();
    let crate::frame::BindingRetirementStatus::CleanupRequested { effect } = retirement.status()
    else {
        panic!("cleanup must remain owned by its original ledger");
    };
    let record = session.engine.viewport().effects().record(effect).unwrap();
    assert_eq!(record.phase(), crate::effect::EffectPhase::Requested);
    assert!(!record.was_emitted());
    assert!(record.delivery().is_none());
    assert_eq!(
        session
            .engine
            .viewport()
            .retained_native_staging_resources()
            .cloned()
            .collect::<Vec<_>>(),
        retained
    );
    session
        .engine
        .viewport()
        .validate_native_staging_resource_conservation()
        .unwrap();
    let (step, mut report) = recover_and_commit(&mut session);
    assert_eq!(step, NativeCreateRecoveryStep::None);
    assert!(report.take_native_effects().is_empty());
    assert_eq!(session.committed_source_sequence, sequence + 1);
    session.abandoned_native_effects = NativeEffectDropQueue::with_capacity_limit(1).unwrap();
    let mut report = commit_managed_frame_report(&mut session);
    let release = take_only_native_effect(&mut report);
    assert!(
        matches!(release.operation(), NativeEffectOperation::ReleaseChild { binding } if *binding == child)
    );
    assert!(matches!(
        release.accepted(),
        Some(NativeEffectAcknowledgement::Close(_))
    ));
}

#[test]
fn native_create_recovery_blocked_host_requires_committed_measurements_without_presentation() {
    let (mut session, root, child) = transferred_native_create();
    assert!(
        session
            .engine
            .scene()
            .ready_candidate(root.surface())
            .is_none()
    );
    let workspace = session.engine.workspace().clone();
    let saga = session
        .engine
        .viewport()
        .native_create_sagas()
        .next()
        .unwrap()
        .1
        .clone();
    let retained = session
        .engine
        .viewport()
        .retained_native_staging_resources()
        .cloned()
        .collect::<Vec<_>>();
    let sequence = session.committed_source_sequence;
    for attempt in 1..=2 {
        let (step, mut report) = recover_and_commit(&mut session);
        let NativeCreateRecoveryStep::Blocked {
            submitted,
            surface,
            host,
            reason,
        } = step
        else {
            panic!("deferred surfaces must leave host recovery blocked: {step:?}");
        };
        assert_eq!(
            (surface, host, reason),
            (
                child.surface(),
                root.surface(),
                NativeCreateRecoveryBlockedReason::HostAuthorityUnavailable
            )
        );
        assert_eq!(
            report.submitted_action_outcome(submitted),
            Some(&HostInputOutcome::NativeCreateFirstLiveRecoveryBlocked {
                surface,
                host,
                reason
            })
        );
        assert!(report.native_admissions().is_empty());
        assert!(report.take_native_effects().is_empty());
        assert_eq!(session.committed_source_sequence, sequence + attempt);
        assert_eq!(session.engine.workspace(), &workspace);
        assert_eq!(
            session.engine.viewport().native_create_saga(saga.id()),
            Some(&saga)
        );
        assert!(
            session
                .engine
                .viewport()
                .binding_retirements()
                .next()
                .is_none()
        );
        assert_eq!(
            session
                .engine
                .viewport()
                .retained_native_staging_resources()
                .cloned()
                .collect::<Vec<_>>(),
            retained
        );
    }
    // Only commit measurements here; no surface output is painted or presented.
    measure_all_surfaces(&mut session);
    assert!(
        session
            .engine
            .scene()
            .ready_candidate(root.surface())
            .is_some()
    );
    assert_eq!(session.engine.workspace(), &workspace);
    assert_eq!(session.committed_source_sequence, sequence + 2);
    let (step, mut report) = recover_and_commit(&mut session);
    assert_recovered_report(step, &report);
    let release = take_only_native_effect(&mut report);
    assert!(matches!(
        release.accepted(),
        Some(NativeEffectAcknowledgement::Close(_))
    ));
}

#[test]
fn native_create_recovery_foreign_binding_cannot_consume_same_saga_authority() {
    let (mut session, root, child) = transferred_native_create();
    measure_recovery_host(&mut session);
    let workspace = session.engine.workspace().clone();
    let saga = session
        .engine
        .viewport()
        .native_create_sagas()
        .next()
        .unwrap()
        .1
        .clone();
    let retained = session
        .engine
        .viewport()
        .retained_native_staging_resources()
        .cloned()
        .collect::<Vec<_>>();
    let sequence = session.committed_source_sequence;
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .append(EngineInput::RecoverNativeCreateFirstLive {
            expected: frame.version(),
            saga: saga.id(),
            binding: root.binding,
        })
        .unwrap();
    let submitted = DockspaceSubmittedAction::new(
        frame.frame.presentation_attempt(),
        SourceSequence::new(frame.next_source_sequence),
    );
    assert!(
        matches!(frame.frame.last_input_outcome(), Some(InputOutcome::NativeCreateFirstLiveRecovery {
        saga: id, binding, host, result: NativeCreateFirstLiveRecoveryResult::Blocked(SurfaceRecoveryBlockedReason::ObligationMismatch)
    }) if *id == saga.id() && *binding == root.binding && *host == root.surface())
    );
    assert_eq!(frame.frame.view().workspace(), &workspace);
    assert_eq!(
        frame.frame.view().viewport().native_create_saga(saga.id()),
        Some(&saga)
    );
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    assert_eq!(
        report.submitted_action_outcome(submitted),
        Some(&HostInputOutcome::NativeCreateFirstLiveRecoveryBlocked {
            surface: root.surface(),
            host: root.surface(),
            reason: NativeCreateRecoveryBlockedReason::AuthorityChanged,
        })
    );
    assert!(report.native_admissions().is_empty());
    assert!(report.take_native_effects().is_empty());
    assert_eq!(session.engine.workspace(), &workspace);
    assert_eq!(
        session.engine.viewport().native_create_saga(saga.id()),
        Some(&saga)
    );
    assert_eq!(session.committed_source_sequence, sequence + 1);
    assert!(
        session
            .engine
            .viewport()
            .binding_retirements()
            .next()
            .is_none()
    );
    assert_eq!(
        session
            .engine
            .viewport()
            .retained_native_staging_resources()
            .cloned()
            .collect::<Vec<_>>(),
        retained
    );
    session
        .engine
        .viewport()
        .validate_native_staging_resource_conservation()
        .unwrap();
    let (step, mut report) = recover_and_commit(&mut session);
    assert_recovered_report(step, &report);
    assert!(
        matches!(step, NativeCreateRecoveryStep::Applied { surface, .. } if surface == child.surface())
    );
    let release = take_only_native_effect(&mut report);
    assert!(matches!(
        release.accepted(),
        Some(NativeEffectAcknowledgement::Close(_))
    ));
}

#[test]
fn native_create_recovery_late_first_live_terminal_cannot_admit_vacated_child() {
    let (mut session, _, child) = transferred_native_create();
    measure_all_surfaces(&mut session);
    let mut paint = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let receipt = execute_surface_program(&mut paint, child.surface());
    paint.confirm_surface_painted(receipt).unwrap();
    paint
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = paint.commit().unwrap();
    assert!(report.native_admissions().is_empty());
    let outputs = report.take_painted_outputs();
    let [output] = outputs.try_into().unwrap_or_else(|outputs: Vec<_>| {
        panic!("expected one pending first-live output, got {outputs:?}")
    });
    let retained = session
        .engine
        .viewport()
        .retained_native_staging_resources()
        .cloned()
        .collect::<Vec<_>>();
    let (step, mut report) = recover_and_commit(&mut session);
    assert_recovered_report(step, &report);
    let release = take_only_native_effect(&mut report);
    assert!(matches!(
        release.accepted(),
        Some(NativeEffectAcknowledgement::Close(_))
    ));
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .expect("the late terminal still settles its exact output owner");
    let mut report = commit_managed_frame_report(&mut session);
    assert!(report.native_admissions().is_empty());
    assert!(report.take_native_effects().is_empty());
    assert!(
        session
            .engine
            .workspace()
            .surface(child.surface())
            .is_none()
    );
    assert!(
        session
            .engine
            .viewport()
            .native_create_sagas()
            .next()
            .is_none()
    );
    for item in [ITEM, SECOND_ITEM] {
        assert_eq!(session.view().item(item).unwrap().surface(), SURFACE);
    }
    assert_eq!(
        session
            .engine
            .viewport()
            .retained_native_staging_resources()
            .cloned()
            .collect::<Vec<_>>(),
        retained
    );
    session
        .engine
        .viewport()
        .validate_native_staging_resource_conservation()
        .unwrap();
}

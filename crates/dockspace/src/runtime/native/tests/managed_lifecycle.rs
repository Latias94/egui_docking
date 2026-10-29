use super::*;

mod budget;
mod capacity;
mod deferred_close;
mod overload;
mod report_backpressure;

use crate::engine::EngineInput;
use crate::geometry::PhysicalPoint;
use crate::graph::ContainedFloating;
use crate::model::{
    DockAnchor, DockPlacement, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, FloatingPresentationId,
};
#[cfg(feature = "serde")]
use crate::runtime::{DockspaceDocumentBootstrap, DockspaceDocumentId};
use crate::runtime::{
    DockspaceHostFrame, DockspacePresentationCommandKind, DockspacePresentationCommandUnavailable,
    DockspaceReceiverDescriptor, HostFrameReport, NativeEffectAcknowledgement,
    NativeEffectOperation, NativeStagingPresentationPhase, NativeWindowPlacement,
};

const SECOND_ITEM: ItemId = ItemId::new(2);
const ROOT_ORIGIN_X: f64 = 100.0;
const ROOT_ORIGIN_Y: f64 = 80.0;

fn execute_surface_program(
    frame: &mut DockspaceHostFrame<'_>,
    surface: SurfaceId,
) -> SurfaceProgramExecutionReceipt {
    frame
        .paint_plan(surface)
        .expect("managed native paint plan lookup succeeds")
        .expect("managed native surface candidate is paintable")
        .execute_surface_program(|_| Ok::<(), ()>(()))
        .expect("managed native renderer executes the complete surface program")
}

#[cfg(feature = "serde")]
#[test]
fn joined_document_restore_rebases_after_a_workspace_mutating_native_prefix() {
    const DOCUMENT: DockspaceDocumentId = DockspaceDocumentId::from_bytes([0x5a; 16]);

    fn persistent_session() -> (DockspaceSession, ItemId, ItemId) {
        let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
        let first = bootstrap
            .ensure_item("pane:first")
            .expect("the first item allocates");
        let second = bootstrap
            .ensure_item("pane:second")
            .expect("the second item allocates");
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([first, second])),
        )])
        .expect("the persistent native layout validates");
        let session =
            DockspaceSession::from_persistent_layout(layout, DockPolicy::default(), bootstrap)
                .expect("the persistent document session initializes");
        (session, first, second)
    }

    let (mut source, source_first, source_second) = persistent_session();
    let bytes = source
        .save_document_json()
        .expect("the source document encodes");

    let (mut target, first, second) = persistent_session();
    assert_eq!((first, second), (source_first, source_second));
    target
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .expect("the managed native host enrolls");
    target
        .configure_managed_native_capabilities(full_managed_capabilities())
        .expect("the managed backend capabilities configure");
    let prepared = target
        .prepare_document_restore_json(&bytes, |document, key| {
            document == DOCUMENT && matches!(key, "pane:first" | "pane:second")
        })
        .expect("the native document restore prepares once");

    let expected = target.version();
    target
        .native
        .as_mut()
        .expect("the managed native state exists")
        .recorder_mut()
        .record_semantic_input(EngineInput::SelectItem {
            expected,
            item: second,
        })
        .expect("the native semantic prefix records");

    {
        let restore = target
            .begin_native_document_restore_frame(&prepared, |_| NativeReceiverAnswer::Unknown)
            .expect("the rollbackable joined restore frame begins");
        assert!(restore.version() != expected);
    }
    assert_eq!(target.version(), expected);
    assert!(
        target
            .view()
            .item(first)
            .is_some_and(|item| item.is_selected())
    );

    let mut restore = target
        .begin_native_document_restore_frame(&prepared, |_| NativeReceiverAnswer::Unknown)
        .expect("the joined restore frame begins after the native prefix");
    assert!(restore.version() != expected);
    restore
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("test bounds validate"),
                LogicalSize::new(32.0, 24.0).expect("test minimum validates"),
                80.0,
            )
            .expect("test metrics validate"),
        )
        .expect("the restored candidate measures");
    let report = restore
        .commit()
        .expect("the restore publishes after the native prefix");

    assert!(matches!(
        report.inputs(),
        [super::super::super::HostInputOutcome::DocumentRestored { .. }]
    ));
    assert!(
        target
            .view()
            .item(first)
            .is_some_and(|item| item.is_selected())
    );
}

#[test]
fn joined_layout_reset_retries_after_a_workspace_mutating_native_prefix() {
    let layout = |root| {
        DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(root, DockspaceNode::central_tabs([ITEM, SECOND_ITEM])),
        )])
        .expect("the joined reset layout validates")
    };
    // The default's root is below the live frontier and must receive a fresh ID.
    let mut target = DockspaceSession::from_layout(layout(RootId::new(2)), DockPolicy::default())
        .expect("the layout-only session initializes");
    target
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .expect("the managed native host enrolls");
    target
        .configure_managed_native_capabilities(full_managed_capabilities())
        .expect("the managed backend capabilities configure");
    let prepared = target
        .prepare_layout_reset(layout(ROOT))
        .expect("the reset prepares against the published version");
    let expected = target.version();
    let committed = target.engine.backend_ingress_committed_through();
    let source_sequence = target.committed_source_sequence;
    let prefix = target
        .native
        .as_mut()
        .expect("the managed native state exists")
        .recorder_mut()
        .record_semantic_input(EngineInput::SelectItem {
            expected,
            item: SECOND_ITEM,
        })
        .expect("the workspace-mutating native prefix records");
    let recorded = target.native_input_recorded_count();

    let candidate_version = {
        let reset = target
            .begin_native_layout_reset_frame(&prepared, |_| NativeReceiverAnswer::Unknown)
            .expect("the rollbackable joined reset begins");
        assert_ne!(reset.version().epoch(), expected.epoch());
        assert_eq!(reset.view().item(ITEM).unwrap().root(), RootId::new(3));
        assert!(reset.view().item(ITEM).unwrap().is_selected());
        assert_eq!(reset.frame.backend_ingress_through(), prefix);
        reset.version()
    };
    assert_eq!(target.version(), expected);
    assert_eq!(target.engine.backend_ingress_committed_through(), committed);
    assert_eq!(target.committed_source_sequence, source_sequence);
    assert_eq!(target.native_input_recorded_count(), recorded);
    assert_eq!(target.view().item(ITEM).unwrap().root(), RootId::new(2));
    assert!(target.view().item(ITEM).unwrap().is_selected());

    let mut reset = target
        .begin_native_layout_reset_frame(&prepared, |_| NativeReceiverAnswer::Unknown)
        .expect("the same prepared reset replays the unchanged native prefix");
    assert_eq!(reset.version(), candidate_version);
    assert_eq!(reset.frame.backend_ingress_through(), prefix);
    assert_eq!(reset.view().item(ITEM).unwrap().root(), RootId::new(3));
    reset
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("test bounds validate"),
                LogicalSize::new(32.0, 24.0).expect("test minimum validates"),
                80.0,
            )
            .expect("test metrics validate"),
        )
        .expect("the reset candidate measures");
    let report = reset.commit().expect("the joined reset commits once");
    let [super::super::super::HostInputOutcome::LayoutReset { previous, current }] =
        report.inputs()
    else {
        panic!("the reset must supersede the prefix's product-visible result");
    };
    assert_eq!(previous.epoch(), expected.epoch());
    assert_ne!(
        *previous, expected,
        "the native selection must advance the prefix version"
    );
    assert_eq!(*current, candidate_version);
    assert_eq!(target.engine.backend_ingress_committed_through(), prefix);
    assert_eq!(target.committed_source_sequence, source_sequence + 1);
    assert_eq!(target.native_input_recorded_count(), recorded);
    assert!(target.view().item(ITEM).unwrap().is_selected());

    let mut next = target
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the next native frame starts after the committed prefix");
    next.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the measured layout remains paintable");
    let report = next
        .commit()
        .expect("the next frame commits without replay");
    assert!(report.inputs().is_empty());
    assert_eq!(target.version(), candidate_version);
    assert_eq!(target.engine.backend_ingress_committed_through(), prefix);
    assert_eq!(target.committed_source_sequence, source_sequence + 1);
    assert_eq!(target.native_input_recorded_count(), recorded);
}

#[test]
fn programmatic_native_root_tear_off_starts_the_shared_create_saga() {
    let (mut session, _) = programmatic_tear_off_session();
    let before = session.version();
    let placement = PhysicalRect::new(920.0, 120.0, 420.0, 320.0)
        .expect("the requested outer placement validates");

    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the programmatic tear-off frame begins");
    frame
        .move_root_to_new_window_current(ROOT, NativeWindowPlacement::new(placement))
        .expect("the revision-bound root tear-off stages");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the source surface retains its exact presentation");
    let mut report = frame.commit().expect("the native create request commits");
    let effects = report.take_native_effects();
    assert_eq!(
        effects.len(),
        1,
        "programmatic tear-off produced the wrong effect set; inputs: {:?}",
        report.inputs()
    );
    let request = effects.into_iter().next().expect("one effect was asserted");
    let child_binding = match request.operation() {
        NativeEffectOperation::CreateWindow {
            binding,
            placement: requested,
            ..
        } if *requested == placement => *binding,
        operation => panic!("programmatic tear-off emitted the wrong effect: {operation:?}"),
    };

    assert_eq!(
        report.inputs(),
        &[super::super::super::HostInputOutcome::ProductActionApplied(
            crate::model::DockspaceActionOutcome::NativeRootTearOffRequested {
                root: ROOT,
                source_surface: SURFACE,
                target_surface: child_binding.surface(),
                items: vec![ITEM, SECOND_ITEM],
            },
        )]
    );
    assert_eq!(session.version(), before);
    assert_eq!(
        session.view().item(ITEM).map(|item| item.surface()),
        Some(SURFACE)
    );
    assert_eq!(
        session.view().item(SECOND_ITEM).map(|item| item.surface()),
        Some(SURFACE)
    );
}

#[test]
fn duplicate_programmatic_native_root_tear_off_is_rejected_atomically() {
    let (mut session, _) = programmatic_tear_off_session();
    let before = session.version();
    let placement = NativeWindowPlacement::new(
        PhysicalRect::new(920.0, 120.0, 420.0, 320.0)
            .expect("the requested outer placement validates"),
    );

    let mut first = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the first programmatic tear-off frame begins");
    first
        .move_root_to_new_window_current(ROOT, placement)
        .expect("the first programmatic tear-off stages");
    first
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the first source presentation remains retained");
    let mut first_report = first.commit().expect("the first native create commits");
    let pending_request = first_report
        .take_native_effects()
        .into_iter()
        .next()
        .expect("the first create request remains outstanding");

    let mut duplicate = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the duplicate programmatic tear-off frame begins");
    duplicate
        .move_root_to_new_window_current(ROOT, placement)
        .expect("the duplicate programmatic tear-off stages structurally");
    duplicate
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the duplicate frame retains the source presentation");
    let mut duplicate_report = duplicate
        .commit()
        .expect("the duplicate tear-off rejection commits");

    assert_eq!(
        duplicate_report.inputs(),
        &[
            super::super::super::HostInputOutcome::ProductActionRejected(
                crate::model::DockspaceActionRejection::Conflict,
            )
        ]
    );
    assert!(duplicate_report.take_native_effects().is_empty());
    assert_eq!(session.version(), before);
    assert_eq!(
        session.view().item(ITEM).map(|item| item.surface()),
        Some(SURFACE)
    );
    assert_eq!(
        session.view().item(SECOND_ITEM).map(|item| item.surface()),
        Some(SURFACE)
    );

    drop(pending_request);
}

#[test]
fn native_create_cleanup_cancels_only_one_create_from_the_current_candidate() {
    let (mut session, _) = programmatic_tear_off_session();
    let source = session.engine.workspace().clone();
    let source_sequence = session.committed_source_sequence;
    let placement = NativeWindowPlacement::new(
        PhysicalRect::new(920.0, 120.0, 420.0, 320.0).expect("the requested placement validates"),
    );
    assert!(
        session
            .engine
            .viewport()
            .native_create_sagas()
            .next()
            .is_none()
    );

    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the cleanup frame begins");
    assert!(!frame.cancel_next_native_create().unwrap());
    assert_eq!(frame.next_source_sequence, source_sequence);
    for root in [RootId::new(2), ROOT] {
        frame
            .move_root_to_new_window_current(root, placement)
            .unwrap();
    }
    let creates = frame
        .frame
        .view()
        .viewport()
        .native_create_sagas()
        .map(|(id, saga)| (id, saga.binding()))
        .collect::<Vec<_>>();
    assert_eq!(creates.len(), 2);

    assert!(frame.cancel_next_native_create().unwrap());
    assert_eq!(frame.next_source_sequence, source_sequence + 3);
    let remaining = frame
        .frame
        .view()
        .viewport()
        .native_create_sagas()
        .map(|(id, saga)| (id, saga.binding()))
        .collect::<Vec<_>>();
    assert_eq!(remaining, creates[1..]);
    assert_eq!(frame.frame.view().workspace(), &source);
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame
        .commit()
        .expect("only the selected cancellation publishes");
    let create = take_only_native_effect(&mut report);
    assert!(matches!(
        create.operation(),
        NativeEffectOperation::CreateWindow { binding, .. } if binding.binding == creates[1].1
    ));
    assert!(
        session
            .engine
            .viewport()
            .native_create_saga(creates[0].0)
            .is_none()
    );
    assert!(
        session
            .engine
            .viewport()
            .native_create_saga(creates[1].0)
            .is_some()
    );
    assert_eq!(session.engine.workspace(), &source);
}

#[test]
fn native_create_cleanup_commits_under_report_backpressure_and_compensates_once() {
    let (mut session, root_binding) = programmatic_tear_off_session();
    assert!(session.abandoned_native_effects.is_quiescent());
    session.abandoned_native_effects = NativeEffectDropQueue::with_capacity_limit(1)
        .expect("the single effect-report slot preallocates");
    let source = session.engine.workspace().clone();
    let placement =
        PhysicalRect::new(920.0, 120.0, 420.0, 320.0).expect("the requested placement validates");
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the native create frame begins");
    frame
        .move_root_to_new_window_current(ROOT, NativeWindowPlacement::new(placement))
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().expect("the native create publishes");
    let create = take_only_native_effect(&mut report);
    let child = create.operation().binding();
    let Some(NativeEffectAcknowledgement::Presentation(acknowledgement)) = create.accepted() else {
        panic!("create must return its exact presentation acknowledgement");
    };
    session
        .report_managed_native_snapshot(
            [
                (root_binding, root_window_facts()),
                (
                    child,
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
        .expect("the hidden child snapshot records");
    commit_managed_frame(&mut session);
    let pre_show = paint_staging(&mut session, child, NativeStagingPresentationPhase::PreShow);
    session
        .report_native_staging_presentation(pre_show, SurfacePresentationResult::Presented)
        .expect("the pre-show terminal records");
    let mut report = commit_managed_frame_report(&mut session);
    let show = take_only_native_effect(&mut report);
    assert!(matches!(
        show.operation(),
        NativeEffectOperation::ShowWindow { binding } if *binding == child
    ));
    let (saga, before) = session
        .engine
        .viewport()
        .native_create_sagas()
        .map(|(id, saga)| (id, saga.clone()))
        .next()
        .expect("the hidden child retains its exact saga");
    let retained = session
        .engine
        .viewport()
        .retained_native_staging_resources()
        .cloned()
        .collect::<Vec<_>>();
    let source_sequence = session.committed_source_sequence;
    let version = session.version();
    let committed = session.engine.backend_ingress_committed_through();

    let mut rollback = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the rollbackable cleanup frame begins");
    assert!(rollback.cancel_next_native_create().unwrap());
    assert!(
        rollback
            .frame
            .view()
            .viewport()
            .native_create_saga(saga)
            .is_none()
    );
    assert_eq!(rollback.frame.view().workspace(), &source);
    rollback
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    drop(rollback);

    assert_eq!(
        session.engine.viewport().native_create_saga(saga),
        Some(&before)
    );
    assert!(
        session
            .engine
            .viewport()
            .binding_retirements()
            .next()
            .is_none()
    );
    assert_eq!(session.committed_source_sequence, source_sequence);
    assert_eq!(session.version(), version);
    assert_eq!(session.engine.workspace(), &source);
    assert_eq!(
        session
            .engine
            .viewport()
            .retained_native_staging_resources()
            .cloned()
            .collect::<Vec<_>>(),
        retained
    );

    assert_eq!(session.abandoned_native_effects.available_slots(), Some(0));
    let mut blocked = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the mixed cleanup and application frame begins");
    assert!(blocked.cancel_next_native_create().unwrap());
    blocked.select_item_current(SECOND_ITEM).unwrap();
    blocked
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let error = blocked
        .commit()
        .expect_err("ordinary application input still requires effect delivery capacity");
    assert_eq!(
        error.native_kind(),
        Some(NativeHostErrorKind::InputCapacity)
    );
    assert_eq!(
        session.engine.viewport().native_create_saga(saga),
        Some(&before)
    );
    assert_eq!(session.committed_source_sequence, source_sequence);
    assert_eq!(session.version(), version);
    assert_eq!(session.engine.workspace(), &source);
    assert!(
        session
            .engine
            .viewport()
            .binding_retirements()
            .next()
            .is_none()
    );
    let mut retry = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the next frame retries the same cleanup");
    assert_eq!(retry.frame.backend_ingress_through(), committed);
    assert!(retry.cancel_next_native_create().unwrap());
    assert!(!retry.cancel_next_native_create().unwrap());
    assert_eq!(retry.next_source_sequence, source_sequence + 1);
    retry
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = retry.commit().expect("the exact cancellation commits");
    assert!(report.presentation_transitions().is_empty());
    assert!(report.take_native_effects().is_empty());
    assert_eq!(session.abandoned_native_effects.available_slots(), Some(0));
    assert!(session.engine.viewport().native_create_saga(saga).is_none());
    assert_eq!(session.engine.workspace(), &source);
    assert_eq!(session.committed_source_sequence, source_sequence + 1);
    let retirement = session
        .engine
        .viewport()
        .binding_retirements()
        .find_map(|(binding, retirement)| (binding == child.binding).then_some(retirement))
        .expect("the exact child retains its cleanup owner");
    assert_eq!(
        retirement.origin(),
        crate::frame::BindingRetirementOrigin::NativeCreateAborted {
            create: before.create()
        }
    );
    let crate::frame::BindingRetirementStatus::CleanupRequested { effect } = retirement.status()
    else {
        panic!("the cancelled child must retain one exact compensation");
    };
    let queued = session.engine.viewport().effects().record(effect).unwrap();
    assert_eq!(queued.phase(), crate::effect::EffectPhase::Requested);
    assert!(!queued.was_emitted());
    assert!(queued.delivery().is_none());
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

    let Some(NativeEffectAcknowledgement::Presentation(_)) = show.accepted() else {
        panic!("the show dispatch releases its original report slot");
    };
    assert_eq!(session.abandoned_native_effects.available_slots(), Some(1));
    let mut delivery_report = commit_managed_frame_report(&mut session);
    let compensation = take_only_native_effect(&mut delivery_report);
    assert!(matches!(
        compensation.operation(),
        NativeEffectOperation::CompensatingClose { binding } if *binding == child
    ));
    let delivery = session
        .engine
        .viewport()
        .effects()
        .record(effect)
        .unwrap()
        .delivery();
    assert!(delivery.is_some());
    assert_eq!(session.committed_source_sequence, source_sequence + 1);

    let mut next = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the next cleanup probe begins");
    assert!(!next.cancel_next_native_create().unwrap());
    next.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = next.commit().expect("the quiet cleanup probe commits");
    assert!(report.take_native_effects().is_empty());
    assert_eq!(session.committed_source_sequence, source_sequence + 1);
    assert_eq!(session.engine.workspace(), &source);
    assert_eq!(
        session
            .engine
            .viewport()
            .effects()
            .record(effect)
            .unwrap()
            .delivery(),
        delivery
    );
    assert_eq!(
        session
            .engine
            .viewport()
            .effects()
            .records()
            .filter(|(_, record)| matches!(
                record.request().effect(),
                PlatformEffect::CompensatingClose { binding, .. } if *binding == child.binding
            ))
            .count(),
        1
    );
    let Some(NativeEffectAcknowledgement::Close(_)) = compensation.accepted() else {
        panic!("compensation retains its exact close acknowledgement");
    };
}

#[test]
fn programmatic_native_root_tear_off_rejects_when_it_would_remove_the_recovery_host() {
    let (mut session, _) = managed_tear_off_session();
    let before = session.version();
    let placement = NativeWindowPlacement::new(
        PhysicalRect::new(920.0, 120.0, 420.0, 320.0)
            .expect("the requested outer placement validates"),
    );

    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the programmatic tear-off frame begins");
    frame
        .move_root_to_new_window_current(ROOT, placement)
        .expect("the revision-bound root tear-off stages structurally");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the source presentation remains retained");
    let mut report = frame
        .commit()
        .expect("the missing recovery-host rejection commits");

    assert_eq!(
        report.inputs(),
        &[
            super::super::super::HostInputOutcome::ProductActionRejected(
                crate::model::DockspaceActionRejection::NativeUnavailable,
            )
        ]
    );
    assert!(report.take_native_effects().is_empty());
    assert_eq!(session.version(), before);
    assert_eq!(
        session.view().item(ITEM).map(|item| item.surface()),
        Some(SURFACE)
    );
    assert_eq!(
        session.view().item(SECOND_ITEM).map(|item| item.surface()),
        Some(SURFACE)
    );
}

#[inline(never)]
fn assert_cleanup_skips_transferred_candidate(session: &mut DockspaceSession) {
    let saga = session
        .engine
        .viewport()
        .native_create_sagas()
        .next()
        .unwrap()
        .0;
    assert!(matches!(
        session
            .engine
            .viewport()
            .native_create_saga(saga)
            .unwrap()
            .phase(),
        crate::frame::NativeCreatePhase::AwaitingPostShowPresentation { .. }
    ));
    let source_sequence = session.committed_source_sequence;
    let mut transfer = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the post-show terminal transfers ownership in this candidate");
    assert!(matches!(
        transfer
            .frame
            .view()
            .viewport()
            .native_create_saga(saga)
            .unwrap()
            .phase(),
        crate::frame::NativeCreatePhase::AwaitingFirstLivePresentation { .. }
    ));
    assert!(!transfer.cancel_next_native_create().unwrap());
    assert_eq!(transfer.next_source_sequence, source_sequence);
    transfer
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = transfer
        .commit()
        .expect("cleanup skips the transferred candidate");
    assert!(report.native_admissions().is_empty());
}

#[inline(never)]
fn assert_dropped_redock_preserves_transition(
    session: &mut DockspaceSession,
    child_binding: NativeSurfaceBinding,
    child_root: RootId,
) {
    let mut dropped_redock = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the redock with a dropped target begins");
    dropped_redock
        .dock_root_current(
            child_root,
            DockPlacement::Center(DockAnchor::Item(SECOND_ITEM)),
        )
        .unwrap();
    assert!(!dropped_redock.cancel_next_native_create().unwrap());
    dropped_redock
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = dropped_redock
        .commit()
        .expect("the pending redock publishes");
    let [
        super::super::super::HostInputOutcome::ProductPresentationActionRequested {
            transition,
            ..
        },
    ] = report.inputs()
    else {
        panic!("the redock must retain its original transition identity");
    };
    let requested_transition = *transition;
    measure_all_surfaces(session);
    let mut paint = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the redock preview paint begins");
    let receipt = execute_surface_program(&mut paint, SURFACE);
    paint.confirm_surface_painted(receipt).unwrap();
    paint
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = paint.commit().expect("the target preview output publishes");
    let outputs = report.take_painted_outputs();
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0].surface(), SURFACE);
    drop(outputs);

    let mut cleanup = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the dropped target terminal enters the cleanup candidate");
    assert!(!cleanup.cancel_next_native_create().unwrap());
    cleanup
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = cleanup
        .commit()
        .expect("the original rehome terminal commits");
    assert!(matches!(
        report.presentation_transitions(),
        [transition]
            if transition.id() == requested_transition
                && transition.root() == child_root
                && transition.source_surface() == child_binding.surface()
                && transition.target_surface() == SURFACE
                && transition.result()
                    == crate::runtime::DockspacePresentationTransitionResult::TargetNotPresented
    ));
    assert!(report.take_native_effects().is_empty());
    assert_eq!(
        session.view().item(ITEM).unwrap().surface(),
        child_binding.surface()
    );
    let report = paint_and_present_all_native_surfaces(session);
    assert!(report.presentation_transitions().is_empty());
}

#[test]
fn managed_native_tear_off_reaches_first_live_through_the_public_runtime() {
    let PendingNativeCreate {
        mut session,
        root_binding,
        child_binding,
        placement,
        request: create,
        restore_acknowledgement,
    } = request_native_create();
    let retained_streams_before = session
        .engine
        .runtime_retention_manifest()
        .presentation_hosts()
        .retained_stream_states();
    let capture_generations_before = session.presentation.retained_capture_generation_count();
    let create_ack = match create.accepted() {
        Some(NativeEffectAcknowledgement::Presentation(acknowledgement)) => acknowledgement,
        acknowledgement => panic!("create returned the wrong acknowledgement: {acknowledgement:?}"),
    };
    assert_ne!(child_binding.surface(), SURFACE);
    assert_eq!(
        session.view().item(ITEM).map(|item| item.surface()),
        Some(SURFACE)
    );

    session
        .report_managed_native_snapshot(
            [
                (
                    root_binding,
                    root_window_facts().with_input(
                        NativeWindowInputState::ReceivesInput,
                        Some(restore_acknowledgement),
                    ),
                ),
                (
                    child_binding,
                    NativeWindowFacts::live()
                        .with_content_bounds(placement)
                        .with_outer_bounds(placement)
                        .with_native_scale_factor(native_scale())
                        .with_presentation_scale_factor(native_scale())
                        .with_input(NativeWindowInputState::ReceivesInput, None)
                        .with_presentation(NativeWindowPresentationState::Hidden, Some(create_ack))
                        .with_close(NativeCloseState::Clear, None),
                ),
            ],
            NativeWorkAreaRoster::Exact(vec![work_area(1)]),
        )
        .expect("the exact hidden child roster records");
    commit_managed_frame(&mut session);

    let pre_show = paint_staging(
        &mut session,
        child_binding,
        NativeStagingPresentationPhase::PreShow,
    );
    session
        .report_native_staging_presentation(pre_show, SurfacePresentationResult::Presented)
        .expect("the pre-show staging result records");
    let mut pre_show_report = commit_managed_frame_report(&mut session);
    let show = take_only_native_effect(&mut pre_show_report);
    assert!(matches!(
        show.operation(),
        NativeEffectOperation::ShowWindow { binding } if *binding == child_binding
    ));
    let show_ack = match show.accepted() {
        Some(NativeEffectAcknowledgement::Presentation(acknowledgement)) => acknowledgement,
        acknowledgement => panic!("show returned the wrong acknowledgement: {acknowledgement:?}"),
    };

    session
        .report_managed_native_snapshot(
            [
                (root_binding, root_window_facts()),
                (
                    child_binding,
                    NativeWindowFacts::live()
                        .with_content_bounds(placement)
                        .with_outer_bounds(placement)
                        .with_native_scale_factor(native_scale())
                        .with_presentation_scale_factor(native_scale())
                        .with_input(NativeWindowInputState::ReceivesInput, None)
                        .with_presentation(NativeWindowPresentationState::Hidden, Some(show_ack))
                        .with_close(NativeCloseState::Clear, None),
                ),
            ],
            NativeWorkAreaRoster::Exact(vec![work_area(1)]),
        )
        .expect("the exact show acknowledgement records while the child remains hidden");
    commit_managed_frame(&mut session);

    session
        .report_managed_native_snapshot(
            [
                (root_binding, root_window_facts()),
                (
                    child_binding,
                    NativeWindowFacts::live()
                        .with_content_bounds(placement)
                        .with_outer_bounds(placement)
                        .with_native_scale_factor(native_scale())
                        .with_presentation_scale_factor(native_scale())
                        .with_input(NativeWindowInputState::ReceivesInput, None)
                        .with_presentation(NativeWindowPresentationState::Visible, None)
                        .with_close(NativeCloseState::Clear, None),
                ),
            ],
            NativeWorkAreaRoster::Exact(vec![work_area(1)]),
        )
        .expect("the later exact visible child roster records");
    commit_managed_frame(&mut session);

    let post_show = paint_staging(
        &mut session,
        child_binding,
        NativeStagingPresentationPhase::PostShow,
    );
    session
        .report_native_staging_presentation(post_show, SurfacePresentationResult::Presented)
        .expect("the post-show staging result records");
    assert_cleanup_skips_transferred_candidate(&mut session);

    measure_all_surfaces(&mut session);
    let mut first_live = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the first-live paint frame begins");
    assert!(!first_live.cancel_next_native_create().unwrap());
    assert!(
        first_live
            .paint_plan(child_binding.surface())
            .expect("the child plan resolves")
            .is_some(),
        "ownership transfer must publish a semantic child plan"
    );
    let receipt = execute_surface_program(&mut first_live, child_binding.surface());
    first_live
        .confirm_surface_painted(receipt)
        .expect("the first-live child output is painted");
    first_live
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the root surface retains its exact plan");
    let mut first_live_report = first_live.commit().expect("the first-live paint commits");
    assert!(first_live_report.native_admissions().is_empty());
    present_surface_outputs(&mut session, &mut first_live_report);
    let admitted = commit_managed_frame_report(&mut session);

    assert_eq!(admitted.native_admissions(), &[child_binding]);
    assert_eq!(
        session.view().item(ITEM).map(|item| item.surface()),
        Some(child_binding.surface())
    );
    assert_eq!(
        session.view().item(SECOND_ITEM).map(|item| item.surface()),
        Some(SURFACE)
    );
    assert_eq!(
        session
            .engine
            .runtime_retention_manifest()
            .presentation_hosts()
            .retained_stream_states(),
        retained_streams_before + 1,
        "the admitted child owns one exact presentation stream",
    );
    assert_eq!(
        session.presentation.retained_capture_generation_count(),
        capture_generations_before + 1,
        "the runtime sidecar retains the child stream generation",
    );
    let child_root = session
        .view()
        .item(ITEM)
        .expect("the child still owns the torn-off item")
        .root();
    let mut availability = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the child presentation-command frame begins");
    let child_plan = availability
        .paint_plan(child_binding.surface())
        .expect("the child presentation-command plan resolves")
        .expect("the child presentation-command plan is ready");
    let float = child_plan
        .presentation_command(child_root, DockspacePresentationCommandKind::Float)
        .expect("the child float availability resolves")
        .expect("the native child owns the torn-off root");
    assert_eq!(
        float.unavailable_reason(),
        Some(DockspacePresentationCommandUnavailable::Action(
            crate::model::DockspaceActionRejection::PresentationUnavailable { surface: SURFACE }
        )),
        "the child must not prepare contained recovery before its host has exact presentation authority"
    );
    let move_to_new_window = child_plan
        .presentation_command(
            child_root,
            DockspacePresentationCommandKind::MoveToNewWindow,
        )
        .expect("the child promotion availability resolves")
        .expect("the native child owns the torn-off root");
    assert_eq!(
        move_to_new_window.unavailable_reason(),
        Some(DockspacePresentationCommandUnavailable::AlreadyNative)
    );
    availability
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the child presentation-command frame settles every surface");
    availability
        .commit()
        .expect("the child presentation-command frame commits");

    let _ = paint_and_present_all_native_surfaces(&mut session);
    let mut steady_availability = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the steady child presentation-command frame begins");
    let steady_child_plan = steady_availability
        .paint_plan(child_binding.surface())
        .expect("the steady child presentation-command plan resolves")
        .expect("the steady child presentation-command plan is ready");
    let steady_float = steady_child_plan
        .presentation_command(child_root, DockspacePresentationCommandKind::Float)
        .expect("the steady child float availability resolves")
        .expect("the native child still owns the torn-off root");
    assert!(
        steady_float.is_ready(),
        "a managed child may recover into its presented host as contained content: {:?}",
        steady_float.unavailable_reason()
    );
    steady_availability
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the steady child presentation-command frame settles every surface");
    steady_availability
        .commit()
        .expect("the steady child presentation-command frame commits");
    assert!(
        session.engine.interaction_projection(SURFACE).is_some(),
        "recovery scene after complete presentation: {:?}",
        session.engine.scene().surface(SURFACE),
    );

    assert_dropped_redock_preserves_transition(&mut session, child_binding, child_root);

    let mut redock = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the child redock frame begins");
    redock
        .dock_root_current(
            child_root,
            crate::model::DockPlacement::Center(crate::model::DockAnchor::Item(SECOND_ITEM)),
        )
        .expect("the complete child root redocks into the recovery surface");
    redock
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the redock frame settles every surface");
    let mut redock_report = redock.commit().expect("the child redock commits");
    assert!(
        matches!(
            redock_report.inputs(),
            [
                super::super::super::HostInputOutcome::ProductPresentationActionRequested {
                    outcome: crate::model::DockspaceActionOutcome::RootDockRequested { .. },
                    ..
                }
            ]
        ),
        "actual redock inputs: {:?}",
        redock_report.inputs()
    );
    assert!(redock_report.take_native_effects().is_empty());
    assert_eq!(
        session
            .view()
            .item(ITEM)
            .expect("the item remains available before presentation")
            .surface(),
        child_binding.surface(),
    );

    let mut settled = paint_and_present_all_native_surfaces(&mut session);
    assert!(
        matches!(
            settled.presentation_transitions(),
            [transition]
                if transition.root() == child_root
                    && transition.source_surface() == child_binding.surface()
                    && transition.target_surface() == SURFACE
                    && transition.result()
                        == crate::runtime::DockspacePresentationTransitionResult::Applied
        ),
        "actual presentation transitions: {:?}",
        settled.presentation_transitions()
    );
    let release = take_only_native_effect(&mut settled);
    assert!(matches!(
        release.operation(),
        NativeEffectOperation::ReleaseChild { binding } if *binding == child_binding
    ));
    let close_ack = match release.accepted() {
        Some(NativeEffectAcknowledgement::Close(acknowledgement)) => acknowledgement,
        acknowledgement => {
            panic!("release returned the wrong acknowledgement: {acknowledgement:?}")
        }
    };
    session
        .report_managed_native_snapshot(
            [
                (root_binding, root_window_facts()),
                (child_binding, NativeWindowFacts::destroyed_after(close_ack)),
            ],
            NativeWorkAreaRoster::Exact(vec![work_area(1)]),
        )
        .expect("the exact destroyed child roster records");
    commit_managed_frame(&mut session);
    session
        .report_native_binding_quiescence(child_binding)
        .expect("the retired child binding becomes externally quiescent");
    commit_managed_frame(&mut session);
    commit_managed_frame(&mut session);

    assert_eq!(
        session
            .engine
            .runtime_retention_manifest()
            .bindings()
            .destroyed_binding_guards(),
        0,
        "the committed binding quiescence reclaims its destroyed guard",
    );
    assert_eq!(
        session
            .engine
            .runtime_retention_manifest()
            .presentation_hosts()
            .retained_stream_states(),
        retained_streams_before,
        "the next renderer-quiescent boundary reclaims the retired child presentation stream",
    );
    assert_eq!(
        session.presentation.retained_capture_generation_count(),
        capture_generations_before,
        "the sidecar generation disappears after exact core stream compaction",
    );
}

#[test]
fn managed_native_create_dispatch_failure_preserves_source_ownership() {
    let PendingNativeCreate {
        mut session,
        root_binding,
        child_binding,
        request,
        restore_acknowledgement,
        ..
    } = request_native_create();
    let failed = request.dispatch_failed(NativeDispatchFailure::WindowUnavailable);
    session
        .report_native_effect_result(failed)
        .expect("the exact create failure records");
    let mut failure_report = commit_managed_frame_report(&mut session);
    assert!(failure_report.take_native_effects().is_empty());

    session
        .report_managed_native_snapshot(
            [(
                root_binding,
                root_window_facts().with_input(
                    NativeWindowInputState::ReceivesInput,
                    Some(restore_acknowledgement),
                ),
            )],
            NativeWorkAreaRoster::Exact(vec![work_area(1)]),
        )
        .expect("the exact root input restoration records after create failure");
    let mut report = commit_managed_frame_report(&mut session);

    assert!(report.take_native_effects().is_empty());
    assert_eq!(
        session.view().item(ITEM).map(|item| item.surface()),
        Some(SURFACE)
    );
    assert_eq!(
        session.view().item(SECOND_ITEM).map(|item| item.surface()),
        Some(SURFACE)
    );
    assert!(!session.is_current_native_binding(child_binding));
}

struct PendingNativeCreate {
    session: DockspaceSession,
    root_binding: NativeSurfaceBinding,
    child_binding: NativeSurfaceBinding,
    placement: PhysicalRect,
    request: crate::runtime::NativeEffectRequest,
    restore_acknowledgement: crate::runtime::NativeInputEffectAcknowledgement,
}

#[test]
fn tracked_application_action_correlation_ignores_an_earlier_pointer_close_request() {
    const MAIN_ITEM: ItemId = ItemId::new(3);
    const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);

    let (mut session, root_binding) = programmatic_tear_off_session();
    let source_receiver = {
        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("the contained receiver inspection frame begins");
        let plan = frame
            .paint_plan(SURFACE)
            .expect("the contained source plan resolves")
            .expect("the contained source plan is ready");
        let contained = plan
            .contained()
            .find(|contained| contained.floating() == FLOATING)
            .expect("the contained floating is present");
        plan.receiver_for_contained_close(contained)
            .expect("the contained floating exposes its dock-back receiver")
    };
    let source_point = source_receiver.center();
    let source_desktop = PhysicalPoint::new(
        ROOT_ORIGIN_X + source_point.x(),
        ROOT_ORIGIN_Y + source_point.y(),
    )
    .expect("the contained close desktop point is finite");
    let pointer = NativePointerId::new(1);
    let application =
        session.prepare_dock_root(ROOT, DockPlacement::Center(DockAnchor::Item(MAIN_ITEM)));

    session
        .record_native_pointer(NativePointerInput::new(
            pointer,
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            NativeDesktopPointerLocation::new(
                NativeDesktopPosition::Exact(source_desktop),
                NativePointerHover::Dock(root_binding),
                None,
            ),
            NativePointerOwner::Native(root_binding),
            NativePointerOwner::Native(root_binding),
        ))
        .expect("the exact contained close press records");
    session
        .record_native_pointer(NativePointerInput::new(
            pointer,
            NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
            NativeDesktopPointerLocation::new(
                NativeDesktopPosition::Exact(source_desktop),
                NativePointerHover::Dock(root_binding),
                None,
            ),
            NativePointerOwner::Native(root_binding),
            NativePointerOwner::None,
        ))
        .expect("the exact contained close release records");
    let mut release = session
        .begin_native_host_frame(|query| receiver_answer(query, source_receiver))
        .expect("the joined pointer and application frame begins");
    let application_request = release
        .submit_prepared_action(application)
        .expect("the application action receives an opaque request identity");
    release
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the joined frame retains current surface authority");
    let report = release.commit().expect("the joined frame commits");

    assert!(
        matches!(
            report.inputs(),
            [
                super::super::super::HostInputOutcome::CloseRequested {
                    plan,
                    reused: false,
                    origin: super::super::super::HostCloseRequestOrigin::Interaction,
                },
                super::super::super::HostInputOutcome::ProductActionApplied(
                    crate::model::DockspaceActionOutcome::RootDocked {
                        root: ROOT,
                        target_root,
                        changed: true,
                        ..
                    }
                ),
            ] if plan.target() == (crate::close_plan::ClosePlanTarget::Root { root: ROOT })
                && *target_root == RootId::new(2)
        ),
        "actual joined outcomes: {:?}",
        report.inputs()
    );
    let application_outcome = report
        .submitted_action_outcome(application_request)
        .expect("the opaque request resolves its exact terminal");
    assert!(matches!(
        application_outcome,
        super::super::super::HostInputOutcome::ProductActionApplied(
            crate::model::DockspaceActionOutcome::RootDocked {
                root: ROOT,
                target_root,
                changed: true,
                ..
            }
        ) if *target_root == RootId::new(2)
    ));
    assert!(std::ptr::eq(application_outcome, &report.inputs()[1]));
}

pub(super) fn prepare_native_tear_off_release(
    limit: Option<NativeInputStorageLimit>,
) -> (
    DockspaceSession,
    NativeSurfaceBinding,
    DockspaceReceiverDescriptor,
) {
    let (mut session, root_binding) = managed_tear_off_session_with_storage_limit(limit);
    let work_area_binding = session
        .native_work_area(HostWorkAreaToken::new(1))
        .expect("the committed work-area roster mints route authority");
    let source_receiver = presented_tab_receiver(&mut session, ITEM);
    let source_point = source_receiver.center();
    let source_desktop = PhysicalPoint::new(
        ROOT_ORIGIN_X + source_point.x(),
        ROOT_ORIGIN_Y + source_point.y(),
    )
    .expect("the source desktop point is finite");
    let outside = PhysicalPoint::new(1_000.0, 700.0).expect("the outside point is finite");
    let pointer = NativePointerId::new(1);

    session
        .record_native_pointer(NativePointerInput::new(
            pointer,
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            NativeDesktopPointerLocation::new(
                NativeDesktopPosition::Exact(source_desktop),
                NativePointerHover::Dock(root_binding),
                None,
            ),
            NativePointerOwner::Native(root_binding),
            NativePointerOwner::Native(root_binding),
        ))
        .expect("the exact source press records");
    session
        .record_native_pointer(NativePointerInput::new(
            pointer,
            NativePointerEvent::Moved,
            NativeDesktopPointerLocation::new(
                NativeDesktopPosition::Exact(outside),
                NativePointerHover::OutsideAll,
                Some(work_area_binding),
            ),
            NativePointerOwner::Native(root_binding),
            NativePointerOwner::Native(root_binding),
        ))
        .expect("the exact outside-all move records");

    let mut route = session
        .begin_native_host_frame(|query| receiver_answer(query, source_receiver))
        .expect("the native drag frame begins");
    assert!(
        !program_has_drag_preview(
            route
                .paint_plan(SURFACE)
                .expect("the source plan resolves")
                .expect("the source remains paintable")
        ),
        "an unconfirmed pointer route must not expose a native preview"
    );
    let receipt = execute_surface_program(&mut route, SURFACE);
    route
        .confirm_surface_painted(receipt)
        .expect("the route-request frame is painted");
    let mut route_report = route.commit().expect("the route-request frame commits");
    let enable = take_only_native_effect(&mut route_report);
    assert!(matches!(
        enable.operation(),
        NativeEffectOperation::SetPointerPassthrough {
            binding,
            enabled: true,
        } if *binding == root_binding
    ));
    let enable_acknowledgement = match enable.accepted() {
        Some(NativeEffectAcknowledgement::Input(acknowledgement)) => acknowledgement,
        acknowledgement => panic!("pointer enable returned {acknowledgement:?}"),
    };
    present_surface_outputs(&mut session, &mut route_report);
    session
        .report_managed_native_snapshot(
            [(
                root_binding,
                root_window_facts().with_input(
                    NativeWindowInputState::PassThrough,
                    Some(enable_acknowledgement),
                ),
            )],
            NativeWorkAreaRoster::Exact(vec![work_area(1)]),
        )
        .expect("the exact enabled input observation records");
    commit_managed_frame(&mut session);

    session
        .record_native_pointer(NativePointerInput::new(
            pointer,
            NativePointerEvent::Moved,
            NativeDesktopPointerLocation::new(
                NativeDesktopPosition::Exact(outside),
                NativePointerHover::OutsideAll,
                Some(work_area_binding),
            ),
            NativePointerOwner::Native(root_binding),
            NativePointerOwner::Native(root_binding),
        ))
        .expect("the confirmed outside-all move records");
    let mut preview = session
        .begin_native_host_frame(|query| receiver_answer(query, source_receiver))
        .expect("the confirmed native preview frame begins");
    assert!(
        program_has_drag_preview(
            preview
                .paint_plan(SURFACE)
                .expect("the confirmed source plan resolves")
                .expect("the confirmed source remains paintable")
        ),
        "the confirmed outside-all move must expose a core-owned native preview"
    );
    let receipt = execute_surface_program(&mut preview, SURFACE);
    preview
        .confirm_surface_painted(receipt)
        .expect("the exact native preview is painted");
    let mut preview_report = preview.commit().expect("the native preview frame commits");
    assert!(preview_report.take_native_effects().is_empty());
    present_surface_outputs(&mut session, &mut preview_report);

    session
        .record_native_pointer(NativePointerInput::new(
            pointer,
            NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
            NativeDesktopPointerLocation::new(
                NativeDesktopPosition::Exact(outside),
                NativePointerHover::OutsideAll,
                Some(work_area_binding),
            ),
            NativePointerOwner::Native(root_binding),
            NativePointerOwner::None,
        ))
        .expect("the exact outside-all release records");
    (session, root_binding, source_receiver)
}

fn request_native_create() -> PendingNativeCreate {
    let (mut session, root_binding, source_receiver) = prepare_native_tear_off_release(None);
    let mut release = session
        .begin_native_host_frame(|query| receiver_answer(query, source_receiver))
        .expect("the native release frame begins");
    release
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the release frame retains current surface authority");
    let mut release_report = release.commit().expect("the native release commits");
    let mut create = None;
    let mut restore_acknowledgement = None;
    let effects = release_report.take_native_effects();
    for request in effects {
        match request.operation() {
            NativeEffectOperation::CreateWindow { .. } => create = Some(request),
            NativeEffectOperation::SetPointerPassthrough {
                binding,
                enabled: false,
            } if *binding == root_binding => {
                restore_acknowledgement = match request.accepted() {
                    Some(NativeEffectAcknowledgement::Input(acknowledgement)) => {
                        Some(acknowledgement)
                    }
                    acknowledgement => {
                        panic!("pointer restore returned {acknowledgement:?}")
                    }
                };
            }
            operation => panic!("outside-all release emitted unexpected {operation:?}"),
        }
    }
    let request = create.expect("outside-all release emits one CreateWindow");
    let restore_acknowledgement =
        restore_acknowledgement.expect("outside-all release restores pointer input");
    let (child_binding, placement) = match request.operation() {
        NativeEffectOperation::CreateWindow {
            binding, placement, ..
        } => (*binding, *placement),
        operation => unreachable!("stored create request changed to {operation:?}"),
    };
    assert_ne!(child_binding.surface(), SURFACE);
    assert_eq!(
        session.view().item(ITEM).map(|item| item.surface()),
        Some(SURFACE)
    );

    PendingNativeCreate {
        session,
        root_binding,
        child_binding,
        placement,
        request,
        restore_acknowledgement,
    }
}

fn managed_tear_off_session() -> (DockspaceSession, NativeSurfaceBinding) {
    managed_tear_off_session_with_storage_limit(None)
}

fn managed_tear_off_session_with_storage_limit(
    limit: Option<NativeInputStorageLimit>,
) -> (DockspaceSession, NativeSurfaceBinding) {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM, SECOND_ITEM]));
    builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    managed_session_from_workspace_with_storage_limit(
        builder.build().expect("the tear-off workspace validates"),
        limit,
    )
}

#[test]
fn registered_root_window_menu_allows_contained_float_and_child_promotion() {
    const MAIN_ROOT: RootId = RootId::new(2);

    let (mut session, _) = programmatic_tear_off_session();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("native presentation-command inspection frame begins");
    let plan = frame
        .paint_plan(SURFACE)
        .expect("native main paint plan resolves")
        .expect("native main paint plan is ready");

    for kind in [
        DockspacePresentationCommandKind::Float,
        DockspacePresentationCommandKind::MoveToNewWindow,
    ] {
        let command = plan
            .presentation_command(MAIN_ROOT, kind)
            .expect("native presentation-command availability resolves")
            .expect("the native main root belongs to this surface");
        assert!(
            command.is_ready(),
            "registered root-window command {kind:?} unexpectedly unavailable: {:?}",
            command.unavailable_reason()
        );
    }

    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("native presentation-command inspection settles the surface");
    frame
        .commit()
        .expect("native presentation-command inspection commits");
}

pub(super) fn programmatic_tear_off_session() -> (DockspaceSession, NativeSurfaceBinding) {
    const MAIN_ROOT: RootId = RootId::new(2);
    const MAIN_ITEM: ItemId = ItemId::new(3);
    const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);

    let mut builder = Workspace::builder();
    let contained_tabs = builder.insert_node(Node::tabs([ITEM, SECOND_ITEM]));
    let main_tabs = builder.insert_node(Node::tabs([MAIN_ITEM]));
    builder.set_root(
        ROOT,
        RootRecord::new(contained_tabs).with_central(contained_tabs),
    );
    builder.set_root(
        MAIN_ROOT,
        RootRecord::new(main_tabs).with_central(main_tabs),
    );
    builder.set_surface(
        SURFACE,
        SurfacePresentation {
            main_root: Some(MAIN_ROOT),
            contained: vec![FLOATING],
        },
    );
    builder.set_contained_floating(
        FLOATING,
        ContainedFloating::new(
            ROOT,
            LogicalRect::new(40.0, 50.0, 360.0, 280.0).expect("the contained root bounds validate"),
        ),
    );
    managed_session_from_workspace(
        builder
            .build()
            .expect("the programmatic tear-off workspace validates"),
    )
}

fn managed_session_from_workspace(
    workspace: Workspace,
) -> (DockspaceSession, NativeSurfaceBinding) {
    managed_session_from_workspace_with_storage_limit(workspace, None)
}

fn managed_session_from_workspace_with_storage_limit(
    workspace: Workspace,
    limit: Option<NativeInputStorageLimit>,
) -> (DockspaceSession, NativeSurfaceBinding) {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut session = DockspaceSession::from_workspace_for_test(workspace, policy)
        .expect("the tear-off session initializes");
    if let Some(limit) = limit {
        session
            .enable_managed_native_host_with_input_storage_limit(
                NativePointerRoster::Exact(Vec::new()),
                limit,
            )
            .expect("the finite managed native provider enrolls");
    } else {
        session
            .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
            .expect("the managed native provider enrolls");
    }
    session
        .configure_managed_native_capabilities(full_managed_capabilities())
        .expect("the managed backend capabilities configure");
    session
        .register_native_root(SURFACE, WINDOW)
        .expect("the root registration records");
    let mut registration = session.begin_host_frame().expect("the root frame begins");
    registration
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the root registration frame settles every surface");
    let report = registration
        .commit()
        .expect("the root registration commits");
    let binding = match report.inputs() {
        [super::super::super::HostInputOutcome::NativeSurfaceRegistered { binding }] => *binding,
        outcomes => panic!("expected one root registration, got {outcomes:?}"),
    };
    session
        .report_managed_native_snapshot(
            [(binding, root_window_facts())],
            NativeWorkAreaRoster::Exact(vec![work_area(1)]),
        )
        .expect("the exact root snapshot records");
    commit_managed_frame(&mut session);
    measure_all_surfaces(&mut session);
    let (output, _) = paint_native_output(&mut session);
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .expect("the root output presentation records");
    commit_managed_frame(&mut session);
    (session, binding)
}

fn root_window_facts() -> NativeWindowFacts {
    let content = PhysicalRect::new(ROOT_ORIGIN_X, ROOT_ORIGIN_Y, 640.0, 480.0)
        .expect("the root content bounds validate");
    let outer = PhysicalRect::new(ROOT_ORIGIN_X - 8.0, ROOT_ORIGIN_Y - 30.0, 656.0, 518.0)
        .expect("the root outer bounds validate");
    NativeWindowFacts::live()
        .with_content_bounds(content)
        .with_outer_bounds(outer)
        .with_native_scale_factor(native_scale())
        .with_presentation_scale_factor(native_scale())
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible, None)
        .with_close(NativeCloseState::Clear, None)
}

fn native_scale() -> ScaleFactor {
    ScaleFactor::new(1.0).expect("the native scale validates")
}

fn presented_tab_receiver(
    session: &mut DockspaceSession,
    item: ItemId,
) -> DockspaceReceiverDescriptor {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the receiver inspection frame begins");
    let plan = frame
        .paint_plan(SURFACE)
        .expect("the source plan resolves")
        .expect("the source plan is ready");
    let tab = plan
        .tabs()
        .find(|tab| tab.item() == item)
        .expect("the source tab is present");
    plan.receiver_for_tab_body(tab)
        .expect("the source tab exposes its exact drag receiver")
}

fn receiver_answer(
    query: NativeReceiverQuery,
    descriptor: DockspaceReceiverDescriptor,
) -> NativeReceiverAnswer {
    query
        .bind_receiver(&descriptor)
        .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock)
}

fn measure_all_surfaces(session: &mut DockspaceSession) {
    let metrics = UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("test bounds validate"),
        LogicalSize::new(32.0, 24.0).expect("test minimum validates"),
        80.0,
    )
    .expect("test metrics validate");
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the measurement frame begins");
    for surface in frame.surfaces() {
        frame
            .measure_surface(surface, metrics)
            .expect("the complete surface measurement stages");
    }
    frame.commit().expect("the complete measurements commit");
}

fn present_surface_outputs(session: &mut DockspaceSession, report: &mut HostFrameReport) {
    let outputs = report.take_painted_outputs();
    assert!(!outputs.is_empty(), "the frame must emit a painted output");
    for output in outputs {
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .expect("the exact painted output presentation records");
    }
}

fn paint_staging(
    session: &mut DockspaceSession,
    binding: NativeSurfaceBinding,
    phase: NativeStagingPresentationPhase,
) -> crate::runtime::PaintedNativeStagingOutput {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the staging frame begins");
    let request = frame
        .native_staging_paints()
        .into_iter()
        .find(|request| request.binding() == binding && request.phase() == phase)
        .unwrap_or_else(|| panic!("the {phase:?} staging request must exist"));
    frame
        .confirm_native_staging_painted(request)
        .expect("the exact staging placeholder is painted");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the staging frame retains semantic surfaces");
    let mut report = frame.commit().expect("the staging frame commits");
    let outputs = report.take_painted_native_staging_outputs();
    let [output] = outputs.try_into().unwrap_or_else(|outputs: Vec<_>| {
        panic!("expected one {phase:?} staging output, got {outputs:?}")
    });
    output
}

fn commit_managed_frame_report(session: &mut DockspaceSession) -> HostFrameReport {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the managed frame begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the managed frame settles every surface");
    frame.commit().expect("the managed frame commits")
}

fn take_only_native_effect(report: &mut HostFrameReport) -> crate::runtime::NativeEffectRequest {
    let effects = report.take_native_effects();
    let [effect] = effects
        .try_into()
        .unwrap_or_else(|effects: Vec<_>| panic!("expected one native effect, got {effects:?}"));
    effect
}

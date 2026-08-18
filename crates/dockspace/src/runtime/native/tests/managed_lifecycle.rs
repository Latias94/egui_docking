use super::*;

use crate::geometry::PhysicalPoint;
use crate::graph::ContainedFloating;
use crate::model::FloatingPresentationId;
use crate::runtime::{
    DockspaceReceiverDescriptor, HostFrameReport, NativeEffectAcknowledgement,
    NativeEffectOperation, NativeStagingPresentationPhase, NativeWindowPlacement,
};
#[cfg(feature = "serde")]
use crate::{
    engine::EngineInput,
    model::{DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout},
    runtime::{DockspaceDocumentBootstrap, DockspaceDocumentId},
};

const SECOND_ITEM: ItemId = ItemId::new(2);
const ROOT_ORIGIN_X: f64 = 100.0;
const ROOT_ORIGIN_Y: f64 = 80.0;

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
fn programmatic_native_root_tear_off_starts_the_shared_create_saga() {
    let (mut session, _) = programmatic_tear_off_session();
    let before = session.version();
    let placement = PhysicalRect::new(920.0, 120.0, 420.0, 320.0)
        .expect("the requested outer placement validates");

    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the programmatic tear-off frame begins");
    frame
        .tear_off_root_current(ROOT, NativeWindowPlacement::new(placement))
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
        .tear_off_root_current(ROOT, placement)
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
        .tear_off_root_current(ROOT, placement)
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
        .tear_off_root_current(ROOT, placement)
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
    commit_managed_frame(&mut session);

    measure_all_surfaces(&mut session);
    let mut first_live = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the first-live paint frame begins");
    assert!(
        first_live
            .paint_plan(child_binding.surface())
            .expect("the child plan resolves")
            .is_some(),
        "ownership transfer must publish a semantic child plan"
    );
    first_live
        .confirm_surface_painted(child_binding.surface())
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
    let _ = paint_and_present_all_native_surfaces(&mut session);
    assert!(
        session.engine.interaction_projection(SURFACE).is_some(),
        "recovery scene after complete presentation: {:?}",
        session.engine.scene().surface(SURFACE),
    );

    let child_root = session
        .view()
        .item(ITEM)
        .expect("the child still owns the torn-off item")
        .root();
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

fn request_native_create() -> PendingNativeCreate {
    let (mut session, root_binding) = managed_tear_off_session();
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

    let mut preview = session
        .begin_native_host_frame(|query| receiver_answer(query, source_receiver))
        .expect("the native drag frame begins");
    assert!(
        preview
            .paint_plan(SURFACE)
            .expect("the source plan resolves")
            .expect("the source remains paintable")
            .drag_preview()
            .is_some(),
        "the outside-all move must expose a core-owned native preview"
    );
    preview
        .confirm_surface_painted(SURFACE)
        .expect("the exact preview is painted");
    let mut preview_report = preview.commit().expect("the preview frame commits");
    let enable = take_only_native_effect(&mut preview_report);
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
    present_surface_outputs(&mut session, &mut preview_report);
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
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM, SECOND_ITEM]));
    builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    managed_session_from_workspace(builder.build().expect("the tear-off workspace validates"))
}

fn programmatic_tear_off_session() -> (DockspaceSession, NativeSurfaceBinding) {
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
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut session = DockspaceSession::from_workspace_for_test(workspace, policy)
        .expect("the tear-off session initializes");
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .expect("the managed native provider enrolls");
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

//! Incremental logical surfaces preserve unrelated public presentation capabilities.

use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalRect, ScaleFactor};
use dockspace::model::{
    DockAnchor, DockPlacement, DockspaceActionOutcome, DockspaceContainedLayout, DockspaceLayout,
    DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, FloatingPresentationId, ItemId,
    LogicalSurfaceRejection, NativeWindowPlacement, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceReceiverDescriptor, DockspaceSession, DockspaceSubmittedAction,
    HostFrameReport, HostInputOutcome, HostPointerInput, HostPointerRoute, HostWindowToken,
    NativeHostCapabilities, NativeHostCapability, NativePointerRoster, NativeReceiverAnswer,
    NativeWindowFacts, NativeWindowInputState, NativeWindowPresentationState, NativeWorkAreaRoster,
    PaintedSurfaceOutput, PresentedDockReceiver, PresentedDockspaceSurface,
    SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step,
    SurfacePointerButton, SurfacePointerCapture, SurfacePointerEvent, SurfacePointerId,
    SurfacePointerReceiverFacts, SurfacePresentationResult, SurfaceUnavailableReason,
    UniformSurfaceMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const S1: SurfaceId = SurfaceId::new(1);
const S2: SurfaceId = SurfaceId::new(2);
const S3: SurfaceId = SurfaceId::new(3);
const R1: RootId = RootId::new(1);
const R2: RootId = RootId::new(2);
const R3: RootId = RootId::new(3);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);
const FOURTH: ItemId = ItemId::new(4);

fn primary_surface() -> DockspaceSurfaceLayout {
    DockspaceSurfaceLayout::new(
        S1,
        DockspaceRootLayout::new(R1, DockspaceNode::central_tabs([FIRST, SECOND])),
    )
}

fn session() -> DockspaceSession {
    let layout = DockspaceLayout::new([primary_surface()]).unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session.enable_host_pointer().unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    measure(&mut frame, S1);
    finish(frame);
    session
}

fn measure(frame: &mut DockspaceHostFrame<'_>, surface: SurfaceId) {
    frame
        .measure_surface(
            surface,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
                LogicalSize::new(48.0, 32.0).unwrap(),
                96.0,
            )
            .unwrap(),
        )
        .unwrap();
}

fn finish(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn settle(session: &mut DockspaceSession) -> HostFrameReport {
    finish(session.begin_host_frame().unwrap())
}

fn paint(
    session: &mut DockspaceSession,
    surface: SurfaceId,
    tab: Option<ItemId>,
) -> (
    RecordingRenderer,
    DockspaceReceiverDescriptor,
    PaintedSurfaceOutput,
) {
    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(surface).unwrap().unwrap();
    let descriptor = match tab {
        Some(item) => plan.tab_receiver(item).unwrap(),
        None => {
            let mut receiver = None;
            plan.visit_surface_program(|step| {
                if let Step::Operation {
                    operation: Operation::Pane(pane),
                    ..
                } = step
                {
                    assert!(pane.selected().is_none());
                    receiver = plan.receiver_for_pane(pane);
                }
                Ok::<(), ()>(())
            })
            .unwrap();
            receiver.expect("the empty central leaf must expose its pane receiver")
        }
    };
    let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    let mut report = finish(frame);
    let [output] = report.take_painted_outputs().try_into().unwrap();
    assert_eq!(output.surface(), surface);
    assert!(matches!(
        recording.records.last(),
        Some(OwnedRecord::SurfaceEnd(semantic)) if output.matches_semantic_output(*semantic)
    ));
    (recording, descriptor, output)
}

fn present(
    session: &mut DockspaceSession,
    output: PaintedSurfaceOutput,
) -> PresentedDockspaceSurface {
    let surface = output.surface();
    let artifact = output.presentation_artifact();
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    let report = settle(session);
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.presents(artifact))
    );
    let proof = session.presented_surface(surface).unwrap();
    assert!(proof.matches_presentation_artifact(artifact));
    proof
}

fn paint_and_present(
    session: &mut DockspaceSession,
    surface: SurfaceId,
    tab: Option<ItemId>,
) -> (PresentedDockspaceSurface, PresentedDockReceiver) {
    let (_, descriptor, output) = paint(session, surface, tab);
    let proof = present(session, output);
    (proof, session.bind_presented_receiver(&descriptor).unwrap())
}

fn assert_preserved(
    session: &DockspaceSession,
    proof: PresentedDockspaceSurface,
    receiver: PresentedDockReceiver,
) {
    assert_eq!(session.presented_surface(proof.surface()), Some(proof));
    assert_eq!(
        session.bind_presented_receiver(&receiver.descriptor()),
        Some(receiver)
    );
}

fn click(
    session: &mut DockspaceSession,
    proof: PresentedDockspaceSurface,
    receiver: PresentedDockReceiver,
) -> HostFrameReport {
    let route = HostPointerRoute::new(proof, receiver.center());
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer_batch(
            [
                SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
            ]
            .map(|event| {
                HostPointerInput::new(
                    SurfacePointerId::new(1),
                    event,
                    route,
                    route,
                    SurfacePointerCapture::ProviderEndpoint,
                    SurfacePointerReceiverFacts::delivery(&receiver),
                )
            }),
        )
        .unwrap();
    finish(frame)
}

fn assert_rejected(
    report: &HostFrameReport,
    submitted: DockspaceSubmittedAction,
    surface: SurfaceId,
    reason: dockspace::runtime::LogicalSurfaceRejection,
) {
    assert_eq!(
        report.submitted_action_outcome(submitted),
        Some(&HostInputOutcome::LogicalSurfaceRejected { surface, reason })
    );
}

#[test]
fn logical_surface_journey_preserves_unaffected_presented_authority() {
    let mut session = session();
    let (before_attach, second_receiver) = paint_and_present(&mut session, S1, Some(SECOND));
    let version = session.version();
    let mut frame = session.begin_host_frame().unwrap();
    let submitted = frame.attach_empty_surface_current(S2, R2).unwrap();
    assert_eq!(frame.surfaces(), [S1, S2]);
    let central = frame.view().root(R2).unwrap().central().unwrap();
    assert!(central.is_central());
    assert!(central.tabs().unwrap().items().is_empty());
    measure(&mut frame, S2);
    let mut attached = finish(frame);
    assert_eq!(attached.before(), version);
    assert_eq!(attached.after(), session.version());
    assert_ne!(attached.after(), version);
    assert_eq!(
        attached.submitted_action_outcome(submitted),
        Some(&HostInputOutcome::LogicalSurfaceAttached {
            surface: S2,
            root: R2,
            version: attached.after(),
        })
    );
    assert!(attached.native_bindings().is_empty());
    assert!(attached.take_native_effects().is_empty());
    assert_preserved(&session, before_attach, second_receiver);

    let (empty, _, output) = paint(&mut session, S2, None);
    assert!(
        empty
            .records
            .iter()
            .any(|record| matches!(record, OwnedRecord::PaneContent { item: None, .. }))
    );
    present(&mut session, output);
    assert_preserved(&session, before_attach, second_receiver);
    assert!(click(&mut session, before_attach, second_receiver).workspace_changed());
    assert!(session.view().item(SECOND).unwrap().is_selected());
    assert!(!session.has_active_gesture());

    let mut frame = session.begin_host_frame().unwrap();
    frame
        .dock_item_current(SECOND, DockPlacement::Center(DockAnchor::Central(R2)))
        .unwrap();
    measure(&mut frame, S1);
    measure(&mut frame, S2);
    let docked = finish(frame);
    assert!(matches!(
        docked.inputs(),
        [HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Docked {
                item: SECOND,
                source_root: R1,
                target_root: R2,
                changed: true,
            }
        )]
    ));
    assert_eq!(session.view().item(SECOND).unwrap().surface(), S2);
    assert_eq!(session.view().item(FIRST).unwrap().surface(), S1);

    let mut frame = session.begin_host_frame().unwrap();
    frame
        .dock_item_current(SECOND, DockPlacement::Center(DockAnchor::Central(R1)))
        .unwrap();
    measure(&mut frame, S1);
    measure(&mut frame, S2);
    assert!(finish(frame).workspace_changed());
    assert!(
        session
            .view()
            .root(R2)
            .unwrap()
            .central()
            .unwrap()
            .tabs()
            .unwrap()
            .items()
            .is_empty()
    );
    assert!(session.view().surface(S2).is_some());
    let (before_retire, first_receiver) = paint_and_present(&mut session, S1, Some(FIRST));

    let version = session.version();
    let mut frame = session.begin_host_frame().unwrap();
    let submitted = frame.retire_empty_surface_current(S2).unwrap();
    assert_eq!(frame.surfaces(), [S1]);
    let mut retired = finish(frame);
    assert_eq!(retired.before(), version);
    assert_eq!(retired.after(), session.version());
    assert_ne!(retired.after(), version);
    assert_eq!(
        retired.submitted_action_outcome(submitted),
        Some(&HostInputOutcome::LogicalSurfaceRetired {
            surface: S2,
            version: retired.after()
        })
    );
    assert!(session.view().surface(S2).is_none());
    assert!(session.view().root(R2).is_none());
    assert!(retired.native_bindings().is_empty());
    assert!(retired.take_native_effects().is_empty());
    assert_preserved(&session, before_retire, first_receiver);
    assert!(click(&mut session, before_retire, first_receiver).workspace_changed());
    assert!(session.view().item(FIRST).unwrap().is_selected());
    assert_eq!(
        session.view().item(FIRST).unwrap().tabs().items(),
        &[FIRST, SECOND]
    );
    assert!(!session.has_active_gesture());
}

#[test]
fn logical_surface_refusals_and_frame_rollback_are_atomic() {
    let mut session = session();
    let (proof, receiver) = paint_and_present(&mut session, S1, Some(SECOND));
    let version = session.version();
    let topology = format!("{:?}", session.view());
    let mut frame = session.begin_host_frame().unwrap();
    let duplicate = frame.attach_empty_surface_current(S1, R2).unwrap();
    let root_owned = frame.attach_empty_surface_current(S2, R1).unwrap();
    let missing = frame.retire_empty_surface_current(S3).unwrap();
    let nonempty = frame.retire_empty_surface_current(S1).unwrap();
    let report = finish(frame);
    for (submitted, surface, reason) in [
        (
            duplicate,
            S1,
            LogicalSurfaceRejection::SurfaceAlreadyAttached,
        ),
        (
            root_owned,
            S2,
            LogicalSurfaceRejection::RootAlreadyOwned { root: R1 },
        ),
        (missing, S3, LogicalSurfaceRejection::SurfaceUnavailable),
        (
            nonempty,
            S1,
            LogicalSurfaceRejection::SurfaceNotEmpty { items: 2 },
        ),
    ] {
        assert_rejected(&report, submitted, surface, reason);
    }
    assert_eq!((report.before(), report.after()), (version, version));
    assert!(!report.workspace_changed());
    assert_eq!(format!("{:?}", session.view()), topology);
    assert_preserved(&session, proof, receiver);

    let mut abandoned = session.begin_host_frame().unwrap();
    let abandoned_attach = abandoned.attach_empty_surface_current(S2, R2).unwrap();
    abandoned
        .dock_item_current(SECOND, DockPlacement::Center(DockAnchor::Central(R2)))
        .unwrap();
    assert_eq!(abandoned.view().item(SECOND).unwrap().surface(), S2);
    drop(abandoned);
    assert_eq!(session.version(), version);
    assert_eq!(format!("{:?}", session.view()), topology);
    assert_preserved(&session, proof, receiver);

    let mut failed = session.begin_host_frame().unwrap();
    let failed_attach = failed.attach_empty_surface_current(S2, R2).unwrap();
    measure(&mut failed, S1);
    assert!(failed.select_item_current(FIRST).is_err());
    assert!(failed.commit().is_err());
    assert_eq!(session.version(), version);
    assert_eq!(format!("{:?}", session.view()), topology);
    assert_preserved(&session, proof, receiver);

    let mut retry = session.begin_host_frame().unwrap();
    let committed_attach = retry.attach_empty_surface_current(S2, R2).unwrap();
    let attached = finish(retry);
    assert!(matches!(
        attached.submitted_action_outcome(committed_attach),
        Some(HostInputOutcome::LogicalSurfaceAttached {
            surface: S2,
            root: R2,
            ..
        })
    ));
    assert!(
        attached
            .submitted_action_outcome(abandoned_attach)
            .is_none()
    );
    assert!(attached.submitted_action_outcome(failed_attach).is_none());
    assert_preserved(&session, proof, receiver);

    let version = session.version();
    let topology = format!("{:?}", session.view());
    let mut abandoned = session.begin_host_frame().unwrap();
    let abandoned_retire = abandoned.retire_empty_surface_current(S2).unwrap();
    assert!(abandoned.view().surface(S2).is_none());
    drop(abandoned);
    assert_eq!(session.version(), version);
    assert_eq!(format!("{:?}", session.view()), topology);
    assert_preserved(&session, proof, receiver);

    let mut failed = session.begin_host_frame().unwrap();
    let failed_retire = failed.retire_empty_surface_current(S2).unwrap();
    measure(&mut failed, S1);
    assert!(failed.select_item_current(FIRST).is_err());
    assert!(failed.commit().is_err());
    assert_eq!(session.version(), version);
    assert_eq!(format!("{:?}", session.view()), topology);
    assert_preserved(&session, proof, receiver);

    let mut retry = session.begin_host_frame().unwrap();
    let committed_retire = retry.retire_empty_surface_current(S2).unwrap();
    let retired = finish(retry);
    assert!(matches!(
        retired.submitted_action_outcome(committed_retire),
        Some(HostInputOutcome::LogicalSurfaceRetired { surface: S2, .. })
    ));
    assert!(retired.submitted_action_outcome(abandoned_retire).is_none());
    assert!(retired.submitted_action_outcome(failed_retire).is_none());
    assert_preserved(&session, proof, receiver);

    let layout = DockspaceLayout::new([
        primary_surface(),
        DockspaceSurfaceLayout::new(
            S2,
            DockspaceRootLayout::new(R2, DockspaceNode::central_tabs([])),
        )
        .with_contained(DockspaceContainedLayout::new(
            FloatingPresentationId::new(1),
            DockspaceRootLayout::new(R3, DockspaceNode::tabs([THIRD, FOURTH])),
            LogicalRect::new(40.0, 50.0, 320.0, 240.0).unwrap(),
        )),
    ])
    .unwrap();
    let mut forest = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let version = forest.version();
    let mut frame = forest.begin_host_frame().unwrap();
    let submitted = frame.retire_empty_surface_current(S2).unwrap();
    let report = finish(frame);
    assert_rejected(
        &report,
        submitted,
        S2,
        LogicalSurfaceRejection::SurfaceNotEmpty { items: 2 },
    );
    assert_eq!(forest.version(), version);
    assert_eq!(forest.view().surface(S2).unwrap().contained_count(), 1);
    assert_eq!(forest.view().item(THIRD).unwrap().surface(), S2);
    assert_eq!(forest.view().item(FOURTH).unwrap().surface(), S2);

    let mut policy = forest.policy().clone();
    policy.set_arrangement_locked(true);
    let mut frame = forest.begin_host_frame().unwrap();
    frame.replace_policy(policy).unwrap();
    finish(frame);
    let version = forest.version();
    let mut frame = forest.begin_host_frame().unwrap();
    let attach = frame
        .attach_empty_surface_current(S3, RootId::new(4))
        .unwrap();
    let retire = frame.retire_empty_surface_current(S2).unwrap();
    let report = finish(frame);
    assert_rejected(&report, attach, S3, LogicalSurfaceRejection::PolicyDenied);
    assert_rejected(&report, retire, S2, LogicalSurfaceRejection::PolicyDenied);
    assert_eq!(forest.version(), version);

    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            S1,
            DockspaceRootLayout::new(R1, DockspaceNode::central_tabs([])),
        ),
        DockspaceSurfaceLayout::new(
            S2,
            DockspaceRootLayout::new(R2, DockspaceNode::central_tabs([])),
        ),
    ])
    .unwrap();
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut native = DockspaceSession::from_layout(layout, policy).unwrap();
    native
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    native
        .configure_managed_native_capabilities(
            NativeHostCapabilities::none_supported()
                .with(NativeHostCapability::NativeWindowLifecycle)
                .with(NativeHostCapability::AuthoritativeInventory)
                .with(NativeHostCapability::GlobalWindowPlacement),
        )
        .unwrap();
    native
        .register_native_root(S1, HostWindowToken::new(1))
        .unwrap();
    let registration = finish(
        native
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    let [binding] = registration.native_bindings() else {
        panic!("the fixture must register its native root")
    };
    let binding = *binding;
    let bounds = PhysicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap();
    let scale = ScaleFactor::new(1.0).unwrap();
    native
        .report_managed_native_snapshot(
            [(
                binding,
                NativeWindowFacts::live()
                    .with_content_bounds(bounds)
                    .with_outer_bounds(bounds)
                    .with_native_scale_factor(scale)
                    .with_presentation_scale_factor(scale)
                    .with_input(NativeWindowInputState::ReceivesInput, None)
                    .with_presentation(NativeWindowPresentationState::Visible),
            )],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    finish(
        native
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    let version = native.version();
    let mut frame = native
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let bound_retire = frame.retire_empty_surface_current(S1).unwrap();
    let creation = frame
        .create_native_surface_current(S2, S1, NativeWindowPlacement::new(bounds))
        .unwrap();
    assert!(frame.cancel_next_native_create().unwrap());
    let recovering_retire = frame.retire_empty_surface_current(S2).unwrap();
    let mut report = finish(frame);
    assert_rejected(
        &report,
        bound_retire,
        S1,
        LogicalSurfaceRejection::NativeBindingPresent,
    );
    assert!(matches!(
        report.submitted_action_outcome(creation),
        Some(HostInputOutcome::NativeSurfaceCreateRequested { .. })
    ));
    assert_rejected(
        &report,
        recovering_retire,
        S2,
        LogicalSurfaceRejection::RecoveryConflict,
    );
    assert_eq!(native.version(), version);
    assert_eq!(report.native_bindings(), &[binding]);
    assert!(report.take_native_effects().is_empty());
    assert!(native.view().surface(S1).is_some());
    assert!(native.view().surface(S2).is_some());
}

#[test]
fn logical_surface_reattach_cannot_reauthorize_predecessor_outputs() {
    for same_frame in [false, true] {
        let mut session = session();
        let (s1, s1_receiver) = paint_and_present(&mut session, S1, Some(SECOND));
        let mut frame = session.begin_host_frame().unwrap();
        frame.attach_empty_surface_current(S2, R2).unwrap();
        measure(&mut frame, S2);
        finish(frame);
        let (old_surface, old_receiver) = paint_and_present(&mut session, S2, None);
        let (_, old_descriptor, late_output) = paint(&mut session, S2, None);
        let late_artifact = late_output.presentation_artifact();
        assert_eq!(
            session.bind_presented_receiver(&old_descriptor),
            Some(old_receiver)
        );
        assert_preserved(&session, s1, s1_receiver);

        let mut frame = session.begin_host_frame().unwrap();
        let retire = frame.retire_empty_surface_current(S2).unwrap();
        let retired_version = frame.version();
        if same_frame {
            let attach = frame.attach_empty_surface_current(S2, R2).unwrap();
            let report = finish(frame);
            assert_eq!(
                report.submitted_action_outcome(retire),
                Some(&HostInputOutcome::LogicalSurfaceRetired {
                    surface: S2,
                    version: retired_version
                })
            );
            assert_eq!(
                report.submitted_action_outcome(attach),
                Some(&HostInputOutcome::LogicalSurfaceAttached {
                    surface: S2,
                    root: R2,
                    version: report.after()
                })
            );
        } else {
            let report = finish(frame);
            assert_eq!(
                report.submitted_action_outcome(retire),
                Some(&HostInputOutcome::LogicalSurfaceRetired {
                    surface: S2,
                    version: report.after()
                })
            );
            assert!(session.view().surface(S2).is_none());
            assert!(session.presented_surface(S2).is_none());
            assert_preserved(&session, s1, s1_receiver);
            let mut frame = session.begin_host_frame().unwrap();
            let attach = frame.attach_empty_surface_current(S2, R2).unwrap();
            let report = finish(frame);
            assert_eq!(
                report.submitted_action_outcome(attach),
                Some(&HostInputOutcome::LogicalSurfaceAttached {
                    surface: S2,
                    root: R2,
                    version: report.after()
                })
            );
        }
        let version = session.version();
        assert!(
            session
                .view()
                .root(R2)
                .unwrap()
                .central()
                .unwrap()
                .tabs()
                .unwrap()
                .items()
                .is_empty()
        );
        assert!(session.presented_surface(S2).is_none());
        assert!(session.bind_presented_receiver(&old_descriptor).is_none());
        assert!(
            session
                .bind_presented_receiver(&old_receiver.descriptor())
                .is_none()
        );
        assert_preserved(&session, s1, s1_receiver);

        session
            .report_surface_presentation(late_output, SurfacePresentationResult::Presented)
            .unwrap();
        let report = settle(&mut session);
        assert!(
            report
                .surface_presentation_commits()
                .iter()
                .any(|commit| commit.retires(late_artifact))
        );
        assert_eq!(session.version(), version);
        assert!(session.presented_surface(S2).is_none());
        assert!(session.bind_presented_receiver(&old_descriptor).is_none());
        assert_preserved(&session, s1, s1_receiver);

        let mut frame = session.begin_host_frame().unwrap();
        measure(&mut frame, S2);
        finish(frame);
        let (_, new_descriptor, fresh_output) = paint(&mut session, S2, None);
        assert_ne!(new_descriptor, old_descriptor);
        assert!(session.presented_surface(S2).is_none());
        assert!(session.bind_presented_receiver(&new_descriptor).is_none());
        let fresh = present(&mut session, fresh_output);
        assert_ne!(fresh, old_surface);
        assert!(session.bind_presented_receiver(&new_descriptor).is_some());
        assert!(session.bind_presented_receiver(&old_descriptor).is_none());
        assert_preserved(&session, s1, s1_receiver);
        assert_eq!(session.version(), version);
    }
}

#[test]
fn logical_surface_reattach_presents_successor_before_predecessor_outputs_settle() {
    for late_result in [
        SurfacePresentationResult::Presented,
        SurfacePresentationResult::Dropped,
    ] {
        let mut session = session();
        let (s1, s1_receiver) = paint_and_present(&mut session, S1, Some(SECOND));
        let mut frame = session.begin_host_frame().unwrap();
        frame.attach_empty_surface_current(S2, R2).unwrap();
        measure(&mut frame, S2);
        finish(frame);
        let (old_surface, old_receiver) = paint_and_present(&mut session, S2, None);
        let (_, old_descriptor, late_output) = paint(&mut session, S2, None);
        let late_artifact = late_output.presentation_artifact();

        let mut frame = session.begin_host_frame().unwrap();
        frame.retire_empty_surface_current(S2).unwrap();
        finish(frame);
        assert!(session.view().surface(S2).is_none());
        assert!(session.presented_surface(S2).is_none());
        let mut frame = session.begin_host_frame().unwrap();
        frame.attach_empty_surface_current(S2, R2).unwrap();
        measure(&mut frame, S2);
        finish(frame);
        let version = session.version();
        let (_, new_descriptor, fresh_output) = paint(&mut session, S2, None);
        let fresh_artifact = fresh_output.presentation_artifact();
        assert_ne!(fresh_artifact, late_artifact);
        assert_ne!(new_descriptor, old_descriptor);
        assert!(session.presented_surface(S2).is_none());
        assert!(session.bind_presented_receiver(&new_descriptor).is_none());

        let fresh = present(&mut session, fresh_output);
        assert_ne!(fresh, old_surface);
        assert!(!fresh.matches_presentation_artifact(late_artifact));
        let fresh_receiver = session.bind_presented_receiver(&new_descriptor).unwrap();
        assert!(session.bind_presented_receiver(&old_descriptor).is_none());
        assert!(
            session
                .bind_presented_receiver(&old_receiver.descriptor())
                .is_none()
        );
        assert_preserved(&session, s1, s1_receiver);

        session
            .report_surface_presentation(late_output, late_result)
            .unwrap();
        let report = settle(&mut session);
        assert!(
            report
                .surface_presentation_commits()
                .iter()
                .any(|commit| commit.retires(late_artifact))
        );
        assert!(
            report
                .surface_presentation_commits()
                .iter()
                .all(|commit| !commit.settles(fresh_artifact))
        );
        assert_preserved(&session, fresh, fresh_receiver);
        assert!(session.bind_presented_receiver(&old_descriptor).is_none());
        assert_preserved(&session, s1, s1_receiver);

        let reclaimed = settle(&mut session);
        assert!(reclaimed.surface_presentation_commits().is_empty());
        assert_preserved(&session, fresh, fresh_receiver);
        assert!(session.bind_presented_receiver(&old_descriptor).is_none());
        assert_preserved(&session, s1, s1_receiver);
        assert_eq!(session.version(), version);
    }
}

#[test]
fn logical_surface_reattach_keeps_predecessor_results_separate_from_pending_successor() {
    for late_result in [
        SurfacePresentationResult::Presented,
        SurfacePresentationResult::Dropped,
    ] {
        let mut session = session();
        let (s1, s1_receiver) = paint_and_present(&mut session, S1, Some(SECOND));
        let mut frame = session.begin_host_frame().unwrap();
        frame.attach_empty_surface_current(S2, R2).unwrap();
        measure(&mut frame, S2);
        finish(frame);
        let (_, old_descriptor, late_output) = paint(&mut session, S2, None);
        let late_artifact = late_output.presentation_artifact();

        let mut frame = session.begin_host_frame().unwrap();
        frame.retire_empty_surface_current(S2).unwrap();
        finish(frame);
        let mut frame = session.begin_host_frame().unwrap();
        frame.attach_empty_surface_current(S2, R2).unwrap();
        measure(&mut frame, S2);
        finish(frame);
        let version = session.version();
        let (_, new_descriptor, fresh_output) = paint(&mut session, S2, None);
        let fresh_artifact = fresh_output.presentation_artifact();
        assert_ne!(fresh_artifact, late_artifact);
        assert_ne!(new_descriptor, old_descriptor);

        session
            .report_surface_presentation(late_output, late_result)
            .unwrap();
        let report = settle(&mut session);
        assert!(
            report
                .surface_presentation_commits()
                .iter()
                .any(|commit| commit.retires(late_artifact))
        );
        assert!(
            report
                .surface_presentation_commits()
                .iter()
                .all(|commit| !commit.settles(fresh_artifact))
        );
        assert!(session.presented_surface(S2).is_none());
        assert!(session.bind_presented_receiver(&old_descriptor).is_none());
        assert!(session.bind_presented_receiver(&new_descriptor).is_none());
        assert_preserved(&session, s1, s1_receiver);

        let reclaimed = settle(&mut session);
        assert!(reclaimed.surface_presentation_commits().is_empty());
        assert!(session.presented_surface(S2).is_none());
        let fresh = present(&mut session, fresh_output);
        assert!(!fresh.matches_presentation_artifact(late_artifact));
        assert!(session.bind_presented_receiver(&old_descriptor).is_none());
        assert!(session.bind_presented_receiver(&new_descriptor).is_some());
        assert_preserved(&session, s1, s1_receiver);
        assert_eq!(session.version(), version);
    }
}

#[test]
fn logical_surface_reattach_does_not_publish_presentation_from_aborted_frames() {
    for fail_frame in [false, true] {
        let mut session = session();
        let (s1, s1_receiver) = paint_and_present(&mut session, S1, Some(SECOND));
        let mut frame = session.begin_host_frame().unwrap();
        frame.attach_empty_surface_current(S2, R2).unwrap();
        measure(&mut frame, S2);
        finish(frame);
        let (old_surface, old_receiver) = paint_and_present(&mut session, S2, None);
        let (_, old_descriptor, late_output) = paint(&mut session, S2, None);
        let late_artifact = late_output.presentation_artifact();

        let version = session.version();
        let mut aborted = session.begin_host_frame().unwrap();
        aborted.retire_empty_surface_current(S2).unwrap();
        if fail_frame {
            measure(&mut aborted, S1);
            assert!(aborted.select_item_current(FIRST).is_err());
            assert!(aborted.commit().is_err());
        } else {
            drop(aborted);
        }
        assert_eq!(session.version(), version);
        assert!(session.view().surface(S2).is_some());
        assert_preserved(&session, old_surface, old_receiver);
        assert_preserved(&session, s1, s1_receiver);

        let mut frame = session.begin_host_frame().unwrap();
        frame.retire_empty_surface_current(S2).unwrap();
        finish(frame);
        let mut frame = session.begin_host_frame().unwrap();
        frame.attach_empty_surface_current(S2, R2).unwrap();
        measure(&mut frame, S2);
        finish(frame);
        let version = session.version();
        let (_, new_descriptor, fresh_output) = paint(&mut session, S2, None);
        let fresh_artifact = fresh_output.presentation_artifact();
        session
            .report_surface_presentation(fresh_output, SurfacePresentationResult::Presented)
            .unwrap();
        session
            .report_surface_presentation(late_output, SurfacePresentationResult::Presented)
            .unwrap();

        let mut aborted = session.begin_host_frame().unwrap();
        if fail_frame {
            measure(&mut aborted, S1);
            assert!(aborted.select_item_current(FIRST).is_err());
            assert!(aborted.commit().is_err());
        } else {
            drop(aborted);
        }
        assert_eq!(session.version(), version);
        assert!(session.presented_surface(S2).is_none());
        assert!(session.bind_presented_receiver(&old_descriptor).is_none());
        assert!(session.bind_presented_receiver(&new_descriptor).is_none());
        assert_preserved(&session, s1, s1_receiver);

        let report = settle(&mut session);
        assert!(
            report
                .surface_presentation_commits()
                .iter()
                .any(|commit| commit.retires(late_artifact))
        );
        assert!(
            report
                .surface_presentation_commits()
                .iter()
                .any(|commit| commit.presents(fresh_artifact))
        );
        let fresh = session.presented_surface(S2).unwrap();
        assert!(fresh.matches_presentation_artifact(fresh_artifact));
        assert!(!fresh.matches_presentation_artifact(late_artifact));
        let fresh_receiver = session.bind_presented_receiver(&new_descriptor).unwrap();
        assert!(session.bind_presented_receiver(&old_descriptor).is_none());
        assert_preserved(&session, s1, s1_receiver);

        settle(&mut session);
        assert_preserved(&session, fresh, fresh_receiver);
        assert_preserved(&session, s1, s1_receiver);
        assert_eq!(session.version(), version);
    }
}

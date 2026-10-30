//! Renderer-neutral local geometry without desktop placement or native pointer claims.

use dockspace::geometry::{
    LogicalRect, LogicalSize, PhysicalPoint, PhysicalRect, PhysicalSize, ScaleFactor,
};
use dockspace::model::{
    DockspaceActionOutcome, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceSemanticOutput, DockspaceSession, HostFrameReport,
    HostInputOutcome, HostWindowToken, NativeCreateStrategy, NativeDesktopPointerLocation,
    NativeDesktopPosition, NativeHostCapabilities, NativeHostCapability, NativeHostErrorKind,
    NativePointerButton, NativePointerEvent, NativePointerHover, NativePointerId,
    NativePointerInput, NativePointerOwner, NativePointerRoster, NativeReceiverAnswer,
    NativeSurfaceBinding, NativeWindowFacts, NativeWindowInputState, NativeWindowPresentationState,
    NativeWorkAreaRoster, PaintedSurfaceOutput, SurfacePaintDisposition, SurfacePresentationResult,
    SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);

fn managed_root() -> (DockspaceSession, NativeSurfaceBinding) {
    managed_root_with_capabilities(
        NativeHostCapabilities::none_supported().with(NativeHostCapability::AuthoritativeInventory),
    )
}

fn managed_root_with_capabilities(
    capabilities: NativeHostCapabilities,
) -> (DockspaceSession, NativeSurfaceBinding) {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, SECOND])),
    )])
    .unwrap();
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut session = DockspaceSession::from_layout(layout, policy).unwrap();
    session
        .enable_managed_native_host(
            NativeCreateStrategy::DirectPrepared,
            NativePointerRoster::Exact(Vec::new()),
        )
        .unwrap();
    session
        .configure_managed_native_capabilities(capabilities)
        .unwrap();
    session
        .register_native_root(SURFACE, HostWindowToken::new(1))
        .unwrap();
    let registration = idle(&mut session);
    let [binding] = registration.native_bindings() else {
        panic!("registration must publish the sole externally owned native binding");
    };
    assert_eq!(binding.surface(), SURFACE);
    (session, *binding)
}

fn local_facts() -> NativeWindowFacts {
    NativeWindowFacts::live()
        .with_content_size(PhysicalSize::new(1200.0, 900.0).unwrap())
        .with_native_scale_factor(ScaleFactor::new(2.0).unwrap())
        .with_presentation_scale_factor(ScaleFactor::new(1.5).unwrap())
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible, None)
}

fn local_bounds() -> LogicalRect {
    // Renderer coordinates use the presentation scale, independently of native placement.
    LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap()
}

fn measure(frame: &mut DockspaceHostFrame<'_>) {
    frame
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(local_bounds(), LogicalSize::new(32.0, 24.0).unwrap(), 80.0)
                .unwrap(),
        )
        .unwrap();
}

fn finish(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    assert_eq!(frame.native_staging_paints().len(), 0);
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    assert!(report.native_admissions().is_empty());
    assert!(report.take_native_effects().is_empty());
    assert!(report.take_painted_native_staging_outputs().is_empty());
    report
}

fn idle(session: &mut DockspaceSession) -> HostFrameReport {
    let mut report = finish(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    assert!(report.take_painted_outputs().is_empty());
    report
}

fn assert_snapshot_applied(report: &HostFrameReport) {
    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::NativePlatformSnapshotApplied { close_requests }]
            if close_requests.is_empty()
    ));
    assert!(!report.workspace_changed());
}

fn record(frame: &mut DockspaceHostFrame<'_>, selected: ItemId) -> DockspaceSemanticOutput {
    let plan = frame
        .paint_plan(SURFACE)
        .unwrap()
        .expect("local size and scales must suffice for a complete semantic paint plan");
    let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
    assert_eq!(
        recording.records.first(),
        Some(&OwnedRecord::RootBegin(ROOT))
    );
    assert!(recording.records.contains(&OwnedRecord::RootEnd(ROOT)));
    let panes = recording
        .records
        .iter()
        .filter_map(|record| match record {
            OwnedRecord::PaneContent {
                visual,
                item,
                bounds,
                paint,
            } => Some((*visual, *item, *bounds, *paint)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [(visual, item, content, paint)] = panes.as_slice() else {
        panic!("the complete root program must record exactly one pane");
    };
    assert_eq!(*item, Some(selected));
    assert_eq!(*paint, SurfacePaintDisposition::Visible);
    assert!(content.width() > 0.0 && content.height() > 0.0);
    assert!(recording.records.contains(&OwnedRecord::Geometry {
        visual: *visual,
        bounds: local_bounds(),
        paint: SurfacePaintDisposition::Visible,
    }));
    let Some(OwnedRecord::SurfaceEnd(semantic)) = recording.records.last() else {
        panic!("the accepting renderer must reach the complete program's semantic seal");
    };
    let semantic = *semantic;
    assert_eq!(semantic.surface(), SURFACE);
    frame.confirm_surface_painted(receipt).unwrap();
    semantic
}

fn take_output(
    report: &mut HostFrameReport,
    binding: NativeSurfaceBinding,
    semantic: DockspaceSemanticOutput,
) -> PaintedSurfaceOutput {
    let [output] = report.take_painted_outputs().try_into().unwrap();
    assert!(output.matches_native_binding(binding));
    assert!(output.matches_semantic_output(semantic));
    output
}

fn paint(
    session: &mut DockspaceSession,
    binding: NativeSurfaceBinding,
    selected: ItemId,
) -> PaintedSurfaceOutput {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let semantic = record(&mut frame, selected);
    let mut report = finish(frame);
    assert!(!report.workspace_changed());
    take_output(&mut report, binding, semantic)
}

fn present(session: &mut DockspaceSession, output: PaintedSurfaceOutput) {
    let artifact = output.presentation_artifact();
    let previous = session.presented_surface(SURFACE);
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    assert_eq!(session.presented_surface(SURFACE), previous);
    let report = idle(session);
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.presents(artifact))
    );
    assert!(
        session
            .presented_surface(SURFACE)
            .expect("only the committed Presented observation grants current authority")
            .matches_presentation_artifact(artifact)
    );
}

#[test]
fn local_size_only_root_presents_and_current_selection_commits_once_after_drop() {
    let (mut session, binding) = managed_root();
    session
        .report_managed_native_snapshot([(binding, local_facts())], NativeWorkAreaRoster::Unknown)
        .unwrap();
    assert_snapshot_applied(&idle(&mut session));
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    measure(&mut frame);
    assert!(!finish(frame).workspace_changed());
    let output = paint(&mut session, binding, FIRST);
    assert!(session.presented_surface(SURFACE).is_none());
    present(&mut session, output);

    let version = session.version();
    let recorded = session.native_input_recorded_count();
    let presented = session.presented_surface(SURFACE).unwrap();
    {
        let mut dropped = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        dropped.select_item_current(SECOND).unwrap();
        assert!(dropped.view().item(SECOND).unwrap().is_selected());
        assert_ne!(dropped.version(), version);
        measure(&mut dropped);
    }
    assert_eq!(session.version(), version);
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert!(session.view().item(FIRST).unwrap().is_selected());
    assert_eq!(session.presented_surface(SURFACE), Some(presented));

    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.select_item_current(SECOND).unwrap();
    measure(&mut frame);
    let mut report = finish(frame);
    assert!(report.workspace_changed());
    assert_eq!(report.before(), version);
    assert_eq!(report.after(), session.version());
    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Selected {
                item: SECOND,
                changed: true,
            },
        )]
    );
    assert!(report.take_painted_outputs().is_empty());
    assert!(session.view().item(SECOND).unwrap().is_selected());
    let output = paint(&mut session, binding, SECOND);
    present(&mut session, output);

    let committed = session.version();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.select_item_current(SECOND).unwrap();
    let report = finish(frame);
    assert!(!report.workspace_changed());
    assert_eq!(session.version(), committed);
    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Selected {
                item: SECOND,
                changed: false,
            },
        )]
    );
}

#[test]
fn conflicting_content_size_is_atomic_and_corrected_local_snapshot_and_paint_retry_cleanly() {
    let (mut session, binding) = managed_root();
    let version = session.version();
    let recorded = session.native_input_recorded_count();
    // Desktop bounds occur only in this rejected, deliberately contradictory snapshot.
    let conflicting =
        local_facts().with_content_bounds(PhysicalRect::new(37.0, 29.0, 1201.0, 900.0).unwrap());
    let error = session
        .report_managed_native_snapshot([(binding, conflicting)], NativeWorkAreaRoster::Unknown)
        .unwrap_err();
    assert_eq!(error.native_kind(), Some(NativeHostErrorKind::InvalidFacts));
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert_eq!(session.version(), version);
    assert!(!session.has_pending_native_input());
    assert!(session.presented_surface(SURFACE).is_none());

    // The corrected size-only facts append at the next unconsumed record boundary.
    session
        .report_managed_native_snapshot([(binding, local_facts())], NativeWorkAreaRoster::Unknown)
        .unwrap();
    assert_eq!(session.native_input_recorded_count(), recorded + 1);
    assert_eq!(session.version(), version);
    assert!(session.has_pending_native_input());
    {
        let mut dropped = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        measure(&mut dropped);
    }
    assert_eq!(session.version(), version);
    assert_eq!(session.native_input_recorded_count(), recorded + 1);
    assert!(session.has_pending_native_input());
    assert!(session.presented_surface(SURFACE).is_none());

    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    measure(&mut frame);
    let mut report = finish(frame);
    assert_snapshot_applied(&report);
    assert!(report.take_painted_outputs().is_empty());
    assert_eq!(session.native_input_recorded_count(), recorded + 1);
    assert_eq!(session.version(), version);
    assert!(!session.has_pending_native_input());

    {
        let mut dropped = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        record(&mut dropped, FIRST);
    }
    assert_eq!(session.version(), version);
    assert_eq!(session.native_input_recorded_count(), recorded + 1);
    assert!(session.presented_surface(SURFACE).is_none());
    let output = paint(&mut session, binding, FIRST);
    assert_eq!(session.version(), version);
    assert_eq!(session.native_input_recorded_count(), recorded + 1);
    assert!(session.presented_surface(SURFACE).is_none());
    present(&mut session, output);
    assert!(idle(&mut session).inputs().is_empty());
    assert_eq!(session.version(), version);
    assert!(!session.has_pending_native_input());
}

#[test]
fn a_desktop_pointer_cannot_borrow_local_geometry_without_a_real_desktop_origin() {
    let capabilities = [
        NativeHostCapability::AuthoritativeInventory,
        NativeHostCapability::HoveredWindow,
        NativeHostCapability::DesktopPointerPosition,
        NativeHostCapability::AuthoritativeButtonState,
        NativeHostCapability::PointerHitTestObservation,
    ]
    .into_iter()
    .fold(
        NativeHostCapabilities::none_supported(),
        |caps, capability| caps.with(capability),
    );
    let (mut session, binding) = managed_root_with_capabilities(capabilities);
    session
        .report_managed_native_snapshot([(binding, local_facts())], NativeWorkAreaRoster::Unknown)
        .unwrap();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    measure(&mut frame);
    finish(frame);
    let output = paint(&mut session, binding, FIRST);
    present(&mut session, output);

    let point = {
        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        let point = frame
            .paint_plan(SURFACE)
            .unwrap()
            .unwrap()
            .tab_receiver(SECOND)
            .unwrap()
            .center();
        PhysicalPoint::new(point.x(), point.y()).unwrap()
    };
    let version = session.version();
    let recorded = session.native_input_recorded_count();
    session
        .record_native_pointer_batch(
            [
                NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
                NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
            ]
            .map(|event| {
                NativePointerInput::new(
                    NativePointerId::new(17),
                    event,
                    NativeDesktopPointerLocation::new(
                        NativeDesktopPosition::Exact(point),
                        NativePointerHover::Dock(binding),
                        None,
                    ),
                    NativePointerOwner::Native(binding),
                    NativePointerOwner::Native(binding),
                )
            }),
        )
        .unwrap();
    assert_eq!(session.native_input_recorded_count(), recorded + 2);
    let frame = session
        .begin_native_host_frame(|_| {
            panic!("desktop input needs an observed origin before querying a local receiver")
        })
        .unwrap();
    let report = finish(frame);
    assert!(!report.workspace_changed());
    assert_eq!(session.version(), version);
    assert!(session.view().item(FIRST).unwrap().is_selected());
    assert!(!session.view().item(SECOND).unwrap().is_selected());
    assert!(!session.has_pending_native_input());
}

#[test]
fn desktop_origin_withdrawal_and_return_require_fresh_exact_output() {
    let (mut session, binding) = managed_root();
    let positioned =
        |x| local_facts().with_content_bounds(PhysicalRect::new(x, 50.0, 1200.0, 900.0).unwrap());
    session
        .report_managed_native_snapshot(
            [(binding, positioned(100.0))],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    measure(&mut frame);
    finish(frame);
    let initial = paint(&mut session, binding, FIRST);
    present(&mut session, initial);
    let version = session.version();

    for facts in [local_facts(), positioned(200.0)] {
        let old = paint(&mut session, binding, FIRST);
        let old_artifact = old.presentation_artifact();
        session
            .report_managed_native_snapshot([(binding, facts)], NativeWorkAreaRoster::Unknown)
            .unwrap();
        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        measure(&mut frame);
        let report = finish(frame);
        assert!(!report.workspace_changed());
        assert!(session.presented_surface(SURFACE).is_none());

        session
            .report_surface_presentation(old, SurfacePresentationResult::Presented)
            .unwrap();
        let stale = idle(&mut session);
        assert!(
            stale
                .surface_presentation_commits()
                .iter()
                .any(|commit| { commit.retires(old_artifact) })
        );
        assert!(session.presented_surface(SURFACE).is_none());

        let fresh = paint(&mut session, binding, FIRST);
        assert_ne!(fresh.presentation_artifact(), old_artifact);
        present(&mut session, fresh);
        assert_eq!(session.version(), version);
        assert!(session.view().item(FIRST).unwrap().is_selected());
    }
}

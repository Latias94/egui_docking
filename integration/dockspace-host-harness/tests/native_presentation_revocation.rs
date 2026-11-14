use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceManipulationCancelReason, DockspaceManipulationStatus, DockspaceSession,
    HostFrameReport, HostWindowToken, HostWorkAreaToken, NativeCloseState,
    NativeDesktopPointerLocation, NativeDesktopPosition, NativeHostCapabilities,
    NativeHostCapability, NativePointerButton, NativePointerEvent, NativePointerHover,
    NativePointerId, NativePointerInput, NativePointerOwner, NativePointerRoster,
    NativeReceiverAnswer, NativeReceiverQuery, NativeSurfaceBinding, NativeWindowFacts,
    NativeWindowHit, NativeWindowInputState, NativeWindowPresentationState, NativeWorkAreaFacts,
    NativeWorkAreaRoster, PaintedSurfaceOutput, SurfacePresentationArtifact,
    SurfacePresentationResult, UniformSurfaceMetrics,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const FIRST: ItemId = ItemId::new(1);

fn area(width: f64) -> NativeWorkAreaRoster {
    NativeWorkAreaRoster::Exact(vec![NativeWorkAreaFacts::new(
        HostWorkAreaToken::new(1),
        PhysicalRect::new(0.0, 0.0, width, 1080.0).expect("valid work area"),
        ScaleFactor::new(1.0).expect("valid scale"),
    )])
}

fn facts() -> NativeWindowFacts {
    NativeWindowFacts::live()
        .with_content_bounds(
            PhysicalRect::new(100.0, 80.0, 640.0, 480.0).expect("valid content bounds"),
        )
        .with_outer_bounds(PhysicalRect::new(92.0, 50.0, 656.0, 518.0).expect("valid outer bounds"))
        .with_native_scale_factor(ScaleFactor::new(1.0).expect("valid scale"))
        .with_presentation_scale_factor(ScaleFactor::new(1.0).expect("valid scale"))
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible)
        .with_close(NativeCloseState::Clear, None)
}

fn idle(session: &mut DockspaceSession) -> HostFrameReport {
    session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("idle frame begins")
        .commit()
        .expect("idle frame commits")
}

fn paint(session: &mut DockspaceSession) -> PaintedSurfaceOutput {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("paint frame begins");
    let receipt = frame
        .paint_plan(SURFACE)
        .expect("plan lookup")
        .expect("ready plan")
        .execute_surface_program(|_| Ok::<(), ()>(()))
        .expect("preview program executes");
    frame
        .confirm_surface_painted(receipt)
        .expect("paint is confirmed");
    frame
        .commit()
        .expect("paint commits")
        .take_painted_outputs()
        .pop()
        .expect("one output is owned")
}

fn session() -> (
    DockspaceSession,
    NativeSurfaceBinding,
    SurfacePresentationArtifact,
) {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::central_tabs([FIRST, ItemId::new(2)]),
        ),
    )])
    .expect("layout validates");
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut session = DockspaceSession::from_layout(layout, policy).expect("session initializes");
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .expect("managed host enrolls");
    let capabilities = [
        NativeHostCapability::NativeWindowLifecycle,
        NativeHostCapability::AuthoritativeInventory,
        NativeHostCapability::HoveredWindow,
        NativeHostCapability::DesktopPointerPosition,
        NativeHostCapability::AuthoritativeButtonState,
        NativeHostCapability::GlobalWindowPlacement,
        NativeHostCapability::WorkArea,
        NativeHostCapability::PointerHitTestObservation,
        NativeHostCapability::PointerHitTestControl,
        NativeHostCapability::GlobalFocusObservation,
        NativeHostCapability::WindowActivationControl,
        NativeHostCapability::CloseCancellation,
    ]
    .into_iter()
    .fold(
        NativeHostCapabilities::none_supported(),
        |caps, capability| caps.with(capability),
    );
    session
        .configure_managed_native_capabilities(capabilities)
        .expect("native capabilities configure");
    session
        .register_native_root(SURFACE, HostWindowToken::new(1))
        .expect("root registration records");
    let binding = *idle(&mut session)
        .native_bindings()
        .first()
        .expect("root binding exists");
    session
        .report_managed_native_snapshot([(binding, facts())], area(1920.0))
        .expect("root snapshot records");
    idle(&mut session);
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("measurement frame begins");
    frame
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("valid bounds"),
                LogicalSize::new(32.0, 24.0).expect("valid minimum"),
                80.0,
            )
            .expect("valid metrics"),
        )
        .expect("source measures");
    frame.commit().expect("measurement commits");
    let output = paint(&mut session);
    let artifact = output.presentation_artifact();
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .expect("root output is presented");
    idle(&mut session);
    (session, binding, artifact)
}

#[test]
fn revoked_native_release_settles_its_output_without_reviving_the_cancelled_mutation() {
    let (mut session, binding, artifact) = session();
    let work_area = session
        .native_work_area(HostWorkAreaToken::new(1))
        .expect("work area is authoritative");
    let descriptor = {
        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("inspection frame begins");
        frame
            .paint_plan(SURFACE)
            .expect("plan lookup")
            .expect("ready plan")
            .tab_receiver(FIRST)
            .expect("source receiver exists")
    };
    let center = descriptor.center();
    let start = PhysicalPoint::new(100.0 + center.x(), 80.0 + center.y())
        .expect("valid source desktop point");
    let outside = PhysicalPoint::new(1000.0, 700.0).expect("valid outside point");
    let pointer = NativePointerId::new(1);
    for (event, point, hover, area, capture) in [
        (
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            start,
            NativePointerHover::Dock(binding.window()),
            None,
            NativePointerOwner::Native(binding.window()),
        ),
        (
            NativePointerEvent::Moved,
            outside,
            NativePointerHover::OutsideAll,
            Some(work_area),
            NativePointerOwner::Native(binding.window()),
        ),
        (
            NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
            outside,
            NativePointerHover::OutsideAll,
            Some(work_area),
            NativePointerOwner::None,
        ),
    ] {
        session
            .record_native_pointer(NativePointerInput::new(
                pointer,
                event,
                NativeDesktopPointerLocation::new(NativeDesktopPosition::Exact(point), hover, area),
                NativePointerOwner::Native(binding.window()),
                capture,
            ))
            .expect("native edge records");
    }
    let mut release = session
        .begin_native_host_frame(|query| {
            let query = match query {
                NativeReceiverQuery::WindowHit(query) => {
                    return NativeReceiverAnswer::WindowHit(
                        if query.window() == binding.window() {
                            NativeWindowHit::Surface(artifact)
                        } else {
                            NativeWindowHit::Unknown
                        },
                    );
                }
                NativeReceiverQuery::Surface(query) => query,
            };
            query
                .bind_receiver(&descriptor)
                .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock)
        })
        .expect("release frame begins");
    let receipt = release
        .paint_plan(SURFACE)
        .expect("plan lookup")
        .expect("ready release plan")
        .execute_surface_program(|_| Ok::<(), ()>(()))
        .expect("preview program executes");
    release
        .confirm_surface_painted(receipt)
        .expect("release preview is confirmed");
    let mut report = release.commit().expect("release and its preview publish");
    let pending = report
        .manipulation_feedback()
        .iter()
        .find(|feedback| matches!(feedback.status(), DockspaceManipulationStatus::Pending(_)))
        .expect("the unpresented native release must be pending");
    let manipulation_id = pending.manipulation_id();
    let output = report
        .take_painted_outputs()
        .pop()
        .expect("release output exists");
    let artifact = output.presentation_artifact();
    session
        .report_managed_native_snapshot([(binding, facts())], area(1800.0))
        .expect("changed work area records before presentation report");
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .expect("the exact release output is presented");

    let report = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .and_then(|frame| frame.commit());
    assert!(
        report.is_ok(),
        "revoked release must settle without poisoning the input prefix: {report:?}"
    );
    let report = report.expect("terminal presentation report commits");
    assert!(
        report.inputs().iter().any(|outcome| matches!(
            outcome,
            dockspace::runtime::HostInputOutcome::NativePlatformSnapshotApplied { .. }
        )),
        "the changed native snapshot is consumed by the same prefix"
    );
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.presents(artifact))
    );
    let terminal = report
        .manipulation_feedback()
        .iter()
        .filter(|feedback| feedback.manipulation_id() == manipulation_id)
        .filter(|feedback| {
            matches!(
                feedback.status(),
                DockspaceManipulationStatus::Delivered(_)
                    | DockspaceManipulationStatus::Cancelled(_)
                    | DockspaceManipulationStatus::Rejected(_)
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        terminal.len(),
        1,
        "the manipulation publishes exactly one terminal feedback"
    );
    let terminal = terminal[0];
    assert!(matches!(
        terminal.status(),
        DockspaceManipulationStatus::Cancelled(DockspaceManipulationCancelReason::SceneUnavailable)
    ));
    assert_eq!(terminal.manipulation_id(), manipulation_id);
    assert!(!session.has_pending_native_create());
    assert!(!session.has_pending_presentation());
    assert!(!session.has_active_gesture());
    assert!(!session.has_pending_native_input());

    session
        .report_managed_native_snapshot([(binding, facts())], area(1700.0))
        .expect("a later native snapshot records after terminal settlement");
    let next = idle(&mut session);
    assert!(next.inputs().iter().any(|outcome| matches!(
        outcome,
        dockspace::runtime::HostInputOutcome::NativePlatformSnapshotApplied { .. }
    )));
    assert!(next.native_bindings().contains(&binding));
    assert!(next.manipulation_feedback().is_empty());
    assert!(!session.has_pending_native_input());
}

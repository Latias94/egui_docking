use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalRect, ScaleFactor};
use dockspace::model::{
    DockAnchor, DockPlacement, DockspaceContainedLayout, DockspaceLayout, DockspaceNode,
    DockspaceRootLayout, DockspaceSurfaceLayout, FloatingPresentationId, ItemId,
    NativeWindowPlacement, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceSession, HostFrameReport, HostWindowToken, HostWorkAreaToken,
    NativeEffectAcknowledgement, NativeEffectOperation, NativeHostCapabilities,
    NativeHostCapability, NativePointerRoster, NativeReceiverAnswer,
    NativeStagingPresentationPhase, NativeSurfaceBinding, NativeWindowFacts,
    NativeWindowInputState, NativeWindowPresentationState, NativeWorkAreaFacts,
    NativeWorkAreaRoster, SurfacePresentationResult, SurfaceUnavailableReason,
    UniformSurfaceMetrics,
};

const ROOT_SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(1);
const CHILD_ROOT: RootId = RootId::new(2);
const MAIN_ITEM: ItemId = ItemId::new(1);
const CHILD_ITEM: ItemId = ItemId::new(2);
const CONTAINED: FloatingPresentationId = FloatingPresentationId::new(1);
const ROOT_WINDOW: HostWindowToken = HostWindowToken::new(1);
const WORK_AREA: HostWorkAreaToken = HostWorkAreaToken::new(1);

fn managed_capabilities() -> NativeHostCapabilities {
    [
        NativeHostCapability::NativeWindowLifecycle,
        NativeHostCapability::AuthoritativeInventory,
        NativeHostCapability::GlobalWindowPlacement,
        NativeHostCapability::WorkArea,
        NativeHostCapability::PointerHitTestObservation,
        NativeHostCapability::CloseCancellation,
    ]
    .into_iter()
    .fold(
        NativeHostCapabilities::none_supported(),
        |capabilities, capability| capabilities.with(capability),
    )
}

#[test]
fn managed_native_lifecycle_reaches_quiescence_through_the_public_facade() {
    let mut session = session();
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .expect("the managed native host enrolls");
    session
        .configure_managed_native_capabilities(managed_capabilities())
        .expect("the managed backend capabilities configure");
    session
        .register_native_root(ROOT_SURFACE, ROOT_WINDOW)
        .expect("the root registration records");
    let registration = commit_native_frame(&mut session);
    let root_binding = only_binding(&registration);

    session
        .report_managed_native_snapshot([(root_binding, root_window_facts())], exact_work_areas())
        .expect("the exact root snapshot records");
    commit_native_frame(&mut session);
    let _ = paint_and_present_all_surfaces(&mut session);

    let requested_placement =
        PhysicalRect::new(900.0, 120.0, 420.0, 320.0).expect("the child placement validates");
    let mut tear_off = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the tear-off frame begins");
    tear_off
        .tear_off_root_current(CHILD_ROOT, NativeWindowPlacement::new(requested_placement))
        .expect("the contained root tear-off stages");
    tear_off
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the source presentation remains retained");
    let mut tear_off_report = tear_off.commit().expect("the tear-off request commits");
    let create = take_only_effect(&mut tear_off_report);
    let child_binding = match create.operation() {
        NativeEffectOperation::CreateWindow {
            binding, placement, ..
        } if *placement == requested_placement => *binding,
        operation => panic!("expected CreateWindow, got {operation:?}"),
    };
    assert_eq!(item_surface(&session, CHILD_ITEM), Some(ROOT_SURFACE));

    let create_ack = match create.accepted() {
        Some(NativeEffectAcknowledgement::Presentation(acknowledgement)) => acknowledgement,
        acknowledgement => panic!("create returned {acknowledgement:?}"),
    };
    session
        .report_managed_native_snapshot(
            [
                (root_binding, root_window_facts()),
                (
                    child_binding,
                    child_window_facts(requested_placement, NativeWindowPresentationState::Hidden)
                        .with_presentation(NativeWindowPresentationState::Hidden, Some(create_ack)),
                ),
            ],
            exact_work_areas(),
        )
        .expect("the hidden child snapshot records the create acknowledgement");
    commit_native_frame(&mut session);

    let pre_show = paint_staging(
        &mut session,
        child_binding,
        NativeStagingPresentationPhase::PreShow,
    );
    session
        .report_native_staging_presentation(pre_show, SurfacePresentationResult::Presented)
        .expect("the pre-show staging output presents");
    let mut pre_show_report = commit_native_frame(&mut session);
    let show = take_only_effect(&mut pre_show_report);
    assert!(matches!(
        show.operation(),
        NativeEffectOperation::ShowWindow { binding } if *binding == child_binding
    ));
    let show_ack = match show.accepted() {
        Some(NativeEffectAcknowledgement::Presentation(acknowledgement)) => acknowledgement,
        acknowledgement => panic!("show returned {acknowledgement:?}"),
    };

    session
        .report_managed_native_snapshot(
            [
                (root_binding, root_window_facts()),
                (
                    child_binding,
                    child_window_facts(requested_placement, NativeWindowPresentationState::Hidden)
                        .with_presentation(NativeWindowPresentationState::Hidden, Some(show_ack)),
                ),
            ],
            exact_work_areas(),
        )
        .expect("the hidden child snapshot records the show acknowledgement");
    commit_native_frame(&mut session);
    session
        .report_managed_native_snapshot(
            [
                (root_binding, root_window_facts()),
                (
                    child_binding,
                    child_window_facts(requested_placement, NativeWindowPresentationState::Visible),
                ),
            ],
            exact_work_areas(),
        )
        .expect("the exact visible child snapshot records");
    commit_native_frame(&mut session);

    let post_show = paint_staging(
        &mut session,
        child_binding,
        NativeStagingPresentationPhase::PostShow,
    );
    session
        .report_native_staging_presentation(post_show, SurfacePresentationResult::Presented)
        .expect("the post-show staging output presents");
    commit_native_frame(&mut session);

    measure_all_surfaces(&mut session);
    paint_and_present_surface(&mut session, child_binding.surface());
    let admission = commit_native_frame(&mut session);
    assert_eq!(admission.native_admissions(), &[child_binding]);
    assert_eq!(
        item_surface(&session, CHILD_ITEM),
        Some(child_binding.surface())
    );
    measure_all_surfaces(&mut session);
    paint_and_present_surface(&mut session, ROOT_SURFACE);
    commit_native_frame(&mut session);

    let mut redock = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the redock frame begins");
    redock
        .dock_root_current(
            CHILD_ROOT,
            DockPlacement::Center(DockAnchor::Item(MAIN_ITEM)),
        )
        .expect("the complete child root redocks");
    redock
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the redock frame retains current presentations");
    let mut redock_report = redock.commit().expect("the redock commits");
    assert!(
        matches!(
            redock_report.inputs(),
            [dockspace::runtime::HostInputOutcome::ProductActionApplied(
                dockspace::model::DockspaceActionOutcome::RootDockRequested { .. }
            )]
        ),
        "actual redock inputs: {:?}",
        redock_report.inputs()
    );
    assert!(redock_report.take_native_effects().is_empty());
    assert_eq!(
        item_surface(&session, CHILD_ITEM),
        Some(child_binding.surface()),
        "the child remains authoritative before the target output presents",
    );

    let mut settled = paint_and_present_all_surfaces(&mut session);
    assert!(
        matches!(
            settled.presentation_transitions(),
            [transition]
                if transition.root() == CHILD_ROOT
                    && transition.source_surface() == child_binding.surface()
                    && transition.target_surface() == ROOT_SURFACE
                    && transition.result()
                        == dockspace::runtime::DockspacePresentationTransitionResult::Applied
        ),
        "actual presentation transitions: {:?}",
        settled.presentation_transitions()
    );
    let release = take_only_effect(&mut settled);
    assert!(matches!(
        release.operation(),
        NativeEffectOperation::ReleaseChild { binding } if *binding == child_binding
    ));
    let close_ack = match release.accepted() {
        Some(NativeEffectAcknowledgement::Close(acknowledgement)) => acknowledgement,
        acknowledgement => panic!("release returned {acknowledgement:?}"),
    };
    assert_eq!(item_surface(&session, CHILD_ITEM), Some(ROOT_SURFACE));

    session
        .report_managed_native_snapshot(
            [
                (root_binding, root_window_facts()),
                (child_binding, NativeWindowFacts::destroyed_after(close_ack)),
            ],
            exact_work_areas(),
        )
        .expect("the exact destroyed child snapshot records");
    let destroyed = commit_native_frame(&mut session);
    assert_eq!(destroyed.native_bindings(), &[root_binding]);
    assert!(!session.is_current_native_binding(child_binding));
    assert!(session.recognizes_native_binding(child_binding));

    session
        .report_native_binding_quiescence(child_binding)
        .expect("the retired child binding becomes permanently quiescent");
    assert!(!session.recognizes_native_binding(child_binding));
    let mut quiescent = commit_native_frame(&mut session);
    assert_eq!(quiescent.native_bindings(), &[root_binding]);
    assert!(quiescent.take_native_effects().is_empty());
}

fn session() -> DockspaceSession {
    let main = DockspaceRootLayout::new(MAIN_ROOT, DockspaceNode::central_tabs([MAIN_ITEM]));
    let contained = DockspaceContainedLayout::new(
        CONTAINED,
        DockspaceRootLayout::new(CHILD_ROOT, DockspaceNode::tabs([CHILD_ITEM])),
        LogicalRect::new(40.0, 50.0, 360.0, 280.0).expect("contained bounds validate"),
    );
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(ROOT_SURFACE, main).with_contained(contained)
    ])
    .expect("the public native lifecycle layout validates");
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    DockspaceSession::from_layout(layout, policy).expect("the public native session initializes")
}

fn root_window_facts() -> NativeWindowFacts {
    let content =
        PhysicalRect::new(100.0, 80.0, 640.0, 480.0).expect("the root content bounds validate");
    let outer =
        PhysicalRect::new(92.0, 50.0, 656.0, 518.0).expect("the root outer bounds validate");
    NativeWindowFacts::live()
        .with_content_bounds(content)
        .with_outer_bounds(outer)
        .with_native_scale_factor(scale())
        .with_presentation_scale_factor(scale())
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible, None)
}

fn child_window_facts(
    bounds: PhysicalRect,
    presentation: NativeWindowPresentationState,
) -> NativeWindowFacts {
    NativeWindowFacts::live()
        .with_content_bounds(bounds)
        .with_outer_bounds(bounds)
        .with_native_scale_factor(scale())
        .with_presentation_scale_factor(scale())
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(presentation, None)
}

fn exact_work_areas() -> NativeWorkAreaRoster {
    NativeWorkAreaRoster::Exact(vec![NativeWorkAreaFacts::new(
        WORK_AREA,
        PhysicalRect::new(0.0, 0.0, 1_920.0, 1_080.0).expect("work area validates"),
        scale(),
    )])
}

fn scale() -> ScaleFactor {
    ScaleFactor::new(1.0).expect("the test scale validates")
}

fn metrics() -> UniformSurfaceMetrics {
    UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("surface bounds validate"),
        LogicalSize::new(32.0, 24.0).expect("pane minimum validates"),
        80.0,
    )
    .expect("uniform surface metrics validate")
}

fn commit_native_frame(session: &mut DockspaceSession) -> HostFrameReport {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the native frame begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the native frame settles every surface");
    frame.commit().expect("the native frame commits")
}

fn measure_all_surfaces(session: &mut DockspaceSession) {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the measurement frame begins");
    for surface in frame.surfaces() {
        frame
            .measure_surface(surface, metrics())
            .expect("the surface measurement stages");
    }
    frame.commit().expect("the measurements commit");
}

fn paint_and_present_surface(session: &mut DockspaceSession, surface: SurfaceId) {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the paint frame begins");
    assert!(
        frame
            .paint_plan(surface)
            .expect("the paint plan resolves")
            .is_some(),
        "the requested surface must have a ready paint plan"
    );
    frame
        .confirm_surface_painted(surface)
        .expect("the exact surface output is painted");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("other surfaces retain their current presentation");
    let mut report = frame.commit().expect("the painted surface commits");
    let [output] = report
        .take_painted_outputs()
        .try_into()
        .unwrap_or_else(|outputs: Vec<_>| panic!("expected one output, got {outputs:?}"));
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .expect("the exact surface output presents");
}

fn paint_and_present_all_surfaces(session: &mut DockspaceSession) -> HostFrameReport {
    measure_all_surfaces(session);
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the complete paint frame begins");
    let surfaces = frame.surfaces();
    for surface in &surfaces {
        assert!(
            frame
                .paint_plan(*surface)
                .expect("the complete paint plan resolves")
                .is_some(),
            "surface {surface:?} must have a ready paint plan",
        );
        frame
            .confirm_surface_painted(*surface)
            .expect("the complete surface output is painted");
    }
    let mut report = frame.commit().expect("the complete paint frame commits");
    let outputs = report.take_painted_outputs();
    assert_eq!(outputs.len(), surfaces.len());
    for output in outputs {
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .expect("the exact complete output presents");
    }
    commit_native_frame(session)
}

fn paint_staging(
    session: &mut DockspaceSession,
    binding: NativeSurfaceBinding,
    phase: NativeStagingPresentationPhase,
) -> dockspace::runtime::PaintedNativeStagingOutput {
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
        .expect("the exact staging output is painted");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("semantic surfaces retain their current presentation");
    let mut report = frame.commit().expect("the staging paint commits");
    let [output] = report
        .take_painted_native_staging_outputs()
        .try_into()
        .unwrap_or_else(|outputs: Vec<_>| panic!("expected one staging output, got {outputs:?}"));
    output
}

fn take_only_effect(report: &mut HostFrameReport) -> dockspace::runtime::NativeEffectRequest {
    let [effect] = report
        .take_native_effects()
        .try_into()
        .unwrap_or_else(|effects: Vec<_>| panic!("expected one native effect, got {effects:?}"));
    effect
}

fn only_binding(report: &HostFrameReport) -> NativeSurfaceBinding {
    let [binding] = report.native_bindings() else {
        panic!(
            "expected one native binding, got {:?}",
            report.native_bindings()
        );
    };
    *binding
}

fn item_surface(session: &DockspaceSession, item: ItemId) -> Option<SurfaceId> {
    session.view().item(item).map(|item| item.surface())
}

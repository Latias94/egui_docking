//! Shared host queues must distinguish presentation requests from independent sessions.

use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalRect, ScaleFactor};
use dockspace::model::{
    DockAnchor, DockPlacement, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspacePresentationTransitionId, DockspacePresentationTransitionResult,
    DockspaceSession, HostFrameReport, HostInputOutcome, HostWindowToken, NativeHostCapabilities,
    NativeHostCapability, NativePointerRoster, NativeReceiverAnswer, NativeWindowFacts,
    NativeWindowInputState, NativeWindowPresentationState, NativeWorkAreaRoster,
    SurfacePresentationResult, SurfaceUnavailableReason, UniformSurfaceMetrics,
};

const HOST: SurfaceId = SurfaceId::new(1);
const CHILD: SurfaceId = SurfaceId::new(2);
const HOST_ROOT: RootId = RootId::new(1);
const CHILD_ROOT: RootId = RootId::new(2);
const HOST_ITEM: ItemId = ItemId::new(1);
const CHILD_ITEM: ItemId = ItemId::new(2);

fn commit(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn publish(session: &mut DockspaceSession) -> HostFrameReport {
    let metrics = UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
        LogicalSize::new(32.0, 24.0).unwrap(),
        80.0,
    )
    .unwrap();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    for surface in frame.surfaces() {
        frame.measure_surface(surface, metrics).unwrap();
    }
    for surface in frame.surfaces() {
        let receipt = frame
            .paint_plan(surface)
            .unwrap()
            .unwrap()
            .execute_surface_program(|_| Ok::<(), ()>(()))
            .unwrap();
        frame.confirm_surface_painted(receipt).unwrap();
    }
    for output in commit(frame).take_painted_outputs() {
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
    }
    commit(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    )
}

fn session() -> DockspaceSession {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            HOST,
            DockspaceRootLayout::new(HOST_ROOT, DockspaceNode::central_tabs([HOST_ITEM])),
        ),
        DockspaceSurfaceLayout::new(
            CHILD,
            DockspaceRootLayout::new(CHILD_ROOT, DockspaceNode::tabs([CHILD_ITEM])),
        ),
    ])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    let capabilities = [
        NativeHostCapability::NativeWindowLifecycle,
        NativeHostCapability::AuthoritativeInventory,
        NativeHostCapability::GlobalWindowPlacement,
        NativeHostCapability::PointerHitTestObservation,
        NativeHostCapability::PointerHitTestControl,
        NativeHostCapability::CloseCancellation,
    ]
    .into_iter()
    .fold(
        NativeHostCapabilities::none_supported(),
        |caps, capability| caps.with(capability),
    );
    session
        .configure_managed_native_capabilities(capabilities)
        .unwrap();
    session
        .register_native_root(HOST, HostWindowToken::new(1))
        .unwrap();
    commit(session.begin_host_frame().unwrap());
    session
        .register_owned_native_child(CHILD, HostWindowToken::new(2), HOST)
        .unwrap();
    let registered = commit(session.begin_host_frame().unwrap());
    let facts = |surface: SurfaceId| {
        let bounds = PhysicalRect::new(surface.get() as f64 * 700.0, 0.0, 640.0, 480.0).unwrap();
        let scale = ScaleFactor::new(1.0).unwrap();
        NativeWindowFacts::live()
            .with_content_bounds(bounds)
            .with_outer_bounds(bounds)
            .with_native_scale_factor(scale)
            .with_presentation_scale_factor(scale)
            .with_input(NativeWindowInputState::ReceivesInput, None)
            .with_presentation(NativeWindowPresentationState::Visible)
    };
    session
        .report_managed_native_snapshot(
            registered
                .native_bindings()
                .iter()
                .map(|binding| (*binding, facts(binding.surface()))),
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    commit(session.begin_host_frame().unwrap());
    publish(&mut session);
    session
}

fn request(session: &mut DockspaceSession) -> DockspacePresentationTransitionId {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .dock_root_current(
            CHILD_ROOT,
            DockPlacement::Center(DockAnchor::Item(HOST_ITEM)),
        )
        .unwrap();
    let report = commit(frame);
    let [HostInputOutcome::ProductPresentationActionRequested { transition, .. }] = report.inputs()
    else {
        panic!("one native root transfer must wait for presentation: {report:?}");
    };
    *transition
}

#[test]
fn independent_sessions_cannot_alias_presentation_transition_identities() {
    let mut first = session();
    let mut second = session();
    let first_id = request(&mut first);
    let second_id = request(&mut second);
    assert_ne!(
        first_id, second_id,
        "a shared host queue must not match one session's request to another session's terminal"
    );
    for (session, expected, foreign) in [
        (&mut first, first_id, second_id),
        (&mut second, second_id, first_id),
    ] {
        let report = publish(session);
        let [terminal] = report.presentation_transitions() else {
            panic!("the exact presented transfer must settle once: {report:?}");
        };
        assert_eq!(terminal.id(), expected);
        assert_ne!(terminal.id(), foreign);
        assert_eq!(
            terminal.result(),
            DockspacePresentationTransitionResult::Applied
        );
        assert_eq!(session.view().item(CHILD_ITEM).unwrap().surface(), HOST);
    }
}

//! A later visible output revokes older receiver authority while its prefix is pending.

use std::convert::Infallible;

use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalSize, ScaleFactor};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockPresentationConfig, DockspaceHostFrame, DockspaceReceiverDescriptor, DockspaceSession,
    HostFrameReport, HostPointerInput, HostPointerRoute, HostWindowToken, NativeHostCapabilities,
    NativeHostCapability, NativePointerRoster, NativeReceiverAnswer, NativeWindowFacts,
    NativeWindowInputState, NativeWindowPresentationState, NativeWorkAreaRoster,
    PaintedSurfaceOutput, SurfacePointerButton, SurfacePointerCapture, SurfacePointerEvent,
    SurfacePointerId, SurfacePointerReceiverFacts, SurfacePresentationResult,
    SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use dockspace_host_harness::RecordingRenderer;

const SURFACE: SurfaceId = SurfaceId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);

fn metrics(tab_width: f64) -> UniformSurfaceMetrics {
    UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 1000.0, 600.0).unwrap(),
        LogicalSize::new(32.0, 24.0).unwrap(),
        tab_width,
    )
    .unwrap()
}

fn finish(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn emit(
    session: &mut DockspaceSession,
    tab_width: f64,
) -> (
    DockspaceReceiverDescriptor,
    DockspaceReceiverDescriptor,
    PaintedSurfaceOutput,
) {
    let mut measure = session.begin_host_frame().unwrap();
    measure
        .measure_surface(SURFACE, metrics(tab_width))
        .unwrap();
    measure.commit().unwrap();

    let mut paint = session.begin_host_frame().unwrap();
    let plan = paint.paint_plan(SURFACE).unwrap().unwrap();
    let first = plan.tab_receiver(FIRST).unwrap();
    let second = plan.tab_receiver(SECOND).unwrap();
    let (_, receipt) = RecordingRenderer::execute(plan).unwrap();
    paint.confirm_surface_painted(receipt).unwrap();
    let output = paint
        .commit()
        .unwrap()
        .take_painted_outputs()
        .pop()
        .unwrap();
    (first, second, output)
}

fn session() -> DockspaceSession {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([FIRST, SECOND])),
    )])
    .unwrap();
    DockspaceSession::from_layout_with_presentation_config(
        layout,
        DockPolicy::default(),
        DockPresentationConfig::builder()
            .tab_max_width(400.0)
            .build()
            .unwrap(),
    )
    .unwrap()
}

fn native_session_with_headless_authority() -> (
    DockspaceSession,
    dockspace::runtime::NativeSurfaceBinding,
    PaintedSurfaceOutput,
) {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([FIRST, SECOND])),
    )])
    .unwrap();
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut session = DockspaceSession::from_layout(layout, policy).unwrap();

    let mut measure = session.begin_host_frame().unwrap();
    measure.measure_surface(SURFACE, metrics(400.0)).unwrap();
    measure.commit().unwrap();
    let mut paint = session.begin_host_frame().unwrap();
    let receipt = paint
        .paint_plan(SURFACE)
        .unwrap()
        .unwrap()
        .execute_surface_program(|_| Ok::<_, Infallible>(()))
        .unwrap();
    paint.confirm_surface_painted(receipt).unwrap();
    let mut report = paint.commit().unwrap();
    let [headless_output] = report.take_painted_outputs().try_into().unwrap();
    session
        .report_surface_presentation(headless_output, SurfacePresentationResult::Presented)
        .unwrap();
    finish(session.begin_host_frame().unwrap());
    assert!(session.presented_surface(SURFACE).is_some());
    let (_, _, pending_headless) = emit(&mut session, 400.0);

    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    session
        .configure_managed_native_capabilities(
            NativeHostCapabilities::none_supported()
                .with(NativeHostCapability::AuthoritativeInventory),
        )
        .unwrap();
    session
        .register_native_root(SURFACE, HostWindowToken::new(1))
        .unwrap();
    let registration = finish(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    let [binding] = registration.native_bindings() else {
        panic!("native registration must publish one endpoint successor");
    };
    let binding = *binding;
    let facts = NativeWindowFacts::live()
        .with_content_size(PhysicalSize::new(1000.0, 600.0).unwrap())
        .with_native_scale_factor(ScaleFactor::new(1.0).unwrap())
        .with_presentation_scale_factor(ScaleFactor::new(1.0).unwrap())
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible);
    session
        .report_managed_native_snapshot([(binding, facts)], NativeWorkAreaRoster::Unknown)
        .unwrap();
    let mut measure = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    measure.measure_surface(SURFACE, metrics(400.0)).unwrap();
    finish(measure);
    assert!(session.presented_surface(SURFACE).is_none());
    (session, binding, pending_headless)
}

fn paint_native_output(session: &mut DockspaceSession) -> PaintedSurfaceOutput {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let receipt = frame
        .paint_plan(SURFACE)
        .unwrap()
        .unwrap()
        .execute_surface_program(|_| Ok::<_, Infallible>(()))
        .unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    frame
        .commit()
        .unwrap()
        .take_painted_outputs()
        .pop()
        .unwrap()
}

#[test]
fn later_presented_output_revokes_old_receiver_until_its_prefix_is_committed() {
    let mut session = session();
    session.enable_host_pointer().unwrap();

    let (_, old_second_descriptor, first_output) = emit(&mut session, 64.0);
    session
        .report_surface_presentation(first_output, SurfacePresentationResult::Presented)
        .unwrap();
    finish(session.begin_host_frame().unwrap());

    let old_surface = session.presented_surface(SURFACE).unwrap();
    let old_second = session
        .bind_presented_receiver(&old_second_descriptor)
        .unwrap();

    let (_, _, pending_b) = emit(&mut session, 128.0);
    let (_, _, pending_c) = emit(&mut session, 200.0);

    let mut selection = session.begin_host_frame().unwrap();
    selection.select_item_current(FIRST).unwrap();
    selection.measure_surface(SURFACE, metrics(500.0)).unwrap();
    selection.commit().unwrap();

    let (new_first, _, output_d) = emit(&mut session, 500.0);
    let output_d_artifact = output_d.presentation_artifact();
    assert!(new_first.bounds().contains(old_second.center()));
    session
        .report_surface_presentation(output_d, SurfacePresentationResult::Presented)
        .unwrap();
    assert!(session.presented_surface(SURFACE).is_none());
    assert!(
        session
            .bind_presented_receiver(&old_second_descriptor)
            .is_none()
    );
    assert!(
        session
            .host_pointer_hit(SURFACE, old_second.center())
            .is_none()
    );

    let mut input = session.begin_host_frame().unwrap();
    assert!(input.presented_surface(SURFACE).is_none());
    assert!(
        input
            .bind_presented_receiver(&old_second_descriptor)
            .is_none()
    );

    let route = HostPointerRoute::new(old_surface, old_second.center());
    input
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
                    SurfacePointerReceiverFacts::delivery(&old_second),
                )
            }),
        )
        .unwrap();
    finish(input);
    assert!(session.view().item(FIRST).unwrap().is_selected());

    drop((pending_b, pending_c));
    let settled = session.begin_host_frame().unwrap();
    assert!(settled.presented_surface(SURFACE).is_some());
    assert!(settled.bind_presented_receiver(&new_first).is_some());
    let settled = finish(settled);
    assert!(
        settled
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.presents(output_d_artifact))
    );
    assert!(session.presented_surface(SURFACE).is_some());
    assert!(session.bind_presented_receiver(&new_first).is_some());
    assert!(
        session
            .bind_presented_receiver(&old_second_descriptor)
            .is_none()
    );
}

#[test]
fn presented_successor_stream_revokes_authority_from_the_previous_endpoint() {
    let (mut session, binding, pending_headless) = native_session_with_headless_authority();
    let pending_b = paint_native_output(&mut session);
    let pending_c = paint_native_output(&mut session);
    let output_d = paint_native_output(&mut session);
    let artifact_d = output_d.presentation_artifact();
    assert!(output_d.matches_native_binding(binding));

    session
        .report_surface_presentation(output_d, SurfacePresentationResult::Presented)
        .unwrap();
    assert!(session.presented_surface(SURFACE).is_none());

    drop((pending_b, pending_c));
    let settled = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let successor_authority = settled.presented_surface(SURFACE).unwrap();
    assert!(successor_authority.matches_presentation_artifact(artifact_d));
    let report = finish(settled);
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.presents(artifact_d))
    );
    assert!(
        session
            .presented_surface(SURFACE)
            .is_some_and(|authority| authority.matches_presentation_artifact(artifact_d))
    );
    session
        .report_surface_presentation(pending_headless, SurfacePresentationResult::Presented)
        .unwrap();
    assert!(
        session
            .presented_surface(SURFACE)
            .is_some_and(|authority| authority.matches_presentation_artifact(artifact_d))
    );
    finish(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    assert!(
        session
            .presented_surface(SURFACE)
            .is_some_and(|authority| authority.matches_presentation_artifact(artifact_d))
    );
}

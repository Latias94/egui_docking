use std::convert::Infallible;
use std::num::NonZeroUsize;

use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalSize, ScaleFactor};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceSession, HostFrameReport, HostWindowToken, NativeHostCapabilities,
    NativeHostCapability, NativePointerRoster, NativeReceiverAnswer, NativeSurfaceBinding,
    NativeWindowFacts, NativeWindowInputState, NativeWindowPresentationState, NativeWorkAreaRoster,
    PaintedSurfaceOutput, SurfacePresentationResult, SurfaceUnavailableReason, SurfaceWork,
    SurfaceWorkDeferred, UniformSurfaceMetrics,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const ITEM: ItemId = ItemId::new(1);

fn metrics() -> UniformSurfaceMetrics {
    UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
        LogicalSize::new(32.0, 24.0).unwrap(),
        80.0,
    )
    .unwrap()
}

fn finish(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn measure_native_candidate(session: &mut DockspaceSession) {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.measure_surface(SURFACE, metrics()).unwrap();
    finish(frame);
}

struct Fixture {
    session: DockspaceSession,
    binding: NativeSurfaceBinding,
}

fn fixture(capacity: usize) -> Fixture {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([ITEM])),
    )])
    .unwrap();
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut session = DockspaceSession::from_layout(layout, policy).unwrap();
    session
        .configure_output_capacity(NonZeroUsize::new(capacity).unwrap())
        .unwrap();

    let mut frame = session.begin_host_frame().unwrap();
    frame.measure_surface(SURFACE, metrics()).unwrap();
    frame.commit().unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let receipt = plan
        .execute_surface_program(|_| Ok::<_, Infallible>(()))
        .unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    let mut report = frame.commit().unwrap();
    let [output] = report.take_painted_outputs().try_into().unwrap();
    let previous_artifact = output.presentation_artifact();
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    finish(session.begin_host_frame().unwrap());
    let previous_authority = session.presented_surface(SURFACE).unwrap();
    assert!(previous_authority.matches_presentation_artifact(previous_artifact));

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
        panic!("native root registration must publish one exact binding");
    };
    let binding = *binding;
    let facts = NativeWindowFacts::live()
        .with_content_size(PhysicalSize::new(800.0, 600.0).unwrap())
        .with_native_scale_factor(ScaleFactor::new(1.0).unwrap())
        .with_presentation_scale_factor(ScaleFactor::new(1.0).unwrap())
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible);
    session
        .report_managed_native_snapshot([(binding, facts)], NativeWorkAreaRoster::Unknown)
        .unwrap();
    measure_native_candidate(&mut session);

    Fixture { session, binding }
}

fn paint_native_successor(
    session: &mut DockspaceSession,
    binding: NativeSurfaceBinding,
) -> (PaintedSurfaceOutput, usize) {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let mut callbacks = 0;
    let plan = frame
        .paint_plan(SURFACE)
        .unwrap()
        .expect("the admitted native successor must have a paint plan");
    let receipt = plan
        .execute_surface_program(|_| {
            callbacks += 1;
            Ok::<_, Infallible>(())
        })
        .unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    let mut report = frame.commit().unwrap();
    let [output] = report.take_painted_outputs().try_into().unwrap();
    assert!(output.matches_native_binding(binding));
    (output, callbacks)
}

fn present_native_successor(session: &mut DockspaceSession, output: PaintedSurfaceOutput) {
    let artifact = output.presentation_artifact();
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    let report = finish(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.presents(artifact))
    );
    assert!(
        session
            .presented_surface(SURFACE)
            .is_some_and(|authority| authority.matches_presentation_artifact(artifact))
    );
}

#[test]
fn one_slot_handoff_needs_no_reconfiguration_and_retains_output_backpressure() {
    let Fixture {
        mut session,
        binding,
    } = fixture(1);

    let (output, callbacks) = paint_native_successor(&mut session, binding);
    assert!(callbacks > 0);
    assert!(session.presented_surface(SURFACE).is_none());
    // Reapplying the ordinary bound must accept its authorized handoff overlap.
    session
        .configure_output_capacity(NonZeroUsize::MIN)
        .unwrap();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let mut callbacks = 0;
    let receipt = match frame.surface_work(SURFACE).unwrap() {
        SurfaceWork::Deferred(SurfaceWorkDeferred::OutputCapacity) => None,
        SurfaceWork::Deferred(reason) => panic!("unexpected deferral: {reason:?}"),
        SurfaceWork::Ready(plan) => Some(
            plan.execute_surface_program(|_| {
                callbacks += 1;
                Ok::<_, Infallible>(())
            })
            .unwrap(),
        ),
    };
    assert!(receipt.is_none());
    assert_eq!(callbacks, 0);
    let mut report = finish(frame);
    assert!(report.take_painted_outputs().is_empty());
    assert!(report.surface_presentation_commits().is_empty());
    assert!(report.surface_commits().is_empty());
    assert!(!report.workspace_changed());
    assert!(!report.published_state_changed());
    assert!(session.presented_surface(SURFACE).is_none());

    present_native_successor(&mut session, output);
    let (redraw, callbacks) = paint_native_successor(&mut session, binding);
    assert!(callbacks > 0);
    present_native_successor(&mut session, redraw);
}

#[test]
fn two_slots_admit_and_present_the_native_successor() {
    let Fixture {
        mut session,
        binding,
        ..
    } = fixture(2);
    let (output, callbacks) = paint_native_successor(&mut session, binding);
    assert!(callbacks > 0);
    assert!(session.presented_surface(SURFACE).is_none());
    present_native_successor(&mut session, output);
}

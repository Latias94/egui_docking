//! A single prepaid stream slot supports exact endpoint handoff without widening output quotas.

use std::num::NonZeroUsize;

use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalSize, ScaleFactor};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceSession, HostFrameReport, HostWindowToken, NativeHostCapabilities,
    NativeHostCapability, NativeInputStorageLimit, NativePointerRoster, NativeReportStorageLimit,
    NativeSurfaceBinding, NativeWindowFacts, NativeWindowInputState, NativeWindowPresentationState,
    NativeWorkAreaRoster, PaintedSurfaceOutput, SurfacePresentationResult,
    SurfaceUnavailableReason, SurfaceWork, SurfaceWorkDeferred, UniformSurfaceMetrics,
};

const A: SurfaceId = SurfaceId::new(1);
const B: SurfaceId = SurfaceId::new(2);
const C: SurfaceId = SurfaceId::new(3);
const REPORT_BYTES: usize = 1 << 20;

fn metrics() -> UniformSurfaceMetrics {
    UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
        LogicalSize::new(32.0, 24.0).unwrap(),
        80.0,
    )
    .unwrap()
}

fn commit(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn encode(frame: &mut DockspaceHostFrame<'_>, surface: SurfaceId) {
    let plan = match frame.surface_work(surface).unwrap() {
        SurfaceWork::Ready(plan) => plan,
        SurfaceWork::Deferred(reason) => panic!("{surface:?} unexpectedly deferred: {reason:?}"),
    };
    let receipt = plan.execute_surface_program(|_| Ok::<(), ()>(())).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
}

fn assert_capacity_deferred(frame: &mut DockspaceHostFrame<'_>, surface: SurfaceId) {
    assert!(
        matches!(
            frame.surface_work(surface).unwrap(),
            SurfaceWork::Deferred(SurfaceWorkDeferred::OutputCapacity)
        ),
        "{surface:?} must not exceed its output or retained-stream allowance"
    );
}

fn only_output(mut report: HostFrameReport) -> PaintedSurfaceOutput {
    let [output] = report.take_painted_outputs().try_into().unwrap();
    output
}

fn session_with_drained_headless_predecessors() -> (DockspaceSession, [NativeSurfaceBinding; 2]) {
    let layout = DockspaceLayout::new([(A, 1), (B, 2), (C, 3)].map(|(surface, identity)| {
        DockspaceSurfaceLayout::new(
            surface,
            DockspaceRootLayout::new(
                RootId::new(identity),
                DockspaceNode::central_tabs([ItemId::new(identity)]),
            ),
        )
    }))
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session
        .enable_managed_native_host_with_input_storage_limit(
            NativePointerRoster::Exact(Vec::new()),
            NativeInputStorageLimit::new(NonZeroUsize::new(4).unwrap(), 1 << 20)
                .with_report_storage_limit(NativeReportStorageLimit::new(
                    NonZeroUsize::new(2).unwrap(),
                    NonZeroUsize::MIN,
                    REPORT_BYTES,
                )),
        )
        .unwrap();
    session
        .configure_managed_native_capabilities(
            NativeHostCapabilities::none_supported()
                .with(NativeHostCapability::AuthoritativeInventory),
        )
        .unwrap();

    let mut frame = session.begin_native_host_frame(|_| unreachable!()).unwrap();
    for surface in [A, B, C] {
        frame.measure_surface(surface, metrics()).unwrap();
    }
    for surface in [A, B] {
        encode(&mut frame, surface);
    }
    let outputs = commit(frame).take_painted_outputs();
    assert_eq!(outputs.len(), 2);
    let artifacts: Vec<_> = outputs
        .iter()
        .map(PaintedSurfaceOutput::presentation_artifact)
        .collect();
    for output in outputs {
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
    }
    let settled = commit(session.begin_native_host_frame(|_| unreachable!()).unwrap());
    assert_eq!(settled.surface_presentation_commits().len(), 2);
    for artifact in artifacts {
        assert!(
            settled
                .surface_presentation_commits()
                .iter()
                .any(|entry| entry.presents(artifact))
        );
    }

    for (surface, token) in [(A, 1), (B, 2)] {
        session
            .register_native_root(surface, HostWindowToken::new(token))
            .unwrap();
    }
    let registration = commit(session.begin_native_host_frame(|_| unreachable!()).unwrap());
    assert_eq!(registration.native_bindings().len(), 2);
    let bindings = [A, B].map(|surface| {
        *registration
            .native_bindings()
            .iter()
            .find(|binding| binding.surface() == surface)
            .unwrap()
    });
    session
        .report_managed_native_snapshot(
            bindings.map(|binding| {
                (
                    binding,
                    NativeWindowFacts::live()
                        .with_content_size(PhysicalSize::new(640.0, 480.0).unwrap())
                        .with_native_scale_factor(ScaleFactor::new(1.0).unwrap())
                        .with_presentation_scale_factor(ScaleFactor::new(1.0).unwrap())
                        .with_input(NativeWindowInputState::ReceivesInput, None)
                        .with_presentation(NativeWindowPresentationState::Visible),
                )
            }),
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    commit(session.begin_native_host_frame(|_| unreachable!()).unwrap());
    (session, bindings)
}

#[test]
fn endpoint_handoffs_share_one_global_stream_slot_and_keep_the_output_limit() {
    for (first, second) in [(A, B), (B, A)] {
        let (mut session, bindings) = session_with_drained_headless_predecessors();
        let first_binding = *bindings
            .iter()
            .find(|binding| binding.surface() == first)
            .unwrap();
        let second_binding = *bindings
            .iter()
            .find(|binding| binding.surface() == second)
            .unwrap();

        let mut frame = session.begin_native_host_frame(|_| unreachable!()).unwrap();
        for surface in [A, B, C] {
            frame.measure_surface(surface, metrics()).unwrap();
        }
        // The extra stream slot cannot admit a surface without a predecessor.
        assert_capacity_deferred(&mut frame, C);
        // Repeated queries reuse admission rather than charging another stream.
        for _ in 0..2 {
            assert!(matches!(
                frame.surface_work(first).unwrap(),
                SurfaceWork::Ready(_)
            ));
        }
        assert_capacity_deferred(&mut frame, second);
        assert_capacity_deferred(&mut frame, C);
        encode(&mut frame, first);
        assert_capacity_deferred(&mut frame, second);
        let initial = only_output(commit(frame));
        assert!(initial.matches_native_binding(first_binding));
        let initial_artifact = initial.presentation_artifact();

        // Observe the successor before relying on its predecessor being reclaimable.
        session
            .report_surface_presentation(initial, SurfacePresentationResult::Presented)
            .unwrap();
        let settled = commit(session.begin_native_host_frame(|_| unreachable!()).unwrap());
        assert_eq!(settled.surface_presentation_commits().len(), 1);
        assert!(settled.surface_presentation_commits()[0].presents(initial_artifact));
        assert!(
            session
                .presented_surface(first)
                .unwrap()
                .matches_presentation_artifact(initial_artifact)
        );

        let mut frame = session.begin_native_host_frame(|_| unreachable!()).unwrap();
        for surface in [A, B, C] {
            frame.measure_surface(surface, metrics()).unwrap();
        }
        assert_capacity_deferred(&mut frame, C);
        encode(&mut frame, second);
        // An existing stream may paint alongside the newly admitted handoff.
        encode(&mut frame, first);
        let [one, two] = commit(frame).take_painted_outputs().try_into().unwrap();
        let (redraw, successor) = if one.surface() == first {
            (one, two)
        } else {
            (two, one)
        };
        assert!(redraw.matches_native_binding(first_binding));
        assert!(successor.matches_native_binding(second_binding));
        let redraw_artifact = redraw.presentation_artifact();
        let successor_artifact = successor.presentation_artifact();
        assert_ne!(initial_artifact, redraw_artifact);

        let mut full = session.begin_native_host_frame(|_| unreachable!()).unwrap();
        for surface in [A, B, C] {
            full.measure_surface(surface, metrics()).unwrap();
        }
        assert_capacity_deferred(&mut full, first);
        assert_capacity_deferred(&mut full, second);
        assert_capacity_deferred(&mut full, C);
        assert!(commit(full).take_painted_outputs().is_empty());

        session
            .report_surface_presentation(redraw, SurfacePresentationResult::Dropped)
            .unwrap();
        session
            .report_surface_presentation(successor, SurfacePresentationResult::Presented)
            .unwrap();
        let settled = commit(session.begin_native_host_frame(|_| unreachable!()).unwrap());
        assert_eq!(settled.surface_presentation_commits().len(), 2);
        assert!(
            settled
                .surface_presentation_commits()
                .iter()
                .any(|entry| entry.retires(redraw_artifact))
        );
        assert!(
            settled
                .surface_presentation_commits()
                .iter()
                .any(|entry| entry.presents(successor_artifact))
        );

        let mut frame = session.begin_native_host_frame(|_| unreachable!()).unwrap();
        frame.measure_surface(C, metrics()).unwrap();
        assert_capacity_deferred(&mut frame, C);
        assert!(commit(frame).take_painted_outputs().is_empty());
        assert_eq!(
            session.native_input_report_storage_reserved_bytes(),
            Some(REPORT_BYTES)
        );
        assert!(session.native_input_pending_report_capacity_bytes() <= REPORT_BYTES);
    }
}

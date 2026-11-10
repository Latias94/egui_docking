//! Shared native placements exercised only through the public host contract.

use dockspace::geometry::{
    LogicalRect, LogicalSize, PhysicalPoint, PhysicalRect, PhysicalSize, ScaleFactor,
};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceSession, HostFrameReport, HostWindowToken, NativeHostCapabilities,
    NativeHostCapability, NativePointerRoster, NativeReceiverAnswer, NativeSurfaceBinding,
    NativeSurfacePlacement, NativeWindowBinding, NativeWindowFacts, NativeWindowInputState,
    NativeWindowPresentationState, NativeWorkAreaRoster, SurfacePresentationResult,
    SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use dockspace_host_harness::RecordingRenderer;

const A: SurfaceId = SurfaceId::new(1);
const B: SurfaceId = SurfaceId::new(2);

fn layout(a: SurfaceId, b: SurfaceId) -> DockspaceLayout {
    DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            a,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1)]),
            ),
        ),
        DockspaceSurfaceLayout::new(
            b,
            DockspaceRootLayout::new(
                RootId::new(2),
                DockspaceNode::central_tabs([ItemId::new(2)]),
            ),
        ),
    ])
    .unwrap()
}

fn placements(a: SurfaceId, b: SurfaceId) -> [NativeSurfacePlacement; 2] {
    [
        NativeSurfacePlacement::new(
            a,
            LogicalSize::new(400.0, 500.0).unwrap(),
            PhysicalPoint::new(0.0, 0.0).unwrap(),
            ScaleFactor::new(1.0).unwrap(),
            PhysicalRect::new(0.0, 0.0, 400.0, 500.0).unwrap(),
        ),
        NativeSurfacePlacement::new(
            b,
            LogicalSize::new(300.0, 240.0).unwrap(),
            PhysicalPoint::new(460.0, 30.0).unwrap(),
            ScaleFactor::new(2.0).unwrap(),
            PhysicalRect::new(480.0, 40.0, 550.0, 450.0).unwrap(),
        ),
    ]
}

fn facts(scale: f64) -> NativeWindowFacts {
    NativeWindowFacts::live()
        .with_content_size(PhysicalSize::new(1200.0, 900.0).unwrap())
        .with_native_scale_factor(ScaleFactor::new(scale).unwrap())
        .with_presentation_scale_factor(ScaleFactor::new(1.0).unwrap())
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible)
}

fn finish(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn idle(session: &mut DockspaceSession) -> HostFrameReport {
    finish(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    )
}

fn session() -> (DockspaceSession, NativeSurfaceBinding) {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut session = DockspaceSession::from_layout(layout(A, B), policy).unwrap();
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
        .register_native_root(A, HostWindowToken::new(7))
        .unwrap();
    let report = idle(&mut session);
    let [binding] = report.native_bindings() else {
        panic!("one real Root registration");
    };
    let binding = *binding;
    session
        .report_managed_native_snapshot(
            [(binding.window(), facts(1.5))],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    idle(&mut session);
    (session, binding)
}

fn compose(
    session: &mut DockspaceSession,
    window: NativeWindowBinding,
    geometry: &[NativeSurfacePlacement],
) -> Vec<NativeSurfaceBinding> {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .set_native_surface_placements(window, geometry)
        .unwrap();
    let bindings = frame
        .native_surface_bindings_for_window(window)
        .collect::<Vec<_>>();
    for placement in geometry {
        let bounds = LogicalRect::new(
            0.0,
            0.0,
            placement.size().width(),
            placement.size().height(),
        )
        .unwrap();
        frame
            .measure_surface(
                placement.surface(),
                UniformSurfaceMetrics::new(bounds, LogicalSize::new(24.0, 20.0).unwrap(), 80.0)
                    .unwrap(),
            )
            .unwrap();
        let plan = frame
            .paint_plan(placement.surface())
            .unwrap()
            .expect("every shared placement has physical coordinates");
        let (_, receipt) = RecordingRenderer::execute(plan).unwrap();
        frame.confirm_surface_painted(receipt).unwrap();
    }
    let mut report = finish(frame);
    let outputs = report.take_painted_outputs();
    assert_eq!(outputs.len(), geometry.len());
    for output in &outputs {
        assert!(
            bindings
                .iter()
                .any(|binding| output.matches_native_binding(*binding))
        );
    }
    session
        .report_surface_presentations(outputs, SurfacePresentationResult::Presented)
        .unwrap();
    idle(session);
    bindings
}

#[test]
fn empty_shared_roster_is_refused_without_detaching_the_real_window() {
    let (mut session, original) = session();
    let window = original.window();
    let bindings = compose(&mut session, window, &placements(A, B));
    let before = session.version();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    assert!(frame.set_native_surface_placements(window, &[]).is_err());
    drop(frame);
    assert_eq!(session.version(), before);
    assert_eq!(
        session
            .native_surface_bindings_for_window(window)
            .collect::<Vec<_>>(),
        bindings
    );
    assert_eq!(
        session.native_window_bindings().collect::<Vec<_>>(),
        [window]
    );
    session
        .report_managed_native_snapshot([(window, facts(1.5))], NativeWorkAreaRoster::Unknown)
        .unwrap();
    idle(&mut session);
}

#[test]
fn an_explicit_initial_whole_client_placement_keeps_its_layout_after_window_resize() {
    let (mut session, original) = session();
    let window = original.window();
    let placement = session.native_surface_placement(original).unwrap();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .set_native_surface_placements(window, &[placement])
        .unwrap();
    finish(frame);
    let [binding] = session
        .native_surface_bindings_for_window(window)
        .collect::<Vec<_>>()[..]
    else {
        panic!("one explicit Root placement");
    };
    assert_eq!(binding, original);
    session
        .report_managed_native_snapshot(
            [(
                window,
                facts(1.5).with_content_size(PhysicalSize::new(1400.0, 1000.0).unwrap()),
            )],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    idle(&mut session);
    assert_eq!(session.native_surface_placement(binding), Some(placement));
}

#[test]
fn shared_placement_candidate_drop_preserves_the_committed_scope_and_exact_retry() {
    let (mut session, original) = session();
    let window = original.window();
    let geometry = placements(A, B);
    let proposed = {
        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        frame
            .set_native_surface_placements(window, &geometry)
            .unwrap();
        let proposed = frame
            .native_surface_bindings_for_window(window)
            .collect::<Vec<_>>();
        assert_eq!(proposed.len(), 2);
        assert_eq!(
            frame.native_surface_placement(proposed[1]),
            Some(geometry[1])
        );
        proposed
    };
    assert_eq!(
        session
            .native_surface_bindings_for_window(window)
            .collect::<Vec<_>>(),
        [original]
    );
    assert!(session.native_surface_placement(proposed[1]).is_none());
    let bindings = compose(&mut session, window, &geometry);
    assert_eq!(bindings, proposed);
    assert_eq!(
        session.native_window_bindings().collect::<Vec<_>>(),
        [window]
    );
    assert!(session.presented_surface(A).is_some() && session.presented_surface(B).is_some());
}

#[test]
fn shared_window_accepts_one_physical_fact_and_rejects_duplicate_sibling_facts() {
    let (mut session, original) = session();
    let window = original.window();
    let bindings = compose(&mut session, window, &placements(A, B));
    let recorded = session.native_input_recorded_count();
    assert!(
        session
            .report_managed_native_snapshot(
                [(bindings[0], facts(1.5)), (bindings[1], facts(1.5)),],
                NativeWorkAreaRoster::Unknown
            )
            .is_err()
    );
    assert_eq!(session.native_input_recorded_count(), recorded);
    session
        .report_managed_native_snapshot([(window, facts(2.0))], NativeWorkAreaRoster::Unknown)
        .unwrap();
    idle(&mut session);
    assert_eq!(
        session
            .native_surface_bindings_for_window(window)
            .collect::<Vec<_>>(),
        bindings
    );
    assert_eq!(
        session
            .native_local_coordinates(window)
            .unwrap()
            .geometry()
            .native_scale_factor(),
        ScaleFactor::new(2.0).unwrap()
    );
    assert!(session.presented_surface(A).is_none() && session.presented_surface(B).is_none());
    for (binding, placement) in bindings.iter().zip(placements(A, B)) {
        assert_eq!(session.native_surface_placement(*binding), Some(placement));
    }
}

#[test]
fn moving_b_revokes_b_output_while_a_keeps_its_exact_placement_and_presentation() {
    let (mut session, original) = session();
    let window = original.window();
    let mut geometry = placements(A, B);
    let before = compose(&mut session, window, &geometry);
    let a = session.presented_surface(A).unwrap();
    geometry[1] = NativeSurfacePlacement::new(
        B,
        geometry[1].size(),
        PhysicalPoint::new(480.0, 50.0).unwrap(),
        geometry[1].scale_factor(),
        geometry[1].clip(),
    );
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .set_native_surface_placements(window, &geometry)
        .unwrap();
    finish(frame);
    let after = session
        .native_surface_bindings_for_window(window)
        .collect::<Vec<_>>();
    assert_eq!(after[0], before[0]);
    assert_ne!(after[1], before[1]);
    assert_eq!(session.presented_surface(A), Some(a));
    assert!(session.presented_surface(B).is_none());
    assert!(session.native_surface_placement(before[1]).is_none());
}

#[test]
fn shared_root_continuation_publishes_one_window_and_the_complete_new_placement_scope() {
    let (mut session, original) = session();
    let window = original.window();
    let before = compose(&mut session, window, &placements(A, B));
    let a = SurfaceId::new(11);
    let b = SurfaceId::new(12);
    let reset = session.prepare_layout_reset(layout(a, b)).unwrap();
    let mut expected = None;
    for commit in [false, true] {
        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        frame
            .submit_native_layout_reset_with_roots(&reset, [(before[0], a), (before[1], b)])
            .unwrap();
        let pairs = frame
            .proposed_native_root_continuations()
            .collect::<Vec<_>>();
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].successor().window(), pairs[1].successor().window());
        if commit {
            assert_eq!(expected, Some(pairs.clone()));
            let report = finish(frame);
            assert_eq!(report.native_root_continuations(), pairs);
            let successor = pairs[0].successor().window();
            assert_eq!(
                session.native_window_bindings().collect::<Vec<_>>(),
                [successor]
            );
            assert_ne!(successor, window);
            assert_eq!(
                session
                    .native_surface_bindings_for_window(successor)
                    .count(),
                2
            );
            assert!(session.native_local_coordinates(successor).is_none());
            assert!(
                session.presented_surface(a).is_none() && session.presented_surface(b).is_none()
            );
            assert!(
                session
                    .report_managed_native_snapshot(
                        [(window, facts(1.5))],
                        NativeWorkAreaRoster::Unknown
                    )
                    .is_err()
            );
            session
                .report_managed_native_snapshot(
                    [(successor, facts(1.5))],
                    NativeWorkAreaRoster::Unknown,
                )
                .unwrap();
            idle(&mut session);
        } else {
            expected = Some(pairs);
            drop(frame);
            assert_eq!(
                session
                    .native_surface_bindings_for_window(window)
                    .collect::<Vec<_>>(),
                before
            );
        }
    }
}

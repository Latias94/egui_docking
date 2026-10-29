//! Tokenless boundary model; staging acknowledgements do not confirm first-live paint.

use dockspace::model::{DockspaceContainedLayout, FloatingPresentationId, NativeWindowPlacement};
use dockspace::runtime::NativeStagingPresentationPhase;

use super::*;

fn commit_model_frame(native: &mut NativeCoordinator) -> HostFrameReport {
    let mut frame = native
        .session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert!(report.native_admissions().is_empty());
    report
}

#[inline(never)]
fn present_one_host(native: &mut NativeCoordinator, surface: SurfaceId) {
    let mut measured = native
        .session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    measured
        .measure_surface(
            surface,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
                LogicalSize::new(32.0, 24.0).unwrap(),
                80.0,
            )
            .unwrap(),
        )
        .unwrap();
    measured
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    measured.commit().unwrap();
    let mut painted = native
        .session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let receipt = painted
        .paint_plan(surface)
        .unwrap()
        .unwrap()
        .execute_surface_program(|_| Ok::<(), ()>(()))
        .unwrap();
    painted.confirm_surface_painted(receipt).unwrap();
    painted
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = painted.commit().unwrap();
    let [output] = report.take_painted_outputs().try_into().unwrap();
    native
        .session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    commit_model_frame(native);
}

fn transfer_child(
    native: &mut NativeCoordinator,
    root: RootId,
    x: f64,
    observations: &[(NativeSurfaceBinding, NativeWindowFacts)],
) -> NativeSurfaceBinding {
    let mut frame = native
        .session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .move_root_to_new_window_current(
            root,
            NativeWindowPlacement::new(PhysicalRect::new(x, 0.0, 640.0, 480.0).unwrap()),
        )
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    let [create] = report.take_native_effects().try_into().unwrap();
    let child = match create.operation() {
        NativeEffectOperation::CreateWindow { binding, .. } => *binding,
        operation => panic!("expected the model create, got {operation:?}"),
    };
    let Some(NativeEffectAcknowledgement::Presentation(mut acknowledgement)) = create.accepted()
    else {
        panic!("the create supplies its exact acknowledgement");
    };
    // Reuse public staging receipts and effect acknowledgements, without fork tokens.
    for (phase, presentation) in [
        (
            NativeStagingPresentationPhase::PreShow,
            NativeWindowPresentationState::Hidden,
        ),
        (
            NativeStagingPresentationPhase::PostShow,
            NativeWindowPresentationState::Visible,
        ),
    ] {
        let snapshot_acknowledgement = if phase == NativeStagingPresentationPhase::PostShow {
            native
                .session
                .report_managed_native_snapshot(
                    observations.iter().copied().chain([(
                        child,
                        ready_window_facts(x).with_presentation(
                            NativeWindowPresentationState::Hidden,
                            Some(acknowledgement),
                        ),
                    )]),
                    NativeWorkAreaRoster::Unknown,
                )
                .unwrap();
            commit_model_frame(native);
            None
        } else {
            Some(acknowledgement)
        };
        native
            .session
            .report_managed_native_snapshot(
                observations.iter().copied().chain([(
                    child,
                    ready_window_facts(x).with_presentation(presentation, snapshot_acknowledgement),
                )]),
                NativeWorkAreaRoster::Unknown,
            )
            .unwrap();
        commit_model_frame(native);
        let mut frame = native
            .session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        let request = frame
            .native_staging_paints()
            .find(|request| request.binding() == child && request.phase() == phase)
            .unwrap();
        frame.confirm_native_staging_painted(request).unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let mut report = frame.commit().unwrap();
        assert!(report.take_painted_outputs().is_empty());
        let [output] = report
            .take_painted_native_staging_outputs()
            .try_into()
            .unwrap();
        native
            .session
            .report_native_staging_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        let mut report = commit_model_frame(native);
        if phase == NativeStagingPresentationPhase::PreShow {
            let [show] = report.take_native_effects().try_into().unwrap();
            assert!(
                matches!(show.operation(), NativeEffectOperation::ShowWindow { binding } if *binding == child)
            );
            let Some(NativeEffectAcknowledgement::Presentation(show_ack)) = show.accepted() else {
                panic!("the show supplies its exact acknowledgement");
            };
            acknowledgement = show_ack;
        }
    }
    child
}

fn transferred_child() -> (
    NativeCoordinator,
    NativeSurfaceBinding,
    NativeSurfaceBinding,
) {
    let mut native = coordinator();
    let (host, source) = register_root_and_child(&mut native);
    for (binding, viewport, window) in [
        (host, ViewportId::ROOT, WindowId::from(11)),
        (source, viewport_id_for(source), WindowId::from(22)),
    ] {
        native.bind_viewport(viewport, window, binding).unwrap();
    }
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut configuration = native
        .session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    configuration.replace_policy(policy).unwrap();
    configuration
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    configuration.commit().unwrap();
    paint_and_present_all_surfaces(&mut native);

    let child = transfer_child(
        &mut native,
        RootId::new(2),
        1400.0,
        &[
            (host, ready_window_facts(0.0)),
            (source, ready_window_facts(720.0)),
        ],
    );
    // Policy invalidation makes the host stale without cancelling the durable obligation.
    let mut configuration = native
        .session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    configuration.replace_policy(DockPolicy::default()).unwrap();
    configuration
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    configuration.commit().unwrap();
    commit_model_frame(&mut native);
    native
        .bind_viewport(viewport_id_for(child), WindowId::from(33), child)
        .unwrap();
    assert!(native.quarantine_after_fatal().is_empty());
    (native, host, child)
}

#[test]
fn blocked_recovery_waits_for_committed_ready_measurement_before_resuming() {
    let (mut native, host, child) = transferred_child();
    let context = egui::Context::default();
    let version = native.session.version();
    for _ in 0..2 {
        let mut budget = test_budget();
        let advance = native.advance_shutdown_boundary(&context, &mut budget);
        assert!(advance.errors.is_empty(), "{:#?}", advance.errors);
        assert!(!advance.progress, "{advance:#?}");
        assert!(!advance.transition_source_exhausted);
        assert!(!native.is_quiescent());
        assert!(advance.repaint_viewports.is_empty());
        assert_eq!(budget.remaining_records(), 255);
        assert_eq!(native.session.version(), version);
        assert_eq!(
            native
                .session
                .view()
                .item(ItemId::new(2))
                .unwrap()
                .surface(),
            child.surface()
        );
    }

    let mut measure = |frame: &mut NativeHostFrame<'_>, surface| {
        assert_eq!(surface, host.surface());
        frame.frame.measure_surface(
            surface,
            UniformSurfaceMetrics::new(
                LogicalRect::new(20.0, 30.0, 600.0, 420.0).unwrap(),
                LogicalSize::new(32.0, 24.0).unwrap(),
                80.0,
            )
            .unwrap(),
        )?;
        Ok(true)
    };
    let mut budget = test_budget();
    let measured = native.advance_shutdown_boundary_with_close_policy_and_measurement(
        &context,
        crate::NativeWindowClosePolicy::Cancel,
        host.surface(),
        &mut budget,
        Some((host.surface(), &mut measure)),
    );
    assert!(measured.errors.is_empty(), "{:#?}", measured.errors);
    assert!(measured.progress);
    assert!(measured.repaint_viewports.contains(&ViewportId::ROOT));
    assert_eq!(budget.remaining_records(), 255);
    assert_eq!(
        native
            .session
            .view()
            .item(ItemId::new(2))
            .unwrap()
            .surface(),
        child.surface()
    );

    let mut budget = test_budget();
    let recovered = native.advance_shutdown_boundary(&context, &mut budget);
    assert!(recovered.errors.is_empty(), "{:#?}", recovered.errors);
    assert!(recovered.progress);
    assert!(recovered.repaint_viewports.contains(&ViewportId::ROOT));
    assert_eq!(budget.remaining_records(), 255);
    assert_eq!(
        native
            .session
            .view()
            .item(ItemId::new(2))
            .unwrap()
            .surface(),
        host.surface()
    );
    assert!(native.session.recognizes_native_binding(child));
    assert_eq!(native.lifecycle_progress().first_live_admissions(), 0);
    assert_eq!(native.bridge.pending_output_count(), 0);
}

#[test]
fn blocked_first_child_does_not_starve_a_later_recoverable_child() {
    let layout = DockspaceLayout::new([(FIRST_SURFACE, 1, 3), (SECOND_SURFACE, 2, 4)].map(
        |(surface, main, child)| {
            DockspaceSurfaceLayout::new(
                surface,
                DockspaceRootLayout::new(
                    RootId::new(main),
                    DockspaceNode::central_tabs([ItemId::new(main)]),
                ),
            )
            .with_contained(DockspaceContainedLayout::new(
                FloatingPresentationId::new(child),
                DockspaceRootLayout::new(
                    RootId::new(child),
                    DockspaceNode::tabs([ItemId::new(child)]),
                ),
                LogicalRect::new(40.0, 40.0, 320.0, 240.0).unwrap(),
            ))
        },
    ))
    .unwrap();
    let session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let mut native =
        NativeCoordinator::new(session, NativePointerRoster::Exact(Vec::new())).unwrap();
    let (first_host, second_host) = register_roots(&mut native);
    for (binding, viewport, window) in [
        (first_host, ViewportId::ROOT, WindowId::from(11)),
        (
            second_host,
            viewport_id_for(second_host),
            WindowId::from(22),
        ),
    ] {
        native.bind_viewport(viewport, window, binding).unwrap();
    }
    let observations = [
        (first_host, ready_window_facts(0.0)),
        (second_host, ready_window_facts(720.0)),
    ];
    native
        .report_snapshot(
            observations,
            NativeWorkAreaRoster::Unknown,
            test_managed_capabilities(),
        )
        .unwrap();
    commit_model_frame(&mut native);
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut configuration = native
        .session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    configuration.replace_policy(policy).unwrap();
    configuration
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    configuration.commit().unwrap();
    paint_and_present_all_surfaces(&mut native);

    let first_child = transfer_child(&mut native, RootId::new(3), 1400.0, &observations);
    // The first transfer changes the global popup roster. Present only the
    // second host's current ticket; the first child must keep awaiting first-live.
    present_one_host(&mut native, second_host.surface());
    let second_child = transfer_child(
        &mut native,
        RootId::new(4),
        2100.0,
        &[
            observations[0],
            observations[1],
            (first_child, ready_window_facts(1400.0)),
        ],
    );
    assert!(first_child.surface() < second_child.surface());
    for (binding, window) in [
        (first_child, WindowId::from(33)),
        (second_child, WindowId::from(44)),
    ] {
        native
            .bind_viewport(viewport_id_for(binding), window, binding)
            .unwrap();
    }
    let mut configuration = native
        .session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    configuration.replace_policy(DockPolicy::default()).unwrap();
    configuration
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    configuration.commit().unwrap();
    commit_model_frame(&mut native);
    assert!(native.quarantine_after_fatal().is_empty());

    let context = egui::Context::default();
    let version = native.session.version();
    let mut budget = test_budget();
    let blocked = native.advance_shutdown_boundary(&context, &mut budget);
    assert!(blocked.errors.is_empty(), "{blocked:#?}");
    assert!(!blocked.progress, "{blocked:#?}");
    assert!(!blocked.transition_source_exhausted);
    assert!(blocked.repaint_viewports.is_empty());
    assert_eq!(budget.remaining_records(), 254);
    assert_eq!(native.session.version(), version);

    let mut measured_host = None;
    let mut measure = |frame: &mut NativeHostFrame<'_>, host| {
        assert!(measured_host.replace(host).is_none());
        frame.frame.measure_surface(
            host,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
                LogicalSize::new(32.0, 24.0).unwrap(),
                80.0,
            )
            .unwrap(),
        )?;
        Ok(true)
    };
    let mut budget = test_budget();
    let measured = native.advance_shutdown_boundary_with_close_policy_and_measurement(
        &context,
        crate::NativeWindowClosePolicy::Cancel,
        first_host.surface(),
        &mut budget,
        Some((second_host.surface(), &mut measure)),
    );
    assert!(measured.errors.is_empty(), "{measured:#?}");
    assert!(measured.progress);
    assert_eq!(measured_host, Some(second_host.surface()));
    assert_eq!(budget.remaining_records(), 254);
    assert_eq!(native.session.version(), version);

    let mut budget = test_budget();
    let recovered = native.advance_shutdown_boundary(&context, &mut budget);
    assert!(recovered.errors.is_empty(), "{recovered:#?}");
    assert!(recovered.progress);
    assert!(recovered.repaint_viewports.contains(&ViewportId::ROOT));
    assert_eq!(budget.remaining_records(), 254);
    assert_eq!(
        native
            .session
            .view()
            .item(ItemId::new(3))
            .unwrap()
            .surface(),
        first_child.surface(),
    );
    assert_eq!(
        native
            .session
            .view()
            .item(ItemId::new(4))
            .unwrap()
            .surface(),
        second_host.surface(),
    );
    assert!(native.session.recognizes_native_binding(first_child));
    assert_eq!(native.lifecycle_progress().first_live_admissions(), 0);
    let frame = native
        .session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    assert!(
        frame
            .native_create_recovery_surfaces()
            .eq([first_child.surface()])
    );
}

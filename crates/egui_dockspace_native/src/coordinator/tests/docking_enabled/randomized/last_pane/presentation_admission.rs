use super::*;

use dockspace::runtime::{
    PreparedSurfaceAction, SurfaceMeasurementAnswer, SurfaceMeasurementRequest, TabListMenuMetrics,
    TabStripControlKind, TabStripControlMetric, TabStripControlMetrics, TabStripControlPlacement,
    TabStripMetrics,
};

fn menu_measurement(request: SurfaceMeasurementRequest) -> SurfaceMeasurementAnswer {
    match request {
        SurfaceMeasurementRequest::DockBounds { .. }
        | SurfaceMeasurementRequest::PopupPlaneBounds { .. } => {
            SurfaceMeasurementAnswer::Bounds(LogicalRect::new(0.0, 0.0, 260.0, 480.0).unwrap())
        }
        SurfaceMeasurementRequest::PaneMinimum { .. } => {
            SurfaceMeasurementAnswer::PaneMinimum(LogicalSize::new(32.0, 24.0).unwrap())
        }
        SurfaceMeasurementRequest::TabIntrinsic { .. } => {
            SurfaceMeasurementAnswer::TabIntrinsic(80.0)
        }
        SurfaceMeasurementRequest::TabStrip { .. } => {
            let controls = TabStripControlMetrics::new(0.0)
                .unwrap()
                .with_tab_list_menu(
                    TabStripControlMetric::new(20.0, TabStripControlPlacement::ReservedTrailing)
                        .unwrap(),
                );
            SurfaceMeasurementAnswer::TabStrip(
                TabStripMetrics::new(0.0, 0.0)
                    .unwrap()
                    .with_controls(controls)
                    .with_tab_list_menu(
                        TabListMenuMetrics::new(24.0, 8.0, 8.0, 2.0, 160.0, 10.0).unwrap(),
                    ),
            )
        }
    }
}

fn target_output_with_menu_control(
    native: &mut NativeCoordinator,
    has_preview: bool,
) -> (PaintedSurfaceOutput, PreparedSurfaceAction) {
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .frame
        .measure_surface_with(FIRST_SURFACE, menu_measurement)
        .unwrap();
    let plan = frame.frame.paint_plan(FIRST_SURFACE).unwrap().unwrap();
    let mut menu_action = None;
    let mut previews = 0;
    let receipt = plan
        .execute_surface_program(|step| {
            match step {
                SurfacePaintProgramStep::Operation {
                    operation: SurfacePaintProgramOperation::TabStripControl(control),
                    ..
                } if control.kind() == TabStripControlKind::TabListMenu => {
                    assert!(control.enabled());
                    assert!(!control.menu_open());
                    assert!(menu_action.is_none());
                    menu_action = plan.prepare_tab_strip_control_activation(control);
                }
                SurfacePaintProgramStep::DragPreview(preview) => {
                    assert!(matches!(
                        preview.visual(),
                        DockspacePreviewVisual::Dock {
                            surface: FIRST_SURFACE,
                            ..
                        }
                    ));
                    previews += 1;
                }
                _ => {}
            }
            Ok::<(), ()>(())
        })
        .unwrap();
    assert_eq!(previews, usize::from(has_preview));
    frame.frame.confirm_surface_painted(receipt).unwrap();
    let mut report = finish(frame);
    assert!(!report.workspace_changed());
    assert!(report.manipulation_feedback().is_empty());
    assert!(report.take_native_effects().is_empty());
    native.settle_host_frame_report(&report).unwrap();
    let [output] = report.take_painted_outputs().try_into().unwrap();
    (
        output,
        menu_action.expect("the narrow target has a real tab-list menu control"),
    )
}

#[test]
fn a_popup_change_cannot_admit_an_older_preview_to_release_the_last_native_pane() {
    let (mut native, bindings) = routes(true);
    let [host, child]: [NativeSurfaceBinding; 2] = bindings.try_into().unwrap();
    let mut trace = Trace {
        seed: 0xad01_5510,
        round: 0,
        ordinal: 0,
        history: VecDeque::new(),
    };
    let (mut receivers, source, _) = scene(&mut native, FIRST_SURFACE);
    let (initial, _) = target_output_with_menu_control(&mut native, false);
    let artifact = initial.presentation_artifact();
    assert!(initial.matches_native_binding(host));
    native
        .session
        .report_surface_presentation(initial, SurfacePresentationResult::Presented)
        .unwrap();
    let presented = commit_boundary(&mut native);
    native.settle_host_frame_report(&presented).unwrap();
    assert!(
        presented
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.presents(artifact))
    );
    receivers
        .artifacts
        .iter_mut()
        .find(|entry| entry.0 == host.window())
        .unwrap()
        .2 = artifact;
    let target = PhysicalPosition::new(195.0, 360.0);
    global_focus(&mut native, child, &mut trace);
    for (point, state, hover) in [
        (source, Some(ElementState::Pressed), SECOND_SURFACE),
        (target, None, FIRST_SURFACE),
    ] {
        apply(
            &mut native,
            child,
            mouse(
                child,
                point,
                state,
                hover,
                PointerWindowRoute::Window(window(SECOND_SURFACE)),
                None,
            ),
            &receivers,
            true,
            &mut trace,
        );
    }
    global_focus(&mut native, host, &mut trace);
    let version = native.session.version();
    let (held, menu_action) = target_output_with_menu_control(&mut native, true);
    let held_artifact = held.presentation_artifact();
    let mut release = release_last_pane(&mut native, child, target, &receivers, &mut trace);
    let [pending] = release.manipulation_feedback() else {
        panic!("the held preview must gate this release: {release:?}");
    };
    let pending = *pending;
    assert_eq!(
        pending.status(),
        Status::Pending(PendingReason::AwaitingPresentation)
    );
    assert!(release.take_native_effects().is_empty());
    assert!(!release.workspace_changed());
    assert!(native.session.has_pending_presentation());
    assert_child_retained(&native, child);

    // Opening a real control changes popup authority without changing content.
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.frame.submit_surface_action(menu_action).unwrap();
    frame
        .frame
        .measure_surface_with(FIRST_SURFACE, menu_measurement)
        .unwrap();
    let mut menus = 0;
    frame
        .frame
        .paint_plan(FIRST_SURFACE)
        .unwrap()
        .unwrap()
        .visit_surface_program(|step| {
            if let SurfacePaintProgramStep::PopupMenu { .. } = step {
                menus += 1;
            }
            Ok::<(), ()>(())
        })
        .unwrap();
    assert_eq!(menus, 1, "the control must actually open its popup");
    let mut opened = finish(frame);
    native.settle_host_frame_report(&opened).unwrap();
    assert!(!opened.workspace_changed());
    assert!(opened.take_native_effects().is_empty());
    assert_eq!(native.session.version(), version);
    assert_child_retained(&native, child);

    native
        .session
        .report_surface_presentation(held, SurfacePresentationResult::Presented)
        .unwrap();
    let candidate = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    drop(candidate);
    assert_eq!(native.session.version(), version);
    assert_child_retained(&native, child);
    let mut late = commit_boundary(&mut native);
    native.settle_host_frame_report(&late).unwrap();
    assert!(!late.workspace_changed(), "{late:?}");
    assert!(late.take_native_effects().is_empty());
    assert!(
        late.surface_presentation_commits()
            .iter()
            .any(|commit| commit.retires(held_artifact)),
        "the old popup-free output cannot regain presentation authority: {late:?}"
    );
    let feedback: Vec<_> = opened
        .manipulation_feedback()
        .iter()
        .chain(late.manipulation_feedback())
        .collect();
    assert_eq!(feedback.len(), 1, "{opened:?}; {late:?}");
    assert_eq!(feedback[0].manipulation_id(), pending.manipulation_id());
    assert!(matches!(feedback[0].status(), Status::Cancelled(_)));
    assert_eq!(native.session.version(), version);
    assert!(!native.session.has_pending_presentation());
    assert!(!native.session.has_active_gesture());
    assert_child_retained(&native, child);
    assert_terminal_stays_settled(&mut native);
}

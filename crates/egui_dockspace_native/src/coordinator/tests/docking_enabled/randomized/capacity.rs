//! Unequal native client areas use matching presentation and pointer geometry.

use dockspace::runtime::{DockspaceDropDirection, DockspaceGuideScope};

use super::*;

const SIZES: [(f64, f64); 3] = [(700.0, 560.0), (180.0, 300.0), (480.0, 110.0)];

fn metrics(surface: SurfaceId, sizes: &[(f64, f64); 3]) -> UniformSurfaceMetrics {
    let index = SURFACES.iter().position(|id| *id == surface).unwrap();
    let (width, height) = sizes[index];
    UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, width, height).unwrap(),
        LogicalSize::new(96.0, 64.0).unwrap(),
        80.0,
    )
    .unwrap()
}

fn observe_sizes(
    native: &mut NativeCoordinator,
    bindings: &[NativeSurfaceBinding],
    sizes: &[(f64, f64); 3],
    unavailable: Option<(SurfaceId, NativeWindowPresentationState)>,
) {
    native
        .report_snapshot(
            bindings.iter().copied().map(|binding| {
                let index = SURFACES
                    .iter()
                    .position(|surface| *surface == binding.surface())
                    .unwrap();
                let (width, height) = sizes[index];
                let facts = ready_window_facts(x(binding.surface()))
                    .with_content_bounds(
                        PhysicalRect::new(x(binding.surface()), 0.0, width, height).unwrap(),
                    )
                    .with_outer_bounds(
                        PhysicalRect::new(
                            x(binding.surface()) - 8.0,
                            -30.0,
                            width + 16.0,
                            height + 38.0,
                        )
                        .unwrap(),
                    );
                (
                    binding,
                    match unavailable {
                        Some((surface, state)) if surface == binding.surface() => {
                            facts.with_presentation(state)
                        }
                        _ => facts,
                    },
                )
            }),
            NativeWorkAreaRoster::Unknown,
            crate::capabilities::capabilities_for_backend(eframe::NativeWindowingBackend::X11),
        )
        .unwrap();
    let report = commit_boundary(native);
    native.settle_host_frame_report(&report).unwrap();
    assert!(report.inputs().iter().any(|input| matches!(
        input,
        HostInputOutcome::NativePlatformSnapshotApplied { .. }
    )));
}

fn sized_scene(
    native: &mut NativeCoordinator,
    bindings: &[NativeSurfaceBinding],
    sizes: &[(f64, f64); 3],
    item: ItemId,
    target_surface: SurfaceId,
) -> (RecordedSource, PhysicalPosition<f64>, PhysicalPosition<f64>) {
    // The 640x480 parent fixture cannot publish authority for these client areas.
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let mut descriptor = None;
    let mut target = None;
    for surface in SURFACES {
        frame
            .frame
            .measure_surface(surface, metrics(surface, sizes))
            .unwrap();
        let plan = frame.frame.paint_plan(surface).unwrap().unwrap();
        let receipt = plan
            .execute_surface_program(|step| {
                match step {
                    SurfacePaintProgramStep::Operation {
                        operation: SurfacePaintProgramOperation::Tab(tab),
                        ..
                    } if tab.item() == item => {
                        descriptor = plan.receiver_for_tab_body(tab);
                    }
                    SurfacePaintProgramStep::DropGuide { structural, .. }
                        if surface == target_surface
                            && structural.scope() == DockspaceGuideScope::Inner =>
                    {
                        if let Some(center) = structural
                            .targets()
                            .find(|guide| guide.direction() == DockspaceDropDirection::Center)
                        {
                            let bounds = center.hit_bounds();
                            target = Some(PhysicalPosition::new(
                                x(surface) + bounds.x() + bounds.width() * 0.5,
                                bounds.y() + bounds.height() * 0.5,
                            ));
                        }
                    }
                    _ => {}
                }
                Ok::<(), ()>(())
            })
            .unwrap();
        frame.frame.confirm_surface_painted(receipt).unwrap();
    }
    let mut painted = frame.commit().unwrap();
    assert!(painted.take_native_effects().is_empty());
    let artifacts = painted
        .take_painted_outputs()
        .into_iter()
        .map(|output| {
            let binding = bindings
                .iter()
                .copied()
                .find(|binding| output.matches_native_binding(*binding))
                .unwrap();
            let artifact = output.presentation_artifact();
            native
                .session
                .report_surface_presentation(output, SurfacePresentationResult::Presented)
                .unwrap();
            (
                binding.window(),
                native.session.native_surface_placement(binding).unwrap(),
                artifact,
            )
        })
        .collect();
    let observed = commit_boundary(native);
    native.settle_host_frame_report(&observed).unwrap();
    let descriptor = descriptor.expect("the selected source tab must remain operable");
    let source_surface = native.session.view().item(item).unwrap().surface();
    let source = PhysicalPosition::new(
        x(source_surface) + descriptor.center().x(),
        descriptor.center().y(),
    );
    (
        RecordedSource {
            descriptor,
            artifacts,
        },
        source,
        target.unwrap(),
    )
}

fn apply_sized(
    native: &mut NativeCoordinator,
    binding: NativeSurfaceBinding,
    event: WindowEvent,
    receivers: &RecordedSource,
    sizes: &[(f64, f64); 3],
    unavailable: Option<SurfaceId>,
    trace: &mut Trace,
) -> (Option<DockspacePreviewVisual>, HostFrameReport) {
    let ordinal = trace.event(&event);
    let record = captured_record(native, binding, ordinal, event);
    native.bridge.push_record(HostRecord::WindowEvent(record));
    assert_eq!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .unwrap()
            .get(),
        1,
        "{}",
        trace.replay()
    );
    let mut frame = native
        .begin_host_frame(|query| resolve(query, receivers))
        .unwrap();
    let mut preview = None;
    for surface in SURFACES {
        if unavailable == Some(surface) {
            continue;
        }
        frame
            .frame
            .measure_surface(surface, metrics(surface, sizes))
            .unwrap();
        frame
            .frame
            .paint_plan(surface)
            .unwrap()
            .unwrap()
            .visit_surface_program(|step| {
                if let SurfacePaintProgramStep::DragPreview(record) = step {
                    assert!(
                        preview.replace(record.visual()).is_none(),
                        "{}",
                        trace.replay()
                    );
                }
                if let SurfacePaintProgramStep::Operation {
                    operation: SurfacePaintProgramOperation::Pane(pane),
                    ..
                } = step
                {
                    let bounds = pane.content_bounds();
                    assert!(
                        [bounds.x(), bounds.y(), bounds.width(), bounds.height()]
                            .into_iter()
                            .all(f64::is_finite),
                        "{}: {surface:?} has invalid content geometry {bounds:?}",
                        trace.replay()
                    );
                }
                Ok::<(), ()>(())
            })
            .unwrap();
    }
    let mut report = finish(frame);
    assert!(
        report.take_native_effects().is_empty(),
        "{}",
        trace.replay()
    );
    native.settle_host_frame_report(&report).unwrap();
    for id in 1..=5 {
        assert!(
            native.session.view().item(ItemId::new(id)).is_some(),
            "{}: lost pane {id}",
            trace.replay()
        );
    }
    (preview, report)
}

fn dock(
    native: &mut NativeCoordinator,
    bindings: &[NativeSurfaceBinding],
    sizes: &[(f64, f64); 3],
    item: ItemId,
    destination: SurfaceId,
    trace: &mut Trace,
) {
    trace.round += 1;
    let source_surface = native.session.view().item(item).unwrap().surface();
    let binding = *bindings
        .iter()
        .find(|binding| binding.surface() == source_surface)
        .unwrap();
    let destination_binding = *bindings
        .iter()
        .find(|binding| binding.surface() == destination)
        .unwrap();
    let (receivers, source, target) = sized_scene(native, bindings, sizes, item, destination);
    global_focus(native, binding, trace);
    let capture = PointerWindowRoute::Window(window(source_surface));
    for (point, state, hover) in [
        (source, Some(ElementState::Pressed), source_surface),
        (target, None, destination),
    ] {
        let (preview, _) = apply_sized(
            native,
            binding,
            mouse(binding, point, state, hover, capture, None),
            &receivers,
            sizes,
            None,
            trace,
        );
        if state.is_none() {
            assert!(
                matches!(preview, Some(DockspacePreviewVisual::Dock { surface, .. })
                if surface == destination),
                "{}: {preview:?}",
                trace.replay()
            );
        }
    }
    global_focus(native, destination_binding, trace);
    let (_, report) = apply_sized(
        native,
        binding,
        mouse(
            binding,
            target,
            Some(ElementState::Released),
            destination,
            PointerWindowRoute::None,
            None,
        ),
        &receivers,
        sizes,
        None,
        trace,
    );
    assert!(
        report.workspace_changed(),
        "{}: {:?}",
        trace.replay(),
        report.manipulation_feedback()
    );
    assert!(
        report
            .manipulation_feedback()
            .iter()
            .any(|feedback| feedback.status() == Status::Delivered(Delivery::Dock)),
        "{}",
        trace.replay()
    );
    assert_eq!(
        native.session.view().item(item).unwrap().surface(),
        destination,
        "{}",
        trace.replay()
    );
    assert!(!native.session.has_active_gesture(), "{}", trace.replay());
    assert!(
        !native.session.has_pending_native_input(),
        "{}",
        trace.replay()
    );
}

#[test]
fn narrow_and_short_native_windows_keep_raw_docking_operable_after_tab_overflow() {
    let (mut native, bindings) = routes(false);
    let mut trace = Trace {
        seed: 0xca9a,
        round: 0,
        ordinal: 0,
        history: VecDeque::new(),
    };
    observe_sizes(&mut native, &bindings, &SIZES, None);
    for item in [ItemId::new(3), ItemId::new(1)] {
        dock(
            &mut native,
            &bindings,
            &SIZES,
            item,
            SECOND_SURFACE,
            &mut trace,
        );
    }
    sized_scene(
        &mut native,
        &bindings,
        &SIZES,
        ItemId::new(1),
        FIRST_SURFACE,
    );
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let mut overflowed = false;
    frame
        .frame
        .paint_plan(SECOND_SURFACE)
        .unwrap()
        .unwrap()
        .visit_surface_program(|step| {
            if let SurfacePaintProgramStep::Operation {
                operation: SurfacePaintProgramOperation::TabBar(bar),
                ..
            } = step
            {
                assert_eq!(bar.members().len(), 4);
                overflowed = bar.maximum_scroll_offset() > 0.0 && !bar.hidden_items().is_empty();
            }
            Ok::<(), ()>(())
        })
        .unwrap();
    finish(frame);
    assert!(
        overflowed,
        "the narrow window must exercise real tab-strip overflow"
    );
    for destination in [
        THIRD_SURFACE,
        FIRST_SURFACE,
        SECOND_SURFACE,
        THIRD_SURFACE,
        FIRST_SURFACE,
        SECOND_SURFACE,
    ] {
        dock(
            &mut native,
            &bindings,
            &SIZES,
            ItemId::new(1),
            destination,
            &mut trace,
        );
    }
    assert_eq!(
        native
            .session
            .view()
            .item(ItemId::new(3))
            .unwrap()
            .surface(),
        SECOND_SURFACE
    );
    assert_eq!(
        native
            .session
            .view()
            .item(ItemId::new(5))
            .unwrap()
            .surface(),
        THIRD_SURFACE
    );
}

#[test]
fn a_target_hidden_or_minimized_before_raw_release_cannot_receive_the_pane() {
    for state in [
        NativeWindowPresentationState::Hidden,
        NativeWindowPresentationState::Minimized,
    ] {
        let (mut native, bindings) = routes(false);
        let mut trace = Trace {
            seed: 0xc105e,
            round: 0,
            ordinal: 0,
            history: VecDeque::new(),
        };
        observe_sizes(&mut native, &bindings, &SIZES, None);
        let source_binding = *bindings
            .iter()
            .find(|binding| binding.surface() == SECOND_SURFACE)
            .unwrap();
        let (receivers, source, target) =
            sized_scene(&mut native, &bindings, &SIZES, MOVING, FIRST_SURFACE);
        global_focus(&mut native, source_binding, &mut trace);
        for (point, edge, hover) in [
            (source, Some(ElementState::Pressed), SECOND_SURFACE),
            (target, None, FIRST_SURFACE),
        ] {
            let (preview, _) = apply_sized(
                &mut native,
                source_binding,
                mouse(
                    source_binding,
                    point,
                    edge,
                    hover,
                    PointerWindowRoute::Window(window(SECOND_SURFACE)),
                    None,
                ),
                &receivers,
                &SIZES,
                None,
                &mut trace,
            );
            if edge.is_none() {
                assert!(
                    matches!(
                        preview,
                        Some(DockspacePreviewVisual::Dock {
                            surface: FIRST_SURFACE,
                            ..
                        })
                    ),
                    "{}: target must be reachable before becoming {state:?}: {preview:?}",
                    trace.replay()
                );
            }
        }
        assert!(native.session.has_active_gesture());
        observe_sizes(&mut native, &bindings, &SIZES, Some((FIRST_SURFACE, state)));
        // A late callback can still name the formerly hovered HWND/NSWindow.
        let (_, released) = apply_sized(
            &mut native,
            source_binding,
            mouse(
                source_binding,
                target,
                Some(ElementState::Released),
                FIRST_SURFACE,
                PointerWindowRoute::None,
                None,
            ),
            &receivers,
            &SIZES,
            Some(FIRST_SURFACE),
            &mut trace,
        );
        assert_eq!(
            native.session.view().item(MOVING).unwrap().surface(),
            SECOND_SURFACE,
            "{}: {state:?} received a late drop: {:?}",
            trace.replay(),
            released.manipulation_feedback()
        );
        assert!(
            !released
                .manipulation_feedback()
                .iter()
                .any(|feedback| feedback.status() == Status::Delivered(Delivery::Dock))
        );
        // Cancellation must clear any release waiting on the retired presentation.
        apply_sized(
            &mut native,
            source_binding,
            mouse(
                source_binding,
                source,
                None,
                SECOND_SURFACE,
                PointerWindowRoute::Foreign,
                None,
            ),
            &receivers,
            &SIZES,
            Some(FIRST_SURFACE),
            &mut trace,
        );
        assert!(!native.session.has_active_gesture(), "{}", trace.replay());
        observe_sizes(&mut native, &bindings, &SIZES, None);
        dock(
            &mut native,
            &bindings,
            &SIZES,
            MOVING,
            THIRD_SURFACE,
            &mut trace,
        );
        dock(
            &mut native,
            &bindings,
            &SIZES,
            MOVING,
            FIRST_SURFACE,
            &mut trace,
        );
    }
}

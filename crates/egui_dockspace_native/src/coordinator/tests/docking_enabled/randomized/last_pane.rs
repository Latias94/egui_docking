//! Final-pane delivery waits for presentation before retiring its owned native child.

use super::*;
use dockspace::runtime::{
    DockspaceManipulationCancelReason as CancelReason, DockspaceManipulationFeedback,
    DockspaceManipulationPendingReason as PendingReason,
};

#[test]
fn the_last_pane_raw_drop_retires_its_owned_native_child_after_presentation() {
    let (mut native, bindings) = routes(true);
    let [host, child]: [NativeSurfaceBinding; 2] = bindings.try_into().unwrap();
    let mut trace = Trace {
        seed: 0xc11d,
        round: 0,
        ordinal: 0,
        history: VecDeque::new(),
    };
    let (mut receivers, source, target) = scene(&mut native, FIRST_SURFACE);
    global_focus(&mut native, child, &mut trace);
    let held = PointerWindowRoute::Window(window(SECOND_SURFACE));
    for (point, state, hover) in [
        (source, Some(ElementState::Pressed), SECOND_SURFACE),
        (target, None, FIRST_SURFACE),
    ] {
        apply(
            &mut native,
            child,
            mouse(child, point, state, hover, held, None),
            &receivers,
            true,
            &mut trace,
        );
    }
    global_focus(&mut native, host, &mut trace);
    // Publish the cross-window preview before releasing the last pane. The
    // source's native lifetime must survive until a destination is presented.
    let (_, artifacts) = paint_and_present_all_surfaces_with_artifacts(&mut native);
    receivers.artifacts = artifacts
        .into_iter()
        .map(|(binding, artifact)| {
            (
                binding.window(),
                native.session.native_surface_placement(binding).unwrap(),
                artifact,
            )
        })
        .collect();
    let release = mouse(
        child,
        target,
        Some(ElementState::Released),
        FIRST_SURFACE,
        PointerWindowRoute::None,
        None,
    );
    let ordinal = trace.event(&release);
    native
        .bridge
        .push_record(HostRecord::WindowEvent(captured_record(
            &native, child, ordinal, release,
        )));
    native
        .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
        .unwrap();
    let mut frame = native
        .begin_host_frame(|query| resolve(query, &receivers))
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    assert!(!native.session.has_active_gesture(), "{}", trace.replay());
    let mut effects = report.take_native_effects();
    let mut delivered = report
        .manipulation_feedback()
        .iter()
        .any(|feedback| feedback.status() == Status::Delivered(Delivery::Dock));
    for _ in 0..3 {
        let mut presented = paint_and_present_all_surfaces(&mut native);
        delivered |= presented
            .manipulation_feedback()
            .iter()
            .any(|feedback| feedback.status() == Status::Delivered(Delivery::Dock));
        effects.extend(presented.take_native_effects());
        if effects.iter().any(|effect| {
            effect.operation() == &NativeEffectOperation::ReleaseChild { binding: child }
        }) {
            break;
        }
    }
    assert!(
        delivered,
        "{}: the raw release must deliver after its presentation gate",
        trace.replay()
    );
    assert_eq!(
        native.session.view().item(MOVING).unwrap().surface(),
        FIRST_SURFACE
    );
    assert!(
        matches!(effects.as_slice(), [effect] if effect.operation() == &NativeEffectOperation::ReleaseChild { binding: child }),
        "{}: {effects:?}",
        trace.replay()
    );
    retire_child(&mut native, host, child, effects, &mut trace);
}

fn retire_child(
    native: &mut NativeCoordinator,
    host: NativeSurfaceBinding,
    child: NativeSurfaceBinding,
    effects: Vec<NativeEffectRequest>,
    trace: &mut Trace,
) {
    use crate::window_snapshot::CompiledWindowObservation;

    let context = egui::Context::default();
    native.accept_native_effects(&context, effects).unwrap();
    let child_viewport = ViewportId::from_hash_of("randomized-peer");
    let ordinal = trace.event(WindowEvent::Destroyed);
    let destroyed = native.bridge.with_viewports_mut(|routes| {
        NativeWindowEventRecord::for_test_ingress(
            ordinal,
            window(SECOND_SURFACE),
            Some(child_viewport),
            WindowEvent::Destroyed,
            routes,
        )
    });
    assert_eq!(destroyed.binding(), Some(child));
    native
        .bridge
        .push_record(HostRecord::WindowEvent(destroyed));
    native
        .reduce_callback_prefix(&context, &mut test_budget())
        .unwrap();
    commit_boundary(native);
    native.bridge.push_viewport_roster(
        NativeViewportRosterRecord::for_test_with_backend_and_work_areas(
            [
                CompiledWindowObservation::new(host, ready_window_facts(0.0))
                    .with_focus_reportable_for_test(),
            ],
            crate::work_area::FrozenWorkAreaRoster::Unknown,
            eframe::NativeWindowingBackend::X11,
        ),
    );
    native
        .reduce_callback_prefix(&context, &mut test_budget())
        .unwrap();
    let mut observed = commit_boundary(native);
    assert!(observed.take_native_effects().is_empty());
    assert!(observed.manipulation_feedback().is_empty());
    native.settle_host_frame_report(&observed).unwrap();
    let retired = native.prepare_committed_retirements().unwrap().unwrap();
    assert!(native.commit_retirements(retired).is_empty());
    assert!(native.try_report_retirement_quiescence().unwrap());
    for _ in 0..2 {
        let mut report = commit_boundary(native);
        assert!(report.take_native_effects().is_empty());
        assert!(report.manipulation_feedback().is_empty());
        assert!(!report.workspace_changed());
        native.settle_host_frame_report(&report).unwrap();
    }
    assert!(!native.session.recognizes_native_binding(child));
    assert!(!native.binding_has_retained_host_references(child));
    assert_eq!(native.lifecycle_progress().quiescent_retirements(), 1);
    assert!(native.session.is_current_native_binding(host));
    for id in 1..=5 {
        assert_eq!(
            native
                .session
                .view()
                .item(ItemId::new(id))
                .unwrap()
                .surface(),
            FIRST_SURFACE
        );
    }
    assert!(!native.session.has_active_gesture());
    assert!(!native.session.has_pending_native_input());
}

fn hold_target_preview(
    native: &mut NativeCoordinator,
    host: NativeSurfaceBinding,
) -> PaintedSurfaceOutput {
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .frame
        .measure_surface(
            FIRST_SURFACE,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
                LogicalSize::new(32.0, 24.0).unwrap(),
                80.0,
            )
            .unwrap(),
        )
        .unwrap();
    let plan = frame.frame.paint_plan(FIRST_SURFACE).unwrap().unwrap();
    let mut previews = 0;
    let receipt = plan
        .execute_surface_program(|step| {
            if let SurfacePaintProgramStep::DragPreview(preview) = step {
                assert!(matches!(
                    preview.visual(),
                    DockspacePreviewVisual::Dock {
                        surface: FIRST_SURFACE,
                        ..
                    }
                ));
                previews += 1;
            }
            Ok::<(), ()>(())
        })
        .unwrap();
    assert_eq!(
        previews, 1,
        "the held output must contain the exact dock preview"
    );
    frame.frame.confirm_surface_painted(receipt).unwrap();
    let mut report = finish(frame);
    assert!(report.manipulation_feedback().is_empty());
    assert!(report.take_native_effects().is_empty());
    native.settle_host_frame_report(&report).unwrap();
    let [output] = report.take_painted_outputs().try_into().unwrap();
    assert!(output.matches_native_binding(host));
    output
}

fn release_last_pane(
    native: &mut NativeCoordinator,
    child: NativeSurfaceBinding,
    target: PhysicalPosition<f64>,
    receivers: &RecordedSource,
    trace: &mut Trace,
) -> HostFrameReport {
    let event = mouse(
        child,
        target,
        Some(ElementState::Released),
        FIRST_SURFACE,
        PointerWindowRoute::None,
        None,
    );
    let ordinal = trace.event(&event);
    let record = captured_record(native, child, ordinal, event);
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
    let frame = native
        .begin_host_frame(|query| resolve(query, receivers))
        .unwrap();
    let report = finish(frame);
    native.settle_host_frame_report(&report).unwrap();
    report
}

fn assert_child_retained(native: &NativeCoordinator, child: NativeSurfaceBinding) {
    assert!(native.session.is_current_native_binding(child));
    assert_eq!(
        native.session.view().item(MOVING).unwrap().surface(),
        SECOND_SURFACE
    );
    let view = native.session.view();
    let source = view.surface(SECOND_SURFACE).unwrap();
    assert!(source.is_rootless());
    assert_eq!(source.contained_count(), 1);
    assert_eq!(
        source
            .contained()
            .next()
            .unwrap()
            .root()
            .unwrap()
            .content()
            .unwrap()
            .tabs()
            .unwrap()
            .items(),
        &[MOVING]
    );
    assert_eq!(
        view.surface(FIRST_SURFACE)
            .unwrap()
            .main_root()
            .unwrap()
            .central()
            .unwrap()
            .tabs()
            .unwrap()
            .items(),
        &[
            ItemId::new(1),
            ItemId::new(3),
            ItemId::new(4),
            ItemId::new(5)
        ]
    );
}

fn held_last_pane_drop(result: SurfacePresentationResult, with_later_outputs: bool) {
    let (mut native, bindings) = routes(true);
    let [host, child]: [NativeSurfaceBinding; 2] = bindings.try_into().unwrap();
    let mut trace = Trace {
        seed: 0x7a29e7,
        round: 0,
        ordinal: 0,
        history: VecDeque::new(),
    };
    let (receivers, source, target) = scene(&mut native, FIRST_SURFACE);
    global_focus(&mut native, child, &mut trace);
    let held = PointerWindowRoute::Window(window(SECOND_SURFACE));
    for (point, state, hover) in [
        (source, Some(ElementState::Pressed), SECOND_SURFACE),
        (target, None, FIRST_SURFACE),
    ] {
        apply(
            &mut native,
            child,
            mouse(child, point, state, hover, held, None),
            &receivers,
            true,
            &mut trace,
        );
    }
    global_focus(&mut native, host, &mut trace);
    let version = native.session.version();
    let output = hold_target_preview(&mut native, host);
    let (older, output) = if with_later_outputs {
        let later = hold_target_preview(&mut native, host);
        assert_ne!(
            output.presentation_artifact(),
            later.presentation_artifact()
        );
        (Some(output), later)
    } else {
        (None, output)
    };
    assert_child_retained(&native, child);
    assert!(native.session.has_active_gesture());

    // Window-hit answers retain the previously presented artifacts. The held
    // preview output cannot authorize itself as a presented input receiver.
    let mut released = release_last_pane(&mut native, child, target, &receivers, &mut trace);
    let [pending] = released.manipulation_feedback() else {
        panic!("the unpresented last-pane release must wait: {released:?}");
    };
    let pending = *pending;
    assert_eq!(
        pending.status(),
        Status::Pending(PendingReason::AwaitingPresentation)
    );
    assert!(released.take_native_effects().is_empty());
    assert!(!released.workspace_changed());
    assert_eq!(native.session.version(), version);
    assert!(!native.session.has_active_gesture());
    assert!(native.session.has_pending_presentation());
    assert_child_retained(&native, child);
    for _ in 0..2 {
        let mut waiting = commit_boundary(&mut native);
        assert!(waiting.manipulation_feedback().is_empty());
        assert!(waiting.take_native_effects().is_empty());
        assert!(!waiting.workspace_changed());
        native.settle_host_frame_report(&waiting).unwrap();
        assert!(native.session.has_pending_presentation());
        assert_child_retained(&native, child);
    }

    let output = if let Some(older) = older {
        drop_one_preview_while_release_waits(&mut native, child, older);
        // This copy is emitted after the release. Dropping the inherited
        // predecessor must preserve its independently pending terminal.
        let later = hold_target_preview(&mut native, host);
        drop_one_preview_while_release_waits(&mut native, child, output);
        later
    } else {
        output
    };
    let artifact = output.presentation_artifact();
    native
        .session
        .report_surface_presentation(output, result)
        .unwrap();
    // A terminal observation is rollbackable too. Its candidate must not
    // publish delivery, cancellation or native retirement before commit.
    let candidate = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    drop(candidate);
    assert_eq!(native.session.version(), version);
    assert!(native.session.has_pending_presentation());
    assert_child_retained(&native, child);
    let mut settled = commit_boundary(&mut native);
    native.settle_host_frame_report(&settled).unwrap();
    let [terminal] = settled.manipulation_feedback() else {
        panic!("the exact {result:?} terminal must finish its pending release: {settled:?}");
    };
    let terminal = *terminal;
    assert_eq!(terminal.manipulation_id(), pending.manipulation_id());
    assert_ne!(terminal.id(), pending.id());
    assert_eq!(
        settled
            .surface_presentation_commits()
            .iter()
            .filter(|commit| commit.settles(artifact))
            .count(),
        1
    );
    assert!(!native.session.has_pending_presentation());
    let effects = match result {
        SurfacePresentationResult::Presented => {
            assert_eq!(terminal.status(), Status::Delivered(Delivery::Dock));
            assert!(settled.workspace_changed());
            assert_ne!(native.session.version(), version);
            assert!(
                settled
                    .surface_presentation_commits()
                    .iter()
                    .any(|commit| commit.presents(artifact))
            );
            settled.take_native_effects()
        }
        SurfacePresentationResult::Dropped => {
            assert_eq!(
                terminal.status(),
                Status::Cancelled(CancelReason::SceneUnavailable)
            );
            assert!(!settled.workspace_changed());
            assert_eq!(native.session.version(), version);
            assert!(settled.take_native_effects().is_empty());
            assert!(
                settled
                    .surface_presentation_commits()
                    .iter()
                    .any(|commit| commit.retires(artifact))
            );
            assert_child_retained(&native, child);
            assert_terminal_stays_settled(&mut native);
            retry_last_pane_after_cancellation(&mut native, host, child, terminal, &mut trace)
        }
    };
    assert!(matches!(effects.as_slice(), [effect]
        if effect.operation() == &NativeEffectOperation::ReleaseChild { binding: child }));
    assert!(!native.session.has_active_gesture());
    assert!(!native.session.has_pending_presentation());
    assert_eq!(
        native.session.view().item(MOVING).unwrap().surface(),
        FIRST_SURFACE
    );
    assert!(native.session.view().surface(SECOND_SURFACE).is_none());
    assert_terminal_stays_settled(&mut native);
    retire_child(&mut native, host, child, effects, &mut trace);
}

fn drop_one_preview_while_release_waits(
    native: &mut NativeCoordinator,
    child: NativeSurfaceBinding,
    output: PaintedSurfaceOutput,
) {
    let version = native.session.version();
    let artifact = output.presentation_artifact();
    native
        .session
        .report_surface_presentation(output, SurfacePresentationResult::Dropped)
        .unwrap();
    let mut report = commit_boundary(native);
    assert_eq!(
        report
            .surface_presentation_commits()
            .iter()
            .filter(|commit| commit.retires(artifact))
            .count(),
        1
    );
    assert!(report.manipulation_feedback().is_empty());
    assert!(report.take_native_effects().is_empty());
    assert!(!report.workspace_changed());
    assert_eq!(native.session.version(), version);
    assert!(native.session.has_pending_presentation());
    native.settle_host_frame_report(&report).unwrap();
    assert_child_retained(native, child);
}

fn assert_terminal_stays_settled(native: &mut NativeCoordinator) {
    let version = native.session.version();
    for _ in 0..2 {
        let mut report = commit_boundary(native);
        assert!(report.manipulation_feedback().is_empty());
        assert!(report.take_native_effects().is_empty());
        assert!(!report.workspace_changed());
        assert_eq!(native.session.version(), version);
        assert!(!native.session.has_pending_presentation());
        native.settle_host_frame_report(&report).unwrap();
    }
}

fn retry_last_pane_after_cancellation(
    native: &mut NativeCoordinator,
    host: NativeSurfaceBinding,
    child: NativeSurfaceBinding,
    cancelled: DockspaceManipulationFeedback,
    trace: &mut Trace,
) -> Vec<NativeEffectRequest> {
    let (receivers, source, target) = scene(native, FIRST_SURFACE);
    global_focus(native, child, trace);
    let held = PointerWindowRoute::Window(window(SECOND_SURFACE));
    for (point, state, hover) in [
        (source, Some(ElementState::Pressed), SECOND_SURFACE),
        (target, None, FIRST_SURFACE),
    ] {
        apply(
            native,
            child,
            mouse(child, point, state, hover, held, None),
            &receivers,
            true,
            trace,
        );
    }
    global_focus(native, host, trace);
    let mut released = release_last_pane(native, child, target, &receivers, trace);
    let [pending] = released.manipulation_feedback() else {
        panic!("the replacement drag must acquire a fresh release: {released:?}");
    };
    let pending = *pending;
    assert_eq!(
        pending.status(),
        Status::Pending(PendingReason::AwaitingPresentation)
    );
    assert_ne!(pending.manipulation_id(), cancelled.manipulation_id());
    assert!(released.take_native_effects().is_empty());
    assert_child_retained(native, child);
    let output = hold_target_preview(native, host);
    native
        .session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    let mut report = commit_boundary(native);
    native.settle_host_frame_report(&report).unwrap();
    let [delivered] = report.manipulation_feedback() else {
        panic!("the replacement drag must deliver exactly once: {report:?}");
    };
    assert_eq!(delivered.manipulation_id(), pending.manipulation_id());
    assert_eq!(delivered.status(), Status::Delivered(Delivery::Dock));
    assert!(report.workspace_changed());
    report.take_native_effects()
}

#[test]
fn a_held_target_preview_presents_the_last_pane_once_before_releasing_its_child() {
    held_last_pane_drop(SurfacePresentationResult::Presented, false);
}

#[test]
fn a_held_target_preview_dropped_after_release_keeps_its_child_and_allows_a_new_drag() {
    held_last_pane_drop(SurfacePresentationResult::Dropped, false);
}

#[test]
fn dropping_earlier_preview_copies_keeps_the_last_pane_release_waiting_for_later_copies() {
    for result in [
        SurfacePresentationResult::Presented,
        SurfacePresentationResult::Dropped,
    ] {
        held_last_pane_drop(result, true);
    }
}

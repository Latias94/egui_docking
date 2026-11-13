//! Final-pane delivery waits for presentation before retiring its owned native child.

use super::*;

#[test]
fn the_last_pane_raw_drop_retires_its_owned_native_child_after_presentation() {
    use crate::window_snapshot::CompiledWindowObservation;
    use dockspace::runtime::NativeEffectOperation;

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
    commit_boundary(&mut native);
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
    let observed = commit_boundary(&mut native);
    native.settle_host_frame_report(&observed).unwrap();
    let retired = native.prepare_committed_retirements().unwrap().unwrap();
    assert!(native.commit_retirements(retired).is_empty());
    assert!(native.try_report_retirement_quiescence().unwrap());
    commit_boundary(&mut native);
    commit_boundary(&mut native);
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

//! Real callback, presentation, and window-retirement interleavings.

use super::*;
use crate::window_snapshot::CompiledWindowObservation;
use dockspace::model::DockspaceActionOutcome;
use dockspace::runtime::DockspaceManipulationFeedback;

fn trace(seed: u64) -> Trace {
    Trace {
        seed,
        round: 0,
        ordinal: 0,
        history: VecDeque::new(),
    }
}

fn binding(bindings: &[NativeSurfaceBinding], surface: SurfaceId) -> NativeSurfaceBinding {
    bindings
        .iter()
        .copied()
        .find(|binding| binding.surface() == surface)
        .unwrap()
}

fn assert_items(native: &NativeCoordinator) {
    let view = native.session.view();
    let mut items = Vec::new();
    for surface in view.surfaces() {
        for root in surface.main_root().into_iter().chain(
            surface
                .contained()
                .map(|contained| contained.root().unwrap()),
        ) {
            let mut pending = vec![root.content().unwrap()];
            while let Some(node) = pending.pop() {
                if let Some(tabs) = node.tabs() {
                    for &item in tabs.items() {
                        items.push(item);
                        let location = view.item(item).unwrap();
                        assert_eq!(location.surface(), surface.id());
                        assert_eq!(location.root(), root.id());
                    }
                } else {
                    pending.extend(node.split().unwrap().children());
                }
            }
        }
    }
    items.sort_by_key(|item| item.get());
    assert_eq!(items, (1..=5).map(ItemId::new).collect::<Vec<_>>());
}

fn finish_observation(
    native: &mut NativeCoordinator,
    receivers: &RecordedSource,
) -> HostFrameReport {
    let frame = native
        .begin_host_frame(|query| resolve(query, receivers))
        .unwrap();
    let mut report = finish(frame);
    assert!(report.take_native_effects().is_empty());
    native.settle_host_frame_report(&report).unwrap();
    assert_items(native);
    report
}

fn reduce_record(
    native: &mut NativeCoordinator,
    record: HostRecord,
    receivers: &RecordedSource,
) -> HostFrameReport {
    native.bridge.push_record(record);
    assert_eq!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .unwrap()
            .get(),
        1
    );
    finish_observation(native, receivers)
}

fn callback(
    native: &mut NativeCoordinator,
    delivery: NativeSurfaceBinding,
    event: WindowEvent,
    receivers: &RecordedSource,
    trace: &mut Trace,
) -> HostFrameReport {
    let ordinal = trace.event(&event);
    let record = native.bridge.with_viewports_mut(|routes| {
        NativeWindowEventRecord::for_test_ingress(
            ordinal,
            window(delivery.surface()),
            None,
            event,
            routes,
        )
    });
    reduce_record(native, HostRecord::WindowEvent(record), receivers)
}

fn roster(
    native: &mut NativeCoordinator,
    bindings: &[NativeSurfaceBinding],
    receivers: &RecordedSource,
) -> HostFrameReport {
    native.bridge.push_viewport_roster(
        NativeViewportRosterRecord::for_test_with_backend_and_work_areas(
            bindings.iter().copied().map(|binding| {
                CompiledWindowObservation::new(binding, ready_window_facts(x(binding.surface())))
                    .with_focus_reportable_for_test()
            }),
            crate::work_area::FrozenWorkAreaRoster::Unknown,
            eframe::NativeWindowingBackend::X11,
        ),
    );
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .is_some()
    );
    let report = finish_observation(native, receivers);
    assert!(report.inputs().iter().any(|input| matches!(
        input,
        HostInputOutcome::NativePlatformSnapshotApplied { .. }
    )));
    report
}

fn begin_drag(
    native: &mut NativeCoordinator,
    bindings: &[NativeSurfaceBinding],
    destination: SurfaceId,
    trace: &mut Trace,
) -> (RecordedSource, PhysicalPosition<f64>, PhysicalPosition<f64>) {
    let source_surface = native.session.view().item(MOVING).unwrap().surface();
    let source_binding = binding(bindings, source_surface);
    let (receivers, source, target) = scene(native, destination);
    global_focus(native, source_binding, trace);
    let held = PointerWindowRoute::Window(window(source_surface));
    for (point, state, hover) in [
        (source, Some(ElementState::Pressed), source_surface),
        (target, None, destination),
    ] {
        let (preview, report) = apply(
            native,
            source_binding,
            mouse(source_binding, point, state, hover, held, None),
            &receivers,
            false,
            trace,
        );
        assert!(report.manipulation_feedback().is_empty());
        if state.is_none() {
            assert!(matches!(
                preview,
                Some(DockspacePreviewVisual::Dock { surface, .. }) if surface == destination
            ));
        }
    }
    assert!(native.session.has_active_gesture());
    (receivers, source, target)
}

fn terminal_feedback(report: &HostFrameReport) -> Vec<DockspaceManipulationFeedback> {
    report
        .manipulation_feedback()
        .iter()
        .copied()
        .filter(|feedback| !matches!(feedback.status(), Status::Pending(_)))
        .collect()
}

fn retire_and_recover(
    native: &mut NativeCoordinator,
    bindings: &[NativeSurfaceBinding],
    dead: NativeSurfaceBinding,
    receivers: &RecordedSource,
    target: PhysicalPosition<f64>,
    trace: &mut Trace,
) -> DockspaceManipulationFeedback {
    // The attached window owns one Destroyed receipt. Duplicate OS callbacks
    // are rejected at raw ingress rather than reduced into another core fact.
    let ordinal = trace.event(WindowEvent::Destroyed);
    let duplicate = native.bridge.with_viewports_mut(|routes| {
        NativeWindowEventRecord::for_test_ingress(
            ordinal,
            window(dead.surface()),
            None,
            WindowEvent::Destroyed,
            routes,
        )
    });
    native
        .bridge
        .push_record(HostRecord::WindowEvent(duplicate));
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .is_none()
    );
    assert_eq!(native.lifecycle_progress().destroyed_observations(), 1);
    for event in [
        WindowEvent::Focused(true),
        mouse(
            dead,
            target,
            Some(ElementState::Released),
            FIRST_SURFACE,
            PointerWindowRoute::Window(window(dead.surface())),
            None,
        ),
    ] {
        let report = callback(native, dead, event, receivers, trace);
        assert!(!report.workspace_changed());
        assert!(report.manipulation_feedback().is_empty());
    }
    let retired = native.prepare_committed_retirements().unwrap().unwrap();
    assert!(native.commit_retirements(retired).is_empty());
    assert!(native.try_report_retirement_quiescence().unwrap());
    for _ in 0..2 {
        let report = finish_observation(native, receivers);
        assert!(report.manipulation_feedback().is_empty());
    }
    assert!(!native.session.recognizes_native_binding(dead));
    assert!(!native.binding_has_retained_host_references(dead));
    assert!(
        native
            .session
            .native_local_coordinates(dead.window())
            .is_none()
    );
    assert!(!native.pointer_translator.references_binding(dead.window()));

    // External-window destruction retains logical content. The application can
    // recover that complete root into a survivor before resuming physical input.
    let (survivor, anchor) = if dead.surface() == SECOND_SURFACE {
        (FIRST_SURFACE, ItemId::new(1))
    } else {
        (THIRD_SURFACE, ItemId::new(5))
    };
    global_focus(native, binding(bindings, survivor), trace);
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let recovered = frame
        .frame
        .dock_root_current(
            RootId::new(dead.surface().get()),
            DockPlacement::Center(DockAnchor::Item(anchor)),
        )
        .unwrap();
    let mut report = finish(frame);
    assert!(matches!(
        report.submitted_action_outcome(recovered),
        Some(HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::RootDocked { changed: true, .. }
        ))
    ));
    assert!(report.take_native_effects().is_empty());
    native.settle_host_frame_report(&report).unwrap();
    assert_items(native);
    assert!(native.session.view().surface(dead.surface()).is_none());
    assert!(
        bindings
            .iter()
            .filter(|binding| **binding != dead)
            .all(|binding| native.session.is_current_native_binding(*binding))
    );
    complete_drag(native, bindings, THIRD_SURFACE, trace)
}

fn complete_drag(
    native: &mut NativeCoordinator,
    bindings: &[NativeSurfaceBinding],
    destination: SurfaceId,
    trace: &mut Trace,
) -> DockspaceManipulationFeedback {
    let (receivers, _, target) = begin_drag(native, bindings, destination, trace);
    let source_surface = native.session.view().item(MOVING).unwrap().surface();
    let source = binding(bindings, source_surface);
    global_focus(native, binding(bindings, destination), trace);
    let event = mouse(
        source,
        target,
        Some(ElementState::Released),
        destination,
        PointerWindowRoute::None,
        None,
    );
    let report = callback(native, source, event, &receivers, trace);
    let [delivered]: [DockspaceManipulationFeedback; 1] =
        terminal_feedback(&report).try_into().unwrap();
    assert_eq!(delivered.status(), Status::Delivered(Delivery::Dock));
    assert!(report.workspace_changed());
    assert_eq!(
        native.session.view().item(MOVING).unwrap().surface(),
        destination
    );
    assert!(!native.session.has_active_gesture());
    assert!(!native.session.has_pending_native_input());
    assert_eq!(native.bridge.input_backlog_for_test(), (0, None));
    assert_items(native);
    delivered
}

#[test]
fn a_duplicate_raw_release_after_delivery_is_inert_and_does_not_poison_the_next_drag() {
    let (mut native, bindings) = routes(false);
    let mut trace = trace(0xd0b1e);
    let source = binding(&bindings, SECOND_SURFACE);
    let (receivers, _, target) = begin_drag(&mut native, &bindings, FIRST_SURFACE, &mut trace);
    global_focus(&mut native, binding(&bindings, FIRST_SURFACE), &mut trace);
    let event = mouse(
        source,
        target,
        Some(ElementState::Released),
        FIRST_SURFACE,
        PointerWindowRoute::None,
        None,
    );
    let report = callback(&mut native, source, event.clone(), &receivers, &mut trace);
    let [delivered]: [DockspaceManipulationFeedback; 1] =
        terminal_feedback(&report).try_into().unwrap();
    assert_eq!(delivered.status(), Status::Delivered(Delivery::Dock));
    assert!(report.workspace_changed());
    let version = native.session.version();
    let duplicate = callback(&mut native, source, event, &receivers, &mut trace);
    assert!(!duplicate.workspace_changed());
    assert!(duplicate.manipulation_feedback().is_empty());
    assert_eq!(native.session.version(), version);
    assert!(!native.session.has_active_gesture());
    assert!(!native.session.has_pending_native_input());
    assert_items(&native);
    let next = complete_drag(&mut native, &bindings, THIRD_SURFACE, &mut trace);
    assert_ne!(next.manipulation_id(), delivered.manipulation_id());
}

#[test]
fn a_source_close_and_destroyed_callback_cancel_once_and_leave_survivors_operable() {
    let (mut native, bindings) = routes(false);
    let mut trace = trace(0xc105e);
    let source = binding(&bindings, SECOND_SURFACE);
    let (receivers, _, target) = begin_drag(&mut native, &bindings, FIRST_SURFACE, &mut trace);
    let mut terminals = Vec::new();
    let closed = callback(
        &mut native,
        source,
        WindowEvent::CloseRequested,
        &receivers,
        &mut trace,
    );
    assert!(closed.inputs().iter().any(|input| matches!(
        input,
        HostInputOutcome::NativeCloseObservationApplied { close_requests } if close_requests.len() == 1
    )));
    assert!(!closed.workspace_changed());
    terminals.extend(terminal_feedback(&closed));
    let destroyed = callback(
        &mut native,
        source,
        WindowEvent::Destroyed,
        &receivers,
        &mut trace,
    );
    assert!(destroyed.inputs().iter().any(|input| matches!(
        input,
        HostInputOutcome::NativeInputBindingRetired { binding } if binding.window() == source.window()
    )));
    terminals.extend(terminal_feedback(&destroyed));
    let live = bindings
        .iter()
        .copied()
        .filter(|binding| *binding != source)
        .collect::<Vec<_>>();
    let observed = roster(&mut native, &live, &receivers);
    terminals.extend(terminal_feedback(&observed));
    assert_eq!(terminals.len(), 1, "{}: {terminals:?}", trace.replay());
    assert!(matches!(terminals[0].status(), Status::Cancelled(_)));
    assert!(!native.session.has_active_gesture());
    assert_eq!(
        native.session.view().item(MOVING).unwrap().surface(),
        SECOND_SURFACE
    );
    assert!(!native.session.is_current_native_binding(source));
    let delivered = retire_and_recover(
        &mut native,
        &bindings,
        source,
        &receivers,
        target,
        &mut trace,
    );
    assert_ne!(delivered.manipulation_id(), terminals[0].manipulation_id());
}

#[test]
fn a_destroyed_drag_target_cannot_receive_the_release_and_leaves_survivors_operable() {
    let (mut native, bindings) = routes(false);
    let mut trace = trace(0xde57);
    let source = binding(&bindings, SECOND_SURFACE);
    let dead = binding(&bindings, FIRST_SURFACE);
    let (receivers, _, target) = begin_drag(&mut native, &bindings, FIRST_SURFACE, &mut trace);
    let destroyed = callback(
        &mut native,
        dead,
        WindowEvent::Destroyed,
        &receivers,
        &mut trace,
    );
    let mut terminals = terminal_feedback(&destroyed);
    let live = bindings
        .iter()
        .copied()
        .filter(|binding| *binding != dead)
        .collect::<Vec<_>>();
    terminals.extend(terminal_feedback(&roster(&mut native, &live, &receivers)));
    let released = callback(
        &mut native,
        source,
        mouse(
            source,
            target,
            Some(ElementState::Released),
            FIRST_SURFACE,
            PointerWindowRoute::None,
            None,
        ),
        &receivers,
        &mut trace,
    );
    assert!(!released.workspace_changed());
    terminals.extend(terminal_feedback(&released));
    assert_eq!(
        terminals.len(),
        1,
        "the release into the destroyed target must terminate once: {terminals:?}"
    );
    assert!(matches!(terminals[0].status(), Status::Cancelled(_)));
    assert_eq!(
        native.session.view().item(MOVING).unwrap().surface(),
        SECOND_SURFACE
    );
    assert!(!native.session.is_current_native_binding(dead));

    let delivered =
        retire_and_recover(&mut native, &bindings, dead, &receivers, target, &mut trace);
    assert_ne!(delivered.manipulation_id(), terminals[0].manipulation_id());
}

#[test]
fn an_incomplete_roster_and_unknown_capture_cannot_resurrect_a_drag_after_a_fresh_press() {
    let (mut native, bindings) = routes(false);
    let mut trace = trace(0xabc0);
    let source = binding(&bindings, SECOND_SURFACE);
    let (receivers, position, target) =
        begin_drag(&mut native, &bindings, FIRST_SURFACE, &mut trace);
    let ordinal = trace.event("unknown global focus");
    let report = reduce_record(
        &mut native,
        HostRecord::GlobalFocus(crate::focus_control::NativeGlobalFocusRecord::for_test(
            ordinal,
            NativeGlobalFocus::Unknown,
        )),
        &receivers,
    );
    assert!(!report.workspace_changed());
    assert!(report.manipulation_feedback().is_empty());
    let unknown = callback(
        &mut native,
        source,
        mouse(
            source,
            target,
            None,
            FIRST_SURFACE,
            PointerWindowRoute::Unknown,
            None,
        ),
        &receivers,
        &mut trace,
    );
    assert!(unknown.manipulation_feedback().is_empty());
    assert!(native.session.has_active_gesture());
    let incomplete = bindings
        .iter()
        .copied()
        .filter(|binding| *binding != source)
        .collect::<Vec<_>>();
    let report = roster(&mut native, &incomplete, &receivers);
    let mut terminals = terminal_feedback(&report);
    assert!(
        bindings
            .iter()
            .all(|binding| native.session.is_current_native_binding(*binding))
    );
    assert_eq!(native.lifecycle_progress().destroyed_observations(), 0);
    assert!(native.prepare_committed_retirements().unwrap().is_none());

    for (point, state, hover) in [
        (position, ElementState::Pressed, SECOND_SURFACE),
        (target, ElementState::Released, FIRST_SURFACE),
    ] {
        let report = callback(
            &mut native,
            source,
            mouse(
                source,
                point,
                Some(state),
                hover,
                PointerWindowRoute::Unknown,
                None,
            ),
            &receivers,
            &mut trace,
        );
        assert!(!report.workspace_changed());
        terminals.extend(terminal_feedback(&report));
        assert_eq!(
            native.session.view().item(MOVING).unwrap().surface(),
            SECOND_SURFACE
        );
    }
    assert!(
        terminals
            .iter()
            .any(|feedback| matches!(feedback.status(), Status::Cancelled(_)))
    );
    for (index, terminal) in terminals.iter().enumerate() {
        assert!(
            !terminals[..index]
                .iter()
                .any(|earlier| earlier.manipulation_id() == terminal.manipulation_id())
        );
        assert!(!matches!(terminal.status(), Status::Delivered(_)));
    }
    assert!(!native.session.has_active_gesture());
    let restored = roster(&mut native, &bindings, &receivers);
    assert!(restored.manipulation_feedback().is_empty());
    assert!(
        bindings
            .iter()
            .all(|binding| native.session.is_current_native_binding(*binding))
    );
    let delivered = complete_drag(&mut native, &bindings, THIRD_SURFACE, &mut trace);
    assert!(
        terminals
            .iter()
            .all(|terminal| { terminal.manipulation_id() != delivered.manipulation_id() })
    );
}

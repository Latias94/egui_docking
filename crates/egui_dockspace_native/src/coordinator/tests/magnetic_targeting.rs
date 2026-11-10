use std::time::Duration;

use dockspace::geometry::LogicalPoint;
use dockspace::runtime::{
    DockspaceManipulationDelivery as Delivery, DockspaceManipulationStatus as Status,
    DockspacePreviewVisual, DockspaceReceiverDescriptor, NativeInputStorageLimit,
    NativeReceiverPurpose,
};
use winit::dpi::PhysicalPosition;
use winit::event::{DeviceId, ElementState, MouseButton, PointerEventFacts, PointerWindowRoute};
use winit::keyboard::ModifiersState;

use super::*;

#[path = "escape_cancel.rs"]
mod escape_cancel;

fn captured_record(
    native: &NativeCoordinator,
    binding: NativeSurfaceBinding,
    ordinal: u64,
    event: WindowEvent,
) -> NativeWindowEventRecord {
    let record = native.bridge.with_viewports(|routes| {
        let viewport = routes.viewport_for_window(binding.window()).unwrap();
        NativeWindowEventRecord::for_test_snapshot(
            ordinal,
            routes.window(viewport).unwrap(),
            Some(viewport),
            event,
            routes,
        )
    });
    assert_eq!(record.binding(), Some(binding));
    record
}

fn mouse_event(
    desktop: PhysicalPosition<f64>,
    state: Option<ElementState>,
    hover: u64,
    modifiers: Option<ModifiersState>,
) -> WindowEvent {
    // This fixture delivers every edge through the source window at x = 720.
    let position = PhysicalPosition::new(desktop.x - 720.0, desktop.y);
    let facts = PointerEventFacts {
        surface_position: Some(position),
        desktop_position: Some(desktop),
        modifiers,
        hover: PointerWindowRoute::Window(WindowId::from(hover)),
        capture: PointerWindowRoute::Window(WindowId::from(22)),
        ..PointerEventFacts::default()
    };
    match state {
        Some(state) => WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            primary_press: None,
            state,
            button: MouseButton::Left,
            facts,
        },
        None => WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position,
            facts,
        },
    }
}

fn record_pointer(
    native: &mut NativeCoordinator,
    binding: NativeSurfaceBinding,
    ordinal: u64,
    event: WindowEvent,
) {
    let record = captured_record(native, binding, ordinal, event);
    let NativePointerTranslation::Input(input) =
        native
            .pointer_translator
            .translate_captured(&record, Duration::from_millis(40), |_| None)
    else {
        panic!("the fixture retains a real pointer edge");
    };
    native.record_pointer(input).unwrap();
}

fn presented_routes() -> (
    NativeCoordinator,
    NativeSurfaceBinding,
    NativeSurfaceBinding,
) {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    for (viewport, window, binding) in [
        (ViewportId::ROOT, WindowId::from(11), first),
        (
            ViewportId::from_hash_of("local-coordinate-peer"),
            WindowId::from(22),
            second,
        ),
    ] {
        native.bind_viewport(viewport, window, binding).unwrap();
    }
    native
        .report_snapshot(
            [
                (first, ready_window_facts(0.0)),
                (second, ready_window_facts(720.0)),
            ],
            NativeWorkAreaRoster::Unknown,
            crate::capabilities::capabilities_for_backend(eframe::NativeWindowingBackend::X11),
        )
        .unwrap();
    commit_boundary(&mut native);
    paint_and_present_all_surfaces(&mut native);
    (native, first, second)
}

fn shared_route_facts() -> NativeWindowFacts {
    let bounds = PhysicalRect::new(0.0, 0.0, 1400.0, 480.0).unwrap();
    ready_window_facts(0.0)
        .with_content_bounds(bounds)
        .with_outer_bounds(bounds)
}

fn shared_routes() -> (NativeCoordinator, NativeSurfaceBinding) {
    let mut native = coordinator();
    native
        .register_native_root(FIRST_SURFACE, HostWindowToken::new(22))
        .unwrap();
    let original = commit_boundary(&mut native).native_bindings()[0];
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(22), original)
        .unwrap();
    native
        .report_snapshot(
            [(original, shared_route_facts())],
            NativeWorkAreaRoster::Unknown,
            crate::capabilities::capabilities_for_backend(eframe::NativeWindowingBackend::X11),
        )
        .unwrap();
    commit_boundary(&mut native);
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .frame
        .set_native_surface_placements(
            original.window(),
            &[FIRST_SURFACE, SECOND_SURFACE].map(|surface| {
                let x = if surface == FIRST_SURFACE { 0.0 } else { 720.0 };
                dockspace::runtime::NativeSurfacePlacement::new(
                    surface,
                    LogicalSize::new(640.0, 480.0).unwrap(),
                    dockspace::geometry::PhysicalPoint::new(x, 0.0).unwrap(),
                    ScaleFactor::new(1.0).unwrap(),
                    PhysicalRect::new(x, 0.0, 640.0, 480.0).unwrap(),
                )
            }),
        )
        .unwrap();
    finish(frame);
    paint_and_present_all_surfaces(&mut native);
    assert!(!native.session.recognizes_native_binding(original));
    assert!(
        native
            .session
            .recognizes_native_window_binding(original.window())
    );
    assert_eq!(native.viewport_binding(ViewportId::ROOT), Some(original));
    (native, original)
}

struct RecordedSource {
    descriptor: DockspaceReceiverDescriptor,
    artifacts: Vec<(
        dockspace::runtime::NativeWindowBinding,
        dockspace::runtime::NativeSurfacePlacement,
        dockspace::runtime::SurfacePresentationArtifact,
    )>,
}

fn source_and_target(
    native: &mut NativeCoordinator,
) -> (RecordedSource, PhysicalPosition<f64>, PhysicalPosition<f64>) {
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let source_plan = frame.frame.paint_plan(SECOND_SURFACE).unwrap().unwrap();
    let mut receiver = None;
    source_plan
        .visit_surface_program(|step| {
            if let SurfacePaintProgramStep::Operation {
                operation: SurfacePaintProgramOperation::Tab(tab),
                ..
            } = step
                && tab.item() == ItemId::new(2)
            {
                receiver = source_plan.receiver_for_tab_body(tab);
            }
            Ok::<(), ()>(())
        })
        .unwrap();
    let receiver = receiver.unwrap();
    let source = PhysicalPosition::new(720.0 + receiver.center().x(), receiver.center().y());
    let target_plan = frame.frame.paint_plan(FIRST_SURFACE).unwrap().unwrap();
    let mut target = None;
    target_plan
        .visit_surface_program(|step| {
            if let SurfacePaintProgramStep::Operation {
                operation: SurfacePaintProgramOperation::Pane(pane),
                ..
            } = step
            {
                let bounds = pane.content_bounds();
                target = Some(PhysicalPosition::new(
                    bounds.x() + bounds.width() * 0.75,
                    bounds.y() + bounds.height() * 0.75,
                ));
            }
            Ok::<(), ()>(())
        })
        .unwrap();
    let target = target.unwrap();
    let point = LogicalPoint::new(target.x, target.y).unwrap();
    target_plan
        .visit_surface_program(|step| {
            if let SurfacePaintProgramStep::DropGuide { structural, .. } = step {
                assert!(
                    structural
                        .targets()
                        .all(|guide| !guide.hit_bounds().contains(point))
                );
            }
            Ok::<(), ()>(())
        })
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    let (_, artifacts) = paint_and_present_all_surfaces_with_artifacts(native);
    let artifacts = artifacts
        .into_iter()
        .map(|(binding, artifact)| {
            (
                binding.window(),
                native.session.native_surface_placement(binding).unwrap(),
                artifact,
            )
        })
        .collect();
    (
        RecordedSource {
            descriptor: receiver,
            artifacts,
        },
        source,
        target,
    )
}

fn resolve(query: NativeReceiverQuery, source: &RecordedSource) -> NativeReceiverAnswer {
    let query = match query {
        NativeReceiverQuery::WindowHit(query) => {
            return NativeReceiverAnswer::WindowHit(
                source
                    .artifacts
                    .iter()
                    .find(|(window, placement, _)| {
                        *window == query.window() && placement.clip().contains(query.point())
                    })
                    .map_or(
                        dockspace::runtime::NativeWindowHit::Unknown,
                        |(_, _, artifact)| dockspace::runtime::NativeWindowHit::Surface(*artifact),
                    ),
            );
        }
        NativeReceiverQuery::Surface(query) => query,
    };
    if matches!(query.purpose(), NativeReceiverPurpose::HoverHit) {
        NativeReceiverAnswer::ClearHoverLane(query.presented_surface())
    } else {
        query
            .bind_receiver(&source.descriptor)
            .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock)
    }
}

fn preview(frame: &mut NativeHostFrame<'_>) -> Option<DockspacePreviewVisual> {
    if frame.frame.paint_plan(FIRST_SURFACE).unwrap().is_none() {
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
    }
    let mut preview = None;
    frame
        .frame
        .paint_plan(FIRST_SURFACE)
        .unwrap()
        .unwrap()
        .visit_surface_program(|step| {
            if let SurfacePaintProgramStep::DragPreview(record) = step {
                assert!(preview.replace(record.visual()).is_none());
            }
            Ok::<(), ()>(())
        })
        .unwrap();
    preview
}

fn finish(mut frame: NativeHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn commit_boundary(native: &mut NativeCoordinator) -> HostFrameReport {
    let frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    finish(frame)
}

#[test]
fn shared_sibling_shift_callback_uses_the_physical_window_after_placement_rebinding() {
    let (mut native, original) = shared_routes();
    let (receivers, source, target) = source_and_target(&mut native);
    for (ordinal, point, state) in [(1, source, Some(ElementState::Pressed)), (2, target, None)] {
        record_pointer(
            &mut native,
            original,
            ordinal,
            mouse_event(point, state, 22, None),
        );
    }
    let mut dragging = native
        .begin_host_frame(|query| resolve(query, &receivers))
        .unwrap();
    assert!(matches!(
        preview(&mut dragging),
        Some(DockspacePreviewVisual::Dock {
            surface: FIRST_SURFACE,
            ..
        })
    ));
    assert!(!finish(dragging).workspace_changed());
    assert!(native.session.has_active_gesture());

    let shift = captured_record(
        &native,
        original,
        3,
        WindowEvent::ModifiersChanged(ModifiersState::SHIFT.into()),
    );
    native.bridge.push_record(HostRecord::WindowEvent(shift));
    native
        .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
        .unwrap()
        .unwrap();
    let mut shifted = native
        .begin_host_frame(|query| resolve(query, &receivers))
        .unwrap();
    assert!(preview(&mut shifted).is_none());
    let report = finish(shifted);
    assert!(report.inputs().iter().any(|input| matches!(
        input,
        HostInputOutcome::MagneticTargetingUpdated {
            enabled: false,
            changed: true
        }
    )));
    assert!(!report.workspace_changed());
    assert!(native.session.has_active_gesture());
}

#[test]
fn shared_window_destroyed_callback_survives_retirement_of_its_original_placement() {
    let (mut native, original) = shared_routes();
    let sibling = native
        .session
        .native_surface_bindings_for_window(original.window())
        .find(|binding| binding.surface() == SECOND_SURFACE)
        .unwrap();
    let mut selection = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    selection.frame.select_item_current(ItemId::new(2)).unwrap();
    let mut selected = finish(selection);
    let effects = selected.take_native_effects();
    assert!(matches!(effects.as_slice(), [request]
        if request.operation() == &NativeEffectOperation::RequestFocus { binding: sibling }));
    native
        .accept_native_effects(&egui::Context::default(), effects)
        .unwrap();
    assert_eq!(native.focus_control.pending_binding(), Some(sibling));
    let (receivers, source, target) = source_and_target(&mut native);
    for (ordinal, point, state) in [(1, source, Some(ElementState::Pressed)), (2, target, None)] {
        record_pointer(
            &mut native,
            original,
            ordinal,
            mouse_event(point, state, 22, None),
        );
    }
    let dragging = native
        .begin_host_frame(|query| resolve(query, &receivers))
        .unwrap();
    assert!(!finish(dragging).workspace_changed());
    assert!(native.session.has_active_gesture());

    let destroyed = native.bridge.with_viewports_mut(|routes| {
        NativeWindowEventRecord::for_test_ingress(
            3,
            WindowId::from(22),
            Some(ViewportId::ROOT),
            WindowEvent::Destroyed,
            routes,
        )
    });
    assert_eq!(destroyed.binding(), Some(original));
    native
        .bridge
        .push_record(HostRecord::WindowEvent(destroyed));
    native
        .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
        .unwrap()
        .unwrap();
    assert!(native.focus_control.pending_binding().is_none());
    assert!(
        native
            .pending_effect_results
            .iter()
            .any(|pending| { pending.result.binding() == sibling })
    );
    let report = commit_boundary(&mut native);
    assert!(report.inputs().iter().any(|input| matches!(
        input,
        HostInputOutcome::NativeInputBindingRetired { binding }
            if binding.window() == original.window()
    )));
    assert!(!native.session.has_active_gesture());
    assert!(native.bridge.retirement_requires_snapshot());
    native.bridge.push_viewport_roster(
        NativeViewportRosterRecord::for_test_with_backend_and_work_areas(
            [],
            crate::work_area::FrozenWorkAreaRoster::Unknown,
            eframe::NativeWindowingBackend::X11,
        ),
    );
    assert!(native.reduce_next_viewport_roster().unwrap());
    let report = commit_boundary(&mut native);
    assert!(report.inputs().iter().any(|input| matches!(
        input,
        HostInputOutcome::NativePlatformSnapshotApplied { .. }
    )));
    assert!(native.settle_host_frame_report(&report).unwrap());
    assert!(!native.bridge.retirement_requires_snapshot());
    assert!(
        native
            .session
            .native_local_coordinates(original.window())
            .is_none()
    );
    for surface in [FIRST_SURFACE, SECOND_SURFACE] {
        assert!(native.session.presented_surface(surface).is_none());
    }
}

#[test]
fn one_modifier_callback_records_once_for_several_mouse_devices_and_charges_only_at_poll() {
    let (mut native, _, binding) = presented_routes();
    let press = captured_record(
        &native,
        binding,
        1,
        mouse_event(
            PhysicalPosition::new(740.0, 70.0),
            Some(ElementState::Pressed),
            22,
            None,
        ),
    );
    for device in 1..=4 {
        let NativePointerTranslation::Input(input) = native
            .pointer_translator
            .translate_for_test_device(&press, device, |_| None)
        else {
            panic!("each physical mouse enrolls independently");
        };
        native.record_pointer(input).unwrap();
    }
    commit_boundary(&mut native);
    let record = captured_record(
        &native,
        binding,
        2,
        WindowEvent::ModifiersChanged(ModifiersState::SHIFT.into()),
    );
    let recorded = native.session.native_input_recorded_count();
    let translator = format!("{:?}", native.pointer_translator);
    let mut budget = test_budget();
    let allowance = budget.remaining_records();
    native.bridge.push_record(HostRecord::WindowEvent(record));
    assert_eq!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut budget)
            .unwrap()
            .unwrap()
            .get(),
        1
    );
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    assert_eq!(format!("{:?}", native.pointer_translator), translator);
    assert_eq!(budget.remaining_records(), allowance);
    assert!(native.next_window_event().unwrap().is_none());
    let frame = native
        .poll_host_frame(&mut budget, |_| NativeReceiverAnswer::Unknown)
        .unwrap()
        .unwrap();
    finish(frame);
    assert_eq!(budget.remaining_records(), allowance - 1);
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
}

#[test]
fn raw_modifier_order_survives_a_dropped_frame_without_recording_the_successor() {
    let (mut native, binding, _) = presented_routes();
    for (ordinal, modifiers) in [(1, ModifiersState::SHIFT), (2, ModifiersState::empty())] {
        let record = captured_record(
            &native,
            binding,
            ordinal,
            WindowEvent::ModifiersChanged(modifiers.into()),
        );
        native.bridge.push_record(HostRecord::WindowEvent(record));
    }
    let recorded = native.session.native_input_recorded_count();
    native
        .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
        .unwrap()
        .unwrap();
    assert_eq!(native.next_window_event().unwrap().unwrap().ordinal(), 2);
    let version = native.session.version();
    drop(
        native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    assert_eq!(native.session.version(), version);
    assert!(native.bridge.callback_boundary_pending());
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .is_none()
    );
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    assert_eq!(native.next_window_event().unwrap().unwrap().ordinal(), 2);
    commit_boundary(&mut native);
    native
        .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
        .unwrap()
        .unwrap();
    assert_eq!(native.session.native_input_recorded_count(), recorded + 2);
    assert!(native.next_window_event().unwrap().is_none());
    commit_boundary(&mut native);
}

#[test]
fn modifier_callback_waits_for_core_capacity_before_acknowledgement() {
    let mut native = NativeCoordinator::new_with_storage_limits(
        coordinator_session(),
        NativePointerRoster::Exact(Vec::new()),
        Some(NativeInputStorageLimit::new(
            std::num::NonZeroUsize::new(4).unwrap(),
            1024 * 1024,
        )),
        None,
    )
    .unwrap();
    let (binding, _) = register_roots(&mut native);
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(11), binding)
        .unwrap();
    let motion = super::callback_prefix::pointer_record(&native, binding, 1, false);
    let NativePointerTranslation::Input(input) =
        native.pointer_translator.translate(&motion, |_| None)
    else {
        panic!("the motion supplies an independent pointer edge");
    };
    native
        .session
        .record_native_pointer_sequence([input; 4])
        .unwrap();
    let record = captured_record(
        &native,
        binding,
        2,
        WindowEvent::ModifiersChanged(ModifiersState::SHIFT.into()),
    );
    native
        .bridge
        .push_record(HostRecord::WindowEvent(record.clone()));
    let recorded = native.session.native_input_recorded_count();
    let mut budget = test_budget();
    let allowance = budget.remaining_records();
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut budget)
            .unwrap()
            .is_none()
    );
    assert_eq!(native.next_window_event().unwrap(), Some(record));
    assert_eq!(native.session.native_input_recorded_count(), recorded);
    assert_eq!(budget.remaining_records(), allowance);
    commit_boundary(&mut native);
    native
        .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
        .unwrap()
        .unwrap();
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    assert!(native.next_window_event().unwrap().is_none());
    commit_boundary(&mut native);
}

#[test]
fn same_batch_press_modifier_move_and_release_choose_the_owner_during_reduction() {
    for shift in [false, true] {
        let (mut native, _, source_binding) = presented_routes();
        let (receiver, source, target) = source_and_target(&mut native);
        let recorded = native.session.native_input_recorded_count();
        record_pointer(
            &mut native,
            source_binding,
            1,
            mouse_event(source, Some(ElementState::Pressed), 22, None),
        );
        assert!(!native.session.has_active_gesture());
        let modifiers = if shift {
            ModifiersState::SHIFT
        } else {
            ModifiersState::empty()
        };
        let control = captured_record(
            &native,
            source_binding,
            2,
            WindowEvent::ModifiersChanged(modifiers.into()),
        );
        native.reduce_non_pointer_event(&control).unwrap();
        for (ordinal, state) in [(3, None), (4, Some(ElementState::Released))] {
            record_pointer(
                &mut native,
                source_binding,
                ordinal,
                mouse_event(target, state, 11, None),
            );
        }
        assert_eq!(native.session.native_input_recorded_count(), recorded + 4);
        let version = native.session.version();
        let mut dropped = native
            .begin_host_frame(|query| resolve(query, &receiver))
            .unwrap();
        let expected = preview(&mut dropped);
        assert!(
            expected.is_none(),
            "a completed release leaves no active preview"
        );
        drop(dropped);
        assert_eq!(native.session.version(), version);
        assert_eq!(native.session.native_input_recorded_count(), recorded + 4);
        let mut retry = native
            .begin_host_frame(|query| resolve(query, &receiver))
            .unwrap();
        assert_eq!(preview(&mut retry), expected);
        let report = finish(retry);
        assert_eq!(report.workspace_changed(), !shift);
        let [terminal] = report.manipulation_feedback() else {
            panic!("one release must publish exactly one result: {report:?}");
        };
        if shift {
            assert!(matches!(
                terminal.status(),
                Status::Rejected(_) | Status::Cancelled(_)
            ));
            assert_eq!(native.session.version(), version);
            assert_eq!(
                native
                    .session
                    .view()
                    .item(ItemId::new(2))
                    .unwrap()
                    .surface(),
                SECOND_SURFACE
            );
        } else {
            assert_eq!(terminal.status(), Status::Delivered(Delivery::Dock));
            assert_ne!(native.session.version(), version);
            assert_eq!(
                native
                    .session
                    .view()
                    .item(ItemId::new(2))
                    .unwrap()
                    .surface(),
                FIRST_SURFACE
            );
        }
        assert!(!native.session.has_pending_native_input());
        assert!(!native.session.has_active_gesture());
        assert!(!native.session.has_pending_presentation());
        assert_eq!(native.session.native_input_recorded_count(), recorded + 4);
        let committed = native.session.version();
        let repeated = commit_boundary(&mut native);
        assert!(repeated.manipulation_feedback().is_empty());
        assert!(!repeated.workspace_changed());
        assert_eq!(native.session.version(), committed);
    }
}

#[test]
fn committed_release_survives_late_shift_and_a_dropped_successor_output() {
    let (mut native, _, binding) = presented_routes();
    let (receiver, source, target) = source_and_target(&mut native);
    for (ordinal, point, state, hover) in [
        (1, source, Some(ElementState::Pressed), 22),
        (2, target, None, 11),
        (3, target, Some(ElementState::Released), 11),
    ] {
        record_pointer(
            &mut native,
            binding,
            ordinal,
            mouse_event(point, state, hover, None),
        );
    }
    let version = native.session.version();
    let mut release = native
        .begin_host_frame(|query| resolve(query, &receiver))
        .unwrap();
    assert!(preview(&mut release).is_none());
    let receipt = release
        .frame
        .paint_plan(FIRST_SURFACE)
        .unwrap()
        .unwrap()
        .execute_surface_program(|_| Ok::<(), ()>(()))
        .unwrap();
    release.frame.confirm_surface_painted(receipt).unwrap();
    let mut released = finish(release);
    let [terminal] = released.manipulation_feedback() else {
        panic!("the successful release must deliver once: {released:?}");
    };
    assert_eq!(terminal.status(), Status::Delivered(Delivery::Dock));
    assert!(released.workspace_changed());
    assert_ne!(native.session.version(), version);
    let committed = native.session.version();
    assert_eq!(
        native
            .session
            .view()
            .item(ItemId::new(2))
            .unwrap()
            .surface(),
        FIRST_SURFACE
    );
    assert!(!native.session.has_pending_presentation());
    assert!(!native.session.has_active_gesture());
    let [output] = released.take_painted_outputs().try_into().unwrap();
    let artifact = output.presentation_artifact();
    let late_shift = captured_record(
        &native,
        binding,
        4,
        WindowEvent::ModifiersChanged(ModifiersState::SHIFT.into()),
    );
    native
        .bridge
        .push_record(HostRecord::WindowEvent(late_shift));
    native
        .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
        .unwrap()
        .unwrap();
    let mut frame = native
        .begin_host_frame(|query| resolve(query, &receiver))
        .unwrap();
    assert!(preview(&mut frame).is_none());
    let late = finish(frame);
    assert!(!late.workspace_changed());
    assert!(late.manipulation_feedback().is_empty());
    native
        .session
        .report_surface_presentation(output, SurfacePresentationResult::Dropped)
        .unwrap();
    let settled = commit_boundary(&mut native);
    assert_eq!(
        settled
            .surface_presentation_commits()
            .iter()
            .filter(|commit| commit.settles(artifact))
            .count(),
        1
    );
    assert!(settled.manipulation_feedback().is_empty());
    assert!(!settled.workspace_changed());
    assert_eq!(native.session.version(), committed);
    assert_eq!(
        native
            .session
            .view()
            .item(ItemId::new(2))
            .unwrap()
            .surface(),
        FIRST_SURFACE
    );
    assert!(!native.session.has_pending_presentation());
}

#[test]
fn stale_modifier_binding_cannot_suspend_the_successor_gesture() {
    let (mut native, _, predecessor) = presented_routes();
    let stale = captured_record(
        &native,
        predecessor,
        10,
        WindowEvent::ModifiersChanged(ModifiersState::SHIFT.into()),
    );
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            FIRST_SURFACE,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1)]),
            ),
        ),
        DockspaceSurfaceLayout::new(
            SECOND_SURFACE,
            DockspaceRootLayout::new(RootId::new(2), DockspaceNode::tabs([ItemId::new(2)])),
        ),
    ])
    .unwrap();
    let reset = native.session.prepare_layout_reset(layout).unwrap();
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.frame.submit_layout_reset(&reset).unwrap();
    let report = finish(frame);
    let successor = report
        .native_bindings()
        .iter()
        .copied()
        .find(|binding| binding.surface() == SECOND_SURFACE)
        .unwrap();
    assert_ne!(successor, predecessor);
    assert_eq!(successor.window_token(), predecessor.window_token());
    let observations = report
        .native_bindings()
        .iter()
        .map(|binding| {
            let x = if binding.surface() == FIRST_SURFACE {
                0.0
            } else {
                720.0
            };
            (*binding, ready_window_facts(x))
        })
        .collect::<Vec<_>>();
    native.settle_host_frame_report(&report).unwrap();
    native
        .report_snapshot(
            observations,
            NativeWorkAreaRoster::Unknown,
            crate::capabilities::capabilities_for_backend(eframe::NativeWindowingBackend::X11),
        )
        .unwrap();
    commit_boundary(&mut native);
    paint_and_present_all_surfaces(&mut native);
    let (receiver, source, target) = source_and_target(&mut native);
    for (ordinal, point, state, hover) in [
        (1, source, Some(ElementState::Pressed), 22),
        (2, target, None, 11),
    ] {
        record_pointer(
            &mut native,
            successor,
            ordinal,
            mouse_event(point, state, hover, None),
        );
    }
    let mut dragging = native
        .begin_host_frame(|query| resolve(query, &receiver))
        .unwrap();
    let expected = preview(&mut dragging);
    assert!(matches!(
        expected,
        Some(DockspacePreviewVisual::Dock {
            surface: FIRST_SURFACE,
            ..
        })
    ));
    finish(dragging);
    assert!(native.session.has_active_gesture());
    assert_eq!(stale.binding(), Some(predecessor));
    let recorded = native.session.native_input_recorded_count();
    native.bridge.push_record(HostRecord::WindowEvent(stale));
    native
        .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
        .unwrap()
        .unwrap();
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    let mut frame = native
        .begin_host_frame(|query| resolve(query, &receiver))
        .unwrap();
    assert_eq!(preview(&mut frame), expected);
    finish(frame);
    assert!(native.session.has_active_gesture());
    assert!(native.session.is_current_native_binding(successor));
}

#[test]
fn shift_keeps_direct_guide_targeting_available() {
    let (mut native, _, binding) = presented_routes();
    let (receiver, source, _) = source_and_target(&mut native);
    let mut inspection = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let mut guide = None;
    inspection
        .frame
        .paint_plan(FIRST_SURFACE)
        .unwrap()
        .unwrap()
        .visit_surface_program(|step| {
            if let SurfacePaintProgramStep::DropGuide { structural, .. } = step
                && let Some(center) = structural.targets().find(|target| {
                    target.direction() == dockspace::runtime::DockspaceDropDirection::Center
                })
            {
                let bounds = center.hit_bounds();
                guide = Some(PhysicalPosition::new(
                    bounds.x() + bounds.width() * 0.5,
                    bounds.y() + bounds.height() * 0.5,
                ));
            }
            Ok::<(), ()>(())
        })
        .unwrap();
    let guide = guide.expect("the central pane publishes its explicit center guide");
    finish(inspection);
    for (ordinal, point, state, hover) in [
        (1, source, Some(ElementState::Pressed), 22),
        (2, guide, None, 11),
        (3, guide, Some(ElementState::Released), 11),
    ] {
        record_pointer(
            &mut native,
            binding,
            ordinal,
            mouse_event(point, state, hover, Some(ModifiersState::SHIFT)),
        );
    }
    let mut frame = native
        .begin_host_frame(|query| resolve(query, &receiver))
        .unwrap();
    assert!(preview(&mut frame).is_none());
    let released = finish(frame);
    let [terminal] = released.manipulation_feedback() else {
        panic!("the explicit guide must deliver despite Shift: {released:?}");
    };
    assert_eq!(terminal.status(), Status::Delivered(Delivery::Dock));
    assert!(released.workspace_changed());
    assert_eq!(
        native
            .session
            .view()
            .item(ItemId::new(2))
            .unwrap()
            .surface(),
        FIRST_SURFACE
    );
    assert!(!native.session.has_pending_presentation());
    assert!(!native.session.has_active_gesture());
    let version = native.session.version();
    let settled = commit_boundary(&mut native);
    assert!(settled.manipulation_feedback().is_empty());
    assert!(!settled.workspace_changed());
    assert_eq!(native.session.version(), version);
}

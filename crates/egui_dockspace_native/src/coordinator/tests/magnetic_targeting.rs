use std::time::Duration;

use dockspace::geometry::LogicalPoint;
use dockspace::runtime::{
    DockspacePreviewVisual, DockspaceReceiverDescriptor, NativeInputStorageLimit,
    NativeReceiverPurpose,
};
use winit::dpi::PhysicalPosition;
use winit::event::{DeviceId, ElementState, MouseButton, PointerEventFacts, PointerWindowRoute};
use winit::keyboard::ModifiersState;

use super::callback_prefix::commit_boundary;
use super::*;

fn captured_record(
    native: &NativeCoordinator,
    binding: NativeSurfaceBinding,
    ordinal: u64,
    event: WindowEvent,
) -> NativeWindowEventRecord {
    let viewport = native.surface_viewport(binding.surface()).unwrap();
    let record = native.bridge.with_viewports(|routes| {
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

fn source_and_target(
    native: &mut NativeCoordinator,
) -> (
    DockspaceReceiverDescriptor,
    PhysicalPosition<f64>,
    PhysicalPosition<f64>,
) {
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
    (receiver, source, target)
}

fn resolve(
    query: NativeReceiverQuery,
    source: &DockspaceReceiverDescriptor,
) -> NativeReceiverAnswer {
    if matches!(query.purpose(), NativeReceiverPurpose::HoverHit) {
        NativeReceiverAnswer::ClearHoverLane(query.presented_surface())
    } else {
        query
            .bind_receiver(source)
            .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock)
    }
}

fn preview(frame: &mut NativeHostFrame<'_>) -> Option<DockspacePreviewVisual> {
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
        assert_eq!(
            matches!(
                expected,
                Some(DockspacePreviewVisual::Dock {
                    surface: FIRST_SURFACE,
                    ..
                })
            ),
            !shift,
            "missing modifiers on subsequent real edges and fresh queries must preserve Shift"
        );
        drop(dropped);
        assert_eq!(native.session.version(), version);
        assert_eq!(native.session.native_input_recorded_count(), recorded + 4);
        let mut retry = native
            .begin_host_frame(|query| resolve(query, &receiver))
            .unwrap();
        assert_eq!(preview(&mut retry), expected);
        let report = finish(retry);
        assert!(!report.workspace_changed());
        assert!(!native.session.has_pending_native_input());
    }
}

#[test]
fn pending_release_keeps_its_dock_preview_after_late_shift() {
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
    let mut release = native
        .begin_host_frame(|query| resolve(query, &receiver))
        .unwrap();
    let frozen = preview(&mut release);
    assert!(matches!(
        frozen,
        Some(DockspacePreviewVisual::Dock {
            surface: FIRST_SURFACE,
            ..
        })
    ));
    finish(release);
    assert!(native.session.has_pending_presentation());
    assert!(!native.session.has_active_gesture());
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
    assert_eq!(preview(&mut frame), frozen);
    assert!(!finish(frame).workspace_changed());
    assert!(native.session.has_pending_presentation());
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
    assert!(matches!(
        preview(&mut frame),
        Some(DockspacePreviewVisual::Dock {
            surface: FIRST_SURFACE,
            ..
        })
    ));
    finish(frame);
    assert!(native.session.has_pending_presentation());
}

use super::*;
use winit::dpi::PhysicalPosition;
use winit::event::{DeviceId, ElementState, MouseButton, PointerEventFacts, PointerWindowRoute};

pub(super) fn pointer_record(
    native: &NativeCoordinator,
    binding: NativeSurfaceBinding,
    ordinal: u64,
    pressed: bool,
) -> NativeWindowEventRecord {
    let position = PhysicalPosition::new(40.0 + ordinal as f64, 70.0);
    pointer_record_at(native, binding, ordinal, pressed, position)
}

fn pointer_record_at(
    native: &NativeCoordinator,
    binding: NativeSurfaceBinding,
    ordinal: u64,
    pressed: bool,
    position: PhysicalPosition<f64>,
) -> NativeWindowEventRecord {
    let window = WindowId::from(11);
    let facts = PointerEventFacts {
        surface_position: Some(position),
        desktop_position: Some(position),
        modifiers: None,
        hover: PointerWindowRoute::Window(window),
        capture: PointerWindowRoute::Window(window),
    };
    let event = if pressed {
        WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: ElementState::Pressed,
            button: MouseButton::Left,
            facts,
        }
    } else {
        WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position,
            facts,
        }
    };
    let record = native.bridge.with_viewports(|routes| {
        NativeWindowEventRecord::for_test_snapshot(
            ordinal,
            window,
            Some(ViewportId::ROOT),
            event,
            routes,
        )
    });
    assert_eq!(record.binding(), Some(binding));
    record
}

pub(super) fn commit_boundary(native: &mut NativeCoordinator) {
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
}

#[test]
fn active_motion_prefix_preserves_256_records_and_retries_before_its_successor() {
    let mut native = coordinator();
    let (binding, _) = register_roots(&mut native);
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(11), binding)
        .unwrap();
    native
        .bridge
        .push_record(HostRecord::WindowEvent(pointer_record(
            &native, binding, 1, true,
        )));
    assert_eq!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .unwrap()
            .get(),
        1
    );
    commit_boundary(&mut native);
    assert!(!native.pointer_translator.can_coalesce_idle_cursor_moves());

    for ordinal in 2..=258 {
        native
            .bridge
            .push_record(HostRecord::WindowEvent(pointer_record(
                &native, binding, ordinal, false,
            )));
    }
    assert_eq!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .unwrap()
            .get(),
        256
    );
    assert_eq!(native.next_window_event().unwrap().unwrap().ordinal(), 258);
    assert!(native.bridge.callback_boundary_pending());

    // Dropping the candidate keeps the exact semantic prefix for retry and
    // cannot translate another raw record into that pending boundary.
    let frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    drop(frame);
    assert!(
        !native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .is_some()
    );
    assert_eq!(native.next_window_event().unwrap().unwrap().ordinal(), 258);
    commit_boundary(&mut native);
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .is_some()
    );
    assert!(native.next_window_event().unwrap().is_none());
    commit_boundary(&mut native);
    assert!(!native.bridge.callback_boundary_pending());
}

#[test]
fn active_motion_prefix_stops_before_a_focus_barrier_and_keeps_the_tail() {
    let mut native = coordinator();
    let (binding, _) = register_roots(&mut native);
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(11), binding)
        .unwrap();
    native
        .bridge
        .push_record(HostRecord::WindowEvent(pointer_record(
            &native, binding, 1, true,
        )));
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .is_some()
    );
    commit_boundary(&mut native);
    for ordinal in 2..=5 {
        native
            .bridge
            .push_record(HostRecord::WindowEvent(pointer_record(
                &native, binding, ordinal, false,
            )));
    }
    native
        .bridge
        .push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            6,
            WindowId::from(11),
            Some(ViewportId::ROOT),
            Some(binding),
            WindowEvent::Focused(false),
        )));
    native
        .bridge
        .push_record(HostRecord::WindowEvent(pointer_record(
            &native, binding, 7, false,
        )));
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .is_some()
    );
    assert_eq!(native.next_window_event().unwrap().unwrap().ordinal(), 6);
    commit_boundary(&mut native);
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .is_some()
    );
    assert_eq!(native.next_window_event().unwrap().unwrap().ordinal(), 7);
    commit_boundary(&mut native);
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .is_some()
    );
    assert!(native.next_window_event().unwrap().is_none());
    commit_boundary(&mut native);
}

#[test]
fn dropping_an_unretained_cursor_claim_leaves_its_exact_records_available() {
    let mut native = coordinator();
    let (binding, _) = register_roots(&mut native);
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(11), binding)
        .unwrap();
    for ordinal in 1..=3 {
        native
            .bridge
            .push_record(HostRecord::WindowEvent(pointer_record(
                &native, binding, ordinal, false,
            )));
    }
    let first = native.bridge.claim_cursor_prefix(2).unwrap();
    assert_eq!(
        first
            .events()
            .map(NativeWindowEventRecord::ordinal)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    drop(first);
    assert!(!native.bridge.callback_boundary_pending());
    let retry = native.bridge.claim_cursor_prefix(2).unwrap();
    assert_eq!(
        retry
            .events()
            .map(NativeWindowEventRecord::ordinal)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    let count = retry.record_count();
    assert!(retry.retain_prefix(count));
    assert!(native.bridge.callback_boundary_pending());
    assert!(native.bridge.claim_cursor_prefix(2).is_none());
    assert_eq!(native.next_window_event().unwrap().unwrap().ordinal(), 3);
}

#[test]
fn translation_deadline_retains_only_the_translated_part_of_a_cursor_claim() {
    use dockspace::runtime::NativeMonotonicClock;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    let mut native = coordinator();
    let (binding, _) = register_roots(&mut native);
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(11), binding)
        .unwrap();
    native
        .bridge
        .push_record(HostRecord::WindowEvent(pointer_record(
            &native, binding, 1, true,
        )));
    native
        .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
        .unwrap();
    commit_boundary(&mut native);
    for ordinal in 2..=4 {
        native
            .bridge
            .push_record(HostRecord::WindowEvent(pointer_record(
                &native, binding, ordinal, false,
            )));
    }
    let calls = Arc::new(AtomicU64::new(0));
    let source = Arc::clone(&calls);
    let clock = NativeMonotonicClock::new(move || {
        if source.fetch_add(1, Ordering::SeqCst) < 4 {
            Duration::ZERO
        } else {
            Duration::from_millis(2)
        }
    });
    let mut budget = NativeInputBudget::new(&clock);
    assert_eq!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut budget)
            .unwrap()
            .unwrap()
            .get(),
        1
    );
    assert!(budget.is_exhausted());
    assert_eq!(native.next_window_event().unwrap().unwrap().ordinal(), 3);
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut budget)
            .unwrap()
            .is_none()
    );
    assert_eq!(native.next_window_event().unwrap().unwrap().ordinal(), 3);
    commit_boundary(&mut native);
    assert_eq!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .unwrap()
            .get(),
        2
    );
    assert!(native.next_window_event().unwrap().is_none());
    commit_boundary(&mut native);
}

#[test]
fn foreign_cursor_prefix_rejection_keeps_raw_input_and_translator_unpublished() {
    let mut native = coordinator();
    let (current, _) = register_roots(&mut native);
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(11), current)
        .unwrap();
    native
        .bridge
        .push_record(HostRecord::WindowEvent(pointer_record(
            &native, current, 1, true,
        )));
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .is_some()
    );
    commit_boundary(&mut native);
    let mut foreign = coordinator();
    let (binding, _) = register_roots(&mut foreign);
    for ordinal in 2..=3 {
        native
            .bridge
            .push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
                ordinal,
                WindowId::from(11),
                Some(ViewportId::ROOT),
                Some(binding),
                WindowEvent::CursorMoved {
                    device_id: DeviceId::dummy(),
                    position: PhysicalPosition::new(100.0, 70.0),
                    facts: PointerEventFacts::default(),
                },
            )));
    }
    for _ in 0..2 {
        let error = native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap_err();
        assert_eq!(error.kind(), crate::NativeRuntimeErrorKind::HostProtocol);
        assert!(!native.bridge.callback_boundary_pending());
        assert!(!native.pointer_translator.can_coalesce_idle_cursor_moves());
        assert_eq!(native.next_window_event().unwrap().unwrap().ordinal(), 2);
    }
}

#[test]
fn active_drag_replays_every_cursor_point_identically_after_a_dropped_frame() {
    use dockspace::geometry::LogicalPoint;
    use dockspace::runtime::NativeReceiverPurpose;

    let mut native = coordinator();
    let (binding, second) = register_roots(&mut native);
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(11), binding)
        .unwrap();
    native
        .report_snapshot(
            [
                (binding, ready_window_facts(0.0)),
                (second, ready_window_facts(720.0)),
            ],
            NativeWorkAreaRoster::Unknown,
            test_managed_capabilities(),
        )
        .unwrap();
    commit_boundary(&mut native);
    paint_and_present_all_surfaces(&mut native);
    let mut inspection = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let source = inspection
        .frame
        .paint_plan(FIRST_SURFACE)
        .unwrap()
        .unwrap()
        .tab_receiver(ItemId::new(1))
        .unwrap();
    inspection
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    inspection.commit().unwrap();
    let start = source.center();
    native
        .bridge
        .push_record(HostRecord::WindowEvent(pointer_record_at(
            &native,
            binding,
            1,
            true,
            PhysicalPosition::new(start.x(), start.y()),
        )));
    assert_eq!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .unwrap()
            .get(),
        1
    );
    let mut press = native
        .begin_host_frame(|query| {
            query
                .bind_receiver(&source)
                .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock)
        })
        .unwrap();
    press
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    press.commit().unwrap();
    assert!(native.session.has_active_gesture());
    let version = native.session.version();
    let expected: Vec<_> = (1..=256)
        .map(|index| LogicalPoint::new(start.x() + 24.0 + f64::from(index) * 0.25, 120.0).unwrap())
        .collect();
    for (ordinal, point) in (2..).zip(&expected) {
        native
            .bridge
            .push_record(HostRecord::WindowEvent(pointer_record_at(
                &native,
                binding,
                ordinal,
                false,
                PhysicalPosition::new(point.x(), point.y()),
            )));
    }
    assert_eq!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .unwrap()
            .get(),
        256
    );
    assert!(native.next_window_event().unwrap().is_none());
    for should_commit in [false, true] {
        let mut observed = Vec::new();
        let mut frame = native
            .begin_host_frame(|query| {
                assert_eq!(query.purpose(), &NativeReceiverPurpose::HoverHit);
                assert_eq!(query.surface(), FIRST_SURFACE);
                observed.push(query.point().unwrap());
                NativeReceiverAnswer::NoReceiver(query.presented_surface())
            })
            .unwrap();
        assert_eq!(
            observed, expected,
            "every semantic cursor edge preserves its input point and order"
        );
        if should_commit {
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            frame.commit().unwrap();
        } else {
            drop(frame);
        }
        assert_eq!(native.session.version(), version);
        assert!(native.session.has_active_gesture());
    }
    assert!(!native.bridge.callback_boundary_pending());
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .is_none()
    );
}

use super::callback_prefix::{commit_boundary, pointer_record};
use super::*;
use dockspace::runtime::NativeInputStorageLimit;
use std::num::NonZeroUsize;

fn bounded_coordinator(slots: usize) -> (NativeCoordinator, NativeSurfaceBinding) {
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
    let session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let mut native = NativeCoordinator::new_with_storage_limits(
        session,
        NativePointerRoster::Exact(Vec::new()),
        Some(NativeInputStorageLimit::new(
            NonZeroUsize::new(slots).unwrap(),
            1024 * 1024,
        )),
        None,
    )
    .unwrap();
    let (binding, _) = register_roots(&mut native);
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(11), binding)
        .unwrap();
    assert!(!native.session.has_pending_native_input());
    (native, binding)
}

#[test]
fn selected_host_allowance_drains_every_accepted_contact_before_one_retryable_cancellation() {
    use crate::mailbox::NativeDispatch;
    use winit::dpi::PhysicalPosition;
    use winit::event::{DeviceId, Touch, TouchPhase};

    let (mut native, binding) = bounded_coordinator(4);
    let baseline = native.bridge.owned_input_capacity_bytes(None);
    let core_allowance = 1024 * 1024;
    let growth = NativePointerTranslator::callback_translation_capacity_bytes();
    let host_allowance = baseline
        + native
            .pointer_translator
            .reserved_translation_capacity_bytes()
        + 32 * growth;
    assert!(
        native
            .bridge
            .configure_input_storage_allowance(host_allowance + core_allowance, core_allowance,)
    );
    for ordinal in 1..=128 {
        native
            .bridge
            .push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
                ordinal,
                WindowId::from(11),
                Some(ViewportId::ROOT),
                Some(binding),
                WindowEvent::Touch(Touch {
                    device_id: DeviceId::dummy(),
                    phase: TouchPhase::Started,
                    location: PhysicalPosition::new(10.0, 20.0),
                    force: None,
                    id: ordinal,
                }),
            )));
    }
    let boundary = native.bridge.freeze_ordinary_for_test();
    assert!((1..128).contains(&boundary));
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::ContinueBoundary
    );
    commit_boundary(&mut native);
    let recorded = native.session.native_input_recorded_count();
    let context = egui::Context::default();
    for consumed in 1..=boundary {
        assert_eq!(
            native.next_window_event().unwrap().unwrap().ordinal(),
            consumed
        );
        assert_eq!(
            native
                .reduce_callback_prefix(&context, &mut test_budget())
                .unwrap(),
            Some(NonZeroUsize::MIN)
        );
        assert_eq!(
            native.session.native_input_recorded_count(),
            recorded + consumed
        );
        let reserved = native
            .pointer_translator
            .reserved_translation_capacity_bytes()
            + (boundary - consumed) as usize * growth;
        assert!(baseline + reserved <= host_allowance);
        let retained = format!("{:?}", native.pointer_translator);
        let version = native.session.version();
        let mut frame = native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        if consumed == 1 {
            drop(frame);
            assert_eq!(native.session.version(), version);
            assert_eq!(format!("{:?}", native.pointer_translator), retained);
            assert!(
                native
                    .reduce_callback_prefix(&context, &mut test_budget())
                    .unwrap()
                    .is_none()
            );
        } else {
            let report = frame.commit().unwrap();
            assert!(
                !report
                    .inputs()
                    .contains(&HostInputOutcome::NativeInputCancelledForOverload)
            );
            continue;
        }
        commit_boundary(&mut native);
    }
    assert!(native.next_window_event().unwrap().is_none());
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::Cancel { boundary }
    );
    assert!(!native.pointer_translator.can_coalesce_idle_cursor_moves());
    let retained = format!("{:?}", native.pointer_translator);
    let mut cancelled = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    cancelled
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    drop(cancelled);
    assert_eq!(format!("{:?}", native.pointer_translator), retained);
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::Cancel { boundary }
    );
    let mut cancelled = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    cancelled
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = cancelled.commit().unwrap();
    assert_eq!(
        report
            .inputs()
            .iter()
            .filter(|outcome| matches!(outcome, HostInputOutcome::NativeInputCancelledForOverload))
            .count(),
        1
    );
    assert!(native.pointer_translator.can_coalesce_idle_cursor_moves());
    assert_eq!(
        native.pointer_translator.owned_data_capacity_bytes(),
        size_of::<NativePointerTranslator>()
    );
    assert_eq!(
        native.session.native_input_recorded_count(),
        recorded + boundary
    );
}

#[test]
fn bounded_cursor_prefix_clips_yields_when_full_and_resumes_the_same_stream() {
    let (mut native, binding) = bounded_coordinator(4);
    assert_eq!(
        native.session.prepare_native_pointer_capacity().unwrap(),
        Some(4)
    );

    // Activate the real translator before testing capacity, so idle coalescing
    // cannot hide an incorrectly consumed cursor suffix.
    for (ordinal, pressed) in [(1, true), (2, false)] {
        native
            .bridge
            .push_record(HostRecord::WindowEvent(pointer_record(
                &native, binding, ordinal, pressed,
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
    }
    assert!(!native.pointer_translator.can_coalesce_idle_cursor_moves());
    let movement_record = pointer_record(&native, binding, 2, false);
    let NativePointerTranslation::Input(movement) = native
        .pointer_translator
        .clone()
        .translate(&movement_record, |_| None)
    else {
        panic!("the pressed pointer must retain its movement stream");
    };

    native
        .session
        .record_native_pointer_sequence([movement; 2])
        .unwrap();
    assert_eq!(
        native.session.prepare_native_pointer_capacity().unwrap(),
        Some(2)
    );
    for ordinal in 3..=6 {
        native
            .bridge
            .push_record(HostRecord::WindowEvent(pointer_record(
                &native, binding, ordinal, false,
            )));
    }
    let recorded = native.session.native_input_recorded_count();
    assert_eq!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .unwrap()
            .get(),
        2
    );
    assert_eq!(native.session.native_input_recorded_count(), recorded + 2);
    assert_eq!(native.next_window_event().unwrap().unwrap().ordinal(), 5);
    assert!(native.bridge.callback_boundary_pending());
    assert!(native.session.has_pending_native_input());
    assert_eq!(
        native.session.prepare_native_pointer_capacity().unwrap(),
        Some(0)
    );
    commit_boundary(&mut native);
    assert!(!native.session.has_pending_native_input());
    assert_eq!(
        native.session.prepare_native_pointer_capacity().unwrap(),
        Some(4)
    );

    // Fill core independently while the exact raw suffix still belongs to the bridge.
    native
        .session
        .record_native_pointer_sequence([movement; 4])
        .unwrap();
    assert!(!native.bridge.callback_boundary_pending());
    let recorded = native.session.native_input_recorded_count();
    let version = native.session.version();
    let translator = format!("{:?}", native.pointer_translator);
    let mut budget = test_budget();
    let allowance = budget.remaining_records();
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut budget)
            .unwrap()
            .is_none()
    );
    assert!(native.bridge.callback_boundary_pending());
    assert!(native.session.has_pending_native_input());
    assert_eq!(native.session.native_input_recorded_count(), recorded);
    assert_eq!(native.session.version(), version);
    assert_eq!(budget.remaining_records(), allowance);
    assert_eq!(native.next_window_event().unwrap().unwrap().ordinal(), 5);
    assert_eq!(format!("{:?}", native.pointer_translator), translator);

    commit_boundary(&mut native);
    assert!(!native.session.has_pending_native_input());
    assert!(!native.bridge.callback_boundary_pending());
    assert_eq!(
        native.session.prepare_native_pointer_capacity().unwrap(),
        Some(4)
    );
    assert_eq!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .unwrap()
            .get(),
        2
    );
    assert_eq!(native.session.native_input_recorded_count(), recorded + 2);
    assert!(native.next_window_event().unwrap().is_none());
    assert_eq!(
        native
            .pointer_translator
            .clone()
            .translate(&movement_record, |_| None),
        NativePointerTranslation::Input(movement),
        "reclaiming transport must preserve the original pressed pointer identity"
    );
    commit_boundary(&mut native);
    assert!(!native.session.has_pending_native_input());
    assert!(!native.bridge.callback_boundary_pending());
}

#[test]
fn five_pressed_devices_retire_with_one_fact_in_four_slots_and_replay_a_dropped_frame() {
    let (mut native, binding) = bounded_coordinator(4);
    for device in 1..=5 {
        let press = pointer_record(&native, binding, device, true);
        let NativePointerTranslation::Input(input) = native
            .pointer_translator
            .translate_for_test_device(&press, device, |_| None)
        else {
            panic!("each independent pressed device must produce a live stream");
        };
        native.session.record_native_pointer(input).unwrap();
        commit_boundary(&mut native);
    }
    // Capture through the physical receipt; a detached synthetic record cannot
    // authorize binding retirement or own its once-only retry progress.
    let destroyed = NativeWindowEventRecord::for_test(
        100,
        WindowId::from(11),
        Some(ViewportId::ROOT),
        Some(binding),
        WindowEvent::Destroyed,
    );
    native
        .bridge
        .push_record(HostRecord::WindowEvent(destroyed));
    let recorded = native.session.native_input_recorded_count();
    let version = native.session.version();
    assert!(!native.session.has_pending_native_input());
    assert_eq!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap(),
        Some(NonZeroUsize::MIN)
    );
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    assert_eq!(
        native.session.prepare_native_pointer_capacity().unwrap(),
        Some(3)
    );
    assert!(native.session.has_pending_native_input());
    assert!(native.bridge.callback_boundary_pending());
    assert!(native.next_window_event().unwrap().is_none());
    assert!(!native.pointer_translator.references_binding(binding));
    let translator = format!("{:?}", native.pointer_translator);

    {
        let mut frame = native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
    }
    assert_eq!(native.session.version(), version);
    assert!(native.session.has_pending_native_input());
    assert!(native.bridge.retirement_destroyed_observations().is_empty());
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .is_none()
    );
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    assert_eq!(format!("{:?}", native.pointer_translator), translator);

    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert_eq!(
        report.inputs(),
        [HostInputOutcome::NativeInputBindingRetired { binding }],
        "one backend fact replaces every per-pointer cancellation and reset"
    );
    assert!(!native.session.has_pending_native_input());
    assert!(!native.bridge.callback_boundary_pending());
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    assert_eq!(
        native.session.prepare_native_pointer_capacity().unwrap(),
        Some(4)
    );
    assert_eq!(native.bridge.retirement_destroyed_observations().len(), 1);
}

#[test]
fn full_transport_keeps_destroyed_callback_and_translator_retryable_until_capacity_commits() {
    let (mut native, binding) = bounded_coordinator(4);
    let press = pointer_record(&native, binding, 1, true);
    let NativePointerTranslation::Input(input) =
        native.pointer_translator.translate(&press, |_| None)
    else {
        panic!("the pressed pointer must produce a live stream");
    };
    native.session.record_native_pointer(input).unwrap();
    commit_boundary(&mut native);

    let movement_record = pointer_record(&native, binding, 2, false);
    let NativePointerTranslation::Input(movement) = native
        .pointer_translator
        .clone()
        .translate(&movement_record, |_| None)
    else {
        panic!("the live pointer must retain its movement stream");
    };
    native
        .session
        .record_native_pointer_sequence([movement; 4])
        .unwrap();
    native
        .bridge
        .push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            3,
            WindowId::from(11),
            Some(ViewportId::ROOT),
            Some(binding),
            WindowEvent::Destroyed,
        )));
    let recorded = native.session.native_input_recorded_count();
    let version = native.session.version();
    let translator = format!("{:?}", native.pointer_translator);
    let mut budget = test_budget();
    let allowance = budget.remaining_records();
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut budget)
            .unwrap()
            .is_none()
    );
    assert!(native.bridge.callback_boundary_pending());
    assert_eq!(native.next_window_event().unwrap().unwrap().ordinal(), 3);
    assert_eq!(native.session.native_input_recorded_count(), recorded);
    assert_eq!(native.session.version(), version);
    assert_eq!(format!("{:?}", native.pointer_translator), translator);
    assert_eq!(budget.remaining_records(), allowance);
    assert!(native.bridge.retirement_destroyed_observations().is_empty());

    commit_boundary(&mut native);
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .is_some()
    );
    assert!(native.next_window_event().unwrap().is_none());
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    assert!(!native.pointer_translator.references_binding(binding));
    commit_boundary(&mut native);
    assert!(!native.session.has_pending_native_input());
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
}

use dockspace::runtime::{
    NativeDesktopPointerLocation, NativeDesktopPosition, NativePointerButton,
    NativePointerCancelReason, NativePointerEvent, NativePointerHover, NativePointerId,
    NativePointerInput, NativePointerOwner, NativeReceiverAnswer, SurfaceUnavailableReason,
};
use winit::dpi::PhysicalPosition;
use winit::event::{
    DeviceId, ElementState, MouseButton, PointerEventFacts, Touch, TouchPhase, WindowEvent,
};

use super::*;
use crate::event::{NativeDeviceRemovalRecord, NativePointerRouteSnapshot};
use crate::mailbox::HostRecord;

#[test]
fn captured_touch_metadata_uses_the_original_journal_clock_and_contact_class() {
    use dockspace::runtime::{NativeMonotonicClock, PointerDeviceKind, PointerEventMetadata};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    let ticks = Arc::new(AtomicU64::new(0));
    let observed = Arc::clone(&ticks);
    let mut native = coordinator_with_clock(NativeMonotonicClock::new(move || {
        Duration::from_millis(observed.load(Ordering::SeqCst))
    }));
    let (binding, _) = register_roots(&mut native);
    let window = WindowId::from(99);
    native
        .bind_viewport(ViewportId::ROOT, window, binding)
        .unwrap();
    ticks.store(40, Ordering::SeqCst);
    let record = native.bridge.with_viewports(|routes| {
        NativeWindowEventRecord::for_test_snapshot(
            1,
            window,
            Some(ViewportId::ROOT),
            WindowEvent::Touch(Touch {
                device_id: DeviceId::dummy(),
                phase: TouchPhase::Started,
                location: PhysicalPosition::new(24.0, 36.0),
                facts: PointerEventFacts::default(),
                force: None,
                id: 15,
            }),
            routes,
        )
    });
    native.bridge.push_record(HostRecord::WindowEvent(record));
    let expected = pointer_input(
        binding,
        NativePointerId::new(1),
        NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
    )
    .with_metadata(PointerEventMetadata::new(
        PointerDeviceKind::Touch,
        Duration::from_millis(40),
    ));
    for now in [100, 200] {
        ticks.store(now, Ordering::SeqCst);
        let (record, captured_at) = native.bridge.front_captured_event().unwrap();
        let mut candidate = native.pointer_translator.clone();
        assert_eq!(
            candidate.translate_captured(&record, captured_at, |_| None),
            NativePointerTranslation::Input(expected)
        );
        assert!(native.pointer_translator.can_coalesce_idle_cursor_moves());
        assert!(!native.bridge.callback_boundary_pending());
    }
    let recorded = native.session.native_input_recorded_count();
    assert_eq!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .unwrap()
            .get(),
        1
    );
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    assert!(native.bridge.front_captured_event().is_none());
    assert!(!native.pointer_translator.can_coalesce_idle_cursor_moves());
    assert!(native.bridge.callback_boundary_pending());
    let version = native.session.version();
    drop(
        native
            .begin_host_frame(|_| panic!("a contact without position grants no receiver authority"))
            .unwrap(),
    );
    assert_eq!(native.session.version(), version);
    assert!(native.bridge.callback_boundary_pending());
    commit_pending_pointer_frame(&mut native);
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    assert!(!native.bridge.callback_boundary_pending());
}

#[test]
fn touch_snapshots_freeze_exact_delivery_and_preserve_terminal_semantics() {
    for (terminal, terminal_event) in [
        (
            TouchPhase::Ended,
            NativePointerEvent::ContactEnded(NativePointerButton::Primary),
        ),
        (
            TouchPhase::Cancelled,
            NativePointerEvent::StreamCancelled(
                NativePointerCancelReason::ExplicitPlatformCancellation,
            ),
        ),
    ] {
        let mut native = coordinator();
        let (binding, _) = register_roots(&mut native);
        let window = WindowId::from(97);
        native
            .bind_viewport(ViewportId::ROOT, window, binding)
            .expect("touch viewport binds");
        let mut translator = NativePointerTranslator::default();

        for (ordinal, phase, event) in [
            (
                1,
                TouchPhase::Started,
                NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            ),
            (2, TouchPhase::Moved, NativePointerEvent::Moved),
            (3, terminal, terminal_event),
        ] {
            let record = native.bridge.with_viewports(|routes| {
                NativeWindowEventRecord::for_test_snapshot(
                    ordinal,
                    window,
                    Some(ViewportId::ROOT),
                    WindowEvent::Touch(Touch {
                        device_id: DeviceId::dummy(),
                        phase,
                        location: PhysicalPosition::new(24.0, 36.0),
                        facts: PointerEventFacts::default(),
                        force: None,
                        id: 13,
                    }),
                    routes,
                )
            });
            assert_eq!(record.binding(), Some(binding));
            let routes = record.pointer_routes().expect("touch routes are frozen");
            assert_eq!(routes.delivery(), NativePointerRouteSnapshot::Dock(binding));
            assert_eq!(routes.hover(), NativePointerRouteSnapshot::Unknown);
            assert_eq!(routes.capture(), NativePointerRouteSnapshot::Unknown);

            let expected = if matches!(phase, TouchPhase::Cancelled) {
                authority_free_pointer_input(NativePointerId::new(1), event)
            } else {
                pointer_input(binding, NativePointerId::new(1), event)
            };
            assert_eq!(
                translator.translate(&record, |_| None),
                NativePointerTranslation::Input(expected)
            );
        }
        assert!(!translator.references_binding(binding));
    }
}

#[test]
fn destroyed_touch_snapshots_never_recover_retained_lifecycle_binding() {
    for (terminal, terminal_event) in [
        (
            TouchPhase::Ended,
            NativePointerEvent::ContactEnded(NativePointerButton::Primary),
        ),
        (
            TouchPhase::Cancelled,
            NativePointerEvent::StreamCancelled(
                NativePointerCancelReason::ExplicitPlatformCancellation,
            ),
        ),
    ] {
        let mut native = coordinator();
        let (binding, _) = register_roots(&mut native);
        let window = WindowId::from(98);
        native
            .bind_viewport(ViewportId::ROOT, window, binding)
            .expect("touch viewport binds");
        let touch = |phase| {
            WindowEvent::Touch(Touch {
                device_id: DeviceId::dummy(),
                phase,
                location: PhysicalPosition::new(24.0, 36.0),
                facts: PointerEventFacts::default(),
                force: None,
                id: 14,
            })
        };
        let started = native.bridge.with_viewports(|routes| {
            NativeWindowEventRecord::for_test_snapshot(
                1,
                window,
                Some(ViewportId::ROOT),
                touch(TouchPhase::Started),
                routes,
            )
        });
        let mut translator = NativePointerTranslator::default();
        assert_eq!(
            translator.translate(&started, |_| None),
            NativePointerTranslation::Input(pointer_input(
                binding,
                NativePointerId::new(1),
                NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            ))
        );

        let destroyed = native.bridge.with_viewports_mut(|routes| {
            let destroyed = NativeWindowEventRecord::for_test_ingress(
                2,
                window,
                Some(ViewportId::ROOT),
                WindowEvent::Destroyed,
                routes,
            );
            assert_eq!(
                routes.binding_for_event(window, Some(ViewportId::ROOT)),
                Some(binding),
                "pointer suppression retains the lifecycle binding"
            );
            assert_eq!(
                routes.pointer_binding_for_event(window, Some(ViewportId::ROOT)),
                None
            );
            destroyed
        });
        assert_eq!(destroyed.binding(), Some(binding));
        assert_eq!(
            started.pointer_routes().unwrap().delivery(),
            NativePointerRouteSnapshot::Dock(binding),
            "suppression cannot rewrite an earlier touch snapshot"
        );

        let snapshot = |ordinal, phase| {
            native.bridge.with_viewports(|routes| {
                NativeWindowEventRecord::for_test_snapshot(
                    ordinal,
                    window,
                    Some(ViewportId::ROOT),
                    touch(phase),
                    routes,
                )
            })
        };
        let moved = snapshot(3, TouchPhase::Moved);
        let terminated = snapshot(4, terminal);
        let late_move = snapshot(5, TouchPhase::Moved);
        let restarted = snapshot(6, TouchPhase::Started);
        for record in [&moved, &terminated, &late_move, &restarted] {
            assert_eq!(record.binding(), Some(binding));
            let routes = record.pointer_routes().expect("touch routes are frozen");
            assert_eq!(routes.delivery(), NativePointerRouteSnapshot::Unknown);
            assert_eq!(routes.hover(), NativePointerRouteSnapshot::Unknown);
            assert_eq!(routes.capture(), NativePointerRouteSnapshot::Unknown);
        }

        let mut retired = translator.clone();
        retired.retire_binding(binding);
        assert!(!retired.references_binding(binding));
        for record in [&moved, &terminated, &late_move] {
            assert_eq!(
                retired.translate(record, |_| None),
                NativePointerTranslation::Ignored,
                "suppressed touch snapshots cannot bypass retired-contact tombstones"
            );
        }
        assert_eq!(
            translator.translate(&moved, |_| None),
            NativePointerTranslation::Ignored
        );
        assert_eq!(
            translator.translate(&terminated, |_| None),
            NativePointerTranslation::Input(authority_free_pointer_input(
                NativePointerId::new(1),
                terminal_event,
            )),
            "a suppressed terminal closes the contact without lifecycle authority"
        );
        assert_eq!(
            translator.translate(&late_move, |_| None),
            NativePointerTranslation::Ignored,
            "a terminal cannot be followed by a resurrected contact"
        );
        for translator in [&mut translator, &mut retired] {
            assert_eq!(
                translator.translate(&restarted, |_| None),
                NativePointerTranslation::Input(authority_free_pointer_input(
                    NativePointerId::new(2),
                    NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
                )),
                "an explicit start gets a fresh identity without recovering lifecycle authority"
            );
            assert!(!translator.references_binding(binding));
        }
    }
}

#[test]
fn mouse_and_touch_contacts_receive_distinct_adapter_pointer_identities() {
    let mut native = coordinator();
    let (binding, _) = register_roots(&mut native);
    let window = WindowId::from(81);
    let device = DeviceId::dummy();
    let records = [
        NativeWindowEventRecord::for_test(
            1,
            window,
            None,
            Some(binding),
            WindowEvent::MouseInput {
                device_id: device,
                state: ElementState::Pressed,
                button: MouseButton::Left,
                facts: PointerEventFacts::default(),
            },
        ),
        touch_record(2, window, binding, device, 11, TouchPhase::Started),
        touch_record(3, window, binding, device, 12, TouchPhase::Started),
    ];
    let mut translator = NativePointerTranslator::default();

    for (record, pointer) in records.iter().zip(1..) {
        assert_eq!(
            translator.translate(record, |_| None),
            NativePointerTranslation::Input(pointer_input(
                binding,
                NativePointerId::new(pointer),
                NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            ))
        );
    }
}

#[test]
fn touch_contact_stays_with_its_origin_and_reused_id_gets_a_fresh_pointer() {
    let mut native = coordinator();
    let (source, target) = register_roots(&mut native);
    let source_window = WindowId::from(91);
    let target_window = WindowId::from(92);
    let device = DeviceId::dummy();
    let mut translator = NativePointerTranslator::default();

    assert_eq!(
        translator.translate(
            &touch_record(1, source_window, source, device, 7, TouchPhase::Started,),
            |_| None,
        ),
        NativePointerTranslation::Input(pointer_input(
            source,
            NativePointerId::new(1),
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
        ))
    );
    assert_eq!(
        translator.translate(
            &touch_record(2, target_window, target, device, 7, TouchPhase::Moved,),
            |_| None,
        ),
        NativePointerTranslation::Ignored,
        "another exact delivery window cannot take over an active contact"
    );
    assert_eq!(
        translator.translate(
            &touch_record(3, source_window, source, device, 7, TouchPhase::Moved,),
            |_| None,
        ),
        NativePointerTranslation::Input(pointer_input(
            source,
            NativePointerId::new(1),
            NativePointerEvent::Moved,
        ))
    );
    assert_eq!(
        translator.translate(
            &touch_record(4, source_window, source, device, 7, TouchPhase::Ended,),
            |_| None,
        ),
        NativePointerTranslation::Input(pointer_input(
            source,
            NativePointerId::new(1),
            NativePointerEvent::ContactEnded(NativePointerButton::Primary),
        ))
    );
    assert_eq!(
        translator.translate(
            &touch_record(5, target_window, target, device, 7, TouchPhase::Started,),
            |_| None,
        ),
        NativePointerTranslation::Input(pointer_input(
            target,
            NativePointerId::new(2),
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
        )),
        "winit touch IDs are reusable only after a fresh adapter identity is minted"
    );
}

#[test]
fn cross_window_touch_end_terminates_without_transferring_authority() {
    let mut native = coordinator();
    let (source, target) = register_roots(&mut native);
    let source_window = WindowId::from(93);
    let target_window = WindowId::from(94);
    let device = DeviceId::dummy();
    let mut translator = NativePointerTranslator::default();

    assert!(matches!(
        translator.translate(
            &touch_record(1, source_window, source, device, 8, TouchPhase::Started),
            |_| None,
        ),
        NativePointerTranslation::Input(_)
    ));
    assert_eq!(
        translator.translate(
            &touch_record(2, target_window, target, device, 8, TouchPhase::Ended),
            |_| None,
        ),
        NativePointerTranslation::Input(authority_free_pointer_input(
            NativePointerId::new(1),
            NativePointerEvent::ContactEnded(NativePointerButton::Primary),
        )),
        "the terminal closes the original contact without granting the callback window authority"
    );
    assert_eq!(
        translator.translate(
            &touch_record(3, target_window, target, device, 8, TouchPhase::Started),
            |_| None,
        ),
        NativePointerTranslation::Input(pointer_input(
            target,
            NativePointerId::new(2),
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
        )),
        "a fail-closed terminal still releases the reusable winit contact ID"
    );
}

#[test]
fn cancelled_touch_terminates_the_contact_before_id_reuse() {
    let mut native = coordinator();
    let (binding, _) = register_roots(&mut native);
    let window = WindowId::from(95);
    let device = DeviceId::dummy();
    let mut translator = NativePointerTranslator::default();

    assert!(matches!(
        translator.translate(
            &touch_record(1, window, binding, device, 9, TouchPhase::Started,),
            |_| None,
        ),
        NativePointerTranslation::Input(_)
    ));
    assert_eq!(
        translator.translate(
            &touch_record(2, window, binding, device, 9, TouchPhase::Cancelled,),
            |_| None,
        ),
        NativePointerTranslation::Input(authority_free_pointer_input(
            NativePointerId::new(1),
            NativePointerEvent::StreamCancelled(
                NativePointerCancelReason::ExplicitPlatformCancellation,
            ),
        ))
    );
    assert_eq!(
        translator.translate(
            &touch_record(3, window, binding, device, 9, TouchPhase::Moved,),
            |_| None,
        ),
        NativePointerTranslation::Ignored,
        "a terminal contact cannot be resurrected by a late move"
    );
    assert_eq!(
        translator.translate(
            &touch_record(4, window, binding, device, 9, TouchPhase::Started,),
            |_| None,
        ),
        NativePointerTranslation::Input(pointer_input(
            binding,
            NativePointerId::new(2),
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
        ))
    );
}

#[test]
fn destroyed_touch_binding_tombstones_late_terminal_before_contact_id_reuse() {
    let mut native = coordinator();
    let (binding, _) = register_roots(&mut native);
    let window = WindowId::from(96);
    let device = DeviceId::dummy();
    let mut translator = NativePointerTranslator::default();
    let started = touch_record(1, window, binding, device, 10, TouchPhase::Started);

    assert_eq!(
        translator.translate(&started, |_| None),
        NativePointerTranslation::Input(pointer_input(
            binding,
            NativePointerId::new(1),
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
        ))
    );
    translator.retire_binding(binding);
    assert!(!translator.references_binding(binding));
    assert_eq!(
        translator.translate(
            &touch_record(2, window, binding, device, 10, TouchPhase::Ended),
            |_| None,
        ),
        NativePointerTranslation::Ignored,
        "a late terminal must be consumed by the retired-contact tombstone"
    );
    assert_eq!(
        translator.translate(&started, |_| None),
        NativePointerTranslation::Input(pointer_input(
            binding,
            NativePointerId::new(2),
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
        )),
        "an explicit Started event reuses the contact ID with a fresh adapter pointer"
    );
}

#[test]
fn uncorrelated_raw_removal_forgets_all_window_input_identity_shapes() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let first_window = WindowId::from(101);
    let second_window = WindowId::from(102);
    let device = DeviceId::dummy();
    let first_mouse = NativeWindowEventRecord::for_test(
        1,
        first_window,
        None,
        Some(first),
        WindowEvent::MouseInput {
            device_id: device,
            state: ElementState::Pressed,
            button: MouseButton::Left,
            facts: PointerEventFacts::default(),
        },
    );
    let first_touch = touch_record(2, first_window, first, device, 21, TouchPhase::Started);
    let second_mouse = NativeWindowEventRecord::for_test(
        3,
        second_window,
        None,
        Some(second),
        WindowEvent::MouseInput {
            device_id: device,
            state: ElementState::Pressed,
            button: MouseButton::Left,
            facts: PointerEventFacts::default(),
        },
    );
    let mut translator = NativePointerTranslator::default();

    assert!(matches!(
        translator.translate_for_test_device(&first_mouse, 0, |_| None),
        NativePointerTranslation::Input(_)
    ));
    assert!(matches!(
        translator.translate_for_test_device(&first_touch, 2, |_| None),
        NativePointerTranslation::Input(_)
    ));
    assert!(matches!(
        translator.translate_for_test_device(&second_mouse, 3, |_| None),
        NativePointerTranslation::Input(_)
    ));

    translator.forget_removed_devices();
    assert!(translator.can_coalesce_idle_cursor_moves());
    assert!(!translator.references_binding(first));
    assert!(!translator.references_binding(second));

    let late_release = NativeWindowEventRecord::for_test(
        4,
        first_window,
        None,
        Some(first),
        WindowEvent::MouseInput {
            device_id: device,
            state: ElementState::Released,
            button: MouseButton::Left,
            facts: PointerEventFacts::default(),
        },
    );
    assert_eq!(
        translator.translate_for_test_device(&late_release, 0, |_| None),
        NativePointerTranslation::Ignored,
        "a removed device cannot recreate its old stream from a late release"
    );

    let second_release = NativeWindowEventRecord::for_test(
        5,
        second_window,
        None,
        Some(second),
        WindowEvent::MouseInput {
            device_id: device,
            state: ElementState::Released,
            button: MouseButton::Left,
            facts: PointerEventFacts::default(),
        },
    );
    assert_eq!(
        translator.translate_for_test_device(&second_release, 3, |_| None),
        NativePointerTranslation::Ignored,
        "a different virtual window device is also tombstoned by the uncorrelated removal"
    );
    assert_eq!(
        translator.translate_for_test_device(
            &touch_record(6, first_window, first, device, 21, TouchPhase::Moved),
            2,
            |_| None,
        ),
        NativePointerTranslation::Ignored,
        "the same provider reset also tombstones touch contacts"
    );

    assert_eq!(
        translator.translate_for_test_device(&first_mouse, 0, |_| None),
        NativePointerTranslation::Input(pointer_input(
            first,
            NativePointerId::new(4),
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
        )),
        "an explicit start gives the Windows-style virtual identity a fresh adapter pointer"
    );
    assert_eq!(
        translator.translate_for_test_device(&second_mouse, 3, |_| None),
        NativePointerTranslation::Input(pointer_input(
            second,
            NativePointerId::new(5),
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
        )),
        "an explicit start independently reenrolls the X11-style master identity"
    );
    assert_eq!(
        translator.translate_for_test_device(&first_touch, 2, |_| None),
        NativePointerTranslation::Input(pointer_input(
            first,
            NativePointerId::new(6),
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
        )),
        "a fresh touch start preserves the identity allocator's monotonic order"
    );
}

#[test]
fn removed_device_tail_cannot_reenroll_before_an_explicit_stream_start() {
    let mut native = coordinator();
    let (binding, _) = register_roots(&mut native);
    let window = WindowId::from(103);
    let device = DeviceId::dummy();
    let mut translator = NativePointerTranslator::default();
    let press = NativeWindowEventRecord::for_test(
        1,
        window,
        None,
        Some(binding),
        WindowEvent::MouseInput {
            device_id: device,
            state: ElementState::Pressed,
            button: MouseButton::Left,
            facts: PointerEventFacts::default(),
        },
    );

    assert!(matches!(
        translator.translate_for_test_device(&press, 404, |_| None),
        NativePointerTranslation::Input(_)
    ));
    translator.forget_removed_devices();

    let late_move = NativeWindowEventRecord::for_test(
        2,
        window,
        None,
        Some(binding),
        WindowEvent::CursorMoved {
            device_id: device,
            position: PhysicalPosition::new(30.0, 40.0),
            facts: PointerEventFacts::default(),
        },
    );
    let late_release = NativeWindowEventRecord::for_test(
        3,
        window,
        None,
        Some(binding),
        WindowEvent::MouseInput {
            device_id: device,
            state: ElementState::Released,
            button: MouseButton::Left,
            facts: PointerEventFacts::default(),
        },
    );
    assert_eq!(
        translator.translate_for_test_device(&late_move, 404, |_| None),
        NativePointerTranslation::Ignored
    );
    assert_eq!(
        translator.translate_for_test_device(&late_release, 404, |_| None),
        NativePointerTranslation::Ignored,
        "a late move cannot clear the tombstone for its following release"
    );
    assert_eq!(
        translator.translate_for_test_device(&press, 404, |_| None),
        NativePointerTranslation::Input(pointer_input(
            binding,
            NativePointerId::new(2),
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
        )),
        "only an explicit press starts the reused device lifetime"
    );
}

#[test]
fn device_removal_callback_commits_one_provider_reset_fact() {
    assert_device_removal_commits(false);
}

#[test]
fn quarantined_device_removal_still_commits_one_provider_reset_fact() {
    assert_device_removal_commits(true);
}

fn assert_device_removal_commits(quarantined: bool) {
    let mut native = coordinator();
    let (binding, _) = register_roots(&mut native);
    let window = WindowId::from(111);
    let device = DeviceId::dummy();
    let callbacks = [
        NativeWindowEventRecord::for_test(
            1,
            window,
            None,
            Some(binding),
            WindowEvent::MouseInput {
                device_id: device,
                state: ElementState::Pressed,
                button: MouseButton::Left,
                facts: PointerEventFacts::default(),
            },
        ),
        touch_record(2, window, binding, device, 31, TouchPhase::Started),
    ];

    for callback in callbacks {
        native.bridge.push_record(HostRecord::WindowEvent(callback));
        assert!(
            native
                .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
                .expect("the pointer callback records")
                .is_some()
        );
        commit_pending_pointer_frame(&mut native);
    }

    assert!(!native.pointer_translator.can_coalesce_idle_cursor_moves());
    if quarantined {
        assert!(native.quarantine_after_fatal().is_empty());
    }
    let recorded = native.session.native_input_recorded_count();
    native.bridge.push_record(HostRecord::DeviceRemoval(
        NativeDeviceRemovalRecord::for_test(3),
    ));
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect("one removal callback records the provider reset fact")
            .is_some()
    );
    assert_eq!(
        native.session.native_input_recorded_count(),
        recorded + 1,
        "one accepted removal records one fact regardless of the live stream count",
    );
    drop(
        native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("the provider reset candidate can be dropped"),
    );
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    let report = commit_pending_pointer_frame(&mut native);
    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::NativeInputDeviceRemoval]
    );

    assert!(!native.bridge.has_pending_callback_records());
    assert!(!native.session.has_active_gesture());
    assert!(native.pointer_translator.can_coalesce_idle_cursor_moves());
    assert!(!native.pointer_translator.references_binding(binding));
}

fn touch_record(
    ordinal: u64,
    window: WindowId,
    binding: NativeSurfaceBinding,
    device_id: DeviceId,
    id: u64,
    phase: TouchPhase,
) -> NativeWindowEventRecord {
    NativeWindowEventRecord::for_test(
        ordinal,
        window,
        None,
        Some(binding),
        WindowEvent::Touch(Touch {
            device_id,
            phase,
            location: PhysicalPosition::new(24.0, 36.0),
            facts: PointerEventFacts::default(),
            force: None,
            id,
        }),
    )
}

fn pointer_input(
    binding: NativeSurfaceBinding,
    pointer: NativePointerId,
    event: NativePointerEvent,
) -> NativePointerInput {
    NativePointerInput::new(
        pointer,
        event,
        NativeDesktopPointerLocation::new(
            NativeDesktopPosition::Unknown,
            NativePointerHover::Unknown,
            None,
        ),
        NativePointerOwner::Native(binding),
        NativePointerOwner::Unknown,
    )
}

fn authority_free_pointer_input(
    pointer: NativePointerId,
    event: NativePointerEvent,
) -> NativePointerInput {
    NativePointerInput::new(
        pointer,
        event,
        NativeDesktopPointerLocation::new(
            NativeDesktopPosition::Unknown,
            NativePointerHover::Unknown,
            None,
        ),
        NativePointerOwner::Unknown,
        NativePointerOwner::Unknown,
    )
}

fn commit_pending_pointer_frame(native: &mut NativeCoordinator) -> HostFrameReport {
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the pending pointer frame begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the pending pointer frame settles every surface");
    frame.commit().expect("the pending pointer frame commits")
}

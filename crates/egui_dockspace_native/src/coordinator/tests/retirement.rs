use dockspace::runtime::{
    NativeDesktopPointerLocation, NativeDesktopPosition, NativePointerCancelReason,
    NativePointerEvent, NativePointerHover, NativePointerId, NativePointerInput,
    NativePointerOwner,
};
use eframe::egui::ViewportCommand;
use winit::dpi::PhysicalPosition;
use winit::event::{
    DeviceId, ElementState, MouseButton, MouseScrollDelta, PointerEventFacts, PointerWindowRoute,
    TouchPhase, WindowEvent,
};

use super::*;
use crate::event::NativePointerRouteSnapshot;

#[test]
fn destroyed_binding_cancels_pressed_pointer_before_successor_press() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let first_window = WindowId::from(11);
    let second_window = WindowId::from(22);
    let second_viewport = ViewportId::from_hash_of("successor-pointer-window");
    native
        .bind_viewport(ViewportId::ROOT, first_window, first)
        .expect("first viewport binds");
    native
        .bind_viewport(second_viewport, second_window, second)
        .expect("successor viewport binds");

    let first_press = NativeWindowEventRecord::for_test(
        1,
        first_window,
        Some(ViewportId::ROOT),
        Some(first),
        WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: ElementState::Pressed,
            button: MouseButton::Left,
            facts: PointerEventFacts::default(),
        },
    );
    native
        .bridge
        .push_record(HostRecord::WindowEvent(first_press));
    assert!(
        native
            .reduce_callback_head()
            .expect("the first press enters the native pointer journal")
    );
    commit_pending_pointer_frame(&mut native);

    let destroyed = {
        let mut viewports = native
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        NativeWindowEventRecord::for_test_ingress(
            2,
            first_window,
            Some(ViewportId::ROOT),
            WindowEvent::Destroyed,
            &mut viewports,
        )
    };
    native
        .bridge
        .push_record(HostRecord::WindowEvent(destroyed));
    assert!(
        native
            .reduce_callback_head()
            .expect("the binding-retired pointer terminal records first")
    );
    assert!(native.pointer_translator.has_pending_provider_tail());
    commit_pending_pointer_frame(&mut native);
    assert!(
        native
            .reduce_callback_head()
            .expect("the retained destroyed callback completes on the next boundary")
    );
    assert!(!native.pointer_translator.has_pending_provider_tail());
    commit_pending_pointer_frame(&mut native);

    let late_release = NativeWindowEventRecord::for_test(
        3,
        second_window,
        Some(second_viewport),
        Some(second),
        WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: ElementState::Released,
            button: MouseButton::Left,
            facts: PointerEventFacts::default(),
        },
    );
    native
        .bridge
        .push_record(HostRecord::WindowEvent(late_release));
    assert!(
        native
            .reduce_callback_head()
            .expect("the late physical release consumes the cancelled-button tombstone")
    );
    commit_pending_pointer_frame(&mut native);

    let successor_press = NativeWindowEventRecord::for_test(
        4,
        second_window,
        Some(second_viewport),
        Some(second),
        WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: ElementState::Pressed,
            button: MouseButton::Left,
            facts: PointerEventFacts::default(),
        },
    );
    native
        .bridge
        .push_record(HostRecord::WindowEvent(successor_press));
    assert!(
        native
            .reduce_callback_head()
            .expect("the successor press opens a fresh pointer incarnation")
    );
    commit_pending_pointer_frame(&mut native);

    assert!(!native.pointer_translator.references_binding(first));
    assert!(native.pointer_translator.references_binding(second));
}

#[test]
fn idle_delivery_does_not_become_a_persistent_pointer_owner() {
    let mut native = coordinator();
    let (binding, _) = register_roots(&mut native);
    let moved = NativeWindowEventRecord::for_test(
        1,
        WindowId::from(11),
        Some(ViewportId::ROOT),
        Some(binding),
        WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(10.0, 20.0),
            facts: PointerEventFacts::default(),
        },
    );

    assert!(matches!(
        native.pointer_translator.translate(&moved, |_| None),
        NativePointerTranslation::Input(_)
    ));
    assert!(!native.pointer_translator.references_binding(binding));
    assert!(
        native
            .pointer_translator
            .cancel_destroyed_binding(binding)
            .is_none(),
        "an idle delivery route is not a live pointer owner"
    );
}

#[test]
fn released_button_no_longer_keeps_its_press_binding_live() {
    let mut native = coordinator();
    let (binding, _) = register_roots(&mut native);
    let window = WindowId::from(11);

    for (ordinal, state) in [(1, ElementState::Pressed), (2, ElementState::Released)] {
        let record = NativeWindowEventRecord::for_test(
            ordinal,
            window,
            Some(ViewportId::ROOT),
            Some(binding),
            WindowEvent::MouseInput {
                device_id: DeviceId::dummy(),
                state,
                button: MouseButton::Left,
                facts: PointerEventFacts::default(),
            },
        );
        assert!(matches!(
            native.pointer_translator.translate(&record, |_| None),
            NativePointerTranslation::Input(_)
        ));
    }

    assert!(!native.pointer_translator.references_binding(binding));
    assert!(
        native
            .pointer_translator
            .cancel_destroyed_binding(binding)
            .is_none(),
        "a normally released button must not synthesize a later cancellation"
    );
}

#[test]
fn pressed_delivery_and_capture_bindings_remain_independent() {
    let mut native = coordinator();
    let (delivery, captured) = register_roots(&mut native);
    let delivery_window = WindowId::from(11);
    let captured_window = WindowId::from(22);
    let captured_viewport = ViewportId::from_hash_of("captured-press-owner");
    native
        .bind_viewport(ViewportId::ROOT, delivery_window, delivery)
        .expect("delivery viewport binds");
    native
        .bind_viewport(captured_viewport, captured_window, captured)
        .expect("captured viewport binds");

    let press = NativeWindowEventRecord::for_test_snapshot(
        1,
        delivery_window,
        Some(ViewportId::ROOT),
        WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: ElementState::Pressed,
            button: MouseButton::Left,
            facts: PointerEventFacts {
                capture: PointerWindowRoute::Window(captured_window),
                ..PointerEventFacts::default()
            },
        },
        &native
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner),
    );
    assert!(matches!(
        native.pointer_translator.translate(&press, |_| None),
        NativePointerTranslation::Input(_)
    ));
    assert!(native.pointer_translator.references_binding(delivery));
    assert!(native.pointer_translator.references_binding(captured));

    let capture_released = NativeWindowEventRecord::for_test_snapshot(
        2,
        delivery_window,
        Some(ViewportId::ROOT),
        WindowEvent::PointerCaptureChanged {
            device_id: DeviceId::dummy(),
            capture: PointerWindowRoute::None,
        },
        &native
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner),
    );
    assert!(matches!(
        native
            .pointer_translator
            .translate(&capture_released, |_| None),
        NativePointerTranslation::Input(_)
    ));
    assert!(native.pointer_translator.references_binding(delivery));
    assert!(!native.pointer_translator.references_binding(captured));
    assert_eq!(
        native.pointer_translator.cancel_destroyed_binding(delivery),
        Some(retired_pointer_input())
    );
}

#[test]
fn capture_retirement_swallows_release_from_unknown_delivery() {
    let mut native = coordinator();
    let (captured, _) = register_roots(&mut native);
    let captured_window = WindowId::from(22);
    let captured_viewport = ViewportId::from_hash_of("captured-unknown-delivery");
    native
        .bind_viewport(captured_viewport, captured_window, captured)
        .expect("captured viewport binds");

    let unknown_window = WindowId::from(33);
    let press = NativeWindowEventRecord::for_test_snapshot(
        1,
        unknown_window,
        None,
        WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: ElementState::Pressed,
            button: MouseButton::Left,
            facts: PointerEventFacts {
                capture: PointerWindowRoute::Window(captured_window),
                ..PointerEventFacts::default()
            },
        },
        &native
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner),
    );
    assert!(matches!(
        native.pointer_translator.translate(&press, |_| None),
        NativePointerTranslation::Input(_)
    ));
    assert!(native.pointer_translator.references_binding(captured));
    assert_eq!(
        native.pointer_translator.cancel_destroyed_binding(captured),
        Some(retired_pointer_input())
    );
    assert_eq!(
        native.pointer_translator.reset_destroyed_binding(captured),
        None
    );

    let late_release = NativeWindowEventRecord::for_test(
        2,
        unknown_window,
        None,
        None,
        WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: ElementState::Released,
            button: MouseButton::Left,
            facts: PointerEventFacts::default(),
        },
    );
    assert_eq!(
        native.pointer_translator.translate(&late_release, |_| None),
        NativePointerTranslation::Ignored
    );
}

fn retired_pointer_input() -> NativePointerInput {
    NativePointerInput::new(
        NativePointerId::new(1),
        NativePointerEvent::StreamCancelled(NativePointerCancelReason::BindingRetired),
        NativeDesktopPointerLocation::new(
            NativeDesktopPosition::Unknown,
            NativePointerHover::Unknown,
            None,
        ),
        NativePointerOwner::Unknown,
        NativePointerOwner::Unknown,
    )
}

fn commit_pending_pointer_frame(native: &mut NativeCoordinator) {
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the pending pointer frame begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the pending pointer frame settles every surface");
    frame.commit().expect("the pending pointer frame commits");
}

#[test]
fn release_child_preserves_scroll_correlation_for_the_provider_terminal() {
    let mut native = coordinator();
    let (root, child_binding) = register_root_and_child(&mut native);
    let root_window = WindowId::from(11);
    let child_window = WindowId::from(22);
    let child = ViewportId::from_hash_of("scrolling-release-child");
    native
        .bind_viewport(ViewportId::ROOT, root_window, root)
        .expect("root viewport binds");
    native
        .bind_viewport(child, child_window, child_binding)
        .expect("child viewport binds");

    let started = NativeWindowEventRecord::for_test(
        9,
        child_window,
        Some(child),
        Some(child_binding),
        WindowEvent::MouseWheel {
            device_id: DeviceId::dummy(),
            delta: MouseScrollDelta::LineDelta(0.0, 1.0),
            phase: TouchPhase::Started,
            facts: PointerEventFacts::default(),
        },
    );
    assert!(matches!(
        native.pointer_translator.translate(&started, |_| None),
        NativePointerTranslation::Input(_)
    ));

    let mut redock = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the child redock frame begins");
    redock
        .frame
        .dock_root_current(
            RootId::new(2),
            DockPlacement::Center(DockAnchor::Item(ItemId::new(1))),
        )
        .expect("the complete child root redocks");
    redock
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the child redock settles every surface");
    let mut report = redock.commit().expect("the child redock commits");
    let effects = report.take_native_effects();
    assert!(matches!(
        effects.as_slice(),
        [request]
            if matches!(
                request.operation(),
                NativeEffectOperation::ReleaseChild { binding } if *binding == child_binding
            )
    ));
    native
        .accept_native_effects(effects)
        .expect("the child release is accepted");

    assert!(
        native.pointer_translator.references_binding(child_binding),
        "ReleaseChild cannot replace the provider's terminal scroll edge"
    );
    assert!(
        native
            .pointer_translator
            .cancel_destroyed_binding(child_binding)
            .is_some(),
        "the later Destroyed callback can still retire the semantic owner"
    );
    assert!(
        native
            .pointer_translator
            .reset_destroyed_binding(child_binding)
            .is_some(),
        "the following boundary can still close the provider tail"
    );
}

#[test]
fn destroyed_child_route_retires_only_after_tombstone_commit_and_quiescence() {
    let mut native = coordinator();
    assert_eq!(
        native.lifecycle_progress(),
        NativeLifecycleProgress::default()
    );
    let (first, second) = register_root_and_child(&mut native);
    let first_window = WindowId::from(11);
    let second_window = WindowId::from(22);
    let child = ViewportId::from_hash_of("retired-native-child");
    native
        .bind_viewport(ViewportId::ROOT, first_window, first)
        .expect("root viewport binds");
    native
        .bind_viewport(child, second_window, second)
        .expect("child viewport binds");

    let mut redock = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the child redock frame begins");
    redock
        .frame
        .dock_root_current(
            RootId::new(2),
            DockPlacement::Center(DockAnchor::Item(ItemId::new(1))),
        )
        .expect("the complete child root redocks through the product action");
    redock
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the child redock settles every surface");
    let mut redocked = redock.commit().expect("the child redock commits");
    let effects = redocked.take_native_effects();
    assert_eq!(
        effects.len(),
        1,
        "redocking one child emits one release; inputs={:#?}; view={:#?}",
        redocked.inputs(),
        native.session().view(),
    );
    assert!(matches!(
        effects[0].operation(),
        NativeEffectOperation::ReleaseChild { binding } if *binding == second
    ));
    native
        .accept_native_effects(effects)
        .expect("the exact child release acknowledgement is retained");

    let destroyed = {
        let mut viewports = native
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        NativeWindowEventRecord::for_test_ingress(
            14,
            second_window,
            None,
            WindowEvent::Destroyed,
            &mut viewports,
        )
    };
    assert_eq!(destroyed.viewport_id(), Some(child));
    assert_eq!(destroyed.binding(), Some(second));
    native
        .bridge
        .push_record(HostRecord::WindowEvent(destroyed.clone()));
    native
        .bridge
        .push_record(HostRecord::WindowEvent(destroyed));
    let late_pointer = NativeWindowEventRecord::for_test_snapshot(
        15,
        first_window,
        Some(ViewportId::ROOT),
        WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(10.0, 20.0),
            facts: PointerEventFacts {
                hover: PointerWindowRoute::Window(second_window),
                capture: PointerWindowRoute::Window(second_window),
                ..PointerEventFacts::default()
            },
        },
        &native
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner),
    );
    let routes = late_pointer
        .pointer_routes()
        .expect("the late event retains explicit pointer authority");
    assert_eq!(routes.delivery(), NativePointerRouteSnapshot::Dock(first));
    assert_eq!(routes.hover(), NativePointerRouteSnapshot::Unknown);
    assert_eq!(routes.capture(), NativePointerRouteSnapshot::Unknown);
    assert!(!late_pointer.references_binding(second));
    native.bridge.push_viewport_roster(live_roster([first]));
    native
        .bridge
        .push_record(HostRecord::WindowEvent(late_pointer));

    assert!(
        native
            .reduce_callback_head()
            .expect("the exact destruction callback is retained")
    );
    assert_eq!(native.lifecycle_progress().destroyed_observations(), 1);
    let mut event_frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the destruction callback boundary begins");
    event_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the callback boundary settles every surface");
    event_frame
        .commit()
        .expect("the destruction callback boundary commits");

    assert!(
        native
            .reduce_callback_head()
            .expect("the duplicate destruction callback is retained idempotently")
    );
    assert_eq!(native.lifecycle_progress().destroyed_observations(), 1);
    let mut duplicate_event_frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the duplicate destruction callback boundary begins");
    duplicate_event_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the duplicate callback boundary settles every surface");
    duplicate_event_frame
        .commit()
        .expect("the duplicate destruction callback boundary commits");

    assert_eq!(native.viewport_binding(child), Some(second));
    assert!(!native.session.is_current_native_binding(second));
    assert!(native.session.recognizes_native_binding(second));
    assert!(
        native
            .deferred_viewport_specs()
            .iter()
            .all(|spec| spec.binding() != second),
        "the destruction callback stops re-declaring the retired child"
    );
    let (_, destroyed_facts) = native
        .retirements
        .destroyed_observations()
        .next()
        .expect("the exact destroyed child is awaiting snapshot publication");
    assert_ne!(
        destroyed_facts,
        NativeWindowFacts::destroyed(),
        "the destroyed tombstone retains the ReleaseChild acknowledgement"
    );

    assert!(
        native
            .reduce_callback_head()
            .expect("the complete roster publishes the tombstone")
    );
    let mut roster_frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the roster boundary begins");
    roster_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the roster boundary settles every surface");
    let mut report = roster_frame
        .commit()
        .expect("the destroyed tombstone commits");
    assert!(
        report.take_native_effects().is_empty(),
        "the exact close acknowledgement settles ReleaseChild without cleanup fallback"
    );
    assert!(native.settle_host_frame_inputs(report.inputs()));

    assert!(
        native
            .reduce_callback_head()
            .expect("the post-tombstone pointer edge remains reducible")
    );
    let mut pointer_frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the post-tombstone pointer boundary begins");
    pointer_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the pointer boundary settles every surface");
    pointer_frame
        .commit()
        .expect("the post-tombstone pointer boundary commits");

    let prepared = native
        .prepare_committed_retirements()
        .expect("the exact committed route prepares atomically")
        .expect("one exact route is ready to retire");
    drop(prepared);
    assert_eq!(native.viewport_binding(child), Some(second));
    let prepared = native
        .prepare_committed_retirements()
        .expect("an abandoned route retirement remains retryable")
        .expect("the exact route can be prepared again");

    let mut retirement_frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the route retirement boundary begins");
    retirement_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the route retirement boundary settles every surface");
    retirement_frame
        .commit()
        .expect("the route retirement boundary commits");
    assert!(
        native.commit_retirements(prepared).is_empty(),
        "the fixture has no abandoned renderer outputs"
    );

    assert_eq!(native.viewport_binding(child), None);
    assert!(!native.session.is_current_native_binding(second));
    assert!(native.session.recognizes_native_binding(second));
    assert!(
        native
            .try_report_retirement_quiescence()
            .expect("the retired route becomes quiescent")
    );
    assert_eq!(native.lifecycle_progress().quiescent_retirements(), 1);
    assert!(!native.session.recognizes_native_binding(second));

    let mut quiescence_frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the quiescence boundary begins");
    quiescence_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the quiescence boundary settles every surface");
    quiescence_frame
        .commit()
        .expect("the quiescence boundary commits");

    let mut reclaimed_frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the committed quiescence prefix is reclaimed");
    reclaimed_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the reclamation boundary settles every surface");
    reclaimed_frame
        .commit()
        .expect("the quiescence prefix retires its exact destroyed guard");
}

#[test]
fn shutdown_drains_destroyed_child_to_quiescence_without_paint() {
    let mut native = coordinator();
    let (root, child_binding) = register_root_and_child(&mut native);
    let root_window = WindowId::from(31);
    let child_window = WindowId::from(32);
    let child = ViewportId::from_hash_of("shutdown-drain-child");
    native
        .bind_viewport(ViewportId::ROOT, root_window, root)
        .expect("root viewport binds");
    native
        .bind_viewport(child, child_window, child_binding)
        .expect("child viewport binds");

    let mut redock = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the child redock frame begins");
    redock
        .frame
        .dock_root_current(
            RootId::new(2),
            DockPlacement::Center(DockAnchor::Item(ItemId::new(1))),
        )
        .expect("the complete child root redocks");
    redock
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the child redock settles every surface");
    let mut report = redock.commit().expect("the child redock commits");
    native
        .accept_native_effects(report.take_native_effects())
        .expect("the child release is accepted before shutdown");
    assert!(native.quarantine_after_fatal().is_empty());

    let destroyed = {
        let mut viewports = native
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        NativeWindowEventRecord::for_test_ingress(
            41,
            child_window,
            Some(child),
            WindowEvent::Destroyed,
            &mut viewports,
        )
    };
    native
        .bridge
        .push_record(HostRecord::WindowEvent(destroyed));
    native.bridge.push_viewport_roster(live_roster([root]));

    for phase in ["destroyed", "roster", "route", "quiescence"] {
        let advance = native.advance_shutdown_boundary();
        assert!(
            advance.progress,
            "shutdown {phase} boundary made no progress"
        );
        assert!(advance.commands.is_empty());
        assert!(advance.abandoned_outputs.is_empty());
    }

    assert_eq!(native.viewport_binding(child), None);
    assert!(!native.session.is_current_native_binding(child_binding));
    assert!(!native.session.recognizes_native_binding(child_binding));
}

#[test]
fn redocked_external_root_does_not_request_platform_close() {
    let mut native = coordinator();
    let (first, second) = register_roots(&mut native);
    let second_viewport = ViewportId::from_hash_of("externally-owned-secondary-root");
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(11), first)
        .expect("primary root viewport binds");
    native
        .bind_viewport(second_viewport, WindowId::from(22), second)
        .expect("secondary root viewport binds");
    native
        .report_snapshot(
            [
                (first, NativeWindowFacts::live()),
                (second, NativeWindowFacts::live()),
            ],
            NativeWorkAreaRoster::Unknown,
        )
        .expect("the exact root inventory records");
    let mut inventory = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the root inventory frame begins");
    inventory
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the root inventory settles every surface");
    inventory.commit().expect("the root inventory commits");

    let mut redock = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the application root redock frame begins");
    redock
        .frame
        .dock_root_current(
            RootId::new(2),
            DockPlacement::Center(DockAnchor::Item(ItemId::new(1))),
        )
        .expect("the complete application root redocks through the product action");
    redock
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the application root redock settles every surface");
    let mut report = redock.commit().expect("the external root redock commits");
    assert!(
        report.take_native_effects().is_empty(),
        "an externally owned root unbinds without a platform close effect"
    );
    assert!(native.take_viewport_commands().is_empty());
    assert!(!native.session().is_current_native_binding(second));
}

#[test]
fn queued_viewport_close_retains_its_exact_binding_until_dispatch() {
    let mut native = coordinator();
    let (_, binding) = register_roots(&mut native);
    let viewport = ViewportId::from_hash_of("queued-root-close-command");

    native
        .effects
        .queue_command(viewport, binding, ViewportCommand::Close);
    assert!(native.effects.references_binding(binding));
    assert_eq!(
        native.take_viewport_commands(),
        [(viewport, ViewportCommand::Close)]
    );
    assert!(!native.effects.references_binding(binding));
}

#[test]
fn queued_pointer_routes_keep_every_referenced_binding_live() {
    let mut native = coordinator();
    let (delivery, captured) = register_roots(&mut native);
    let delivery_window = WindowId::from(11);
    let captured_window = WindowId::from(22);
    let captured_viewport = ViewportId::from_hash_of("captured-native-child");
    native
        .bind_viewport(ViewportId::ROOT, delivery_window, delivery)
        .expect("delivery viewport binds");
    native
        .bind_viewport(captured_viewport, captured_window, captured)
        .expect("captured viewport binds");

    let event = WindowEvent::MouseInput {
        device_id: DeviceId::dummy(),
        state: ElementState::Released,
        button: MouseButton::Left,
        facts: PointerEventFacts {
            hover: PointerWindowRoute::Window(captured_window),
            capture: PointerWindowRoute::Window(captured_window),
            ..PointerEventFacts::default()
        },
    };
    let record = NativeWindowEventRecord::for_test_snapshot(
        15,
        delivery_window,
        Some(ViewportId::ROOT),
        event,
        &native
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner),
    );

    assert_eq!(record.binding(), Some(delivery));
    assert!(record.references_binding(captured));
    native.bridge.push_record(HostRecord::WindowEvent(record));
    assert!(native.bridge.references_binding(captured));
}

use eframe::egui::ViewportCommand;
use winit::dpi::PhysicalPosition;
use winit::event::{
    DeviceId, ElementState, MouseButton, MouseScrollDelta, PointerEventFacts, PointerWindowRoute,
    TouchPhase, WindowEvent,
};

use super::*;
use crate::event::NativePointerRouteSnapshot;

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
        let advance = native
            .advance_shutdown_boundary()
            .unwrap_or_else(|error| panic!("shutdown {phase} boundary failed: {error}"));
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

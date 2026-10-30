use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceSession, HostInputOutcome, HostWindowToken, NativePointerRoster,
    SurfaceUnavailableReason,
};
use eframe::{
    NativeViewportPointerPassthroughCommandToken, NativeViewportPointerPassthroughStatus,
    NativeViewportPropertyCommandRetirement, egui,
};
use winit::dpi::PhysicalPosition;
use winit::event::{DeviceId, PointerEventFacts, WindowEvent};

use super::*;

fn bindings() -> (NativeSurfaceBinding, NativeSurfaceBinding) {
    let layout = DockspaceLayout::new([1, 2].map(|id| {
        DockspaceSurfaceLayout::new(
            SurfaceId::new(id),
            DockspaceRootLayout::new(
                RootId::new(id),
                DockspaceNode::central_tabs([ItemId::new(id)]),
            ),
        )
    }))
    .expect("test layout validates");
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default())
        .expect("test session initializes");
    session
        .enable_managed_native_host(
            dockspace::runtime::NativeCreateStrategy::Staged,
            NativePointerRoster::Exact(Vec::new()),
        )
        .expect("native host enrolls");
    for id in [1, 2] {
        session
            .register_native_root(SurfaceId::new(id), HostWindowToken::new(id))
            .expect("native root registers");
    }
    let mut frame = session
        .begin_host_frame()
        .expect("registration frame begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("registration surfaces settle");
    let report = frame.commit().expect("registration frame commits");
    let mut found = report.inputs().iter().filter_map(|input| match input {
        HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
        _ => None,
    });
    (found.next().unwrap(), found.next().unwrap())
}

fn bridge_with_clock() -> (NativeHostBridge, Arc<AtomicU64>) {
    let now = Arc::new(AtomicU64::new(0));
    let source = Arc::clone(&now);
    let bridge = NativeHostBridge::with_clock(
        NativeViewportMap::default(),
        NativeMonotonicClock::new(move || Duration::from_millis(source.load(Ordering::SeqCst))),
    );
    (bridge, now)
}

fn cursor(
    ordinal: u64,
    window: WindowId,
    viewport: ViewportId,
    binding: NativeSurfaceBinding,
) -> HostRecord {
    HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
        ordinal,
        window,
        Some(viewport),
        Some(binding),
        WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(ordinal as f64, 20.0),
            facts: PointerEventFacts::default(),
        },
    ))
}

fn pointer_callback(
    token: NativeViewportPointerPassthroughCommandToken,
    viewport: ViewportId,
    window: WindowId,
    binding: Option<NativeSurfaceBinding>,
    enabled: bool,
) -> NativePointerPassthroughRecord {
    NativePointerPassthroughRecord::for_test(
        token,
        viewport,
        window,
        binding,
        enabled,
        NativeViewportPointerPassthroughStatus::Applied,
    )
}

#[test]
fn owner_slots_merge_with_raw_input_in_capture_order_across_resource_keys() {
    let (first_binding, second_binding) = bindings();
    let first_viewport = ViewportId::ROOT;
    let second_viewport = ViewportId::from_hash_of("ordered-second-command-owner");
    let context = egui::Context::default();
    let first_token =
        eframe::queue_native_viewport_pointer_passthrough(&context, first_viewport, true);
    let second_token =
        eframe::queue_native_viewport_pointer_passthrough(&context, second_viewport, false);
    let first = pointer_callback(
        first_token,
        first_viewport,
        WindowId::from(31),
        Some(first_binding),
        true,
    );
    let second = pointer_callback(
        second_token,
        second_viewport,
        WindowId::from(32),
        Some(second_binding),
        false,
    );
    let (bridge, now) = bridge_with_clock();
    {
        let mut records = bridge.lock();
        assert!(records.window_resources.reserve_pointer_callback(
            first_viewport,
            first_binding,
            first.window(),
            first_token,
            true,
        ));
        assert!(records.window_resources.reserve_pointer_callback(
            second_viewport,
            second_binding,
            second.window(),
            second_token,
            false,
        ));
        // Capture in reverse resource order so map traversal cannot be the clock.
        let (early, late) = if first_viewport < second_viewport {
            (second, first)
        } else {
            (first, second)
        };
        assert!(records.record_viewport_pointer_passthrough(early));
        now.store(10, Ordering::SeqCst);
        records.record_device_removal(NativeDeviceRemovalRecord::for_test(71));
        now.store(20, Ordering::SeqCst);
        assert!(records.record_viewport_pointer_passthrough(late));
        assert_eq!(records.journal.len(), 1);
        assert_eq!(records.retained_record_count(), 3);
        let ordered: Vec<_> = records
            .ordered_ingress()
            .map(|ingress| ingress.ordinal())
            .collect();
        assert_eq!(ordered, [1, 2, 3]);
        assert!(matches!(
            records.ordered_ingress().nth(1).unwrap().record(),
            HostRecord::DeviceRemoval(removal) if *removal == NativeDeviceRemovalRecord::for_test(71)
        ));
        assert!(records.raw_prefix().next().is_none());
        assert!(
            matches!(records.front_record(), Some(HostRecord::ViewportPointerPassthrough(record)) if *record == early)
        );
        records.transfer_front_record();
        assert_eq!(records.retained_record_count(), 3);
        assert!(matches!(
            records.front_record(),
            Some(HostRecord::DeviceRemoval(_))
        ));
        records.transfer_front_record();
        assert!(
            matches!(records.front_record(), Some(HostRecord::ViewportPointerPassthrough(record)) if *record == late)
        );
    }
    now.store(50, Ordering::SeqCst);
    bridge.commit_frame_boundary(true);
    assert_eq!(bridge.lock().retained_record_count(), 3);
    assert_eq!(
        bridge.lock().oldest_record_age(),
        Some(Duration::from_millis(50))
    );
    bridge.commit_frame_boundary(false);
    assert_eq!(bridge.lock().retained_record_count(), 1);
    assert_eq!(
        bridge.lock().oldest_record_age(),
        Some(Duration::from_millis(30))
    );
}

#[test]
fn reserved_callback_blocks_cursor_front_coalescing_and_claim_until_exact_handoff() {
    let (binding, foreign) = bindings();
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(11);
    let context = egui::Context::default();
    let first = eframe::queue_native_viewport_pointer_passthrough(&context, viewport, true);
    let next = eframe::queue_native_viewport_pointer_passthrough(&context, viewport, false);
    let callback = pointer_callback(first, viewport, window, Some(binding), true);
    let (bridge, now) = bridge_with_clock();

    bridge.push_record(cursor(1, window, viewport, binding));
    now.store(10, Ordering::SeqCst);
    {
        let mut records = bridge.lock();
        assert!(
            records
                .window_resources
                .reserve_pointer_callback(viewport, binding, window, first, true,)
        );
        assert!(records.record_viewport_pointer_passthrough(callback));
    }
    now.store(20, Ordering::SeqCst);
    bridge.push_record(cursor(3, window, viewport, binding));
    now.store(50, Ordering::SeqCst);
    {
        let records = bridge.lock();
        assert_eq!(records.journal.captured_through(), 3);
        assert_eq!(records.journal.len(), 2);
        assert_eq!(records.raw_prefix().count(), 1);
        assert_eq!(records.retained_record_count(), 3);
        assert_eq!(records.oldest_record_age(), Some(Duration::from_millis(50)));
        let ingress = records.window_resources.callback_ingress().next().unwrap();
        assert_eq!(ingress.ordinal(), 2);
        assert_eq!(ingress.captured_at(), Duration::from_millis(10));
    }
    assert_eq!(bridge.front_event().unwrap().ordinal(), 1);
    assert!(bridge.front_viewport_pointer_passthrough().is_none());
    assert!(!bridge.acknowledge_event(3));
    assert!(!bridge.acknowledge_viewport_pointer_passthrough(callback));
    {
        let claim = bridge.claim_cursor_prefix(256).unwrap();
        assert_eq!(claim.record_count().get(), 1);
        assert_eq!(
            claim
                .events()
                .map(|event| event.ordinal())
                .collect::<Vec<_>>(),
            [1]
        );
    }
    let mut budget = NativeInputBudget::new(&NativeMonotonicClock::new(|| Duration::ZERO));
    assert!(!bridge.coalesce_idle_cursor_prefix(&mut budget));
    assert_eq!(bridge.front_event().unwrap().ordinal(), 1);
    assert_eq!(bridge.lock().retained_record_count(), 3);

    assert!(bridge.acknowledge_event(1));
    assert!(bridge.claim_cursor_prefix(256).is_none());
    bridge.commit_frame_boundary(true);
    assert_eq!(bridge.lock().retained_record_count(), 3);
    assert_eq!(
        bridge.lock().oldest_record_age(),
        Some(Duration::from_millis(50))
    );
    bridge.commit_frame_boundary(false);
    assert_eq!(bridge.front_viewport_pointer_passthrough(), Some(callback));
    assert!(bridge.front_event().is_none());
    assert!(bridge.claim_cursor_prefix(256).is_none());
    assert!(!bridge.coalesce_idle_cursor_prefix(&mut budget));
    now.store(100, Ordering::SeqCst);
    {
        let mut records = bridge.lock();
        assert_eq!(records.retained_record_count(), 2);
        assert_eq!(records.oldest_record_age(), Some(Duration::from_millis(90)));
        assert_eq!(
            records.journal.oldest_age(),
            Some(Duration::from_millis(80))
        );
        assert!(
            !records
                .window_resources
                .reserve_pointer_callback(viewport, binding, window, next, false,)
        );
    }
    assert!(
        !bridge.acknowledge_viewport_pointer_passthrough(callback.qualified_for_binding(foreign),)
    );
    assert!(bridge.acknowledge_viewport_pointer_passthrough(callback));
    assert!(!bridge.acknowledge_viewport_pointer_passthrough(callback));
    {
        let mut records = bridge.lock();
        assert_eq!(records.window_resources.callback_ingress().count(), 0);
        assert_eq!(records.retained_record_count(), 2);
        assert_eq!(records.oldest_record_age(), Some(Duration::from_millis(90)));
        assert!(
            !records
                .window_resources
                .reserve_pointer_callback(viewport, binding, window, next, false,)
        );
    }
    bridge.commit_frame_boundary(true);
    assert!(bridge.callback_boundary_pending());
    {
        let mut records = bridge.lock();
        assert_eq!(records.retained_record_count(), 2);
        assert_eq!(records.oldest_record_age(), Some(Duration::from_millis(90)));
        assert!(
            !records
                .window_resources
                .reserve_pointer_callback(viewport, binding, window, next, false,)
        );
    }
    bridge.commit_frame_boundary(false);
    {
        let mut records = bridge.lock();
        assert_eq!(records.retained_record_count(), 1);
        assert_eq!(records.oldest_record_age(), Some(Duration::from_millis(80)));
        assert_eq!(records.journal.captured_through(), 3);
        assert!(!records.window_resources.has_pending_work());
        assert!(
            records
                .window_resources
                .reserve_pointer_callback(viewport, binding, window, next, false,)
        );
    }
    assert_eq!(bridge.front_event().unwrap().ordinal(), 3);
    assert!(bridge.acknowledge_event(3));
    bridge.commit_frame_boundary(false);
    assert_eq!(bridge.lock().retained_record_count(), 0);
    assert_eq!(bridge.lock().oldest_record_age(), None);
}

#[test]
fn external_callback_stays_raw_and_exact_duplicates_never_recapture_ingress() {
    let (binding, _) = bindings();
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(21);
    let context = egui::Context::default();
    let reserved = eframe::queue_native_viewport_pointer_passthrough(&context, viewport, true);
    let external = eframe::queue_native_viewport_pointer_passthrough(&context, viewport, false);
    let callback = pointer_callback(reserved, viewport, window, Some(binding), true);
    let ordinary = pointer_callback(external, viewport, window, None, false);
    let bridge = NativeHostBridge::new(NativeViewportMap::default());
    {
        let mut records = bridge.lock();
        assert!(
            records
                .window_resources
                .reserve_pointer_callback(viewport, binding, window, reserved, true,)
        );
        assert!(records.record_viewport_pointer_passthrough(ordinary));
        assert_eq!(records.window_resources.callback_ingress().count(), 0);
        assert!(
            matches!(records.journal.front(), Some(HostRecord::ViewportPointerPassthrough(record)) if *record == ordinary)
        );
        assert!(records.record_viewport_pointer_passthrough(callback));
        assert_eq!(records.journal.len(), 1);
        assert_eq!(records.retained_record_count(), 2);
        assert_eq!(records.journal.captured_through(), 2);
        assert!(!records.record_viewport_pointer_passthrough(callback));
        assert_eq!(records.journal.captured_through(), 2);
        assert_eq!(records.retained_record_count(), 2);
    }
    assert_eq!(bridge.front_viewport_pointer_passthrough(), Some(ordinary));
    assert!(!bridge.acknowledge_viewport_pointer_passthrough(callback));
    assert!(bridge.acknowledge_viewport_pointer_passthrough(ordinary));
    bridge.commit_frame_boundary(false);
    assert_eq!(bridge.front_viewport_pointer_passthrough(), Some(callback));
    assert!(bridge.acknowledge_viewport_pointer_passthrough(callback));
    bridge.commit_frame_boundary(true);
    {
        let mut records = bridge.lock();
        assert!(!records.record_viewport_pointer_passthrough(callback));
        assert_eq!(records.journal.captured_through(), 2);
        assert_eq!(records.retained_record_count(), 1);
        assert_eq!(records.window_resources.callback_ingress().count(), 0);
    }
    bridge.commit_frame_boundary(false);
    {
        let mut records = bridge.lock();
        assert!(!records.record_viewport_pointer_passthrough(callback));
        assert_eq!(records.journal.captured_through(), 2);
        assert_eq!(records.retained_record_count(), 0);
        assert_eq!(records.oldest_record_age(), None);
    }
    assert!(!bridge.has_pending_coordinator_work());
}

#[test]
fn fatal_rearms_terminal_roster_for_an_owner_held_pointer_callback() {
    let (binding, _) = bindings();
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(51);
    let roster = NativeViewportRosterRecord::for_test(
        [CompiledWindowObservation::new(
            binding,
            NativeWindowFacts::live(),
        )],
        true,
    );
    for with_raw_prefix in [false, true] {
        let bridge = NativeHostBridge::new(NativeViewportMap::default());

        // Settle an earlier obligation so fatal must rearm a fresh generation.
        bridge.require_terminal_roster();
        bridge.push_terminal_viewport_roster(roster.clone());
        let baseline = bridge.front_viewport_roster().unwrap();
        assert!(bridge.acknowledge_viewport_roster(&baseline));
        assert!(bridge.settle_viewport_roster(&baseline, true));
        bridge.commit_frame_boundary(false);
        {
            let records = bridge.lock();
            assert!(!records.terminal_roster.needs_capture());
            assert!(!records.terminal_roster.has_pending());
            assert_eq!(records.retained_record_count(), 0);
        }
        let context = egui::Context::default();
        let token = eframe::queue_native_viewport_pointer_passthrough(&context, viewport, true);
        let callback = pointer_callback(token, viewport, window, Some(binding), true);
        if with_raw_prefix {
            bridge.push_record(cursor(2, window, viewport, binding));
        }
        {
            let mut records = bridge.lock();
            assert!(
                records
                    .window_resources
                    .reserve_pointer_callback(viewport, binding, window, token, true,)
            );
            assert!(records.record_viewport_pointer_passthrough(callback));
            assert_eq!(records.journal.len(), usize::from(with_raw_prefix));
            assert_eq!(records.window_resources.callback_ingress().count(), 1);
            assert!(!records.terminal_roster.needs_capture());
            assert!(!records.terminal_roster.has_pending());
        }

        assert!(bridge.quarantine_after_fatal().is_empty());
        assert!(bridge.lock().terminal_roster.needs_capture());
        assert!(bridge.lock().terminal_roster.has_pending());
        if with_raw_prefix {
            assert_eq!(bridge.front_event().unwrap().ordinal(), 2);
            assert!(bridge.acknowledge_event(2));
            bridge.commit_frame_boundary(false);
            assert!(bridge.lock().terminal_roster.needs_capture());
        }
        assert_eq!(bridge.front_viewport_pointer_passthrough(), Some(callback));
        assert!(bridge.acknowledge_viewport_pointer_passthrough(callback));
        bridge.commit_frame_boundary(false);
        {
            let records = bridge.lock();
            assert!(records.journal.is_empty());
            assert_eq!(records.window_resources.callback_ingress().count(), 0);
            assert_eq!(records.retained_record_count(), 0);
            assert!(records.terminal_roster.needs_capture());
            assert!(records.terminal_roster.has_pending());
        }
        assert!(bridge.has_pending_coordinator_work());

        bridge.push_terminal_viewport_roster(roster.clone());
        let terminal = bridge.front_viewport_roster().unwrap();
        assert!(terminal.is_terminal());
        assert_eq!(terminal.roster(), &roster);
        assert!(!bridge.lock().terminal_roster.needs_capture());
        assert!(bridge.lock().terminal_roster.has_pending());
        assert!(bridge.acknowledge_viewport_roster(&terminal));
        assert!(bridge.lock().terminal_roster.has_pending());
        assert!(bridge.settle_viewport_roster(&terminal, true));
        bridge.commit_frame_boundary(false);
        {
            let mut records = bridge.lock();
            assert!(!records.terminal_roster.needs_capture());
            assert!(!records.terminal_roster.has_pending());
            assert_eq!(records.retained_record_count(), 0);
            assert!(!records.record_terminal_viewport_roster(roster.clone()));
        }
        assert!(bridge.front_viewport_roster().is_none());
        assert!(!bridge.has_pending_coordinator_work());
    }
}

#[test]
fn queued_property_retirement_keeps_its_exact_owner_until_ack_and_full_commit() {
    let (first, next) = bindings();
    let viewport = ViewportId::from_hash_of("retired-queued-property-command");
    let old_window = WindowId::from(41);
    let new_window = WindowId::from(42);
    let context = egui::Context::default();
    let first_token = eframe::queue_native_viewport_pointer_passthrough(&context, viewport, true);
    let next_token = eframe::queue_native_viewport_pointer_passthrough(&context, viewport, false);
    let replacement_token =
        eframe::queue_native_viewport_pointer_passthrough(&context, viewport, true);
    let retirement = NativeViewportPropertyCommandRetirement::PointerPassthrough {
        viewport_id: viewport,
        token: first_token,
        enabled: true,
    };
    let expected = NativePointerPassthroughRecord::for_test(
        first_token,
        viewport,
        old_window,
        Some(first),
        true,
        NativeViewportPointerPassthroughStatus::Retired,
    );
    let (bridge, now) = bridge_with_clock();
    let rect = NativePhysicalRect::new(0, 0, 640, 480);
    {
        let mut records = bridge.lock();
        assert!(records.window_resources.reserve_pointer_callback(
            viewport,
            first,
            old_window,
            first_token,
            true,
        ));
        assert!(records.retire_deferred_binding(viewport, first).is_empty());
        assert!(
            records
                .window_resources
                .reserve_create(viewport, next, rect)
        );
        assert!(
            records
                .window_resources
                .reserve_pointer_callback(viewport, next, new_window, next_token, false,)
        );
        records.viewports.reserve(viewport, next).unwrap();
        records.block_semantic_viewport(viewport);
    }
    bridge.commit_frame_boundary(false);
    {
        let records = bridge.lock();
        assert!(records.window_resources.references_binding(first));
        assert!(
            records
                .window_resources
                .references_viewport_binding(viewport, next)
        );
        assert_eq!(records.window_resources.callback_ingress().count(), 0);
        assert_eq!(records.journal.captured_through(), 0);
        assert_eq!(records.retained_record_count(), 0);
    }
    bridge.block_semantic_viewport(viewport);
    assert_eq!(
        NativeHostHandler::on_viewport_property_command_retired(
            &bridge,
            NativeViewportPropertyCommandRetirement::PointerPassthrough {
                viewport_id: viewport,
                token: first_token,
                enabled: false,
            },
        ),
        NativeHostWake::Wait,
    );
    assert_eq!(bridge.lock().journal.captured_through(), 0);
    assert!(bridge.front_viewport_pointer_passthrough().is_none());
    now.store(10, Ordering::SeqCst);
    assert_eq!(
        NativeHostHandler::on_viewport_property_command_retired(&bridge, retirement),
        NativeHostWake::RepaintRootAfterCurrent,
    );
    assert_eq!(
        NativeHostHandler::on_viewport_property_command_retired(&bridge, retirement),
        NativeHostWake::Wait,
    );
    assert_eq!(bridge.front_viewport_pointer_passthrough(), Some(expected));
    {
        let records = bridge.lock();
        assert_eq!(records.journal.captured_through(), 1);
        assert!(records.journal.is_empty());
        assert_eq!(records.retained_record_count(), 1);
        let ingress = records.window_resources.callback_ingress().next().unwrap();
        assert_eq!(ingress.ordinal(), 1);
        assert_eq!(ingress.captured_at(), Duration::from_millis(10));
        assert!(records.viewports.semantic_viewport_blocked(viewport));
    }
    bridge.commit_frame_boundary(true);
    assert!(bridge.lock().window_resources.references_binding(first));
    assert_eq!(bridge.front_viewport_pointer_passthrough(), Some(expected));
    assert!(
        !bridge.acknowledge_viewport_pointer_passthrough(expected.qualified_for_binding(next),)
    );
    assert!(bridge.acknowledge_viewport_pointer_passthrough(expected));
    bridge.commit_frame_boundary(true);
    now.store(100, Ordering::SeqCst);
    assert_eq!(
        NativeHostHandler::on_viewport_property_command_retired(&bridge, retirement),
        NativeHostWake::Wait,
    );
    {
        let records = bridge.lock();
        assert!(records.window_resources.references_binding(first));
        assert_eq!(records.window_resources.callback_ingress().count(), 0);
        assert_eq!(records.journal.captured_through(), 1);
        assert_eq!(records.retained_record_count(), 1);
        assert_eq!(records.oldest_record_age(), Some(Duration::from_millis(90)));
        assert!(records.viewports.semantic_viewport_blocked(viewport));
    }
    bridge.commit_frame_boundary(false);
    {
        let mut records = bridge.lock();
        assert!(!records.window_resources.references_binding(first));
        assert!(
            records
                .window_resources
                .references_viewport_binding(viewport, next)
        );
        assert_eq!(
            records.window_resources.create(viewport).unwrap().binding,
            next
        );
        assert_eq!(
            records.window_resources.hidden_binding(viewport),
            Some(next)
        );
        assert_eq!(records.journal.captured_through(), 1);
        assert_eq!(records.retained_record_count(), 0);
        assert_eq!(records.oldest_record_age(), None);
        assert!(!records.window_resources.reserve_pointer_callback(
            viewport,
            next,
            new_window,
            replacement_token,
            true,
        ));
    }
    assert!(bridge.front_viewport_pointer_passthrough().is_none());
}

#[test]
fn retired_awaiting_callback_keeps_its_owner_and_successors_blocked_boundary() {
    assert_retired_callback_order(false);
}

#[test]
fn retired_received_callback_keeps_its_owner_and_successors_blocked_boundary() {
    assert_retired_callback_order(true);
}

fn assert_retired_callback_order(received_before_retirement: bool) {
    let (first, next) = bindings();
    let viewport = ViewportId::from_hash_of("retired-command-ingress");
    let old_window = WindowId::from(31);
    let new_window = WindowId::from(32);
    let context = egui::Context::default();
    let first_token = eframe::queue_native_viewport_pointer_passthrough(&context, viewport, true);
    let next_token = eframe::queue_native_viewport_pointer_passthrough(&context, viewport, false);
    let callback = pointer_callback(first_token, viewport, old_window, Some(first), true);
    let late = callback.qualified_for_binding(next);
    let bridge = NativeHostBridge::new(NativeViewportMap::default());
    let rect = NativePhysicalRect::new(0, 0, 640, 480);
    bridge.push_record(cursor(1, old_window, viewport, first));
    {
        let mut records = bridge.lock();
        assert!(records.window_resources.reserve_pointer_callback(
            viewport,
            first,
            old_window,
            first_token,
            true,
        ));
        if received_before_retirement {
            assert!(records.record_viewport_pointer_passthrough(callback));
        }
        assert!(records.retire_deferred_binding(viewport, first).is_empty());
        assert!(records.window_resources.references_binding(first));
        assert!(
            records
                .window_resources
                .reserve_create(viewport, next, rect)
        );
        assert!(
            records
                .window_resources
                .reserve_pointer_callback(viewport, next, new_window, next_token, false,)
        );
        records.viewports.reserve(viewport, next).unwrap();
        records.block_semantic_viewport(viewport);
        assert!(records.retire_deferred_binding(viewport, first).is_empty());
        assert!(records.viewports.semantic_viewport_blocked(viewport));
        assert!(!records.viewports.semantic_viewport_ready(viewport));
        assert_eq!(
            records.record_viewport_pointer_passthrough(late),
            !received_before_retirement,
        );
        assert_eq!(records.journal.captured_through(), 2);
        assert_eq!(records.retained_record_count(), 2);
        let ingress = records.window_resources.callback_ingress().next().unwrap();
        assert_eq!(ingress.ordinal(), 2);
        assert!(
            matches!(ingress.record(), HostRecord::ViewportPointerPassthrough(record) if *record == callback)
        );
        assert_eq!(
            records.window_resources.create(viewport).unwrap().binding,
            next
        );
        assert!(records.viewports.semantic_viewport_blocked(viewport));
    }
    bridge.push_record(cursor(3, new_window, viewport, next));
    assert_eq!(bridge.front_event().unwrap().ordinal(), 1);
    assert!(!bridge.acknowledge_viewport_pointer_passthrough(callback));
    assert!(bridge.acknowledge_event(1));
    bridge.commit_frame_boundary(false);
    assert_eq!(bridge.front_viewport_pointer_passthrough(), Some(callback));
    assert!(bridge.front_event().is_none());
    assert!(!bridge.acknowledge_viewport_pointer_passthrough(late));
    {
        let records = bridge.lock();
        assert!(records.window_resources.references_binding(first));
        assert!(records.viewports.semantic_viewport_blocked(viewport));
        assert!(!records.viewports.semantic_viewport_ready(viewport));
    }
    assert!(bridge.acknowledge_viewport_pointer_passthrough(callback));
    bridge.commit_frame_boundary(true);
    {
        let records = bridge.lock();
        assert!(records.window_resources.references_binding(first));
        assert!(
            records
                .window_resources
                .references_viewport_binding(viewport, next)
        );
        assert_eq!(records.retained_record_count(), 2);
        assert!(records.viewports.semantic_viewport_blocked(viewport));
    }
    bridge.commit_frame_boundary(false);
    {
        let records = bridge.lock();
        assert!(!records.window_resources.references_binding(first));
        assert!(
            records
                .window_resources
                .references_viewport_binding(viewport, next)
        );
        assert_eq!(
            records.window_resources.create(viewport).unwrap().binding,
            next
        );
        assert_eq!(records.retained_record_count(), 1);
        assert!(records.viewports.semantic_viewport_blocked(viewport));
        assert!(!records.viewports.semantic_viewport_ready(viewport));
    }
    assert_eq!(bridge.front_event().unwrap().ordinal(), 3);
    assert!(bridge.acknowledge_event(3));
    bridge.commit_frame_boundary(false);
    assert_eq!(bridge.take_ready_semantic_viewports(), [viewport]);
}

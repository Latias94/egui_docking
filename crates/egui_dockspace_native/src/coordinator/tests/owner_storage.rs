use std::num::NonZeroUsize;

use dockspace::runtime::{
    NativeInputStorageLimit, NativeProducerStorageLimit, NativeReportStorageLimit,
};

use super::*;
use crate::input_control::NativePointerPassthroughRecord;

pub(super) fn layout() -> DockspaceLayout {
    DockspaceLayout::new([1, 2].map(|id| {
        DockspaceSurfaceLayout::new(
            SurfaceId::new(id),
            DockspaceRootLayout::new(
                RootId::new(id),
                DockspaceNode::central_tabs([ItemId::new(id)]),
            ),
        )
    }))
    .unwrap()
}

fn input_limit() -> NativeInputStorageLimit {
    NativeInputStorageLimit::new(NonZeroUsize::new(16).unwrap(), 1 << 20)
        .with_producer_storage_limit(NativeProducerStorageLimit::new(
            NonZeroUsize::new(2).unwrap(),
            NonZeroUsize::MIN,
            1 << 20,
        ))
        .with_report_storage_limit(NativeReportStorageLimit::new(
            NonZeroUsize::new(8).unwrap(),
            NonZeroUsize::new(2).unwrap(),
            1 << 20,
        ))
}

fn owners(resources: usize, commands: usize) -> NativeOwnerStorageLimit {
    NativeOwnerStorageLimit {
        resource_slots: resources,
        command_slots: commands,
        close_slots: 2,
        byte_limit: NativeOwnerStorageLimit::required_bytes(2, 2, resources, commands, 2).unwrap(),
    }
}

fn bounded(resources: usize, commands: usize) -> NativeCoordinator {
    NativeCoordinator::new_with_storage_limits(
        DockspaceSession::from_layout(layout(), DockPolicy::default()).unwrap(),
        NativePointerRoster::Exact(Vec::new()),
        Some(input_limit()),
        Some(owners(resources, commands)),
    )
    .unwrap()
}

#[test]
fn owner_configuration_requires_real_producer_and_report_bounds_and_the_complete_prepaid_bytes() {
    let config = owners(2, 2);
    for selected in [
        None,
        Some(NativeInputStorageLimit::new(
            NonZeroUsize::new(16).unwrap(),
            1 << 20,
        )),
    ] {
        let error = NativeCoordinator::new_with_storage_limits(
            DockspaceSession::from_layout(layout(), DockPolicy::default()).unwrap(),
            NativePointerRoster::Exact(Vec::new()),
            selected,
            Some(config),
        )
        .unwrap_err();
        assert_eq!(
            error.kind(),
            crate::NativeRuntimeErrorKind::InvalidConfiguration
        );
    }
    let mut short = config;
    short.byte_limit -= 1;
    assert!(
        NativeCoordinator::new_with_storage_limits(
            DockspaceSession::from_layout(layout(), DockPolicy::default()).unwrap(),
            NativePointerRoster::Exact(Vec::new()),
            Some(input_limit()),
            Some(short),
        )
        .unwrap_err()
        .is_input_capacity()
    );
    assert!(NativeOwnerStorageLimit::required_bytes(2, 2, usize::MAX, 2, 2).is_none());
    let native = bounded(2, 2);
    assert_eq!(native.owner_storage.unwrap().byte_limit, config.byte_limit);
    assert!(
        native.input_control.owned_data_capacity_bytes()
            + native.effects.owned_data_capacity_bytes()
            + native.deferred_viewports.owned_data_capacity_bytes()
            <= config.byte_limit
    );
}

#[test]
fn zero_resource_capacity_refuses_a_root_route_before_any_backend_work_or_source_capture() {
    let mut native = bounded(0, 0);
    let (binding, _) = register_roots(&mut native);
    let window = WindowId::from(11);
    assert!(matches!(
        native.bind_viewport(ViewportId::ROOT, window, binding),
        Err(NativeViewportBindingError::CapacityExceeded { limit: 0 }),
    ));
    let before = native.owned_input_capacity_bytes().unwrap();
    let recorded = native.session.native_input_recorded_count();
    let prepared = eframe::prepare_native_viewport_focus(ViewportId::ROOT);
    assert!(!native.bridge.reserve_focus_callback(
        ViewportId::ROOT,
        binding,
        window,
        prepared.token()
    ));
    drop(prepared);
    assert_eq!(native.session.native_input_recorded_count(), recorded);
    assert_eq!(native.owned_input_capacity_bytes().unwrap(), before);
    assert!(!native.bridge.has_pending_callback_records());
    assert_eq!(native.viewport_binding(ViewportId::ROOT), None);
}

#[test]
fn manual_quiescence_shares_the_automatic_guard_for_stable_input_observations() {
    let mut native = bounded(1, 1);
    let (binding, _) = register_roots(&mut native);
    let prepared = eframe::prepare_native_viewport_pointer_passthrough(ViewportId::ROOT, false);
    // Install a stable sidecar directly to isolate the quiescence boundary.
    // This fixture does not claim a physical callback or window destruction.
    let observation = NativePointerPassthroughRecord::for_test(
        prepared.token(),
        ViewportId::ROOT,
        WindowId::from(11),
        Some(binding),
        false,
        eframe::NativeViewportPointerPassthroughStatus::Applied,
    );
    assert!(
        native
            .input_control
            .observe_unowned_applied(observation)
            .unwrap()
    );
    drop(prepared);
    assert!(!native.input_control.has_pending_work());
    let replacement = DockspaceLayout::empty();
    let reset = native.session.prepare_layout_reset(replacement).unwrap();
    let mut frame = native
        .session
        .begin_native_layout_reset_frame(&reset, |_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    assert!(!native.session.is_current_native_binding(binding));
    assert!(native.session.recognizes_native_binding(binding));
    let recorded = native.session.native_input_recorded_count();
    let backing = native.input_control.owned_data_capacity_bytes();
    assert!(native.binding_has_retained_host_references(binding));
    assert_eq!(
        native
            .report_binding_quiescence(binding)
            .unwrap_err()
            .kind(),
        crate::NativeRuntimeErrorKind::OperationConflict
    );
    assert_eq!(native.session.native_input_recorded_count(), recorded);
    assert!(native.session.recognizes_native_binding(binding));
    assert_eq!(native.input_control.owned_data_capacity_bytes(), backing);
    native.retain_retired_input_results(binding);
    assert!(!native.binding_has_retained_host_references(binding));
    assert_eq!(native.input_control.owned_data_capacity_bytes(), backing);
}

#[test]
fn finite_no_frame_preparation_reuses_one_ready_backing_and_keeps_the_exact_owner_until_commit() {
    let mut native = bounded(2, 1);
    let (first, second) = register_roots(&mut native);
    let child = ViewportId::from_hash_of("prepaid-no-frame-child");
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(11), first)
        .unwrap();
    native
        .bind_viewport(child, WindowId::from(22), second)
        .unwrap();
    let invalidation =
        PendingNoFrameInvalidation::terminal_observed_for_test(child, WindowId::from(22), second);
    native
        .bridge
        .retain_no_frame(child, second, invalidation)
        .unwrap();
    let baseline = native.owned_input_capacity_bytes().unwrap();
    let mut prepared = native.prepare_no_frame_invalidations().unwrap();
    assert_eq!(prepared.ready, [invalidation]);
    assert_eq!(prepared.ready.capacity(), 2);
    assert_eq!(prepared.repaint_viewports.capacity(), 2);
    let ready_backing = prepared.ready.as_ptr();
    let repaint_backing = prepared.repaint_viewports.as_ptr();
    prepared.select_for_surfaces(&[], true);
    assert!(prepared.ready.is_empty());
    assert_eq!(prepared.repaint_viewports, [child]);
    assert_eq!(prepared.ready.as_ptr(), ready_backing);
    assert_eq!(prepared.repaint_viewports.as_ptr(), repaint_backing);
    drop(prepared);
    assert_eq!(
        native.bridge.no_frame_for_binding(second),
        Some(invalidation)
    );
    assert_eq!(native.owned_input_capacity_bytes().unwrap(), baseline);
    let mut retry = native.prepare_no_frame_invalidations().unwrap();
    retry.select_for_surfaces(&[second.surface()], true);
    assert_eq!(retry.ready, [invalidation]);
    assert!(retry.repaint_viewports.is_empty());
    native.finish_no_frame_invalidations(&[]);
    assert_eq!(native.bridge.pending_no_frame_count(), 1);
    native.finish_no_frame_invalidations(&retry.ready);
    assert_eq!(native.bridge.pending_no_frame_count(), 0);
    assert_eq!(native.owned_input_capacity_bytes().unwrap(), baseline);
    // The live route retains the original resource row after observations clear.
    native
        .bridge
        .retain_no_frame(child, second, invalidation)
        .unwrap();
    assert_eq!(native.owned_input_capacity_bytes().unwrap(), baseline);
}

pub(super) fn combined_profile() -> NativeCombinedStorageLimit {
    let windows = 16;
    let effects = 16;
    NativeCombinedStorageLimit {
        input: NativeInputStorageLimit::new(NonZeroUsize::new(512).unwrap(), 768 * 1024)
            .with_producer_storage_limit(NativeProducerStorageLimit::new(
                NonZeroUsize::new(windows).unwrap(),
                NonZeroUsize::new(8).unwrap(),
                128 * 1024,
            ))
            .with_report_storage_limit(NativeReportStorageLimit::new(
                NonZeroUsize::new(64).unwrap(),
                NonZeroUsize::new(effects).unwrap(),
                256 * 1024,
            )),
        owners: NativeOwnerStorageLimit {
            resource_slots: 48,
            command_slots: 32,
            close_slots: 16,
            byte_limit: NativeOwnerStorageLimit::required_bytes(windows, effects, 48, 32, 16)
                .unwrap(),
        },
        byte_limit: 4 * 1024 * 1024,
    }
}

fn combined() -> NativeCoordinator {
    NativeCoordinator::new_with_combined_storage_limit_and_clock(
        DockspaceSession::from_layout(layout(), DockPolicy::default()).unwrap(),
        NativePointerRoster::Exact(Vec::new()),
        combined_profile(),
        dockspace::runtime::NativeMonotonicClock::new(|| std::time::Duration::ZERO),
    )
    .unwrap()
}

#[test]
fn combined_initial_admission_checks_the_full_promise_before_any_provider_can_be_returned() {
    let profile = combined_profile();
    let required = profile.initial_required_bytes().unwrap();
    assert!(
        required <= profile.byte_limit,
        "complete initial promise {required} exceeds {}",
        profile.byte_limit
    );
    let mut short = profile;
    short.byte_limit = required - 1;
    assert!(
        NativeCoordinator::new_with_combined_storage_limit(
            DockspaceSession::from_layout(layout(), DockPolicy::default()).unwrap(),
            NativePointerRoster::Exact(Vec::new()),
            short,
        )
        .unwrap_err()
        .is_input_capacity()
    );
    let mut exact = profile;
    exact.byte_limit = required;
    let native = NativeCoordinator::new_with_combined_storage_limit(
        DockspaceSession::from_layout(layout(), DockPolicy::default()).unwrap(),
        NativePointerRoster::Exact(Vec::new()),
        exact,
    )
    .unwrap();
    assert_eq!(native.combined_storage_limit, Some(required));
    let reserved = native.combined_storage_reserved_bytes().unwrap();
    assert!(reserved <= required);
    assert!(native.owned_input_capacity_bytes().unwrap() <= reserved);
    assert_eq!(
        native
            .session
            .native_input_producer_storage_reserved_bytes(),
        Some(128 * 1024)
    );
    assert_eq!(
        native.session.native_input_report_storage_reserved_bytes(),
        Some(256 * 1024)
    );
}

#[test]
fn selected_combined_allowance_freezes_pointer_ingress_before_minting_an_unfunded_source() {
    use winit::dpi::PhysicalPosition;
    use winit::event::{DeviceId, PointerEventFacts};

    let mut native = combined();
    let (binding, _) = register_roots(&mut native);
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(11), binding)
        .unwrap();
    let version = native.session.version();
    let recorded = native.session.native_input_recorded_count();
    let mut accepted = 0;
    for ordinal in 1..=4096 {
        let captured_before = native.bridge.captured_through_for_test();
        native
            .bridge
            .push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
                ordinal,
                WindowId::from(11),
                Some(ViewportId::ROOT),
                Some(binding),
                WindowEvent::CursorMoved {
                    device_id: DeviceId::dummy(),
                    position: PhysicalPosition::new(ordinal as f64, 20.0),
                    facts: PointerEventFacts::default(),
                },
            )));
        let reserved = native.combined_storage_reserved_bytes().unwrap();
        assert!(reserved <= 4 * 1024 * 1024);
        assert!(native.owned_input_capacity_bytes().unwrap() <= reserved);
        if !native.bridge.output_order_is_valid() {
            panic!("capacity rejection must not alter output order");
        }
        if native.bridge.captured_through_for_test() == captured_before {
            break;
        }
        accepted += 1;
    }
    assert!((1..4096).contains(&accepted));
    assert_eq!(native.session.version(), version);
    assert_eq!(native.session.native_input_recorded_count(), recorded);
    let before = native.combined_storage_reserved_bytes().unwrap();
    let captured = native.bridge.captured_through_for_test();
    native
        .bridge
        .push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            5000,
            WindowId::from(11),
            Some(ViewportId::ROOT),
            Some(binding),
            WindowEvent::CursorLeft {
                device_id: DeviceId::dummy(),
            },
        )));
    assert_eq!(native.combined_storage_reserved_bytes(), Some(before));
    assert_eq!(native.bridge.captured_through_for_test(), captured);

    let context = egui::Context::default();
    // Freezing first retains the complete pre-existing core boundary. Commit
    // that prelude before attempting to consume its frozen callback prefix.
    let mut prelude = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    prelude
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    assert!(
        !prelude
            .commit()
            .unwrap()
            .inputs()
            .contains(&HostInputOutcome::NativeInputCancelledForOverload)
    );
    let mut dropped_prefix = false;
    let mut consumed = 0;
    while native.bridge.front_event().is_some() {
        let mut budget = test_budget();
        let remaining = budget.remaining_records();
        assert!(
            native
                .reduce_callback_prefix(&context, &mut budget)
                .unwrap()
                .is_some()
        );
        let mut frame = native
            .poll_host_frame(&mut budget, |_| NativeReceiverAnswer::Unknown)
            .unwrap()
            .expect("the same dispatch budget also reduces the retained core pointer edge");
        consumed += remaining - budget.remaining_records();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        if !dropped_prefix {
            let version = frame.frame.version();
            drop(frame);
            assert_eq!(native.session.version(), version);
            dropped_prefix = true;
            let mut retry = native
                .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
                .unwrap();
            retry
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            assert!(
                !retry
                    .commit()
                    .unwrap()
                    .inputs()
                    .contains(&HostInputOutcome::NativeInputCancelledForOverload)
            );
        } else {
            assert!(
                !frame
                    .commit()
                    .unwrap()
                    .inputs()
                    .contains(&HostInputOutcome::NativeInputCancelledForOverload)
            );
        }
        assert!(native.combined_storage_reserved_bytes().unwrap() <= 4 * 1024 * 1024);
    }
    assert_eq!(consumed, accepted);
    assert_eq!(native.bridge.captured_through_for_test(), captured);
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::Cancel { boundary: captured }
    );
    let mut cancelled = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    cancelled
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    drop(cancelled);
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::Cancel { boundary: captured }
    );
    let mut retry = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    retry
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = retry.commit().unwrap();
    assert_eq!(
        report
            .inputs()
            .iter()
            .filter(|outcome| **outcome == HostInputOutcome::NativeInputCancelledForOverload)
            .count(),
        1
    );
    assert_eq!(native.bridge.admission_dispatch(), NativeDispatch::Idle);
    assert!(native.combined_storage_reserved_bytes().unwrap() < before);
    native
        .bridge
        .push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            6000,
            WindowId::from(11),
            Some(ViewportId::ROOT),
            Some(binding),
            WindowEvent::CursorLeft {
                device_id: DeviceId::dummy(),
            },
        )));
    assert_eq!(native.bridge.captured_through_for_test(), captured + 1);
    assert_eq!(native.bridge.front_event().unwrap().ordinal(), 6000);
    assert!(native.combined_storage_reserved_bytes().unwrap() <= 4 * 1024 * 1024);
}

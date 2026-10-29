use std::num::NonZeroUsize;

use dockspace::runtime::{
    NativeInputStorageLimit, NativeProducerStorageLimit, NativeReportStorageLimit,
};

use super::*;
use crate::input_control::NativePointerPassthroughRecord;

fn layout() -> DockspaceLayout {
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
        byte_limit: NativeOwnerStorageLimit::required_bytes(2, 2, resources, commands).unwrap(),
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
    assert!(NativeOwnerStorageLimit::required_bytes(2, 2, usize::MAX, 2).is_none());
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
fn zero_resource_capacity_refuses_a_prepared_command_before_any_backend_work_or_source_capture() {
    let mut native = bounded(0, 0);
    let (binding, _) = register_roots(&mut native);
    let window = WindowId::from(11);
    native
        .bind_viewport(ViewportId::ROOT, window, binding)
        .unwrap();
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
    assert_eq!(native.viewport_binding(ViewportId::ROOT), Some(binding));
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

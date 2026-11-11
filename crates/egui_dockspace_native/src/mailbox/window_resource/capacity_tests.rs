use dockspace::runtime::NativeMonotonicClock;
use eframe::{NativeViewportPointerPassthroughStatus, egui};

use super::super::journal::HostIngressJournal;
use super::tests::bindings;
use super::*;

#[test]
fn shared_placements_reuse_one_window_slot_without_retagging_pending_callbacks() {
    use dockspace::geometry::{LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor};
    use dockspace::model::{
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
        RootId, SurfaceId,
    };
    use dockspace::runtime::{
        DockspaceSession, HostWindowToken, NativePointerRoster, NativeSurfacePlacement,
        SurfaceUnavailableReason,
    };

    let layout = DockspaceLayout::new([1, 2].map(|id| {
        DockspaceSurfaceLayout::new(
            SurfaceId::new(id),
            DockspaceRootLayout::new(
                RootId::new(id),
                DockspaceNode::central_tabs([ItemId::new(id)]),
            ),
        )
    }))
    .unwrap();
    let mut session =
        DockspaceSession::from_layout(layout.clone(), dockspace::policy::DockPolicy::default())
            .unwrap();
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    session
        .register_native_root(SurfaceId::new(1), HostWindowToken::new(1))
        .unwrap();
    let mut registration = session.begin_host_frame().unwrap();
    registration
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let original = registration.commit().unwrap().native_bindings()[0];
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(61);
    let context = egui::Context::default();
    let callback = eframe::queue_native_viewport_pointer_passthrough(&context, viewport, false);
    let mut resources = NativeWindowResources::with_capacity_limit(1).unwrap();
    let bytes = resources.owned_data_capacity_bytes();
    resources
        .prepare_host_reference(viewport, original)
        .unwrap();
    resources.commit_host_reference(viewport, original);
    assert!(resources.reserve_pointer_callback(viewport, original, window, callback, false));

    let placements = [1, 2].map(|id| {
        let x = if id == 1 { 0.0 } else { 320.0 };
        NativeSurfacePlacement::new(
            SurfaceId::new(id),
            LogicalSize::new(300.0, 240.0).unwrap(),
            PhysicalPoint::new(x, 0.0).unwrap(),
            ScaleFactor::new(1.0).unwrap(),
            PhysicalRect::new(x, 0.0, 300.0, 240.0).unwrap(),
        )
    });
    let mut candidate = session.begin_host_frame().unwrap();
    candidate
        .set_native_surface_placements(original.window(), &placements)
        .unwrap();
    candidate
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    candidate.commit().unwrap();
    for binding in session.native_surface_bindings_for_window(original.window()) {
        resources.prepare_host_reference(viewport, binding).unwrap();
        resources.commit_host_reference(viewport, binding);
        assert!(resources.references_viewport_binding(viewport, binding));
        assert!(resources.references_binding(binding));
        assert!(!resources.references_placement(binding));
    }
    assert!(resources.references_placement(original));
    assert_eq!(resources.entries.len(), 1);
    assert_eq!(resources.entries[0].binding, original);
    assert_eq!(resources.owned_data_capacity_bytes(), bytes);
    let current = session
        .native_surface_bindings_for_window(original.window())
        .next()
        .unwrap();
    let no_frame = PendingNoFrameInvalidation {
        terminal: crate::coordinator::NoFrameTerminal::Observed,
        viewport,
        window,
        create_attempt: None,
        binding: current,
    };
    resources
        .retain_no_frame(viewport, current, no_frame)
        .unwrap();
    assert!(resources.clear_no_frame_if(no_frame));
    assert_eq!(resources.pending_no_frame().count(), 0);
    let mut replacement_resources = NativeWindowResources::with_capacity_limit(2).unwrap();
    replacement_resources
        .prepare_host_reference(viewport, original)
        .unwrap();
    replacement_resources.commit_host_reference(viewport, original);
    let reset = session.prepare_layout_reset(layout).unwrap();
    let mut replacement = session.begin_host_frame().unwrap();
    replacement.submit_layout_reset(&reset).unwrap();
    assert_eq!(replacement.proposed_native_root_continuations().count(), 2);
    for continuation in replacement.proposed_native_root_continuations() {
        replacement_resources
            .prepare_workspace_replacement(viewport, continuation.successor())
            .unwrap();
    }
    assert_eq!(
        replacement_resources.entries.len(),
        2,
        "the complete shared successor prepays one additional physical owner"
    );
    replacement_resources.abort_workspace_replacement();
    drop(replacement);
    assert_eq!(replacement_resources.entries.len(), 1);
    assert!(replacement_resources.references_viewport_binding(viewport, original));

    let record = NativePointerPassthroughRecord::for_test(
        callback,
        viewport,
        window,
        None,
        false,
        NativeViewportPointerPassthroughStatus::Applied,
    );
    let mut journal = HostIngressJournal::new(NativeMonotonicClock::system());
    assert_eq!(
        resources.accept_pointer_callback(record, |binding| {
            assert_eq!(binding, original);
            journal.reserve(HostRecord::ViewportPointerPassthrough(
                record.qualified_for_binding(binding),
            ))
        }),
        CommandCallbackAdmission::First,
    );
}

#[test]
fn one_entry_limit_charges_the_same_binding_on_another_viewport() {
    let (binding, _) = bindings();
    let viewport = ViewportId::ROOT;
    let peer = ViewportId::from_hash_of("bounded-resource-peer");
    let rect = NativePhysicalRect::new(0, 0, 640, 480);
    let window = WindowId::from(11);
    let context = egui::Context::default();
    let mut resources = NativeWindowResources::with_capacity_limit(1).unwrap();
    let bytes = resources.owned_data_capacity_bytes();
    let backing = resources.entries.as_ptr();

    assert!(resources.reserve_create(viewport, binding, rect));
    assert!(resources.reserve_pointer_callback(
        viewport,
        binding,
        window,
        eframe::queue_native_viewport_pointer_passthrough(&context, viewport, true),
        true,
    ));
    assert!(resources.reserve_focus_callback(
        viewport,
        binding,
        window,
        eframe::queue_native_viewport_focus(&context, viewport),
    ));
    assert!(resources.reserve_visibility_callback(
        viewport,
        binding,
        window,
        eframe::queue_native_viewport_visibility(&context, viewport, true),
        true,
    ));
    assert!(!resources.can_configure_capacity_limit());

    assert!(!resources.reserve_create(peer, binding, rect));
    assert!(!resources.reserve_pointer_callback(
        peer,
        binding,
        window,
        eframe::queue_native_viewport_pointer_passthrough(&context, peer, false),
        false,
    ));
    assert!(!resources.reserve_focus_callback(
        peer,
        binding,
        window,
        eframe::queue_native_viewport_focus(&context, peer),
    ));
    assert!(!resources.reserve_visibility_callback(
        peer,
        binding,
        window,
        eframe::queue_native_viewport_visibility(&context, peer, false),
        false,
    ));
    assert_eq!(resources.entries.len(), 1);
    assert_eq!(resources.hidden_binding(viewport), Some(binding));
    assert!(!resources.references_viewport_binding(peer, binding));
    assert_eq!(resources.owned_data_capacity_bytes(), bytes);
    assert_eq!(resources.entries.as_ptr(), backing);
    assert_eq!(resources.callback_ingress().count(), 0);
}

#[test]
fn two_entries_retain_the_old_received_owner_through_successor_admission_and_full_commit() {
    let (first, next) = bindings();
    let viewport = ViewportId::ROOT;
    let peer = ViewportId::from_hash_of("bounded-third-resource");
    let window = WindowId::from(21);
    let context = egui::Context::default();
    let token = eframe::queue_native_viewport_pointer_passthrough(&context, viewport, true);
    let record = NativePointerPassthroughRecord::for_test(
        token,
        viewport,
        window,
        None,
        true,
        NativeViewportPointerPassthroughStatus::Applied,
    );
    let mut resources = NativeWindowResources::with_capacity_limit(2).unwrap();
    let mut journal = HostIngressJournal::new(NativeMonotonicClock::system());
    let bytes = resources.owned_data_capacity_bytes();
    let backing = resources.entries.as_ptr();
    assert!(resources.reserve_pointer_callback(viewport, first, window, token, true));
    assert_eq!(
        resources.accept_pointer_callback(record, |binding| {
            assert_eq!(binding, first);
            journal.reserve(HostRecord::ViewportPointerPassthrough(
                record.qualified_for_binding(binding),
            ))
        }),
        CommandCallbackAdmission::First,
    );
    let ordinal = resources.callback_ingress().next().unwrap().ordinal();
    resources.retire(viewport, first);
    let rect = NativePhysicalRect::new(0, 0, 640, 480);
    assert!(resources.reserve_create(viewport, next, rect));
    assert!(!resources.reserve_create(peer, next, rect));
    assert_eq!(resources.entries.len(), 2);
    resources.commit_transferred_callbacks();
    assert!(resources.references_binding(first));
    assert_eq!(
        resources.callback_ingress().next().unwrap().ordinal(),
        ordinal
    );
    assert!(resources.take_callback_ingress(ordinal + 1).is_none());
    let ingress = resources.take_callback_ingress(ordinal).unwrap();
    assert!(matches!(
        ingress.record(),
        HostRecord::ViewportPointerPassthrough(record) if record.binding() == Some(first)
    ));
    journal.transfer_reserved(ingress);
    assert!(resources.references_binding(first));
    assert!(!resources.reserve_create(peer, next, rect));
    assert_eq!(journal.captured_through(), ordinal);
    assert_eq!(resources.owned_data_capacity_bytes(), bytes);

    journal.commit_transferred();
    resources.commit_transferred_callbacks();
    assert!(!resources.references_binding(first));
    assert_eq!(resources.create(viewport).unwrap().binding, next);
    assert_eq!(resources.entries.len(), 1);
    assert_eq!(resources.owned_data_capacity_bytes(), bytes);
    assert_eq!(resources.entries.as_ptr(), backing);
    assert!(resources.reserve_create(peer, next, rect));
    assert_eq!(resources.owned_data_capacity_bytes(), bytes);
    assert_eq!(journal.captured_through(), ordinal);
}

#[test]
fn settled_capture_retains_retired_cleanup_backing_until_its_exact_release() {
    let (first, next) = bindings();
    let viewport = ViewportId::ROOT;
    let peer = ViewportId::from_hash_of("capture-cleanup-capacity");
    let rect = NativePhysicalRect::new(0, 0, 640, 480);
    let mut resources = NativeWindowResources::with_capacity_limit(2).unwrap();
    let bytes = resources.owned_data_capacity_bytes();
    let backing = resources.entries.as_ptr();
    // Model the resource hold after a settled acquisition. The real press and
    // backend acquisition are exercised by the isolated X11 capture journey.
    resources
        .command_owner(viewport, first)
        .unwrap()
        .capture_duty_held = true;
    resources.retire(viewport, first);
    resources.refresh_host_references(|_, _| false);
    resources.commit_transferred_callbacks();
    assert!(resources.references_binding(first));
    assert!(resources.has_pending_work());
    assert!(resources.reserve_create(viewport, next, rect));
    assert!(!resources.reserve_create(peer, next, rect));

    resources.release_capture_binding(next);
    resources.commit_transferred_callbacks();
    assert!(resources.references_binding(first));
    assert_eq!(resources.entries.len(), 2);
    resources.release_capture_binding(first);
    assert!(!resources.references_binding(first));
    assert_eq!(resources.create(viewport).unwrap().binding, next);
    assert!(resources.reserve_create(peer, next, rect));
    assert_eq!(resources.owned_data_capacity_bytes(), bytes);
    assert_eq!(resources.entries.as_ptr(), backing);
}

#[test]
fn renderer_preparation_uses_the_existing_prepaid_exact_window_slot() {
    let (binding, successor) = bindings();
    let viewport = ViewportId::ROOT;
    let mut resources = NativeWindowResources::with_capacity_limit(1).unwrap();
    let bytes = resources.owned_data_capacity_bytes();
    let backing = resources.entries.as_ptr();
    let rect = NativePhysicalRect::new(0, 0, 640, 480);
    assert!(resources.reserve_create(viewport, binding, rect));
    resources.begin_preparation(viewport, binding, WindowId::from(17));
    assert!(resources.has_pending_work());
    assert!(!resources.reserve_create(viewport, successor, rect));
    assert_eq!(resources.entries.as_ptr(), backing);
    assert_eq!(resources.owned_data_capacity_bytes(), bytes);
    resources.retire(viewport, binding);
    resources.refresh_host_references(|_, _| false);
    assert!(resources.entries.is_empty());
    assert_eq!(resources.entries.as_ptr(), backing);
    assert_eq!(resources.owned_data_capacity_bytes(), bytes);
}

#[test]
fn zero_entry_limit_and_reservation_overflow_are_explicit() {
    let (binding, _) = bindings();
    let mut resources = NativeWindowResources::with_capacity_limit(0).unwrap();
    assert!(resources.can_configure_capacity_limit());
    assert_eq!(
        NativeWindowResources::reservation_bytes(0),
        Some(size_of::<NativeWindowResources>()),
    );
    assert!(!resources.reserve_create(
        ViewportId::ROOT,
        binding,
        NativePhysicalRect::new(0, 0, 640, 480),
    ));
    assert_eq!(resources.entries.capacity(), 0);
    assert_eq!(NativeWindowResources::reservation_bytes(usize::MAX), None);
    assert_eq!(
        NativeWindowResources::with_capacity_limit(usize::MAX).unwrap_err(),
        NativeHostProtocolError::NativeReportStorageUnavailable,
    );
}

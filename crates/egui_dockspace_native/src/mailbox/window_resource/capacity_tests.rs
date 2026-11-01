use dockspace::runtime::NativeMonotonicClock;
use eframe::{NativeViewportPointerPassthroughStatus, egui};

use super::super::journal::HostIngressJournal;
use super::tests::{bindings, reissued_staging_requests};
use super::*;

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
fn oversized_staging_batch_preserves_existing_basis_and_every_owner() {
    let [first, newer] = reissued_staging_requests();
    let viewport = ViewportId::ROOT;
    let first_peer = ViewportId::from_hash_of("bounded-first-staging-peer");
    let second_peer = ViewportId::from_hash_of("bounded-second-staging-peer");
    let mut resources = NativeWindowResources::with_capacity_limit(2).unwrap();
    resources
        .publish_staging(BTreeMap::from([(viewport, first)]))
        .unwrap();
    let bytes = resources.owned_data_capacity_bytes();
    let backing = resources.entries.as_ptr();
    let requests = BTreeMap::from([(viewport, newer), (first_peer, newer), (second_peer, newer)]);
    assert!(!resources.can_publish_staging(&requests));
    assert_eq!(
        resources.prepare_staging(requests.clone()).unwrap_err(),
        NativeHostProtocolError::NativeReportStorageUnavailable,
    );
    assert!(resources.publish_staging(requests).is_none());
    assert_eq!(resources.entries.len(), 1);
    assert_eq!(resources.staging(viewport), Some(first));
    assert_eq!(resources.staging(first_peer), None);
    assert_eq!(resources.staging(second_peer), None);
    assert_eq!(resources.callback_ingress().count(), 0);
    assert_eq!(resources.owned_data_capacity_bytes(), bytes);
    assert_eq!(resources.entries.as_ptr(), backing);
}

#[test]
fn staging_preparation_pays_backing_before_publication_and_empty_storage_keeps_its_charge() {
    let [request, _] = reissued_staging_requests();
    let viewport = ViewportId::ROOT;
    let peer = ViewportId::from_hash_of("prepared-staging-peer");
    for finite in [false, true] {
        let mut resources = if finite {
            NativeWindowResources::with_capacity_limit(2).unwrap()
        } else {
            NativeWindowResources::default()
        };
        let requests = BTreeMap::from([(viewport, request), (peer, request)]);
        let dropped = resources.prepare_staging(requests.clone()).unwrap();
        let bytes = resources.owned_data_capacity_bytes();
        let backing = resources.entries.as_ptr();
        drop(dropped);
        assert!(resources.entries.is_empty());
        assert!(resources.can_configure_capacity_limit());
        assert!(!resources.has_pending_work());
        assert_eq!(resources.owned_data_capacity_bytes(), bytes);

        let prepared = resources.prepare_staging(requests.clone()).unwrap();
        assert_eq!(
            prepared.owned_data_capacity_bytes(),
            size_of::<PreparedWindowStaging>()
                + prepared.requests.capacity()
                    * size_of::<(ViewportId, NativeStagingPaintRequest)>()
                + prepared.wake.capacity() * size_of::<ViewportId>(),
        );
        if finite {
            assert_eq!(
                NativeWindowResources::staging_reservation_bytes(2),
                Some(prepared.owned_data_capacity_bytes()),
            );
        }
        let wake_backing = prepared.wake.as_ptr();
        let wake_capacity = prepared.wake.capacity();
        assert_eq!(resources.staging(viewport), None);
        assert_eq!(resources.staging(peer), None);
        let wake = resources.commit_staging(prepared);
        assert_eq!(wake.as_ptr(), wake_backing);
        assert_eq!(wake.capacity(), wake_capacity);
        assert_eq!(wake.len(), 2);
        assert!(wake.contains(&viewport));
        assert!(wake.contains(&peer));
        assert_eq!(resources.entries.as_ptr(), backing);
        assert_eq!(resources.owned_data_capacity_bytes(), bytes);
        assert!(resources.can_publish_staging(&requests));
        resources.clear_staging();
        resources.commit_transferred_callbacks();
        assert!(resources.entries.is_empty());
        assert!(resources.can_configure_capacity_limit());
        assert_eq!(resources.entries.as_ptr(), backing);
        assert_eq!(resources.owned_data_capacity_bytes(), bytes);
        if finite {
            assert_eq!(NativeWindowResources::reservation_bytes(2), Some(bytes));
        }
    }
}

#[test]
fn repeated_staging_viewport_rejects_before_an_unbounded_iterator_can_grow_storage() {
    use std::cell::Cell;

    let [request, _] = reissued_staging_requests();
    let viewport = ViewportId::ROOT;
    for finite in [false, true] {
        let mut resources = if finite {
            NativeWindowResources::with_capacity_limit(2).unwrap()
        } else {
            NativeWindowResources::default()
        };
        let bytes = resources.owned_data_capacity_bytes();
        let read = Cell::new(0);
        let requests = std::iter::from_fn(|| {
            read.set(read.get() + 1);
            assert!(
                read.get() <= 2,
                "the first duplicate must terminate preparation"
            );
            Some((viewport, request))
        });
        assert_eq!(
            resources.prepare_staging(requests).unwrap_err(),
            NativeHostProtocolError::OutputRouteAttachmentFailed,
        );
        assert_eq!(read.get(), 2);
        assert!(resources.entries.is_empty());
        assert_eq!(resources.owned_data_capacity_bytes(), bytes);
    }
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
    let prepared = resources.prepare_staging(BTreeMap::new()).unwrap();
    assert!(resources.commit_staging(prepared).is_empty());
    assert_eq!(resources.entries.capacity(), 0);
    assert_eq!(NativeWindowResources::reservation_bytes(usize::MAX), None);
    assert_eq!(
        NativeWindowResources::staging_reservation_bytes(usize::MAX),
        None,
    );
    assert_eq!(
        NativeWindowResources::with_capacity_limit(usize::MAX).unwrap_err(),
        NativeHostProtocolError::NativeReportStorageUnavailable,
    );
}

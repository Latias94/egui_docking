use crate::coordinator::NoFrameTerminal;

use super::tests::{bindings, staging_observations};
use super::*;

fn observation() -> (
    NativeSurfaceBinding,
    NativePresentationEffectAcknowledgement,
) {
    let ([request, _], acknowledgement) = staging_observations();
    (request.binding(), acknowledgement)
}

fn no_frame(
    viewport: ViewportId,
    binding: NativeSurfaceBinding,
    window: u64,
) -> PendingNoFrameInvalidation {
    PendingNoFrameInvalidation {
        terminal: NoFrameTerminal::Observed,
        viewport,
        window: WindowId::from(window),
        create_attempt: None,
        binding,
    }
}

fn pin(resources: &mut NativeWindowResources, viewport: ViewportId, binding: NativeSurfaceBinding) {
    resources.prepare_host_reference(viewport, binding).unwrap();
    resources.commit_host_reference(viewport, binding);
}

#[test]
fn host_reference_preparation_refuses_capacity_before_changing_the_active_owner() {
    let (binding, successor) = bindings();
    let viewport = ViewportId::ROOT;
    let peer = ViewportId::from_hash_of("host-reference-peer");
    let rect = NativePhysicalRect::new(0, 0, 640, 480);
    let mut resources = NativeWindowResources::with_capacity_limit(1).unwrap();
    let bytes = resources.owned_data_capacity_bytes();
    let backing = resources.entries.as_ptr();
    assert_eq!(resources.resource_slot_limit(), Some(1));

    resources.prepare_host_reference(viewport, binding).unwrap();
    assert!(resources.entries.is_empty());
    assert!(!resources.has_pending_work());
    assert!(resources.reserve_create(viewport, binding, rect));
    resources.prepare_host_reference(viewport, binding).unwrap();
    for (target, binding) in [(peer, binding), (viewport, successor)] {
        assert_eq!(
            resources.prepare_host_reference(target, binding),
            Err(NativeHostProtocolError::NativeReportStorageUnavailable),
        );
    }
    let old = resources.resource(viewport, binding).unwrap();
    assert!(!old.retired);
    assert!(!old.retained_by_host);
    assert_eq!(old.create_rect, Some(rect));
    assert!(old.hidden_render);
    assert_eq!(resources.entries.len(), 1);
    assert_eq!(resources.entries.as_ptr(), backing);
    assert_eq!(resources.owned_data_capacity_bytes(), bytes);

    let mut zero = NativeWindowResources::with_capacity_limit(0).unwrap();
    assert_eq!(
        zero.prepare_host_reference(viewport, binding),
        Err(NativeHostProtocolError::NativeReportStorageUnavailable),
    );
    assert!(zero.entries.is_empty());
}

#[test]
fn prepared_successor_pin_does_not_mutate_its_predecessor_before_explicit_retirement() {
    let (binding, successor) = bindings();
    let viewport = ViewportId::ROOT;
    let rect = NativePhysicalRect::new(0, 0, 640, 480);
    let mut resources = NativeWindowResources::with_capacity_limit(2).unwrap();
    let bytes = resources.owned_data_capacity_bytes();
    let backing = resources.entries.as_ptr();
    assert!(resources.reserve_create(viewport, binding, rect));
    pin(&mut resources, viewport, binding);

    resources
        .prepare_host_reference(viewport, successor)
        .unwrap();
    assert_eq!(resources.entries.len(), 1);
    assert_eq!(resources.create(viewport).unwrap().binding, binding);
    assert!(!resources.resource(viewport, binding).unwrap().retired);
    resources.commit_host_reference(viewport, successor);
    assert_eq!(resources.entries.len(), 2);
    assert_eq!(resources.create(viewport).unwrap().binding, binding);
    assert!(!resources.reserve_create(viewport, successor, rect));

    resources.retire(viewport, binding);
    assert!(resources.reserve_create(viewport, successor, rect));
    assert_eq!(resources.create(viewport).unwrap().binding, successor);
    // The old announced output and the new route retain separate exact rows.
    resources.refresh_host_references(|target, _| target == viewport);
    assert!(resources.references_binding(binding));
    assert!(!resources.reserve_create(viewport, binding, rect));
    // Removing the final old output releases only its retired row.
    resources.refresh_host_references(|target, current| target == viewport && current == successor);
    assert!(!resources.references_binding(binding));
    assert_eq!(resources.entries.len(), 1);
    assert_eq!(resources.create(viewport).unwrap().binding, successor);
    assert_eq!(resources.entries.as_ptr(), backing);
    assert_eq!(resources.owned_data_capacity_bytes(), bytes);
}

#[test]
fn pin_only_retention_is_idle_and_refresh_does_not_retire_an_unrouted_create() {
    let (binding, _) = bindings();
    let viewport = ViewportId::ROOT;
    let rect = NativePhysicalRect::new(0, 0, 640, 480);
    for finite in [false, true] {
        let mut resources = if finite {
            NativeWindowResources::with_capacity_limit(1).unwrap()
        } else {
            NativeWindowResources::default()
        };
        assert_eq!(resources.resource_slot_limit(), finite.then_some(1));
        resources.prepare_host_reference(viewport, binding).unwrap();
        let bytes = resources.owned_data_capacity_bytes();
        let backing = resources.entries.as_ptr();
        assert!(resources.entries.is_empty());
        resources.commit_host_reference(viewport, binding);
        assert!(resources.references_binding(binding));
        assert!(!resources.has_pending_work());
        resources.commit_transferred_callbacks();
        resources.clear_staging();
        assert!(resources.references_binding(binding));
        resources.refresh_host_references(|_, _| false);
        assert!(resources.entries.is_empty());

        assert!(resources.reserve_create(viewport, binding, rect));
        resources.refresh_host_references(|_, _| false);
        assert!(!resources.resource(viewport, binding).unwrap().retired);
        assert_eq!(resources.create(viewport).unwrap().rect, rect);
        assert!(resources.clear_create(viewport, binding));
        assert!(resources.references_binding(binding));
        assert!(resources.clear_hidden(viewport, binding));
        assert!(resources.entries.is_empty());
        assert!(!resources.has_pending_work());
        assert_eq!(resources.entries.as_ptr(), backing);
        assert_eq!(resources.owned_data_capacity_bytes(), bytes);
    }
}

#[test]
fn presentation_ack_needs_an_existing_row_and_rejects_conflicts_across_viewport_aliases() {
    let (binding, acknowledgement) = observation();
    let (foreign, conflict) = observation();
    assert_ne!(acknowledgement, conflict);
    let viewport = ViewportId::ROOT;
    let peer = ViewportId::from_hash_of("ack-alias-peer");
    let mut resources = NativeWindowResources::with_capacity_limit(2).unwrap();
    let bytes = resources.owned_data_capacity_bytes();
    let backing = resources.entries.as_ptr();
    assert!(!resources.can_retain_presentation_ack(viewport, binding));
    assert_eq!(
        resources.retain_presentation_ack(viewport, binding, acknowledgement),
        Err(acknowledgement),
    );
    assert!(resources.entries.is_empty());

    pin(&mut resources, viewport, binding);
    pin(&mut resources, peer, binding);
    assert!(resources.can_retain_presentation_ack(viewport, binding));
    assert!(resources.can_retain_presentation_ack(peer, binding));
    resources
        .retain_presentation_ack(viewport, binding, acknowledgement)
        .unwrap();
    assert!(!resources.can_retain_presentation_ack(viewport, binding));
    assert!(!resources.can_retain_presentation_ack(peer, binding));
    assert_eq!(
        resources.retain_presentation_ack(peer, binding, conflict),
        Err(conflict),
    );
    assert_eq!(resources.presentation_ack(binding), Some(acknowledgement));
    assert_eq!(resources.presentation_ack(foreign), None);
    assert_eq!(resources.presentation_ack_count(), 1);
    assert!(resources.has_pending_work());
    assert!(resources.clear_presentation_ack(binding));
    assert!(!resources.clear_presentation_ack(binding));
    assert_eq!(resources.presentation_ack_count(), 0);
    assert!(!resources.has_pending_work());
    assert_eq!(resources.entries.len(), 2);
    assert_eq!(resources.entries.as_ptr(), backing);
    assert_eq!(resources.owned_data_capacity_bytes(), bytes);
}

#[test]
fn no_frame_keeps_the_first_exact_record_across_viewport_aliases_at_full_capacity() {
    let (binding, foreign) = bindings();
    let viewport = ViewportId::ROOT;
    let peer = ViewportId::from_hash_of("no-frame-alias-peer");
    let first = no_frame(viewport, binding, 11);
    let later = no_frame(peer, binding, 12);
    let missing = no_frame(viewport, foreign, 13);
    let mut resources = NativeWindowResources::with_capacity_limit(2).unwrap();
    let bytes = resources.owned_data_capacity_bytes();
    let backing = resources.entries.as_ptr();
    assert_eq!(
        resources.retain_no_frame(viewport, binding, first),
        Err(first)
    );
    assert!(resources.pending_no_frame().next().is_none());
    assert!(resources.entries.is_empty());
    pin(&mut resources, viewport, binding);
    pin(&mut resources, peer, binding);
    resources.retain_no_frame(viewport, binding, first).unwrap();
    resources.retain_no_frame(peer, binding, later).unwrap();
    assert_eq!(resources.pending_no_frame().count(), 1);
    assert_eq!(resources.pending_no_frame().next(), Some(&first));
    assert!(resources.has_pending_work());
    assert_eq!(
        resources.retain_no_frame(viewport, foreign, missing),
        Err(missing),
    );
    assert_eq!(resources.retain_no_frame(peer, binding, first), Err(first));
    assert!(!resources.clear_no_frame_if(later));
    assert!(!resources.clear_no_frame_if(missing));
    assert_eq!(resources.pending_no_frame().next(), Some(&first));
    assert!(resources.clear_no_frame_if(first));
    assert!(!resources.clear_no_frame_if(first));
    assert!(resources.pending_no_frame().next().is_none());
    assert!(!resources.has_pending_work());
    assert_eq!(resources.entries.len(), 2);
    assert_eq!(resources.entries.as_ptr(), backing);
    assert_eq!(resources.owned_data_capacity_bytes(), bytes);
}

#[test]
fn retirement_preserves_both_observations_until_exact_settlement_and_then_prunes() {
    let (binding, acknowledgement) = observation();
    let successor = bindings().0;
    let viewport = ViewportId::ROOT;
    let peer = ViewportId::from_hash_of("retired-observation-overflow");
    let record = no_frame(viewport, binding, 21);
    let rect = NativePhysicalRect::new(0, 0, 640, 480);
    let mut resources = NativeWindowResources::with_capacity_limit(2).unwrap();
    let bytes = resources.owned_data_capacity_bytes();
    let backing = resources.entries.as_ptr();
    assert!(resources.reserve_create(viewport, binding, rect));
    resources
        .retain_presentation_ack(viewport, binding, acknowledgement)
        .unwrap();
    resources
        .retain_no_frame(viewport, binding, record)
        .unwrap();
    resources.retire(viewport, binding);
    assert!(resources.reserve_create(viewport, successor, rect));
    assert!(!resources.reserve_create(peer, successor, rect));
    resources.clear_staging();
    resources.commit_transferred_callbacks();
    assert!(resources.references_binding(binding));
    assert_eq!(resources.presentation_ack(binding), Some(acknowledgement));
    assert_eq!(resources.pending_no_frame().next(), Some(&record));

    assert!(resources.clear_presentation_ack(binding));
    assert!(resources.references_binding(binding));
    assert!(!resources.clear_no_frame_if(no_frame(viewport, binding, 22)));
    assert!(resources.clear_no_frame_if(record));
    assert!(!resources.references_binding(binding));
    assert_eq!(resources.create(viewport).unwrap().binding, successor);
    assert!(resources.clear_create(viewport, successor));
    assert!(resources.clear_hidden(viewport, successor));
    assert!(resources.entries.is_empty());
    assert!(!resources.has_pending_work());
    assert_eq!(resources.entries.as_ptr(), backing);
    assert_eq!(resources.owned_data_capacity_bytes(), bytes);
}

#[test]
fn borrowed_no_frame_filtering_prunes_only_empty_rows_without_releasing_prepaid_backing() {
    let (binding, acknowledgement) = observation();
    let other = bindings().0;
    let viewport = ViewportId::ROOT;
    let peer = ViewportId::from_hash_of("borrowed-no-frame-peer");
    let first = no_frame(viewport, binding, 31);
    let second = no_frame(peer, other, 32);
    let mut resources = NativeWindowResources::with_capacity_limit(2).unwrap();
    let bytes = resources.owned_data_capacity_bytes();
    let backing = resources.entries.as_ptr();
    pin(&mut resources, viewport, binding);
    pin(&mut resources, peer, other);
    resources.retain_no_frame(viewport, binding, first).unwrap();
    resources.retain_no_frame(peer, other, second).unwrap();
    resources
        .retain_presentation_ack(viewport, binding, acknowledgement)
        .unwrap();
    resources.retire(viewport, binding);
    resources.retire(peer, other);
    resources.refresh_host_references(|_, _| false);
    let mut visited = 0;
    resources.retain_no_frame_records(|record| {
        record.terminal = NoFrameTerminal::Observed;
        visited += 1;
        true
    });
    assert_eq!(visited, 2);
    assert_eq!(resources.pending_no_frame().count(), 2);

    resources.retain_no_frame_records(|record| {
        assert!(record.binding == binding || record.binding == other);
        false
    });
    assert!(resources.pending_no_frame().next().is_none());
    assert!(!resources.references_binding(other));
    assert!(resources.references_binding(binding));
    assert!(resources.has_pending_work());
    assert!(resources.clear_presentation_ack(binding));
    assert!(resources.entries.is_empty());
    assert!(!resources.has_pending_work());
    assert_eq!(resources.entries.as_ptr(), backing);
    assert_eq!(resources.owned_data_capacity_bytes(), bytes);
    assert_eq!(NativeWindowResources::reservation_bytes(2), Some(bytes));
}

#[test]
fn foreign_provider_bindings_with_reused_public_ids_keep_independent_observations() {
    let (predecessor, first_ack) = observation();
    let (successor, next_ack) = observation();
    assert_eq!(predecessor.surface(), successor.surface());
    assert_eq!(predecessor.window_token(), successor.window_token());
    assert_ne!(predecessor, successor);
    let viewport = ViewportId::ROOT;
    let first = no_frame(viewport, predecessor, 41);
    let next = no_frame(viewport, successor, 41);
    let mut resources = NativeWindowResources::with_capacity_limit(2).unwrap();
    pin(&mut resources, viewport, predecessor);
    pin(&mut resources, viewport, successor);
    resources
        .retain_presentation_ack(viewport, predecessor, first_ack)
        .unwrap();
    resources
        .retain_presentation_ack(viewport, successor, next_ack)
        .unwrap();
    resources
        .retain_no_frame(viewport, predecessor, first)
        .unwrap();
    resources
        .retain_no_frame(viewport, successor, next)
        .unwrap();
    assert_eq!(resources.presentation_ack_count(), 2);
    assert_eq!(resources.pending_no_frame().count(), 2);

    resources.retire(viewport, predecessor);
    resources.refresh_host_references(|_, binding| binding == successor);
    assert!(resources.clear_presentation_ack(predecessor));
    assert!(resources.clear_no_frame_if(first));
    assert!(!resources.references_binding(predecessor));
    assert_eq!(resources.presentation_ack(successor), Some(next_ack));
    assert_eq!(resources.pending_no_frame().next(), Some(&next));
    assert_eq!(resources.entries.len(), 1);
}

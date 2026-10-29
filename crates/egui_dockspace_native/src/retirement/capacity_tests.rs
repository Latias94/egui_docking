use super::*;
use crate::mailbox::{HostRecord, journal::HostIngressJournal};
use crate::viewport_callback::NativeViewportCreateFailureRecord;
use crate::viewport_map::{CreateAdmission, CreateTerminal, NativeViewportMap};
use dockspace::runtime::{
    DockspaceSession, HostFrameReport, HostInputOutcome, NativeCloseState,
    NativeEffectAcknowledgement, NativeWorkAreaRoster, SurfaceUnavailableReason,
};
use eframe::NativeViewportCreateFailureKind;

fn commit(session: &mut DockspaceSession) -> HostFrameReport {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn close_acknowledgement() -> (
    DockspaceSession,
    NativeSurfaceBinding,
    NativeCloseEffectAcknowledgement,
) {
    let (mut session, binding) = super::tests::registered_session();
    session
        .configure_managed_native_capabilities(crate::capabilities::capabilities_for_backend(
            eframe::NativeWindowingBackend::Windows,
        ))
        .unwrap();
    session
        .report_managed_native_snapshot(
            [(binding, NativeWindowFacts::live())],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    commit(&mut session);
    session
        .publish_native_close(binding, NativeCloseState::Requested, None)
        .unwrap();
    let request = commit(&mut session)
        .inputs()
        .iter()
        .find_map(|input| match input {
            HostInputOutcome::NativeCloseObservationApplied { close_requests } => {
                close_requests.first().copied()
            }
            _ => None,
        })
        .unwrap();
    session.cancel_native_surface_close(request).unwrap();
    let mut effects = commit(&mut session).take_native_effects().into_iter();
    let effect = effects.next().unwrap();
    assert!(effects.next().is_none());
    let Some(NativeEffectAcknowledgement::Close(acknowledgement)) = effect.accepted() else {
        panic!("the real core cancellation supplies a close acknowledgement")
    };
    (session, binding, acknowledgement)
}

fn window_receipt(
    viewport: ViewportId,
    window: WindowId,
    binding: NativeSurfaceBinding,
) -> NativeWindowReceipt {
    let mut routes = NativeViewportMap::default();
    routes.bind(viewport, window, binding).unwrap();
    routes.take_window_receipt(viewport, binding).unwrap()
}

fn failed_create(
    viewport: ViewportId,
    binding: NativeSurfaceBinding,
) -> (NativeCreateReceipt, u64) {
    let mut journal = HostIngressJournal::new(dockspace::runtime::NativeMonotonicClock::system());
    failed_create_in(&mut journal, viewport, binding)
}

fn failed_create_in(
    journal: &mut HostIngressJournal,
    viewport: ViewportId,
    binding: NativeSurfaceBinding,
) -> (NativeCreateReceipt, u64) {
    let mut receipt = NativeCreateReceipt::new(None);
    let failure = NativeViewportCreateFailureKind::WindowUnavailable;
    assert_eq!(
        receipt.receive(None, CreateTerminal::Failed(failure), || {
            journal.reserve(HostRecord::ViewportCreateFailed(
                NativeViewportCreateFailureRecord::new(viewport, binding, failure),
            ))
        }),
        CreateAdmission::First,
    );
    let ordinal = receipt.ingress().unwrap().ordinal();
    assert!(receipt.transfer(None, CreateTerminal::Failed(failure)));
    (receipt, ordinal)
}

fn commit_failed_create(
    state: &mut NativeRetirementState,
    binding: NativeSurfaceBinding,
    ordinal: u64,
) {
    state.mark_create_failure_recorded(binding);
    state.commit_create_failures();
    assert!(!state.is_quiescence_candidate(binding));
    let ingress = state.take_create_ingress(ordinal).unwrap();
    assert_eq!(ingress.ordinal(), ordinal);
    assert!(state.take_create_ingress(ordinal).is_none());
    assert!(!state.is_quiescence_candidate(binding));
    state.commit_create_failures();
    assert!(state.is_quiescence_candidate(binding));
}

#[test]
fn one_shared_slot_keeps_the_failed_create_until_quiescence_then_retries_the_original_window() {
    let failed = super::tests::binding();
    let live = super::tests::binding();
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(31);
    let mut state = NativeRetirementState::with_capacity_limit(1).unwrap();
    let baseline = state.owned_data_capacity_bytes();
    let backing = state.entries.as_ptr();
    let (receipt, ordinal) = failed_create(viewport, failed);
    state
        .begin_unmaterialized_quiescence(failed, receipt)
        .unwrap();
    let receipt = state
        .retain_window(window_receipt(viewport, window, live))
        .unwrap_err();
    assert_eq!(receipt.binding(), live);
    assert_eq!(receipt.window(), window);
    assert!(!state.can_retain_window(live, window));
    assert_eq!(
        state.prepare_capacity_for_retained_lifetimes(2),
        Err(NativeHostProtocolError::NativeReportStorageUnavailable),
    );
    commit_failed_create(&mut state, failed, ordinal);
    assert_eq!(state.next_quiescence_candidate(None), Some(failed));
    let receipt = state.retain_window(receipt).unwrap_err();
    assert!(state.has_quiescence_owner(failed));
    assert!(state.finish_quiescence(failed));
    assert!(!state.finish_quiescence(failed));
    assert!(!state.has_pending_work());
    assert!(!state.can_configure_capacity_limit());
    state.retain_window(receipt).unwrap();
    assert!(state.has_window(viewport, live));
    assert_eq!(state.bindings().collect::<Vec<_>>(), [live]);
    assert_eq!(state.entries.as_ptr(), backing);
    assert_eq!(state.owned_data_capacity_bytes(), baseline);
}

#[test]
fn release_preserves_its_acknowledgement_through_full_commit_and_rejected_snapshot_retry() {
    let (_session, binding, acknowledgement) = close_acknowledgement();
    let other = super::tests::binding();
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(41);
    let mut state = NativeRetirementState::with_capacity_limit(1).unwrap();
    let baseline = state.owned_data_capacity_bytes();
    let backing = state.entries.as_ptr();
    state
        .begin_release(
            viewport,
            binding,
            acknowledgement,
            Some(window_receipt(viewport, window, binding)),
        )
        .unwrap();
    let (receipt, ordinal) = failed_create(viewport, other);
    let receipt = state
        .begin_unmaterialized_quiescence(other, receipt)
        .unwrap_err();
    assert_eq!(receipt.ingress().unwrap().ordinal(), ordinal);
    assert_eq!(
        state.receive_destroyed(window, Some(viewport), 5, |viewport, binding| {
            super::tests::destroyed_ingress(viewport, binding, window, 5)
        }),
        Some((viewport, binding)),
    );
    let capture = state.window_ingress().next().unwrap().ordinal();
    assert_eq!(
        state.observe_destroyed(viewport, binding, window, 5),
        DestroyedObservation::Recorded,
    );
    assert!(state.transfer_destroyed(binding, window, 5));
    state.commit_destroyed_callbacks();
    assert!(!state.requires_snapshot());
    let ingress = state.take_window_ingress(capture).unwrap();
    assert_eq!(ingress.window_event().unwrap().ordinal(), 5);
    assert!(!state.requires_snapshot());
    state.commit_destroyed_callbacks();
    assert_eq!(
        state.destroyed_observations().next(),
        Some((binding, NativeWindowFacts::destroyed_after(acknowledgement))),
    );
    for _ in 0..2 {
        state.mark_snapshot_queued();
        assert!(state.committed_routes().next().is_none());
        state.settle_snapshot(false);
        assert!(state.requires_snapshot());
        assert!(!state.finish_quiescence(binding));
        assert_eq!(
            state.destroyed_observations().next(),
            Some((binding, NativeWindowFacts::destroyed_after(acknowledgement))),
        );
    }
    state.mark_snapshot_queued();
    state.settle_snapshot(true);
    let committed = state.committed_routes().next().unwrap();
    assert_eq!(committed.viewport(), viewport);
    assert_eq!(committed.binding(), binding);
    assert!(!state.is_quiescence_candidate(binding));
    assert!(state.can_retire_committed_route_for_replacement(viewport, binding));
    state.commit_routes(&[committed]);
    assert_eq!(state.next_quiescence_candidate(None), Some(binding));
    let receipt = state
        .begin_unmaterialized_quiescence(other, receipt)
        .unwrap_err();
    assert!(state.finish_quiescence(binding));
    state
        .begin_unmaterialized_quiescence(other, receipt)
        .unwrap();
    assert_eq!(state.create_ingress().next().unwrap().ordinal(), ordinal);
    assert_eq!(state.entries.as_ptr(), backing);
    assert_eq!(state.owned_data_capacity_bytes(), baseline);
}

#[test]
fn capacity_and_identity_refusals_return_the_original_release_capabilities() {
    let (_session, binding, acknowledgement) = close_acknowledgement();
    let failed = super::tests::binding();
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(51);
    let mut state = NativeRetirementState::with_capacity_limit(1).unwrap();
    let baseline = state.owned_data_capacity_bytes();
    let (failed_receipt, ordinal) = failed_create(viewport, failed);
    state
        .begin_unmaterialized_quiescence(failed, failed_receipt)
        .unwrap();
    assert!(!state.can_begin_release(viewport, binding));
    let (retry_ack, retry_window) = state
        .begin_release(
            viewport,
            binding,
            acknowledgement,
            Some(window_receipt(viewport, window, binding)),
        )
        .unwrap_err();
    assert_eq!(retry_ack, acknowledgement);
    let retry_window = retry_window.unwrap();
    assert_eq!(retry_window.viewport(), viewport);
    assert_eq!(retry_window.window(), window);
    assert_eq!(retry_window.binding(), binding);
    commit_failed_create(&mut state, failed, ordinal);
    assert!(state.finish_quiescence(failed));
    let foreign_viewport = ViewportId::from_hash_of("foreign-release");
    let (retry_ack, retry_window) = state
        .begin_release(foreign_viewport, binding, retry_ack, Some(retry_window))
        .unwrap_err();
    assert_eq!(retry_ack, acknowledgement);
    assert!(!state.has_pending_work());
    state
        .begin_release(viewport, binding, retry_ack, retry_window)
        .unwrap();
    let (retry_ack, retry_window) = state
        .begin_release(viewport, binding, acknowledgement, None)
        .unwrap_err();
    assert_eq!(retry_ack, acknowledgement);
    assert!(retry_window.is_none());
    assert_eq!(state.bindings().count(), 1);
    assert_eq!(state.owned_data_capacity_bytes(), baseline);
}

#[test]
fn lifetime_exclusivity_rejects_viewport_aliases_and_both_terminal_kinds() {
    let (_session, binding, acknowledgement) = close_acknowledgement();
    let foreign = super::tests::binding();
    assert_eq!(binding.surface(), foreign.surface());
    assert_eq!(binding.window_token(), foreign.window_token());
    assert!(!binding.same_window_lifetime(foreign));
    let viewport = ViewportId::ROOT;
    let alias = ViewportId::from_hash_of("alias-window");
    let window = WindowId::from(61);
    let mut state = NativeRetirementState::with_capacity_limit(2).unwrap();
    let baseline = state.owned_data_capacity_bytes();
    let backing = state.entries.as_ptr();
    state
        .retain_window(window_receipt(viewport, window, binding))
        .unwrap();
    assert!(state.can_begin_release(viewport, binding));
    assert!(!state.can_begin_release(alias, binding));
    let (returned, receipt) = state
        .begin_release(alias, binding, acknowledgement, None)
        .unwrap_err();
    assert_eq!(returned, acknowledgement);
    assert!(receipt.is_none());
    let receipt = state
        .retain_window(window_receipt(alias, WindowId::from(62), binding))
        .unwrap_err();
    assert_eq!(receipt.binding(), binding);
    assert_eq!(receipt.viewport(), alias);
    let failed = NativeCreateReceipt::new(None);
    assert!(
        state
            .begin_unmaterialized_quiescence(binding, failed)
            .is_err()
    );
    assert!(!state.adopt_successor_binding(binding, foreign));
    assert!(state.has_window(viewport, binding));
    state
        .begin_unmaterialized_quiescence(foreign, NativeCreateReceipt::new(None))
        .unwrap();
    assert!(!state.can_begin_release(alias, foreign));
    assert!(!state.can_retain_window(foreign, WindowId::from(63)));
    state
        .begin_release(viewport, binding, acknowledgement, None)
        .unwrap();
    assert_eq!(state.bindings().count(), 2);
    assert_eq!(state.entries.as_ptr(), backing);
    assert_eq!(state.owned_data_capacity_bytes(), baseline);
}

#[test]
fn quiescence_cursor_orders_mixed_owners_and_keeps_a_blocked_candidate() {
    let mut bindings = [
        super::tests::binding(),
        super::tests::binding(),
        super::tests::binding(),
    ];
    bindings.sort_unstable();
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(71);
    let mut state = NativeRetirementState::with_capacity_limit(3).unwrap();
    let baseline = state.owned_data_capacity_bytes();
    let backing = state.entries.as_ptr();
    let mut journal = HostIngressJournal::new(dockspace::runtime::NativeMonotonicClock::system());
    let (last, last_ordinal) = failed_create_in(&mut journal, viewport, bindings[2]);
    state
        .begin_unmaterialized_quiescence(bindings[2], last)
        .unwrap();
    state
        .retain_window(window_receipt(viewport, window, bindings[1]))
        .unwrap();
    let (first, first_ordinal) = failed_create_in(&mut journal, viewport, bindings[0]);
    assert_ne!(first_ordinal, last_ordinal);
    state
        .begin_unmaterialized_quiescence(bindings[0], first)
        .unwrap();
    commit_failed_create(&mut state, bindings[0], first_ordinal);
    commit_failed_create(&mut state, bindings[2], last_ordinal);
    assert_eq!(state.next_quiescence_candidate(None), Some(bindings[0]));
    assert_eq!(
        state.next_quiescence_candidate(Some(bindings[0])),
        Some(bindings[2])
    );
    assert!(state.finish_quiescence(bindings[2]));
    assert_eq!(state.next_quiescence_candidate(Some(bindings[2])), None);
    assert_eq!(state.next_quiescence_candidate(None), Some(bindings[0]));
    assert!(!state.finish_quiescence(bindings[1]));
    assert_eq!(state.bindings().collect::<Vec<_>>(), bindings[..2]);
    assert_eq!(state.entries.as_ptr(), backing);
    assert_eq!(state.owned_data_capacity_bytes(), baseline);
}

#[test]
fn reservation_and_unlimited_preparation_preserve_existing_backing_on_overflow() {
    for slots in [0, 1, 7] {
        let mut state = NativeRetirementState::with_capacity_limit(slots).unwrap();
        assert_eq!(state.capacity_limit(), Some(slots));
        assert!(!state.can_configure_capacity_limit());
        assert_eq!(
            Some(state.owned_data_capacity_bytes()),
            NativeRetirementState::reservation_bytes(slots),
        );
        let backing = state.entries.as_ptr();
        assert!(state.prepare_capacity_for_retained_lifetimes(slots).is_ok());
        assert_eq!(
            state.prepare_capacity_for_retained_lifetimes(slots + 1),
            Err(NativeHostProtocolError::NativeReportStorageUnavailable),
        );
        assert_eq!(state.entries.as_ptr(), backing);
    }
    let mut state = NativeRetirementState::default();
    assert!(state.can_configure_capacity_limit());
    assert_eq!(state.capacity_limit(), None);
    state.prepare_capacity_for_retained_lifetimes(3).unwrap();
    assert!(!state.has_pending_work());
    assert!(state.can_configure_capacity_limit());
    assert_eq!(state.entries.capacity(), 3);
    let baseline = state.owned_data_capacity_bytes();
    let backing = state.entries.as_ptr();
    state.prepare_capacity_for_retained_lifetimes(1).unwrap();
    for required in [usize::MAX, usize::MAX / size_of::<RetirementEntry>() + 1] {
        assert_eq!(NativeRetirementState::reservation_bytes(required), None);
        assert_eq!(
            state.prepare_capacity_for_retained_lifetimes(required),
            Err(NativeHostProtocolError::NativeReportStorageUnavailable),
        );
        assert_eq!(
            NativeRetirementState::with_capacity_limit(required).unwrap_err(),
            NativeHostProtocolError::NativeReportStorageUnavailable,
        );
    }
    assert_eq!(state.entries.as_ptr(), backing);
    assert_eq!(state.owned_data_capacity_bytes(), baseline);
    state.prepare_capacity_for_retained_lifetimes(4).unwrap();
    assert_eq!(state.entries.capacity(), 4);
    assert_eq!(
        Some(state.owned_data_capacity_bytes()),
        NativeRetirementState::reservation_bytes(4),
    );
}

#[test]
fn default_unlimited_accepts_more_lifetimes_than_the_selected_finite_bound() {
    let bindings = [super::tests::binding(), super::tests::binding()];
    let mut finite = NativeRetirementState::with_capacity_limit(1).unwrap();
    let mut unlimited = NativeRetirementState::default();
    unlimited
        .prepare_capacity_for_retained_lifetimes(2)
        .unwrap();
    let baseline = unlimited.owned_data_capacity_bytes();
    let backing = unlimited.entries.as_ptr();
    for binding in bindings {
        unlimited
            .begin_unmaterialized_quiescence(binding, NativeCreateReceipt::new(None))
            .unwrap();
    }
    finite
        .begin_unmaterialized_quiescence(bindings[0], NativeCreateReceipt::new(None))
        .unwrap();
    assert!(
        finite
            .begin_unmaterialized_quiescence(bindings[1], NativeCreateReceipt::new(None))
            .is_err()
    );
    assert_eq!(unlimited.bindings().count(), 2);
    assert_eq!(unlimited.entries.as_ptr(), backing);
    assert_eq!(unlimited.owned_data_capacity_bytes(), baseline);
    assert!(!unlimited.can_configure_capacity_limit());
    assert_eq!(unlimited.capacity_limit(), None);
}

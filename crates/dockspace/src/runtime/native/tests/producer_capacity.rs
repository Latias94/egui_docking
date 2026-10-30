use super::*;
use crate::model::{DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout};
use crate::runtime::{DockspaceHostFrame, NativeInputStorageLimit, NativeProducerStorageLimit};
use std::cell::Cell;
use std::num::NonZeroUsize;

const WORK_BYTES: usize = 1 << 20;
const OTHER_SURFACE: SurfaceId = SurfaceId::new(2);

fn layout(count: u64) -> DockspaceLayout {
    DockspaceLayout::new((1..=count).map(|identity| {
        DockspaceSurfaceLayout::new(
            SurfaceId::new(identity),
            DockspaceRootLayout::new(
                RootId::new(identity),
                DockspaceNode::central_tabs([ItemId::new(identity)]),
            ),
        )
    }))
    .unwrap()
}

fn producer_limit(bindings: usize, areas: usize, bytes: usize) -> NativeProducerStorageLimit {
    NativeProducerStorageLimit::new(
        NonZeroUsize::new(bindings).unwrap(),
        NonZeroUsize::new(areas).unwrap(),
        bytes,
    )
}

fn input_limit(producer: NativeProducerStorageLimit) -> NativeInputStorageLimit {
    NativeInputStorageLimit::new(NonZeroUsize::new(16).unwrap(), WORK_BYTES)
        .with_producer_storage_limit(producer)
}

fn enroll(session: &mut DockspaceSession, producer: NativeProducerStorageLimit) {
    session
        .enable_managed_native_host_with_input_storage_limit(
            crate::runtime::NativeCreateStrategy::Staged,
            NativePointerRoster::Exact(Vec::new()),
            input_limit(producer),
        )
        .unwrap();
    session
        .configure_managed_native_capabilities(full_managed_capabilities())
        .unwrap();
}

fn complete(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn commit(session: &mut DockspaceSession) -> HostFrameReport {
    complete(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    )
}

// Existing unbounded lifecycle fixtures can select a finite store for a
// prospective-operation test without changing their published core authority.
fn bound_existing(session: &mut DockspaceSession, bindings: usize, areas: usize) {
    let limit = producer_limit(bindings, areas, WORK_BYTES);
    let native = session.native.as_mut().unwrap();
    let producer =
        NativeProducerState::prepare(&session.engine, Some(&native.producer), Some(limit)).unwrap();
    native.producer = producer;
    native.producer_storage_limit = Some(limit);
}

#[test]
fn producer_enrollment_prepays_the_complete_peak_and_one_byte_short_is_inert() {
    let probe = producer_limit(3, 2, usize::MAX);
    let required = probe.required_bytes().unwrap();
    let mut session = DockspaceSession::from_layout(layout(1), DockPolicy::default()).unwrap();
    assert_eq!(session.native_input_producer_storage_capacity_bytes(), 0);
    assert_eq!(session.native_input_producer_storage_reserved_bytes(), None);
    let version = session.version();
    let frontier = session.engine.presentation_identity_frontier();
    let watermark = session.engine.backend_ingress_committed_through();
    for bytes in [0, required - 1] {
        let error = session
            .enable_managed_native_host_with_input_storage_limit(
                crate::runtime::NativeCreateStrategy::Staged,
                NativePointerRoster::Exact(Vec::new()),
                input_limit(producer_limit(3, 2, bytes)),
            )
            .unwrap_err();
        assert_eq!(
            error.native_kind(),
            Some(NativeHostErrorKind::InputCapacity)
        );
        assert!(session.native.is_none());
        assert!(session.engine.backend_ingress_provider().is_none());
        assert!(session.engine.platform_provider().is_none());
        assert!(session.engine.pointer_provider().is_none());
        assert_eq!(session.version(), version);
        assert_eq!(session.engine.presentation_identity_frontier(), frontier);
        assert_eq!(
            session.engine.backend_ingress_committed_through(),
            watermark
        );
        assert_eq!(session.native_input_recorded_count(), 0);
        assert_eq!(session.native_input_producer_storage_capacity_bytes(), 0);
        assert_eq!(session.native_input_producer_storage_reserved_bytes(), None);
    }
    let selected = producer_limit(3, 2, required);
    assert_eq!(
        input_limit(selected).producer_storage_limit(),
        Some(selected)
    );
    assert_eq!(selected.binding_slots().get(), 3);
    assert_eq!(selected.work_area_slots().get(), 2);
    enroll(&mut session, selected);
    assert_eq!(
        session.native_input_producer_storage_reserved_bytes(),
        Some(required)
    );
    assert_eq!(
        session.native_input_producer_storage_capacity_bytes(),
        super::super::storage::producer_inline_capacity_bytes()
            + 3 * size_of::<NativeBindingState>()
            + 2 * size_of::<WorkAreaToken>()
    );
    commit(&mut session);
    let native = session.native.as_ref().unwrap();
    let prepared = native.prepare_native_commit(&session.engine).unwrap();
    let overlapping_backing = session.native_input_producer_storage_capacity_bytes()
        + prepared.next_producer.owned_capacity_bytes()
        + (prepared.report.bindings.capacity() + prepared.report.admissions.capacity())
            * size_of::<NativeSurfaceBinding>();
    assert!(overlapping_backing <= required);
    assert_eq!(
        session.native_input_producer_storage_reserved_bytes(),
        Some(required)
    );
    assert_eq!(
        producer_limit(usize::MAX, 1, usize::MAX).validate(),
        Err(NativePlatformError::InputCapacity)
    );
}

#[test]
fn initial_registry_over_bound_rejects_before_publishing_any_provider() {
    let mut session = DockspaceSession::from_layout(layout(2), DockPolicy::default()).unwrap();
    session
        .enable_managed_native_host(
            crate::runtime::NativeCreateStrategy::Staged,
            NativePointerRoster::Exact(Vec::new()),
        )
        .unwrap();
    session.register_native_root(SURFACE, WINDOW).unwrap();
    session
        .register_native_root(OTHER_SURFACE, HostWindowToken::new(42))
        .unwrap();
    commit(&mut session);
    // End the fixture provider through its real drain/replacement protocol;
    // external registrations remain in the complete registry for enrollment.
    let native = session.native.take().unwrap();
    let mut drained = native.recorder.drain();
    let replacement = session
        .engine
        .begin_backend_ingress_provider_replacement(&mut drained)
        .unwrap();
    drop(replacement);
    session
        .engine
        .abort_backend_ingress_provider_replacement()
        .unwrap();
    assert_eq!(session.engine.viewport().registry().records().count(), 2);
    assert!(session.engine.backend_ingress_provider().is_none());
    let frontier = session.engine.presentation_identity_frontier();
    let watermark = session.engine.backend_ingress_committed_through();
    let version = session.version();
    let error = session
        .enable_managed_native_host_with_input_storage_limit(
            crate::runtime::NativeCreateStrategy::Staged,
            NativePointerRoster::Exact(Vec::new()),
            input_limit(producer_limit(1, 1, WORK_BYTES)),
        )
        .unwrap_err();
    assert_eq!(
        error.native_kind(),
        Some(NativeHostErrorKind::InputCapacity)
    );
    assert!(session.native.is_none());
    assert!(session.engine.backend_ingress_provider().is_none());
    assert!(session.engine.platform_provider().is_none());
    assert!(session.engine.pointer_provider().is_none());
    assert_eq!(session.engine.presentation_identity_frontier(), frontier);
    assert_eq!(
        session.engine.backend_ingress_committed_through(),
        watermark
    );
    assert_eq!(session.version(), version);
}

#[test]
fn registration_over_bound_rejects_before_append_and_the_original_prefix_commits() {
    let mut session = DockspaceSession::from_layout(layout(2), DockPolicy::default()).unwrap();
    enroll(&mut session, producer_limit(1, 1, WORK_BYTES));
    session.register_native_root(SURFACE, WINDOW).unwrap();
    let watermark = session.engine.backend_ingress_committed_through();
    let recorded = session.native_input_recorded_count();
    let source = session.committed_source_sequence;
    let version = session.version();
    let frontier = session.engine.presentation_identity_frontier();
    let backing = session.native_input_producer_storage_capacity_bytes();
    for _ in 0..2 {
        assert_eq!(
            session
                .register_native_root(OTHER_SURFACE, HostWindowToken::new(42))
                .unwrap_err()
                .native_kind(),
            Some(NativeHostErrorKind::InputCapacity)
        );
        assert_eq!(
            session.engine.backend_ingress_committed_through(),
            watermark
        );
        assert_eq!(session.native_input_recorded_count(), recorded);
        assert_eq!(session.committed_source_sequence, source);
        assert_eq!(session.version(), version);
        assert_eq!(session.engine.presentation_identity_frontier(), frontier);
        assert_eq!(
            session.native_input_producer_storage_capacity_bytes(),
            backing
        );
    }
    let report = commit(&mut session);
    assert_eq!(report.native_bindings().len(), 1);
    assert!(matches!(
        report.inputs(),
        [crate::runtime::HostInputOutcome::NativeSurfaceRegistered { .. }]
    ));
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert_eq!(
        session.engine.backend_ingress_committed_through().get(),
        recorded
    );
    assert_eq!(
        session
            .register_native_root(OTHER_SURFACE, HostWindowToken::new(42))
            .unwrap_err()
            .native_kind(),
        Some(NativeHostErrorKind::InputCapacity)
    );
    assert_eq!(
        session
            .register_owned_native_child(OTHER_SURFACE, HostWindowToken::new(43), SURFACE)
            .unwrap_err()
            .native_kind(),
        Some(NativeHostErrorKind::InputCapacity)
    );
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert!(commit(&mut session).inputs().is_empty());
}

struct MisleadingHint<I>(I);

impl<I: Iterator> Iterator for MisleadingHint<I> {
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (usize::MAX, Some(usize::MAX))
    }
}

#[test]
fn finite_snapshot_ignores_hints_and_rejects_a_foreign_suffix_without_advancing_generations() {
    let (mut session, binding) =
        native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    let (_, foreign) = native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    bound_existing(&mut session, 1, 1);
    session
        .report_managed_native_snapshot(
            MisleadingHint(std::iter::once((binding, NativeWindowFacts::live()))),
            NativeWorkAreaRoster::Exact(vec![work_area(1)]),
        )
        .unwrap();
    commit(&mut session);
    session.prepare_native_pointer_capacity().unwrap();
    let before = format!("{:?}", session.native);
    let recorded = session.native_input_recorded_count();
    let reads = Cell::new(0);
    let observations = MisleadingHint(std::iter::from_fn(|| {
        let read = reads.get() + 1;
        reads.set(read);
        match read {
            1 => Some((binding, NativeWindowFacts::live())),
            2 => Some((foreign, NativeWindowFacts::live())),
            _ => panic!("a rejected suffix must not consume the caller's tail"),
        }
    }));
    assert_eq!(
        session
            .report_managed_native_snapshot(observations, NativeWorkAreaRoster::Unknown)
            .unwrap_err()
            .native_kind(),
        Some(NativeHostErrorKind::StaleBinding)
    );
    assert_eq!(reads.get(), 2);
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert_eq!(format!("{:?}", session.native), before);
    session
        .report_managed_native_snapshot(
            std::iter::once((binding, NativeWindowFacts::live())).filter(|_| true),
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    let state = session
        .native
        .as_ref()
        .unwrap()
        .producer
        .binding(binding.binding)
        .unwrap();
    assert_eq!((state.window_generation, state.close_generation), (2, 2));
}

#[test]
fn work_area_length_is_checked_before_borrowing_the_snapshot_iterator() {
    let (mut session, _) = native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    bound_existing(&mut session, 1, 1);
    session.prepare_native_pointer_capacity().unwrap();
    let before = format!("{:?}", session.native);
    let observations =
        std::iter::from_fn(|| panic!("work-area admission precedes iterator access"));
    assert_eq!(
        session
            .report_managed_native_snapshot(
                observations,
                NativeWorkAreaRoster::Exact(vec![work_area(1), work_area(2)]),
            )
            .unwrap_err()
            .native_kind(),
        Some(NativeHostErrorKind::InputCapacity)
    );
    assert_eq!(format!("{:?}", session.native), before);
}

#[test]
fn retired_slots_reject_new_registration_and_quiescence_restores_public_progress() {
    let mut session = DockspaceSession::from_layout(layout(1), DockPolicy::default()).unwrap();
    let limit = NativeInputStorageLimit::new(NonZeroUsize::new(1).unwrap(), WORK_BYTES)
        .with_producer_storage_limit(producer_limit(1, 1, WORK_BYTES));
    session
        .enable_managed_native_host_with_input_storage_limit(
            crate::runtime::NativeCreateStrategy::Staged,
            NativePointerRoster::Exact(Vec::new()),
            limit,
        )
        .unwrap();
    session
        .configure_managed_native_capabilities(full_managed_capabilities())
        .unwrap();
    commit(&mut session);
    session.register_native_root(SURFACE, WINDOW).unwrap();
    let binding = commit(&mut session).native_bindings()[0];
    let replacement = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        OTHER_SURFACE,
        DockspaceRootLayout::new(RootId::new(2), DockspaceNode::central_tabs([ITEM])),
    )])
    .unwrap();
    let reset = session.prepare_layout_reset(replacement).unwrap();
    complete(
        session
            .begin_native_layout_reset_frame(&reset, |_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    assert!(!session.is_current_native_binding(binding));
    assert!(session.recognizes_native_binding(binding));
    session
        .report_managed_native_snapshot(
            [(binding, NativeWindowFacts::destroyed())],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    commit(&mut session);
    session
        .record_native_pointer(unknown_native_pointer_input(9, NativePointerEvent::Moved))
        .unwrap();
    let recorded = session.native_input_recorded_count();
    let watermark = session.engine.backend_ingress_committed_through();
    let backing = session.native_input_producer_storage_capacity_bytes();
    let before = session.native.as_ref().unwrap().producer.bindings.clone();
    assert_eq!(
        session
            .register_native_root(OTHER_SURFACE, HostWindowToken::new(42))
            .unwrap_err()
            .native_kind(),
        Some(NativeHostErrorKind::InputCapacity)
    );
    assert_eq!(
        session
            .report_native_binding_quiescence(binding)
            .unwrap_err()
            .native_kind(),
        Some(NativeHostErrorKind::InputCapacity)
    );
    assert_eq!(
        session.engine.backend_ingress_committed_through(),
        watermark
    );
    assert_eq!(session.native.as_ref().unwrap().producer.bindings, before);
    assert!(session.recognizes_native_binding(binding));
    assert_eq!(session.native_input_recorded_count(), recorded);
    commit(&mut session);
    assert_eq!(
        session.engine.backend_ingress_committed_through().get(),
        recorded
    );
    session.report_native_binding_quiescence(binding).unwrap();
    assert!(!session.recognizes_native_binding(binding));
    assert_eq!(session.native_input_recorded_count(), recorded + 1);
    assert_eq!(
        session.native_input_producer_storage_capacity_bytes(),
        backing
    );
    commit(&mut session);
    session
        .register_native_root(OTHER_SURFACE, HostWindowToken::new(42))
        .unwrap();
    let report = commit(&mut session);
    assert_eq!(report.native_bindings().len(), 1);
    assert_eq!(report.native_bindings()[0].surface(), OTHER_SURFACE);
    assert_eq!(
        session.engine.backend_ingress_committed_through().get(),
        recorded + 2
    );
    assert!(commit(&mut session).inputs().is_empty());
}

#[test]
fn reset_and_tear_off_reject_the_complete_prospective_registry_before_publish() {
    let (mut session, binding) =
        native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    bound_existing(&mut session, 1, 1);
    let reset = session.prepare_layout_reset(layout(1)).unwrap();
    let version = session.version();
    let watermark = session.engine.backend_ingress_committed_through();
    let frontier = session.engine.presentation_identity_frontier();
    let source = session.committed_source_sequence;
    let before = session.native.as_ref().unwrap().producer.bindings.clone();
    let mut frame = session
        .begin_native_layout_reset_frame(&reset, |_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    assert_eq!(
        frame.commit().unwrap_err().native_kind(),
        Some(NativeHostErrorKind::InputCapacity)
    );
    assert_eq!(session.version(), version);
    assert_eq!(
        session.engine.backend_ingress_committed_through(),
        watermark
    );
    assert_eq!(session.engine.presentation_identity_frontier(), frontier);
    assert_eq!(session.committed_source_sequence, source);
    assert_eq!(session.native.as_ref().unwrap().producer.bindings, before);
    assert!(session.is_current_native_binding(binding));

    let (mut session, _) = managed_lifecycle::programmatic_tear_off_session();
    bound_existing(&mut session, 1, 1);
    let version = session.version();
    let watermark = session.engine.backend_ingress_committed_through();
    let frontier = session.engine.presentation_identity_frontier();
    let recorded = session.native_input_recorded_count();
    let source = session.committed_source_sequence;
    session
        .record_native_pointer(unknown_native_pointer_input(9, NativePointerEvent::Moved))
        .unwrap();
    let accepted_prefix = session.native_input_recorded_count();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .move_root_to_new_window_current(
            ROOT,
            crate::runtime::NativeWindowPlacement::new(
                PhysicalRect::new(920.0, 120.0, 420.0, 320.0).unwrap(),
            ),
        )
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    assert_eq!(
        report.inputs(),
        [crate::runtime::HostInputOutcome::ProductActionRejected(
            crate::model::DockspaceActionRejection::NativeUnavailable,
        )]
    );
    assert!(report.take_native_effects().is_empty());
    assert_eq!(session.version(), version);
    assert!(session.engine.backend_ingress_committed_through() > watermark);
    assert_eq!(
        session.engine.backend_ingress_committed_through().get(),
        accepted_prefix
    );
    assert_eq!(session.engine.presentation_identity_frontier(), frontier);
    assert_eq!(session.native_input_recorded_count(), recorded + 1);
    assert_eq!(session.committed_source_sequence, source + 1);
    assert_eq!(session.view().item(ITEM).unwrap().surface(), SURFACE);
    assert!(session.abandoned_native_effects.is_quiescent());
    assert!(commit(&mut session).inputs().is_empty());
}

#[test]
fn dropping_both_prepared_candidates_keeps_one_exact_registration_retry() {
    let mut session = DockspaceSession::from_layout(layout(1), DockPolicy::default()).unwrap();
    enroll(&mut session, producer_limit(1, 1, WORK_BYTES));
    session.register_native_root(SURFACE, WINDOW).unwrap();
    let version = session.version();
    let watermark = session.engine.backend_ingress_committed_through();
    let recorded = session.native_input_recorded_count();
    let before = session.native.as_ref().unwrap().producer.bindings.clone();
    {
        let mut host = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        host.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let DockspaceHostFrame { session, frame, .. } = host;
        let mut frame = frame.into_presentation().unwrap();
        frame
            .resolve_all_presentation_obligations_unavailable(
                crate::engine::HostPresentationUnavailableReason::OutputNotProduced,
            )
            .unwrap();
        let prepared_core = frame.prepare_owned(&session.engine).unwrap();
        let prepared_native = session
            .native
            .as_ref()
            .unwrap()
            .prepare_native_commit(prepared_core.candidate_engine())
            .unwrap();
        assert_eq!(prepared_native.report.bindings.len(), 1);
        assert_eq!(session.native.as_ref().unwrap().producer.bindings, before);
        drop(prepared_native);
        drop(prepared_core);
    }
    assert_eq!(session.version(), version);
    assert_eq!(
        session.engine.backend_ingress_committed_through(),
        watermark
    );
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert_eq!(session.native.as_ref().unwrap().producer.bindings, before);
    let report = commit(&mut session);
    assert_eq!(report.native_bindings().len(), 1);
    assert!(matches!(
        report.inputs(),
        [crate::runtime::HostInputOutcome::NativeSurfaceRegistered { .. }]
    ));
    assert_eq!(
        session.engine.backend_ingress_committed_through().get(),
        recorded
    );
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert!(commit(&mut session).inputs().is_empty());
}

#[test]
fn accepted_pointer_release_rejects_tear_off_at_capacity_and_commits_once() {
    let limit = producer_limit(1, 1, WORK_BYTES);
    let (mut session, binding, receiver) =
        managed_lifecycle::prepare_native_tear_off_release(Some(input_limit(limit)));
    let recorded = session.native_input_recorded_count();
    let watermark = session.engine.backend_ingress_committed_through();
    let pointer = session.native.as_ref().unwrap().recorder.pointer_through();
    let version = session.version();
    let frontier = session.engine.presentation_identity_frontier();
    let before = session.native.as_ref().unwrap().producer.bindings.clone();
    let resolve = |query: NativeReceiverQuery| {
        query
            .bind_receiver(&receiver)
            .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock)
    };
    drop(session.begin_native_host_frame(resolve).unwrap());
    assert_eq!(
        session.engine.backend_ingress_committed_through(),
        watermark
    );
    // The first poll records the already accepted preview presentation. Its
    // backend fact survives a dropped reducer candidate and is not duplicated.
    assert_eq!(session.native_input_recorded_count(), recorded + 1);
    let recorded = session.native_input_recorded_count();
    drop(session.begin_native_host_frame(resolve).unwrap());
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert_eq!(session.native.as_ref().unwrap().producer.bindings, before);
    let mut report = complete(session.begin_native_host_frame(resolve).unwrap());
    assert_eq!(
        report
            .manipulation_feedback()
            .iter()
            .filter(|feedback| matches!(
                feedback.status(),
                crate::runtime::DockspaceManipulationStatus::Rejected(_)
            ))
            .count(),
        1,
        "the consumed release publishes one terminal rejection"
    );
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert_eq!(
        session.engine.backend_ingress_committed_through().get(),
        recorded
    );
    assert_eq!(
        session.native.as_ref().unwrap().recorder.pointer_through(),
        pointer
    );
    assert_eq!(session.version(), version);
    assert_eq!(session.engine.presentation_identity_frontier(), frontier);
    assert_eq!(report.native_bindings(), [binding]);
    assert_eq!(session.native.as_ref().unwrap().producer.bindings, before);
    assert_eq!(
        session.engine.interaction().status(),
        crate::interaction::InteractionStatus::Idle
    );
    assert_eq!(
        session.engine.pointer_button_authority(),
        crate::pointer_journal::AnyButtonDownAuthority::KnownAllReleased
    );
    for request in report.take_native_effects() {
        assert!(!matches!(
            request.operation(),
            crate::runtime::NativeEffectOperation::CreateWindow { .. }
        ));
        let _ = request.accepted();
    }
    assert!(!session.has_pending_native_input());
    let mut next = commit(&mut session);
    assert_eq!(
        session.engine.backend_ingress_committed_through().get(),
        recorded
    );
    assert_eq!(session.native_input_recorded_count(), recorded);
    assert!(next.inputs().is_empty());
    assert!(next.manipulation_feedback().is_empty());
    assert!(next.take_native_effects().is_empty());
}

#[test]
fn default_unlimited_producer_exceeds_a_selected_binding_and_work_area_bound() {
    let selected_bound = 256;
    let mut session =
        DockspaceSession::from_layout(layout(selected_bound + 1), DockPolicy::default()).unwrap();
    session
        .enable_managed_native_host(
            crate::runtime::NativeCreateStrategy::Staged,
            NativePointerRoster::Exact(Vec::new()),
        )
        .unwrap();
    session
        .configure_managed_native_capabilities(full_managed_capabilities())
        .unwrap();
    for identity in 1..=selected_bound + 1 {
        session
            .register_native_root(SurfaceId::new(identity), HostWindowToken::new(identity))
            .unwrap();
    }
    let report = commit(&mut session);
    assert_eq!(
        report.native_bindings().len(),
        (selected_bound + 1) as usize
    );
    assert_eq!(session.native_input_producer_storage_reserved_bytes(), None);
    session
        .report_managed_native_snapshot(
            report
                .native_bindings()
                .iter()
                .copied()
                .map(|binding| (binding, NativeWindowFacts::live())),
            NativeWorkAreaRoster::Exact((1..=65).map(work_area).collect()),
        )
        .unwrap();
    assert_eq!(
        commit(&mut session).native_bindings().len(),
        (selected_bound + 1) as usize
    );
    assert_eq!(
        session.native.as_ref().unwrap().producer.work_areas.len(),
        65
    );
}

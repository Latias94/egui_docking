use super::*;
use crate::model::{DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout};
use crate::runtime::{DockspaceHostFrame, NativeInputStorageLimit, NativeReportStorageLimit};
use std::num::NonZeroUsize;
use std::time::Duration;

const WORK_BYTES: usize = 1024 * 1024;

fn unenrolled_session() -> DockspaceSession {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([ITEM])),
    )])
    .unwrap();
    DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap()
}

fn limit(slots: usize, bytes: usize) -> NativeInputStorageLimit {
    NativeInputStorageLimit::new(NonZeroUsize::new(slots).unwrap(), bytes)
}

fn complete(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

#[test]
fn report_storage_is_promised_before_provider_enrollment_and_rejected_configuration_is_inert() {
    let mut session = unenrolled_session();
    let slots = NonZeroUsize::new(1).unwrap();
    let frontier = session.engine.presentation_identity_frontier();
    let rejected = limit(4, WORK_BYTES)
        .with_report_storage_limit(NativeReportStorageLimit::new(slots, slots, 0));
    assert_eq!(
        session
            .enable_managed_native_host_with_input_storage_limit(
                NativePointerRoster::Exact(Vec::new()),
                rejected,
            )
            .unwrap_err()
            .native_kind(),
        Some(NativeHostErrorKind::InputCapacity)
    );
    assert!(session.native.is_none());
    assert!(session.engine.backend_ingress_provider().is_none());
    assert_eq!(session.engine.presentation_identity_frontier(), frontier);
    assert_eq!(session.native_input_report_storage_reserved_bytes(), None);
    assert!(session.presentation.is_quiescent());
    assert!(session.abandoned_native_effects.is_quiescent());

    let report_bytes = crate::runtime::presentation::RuntimePresentationState::reservation_bytes(1)
        .unwrap()
        + crate::runtime::native_effect::NativeEffectDropQueue::reservation_bytes(1).unwrap()
        + session.presentation.owned_data_capacity_bytes()
        + session.abandoned_native_effects.owned_data_capacity_bytes();
    let one_byte_short = limit(4, WORK_BYTES).with_report_storage_limit(
        NativeReportStorageLimit::new(slots, slots, report_bytes - 1),
    );
    assert_eq!(
        session
            .enable_managed_native_host_with_input_storage_limit(
                NativePointerRoster::Exact(Vec::new()),
                one_byte_short,
            )
            .unwrap_err()
            .native_kind(),
        Some(NativeHostErrorKind::InputCapacity)
    );
    assert!(session.engine.backend_ingress_provider().is_none());
    assert_eq!(session.engine.presentation_identity_frontier(), frontier);
    let reports = NativeReportStorageLimit::new(slots, slots, report_bytes);
    session
        .enable_managed_native_host_with_input_storage_limit(
            NativePointerRoster::Exact(Vec::new()),
            limit(4, WORK_BYTES).with_report_storage_limit(reports),
        )
        .unwrap();
    assert_eq!(
        session.native_input_report_storage_reserved_bytes(),
        Some(report_bytes)
    );
    assert!(session.native_input_pending_report_capacity_bytes() <= report_bytes);
    complete(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    assert_eq!(
        session.native_input_report_storage_reserved_bytes(),
        Some(report_bytes)
    );
}

#[test]
fn preparation_capacity_reclaims_only_committed_input_and_reports_a_full_prefix() {
    let mut session = unenrolled_session();
    session
        .enable_managed_native_host_with_input_storage_limit(
            NativePointerRoster::Exact(Vec::new()),
            limit(2, WORK_BYTES),
        )
        .unwrap();
    assert!(session.has_pending_native_input());
    assert_eq!(session.prepare_native_pointer_capacity().unwrap(), Some(1));
    session
        .record_native_pointer(unknown_native_pointer_input(11, NativePointerEvent::Moved))
        .unwrap();
    let recorded = session.native_input_recorded_count();
    assert_eq!(session.prepare_native_pointer_capacity().unwrap(), Some(0));
    let before = session
        .native
        .as_ref()
        .unwrap()
        .recorder
        .pending_batch()
        .unwrap();
    let version = session.version();
    drop(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    assert!(session.has_pending_native_input());
    assert_eq!(session.prepare_native_pointer_capacity().unwrap(), Some(0));
    assert_eq!(
        session
            .native
            .as_ref()
            .unwrap()
            .recorder
            .pending_batch()
            .unwrap(),
        before
    );
    assert_eq!(session.version(), version);
    complete(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    assert!(!session.has_pending_native_input());
    assert!(
        !session
            .native
            .as_ref()
            .unwrap()
            .recorder
            .retained_records()
            .is_empty()
    );
    assert_eq!(session.prepare_native_pointer_capacity().unwrap(), Some(2));
    assert!(
        session
            .native
            .as_ref()
            .unwrap()
            .recorder
            .retained_records()
            .is_empty()
    );
    assert_eq!(session.native_input_recorded_count(), recorded);
    session
        .record_native_pointer_sequence([
            unknown_native_pointer_input(11, NativePointerEvent::Moved),
            unknown_native_pointer_input(11, NativePointerEvent::Moved),
        ])
        .unwrap();
    assert!(session.has_pending_native_input());
    assert_eq!(session.native_input_recorded_count(), recorded + 2);
}

#[test]
fn preparation_capacity_reports_an_empty_transport_that_cannot_hold_one_pointer_edge() {
    let mut probe = unenrolled_session();
    probe
        .enable_managed_native_host_with_input_storage_limit(
            NativePointerRoster::Exact(Vec::new()),
            limit(1, WORK_BYTES),
        )
        .unwrap();
    let checkpoint_bytes = probe
        .native
        .as_ref()
        .unwrap()
        .recorder
        .native_transport_reserved_bytes()
        .unwrap();
    let mut session = unenrolled_session();
    session
        .enable_managed_native_host_with_input_storage_limit(
            NativePointerRoster::Exact(Vec::new()),
            limit(1, checkpoint_bytes),
        )
        .unwrap();
    complete(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    assert!(!session.has_pending_native_input());
    let recorded = session.native_input_recorded_count();
    for _ in 0..2 {
        assert_eq!(
            session
                .prepare_native_pointer_capacity()
                .unwrap_err()
                .native_kind(),
            Some(NativeHostErrorKind::InputCapacity)
        );
        assert!(!session.has_pending_native_input());
        assert_eq!(session.native_input_recorded_count(), recorded);
    }
}

#[test]
fn bounded_enrollment_rejects_fixed_and_checkpoint_cost_without_publishing_a_provider() {
    let roster = || {
        NativePointerRoster::Exact(vec![NativePointerState::new(
            NativePointerId::new(7),
            [NativePointerButton::Primary],
            NativePointerOwner::None,
        )])
    };
    let mut probe = unenrolled_session();
    probe
        .enable_managed_native_host_with_input_storage_limit(roster(), limit(2, WORK_BYTES))
        .unwrap();
    let required = probe
        .native
        .as_ref()
        .unwrap()
        .recorder
        .native_transport_reserved_bytes()
        .unwrap();
    assert!(required > 0);

    let mut session = unenrolled_session();
    let version = session.version();
    let source = session.committed_source_sequence;
    for bytes in [0, required - 1] {
        let error = session
            .enable_managed_native_host_with_input_storage_limit(roster(), limit(2, bytes))
            .expect_err("neither fixed storage nor checkpoint overflow may partially enroll");
        assert_eq!(
            error.native_kind(),
            Some(NativeHostErrorKind::InputCapacity)
        );
        assert!(session.native.is_none());
        assert_eq!(session.engine.backend_ingress_provider(), None);
        assert_eq!(session.engine.platform_provider(), None);
        assert_eq!(session.engine.pointer_provider(), None);
        assert_eq!(session.native_input_recorded_count(), 0);
        assert_eq!(session.native_input_owned_capacity_bytes().unwrap(), 0);
        assert_eq!(session.version(), version);
        assert_eq!(session.committed_source_sequence, source);
    }

    session
        .enable_managed_native_host_with_input_storage_limit(roster(), limit(2, required))
        .expect("a failed enrollment leaves the same session retryable");
    assert_eq!(session.native_input_recorded_count(), 1);
    assert_eq!(
        session
            .native
            .as_ref()
            .unwrap()
            .recorder
            .pointer_through()
            .get(),
        0
    );
    let frame = session
        .begin_native_host_frame(|_| panic!("an enrollment checkpoint has no receiver edges"))
        .unwrap();
    assert!(!complete(frame).native_input_pending());
    assert!(session.engine.backend_ingress_provider().is_some());
    assert!(session.engine.platform_provider().is_some());
    assert!(session.engine.pointer_provider().is_some());
}

#[test]
fn bounded_pointer_batch_rejects_the_whole_append_before_advancing_accepted_input() {
    let mut session = unenrolled_session();
    session
        .enable_managed_native_host_with_input_storage_limit(
            NativePointerRoster::Exact(Vec::new()),
            limit(2, WORK_BYTES),
        )
        .unwrap();
    let recorder = &session.native.as_ref().unwrap().recorder;
    let before = recorder.pending_batch().unwrap();
    let pointer = recorder.pointer_through();
    let bytes = recorder.native_transport_reserved_bytes();
    let backing = recorder.record_buffer_capacity_bytes();

    let error = session
        .record_native_pointer_batch([
            unknown_native_pointer_input(11, NativePointerEvent::Moved),
            unknown_native_pointer_input(11, NativePointerEvent::StreamEnded),
        ])
        .expect_err("one free slot cannot admit a two-record atomic pointer batch");
    assert_eq!(
        error.native_kind(),
        Some(NativeHostErrorKind::InputCapacity)
    );
    let recorder = &session.native.as_ref().unwrap().recorder;
    assert_eq!(recorder.recorded_through(), before.through());
    assert_eq!(recorder.pointer_through(), pointer);
    assert_eq!(recorder.pending_batch().unwrap(), before);
    assert_eq!(recorder.native_transport_reserved_bytes(), bytes);
    assert_eq!(recorder.record_buffer_capacity_bytes(), backing);

    session
        .record_native_pointer(unknown_native_pointer_input(11, NativePointerEvent::Moved))
        .expect("the refused batch does not consume the remaining slot");
    let recorder = &session.native.as_ref().unwrap().recorder;
    assert_eq!(
        recorder.recorded_through().get(),
        before.through().get() + 1
    );
    assert_eq!(recorder.pointer_through().get(), pointer.get() + 1);
    complete(
        session
            .begin_native_host_frame(|_| panic!("unknown positions cannot request a receiver"))
            .unwrap(),
    );
    session
        .record_native_pointer_batch([
            unknown_native_pointer_input(11, NativePointerEvent::Moved),
            unknown_native_pointer_input(11, NativePointerEvent::StreamEnded),
        ])
        .expect("the same complete batch fits after the accepted prefix drains");
    assert_eq!(
        session.native_input_recorded_count(),
        before.through().get() + 3
    );
    assert_eq!(
        session
            .native
            .as_ref()
            .unwrap()
            .recorder
            .pointer_through()
            .get(),
        pointer.get() + 3
    );
    complete(
        session
            .begin_native_host_frame(|_| panic!("unknown positions cannot request a receiver"))
            .unwrap(),
    );
}

#[test]
fn full_input_slot_drains_before_retrying_the_same_pending_presentation_once() {
    let mut session = unenrolled_session();
    session
        .enable_managed_native_host_with_input_storage_limit(
            NativePointerRoster::Exact(Vec::new()),
            limit(1, WORK_BYTES),
        )
        .unwrap();
    session
        .configure_managed_native_capabilities(full_managed_capabilities())
        .unwrap();
    commit_managed_frame(&mut session);
    session.register_native_root(SURFACE, WINDOW).unwrap();
    let report = complete(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    let binding = report
        .inputs()
        .iter()
        .find_map(|input| match input {
            crate::runtime::HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
            _ => None,
        })
        .unwrap();
    let bounds = PhysicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap();
    let scale = ScaleFactor::new(1.0).unwrap();
    session
        .report_managed_native_snapshot(
            [(
                binding,
                NativeWindowFacts::live()
                    .with_content_bounds(bounds)
                    .with_outer_bounds(bounds)
                    .with_native_scale_factor(scale)
                    .with_presentation_scale_factor(scale)
                    .with_input(NativeWindowInputState::ReceivesInput, None)
                    .with_presentation(NativeWindowPresentationState::Visible)
                    .with_close(NativeCloseState::Clear, None),
            )],
            NativeWorkAreaRoster::Exact(vec![work_area(1)]),
        )
        .unwrap();
    commit_managed_frame(&mut session);
    let (output, _) = paint_native_output(&mut session);
    let artifact = output.presentation_artifact();
    session
        .record_native_pointer(unknown_native_pointer_input(11, NativePointerEvent::Moved))
        .unwrap();
    let recorded = session.native_input_recorded_count();
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .expect(
            "the presentation sidecar retains the affine output while recorder storage is full",
        );
    let clock = NativeMonotonicClock::new(|| Duration::ZERO);
    let poll = |session: &mut DockspaceSession| {
        complete(
            session
                .poll_native_host_frame(&mut NativeInputBudget::new(&clock), |_| {
                    panic!("unknown positions cannot request a receiver")
                })
                .unwrap()
                .expect("accepted records must drain despite the blocked presentation append"),
        )
    };
    let drained = poll(&mut session);
    assert!(drained.native_input_pending());
    assert!(drained.surface_presentation_commits().is_empty());
    assert_eq!(session.native_input_recorded_count(), recorded);

    {
        let retry = session
            .poll_native_host_frame(&mut NativeInputBudget::new(&clock), |_| {
                panic!("a presentation observation has no receiver edges")
            })
            .unwrap()
            .expect("the pending presentation append retries after prefix reclamation");
        assert!(!retry.native_input_pending());
    }
    assert_eq!(session.native_input_recorded_count(), recorded + 1);
    let settled = poll(&mut session);
    assert!(!settled.native_input_pending());
    assert_eq!(settled.surface_presentation_commits().len(), 1);
    assert!(settled.surface_presentation_commits()[0].settles(artifact));
    assert_eq!(session.native_input_recorded_count(), recorded + 1);

    let quiet = poll(&mut session);
    assert!(!quiet.native_input_pending());
    assert!(quiet.surface_presentation_commits().is_empty());
    assert_eq!(session.native_input_recorded_count(), recorded + 1);
}

#[test]
fn snapshot_byte_rejection_preserves_generations_and_exact_capacity_retries() {
    use crate::backend_ingress::BackendIngressPayload;
    use crate::platform::ObservedWorkArea;
    use std::sync::atomic::AtomicU8;

    let (mut session, binding) =
        native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    session
        .report_managed_native_snapshot(
            [(binding, NativeWindowFacts::live())],
            NativeWorkAreaRoster::Exact(vec![work_area(1)]),
        )
        .unwrap();
    commit_managed_frame(&mut session);
    let slots = NonZeroUsize::new(4).unwrap();
    let native = session.native.as_mut().unwrap();
    native
        .reclaim_committed_prefix(&mut session.engine)
        .unwrap();
    native
        .recorder
        .configure_native_transport_capacity(slots, WORK_BYTES)
        .unwrap();
    let fixed = native.recorder.native_transport_reserved_bytes().unwrap();
    let descendants = size_of::<ViewportBinding>()
        + size_of::<ObservedWindow>()
        + size_of::<WindowCloseObservation>()
        + 3 * size_of::<ObservedWorkArea>();
    let required = fixed + size_of::<BackendIngressPayload>() + size_of::<AtomicU8>() + descendants;
    native
        .recorder
        .configure_native_transport_capacity(slots, required - 1)
        .unwrap();
    let before = (
        native.snapshot_generation,
        native.producer.bindings.clone(),
        native.focus_generation,
        native.focus,
        native.binding_roster_unsettled_through,
    );
    let recorded = native.recorder.recorded_through();
    let pointer = native.recorder.pointer_through();
    let payloads = native.recorder.pending_batch().unwrap();
    let backing = native.recorder.record_buffer_capacity_bytes();
    let version = session.version();
    let work_areas = || {
        let mut areas = Vec::with_capacity(16);
        areas.extend((1..=3).map(work_area));
        NativeWorkAreaRoster::Exact(areas)
    };

    let error = session
        .report_managed_native_snapshot([(binding, NativeWindowFacts::live())], work_areas())
        .expect_err("one byte short must reject before publishing any snapshot state");
    assert_eq!(
        error.native_kind(),
        Some(NativeHostErrorKind::InputCapacity)
    );
    let native = session.native.as_mut().unwrap();
    assert_eq!(
        (
            native.snapshot_generation,
            native.producer.bindings.clone(),
            native.focus_generation,
            native.focus,
            native.binding_roster_unsettled_through,
        ),
        before
    );
    assert_eq!(native.recorder.recorded_through(), recorded);
    assert_eq!(native.recorder.pointer_through(), pointer);
    assert_eq!(native.recorder.pending_batch().unwrap(), payloads);
    assert_eq!(native.recorder.record_buffer_capacity_bytes(), backing);
    assert_eq!(
        native.recorder.native_transport_reserved_bytes(),
        Some(fixed)
    );
    native
        .recorder
        .configure_native_transport_capacity(slots, required)
        .unwrap();
    session
        .report_managed_native_snapshot([(binding, NativeWindowFacts::live())], work_areas())
        .expect("the same complete snapshot fits its exact prospective allowance");
    let native = session.native.as_ref().unwrap();
    assert_eq!(native.recorder.recorded_through().get(), recorded.get() + 1);
    assert_eq!(native.recorder.pointer_through(), pointer);
    assert_eq!(native.snapshot_generation, before.0 + 1);
    assert_eq!(
        native.recorder.native_transport_reserved_bytes(),
        Some(required)
    );
    let BackendIngressPayload::PlatformSnapshot { snapshot, .. } =
        native.recorder.retained_records().last().unwrap().payload()
    else {
        panic!("the accepted record must own the complete platform snapshot");
    };
    assert_eq!(snapshot.owned_payload_capacity_bytes(), descendants);
    assert!(session.native_input_owned_capacity_bytes().unwrap() <= required);
    assert_eq!(session.version(), version);
}

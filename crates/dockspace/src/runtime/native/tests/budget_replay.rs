use super::*;

use crate::backend_ingress::BackendIngressPayload;
use crate::runtime::{DockspaceHostFrame, DockspaceRuntimeErrorKind};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

fn managed_session() -> DockspaceSession {
    let (mut session, _) = native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    commit_managed_frame(&mut session);
    assert!(
        session
            .native
            .as_ref()
            .expect("the managed host exists")
            .recorder
            .batch_after(session.engine.backend_ingress_committed_through())
            .expect("the initial native prefix is committed")
            .is_empty()
    );
    session
}

fn fixed_clock() -> NativeMonotonicClock {
    NativeMonotonicClock::new(|| Duration::ZERO)
}

fn poll_frame<'a>(
    session: &'a mut DockspaceSession,
    budget: &mut NativeInputBudget,
) -> DockspaceHostFrame<'a> {
    session
        .poll_native_host_frame(budget, |_| {
            panic!("unknown pointer locations must not query a rendered receiver")
        })
        .expect("the native prefix polls successfully")
        .expect("the independent prefix can seal")
}

fn commit_frame(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the budgeted frame settles every surface");
    frame.commit().expect("the budgeted prefix commits")
}

fn pending_pointer_sequences(session: &DockspaceSession) -> Vec<u64> {
    let batch = session
        .native
        .as_ref()
        .expect("the managed host exists")
        .recorder
        .batch_after(session.engine.backend_ingress_committed_through())
        .expect("the unpublished suffix remains replayable");
    batch
        .records()
        .iter()
        .map(|record| {
            let BackendIngressPayload::PointerSegment(journal) = record.payload() else {
                panic!("the replay suffix must contain only pointer records");
            };
            let [edge] = journal.edges() else {
                panic!("each native pointer record contains exactly one edge");
            };
            assert_eq!(journal.previous().get() + 1, edge.sequence().get());
            assert_eq!(journal.through(), edge.sequence());
            edge.sequence().get()
        })
        .collect()
}

#[test]
fn sequence_257_commits_256_then_one_with_exact_watermarks() {
    let mut session = managed_session();
    let ingress_before = session.engine.backend_ingress_committed_through().get();
    let source_before = session.committed_source_sequence;
    let semantic_before = session.engine.semantic_input_watermark();
    let pointer_before = session
        .native
        .as_ref()
        .unwrap()
        .recorder
        .pointer_through()
        .get();
    session
        .record_native_pointer_sequence(
            (0..257).map(|_| unknown_native_pointer_input(11, NativePointerEvent::Moved)),
        )
        .expect("257 independent pointer records are admitted");

    let clock = fixed_clock();
    let mut first_budget = NativeInputBudget::new(&clock);
    let first = poll_frame(&mut session, &mut first_budget);
    assert!(first.native_input_pending());
    assert_eq!(first_budget.remaining_records(), 0);
    assert_eq!(
        first.frame.backend_ingress_through().get(),
        ingress_before + 256
    );
    assert_eq!(first.next_source_sequence, source_before);
    assert!(commit_frame(first).native_input_pending());
    assert_eq!(
        session.engine.backend_ingress_committed_through().get(),
        ingress_before + 256
    );
    assert_eq!(session.committed_source_sequence, source_before);
    assert_eq!(session.engine.semantic_input_watermark(), semantic_before);
    assert_eq!(
        pending_pointer_sequences(&session),
        vec![pointer_before + 257]
    );

    let mut second_budget = NativeInputBudget::new(&clock);
    let second = poll_frame(&mut session, &mut second_budget);
    assert!(!second.native_input_pending());
    assert_eq!(second_budget.remaining_records(), 255);
    assert!(!commit_frame(second).native_input_pending());
    assert_eq!(
        session.engine.backend_ingress_committed_through().get(),
        ingress_before + 257
    );
    assert_eq!(
        session
            .native
            .as_ref()
            .unwrap()
            .recorder
            .pointer_through()
            .get(),
        pointer_before + 257
    );
    assert_eq!(session.committed_source_sequence, source_before);
    assert_eq!(session.engine.semantic_input_watermark(), semantic_before);
    assert!(pending_pointer_sequences(&session).is_empty());
}

#[test]
fn atomic_257_retains_256_privately_and_excludes_later_appends() {
    let mut session = managed_session();
    let version_before = session.version();
    let ingress_before = session.engine.backend_ingress_committed_through();
    let input_before = session.engine.last_input_sequence();
    let source_before = session.committed_source_sequence;
    let semantic_before = session.engine.semantic_input_watermark();
    let pointer_before = session
        .native
        .as_ref()
        .unwrap()
        .recorder
        .pointer_through()
        .get();
    session
        .record_native_pointer_batch(
            (0..257).map(|_| unknown_native_pointer_input(11, NativePointerEvent::Moved)),
        )
        .expect("one indivisible 257-record callback is admitted");
    let recorded = session.native.as_ref().unwrap().recorder.recorded_through();
    assert_eq!(recorded.get(), ingress_before.get() + 257);

    let clock = fixed_clock();
    let mut first_budget = NativeInputBudget::new(&clock);
    assert!(
        session
            .poll_native_host_frame(&mut first_budget, |_| {
                panic!("unknown pointer locations cannot ask a receiver question")
            })
            .expect("the oversized atomic group suspends successfully")
            .is_none()
    );
    assert_eq!(first_budget.remaining_records(), 0);
    assert!(session.native.as_ref().unwrap().pending_replay.is_some());
    assert_eq!(session.version(), version_before);
    assert_eq!(session.engine.last_input_sequence(), input_before);
    assert_eq!(
        session.engine.backend_ingress_committed_through(),
        ingress_before
    );
    assert_eq!(session.committed_source_sequence, source_before);
    assert_eq!(session.engine.semantic_input_watermark(), semantic_before);
    assert_eq!(
        session.native.as_ref().unwrap().recorder.recorded_through(),
        recorded
    );
    assert_eq!(pending_pointer_sequences(&session).len(), 257);

    let (attempt, candidate_through, replay_buffer) = {
        let pending = session
            .native
            .as_ref()
            .unwrap()
            .pending_replay
            .as_ref()
            .unwrap();
        (
            pending.frame.presentation_attempt(),
            pending.frame.backend_ingress_through(),
            pending.frame.backend_ingress_buffer_capacity_bytes(),
        )
    };
    session
        .record_native_pointer(unknown_native_pointer_input(12, NativePointerEvent::Moved))
        .expect("a later callback appends outside the frozen candidate");
    {
        let native = session.native.as_ref().unwrap();
        let pending = native.pending_replay.as_ref().unwrap();
        assert_eq!(pending.frame.presentation_attempt(), attempt);
        assert_eq!(pending.frame.backend_ingress_through(), candidate_through);
        assert_eq!(
            pending.frame.backend_ingress_buffer_capacity_bytes(),
            replay_buffer
        );
        assert_eq!(native.recorder.recorded_through().get(), recorded.get() + 1);
        assert_eq!(native.recorder.retained_records().len(), 258);
    }
    assert_eq!(
        session.engine.backend_ingress_committed_through(),
        ingress_before
    );
    assert_eq!(pending_pointer_sequences(&session).len(), 258);
    let mut second_budget = NativeInputBudget::new(&clock);
    let second = poll_frame(&mut session, &mut second_budget);
    assert_eq!(
        second_budget.remaining_records(),
        255,
        "only the retained final record reduces"
    );
    assert_eq!(second.frame.backend_ingress_through(), recorded);
    assert!(second.native_input_pending());
    assert_eq!(
        second.session.engine.backend_ingress_committed_through(),
        ingress_before
    );
    assert!(commit_frame(second).native_input_pending());
    assert!(session.native.as_ref().unwrap().pending_replay.is_none());
    assert_eq!(session.engine.backend_ingress_committed_through(), recorded);
    assert_eq!(
        pending_pointer_sequences(&session),
        vec![pointer_before + 258]
    );

    let mut tail_budget = NativeInputBudget::new(&clock);
    let tail = poll_frame(&mut session, &mut tail_budget);
    assert_eq!(tail_budget.remaining_records(), 255);
    assert!(!tail.native_input_pending());
    assert!(!commit_frame(tail).native_input_pending());
    assert_eq!(
        session.engine.backend_ingress_committed_through().get(),
        recorded.get() + 1
    );
    assert_eq!(session.committed_source_sequence, source_before);
    assert_eq!(session.engine.semantic_input_watermark(), semantic_before);
    assert!(pending_pointer_sequences(&session).is_empty());
}

#[test]
fn clock_deadline_seals_after_one_record_without_starting_its_successor() {
    let mut session = managed_session();
    let ingress_before = session.engine.backend_ingress_committed_through().get();
    let pointer_before = session
        .native
        .as_ref()
        .unwrap()
        .recorder
        .pointer_through()
        .get();
    session
        .record_native_pointer_sequence(
            (0..2).map(|_| unknown_native_pointer_input(11, NativePointerEvent::Moved)),
        )
        .expect("two independent records are admitted");

    // Construction and the first pre-reduction check see zero; the next check
    // reaches the inclusive deadline. This tests boundary checks, not resolver cost.
    let calls = Arc::new(AtomicU64::new(0));
    let source = Arc::clone(&calls);
    let clock = NativeMonotonicClock::new(move || {
        if source.fetch_add(1, Ordering::SeqCst) < 2 {
            Duration::ZERO
        } else {
            Duration::from_millis(2)
        }
    });
    let mut budget = NativeInputBudget::new(&clock);
    let first = poll_frame(&mut session, &mut budget);
    assert_eq!(budget.remaining_records(), 255);
    assert_eq!(
        first.frame.backend_ingress_through().get(),
        ingress_before + 1
    );
    assert!(first.native_input_pending());
    assert!(commit_frame(first).native_input_pending());
    assert_eq!(
        pending_pointer_sequences(&session),
        vec![pointer_before + 2]
    );

    let mut tail_budget = NativeInputBudget::new(&fixed_clock());
    let tail = poll_frame(&mut session, &mut tail_budget);
    assert_eq!(tail_budget.remaining_records(), 255);
    assert!(!commit_frame(tail).native_input_pending());
    assert_eq!(
        session.engine.backend_ingress_committed_through().get(),
        ingress_before + 2
    );
}

#[test]
fn dropping_a_budgeted_prefix_retries_without_advancing_live_watermarks() {
    let mut session = managed_session();
    let version_before = session.version();
    let ingress_before = session.engine.backend_ingress_committed_through();
    let input_before = session.engine.last_input_sequence();
    let source_before = session.committed_source_sequence;
    let semantic_before = session.engine.semantic_input_watermark();
    session
        .record_native_pointer_sequence(
            (0..257).map(|_| unknown_native_pointer_input(11, NativePointerEvent::Moved)),
        )
        .expect("a sequence with an unpublished tail is admitted");
    let retained = session
        .native
        .as_ref()
        .unwrap()
        .recorder
        .batch_after(ingress_before)
        .unwrap();
    let clock = fixed_clock();

    for _ in 0..3 {
        let mut budget = NativeInputBudget::new(&clock);
        let frame = poll_frame(&mut session, &mut budget);
        assert!(frame.native_input_pending());
        assert_eq!(budget.remaining_records(), 0);
        assert_eq!(
            frame.frame.backend_ingress_through().get(),
            ingress_before.get() + 256
        );
        drop(frame);
        assert_eq!(session.version(), version_before);
        assert_eq!(session.engine.last_input_sequence(), input_before);
        assert_eq!(
            session.engine.backend_ingress_committed_through(),
            ingress_before
        );
        assert_eq!(session.committed_source_sequence, source_before);
        assert_eq!(session.engine.semantic_input_watermark(), semantic_before);
        assert!(session.native.as_ref().unwrap().pending_replay.is_none());
        assert_eq!(
            session
                .native
                .as_ref()
                .unwrap()
                .recorder
                .batch_after(ingress_before)
                .unwrap(),
            retained
        );
    }

    let mut budget = NativeInputBudget::new(&clock);
    assert!(commit_frame(poll_frame(&mut session, &mut budget)).native_input_pending());
    let mut tail_budget = NativeInputBudget::new(&clock);
    assert!(!commit_frame(poll_frame(&mut session, &mut tail_budget)).native_input_pending());
    assert_eq!(
        session.engine.backend_ingress_committed_through().get(),
        ingress_before.get() + 257
    );
    assert!(pending_pointer_sequences(&session).is_empty());
}

#[test]
fn pending_native_tail_rejects_current_and_prepared_actions_without_consuming_source() {
    let mut session = managed_session();
    let source_before = session.committed_source_sequence;
    let semantic_before = session.engine.semantic_input_watermark();
    let prepared = session.prepare_select_item(ITEM);
    session
        .record_native_pointer_sequence(
            (0..257).map(|_| unknown_native_pointer_input(11, NativePointerEvent::Moved)),
        )
        .expect("the native sequence leaves one record beyond the frame budget");

    let clock = fixed_clock();
    let mut budget = NativeInputBudget::new(&clock);
    let mut frame = poll_frame(&mut session, &mut budget);
    assert!(frame.native_input_pending());
    let current_error = frame
        .select_item_current(ITEM)
        .expect_err("native backlog gates current actions");
    assert_eq!(
        current_error.kind(),
        DockspaceRuntimeErrorKind::OperationConflict
    );
    let prepared_error = frame
        .submit_prepared_action(prepared)
        .expect_err("native backlog gates prepared actions");
    assert_eq!(
        prepared_error.kind(),
        DockspaceRuntimeErrorKind::OperationConflict
    );
    assert_eq!(frame.next_source_sequence, source_before);
    assert!(commit_frame(frame).native_input_pending());
    assert_eq!(session.committed_source_sequence, source_before);
    assert_eq!(session.engine.semantic_input_watermark(), semantic_before);

    let prepared = session.prepare_select_item(ITEM);
    let mut tail_budget = NativeInputBudget::new(&clock);
    let mut tail = poll_frame(&mut session, &mut tail_budget);
    assert!(!tail.native_input_pending());
    tail.submit_prepared_action(prepared)
        .expect("application input resumes once the tail drains");
    assert_eq!(tail.next_source_sequence, source_before + 1);
    assert!(!commit_frame(tail).native_input_pending());
    assert_eq!(session.committed_source_sequence, source_before + 1);
    assert_eq!(
        session.engine.semantic_input_watermark().unwrap().get(),
        source_before + 1
    );
}

#[test]
fn uncommitted_roster_change_stays_unsettled_after_an_earlier_prefix_commit() {
    let template = managed_session();
    let mut session = DockspaceSession::from_workspace_for_test(
        template.workspace().clone(),
        DockPolicy::default(),
    )
    .expect("the unregistered workspace must initialize");
    session
        .enable_managed_native_host(
            crate::runtime::NativeCreateStrategy::Staged,
            NativePointerRoster::Exact(Vec::new()),
        )
        .expect("the native host must enroll before registration");
    commit_managed_frame(&mut session);
    session
        .record_native_pointer_sequence(
            (0..256).map(|_| unknown_native_pointer_input(11, NativePointerEvent::Moved)),
        )
        .expect("the predecessor prefix is accepted");
    session
        .register_native_root(SURFACE, WINDOW)
        .expect("the initial root registration follows the predecessor prefix");
    let through = session
        .native
        .as_ref()
        .unwrap()
        .binding_roster_unsettled_through;
    assert_eq!(
        through,
        Some(session.native.as_ref().unwrap().recorder.recorded_through())
    );

    let mut budget = NativeInputBudget::new(&fixed_clock());
    assert!(commit_frame(poll_frame(&mut session, &mut budget)).native_input_pending());
    assert_eq!(
        session
            .native
            .as_ref()
            .unwrap()
            .binding_roster_unsettled_through,
        through
    );
    assert_eq!(
        session
            .report_managed_native_snapshot([], NativeWorkAreaRoster::Unknown)
            .expect_err("a partial commit cannot authorize another roster snapshot")
            .native_kind(),
        Some(NativeHostErrorKind::OperationConflict),
    );

    let mut tail_budget = NativeInputBudget::new(&fixed_clock());
    assert!(!commit_frame(poll_frame(&mut session, &mut tail_budget)).native_input_pending());
    assert!(
        session
            .native
            .as_ref()
            .unwrap()
            .binding_roster_unsettled_through
            .is_none()
    );
    assert_eq!(
        session.engine.backend_ingress_committed_through(),
        through.unwrap()
    );
}

#[test]
fn already_expired_budget_retains_the_unstarted_attempt_without_claiming_a_record() {
    let mut session = managed_session();
    session
        .record_native_pointer(unknown_native_pointer_input(11, NativePointerEvent::Moved))
        .expect("the pointer record is accepted");
    let before = session.engine.backend_ingress_committed_through();
    let nanos = Arc::new(AtomicU64::new(0));
    let source = Arc::clone(&nanos);
    let clock =
        NativeMonotonicClock::new(move || Duration::from_nanos(source.load(Ordering::SeqCst)));
    let mut expired = NativeInputBudget::new(&clock);
    nanos.store(2_000_000, Ordering::SeqCst);
    assert!(
        session
            .poll_native_host_frame(&mut expired, |_| panic!("no record can start"))
            .expect("a prior-stage deadline yields without error")
            .is_none()
    );
    assert_eq!(expired.remaining_records(), 256);
    assert_eq!(session.engine.backend_ingress_committed_through(), before);
    assert!(session.native.as_ref().unwrap().pending_replay.is_some());
    let mut next = NativeInputBudget::new(&clock);
    assert!(!commit_frame(poll_frame(&mut session, &mut next)).native_input_pending());
    assert_eq!(next.remaining_records(), 255);
    assert_eq!(
        session.engine.backend_ingress_committed_through().get(),
        before.get() + 1
    );
}

#[test]
fn revoked_suspended_atomic_candidate_cannot_publish_reused_record_ordinals() {
    let mut session = managed_session();
    let before = session.engine.backend_ingress_committed_through();
    let original_version = session.version();
    let start = session.native.as_ref().unwrap().recorder.savepoint();
    let append = |session: &mut DockspaceSession| {
        session
            .record_native_pointer_batch(
                (0..257).map(|_| unknown_native_pointer_input(11, NativePointerEvent::Moved)),
            )
            .expect("the complete indivisible group must record");
    };
    append(&mut session);
    let mut first = NativeInputBudget::new(&fixed_clock());
    assert!(
        session
            .poll_native_host_frame(&mut first, |_| NativeReceiverAnswer::Unknown)
            .expect("the atomic group suspends")
            .is_none()
    );
    session
        .native
        .as_mut()
        .unwrap()
        .recorder
        .rollback_to(start)
        .expect("the unpublished candidate must retain producer rollback authority");
    append(&mut session);
    let mut rejected = NativeInputBudget::new(&fixed_clock());
    assert!(
        session
            .poll_native_host_frame(&mut rejected, |_| NativeReceiverAnswer::Unknown)
            .is_err()
    );
    assert_eq!(session.version(), original_version);
    assert_eq!(session.engine.backend_ingress_committed_through(), before);
    assert!(session.native.as_ref().unwrap().pending_replay.is_none());

    let mut retry = NativeInputBudget::new(&fixed_clock());
    assert!(
        session
            .poll_native_host_frame(&mut retry, |_| NativeReceiverAnswer::Unknown)
            .expect("the replacement group starts from the published watermark")
            .is_none()
    );
    let mut finish = NativeInputBudget::new(&fixed_clock());
    assert!(!commit_frame(poll_frame(&mut session, &mut finish)).native_input_pending());
    assert_eq!(finish.remaining_records(), 255);
    assert_eq!(
        session.engine.backend_ingress_committed_through().get(),
        before.get() + 257
    );
    assert_eq!(session.version(), original_version);
}

#[test]
fn split_presentation_reports_settle_only_the_stream_in_each_exact_prefix() {
    const OTHER_SURFACE: SurfaceId = SurfaceId::new(2);
    let mut builder = Workspace::builder();
    for id in 1..=2 {
        let tabs = builder.insert_node(Node::tabs([ItemId::new(id)]));
        builder.set_root(RootId::new(id), RootRecord::new(tabs));
        builder.set_surface(
            SurfaceId::new(id),
            SurfacePresentation::with_main(RootId::new(id)),
        );
    }
    let mut session = DockspaceSession::from_workspace_for_test(
        builder.build().expect("the two-surface workspace is valid"),
        DockPolicy::default(),
    )
    .expect("the session must initialize");
    session
        .enable_managed_native_host(
            crate::runtime::NativeCreateStrategy::Staged,
            NativePointerRoster::Exact(Vec::new()),
        )
        .expect("the native host must enroll");
    session
        .configure_managed_native_capabilities(full_managed_capabilities())
        .expect("the native capabilities must configure");
    session
        .register_native_root(SURFACE, HostWindowToken::new(1))
        .unwrap();
    session
        .register_native_root(OTHER_SURFACE, HostWindowToken::new(2))
        .unwrap();
    commit_managed_frame(&mut session);
    let bounds = PhysicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap();
    let scale = ScaleFactor::new(1.0).unwrap();
    let facts = NativeWindowFacts::live()
        .with_content_bounds(bounds)
        .with_outer_bounds(bounds)
        .with_native_scale_factor(scale)
        .with_presentation_scale_factor(scale)
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible, None)
        .with_close(NativeCloseState::Clear, None);
    let observations = session
        .native
        .as_ref()
        .unwrap()
        .current_bindings()
        .map(|binding| (binding, facts))
        .collect::<Vec<_>>();
    session
        .report_managed_native_snapshot(
            observations,
            NativeWorkAreaRoster::Exact(vec![work_area(1)]),
        )
        .expect("the exact native roster must record");
    commit_managed_frame(&mut session);
    let (first, _) = paint_native_surface_output(&mut session, SURFACE);
    let (second, _) = paint_native_surface_output(&mut session, OTHER_SURFACE);
    let first_artifact = first.presentation_artifact();
    let second_artifact = second.presentation_artifact();
    session
        .report_surface_presentation(first, SurfacePresentationResult::Presented)
        .unwrap();
    session
        .report_surface_presentation(second, SurfacePresentationResult::Presented)
        .unwrap();
    let before = session.engine.backend_ingress_committed_through();
    session
        .record_native_pointer_sequence(
            (0..255).map(|_| unknown_native_pointer_input(11, NativePointerEvent::Moved)),
        )
        .expect("the predecessor prefix must leave one report in its record budget");

    let mut first_budget = NativeInputBudget::new(&fixed_clock());
    let first_report = commit_frame(poll_frame(&mut session, &mut first_budget));
    assert!(first_report.native_input_pending());
    assert_eq!(first_report.surface_presentation_commits().len(), 1);
    assert_eq!(
        session.engine.backend_ingress_committed_through().get(),
        before.get() + 256
    );
    let mut next_budget = NativeInputBudget::new(&fixed_clock());
    let second_report = commit_frame(poll_frame(&mut session, &mut next_budget));
    assert!(!second_report.native_input_pending());
    assert_eq!(second_report.surface_presentation_commits().len(), 1);
    let first_commit = first_report.surface_presentation_commits()[0];
    let second_commit = second_report.surface_presentation_commits()[0];
    assert_ne!(
        first_commit.settles(first_artifact),
        first_commit.settles(second_artifact)
    );
    assert_ne!(
        second_commit.settles(first_artifact),
        second_commit.settles(second_artifact)
    );
    assert_ne!(
        first_commit.settles(first_artifact),
        second_commit.settles(first_artifact)
    );
    assert_eq!(
        session.engine.backend_ingress_committed_through().get(),
        before.get() + 257
    );
    let mut quiet_budget = NativeInputBudget::new(&fixed_clock());
    let quiet = commit_frame(poll_frame(&mut session, &mut quiet_budget));
    assert!(!quiet.native_input_pending());
    assert!(quiet.surface_presentation_commits().is_empty());
    assert_eq!(quiet_budget.remaining_records(), 256);
}

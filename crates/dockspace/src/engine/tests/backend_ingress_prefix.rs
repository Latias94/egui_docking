use super::*;

fn record_validations(recorder: &mut BackendIngressRecorder, count: usize) {
    for _ in 0..count {
        recorder
            .record_semantic_input(EngineInput::ValidateWorkspace)
            .expect("a validation record must append");
    }
}

fn begin_prefix_fixture() -> (DockEngine, PresentationHostLease, BackendIngressRecorder) {
    let mut engine = single_surface_engine(SOURCE_SURFACE, SOURCE_ROOT, ItemId::new(1));
    let host = engine
        .create_presentation_host()
        .expect("the presentation host must enroll");
    let recorder = engine
        .create_backend_ingress_provider(host, PointerEdgeSequence::new(0))
        .expect("the joined provider must enroll");
    (engine, host, recorder)
}

#[test]
fn sealed_256_record_prefix_excludes_revoked_tail_and_resumes_at_exact_identity() {
    let (mut engine, host, mut recorder) = begin_prefix_fixture();
    let initial_version = engine.version();
    record_validations(&mut recorder, 256);
    let tail_savepoint = recorder.savepoint();
    record_validations(&mut recorder, 1);
    let frozen = recorder
        .pending_batch()
        .expect("the complete batch must freeze");
    let mut frame = begin_test_host_frame(&engine, host);
    let mut progress = frame
        .submit_backend_ingress(frozen)
        .expect("the first record must reduce");
    for completed_records in 1..=256 {
        assert_eq!(
            progress,
            BackendIngressProgress::RecordBoundary {
                completed_records,
                can_seal: true
            },
        );
        assert_eq!(frame.candidate.backend_ingress.committed_through().get(), 0);
        assert_eq!(engine.backend_ingress_committed_through().get(), 0);
        if completed_records < 256 {
            progress = frame
                .resume_backend_ingress()
                .expect("exactly one successor must reduce");
        }
    }
    let sealed = frame
        .seal_backend_ingress_prefix()
        .expect("the completed prefix must seal");
    assert_eq!(sealed.get(), 256);
    assert_eq!(engine.backend_ingress_committed_through().get(), 0);
    recorder
        .rollback_to(tail_savepoint)
        .expect("the unprocessed tail must remain producer-owned");
    complete_host_frame_with_explicit_surface_roster(&engine, &mut frame);
    let transition = frame.finish(&mut engine).expect(
        "revoking the selected but unprocessed tail must not invalidate the prefix commit guard",
    );
    assert_eq!(transition.reduced_inputs().len(), 256);
    assert_eq!(engine.backend_ingress_committed_through(), sealed);
    assert_eq!(engine.version(), initial_version);

    record_validations(&mut recorder, 1);
    let suffix = recorder
        .batch_after(sealed)
        .expect("the fresh tail must continue the exact committed predecessor");
    assert_eq!(suffix.len(), 1);
    assert_eq!(suffix.previous(), sealed);
    let mut next = begin_test_host_frame(&engine, host);
    assert_eq!(
        next.submit_backend_ingress(suffix)
            .expect("the replacement tail must reduce"),
        BackendIngressProgress::Complete,
    );
    complete_host_frame_with_explicit_surface_roster(&engine, &mut next);
    next.finish(&mut engine)
        .expect("the replacement tail must publish exactly once");
    assert_eq!(engine.backend_ingress_committed_through().get(), 257);
}

#[test]
fn dropped_sealed_prefix_retries_from_published_watermark() {
    let (mut engine, host, mut recorder) = begin_prefix_fixture();
    record_validations(&mut recorder, 3);
    let frozen = recorder
        .pending_batch()
        .expect("the complete batch must freeze");
    let mut dropped = begin_test_host_frame(&engine, host);
    assert_eq!(
        dropped
            .submit_backend_ingress(frozen.clone())
            .expect("the first record must reduce"),
        BackendIngressProgress::RecordBoundary {
            completed_records: 1,
            can_seal: true,
        },
    );
    dropped
        .seal_backend_ingress_prefix()
        .expect("the first record must seal privately");
    drop(dropped);
    assert_eq!(engine.backend_ingress_committed_through().get(), 0);
    assert_eq!(
        recorder
            .batch_after(engine.backend_ingress_committed_through())
            .expect("discarding a frame must preserve the full immutable retry"),
        frozen,
    );

    let mut retry = begin_test_host_frame(&engine, host);
    let mut progress = retry
        .submit_backend_ingress(frozen)
        .expect("the identical batch must retry");
    while matches!(progress, BackendIngressProgress::RecordBoundary { .. }) {
        progress = retry
            .resume_backend_ingress()
            .expect("each retry successor must reduce");
    }
    assert_eq!(progress, BackendIngressProgress::Complete);
    complete_host_frame_with_explicit_surface_roster(&engine, &mut retry);
    let transition = retry
        .finish(&mut engine)
        .expect("the whole retry must commit");
    assert_eq!(transition.reduced_inputs().len(), 3);
    assert_eq!(engine.backend_ingress_committed_through().get(), 3);
}

#[test]
fn sealed_prefix_leaves_unprocessed_tail_rollbackable_after_publication() {
    let (mut engine, host, mut recorder) = begin_prefix_fixture();
    record_validations(&mut recorder, 1);
    let tail_savepoint = recorder.savepoint();
    record_validations(&mut recorder, 1);
    let mut frame = begin_test_host_frame(&engine, host);
    frame
        .submit_backend_ingress(recorder.pending_batch().expect("both records must freeze"))
        .expect("only the first record must reduce");
    frame
        .seal_backend_ingress_prefix()
        .expect("the first record must seal");
    complete_host_frame_with_explicit_surface_roster(&engine, &mut frame);
    frame.finish(&mut engine).expect("the prefix must publish");
    assert_eq!(engine.backend_ingress_committed_through().get(), 1);
    recorder
        .rollback_to(tail_savepoint)
        .expect("prefix publication must not mark the selected tail committed");
    assert_eq!(recorder.recorded_through().get(), 1);
}

#[test]
fn sealed_prefix_guard_still_rejects_revocation_of_a_processed_record() {
    let (mut engine, host, mut recorder) = begin_prefix_fixture();
    let savepoint = recorder.savepoint();
    record_validations(&mut recorder, 2);
    let mut frame = begin_test_host_frame(&engine, host);
    assert!(matches!(
        frame
            .submit_backend_ingress(recorder.pending_batch().expect("the batch must freeze"))
            .expect("the first record must reduce"),
        BackendIngressProgress::RecordBoundary { .. },
    ));
    frame
        .seal_backend_ingress_prefix()
        .expect("the first record must seal privately");
    recorder
        .rollback_to(savepoint)
        .expect("private candidate state cannot commit producer records");
    record_validations(&mut recorder, 1);
    complete_host_frame_with_explicit_surface_roster(&engine, &mut frame);
    assert!(matches!(
        frame.finish(&mut engine),
        Err(EngineError::BackendIngress {
            source: BackendIngressError::RecordRevoked { .. },
        }),
    ));
    assert_eq!(engine.backend_ingress_committed_through().get(), 0);

    let mut retry = begin_test_host_frame(&engine, host);
    assert_eq!(
        retry
            .submit_backend_ingress(
                recorder
                    .pending_batch()
                    .expect("the new branch must freeze")
            )
            .expect("the replacement branch must reduce"),
        BackendIngressProgress::Complete,
    );
    complete_host_frame_with_explicit_surface_roster(&engine, &mut retry);
    retry
        .finish(&mut engine)
        .expect("publication failure must leave the replacement branch retryable");
    assert_eq!(engine.backend_ingress_committed_through().get(), 1);
}

#[test]
fn pending_pointer_challenge_cannot_seal_or_start_its_successor() {
    let (mut engine, host, mut recorder) = begin_prefix_fixture();
    record_empty_backend_checkpoint(&mut recorder);
    record_validations(&mut recorder, 1);
    let frozen = recorder
        .pending_batch()
        .expect("the pointer and suffix must freeze");
    for seal in [true, false] {
        let mut failed = begin_test_host_frame(&engine, host);
        assert_eq!(
            failed
                .submit_backend_ingress(frozen.clone())
                .expect("the pointer must freeze its receiver challenge"),
            BackendIngressProgress::ReceiverReceiptsRequired,
        );
        let error = if seal {
            failed.seal_backend_ingress_prefix().unwrap_err()
        } else {
            failed.resume_backend_ingress().unwrap_err()
        };
        assert_eq!(error, CoreHostFrameError::BackendIngressBoundaryRequired);
        assert!(failed.finish(&mut engine).is_err());
        assert_eq!(engine.backend_ingress_committed_through().get(), 0);
    }

    let mut retry = begin_test_host_frame(&engine, host);
    assert_eq!(
        retry
            .submit_backend_ingress(frozen)
            .expect("the pending pointer record must retry"),
        BackendIngressProgress::ReceiverReceiptsRequired,
    );
    assert_eq!(
        retry
            .submit_backend_pointer_receiver_receipts(
                PointerReceiverReceiptBatch::new([]).expect("the receipt set must be exact"),
            )
            .expect("receipts must complete only this record"),
        BackendIngressProgress::RecordBoundary {
            completed_records: 1,
            can_seal: true,
        },
    );
    let through = retry
        .seal_backend_ingress_prefix()
        .expect("the now-complete pointer record must seal");
    assert_eq!(through.get(), 1);
    complete_host_frame_with_explicit_surface_roster(&engine, &mut retry);
    retry
        .finish(&mut engine)
        .expect("the pointer prefix must commit");
    assert_eq!(engine.backend_ingress_committed_through(), through);
    assert_eq!(
        recorder
            .batch_after(through)
            .expect("the successor must remain pending")
            .len(),
        1,
    );
}

#[test]
fn atomic_group_can_pause_computation_but_only_its_last_record_can_seal() {
    let (mut engine, host, mut recorder) = begin_prefix_fixture();
    record_validations(&mut recorder, 1);
    let atomic_start = recorder.savepoint();
    record_validations(&mut recorder, 2);
    recorder
        .finish_atomic_append(atomic_start)
        .expect("the unexposed append group must finalize");
    record_validations(&mut recorder, 1);
    let frozen = recorder
        .pending_batch()
        .expect("the complete groups must freeze");

    let mut failed = begin_test_host_frame(&engine, host);
    assert_eq!(
        failed
            .submit_backend_ingress(frozen.clone())
            .expect("the independent predecessor must reduce"),
        BackendIngressProgress::RecordBoundary {
            completed_records: 1,
            can_seal: true,
        },
    );
    assert_eq!(
        failed
            .resume_backend_ingress()
            .expect("the first atomic-group record can reduce cooperatively"),
        BackendIngressProgress::RecordBoundary {
            completed_records: 2,
            can_seal: false,
        },
    );
    assert!(matches!(
        failed.seal_backend_ingress_prefix(),
        Err(CoreHostFrameError::BackendIngressRejected {
            source: BackendIngressError::AtomicGroupIncomplete { through },
        }) if through.get() == 2,
    ));
    assert!(failed.finish(&mut engine).is_err());
    assert_eq!(engine.backend_ingress_committed_through().get(), 0);

    let mut retry = begin_test_host_frame(&engine, host);
    retry
        .submit_backend_ingress(frozen)
        .expect("the exact batch must retry");
    retry
        .resume_backend_ingress()
        .expect("the atomic group's first record must retry");
    assert_eq!(
        retry
            .resume_backend_ingress()
            .expect("the atomic group's final record must finish the group"),
        BackendIngressProgress::RecordBoundary {
            completed_records: 3,
            can_seal: true,
        },
    );
    assert_eq!(
        retry
            .seal_backend_ingress_prefix()
            .expect("the complete atomic group must seal")
            .get(),
        3,
    );
    complete_host_frame_with_explicit_surface_roster(&engine, &mut retry);
    let transition = retry
        .finish(&mut engine)
        .expect("both records in the group must publish together");
    assert_eq!(transition.reduced_inputs().len(), 3);
    assert_eq!(engine.backend_ingress_committed_through().get(), 3);
    assert_eq!(
        recorder
            .batch_after(engine.backend_ingress_committed_through())
            .expect("the later independent record must remain pending")
            .len(),
        1,
    );
}

#[test]
fn atomic_group_larger_than_256_records_retains_private_candidate_across_yield() {
    let (mut engine, host, mut recorder) = begin_prefix_fixture();
    let atomic_start = recorder.savepoint();
    record_validations(&mut recorder, 257);
    recorder
        .finish_atomic_append(atomic_start)
        .expect("the large unexposed append group must finalize");
    let mut frame = begin_test_host_frame(&engine, host);
    frame
        .submit_backend_ingress(recorder.pending_batch().expect("the group must freeze"))
        .expect("the group must start cooperatively");
    for _ in 1..256 {
        assert!(matches!(
            frame
                .resume_backend_ingress()
                .expect("each record must reduce separately"),
            BackendIngressProgress::RecordBoundary {
                can_seal: false,
                ..
            },
        ));
    }
    assert_eq!(frame.candidate.backend_ingress.committed_through().get(), 0);
    assert_eq!(engine.backend_ingress_committed_through().get(), 0);
    assert!(frame.backend_ingress_commit_guard.is_none());

    // The same private candidate can cross scheduling turns without publishing
    // a partial terminal roster or rebuilding already reduced records.
    assert_eq!(
        frame
            .resume_backend_ingress()
            .expect("the last record must finish the indivisible group"),
        BackendIngressProgress::Complete,
    );
    assert_eq!(engine.backend_ingress_committed_through().get(), 0);
    complete_host_frame_with_explicit_surface_roster(&engine, &mut frame);
    let transition = frame
        .finish(&mut engine)
        .expect("the entire group must publish exactly once");
    assert_eq!(transition.reduced_inputs().len(), 257);
    assert_eq!(engine.backend_ingress_committed_through().get(), 257);
}

#[test]
fn complete_and_empty_ingress_have_no_resumable_or_sealable_cursor() {
    let (mut engine, host, mut recorder) = begin_prefix_fixture();
    for empty in [true, false] {
        if !empty {
            record_validations(&mut recorder, 1);
        }
        for seal in [true, false] {
            let mut frame = begin_test_host_frame(&engine, host);
            assert_eq!(
                frame
                    .submit_backend_ingress(
                        recorder.pending_batch().expect("the batch must freeze")
                    )
                    .expect("an empty or single-record batch must finish"),
                BackendIngressProgress::Complete,
            );
            let error = if seal {
                frame.seal_backend_ingress_prefix().unwrap_err()
            } else {
                frame.resume_backend_ingress().unwrap_err()
            };
            assert_eq!(error, CoreHostFrameError::BackendIngressBoundaryRequired);
            assert!(frame.finish(&mut engine).is_err());
            assert_eq!(engine.backend_ingress_committed_through().get(), 0);
        }
    }
}

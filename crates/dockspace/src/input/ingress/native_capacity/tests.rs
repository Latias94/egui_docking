use super::*;
use crate::input::ingress::tests::authority_and_recorder;
use crate::input::intent::{Authority, AuthorityUnavailableReason, PointerId};
use crate::input::journal::{
    DesktopRouteFact, PointerCaptureOwner, PointerEdge, PointerEdgeKind, PointerEdgeLocation,
};

fn bounded_recorder(
    slots: usize,
    data_budget: usize,
) -> (BackendIngressAuthority, BackendIngressRecorder) {
    let (authority, mut recorder) = authority_and_recorder(1);
    let slots = NonZeroUsize::new(slots).unwrap();
    recorder
        .configure_native_transport_capacity(slots, usize::MAX)
        .unwrap();
    let fixed = recorder.native_transport_reserved_bytes().unwrap();
    recorder
        .configure_native_transport_capacity(slots, fixed + data_budget)
        .unwrap();
    (authority, recorder)
}

#[test]
fn fresh_transport_preflight_matches_the_actual_fixed_and_payload_reservation() {
    let slots = NonZeroUsize::new(3).unwrap();
    let (_, mut recorder) = authority_and_recorder(1);
    recorder
        .configure_native_transport_capacity(slots, usize::MAX)
        .unwrap();
    let journal = pointer_segment(0);
    let descendants = journal.owned_contents_capacity_bytes();
    recorder.record_pointer_segment(journal).unwrap();
    let required = recorder.native_transport_reserved_bytes().unwrap();
    assert!(
        BackendIngressRecorder::preflight_native_transport_configuration(
            slots,
            required,
            1,
            descendants
        )
        .is_ok()
    );
    assert!(matches!(
        BackendIngressRecorder::preflight_native_transport_configuration(
            slots,
            required - 1,
            1,
            descendants
        ),
        Err(BackendIngressError::NativeTransportCapacityExceeded { .. })
    ));
    assert!(matches!(
        BackendIngressRecorder::preflight_native_transport_configuration(slots, usize::MAX, 4, 0),
        Err(BackendIngressError::NativeTransportCapacityExceeded {
            limit: 3,
            required: 4
        })
    ));
}

fn pointer_segment(previous: u64) -> PointerEdgeJournal {
    let sequence = PointerEdgeSequence::new(previous + 1);
    let reason = AuthorityUnavailableReason::NotReported;
    PointerEdgeJournal::new(
        PointerEdgeSequence::new(previous),
        sequence,
        vec![PointerEdge::new(
            sequence,
            PointerId::new(1),
            PointerEdgeKind::Moved,
            PointerEdgeLocation::Desktop {
                route: DesktopRouteFact::unknown(Authority::Unknown(reason), reason),
            },
            Authority::Known(PointerCaptureOwner::None),
        )],
    )
    .unwrap()
}

#[test]
fn native_pointer_preparation_credits_only_proven_retirement_and_keeps_the_staging_peak() {
    let descendants = PointerEdgeJournal::single_edge_owned_contents_capacity_bytes();
    let data = payload_data_bytes(
        &BackendIngressPayload::PointerSegment(pointer_segment(0)),
        usize::MAX,
    )
    .unwrap();
    let (mut authority, mut recorder) = bounded_recorder(2, 2 * data + descendants - 1);
    let fixed = recorder.native_transport_reserved_bytes().unwrap();
    recorder.record_pointer_segment(pointer_segment(0)).unwrap();
    recorder.record_pointer_segment(pointer_segment(1)).unwrap();
    let batch = recorder.pending_batch().unwrap();
    authority.commit_batch(&batch).unwrap();
    batch
        .commit_guard()
        .publish_with(|| Ok::<(), ()>(()))
        .unwrap()
        .unwrap();
    drop(batch);
    let committed = authority.commit_watermark().unwrap();
    assert_eq!(recorder.native_pointer_preparation_limit(None), Ok(Some(0)));
    assert_eq!(
        recorder.native_pointer_preparation_limit(Some(committed)),
        Ok(Some(0)),
        "a committed payload still occupies data capacity during caller iteration"
    );
    assert_eq!(recorder.retired_through, BackendIngressOrdinal::ORIGIN);
    assert_eq!(recorder.retained_records().len(), 2);

    recorder
        .configure_native_transport_capacity(
            NonZeroUsize::new(2).unwrap(),
            fixed + 2 * data + 2 * descendants,
        )
        .unwrap();
    assert_eq!(
        recorder.native_pointer_preparation_limit(None),
        Ok(Some(0)),
        "a suspended replay must not receive retirement credit"
    );
    assert_eq!(
        recorder.native_pointer_preparation_limit(Some(committed)),
        Ok(Some(2))
    );
    let forged = BackendIngressCommitWatermark {
        record_identity: BackendIngressRecordIdentity::ORIGIN,
        ..committed
    };
    assert!(matches!(
        recorder.native_pointer_preparation_limit(Some(forged)),
        Err(BackendIngressError::CommitWatermarkContentMismatch { .. })
    ));
    assert_eq!(recorder.retained_records().len(), 2);
    assert_eq!(recorder.retired_through, BackendIngressOrdinal::ORIGIN);
}

#[test]
fn native_capacity_rejects_full_bytes_or_slots_before_minting_or_growing() {
    let fixed_data =
        payload_data_bytes(&BackendIngressPayload::NativeInputDeviceRemoval, usize::MAX).unwrap();
    let pointer_data = payload_data_bytes(
        &BackendIngressPayload::PointerSegment(pointer_segment(0)),
        usize::MAX,
    )
    .unwrap();
    for (slots, data_budget) in [(2, fixed_data), (1, fixed_data + pointer_data)] {
        let (_, mut recorder) = bounded_recorder(slots, data_budget);
        recorder.record_native_input_device_removal().unwrap();
        let before = recorder.pending_batch().unwrap();
        let identity = recorder.last_record_identity;
        let backing = recorder.records.as_ptr();
        let buffer_bytes = recorder.record_buffer_capacity_bytes();
        let reserved = recorder.native_transport_reserved_bytes();
        let descendants = pointer_segment(0).owned_contents_capacity_bytes();
        let preflight = recorder
            .preflight_native_transport_append(1, descendants)
            .unwrap_err();
        let error = recorder
            .record_pointer_segment(pointer_segment(0))
            .unwrap_err();
        assert_eq!(preflight, error);
        if slots == 1 {
            assert_eq!(
                error,
                BackendIngressError::NativeTransportCapacityExceeded {
                    limit: 1,
                    required: 2,
                }
            );
        } else {
            let capacity = recorder.native_transport_capacity.as_ref().unwrap();
            assert_eq!(
                error,
                BackendIngressError::NativeTransportCapacityExceeded {
                    limit: capacity.byte_limit,
                    required: reserved.unwrap() + pointer_data,
                }
            );
        }
        assert_eq!(recorder.pending_batch().unwrap(), before);
        assert_eq!(recorder.last_record_identity, identity);
        assert_eq!(recorder.pointer_through(), PointerEdgeSequence::new(0));
        assert_eq!(recorder.records.as_ptr(), backing);
        assert_eq!(recorder.record_buffer_capacity_bytes(), buffer_bytes);
        assert_eq!(recorder.native_transport_reserved_bytes(), reserved);
    }
}

#[test]
fn native_capacity_configuration_checks_existing_payloads_and_preserves_failure_state() {
    let (_, mut generic) = authority_and_recorder(1);
    generic
        .record_semantic_input(EngineInput::ValidateWorkspace)
        .unwrap();
    let before = generic.pending_batch().unwrap();
    let backing = generic.records.as_ptr();
    assert_eq!(
        generic.configure_native_transport_capacity(NonZeroUsize::new(8).unwrap(), usize::MAX),
        Err(BackendIngressError::UnsupportedNativePayloadCapacity),
    );
    assert_eq!(generic.pending_batch().unwrap(), before);
    assert_eq!(generic.records.as_ptr(), backing);
    assert_eq!(generic.native_transport_reserved_bytes(), None);
    generic
        .record_semantic_input(EngineInput::ValidateWorkspace)
        .unwrap();

    let (_, mut native) = authority_and_recorder(1);
    native.record_native_input_device_removal().unwrap();
    let data = retained_data_bytes(&native.records, usize::MAX).unwrap();
    native
        .configure_native_transport_capacity(NonZeroUsize::new(8).unwrap(), usize::MAX)
        .unwrap();
    let reserved = native.native_transport_reserved_bytes().unwrap();
    let backing = native.records.as_ptr();
    let fixed = native
        .native_transport_capacity
        .as_ref()
        .unwrap()
        .fixed_reserved_bytes;
    assert_eq!(reserved, fixed + data);
    assert!(matches!(
        native.configure_native_transport_capacity(NonZeroUsize::new(8).unwrap(), reserved - 1),
        Err(BackendIngressError::NativeTransportCapacityExceeded { .. }),
    ));
    assert_eq!(native.native_transport_reserved_bytes(), Some(reserved));
    assert_eq!(native.records.as_ptr(), backing);
    let before = native.recorded_through();
    assert_eq!(
        native.record_semantic_input(EngineInput::ValidateWorkspace),
        Err(BackendIngressError::UnsupportedNativePayloadCapacity),
    );
    assert_eq!(native.recorded_through(), before);
}

#[test]
fn native_capacity_rollback_returns_payload_charge_but_retains_backing_and_fixed_reserve() {
    let payload = BackendIngressPayload::PointerSegment(pointer_segment(0));
    let data = payload_data_bytes(&payload, usize::MAX).unwrap();
    let (_, mut recorder) = bounded_recorder(2, data);
    let fixed = recorder.native_transport_reserved_bytes().unwrap();
    let backing = recorder.records.as_ptr();
    let buffer_bytes = recorder.record_buffer_capacity_bytes();
    let savepoint = recorder.savepoint();
    recorder.record_pointer_segment(pointer_segment(0)).unwrap();
    let owner = Arc::downgrade(&recorder.records[0].payload);
    assert_eq!(
        recorder.native_transport_reserved_bytes(),
        Some(fixed + data)
    );
    assert!(recorder.record_pointer_segment(pointer_segment(1)).is_err());
    recorder.rollback_to(savepoint).unwrap();
    assert!(owner.upgrade().is_none());
    assert_eq!(recorder.native_transport_reserved_bytes(), Some(fixed));
    assert_eq!(recorder.recorded_through(), BackendIngressOrdinal::ORIGIN);
    assert_eq!(recorder.pointer_through(), PointerEdgeSequence::new(0));
    assert_eq!(recorder.records.as_ptr(), backing);
    assert_eq!(recorder.record_buffer_capacity_bytes(), buffer_bytes);
    recorder.record_pointer_segment(pointer_segment(0)).unwrap();
    assert_eq!(
        recorder.native_transport_reserved_bytes(),
        Some(fixed + data)
    );
}

#[test]
fn native_capacity_retirement_releases_data_and_reserves_exact_receipt_capacity() {
    let fixed_data =
        payload_data_bytes(&BackendIngressPayload::NativeInputDeviceRemoval, usize::MAX).unwrap();
    let (mut authority, mut recorder) = bounded_recorder(2, 2 * fixed_data);
    let fixed = recorder.native_transport_reserved_bytes().unwrap();
    let backing_bytes = recorder.record_buffer_capacity_bytes();
    let binding = ViewportBinding::new(
        recorder.lease().authority_domain(),
        WorkspaceEpoch::new(0),
        SurfaceId::new(1),
        WindowToken::new(1),
        crate::platform::viewport::WindowIncarnation::new(1),
    );
    recorder
        .record_platform_binding_quiescence(binding)
        .unwrap();
    recorder.record_native_input_device_removal().unwrap();
    let owner = Arc::downgrade(&recorder.records[0].payload);
    let batch = recorder.pending_batch().unwrap();
    authority.commit_batch(&batch).unwrap();
    batch
        .commit_guard()
        .publish_with(|| Ok::<(), ()>(()))
        .unwrap()
        .unwrap();
    drop(batch);
    let receipt = recorder
        .retire_committed_prefix(authority.commit_watermark().unwrap())
        .unwrap()
        .unwrap();
    assert!(owner.upgrade().is_none());
    assert_eq!(receipt.binding_quiescences(), [binding]);
    assert_eq!(receipt.owned_capacity_bytes(), size_of::<ViewportBinding>());
    assert_eq!(recorder.native_transport_reserved_bytes(), Some(fixed));
    assert_eq!(recorder.record_buffer_capacity_bytes(), backing_bytes);
    recorder.record_native_input_device_removal().unwrap();
    assert_eq!(
        recorder.native_transport_reserved_bytes(),
        Some(fixed + fixed_data)
    );
}

#[test]
fn native_capacity_replay_reserves_full_atomic_group_and_shares_payload_data() {
    let count = 257;
    let data =
        payload_data_bytes(&BackendIngressPayload::NativeInputDeviceRemoval, usize::MAX).unwrap();
    let (_, mut recorder) = bounded_recorder(count, count * data);
    let group = recorder.savepoint();
    for _ in 0..count {
        recorder.record_native_input_device_removal().unwrap();
    }
    recorder.finish_atomic_append(group).unwrap();
    let reserved = recorder.native_transport_reserved_bytes();
    let batch = recorder
        .replay_prefix_after(
            BackendIngressOrdinal::ORIGIN,
            NonZeroUsize::new(256).unwrap(),
        )
        .unwrap();
    assert_eq!(batch.len(), count);
    assert_eq!(
        batch.record_buffer_capacity_bytes(),
        count * size_of::<BackendIngressRecord>()
    );
    let guard = batch.commit_guard();
    assert_eq!(
        guard.owned_capacity_bytes(),
        count * size_of::<(BackendIngressOrdinal, BackendIngressRecordLiveness)>(),
    );
    for (recorded, frozen) in recorder.records.iter().zip(batch.records()) {
        assert!(Arc::ptr_eq(&recorded.payload, &frozen.payload));
        assert!(Arc::ptr_eq(&recorded.liveness.0, &frozen.liveness.0));
    }
    let capacity = recorder.native_transport_capacity.as_ref().unwrap();
    assert_eq!(
        capacity.fixed_reserved_bytes,
        size_of::<BackendIngressRecorder>()
            + size_of::<Mutex<()>>()
            + recorder.record_buffer_capacity_bytes()
            + CoreHostFrame::backend_ingress_inline_capacity_bytes()
            + size_of::<Option<BackendIngressPrefixRetirementReceipt>>()
            + batch.record_buffer_capacity_bytes()
            + guard.owned_capacity_bytes()
            + count * size_of::<ViewportBinding>()
            + count * size_of::<PointerEdgeJournal>(),
    );
    assert_eq!(recorder.native_transport_reserved_bytes(), reserved);
}

#[test]
fn native_capacity_precludes_an_oversized_atomic_append_and_rollback_reuses_slots() {
    let count = 257;
    let data =
        payload_data_bytes(&BackendIngressPayload::NativeInputDeviceRemoval, usize::MAX).unwrap();
    let (_, mut recorder) = bounded_recorder(count, (count + 1) * data);
    let fixed = recorder.native_transport_reserved_bytes();
    let buffer_bytes = recorder.record_buffer_capacity_bytes();
    let identity = recorder.last_record_identity;
    assert_eq!(
        recorder.preflight_native_transport_append(count + 1, 0),
        Err(BackendIngressError::NativeTransportCapacityExceeded {
            limit: count,
            required: count + 1,
        }),
    );
    assert_eq!(recorder.last_record_identity, identity);
    assert_eq!(recorder.recorded_through(), BackendIngressOrdinal::ORIGIN);
    assert_eq!(recorder.native_transport_reserved_bytes(), fixed);
    recorder
        .preflight_native_transport_append(count, 0)
        .unwrap();
    let group = recorder.savepoint();
    for _ in 0..count {
        recorder.record_native_input_device_removal().unwrap();
    }
    let identity = recorder.last_record_identity;
    assert_eq!(
        recorder.record_native_input_device_removal(),
        Err(BackendIngressError::NativeTransportCapacityExceeded {
            limit: count,
            required: count + 1,
        }),
    );
    assert_eq!(recorder.last_record_identity, identity);
    recorder.rollback_to(group).unwrap();
    assert_eq!(recorder.recorded_through(), BackendIngressOrdinal::ORIGIN);
    assert_eq!(recorder.native_transport_reserved_bytes(), fixed);
    assert_eq!(recorder.record_buffer_capacity_bytes(), buffer_bytes);
    recorder.record_native_input_device_removal().unwrap();
}

#[test]
fn native_capacity_semantic_box_is_counted_and_unknown_semantics_remain_unbounded_only() {
    let (_, mut recorder) = authority_and_recorder(1);
    recorder
        .record_viewport_registration(
            WorkspaceVersion::new(
                WorkspaceEpoch::new(0),
                crate::ids::WorkspaceRevision::new(0),
            ),
            SurfaceId::new(1),
            WindowToken::new(1),
            ViewportRole::Root,
            None,
        )
        .unwrap();
    let payload = recorder.records[0].payload();
    assert_eq!(
        payload.native_owned_descendant_capacity_bytes(),
        Ok(size_of::<EngineInput>())
    );
    let expected =
        size_of::<BackendIngressPayload>() + size_of::<AtomicU8>() + size_of::<EngineInput>();
    recorder
        .configure_native_transport_capacity(NonZeroUsize::MIN, usize::MAX)
        .unwrap();
    assert_eq!(
        recorder
            .native_transport_capacity
            .as_ref()
            .unwrap()
            .retained_data_bytes,
        expected
    );
    assert_eq!(
        BackendIngressPayload::SemanticInput(Box::new(EngineInput::ValidateWorkspace))
            .native_owned_descendant_capacity_bytes(),
        Err(BackendIngressError::UnsupportedNativePayloadCapacity),
    );
}

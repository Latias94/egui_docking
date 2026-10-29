//! Regression coverage for prepaid native-effect terminal storage.

use super::*;
use crate::effect::EffectLedger;
use crate::ids::{EngineAuthorityDomainId, SurfaceId};
use crate::platform_provider::PlatformObservationAuthority;
use crate::viewport::{InventoryGeneration, WindowIncarnation, WindowToken};

fn emissions(count: usize) -> (PlatformObservationLease, Vec<PlatformEffectEmission>) {
    let domain = EngineAuthorityDomainId::new_for_test(198);
    let mut providers = PlatformObservationAuthority::new(domain);
    let provider = providers.create().expect("the test provider mints");
    let mut ledger = EffectLedger::default();
    for index in 0..count {
        let id = u64::try_from(index + 1).expect("test identities fit");
        let binding = ViewportBinding::new(
            domain,
            WorkspaceEpoch::new(0),
            SurfaceId::new(id),
            WindowToken::new(id),
            WindowIncarnation::new(1),
        );
        ledger
            .request(PlatformEffect::SetPointerPassthrough {
                binding,
                enabled: true,
                after: None,
            })
            .expect("the property effect allocates");
    }
    let emissions = ledger
        .take_new_requests(provider, InventoryGeneration::new(1), |_| false)
        .expect("all property effects emit");
    assert_eq!(emissions.len(), count);
    (provider, emissions)
}

#[test]
fn dropping_unpublished_preparation_only_releases_its_reservations() {
    let (provider, emissions) = emissions(2);
    let queue = NativeEffectDropQueue::with_capacity_limit(2).expect("two slots preallocate");
    let capacity = queue.owned_data_capacity_bytes();
    let prepared = queue.prepare_emissions(&emissions).expect("the batch fits");
    assert!(!queue.is_quiescent());
    assert!(!queue.has_pending());
    assert_eq!(queue.lock().outstanding, 2);
    assert!(matches!(
        queue.prepare_emissions(&emissions[..1]),
        Err(NativePlatformError::InputCapacity)
    ));

    drop(prepared);

    assert!(queue.is_quiescent());
    assert_eq!(queue.owned_data_capacity_bytes(), capacity);
    queue
        .record_for(provider, |_| {
            panic!("unpublished effects have no terminals")
        })
        .expect("the queue is empty");
    drop(
        queue
            .prepare_emissions(&emissions)
            .expect("all slots return"),
    );
    assert!(queue.is_quiescent());
}

#[test]
fn finite_admission_rejects_the_complete_batch_without_partial_reservations() {
    let (_, emissions) = emissions(2);
    let queue = NativeEffectDropQueue::with_capacity_limit(1).expect("one slot preallocates");
    let capacity = queue.owned_data_capacity_bytes();
    assert!(matches!(
        queue.prepare_emissions(&emissions),
        Err(NativePlatformError::InputCapacity)
    ));
    assert!(queue.is_quiescent());
    assert_eq!(queue.owned_data_capacity_bytes(), capacity);
    drop(queue.prepare_emissions(&emissions[..1]).expect("one fits"));
    assert!(queue.is_quiescent());
}

#[test]
fn dropping_every_published_request_keeps_the_prepaid_backing() {
    let (provider, emissions) = emissions(8);
    for queue in [
        NativeEffectDropQueue::with_capacity_limit(8).expect("eight slots preallocate"),
        NativeEffectDropQueue::default(),
    ] {
        let prepared = queue.prepare_emissions(&emissions).expect("the batch fits");
        let capacity = queue.owned_data_capacity_bytes();
        for (index, request) in prepared.commit().into_iter().enumerate() {
            drop(request);
            assert_eq!(queue.owned_data_capacity_bytes(), capacity);
            let state = queue.lock();
            assert_eq!(state.outstanding, emissions.len());
            assert_eq!(state.pending.len(), index + 1);
            assert!(state.pending.iter().all(|result| result.storage.is_none()));
        }
        let mut recorded = 0;
        queue
            .record_for(provider, |result| {
                assert_eq!(result.result.effect(), emissions[recorded].id());
                recorded += 1;
                Ok(())
            })
            .expect("all terminals record");
        assert_eq!(recorded, emissions.len());
        assert!(queue.is_quiescent());
        assert_eq!(queue.owned_data_capacity_bytes(), capacity);
    }
}

#[test]
fn partial_drain_and_repeated_capacity_failures_retain_the_front_and_backing() {
    let (provider, emissions) = emissions(3);
    let queue = NativeEffectDropQueue::with_capacity_limit(3).expect("three slots preallocate");
    drop(
        queue
            .prepare_emissions(&emissions)
            .expect("the batch fits")
            .commit(),
    );
    let capacity = queue.owned_data_capacity_bytes();
    let failing_front = {
        let state = queue.lock();
        state.pending.get(1).expect("the second terminal exists") as *const NativeEffectResult
    };
    let mut attempts = 0;
    assert_eq!(
        queue.record_for(provider, |_| {
            attempts += 1;
            if attempts == 2 {
                Err(NativePlatformError::InputCapacity)
            } else {
                Ok(())
            }
        }),
        Err(NativePlatformError::InputCapacity)
    );
    assert_eq!(queue.lock().outstanding, 2);
    for _ in 0..2 {
        assert_eq!(
            queue.record_for(provider, |result| {
                assert_eq!(result as *const NativeEffectResult, failing_front);
                assert_eq!(result.result.effect(), emissions[1].id());
                Err(NativePlatformError::InputCapacity)
            }),
            Err(NativePlatformError::InputCapacity)
        );
        assert_eq!(queue.owned_data_capacity_bytes(), capacity);
        assert_eq!(queue.lock().outstanding, 2);
    }
    assert!(matches!(
        queue.prepare_emissions(&emissions[..2]),
        Err(NativePlatformError::InputCapacity)
    ));
    drop(
        queue
            .prepare_emissions(&emissions[..1])
            .expect("the recorded slot is free"),
    );
    let mut recorded = 1;
    queue
        .record_for(provider, |result| {
            assert_eq!(result.result.effect(), emissions[recorded].id());
            recorded += 1;
            Ok(())
        })
        .expect("retry consumes the retained suffix");
    assert_eq!(recorded, 3);
    assert!(queue.is_quiescent());
    assert_eq!(queue.owned_data_capacity_bytes(), capacity);
}

#[test]
fn negative_results_keep_the_slot_across_submission_failure_and_retry() {
    let (_, emissions) = emissions(1);
    let classifications: [fn(NativeEffectRequest) -> NativeEffectResult; 3] = [
        |request| request.dispatch_failed(NativeDispatchFailure::WindowUnavailable),
        |request| request.unsupported(NativeUnsupportedReason::BackendUnsupported),
        |request| request.indeterminate(NativeIndeterminateReason::AcknowledgementLost),
    ];
    for classify in classifications {
        let queue = NativeEffectDropQueue::with_capacity_limit(1).expect("one slot preallocates");
        let request = queue
            .prepare_emissions(&emissions)
            .expect("one fits")
            .commit()
            .pop()
            .expect("one request exists");
        let result = classify(request);
        assert_eq!(queue.lock().outstanding, 1);
        assert!(!queue.has_pending());
        let failed = NativeEffectSubmissionError::new(NativePlatformError::InputCapacity, result);
        assert!(matches!(
            queue.prepare_emissions(&emissions),
            Err(NativePlatformError::InputCapacity)
        ));
        let (kind, retry) = failed.into_parts();
        assert_eq!(kind, NativeHostErrorKind::InputCapacity);
        assert_eq!(queue.lock().outstanding, 1);
        assert!(matches!(
            queue.prepare_emissions(&emissions),
            Err(NativePlatformError::InputCapacity)
        ));

        drop(retry);

        assert!(queue.is_quiescent());
        assert!(!queue.has_pending(), "result Drop only returns its lease");
        drop(
            queue
                .prepare_emissions(&emissions)
                .expect("the slot returns"),
        );
    }
}

#[test]
fn accepted_dispatch_releases_the_failure_slot_and_preserves_copy_acknowledgement() {
    let (_, emissions) = emissions(1);
    let queue = NativeEffectDropQueue::with_capacity_limit(1).expect("one slot preallocates");
    let request = queue
        .prepare_emissions(&emissions)
        .expect("one fits")
        .commit()
        .pop()
        .expect("one request exists");
    let Some(NativeEffectAcknowledgement::Input(acknowledgement)) = request.accepted() else {
        panic!("the input effect returns its acknowledgement");
    };
    let copied = acknowledgement;
    assert_eq!(copied, acknowledgement);
    assert!(queue.is_quiescent());
    drop(
        queue
            .prepare_emissions(&emissions)
            .expect("accepted frees the slot"),
    );
    assert!(!queue.has_pending());
}

#[test]
fn queued_terminals_do_not_keep_their_own_store_alive() {
    let (_, emissions) = emissions(1);
    let queue = NativeEffectDropQueue::with_capacity_limit(1).expect("one slot preallocates");
    let weak = Arc::downgrade(&queue.0);
    drop(
        queue
            .prepare_emissions(&emissions)
            .expect("one fits")
            .commit(),
    );
    assert!(queue.has_pending());

    drop(queue);

    assert!(weak.upgrade().is_none());
}

#[test]
fn reservation_checks_overflow_and_covers_preallocated_adapter_result_storage() {
    assert!(NativeEffectDropQueue::reservation_bytes(usize::MAX).is_none());
    assert!(matches!(
        NativeEffectDropQueue::with_capacity_limit(usize::MAX),
        Err(NativePlatformError::InputCapacity)
    ));
    for slots in [0, 1, 8] {
        let queue = NativeEffectDropQueue::with_capacity_limit(slots).expect("the store fits");
        assert_eq!(queue.lock().pending.capacity(), slots);
        let mut adapter_results = VecDeque::new();
        adapter_results
            .try_reserve_exact(slots)
            .expect("the adapter store fits");
        let adapter_results: VecDeque<(NativeEffectResult, Option<NativeHostErrorKind>)> =
            adapter_results;
        let resident = queue.owned_data_capacity_bytes()
            + size_of_val(&adapter_results)
            + adapter_results.capacity()
                * size_of::<(NativeEffectResult, Option<NativeHostErrorKind>)>();
        assert!(
            NativeEffectDropQueue::reservation_bytes(slots).expect("the quota fits") >= resident
        );
    }
}

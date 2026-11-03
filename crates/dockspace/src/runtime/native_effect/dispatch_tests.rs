//! Dispatch-start and prepaid uncertain-terminal regression coverage.

use std::panic::{AssertUnwindSafe, catch_unwind};

use super::*;
use crate::effect::{EffectLedger, EffectPhase, EffectTransition};
use crate::ids::{EngineAuthorityDomainId, SurfaceId};
use crate::platform_provider::PlatformObservationAuthority;
use crate::viewport::{InventoryGeneration, WindowIncarnation, WindowToken};

fn emitted_release() -> (EffectLedger, PlatformEffectEmission) {
    let domain = EngineAuthorityDomainId::new_for_test(199);
    let mut providers = PlatformObservationAuthority::new(domain);
    let provider = providers.create().expect("the provider mints");
    let binding = ViewportBinding::new(
        domain,
        WorkspaceEpoch::new(0),
        SurfaceId::new(1),
        WindowToken::new(1),
        WindowIncarnation::new(1),
    );
    let mut ledger = EffectLedger::default();
    ledger
        .request(PlatformEffect::ReleaseChild { binding })
        .expect("the release allocates");
    let emission = ledger
        .take_new_requests(provider, InventoryGeneration::new(1), |_| false)
        .expect("the release emits")
        .pop()
        .expect("one release exists");
    (ledger, emission)
}

fn request(
    queue: &NativeEffectDropQueue,
    emission: &PlatformEffectEmission,
) -> NativeEffectRequest {
    queue
        .prepare_emissions(std::slice::from_ref(emission))
        .expect("the single prepaid request fits")
        .commit()
        .pop()
        .expect("one request exists")
}

#[test]
fn dispatch_start_unwind_preserves_exact_compensation_and_prepaid_retry_storage() {
    let (mut ledger, emission) = emitted_release();
    let queue = NativeEffectDropQueue::with_capacity_limit(1).expect("one slot preallocates");
    let request = request(&queue, &emission);
    let bytes = queue.owned_data_capacity_bytes();

    let panicked = catch_unwind(AssertUnwindSafe(|| {
        let _dispatch = request.begin_dispatch();
        panic!("the backend may have consumed the operation before unwinding");
    }));
    assert!(panicked.is_err());
    assert_eq!(queue.owned_data_capacity_bytes(), bytes);
    assert_eq!(queue.available_slots(), Some(0));
    assert_eq!(queue.lock().pending.len(), 1);
    assert!(matches!(
        queue.prepare_emissions(std::slice::from_ref(&emission)),
        Err(NativePlatformError::InputCapacity)
    ));

    for _ in 0..2 {
        assert_eq!(
            queue.record_for(emission.provider(), |result| {
                assert_eq!(result.binding, emission.effect().binding());
                assert_eq!(result.result.effect(), emission.id());
                assert_eq!(result.result.epoch(), emission.epoch());
                assert!(result.can_be_correlated_by_cleanup());
                assert_eq!(
                    result.result.result(),
                    EffectDispatchResult::Indeterminate(
                        EffectIndeterminateReason::AcknowledgementLost,
                    )
                );
                Err(NativePlatformError::InputCapacity)
            }),
            Err(NativePlatformError::InputCapacity)
        );
        assert_eq!(queue.available_slots(), Some(0));
        assert_eq!(queue.owned_data_capacity_bytes(), bytes);
    }

    queue
        .record_for(emission.provider(), |result| {
            assert_eq!(
                ledger.report(emission.provider(), emission.epoch(), result.result),
                EffectTransition::Applied
            );
            Ok(())
        })
        .expect("the exact uncertain result records once");
    assert!(queue.is_quiescent());
    assert_eq!(queue.owned_data_capacity_bytes(), bytes);
    assert_eq!(
        ledger
            .record(emission.id())
            .expect("the effect remains")
            .phase(),
        EffectPhase::Indeterminate(EffectIndeterminateReason::AcknowledgementLost)
    );
    assert!(
        ledger
            .take_new_requests(emission.provider(), InventoryGeneration::new(2), |_| false)
            .expect("the next extraction succeeds")
            .is_empty(),
        "an uncertain destructive effect must never redispatch"
    );
    assert_eq!(ledger.retention_manifest().unsettled_records(), 1);
}

#[test]
fn deferred_observation_owner_cannot_revert_a_started_request_to_non_dispatch() {
    let (_, emission) = emitted_release();
    for abandon_by_drop in [false, true] {
        let queue = NativeEffectDropQueue::with_capacity_limit(1).unwrap();
        let retained = request(&queue, &emission)
            .begin_dispatch()
            .await_observation();
        let bytes = queue.owned_data_capacity_bytes();
        if abandon_by_drop {
            drop(retained);
            queue
                .record_for(emission.provider(), |result| {
                    assert_eq!(
                        result.result.result(),
                        EffectDispatchResult::Indeterminate(
                            EffectIndeterminateReason::AcknowledgementLost,
                        )
                    );
                    Ok(())
                })
                .unwrap();
        } else {
            let result = retained.dispatch_failed(NativeDispatchFailure::ProviderStopped);
            assert_eq!(
                result.result.result(),
                EffectDispatchResult::Indeterminate(EffectIndeterminateReason::AcknowledgementLost,)
            );
            assert_eq!(queue.available_slots(), Some(0));
            drop(result);
        }
        assert!(queue.is_quiescent());
        assert_eq!(queue.owned_data_capacity_bytes(), bytes);
    }
}

#[test]
fn undispatched_drop_and_definitive_dispatch_results_remain_distinct() {
    let (_, emission) = emitted_release();
    let queue = NativeEffectDropQueue::with_capacity_limit(1).unwrap();
    drop(request(&queue, &emission));
    queue
        .record_for(emission.provider(), |result| {
            assert_eq!(
                result.result.result(),
                EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped)
            );
            Ok(())
        })
        .unwrap();

    let dispatch = request(&queue, &emission).begin_dispatch();
    assert_eq!(dispatch.operation().binding().surface(), SurfaceId::new(1));
    let result = dispatch.not_applied(NativeDispatchFailure::WindowUnavailable);
    assert_eq!(
        result.result.result(),
        EffectDispatchResult::DispatchFailed(DispatchFailureReason::WindowUnavailable)
    );
    assert_eq!(queue.available_slots(), Some(0));
    drop(result);
    assert!(queue.is_quiescent());

    let acknowledgement = request(&queue, &emission).begin_dispatch().accepted();
    assert!(matches!(
        acknowledgement,
        Some(NativeEffectAcknowledgement::Close(_))
    ));
    assert!(queue.is_quiescent());
    assert!(!queue.has_pending());
}

use super::*;
use crate::close::plan::{CloseAuthority, CloseCoordinator, NativeCloseEdge, SurfaceCloseRequest};
use crate::engine::transition::WorkspaceVersion;
use crate::ids::{EngineAuthorityDomainId, SurfaceId, WorkspaceRevision};
use crate::platform::provider::PlatformObservationAuthority;
use crate::platform::viewport::{WindowIncarnation, WindowToken};
use crate::policy::PolicyRevision;

fn binding(epoch: u64, incarnation: u64) -> ViewportBinding {
    ViewportBinding::new(
        EngineAuthorityDomainId::new_for_test(17),
        WorkspaceEpoch::new(epoch),
        SurfaceId::new(2),
        WindowToken::new(3),
        WindowIncarnation::new(incarnation),
    )
}

fn create_effect(binding: ViewportBinding) -> PlatformEffect {
    PlatformEffect::CreateWindow {
        binding,
        placement: PhysicalRect::new(0.0, 0.0, 100.0, 80.0).expect("test placement must be valid"),
        role: ViewportRole::Child,
    }
}

fn test_provider() -> PlatformObservationLease {
    let mut authority =
        PlatformObservationAuthority::new(EngineAuthorityDomainId::new_for_test(17));
    authority.create().expect("test provider must be available")
}

fn test_provider_replacement() -> (PlatformObservationLease, PlatformObservationLease) {
    let mut authority =
        PlatformObservationAuthority::new(EngineAuthorityDomainId::new_for_test(17));
    let first = authority
        .create()
        .expect("first test provider must be available");
    let ticket = authority
        .begin_replacement(first)
        .expect("replacement test provider must be reserved");
    let second = authority
        .finish_replacement(ticket)
        .expect("replacement test provider must be activated");
    (first, second)
}

#[test]
fn published_terminal_effects_compact_to_the_monotonic_identity_frontier() {
    let binding = binding(0, 1);
    let mut ledger = EffectLedger::default();
    let mut last = EffectId::default();

    for _ in 0..10_000 {
        let effect = ledger
            .request(create_effect(binding))
            .expect("effect identity must remain available");
        assert_eq!(
            ledger.invalidate_unemitted(effect, EffectInvalidation::NativeCreateAborted),
            EffectInvalidationTransition::Applied,
        );
        last = effect;
    }

    ledger.mark_boundary_published();
    let retention = ledger.retention_manifest();
    assert_eq!(retention.unsettled_records(), 0);
    assert_eq!(retention.terminal_record_guards(), 10_000);
    assert_eq!(retention.revoked_provider_guards(), 0);
    assert_eq!(retention.retained_structure_count(), 10_000);
    assert_eq!(retention.provider_release_barrier(), None,);

    assert_eq!(
        ledger.compact_published_terminal(&BTreeSet::from([last])),
        9_999,
    );
    assert_eq!(ledger.retention_manifest().terminal_record_guards(), 1);
    assert_eq!(ledger.compact_published_terminal(&BTreeSet::new()), 1,);
    assert_eq!(ledger.retention_manifest().retained_structure_count(), 0);
    assert_eq!(
        ledger.mark_destroyed(last, binding, InventoryGeneration::new(1)),
        EffectTransition::RetiredTerminal,
    );
}

#[test]
fn emitted_lane_tail_does_not_retain_terminal_predecessor_detail() {
    let binding = binding(0, 1);
    let mut ledger = EffectLedger::default();
    let first = ledger
        .request(PlatformEffect::SetPointerPassthrough {
            binding,
            enabled: true,
            after: None,
        })
        .expect("first effect identity must be available");
    let second = ledger
        .request(PlatformEffect::SetPointerPassthrough {
            binding,
            enabled: false,
            after: Some(first),
        })
        .expect("second effect identity must be available");
    assert_eq!(
        take_ordinary_requests(&mut ledger, InventoryGeneration::new(1)).len(),
        2,
    );
    for effect in [first, second] {
        assert_eq!(
            ledger.mark_observed_applied(
                test_provider(),
                effect,
                binding,
                InventoryGeneration::new(2),
            ),
            EffectTransition::Applied,
        );
    }
    ledger.mark_boundary_published();

    assert_eq!(
        ledger.compact_published_terminal(&BTreeSet::from([second])),
        1,
    );
    assert_eq!(ledger.records().count(), 1);
    assert_eq!(ledger.compact_published_terminal(&BTreeSet::new()), 1);
    assert_eq!(ledger.records().count(), 0);
}

#[test]
fn ten_thousand_completed_focus_requests_retain_only_the_live_lane_tail() {
    let provider = test_provider();
    let mut ledger = EffectLedger::default();
    let mut tail = None;

    for index in 0..10_000 {
        let binding = binding((index % 2) + 1, 1);
        let effect = ledger
            .request(PlatformEffect::RequestFocus {
                binding,
                after: tail,
            })
            .expect("focus effect identity must remain available");
        let emissions = ledger
            .take_new_requests(provider, InventoryGeneration::new(index + 1), |_| false)
            .expect("the next focus request must follow the emitted lane tail");
        assert_eq!(emissions.len(), 1);
        assert_eq!(emissions[0].id(), effect);
        assert_eq!(
            ledger.mark_observed_applied(
                provider,
                effect,
                binding,
                InventoryGeneration::new(index + 2),
            ),
            EffectTransition::Applied,
        );
        tail = Some(effect);
    }

    ledger.mark_boundary_published();
    assert_eq!(ledger.retention_manifest().terminal_record_guards(), 10_000);
    assert_eq!(
        ledger.compact_published_terminal(&BTreeSet::from([
            tail.expect("the focus lane has a tail"),
        ])),
        9_999,
    );
    assert_eq!(ledger.retention_manifest().terminal_record_guards(), 1);
}

#[test]
fn semantic_cleanup_subjects_survive_until_their_live_owner_is_terminal() {
    let binding = binding(0, 1);
    let mut ledger = EffectLedger::default();
    let compensated = ledger
        .request(create_effect(binding))
        .expect("compensated effect identity must be available");
    let observed = ledger
        .request(create_effect(binding))
        .expect("observed cleanup identity must be available");
    for effect in [compensated, observed] {
        assert_eq!(
            ledger.invalidate_unemitted(effect, EffectInvalidation::NativeCreateAborted),
            EffectInvalidationTransition::Applied,
        );
    }
    let compensation = ledger
        .request(PlatformEffect::CompensatingClose {
            binding,
            compensates: compensated,
        })
        .expect("compensation identity must be available");
    let continuation = ledger
        .request(PlatformEffect::ContinueCleanup {
            binding,
            predecessor: observed,
            after: None,
        })
        .expect("continuation identity must be available");
    ledger.mark_boundary_published();

    assert_eq!(ledger.compact_published_terminal(&BTreeSet::new()), 0);
    assert_eq!(ledger.records().count(), 4);
    for effect in [compensation, continuation] {
        assert_eq!(
            ledger.invalidate_unemitted(effect, EffectInvalidation::NativeCreateAborted),
            EffectInvalidationTransition::Applied,
        );
    }
    ledger.mark_boundary_published();
    assert_eq!(ledger.compact_published_terminal(&BTreeSet::new()), 4);
    assert_eq!(ledger.records().count(), 0);
}

#[test]
fn retention_accounts_for_revoked_provider_tombstones() {
    let provider = test_provider();
    let mut ledger = EffectLedger::default();
    ledger.revoke_provider_authority(provider);

    let retention = ledger.retention_manifest();
    assert_eq!(retention.unsettled_records(), 0);
    assert_eq!(retention.terminal_record_guards(), 0);
    assert_eq!(retention.revoked_provider_guards(), 1);
    assert_eq!(retention.retained_structure_count(), 1);
    assert_eq!(
        retention.provider_release_barrier(),
        Some(crate::engine::resources::RuntimeRetentionReleaseBarrier::EffectIngressQuiesced),
    );
}

fn take_ordinary_requests(
    ledger: &mut EffectLedger,
    inventory_generation: InventoryGeneration,
) -> Vec<PlatformEffectEmission> {
    ledger
        .take_new_requests(test_provider(), inventory_generation, |_| false)
        .expect("ordinary test effects must have valid causal predecessors")
}

fn native_close_edge(
    binding: ViewportBinding,
    observed_at: u64,
    received_at: u64,
) -> NativeCloseEdge {
    NativeCloseEdge::from_authoritative_requested(
        EngineAuthorityDomainId::new_for_test(17),
        binding,
        CloseObservationGeneration::new(observed_at),
        InventoryGeneration::new(received_at),
    )
}

fn close_request(edge: NativeCloseEdge) -> CloseRequestId {
    let domain = edge.domain();
    let authority = CloseAuthority::new(
        domain,
        WorkspaceVersion::new(edge.binding().epoch(), WorkspaceRevision::new(1)),
        PolicyRevision::new(1),
    );
    let mut coordinator = CloseCoordinator::new(domain);
    coordinator
        .open_surface(authority, edge, SurfaceCloseRequest::RetainLayout, [], ())
        .expect("test close plan must open")
        .request()
}

#[test]
fn native_close_fence_is_frozen_only_when_the_request_is_extracted() {
    let mut ledger = EffectLedger::default();
    let binding = binding(1, 1);
    let edge = native_close_edge(binding, 7, 11);
    let effect = ledger
        .request_native_close(
            WorkspaceEpoch::new(1),
            close_request(edge),
            edge,
            NativeCloseResolution::Accept,
            None,
        )
        .expect("native close effect must allocate");

    let queued = ledger.record(effect).expect("request must be recorded");
    assert!(!queued.was_emitted());
    assert_eq!(queued.request().native_close_emission_fence(), None);

    assert_eq!(queued.request().native_close_edge(), Some(edge));

    let provider = test_provider();
    let emitted = ledger
        .take_new_requests(provider, InventoryGeneration::new(12), |actual| {
            assert_eq!(actual, edge);
            true
        })
        .expect("native close effect must be causally valid");
    assert_eq!(emitted.len(), 1);
    assert_eq!(
        emitted[0].native_close_emission_fence(),
        Some(NativeCloseEmissionFence {
            provider,
            binding,
            observed_through: CloseObservationGeneration::new(7),
            received_through: InventoryGeneration::new(12),
            after_effect: None,
        })
    );
}

#[test]
fn native_close_without_authoritative_edge_remains_requested_and_unemitted() {
    let mut ledger = EffectLedger::default();
    let binding = binding(1, 1);
    let edge = native_close_edge(binding, 8, 3);
    let effect = ledger
        .request_native_close(
            WorkspaceEpoch::new(1),
            close_request(edge),
            edge,
            NativeCloseResolution::Cancel,
            None,
        )
        .expect("native close effect must allocate");

    assert!(
        ledger
            .take_new_requests(test_provider(), InventoryGeneration::new(3), |_| false)
            .expect("queued request validation must succeed")
            .is_empty()
    );
    let queued = ledger.record(effect).expect("request must remain recorded");
    assert_eq!(queued.phase(), EffectPhase::Requested);
    assert!(!queued.was_emitted());
    assert_eq!(queued.request().native_close_emission_fence(), None);

    assert_eq!(
        ledger
            .take_new_requests(
                test_provider(),
                InventoryGeneration::new(4),
                |actual| actual == edge,
            )
            .expect("native close effect must be causally valid")
            .len(),
        1
    );
}

#[test]
fn native_close_effect_never_emits_against_a_reissued_edge() {
    let mut ledger = EffectLedger::default();
    let binding = binding(1, 1);
    let expected = native_close_edge(binding, 5, 5);
    let replacement = native_close_edge(binding, 7, 7);
    let same_provider_generation_with_new_ingress = native_close_edge(binding, 5, 6);
    let effect = ledger
        .request_native_close(
            WorkspaceEpoch::new(1),
            close_request(expected),
            expected,
            NativeCloseResolution::Cancel,
            None,
        )
        .expect("native close effect must allocate");

    assert!(
        ledger
            .take_new_requests(
                test_provider(),
                InventoryGeneration::new(7),
                |current| current == replacement,
            )
            .expect("stale native edge must remain queued")
            .is_empty()
    );
    assert!(
        ledger
            .take_new_requests(
                test_provider(),
                InventoryGeneration::new(7),
                |current| current == same_provider_generation_with_new_ingress,
            )
            .expect("reissued native edge must remain queued")
            .is_empty()
    );
    assert!(
        !ledger
            .record(effect)
            .expect("stale request remains queryable")
            .was_emitted()
    );

    let emitted = ledger
        .take_new_requests(test_provider(), InventoryGeneration::new(7), |current| {
            current == expected
        })
        .expect("exact native edge must emit");
    assert_eq!(emitted.len(), 1);
    assert_eq!(emitted[0].id(), effect);
    assert_eq!(emitted[0].native_close_edge(), Some(expected));
}

#[test]
fn native_close_deduplication_includes_the_exact_edge() {
    let mut ledger = EffectLedger::default();
    let binding = binding(1, 1);
    let first_edge = native_close_edge(binding, 5, 5);
    let second_edge = native_close_edge(binding, 7, 7);
    let request = close_request(first_edge);

    let first = ledger
        .request_native_close(
            WorkspaceEpoch::new(1),
            request,
            first_edge,
            NativeCloseResolution::Accept,
            None,
        )
        .expect("first native close effect must allocate");
    let second = ledger
        .request_native_close(
            WorkspaceEpoch::new(1),
            request,
            second_edge,
            NativeCloseResolution::Accept,
            None,
        )
        .expect("reissued edge must allocate a distinct effect");

    assert_ne!(first, second);
    assert_eq!(ledger.records().count(), 2);
}

#[test]
fn ordinary_effects_never_receive_a_native_close_fence() {
    let mut ledger = EffectLedger::default();
    let effect = ledger
        .request(create_effect(binding(1, 1)))
        .expect("ordinary effect must allocate");

    let emitted = take_ordinary_requests(&mut ledger, InventoryGeneration::new(1));
    assert_eq!(emitted.len(), 1);
    assert_eq!(emitted[0].id(), effect);
    assert_eq!(emitted[0].native_close_emission_fence(), None);
}

#[test]
fn native_close_predecessor_is_preserved_for_the_same_provider() {
    let mut ledger = EffectLedger::default();
    let binding = binding(1, 1);
    let edge = native_close_edge(binding, 6, 4);
    let provider = test_provider();
    let predecessor = ledger
        .request(PlatformEffect::CancelRootClose { binding })
        .expect("predecessor must allocate");
    ledger
        .take_new_requests(provider, InventoryGeneration::new(4), |_| false)
        .expect("predecessor must emit");
    let effect = ledger
        .request_native_close(
            WorkspaceEpoch::new(1),
            close_request(edge),
            edge,
            NativeCloseResolution::Cancel,
            Some(predecessor),
        )
        .expect("native close effect must allocate");

    let emitted = ledger
        .take_new_requests(provider, InventoryGeneration::new(5), |actual| {
            actual == edge
        })
        .expect("same-provider predecessor must be valid");
    assert_eq!(emitted[0].id(), effect);
    assert_eq!(
        emitted[0]
            .native_close_emission_fence()
            .expect("native request must carry a fence")
            .after_effect(),
        Some(predecessor)
    );
}

#[test]
fn emitted_native_close_is_never_extracted_twice() {
    let mut ledger = EffectLedger::default();
    let binding = binding(1, 1);
    let edge = native_close_edge(binding, 3, 2);
    ledger
        .request_native_close(
            WorkspaceEpoch::new(1),
            close_request(edge),
            edge,
            NativeCloseResolution::Accept,
            None,
        )
        .expect("native close effect must allocate");

    assert_eq!(
        ledger
            .take_new_requests(
                test_provider(),
                InventoryGeneration::new(2),
                |actual| actual == edge,
            )
            .expect("first extraction must succeed")
            .len(),
        1
    );
    assert!(
        ledger
            .take_new_requests(
                test_provider(),
                InventoryGeneration::new(9),
                |actual| actual == edge,
            )
            .expect("duplicate extraction check must succeed")
            .is_empty()
    );
}

#[test]
fn bounded_extraction_never_emits_an_older_queued_request() {
    let mut ledger = EffectLedger::default();
    let older = ledger
        .request(create_effect(binding(1, 1)))
        .expect("older effect must allocate");
    let boundary = ledger.latest_id();
    let newer = ledger
        .request(create_effect(binding(2, 1)))
        .expect("newer effect must allocate");

    let isolated = ledger
        .take_new_requests_after(
            boundary,
            test_provider(),
            InventoryGeneration::new(1),
            |_| false,
        )
        .expect("bounded extraction must succeed");
    assert_eq!(isolated.len(), 1);
    assert_eq!(isolated[0].id(), newer);
    assert!(
        !ledger
            .record(older)
            .expect("older record exists")
            .was_emitted()
    );
    assert!(
        ledger
            .record(newer)
            .expect("newer record exists")
            .was_emitted()
    );

    let remaining = take_ordinary_requests(&mut ledger, InventoryGeneration::new(1));
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id(), older);
}

#[test]
fn repeated_native_close_resolution_reuses_the_original_effect_across_ticks() {
    let mut ledger = EffectLedger::default();
    let binding = binding(1, 1);
    let edge = native_close_edge(binding, 3, 2);
    let request = close_request(edge);

    let first = ledger
        .request_native_close(
            WorkspaceEpoch::new(1),
            request,
            edge,
            NativeCloseResolution::Accept,
            None,
        )
        .expect("native close effect must allocate");
    let repeated_before_emission = ledger
        .request_native_close(
            WorkspaceEpoch::new(1),
            request,
            edge,
            NativeCloseResolution::Accept,
            None,
        )
        .expect("the same native close resolution must be idempotent");
    assert_eq!(repeated_before_emission, first);
    assert_eq!(ledger.records().count(), 1);

    assert_eq!(
        ledger
            .take_new_requests(
                test_provider(),
                InventoryGeneration::new(2),
                |actual| actual == edge,
            )
            .expect("native close extraction must succeed")
            .len(),
        1
    );

    let repeated_after_emission = ledger
        .request_native_close(
            WorkspaceEpoch::new(1),
            request,
            edge,
            NativeCloseResolution::Accept,
            None,
        )
        .expect("the emitted native close resolution must remain idempotent");
    assert_eq!(repeated_after_emission, first);
    assert_eq!(ledger.records().count(), 1);
    assert!(
        ledger
            .take_new_requests(
                test_provider(),
                InventoryGeneration::new(4),
                |actual| actual == edge,
            )
            .expect("duplicate extraction check must succeed")
            .is_empty()
    );
}

#[test]
fn ordinary_request_entry_points_reject_native_close_effects() {
    let mut ledger = EffectLedger::default();
    let binding = binding(1, 1);
    let edge = native_close_edge(binding, 1, 1);
    let native = PlatformEffect::ResolveNativeClose {
        request: close_request(edge),
        edge,
        resolution: NativeCloseResolution::Accept,
    };
    assert_eq!(
        ledger.request(native.clone()),
        Err(EffectLedgerError::NativeCloseRequiresDedicatedRequest)
    );
    assert_eq!(
        ledger.request_in(WorkspaceEpoch::new(1), native),
        Err(EffectLedgerError::NativeCloseRequiresDedicatedRequest)
    );
    assert!(ledger.records().next().is_none());
}

#[test]
fn only_accepted_native_close_is_non_idempotent() {
    let binding = binding(1, 1);
    let edge = native_close_edge(binding, 1, 1);
    let request = close_request(edge);
    assert!(
        PlatformEffect::ResolveNativeClose {
            request,
            edge,
            resolution: NativeCloseResolution::Accept,
        }
        .is_non_idempotent()
    );
    assert!(
        !PlatformEffect::ResolveNativeClose {
            request,
            edge,
            resolution: NativeCloseResolution::Cancel,
        }
        .is_non_idempotent()
    );
}

#[test]
fn request_is_emitted_once_and_indeterminate_never_redispatches() {
    let mut ledger = EffectLedger::default();
    let binding = binding(1, 1);
    let effect = ledger
        .request(create_effect(binding))
        .expect("effect identity must be available");
    assert_eq!(
        take_ordinary_requests(&mut ledger, InventoryGeneration::new(1)).len(),
        1
    );
    assert!(take_ordinary_requests(&mut ledger, InventoryGeneration::new(1)).is_empty());

    assert_eq!(
        ledger.report(
            test_provider(),
            WorkspaceEpoch::new(1),
            EffectResult::new(
                effect,
                WorkspaceEpoch::new(1),
                EffectDispatchResult::Indeterminate(
                    EffectIndeterminateReason::AcknowledgementLost,
                ),
            ),
        ),
        EffectTransition::Applied
    );
    assert!(take_ordinary_requests(&mut ledger, InventoryGeneration::new(1)).is_empty());
}

#[test]
fn definitive_failure_may_refine_an_indeterminate_dispatch() {
    let mut ledger = EffectLedger::default();
    let effect = ledger
        .request(create_effect(binding(1, 1)))
        .expect("effect identity must be available");
    let requests = take_ordinary_requests(&mut ledger, InventoryGeneration::new(1));
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].id(), effect);
    assert_eq!(
        ledger.report(
            test_provider(),
            WorkspaceEpoch::new(1),
            EffectResult::new(
                effect,
                WorkspaceEpoch::new(1),
                EffectDispatchResult::Indeterminate(
                    EffectIndeterminateReason::AcknowledgementLost,
                ),
            ),
        ),
        EffectTransition::Applied
    );
    assert_eq!(
        ledger.report(
            test_provider(),
            WorkspaceEpoch::new(1),
            EffectResult::new(
                effect,
                WorkspaceEpoch::new(1),
                EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped),
            ),
        ),
        EffectTransition::Applied
    );
    assert_eq!(
        ledger.record(effect).map(EffectRecord::phase),
        Some(EffectPhase::DispatchFailed(
            DispatchFailureReason::ProviderStopped
        ))
    );
}

#[test]
fn cleanup_continuation_dispatch_results_do_not_mutate_the_destructive_predecessor() {
    for (result, expected_phase) in [
        (
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped),
            EffectPhase::ObservationDispatchFailed(DispatchFailureReason::ProviderStopped),
        ),
        (
            EffectDispatchResult::Unsupported(EffectUnsupportedReason::BackendUnsupported),
            EffectPhase::ObservationUnsupported(EffectUnsupportedReason::BackendUnsupported),
        ),
    ] {
        let mut ledger = EffectLedger::default();
        let binding = binding(1, 1);
        let predecessor = ledger
            .request(PlatformEffect::ReleaseChild { binding })
            .expect("destructive cleanup must allocate");
        let _ = take_ordinary_requests(&mut ledger, InventoryGeneration::new(1));
        let continuation = ledger
            .request_in(
                WorkspaceEpoch::new(2),
                PlatformEffect::ContinueCleanup {
                    binding,
                    predecessor,
                    after: None,
                },
            )
            .expect("observation continuation must allocate");
        let _ = take_ordinary_requests(&mut ledger, InventoryGeneration::new(2));

        assert_eq!(
            ledger.report(
                test_provider(),
                WorkspaceEpoch::new(2),
                EffectResult::new(continuation, WorkspaceEpoch::new(2), result),
            ),
            EffectTransition::Applied
        );
        assert_eq!(
            ledger.record(continuation).map(EffectRecord::phase),
            Some(expected_phase)
        );
        assert_eq!(
            ledger.record(predecessor).map(EffectRecord::phase),
            Some(EffectPhase::Requested)
        );
    }
}

#[test]
fn cleanup_observation_token_is_the_only_cross_epoch_predecessor_result_authority() {
    let mut ledger = EffectLedger::default();
    let (predecessor_provider, observation_provider) = test_provider_replacement();
    let target_binding = binding(1, 1);
    let predecessor = ledger
        .request(PlatformEffect::ReleaseChild {
            binding: target_binding,
        })
        .expect("destructive cleanup must allocate");
    ledger
        .take_new_requests(predecessor_provider, InventoryGeneration::new(1), |_| false)
        .expect("destructive cleanup must emit");
    let continuation = ledger
        .request_in(
            WorkspaceEpoch::new(2),
            PlatformEffect::ContinueCleanup {
                binding: target_binding,
                predecessor,
                after: None,
            },
        )
        .expect("cleanup continuation must allocate");
    let emission = ledger
        .take_new_requests(observation_provider, InventoryGeneration::new(2), |_| false)
        .expect("cleanup continuation must emit")
        .pop()
        .expect("one cleanup continuation must be emitted");
    let token = emission
        .cleanup_observation_token()
        .expect("cleanup continuation must mint observation authority");
    let failure = EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped);

    assert_eq!(
        ledger.report(
            observation_provider,
            WorkspaceEpoch::new(2),
            EffectResult::new(predecessor, WorkspaceEpoch::new(1), failure),
        ),
        EffectTransition::StaleEpoch
    );
    let correlated = EffectResult::observed_via_cleanup(token, WorkspaceEpoch::new(1), failure);
    assert_eq!(correlated.receipt_epoch(), WorkspaceEpoch::new(2));
    assert_eq!(
        ledger
            .report_cleanup_observation(observation_provider, WorkspaceEpoch::new(2), correlated,),
        EffectTransition::Applied
    );
    assert_eq!(
        ledger.record(predecessor).map(EffectRecord::phase),
        Some(EffectPhase::DispatchFailed(
            DispatchFailureReason::ProviderStopped,
        ))
    );
    assert_eq!(
        ledger.record(continuation).map(EffectRecord::phase),
        Some(EffectPhase::CleanupResultObserved { predecessor })
    );
    assert_eq!(
        ledger
            .report_cleanup_observation(observation_provider, WorkspaceEpoch::new(2), correlated,),
        EffectTransition::Duplicate
    );
    assert_eq!(
        ledger.report_cleanup_observation(
            observation_provider,
            WorkspaceEpoch::new(2),
            EffectResult::observed_via_cleanup(
                token,
                WorkspaceEpoch::new(1),
                EffectDispatchResult::Unsupported(EffectUnsupportedReason::BackendUnsupported,),
            ),
        ),
        EffectTransition::CausalityBarrier
    );
}

#[test]
fn cleanup_observation_rejects_foreign_provider_and_spliced_binding_atomically() {
    let mut ledger = EffectLedger::default();
    let (predecessor_provider, observation_provider) = test_provider_replacement();
    let target_binding = binding(1, 1);
    let predecessor = ledger
        .request(PlatformEffect::ReleaseChild {
            binding: target_binding,
        })
        .expect("destructive cleanup must allocate");
    ledger
        .take_new_requests(predecessor_provider, InventoryGeneration::new(1), |_| false)
        .expect("destructive cleanup must emit");
    let continuation = ledger
        .request_in(
            WorkspaceEpoch::new(2),
            PlatformEffect::ContinueCleanup {
                binding: target_binding,
                predecessor,
                after: None,
            },
        )
        .expect("cleanup continuation must allocate");
    let emission = ledger
        .take_new_requests(observation_provider, InventoryGeneration::new(2), |_| false)
        .expect("cleanup continuation must emit")
        .pop()
        .expect("one cleanup continuation must be emitted");
    let token = emission
        .cleanup_observation_token()
        .expect("cleanup continuation must mint observation authority");
    let failure = EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped);

    assert_eq!(
        ledger.report_cleanup_observation(
            predecessor_provider,
            WorkspaceEpoch::new(2),
            EffectResult::observed_via_cleanup(token, WorkspaceEpoch::new(1), failure),
        ),
        EffectTransition::ProviderMismatch
    );
    assert_eq!(
        ledger.record(predecessor).map(EffectRecord::phase),
        Some(EffectPhase::Requested)
    );
    assert_eq!(
        ledger.record(continuation).map(EffectRecord::phase),
        Some(EffectPhase::Requested)
    );

    let spliced = CleanupObservationToken {
        binding: binding(1, 2),
        ..token
    };
    assert_eq!(
        ledger.report_cleanup_observation(
            observation_provider,
            WorkspaceEpoch::new(2),
            EffectResult::observed_via_cleanup(spliced, WorkspaceEpoch::new(1), failure),
        ),
        EffectTransition::BindingMismatch
    );
    assert_eq!(
        ledger.record(predecessor).map(EffectRecord::phase),
        Some(EffectPhase::Requested)
    );
    assert_eq!(
        ledger.record(continuation).map(EffectRecord::phase),
        Some(EffectPhase::Requested)
    );
}

#[test]
fn indeterminate_cleanup_observation_remains_live_until_a_definitive_result() {
    let mut ledger = EffectLedger::default();
    let (predecessor_provider, observation_provider) = test_provider_replacement();
    let binding = binding(1, 1);
    let predecessor = ledger
        .request(PlatformEffect::ReleaseChild { binding })
        .expect("destructive cleanup must allocate");
    ledger
        .take_new_requests(predecessor_provider, InventoryGeneration::new(1), |_| false)
        .expect("destructive cleanup must emit");
    ledger.revoke_provider_authority(predecessor_provider);
    let continuation = ledger
        .request_in(
            WorkspaceEpoch::new(2),
            PlatformEffect::ContinueCleanup {
                binding,
                predecessor,
                after: None,
            },
        )
        .expect("cleanup continuation must allocate");
    let emission = ledger
        .take_new_requests(observation_provider, InventoryGeneration::new(2), |_| false)
        .expect("cleanup continuation must emit")
        .pop()
        .expect("one cleanup continuation must be emitted");
    let token = emission
        .cleanup_observation_token()
        .expect("cleanup continuation must mint observation authority");

    let indeterminate = EffectResult::observed_via_cleanup(
        token,
        WorkspaceEpoch::new(1),
        EffectDispatchResult::Indeterminate(EffectIndeterminateReason::AcknowledgementLost),
    );
    assert_eq!(
        ledger.report_cleanup_observation(
            observation_provider,
            WorkspaceEpoch::new(2),
            indeterminate,
        ),
        EffectTransition::Applied
    );
    assert_eq!(
        ledger.record(continuation).map(EffectRecord::phase),
        Some(EffectPhase::CleanupObservationIndeterminate {
            predecessor,
            reason: EffectIndeterminateReason::AcknowledgementLost,
        })
    );
    assert_eq!(
        ledger.record(predecessor).map(EffectRecord::phase),
        Some(EffectPhase::Indeterminate(
            EffectIndeterminateReason::ProviderRestarted,
        )),
        "the observer owns its acknowledgement-loss reason without erasing provider revocation",
    );
    ledger.mark_boundary_published();
    assert_eq!(ledger.compact_published_terminal(&BTreeSet::new()), 0);

    let definitive = EffectResult::observed_via_cleanup(
        token,
        WorkspaceEpoch::new(1),
        EffectDispatchResult::Unsupported(EffectUnsupportedReason::BackendUnsupported),
    );
    assert_eq!(
        ledger
            .report_cleanup_observation(observation_provider, WorkspaceEpoch::new(2), definitive,),
        EffectTransition::Applied
    );
    assert_eq!(
        ledger.record(continuation).map(EffectRecord::phase),
        Some(EffectPhase::CleanupResultObserved { predecessor })
    );
    assert_eq!(
        ledger.record(predecessor).map(EffectRecord::phase),
        Some(EffectPhase::Unsupported(
            EffectUnsupportedReason::BackendUnsupported,
        ))
    );
    ledger.mark_boundary_published();
    assert_eq!(ledger.compact_published_terminal(&BTreeSet::new()), 1);
    assert!(ledger.record(continuation).is_none());
}

#[test]
fn provider_replacement_supersedes_an_indeterminate_cleanup_observer() {
    let mut ledger = EffectLedger::default();
    let (predecessor_provider, observation_provider) = test_provider_replacement();
    let binding = binding(1, 1);
    let predecessor = ledger
        .request(PlatformEffect::ReleaseChild { binding })
        .expect("destructive cleanup must allocate");
    ledger
        .take_new_requests(predecessor_provider, InventoryGeneration::new(1), |_| false)
        .expect("destructive cleanup must emit");
    let continuation = ledger
        .request_in(
            WorkspaceEpoch::new(2),
            PlatformEffect::ContinueCleanup {
                binding,
                predecessor,
                after: None,
            },
        )
        .expect("cleanup continuation must allocate");
    let token = ledger
        .take_new_requests(observation_provider, InventoryGeneration::new(2), |_| false)
        .expect("cleanup continuation must emit")
        .pop()
        .and_then(|emission| emission.cleanup_observation_token())
        .expect("cleanup continuation must mint observation authority");
    assert_eq!(
        ledger.report_cleanup_observation(
            observation_provider,
            WorkspaceEpoch::new(2),
            EffectResult::observed_via_cleanup(
                token,
                WorkspaceEpoch::new(1),
                EffectDispatchResult::Indeterminate(
                    EffectIndeterminateReason::AcknowledgementLost,
                ),
            ),
        ),
        EffectTransition::Applied
    );

    ledger.revoke_provider_authority(observation_provider);
    assert_eq!(
        ledger.record(continuation).map(EffectRecord::phase),
        Some(EffectPhase::Invalidated {
            cause: EffectInvalidation::PlatformProviderReplaced {
                provider: observation_provider,
            },
        })
    );
    ledger.mark_boundary_published();
    assert_eq!(ledger.compact_published_terminal(&BTreeSet::new()), 1);
    assert!(ledger.record(continuation).is_none());
    assert!(ledger.record(predecessor).is_some());
}

#[test]
fn conflicting_indeterminate_cleanup_observation_is_rejected_atomically() {
    let mut ledger = EffectLedger::default();
    let (predecessor_provider, observation_provider) = test_provider_replacement();
    let binding = binding(1, 1);
    let predecessor = ledger
        .request(PlatformEffect::ReleaseChild { binding })
        .expect("destructive cleanup must allocate");
    ledger
        .take_new_requests(predecessor_provider, InventoryGeneration::new(1), |_| false)
        .expect("destructive cleanup must emit");
    let continuation = ledger
        .request_in(
            WorkspaceEpoch::new(2),
            PlatformEffect::ContinueCleanup {
                binding,
                predecessor,
                after: None,
            },
        )
        .expect("cleanup continuation must allocate");
    let token = ledger
        .take_new_requests(observation_provider, InventoryGeneration::new(2), |_| false)
        .expect("cleanup continuation must emit")
        .pop()
        .and_then(|emission| emission.cleanup_observation_token())
        .expect("cleanup continuation must mint observation authority");
    let first = EffectResult::observed_via_cleanup(
        token,
        WorkspaceEpoch::new(1),
        EffectDispatchResult::Indeterminate(EffectIndeterminateReason::AcknowledgementLost),
    );
    assert_eq!(
        ledger.report_cleanup_observation(observation_provider, WorkspaceEpoch::new(2), first,),
        EffectTransition::Applied
    );
    let before = ledger.clone();
    assert_eq!(
        ledger.report_cleanup_observation(
            observation_provider,
            WorkspaceEpoch::new(2),
            EffectResult::observed_via_cleanup(
                token,
                WorkspaceEpoch::new(1),
                EffectDispatchResult::Indeterminate(EffectIndeterminateReason::ProviderRestarted,),
            ),
        ),
        EffectTransition::CausalityBarrier
    );
    assert_eq!(ledger.records, before.records);
    assert_eq!(
        ledger.report_cleanup_observation(observation_provider, WorkspaceEpoch::new(2), first,),
        EffectTransition::Duplicate
    );
    assert_eq!(
        ledger.record(continuation).map(EffectRecord::phase),
        Some(EffectPhase::CleanupObservationIndeterminate {
            predecessor,
            reason: EffectIndeterminateReason::AcknowledgementLost,
        })
    );
}

#[test]
fn cleanup_observation_churn_retains_only_subject_and_current_observer() {
    let mut ledger = EffectLedger::default();
    let provider = test_provider();
    let binding = binding(1, 1);
    let predecessor = ledger
        .request(PlatformEffect::ReleaseChild { binding })
        .expect("destructive cleanup must allocate");
    ledger
        .take_new_requests(provider, InventoryGeneration::new(1), |_| false)
        .expect("destructive cleanup must emit");
    let mut current = ledger
        .request_in(
            WorkspaceEpoch::new(1),
            PlatformEffect::ContinueCleanup {
                binding,
                predecessor,
                after: None,
            },
        )
        .expect("first cleanup observer must allocate");
    ledger
        .take_new_requests(provider, InventoryGeneration::new(2), |_| false)
        .expect("first cleanup observer must emit");

    for generation in 3..=10_002 {
        let successor = ledger
            .request_in(
                WorkspaceEpoch::new(1),
                PlatformEffect::ContinueCleanup {
                    binding,
                    predecessor,
                    after: Some(current),
                },
            )
            .expect("successor cleanup observer must allocate");
        assert_eq!(
            ledger.supersede_cleanup_observation(current, successor, predecessor, binding,),
            EffectTransition::Applied
        );
        ledger
            .take_new_requests(provider, InventoryGeneration::new(generation), |_| false)
            .expect("successor cleanup observer must emit");
        ledger.mark_boundary_published();
        assert_eq!(ledger.compact_published_terminal(&BTreeSet::new()), 1);
        assert_eq!(ledger.records.len(), 2);
        current = successor;
    }

    assert!(ledger.record(predecessor).is_some());
    assert!(ledger.record(current).is_some());
}

#[test]
fn stale_duplicate_and_binding_mismatch_results_are_harmless() {
    let mut ledger = EffectLedger::default();
    let current_binding = binding(4, 1);
    let effect = ledger
        .request(create_effect(current_binding))
        .expect("effect identity must be available");
    let result = EffectResult::new(
        effect,
        WorkspaceEpoch::new(4),
        EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected),
    );
    assert_eq!(
        ledger.report(test_provider(), WorkspaceEpoch::new(5), result),
        EffectTransition::StaleEpoch
    );
    let requests = take_ordinary_requests(&mut ledger, InventoryGeneration::new(1));
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].id(), effect);
    assert_eq!(
        ledger.report(test_provider(), WorkspaceEpoch::new(4), result),
        EffectTransition::Applied
    );
    assert_eq!(
        ledger.report(test_provider(), WorkspaceEpoch::new(4), result),
        EffectTransition::Duplicate
    );
    assert_eq!(
        ledger.mark_observed_applied(
            test_provider(),
            effect,
            binding(4, 2),
            InventoryGeneration::new(1),
        ),
        EffectTransition::BindingMismatch
    );
    assert_eq!(
        ledger.mark_observed_applied(
            test_provider(),
            effect,
            current_binding,
            InventoryGeneration::new(2),
        ),
        EffectTransition::Applied
    );
}

#[test]
fn effect_identity_exhaustion_does_not_mutate_the_ledger() {
    let mut ledger = EffectLedger::default();
    ledger.exhaust_effect_ids();
    let before = ledger.clone();
    assert_eq!(
        ledger.request(create_effect(binding(0, 1))),
        Err(EffectLedgerError::EffectIdExhausted)
    );
    assert_eq!(ledger, before);
}

#[test]
fn restore_can_issue_cleanup_for_an_exact_older_binding() {
    let mut ledger = EffectLedger::default();
    let old_binding = binding(3, 1);
    let effect = ledger
        .request_in(
            WorkspaceEpoch::new(4),
            PlatformEffect::CompensatingClose {
                binding: old_binding,
                compensates: EffectId::new(7),
            },
        )
        .expect("cleanup identity must be available");
    let request = ledger
        .take_new_requests(test_provider(), InventoryGeneration::new(1), |_| false)
        .expect("cleanup extraction must succeed")
        .pop()
        .expect("cleanup must be emitted");
    assert_eq!(request.epoch(), WorkspaceEpoch::new(4));
    assert_eq!(request.effect().binding(), old_binding);
    assert_eq!(
        ledger.mark_destroyed(effect, old_binding, InventoryGeneration::new(9)),
        EffectTransition::Applied
    );
}

#[test]
fn restore_invalidates_only_never_emitted_older_requests() {
    let mut ledger = EffectLedger::default();
    let first_old_emitted = ledger
        .request(create_effect(binding(1, 1)))
        .expect("old request must allocate");
    let old_emitted = ledger
        .request(create_effect(binding(2, 2)))
        .expect("second request must allocate");
    let _ = take_ordinary_requests(&mut ledger, InventoryGeneration::new(1));
    let current = ledger
        .request(create_effect(binding(3, 3)))
        .expect("current request must allocate");

    // Create a distinct never-emitted old request after draining the earlier records.
    let late_old = ledger
        .request_in(WorkspaceEpoch::new(1), create_effect(binding(1, 4)))
        .expect("late old request must allocate");
    ledger.invalidate_unemitted_for_workspace_replacement(WorkspaceEpoch::new(3));

    assert!(
        ledger
            .record(first_old_emitted)
            .is_some_and(EffectRecord::was_emitted)
    );
    assert!(
        ledger
            .record(old_emitted)
            .is_some_and(EffectRecord::was_emitted)
    );
    assert_eq!(
        ledger.record(late_old).map(EffectRecord::phase),
        Some(EffectPhase::Invalidated {
            cause: EffectInvalidation::WorkspaceReplaced {
                replacement_epoch: WorkspaceEpoch::new(3),
            },
        })
    );
    let expected_current = ledger
        .record(current)
        .expect("current record must remain")
        .request()
        .clone();
    let emissions = take_ordinary_requests(&mut ledger, InventoryGeneration::new(2));
    assert_eq!(emissions.len(), 1);
    assert_eq!(emissions[0].request(), &expected_current);
}

#[test]
fn exact_unemitted_invalidation_prevents_same_tick_extraction() {
    let mut ledger = EffectLedger::default();
    let effect = ledger
        .request(create_effect(binding(1, 1)))
        .expect("create request must allocate");

    assert_eq!(
        ledger.invalidate_unemitted(effect, EffectInvalidation::NativeCreateAborted),
        EffectInvalidationTransition::Applied
    );
    assert!(
        take_ordinary_requests(&mut ledger, InventoryGeneration::new(1)).is_empty(),
        "an invalidated create must never reach the adapter"
    );
    assert_eq!(
        ledger.record(effect).map(EffectRecord::phase),
        Some(EffectPhase::Invalidated {
            cause: EffectInvalidation::NativeCreateAborted,
        })
    );
}

#[test]
fn emission_and_observation_are_bound_to_the_exact_provider() {
    let mut ledger = EffectLedger::default();
    let (first, replacement) = test_provider_replacement();
    let binding = binding(1, 1);
    let effect = ledger
        .request(create_effect(binding))
        .expect("effect must allocate");

    let emissions = ledger
        .take_new_requests(first, InventoryGeneration::new(1), |_| false)
        .expect("effect must emit");
    assert_eq!(emissions.len(), 1);
    assert_eq!(emissions[0].provider(), first);
    assert_eq!(
        emissions[0].delivery().inventory_generation(),
        InventoryGeneration::new(1)
    );
    assert_eq!(
        ledger.record(effect).and_then(EffectRecord::provider),
        Some(first)
    );

    let result = EffectResult::new(
        effect,
        WorkspaceEpoch::new(1),
        EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped),
    );
    assert_eq!(
        ledger.report(replacement, WorkspaceEpoch::new(1), result),
        EffectTransition::ProviderMismatch
    );
    assert_eq!(
        ledger.mark_observed_applied(replacement, effect, binding, InventoryGeneration::new(2),),
        EffectTransition::ProviderMismatch
    );
    assert_eq!(
        ledger.record(effect).map(EffectRecord::phase),
        Some(EffectPhase::Requested)
    );
    assert_eq!(
        ledger.mark_observed_applied(first, effect, binding, InventoryGeneration::new(2)),
        EffectTransition::Applied
    );
}

#[test]
fn same_batch_causal_predecessor_is_delivered_to_one_provider() {
    let mut ledger = EffectLedger::default();
    let provider = test_provider();
    let binding = binding(1, 1);
    let predecessor = ledger
        .request(PlatformEffect::SetPointerPassthrough {
            binding,
            enabled: true,
            after: None,
        })
        .expect("predecessor must allocate");
    let successor = ledger
        .request(PlatformEffect::SetPointerPassthrough {
            binding,
            enabled: false,
            after: Some(predecessor),
        })
        .expect("successor must allocate");

    let emissions = ledger
        .take_new_requests(provider, InventoryGeneration::new(1), |_| false)
        .expect("same-batch causal chain must emit atomically");
    assert_eq!(
        emissions
            .iter()
            .map(PlatformEffectEmission::id)
            .collect::<Vec<_>>(),
        vec![predecessor, successor]
    );
    assert!(
        emissions
            .iter()
            .all(|emission| emission.provider() == provider)
    );
}

#[test]
fn cross_provider_after_dependency_rejects_the_complete_batch() {
    let mut ledger = EffectLedger::default();
    let (first, replacement) = test_provider_replacement();
    let binding = binding(1, 1);
    let predecessor = ledger
        .request(PlatformEffect::SetPointerPassthrough {
            binding,
            enabled: true,
            after: None,
        })
        .expect("predecessor must allocate");
    ledger
        .take_new_requests(first, InventoryGeneration::new(1), |_| false)
        .expect("predecessor must emit");
    let unrelated = ledger
        .request(create_effect(binding))
        .expect("unrelated effect must allocate");
    let successor = ledger
        .request(PlatformEffect::SetPointerPassthrough {
            binding,
            enabled: false,
            after: Some(predecessor),
        })
        .expect("successor must allocate");
    let before = ledger.clone();

    assert_eq!(
        ledger.take_new_requests(replacement, InventoryGeneration::new(2), |_| false),
        Err(EffectLedgerError::CausalPredecessorProviderMismatch {
            effect: successor,
            predecessor,
            requested_provider: replacement,
            delivered_provider: first,
        })
    );
    assert_eq!(ledger, before, "failed extraction must be atomic");
    assert_eq!(
        ledger.record(unrelated).and_then(EffectRecord::delivery),
        None
    );
    assert_eq!(
        ledger.record(successor).and_then(EffectRecord::delivery),
        None
    );
}

#[test]
fn cross_provider_cleanup_subject_is_observable_by_a_replacement_provider() {
    let mut ledger = EffectLedger::default();
    let (first, replacement) = test_provider_replacement();
    let binding = binding(1, 1);
    let predecessor = ledger
        .request(PlatformEffect::ReleaseChild { binding })
        .expect("predecessor must allocate");
    ledger
        .take_new_requests(first, InventoryGeneration::new(1), |_| false)
        .expect("predecessor must emit");
    let continuation = ledger
        .request_in(
            WorkspaceEpoch::new(2),
            PlatformEffect::ContinueCleanup {
                binding,
                predecessor,
                after: None,
            },
        )
        .expect("continuation must allocate");

    let emissions = ledger
        .take_new_requests(replacement, InventoryGeneration::new(2), |_| false)
        .expect("the destructive subject may predate the observation provider");
    assert_eq!(emissions.len(), 1);
    assert_eq!(emissions[0].id(), continuation);
    assert_eq!(emissions[0].delivery().provider(), replacement);
}

#[test]
fn cross_provider_cleanup_observation_lane_is_rejected() {
    let mut ledger = EffectLedger::default();
    let (first, replacement) = test_provider_replacement();
    let binding = binding(1, 1);
    let predecessor = ledger
        .request(PlatformEffect::ReleaseChild { binding })
        .expect("destructive subject must allocate");
    ledger
        .take_new_requests(first, InventoryGeneration::new(1), |_| false)
        .expect("destructive subject must emit");
    let old_observer = ledger
        .request_in(
            WorkspaceEpoch::new(2),
            PlatformEffect::ContinueCleanup {
                binding,
                predecessor,
                after: None,
            },
        )
        .expect("old observer must allocate");
    ledger
        .take_new_requests(first, InventoryGeneration::new(2), |_| false)
        .expect("old observer must emit");
    let successor = ledger
        .request_in(
            WorkspaceEpoch::new(2),
            PlatformEffect::ContinueCleanup {
                binding,
                predecessor,
                after: Some(old_observer),
            },
        )
        .expect("successor observer must allocate");

    assert_eq!(
        ledger.take_new_requests(replacement, InventoryGeneration::new(3), |_| false),
        Err(EffectLedgerError::CausalPredecessorProviderMismatch {
            effect: successor,
            predecessor: old_observer,
            requested_provider: replacement,
            delivered_provider: first,
        })
    );
}

#[test]
fn provider_cutover_invalidates_only_transitive_queued_causal_successors() {
    let mut ledger = EffectLedger::default();
    let (first, replacement) = test_provider_replacement();
    let binding = binding(1, 1);
    let predecessor = ledger
        .request(PlatformEffect::SetPointerPassthrough {
            binding,
            enabled: true,
            after: None,
        })
        .expect("predecessor must allocate");
    ledger
        .take_new_requests(first, InventoryGeneration::new(1), |_| false)
        .expect("predecessor must emit");
    let direct = ledger
        .request(PlatformEffect::SetPointerPassthrough {
            binding,
            enabled: false,
            after: Some(predecessor),
        })
        .expect("direct successor must allocate");
    let transitive = ledger
        .request(PlatformEffect::SetPointerPassthrough {
            binding,
            enabled: true,
            after: Some(direct),
        })
        .expect("transitive successor must allocate");
    let independent = ledger
        .request(create_effect(binding))
        .expect("independent effect must allocate");

    assert_eq!(
        ledger.invalidate_unemitted_causal_successors_for_provider_replacement(first),
        vec![direct, transitive]
    );
    for effect in [direct, transitive] {
        assert_eq!(
            ledger.record(effect).map(EffectRecord::phase),
            Some(EffectPhase::Invalidated {
                cause: EffectInvalidation::PlatformProviderReplaced { provider: first },
            })
        );
        assert_eq!(ledger.record(effect).and_then(EffectRecord::delivery), None);
    }
    assert_eq!(
        ledger.record(independent).map(EffectRecord::phase),
        Some(EffectPhase::Requested)
    );

    let emissions = ledger
        .take_new_requests(replacement, InventoryGeneration::new(2), |_| false)
        .expect("independent request may use replacement provider");
    assert_eq!(emissions.len(), 1);
    assert_eq!(emissions[0].id(), independent);
    assert_eq!(emissions[0].provider(), replacement);
}

#[test]
fn unavailable_predecessor_rejects_extraction_without_mutation() {
    let mut ledger = EffectLedger::default();
    let missing = EffectId::new(900);
    let successor = ledger
        .request(PlatformEffect::RequestFocus {
            binding: binding(1, 1),
            after: Some(missing),
        })
        .expect("successor must allocate");
    let before = ledger.clone();

    assert_eq!(
        ledger.take_new_requests(test_provider(), InventoryGeneration::new(1), |_| false),
        Err(EffectLedgerError::CausalPredecessorUnavailable {
            effect: successor,
            predecessor: missing,
        })
    );
    assert_eq!(ledger, before);
}

#[test]
fn provider_revocation_preserves_delivery_and_terminal_history() {
    let mut ledger = EffectLedger::default();
    let (first, replacement) = test_provider_replacement();
    let binding = binding(1, 1);
    let outstanding = ledger
        .request(create_effect(binding))
        .expect("outstanding effect must allocate");
    let indeterminate = ledger
        .request(create_effect(binding))
        .expect("indeterminate effect must allocate");
    let terminal = ledger
        .request(create_effect(binding))
        .expect("terminal effect must allocate");
    ledger
        .take_new_requests(first, InventoryGeneration::new(1), |_| false)
        .expect("initial effects must emit");
    assert_eq!(
        ledger.report(
            first,
            WorkspaceEpoch::new(1),
            EffectResult::new(
                indeterminate,
                WorkspaceEpoch::new(1),
                EffectDispatchResult::Indeterminate(
                    EffectIndeterminateReason::AcknowledgementLost,
                ),
            ),
        ),
        EffectTransition::Applied
    );
    assert_eq!(
        ledger.report(
            first,
            WorkspaceEpoch::new(1),
            EffectResult::new(
                terminal,
                WorkspaceEpoch::new(1),
                EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped,),
            ),
        ),
        EffectTransition::Applied
    );
    let queued = ledger
        .request(create_effect(binding))
        .expect("queued effect must allocate");

    ledger.revoke_provider_authority(first);

    for effect in [outstanding, indeterminate] {
        let record = ledger.record(effect).expect("emitted record must remain");
        assert_eq!(
            record.phase(),
            EffectPhase::Indeterminate(EffectIndeterminateReason::ProviderRestarted)
        );
        assert_eq!(record.provider(), Some(first));
    }
    assert_eq!(
        ledger.record(terminal).map(EffectRecord::phase),
        Some(EffectPhase::DispatchFailed(
            DispatchFailureReason::ProviderStopped,
        ))
    );
    let queued_record = ledger.record(queued).expect("queued record must remain");
    assert_eq!(queued_record.phase(), EffectPhase::Requested);
    assert_eq!(queued_record.delivery(), None);
    assert_eq!(
        ledger.report(
            replacement,
            WorkspaceEpoch::new(1),
            EffectResult::new(
                outstanding,
                WorkspaceEpoch::new(1),
                EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped,),
            ),
        ),
        EffectTransition::ProviderMismatch
    );
    assert_eq!(
        ledger.report(
            first,
            WorkspaceEpoch::new(1),
            EffectResult::new(
                outstanding,
                WorkspaceEpoch::new(1),
                EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped,),
            ),
        ),
        EffectTransition::ProviderMismatch,
        "a delayed result from the revoked provider must remain inert"
    );
    assert_eq!(
        ledger.take_new_requests(first, InventoryGeneration::new(2), |_| false),
        Err(EffectLedgerError::ProviderAuthorityRevoked { provider: first })
    );

    let replacement_emissions = ledger
        .take_new_requests(replacement, InventoryGeneration::new(2), |_| false)
        .expect("never-emitted request may move to the replacement provider");
    assert_eq!(replacement_emissions.len(), 1);
    assert_eq!(replacement_emissions[0].id(), queued);
    assert_eq!(replacement_emissions[0].provider(), replacement);
    assert_eq!(
        ledger.record(outstanding).and_then(EffectRecord::provider),
        Some(first)
    );
}

#[test]
fn destruction_is_exact_state_and_never_fabricates_delivery() {
    let mut ledger = EffectLedger::default();
    let binding = binding(1, 1);
    let effect = ledger
        .request(create_effect(binding))
        .expect("effect must allocate");

    assert_eq!(
        ledger.mark_destroyed(effect, binding, InventoryGeneration::new(9)),
        EffectTransition::Applied
    );
    let record = ledger.record(effect).expect("destroyed record must remain");
    assert_eq!(record.delivery(), None);
    assert_eq!(
        record.phase(),
        EffectPhase::Destroyed {
            inventory_generation: InventoryGeneration::new(9),
        }
    );
    assert!(
        ledger
            .take_new_requests(test_provider(), InventoryGeneration::new(10), |_| false)
            .expect("destroyed state must not corrupt extraction")
            .is_empty()
    );
}

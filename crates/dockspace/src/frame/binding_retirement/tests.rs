use super::*;
use crate::effect::DispatchFailureReason;
use crate::ids::{EngineAuthorityDomainId, SurfaceId, WorkspaceEpoch};
use crate::platform_provider::PlatformObservationAuthority;
use crate::viewport::WindowIncarnation;

fn binding(surface: u64, token: u64) -> ViewportBinding {
    ViewportBinding::new(
        EngineAuthorityDomainId::new_for_test(1),
        WorkspaceEpoch::new(0),
        SurfaceId::new(surface),
        WindowToken::new(token),
        WindowIncarnation::new(1),
    )
}

fn runtime_request(
    binding: ViewportBinding,
    status: BindingRetirementStatus,
) -> BindingRetirementRequest {
    BindingRetirementRequest {
        binding,
        role: ViewportRole::Child,
        ownership: ViewportOwnership::RuntimeOwned,
        origin: BindingRetirementOrigin::SurfaceVacated,
        status,
        observed: true,
        input_observations: WindowInputObservationStream::default(),
        close_observations: WindowCloseObservationStream::default(),
        may_reappear: false,
        cleanup: BindingRetirementCleanup::ReleaseOwnedWindow,
        retained_staging_resource: None,
    }
}

fn provider_pair() -> (PlatformObservationLease, PlatformObservationLease) {
    let mut authority = PlatformObservationAuthority::new(EngineAuthorityDomainId::new_for_test(1));
    let first = authority.create().expect("first provider must enroll");
    let ticket = authority
        .begin_replacement(first)
        .expect("first provider replacement must begin");
    let second = authority
        .finish_replacement(ticket)
        .expect("successor provider must activate");
    (first, second)
}

#[test]
fn quiesced_producer_compacts_only_its_destroyed_binding_guards() {
    let mut lifecycle = BindingRetirementLifecycle::default();
    let (first, second) = provider_pair();

    for token in 1..=10_000 {
        let provider = if token <= 6_000 { first } else { second };
        lifecycle.record_destroyed_tombstone(binding(1, token), provider);
    }

    let retention = lifecycle.retention_manifest();
    assert_eq!(retention.active_retirements(), 0);
    assert_eq!(retention.token_index_entries(), 0);
    assert_eq!(retention.cleanup_lineage_entries(), 0);
    assert_eq!(retention.destroyed_binding_guards(), 10_000);
    assert_eq!(retention.retained_structure_count(), 10_000);
    assert_eq!(
        retention.terminal_release_barrier(),
        Some(crate::retention::RuntimeRetentionReleaseBarrier::PlatformObservationIngressQuiesced),
    );

    assert_eq!(lifecycle.compact_destroyed_tombstones_from(first), 6_000);
    assert_eq!(
        lifecycle.retention_manifest().destroyed_binding_guards(),
        4_000
    );
    assert!(lifecycle.was_destroyed(binding(1, 6_001)));
    assert!(!lifecycle.was_destroyed(binding(1, 6_000)));

    assert_eq!(lifecycle.compact_destroyed_tombstones_from(first), 0);
    assert_eq!(lifecycle.compact_destroyed_tombstones_from(second), 4_000);
    assert_eq!(lifecycle.retention_manifest().destroyed_binding_guards(), 0);
    assert_eq!(
        lifecycle.retention_manifest().terminal_release_barrier(),
        None
    );
}

#[test]
fn exact_quiescence_compacts_same_provider_guards_without_cross_binding_fallthrough() {
    let mut lifecycle = BindingRetirementLifecycle::default();
    let (provider, foreign_provider) = provider_pair();
    for token in 1..=10_000 {
        lifecycle.record_destroyed_tombstone(binding(1, token), provider);
    }

    let first = binding(1, 1);
    assert_eq!(
        lifecycle
            .compact_destroyed_tombstone(first, foreign_provider)
            .expect_err("a foreign provider cannot compact the exact guard"),
        BindingRetirementLifecycleError::DestroyedTombstoneProviderMismatch {
            binding: first,
            expected: provider,
            submitted: foreign_provider,
        }
    );
    assert!(lifecycle.was_destroyed(first));

    for token in 1..=10_000 {
        lifecycle
            .compact_destroyed_tombstone(binding(1, token), provider)
            .expect("each exact producer proof must compact one guard");
    }
    assert_eq!(lifecycle.retention_manifest().destroyed_binding_guards(), 0);
    assert_eq!(
        lifecycle
            .compact_destroyed_tombstone(first, provider)
            .expect_err("one exact proof cannot compact twice"),
        BindingRetirementLifecycleError::DestroyedTombstoneMissing { binding: first },
    );
}

#[test]
fn retention_accounts_for_active_quarantine_and_cleanup_indexes() {
    let binding = binding(1, 10);
    let mut lifecycle = BindingRetirementLifecycle::default();
    lifecycle
        .begin(runtime_request(
            binding,
            BindingRetirementStatus::CleanupRequested {
                effect: EffectId::new(7),
            },
        ))
        .expect("retirement must begin");

    let retention = lifecycle.retention_manifest();
    assert_eq!(retention.active_retirements(), 1);
    assert_eq!(retention.token_index_entries(), 1);
    assert_eq!(retention.cleanup_lineage_entries(), 1);
    assert_eq!(retention.destroyed_binding_guards(), 0);
    assert_eq!(retention.retained_structure_count(), 3);
    assert_eq!(retention.terminal_release_barrier(), None);
}

#[test]
fn cleanup_lineage_keeps_emitted_predecessors_indexed_until_terminal() {
    let binding = binding(1, 10);
    let predecessor = EffectId::new(1);
    let successor = EffectId::new(2);
    let mut lifecycle = BindingRetirementLifecycle::default();
    lifecycle
        .begin(runtime_request(
            binding,
            BindingRetirementStatus::CleanupRequested {
                effect: predecessor,
            },
        ))
        .expect("retirement must begin");
    lifecycle
        .accept_cleanup_effect(binding, successor)
        .expect("cleanup continuation must become current");

    assert_eq!(
        lifecycle
            .reduce_effect(
                predecessor,
                EffectPhase::DispatchFailed(DispatchFailureReason::ProviderStopped),
            )
            .expect("late predecessor result must reduce"),
        Some(binding)
    );
    assert_eq!(
        lifecycle.get(&binding).map(BindingRetirement::status),
        Some(BindingRetirementStatus::CleanupFailed {
            effect: predecessor,
        })
    );
    assert_eq!(
        lifecycle
            .reduce_effect(
                successor,
                EffectPhase::DispatchFailed(DispatchFailureReason::ProviderStopped),
            )
            .expect("successor identity must remain in the same cleanup lineage"),
        Some(binding)
    );
}

#[test]
fn terminal_destruction_releases_token_and_complete_cleanup_lineage() {
    let binding = binding(1, 10);
    let (provider, _) = provider_pair();
    let predecessor = EffectId::new(1);
    let successor = EffectId::new(2);
    let mut lifecycle = BindingRetirementLifecycle::default();
    lifecycle
        .begin(runtime_request(
            binding,
            BindingRetirementStatus::CleanupRequested {
                effect: predecessor,
            },
        ))
        .expect("retirement must begin");
    lifecycle
        .accept_cleanup_effect(binding, successor)
        .expect("cleanup continuation must become current");

    let terminal = lifecycle
        .finish_destroyed(binding, provider)
        .expect("exact destruction must finish the retirement");
    assert_eq!(terminal.binding(), binding);
    assert!(lifecycle.was_destroyed(binding));
    assert!(!lifecycle.token_is_reserved(binding.token()));
    assert_eq!(
        lifecycle
            .reduce_effect(
                predecessor,
                EffectPhase::DispatchFailed(DispatchFailureReason::ProviderStopped),
            )
            .expect("terminal lineage lookup is infallible"),
        None
    );
    assert_eq!(
        lifecycle
            .reduce_effect(
                successor,
                EffectPhase::DispatchFailed(DispatchFailureReason::ProviderStopped),
            )
            .expect("terminal successor lookup is infallible"),
        None
    );
}

#[test]
fn begin_rejects_token_and_cleanup_effect_aliases() {
    let first = binding(1, 10);
    let same_token = binding(2, 10);
    let other = binding(3, 11);
    let effect = EffectId::new(1);
    let status = BindingRetirementStatus::CleanupRequested { effect };
    let mut lifecycle = BindingRetirementLifecycle::default();
    lifecycle
        .begin(runtime_request(first, status))
        .expect("first retirement must begin");

    assert_eq!(
        lifecycle.begin(runtime_request(same_token, status)),
        Err(BindingRetirementLifecycleError::TokenReserved {
            token: first.token(),
        })
    );
    assert_eq!(
        lifecycle.begin(runtime_request(other, status)),
        Err(BindingRetirementLifecycleError::CleanupEffectOwned {
            effect,
            owner: first,
        })
    );
}

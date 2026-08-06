use super::*;
use crate::effect::{DispatchFailureReason, EffectIndeterminateReason};
use crate::ids::{EngineAuthorityDomainId, SurfaceId, WorkspaceEpoch};
use crate::intent::Authority;
use crate::platform::{
    CloseEffectAcknowledgement, InputEffectAcknowledgement, WindowCloseObservation,
    WindowCloseState, WindowInputObservation, WindowInputState,
};
use crate::platform_provider::PlatformObservationAuthority;
use crate::viewport::{CloseObservationGeneration, InputObservationGeneration, WindowIncarnation};

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
) -> BindingCleanupRequest {
    BindingCleanupRequest {
        binding,
        role: ViewportRole::Child,
        ownership: ViewportOwnership::RuntimeOwned,
        origin: BindingRetirementOrigin::SurfaceVacated,
        status,
        observed: true,
        input_observations: WindowInputObservationStream::default(),
        close_observations: WindowCloseObservationStream::default(),
        may_reappear: false,
        cleanup: BindingCleanupAction::ReleaseOwnedWindow,
        retained_staging_resource: None,
        purpose: BindingCleanupPurpose::Retired,
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
fn provider_replacement_rebases_retired_binding_observation_generations() {
    let binding = binding(1, 10);
    let mut input_observations = WindowInputObservationStream::default();
    input_observations.observe(
        binding,
        Some(WindowInputObservation::new(
            binding,
            InputObservationGeneration::new(50),
            Authority::Known(WindowInputState::ReceivesInput),
            InputEffectAcknowledgement::known(None),
        )),
    );
    let mut close_observations = WindowCloseObservationStream::default();
    close_observations.observe(
        binding,
        Some(WindowCloseObservation::new(
            binding,
            CloseObservationGeneration::new(50),
            Authority::Known(WindowCloseState::LiveClear),
            CloseEffectAcknowledgement::known(None),
        )),
    );
    let mut request = runtime_request(binding, BindingRetirementStatus::AwaitingAppearance);
    request.input_observations = input_observations;
    request.close_observations = close_observations;
    let mut lifecycle = BindingCleanupLifecycle::default();
    lifecycle.begin(request).expect("retirement must begin");

    lifecycle.reset_for_provider_replacement();
    assert!(
        lifecycle
            .plan_drive(binding, false)
            .expect("successor provider must be able to drive the retained binding")
            .is_none()
    );
    assert_eq!(
        lifecycle.get(&binding).map(BindingRetirement::status),
        Some(BindingRetirementStatus::AwaitingAppearance),
    );
    let retirement = lifecycle
        .entries
        .get_mut(&binding)
        .expect("retirement must remain owned across provider replacement");
    assert!(!retirement.observed);
    assert!(retirement.may_reappear);
    retirement.input_observations.observe(
        binding,
        Some(WindowInputObservation::new(
            binding,
            InputObservationGeneration::new(1),
            Authority::Known(WindowInputState::PassThrough),
            InputEffectAcknowledgement::known(None),
        )),
    );
    retirement.close_observations.observe(
        binding,
        Some(WindowCloseObservation::new(
            binding,
            CloseObservationGeneration::new(1),
            Authority::Known(WindowCloseState::LiveRequested),
            CloseEffectAcknowledgement::known(None),
        )),
    );

    assert_eq!(
        retirement
            .input_observations
            .current()
            .map(WindowInputObservation::generation),
        Some(InputObservationGeneration::new(1)),
    );
    assert_eq!(
        retirement
            .close_observations
            .current()
            .map(WindowCloseObservation::generation),
        Some(CloseObservationGeneration::new(1)),
    );
}

#[test]
fn quiesced_producer_compacts_only_its_destroyed_binding_guards() {
    let mut lifecycle = BindingCleanupLifecycle::default();
    let (first, second) = provider_pair();

    for token in 1..=10_000 {
        let provider = if token <= 6_000 { first } else { second };
        lifecycle.record_destroyed_tombstone(binding(1, token), provider);
    }

    let retention = lifecycle.retention_manifest();
    assert_eq!(retention.active_cleanup_obligations(), 0);
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
    let mut lifecycle = BindingCleanupLifecycle::default();
    let (provider, foreign_provider) = provider_pair();
    for token in 1..=10_000 {
        lifecycle.record_destroyed_tombstone(binding(1, token), provider);
    }

    let first = binding(1, 1);
    assert_eq!(
        lifecycle
            .compact_destroyed_tombstone(first, foreign_provider)
            .expect_err("a foreign provider cannot compact the exact guard"),
        BindingCleanupError::DestroyedTombstoneProviderMismatch {
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
        BindingCleanupError::DestroyedTombstoneMissing { binding: first },
    );
}

#[test]
fn retention_accounts_for_active_quarantine_and_cleanup_indexes() {
    let binding = binding(1, 10);
    let mut lifecycle = BindingCleanupLifecycle::default();
    lifecycle
        .begin(runtime_request(
            binding,
            BindingRetirementStatus::CleanupRequested {
                effect: EffectId::new(7),
            },
        ))
        .expect("retirement must begin");

    let retention = lifecycle.retention_manifest();
    assert_eq!(retention.active_cleanup_obligations(), 1);
    assert_eq!(retention.token_index_entries(), 1);
    assert_eq!(retention.cleanup_lineage_entries(), 1);
    assert_eq!(retention.destroyed_binding_guards(), 0);
    assert_eq!(retention.retained_structure_count(), 3);
    assert_eq!(retention.terminal_release_barrier(), None);
}

#[test]
fn cleanup_lineage_retains_only_the_destructive_predecessor_and_current_observer() {
    let binding = binding(1, 10);
    let predecessor = EffectId::new(1);
    let successor = EffectId::new(2);
    let latest_successor = EffectId::new(3);
    let mut lifecycle = BindingCleanupLifecycle::default();
    lifecycle
        .begin(runtime_request(
            binding,
            BindingRetirementStatus::CleanupRequested {
                effect: predecessor,
            },
        ))
        .expect("retirement must begin");
    lifecycle
        .accept_cleanup_observation_effect(binding, successor, predecessor)
        .expect("cleanup continuation must become current");
    lifecycle
        .accept_cleanup_observation_effect(binding, latest_successor, predecessor)
        .expect("a newer cleanup continuation must supersede the intermediate observer");
    assert_eq!(lifecycle.retention_manifest().cleanup_lineage_entries(), 2);
    assert_eq!(
        lifecycle
            .reduce_effect(
                successor,
                EffectPhase::DispatchFailed(DispatchFailureReason::ProviderStopped),
            )
            .expect("superseded observer lookup is infallible"),
        None
    );

    assert_eq!(
        lifecycle
            .reduce_effect(
                predecessor,
                EffectPhase::Indeterminate(EffectIndeterminateReason::ProviderRestarted),
            )
            .expect("the exact destructive predecessor remains indexed"),
        Some(binding)
    );
    assert_eq!(
        lifecycle.get(&binding).map(BindingRetirement::status),
        Some(BindingRetirementStatus::CleanupRequested {
            effect: latest_successor,
        })
    );

    let mut direct = lifecycle.clone();
    assert_eq!(
        direct
            .reduce_effect(
                predecessor,
                EffectPhase::DispatchFailed(DispatchFailureReason::ProviderStopped),
            )
            .expect("a definitive direct predecessor result must reduce"),
        Some(binding)
    );
    assert_eq!(
        direct.get(&binding).map(BindingRetirement::status),
        Some(BindingRetirementStatus::CleanupFailed {
            effect: predecessor,
        })
    );
    assert_eq!(direct.retention_manifest().cleanup_lineage_entries(), 1);

    assert_eq!(
        lifecycle
            .reduce_cleanup_observation(
                latest_successor,
                predecessor,
                EffectPhase::DispatchFailed(DispatchFailureReason::ProviderStopped),
            )
            .expect("the active successor may correlate its exact predecessor"),
        Some(binding)
    );
    assert_eq!(
        lifecycle.get(&binding).map(BindingRetirement::status),
        Some(BindingRetirementStatus::CleanupFailed {
            effect: predecessor,
        })
    );
    assert_eq!(lifecycle.retention_manifest().cleanup_lineage_entries(), 1);
}

#[test]
fn cleanup_observation_alias_rejection_is_atomic() {
    let first = binding(1, 10);
    let second = binding(2, 11);
    let first_predecessor = EffectId::new(1);
    let second_effect = EffectId::new(2);
    let mut lifecycle = BindingCleanupLifecycle::default();
    lifecycle
        .begin(runtime_request(
            first,
            BindingRetirementStatus::CleanupRequested {
                effect: first_predecessor,
            },
        ))
        .expect("first retirement must begin");
    lifecycle
        .begin(runtime_request(
            second,
            BindingRetirementStatus::CleanupRequested {
                effect: second_effect,
            },
        ))
        .expect("second retirement must begin");
    let before = lifecycle.clone();

    assert_eq!(
        lifecycle.accept_cleanup_observation_effect(first, second_effect, first_predecessor),
        Err(BindingCleanupError::CleanupEffectOwned {
            effect: second_effect,
            owner: second,
        })
    );
    assert_eq!(lifecycle, before);
}

#[test]
fn terminal_destruction_releases_token_and_complete_cleanup_lineage() {
    let binding = binding(1, 10);
    let (provider, _) = provider_pair();
    let predecessor = EffectId::new(1);
    let successor = EffectId::new(2);
    let mut lifecycle = BindingCleanupLifecycle::default();
    lifecycle
        .begin(runtime_request(
            binding,
            BindingRetirementStatus::CleanupRequested {
                effect: predecessor,
            },
        ))
        .expect("retirement must begin");
    lifecycle
        .accept_cleanup_observation_effect(binding, successor, predecessor)
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
    let mut lifecycle = BindingCleanupLifecycle::default();
    lifecycle
        .begin(runtime_request(first, status))
        .expect("first retirement must begin");

    assert_eq!(
        lifecycle.begin(runtime_request(same_token, status)),
        Err(BindingCleanupError::TokenReserved {
            token: first.token(),
        })
    );
    assert_eq!(
        lifecycle.begin(runtime_request(other, status)),
        Err(BindingCleanupError::CleanupEffectOwned {
            effect,
            owner: first,
        })
    );
}

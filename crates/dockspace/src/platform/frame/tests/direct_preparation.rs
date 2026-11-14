use super::*;
use crate::platform::{NativeCreateReadiness, WindowCreatePreparationObservation};

#[test]
fn never_created_replacement_retains_the_exact_provider_guard_until_quiescence() {
    let mut coordinator = ViewportCoordinator::default();
    let epoch = WorkspaceEpoch::default();
    let surface = SurfaceId::new(710);
    let replacement = coordinator
        .registry
        .reserve(epoch, surface, ViewportRole::Child)
        .unwrap();
    let destroyed = ViewportBinding::new(
        EngineAuthorityDomainId::new_for_test(1),
        epoch,
        surface,
        WindowToken::new(709),
        WindowIncarnation::new(709),
    );
    let create = coordinator
        .effects
        .request(PlatformEffect::RequestReplacement {
            binding: replacement,
            placement: PhysicalRect::new(5.0, 0.0, 310.0, 225.0).unwrap(),
            role: ViewportRole::Child,
        })
        .unwrap();
    coordinator
        .recovery_replacements
        .begin(RecoveryPendingRequest::replacement(
            destroyed,
            ViewportRole::Child,
            recovery(712),
            replacement,
            create,
        ))
        .unwrap();
    let [request]: [_; 1] = coordinator.take_new_effects().try_into().unwrap();
    assert_eq!(request.id(), create);
    let provider = coordinator
        .effects
        .record(create)
        .unwrap()
        .provider()
        .unwrap();
    coordinator
        .report_effect_from(
            provider,
            epoch,
            EffectResult::new(
                create,
                epoch,
                EffectDispatchResult::DispatchFailed(
                    crate::platform::effect::DispatchFailureReason::WindowUnavailable,
                ),
            ),
        )
        .unwrap();
    assert!(coordinator.viewport(surface).is_none());
    assert_eq!(
        coordinator
            .recovery_pending(surface)
            .unwrap()
            .replacement_binding(),
        None
    );
    assert_eq!(
        coordinator
            .binding_retention_manifest()
            .terminal_binding_guards(),
        1
    );
    assert!(coordinator.take_new_effects().is_empty());
    coordinator
        .compact_quiesced_terminal_binding_guard(replacement, provider)
        .unwrap();
    assert_eq!(
        coordinator
            .binding_retention_manifest()
            .terminal_binding_guards(),
        0
    );
    assert_eq!(
        coordinator.compact_quiesced_terminal_binding_guard(replacement, provider),
        Err(ViewportCoordinatorError::TerminalBindingGuardMissing {
            binding: replacement
        }),
    );
}

#[test]
fn prepared_replacement_failed_before_first_live_revokes_readiness_and_closes_exact_binding() {
    let (mut coordinator, destroyed, replacement, obligation) = ready_recovery_replacement();
    let create = coordinator
        .recovery_pending(destroyed.surface())
        .and_then(RecoveryPending::replacement_effect)
        .expect("the first-live replacement must retain its create effect");
    assert!(coordinator.native_ready_basis(replacement).is_some());

    let failed = observed_window_at(replacement, 4).with_create_preparation(
        WindowCreatePreparationObservation::new(
            replacement,
            CoordinateObservationGeneration::new(4),
            Authority::Known(NativeCreateReadiness::Failed),
            None,
        ),
    );
    let transition = coordinator
        .publish_snapshot(&snapshot_at(4, vec![failed]))
        .expect("a renderer failure after Prepared must publish");

    assert_eq!(coordinator.native_ready_basis(replacement), None);
    assert!(
        !transition
            .actions()
            .iter()
            .any(|action| matches!(action, ViewportLifecycleAction::NativeSurfaceReady { .. }))
    );
    let pending = coordinator
        .recovery_pending(destroyed.surface())
        .expect("renderer failure must preserve the original content recovery");
    assert_eq!(pending.source_binding(), destroyed);
    assert_eq!(pending.recovery_obligation(), obligation);
    assert_eq!(pending.replacement_binding(), None);
    assert_eq!(
        pending.status(),
        RecoveryPendingStatus::ReplacementFailed {
            replacement: create,
            failed: create,
        }
    );
    assert!(coordinator.viewport(destroyed.surface()).is_none());
    assert_eq!(
        coordinator.admit_native_surface_first_live(
            NativeSurfaceEstablishmentOrigin::Destroyed { binding: destroyed },
            replacement,
            obligation
        ),
        Err(
            ViewportCoordinatorError::PendingRecoveryRegistrationMismatch {
                surface: destroyed.surface(),
            }
        )
    );

    let emitted = coordinator.take_new_effects();
    let [cleanup] = emitted.as_slice() else {
        panic!("renderer failure must issue one exact compensation: {emitted:?}");
    };
    assert_eq!(
        cleanup.effect(),
        &PlatformEffect::CompensatingClose {
            binding: replacement,
            compensates: create,
        }
    );
    assert_eq!(
        retirement(&coordinator, replacement).origin(),
        BindingRetirementOrigin::RecoveryReplacementFailed {
            replacement: create,
            failed: create,
        }
    );
    assert_eq!(
        retirement(&coordinator, replacement).status(),
        BindingRetirementStatus::CleanupRequested {
            effect: cleanup.id(),
        }
    );
    assert_eq!(coordinator.retained_native_staging_resources().len(), 0);

    coordinator
        .publish_snapshot(&snapshot_with_close_at(
            5,
            Vec::new(),
            vec![close_observation(
                replacement,
                5,
                WindowCloseState::Destroyed,
            )],
        ))
        .expect("exact failed-window destruction must finish compensation");
    assert!(!coordinator.binding_cleanup.contains_key(&replacement));
    coordinator
        .validate_native_staging_resource_conservation()
        .expect("failed replacement cleanup must not own a native-create resource");
}

#[test]
fn direct_replacement_provider_handoff_before_first_live_compensates_with_successor() {
    let mut coordinator = ViewportCoordinator::default();
    let surface = SurfaceId::new(810);
    let epoch = WorkspaceEpoch::default();
    let destroyed = ViewportBinding::new(
        EngineAuthorityDomainId::new_for_test(1),
        epoch,
        surface,
        WindowToken::new(809),
        WindowIncarnation::new(809),
    );
    let obligation = recovery(812);
    let replacement = coordinator
        .registry
        .reserve(epoch, surface, ViewportRole::Child)
        .expect("the direct replacement must reserve an exact pending binding");
    let create = coordinator
        .effects
        .request(PlatformEffect::RequestReplacement {
            binding: replacement,
            placement: PhysicalRect::new(5.0, 0.0, 310.0, 225.0)
                .expect("replacement placement must be valid"),
            role: ViewportRole::Child,
        })
        .expect("the direct replacement create effect must enter the ledger");
    coordinator
        .recovery_replacements
        .begin(RecoveryPendingRequest::replacement(
            destroyed,
            ViewportRole::Child,
            obligation,
            replacement,
            create,
        ))
        .expect("the original recovery must own the direct replacement");
    let emitted = coordinator.take_new_effects();
    assert!(matches!(emitted.as_slice(), [request] if request.id() == create));

    let prepared = observed_window_at(replacement, 1).with_create_preparation(
        WindowCreatePreparationObservation::new(
            replacement,
            CoordinateObservationGeneration::new(1),
            Authority::Known(NativeCreateReadiness::Prepared),
            Some(create),
        ),
    );
    let transition = coordinator
        .publish_snapshot(&snapshot_at(1, vec![prepared.clone()]))
        .expect("one visible snapshot may acknowledge create and prove preparation");
    assert!(matches!(
        transition.actions(),
        [ViewportLifecycleAction::NativeSurfaceReady {
            origin,
            binding,
            recovery_obligation,
        }] if *origin == NativeSurfaceEstablishmentOrigin::Destroyed { binding: destroyed }
            && *binding == replacement
            && *recovery_obligation == obligation
    ));
    assert!(coordinator.native_ready_basis(replacement).is_some());
    assert_eq!(
        coordinator
            .recovery_pending(surface)
            .expect("the direct replacement must wait for first-live admission")
            .status(),
        RecoveryPendingStatus::AwaitingFirstLivePresentation
    );
    let predecessor = coordinator
        .platform_provider()
        .expect("the prepared replacement must have an active provider");

    let ticket = coordinator
        .begin_platform_provider_replacement(predecessor)
        .expect("provider handoff must revoke a first-live direct replacement");
    let provider_lost = RecoveryPendingStatus::ReplacementProviderLost {
        replacement: create,
        last_effect: create,
    };
    let pending = coordinator
        .recovery_pending(surface)
        .expect("provider handoff must preserve the original recovery");
    assert_eq!(pending.status(), provider_lost);
    assert_eq!(pending.source_binding(), destroyed);
    assert_eq!(pending.recovery_obligation(), obligation);
    assert_eq!(pending.replacement_binding(), None);
    assert_eq!(coordinator.native_ready_basis(replacement), None);
    assert_eq!(
        retirement(&coordinator, replacement).origin(),
        BindingRetirementOrigin::RecoveryReplacementProviderLost {
            replacement: create,
            last_effect: create,
        }
    );

    let successor = coordinator
        .finish_platform_provider_replacement(ticket)
        .expect("the successor must enroll without adopting the old create delivery");
    assert_ne!(successor, predecessor);
    assert!(coordinator.take_new_effects().is_empty());
    coordinator
        .publish_snapshot(&snapshot_at(1, vec![prepared]))
        .expect("successor preparation must drive cleanup rather than revive the old proof");
    let emitted = coordinator.take_new_effects();
    let [cleanup] = emitted.as_slice() else {
        panic!("successor observation must issue one exact compensation: {emitted:?}");
    };
    assert_eq!(
        cleanup.effect(),
        &PlatformEffect::CompensatingClose {
            binding: replacement,
            compensates: create,
        }
    );
    assert_eq!(
        coordinator
            .effects()
            .record(cleanup.id())
            .and_then(|effect| effect.provider()),
        Some(successor)
    );
    assert_eq!(
        retirement(&coordinator, replacement).status(),
        BindingRetirementStatus::CleanupRequested {
            effect: cleanup.id(),
        }
    );
    assert_eq!(
        coordinator
            .recovery_pending(surface)
            .map(RecoveryPending::status),
        Some(provider_lost)
    );
    assert_eq!(coordinator.native_ready_basis(replacement), None);
    assert_eq!(
        coordinator.admit_native_surface_first_live(
            NativeSurfaceEstablishmentOrigin::Destroyed { binding: destroyed },
            replacement,
            obligation
        ),
        Err(ViewportCoordinatorError::PendingRecoveryRegistrationMismatch { surface })
    );

    coordinator
        .publish_snapshot(&snapshot_with_close_at(
            2,
            Vec::new(),
            vec![close_observation(
                replacement,
                2,
                WindowCloseState::Destroyed,
            )],
        ))
        .expect("successor destruction must settle the old replacement cleanup");
    assert!(!coordinator.binding_cleanup.contains_key(&replacement));
    assert_eq!(
        coordinator
            .recovery_pending(surface)
            .map(RecoveryPending::status),
        Some(provider_lost)
    );
    coordinator
        .validate_native_staging_resource_conservation()
        .expect("provider-lost replacement cleanup must not fabricate create resources");
}

#[test]
fn prepared_replacement_first_live_survives_terminal_effect_compaction() {
    let (mut coordinator, destroyed, replacement, obligation) = ready_recovery_replacement();
    let basis = coordinator
        .native_ready_basis(replacement)
        .expect("completed staging must authorize the first-live output");
    let create = coordinator
        .recovery_pending(destroyed.surface())
        .unwrap()
        .replacement_effect()
        .unwrap();
    let mut retained = BTreeSet::new();
    coordinator.extend_referenced_effects(&mut retained);
    coordinator.mark_effect_boundary_published();
    coordinator.compact_published_terminal_effects(&retained);

    assert_eq!(coordinator.native_ready_basis(replacement), Some(basis));
    let transition = coordinator
        .publish_snapshot(&recovery_presentation_snapshot(
            replacement,
            4,
            WindowPresentationState::Visible,
            None,
        ))
        .expect("compatible visible facts after compaction must preserve readiness");
    assert!(transition.actions().is_empty());
    assert_eq!(coordinator.native_ready_basis(replacement), Some(basis));
    coordinator
        .admit_native_surface_first_live(
            NativeSurfaceEstablishmentOrigin::Destroyed { binding: destroyed },
            replacement,
            obligation,
        )
        .expect("effect compaction must not strand exact first-live admission");
    assert_eq!(
        coordinator
            .viewport(replacement.surface())
            .map(ViewportRecord::admission),
        Some(ViewportAdmission::Admitted)
    );

    retained.clear();
    coordinator.extend_referenced_effects(&mut retained);
    coordinator.compact_published_terminal_effects(&retained);
    assert!(coordinator.effects().record(create).is_none());
}

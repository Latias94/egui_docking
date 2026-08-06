use super::*;

fn staging_cleanup(transition: &EngineTransition, pending: &PendingFixture) -> EffectId {
    transition
        .platform_effects()
        .iter()
        .find_map(|emission| {
            matches!(
                emission.effect(),
                PlatformEffect::CompensatingClose {
                    binding,
                    compensates,
                } if *binding == pending.replacement_binding
                    && *compensates == pending.replacement_effect
            )
            .then_some(emission.id())
        })
        .expect("staging close must own one correlated cleanup")
}

fn request_staging_close(pending: &mut PendingFixture) -> EffectId {
    let requested = publish_windows_with_close(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        vec![
            unavailable_host_window(pending.fixture.host_binding),
            replacement_window(pending.replacement_binding),
        ],
        &[(
            pending.replacement_binding,
            WindowCloseState::LiveRequested,
            None,
        )],
    );
    staging_cleanup(&requested, pending)
}

fn report_dispatch_failure(pending: &mut PendingFixture, effect: EffectId) -> EngineTransition {
    let expected_epoch = pending.fixture.engine.version().epoch();
    let provider = pending.fixture.presentation_host.platform_provider();
    submit_test_input(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        EngineInput::ReportPlatformEffect {
            provider,
            expected_epoch,
            result: EffectResult::new(
                effect,
                expected_epoch,
                EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected),
            ),
        },
    )
    .expect("effect dispatch failure must reduce")
}

#[test]
fn joined_provider_replacement_retains_an_emitted_unseen_recovery_window() {
    let mut pending = pending_joined_fixture();
    let replacement = pending.replacement_binding;
    let _ = replace_joined_backend(&mut pending.fixture);

    let absent = publish_windows(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        vec![unavailable_host_window(pending.fixture.host_binding)],
    );
    assert_no_new_effects(&absent);
    let retirement = pending
        .fixture
        .engine
        .viewport()
        .binding_retirements()
        .find_map(|(binding, retirement)| (binding == replacement).then_some(retirement))
        .expect("an emitted create remains quarantined until its exact outcome is known");
    assert_eq!(
        retirement.status(),
        BindingRetirementStatus::AwaitingAppearance
    );
    assert!(!retirement.observed());
    assert!(retirement.may_reappear());

    let appeared = publish_windows(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        vec![
            unavailable_host_window(pending.fixture.host_binding),
            replacement_window(replacement),
        ],
    );
    let cleanup = staging_cleanup(&appeared, &pending);
    let retirement = pending
        .fixture
        .engine
        .viewport()
        .binding_retirements()
        .find_map(|(binding, retirement)| (binding == replacement).then_some(retirement))
        .expect("the late window retains one exact cleanup owner");
    assert_eq!(
        retirement.status(),
        BindingRetirementStatus::CleanupRequested { effect: cleanup }
    );
}

#[test]
fn joined_provider_replacement_transfers_an_active_recovery_compensation() {
    let mut pending = pending_joined_fixture();
    let replacement = pending.replacement_binding;
    let recovered = make_host_current_and_retry_recovery(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
    );
    let predecessor_cleanup = staging_cleanup(&recovered, &pending);
    let _ = replace_joined_backend(&mut pending.fixture);

    let continued = publish_windows(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        vec![
            host_window(pending.fixture.host_binding),
            replacement_window(replacement),
        ],
    );
    let continuation = continued
        .platform_effects()
        .iter()
        .find_map(|emission| match emission.effect() {
            PlatformEffect::ContinueCleanup {
                binding,
                predecessor,
                ..
            } if *binding == replacement && *predecessor == predecessor_cleanup => {
                Some(emission.id())
            }
            _ => None,
        })
        .expect("successor provider must continue the exact destructive cleanup");
    let retirement = pending
        .fixture
        .engine
        .viewport()
        .binding_retirements()
        .find_map(|(binding, retirement)| (binding == replacement).then_some(retirement))
        .expect("the exact binding retirement owns the continuation");
    assert_eq!(
        retirement.status(),
        BindingRetirementStatus::CleanupRequested {
            effect: continuation,
        },
        "unexpected successor cleanup lineage: {:?}",
        continued.platform_effects(),
    );

    let destroyed = publish_windows_with_close(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        vec![host_window(pending.fixture.host_binding)],
        &[(replacement, WindowCloseState::Destroyed, Some(continuation))],
    );
    assert_no_new_effects(&destroyed);
    assert!(
        pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .is_none(),
        "exact destruction must consume the transferred recovery obligation"
    );
    assert!(
        pending
            .fixture
            .engine
            .viewport()
            .binding_retirements()
            .all(|(binding, _)| binding != replacement),
        "exact destruction must retire the transferred cleanup owner"
    );
}

#[test]
fn workspace_restore_preserves_a_transferred_recovery_compensation() {
    let mut pending = pending_joined_fixture();
    let replacement = pending.replacement_binding;
    let recovered = make_host_current_and_retry_recovery(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
    );
    let predecessor_cleanup = staging_cleanup(&recovered, &pending);
    let _ = replace_joined_backend(&mut pending.fixture);
    assert!(
        pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .is_none(),
        "the retirement lifecycle must become the sole cleanup owner"
    );
    let restored = submit_test_input(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        EngineInput::ReplaceWorkspace(host_only_workspace()),
    )
    .expect("workspace restore must atomically detach the transferred recovery callback");
    let old_host_binding = pending.fixture.host_binding;
    pending.fixture.host_binding = match restored.reduced_inputs()[0].outcome() {
        InputOutcome::WorkspaceReplaced { reconciliation, .. } => reconciliation
            .rebound()
            .iter()
            .find_map(|(old, rebound)| (*old == old_host_binding).then_some(*rebound))
            .expect("workspace restore must expose the rebound root binding"),
        outcome => panic!("unexpected workspace restore outcome: {outcome:?}"),
    };
    let continuation = restored
        .platform_effects()
        .iter()
        .find_map(|emission| match emission.effect() {
            PlatformEffect::ContinueCleanup {
                binding,
                predecessor,
                ..
            } if *binding == replacement && *predecessor == predecessor_cleanup => {
                Some(emission.id())
            }
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!(
                "workspace restore must preserve the exact destructive cleanup lineage: {:?}",
                restored.platform_effects()
            )
        });
    assert!(
        pending
            .fixture
            .engine
            .viewport()
            .recovery_pending(SURFACE_CHILD)
            .is_none(),
        "workspace restore must consume the obsolete recovery callback"
    );
    let retirement = pending
        .fixture
        .engine
        .viewport()
        .binding_retirements()
        .find_map(|(binding, retirement)| (binding == replacement).then_some(retirement))
        .expect("workspace restore must retain the exact cleanup owner");
    assert_eq!(
        retirement.origin(),
        BindingRetirementOrigin::RecoveryReplacementCompensationTransferred {
            replacement: pending.replacement_effect,
            cleanup: predecessor_cleanup,
        }
    );
    assert_eq!(
        retirement.status(),
        BindingRetirementStatus::CleanupRequested {
            effect: continuation,
        }
    );

    let destroyed = publish_windows_with_close(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        vec![host_window(pending.fixture.host_binding)],
        &[(replacement, WindowCloseState::Destroyed, Some(continuation))],
    );
    assert_no_new_effects(&destroyed);
    assert!(
        pending
            .fixture
            .engine
            .viewport()
            .binding_retirements()
            .all(|(binding, _)| binding != replacement),
        "exact destruction must terminate the transferred workspace retirement"
    );
}

#[test]
fn staging_close_cleanup_migrates_when_recovery_show_fails() {
    let mut pending = pending_fixture();
    let show = present_replacement_pre_show(&mut pending);
    let cleanup = request_staging_close(&mut pending);

    let failed_show = report_dispatch_failure(&mut pending, show);
    assert_no_new_effects(&failed_show);
    assert_eq!(
        effect_count(&pending.fixture.engine, |effect| matches!(
            effect,
            PlatformEffect::CompensatingClose { binding, .. }
                if *binding == pending.replacement_binding
        )),
        1,
        "show failure must transfer the existing cleanup instead of minting another"
    );
    let retirement = pending
        .fixture
        .engine
        .viewport()
        .binding_retirements()
        .find_map(|(binding, retirement)| {
            (binding == pending.replacement_binding).then_some(retirement)
        })
        .expect("failed replacement binding must be retired");
    assert_eq!(
        retirement.status(),
        BindingRetirementStatus::CleanupRequested { effect: cleanup }
    );

    let failed_cleanup = report_dispatch_failure(&mut pending, cleanup);
    assert_no_new_effects(&failed_cleanup);
    let expected = pending.fixture.engine.version();
    let retried = submit_test_input(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        EngineInput::RetryViewportCleanup {
            expected,
            failed_effect: cleanup,
        },
    )
    .expect("the transferred cleanup must remain explicitly retryable");
    let retry = staging_cleanup(&retried, &pending);
    assert_ne!(retry, cleanup);
}

#[test]
fn resumed_staging_close_does_not_transfer_its_revoked_cleanup() {
    let mut pending = pending_fixture();
    let show = present_replacement_pre_show(&mut pending);
    let revoked_cleanup = request_staging_close(&mut pending);
    let failed_cleanup = report_dispatch_failure(&mut pending, revoked_cleanup);
    assert_no_new_effects(&failed_cleanup);

    let cleared = publish_windows_with_facts(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        vec![
            unavailable_host_window(pending.fixture.host_binding),
            replacement_window(pending.replacement_binding),
        ],
        Authority::Known(WindowPresentationState::Visible),
        &[(
            pending.replacement_binding,
            WindowCloseState::LiveClear,
            None,
        )],
        platform_capabilities(),
    );
    assert_no_new_effects(&cleared);

    let failed_show = report_dispatch_failure(&mut pending, show);
    let replacement_cleanup = staging_cleanup(&failed_show, &pending);
    assert_ne!(replacement_cleanup, revoked_cleanup);
    let retirement = pending
        .fixture
        .engine
        .viewport()
        .binding_retirements()
        .find_map(|(binding, retirement)| {
            (binding == pending.replacement_binding).then_some(retirement)
        })
        .expect("show failure must create an independent retirement owner");
    assert_eq!(
        retirement.status(),
        BindingRetirementStatus::CleanupRequested {
            effect: replacement_cleanup,
        }
    );
}

#[test]
fn workspace_restore_continues_an_emitted_staging_cleanup() {
    let mut pending = pending_fixture();
    let cleanup = request_staging_close(&mut pending);

    let restored = submit_test_input(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        EngineInput::ReplaceWorkspace(host_only_workspace()),
    )
    .expect("workspace restore must migrate the staging cleanup owner");
    let continuation = restored
        .platform_effects()
        .iter()
        .find_map(|emission| match emission.effect() {
            PlatformEffect::ContinueCleanup {
                binding,
                predecessor,
                after: None,
            } if *binding == pending.replacement_binding && *predecessor == cleanup => {
                Some(emission.id())
            }
            _ => None,
        })
        .expect("restore must continue observation of the existing cleanup");
    assert!(restored.platform_effects().iter().all(|emission| {
        !matches!(
            emission.effect(),
            PlatformEffect::CompensatingClose { .. }
                | PlatformEffect::ReleaseChild { .. }
                | PlatformEffect::RequestRootClose { .. }
        )
    }));
    let retirement = pending
        .fixture
        .engine
        .viewport()
        .binding_retirements()
        .find_map(|(binding, retirement)| {
            (binding == pending.replacement_binding).then_some(retirement)
        })
        .expect("restored replacement binding must retain one cleanup owner");
    assert_eq!(
        retirement.status(),
        BindingRetirementStatus::CleanupRequested {
            effect: continuation,
        }
    );
}

#[test]
fn workspace_restore_preserves_a_failed_staging_cleanup_retry_fence() {
    let mut pending = pending_fixture();
    let cleanup = request_staging_close(&mut pending);
    let failed = report_dispatch_failure(&mut pending, cleanup);
    assert_no_new_effects(&failed);

    let restored = submit_test_input(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        EngineInput::ReplaceWorkspace(host_only_workspace()),
    )
    .expect("workspace restore must preserve the failed cleanup owner");
    assert_no_new_effects(&restored);
    let retirement = pending
        .fixture
        .engine
        .viewport()
        .binding_retirements()
        .find_map(|(binding, retirement)| {
            (binding == pending.replacement_binding).then_some(retirement)
        })
        .expect("restored replacement binding must remain retired");
    assert_eq!(
        retirement.status(),
        BindingRetirementStatus::CleanupFailed { effect: cleanup }
    );

    let expected = pending.fixture.engine.version();
    let retried = submit_test_input(
        &mut pending.fixture.engine,
        &mut pending.fixture.presentation_host,
        EngineInput::RetryViewportCleanup {
            expected,
            failed_effect: cleanup,
        },
    )
    .expect("only explicit input may retry the failed destructive cleanup");
    let retry = staging_cleanup(&retried, &pending);
    assert_ne!(retry, cleanup);
}

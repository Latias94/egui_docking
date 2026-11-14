//! Lifecycle behavior regressions.

use super::*;

#[test]
fn exact_root_continuation_preserves_only_the_current_physical_focus_fact() {
    let predecessor = binding();
    let successor = ViewportBinding::new(
        EngineAuthorityDomainId::new_for_test(1),
        WorkspaceEpoch::new(2),
        SurfaceId::new(2),
        WindowToken::new(1),
        WindowIncarnation::new(2),
    );
    for focus in [
        GlobalFocusedWindow::Dock(predecessor.window()),
        GlobalFocusedWindow::Dock(successor.window()),
        GlobalFocusedWindow::Foreign,
        GlobalFocusedWindow::None,
    ] {
        let mut coordinator = ViewportFocusCoordinator::default();
        publish_platform_focus(&mut coordinator, focus_observation(5, focus), 1);
        coordinator.reconcile_workspace_replacement(&[(predecessor, successor)]);
        let expected_ui_focus = match focus {
            GlobalFocusedWindow::Foreign => CurrentUiFocus::External,
            GlobalFocusedWindow::None => CurrentUiFocus::None,
            GlobalFocusedWindow::Dock(_) => CurrentUiFocus::Unknown,
        };
        assert_eq!(coordinator.current_focus(), Some(expected_ui_focus));
        assert_eq!(coordinator.pending_pane_intent(), None);
        assert_eq!(
            coordinator.panel_focus(predecessor.surface()),
            PanelFocusRecord::NoHistory
        );
        if focus == GlobalFocusedWindow::Dock(predecessor.window()) {
            let expected = focus_observation(5, GlobalFocusedWindow::Dock(successor.window()));
            assert_eq!(coordinator.focus_observation(), Some(expected));
            assert!(matches!(
                publish_platform_focus(&mut coordinator, expected, 2,),
                FocusObservationTransition::Duplicate
            ));
            assert!(matches!(
                publish_platform_focus(
                    &mut coordinator,
                    focus_observation(6, GlobalFocusedWindow::Foreign),
                    3,
                ),
                FocusObservationTransition::Applied(_)
            ));
            assert_eq!(coordinator.current_focus(), Some(CurrentUiFocus::External));
        } else if matches!(
            focus,
            GlobalFocusedWindow::Foreign | GlobalFocusedWindow::None
        ) {
            assert_eq!(
                coordinator.focus_observation(),
                Some(focus_observation(5, focus))
            );
        } else {
            assert_eq!(coordinator.focus_observation(), None);
        }
    }
    let mut revoked = ViewportFocusCoordinator::default();
    publish_platform_focus(
        &mut revoked,
        focus_observation(5, GlobalFocusedWindow::Dock(predecessor.window())),
        1,
    );
    revoked.focus_observations.observe(None);
    revoked.reconcile_workspace_replacement(&[(predecessor, successor)]);
    assert_eq!(
        revoked.focus_observation(),
        None,
        "continuation cannot revive revoked authority"
    );
    assert_eq!(revoked.current_focus(), Some(CurrentUiFocus::Unknown));
}

#[test]
fn repeated_workspace_replacement_cannot_turn_revoked_focus_into_no_history() {
    for last in [
        focus_tombstone(5),
        focus_observation(5, GlobalFocusedWindow::Dock(binding().window())),
    ] {
        let mut coordinator = ViewportFocusCoordinator::default();
        publish_platform_focus(
            &mut coordinator,
            focus_observation(3, GlobalFocusedWindow::Dock(binding().window())),
            1,
        );
        publish_platform_focus(&mut coordinator, last, 2);
        let stream = coordinator.focus_observations;
        for _ in 0..2 {
            coordinator.reconcile_workspace_replacement(&[]);
            assert_eq!(coordinator.focus_observations, stream);
            assert_eq!(coordinator.current_focus(), Some(CurrentUiFocus::Unknown));
            assert_eq!(
                coordinator.workspace_replacement_focus(),
                WorkspaceReplacementFocus::OutsideDock
            );
            assert_eq!(coordinator.pending_pane_intent(), None);
        }
    }

    let mut unobserved = ViewportFocusCoordinator::default();
    unobserved.reconcile_workspace_replacement(&[]);
    assert_eq!(unobserved.current_focus(), None);
    assert_eq!(
        unobserved.workspace_replacement_focus(),
        WorkspaceReplacementFocus::NoHistory
    );
}

#[test]
fn workspace_replacement_preserves_the_focus_provider_replay_watermark() {
    let mut coordinator = ViewportFocusCoordinator::default();
    let _ = publish_platform_focus(
        &mut coordinator,
        focus_observation(99, GlobalFocusedWindow::Dock(binding().window())),
        1,
    );

    coordinator.reconcile_workspace_replacement(&[]);
    let replacement = focus_observation(1, GlobalFocusedWindow::Dock(second_binding().window()));
    assert!(matches!(
        publish_platform_focus(&mut coordinator, replacement, 2,),
        FocusObservationTransition::Stale { .. }
    ));
    assert_eq!(coordinator.focus_observation(), None);
    let replacement = focus_observation(100, GlobalFocusedWindow::Dock(second_binding().window()));
    assert!(matches!(
        publish_platform_focus(&mut coordinator, replacement, 3,),
        FocusObservationTransition::Applied(_)
    ));
    assert_eq!(coordinator.focus_observation(), Some(replacement));
}

#[test]
fn replacement_capture_distinguishes_outside_focus_and_pending_semantic_owner() {
    let target = binding();
    let mut coordinator = ViewportFocusCoordinator::default();
    assert_eq!(
        coordinator.workspace_replacement_focus(),
        WorkspaceReplacementFocus::NoHistory
    );
    coordinator
        .panel_focus_by_surface
        .insert(target.surface(), PanelFocusRecord::Item(ItemId::new(1)));
    publish_platform_focus(
        &mut coordinator,
        focus_observation(1, GlobalFocusedWindow::Foreign),
        1,
    );
    assert_eq!(
        coordinator.workspace_replacement_focus(),
        WorkspaceReplacementFocus::OutsideDock
    );
    let waiting = coordinator
        .request_workspace_replacement_focus(
            target.surface(),
            PanelFocus::Item(ItemId::new(1)),
            true,
            focus_stamp(2),
        )
        .unwrap()
        .unwrap();
    publish_platform_focus(
        &mut coordinator,
        focus_observation(2, GlobalFocusedWindow::Foreign),
        3,
    );
    assert_eq!(
        coordinator.pending_pane_intent().unwrap().id(),
        waiting.id()
    );
    assert_eq!(
        coordinator.workspace_replacement_focus(),
        WorkspaceReplacementFocus::Dock {
            surface: target.surface(),
            item: Some(ItemId::new(1)),
        }
    );
    let semantic = coordinator
        .request_local_workspace_focus(target.surface(), None, focus_stamp(4))
        .unwrap()
        .unwrap();
    assert_ne!(semantic.id(), waiting.id());
    assert_eq!(coordinator.pending_pane_intent(), Some(semantic));
    assert_eq!(
        coordinator.workspace_replacement_focus(),
        WorkspaceReplacementFocus::Dock {
            surface: target.surface(),
            item: None,
        }
    );
}

#[test]
fn focus_delta_hides_waiting_and_publishes_only_local_or_ready_bound_intents() {
    let target = binding();
    let effects = EffectLedger::default();
    let before = ViewportFocusCoordinator::default();
    let mut waiting = before.clone();
    let intent = waiting
        .request_workspace_replacement_focus(
            target.surface(),
            PanelFocus::Item(ItemId::new(1)),
            true,
            focus_stamp(1),
        )
        .unwrap()
        .unwrap();
    waiting.mark_boundary_published();
    assert_eq!(waiting.published_pane_intent(), None);
    assert!(
        FocusDelta::between(&before, &waiting, &effects, &effects, &[])
            .pane_intent()
            .is_none()
    );
    assert!(
        FocusDelta::between(&waiting, &before, &effects, &effects, &[])
            .pane_intent()
            .is_none()
    );

    let mut ready = waiting.clone();
    ready.resume_workspace_replacement_focus(|_| Some(target));
    ready.mark_boundary_published();
    let bound = ready.published_pane_intent().unwrap();
    assert_eq!(bound.id(), intent.id());
    let delta = FocusDelta::between(&waiting, &ready, &effects, &effects, &[]);
    let change = delta.pane_intent().unwrap();
    assert_eq!(change.before(), &None);
    assert_eq!(change.after(), &Some(bound));

    let mut local = before.clone();
    let local_intent = local
        .request_local_pane_focus(target.surface(), ItemId::new(1), None, focus_stamp(1))
        .unwrap()
        .unwrap();
    local.mark_boundary_published();
    let delta = FocusDelta::between(&before, &local, &effects, &effects, &[]);
    assert_eq!(delta.pane_intent().unwrap().after(), &Some(local_intent));
    let delta = FocusDelta::between(&local, &waiting, &effects, &effects, &[]);
    assert_eq!(delta.pane_intent().unwrap().before(), &Some(local_intent));
    assert_eq!(delta.pane_intent().unwrap().after(), &None);
}

#[test]
fn focus_delta_reports_final_intent_surface_and_effect_state() {
    let target = binding();
    let before = ViewportFocusCoordinator::default();
    let before_effects = EffectLedger::default();
    let mut provider_authority =
        PlatformObservationAuthority::new(EngineAuthorityDomainId::new_for_test(1));
    let provider = provider_authority
        .create()
        .expect("test platform provider must be available");
    let mut after = before.clone();
    let _ = after
        .publish_focus_observation(
            focus_observation(1, GlobalFocusedWindow::Dock(target.window())),
            focus_stamp(1),
            |candidate| candidate == target,
            |_, item| item == ItemId::new(1),
        )
        .expect("focus observation must apply");
    assert!(matches!(
        after.publish_pane_focus_observation(
            PaneFocusObservation::new(
                PaneFocusObservationGeneration::new(1),
                target,
                PanelFocus::Item(ItemId::new(1)),
            ),
            |candidate| candidate == target,
            |_, item| item == ItemId::new(1),
        ),
        PaneFocusObservationTransition::Applied { .. }
    ));
    let activation = after
        .request_activation(
            ViewportActivationRequest::explicit(target, PanelFocus::Item(ItemId::new(1))),
            focus_stamp(2),
            true,
            |candidate| candidate == target,
            |_, item| item == ItemId::new(1),
        )
        .expect("already focused activation must allocate");
    let ActivationStartOutcome::PaneFocusReady { intent } = activation.outcome() else {
        panic!("already focused activation must publish an intent");
    };
    let mut after_effects = before_effects.clone();
    let effect = after_effects
        .request(PlatformEffect::RequestFocus {
            binding: target,
            after: None,
        })
        .expect("focus effect must allocate");
    let emitted = after_effects
        .take_new_requests(provider, InventoryGeneration::new(1), |_| false)
        .expect("focus effect must have valid causal predecessors");
    assert_eq!(emitted.len(), 1);
    assert_eq!(emitted[0].id(), effect);

    let installed = FocusDelta::between(&before, &after, &before_effects, &after_effects, &[]);
    assert_installed_focus_delta(&installed, intent, effect);
    after.mark_boundary_published();

    let before_settlement = after.clone();
    let before_settlement_effects = after_effects.clone();
    assert!(matches!(
        after.publish_pane_focus_observation(
            PaneFocusObservation::new(
                PaneFocusObservationGeneration::new(2),
                target,
                PanelFocus::Item(ItemId::new(1)),
            )
            .acknowledging(intent.id()),
            |candidate| candidate == target,
            |_, item| item == ItemId::new(1),
        ),
        PaneFocusObservationTransition::Applied {
            cleared_intent: Some(cleared)
        } if cleared == intent.id()
    ));
    assert_eq!(
        after_effects.mark_observed_applied(provider, effect, target, InventoryGeneration::new(2),),
        crate::platform::effect::EffectTransition::Applied
    );
    let observed = ObservedPlatformFocusEffect {
        effect,
        binding: target,
        generation: FocusObservationGeneration::new(2),
        evidence: PlatformFocusEvidence::ExactEffectAcknowledgement,
    };
    let settled = FocusDelta::between(
        &before_settlement,
        &after,
        &before_settlement_effects,
        &after_effects,
        &[observed],
    );
    assert_settled_focus_delta(&settled, intent, observed, InventoryGeneration::new(2));

    let before_cleanup = after.clone();
    assert!(after.clear_surface(target.surface()).changed());
    let cleanup = FocusDelta::between(&before_cleanup, &after, &after_effects, &after_effects, &[]);
    assert_eq!(cleanup.surface_focus().len(), 1);
    assert!(cleanup.surface_focus()[0].state().after().is_none());
}

#[test]
fn focus_delta_collapses_drop_then_close_to_the_final_pane_intent() {
    let target = binding();
    let before = ViewportFocusCoordinator::default();
    let effects = EffectLedger::default();
    let mut after = before.clone();
    let _ = after
        .publish_focus_observation(
            focus_observation(1, GlobalFocusedWindow::Dock(target.window())),
            focus_stamp(1),
            |candidate| candidate == target,
            |_, item| item == ItemId::new(1) || item == ItemId::new(2),
        )
        .expect("focus observation must apply");

    let drop = after
        .request_activation(
            ViewportActivationRequest::drop_committed(
                target,
                PaneFocusDisposition::Set(ItemId::new(1)),
            ),
            focus_stamp(2),
            true,
            |candidate| candidate == target,
            |_, item| item == ItemId::new(1) || item == ItemId::new(2),
        )
        .expect("drop activation must allocate");
    let ActivationStartOutcome::PaneFocusReady { intent: drop } = drop.outcome() else {
        panic!("focused drop target must publish a pane intent");
    };
    assert_eq!(drop.cause(), Some(ViewportActivationCause::DropCommitted));

    let close_request = close_request();
    let close = after
        .request_activation(
            ViewportActivationRequest::close_recovery(
                target,
                PaneFocusDisposition::Set(ItemId::new(2)),
                close_request,
            ),
            focus_stamp(3),
            true,
            |candidate| candidate == target,
            |_, item| item == ItemId::new(1) || item == ItemId::new(2),
        )
        .expect("close recovery must allocate");
    let ActivationStartOutcome::PaneFocusReady { intent: close } = close.outcome() else {
        panic!("focused close target must publish a pane intent");
    };
    assert_ne!(drop.id(), close.id());
    assert_eq!(close.source(), PaneFocusIntentSource::CloseRecovery);
    assert_eq!(
        close.cause(),
        Some(ViewportActivationCause::CloseRecovery {
            request: close_request
        })
    );

    let delta = FocusDelta::between(&before, &after, &effects, &effects, &[]);
    let pane = delta
        .pane_intent()
        .expect("the boundary must publish its final pane intent");
    assert_eq!(pane.before(), &None);
    assert_eq!(pane.after(), &Some(close));
    assert_ne!(pane.after(), &Some(drop));
}

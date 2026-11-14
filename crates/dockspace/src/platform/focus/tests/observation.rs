//! Observation behavior regressions.

use super::*;

#[test]
fn focus_observation_stream_requires_causal_tombstones() {
    let mut stream = FocusObservationStream::default();
    let initial = focus_observation(4, GlobalFocusedWindow::Dock(binding().window()));
    stream.observe(Some(initial));
    assert_eq!(stream.current(), Some(initial));

    stream.observe(Some(focus_observation(3, GlobalFocusedWindow::Foreign)));
    assert_eq!(stream.current(), Some(initial), "older facts are inert");
    stream.observe(Some(initial));
    assert_eq!(stream.current(), Some(initial), "duplicates are inert");

    stream.observe(Some(focus_observation(4, GlobalFocusedWindow::Foreign)));
    assert_eq!(
        stream.current(),
        None,
        "same-generation conflicts revoke focus authority"
    );
    stream.observe(Some(focus_observation(5, GlobalFocusedWindow::None)));
    assert_eq!(
        stream.current(),
        None,
        "known focus cannot bridge a same-generation conflict"
    );
    stream.observe(Some(focus_tombstone(6)));
    assert_eq!(stream.current(), None);

    let restored = focus_observation(7, GlobalFocusedWindow::Foreign);
    stream.observe(Some(restored));
    assert_eq!(stream.current(), Some(restored));
    assert_eq!(
        stream.generation_watermark(),
        Some(FocusObservationGeneration::new(7))
    );
}

#[test]
fn focus_observation_stream_quarantines_missing_and_skipped_generations() {
    let mut stream = FocusObservationStream::default();
    stream.observe(Some(focus_observation(
        1,
        GlobalFocusedWindow::Dock(binding().window()),
    )));

    stream.observe(None);
    stream.observe(Some(focus_observation(2, GlobalFocusedWindow::Foreign)));
    assert_eq!(
        stream.current(),
        None,
        "known focus cannot bridge an unversioned gap"
    );
    stream.observe(Some(focus_tombstone(3)));
    let after_missing = focus_observation(4, GlobalFocusedWindow::Foreign);
    stream.observe(Some(after_missing));
    assert_eq!(stream.current(), Some(after_missing));

    stream.observe(Some(focus_observation(6, GlobalFocusedWindow::None)));
    assert_eq!(
        stream.current(),
        None,
        "a skipped generation revokes focus authority"
    );
    stream.observe(Some(focus_observation(7, GlobalFocusedWindow::Foreign)));
    assert_eq!(
        stream.current(),
        None,
        "known focus cannot bridge a numeric generation gap"
    );
    stream.observe(Some(focus_tombstone(8)));
    let restored = focus_observation(9, GlobalFocusedWindow::Dock(second_binding().window()));
    stream.observe(Some(restored));
    assert_eq!(stream.current(), Some(restored));

    stream.observe(Some(focus_tombstone(11)));
    stream.observe(Some(focus_observation(12, GlobalFocusedWindow::None)));
    assert_eq!(
        stream.current(),
        None,
        "a skipped unknown envelope is not the required next-generation tombstone"
    );
    stream.observe(Some(focus_tombstone(13)));
    let post_gap = focus_observation(14, GlobalFocusedWindow::None);
    stream.observe(Some(post_gap));
    assert_eq!(stream.current(), Some(post_gap));
}

#[test]
fn focus_observation_stream_provider_replacement_resets_its_namespace() {
    let mut stream = FocusObservationStream::default();
    stream.observe(Some(focus_observation(
        40,
        GlobalFocusedWindow::Dock(binding().window()),
    )));
    stream.observe(Some(focus_observation(40, GlobalFocusedWindow::Foreign)));
    assert_eq!(stream.current(), None);

    stream.reset_for_provider_replacement();
    assert_eq!(stream.current(), None);
    assert_eq!(stream.generation_watermark(), None);

    let replacement_baseline =
        focus_observation(1, GlobalFocusedWindow::Dock(second_binding().window()));
    stream.observe(Some(replacement_baseline));
    assert_eq!(stream.current(), Some(replacement_baseline));

    stream.observe(Some(FocusObservationEnvelope::new(
        FocusObservationGeneration::new(2),
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
        Authority::Known(None),
    )));
    assert_eq!(
        stream.current(),
        None,
        "unknown focus must not be synthesized into a known no-focus fact"
    );
    let explicit_none = focus_observation(3, GlobalFocusedWindow::None);
    stream.observe(Some(explicit_none));
    assert_eq!(stream.current(), Some(explicit_none));
}

#[test]
fn coordinator_quarantines_known_focus_after_equal_generation_conflict() {
    let target = binding();
    let replacement = second_binding();
    let mut coordinator = ViewportFocusCoordinator::default();
    assert!(matches!(
        publish_platform_focus(
            &mut coordinator,
            focus_observation(10, GlobalFocusedWindow::Dock(target.window())),
            1,
        ),
        FocusObservationTransition::Applied(_)
    ));

    let activation = coordinator
        .request_activation(
            ViewportActivationRequest::explicit(replacement, PanelFocus::Item(ItemId::new(2))),
            focus_stamp(2),
            true,
            |candidate| candidate == target || candidate == replacement,
            |surface, item| surface == replacement.surface() && item == ItemId::new(2),
        )
        .expect("activation identity must remain available");
    assert!(matches!(
        activation.outcome(),
        ActivationStartOutcome::RequestPlatformFocus { .. }
    ));

    let conflict = publish_platform_focus(
        &mut coordinator,
        focus_observation(10, GlobalFocusedWindow::Dock(replacement.window())),
        3,
    );
    let FocusObservationTransition::AuthorityRevoked {
        generation,
        reason,
        cleanup,
        ..
    } = conflict
    else {
        panic!("an equal-generation conflict must revoke focus authority");
    };
    assert_eq!(generation, Some(FocusObservationGeneration::new(10)));
    assert_eq!(reason, FocusAuthorityRevocation::EqualGenerationConflict);
    assert!(cleanup.global_authority_removed());
    assert_eq!(
        cleanup.pending_activation_cancelled(),
        Some((
            activation.generation(),
            ActivationCancellation::PlatformAuthorityRevoked,
        ))
    );
    assert_eq!(coordinator.focus_observation(), None);

    assert!(matches!(
        publish_platform_focus(
            &mut coordinator,
            focus_observation(11, GlobalFocusedWindow::Dock(replacement.window())),
            4,
        ),
        FocusObservationTransition::TombstoneRequired {
            generation,
            ..
        } if generation == FocusObservationGeneration::new(11)
    ));
    assert_eq!(coordinator.focus_observation(), None);

    assert!(matches!(
        publish_platform_focus(&mut coordinator, focus_tombstone(12), 5,),
        FocusObservationTransition::AuthorityRevoked {
            reason: FocusAuthorityRevocation::Unknown(AuthorityUnavailableReason::NotReported),
            ..
        }
    ));
    let restored = focus_observation(13, GlobalFocusedWindow::Dock(replacement.window()));
    assert!(matches!(
        publish_platform_focus(&mut coordinator, restored, 6,),
        FocusObservationTransition::Applied(_)
    ));
    assert_eq!(coordinator.focus_observation(), Some(restored));
}

#[test]
fn coordinator_revokes_focus_for_generation_gaps_and_unknown_tombstones() {
    let mut coordinator = ViewportFocusCoordinator::default();
    let initial = focus_observation(20, GlobalFocusedWindow::Dock(binding().window()));
    let _ = publish_platform_focus(&mut coordinator, initial, 1);

    let gap = publish_platform_focus(
        &mut coordinator,
        focus_observation(22, GlobalFocusedWindow::Foreign),
        2,
    );
    assert!(matches!(
        gap,
        FocusObservationTransition::AuthorityRevoked {
            generation: Some(generation),
            reason: FocusAuthorityRevocation::GenerationGap { previous },
            cleanup,
            ..
        } if generation == FocusObservationGeneration::new(22)
            && previous == FocusObservationGeneration::new(20)
            && cleanup.global_authority_removed()
    ));
    assert_eq!(coordinator.focus_observation(), None);

    let _ = publish_platform_focus(&mut coordinator, focus_tombstone(23), 3);
    let restored = focus_observation(24, GlobalFocusedWindow::Foreign);
    let _ = publish_platform_focus(&mut coordinator, restored, 4);
    assert_eq!(coordinator.focus_observation(), Some(restored));

    let unknown = publish_platform_focus(&mut coordinator, focus_tombstone(25), 5);
    assert!(matches!(
        unknown,
        FocusObservationTransition::AuthorityRevoked {
            generation: Some(generation),
            reason: FocusAuthorityRevocation::Unknown(
                AuthorityUnavailableReason::NotReported
            ),
            cleanup,
            ..
        } if generation == FocusObservationGeneration::new(25)
            && cleanup.global_authority_removed()
    ));
    assert_eq!(
        coordinator.focus_observation(),
        None,
        "unknown focus must not be synthesized into GlobalFocusedWindow::None"
    );
}

#[test]
fn retired_duplicate_preserves_raw_focus_and_accepts_the_next_known_focus() {
    for successor in [
        GlobalFocusedWindow::Dock(second_binding().window()),
        GlobalFocusedWindow::Foreign,
    ] {
        let mut coordinator = ViewportFocusCoordinator::default();
        let raw = focus_observation(10, GlobalFocusedWindow::Dock(binding().window()));
        assert!(matches!(
            publish_platform_focus(&mut coordinator, raw, 1,),
            FocusObservationTransition::Applied(_)
        ));

        for (causal, had_authority) in [(2, true), (3, false)] {
            let transition = coordinator
                .publish_platform_focus_observation_with_retired_binding(
                    raw,
                    focus_stamp(causal),
                    true,
                    |candidate| candidate == second_binding(),
                    |candidate| candidate == second_binding(),
                    |_| false,
                    |_, _| false,
                )
                .expect("retirement must revoke only effective focus authority");
            assert!(matches!(
                transition,
                FocusObservationTransition::AuthorityRevoked {
                    generation: Some(generation),
                    reason: FocusAuthorityRevocation::Unknown(
                        AuthorityUnavailableReason::NotReported
                    ),
                    cleanup,
                    ..
                } if generation == raw.generation()
                    && cleanup.global_authority_removed() == had_authority
            ));
            assert_eq!(coordinator.focus_observation(), None);
            assert_eq!(coordinator.focus_observations.last_envelope, Some(raw));
            assert_eq!(
                coordinator.focus_observations.generation_watermark(),
                Some(raw.generation())
            );
            assert!(!coordinator.focus_observations.requires_tombstone);
        }

        let restored = focus_observation(11, successor);
        assert!(matches!(
            publish_platform_focus(&mut coordinator, restored, 4,),
            FocusObservationTransition::Applied(_)
        ));
        assert_eq!(coordinator.focus_observation(), Some(restored));
    }
}

#[test]
fn stale_retired_focus_does_not_revoke_a_newer_root_observation() {
    let mut coordinator = ViewportFocusCoordinator::default();
    let retired = focus_observation(10, GlobalFocusedWindow::Dock(binding().window()));
    let current = focus_observation(11, GlobalFocusedWindow::Dock(second_binding().window()));
    for (causal, observation) in [(1, retired), (2, current)] {
        assert!(matches!(
            publish_platform_focus(&mut coordinator, observation, causal,),
            FocusObservationTransition::Applied(_)
        ));
    }

    let transition = coordinator
        .publish_platform_focus_observation_with_retired_binding(
            retired,
            focus_stamp(3),
            true,
            |candidate| candidate == second_binding(),
            |candidate| candidate == second_binding(),
            |_| false,
            |_, _| false,
        )
        .expect("stale retirement observations must remain inert");
    assert!(matches!(
        transition,
        FocusObservationTransition::Stale { current: generation }
            if generation == current.generation()
    ));
    assert_eq!(coordinator.focus_observation(), Some(current));
    assert_eq!(coordinator.focus_observations.last_envelope, Some(current));
    assert_eq!(
        coordinator.focus_observations.generation_watermark(),
        Some(current.generation())
    );
    assert!(!coordinator.focus_observations.requires_tombstone);
}

#[test]
fn retired_focus_conflicts_and_gaps_still_require_a_provider_tombstone() {
    for generation in [10, 12] {
        let mut coordinator = ViewportFocusCoordinator::default();
        let initial = focus_observation(10, GlobalFocusedWindow::Dock(binding().window()));
        let _ = publish_platform_focus(&mut coordinator, initial, 1);
        let raw = focus_observation(
            generation,
            GlobalFocusedWindow::Dock(third_binding().window()),
        );
        let transition = coordinator
            .publish_platform_focus_observation_with_retired_binding(
                raw,
                focus_stamp(2),
                true,
                |candidate| candidate == second_binding(),
                |candidate| candidate == second_binding(),
                |_| false,
                |_, _| false,
            )
            .expect("retirement must preserve raw provider stream faults");
        let expected_reason = if generation == 10 {
            FocusAuthorityRevocation::EqualGenerationConflict
        } else {
            FocusAuthorityRevocation::GenerationGap {
                previous: initial.generation(),
            }
        };
        assert!(matches!(
            transition,
            FocusObservationTransition::AuthorityRevoked { reason, .. }
                if reason == expected_reason
        ));
        assert_eq!(coordinator.focus_observation(), None);
        assert_eq!(
            coordinator.focus_observations.last_envelope,
            Some(if generation == 10 { initial } else { raw })
        );
        assert!(coordinator.focus_observations.requires_tombstone);

        let blocked = focus_observation(
            generation + 1,
            GlobalFocusedWindow::Dock(third_binding().window()),
        );
        assert!(matches!(
            coordinator
                .publish_platform_focus_observation_with_retired_binding(
                    blocked,
                    focus_stamp(3),
                    true,
                    |candidate| candidate == second_binding(),
                    |candidate| candidate == second_binding(),
                    |_| false,
                    |_, _| false,
                )
                .expect("retirement cannot substitute for a provider tombstone"),
            FocusObservationTransition::TombstoneRequired { .. }
        ));
        assert!(coordinator.focus_observations.requires_tombstone);

        let _ = publish_platform_focus(&mut coordinator, focus_tombstone(generation + 2), 4);
        assert!(!coordinator.focus_observations.requires_tombstone);
        let restored = focus_observation(generation + 3, GlobalFocusedWindow::Foreign);
        assert!(matches!(
            publish_platform_focus(&mut coordinator, restored, 5,),
            FocusObservationTransition::Applied(_)
        ));
        assert_eq!(coordinator.focus_observation(), Some(restored));
    }
}

#[test]
fn focus_stream_faults_preserve_native_activation_reservations() {
    let owner = NativeCreateSagaId::new(71);
    let request = ViewportActivationRequest::tear_off_committed(
        second_binding(),
        PaneFocusDisposition::Set(ItemId::new(2)),
    );
    let reservation = (owner, focus_stamp(2), request);
    let mut coordinator = ViewportFocusCoordinator::default();
    let _ = publish_platform_focus(
        &mut coordinator,
        focus_observation(20, GlobalFocusedWindow::Dock(binding().window())),
        1,
    );
    assert!(coordinator.reserve_activation_causal(owner, focus_stamp(2), request));

    let gap = publish_platform_focus(
        &mut coordinator,
        focus_observation(22, GlobalFocusedWindow::Foreign),
        3,
    );
    assert!(matches!(
        gap,
        FocusObservationTransition::AuthorityRevoked {
            reason: FocusAuthorityRevocation::GenerationGap { .. },
            ..
        }
    ));
    assert_eq!(
        coordinator.winning_activation_reservation(),
        Some(reservation),
        "a provider stream fault must not revoke a core-owned native reservation"
    );

    let tombstone_required = publish_platform_focus(
        &mut coordinator,
        focus_observation(23, GlobalFocusedWindow::None),
        4,
    );
    assert!(matches!(
        tombstone_required,
        FocusObservationTransition::TombstoneRequired { .. }
    ));
    assert_eq!(
        coordinator.winning_activation_reservation(),
        Some(reservation)
    );

    let tombstone = publish_platform_focus(&mut coordinator, focus_tombstone(24), 5);
    assert!(matches!(
        tombstone,
        FocusObservationTransition::AuthorityRevoked {
            reason: FocusAuthorityRevocation::Unknown(_),
            ..
        }
    ));
    assert_eq!(
        coordinator.winning_activation_reservation(),
        Some(reservation)
    );

    let baseline = publish_platform_focus(
        &mut coordinator,
        focus_observation(25, GlobalFocusedWindow::None),
        6,
    );
    assert!(matches!(baseline, FocusObservationTransition::Applied(_)));

    let activation = coordinator
        .request_reserved_activation(
            owner,
            request,
            focus_stamp(2),
            true,
            |candidate| candidate == binding() || candidate == second_binding(),
            |surface, item| surface == second_binding().surface() && item == ItemId::new(2),
        )
        .expect("the exact native reservation must remain replayable");
    assert_eq!(
        activation.outcome(),
        ActivationStartOutcome::RequestPlatformFocus {
            target: second_binding(),
        }
    );
}

#[test]
fn equal_generation_focus_conflicts_preserve_native_activation_reservations() {
    let owner = NativeCreateSagaId::new(72);
    let request = ViewportActivationRequest::tear_off_committed(
        second_binding(),
        PaneFocusDisposition::Set(ItemId::new(2)),
    );
    let mut coordinator = ViewportFocusCoordinator::default();
    let _ = publish_platform_focus(
        &mut coordinator,
        focus_observation(30, GlobalFocusedWindow::Dock(binding().window())),
        1,
    );
    assert!(coordinator.reserve_activation_causal(owner, focus_stamp(2), request));

    let conflict = publish_platform_focus(
        &mut coordinator,
        focus_observation(30, GlobalFocusedWindow::Foreign),
        3,
    );
    assert!(matches!(
        conflict,
        FocusObservationTransition::AuthorityRevoked {
            reason: FocusAuthorityRevocation::EqualGenerationConflict,
            ..
        }
    ));
    assert_eq!(
        coordinator.winning_activation_reservation(),
        Some((owner, focus_stamp(2), request))
    );
}

#[test]
fn provider_replacement_revokes_native_activation_reservations() {
    let owner = NativeCreateSagaId::new(73);
    let request = ViewportActivationRequest::tear_off_committed(
        second_binding(),
        PaneFocusDisposition::Set(ItemId::new(2)),
    );
    let mut coordinator = ViewportFocusCoordinator::default();
    let _ = publish_platform_focus(
        &mut coordinator,
        focus_observation(40, GlobalFocusedWindow::Dock(binding().window())),
        1,
    );
    assert!(coordinator.reserve_activation_causal(owner, focus_stamp(2), request));

    let cleanup = coordinator.revoke_platform_provider_authority();

    assert!(cleanup.global_authority_removed());
    assert!(coordinator.activation_reservations().is_empty());
    assert_eq!(coordinator.winning_activation_reservation(), None);
}

#[test]
fn provider_revocation_preserves_pane_history_and_resets_focus_namespace() {
    let target = binding();
    let replacement = second_binding();
    let mut coordinator = ViewportFocusCoordinator::default();
    let _ = publish_platform_focus(
        &mut coordinator,
        focus_observation(40, GlobalFocusedWindow::Dock(target.window())),
        1,
    );
    assert!(matches!(
        coordinator.publish_pane_focus_observation(
            PaneFocusObservation::new(
                PaneFocusObservationGeneration::new(1),
                target,
                PanelFocus::Item(ItemId::new(1)),
            ),
            |candidate| candidate == target || candidate == replacement,
            |surface, item| surface == target.surface() && item == ItemId::new(1),
        ),
        PaneFocusObservationTransition::Applied { .. }
    ));

    let pane_activation = coordinator
        .request_activation(
            ViewportActivationRequest::explicit(target, PanelFocus::Item(ItemId::new(1))),
            focus_stamp(2),
            true,
            |candidate| candidate == target || candidate == replacement,
            |surface, item| surface == target.surface() && item == ItemId::new(1),
        )
        .expect("pane activation must remain available");
    let ActivationStartOutcome::PaneFocusReady { .. } = pane_activation.outcome() else {
        panic!("an already-focused target must install a pane intent");
    };
    let observe_only = coordinator
        .request_activation(
            ViewportActivationRequest::pointer_tab_gesture(
                replacement,
                PanelFocus::Item(ItemId::new(2)),
            ),
            focus_stamp(3),
            true,
            |candidate| candidate == target || candidate == replacement,
            |surface, item| surface == replacement.surface() && item == ItemId::new(2),
        )
        .expect("observe-only activation must remain available");
    let ActivationStartOutcome::ObserveOnlyRecorded { record } = observe_only.outcome() else {
        panic!("an unfocused pointer target must wait for provider focus");
    };
    assert!(
        coordinator.pending_pane_intent().is_none(),
        "a newer causal claim immediately supersedes an unacknowledged older pane intent"
    );

    let cleanup = coordinator.revoke_platform_provider_authority();
    assert!(cleanup.global_authority_removed());
    assert_eq!(cleanup.pending_intent_removed(), None);
    assert_eq!(
        cleanup.observe_only_activation_removed(),
        Some(record.generation())
    );
    assert_eq!(coordinator.focus_observation(), None);
    assert_eq!(
        coordinator.panel_focus(target.surface()),
        PanelFocusRecord::Item(ItemId::new(1))
    );
    assert!(
        coordinator
            .surface_focus_state(target.surface())
            .and_then(SurfaceFocusState::observation)
            .is_some(),
        "provider replacement must retain the pane observation history"
    );

    let replacement_baseline =
        focus_observation(1, GlobalFocusedWindow::Dock(replacement.window()));
    assert!(matches!(
        publish_platform_focus(&mut coordinator, replacement_baseline, 4,),
        FocusObservationTransition::Applied(_)
    ));
    assert_eq!(coordinator.focus_observation(), Some(replacement_baseline));

    let pending = coordinator
        .request_activation(
            ViewportActivationRequest::explicit(target, PanelFocus::Item(ItemId::new(1))),
            focus_stamp(5),
            true,
            |candidate| candidate == target || candidate == replacement,
            |surface, item| surface == target.surface() && item == ItemId::new(1),
        )
        .expect("pending activation must remain available");
    let cleanup = coordinator.revoke_platform_provider_authority();
    assert_eq!(
        cleanup.pending_activation_cancelled(),
        Some((
            pending.generation(),
            ActivationCancellation::PlatformAuthorityRevoked,
        ))
    );
    assert_eq!(
        coordinator.panel_focus(target.surface()),
        PanelFocusRecord::Item(ItemId::new(1))
    );
}

#[test]
fn physical_window_focus_never_restores_a_surface_history() {
    let first = binding();
    let second = second_binding();
    let mut coordinator = ViewportFocusCoordinator::default();
    coordinator
        .panel_focus_by_surface
        .insert(first.surface(), PanelFocusRecord::Item(ItemId::new(1)));
    coordinator
        .panel_focus_by_surface
        .insert(second.surface(), PanelFocusRecord::Item(ItemId::new(2)));
    for (index, focused) in [
        GlobalFocusedWindow::Dock(first.window()),
        GlobalFocusedWindow::Dock(second.window()),
        GlobalFocusedWindow::Foreign,
        GlobalFocusedWindow::Dock(first.window()),
    ]
    .into_iter()
    .enumerate()
    {
        let transition = publish_platform_focus(
            &mut coordinator,
            focus_observation(index as u64 + 1, focused),
            index as u64 + 1,
        );
        let FocusObservationTransition::Applied(applied) = transition else {
            panic!("each next focus fact must be accepted");
        };
        assert!(applied.pane_intent().is_none());
        assert!(coordinator.pending_pane_intent().is_none());
        assert_eq!(coordinator.winning_focus_target(), None);
        assert_eq!(
            coordinator.current_focus(),
            Some(match focused {
                GlobalFocusedWindow::Foreign => CurrentUiFocus::External,
                GlobalFocusedWindow::Dock(_) => CurrentUiFocus::Unknown,
                GlobalFocusedWindow::None => CurrentUiFocus::None,
            })
        );
    }
    assert_eq!(
        coordinator.panel_focus(first.surface()),
        PanelFocusRecord::Item(ItemId::new(1))
    );
    assert_eq!(
        coordinator.panel_focus(second.surface()),
        PanelFocusRecord::Item(ItemId::new(2))
    );
}

#[test]
fn shared_window_focus_preserves_an_exact_pane_fact_without_selecting_a_sibling() {
    let first = binding();
    let second = shared_window_sibling();
    let is_current = |binding| binding == first || binding == second;
    let item_is_current = |surface, item| {
        (surface == first.surface() && item == ItemId::new(1))
            || (surface == second.surface() && item == ItemId::new(2))
    };
    let mut coordinator = ViewportFocusCoordinator::default();
    coordinator
        .publish_focus_observation(
            focus_observation(1, GlobalFocusedWindow::Dock(first.window())),
            focus_stamp(1),
            is_current,
            item_is_current,
        )
        .expect("the physical window is observable");
    assert_eq!(coordinator.current_focus(), Some(CurrentUiFocus::Unknown));
    let observed = coordinator.publish_pane_focus_observation(
        PaneFocusObservation::new(
            PaneFocusObservationGeneration::new(1),
            second,
            PanelFocus::Item(ItemId::new(2)),
        ),
        is_current,
        item_is_current,
    );
    assert!(matches!(
        observed,
        PaneFocusObservationTransition::Applied { .. }
    ));
    assert_eq!(
        coordinator.current_pane_focus(second.surface()),
        Some(ItemId::new(2))
    );
    coordinator
        .publish_focus_observation(
            focus_observation(2, GlobalFocusedWindow::Dock(first.window())),
            focus_stamp(2),
            is_current,
            item_is_current,
        )
        .expect("the next fact describes the same physical window");
    assert_eq!(
        coordinator.current_pane_focus(second.surface()),
        Some(ItemId::new(2))
    );
    assert_eq!(coordinator.current_pane_focus(first.surface()), None);
    assert!(coordinator.pending_pane_intent().is_none());
    assert_eq!(
        coordinator.workspace_replacement_focus(),
        WorkspaceReplacementFocus::Dock {
            surface: second.surface(),
            item: Some(ItemId::new(2))
        }
    );
    coordinator.clear_binding(first);
    assert_eq!(
        coordinator.current_pane_focus(second.surface()),
        Some(ItemId::new(2))
    );
    assert!(matches!(
        coordinator.focus_observation().map(|observation| *observation.focused()),
        Some(Authority::Known(GlobalFocusedWindow::Dock(window))) if window == first.window()
    ));
}

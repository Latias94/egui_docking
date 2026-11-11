//! Activation behavior regressions.

use super::*;

#[test]
fn activation_generation_exhaustion_is_atomic() {
    let mut coordinator = ViewportFocusCoordinator::default();
    coordinator.exhaust_activation_generations();
    let before = coordinator.clone();
    assert_eq!(
        coordinator.request_activation(
            ViewportActivationRequest::explicit(binding(), PanelFocus::None),
            focus_stamp(1),
            true,
            |_| true,
            |_, _| true,
        ),
        Err(ViewportFocusError::ActivationGenerationExhausted)
    );
    assert_eq!(coordinator, before);
}

#[test]
fn shared_window_activation_keeps_its_exact_pane_target_without_an_os_focus_effect() {
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
    let start = coordinator
        .request_activation(
            ViewportActivationRequest::explicit(second, PanelFocus::Item(ItemId::new(2))),
            focus_stamp(2),
            false,
            is_current,
            item_is_current,
        )
        .expect("a sibling in the focused window needs no native activation control");
    let ActivationStartOutcome::PaneFocusReady { intent } = start.outcome() else {
        panic!("the exact semantic target must receive a pane request immediately");
    };
    assert_eq!(intent.native_guard(), Some(second));
    assert_eq!(intent.surface(), second.surface());
    assert!(coordinator.pending_activation().is_none());
    assert_eq!(coordinator.current_pane_focus(second.surface()), None);
    coordinator.mark_boundary_published();
    let sibling = coordinator.publish_pane_focus_observation(
        PaneFocusObservation::new(
            PaneFocusObservationGeneration::new(1),
            first,
            PanelFocus::Item(ItemId::new(1)),
        )
        .acknowledging(intent.id()),
        is_current,
        item_is_current,
    );
    assert!(matches!(
        sibling,
        PaneFocusObservationTransition::Rejected(
            PaneFocusObservationRejection::IntentBindingMismatch
        )
    ));
    assert_eq!(coordinator.pending_pane_intent(), Some(intent));
    let exact = coordinator.publish_pane_focus_observation(
        PaneFocusObservation::new(
            PaneFocusObservationGeneration::new(1),
            second,
            PanelFocus::Item(ItemId::new(2)),
        )
        .acknowledging(intent.id()),
        is_current,
        item_is_current,
    );
    assert!(
        matches!(exact, PaneFocusObservationTransition::Applied { cleared_intent: Some(id), .. } if id == intent.id())
    );
    assert_eq!(
        coordinator.current_pane_focus(second.surface()),
        Some(ItemId::new(2))
    );
    assert!(coordinator.pending_pane_intent().is_none());
}

#[test]
fn unknown_focus_baseline_allows_activation_without_claiming_observed_focus() {
    let target = second_binding();
    let mut coordinator = ViewportFocusCoordinator::default();
    let baseline = focus_tombstone(1);
    publish_platform_focus(&mut coordinator, baseline, 1);
    let request = ViewportActivationRequest::explicit(target, PanelFocus::Item(ItemId::new(2)));
    let unsupported = coordinator
        .request_activation(
            request,
            focus_stamp(2),
            false,
            |candidate| candidate == target,
            |_, _| true,
        )
        .unwrap();
    assert_eq!(
        unsupported.outcome(),
        ActivationStartOutcome::Suppressed(ActivationSuppression::PlatformFocusControlUnavailable)
    );
    let start = coordinator
        .request_activation(
            request,
            focus_stamp(3),
            true,
            |candidate| candidate == target,
            |surface, item| surface == target.surface() && item == ItemId::new(2),
        )
        .unwrap();
    assert_eq!(
        start.outcome(),
        ActivationStartOutcome::RequestPlatformFocus { target }
    );
    assert_eq!(
        coordinator
            .pending_activation()
            .unwrap()
            .observation_baseline(),
        baseline.generation()
    );
    assert!(coordinator.focus_observation().is_none());
    assert_eq!(coordinator.current_focus(), Some(CurrentUiFocus::Unknown));
    assert!(coordinator.pending_pane_intent().is_none());
    assert_eq!(
        coordinator.attach_platform_focus_effect(start.generation(), EffectId::new(1)),
        FocusEffectAttachment::Applied
    );
    assert!(matches!(
        publish_platform_focus(&mut coordinator, baseline, 4,),
        FocusObservationTransition::Duplicate
    ));
    assert!(coordinator.pending_activation().is_some());
    assert!(coordinator.pending_pane_intent().is_none());

    let completed = publish_platform_focus(
        &mut coordinator,
        focus_observation(2, GlobalFocusedWindow::Dock(target.window())),
        5,
    );
    let FocusObservationTransition::Applied(completed) = completed else {
        panic!("a fresh exact native focus observation must complete activation");
    };
    assert_eq!(completed.completed_activation(), Some(start.generation()));
    assert_eq!(
        completed.pane_intent().and_then(PaneFocusIntent::item),
        Some(ItemId::new(2))
    );
}

#[test]
fn activation_without_a_valid_observation_stream_stays_suppressed() {
    let mut missing = FocusObservationStream::default();
    missing.observe(Some(focus_tombstone(1)));
    missing.observe(None);
    let mut conflict = FocusObservationStream::default();
    conflict.observe(Some(focus_tombstone(1)));
    conflict.observe(Some(focus_observation(1, GlobalFocusedWindow::None)));
    let mut gap = FocusObservationStream::default();
    gap.observe(Some(focus_tombstone(1)));
    gap.observe(Some(focus_tombstone(3)));
    let mut replacement = FocusObservationStream::default();
    replacement.observe(Some(focus_tombstone(1)));
    replacement.reset_for_provider_replacement();
    for stream in [
        FocusObservationStream::default(),
        missing,
        conflict,
        gap,
        replacement,
    ] {
        let mut coordinator = ViewportFocusCoordinator {
            focus_observations: stream,
            ..Default::default()
        };
        let start = coordinator
            .request_activation(
                ViewportActivationRequest::explicit(binding(), PanelFocus::None),
                focus_stamp(1),
                true,
                |_| true,
                |_, _| true,
            )
            .unwrap();
        assert_eq!(
            start.outcome(),
            ActivationStartOutcome::Suppressed(
                ActivationSuppression::FocusObservationBaselineUnavailable
            )
        );
        assert!(coordinator.pending_activation().is_none());
        assert!(coordinator.pending_pane_intent().is_none());
    }
}

#[test]
fn explicit_activation_completes_only_its_requested_pane() {
    let target = second_binding();
    let mut coordinator = ViewportFocusCoordinator::default();
    let _ = publish_platform_focus(
        &mut coordinator,
        focus_observation(1, GlobalFocusedWindow::Dock(binding().window())),
        1,
    );
    let start = coordinator
        .request_activation(
            ViewportActivationRequest::explicit(target, PanelFocus::Item(ItemId::new(2))),
            focus_stamp(2),
            true,
            |candidate| [binding(), target].contains(&candidate),
            |surface, item| surface == target.surface() && item == ItemId::new(2),
        )
        .expect("explicit activation must allocate");
    let effect = EffectId::new(1);
    assert_eq!(
        coordinator.attach_platform_focus_effect(start.generation(), effect),
        FocusEffectAttachment::Applied
    );

    let completed = publish_platform_focus(
        &mut coordinator,
        focus_observation(2, GlobalFocusedWindow::Dock(target.window())),
        3,
    );
    let FocusObservationTransition::Applied(completed) = completed else {
        panic!("explicit target focus must be accepted");
    };
    assert_eq!(completed.completed_activation(), Some(start.generation()));
    assert_eq!(
        completed.pane_intent().map(PaneFocusIntent::source),
        Some(PaneFocusIntentSource::ExplicitViewportActivation)
    );
}

#[test]
fn closing_binding_cancels_unissued_activation_but_settles_emitted_obligation() {
    let target = second_binding();
    let current = binding();
    let binding_is_live = |candidate| candidate == current || candidate == target;
    let item_is_live = |surface, item| {
        (surface == current.surface() && item == ItemId::new(1))
            || (surface == target.surface() && item == ItemId::new(2))
    };

    let mut unissued = ViewportFocusCoordinator::default();
    let _ = publish_platform_focus(
        &mut unissued,
        focus_observation(1, GlobalFocusedWindow::Dock(current.window())),
        1,
    );
    let start = unissued
        .request_activation(
            ViewportActivationRequest::explicit(target, PanelFocus::Item(ItemId::new(2))),
            focus_stamp(2),
            true,
            binding_is_live,
            item_is_live,
        )
        .expect("unissued activation must allocate");
    let cleanup = unissued.reconcile_authority(
        |_| true,
        binding_is_live,
        |candidate| candidate == current,
        item_is_live,
    );
    assert_eq!(
        cleanup.pending_activation_cancelled(),
        Some((start.generation(), ActivationCancellation::StaleBinding))
    );

    let mut emitted = ViewportFocusCoordinator::default();
    let _ = publish_platform_focus(
        &mut emitted,
        focus_observation(1, GlobalFocusedWindow::Dock(current.window())),
        1,
    );
    let start = emitted
        .request_activation(
            ViewportActivationRequest::explicit(target, PanelFocus::Item(ItemId::new(2))),
            focus_stamp(2),
            true,
            binding_is_live,
            item_is_live,
        )
        .expect("emitted activation must allocate");
    let effect = EffectId::new(88);
    assert_eq!(
        emitted.attach_platform_focus_effect(start.generation(), effect),
        FocusEffectAttachment::Applied
    );
    assert!(
        !emitted
            .reconcile_authority(
                |_| true,
                binding_is_live,
                |candidate| candidate == current,
                item_is_live,
            )
            .changed(),
        "an emitted request remains observable while its target is closing"
    );

    let settled = emitted
        .publish_platform_focus_observation(
            focus_observation(2, GlobalFocusedWindow::Dock(target.window())),
            focus_stamp(3),
            binding_is_live,
            |candidate| candidate == current,
            |_| false,
            item_is_live,
        )
        .expect("late target focus must settle the emitted request");
    let FocusObservationTransition::Applied(settled) = settled else {
        panic!("late target focus must apply");
    };
    let intent = settled
        .pane_intent()
        .expect("the original pane obligation must survive settlement");
    assert_eq!(intent.causal(), focus_stamp(2));
    assert!(
        !emitted
            .reconcile_authority(
                |_| true,
                binding_is_live,
                |candidate| candidate == current,
                item_is_live,
            )
            .changed(),
        "the pane continuation of an emitted activation remains acknowledgeable"
    );
}

#[test]
fn published_observe_only_obligation_accepts_a_late_closing_ack() {
    let target = second_binding();
    let current = binding();
    let binding_is_live = |candidate| candidate == current || candidate == target;
    let item_is_live = |surface, item| {
        (surface == current.surface() && item == ItemId::new(1))
            || (surface == target.surface() && item == ItemId::new(2))
    };
    let mut coordinator = ViewportFocusCoordinator::default();
    let _ = publish_platform_focus(
        &mut coordinator,
        focus_observation(1, GlobalFocusedWindow::Dock(current.window())),
        1,
    );
    let start = coordinator
        .request_activation(
            ViewportActivationRequest::pointer_tab_gesture(
                target,
                PanelFocus::Item(ItemId::new(2)),
            ),
            focus_stamp(2),
            true,
            binding_is_live,
            item_is_live,
        )
        .expect("observe-only activation must allocate");
    assert!(matches!(
        start.outcome(),
        ActivationStartOutcome::ObserveOnlyRecorded { .. }
    ));
    coordinator.mark_boundary_published();
    assert!(
        !coordinator
            .reconcile_authority(
                |_| true,
                binding_is_live,
                |candidate| candidate == current,
                item_is_live,
            )
            .changed()
    );

    let settled = coordinator
        .publish_platform_focus_observation(
            focus_observation(2, GlobalFocusedWindow::Dock(target.window())),
            focus_stamp(3),
            binding_is_live,
            |candidate| candidate == current,
            |_| false,
            item_is_live,
        )
        .expect("late pointer focus must settle");
    let FocusObservationTransition::Applied(settled) = settled else {
        panic!("late pointer focus must apply");
    };
    let intent = settled
        .pane_intent()
        .expect("late pointer focus must release the pane intent");
    assert_eq!(intent.causal(), focus_stamp(2));
    assert!(
        !coordinator
            .reconcile_authority(
                |_| true,
                binding_is_live,
                |candidate| candidate == current,
                item_is_live,
            )
            .changed()
    );
    assert_eq!(
        coordinator.publish_pane_focus_observation(
            PaneFocusObservation::new(
                PaneFocusObservationGeneration::new(1),
                target,
                PanelFocus::Item(ItemId::new(2)),
            )
            .acknowledging(intent.id()),
            binding_is_live,
            item_is_live,
        ),
        PaneFocusObservationTransition::Applied {
            cleared_intent: Some(intent.id())
        }
    );
}

#[test]
fn same_generation_explicit_activation_cannot_clear_higher_priority_close_recovery() {
    let target = binding();
    let mut coordinator = ViewportFocusCoordinator::default();
    let _ = publish_platform_focus(
        &mut coordinator,
        focus_observation(1, GlobalFocusedWindow::Dock(target.window())),
        1,
    );

    let recovery = coordinator
        .request_activation(
            ViewportActivationRequest::close_recovery(
                target,
                PaneFocusDisposition::Set(ItemId::new(1)),
                close_request(),
            ),
            focus_stamp(2),
            true,
            |candidate| candidate == target,
            |surface, item| surface == target.surface() && item == ItemId::new(1),
        )
        .expect("focused close recovery must allocate");
    let ActivationStartOutcome::PaneFocusReady { intent: recovery } = recovery.outcome() else {
        panic!("focused close recovery must install a pane intent");
    };

    let explicit = coordinator
        .request_activation(
            ViewportActivationRequest::explicit(target, PanelFocus::Item(ItemId::new(1))),
            focus_stamp(2),
            true,
            |candidate| candidate == target,
            |surface, item| surface == target.surface() && item == ItemId::new(1),
        )
        .expect("explicit activation must allocate its diagnostic identity");
    assert_eq!(
        explicit.outcome(),
        ActivationStartOutcome::Suppressed(ActivationSuppression::CausallySuperseded {
            current: PaneFocusIntentGeneration::new(2),
        }),
        "a lower-priority same-generation activation must lose arbitration"
    );
    assert_eq!(
        coordinator.pending_pane_intent(),
        Some(recovery),
        "a rejected activation must leave the existing recovery intent intact"
    );
}

#[test]
fn same_generation_close_recovery_supersedes_lower_priority_explicit_activation() {
    let target = binding();
    let mut coordinator = ViewportFocusCoordinator::default();
    let _ = publish_platform_focus(
        &mut coordinator,
        focus_observation(1, GlobalFocusedWindow::Dock(target.window())),
        1,
    );

    let explicit = coordinator
        .request_activation(
            ViewportActivationRequest::explicit(target, PanelFocus::Item(ItemId::new(1))),
            focus_stamp(2),
            true,
            |candidate| candidate == target,
            |surface, item| surface == target.surface() && item == ItemId::new(1),
        )
        .expect("focused explicit activation must allocate");
    let ActivationStartOutcome::PaneFocusReady { intent: explicit } = explicit.outcome() else {
        panic!("focused explicit activation must install a pane intent");
    };

    let recovery = coordinator
        .request_activation(
            ViewportActivationRequest::close_recovery(
                target,
                PaneFocusDisposition::Set(ItemId::new(1)),
                close_request(),
            ),
            focus_stamp(2),
            true,
            |candidate| candidate == target,
            |surface, item| surface == target.surface() && item == ItemId::new(1),
        )
        .expect("close recovery must allocate");
    let ActivationStartOutcome::PaneFocusReady { intent: recovery } = recovery.outcome() else {
        panic!("higher-priority same-generation recovery must install its pane intent");
    };
    assert_ne!(recovery.id(), explicit.id());
    assert_eq!(recovery.source(), PaneFocusIntentSource::CloseRecovery);
    assert_eq!(coordinator.pending_pane_intent(), Some(recovery));
}

#[test]
fn same_generation_high_priority_preserve_wins_in_both_orders() {
    let target = binding();
    let is_current = |candidate| candidate == target;
    let item_is_current = |surface, item| surface == target.surface() && item == ItemId::new(1);
    let preserve = || {
        ViewportActivationRequest::with_disposition(
            target,
            PaneFocusDisposition::Preserve,
            ViewportActivationCause::CloseRecovery {
                request: close_request(),
            },
        )
    };

    let mut lower_first = ViewportFocusCoordinator::default();
    let _ = publish_platform_focus(
        &mut lower_first,
        focus_observation(1, GlobalFocusedWindow::Dock(target.window())),
        1,
    );
    let lower = lower_first
        .request_activation(
            ViewportActivationRequest::explicit(target, PanelFocus::Item(ItemId::new(1))),
            focus_stamp(2),
            true,
            is_current,
            item_is_current,
        )
        .expect("lower-priority activation must allocate");
    let ActivationStartOutcome::PaneFocusReady { intent: lower } = lower.outcome() else {
        panic!("focused lower-priority activation must install an intent");
    };
    lower_first.mark_boundary_published();
    let higher = lower_first
        .request_activation(
            preserve(),
            focus_stamp(2),
            true,
            is_current,
            item_is_current,
        )
        .expect("higher-priority preserve must allocate");
    assert_eq!(
        higher.outcome(),
        ActivationStartOutcome::PaneFocusPreserved { target }
    );
    assert!(lower_first.pending_pane_intent().is_none());
    assert_eq!(
        lower_first.publish_pane_focus_observation(
            PaneFocusObservation::new(
                PaneFocusObservationGeneration::new(1),
                target,
                PanelFocus::Item(ItemId::new(1)),
            )
            .acknowledging(lower.id()),
            is_current,
            item_is_current,
        ),
        PaneFocusObservationTransition::Rejected(PaneFocusObservationRejection::UnknownIntent {
            intent: lower.id()
        })
    );

    let mut higher_first = ViewportFocusCoordinator::default();
    let _ = publish_platform_focus(
        &mut higher_first,
        focus_observation(1, GlobalFocusedWindow::Dock(target.window())),
        1,
    );
    let _ = higher_first
        .request_activation(
            preserve(),
            focus_stamp(2),
            true,
            is_current,
            item_is_current,
        )
        .expect("higher-priority preserve must allocate first");
    let lower = higher_first
        .request_activation(
            ViewportActivationRequest::explicit(target, PanelFocus::Item(ItemId::new(1))),
            focus_stamp(2),
            true,
            is_current,
            item_is_current,
        )
        .expect("lower-priority activation receives a diagnostic result");
    assert_eq!(
        lower.outcome(),
        ActivationStartOutcome::Suppressed(ActivationSuppression::CausallySuperseded {
            current: PaneFocusIntentGeneration::new(2),
        })
    );
    assert!(higher_first.pending_pane_intent().is_none());
}

#[test]
fn cancelling_a_newer_native_reservation_restores_the_older_owner() {
    let older = binding();
    let newer = second_binding();
    let older_owner = NativeCreateSagaId::new(41);
    let newer_owner = NativeCreateSagaId::new(42);
    let older_request = ViewportActivationRequest::tear_off_committed(
        older,
        PaneFocusDisposition::Set(ItemId::new(1)),
    );
    let newer_request = ViewportActivationRequest::tear_off_committed(
        newer,
        PaneFocusDisposition::Set(ItemId::new(2)),
    );
    let mut coordinator = ViewportFocusCoordinator::default();

    assert!(coordinator.reserve_activation_causal(older_owner, focus_stamp(2), older_request,));
    assert!(coordinator.reserve_activation_causal(newer_owner, focus_stamp(3), newer_request,));
    assert_eq!(coordinator.winning_focus_target(), Some(newer));
    assert_eq!(
        coordinator.winning_activation_reservation(),
        Some((newer_owner, focus_stamp(3), newer_request))
    );

    assert!(coordinator.cancel_activation_reservation(newer_owner));
    assert_eq!(coordinator.winning_focus_target(), Some(older));
    assert_eq!(
        coordinator.winning_activation_reservation(),
        Some((older_owner, focus_stamp(2), older_request))
    );
    assert!(!coordinator.cancel_activation_reservation(newer_owner));
}

#[test]
fn cancelling_an_older_native_reservation_cannot_roll_back_a_newer_owner() {
    let older = binding();
    let newer = second_binding();
    let older_owner = NativeCreateSagaId::new(43);
    let newer_owner = NativeCreateSagaId::new(44);
    let older_request = ViewportActivationRequest::tear_off_committed(
        older,
        PaneFocusDisposition::Set(ItemId::new(1)),
    );
    let newer_request = ViewportActivationRequest::tear_off_committed(
        newer,
        PaneFocusDisposition::Set(ItemId::new(2)),
    );
    let mut coordinator = ViewportFocusCoordinator::default();

    assert!(coordinator.reserve_activation_causal(older_owner, focus_stamp(2), older_request,));
    assert!(coordinator.reserve_activation_causal(newer_owner, focus_stamp(3), newer_request,));
    assert!(coordinator.cancel_activation_reservation(older_owner));
    assert_eq!(coordinator.winning_focus_target(), Some(newer));
    assert_eq!(
        coordinator.winning_activation_reservation(),
        Some((newer_owner, focus_stamp(3), newer_request))
    );
}

#[test]
fn native_activation_reservation_blocks_older_readiness_completion() {
    let older = binding();
    let newer = second_binding();
    let bindings_are_live = |candidate| candidate == older || candidate == newer;
    let items_are_live = |surface, item| {
        (surface == older.surface() && item == ItemId::new(1))
            || (surface == newer.surface() && item == ItemId::new(2))
    };
    let older_request = ViewportActivationRequest::tear_off_committed(
        older,
        PaneFocusDisposition::Set(ItemId::new(1)),
    );
    let newer_request = ViewportActivationRequest::tear_off_committed(
        newer,
        PaneFocusDisposition::Set(ItemId::new(2)),
    );
    let mut coordinator = ViewportFocusCoordinator::default();
    let _ = publish_platform_focus(
        &mut coordinator,
        focus_observation(1, GlobalFocusedWindow::Foreign),
        1,
    );

    assert!(coordinator.reserve_activation_causal(
        NativeCreateSagaId::new(1),
        focus_stamp(2),
        older_request,
    ));
    assert!(coordinator.reserve_activation_causal(
        NativeCreateSagaId::new(2),
        focus_stamp(3),
        newer_request,
    ));

    let late_older = coordinator
        .request_reserved_activation(
            NativeCreateSagaId::new(1),
            older_request,
            focus_stamp(2),
            true,
            bindings_are_live,
            items_are_live,
        )
        .expect("an older completion must produce a diagnostic result");
    assert_eq!(
        late_older.outcome(),
        ActivationStartOutcome::Suppressed(ActivationSuppression::CausallySuperseded {
            current: PaneFocusIntentGeneration::new(3),
        })
    );

    let current = coordinator
        .request_reserved_activation(
            NativeCreateSagaId::new(2),
            newer_request,
            focus_stamp(3),
            true,
            bindings_are_live,
            items_are_live,
        )
        .expect("the exact reserved completion must remain admissible");
    assert_eq!(
        current.outcome(),
        ActivationStartOutcome::RequestPlatformFocus { target: newer }
    );
}

#[test]
fn isolated_staging_focus_completes_the_exact_reserved_claim_after_admission() {
    let current = binding();
    let staging = second_binding();
    let request = ViewportActivationRequest::tear_off_committed(
        staging,
        PaneFocusDisposition::Set(ItemId::new(2)),
    );
    let mut coordinator = ViewportFocusCoordinator::default();
    let _ = publish_platform_focus(
        &mut coordinator,
        focus_observation(1, GlobalFocusedWindow::Dock(current.window())),
        1,
    );
    assert!(coordinator.reserve_activation_causal(
        NativeCreateSagaId::new(1),
        focus_stamp(2),
        request,
    ));

    let staging_focus = coordinator
        .publish_platform_focus_observation(
            focus_observation(2, GlobalFocusedWindow::Dock(staging.window())),
            focus_stamp(3),
            |candidate| candidate == current,
            |candidate| candidate == current,
            |candidate| candidate == staging.window(),
            |surface, item| surface == staging.surface() && item == ItemId::new(2),
        )
        .expect("a structurally exact staging focus fact must be retained");
    let FocusObservationTransition::Applied(staging_focus) = staging_focus else {
        panic!("staging focus must apply in its isolated lane");
    };
    assert!(staging_focus.pane_intent().is_none());

    let admitted = coordinator
        .request_reserved_activation(
            NativeCreateSagaId::new(1),
            request,
            focus_stamp(2),
            true,
            |candidate| candidate == current || candidate == staging,
            |surface, item| surface == staging.surface() && item == ItemId::new(2),
        )
        .expect("the admitted target must replay its release-time claim");
    let ActivationStartOutcome::PaneFocusReady { intent } = admitted.outcome() else {
        panic!("the already-focused admitted target must install its exact pane intent");
    };
    assert_eq!(intent.surface(), staging.surface());
    assert_eq!(intent.native_guard(), Some(staging));
    assert_eq!(intent.causal(), focus_stamp(2));
    assert_eq!(intent.focus(), PanelFocus::Item(ItemId::new(2)));
}

#[test]
fn lifecycle_compensation_reasserts_a_pointer_winner_after_old_staging_focus() {
    let older = binding();
    let winner = second_binding();
    let binding_is_live = |candidate| candidate == older || candidate == winner;
    let item_is_live = |surface, item| {
        (surface == older.surface() && item == ItemId::new(1))
            || (surface == winner.surface() && item == ItemId::new(2))
    };
    let mut coordinator = ViewportFocusCoordinator::default();
    let _ = publish_platform_focus(
        &mut coordinator,
        focus_observation(1, GlobalFocusedWindow::Dock(winner.window())),
        1,
    );
    let selected = coordinator
        .request_activation(
            ViewportActivationRequest::pointer_tab_gesture(
                winner,
                PanelFocus::Item(ItemId::new(2)),
            ),
            focus_stamp(2),
            true,
            binding_is_live,
            item_is_live,
        )
        .expect("the winning tab gesture must allocate");
    let ActivationStartOutcome::PaneFocusReady { intent } = selected.outcome() else {
        panic!("the focused winning tab must receive a pane intent");
    };
    coordinator.mark_boundary_published();
    let _ = coordinator.publish_pane_focus_observation(
        PaneFocusObservation::new(
            PaneFocusObservationGeneration::new(1),
            winner,
            PanelFocus::Item(ItemId::new(2)),
        )
        .acknowledging(intent.id()),
        binding_is_live,
        item_is_live,
    );

    let old_completion = coordinator
        .request_activation(
            ViewportActivationRequest::tear_off_committed(
                older,
                PaneFocusDisposition::Set(ItemId::new(1)),
            ),
            focus_stamp(1),
            true,
            binding_is_live,
            item_is_live,
        )
        .expect("old completion must return a causal diagnostic");
    assert!(matches!(
        old_completion.outcome(),
        ActivationStartOutcome::Suppressed(ActivationSuppression::CausallySuperseded { .. })
    ));
    let suppressed = coordinator.suppressed_tear_off_bindings();
    assert!(suppressed.contains(&older));

    let reasserted = coordinator
        .request_winning_lifecycle_compensation(true, binding_is_live, item_is_live)
        .expect("winner compensation must allocate")
        .expect("the latest focus claim names a docking target");
    assert_eq!(
        reasserted.outcome(),
        ActivationStartOutcome::RequestPlatformFocus { target: winner }
    );
    assert_eq!(
        coordinator
            .pending_activation()
            .map(PendingViewportActivation::request),
        Some(ViewportActivationRequest::pointer_tab_gesture(
            winner,
            PanelFocus::Item(ItemId::new(2)),
        ))
    );
    let effect = EffectId::new(41);
    assert_eq!(
        coordinator.attach_platform_focus_effect(reasserted.generation(), effect),
        FocusEffectAttachment::Applied
    );

    let late_auto_focus = coordinator
        .publish_platform_focus_observation(
            focus_observation(2, GlobalFocusedWindow::Dock(older.window())),
            focus_stamp(3),
            binding_is_live,
            binding_is_live,
            |candidate| {
                suppressed
                    .iter()
                    .any(|binding| binding.window() == candidate)
            },
            item_is_live,
        )
        .expect("the delayed auto-focus fact must remain isolated from pane arbitration");
    let FocusObservationTransition::Applied(late_auto_focus) = late_auto_focus else {
        panic!("the delayed auto-focus fact must advance the provider stream");
    };
    assert!(late_auto_focus.pane_intent().is_none());
    assert!(coordinator.pending_activation().is_some());
    assert!(coordinator.suppressed_tear_off_bindings().contains(&older));

    let winner_restored = coordinator
        .publish_platform_focus_observation(
            focus_observation(3, GlobalFocusedWindow::Dock(winner.window())),
            focus_stamp(4),
            binding_is_live,
            binding_is_live,
            |_| false,
            item_is_live,
        )
        .expect("the post-create focus barrier must settle on its exact winner");
    let FocusObservationTransition::Applied(winner_restored) = winner_restored else {
        panic!("the winner focus fact must advance the provider stream");
    };
    assert_eq!(
        winner_restored.completed_activation(),
        Some(reasserted.generation())
    );
    assert!(coordinator.suppressed_tear_off_bindings().is_empty());

    let later_user_focus = coordinator
        .publish_platform_focus_observation(
            focus_observation(4, GlobalFocusedWindow::Dock(older.window())),
            focus_stamp(5),
            binding_is_live,
            binding_is_live,
            |_| false,
            item_is_live,
        )
        .expect("focus after the settled barrier must remain a normal platform fact");
    assert!(matches!(
        later_user_focus,
        FocusObservationTransition::Applied(_)
    ));
    assert_eq!(coordinator.winning_focus_target(), None);
}

#[test]
fn newer_pointer_claim_retargets_an_unsettled_lifecycle_focus_barrier() {
    let (mut coordinator, older, old_winner, barrier, old_effect) =
        pending_lifecycle_focus_barrier();
    let new_winner = third_binding();
    let binding_is_live =
        |candidate| candidate == older || candidate == old_winner || candidate == new_winner;
    let item_is_live = |surface, item| {
        (surface == older.surface() && item == ItemId::new(1))
            || (surface == old_winner.surface() && item == ItemId::new(2))
            || (surface == new_winner.surface() && item == ItemId::new(3))
    };

    let successor = coordinator
        .request_activation(
            ViewportActivationRequest::pointer_tab_gesture(
                new_winner,
                PanelFocus::Item(ItemId::new(3)),
            ),
            focus_stamp(4),
            true,
            binding_is_live,
            item_is_live,
        )
        .expect("the pointer claim must serialize after the lifecycle barrier");
    assert_eq!(
        successor
            .superseded()
            .map(PendingViewportActivation::generation),
        Some(barrier.generation())
    );
    assert_eq!(
        successor.outcome(),
        ActivationStartOutcome::RequestPlatformFocus { target: new_winner }
    );
    assert_eq!(coordinator.winning_focus_target(), Some(new_winner));
    assert!(coordinator.suppressed_tear_off_bindings().contains(&older));
    assert!(
        coordinator
            .suppressed_tear_off_bindings()
            .contains(&old_winner)
    );

    let successor_effect = EffectId::new(43);
    assert_eq!(
        coordinator.attach_platform_focus_effect(successor.generation(), successor_effect),
        FocusEffectAttachment::Applied
    );
    let late_old_focus = coordinator
        .publish_platform_focus_observation(
            FocusObservationEnvelope::new(
                FocusObservationGeneration::new(2),
                Authority::Known(GlobalFocusedWindow::Dock(old_winner.window())),
                Authority::Known(Some(old_effect)),
            ),
            focus_stamp(5),
            binding_is_live,
            binding_is_live,
            |_| false,
            item_is_live,
        )
        .expect("the predecessor effect may settle without regaining focus authority");
    assert!(matches!(
        late_old_focus,
        FocusObservationTransition::Applied(_)
    ));
    assert_eq!(coordinator.winning_focus_target(), Some(new_winner));
    assert_eq!(
        coordinator
            .pending_activation()
            .map(PendingViewportActivation::generation),
        Some(successor.generation())
    );

    let completed = coordinator
        .publish_platform_focus_observation(
            FocusObservationEnvelope::new(
                FocusObservationGeneration::new(3),
                Authority::Known(GlobalFocusedWindow::Dock(new_winner.window())),
                Authority::Known(Some(successor_effect)),
            ),
            focus_stamp(6),
            binding_is_live,
            binding_is_live,
            |_| false,
            item_is_live,
        )
        .expect("the successor focus proof must settle the retargeted barrier");
    let FocusObservationTransition::Applied(completed) = completed else {
        panic!("the successor observation must advance the provider stream");
    };
    assert_eq!(
        completed.completed_activation(),
        Some(successor.generation())
    );
    assert_eq!(
        completed.pane_intent().map(PaneFocusIntent::native_guard),
        Some(Some(new_winner))
    );
    assert!(coordinator.suppressed_tear_off_bindings().is_empty());
}

#[test]
fn provider_replacement_atomically_retires_an_unsettled_lifecycle_focus_barrier() {
    let (mut coordinator, older, _winner, barrier, _effect) = pending_lifecycle_focus_barrier();
    assert!(coordinator.suppressed_tear_off_bindings().contains(&older));

    let cleanup = coordinator.revoke_platform_provider_authority();
    assert_eq!(
        cleanup.pending_activation_cancelled(),
        Some((
            barrier.generation(),
            ActivationCancellation::PlatformAuthorityRevoked,
        ))
    );
    assert!(coordinator.suppressed_tear_off_bindings().is_empty());

    let replacement_focus = publish_platform_focus(
        &mut coordinator,
        focus_observation(1, GlobalFocusedWindow::Dock(older.window())),
        4,
    );
    assert!(matches!(
        replacement_focus,
        FocusObservationTransition::Applied(_)
    ));
    assert_eq!(
        coordinator
            .focus_observation()
            .map(|observation| *observation.focused()),
        Some(Authority::Known(GlobalFocusedWindow::Dock(older.window())))
    );
    let _ = publish_platform_focus(
        &mut coordinator,
        focus_observation(2, GlobalFocusedWindow::Foreign),
        5,
    );
    let _ = publish_platform_focus(
        &mut coordinator,
        focus_observation(3, GlobalFocusedWindow::Dock(older.window())),
        6,
    );
    assert_eq!(coordinator.winning_focus_target(), None);
}

#[test]
fn failed_lifecycle_focus_barrier_reconciles_an_unchanged_current_focus_once() {
    let (mut coordinator, older, _winner, barrier, effect) = pending_lifecycle_focus_barrier();
    let isolated = publish_platform_focus(
        &mut coordinator,
        focus_observation(2, GlobalFocusedWindow::Dock(older.window())),
        4,
    );
    assert!(matches!(isolated, FocusObservationTransition::Applied(_)));
    assert_ne!(coordinator.winning_focus_target(), Some(older));

    assert!(matches!(
        coordinator.report_platform_focus_effect(
            effect,
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected),
        ),
        FocusEffectReportTransition::Cancelled {
            activation,
            reason: ActivationCancellation::EffectFailed,
        } if activation == barrier.generation()
    ));
    assert!(coordinator.suppressed_tear_off_bindings().is_empty());

    let reconciled = publish_platform_focus(
        &mut coordinator,
        focus_observation(3, GlobalFocusedWindow::Dock(older.window())),
        5,
    );
    assert!(matches!(reconciled, FocusObservationTransition::Applied(_)));
    assert_eq!(coordinator.winning_focus_target(), None);

    let duplicate_target = publish_platform_focus(
        &mut coordinator,
        focus_observation(4, GlobalFocusedWindow::Dock(older.window())),
        6,
    );
    assert!(matches!(
        duplicate_target,
        FocusObservationTransition::Applied(_)
    ));
    assert_eq!(coordinator.winning_focus_target(), None);
}

#[test]
fn acknowledged_lifecycle_barrier_with_quarantined_target_reasserts_winner() {
    let (mut coordinator, older, winner, barrier, effect) = pending_lifecycle_focus_barrier();
    let binding_is_live = |candidate| candidate == older || candidate == winner;
    let item_is_live = |surface, item| {
        (surface == older.surface() && item == ItemId::new(1))
            || (surface == winner.surface() && item == ItemId::new(2))
    };

    let transition = coordinator
        .publish_platform_focus_observation(
            FocusObservationEnvelope::new(
                FocusObservationGeneration::new(2),
                Authority::Known(GlobalFocusedWindow::Dock(older.window())),
                Authority::Known(Some(effect)),
            ),
            focus_stamp(4),
            binding_is_live,
            binding_is_live,
            |_| false,
            item_is_live,
        )
        .expect("the exact effect acknowledgement and focus fact must reduce atomically");
    let FocusObservationTransition::Applied(applied) = transition else {
        panic!("the acknowledged observation must advance the provider stream");
    };
    assert_eq!(
        applied.observed_effect().map(|observed| observed.effect()),
        Some(effect)
    );
    assert_eq!(
        applied.cancelled_activation(),
        Some((
            barrier.generation(),
            ActivationCancellation::EffectObservedWithoutTargetFocus,
        ))
    );
    assert_eq!(coordinator.winning_focus_target(), Some(winner));
    assert!(coordinator.suppressed_tear_off_bindings().contains(&older));
    assert!(matches!(
        coordinator
            .pending_activation()
            .map(PendingViewportActivation::platform_focus),
        Some(PendingPlatformFocus::EffectRequired)
    ));
}

#[test]
fn unknown_exact_ack_preserves_the_complete_lifecycle_focus_lane() {
    let (mut coordinator, older, winner, barrier, effect) = pending_lifecycle_focus_barrier();
    let binding_is_live = |candidate| candidate == older || candidate == winner;
    let item_is_live = |surface, item| {
        (surface == older.surface() && item == ItemId::new(1))
            || (surface == winner.surface() && item == ItemId::new(2))
    };
    let reason = AuthorityUnavailableReason::NotReported;

    let unknown = coordinator
        .publish_platform_focus_observation(
            FocusObservationEnvelope::new(
                FocusObservationGeneration::new(2),
                Authority::Unknown(reason),
                Authority::Known(Some(effect)),
            ),
            focus_stamp(4),
            binding_is_live,
            binding_is_live,
            |_| false,
            item_is_live,
        )
        .expect("the exact acknowledgement must survive unknown focus authority");
    let FocusObservationTransition::AuthorityRevoked {
        reason: revoked,
        cleanup,
        effect_settlement,
        ..
    } = unknown
    else {
        panic!("unknown focus must revoke only the provider focus namespace");
    };
    assert_eq!(revoked, FocusAuthorityRevocation::Unknown(reason));
    assert!(cleanup.pending_activation_cancelled().is_none());
    assert_eq!(
        effect_settlement
            .observed_effect()
            .map(ObservedPlatformFocusEffect::effect),
        Some(effect)
    );
    assert!(matches!(
        coordinator
            .pending_activation()
            .map(PendingViewportActivation::platform_focus),
        Some(PendingPlatformFocus::ObservedAwaitingTarget {
            effect: pending_effect,
            ..
        }) if pending_effect == effect
    ));
    assert_eq!(coordinator.winning_focus_target(), Some(winner));
    assert!(coordinator.suppressed_tear_off_bindings().contains(&older));

    let late_old_target = coordinator
        .publish_platform_focus_observation(
            focus_observation(3, GlobalFocusedWindow::Dock(older.window())),
            focus_stamp(5),
            binding_is_live,
            binding_is_live,
            |_| false,
            item_is_live,
        )
        .expect("the late isolated target must advance only the provider stream");
    assert!(matches!(
        late_old_target,
        FocusObservationTransition::Applied(_)
    ));
    assert_eq!(coordinator.winning_focus_target(), Some(winner));
    let replacement = coordinator
        .pending_activation()
        .expect("the quarantined target must create a new winner reassertion");
    assert_ne!(replacement.generation(), barrier.generation());
    assert_eq!(
        replacement.platform_focus(),
        PendingPlatformFocus::EffectRequired
    );
    let replacement_effect = EffectId::new(44);
    assert_eq!(
        coordinator.attach_platform_focus_effect(replacement.generation(), replacement_effect,),
        FocusEffectAttachment::Applied
    );

    let completed = coordinator
        .publish_platform_focus_observation(
            FocusObservationEnvelope::new(
                FocusObservationGeneration::new(4),
                Authority::Known(GlobalFocusedWindow::Dock(winner.window())),
                Authority::Known(Some(replacement_effect)),
            ),
            focus_stamp(6),
            binding_is_live,
            binding_is_live,
            |_| false,
            item_is_live,
        )
        .expect("the later winner fact must complete the retained lane");
    let FocusObservationTransition::Applied(completed) = completed else {
        panic!("the winner fact must advance the replacement focus baseline");
    };
    assert_eq!(
        completed.completed_activation(),
        Some(replacement.generation())
    );
    assert!(coordinator.suppressed_tear_off_bindings().is_empty());
}

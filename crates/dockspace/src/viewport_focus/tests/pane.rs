//! Pane behavior regressions.

use super::*;

#[test]
fn pane_intent_identity_exhaustion_does_not_publish_partial_state() {
    let mut coordinator = ViewportFocusCoordinator::default();
    let target = binding();
    coordinator
        .focus_observations
        .force_current_for_test(FocusObservationEnvelope::new(
            FocusObservationGeneration::new(1),
            Authority::Known(GlobalFocusedWindow::Dock(target.window())),
            Authority::Known(None),
        ));
    coordinator.exhaust_pane_intent_ids();
    let before = coordinator.clone();
    assert_eq!(
        coordinator.request_activation(
            ViewportActivationRequest::explicit(target, PanelFocus::None),
            focus_stamp(1),
            true,
            |_| true,
            |_, _| true,
        ),
        Err(ViewportFocusError::PaneFocusIntentIdExhausted)
    );
    assert_eq!(coordinator, before);
}

#[test]
fn local_focus_preservation_retires_old_pane_intents_and_late_native_activation() {
    let target = binding();
    let mut coordinator = ViewportFocusCoordinator::default();
    let _ = publish_platform_focus(
        &mut coordinator,
        focus_observation(1, GlobalFocusedWindow::Foreign),
        1,
    );
    let request = ViewportActivationRequest::explicit(target, PanelFocus::Item(ItemId::new(1)));
    let owner = NativeCreateSagaId::new(1);
    assert!(coordinator.reserve_activation_causal(owner, focus_stamp(2), request));
    coordinator
        .request_local_pane_focus(
            target.surface(),
            ItemId::new(1),
            Some(target),
            focus_stamp(3),
        )
        .unwrap()
        .expect("the older content-focus request must remain unacknowledged");
    let observed = coordinator.current_focus();

    assert!(coordinator.preserve_local_focus(target.surface(), Some(target), focus_stamp(4)));
    assert_eq!(coordinator.pending_pane_intent(), None);
    assert_eq!(coordinator.pending_activation(), None);
    assert_eq!(coordinator.current_focus(), observed);
    assert_eq!(
        coordinator.panel_focus(target.surface()),
        PanelFocusRecord::NoHistory
    );

    let late = coordinator
        .request_reserved_activation(
            owner,
            request,
            focus_stamp(2),
            true,
            |candidate| candidate == target,
            |surface, item| surface == target.surface() && item == ItemId::new(1),
        )
        .unwrap();
    assert_eq!(
        late.outcome(),
        ActivationStartOutcome::Suppressed(ActivationSuppression::CausallySuperseded {
            current: PaneFocusIntentGeneration::new(4),
        })
    );
    assert_eq!(coordinator.pending_pane_intent(), None);
}

#[test]
fn matching_unacknowledged_pane_observation_does_not_consume_intent() {
    let target = binding();
    let binding_is_live = |candidate| candidate == target;
    let item_is_live = |surface, item| surface == target.surface() && item == ItemId::new(1);
    let mut coordinator = ViewportFocusCoordinator::default();
    let _ = publish_platform_focus(
        &mut coordinator,
        focus_observation(1, GlobalFocusedWindow::Dock(target.window())),
        1,
    );
    let _ = coordinator.publish_pane_focus_observation(
        PaneFocusObservation::new(
            PaneFocusObservationGeneration::new(1),
            target,
            PanelFocus::Item(ItemId::new(1)),
        ),
        binding_is_live,
        item_is_live,
    );
    let activation = coordinator
        .request_activation(
            ViewportActivationRequest::explicit(target, PanelFocus::Item(ItemId::new(1))),
            focus_stamp(2),
            true,
            binding_is_live,
            item_is_live,
        )
        .expect("focused activation must allocate");
    let ActivationStartOutcome::PaneFocusReady { intent } = activation.outcome() else {
        panic!("focused activation must install an intent");
    };
    coordinator.mark_boundary_published();

    assert_eq!(
        coordinator.publish_pane_focus_observation(
            PaneFocusObservation::new(
                PaneFocusObservationGeneration::new(2),
                target,
                PanelFocus::Item(ItemId::new(1)),
            ),
            binding_is_live,
            item_is_live,
        ),
        PaneFocusObservationTransition::Applied {
            cleared_intent: None,
        }
    );
    assert_eq!(coordinator.pending_pane_intent(), Some(intent));
    assert_eq!(
        coordinator.publish_pane_focus_observation(
            PaneFocusObservation::new(
                PaneFocusObservationGeneration::new(3),
                target,
                PanelFocus::None,
            ),
            binding_is_live,
            item_is_live,
        ),
        PaneFocusObservationTransition::Applied {
            cleared_intent: None,
        }
    );
    assert_eq!(
        coordinator.pending_pane_intent(),
        Some(intent),
        "an ambient not-focused fact cannot terminate an exact request"
    );
    assert_eq!(
        coordinator.publish_pane_focus_observation(
            PaneFocusObservation::new(
                PaneFocusObservationGeneration::new(4),
                target,
                PanelFocus::Item(ItemId::new(1)),
            )
            .acknowledging(intent.id()),
            binding_is_live,
            item_is_live,
        ),
        PaneFocusObservationTransition::Applied {
            cleared_intent: Some(intent.id()),
        }
    );
}

#[test]
fn exact_pane_focus_observation_keeps_not_focused_pending_until_focused() {
    let target = binding().surface();
    let item = ItemId::new(1);
    let mut coordinator = ViewportFocusCoordinator::default();
    let intent = coordinator
        .request_local_pane_focus(target, item, None, focus_stamp(1))
        .expect("local pane focus must allocate")
        .expect("local pane focus must install an intent");

    assert_eq!(intent.surface(), target);
    assert_eq!(intent.item(), Some(item));
    assert_eq!(intent.native_guard(), None);
    assert_eq!(coordinator.published_pane_intent(), None);

    coordinator.mark_boundary_published();
    assert_eq!(coordinator.published_pane_intent(), Some(intent));
    assert_eq!(
        coordinator.publish_pane_focus_request_observation(
            PaneFocusRequestObservation::new(
                PaneFocusObservationGeneration::new(1),
                intent,
                PaneFocusRequestObservationState::NotFocused,
            ),
            |_| false,
            |surface, candidate| surface == target && candidate == item,
        ),
        PaneFocusObservationTransition::NotFocused {
            intent: intent.id(),
        }
    );
    assert_eq!(coordinator.pending_pane_intent(), Some(intent));
    assert_eq!(coordinator.published_pane_intent(), Some(intent));

    assert_eq!(
        coordinator.publish_pane_focus_request_observation(
            PaneFocusRequestObservation::new(
                PaneFocusObservationGeneration::new(2),
                intent,
                PaneFocusRequestObservationState::Focused,
            ),
            |_| false,
            |surface, candidate| surface == target && candidate == item,
        ),
        PaneFocusObservationTransition::Focused {
            cleared_intent: intent.id(),
        }
    );
    assert_eq!(coordinator.pending_pane_intent(), None);
    assert_eq!(coordinator.published_pane_intent(), None);
    assert_eq!(
        coordinator.panel_focus(target),
        PanelFocusRecord::Item(item)
    );
}

#[test]
fn unavailable_pane_focus_observation_terminates_only_its_exact_request() {
    let target = binding().surface();
    let item = ItemId::new(1);
    let mut coordinator = ViewportFocusCoordinator::default();
    let unavailable = coordinator
        .request_local_pane_focus(target, item, None, focus_stamp(1))
        .expect("local pane focus must allocate")
        .expect("local pane focus must install an intent");
    coordinator.mark_boundary_published();
    let terminal = PaneFocusRequestObservation::new(
        PaneFocusObservationGeneration::new(1),
        unavailable,
        PaneFocusRequestObservationState::Unavailable,
    );

    assert_eq!(
        coordinator.publish_pane_focus_request_observation(
            terminal,
            |_| false,
            |surface, candidate| surface == target && candidate == item,
        ),
        PaneFocusObservationTransition::Unavailable {
            intent: unavailable.id(),
        }
    );
    assert_eq!(coordinator.pending_pane_intent(), None);
    assert_eq!(
        coordinator.panel_focus(target),
        PanelFocusRecord::NoHistory,
        "unavailable focus must not guess a fallback pane"
    );
    assert_eq!(
        coordinator.publish_pane_focus_request_observation(
            terminal,
            |_| false,
            |surface, candidate| surface == target && candidate == item,
        ),
        PaneFocusObservationTransition::Rejected(PaneFocusObservationRejection::UnknownIntent {
            intent: unavailable.id(),
        })
    );

    let replacement = coordinator
        .request_local_pane_focus(target, item, None, focus_stamp(2))
        .expect("a later local focus request must allocate")
        .expect("a later causal generation must install a fresh request");
    coordinator.mark_boundary_published();
    assert_ne!(replacement.id(), unavailable.id());
    assert_eq!(
        coordinator.publish_pane_focus_request_observation(
            PaneFocusRequestObservation::new(
                PaneFocusObservationGeneration::new(2),
                unavailable,
                PaneFocusRequestObservationState::Focused,
            ),
            |_| false,
            |surface, candidate| surface == target && candidate == item,
        ),
        PaneFocusObservationTransition::Rejected(PaneFocusObservationRejection::UnknownIntent {
            intent: unavailable.id(),
        })
    );
    assert_eq!(coordinator.pending_pane_intent(), Some(replacement));
}

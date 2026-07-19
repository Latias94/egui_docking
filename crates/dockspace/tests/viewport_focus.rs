//! Adapter-neutral viewport focus contract tests.
//!
//! This target is activated when `viewport_focus` becomes part of the crate facade.

use dockspace::effect::{
    DispatchFailureReason, EffectDispatchResult, EffectId, EffectIndeterminateReason,
};
use dockspace::engine::DockEngine;
use dockspace::frame::{PanelFocus, ViewportCloseRequestId};
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, RootId, SurfaceId};
use dockspace::intent::{Authority, AuthorityUnavailableReason};
use dockspace::policy::DockPolicy;
use dockspace::transition::InputOutcome;
use dockspace::viewport::{ViewportBinding, ViewportRole, WindowToken};
use dockspace::viewport_focus::{
    ActivationStartOutcome, ActivationSuppression, FocusEffectAttachment,
    FocusEffectReportTransition, FocusObservationEnvelope, FocusObservationGeneration,
    FocusObservationTransition, GlobalFocusedWindow, PaneFocusIntentGeneration, PaneFocusIntentId,
    PaneFocusIntentSource, PaneFocusObservation, PaneFocusObservationGeneration,
    PaneFocusObservationRejection, PaneFocusObservationTransition, PanelFocusRecord,
    PendingPlatformFocus, PendingViewportActivation, PlatformFocusEvidence,
    ViewportActivationCause, ViewportActivationRequest, ViewportFocusCoordinator,
};

const SURFACE_A: SurfaceId = SurfaceId::new(1);
const SURFACE_B: SurfaceId = SurfaceId::new(2);
const ITEM_A: ItemId = ItemId::new(10);
const ITEM_B: ItemId = ItemId::new(20);

fn bindings(tokens: [u64; 2]) -> [ViewportBinding; 2] {
    let mut builder = Workspace::builder();
    let tabs_a = builder.insert_node(Node::tabs([ITEM_A]));
    let tabs_b = builder.insert_node(Node::tabs([ITEM_B]));
    builder.set_root(RootId::new(1), RootRecord::new(tabs_a));
    builder.set_root(RootId::new(2), RootRecord::new(tabs_b));
    builder.set_surface(SURFACE_A, SurfacePresentation::new(RootId::new(1)));
    builder.set_surface(SURFACE_B, SurfacePresentation::new(RootId::new(2)));
    let workspace = builder.build().expect("focus fixture must be valid");
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("focus engine must be valid");
    engine
        .enqueue_viewport_registration(
            SURFACE_A,
            WindowToken::new(tokens[0]),
            ViewportRole::Root,
            None,
        )
        .expect("surface A registration must enqueue");
    engine
        .enqueue_viewport_registration(
            SURFACE_B,
            WindowToken::new(tokens[1]),
            ViewportRole::Root,
            None,
        )
        .expect("surface B registration must enqueue");
    let transition = engine
        .reduce_pending()
        .expect("focus registrations must reduce");
    let registered = transition
        .reduced_inputs()
        .iter()
        .map(|reduced| match reduced.outcome() {
            InputOutcome::ViewportRegistered { binding } => *binding,
            outcome => panic!("unexpected registration outcome: {outcome:?}"),
        })
        .collect::<Vec<_>>();
    [registered[0], registered[1]]
}

fn focus_envelope(
    generation: u64,
    focused: Authority<GlobalFocusedWindow>,
) -> FocusObservationEnvelope {
    FocusObservationEnvelope::new(
        FocusObservationGeneration::new(generation),
        focused,
        Authority::Known(None),
    )
}

fn focus_envelope_with_ack(
    generation: u64,
    focused: Authority<GlobalFocusedWindow>,
    effect: EffectId,
) -> FocusObservationEnvelope {
    FocusObservationEnvelope::new(
        FocusObservationGeneration::new(generation),
        focused,
        Authority::Known(Some(effect)),
    )
}

fn current_bindings(current: [ViewportBinding; 2]) -> impl Fn(ViewportBinding) -> bool + Copy {
    move |binding| current.contains(&binding)
}

fn current_items(surface: SurfaceId, item: ItemId) -> bool {
    matches!((surface, item), (SURFACE_A, ITEM_A) | (SURFACE_B, ITEM_B))
}

fn publish(
    coordinator: &mut ViewportFocusCoordinator,
    envelope: FocusObservationEnvelope,
    pane_generation: u64,
    current: [ViewportBinding; 2],
) -> FocusObservationTransition {
    coordinator
        .publish_focus_observation(
            envelope,
            PaneFocusIntentGeneration::new(pane_generation),
            current_bindings(current),
            current_items,
        )
        .expect("focus identity domains must remain available")
}

#[test]
fn explicit_activation_waits_for_a_newer_exact_binding_observation() {
    let current = bindings([11, 12]);
    let [binding_a, binding_b] = current;
    let mut coordinator = ViewportFocusCoordinator::default();
    let initial = publish(
        &mut coordinator,
        focus_envelope(10, Authority::Known(GlobalFocusedWindow::Dock(binding_a))),
        10,
        current,
    );
    assert!(matches!(initial, FocusObservationTransition::Applied(_)));

    let start = coordinator
        .request_activation(
            ViewportActivationRequest::new(
                binding_b,
                PanelFocus::Item(ITEM_B),
                ViewportActivationCause::Explicit,
            ),
            PaneFocusIntentGeneration::new(11),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("activation generation must be available");
    assert_eq!(
        start.outcome(),
        ActivationStartOutcome::RequestPlatformFocus { target: binding_b }
    );
    let effect = EffectId::new(41);
    assert_eq!(
        coordinator.attach_platform_focus_effect(start.generation(), effect),
        FocusEffectAttachment::Applied
    );

    assert_eq!(
        publish(
            &mut coordinator,
            focus_envelope(10, Authority::Known(GlobalFocusedWindow::Dock(binding_b))),
            12,
            current,
        ),
        FocusObservationTransition::EqualGenerationConflict {
            generation: FocusObservationGeneration::new(10)
        }
    );
    let completed = publish(
        &mut coordinator,
        focus_envelope(11, Authority::Known(GlobalFocusedWindow::Dock(binding_b))),
        13,
        current,
    );
    let FocusObservationTransition::Applied(completed) = completed else {
        panic!("matching focus observation must be accepted");
    };
    assert_eq!(completed.completed_activation(), Some(start.generation()));
    assert_eq!(
        completed
            .observed_effect()
            .expect("matching observation must settle the effect")
            .evidence(),
        PlatformFocusEvidence::NewerMatchingObservation
    );
    let intent = completed
        .pane_intent()
        .expect("matching native focus must release pane focus");
    assert_eq!(intent.target(), binding_b);
    assert_eq!(intent.focus(), PanelFocus::Item(ITEM_B));
    assert_eq!(
        intent.source(),
        PaneFocusIntentSource::ExplicitViewportActivation
    );
}

#[test]
fn exact_effect_ack_without_target_focus_settles_effect_but_not_pane_focus() {
    let current = bindings([21, 22]);
    let [binding_a, binding_b] = current;
    let mut coordinator = ViewportFocusCoordinator::default();
    let _ = publish(
        &mut coordinator,
        focus_envelope(1, Authority::Known(GlobalFocusedWindow::Dock(binding_a))),
        1,
        current,
    );
    let start = coordinator
        .request_activation(
            ViewportActivationRequest::new(
                binding_b,
                PanelFocus::None,
                ViewportActivationCause::DropCommitted,
            ),
            PaneFocusIntentGeneration::new(2),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("activation must allocate");
    let effect = EffectId::new(7);
    assert_eq!(
        coordinator.attach_platform_focus_effect(start.generation(), effect),
        FocusEffectAttachment::Applied
    );

    let applied = publish(
        &mut coordinator,
        focus_envelope_with_ack(2, Authority::Known(GlobalFocusedWindow::Foreign), effect),
        3,
        current,
    );
    let FocusObservationTransition::Applied(applied) = applied else {
        panic!("acknowledgement must be accepted");
    };
    assert_eq!(
        applied
            .observed_effect()
            .expect("exact acknowledgement must settle the effect")
            .evidence(),
        PlatformFocusEvidence::ExactEffectAcknowledgement
    );
    assert!(applied.completed_activation().is_none());
    assert!(applied.pane_intent().is_none());
    assert!(coordinator.pending_activation().is_none());
}

#[test]
fn newer_different_dock_observation_does_not_preempt_an_unobserved_focus_effect() {
    let current = bindings([23, 24]);
    let [binding_a, binding_b] = current;
    let mut coordinator = ViewportFocusCoordinator::default();
    let _ = publish(
        &mut coordinator,
        focus_envelope(1, Authority::Known(GlobalFocusedWindow::Dock(binding_a))),
        1,
        current,
    );
    let start = coordinator
        .request_activation(
            ViewportActivationRequest::explicit(binding_b, PanelFocus::Item(ITEM_B)),
            PaneFocusIntentGeneration::new(2),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("activation must allocate");
    let effect = EffectId::new(70);
    let _ = coordinator.attach_platform_focus_effect(start.generation(), effect);

    let _ = publish(
        &mut coordinator,
        focus_envelope(2, Authority::Known(GlobalFocusedWindow::Dock(binding_a))),
        3,
        current,
    );
    assert_eq!(
        coordinator
            .pending_activation()
            .map(PendingViewportActivation::generation),
        Some(start.generation())
    );
}

#[test]
fn exact_ack_with_unknown_focus_waits_for_later_authoritative_target() {
    let current = bindings([25, 26]);
    let [binding_a, binding_b] = current;
    let mut coordinator = ViewportFocusCoordinator::default();
    let _ = publish(
        &mut coordinator,
        focus_envelope(1, Authority::Known(GlobalFocusedWindow::Dock(binding_a))),
        1,
        current,
    );
    let start = coordinator
        .request_activation(
            ViewportActivationRequest::explicit(binding_b, PanelFocus::Item(ITEM_B)),
            PaneFocusIntentGeneration::new(2),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("activation must allocate");
    let effect = EffectId::new(71);
    let _ = coordinator.attach_platform_focus_effect(start.generation(), effect);

    let acknowledged = publish(
        &mut coordinator,
        focus_envelope_with_ack(
            2,
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            effect,
        ),
        3,
        current,
    );
    let FocusObservationTransition::Applied(acknowledged) = acknowledged else {
        panic!("exact acknowledgement must be accepted");
    };
    assert!(acknowledged.observed_effect().is_some());
    assert!(matches!(
        coordinator
            .pending_activation()
            .map(PendingViewportActivation::platform_focus),
        Some(PendingPlatformFocus::ObservedAwaitingTarget {
            effect: pending_effect,
            ..
        }) if pending_effect == effect
    ));

    let completed = publish(
        &mut coordinator,
        focus_envelope(3, Authority::Known(GlobalFocusedWindow::Dock(binding_b))),
        4,
        current,
    );
    let FocusObservationTransition::Applied(completed) = completed else {
        panic!("later target focus must be accepted");
    };
    assert_eq!(completed.completed_activation(), Some(start.generation()));
    assert!(completed.pane_intent().is_some());
}

#[test]
fn superseding_activation_survives_a_late_predecessor_focus() {
    let current = bindings([27, 28]);
    let [binding_a, binding_b] = current;
    let mut coordinator = ViewportFocusCoordinator::default();
    let _ = publish(
        &mut coordinator,
        focus_envelope(1, Authority::Known(GlobalFocusedWindow::Dock(binding_a))),
        1,
        current,
    );

    let first = coordinator
        .request_activation(
            ViewportActivationRequest::explicit(binding_b, PanelFocus::Item(ITEM_B)),
            PaneFocusIntentGeneration::new(2),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("first activation must allocate");
    let first_effect = EffectId::new(72);
    let _ = coordinator.attach_platform_focus_effect(first.generation(), first_effect);
    let second = coordinator
        .request_activation(
            ViewportActivationRequest::explicit(binding_a, PanelFocus::Item(ITEM_A)),
            PaneFocusIntentGeneration::new(3),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("second activation must allocate");
    assert_eq!(
        second
            .superseded()
            .map(PendingViewportActivation::generation),
        Some(first.generation())
    );
    assert_eq!(
        second.outcome(),
        ActivationStartOutcome::RequestPlatformFocus { target: binding_a },
        "the successor must serialize even when its target was focused at baseline"
    );
    let second_effect = EffectId::new(73);
    let _ = coordinator.attach_platform_focus_effect(second.generation(), second_effect);

    let _ = publish(
        &mut coordinator,
        focus_envelope_with_ack(
            2,
            Authority::Known(GlobalFocusedWindow::Dock(binding_b)),
            first_effect,
        ),
        4,
        current,
    );
    assert_eq!(
        coordinator
            .pending_activation()
            .map(PendingViewportActivation::generation),
        Some(second.generation()),
        "a late predecessor must not cancel the successor"
    );

    let completed = publish(
        &mut coordinator,
        focus_envelope_with_ack(
            3,
            Authority::Known(GlobalFocusedWindow::Dock(binding_a)),
            second_effect,
        ),
        5,
        current,
    );
    let FocusObservationTransition::Applied(completed) = completed else {
        panic!("successor acknowledgement must be accepted");
    };
    assert_eq!(completed.completed_activation(), Some(second.generation()));
}

#[test]
fn indeterminate_focus_effect_waits_but_definitive_failure_cancels() {
    let current = bindings([31, 32]);
    let [binding_a, binding_b] = current;
    let mut coordinator = ViewportFocusCoordinator::default();
    let _ = publish(
        &mut coordinator,
        focus_envelope(1, Authority::Known(GlobalFocusedWindow::Dock(binding_a))),
        1,
        current,
    );
    let start = coordinator
        .request_activation(
            ViewportActivationRequest::new(
                binding_b,
                PanelFocus::Item(ITEM_B),
                ViewportActivationCause::Explicit,
            ),
            PaneFocusIntentGeneration::new(2),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("activation must allocate");
    let effect = EffectId::new(8);
    let _ = coordinator.attach_platform_focus_effect(start.generation(), effect);
    assert_eq!(
        coordinator.report_platform_focus_effect(
            effect,
            EffectDispatchResult::Indeterminate(EffectIndeterminateReason::AcknowledgementLost),
        ),
        FocusEffectReportTransition::Indeterminate {
            activation: start.generation()
        }
    );
    assert!(coordinator.pending_activation().is_some());

    assert!(matches!(
        coordinator.report_platform_focus_effect(
            effect,
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected),
        ),
        FocusEffectReportTransition::Cancelled { .. }
    ));
    assert!(coordinator.pending_activation().is_none());
    let late = publish(
        &mut coordinator,
        focus_envelope(2, Authority::Known(GlobalFocusedWindow::Dock(binding_b))),
        3,
        current,
    );
    let FocusObservationTransition::Applied(late) = late else {
        panic!("late focus is still a valid global fact");
    };
    assert!(late.completed_activation().is_none());
    assert!(late.pane_intent().is_none());
}

#[test]
fn close_recovery_is_observe_only_and_preserves_item_or_none_exactly() {
    let current = bindings([41, 42]);
    let [binding_a, binding_b] = current;
    let mut coordinator = ViewportFocusCoordinator::default();
    let _ = publish(
        &mut coordinator,
        focus_envelope(4, Authority::Known(GlobalFocusedWindow::Dock(binding_b))),
        4,
        current,
    );
    let close = ViewportCloseRequestId::new(9);
    let item = coordinator
        .request_activation(
            ViewportActivationRequest::close_recovery(binding_b, PanelFocus::Item(ITEM_B), close),
            PaneFocusIntentGeneration::new(5),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("close recovery must allocate");
    let ActivationStartOutcome::PaneFocusReady { intent } = item.outcome() else {
        panic!("already-focused close target must create pane intent");
    };
    assert_eq!(intent.focus(), PanelFocus::Item(ITEM_B));
    assert_eq!(intent.source(), PaneFocusIntentSource::CloseRecovery);
    assert!(coordinator.pending_activation().is_none());

    let none = coordinator
        .request_activation(
            ViewportActivationRequest::close_recovery(
                binding_b,
                PanelFocus::None,
                ViewportCloseRequestId::new(10),
            ),
            PaneFocusIntentGeneration::new(6),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("no-panel recovery must allocate");
    let ActivationStartOutcome::PaneFocusReady { intent } = none.outcome() else {
        panic!("explicit no-panel state must remain actionable");
    };
    assert_eq!(intent.focus(), PanelFocus::None);

    let _ = publish(
        &mut coordinator,
        focus_envelope(5, Authority::Known(GlobalFocusedWindow::Dock(binding_a))),
        7,
        current,
    );
    let recorded = coordinator
        .request_activation(
            ViewportActivationRequest::close_recovery(
                binding_b,
                PanelFocus::Item(ITEM_B),
                ViewportCloseRequestId::new(11),
            ),
            PaneFocusIntentGeneration::new(8),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("observe-only recovery still gets an identity");
    let ActivationStartOutcome::ObserveOnlyRecorded { record } = recorded.outcome() else {
        panic!("unfocused close recovery must remain queryable without becoming actionable");
    };
    assert_eq!(record.request().target(), binding_b);
    assert_eq!(record.request().focus(), PanelFocus::Item(ITEM_B));
    assert_eq!(coordinator.recorded_observe_only_activation(), Some(record));
    assert!(coordinator.pending_activation().is_none());
    let cleared = publish(
        &mut coordinator,
        focus_envelope(
            6,
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        ),
        9,
        current,
    );
    let FocusObservationTransition::Applied(cleared) = cleared else {
        panic!("new global observation must be accepted");
    };
    assert_eq!(
        cleared.cleared_observe_only_activation(),
        Some(record.generation())
    );
    assert!(coordinator.recorded_observe_only_activation().is_none());
}

#[test]
fn close_recovery_pane_intent_survives_same_target_and_unknown_focus() {
    let current = bindings([43, 44]);
    let [binding_a, binding_b] = current;
    let mut coordinator = ViewportFocusCoordinator::default();
    let _ = publish(
        &mut coordinator,
        focus_envelope(1, Authority::Known(GlobalFocusedWindow::Dock(binding_b))),
        1,
        current,
    );
    let start = coordinator
        .request_activation(
            ViewportActivationRequest::close_recovery(
                binding_b,
                PanelFocus::Item(ITEM_B),
                ViewportCloseRequestId::new(12),
            ),
            PaneFocusIntentGeneration::new(2),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("close recovery must allocate");
    let ActivationStartOutcome::PaneFocusReady { intent } = start.outcome() else {
        panic!("focused target must produce pane intent");
    };

    let _ = publish(
        &mut coordinator,
        focus_envelope(2, Authority::Known(GlobalFocusedWindow::Dock(binding_b))),
        3,
        current,
    );
    assert_eq!(coordinator.pending_pane_intent(), Some(intent));
    let _ = publish(
        &mut coordinator,
        focus_envelope(
            3,
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        ),
        4,
        current,
    );
    assert_eq!(coordinator.pending_pane_intent(), Some(intent));
    let _ = publish(
        &mut coordinator,
        focus_envelope(4, Authority::Known(GlobalFocusedWindow::Dock(binding_a))),
        5,
        current,
    );
    assert!(coordinator.pending_pane_intent().is_none());
}

#[test]
fn pane_focus_history_has_three_states_and_requires_exact_acknowledgement() {
    let current = bindings([51, 52]);
    let [binding_a, _] = current;
    let mut coordinator = ViewportFocusCoordinator::default();
    assert_eq!(
        coordinator.panel_focus(SURFACE_A),
        PanelFocusRecord::NoHistory
    );
    let _ = publish(
        &mut coordinator,
        focus_envelope(1, Authority::Known(GlobalFocusedWindow::Dock(binding_a))),
        1,
        current,
    );
    let start = coordinator
        .request_activation(
            ViewportActivationRequest::new(
                binding_a,
                PanelFocus::Item(ITEM_A),
                ViewportActivationCause::Explicit,
            ),
            PaneFocusIntentGeneration::new(2),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("already-focused activation must allocate");
    let ActivationStartOutcome::PaneFocusReady { intent } = start.outcome() else {
        panic!("pane intent must be ready");
    };
    assert_eq!(
        coordinator.panel_focus(SURFACE_A),
        PanelFocusRecord::NoHistory
    );
    assert_eq!(
        coordinator.publish_pane_focus_observation(
            PaneFocusObservation::new(
                PaneFocusObservationGeneration::new(1),
                binding_a,
                PanelFocus::Item(ITEM_A),
            )
            .acknowledging(PaneFocusIntentId::new(99)),
            current_bindings(current),
            current_items,
        ),
        PaneFocusObservationTransition::Rejected(PaneFocusObservationRejection::UnknownIntent {
            intent: PaneFocusIntentId::new(99)
        })
    );
    assert_eq!(
        coordinator.panel_focus(SURFACE_A),
        PanelFocusRecord::NoHistory
    );
    assert_eq!(
        coordinator.publish_pane_focus_observation(
            PaneFocusObservation::new(
                PaneFocusObservationGeneration::new(2),
                binding_a,
                PanelFocus::Item(ITEM_A),
            )
            .acknowledging(intent.id()),
            current_bindings(current),
            current_items,
        ),
        PaneFocusObservationTransition::Applied {
            cleared_intent: Some(intent.id())
        }
    );
    assert_eq!(
        coordinator.panel_focus(SURFACE_A),
        PanelFocusRecord::Item(ITEM_A)
    );
    assert_eq!(
        coordinator.publish_pane_focus_observation(
            PaneFocusObservation::new(
                PaneFocusObservationGeneration::new(3),
                binding_a,
                PanelFocus::None,
            ),
            current_bindings(current),
            current_items,
        ),
        PaneFocusObservationTransition::Applied {
            cleared_intent: None
        }
    );
    assert_eq!(coordinator.panel_focus(SURFACE_A), PanelFocusRecord::None);
}

#[test]
fn no_history_never_uses_selected_tab_as_pane_focus() {
    let current = bindings([61, 62]);
    let [binding_a, _] = current;
    let mut coordinator = ViewportFocusCoordinator::default();
    let applied = publish(
        &mut coordinator,
        focus_envelope(1, Authority::Known(GlobalFocusedWindow::Dock(binding_a))),
        1,
        current,
    );
    let FocusObservationTransition::Applied(applied) = applied else {
        panic!("first global focus fact must be accepted");
    };
    assert!(applied.pane_intent().is_none());
    assert_eq!(
        coordinator.panel_focus(SURFACE_A),
        PanelFocusRecord::NoHistory
    );

    let unknown = publish(
        &mut coordinator,
        focus_envelope(
            2,
            Authority::Unknown(AuthorityUnavailableReason::ProviderUnavailable),
        ),
        2,
        current,
    );
    assert!(matches!(unknown, FocusObservationTransition::Applied(_)));
    assert!(coordinator.pending_pane_intent().is_none());
}

#[test]
fn newer_known_foreign_focus_clears_an_unacknowledged_explicit_pane_intent() {
    let current = bindings([63, 64]);
    let [binding_a, _] = current;
    let mut coordinator = ViewportFocusCoordinator::default();
    let _ = publish(
        &mut coordinator,
        focus_envelope(1, Authority::Known(GlobalFocusedWindow::Dock(binding_a))),
        1,
        current,
    );
    let start = coordinator
        .request_activation(
            ViewportActivationRequest::explicit(binding_a, PanelFocus::Item(ITEM_A)),
            PaneFocusIntentGeneration::new(2),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("already-focused activation must allocate");
    assert!(matches!(
        start.outcome(),
        ActivationStartOutcome::PaneFocusReady { .. }
    ));

    let _ = publish(
        &mut coordinator,
        focus_envelope(2, Authority::Known(GlobalFocusedWindow::Foreign)),
        3,
        current,
    );
    assert!(coordinator.pending_pane_intent().is_none());
}

#[test]
fn stale_binding_item_and_close_cleanup_remove_only_owned_state() {
    let current = bindings([71, 72]);
    let replacement = bindings([81, 82]);
    let [binding_a, binding_b] = current;
    let mut coordinator = ViewportFocusCoordinator::default();
    let _ = publish(
        &mut coordinator,
        focus_envelope(1, Authority::Known(GlobalFocusedWindow::Dock(binding_b))),
        1,
        current,
    );
    let close = ViewportCloseRequestId::new(17);
    let start = coordinator
        .request_activation(
            ViewportActivationRequest::close_recovery(binding_b, PanelFocus::Item(ITEM_B), close),
            PaneFocusIntentGeneration::new(2),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("close recovery must allocate");
    assert!(matches!(
        start.outcome(),
        ActivationStartOutcome::PaneFocusReady { .. }
    ));
    assert!(coordinator.clear_close_request(close).changed());
    assert!(coordinator.pending_pane_intent().is_none());

    let _ = coordinator.publish_pane_focus_observation(
        PaneFocusObservation::new(
            PaneFocusObservationGeneration::new(1),
            binding_a,
            PanelFocus::Item(ITEM_A),
        ),
        current_bindings(current),
        current_items,
    );
    assert_eq!(
        coordinator.panel_focus(SURFACE_A),
        PanelFocusRecord::Item(ITEM_A)
    );
    let binding_cleanup = coordinator.clear_binding(binding_a);
    assert!(binding_cleanup.changed());
    assert_eq!(
        coordinator.panel_focus(SURFACE_A),
        PanelFocusRecord::Item(ITEM_A),
        "rebinding preserves logical per-surface focus history"
    );
    assert!(coordinator.clear_item(ITEM_A).changed());
    assert_eq!(
        coordinator.panel_focus(SURFACE_A),
        PanelFocusRecord::NoHistory
    );

    let stale = coordinator
        .request_activation(
            ViewportActivationRequest::new(
                replacement[0],
                PanelFocus::None,
                ViewportActivationCause::Explicit,
            ),
            PaneFocusIntentGeneration::new(3),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("stale activation still receives diagnostic identity");
    assert_eq!(
        stale.outcome(),
        ActivationStartOutcome::Suppressed(ActivationSuppression::StaleBinding)
    );
}

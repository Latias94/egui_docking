//! Adapter-neutral viewport focus contract tests.
//!
//! This target is activated when `viewport_focus` becomes part of the crate facade.

use super::support;

use dockspace::effect::{
    DispatchFailureReason, EffectDispatchResult, EffectId, EffectIndeterminateReason,
};
use dockspace::engine::{DockEngine, EngineInput};
use dockspace::event::ReductionCause;
use dockspace::frame::PanelFocus;
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, ReducerTickId, RootId, StableInputSourceId, SurfaceId};
use dockspace::intent::{Authority, AuthorityUnavailableReason};
use dockspace::policy::DockPolicy;
use dockspace::transition::InputOutcome;
use dockspace::viewport::{ViewportBinding, ViewportRole, WindowToken};
use dockspace::viewport_focus::{
    ActivationCancellation, ActivationStartOutcome, ActivationSuppression,
    FocusAuthorityRevocation, FocusCausalStamp, FocusEffectAttachment, FocusEffectReportTransition,
    FocusObservationEnvelope, FocusObservationGeneration, FocusObservationTransition,
    GlobalFocusedWindow, PaneFocusDisposition, PaneFocusIntentGeneration, PaneFocusIntentId,
    PaneFocusIntentSource, PaneFocusObservation, PaneFocusObservationGeneration,
    PaneFocusObservationRejection, PaneFocusObservationTransition, PanelFocusRecord,
    PendingPlatformFocus, PendingViewportActivation, PlatformFocusEvidence,
    ViewportActivationCause, ViewportActivationRequest, ViewportFocusCoordinator,
};
use support::{TestPresentationHost, submit_inputs};

const SURFACE_A: SurfaceId = SurfaceId::new(1);
const SURFACE_B: SurfaceId = SurfaceId::new(2);
const ITEM_A: ItemId = ItemId::new(10);
const ITEM_B: ItemId = ItemId::new(20);
const FOCUS_INPUT_SOURCE: StableInputSourceId = StableInputSourceId::new(0xF0C0);

fn bindings(tokens: [u64; 2]) -> [ViewportBinding; 2] {
    let mut builder = Workspace::builder();
    let tabs_a = builder.insert_node(Node::tabs([ITEM_A]));
    let tabs_b = builder.insert_node(Node::tabs([ITEM_B]));
    builder.set_root(RootId::new(1), RootRecord::new(tabs_a));
    builder.set_root(RootId::new(2), RootRecord::new(tabs_b));
    builder.set_surface(SURFACE_A, SurfacePresentation::with_main(RootId::new(1)));
    builder.set_surface(SURFACE_B, SurfacePresentation::with_main(RootId::new(2)));
    let workspace = builder.build().expect("focus fixture must be valid");
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("focus engine must be valid");
    let mut presentation_host = TestPresentationHost::new(&mut engine);
    let provider = presentation_host.platform_provider();
    let expected = engine.version();
    let transition = submit_inputs(
        &mut engine,
        &mut presentation_host,
        FOCUS_INPUT_SOURCE,
        [
            EngineInput::RegisterViewport {
                provider,
                expected,
                surface: SURFACE_A,
                token: WindowToken::new(tokens[0]),
                role: ViewportRole::Root,
                recovery_target: None,
            },
            EngineInput::RegisterViewport {
                provider,
                expected,
                surface: SURFACE_B,
                token: WindowToken::new(tokens[1]),
                role: ViewportRole::Root,
                recovery_target: None,
            },
        ],
    )
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

fn focus_stamp(generation: u64) -> FocusCausalStamp {
    FocusCausalStamp::new(
        PaneFocusIntentGeneration::new(generation),
        ReductionCause::SurfaceContributionBatch {
            tick: ReducerTickId::new(generation),
        },
    )
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
            focus_stamp(pane_generation),
            current_bindings(current),
            current_items,
        )
        .expect("focus identity domains must remain available")
}

#[test]
fn focused_pointer_tab_gesture_installs_pane_intent_without_platform_focus() {
    let current = bindings([9, 10]);
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
            ViewportActivationRequest::new(
                binding_a,
                PanelFocus::Item(ITEM_A),
                ViewportActivationCause::PointerTabGesture,
            ),
            focus_stamp(2),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("pointer tab focus identity must allocate");

    let ActivationStartOutcome::PaneFocusReady { intent } = start.outcome() else {
        panic!("an already-focused pointer target must release pane focus immediately");
    };
    assert_eq!(intent.source(), PaneFocusIntentSource::PointerTabGesture);
    assert_eq!(
        intent.cause(),
        Some(ViewportActivationCause::PointerTabGesture)
    );
    assert_eq!(intent.focus(), PanelFocus::Item(ITEM_A));
    assert!(coordinator.pending_activation().is_none());
    assert!(coordinator.recorded_observe_only_activation().is_none());
}

#[test]
fn focused_drop_with_no_history_preserves_pane_focus_without_minting_an_intent() {
    let current = bindings([91, 92]);
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
            ViewportActivationRequest::with_disposition(
                binding_a,
                PaneFocusDisposition::Preserve,
                ViewportActivationCause::DropCommitted,
            ),
            focus_stamp(2),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("preserving activation must allocate");

    assert_eq!(
        start.outcome(),
        ActivationStartOutcome::PaneFocusPreserved { target: binding_a }
    );
    assert!(coordinator.pending_activation().is_none());
    assert!(coordinator.pending_pane_intent().is_none());
}

#[test]
fn newer_preserve_suppresses_an_older_delayed_activation() {
    let current = bindings([93, 94]);
    let [binding_a, _] = current;
    let mut coordinator = ViewportFocusCoordinator::default();
    let _ = publish(
        &mut coordinator,
        focus_envelope(1, Authority::Known(GlobalFocusedWindow::Dock(binding_a))),
        1,
        current,
    );

    let preserve = coordinator
        .request_activation(
            ViewportActivationRequest::with_disposition(
                binding_a,
                PaneFocusDisposition::Preserve,
                ViewportActivationCause::DropCommitted,
            ),
            focus_stamp(20),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("newer preserving activation must allocate");
    assert_eq!(
        preserve.outcome(),
        ActivationStartOutcome::PaneFocusPreserved { target: binding_a }
    );

    let delayed = coordinator
        .request_activation(
            ViewportActivationRequest::explicit(binding_a, PanelFocus::Item(ITEM_A)),
            focus_stamp(10),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("older activation receives a diagnostic result");
    assert_eq!(
        delayed.outcome(),
        ActivationStartOutcome::Suppressed(ActivationSuppression::CausallySuperseded {
            current: PaneFocusIntentGeneration::new(20),
        })
    );
    assert!(coordinator.pending_activation().is_none());
    assert!(coordinator.pending_pane_intent().is_none());
}

#[test]
fn newer_no_history_focus_suppresses_an_older_delayed_activation() {
    let current = bindings([95, 96]);
    let [binding_a, _] = current;
    let mut coordinator = ViewportFocusCoordinator::default();
    let applied = publish(
        &mut coordinator,
        focus_envelope(1, Authority::Known(GlobalFocusedWindow::Dock(binding_a))),
        20,
        current,
    );
    let FocusObservationTransition::Applied(applied) = applied else {
        panic!("newer focus observation must apply");
    };
    assert!(applied.pane_intent().is_none());

    let delayed = coordinator
        .request_activation(
            ViewportActivationRequest::explicit(binding_a, PanelFocus::Item(ITEM_A)),
            focus_stamp(10),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("older activation receives a diagnostic result");
    assert_eq!(
        delayed.outcome(),
        ActivationStartOutcome::Suppressed(ActivationSuppression::CausallySuperseded {
            current: PaneFocusIntentGeneration::new(20),
        })
    );
}

#[test]
fn pointer_tab_gesture_waits_for_newer_exact_focus_without_requesting_it() {
    let current = bindings([13, 14]);
    let [binding_a, _] = current;
    let mut coordinator = ViewportFocusCoordinator::default();
    let _ = publish(
        &mut coordinator,
        focus_envelope(1, Authority::Known(GlobalFocusedWindow::Foreign)),
        1,
        current,
    );

    let start = coordinator
        .request_activation(
            ViewportActivationRequest::new(
                binding_a,
                PanelFocus::Item(ITEM_A),
                ViewportActivationCause::PointerTabGesture,
            ),
            focus_stamp(2),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("pointer tab focus identity must allocate");
    let ActivationStartOutcome::ObserveOnlyRecorded { record } = start.outcome() else {
        panic!("an unfocused pointer target must wait for provider focus evidence");
    };
    assert_eq!(
        record.request().cause(),
        ViewportActivationCause::PointerTabGesture
    );
    assert!(coordinator.pending_activation().is_none());
    assert!(coordinator.pending_pane_intent().is_none());

    let observed = publish(
        &mut coordinator,
        focus_envelope(2, Authority::Known(GlobalFocusedWindow::Dock(binding_a))),
        3,
        current,
    );
    let FocusObservationTransition::Applied(observed) = observed else {
        panic!("newer exact focus evidence must settle the pointer gesture");
    };
    let intent = observed
        .pane_intent()
        .expect("settled pointer focus must release its pane intent");
    assert_eq!(intent.causal(), focus_stamp(2));
    assert_eq!(intent.activation(), Some(start.generation()));
    assert_eq!(intent.source(), PaneFocusIntentSource::PointerTabGesture);
    assert_eq!(intent.focus(), PanelFocus::Item(ITEM_A));
    assert_eq!(
        observed.cleared_observe_only_activation(),
        Some(start.generation())
    );
    assert!(coordinator.recorded_observe_only_activation().is_none());
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
            focus_stamp(11),
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
            focus_envelope(10, Authority::Known(GlobalFocusedWindow::Dock(binding_a))),
            12,
            current,
        ),
        FocusObservationTransition::Duplicate
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
    assert_eq!(intent.causal(), focus_stamp(11));
    assert_eq!(intent.target(), binding_b);
    assert_eq!(intent.focus(), PanelFocus::Item(ITEM_B));
    assert_eq!(
        intent.source(),
        PaneFocusIntentSource::ExplicitViewportActivation
    );
}

#[test]
fn unfocused_preserving_activation_completes_without_a_pane_intent() {
    let current = bindings([15, 16]);
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
            ViewportActivationRequest::with_disposition(
                binding_b,
                PaneFocusDisposition::Preserve,
                ViewportActivationCause::DropCommitted,
            ),
            focus_stamp(2),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("preserving activation must allocate");
    let effect = EffectId::new(42);
    assert_eq!(
        coordinator.attach_platform_focus_effect(start.generation(), effect),
        FocusEffectAttachment::Applied
    );

    let completed = publish(
        &mut coordinator,
        focus_envelope_with_ack(
            2,
            Authority::Known(GlobalFocusedWindow::Dock(binding_b)),
            effect,
        ),
        3,
        current,
    );
    let FocusObservationTransition::Applied(completed) = completed else {
        panic!("matching focus observation must settle preserving activation");
    };
    assert_eq!(completed.completed_activation(), Some(start.generation()));
    assert!(completed.pane_intent().is_none());
    assert!(coordinator.pending_pane_intent().is_none());
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
            focus_stamp(2),
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
            focus_stamp(2),
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
            focus_stamp(2),
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
    let FocusObservationTransition::AuthorityRevoked {
        reason,
        cleanup,
        effect_settlement,
        ..
    } = acknowledged
    else {
        panic!("unknown focus must revoke focus authority while retaining the exact effect fact");
    };
    assert_eq!(
        reason,
        FocusAuthorityRevocation::Unknown(AuthorityUnavailableReason::NotReported)
    );
    assert!(cleanup.global_authority_removed());
    assert!(cleanup.pending_activation_cancelled().is_none());
    assert!(effect_settlement.observed_effect().is_some());
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
            focus_stamp(2),
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
            focus_stamp(3),
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
    let intent = completed
        .pane_intent()
        .expect("only the successor may publish the final pane intent");
    assert_eq!(intent.target(), binding_a);
    assert_eq!(intent.causal(), focus_stamp(3));
    assert_eq!(coordinator.pending_pane_intent(), Some(intent));
}

#[test]
fn foreign_focus_quarantines_a_late_explicit_focus_effect_until_reconfirmed() {
    let current = bindings([33, 34]);
    let [binding_a, binding_b] = current;
    let mut coordinator = ViewportFocusCoordinator::default();
    assert!(matches!(
        coordinator.publish_pane_focus_observation(
            PaneFocusObservation::new(
                PaneFocusObservationGeneration::new(1),
                binding_b,
                PanelFocus::Item(ITEM_B),
            ),
            current_bindings(current),
            current_items,
        ),
        PaneFocusObservationTransition::Applied { .. }
    ));
    let _ = publish(
        &mut coordinator,
        focus_envelope(1, Authority::Known(GlobalFocusedWindow::Dock(binding_a))),
        1,
        current,
    );

    let predecessor = coordinator
        .request_activation(
            ViewportActivationRequest::explicit(binding_b, PanelFocus::Item(ITEM_B)),
            focus_stamp(2),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("the explicit focus request must allocate");
    let predecessor_effect = EffectId::new(76);
    assert_eq!(
        coordinator.attach_platform_focus_effect(predecessor.generation(), predecessor_effect),
        FocusEffectAttachment::Applied
    );

    let foreign = publish(
        &mut coordinator,
        focus_envelope(2, Authority::Known(GlobalFocusedWindow::Foreign)),
        3,
        current,
    );
    assert!(matches!(foreign, FocusObservationTransition::Applied(_)));

    let late_predecessor = publish(
        &mut coordinator,
        focus_envelope(3, Authority::Known(GlobalFocusedWindow::Dock(binding_b))),
        4,
        current,
    );
    let FocusObservationTransition::Applied(late_predecessor) = late_predecessor else {
        panic!("the late matching-target proof must settle without regaining focus authority");
    };
    assert_eq!(
        late_predecessor
            .observed_effect()
            .map(|observed| observed.effect()),
        Some(predecessor_effect),
        "the stale effect must still settle exactly once"
    );
    assert!(
        late_predecessor.pane_intent().is_none(),
        "the stale Dock target must remain isolated from pane restoration"
    );
    assert!(
        late_predecessor.completed_activation().is_none(),
        "the stale request may settle its effect but must not complete as the focus winner"
    );
    assert!(coordinator.pending_pane_intent().is_none());

    let reconfirmed = publish(
        &mut coordinator,
        focus_envelope(4, Authority::Known(GlobalFocusedWindow::Foreign)),
        5,
        current,
    );
    assert!(matches!(
        reconfirmed,
        FocusObservationTransition::Applied(_)
    ));

    let later_user_focus = publish(
        &mut coordinator,
        focus_envelope(5, Authority::Known(GlobalFocusedWindow::Dock(binding_b))),
        6,
        current,
    );
    let FocusObservationTransition::Applied(later_user_focus) = later_user_focus else {
        panic!("focus after exact Foreign reconfirmation must be ordinary platform authority");
    };
    let restored = later_user_focus
        .pane_intent()
        .expect("exact Foreign reconfirmation must release the stale-effect barrier");
    assert_eq!(restored.target(), binding_b);
    assert_eq!(restored.focus(), PanelFocus::Item(ITEM_B));
    assert_eq!(restored.source(), PaneFocusIntentSource::PlatformActivation);
}

#[test]
fn failed_superseded_focus_effect_releases_the_external_focus_barrier() {
    let current = bindings([37, 38]);
    let [binding_a, binding_b] = current;
    let mut coordinator = ViewportFocusCoordinator::default();
    assert!(matches!(
        coordinator.publish_pane_focus_observation(
            PaneFocusObservation::new(
                PaneFocusObservationGeneration::new(1),
                binding_b,
                PanelFocus::Item(ITEM_B),
            ),
            current_bindings(current),
            current_items,
        ),
        PaneFocusObservationTransition::Applied { .. }
    ));
    let _ = publish(
        &mut coordinator,
        focus_envelope(1, Authority::Known(GlobalFocusedWindow::Dock(binding_a))),
        1,
        current,
    );

    let predecessor = coordinator
        .request_activation(
            ViewportActivationRequest::explicit(binding_b, PanelFocus::Item(ITEM_B)),
            focus_stamp(2),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("the explicit focus request must allocate");
    let predecessor_effect = EffectId::new(78);
    assert_eq!(
        coordinator.attach_platform_focus_effect(predecessor.generation(), predecessor_effect),
        FocusEffectAttachment::Applied
    );

    let _ = publish(
        &mut coordinator,
        focus_envelope(2, Authority::Known(GlobalFocusedWindow::Foreign)),
        3,
        current,
    );
    assert_eq!(
        coordinator.report_platform_focus_effect(
            predecessor_effect,
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected),
        ),
        FocusEffectReportTransition::Cancelled {
            activation: predecessor.generation(),
            reason: ActivationCancellation::EffectFailed,
        }
    );

    let later_user_focus = publish(
        &mut coordinator,
        focus_envelope(3, Authority::Known(GlobalFocusedWindow::Dock(binding_b))),
        4,
        current,
    );
    let FocusObservationTransition::Applied(later_user_focus) = later_user_focus else {
        panic!("focus after a terminal predecessor failure must be ordinary authority");
    };
    let restored = later_user_focus
        .pane_intent()
        .expect("the terminal failure must release the obsolete target quarantine");
    assert_eq!(restored.target(), binding_b);
    assert_eq!(restored.focus(), PanelFocus::Item(ITEM_B));
}

#[test]
fn newer_foreign_focus_supersedes_none_after_a_late_focus_hazard() {
    let current = bindings([35, 36]);
    let [binding_a, binding_b] = current;
    let mut coordinator = ViewportFocusCoordinator::default();
    let _ = publish(
        &mut coordinator,
        focus_envelope(1, Authority::Known(GlobalFocusedWindow::Dock(binding_a))),
        1,
        current,
    );

    let predecessor = coordinator
        .request_activation(
            ViewportActivationRequest::explicit(binding_b, PanelFocus::Item(ITEM_B)),
            focus_stamp(2),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("the explicit focus request must allocate");
    let predecessor_effect = EffectId::new(77);
    assert_eq!(
        coordinator.attach_platform_focus_effect(predecessor.generation(), predecessor_effect),
        FocusEffectAttachment::Applied
    );

    let none = publish(
        &mut coordinator,
        focus_envelope(2, Authority::Known(GlobalFocusedWindow::None)),
        3,
        current,
    );
    assert!(matches!(none, FocusObservationTransition::Applied(_)));

    let late_predecessor = publish(
        &mut coordinator,
        focus_envelope(3, Authority::Known(GlobalFocusedWindow::Dock(binding_b))),
        4,
        current,
    );
    let FocusObservationTransition::Applied(late_predecessor) = late_predecessor else {
        panic!("the late matching-target proof must settle without regaining focus authority");
    };
    assert_eq!(
        late_predecessor
            .observed_effect()
            .map(|observed| observed.effect()),
        Some(predecessor_effect)
    );
    assert!(late_predecessor.pane_intent().is_none());

    let foreign_winner = publish(
        &mut coordinator,
        focus_envelope(4, Authority::Known(GlobalFocusedWindow::Foreign)),
        5,
        current,
    );
    assert!(matches!(
        foreign_winner,
        FocusObservationTransition::Applied(_)
    ));
    assert!(matches!(
        coordinator.publish_pane_focus_observation(
            PaneFocusObservation::new(
                PaneFocusObservationGeneration::new(1),
                binding_b,
                PanelFocus::Item(ITEM_B),
            ),
            current_bindings(current),
            current_items,
        ),
        PaneFocusObservationTransition::Applied { .. }
    ));

    let later_user_focus = publish(
        &mut coordinator,
        focus_envelope(5, Authority::Known(GlobalFocusedWindow::Dock(binding_b))),
        6,
        current,
    );
    let FocusObservationTransition::Applied(later_user_focus) = later_user_focus else {
        panic!("focus after the newer Foreign winner must be ordinary platform authority");
    };
    let restored = later_user_focus
        .pane_intent()
        .expect("the newer Foreign winner must release the stale-effect barrier");
    assert_eq!(restored.target(), binding_b);
    assert_eq!(restored.focus(), PanelFocus::Item(ITEM_B));
    assert_eq!(restored.source(), PaneFocusIntentSource::PlatformActivation);
}

#[test]
fn newer_dock_focus_releases_an_older_target_quarantine() {
    let current = bindings([39, 40]);
    let [binding_a, binding_b] = current;
    let mut coordinator = ViewportFocusCoordinator::default();
    assert!(matches!(
        coordinator.publish_pane_focus_observation(
            PaneFocusObservation::new(
                PaneFocusObservationGeneration::new(1),
                binding_b,
                PanelFocus::Item(ITEM_B),
            ),
            current_bindings(current),
            current_items,
        ),
        PaneFocusObservationTransition::Applied { .. }
    ));
    let _ = publish(
        &mut coordinator,
        focus_envelope(1, Authority::Known(GlobalFocusedWindow::Dock(binding_a))),
        1,
        current,
    );

    let predecessor = coordinator
        .request_activation(
            ViewportActivationRequest::explicit(binding_b, PanelFocus::Item(ITEM_B)),
            focus_stamp(2),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("the explicit focus request must allocate");
    let predecessor_effect = EffectId::new(79);
    let _ = coordinator.attach_platform_focus_effect(predecessor.generation(), predecessor_effect);

    let _ = publish(
        &mut coordinator,
        focus_envelope(2, Authority::Known(GlobalFocusedWindow::None)),
        3,
        current,
    );
    let _ = publish(
        &mut coordinator,
        focus_envelope(3, Authority::Known(GlobalFocusedWindow::Dock(binding_b))),
        4,
        current,
    );

    let newer_dock_winner = publish(
        &mut coordinator,
        focus_envelope(4, Authority::Known(GlobalFocusedWindow::Dock(binding_a))),
        5,
        current,
    );
    assert!(matches!(
        newer_dock_winner,
        FocusObservationTransition::Applied(_)
    ));

    let later_user_focus = publish(
        &mut coordinator,
        focus_envelope(5, Authority::Known(GlobalFocusedWindow::Dock(binding_b))),
        6,
        current,
    );
    let FocusObservationTransition::Applied(later_user_focus) = later_user_focus else {
        panic!("focus after a newer Dock winner must be ordinary platform authority");
    };
    let restored = later_user_focus
        .pane_intent()
        .expect("the newer Dock winner must release the stale target quarantine");
    assert_eq!(restored.target(), binding_b);
    assert_eq!(restored.focus(), PanelFocus::Item(ITEM_B));
}

#[test]
fn pointer_winner_serializes_after_an_emitted_explicit_focus_hazard() {
    let current = bindings([29, 30]);
    let [binding_a, binding_b] = current;
    let mut coordinator = ViewportFocusCoordinator::default();
    let _ = publish(
        &mut coordinator,
        focus_envelope(1, Authority::Known(GlobalFocusedWindow::Dock(binding_a))),
        1,
        current,
    );

    let predecessor = coordinator
        .request_activation(
            ViewportActivationRequest::explicit(binding_b, PanelFocus::Item(ITEM_B)),
            focus_stamp(2),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("the explicit predecessor must allocate");
    let predecessor_effect = EffectId::new(74);
    let _ = coordinator.attach_platform_focus_effect(predecessor.generation(), predecessor_effect);

    let winner = coordinator
        .request_activation(
            ViewportActivationRequest::new(
                binding_a,
                PanelFocus::Item(ITEM_A),
                ViewportActivationCause::PointerTabGesture,
            ),
            focus_stamp(3),
            true,
            current_bindings(current),
            current_items,
        )
        .expect("the pointer winner must serialize after the emitted predecessor");
    assert_eq!(
        winner
            .superseded()
            .map(PendingViewportActivation::generation),
        Some(predecessor.generation())
    );
    assert_eq!(
        winner.outcome(),
        ActivationStartOutcome::RequestPlatformFocus { target: binding_a },
        "the request is an ordering barrier for the pointer winner, not inferred pointer focus"
    );
    let winner_effect = EffectId::new(75);
    let _ = coordinator.attach_platform_focus_effect(winner.generation(), winner_effect);

    let _ = publish(
        &mut coordinator,
        focus_envelope_with_ack(
            2,
            Authority::Known(GlobalFocusedWindow::Dock(binding_b)),
            predecessor_effect,
        ),
        4,
        current,
    );
    assert_eq!(
        coordinator
            .pending_activation()
            .map(PendingViewportActivation::generation),
        Some(winner.generation())
    );
    assert!(coordinator.pending_pane_intent().is_none());

    let completed = publish(
        &mut coordinator,
        focus_envelope_with_ack(
            3,
            Authority::Known(GlobalFocusedWindow::Dock(binding_a)),
            winner_effect,
        ),
        5,
        current,
    );
    let FocusObservationTransition::Applied(completed) = completed else {
        panic!("the ordered pointer winner must complete");
    };
    let intent = completed
        .pane_intent()
        .expect("the pointer winner must publish its exact pane intent");
    assert_eq!(intent.target(), binding_a);
    assert_eq!(intent.causal(), focus_stamp(3));
    assert_eq!(intent.source(), PaneFocusIntentSource::PointerTabGesture);
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
            focus_stamp(2),
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
            focus_stamp(2),
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
                PaneFocusObservationGeneration::new(2),
                binding_a,
                PanelFocus::Item(ITEM_A),
            )
            .acknowledging(intent.id()),
            current_bindings(current),
            current_items,
        ),
        PaneFocusObservationTransition::Rejected(
            PaneFocusObservationRejection::IntentNotPublished {
                intent: intent.id()
            }
        )
    );
    coordinator.mark_boundary_published();
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
    assert!(matches!(
        unknown,
        FocusObservationTransition::AuthorityRevoked {
            reason: FocusAuthorityRevocation::Unknown(
                AuthorityUnavailableReason::ProviderUnavailable
            ),
            ..
        }
    ));
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
            focus_stamp(2),
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
fn stale_binding_and_item_cleanup_remove_only_owned_state() {
    let current = bindings([71, 72]);
    let replacement = bindings([81, 82]);
    let [binding_a, _] = current;
    let mut coordinator = ViewportFocusCoordinator::default();
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
            focus_stamp(3),
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

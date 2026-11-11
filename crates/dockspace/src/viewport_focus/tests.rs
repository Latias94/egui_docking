//! Shared focus fixtures and domain regression modules.

use super::*;
use crate::close_plan::{CloseAuthority, CloseCoordinator, CloseItemRequirement, ClosePlanTarget};
use crate::effect::DispatchFailureReason;
use crate::ids::{EngineAuthorityDomainId, WorkspaceEpoch, WorkspaceRevision};
use crate::platform_provider::PlatformObservationAuthority;
use crate::policy::{CloseCapability, PolicyRevision};
use crate::transition::WorkspaceVersion;
use crate::viewport::{InventoryGeneration, WindowIncarnation, WindowToken};

fn binding() -> ViewportBinding {
    ViewportBinding::new(
        EngineAuthorityDomainId::new_for_test(1),
        WorkspaceEpoch::new(1),
        SurfaceId::new(1),
        WindowToken::new(1),
        WindowIncarnation::new(1),
    )
}

fn second_binding() -> ViewportBinding {
    ViewportBinding::new(
        EngineAuthorityDomainId::new_for_test(1),
        WorkspaceEpoch::new(1),
        SurfaceId::new(2),
        WindowToken::new(2),
        WindowIncarnation::new(2),
    )
}

fn third_binding() -> ViewportBinding {
    ViewportBinding::new(
        EngineAuthorityDomainId::new_for_test(1),
        WorkspaceEpoch::new(1),
        SurfaceId::new(3),
        WindowToken::new(3),
        WindowIncarnation::new(3),
    )
}

fn close_request() -> CloseRequestId {
    let domain = EngineAuthorityDomainId::new_for_test(1);
    let authority = CloseAuthority::new(
        domain,
        WorkspaceVersion::new(WorkspaceEpoch::new(1), WorkspaceRevision::new(1)),
        PolicyRevision::new(1),
    );
    let item = ItemId::new(7);
    let mut coordinator = CloseCoordinator::new(domain);
    coordinator
        .open(
            authority,
            ClosePlanTarget::Item { item },
            [CloseItemRequirement::new(item, CloseCapability::Immediate)],
            (),
        )
        .expect("test close plan must open")
        .request()
}

fn focus_tombstone(generation: u64) -> FocusObservationEnvelope {
    unknown_focus_observation(
        FocusObservationGeneration::new(generation),
        AuthorityUnavailableReason::NotReported,
    )
}

fn focus_stamp(generation: u64) -> FocusCausalStamp {
    FocusCausalStamp::new(
        PaneFocusIntentGeneration::new(generation),
        ReductionCause::SurfaceContributionBatch {
            tick: crate::ids::ReducerTickId::new(generation),
        },
    )
}

fn focus_observation(generation: u64, focused: GlobalFocusedWindow) -> FocusObservationEnvelope {
    FocusObservationEnvelope::new(
        FocusObservationGeneration::new(generation),
        Authority::Known(focused),
        Authority::Known(None),
    )
}

fn publish_platform_focus(
    coordinator: &mut ViewportFocusCoordinator,
    observation: FocusObservationEnvelope,
    pane_generation: u64,
) -> FocusObservationTransition {
    let bindings = [binding(), second_binding()];
    coordinator
        .publish_platform_focus_observation(
            observation,
            focus_stamp(pane_generation),
            |candidate| bindings.contains(&candidate),
            |candidate| bindings.contains(&candidate),
            |_| false,
            |surface, item| {
                (surface == SurfaceId::new(1) && item == ItemId::new(1))
                    || (surface == SurfaceId::new(2) && item == ItemId::new(2))
            },
        )
        .expect("focus identity domains must remain available")
}

fn pending_lifecycle_focus_barrier() -> (
    ViewportFocusCoordinator,
    ViewportBinding,
    ViewportBinding,
    ActivationStart,
    EffectId,
) {
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
        focus_observation(1, GlobalFocusedWindow::Foreign),
        1,
    );
    let older_request = ViewportActivationRequest::tear_off_committed(
        older,
        PaneFocusDisposition::Set(ItemId::new(1)),
    );
    assert!(coordinator.reserve_activation_causal(
        NativeCreateSagaId::new(1),
        focus_stamp(2),
        older_request,
    ));
    assert!(
        coordinator.admit_pane_causal_claim(PaneCausalClaim::from_request(
            focus_stamp(3),
            ViewportActivationRequest::pointer_tab_gesture(
                winner,
                PanelFocus::Item(ItemId::new(2)),
            ),
        ))
    );
    let old_completion = coordinator
        .request_reserved_activation(
            NativeCreateSagaId::new(1),
            older_request,
            focus_stamp(2),
            true,
            binding_is_live,
            item_is_live,
        )
        .expect("the old completion must reduce to causal suppression");
    assert!(matches!(
        old_completion.outcome(),
        ActivationStartOutcome::Suppressed(ActivationSuppression::CausallySuperseded { .. })
    ));
    let barrier = coordinator
        .request_winning_lifecycle_compensation(true, binding_is_live, item_is_live)
        .expect("the lifecycle barrier must allocate")
        .expect("the winner must remain addressable");
    let effect = EffectId::new(42);
    assert_eq!(
        coordinator.attach_platform_focus_effect(barrier.generation(), effect),
        FocusEffectAttachment::Applied
    );
    (coordinator, older, winner, barrier, effect)
}

fn assert_installed_focus_delta(delta: &FocusDelta, intent: PaneFocusIntent, effect: EffectId) {
    assert!(delta.global_observation().is_some());
    assert_eq!(
        delta
            .pane_intent()
            .and_then(|change| change.after().as_ref())
            .copied(),
        Some(intent)
    );
    assert_eq!(delta.surface_focus().len(), 1);
    assert_eq!(delta.effects().len(), 1);
    assert_eq!(delta.effects()[0].request().id(), effect);
    assert_eq!(delta.effects()[0].phase().before(), &None);
    assert_eq!(
        delta.effects()[0].phase().after(),
        &Some(EffectPhase::Requested)
    );
}

fn assert_settled_focus_delta(
    delta: &FocusDelta,
    intent: PaneFocusIntent,
    observed: ObservedPlatformFocusEffect,
    inventory_generation: InventoryGeneration,
) {
    assert_eq!(
        delta
            .pane_intent()
            .and_then(|change| change.before().as_ref())
            .copied(),
        Some(intent)
    );
    assert_eq!(
        delta
            .pane_intent()
            .and_then(|change| change.after().as_ref()),
        None
    );
    assert_eq!(delta.effects()[0].observed(), Some(observed));
    assert_eq!(
        delta.effects()[0].phase().after(),
        &Some(EffectPhase::ObservedApplied {
            inventory_generation,
        })
    );
}

fn shared_window_sibling() -> ViewportBinding {
    ViewportBinding::new(
        binding().authority_domain(),
        binding().epoch(),
        SurfaceId::new(2),
        binding().token(),
        binding().incarnation(),
    )
}

mod activation;
mod lifecycle;
mod observation;
mod pane;

use super::*;
use crate::ids::{WorkspaceEpoch, WorkspaceRevision};
use crate::viewport::{WindowIncarnation, WindowToken};

const ITEM_A: ItemId = ItemId::new(11);
const ITEM_B: ItemId = ItemId::new(12);
const ITEM_C: ItemId = ItemId::new(13);
const ROOT: RootId = RootId::new(21);
const SURFACE: SurfaceId = SurfaceId::new(31);
const TARGET_SURFACE: SurfaceId = SurfaceId::new(32);

fn domain() -> EngineAuthorityDomainId {
    EngineAuthorityDomainId::new_for_test(7)
}

fn authority(workspace_revision: u64, policy_revision: u64) -> CloseAuthority {
    authority_in_domain(domain(), workspace_revision, policy_revision)
}

fn authority_in_domain(
    authority_domain: EngineAuthorityDomainId,
    workspace_revision: u64,
    policy_revision: u64,
) -> CloseAuthority {
    CloseAuthority::new(
        authority_domain,
        WorkspaceVersion::new(
            WorkspaceEpoch::new(7),
            WorkspaceRevision::new(workspace_revision),
        ),
        PolicyRevision::new(policy_revision),
    )
}

fn coordinator<P>() -> CloseCoordinator<P> {
    CloseCoordinator::new(domain())
}

fn requirement(item: ItemId, capability: CloseCapability) -> CloseItemRequirement {
    CloseItemRequirement::new(item, capability)
}

fn native_binding() -> ViewportBinding {
    ViewportBinding::new(
        domain(),
        WorkspaceEpoch::new(7),
        SURFACE,
        WindowToken::new(41),
        WindowIncarnation::new(51),
    )
}

fn native_edge() -> NativeCloseEdge {
    NativeCloseEdge::from_authoritative_requested(
        domain(),
        native_binding(),
        CloseObservationGeneration::new(7),
        InventoryGeneration::new(7),
    )
}

fn emit_native<P>(
    coordinator: &mut CloseCoordinator<P>,
    request: CloseRequestId,
) -> CloseAdvanceOutcome {
    coordinator.mark_effect_emitted(
        request,
        authority(3, 5),
        native_binding(),
        EffectId::new(101),
        CloseObservationGeneration::new(8),
        InventoryGeneration::new(8),
        None,
    )
}

fn emit_cancellation<P>(
    coordinator: &mut CloseCoordinator<P>,
    request: CloseRequestId,
) -> CloseAdvanceOutcome {
    coordinator.mark_cancellation_effect_emitted(
        request,
        authority(3, 5),
        native_binding(),
        EffectId::new(102),
        CloseObservationGeneration::new(10),
        InventoryGeneration::new(10),
        Some(EffectId::new(101)),
    )
}

fn apply_destroyed<P>(
    coordinator: &mut CloseCoordinator<P>,
    request: CloseRequestId,
    close_authority: CloseAuthority,
) -> CloseAdvanceOutcome {
    coordinator.settle_native(
        request,
        close_authority,
        CloseNativeSettlement::DestroyedProved(valid_destroyed_proof(request)),
    )
}

fn destroyed_proof(
    request: CloseRequestId,
    binding: ViewportBinding,
    generation: u64,
) -> CloseDestroyedProof {
    destroyed_proof_acknowledging(request, binding, generation, EffectId::new(101))
}

fn destroyed_proof_acknowledging(
    request: CloseRequestId,
    binding: ViewportBinding,
    generation: u64,
    acknowledged_effect: EffectId,
) -> CloseDestroyedProof {
    destroyed_proof_at(
        request,
        binding,
        generation,
        generation,
        acknowledged_effect,
    )
}

fn destroyed_proof_at(
    request: CloseRequestId,
    binding: ViewportBinding,
    observation_generation: u64,
    inventory_generation: u64,
    acknowledged_effect: EffectId,
) -> CloseDestroyedProof {
    CloseDestroyedProof::from_authoritative_destroyed(
        request,
        binding,
        CloseObservationGeneration::new(observation_generation),
        InventoryGeneration::new(inventory_generation),
        acknowledged_effect,
    )
}

fn valid_destroyed_proof(request: CloseRequestId) -> CloseDestroyedProof {
    destroyed_proof(request, native_binding(), 11)
}

fn cancellation_proof(
    request: CloseRequestId,
    binding: ViewportBinding,
    close_effect: EffectId,
    cancel_effect: EffectId,
    generation: u64,
) -> CloseCancellationProof {
    cancellation_proof_at(
        request,
        binding,
        Some(close_effect),
        cancel_effect,
        generation,
        generation,
    )
}

fn cancellation_proof_at(
    request: CloseRequestId,
    binding: ViewportBinding,
    close_effect: Option<EffectId>,
    cancel_effect: EffectId,
    observation_generation: u64,
    inventory_generation: u64,
) -> CloseCancellationProof {
    CloseCancellationProof::from_authoritative_live_close_cleared(
        request,
        binding,
        close_effect,
        Some(cancel_effect),
        Some(cancel_effect),
        CloseObservationGeneration::new(observation_generation),
        InventoryGeneration::new(inventory_generation),
    )
}

fn rehome_request() -> SurfaceCloseRequest {
    SurfaceCloseRequest::RehomeAll {
        target: SurfaceRehomeTarget::new(TARGET_SURFACE, None, Vec::new()),
    }
}

fn valid_cancellation_proof(request: CloseRequestId) -> CloseCancellationProof {
    cancellation_proof(
        request,
        native_binding(),
        EffectId::new(101),
        EffectId::new(102),
        11,
    )
}

fn approved_retain_surface_plan<P>(
    coordinator: &mut CloseCoordinator<P>,
    prepared: P,
) -> ClosePlan {
    let plan = coordinator
        .open_surface(
            authority(3, 5),
            native_edge(),
            SurfaceCloseRequest::RetainLayout,
            [],
            prepared,
        )
        .expect("retain-layout plan needs no content decision roster");
    coordinator
        .plan(plan.request())
        .cloned()
        .expect("approved retain plan remains available")
}

fn cancellation_ready_surface_plan(
    coordinator: &mut CloseCoordinator<&'static str>,
) -> CloseRequestId {
    let plan = approved_retain_surface_plan(coordinator, "complete-roster");
    let request = plan.request();
    emit_native(coordinator, request);
    coordinator.request_cancel(request, authority(3, 5));
    emit_cancellation(coordinator, request);
    request
}

fn vetoed_surface_plan(coordinator: &mut CloseCoordinator<&'static str>) -> CloseRequestId {
    let plan = coordinator
        .open_surface(
            authority(3, 5),
            native_edge(),
            SurfaceCloseRequest::CloseContent,
            [requirement(ITEM_A, CloseCapability::Immediate)],
            "complete-roster",
        )
        .expect("surface content-close plan needs one decision");
    let request = plan.request();
    assert_eq!(
        coordinator.resolve(
            request,
            plan.items()[0].token(),
            authority(3, 5),
            CloseDecision::Veto,
        ),
        Ok(CloseResolutionOutcome::Vetoed {
            request,
            item: ITEM_A,
        })
    );
    request
}

fn root_plan(
    coordinator: &mut CloseCoordinator<&'static str>,
    requirements: impl IntoIterator<Item = CloseItemRequirement>,
) -> ClosePlan {
    coordinator
        .open(
            authority(3, 5),
            ClosePlanTarget::Root { root: ROOT },
            requirements,
            "frozen-root-fingerprint",
        )
        .expect("valid root close plan")
}

#[test]
fn published_terminal_close_plans_compact_without_losing_replay_classification() {
    let mut coordinator = coordinator();
    let mut last = None;

    for _ in 0..10_000 {
        let plan = root_plan(
            &mut coordinator,
            [requirement(ITEM_A, CloseCapability::Immediate)],
        );
        let request = plan.request();
        assert_eq!(
            coordinator.resolve(
                request,
                plan.items()[0].token(),
                authority(3, 5),
                CloseDecision::Allow,
            ),
            Ok(CloseResolutionOutcome::Approved { request }),
        );
        assert!(matches!(
            coordinator.mark_local_applied(request, authority(3, 5)),
            CloseAdvanceOutcome::Advanced {
                to: ClosePlanPhase::Applied,
                ..
            }
        ));
        last = Some((request, plan.items()[0].token()));
    }

    coordinator.mark_boundary_published();
    let retention = coordinator.retention_manifest();
    assert_eq!(retention.active_plans(), 0);
    assert_eq!(retention.terminal_plan_guards(), 10_000);
    assert_eq!(retention.decision_token_guards(), 10_000);
    assert_eq!(retention.deferred_token_guards(), 0);
    assert_eq!(retention.retained_structure_count(), 20_000);
    assert_eq!(coordinator.compact_published_terminal(), 10_000);
    let compacted = coordinator.retention_manifest();
    assert_eq!(compacted.active_plans(), 0);
    assert_eq!(compacted.terminal_plan_guards(), 0);
    assert_eq!(compacted.decision_token_guards(), 0);
    assert_eq!(compacted.deferred_token_guards(), 0);
    assert_eq!(compacted.retained_structure_count(), 0);

    let (request, token) = last.expect("the test creates terminal plans");
    assert_eq!(
        coordinator.resolve(request, token, authority(3, 5), CloseDecision::Allow,),
        Ok(CloseResolutionOutcome::Inert(
            CloseInertReason::RetiredTerminal { request },
        )),
    );
}

#[test]
fn retention_accounts_for_live_decision_and_deferred_token_indexes() {
    let mut coordinator = coordinator();
    let plan = root_plan(
        &mut coordinator,
        [requirement(ITEM_A, CloseCapability::DeferredAllowed)],
    );
    let request = plan.request();
    assert!(matches!(
        coordinator.resolve(
            request,
            plan.items()[0].token(),
            authority(3, 5),
            CloseDecision::Deferred,
        ),
        Ok(CloseResolutionOutcome::Deferred { .. })
    ));

    let retention = coordinator.retention_manifest();
    assert_eq!(retention.active_plans(), 1);
    assert_eq!(retention.terminal_plan_guards(), 0);
    assert_eq!(retention.decision_token_guards(), 1);
    assert_eq!(retention.deferred_token_guards(), 1);
    assert_eq!(retention.retained_structure_count(), 3);
}

#[test]
fn identities_are_non_zero_monotonic_and_item_order_is_stable() {
    let mut coordinator = coordinator();
    let first = root_plan(
        &mut coordinator,
        [
            requirement(ITEM_C, CloseCapability::Immediate),
            requirement(ITEM_A, CloseCapability::DeferredAllowed),
            requirement(ITEM_B, CloseCapability::Immediate),
        ],
    );
    let second = root_plan(
        &mut coordinator,
        [requirement(ITEM_A, CloseCapability::Immediate)],
    );

    assert_ne!(first.request().sequence(), 0);
    assert!(second.request().sequence() > first.request().sequence());
    assert_eq!(first.request().domain(), domain());
    assert_eq!(
        first
            .items()
            .iter()
            .map(|item| item.item())
            .collect::<Vec<_>>(),
        vec![ITEM_C, ITEM_A, ITEM_B]
    );
    let tokens = first
        .items()
        .iter()
        .map(|item| item.token().sequence())
        .collect::<Vec<_>>();
    assert!(tokens.iter().all(|token| *token != 0));
    assert!(tokens.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(second.items()[0].token().sequence() > tokens[2]);
    assert_eq!(first.items()[0].token().domain(), domain());
}

#[test]
fn active_target_lookup_reuses_only_current_non_terminal_plan() {
    let mut coordinator = coordinator();
    let target = ClosePlanTarget::Root { root: ROOT };
    let first = root_plan(
        &mut coordinator,
        [requirement(ITEM_A, CloseCapability::Immediate)],
    );

    assert_eq!(coordinator.clone(), coordinator);
    assert_eq!(
        coordinator
            .active_plan_for_target(target, authority(3, 5))
            .map(ClosePlan::request),
        Some(first.request())
    );
    assert!(
        coordinator
            .active_plan_for_target(target, authority(3, 6))
            .is_none()
    );

    coordinator
        .resolve(
            first.request(),
            first.items()[0].token(),
            authority(3, 5),
            CloseDecision::Veto,
        )
        .expect("veto does not allocate a continuation");
    assert!(
        coordinator
            .active_plan_for_target(target, authority(3, 5))
            .is_none()
    );

    let second = root_plan(
        &mut coordinator,
        [requirement(ITEM_B, CloseCapability::Immediate)],
    );
    assert_eq!(
        coordinator
            .active_plan_for_target(target, authority(3, 5))
            .map(ClosePlan::request),
        Some(second.request())
    );
    assert_eq!(
        coordinator.invalidate_stale(authority(4, 5)),
        vec![second.request()]
    );
    assert!(
        coordinator
            .active_plan_for_target(target, authority(3, 5))
            .is_none()
    );
    assert!(
        coordinator
            .active_plan_for_target(target, authority(4, 5))
            .is_none()
    );
}

#[test]
fn decisions_may_arrive_out_of_order_but_only_the_last_allow_approves() {
    let mut coordinator = coordinator();
    let plan = root_plan(
        &mut coordinator,
        [
            requirement(ITEM_A, CloseCapability::Immediate),
            requirement(ITEM_B, CloseCapability::Immediate),
            requirement(ITEM_C, CloseCapability::Immediate),
        ],
    );
    let request = plan.request();
    let tokens = plan
        .items()
        .iter()
        .map(|item| item.token())
        .collect::<Vec<_>>();

    assert_eq!(
        coordinator.resolve(request, tokens[2], authority(3, 5), CloseDecision::Allow),
        Ok(CloseResolutionOutcome::Recorded {
            request,
            item: ITEM_C,
            phase: ClosePlanPhase::Resolving,
        })
    );
    assert_eq!(
        coordinator.resolve(request, tokens[0], authority(3, 5), CloseDecision::Allow),
        Ok(CloseResolutionOutcome::Recorded {
            request,
            item: ITEM_A,
            phase: ClosePlanPhase::Resolving,
        })
    );
    assert!(matches!(
        coordinator.approved(request, authority(3, 5)),
        Err(CloseInertReason::PhaseMismatch { .. })
    ));
    assert_eq!(
        coordinator.resolve(request, tokens[1], authority(3, 5), CloseDecision::Allow),
        Ok(CloseResolutionOutcome::Approved { request })
    );
    let approved = coordinator
        .approved(request, authority(3, 5))
        .expect("last vote exposes prepared close exactly once for commit");
    assert_eq!(approved.plan().phase(), ClosePlanPhase::Approved);
    assert_eq!(*approved.prepared(), "frozen-root-fingerprint");
}

#[test]
fn duplicate_initial_decision_is_inert_and_cannot_replay_approval() {
    let mut coordinator = coordinator();
    let plan = root_plan(
        &mut coordinator,
        [requirement(ITEM_A, CloseCapability::Immediate)],
    );
    let request = plan.request();
    let token = plan.items()[0].token();

    assert_eq!(
        coordinator.resolve(request, token, authority(3, 5), CloseDecision::Allow),
        Ok(CloseResolutionOutcome::Approved { request })
    );
    assert_eq!(
        coordinator.resolve(request, token, authority(3, 5), CloseDecision::Allow),
        Ok(CloseResolutionOutcome::Inert(
            CloseInertReason::DuplicateDecision {
                request,
                item: ITEM_A,
                state: CloseItemDecisionState::Allowed,
            }
        ))
    );
    assert_eq!(
        coordinator.plan(request).map(ClosePlan::phase),
        Some(ClosePlanPhase::Approved)
    );
}

#[test]
fn deferred_decision_mints_one_continuation_and_consumes_it_once() {
    let mut coordinator = coordinator();
    let plan = root_plan(
        &mut coordinator,
        [requirement(ITEM_A, CloseCapability::DeferredAllowed)],
    );
    let request = plan.request();
    let initial = plan.items()[0].token();
    let first = coordinator
        .resolve(request, initial, authority(3, 5), CloseDecision::Deferred)
        .expect("continuation allocation succeeds");
    let CloseResolutionOutcome::Deferred { continuation, .. } = first else {
        panic!("expected deferred continuation, got {first:?}");
    };

    assert_ne!(continuation.sequence(), 0);
    assert_eq!(
        coordinator
            .plan(request)
            .map(|current| current.items()[0].state()),
        Some(CloseItemDecisionState::Deferred { continuation })
    );
    assert_eq!(
        coordinator.resolve(request, initial, authority(3, 5), CloseDecision::Deferred),
        Ok(CloseResolutionOutcome::Inert(
            CloseInertReason::DuplicateDecision {
                request,
                item: ITEM_A,
                state: CloseItemDecisionState::Deferred { continuation },
            }
        ))
    );
    assert_eq!(
        coordinator.continue_deferred(
            request,
            continuation,
            authority(3, 5),
            DeferredCloseDecision::Allow,
        ),
        CloseResolutionOutcome::Approved { request }
    );
    assert_eq!(
        coordinator
            .plan(request)
            .map(|current| current.items()[0].state()),
        Some(CloseItemDecisionState::Allowed)
    );
    assert_eq!(
        coordinator.continue_deferred(
            request,
            continuation,
            authority(3, 5),
            DeferredCloseDecision::Allow,
        ),
        CloseResolutionOutcome::Inert(CloseInertReason::DuplicateDeferredContinuation {
            request,
            item: ITEM_A,
            state: CloseItemDecisionState::Allowed,
        })
    );
}

#[test]
fn immediate_capability_rejects_deferred_without_consuming_initial_token() {
    let mut coordinator = coordinator();
    let plan = root_plan(
        &mut coordinator,
        [requirement(ITEM_A, CloseCapability::Immediate)],
    );
    let request = plan.request();
    let token = plan.items()[0].token();

    assert_eq!(
        coordinator.resolve(request, token, authority(3, 5), CloseDecision::Deferred),
        Ok(CloseResolutionOutcome::Inert(
            CloseInertReason::DeferredNotAllowed {
                request,
                item: ITEM_A,
                capability: CloseCapability::Immediate,
            }
        ))
    );
    assert_eq!(
        coordinator
            .plan(request)
            .map(|current| current.items()[0].state()),
        Some(CloseItemDecisionState::Pending)
    );
    assert_eq!(
        coordinator.resolve(request, token, authority(3, 5), CloseDecision::Allow),
        Ok(CloseResolutionOutcome::Approved { request })
    );
}

#[test]
fn veto_is_terminal_and_late_votes_are_inert() {
    let mut coordinator = coordinator();
    let plan = root_plan(
        &mut coordinator,
        [
            requirement(ITEM_A, CloseCapability::Immediate),
            requirement(ITEM_B, CloseCapability::Immediate),
        ],
    );
    let request = plan.request();

    assert_eq!(
        coordinator.resolve(
            request,
            plan.items()[1].token(),
            authority(3, 5),
            CloseDecision::Veto,
        ),
        Ok(CloseResolutionOutcome::Vetoed {
            request,
            item: ITEM_B,
        })
    );
    assert_eq!(
        coordinator.resolve(
            request,
            plan.items()[0].token(),
            authority(3, 5),
            CloseDecision::Allow,
        ),
        Ok(CloseResolutionOutcome::Inert(CloseInertReason::Terminal {
            request,
            phase: ClosePlanPhase::Vetoed,
        }))
    );
    assert!(matches!(
        coordinator.approved(request, authority(3, 5)),
        Err(CloseInertReason::Terminal {
            phase: ClosePlanPhase::Vetoed,
            ..
        })
    ));
}

#[test]
fn authority_change_marks_plan_stale_and_never_reinterprets_old_vote() {
    let mut coordinator = coordinator();
    let plan = root_plan(
        &mut coordinator,
        [requirement(ITEM_A, CloseCapability::Immediate)],
    );
    let request = plan.request();
    let token = plan.items()[0].token();
    let actual = authority(4, 5);

    assert_eq!(
        coordinator.resolve(request, token, actual, CloseDecision::Allow),
        Ok(CloseResolutionOutcome::Inert(
            CloseInertReason::AuthorityStale {
                request,
                expected: authority(3, 5),
                actual,
            }
        ))
    );
    assert_eq!(
        coordinator.plan(request).map(ClosePlan::phase),
        Some(ClosePlanPhase::Stale)
    );
    assert_eq!(
        coordinator.resolve(request, token, authority(3, 5), CloseDecision::Allow),
        Ok(CloseResolutionOutcome::Inert(CloseInertReason::Terminal {
            request,
            phase: ClosePlanPhase::Stale,
        }))
    );
}

#[test]
fn surface_authority_drift_becomes_a_cancellable_native_obligation() {
    let mut coordinator = coordinator();
    let plan = approved_retain_surface_plan(&mut coordinator, "complete-roster");
    let request = plan.request();
    let newer_authority = authority(4, 6);

    assert_eq!(
        coordinator.mark_effect_emitted(
            request,
            newer_authority,
            native_binding(),
            EffectId::new(101),
            CloseObservationGeneration::new(8),
            InventoryGeneration::new(8),
            None,
        ),
        CloseAdvanceOutcome::Inert(CloseInertReason::AuthorityStale {
            request,
            expected: authority(3, 5),
            actual: newer_authority,
        })
    );
    assert_eq!(
        coordinator.plan(request).map(|current| (
            current.phase(),
            current.destruction_state(),
            current.cancellation_state(),
            current.is_terminal(),
        )),
        Some((
            ClosePlanPhase::CancelRequested,
            CloseDestructionState::None,
            CloseCancellationState::Requested,
            false,
        ))
    );

    assert_eq!(
        coordinator.mark_cancellation_effect_emitted(
            request,
            newer_authority,
            native_binding(),
            EffectId::new(102),
            CloseObservationGeneration::new(8),
            InventoryGeneration::new(8),
            None,
        ),
        CloseAdvanceOutcome::Advanced {
            request,
            from: ClosePlanPhase::CancelRequested,
            to: ClosePlanPhase::CancelRequested,
        }
    );
    let proof = CloseCancellationProof::from_authoritative_live_close_cleared(
        request,
        native_binding(),
        None,
        Some(EffectId::new(102)),
        Some(EffectId::new(102)),
        CloseObservationGeneration::new(9),
        InventoryGeneration::new(9),
    );
    assert_eq!(
        coordinator.settle_native(
            request,
            newer_authority,
            CloseNativeSettlement::CancellationProved(proof),
        ),
        CloseAdvanceOutcome::Advanced {
            request,
            from: ClosePlanPhase::CancelRequested,
            to: ClosePlanPhase::Cancelled,
        }
    );
}

#[test]
fn invalidation_authority_drift_cancels_a_vetoed_surface_plan() {
    let mut coordinator = coordinator();
    let request = vetoed_surface_plan(&mut coordinator);
    let newer_authority = authority(4, 6);

    assert_eq!(coordinator.invalidate_stale(newer_authority), vec![request]);
    assert_eq!(
        coordinator.plan(request).map(|current| (
            current.phase(),
            current.destruction_state(),
            current.cancellation_state(),
            current.is_terminal(),
        )),
        Some((
            ClosePlanPhase::CancelRequested,
            CloseDestructionState::None,
            CloseCancellationState::Requested,
            false,
        ))
    );
    assert!(coordinator.invalidate_stale(newer_authority).is_empty());
}

#[test]
fn guarded_authority_drift_cancels_a_vetoed_surface_plan() {
    let mut coordinator = coordinator();
    let request = vetoed_surface_plan(&mut coordinator);
    let newer_authority = authority(4, 6);

    assert_eq!(
        coordinator.request_cancel(request, newer_authority),
        CloseAdvanceOutcome::Inert(CloseInertReason::AuthorityStale {
            request,
            expected: authority(3, 5),
            actual: newer_authority,
        })
    );
    assert_eq!(
        coordinator.plan(request).map(|current| (
            current.phase(),
            current.destruction_state(),
            current.cancellation_state(),
            current.is_terminal(),
        )),
        Some((
            ClosePlanPhase::CancelRequested,
            CloseDestructionState::None,
            CloseCancellationState::Requested,
            false,
        ))
    );
}

#[test]
fn authority_drift_does_not_reclassify_effect_emitted_or_indeterminate_surface_plans() {
    let mut coordinator = coordinator();
    let plan = approved_retain_surface_plan(&mut coordinator, "complete-roster");
    let request = plan.request();
    let newer_authority = authority(4, 6);
    emit_native(&mut coordinator, request);

    assert!(coordinator.invalidate_stale(newer_authority).is_empty());
    assert_eq!(
        coordinator.plan(request).map(ClosePlan::phase),
        Some(ClosePlanPhase::EffectEmitted)
    );
    assert_eq!(
        coordinator.mark_indeterminate(request, authority(3, 5)),
        CloseAdvanceOutcome::Advanced {
            request,
            from: ClosePlanPhase::EffectEmitted,
            to: ClosePlanPhase::Indeterminate,
        }
    );
    assert!(coordinator.invalidate_stale(newer_authority).is_empty());
    assert_eq!(
        coordinator.plan(request).map(|current| (
            current.phase(),
            current.destruction_state(),
            current.cancellation_state(),
        )),
        Some((
            ClosePlanPhase::Indeterminate,
            CloseDestructionState::EffectEmitted,
            CloseCancellationState::None,
        ))
    );
}

#[test]
fn foreign_domains_are_inert_before_effect_without_staling_local_plan() {
    let mut coordinator = coordinator();
    let plan = root_plan(
        &mut coordinator,
        [requirement(ITEM_A, CloseCapability::Immediate)],
    );
    let foreign_domain = EngineAuthorityDomainId::new_for_test(99);
    let foreign_authority = authority_in_domain(foreign_domain, 3, 5);

    assert_eq!(
        coordinator.resolve(
            plan.request(),
            plan.items()[0].token(),
            foreign_authority,
            CloseDecision::Allow,
        ),
        Ok(CloseResolutionOutcome::Inert(
            CloseInertReason::AuthorityDomainMismatch {
                request: plan.request(),
                expected: domain(),
                actual: foreign_domain,
            }
        ))
    );
    assert!(coordinator.invalidate_stale(foreign_authority).is_empty());
    assert_eq!(
        coordinator
            .plan(plan.request())
            .map(|current| (current.phase(), current.items()[0].state(),)),
        Some((ClosePlanPhase::Requested, CloseItemDecisionState::Pending))
    );

    let mut foreign_coordinator = CloseCoordinator::new(foreign_domain);
    let foreign_plan = foreign_coordinator
        .open(
            foreign_authority,
            ClosePlanTarget::Root { root: ROOT },
            [requirement(ITEM_A, CloseCapability::Immediate)],
            "foreign-root-fingerprint",
        )
        .expect("foreign fixture uses internally consistent authority");
    assert_eq!(
        coordinator.resolve(
            foreign_plan.request(),
            plan.items()[0].token(),
            authority(3, 5),
            CloseDecision::Allow,
        ),
        Ok(CloseResolutionOutcome::Inert(
            CloseInertReason::RequestDomainMismatch {
                request: foreign_plan.request(),
                expected: domain(),
                actual: foreign_domain,
            }
        ))
    );
    assert_eq!(
        coordinator.plan(plan.request()).map(ClosePlan::phase),
        Some(ClosePlanPhase::Requested)
    );
}

#[test]
fn rejected_final_commit_explicitly_stales_prepared_plan_once() {
    let mut coordinator = coordinator();
    let plan = root_plan(
        &mut coordinator,
        [requirement(ITEM_A, CloseCapability::Immediate)],
    );
    let request = plan.request();
    coordinator
        .resolve(
            request,
            plan.items()[0].token(),
            authority(3, 5),
            CloseDecision::Allow,
        )
        .expect("allow is infallible");

    assert_eq!(
        coordinator.mark_stale(request, authority(3, 5)),
        CloseAdvanceOutcome::Advanced {
            request,
            from: ClosePlanPhase::Approved,
            to: ClosePlanPhase::Stale,
        }
    );
    assert_eq!(
        coordinator.mark_stale(request, authority(3, 5)),
        CloseAdvanceOutcome::Inert(CloseInertReason::Terminal {
            request,
            phase: ClosePlanPhase::Stale,
        })
    );
}

#[test]
fn rejected_final_commit_cancels_irreversible_surface_obligation() {
    let mut coordinator = coordinator();
    let plan = approved_retain_surface_plan(&mut coordinator, "complete-roster");
    let request = plan.request();
    emit_native(&mut coordinator, request);
    assert_eq!(
        coordinator
            .plan(request)
            .map(|current| (current.destruction_state(), current.cancellation_state())),
        Some((
            CloseDestructionState::EffectEmitted,
            CloseCancellationState::None,
        ))
    );

    assert_eq!(
        coordinator.mark_stale(request, authority(3, 5)),
        CloseAdvanceOutcome::Advanced {
            request,
            from: ClosePlanPhase::EffectEmitted,
            to: ClosePlanPhase::CancelRequested,
        }
    );
    assert_eq!(
        coordinator.plan(request).map(|current| (
            current.phase(),
            current.destruction_state(),
            current.cancellation_state(),
        )),
        Some((
            ClosePlanPhase::CancelRequested,
            CloseDestructionState::EffectEmitted,
            CloseCancellationState::Requested,
        ))
    );
}

#[test]
fn a_token_owned_by_another_request_is_typed_and_inert() {
    let mut coordinator = coordinator();
    let first = root_plan(
        &mut coordinator,
        [requirement(ITEM_A, CloseCapability::Immediate)],
    );
    let second = root_plan(
        &mut coordinator,
        [requirement(ITEM_B, CloseCapability::Immediate)],
    );

    assert_eq!(
        coordinator.resolve(
            first.request(),
            second.items()[0].token(),
            authority(3, 5),
            CloseDecision::Allow,
        ),
        Ok(CloseResolutionOutcome::Inert(
            CloseInertReason::WrongDecisionToken {
                request: first.request(),
                token: second.items()[0].token(),
                owner: second.request(),
            }
        ))
    );
    assert_eq!(
        coordinator
            .plan(first.request())
            .map(|current| current.items()[0].state()),
        Some(CloseItemDecisionState::Pending)
    );
}

#[test]
fn local_commit_requires_approval_and_applies_once() {
    let mut coordinator = coordinator();
    let plan = root_plan(
        &mut coordinator,
        [requirement(ITEM_A, CloseCapability::Immediate)],
    );
    let request = plan.request();

    assert!(matches!(
        coordinator.mark_local_applied(request, authority(3, 5)),
        CloseAdvanceOutcome::Inert(CloseInertReason::PhaseMismatch { .. })
    ));
    coordinator
        .resolve(
            request,
            plan.items()[0].token(),
            authority(3, 5),
            CloseDecision::Allow,
        )
        .expect("allow is infallible");
    assert_eq!(
        coordinator.mark_local_applied(request, authority(3, 5)),
        CloseAdvanceOutcome::Advanced {
            request,
            from: ClosePlanPhase::Approved,
            to: ClosePlanPhase::Applied,
        }
    );
    assert_eq!(
        coordinator.mark_local_applied(request, authority(3, 5)),
        CloseAdvanceOutcome::Inert(CloseInertReason::Terminal {
            request,
            phase: ClosePlanPhase::Applied,
        })
    );
}

#[test]
fn native_surface_requires_effect_then_destroyed_authority() {
    let mut coordinator = coordinator();
    let plan = coordinator
        .open_surface(
            authority(3, 5),
            native_edge(),
            rehome_request(),
            [],
            "complete-roster-and-target-proof",
        )
        .expect("rehome does not close content items");
    let request = plan.request();
    assert_eq!(plan.phase(), ClosePlanPhase::Approved);
    assert_eq!(coordinator.native_edge(request), Some(native_edge()));
    assert_eq!(
        coordinator.surface_request_for_edge(native_edge()),
        Some(request)
    );

    assert_eq!(
        apply_destroyed(&mut coordinator, request, authority(3, 5)),
        CloseAdvanceOutcome::Inert(CloseInertReason::CloseEffectNotEmitted { request })
    );
    assert_eq!(
        emit_native(&mut coordinator, request),
        CloseAdvanceOutcome::Advanced {
            request,
            from: ClosePlanPhase::Approved,
            to: ClosePlanPhase::EffectEmitted,
        }
    );
    assert_eq!(
        apply_destroyed(&mut coordinator, request, authority(3, 5)),
        CloseAdvanceOutcome::Advanced {
            request,
            from: ClosePlanPhase::EffectEmitted,
            to: ClosePlanPhase::Applied,
        }
    );
}

#[test]
fn native_close_effect_requires_the_edge_causal_fence_and_an_empty_predecessor() {
    let mut coordinator = coordinator();
    let plan = approved_retain_surface_plan(&mut coordinator, "complete-roster");
    let request = plan.request();
    let cases = [
        (
            CloseObservationGeneration::new(6),
            InventoryGeneration::new(8),
            None,
            CloseInertReason::NativeEmissionObservationPrecedesEdge {
                request,
                edge_observed_at: CloseObservationGeneration::new(7),
                issued_after: CloseObservationGeneration::new(6),
            },
        ),
        (
            CloseObservationGeneration::new(8),
            InventoryGeneration::new(6),
            None,
            CloseInertReason::NativeEmissionInventoryPrecedesEdge {
                request,
                edge_received_at: InventoryGeneration::new(7),
                received_after: InventoryGeneration::new(6),
            },
        ),
        (
            CloseObservationGeneration::new(8),
            InventoryGeneration::new(8),
            Some(EffectId::new(100)),
            CloseInertReason::NativeCloseEffectPredecessorMismatch {
                request,
                expected: None,
                actual: Some(EffectId::new(100)),
            },
        ),
    ];

    for (issued_after, received_after, predecessor, reason) in cases {
        assert_eq!(
            coordinator.mark_effect_emitted(
                request,
                authority(3, 5),
                native_binding(),
                EffectId::new(101),
                issued_after,
                received_after,
                predecessor,
            ),
            CloseAdvanceOutcome::Inert(reason)
        );
        assert_eq!(
            coordinator
                .plan(request)
                .map(|current| (current.phase(), current.destruction_state(),)),
            Some((ClosePlanPhase::Approved, CloseDestructionState::None))
        );
    }

    assert_eq!(
        coordinator.mark_effect_emitted(
            request,
            authority(3, 5),
            native_binding(),
            EffectId::new(101),
            CloseObservationGeneration::new(7),
            InventoryGeneration::new(7),
            None,
        ),
        CloseAdvanceOutcome::Advanced {
            request,
            from: ClosePlanPhase::Approved,
            to: ClosePlanPhase::EffectEmitted,
        }
    );
}

#[test]
fn emitted_native_effect_remains_a_settleable_recovery_obligation() {
    let mut coordinator = coordinator();
    let plan = approved_retain_surface_plan(&mut coordinator, "complete-roster");
    let request = plan.request();
    emit_native(&mut coordinator, request);

    let newer_authority = authority(4, 6);
    assert!(coordinator.invalidate_stale(newer_authority).is_empty());
    assert_eq!(
        apply_destroyed(&mut coordinator, request, newer_authority),
        CloseAdvanceOutcome::Advanced {
            request,
            from: ClosePlanPhase::EffectEmitted,
            to: ClosePlanPhase::Applied,
        }
    );
}

#[test]
fn foreign_authority_cannot_advance_post_effect_destruction_lifecycle() {
    let mut coordinator = coordinator();
    let plan = approved_retain_surface_plan(&mut coordinator, "complete-roster");
    let request = plan.request();
    let foreign_domain = EngineAuthorityDomainId::new_for_test(99);
    let foreign_authority = authority_in_domain(foreign_domain, 3, 5);

    emit_native(&mut coordinator, request);
    assert_eq!(
        apply_destroyed(&mut coordinator, request, foreign_authority),
        CloseAdvanceOutcome::Inert(CloseInertReason::AuthorityDomainMismatch {
            request,
            expected: domain(),
            actual: foreign_domain,
        })
    );
    assert_eq!(
        coordinator
            .plan(request)
            .map(|current| (current.phase(), current.destruction_state(),)),
        Some((
            ClosePlanPhase::EffectEmitted,
            CloseDestructionState::EffectEmitted,
        ))
    );

    assert_eq!(
        apply_destroyed(&mut coordinator, request, authority(3, 5)),
        CloseAdvanceOutcome::Advanced {
            request,
            from: ClosePlanPhase::EffectEmitted,
            to: ClosePlanPhase::Applied,
        }
    );
}

#[test]
fn indeterminate_native_effect_waits_without_timeout_and_can_still_settle() {
    let mut coordinator = coordinator();
    let plan = approved_retain_surface_plan(&mut coordinator, ());
    let request = plan.request();
    emit_native(&mut coordinator, request);

    assert_eq!(
        coordinator.mark_indeterminate(request, authority(3, 5)),
        CloseAdvanceOutcome::Advanced {
            request,
            from: ClosePlanPhase::EffectEmitted,
            to: ClosePlanPhase::Indeterminate,
        }
    );
    assert_eq!(
        coordinator.plan(request).map(ClosePlan::phase),
        Some(ClosePlanPhase::Indeterminate)
    );
    assert_eq!(
        apply_destroyed(&mut coordinator, request, authority(3, 5)),
        CloseAdvanceOutcome::Advanced {
            request,
            from: ClosePlanPhase::Indeterminate,
            to: ClosePlanPhase::Applied,
        }
    );
}

#[test]
fn pre_effect_cancellation_is_immediately_terminal() {
    let mut coordinator = coordinator();
    let plan = root_plan(
        &mut coordinator,
        [requirement(ITEM_A, CloseCapability::DeferredAllowed)],
    );
    let request = plan.request();

    assert_eq!(
        coordinator.request_cancel(request, authority(3, 5)),
        CloseAdvanceOutcome::Advanced {
            request,
            from: ClosePlanPhase::Requested,
            to: ClosePlanPhase::Cancelled,
        }
    );
    assert_eq!(
        coordinator.request_cancel(request, authority(3, 5)),
        CloseAdvanceOutcome::Inert(CloseInertReason::Terminal {
            request,
            phase: ClosePlanPhase::Cancelled,
        })
    );
    let recovered = coordinator
        .plan(request)
        .expect("terminal cancellation remains recoverable");
    assert_eq!(recovered.phase(), ClosePlanPhase::Cancelled);
    assert_eq!(recovered.destruction_state(), CloseDestructionState::None);
    assert_eq!(
        recovered.cancellation_state(),
        CloseCancellationState::Settled
    );
    assert!(recovered.phase().is_terminal());
    assert!(
        coordinator
            .active_plans()
            .all(|plan| plan.request() != request)
    );
}

#[test]
fn cancellation_effect_emission_cannot_clear_destroyed_obligation() {
    let mut coordinator = coordinator();
    let plan = approved_retain_surface_plan(&mut coordinator, "complete-roster");
    let request = plan.request();
    emit_native(&mut coordinator, request);

    assert_eq!(
        coordinator.request_cancel(request, authority(3, 5)),
        CloseAdvanceOutcome::Advanced {
            request,
            from: ClosePlanPhase::EffectEmitted,
            to: ClosePlanPhase::CancelRequested,
        }
    );
    assert_eq!(
        coordinator
            .plan(request)
            .map(|current| (current.destruction_state(), current.cancellation_state())),
        Some((
            CloseDestructionState::EffectEmitted,
            CloseCancellationState::Requested,
        ))
    );
    emit_cancellation(&mut coordinator, request);
    assert_eq!(
        coordinator
            .plan(request)
            .map(|current| (current.destruction_state(), current.cancellation_state())),
        Some((
            CloseDestructionState::EffectEmitted,
            CloseCancellationState::Requested,
        ))
    );
    assert_eq!(
        apply_destroyed(&mut coordinator, request, authority(3, 5)),
        CloseAdvanceOutcome::Advanced {
            request,
            from: ClosePlanPhase::CancelRequested,
            to: ClosePlanPhase::Applied,
        }
    );
    assert_eq!(
        coordinator
            .plan(request)
            .map(|current| (current.destruction_state(), current.cancellation_state())),
        Some((CloseDestructionState::None, CloseCancellationState::None,))
    );
}

#[test]
fn destroyed_acknowledging_cancel_successor_wins_the_race() {
    let mut coordinator = coordinator();
    let plan = approved_retain_surface_plan(&mut coordinator, "complete-roster");
    let request = plan.request();
    emit_native(&mut coordinator, request);
    coordinator.request_cancel(request, authority(3, 5));
    emit_cancellation(&mut coordinator, request);

    assert_eq!(
        coordinator.settle_native(
            request,
            authority(3, 5),
            CloseNativeSettlement::DestroyedProved(destroyed_proof_acknowledging(
                request,
                native_binding(),
                11,
                EffectId::new(102),
            )),
        ),
        CloseAdvanceOutcome::Advanced {
            request,
            from: ClosePlanPhase::CancelRequested,
            to: ClosePlanPhase::Applied,
        }
    );
}

#[test]
fn indeterminate_cancellation_keeps_destroyed_application_live() {
    let mut coordinator = coordinator();
    let plan = approved_retain_surface_plan(&mut coordinator, "complete-roster");
    let request = plan.request();
    emit_native(&mut coordinator, request);
    coordinator.request_cancel(request, authority(3, 5));
    emit_cancellation(&mut coordinator, request);

    assert_eq!(
        coordinator.mark_indeterminate(request, authority(3, 5)),
        CloseAdvanceOutcome::Advanced {
            request,
            from: ClosePlanPhase::CancelRequested,
            to: ClosePlanPhase::Indeterminate,
        }
    );
    assert_eq!(
        coordinator
            .plan(request)
            .map(|current| current.cancellation_state()),
        Some(CloseCancellationState::Indeterminate)
    );
    assert_eq!(
        apply_destroyed(&mut coordinator, request, authority(3, 5)),
        CloseAdvanceOutcome::Advanced {
            request,
            from: ClosePlanPhase::Indeterminate,
            to: ClosePlanPhase::Applied,
        }
    );
}

#[test]
fn causal_cancellation_proof_is_the_only_post_effect_cancel_terminal() {
    let mut coordinator = coordinator();
    let request = cancellation_ready_surface_plan(&mut coordinator);

    assert_eq!(
        coordinator.settle_native(
            request,
            authority(3, 5),
            CloseNativeSettlement::CancellationProved(valid_cancellation_proof(request)),
        ),
        CloseAdvanceOutcome::Advanced {
            request,
            from: ClosePlanPhase::CancelRequested,
            to: ClosePlanPhase::Cancelled,
        }
    );
    let plan = coordinator
        .plan(request)
        .expect("terminal plan is retained");
    assert_eq!(plan.phase(), ClosePlanPhase::Cancelled);
    assert_eq!(plan.destruction_state(), CloseDestructionState::None);
    assert_eq!(plan.cancellation_state(), CloseCancellationState::Settled);
}

#[test]
fn surface_veto_requires_proved_native_edge_cancellation() {
    let mut coordinator = coordinator();
    let plan = coordinator
        .open_surface(
            authority(3, 5),
            native_edge(),
            SurfaceCloseRequest::CloseContent,
            [requirement(ITEM_A, CloseCapability::Immediate)],
            "complete-roster",
        )
        .expect("surface plan carries complete decisions");
    let request = plan.request();
    assert_eq!(
        coordinator.resolve(
            request,
            plan.items()[0].token(),
            authority(3, 5),
            CloseDecision::Veto,
        ),
        Ok(CloseResolutionOutcome::Vetoed {
            request,
            item: ITEM_A,
        })
    );
    assert!(
        !coordinator
            .plan(request)
            .expect("plan remains")
            .is_terminal()
    );
    assert_eq!(
        coordinator.request_cancel(request, authority(3, 5)),
        CloseAdvanceOutcome::Advanced {
            request,
            from: ClosePlanPhase::Vetoed,
            to: ClosePlanPhase::CancelRequested,
        }
    );
    assert_eq!(
        coordinator.mark_cancellation_effect_emitted(
            request,
            authority(3, 5),
            native_binding(),
            EffectId::new(102),
            CloseObservationGeneration::new(8),
            InventoryGeneration::new(8),
            None,
        ),
        CloseAdvanceOutcome::Advanced {
            request,
            from: ClosePlanPhase::CancelRequested,
            to: ClosePlanPhase::CancelRequested,
        }
    );
    let proof = CloseCancellationProof::from_authoritative_live_close_cleared(
        request,
        native_binding(),
        None,
        Some(EffectId::new(102)),
        Some(EffectId::new(102)),
        CloseObservationGeneration::new(9),
        InventoryGeneration::new(9),
    );

    assert_eq!(
        coordinator.settle_native(
            request,
            authority(3, 5),
            CloseNativeSettlement::CancellationProved(proof),
        ),
        CloseAdvanceOutcome::Advanced {
            request,
            from: ClosePlanPhase::CancelRequested,
            to: ClosePlanPhase::Cancelled,
        }
    );
}

#[test]
fn cancellation_proof_requires_a_strictly_later_provider_generation() {
    let mut coordinator = coordinator();
    let request = cancellation_ready_surface_plan(&mut coordinator);

    for generation in [9, 10] {
        assert_eq!(
            coordinator.settle_native(
                request,
                authority(3, 5),
                CloseNativeSettlement::CancellationProved(cancellation_proof(
                    request,
                    native_binding(),
                    EffectId::new(101),
                    EffectId::new(102),
                    generation,
                )),
            ),
            CloseAdvanceOutcome::Inert(CloseInertReason::CancellationObservationNotNewer {
                request,
                issued_after: CloseObservationGeneration::new(10),
                actual: CloseObservationGeneration::new(generation),
            })
        );
        assert_eq!(
            coordinator.plan(request).map(ClosePlan::phase),
            Some(ClosePlanPhase::CancelRequested)
        );
    }
}

#[test]
fn cancellation_proof_requires_a_strictly_later_core_receipt_generation() {
    let mut coordinator = coordinator();
    let request = cancellation_ready_surface_plan(&mut coordinator);

    for inventory_generation in [9, 10] {
        let proof = cancellation_proof_at(
            request,
            native_binding(),
            Some(EffectId::new(101)),
            EffectId::new(102),
            99,
            inventory_generation,
        );
        assert_eq!(
            coordinator.settle_native(
                request,
                authority(3, 5),
                CloseNativeSettlement::CancellationProved(proof),
            ),
            CloseAdvanceOutcome::Inert(CloseInertReason::CancellationInventoryNotNewer {
                request,
                issued_after: InventoryGeneration::new(10),
                actual: InventoryGeneration::new(inventory_generation),
            })
        );
    }
}

#[test]
fn cancellation_proof_requires_the_exact_known_effect_frontier() {
    let mut coordinator = coordinator();
    let request = cancellation_ready_surface_plan(&mut coordinator);

    for actual in [None, Some(EffectId::new(101)), Some(EffectId::new(103))] {
        let proof = CloseCancellationProof::from_authoritative_live_close_cleared(
            request,
            native_binding(),
            Some(EffectId::new(101)),
            Some(EffectId::new(102)),
            actual,
            CloseObservationGeneration::new(11),
            InventoryGeneration::new(11),
        );
        assert_eq!(
            coordinator.settle_native(
                request,
                authority(3, 5),
                CloseNativeSettlement::CancellationProved(proof),
            ),
            CloseAdvanceOutcome::Inert(CloseInertReason::CancellationKnownFrontierMismatch {
                request,
                expected: Some(EffectId::new(102)),
                actual,
            })
        );
        assert_eq!(
            coordinator.plan(request).map(|current| (
                current.phase(),
                current.destruction_state(),
                current.cancellation_state(),
            )),
            Some((
                ClosePlanPhase::CancelRequested,
                CloseDestructionState::EffectEmitted,
                CloseCancellationState::Requested,
            ))
        );
    }

    assert_eq!(
        coordinator.settle_native(
            request,
            authority(3, 5),
            CloseNativeSettlement::CancellationProved(valid_cancellation_proof(request)),
        ),
        CloseAdvanceOutcome::Advanced {
            request,
            from: ClosePlanPhase::CancelRequested,
            to: ClosePlanPhase::Cancelled,
        }
    );
}

#[test]
fn cancellation_effect_requires_the_exact_lane_predecessor_not_numeric_order() {
    let mut coordinator = coordinator();
    let plan = approved_retain_surface_plan(&mut coordinator, "complete-roster");
    let request = plan.request();
    emit_native(&mut coordinator, request);
    coordinator.request_cancel(request, authority(3, 5));

    assert_eq!(
        coordinator.mark_cancellation_effect_emitted(
            request,
            authority(3, 5),
            native_binding(),
            EffectId::new(999),
            CloseObservationGeneration::new(10),
            InventoryGeneration::new(10),
            Some(EffectId::new(100)),
        ),
        CloseAdvanceOutcome::Inert(CloseInertReason::CancellationEffectPredecessorMismatch {
            request,
            expected: Some(EffectId::new(101)),
            actual: Some(EffectId::new(100)),
        })
    );
    assert_eq!(
        coordinator.mark_cancellation_effect_emitted(
            request,
            authority(3, 5),
            native_binding(),
            EffectId::new(1),
            CloseObservationGeneration::new(10),
            InventoryGeneration::new(10),
            Some(EffectId::new(101)),
        ),
        CloseAdvanceOutcome::Advanced {
            request,
            from: ClosePlanPhase::CancelRequested,
            to: ClosePlanPhase::CancelRequested,
        }
    );
}

#[test]
fn cancellation_proof_rejects_foreign_request_binding_and_effects_without_mutation() {
    let mut coordinator = coordinator();
    let request = cancellation_ready_surface_plan(&mut coordinator);
    let foreign_domain = EngineAuthorityDomainId::new_for_test(99);
    let foreign_request = CloseRequestId::from_sequence(
        foreign_domain,
        NonZeroU64::new(1).expect("non-zero fixture"),
    );
    let wrong_binding = ViewportBinding::new(
        domain(),
        WorkspaceEpoch::new(7),
        SURFACE,
        WindowToken::new(41),
        WindowIncarnation::new(52),
    );
    let cases = [
        (
            cancellation_proof(
                foreign_request,
                native_binding(),
                EffectId::new(101),
                EffectId::new(102),
                11,
            ),
            CloseInertReason::CancellationProofDomainMismatch {
                request,
                expected: domain(),
                actual: foreign_domain,
            },
        ),
        (
            cancellation_proof(
                request,
                wrong_binding,
                EffectId::new(101),
                EffectId::new(102),
                11,
            ),
            CloseInertReason::NativeBindingMismatch {
                request,
                expected: native_binding(),
                actual: wrong_binding,
            },
        ),
        (
            cancellation_proof(
                request,
                native_binding(),
                EffectId::new(100),
                EffectId::new(102),
                11,
            ),
            CloseInertReason::CancellationEffectPredecessorMismatch {
                request,
                expected: Some(EffectId::new(101)),
                actual: Some(EffectId::new(100)),
            },
        ),
        (
            cancellation_proof(
                request,
                native_binding(),
                EffectId::new(101),
                EffectId::new(103),
                11,
            ),
            CloseInertReason::CancellationEffectMismatch {
                request,
                expected: Some(EffectId::new(102)),
                actual: Some(EffectId::new(103)),
            },
        ),
    ];

    for (proof, reason) in cases {
        assert_eq!(
            coordinator.settle_native(
                request,
                authority(3, 5),
                CloseNativeSettlement::CancellationProved(proof),
            ),
            CloseAdvanceOutcome::Inert(reason)
        );
        assert_eq!(
            coordinator.plan(request).map(|plan| (
                plan.phase(),
                plan.destruction_state(),
                plan.cancellation_state(),
            )),
            Some((
                ClosePlanPhase::CancelRequested,
                CloseDestructionState::EffectEmitted,
                CloseCancellationState::Requested,
            ))
        );
    }
}

#[test]
fn destroyed_proof_rejects_foreign_request_binding_and_old_generation_without_mutation() {
    let mut coordinator = coordinator();
    let plan = approved_retain_surface_plan(&mut coordinator, "complete-roster");
    let request = plan.request();
    emit_native(&mut coordinator, request);

    let foreign_domain = EngineAuthorityDomainId::new_for_test(99);
    let foreign_request = CloseRequestId::from_sequence(
        foreign_domain,
        NonZeroU64::new(1).expect("non-zero fixture"),
    );
    let wrong_request = CloseRequestId::from_sequence(
        domain(),
        NonZeroU64::new(request.sequence() + 1).expect("non-zero fixture"),
    );
    let wrong_binding = ViewportBinding::new(
        domain(),
        WorkspaceEpoch::new(7),
        SURFACE,
        WindowToken::new(41),
        WindowIncarnation::new(52),
    );
    let cases = [
        (
            destroyed_proof(foreign_request, native_binding(), 11),
            CloseInertReason::DestroyedProofDomainMismatch {
                request,
                expected: domain(),
                actual: foreign_domain,
            },
        ),
        (
            destroyed_proof(wrong_request, native_binding(), 11),
            CloseInertReason::DestroyedProofRequestMismatch {
                request,
                proof_request: wrong_request,
            },
        ),
        (
            destroyed_proof(request, wrong_binding, 11),
            CloseInertReason::NativeBindingMismatch {
                request,
                expected: native_binding(),
                actual: wrong_binding,
            },
        ),
        (
            destroyed_proof(request, native_binding(), 7),
            CloseInertReason::DestroyedObservationNotNewer {
                request,
                issued_after: CloseObservationGeneration::new(8),
                actual: CloseObservationGeneration::new(7),
            },
        ),
        (
            destroyed_proof(request, native_binding(), 8),
            CloseInertReason::DestroyedObservationNotNewer {
                request,
                issued_after: CloseObservationGeneration::new(8),
                actual: CloseObservationGeneration::new(8),
            },
        ),
        (
            destroyed_proof_acknowledging(request, native_binding(), 99, EffectId::new(100)),
            CloseInertReason::DestroyedEffectNotAcknowledged {
                request,
                close_effect: EffectId::new(101),
                cancellation_effect: None,
                actual: EffectId::new(100),
            },
        ),
    ];

    for (proof, reason) in cases {
        assert_eq!(
            coordinator.settle_native(
                request,
                authority(3, 5),
                CloseNativeSettlement::DestroyedProved(proof),
            ),
            CloseAdvanceOutcome::Inert(reason)
        );
        assert_eq!(
            coordinator.plan(request).map(|plan| (
                plan.phase(),
                plan.destruction_state(),
                plan.cancellation_state(),
            )),
            Some((
                ClosePlanPhase::EffectEmitted,
                CloseDestructionState::EffectEmitted,
                CloseCancellationState::None,
            ))
        );
    }
}

#[test]
fn destroyed_proof_requires_a_strictly_later_core_receipt_generation() {
    let mut coordinator = coordinator();
    let plan = approved_retain_surface_plan(&mut coordinator, "complete-roster");
    let request = plan.request();
    emit_native(&mut coordinator, request);

    for inventory_generation in [7, 8] {
        let proof = destroyed_proof_at(
            request,
            native_binding(),
            99,
            inventory_generation,
            EffectId::new(101),
        );
        assert_eq!(
            coordinator.settle_native(
                request,
                authority(3, 5),
                CloseNativeSettlement::DestroyedProved(proof),
            ),
            CloseAdvanceOutcome::Inert(CloseInertReason::DestroyedInventoryNotNewer {
                request,
                issued_after: InventoryGeneration::new(8),
                actual: InventoryGeneration::new(inventory_generation),
            })
        );
    }
}

#[test]
fn planned_destroyed_settlement_requires_a_destructive_close_effect() {
    let mut coordinator = coordinator();
    let plan = coordinator
        .open_surface(
            authority(3, 5),
            native_edge(),
            rehome_request(),
            [],
            "complete-roster-and-target-proof",
        )
        .expect("rehome plan requires no content decisions");
    let request = plan.request();

    assert_eq!(
        coordinator.settle_native(
            request,
            authority(3, 5),
            CloseNativeSettlement::DestroyedProved(destroyed_proof(request, native_binding(), 99,)),
        ),
        CloseAdvanceOutcome::Inert(CloseInertReason::CloseEffectNotEmitted { request })
    );
    assert_eq!(
        coordinator.plan(request).map(ClosePlan::phase),
        Some(ClosePlanPhase::Approved)
    );
}

#[test]
fn unproved_external_destruction_is_terminal_without_applying_the_plan() {
    let mut coordinator = coordinator();
    let plan = approved_retain_surface_plan(&mut coordinator, "complete-roster");
    let request = plan.request();
    emit_native(&mut coordinator, request);

    assert_eq!(
        coordinator.mark_externally_destroyed_unproved(request, native_binding()),
        CloseAdvanceOutcome::Advanced {
            request,
            from: ClosePlanPhase::EffectEmitted,
            to: ClosePlanPhase::ExternallyDestroyedUnproved,
        }
    );
    assert_eq!(
        coordinator.plan(request).map(|current| (
            current.phase(),
            current.destruction_state(),
            current.cancellation_state(),
            current.is_terminal(),
        )),
        Some((
            ClosePlanPhase::ExternallyDestroyedUnproved,
            CloseDestructionState::None,
            CloseCancellationState::None,
            true,
        ))
    );
    assert_eq!(coordinator.surface_request_for_edge(native_edge()), None);
    assert!(coordinator.active_plans().next().is_none());
    assert_eq!(
        coordinator.mark_externally_destroyed_unproved(request, native_binding()),
        CloseAdvanceOutcome::Inert(CloseInertReason::Terminal {
            request,
            phase: ClosePlanPhase::ExternallyDestroyedUnproved,
        })
    );
}

#[test]
fn same_batch_requires_both_destroyed_and_cancellation_proofs_to_be_exact() {
    let wrong_binding = ViewportBinding::new(
        domain(),
        WorkspaceEpoch::new(7),
        SURFACE,
        WindowToken::new(41),
        WindowIncarnation::new(52),
    );

    let mut first_coordinator = coordinator();
    let request = cancellation_ready_surface_plan(&mut first_coordinator);
    assert_eq!(
        first_coordinator.settle_native(
            request,
            authority(3, 5),
            CloseNativeSettlement::DestroyedProvedWithCancellation {
                destroyed: destroyed_proof(request, wrong_binding, 11),
                cancellation: valid_cancellation_proof(request),
            },
        ),
        CloseAdvanceOutcome::Inert(CloseInertReason::NativeBindingMismatch {
            request,
            expected: native_binding(),
            actual: wrong_binding,
        })
    );
    assert_eq!(
        first_coordinator.plan(request).map(|plan| (
            plan.phase(),
            plan.destruction_state(),
            plan.cancellation_state(),
        )),
        Some((
            ClosePlanPhase::CancelRequested,
            CloseDestructionState::EffectEmitted,
            CloseCancellationState::Requested,
        ))
    );

    let mut second_coordinator = coordinator();
    let request = cancellation_ready_surface_plan(&mut second_coordinator);
    assert_eq!(
        second_coordinator.settle_native(
            request,
            authority(3, 5),
            CloseNativeSettlement::DestroyedProvedWithCancellation {
                destroyed: valid_destroyed_proof(request),
                cancellation: cancellation_proof(
                    request,
                    native_binding(),
                    EffectId::new(101),
                    EffectId::new(103),
                    11,
                ),
            },
        ),
        CloseAdvanceOutcome::Inert(CloseInertReason::CancellationEffectMismatch {
            request,
            expected: Some(EffectId::new(102)),
            actual: Some(EffectId::new(103)),
        })
    );
    assert_eq!(
        second_coordinator.plan(request).map(|plan| (
            plan.phase(),
            plan.destruction_state(),
            plan.cancellation_state(),
        )),
        Some((
            ClosePlanPhase::CancelRequested,
            CloseDestructionState::EffectEmitted,
            CloseCancellationState::Requested,
        ))
    );
}

#[test]
fn same_batch_destruction_wins_over_valid_cancellation_proof() {
    let mut coordinator = coordinator();
    let request = cancellation_ready_surface_plan(&mut coordinator);
    let proof = valid_cancellation_proof(request);

    assert_eq!(
        coordinator.settle_native(
            request,
            authority(3, 5),
            CloseNativeSettlement::DestroyedProvedWithCancellation {
                destroyed: valid_destroyed_proof(request),
                cancellation: proof,
            },
        ),
        CloseAdvanceOutcome::Advanced {
            request,
            from: ClosePlanPhase::CancelRequested,
            to: ClosePlanPhase::Applied,
        }
    );
    assert_eq!(
        coordinator.settle_native(
            request,
            authority(3, 5),
            CloseNativeSettlement::CancellationProved(proof),
        ),
        CloseAdvanceOutcome::Inert(CloseInertReason::Terminal {
            request,
            phase: ClosePlanPhase::Applied,
        })
    );
}

#[test]
fn invalid_plan_shapes_are_rejected_before_any_identity_is_consumed() {
    let mut coordinator = coordinator();
    assert_eq!(
        coordinator.open(
            authority(3, 5),
            ClosePlanTarget::Item { item: ITEM_A },
            [requirement(ITEM_B, CloseCapability::Immediate)],
            (),
        ),
        Err(CloseCoordinatorError::ItemTargetMismatch {
            target: ITEM_A,
            actual: vec![ITEM_B],
        })
    );
    assert_eq!(
        coordinator.open(
            authority(3, 5),
            ClosePlanTarget::Root { root: ROOT },
            [
                requirement(ITEM_A, CloseCapability::Immediate),
                requirement(ITEM_A, CloseCapability::Immediate),
            ],
            (),
        ),
        Err(CloseCoordinatorError::DuplicateItem { item: ITEM_A })
    );
    assert_eq!(
        coordinator.open(
            authority(3, 5),
            ClosePlanTarget::Root { root: ROOT },
            [requirement(ITEM_A, CloseCapability::Disabled)],
            (),
        ),
        Err(CloseCoordinatorError::DisabledItem { item: ITEM_A })
    );

    let first_valid = coordinator
        .open(
            authority(3, 5),
            ClosePlanTarget::Item { item: ITEM_A },
            [requirement(ITEM_A, CloseCapability::Immediate)],
            (),
        )
        .expect("invalid plans do not consume request identities");
    assert_eq!(first_valid.request().sequence(), 1);
    assert_eq!(first_valid.items()[0].token().sequence(), 1);
}

#[test]
fn coordinator_rejects_authority_from_another_domain_before_minting_ids() {
    let mut coordinator = coordinator();
    let foreign = CloseAuthority::new(
        EngineAuthorityDomainId::new_for_test(99),
        WorkspaceVersion::new(WorkspaceEpoch::new(7), WorkspaceRevision::new(3)),
        PolicyRevision::new(5),
    );

    assert_eq!(
        coordinator.open(
            foreign,
            ClosePlanTarget::Item { item: ITEM_A },
            [requirement(ITEM_A, CloseCapability::Immediate)],
            (),
        ),
        Err(CloseCoordinatorError::AuthorityDomainMismatch {
            expected: domain(),
            actual: EngineAuthorityDomainId::new_for_test(99),
        })
    );

    let first = coordinator
        .open(
            authority(3, 5),
            ClosePlanTarget::Item { item: ITEM_A },
            [requirement(ITEM_A, CloseCapability::Immediate)],
            (),
        )
        .expect("foreign authority does not consume identities");
    assert_eq!(first.request().sequence(), 1);
    assert_eq!(first.items()[0].token().sequence(), 1);
}

#[test]
fn plan_inventory_retains_terminal_snapshots_and_filters_active_plans() {
    let mut coordinator = coordinator();
    let first = root_plan(
        &mut coordinator,
        [requirement(ITEM_A, CloseCapability::Immediate)],
    );
    let second = root_plan(
        &mut coordinator,
        [requirement(ITEM_B, CloseCapability::Immediate)],
    );
    coordinator
        .resolve(
            first.request(),
            first.items()[0].token(),
            authority(3, 5),
            CloseDecision::Veto,
        )
        .expect("veto does not allocate a continuation");

    assert_eq!(
        coordinator
            .plans()
            .map(ClosePlan::request)
            .collect::<Vec<_>>(),
        vec![first.request(), second.request()]
    );
    assert_eq!(
        coordinator
            .active_plans()
            .map(ClosePlan::request)
            .collect::<Vec<_>>(),
        vec![second.request()]
    );
    assert_eq!(
        coordinator.plan(first.request()).map(ClosePlan::phase),
        Some(ClosePlanPhase::Vetoed)
    );
}

#[test]
fn explicit_invalidation_marks_all_active_authority_mismatches_once() {
    let mut coordinator = coordinator();
    let first = root_plan(
        &mut coordinator,
        [requirement(ITEM_A, CloseCapability::Immediate)],
    );
    let second = root_plan(
        &mut coordinator,
        [requirement(ITEM_B, CloseCapability::Immediate)],
    );

    assert_eq!(
        coordinator.invalidate_stale(authority(3, 6)),
        vec![first.request(), second.request()]
    );
    assert!(coordinator.invalidate_stale(authority(3, 6)).is_empty());
    assert_eq!(
        coordinator.plan(first.request()).map(ClosePlan::phase),
        Some(ClosePlanPhase::Stale)
    );
}

#[test]
fn root_and_surface_plan_shapes_require_the_exact_decision_class() {
    let mut coordinator = coordinator();

    assert_eq!(
        coordinator.open(
            authority(3, 5),
            ClosePlanTarget::Root { root: ROOT },
            [],
            (),
        ),
        Err(CloseCoordinatorError::EmptyRootHasNoItemDecisions { root: ROOT })
    );
    let retain = coordinator
        .open_surface(
            authority(3, 5),
            native_edge(),
            SurfaceCloseRequest::RetainLayout,
            [],
            (),
        )
        .expect("an empty retain-layout surface needs no pane decisions");
    assert_eq!(retain.phase(), ClosePlanPhase::Approved);
    let retain_with_decisions = coordinator
        .open_surface(
            authority(3, 5),
            native_edge(),
            SurfaceCloseRequest::RetainLayout,
            [requirement(ITEM_A, CloseCapability::Immediate)],
            (),
        )
        .expect("retain-layout may freeze pane-close decisions");
    assert_eq!(retain_with_decisions.phase(), ClosePlanPhase::Requested);
    assert_eq!(
        coordinator.open_surface(
            authority(3, 5),
            native_edge(),
            rehome_request(),
            [requirement(ITEM_A, CloseCapability::Immediate)],
            (),
        ),
        Err(CloseCoordinatorError::SurfaceRehomeHasItemDecisions {
            surface: SURFACE,
            item_count: 1,
        })
    );

    assert_eq!(
        coordinator.open_surface(
            authority(3, 5),
            native_edge(),
            SurfaceCloseRequest::CloseContent,
            [],
            (),
        ),
        Err(CloseCoordinatorError::SurfaceCloseContentHasNoItemDecisions { surface: SURFACE })
    );

    let close_content = coordinator
        .open_surface(
            authority(3, 5),
            native_edge(),
            SurfaceCloseRequest::CloseContent,
            [
                requirement(ITEM_A, CloseCapability::Immediate),
                requirement(ITEM_B, CloseCapability::DeferredAllowed),
            ],
            (),
        )
        .expect("close-content carries the complete content-decision roster");
    assert_eq!(close_content.phase(), ClosePlanPhase::Requested);

    let rehome = coordinator
        .open_surface(authority(3, 5), native_edge(), rehome_request(), [], ())
        .expect("rehome carries no content-close decisions");
    assert_eq!(rehome.phase(), ClosePlanPhase::Approved);
}

#[test]
fn surface_plan_cannot_exist_without_an_exact_native_edge() {
    let mut coordinator = coordinator();
    assert_eq!(
        coordinator.open(
            authority(3, 5),
            ClosePlanTarget::Surface {
                surface: SURFACE,
                disposition: SurfaceCloseDisposition::RehomeAll {
                    target: TARGET_SURFACE,
                },
            },
            [],
            (),
        ),
        Err(CloseCoordinatorError::SurfaceRequiresNativeEdge)
    );

    let foreign_domain = EngineAuthorityDomainId::new_for_test(8);
    let foreign_edge = NativeCloseEdge::from_authoritative_requested(
        foreign_domain,
        native_binding(),
        CloseObservationGeneration::new(7),
        InventoryGeneration::new(7),
    );
    assert_eq!(
        coordinator.open_surface(authority(3, 5), foreign_edge, rehome_request(), [], (),),
        Err(CloseCoordinatorError::NativeEdgeDomainMismatch {
            expected: domain(),
            actual: foreign_domain,
        })
    );

    let plan = coordinator
        .open_surface(authority(3, 5), native_edge(), rehome_request(), [], ())
        .expect("failed surface opens do not consume request identity");
    assert_eq!(plan.request().sequence(), 1);
    assert_eq!(coordinator.native_edge(plan.request()), Some(native_edge()));
}

#[test]
fn destroyed_proof_exposes_only_its_exact_causal_facts() {
    let request =
        CloseRequestId::from_sequence(domain(), NonZeroU64::new(1).expect("non-zero fixture"));
    let proof = CloseDestroyedProof::from_authoritative_destroyed(
        request,
        native_binding(),
        CloseObservationGeneration::new(11),
        InventoryGeneration::new(12),
        EffectId::new(101),
    );

    assert_eq!(proof.request(), request);
    assert_eq!(proof.binding(), native_binding());
    assert_eq!(
        proof.observation_generation(),
        CloseObservationGeneration::new(11)
    );
    assert_eq!(proof.inventory_generation(), InventoryGeneration::new(12));
    assert_eq!(proof.acknowledged_effect(), EffectId::new(101));
}

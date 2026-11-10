use super::*;
use crate::effect::{
    DispatchFailureReason, EffectDispatchResult, EffectIndeterminateReason, EffectInvalidation,
    EffectPhase, EffectResult, EffectTransition, PlatformEffect,
};
use crate::frame::{
    BindingRetirementOrigin, ViewportCoordinator, ViewportCoordinatorError, ViewportLifecycleAction,
};
use crate::geometry::{PhysicalRect, ScaleFactor};
use crate::ids::{EngineAuthorityDomainId, WorkspaceEpoch};
use crate::intent::{Authority, AuthorityUnavailableReason};
use crate::platform::{
    CapabilityRosterObservation, CloseEffectAcknowledgement, NativeCreateReadiness, ObservedWindow,
    PlatformCapability, PlatformSnapshot, WindowCloseObservation, WindowCloseState,
    WindowCoordinateObservation, WindowCreatePreparationObservation, WindowInputState,
    WindowInventoryObservation, WindowPresentationObservation, WindowPresentationState,
    WorkAreaRosterObservation,
};
use crate::presentation_observation::NativeStagingOwner;
use crate::viewport::{
    CapabilityObservationGeneration, CloseObservationGeneration, CoordinateObservationGeneration,
    InventoryObservationGeneration, PlatformSnapshotGeneration, PresentationObservationGeneration,
    WindowIncarnation, WindowToken, WorkAreaObservationGeneration,
};
use crate::viewport_focus::{FocusObservationGeneration, unknown_focus_observation};
use crate::viewport_registry::{ViewportAdmission, ViewportRegistryError};

fn binding(surface: u64, incarnation: u64) -> ViewportBinding {
    ViewportBinding::new(
        EngineAuthorityDomainId::new_for_test(1),
        WorkspaceEpoch::new(1),
        SurfaceId::new(surface),
        WindowToken::new(10),
        WindowIncarnation::new(incarnation),
    )
}

fn request(destroyed: ViewportBinding, replacement: ViewportBinding) -> RecoveryPendingRequest {
    RecoveryPendingRequest::replacement(
        destroyed,
        ViewportRole::Child,
        SurfaceRecoveryObligationId::new_for_test(1),
        replacement,
        EffectId::new(7),
    )
}

#[test]
fn replacement_lookup_is_exact_across_binding_incarnations() {
    let destroyed = binding(1, 1);
    let replacement = binding(1, 2);
    let stale = binding(1, 3);
    let mut lifecycle = RecoveryReplacementLifecycle::default();
    lifecycle
        .begin(request(destroyed, replacement))
        .expect("replacement must register");

    assert_eq!(
        lifecycle.replacement_surface(replacement),
        Some(SurfaceId::new(1))
    );
    assert_eq!(lifecycle.replacement_surface(stale), None);
    assert!(matches!(
        lifecycle.reset_lost_replacement(SurfaceId::new(1), stale),
        Err(RecoveryReplacementLifecycleError::ReplacementMismatch { .. })
    ));
    assert_eq!(
        lifecycle
            .pending(SurfaceId::new(1))
            .and_then(RecoveryPending::replacement_binding),
        Some(replacement)
    );
}

fn native_coordinator() -> ViewportCoordinator {
    let mut coordinator = ViewportCoordinator::default();
    coordinator
        .capabilities
        .set_native_window_lifecycle(PlatformCapability::Supported);
    coordinator
        .capabilities
        .set_authoritative_inventory(PlatformCapability::Supported);
    coordinator
        .capabilities
        .set_global_window_placement(PlatformCapability::Supported);
    coordinator
}

fn placement() -> PhysicalRect {
    PhysicalRect::new(5.0, 0.0, 310.0, 225.0).expect("test placement must be valid")
}

fn native_snapshot(
    coordinator: &ViewportCoordinator,
    generation: u64,
    windows: Vec<ObservedWindow>,
    closes: Vec<WindowCloseObservation>,
) -> PlatformSnapshot {
    PlatformSnapshot::new(
        PlatformSnapshotGeneration::new(generation),
        CapabilityRosterObservation::new(
            CapabilityObservationGeneration::new(generation),
            Authority::Known(coordinator.capabilities.clone()),
        ),
        unknown_focus_observation(
            FocusObservationGeneration::new(generation),
            AuthorityUnavailableReason::NotReported,
        ),
        WindowInventoryObservation::new(
            InventoryObservationGeneration::new(generation),
            Authority::Known(windows.iter().map(ObservedWindow::binding).collect()),
        )
        .unwrap(),
        windows,
        closes,
        WorkAreaRosterObservation::new(
            WorkAreaObservationGeneration::new(generation),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        )
        .unwrap(),
    )
    .unwrap()
}

fn direct_ready_proof(binding: ViewportBinding, owner: NativeStagingOwner) -> NativeReadyProof {
    NativeReadyProof::for_test(
        binding,
        owner,
        ViewportCoordinator::default().platform_provider().unwrap(),
    )
}

#[test]
fn initial_and_destroyed_establishment_cannot_borrow_each_others_owner_or_origin() {
    let source = binding(1, 1);
    let replacement = binding(1, 2);
    let obligation = SurfaceRecoveryObligationId::new_for_test(1);
    let requests = [
        RecoveryPendingRequest::initial(source, ViewportRole::Child, obligation, EffectId::new(7)),
        RecoveryPendingRequest::replacement(
            source,
            ViewportRole::Child,
            obligation,
            replacement,
            EffectId::new(7),
        ),
    ];
    for request in requests {
        let origin = request.origin();
        assert_eq!(request.source_binding(), source);
        let mut lifecycle = RecoveryReplacementLifecycle::default();
        lifecycle.begin(request).expect("establishment must begin");
        let pending = lifecycle.pending(source.surface()).unwrap();
        let target = pending.replacement_binding().unwrap();
        let owner = pending.staging_owner();
        let (other_origin, other_owner) = match origin {
            NativeSurfaceEstablishmentOrigin::Initial { binding } => (
                NativeSurfaceEstablishmentOrigin::Destroyed { binding },
                NativeStagingOwner::recovery_replacement(binding, obligation),
            ),
            NativeSurfaceEstablishmentOrigin::Destroyed { binding } => (
                NativeSurfaceEstablishmentOrigin::Initial { binding },
                NativeStagingOwner::InitialSurface {
                    binding,
                    recovery_obligation: obligation,
                },
            ),
        };
        assert_eq!(pending.origin(), origin);
        assert_eq!(pending.source_binding(), source);
        let (_, phase) = lifecycle.bringup(target).unwrap();
        let before = lifecycle.clone();
        assert_eq!(
            lifecycle.mark_awaiting_first_live(
                target,
                phase,
                direct_ready_proof(target, other_owner),
            ),
            Err(RecoveryReplacementLifecycleError::InvalidStatus {
                surface: source.surface(),
            })
        );
        assert_eq!(lifecycle, before);
        lifecycle
            .mark_awaiting_first_live(target, phase, direct_ready_proof(target, owner))
            .expect("the exact establishment owner must acquire first-live readiness");
        let before = lifecycle.clone();
        assert_eq!(
            lifecycle.complete_first_live(other_origin, target, obligation),
            Err(RecoveryReplacementLifecycleError::PendingIdentityMismatch {
                surface: source.surface(),
            })
        );
        assert_eq!(lifecycle, before);
        lifecycle
            .complete_first_live(origin, target, obligation)
            .expect("the exact origin must complete, including an initial source equal to target");
        assert!(lifecycle.is_empty());
        assert_eq!(lifecycle.replacement_surface(target), None);
    }
}

#[test]
fn initial_start_requires_exact_capability_and_preserves_atomic_ownership() {
    let surface = SurfaceId::new(1);
    let obligation = SurfaceRecoveryObligationId::new_for_test(1);
    let mut unavailable = ViewportCoordinator::default();
    let before = unavailable.clone();
    assert!(matches!(
        unavailable.start_native_surface(surface, placement(), obligation),
        Err(ViewportCoordinatorError::NativeCapabilityUnavailable { .. })
    ));
    assert_eq!(unavailable, before);

    let mut coordinator = native_coordinator();
    let target = coordinator
        .start_native_surface(surface, placement(), obligation)
        .expect("an unbound logical surface must establish its own native binding");
    let pending = coordinator.recovery_pending(surface).unwrap();
    assert_eq!(pending.source_binding(), target);
    assert_eq!(pending.replacement_binding(), Some(target));
    assert_eq!(
        pending.origin(),
        NativeSurfaceEstablishmentOrigin::Initial { binding: target }
    );
    assert_eq!(target.epoch(), coordinator.workspace_epoch);
    assert_eq!(
        coordinator.viewport(surface).unwrap().admission(),
        ViewportAdmission::Pending
    );
    assert_eq!(coordinator.native_create_sagas().count(), 0);
    assert_eq!(coordinator.retained_native_staging_resources().len(), 0);
    let before = coordinator.clone();
    assert_eq!(
        coordinator.start_native_surface(surface, placement(), obligation),
        Err(ViewportCoordinatorError::PendingRecoveryRegistrationMismatch { surface })
    );
    assert_eq!(coordinator, before);
    assert_eq!(
        coordinator.defer_destroyed_surface_recovery(target, obligation),
        Err(ViewportCoordinatorError::PendingRecoveryObligationMismatch { surface })
    );
    assert_eq!(coordinator, before);
    let emitted = coordinator.take_new_effects();
    assert!(matches!(
        emitted.as_slice(),
        [request] if request.effect() == &PlatformEffect::CreateWindow {
            binding: target,
            placement: placement(),
            role: ViewportRole::Child,

        }
    ));

    let other = SurfaceId::new(2);
    let live = coordinator
        .register_existing(
            target.epoch(),
            other,
            WindowToken::new(20),
            ViewportRole::Child,
            Some(obligation),
        )
        .unwrap();
    let before = coordinator.clone();
    assert_eq!(
        coordinator.start_native_surface(other, placement(), obligation),
        Err(ViewportCoordinatorError::Registry(
            ViewportRegistryError::SurfaceAlreadyRegistered { surface: other }
        ))
    );
    assert_eq!(coordinator, before);
    assert_eq!(
        coordinator.defer_destroyed_surface_recovery(live, obligation),
        Err(ViewportCoordinatorError::DestroyedSurfaceStillObserved { binding: live })
    );
    assert_eq!(coordinator, before);
}

#[test]
fn healthy_initial_never_queues_recovery_retry_until_create_dispatch_fails() {
    let mut coordinator = native_coordinator();
    let surface = SurfaceId::new(1);
    let obligation = SurfaceRecoveryObligationId::new_for_test(1);
    let target = coordinator
        .start_native_surface(surface, placement(), obligation)
        .unwrap();
    assert!(
        !coordinator
            .recovery_pending(surface)
            .unwrap()
            .permits_recovery_retry()
    );
    assert!(
        coordinator
            .reduce_registry_events(&[], &BTreeMap::new())
            .unwrap()
            .is_empty()
    );
    let emitted = coordinator.take_new_effects();
    let [create] = emitted.as_slice() else {
        panic!("initial establishment must emit exactly one create");
    };
    for (result, permits_retry) in [
        (
            EffectDispatchResult::Indeterminate(EffectIndeterminateReason::AcknowledgementLost),
            false,
        ),
        (
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::WindowUnavailable),
            true,
        ),
    ] {
        assert_eq!(
            coordinator
                .report_effect(
                    target.epoch(),
                    EffectResult::new(create.id(), target.epoch(), result)
                )
                .unwrap(),
            EffectTransition::Applied
        );
        let pending = coordinator.recovery_pending(surface).unwrap();
        assert_eq!(pending.permits_recovery_retry(), permits_retry);
        assert_eq!(pending.source_binding(), target);
        assert_eq!(
            pending.origin(),
            NativeSurfaceEstablishmentOrigin::Initial { binding: target }
        );
        let actions = coordinator
            .reduce_registry_events(&[], &BTreeMap::new())
            .unwrap();
        if permits_retry {
            assert_eq!(
                actions,
                vec![ViewportLifecycleAction::RetryRecovery {
                    source_binding: target,
                    recovery_obligation: obligation,
                }]
            );
        } else {
            assert!(actions.is_empty());
        }
    }
    assert!(coordinator.viewport(surface).is_none());
    assert_eq!(coordinator.retained_native_staging_resources().len(), 0);
}

#[test]
fn initial_loss_and_host_recovery_keep_exact_cleanup_ownership_without_graph_destruction() {
    for recovered_to_host in [false, true] {
        let mut coordinator = native_coordinator();
        let surface = SurfaceId::new(1);
        let obligation = SurfaceRecoveryObligationId::new_for_test(1);
        let target = coordinator
            .start_native_surface(surface, placement(), obligation)
            .unwrap();
        let emitted = coordinator.take_new_effects();
        let create = emitted[0].id();
        if recovered_to_host {
            coordinator.complete_pending_recovery(surface).unwrap();
            assert!(matches!(
                coordinator.recovery_pending(surface).unwrap().status(),
                RecoveryPendingStatus::AwaitingReplacementCleanup { replacement }
                    if replacement == create
            ));
            assert!(coordinator.recovery_retry_is_suppressed(target, obligation));
            assert!(
                coordinator
                    .reduce_registry_events(&[], &BTreeMap::new())
                    .unwrap()
                    .is_empty()
            );
            let cleanup = coordinator.take_new_effects();
            assert!(matches!(
                cleanup.as_slice(),
                [request] if request.effect() == &PlatformEffect::CompensatingClose {
                    binding: target,
                    compensates: create,
                }
            ));
        }
        let snapshot = native_snapshot(
            &coordinator,
            1,
            Vec::new(),
            vec![WindowCloseObservation::new(
                target,
                CloseObservationGeneration::new(1),
                Authority::Known(WindowCloseState::Destroyed),
                CloseEffectAcknowledgement::known(None),
            )],
        );
        let transition = coordinator.publish_snapshot(&snapshot).unwrap();
        assert!(transition.actions().is_empty());
        assert!(coordinator.viewport(surface).is_none());
        if recovered_to_host {
            assert!(coordinator.recovery_pending(surface).is_none());
        } else {
            let pending = coordinator.recovery_pending(surface).unwrap();
            assert_eq!(
                pending.status(),
                RecoveryPendingStatus::AwaitingRecoveryHost
            );
            assert!(pending.permits_recovery_retry());
            assert_eq!(
                pending.origin(),
                NativeSurfaceEstablishmentOrigin::Initial { binding: target }
            );
            assert!(pending.permits_initial_retry());
            let successor = coordinator
                .start_native_surface(surface, placement(), obligation)
                .expect("initial destruction before admission must permit an explicit retry");
            assert_ne!(successor, target);
            assert_eq!(
                coordinator
                    .recovery_pending(surface)
                    .unwrap()
                    .recovery_obligation(),
                obligation
            );
        }
        assert_eq!(coordinator.retained_native_staging_resources().len(), 0);
    }
}

#[test]
fn initial_provider_handoff_keeps_the_original_recovery_owner() {
    let mut coordinator = native_coordinator();
    let surface = SurfaceId::new(1);
    let obligation = SurfaceRecoveryObligationId::new_for_test(1);
    let target = coordinator
        .start_native_surface(surface, placement(), obligation)
        .unwrap();
    let predecessor = coordinator.platform_provider().unwrap();
    let ticket = coordinator
        .begin_platform_provider_replacement(predecessor)
        .unwrap();
    let pending = coordinator.recovery_pending(surface).unwrap();
    assert_eq!(pending.source_binding(), target);
    assert_eq!(pending.recovery_obligation(), obligation);
    assert_eq!(
        pending.origin(),
        NativeSurfaceEstablishmentOrigin::Initial { binding: target }
    );
    assert_eq!(pending.replacement_binding(), None);
    assert!(matches!(
        pending.status(),
        RecoveryPendingStatus::ReplacementProviderLost { .. }
    ));
    assert!(pending.permits_recovery_retry());
    assert!(coordinator.viewport(surface).is_none());
    let successor = coordinator
        .finish_platform_provider_replacement(ticket)
        .unwrap();
    assert_ne!(predecessor, successor);
    assert_eq!(
        coordinator
            .reduce_registry_events(&[], &BTreeMap::new())
            .unwrap(),
        vec![ViewportLifecycleAction::RetryRecovery {
            source_binding: target,
            recovery_obligation: obligation,
        }]
    );
    let mut retried = coordinator.clone();
    retried
        .publish_snapshot(&native_snapshot(
            &native_coordinator(),
            1,
            Vec::new(),
            Vec::new(),
        ))
        .expect("the successor must report its own creation capabilities before retry");
    assert!(
        retried
            .recovery_pending(surface)
            .unwrap()
            .permits_initial_retry()
    );
    let binding = retried
        .start_native_surface(surface, placement(), obligation)
        .expect("provider loss must permit an explicit fresh initial attempt");
    assert_ne!(binding, target);
    assert_eq!(
        retried.recovery_pending(surface).unwrap().origin(),
        NativeSurfaceEstablishmentOrigin::Initial { binding }
    );
    assert_eq!(
        retried
            .recovery_pending(surface)
            .unwrap()
            .recovery_obligation(),
        obligation
    );
    coordinator.complete_pending_recovery(surface).unwrap();
    assert!(coordinator.recovery_pending(surface).is_none());
    assert_eq!(coordinator.retained_native_staging_resources().len(), 0);
}

#[test]
fn initial_cancel_before_dispatch_is_typed_exact_and_retains_the_published_binding_guard() {
    let mut coordinator = native_coordinator();
    let surface = SurfaceId::new(1);
    let obligation = SurfaceRecoveryObligationId::new_for_test(1);
    let target = coordinator
        .start_native_surface(surface, placement(), obligation)
        .unwrap();
    let create = coordinator
        .recovery_pending(surface)
        .unwrap()
        .replacement_effect()
        .unwrap();
    coordinator.mark_effect_boundary_published();
    let before = coordinator.clone();
    assert!(
        !coordinator
            .cancel_initial_native_surface(binding(1, 2))
            .unwrap()
    );
    assert_eq!(coordinator, before);
    assert!(coordinator.cancel_initial_native_surface(target).unwrap());
    let pending = coordinator.recovery_pending(surface).unwrap();
    assert_eq!(
        pending.origin(),
        NativeSurfaceEstablishmentOrigin::Initial { binding: target }
    );
    assert_eq!(pending.recovery_obligation(), obligation);
    assert_eq!(pending.replacement_binding(), None);
    assert_eq!(
        pending.status(),
        RecoveryPendingStatus::ReplacementCancelled {
            replacement: create
        }
    );
    assert!(pending.permits_recovery_retry());
    assert_eq!(
        coordinator.effects.record(create).unwrap().phase(),
        EffectPhase::Invalidated {
            cause: EffectInvalidation::NativeSurfaceCancelled
        }
    );
    assert!(coordinator.viewport(surface).is_none());
    assert_eq!(
        coordinator
            .binding_retention_manifest()
            .terminal_binding_guards(),
        1
    );
    assert!(coordinator.take_new_effects().is_empty());
    let before = coordinator.clone();
    assert!(!coordinator.cancel_initial_native_surface(target).unwrap());
    assert_eq!(coordinator, before);
    assert_eq!(
        coordinator
            .reduce_registry_events(&[], &BTreeMap::new())
            .unwrap(),
        vec![ViewportLifecycleAction::RetryRecovery {
            source_binding: target,
            recovery_obligation: obligation,
        }]
    );
    let guards = coordinator.binding_cleanup.terminal_bindings().clone();
    assert!(guards.contains(&target));
    let successor = coordinator
        .start_native_surface(surface, placement(), obligation)
        .expect("cancelled uncreated binding must permit a fresh initial attempt");
    assert_ne!(successor, target);
    assert_eq!(coordinator.binding_cleanup.terminal_bindings(), &guards);
    assert!(!coordinator.cancel_initial_native_surface(target).unwrap());
}

#[test]
fn initial_retry_after_dispatch_failure_uses_a_fresh_binding_and_existing_obligation() {
    {
        let mut coordinator = native_coordinator();
        let surface = SurfaceId::new(1);
        let obligation = SurfaceRecoveryObligationId::new_for_test(1);
        let predecessor = coordinator
            .start_native_surface(surface, placement(), obligation)
            .unwrap();
        let emitted = coordinator.take_new_effects();
        let create = emitted[0].id();
        assert_eq!(
            coordinator
                .report_effect(
                    predecessor.epoch(),
                    EffectResult::new(
                        create,
                        predecessor.epoch(),
                        EffectDispatchResult::DispatchFailed(
                            DispatchFailureReason::WindowUnavailable
                        ),
                    ),
                )
                .unwrap(),
            EffectTransition::Applied
        );
        let pending = coordinator.recovery_pending(surface).unwrap();
        assert!(pending.permits_initial_retry());
        assert_eq!(pending.replacement_binding(), None);
        let guards = coordinator.binding_cleanup.terminal_bindings().clone();
        assert!(guards.contains(&predecessor));
        let requested = PhysicalRect::new(50.0, 40.0, 600.0, 450.0).unwrap();
        let successor = coordinator
            .start_native_surface(surface, requested, obligation)
            .unwrap();
        assert_eq!(successor.surface(), predecessor.surface());
        assert_eq!(successor.epoch(), predecessor.epoch());
        assert_ne!(successor.incarnation(), predecessor.incarnation());
        assert_ne!(successor.token(), predecessor.token());
        let pending = coordinator.recovery_pending(surface).unwrap();
        assert_eq!(pending.source_binding(), successor);
        assert_eq!(pending.replacement_binding(), Some(successor));
        assert_eq!(pending.recovery_obligation(), obligation);
        assert_eq!(
            pending.origin(),
            NativeSurfaceEstablishmentOrigin::Initial { binding: successor }
        );
        assert!(!pending.permits_initial_retry());
        assert!(!pending.permits_recovery_retry());
        assert_eq!(coordinator.pending_recoveries().count(), 1);
        assert_eq!(
            coordinator
                .recovery_replacements
                .replacement_surface(predecessor),
            None
        );
        assert_eq!(
            coordinator
                .recovery_replacements
                .replacement_surface(successor),
            Some(surface)
        );
        assert_eq!(coordinator.binding_cleanup.terminal_bindings(), &guards);
        assert_eq!(
            coordinator.effects.record(create).unwrap().phase(),
            EffectPhase::DispatchFailed(DispatchFailureReason::WindowUnavailable)
        );
        let record = coordinator.viewport(surface).unwrap();
        assert_eq!(record.binding(), successor);
        assert_eq!(record.admission(), ViewportAdmission::Pending);
        assert!(record.coordinates().is_none());
        let emitted = coordinator.take_new_effects();
        assert!(matches!(
            emitted.as_slice(),
            [request] if request.id() != create && request.effect() == &PlatformEffect::CreateWindow {
                binding: successor,
                placement: requested,
                role: ViewportRole::Child,
            }
        ));
        assert!(coordinator.take_new_effects().is_empty());
        assert!(
            coordinator
                .reduce_registry_events(&[], &BTreeMap::new())
                .unwrap()
                .is_empty()
        );
        assert_eq!(coordinator.native_create_sagas().count(), 0);
        assert_eq!(coordinator.retained_native_staging_resources().len(), 0);
    }
}

#[test]
fn initial_retry_preserves_cancelled_binding_cleanup_across_late_appearance_and_destruction() {
    let mut coordinator = native_coordinator();
    let surface = SurfaceId::new(1);
    let obligation = SurfaceRecoveryObligationId::new_for_test(1);
    let predecessor = coordinator
        .start_native_surface(surface, placement(), obligation)
        .unwrap();
    let emitted = coordinator.take_new_effects();
    let create = emitted[0].id();
    assert!(
        coordinator
            .cancel_initial_native_surface(predecessor)
            .unwrap()
    );
    assert!(
        coordinator
            .recovery_pending(surface)
            .unwrap()
            .permits_initial_retry()
    );
    let cleanup = coordinator
        .binding_cleanup
        .get(&predecessor)
        .unwrap()
        .clone();
    let successor = coordinator
        .start_native_surface(surface, placement(), obligation)
        .unwrap();
    assert_ne!(predecessor.incarnation(), successor.incarnation());
    assert_eq!(
        coordinator.binding_cleanup.get(&predecessor),
        Some(&cleanup)
    );
    let emitted = coordinator.take_new_effects();
    assert!(
        matches!(emitted.as_slice(), [request] if matches!(request.effect(), PlatformEffect::CreateWindow { binding, .. } if *binding == successor))
    );
    let pending = coordinator.recovery_pending(surface).unwrap().clone();
    let observed_successor = ObservedWindow::new(successor)
        .with_input_state(Authority::Known(WindowInputState::ReceivesInput));
    let snapshot = native_snapshot(
        &coordinator,
        1,
        vec![
            ObservedWindow::new(predecessor)
                .with_input_state(Authority::Known(WindowInputState::ReceivesInput)),
            observed_successor.clone(),
        ],
        Vec::new(),
    );
    assert!(
        coordinator
            .publish_snapshot(&snapshot)
            .unwrap()
            .actions()
            .is_empty()
    );
    assert_eq!(coordinator.recovery_pending(surface), Some(&pending));
    assert_eq!(coordinator.viewport(surface).unwrap().binding(), successor);
    assert_eq!(
        coordinator.viewport(surface).unwrap().admission(),
        ViewportAdmission::Pending
    );
    assert!(!coordinator.binding_cleanup.contains_key(&successor));
    let emitted = coordinator.take_new_effects();
    assert!(matches!(
        emitted.as_slice(),
        [request] if request.effect() == &PlatformEffect::CompensatingClose {
            binding: predecessor,
            compensates: create,
        }
    ));
    assert_eq!(
        coordinator
            .binding_cleanup
            .get(&predecessor)
            .unwrap()
            .origin(),
        BindingRetirementOrigin::NativeSurfaceCancelled { create }
    );
    let snapshot = native_snapshot(
        &coordinator,
        2,
        vec![observed_successor],
        vec![WindowCloseObservation::new(
            predecessor,
            CloseObservationGeneration::new(1),
            Authority::Known(WindowCloseState::Destroyed),
            CloseEffectAcknowledgement::known(None),
        )],
    );
    assert!(
        coordinator
            .publish_snapshot(&snapshot)
            .unwrap()
            .actions()
            .is_empty()
    );
    assert_eq!(coordinator.recovery_pending(surface), Some(&pending));
    assert_eq!(coordinator.viewport(surface).unwrap().binding(), successor);
    assert!(!coordinator.binding_cleanup.contains_key(&predecessor));
    assert!(
        coordinator
            .binding_cleanup
            .terminal_bindings()
            .contains(&predecessor)
    );
    assert!(
        !coordinator
            .binding_cleanup
            .terminal_bindings()
            .contains(&successor)
    );
    let before = coordinator.clone();
    assert!(
        !coordinator
            .cancel_initial_native_surface(predecessor)
            .unwrap()
    );
    assert_eq!(coordinator, before);
    assert!(coordinator.take_new_effects().is_empty());
}

#[test]
fn initial_retry_rejects_foreign_obligations_and_destroyed_owners_atomically() {
    let mut coordinator = native_coordinator();
    let surface = SurfaceId::new(1);
    let obligation = SurfaceRecoveryObligationId::new_for_test(1);
    let binding = coordinator
        .start_native_surface(surface, placement(), obligation)
        .unwrap();
    assert!(coordinator.cancel_initial_native_surface(binding).unwrap());
    assert!(
        coordinator
            .recovery_pending(surface)
            .unwrap()
            .permits_initial_retry()
    );
    let before = coordinator.clone();
    assert_eq!(
        coordinator.start_native_surface(
            surface,
            placement(),
            SurfaceRecoveryObligationId::new_for_test(2)
        ),
        Err(ViewportCoordinatorError::PendingRecoveryObligationMismatch { surface })
    );
    assert_eq!(coordinator, before);

    let mut coordinator = native_coordinator();
    let destroyed = coordinator
        .register_existing(
            coordinator.workspace_epoch,
            surface,
            WindowToken::new(10),
            ViewportRole::Child,
            Some(obligation),
        )
        .unwrap();
    let snapshot = native_snapshot(
        &coordinator,
        1,
        Vec::new(),
        vec![WindowCloseObservation::new(
            destroyed,
            CloseObservationGeneration::new(1),
            Authority::Known(WindowCloseState::Destroyed),
            CloseEffectAcknowledgement::known(None),
        )],
    );
    coordinator.publish_snapshot(&snapshot).unwrap();
    coordinator
        .defer_destroyed_surface_recovery(destroyed, obligation)
        .unwrap();
    let pending = coordinator.recovery_pending(surface).unwrap();
    assert_eq!(
        pending.origin(),
        NativeSurfaceEstablishmentOrigin::Destroyed { binding: destroyed }
    );
    assert_eq!(
        pending.status(),
        RecoveryPendingStatus::AwaitingRecoveryHost
    );
    assert_eq!(pending.replacement_binding(), None);
    assert!(!pending.permits_initial_retry());
    let before = coordinator.clone();
    assert_eq!(
        coordinator.start_native_surface(surface, placement(), obligation),
        Err(ViewportCoordinatorError::PendingRecoveryRegistrationMismatch { surface })
    );
    assert_eq!(coordinator, before);
}

#[test]
fn initial_retry_allocation_failure_keeps_the_terminal_attempt_unchanged() {
    let mut coordinator = native_coordinator();
    let surface = SurfaceId::new(1);
    let obligation = SurfaceRecoveryObligationId::new_for_test(1);
    let binding = coordinator
        .start_native_surface(surface, placement(), obligation)
        .unwrap();
    assert!(coordinator.cancel_initial_native_surface(binding).unwrap());
    let mut exhausted_effects = coordinator.clone();
    exhausted_effects.effects.exhaust_effect_ids();
    let before = exhausted_effects.clone();
    assert_eq!(
        exhausted_effects.start_native_surface(surface, placement(), obligation),
        Err(ViewportCoordinatorError::Effect(
            crate::effect::EffectLedgerError::EffectIdExhausted
        ))
    );
    assert_eq!(exhausted_effects, before);
    coordinator
        .registry
        .set_native_binding_allocation_budget(Some(0));
    let before = coordinator.clone();
    assert_eq!(
        coordinator.start_native_surface(surface, placement(), obligation),
        Err(ViewportCoordinatorError::Registry(
            ViewportRegistryError::NativeBindingCapacity
        ))
    );
    assert_eq!(coordinator, before);
}

#[test]
fn initial_retry_cannot_replace_first_live_or_pending_cleanup() {
    let mut coordinator = native_coordinator();
    let surface = SurfaceId::new(1);
    let obligation = SurfaceRecoveryObligationId::new_for_test(1);
    let binding = coordinator
        .start_native_surface(surface, placement(), obligation)
        .unwrap();
    let emitted = coordinator.take_new_effects();
    let create = emitted[0].id();
    let prepared = ObservedWindow::new(binding)
        .with_coordinate_observation(WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(1),
            Authority::Known(placement()),
            Authority::Known(placement()),
            Authority::Known(ScaleFactor::new(1.0).unwrap()),
            Authority::Known(ScaleFactor::new(1.0).unwrap()),
        ))
        .with_input_state(Authority::Known(WindowInputState::ReceivesInput))
        .with_presentation_observation(WindowPresentationObservation::new(
            binding,
            PresentationObservationGeneration::new(1),
            Authority::Known(WindowPresentationState::Visible),
        ))
        .with_create_preparation(WindowCreatePreparationObservation::new(
            binding,
            CoordinateObservationGeneration::new(1),
            Authority::Known(NativeCreateReadiness::Prepared),
            Some(create),
        ));
    let snapshot = native_snapshot(&coordinator, 1, vec![prepared], Vec::new());
    let transition = coordinator.publish_snapshot(&snapshot).unwrap();
    assert_eq!(
        transition.actions(),
        &[ViewportLifecycleAction::NativeSurfaceReady {
            origin: NativeSurfaceEstablishmentOrigin::Initial { binding },
            binding,
            recovery_obligation: obligation,
        }]
    );
    assert_eq!(
        coordinator.recovery_pending(surface).unwrap().status(),
        RecoveryPendingStatus::AwaitingFirstLivePresentation
    );
    assert!(
        !coordinator
            .recovery_pending(surface)
            .unwrap()
            .permits_initial_retry()
    );
    let before = coordinator.clone();
    assert_eq!(
        coordinator.start_native_surface(surface, placement(), obligation),
        Err(ViewportCoordinatorError::PendingRecoveryRegistrationMismatch { surface })
    );
    assert_eq!(coordinator, before);

    coordinator.complete_pending_recovery(surface).unwrap();
    assert_eq!(
        coordinator.recovery_pending(surface).unwrap().status(),
        RecoveryPendingStatus::AwaitingReplacementCleanup {
            replacement: create
        }
    );
    assert!(
        !coordinator
            .recovery_pending(surface)
            .unwrap()
            .permits_initial_retry()
    );
    let before = coordinator.clone();
    assert_eq!(
        coordinator.start_native_surface(surface, placement(), obligation),
        Err(ViewportCoordinatorError::PendingRecoveryRegistrationMismatch { surface })
    );
    assert_eq!(coordinator, before);
}

#[test]
fn initial_cancel_after_dispatch_compensates_a_late_window_without_changing_the_cancel_reason() {
    let mut coordinator = native_coordinator();
    let surface = SurfaceId::new(1);
    let obligation = SurfaceRecoveryObligationId::new_for_test(1);
    let target = coordinator
        .start_native_surface(surface, placement(), obligation)
        .unwrap();
    let emitted = coordinator.take_new_effects();
    let create = emitted[0].id();
    assert!(coordinator.cancel_initial_native_surface(target).unwrap());
    assert_eq!(
        coordinator.effects.record(create).unwrap().phase(),
        EffectPhase::Requested
    );
    assert_eq!(
        coordinator.binding_cleanup.get(&target).unwrap().origin(),
        BindingRetirementOrigin::NativeSurfaceCancelled { create }
    );
    assert!(coordinator.take_new_effects().is_empty());
    let snapshot = native_snapshot(
        &coordinator,
        1,
        vec![
            ObservedWindow::new(target)
                .with_input_state(Authority::Known(WindowInputState::ReceivesInput)),
        ],
        Vec::new(),
    );
    let transition = coordinator.publish_snapshot(&snapshot).unwrap();
    assert_eq!(
        transition.actions(),
        &[ViewportLifecycleAction::RetryRecovery {
            source_binding: target,
            recovery_obligation: obligation,
        }]
    );
    let emitted = coordinator.take_new_effects();
    assert!(matches!(
        emitted.as_slice(),
        [request] if request.effect() == &PlatformEffect::CompensatingClose {
            binding: target,
            compensates: create,
        }
    ));
    assert_eq!(
        coordinator.recovery_pending(surface).unwrap().status(),
        RecoveryPendingStatus::ReplacementCancelled {
            replacement: create
        }
    );
    assert!(!coordinator.cancel_initial_native_surface(target).unwrap());
    assert_eq!(coordinator.retained_native_staging_resources().len(), 0);
}

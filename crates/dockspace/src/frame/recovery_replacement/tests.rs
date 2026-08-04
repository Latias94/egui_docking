use super::*;
use crate::ids::{EngineAuthorityDomainId, NativeCreateSagaId, WorkspaceEpoch};
use crate::viewport::{WindowIncarnation, WindowToken};

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
        None,
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

#[test]
fn one_retained_resource_cannot_back_two_pending_recoveries() {
    let resource = NativeStagingResourceId::mint(
        EngineAuthorityDomainId::new_for_test(1),
        NativeCreateSagaId::new(9),
    );
    let mut first = request(binding(1, 1), binding(1, 2));
    first.retained_staging_resource = Some(resource);
    let mut second = request(binding(2, 1), binding(2, 2));
    second.retained_staging_resource = Some(resource);
    let mut lifecycle = RecoveryReplacementLifecycle::default();
    lifecycle
        .begin(first)
        .expect("first resource owner must register");

    assert_eq!(
        lifecycle.begin(second),
        Err(RecoveryReplacementLifecycleError::DuplicateRetainedResource { resource })
    );
    assert_eq!(lifecycle.len(), 1);
}

use super::*;
use crate::ids::{EngineAuthorityDomainId, WorkspaceEpoch};
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

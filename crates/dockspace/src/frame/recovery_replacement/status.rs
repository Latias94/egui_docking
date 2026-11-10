//! Recovery replacement state and read-only public projection.

use crate::effect::EffectId;
use crate::presentation_observation::NativeStagingOwner;
use crate::surface_recovery::SurfaceRecoveryObligationId;
use crate::viewport::{
    InventoryGeneration, NativeSurfaceEstablishmentOrigin, ViewportBinding, ViewportRole,
};

use super::super::native_bringup::{NativePreparation, NativeReadyProof};

/// Queryable native establishment phase of an existing logical surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecoveryPendingStatus {
    AwaitingRecoveryHost,
    ReplacementRequested {
        replacement: EffectId,
    },
    ReplacementIndeterminate {
        replacement: EffectId,
    },
    ReplacementFailed {
        replacement: EffectId,
        failed: EffectId,
    },
    /// The platform provider changed before the replacement completed staging.
    ReplacementProviderLost {
        replacement: EffectId,
        last_effect: EffectId,
    },
    /// The owner explicitly cancelled native establishment without losing logical content.
    ReplacementCancelled {
        replacement: EffectId,
    },
    AwaitingFirstLivePresentation,
    AwaitingReplacementCleanup {
        replacement: EffectId,
    },
}

/// Structurally valid recovery replacement state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum RecoveryReplacementPhase {
    AwaitingRecoveryHost,
    ReplacementRequested {
        binding: ViewportBinding,
        create: EffectId,
    },
    ReplacementIndeterminate {
        binding: ViewportBinding,
        create: EffectId,
    },
    BringingUp {
        binding: ViewportBinding,
        phase: NativePreparation,
    },
    Failed {
        binding: Option<ViewportBinding>,
        replacement_effect: EffectId,
        failed_effect: EffectId,
    },
    ProviderLost {
        binding: Option<ViewportBinding>,
        replacement_effect: EffectId,
        last_effect: EffectId,
    },
    Cancelled {
        binding: Option<ViewportBinding>,
        replacement_effect: EffectId,
    },
    AwaitingFirstLive {
        proof: NativeReadyProof,
    },
    AwaitingCleanup {
        binding: ViewportBinding,
        replacement_effect: EffectId,
    },
}

impl RecoveryReplacementPhase {
    pub(super) fn owns_dispatch_effect(self, effect: EffectId) -> bool {
        match self {
            Self::ReplacementRequested { create, .. }
            | Self::ReplacementIndeterminate { create, .. } => create == effect,
            Self::BringingUp { phase, .. } => phase.create_effect() == effect,
            Self::Failed { failed_effect, .. } => failed_effect == effect,
            Self::AwaitingRecoveryHost
            | Self::ProviderLost { .. }
            | Self::Cancelled { .. }
            | Self::AwaitingFirstLive { .. }
            | Self::AwaitingCleanup { .. } => false,
        }
    }

    pub(super) const fn replacement_binding(self) -> Option<ViewportBinding> {
        match self {
            Self::AwaitingRecoveryHost => None,
            Self::ReplacementRequested { binding, .. }
            | Self::ReplacementIndeterminate { binding, .. }
            | Self::BringingUp { binding, .. }
            | Self::AwaitingCleanup { binding, .. } => Some(binding),
            Self::AwaitingFirstLive { proof } => Some(proof.binding()),
            Self::Failed { binding, .. }
            | Self::ProviderLost { binding, .. }
            | Self::Cancelled { binding, .. } => binding,
        }
    }

    pub(super) const fn replacement_effect(self) -> Option<EffectId> {
        match self {
            Self::AwaitingRecoveryHost => None,
            Self::ReplacementRequested { create, .. }
            | Self::ReplacementIndeterminate { create, .. } => Some(create),
            Self::BringingUp { phase, .. } => Some(phase.create_effect()),
            Self::Failed {
                replacement_effect, ..
            }
            | Self::ProviderLost {
                replacement_effect, ..
            }
            | Self::Cancelled {
                replacement_effect, ..
            }
            | Self::AwaitingCleanup {
                replacement_effect, ..
            } => Some(replacement_effect),
            Self::AwaitingFirstLive { proof, .. } => Some(proof.create()),
        }
    }

    fn status(self) -> RecoveryPendingStatus {
        match self {
            Self::AwaitingRecoveryHost => RecoveryPendingStatus::AwaitingRecoveryHost,
            Self::ReplacementRequested { create, .. } => {
                RecoveryPendingStatus::ReplacementRequested {
                    replacement: create,
                }
            }
            Self::ReplacementIndeterminate { create, .. } => {
                RecoveryPendingStatus::ReplacementIndeterminate {
                    replacement: create,
                }
            }
            Self::BringingUp { phase, .. } => RecoveryPendingStatus::ReplacementRequested {
                replacement: phase.create_effect(),
            },
            Self::AwaitingFirstLive { .. } => RecoveryPendingStatus::AwaitingFirstLivePresentation,
            Self::Failed {
                replacement_effect,
                failed_effect,
                ..
            } => RecoveryPendingStatus::ReplacementFailed {
                replacement: replacement_effect,
                failed: failed_effect,
            },
            Self::ProviderLost {
                replacement_effect,
                last_effect,
                ..
            } => RecoveryPendingStatus::ReplacementProviderLost {
                replacement: replacement_effect,
                last_effect,
            },
            Self::Cancelled {
                replacement_effect, ..
            } => RecoveryPendingStatus::ReplacementCancelled {
                replacement: replacement_effect,
            },
            Self::AwaitingCleanup {
                replacement_effect, ..
            } => RecoveryPendingStatus::AwaitingReplacementCleanup {
                replacement: replacement_effect,
            },
        }
    }
}

/// Native establishment retained until a contained host or exact native output is ready.
#[derive(Debug, Clone, PartialEq)]
pub struct RecoveryPending {
    pub(super) origin: NativeSurfaceEstablishmentOrigin,
    pub(super) role: ViewportRole,
    pub(super) recovery_obligation: SurfaceRecoveryObligationId,
    pub(super) phase: RecoveryReplacementPhase,
    pub(super) admission_not_before: Option<InventoryGeneration>,
}

impl RecoveryPending {
    #[must_use]
    pub const fn source_binding(&self) -> ViewportBinding {
        self.origin.binding()
    }

    pub(crate) const fn origin(&self) -> NativeSurfaceEstablishmentOrigin {
        self.origin
    }

    #[must_use]
    pub const fn role(&self) -> ViewportRole {
        self.role
    }

    pub(crate) const fn recovery_obligation(&self) -> SurfaceRecoveryObligationId {
        self.recovery_obligation
    }

    #[must_use]
    pub const fn replacement_binding(&self) -> Option<ViewportBinding> {
        self.phase.replacement_binding()
    }

    #[must_use]
    pub const fn replacement_effect(&self) -> Option<EffectId> {
        self.phase.replacement_effect()
    }

    #[must_use]
    pub fn status(&self) -> RecoveryPendingStatus {
        self.phase.status()
    }

    pub(in crate::frame) const fn staging_owner(&self) -> NativeStagingOwner {
        match self.origin {
            NativeSurfaceEstablishmentOrigin::Destroyed { binding } => {
                NativeStagingOwner::recovery_replacement(binding, self.recovery_obligation)
            }
            NativeSurfaceEstablishmentOrigin::Initial { binding } => {
                NativeStagingOwner::InitialSurface {
                    binding,
                    recovery_obligation: self.recovery_obligation,
                }
            }
        }
    }

    /// An explicit initial retry may replace only a detached terminal attempt.
    pub(crate) const fn permits_initial_retry(&self) -> bool {
        matches!(
            (self.origin, self.phase),
            (
                NativeSurfaceEstablishmentOrigin::Initial { .. },
                RecoveryReplacementPhase::AwaitingRecoveryHost
                    | RecoveryReplacementPhase::Failed { binding: None, .. }
                    | RecoveryReplacementPhase::ProviderLost { binding: None, .. }
                    | RecoveryReplacementPhase::Cancelled { binding: None, .. }
            )
        )
    }

    pub(crate) const fn permits_recovery_retry(&self) -> bool {
        match self.phase {
            RecoveryReplacementPhase::AwaitingRecoveryHost
            | RecoveryReplacementPhase::Failed { .. }
            | RecoveryReplacementPhase::ProviderLost { .. }
            | RecoveryReplacementPhase::Cancelled { .. } => true,
            RecoveryReplacementPhase::ReplacementRequested { .. }
            | RecoveryReplacementPhase::ReplacementIndeterminate { .. }
            | RecoveryReplacementPhase::BringingUp { .. } => matches!(
                self.origin,
                NativeSurfaceEstablishmentOrigin::Destroyed { .. }
            ),
            RecoveryReplacementPhase::AwaitingFirstLive { .. }
            | RecoveryReplacementPhase::AwaitingCleanup { .. } => false,
        }
    }

    pub(in crate::frame) const fn first_live_proof(&self) -> Option<NativeReadyProof> {
        match self.phase {
            RecoveryReplacementPhase::AwaitingFirstLive { proof } => Some(proof),
            _ => None,
        }
    }

    pub(in crate::frame) const fn suspends_live_presentation(&self) -> bool {
        !matches!(
            self.phase,
            RecoveryReplacementPhase::AwaitingFirstLive { .. }
        )
    }

    pub(in crate::frame) const fn admission_not_before(&self) -> Option<InventoryGeneration> {
        self.admission_not_before
    }
}

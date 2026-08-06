//! Recovery replacement state and read-only public projection.

use crate::effect::EffectId;
use crate::presentation_observation::NativeStagingResourceId;
use crate::surface_recovery::SurfaceRecoveryObligationId;
use crate::viewport::{InventoryGeneration, ViewportBinding, ViewportRole};

use super::super::native_bringup::{NativeBringupPhase, NativeVisibleProof};

/// Queryable recovery phase after a native surface was authoritatively destroyed.
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
    AwaitingPreShowPresentation {
        replacement: EffectId,
    },
    AwaitingShowAcknowledgement {
        replacement: EffectId,
        show: EffectId,
    },
    AwaitingVisible {
        replacement: EffectId,
        show: EffectId,
    },
    AwaitingPostShowPresentation {
        replacement: EffectId,
        show: EffectId,
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
        phase: NativeBringupPhase,
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
    AwaitingFirstLive {
        proof: NativeVisibleProof,
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
            Self::BringingUp { phase, .. } => {
                phase.create_effect() == effect || phase.show_effect() == Some(effect)
            }
            Self::Failed { failed_effect, .. } => failed_effect == effect,
            Self::AwaitingRecoveryHost
            | Self::ProviderLost { .. }
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
            Self::Failed { binding, .. } | Self::ProviderLost { binding, .. } => binding,
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
            Self::BringingUp {
                phase: NativeBringupPhase::AwaitingHidden { .. },
                ..
            } => unreachable!("initial recovery replacement phase has a dedicated state"),
            Self::BringingUp {
                phase: NativeBringupPhase::AwaitingPreShowPresentation { create, .. },
                ..
            } => RecoveryPendingStatus::AwaitingPreShowPresentation {
                replacement: create,
            },
            Self::BringingUp {
                phase: NativeBringupPhase::AwaitingShowAcknowledgement { create, show, .. },
                ..
            } => RecoveryPendingStatus::AwaitingShowAcknowledgement {
                replacement: create,
                show,
            },
            Self::BringingUp {
                phase: NativeBringupPhase::AwaitingVisible { create, show, .. },
                ..
            } => RecoveryPendingStatus::AwaitingVisible {
                replacement: create,
                show,
            },
            Self::BringingUp {
                phase: NativeBringupPhase::AwaitingPostShowPresentation { visibility, .. },
                ..
            } => RecoveryPendingStatus::AwaitingPostShowPresentation {
                replacement: visibility.create,
                show: visibility.show,
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
            Self::AwaitingCleanup {
                replacement_effect, ..
            } => RecoveryPendingStatus::AwaitingReplacementCleanup {
                replacement: replacement_effect,
            },
        }
    }
}

/// Whole-root recovery retained while neither a contained host nor replacement is ready.
#[derive(Debug, Clone, PartialEq)]
pub struct RecoveryPending {
    pub(super) destroyed_binding: ViewportBinding,
    pub(super) role: ViewportRole,
    pub(super) recovery_obligation: SurfaceRecoveryObligationId,
    pub(super) retained_staging_resource: Option<NativeStagingResourceId>,
    pub(super) phase: RecoveryReplacementPhase,
    pub(super) admission_not_before: Option<InventoryGeneration>,
}

impl RecoveryPending {
    #[must_use]
    pub const fn destroyed_binding(&self) -> ViewportBinding {
        self.destroyed_binding
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

    /// Returns the retained source resource associated with a transferred native create.
    #[must_use]
    pub const fn retained_staging_resource(&self) -> Option<NativeStagingResourceId> {
        self.retained_staging_resource
    }

    #[must_use]
    pub fn status(&self) -> RecoveryPendingStatus {
        self.phase.status()
    }

    pub(in crate::frame) const fn bringup_phase(&self) -> Option<NativeBringupPhase> {
        match self.phase {
            RecoveryReplacementPhase::BringingUp { phase, .. } => Some(phase),
            RecoveryReplacementPhase::AwaitingRecoveryHost
            | RecoveryReplacementPhase::ReplacementRequested { .. }
            | RecoveryReplacementPhase::ReplacementIndeterminate { .. }
            | RecoveryReplacementPhase::Failed { .. }
            | RecoveryReplacementPhase::ProviderLost { .. }
            | RecoveryReplacementPhase::AwaitingFirstLive { .. }
            | RecoveryReplacementPhase::AwaitingCleanup { .. } => None,
        }
    }

    pub(in crate::frame) const fn first_live_proof(&self) -> Option<NativeVisibleProof> {
        match self.phase {
            RecoveryReplacementPhase::AwaitingFirstLive { proof, .. } => Some(proof),
            RecoveryReplacementPhase::AwaitingRecoveryHost
            | RecoveryReplacementPhase::ReplacementRequested { .. }
            | RecoveryReplacementPhase::ReplacementIndeterminate { .. }
            | RecoveryReplacementPhase::BringingUp { .. }
            | RecoveryReplacementPhase::Failed { .. }
            | RecoveryReplacementPhase::ProviderLost { .. }
            | RecoveryReplacementPhase::AwaitingCleanup { .. } => None,
        }
    }

    pub(in crate::frame) const fn suspends_live_presentation(&self) -> bool {
        match self.phase {
            RecoveryReplacementPhase::AwaitingFirstLive { .. } => false,
            RecoveryReplacementPhase::AwaitingRecoveryHost
            | RecoveryReplacementPhase::ReplacementRequested { .. }
            | RecoveryReplacementPhase::ReplacementIndeterminate { .. }
            | RecoveryReplacementPhase::BringingUp { .. }
            | RecoveryReplacementPhase::Failed { .. }
            | RecoveryReplacementPhase::ProviderLost { .. }
            | RecoveryReplacementPhase::AwaitingCleanup { .. } => true,
        }
    }

    pub(in crate::frame) const fn admission_not_before(&self) -> Option<InventoryGeneration> {
        self.admission_not_before
    }
}

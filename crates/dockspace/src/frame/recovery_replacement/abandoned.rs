//! Affine transfer from failed recovery bring-up into exact binding cleanup.

use crate::effect::EffectId;

use super::StagingCloseRetirementTransfer;

/// Why a pre-admission replacement stopped being a valid bring-up owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::frame) enum AbandonedReplacementCause {
    DispatchFailed { failed_effect: EffectId },
    ProviderLost { last_effect: EffectId },
}

/// Abandoned replacement identity detached together with its optional close owner.
#[derive(Debug, PartialEq, Eq)]
pub(in crate::frame) struct AbandonedReplacementDetach {
    pub(super) replacement_effect: EffectId,
    pub(super) cause: AbandonedReplacementCause,
    pub(super) staging_close: Option<StagingCloseRetirementTransfer>,
}

impl AbandonedReplacementDetach {
    pub(in crate::frame) fn into_parts(
        self,
    ) -> (
        EffectId,
        AbandonedReplacementCause,
        Option<StagingCloseRetirementTransfer>,
    ) {
        (self.replacement_effect, self.cause, self.staging_close)
    }
}

//! Affine transfer from abandoned native bring-up into exact binding cleanup.

use crate::platform::effect::EffectId;

/// Why a pre-admission replacement stopped being a valid bring-up owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::platform::frame) enum AbandonedReplacementCause {
    DispatchFailed { failed_effect: EffectId },
    ProviderLost { last_effect: EffectId },
    Cancelled,
}

/// Abandoned replacement identity detached from the recovery owner.
#[derive(Debug, PartialEq, Eq)]
pub(in crate::platform::frame) struct AbandonedReplacementDetach {
    pub(super) replacement_effect: EffectId,
    pub(super) cause: AbandonedReplacementCause,
}

impl AbandonedReplacementDetach {
    pub(in crate::platform::frame) fn into_parts(self) -> (EffectId, AbandonedReplacementCause) {
        (self.replacement_effect, self.cause)
    }
}

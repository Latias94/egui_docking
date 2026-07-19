//! Core-owned platform effect ledger.
//!
//! Dispatch acknowledgement is deliberately absent: an adapter call returning
//! successfully does not prove that a window exists, a flag changed, or a close
//! completed. Only matching authoritative observations can do that.

use std::collections::BTreeMap;

use thiserror::Error;

use crate::geometry::PhysicalRect;
use crate::ids::WorkspaceEpoch;
use crate::viewport::{InventoryGeneration, ViewportBinding, ViewportRole};

/// Monotonic identity of one exact platform side effect.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct EffectId(u64);

impl EffectId {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    #[must_use]
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

/// Exact adapter operation requested by the core.
#[derive(Debug, Clone, PartialEq)]
pub enum PlatformEffect {
    /// Create one hidden native window at the exact requested placement.
    ///
    /// The adapter must not expose it for input until the core later emits
    /// [`PlatformEffect::ShowWindow`].
    CreateWindow {
        binding: ViewportBinding,
        placement: PhysicalRect,
        role: ViewportRole,
    },
    /// Show a previously created hidden window after topology commit.
    ShowWindow {
        binding: ViewportBinding,
    },
    CompensatingClose {
        binding: ViewportBinding,
        compensates: EffectId,
    },
    CancelRootClose {
        binding: ViewportBinding,
    },
    RetainChild {
        binding: ViewportBinding,
    },
    ReleaseChild {
        binding: ViewportBinding,
    },
    RequestRootClose {
        binding: ViewportBinding,
    },
    SetPointerPassthrough {
        binding: ViewportBinding,
        enabled: bool,
    },
    /// Ask the adapter to focus one exact current window incarnation.
    RequestFocus {
        binding: ViewportBinding,
    },
    RequestReplacement {
        binding: ViewportBinding,
        placement: PhysicalRect,
        role: ViewportRole,
    },
}

impl PlatformEffect {
    #[must_use]
    pub const fn binding(&self) -> ViewportBinding {
        match self {
            Self::CreateWindow { binding, .. }
            | Self::ShowWindow { binding }
            | Self::CompensatingClose { binding, .. }
            | Self::CancelRootClose { binding }
            | Self::RetainChild { binding }
            | Self::ReleaseChild { binding }
            | Self::RequestRootClose { binding }
            | Self::SetPointerPassthrough { binding, .. }
            | Self::RequestFocus { binding }
            | Self::RequestReplacement { binding, .. } => *binding,
        }
    }

    #[must_use]
    pub const fn is_non_idempotent(&self) -> bool {
        matches!(
            self,
            Self::CreateWindow { .. }
                | Self::CompensatingClose { .. }
                | Self::ReleaseChild { .. }
                | Self::RequestRootClose { .. }
                | Self::RequestReplacement { .. }
        )
    }
}

/// One immutable request emitted to the adapter exactly once.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectRequest {
    id: EffectId,
    epoch: WorkspaceEpoch,
    effect: PlatformEffect,
}

impl EffectRequest {
    #[must_use]
    pub const fn id(&self) -> EffectId {
        self.id
    }

    #[must_use]
    pub const fn epoch(&self) -> WorkspaceEpoch {
        self.epoch
    }

    #[must_use]
    pub const fn effect(&self) -> &PlatformEffect {
        &self.effect
    }
}

/// Adapter-level reason dispatch did not occur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DispatchFailureReason {
    AdapterRejected,
    WindowUnavailable,
    ProviderStopped,
}

/// Why an adapter authoritatively cannot execute an effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectUnsupportedReason {
    BackendUnsupported,
    CapabilityRevoked,
}

/// Why dispatch outcome can no longer be established.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectIndeterminateReason {
    AcknowledgementLost,
    ProviderRestarted,
}

/// Public phase of one effect ledger record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectPhase {
    Requested,
    DispatchFailed(DispatchFailureReason),
    ObservedApplied {
        inventory_generation: InventoryGeneration,
    },
    Unsupported(EffectUnsupportedReason),
    Indeterminate(EffectIndeterminateReason),
    Destroyed {
        inventory_generation: InventoryGeneration,
    },
    /// The request was never emitted and a newer workspace epoch superseded it.
    InvalidatedByRestore {
        replacement_epoch: WorkspaceEpoch,
    },
}

/// Result an adapter may report without claiming platform state changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectDispatchResult {
    DispatchFailed(DispatchFailureReason),
    Unsupported(EffectUnsupportedReason),
    Indeterminate(EffectIndeterminateReason),
}

/// Correlated adapter result for one effect request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EffectResult {
    effect: EffectId,
    epoch: WorkspaceEpoch,
    result: EffectDispatchResult,
}

impl EffectResult {
    #[must_use]
    pub const fn new(
        effect: EffectId,
        epoch: WorkspaceEpoch,
        result: EffectDispatchResult,
    ) -> Self {
        Self {
            effect,
            epoch,
            result,
        }
    }

    #[must_use]
    pub const fn effect(self) -> EffectId {
        self.effect
    }

    #[must_use]
    pub const fn epoch(self) -> WorkspaceEpoch {
        self.epoch
    }

    #[must_use]
    pub const fn result(self) -> EffectDispatchResult {
        self.result
    }
}

/// Queryable immutable request and current phase.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectRecord {
    request: EffectRequest,
    phase: EffectPhase,
    emitted: bool,
}

impl EffectRecord {
    #[must_use]
    pub const fn request(&self) -> &EffectRequest {
        &self.request
    }

    #[must_use]
    pub const fn phase(&self) -> EffectPhase {
        self.phase
    }

    #[must_use]
    pub const fn was_emitted(&self) -> bool {
        self.emitted
    }
}

/// Deterministic outcome of a stale, duplicate, or accepted ledger transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectTransition {
    Applied,
    Duplicate,
    StaleEpoch,
    UnknownEffect,
    BindingMismatch,
}

/// Core-owned ledger retaining exact effect identity and outcome.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EffectLedger {
    last_effect: EffectId,
    records: BTreeMap<EffectId, EffectRecord>,
}

impl EffectLedger {
    /// Adds one request without publishing it to an adapter yet.
    ///
    /// # Errors
    ///
    /// Returns [`EffectLedgerError::EffectIdExhausted`] rather than wrapping.
    pub(crate) fn request(
        &mut self,
        effect: PlatformEffect,
    ) -> Result<EffectId, EffectLedgerError> {
        self.request_in(effect.binding().epoch(), effect)
    }

    /// Adds a request issued by `issuance_epoch`, which may target an older binding.
    ///
    /// Restore reconciliation uses this to clean up windows which can appear after their
    /// original workspace epoch has been replaced.
    pub(crate) fn request_in(
        &mut self,
        issuance_epoch: WorkspaceEpoch,
        effect: PlatformEffect,
    ) -> Result<EffectId, EffectLedgerError> {
        let id = self
            .last_effect
            .checked_next()
            .ok_or(EffectLedgerError::EffectIdExhausted)?;
        let request = EffectRequest {
            id,
            epoch: issuance_epoch,
            effect,
        };
        self.records.insert(
            id,
            EffectRecord {
                request,
                phase: EffectPhase::Requested,
                emitted: false,
            },
        );
        self.last_effect = id;
        Ok(id)
    }

    /// Prevents never-emitted requests from older epochs from reaching an adapter after restore.
    pub(crate) fn invalidate_unemitted_before(&mut self, replacement_epoch: WorkspaceEpoch) {
        for record in self.records.values_mut() {
            if record.request.epoch < replacement_epoch
                && !record.emitted
                && matches!(record.phase, EffectPhase::Requested)
            {
                record.phase = EffectPhase::InvalidatedByRestore { replacement_epoch };
            }
        }
    }

    /// Returns each newly requested effect exactly once and marks it emitted.
    pub(crate) fn take_new_requests(&mut self) -> Vec<EffectRequest> {
        let mut requests = Vec::new();
        for record in self.records.values_mut() {
            if !record.emitted && matches!(record.phase, EffectPhase::Requested) {
                record.emitted = true;
                requests.push(record.request.clone());
            }
        }
        requests
    }

    /// Applies a result which cannot claim the platform state changed.
    pub(crate) fn report(
        &mut self,
        current_epoch: WorkspaceEpoch,
        result: EffectResult,
    ) -> EffectTransition {
        if result.epoch != current_epoch {
            return EffectTransition::StaleEpoch;
        }
        let Some(record) = self.records.get_mut(&result.effect) else {
            return EffectTransition::UnknownEffect;
        };
        if record.request.epoch != result.epoch {
            return EffectTransition::StaleEpoch;
        }
        let phase = match result.result {
            EffectDispatchResult::DispatchFailed(reason) => EffectPhase::DispatchFailed(reason),
            EffectDispatchResult::Unsupported(reason) => EffectPhase::Unsupported(reason),
            EffectDispatchResult::Indeterminate(reason) => EffectPhase::Indeterminate(reason),
        };
        if record.phase == phase {
            return EffectTransition::Duplicate;
        }
        if !matches!(record.phase, EffectPhase::Requested) {
            return EffectTransition::Duplicate;
        }
        record.phase = phase;
        EffectTransition::Applied
    }

    pub(crate) fn mark_observed_applied(
        &mut self,
        effect: EffectId,
        binding: ViewportBinding,
        inventory_generation: InventoryGeneration,
    ) -> EffectTransition {
        self.mark_observation(
            effect,
            binding,
            EffectPhase::ObservedApplied {
                inventory_generation,
            },
        )
    }

    pub(crate) fn mark_destroyed(
        &mut self,
        effect: EffectId,
        binding: ViewportBinding,
        inventory_generation: InventoryGeneration,
    ) -> EffectTransition {
        self.mark_observation(
            effect,
            binding,
            EffectPhase::Destroyed {
                inventory_generation,
            },
        )
    }

    fn mark_observation(
        &mut self,
        effect: EffectId,
        binding: ViewportBinding,
        phase: EffectPhase,
    ) -> EffectTransition {
        let Some(record) = self.records.get_mut(&effect) else {
            return EffectTransition::UnknownEffect;
        };
        if record.request.effect.binding() != binding {
            return EffectTransition::BindingMismatch;
        }
        if record.phase == phase {
            return EffectTransition::Duplicate;
        }
        if matches!(
            record.phase,
            EffectPhase::ObservedApplied { .. }
                | EffectPhase::Destroyed { .. }
                | EffectPhase::InvalidatedByRestore { .. }
        ) {
            return EffectTransition::Duplicate;
        }
        record.phase = phase;
        EffectTransition::Applied
    }

    #[must_use]
    pub fn record(&self, effect: EffectId) -> Option<&EffectRecord> {
        self.records.get(&effect)
    }

    pub fn records(&self) -> impl Iterator<Item = (EffectId, &EffectRecord)> {
        self.records.iter().map(|(id, record)| (*id, record))
    }

    #[cfg(test)]
    pub(crate) fn exhaust_effect_ids(&mut self) {
        self.last_effect = EffectId::new(u64::MAX);
    }
}

/// Fatal ledger allocation failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum EffectLedgerError {
    #[error("platform effect identity is exhausted")]
    EffectIdExhausted,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::SurfaceId;
    use crate::viewport::{WindowIncarnation, WindowToken};

    fn binding(epoch: u64, incarnation: u64) -> ViewportBinding {
        ViewportBinding::new(
            WorkspaceEpoch::new(epoch),
            SurfaceId::new(2),
            WindowToken::new(3),
            WindowIncarnation::new(incarnation),
        )
    }

    fn create_effect(binding: ViewportBinding) -> PlatformEffect {
        PlatformEffect::CreateWindow {
            binding,
            placement: PhysicalRect::new(0.0, 0.0, 100.0, 80.0)
                .expect("test placement must be valid"),
            role: ViewportRole::Child,
        }
    }

    #[test]
    fn request_is_emitted_once_and_indeterminate_never_redispatches() {
        let mut ledger = EffectLedger::default();
        let binding = binding(1, 1);
        let effect = ledger
            .request(create_effect(binding))
            .expect("effect identity must be available");
        assert_eq!(ledger.take_new_requests().len(), 1);
        assert!(ledger.take_new_requests().is_empty());

        assert_eq!(
            ledger.report(
                WorkspaceEpoch::new(1),
                EffectResult::new(
                    effect,
                    WorkspaceEpoch::new(1),
                    EffectDispatchResult::Indeterminate(
                        EffectIndeterminateReason::AcknowledgementLost,
                    ),
                ),
            ),
            EffectTransition::Applied
        );
        assert!(ledger.take_new_requests().is_empty());
    }

    #[test]
    fn stale_duplicate_and_binding_mismatch_results_are_harmless() {
        let mut ledger = EffectLedger::default();
        let current_binding = binding(4, 1);
        let effect = ledger
            .request(create_effect(current_binding))
            .expect("effect identity must be available");
        let result = EffectResult::new(
            effect,
            WorkspaceEpoch::new(4),
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::AdapterRejected),
        );
        assert_eq!(
            ledger.report(WorkspaceEpoch::new(5), result),
            EffectTransition::StaleEpoch
        );
        assert_eq!(
            ledger.report(WorkspaceEpoch::new(4), result),
            EffectTransition::Applied
        );
        assert_eq!(
            ledger.report(WorkspaceEpoch::new(4), result),
            EffectTransition::Duplicate
        );
        assert_eq!(
            ledger.mark_observed_applied(effect, binding(4, 2), InventoryGeneration::new(1)),
            EffectTransition::BindingMismatch
        );
        assert_eq!(
            ledger.mark_observed_applied(effect, current_binding, InventoryGeneration::new(2)),
            EffectTransition::Applied
        );
    }

    #[test]
    fn effect_identity_exhaustion_does_not_mutate_the_ledger() {
        let mut ledger = EffectLedger::default();
        ledger.exhaust_effect_ids();
        let before = ledger.clone();
        assert_eq!(
            ledger.request(create_effect(binding(0, 1))),
            Err(EffectLedgerError::EffectIdExhausted)
        );
        assert_eq!(ledger, before);
    }

    #[test]
    fn restore_can_issue_cleanup_for_an_exact_older_binding() {
        let mut ledger = EffectLedger::default();
        let old_binding = binding(3, 1);
        let effect = ledger
            .request_in(
                WorkspaceEpoch::new(4),
                PlatformEffect::CompensatingClose {
                    binding: old_binding,
                    compensates: EffectId::new(7),
                },
            )
            .expect("cleanup identity must be available");
        let request = ledger
            .take_new_requests()
            .pop()
            .expect("cleanup must be emitted");
        assert_eq!(request.epoch(), WorkspaceEpoch::new(4));
        assert_eq!(request.effect().binding(), old_binding);
        assert_eq!(
            ledger.mark_destroyed(effect, old_binding, InventoryGeneration::new(9)),
            EffectTransition::Applied
        );
    }

    #[test]
    fn restore_invalidates_only_never_emitted_older_requests() {
        let mut ledger = EffectLedger::default();
        let first_old_emitted = ledger
            .request(create_effect(binding(1, 1)))
            .expect("old request must allocate");
        let old_emitted = ledger
            .request(create_effect(binding(2, 2)))
            .expect("second request must allocate");
        let _ = ledger.take_new_requests();
        let current = ledger
            .request(create_effect(binding(3, 3)))
            .expect("current request must allocate");

        // Create a distinct never-emitted old request after draining the earlier records.
        let late_old = ledger
            .request_in(WorkspaceEpoch::new(1), create_effect(binding(1, 4)))
            .expect("late old request must allocate");
        ledger.invalidate_unemitted_before(WorkspaceEpoch::new(3));

        assert!(
            ledger
                .record(first_old_emitted)
                .is_some_and(EffectRecord::was_emitted)
        );
        assert!(
            ledger
                .record(old_emitted)
                .is_some_and(EffectRecord::was_emitted)
        );
        assert_eq!(
            ledger.record(late_old).map(EffectRecord::phase),
            Some(EffectPhase::InvalidatedByRestore {
                replacement_epoch: WorkspaceEpoch::new(3),
            })
        );
        let expected_current = ledger
            .record(current)
            .expect("current record must remain")
            .request()
            .clone();
        assert_eq!(ledger.take_new_requests(), vec![expected_current]);
    }
}

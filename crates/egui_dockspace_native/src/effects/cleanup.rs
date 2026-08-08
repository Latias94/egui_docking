//! Exact rendezvous between delayed destructive results and cleanup observers.

use std::collections::BTreeMap;

use dockspace::effect::{EffectDispatchResult, EffectId};
use dockspace::ids::WorkspaceEpoch;
use egui_dockspace::backend::ExactNativeViewport;
#[cfg(test)]
use egui_dockspace::backend::NativeViewportIncarnation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PendingCleanupResult<Binding> {
    epoch: WorkspaceEpoch,
    result: EffectDispatchResult,
    binding: Binding,
    native: Option<ExactNativeViewport>,
}

impl<Binding: Copy> PendingCleanupResult<Binding> {
    pub(super) const fn local(
        epoch: WorkspaceEpoch,
        result: EffectDispatchResult,
        binding: Binding,
    ) -> Self {
        Self {
            epoch,
            result,
            binding,
            native: None,
        }
    }

    pub(super) const fn native(
        epoch: WorkspaceEpoch,
        result: EffectDispatchResult,
        binding: Binding,
        native: ExactNativeViewport,
    ) -> Self {
        Self {
            epoch,
            result,
            binding,
            native: Some(native),
        }
    }

    pub(super) const fn epoch(self) -> WorkspaceEpoch {
        self.epoch
    }

    pub(super) const fn result(self) -> EffectDispatchResult {
        self.result
    }

    const fn native_viewport(self) -> Option<ExactNativeViewport> {
        self.native
    }

    const fn is_indeterminate(self) -> bool {
        matches!(self.result, EffectDispatchResult::Indeterminate(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CleanupObservation<Token, Provider, Binding> {
    token: Token,
    binding: Binding,
    native: Option<ExactNativeViewport>,
    receipt_epoch: WorkspaceEpoch,
    provider: Provider,
    provider_generation: u64,
    scheduled: Option<PendingCleanupResult<Binding>>,
}

impl<Token: Copy, Provider: Copy, Binding: Copy> CleanupObservation<Token, Provider, Binding> {
    pub(super) const fn new(
        token: Token,
        binding: Binding,
        native: ExactNativeViewport,
        receipt_epoch: WorkspaceEpoch,
        provider: Provider,
        provider_generation: u64,
    ) -> Self {
        Self {
            token,
            binding,
            native: Some(native),
            receipt_epoch,
            provider,
            provider_generation,
            scheduled: None,
        }
    }

    /// Correlates a synchronous adapter rejection after its native route disappeared.
    ///
    /// Registration accepts this observation only when the exact predecessor and binding
    /// already own a buffered local result. Native callbacks must continue to use [`Self::new`]
    /// so their viewport incarnation remains part of the authority proof.
    pub(super) const fn local(
        token: Token,
        binding: Binding,
        receipt_epoch: WorkspaceEpoch,
        provider: Provider,
        provider_generation: u64,
    ) -> Self {
        Self {
            token,
            binding,
            native: None,
            receipt_epoch,
            provider,
            provider_generation,
            scheduled: None,
        }
    }

    pub(super) const fn token(self) -> Token {
        self.token
    }

    pub(super) const fn receipt_epoch(self) -> WorkspaceEpoch {
        self.receipt_epoch
    }

    const fn authority_frontier(self) -> (WorkspaceEpoch, u64) {
        (self.receipt_epoch, self.provider_generation)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SettledCleanupResult<Binding> {
    effect: EffectId,
    pending: PendingCleanupResult<Binding>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CleanupDelivery<Token, Provider, Binding> {
    predecessor: EffectId,
    observation: CleanupObservation<Token, Provider, Binding>,
    pending: PendingCleanupResult<Binding>,
}

impl<Token: Copy, Provider: Copy, Binding: Copy> CleanupDelivery<Token, Provider, Binding> {
    pub(super) const fn predecessor(self) -> EffectId {
        self.predecessor
    }

    pub(super) const fn token(self) -> Token {
        self.observation.token()
    }

    pub(super) const fn receipt_epoch(self) -> WorkspaceEpoch {
        self.observation.receipt_epoch()
    }

    pub(super) const fn pending(self) -> PendingCleanupResult<Binding> {
        self.pending
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CleanupDisposition<Token, Provider, Binding> {
    Buffered,
    SettledDuplicate,
    Deliver(CleanupDelivery<Token, Provider, Binding>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CleanupRendezvousError {
    NativeMismatch,
    LocalResultUnavailable,
    StaleObservation,
    ConflictingObservation,
    ConflictingResult,
    DuplicateIngressOrdinal,
}

#[derive(Clone, Debug)]
pub(super) struct CleanupRendezvous<Token, Provider, Binding> {
    observations: BTreeMap<EffectId, CleanupObservation<Token, Provider, Binding>>,
    pending: BTreeMap<EffectId, PendingCleanupResult<Binding>>,
    settled: BTreeMap<ExactNativeViewport, SettledCleanupResult<Binding>>,
    inflight: BTreeMap<u64, CleanupDelivery<Token, Provider, Binding>>,
}

impl<Token, Provider, Binding> Default for CleanupRendezvous<Token, Provider, Binding> {
    fn default() -> Self {
        Self {
            observations: BTreeMap::new(),
            pending: BTreeMap::new(),
            settled: BTreeMap::new(),
            inflight: BTreeMap::new(),
        }
    }
}

impl<Token, Provider, Binding> CleanupRendezvous<Token, Provider, Binding>
where
    Token: Copy + Eq,
    Provider: Copy + Eq,
    Binding: Copy + Eq,
{
    pub(super) fn register_observation(
        &mut self,
        predecessor: EffectId,
        observation: CleanupObservation<Token, Provider, Binding>,
    ) -> Result<CleanupDisposition<Token, Provider, Binding>, CleanupRendezvousError> {
        if let Some(native) = observation.native {
            if self
                .settled
                .get(&native)
                .is_some_and(|settled| settled.effect >= predecessor)
            {
                return Ok(CleanupDisposition::SettledDuplicate);
            }
        } else if !self.pending.get(&predecessor).is_some_and(|pending| {
            pending.binding == observation.binding && pending.native.is_none()
        }) {
            return Err(CleanupRendezvousError::LocalResultUnavailable);
        }
        if self.pending.get(&predecessor).is_some_and(|pending| {
            pending.binding != observation.binding
                || pending
                    .native
                    .is_some_and(|native| observation.native != Some(native))
        }) {
            return Err(CleanupRendezvousError::NativeMismatch);
        }

        match self.observations.get(&predecessor).copied() {
            Some(existing)
                if existing.token == observation.token
                    && existing.native == observation.native
                    && existing.receipt_epoch == observation.receipt_epoch
                    && existing.provider == observation.provider
                    && existing.provider_generation == observation.provider_generation => {}
            Some(existing) if existing.native != observation.native => {
                return Err(CleanupRendezvousError::NativeMismatch);
            }
            Some(existing) if observation.authority_frontier() < existing.authority_frontier() => {
                return Err(CleanupRendezvousError::StaleObservation);
            }
            Some(existing) if observation.authority_frontier() == existing.authority_frontier() => {
                return Err(CleanupRendezvousError::ConflictingObservation);
            }
            Some(_) | None => {
                self.observations.insert(predecessor, observation);
            }
        }

        self.prepare_delivery(predecessor)
    }

    pub(super) fn record_result(
        &mut self,
        predecessor: EffectId,
        pending: PendingCleanupResult<Binding>,
        current_epoch: WorkspaceEpoch,
        current_provider: Provider,
    ) -> Result<CleanupDisposition<Token, Provider, Binding>, CleanupRendezvousError> {
        if let Some(native) = pending.native_viewport()
            && let Some(settled) = self.settled.get(&native).copied()
            && settled.effect >= predecessor
        {
            return if settled.effect > predecessor || settled.pending == pending {
                Ok(CleanupDisposition::SettledDuplicate)
            } else {
                Err(CleanupRendezvousError::ConflictingResult)
            };
        }

        match self.pending.get(&predecessor).copied() {
            None => {
                self.pending.insert(predecessor, pending);
            }
            Some(existing) if existing == pending => {}
            Some(existing)
                if existing.epoch == pending.epoch
                    && existing.binding == pending.binding
                    && existing.native == pending.native
                    && existing.is_indeterminate()
                    && !pending.is_indeterminate() =>
            {
                self.pending.insert(predecessor, pending);
            }
            Some(_) => return Err(CleanupRendezvousError::ConflictingResult),
        }

        let observation_matches =
            self.observations
                .get(&predecessor)
                .copied()
                .is_some_and(|observation| {
                    observation.binding == pending.binding
                        && pending
                            .native
                            .is_none_or(|native| observation.native == Some(native))
                        && observation.receipt_epoch == current_epoch
                        && observation.provider == current_provider
                });
        if !observation_matches {
            return Ok(CleanupDisposition::Buffered);
        }
        self.prepare_delivery(predecessor)
    }

    pub(super) fn delivery_is_current(
        &self,
        delivery: CleanupDelivery<Token, Provider, Binding>,
        provider: Provider,
    ) -> bool {
        self.observations
            .get(&delivery.predecessor)
            .is_some_and(|observation| {
                observation.token == delivery.observation.token
                    && observation.provider == provider
                    && observation.scheduled == Some(delivery.pending)
            })
            && self.pending.get(&delivery.predecessor) == Some(&delivery.pending)
    }

    pub(super) fn mark_inflight(
        &mut self,
        ordinal: u64,
        delivery: CleanupDelivery<Token, Provider, Binding>,
    ) -> Result<(), CleanupRendezvousError> {
        if self.inflight.insert(ordinal, delivery).is_some() {
            return Err(CleanupRendezvousError::DuplicateIngressOrdinal);
        }
        Ok(())
    }

    pub(super) fn take_inflight(
        &mut self,
        ordinal: u64,
    ) -> Option<CleanupDelivery<Token, Provider, Binding>> {
        self.inflight.remove(&ordinal)
    }

    pub(super) fn reject_delivery(&mut self, delivery: CleanupDelivery<Token, Provider, Binding>) {
        if let Some(observation) = self.observations.get_mut(&delivery.predecessor)
            && observation.token == delivery.observation.token
            && observation.scheduled == Some(delivery.pending)
        {
            observation.scheduled = None;
        }
    }

    pub(super) fn accept_delivery(&mut self, delivery: CleanupDelivery<Token, Provider, Binding>) {
        self.reject_delivery(delivery);
        if delivery.pending.is_indeterminate() {
            return;
        }
        self.pending.remove(&delivery.predecessor);
        if self
            .observations
            .get(&delivery.predecessor)
            .is_some_and(|observation| observation.token == delivery.observation.token)
        {
            self.observations.remove(&delivery.predecessor);
        }
        if let Some(native) = delivery.pending.native_viewport() {
            let settled = SettledCleanupResult {
                effect: delivery.predecessor,
                pending: delivery.pending,
            };
            match self.settled.get(&native).copied() {
                Some(existing) if existing.effect > settled.effect => {}
                Some(existing) if existing.effect == settled.effect => {
                    debug_assert!(existing == settled);
                }
                Some(_) | None => {
                    self.settled.insert(native, settled);
                }
            }
        }
    }

    pub(super) fn forget_native(&mut self, native: ExactNativeViewport) {
        self.observations
            .retain(|_, observation| observation.native != Some(native));
        self.pending
            .retain(|_, pending| pending.native_viewport() != Some(native));
        self.inflight
            .retain(|_, delivery| delivery.pending.native_viewport() != Some(native));
    }

    /// Releases duplicate-detection history only after exact ingress quiescence.
    pub(super) fn retire_native(&mut self, native: ExactNativeViewport) {
        self.settled.remove(&native);
    }

    /// Releases all buffered authority for one core binding after exact ingress quiescence.
    pub(super) fn retire_binding(&mut self, binding: Binding, native: ExactNativeViewport) {
        self.observations
            .retain(|_, observation| observation.binding != binding);
        self.pending.retain(|_, pending| pending.binding != binding);
        self.inflight
            .retain(|_, delivery| delivery.pending.binding != binding);
        self.retire_native(native);
    }

    #[cfg(test)]
    pub(super) fn retained_counts(&self) -> (usize, usize, usize, usize) {
        (
            self.observations.len(),
            self.pending.len(),
            self.settled.len(),
            self.inflight.len(),
        )
    }

    fn prepare_delivery(
        &mut self,
        predecessor: EffectId,
    ) -> Result<CleanupDisposition<Token, Provider, Binding>, CleanupRendezvousError> {
        let Some(pending) = self.pending.get(&predecessor).copied() else {
            return Ok(CleanupDisposition::Buffered);
        };
        let Some(observation) = self.observations.get_mut(&predecessor) else {
            return Ok(CleanupDisposition::Buffered);
        };
        if observation.binding != pending.binding
            || pending
                .native
                .is_some_and(|native| observation.native != Some(native))
        {
            return Err(CleanupRendezvousError::NativeMismatch);
        }
        if observation.scheduled == Some(pending) {
            return Ok(CleanupDisposition::Buffered);
        }
        observation.scheduled = Some(pending);
        Ok(CleanupDisposition::Deliver(CleanupDelivery {
            predecessor,
            observation: *observation,
            pending,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dockspace::effect::{
        DispatchFailureReason, EffectIndeterminateReason, EffectUnsupportedReason,
    };
    use egui::ViewportId;
    fn native(value: u64) -> ExactNativeViewport {
        ExactNativeViewport::new(
            ViewportId::from_hash_of(format!("cleanup-native-{value}")),
            NativeViewportIncarnation::new(value),
        )
    }

    fn result(
        native: ExactNativeViewport,
        dispatch: EffectDispatchResult,
    ) -> PendingCleanupResult<u8> {
        PendingCleanupResult::native(WorkspaceEpoch::new(1), dispatch, 1, native)
    }

    fn observation(
        token: u8,
        native: ExactNativeViewport,
        epoch: u64,
        provider: u8,
    ) -> CleanupObservation<u8, u8, u8> {
        CleanupObservation::new(
            token,
            1,
            native,
            WorkspaceEpoch::new(epoch),
            provider,
            u64::from(provider),
        )
    }

    #[test]
    fn result_before_observation_correlates_without_losing_the_raw_result() {
        let mut rendezvous = CleanupRendezvous::default();
        let predecessor = EffectId::new(1);
        let native = native(1);
        let pending = result(
            native,
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped),
        );
        assert_eq!(
            rendezvous
                .record_result(predecessor, pending, WorkspaceEpoch::new(2), 2)
                .expect("result buffering must succeed"),
            CleanupDisposition::Buffered
        );
        let CleanupDisposition::Deliver(delivery) = rendezvous
            .register_observation(predecessor, observation(7, native, 2, 2))
            .expect("successor observation must correlate")
        else {
            panic!("successor observation must schedule the pending result");
        };
        assert_eq!(delivery.pending(), pending);
        assert_eq!(rendezvous.retained_counts(), (1, 1, 0, 0));
    }

    #[test]
    fn provider_handoff_rejects_the_old_delivery_and_reschedules_the_successor() {
        let mut rendezvous = CleanupRendezvous::default();
        let predecessor = EffectId::new(1);
        let native = native(1);
        let pending = result(
            native,
            EffectDispatchResult::Unsupported(EffectUnsupportedReason::BackendUnsupported),
        );
        rendezvous
            .record_result(predecessor, pending, WorkspaceEpoch::new(2), 2)
            .expect("result buffering must succeed");
        let CleanupDisposition::Deliver(old) = rendezvous
            .register_observation(predecessor, observation(7, native, 2, 2))
            .expect("old observation must schedule")
        else {
            panic!("old observation must schedule the pending result");
        };
        assert!(!rendezvous.delivery_is_current(old, 3));
        rendezvous.reject_delivery(old);

        let CleanupDisposition::Deliver(successor) = rendezvous
            .register_observation(predecessor, observation(8, native, 2, 3))
            .expect("new provider must replace the old observer")
        else {
            panic!("new provider must reschedule the pending result");
        };
        assert!(rendezvous.delivery_is_current(successor, 3));
        rendezvous.accept_delivery(successor);
        assert_eq!(rendezvous.retained_counts(), (0, 0, 1, 0));
    }

    #[test]
    fn indeterminate_result_keeps_the_observer_until_terminal_refinement() {
        let mut rendezvous = CleanupRendezvous::default();
        let predecessor = EffectId::new(1);
        let native = native(1);
        rendezvous
            .register_observation(predecessor, observation(7, native, 2, 2))
            .expect("observation registration must succeed");
        let indeterminate = result(
            native,
            EffectDispatchResult::Indeterminate(EffectIndeterminateReason::AcknowledgementLost),
        );
        let CleanupDisposition::Deliver(delivery) = rendezvous
            .record_result(predecessor, indeterminate, WorkspaceEpoch::new(2), 2)
            .expect("indeterminate result must correlate")
        else {
            panic!("indeterminate result must schedule");
        };
        rendezvous.accept_delivery(delivery);
        assert_eq!(rendezvous.retained_counts(), (1, 1, 0, 0));

        let terminal = result(
            native,
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped),
        );
        let CleanupDisposition::Deliver(delivery) = rendezvous
            .record_result(predecessor, terminal, WorkspaceEpoch::new(2), 2)
            .expect("terminal refinement must correlate")
        else {
            panic!("terminal refinement must schedule");
        };
        rendezvous.accept_delivery(delivery);
        assert_eq!(rendezvous.retained_counts(), (0, 0, 1, 0));
    }

    #[test]
    fn mismatch_conflict_and_candidate_rollback_are_fail_closed() {
        let predecessor = EffectId::new(1);
        let first_native = native(1);
        let second_native = native(2);
        let pending = result(
            first_native,
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped),
        );
        let mut live = CleanupRendezvous::default();
        let mut candidate = live.clone();
        candidate
            .record_result(predecessor, pending, WorkspaceEpoch::new(2), 2)
            .expect("candidate result must buffer");
        assert_eq!(live.retained_counts(), (0, 0, 0, 0));
        assert_eq!(candidate.retained_counts(), (0, 1, 0, 0));

        assert_eq!(
            candidate.register_observation(predecessor, observation(7, second_native, 2, 2)),
            Err(CleanupRendezvousError::NativeMismatch)
        );
        assert_eq!(candidate.retained_counts(), (0, 1, 0, 0));
        assert_eq!(
            candidate.record_result(
                predecessor,
                result(
                    first_native,
                    EffectDispatchResult::Unsupported(EffectUnsupportedReason::BackendUnsupported,),
                ),
                WorkspaceEpoch::new(2),
                2,
            ),
            Err(CleanupRendezvousError::ConflictingResult)
        );
        live = candidate;
        assert_eq!(live.retained_counts(), (0, 1, 0, 0));
    }

    #[test]
    fn local_result_survives_handoff_without_retaining_a_native_tombstone() {
        let mut rendezvous = CleanupRendezvous::default();
        let predecessor = EffectId::new(1);
        let observation = CleanupObservation::local(7_u64, 1_u8, WorkspaceEpoch::new(2), 2_u8, 2);
        assert_eq!(
            rendezvous.register_observation(predecessor, observation),
            Err(CleanupRendezvousError::LocalResultUnavailable),
            "route-less authority exists only for an already-buffered local rejection",
        );
        let pending = PendingCleanupResult::local(
            WorkspaceEpoch::new(1),
            EffectDispatchResult::DispatchFailed(DispatchFailureReason::WindowUnavailable),
            1_u8,
        );
        assert_eq!(
            rendezvous
                .record_result(predecessor, pending, WorkspaceEpoch::new(2), 2_u8)
                .expect("local result must buffer across the provider handoff"),
            CleanupDisposition::Buffered
        );
        let CleanupDisposition::Deliver(delivery) = rendezvous
            .register_observation(predecessor, observation)
            .expect("the successor must correlate the local result")
        else {
            panic!("the successor must schedule the buffered local result");
        };
        rendezvous.accept_delivery(delivery);
        assert_eq!(rendezvous.retained_counts(), (0, 0, 0, 0));
    }

    #[test]
    fn settled_frontier_is_bounded_until_exact_native_quiescence() {
        let mut rendezvous: CleanupRendezvous<u64, u64, u8> = CleanupRendezvous::default();
        let native = native(1);
        for value in 1..=10_000 {
            let predecessor = EffectId::new(value);
            let pending = PendingCleanupResult::native(
                WorkspaceEpoch::new(1),
                EffectDispatchResult::DispatchFailed(DispatchFailureReason::ProviderStopped),
                1_u8,
                native,
            );
            assert_eq!(
                rendezvous
                    .record_result(predecessor, pending, WorkspaceEpoch::new(2), 2_u64)
                    .expect("each newer cleanup result must remain admissible"),
                CleanupDisposition::Buffered
            );
            let observation =
                CleanupObservation::new(value, 1_u8, native, WorkspaceEpoch::new(2), 2_u64, 2);
            let CleanupDisposition::Deliver(delivery) = rendezvous
                .register_observation(predecessor, observation)
                .expect("each newer observation must schedule")
            else {
                panic!("each newer observation must deliver the pending result");
            };
            rendezvous.accept_delivery(delivery);
            assert_eq!(rendezvous.retained_counts(), (0, 0, 1, 0));
        }

        rendezvous.forget_native(native);
        assert_eq!(rendezvous.retained_counts(), (0, 0, 1, 0));
        rendezvous.retire_native(native);
        assert_eq!(rendezvous.retained_counts(), (0, 0, 0, 0));
    }
}

//! Pointer pass-through ownership and causal recovery state.

use std::collections::{BTreeMap, BTreeSet};

use crate::effect::{EffectDispatchResult, EffectId, EffectPhase};
use crate::intent::PointerId;
use crate::platform::{
    InputEffectAcknowledgement, PlatformCapability, WindowInputObservation, WindowInputState,
};
use crate::viewport::{InputObservationGeneration, ViewportBinding};

/// Sole owner of pointer routes, holders, and pass-through recovery sagas.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct PointerPassthroughLifecycle {
    drag_sources: BTreeMap<PointerId, ViewportBinding>,
    sagas: BTreeMap<ViewportBinding, PointerPassthroughSaga>,
}

/// Immutable recovery state captured before a workspace or binding migration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PointerPassthroughRecovery(PointerPassthroughSaga);

/// One atomic pointer-route reassignment planned by the lifecycle owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PointerRouteChange {
    previous: Option<ViewportBinding>,
    current: ViewportBinding,
}

impl PointerRouteChange {
    pub(super) const fn previous(self) -> Option<ViewportBinding> {
        self.previous
    }

    pub(super) const fn current(self) -> ViewportBinding {
        self.current
    }
}

/// Exact effect attempts required to derive the next lifecycle directive.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct PointerPassthroughEffectAttempts {
    enable: Option<EffectId>,
    restore: Option<EffectId>,
}

impl PointerPassthroughEffectAttempts {
    pub(super) const fn enable(self) -> Option<EffectId> {
        self.enable
    }

    pub(super) const fn restore(self) -> Option<EffectId> {
        self.restore
    }
}

/// Affine request produced by the lifecycle owner and settled with one effect identifier.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct PointerPassthroughEffectRequest {
    binding: ViewportBinding,
    lane_tail: Option<EffectId>,
    kind: PointerPassthroughEffectRequestKind,
}

impl PointerPassthroughEffectRequest {
    pub(super) const fn binding(&self) -> ViewportBinding {
        self.binding
    }

    pub(super) const fn lane_tail(&self) -> Option<EffectId> {
        self.lane_tail
    }

    pub(super) const fn enabled(&self) -> bool {
        matches!(
            self.kind,
            PointerPassthroughEffectRequestKind::Enable { .. }
        )
    }
}

#[derive(Debug, PartialEq, Eq)]
enum PointerPassthroughEffectRequestKind {
    Enable {
        issued_after: InputObservationGeneration,
    },
    Restore {
        issued_after: Option<InputObservationGeneration>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PointerPassthroughLifecycleError {
    SagaMissing { binding: ViewportBinding },
    RestoreObligationMissing { binding: ViewportBinding },
}

/// Complete platform evidence used for one saga reduction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PointerPassthroughEvidence {
    observation: Option<WindowInputObservation>,
    generation_watermark: Option<InputObservationGeneration>,
    window_observed: bool,
    routeable: bool,
    capabilities: PointerPassthroughCapabilities,
}

impl PointerPassthroughEvidence {
    pub(super) const fn new(
        observation: Option<WindowInputObservation>,
        generation_watermark: Option<InputObservationGeneration>,
        window_observed: bool,
        routeable: bool,
        observation_capability: PlatformCapability,
        control_capability: PlatformCapability,
    ) -> Self {
        Self {
            observation,
            generation_watermark,
            window_observed,
            routeable,
            capabilities: PointerPassthroughCapabilities {
                observation: observation_capability,
                control: control_capability,
            },
        }
    }

    pub(super) const fn observation(self) -> Option<WindowInputObservation> {
        self.observation
    }

    fn authoritative_observation(self) -> Option<WindowInputObservation> {
        self.observation
            .filter(|observation| observation.known_state().is_some())
    }

    fn authoritative_state(self) -> Option<WindowInputState> {
        self.authoritative_observation()
            .and_then(WindowInputObservation::known_state)
    }

    fn retry_evidence(self) -> PointerPassthroughRetryEvidence {
        let (acknowledgement_known, acknowledged_effect) =
            self.observation.map_or((false, None), |observation| {
                match observation.acknowledged_effect() {
                    InputEffectAcknowledgement::Known(effect) => (true, effect),
                    InputEffectAcknowledgement::Unknown(_) => (false, None),
                }
            });
        PointerPassthroughRetryEvidence {
            input_state: self
                .observation
                .and_then(WindowInputObservation::known_state),
            acknowledgement_known,
            acknowledged_effect,
            window_observed: self.window_observed,
            routeable: self.routeable,
            capabilities: self.capabilities,
        }
    }
}

impl PointerPassthroughLifecycle {
    pub(super) fn extend_referenced_effects(&self, effects: &mut BTreeSet<EffectId>) {
        for saga in self.sagas.values() {
            effects.extend(saga.lane_tail);
            if let Some(enable) = saga.enable {
                effects.insert(enable.effect.effect);
            }
            if let Some(restore) = saga.restore.and_then(|obligation| obligation.attempt) {
                effects.insert(restore.effect.effect);
            }
        }
    }

    pub(super) fn contains_binding(&self, binding: ViewportBinding) -> bool {
        self.sagas.contains_key(&binding)
    }

    pub(super) fn drag_source(&self, pointer: PointerId) -> Option<ViewportBinding> {
        self.drag_sources.get(&pointer).copied()
    }

    pub(super) fn active_pointers(&self) -> Vec<PointerId> {
        self.drag_sources.keys().copied().collect()
    }

    pub(super) fn tracked_bindings(&self) -> Vec<ViewportBinding> {
        self.sagas.keys().copied().collect()
    }

    /// Atomically transfers one pointer to its sole source binding.
    pub(super) fn begin_routing(
        &mut self,
        pointer: PointerId,
        binding: ViewportBinding,
    ) -> Result<Option<PointerRouteChange>, PointerPassthroughLifecycleError> {
        let previous = self.drag_sources.get(&pointer).copied();
        if previous == Some(binding) {
            if !self
                .sagas
                .get(&binding)
                .is_some_and(|saga| saga.holders.contains(&pointer))
            {
                return Err(PointerPassthroughLifecycleError::SagaMissing { binding });
            }
            return Ok(None);
        }

        if let Some(previous) = previous {
            self.release_holder(pointer, previous)?;
        }
        self.sagas
            .entry(binding)
            .or_insert_with(|| PointerPassthroughSaga::new(pointer))
            .holders
            .insert(pointer);
        self.drag_sources.insert(pointer, binding);
        Ok(Some(PointerRouteChange {
            previous,
            current: binding,
        }))
    }

    pub(super) fn end_routing(
        &mut self,
        pointer: PointerId,
    ) -> Result<Option<ViewportBinding>, PointerPassthroughLifecycleError> {
        let Some(binding) = self.drag_sources.get(&pointer).copied() else {
            return Ok(None);
        };
        self.release_holder(pointer, binding)?;
        Ok(Some(binding))
    }

    fn release_holder(
        &mut self,
        pointer: PointerId,
        binding: ViewportBinding,
    ) -> Result<(), PointerPassthroughLifecycleError> {
        let saga = self
            .sagas
            .get_mut(&binding)
            .ok_or(PointerPassthroughLifecycleError::SagaMissing { binding })?;
        if !saga.holders.remove(&pointer) {
            return Err(PointerPassthroughLifecycleError::SagaMissing { binding });
        }
        self.drag_sources.remove(&pointer);
        Ok(())
    }

    /// Clears all active holders for a binding while retaining any recovery obligation.
    pub(super) fn prepare_binding_for_vacancy(&mut self, binding: ViewportBinding) {
        self.drag_sources.retain(|_, source| *source != binding);
        if let Some(saga) = self.sagas.get_mut(&binding) {
            saga.holders.clear();
        }
    }

    /// Forgets an exact terminal binding and every route that names it.
    pub(super) fn terminate_binding(&mut self, binding: ViewportBinding) {
        self.sagas.remove(&binding);
        self.drag_sources.retain(|_, source| *source != binding);
    }

    pub(super) fn clear(&mut self) {
        self.drag_sources.clear();
        self.sagas.clear();
    }

    pub(super) fn recover_for_provider_replacement(&mut self) {
        self.drag_sources.clear();
        self.sagas.retain(|_, saga| {
            if !saga.recovery_required() {
                return false;
            }
            *saga = PointerPassthroughSaga::provider_recovery(saga);
            true
        });
    }

    pub(super) fn recovery_snapshots(
        &self,
    ) -> BTreeMap<ViewportBinding, PointerPassthroughRecovery> {
        self.sagas
            .iter()
            .filter_map(|(binding, saga)| {
                saga.recovery_required()
                    .then_some((*binding, PointerPassthroughRecovery(saga.clone())))
            })
            .collect()
    }

    pub(super) fn install_epoch_recovery(
        &mut self,
        binding: ViewportBinding,
        previous: &PointerPassthroughRecovery,
    ) {
        self.sagas
            .insert(binding, PointerPassthroughSaga::epoch_recovery(&previous.0));
    }

    pub(super) fn install_retired_recovery(
        &mut self,
        binding: ViewportBinding,
        previous: &PointerPassthroughRecovery,
    ) {
        self.sagas.insert(binding, previous.0.retired_recovery());
    }

    pub(super) fn effect_attempts(
        &self,
        binding: ViewportBinding,
    ) -> PointerPassthroughEffectAttempts {
        self.sagas
            .get(&binding)
            .map_or(PointerPassthroughEffectAttempts::default(), |saga| {
                PointerPassthroughEffectAttempts {
                    enable: saga.enable.map(|attempt| attempt.effect.effect),
                    restore: saga
                        .restore
                        .and_then(|obligation| obligation.attempt)
                        .map(|attempt| attempt.effect.effect),
                }
            })
    }

    pub(super) fn reconcile(
        &mut self,
        binding: ViewportBinding,
        evidence: PointerPassthroughEvidence,
        enable_phase: Option<EffectPhase>,
        restore_phase: Option<EffectPhase>,
    ) -> Result<Option<PointerPassthroughEffectRequest>, PointerPassthroughLifecycleError> {
        let action = self
            .sagas
            .get_mut(&binding)
            .map_or(PointerPassthroughAction::None, |saga| {
                saga.action(evidence, enable_phase, restore_phase)
            });
        match action {
            PointerPassthroughAction::None => Ok(None),
            PointerPassthroughAction::Remove => {
                self.sagas.remove(&binding);
                Ok(None)
            }
            PointerPassthroughAction::RequestEnable => {
                let Some(observation) = evidence.authoritative_observation() else {
                    return Ok(None);
                };
                Ok(self.effect_request(
                    binding,
                    PointerPassthroughEffectRequestKind::Enable {
                        issued_after: observation.generation(),
                    },
                ))
            }
            PointerPassthroughAction::RequestRestore => {
                self.restore_request(binding, evidence).map(Some)
            }
        }
    }

    pub(super) fn restore_request(
        &self,
        binding: ViewportBinding,
        evidence: PointerPassthroughEvidence,
    ) -> Result<PointerPassthroughEffectRequest, PointerPassthroughLifecycleError> {
        let saga = self
            .sagas
            .get(&binding)
            .ok_or(PointerPassthroughLifecycleError::SagaMissing { binding })?;
        if saga.restore.is_none() {
            return Err(PointerPassthroughLifecycleError::RestoreObligationMissing { binding });
        }
        Ok(PointerPassthroughEffectRequest {
            binding,
            lane_tail: saga.lane_tail,
            kind: PointerPassthroughEffectRequestKind::Restore {
                issued_after: evidence.generation_watermark,
            },
        })
    }

    fn effect_request(
        &self,
        binding: ViewportBinding,
        kind: PointerPassthroughEffectRequestKind,
    ) -> Option<PointerPassthroughEffectRequest> {
        self.sagas
            .get(&binding)
            .map(|saga| PointerPassthroughEffectRequest {
                binding,
                lane_tail: saga.lane_tail,
                kind,
            })
    }

    pub(super) fn accept_effect_request(
        &mut self,
        request: PointerPassthroughEffectRequest,
        effect: EffectId,
    ) -> Result<(), PointerPassthroughLifecycleError> {
        let saga = self.sagas.get_mut(&request.binding).ok_or(
            PointerPassthroughLifecycleError::SagaMissing {
                binding: request.binding,
            },
        )?;
        match request.kind {
            PointerPassthroughEffectRequestKind::Enable { issued_after } => {
                saga.enable = Some(PointerInputEnableAttempt {
                    effect: PointerInputEffectAttempt::new(effect),
                    issued_after,
                });
            }
            PointerPassthroughEffectRequestKind::Restore { issued_after } => {
                let restore = saga.restore.as_mut().ok_or(
                    PointerPassthroughLifecycleError::RestoreObligationMissing {
                        binding: request.binding,
                    },
                )?;
                restore.attempt = Some(PointerInputRestoreAttempt {
                    effect: PointerInputEffectAttempt::new(effect),
                    issued_after,
                    terminal_reported_after: None,
                });
            }
        }
        saga.lane_tail = Some(effect);
        Ok(())
    }

    pub(super) fn record_dispatch_result(
        &mut self,
        binding: ViewportBinding,
        effect: EffectId,
        result: EffectDispatchResult,
        evidence: PointerPassthroughEvidence,
    ) -> bool {
        self.sagas
            .get_mut(&binding)
            .is_some_and(|saga| saga.record_dispatch_result(effect, result, evidence))
    }

    pub(super) fn causal_effect(
        &self,
        binding: ViewportBinding,
        observation: WindowInputObservation,
    ) -> Option<EffectId> {
        self.sagas
            .get(&binding)
            .and_then(|saga| saga.causal_effect(observation))
    }

    pub(super) fn accept_observed_effect(&mut self, binding: ViewportBinding, effect: EffectId) {
        if let Some(saga) = self.sagas.get_mut(&binding) {
            saga.accept_observed_effect(effect);
        }
    }

    #[cfg(test)]
    pub(super) fn restore_issued_after(
        &self,
        binding: ViewportBinding,
    ) -> Option<Option<InputObservationGeneration>> {
        self.sagas
            .get(&binding)
            .and_then(|saga| saga.restore)
            .and_then(|obligation| obligation.attempt)
            .map(|attempt| attempt.issued_after)
    }

    #[cfg(test)]
    pub(super) fn restore_terminal_reported_after(
        &self,
        binding: ViewportBinding,
    ) -> Option<Option<InputObservationGeneration>> {
        self.sagas
            .get(&binding)
            .and_then(|saga| saga.restore)
            .and_then(|obligation| obligation.attempt)
            .map(|attempt| attempt.terminal_reported_after)
    }

    #[cfg(test)]
    pub(super) fn restore_state_settled(&self, binding: ViewportBinding) -> Option<bool> {
        self.sagas
            .get(&binding)
            .and_then(|saga| saga.restore)
            .map(|restore| restore.state_settled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PointerPassthroughSaga {
    original: Option<PointerInputOriginal>,
    holders: BTreeSet<PointerId>,
    enable: Option<PointerInputEnableAttempt>,
    restore: Option<PointerInputRestoreObligation>,
    lane_tail: Option<EffectId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PointerInputOriginal {
    state: WindowInputState,
    generation: InputObservationGeneration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PointerInputEffectAttempt {
    effect: EffectId,
    retry: PointerPassthroughRetryFence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PointerInputEnableAttempt {
    effect: PointerInputEffectAttempt,
    issued_after: InputObservationGeneration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PointerInputRestoreAttempt {
    effect: PointerInputEffectAttempt,
    issued_after: Option<InputObservationGeneration>,
    terminal_reported_after: Option<InputObservationGeneration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PointerInputRestoreObligation {
    settlement: PointerInputRestoreSettlement,
    attempt: Option<PointerInputRestoreAttempt>,
    state_settled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PointerInputRestoreSettlement {
    StateOrExactEffect,
    ExactEffect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PointerPassthroughRetryFence {
    None,
    DispatchFailed {
        evidence: PointerPassthroughRetryEvidence,
        edge_seen: bool,
    },
    Unsupported {
        capabilities: PointerPassthroughCapabilities,
        edge_seen: bool,
    },
    Indeterminate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PointerPassthroughAction {
    None,
    Remove,
    RequestEnable,
    RequestRestore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PointerPassthroughCapabilities {
    observation: PlatformCapability,
    control: PlatformCapability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PointerPassthroughRetryEvidence {
    input_state: Option<WindowInputState>,
    acknowledgement_known: bool,
    acknowledged_effect: Option<EffectId>,
    window_observed: bool,
    routeable: bool,
    capabilities: PointerPassthroughCapabilities,
}

impl PointerPassthroughSaga {
    fn new(holder: PointerId) -> Self {
        Self {
            original: None,
            holders: BTreeSet::from([holder]),
            enable: None,
            restore: None,
            lane_tail: None,
        }
    }

    fn epoch_recovery(previous: &Self) -> Self {
        Self {
            original: previous.original,
            holders: BTreeSet::new(),
            enable: None,
            restore: Some(PointerInputRestoreObligation {
                settlement: PointerInputRestoreSettlement::ExactEffect,
                attempt: None,
                state_settled: false,
            }),
            lane_tail: previous.lane_tail,
        }
    }

    fn provider_recovery(previous: &Self) -> Self {
        Self {
            original: previous.original,
            holders: BTreeSet::new(),
            enable: None,
            restore: Some(PointerInputRestoreObligation {
                settlement: PointerInputRestoreSettlement::ExactEffect,
                attempt: None,
                state_settled: false,
            }),
            lane_tail: None,
        }
    }

    fn retired_recovery(&self) -> Self {
        let mut recovery = self.clone();
        recovery.holders.clear();
        recovery
    }

    fn recovery_required(&self) -> bool {
        self.restore.is_some()
            || (self
                .original
                .is_some_and(|original| original.state == WindowInputState::ReceivesInput)
                && self.enable.is_some())
    }

    fn causal_effect(&self, observation: WindowInputObservation) -> Option<EffectId> {
        if let Some(attempt) = self.restore.and_then(|restore| restore.attempt)
            && attempt
                .issued_after
                .is_none_or(|generation| observation.generation() > generation)
            && observation.known_state() == Some(WindowInputState::ReceivesInput)
            && observation.acknowledges(attempt.effect.effect)
        {
            return Some(attempt.effect.effect);
        }
        self.enable.and_then(|attempt| {
            (observation.generation() > attempt.issued_after
                && observation.known_state() == Some(WindowInputState::PassThrough)
                && observation.acknowledges(attempt.effect.effect))
            .then_some(attempt.effect.effect)
        })
    }

    fn accept_observed_effect(&mut self, effect: EffectId) {
        if self
            .restore
            .and_then(|restore| restore.attempt)
            .is_some_and(|attempt| attempt.effect.effect == effect)
        {
            self.restore = None;
            self.enable = None;
        }
    }

    fn record_dispatch_result(
        &mut self,
        effect: EffectId,
        result: EffectDispatchResult,
        evidence: PointerPassthroughEvidence,
    ) -> bool {
        if let Some(enable) = &mut self.enable
            && enable.effect.effect == effect
        {
            enable.effect.record_dispatch_result(result, evidence);
            return true;
        }
        if let Some(restore) = &mut self.restore
            && let Some(attempt) = &mut restore.attempt
            && attempt.effect.effect == effect
        {
            if matches!(
                result,
                EffectDispatchResult::DispatchFailed(_) | EffectDispatchResult::Unsupported(_)
            ) {
                attempt.terminal_reported_after =
                    attempt.issued_after.max(evidence.generation_watermark);
            }
            attempt.effect.record_dispatch_result(result, evidence);
            return true;
        }
        false
    }

    fn action(
        &mut self,
        evidence: PointerPassthroughEvidence,
        enable_phase: Option<EffectPhase>,
        restore_phase: Option<EffectPhase>,
    ) -> PointerPassthroughAction {
        self.capture_original_state(evidence);
        self.observe_retry_edges(evidence);
        self.settle_restore_from_state(evidence, enable_phase, restore_phase);

        if self.restore.is_some_and(|restore| restore.state_settled) {
            if self.holders.is_empty()
                && !matches!(
                    restore_phase,
                    Some(
                        EffectPhase::DispatchFailed(_)
                            | EffectPhase::ObservedApplied { .. }
                            | EffectPhase::Unsupported(_)
                            | EffectPhase::Destroyed { .. }
                            | EffectPhase::Invalidated { .. }
                    )
                )
            {
                return PointerPassthroughAction::None;
            }
            self.restore = None;
        }

        if let Some(restore) = self.restore {
            let request_restore = match restore.attempt {
                None => true,
                Some(attempt) => {
                    evidence.capabilities.control.is_supported()
                        && (matches!(restore_phase, Some(EffectPhase::Invalidated { .. }))
                            || restore_phase.is_some_and(|phase| attempt.effect.can_retry(phase)))
                }
            };
            return if request_restore {
                PointerPassthroughAction::RequestRestore
            } else {
                PointerPassthroughAction::None
            };
        }

        if self.holders.is_empty() {
            if self
                .original
                .is_some_and(|original| original.state == WindowInputState::ReceivesInput)
                && self.enable.is_some()
            {
                self.restore = Some(PointerInputRestoreObligation {
                    settlement: PointerInputRestoreSettlement::StateOrExactEffect,
                    attempt: None,
                    state_settled: false,
                });
                return PointerPassthroughAction::RequestRestore;
            }
            return PointerPassthroughAction::Remove;
        }

        let Some(original) = self.original else {
            return PointerPassthroughAction::None;
        };
        let needs_enable = self.enable.map_or_else(
            || {
                original.state == WindowInputState::ReceivesInput
                    || evidence.authoritative_state() != Some(WindowInputState::PassThrough)
            },
            |attempt| {
                let state_proves_passthrough =
                    evidence
                        .authoritative_observation()
                        .is_some_and(|observation| {
                            observation.generation() > attempt.issued_after
                                && observation.known_state() == Some(WindowInputState::PassThrough)
                        });
                !state_proves_passthrough
                    && (matches!(enable_phase, Some(EffectPhase::Invalidated { .. }))
                        || enable_phase.is_some_and(|phase| attempt.effect.can_retry(phase)))
            },
        );
        if needs_enable
            && evidence.authoritative_observation().is_some()
            && evidence.routeable
            && evidence.capabilities.observation.is_supported()
            && evidence.capabilities.control.is_supported()
        {
            PointerPassthroughAction::RequestEnable
        } else {
            PointerPassthroughAction::None
        }
    }

    fn capture_original_state(&mut self, evidence: PointerPassthroughEvidence) {
        if self.original.is_none() {
            self.original = evidence
                .authoritative_observation()
                .and_then(|observation| {
                    observation.known_state().map(|state| PointerInputOriginal {
                        state,
                        generation: observation.generation(),
                    })
                });
        }
    }

    fn observe_retry_edges(&mut self, evidence: PointerPassthroughEvidence) {
        if let Some(enable) = &mut self.enable {
            enable.effect.observe_retry_edge(evidence);
        }
        if let Some(restore) = &mut self.restore
            && let Some(attempt) = &mut restore.attempt
        {
            attempt.effect.observe_retry_edge(evidence);
        }
    }

    fn settle_restore_from_state(
        &mut self,
        evidence: PointerPassthroughEvidence,
        enable_phase: Option<EffectPhase>,
        restore_phase: Option<EffectPhase>,
    ) {
        let enable_cannot_apply_later = matches!(
            enable_phase,
            Some(
                EffectPhase::DispatchFailed(_)
                    | EffectPhase::ObservedApplied { .. }
                    | EffectPhase::Unsupported(_)
                    | EffectPhase::Destroyed { .. }
            )
        );
        let restore_reached_terminal_lane_position = matches!(
            restore_phase,
            Some(EffectPhase::DispatchFailed(_) | EffectPhase::Unsupported(_))
        );
        let state_can_settle_restore = self.restore.is_some_and(|restore| {
            restore.settlement == PointerInputRestoreSettlement::StateOrExactEffect
                && !restore.state_settled
                && restore.attempt.is_some_and(|attempt| {
                    evidence
                        .authoritative_observation()
                        .is_some_and(|observation| {
                            let barrier = if restore_reached_terminal_lane_position {
                                attempt.terminal_reported_after
                            } else if enable_cannot_apply_later {
                                attempt.issued_after
                            } else {
                                return false;
                            };
                            barrier.is_none_or(|generation| observation.generation() > generation)
                                && observation.known_state()
                                    == Some(WindowInputState::ReceivesInput)
                        })
                })
        });
        if state_can_settle_restore {
            if let Some(restore) = &mut self.restore {
                restore.state_settled = true;
            }
            self.enable = None;
        }
    }
}

impl PointerInputEffectAttempt {
    const fn new(effect: EffectId) -> Self {
        Self {
            effect,
            retry: PointerPassthroughRetryFence::None,
        }
    }

    fn record_dispatch_result(
        &mut self,
        result: EffectDispatchResult,
        evidence: PointerPassthroughEvidence,
    ) {
        self.retry = match result {
            EffectDispatchResult::DispatchFailed(_) => {
                PointerPassthroughRetryFence::DispatchFailed {
                    evidence: evidence.retry_evidence(),
                    edge_seen: false,
                }
            }
            EffectDispatchResult::Unsupported(_) => PointerPassthroughRetryFence::Unsupported {
                capabilities: evidence.capabilities,
                edge_seen: false,
            },
            EffectDispatchResult::Indeterminate(_) => PointerPassthroughRetryFence::Indeterminate,
        };
    }

    fn observe_retry_edge(&mut self, evidence: PointerPassthroughEvidence) {
        match &mut self.retry {
            PointerPassthroughRetryFence::DispatchFailed {
                evidence: blocked,
                edge_seen,
            } => *edge_seen |= *blocked != evidence.retry_evidence(),
            PointerPassthroughRetryFence::Unsupported {
                capabilities,
                edge_seen,
            } => *edge_seen |= *capabilities != evidence.capabilities,
            PointerPassthroughRetryFence::None | PointerPassthroughRetryFence::Indeterminate => {}
        }
    }

    const fn can_retry(self, phase: EffectPhase) -> bool {
        matches!(
            (phase, self.retry),
            (
                EffectPhase::DispatchFailed(_),
                PointerPassthroughRetryFence::DispatchFailed {
                    edge_seen: true,
                    ..
                }
            ) | (
                EffectPhase::Unsupported(_),
                PointerPassthroughRetryFence::Unsupported {
                    edge_seen: true,
                    ..
                }
            )
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{EngineAuthorityDomainId, SurfaceId, WorkspaceEpoch};
    use crate::viewport::{WindowIncarnation, WindowToken};

    fn binding(surface: u64, token: u64, incarnation: u64) -> ViewportBinding {
        ViewportBinding::new(
            EngineAuthorityDomainId::new_for_test(1),
            WorkspaceEpoch::new(0),
            SurfaceId::new(surface),
            WindowToken::new(token),
            WindowIncarnation::new(incarnation),
        )
    }

    #[test]
    fn one_pointer_has_exactly_one_binding_and_holder() {
        let pointer = PointerId::new(7);
        let first = binding(1, 11, 1);
        let successor = binding(2, 12, 1);
        let mut lifecycle = PointerPassthroughLifecycle::default();

        lifecycle
            .begin_routing(pointer, first)
            .expect("first route must be accepted");
        let change = lifecycle
            .begin_routing(pointer, successor)
            .expect("route reassignment must be accepted")
            .expect("a different binding must produce a route change");

        assert_eq!(change.previous(), Some(first));
        assert_eq!(change.current(), successor);
        assert_eq!(lifecycle.drag_source(pointer), Some(successor));
        assert!(!lifecycle.sagas[&first].holders.contains(&pointer));
        assert!(lifecycle.sagas[&successor].holders.contains(&pointer));
        assert_eq!(
            lifecycle
                .sagas
                .values()
                .filter(|saga| saga.holders.contains(&pointer))
                .count(),
            1
        );
    }
}

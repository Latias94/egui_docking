//! Exact child-window retirement state owned by the native adapter.
//!
//! Core owns lifecycle meaning. This module only retains the affine close
//! acknowledgement and the exact eframe route until the operating system
//! reports destruction and the corresponding core frame commits.

use std::collections::BTreeMap;

use dockspace::runtime::{
    NativeCleanupObservation, NativeCloseEffectAcknowledgement, NativeEffectResult,
    NativeSurfaceBinding, NativeWindowFacts,
};
use eframe::egui::ViewportId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetirementPhase {
    AwaitingDestroyed,
    DestroyedObserved,
    SnapshotQueued,
    TombstoneCommitted,
    RouteRetired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DestroyedObservation {
    Rejected,
    Duplicate,
    Recorded,
}

#[derive(Debug, Default)]
struct CleanupRelay {
    observation: Option<NativeCleanupObservation>,
    result: Option<NativeEffectResult>,
}

#[derive(Debug)]
pub(crate) enum CleanupResultRetentionError {
    Occupied(NativeEffectResult),
    CorrelationMismatch,
}

impl CleanupRelay {
    fn can_accept_observation(&self) -> bool {
        self.observation.is_none()
    }

    fn accept_observation(
        &mut self,
        observation: NativeCleanupObservation,
    ) -> Result<Option<NativeEffectResult>, ()> {
        debug_assert!(self.observation.is_none());
        let Some(result) = self.result.take() else {
            self.observation = Some(observation);
            return Ok(None);
        };
        match observation.correlate(result) {
            Ok(result) => Ok(Some(result)),
            Err(error) => {
                let (observation, result) = error.into_parts();
                self.observation = Some(observation);
                self.result = Some(result);
                Err(())
            }
        }
    }

    fn retain_result(
        &mut self,
        result: NativeEffectResult,
    ) -> Result<Option<NativeEffectResult>, CleanupResultRetentionError> {
        if self.result.is_some() {
            return Err(CleanupResultRetentionError::Occupied(result));
        }
        let Some(observation) = self.observation.take() else {
            self.result = Some(result);
            return Ok(None);
        };
        match observation.correlate(result) {
            Ok(result) => Ok(Some(result)),
            Err(error) => {
                let (observation, result) = error.into_parts();
                self.observation = Some(observation);
                self.result = Some(result);
                Err(CleanupResultRetentionError::CorrelationMismatch)
            }
        }
    }

    fn is_pending(&self) -> bool {
        self.observation.is_some() || self.result.is_some()
    }

    /// Drops cleanup capabilities once the exact Destroyed tombstone committed.
    ///
    /// Destroyed is the terminal core fact for the predecessor operation; no
    /// later adapter result may resurrect or re-correlate that cleanup lane.
    fn finish_destroyed_snapshot(&mut self) {
        self.observation = None;
        self.result = None;
    }
}

#[derive(Debug)]
struct PendingRetirement {
    viewport: ViewportId,
    acknowledgement: Option<NativeCloseEffectAcknowledgement>,
    phase: RetirementPhase,
    cleanup: CleanupRelay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CommittedRetirement {
    viewport: ViewportId,
    binding: NativeSurfaceBinding,
}

impl CommittedRetirement {
    pub(crate) const fn viewport(self) -> ViewportId {
        self.viewport
    }

    pub(crate) const fn binding(self) -> NativeSurfaceBinding {
        self.binding
    }
}

#[derive(Debug, Default)]
pub(crate) struct NativeRetirementState {
    pending: BTreeMap<NativeSurfaceBinding, PendingRetirement>,
}

impl NativeRetirementState {
    pub(crate) fn has_pending_work(&self) -> bool {
        !self.pending.is_empty()
    }

    pub(crate) fn requires_snapshot(&self) -> bool {
        self.pending
            .values()
            .any(|pending| matches!(pending.phase, RetirementPhase::DestroyedObserved))
    }

    pub(crate) fn can_begin_release(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.pending_for_lifetime(binding).is_none()
            && !self
                .pending
                .values()
                .any(|pending| pending.viewport == viewport)
    }

    pub(crate) fn begin_release(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        acknowledgement: NativeCloseEffectAcknowledgement,
    ) -> bool {
        if !self.can_begin_release(viewport, binding) {
            return false;
        }
        self.pending.insert(
            binding,
            PendingRetirement {
                viewport,
                acknowledgement: Some(acknowledgement),
                phase: RetirementPhase::AwaitingDestroyed,
                cleanup: CleanupRelay::default(),
            },
        );
        true
    }

    pub(crate) fn observe_destroyed(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> DestroyedObservation {
        if let Some(existing) = self.pending_lifetime_key(binding) {
            if existing != binding && self.pending.contains_key(&binding) {
                return DestroyedObservation::Rejected;
            }
            // Validate the callback route before moving the exact binding key.
            // A rejected successor observation must not clear the predecessor's
            // close acknowledgement or otherwise mutate the retirement lane.
            if !self
                .pending
                .get(&existing)
                .is_some_and(|pending| pending.viewport == viewport)
            {
                return DestroyedObservation::Rejected;
            }
            if existing != binding {
                if !self.adopt_successor_binding(existing, binding) {
                    return DestroyedObservation::Rejected;
                }
            }
        }
        if let Some(pending) = self.pending.get_mut(&binding) {
            if pending.viewport != viewport {
                return DestroyedObservation::Rejected;
            }
            if pending.phase == RetirementPhase::AwaitingDestroyed {
                pending.phase = RetirementPhase::DestroyedObserved;
                return DestroyedObservation::Recorded;
            }
            return DestroyedObservation::Duplicate;
        }
        if self
            .pending
            .values()
            .any(|pending| pending.viewport == viewport)
        {
            return DestroyedObservation::Rejected;
        }
        self.pending.insert(
            binding,
            PendingRetirement {
                viewport,
                acknowledgement: None,
                phase: RetirementPhase::DestroyedObserved,
                cleanup: CleanupRelay::default(),
            },
        );
        DestroyedObservation::Recorded
    }

    fn pending_for_lifetime(&self, binding: NativeSurfaceBinding) -> Option<&PendingRetirement> {
        self.pending_lifetime_key(binding)
            .and_then(|key| self.pending.get(&key))
    }

    fn pending_for_lifetime_mut(
        &mut self,
        binding: NativeSurfaceBinding,
    ) -> Option<&mut PendingRetirement> {
        let key = self.pending_lifetime_key(binding)?;
        self.pending.get_mut(&key)
    }

    fn pending_lifetime_key(&self, binding: NativeSurfaceBinding) -> Option<NativeSurfaceBinding> {
        self.pending
            .keys()
            .copied()
            .find(|candidate| candidate.same_window_lifetime(binding))
    }

    fn adopt_successor_binding(
        &mut self,
        predecessor: NativeSurfaceBinding,
        successor: NativeSurfaceBinding,
    ) -> bool {
        if predecessor == successor {
            return true;
        }
        if self.pending.contains_key(&successor) {
            return false;
        }
        let Some(mut pending) = self.pending.remove(&predecessor) else {
            return false;
        };
        // A provider handoff invalidates the old close acknowledgement. The
        // successor may only publish a plain Destroyed tombstone until it
        // obtains a new exact acknowledgement from core.
        pending.acknowledgement = None;
        self.pending.insert(successor, pending);
        true
    }

    fn adopt_successor_for_lifetime(&mut self, binding: NativeSurfaceBinding) -> bool {
        let Some(predecessor) = self.pending_lifetime_key(binding) else {
            return false;
        };
        self.adopt_successor_binding(predecessor, binding)
    }

    pub(crate) fn can_accept_cleanup_observation(&self, binding: NativeSurfaceBinding) -> bool {
        self.pending_for_lifetime(binding).is_some_and(|pending| {
            matches!(
                pending.phase,
                RetirementPhase::AwaitingDestroyed
                    | RetirementPhase::DestroyedObserved
                    | RetirementPhase::SnapshotQueued
            ) && pending.cleanup.can_accept_observation()
        })
    }

    pub(crate) fn accept_cleanup_observation(
        &mut self,
        binding: NativeSurfaceBinding,
        observation: NativeCleanupObservation,
    ) -> Result<Option<NativeEffectResult>, ()> {
        if !self.adopt_successor_for_lifetime(binding) {
            return Err(());
        }
        let Some(pending) = self.pending_for_lifetime_mut(binding) else {
            return Err(());
        };
        pending.cleanup.accept_observation(observation)
    }

    pub(crate) fn can_accept_cleanup_result(&self, binding: NativeSurfaceBinding) -> bool {
        self.pending_for_lifetime(binding).is_some_and(|pending| {
            !matches!(
                pending.phase,
                RetirementPhase::TombstoneCommitted | RetirementPhase::RouteRetired
            ) && pending.cleanup.result.is_none()
        })
    }

    pub(crate) fn cleanup_is_terminal(&self, binding: NativeSurfaceBinding) -> bool {
        self.pending_for_lifetime(binding).is_some_and(|pending| {
            matches!(
                pending.phase,
                RetirementPhase::TombstoneCommitted | RetirementPhase::RouteRetired
            )
        })
    }

    pub(crate) fn retain_cleanup_result(
        &mut self,
        binding: NativeSurfaceBinding,
        result: NativeEffectResult,
    ) -> Result<Option<NativeEffectResult>, CleanupResultRetentionError> {
        if !self.adopt_successor_for_lifetime(binding) {
            return Err(CleanupResultRetentionError::Occupied(result));
        }
        let Some(pending) = self.pending_for_lifetime_mut(binding) else {
            return Err(CleanupResultRetentionError::Occupied(result));
        };
        if matches!(
            pending.phase,
            RetirementPhase::TombstoneCommitted | RetirementPhase::RouteRetired
        ) {
            return Err(CleanupResultRetentionError::Occupied(result));
        }
        pending.cleanup.retain_result(result)
    }

    pub(crate) fn references_cleanup(&self, binding: NativeSurfaceBinding) -> bool {
        self.pending_for_lifetime(binding)
            .is_some_and(|pending| pending.cleanup.is_pending())
    }

    pub(crate) fn destroyed_observations(
        &self,
    ) -> impl Iterator<Item = (NativeSurfaceBinding, NativeWindowFacts)> + '_ {
        self.pending.iter().filter_map(|(binding, pending)| {
            (pending.phase == RetirementPhase::DestroyedObserved).then(|| {
                let facts = pending.acknowledgement.map_or_else(
                    NativeWindowFacts::destroyed,
                    NativeWindowFacts::destroyed_after,
                );
                (*binding, facts)
            })
        })
    }

    pub(crate) fn mark_snapshot_queued(&mut self) {
        for pending in self.pending.values_mut() {
            if pending.phase == RetirementPhase::DestroyedObserved {
                pending.phase = RetirementPhase::SnapshotQueued;
            }
        }
    }

    pub(crate) fn settle_snapshot(&mut self, applied: bool) {
        for pending in self.pending.values_mut() {
            if pending.phase == RetirementPhase::SnapshotQueued {
                pending.phase = if applied {
                    pending.cleanup.finish_destroyed_snapshot();
                    RetirementPhase::TombstoneCommitted
                } else {
                    RetirementPhase::DestroyedObserved
                };
            }
        }
    }

    pub(crate) fn committed_routes(&self) -> Vec<CommittedRetirement> {
        self.pending
            .iter()
            .filter(|(_, pending)| pending.phase == RetirementPhase::TombstoneCommitted)
            .map(|(binding, pending)| CommittedRetirement {
                viewport: pending.viewport,
                binding: *binding,
            })
            .collect()
    }

    pub(crate) fn can_retire_committed_route_for_replacement(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.pending.get(&binding).is_some_and(|pending| {
            pending.viewport == viewport && pending.phase == RetirementPhase::TombstoneCommitted
        })
    }

    pub(crate) fn retire_committed_route_for_replacement(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        let Some(pending) = self.pending.get_mut(&binding) else {
            return false;
        };
        if pending.viewport != viewport || pending.phase != RetirementPhase::TombstoneCommitted {
            return false;
        }
        pending.phase = RetirementPhase::RouteRetired;
        true
    }

    pub(crate) fn commit_routes(&mut self, committed: &[CommittedRetirement]) {
        for committed in committed {
            let pending = self
                .pending
                .get_mut(&committed.binding)
                .expect("preflighted retirement remains pending");
            debug_assert_eq!(pending.viewport, committed.viewport);
            debug_assert_eq!(pending.phase, RetirementPhase::TombstoneCommitted);
            pending.phase = RetirementPhase::RouteRetired;
        }
    }

    pub(crate) fn quiescence_candidates(&self) -> impl Iterator<Item = NativeSurfaceBinding> + '_ {
        self.pending.iter().filter_map(|(binding, pending)| {
            (pending.phase == RetirementPhase::RouteRetired).then_some(*binding)
        })
    }

    pub(crate) fn finish_quiescence(&mut self, binding: NativeSurfaceBinding) -> bool {
        if !self
            .pending
            .get(&binding)
            .is_some_and(|pending| pending.phase == RetirementPhase::RouteRetired)
        {
            return false;
        }
        self.pending.remove(&binding);
        true
    }
}

#[cfg(test)]
mod tests {
    use dockspace::model::{
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
        RootId, SurfaceId,
    };
    use dockspace::policy::DockPolicy;
    use dockspace::runtime::{
        DockspaceSession, HostInputOutcome, HostWindowToken, NativePointerRoster,
        SurfaceUnavailableReason,
    };

    use super::*;

    fn binding() -> NativeSurfaceBinding {
        let surface = SurfaceId::new(1);
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            surface,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1)]),
            ),
        )])
        .expect("retirement test layout validates");
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default())
            .expect("retirement test session initializes");
        session
            .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
            .expect("managed native host enrolls");
        session
            .register_native_root(surface, HostWindowToken::new(1))
            .expect("native root registration queues");
        let mut frame = session
            .begin_host_frame()
            .expect("registration frame begins");
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("registration frame settles");
        frame
            .commit()
            .expect("registration frame commits")
            .inputs()
            .iter()
            .find_map(|outcome| match outcome {
                HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
                _ => None,
            })
            .expect("registration emits one binding")
    }

    #[test]
    fn uncorrelated_destruction_retires_only_after_tombstone_commit() {
        let binding = binding();
        let viewport = ViewportId::from_hash_of("retired-child");
        let mut state = NativeRetirementState::default();
        assert_eq!(
            state.observe_destroyed(viewport, binding),
            DestroyedObservation::Recorded
        );
        assert_eq!(
            state.observe_destroyed(viewport, binding),
            DestroyedObservation::Duplicate
        );
        assert!(state.requires_snapshot());
        assert_eq!(state.destroyed_observations().count(), 1);
        assert!(state.committed_routes().is_empty());

        state.mark_snapshot_queued();
        assert!(state.committed_routes().is_empty());
        state.settle_snapshot(false);
        assert!(state.requires_snapshot());
        assert_eq!(state.destroyed_observations().count(), 1);

        state.mark_snapshot_queued();
        state.settle_snapshot(true);
        assert!(!state.requires_snapshot());
        let committed = state.committed_routes();
        assert_eq!(committed.len(), 1);
        state.commit_routes(&committed);
        assert_eq!(state.quiescence_candidates().collect::<Vec<_>>(), [binding]);
        assert!(state.finish_quiescence(binding));
        assert!(state.quiescence_candidates().next().is_none());
    }
}

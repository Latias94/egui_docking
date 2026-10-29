//! Exact child-window retirement state owned by the native adapter.
//!
//! Core owns lifecycle meaning. This module only retains the affine close
//! acknowledgement and the exact eframe route until the operating system
//! reports destruction and the corresponding core frame commits.

use std::collections::BTreeMap;
use std::mem::size_of;

use crate::mailbox::journal::ReservedHostRecord;
use crate::viewport_map::{NativeCreateReceipt, NativeWindowReceipt};
use dockspace::runtime::{
    NativeCleanupObservation, NativeCloseEffectAcknowledgement, NativeEffectResult,
    NativeSurfaceBinding, NativeWindowFacts,
};
use eframe::egui::ViewportId;
use winit::window::WindowId;

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
    window: NativeWindowReceipt,
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
    unmaterialized: BTreeMap<NativeSurfaceBinding, NativeCreateReceipt>,
}

impl NativeRetirementState {
    /// Counts inline storage and retained entry data, excluding B-tree node overhead.
    /// Receipts, cleanup capabilities, and supported ingress payloads are inline.
    pub(crate) fn owned_data_capacity_bytes(&self) -> usize {
        size_of::<Self>()
            .saturating_add(
                self.pending.len().saturating_mul(
                    size_of::<NativeSurfaceBinding>() + size_of::<PendingRetirement>(),
                ),
            )
            .saturating_add(self.unmaterialized.len().saturating_mul(
                size_of::<NativeSurfaceBinding>() + size_of::<NativeCreateReceipt>(),
            ))
    }

    pub(crate) fn bindings(&self) -> impl Iterator<Item = NativeSurfaceBinding> + '_ {
        self.pending
            .keys()
            .chain(self.unmaterialized.keys())
            .copied()
    }

    pub(crate) fn has_pending_work(&self) -> bool {
        !self.pending.is_empty() || !self.unmaterialized.is_empty()
    }

    /// Retains a native lifetime which definitively failed before an OS window
    /// existed. No Destroyed observation is possible for this path, so exact
    /// adapter quiescence is the only remaining terminal boundary.
    pub(crate) fn begin_unmaterialized_quiescence(
        &mut self,
        binding: NativeSurfaceBinding,
        receipt: NativeCreateReceipt,
    ) -> bool {
        if self.pending_for_lifetime(binding).is_some()
            || self
                .unmaterialized
                .keys()
                .any(|candidate| candidate.same_window_lifetime(binding))
        {
            return false;
        }
        self.unmaterialized.insert(binding, receipt).is_none()
    }

    pub(crate) fn has_quiescence_owner(&self, binding: NativeSurfaceBinding) -> bool {
        self.pending_for_lifetime(binding).is_some()
            || self
                .unmaterialized
                .keys()
                .any(|candidate| candidate.same_window_lifetime(binding))
    }

    pub(crate) fn is_quiescence_candidate(&self, binding: NativeSurfaceBinding) -> bool {
        self.unmaterialized
            .get(&binding)
            .is_some_and(NativeCreateReceipt::is_committed)
            || self.pending.get(&binding).is_some_and(|pending| {
                pending.phase == RetirementPhase::RouteRetired
                    && pending.window.destroyed_committed()
            })
    }

    pub(crate) fn mark_create_failure_recorded(&mut self, binding: NativeSurfaceBinding) {
        if let Some(receipt) = self.unmaterialized.get_mut(&binding) {
            receipt.mark_failure_recorded();
        }
    }

    pub(crate) fn commit_create_failures(&mut self) {
        for receipt in self.unmaterialized.values_mut() {
            receipt.commit_failure();
        }
    }

    pub(crate) fn create_ingress(&self) -> impl Iterator<Item = &ReservedHostRecord> {
        self.unmaterialized
            .values()
            .filter_map(NativeCreateReceipt::ingress)
    }

    pub(crate) fn take_create_ingress(
        &mut self,
        capture_ordinal: u64,
    ) -> Option<ReservedHostRecord> {
        self.unmaterialized
            .values_mut()
            .find_map(|receipt| receipt.take_ingress(capture_ordinal))
    }

    pub(crate) fn requires_snapshot(&self) -> bool {
        self.pending.values().any(|pending| {
            pending.phase == RetirementPhase::DestroyedObserved
                && pending.window.destroyed_committed()
        })
    }

    pub(crate) fn can_begin_release(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        !self.unmaterialized.contains_key(&binding)
            && self.pending_for_lifetime(binding).is_none_or(|pending| {
                pending.viewport == viewport
                    && pending.phase == RetirementPhase::AwaitingDestroyed
                    && pending.acknowledgement.is_none()
            })
    }

    pub(crate) fn begin_release(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        acknowledgement: NativeCloseEffectAcknowledgement,
        window: Option<NativeWindowReceipt>,
    ) -> bool {
        if !self.can_begin_release(viewport, binding) {
            return false;
        }
        if let Some(pending) = self.pending.get_mut(&binding) {
            if pending.viewport != viewport || window.is_some() {
                return false;
            }
            pending.acknowledgement = Some(acknowledgement);
            return true;
        }
        let Some(window) = window else {
            return false;
        };
        debug_assert_eq!(window.viewport(), viewport);
        debug_assert_eq!(window.binding(), binding);
        self.pending.insert(
            binding,
            PendingRetirement {
                viewport,
                window,
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
        window: WindowId,
        ordinal: u64,
    ) -> DestroyedObservation {
        if let Some(pending) = self.pending.get_mut(&binding) {
            if pending.viewport != viewport
                || pending.window.window() != window
                || !pending.window.received_destroyed(ordinal)
            {
                return DestroyedObservation::Rejected;
            }
            if pending.phase == RetirementPhase::AwaitingDestroyed {
                pending.phase = RetirementPhase::DestroyedObserved;
                return DestroyedObservation::Recorded;
            }
            return DestroyedObservation::Duplicate;
        }
        DestroyedObservation::Rejected
    }

    /// Moves the attached route's sole receipt into the existing retirement owner.
    pub(crate) fn can_retain_window(
        &self,
        binding: NativeSurfaceBinding,
        window: WindowId,
    ) -> bool {
        !self.has_quiescence_owner(binding) && !self.owns_window(window, None)
    }

    pub(crate) fn retain_window(
        &mut self,
        window: NativeWindowReceipt,
    ) -> Result<(), NativeWindowReceipt> {
        let binding = window.binding();
        if !self.can_retain_window(binding, window.window()) {
            return Err(window);
        }
        self.pending.insert(
            binding,
            PendingRetirement {
                viewport: window.viewport(),
                window,
                acknowledgement: None,
                phase: RetirementPhase::AwaitingDestroyed,
                cleanup: CleanupRelay::default(),
            },
        );
        Ok(())
    }

    pub(crate) fn receive_destroyed(
        &mut self,
        window: WindowId,
        viewport: Option<ViewportId>,
        ordinal: u64,
        reserve: impl FnOnce(ViewportId, NativeSurfaceBinding) -> ReservedHostRecord,
    ) -> Option<(ViewportId, NativeSurfaceBinding)> {
        let (binding, pending) = self
            .pending
            .iter_mut()
            .find(|(_, pending)| pending.window.matches_window(window, viewport))?;
        let owner = (pending.viewport, *binding);
        pending
            .window
            .receive_destroyed(ordinal, || reserve(owner.0, owner.1))
            .then_some(owner)
    }

    pub(crate) fn window_ingress(&self) -> impl Iterator<Item = &ReservedHostRecord> {
        self.pending
            .values()
            .filter_map(|pending| pending.window.ingress())
    }

    pub(crate) fn take_window_ingress(
        &mut self,
        capture_ordinal: u64,
    ) -> Option<ReservedHostRecord> {
        self.pending
            .values_mut()
            .find_map(|pending| pending.window.take_ingress(capture_ordinal))
    }

    pub(crate) fn owns_window(&self, window: WindowId, viewport: Option<ViewportId>) -> bool {
        self.pending
            .values()
            .any(|pending| pending.window.matches_window(window, viewport))
    }

    pub(crate) fn has_window(&self, viewport: ViewportId, binding: NativeSurfaceBinding) -> bool {
        self.pending
            .get(&binding)
            .is_some_and(|pending| pending.viewport == viewport)
    }

    pub(crate) fn transfer_destroyed(
        &mut self,
        binding: NativeSurfaceBinding,
        window: WindowId,
        ordinal: u64,
    ) -> bool {
        self.pending
            .get_mut(&binding)
            .filter(|pending| pending.window.window() == window)
            .is_some_and(|pending| pending.window.transfer_destroyed(ordinal))
    }

    pub(crate) fn commit_destroyed_callbacks(&mut self) {
        for pending in self.pending.values_mut() {
            pending.window.commit_destroyed();
        }
    }

    pub(crate) fn retire_unrecognized_destroyed(&mut self, binding: NativeSurfaceBinding) {
        if let Some(pending) = self.pending.get_mut(&binding)
            && pending.phase == RetirementPhase::DestroyedObserved
        {
            pending.cleanup.finish_destroyed_snapshot();
            pending.acknowledgement = None;
            // This provider no longer has a core tombstone to publish. Exact
            // route retirement still waits for the transferred callback commit.
            pending.phase = RetirementPhase::TombstoneCommitted;
        }
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
        pending.window.rebind(successor);
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
            (pending.phase == RetirementPhase::DestroyedObserved
                && pending.window.destroyed_committed())
            .then(|| {
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
            if pending.phase == RetirementPhase::DestroyedObserved
                && pending.window.destroyed_committed()
            {
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
            .filter(|(_, pending)| {
                pending.phase == RetirementPhase::TombstoneCommitted
                    && pending.window.destroyed_committed()
            })
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
            pending.viewport == viewport
                && pending.phase == RetirementPhase::TombstoneCommitted
                && pending.window.destroyed_committed()
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
        self.pending
            .iter()
            .filter_map(|(binding, pending)| {
                (pending.phase == RetirementPhase::RouteRetired
                    && pending.window.destroyed_committed())
                .then_some(*binding)
            })
            .chain(
                self.unmaterialized
                    .iter()
                    .filter_map(|(binding, receipt)| receipt.is_committed().then_some(*binding)),
            )
    }

    pub(crate) fn finish_quiescence(&mut self, binding: NativeSurfaceBinding) -> bool {
        if self
            .unmaterialized
            .get(&binding)
            .is_some_and(NativeCreateReceipt::is_committed)
        {
            self.unmaterialized.remove(&binding);
            return true;
        }
        if !self.pending.get(&binding).is_some_and(|pending| {
            pending.phase == RetirementPhase::RouteRetired && pending.window.destroyed_committed()
        }) {
            return false;
        }
        self.pending.remove(&binding);
        true
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use dockspace::model::{
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
        RootId, SurfaceId,
    };
    use dockspace::policy::DockPolicy;
    use dockspace::runtime::{
        DockspaceSession, HostInputOutcome, HostWindowToken, NativeMonotonicClock,
        NativePointerRoster, SurfaceUnavailableReason,
    };

    use super::*;
    use crate::mailbox::HostRecord;
    use crate::mailbox::journal::HostIngressJournal;

    fn destroyed_ingress(
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        window: WindowId,
        event_ordinal: u64,
    ) -> ReservedHostRecord {
        let mut journal =
            HostIngressJournal::new(NativeMonotonicClock::new(|| Duration::from_millis(17)));
        let predecessor = journal.reserve(HostRecord::DeviceRemoval(
            crate::event::NativeDeviceRemovalRecord::for_test(99),
        ));
        let ingress = journal.reserve(HostRecord::WindowEvent(
            crate::event::NativeWindowEventRecord::for_test(
                event_ordinal,
                window,
                Some(viewport),
                Some(binding),
                winit::event::WindowEvent::Destroyed,
            ),
        ));
        assert_eq!(ingress.ordinal(), predecessor.ordinal() + 1);
        ingress
    }

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
        let empty = state.owned_data_capacity_bytes();
        assert_eq!(empty, size_of::<NativeRetirementState>());
        let window = WindowId::from(22);
        let mut routes = crate::viewport_map::NativeViewportMap::default();
        routes.bind(viewport, window, binding).unwrap();
        state
            .retain_window(routes.take_window_receipt(viewport, binding).unwrap())
            .unwrap();
        let retained = state.owned_data_capacity_bytes();
        assert!(retained > empty);
        for (candidate_window, candidate_viewport, ordinal) in [
            (WindowId::from(23), Some(viewport), 1),
            (window, Some(ViewportId::ROOT), 1),
            (window, Some(viewport), 0),
        ] {
            assert_eq!(
                state.receive_destroyed(
                    candidate_window,
                    candidate_viewport,
                    ordinal,
                    |_, _| panic!("an unmatched or zero-ordinal terminal cannot reserve ingress"),
                ),
                None,
            );
        }
        assert_eq!(
            state.receive_destroyed(
                window,
                Some(viewport),
                1,
                |owner_viewport, owner_binding| {
                    assert_eq!(owner_viewport, viewport);
                    assert_eq!(owner_binding, binding);
                    destroyed_ingress(owner_viewport, owner_binding, window, 1)
                }
            ),
            Some((viewport, binding))
        );
        assert_eq!(state.window_ingress().count(), 1);
        assert_eq!(state.owned_data_capacity_bytes(), retained);
        let capture_ordinal = state.window_ingress().next().unwrap().ordinal();
        assert_ne!(capture_ordinal, 1);
        assert_eq!(
            state.receive_destroyed(window, Some(viewport), 2, |_, _| panic!(
                "a duplicate Destroyed cannot mint another capture"
            )),
            None,
        );
        assert!(state.take_window_ingress(capture_ordinal).is_none());
        assert_eq!(
            state.observe_destroyed(viewport, binding, window, 1),
            DestroyedObservation::Recorded
        );
        assert_eq!(
            state.observe_destroyed(viewport, binding, window, 1),
            DestroyedObservation::Duplicate
        );
        assert!(
            !state.requires_snapshot(),
            "raw admission is not a committed boundary"
        );
        assert!(state.transfer_destroyed(binding, window, 1));
        assert!(!state.requires_snapshot());
        state.commit_destroyed_callbacks();
        assert!(
            !state.requires_snapshot(),
            "semantic transfer cannot commit before the original capture hands off"
        );
        assert!(state.take_window_ingress(1).is_none());
        let ingress = state.take_window_ingress(capture_ordinal).unwrap();
        assert_eq!(ingress.ordinal(), capture_ordinal);
        assert_eq!(ingress.captured_at(), Duration::from_millis(17));
        let event = ingress.window_event().unwrap();
        assert_eq!(event.ordinal(), 1);
        assert_eq!(event.binding(), Some(binding));
        assert_eq!(event.viewport_id(), Some(viewport));
        assert_eq!(event.window_id(), window);
        assert!(state.window_ingress().next().is_none());
        assert!(state.take_window_ingress(capture_ordinal).is_none());
        assert!(!state.requires_snapshot());
        assert_eq!(state.owned_data_capacity_bytes(), retained);
        state.commit_destroyed_callbacks();
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
        assert_eq!(state.owned_data_capacity_bytes(), retained);
        let committed = state.committed_routes();
        assert_eq!(committed.len(), 1);
        state.commit_routes(&committed);
        assert_eq!(state.quiescence_candidates().collect::<Vec<_>>(), [binding]);
        assert_eq!(state.owned_data_capacity_bytes(), retained);
        assert!(state.finish_quiescence(binding));
        assert!(state.quiescence_candidates().next().is_none());
        assert_eq!(state.owned_data_capacity_bytes(), empty);
    }

    #[test]
    fn owned_data_capacity_keeps_failed_create_entries_until_quiescence() {
        let materialized = binding();
        let failed_binding = binding();
        let viewport = ViewportId::from_hash_of("capacity-retirement");
        let mut routes = crate::viewport_map::NativeViewportMap::default();
        routes
            .bind(viewport, WindowId::from(25), materialized)
            .unwrap();
        let mut state = NativeRetirementState::default();
        let empty = state.owned_data_capacity_bytes();
        state
            .retain_window(routes.take_window_receipt(viewport, materialized).unwrap())
            .unwrap();
        let pending = state.owned_data_capacity_bytes();
        assert!(pending > empty);

        let mut journal = HostIngressJournal::new(NativeMonotonicClock::system());
        let mut receipt = NativeCreateReceipt::new(None);
        let failed = crate::viewport_map::CreateTerminal::Failed(
            eframe::NativeViewportCreateFailureKind::WindowUnavailable,
        );
        assert_eq!(
            receipt.receive(None, failed, || {
                journal.reserve(HostRecord::ViewportCreateFailed(
                    crate::viewport_callback::NativeViewportCreateFailureRecord::new(
                        viewport,
                        failed_binding,
                        eframe::NativeViewportCreateFailureKind::WindowUnavailable,
                    ),
                ))
            }),
            crate::viewport_map::CreateAdmission::First,
        );
        let ordinal = receipt.ingress().unwrap().ordinal();
        assert!(receipt.transfer(None, failed));
        assert!(state.begin_unmaterialized_quiescence(failed_binding, receipt));
        let retained = state.owned_data_capacity_bytes();
        assert!(retained > pending);
        state.mark_create_failure_recorded(failed_binding);
        state.commit_create_failures();
        assert!(!state.finish_quiescence(failed_binding));
        assert_eq!(state.owned_data_capacity_bytes(), retained);
        let ingress = state.take_create_ingress(ordinal).unwrap();
        assert_eq!(ingress.ordinal(), ordinal);
        assert_eq!(state.owned_data_capacity_bytes(), retained);
        state.commit_create_failures();
        assert!(state.is_quiescence_candidate(failed_binding));
        assert_eq!(state.owned_data_capacity_bytes(), retained);
        assert!(state.finish_quiescence(failed_binding));
        assert_eq!(state.owned_data_capacity_bytes(), pending);
        assert!(state.has_window(viewport, materialized));
    }

    #[test]
    fn received_live_window_receipt_moves_its_exact_ingress_into_retirement() {
        let binding = binding();
        let viewport = ViewportId::from_hash_of("captured-live-window");
        let window = WindowId::from(24);
        let mut routes = crate::viewport_map::NativeViewportMap::default();
        routes.bind(viewport, window, binding).unwrap();
        let mut receipt = routes.take_window_receipt(viewport, binding).unwrap();
        assert!(receipt.receive_destroyed(3, || destroyed_ingress(viewport, binding, window, 3)));
        assert!(!receipt.receive_destroyed(4, || panic!(
            "capturing an attached receipt is once-only before retirement"
        )));
        let mut state = NativeRetirementState::default();
        assert_eq!(state.window_ingress().count(), 0);
        state.retain_window(receipt).unwrap();
        let ingress = state.window_ingress().next().unwrap();
        let capture_ordinal = ingress.ordinal();
        assert_ne!(capture_ordinal, 3);
        assert_eq!(ingress.captured_at(), Duration::from_millis(17));
        assert_eq!(ingress.window_event().unwrap().ordinal(), 3);
        assert_eq!(
            state.observe_destroyed(viewport, binding, window, 3),
            DestroyedObservation::Recorded,
        );
        assert!(state.take_window_ingress(capture_ordinal).is_none());
        assert!(!state.transfer_destroyed(binding, window, capture_ordinal));
        assert!(state.transfer_destroyed(binding, window, 3));
        state.commit_destroyed_callbacks();
        assert!(!state.requires_snapshot());
        let ingress = state.take_window_ingress(capture_ordinal).unwrap();
        assert_eq!(ingress.window_event().unwrap().binding(), Some(binding));
        assert_eq!(ingress.ordinal(), capture_ordinal);
        assert!(state.take_window_ingress(capture_ordinal).is_none());
        state.commit_destroyed_callbacks();
        assert!(state.requires_snapshot());
    }
}

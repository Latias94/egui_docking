//! Exact child-window retirement state owned by the native adapter.
//!
//! Core owns lifecycle meaning. This module only retains the affine close
//! acknowledgement and the exact eframe route until the operating system
//! reports destruction and the corresponding core frame commits.

use std::collections::BTreeMap;

use dockspace::runtime::{
    NativeCloseEffectAcknowledgement, NativeSurfaceBinding, NativeWindowFacts,
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

#[derive(Debug, Clone, Copy)]
struct PendingRetirement {
    viewport: ViewportId,
    acknowledgement: Option<NativeCloseEffectAcknowledgement>,
    phase: RetirementPhase,
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
    pub(crate) fn can_begin_release(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        !self.pending.contains_key(&binding)
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
            },
        );
        true
    }

    pub(crate) fn observe_destroyed(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        if let Some(pending) = self.pending.get_mut(&binding) {
            if pending.viewport != viewport {
                return false;
            }
            if pending.phase == RetirementPhase::AwaitingDestroyed {
                pending.phase = RetirementPhase::DestroyedObserved;
            }
            return true;
        }
        if self
            .pending
            .values()
            .any(|pending| pending.viewport == viewport)
        {
            return false;
        }
        self.pending.insert(
            binding,
            PendingRetirement {
                viewport,
                acknowledgement: None,
                phase: RetirementPhase::DestroyedObserved,
            },
        );
        true
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

    pub(crate) fn quiescence_candidates(
        &self,
    ) -> impl Iterator<Item = NativeSurfaceBinding> + '_ {
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
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
        SurfaceId,
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
        let mut frame = session.begin_host_frame().expect("registration frame begins");
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
        assert!(state.observe_destroyed(viewport, binding));
        assert_eq!(state.destroyed_observations().count(), 1);
        assert!(state.committed_routes().is_empty());

        state.mark_snapshot_queued();
        assert!(state.committed_routes().is_empty());
        state.settle_snapshot(false);
        assert_eq!(state.destroyed_observations().count(), 1);

        state.mark_snapshot_queued();
        state.settle_snapshot(true);
        let committed = state.committed_routes();
        assert_eq!(committed.len(), 1);
        state.commit_routes(&committed);
        assert_eq!(state.quiescence_candidates().collect::<Vec<_>>(), [binding]);
        assert!(state.finish_quiescence(binding));
        assert!(state.quiescence_candidates().next().is_none());
    }
}

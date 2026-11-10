//! Unbound initial establishment shares the existing surface lifecycle owner.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::geometry::PhysicalRect;
use crate::ids::SurfaceId;
use crate::model::DockspaceActionRejection;
use crate::viewport::ViewportBinding;

use super::{
    RecoveryReplacementLifecycle, RecoveryReplacementLifecycleError, SurfaceEstablishment,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NativeSurfaceInitialRequest {
    pub(crate) surface: SurfaceId,
    pub(crate) recovery_host: SurfaceId,
    pub(crate) placement: PhysicalRect,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NativeSurfaceRestorePlan {
    pub(crate) roots: BTreeSet<SurfaceId>,
    pub(crate) children: Vec<NativeSurfaceInitialRequest>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueuedInitialSurfaceStatus {
    Waiting,
    Cancelled,
    Rejected(DockspaceActionRejection),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct QueuedInitialSurface {
    pub(crate) request: NativeSurfaceInitialRequest,
    pub(crate) status: QueuedInitialSurfaceStatus,
    pub(crate) dependencies: Arc<BTreeSet<ViewportBinding>>,
}

impl RecoveryReplacementLifecycle {
    pub(crate) fn contains_surface(&self, surface: SurfaceId) -> bool {
        self.pending.contains_key(&surface)
    }

    pub(crate) fn surfaces(&self) -> impl Iterator<Item = SurfaceId> + '_ {
        self.pending.keys().copied()
    }

    pub(crate) fn queued_initial(&self, surface: SurfaceId) -> Option<&QueuedInitialSurface> {
        match self.pending.get(&surface)? {
            SurfaceEstablishment::Queued(initial) => Some(initial),
            SurfaceEstablishment::Bound(_) => None,
        }
    }

    pub(crate) fn waiting_initial(&self) -> impl Iterator<Item = &QueuedInitialSurface> {
        self.pending.values().filter_map(|entry| match entry {
            SurfaceEstablishment::Queued(initial)
                if initial.status == QueuedInitialSurfaceStatus::Waiting =>
            {
                Some(initial)
            }
            SurfaceEstablishment::Queued(_) | SurfaceEstablishment::Bound(_) => None,
        })
    }

    pub(crate) fn enqueue_initial(
        &mut self,
        request: NativeSurfaceInitialRequest,
        dependencies: Arc<BTreeSet<ViewportBinding>>,
    ) -> Result<(), RecoveryReplacementLifecycleError> {
        if self.contains_surface(request.surface) {
            return Err(RecoveryReplacementLifecycleError::AlreadyPending {
                surface: request.surface,
            });
        }
        self.pending.insert(
            request.surface,
            SurfaceEstablishment::Queued(QueuedInitialSurface {
                request,
                status: QueuedInitialSurfaceStatus::Waiting,
                dependencies,
            }),
        );
        Ok(())
    }

    pub(crate) fn stop_initial(
        &mut self,
        surface: SurfaceId,
        status: QueuedInitialSurfaceStatus,
    ) -> Result<bool, RecoveryReplacementLifecycleError> {
        let Some(SurfaceEstablishment::Queued(initial)) = self.pending.get_mut(&surface) else {
            return Err(RecoveryReplacementLifecycleError::MissingPending { surface });
        };
        if initial.status != QueuedInitialSurfaceStatus::Waiting {
            return Ok(false);
        }
        initial.status = status;
        Ok(true)
    }

    pub(crate) fn suspended_surfaces(&self) -> impl Iterator<Item = SurfaceId> + '_ {
        self.pending
            .iter()
            .filter_map(|(surface, entry)| match entry {
                SurfaceEstablishment::Queued(_) => Some(*surface),
                SurfaceEstablishment::Bound(pending) => {
                    pending.suspends_live_presentation().then_some(*surface)
                }
            })
    }
}

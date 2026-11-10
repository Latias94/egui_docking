//! Minimal ownership for deferred native viewport effects.
//!
//! This module is deliberately not an effect ledger. Core remains the only
//! authority for effect meaning and ordering. The adapter keeps only affine
//! viewport requests which must survive until eframe reports the matching
//! create, failure, or visibility-dispatch callback.

use dockspace::geometry::PhysicalRect;
use dockspace::runtime::{
    NativeDispatchFailure, NativeEffectOperation, NativeEffectRequest, NativeEffectResult,
    NativeSurfaceBinding, NativeSurfaceRole, NativeUnsupportedReason,
};
use eframe::{NativeViewportCreateFailureKind, egui::ViewportId};

use crate::error::NativeHostProtocolError;
use crate::viewport_callback::NativeViewportCreateFailureRecord;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeViewportEffectKind {
    Create,
    Replacement,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NativeViewportEffectPlan {
    viewport: ViewportId,
    binding: NativeSurfaceBinding,
    placement: PhysicalRect,
    role: NativeSurfaceRole,
    kind: NativeViewportEffectKind,
}

impl NativeViewportEffectPlan {
    pub(crate) const fn viewport(self) -> ViewportId {
        self.viewport
    }

    pub(crate) const fn binding(self) -> NativeSurfaceBinding {
        self.binding
    }

    pub(crate) const fn placement(self) -> PhysicalRect {
        self.placement
    }

    pub(crate) const fn role(self) -> NativeSurfaceRole {
        self.role
    }

    pub(crate) const fn kind(self) -> NativeViewportEffectKind {
        self.kind
    }
}

#[derive(Debug)]
struct PendingViewportEffect {
    plan: NativeViewportEffectPlan,
    request: NativeEffectRequest,
}

#[derive(Debug, Default)]
pub(crate) struct NativeEffectCoordinator {
    pending_viewports: Vec<PendingViewportEffect>,
    effect_slots: Option<usize>,
}

impl NativeEffectCoordinator {
    /// Reserves both affine-request stores.
    /// The stores share E live request slots; each prepays E backing entries so
    /// requests may move between create and show phases without reallocating.
    pub(crate) fn with_storage_limits(
        effect_slots: Option<usize>,
    ) -> Result<Self, NativeHostProtocolError> {
        Self::reservation_bytes(effect_slots)
            .ok_or(NativeHostProtocolError::NativeReportStorageUnavailable)?;
        let mut pending_viewports = Vec::new();
        if let Some(slots) = effect_slots {
            pending_viewports
                .try_reserve_exact(slots)
                .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
            if pending_viewports.capacity() > slots {
                return Err(NativeHostProtocolError::NativeReportStorageUnavailable);
            }
        }
        Ok(Self {
            pending_viewports,
            effect_slots,
        })
    }

    /// Includes inline state and both E backing stores. An unlimited dimension
    /// promises no backing. Core owns the shared affine-request lease queue.
    pub(crate) fn reservation_bytes(effect_slots: Option<usize>) -> Option<usize> {
        let per_effect = size_of::<PendingViewportEffect>();
        size_of::<Self>().checked_add(effect_slots.unwrap_or(0).checked_mul(per_effect)?)
    }

    /// Counts inline state and actual vector capacities, including spare slots.
    /// Allocator overhead is excluded. Core counts shared abandoned-effect
    /// backing once; each retained request keeps its original core lease.
    pub(crate) fn owned_data_capacity_bytes(&self) -> usize {
        size_of::<Self>().saturating_add(
            self.pending_viewports
                .capacity()
                .saturating_mul(size_of::<PendingViewportEffect>()),
        )
    }

    fn can_retain_effect(&self) -> bool {
        self.effect_slots
            .is_none_or(|slots| self.pending_viewports.len() < slots)
    }

    pub(crate) fn has_pending_work(&self) -> bool {
        !self.pending_viewports.is_empty()
    }

    pub(crate) fn awaiting_viewport_callbacks(
        &self,
    ) -> impl Iterator<Item = NativeViewportEffectPlan> + '_ {
        self.pending_viewports.iter().map(|pending| pending.plan)
    }

    pub(crate) fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.pending_viewports
            .iter()
            .any(|pending| pending.plan.binding == binding)
    }

    /// Prepares a create/replacement key and its backing before a callback or
    /// deferred declaration acquires an external owner.
    pub(crate) fn plan(
        &mut self,
        viewport: ViewportId,
        request: &NativeEffectRequest,
    ) -> Option<NativeViewportEffectPlan> {
        if self
            .pending_viewports
            .binary_search_by_key(&viewport, |pending| pending.plan.viewport)
            .is_ok()
            || !self.can_retain_effect()
        {
            return None;
        }
        let (binding, placement, role, kind) = match request.operation() {
            NativeEffectOperation::CreateWindow {
                binding,
                placement,
                role,
            } => (
                *binding,
                *placement,
                *role,
                NativeViewportEffectKind::Create,
            ),
            NativeEffectOperation::RequestReplacement {
                binding,
                placement,
                role,
            } => (
                *binding,
                *placement,
                *role,
                NativeViewportEffectKind::Replacement,
            ),
            _ => return None,
        };
        if self.pending_viewports.len() == self.pending_viewports.capacity()
            && (self.effect_slots.is_some() || self.pending_viewports.try_reserve(1).is_err())
        {
            return None;
        }
        Some(NativeViewportEffectPlan {
            viewport,
            binding,
            placement,
            role,
            kind,
        })
    }

    pub(crate) fn insert(
        &mut self,
        plan: NativeViewportEffectPlan,
        request: NativeEffectRequest,
    ) -> Result<(), NativeEffectRequest> {
        if self.plan(plan.viewport, &request) != Some(plan) {
            return Err(request);
        }
        let index = self
            .pending_viewports
            .partition_point(|pending| pending.plan.viewport < plan.viewport);
        self.pending_viewports
            .insert(index, PendingViewportEffect { plan, request });
        Ok(())
    }

    pub(crate) fn take_for_output(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Option<NativeEffectRequest> {
        let index = self
            .pending_viewports
            .binary_search_by_key(&viewport, |pending| pending.plan.viewport)
            .ok()?;
        if self.pending_viewports.get(index)?.plan.binding != binding {
            return None;
        }
        Some(self.pending_viewports.remove(index).request)
    }

    /// Hands an admitted create to the deferred viewport owner. Until this
    /// point the retained request is still safely undispatched. The backend
    /// may act on the declaration after handoff, so its retained request can
    /// never again be dropped as a known non-dispatch.
    pub(crate) fn dispatch_viewport(
        &mut self,
        plan: NativeViewportEffectPlan,
        declare: impl FnOnce(),
    ) -> bool {
        let Ok(index) = self
            .pending_viewports
            .binary_search_by_key(&plan.viewport, |pending| pending.plan.viewport)
        else {
            return false;
        };
        if self.pending_viewports[index].plan != plan {
            return false;
        }
        let pending = self.pending_viewports.remove(index);
        let dispatch = pending.request.begin_dispatch();
        declare();
        self.pending_viewports.insert(
            index,
            PendingViewportEffect {
                plan,
                request: dispatch.await_observation(),
            },
        );
        true
    }

    pub(crate) fn take_failure_result(
        &mut self,
        failure: NativeViewportCreateFailureRecord,
    ) -> Option<NativeEffectResult> {
        let request = self.take_for_output(failure.viewport(), failure.binding())?;
        Some(match failure.kind() {
            NativeViewportCreateFailureKind::WindowUnavailable => {
                request.not_applied(NativeDispatchFailure::WindowUnavailable)
            }
            NativeViewportCreateFailureKind::VisibilityUnsupported => {
                request.unsupported(NativeUnsupportedReason::BackendUnsupported)
            }
        })
    }

    pub(crate) fn remove_unstarted(
        &mut self,
        plan: NativeViewportEffectPlan,
    ) -> Option<NativeEffectRequest> {
        let index = self
            .pending_viewports
            .binary_search_by_key(&plan.viewport, |pending| pending.plan.viewport)
            .ok()?;
        if self.pending_viewports.get(index)?.plan != plan {
            return None;
        }
        Some(self.pending_viewports.remove(index).request)
    }
}

#[cfg(test)]
mod capacity_tests;

//! Minimal ownership for deferred native viewport effects.
//!
//! This module is deliberately not an effect ledger. Core remains the only
//! authority for effect meaning and ordering. The adapter keeps only affine
//! viewport requests which must survive until eframe reports the matching
//! create, failure, or visibility-dispatch callback.

use dockspace::geometry::PhysicalRect;
use dockspace::runtime::{
    NativeCreateStrategy, NativeDispatchFailure, NativeEffectOperation, NativeEffectRequest,
    NativeEffectResult, NativeSurfaceBinding, NativeSurfaceRole, NativeUnsupportedReason,
};
use eframe::{
    NativeViewportCreateFailureKind, NativeViewportVisibilityCommandToken, egui::ViewportId,
};

use crate::error::NativeHostProtocolError;
use crate::viewport_callback::{NativeViewportCreateFailureRecord, NativeViewportVisibilityRecord};
use winit::window::WindowId;

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

#[derive(Debug)]
struct PendingNativeShow {
    binding: NativeSurfaceBinding,
    viewport: ViewportId,
    window: WindowId,
    token: NativeViewportVisibilityCommandToken,
    request: NativeEffectRequest,
}

#[derive(Debug, Default)]
pub(crate) struct NativeEffectCoordinator {
    pending_viewports: Vec<PendingViewportEffect>,
    pending_shows: Vec<PendingNativeShow>,
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
        let mut pending_shows = Vec::new();
        if let Some(slots) = effect_slots {
            pending_viewports
                .try_reserve_exact(slots)
                .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
            pending_shows
                .try_reserve_exact(slots)
                .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
            if pending_viewports.capacity() > slots || pending_shows.capacity() > slots {
                return Err(NativeHostProtocolError::NativeReportStorageUnavailable);
            }
        }
        Ok(Self {
            pending_viewports,
            pending_shows,
            effect_slots,
        })
    }

    /// Includes inline state and both E backing stores. An unlimited dimension
    /// promises no backing. Core owns the shared affine-request lease queue.
    pub(crate) fn reservation_bytes(effect_slots: Option<usize>) -> Option<usize> {
        let per_effect =
            size_of::<PendingViewportEffect>().checked_add(size_of::<PendingNativeShow>())?;
        size_of::<Self>().checked_add(effect_slots.unwrap_or(0).checked_mul(per_effect)?)
    }

    /// Counts inline state and actual vector capacities, including spare slots.
    /// Allocator overhead is excluded. Core counts shared abandoned-effect
    /// backing once; each retained request keeps its original core lease.
    pub(crate) fn owned_data_capacity_bytes(&self) -> usize {
        size_of::<Self>()
            .saturating_add(
                self.pending_viewports
                    .capacity()
                    .saturating_mul(size_of::<PendingViewportEffect>()),
            )
            .saturating_add(
                self.pending_shows
                    .capacity()
                    .saturating_mul(size_of::<PendingNativeShow>()),
            )
    }

    fn can_retain_effect(&self) -> bool {
        self.effect_slots.is_none_or(|slots| {
            self.pending_viewports
                .len()
                .checked_add(self.pending_shows.len())
                .is_some_and(|retained| retained < slots)
        })
    }

    pub(crate) fn has_pending_work(&self) -> bool {
        !self.pending_viewports.is_empty() || !self.pending_shows.is_empty()
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
            || self.has_pending_show(binding)
    }

    pub(crate) fn has_pending_show(&self, binding: NativeSurfaceBinding) -> bool {
        self.pending_shows
            .binary_search_by_key(&binding, |pending| pending.binding)
            .is_ok()
    }

    /// Completes key, shared-quota and backing admission before reserving a
    /// visibility callback. With no intervening effect insertion, retention
    /// uses this same capacity without allocation. Tokens still settle exactly.
    pub(crate) fn preflight_can_retain_show(&mut self, binding: NativeSurfaceBinding) -> bool {
        if self.has_pending_show(binding) || !self.can_retain_effect() {
            return false;
        }
        if self.pending_shows.len() < self.pending_shows.capacity() {
            return true;
        }
        self.effect_slots.is_none() && self.pending_shows.try_reserve(1).is_ok()
    }

    pub(crate) fn retain_show(
        &mut self,
        binding: NativeSurfaceBinding,
        viewport: ViewportId,
        window: WindowId,
        token: NativeViewportVisibilityCommandToken,
        request: NativeEffectRequest,
    ) -> Result<(), NativeEffectRequest> {
        if !matches!(request.operation(), NativeEffectOperation::ShowWindow { binding: exact } if *exact == binding)
            || !self.preflight_can_retain_show(binding)
        {
            return Err(request);
        }
        let index = self
            .pending_shows
            .partition_point(|pending| pending.binding < binding);
        self.pending_shows.insert(
            index,
            PendingNativeShow {
                binding,
                viewport,
                window,
                token,
                request,
            },
        );
        Ok(())
    }

    pub(crate) fn take_show(
        &mut self,
        record: NativeViewportVisibilityRecord,
    ) -> Option<NativeEffectRequest> {
        if !self.matches_show(record) {
            return None;
        }
        let index = self
            .pending_shows
            .binary_search_by_key(&record.binding(), |pending| pending.binding)
            .ok()?;
        Some(self.pending_shows.remove(index).request)
    }

    pub(crate) fn matches_show(&self, record: NativeViewportVisibilityRecord) -> bool {
        self.pending_shows
            .binary_search_by_key(&record.binding(), |pending| pending.binding)
            .ok()
            .and_then(|index| self.pending_shows.get(index))
            .is_some_and(|pending| {
                pending.viewport == record.viewport()
                    && pending.window == record.window()
                    && pending.token == record.token()
                    && record.visible()
            })
    }

    #[cfg(test)]
    pub(crate) fn show_token(
        &self,
        binding: NativeSurfaceBinding,
    ) -> Option<NativeViewportVisibilityCommandToken> {
        self.pending_shows
            .iter()
            .find(|pending| pending.binding == binding)
            .map(|pending| pending.token)
    }

    pub(crate) fn remove_show(&mut self, binding: NativeSurfaceBinding) {
        if let Ok(index) = self
            .pending_shows
            .binary_search_by_key(&binding, |pending| pending.binding)
        {
            self.pending_shows.remove(index);
        }
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
                strategy: NativeCreateStrategy::Staged,
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
                strategy: NativeCreateStrategy::Staged,
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

    pub(crate) fn take_failure_result(
        &mut self,
        failure: NativeViewportCreateFailureRecord,
    ) -> Option<NativeEffectResult> {
        let request = self.take_for_output(failure.viewport(), failure.binding())?;
        Some(match failure.kind() {
            NativeViewportCreateFailureKind::WindowUnavailable => {
                request.dispatch_failed(NativeDispatchFailure::WindowUnavailable)
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

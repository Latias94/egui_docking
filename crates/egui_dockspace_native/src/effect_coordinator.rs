//! Minimal ownership for deferred native viewport effects.
//!
//! This module is deliberately not an effect ledger. Core remains the only
//! authority for effect meaning and ordering. The adapter keeps only affine
//! viewport requests which must survive until eframe reports the matching
//! create, failure, or visibility-dispatch callback.

use std::collections::BTreeMap;

use dockspace::geometry::PhysicalRect;
use dockspace::runtime::{
    NativeDispatchFailure, NativeEffectOperation, NativeEffectRequest, NativeEffectResult,
    NativeSurfaceBinding, NativeSurfaceRole, NativeUnsupportedReason,
};
use eframe::{
    NativeViewportCreateFailureKind,
    egui::{ViewportCommand, ViewportId},
};

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
    pending_viewports: BTreeMap<ViewportId, PendingViewportEffect>,
    pending_shows: BTreeMap<NativeSurfaceBinding, NativeEffectRequest>,
    pending_commands: Vec<PendingViewportCommand>,
}

#[derive(Debug)]
struct PendingViewportCommand {
    viewport: ViewportId,
    binding: NativeSurfaceBinding,
    command: ViewportCommand,
}

impl NativeEffectCoordinator {
    pub(crate) fn has_pending_work(&self) -> bool {
        !self.pending_viewports.is_empty()
            || !self.pending_shows.is_empty()
            || !self.pending_commands.is_empty()
    }

    pub(crate) fn awaiting_viewport_callbacks(
        &self,
    ) -> impl Iterator<Item = NativeViewportEffectPlan> + '_ {
        self.pending_viewports.values().map(|pending| pending.plan)
    }

    pub(crate) fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.pending_viewports
            .values()
            .any(|pending| pending.plan.binding == binding)
            || self.pending_shows.contains_key(&binding)
            || self
                .pending_commands
                .iter()
                .any(|pending| pending.binding == binding)
    }

    pub(crate) fn queue_command(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        command: ViewportCommand,
    ) {
        self.pending_commands.push(PendingViewportCommand {
            viewport,
            binding,
            command,
        });
    }

    pub(crate) fn take_commands(&mut self) -> Vec<(ViewportId, ViewportCommand)> {
        std::mem::take(&mut self.pending_commands)
            .into_iter()
            .map(|pending| (pending.viewport, pending.command))
            .collect()
    }

    pub(crate) fn remove_commands(&mut self, binding: NativeSurfaceBinding) {
        self.pending_commands
            .retain(|pending| pending.binding != binding);
    }

    pub(crate) fn remove_command(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        command: &ViewportCommand,
    ) -> bool {
        let Some(index) = self.pending_commands.iter().position(|pending| {
            pending.viewport == viewport
                && pending.binding == binding
                && pending.command == *command
        }) else {
            return false;
        };
        self.pending_commands.remove(index);
        true
    }

    pub(crate) fn has_pending_show(&self, binding: NativeSurfaceBinding) -> bool {
        self.pending_shows.contains_key(&binding)
    }

    pub(crate) fn retain_show(
        &mut self,
        binding: NativeSurfaceBinding,
        request: NativeEffectRequest,
    ) -> Result<(), NativeEffectRequest> {
        if self.pending_shows.contains_key(&binding) {
            return Err(request);
        }
        self.pending_shows.insert(binding, request);
        Ok(())
    }

    pub(crate) fn take_show(
        &mut self,
        binding: NativeSurfaceBinding,
    ) -> Option<NativeEffectRequest> {
        self.pending_shows.remove(&binding)
    }

    pub(crate) fn remove_show(&mut self, binding: NativeSurfaceBinding) {
        self.pending_shows.remove(&binding);
    }

    pub(crate) fn plan(
        &self,
        viewport: ViewportId,
        request: &NativeEffectRequest,
    ) -> Option<NativeViewportEffectPlan> {
        if self.pending_viewports.contains_key(&viewport) {
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
        if self.pending_viewports.contains_key(&plan.viewport) {
            return Err(request);
        }
        self.pending_viewports
            .insert(plan.viewport, PendingViewportEffect { plan, request });
        Ok(())
    }

    pub(crate) fn take_for_output(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Option<NativeEffectRequest> {
        let pending = self.pending_viewports.get(&viewport)?;
        if pending.plan.binding != binding {
            return None;
        }
        let pending = self
            .pending_viewports
            .remove(&viewport)
            .expect("the matching pending viewport effect remains present");
        Some(pending.request)
    }

    pub(crate) fn take_failure_result(
        &mut self,
        failure: NativeViewportCreateFailureRecord,
    ) -> Option<NativeEffectResult> {
        let pending = self.pending_viewports.get(&failure.viewport())?;
        if pending.plan.binding != failure.binding() {
            return None;
        }
        let pending = self
            .pending_viewports
            .remove(&failure.viewport())
            .expect("the matching pending viewport effect remains present");
        Some(match failure.kind() {
            NativeViewportCreateFailureKind::WindowUnavailable => pending
                .request
                .dispatch_failed(NativeDispatchFailure::WindowUnavailable),
            NativeViewportCreateFailureKind::VisibilityUnsupported => pending
                .request
                .unsupported(NativeUnsupportedReason::BackendUnsupported),
        })
    }

    pub(crate) fn remove_unstarted(
        &mut self,
        plan: NativeViewportEffectPlan,
    ) -> Option<NativeEffectRequest> {
        let pending = self.pending_viewports.get(&plan.viewport)?;
        if pending.plan != plan {
            return None;
        }
        let pending = self
            .pending_viewports
            .remove(&plan.viewport)
            .expect("the matching pending viewport effect remains present");
        Some(pending.request)
    }
}

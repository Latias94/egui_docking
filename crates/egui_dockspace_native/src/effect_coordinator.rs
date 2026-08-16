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
    state: PendingViewportEffectState,
}

#[derive(Debug)]
enum PendingViewportEffectState {
    AwaitingCallback(NativeEffectRequest),
    FailureResult(NativeEffectResult),
    FailureReported,
}

#[derive(Debug)]
pub(crate) enum PendingShowEffect {
    AwaitingDispatch(NativeEffectRequest),
    AwaitingUnsupportedReport(NativeEffectResult),
}

#[derive(Debug, Default)]
pub(crate) struct NativeEffectCoordinator {
    pending_viewports: BTreeMap<ViewportId, PendingViewportEffect>,
    pending_shows: BTreeMap<NativeSurfaceBinding, PendingShowEffect>,
    pending_commands: Vec<PendingViewportCommand>,
}

#[derive(Debug)]
struct PendingViewportCommand {
    viewport: ViewportId,
    binding: NativeSurfaceBinding,
    command: ViewportCommand,
}

impl NativeEffectCoordinator {
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
        self.pending_shows
            .insert(binding, PendingShowEffect::AwaitingDispatch(request));
        Ok(())
    }

    pub(crate) fn take_show(&mut self, binding: NativeSurfaceBinding) -> Option<PendingShowEffect> {
        self.pending_shows.remove(&binding)
    }

    pub(crate) fn restore_show_result(
        &mut self,
        binding: NativeSurfaceBinding,
        result: NativeEffectResult,
    ) -> Result<(), NativeEffectResult> {
        if self.pending_shows.contains_key(&binding) {
            return Err(result);
        }
        self.pending_shows.insert(
            binding,
            PendingShowEffect::AwaitingUnsupportedReport(result),
        );
        Ok(())
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
        self.pending_viewports.insert(
            plan.viewport,
            PendingViewportEffect {
                plan,
                state: PendingViewportEffectState::AwaitingCallback(request),
            },
        );
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
        match pending.state {
            PendingViewportEffectState::AwaitingCallback(request) => Some(request),
            state @ (PendingViewportEffectState::FailureResult(_)
            | PendingViewportEffectState::FailureReported) => {
                self.pending_viewports
                    .insert(viewport, PendingViewportEffect { state, ..pending });
                None
            }
        }
    }

    pub(crate) fn prepare_failure(&mut self, failure: NativeViewportCreateFailureRecord) -> bool {
        let Some(pending) = self.pending_viewports.get_mut(&failure.viewport()) else {
            return false;
        };
        if pending.plan.binding != failure.binding() {
            return false;
        }
        let state = std::mem::replace(
            &mut pending.state,
            PendingViewportEffectState::FailureReported,
        );
        pending.state = match state {
            PendingViewportEffectState::AwaitingCallback(request) => {
                PendingViewportEffectState::FailureResult(match failure.kind() {
                    NativeViewportCreateFailureKind::WindowUnavailable => {
                        request.dispatch_failed(NativeDispatchFailure::WindowUnavailable)
                    }
                    NativeViewportCreateFailureKind::VisibilityUnsupported => {
                        request.unsupported(NativeUnsupportedReason::BackendUnsupported)
                    }
                })
            }
            state @ (PendingViewportEffectState::FailureResult(_)
            | PendingViewportEffectState::FailureReported) => state,
        };
        true
    }

    pub(crate) fn take_failure_result(
        &mut self,
        failure: NativeViewportCreateFailureRecord,
    ) -> Option<NativeEffectResult> {
        let pending = self.pending_viewports.get_mut(&failure.viewport())?;
        if pending.plan.binding != failure.binding() {
            return None;
        }
        let state = std::mem::replace(
            &mut pending.state,
            PendingViewportEffectState::FailureReported,
        );
        match state {
            PendingViewportEffectState::FailureResult(result) => Some(result),
            state @ (PendingViewportEffectState::AwaitingCallback(_)
            | PendingViewportEffectState::FailureReported) => {
                pending.state = state;
                None
            }
        }
    }

    pub(crate) fn restore_failure_result(
        &mut self,
        failure: NativeViewportCreateFailureRecord,
        result: NativeEffectResult,
    ) -> Result<(), NativeEffectResult> {
        let Some(pending) = self.pending_viewports.get_mut(&failure.viewport()) else {
            return Err(result);
        };
        if pending.plan.binding != failure.binding()
            || !matches!(pending.state, PendingViewportEffectState::FailureReported)
        {
            return Err(result);
        }
        pending.state = PendingViewportEffectState::FailureResult(result);
        Ok(())
    }

    pub(crate) fn failure_reported(&self, failure: NativeViewportCreateFailureRecord) -> bool {
        self.pending_viewports
            .get(&failure.viewport())
            .is_some_and(|pending| {
                pending.plan.binding == failure.binding()
                    && matches!(pending.state, PendingViewportEffectState::FailureReported)
            })
    }

    pub(crate) fn finish_failure(&mut self, failure: NativeViewportCreateFailureRecord) -> bool {
        if !self.failure_reported(failure) {
            return false;
        }
        self.pending_viewports.remove(&failure.viewport());
        true
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
        match pending.state {
            PendingViewportEffectState::AwaitingCallback(request) => Some(request),
            state @ (PendingViewportEffectState::FailureResult(_)
            | PendingViewportEffectState::FailureReported) => {
                self.pending_viewports.insert(
                    plan.viewport,
                    PendingViewportEffect {
                        plan: pending.plan,
                        state,
                    },
                );
                None
            }
        }
    }
}

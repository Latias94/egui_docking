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
    NativeViewportCreateFailureKind, NativeViewportVisibilityCommandToken,
    egui::{ViewportCommand, ViewportId},
};

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
    viewport: ViewportId,
    window: WindowId,
    token: NativeViewportVisibilityCommandToken,
    request: NativeEffectRequest,
}

#[derive(Debug, Default)]
pub(crate) struct NativeEffectCoordinator {
    pending_viewports: BTreeMap<ViewportId, PendingViewportEffect>,
    pending_shows: BTreeMap<NativeSurfaceBinding, PendingNativeShow>,
    pending_commands: Vec<PendingViewportCommand>,
}

#[derive(Debug)]
struct PendingViewportCommand {
    viewport: ViewportId,
    binding: NativeSurfaceBinding,
    command: NativeViewportCommand,
}

/// The coordinator only produces commands without dynamically owned payloads.
#[derive(Debug, Clone, Copy)]
pub(crate) enum NativeViewportCommand {
    Close,
    Focus,
}

impl NativeViewportCommand {
    const fn into_viewport_command(self) -> ViewportCommand {
        match self {
            Self::Close => ViewportCommand::Close,
            Self::Focus => ViewportCommand::Focus,
        }
    }
}

impl NativeEffectCoordinator {
    /// Counts inline storage, retained entry data, and command backing capacity.
    /// B-tree node and allocator overhead are excluded. Request handles are
    /// inline; core counts their shared abandoned-effect queue once.
    pub(crate) fn owned_data_capacity_bytes(&self) -> usize {
        size_of::<Self>()
            .saturating_add(
                self.pending_viewports
                    .len()
                    .saturating_mul(size_of::<ViewportId>() + size_of::<PendingViewportEffect>()),
            )
            .saturating_add(
                self.pending_shows.len().saturating_mul(
                    size_of::<NativeSurfaceBinding>() + size_of::<PendingNativeShow>(),
                ),
            )
            .saturating_add(
                self.pending_commands
                    .capacity()
                    .saturating_mul(size_of::<PendingViewportCommand>()),
            )
    }

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
        command: NativeViewportCommand,
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
            .map(|pending| (pending.viewport, pending.command.into_viewport_command()))
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
                && pending.command.into_viewport_command() == *command
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
        viewport: ViewportId,
        window: WindowId,
        token: NativeViewportVisibilityCommandToken,
        request: NativeEffectRequest,
    ) -> Result<(), NativeEffectRequest> {
        if self.pending_shows.contains_key(&binding) {
            return Err(request);
        }
        self.pending_shows.insert(
            binding,
            PendingNativeShow {
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
        let pending = self.pending_shows.get(&record.binding())?;
        if pending.viewport != record.viewport()
            || pending.window != record.window()
            || pending.token != record.token()
            || !record.visible()
        {
            return None;
        }
        self.pending_shows
            .remove(&record.binding())
            .map(|pending| pending.request)
    }

    #[cfg(test)]
    pub(crate) fn show_token(
        &self,
        binding: NativeSurfaceBinding,
    ) -> Option<NativeViewportVisibilityCommandToken> {
        self.pending_shows
            .get(&binding)
            .map(|pending| pending.token)
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

    #[test]
    fn removed_commands_keep_capacity_until_queue_is_taken() {
        let surface = SurfaceId::new(1);
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            surface,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1)]),
            ),
        )])
        .expect("test layout validates");
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default())
            .expect("test session initializes");
        session
            .enable_managed_native_host(NativePointerRoster::Unknown)
            .expect("managed native host enrolls");
        session
            .register_native_root(surface, HostWindowToken::new(1))
            .expect("root registration queues");
        let mut frame = session
            .begin_host_frame()
            .expect("registration frame begins");
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("registration frame settles");
        let binding = frame
            .commit()
            .expect("registration frame commits")
            .inputs()
            .iter()
            .find_map(|input| match input {
                HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
                _ => None,
            })
            .expect("registration emits one binding");

        let mut effects = NativeEffectCoordinator::default();
        let empty = effects.owned_data_capacity_bytes();
        for _ in 0..8 {
            effects.queue_command(ViewportId::ROOT, binding, NativeViewportCommand::Close);
            effects.queue_command(ViewportId::ROOT, binding, NativeViewportCommand::Focus);
        }
        let queued = effects.owned_data_capacity_bytes();
        let capacity = effects.pending_commands.capacity();
        assert!(queued > empty);

        effects.remove_commands(binding);
        assert!(!effects.has_pending_work());
        assert_eq!(effects.pending_commands.capacity(), capacity);
        assert_eq!(effects.owned_data_capacity_bytes(), queued);

        effects.queue_command(ViewportId::ROOT, binding, NativeViewportCommand::Close);
        effects.queue_command(ViewportId::ROOT, binding, NativeViewportCommand::Focus);
        assert_eq!(effects.owned_data_capacity_bytes(), queued);
        assert!(effects.remove_command(ViewportId::ROOT, binding, &ViewportCommand::Close));
        assert_eq!(effects.owned_data_capacity_bytes(), queued);
        assert_eq!(
            effects.take_commands(),
            vec![(ViewportId::ROOT, ViewportCommand::Focus)],
        );
        assert_eq!(effects.owned_data_capacity_bytes(), empty);
    }
}

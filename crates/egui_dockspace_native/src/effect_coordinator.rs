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
    NativeViewportCreateFailureKind, NativeViewportVisibilityCommandToken,
    egui::{ViewportCommand, ViewportId},
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
    pending_commands: Vec<PendingViewportCommand>,
    effect_slots: Option<usize>,
    command_slots: Option<usize>,
}

#[derive(Debug)]
struct PendingViewportCommand {
    viewport: ViewportId,
    binding: NativeSurfaceBinding,
    command: NativeViewportCommand,
}

/// The coordinator only produces commands without dynamically owned payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// Reserves both affine-request stores and the independent command queue.
    /// The stores share E live request slots; each prepays E backing entries so
    /// requests may move between create and show phases without reallocating.
    pub(crate) fn with_storage_limits(
        effect_slots: Option<usize>,
        command_slots: Option<usize>,
    ) -> Result<Self, NativeHostProtocolError> {
        Self::reservation_bytes(effect_slots, command_slots)
            .ok_or(NativeHostProtocolError::NativeReportStorageUnavailable)?;
        let mut pending_viewports = Vec::new();
        let mut pending_shows = Vec::new();
        let mut pending_commands = Vec::new();
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
        if let Some(slots) = command_slots {
            pending_commands
                .try_reserve_exact(slots)
                .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
            if pending_commands.capacity() > slots {
                return Err(NativeHostProtocolError::NativeReportStorageUnavailable);
            }
        }
        Ok(Self {
            pending_viewports,
            pending_shows,
            pending_commands,
            effect_slots,
            command_slots,
        })
    }

    /// Includes inline state, both E backing stores and independent C backing.
    /// Unlimited dimensions promise no backing. Core owns the shared request
    /// lease queue, and draining commands keeps this same prepaid allocation.
    pub(crate) fn reservation_bytes(
        effect_slots: Option<usize>,
        command_slots: Option<usize>,
    ) -> Option<usize> {
        let per_effect =
            size_of::<PendingViewportEffect>().checked_add(size_of::<PendingNativeShow>())?;
        size_of::<Self>()
            .checked_add(effect_slots.unwrap_or(0).checked_mul(per_effect)?)?
            .checked_add(
                command_slots
                    .unwrap_or(0)
                    .checked_mul(size_of::<PendingViewportCommand>())?,
            )
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
            .saturating_add(
                self.pending_commands
                    .capacity()
                    .saturating_mul(size_of::<PendingViewportCommand>()),
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
        !self.pending_viewports.is_empty()
            || !self.pending_shows.is_empty()
            || !self.pending_commands.is_empty()
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
            || self
                .pending_commands
                .iter()
                .any(|pending| pending.binding == binding)
    }

    /// Performs all command backing allocation before accepting an effect or
    /// allowing an OS operation. A finite queue never grows beyond C slots.
    pub(crate) fn can_queue_command(&mut self) -> bool {
        if self
            .command_slots
            .is_some_and(|slots| self.pending_commands.len() >= slots)
        {
            return false;
        }
        if self.pending_commands.len() < self.pending_commands.capacity() {
            return true;
        }
        self.command_slots.is_none() && self.pending_commands.try_reserve(1).is_ok()
    }

    pub(crate) fn queue_command(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        command: NativeViewportCommand,
    ) -> Result<(), NativeViewportCommand> {
        if !self.can_queue_command() {
            return Err(command);
        }
        self.pending_commands.push(PendingViewportCommand {
            viewport,
            binding,
            command,
        });
        Ok(())
    }

    /// Yields queued commands in arrival order without releasing queue backing.
    pub(crate) fn drain_commands(
        &mut self,
    ) -> impl Iterator<Item = (ViewportId, ViewportCommand)> + '_ {
        self.pending_commands
            .drain(..)
            .map(|pending| (pending.viewport, pending.command.into_viewport_command()))
    }

    #[cfg(test)]
    pub(crate) fn take_commands(&mut self) -> Vec<(ViewportId, ViewportCommand)> {
        self.drain_commands().collect()
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
    fn removed_and_drained_commands_keep_backing_capacity() {
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
            .enable_managed_native_host(
                dockspace::runtime::NativeCreateStrategy::Staged,
                NativePointerRoster::Unknown,
            )
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
            effects
                .queue_command(ViewportId::ROOT, binding, NativeViewportCommand::Close)
                .unwrap();
            effects
                .queue_command(ViewportId::ROOT, binding, NativeViewportCommand::Focus)
                .unwrap();
        }
        let queued = effects.owned_data_capacity_bytes();
        let capacity = effects.pending_commands.capacity();
        assert!(queued > empty);

        effects.remove_commands(binding);
        assert!(!effects.has_pending_work());
        assert_eq!(effects.pending_commands.capacity(), capacity);
        assert_eq!(effects.owned_data_capacity_bytes(), queued);

        effects
            .queue_command(ViewportId::ROOT, binding, NativeViewportCommand::Close)
            .unwrap();
        effects
            .queue_command(ViewportId::ROOT, binding, NativeViewportCommand::Focus)
            .unwrap();
        assert_eq!(effects.owned_data_capacity_bytes(), queued);
        assert!(effects.remove_command(ViewportId::ROOT, binding, &ViewportCommand::Close));
        assert_eq!(effects.owned_data_capacity_bytes(), queued);
        assert_eq!(
            effects.take_commands(),
            vec![(ViewportId::ROOT, ViewportCommand::Focus)],
        );
        assert_eq!(effects.owned_data_capacity_bytes(), queued);
        assert_eq!(effects.pending_commands.capacity(), capacity);
    }
}

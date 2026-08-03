//! Durable presentation identity allocation and retirement authority.

use super::*;

/// Sole owner of the non-reusable presentation identity frontier.
///
/// Workspace publication, document restore, and native reservations all flow
/// through this owner so speculative engine candidates cannot update one
/// identity lane without preserving the other retired lanes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PresentationIdentityAuthority {
    frontier: PresentationIdentityFrontier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DragPresentationIdentityReservation {
    surface: SurfaceId,
    root: Option<RootId>,
    floating: Option<FloatingPresentationId>,
}

impl DragPresentationIdentityReservation {
    pub(super) const fn surface(self) -> SurfaceId {
        self.surface
    }

    pub(super) const fn root(self) -> Option<RootId> {
        self.root
    }

    pub(super) const fn floating(self) -> Option<FloatingPresentationId> {
        self.floating
    }
}

impl PresentationIdentityAuthority {
    pub(super) fn new(workspace: &Workspace, restored: PresentationIdentityFrontier) -> Self {
        let mut authority = Self { frontier: restored };
        authority.observe_workspace(workspace);
        authority
    }

    pub(super) const fn frontier(&self) -> PresentationIdentityFrontier {
        self.frontier
    }

    pub(super) fn merge_restored(
        &mut self,
        workspace: &Workspace,
        restored: PresentationIdentityFrontier,
    ) {
        self.frontier.merge(restored);
        self.observe_workspace(workspace);
    }

    pub(super) fn observe_workspace(&mut self, workspace: &Workspace) {
        for (surface, _) in workspace.surfaces() {
            self.frontier.observe_surface(surface);
        }
        for (root, _) in workspace.roots() {
            self.frontier.observe_root(root);
        }
        for (floating, _) in workspace.contained_floatings() {
            self.frontier.observe_floating(floating);
        }
    }

    pub(super) fn observe_native_reservations(
        &mut self,
        reservations: impl IntoIterator<Item = (SurfaceId, RootId, FloatingPresentationId)>,
    ) {
        for (surface, root, floating) in reservations {
            self.observe_native(surface, root, floating);
        }
    }

    pub(super) fn observe_native(
        &mut self,
        surface: SurfaceId,
        root: RootId,
        floating: FloatingPresentationId,
    ) {
        self.frontier.observe_surface(surface);
        self.frontier.observe_root(root);
        self.frontier.observe_floating(floating);
    }

    pub(super) fn observe_surface(&mut self, surface: SurfaceId) {
        self.frontier.observe_surface(surface);
    }

    pub(super) fn observe_root(&mut self, root: RootId) {
        self.frontier.observe_root(root);
    }

    pub(super) fn observe_floating(&mut self, floating: FloatingPresentationId) {
        self.frontier.observe_floating(floating);
    }

    pub(super) fn validate_fresh_surface(&self, surface: SurfaceId) -> Result<(), CommandError> {
        (surface.get() > self.frontier.last_surface())
            .then_some(())
            .ok_or(CommandError::RetiredSurfaceId {
                surface,
                frontier: self.frontier.last_surface(),
            })
    }

    pub(super) fn validate_fresh_or_continuing_surface(
        &self,
        surface: SurfaceId,
        continuing: bool,
    ) -> Result<(), CommandError> {
        if surface.get() > self.frontier.last_surface() || continuing {
            return Ok(());
        }
        Err(CommandError::RetiredSurfaceId {
            surface,
            frontier: self.frontier.last_surface(),
        })
    }

    pub(super) fn validate_fresh_root(&self, root: RootId) -> Result<(), CommandError> {
        (root.get() > self.frontier.last_root())
            .then_some(())
            .ok_or(CommandError::RetiredRootId {
                root,
                frontier: self.frontier.last_root(),
            })
    }

    pub(super) fn validate_fresh_floating(
        &self,
        floating: FloatingPresentationId,
    ) -> Result<(), CommandError> {
        (floating.get() > self.frontier.last_floating())
            .then_some(())
            .ok_or(CommandError::RetiredFloatingId {
                floating,
                frontier: self.frontier.last_floating(),
            })
    }

    #[cfg(test)]
    pub(super) fn reserve_root(&mut self) -> Result<RootId, EngineError> {
        self.frontier
            .reserve_root()
            .ok_or(EngineError::PresentationRootIdentityExhausted)
    }

    #[cfg(test)]
    pub(super) fn reserve_surface(&mut self) -> Result<SurfaceId, EngineError> {
        self.frontier
            .reserve_surface()
            .ok_or(EngineError::PresentationSurfaceIdentityExhausted)
    }

    pub(super) fn reserve_floating(&mut self) -> Result<FloatingPresentationId, EngineError> {
        self.frontier
            .reserve_floating()
            .ok_or(EngineError::PresentationFloatingIdentityExhausted)
    }

    pub(super) fn reserve_drag_presentation(
        &mut self,
        reserve_root: bool,
        reserve_floating: bool,
    ) -> Result<DragPresentationIdentityReservation, EngineError> {
        let mut candidate = self.frontier;
        let root = if reserve_root {
            Some(
                candidate
                    .reserve_root()
                    .ok_or(EngineError::PresentationRootIdentityExhausted)?,
            )
        } else {
            None
        };
        let floating = if reserve_floating {
            Some(
                candidate
                    .reserve_floating()
                    .ok_or(EngineError::PresentationFloatingIdentityExhausted)?,
            )
        } else {
            None
        };
        let surface = candidate
            .reserve_surface()
            .ok_or(EngineError::PresentationSurfaceIdentityExhausted)?;
        self.frontier = candidate;
        Ok(DragPresentationIdentityReservation {
            surface,
            root,
            floating,
        })
    }
}

impl DockEngine {
    fn validate_fresh_surface_identity(&self, surface: SurfaceId) -> Result<(), CommandError> {
        self.presentation_identity.validate_fresh_surface(surface)
    }

    fn validate_fresh_or_continuing_surface_identity(
        &self,
        surface: SurfaceId,
    ) -> Result<(), CommandError> {
        // A host frame may vacate and repopulate one still-bound surface before
        // publication. The binding gives that exact surface continuity through
        // the atomic tick; a pending native proposal never receives this escape
        // hatch because its tuple is a distinct, already-retired reservation.
        let continuing = self.workspace.surface(surface).is_none()
            && self.viewport.viewport(surface).is_some()
            && !self
                .viewport
                .native_create_sagas()
                .any(|(_, saga)| saga.prepared().proposal().surface() == surface);
        self.presentation_identity
            .validate_fresh_or_continuing_surface(surface, continuing)
    }

    fn validate_fresh_root_identity(&self, root: RootId) -> Result<(), CommandError> {
        self.presentation_identity.validate_fresh_root(root)
    }

    fn validate_fresh_floating_identity(
        &self,
        floating: FloatingPresentationId,
    ) -> Result<(), CommandError> {
        self.presentation_identity.validate_fresh_floating(floating)
    }

    pub(super) fn validate_application_command_identity_freshness(
        &self,
        command: &WorkspaceCommand,
    ) -> Result<(), CommandError> {
        match command {
            WorkspaceCommand::CreateSurfaceRoot { surface, root, .. } => {
                self.validate_fresh_or_continuing_surface_identity(*surface)?;
                self.validate_fresh_root_identity(*root)
            }
            WorkspaceCommand::CreateContainedRoot { root, floating, .. } => {
                self.validate_fresh_root_identity(*root)?;
                self.validate_fresh_floating_identity(*floating)
            }
            WorkspaceCommand::InstallMainRoot { root, .. } => {
                self.validate_fresh_root_identity(*root)
            }
            WorkspaceCommand::RehomeRoot { source, target } => match target {
                RootPresentationTarget::NewSurface { surface } => {
                    self.validate_fresh_or_continuing_surface_identity(*surface)
                }
                RootPresentationTarget::Contained { floating, .. }
                    if !matches!(
                        self.workspace.presentation_for_root(source.root()),
                        Some(crate::RootPresentationOwner::Contained { .. })
                    ) =>
                {
                    self.validate_fresh_floating_identity(*floating)
                }
                RootPresentationTarget::Main { .. } | RootPresentationTarget::Contained { .. } => {
                    Ok(())
                }
            },
            WorkspaceCommand::Select { .. }
            | WorkspaceCommand::Reorder { .. }
            | WorkspaceCommand::Open { .. }
            | WorkspaceCommand::Move { .. }
            | WorkspaceCommand::ResizeSplits { .. }
            | WorkspaceCommand::PromoteContained { .. }
            | WorkspaceCommand::UpdateContainedRect { .. }
            | WorkspaceCommand::UpdateContainedPresentation { .. }
            | WorkspaceCommand::RaiseContained { .. }
            | WorkspaceCommand::RemoveEmptyRoot { .. } => Ok(()),
        }
    }

    pub(super) fn adopt_application_command_identities(&mut self, command: &WorkspaceCommand) {
        match command {
            WorkspaceCommand::CreateSurfaceRoot { surface, root, .. } => {
                self.presentation_identity.observe_surface(*surface);
                self.presentation_identity.observe_root(*root);
            }
            WorkspaceCommand::CreateContainedRoot { root, floating, .. } => {
                self.presentation_identity.observe_root(*root);
                self.presentation_identity.observe_floating(*floating);
            }
            WorkspaceCommand::InstallMainRoot { root, .. } => {
                self.presentation_identity.observe_root(*root);
            }
            WorkspaceCommand::RehomeRoot { source, target } => match target {
                RootPresentationTarget::NewSurface { surface } => {
                    self.presentation_identity.observe_surface(*surface);
                }
                RootPresentationTarget::Contained { floating, .. }
                    if !matches!(
                        self.workspace.presentation_for_root(source.root()),
                        Some(crate::RootPresentationOwner::Contained { .. })
                    ) =>
                {
                    self.presentation_identity.observe_floating(*floating);
                }
                RootPresentationTarget::Main { .. } | RootPresentationTarget::Contained { .. } => {}
            },
            WorkspaceCommand::Select { .. }
            | WorkspaceCommand::Reorder { .. }
            | WorkspaceCommand::Open { .. }
            | WorkspaceCommand::Move { .. }
            | WorkspaceCommand::ResizeSplits { .. }
            | WorkspaceCommand::PromoteContained { .. }
            | WorkspaceCommand::UpdateContainedRect { .. }
            | WorkspaceCommand::UpdateContainedPresentation { .. }
            | WorkspaceCommand::RaiseContained { .. }
            | WorkspaceCommand::RemoveEmptyRoot { .. } => {}
        }
    }

    pub(super) fn validate_workspace_replacement_identity_freshness(
        &self,
        replacement: &Workspace,
    ) -> Result<(), CommandError> {
        for (surface, _) in replacement.surfaces() {
            if self.workspace.surface(surface).is_none() {
                self.validate_fresh_surface_identity(surface)?;
            }
        }
        for (root, _) in replacement.roots() {
            if self.workspace.root(root).is_none() {
                self.validate_fresh_root_identity(root)?;
            }
        }
        for (floating, _) in replacement.contained_floatings() {
            if self.workspace.contained_floating(floating).is_none() {
                self.validate_fresh_floating_identity(floating)?;
            }
        }
        Ok(())
    }

    fn observe_pending_native_identity_reservations(&mut self) {
        let reservations = self
            .viewport
            .native_create_sagas()
            .map(|(_, saga)| {
                (
                    saga.prepared().proposal().surface(),
                    saga.prepared().proposal().root(),
                    saga.prepared().proposal().converted_main().floating(),
                )
            })
            .collect::<Vec<_>>();
        self.presentation_identity
            .observe_native_reservations(reservations);
    }

    #[cfg(test)]
    pub(super) fn reserve_presentation_root_identity(&mut self) -> Result<RootId, EngineError> {
        self.observe_pending_native_identity_reservations();
        self.presentation_identity
            .observe_workspace(&self.workspace);
        self.presentation_identity.reserve_root()
    }

    #[cfg(test)]
    pub(super) fn reserve_presentation_surface_identity(
        &mut self,
    ) -> Result<crate::ids::SurfaceId, EngineError> {
        self.observe_pending_native_identity_reservations();
        self.presentation_identity
            .observe_workspace(&self.workspace);
        self.presentation_identity.reserve_surface()
    }

    pub(super) fn reserve_presentation_floating_identity(
        &mut self,
    ) -> Result<FloatingPresentationId, EngineError> {
        self.observe_pending_native_identity_reservations();
        self.presentation_identity
            .observe_workspace(&self.workspace);
        self.presentation_identity.reserve_floating()
    }

    pub(super) fn reserve_drag_presentation_identities(
        &mut self,
        reserve_root: bool,
        reserve_floating: bool,
    ) -> Result<DragPresentationIdentityReservation, EngineError> {
        self.observe_pending_native_identity_reservations();
        self.presentation_identity
            .observe_workspace(&self.workspace);
        self.presentation_identity
            .reserve_drag_presentation(reserve_root, reserve_floating)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composite_drag_reservation_is_atomic_when_the_surface_lane_is_exhausted() {
        let frontier = PresentationIdentityFrontier::from_counters(u64::MAX, 0, 0);
        let mut authority = PresentationIdentityAuthority { frontier };

        assert!(matches!(
            authority.reserve_drag_presentation(true, true),
            Err(EngineError::PresentationSurfaceIdentityExhausted)
        ));
        assert_eq!(authority.frontier(), frontier);
    }
}

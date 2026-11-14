use super::*;

impl DockEngine {
    pub(super) fn reduce_native_surface_placements(
        &mut self,
        input: InputSequence,
        provider: PlatformObservationLease,
        expected: WorkspaceVersion,
        window: crate::platform::viewport::WindowBinding,
        placements: &[crate::platform::coordinates::NativeSurfacePlacement],
    ) -> Result<InputOutcome, EngineError> {
        if expected != self.version {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: self.version,
            });
        }
        if let Some(rejected) = self.platform_provider_rejection(provider) {
            return Ok(rejected);
        }
        let root_window = self
            .viewport
            .registry()
            .window(window)
            .is_some_and(|record| {
                record.role() == ViewportRole::Root
                    && record.ownership() == ViewportOwnership::External
            });
        if !root_window {
            return Err(EngineError::Viewport {
                input,
                source: ViewportCoordinatorError::Registry(
                    crate::platform::registry::ViewportRegistryError::WindowPlacementUnavailable {
                        binding: window,
                    },
                ),
            });
        }
        for placement in placements {
            if self.workspace.surface(placement.surface()).is_none() {
                return Err(EngineError::Viewport {
                    input,
                    source: ViewportCoordinatorError::Registry(
                        crate::platform::registry::ViewportRegistryError::MissingSurface {
                            surface: placement.surface(),
                        },
                    ),
                });
            }
            if self
                .bound_surface_recoveries
                .contains_key(&placement.surface())
                || self.viewport.has_surface_establishment(placement.surface())
            {
                return Err(EngineError::Viewport {
                    input,
                    source: ViewportCoordinatorError::Registry(
                        crate::platform::registry::ViewportRegistryError::SurfaceAlreadyRegistered {
                            surface: placement.surface(),
                        },
                    ),
                });
            }
        }
        let changed = self
            .viewport
            .set_surface_placements(window, placements)
            .map_err(|source| EngineError::Viewport { input, source })?;
        for placement in placements {
            if !self
                .root_recovery_anchors
                .contains_key(&placement.surface())
            {
                let id = self
                    .last_root_recovery_anchor
                    .checked_next()
                    .ok_or(EngineError::RootRecoveryAnchorExhausted { input })?;
                self.root_recovery_anchors.insert(
                    placement.surface(),
                    RootRecoveryAnchor::new(self.authority_domain, id, placement.surface()),
                );
                self.last_root_recovery_anchor = id;
            }
        }
        let bindings_changed = !changed.is_empty();
        for surface in changed {
            self.invalidate_surface_coordinate_scene(surface)
                .map_err(|source| Self::scene_revision_error(input, source))?;
        }
        Ok(InputOutcome::NativeSurfacePlacementsChanged {
            window,
            changed: bindings_changed,
        })
    }
}

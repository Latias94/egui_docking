//! Semantic contained resize adjustments over an exact current candidate.

use super::*;

impl DockEngine {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn resize_contained_from_candidate(
        &mut self,
        input: InputSequence,
        candidate: &crate::presentation::scene::SurfacePlanScene,
        floating: FloatingPresentationId,
        direction: crate::presentation::scene::ContainedResizeDirection,
        delta: f64,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        let placement = match self.prepare_cardinal_contained_resize_placement(
            candidate.stamp(),
            candidate.plan(),
            floating,
            direction,
            delta,
        ) {
            Ok(placement) => placement,
            Err(error) => return Ok(self.candidate_action_rejection(error)),
        };
        if let Err(error) =
            Self::validate_contained_placement_geometry(placement.placement, candidate)
        {
            return Ok(self.candidate_action_rejection(
                InteractionRejection::ContainedPlacementUnavailable(error),
            ));
        }
        let command = WorkspaceCommand::UpdateContainedRect {
            surface: placement.placement.surface(),
            root: placement.root,
            floating: placement.floating,
            expected_rect: placement.expected_rect,
            rect: placement.placement.clamped_rect(),
        };
        let outcome =
            match self.apply_interaction_command_with_policy(input, policy, &command, events)? {
                CommandApplication::Applied { outcome, changed } => {
                    if changed {
                        self.invalidate_transient(
                            input,
                            InteractionCancelReason::WorkspaceChanged,
                            interaction_events,
                        )?;
                    }
                    InteractionOutcome::ContainedPlacementApplied { outcome, changed }
                }
                CommandApplication::Rejected(error) => {
                    InteractionOutcome::Rejected(InteractionRejection::CommandRejected(error))
                }
            };
        Ok(InputOutcome::InteractionProcessed {
            outcome,
            version: self.version,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn reduce_local_contained_resize_adjustment(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        floating: FloatingPresentationId,
        direction: crate::presentation::scene::ContainedResizeDirection,
        delta: f64,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != self.version {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: self.version,
            });
        }
        let placement = {
            let candidate = match self.semantic_candidate(scene) {
                Ok(candidate) => candidate,
                Err(error) => return Ok(self.candidate_action_rejection(error)),
            };
            match self.prepare_cardinal_contained_resize_placement(
                scene,
                candidate.plan(),
                floating,
                direction,
                delta,
            ) {
                Ok(placement) => placement,
                Err(error) => return Ok(self.candidate_action_rejection(error)),
            }
        };
        let outcome = self.apply_contained_placement(
            input,
            placement,
            ContainedPlacementAuthority::CurrentCandidate,
            policy,
            events,
            interaction_events,
        )?;
        Ok(InputOutcome::InteractionProcessed {
            outcome,
            version: self.version,
        })
    }

    pub(super) fn prepare_cardinal_contained_resize_placement(
        &self,
        scene: SurfaceSceneStamp,
        plan: &crate::presentation::scene::PresentationPlan,
        floating: FloatingPresentationId,
        direction: crate::presentation::scene::ContainedResizeDirection,
        delta: f64,
    ) -> Result<ContainedPlacementInput, InteractionRejection> {
        let surface = scene.surface();
        if plan.surface() != surface {
            return Err(InteractionRejection::TargetAuthorityInvalid);
        }
        let contained = plan
            .contained_records()
            .iter()
            .find(|record| {
                record.floating() == floating
                    && record.transform_operable()
                    && record.resize().iter().any(|resize| {
                        resize.direction() == direction
                            && plan.region_is_operable(resize.hit().rect(), record.layer())
                    })
            })
            .ok_or(InteractionRejection::ActionReceiverUnavailable {
                target: PresentationHitRegionKind::ContainedResize {
                    floating,
                    direction,
                },
            })?;
        let workspace_record = self
            .workspace
            .contained_floating(floating)
            .filter(|record| record.root == contained.root())
            .ok_or(InteractionRejection::TargetAuthorityInvalid)?;
        let unavailable = || {
            InteractionRejection::ContainedPlacementUnavailable(
                ContainedPlacementUnavailable::UnrepresentableGeometry { surface },
            )
        };
        let requested = cardinal_contained_resize_requested_rect(
            workspace_record.rect,
            plan.bounds(),
            contained.minimum_size(),
            direction,
            delta,
        )
        .map_err(|()| unavailable())?;
        let clamped =
            clamp_contained_rect(surface, plan.bounds(), requested, contained.minimum_size())
                .map_err(InteractionRejection::ContainedPlacementUnavailable)?;
        Ok(ContainedPlacementInput {
            root: contained.root(),
            floating,
            expected_rect: workspace_record.rect,
            placement: ContainedPlacementProof::new(
                scene,
                surface,
                requested,
                contained.minimum_size(),
                plan.bounds(),
                clamped,
            ),
        })
    }
}

impl DockEngine {
    /// Produces a deterministic contained placement from current ready surface bounds.
    ///
    /// The returned proof is valid only for the exact sealed scene generation from which it was
    /// derived. The requested size is first expanded to `minimum_size`, capped by the surface
    /// bounds, and then translated into those bounds without any history-based heuristic.
    ///
    /// # Errors
    ///
    /// Returns [`ContainedPlacementUnavailable`] when no current ready scene exists, the surface
    /// has no usable area, or finite corners cannot represent a strictly positive finite clamp.
    pub fn contained_placement(
        &self,
        surface: crate::ids::SurfaceId,
        requested_rect: crate::geometry::LogicalRect,
        minimum_size: crate::geometry::LogicalSize,
    ) -> Result<ContainedPlacementProof, ContainedPlacementUnavailable> {
        Self::contained_placement_from_scene(
            &self.presentation_authority.scene,
            self.version,
            surface,
            requested_rect,
            minimum_size,
        )
    }

    pub(super) fn contained_placement_from_scene(
        scene: &SurfaceSceneSet,
        workspace: WorkspaceVersion,
        surface: crate::ids::SurfaceId,
        requested_rect: crate::geometry::LogicalRect,
        minimum_size: crate::geometry::LogicalSize,
    ) -> Result<ContainedPlacementProof, ContainedPlacementUnavailable> {
        let (stamp, bounds) = Self::ready_surface_bounds(scene, workspace, surface)?;
        let clamped_rect = clamp_contained_rect(surface, bounds, requested_rect, minimum_size)?;
        Ok(ContainedPlacementProof::new(
            stamp,
            surface,
            requested_rect,
            minimum_size,
            bounds,
            clamped_rect,
        ))
    }

    pub(super) fn contained_placement_from_local_candidate(
        candidate: &crate::presentation::scene::SurfacePlanScene,
        workspace: WorkspaceVersion,
        surface: crate::ids::SurfaceId,
        requested_rect: crate::geometry::LogicalRect,
        minimum_size: crate::geometry::LogicalSize,
    ) -> Result<ContainedPlacementProof, ContainedPlacementUnavailable> {
        if candidate.stamp().surface() != surface
            || candidate.stamp().requirement().workspace_epoch() != workspace.epoch()
        {
            return Err(ContainedPlacementUnavailable::SceneUnavailable);
        }
        let bounds = candidate.plan().bounds();
        let clamped_rect = clamp_contained_rect(surface, bounds, requested_rect, minimum_size)?;
        Ok(ContainedPlacementProof::new(
            candidate.stamp(),
            surface,
            requested_rect,
            minimum_size,
            bounds,
            clamped_rect,
        ))
    }

    pub(super) fn ready_surface_bounds(
        scene: &SurfaceSceneSet,
        workspace: WorkspaceVersion,
        surface: crate::ids::SurfaceId,
    ) -> Result<(SurfaceSceneStamp, crate::geometry::LogicalRect), ContainedPlacementUnavailable>
    {
        let ready = match scene.ready_surface(surface) {
            Some(ready) => ready,
            None if matches!(scene.surface(surface), Some(SurfaceScene::Ready(_))) => {
                return Err(ContainedPlacementUnavailable::PendingPaintSurface { surface });
            }
            None if matches!(scene.surface(surface), Some(SurfaceScene::Stale(_))) => {
                return Err(ContainedPlacementUnavailable::StaleSurface { surface });
            }
            None if matches!(scene.surface(surface), Some(SurfaceScene::Bootstrap(_))) => {
                return Err(ContainedPlacementUnavailable::BootstrapSurface { surface });
            }
            None => return Err(ContainedPlacementUnavailable::MissingSurface { surface }),
        };
        if ready.stamp().requirement().workspace_epoch() != workspace.epoch() {
            return Err(ContainedPlacementUnavailable::SceneUnavailable);
        }
        Ok((ready.stamp(), ready.plan().bounds()))
    }

    pub(super) fn validate_contained_placement(
        &self,
        proof: ContainedPlacementProof,
    ) -> Result<(), ContainedPlacementUnavailable> {
        let scene = &self.presentation_authority.scene;
        let current = scene
            .ready_surface(proof.surface())
            .map(|ready| ready.stamp());
        if current != Some(proof.scene())
            || proof.scene().requirement().workspace_epoch() != self.version.epoch()
        {
            return Err(ContainedPlacementUnavailable::StaleScene {
                expected: proof.scene(),
                current,
            });
        }
        let ready = match scene.ready_surface(proof.surface()) {
            Some(ready) => ready,
            None if matches!(scene.surface(proof.surface()), Some(SurfaceScene::Ready(_))) => {
                return Err(ContainedPlacementUnavailable::PendingPaintSurface {
                    surface: proof.surface(),
                });
            }
            None if matches!(scene.surface(proof.surface()), Some(SurfaceScene::Stale(_))) => {
                return Err(ContainedPlacementUnavailable::StaleSurface {
                    surface: proof.surface(),
                });
            }
            None if matches!(
                scene.surface(proof.surface()),
                Some(SurfaceScene::Bootstrap(_))
            ) =>
            {
                return Err(ContainedPlacementUnavailable::BootstrapSurface {
                    surface: proof.surface(),
                });
            }
            None => {
                return Err(ContainedPlacementUnavailable::MissingSurface {
                    surface: proof.surface(),
                });
            }
        };
        Self::validate_contained_placement_geometry(proof, ready)
    }

    pub(super) fn validate_contained_placement_geometry(
        proof: ContainedPlacementProof,
        ready: &crate::presentation::scene::SurfacePlanScene,
    ) -> Result<(), ContainedPlacementUnavailable> {
        if ready.stamp() != proof.scene() {
            return Err(ContainedPlacementUnavailable::StaleScene {
                expected: proof.scene(),
                current: Some(ready.stamp()),
            });
        }
        if ready.plan().bounds() != proof.surface_bounds() {
            return Err(ContainedPlacementUnavailable::ProofMismatch {
                surface: proof.surface(),
            });
        }
        let reproduced = clamp_contained_rect(
            proof.surface(),
            ready.plan().bounds(),
            proof.requested_rect(),
            proof.minimum_size(),
        )?;
        if reproduced != proof.clamped_rect() {
            return Err(ContainedPlacementUnavailable::ProofMismatch {
                surface: proof.surface(),
            });
        }
        Ok(())
    }
}

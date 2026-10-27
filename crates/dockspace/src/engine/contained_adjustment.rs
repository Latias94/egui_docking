//! Semantic contained resize adjustments over an exact current candidate.

use super::*;

impl DockEngine {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn reduce_local_contained_resize_adjustment(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        application_base: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        floating: FloatingPresentationId,
        direction: crate::scene::ContainedResizeDirection,
        delta: f64,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        if expected != application_base {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: application_base,
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
        plan: &crate::scene::PresentationPlan,
        floating: FloatingPresentationId,
        direction: crate::scene::ContainedResizeDirection,
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
            .ok_or(InteractionRejection::SemanticReceiverUnavailable {
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

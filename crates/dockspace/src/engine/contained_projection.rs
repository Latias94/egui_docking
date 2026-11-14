//! Transient contained layout compiled through the normal measured surface path.

#[cfg(test)]
mod tests;

use super::contained_geometry::{contained_manipulation_point, contained_move_geometry};
use super::*;
use crate::interaction::{ActiveResizeView, DragSessionId};
use crate::presentation::scene::{ContainedLayoutOwner, ContainedLayoutProjection};
use std::sync::Arc;

impl DockEngine {
    pub(super) fn contained_layout_projection(
        &self,
        surface: SurfaceId,
    ) -> Option<ContainedLayoutProjection> {
        let transform = match self.interaction.status() {
            InteractionStatus::ContainedTransforming { session } => {
                self.interaction.active_contained_transform(session).ok()
            }
            _ => None,
        };
        if let Some(transform) = transform.filter(|transform| transform.surface == surface)
            && let Some(preview) = transform.preview.as_ref()
        {
            return Some(ContainedLayoutProjection {
                owner: ContainedLayoutOwner::Transform(transform.session),
                surface,
                root: transform.root,
                floating: transform.floating,
                source_rect: transform.source_rect,
                rect: preview.public().rect(),
                target_basis: self.contained_projection_target_basis(surface),
            });
        }

        let drag = match self.interaction.status() {
            InteractionStatus::Dragging { session } => self.interaction.active_drag(session).ok(),
            _ => None,
        }
        .or_else(|| {
            self.pending_mutation
                .as_ref()
                .and_then(PendingMutation::drag_release)
                .map(|pending| &pending.drag)
        })?;
        let FrozenDragOrigin::Contained(origin) = &drag.origin else {
            return None;
        };
        if origin.surface != surface
            || drag
                .complete_root
                .as_ref()
                .is_none_or(|source| source.root() != origin.root)
        {
            return None;
        }
        let point = contained_manipulation_point(drag.last_advanced_position()?, surface)?;
        let bounds = self
            .scene()
            .surface(surface)?
            .paint_projection()?
            .plan()
            .bounds();
        let rect = match drag.preview().map(|preview| preview.proof()) {
            Some(PreviewProof::Contained { proposal, .. })
                if proposal.surface() == surface
                    && proposal.root() == origin.root
                    && proposal.floating() == origin.floating
                    && proposal.placement().surface_bounds() == bounds =>
            {
                // The non-docking candidate already solved movement from the
                // validated manipulation lane. Its content uses that result.
                proposal.rect()
            }
            _ => {
                // Docking feedback has destination geometry instead. The live
                // source still follows the same manipulation-only move rule.
                contained_move_geometry(bounds, origin.source_rect, origin.initial_pointer, point)
                    .ok()?
                    .1
            }
        };
        Some(ContainedLayoutProjection {
            owner: ContainedLayoutOwner::Drag(drag.session),
            surface,
            root: origin.root,
            floating: origin.floating,
            source_rect: origin.source_rect,
            rect,
            target_basis: self.contained_projection_target_basis(surface),
        })
    }

    fn contained_projection_target_basis(&self, surface: SurfaceId) -> Option<SurfaceSceneStamp> {
        self.scene()
            .ready_candidate(surface)?
            .plan()
            .contained_projection()
            .and_then(|projection| projection.target_basis)
    }

    pub(super) fn drag_target_layout_basis(
        &self,
        surface: SurfaceId,
        session: DragSessionId,
    ) -> Option<SurfaceSceneStamp> {
        let candidate = self.scene().ready_candidate(surface)?;
        Some(
            candidate
                .plan()
                .contained_projection()
                .filter(|projection| projection.owner == ContainedLayoutOwner::Drag(session))
                .and_then(|projection| projection.target_basis)
                .unwrap_or(candidate.stamp()),
        )
    }

    pub(super) fn current_contained_projection_is_owned(&self, surface: SurfaceId) -> bool {
        let Some(current) = self
            .scene()
            .ready_candidate(surface)
            .and_then(|candidate| candidate.plan().contained_projection())
        else {
            return false;
        };
        if self.contained_layout_projection(surface) != Some(current) {
            return false;
        }
        match current.owner {
            ContainedLayoutOwner::Drag(_) => true,
            ContainedLayoutOwner::Transform(session) => self
                .interaction
                .active_contained_transform(session)
                .ok()
                .is_some_and(|transform| self.contained_transform_layout_is_compatible(transform)),
        }
    }

    pub(super) fn contained_transform_layout_is_compatible(
        &self,
        transform: &ActiveContainedTransform,
    ) -> bool {
        self.scene()
            .ready_candidate(transform.surface)
            .is_some_and(|candidate| {
                candidate.plan().bounds() == transform.surface_bounds
                    && candidate.plan().contained_minimums().iter().any(|minimum| {
                        minimum.floating() == transform.floating
                            && minimum.minimum_size() == transform.minimum_size
                    })
            })
    }

    pub(super) fn refresh_presentation_layouts(
        &mut self,
        tick: ReducerTickId,
        freshly_measured: Option<SurfaceId>,
    ) -> Result<bool, EngineError> {
        let surfaces = self
            .scene()
            .surfaces()
            .map(|(surface, _)| *surface)
            .collect::<Vec<_>>();
        let mut changed = false;
        for surface in surfaces {
            if Some(surface) == freshly_measured {
                continue;
            }
            let projection = self.contained_layout_projection(surface);
            let Some(candidate) = self.scene().ready_candidate(surface) else {
                continue;
            };
            let motion_changed = {
                #[cfg(feature = "motion")]
                {
                    self.motion_needs_refresh(surface)
                }
                #[cfg(not(feature = "motion"))]
                {
                    false
                }
            };
            let resize_updates = self
                .interaction
                .active_resize_view()
                .filter(|resize| resize.surface() == surface)
                .map(ActiveResizeView::updates)
                .unwrap_or_default();
            if candidate.plan().contained_projection() == projection
                && candidate.plan().resize_projection() == resize_updates
                && !motion_changed
            {
                continue;
            }
            let Some(layout) = candidate.plan().layout_facts() else {
                continue;
            };
            let measurements = Arc::clone(layout.measurements());
            let token = self.begin_surface_contribution(surface).map_err(|source| {
                EngineError::PresentationRequirementInvariant {
                    input: Some(self.last_input),
                    detail: source.to_string(),
                }
            })?;
            if measurements.ticket() != token.ticket() {
                continue;
            }
            let contribution = self
                .prepare_surface_contribution(token, (*measurements).clone())
                .map_err(|source| EngineError::PresentationRequirementInvariant {
                    input: Some(self.last_input),
                    detail: source.to_string(),
                })?;
            let outcome =
                self.reduce_surface_contribution(tick, contribution, self.policy.revision())?;
            changed |= outcome.changes_surface_authority();
        }
        Ok(changed)
    }
}

//! Transient contained layout compiled through the normal measured surface path.

#[cfg(test)]
mod tests;

use super::*;
use crate::interaction::DragSessionId;
use crate::scene::{ContainedLayoutOwner, ContainedLayoutProjection};
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
        }
        .or_else(|| {
            self.pending_contained_transform_release
                .as_ref()
                .map(|pending| &pending.transform)
        });
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
            self.pending_drag_release
                .as_ref()
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
        let point = match drag.last_advanced_position()? {
            ManipulationPositionKey::JournalHost { manipulation, .. } => manipulation
                .filter(|route| route.surface() == surface)?
                .position(),
            ManipulationPositionKey::JournalNativeLocal { manipulation, .. } => manipulation
                .filter(|route| route.binding().surface() == surface)?
                .surface_position(),
            ManipulationPositionKey::JournalSurface {
                scope: PointerProviderScope::SurfaceLocal(scope),
                position: Authority::Known(point),
            } if scope.surface() == surface => point,
            ManipulationPositionKey::JournalDesktop(DesktopRouteValidation::Known(
                crate::pointer_journal::ValidatedDesktopRoute::Dock(route),
            )) if route.binding().surface() == surface => route.surface_position(),
            _ => return None,
        };
        let bounds = self
            .scene()
            .surface(surface)?
            .paint_projection()?
            .plan()
            .bounds();
        let requested = LogicalRect::new(
            origin.source_rect.x() + point.x() - origin.initial_pointer.x(),
            origin.source_rect.y() + point.y() - origin.initial_pointer.y(),
            origin.source_rect.width(),
            origin.source_rect.height(),
        )
        .ok()?;
        let rect = clamp_moved_contained_rect(bounds, requested).ok()?;
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
            ContainedLayoutOwner::Drag(_) => current.target_basis.is_some(),
            ContainedLayoutOwner::Transform(session) => self
                .interaction
                .active_contained_transform(session)
                .ok()
                .or_else(|| {
                    self.pending_contained_transform_release
                        .as_ref()
                        .filter(|pending| pending.session == session)
                        .map(|pending| &pending.transform)
                })
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

    pub(super) fn refresh_contained_layouts(
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
            if candidate.plan().contained_projection() == projection {
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
            let policy = self.policy.clone();
            let outcome = self.reduce_surface_contribution(tick, contribution, &policy)?;
            changed |= outcome.changes_surface_authority();
        }
        Ok(changed)
    }
}

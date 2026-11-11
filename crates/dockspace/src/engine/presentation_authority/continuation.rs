//! Current-presentation gates and gesture continuation after scene changes.

use crate::engine::*;

impl DockEngine {
    /// Returns one exact, independently presented interaction projection under
    /// the current workspace popup routing identity.
    #[must_use]
    pub fn interaction_projection(
        &self,
        surface: SurfaceId,
    ) -> Option<SurfaceInteractionProjection<'_>> {
        self.presentation_authority
            .scene
            .interaction_projection(surface)
    }

    /// Returns one exact final-presentation authority after the global gate.
    #[must_use]
    pub fn interaction_authority(&self, surface: SurfaceId) -> Option<PresentedSurfaceAuthority> {
        self.presentation_authority
            .scene
            .interaction_authority(surface)
    }

    #[cfg(test)]
    pub(in crate::engine) fn freeze_interaction_projection(
        projection: SurfaceInteractionProjection<'_>,
    ) -> FrozenPresentationAuthority {
        FrozenPresentationAuthority::new(projection.authority(), projection.popup_revision())
    }

    pub(in crate::engine) fn freeze_journal_presentation(
        presentation: &JournalSurfacePresentation,
    ) -> FrozenPresentationAuthority {
        FrozenPresentationAuthority::new(presentation.authority(), presentation.popup_revision())
    }

    pub(in crate::engine) fn presentation_authority_is_current(
        &self,
        frozen: FrozenPresentationAuthority,
    ) -> bool {
        self.presentation_authority
            .scene
            .interaction_projection(frozen.surface())
            .is_some_and(|current| {
                current.popup_revision() == frozen.popup_revision
                    && current
                        .authority()
                        .same_interaction_semantics(frozen.presented)
            })
    }

    pub(in crate::engine) fn resize_continuation_is_current(
        &self,
        frozen: FrozenPresentationAuthority,
    ) -> bool {
        let InteractionStatus::Resizing { session } = self.interaction.status() else {
            return false;
        };
        let Ok(resize) = self.interaction.active_resize(session) else {
            return false;
        };
        if resize.authority != frozen
            || resize.surface != frozen.surface()
            || self
                .presentation_authority
                .presentation_requirements
                .surface(resize.surface)
                .is_none_or(|requirements| requirements.ticket() != resize.scene.requirement())
            || !Self::coordinate_capture_matches_current(
                resize.coordinate_capture,
                self.viewport.viewport(resize.surface),
                self.viewport.surface_coordinate_authority(resize.surface),
            )
        {
            return false;
        }

        resize
            .axis_groups
            .iter()
            .flat_map(|group| group.sources())
            .all(|source| {
                self.workspace
                    .verify_reference(
                        source.root(),
                        source.node(),
                        source.reference(),
                        crate::error::ReferenceRole::Source,
                    )
                    .is_ok()
                    && matches!(self.workspace.presentation_for_root(source.root()),
                    Some(crate::RootPresentationOwner::Main { surface })
                    | Some(crate::RootPresentationOwner::Contained { surface, .. })
                    if surface == resize.surface)
            })
    }

    pub(in crate::engine) fn scene_gesture_continuation_draft(
        &self,
        activation: ReductionCause,
        owner: GestureOwner,
        origin: FrozenPresentationAuthority,
        source: SceneGestureContinuationSource,
    ) -> Option<SceneGestureContinuationDraft> {
        Some(SceneGestureContinuationDraft {
            activation,
            owner,
            origin,
            workspace: self.version,
            policy: self.policy.revision(),
            config: self.presentation_authority.presentation_config_revision,
            requirements: self
                .presentation_authority
                .presentation_requirements
                .revision(),
            popup_routing: self
                .presentation_authority
                .presentation_requirements
                .popup()
                .revision(),
            source,
        })
    }

    pub(in crate::engine) fn scene_gesture_continuation_is_current(
        &self,
        continuation: &SceneGestureContinuation,
    ) -> bool {
        let draft = &continuation.draft;
        if self.version != draft.workspace
            || self.policy.revision() != draft.policy
            || self.presentation_authority.presentation_config_revision != draft.config
            || self
                .presentation_authority
                .presentation_requirements
                .revision()
                != draft.requirements
            || self
                .presentation_authority
                .presentation_requirements
                .popup()
                .revision()
                != draft.popup_routing
            || self.interaction.active_owner() != Some(draft.owner)
        {
            return false;
        }

        match (&continuation.session, &draft.source) {
            (
                SceneGestureSession::Drag(session),
                SceneGestureContinuationSource::Drag {
                    payload,
                    source_surface,
                    complete_root,
                    origin,
                    coordinates,
                },
            ) => {
                let active_source_matches = match self.interaction.status() {
                    InteractionStatus::Armed { session: active } if active == *session => {
                        self.interaction.armed_drag(*session).is_ok_and(|drag| {
                            drag.payload == *payload
                                && drag.source_surface == *source_surface
                                && drag.complete_root == *complete_root
                                && drag.origin == *origin
                                && drag.presentation == draft.origin
                        })
                    }
                    InteractionStatus::Dragging { session: active } if active == *session => {
                        self.interaction.active_drag(*session).is_ok_and(|drag| {
                            drag.payload == *payload
                                && drag.source_surface == *source_surface
                                && drag.complete_root == *complete_root
                                && drag.origin == *origin
                                && drag.presentation == draft.origin
                        })
                    }
                    InteractionStatus::Idle
                    | InteractionStatus::Pressed { .. }
                    | InteractionStatus::Armed { .. }
                    | InteractionStatus::Dragging { .. }
                    | InteractionStatus::Resizing { .. }
                    | InteractionStatus::ContainedTransforming { .. } => false,
                };
                let source_coordinates_are_current = match (self.interaction.status(), origin) {
                    (
                        InteractionStatus::Dragging { session: active },
                        FrozenDragOrigin::Workspace,
                    ) if active == *session => Self::coordinate_capture_incarnation_is_current(
                        *coordinates,
                        self.viewport.viewport(*source_surface),
                        self.viewport.surface_coordinate_authority(*source_surface),
                    ),
                    _ => Self::coordinate_capture_matches_current(
                        *coordinates,
                        self.viewport.viewport(*source_surface),
                        self.viewport.surface_coordinate_authority(*source_surface),
                    ),
                };
                if draft.origin.surface() != *source_surface
                    || !active_source_matches
                    || !source_coordinates_are_current
                {
                    return false;
                }
                let Ok(prepared) = self.prepare_drag_source(payload) else {
                    return false;
                };
                if prepared.source_surface != *source_surface
                    || prepared.complete_root != *complete_root
                    || !self.frozen_drag_origin_is_current(origin.clone())
                {
                    return false;
                }
                match origin {
                    FrozenDragOrigin::Workspace => true,
                    FrozenDragOrigin::Contained(origin) => self
                        .workspace
                        .capture_contained_roster(origin.surface)
                        .is_ok_and(|current| current == origin.source_roster),
                }
            }
            (
                SceneGestureSession::ContainedTransform(session),
                SceneGestureContinuationSource::ContainedTransform {
                    source,
                    surface,
                    root,
                    floating,
                    source_rect,
                    expected_roster,
                    coordinates,
                },
            ) => {
                let Ok(transform) = self.interaction.active_contained_transform(*session) else {
                    return false;
                };
                draft.origin.surface() == *surface
                    && transform.presentation == draft.origin
                    && transform.surface == *surface
                    && transform.root == *root
                    && transform.floating == *floating
                    && transform.source_rect == *source_rect
                    && Self::coordinate_capture_matches_current(
                        *coordinates,
                        self.viewport.viewport(*surface),
                        self.viewport.surface_coordinate_authority(*surface),
                    )
                    && self
                        .workspace
                        .capture_node_source(*root, source.node())
                        .is_ok_and(|current| current == *source)
                    && self.workspace.presentation_for_root(*root)
                        == Some(crate::RootPresentationOwner::Contained {
                            surface: *surface,
                            floating: *floating,
                        })
                    && self
                        .workspace
                        .contained_floating(*floating)
                        .is_some_and(|record| record.root == *root && record.rect == *source_rect)
                    && self
                        .workspace
                        .capture_contained_roster(*surface)
                        .is_ok_and(|current| current == *expected_roster)
            }
            (
                SceneGestureSession::Drag(_),
                SceneGestureContinuationSource::ContainedTransform { .. },
            )
            | (
                SceneGestureSession::ContainedTransform(_),
                SceneGestureContinuationSource::Drag { .. },
            ) => false,
        }
    }

    pub(in crate::engine) fn cancel_active_if_presentation_revoked(
        &mut self,
        input_for_error: InputSequence,
    ) -> Result<Option<InteractionStatus>, EngineError> {
        let Some(frozen) = self.interaction.active_presentation_authority() else {
            return Ok(None);
        };
        if self.presentation_authority_is_current(frozen)
            || self.resize_continuation_is_current(frozen)
            || self
                .interaction
                .active_scene_gesture_continuation()
                .is_some_and(|continuation| {
                    self.scene_gesture_continuation_is_current(continuation)
                })
        {
            return Ok(None);
        }
        self.viewport
            .end_all_drag_routing()
            .map_err(|source| EngineError::Viewport {
                input: input_for_error,
                source,
            })?;
        Ok(self.interaction.cancel_active())
    }

    pub(in crate::engine) fn cancel_revoked_presentation_caused(
        &mut self,
        cause: ReductionCause,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<Option<InteractionOutcome>, EngineError> {
        let Some(status) = self.cancel_active_if_presentation_revoked(self.last_input)? else {
            return Ok(None);
        };
        let reason = InteractionCancelReason::SceneUnavailable;
        interaction_events.push(InteractionEvent::new_caused(
            cause,
            self.version,
            InteractionEventKind::Cancelled { status, reason },
        ));
        Ok(Some(InteractionOutcome::Cancelled { status, reason }))
    }

    pub(in crate::engine) fn cancel_revoked_presentation_input(
        &mut self,
        input: InputSequence,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<Option<InteractionOutcome>, EngineError> {
        let Some(status) = self.cancel_active_if_presentation_revoked(input)? else {
            return Ok(None);
        };
        let reason = InteractionCancelReason::SceneUnavailable;
        interaction_events.push(InteractionEvent::new(
            input,
            self.version,
            InteractionEventKind::Cancelled { status, reason },
        ));
        Ok(Some(InteractionOutcome::Cancelled { status, reason }))
    }
}

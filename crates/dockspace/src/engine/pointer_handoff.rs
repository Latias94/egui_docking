//! One ordered coordinate-lane upgrade within the existing native gesture owner.

use super::*;

/// Private preparation for one edgewise segment, never a second input capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NativePointerHandoffProjection {
    Local {
        stream: PointerStreamId,
    },
    Desktop {
        stream: PointerStreamId,
    },
    Upgrade {
        sequence: PointerEdgeSequence,
        owner: GestureOwner,
        session: crate::interaction::DragSessionId,
    },
}

impl NativePointerHandoffProjection {
    pub(super) const fn stream(self) -> PointerStreamId {
        match self {
            Self::Local { stream } | Self::Desktop { stream } => stream,
            Self::Upgrade { owner, .. } => owner.stream(),
        }
    }

    pub(super) fn edge_view(self, edge: &PointerEdge) -> PointerEdge {
        let PointerEdgeLocation::NativeLocalToDesktop { local, desktop } = edge.location() else {
            unreachable!("only an explicit dual-location edge owns a handoff projection");
        };
        let location = match self {
            Self::Local { .. } => PointerEdgeLocation::NativeLocal {
                route: Authority::Known(local),
            },
            Self::Desktop { .. } | Self::Upgrade { .. } => {
                PointerEdgeLocation::Desktop { route: desktop }
            }
        };
        edge.with_location(location)
    }

    pub(super) fn upgrades_owner(self) -> bool {
        matches!(self, Self::Upgrade { .. })
    }
}

impl DockEngine {
    pub(super) fn prepare_native_handoff_projection(
        &self,
        stream: PointerStreamId,
        edge: &PointerEdge,
    ) -> Option<NativePointerHandoffProjection> {
        let PointerEdgeLocation::NativeLocalToDesktop { local, desktop } = edge.location() else {
            return None;
        };
        let local_fallback = Some(NativePointerHandoffProjection::Local { stream });
        let Some(owner) = self
            .interaction
            .active_owner()
            .filter(|owner| owner.stream() == stream)
        else {
            return local_fallback;
        };
        if owner.is_desktop() {
            // An already-established desktop lane consumes its ordinary facts;
            // the extra local sample cannot grant another coordinate upgrade.
            return Some(NativePointerHandoffProjection::Desktop { stream });
        }
        let (
            GestureLane::NativeLocal {
                binding,
                coordinate_generation,
            },
            InteractionStatus::Dragging { session },
            PointerEdgeKind::Moved,
        ) = (owner.lane(), self.interaction.status(), edge.kind())
        else {
            return local_fallback;
        };
        if local.binding() != binding
            || local.coordinate_generation() != coordinate_generation
            || self.pointer_provider() != Some(stream.lease())
            || edge.delivery_owner() != Authority::Known(PointerEventDeliveryOwner::Native(binding))
            || edge.capture_owner() != Authority::Known(PointerCaptureOwner::Native(binding))
            || !self
                .pointer_journal
                .native_primary_press_continuity(stream, binding)
        {
            return local_fallback;
        }
        let Ok(drag) = self.interaction.active_drag(session) else {
            return local_fallback;
        };
        if drag.owner != owner
            || drag.source_surface != binding.surface()
            || drag.presentation.presented.binding() != Some(binding)
            || drag.journal_capture_authority
                != Authority::Known(PointerCaptureOwner::Native(binding))
            || !self.drag_source_presentation_allows_targeting(drag)
        {
            return local_fallback;
        }
        let PointerProviderScope::NativeJoined(host) = stream.lease().scope() else {
            return local_fallback;
        };
        let Some(coordinates) = self
            .native_local_coordinate_snapshot(host, binding)
            .filter(|coordinates| coordinates.coordinate_generation() == coordinate_generation)
        else {
            return local_fallback;
        };
        let (Some(content), Authority::Known(point)) =
            (coordinates.content_bounds(), desktop.position())
        else {
            return local_fallback;
        };
        if point.x() - content.x() != local.client_position().x()
            || point.y() - content.y() != local.client_position().y()
            || coordinates.desktop_to_surface(point).ok() != Some(local.surface_position())
        {
            return local_fallback;
        }
        Some(NativePointerHandoffProjection::Upgrade {
            sequence: edge.sequence(),
            owner,
            session,
        })
    }

    pub(super) fn native_handoff_candidate_spec(
        &self,
        projection: NativePointerHandoffProjection,
        edge: &PointerEdge,
    ) -> PointerReceiverCandidateSpec {
        let NativePointerHandoffProjection::Upgrade { owner, .. } = projection else {
            return PointerReceiverCandidateSpec::not_applicable(edge.sequence());
        };
        if self.prepare_native_handoff_projection(owner.stream(), edge) != Some(projection) {
            return PointerReceiverCandidateSpec::not_applicable(edge.sequence());
        }
        let effective = projection.edge_view(edge);
        let position = journal_position_key(self, owner.stream(), &effective);
        if !position.hover_probe_requested() {
            return PointerReceiverCandidateSpec::not_applicable(edge.sequence());
        }
        let probe = position.hover_probe().filter(|probe| {
            self.interaction_projection(probe.surface())
                .is_some_and(|projection| {
                    effective
                        .desktop_route()
                        .map(|route| {
                            route.validate_against_registry(
                                self.authority_domain,
                                self.viewport.registry(),
                            )
                        })
                        .and_then(DesktopRouteValidation::dock_route)
                        .is_some_and(|route| {
                            route
                                .validate_presented_authority(projection.authority())
                                .is_ok()
                        })
                })
        });
        PointerReceiverCandidateSpec::hover_hit(
            edge.sequence(),
            probe.map(|probe| probe.surface()),
            probe.map(SurfacePointer::position),
        )
    }

    pub(super) fn apply_native_handoff_projection(
        &mut self,
        projection: NativePointerHandoffProjection,
        edge: &PointerEdge,
    ) -> Result<(), EngineError> {
        let NativePointerHandoffProjection::Upgrade {
            sequence,
            owner,
            session,
        } = projection
        else {
            return Ok(());
        };
        if sequence != edge.sequence()
            || self.prepare_native_handoff_projection(owner.stream(), edge) != Some(projection)
        {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "native handoff no longer matches its original ordered owner",
            });
        }
        let source =
            self.interaction
                .active_surface()
                .ok_or(EngineError::ReductionCauseInvariant {
                    detail: "native handoff lost its source surface",
                })?;
        self.interaction
            .upgrade_drag_to_desktop(session, owner)
            .map_err(|_| EngineError::ReductionCauseInvariant {
                detail: "validated native handoff could not update its exact gesture owner",
            })?;
        if self
            .viewport
            .physical_cross_surface_drag_capability()
            .is_supported()
        {
            let drag = self.interaction.active_drag(session).map_err(|_| {
                EngineError::ReductionCauseInvariant {
                    detail: "native handoff lost its active payload",
                }
            })?;
            let input = self.native_drag_input_mode(drag);
            let _ = self
                .viewport
                .begin_drag_routing(owner.pointer(), source, input)
                .map_err(|source| EngineError::Viewport {
                    input: self.last_input,
                    source,
                })?;
        }
        Ok(())
    }
}

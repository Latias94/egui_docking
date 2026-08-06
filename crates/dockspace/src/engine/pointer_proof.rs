//! Pointer receiver and presentation authority validation.

use super::*;

pub(super) fn pointer_receiver_candidate_spec(
    engine: &DockEngine,
    edge: &PointerEdge,
    stream: PointerStreamId,
) -> PointerReceiverCandidateSpec {
    if matches!(edge.kind(), PointerEdgeKind::Scrolled(_)) {
        let PointerEdgeKind::Scrolled(scroll) = edge.kind() else {
            unreachable!("the branch is restricted to scroll edges");
        };
        let delivery_point = match edge.location() {
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(point),
            } => Some(point),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Unknown(_),
            } => None,
            PointerEdgeLocation::Desktop { .. } => validated_desktop_delivery_route(engine, edge)
                .and_then(DesktopRouteValidation::dock_route)
                .map(|route| route.surface_position()),
        };
        return PointerReceiverCandidateSpec::scroll_delivery(
            edge.sequence(),
            delivery_point,
            engine.scroll_receiver_challenge(stream, scroll),
        );
    }
    let (receiver_route, hover_point) = match edge.location() {
        PointerEdgeLocation::SurfaceLocal {
            position: Authority::Known(point),
        } => (true, Some(point)),
        PointerEdgeLocation::SurfaceLocal {
            position: Authority::Unknown(_),
        } => (true, None),
        PointerEdgeLocation::Desktop { route } => match route
            .validate_against_registry(engine.authority_domain, engine.viewport.registry())
            .dock_route()
        {
            Some(route) => (true, Some(route.surface_position())),
            None => (false, None),
        },
    };
    if !receiver_route {
        return PointerReceiverCandidateSpec::not_applicable(edge.sequence());
    }

    let owns_stream = engine.interaction.active_stream() == Some(stream);
    let (delivery, hover) = match (engine.interaction.status(), edge.kind(), owns_stream) {
        (InteractionStatus::Idle, PointerEdgeKind::ButtonPressed(PointerButton::Primary), _) => {
            (true, false)
        }
        (
            InteractionStatus::Pressed { .. },
            PointerEdgeKind::ButtonReleased(PointerButton::Primary)
            | PointerEdgeKind::ContactEnded(PointerButton::Primary),
            true,
        ) => (true, false),
        (
            InteractionStatus::Armed { .. } | InteractionStatus::Dragging { .. },
            PointerEdgeKind::Moved,
            true,
        )
        | (
            InteractionStatus::Dragging { .. },
            PointerEdgeKind::ButtonReleased(PointerButton::Primary)
            | PointerEdgeKind::ContactEnded(PointerButton::Primary),
            true,
        ) => (false, true),
        _ => (false, false),
    };
    match (delivery, hover) {
        (false, false) => PointerReceiverCandidateSpec::not_applicable(edge.sequence()),
        (true, false) => PointerReceiverCandidateSpec::delivery(edge.sequence(), hover_point),
        (false, true) => PointerReceiverCandidateSpec::hover_hit(edge.sequence(), hover_point),
        (true, true) => {
            PointerReceiverCandidateSpec::delivery_and_hover_hit(edge.sequence(), hover_point)
        }
    }
}

/// Joins the edge-local delivery binding with the independent desktop position.
/// The hovered-window classification is deliberately not consulted.
pub(super) fn validated_desktop_delivery_route(
    engine: &DockEngine,
    edge: &PointerEdge,
) -> Option<DesktopRouteValidation> {
    let position = edge.desktop_route()?.position();
    let Authority::Known(PointerEventDeliveryOwner::Native(binding)) = edge.delivery_owner() else {
        return None;
    };
    Some(
        DesktopRouteFact::dock_from_desktop_position(binding, position)
            .validate_against_registry(engine.authority_domain, engine.viewport.registry()),
    )
}

/// Returns the semantic delivery lane which must have produced a core action
/// for one claimed receiver region.
///
/// Pane and contained-frame fallbacks deliberately have no action lane: they
/// prove only that docking did not own a control activation at that point.
const fn delivery_action_lane(kind: PresentationHitRegionKind) -> Option<PresentationPointerLane> {
    match kind {
        PresentationHitRegionKind::TabClose(_)
        | PresentationHitRegionKind::TabStripControl(_)
        | PresentationHitRegionKind::TabListMenuRow { .. }
        | PresentationHitRegionKind::TabListMenuBlocker(_)
        | PresentationHitRegionKind::TabListMenuBackdrop(_)
        | PresentationHitRegionKind::ContainedClose(_) => Some(PresentationPointerLane::Click),
        PresentationHitRegionKind::TabStripScroll(_)
        | PresentationHitRegionKind::TabListMenuScroll(_) => Some(PresentationPointerLane::Scroll),
        PresentationHitRegionKind::TabBody(_)
        | PresentationHitRegionKind::TabGroupGrip(_)
        | PresentationHitRegionKind::SplitterHandle(_)
        | PresentationHitRegionKind::SplitterJunction(_)
        | PresentationHitRegionKind::ContainedTitle(_)
        | PresentationHitRegionKind::ContainedResize { .. } => Some(PresentationPointerLane::Drag),
        PresentationHitRegionKind::PaneBody(_)
        | PresentationHitRegionKind::ContainedFrameBlocker(_)
        | PresentationHitRegionKind::DropGuideActivation(_)
        | PresentationHitRegionKind::DropTarget(_) => None,
    }
}

pub(super) const fn is_tab_list_menu_click(kind: PresentationHitRegionKind) -> bool {
    matches!(
        kind,
        PresentationHitRegionKind::TabListMenuRow { .. }
            | PresentationHitRegionKind::TabListMenuBlocker(_)
            | PresentationHitRegionKind::TabListMenuBackdrop(_)
    )
}

/// Failure while proving that a receipt names the semantic receiver at the
/// journal edge's exact logical point.
///
/// Receipt construction proves only that a region belongs to a presented
/// output. The reducer additionally owns the point-to-region relationship so
/// an adapter cannot reuse another valid region from the same output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PointerReceiverGeometryError {
    /// The receipt survived output-authority validation but the corresponding
    /// current interaction manifest is no longer available. This is an engine
    /// invariant failure surfaced as a fail-closed receipt rejection.
    #[error(
        "pointer receipt for edge {sequence} has no current interaction manifest for surface {surface}"
    )]
    InteractionManifestUnavailable {
        /// Exact provider sequence carrying the claim.
        sequence: crate::pointer_journal::PointerEdgeSequence,
        /// Surface named by the receipt's semantic region.
        surface: SurfaceId,
    },
    /// The current provider lane cannot supply a logical point for a docking
    /// receiver claim.
    #[error("pointer receipt for edge {sequence} names a docking region without a logical point")]
    LogicalPointUnavailable {
        /// Exact provider sequence carrying the claim.
        sequence: crate::pointer_journal::PointerEdgeSequence,
    },
    /// A known receiver-absence claim was bound to another logical surface.
    #[error(
        "pointer receipt for edge {sequence} claims no receiver on surface {actual}, expected {expected}"
    )]
    AbsenceSurfaceMismatch {
        /// Exact provider sequence carrying the claim.
        sequence: crate::pointer_journal::PointerEdgeSequence,
        /// Surface implied by the provider route.
        expected: SurfaceId,
        /// Surface bound to the claimed absence observation.
        actual: SurfaceId,
    },
    /// A desktop-global route was validated against the native inventory, but
    /// the receiver output belongs to another binding incarnation or
    /// coordinate generation.
    #[error(
        "pointer receipt for edge {sequence} does not match its exact desktop route presentation: {source}"
    )]
    DesktopRoutePresentationMismatch {
        /// Exact provider sequence carrying the claim.
        sequence: crate::pointer_journal::PointerEdgeSequence,
        /// Exact route-to-presentation mismatch.
        #[source]
        source: DesktopRoutePresentationError,
    },
    /// A local edge was answered using a receiver from another surface.
    #[error(
        "pointer receipt for edge {sequence} names receiver {region:?} on surface {actual}, expected {expected}"
    )]
    SurfaceMismatch {
        /// Exact provider sequence carrying the claim.
        sequence: crate::pointer_journal::PointerEdgeSequence,
        /// Surface implied by the provider's local endpoint.
        expected: SurfaceId,
        /// Surface owned by the receipt's semantic region.
        actual: SurfaceId,
        /// Claimed semantic region.
        region: PresentationHitRegionId,
    },
    /// A claimed docking region does not cover the edge's exact logical point.
    #[error("pointer receipt for edge {sequence} names region {region:?} outside the edge point")]
    RegionDoesNotCoverPoint {
        /// Exact provider sequence carrying the claim.
        sequence: crate::pointer_journal::PointerEdgeSequence,
        /// Claimed semantic region.
        region: PresentationHitRegionId,
    },
    /// A receiver claim lost the core-owned deterministic winner comparison
    /// for its action lane. `None` represents an asserted known absence.
    #[error(
        "pointer receipt for edge {sequence} claims receiver {claimed:?}, but lane {lane:?} winner is {winner:?}"
    )]
    ReceiverWinnerMismatch {
        /// Exact provider sequence carrying the claim.
        sequence: crate::pointer_journal::PointerEdgeSequence,
        /// Action lane selected from the semantic region kind.
        lane: PresentationPointerLane,
        /// Region claimed by the adapter, or `None` for known absence.
        claimed: Option<PresentationHitRegionId>,
        /// Core-computed winner, when one exists.
        winner: Option<PresentationHitRegionId>,
    },
    /// The core hit manifest contained two equally ranked regions on a lane,
    /// so no receipt can safely choose one.
    #[error("pointer receipt for edge {sequence} has an ambiguous {lane:?} receiver winner")]
    ReceiverWinnerAmbiguous {
        /// Exact provider sequence carrying the claim.
        sequence: crate::pointer_journal::PointerEdgeSequence,
        /// Action lane selected from the semantic region kind.
        lane: PresentationPointerLane,
    },
}

impl DockEngine {
    pub(super) fn validate_pointer_receiver_geometry(
        &self,
        provider: PointerInputLease,
        journal: &PointerEdgeJournal,
        receipts: &ValidatedPointerReceiverReceiptBatch,
        snapshot: &JournalPresentationSnapshot,
        desktop_hover_routes: &BTreeMap<PointerEdgeSequence, DesktopRouteValidation>,
        desktop_delivery_routes: &BTreeMap<PointerEdgeSequence, DesktopRouteValidation>,
    ) -> Result<(), EngineError> {
        if snapshot.provider() != provider {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "pointer presentation snapshot belongs to another provider",
            });
        }
        let receipts = receipts
            .receipts()
            .iter()
            .map(|receipt| (receipt.candidate().sequence(), receipt.observation()))
            .collect::<BTreeMap<_, _>>();
        for edge in journal.edges() {
            let Some(observation) = receipts.get(&edge.sequence()).copied() else {
                return Err(EngineError::ReductionCauseInvariant {
                    detail: "validated pointer receipt is absent for a journal edge",
                });
            };
            let desktop_hover_route = desktop_hover_routes.get(&edge.sequence()).copied();
            let desktop_delivery_route = desktop_delivery_routes.get(&edge.sequence()).copied();
            let PointerReceiverObservation::Presented(presented) = observation else {
                continue;
            };
            for probe in presented.probes() {
                match probe {
                    PointerReceiverProbeReceipt::Delivery(delivery) => {
                        for (lane, disposition) in [
                            (PresentationPointerLane::Click, delivery.click()),
                            (PresentationPointerLane::Drag, delivery.drag()),
                            (PresentationPointerLane::Scroll, delivery.scroll()),
                        ] {
                            let desktop_route = if lane == PresentationPointerLane::Scroll {
                                desktop_delivery_route
                            } else {
                                desktop_hover_route
                            };
                            if lane == PresentationPointerLane::Scroll
                                && matches!(edge.kind(), PointerEdgeKind::Scrolled(scroll)
                                    if matches!(scroll.phase(), crate::pointer_journal::ScrollPhase::Discrete | crate::pointer_journal::ScrollPhase::Begin))
                                && matches!(
                                    disposition,
                                    PointerReceiverDeliveryDisposition::NoReceiver
                                        | PointerReceiverDeliveryDisposition::DockCanvas
                                )
                            {
                                self.validate_pointer_receiver_absence(
                                    provider,
                                    edge,
                                    desktop_route,
                                    lane,
                                    delivery.output(),
                                    delivery.authority(),
                                    snapshot,
                                )?;
                            }
                            let PointerReceiverDeliveryDisposition::Dock(region) = disposition
                            else {
                                continue;
                            };
                            // A smooth continuation may have no event-time local point.
                            // Its exact locked owner is validated in provider order by
                            // the scroll FSM, inside the rollback candidate.
                            if lane == PresentationPointerLane::Scroll
                                && matches!(edge.kind(), PointerEdgeKind::Scrolled(scroll)
                                    if !matches!(scroll.phase(), crate::pointer_journal::ScrollPhase::Discrete | crate::pointer_journal::ScrollPhase::Begin))
                                && Self::journal_logical_point(edge, desktop_route).is_none()
                            {
                                continue;
                            }
                            self.validate_pointer_receiver_region(
                                provider,
                                edge,
                                desktop_route,
                                region,
                                Some(lane),
                                delivery.output(),
                                delivery.authority(),
                                snapshot,
                            )?;
                        }
                    }
                    PointerReceiverProbeReceipt::HoverHit(hover) => {
                        let PointerReceiverHoverHitDisposition::Dock(region) = hover.disposition()
                        else {
                            continue;
                        };
                        self.validate_pointer_receiver_region(
                            provider,
                            edge,
                            desktop_hover_route,
                            region,
                            Some(PresentationPointerLane::HoverDrop),
                            hover.output(),
                            hover.authority(),
                            snapshot,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_pointer_receiver_absence(
        &self,
        provider: PointerInputLease,
        edge: &PointerEdge,
        desktop_route: Option<DesktopRouteValidation>,
        lane: PresentationPointerLane,
        output: Option<SurfacePresentationOutputTicket>,
        authority: Option<PresentedSurfaceAuthority>,
        snapshot: &JournalPresentationSnapshot,
    ) -> Result<(), EngineError> {
        let (surface, point, desktop_route) = match (provider.scope(), edge.location()) {
            (
                PointerProviderScope::SurfaceLocal(scope),
                PointerEdgeLocation::SurfaceLocal {
                    position: Authority::Known(point),
                },
            ) => (scope.surface(), point, None),
            (PointerProviderScope::DesktopGlobal, PointerEdgeLocation::Desktop { .. }) => {
                let Some(route) = desktop_route.and_then(DesktopRouteValidation::dock_route) else {
                    return Err(EngineError::PointerReceiverGeometry {
                        source: PointerReceiverGeometryError::LogicalPointUnavailable {
                            sequence: edge.sequence(),
                        },
                    });
                };
                (
                    route.binding().surface(),
                    route.surface_position(),
                    Some(route),
                )
            }
            _ => {
                return Err(EngineError::PointerReceiverGeometry {
                    source: PointerReceiverGeometryError::LogicalPointUnavailable {
                        sequence: edge.sequence(),
                    },
                });
            }
        };
        let (Some(output), Some(authority)) = (output, authority) else {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "known scroll-receiver absence has no atomic presentation binding",
            });
        };
        if let Some(route) = desktop_route {
            route
                .validate_presented_authority(authority)
                .map_err(|source| EngineError::PointerReceiverGeometry {
                    source: PointerReceiverGeometryError::DesktopRoutePresentationMismatch {
                        sequence: edge.sequence(),
                        source,
                    },
                })?;
        }
        let presentation = snapshot.presentation(output, authority).map_err(|source| {
            EngineError::JournalPresentationSnapshot {
                detail: source.to_string(),
            }
        })?;
        if presentation.surface() != surface {
            return Err(EngineError::PointerReceiverGeometry {
                source: PointerReceiverGeometryError::AbsenceSurfaceMismatch {
                    sequence: edge.sequence(),
                    expected: surface,
                    actual: presentation.surface(),
                },
            });
        }
        let winner = presentation
            .hit_manifest()
            .resolve_exclusive(lane, point)
            .map_err(|_| EngineError::PointerReceiverGeometry {
                source: PointerReceiverGeometryError::ReceiverWinnerAmbiguous {
                    sequence: edge.sequence(),
                    lane,
                },
            })?
            .map(|winner| winner.id());
        if winner.is_some() {
            return Err(EngineError::PointerReceiverGeometry {
                source: PointerReceiverGeometryError::ReceiverWinnerMismatch {
                    sequence: edge.sequence(),
                    lane,
                    claimed: None,
                    winner,
                },
            });
        }
        Ok(())
    }

    fn validate_pointer_receiver_region(
        &self,
        provider: PointerInputLease,
        edge: &PointerEdge,
        desktop_route: Option<DesktopRouteValidation>,
        region: PresentationHitRegionId,
        delivery_lane: Option<PresentationPointerLane>,
        output: Option<SurfacePresentationOutputTicket>,
        authority: Option<PresentedSurfaceAuthority>,
        snapshot: &JournalPresentationSnapshot,
    ) -> Result<(), EngineError> {
        let (surface, point, desktop_route) = match (provider.scope(), edge.location()) {
            (
                PointerProviderScope::SurfaceLocal(scope),
                PointerEdgeLocation::SurfaceLocal {
                    position: Authority::Known(point),
                },
            ) => (scope.surface(), point, None),
            (PointerProviderScope::DesktopGlobal, PointerEdgeLocation::Desktop { .. }) => {
                let Some(route) = desktop_route.and_then(DesktopRouteValidation::dock_route) else {
                    return Err(EngineError::PointerReceiverGeometry {
                        source: PointerReceiverGeometryError::LogicalPointUnavailable {
                            sequence: edge.sequence(),
                        },
                    });
                };
                (
                    route.binding().surface(),
                    route.surface_position(),
                    Some(route),
                )
            }
            _ => {
                return Err(EngineError::PointerReceiverGeometry {
                    source: PointerReceiverGeometryError::LogicalPointUnavailable {
                        sequence: edge.sequence(),
                    },
                });
            }
        };
        if region.surface() != surface {
            return Err(EngineError::PointerReceiverGeometry {
                source: PointerReceiverGeometryError::SurfaceMismatch {
                    sequence: edge.sequence(),
                    expected: surface,
                    actual: region.surface(),
                    region,
                },
            });
        }
        let (Some(output), Some(authority)) = (output, authority) else {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "known docking receiver has no atomic presentation binding",
            });
        };
        if let Some(route) = desktop_route {
            route
                .validate_presented_authority(authority)
                .map_err(|source| EngineError::PointerReceiverGeometry {
                    source: PointerReceiverGeometryError::DesktopRoutePresentationMismatch {
                        sequence: edge.sequence(),
                        source,
                    },
                })?;
        }
        let presentation = snapshot.presentation(output, authority).map_err(|source| {
            EngineError::JournalPresentationSnapshot {
                detail: source.to_string(),
            }
        })?;
        if presentation.surface() != surface {
            return Err(EngineError::PointerReceiverGeometry {
                source: PointerReceiverGeometryError::SurfaceMismatch {
                    sequence: edge.sequence(),
                    expected: surface,
                    actual: presentation.surface(),
                    region,
                },
            });
        }
        let manifest = presentation.hit_manifest();
        let region_record =
            manifest
                .region(region)
                .ok_or(EngineError::PointerReceiverGeometry {
                    source: PointerReceiverGeometryError::InteractionManifestUnavailable {
                        sequence: edge.sequence(),
                        surface,
                    },
                })?;
        if !region_record.hit().contains(point) {
            return Err(EngineError::PointerReceiverGeometry {
                source: PointerReceiverGeometryError::RegionDoesNotCoverPoint {
                    sequence: edge.sequence(),
                    region,
                },
            });
        }
        let Some(lane) = delivery_lane else {
            return Ok(());
        };
        let winner = match manifest.resolve_exclusive(lane, point) {
            Ok(winner) => winner.map(|winner| winner.id()),
            Err(_) => {
                return Err(EngineError::PointerReceiverGeometry {
                    source: PointerReceiverGeometryError::ReceiverWinnerAmbiguous {
                        sequence: edge.sequence(),
                        lane,
                    },
                });
            }
        };
        if winner != Some(region) {
            return Err(EngineError::PointerReceiverGeometry {
                source: PointerReceiverGeometryError::ReceiverWinnerMismatch {
                    sequence: edge.sequence(),
                    lane,
                    claimed: Some(region),
                    winner,
                },
            });
        }
        Ok(())
    }

    pub(super) fn journal_logical_point(
        edge: &PointerEdge,
        desktop_route: Option<DesktopRouteValidation>,
    ) -> Option<crate::geometry::LogicalPoint> {
        match edge.location() {
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(point),
            } => Some(point),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Unknown(_),
            } => None,
            PointerEdgeLocation::Desktop { .. } => desktop_route
                .and_then(DesktopRouteValidation::dock_route)
                .map(|route| route.surface_position()),
        }
    }

    pub(super) const fn journal_route_preview_status(
        edge: &PointerEdge,
        desktop_route: Option<DesktopRouteValidation>,
    ) -> PreviewResolutionStatus {
        match (edge.location(), desktop_route) {
            (
                PointerEdgeLocation::Desktop { .. },
                Some(DesktopRouteValidation::Known(ValidatedDesktopRoute::Foreign { .. })),
            ) => PreviewResolutionStatus::OpaqueBlocker,
            (
                PointerEdgeLocation::Desktop { .. },
                Some(DesktopRouteValidation::Known(ValidatedDesktopRoute::NoWindow { .. })),
            ) => PreviewResolutionStatus::KnownNone,
            _ => PreviewResolutionStatus::UnknownAuthority,
        }
    }

    pub(super) fn validate_journal_desktop_presentation(
        edge: &PointerEdge,
        desktop_route: Option<DesktopRouteValidation>,
        authority: PresentedSurfaceAuthority,
    ) -> Result<(), EngineError> {
        if !matches!(edge.location(), PointerEdgeLocation::Desktop { .. }) {
            return Ok(());
        }
        let Some(route) = desktop_route.and_then(DesktopRouteValidation::dock_route) else {
            return Err(EngineError::PointerReceiverGeometry {
                source: PointerReceiverGeometryError::LogicalPointUnavailable {
                    sequence: edge.sequence(),
                },
            });
        };
        route
            .validate_presented_authority(authority)
            .map_err(|source| EngineError::PointerReceiverGeometry {
                source: PointerReceiverGeometryError::DesktopRoutePresentationMismatch {
                    sequence: edge.sequence(),
                    source,
                },
            })
    }

    fn journal_delivery_region_for_lane<'snapshot>(
        cause: ReductionCause,
        receipt: &ValidatedPointerReceiverReceipt,
        snapshot: &'snapshot JournalPresentationSnapshot,
        lane: PresentationPointerLane,
    ) -> Result<
        Option<(
            PresentationHitRegionId,
            &'snapshot JournalSurfacePresentation,
        )>,
        EngineError,
    > {
        let PointerReceiverObservation::Presented(observation) = receipt.observation() else {
            return Ok(None);
        };
        let Some(delivery) = observation.probes().iter().find_map(|probe| match probe {
            PointerReceiverProbeReceipt::Delivery(delivery) => Some(delivery),
            PointerReceiverProbeReceipt::HoverHit(_) => None,
        }) else {
            return Err(EngineError::PointerInteractionInvariant {
                cause,
                detail: "primary press receipt has no delivery probe".to_owned(),
            });
        };
        let disposition = match lane {
            PresentationPointerLane::Click => delivery.click(),
            PresentationPointerLane::Drag => delivery.drag(),
            PresentationPointerLane::Scroll => delivery.scroll(),
            PresentationPointerLane::HoverDrop => {
                return Err(EngineError::PointerInteractionInvariant {
                    cause,
                    detail: "hover-drop lane cannot be read from a delivery probe".to_owned(),
                });
            }
        };
        let PointerReceiverDeliveryDisposition::Dock(region) = disposition else {
            return Ok(None);
        };
        let (Some(output), Some(authority)) = (delivery.output(), delivery.authority()) else {
            return Err(EngineError::PointerInteractionInvariant {
                cause,
                detail: "known delivery has no atomic presentation binding".to_owned(),
            });
        };
        let presentation = snapshot.presentation(output, authority).map_err(|source| {
            EngineError::PointerInteractionInvariant {
                cause,
                detail: source.to_string(),
            }
        })?;
        Ok(Some((region, presentation)))
    }

    pub(super) fn journal_semantic_press_delivery<'snapshot>(
        cause: ReductionCause,
        receipt: &ValidatedPointerReceiverReceipt,
        snapshot: &'snapshot JournalPresentationSnapshot,
    ) -> Result<
        Option<(
            PresentationHitRegionId,
            &'snapshot JournalSurfacePresentation,
        )>,
        EngineError,
    > {
        let click = Self::journal_delivery_region_for_lane(
            cause,
            receipt,
            snapshot,
            PresentationPointerLane::Click,
        )?
        .filter(|(region, _)| {
            delivery_action_lane(region.kind()) == Some(PresentationPointerLane::Click)
        });
        let drag = Self::journal_delivery_region_for_lane(
            cause,
            receipt,
            snapshot,
            PresentationPointerLane::Drag,
        )?
        .filter(|(region, _)| {
            delivery_action_lane(region.kind()) == Some(PresentationPointerLane::Drag)
        });
        match (click, drag) {
            (Some(_), Some(_)) => Err(EngineError::PointerInteractionInvariant {
                cause,
                detail: "one press resolved to different executable click and drag receivers"
                    .to_owned(),
            }),
            (Some(delivery), None) | (None, Some(delivery)) => Ok(Some(delivery)),
            (None, None) => Ok(None),
        }
    }

    pub(super) fn journal_click_delivery<'snapshot>(
        cause: ReductionCause,
        edge: &PointerEdge,
        receipt: &ValidatedPointerReceiverReceipt,
        snapshot: &'snapshot JournalPresentationSnapshot,
        desktop_route: Option<DesktopRouteValidation>,
    ) -> Result<JournalClickDelivery<'snapshot>, EngineError> {
        let observation = match receipt.observation() {
            PointerReceiverObservation::Unknown(_) => return Ok(JournalClickDelivery::Unknown),
            PointerReceiverObservation::NotApplicable => {
                return match edge.location() {
                    PointerEdgeLocation::Desktop { .. } => match desktop_route {
                        Some(DesktopRouteValidation::Known(ValidatedDesktopRoute::NoWindow {
                            ..
                        })) => Ok(JournalClickDelivery::KnownMismatch),
                        Some(DesktopRouteValidation::Known(ValidatedDesktopRoute::Foreign {
                            ..
                        })) => Ok(JournalClickDelivery::Blocked),
                        Some(DesktopRouteValidation::Known(ValidatedDesktopRoute::Dock(_)))
                        | Some(DesktopRouteValidation::Unavailable(_))
                        | None => Ok(JournalClickDelivery::Unknown),
                    },
                    PointerEdgeLocation::SurfaceLocal { .. } => {
                        Err(EngineError::PointerInteractionInvariant {
                            cause,
                            detail: "surface-local primary release receipt has no delivery role"
                                .to_owned(),
                        })
                    }
                };
            }
            PointerReceiverObservation::Presented(observation) => observation,
        };
        let Some(delivery) = observation.probes().iter().find_map(|probe| match probe {
            PointerReceiverProbeReceipt::Delivery(delivery) => Some(delivery),
            PointerReceiverProbeReceipt::HoverHit(_) => None,
        }) else {
            return Err(EngineError::PointerInteractionInvariant {
                cause,
                detail: "primary release receipt has no delivery probe".to_owned(),
            });
        };
        match delivery.click() {
            PointerReceiverDeliveryDisposition::Dock(region) => {
                let (Some(output), Some(authority)) = (delivery.output(), delivery.authority())
                else {
                    return Err(EngineError::PointerInteractionInvariant {
                        cause,
                        detail: "known click delivery has no atomic presentation binding"
                            .to_owned(),
                    });
                };
                let presentation = snapshot.presentation(output, authority).map_err(|source| {
                    EngineError::PointerInteractionInvariant {
                        cause,
                        detail: source.to_string(),
                    }
                })?;
                Ok(JournalClickDelivery::Dock(region, presentation))
            }
            PointerReceiverDeliveryDisposition::DockCanvas
            | PointerReceiverDeliveryDisposition::NoReceiver => {
                Ok(JournalClickDelivery::KnownMismatch)
            }
            PointerReceiverDeliveryDisposition::Blocked => Ok(JournalClickDelivery::Blocked),
            PointerReceiverDeliveryDisposition::Unknown(_) => Ok(JournalClickDelivery::Unknown),
        }
    }
}

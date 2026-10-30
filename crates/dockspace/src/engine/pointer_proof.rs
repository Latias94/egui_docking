//! Pointer receiver and presentation authority validation.

use super::*;
use crate::pointer_journal::{DesktopDockRoute, HostLocalPointerRoute, PointerDeviceKind};
use crate::pointer_receiver::PointerReceiverDeliveryRequest;
use crate::presentation_hit::PresentationHitSuppression;

fn owned_host_local_route(
    engine: &DockEngine,
    route: Authority<HostLocalPointerRoute>,
) -> Option<HostLocalPointerRoute> {
    let route = route.known().copied()?;
    let host = engine.pointer_provider()?.scope().host_local()?;
    let authority = route.authority();
    (authority.endpoint() == HostPresentationEndpoint::Headless
        && engine
            .presentation_authority
            .presentation
            .active_surface_scope(route.surface())
            == Some((authority.stream(), host, HostPresentationEndpoint::Headless)))
    .then_some(route)
}

fn validated_host_local_route(
    engine: &DockEngine,
    route: Authority<HostLocalPointerRoute>,
) -> Option<HostLocalPointerRoute> {
    let route = owned_host_local_route(engine, route)?;
    engine
        .interaction_projection(route.surface())
        .is_some_and(|projection| projection.authority() == route.authority())
        .then_some(route)
}

fn native_local_route_matches_authority(
    route: NativeLocalPointerRoute,
    authority: PresentedSurfaceAuthority,
) -> bool {
    authority.surface() == route.binding().surface()
        && authority.binding() == Some(route.binding())
        && authority.coordinate_generation() == route.coordinate_generation()
}

pub(super) fn validated_native_local_route(
    engine: &DockEngine,
    edge: &PointerEdge,
) -> Option<NativeLocalPointerRoute> {
    let PointerEdgeLocation::NativeLocal {
        route: Authority::Known(route),
    } = edge.location()
    else {
        return None;
    };
    let PointerProviderScope::NativeJoined(host) = engine.pointer_provider()?.scope() else {
        return None;
    };
    if let Some(owner) = engine.interaction.active_owner()
        && owner.pointer() == edge.pointer()
        && !matches!(edge.kind(), PointerEdgeKind::Scrolled(_))
        && !engine.journal_edge_matches_gesture_lane(owner, edge)
    {
        return None;
    }
    let coordinates = engine.native_local_coordinate_snapshot(host, route.binding())?;
    if coordinates.coordinate_generation() != route.coordinate_generation() {
        return None;
    }
    let expected = LogicalPoint::new(
        route.client_position().x() / coordinates.presentation_scale_factor().get(),
        route.client_position().y() / coordinates.presentation_scale_factor().get(),
    )
    .ok()?;
    (expected == route.surface_position()).then_some(route)
}

fn validated_native_local_hover(
    engine: &DockEngine,
    edge: &PointerEdge,
) -> Option<NativeLocalPointerRoute> {
    let route = validated_native_local_route(engine, edge)?;
    let projection = engine.interaction_projection(route.binding().surface())?;
    (native_local_route_matches_authority(route, projection.authority())
        && projection
            .plan()
            .bounds()
            .contains(route.surface_position()))
    .then_some(route)
}

pub(super) fn pointer_receiver_candidate_spec(
    engine: &DockEngine,
    edge: &PointerEdge,
    stream: PointerStreamId,
) -> PointerReceiverCandidateSpec {
    if let Some(owner) = engine.interaction.active_owner()
        && owner.stream() == stream
        && !engine.journal_edge_matches_gesture_lane(owner, edge)
        && !matches!(edge.kind(), PointerEdgeKind::Scrolled(_))
    {
        return PointerReceiverCandidateSpec::not_applicable(edge.sequence());
    }
    if matches!(edge.kind(), PointerEdgeKind::Scrolled(_)) {
        let PointerEdgeKind::Scrolled(scroll) = edge.kind() else {
            unreachable!("the branch is restricted to scroll edges");
        };
        let challenge = engine.scroll_receiver_challenge(stream, scroll);
        let (delivery_surface, delivery_point) = match challenge {
            ScrollReceiverChallenge::Locked {
                receiver,
                probe_point,
                ..
            } => (Some(receiver.surface()), Some(probe_point)),
            _ => match edge.location() {
                PointerEdgeLocation::SurfaceLocal {
                    position: Authority::Known(point),
                } => (
                    stream
                        .lease()
                        .scope()
                        .surface_local()
                        .map(|scope| scope.surface()),
                    Some(point),
                ),
                PointerEdgeLocation::SurfaceLocal {
                    position: Authority::Unknown(_),
                } => (
                    stream
                        .lease()
                        .scope()
                        .surface_local()
                        .map(|scope| scope.surface()),
                    None,
                ),
                PointerEdgeLocation::HostLocal { manipulation, .. } => {
                    let route = validated_host_local_route(engine, manipulation);
                    (
                        route.map(HostLocalPointerRoute::surface),
                        route.map(HostLocalPointerRoute::position),
                    )
                }
                PointerEdgeLocation::NativeLocal { .. } => {
                    let route = validated_native_local_route(engine, edge);
                    (
                        route.map(|route| route.binding().surface()),
                        route.map(NativeLocalPointerRoute::surface_position),
                    )
                }
                PointerEdgeLocation::NativeLocalToDesktop { .. } => (None, None),
                PointerEdgeLocation::Desktop { .. } => {
                    let route = validated_desktop_delivery_route(engine, edge)
                        .and_then(DesktopRouteValidation::dock_route);
                    let surface = match scroll.delivery() {
                        Authority::Known(endpoint) => Some(endpoint.surface()),
                        Authority::Unknown(_) => route.map(|route| route.binding().surface()),
                    };
                    (surface, route.map(DesktopDockRoute::surface_position))
                }
            },
        };
        return PointerReceiverCandidateSpec::scroll_delivery(
            edge.sequence(),
            delivery_surface,
            delivery_point,
            challenge,
        );
    }
    let owns_stream = engine.interaction.active_stream() == Some(stream);
    let position = journal_position_key(engine, stream, edge);
    let (delivery, hover) = match (engine.interaction.status(), edge.kind(), owns_stream) {
        (InteractionStatus::Idle, PointerEdgeKind::ButtonPressed(PointerButton::Primary), _) => {
            (PointerReceiverDeliveryRequest::ClickAndDrag, false)
        }
        (InteractionStatus::Pressed { session }, kind, true)
            if engine
                .interaction
                .active_click(session)
                .is_ok_and(|click| click.touch.is_some()) =>
        {
            match kind {
                PointerEdgeKind::Continuation(_)
                | PointerEdgeKind::ButtonReleased(PointerButton::Primary)
                | PointerEdgeKind::ContactEnded(PointerButton::Primary) => {
                    (PointerReceiverDeliveryRequest::ClickAndDrag, false)
                }
                PointerEdgeKind::Moved => {
                    let hover = engine.interaction.active_click(session).is_ok_and(|click| {
                        click.touch.as_ref().is_some_and(|touch| touch.armed)
                            && matches!(
                                click.region.kind(),
                                PresentationHitRegionKind::TabBody(_)
                                    | PresentationHitRegionKind::TabGroupDrag { .. }
                                    | PresentationHitRegionKind::ContainedTitle(_)
                            )
                    });
                    (
                        PointerReceiverDeliveryRequest::None,
                        hover && position.hover_probe_requested(),
                    )
                }
                _ => (PointerReceiverDeliveryRequest::None, false),
            }
        }
        (
            InteractionStatus::Pressed { .. },
            PointerEdgeKind::ButtonReleased(PointerButton::Primary)
            | PointerEdgeKind::ContactEnded(PointerButton::Primary),
            true,
        ) => (PointerReceiverDeliveryRequest::Click, false),
        (InteractionStatus::Armed { .. }, PointerEdgeKind::Moved, true) => (
            PointerReceiverDeliveryRequest::None,
            position.hover_probe_requested(),
        ),
        (
            InteractionStatus::Dragging { session },
            PointerEdgeKind::Moved | PointerEdgeKind::Continuation(_),
            true,
        ) => (
            PointerReceiverDeliveryRequest::None,
            engine
                .interaction
                .active_drag(session)
                .is_ok_and(|_| position.hover_probe_requested()),
        ),
        (
            InteractionStatus::Dragging { session },
            PointerEdgeKind::ButtonReleased(PointerButton::Primary)
            | PointerEdgeKind::ContactEnded(PointerButton::Primary),
            true,
        ) => {
            let hover = engine
                .interaction
                .active_drag(session)
                .is_ok_and(|_| position.hover_probe_requested());
            // Every active drag release is a fresh receiver-authority boundary,
            // including an unchanged point. An explicit outside-all desktop
            // route is the only release without a surface receiver to probe.
            (PointerReceiverDeliveryRequest::None, hover)
        }
        _ => (PointerReceiverDeliveryRequest::None, false),
    };
    if delivery != PointerReceiverDeliveryRequest::None {
        let (surface, point) = match edge.location() {
            PointerEdgeLocation::SurfaceLocal { position } => (
                stream
                    .lease()
                    .scope()
                    .surface_local()
                    .map(|scope| scope.surface()),
                position.known().copied(),
            ),
            PointerEdgeLocation::HostLocal { manipulation, .. } => {
                let route = validated_host_local_route(engine, manipulation);
                (
                    route.map(HostLocalPointerRoute::surface),
                    route.map(HostLocalPointerRoute::position),
                )
            }
            PointerEdgeLocation::NativeLocal { .. } => {
                let route = validated_native_local_hover(engine, edge);
                (
                    route.map(|route| route.binding().surface()),
                    route.map(NativeLocalPointerRoute::surface_position),
                )
            }
            PointerEdgeLocation::NativeLocalToDesktop { .. } => (None, None),
            PointerEdgeLocation::Desktop { .. } => {
                let route = validated_desktop_delivery_route(engine, edge)
                    .and_then(DesktopRouteValidation::dock_route);
                (
                    route.map(|route| route.binding().surface()),
                    route.map(DesktopDockRoute::surface_position),
                )
            }
        };
        return PointerReceiverCandidateSpec::delivery(edge.sequence(), surface, point, delivery);
    }
    if hover {
        let surface = position.hover_surface();
        let point = position.hover_probe().map(SurfacePointer::position);
        return PointerReceiverCandidateSpec::hover_hit(edge.sequence(), surface, point);
    }
    PointerReceiverCandidateSpec::not_applicable(edge.sequence())
}

pub(super) fn journal_position_key(
    engine: &DockEngine,
    stream: PointerStreamId,
    edge: &PointerEdge,
) -> ManipulationPositionKey {
    match edge.location() {
        PointerEdgeLocation::SurfaceLocal { position } => ManipulationPositionKey::JournalSurface {
            scope: stream.lease().scope(),
            position,
        },
        PointerEdgeLocation::HostLocal { hover, .. } => ManipulationPositionKey::JournalHost {
            manipulation: engine.journal_host_local_manipulation(edge),
            hover: validated_host_local_route(engine, hover),
        },
        PointerEdgeLocation::NativeLocal { .. } => ManipulationPositionKey::JournalNativeLocal {
            manipulation: validated_native_local_route(engine, edge),
            hover: validated_native_local_hover(engine, edge),
        },
        PointerEdgeLocation::NativeLocalToDesktop { .. } => {
            ManipulationPositionKey::JournalNativeLocal {
                manipulation: None,
                hover: None,
            }
        }
        PointerEdgeLocation::Desktop { route } => ManipulationPositionKey::JournalDesktop(
            route.validate_against_registry(engine.authority_domain, engine.viewport.registry()),
        ),
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
        | PresentationHitRegionKind::TabGroupDrag { .. }
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
    /// A native-local receiver belongs to another binding or coordinate generation.
    #[error(
        "pointer receipt for edge {sequence} does not match its native-local coordinate authority"
    )]
    NativeLocalPresentationMismatch {
        /// Exact provider edge carrying the receiver claim.
        sequence: crate::pointer_journal::PointerEdgeSequence,
    },
    /// A known host-local hover answer does not name the edge's exact route output.
    #[error(
        "pointer hover receipt for edge {sequence} names authority {actual:?}, expected its host-local route {expected:?}"
    )]
    HostLocalHoverPresentationMismatch {
        /// Exact provider sequence carrying the claim.
        sequence: PointerEdgeSequence,
        /// Exact authority supplied by the validated hover route.
        expected: PresentedSurfaceAuthority,
        /// Exact authority supplied by the receiver answer.
        actual: PresentedSurfaceAuthority,
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
    pub(super) fn journal_gesture_owner(
        &self,
        stream: PointerStreamId,
        edge: &PointerEdge,
        desktop_route: Option<DesktopRouteValidation>,
    ) -> Option<GestureOwner> {
        let lane = match (stream.lease().scope(), edge.location()) {
            (
                PointerProviderScope::DesktopGlobal | PointerProviderScope::NativeJoined(_),
                PointerEdgeLocation::Desktop { .. },
            ) => {
                desktop_route.and_then(DesktopRouteValidation::dock_route)?;
                GestureLane::Desktop
            }
            (PointerProviderScope::NativeJoined(_), PointerEdgeLocation::NativeLocal { .. }) => {
                let route = validated_native_local_route(self, edge)?;
                GestureLane::NativeLocal {
                    binding: route.binding(),
                    coordinate_generation: route.coordinate_generation(),
                }
            }
            (PointerProviderScope::HostLocal(_), PointerEdgeLocation::HostLocal { .. }) => {
                self.journal_host_local_manipulation(edge)?;
                GestureLane::HostLocal
            }
            (
                PointerProviderScope::SurfaceLocal(_),
                PointerEdgeLocation::SurfaceLocal {
                    position: Authority::Known(_),
                },
            ) => GestureLane::SurfaceLocal,
            _ => return None,
        };
        Some(GestureOwner::new(stream, lane))
    }

    pub(crate) fn native_local_coordinate_snapshot(
        &self,
        host: PresentationHostLease,
        binding: ViewportBinding,
    ) -> Option<CoordinateSnapshot> {
        let provider = self.backend_ingress_provider()?;
        if provider.presentation_host() != host
            || provider.pointer_provider().scope() != PointerProviderScope::NativeJoined(host)
        {
            return None;
        }
        let (_, owner, endpoint) = self
            .presentation_authority
            .presentation
            .active_surface_scope(binding.surface())?;
        if owner != host || endpoint != HostPresentationEndpoint::Native(binding) {
            return None;
        }
        let record = self.viewport.viewport(binding.surface())?;
        if record.binding() != binding || !record.is_routeable() {
            return None;
        }
        let coordinates = record.coordinates()?;
        (coordinates.content_size().width() > 0.0 && coordinates.content_size().height() > 0.0)
            .then_some(coordinates)
    }

    pub(super) fn journal_edge_matches_gesture_lane(
        &self,
        owner: GestureOwner,
        edge: &PointerEdge,
    ) -> bool {
        match (owner.lane(), edge.location()) {
            (GestureLane::Desktop, PointerEdgeLocation::Desktop { .. })
            | (GestureLane::HostLocal, PointerEdgeLocation::HostLocal { .. })
            | (GestureLane::SurfaceLocal, PointerEdgeLocation::SurfaceLocal { .. }) => true,
            (
                GestureLane::NativeLocal {
                    binding,
                    coordinate_generation,
                },
                PointerEdgeLocation::NativeLocal {
                    route: Authority::Known(route),
                },
            ) => {
                route.binding() == binding && route.coordinate_generation() == coordinate_generation
            }
            _ => false,
        }
    }

    pub(super) fn validate_journal_native_local_presentation(
        &self,
        edge: &PointerEdge,
        authority: PresentedSurfaceAuthority,
    ) -> Result<(), EngineError> {
        if !matches!(edge.location(), PointerEdgeLocation::NativeLocal { .. }) {
            return Ok(());
        }
        if validated_native_local_hover(self, edge)
            .is_some_and(|route| native_local_route_matches_authority(route, authority))
        {
            return Ok(());
        }
        Err(EngineError::PointerReceiverGeometry {
            source: PointerReceiverGeometryError::NativeLocalPresentationMismatch {
                sequence: edge.sequence(),
            },
        })
    }

    pub(super) fn journal_pointer_hit_device(edge: &PointerEdge) -> PointerDeviceKind {
        match edge.location() {
            PointerEdgeLocation::SurfaceLocal { .. }
            | PointerEdgeLocation::HostLocal { .. }
            | PointerEdgeLocation::NativeLocal { .. }
            | PointerEdgeLocation::NativeLocalToDesktop { .. } => edge.metadata().device(),
            PointerEdgeLocation::Desktop { .. } => PointerDeviceKind::Unknown,
        }
    }
    pub(crate) fn hover_drop_suppression(&self) -> PresentationHitSuppression {
        let source = match self.interaction.status() {
            InteractionStatus::Armed { session } => self
                .interaction
                .armed_drag(session)
                .ok()
                .and_then(|drag| drag.complete_root.as_ref())
                .map(NodeSource::root),
            InteractionStatus::Dragging { session } => self
                .interaction
                .active_drag(session)
                .ok()
                .and_then(|drag| drag.complete_root.as_ref())
                .map(NodeSource::root),
            InteractionStatus::Pressed { session } => self
                .interaction
                .active_click(session)
                .ok()
                .filter(|click| click.touch.as_ref().is_some_and(|touch| touch.armed))
                .and_then(|click| self.touch_complete_drag_root(click.region.kind())),
            _ => None,
        };
        let Some(source) = source else {
            return PresentationHitSuppression::default();
        };
        let contained = match self.workspace.presentation_for_root(source) {
            Some(crate::RootPresentationOwner::Contained { floating, .. }) => Some(floating),
            Some(crate::RootPresentationOwner::Main { .. }) => None,
            None => return PresentationHitSuppression::default(),
        };
        PresentationHitSuppression::complete_root(source, contained)
    }

    pub(super) fn validate_pointer_receiver_geometry(
        &self,
        provider: PointerInputLease,
        edges: &[PointerEdge],
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
            .map(|receipt| {
                (
                    receipt.candidate().sequence(),
                    (receipt.observation(), receipt.delivery_request()),
                )
            })
            .collect::<BTreeMap<_, _>>();
        for edge in edges {
            let Some((observation, delivery_request)) = receipts.get(&edge.sequence()).copied()
            else {
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
                            let desktop_route = desktop_delivery_route;
                            let requested = matches!(
                                (delivery_request, lane),
                                (
                                    PointerReceiverDeliveryRequest::Click
                                        | PointerReceiverDeliveryRequest::ClickAndDrag,
                                    PresentationPointerLane::Click
                                ) | (
                                    PointerReceiverDeliveryRequest::ClickAndDrag,
                                    PresentationPointerLane::Drag
                                )
                            );
                            let explicit_absence = requested
                                && disposition == PointerReceiverDeliveryDisposition::NoReceiver;
                            let scroll_absence = lane == PresentationPointerLane::Scroll
                                && matches!(edge.kind(), PointerEdgeKind::Scrolled(scroll)
                                    if matches!(scroll.phase(), crate::pointer_journal::ScrollPhase::Discrete | crate::pointer_journal::ScrollPhase::Begin))
                                && matches!(
                                    disposition,
                                    PointerReceiverDeliveryDisposition::NoReceiver
                                        | PointerReceiverDeliveryDisposition::DockCanvas
                                );
                            if explicit_absence || scroll_absence {
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
                                && self.journal_logical_point(edge, desktop_route).is_none()
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
                        if matches!(edge.location(), PointerEdgeLocation::NativeLocal { .. })
                            && let Some(authority) = hover.authority()
                        {
                            self.validate_journal_native_local_presentation(edge, authority)?;
                        }
                        if let PointerEdgeLocation::HostLocal { hover: route, .. } = edge.location()
                            && let Some(actual) = hover.authority()
                        {
                            let route = validated_host_local_route(self, route).ok_or(
                                EngineError::PointerReceiverGeometry {
                                    source: PointerReceiverGeometryError::LogicalPointUnavailable {
                                        sequence: edge.sequence(),
                                    },
                                },
                            )?;
                            if actual != route.authority() {
                                return Err(EngineError::PointerReceiverGeometry {
                                    source: PointerReceiverGeometryError::HostLocalHoverPresentationMismatch {
                                        sequence: edge.sequence(),
                                        expected: route.authority(),
                                        actual,
                                    },
                                });
                            }
                        }
                        if hover.disposition() == PointerReceiverHoverHitDisposition::NoReceiver {
                            self.validate_pointer_receiver_absence(
                                provider,
                                edge,
                                desktop_hover_route,
                                PresentationPointerLane::HoverDrop,
                                hover.output(),
                                hover.authority(),
                                snapshot,
                            )?;
                        }
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
            (
                PointerProviderScope::HostLocal(_),
                PointerEdgeLocation::HostLocal {
                    manipulation,
                    hover,
                },
            ) => {
                let route = if lane == PresentationPointerLane::HoverDrop {
                    hover
                } else {
                    manipulation
                };
                let route = validated_host_local_route(self, route)
                    .filter(|route| Some(route.authority()) == authority)
                    .ok_or(EngineError::PointerReceiverGeometry {
                        source: PointerReceiverGeometryError::LogicalPointUnavailable {
                            sequence: edge.sequence(),
                        },
                    })?;
                (route.surface(), route.position(), None)
            }
            (PointerProviderScope::NativeJoined(_), PointerEdgeLocation::NativeLocal { .. }) => {
                let authority = authority.ok_or(EngineError::PointerReceiverGeometry {
                    source: PointerReceiverGeometryError::LogicalPointUnavailable {
                        sequence: edge.sequence(),
                    },
                })?;
                self.validate_journal_native_local_presentation(edge, authority)?;
                let route = validated_native_local_route(self, edge).ok_or(
                    EngineError::PointerReceiverGeometry {
                        source: PointerReceiverGeometryError::LogicalPointUnavailable {
                            sequence: edge.sequence(),
                        },
                    },
                )?;
                (route.binding().surface(), route.surface_position(), None)
            }
            (
                PointerProviderScope::DesktopGlobal | PointerProviderScope::NativeJoined(_),
                PointerEdgeLocation::Desktop { .. },
            ) => {
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
                detail: "known receiver absence has no atomic presentation binding",
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
        let suppression = if lane == PresentationPointerLane::HoverDrop {
            self.hover_drop_suppression()
        } else {
            PresentationHitSuppression::default()
        };
        let winner = presentation
            .hit_manifest()
            .resolve_exclusive_for_device(
                lane,
                point,
                suppression,
                Self::journal_pointer_hit_device(edge),
            )
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
            (
                PointerProviderScope::HostLocal(_),
                PointerEdgeLocation::HostLocal {
                    manipulation,
                    hover,
                },
            ) => {
                let route = if delivery_lane == Some(PresentationPointerLane::HoverDrop) {
                    hover
                } else {
                    manipulation
                };
                let route = validated_host_local_route(self, route)
                    .filter(|route| Some(route.authority()) == authority)
                    .ok_or(EngineError::PointerReceiverGeometry {
                        source: PointerReceiverGeometryError::LogicalPointUnavailable {
                            sequence: edge.sequence(),
                        },
                    })?;
                (route.surface(), route.position(), None)
            }
            (PointerProviderScope::NativeJoined(_), PointerEdgeLocation::NativeLocal { .. }) => {
                let authority = authority.ok_or(EngineError::PointerReceiverGeometry {
                    source: PointerReceiverGeometryError::LogicalPointUnavailable {
                        sequence: edge.sequence(),
                    },
                })?;
                self.validate_journal_native_local_presentation(edge, authority)?;
                let route = validated_native_local_route(self, edge).ok_or(
                    EngineError::PointerReceiverGeometry {
                        source: PointerReceiverGeometryError::LogicalPointUnavailable {
                            sequence: edge.sequence(),
                        },
                    },
                )?;
                (route.binding().surface(), route.surface_position(), None)
            }
            (
                PointerProviderScope::DesktopGlobal | PointerProviderScope::NativeJoined(_),
                PointerEdgeLocation::Desktop { .. },
            ) => {
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
        if !manifest.region_contains_for_device(
            region_record.id(),
            point,
            Self::journal_pointer_hit_device(edge),
        ) {
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
        let suppression = if lane == PresentationPointerLane::HoverDrop {
            self.hover_drop_suppression()
        } else {
            PresentationHitSuppression::default()
        };
        let winner = match manifest.resolve_exclusive_for_device(
            lane,
            point,
            suppression,
            Self::journal_pointer_hit_device(edge),
        ) {
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

    pub(super) fn journal_host_local_manipulation(
        &self,
        edge: &PointerEdge,
    ) -> Option<HostLocalPointerRoute> {
        let PointerEdgeLocation::HostLocal { manipulation, .. } = edge.location() else {
            return None;
        };
        if let Some(route) = validated_host_local_route(self, manipulation) {
            return Some(route);
        }
        let route = owned_host_local_route(self, manipulation)?;
        let stream = self.interaction.active_stream()?;
        if Some(stream.lease()) != self.pointer_provider() || stream.pointer() != edge.pointer() {
            return None;
        }
        // Selection, raising and live resizing may invalidate their own source
        // scene. Only the exact active owner can retain that source coordinate
        // proof; hover and new delivery claims always require current output.
        let frozen = self.interaction.active_presentation_authority()?;
        if frozen.presented != route.authority() {
            return None;
        }
        (self.resize_continuation_is_current(frozen)
            || self
                .interaction
                .active_scene_gesture_continuation()
                .is_some_and(|continuation| {
                    continuation.draft.origin == frozen
                        && self.scene_gesture_continuation_is_current(continuation)
                }))
        .then_some(route)
    }

    pub(super) fn journal_logical_point(
        &self,
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
            PointerEdgeLocation::HostLocal { .. } => self
                .journal_host_local_manipulation(edge)
                .map(HostLocalPointerRoute::position),
            PointerEdgeLocation::NativeLocal { .. } => validated_native_local_route(self, edge)
                .map(NativeLocalPointerRoute::surface_position),
            PointerEdgeLocation::NativeLocalToDesktop { .. } => None,
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
            (Some((click, _)), Some((drag, _))) => Err(EngineError::PointerInteractionInvariant {
                cause,
                detail: format!(
                    "one press resolved to different executable click {click:?} and drag {drag:?} receivers"
                ),
            }),
            (Some(delivery), None) | (None, Some(delivery)) => Ok(Some(delivery)),
            (None, None) => Self::journal_delivery_region_for_lane(
                cause,
                receipt,
                snapshot,
                PresentationPointerLane::Click,
            )
            .map(|delivery| {
                delivery.filter(|(region, _)| {
                    matches!(
                        region.kind(),
                        PresentationHitRegionKind::PaneBody(_)
                            | PresentationHitRegionKind::ContainedFrameBlocker(_)
                    )
                })
            }),
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
                        Some(
                            DesktopRouteValidation::Known(ValidatedDesktopRoute::Dock(_))
                            | DesktopRouteValidation::Unavailable(_),
                        )
                        | None => Ok(JournalClickDelivery::Unknown),
                    },
                    PointerEdgeLocation::NativeLocal { .. }
                    | PointerEdgeLocation::NativeLocalToDesktop { .. } => {
                        Ok(JournalClickDelivery::Unknown)
                    }
                    PointerEdgeLocation::SurfaceLocal { .. }
                    | PointerEdgeLocation::HostLocal { .. } => {
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
            PointerReceiverDeliveryDisposition::DockCanvas => {
                Ok(JournalClickDelivery::KnownMismatch)
            }
            PointerReceiverDeliveryDisposition::NoReceiver => match edge.location() {
                PointerEdgeLocation::Desktop { .. } => match desktop_route {
                    Some(DesktopRouteValidation::Known(ValidatedDesktopRoute::Foreign {
                        ..
                    })) => Ok(JournalClickDelivery::Blocked),
                    Some(DesktopRouteValidation::Unavailable(_)) | None => {
                        Ok(JournalClickDelivery::Unknown)
                    }
                    Some(DesktopRouteValidation::Known(
                        ValidatedDesktopRoute::Dock(_) | ValidatedDesktopRoute::NoWindow { .. },
                    )) => Ok(JournalClickDelivery::KnownMismatch),
                },
                PointerEdgeLocation::NativeLocal { .. }
                | PointerEdgeLocation::NativeLocalToDesktop { .. }
                | PointerEdgeLocation::SurfaceLocal { .. }
                | PointerEdgeLocation::HostLocal { .. } => Ok(JournalClickDelivery::KnownMismatch),
            },
            PointerReceiverDeliveryDisposition::Blocked => Ok(JournalClickDelivery::Blocked),
            PointerReceiverDeliveryDisposition::Unknown(_) => Ok(JournalClickDelivery::Unknown),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{TestPresentationHost, publish_surface};
    use super::*;
    use crate::graph::{RootRecord, SurfacePresentation};
    use crate::intent::PointerId;
    use crate::pointer_journal::HostLocalPointerRoute;
    use crate::pointer_receiver::PresentedPointerReceiverObservation;

    #[test]
    fn host_local_known_empty_and_blocked_hover_require_the_exact_route_emission() {
        let surface = SurfaceId::new(1);
        let root = RootId::new(1);
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
        builder.set_root(root, RootRecord::new(tabs).with_central(tabs));
        builder.set_surface(surface, SurfacePresentation::with_main(root));
        let mut engine = DockEngine::new(builder.build().unwrap(), DockPolicy::default()).unwrap();
        let mut host = TestPresentationHost::new(&mut engine);
        let bounds = LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap();
        publish_surface(&mut engine, &mut host, surface, bounds);
        let previous = JournalSurfacePresentation::from_interaction(
            engine.interaction_projection(surface).unwrap(),
        );
        publish_surface(&mut engine, &mut host, surface, bounds);
        let current = JournalSurfacePresentation::from_interaction(
            engine.interaction_projection(surface).unwrap(),
        );
        assert_eq!(previous.output_ticket(), current.output_ticket());
        assert_ne!(previous.authority(), current.authority());
        assert!(
            previous
                .authority()
                .same_interaction_semantics(current.authority())
        );

        let producer = engine
            .create_host_local_pointer_provider(host.lease(), PointerEdgeSequence::new(0))
            .unwrap();
        let provider = producer.lease();
        let sequence = PointerEdgeSequence::new(1);
        let hover_point = LogicalPoint::new(900.0, 700.0).unwrap();
        assert!(
            current
                .hit_manifest()
                .resolve_exclusive(PresentationPointerLane::HoverDrop, hover_point)
                .unwrap()
                .is_none()
        );
        let target = current
            .hit_manifest()
            .regions()
            .iter()
            .find(|region| {
                matches!(region.id().kind(), PresentationHitRegionKind::DropTarget(
                    crate::drop_target::DropTargetId::Center { root: target_root, .. }
                ) if target_root == root)
            })
            .unwrap();
        let rect = target.hit().rect();
        let occupied_point = LogicalPoint::new(
            rect.x() + rect.width() * 0.5,
            rect.y() + rect.height() * 0.5,
        )
        .unwrap();
        for (hover_point, expected_winner) in
            [(hover_point, None), (occupied_point, Some(target.id()))]
        {
            let edge = PointerEdge::new(
                sequence,
                PointerId::new(1),
                PointerEdgeKind::Moved,
                PointerEdgeLocation::HostLocal {
                    manipulation: Authority::Known(HostLocalPointerRoute::new(
                        current.authority(),
                        LogicalPoint::new(100.0, 20.0).unwrap(),
                    )),
                    hover: Authority::Known(HostLocalPointerRoute::new(
                        current.authority(),
                        hover_point,
                    )),
                },
                Authority::Known(PointerCaptureOwner::ProviderEndpoint),
            );
            let journal =
                PointerEdgeJournal::new(PointerEdgeSequence::new(0), sequence, vec![edge]).unwrap();
            let current_outputs = BTreeMap::from([(
                surface,
                PointerReceiverPresentedOutput::from_interaction(current.projection()),
            )]);
            for disposition in [
                PointerReceiverHoverHitDisposition::NoReceiver,
                PointerReceiverHoverHitDisposition::Blocked,
            ] {
                for receipt_presentation in [&previous, &current] {
                    let roster = PointerReceiverCandidateRoster::freeze(
                        PointerReceiverAttemptIssuer::new(engine.authority_domain)
                            .issue(provider)
                            .unwrap(),
                        vec![PointerReceiverCandidateSpec::hover_hit(
                            sequence,
                            Some(surface),
                            Some(hover_point),
                        )],
                        vec![PointerReceiverPresentedOutput::from_interaction(
                            receipt_presentation.projection(),
                        )],
                    )
                    .unwrap();
                    let observation = PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::HoverHit(
                            PointerReceiverHoverHit::new(
                                receipt_presentation.projection(),
                                hover_point,
                                disposition,
                            )
                            .unwrap(),
                        ),
                    ])
                    .unwrap();
                    let receipts = roster
                        .validate_against_presented_outputs(
                            PointerReceiverReceiptBatch::new([roster.candidates()[0]
                                .receipt(PointerReceiverObservation::Presented(observation))])
                            .unwrap(),
                            &current_outputs,
                        )
                        .expect(
                            "generic receipt validation allows unchanged interaction semantics",
                        );
                    let snapshot = JournalPresentationSnapshot::from_validated_receipts(
                        provider,
                        &BTreeMap::from([(surface, receipt_presentation.clone())]),
                        &receipts,
                    )
                    .unwrap();
                    let result = engine.validate_pointer_receiver_geometry(
                        provider,
                        journal.edges(),
                        &receipts,
                        &snapshot,
                        &BTreeMap::new(),
                        &BTreeMap::new(),
                    );
                    if receipt_presentation.authority() != current.authority() {
                        assert!(matches!(result, Err(EngineError::PointerReceiverGeometry {
                        source: PointerReceiverGeometryError::HostLocalHoverPresentationMismatch {
                            expected, actual, ..
                        },
                    }) if expected == current.authority() && actual == previous.authority()));
                    } else if disposition == PointerReceiverHoverHitDisposition::NoReceiver
                        && expected_winner.is_some()
                    {
                        assert!(matches!(result, Err(EngineError::PointerReceiverGeometry {
                        source: PointerReceiverGeometryError::ReceiverWinnerMismatch {
                            lane: PresentationPointerLane::HoverDrop,
                            claimed: None,
                            winner,
                            ..
                        },
                    }) if winner == expected_winner));
                    } else {
                        result.expect("an exact empty or blocked observation remains valid");
                    }
                }
            }
        }
    }
}

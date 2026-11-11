//! Pointer receiver and presentation authority validation.

use super::*;
use crate::pointer_journal::{
    DesktopDockRoute, HostLocalPointerRoute, NativePointerRoutes, NativeSurfacePointerRoute,
    PointerDeviceKind,
};
use crate::pointer_receiver::PointerReceiverDeliveryRequest;
use crate::pointer_receiver::{NativeWindowHitReceipt, NativeWindowPointerProbe};
use crate::presentation_hit::PresentationHitSuppression;
use crate::viewport::WindowBinding;

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
    route: NativeSurfacePointerRoute,
    authority: PresentedSurfaceAuthority,
) -> bool {
    authority.surface() == route.binding().surface()
        && authority.binding() == Some(route.binding())
        && authority.coordinate_generation() == route.coordinate_generation()
}

fn native_receiver_desktop_route(
    edge: &PointerEdge,
    scope: PointerProviderScope,
) -> Option<DesktopDockRoute> {
    if !matches!(scope, PointerProviderScope::NativeJoined(_)) {
        return None;
    }
    let route = edge.native_routes()?.hover?;
    let point = edge.desktop_route()?.position().known().copied()?;
    Some(DesktopDockRoute::new(
        route.binding(),
        route.coordinate_generation(),
        point,
        route.surface_position(),
    ))
}

fn native_source_binding(engine: &DockEngine, edge: &PointerEdge) -> Option<ViewportBinding> {
    engine
        .interaction
        .active_owner()
        .filter(|owner| owner.pointer() == edge.pointer())?;
    engine
        .interaction
        .active_presentation_authority()?
        .presented
        .binding()
}

fn native_source_route(
    engine: &DockEngine,
    edge: &PointerEdge,
    selected: Option<NativeSurfacePointerRoute>,
) -> Option<NativeSurfacePointerRoute> {
    let binding = native_source_binding(engine, edge)
        .or_else(|| selected.map(NativeSurfacePointerRoute::binding))?;
    let probe = native_window_pointer_probe(engine, edge, false)?;
    if binding.window() != probe.window() {
        return None;
    }
    let record = engine
        .viewport
        .registry()
        .placement_record(binding)
        .filter(|record| record.is_routeable())?;
    let coordinates = record.coordinates()?;
    if coordinates.window_coordinates().coordinate_generation() != probe.coordinate_generation()
        || engine
            .interaction
            .active_presentation_authority()
            .filter(|_| native_source_binding(engine, edge) == Some(binding))
            .is_some_and(|presentation| {
                presentation.presented.coordinate_generation()
                    != coordinates.coordinate_generation()
            })
    {
        return None;
    }
    let point = coordinates
        .client_to_surface(probe.client_position())
        .ok()?;
    Some(NativeSurfacePointerRoute::new(
        binding,
        coordinates.coordinate_generation(),
        probe.client_position(),
        point,
    ))
}

pub(super) fn native_window_pointer_probe(
    engine: &DockEngine,
    edge: &PointerEdge,
    hover: bool,
) -> Option<NativeWindowPointerProbe> {
    let PointerProviderScope::NativeJoined(host) = engine.pointer_provider()?.scope() else {
        return None;
    };
    match edge.location() {
        PointerEdgeLocation::NativeLocal {
            route: Authority::Known(route),
        } => {
            let coordinates = engine.native_window_coordinate_snapshot(host, route.binding())?;
            if coordinates.coordinate_generation() != route.coordinate_generation() {
                return None;
            }
            Some(NativeWindowPointerProbe::new(
                route.binding(),
                route.coordinate_generation(),
                route.client_position(),
            ))
        }
        PointerEdgeLocation::Desktop { route } => {
            let window = if hover {
                let Authority::Known(crate::pointer_journal::DesktopHoveredWindow::Dock(window)) =
                    route.hovered()
                else {
                    return None;
                };
                window
            } else {
                let Authority::Known(PointerEventDeliveryOwner::Native(window)) =
                    edge.delivery_owner()
                else {
                    return None;
                };
                window
            };
            let coordinates = engine.native_window_coordinate_snapshot(host, window)?;
            let origin = coordinates.content_bounds()?.min();
            let point = route.position().known().copied()?;
            let point =
                crate::geometry::PhysicalPoint::new(point.x() - origin.x(), point.y() - origin.y())
                    .ok()?;
            Some(NativeWindowPointerProbe::new(
                window,
                coordinates.coordinate_generation(),
                point,
            ))
        }
        _ => None,
    }
}

pub(super) fn validated_native_local_route(
    engine: &DockEngine,
    edge: &PointerEdge,
) -> Option<NativeSurfacePointerRoute> {
    if !matches!(edge.location(), PointerEdgeLocation::NativeLocal { .. }) {
        return None;
    }
    if let Some(owner) = engine.interaction.active_owner()
        && owner.pointer() == edge.pointer()
        && !matches!(edge.kind(), PointerEdgeKind::Scrolled(_))
        && !engine.journal_edge_matches_gesture_lane(owner, edge)
    {
        return None;
    }
    let selected = edge.native_routes().and_then(|routes| routes.manipulation);
    native_source_route(engine, edge, selected)
}

fn validated_native_local_hover(
    engine: &DockEngine,
    edge: &PointerEdge,
) -> Option<NativeSurfacePointerRoute> {
    let route = edge.native_routes()?.hover?;
    let probe = native_window_pointer_probe(engine, edge, true)?;
    if route.binding().window() != probe.window()
        || route.client_position() != probe.client_position()
    {
        return None;
    }
    let record = engine
        .viewport
        .registry()
        .placement_record(route.binding())
        .filter(|record| record.is_routeable())?;
    let coordinates = record.coordinates()?;
    let projection = engine.interaction_projection(route.binding().surface())?;
    (coordinates.window_coordinates().coordinate_generation() == probe.coordinate_generation()
        && coordinates
            .surface_client_geometry()
            .clip()
            .contains(route.client_position())
        && coordinates.client_to_surface(route.client_position()).ok()
            == Some(route.surface_position())
        && native_local_route_matches_authority(route, projection.authority())
        && DockEngine::native_surface_hit_contains(projection, route.surface_position()))
    .then_some(route)
}

/// Derives two routes inside the existing segment; the original ledger edge is unchanged.
pub(super) fn resolve_native_pointer_edge(
    engine: &DockEngine,
    edge: &PointerEdge,
    receipt: &ValidatedPointerReceiverReceipt,
) -> Result<PointerEdge, EngineError> {
    let hover = receipt.native_hit().and_then(NativeWindowHitReceipt::route);
    if let Some(route) = hover {
        let probe = native_window_pointer_probe(
            engine,
            edge,
            receipt.delivery_request() == PointerReceiverDeliveryRequest::None,
        )
        .ok_or(EngineError::PointerReceiverGeometry {
            source: PointerReceiverGeometryError::LogicalPointUnavailable {
                sequence: edge.sequence(),
            },
        })?;
        let record = engine
            .viewport
            .registry()
            .placement_record(route.binding())
            .filter(|record| record.is_routeable());
        let valid = record
            .and_then(|record| record.coordinates())
            .is_some_and(|coordinates| {
                route.binding().window() == probe.window()
                    && coordinates.window_coordinates().coordinate_generation()
                        == probe.coordinate_generation()
                    && coordinates.coordinate_generation() == route.coordinate_generation()
                    && route.client_position() == probe.client_position()
                    && coordinates
                        .surface_client_geometry()
                        .clip()
                        .contains(route.client_position())
                    && coordinates.client_to_surface(route.client_position()).ok()
                        == Some(route.surface_position())
            })
            && engine
                .interaction_projection(route.binding().surface())
                .is_some_and(|projection| {
                    DockEngine::native_surface_hit_contains(projection, route.surface_position())
                });
        if !valid {
            return Err(EngineError::PointerReceiverGeometry {
                source: PointerReceiverGeometryError::LogicalPointUnavailable {
                    sequence: edge.sequence(),
                },
            });
        }
    }
    let source = native_source_route(engine, edge, hover);
    let mut effective = edge.with_native_routes(NativePointerRoutes {
        manipulation: source,
        hover,
    });
    if let (PointerEdgeLocation::Desktop { route: original }, Some(hover)) =
        (edge.location(), hover)
    {
        if let Authority::Known(point) = original.position() {
            effective = effective.with_location(PointerEdgeLocation::Desktop {
                route: DesktopRouteFact::dock(DesktopDockRoute::new(
                    hover.binding(),
                    hover.coordinate_generation(),
                    point,
                    hover.surface_position(),
                )),
            });
        }
    }
    Ok(effective)
}

pub(super) fn pointer_receiver_candidate_spec(
    engine: &DockEngine,
    edge: &PointerEdge,
    stream: PointerStreamId,
) -> PointerReceiverCandidateSpec {
    if matches!(edge.kind(), PointerEdgeKind::CaptureAcquired(_)) {
        // Acquiring capture may postpone timed work, but cannot inspect or execute a target.
        return PointerReceiverCandidateSpec::not_applicable(edge.sequence());
    }
    if let PointerEdgeKind::Continuation(token) = edge.kind()
        && (!engine.journal_continuation_is_current(stream, token)
            || engine
                .interaction
                .active_owner()
                .is_none_or(|owner| !engine.stationary_edge_has_authority(owner, edge)))
    {
        return PointerReceiverCandidateSpec::not_applicable(edge.sequence());
    }
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
                        route.map(NativeSurfacePointerRoute::surface_position),
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
        let spec = PointerReceiverCandidateSpec::scroll_delivery(
            edge.sequence(),
            delivery_surface,
            delivery_point,
            challenge,
        );
        return if !matches!(challenge, ScrollReceiverChallenge::Locked { .. })
            && challenge.requires_receiver_probe()
        {
            native_window_pointer_probe(engine, edge, false).map_or(spec.clone(), |probe| {
                spec.with_native_window(probe, PointerReceiverDeliveryRequest::Scroll)
            })
        } else {
            spec
        };
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
                    route.map(NativeSurfacePointerRoute::surface_position),
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
        let spec =
            PointerReceiverCandidateSpec::delivery(edge.sequence(), surface, point, delivery);
        return native_window_pointer_probe(engine, edge, false).map_or(spec.clone(), |probe| {
            spec.with_native_window(probe, delivery)
        });
    }
    if hover {
        let surface = position.hover_surface();
        let point = position.hover_probe().map(SurfacePointer::position);
        let spec = PointerReceiverCandidateSpec::hover_hit(edge.sequence(), surface, point);
        return native_window_pointer_probe(engine, edge, true).map_or(spec.clone(), |probe| {
            spec.with_native_window(probe, PointerReceiverDeliveryRequest::None)
        });
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
    if !engine
        .pointer_provider()
        .is_some_and(|provider| matches!(provider.scope(), PointerProviderScope::NativeJoined(_)))
    {
        let Authority::Known(PointerEventDeliveryOwner::Native(window)) = edge.delivery_owner()
        else {
            return None;
        };
        let desktop = edge.desktop_route()?;
        let endpoint = match edge.kind() {
            PointerEdgeKind::Scrolled(scroll) => scroll
                .delivery()
                .known()
                .and_then(|endpoint| endpoint.binding()),
            _ => None,
        };
        let binding = endpoint
            .or_else(|| native_source_binding(engine, edge))
            .filter(|binding| binding.window() == window)
            .or_else(|| {
                desktop
                    .validate_against_registry(engine.authority_domain, engine.viewport.registry())
                    .dock_route()
                    .map(DesktopDockRoute::binding)
                    .filter(|binding| binding.window() == window)
            })
            .or_else(|| {
                let mut placements = engine.viewport.registry().bindings_for_window(window);
                let binding = placements.next()?;
                placements.next().is_none().then_some(binding)
            })?;
        let route = DesktopRouteFact::dock_from_desktop_position(binding, desktop.position())
            .validate_against_registry(engine.authority_domain, engine.viewport.registry());
        return route.dock_route().map(|_| route);
    }
    let point = edge.desktop_route()?.position().known().copied()?;
    let selected = edge.native_routes().and_then(|routes| routes.manipulation);
    let source = native_source_route(engine, edge, selected)?;
    Some(DesktopRouteValidation::Known(
        crate::pointer_journal::ValidatedDesktopRoute::Dock(DesktopDockRoute::new(
            source.binding(),
            source.coordinate_generation(),
            point,
            source.surface_position(),
        )),
    ))
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
                    binding: route.binding().window(),
                    coordinate_generation: self
                        .viewport
                        .registry()
                        .window_coordinates(route.binding().window())?
                        .coordinate_generation(),
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

    pub(crate) fn native_surface_hit_contains(
        projection: SurfaceInteractionProjection<'_>,
        point: LogicalPoint,
    ) -> bool {
        if projection.plan().bounds().contains(point) {
            return true;
        }
        let popup = projection.plan().popup();
        // Admit exact active popup receivers outside the dock layout. Its
        // semantic backdrop still blocks docking in otherwise blank space.
        popup.revision() == projection.popup_revision()
            && popup.session().is_some_and(|session| {
                projection
                    .hit_manifest()
                    .region_for_kind(PresentationHitRegionKind::TabListMenuBackdrop(session))
                    .is_some_and(|backdrop| backdrop.hit().contains(point))
            })
    }

    pub(crate) fn native_window_coordinate_snapshot(
        &self,
        host: PresentationHostLease,
        window: WindowBinding,
    ) -> Option<crate::coordinates::WindowCoordinateSnapshot> {
        let provider = self.backend_ingress_provider()?;
        if provider.presentation_host() != host
            || provider.pointer_provider().scope() != PointerProviderScope::NativeJoined(host)
        {
            return None;
        }
        let coordinates = self
            .viewport
            .registry()
            .window(window)
            .filter(|record| record.is_routeable())?
            .coordinates()?;
        (coordinates.content_size().width() > 0.0 && coordinates.content_size().height() > 0.0)
            .then_some(coordinates)
    }

    pub(crate) fn native_local_coordinate_snapshot(
        &self,
        host: PresentationHostLease,
        binding: ViewportBinding,
    ) -> Option<CoordinateSnapshot> {
        self.native_window_coordinate_snapshot(host, binding.window())?;
        let (_, owner, endpoint) = self
            .presentation_authority
            .presentation
            .active_surface_scope(binding.surface())?;
        if owner != host || endpoint != HostPresentationEndpoint::Native(binding) {
            return None;
        }
        self.viewport
            .registry()
            .placement_record(binding)
            .filter(|record| record.is_routeable())?
            .coordinates()
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

    pub(super) fn journal_pointer_hit_device(&self, edge: &PointerEdge) -> PointerDeviceKind {
        match edge.location() {
            PointerEdgeLocation::SurfaceLocal { .. }
            | PointerEdgeLocation::HostLocal { .. }
            | PointerEdgeLocation::NativeLocal { .. }
            | PointerEdgeLocation::NativeLocalToDesktop { .. } => edge.metadata().device(),
            PointerEdgeLocation::Desktop { .. } => {
                if self.pointer_provider().is_some_and(|provider| {
                    matches!(provider.scope(), PointerProviderScope::NativeJoined(_))
                }) {
                    edge.metadata().device()
                } else {
                    PointerDeviceKind::Unknown
                }
            }
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
                            // A smooth continuation probes its frozen receiver,
                            // independently of any current pointer placement.
                            // The scroll FSM validates that exact owner in the
                            // same rollback candidate.
                            if lane == PresentationPointerLane::Scroll
                                && matches!(edge.kind(), PointerEdgeKind::Scrolled(scroll)
                                    if !matches!(scroll.phase(), crate::pointer_journal::ScrollPhase::Discrete | crate::pointer_journal::ScrollPhase::Begin))
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
                let route = validated_native_local_hover(self, edge).ok_or(
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
                let Some(route) = native_receiver_desktop_route(edge, provider.scope())
                    .or_else(|| desktop_route.and_then(DesktopRouteValidation::dock_route))
                else {
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
                self.journal_pointer_hit_device(edge),
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
                let route = validated_native_local_hover(self, edge).ok_or(
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
                let Some(route) = native_receiver_desktop_route(edge, provider.scope())
                    .or_else(|| desktop_route.and_then(DesktopRouteValidation::dock_route))
                else {
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
            self.journal_pointer_hit_device(edge),
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
            self.journal_pointer_hit_device(edge),
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
        (self.host_local_manipulation_authority(edge.pointer()) == Some(route.authority()))
            .then_some(route)
    }

    /// Returns only the active source's retained coordinate authority.
    pub(crate) fn host_local_manipulation_authority(
        &self,
        pointer: crate::intent::PointerId,
    ) -> Option<PresentedSurfaceAuthority> {
        let stream = self.interaction.active_stream()?;
        let host = self.pointer_provider()?.scope().host_local()?;
        if Some(stream.lease()) != self.pointer_provider() || stream.pointer() != pointer {
            return None;
        }
        // Selection, raising and live resizing may invalidate their own source
        // scene. Only the exact active owner can retain that source coordinate
        // proof; hover and new delivery claims always require current output.
        let frozen = self.interaction.active_presentation_authority()?;
        let authority = frozen.presented;
        if authority.endpoint() != HostPresentationEndpoint::Headless
            || self
                .presentation_authority
                .presentation
                .active_surface_scope(authority.surface())
                != Some((authority.stream(), host, HostPresentationEndpoint::Headless))
        {
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
        .then_some(authority)
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
                .map(NativeSurfacePointerRoute::surface_position),
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

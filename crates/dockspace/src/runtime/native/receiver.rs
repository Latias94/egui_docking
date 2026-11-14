//! Product-level native receiver resolution.

use super::{
    NativePlatformError, NativeProjectedScrollDelta, NativeReceiverAnswer, NativeReceiverPurpose,
    NativeReceiverQuery, NativeScrollReceiverChallenge, NativeSurfaceReceiverQuery,
};
use crate::engine::CoreHostFrame;
use crate::pointer_receiver::{
    NativeWindowHitReceipt, PointerReceiverCandidate, PointerReceiverDelivery,
    PointerReceiverDeliveryDisposition, PointerReceiverDeliveryRequest, PointerReceiverHoverHit,
    PointerReceiverHoverHitDisposition, PointerReceiverObservation, PointerReceiverProbeReceipt,
    PointerReceiverProbeRequest, PointerReceiverUnknownReason, PresentedPointerReceiverObservation,
    ScrollReceiverChallenge,
};
use crate::runtime::{
    DockspaceRuntimeError, PresentedDockReceiver, PresentedDockspaceSurface, interaction,
};

pub(super) fn resolve_receiver_observation(
    frame: &CoreHostFrame,
    candidate: &PointerReceiverCandidate,
    resolver: &mut dyn FnMut(NativeReceiverQuery) -> NativeReceiverAnswer,
    presentation: &crate::runtime::presentation::RuntimePresentationState,
) -> Result<(PointerReceiverObservation, Option<NativeWindowHitReceipt>), DockspaceRuntimeError> {
    let native_hit = if let Some(probe) = candidate.native_window() {
        let outside_client = frame
            .view()
            .viewport()
            .registry()
            .window(probe.window())
            .and_then(|record| record.coordinates())
            .is_some_and(|coordinates| {
                let point = probe.client_position();
                point.x() < 0.0
                    || point.y() < 0.0
                    || point.x() > coordinates.content_size().width()
                    || point.y() > coordinates.content_size().height()
            });
        if outside_client {
            return Ok((
                unknown_observation(),
                Some(NativeWindowHitReceipt::WindowBackground),
            ));
        }
        let Some(query) = super::window_hit::NativeWindowHitQuery::from_candidate(frame, candidate)
        else {
            return Ok((unknown_observation(), Some(NativeWindowHitReceipt::Unknown)));
        };
        let hit = match resolver(NativeReceiverQuery::WindowHit(query)) {
            NativeReceiverAnswer::WindowHit(hit) => hit,
            _ => super::window_hit::NativeWindowHit::Unknown,
        };
        Some(super::window_hit::validate_window_hit(
            frame, candidate, hit,
        ))
    } else {
        None
    };
    let selected = native_hit.map(|hit| candidate.with_native_selection(hit));
    let candidate = selected.as_ref().unwrap_or(candidate);
    let surface = match candidate.probes() {
        PointerReceiverProbeRequest::Delivery => candidate.delivery_surface(),
        PointerReceiverProbeRequest::HoverHit => candidate.hover_surface(),
        PointerReceiverProbeRequest::NotApplicable => None,
    };
    if surface.is_some_and(|surface| {
        frame
            .view()
            .interaction_projection(surface)
            .is_some_and(|projection| {
                presentation.blocks_pointer_authority(surface, projection.authority())
            })
    }) {
        return Ok((unknown_observation(), native_hit));
    }
    let observation = match candidate.probes() {
        PointerReceiverProbeRequest::NotApplicable => Ok(PointerReceiverObservation::NotApplicable),
        PointerReceiverProbeRequest::Delivery => {
            resolve_delivery_observation(frame, candidate, resolver)
        }
        PointerReceiverProbeRequest::HoverHit => {
            resolve_hover_observation(frame, candidate, resolver)
        }
    }?;
    Ok((observation, native_hit))
}

fn resolve_delivery_observation(
    frame: &CoreHostFrame,
    candidate: &PointerReceiverCandidate,
    resolver: &mut dyn FnMut(NativeReceiverQuery) -> NativeReceiverAnswer,
) -> Result<PointerReceiverObservation, DockspaceRuntimeError> {
    let Some(surface) = candidate.delivery_surface() else {
        return Ok(unknown_observation());
    };
    let Some(projection) = frame.view().interaction_projection(surface) else {
        return Ok(unknown_observation());
    };
    let presented_surface = PresentedDockspaceSurface::from_projection(projection);
    let unknown =
        PointerReceiverDeliveryDisposition::Unknown(PointerReceiverUnknownReason::NotReported);
    let (click, drag, scroll) = match candidate.delivery_request() {
        PointerReceiverDeliveryRequest::None => {
            return Err(NativePlatformError::ProtocolInvariant.into());
        }
        PointerReceiverDeliveryRequest::Click => {
            let query = NativeSurfaceReceiverQuery {
                purpose: NativeReceiverPurpose::ClickDelivery,
                presented_surface,
                point: candidate.route_point(),
                client_position: physical_client_point(
                    frame,
                    candidate,
                    surface,
                    candidate.route_point(),
                ),
                metadata: candidate.metadata(),
            };
            (
                resolve_delivery_lane(projection, &resolver(NativeReceiverQuery::Surface(query))),
                unknown,
                unknown,
            )
        }
        PointerReceiverDeliveryRequest::ClickAndDrag => {
            let click = NativeSurfaceReceiverQuery {
                purpose: NativeReceiverPurpose::ClickDelivery,
                presented_surface,
                point: candidate.route_point(),
                client_position: physical_client_point(
                    frame,
                    candidate,
                    surface,
                    candidate.route_point(),
                ),
                metadata: candidate.metadata(),
            };
            let drag = NativeSurfaceReceiverQuery {
                purpose: NativeReceiverPurpose::DragDelivery,
                presented_surface,
                point: candidate.route_point(),
                client_position: physical_client_point(
                    frame,
                    candidate,
                    surface,
                    candidate.route_point(),
                ),
                metadata: candidate.metadata(),
            };
            (
                resolve_delivery_lane(projection, &resolver(NativeReceiverQuery::Surface(click))),
                resolve_delivery_lane(projection, &resolver(NativeReceiverQuery::Surface(drag))),
                unknown,
            )
        }
        PointerReceiverDeliveryRequest::Scroll => {
            let Some(challenge) = native_scroll_challenge(frame, candidate) else {
                return Ok(unknown_observation());
            };
            let query = NativeSurfaceReceiverQuery {
                purpose: NativeReceiverPurpose::ScrollDelivery(challenge),
                presented_surface,
                point: candidate.route_point(),
                client_position: physical_client_point(
                    frame,
                    candidate,
                    surface,
                    candidate.route_point(),
                ),
                metadata: candidate.metadata(),
            };
            (
                unknown,
                unknown,
                resolve_delivery_lane(projection, &resolver(NativeReceiverQuery::Surface(query))),
            )
        }
    };
    let delivery = PointerReceiverDelivery::from_lanes_with_scroll(projection, click, drag, scroll)
        .map_err(|_| NativePlatformError::ProtocolInvariant)?;
    presented_observation(PointerReceiverProbeReceipt::Delivery(delivery))
}

fn resolve_hover_observation(
    frame: &CoreHostFrame,
    candidate: &PointerReceiverCandidate,
    resolver: &mut dyn FnMut(NativeReceiverQuery) -> NativeReceiverAnswer,
) -> Result<PointerReceiverObservation, DockspaceRuntimeError> {
    let (Some(surface), Some(point)) = (candidate.hover_surface(), candidate.hover_point()) else {
        return Ok(unknown_observation());
    };
    let Some(projection) = frame.view().interaction_projection(surface) else {
        return Ok(unknown_observation());
    };
    let query = NativeSurfaceReceiverQuery {
        purpose: NativeReceiverPurpose::HoverHit,
        presented_surface: PresentedDockspaceSurface::from_projection(projection),
        point: Some(point),
        client_position: physical_client_point(frame, candidate, surface, Some(point)),
        metadata: candidate.metadata(),
    };
    let disposition = match resolver(NativeReceiverQuery::Surface(query)) {
        NativeReceiverAnswer::Dock(receiver)
            if interaction::receiver_matches(&receiver, &projection) =>
        {
            PointerReceiverHoverHitDisposition::Dock(receiver.region())
        }
        NativeReceiverAnswer::EmptyLane(surface)
            if interaction::surface_matches(&surface, &projection) =>
        {
            PointerReceiverHoverHitDisposition::EmptyLane
        }
        NativeReceiverAnswer::ClearHoverLane(surface)
            if interaction::surface_matches(&surface, &projection) =>
        {
            let hover = frame
                .view()
                .resolve_hover_drop_receiver(surface.surface(), point)
                .map_err(|_| NativePlatformError::ProtocolInvariant)?;
            return presented_observation(PointerReceiverProbeReceipt::HoverHit(hover));
        }
        NativeReceiverAnswer::Blocked(surface)
            if interaction::surface_matches(&surface, &projection) =>
        {
            PointerReceiverHoverHitDisposition::Blocked
        }
        NativeReceiverAnswer::WindowHit(_)
        | NativeReceiverAnswer::Dock(_)
        | NativeReceiverAnswer::EmptyLane(_)
        | NativeReceiverAnswer::ClearHoverLane(_)
        | NativeReceiverAnswer::Blocked(_)
        | NativeReceiverAnswer::Unknown => {
            PointerReceiverHoverHitDisposition::Unknown(PointerReceiverUnknownReason::NotReported)
        }
    };
    let hover = PointerReceiverHoverHit::new(projection, point, disposition)
        .map_err(|_| NativePlatformError::ProtocolInvariant)?;
    presented_observation(PointerReceiverProbeReceipt::HoverHit(hover))
}

fn native_scroll_challenge(
    frame: &CoreHostFrame,
    candidate: &PointerReceiverCandidate,
) -> Option<NativeScrollReceiverChallenge> {
    match candidate.scroll_challenge()? {
        ScrollReceiverChallenge::Spatial { projected_delta } => {
            Some(NativeScrollReceiverChallenge::Spatial {
                projected_delta: projected_delta.map(NativeProjectedScrollDelta::from_core),
            })
        }
        ScrollReceiverChallenge::Locked {
            receiver,
            projected_delta,
            ..
        } => {
            let projection = frame.view().interaction_projection(receiver.surface())?;
            let receiver = PresentedDockReceiver::from_projection_region(projection, receiver)?;
            Some(NativeScrollReceiverChallenge::Locked {
                receiver,
                projected_delta: projected_delta.map(NativeProjectedScrollDelta::from_core),
            })
        }
        ScrollReceiverChallenge::OwnedTerminal
        | ScrollReceiverChallenge::FrameworkReserved
        | ScrollReceiverChallenge::Unavailable => None,
    }
}

fn resolve_delivery_lane(
    projection: crate::scene::SurfaceInteractionProjection<'_>,
    answer: &NativeReceiverAnswer,
) -> PointerReceiverDeliveryDisposition {
    match answer {
        NativeReceiverAnswer::Dock(receiver)
            if interaction::receiver_matches(&receiver, &projection) =>
        {
            PointerReceiverDeliveryDisposition::Dock(receiver.region())
        }
        NativeReceiverAnswer::EmptyLane(surface)
            if interaction::surface_matches(&surface, &projection) =>
        {
            PointerReceiverDeliveryDisposition::EmptyLane
        }
        NativeReceiverAnswer::Blocked(surface)
            if interaction::surface_matches(&surface, &projection) =>
        {
            PointerReceiverDeliveryDisposition::Blocked
        }
        NativeReceiverAnswer::WindowHit(_)
        | NativeReceiverAnswer::Dock(_)
        | NativeReceiverAnswer::EmptyLane(_)
        | NativeReceiverAnswer::ClearHoverLane(_)
        | NativeReceiverAnswer::Blocked(_)
        | NativeReceiverAnswer::Unknown => {
            PointerReceiverDeliveryDisposition::Unknown(PointerReceiverUnknownReason::NotReported)
        }
    }
}

fn presented_observation(
    probe: PointerReceiverProbeReceipt,
) -> Result<PointerReceiverObservation, DockspaceRuntimeError> {
    let presented = PresentedPointerReceiverObservation::new([probe])
        .map_err(|_| NativePlatformError::ProtocolInvariant)?;
    Ok(PointerReceiverObservation::Presented(presented))
}

const fn unknown_observation() -> PointerReceiverObservation {
    PointerReceiverObservation::Unknown(PointerReceiverUnknownReason::NotReported)
}

fn physical_client_point(
    frame: &CoreHostFrame,
    candidate: &PointerReceiverCandidate,
    surface: crate::ids::SurfaceId,
    point: Option<crate::geometry::LogicalPoint>,
) -> Option<crate::geometry::PhysicalPoint> {
    if let Some(probe) = candidate.native_window() {
        return Some(probe.client_position());
    }
    frame
        .view()
        .viewport()
        .viewport(surface)?
        .coordinates()?
        .surface_client_geometry()
        .surface_to_client(point?)
        .ok()
}

//! Product-level native receiver resolution.

use super::{
    NativePlatformError, NativeProjectedScrollDelta, NativeReceiverAnswer, NativeReceiverPurpose,
    NativeReceiverQuery, NativeScrollReceiverChallenge,
};
use crate::engine::CoreHostFrame;
use crate::pointer_receiver::{
    PointerReceiverCandidate, PointerReceiverDelivery, PointerReceiverDeliveryDisposition,
    PointerReceiverDeliveryRequest, PointerReceiverHoverHit, PointerReceiverHoverHitDisposition,
    PointerReceiverObservation, PointerReceiverProbeReceipt, PointerReceiverProbeRequest,
    PointerReceiverUnknownReason, PresentedPointerReceiverObservation, ScrollReceiverChallenge,
};
use crate::runtime::{
    DockspaceRuntimeError, PresentedDockReceiver, PresentedDockspaceSurface, interaction,
};

pub(super) fn resolve_receiver_observation(
    frame: &CoreHostFrame,
    candidate: &PointerReceiverCandidate,
    resolver: &mut dyn FnMut(NativeReceiverQuery) -> NativeReceiverAnswer,
) -> Result<PointerReceiverObservation, DockspaceRuntimeError> {
    match candidate.probes() {
        PointerReceiverProbeRequest::NotApplicable => Ok(PointerReceiverObservation::NotApplicable),
        PointerReceiverProbeRequest::Delivery => {
            resolve_delivery_observation(frame, candidate, resolver)
        }
        PointerReceiverProbeRequest::HoverHit => {
            resolve_hover_observation(frame, candidate, resolver)
        }
    }
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
            let query = NativeReceiverQuery {
                purpose: NativeReceiverPurpose::ClickDelivery,
                presented_surface,
                point: candidate.route_point(),
            };
            (
                resolve_delivery_lane(projection, &resolver(query)),
                unknown,
                unknown,
            )
        }
        PointerReceiverDeliveryRequest::ClickAndDrag => {
            let click = NativeReceiverQuery {
                purpose: NativeReceiverPurpose::ClickDelivery,
                presented_surface,
                point: candidate.route_point(),
            };
            let drag = NativeReceiverQuery {
                purpose: NativeReceiverPurpose::DragDelivery,
                presented_surface,
                point: candidate.route_point(),
            };
            (
                resolve_delivery_lane(projection, &resolver(click)),
                resolve_delivery_lane(projection, &resolver(drag)),
                unknown,
            )
        }
        PointerReceiverDeliveryRequest::Scroll => {
            let Some(challenge) = native_scroll_challenge(frame, candidate) else {
                return Ok(unknown_observation());
            };
            let query = NativeReceiverQuery {
                purpose: NativeReceiverPurpose::ScrollDelivery(challenge),
                presented_surface,
                point: candidate.route_point(),
            };
            (
                unknown,
                unknown,
                resolve_delivery_lane(projection, &resolver(query)),
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
    let query = NativeReceiverQuery {
        purpose: NativeReceiverPurpose::HoverHit,
        presented_surface: PresentedDockspaceSurface::from_projection(projection),
        point: Some(point),
    };
    let disposition = match resolver(query) {
        NativeReceiverAnswer::Dock(receiver)
            if interaction::receiver_matches(&receiver, &projection) =>
        {
            PointerReceiverHoverHitDisposition::Dock(receiver.region())
        }
        NativeReceiverAnswer::NoReceiver(surface)
            if interaction::surface_matches(&surface, &projection) =>
        {
            PointerReceiverHoverHitDisposition::NoReceiver
        }
        NativeReceiverAnswer::Blocked(surface)
            if interaction::surface_matches(&surface, &projection) =>
        {
            PointerReceiverHoverHitDisposition::Blocked
        }
        NativeReceiverAnswer::Dock(_)
        | NativeReceiverAnswer::NoReceiver(_)
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
        NativeReceiverAnswer::NoReceiver(surface)
            if interaction::surface_matches(&surface, &projection) =>
        {
            PointerReceiverDeliveryDisposition::NoReceiver
        }
        NativeReceiverAnswer::Blocked(surface)
            if interaction::surface_matches(&surface, &projection) =>
        {
            PointerReceiverDeliveryDisposition::Blocked
        }
        NativeReceiverAnswer::Dock(_)
        | NativeReceiverAnswer::NoReceiver(_)
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

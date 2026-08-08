//! Exact native pointer receiver evidence.

use std::collections::BTreeMap;

use dockspace::geometry::LogicalPoint;
use dockspace::backend::pointer_receiver::{
    PointerReceiverCandidate, PointerReceiverDelivery, PointerReceiverDeliveryDisposition,
    PointerReceiverHoverHit, PointerReceiverHoverHitDisposition, PointerReceiverObservation,
    PointerReceiverProbe, PointerReceiverProbeReceipt, PointerReceiverReceiptBatch,
    PointerReceiverUnknownReason, PresentedPointerReceiverObservation, ScrollReceiverChallenge,
};
use dockspace::backend::presentation_hit::{PresentationHitRegionKind, PresentationPointerLane};
use dockspace::backend::scene::SurfaceInteractionProjection;
use eframe::{
    NativePhysicalPoint, NativePointerEdge, NativePointerEdgeKind, NativePointerSequence,
    NativeViewportBinding,
};
use egui::{
    PointerHit, PointerReceiverAuthority, Pos2, ScrollProbe, ScrollReceiver, Sense, WidgetReceiver,
};
use egui_dockspace::Dockspace;
use egui_dockspace::backend::{
    EguiNativeInputSession, PaintReceiverFingerprint, PaintReceiverLookup,
};

use crate::NativeRuntimeError;
use crate::ingress::{BoundNativeRoute, RetainedPointerEdge};
use crate::presentation::PresentedNativePointerGraph;

pub(crate) struct PointerReceiverResolution {
    receipts: PointerReceiverReceiptBatch,
    scroll_claims: Vec<NativeScrollDerivativeClaim>,
}

impl PointerReceiverResolution {
    pub(crate) fn into_parts(
        self,
    ) -> (
        PointerReceiverReceiptBatch,
        Vec<NativeScrollDerivativeClaim>,
    ) {
        (self.receipts, self.scroll_claims)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NativeScrollDerivativeClaim {
    pointer_sequence: NativePointerSequence,
}

impl NativeScrollDerivativeClaim {
    pub(crate) const fn pointer_sequence(self) -> NativePointerSequence {
        self.pointer_sequence
    }
}

pub(crate) fn pointer_receiver_receipts(
    dockspace: &Dockspace,
    input: &EguiNativeInputSession,
    pointer_edges: &BTreeMap<u64, RetainedPointerEdge>,
) -> Result<PointerReceiverResolution, NativeRuntimeError> {
    let candidates =
        input
            .pointer_receiver_candidates()
            .ok_or(NativeRuntimeError::IngressUnavailable(
                "core paused backend ingress without a receiver challenge",
            ))?;
    let mut receipts = Vec::with_capacity(candidates.candidates().len());
    let mut scroll_claims = Vec::new();
    for candidate in candidates.candidates() {
        let resolved = resolve_candidate(dockspace, input, pointer_edges, candidate)?;
        if let Some(claim) = resolved.scroll_claim {
            scroll_claims.push(claim);
        }
        receipts.push(candidate.receipt(resolved.observation));
    }
    Ok(PointerReceiverResolution {
        receipts: PointerReceiverReceiptBatch::new(receipts)?,
        scroll_claims,
    })
}

struct ResolvedPointerReceiver {
    observation: PointerReceiverObservation,
    scroll_claim: Option<NativeScrollDerivativeClaim>,
}

fn resolve_candidate(
    dockspace: &Dockspace,
    input: &EguiNativeInputSession,
    pointer_edges: &BTreeMap<u64, RetainedPointerEdge>,
    candidate: &PointerReceiverCandidate,
) -> Result<ResolvedPointerReceiver, NativeRuntimeError> {
    let challenge = candidate.scroll_challenge();
    let retained = pointer_edges.get(&candidate.id().sequence().get());
    let locked_claim = match challenge {
        Some(ScrollReceiverChallenge::Locked { .. } | ScrollReceiverChallenge::OwnedTerminal) => {
            retained.map(|retained| scroll_derivative_claim(retained.edge()))
        }
        Some(
            ScrollReceiverChallenge::Spatial { .. }
            | ScrollReceiverChallenge::FrameworkReserved
            | ScrollReceiverChallenge::Unavailable,
        )
        | None => None,
    };
    if !candidate.receiver_is_applicable() {
        return Ok(ResolvedPointerReceiver {
            observation: PointerReceiverObservation::NotApplicable,
            scroll_claim: locked_claim,
        });
    }
    let Some(retained) = retained else {
        return Ok(resolved_unknown_with_claim(
            PointerReceiverUnknownReason::EventCorrelationUnavailable,
            locked_claim,
        ));
    };
    let edge = retained.edge();
    let mut probes = Vec::new();
    let mut scroll_claim = None;
    if candidate.probes().requires(PointerReceiverProbe::Delivery) {
        let Some(route) = retained.delivery_route() else {
            return Ok(resolved_unknown_with_claim(
                PointerReceiverUnknownReason::EventCorrelationUnavailable,
                locked_claim,
            ));
        };
        let Some(projection) = input
            .receiver_view()
            .interaction_projection(route.surface())
        else {
            return Ok(resolved_unknown_with_claim(
                PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
                locked_claim,
            ));
        };
        let Some(graph) = retained.graphs().delivery() else {
            return Ok(resolved_unknown_with_claim(
                PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
                locked_claim,
            ));
        };
        if graph.native() != route.exact() || graph.scene() != projection.output_ticket() {
            return Ok(resolved_unknown_with_claim(
                PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
                locked_claim,
            ));
        }
        let delivery = match challenge {
            Some(ScrollReceiverChallenge::Locked {
                receiver,
                probe_point,
                projected_delta,
            }) => locked_scroll_delivery(
                dockspace,
                input,
                route,
                projection,
                graph.graph(),
                receiver,
                probe_point,
                projected_delta,
            )?,
            Some(ScrollReceiverChallenge::Spatial { .. }) | None => {
                let Some(logical_point) = candidate.route_point() else {
                    return Ok(resolved_unknown_with_claim(
                        PointerReceiverUnknownReason::EventCorrelationUnavailable,
                        locked_claim,
                    ));
                };
                let Some(egui_point) = presented_point(edge, route, graph, logical_point, true)
                else {
                    return Ok(resolved_unknown_with_claim(
                        PointerReceiverUnknownReason::EventCorrelationUnavailable,
                        locked_claim,
                    ));
                };
                let PointerReceiverAuthority::Known(hit) = graph.graph().probe(egui_point) else {
                    return Ok(resolved_unknown_with_claim(
                        PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
                        locked_claim,
                    ));
                };
                match challenge {
                    Some(ScrollReceiverChallenge::Spatial { projected_delta }) => {
                        scroll_delivery_receipt(
                            dockspace,
                            input,
                            route,
                            projection,
                            logical_point,
                            projected_delta,
                            graph.graph(),
                            egui_point,
                            &hit,
                        )?
                    }
                    None => pointer_delivery_receipt(
                        dockspace,
                        input,
                        route,
                        projection,
                        logical_point,
                        &hit,
                    )?,
                    Some(_) => unreachable!("the outer match restricts this receiver challenge"),
                }
            }
            Some(
                ScrollReceiverChallenge::OwnedTerminal
                | ScrollReceiverChallenge::FrameworkReserved
                | ScrollReceiverChallenge::Unavailable,
            ) => {
                return Err(NativeRuntimeError::IngressUnavailable(
                    "non-probing scroll challenge requested receiver evidence",
                ));
            }
        };
        let spatial_claim = if matches!(challenge, Some(ScrollReceiverChallenge::Spatial { .. }))
            && matches!(edge.kind(), NativePointerEdgeKind::Scrolled(_))
            && matches!(
                delivery.scroll(),
                PointerReceiverDeliveryDisposition::Dock(_)
            ) {
            Some(scroll_derivative_claim(edge))
        } else {
            None
        };
        scroll_claim = locked_claim.or(spatial_claim);
        probes.push(PointerReceiverProbeReceipt::Delivery(delivery));
    }
    if candidate.probes().requires(PointerReceiverProbe::HoverHit) {
        let Some(route) = retained.hovered_route() else {
            return Ok(resolved_unknown(
                PointerReceiverUnknownReason::EventCorrelationUnavailable,
            ));
        };
        let Some(candidate_point) = candidate.hover_point() else {
            return Ok(resolved_unknown(
                PointerReceiverUnknownReason::EventCorrelationUnavailable,
            ));
        };
        if candidate.route_point() != Some(candidate_point) {
            return Ok(resolved_unknown(
                PointerReceiverUnknownReason::EventCorrelationUnavailable,
            ));
        }
        let Some(projection) = input
            .receiver_view()
            .interaction_projection(route.surface())
        else {
            return Ok(resolved_unknown(
                PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
            ));
        };
        let Some(graph) = retained.graphs().hover() else {
            return Ok(resolved_unknown(
                PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
            ));
        };
        if graph.native() != route.exact() || graph.scene() != projection.output_ticket() {
            return Ok(resolved_unknown(
                PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
            ));
        }
        let Some(egui_point) = presented_point(edge, route, graph, candidate_point, false) else {
            return Ok(resolved_unknown(
                PointerReceiverUnknownReason::EventCorrelationUnavailable,
            ));
        };
        let PointerReceiverAuthority::Known(hit) = graph.graph().probe(egui_point) else {
            return Ok(resolved_unknown(
                PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
            ));
        };
        probes.push(PointerReceiverProbeReceipt::HoverHit(hover_receipt(
            dockspace,
            input,
            route,
            candidate_point,
            &hit,
        )?));
    }
    let presented = PresentedPointerReceiverObservation::new(probes)
        .map_err(egui_dockspace::DockspaceError::from)?;
    Ok(ResolvedPointerReceiver {
        observation: PointerReceiverObservation::Presented(presented),
        scroll_claim,
    })
}

fn presented_point(
    edge: &NativePointerEdge,
    route: BoundNativeRoute,
    presented: &PresentedNativePointerGraph,
    route_point: LogicalPoint,
    delivery: bool,
) -> Option<Pos2> {
    let graph = presented.graph();
    if graph.viewport_id() != route.exact().viewport() {
        return None;
    }
    let capture = if delivery {
        edge.delivery_coordinates()
    } else {
        edge.hovered_coordinates()
    }
    .value()?;
    if capture.binding() != route.native_binding() {
        return None;
    }
    let native_scale = capture.native_scale_factor();
    let presentation_scale = capture.presentation_scale_factor();
    if !presentation_scales_match(
        native_scale,
        presentation_scale,
        graph.native_pixels_per_point(),
        graph.pixels_per_point(),
    ) {
        return None;
    }
    let position = edge.position().value()?;
    let logical =
        logical_point_from_physical(*position, capture.content_origin(), presentation_scale)?;
    if logical != route_point {
        return None;
    }
    let point = Pos2::new(route_point.x() as f32, route_point.y() as f32);
    point.is_finite().then_some(point)
}

fn presentation_scales_match(
    native_scale: f64,
    presentation_scale: f64,
    graph_native_scale: f32,
    graph_presentation_scale: f32,
) -> bool {
    native_scale.is_finite()
        && native_scale > 0.0
        && presentation_scale.is_finite()
        && presentation_scale > 0.0
        && native_scale as f32 == graph_native_scale
        && presentation_scale as f32 == graph_presentation_scale
}

fn logical_point_from_physical(
    position: NativePhysicalPoint,
    origin: NativePhysicalPoint,
    presentation_scale: f64,
) -> Option<LogicalPoint> {
    let x = f64::from(position.x().checked_sub(origin.x())?) / presentation_scale;
    let y = f64::from(position.y().checked_sub(origin.y())?) / presentation_scale;
    LogicalPoint::new(x, y).ok()
}

fn locked_scroll_delivery(
    dockspace: &Dockspace,
    input: &EguiNativeInputSession,
    route: BoundNativeRoute,
    projection: SurfaceInteractionProjection<'_>,
    graph: &egui::PointerHitGraphSnapshot,
    locked: dockspace::backend::presentation_hit::PresentationHitRegionId,
    probe_point: LogicalPoint,
    projected_delta: Option<dockspace::backend::pointer_journal::FiniteScrollVector>,
) -> Result<PointerReceiverDelivery, NativeRuntimeError> {
    let unknown = PointerReceiverDeliveryDisposition::Unknown(
        PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
    );
    let scroll = if locked.surface() != route.surface()
        || projection.hit_manifest().region(locked).is_none()
    {
        PointerReceiverDeliveryDisposition::NoReceiver
    } else {
        let mut matching = None;
        let mut authority_unavailable = false;
        for receiver in graph.scroll_receivers() {
            match lookup_scroll_receiver(dockspace, input, route, projection, graph, receiver)? {
                PaintReceiverLookup::Dock(kind) if receiver.enabled() && kind == locked.kind() => {
                    if matching.replace(receiver).is_some() {
                        authority_unavailable = true;
                    }
                }
                PaintReceiverLookup::FingerprintMismatch
                | PaintReceiverLookup::GenerationUnavailable => {
                    authority_unavailable = true;
                }
                PaintReceiverLookup::Dock(_) | PaintReceiverLookup::Foreign => {}
            }
        }
        match (matching, authority_unavailable, projected_delta) {
            (Some(_), false, None) => PointerReceiverDeliveryDisposition::Dock(locked),
            (Some(expected), false, Some(projected_delta)) => {
                let point = egui::pos2(probe_point.x() as f32, probe_point.y() as f32);
                match graph
                    .probe_projected_scroll(point, projected_egui_scroll_vector(projected_delta))
                {
                    PointerReceiverAuthority::Known(ScrollProbe::Receiver { receiver, .. })
                        if receiver == expected =>
                    {
                        PointerReceiverDeliveryDisposition::Dock(locked)
                    }
                    PointerReceiverAuthority::Known(ScrollProbe::Receiver { .. })
                    | PointerReceiverAuthority::Known(
                        ScrollProbe::Blocked | ScrollProbe::FrameworkOwned,
                    ) => PointerReceiverDeliveryDisposition::Blocked,
                    PointerReceiverAuthority::Known(ScrollProbe::NoReceiver) => {
                        PointerReceiverDeliveryDisposition::NoReceiver
                    }
                    PointerReceiverAuthority::Known(ScrollProbe::AwaitingDelta)
                    | PointerReceiverAuthority::Unknown(_) => unknown,
                }
            }
            (None, false, _) => PointerReceiverDeliveryDisposition::NoReceiver,
            (_, true, _) => unknown,
        }
    };
    PointerReceiverDelivery::from_lanes_with_scroll(projection, unknown, unknown, scroll)
        .map_err(egui_dockspace::DockspaceError::from)
        .map_err(NativeRuntimeError::from)
}

fn scroll_delivery_receipt(
    dockspace: &Dockspace,
    input: &EguiNativeInputSession,
    route: BoundNativeRoute,
    projection: SurfaceInteractionProjection<'_>,
    point: LogicalPoint,
    scroll_probe_vector: Option<dockspace::backend::pointer_journal::FiniteScrollVector>,
    graph: &egui::PointerHitGraphSnapshot,
    egui_point: Pos2,
    hit: &PointerHit,
) -> Result<PointerReceiverDelivery, NativeRuntimeError> {
    let (click, drag) = pointer_delivery_lanes(dockspace, input, route, projection, point, hit)?;
    let scroll = scroll_delivery_lane(
        dockspace,
        input,
        route,
        projection,
        point,
        scroll_probe_vector,
        graph,
        egui_point,
    )?;
    PointerReceiverDelivery::from_lanes_with_scroll(projection, click, drag, scroll)
        .map_err(egui_dockspace::DockspaceError::from)
        .map_err(NativeRuntimeError::from)
}

fn pointer_delivery_receipt(
    dockspace: &Dockspace,
    input: &EguiNativeInputSession,
    route: BoundNativeRoute,
    projection: SurfaceInteractionProjection<'_>,
    point: LogicalPoint,
    hit: &PointerHit,
) -> Result<PointerReceiverDelivery, NativeRuntimeError> {
    let (click, drag) = pointer_delivery_lanes(dockspace, input, route, projection, point, hit)?;
    PointerReceiverDelivery::from_lanes(projection, click, drag)
        .map_err(egui_dockspace::DockspaceError::from)
        .map_err(NativeRuntimeError::from)
}

fn pointer_delivery_lanes(
    dockspace: &Dockspace,
    input: &EguiNativeInputSession,
    route: BoundNativeRoute,
    projection: SurfaceInteractionProjection<'_>,
    point: LogicalPoint,
    hit: &PointerHit,
) -> Result<
    (
        PointerReceiverDeliveryDisposition,
        PointerReceiverDeliveryDisposition,
    ),
    NativeRuntimeError,
> {
    let click = delivery_lane(
        dockspace,
        input,
        route,
        projection,
        point,
        hit,
        PresentationPointerLane::Click,
        hit.click_receiver,
    )?;
    let drag = delivery_lane(
        dockspace,
        input,
        route,
        projection,
        point,
        hit,
        PresentationPointerLane::Drag,
        hit.drag_receiver,
    )?;
    Ok((click, drag))
}

fn scroll_delivery_lane(
    dockspace: &Dockspace,
    input: &EguiNativeInputSession,
    route: BoundNativeRoute,
    projection: SurfaceInteractionProjection<'_>,
    point: LogicalPoint,
    scroll_probe_vector: Option<dockspace::backend::pointer_journal::FiniteScrollVector>,
    graph: &egui::PointerHitGraphSnapshot,
    egui_point: Pos2,
) -> Result<PointerReceiverDeliveryDisposition, NativeRuntimeError> {
    let probe = match scroll_probe_vector {
        Some(delta) => {
            graph.probe_projected_scroll(egui_point, projected_egui_scroll_vector(delta))
        }
        None => graph.probe_scroll_owner(egui_point),
    };
    let PointerReceiverAuthority::Known(probe) = probe else {
        return Ok(PointerReceiverDeliveryDisposition::Unknown(
            PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
        ));
    };
    let ScrollProbe::Receiver { receiver, .. } = probe else {
        return Ok(match probe {
            ScrollProbe::Blocked | ScrollProbe::FrameworkOwned => {
                PointerReceiverDeliveryDisposition::Blocked
            }
            ScrollProbe::NoReceiver => PointerReceiverDeliveryDisposition::NoReceiver,
            ScrollProbe::AwaitingDelta => PointerReceiverDeliveryDisposition::Unknown(
                PointerReceiverUnknownReason::EventCorrelationUnavailable,
            ),
            ScrollProbe::Receiver { .. } => unreachable!(),
        });
    };
    match lookup_scroll_receiver(dockspace, input, route, projection, graph, receiver)? {
        PaintReceiverLookup::Dock(kind) => {
            Ok(
                region_for_lane(projection, kind, PresentationPointerLane::Scroll, point).map_or(
                    PointerReceiverDeliveryDisposition::Unknown(
                        PointerReceiverUnknownReason::EventCorrelationUnavailable,
                    ),
                    PointerReceiverDeliveryDisposition::Dock,
                ),
            )
        }
        PaintReceiverLookup::Foreign => Ok(PointerReceiverDeliveryDisposition::Blocked),
        PaintReceiverLookup::FingerprintMismatch | PaintReceiverLookup::GenerationUnavailable => {
            Ok(PointerReceiverDeliveryDisposition::Unknown(
                PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
            ))
        }
    }
}

fn projected_egui_scroll_vector(
    vector: dockspace::backend::pointer_journal::FiniteScrollVector,
) -> egui::Vec2 {
    egui::vec2(vector.x().signum() as f32, vector.y().signum() as f32)
}

#[allow(clippy::too_many_arguments)]
fn delivery_lane(
    dockspace: &Dockspace,
    input: &EguiNativeInputSession,
    route: BoundNativeRoute,
    projection: SurfaceInteractionProjection<'_>,
    point: LogicalPoint,
    hit: &PointerHit,
    lane: PresentationPointerLane,
    receiver: Option<WidgetReceiver>,
) -> Result<PointerReceiverDeliveryDisposition, NativeRuntimeError> {
    let Some(receiver) = receiver else {
        return Ok(
            if hit.blocking_layer.is_some() || hit.top_widget.is_some() {
                PointerReceiverDeliveryDisposition::Blocked
            } else {
                PointerReceiverDeliveryDisposition::NoReceiver
            },
        );
    };
    if hit
        .blocking_layer
        .is_some_and(|layer| layer != receiver.layer_id)
    {
        return Ok(PointerReceiverDeliveryDisposition::Blocked);
    }
    match lookup_receiver(dockspace, input, route, projection, hit, receiver)? {
        PaintReceiverLookup::Dock(kind) => Ok(region_for_lane(projection, kind, lane, point)
            .map_or(
                PointerReceiverDeliveryDisposition::Unknown(
                    PointerReceiverUnknownReason::EventCorrelationUnavailable,
                ),
                PointerReceiverDeliveryDisposition::Dock,
            )),
        PaintReceiverLookup::Foreign => Ok(PointerReceiverDeliveryDisposition::Blocked),
        PaintReceiverLookup::FingerprintMismatch | PaintReceiverLookup::GenerationUnavailable => {
            Ok(PointerReceiverDeliveryDisposition::Unknown(
                PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
            ))
        }
    }
}

fn hover_receipt(
    dockspace: &Dockspace,
    input: &EguiNativeInputSession,
    route: BoundNativeRoute,
    point: LogicalPoint,
    hit: &PointerHit,
) -> Result<PointerReceiverHoverHit, NativeRuntimeError> {
    let view = input.receiver_view();
    let projection = view.interaction_projection(route.surface()).ok_or(
        NativeRuntimeError::IngressUnavailable("interactive surface projection disappeared"),
    )?;
    let Some(receiver) = hit.top_widget else {
        let disposition = if hit.blocking_layer.is_some() {
            PointerReceiverHoverHitDisposition::Blocked
        } else {
            PointerReceiverHoverHitDisposition::NoReceiver
        };
        return PointerReceiverHoverHit::new(projection, point, disposition)
            .map_err(egui_dockspace::DockspaceError::from)
            .map_err(NativeRuntimeError::from);
    };
    if hit
        .blocking_layer
        .is_some_and(|layer| layer != receiver.layer_id)
    {
        return PointerReceiverHoverHit::new(
            projection,
            point,
            PointerReceiverHoverHitDisposition::Blocked,
        )
        .map_err(egui_dockspace::DockspaceError::from)
        .map_err(NativeRuntimeError::from);
    }
    match lookup_receiver(dockspace, input, route, projection, hit, receiver)? {
        PaintReceiverLookup::Dock(kind) => {
            let Some(region) =
                region_for_lane(projection, kind, PresentationPointerLane::HoverDrop, point)
            else {
                return Ok(PointerReceiverHoverHit::unknown(
                    PointerReceiverUnknownReason::EventCorrelationUnavailable,
                ));
            };
            let core = view
                .resolve_hover_drop_receiver(route.surface(), point)
                .map_err(egui_dockspace::DockspaceError::from)?;
            if core.disposition() == PointerReceiverHoverHitDisposition::Dock(region) {
                Ok(core)
            } else {
                Ok(PointerReceiverHoverHit::unknown(
                    PointerReceiverUnknownReason::LayerAuthorityUnavailable,
                ))
            }
        }
        PaintReceiverLookup::Foreign => PointerReceiverHoverHit::new(
            projection,
            point,
            PointerReceiverHoverHitDisposition::Blocked,
        )
        .map_err(egui_dockspace::DockspaceError::from)
        .map_err(NativeRuntimeError::from),
        PaintReceiverLookup::FingerprintMismatch | PaintReceiverLookup::GenerationUnavailable => {
            Ok(PointerReceiverHoverHit::unknown(
                PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
            ))
        }
    }
}

fn lookup_receiver(
    dockspace: &Dockspace,
    input: &EguiNativeInputSession,
    route: BoundNativeRoute,
    projection: SurfaceInteractionProjection<'_>,
    hit: &PointerHit,
    receiver: WidgetReceiver,
) -> Result<PaintReceiverLookup, NativeRuntimeError> {
    let fingerprint = PaintReceiverFingerprint::new(
        receiver.id,
        receiver.layer_id,
        receiver.interact_rect,
        receiver.sense,
        receiver.enabled,
    );
    Ok(input.resolve_receiver(
        dockspace,
        route.exact(),
        projection.output_ticket(),
        projection.authority(),
        hit.widget_pass_nr,
        fingerprint,
    )?)
}

fn lookup_scroll_receiver(
    dockspace: &Dockspace,
    input: &EguiNativeInputSession,
    route: BoundNativeRoute,
    projection: SurfaceInteractionProjection<'_>,
    graph: &egui::PointerHitGraphSnapshot,
    receiver: ScrollReceiver,
) -> Result<PaintReceiverLookup, NativeRuntimeError> {
    Ok(input.resolve_receiver(
        dockspace,
        route.exact(),
        projection.output_ticket(),
        projection.authority(),
        graph.widget_pass_nr(),
        PaintReceiverFingerprint::new(
            receiver.id(),
            receiver.layer_id(),
            receiver.interact_rect(),
            Sense::hover(),
            receiver.enabled(),
        ),
    )?)
}

fn region_for_lane(
    projection: SurfaceInteractionProjection<'_>,
    kind: PresentationHitRegionKind,
    lane: PresentationPointerLane,
    point: LogicalPoint,
) -> Option<dockspace::backend::presentation_hit::PresentationHitRegionId> {
    let mut regions = projection.hit_manifest().regions().iter().filter(|region| {
        region.id().kind() == kind && region.lanes().contains(lane) && region.hit().contains(point)
    });
    let region = regions.next()?.id();
    regions.next().is_none().then_some(region)
}

const fn unknown(reason: PointerReceiverUnknownReason) -> PointerReceiverObservation {
    PointerReceiverObservation::Unknown(reason)
}

const fn resolved_unknown(reason: PointerReceiverUnknownReason) -> ResolvedPointerReceiver {
    resolved_unknown_with_claim(reason, None)
}

const fn resolved_unknown_with_claim(
    reason: PointerReceiverUnknownReason,
    scroll_claim: Option<NativeScrollDerivativeClaim>,
) -> ResolvedPointerReceiver {
    ResolvedPointerReceiver {
        observation: unknown(reason),
        scroll_claim,
    }
}

const fn scroll_derivative_claim(edge: &NativePointerEdge) -> NativeScrollDerivativeClaim {
    NativeScrollDerivativeClaim {
        pointer_sequence: edge.sequence(),
    }
}

fn _assert_native_binding_is_copy(_: NativeViewportBinding) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoomed_presentation_uses_effective_scale_for_surface_coordinates() {
        assert!(presentation_scales_match(2.0, 2.5, 2.0, 2.5));
        assert_eq!(
            logical_point_from_physical(
                NativePhysicalPoint::new(350, 450),
                NativePhysicalPoint::new(100, 200),
                2.5,
            ),
            Some(LogicalPoint::new(100.0, 100.0).unwrap())
        );
    }

    #[test]
    fn stale_presentation_scale_fails_closed() {
        assert!(!presentation_scales_match(2.0, 2.0, 2.0, 2.5));
        assert!(!presentation_scales_match(2.0, 2.5, 1.5, 2.5));
    }

    #[test]
    fn receiver_probe_preserves_tiny_nonzero_f64_direction() {
        let vector =
            dockspace::backend::pointer_journal::FiniteScrollVector::new(f64::MAX, f64::MIN_POSITIVE)
                .expect("the core probe vector is finite");

        assert_eq!(projected_egui_scroll_vector(vector), egui::vec2(1.0, 1.0));
    }
}

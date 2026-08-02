//! Exact native pointer receiver evidence.

use std::collections::BTreeMap;

use dockspace::geometry::LogicalPoint;
use dockspace::pointer_receiver::{
    PointerReceiverCandidate, PointerReceiverDelivery, PointerReceiverDeliveryDisposition,
    PointerReceiverHoverHit, PointerReceiverHoverHitDisposition, PointerReceiverObservation,
    PointerReceiverProbe, PointerReceiverProbeReceipt, PointerReceiverReceiptBatch,
    PointerReceiverUnknownReason, PresentedPointerReceiverObservation,
};
use dockspace::presentation_hit::{PresentationHitRegionKind, PresentationPointerLane};
use dockspace::scene::SurfaceInteractionProjection;
use eframe::{
    NativePhysicalPoint, NativePointerEdge, NativePointerEdgeKind, NativePointerSequence,
    NativeViewportBinding,
};
use egui::{
    PointerHit, PointerReceiverAuthority, Pos2, ScrollProbe, ScrollReceiver, WidgetReceiver,
};
use egui_dockspace::{
    Dockspace, EguiNativeInputSession, PaintReceiverFingerprint, PaintReceiverLookup,
};

use crate::NativeRuntimeError;
use crate::ingress::{BoundNativeRoute, RetainedPointerEdge, egui_modifiers};
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
    binding: NativeViewportBinding,
    pointer_sequence: NativePointerSequence,
}

impl NativeScrollDerivativeClaim {
    pub(crate) const fn binding(self) -> NativeViewportBinding {
        self.binding
    }

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
    if !candidate.receiver_is_applicable() {
        return Ok(ResolvedPointerReceiver {
            observation: PointerReceiverObservation::NotApplicable,
            scroll_claim: None,
        });
    }
    let Some(retained) = pointer_edges.get(&candidate.id().sequence().get()) else {
        return Ok(resolved_unknown(
            PointerReceiverUnknownReason::EventCorrelationUnavailable,
        ));
    };
    let edge = retained.edge();
    let mut probes = Vec::new();
    let mut scroll_claim = None;
    if candidate.probes().requires(PointerReceiverProbe::Delivery) {
        let Some(route) = retained.delivery_route() else {
            return Ok(resolved_unknown(
                PointerReceiverUnknownReason::EventCorrelationUnavailable,
            ));
        };
        let Some(projection) = input
            .receiver_view()
            .interaction_projection(route.surface())
        else {
            return Ok(resolved_unknown(
                PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
            ));
        };
        let Some(graph) = retained.graphs().delivery() else {
            return Ok(resolved_unknown(
                PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
            ));
        };
        if graph.native() != route.exact() || graph.scene() != projection.output_ticket() {
            return Ok(resolved_unknown(
                PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
            ));
        }
        let Some(logical_point) = candidate.route_point() else {
            return Ok(resolved_unknown(
                PointerReceiverUnknownReason::EventCorrelationUnavailable,
            ));
        };
        let Some(egui_point) = presented_point(edge, route, graph, logical_point, true) else {
            return Ok(resolved_unknown(
                PointerReceiverUnknownReason::EventCorrelationUnavailable,
            ));
        };
        let PointerReceiverAuthority::Known(hit) = graph.graph().probe(egui_point) else {
            return Ok(resolved_unknown(
                PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
            ));
        };
        let delivery = delivery_receipt(
            dockspace,
            input,
            route,
            projection,
            logical_point,
            edge,
            graph.graph(),
            egui_point,
            &hit,
        )?;
        if matches!(edge.kind(), NativePointerEdgeKind::Scrolled(_))
            && matches!(
                delivery.scroll(),
                PointerReceiverDeliveryDisposition::Dock(_)
            )
        {
            scroll_claim = Some(NativeScrollDerivativeClaim {
                binding: route.native_binding(),
                pointer_sequence: edge.sequence(),
            });
        }
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

fn delivery_receipt(
    dockspace: &Dockspace,
    input: &EguiNativeInputSession,
    route: BoundNativeRoute,
    projection: SurfaceInteractionProjection<'_>,
    point: LogicalPoint,
    edge: &NativePointerEdge,
    graph: &egui::PointerHitGraphSnapshot,
    egui_point: Pos2,
    hit: &PointerHit,
) -> Result<PointerReceiverDelivery, NativeRuntimeError> {
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
    let scroll = scroll_delivery_lane(
        dockspace, input, route, projection, point, edge, graph, egui_point,
    )?;
    PointerReceiverDelivery::from_lanes_with_scroll(projection, click, drag, scroll)
        .map_err(egui_dockspace::DockspaceError::from)
        .map_err(NativeRuntimeError::from)
}

fn scroll_delivery_lane(
    dockspace: &Dockspace,
    input: &EguiNativeInputSession,
    route: BoundNativeRoute,
    projection: SurfaceInteractionProjection<'_>,
    point: LogicalPoint,
    edge: &NativePointerEdge,
    graph: &egui::PointerHitGraphSnapshot,
    egui_point: Pos2,
) -> Result<PointerReceiverDeliveryDisposition, NativeRuntimeError> {
    let Some((delta, modifiers)) = native_scroll_probe_input(edge) else {
        return Ok(PointerReceiverDeliveryDisposition::Unknown(
            PointerReceiverUnknownReason::EventCorrelationUnavailable,
        ));
    };
    let PointerReceiverAuthority::Known(probe) = graph.probe_scroll(egui_point, delta, modifiers)
    else {
        return Ok(PointerReceiverDeliveryDisposition::Unknown(
            PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
        ));
    };
    let ScrollProbe::Receiver { receiver, .. } = probe else {
        return Ok(match probe {
            ScrollProbe::Blocked | ScrollProbe::FrameworkOwned => {
                PointerReceiverDeliveryDisposition::Blocked
            }
            ScrollProbe::NoReceiver | ScrollProbe::AwaitingDelta => {
                PointerReceiverDeliveryDisposition::NoReceiver
            }
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

fn native_scroll_probe_input(edge: &NativePointerEdge) -> Option<(egui::Vec2, egui::Modifiers)> {
    let NativePointerEdgeKind::Scrolled(scroll) = edge.kind() else {
        return None;
    };
    let vector = scroll.delta().map_or(egui::Vec2::ZERO, |delta| {
        let vector = delta.vector();
        egui::vec2(vector.x() as f32, vector.y() as f32)
    });
    let modifiers = scroll.modifiers().value().copied()?;
    Some((vector, egui_modifiers(modifiers)))
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
        PaintReceiverFingerprint::from_scroll_receiver(receiver),
    )?)
}

fn region_for_lane(
    projection: SurfaceInteractionProjection<'_>,
    kind: PresentationHitRegionKind,
    lane: PresentationPointerLane,
    point: LogicalPoint,
) -> Option<dockspace::presentation_hit::PresentationHitRegionId> {
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
    ResolvedPointerReceiver {
        observation: unknown(reason),
        scroll_claim: None,
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
}

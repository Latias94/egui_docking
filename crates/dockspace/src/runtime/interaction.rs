//! Opaque renderer-facing measurement, paint, and pointer capabilities.

use thiserror::Error;

use super::{DockspaceHostFrame, DockspaceRuntimeError, DockspaceSession};
use crate::drop_target::DropTargetId;
use crate::geometry::{LogicalPoint, LogicalRect, LogicalSize, PhysicalRect};
use crate::ids::{ItemId, SurfaceId};
use crate::intent::{Authority, PointerButton, PointerId};
use crate::interaction::{InteractionPreview, PreviewVisual};
use crate::pointer_journal::{
    PointerCaptureOwner, PointerEdge, PointerEdgeJournal, PointerEdgeKind, PointerEdgeLocation,
    PointerEdgeSequence, PointerInputLease, PointerProviderScope, SurfaceLocalPointerEndpoint,
    SurfaceLocalPointerScope,
};
use crate::pointer_receiver::{
    PointerReceiverCandidate, PointerReceiverDelivery, PointerReceiverDeliveryDisposition,
    PointerReceiverHoverHit, PointerReceiverHoverHitDisposition, PointerReceiverObservation,
    PointerReceiverProbeReceipt, PointerReceiverReceiptBatch, PointerReceiverUnknownReason,
    PresentedPointerReceiverObservation,
};
use crate::presentation_hit::{PresentationHitRegionId, PresentationHitRegionKind};
use crate::presentation_observation::{PresentedSurfaceAuthority, SurfacePresentationOutputTicket};
use crate::scene::{PresentationPlan, SurfaceScene};
use crate::scene_manifest::{
    Measurement, MeasurementUnavailableReason, SurfaceMeasurements, TabIntrinsic, TabStripMetrics,
};

/// Uniform measurements for a renderer whose panes and tabs share one metric.
///
/// This deliberately hides manifest keys. Rich adapters can later use the same
/// facade with semantic per-item callbacks without gaining access to core
/// scene stamps or requirement identities.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UniformSurfaceMetrics {
    bounds: LogicalRect,
    pane_minimum: LogicalSize,
    tab_intrinsic: TabIntrinsic,
    tab_strip: TabStripMetrics,
}

impl UniformSurfaceMetrics {
    /// Validates one complete uniform measurement profile.
    ///
    /// # Errors
    ///
    /// Returns an error when the tab content width is negative or non-finite.
    pub fn new(
        bounds: LogicalRect,
        pane_minimum: LogicalSize,
        tab_content_width: f64,
    ) -> Result<Self, DockspaceInteractionError> {
        let tab_intrinsic = TabIntrinsic::new(tab_content_width)
            .map_err(|_| DockspaceInteractionError::InvalidMeasurementProfile)?;
        let tab_strip = TabStripMetrics::new(0.0, 0.0)
            .map_err(|_| DockspaceInteractionError::InvalidMeasurementProfile)?;
        Ok(Self {
            bounds,
            pane_minimum,
            tab_intrinsic,
            tab_strip,
        })
    }
}

/// Opaque receiver descriptor captured while painting one semantic output.
///
/// A descriptor is not input authority. It must be rebound after a concrete
/// output is finally presented before an event may name it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DockspaceReceiverDescriptor {
    output: SurfacePresentationOutputTicket,
    region: PresentationHitRegionId,
    bounds: LogicalRect,
    center: LogicalPoint,
}

impl DockspaceReceiverDescriptor {
    /// Returns the exact receiver rectangle supplied to the renderer.
    #[must_use]
    pub const fn bounds(self) -> LogicalRect {
        self.bounds
    }

    /// Returns the center of the exact half-open receiver rectangle.
    #[must_use]
    pub const fn center(self) -> LogicalPoint {
        self.center
    }
}

/// Exact presented surface capability used to qualify known-empty facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentedDockspaceSurface {
    surface: SurfaceId,
    output: SurfacePresentationOutputTicket,
    authority: PresentedSurfaceAuthority,
}

/// Exact framework receiver bound to one concrete final-presentation output.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PresentedDockReceiver {
    descriptor: DockspaceReceiverDescriptor,
    authority: PresentedSurfaceAuthority,
}

impl PresentedDockReceiver {
    /// Returns the exact receiver rectangle presented by the renderer.
    #[must_use]
    pub const fn bounds(self) -> LogicalRect {
        self.descriptor.bounds
    }

    /// Returns the receiver center in surface-local logical coordinates.
    #[must_use]
    pub const fn center(self) -> LogicalPoint {
        self.descriptor.center()
    }
}

/// Stable renderer-facing preview geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DockspacePreviewVisual {
    /// Highlight one exact docking rectangle.
    Dock {
        /// Surface which paints the highlight.
        surface: SurfaceId,
        /// Final logical highlight geometry.
        rect: LogicalRect,
    },
    /// Paint one contained-floating placement.
    Contained {
        /// Host surface.
        surface: SurfaceId,
        /// Final logical placement.
        rect: LogicalRect,
        /// Whether this is an explicitly enabled native fallback.
        fallback: bool,
    },
    /// Paint a source-hosted cue for a future native surface.
    Native {
        /// Existing surface which paints the cue.
        host_surface: SurfaceId,
        /// Reserved future surface.
        target_surface: SurfaceId,
        /// Final desktop-physical placement.
        placement: PhysicalRect,
    },
}

/// Borrowed preview which belongs to one exact paint plan.
#[derive(Debug, Clone, Copy)]
pub struct DockspaceDragPreview<'plan> {
    preview: &'plan InteractionPreview,
}

impl DockspaceDragPreview<'_> {
    /// Returns the renderer-neutral geometry which must be painted.
    #[must_use]
    pub fn visual(self) -> DockspacePreviewVisual {
        match *self.preview.visual() {
            PreviewVisual::Dock { surface, rect, .. } => {
                DockspacePreviewVisual::Dock { surface, rect }
            }
            PreviewVisual::Contained {
                surface,
                rect,
                fallback,
            } => DockspacePreviewVisual::Contained {
                surface,
                rect,
                fallback,
            },
            PreviewVisual::Native {
                host_surface,
                target_surface,
                placement,
            } => DockspacePreviewVisual::Native {
                host_surface,
                target_surface,
                placement,
            },
        }
    }
}

/// Read-only plan supplied before one exact renderer paint.
#[derive(Debug, Clone, Copy)]
pub struct SurfacePaintPlan<'frame> {
    surface: SurfaceId,
    output: SurfacePresentationOutputTicket,
    plan: &'frame PresentationPlan,
    hit_manifest: &'frame crate::presentation_hit::PresentationHitManifest,
    drag_preview: Option<&'frame InteractionPreview>,
}

impl<'frame> SurfacePaintPlan<'frame> {
    /// Returns the logical surface painted by this plan.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    /// Returns the exact surface bounds.
    #[must_use]
    pub fn bounds(self) -> LogicalRect {
        self.plan.bounds()
    }

    /// Returns the exact receiver descriptor for one visible tab.
    #[must_use]
    pub fn tab_receiver(self, item: ItemId) -> Option<DockspaceReceiverDescriptor> {
        self.receiver(
            |kind| matches!(kind, PresentationHitRegionKind::TabBody(tab) if tab.item == item),
        )
    }

    /// Returns the exact center-drop receiver for the stack containing `item`.
    #[must_use]
    pub fn center_drop_receiver_for_item(
        self,
        item: ItemId,
    ) -> Option<DockspaceReceiverDescriptor> {
        let tabs = self
            .plan
            .tab_records()
            .iter()
            .find(|record| record.id().item == item)?
            .id()
            .tabs;
        self.receiver(|kind| {
            matches!(
                kind,
                PresentationHitRegionKind::DropTarget(DropTargetId::Center {
                    tabs: target,
                    ..
                }) if target == tabs
            )
        })
    }

    /// Returns the exact transient drag preview included in this paint.
    #[must_use]
    pub fn drag_preview(self) -> Option<DockspaceDragPreview<'frame>> {
        self.drag_preview
            .map(|preview| DockspaceDragPreview { preview })
    }

    fn receiver(
        self,
        matches: impl Fn(PresentationHitRegionKind) -> bool,
    ) -> Option<DockspaceReceiverDescriptor> {
        let region = self
            .hit_manifest
            .regions()
            .iter()
            .copied()
            .find(|region| !region.is_passive() && matches(region.id().kind()))?;
        Some(DockspaceReceiverDescriptor {
            output: self.output,
            region: region.id(),
            bounds: region.hit().rect(),
            center: LogicalPoint::new(
                region.hit().rect().x() + region.hit().rect().width() * 0.5,
                region.hit().rect().y() + region.hit().rect().height() * 0.5,
            )
            .ok()?,
        })
    }
}

/// One surface-local primary-pointer edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfacePointerEvent {
    /// Primary button press.
    PrimaryPressed,
    /// Pointer motion while the provider owns the stream.
    Moved,
    /// Primary button release.
    PrimaryReleased,
}

#[derive(Debug, Clone, Copy)]
enum ReceiverFact<'receiver> {
    Dock(&'receiver PresentedDockReceiver),
    NoReceiver(&'receiver PresentedDockspaceSurface),
    Blocked(&'receiver PresentedDockspaceSurface),
    Unknown,
}

/// Independent framework delivery and hover facts for one pointer edge.
#[derive(Debug, Clone, Copy)]
pub struct SurfacePointerReceiverFacts<'receiver> {
    delivery: ReceiverFact<'receiver>,
    hover: ReceiverFact<'receiver>,
}

impl<'receiver> SurfacePointerReceiverFacts<'receiver> {
    /// Reports no authoritative receiver fact for either lane.
    #[must_use]
    pub const fn unknown() -> Self {
        Self {
            delivery: ReceiverFact::Unknown,
            hover: ReceiverFact::Unknown,
        }
    }

    /// Reports the exact framework receiver which accepted delivery.
    #[must_use]
    pub const fn delivery(receiver: &'receiver PresentedDockReceiver) -> Self {
        Self {
            delivery: ReceiverFact::Dock(receiver),
            hover: ReceiverFact::Unknown,
        }
    }

    /// Reports the exact point-bound docking receiver under an active drag.
    #[must_use]
    pub const fn hover(receiver: &'receiver PresentedDockReceiver) -> Self {
        Self {
            delivery: ReceiverFact::Unknown,
            hover: ReceiverFact::Dock(receiver),
        }
    }

    /// Reports an authoritative known-empty hover result for the current output.
    #[must_use]
    pub const fn no_hover(surface: &'receiver PresentedDockspaceSurface) -> Self {
        Self {
            delivery: ReceiverFact::Unknown,
            hover: ReceiverFact::NoReceiver(surface),
        }
    }

    /// Reports that a higher framework layer blocked hover-drop delivery.
    #[must_use]
    pub const fn blocked_hover(surface: &'receiver PresentedDockspaceSurface) -> Self {
        Self {
            delivery: ReceiverFact::Unknown,
            hover: ReceiverFact::Blocked(surface),
        }
    }
}

/// Failure at the renderer-neutral interaction boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DockspaceInteractionError {
    /// One public measurement profile contained an invalid scalar.
    #[error("surface measurement profile contains an invalid scalar")]
    InvalidMeasurementProfile,
    /// The requested surface is outside the current frame roster.
    #[error("surface {surface} is outside the current host-frame roster")]
    SurfaceOutsideRoster {
        /// Rejected surface.
        surface: SurfaceId,
    },
    /// A surface already supplied a measurement or paint answer.
    #[error("surface {surface} already has a contribution or paint answer")]
    SurfaceAlreadyAnswered {
        /// Duplicated surface.
        surface: SurfaceId,
    },
    /// The current surface has no Ready candidate eligible for paint.
    #[error("surface {surface} has no current paintable candidate")]
    SurfaceNotPaintable {
        /// Requested surface.
        surface: SurfaceId,
    },
    /// The uniform writer disagreed with the exact core manifest.
    #[error("uniform measurement writer could not answer the core manifest exactly")]
    MeasurementRosterInvariant,
    /// A pointer provider is already active for another surface.
    #[error("a surface-local pointer provider is already active for another surface")]
    PointerProviderAlreadyActive,
    /// The requested surface does not yet have final-presentation authority.
    #[error("surface {surface} has no final-presentation authority")]
    PresentationAuthorityUnavailable {
        /// Requested surface.
        surface: SurfaceId,
    },
    /// No facade-owned pointer provider is active.
    #[error("no facade-owned surface pointer provider is active")]
    PointerProviderUnavailable,
    /// The provider-owned pointer sequence cannot advance without wrapping.
    #[error("surface pointer sequence is exhausted")]
    PointerSequenceExhausted,
    /// A core constructor rejected facade-owned canonical pointer data.
    #[error("facade-owned pointer protocol data violated an internal invariant")]
    PointerProtocolInvariant,
    /// A surface-local pointer batch was already supplied for this host frame.
    #[error("surface pointer input was already submitted for this host frame")]
    PointerInputAlreadySubmitted,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct RuntimePointerState {
    lease: PointerInputLease,
    surface: SurfaceId,
    committed_sequence: u64,
}

impl RuntimePointerState {
    pub(super) const fn committed_sequence(self) -> u64 {
        self.committed_sequence
    }

    pub(super) fn commit_sequence(&mut self, sequence: u64) {
        self.committed_sequence = sequence;
    }
}

impl DockspaceSession {
    /// Consumes one actual renderer result for a previously painted output.
    ///
    /// The confirmation is submitted at the next host-frame prelude; it does
    /// not grant interaction authority synchronously.
    ///
    /// # Errors
    ///
    /// Returns an error when the capability is foreign, stale, already
    /// consumed, or conflicts with another pending confirmation.
    pub fn confirm_presented(
        &mut self,
        output: super::PaintedSurfaceOutput,
    ) -> Result<(), DockspaceRuntimeError> {
        self.presentation.confirm_presented(output)?;
        Ok(())
    }

    /// Enables one logical surface as the sole surface-local pointer endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error until an exact output for `surface` has crossed the
    /// final-presentation observation boundary.
    pub fn enable_surface_pointer(
        &mut self,
        surface: SurfaceId,
    ) -> Result<(), DockspaceRuntimeError> {
        if let Some(pointer) = self.pointer {
            return if pointer.surface == surface {
                Ok(())
            } else {
                Err(DockspaceInteractionError::PointerProviderAlreadyActive.into())
            };
        }
        if self.engine.interaction_projection(surface).is_none() {
            return Err(
                DockspaceInteractionError::PresentationAuthorityUnavailable { surface }.into(),
            );
        }
        let lease = self.engine.create_pointer_provider(
            PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(
                self.presentation_host,
                SurfaceLocalPointerEndpoint::Logical(surface),
            )),
            PointerEdgeSequence::new(0),
        )?;
        self.pointer = Some(RuntimePointerState {
            lease,
            surface,
            committed_sequence: 0,
        });
        Ok(())
    }

    /// Returns the current exact presented surface capability.
    #[must_use]
    pub fn presented_surface(&self, surface: SurfaceId) -> Option<PresentedDockspaceSurface> {
        let projection = self.engine.interaction_projection(surface)?;
        Some(PresentedDockspaceSurface {
            surface,
            output: projection.output_ticket(),
            authority: projection.authority(),
        })
    }

    /// Binds a paint-time descriptor to the exact currently presented output.
    #[must_use]
    pub fn bind_presented_receiver(
        &self,
        descriptor: &DockspaceReceiverDescriptor,
    ) -> Option<PresentedDockReceiver> {
        let projection = self
            .engine
            .interaction_projection(descriptor.region.surface())?;
        (projection.output_ticket() == descriptor.output
            && projection
                .hit_manifest()
                .region(descriptor.region)
                .is_some())
        .then_some(PresentedDockReceiver {
            descriptor: *descriptor,
            authority: projection.authority(),
        })
    }
}

impl DockspaceHostFrame<'_> {
    /// Supplies one complete uniform measurement answer for a frozen surface.
    ///
    /// # Errors
    ///
    /// Returns an error when the surface is outside the frozen roster, already
    /// answered, or the measurements cannot satisfy the core manifest.
    pub fn measure_surface(
        &mut self,
        surface: SurfaceId,
        metrics: UniformSurfaceMetrics,
    ) -> Result<(), DockspaceRuntimeError> {
        self.complete_pointer_input()?;
        if self.surface_answered(surface) {
            return Err(DockspaceInteractionError::SurfaceAlreadyAnswered { surface }.into());
        }
        let requirements = self
            .frame
            .view()
            .presentation_requirements()
            .surface(surface)
            .ok_or(DockspaceInteractionError::SurfaceOutsideRoster { surface })?;
        let mut measurements = SurfaceMeasurements::new(requirements.ticket());
        measurements
            .set_bounds(requirements.bounds(), Measurement::Measured(metrics.bounds))
            .map_err(|_| DockspaceInteractionError::MeasurementRosterInvariant)?;
        if let Some(key) = requirements.popup_plane_bounds() {
            measurements
                .set_popup_plane_bounds(key, Measurement::Measured(metrics.bounds))
                .map_err(|_| DockspaceInteractionError::MeasurementRosterInvariant)?;
        }
        for key in requirements.pane_minimums() {
            measurements
                .insert_pane_minimum(key, Measurement::Measured(metrics.pane_minimum))
                .map_err(|_| DockspaceInteractionError::MeasurementRosterInvariant)?;
        }
        for key in requirements.tab_intrinsics() {
            measurements
                .insert_tab_intrinsic(key, Measurement::Measured(metrics.tab_intrinsic))
                .map_err(|_| DockspaceInteractionError::MeasurementRosterInvariant)?;
        }
        for key in requirements.tab_strips() {
            measurements
                .insert_tab_strip(key, Measurement::Measured(metrics.tab_strip))
                .map_err(|_| DockspaceInteractionError::MeasurementRosterInvariant)?;
        }
        let token = self.frame.view().begin_surface_contribution(surface)?;
        let contribution = self
            .frame
            .view()
            .prepare_surface_contribution(token, measurements)?;
        self.frame.push_surface_contribution(contribution)?;
        Ok(())
    }

    /// Freezes pointer input and returns the current candidate plan which the
    /// renderer may paint.
    ///
    /// # Errors
    ///
    /// Returns an error when the pointer-input phase cannot be completed.
    pub fn paint_plan(
        &mut self,
        surface: SurfaceId,
    ) -> Result<Option<SurfacePaintPlan<'_>>, DockspaceRuntimeError> {
        self.complete_pointer_input()?;
        let view = self.frame.view();
        let Some(scene) = view.scene().surface(surface).and_then(SurfaceScene::ready) else {
            return Ok(None);
        };
        let candidate = scene.candidate();
        Ok(Some(SurfacePaintPlan {
            surface,
            output: candidate.output_ticket(),
            plan: candidate.plan(),
            hit_manifest: candidate.hit_manifest(),
            drag_preview: view.presentation_drag_preview(surface),
        }))
    }

    /// Records that the renderer painted the complete current plan, including
    /// every transient preview exposed by [`Self::paint_plan`].
    ///
    /// # Errors
    ///
    /// Returns an error when the surface was already answered or has no
    /// current paintable plan.
    pub fn confirm_surface_painted(
        &mut self,
        surface: SurfaceId,
    ) -> Result<(), DockspaceRuntimeError> {
        if self.surface_answered(surface) {
            return Err(DockspaceInteractionError::SurfaceAlreadyAnswered { surface }.into());
        }
        if self.paint_plan(surface)?.is_none() {
            return Err(DockspaceInteractionError::SurfaceNotPaintable { surface }.into());
        }
        self.painted_surfaces.insert(surface);
        Ok(())
    }

    /// Explicitly settles every unanswered surface by retaining a current
    /// Ready candidate or reporting the supplied unavailability reason.
    ///
    /// # Errors
    ///
    /// Returns an error when any contribution is stale, duplicated, or cannot
    /// satisfy the frozen surface roster.
    pub fn complete_unpainted_surfaces(
        &mut self,
        reason: MeasurementUnavailableReason,
    ) -> Result<(), DockspaceRuntimeError> {
        self.complete_pointer_input()?;
        let surfaces = self.frame.surfaces().collect::<Vec<_>>();
        for surface in surfaces {
            if self.surface_answered(surface) {
                continue;
            }
            let token = self.frame.view().begin_surface_contribution(surface)?;
            let contribution = if matches!(
                self.frame.view().scene().surface(surface),
                Some(SurfaceScene::Ready(_))
            ) {
                self.frame
                    .view()
                    .prepare_surface_retained_contribution(token)?
            } else {
                self.frame
                    .view()
                    .prepare_surface_unavailable_contribution(token, reason)?
            };
            self.frame.push_surface_contribution(contribution)?;
        }
        Ok(())
    }

    /// Submits one lossless surface-local primary pointer edge and its exact
    /// framework receiver facts.
    ///
    /// # Errors
    ///
    /// Returns an error when no pointer provider is active, the sequence is
    /// exhausted, or the edge and receiver facts violate the frozen protocol.
    pub fn submit_surface_pointer(
        &mut self,
        event: SurfacePointerEvent,
        position: LogicalPoint,
        facts: SurfacePointerReceiverFacts<'_>,
    ) -> Result<(), DockspaceRuntimeError> {
        if self.pointer_input_submitted {
            return Err(DockspaceInteractionError::PointerInputAlreadySubmitted.into());
        }
        let pointer = self
            .session
            .pointer
            .ok_or(DockspaceInteractionError::PointerProviderUnavailable)?;
        let previous = self
            .next_pointer_sequence
            .ok_or(DockspaceInteractionError::PointerProviderUnavailable)?;
        let sequence = previous
            .checked_add(1)
            .ok_or(DockspaceInteractionError::PointerSequenceExhausted)?;
        let kind = match event {
            SurfacePointerEvent::PrimaryPressed => {
                PointerEdgeKind::ButtonPressed(PointerButton::Primary)
            }
            SurfacePointerEvent::Moved => PointerEdgeKind::Moved,
            SurfacePointerEvent::PrimaryReleased => {
                PointerEdgeKind::ButtonReleased(PointerButton::Primary)
            }
        };
        let capture = match event {
            SurfacePointerEvent::PrimaryReleased => PointerCaptureOwner::None,
            SurfacePointerEvent::PrimaryPressed | SurfacePointerEvent::Moved => {
                PointerCaptureOwner::ProviderEndpoint
            }
        };
        let edge_sequence = PointerEdgeSequence::new(sequence);
        let journal = PointerEdgeJournal::new(
            PointerEdgeSequence::new(previous),
            edge_sequence,
            vec![PointerEdge::new(
                edge_sequence,
                PointerId::new(1),
                kind,
                PointerEdgeLocation::SurfaceLocal {
                    position: Authority::Known(position),
                },
                Authority::Known(capture),
            )],
        )
        .map_err(|_| DockspaceInteractionError::PointerProtocolInvariant)?;
        self.frame.submit_pointer_journal(pointer.lease, journal)?;
        let candidates = self
            .frame
            .pointer_receiver_candidates()
            .ok_or(DockspaceInteractionError::PointerProtocolInvariant)?;
        let [candidate] = candidates.candidates() else {
            return Err(DockspaceInteractionError::PointerProtocolInvariant.into());
        };
        let observation = self.pointer_observation(pointer.surface, candidate, facts)?;
        let receipts = PointerReceiverReceiptBatch::new([candidate.receipt(observation)])
            .map_err(|_| DockspaceInteractionError::PointerProtocolInvariant)?;
        self.frame.submit_pointer_receiver_receipts(receipts)?;
        self.next_pointer_sequence = Some(sequence);
        self.pointer_input_submitted = true;
        Ok(())
    }

    pub(super) fn complete_pointer_input(&mut self) -> Result<(), DockspaceRuntimeError> {
        if self.pointer_input_submitted || self.session.pointer.is_none() {
            return Ok(());
        }
        let pointer = self
            .session
            .pointer
            .ok_or(DockspaceInteractionError::PointerProviderUnavailable)?;
        let sequence = self
            .next_pointer_sequence
            .ok_or(DockspaceInteractionError::PointerProviderUnavailable)?;
        let watermark = PointerEdgeSequence::new(sequence);
        let journal = PointerEdgeJournal::new(watermark, watermark, Vec::new())
            .map_err(|_| DockspaceInteractionError::PointerProtocolInvariant)?;
        self.frame.submit_pointer_journal(pointer.lease, journal)?;
        let candidates = self
            .frame
            .pointer_receiver_candidates()
            .ok_or(DockspaceInteractionError::PointerProtocolInvariant)?;
        if !candidates.candidates().is_empty() {
            return Err(DockspaceInteractionError::PointerProtocolInvariant.into());
        }
        let receipts = PointerReceiverReceiptBatch::new(Vec::new())
            .map_err(|_| DockspaceInteractionError::PointerProtocolInvariant)?;
        self.frame.submit_pointer_receiver_receipts(receipts)?;
        self.pointer_input_submitted = true;
        Ok(())
    }

    fn pointer_observation(
        &self,
        surface: SurfaceId,
        candidate: &PointerReceiverCandidate,
        facts: SurfacePointerReceiverFacts<'_>,
    ) -> Result<PointerReceiverObservation, DockspaceRuntimeError> {
        if candidate.probes().is_not_applicable() {
            return Ok(PointerReceiverObservation::NotApplicable);
        }
        let projection = self.frame.view().interaction_projection(surface);
        let mut probes = Vec::with_capacity(candidate.probes().probes().len());
        if candidate
            .probes()
            .requires(crate::pointer_receiver::PointerReceiverProbe::Delivery)
        {
            probes.push(PointerReceiverProbeReceipt::Delivery(delivery_fact(
                projection.as_ref(),
                facts.delivery,
            )?));
        }
        if candidate
            .probes()
            .requires(crate::pointer_receiver::PointerReceiverProbe::HoverHit)
        {
            probes.push(PointerReceiverProbeReceipt::HoverHit(hover_fact(
                projection.as_ref(),
                candidate.hover_point(),
                facts.hover,
            )?));
        }
        let presented = PresentedPointerReceiverObservation::new(probes)
            .map_err(|_| DockspaceInteractionError::PointerProtocolInvariant)?;
        Ok(PointerReceiverObservation::Presented(presented))
    }

    fn surface_answered(&self, surface: SurfaceId) -> bool {
        self.painted_surfaces.contains(&surface)
            || self
                .frame
                .surface_contributions()
                .iter()
                .any(|contribution| contribution.surface() == surface)
    }
}

fn delivery_fact(
    projection: Option<&crate::scene::SurfaceInteractionProjection<'_>>,
    fact: ReceiverFact<'_>,
) -> Result<PointerReceiverDelivery, DockspaceRuntimeError> {
    let Some(projection) = projection else {
        return Ok(PointerReceiverDelivery::unknown(
            PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
        ));
    };
    let disposition = match fact {
        ReceiverFact::Dock(receiver) if receiver_matches(receiver, projection) => {
            PointerReceiverDeliveryDisposition::Dock(receiver.descriptor.region)
        }
        ReceiverFact::NoReceiver(surface) if surface_matches(surface, projection) => {
            PointerReceiverDeliveryDisposition::NoReceiver
        }
        ReceiverFact::Blocked(surface) if surface_matches(surface, projection) => {
            PointerReceiverDeliveryDisposition::Blocked
        }
        ReceiverFact::Dock(_)
        | ReceiverFact::NoReceiver(_)
        | ReceiverFact::Blocked(_)
        | ReceiverFact::Unknown => {
            return Ok(PointerReceiverDelivery::unknown(
                PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
            ));
        }
    };
    PointerReceiverDelivery::new(*projection, disposition)
        .map_err(|_| DockspaceInteractionError::PointerProtocolInvariant.into())
}

fn hover_fact(
    projection: Option<&crate::scene::SurfaceInteractionProjection<'_>>,
    point: Option<LogicalPoint>,
    fact: ReceiverFact<'_>,
) -> Result<PointerReceiverHoverHit, DockspaceRuntimeError> {
    let Some(projection) = projection else {
        return Ok(PointerReceiverHoverHit::unknown(
            PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
        ));
    };
    let Some(point) = point else {
        return Ok(PointerReceiverHoverHit::unknown(
            PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
        ));
    };
    let disposition = match fact {
        ReceiverFact::Dock(receiver) if receiver_matches(receiver, projection) => {
            PointerReceiverHoverHitDisposition::Dock(receiver.descriptor.region)
        }
        ReceiverFact::NoReceiver(surface) if surface_matches(surface, projection) => {
            PointerReceiverHoverHitDisposition::NoReceiver
        }
        ReceiverFact::Blocked(surface) if surface_matches(surface, projection) => {
            PointerReceiverHoverHitDisposition::Blocked
        }
        ReceiverFact::Dock(_)
        | ReceiverFact::NoReceiver(_)
        | ReceiverFact::Blocked(_)
        | ReceiverFact::Unknown => {
            return Ok(PointerReceiverHoverHit::unknown(
                PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
            ));
        }
    };
    PointerReceiverHoverHit::new(*projection, point, disposition)
        .map_err(|_| DockspaceInteractionError::PointerProtocolInvariant.into())
}

fn receiver_matches(
    receiver: &PresentedDockReceiver,
    projection: &crate::scene::SurfaceInteractionProjection<'_>,
) -> bool {
    receiver.descriptor.output == projection.output_ticket()
        && receiver.authority == projection.authority()
        && projection
            .hit_manifest()
            .region(receiver.descriptor.region)
            .is_some()
}

fn surface_matches(
    surface: &PresentedDockspaceSurface,
    projection: &crate::scene::SurfaceInteractionProjection<'_>,
) -> bool {
    surface.surface == projection.output_ticket().surface()
        && surface.output == projection.output_ticket()
        && surface.authority == projection.authority()
}

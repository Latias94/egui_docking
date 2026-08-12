//! Opaque renderer-facing pointer capabilities.

use thiserror::Error;

use super::{
    DockspaceHostFrame, DockspaceReceiverDescriptor, DockspaceRuntimeError, DockspaceSession,
    SurfacePaintPlan, SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use crate::engine::EngineError;
use crate::geometry::{LogicalPoint, LogicalRect};
use crate::ids::SurfaceId;
use crate::intent::{Authority, AuthorityUnavailableReason, PointerButton, PointerId};
use crate::pointer_journal::{
    FiniteScrollVector, PhysicalScrollCoordinates, PointerCaptureOwner, PointerEdge,
    PointerEdgeJournal, PointerEdgeKind, PointerEdgeLocation, PointerEdgeSequence,
    PointerStreamCancelReason, ScrollCancelReason, ScrollDeliveryEndpoint, ScrollDelta,
    ScrollDeviceId, ScrollEdge, ScrollModifiers, ScrollMomentum, ScrollPhase, ScrollSequenceToken,
    SurfaceLocalPointerProvider,
};
use crate::pointer_receiver::{
    PointerReceiverCandidate, PointerReceiverDelivery, PointerReceiverDeliveryDisposition,
    PointerReceiverHoverHit, PointerReceiverHoverHitDisposition, PointerReceiverObservation,
    PointerReceiverProbeReceipt, PointerReceiverReceiptBatch, PointerReceiverUnknownReason,
    PresentedPointerReceiverObservation,
};
use crate::presentation_observation::{PresentedSurfaceAuthority, SurfacePresentationOutputTicket};
use crate::scene::SurfaceScene;

/// Exact presented surface capability used to qualify known-empty facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentedDockspaceSurface {
    surface: SurfaceId,
    output: SurfacePresentationOutputTicket,
    authority: PresentedSurfaceAuthority,
}

impl PresentedDockspaceSurface {
    pub(super) const fn from_projection(
        projection: crate::scene::SurfaceInteractionProjection<'_>,
    ) -> Self {
        Self {
            surface: projection.output_ticket().surface(),
            output: projection.output_ticket(),
            authority: projection.authority(),
        }
    }

    /// Returns the logical surface authorized by this presentation proof.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    pub(super) fn bind_receiver(
        self,
        descriptor: &DockspaceReceiverDescriptor,
    ) -> Option<PresentedDockReceiver> {
        (descriptor.output == self.output && descriptor.output.surface() == self.surface).then_some(
            PresentedDockReceiver {
                descriptor: *descriptor,
                authority: self.authority,
            },
        )
    }

    pub(super) fn matches_native_binding(
        self,
        binding: super::native::NativeSurfaceBinding,
    ) -> bool {
        match self.authority.endpoint() {
            crate::presentation_observation::HostPresentationEndpoint::Headless => false,
            crate::presentation_observation::HostPresentationEndpoint::Native(endpoint) => {
                binding.matches_viewport_binding(endpoint)
            }
        }
    }

    pub(super) fn matches_semantic_output(self, output: super::DockspaceSemanticOutput) -> bool {
        self.output == output.ticket
    }
}

/// Exact framework receiver bound to one concrete final-presentation output.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PresentedDockReceiver {
    descriptor: DockspaceReceiverDescriptor,
    authority: PresentedSurfaceAuthority,
}

impl PresentedDockReceiver {
    pub(super) fn from_projection_region(
        projection: crate::scene::SurfaceInteractionProjection<'_>,
        region: crate::presentation_hit::PresentationHitRegionId,
    ) -> Option<Self> {
        let record = projection.hit_manifest().region(region)?;
        let descriptor = DockspaceReceiverDescriptor::from_projection_region(
            projection.output_ticket(),
            region,
            record.lanes(),
            record.hit().rect(),
        )?;
        Some(Self {
            descriptor,
            authority: projection.authority(),
        })
    }

    /// Returns the exact receiver rectangle presented by the renderer.
    #[must_use]
    pub const fn bounds(self) -> LogicalRect {
        self.descriptor.bounds()
    }

    /// Returns the receiver center in surface-local logical coordinates.
    #[must_use]
    pub const fn center(self) -> LogicalPoint {
        self.descriptor.center()
    }

    pub(super) const fn region(self) -> crate::presentation_hit::PresentationHitRegionId {
        self.descriptor.region
    }
}

/// Stable pointer identity owned by one renderer-neutral surface provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct SurfacePointerId(u64);

impl SurfacePointerId {
    /// Creates an identity from the host provider's stable representation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the host provider's stable representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Renderer-neutral pointer button identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SurfacePointerButton {
    /// Primary selection and drag button.
    Primary,
    /// Secondary context button.
    Secondary,
    /// Middle pointer button.
    Middle,
    /// Host-defined additional button.
    Other(u16),
}

/// Event-time capture authority for one surface-local edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfacePointerCapture {
    /// The provider endpoint frozen into this session owns capture.
    ProviderEndpoint,
    /// A receiver outside this dockspace owns capture.
    Foreign,
    /// No receiver owns capture.
    None,
    /// The host could not observe capture authority for this edge.
    Unknown,
}

/// Event-time surface-local pointer position authority.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SurfacePointerPosition {
    /// The host observed one exact logical point for this edge.
    Known(LogicalPoint),
    /// The host could not observe an event-time logical point.
    Unknown,
}

/// Explicit lifecycle reason for terminating one pointer stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfacePointerCancelReason {
    /// The physical or virtual device was removed.
    DeviceRemoved,
    /// The platform explicitly cancelled this stream.
    ExplicitPlatformCancellation,
    /// The exact surface or native endpoint which owned this stream retired.
    BindingRetired,
}

/// Raw two-axis scroll delta retained until core selects the receiver.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SurfaceScrollDelta {
    /// Native physical pixels.
    PhysicalPixels {
        /// Horizontal content movement.
        x: f64,
        /// Vertical content movement.
        y: f64,
    },
    /// Renderer-independent logical points.
    LogicalPoints {
        /// Horizontal content movement.
        x: f64,
        /// Vertical content movement.
        y: f64,
    },
    /// Provider line units.
    Lines {
        /// Horizontal content movement.
        x: f64,
        /// Vertical content movement.
        y: f64,
    },
    /// Provider page units.
    Pages {
        /// Horizontal content movement.
        x: f64,
        /// Vertical content movement.
        y: f64,
    },
}

/// Explicit provider reason for cancelling a phaseful scroll sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfaceScrollCancelReason {
    /// The platform explicitly cancelled the gesture.
    PlatformCancelled,
    /// The delivery binding retired while the physical sequence may still emit a terminal tail.
    BindingRetired,
    /// The scroll device was removed.
    DeviceRemoved,
    /// The provider reset its sequence namespace.
    ProviderReset,
}

/// Provider-owned identity of one physical or virtual scroll device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct SurfaceScrollDeviceId(u64);

impl SurfaceScrollDeviceId {
    /// Creates a scroll-device identity from the host provider representation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the host provider representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Provider-owned monotonic identity of one smooth-scroll sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct SurfaceScrollSequenceId(u64);

impl SurfaceScrollSequenceId {
    /// Creates a sequence identity from the host provider representation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the host provider representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Native phase and payload shape of one lossless scroll sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SurfaceScrollPhase {
    /// One independent wheel step.
    Discrete {
        /// Raw sample delta.
        delta: SurfaceScrollDelta,
    },
    /// Starts one provider-defined smooth sequence.
    Begin {
        /// Provider-owned sequence identity.
        sequence: SurfaceScrollSequenceId,
        /// Optional first sample.
        delta: Option<SurfaceScrollDelta>,
    },
    /// Continues one provider-defined smooth sequence.
    Update {
        /// Provider-owned sequence identity.
        sequence: SurfaceScrollSequenceId,
        /// Raw continuation sample.
        delta: SurfaceScrollDelta,
    },
    /// Terminates a smooth sequence after an optional final sample.
    End {
        /// Provider-owned sequence identity.
        sequence: SurfaceScrollSequenceId,
        /// Optional final sample.
        delta: Option<SurfaceScrollDelta>,
    },
    /// Terminates a smooth sequence without applying another sample.
    Cancel {
        /// Provider-owned sequence identity.
        sequence: SurfaceScrollSequenceId,
        /// Explicit cancellation reason.
        reason: SurfaceScrollCancelReason,
    },
}

/// Direct-versus-momentum provenance for one scroll sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfaceScrollMomentum {
    /// Direct user-controlled movement.
    Direct,
    /// Platform-generated momentum within the same sequence.
    Momentum,
}

/// Exact event-time keyboard modifiers for one scroll sample.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct SurfaceScrollModifiers {
    shift: bool,
    control: bool,
    alt: bool,
    command: bool,
}

impl SurfaceScrollModifiers {
    /// Creates one exact modifier snapshot.
    #[must_use]
    pub const fn new(shift: bool, control: bool, alt: bool, command: bool) -> Self {
        Self {
            shift,
            control,
            alt,
            command,
        }
    }
}

/// One lossless wheel or trackpad sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceScrollEvent {
    device: SurfaceScrollDeviceId,
    phase: SurfaceScrollPhase,
    momentum: SurfaceScrollMomentum,
    modifiers: SurfaceScrollModifiers,
}

impl SurfaceScrollEvent {
    /// Creates one exact scroll sample.
    #[must_use]
    pub const fn new(
        device: SurfaceScrollDeviceId,
        phase: SurfaceScrollPhase,
        momentum: SurfaceScrollMomentum,
        modifiers: SurfaceScrollModifiers,
    ) -> Self {
        Self {
            device,
            phase,
            momentum,
            modifiers,
        }
    }
}

/// One surface-local pointer edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SurfacePointerEvent {
    /// Pointer motion while the provider owns the stream.
    Moved,
    /// One button became pressed.
    ButtonPressed(SurfacePointerButton),
    /// One button became released.
    ButtonReleased(SurfacePointerButton),
    /// A touch or pen contact released its button and ended its pointer identity.
    ContactEnded(SurfacePointerButton),
    /// A buttonless pointer identity ended normally.
    StreamEnded,
    /// The provider observed a capture transition.
    CaptureChanged,
    /// The provider explicitly terminated the pointer stream.
    StreamCancelled(SurfacePointerCancelReason),
    /// One lossless wheel or trackpad sample.
    Scrolled(SurfaceScrollEvent),
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

/// One ordered surface-local edge and its independent framework facts.
#[derive(Debug, Clone, Copy)]
pub struct SurfacePointerInput<'receiver> {
    pointer: SurfacePointerId,
    event: SurfacePointerEvent,
    position: SurfacePointerPosition,
    capture: SurfacePointerCapture,
    receivers: SurfacePointerReceiverFacts<'receiver>,
}

impl<'receiver> SurfacePointerInput<'receiver> {
    /// Creates one event-time input record.
    #[must_use]
    pub const fn new(
        pointer: SurfacePointerId,
        event: SurfacePointerEvent,
        position: SurfacePointerPosition,
        capture: SurfacePointerCapture,
        receivers: SurfacePointerReceiverFacts<'receiver>,
    ) -> Self {
        Self {
            pointer,
            event,
            position,
            capture,
            receivers,
        }
    }
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

    /// Adds the exact framework receiver which accepted delivery.
    ///
    /// Delivery and hover remain independent event-time facts even though one
    /// physical edge asks at most one receiver question.
    #[must_use]
    pub const fn with_delivery(mut self, receiver: &'receiver PresentedDockReceiver) -> Self {
        self.delivery = ReceiverFact::Dock(receiver);
        self
    }

    /// Adds an authoritative known-empty delivery result.
    #[must_use]
    pub const fn with_no_delivery(mut self, surface: &'receiver PresentedDockspaceSurface) -> Self {
        self.delivery = ReceiverFact::NoReceiver(surface);
        self
    }

    /// Adds an authoritative higher-layer delivery blocker.
    #[must_use]
    pub const fn with_blocked_delivery(
        mut self,
        surface: &'receiver PresentedDockspaceSurface,
    ) -> Self {
        self.delivery = ReceiverFact::Blocked(surface);
        self
    }

    /// Adds the exact point-bound docking receiver under the pointer.
    #[must_use]
    pub const fn with_hover(mut self, receiver: &'receiver PresentedDockReceiver) -> Self {
        self.hover = ReceiverFact::Dock(receiver);
        self
    }

    /// Adds an authoritative known-empty hover result.
    #[must_use]
    pub const fn with_no_hover(mut self, surface: &'receiver PresentedDockspaceSurface) -> Self {
        self.hover = ReceiverFact::NoReceiver(surface);
        self
    }

    /// Adds an authoritative higher-layer hover blocker.
    #[must_use]
    pub const fn with_blocked_hover(
        mut self,
        surface: &'receiver PresentedDockspaceSurface,
    ) -> Self {
        self.hover = ReceiverFact::Blocked(surface);
        self
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
    /// A product measurement answer used a value variant for another request kind.
    #[error("surface measurement answer does not match its core-derived request")]
    MeasurementAnswerMismatch,
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
    /// The producer cannot stop while an uncommitted host frame owns its lane.
    #[error("surface pointer producer still belongs to an uncommitted host frame")]
    PointerFrameInFlight,
    /// An explicit pointer batch contained no edges.
    #[error("an explicit surface pointer batch must contain at least one edge")]
    PointerBatchEmpty,
    /// One scroll sample contained a non-finite or structurally invalid value.
    #[error("surface scroll sample is structurally invalid")]
    InvalidScrollSample,
}

#[derive(Debug)]
pub(super) struct RuntimePointerState {
    provider: SurfaceLocalPointerProvider,
    surface: SurfaceId,
}

impl RuntimePointerState {
    pub(super) const fn new(provider: SurfaceLocalPointerProvider, surface: SurfaceId) -> Self {
        Self { provider, surface }
    }

    pub(super) fn committed_sequence(&self) -> u64 {
        self.provider.committed_through().get()
    }

    const fn surface(&self) -> SurfaceId {
        self.surface
    }

    const fn provider(&self) -> &SurfaceLocalPointerProvider {
        &self.provider
    }
}

/// Exact adapter work caused by retiring one surface-local pointer producer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "surface pointer retirement repaint work must be handled"]
pub struct SurfacePointerRetirement {
    surface: SurfaceId,
    interaction_changed: bool,
    repaint_required: bool,
}

impl SurfacePointerRetirement {
    /// Returns the surface whose producer lifetime ended.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    /// Returns whether the host must invalidate interaction decoration.
    #[must_use]
    pub const fn interaction_changed(self) -> bool {
        self.interaction_changed
    }

    /// Returns whether the host must rebuild this surface presentation.
    #[must_use]
    pub const fn repaint_required(self) -> bool {
        self.repaint_required
    }
}

impl DockspaceSession {
    /// Enables one presented surface as the sole surface-local pointer endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error until an exact output for `surface` has crossed the
    /// final-presentation observation boundary.
    pub fn enable_surface_pointer(
        &mut self,
        surface: SurfaceId,
    ) -> Result<(), DockspaceRuntimeError> {
        self.reconcile_surface_pointer_provider()?;
        if let Some(pointer) = self.pointer.as_ref() {
            return if pointer.surface() == surface {
                Ok(())
            } else {
                Err(DockspaceInteractionError::PointerProviderAlreadyActive.into())
            };
        }
        let provider = match self.engine.create_current_surface_local_pointer_provider(
            self.presentation_host,
            surface,
            PointerEdgeSequence::new(0),
        ) {
            Ok(provider) => provider,
            Err(
                EngineError::PointerProviderSurfaceAuthorityUnavailable { .. }
                | EngineError::PointerProviderSurfacePresentationMismatch { .. },
            ) => {
                return Err(
                    DockspaceInteractionError::PresentationAuthorityUnavailable { surface }.into(),
                );
            }
            Err(source) => return Err(source.into()),
        };
        self.pointer = Some(RuntimePointerState::new(provider, surface));
        Ok(())
    }

    /// Disables and atomically retires the current surface-local pointer endpoint.
    ///
    /// The producer is drained before core retirement. A rejected retirement
    /// restores the complete producer state so the caller can retry without
    /// losing the committed watermark.
    ///
    /// # Errors
    ///
    /// Returns a core lifecycle error while leaving the producer active and
    /// retryable.
    pub fn disable_surface_pointer(
        &mut self,
    ) -> Result<Option<SurfacePointerRetirement>, DockspaceRuntimeError> {
        self.retire_surface_pointer_provider()
    }

    pub(super) fn reconcile_surface_pointer_provider(
        &mut self,
    ) -> Result<(), DockspaceRuntimeError> {
        let stale = self.pointer.as_ref().is_some_and(|pointer| {
            self.engine.pointer_provider() != Some(pointer.provider().lease())
        });
        if stale {
            let retired = self.retire_surface_pointer_provider()?;
            debug_assert!(retired.is_some());
        }
        Ok(())
    }

    fn retire_surface_pointer_provider(
        &mut self,
    ) -> Result<Option<SurfacePointerRetirement>, DockspaceRuntimeError> {
        let Some(pointer) = self.pointer.take() else {
            return Ok(None);
        };
        let RuntimePointerState { provider, surface } = pointer;
        let mut receipt = match provider.drain() {
            Ok(receipt) => receipt,
            Err(error) => {
                self.pointer = Some(RuntimePointerState::new(error.into_provider(), surface));
                return Err(DockspaceInteractionError::PointerFrameInFlight.into());
            }
        };
        match self
            .engine
            .retire_quiesced_surface_local_pointer_provider(&mut receipt)
        {
            Ok(outcome) => Ok(Some(SurfacePointerRetirement {
                surface,
                interaction_changed: outcome.interaction_changed(),
                repaint_required: outcome.repaint_required(),
            })),
            Err(error) => {
                let provider = receipt
                    .into_provider()
                    .expect("a rejected runtime pointer retirement preserves its drain proof");
                self.pointer = Some(RuntimePointerState::new(provider, surface));
                Err(error.into())
            }
        }
    }

    /// Returns the current exact presented surface capability.
    #[must_use]
    pub fn presented_surface(&self, surface: SurfaceId) -> Option<PresentedDockspaceSurface> {
        let projection = self.engine.interaction_projection(surface)?;
        Some(PresentedDockspaceSurface::from_projection(projection))
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
        let surface = PresentedDockspaceSurface::from_projection(projection);
        projection.hit_manifest().region(descriptor.region)?;
        surface.bind_receiver(descriptor)
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
        self.measure_surface_with(surface, |request| match request {
            super::SurfaceMeasurementRequest::DockBounds { .. }
            | super::SurfaceMeasurementRequest::PopupPlaneBounds { .. } => {
                super::SurfaceMeasurementAnswer::Bounds(metrics.bounds)
            }
            super::SurfaceMeasurementRequest::PaneMinimum { .. } => {
                super::SurfaceMeasurementAnswer::PaneMinimum(metrics.pane_minimum)
            }
            super::SurfaceMeasurementRequest::TabIntrinsic { .. } => {
                super::SurfaceMeasurementAnswer::TabIntrinsic(metrics.tab_intrinsic.content_width())
            }
            super::SurfaceMeasurementRequest::TabStrip { .. } => {
                super::SurfaceMeasurementAnswer::TabStrip(metrics.tab_strip)
            }
        })
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
        let drop_affordance = view.interaction().drop_affordance().filter(|affordance| {
            affordance.surface() == surface && affordance.scene() == candidate.stamp()
        });
        Ok(Some(SurfacePaintPlan {
            surface,
            authority_domain: self.session.engine.authority_domain(),
            version: view.version(),
            scene: candidate.stamp(),
            output: candidate.output_ticket(),
            plan: candidate.plan(),
            hit_manifest: candidate.hit_manifest(),
            drop_affordance,
            drag_preview: view.presentation_drag_preview(surface),
            contained_transform_preview: view.presentation_contained_transform_preview(surface),
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
        reason: SurfaceUnavailableReason,
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
                    .prepare_surface_unavailable_contribution(token, reason.into())?
            };
            self.frame.push_surface_contribution(contribution)?;
        }
        Ok(())
    }

    /// Submits one ordered surface-local edge.
    ///
    /// This is the one-element form of [`Self::submit_surface_pointer_batch`].
    /// The input still carries an explicit pointer identity, position authority,
    /// capture authority, and receiver facts.
    ///
    /// # Errors
    ///
    /// Returns an error when no pointer provider is active or the edge violates
    /// the frozen protocol.
    pub fn submit_surface_pointer(
        &mut self,
        input: SurfacePointerInput<'_>,
    ) -> Result<(), DockspaceRuntimeError> {
        self.submit_surface_pointer_batch([input])
    }

    /// Submits one lossless, provider-ordered batch of surface-local edges.
    ///
    /// Sequence identities are assigned atomically in iterator order. Every
    /// edge retains its own pointer, position, capture, scroll, and independent
    /// receiver facts; no final-frame snapshot is used to reconstruct them.
    ///
    /// # Errors
    ///
    /// Returns an error when no pointer provider is active, the sequence is
    /// exhausted, the batch is empty, or an edge and its receiver facts violate
    /// the frozen protocol.
    pub fn submit_surface_pointer_batch<'receiver>(
        &mut self,
        inputs: impl IntoIterator<Item = SurfacePointerInput<'receiver>>,
    ) -> Result<(), DockspaceRuntimeError> {
        if self.pointer_input_submitted {
            return Err(DockspaceInteractionError::PointerInputAlreadySubmitted.into());
        }
        let inputs = inputs.into_iter().collect::<Vec<_>>();
        if inputs.is_empty() {
            return Err(DockspaceInteractionError::PointerBatchEmpty.into());
        }
        let surface = self
            .session
            .pointer
            .as_ref()
            .map(RuntimePointerState::surface)
            .ok_or(DockspaceInteractionError::PointerProviderUnavailable)?;
        let previous = self
            .next_pointer_sequence
            .ok_or(DockspaceInteractionError::PointerProviderUnavailable)?;
        let mut sequence = previous;
        let mut prepared = Vec::with_capacity(inputs.len());
        for input in inputs {
            let segment_start = sequence;
            sequence = sequence
                .checked_add(1)
                .ok_or(DockspaceInteractionError::PointerSequenceExhausted)?;
            let edge_sequence = PointerEdgeSequence::new(sequence);
            let kind = self.surface_pointer_kind(surface, input.event)?;
            let location = PointerEdgeLocation::SurfaceLocal {
                position: match input.position {
                    SurfacePointerPosition::Known(position) => Authority::Known(position),
                    SurfacePointerPosition::Unknown => {
                        Authority::Unknown(AuthorityUnavailableReason::NotReported)
                    }
                },
            };
            let edge = PointerEdge::new(
                edge_sequence,
                PointerId::new(input.pointer.get()),
                kind,
                location,
                surface_capture(input.capture),
            );
            let journal = PointerEdgeJournal::new(
                PointerEdgeSequence::new(segment_start),
                edge_sequence,
                vec![edge],
            )
            .map_err(|_| DockspaceInteractionError::PointerProtocolInvariant)?;
            prepared.push((edge_sequence, input, journal));
        }
        for (edge_sequence, input, journal) in prepared {
            self.submit_runtime_pointer_journal(journal)?;
            let candidates = self
                .frame
                .pointer_receiver_candidates()
                .ok_or(DockspaceInteractionError::PointerProtocolInvariant)?;
            let [candidate] = candidates.candidates() else {
                return Err(DockspaceInteractionError::PointerProtocolInvariant.into());
            };
            if candidate.id().sequence() != edge_sequence {
                return Err(DockspaceInteractionError::PointerProtocolInvariant.into());
            }
            let observation = self.pointer_observation(surface, candidate, input.receivers)?;
            let receipts = PointerReceiverReceiptBatch::new([candidate.receipt(observation)])
                .map_err(|_| DockspaceInteractionError::PointerProtocolInvariant)?;
            self.frame.submit_pointer_receiver_receipts(receipts)?;
        }
        self.next_pointer_sequence = Some(sequence);
        self.pointer_input_submitted = true;
        Ok(())
    }

    fn surface_pointer_kind(
        &self,
        surface: SurfaceId,
        event: SurfacePointerEvent,
    ) -> Result<PointerEdgeKind, DockspaceRuntimeError> {
        Ok(match event {
            SurfacePointerEvent::Moved => PointerEdgeKind::Moved,
            SurfacePointerEvent::ButtonPressed(button) => {
                PointerEdgeKind::ButtonPressed(surface_button(button))
            }
            SurfacePointerEvent::ButtonReleased(button) => {
                PointerEdgeKind::ButtonReleased(surface_button(button))
            }
            SurfacePointerEvent::ContactEnded(button) => {
                PointerEdgeKind::ContactEnded(surface_button(button))
            }
            SurfacePointerEvent::StreamEnded => PointerEdgeKind::StreamEnded,
            SurfacePointerEvent::CaptureChanged => PointerEdgeKind::CaptureChanged,
            SurfacePointerEvent::StreamCancelled(reason) => {
                PointerEdgeKind::StreamCancelled(surface_cancel_reason(reason))
            }
            SurfacePointerEvent::Scrolled(scroll) => {
                PointerEdgeKind::Scrolled(self.surface_scroll_edge(surface, scroll)?)
            }
        })
    }

    fn surface_scroll_edge(
        &self,
        surface: SurfaceId,
        event: SurfaceScrollEvent,
    ) -> Result<ScrollEdge, DockspaceRuntimeError> {
        let authority = self
            .frame
            .view()
            .interaction_projection(surface)
            .map(|projection| projection.authority());
        let delivery = match authority {
            Some(authority) => Authority::Known(
                ScrollDeliveryEndpoint::new(
                    self.session.presentation_host,
                    surface,
                    authority.binding(),
                    authority.coordinate_generation(),
                )
                .map_err(|_| DockspaceInteractionError::PointerProtocolInvariant)?,
            ),
            None => Authority::Unknown(AuthorityUnavailableReason::NotReported),
        };
        let (sequence, phase, delta) = match event.phase {
            SurfaceScrollPhase::Discrete { delta } => (None, ScrollPhase::Discrete, Some(delta)),
            SurfaceScrollPhase::Begin { sequence, delta } => (
                Some(ScrollSequenceToken::new(sequence.get())),
                ScrollPhase::Begin,
                delta,
            ),
            SurfaceScrollPhase::Update { sequence, delta } => (
                Some(ScrollSequenceToken::new(sequence.get())),
                ScrollPhase::Update,
                Some(delta),
            ),
            SurfaceScrollPhase::End { sequence, delta } => (
                Some(ScrollSequenceToken::new(sequence.get())),
                ScrollPhase::End,
                delta,
            ),
            SurfaceScrollPhase::Cancel { sequence, reason } => (
                Some(ScrollSequenceToken::new(sequence.get())),
                ScrollPhase::Cancel(surface_scroll_cancel_reason(reason)),
                None,
            ),
        };
        let delta = delta
            .map(|delta| surface_scroll_delta(delta, authority))
            .transpose()?;
        ScrollEdge::new(
            ScrollDeviceId::new(event.device.get()),
            sequence,
            phase,
            delta,
            Authority::Known(match event.momentum {
                SurfaceScrollMomentum::Direct => ScrollMomentum::Direct,
                SurfaceScrollMomentum::Momentum => ScrollMomentum::Momentum,
            }),
            Authority::Known(ScrollModifiers::new(
                event.modifiers.shift,
                event.modifiers.control,
                event.modifiers.alt,
                event.modifiers.command,
            )),
            delivery,
        )
        .map_err(|_| DockspaceInteractionError::InvalidScrollSample.into())
    }

    pub(super) fn complete_pointer_input(&mut self) -> Result<(), DockspaceRuntimeError> {
        if self.pointer_input_submitted || self.session.pointer.is_none() {
            return Ok(());
        }
        let sequence = self
            .next_pointer_sequence
            .ok_or(DockspaceInteractionError::PointerProviderUnavailable)?;
        let watermark = PointerEdgeSequence::new(sequence);
        let journal = PointerEdgeJournal::new(watermark, watermark, Vec::new())
            .map_err(|_| DockspaceInteractionError::PointerProtocolInvariant)?;
        self.submit_runtime_pointer_journal(journal)?;
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

    fn submit_runtime_pointer_journal(
        &mut self,
        journal: PointerEdgeJournal,
    ) -> Result<(), DockspaceRuntimeError> {
        let provider = self
            .session
            .pointer
            .as_ref()
            .map(RuntimePointerState::provider)
            .ok_or(DockspaceInteractionError::PointerProviderUnavailable)?;
        self.frame
            .submit_surface_pointer_journal(provider, journal)?;
        Ok(())
    }

    fn pointer_observation(
        &self,
        surface: SurfaceId,
        candidate: &PointerReceiverCandidate,
        facts: SurfacePointerReceiverFacts<'_>,
    ) -> Result<PointerReceiverObservation, DockspaceRuntimeError> {
        if !candidate.receiver_is_applicable() {
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

    pub(super) fn surface_answered(&self, surface: SurfaceId) -> bool {
        self.painted_surfaces.contains(&surface)
            || self
                .frame
                .surface_contributions()
                .iter()
                .any(|contribution| contribution.surface() == surface)
    }
}

const fn surface_button(button: SurfacePointerButton) -> PointerButton {
    match button {
        SurfacePointerButton::Primary => PointerButton::Primary,
        SurfacePointerButton::Secondary => PointerButton::Secondary,
        SurfacePointerButton::Middle => PointerButton::Middle,
        SurfacePointerButton::Other(button) => PointerButton::Other(button),
    }
}

const fn surface_capture(capture: SurfacePointerCapture) -> Authority<PointerCaptureOwner> {
    match capture {
        SurfacePointerCapture::ProviderEndpoint => {
            Authority::Known(PointerCaptureOwner::ProviderEndpoint)
        }
        SurfacePointerCapture::Foreign => Authority::Known(PointerCaptureOwner::Foreign),
        SurfacePointerCapture::None => Authority::Known(PointerCaptureOwner::None),
        SurfacePointerCapture::Unknown => {
            Authority::Unknown(AuthorityUnavailableReason::NotReported)
        }
    }
}

const fn surface_cancel_reason(reason: SurfacePointerCancelReason) -> PointerStreamCancelReason {
    match reason {
        SurfacePointerCancelReason::DeviceRemoved => PointerStreamCancelReason::DeviceRemoved,
        SurfacePointerCancelReason::ExplicitPlatformCancellation => {
            PointerStreamCancelReason::ExplicitPlatformCancellation
        }
        SurfacePointerCancelReason::BindingRetired => PointerStreamCancelReason::BindingRetired,
    }
}

const fn surface_scroll_cancel_reason(reason: SurfaceScrollCancelReason) -> ScrollCancelReason {
    match reason {
        SurfaceScrollCancelReason::PlatformCancelled => ScrollCancelReason::PlatformCancelled,
        SurfaceScrollCancelReason::BindingRetired => ScrollCancelReason::BindingRetired,
        SurfaceScrollCancelReason::DeviceRemoved => ScrollCancelReason::DeviceRemoved,
        SurfaceScrollCancelReason::ProviderReset => ScrollCancelReason::ProviderReset,
    }
}

fn surface_scroll_delta(
    delta: SurfaceScrollDelta,
    authority: Option<PresentedSurfaceAuthority>,
) -> Result<ScrollDelta, DockspaceRuntimeError> {
    let (x, y) = match delta {
        SurfaceScrollDelta::PhysicalPixels { x, y }
        | SurfaceScrollDelta::LogicalPoints { x, y }
        | SurfaceScrollDelta::Lines { x, y }
        | SurfaceScrollDelta::Pages { x, y } => (x, y),
    };
    let vector = FiniteScrollVector::new(x, y)
        .map_err(|_| DockspaceInteractionError::InvalidScrollSample)?;
    Ok(match delta {
        SurfaceScrollDelta::PhysicalPixels { .. } => ScrollDelta::PhysicalPixels {
            delta: vector,
            coordinates: authority
                .and_then(|authority| {
                    authority.binding().map(|binding| {
                        PhysicalScrollCoordinates::new(binding, authority.coordinate_generation())
                    })
                })
                .map_or_else(
                    || Authority::Unknown(AuthorityUnavailableReason::NotReported),
                    Authority::Known,
                ),
        },
        SurfaceScrollDelta::LogicalPoints { .. } => ScrollDelta::LogicalPoints(vector),
        SurfaceScrollDelta::Lines { .. } => ScrollDelta::Lines(vector),
        SurfaceScrollDelta::Pages { .. } => ScrollDelta::Pages(vector),
    })
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

pub(super) fn receiver_matches(
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

pub(super) fn surface_matches(
    surface: &PresentedDockspaceSurface,
    projection: &crate::scene::SurfaceInteractionProjection<'_>,
) -> bool {
    surface.surface == projection.output_ticket().surface()
        && surface.output == projection.output_ticket()
        && surface.authority == projection.authority()
}

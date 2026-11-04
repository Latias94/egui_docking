//! Opaque renderer-facing pointer capabilities.

use thiserror::Error;

use super::{
    DockspaceHostFrame, DockspaceReceiverDescriptor, DockspaceRuntimeError, DockspaceSession,
};
use crate::engine::EngineError;
use crate::geometry::{LogicalPoint, LogicalRect};
use crate::ids::SurfaceId;
use crate::intent::{Authority, AuthorityUnavailableReason, PointerButton, PointerId};
use crate::pointer_journal::{
    FiniteScrollVector, HostLocalPointerRoute, PhysicalScrollCoordinates, PointerCaptureOwner,
    PointerDeviceKind, PointerEdge, PointerEdgeJournal, PointerEdgeKind, PointerEdgeLocation,
    PointerEdgeSequence, PointerStreamCancelReason, ScrollCancelReason, ScrollDeliveryEndpoint,
    ScrollDelta, ScrollDeviceId, ScrollEdge, ScrollModifiers, ScrollMomentum, ScrollPhase,
    ScrollSequenceToken, SurfaceLocalPointerProvider,
};
use crate::pointer_receiver::{
    PointerReceiverCandidate, PointerReceiverDelivery, PointerReceiverDeliveryDisposition,
    PointerReceiverHoverHit, PointerReceiverHoverHitDisposition, PointerReceiverObservation,
    PointerReceiverProbeReceipt, PointerReceiverReceiptBatch, PointerReceiverUnknownReason,
    PresentedPointerReceiverObservation,
};
use crate::presentation_observation::{PresentedSurfaceAuthority, SurfacePresentationOutputTicket};

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

    /// Returns whether this capability names the exact committed renderer artifact.
    #[must_use]
    pub fn matches_presentation_artifact(
        self,
        artifact: super::SurfacePresentationArtifact,
    ) -> bool {
        let output = artifact.output();
        let payload = output.payload();
        output.surface() == self.surface
            && payload.scene() == Some(self.output)
            && payload.coordinate_generation() == Some(self.authority.coordinate_generation())
            && output.stream() == self.authority.stream()
            && output.key() == self.authority.emission()
            && output.endpoint() == self.authority.endpoint()
    }
}

/// Exact framework receiver bound to one concrete final-presentation output.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PresentedDockReceiver {
    descriptor: DockspaceReceiverDescriptor,
    authority: PresentedSurfaceAuthority,
}

impl PresentedDockReceiver {
    /// Returns the semantic descriptor bound to this exact presented output.
    #[must_use]
    pub const fn descriptor(self) -> DockspaceReceiverDescriptor {
        self.descriptor
    }
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

    /// Returns the base receiver rectangle from this exact presented output.
    ///
    /// Expanded touch targets may extend beyond it, or survive an empty base
    /// rectangle. Use the device-aware query to resolve those targets.
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

/// Core-owned receiver candidates at one point in a presented logical surface.
///
/// This identifies docking's semantic winners. The host must separately prove
/// framework delivery or hover eligibility before submitting receiver facts.
#[derive(Debug, Clone, Copy)]
pub struct HostPointerHit {
    surface: PresentedDockspaceSurface,
    delivery: Option<PresentedDockReceiver>,
    drag_delivery: Option<PresentedDockReceiver>,
    hover: Option<PresentedDockReceiver>,
}

impl HostPointerHit {
    /// Returns the exact presentation used for this query.
    #[must_use]
    pub const fn surface(self) -> PresentedDockspaceSurface {
        self.surface
    }

    /// Returns the independent click-delivery winner at the queried point.
    #[must_use]
    pub const fn delivery(self) -> Option<PresentedDockReceiver> {
        self.delivery
    }

    /// Returns the independent drag-lane winner, which may differ from click.
    #[must_use]
    pub const fn drag_delivery(self) -> Option<PresentedDockReceiver> {
        self.drag_delivery
    }

    /// Returns the core-owned hover/drop receiver at the queried point.
    #[must_use]
    pub const fn hover(self) -> Option<PresentedDockReceiver> {
        self.hover
    }
}

/// Stable pointer identity owned by one renderer-neutral input provider.
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

/// Event-time capture authority within one renderer-neutral input domain.
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
    /// The user explicitly cancelled this manipulation with Escape.
    Escape,
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
///
/// Identities must increase across all phaseful scroll sequences submitted by the
/// same pointer-provider lease, including sequences on different streams or devices.
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

/// One renderer-neutral pointer edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SurfacePointerEvent {
    /// Changes magnetic targeting intent without reporting motion or capture.
    ///
    /// The directive applies only to this provider's matching active pointer.
    /// Enabling permits fresh acquisition on a later real pointer edge.
    MagneticTargetingChanged {
        /// Whether magnetic acquisition is enabled; explicit targets stay available.
        enabled: bool,
    },
    /// Pointer motion while the provider owns the stream.
    Moved,
    /// Rechecks one core-issued stationary action with fresh receiver facts.
    Continuation(crate::interaction::PointerContinuationToken),
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
    ClearHoverLane(&'receiver PresentedDockspaceSurface),
    Blocked(&'receiver PresentedDockspaceSurface),
    Unknown,
}

/// Independent framework delivery and hover facts for one pointer edge.
#[derive(Debug, Clone, Copy)]
pub struct SurfacePointerReceiverFacts<'receiver> {
    delivery: ReceiverFact<'receiver>,
    drag_delivery: Option<ReceiverFact<'receiver>>,
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
    metadata: super::PointerEventMetadata,
}

/// Event-time route within one host's logical presentation domain.
///
/// An unknown route never asserts that the pointer is outside all desktop windows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HostPointerRoute {
    /// A point in one exact presented surface output.
    Surface {
        /// Presentation capability for the routed surface.
        surface: PresentedDockspaceSurface,
        /// Surface-local logical position at this edge.
        position: LogicalPoint,
    },
    /// The host cannot qualify a surface at this edge.
    Unknown,
}

impl HostPointerRoute {
    /// Binds a local point to an exact presented surface.
    #[must_use]
    pub const fn new(surface: PresentedDockspaceSurface, position: LogicalPoint) -> Self {
        Self::Surface { surface, position }
    }

    const fn surface(self) -> Option<SurfaceId> {
        match self {
            Self::Surface { surface, .. } => Some(surface.surface),
            Self::Unknown => None,
        }
    }

    fn authority(self) -> Authority<HostLocalPointerRoute> {
        match self {
            Self::Surface { surface, position } => {
                Authority::Known(HostLocalPointerRoute::new(surface.authority, position))
            }
            Self::Unknown => Authority::Unknown(AuthorityUnavailableReason::NotReported),
        }
    }
}

/// One host-ordered edge with independent manipulation and hover routes.
///
/// Capture belongs to the host input domain. Changing the hover surface does
/// not replace the producer or transfer the gesture owner.
#[derive(Debug, Clone, Copy)]
pub struct HostPointerInput<'receiver> {
    pointer: SurfacePointerId,
    event: SurfacePointerEvent,
    manipulation: HostPointerRoute,
    hover: HostPointerRoute,
    capture: SurfacePointerCapture,
    receivers: SurfacePointerReceiverFacts<'receiver>,
    metadata: super::PointerEventMetadata,
}

impl<'receiver> HostPointerInput<'receiver> {
    /// Creates one event-time record in the host's total pointer order.
    #[must_use]
    pub const fn new(
        pointer: SurfacePointerId,
        event: SurfacePointerEvent,
        manipulation: HostPointerRoute,
        hover: HostPointerRoute,
        capture: SurfacePointerCapture,
        receivers: SurfacePointerReceiverFacts<'receiver>,
    ) -> Self {
        Self {
            pointer,
            event,
            manipulation,
            hover,
            capture,
            receivers,
            metadata: super::PointerEventMetadata::unknown(),
        }
    }

    /// Attaches device and monotonic event time without deriving receiver facts.
    #[must_use]
    pub const fn with_metadata(mut self, metadata: super::PointerEventMetadata) -> Self {
        self.metadata = metadata;
        self
    }
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
            metadata: super::PointerEventMetadata::unknown(),
        }
    }

    /// Attaches device and monotonic event time without deriving receiver facts.
    #[must_use]
    pub const fn with_metadata(mut self, metadata: super::PointerEventMetadata) -> Self {
        self.metadata = metadata;
        self
    }
}

impl<'receiver> SurfacePointerReceiverFacts<'receiver> {
    /// Overrides only the drag-delivery lane with an exact receiver.
    #[must_use]
    pub const fn with_drag_delivery(mut self, receiver: &'receiver PresentedDockReceiver) -> Self {
        self.drag_delivery = Some(ReceiverFact::Dock(receiver));
        self
    }

    /// Reports known-empty drag delivery independently from click delivery.
    #[must_use]
    pub const fn with_no_drag_delivery(
        mut self,
        surface: &'receiver PresentedDockspaceSurface,
    ) -> Self {
        self.drag_delivery = Some(ReceiverFact::NoReceiver(surface));
        self
    }

    /// Reports no authoritative receiver fact for either lane.
    #[must_use]
    pub const fn unknown() -> Self {
        Self {
            delivery: ReceiverFact::Unknown,
            drag_delivery: None,
            hover: ReceiverFact::Unknown,
        }
    }

    /// Reports the exact framework receiver which accepted delivery.
    #[must_use]
    pub const fn delivery(receiver: &'receiver PresentedDockReceiver) -> Self {
        Self {
            delivery: ReceiverFact::Dock(receiver),
            drag_delivery: None,
            hover: ReceiverFact::Unknown,
        }
    }

    /// Reports the exact point-bound docking receiver under an active drag.
    #[must_use]
    pub const fn hover(receiver: &'receiver PresentedDockReceiver) -> Self {
        Self {
            delivery: ReceiverFact::Unknown,
            drag_delivery: None,
            hover: ReceiverFact::Dock(receiver),
        }
    }

    /// Reports an authoritative known-empty hover result for the current output.
    #[must_use]
    pub const fn no_hover(surface: &'receiver PresentedDockspaceSurface) -> Self {
        Self {
            delivery: ReceiverFact::Unknown,
            drag_delivery: None,
            hover: ReceiverFact::NoReceiver(surface),
        }
    }

    /// Reports that a higher framework layer blocked hover-drop delivery.
    #[must_use]
    pub const fn blocked_hover(surface: &'receiver PresentedDockspaceSurface) -> Self {
        Self {
            delivery: ReceiverFact::Unknown,
            drag_delivery: None,
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

    /// Proves that no framework receiver blocks HoverDrop in this exact output.
    ///
    /// Unlike [`Self::with_hover`] and [`Self::with_no_hover`], this does not
    /// assert a semantic winner. Core resolves it at the edge's exact point
    /// after preceding edges have reduced, including complete-source suppression.
    /// Unknown widgets, overlays and unavailable framework hit facts must not
    /// be reported as a clear lane.
    #[must_use]
    pub const fn with_clear_hover_lane(
        mut self,
        surface: &'receiver PresentedDockspaceSurface,
    ) -> Self {
        self.hover = ReceiverFact::ClearHoverLane(surface);
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
    /// Repeating encoding requires an explicitly opened encoding pass.
    #[error("the update has no encoding pass to restart")]
    EncodingPassUnavailable,
    /// The private encoding pass counter cannot advance.
    #[error("the encoding pass identity is exhausted")]
    EncodingPassExhausted,
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
    /// A completed surface program belongs to another host-frame attempt or candidate.
    #[error("surface {surface} execution receipt does not match the current host frame")]
    SurfaceProgramExecutionMismatch {
        /// Surface named by the rejected receipt.
        surface: SurfaceId,
    },
    /// The uniform writer disagreed with the exact core manifest.
    #[error("uniform measurement writer could not answer the core manifest exactly")]
    MeasurementRosterInvariant,
    /// A product measurement answer used a value variant for another request kind.
    #[error("surface measurement answer does not match its core-derived request")]
    MeasurementAnswerMismatch,
    /// A pointer provider is already active for another input domain.
    #[error("a pointer provider is already active for another input domain")]
    PointerProviderAlreadyActive,
    /// The requested surface does not yet have final-presentation authority.
    #[error("surface {surface} has no final-presentation authority")]
    PresentationAuthorityUnavailable {
        /// Requested surface.
        surface: SurfaceId,
    },
    /// No facade-owned pointer provider is active.
    #[error("no facade-owned pointer provider is active")]
    PointerProviderUnavailable,
    /// The adapter-owned pointer identity cannot advance without wrapping.
    #[error("pointer identity is exhausted")]
    PointerIdentityExhausted,
    /// The provider-owned pointer sequence cannot advance without wrapping.
    #[error("pointer sequence is exhausted")]
    PointerSequenceExhausted,
    /// A core constructor rejected facade-owned canonical pointer data.
    #[error("facade-owned pointer protocol data violated an internal invariant")]
    PointerProtocolInvariant,
    /// A pointer batch was already supplied for this host frame.
    #[error("pointer input was already submitted for this host frame")]
    PointerInputAlreadySubmitted,
    /// The producer cannot stop while an uncommitted host frame owns its lane.
    #[error("pointer producer still belongs to an uncommitted host frame")]
    PointerFrameInFlight,
    /// An explicit pointer batch contained no edges.
    #[error("an explicit pointer batch must contain at least one edge")]
    PointerBatchEmpty,
    /// One scroll sample contained a non-finite or structurally invalid value.
    #[error("surface scroll sample is structurally invalid")]
    InvalidScrollSample,
}

#[derive(Debug)]
pub(super) struct RuntimePointerState {
    provider: SurfaceLocalPointerProvider,
    surface: Option<SurfaceId>,
}

impl RuntimePointerState {
    pub(super) const fn new(
        provider: SurfaceLocalPointerProvider,
        surface: Option<SurfaceId>,
    ) -> Self {
        Self { provider, surface }
    }

    pub(super) fn committed_sequence(&self) -> u64 {
        self.provider.committed_through().get()
    }

    const fn surface(&self) -> Option<SurfaceId> {
        self.surface
    }

    const fn provider(&self) -> &SurfaceLocalPointerProvider {
        &self.provider
    }
}

/// Repaint work caused by retiring a host input domain.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "host pointer retirement repaint work must be handled"]
pub struct HostPointerRetirement {
    interaction_changed: bool,
    repaint_required: bool,
    manipulation_feedback: Vec<super::DockspaceManipulationFeedback>,
}

impl HostPointerRetirement {
    /// Returns whether interaction decoration changed on the host.
    #[must_use]
    pub const fn interaction_changed(&self) -> bool {
        self.interaction_changed
    }

    /// Returns whether the host must rebuild affected surface presentations.
    #[must_use]
    pub const fn repaint_required(&self) -> bool {
        self.repaint_required
    }

    /// Returns committed terminal feedback from this retirement boundary.
    pub fn manipulation_feedback(&self) -> &[super::DockspaceManipulationFeedback] {
        &self.manipulation_feedback
    }
}

/// Exact adapter work caused by retiring one surface-local pointer producer.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "surface pointer retirement repaint work must be handled"]
pub struct SurfacePointerRetirement {
    surface: SurfaceId,
    interaction_changed: bool,
    repaint_required: bool,
    manipulation_feedback: Vec<super::DockspaceManipulationFeedback>,
}

impl SurfacePointerRetirement {
    /// Returns the surface whose producer lifetime ended.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Returns whether the host must invalidate interaction decoration.
    #[must_use]
    pub const fn interaction_changed(&self) -> bool {
        self.interaction_changed
    }

    /// Returns whether the host must rebuild this surface presentation.
    #[must_use]
    pub const fn repaint_required(&self) -> bool {
        self.repaint_required
    }

    /// Returns committed terminal feedback from this retirement boundary.
    pub fn manipulation_feedback(&self) -> &[super::DockspaceManipulationFeedback] {
        &self.manipulation_feedback
    }
}

impl DockspaceSession {
    /// Queries docking's semantic receiver winners in one exact current output.
    ///
    /// Framework overlays and application widgets may still block these candidates.
    /// This query grants neither event delivery nor capture authority by itself.
    #[must_use]
    pub fn host_pointer_hit(
        &self,
        surface: SurfaceId,
        point: LogicalPoint,
    ) -> Option<HostPointerHit> {
        self.host_pointer_hit_for_device(surface, point, PointerDeviceKind::Unknown)
    }

    /// Queries current semantic receivers using an explicitly observed device class.
    ///
    /// Explicit Touch uses the output's expanded resize targets. Mouse, Pen and
    /// Unknown retain the base geometry. Receiver descriptors still describe the
    /// base control; their bounds may be empty and are not a union of touch hit fragments.
    /// Framework clearance, capture and exact-output input validation remain
    /// required before a candidate can start a gesture.
    #[must_use]
    pub fn host_pointer_hit_for_device(
        &self,
        surface: SurfaceId,
        point: LogicalPoint,
        device: PointerDeviceKind,
    ) -> Option<HostPointerHit> {
        host_pointer_hit(&self.engine, &self.presentation, surface, point, device)
    }

    /// Enables one affine input producer spanning this host's logical surfaces.
    ///
    /// No native window or already-presented surface is required. Each edge
    /// independently supplies exact manipulation and hover presentation routes.
    ///
    /// # Errors
    ///
    /// Returns an error if another input domain is active or the host retired.
    pub fn enable_host_pointer(&mut self) -> Result<(), DockspaceRuntimeError> {
        self.reconcile_pointer_provider()?;
        if let Some(pointer) = self.pointer.as_ref() {
            return if pointer.surface().is_none() {
                Ok(())
            } else {
                Err(DockspaceInteractionError::PointerProviderAlreadyActive.into())
            };
        }
        let provider = self.engine.create_host_local_pointer_provider(
            self.presentation_host,
            PointerEdgeSequence::new(0),
        )?;
        self.pointer = Some(RuntimePointerState::new(provider, None));
        Ok(())
    }

    /// Drains and retires the host input domain atomically.
    ///
    /// # Errors
    ///
    /// Returns an error while a frame owns the producer or another domain is active.
    pub fn disable_host_pointer(
        &mut self,
    ) -> Result<Option<HostPointerRetirement>, DockspaceRuntimeError> {
        if self
            .pointer
            .as_ref()
            .is_some_and(|pointer| pointer.surface().is_some())
        {
            return Err(DockspaceInteractionError::PointerProviderAlreadyActive.into());
        }
        Ok(self.retire_pointer_provider()?.map(|(_, work)| work))
    }
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
        self.reconcile_pointer_provider()?;
        if let Some(pointer) = self.pointer.as_ref() {
            return if pointer.surface() == Some(surface) {
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
        self.pointer = Some(RuntimePointerState::new(provider, Some(surface)));
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
        if self
            .pointer
            .as_ref()
            .is_some_and(|pointer| pointer.surface().is_none())
        {
            return Err(DockspaceInteractionError::PointerProviderAlreadyActive.into());
        }
        Ok(self
            .retire_pointer_provider()?
            .map(|(surface, work)| SurfacePointerRetirement {
                surface: surface.expect("surface producer retains its endpoint"),
                interaction_changed: work.interaction_changed,
                repaint_required: work.repaint_required,
                manipulation_feedback: work.manipulation_feedback,
            }))
    }

    pub(super) fn reconcile_pointer_provider(&mut self) -> Result<(), DockspaceRuntimeError> {
        let stale = self.pointer.as_ref().is_some_and(|pointer| {
            self.engine.pointer_provider() != Some(pointer.provider().lease())
        });
        if stale {
            let retired = self.retire_pointer_provider()?;
            debug_assert!(retired.is_some());
        }
        Ok(())
    }

    fn retire_pointer_provider(
        &mut self,
    ) -> Result<Option<(Option<SurfaceId>, HostPointerRetirement)>, DockspaceRuntimeError> {
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
            Ok(outcome) => Ok(Some((
                surface,
                HostPointerRetirement {
                    interaction_changed: outcome.interaction_changed(),
                    repaint_required: outcome.repaint_required(),
                    manipulation_feedback: outcome
                        .interaction_events()
                        .iter()
                        .enumerate()
                        .filter_map(|(index, event)| {
                            super::DockspaceManipulationFeedback::from_event(
                                self.engine.authority_domain(),
                                self.engine.last_reducer_tick(),
                                index,
                                event,
                            )
                        })
                        .collect(),
                },
            ))),
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
    ///
    /// Returns `None` while the host has reported a newer paint as presented
    /// but core has not yet accepted its ordered presentation prefix.
    #[must_use]
    pub fn presented_surface(&self, surface: SurfaceId) -> Option<PresentedDockspaceSurface> {
        let projection = self.engine.interaction_projection(surface)?;
        if self
            .presentation
            .blocks_pointer_authority(surface, projection.authority())
        {
            return None;
        }
        Some(PresentedDockspaceSurface::from_projection(projection))
    }

    /// Binds a paint-time descriptor to the exact currently presented output.
    ///
    /// Returns `None` while a newer host-reported paint awaits ordered core
    /// settlement, preventing receivers from the previous pixels being reused.
    #[must_use]
    pub fn bind_presented_receiver(
        &self,
        descriptor: &DockspaceReceiverDescriptor,
    ) -> Option<PresentedDockReceiver> {
        bind_presented_receiver(&self.engine, &self.presentation, descriptor)
    }
}

fn host_pointer_hit(
    engine: &crate::engine::DockEngine,
    presentation: &super::presentation::RuntimePresentationState,
    surface: SurfaceId,
    point: LogicalPoint,
    device: PointerDeviceKind,
) -> Option<HostPointerHit> {
    use crate::presentation_hit::PresentationPointerLane;
    let projection = engine.interaction_projection(surface)?;
    if presentation.blocks_pointer_authority(surface, projection.authority()) {
        return None;
    }
    let resolve = |lane| {
        let suppression = if lane == PresentationPointerLane::HoverDrop {
            engine.hover_drop_suppression()
        } else {
            Default::default()
        };
        projection
            .hit_manifest()
            .resolve_exclusive_for_device(lane, point, suppression, device)
            .ok()
            .map(|region| {
                region.and_then(|region| {
                    PresentedDockReceiver::from_projection_region(projection, region.id())
                })
            })
    };
    Some(HostPointerHit {
        surface: PresentedDockspaceSurface::from_projection(projection),
        delivery: resolve(PresentationPointerLane::Click)?,
        drag_delivery: resolve(PresentationPointerLane::Drag)?,
        hover: resolve(PresentationPointerLane::HoverDrop)?,
    })
}

fn pointer_authority_blocked(
    engine: &crate::engine::DockEngine,
    presentation: &super::presentation::RuntimePresentationState,
    surface: SurfaceId,
) -> bool {
    engine
        .interaction_projection(surface)
        .is_some_and(|projection| {
            presentation.blocks_pointer_authority(surface, projection.authority())
        })
}

fn bind_presented_receiver(
    engine: &crate::engine::DockEngine,
    presentation: &super::presentation::RuntimePresentationState,
    descriptor: &DockspaceReceiverDescriptor,
) -> Option<PresentedDockReceiver> {
    let projection = engine.interaction_projection(descriptor.region.surface())?;
    if presentation.blocks_pointer_authority(descriptor.region.surface(), projection.authority()) {
        return None;
    }
    projection.hit_manifest().region(descriptor.region)?;
    if !engine.presented_receiver_is_current(projection.plan(), descriptor.region) {
        return None;
    }
    PresentedDockspaceSurface::from_projection(projection).bind_receiver(descriptor)
}

struct RuntimePointerInput<'receiver> {
    pointer: SurfacePointerId,
    event: SurfacePointerEvent,
    location: PointerEdgeLocation,
    manipulation_surface: Option<SurfaceId>,
    capture: SurfacePointerCapture,
    receivers: SurfacePointerReceiverFacts<'receiver>,
    metadata: super::PointerEventMetadata,
}

impl DockspaceHostFrame<'_> {
    /// Returns retained source coordinates for this exact active host pointer.
    ///
    /// This capability is valid only as a manipulation route for the active
    /// gesture. It grants no hover, receiver or new-press authority. Coordinate,
    /// owner, epoch and source continuation checks still apply to every edge.
    #[must_use]
    pub fn host_pointer_manipulation_surface(
        &self,
        pointer: SurfacePointerId,
    ) -> Option<PresentedDockspaceSurface> {
        let authority = self
            .frame
            .candidate_engine()
            .host_local_manipulation_authority(crate::intent::PointerId::new(pointer.get()))?;
        Some(PresentedDockspaceSurface {
            surface: authority.surface(),
            output: authority.ticket(),
            authority,
        })
    }

    /// Returns the exact presented capability after this update's accepted observations.
    ///
    /// A host-reported paint remains withheld until its presentation prefix is
    /// atomically committed to the session.
    pub fn presented_surface(&self, surface: SurfaceId) -> Option<PresentedDockspaceSurface> {
        let projection = self
            .frame
            .candidate_engine()
            .interaction_projection(surface)?;
        if self
            .session
            .presentation
            .blocks_pointer_authority(surface, projection.authority())
        {
            return None;
        }
        Some(PresentedDockspaceSurface::from_projection(projection))
    }

    /// Binds only receivers authorized by the accepted input prefix.
    ///
    /// A reported paint that is still pending core publication temporarily
    /// withholds receiver authority for its surface.
    pub fn bind_presented_receiver(
        &self,
        descriptor: &DockspaceReceiverDescriptor,
    ) -> Option<PresentedDockReceiver> {
        bind_presented_receiver(
            self.frame.candidate_engine(),
            &self.session.presentation,
            descriptor,
        )
    }

    /// Resolves current semantic candidates; framework clearance is still required.
    pub fn host_pointer_hit_for_device(
        &self,
        surface: SurfaceId,
        point: LogicalPoint,
        device: PointerDeviceKind,
    ) -> Option<HostPointerHit> {
        host_pointer_hit(
            self.frame.candidate_engine(),
            &self.session.presentation,
            surface,
            point,
            device,
        )
    }

    /// Returns whether the private input prefix still owns a gesture.
    pub fn has_active_gesture(&self) -> bool {
        !matches!(
            self.frame.view().interaction().status(),
            crate::interaction::InteractionStatus::Idle
        )
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
        let surface = self
            .session
            .pointer
            .as_ref()
            .and_then(RuntimePointerState::surface)
            .ok_or(DockspaceInteractionError::PointerProviderUnavailable)?;
        let inputs = inputs
            .into_iter()
            .map(|input| RuntimePointerInput {
                pointer: input.pointer,
                event: input.event,
                location: PointerEdgeLocation::SurfaceLocal {
                    position: match input.position {
                        SurfacePointerPosition::Known(position) => Authority::Known(position),
                        SurfacePointerPosition::Unknown => {
                            Authority::Unknown(AuthorityUnavailableReason::NotReported)
                        }
                    },
                },
                manipulation_surface: Some(surface),
                capture: input.capture,
                receivers: input.receivers,
                metadata: input.metadata,
            })
            .collect::<Vec<_>>();
        self.submit_pointer_batch(inputs)
    }

    /// Submits one edge in the host input domain.
    ///
    /// # Errors
    ///
    /// Returns the same protocol errors as [`Self::submit_host_pointer_batch`].
    pub fn submit_host_pointer(
        &mut self,
        input: HostPointerInput<'_>,
    ) -> Result<(), DockspaceRuntimeError> {
        self.submit_host_pointer_batch([input])
    }

    /// Submits a lossless host-ordered batch across logical surfaces.
    ///
    /// Each edge retains independent source manipulation and target hover routes.
    /// All surfaces share one sequence and one capture owner; no desktop routing
    /// or outside-all authority is inferred from an unknown logical route.
    ///
    /// # Errors
    ///
    /// Returns an error if no host input domain is active, the batch is empty,
    /// encoding has started, or a protocol fact is inconsistent. Consecutive
    /// batches append to the same unpublished host-local input prefix.
    pub fn submit_host_pointer_batch<'receiver>(
        &mut self,
        inputs: impl IntoIterator<Item = HostPointerInput<'receiver>>,
    ) -> Result<(), DockspaceRuntimeError> {
        let pointer = self
            .session
            .pointer
            .as_ref()
            .ok_or(DockspaceInteractionError::PointerProviderUnavailable)?;
        if pointer.surface().is_some() {
            return Err(DockspaceInteractionError::PointerProviderUnavailable.into());
        }
        let inputs = inputs
            .into_iter()
            .map(|input| RuntimePointerInput {
                pointer: input.pointer,
                event: input.event,
                location: PointerEdgeLocation::HostLocal {
                    manipulation: input.manipulation.authority(),
                    hover: input.hover.authority(),
                },
                manipulation_surface: input.manipulation.surface(),
                capture: input.capture,
                receivers: input.receivers,
                metadata: input.metadata,
            })
            .collect::<Vec<_>>();
        self.submit_pointer_batch(inputs)
    }

    fn submit_pointer_batch(
        &mut self,
        inputs: Vec<RuntimePointerInput<'_>>,
    ) -> Result<(), DockspaceRuntimeError> {
        self.ensure_semantic_input_allowed()?;
        if self.pointer_input_submitted
            && self
                .session
                .pointer
                .as_ref()
                .is_some_and(|pointer| pointer.surface().is_some())
        {
            return Err(DockspaceInteractionError::PointerInputAlreadySubmitted.into());
        }
        if inputs.is_empty() {
            return Err(DockspaceInteractionError::PointerBatchEmpty.into());
        }
        let mut sequence = self
            .next_pointer_sequence
            .ok_or(DockspaceInteractionError::PointerProviderUnavailable)?;
        let mut prepared = Vec::with_capacity(inputs.len());
        for input in inputs {
            if matches!(
                input.event,
                SurfacePointerEvent::MagneticTargetingChanged { .. }
            ) {
                prepared.push((None, input));
                continue;
            }
            let segment_start = sequence;
            sequence = sequence
                .checked_add(1)
                .ok_or(DockspaceInteractionError::PointerSequenceExhausted)?;
            let edge_sequence = PointerEdgeSequence::new(sequence);
            let kind = self.surface_pointer_kind(input.manipulation_surface, input.event)?;
            let edge = PointerEdge::new(
                edge_sequence,
                PointerId::new(input.pointer.get()),
                kind,
                input.location,
                surface_capture(input.capture),
            )
            .with_metadata(input.metadata);
            let journal = PointerEdgeJournal::new(
                PointerEdgeSequence::new(segment_start),
                edge_sequence,
                vec![edge],
            )
            .map_err(|_| DockspaceInteractionError::PointerProtocolInvariant)?;
            prepared.push((Some((edge_sequence, journal)), input));
        }
        if prepared.iter().all(|(edge, _)| edge.is_none()) {
            // Preserve the provider's exact empty interval and commit guard
            // without inventing a pointer edge for a modifier-only batch.
            self.complete_pointer_journal()?;
        }
        for (prepared_edge, input) in prepared {
            let Some((edge_sequence, journal)) = prepared_edge else {
                let SurfacePointerEvent::MagneticTargetingChanged { enabled } = input.event else {
                    unreachable!("only targeting directives omit a pointer journal");
                };
                let provider = self
                    .session
                    .pointer
                    .as_ref()
                    .map(|pointer| pointer.provider().lease())
                    .ok_or(DockspaceInteractionError::PointerProviderUnavailable)?;
                self.append(crate::engine::EngineInput::UpdatePointerMagneticTargeting {
                    provider,
                    pointer: PointerId::new(input.pointer.get()),
                    enabled,
                })?;
                continue;
            };
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
            let observation = self.pointer_observation(candidate, input.receivers)?;
            let receipts = PointerReceiverReceiptBatch::new([candidate.receipt(observation)])
                .map_err(|_| DockspaceInteractionError::PointerProtocolInvariant)?;
            if let Err(error) = self.frame.submit_pointer_receiver_receipts(receipts) {
                return Err(self
                    .frame
                    .take_input_prefix_error()
                    .map_or_else(|| error.into(), Into::into));
            }
        }
        self.next_pointer_sequence = Some(sequence);
        self.pointer_input_submitted = true;
        Ok(())
    }

    fn surface_pointer_kind(
        &self,
        surface: Option<SurfaceId>,
        event: SurfacePointerEvent,
    ) -> Result<PointerEdgeKind, DockspaceRuntimeError> {
        Ok(match event {
            SurfacePointerEvent::MagneticTargetingChanged { .. } => {
                unreachable!("targeting directives are semantic inputs, not pointer edges")
            }
            SurfacePointerEvent::Moved => PointerEdgeKind::Moved,
            SurfacePointerEvent::Continuation(token) => PointerEdgeKind::Continuation(token),
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
        surface: Option<SurfaceId>,
        event: SurfaceScrollEvent,
    ) -> Result<ScrollEdge, DockspaceRuntimeError> {
        let authority = surface
            .filter(|surface| {
                !pointer_authority_blocked(
                    self.frame.candidate_engine(),
                    &self.session.presentation,
                    *surface,
                )
            })
            .and_then(|surface| self.frame.view().interaction_projection(surface))
            .map(|projection| projection.authority());
        let delivery = match authority {
            Some(authority) => Authority::Known(
                ScrollDeliveryEndpoint::new(
                    self.session.presentation_host,
                    authority.surface(),
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
        self.complete_pointer_journal()?;
        self.frame.refresh_contained_layout()?;
        Ok(())
    }

    pub(super) fn complete_pointer_journal(&mut self) -> Result<(), DockspaceRuntimeError> {
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
        candidate: &PointerReceiverCandidate,
        facts: SurfacePointerReceiverFacts<'_>,
    ) -> Result<PointerReceiverObservation, DockspaceRuntimeError> {
        if !candidate.receiver_is_applicable() {
            return Ok(PointerReceiverObservation::NotApplicable);
        }
        let delivery_projection = candidate
            .delivery_surface()
            .filter(|surface| {
                !pointer_authority_blocked(
                    self.frame.candidate_engine(),
                    &self.session.presentation,
                    *surface,
                )
            })
            .and_then(|surface| self.frame.view().interaction_projection(surface));
        let mut probes = Vec::with_capacity(candidate.probes().probes().len());
        if candidate
            .probes()
            .requires(crate::pointer_receiver::PointerReceiverProbe::Delivery)
        {
            let delivery = delivery_fact(delivery_projection.as_ref(), facts.delivery)?;
            let delivery = if let (Some(projection), Some(drag_fact)) =
                (delivery_projection, facts.drag_delivery)
            {
                let drag = delivery_fact(Some(&projection), drag_fact)?;
                PointerReceiverDelivery::from_lanes_with_scroll(
                    projection,
                    delivery.click(),
                    drag.drag(),
                    delivery.scroll(),
                )
                .map_err(|_| DockspaceInteractionError::PointerProtocolInvariant)?
            } else {
                delivery
            };
            probes.push(PointerReceiverProbeReceipt::Delivery(delivery));
        }
        if candidate
            .probes()
            .requires(crate::pointer_receiver::PointerReceiverProbe::HoverHit)
        {
            probes.push(PointerReceiverProbeReceipt::HoverHit(hover_fact(
                self.frame.view(),
                candidate.hover_surface().filter(|surface| {
                    !pointer_authority_blocked(
                        self.frame.candidate_engine(),
                        &self.session.presentation,
                        *surface,
                    )
                }),
                candidate.hover_point(),
                facts.hover,
            )?));
        }
        let presented = PresentedPointerReceiverObservation::new(probes)
            .map_err(|_| DockspaceInteractionError::PointerProtocolInvariant)?;
        Ok(PointerReceiverObservation::Presented(presented))
    }

    pub(super) fn surface_answered(&self, surface: SurfaceId) -> bool {
        self.painted_surfaces.contains_key(&surface)
            || self.frame.surface_is_measured(surface)
            || self
                .frame
                .surface_contributions()
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
        SurfacePointerCancelReason::Escape => PointerStreamCancelReason::Escape,
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
        | ReceiverFact::ClearHoverLane(_)
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
    view: crate::engine::HostFrameView<'_>,
    surface: Option<SurfaceId>,
    point: Option<LogicalPoint>,
    fact: ReceiverFact<'_>,
) -> Result<PointerReceiverHoverHit, DockspaceRuntimeError> {
    let Some(projection) = surface.and_then(|surface| view.interaction_projection(surface)) else {
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
        ReceiverFact::Dock(receiver) if receiver_matches(receiver, &projection) => {
            PointerReceiverHoverHitDisposition::Dock(receiver.descriptor.region)
        }
        ReceiverFact::NoReceiver(surface) if surface_matches(surface, &projection) => {
            PointerReceiverHoverHitDisposition::NoReceiver
        }
        ReceiverFact::ClearHoverLane(surface) if surface_matches(surface, &projection) => {
            return view
                .resolve_hover_drop_receiver(surface.surface, point)
                .map_err(|_| DockspaceInteractionError::PointerProtocolInvariant.into());
        }
        ReceiverFact::Blocked(surface) if surface_matches(surface, &projection) => {
            PointerReceiverHoverHitDisposition::Blocked
        }
        ReceiverFact::Dock(_)
        | ReceiverFact::NoReceiver(_)
        | ReceiverFact::ClearHoverLane(_)
        | ReceiverFact::Blocked(_)
        | ReceiverFact::Unknown => {
            return Ok(PointerReceiverHoverHit::unknown(
                PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
            ));
        }
    };
    PointerReceiverHoverHit::new(projection, point, disposition)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavior_tests::support::{TestPresentationHost, publish_surface};
    use crate::drop_target::DropTargetId;
    use crate::engine::DockEngine;
    use crate::graph::{Node, RootRecord, SurfacePresentation, Workspace};
    use crate::ids::{ItemId, RootId};
    use crate::policy::DockPolicy;
    use crate::presentation_hit::PresentationHitRegionKind;

    #[test]
    fn clear_hover_lane_resolves_only_an_exact_current_presentation() {
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
        let previous = PresentedDockspaceSurface::from_projection(
            engine.interaction_projection(surface).unwrap(),
        );
        publish_surface(&mut engine, &mut host, surface, bounds);
        let frame = host.begin(&engine);
        let projection = frame.view().interaction_projection(surface).unwrap();
        let current = PresentedDockspaceSurface::from_projection(projection);
        assert_eq!(previous.output, current.output);
        assert_ne!(previous.authority, current.authority);
        let target = projection
            .hit_manifest()
            .regions()
            .iter()
            .find(|region| {
                matches!(region.id().kind(), PresentationHitRegionKind::DropTarget(
                    DropTargetId::Center { root: target_root, .. }
                ) if target_root == root)
            })
            .unwrap();
        let rect = target.hit().rect();
        let point = LogicalPoint::new(
            rect.x() + rect.width() * 0.5,
            rect.y() + rect.height() * 0.5,
        )
        .unwrap();
        let receiver =
            PresentedDockReceiver::from_projection_region(projection, target.id()).unwrap();
        for (facts, expected) in [
            (
                SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&current),
                PointerReceiverHoverHitDisposition::Dock(target.id()),
            ),
            (
                SurfacePointerReceiverFacts::unknown().with_hover(&receiver),
                PointerReceiverHoverHitDisposition::Dock(target.id()),
            ),
            (
                SurfacePointerReceiverFacts::unknown().with_no_hover(&current),
                PointerReceiverHoverHitDisposition::NoReceiver,
            ),
            (
                SurfacePointerReceiverFacts::blocked_hover(&current),
                PointerReceiverHoverHitDisposition::Blocked,
            ),
            (
                SurfacePointerReceiverFacts::unknown(),
                PointerReceiverHoverHitDisposition::Unknown(
                    PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
                ),
            ),
            (
                SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&previous),
                PointerReceiverHoverHitDisposition::Unknown(
                    PointerReceiverUnknownReason::PresentationAuthorityUnavailable,
                ),
            ),
        ] {
            let observation =
                hover_fact(frame.view(), Some(surface), Some(point), facts.hover).unwrap();
            assert_eq!(observation.disposition(), expected);
            if !matches!(expected, PointerReceiverHoverHitDisposition::Unknown(_)) {
                assert_eq!(observation.output(), Some(current.output));
                assert_eq!(observation.authority(), Some(current.authority));
                assert_eq!(observation.point(), Some(point));
            }
        }
        let clear = SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&current);
        let empty_point = LogicalPoint::new(900.0, 700.0).unwrap();
        assert_eq!(
            hover_fact(frame.view(), Some(surface), Some(empty_point), clear.hover)
                .unwrap()
                .disposition(),
            PointerReceiverHoverHitDisposition::NoReceiver,
        );
        for (surface, point) in [(None, Some(point)), (Some(surface), None)] {
            assert!(matches!(
                hover_fact(frame.view(), surface, point, clear.hover)
                    .unwrap()
                    .disposition(),
                PointerReceiverHoverHitDisposition::Unknown(_),
            ));
        }
    }
}

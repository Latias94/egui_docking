//! Product-facing native pointer facts and synchronous receiver queries.

use super::{
    NativeHostProfile, NativeLocalPointerLocation, NativeLocalPointerSample, NativePlatformError,
    NativeSurfaceBinding, NativeWorkAreaBinding, RuntimeNativeState,
};
use crate::geometry::{LogicalPoint, PhysicalPoint};
use crate::ids::SurfaceId;
use crate::intent::{Authority, AuthorityUnavailableReason, PointerButton, PointerId};
use crate::pointer_journal::{
    DesktopRouteFact, DesktopWorkAreaRoute, FiniteScrollVector, NativeLocalPointerRoute,
    PhysicalScrollCoordinates, PointerAuthorityCheckpoint, PointerCaptureOwner, PointerEdge,
    PointerEdgeJournal, PointerEdgeKind, PointerEdgeLocation, PointerEventDeliveryOwner,
    PointerStateObservation, PointerStreamCancelReason, ScrollCancelReason, ScrollDeliveryEndpoint,
    ScrollDelta, ScrollDeviceId, ScrollEdge, ScrollModifiers, ScrollMomentum, ScrollPhase,
    ScrollSequenceToken,
};
use crate::presentation_hit::PresentationPointerLane;

/// Stable pointer identity supplied by a native host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct NativePointerId(u64);

impl NativePointerId {
    /// Creates one host-owned pointer identity.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub(super) const fn into_core(self) -> PointerId {
        PointerId::new(self.0)
    }
}

/// Pointer button identity reported at event time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NativePointerButton {
    /// Primary selection and docking button.
    Primary,
    /// Secondary button.
    Secondary,
    /// Middle button.
    Middle,
    /// Host-defined additional button.
    Other(u16),
}

impl NativePointerButton {
    pub(super) const fn into_core(self) -> PointerButton {
        match self {
            Self::Primary => PointerButton::Primary,
            Self::Secondary => PointerButton::Secondary,
            Self::Middle => PointerButton::Middle,
            Self::Other(button) => PointerButton::Other(button),
        }
    }
}

/// One native pointer lifecycle edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NativePointerEvent {
    /// Pointer motion.
    Moved,
    /// Rechecks one core-issued stationary action with fresh receiver facts.
    Continuation(crate::interaction::PointerContinuationToken),
    /// Reports newly acquired capture for an existing native desktop drag.
    /// Core starts a new stationary interval without executing its old deadline.
    CaptureAcquired(crate::interaction::PointerContinuationToken),
    /// One button became pressed.
    ButtonPressed(NativePointerButton),
    /// One button became released while the pointer remains live.
    ButtonReleased(NativePointerButton),
    /// A contact ended and released one button.
    ContactEnded(NativePointerButton),
    /// A buttonless pointer stream ended normally.
    StreamEnded,
    /// The provider observed a capture transition.
    CaptureChanged,
    /// The host terminated the stream abnormally.
    StreamCancelled(NativePointerCancelReason),
    /// One lossless wheel or trackpad sample.
    Scrolled(NativeScrollEvent),
}

/// Typed reason why a native host ended one pointer stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativePointerCancelReason {
    /// The host observed a device-removal boundary and invalidated this stream.
    DeviceRemoved,
    /// The platform explicitly cancelled this stream.
    ExplicitPlatformCancellation,
    /// The exact native binding which owned this stream retired.
    BindingRetired,
}

/// Event-time desktop position authority.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NativeDesktopPosition {
    /// Exact physical desktop coordinates captured with the OS event.
    Exact(PhysicalPoint),
    /// The platform did not provide an exact desktop position.
    Unknown,
}

/// Event-time native hover classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativePointerHover {
    /// One exact current docking-window binding is under the pointer.
    Dock(NativeSurfaceBinding),
    /// A foreign native window is under the pointer.
    Foreign,
    /// The host authoritatively observed no native window.
    OutsideAll,
    /// The host could not determine the top native receiver.
    Unknown,
}

/// Independent delivery or capture owner fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativePointerOwner {
    /// One exact current docking-window binding.
    Native(NativeSurfaceBinding),
    /// A receiver outside this dockspace.
    Foreign,
    /// No receiver owns the edge.
    None,
    /// The platform did not expose the owner.
    Unknown,
}

/// Complete native pointer state observed at enrollment or overload recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativePointerState {
    pointer: NativePointerId,
    pressed_buttons: Vec<NativePointerButton>,
    capture: NativePointerOwner,
}

impl NativePointerState {
    /// Creates one pointer entry in the provider's complete state roster.
    #[must_use]
    pub fn new(
        pointer: NativePointerId,
        pressed_buttons: impl IntoIterator<Item = NativePointerButton>,
        capture: NativePointerOwner,
    ) -> Self {
        Self {
            pointer,
            pressed_buttons: pressed_buttons.into_iter().collect(),
            capture,
        }
    }

    pub(super) const fn pointer(&self) -> NativePointerId {
        self.pointer
    }

    pub(super) fn pressed_buttons(&self) -> &[NativePointerButton] {
        &self.pressed_buttons
    }

    pub(super) const fn capture(&self) -> NativePointerOwner {
        self.capture
    }
}

/// Complete button and capture authority observed by a managed pointer provider.
///
/// Supply the initial roster to
/// [`DockspaceSession::enable_managed_native_host`](crate::runtime::DockspaceSession::enable_managed_native_host)
/// as part of enrollment. After overload cancellation,
/// [`DockspaceSession::report_native_pointer_roster`](crate::runtime::DockspaceSession::report_native_pointer_roster)
/// accepts fresh observations at the current committed pointer watermark until
/// one complete roster commits, including after subsequent press/release edges.
/// Unknown reports leave recovery available and never imply all buttons released.
///
/// Native capture cannot appear in the initial roster because enrollment mints
/// binding capabilities; use [`Self::Unknown`] when it cannot be expressed exactly.
/// A recovery roster may name an exact current native capture binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativePointerRoster {
    /// Complete current pointer roster. An empty roster proves all buttons released.
    Exact(Vec<NativePointerState>),
    /// The host cannot currently observe a complete pointer roster.
    Unknown,
}

/// Provider-owned identity of one native scroll device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct NativeScrollDeviceId(u64);

impl NativeScrollDeviceId {
    /// Creates one stable device identity in the provider namespace.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub(super) const fn get(self) -> u64 {
        self.0
    }
}

/// Monotonic identity of one explicitly phaseful native scroll sequence.
///
/// Identities must increase across all phaseful scroll sequences submitted by the
/// same managed-native pointer-provider lease, including sequences on different
/// streams or devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct NativeScrollSequenceId(u64);

impl NativeScrollSequenceId {
    /// Creates one sequence identity in the provider namespace.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub(super) const fn get(self) -> u64 {
        self.0
    }
}

/// Lossless two-axis scroll delta in the platform-reported unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NativeScrollDelta {
    /// Physical pixels tied to the exact native delivery binding and coordinate generation.
    PhysicalPixels { x: f64, y: f64 },
    /// Logical points in the delivery surface coordinate system.
    LogicalPoints { x: f64, y: f64 },
    /// Platform-defined line increments.
    Lines { x: f64, y: f64 },
    /// Platform-defined page increments.
    Pages { x: f64, y: f64 },
}

/// Native phase and payload shape of one scroll sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NativeScrollPhase {
    /// One standalone wheel sample.
    Discrete { delta: NativeScrollDelta },
    /// Starts a smooth sequence and may carry its first delta.
    Begin {
        sequence: NativeScrollSequenceId,
        delta: Option<NativeScrollDelta>,
    },
    /// Continues a smooth sequence.
    Update {
        sequence: NativeScrollSequenceId,
        delta: NativeScrollDelta,
    },
    /// Ends a smooth sequence and may carry its final delta.
    End {
        sequence: NativeScrollSequenceId,
        delta: Option<NativeScrollDelta>,
    },
    /// Cancels a smooth sequence for an explicit platform reason.
    Cancel {
        sequence: NativeScrollSequenceId,
        reason: NativeScrollCancelReason,
    },
}

/// Explicit native reason for cancelling a phaseful scroll sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeScrollCancelReason {
    /// The platform cancelled the gesture.
    PlatformCancelled,
    /// The exact native binding retired.
    BindingRetired,
    /// The host observed a device-removal boundary and invalidated this sequence.
    DeviceRemoved,
    /// The provider reset its sequence namespace.
    ProviderReset,
}

/// Direct-versus-momentum authority captured with one native sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeScrollMomentum {
    /// Direct user-controlled movement.
    Direct,
    /// Platform-generated momentum in the same sequence.
    Momentum,
    /// The platform did not expose event-time momentum provenance.
    Unknown,
}

/// Event-time modifier authority captured with one native sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NativeScrollModifiers {
    /// Exact modifier state at the event boundary.
    Exact {
        /// Shift was pressed.
        shift: bool,
        /// Control was pressed.
        control: bool,
        /// Alt was pressed.
        alt: bool,
        /// The platform command modifier was pressed.
        command: bool,
    },
    /// The platform did not expose event-time modifier state.
    Unknown,
}

/// One native scroll sample carried by the desktop pointer journal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeScrollEvent {
    device: NativeScrollDeviceId,
    phase: NativeScrollPhase,
    momentum: NativeScrollMomentum,
    modifiers: NativeScrollModifiers,
}

impl NativeScrollEvent {
    /// Creates one lossless native scroll sample.
    #[must_use]
    pub const fn new(
        device: NativeScrollDeviceId,
        phase: NativeScrollPhase,
        momentum: NativeScrollMomentum,
        modifiers: NativeScrollModifiers,
    ) -> Self {
        Self {
            device,
            phase,
            momentum,
            modifiers,
        }
    }

    pub(super) const fn device(self) -> NativeScrollDeviceId {
        self.device
    }

    pub(super) const fn phase(self) -> NativeScrollPhase {
        self.phase
    }

    pub(super) const fn momentum(self) -> NativeScrollMomentum {
        self.momentum
    }

    pub(super) const fn modifiers(self) -> NativeScrollModifiers {
        self.modifiers
    }
}

/// Event-time location and hover facts for one desktop-global edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeDesktopPointerLocation {
    position: NativeDesktopPosition,
    hover: NativePointerHover,
    work_area: Option<NativeWorkAreaBinding>,
}

impl NativeDesktopPointerLocation {
    /// Creates one desktop-global location fact.
    #[must_use]
    pub const fn new(
        position: NativeDesktopPosition,
        hover: NativePointerHover,
        work_area: Option<NativeWorkAreaBinding>,
    ) -> Self {
        Self {
            position,
            hover,
            work_area,
        }
    }

    pub(super) const fn position(self) -> NativeDesktopPosition {
        self.position
    }

    pub(super) const fn hover(self) -> NativePointerHover {
        self.hover
    }

    pub(super) const fn work_area(self) -> Option<NativeWorkAreaBinding> {
        self.work_area
    }
}

/// Local and desktop positions observed for the same native pointer event.
///
/// This packet requests an explicit handoff without granting desktop authority.
/// Core validates gesture continuity and exact coordinate mapping at the event's
/// reducer ordinal. The local sample retains its original coordinate authority.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeLocalToDesktopHandoff {
    local: NativeLocalPointerSample,
    desktop: NativeDesktopPointerLocation,
}

impl NativeLocalToDesktopHandoff {
    /// Reports two locations observed for one event without authorizing a handoff.
    #[must_use]
    pub const fn new(
        local: NativeLocalPointerSample,
        desktop: NativeDesktopPointerLocation,
    ) -> Self {
        Self { local, desktop }
    }

    /// Returns the immutable event-time client coordinate sample.
    #[must_use]
    pub const fn local(self) -> NativeLocalPointerSample {
        self.local
    }

    /// Returns the independently observed desktop position and native hover.
    #[must_use]
    pub const fn desktop(self) -> NativeDesktopPointerLocation {
        self.desktop
    }
}

/// Event-time coordinate lane for one joined native pointer edge.
///
/// A local sample grants no desktop origin, global hover, or outside-all proof.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NativePointerLocation {
    /// Desktop position and independently observed native hover facts.
    Desktop(NativeDesktopPointerLocation),
    /// Client coordinates captured under committed local geometry.
    NativeLocal(NativeLocalPointerLocation),
    /// Local and desktop facts requesting an explicitly validated handoff.
    LocalToDesktop(NativeLocalToDesktopHandoff),
}

/// One native pointer edge. Sequence and receiver receipts are minted by the
/// runtime, never supplied by the adapter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativePointerInput {
    pointer: NativePointerId,
    event: NativePointerEvent,
    location: NativePointerLocation,
    delivery: NativePointerOwner,
    capture: NativePointerOwner,
    metadata: crate::runtime::PointerEventMetadata,
}

impl NativePointerInput {
    /// Creates one complete event-time desktop native input fact.
    #[must_use]
    pub const fn new(
        pointer: NativePointerId,
        event: NativePointerEvent,
        location: NativeDesktopPointerLocation,
        delivery: NativePointerOwner,
        capture: NativePointerOwner,
    ) -> Self {
        Self {
            pointer,
            event,
            location: NativePointerLocation::Desktop(location),
            delivery,
            capture,
            metadata: crate::runtime::PointerEventMetadata::unknown(),
        }
    }

    /// Creates one native input fact using client-local coordinate authority.
    ///
    /// Unknown coordinates retain the original native delivery and capture so
    /// release and cancellation can still settle the existing pointer stream.
    #[must_use]
    pub const fn new_local(
        pointer: NativePointerId,
        event: NativePointerEvent,
        location: NativeLocalPointerLocation,
        delivery: NativePointerOwner,
        capture: NativePointerOwner,
    ) -> Self {
        Self {
            pointer,
            event,
            location: NativePointerLocation::NativeLocal(location),
            delivery,
            capture,
            metadata: crate::runtime::PointerEventMetadata::unknown(),
        }
    }

    /// Creates one input carrying both locations for an explicit native handoff.
    ///
    /// Recording preserves the original sample, delivery, and capture. It does
    /// not inspect the current gesture or grant permission to change its lane.
    #[must_use]
    pub const fn new_handoff(
        pointer: NativePointerId,
        event: NativePointerEvent,
        packet: NativeLocalToDesktopHandoff,
        delivery: NativePointerOwner,
        capture: NativePointerOwner,
    ) -> Self {
        Self {
            pointer,
            event,
            location: NativePointerLocation::LocalToDesktop(packet),
            delivery,
            capture,
            metadata: crate::runtime::PointerEventMetadata::unknown(),
        }
    }

    /// Attaches host-observed device and monotonic clock facts to this edge.
    #[must_use]
    pub const fn with_metadata(mut self, metadata: crate::runtime::PointerEventMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    pub(super) const fn pointer(self) -> NativePointerId {
        self.pointer
    }

    pub(super) const fn event(self) -> NativePointerEvent {
        self.event
    }

    /// Returns the event-time coordinate lane retained by this input.
    #[must_use]
    pub const fn location(self) -> NativePointerLocation {
        self.location
    }

    pub(super) const fn delivery(self) -> NativePointerOwner {
        self.delivery
    }

    pub(super) const fn capture(self) -> NativePointerOwner {
        self.capture
    }
}

/// Modifier-projected scroll direction used for receiver admission.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeProjectedScrollDelta {
    x: f64,
    y: f64,
}

impl NativeProjectedScrollDelta {
    pub(in crate::runtime) const fn from_core(delta: FiniteScrollVector) -> Self {
        Self {
            x: delta.x(),
            y: delta.y(),
        }
    }

    /// Returns horizontal content movement.
    #[must_use]
    pub const fn x(self) -> f64 {
        self.x
    }

    /// Returns vertical content movement.
    #[must_use]
    pub const fn y(self) -> f64 {
        self.y
    }
}

/// Core-owned scroll receiver requirement for one exact native edge.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(
    clippy::large_enum_variant,
    reason = "receiver authority stays inline to avoid allocating on smooth-scroll input"
)]
pub enum NativeScrollReceiverChallenge {
    /// Resolve the frontmost receiver at the query point.
    Spatial {
        /// Modifier-projected direction, when the edge carries a directional sample.
        projected_delta: Option<NativeProjectedScrollDelta>,
    },
    /// Prove that the receiver frozen at sequence start still owns delivery.
    Locked {
        /// Exact receiver capability frozen by core for this sequence.
        receiver: super::super::PresentedDockReceiver,
        /// Modifier-projected direction, when the edge carries a directional sample.
        projected_delta: Option<NativeProjectedScrollDelta>,
    },
}

impl NativeScrollReceiverChallenge {
    /// Returns the projected direction used for receiver admission.
    #[must_use]
    pub const fn projected_delta(&self) -> Option<NativeProjectedScrollDelta> {
        match self {
            Self::Spatial { projected_delta }
            | Self::Locked {
                projected_delta, ..
            } => *projected_delta,
        }
    }
}

/// One independent product-level receiver question.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(
    clippy::large_enum_variant,
    reason = "receiver authority stays inline to avoid allocating on smooth-scroll input"
)]
pub enum NativeReceiverPurpose {
    /// Resolve the framework receiver for the click lane.
    ClickDelivery,
    /// Resolve the framework receiver for the drag lane.
    DragDelivery,
    /// Resolve the framework receiver for one scroll edge.
    ScrollDelivery(NativeScrollReceiverChallenge),
    /// Resolve the frontmost receiver at the event-time hover point.
    HoverHit,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeReceiverQuery {
    pub(in crate::runtime) purpose: NativeReceiverPurpose,
    pub(in crate::runtime) presented_surface: super::super::PresentedDockspaceSurface,
    pub(in crate::runtime) point: Option<LogicalPoint>,
    pub(in crate::runtime) metadata: crate::runtime::PointerEventMetadata,
}

impl NativeReceiverQuery {
    /// Returns the exact receiver lane or hover question.
    #[must_use]
    pub const fn purpose(&self) -> &NativeReceiverPurpose {
        &self.purpose
    }

    /// Returns the device and original capture time of the edge being resolved.
    ///
    /// Retrying an unpublished frame retains these facts rather than sampling
    /// the host clock again. Unknown device facts remain explicit.
    #[must_use]
    pub const fn metadata(self) -> crate::runtime::PointerEventMetadata {
        self.metadata
    }

    /// Returns the exact logical surface which must answer the question.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.presented_surface.surface()
    }

    /// Returns the exact presented surface capability for surface-qualified answers.
    #[must_use]
    pub const fn presented_surface(self) -> super::super::PresentedDockspaceSurface {
        self.presented_surface
    }

    /// Binds one opaque paint-time descriptor to this exact presented output.
    ///
    /// A descriptor from another engine, surface, or semantic output is rejected.
    #[must_use]
    pub fn bind_receiver(
        self,
        descriptor: &super::super::DockspaceReceiverDescriptor,
    ) -> Option<super::super::PresentedDockReceiver> {
        if !descriptor.supports_lane(self.required_lane()) {
            return None;
        }
        self.presented_surface.bind_receiver(descriptor)
    }

    /// Qualifies a registered framework receiver as clearing the hover lane.
    ///
    /// This checks the descriptor's exact output and bounds without requiring a
    /// HoverDrop semantic lane. The adapter must independently prove that this
    /// registered widget is the unique frontmost hit in the presented pass.
    /// Core, not this descriptor, selects the source-suppressed semantic winner.
    #[must_use]
    pub fn clear_hover_lane(
        self,
        descriptor: &super::super::DockspaceReceiverDescriptor,
    ) -> Option<super::super::PresentedDockspaceSurface> {
        if !matches!(self.purpose, NativeReceiverPurpose::HoverHit)
            || !descriptor.bounds().contains(self.point?)
        {
            return None;
        }
        self.presented_surface.bind_receiver(descriptor)?;
        Some(self.presented_surface)
    }

    /// Returns whether this query belongs to the exact native window incarnation.
    #[must_use]
    pub fn matches_native_binding(self, binding: super::NativeSurfaceBinding) -> bool {
        self.presented_surface.matches_native_binding(binding)
    }

    /// Returns whether this query names the exact semantic output painted by the host.
    #[must_use]
    pub fn matches_semantic_output(self, output: super::super::DockspaceSemanticOutput) -> bool {
        self.presented_surface.matches_semantic_output(output)
    }

    /// Returns whether this query was minted from the exact committed surface
    /// emission which owns an adapter artifact.
    #[must_use]
    pub fn matches_presentation_artifact(
        self,
        artifact: super::super::SurfacePresentationArtifact,
    ) -> bool {
        self.presented_surface
            .matches_presentation_artifact(artifact)
    }

    /// Returns the exact point required for a known answer.
    ///
    /// Initial spatial queries use the event-time point. A smooth-scroll
    /// continuation uses the probe point frozen with its sequence owner.
    #[must_use]
    pub const fn point(self) -> Option<LogicalPoint> {
        self.point
    }

    const fn required_lane(self) -> PresentationPointerLane {
        match self.purpose {
            NativeReceiverPurpose::ClickDelivery => PresentationPointerLane::Click,
            NativeReceiverPurpose::DragDelivery => PresentationPointerLane::Drag,
            NativeReceiverPurpose::ScrollDelivery(_) => PresentationPointerLane::Scroll,
            NativeReceiverPurpose::HoverHit => PresentationPointerLane::HoverDrop,
        }
    }
}

/// One synchronous receiver answer. A known capability is copyable and
/// already bound to a final presentation output; no raw hit graph crosses the
/// runtime boundary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NativeReceiverAnswer {
    /// A concrete docking receiver accepted the edge.
    Dock(super::super::PresentedDockReceiver),
    /// The current surface proved that no docking receiver accepted the edge.
    NoReceiver(super::super::PresentedDockspaceSurface),
    /// The exact output proves no framework receiver blocks HoverDrop.
    ///
    /// This is not a semantic winner or known-empty assertion. Core resolves
    /// the winner at the edge's exact point after preceding edges have reduced.
    /// It is valid only for hover queries; unknown widgets must not clear a lane.
    ClearHoverLane(super::super::PresentedDockspaceSurface),
    /// A higher framework receiver blocked docking.
    Blocked(super::super::PresentedDockspaceSurface),
    /// Receiver authority was unavailable and the edge must fail closed.
    Unknown,
}

impl RuntimeNativeState {
    pub(super) fn compile_pointer_roster(
        &self,
        roster: NativePointerRoster,
    ) -> Result<PointerAuthorityCheckpoint, NativePlatformError> {
        self.recorder
            .preflight_native_transport_append(1, pointer_authority_descendant_bytes(&roster)?)
            .map_err(NativePlatformError::from_ingress)?;
        compile_pointer_authority(roster, self.recorder.pointer_through(), |owner| {
            self.capture_owner(owner)
        })
    }

    pub(super) fn record_pointer_authority(
        &mut self,
        checkpoint: PointerAuthorityCheckpoint,
    ) -> Result<(), NativePlatformError> {
        let watermark = self.recorder.pointer_through();
        let journal = PointerEdgeJournal::new(watermark, watermark, Vec::new())
            .and_then(|journal| journal.with_authority_checkpoint(checkpoint))
            .map_err(|_| NativePlatformError::InvalidPointerFacts)?;
        self.recorder
            .record_pointer_segment(journal)
            .map_err(NativePlatformError::from_ingress)?;
        Ok(())
    }

    pub(super) fn prepare_pointer_journals(
        &self,
        inputs: impl IntoIterator<Item = NativePointerInput>,
        reclaimable: Option<crate::backend_ingress::BackendIngressCommitWatermark>,
    ) -> Result<Vec<PointerEdgeJournal>, NativePlatformError> {
        if self.profile != NativeHostProfile::ManagedDesktop {
            return Err(NativePlatformError::HostProfileMismatch);
        }
        let limit = self
            .recorder
            .native_pointer_preparation_limit(reclaimable)
            .map_err(NativePlatformError::from_ingress)?;
        let mut journals = Vec::new();
        let mut previous = self.recorder.pointer_through();
        for input in inputs {
            if limit.is_some_and(|limit| journals.len() >= limit) {
                return Err(NativePlatformError::InputCapacity);
            }
            if journals.len() == journals.capacity() {
                if let Some(limit) = limit {
                    journals
                        .try_reserve_exact(limit)
                        .map_err(|_| NativePlatformError::InputCapacity)?;
                    if journals.capacity() > limit {
                        return Err(NativePlatformError::InputCapacity);
                    }
                } else {
                    journals
                        .try_reserve(1)
                        .map_err(|_| NativePlatformError::InputCapacity)?;
                }
            }
            let journal = self.compile_pointer_journal(input, previous)?;
            previous = journal.through();
            journals.push(journal);
        }
        Ok(journals)
    }

    pub(super) fn append_prepared_pointer_journals(
        &mut self,
        journals: Vec<PointerEdgeJournal>,
        atomic_commit: bool,
    ) -> Result<(), NativePlatformError> {
        let descendants = journals.iter().try_fold(0_usize, |bytes, journal| {
            bytes
                .checked_add(journal.owned_contents_capacity_bytes())
                .ok_or(NativePlatformError::InputCapacity)
        })?;
        self.recorder
            .preflight_native_transport_append(journals.len(), descendants)
            .map_err(NativePlatformError::from_ingress)?;
        let savepoint = self.recorder.savepoint();
        for journal in journals {
            if let Err(error) = self.recorder.record_pointer_segment(journal) {
                self.recorder
                    .rollback_to(savepoint)
                    .map_err(|_| NativePlatformError::ProtocolInvariant)?;
                return Err(NativePlatformError::from_ingress(error));
            }
        }
        if atomic_commit {
            self.recorder
                .finish_atomic_append(savepoint)
                .map_err(|_| NativePlatformError::ProtocolInvariant)?;
        }
        Ok(())
    }

    pub(super) fn record_pointer(
        &mut self,
        input: NativePointerInput,
    ) -> Result<(), NativePlatformError> {
        self.recorder
            .preflight_native_transport_append(
                1,
                PointerEdgeJournal::single_edge_owned_contents_capacity_bytes(),
            )
            .map_err(NativePlatformError::from_ingress)?;
        let journal = self.compile_pointer_journal(input, self.recorder.pointer_through())?;
        self.recorder
            .record_pointer_segment(journal)
            .map_err(NativePlatformError::from_ingress)?;
        Ok(())
    }

    fn compile_pointer_journal(
        &self,
        input: NativePointerInput,
        previous: crate::pointer_journal::PointerEdgeSequence,
    ) -> Result<PointerEdgeJournal, NativePlatformError> {
        if self.profile != NativeHostProfile::ManagedDesktop {
            return Err(NativePlatformError::HostProfileMismatch);
        }
        let sequence = previous
            .checked_next()
            .ok_or(NativePlatformError::PointerSequenceExhausted)?;
        let location = self.pointer_location(input.location())?;
        let delivery = self.delivery_owner(input.delivery())?;
        let capture = self.capture_owner(input.capture())?;
        let kind = match input.event() {
            NativePointerEvent::Moved => PointerEdgeKind::Moved,
            NativePointerEvent::Continuation(token) => PointerEdgeKind::Continuation(token),
            NativePointerEvent::CaptureAcquired(token) => PointerEdgeKind::CaptureAcquired(token),
            NativePointerEvent::ButtonPressed(button) => {
                PointerEdgeKind::ButtonPressed(button.into_core())
            }
            NativePointerEvent::ButtonReleased(button) => {
                PointerEdgeKind::ButtonReleased(button.into_core())
            }
            NativePointerEvent::ContactEnded(button) => {
                PointerEdgeKind::ContactEnded(button.into_core())
            }
            NativePointerEvent::StreamEnded => PointerEdgeKind::StreamEnded,
            NativePointerEvent::CaptureChanged => PointerEdgeKind::CaptureChanged,
            NativePointerEvent::StreamCancelled(reason) => {
                PointerEdgeKind::StreamCancelled(native_pointer_cancel_reason(reason))
            }
            NativePointerEvent::Scrolled(scroll) => {
                PointerEdgeKind::Scrolled(Self::scroll_edge(scroll)?)
            }
        };
        let edge = PointerEdge::new_with_delivery(
            sequence,
            input.pointer().into_core(),
            kind,
            location,
            delivery,
            capture,
        )
        .with_metadata(input.metadata);
        PointerEdgeJournal::new(previous, sequence, vec![edge])
            .map_err(|_| NativePlatformError::ProtocolInvariant)
    }

    fn scroll_edge(event: NativeScrollEvent) -> Result<ScrollEdge, NativePlatformError> {
        // The exact presentation endpoint belongs to the reducer ordinal. A
        // platform snapshot earlier in this same batch may replace its binding
        // or coordinate generation, so the recorder must not copy the previous
        // published projection here.
        let delivery = Authority::Unknown(AuthorityUnavailableReason::NotReported);
        let (sequence, phase, delta) = match event.phase() {
            NativeScrollPhase::Discrete { delta } => (None, ScrollPhase::Discrete, Some(delta)),
            NativeScrollPhase::Begin { sequence, delta } => (
                Some(ScrollSequenceToken::new(sequence.get())),
                ScrollPhase::Begin,
                delta,
            ),
            NativeScrollPhase::Update { sequence, delta } => (
                Some(ScrollSequenceToken::new(sequence.get())),
                ScrollPhase::Update,
                Some(delta),
            ),
            NativeScrollPhase::End { sequence, delta } => (
                Some(ScrollSequenceToken::new(sequence.get())),
                ScrollPhase::End,
                delta,
            ),
            NativeScrollPhase::Cancel { sequence, reason } => (
                Some(ScrollSequenceToken::new(sequence.get())),
                ScrollPhase::Cancel(native_scroll_cancel_reason(reason)),
                None,
            ),
        };
        let delta = delta
            .map(|delta| native_scroll_delta(delta, delivery))
            .transpose()?;
        let momentum = match event.momentum() {
            NativeScrollMomentum::Direct => Authority::Known(ScrollMomentum::Direct),
            NativeScrollMomentum::Momentum => Authority::Known(ScrollMomentum::Momentum),
            NativeScrollMomentum::Unknown => {
                Authority::Unknown(AuthorityUnavailableReason::NotReported)
            }
        };
        let modifiers = match event.modifiers() {
            NativeScrollModifiers::Exact {
                shift,
                control,
                alt,
                command,
            } => Authority::Known(ScrollModifiers::new(shift, control, alt, command)),
            NativeScrollModifiers::Unknown => {
                Authority::Unknown(AuthorityUnavailableReason::NotReported)
            }
        };
        ScrollEdge::new(
            ScrollDeviceId::new(event.device().get()),
            sequence,
            phase,
            delta,
            momentum,
            modifiers,
            delivery,
        )
        .map_err(|_| NativePlatformError::InvalidPointerFacts)
    }

    fn pointer_location(
        &self,
        location: NativePointerLocation,
    ) -> Result<PointerEdgeLocation, NativePlatformError> {
        match location {
            NativePointerLocation::Desktop(location) => Ok(PointerEdgeLocation::Desktop {
                route: self.desktop_pointer_route(location)?,
            }),
            NativePointerLocation::NativeLocal(location) => {
                let route = match location {
                    NativeLocalPointerLocation::Exact(sample) => {
                        Authority::Known(self.native_local_pointer_route(sample)?)
                    }
                    NativeLocalPointerLocation::Unknown => {
                        Authority::Unknown(AuthorityUnavailableReason::NotReported)
                    }
                };
                Ok(PointerEdgeLocation::NativeLocal { route })
            }
            NativePointerLocation::LocalToDesktop(packet) => {
                Ok(PointerEdgeLocation::NativeLocalToDesktop {
                    local: self.native_local_pointer_route(packet.local())?,
                    desktop: self.desktop_pointer_route(packet.desktop())?,
                })
            }
        }
    }

    fn native_local_pointer_route(
        &self,
        sample: NativeLocalPointerSample,
    ) -> Result<NativeLocalPointerRoute, NativePlatformError> {
        let coordinates = sample.coordinates();
        if coordinates.backend() != self.recorder.lease() {
            return Err(NativePlatformError::InvalidPointerFacts);
        }
        self.validate_pointer_binding(coordinates.binding())?;
        // Do not reinterpret an old sample using recorder-time geometry.
        // Current coordinate and handoff authority belong to the reducer ordinal.
        Ok(NativeLocalPointerRoute::new(
            coordinates.binding().binding,
            coordinates.coordinate_generation(),
            sample.client_position(),
            sample.surface_position(),
        ))
    }

    fn desktop_pointer_route(
        &self,
        location: NativeDesktopPointerLocation,
    ) -> Result<DesktopRouteFact, NativePlatformError> {
        let position = location.position();
        let hover = location.hover();
        let work_area = location.work_area();
        let position = match position {
            NativeDesktopPosition::Exact(position) => Authority::Known(position),
            NativeDesktopPosition::Unknown => {
                Authority::Unknown(AuthorityUnavailableReason::NotReported)
            }
        };
        let route = match hover {
            NativePointerHover::Dock(binding) => {
                self.validate_pointer_binding(binding)?;
                if work_area.is_some() {
                    return Err(NativePlatformError::InvalidPointerFacts);
                }
                DesktopRouteFact::dock_from_desktop_position(binding.binding, position)
            }
            NativePointerHover::Foreign => {
                if work_area.is_some() {
                    return Err(NativePlatformError::InvalidPointerFacts);
                }
                DesktopRouteFact::foreign(position)
            }
            NativePointerHover::OutsideAll => {
                let work_area = match work_area {
                    Some(work_area) => {
                        self.validate_work_area_binding(work_area)?;
                        Authority::Known(DesktopWorkAreaRoute::new(
                            work_area.provider,
                            work_area.generation,
                            work_area.token,
                        ))
                    }
                    None => Authority::Unknown(AuthorityUnavailableReason::NotReported),
                };
                DesktopRouteFact::no_window(position, work_area)
            }
            NativePointerHover::Unknown => {
                if work_area.is_some() {
                    return Err(NativePlatformError::InvalidPointerFacts);
                }
                DesktopRouteFact::unknown(position, AuthorityUnavailableReason::NotReported)
            }
        };
        Ok(route)
    }

    fn delivery_owner(
        &self,
        owner: NativePointerOwner,
    ) -> Result<Authority<PointerEventDeliveryOwner>, NativePlatformError> {
        Ok(match owner {
            NativePointerOwner::Native(binding) => {
                self.validate_pointer_binding(binding)?;
                Authority::Known(PointerEventDeliveryOwner::Native(binding.binding))
            }
            NativePointerOwner::Foreign => Authority::Known(PointerEventDeliveryOwner::Foreign),
            NativePointerOwner::None => Authority::Known(PointerEventDeliveryOwner::None),
            NativePointerOwner::Unknown => {
                Authority::Unknown(AuthorityUnavailableReason::NotReported)
            }
        })
    }

    fn capture_owner(
        &self,
        owner: NativePointerOwner,
    ) -> Result<Authority<PointerCaptureOwner>, NativePlatformError> {
        Ok(match owner {
            NativePointerOwner::Native(binding) => {
                self.validate_pointer_binding(binding)?;
                Authority::Known(PointerCaptureOwner::Native(binding.binding))
            }
            NativePointerOwner::Foreign => Authority::Known(PointerCaptureOwner::Foreign),
            NativePointerOwner::None => Authority::Known(PointerCaptureOwner::None),
            NativePointerOwner::Unknown => {
                Authority::Unknown(AuthorityUnavailableReason::NotReported)
            }
        })
    }

    fn validate_pointer_binding(
        &self,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativePlatformError> {
        if binding.provider != self.provider() || !self.binding_is_live(binding) {
            return Err(NativePlatformError::InvalidPointerFacts);
        }
        Ok(())
    }

    fn validate_work_area_binding(
        &self,
        work_area: NativeWorkAreaBinding,
    ) -> Result<(), NativePlatformError> {
        if work_area.provider != self.provider()
            || self.work_area_binding(work_area.token) != Some(work_area)
        {
            return Err(NativePlatformError::InvalidPointerFacts);
        }
        Ok(())
    }
}

const fn native_pointer_cancel_reason(
    reason: NativePointerCancelReason,
) -> PointerStreamCancelReason {
    match reason {
        NativePointerCancelReason::DeviceRemoved => PointerStreamCancelReason::DeviceRemoved,
        NativePointerCancelReason::ExplicitPlatformCancellation => {
            PointerStreamCancelReason::ExplicitPlatformCancellation
        }
        NativePointerCancelReason::BindingRetired => PointerStreamCancelReason::BindingRetired,
    }
}

pub(super) fn compile_initial_pointer_authority(
    roster: NativePointerRoster,
) -> Result<PointerAuthorityCheckpoint, NativePlatformError> {
    let watermark = crate::pointer_journal::PointerEdgeSequence::new(0);
    compile_pointer_authority(roster, watermark, |owner| match owner {
        NativePointerOwner::Foreign => Ok(Authority::Known(PointerCaptureOwner::Foreign)),
        NativePointerOwner::None => Ok(Authority::Known(PointerCaptureOwner::None)),
        NativePointerOwner::Unknown => {
            Ok(Authority::Unknown(AuthorityUnavailableReason::NotReported))
        }
        // Native bindings are minted by enrollment and cannot pre-exist it.
        NativePointerOwner::Native(_) => Err(NativePlatformError::InvalidPointerFacts),
    })
}

pub(super) fn pointer_authority_descendant_bytes(
    roster: &NativePointerRoster,
) -> Result<usize, NativePlatformError> {
    let fixed = PointerEdgeJournal::empty_owned_contents_capacity_bytes();
    let NativePointerRoster::Exact(states) = roster else {
        return Ok(fixed);
    };
    let bytes = states
        .len()
        .checked_mul(size_of::<PointerStateObservation>())
        .and_then(|bytes| fixed.checked_add(bytes))
        .ok_or(NativePlatformError::InputCapacity)?;
    states.iter().try_fold(bytes, |bytes, state| {
        state
            .pressed_buttons()
            .len()
            .checked_mul(size_of::<PointerButton>())
            .and_then(|buttons| bytes.checked_add(buttons))
            .ok_or(NativePlatformError::InputCapacity)
    })
}

fn compile_pointer_authority(
    roster: NativePointerRoster,
    watermark: crate::pointer_journal::PointerEdgeSequence,
    resolve_capture: impl Fn(
        NativePointerOwner,
    ) -> Result<Authority<PointerCaptureOwner>, NativePlatformError>,
) -> Result<PointerAuthorityCheckpoint, NativePlatformError> {
    match roster {
        NativePointerRoster::Exact(states) => {
            let mut observations = Vec::with_capacity(states.len());
            for state in states {
                let mut buttons = Vec::with_capacity(state.pressed_buttons().len());
                buttons.extend(
                    state
                        .pressed_buttons()
                        .iter()
                        .copied()
                        .map(NativePointerButton::into_core),
                );
                let capture = resolve_capture(state.capture())?;
                observations.push(
                    PointerStateObservation::new(state.pointer().into_core(), buttons, capture)
                        .map_err(|_| NativePlatformError::InvalidPointerFacts)?,
                );
            }
            PointerAuthorityCheckpoint::known(watermark, observations)
                .map_err(|_| NativePlatformError::InvalidPointerFacts)
        }
        NativePointerRoster::Unknown => Ok(PointerAuthorityCheckpoint::unknown(
            watermark,
            AuthorityUnavailableReason::NotReported,
        )),
    }
}

const fn native_scroll_cancel_reason(reason: NativeScrollCancelReason) -> ScrollCancelReason {
    match reason {
        NativeScrollCancelReason::PlatformCancelled => ScrollCancelReason::PlatformCancelled,
        NativeScrollCancelReason::BindingRetired => ScrollCancelReason::BindingRetired,
        NativeScrollCancelReason::DeviceRemoved => ScrollCancelReason::DeviceRemoved,
        NativeScrollCancelReason::ProviderReset => ScrollCancelReason::ProviderReset,
    }
}

fn native_scroll_delta(
    delta: NativeScrollDelta,
    delivery: Authority<ScrollDeliveryEndpoint>,
) -> Result<ScrollDelta, NativePlatformError> {
    let (x, y) = match delta {
        NativeScrollDelta::PhysicalPixels { x, y }
        | NativeScrollDelta::LogicalPoints { x, y }
        | NativeScrollDelta::Lines { x, y }
        | NativeScrollDelta::Pages { x, y } => (x, y),
    };
    let vector =
        FiniteScrollVector::new(x, y).map_err(|_| NativePlatformError::InvalidPointerFacts)?;
    Ok(match delta {
        NativeScrollDelta::PhysicalPixels { .. } => {
            let coordinates = match delivery {
                Authority::Known(endpoint) => endpoint.binding().map_or(
                    Authority::Unknown(AuthorityUnavailableReason::NotReported),
                    |binding| {
                        Authority::Known(PhysicalScrollCoordinates::new(
                            binding,
                            endpoint.coordinate_generation(),
                        ))
                    },
                ),
                Authority::Unknown(reason) => Authority::Unknown(reason),
            };
            ScrollDelta::PhysicalPixels {
                delta: vector,
                coordinates,
            }
        }
        NativeScrollDelta::LogicalPoints { .. } => ScrollDelta::LogicalPoints(vector),
        NativeScrollDelta::Lines { .. } => ScrollDelta::Lines(vector),
        NativeScrollDelta::Pages { .. } => ScrollDelta::Pages(vector),
    })
}

#[cfg(test)]
mod handoff_tests {
    use super::*;
    use crate::coordinates::CoordinateSnapshot;
    use crate::geometry::{PhysicalSize, ScaleFactor};
    use crate::graph::{Node, RootRecord, SurfacePresentation, Workspace};
    use crate::ids::{ItemId, RootId};
    use crate::platform::WindowCoordinateObservation;
    use crate::policy::DockPolicy;
    use crate::runtime::{
        DockspaceSession, HostInputOutcome, HostWindowToken, NativeCreateStrategy,
        NativeHostErrorKind, NativeLocalCoordinates, NativeLocalPointerFact, PointerDeviceKind,
        PointerEventMetadata, SurfaceUnavailableReason,
    };
    use crate::viewport::{CoordinateGeneration, CoordinateObservationGeneration};
    use std::time::Duration;

    fn fixture() -> (
        DockspaceSession,
        NativeSurfaceBinding,
        NativeLocalPointerSample,
    ) {
        let surface = SurfaceId::new(1);
        let root = RootId::new(1);
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
        builder.set_root(root, RootRecord::new(tabs));
        builder.set_surface(surface, SurfacePresentation::with_main(root));
        let mut session = DockspaceSession::from_workspace_for_test(
            builder.build().unwrap(),
            DockPolicy::default(),
        )
        .unwrap();
        session
            .enable_managed_native_host(
                NativeCreateStrategy::Staged,
                NativePointerRoster::Exact(Vec::new()),
            )
            .unwrap();
        session
            .register_native_root(surface, HostWindowToken::new(41))
            .unwrap();
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let report = frame.commit().unwrap();
        let [HostInputOutcome::NativeSurfaceRegistered { binding }] = report.inputs() else {
            panic!("the fixture registers one exact native root");
        };
        let binding = *binding;
        let native_scale = ScaleFactor::new(2.0).unwrap();
        let presentation_scale = ScaleFactor::new(3.0).unwrap();
        let size = PhysicalSize::new(900.0, 600.0).unwrap();
        let unknown = Authority::Unknown(AuthorityUnavailableReason::NotReported);
        let observation = WindowCoordinateObservation::new(
            binding.binding,
            CoordinateObservationGeneration::new(3),
            unknown,
            unknown,
            Authority::Known(native_scale),
            Authority::Known(presentation_scale),
        )
        .with_content_size(Authority::Known(size));
        let snapshot = CoordinateSnapshot::from_observation(
            binding.binding,
            CoordinateGeneration::new(7),
            observation,
        )
        .unwrap();
        // Compilation checks structural ownership only. Current coordinate and
        // gesture authority are intentionally left for the reducer to prove.
        let coordinates = NativeLocalCoordinates::new(
            session.native.as_ref().unwrap().recorder.lease(),
            binding,
            snapshot,
        );
        let sample = coordinates
            .capture(
                binding,
                NativeLocalPointerFact::new(
                    PhysicalPoint::new(60.0, 30.0).unwrap(),
                    size,
                    native_scale,
                ),
                coordinates.geometry(),
            )
            .unwrap();
        (session, binding, sample)
    }

    fn desktop(binding: NativeSurfaceBinding) -> NativeDesktopPointerLocation {
        NativeDesktopPointerLocation::new(
            NativeDesktopPosition::Exact(PhysicalPoint::new(460.0, -270.0).unwrap()),
            NativePointerHover::Dock(binding),
            None,
        )
    }

    #[test]
    fn handoff_compilation_preserves_both_facts_without_reading_gesture_authority() {
        let (session, binding, sample) = fixture();
        let packet = NativeLocalToDesktopHandoff::new(sample, desktop(binding));
        assert_eq!(packet.local(), sample);
        assert_eq!(packet.desktop(), desktop(binding));
        let metadata =
            PointerEventMetadata::new(PointerDeviceKind::Mouse, Duration::from_millis(9));
        let input = NativePointerInput::new_handoff(
            NativePointerId::new(11),
            NativePointerEvent::Moved,
            packet,
            NativePointerOwner::Native(binding),
            NativePointerOwner::Native(binding),
        )
        .with_metadata(metadata);
        assert_eq!(
            input.location(),
            NativePointerLocation::LocalToDesktop(packet)
        );
        let native = session.native.as_ref().unwrap();
        let recorded = native.recorder.recorded_through();
        let through = native.recorder.pointer_through();
        let journal = native.compile_pointer_journal(input, through).unwrap();
        let [edge] = journal.edges() else {
            panic!("one input compiles to one edge")
        };
        let PointerEdgeLocation::NativeLocalToDesktop { local, desktop } = edge.location() else {
            panic!("the raw journal retains the explicit dual location");
        };
        assert_eq!(local.binding(), binding.binding);
        assert_eq!(local.coordinate_generation(), CoordinateGeneration::new(7));
        assert_eq!(local.client_position(), sample.client_position());
        assert_eq!(local.surface_position(), sample.surface_position());
        assert_eq!(
            desktop.position(),
            Authority::Known(PhysicalPoint::new(460.0, -270.0).unwrap())
        );
        assert_eq!(
            desktop.hovered(),
            Authority::Known(crate::pointer_journal::DesktopHoveredWindow::Dock(
                binding.binding
            ))
        );
        assert_eq!(edge.desktop_route(), None);
        assert_eq!(edge.pointer(), PointerId::new(11));
        assert_eq!(edge.kind(), PointerEdgeKind::Moved);
        assert_eq!(edge.metadata(), metadata);
        assert_eq!(
            edge.delivery_owner(),
            Authority::Known(PointerEventDeliveryOwner::Native(binding.binding))
        );
        assert_eq!(
            edge.capture_owner(),
            Authority::Known(PointerCaptureOwner::Native(binding.binding))
        );
        assert_eq!(native.recorder.recorded_through(), recorded);
        assert_eq!(native.recorder.pointer_through(), through);
    }

    #[test]
    fn foreign_handoff_facts_reject_the_whole_batch_before_recording_a_prefix() {
        let (mut session, binding, sample) = fixture();
        let (_, foreign_binding, foreign_sample) = fixture();
        let valid = NativePointerInput::new_local(
            NativePointerId::new(11),
            NativePointerEvent::Moved,
            NativeLocalPointerLocation::Exact(sample),
            NativePointerOwner::Native(binding),
            NativePointerOwner::None,
        );
        let native = session.native.as_ref().unwrap();
        let before = native.recorder.pending_batch().unwrap();
        let recorded = native.recorder.recorded_through();
        let through = native.recorder.pointer_through();
        for packet in [
            NativeLocalToDesktopHandoff::new(foreign_sample, desktop(binding)),
            NativeLocalToDesktopHandoff::new(sample, desktop(foreign_binding)),
        ] {
            let invalid = NativePointerInput::new_handoff(
                NativePointerId::new(11),
                NativePointerEvent::Moved,
                packet,
                NativePointerOwner::Native(binding),
                NativePointerOwner::None,
            );
            let error = session
                .record_native_pointer_batch([valid, invalid])
                .unwrap_err();
            assert_eq!(error.native_kind(), Some(NativeHostErrorKind::InvalidFacts));
            let native = session.native.as_ref().unwrap();
            assert_eq!(native.recorder.pending_batch().unwrap(), before);
            assert_eq!(native.recorder.recorded_through(), recorded);
            assert_eq!(native.recorder.pointer_through(), through);
        }
        let corrected = NativePointerInput::new_handoff(
            NativePointerId::new(11),
            NativePointerEvent::Moved,
            NativeLocalToDesktopHandoff::new(sample, desktop(binding)),
            NativePointerOwner::Native(binding),
            NativePointerOwner::None,
        );
        session
            .record_native_pointer_batch([valid, corrected])
            .unwrap();
        assert_eq!(
            session
                .native
                .as_ref()
                .unwrap()
                .recorder
                .pointer_through()
                .get(),
            through.get() + 2
        );
    }
}

#[cfg(test)]
mod capacity_tests {
    use super::*;

    #[test]
    fn prospective_checkpoint_bytes_match_compiled_odd_sized_rosters() {
        for count in [0, 1, 3, 5, 17] {
            let roster = NativePointerRoster::Exact(
                (0..count)
                    .map(|index| {
                        NativePointerState::new(
                            NativePointerId::new(index),
                            [
                                NativePointerButton::Primary,
                                NativePointerButton::Secondary,
                                NativePointerButton::Other(7),
                            ],
                            NativePointerOwner::None,
                        )
                    })
                    .collect(),
            );
            let expected = pointer_authority_descendant_bytes(&roster).unwrap();
            let checkpoint = compile_initial_pointer_authority(roster).unwrap();
            let watermark = crate::pointer_journal::PointerEdgeSequence::new(0);
            let journal = PointerEdgeJournal::new(watermark, watermark, Vec::new())
                .unwrap()
                .with_authority_checkpoint(checkpoint)
                .unwrap();
            assert_eq!(journal.owned_contents_capacity_bytes(), expected);
        }
        let roster = NativePointerRoster::Unknown;
        let expected = pointer_authority_descendant_bytes(&roster).unwrap();
        let checkpoint = compile_initial_pointer_authority(roster).unwrap();
        let watermark = crate::pointer_journal::PointerEdgeSequence::new(0);
        let journal = PointerEdgeJournal::new(watermark, watermark, Vec::new())
            .unwrap()
            .with_authority_checkpoint(checkpoint)
            .unwrap();
        assert_eq!(journal.owned_contents_capacity_bytes(), expected);
    }
}

//! Product-facing native pointer facts and synchronous receiver queries.

use super::{
    NativeHostProfile, NativePlatformError, NativeSurfaceBinding, NativeWorkAreaBinding,
    RuntimeNativeState,
};
use crate::geometry::{LogicalPoint, PhysicalPoint};
use crate::ids::SurfaceId;
use crate::intent::{Authority, AuthorityUnavailableReason, PointerButton, PointerId};
use crate::pointer_journal::{
    DesktopRouteFact, DesktopWorkAreaRoute, FiniteScrollVector, PhysicalScrollCoordinates,
    PointerAuthorityCheckpoint, PointerCaptureOwner, PointerEdge, PointerEdgeJournal,
    PointerEdgeKind, PointerEdgeLocation, PointerEventDeliveryOwner, PointerStateObservation,
    PointerStreamCancelReason, ScrollCancelReason, ScrollDeliveryEndpoint, ScrollDelta,
    ScrollDeviceId, ScrollEdge, ScrollModifiers, ScrollMomentum, ScrollPhase, ScrollSequenceToken,
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
    /// The host explicitly cancelled the stream.
    StreamCancelled,
    /// One lossless wheel or trackpad sample.
    Scrolled(NativeScrollEvent),
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

/// Complete native pointer state observed at provider enrollment.
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

/// Initial complete button and capture authority for a managed pointer provider.
///
/// This value is consumed by
/// [`DockspaceSession::enable_managed_native_host`](crate::runtime::DockspaceSession::enable_managed_native_host)
/// as part of provider enrollment. It cannot be appended after platform or
/// pointer records have started. Native-binding capture cannot appear in the
/// initial roster because binding capabilities are minted by that enrollment;
/// use [`Self::Unknown`] when a pre-existing capture cannot be expressed exactly.
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
/// Identities must increase across all scroll devices in the same pointer stream.
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
    /// The scroll device was removed.
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

/// One native pointer edge. Sequence and receiver receipts are minted by the
/// runtime, never supplied by the adapter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativePointerInput {
    pointer: NativePointerId,
    event: NativePointerEvent,
    location: NativeDesktopPointerLocation,
    delivery: NativePointerOwner,
    capture: NativePointerOwner,
}

impl NativePointerInput {
    /// Creates one complete event-time native input fact.
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
            location,
            delivery,
            capture,
        }
    }

    pub(super) const fn pointer(self) -> NativePointerId {
        self.pointer
    }

    pub(super) const fn event(self) -> NativePointerEvent {
        self.event
    }

    pub(super) const fn location(self) -> NativeDesktopPointerLocation {
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
}

impl NativeReceiverQuery {
    /// Returns the exact receiver lane or hover question.
    #[must_use]
    pub const fn purpose(&self) -> &NativeReceiverPurpose {
        &self.purpose
    }

    /// Returns the exact logical surface which must answer the question.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.presented_surface.surface()
    }

    /// Returns the exact presented surface capability for known-empty or blocked answers.
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
    /// A higher framework receiver blocked docking.
    Blocked(super::super::PresentedDockspaceSurface),
    /// Receiver authority was unavailable and the edge must fail closed.
    Unknown,
}

impl RuntimeNativeState {
    pub(super) fn record_initial_pointer_authority(
        &mut self,
        checkpoint: PointerAuthorityCheckpoint,
    ) {
        debug_assert_eq!(self.profile, NativeHostProfile::ManagedDesktop);
        let watermark = self.recorder.pointer_through();
        debug_assert_eq!(checkpoint.observed_through(), watermark);
        let journal = PointerEdgeJournal::new(watermark, watermark, Vec::new())
            .and_then(|journal| journal.with_authority_checkpoint(checkpoint))
            .expect("a prevalidated enrollment checkpoint forms a valid empty journal");
        self.recorder
            .record_pointer_segment(journal)
            .expect("a fresh managed recorder accepts its enrollment checkpoint");
    }

    pub(super) fn record_pointer(
        &mut self,
        input: NativePointerInput,
    ) -> Result<(), NativePlatformError> {
        if self.profile != NativeHostProfile::ManagedDesktop {
            return Err(NativePlatformError::HostProfileMismatch);
        }
        let previous = self.recorder.pointer_through();
        let sequence = previous
            .checked_next()
            .ok_or(NativePlatformError::PointerSequenceExhausted)?;
        let location = self.pointer_location(input.location())?;
        let delivery = self.delivery_owner(input.delivery())?;
        let capture = self.capture_owner(input.capture())?;
        let kind = match input.event() {
            NativePointerEvent::Moved => PointerEdgeKind::Moved,
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
            NativePointerEvent::StreamCancelled => PointerEdgeKind::StreamCancelled(
                PointerStreamCancelReason::ExplicitPlatformCancellation,
            ),
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
        );
        let journal = PointerEdgeJournal::new(previous, sequence, vec![edge])
            .map_err(|_| NativePlatformError::ProtocolInvariant)?;
        self.recorder
            .record_pointer_segment(journal)
            .map_err(|_| NativePlatformError::ProtocolInvariant)?;
        Ok(())
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
        location: NativeDesktopPointerLocation,
    ) -> Result<PointerEdgeLocation, NativePlatformError> {
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
        Ok(PointerEdgeLocation::Desktop { route })
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
        if binding.provider != self.provider()
            || self.bindings.get(&binding.surface()) != Some(&binding)
        {
            return Err(NativePlatformError::InvalidPointerFacts);
        }
        Ok(())
    }

    fn validate_work_area_binding(
        &self,
        work_area: NativeWorkAreaBinding,
    ) -> Result<(), NativePlatformError> {
        if work_area.provider != self.provider()
            || self.work_areas.get(&work_area.token) != Some(&work_area)
        {
            return Err(NativePlatformError::InvalidPointerFacts);
        }
        Ok(())
    }
}

pub(super) fn compile_initial_pointer_authority(
    roster: NativePointerRoster,
) -> Result<PointerAuthorityCheckpoint, NativePlatformError> {
    let watermark = crate::pointer_journal::PointerEdgeSequence::new(0);
    match roster {
        NativePointerRoster::Exact(states) => {
            let states = states
                .into_iter()
                .map(|state| {
                    let buttons = state
                        .pressed_buttons()
                        .iter()
                        .copied()
                        .map(NativePointerButton::into_core)
                        .collect();
                    let capture = match state.capture() {
                        NativePointerOwner::Foreign => {
                            Authority::Known(PointerCaptureOwner::Foreign)
                        }
                        NativePointerOwner::None => Authority::Known(PointerCaptureOwner::None),
                        NativePointerOwner::Unknown => {
                            Authority::Unknown(AuthorityUnavailableReason::NotReported)
                        }
                        // Native bindings are minted by enrollment and cannot
                        // pre-exist this atomic boundary.
                        NativePointerOwner::Native(_) => {
                            return Err(NativePlatformError::InvalidPointerFacts);
                        }
                    };
                    PointerStateObservation::new(state.pointer().into_core(), buttons, capture)
                        .map_err(|_| NativePlatformError::InvalidPointerFacts)
                })
                .collect::<Result<Vec<_>, _>>()?;
            PointerAuthorityCheckpoint::known(watermark, states)
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

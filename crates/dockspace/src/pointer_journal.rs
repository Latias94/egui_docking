//! Lossless, provider-ordered pointer input facts.
//!
//! A journal covers one complete provider watermark interval `(previous, through]`.
//! Every edge carries the facts observed at that edge, so reducers do not have to
//! reconstruct event order from an end-of-frame pointer snapshot.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use thiserror::Error;

use crate::geometry::{LogicalPoint, PhysicalPoint};
use crate::ids::{EngineAuthorityDomainId, SurfaceId};
use crate::intent::{Authority, AuthorityUnavailableReason, PointerButton, PointerId};
use crate::platform_provider::PlatformObservationLease;
use crate::presentation_observation::PresentationHostLease;
use crate::viewport::{CoordinateGeneration, ViewportBinding, WorkAreaGeneration, WorkAreaToken};

mod desktop_route;
mod ledger;
mod scroll;

#[cfg(test)]
mod tests;

/// Exact endpoint from which one surface-local provider reports input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SurfaceLocalPointerEndpoint {
    /// Renderer-local logical presentation of one surface.
    Logical(SurfaceId),
    /// Exact native-window incarnation presenting one surface.
    Native(ViewportBinding),
}

impl SurfaceLocalPointerEndpoint {
    /// Returns the logical surface presented by this endpoint.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        match self {
            Self::Logical(surface) => surface,
            Self::Native(binding) => binding.surface(),
        }
    }

    /// Returns the exact native binding, when this is a native endpoint.
    #[must_use]
    pub const fn native_binding(self) -> Option<ViewportBinding> {
        match self {
            Self::Logical(_) => None,
            Self::Native(binding) => Some(binding),
        }
    }
}

/// Frozen authority scope for a provider that can observe only one surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SurfaceLocalPointerScope {
    host: PresentationHostLease,
    endpoint: SurfaceLocalPointerEndpoint,
    coordinate_generation: Option<CoordinateGeneration>,
}

impl SurfaceLocalPointerScope {
    /// Describes one surface-local provider scope.
    ///
    /// The endpoint is the sole surface authority, so endpoint/surface mismatch
    /// is unrepresentable. The owning ledger validates host and native-binding
    /// authority domains before minting a provider lease.
    #[must_use]
    pub const fn new(host: PresentationHostLease, endpoint: SurfaceLocalPointerEndpoint) -> Self {
        Self {
            host,
            endpoint,
            coordinate_generation: None,
        }
    }

    /// Describes a native surface-local provider frozen to one coordinate
    /// authority generation.
    #[must_use]
    pub const fn new_native(
        host: PresentationHostLease,
        binding: ViewportBinding,
        coordinate_generation: CoordinateGeneration,
    ) -> Self {
        Self {
            host,
            endpoint: SurfaceLocalPointerEndpoint::Native(binding),
            coordinate_generation: Some(coordinate_generation),
        }
    }

    /// Returns the presentation host which owns this local input lane.
    #[must_use]
    pub const fn host(self) -> PresentationHostLease {
        self.host
    }

    /// Returns the sole logical surface visible to this input lane.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.endpoint.surface()
    }

    /// Returns the exact presentation endpoint visible to this input lane.
    #[must_use]
    pub const fn endpoint(self) -> SurfaceLocalPointerEndpoint {
        self.endpoint
    }

    /// Returns the frozen native coordinate generation, when present.
    #[must_use]
    pub const fn coordinate_generation(self) -> Option<CoordinateGeneration> {
        self.coordinate_generation
    }
}

/// Frozen observation lane of one pointer provider incarnation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PointerProviderScope {
    /// One platform aggregator with a total order across desktop viewports.
    DesktopGlobal,
    /// One presentation host endpoint with no global hover or outside facts.
    SurfaceLocal(SurfaceLocalPointerScope),
}

impl PointerProviderScope {
    /// Returns the frozen surface-local scope, when present.
    #[must_use]
    pub const fn surface_local(self) -> Option<SurfaceLocalPointerScope> {
        match self {
            Self::DesktopGlobal => None,
            Self::SurfaceLocal(scope) => Some(scope),
        }
    }
}

/// Exact desktop-to-surface coordinate route reported for one native docking
/// viewport.
///
/// The native runtime may report this value, but it does not become core
/// authority merely by being constructed. The reducer must validate the exact
/// binding, coordinate generation, and physical-to-logical conversion against
/// the current registry, then bind it again to the presented output that owns
/// the receiver receipt. Keeping all four facts together prevents a token-only
/// window lookup from accepting an old incarnation after the platform reuses a
/// native token.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DesktopDockRoute {
    binding: ViewportBinding,
    coordinate_generation: CoordinateGeneration,
    desktop_position: PhysicalPoint,
    surface_position: LogicalPoint,
}

/// Authoritative classification of the native window under a desktop pointer.
///
/// `None` is an explicit provider fact that no native window was under the
/// physical pointer. It remains distinct from `Foreign` and `Unknown`: only a
/// native runtime with complete global hover inventory may report it, and core
/// must never infer it from rectangle overlap.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DesktopHoveredWindow {
    /// One exact docking viewport binding is under the pointer.
    ///
    /// The independent desktop-to-local coordinate route may still be
    /// unavailable. A known docking hover alone never authorizes a receiver
    /// hit or drop target.
    Dock(ViewportBinding),
    /// A foreign native window is under the pointer.
    Foreign,
    /// The provider authoritatively observed no native window under the pointer.
    None,
}

/// Exact platform-work-area selection observed with one outside-all edge.
///
/// The token alone is insufficient because native providers and monitor
/// rosters can be replaced independently. Core accepts this selection only
/// while both the platform-provider lease and work-area generation remain
/// current.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopWorkAreaRoute {
    provider: PlatformObservationLease,
    generation: WorkAreaGeneration,
    token: WorkAreaToken,
}

/// Event-time desktop pointer route facts for one [`PointerEdge`].
///
/// This type makes `Dock`, `Foreign`, explicit no-window, and unavailable
/// hover distinct. The public constructors admit provider facts only; the
/// validator describes whether current facts corroborate them. The engine
/// remains the sole authority consumer: no public reducer entry accepts a
/// validation result in place of a journal fact.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DesktopRouteFact {
    position: Authority<PhysicalPoint>,
    hovered: Authority<DesktopHoveredWindow>,
    dock_route: Option<Authority<DesktopDockRoute>>,
    derive_dock_route_in_core: bool,
    work_area: Option<Authority<DesktopWorkAreaRoute>>,
}

/// Core-validated result of one desktop route fact.
///
/// This is a diagnostic validation result. It is not accepted by any input
/// reducer, so constructing one cannot grant routing authority.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DesktopRouteValidation {
    /// Current registry facts prove the exact route classification.
    Known(ValidatedDesktopRoute),
    /// The provider fact could not be made authoritative. Consumers must clear
    /// previews and avoid delivery/drop commits rather than substitute a
    /// geometry-derived target.
    Unavailable(DesktopRouteUnavailable),
}

/// One desktop route classification after registry validation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ValidatedDesktopRoute {
    /// A current route into one exact docking viewport.
    Dock(DesktopDockRoute),
    /// A foreign window blocks docking at this event-time position.
    Foreign {
        /// Physical position remains diagnostic only when unavailable.
        position: Authority<PhysicalPoint>,
    },
    /// The provider explicitly observed no native window under the pointer.
    NoWindow {
        /// Physical position required for any later native placement.
        position: Authority<PhysicalPoint>,
        /// Explicit monitor work area selected by the native provider.
        work_area: Authority<DesktopWorkAreaRoute>,
    },
}

/// Typed reason why a desktop route fact cannot authorize interaction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DesktopRouteUnavailable {
    /// The native runtime did not know the hovered-window fact.
    HoverUnknown {
        /// The independently known or unavailable physical position.
        position: Authority<PhysicalPoint>,
        /// Reason the hover fact was unavailable.
        reason: crate::intent::AuthorityUnavailableReason,
    },
    /// The native viewport under the pointer was known, but its exact
    /// physical-to-logical coordinate route was unavailable.
    CoordinateRouteUnknown {
        /// Exact native binding under the pointer.
        binding: ViewportBinding,
        /// The independently known or unavailable physical position.
        position: Authority<PhysicalPoint>,
        /// Reason the local route could not be reported.
        reason: crate::intent::AuthorityUnavailableReason,
    },
    /// The hovered binding was exact, but event-time desktop position was unavailable.
    DesktopPositionUnknown {
        /// Exact native binding under the pointer.
        binding: ViewportBinding,
        /// Reason the desktop position could not be observed.
        reason: crate::intent::AuthorityUnavailableReason,
    },
    /// The route binding belongs to another engine authority domain.
    ForeignAuthorityDomain {
        /// Binding reported by the native provider.
        binding: ViewportBinding,
        /// Domain owned by this engine.
        expected: EngineAuthorityDomainId,
        /// Domain encoded by the reported binding.
        submitted: EngineAuthorityDomainId,
    },
    /// No current logical surface owns the reported binding's surface id.
    BindingMissing {
        /// Binding reported by the native provider.
        binding: ViewportBinding,
    },
    /// The surface exists but its exact binding, including incarnation, changed.
    StaleBinding {
        /// Binding reported by the delayed provider edge.
        observed: ViewportBinding,
        /// Exact current binding for the logical surface.
        current: ViewportBinding,
    },
    /// The binding is not currently admitted, visible, and routeable.
    TargetNotRouteable {
        /// Binding reported by the native provider.
        binding: ViewportBinding,
    },
    /// The route named an old coordinate authority generation.
    CoordinateGenerationMismatch {
        /// Binding reported by the native provider.
        binding: ViewportBinding,
        /// Coordinate generation carried by the route.
        submitted: CoordinateGeneration,
        /// Current registry coordinate generation.
        current: CoordinateGeneration,
    },
    /// The route referenced a binding whose coordinates are not authoritative.
    CoordinatesUnavailable {
        /// Binding reported by the native provider.
        binding: ViewportBinding,
    },
    /// The core could not represent the exact physical-to-logical conversion.
    CoordinateConversionUnavailable {
        /// Binding reported by the native provider.
        binding: ViewportBinding,
    },
    /// The adapter-provided logical point differs from the sole exact core
    /// conversion for the submitted physical point and target scale.
    CoordinatePointMismatch {
        /// Binding reported by the native provider.
        binding: ViewportBinding,
        /// Physical point supplied by the native provider.
        physical: PhysicalPoint,
        /// Core-computed logical point.
        expected: LogicalPoint,
        /// Adapter-submitted logical point.
        submitted: LogicalPoint,
    },
}

/// Why an otherwise registry-validated desktop route cannot authorize one
/// exact presented output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DesktopRoutePresentationError {
    /// The output's logical surface differs from the route binding's surface.
    #[error("desktop route surface {route:?} differs from presentation surface {authority:?}")]
    SurfaceMismatch {
        route: SurfaceId,
        authority: SurfaceId,
    },
    /// The output did not originate from the exact native binding incarnation.
    #[error("desktop route binding {route:?} differs from presentation binding {authority:?}")]
    BindingMismatch {
        /// Exact binding carried by the route.
        route: ViewportBinding,
        /// Native binding, if any, retained by the presentation authority.
        authority: Option<ViewportBinding>,
    },
    /// The output was observed with a different coordinate authority generation.
    #[error(
        "desktop route coordinate generation {route:?} differs from presentation generation {authority:?} for {binding:?}"
    )]
    CoordinateGenerationMismatch {
        /// Exact route binding.
        binding: ViewportBinding,
        /// Generation carried by the route.
        route: CoordinateGeneration,
        /// Generation retained by the output authority.
        authority: CoordinateGeneration,
    },
}

/// Event-time position and hover facts in one provider's frozen coordinate lane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointerEdgeLocation {
    /// Desktop-global physical route facts.
    ///
    /// A known docking hover includes a binding-incarnation and local point;
    /// the core validates both before it can request or accept a receiver hit.
    Desktop { route: DesktopRouteFact },
    /// Position in the sole surface-local endpoint's logical coordinates.
    ///
    /// This lane deliberately cannot represent global hover or outside-all
    /// semantics.
    SurfaceLocal {
        /// Event-time position in renderer-independent logical coordinates.
        position: Authority<LogicalPoint>,
    },
}

/// Coordinate lane carried by one pointer edge.
///
/// This is exposed in ledger diagnostics so a provider can distinguish a
/// desktop/global journal from a surface-local journal scope mismatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerEdgeLocationLane {
    Desktop,
    SurfaceLocal,
}

impl PointerEdgeLocation {
    const fn lane(self) -> PointerEdgeLocationLane {
        match self {
            Self::Desktop { .. } => PointerEdgeLocationLane::Desktop,
            Self::SurfaceLocal { .. } => PointerEdgeLocationLane::SurfaceLocal,
        }
    }

    /// Returns the complete desktop route fact for a desktop-global edge.
    #[must_use]
    pub const fn desktop_route(self) -> Option<DesktopRouteFact> {
        match self {
            Self::Desktop { route } => Some(route),
            Self::SurfaceLocal { .. } => None,
        }
    }

    /// Returns the logical position carried by a surface-local edge.
    #[must_use]
    pub const fn surface_local_position(self) -> Option<Authority<LogicalPoint>> {
        match self {
            Self::Desktop { .. } => None,
            Self::SurfaceLocal { position } => Some(position),
        }
    }
}

/// Provider-owned, strictly monotonic sequence of pointer edges in one scope.
///
/// This is not a per-device counter. A desktop-global provider assigns a total
/// order across every pointer and viewport it reports; a surface-local provider
/// orders every pointer delivered through its one frozen endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct PointerEdgeSequence(u64);

impl PointerEdgeSequence {
    /// Creates a sequence from its provider-owned protocol representation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the provider-owned protocol representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next sequence without permitting integer wrapping.
    #[must_use]
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

impl fmt::Display for PointerEdgeSequence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Authoritative owner of pointer capture at one input edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerCaptureOwner {
    /// The endpoint frozen into this surface-local provider lease owns capture.
    ///
    /// Desktop-global providers cannot report this value because they do not
    /// have one implicit endpoint. A surface-local provider reports this value
    /// even when its endpoint is backed by a native viewport binding; the exact
    /// binding remains part of the provider lease instead of being repeated in
    /// every edge.
    ProviderEndpoint,
    /// One exact native viewport binding owns capture.
    ///
    /// This is reserved for desktop-global providers. The complete binding,
    /// including its window incarnation, keeps a delayed observation of an old
    /// window distinct from a successor which reused the same platform token;
    /// downstream inventory validation must still require the exact binding.
    Native(ViewportBinding),
    /// A receiver outside this docking engine owns capture.
    Foreign,
    /// No receiver owns capture.
    None,
}

/// Exact native or provider endpoint which delivered one pointer edge.
///
/// Delivery is an edge-local fact. It neither implies that the endpoint owns
/// persistent pointer capture nor identifies the window under the pointer.
/// Keeping those three authorities independent lets a backend report events
/// delivered by a frozen source window while global capture is unavailable and
/// hover routes through another window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerEventDeliveryOwner {
    /// The immutable endpoint of a surface-local provider delivered the edge.
    ProviderEndpoint,
    /// One exact native viewport incarnation delivered the edge.
    Native(ViewportBinding),
    /// A receiver outside this docking engine delivered the edge.
    Foreign,
    /// The edge had no native-window delivery endpoint.
    None,
}

/// Scope-relative proof of whether any pointer button is currently pressed.
///
/// A provider begins with [`Self::Unknown`]. Individual press edges can prove
/// [`Self::KnownDown`], but only a complete authority checkpoint can establish
/// [`Self::KnownAllReleased`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnyButtonDownAuthority {
    /// At least one exact pointer/button pair is known to remain pressed.
    KnownDown,
    /// A complete provider roster proves that every pointer button is released.
    KnownAllReleased,
    /// The provider has not proved a complete all-released state.
    Unknown(crate::intent::AuthorityUnavailableReason),
}

/// Complete state of one pointer at an authority checkpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointerStateObservation {
    pointer: PointerId,
    pressed_buttons: Vec<PointerButton>,
    capture_owner: Authority<PointerCaptureOwner>,
}

impl PointerStateObservation {
    /// Creates one canonical pointer-state entry.
    ///
    /// # Errors
    ///
    /// Returns an error when the pressed-button roster contains a duplicate.
    pub fn new(
        pointer: PointerId,
        mut pressed_buttons: Vec<PointerButton>,
        capture_owner: Authority<PointerCaptureOwner>,
    ) -> Result<Self, PointerAuthorityCheckpointError> {
        pressed_buttons.sort_unstable();
        if let Some(button) = pressed_buttons
            .windows(2)
            .find_map(|pair| (pair[0] == pair[1]).then_some(pair[0]))
        {
            return Err(PointerAuthorityCheckpointError::DuplicateButton { pointer, button });
        }
        Ok(Self {
            pointer,
            pressed_buttons,
            capture_owner,
        })
    }

    /// Returns the stable provider pointer identity.
    #[must_use]
    pub const fn pointer(&self) -> PointerId {
        self.pointer
    }

    /// Returns the canonical pressed-button roster.
    #[must_use]
    pub fn pressed_buttons(&self) -> &[PointerButton] {
        &self.pressed_buttons
    }

    /// Returns the capture authority observed for this pointer.
    #[must_use]
    pub const fn capture_owner(&self) -> Authority<PointerCaptureOwner> {
        self.capture_owner
    }
}

/// Complete pointer authority observed immediately after one provider watermark.
///
/// The checkpoint is a baseline for the following journal interval. A known
/// empty roster is the sole representation which proves that all pointer
/// buttons are released when no pointers are active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointerAuthorityCheckpoint {
    observed_through: PointerEdgeSequence,
    pointers: Authority<Vec<PointerStateObservation>>,
}

impl PointerAuthorityCheckpoint {
    /// Creates a canonical complete pointer roster at one committed watermark.
    ///
    /// # Errors
    ///
    /// Returns an error when a pointer identity occurs more than once.
    pub fn known(
        observed_through: PointerEdgeSequence,
        mut pointers: Vec<PointerStateObservation>,
    ) -> Result<Self, PointerAuthorityCheckpointError> {
        pointers.sort_unstable_by_key(PointerStateObservation::pointer);
        if let Some(pointer) = pointers
            .windows(2)
            .find_map(|pair| (pair[0].pointer == pair[1].pointer).then_some(pair[0].pointer))
        {
            return Err(PointerAuthorityCheckpointError::DuplicatePointer { pointer });
        }
        Ok(Self {
            observed_through,
            pointers: Authority::Known(pointers),
        })
    }

    /// Records that complete pointer authority is unavailable at one watermark.
    #[must_use]
    pub const fn unknown(
        observed_through: PointerEdgeSequence,
        reason: crate::intent::AuthorityUnavailableReason,
    ) -> Self {
        Self {
            observed_through,
            pointers: Authority::Unknown(reason),
        }
    }

    /// Returns the exact provider watermark covered by this checkpoint.
    #[must_use]
    pub const fn observed_through(&self) -> PointerEdgeSequence {
        self.observed_through
    }

    /// Returns the complete roster or its explicit unavailable reason.
    #[must_use]
    pub const fn pointers(&self) -> &Authority<Vec<PointerStateObservation>> {
        &self.pointers
    }
}

/// Structural error while constructing a pointer authority checkpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PointerAuthorityCheckpointError {
    /// One pointer listed the same pressed button more than once.
    #[error("pointer {pointer:?} repeats pressed button {button:?}")]
    DuplicateButton {
        /// Pointer carrying the duplicate button.
        pointer: PointerId,
        /// Repeated button.
        button: PointerButton,
    },
    /// A complete checkpoint listed one pointer identity more than once.
    #[error("pointer authority checkpoint repeats pointer {pointer:?}")]
    DuplicatePointer {
        /// Repeated provider pointer identity.
        pointer: PointerId,
    },
}

/// Typed reason why a provider ended one pointer stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerStreamCancelReason {
    /// The physical or virtual input device was removed.
    DeviceRemoved,
    /// The platform explicitly cancelled this stream.
    ExplicitPlatformCancellation,
    /// The exact surface or native endpoint which owned this stream retired.
    BindingRetired,
}

/// Provider-owned identity of one physical or virtual scroll device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct ScrollDeviceId(u64);

/// Provider-owned monotonic identity of one explicitly phaseful smooth-scroll sequence.
///
/// Tokens must increase strictly within one provider, pointer stream, and
/// scroll-device tuple. A provider which resets or exhausts this counter must
/// replace its input lease before publishing another smooth sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct ScrollSequenceToken(u64);

/// Finite two-axis content movement retained without float equality ambiguity.
///
/// Negative zero is canonicalized to positive zero. NaN and infinity are
/// rejected before an edge can enter a journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiniteScrollVector {
    x_bits: u64,
    y_bits: u64,
}

/// Exact coordinate authority used to convert one physical-pixel scroll sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhysicalScrollCoordinates {
    binding: ViewportBinding,
    coordinate_generation: CoordinateGeneration,
}

/// Unit retained for one raw scroll sample until its exact receiver is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollDelta {
    /// Native physical pixels with explicit known-or-unknown conversion authority.
    PhysicalPixels {
        /// Raw content movement.
        delta: FiniteScrollVector,
        /// Exact binding and coordinate generation, or why they were unavailable.
        coordinates: Authority<PhysicalScrollCoordinates>,
    },
    /// Renderer-independent logical points.
    LogicalPoints(FiniteScrollVector),
    /// Provider line units, converted only after receiver selection.
    Lines(FiniteScrollVector),
    /// Provider page units, converted only after receiver selection.
    Pages(FiniteScrollVector),
}

/// Native phase of one scroll edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollPhase {
    /// One independent wheel step which owns no persistent session.
    Discrete,
    /// Start one provider-defined smooth sequence.
    Begin,
    /// Continue one existing smooth sequence.
    Update,
    /// Apply an optional final sample and terminate the sequence.
    End,
    /// Terminate without applying a sample.
    Cancel(ScrollCancelReason),
}

/// Explicit provider reason for cancelling a smooth scroll sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollCancelReason {
    /// The platform explicitly cancelled the gesture.
    PlatformCancelled,
    /// The delivery binding retired while the physical sequence may still emit a terminal tail.
    BindingRetired,
    /// The scroll device was removed.
    DeviceRemoved,
    /// The provider reset its sequence namespace.
    ProviderReset,
}

/// Whether a sample came directly from input or platform momentum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollMomentum {
    /// Direct user-controlled movement.
    Direct,
    /// Platform-generated momentum within the same sequence.
    Momentum,
}

/// Event-time keyboard modifiers retained with one scroll sample.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ScrollModifiers {
    shift: bool,
    control: bool,
    alt: bool,
    command: bool,
}

/// Exact presentation endpoint which physically received one scroll edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScrollDeliveryEndpoint {
    host: PresentationHostLease,
    surface: SurfaceId,
    binding: Option<ViewportBinding>,
    coordinate_generation: CoordinateGeneration,
}

/// Lossless raw scroll sample carried by one pointer edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollEdge {
    device: ScrollDeviceId,
    sequence: Option<ScrollSequenceToken>,
    phase: ScrollPhase,
    delta: Option<ScrollDelta>,
    momentum: Authority<ScrollMomentum>,
    modifiers: Authority<ScrollModifiers>,
    delivery: Authority<ScrollDeliveryEndpoint>,
}

/// Structural failure while constructing or validating a scroll edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ScrollEdgeError {
    /// One delta component was NaN or infinite.
    #[error("scroll delta component {axis} must be finite")]
    NonFiniteDelta {
        /// Invalid vector axis.
        axis: &'static str,
    },
    /// Phase, sequence token, and optional delta violate the scroll schema.
    #[error("scroll phase {phase:?} has an invalid sequence-token or delta shape")]
    InvalidPhaseShape {
        /// Rejected native phase.
        phase: ScrollPhase,
    },
    /// A native delivery binding named another logical surface.
    #[error(
        "scroll delivery surface {surface} differs from native binding surface {binding_surface}"
    )]
    DeliverySurfaceMismatch {
        /// Declared logical delivery surface.
        surface: SurfaceId,
        /// Surface frozen into the native binding.
        binding_surface: SurfaceId,
    },
    /// Delivery host and binding belong to different engine authority domains.
    #[error("scroll delivery host and native binding belong to different authority domains")]
    DeliveryAuthorityDomainMismatch,
    /// Physical pixel scale authority did not match the delivery endpoint.
    #[error("physical scroll delta binding or coordinate generation differs from delivery")]
    PhysicalDeltaEndpointMismatch,
}

/// One ordered change in a pointer stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerEdgeKind {
    /// The pointer moved.
    Moved,
    /// One button became pressed.
    ButtonPressed(PointerButton),
    /// One button became released while the provider-local pointer remains live.
    ButtonReleased(PointerButton),
    /// A touch or pen contact released its button and ended its pointer identity.
    ContactEnded(PointerButton),
    /// A buttonless pointer identity ended normally.
    ///
    /// This is distinct from [`Self::ContactEnded`], which must release one
    /// button, and [`Self::StreamCancelled`], which represents abnormal
    /// termination.
    StreamEnded,
    /// The provider observed a capture transition.
    ///
    /// The result exists only in [`PointerEdge::capture_owner`]. A known
    /// [`PointerCaptureOwner::None`] or [`PointerCaptureOwner::Foreign`] may
    /// prove capture loss; [`Authority::Unknown`] preserves the session while
    /// preventing actions that require capture authority.
    CaptureChanged,
    /// The provider explicitly terminated the pointer stream for a typed
    /// lifecycle reason.
    ///
    /// An unavailable event-time fact remains [`Authority::Unknown`] and must
    /// not be converted into cancellation.
    StreamCancelled(PointerStreamCancelReason),
    /// One lossless wheel or trackpad sample.
    Scrolled(ScrollEdge),
}

/// One lossless pointer edge with the authority facts observed at event time.
#[derive(Debug, Clone, PartialEq)]
pub struct PointerEdge {
    sequence: PointerEdgeSequence,
    pointer: PointerId,
    kind: PointerEdgeKind,
    location: PointerEdgeLocation,
    delivery_owner: Authority<PointerEventDeliveryOwner>,
    capture_owner: Authority<PointerCaptureOwner>,
}

impl PointerEdge {
    /// Creates one pointer edge from provider-ordered event-time facts.
    #[must_use]
    pub const fn new(
        sequence: PointerEdgeSequence,
        pointer: PointerId,
        kind: PointerEdgeKind,
        location: PointerEdgeLocation,
        capture_owner: Authority<PointerCaptureOwner>,
    ) -> Self {
        let delivery_owner = match location {
            PointerEdgeLocation::SurfaceLocal { .. } => {
                Authority::Known(PointerEventDeliveryOwner::ProviderEndpoint)
            }
            PointerEdgeLocation::Desktop { .. } => {
                Authority::Unknown(AuthorityUnavailableReason::NotReported)
            }
        };
        Self::new_with_delivery(
            sequence,
            pointer,
            kind,
            location,
            delivery_owner,
            capture_owner,
        )
    }

    /// Creates one pointer edge with an explicit edge-local delivery fact.
    ///
    /// Desktop-global providers must use this constructor. [`Self::new`] keeps
    /// the exact endpoint implicit only for a surface-local provider, whose
    /// immutable lease makes that endpoint unambiguous.
    #[must_use]
    pub const fn new_with_delivery(
        sequence: PointerEdgeSequence,
        pointer: PointerId,
        kind: PointerEdgeKind,
        location: PointerEdgeLocation,
        delivery_owner: Authority<PointerEventDeliveryOwner>,
        capture_owner: Authority<PointerCaptureOwner>,
    ) -> Self {
        Self {
            sequence,
            pointer,
            kind,
            location,
            delivery_owner,
            capture_owner,
        }
    }

    /// Returns this edge's provider-owned sequence.
    #[must_use]
    pub const fn sequence(&self) -> PointerEdgeSequence {
        self.sequence
    }

    /// Returns the stable pointer identity affected by this edge.
    #[must_use]
    pub const fn pointer(&self) -> PointerId {
        self.pointer
    }

    /// Returns the ordered change represented by this edge.
    #[must_use]
    pub const fn kind(&self) -> PointerEdgeKind {
        self.kind
    }

    /// Returns whether accepting this edge must retire its pointer stream.
    #[must_use]
    pub const fn ends_stream(&self) -> bool {
        matches!(
            self.kind,
            PointerEdgeKind::ContactEnded(_)
                | PointerEdgeKind::StreamEnded
                | PointerEdgeKind::StreamCancelled(_)
        )
    }

    /// Returns the event-time location in this provider's frozen scope.
    #[must_use]
    pub const fn location(&self) -> PointerEdgeLocation {
        self.location
    }

    /// Returns the complete desktop route fact when this edge belongs to the
    /// desktop-global coordinate lane.
    #[must_use]
    pub const fn desktop_route(&self) -> Option<DesktopRouteFact> {
        self.location.desktop_route()
    }

    /// Returns the exact endpoint which delivered this edge.
    ///
    /// This fact is valid only for the edge itself. Consumers must not retain
    /// it as evidence of platform pointer capture or derive hover from it.
    #[must_use]
    pub const fn delivery_owner(&self) -> Authority<PointerEventDeliveryOwner> {
        self.delivery_owner
    }

    /// Returns the event-time pointer capture owner.
    #[must_use]
    pub const fn capture_owner(&self) -> Authority<PointerCaptureOwner> {
        self.capture_owner
    }
}

/// Complete provider watermark interval of ordered pointer edges.
///
/// The journal proves that its edges exactly and contiguously cover
/// `(previous, through]`. An empty journal is therefore valid exactly when the
/// two watermarks are equal.
#[derive(Debug, Clone, PartialEq)]
pub struct PointerEdgeJournal {
    previous: PointerEdgeSequence,
    through: PointerEdgeSequence,
    edges: Vec<PointerEdge>,
    authority_checkpoint: Option<PointerAuthorityCheckpoint>,
}

impl PointerEdgeJournal {
    /// Creates a complete journal for `(previous, through]`.
    ///
    /// # Errors
    ///
    /// Returns [`PointerJournalError`] when the watermark interval is reversed
    /// or the supplied edges do not exactly and contiguously cover it.
    pub fn new(
        previous: PointerEdgeSequence,
        through: PointerEdgeSequence,
        edges: Vec<PointerEdge>,
    ) -> Result<Self, PointerJournalError> {
        Self::validate_interval(previous, through, &edges)?;

        Ok(Self {
            previous,
            through,
            edges,
            authority_checkpoint: None,
        })
    }

    /// Attaches the complete authority baseline observed at this interval's
    /// exclusive lower watermark.
    ///
    /// # Errors
    ///
    /// Returns an error when the checkpoint belongs to another watermark or a
    /// checkpoint was already attached to this journal value.
    pub fn with_authority_checkpoint(
        mut self,
        checkpoint: PointerAuthorityCheckpoint,
    ) -> Result<Self, PointerJournalError> {
        if checkpoint.observed_through() != self.previous {
            return Err(PointerJournalError::AuthorityCheckpointWatermarkMismatch {
                previous: self.previous,
                observed_through: checkpoint.observed_through(),
            });
        }
        if self.authority_checkpoint.is_some() {
            return Err(PointerJournalError::AuthorityCheckpointAlreadyAttached {
                previous: self.previous,
            });
        }
        self.authority_checkpoint = Some(checkpoint);
        Ok(self)
    }

    fn validate(&self) -> Result<(), PointerJournalError> {
        Self::validate_interval(self.previous, self.through, &self.edges)
    }

    fn validate_interval(
        previous: PointerEdgeSequence,
        through: PointerEdgeSequence,
        edges: &[PointerEdge],
    ) -> Result<(), PointerJournalError> {
        if through < previous {
            return Err(PointerJournalError::WatermarkReversed { previous, through });
        }

        let mut cursor = previous;
        for edge in edges {
            let actual = edge.sequence();
            if let PointerEdgeKind::Scrolled(scroll) = edge.kind() {
                scroll
                    .validate()
                    .map_err(|source| PointerJournalError::InvalidScrollEdge {
                        sequence: actual,
                        source,
                    })?;
            }
            if actual <= cursor {
                return Err(PointerJournalError::SequenceNotIncreasing {
                    previous: cursor,
                    actual,
                });
            }
            if actual > through {
                return Err(PointerJournalError::SequenceBeyondThrough { actual, through });
            }

            let expected = cursor
                .checked_next()
                .ok_or(PointerJournalError::SequenceSpaceExhausted { after: cursor })?;
            if actual != expected {
                return Err(PointerJournalError::SequenceGap { expected, actual });
            }
            cursor = actual;
        }

        if cursor != through {
            return Err(PointerJournalError::RangeIncomplete {
                expected_through: through,
                actual_through: cursor,
            });
        }

        Ok(())
    }

    /// Returns the exclusive lower watermark of this complete interval.
    #[must_use]
    pub const fn previous(&self) -> PointerEdgeSequence {
        self.previous
    }

    /// Returns the inclusive upper watermark of this complete interval.
    #[must_use]
    pub const fn through(&self) -> PointerEdgeSequence {
        self.through
    }

    /// Returns the edges in provider order.
    #[must_use]
    pub fn edges(&self) -> &[PointerEdge] {
        &self.edges
    }

    /// Returns the optional complete authority baseline for this interval.
    #[must_use]
    pub const fn authority_checkpoint(&self) -> Option<&PointerAuthorityCheckpoint> {
        self.authority_checkpoint.as_ref()
    }

    /// Returns the number of edges in this interval.
    #[must_use]
    pub fn len(&self) -> usize {
        self.edges.len()
    }

    /// Returns whether this interval contains no edges.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }

    /// Consumes the journal and returns its provider-ordered edges.
    #[must_use]
    pub fn into_edges(self) -> Vec<PointerEdge> {
        self.edges
    }
}

/// Structural error while constructing a complete pointer journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PointerJournalError {
    /// One scroll edge violated the typed phase or coordinate schema.
    #[error("pointer edge {sequence} contains an invalid scroll sample: {source}")]
    InvalidScrollEdge {
        /// Provider sequence of the malformed edge.
        sequence: PointerEdgeSequence,
        /// Typed scroll schema failure.
        source: ScrollEdgeError,
    },
    /// A checkpoint did not describe the journal's exclusive lower watermark.
    #[error(
        "pointer authority checkpoint covers {observed_through}, but journal starts at {previous}"
    )]
    AuthorityCheckpointWatermarkMismatch {
        /// Exclusive lower watermark required by the journal.
        previous: PointerEdgeSequence,
        /// Watermark actually covered by the checkpoint.
        observed_through: PointerEdgeSequence,
    },
    /// A journal value may carry only one complete authority baseline.
    #[error("pointer journal at watermark {previous} already has an authority checkpoint")]
    AuthorityCheckpointAlreadyAttached {
        /// Journal predecessor watermark.
        previous: PointerEdgeSequence,
    },
    /// The inclusive upper watermark preceded the exclusive lower watermark.
    #[error("pointer journal watermark {through} precedes {previous}")]
    WatermarkReversed {
        /// Exclusive lower watermark.
        previous: PointerEdgeSequence,
        /// Invalid inclusive upper watermark.
        through: PointerEdgeSequence,
    },
    /// An edge repeated or moved behind the last accepted sequence.
    #[error("pointer edge sequence {actual} does not follow increasing sequence {previous}")]
    SequenceNotIncreasing {
        /// Last accepted sequence.
        previous: PointerEdgeSequence,
        /// Repeated or descending sequence.
        actual: PointerEdgeSequence,
    },
    /// An edge exceeded the journal's declared upper watermark.
    #[error("pointer edge sequence {actual} exceeds journal watermark {through}")]
    SequenceBeyondThrough {
        /// Out-of-range edge sequence.
        actual: PointerEdgeSequence,
        /// Inclusive upper watermark.
        through: PointerEdgeSequence,
    },
    /// The provider omitted at least one sequence inside the declared interval.
    #[error("pointer journal expected sequence {expected}, but received {actual}")]
    SequenceGap {
        /// Next contiguous sequence required by the interval.
        expected: PointerEdgeSequence,
        /// Sequence actually supplied.
        actual: PointerEdgeSequence,
    },
    /// The sequence domain could not advance without wrapping.
    #[error("pointer edge sequence space is exhausted after {after}")]
    SequenceSpaceExhausted {
        /// Last accepted sequence.
        after: PointerEdgeSequence,
    },
    /// The supplied edges ended before the declared upper watermark.
    #[error(
        "pointer journal ends at {actual_through}, before declared watermark {expected_through}"
    )]
    RangeIncomplete {
        /// Inclusive upper watermark declared by the provider.
        expected_through: PointerEdgeSequence,
        /// Last sequence actually supplied.
        actual_through: PointerEdgeSequence,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
struct PointerProviderIncarnation(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
struct PointerStreamIncarnation(u64);

/// Core-minted identity of one pointer provider incarnation and frozen scope.
///
/// The same live lease spans host-frame boundaries; it is not a single-use
/// capability. The private pointer-journal ledger admits exactly one live provider,
/// retires old incarnations, and retains the accepted watermark per lease so a
/// journal cannot be replayed. Private fields and a crate-private constructor
/// prevent adapters from minting or rebinding this authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PointerInputLease {
    authority_domain: EngineAuthorityDomainId,
    incarnation: PointerProviderIncarnation,
    scope: PointerProviderScope,
}

/// Affine producer ownership for one surface-local pointer provider.
///
/// The provider lease remains copyable transport identity, while this value is
/// the sole producer-lifetime capability. A specialized host frame retains an
/// internal commit guard which advances this producer's watermark only when
/// the matching core candidate publishes. The producer cannot be drained while
/// that frame remains uncommitted.
#[derive(Debug)]
#[must_use = "a surface-local pointer producer must be retained or drained"]
pub struct SurfaceLocalPointerProvider {
    lease: PointerInputLease,
    state: Arc<Mutex<SurfaceLocalPointerProducerState>>,
}

#[derive(Debug, PartialEq, Eq)]
struct SurfaceLocalPointerProducerState {
    committed_through: PointerEdgeSequence,
    next_frame_attempt: u64,
    in_flight: Option<SurfaceLocalPointerFrameAttempt>,
}

/// Weak core-side witness for one surface-local producer lifetime.
///
/// A live producer, an in-flight frame guard, or a drain receipt retains the
/// strong state. Once all three disappear, the core can prove that no adapter
/// can submit another edge for this lease and may reclaim the abandoned lane.
#[derive(Debug, Clone)]
pub(crate) struct SurfaceLocalPointerProducerMonitor {
    state: Weak<Mutex<SurfaceLocalPointerProducerState>>,
}

impl SurfaceLocalPointerProducerMonitor {
    pub(crate) fn is_abandoned(&self) -> bool {
        self.state.upgrade().is_none()
    }
}

impl PartialEq for SurfaceLocalPointerProducerMonitor {
    fn eq(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.state, &other.state)
    }
}

impl Eq for SurfaceLocalPointerProducerMonitor {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SurfaceLocalPointerFrameAttempt {
    id: u64,
    through: PointerEdgeSequence,
}

#[derive(Debug)]
pub(crate) struct SurfaceLocalPointerFrameCommit {
    lease: PointerInputLease,
    state: Arc<Mutex<SurfaceLocalPointerProducerState>>,
    attempt: SurfaceLocalPointerFrameAttempt,
    finalized: bool,
}

impl SurfaceLocalPointerProvider {
    pub(crate) fn new(lease: PointerInputLease, committed_through: PointerEdgeSequence) -> Self {
        Self::from_state(
            lease,
            Arc::new(Mutex::new(SurfaceLocalPointerProducerState {
                committed_through,
                next_frame_attempt: 0,
                in_flight: None,
            })),
        )
    }

    fn from_state(
        lease: PointerInputLease,
        state: Arc<Mutex<SurfaceLocalPointerProducerState>>,
    ) -> Self {
        Self { lease, state }
    }

    pub(crate) fn monitor(&self) -> SurfaceLocalPointerProducerMonitor {
        SurfaceLocalPointerProducerMonitor {
            state: Arc::downgrade(&self.state),
        }
    }

    /// Returns the exact core-minted provider lease used by pointer journals.
    #[must_use]
    pub const fn lease(&self) -> PointerInputLease {
        self.lease
    }

    /// Returns the final core-committed edge watermark known by this producer.
    #[must_use]
    pub fn committed_through(&self) -> PointerEdgeSequence {
        self.lock_state().committed_through
    }

    pub(crate) fn begin_frame_submission(
        &self,
        previous: PointerEdgeSequence,
        through: PointerEdgeSequence,
    ) -> Result<SurfaceLocalPointerFrameCommit, SurfaceLocalPointerProviderError> {
        let mut state = self.lock_state();
        if previous != state.committed_through {
            return Err(
                SurfaceLocalPointerProviderError::CommittedWatermarkMismatch {
                    lease: self.lease,
                    committed_through: state.committed_through,
                    submitted_previous: previous,
                },
            );
        }
        if let Some(in_flight) = state.in_flight {
            return Err(SurfaceLocalPointerProviderError::FrameSubmissionInFlight {
                lease: self.lease,
                attempt: in_flight.id,
            });
        }
        let id = state
            .next_frame_attempt
            .checked_add(1)
            .ok_or(SurfaceLocalPointerProviderError::FrameAttemptExhausted { lease: self.lease })?;
        let attempt = SurfaceLocalPointerFrameAttempt { id, through };
        state.next_frame_attempt = id;
        state.in_flight = Some(attempt);
        drop(state);
        Ok(SurfaceLocalPointerFrameCommit {
            lease: self.lease,
            state: Arc::clone(&self.state),
            attempt,
            finalized: false,
        })
    }

    /// Stops this producer and freezes its exact final watermark.
    ///
    /// # Errors
    ///
    /// Returns the still-owned provider when one uncommitted host frame retains
    /// its producer lane.
    pub fn drain(self) -> Result<SurfaceLocalPointerDrainReceipt, SurfaceLocalPointerDrainError> {
        let (committed_through, in_flight) = {
            let state = self.lock_state();
            (state.committed_through, state.in_flight)
        };
        if let Some(in_flight) = in_flight {
            return Err(SurfaceLocalPointerDrainError {
                lease: self.lease,
                attempt: in_flight.id,
                provider: self,
            });
        }
        let Self { lease, state } = self;
        Ok(SurfaceLocalPointerDrainReceipt {
            lease,
            committed_through,
            state,
            consumed: false,
        })
    }

    fn lock_state(&self) -> MutexGuard<'_, SurfaceLocalPointerProducerState> {
        lock_surface_local_pointer_state(&self.state)
    }
}

impl PartialEq for SurfaceLocalPointerProvider {
    fn eq(&self, other: &Self) -> bool {
        self.lease == other.lease && Arc::ptr_eq(&self.state, &other.state)
    }
}

impl Eq for SurfaceLocalPointerProvider {}

impl SurfaceLocalPointerFrameCommit {
    pub(crate) fn lease(&self) -> PointerInputLease {
        self.lease
    }

    pub(crate) fn through(&self) -> PointerEdgeSequence {
        self.attempt.through
    }

    pub(crate) fn extend(
        &mut self,
        provider: &SurfaceLocalPointerProvider,
        previous: PointerEdgeSequence,
        through: PointerEdgeSequence,
    ) -> Result<(), SurfaceLocalPointerProviderError> {
        if provider.lease != self.lease || !Arc::ptr_eq(&provider.state, &self.state) {
            return Err(SurfaceLocalPointerProviderError::FrameProviderMismatch {
                expected: self.lease,
                submitted: provider.lease,
            });
        }
        if previous != self.attempt.through {
            return Err(SurfaceLocalPointerProviderError::FrameWatermarkMismatch {
                lease: self.lease,
                expected_previous: self.attempt.through,
                submitted_previous: previous,
            });
        }
        let mut state = lock_surface_local_pointer_state(&self.state);
        if state.in_flight != Some(self.attempt) {
            return Err(SurfaceLocalPointerProviderError::FrameSubmissionLost {
                lease: self.lease,
                attempt: self.attempt.id,
            });
        }
        self.attempt.through = through;
        state.in_flight = Some(self.attempt);
        Ok(())
    }

    /// Publishes one already validated core candidate while hiding the producer
    /// watermark behind the same lock.
    ///
    /// The closure must perform only the infallible destination swap. Concurrent
    /// producer readers remain blocked until both the core state and watermark
    /// name the same committed prefix.
    pub(crate) fn publish_with<T>(
        &mut self,
        publish_core: impl FnOnce() -> T,
    ) -> Result<T, SurfaceLocalPointerProviderError> {
        let mut state = lock_surface_local_pointer_state(&self.state);
        if state.in_flight != Some(self.attempt) {
            return Err(SurfaceLocalPointerProviderError::FrameSubmissionLost {
                lease: self.lease,
                attempt: self.attempt.id,
            });
        }
        let published = publish_core();
        state.committed_through = self.attempt.through;
        state.in_flight = None;
        self.finalized = true;
        Ok(published)
    }
}

impl Drop for SurfaceLocalPointerFrameCommit {
    fn drop(&mut self) {
        if self.finalized {
            return;
        }
        let mut state = lock_surface_local_pointer_state(&self.state);
        if state.in_flight == Some(self.attempt) {
            state.in_flight = None;
        }
    }
}

fn lock_surface_local_pointer_state(
    state: &Mutex<SurfaceLocalPointerProducerState>,
) -> MutexGuard<'_, SurfaceLocalPointerProducerState> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Failed attempt to stop a producer while a host frame still owns its lane.
#[derive(Debug, Error)]
#[error(
    "surface-local pointer provider {lease:?} still has uncommitted host-frame attempt {attempt}"
)]
pub struct SurfaceLocalPointerDrainError {
    lease: PointerInputLease,
    attempt: u64,
    provider: SurfaceLocalPointerProvider,
}

impl SurfaceLocalPointerDrainError {
    /// Returns the exact provider which remains live and retryable.
    #[must_use]
    pub fn into_provider(self) -> SurfaceLocalPointerProvider {
        self.provider
    }
}

/// Affine proof that one surface-local pointer producer has stopped.
///
/// The core consumes this proof only after the exact lease and final watermark
/// match its live pointer ledger. Failed retirement leaves the proof retryable;
/// an adapter may restore the producer with [`Self::into_provider`].
#[derive(Debug)]
#[must_use = "a drained surface-local pointer producer must be retired or restored"]
pub struct SurfaceLocalPointerDrainReceipt {
    lease: PointerInputLease,
    committed_through: PointerEdgeSequence,
    state: Arc<Mutex<SurfaceLocalPointerProducerState>>,
    consumed: bool,
}

impl PartialEq for SurfaceLocalPointerDrainReceipt {
    fn eq(&self, other: &Self) -> bool {
        self.lease == other.lease
            && self.committed_through == other.committed_through
            && self.consumed == other.consumed
            && Arc::ptr_eq(&self.state, &other.state)
    }
}

impl Eq for SurfaceLocalPointerDrainReceipt {}

impl SurfaceLocalPointerDrainReceipt {
    /// Returns the exact provider incarnation whose producer stopped.
    #[must_use]
    pub const fn lease(&self) -> PointerInputLease {
        self.lease
    }

    /// Returns the final edge watermark observed before producer shutdown.
    #[must_use]
    pub const fn committed_through(&self) -> PointerEdgeSequence {
        self.committed_through
    }

    /// Returns whether core consumed this proof at a successful retirement boundary.
    #[must_use]
    pub const fn is_consumed(&self) -> bool {
        self.consumed
    }

    /// Restores producer ownership after a rejected retirement attempt.
    ///
    /// # Errors
    ///
    /// Returns an error after core has already consumed the proof.
    pub fn into_provider(
        self,
    ) -> Result<SurfaceLocalPointerProvider, SurfaceLocalPointerProviderError> {
        if self.consumed {
            return Err(SurfaceLocalPointerProviderError::DrainReceiptConsumed {
                lease: self.lease,
            });
        }
        Ok(SurfaceLocalPointerProvider::from_state(
            self.lease, self.state,
        ))
    }

    pub(crate) fn consume(&mut self) -> bool {
        if self.consumed {
            false
        } else {
            self.consumed = true;
            true
        }
    }
}

/// Result of atomically retiring one drained surface-local pointer producer.
///
/// Surface-local retirement cannot delegate platform effects or focus work to
/// the renderer. The core therefore exposes only its authority disposition and
/// the exact local invalidation requirements after cancelling local state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "surface-local retirement authority and repaint work must be handled"]
pub struct SurfaceLocalPointerRetirementOutcome {
    disposition: SurfaceLocalPointerRetirementDisposition,
    interaction_changed: bool,
    repaint_required: bool,
}

impl SurfaceLocalPointerRetirementOutcome {
    pub(crate) const fn retired_active(interaction_changed: bool) -> Self {
        Self {
            disposition: SurfaceLocalPointerRetirementDisposition::RetiredActive,
            interaction_changed,
            repaint_required: true,
        }
    }

    pub(crate) const fn compacted_previously_retired() -> Self {
        Self {
            disposition: SurfaceLocalPointerRetirementDisposition::CompactedPreviouslyRetired,
            interaction_changed: false,
            repaint_required: false,
        }
    }

    /// Returns whether this call retired live authority or only compacted an
    /// already retired provider tombstone.
    #[must_use]
    pub const fn disposition(self) -> SurfaceLocalPointerRetirementDisposition {
        self.disposition
    }

    /// Returns whether retirement changed the active interaction state.
    #[must_use]
    pub const fn interaction_changed(self) -> bool {
        self.interaction_changed
    }

    /// Returns whether the owning surface must rebuild its presentation.
    #[must_use]
    pub const fn repaint_required(self) -> bool {
        self.repaint_required
    }
}

/// Authority disposition published by surface-local producer settlement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceLocalPointerRetirementDisposition {
    /// The receipt retired the currently active provider authority.
    RetiredActive,
    /// Core had already retired the provider and this call only compacted its tombstone.
    CompactedPreviouslyRetired,
}

/// Failure in the affine surface-local pointer producer lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SurfaceLocalPointerProviderError {
    /// A new frame did not begin at the producer's committed watermark.
    #[error(
        "surface-local pointer provider {lease:?} is committed through {committed_through}, but the frame begins at {submitted_previous}"
    )]
    CommittedWatermarkMismatch {
        /// Exact producer whose frame interval was invalid.
        lease: PointerInputLease,
        /// Latest watermark published by the core for this producer.
        committed_through: PointerEdgeSequence,
        /// Frame interval lower bound supplied by the adapter.
        submitted_previous: PointerEdgeSequence,
    },
    /// Another unresolved host frame already owns this producer lane.
    #[error(
        "surface-local pointer provider {lease:?} already has host-frame attempt {attempt} in flight"
    )]
    FrameSubmissionInFlight {
        /// Exact producer already retained by another frame.
        lease: PointerInputLease,
        /// Private diagnostic identity of the unresolved frame attempt.
        attempt: u64,
    },
    /// The producer cannot mint another private frame-attempt identity.
    #[error("surface-local pointer provider {lease:?} exhausted its frame-attempt identity")]
    FrameAttemptExhausted {
        /// Exact producer whose private attempt identity exhausted.
        lease: PointerInputLease,
    },
    /// A frame attempted to extend a different producer.
    #[error(
        "surface-local pointer frame belongs to {expected:?}, but received producer {submitted:?}"
    )]
    FrameProviderMismatch {
        /// Producer retained by the existing frame guard.
        expected: PointerInputLease,
        /// Different producer supplied by the later segment.
        submitted: PointerInputLease,
    },
    /// A later frame segment did not continue the staged producer interval.
    #[error(
        "surface-local pointer provider {lease:?} frame continues from {submitted_previous}, expected {expected_previous}"
    )]
    FrameWatermarkMismatch {
        /// Exact producer whose segment interval was discontinuous.
        lease: PointerInputLease,
        /// Final watermark retained by the preceding segment.
        expected_previous: PointerEdgeSequence,
        /// Lower bound supplied by the later segment.
        submitted_previous: PointerEdgeSequence,
    },
    /// The producer no longer retains the exact staged frame attempt.
    #[error("surface-local pointer provider {lease:?} lost staged host-frame attempt {attempt}")]
    FrameSubmissionLost {
        /// Exact producer whose staged guard disappeared.
        lease: PointerInputLease,
        /// Private diagnostic identity of the missing frame attempt.
        attempt: u64,
    },
    /// A consumed drain proof cannot recreate its predecessor producer.
    #[error("surface-local pointer drain receipt for {lease:?} was already consumed")]
    DrainReceiptConsumed {
        /// Exact producer lease named by the consumed proof.
        lease: PointerInputLease,
    },
}

/// Opaque core-minted identity of one pointer stream in one provider lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PointerStreamId {
    lease: PointerInputLease,
    pointer: PointerId,
    incarnation: PointerStreamIncarnation,
}

/// Core-minted acknowledgement identity for one accepted pointer edge.
///
/// The exact provider lease participates in identity so neither another engine
/// nor a later provider incarnation can acknowledge an edge merely by reusing
/// its sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PointerEdgeTicket {
    lease: PointerInputLease,
    sequence: PointerEdgeSequence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JournalPointerAuthority {
    buttons_complete: bool,
    unavailable_reason: AuthorityUnavailableReason,
    pressed_buttons: BTreeSet<(PointerId, PointerButton)>,
    captures: BTreeMap<PointerId, Authority<PointerCaptureOwner>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "reserved for the independent pointer-ledger integration"
)]
struct ActivePointerProvider {
    lease: PointerInputLease,
    committed_through: PointerEdgeSequence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "reserved for the independent pointer-ledger integration"
)]
struct RetiredPointerInputLease {
    lease: PointerInputLease,
    committed_through: PointerEdgeSequence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfaceLocalPointerQuiescenceDisposition {
    RetiredActive,
    CompactedPreviouslyRetired,
}

/// Identity of one in-memory ledger instance.
///
/// A prepared journal keeps this allocation alive and `commit_prepared` uses
/// pointer identity rather than value equality. Two independently-created
/// engines may otherwise have the same authority domain, provider
/// incarnation, and watermark, which would permit a cross-ledger ABA replay.
#[derive(Debug)]
struct PointerJournalLedgerIdentity;

/// Monotonic mutation version for one ledger instance.
///
/// A successful empty interval deliberately advances this version even though
/// its watermark is unchanged. That makes a prepared empty journal single-use
/// and prevents it from being replayed indefinitely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct PointerJournalLedgerVersion(u64);

/// Validated journal candidate that has not yet advanced the ledger.
///
/// This is deliberately non-cloneable and owns the exact journal accepted by
/// `prepare_candidate`. The token is meaningful only for the one ledger
/// instance and mutation version that minted it.
#[derive(Debug)]
#[allow(
    dead_code,
    reason = "reserved for the independent pointer-ledger integration"
)]
pub(crate) struct PreparedPointerJournal {
    ledger_identity: Arc<PointerJournalLedgerIdentity>,
    ledger_version: PointerJournalLedgerVersion,
    lease: PointerInputLease,
    committed_through: PointerEdgeSequence,
    journal: PointerEdgeJournal,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(
    dead_code,
    reason = "reserved for the independent pointer-ledger integration"
)]
pub(crate) struct AcceptedPointerEdge {
    stream: PointerStreamId,
    ticket: PointerEdgeTicket,
    button_authority_after: AnyButtonDownAuthority,
    capture_authority_for_reduction: Authority<PointerCaptureOwner>,
    capture_authority_after: Authority<PointerCaptureOwner>,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(
    dead_code,
    reason = "reserved for the independent pointer-ledger integration"
)]
pub(crate) struct PointerJournalCommit {
    lease: PointerInputLease,
    journal: PointerEdgeJournal,
    accepted_edges: Vec<AcceptedPointerEdge>,
}

/// Core-owned authority ledger for the sole live pointer provider.
///
/// This is intentionally not an adapter facade. `DockEngine` integration must
/// own this ledger, give its active lease to the platform provider, and reduce
/// only journals returned in [`PointerJournalCommit`].
#[derive(Debug)]
#[allow(
    dead_code,
    reason = "reserved for the independent pointer-ledger integration"
)]
pub(crate) struct PointerJournalLedger {
    authority_domain: EngineAuthorityDomainId,
    identity: Arc<PointerJournalLedgerIdentity>,
    version: PointerJournalLedgerVersion,
    last_incarnation: PointerProviderIncarnation,
    last_stream_incarnation: PointerStreamIncarnation,
    active_streams: BTreeMap<PointerId, PointerStreamId>,
    authority: JournalPointerAuthority,
    authority_checkpoint_boundary: Option<PointerEdgeSequence>,
    last_checkpoint: Option<PointerAuthorityCheckpoint>,
    active: Option<ActivePointerProvider>,
    surface_local_producer: Option<(PointerInputLease, SurfaceLocalPointerProducerMonitor)>,
    retired_surface_local_producers:
        BTreeMap<PointerInputLease, SurfaceLocalPointerProducerMonitor>,
    retired: BTreeMap<PointerInputLease, RetiredPointerInputLease>,
    compacted_retired_through: PointerProviderIncarnation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[allow(
    dead_code,
    reason = "reserved for the independent pointer-ledger integration"
)]
pub enum PointerJournalLedgerError {
    #[error("pointer provider {active:?} is already active")]
    ProviderAlreadyActive { active: PointerInputLease },
    #[error("pointer provider incarnation counter is exhausted")]
    ProviderIncarnationExhausted,
    #[error("pointer stream incarnation counter is exhausted")]
    StreamIncarnationExhausted,
    #[error("pointer journal ledger mutation version is exhausted")]
    LedgerVersionExhausted,
    #[error(
        "surface-local host {host:?} belongs to authority domain {submitted:?}, expected {expected:?}"
    )]
    ForeignSurfaceLocalHost {
        host: PresentationHostLease,
        expected: EngineAuthorityDomainId,
        submitted: EngineAuthorityDomainId,
    },
    #[error(
        "surface-local native binding {binding:?} belongs to authority domain {submitted:?}, expected {expected:?}"
    )]
    ForeignSurfaceLocalBinding {
        binding: ViewportBinding,
        expected: EngineAuthorityDomainId,
        submitted: EngineAuthorityDomainId,
    },
    #[error("pointer lease belongs to authority domain {submitted:?}, expected {expected:?}")]
    ForeignLease {
        expected: EngineAuthorityDomainId,
        submitted: EngineAuthorityDomainId,
    },
    #[error("pointer input lease {lease:?} was never minted by this ledger")]
    UnknownLease { lease: PointerInputLease },
    #[error("pointer input lease {lease:?} is retired at committed watermark {committed_through}")]
    RetiredLease {
        lease: PointerInputLease,
        committed_through: PointerEdgeSequence,
    },
    #[error("pointer input lease {lease:?} is retired behind a quiesced provider frontier")]
    CompactedLease { lease: PointerInputLease },
    #[error("surface-local pointer quiescence receipt for {lease:?} was already consumed")]
    SurfaceLocalQuiescenceReceiptConsumed { lease: PointerInputLease },
    #[error("pointer lease {lease:?} is not a surface-local producer")]
    SurfaceLocalQuiescenceScopeMismatch { lease: PointerInputLease },
    #[error(
        "surface-local pointer quiescence for {lease:?} reports watermark {submitted}, but core committed through {committed_through}"
    )]
    SurfaceLocalQuiescenceWatermarkMismatch {
        lease: PointerInputLease,
        committed_through: PointerEdgeSequence,
        submitted: PointerEdgeSequence,
    },
    #[error("surface-local pointer producer for {lease:?} still has a live owner")]
    SurfaceLocalProducerNotAbandoned { lease: PointerInputLease },
    #[error(
        "pointer journal for {lease:?} replays previous watermark {submitted_previous}; committed watermark is {committed_through}"
    )]
    JournalReplay {
        lease: PointerInputLease,
        committed_through: PointerEdgeSequence,
        submitted_previous: PointerEdgeSequence,
    },
    #[error(
        "pointer journal for {lease:?} starts ahead at {submitted_previous}; committed watermark is {committed_through}"
    )]
    JournalAhead {
        lease: PointerInputLease,
        committed_through: PointerEdgeSequence,
        submitted_previous: PointerEdgeSequence,
    },
    #[error(
        "pointer provider {lease:?} submitted conflicting authority checkpoints at watermark {observed_through}"
    )]
    ConflictingAuthorityCheckpoint {
        lease: PointerInputLease,
        observed_through: PointerEdgeSequence,
    },
    #[error(
        "pointer provider {lease:?} submitted an authority checkpoint at {observed_through} after its enrollment baseline was closed"
    )]
    AuthorityCheckpointAfterPointerEdges {
        lease: PointerInputLease,
        observed_through: PointerEdgeSequence,
    },
    #[error(
        "pointer edge {sequence} presses button {button:?} which is already down for pointer {pointer:?}"
    )]
    ButtonAlreadyPressed {
        pointer: PointerId,
        button: PointerButton,
        sequence: PointerEdgeSequence,
    },
    #[error(
        "pointer edge {sequence} releases button {button:?} which is not down for pointer {pointer:?}"
    )]
    ButtonNotPressed {
        pointer: PointerId,
        button: PointerButton,
        sequence: PointerEdgeSequence,
    },
    #[error(
        "normal stream end at pointer edge {sequence} leaves a pressed button for pointer {pointer:?}"
    )]
    StreamEndLeavesButtonsPressed {
        pointer: PointerId,
        sequence: PointerEdgeSequence,
    },
    #[error(
        "pointer edge {sequence} location lane {submitted:?} does not match provider lease {lease:?}"
    )]
    LocationScopeMismatch {
        lease: PointerInputLease,
        sequence: PointerEdgeSequence,
        submitted: PointerEdgeLocationLane,
    },
    #[error(
        "pointer edge {sequence} capture owner {submitted:?} does not match provider lease {lease:?}"
    )]
    CaptureScopeMismatch {
        lease: PointerInputLease,
        sequence: PointerEdgeSequence,
        submitted: PointerCaptureOwner,
    },
    #[error(
        "pointer edge {sequence} delivery owner {submitted:?} does not match provider lease {lease:?}"
    )]
    DeliveryScopeMismatch {
        lease: PointerInputLease,
        sequence: PointerEdgeSequence,
        submitted: PointerEventDeliveryOwner,
    },
    #[error(
        "pointer edge {sequence} native delivery binding {binding:?} for {lease:?} belongs to authority domain {submitted:?}, expected {expected:?}"
    )]
    ForeignDeliveryBinding {
        lease: PointerInputLease,
        sequence: PointerEdgeSequence,
        binding: ViewportBinding,
        expected: EngineAuthorityDomainId,
        submitted: EngineAuthorityDomainId,
    },
    #[error(
        "pointer edge {sequence} native capture binding {binding:?} for {lease:?} belongs to authority domain {submitted:?}, expected {expected:?}"
    )]
    ForeignCaptureBinding {
        lease: PointerInputLease,
        sequence: PointerEdgeSequence,
        binding: ViewportBinding,
        expected: EngineAuthorityDomainId,
        submitted: EngineAuthorityDomainId,
    },
    #[error(
        "pointer edge {sequence} desktop route binding {binding:?} for {lease:?} belongs to authority domain {submitted:?}, expected {expected:?}"
    )]
    ForeignDesktopRouteBinding {
        lease: PointerInputLease,
        sequence: PointerEdgeSequence,
        binding: ViewportBinding,
        expected: EngineAuthorityDomainId,
        submitted: EngineAuthorityDomainId,
    },
    #[error(
        "scroll edge {sequence} delivery host {host:?} belongs to another engine for provider {lease:?}"
    )]
    ForeignScrollDeliveryHost {
        lease: PointerInputLease,
        sequence: PointerEdgeSequence,
        host: PresentationHostLease,
    },
    #[error(
        "scroll edge {sequence} delivery endpoint {endpoint:?} is outside provider scope {lease:?}"
    )]
    ScrollDeliveryScopeMismatch {
        lease: PointerInputLease,
        sequence: PointerEdgeSequence,
        endpoint: ScrollDeliveryEndpoint,
    },
    #[error(
        "desktop-global scroll edge {sequence} has no native binding in endpoint {endpoint:?} for {lease:?}"
    )]
    DesktopScrollDeliveryBindingMissing {
        lease: PointerInputLease,
        sequence: PointerEdgeSequence,
        endpoint: ScrollDeliveryEndpoint,
    },
    #[error(
        "scroll edge {sequence} endpoint {endpoint:?} contradicts delivery owner {owner:?} for {lease:?}"
    )]
    ScrollDeliveryOwnerMismatch {
        lease: PointerInputLease,
        sequence: PointerEdgeSequence,
        endpoint: ScrollDeliveryEndpoint,
        owner: PointerEventDeliveryOwner,
    },
    #[error("pointer journal failed structural validation: {0}")]
    InvalidJournal(PointerJournalError),
    #[error("prepared pointer journal belongs to another ledger instance")]
    ForeignPreparedJournal,
    #[error(
        "prepared pointer journal for {lease:?} is stale: version {prepared_version} at watermark {prepared_committed_through}, active version {active_version} at watermark {active_committed_through}"
    )]
    PreparedJournalStale {
        lease: PointerInputLease,
        prepared_version: u64,
        active_version: u64,
        prepared_committed_through: PointerEdgeSequence,
        active_committed_through: PointerEdgeSequence,
    },
}

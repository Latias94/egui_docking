//! Lossless, provider-ordered pointer input facts.
//!
//! A journal covers one complete provider watermark interval `(previous, through]`.
//! Every edge carries the facts observed at that edge, so reducers do not have to
//! reconstruct event order from an end-of-frame pointer snapshot.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use thiserror::Error;

use crate::backend_ingress::BackendIngressDrainReceipt;
use crate::geometry::{LogicalPoint, PhysicalPoint};
use crate::ids::{EngineAuthorityDomainId, SurfaceId};
use crate::intent::{Authority, AuthorityUnavailableReason, PointerButton, PointerId};
use crate::platform_provider::PlatformObservationLease;
use crate::presentation_observation::{PresentationHostLease, PresentedSurfaceAuthority};
use crate::viewport::{CoordinateGeneration, ViewportBinding, WorkAreaGeneration, WorkAreaToken};
use crate::viewport_registry::ViewportRegistry;

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
}

impl SurfaceLocalPointerScope {
    /// Describes one surface-local provider scope.
    ///
    /// The endpoint is the sole surface authority, so endpoint/surface mismatch
    /// is unrepresentable. The owning ledger validates host and native-binding
    /// authority domains before minting a provider lease.
    #[must_use]
    pub const fn new(host: PresentationHostLease, endpoint: SurfaceLocalPointerEndpoint) -> Self {
        Self { host, endpoint }
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

impl DesktopDockRoute {
    /// Describes one adapter-observed desktop route.
    ///
    /// This is an input fact, not an authority capability. Callers must submit
    /// it through a desktop-global pointer lease; core validation is required
    /// before it can authorize a receiver or drop target.
    #[must_use]
    pub const fn new(
        binding: ViewportBinding,
        coordinate_generation: CoordinateGeneration,
        desktop_position: PhysicalPoint,
        surface_position: LogicalPoint,
    ) -> Self {
        Self {
            binding,
            coordinate_generation,
            desktop_position,
            surface_position,
        }
    }

    /// Returns the exact native viewport incarnation under the pointer.
    #[must_use]
    pub const fn binding(self) -> ViewportBinding {
        self.binding
    }

    /// Returns the coordinate authority generation used for conversion.
    #[must_use]
    pub const fn coordinate_generation(self) -> CoordinateGeneration {
        self.coordinate_generation
    }

    /// Returns the event-time desktop physical position.
    #[must_use]
    pub const fn desktop_position(self) -> PhysicalPoint {
        self.desktop_position
    }

    /// Returns the event-time point in the target surface's logical space.
    #[must_use]
    pub const fn surface_position(self) -> LogicalPoint {
        self.surface_position
    }

    /// Validates that a final-presentation authority belongs to this exact
    /// native route. The output must retain the same binding incarnation and
    /// coordinate generation; a matching logical surface alone is insufficient.
    pub fn validate_presented_authority(
        self,
        authority: PresentedSurfaceAuthority,
    ) -> Result<(), DesktopRoutePresentationError> {
        if authority.surface() != self.binding.surface() {
            return Err(DesktopRoutePresentationError::SurfaceMismatch {
                route: self.binding.surface(),
                authority: authority.surface(),
            });
        }
        if authority.binding() != Some(self.binding) {
            return Err(DesktopRoutePresentationError::BindingMismatch {
                route: self.binding,
                authority: authority.binding(),
            });
        }
        if authority.coordinate_generation() != self.coordinate_generation {
            return Err(
                DesktopRoutePresentationError::CoordinateGenerationMismatch {
                    binding: self.binding,
                    route: self.coordinate_generation,
                    authority: authority.coordinate_generation(),
                },
            );
        }
        Ok(())
    }
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

impl DesktopWorkAreaRoute {
    /// Describes one explicit event-time work-area selection.
    #[must_use]
    pub const fn new(
        provider: PlatformObservationLease,
        generation: WorkAreaGeneration,
        token: WorkAreaToken,
    ) -> Self {
        Self {
            provider,
            generation,
            token,
        }
    }

    /// Returns the platform-provider incarnation which selected the work area.
    #[must_use]
    pub const fn provider(self) -> PlatformObservationLease {
        self.provider
    }

    /// Returns the exact core work-area generation observed by the provider.
    #[must_use]
    pub const fn generation(self) -> WorkAreaGeneration {
        self.generation
    }

    /// Returns the selected work-area token.
    #[must_use]
    pub const fn token(self) -> WorkAreaToken {
        self.token
    }
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

impl DesktopRouteFact {
    /// Reports one exact docking viewport route.
    #[must_use]
    pub const fn dock(route: DesktopDockRoute) -> Self {
        Self {
            position: Authority::Known(route.desktop_position()),
            hovered: Authority::Known(DesktopHoveredWindow::Dock(route.binding())),
            dock_route: Some(Authority::Known(route)),
            derive_dock_route_in_core: false,
            work_area: None,
        }
    }

    /// Reports an exact hovered docking binding and desktop position while
    /// delegating physical-to-logical conversion to the core reducer.
    ///
    /// This is the native backend path. The core validates the exact binding
    /// against the registry state current at this edge's reducer ordinal, then
    /// uses that same record's coordinate generation and transform. An adapter
    /// must not query the pre-frame engine and echo a potentially stale local
    /// point or generation into an ordered batch.
    #[must_use]
    pub const fn dock_from_desktop_position(
        binding: ViewportBinding,
        position: Authority<PhysicalPoint>,
    ) -> Self {
        Self {
            position,
            hovered: Authority::Known(DesktopHoveredWindow::Dock(binding)),
            dock_route: None,
            derive_dock_route_in_core: true,
            work_area: None,
        }
    }

    /// Reports a known docking viewport whose event-time coordinate conversion
    /// was unavailable.
    ///
    /// This preserves the native hover classification without letting a caller
    /// manufacture a logical point from it. Core validation produces a typed
    /// unavailable result, so the edge can still consume ordering while any
    /// receiver or drop operation fails closed.
    #[must_use]
    pub const fn dock_without_coordinate_route(
        binding: ViewportBinding,
        position: Authority<PhysicalPoint>,
        reason: crate::intent::AuthorityUnavailableReason,
    ) -> Self {
        Self {
            position,
            hovered: Authority::Known(DesktopHoveredWindow::Dock(binding)),
            dock_route: Some(Authority::Unknown(reason)),
            derive_dock_route_in_core: false,
            work_area: None,
        }
    }

    /// Reports a foreign window at a known or unavailable physical position.
    #[must_use]
    pub const fn foreign(position: Authority<PhysicalPoint>) -> Self {
        Self {
            position,
            hovered: Authority::Known(DesktopHoveredWindow::Foreign),
            dock_route: None,
            derive_dock_route_in_core: false,
            work_area: None,
        }
    }

    /// Reports that no native window was under a known or unavailable physical
    /// position. This is never synthesized from docking surface geometry.
    #[must_use]
    pub const fn no_window(
        position: Authority<PhysicalPoint>,
        work_area: Authority<DesktopWorkAreaRoute>,
    ) -> Self {
        Self {
            position,
            hovered: Authority::Known(DesktopHoveredWindow::None),
            dock_route: None,
            derive_dock_route_in_core: false,
            work_area: Some(work_area),
        }
    }

    /// Reports a desktop route whose hovered-window fact is unavailable.
    #[must_use]
    pub const fn unknown(
        position: Authority<PhysicalPoint>,
        reason: crate::intent::AuthorityUnavailableReason,
    ) -> Self {
        Self {
            position,
            hovered: Authority::Unknown(reason),
            dock_route: None,
            derive_dock_route_in_core: false,
            work_area: None,
        }
    }

    /// Returns the event-time desktop physical position.
    #[must_use]
    pub const fn position(self) -> Authority<PhysicalPoint> {
        self.position
    }

    /// Returns the event-time native hovered-window classification.
    #[must_use]
    pub const fn hovered(self) -> Authority<DesktopHoveredWindow> {
        self.hovered
    }

    /// Returns the route-conversion authority for a known dock hover.
    ///
    /// `None` means either that this fact did not identify a docking viewport
    /// or that it deliberately delegated conversion to core. Inspect
    /// [`Self::hovered`] to distinguish those cases. `Some(Unknown(_))` means
    /// the provider explicitly reported that conversion was unavailable.
    #[must_use]
    pub const fn dock_route_authority(self) -> Option<Authority<DesktopDockRoute>> {
        self.dock_route
    }

    /// Returns the exact dock route when both hover and coordinate facts were
    /// reported together.
    #[must_use]
    pub const fn dock_route(self) -> Option<DesktopDockRoute> {
        match self.dock_route {
            Some(Authority::Known(route)) => Some(route),
            Some(Authority::Unknown(_)) | None => None,
        }
    }

    /// Returns the event-time work-area authority for an explicit no-window route.
    #[must_use]
    pub const fn work_area(self) -> Option<Authority<DesktopWorkAreaRoute>> {
        self.work_area
    }

    /// Validates this provider fact against the current core-owned native
    /// viewport registry. A stale or contradictory route becomes a typed
    /// unavailable result rather than a guessed target.
    pub fn validate_against_registry(
        self,
        authority_domain: EngineAuthorityDomainId,
        registry: &ViewportRegistry,
    ) -> DesktopRouteValidation {
        let (binding, submitted_route) = match self.hovered {
            Authority::Known(DesktopHoveredWindow::Dock(binding)) => match self.dock_route {
                Some(Authority::Known(route)) => (binding, Some(route)),
                Some(Authority::Unknown(reason)) => {
                    return DesktopRouteValidation::Unavailable(
                        DesktopRouteUnavailable::CoordinateRouteUnknown {
                            binding,
                            position: self.position,
                            reason,
                        },
                    );
                }
                None if self.derive_dock_route_in_core => (binding, None),
                None => unreachable!("a known docking hover always carries coordinate authority"),
            },
            Authority::Known(DesktopHoveredWindow::Foreign) => {
                return DesktopRouteValidation::Known(ValidatedDesktopRoute::Foreign {
                    position: self.position,
                });
            }
            Authority::Known(DesktopHoveredWindow::None) => {
                return DesktopRouteValidation::Known(ValidatedDesktopRoute::NoWindow {
                    position: self.position,
                    work_area: self
                        .work_area
                        .unwrap_or(Authority::Unknown(AuthorityUnavailableReason::NotReported)),
                });
            }
            Authority::Unknown(reason) => {
                return DesktopRouteValidation::Unavailable(
                    DesktopRouteUnavailable::HoverUnknown {
                        position: self.position,
                        reason,
                    },
                );
            }
        };

        if binding.authority_domain() != authority_domain {
            return DesktopRouteValidation::Unavailable(
                DesktopRouteUnavailable::ForeignAuthorityDomain {
                    binding,
                    expected: authority_domain,
                    submitted: binding.authority_domain(),
                },
            );
        }
        let Some(record) = registry.record(binding.surface()) else {
            return DesktopRouteValidation::Unavailable(DesktopRouteUnavailable::BindingMissing {
                binding,
            });
        };
        if record.binding() != binding {
            return DesktopRouteValidation::Unavailable(DesktopRouteUnavailable::StaleBinding {
                observed: binding,
                current: record.binding(),
            });
        }
        if !record.is_routeable() {
            return DesktopRouteValidation::Unavailable(
                DesktopRouteUnavailable::TargetNotRouteable { binding },
            );
        }
        let Some(coordinates) = record.coordinates() else {
            return DesktopRouteValidation::Unavailable(
                DesktopRouteUnavailable::CoordinatesUnavailable { binding },
            );
        };
        if let Some(route) = submitted_route
            && record.coordinate_generation() != route.coordinate_generation
        {
            return DesktopRouteValidation::Unavailable(
                DesktopRouteUnavailable::CoordinateGenerationMismatch {
                    binding,
                    submitted: route.coordinate_generation,
                    current: record.coordinate_generation(),
                },
            );
        }
        let Authority::Known(physical) = self.position else {
            return DesktopRouteValidation::Unavailable(
                DesktopRouteUnavailable::DesktopPositionUnknown {
                    binding,
                    reason: match self.position {
                        Authority::Unknown(reason) => reason,
                        Authority::Known(_) => unreachable!("known position matched above"),
                    },
                },
            );
        };
        let expected = match coordinates.desktop_to_surface(physical) {
            Ok(point) => point,
            Err(_) => {
                return DesktopRouteValidation::Unavailable(
                    DesktopRouteUnavailable::CoordinateConversionUnavailable { binding },
                );
            }
        };
        let route = if let Some(route) = submitted_route {
            if expected != route.surface_position {
                return DesktopRouteValidation::Unavailable(
                    DesktopRouteUnavailable::CoordinatePointMismatch {
                        binding,
                        physical,
                        expected,
                        submitted: route.surface_position,
                    },
                );
            }
            route
        } else {
            DesktopDockRoute::new(binding, record.coordinate_generation(), physical, expected)
        };
        DesktopRouteValidation::Known(ValidatedDesktopRoute::Dock(route))
    }
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

impl DesktopRouteValidation {
    /// Returns the validated docking route, when this fact resolved to one
    /// exact current docking viewport.
    pub const fn dock_route(self) -> Option<DesktopDockRoute> {
        match self {
            Self::Known(ValidatedDesktopRoute::Dock(route)) => Some(route),
            Self::Known(
                ValidatedDesktopRoute::Foreign { .. } | ValidatedDesktopRoute::NoWindow { .. },
            )
            | Self::Unavailable(_) => None,
        }
    }
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
    /// The active input provider shut down permanently.
    ProviderShutdown,
    /// The platform explicitly cancelled this stream.
    ExplicitPlatformCancellation,
    /// The provider reset and must continue under a new input lease.
    ProviderReset,
}

/// Provider-owned identity of one physical or virtual scroll device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct ScrollDeviceId(u64);

impl ScrollDeviceId {
    /// Creates a device identity from its provider protocol representation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the provider protocol representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Provider-owned monotonic identity of one explicitly phaseful smooth-scroll sequence.
///
/// Tokens must increase strictly within one provider, pointer stream, and
/// scroll-device tuple. A provider which resets or exhausts this counter must
/// replace its input lease before publishing another smooth sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct ScrollSequenceToken(u64);

impl ScrollSequenceToken {
    /// Creates a sequence token from its provider protocol representation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the provider protocol representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Finite two-axis content movement retained without float equality ambiguity.
///
/// Negative zero is canonicalized to positive zero. NaN and infinity are
/// rejected before an edge can enter a journal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FiniteScrollVector {
    x_bits: u64,
    y_bits: u64,
}

impl FiniteScrollVector {
    /// Creates a finite content-movement vector.
    ///
    /// # Errors
    ///
    /// Returns [`ScrollEdgeError::NonFiniteDelta`] when either component is NaN
    /// or infinite.
    pub fn new(x: f64, y: f64) -> Result<Self, ScrollEdgeError> {
        if !x.is_finite() {
            return Err(ScrollEdgeError::NonFiniteDelta { axis: "x" });
        }
        if !y.is_finite() {
            return Err(ScrollEdgeError::NonFiniteDelta { axis: "y" });
        }
        Ok(Self {
            x_bits: canonical_scroll_component(x).to_bits(),
            y_bits: canonical_scroll_component(y).to_bits(),
        })
    }

    /// Returns horizontal content movement.
    #[must_use]
    pub const fn x(self) -> f64 {
        f64::from_bits(self.x_bits)
    }

    /// Returns vertical content movement.
    #[must_use]
    pub const fn y(self) -> f64 {
        f64::from_bits(self.y_bits)
    }
}

const fn canonical_scroll_component(value: f64) -> f64 {
    if value == 0.0 { 0.0 } else { value }
}

/// Unit retained for one raw scroll sample until its exact receiver is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollDelta {
    /// Native physical pixels under one exact binding and coordinate generation.
    PhysicalPixels {
        /// Raw content movement.
        delta: FiniteScrollVector,
        /// Native viewport incarnation whose scale converts the sample.
        binding: ViewportBinding,
        /// Exact coordinate generation observed with the sample.
        coordinate_generation: CoordinateGeneration,
    },
    /// Renderer-independent logical points.
    LogicalPoints(FiniteScrollVector),
    /// Provider line units, converted only after receiver selection.
    Lines(FiniteScrollVector),
    /// Provider page units, converted only after receiver selection.
    Pages(FiniteScrollVector),
}

impl ScrollDelta {
    /// Returns the raw two-axis content movement.
    #[must_use]
    pub const fn vector(self) -> FiniteScrollVector {
        match self {
            Self::PhysicalPixels { delta, .. }
            | Self::LogicalPoints(delta)
            | Self::Lines(delta)
            | Self::Pages(delta) => delta,
        }
    }
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

impl ScrollModifiers {
    /// Creates an exact modifier snapshot.
    #[must_use]
    pub const fn new(shift: bool, control: bool, alt: bool, command: bool) -> Self {
        Self {
            shift,
            control,
            alt,
            command,
        }
    }

    /// Returns whether Shift was pressed.
    #[must_use]
    pub const fn shift(self) -> bool {
        self.shift
    }

    /// Returns whether Control was pressed.
    #[must_use]
    pub const fn control(self) -> bool {
        self.control
    }

    /// Returns whether Alt was pressed.
    #[must_use]
    pub const fn alt(self) -> bool {
        self.alt
    }

    /// Returns whether the platform command modifier was pressed.
    #[must_use]
    pub const fn command(self) -> bool {
        self.command
    }
}

/// Exact presentation endpoint which physically received one scroll edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScrollDeliveryEndpoint {
    host: PresentationHostLease,
    surface: SurfaceId,
    binding: Option<ViewportBinding>,
    coordinate_generation: CoordinateGeneration,
}

impl ScrollDeliveryEndpoint {
    /// Creates one endpoint-bound delivery fact.
    ///
    /// # Errors
    ///
    /// Returns [`ScrollEdgeError`] when a native binding names another surface
    /// or engine authority domain.
    pub fn new(
        host: PresentationHostLease,
        surface: SurfaceId,
        binding: Option<ViewportBinding>,
        coordinate_generation: CoordinateGeneration,
    ) -> Result<Self, ScrollEdgeError> {
        if let Some(binding) = binding {
            if binding.surface() != surface {
                return Err(ScrollEdgeError::DeliverySurfaceMismatch {
                    surface,
                    binding_surface: binding.surface(),
                });
            }
            if binding.authority_domain() != host.authority_domain() {
                return Err(ScrollEdgeError::DeliveryAuthorityDomainMismatch);
            }
        }
        Ok(Self {
            host,
            surface,
            binding,
            coordinate_generation,
        })
    }

    /// Returns the presentation host which received the edge.
    #[must_use]
    pub const fn host(self) -> PresentationHostLease {
        self.host
    }

    /// Returns the logical surface which received the edge.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    /// Returns the exact native binding, when delivery was native.
    #[must_use]
    pub const fn binding(self) -> Option<ViewportBinding> {
        self.binding
    }

    /// Returns the coordinate generation observed at delivery.
    #[must_use]
    pub const fn coordinate_generation(self) -> CoordinateGeneration {
        self.coordinate_generation
    }
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

impl ScrollEdge {
    /// Creates and structurally validates one scroll sample.
    ///
    /// # Errors
    ///
    /// Returns [`ScrollEdgeError`] when phase, sequence, delta, or physical
    /// coordinate facts do not form a legal edge.
    pub fn new(
        device: ScrollDeviceId,
        sequence: Option<ScrollSequenceToken>,
        phase: ScrollPhase,
        delta: Option<ScrollDelta>,
        momentum: Authority<ScrollMomentum>,
        modifiers: Authority<ScrollModifiers>,
        delivery: Authority<ScrollDeliveryEndpoint>,
    ) -> Result<Self, ScrollEdgeError> {
        let edge = Self {
            device,
            sequence,
            phase,
            delta,
            momentum,
            modifiers,
            delivery,
        };
        edge.validate()?;
        Ok(edge)
    }

    fn validate(self) -> Result<(), ScrollEdgeError> {
        let legal_shape = match self.phase {
            ScrollPhase::Discrete => self.sequence.is_none() && self.delta.is_some(),
            ScrollPhase::Begin => self.sequence.is_some(),
            ScrollPhase::Update => self.sequence.is_some() && self.delta.is_some(),
            ScrollPhase::End => self.sequence.is_some(),
            ScrollPhase::Cancel(_) => self.sequence.is_some() && self.delta.is_none(),
        };
        if !legal_shape {
            return Err(ScrollEdgeError::InvalidPhaseShape { phase: self.phase });
        }
        if let (
            Some(ScrollDelta::PhysicalPixels {
                binding,
                coordinate_generation,
                ..
            }),
            Authority::Known(delivery),
        ) = (self.delta, self.delivery)
            && (delivery.binding() != Some(binding)
                || delivery.coordinate_generation() != coordinate_generation)
        {
            return Err(ScrollEdgeError::PhysicalDeltaEndpointMismatch);
        }
        Ok(())
    }

    /// Returns the provider-owned device identity.
    #[must_use]
    pub const fn device(self) -> ScrollDeviceId {
        self.device
    }

    /// Returns the provider sequence token for a phaseful sample.
    #[must_use]
    pub const fn sequence(self) -> Option<ScrollSequenceToken> {
        self.sequence
    }

    /// Returns the native scroll phase.
    #[must_use]
    pub const fn phase(self) -> ScrollPhase {
        self.phase
    }

    /// Returns the optional raw delta.
    #[must_use]
    pub const fn delta(self) -> Option<ScrollDelta> {
        self.delta
    }

    /// Returns direct-versus-momentum authority.
    #[must_use]
    pub const fn momentum(self) -> Authority<ScrollMomentum> {
        self.momentum
    }

    /// Returns the event-time modifier authority.
    #[must_use]
    pub const fn modifiers(self) -> Authority<ScrollModifiers> {
        self.modifiers
    }

    /// Returns the physical delivery endpoint authority.
    #[must_use]
    pub const fn delivery(self) -> Authority<ScrollDeliveryEndpoint> {
        self.delivery
    }
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
    /// One button became released.
    ButtonReleased(PointerButton),
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
    stream_terminal: bool,
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
            stream_terminal: false,
            location,
            delivery_owner,
            capture_owner,
        }
    }

    /// Marks this physical edge as the final edge of an ephemeral pointer stream.
    ///
    /// Touch providers use this on the normal release edge. The release keeps
    /// its interaction semantics, while the ledger retires the stream only
    /// after the exact edge is accepted. Core interactions owned by that exact
    /// stream reach their terminal transition in the same ordered reduction.
    #[must_use]
    pub const fn ending_stream(mut self) -> Self {
        self.stream_terminal = true;
        self
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
        self.stream_terminal
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

impl PointerProviderIncarnation {
    #[allow(
        dead_code,
        reason = "reserved for the independent pointer-ledger integration"
    )]
    fn checked_next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
struct PointerStreamIncarnation(u64);

impl PointerStreamIncarnation {
    fn checked_next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

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

impl PointerInputLease {
    #[allow(
        dead_code,
        reason = "reserved for the host-frame reducer authority boundary"
    )]
    pub(crate) const fn new(
        authority_domain: EngineAuthorityDomainId,
        incarnation: u64,
        scope: PointerProviderScope,
    ) -> Self {
        Self {
            authority_domain,
            incarnation: PointerProviderIncarnation(incarnation),
            scope,
        }
    }

    /// Returns the engine authority domain which minted this lease.
    #[must_use]
    pub const fn authority_domain(self) -> EngineAuthorityDomainId {
        self.authority_domain
    }

    /// Returns the process-local diagnostic representation of this incarnation.
    #[must_use]
    pub const fn incarnation(self) -> u64 {
        self.incarnation.0
    }

    /// Returns the immutable observation lane assigned to this incarnation.
    #[must_use]
    pub const fn scope(self) -> PointerProviderScope {
        self.scope
    }
}

/// Opaque core-minted identity of one pointer stream in one provider lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PointerStreamId {
    lease: PointerInputLease,
    pointer: PointerId,
    incarnation: PointerStreamIncarnation,
}

impl PointerStreamId {
    #[allow(
        dead_code,
        reason = "reserved for the host-frame pointer reducer boundary"
    )]
    pub(crate) const fn new(
        lease: PointerInputLease,
        pointer: PointerId,
        incarnation: u64,
    ) -> Self {
        Self {
            lease,
            pointer,
            incarnation: PointerStreamIncarnation(incarnation),
        }
    }

    /// Returns the exact provider incarnation which owns this stream.
    #[must_use]
    pub const fn lease(self) -> PointerInputLease {
        self.lease
    }

    /// Returns the provider-owned pointer identity within that incarnation.
    #[must_use]
    pub const fn pointer(self) -> PointerId {
        self.pointer
    }

    /// Returns the process-local diagnostic representation of this stream
    /// incarnation.
    ///
    /// Incarnations are minted monotonically by the core at journal commit.
    /// They are never provider input and cannot be reused after cancellation.
    #[must_use]
    pub const fn incarnation(self) -> u64 {
        self.incarnation.0
    }
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

impl PointerEdgeTicket {
    #[allow(
        dead_code,
        reason = "reserved for the host-frame reducer acknowledgement boundary"
    )]
    pub(crate) const fn new(lease: PointerInputLease, sequence: PointerEdgeSequence) -> Self {
        Self { lease, sequence }
    }

    /// Returns the exact provider incarnation which owns this edge.
    #[must_use]
    pub const fn lease(self) -> PointerInputLease {
        self.lease
    }

    /// Returns the engine authority domain which minted this ticket's lease.
    #[must_use]
    pub const fn authority_domain(self) -> EngineAuthorityDomainId {
        self.lease.authority_domain()
    }

    /// Returns the accepted provider edge sequence.
    #[must_use]
    pub const fn sequence(self) -> PointerEdgeSequence {
        self.sequence
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JournalPointerAuthority {
    buttons_complete: bool,
    unavailable_reason: AuthorityUnavailableReason,
    pressed_buttons: BTreeSet<(PointerId, PointerButton)>,
    captures: BTreeMap<PointerId, Authority<PointerCaptureOwner>>,
}

impl JournalPointerAuthority {
    fn unknown(reason: AuthorityUnavailableReason) -> Self {
        Self {
            buttons_complete: false,
            unavailable_reason: reason,
            pressed_buttons: BTreeSet::new(),
            captures: BTreeMap::new(),
        }
    }

    fn apply_checkpoint(&mut self, checkpoint: &PointerAuthorityCheckpoint) {
        self.pressed_buttons.clear();
        self.captures.clear();
        match checkpoint.pointers() {
            Authority::Known(pointers) => {
                self.buttons_complete = true;
                for pointer in pointers {
                    for button in pointer.pressed_buttons() {
                        let _ = self.pressed_buttons.insert((pointer.pointer(), *button));
                    }
                    let _ = self
                        .captures
                        .insert(pointer.pointer(), pointer.capture_owner());
                }
            }
            Authority::Unknown(reason) => {
                self.buttons_complete = false;
                self.unavailable_reason = *reason;
            }
        }
    }

    fn apply_edge(
        &mut self,
        edge: &PointerEdge,
    ) -> (AnyButtonDownAuthority, Authority<PointerCaptureOwner>) {
        let pointer = edge.pointer();
        let _ = self.captures.insert(pointer, edge.capture_owner());
        match edge.kind() {
            PointerEdgeKind::ButtonPressed(button) => {
                let _ = self.pressed_buttons.insert((pointer, button));
            }
            PointerEdgeKind::ButtonReleased(button) => {
                let _ = self.pressed_buttons.remove(&(pointer, button));
            }
            PointerEdgeKind::StreamCancelled(_) => {
                self.pressed_buttons
                    .retain(|(active, _)| *active != pointer);
                self.buttons_complete = false;
                self.unavailable_reason = AuthorityUnavailableReason::ProviderUnavailable;
                let _ = self.captures.remove(&pointer);
            }
            PointerEdgeKind::Moved
            | PointerEdgeKind::CaptureChanged
            | PointerEdgeKind::Scrolled(_) => {}
        }
        let capture = self
            .captures
            .get(&pointer)
            .copied()
            .unwrap_or(Authority::Unknown(
                AuthorityUnavailableReason::ProviderUnavailable,
            ));
        (self.button_authority(), capture)
    }

    fn button_authority(&self) -> AnyButtonDownAuthority {
        if !self.pressed_buttons.is_empty() {
            AnyButtonDownAuthority::KnownDown
        } else if self.buttons_complete {
            AnyButtonDownAuthority::KnownAllReleased
        } else {
            AnyButtonDownAuthority::Unknown(self.unavailable_reason)
        }
    }
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

impl PointerJournalLedgerVersion {
    const INITIAL: Self = Self(0);

    fn checked_next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

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

#[allow(
    dead_code,
    reason = "reserved for the independent pointer-ledger integration"
)]
impl PreparedPointerJournal {
    /// Returns the exact provider lease that was validated during preparation.
    pub(crate) const fn lease(&self) -> PointerInputLease {
        self.lease
    }

    /// Returns the journal that was validated during preparation.
    pub(crate) const fn journal(&self) -> &PointerEdgeJournal {
        &self.journal
    }
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
    capture_authority_after: Authority<PointerCaptureOwner>,
}

#[allow(
    dead_code,
    reason = "reserved for the independent pointer-ledger integration"
)]
impl AcceptedPointerEdge {
    /// Returns the exact core-minted stream affected by this accepted edge.
    pub(crate) const fn stream(&self) -> PointerStreamId {
        self.stream
    }

    /// Returns the exact acknowledgement identity of this accepted edge.
    pub(crate) const fn ticket(&self) -> PointerEdgeTicket {
        self.ticket
    }

    pub(crate) const fn button_authority_after(&self) -> AnyButtonDownAuthority {
        self.button_authority_after
    }

    pub(crate) const fn capture_authority_after(&self) -> Authority<PointerCaptureOwner> {
        self.capture_authority_after
    }
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

#[allow(
    dead_code,
    reason = "reserved for the independent pointer-ledger integration"
)]
impl PointerJournalCommit {
    pub(crate) const fn lease(&self) -> PointerInputLease {
        self.lease
    }

    pub(crate) const fn journal(&self) -> &PointerEdgeJournal {
        &self.journal
    }

    /// Returns the stream and ticket identity of every journal edge in exact
    /// provider order.
    ///
    /// Entry `n` corresponds to journal edge `n`. A `StreamCancelled` entry
    /// still names the stream it terminates; only a later edge for the same
    /// provider pointer receives a successor stream incarnation.
    pub(crate) fn accepted_edges(&self) -> &[AcceptedPointerEdge] {
        &self.accepted_edges
    }
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
    last_checkpoint: Option<PointerAuthorityCheckpoint>,
    active: Option<ActivePointerProvider>,
    retired: BTreeMap<PointerInputLease, RetiredPointerInputLease>,
    compacted_retired_through: PointerProviderIncarnation,
}

/// Cloning a ledger creates a speculative snapshot of the same logical
/// authority, for `DockEngine`'s atomic candidate reduction. It is not an
/// independent provider authority: the clone intentionally shares the ledger
/// identity so a token prepared before candidate cloning can commit on the
/// candidate that will replace the source state.
impl Clone for PointerJournalLedger {
    fn clone(&self) -> Self {
        Self {
            authority_domain: self.authority_domain,
            identity: Arc::clone(&self.identity),
            version: self.version,
            last_incarnation: self.last_incarnation,
            last_stream_incarnation: self.last_stream_incarnation,
            active_streams: self.active_streams.clone(),
            authority: self.authority.clone(),
            last_checkpoint: self.last_checkpoint.clone(),
            active: self.active,
            retired: self.retired.clone(),
            compacted_retired_through: self.compacted_retired_through,
        }
    }
}

impl PartialEq for PointerJournalLedger {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.identity, &other.identity)
            && self.authority_domain == other.authority_domain
            && self.version == other.version
            && self.last_incarnation == other.last_incarnation
            && self.last_stream_incarnation == other.last_stream_incarnation
            && self.active_streams == other.active_streams
            && self.authority == other.authority
            && self.last_checkpoint == other.last_checkpoint
            && self.active == other.active
            && self.retired == other.retired
            && self.compacted_retired_through == other.compacted_retired_through
    }
}

impl Eq for PointerJournalLedger {}

#[allow(
    dead_code,
    reason = "reserved for the independent pointer-ledger integration"
)]
impl PointerJournalLedger {
    pub(crate) fn new(authority_domain: EngineAuthorityDomainId) -> Self {
        Self {
            authority_domain,
            identity: Arc::new(PointerJournalLedgerIdentity),
            version: PointerJournalLedgerVersion::INITIAL,
            last_incarnation: PointerProviderIncarnation(0),
            last_stream_incarnation: PointerStreamIncarnation(0),
            active_streams: BTreeMap::new(),
            authority: JournalPointerAuthority::unknown(
                AuthorityUnavailableReason::ProviderUnavailable,
            ),
            last_checkpoint: None,
            active: None,
            retired: BTreeMap::new(),
            compacted_retired_through: PointerProviderIncarnation(0),
        }
    }

    pub(crate) fn retention_manifest(&self) -> crate::retention::PointerRetentionManifest {
        let detailed_in_compacted_range = self
            .retired
            .keys()
            .filter(|lease| lease.incarnation <= self.compacted_retired_through)
            .count() as u64;
        let logical_compacted_leases = self
            .compacted_retired_through
            .0
            .saturating_sub(detailed_in_compacted_range);
        crate::retention::PointerRetentionManifest::new(
            usize::from(self.active.is_some()),
            self.active_streams.len(),
            self.retired.len(),
            usize::from(self.compacted_retired_through.0 > 0),
            logical_compacted_leases,
        )
    }

    /// Returns the sole live provider lease, if this authority currently owns one.
    pub(crate) const fn active_lease(&self) -> Option<PointerInputLease> {
        match self.active {
            Some(active) => Some(active.lease),
            None => None,
        }
    }

    pub(crate) fn button_authority(&self) -> AnyButtonDownAuthority {
        self.authority.button_authority()
    }

    /// Creates the sole live provider incarnation with one immutable scope.
    ///
    /// `committed_through` is the provider watermark already known at the
    /// authority handoff. The first journal must begin exactly there.
    pub(crate) fn create_provider(
        &mut self,
        scope: PointerProviderScope,
        committed_through: PointerEdgeSequence,
    ) -> Result<PointerInputLease, PointerJournalLedgerError> {
        if let Some(active) = self.active {
            return Err(PointerJournalLedgerError::ProviderAlreadyActive {
                active: active.lease,
            });
        }
        self.validate_scope(scope)?;

        let incarnation = self
            .last_incarnation
            .checked_next()
            .ok_or(PointerJournalLedgerError::ProviderIncarnationExhausted)?;
        let next_version = self.next_version()?;
        let lease = PointerInputLease::new(self.authority_domain, incarnation.0, scope);
        self.last_incarnation = incarnation;
        self.version = next_version;
        self.active_streams.clear();
        self.authority =
            JournalPointerAuthority::unknown(AuthorityUnavailableReason::ProviderUnavailable);
        self.last_checkpoint = None;
        self.active = Some(ActivePointerProvider {
            lease,
            committed_through,
        });
        Ok(lease)
    }

    /// Permanently retires one exact live provider incarnation.
    pub(crate) fn retire_provider(
        &mut self,
        lease: PointerInputLease,
    ) -> Result<(), PointerJournalLedgerError> {
        let committed_through = self.require_active(lease)?;
        let next_version = self.next_version()?;
        let tombstone = RetiredPointerInputLease {
            lease,
            committed_through,
        };
        self.active = None;
        self.active_streams.clear();
        self.authority =
            JournalPointerAuthority::unknown(AuthorityUnavailableReason::ProviderUnavailable);
        self.last_checkpoint = None;
        let _ = self.retired.insert(lease, tombstone);
        self.version = next_version;
        Ok(())
    }

    /// Compacts one exact retired lease after its producer lane has stopped and joined.
    ///
    /// The detailed final watermark is discarded, but the monotonic incarnation frontier
    /// continues to classify every delayed journal from this lease as retired. Callers must own
    /// the typed quiescence proof; ordinary retirement alone is not sufficient.
    pub(crate) fn compact_quiesced_backend(
        &mut self,
        receipt: &BackendIngressDrainReceipt,
    ) -> Result<(), PointerJournalLedgerError> {
        self.compact_retired_provider(receipt.lease().pointer_provider())
    }

    fn compact_retired_provider(
        &mut self,
        lease: PointerInputLease,
    ) -> Result<(), PointerJournalLedgerError> {
        if lease.authority_domain() != self.authority_domain {
            return Err(PointerJournalLedgerError::ForeignLease {
                expected: self.authority_domain,
                submitted: lease.authority_domain(),
            });
        }
        if !self.retired.contains_key(&lease) {
            if lease.incarnation <= self.compacted_retired_through && lease.incarnation.0 != 0 {
                return Err(PointerJournalLedgerError::CompactedLease { lease });
            }
            return Err(PointerJournalLedgerError::UnknownLease { lease });
        }
        let next_version = self.next_version()?;
        let removed = self.retired.remove(&lease);
        debug_assert!(removed.is_some(), "validated retirement remains present");
        self.compacted_retired_through = self.compacted_retired_through.max(lease.incarnation);
        self.version = next_version;
        Ok(())
    }

    /// Validates and freezes one journal candidate without mutating the ledger.
    ///
    /// The returned token contains no tickets and cannot authorize input on
    /// its own. The host must first complete its exact receipt join, then pass
    /// the token to [`Self::commit_prepared`]. Rejected receipts can therefore
    /// retry preparation without consuming the provider watermark.
    pub(crate) fn prepare_candidate(
        &self,
        lease: PointerInputLease,
        journal: PointerEdgeJournal,
    ) -> Result<PreparedPointerJournal, PointerJournalLedgerError> {
        let committed_through = self.require_active(lease)?;
        journal
            .validate()
            .map_err(PointerJournalLedgerError::InvalidJournal)?;
        if journal.previous() < committed_through {
            return Err(PointerJournalLedgerError::JournalReplay {
                lease,
                committed_through,
                submitted_previous: journal.previous(),
            });
        }
        if journal.previous() > committed_through {
            return Err(PointerJournalLedgerError::JournalAhead {
                lease,
                committed_through,
                submitted_previous: journal.previous(),
            });
        }
        for edge in journal.edges() {
            let submitted = edge.location().lane();
            let matches_scope = matches!(
                (lease.scope(), submitted),
                (
                    PointerProviderScope::DesktopGlobal,
                    PointerEdgeLocationLane::Desktop
                ) | (
                    PointerProviderScope::SurfaceLocal(_),
                    PointerEdgeLocationLane::SurfaceLocal
                )
            );
            if !matches_scope {
                return Err(PointerJournalLedgerError::LocationScopeMismatch {
                    lease,
                    sequence: edge.sequence(),
                    submitted,
                });
            }
            self.validate_capture_scope(lease, edge)?;
            self.validate_delivery_scope(lease, edge)?;
            self.validate_desktop_route_scope(lease, edge)?;
            self.validate_scroll_scope(lease, edge)?;
        }
        if let Some(checkpoint) = journal.authority_checkpoint()
            && let Authority::Known(pointers) = checkpoint.pointers()
        {
            for pointer in pointers {
                self.validate_capture_owner(
                    lease,
                    checkpoint.observed_through(),
                    pointer.capture_owner(),
                )?;
            }
        }

        Ok(PreparedPointerJournal {
            ledger_identity: Arc::clone(&self.identity),
            ledger_version: self.version,
            lease,
            committed_through,
            journal,
        })
    }

    /// Atomically commits one exact journal prepared by this ledger instance.
    ///
    /// The prepared token is accepted only when the live provider lease,
    /// committed watermark, and ledger mutation version still exactly match
    /// the state observed by `prepare_candidate`. Every successful edge then
    /// receives a ticket and exact stream bound to that lease. A successful
    /// empty interval also advances the ledger version so the token cannot be
    /// replayed.
    pub(crate) fn commit_prepared(
        &mut self,
        prepared: PreparedPointerJournal,
    ) -> Result<PointerJournalCommit, PointerJournalLedgerError> {
        if !Arc::ptr_eq(&self.identity, &prepared.ledger_identity) {
            return Err(PointerJournalLedgerError::ForeignPreparedJournal);
        }

        let active_through = self.require_active(prepared.lease)?;
        if self.version != prepared.ledger_version
            || active_through != prepared.committed_through
            || prepared.journal.previous() != prepared.committed_through
        {
            return Err(PointerJournalLedgerError::PreparedJournalStale {
                lease: prepared.lease,
                prepared_version: prepared.ledger_version.0,
                active_version: self.version.0,
                prepared_committed_through: prepared.committed_through,
                active_committed_through: active_through,
            });
        }

        let next_version = self.next_version()?;
        let (last_stream_incarnation, active_streams, authority, last_checkpoint, accepted_edges) =
            self.plan_accepted_edges(prepared.lease, &prepared.journal)?;

        let Some(active) = &mut self.active else {
            return Err(PointerJournalLedgerError::UnknownLease {
                lease: prepared.lease,
            });
        };
        if active.lease != prepared.lease {
            return Err(PointerJournalLedgerError::UnknownLease {
                lease: prepared.lease,
            });
        }
        active.committed_through = prepared.journal.through();
        self.last_stream_incarnation = last_stream_incarnation;
        self.active_streams = active_streams;
        self.authority = authority;
        self.last_checkpoint = last_checkpoint;
        self.version = next_version;

        Ok(PointerJournalCommit {
            lease: prepared.lease,
            journal: prepared.journal,
            accepted_edges,
        })
    }

    fn validate_capture_scope(
        &self,
        lease: PointerInputLease,
        edge: &PointerEdge,
    ) -> Result<(), PointerJournalLedgerError> {
        self.validate_capture_owner(lease, edge.sequence(), edge.capture_owner())
    }

    fn validate_delivery_scope(
        &self,
        lease: PointerInputLease,
        edge: &PointerEdge,
    ) -> Result<(), PointerJournalLedgerError> {
        let Authority::Known(delivery_owner) = edge.delivery_owner() else {
            return Ok(());
        };

        match (lease.scope(), delivery_owner) {
            (PointerProviderScope::DesktopGlobal, PointerEventDeliveryOwner::ProviderEndpoint)
            | (PointerProviderScope::SurfaceLocal(_), PointerEventDeliveryOwner::Native(_)) => {
                Err(PointerJournalLedgerError::DeliveryScopeMismatch {
                    lease,
                    sequence: edge.sequence(),
                    submitted: delivery_owner,
                })
            }
            (PointerProviderScope::DesktopGlobal, PointerEventDeliveryOwner::Native(binding))
                if binding.authority_domain() != self.authority_domain =>
            {
                Err(PointerJournalLedgerError::ForeignDeliveryBinding {
                    lease,
                    sequence: edge.sequence(),
                    binding,
                    expected: self.authority_domain,
                    submitted: binding.authority_domain(),
                })
            }
            _ => Ok(()),
        }
    }

    fn validate_capture_owner(
        &self,
        lease: PointerInputLease,
        sequence: PointerEdgeSequence,
        capture: Authority<PointerCaptureOwner>,
    ) -> Result<(), PointerJournalLedgerError> {
        let Authority::Known(capture_owner) = capture else {
            return Ok(());
        };

        match (lease.scope(), capture_owner) {
            (PointerProviderScope::DesktopGlobal, PointerCaptureOwner::ProviderEndpoint)
            | (PointerProviderScope::SurfaceLocal(_), PointerCaptureOwner::Native(_)) => {
                Err(PointerJournalLedgerError::CaptureScopeMismatch {
                    lease,
                    sequence,
                    submitted: capture_owner,
                })
            }
            (PointerProviderScope::DesktopGlobal, PointerCaptureOwner::Native(binding))
                if binding.authority_domain() != self.authority_domain =>
            {
                Err(PointerJournalLedgerError::ForeignCaptureBinding {
                    lease,
                    sequence,
                    binding,
                    expected: self.authority_domain,
                    submitted: binding.authority_domain(),
                })
            }
            _ => Ok(()),
        }
    }

    fn validate_desktop_route_scope(
        &self,
        lease: PointerInputLease,
        edge: &PointerEdge,
    ) -> Result<(), PointerJournalLedgerError> {
        let Some(binding) = edge
            .desktop_route()
            .and_then(|route| match route.hovered() {
                Authority::Known(DesktopHoveredWindow::Dock(binding)) => Some(binding),
                Authority::Known(DesktopHoveredWindow::Foreign | DesktopHoveredWindow::None)
                | Authority::Unknown(_) => None,
            })
        else {
            return Ok(());
        };
        if binding.authority_domain() != self.authority_domain {
            return Err(PointerJournalLedgerError::ForeignDesktopRouteBinding {
                lease,
                sequence: edge.sequence(),
                binding,
                expected: self.authority_domain,
                submitted: binding.authority_domain(),
            });
        }
        Ok(())
    }

    fn validate_scroll_scope(
        &self,
        lease: PointerInputLease,
        edge: &PointerEdge,
    ) -> Result<(), PointerJournalLedgerError> {
        let PointerEdgeKind::Scrolled(scroll) = edge.kind() else {
            return Ok(());
        };
        let Authority::Known(endpoint) = scroll.delivery() else {
            return Ok(());
        };
        if endpoint.host().authority_domain() != self.authority_domain {
            return Err(PointerJournalLedgerError::ForeignScrollDeliveryHost {
                lease,
                sequence: edge.sequence(),
                host: endpoint.host(),
            });
        }
        match lease.scope() {
            PointerProviderScope::SurfaceLocal(scope)
                if endpoint.host() != scope.host()
                    || endpoint.surface() != scope.surface()
                    || endpoint.binding() != scope.endpoint().native_binding() =>
            {
                Err(PointerJournalLedgerError::ScrollDeliveryScopeMismatch {
                    lease,
                    sequence: edge.sequence(),
                    endpoint,
                })
            }
            PointerProviderScope::DesktopGlobal if endpoint.binding().is_none() => Err(
                PointerJournalLedgerError::DesktopScrollDeliveryBindingMissing {
                    lease,
                    sequence: edge.sequence(),
                    endpoint,
                },
            ),
            PointerProviderScope::DesktopGlobal
            | PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope { .. }) => {
                if let (Authority::Known(owner), Some(binding)) =
                    (edge.delivery_owner(), endpoint.binding())
                    && owner != PointerEventDeliveryOwner::Native(binding)
                    && owner != PointerEventDeliveryOwner::ProviderEndpoint
                {
                    return Err(PointerJournalLedgerError::ScrollDeliveryOwnerMismatch {
                        lease,
                        sequence: edge.sequence(),
                        endpoint,
                        owner,
                    });
                }
                Ok(())
            }
        }
    }

    fn plan_accepted_edges(
        &self,
        lease: PointerInputLease,
        journal: &PointerEdgeJournal,
    ) -> Result<
        (
            PointerStreamIncarnation,
            BTreeMap<PointerId, PointerStreamId>,
            JournalPointerAuthority,
            Option<PointerAuthorityCheckpoint>,
            Vec<AcceptedPointerEdge>,
        ),
        PointerJournalLedgerError,
    > {
        let mut last_stream_incarnation = self.last_stream_incarnation;
        let mut active_streams = self.active_streams.clone();
        let mut authority = self.authority.clone();
        let mut last_checkpoint = self.last_checkpoint.clone();
        let mut accepted_edges = Vec::with_capacity(journal.len());

        if let Some(checkpoint) = journal.authority_checkpoint() {
            if last_checkpoint.as_ref().is_some_and(|retained| {
                retained.observed_through() == checkpoint.observed_through()
            }) {
                if last_checkpoint.as_ref() != Some(checkpoint) {
                    return Err(PointerJournalLedgerError::ConflictingAuthorityCheckpoint {
                        lease,
                        observed_through: checkpoint.observed_through(),
                    });
                }
            } else {
                authority.apply_checkpoint(checkpoint);
                last_checkpoint = Some(checkpoint.clone());
            }
        }

        for edge in journal.edges() {
            let stream = match active_streams.get(&edge.pointer()).copied() {
                Some(stream) => stream,
                None => {
                    last_stream_incarnation = last_stream_incarnation
                        .checked_next()
                        .ok_or(PointerJournalLedgerError::StreamIncarnationExhausted)?;
                    let stream =
                        PointerStreamId::new(lease, edge.pointer(), last_stream_incarnation.0);
                    let _ = active_streams.insert(edge.pointer(), stream);
                    stream
                }
            };
            let (button_authority_after, capture_authority_after) = authority.apply_edge(edge);
            accepted_edges.push(AcceptedPointerEdge {
                stream,
                ticket: PointerEdgeTicket::new(lease, edge.sequence()),
                button_authority_after,
                capture_authority_after,
            });

            if edge.ends_stream() || matches!(edge.kind(), PointerEdgeKind::StreamCancelled(_)) {
                let _ = active_streams.remove(&edge.pointer());
            }
        }

        if !journal.is_empty() {
            last_checkpoint = None;
        }

        Ok((
            last_stream_incarnation,
            active_streams,
            authority,
            last_checkpoint,
            accepted_edges,
        ))
    }

    fn validate_scope(&self, scope: PointerProviderScope) -> Result<(), PointerJournalLedgerError> {
        let PointerProviderScope::SurfaceLocal(local) = scope else {
            return Ok(());
        };
        let host_domain = local.host().authority_domain();
        if host_domain != self.authority_domain {
            return Err(PointerJournalLedgerError::ForeignSurfaceLocalHost {
                host: local.host(),
                expected: self.authority_domain,
                submitted: host_domain,
            });
        }
        if let Some(binding) = local.endpoint().native_binding()
            && binding.authority_domain() != self.authority_domain
        {
            return Err(PointerJournalLedgerError::ForeignSurfaceLocalBinding {
                binding,
                expected: self.authority_domain,
                submitted: binding.authority_domain(),
            });
        }
        Ok(())
    }

    fn require_active(
        &self,
        lease: PointerInputLease,
    ) -> Result<PointerEdgeSequence, PointerJournalLedgerError> {
        if lease.authority_domain() != self.authority_domain {
            return Err(PointerJournalLedgerError::ForeignLease {
                expected: self.authority_domain,
                submitted: lease.authority_domain(),
            });
        }
        if let Some(active) = self.active
            && active.lease == lease
        {
            return Ok(active.committed_through);
        }
        if let Some(tombstone) = self.retired.get(&lease) {
            return Err(PointerJournalLedgerError::RetiredLease {
                lease: tombstone.lease,
                committed_through: tombstone.committed_through,
            });
        }
        if lease.incarnation <= self.compacted_retired_through && lease.incarnation.0 != 0 {
            return Err(PointerJournalLedgerError::CompactedLease { lease });
        }
        Err(PointerJournalLedgerError::UnknownLease { lease })
    }

    fn next_version(&self) -> Result<PointerJournalLedgerVersion, PointerJournalLedgerError> {
        self.version
            .checked_next()
            .ok_or(PointerJournalLedgerError::LedgerVersionExhausted)
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend_ingress::BackendIngressRecorder;
    use crate::geometry::{PhysicalRect, ScaleFactor};
    use crate::ids::{SurfaceId, WorkspaceEpoch};
    use crate::intent::AuthorityUnavailableReason;
    use crate::platform::{
        CapabilityRosterObservation, InputEffectAcknowledgement, PlatformCapabilities,
        PlatformCapability, PlatformSnapshot, PresentationEffectAcknowledgement,
        WindowCoordinateObservation, WindowInputObservation, WindowInputState,
        WindowInventoryObservation, WindowPresentationObservation, WindowPresentationState,
        WorkAreaRosterObservation,
    };
    use crate::platform_provider::PlatformObservationAuthority;
    use crate::presentation_observation::PresentationLedger;
    use crate::viewport::{
        CapabilityObservationGeneration, CoordinateObservationGeneration,
        InputObservationGeneration, InventoryObservationGeneration,
        PresentationObservationGeneration, ViewportBinding, ViewportRole, WindowIncarnation,
        WindowToken, WorkAreaObservationGeneration,
    };
    use crate::viewport_focus::{FocusObservationGeneration, unknown_focus_observation};

    #[derive(Debug, Clone, Copy)]
    enum PointerWindow {
        Dock(ViewportBinding),
        Foreign,
        None,
    }

    fn domain(value: u64) -> EngineAuthorityDomainId {
        EngineAuthorityDomainId::new_for_test(value)
    }

    fn binding(domain: EngineAuthorityDomainId, surface: u64, token: u64) -> ViewportBinding {
        binding_with_incarnation(domain, surface, token, 1)
    }

    fn binding_with_incarnation(
        domain: EngineAuthorityDomainId,
        surface: u64,
        token: u64,
        incarnation: u64,
    ) -> ViewportBinding {
        ViewportBinding::new(
            domain,
            WorkspaceEpoch::new(1),
            SurfaceId::new(surface),
            WindowToken::new(token),
            WindowIncarnation::new(incarnation),
        )
    }

    fn host(domain: EngineAuthorityDomainId) -> PresentationHostLease {
        PresentationLedger::new(domain)
            .create_host()
            .expect("presentation host")
    }

    fn desktop_lease(domain: EngineAuthorityDomainId, incarnation: u64) -> PointerInputLease {
        PointerInputLease::new(domain, incarnation, PointerProviderScope::DesktopGlobal)
    }

    fn route_capabilities() -> PlatformCapabilities {
        let mut capabilities = PlatformCapabilities::default();
        capabilities.set_authoritative_inventory(PlatformCapability::Supported);
        capabilities.set_hovered_window(PlatformCapability::Supported);
        capabilities.set_desktop_pointer_position(PlatformCapability::Supported);
        capabilities.set_authoritative_button_state(PlatformCapability::Supported);
        capabilities.set_pointer_hit_test_observation(PlatformCapability::Supported);
        capabilities.set_pointer_hit_test_control(PlatformCapability::Supported);
        capabilities
    }

    fn routeable_registry(
        domain: EngineAuthorityDomainId,
        surface: SurfaceId,
        token: WindowToken,
        origin_x: f64,
        scale: f64,
    ) -> (ViewportRegistry, ViewportBinding, CoordinateGeneration) {
        let mut registry = ViewportRegistry::new(domain);
        let binding = registry
            .register_existing(WorkspaceEpoch::new(1), surface, token, ViewportRole::Child)
            .expect("test native binding registers");
        let window = crate::platform::ObservedWindow::new(binding)
            .with_coordinate_observation(WindowCoordinateObservation::new(
                binding,
                CoordinateObservationGeneration::new(1),
                Authority::Known(
                    PhysicalRect::new(origin_x, 0.0, 600.0, 400.0)
                        .expect("test physical bounds are valid"),
                ),
                Authority::Unknown(AuthorityUnavailableReason::NotReported),
                Authority::Known(ScaleFactor::new(scale).expect("test scale factor is valid")),
                Authority::Known(ScaleFactor::new(scale).expect("test scale factor is valid")),
            ))
            .with_input_observation(WindowInputObservation::new(
                binding,
                InputObservationGeneration::new(1),
                Authority::Known(WindowInputState::ReceivesInput),
                InputEffectAcknowledgement::known(None),
            ))
            .with_presentation_observation(WindowPresentationObservation::new(
                binding,
                PresentationObservationGeneration::new(1),
                Authority::Known(WindowPresentationState::Visible),
                PresentationEffectAcknowledgement::known(None),
            ))
            .with_close_requested(Authority::Known(false));
        let snapshot = PlatformSnapshot::new(
            crate::viewport::PlatformSnapshotGeneration::new(1),
            CapabilityRosterObservation::new(
                CapabilityObservationGeneration::new(1),
                Authority::Known(route_capabilities()),
            ),
            unknown_focus_observation(
                FocusObservationGeneration::new(1),
                AuthorityUnavailableReason::NotReported,
            ),
            WindowInventoryObservation::new(
                InventoryObservationGeneration::new(1),
                Authority::Known(vec![binding]),
            )
            .expect("test inventory is canonical"),
            vec![window],
            Vec::new(),
            WorkAreaRosterObservation::new(
                WorkAreaObservationGeneration::new(1),
                Authority::Unknown(AuthorityUnavailableReason::NotReported),
            )
            .expect("test work-area tombstone is canonical"),
        )
        .expect("test platform snapshot is canonical");
        registry
            .apply_snapshot_for_test(&snapshot)
            .expect("test registry receives authoritative facts");
        let generation = registry
            .record(surface)
            .expect("registered surface remains present")
            .coordinate_generation();
        (registry, binding, generation)
    }

    fn desktop_location(hovered: Authority<PointerWindow>) -> PointerEdgeLocation {
        let position = PhysicalPoint::new(10.0, 20.0).expect("finite desktop point");
        let route = match hovered {
            Authority::Known(PointerWindow::Dock(binding)) => {
                DesktopRouteFact::dock(DesktopDockRoute::new(
                    binding,
                    CoordinateGeneration::new(0),
                    position,
                    LogicalPoint::new(10.0, 20.0).expect("finite logical point"),
                ))
            }
            Authority::Known(PointerWindow::Foreign) => {
                DesktopRouteFact::foreign(Authority::Known(position))
            }
            Authority::Known(PointerWindow::None) => DesktopRouteFact::no_window(
                Authority::Known(position),
                Authority::Unknown(AuthorityUnavailableReason::NotReported),
            ),
            Authority::Unknown(reason) => {
                DesktopRouteFact::unknown(Authority::Known(position), reason)
            }
        };
        PointerEdgeLocation::Desktop { route }
    }

    fn local_location() -> PointerEdgeLocation {
        PointerEdgeLocation::SurfaceLocal {
            position: Authority::Known(
                LogicalPoint::new(10.0, 20.0).expect("finite logical point"),
            ),
        }
    }

    fn edge(
        sequence: u64,
        kind: PointerEdgeKind,
        location: PointerEdgeLocation,
        capture: Authority<PointerCaptureOwner>,
    ) -> PointerEdge {
        edge_for_pointer(sequence, 7, kind, location, capture)
    }

    fn edge_for_pointer(
        sequence: u64,
        pointer: u64,
        kind: PointerEdgeKind,
        location: PointerEdgeLocation,
        capture: Authority<PointerCaptureOwner>,
    ) -> PointerEdge {
        PointerEdge::new(
            PointerEdgeSequence::new(sequence),
            PointerId::new(pointer),
            kind,
            location,
            capture,
        )
    }

    fn moved_journal(previous: u64, through: u64) -> PointerEdgeJournal {
        let edges = (previous..through)
            .map(|sequence| {
                edge(
                    sequence + 1,
                    PointerEdgeKind::Moved,
                    desktop_location(Authority::Known(PointerWindow::None)),
                    Authority::Known(PointerCaptureOwner::None),
                )
            })
            .collect();
        PointerEdgeJournal::new(
            PointerEdgeSequence::new(previous),
            PointerEdgeSequence::new(through),
            edges,
        )
        .expect("complete moved journal")
    }

    fn local_moved_journal(previous: u64, through: u64) -> PointerEdgeJournal {
        let edges = (previous..through)
            .map(|sequence| {
                edge(
                    sequence + 1,
                    PointerEdgeKind::Moved,
                    local_location(),
                    Authority::Known(PointerCaptureOwner::None),
                )
            })
            .collect();
        PointerEdgeJournal::new(
            PointerEdgeSequence::new(previous),
            PointerEdgeSequence::new(through),
            edges,
        )
        .expect("complete local moved journal")
    }

    fn commit_candidate(
        ledger: &mut PointerJournalLedger,
        lease: PointerInputLease,
        journal: PointerEdgeJournal,
    ) -> Result<PointerJournalCommit, PointerJournalLedgerError> {
        let prepared = ledger.prepare_candidate(lease, journal)?;
        ledger.commit_prepared(prepared)
    }

    #[test]
    fn release_then_press_across_windows_preserves_provider_order() {
        let domain = domain(1);
        let window_a = binding(domain, 10, 100);
        let window_b = binding(domain, 20, 200);
        let journal = PointerEdgeJournal::new(
            PointerEdgeSequence::new(40),
            PointerEdgeSequence::new(42),
            vec![
                edge(
                    41,
                    PointerEdgeKind::ButtonReleased(PointerButton::Primary),
                    desktop_location(Authority::Known(PointerWindow::Dock(window_a))),
                    Authority::Known(PointerCaptureOwner::None),
                ),
                edge(
                    42,
                    PointerEdgeKind::ButtonPressed(PointerButton::Primary),
                    desktop_location(Authority::Known(PointerWindow::Dock(window_b))),
                    Authority::Known(PointerCaptureOwner::Native(window_b)),
                ),
            ],
        )
        .expect("complete journal");

        assert_eq!(journal.len(), 2);
        assert_eq!(
            journal.edges()[0].location(),
            desktop_location(Authority::Known(PointerWindow::Dock(window_a)))
        );
        assert_eq!(
            journal.edges()[1].location(),
            desktop_location(Authority::Known(PointerWindow::Dock(window_b)))
        );
        assert_eq!(
            journal
                .edges()
                .iter()
                .map(PointerEdge::kind)
                .collect::<Vec<_>>(),
            vec![
                PointerEdgeKind::ButtonReleased(PointerButton::Primary),
                PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            ]
        );
    }

    #[test]
    fn equal_watermarks_encode_an_authoritative_empty_interval() {
        let watermark = PointerEdgeSequence::new(9);
        let journal = PointerEdgeJournal::new(watermark, watermark, Vec::new())
            .expect("equal watermarks permit an empty journal");

        assert!(journal.is_empty());
        assert_eq!(journal.previous(), watermark);
        assert_eq!(journal.through(), watermark);
    }

    #[test]
    fn duplicate_and_descending_sequences_are_rejected() {
        let unknown_window = Authority::Unknown(AuthorityUnavailableReason::NotReported);
        let unknown_location = desktop_location(unknown_window);
        let unknown_capture = Authority::Unknown(AuthorityUnavailableReason::NotReported);

        let duplicate = PointerEdgeJournal::new(
            PointerEdgeSequence::new(0),
            PointerEdgeSequence::new(2),
            vec![
                edge(1, PointerEdgeKind::Moved, unknown_location, unknown_capture),
                edge(1, PointerEdgeKind::Moved, unknown_location, unknown_capture),
            ],
        );
        assert_eq!(
            duplicate,
            Err(PointerJournalError::SequenceNotIncreasing {
                previous: PointerEdgeSequence::new(1),
                actual: PointerEdgeSequence::new(1),
            })
        );

        let descending = PointerEdgeJournal::new(
            PointerEdgeSequence::new(0),
            PointerEdgeSequence::new(2),
            vec![
                edge(1, PointerEdgeKind::Moved, unknown_location, unknown_capture),
                edge(0, PointerEdgeKind::Moved, unknown_location, unknown_capture),
            ],
        );
        assert_eq!(
            descending,
            Err(PointerJournalError::SequenceNotIncreasing {
                previous: PointerEdgeSequence::new(1),
                actual: PointerEdgeSequence::new(0),
            })
        );
    }

    #[test]
    fn a_gap_inside_the_declared_interval_is_rejected() {
        let result = PointerEdgeJournal::new(
            PointerEdgeSequence::new(5),
            PointerEdgeSequence::new(7),
            vec![edge(
                7,
                PointerEdgeKind::Moved,
                desktop_location(Authority::Known(PointerWindow::None)),
                Authority::Known(PointerCaptureOwner::None),
            )],
        );

        assert_eq!(
            result,
            Err(PointerJournalError::SequenceGap {
                expected: PointerEdgeSequence::new(6),
                actual: PointerEdgeSequence::new(7),
            })
        );
    }

    #[test]
    fn unavailable_capture_authority_is_not_no_capture() {
        let unknown = Authority::Unknown(AuthorityUnavailableReason::ProviderUnavailable);
        let no_capture = Authority::Known(PointerCaptureOwner::None);

        assert_ne!(unknown, no_capture);
    }

    #[test]
    fn capture_change_preserves_unknown_capture_authority() {
        let unknown = Authority::Unknown(AuthorityUnavailableReason::SurfaceUnavailable);
        let edge = PointerEdge::new(
            PointerEdgeSequence::new(1),
            PointerId::new(1),
            PointerEdgeKind::CaptureChanged,
            PointerEdgeLocation::Desktop {
                route: DesktopRouteFact::foreign(Authority::Unknown(
                    AuthorityUnavailableReason::NotReported,
                )),
            },
            unknown,
        );

        assert_eq!(edge.kind(), PointerEdgeKind::CaptureChanged);
        assert_eq!(edge.capture_owner(), unknown);
        assert_ne!(
            edge.capture_owner(),
            Authority::Known(PointerCaptureOwner::None)
        );
    }

    #[test]
    fn tickets_include_the_exact_provider_incarnation() {
        let sequence = PointerEdgeSequence::new(11);
        let first_lease = desktop_lease(domain(1), 1);
        let next_lease = desktop_lease(domain(1), 2);
        let first = PointerEdgeTicket::new(first_lease, sequence);
        let next = PointerEdgeTicket::new(next_lease, sequence);

        assert_ne!(first, next);
        assert_eq!(first.lease(), first_lease);
        assert_eq!(first.lease().incarnation(), 1);
        assert_eq!(first.sequence(), sequence);
    }

    #[test]
    fn tickets_from_foreign_engine_domains_are_distinct() {
        let sequence = PointerEdgeSequence::new(11);
        let local = PointerEdgeTicket::new(desktop_lease(domain(1), 1), sequence);
        let foreign = PointerEdgeTicket::new(desktop_lease(domain(2), 1), sequence);

        assert_ne!(local, foreign);
        assert_eq!(local.authority_domain(), domain(1));
    }

    #[test]
    fn ledger_allows_only_one_live_pointer_provider() {
        let mut ledger = PointerJournalLedger::new(domain(1));
        let first = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(10),
            )
            .expect("first provider");

        assert_eq!(
            ledger.create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(10),
            ),
            Err(PointerJournalLedgerError::ProviderAlreadyActive { active: first })
        );

        ledger.retire_provider(first).expect("retire first");
        let second = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(10),
            )
            .expect("successor provider");
        assert_eq!(first.incarnation(), 1);
        assert_eq!(second.incarnation(), 2);
    }

    #[test]
    fn ledger_advances_one_exact_watermark_across_host_frames() {
        let mut ledger = PointerJournalLedger::new(domain(1));
        let lease = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(0),
            )
            .expect("provider");

        let first =
            commit_candidate(&mut ledger, lease, moved_journal(0, 2)).expect("first frame journal");
        assert_eq!(first.lease(), lease);
        assert_eq!(first.journal().previous(), PointerEdgeSequence::new(0));
        assert_eq!(first.journal().through(), PointerEdgeSequence::new(2));
        assert_eq!(
            first
                .accepted_edges()
                .iter()
                .map(|accepted| accepted.ticket().sequence())
                .collect::<Vec<_>>(),
            vec![PointerEdgeSequence::new(1), PointerEdgeSequence::new(2)]
        );
        assert!(
            first
                .accepted_edges()
                .iter()
                .all(|accepted| accepted.ticket().lease() == lease)
        );

        let second = commit_candidate(&mut ledger, lease, moved_journal(2, 3))
            .expect("next frame journal begins at committed watermark");
        assert_eq!(second.accepted_edges().len(), 1);
        assert_eq!(
            second.accepted_edges()[0].ticket(),
            PointerEdgeTicket::new(lease, PointerEdgeSequence::new(3))
        );
    }

    #[test]
    fn preparation_is_pure_and_a_rejected_receipt_can_retry_the_same_interval() {
        let mut ledger = PointerJournalLedger::new(domain(1));
        let lease = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(0),
            )
            .expect("provider");
        let before = ledger.active;
        let before_version = ledger.version;

        let rejected_by_receipts = ledger
            .prepare_candidate(lease, moved_journal(0, 1))
            .expect("candidate is valid before receipt validation");
        assert_eq!(rejected_by_receipts.lease(), lease);
        assert_eq!(
            rejected_by_receipts.journal().through(),
            PointerEdgeSequence::new(1)
        );
        assert_eq!(ledger.active, before);
        assert_eq!(ledger.version, before_version);

        drop(rejected_by_receipts);
        let retry = ledger
            .prepare_candidate(lease, moved_journal(0, 1))
            .expect("receipt rejection did not consume the interval");
        let commit = ledger
            .commit_prepared(retry)
            .expect("retry commits after the receipt join succeeds");

        assert_eq!(commit.accepted_edges().len(), 1);
        assert_eq!(
            commit.accepted_edges()[0].ticket(),
            PointerEdgeTicket::new(lease, PointerEdgeSequence::new(1))
        );
        assert_eq!(
            ledger.active,
            Some(ActivePointerProvider {
                lease,
                committed_through: PointerEdgeSequence::new(1),
            })
        );
        assert_ne!(ledger.version, before_version);
    }

    #[test]
    fn an_empty_prepared_interval_cannot_be_replayed_after_another_commit() {
        let mut ledger = PointerJournalLedger::new(domain(1));
        let lease = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(5),
            )
            .expect("provider");
        let first = ledger
            .prepare_candidate(lease, moved_journal(5, 5))
            .expect("first empty candidate");
        let replay = ledger
            .prepare_candidate(lease, moved_journal(5, 5))
            .expect("second empty candidate observes the same state");
        let prepared_version = first.ledger_version.0;

        ledger
            .commit_prepared(first)
            .expect("first empty interval commits");
        assert_eq!(
            ledger.active,
            Some(ActivePointerProvider {
                lease,
                committed_through: PointerEdgeSequence::new(5),
            })
        );

        assert_eq!(
            ledger.commit_prepared(replay),
            Err(PointerJournalLedgerError::PreparedJournalStale {
                lease,
                prepared_version,
                active_version: ledger.version.0,
                prepared_committed_through: PointerEdgeSequence::new(5),
                active_committed_through: PointerEdgeSequence::new(5),
            })
        );
    }

    #[test]
    fn retiring_a_provider_invalidates_every_prepared_interval() {
        let mut ledger = PointerJournalLedger::new(domain(1));
        let lease = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(0),
            )
            .expect("provider");
        let prepared = ledger
            .prepare_candidate(lease, moved_journal(0, 1))
            .expect("candidate before provider retirement");

        ledger.retire_provider(lease).expect("retire provider");
        assert_eq!(
            ledger.commit_prepared(prepared),
            Err(PointerJournalLedgerError::RetiredLease {
                lease,
                committed_through: PointerEdgeSequence::new(0),
            })
        );
        assert!(ledger.active.is_none());
    }

    #[test]
    fn prepared_journal_cannot_cross_to_an_equivalent_but_distinct_ledger() {
        let authority_domain = domain(1);
        let mut source = PointerJournalLedger::new(authority_domain);
        let source_lease = source
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(0),
            )
            .expect("source provider");
        let prepared = source
            .prepare_candidate(source_lease, moved_journal(0, 1))
            .expect("source candidate");

        let mut target = PointerJournalLedger::new(authority_domain);
        let target_lease = target
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(0),
            )
            .expect("target provider");
        assert_eq!(source_lease, target_lease);

        assert_eq!(
            target.commit_prepared(prepared),
            Err(PointerJournalLedgerError::ForeignPreparedJournal)
        );
        assert_eq!(
            target.active,
            Some(ActivePointerProvider {
                lease: target_lease,
                committed_through: PointerEdgeSequence::new(0),
            })
        );
        assert_eq!(
            source.active,
            Some(ActivePointerProvider {
                lease: source_lease,
                committed_through: PointerEdgeSequence::new(0),
            })
        );
    }

    #[test]
    fn prepare_revalidates_a_structurally_complete_journal_before_freezing_it() {
        let mut ledger = PointerJournalLedger::new(domain(1));
        let lease = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(0),
            )
            .expect("provider");
        let malformed = PointerEdgeJournal {
            previous: PointerEdgeSequence::new(0),
            through: PointerEdgeSequence::new(2),
            edges: vec![edge(
                2,
                PointerEdgeKind::Moved,
                desktop_location(Authority::Known(PointerWindow::None)),
                Authority::Known(PointerCaptureOwner::None),
            )],
            authority_checkpoint: None,
        };

        let error = ledger
            .prepare_candidate(lease, malformed)
            .expect_err("malformed journal cannot be frozen");
        assert_eq!(
            error,
            PointerJournalLedgerError::InvalidJournal(PointerJournalError::SequenceGap {
                expected: PointerEdgeSequence::new(1),
                actual: PointerEdgeSequence::new(2),
            })
        );
        assert_eq!(
            ledger.active,
            Some(ActivePointerProvider {
                lease,
                committed_through: PointerEdgeSequence::new(0),
            })
        );
    }

    #[test]
    fn surface_local_scope_is_frozen_across_committed_watermarks() {
        let authority_domain = domain(1);
        let presentation_host = host(authority_domain);
        let endpoint = SurfaceLocalPointerEndpoint::Logical(SurfaceId::new(9));
        let local = SurfaceLocalPointerScope::new(presentation_host, endpoint);
        let scope = PointerProviderScope::SurfaceLocal(local);
        let mut ledger = PointerJournalLedger::new(authority_domain);

        assert_eq!(local.host(), presentation_host);
        assert_eq!(local.surface(), SurfaceId::new(9));
        assert_eq!(local.endpoint(), endpoint);
        assert_eq!(endpoint.surface(), SurfaceId::new(9));
        assert_eq!(endpoint.native_binding(), None);
        assert_eq!(scope.surface_local(), Some(local));

        let lease = ledger
            .create_provider(scope, PointerEdgeSequence::new(0))
            .expect("surface-local provider");
        let first = commit_candidate(&mut ledger, lease, local_moved_journal(0, 2))
            .expect("first local journal");
        let second = commit_candidate(&mut ledger, lease, local_moved_journal(2, 3))
            .expect("second local journal");

        assert_eq!(lease.scope(), scope);
        assert_eq!(first.lease().scope(), scope);
        assert!(
            first
                .accepted_edges()
                .iter()
                .all(|accepted| accepted.ticket().lease().scope() == scope)
        );
        assert_eq!(second.lease().scope(), scope);
        assert_eq!(
            ledger.active,
            Some(ActivePointerProvider {
                lease,
                committed_through: PointerEdgeSequence::new(3),
            })
        );
    }

    #[test]
    fn surface_local_provider_rejects_foreign_host_and_binding_before_minting() {
        let local_domain = domain(1);
        let foreign_domain = domain(2);
        let mut ledger = PointerJournalLedger::new(local_domain);
        let foreign_host = host(foreign_domain);
        let foreign_host_scope = PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(
            foreign_host,
            SurfaceLocalPointerEndpoint::Logical(SurfaceId::new(7)),
        ));

        assert_eq!(
            ledger.create_provider(foreign_host_scope, PointerEdgeSequence::new(0)),
            Err(PointerJournalLedgerError::ForeignSurfaceLocalHost {
                host: foreign_host,
                expected: local_domain,
                submitted: foreign_domain,
            })
        );
        assert!(ledger.active.is_none());
        assert_eq!(ledger.last_incarnation, PointerProviderIncarnation(0));

        let local_host = host(local_domain);
        let foreign_binding = binding(foreign_domain, 7, 70);
        let foreign_binding_scope =
            PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(
                local_host,
                SurfaceLocalPointerEndpoint::Native(foreign_binding),
            ));
        assert_eq!(
            ledger.create_provider(foreign_binding_scope, PointerEdgeSequence::new(0)),
            Err(PointerJournalLedgerError::ForeignSurfaceLocalBinding {
                binding: foreign_binding,
                expected: local_domain,
                submitted: foreign_domain,
            })
        );
        assert!(ledger.active.is_none());
        assert_eq!(ledger.last_incarnation, PointerProviderIncarnation(0));

        let local_binding = binding(local_domain, 7, 70);
        let endpoint = SurfaceLocalPointerEndpoint::Native(local_binding);
        let valid_scope =
            PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(local_host, endpoint));
        let lease = ledger
            .create_provider(valid_scope, PointerEdgeSequence::new(0))
            .expect("valid local provider after rejected candidates");
        assert_eq!(lease.incarnation(), 1);
        assert_eq!(endpoint.surface(), SurfaceId::new(7));
        assert_eq!(endpoint.native_binding(), Some(local_binding));
    }

    #[test]
    fn location_scope_mismatch_is_atomic_in_both_directions() {
        let authority_domain = domain(1);
        let mut ledger = PointerJournalLedger::new(authority_domain);
        let desktop = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(0),
            )
            .expect("desktop provider");

        assert_eq!(
            commit_candidate(&mut ledger, desktop, local_moved_journal(0, 1)),
            Err(PointerJournalLedgerError::LocationScopeMismatch {
                lease: desktop,
                sequence: PointerEdgeSequence::new(1),
                submitted: PointerEdgeLocationLane::SurfaceLocal,
            })
        );
        assert_eq!(
            ledger.active,
            Some(ActivePointerProvider {
                lease: desktop,
                committed_through: PointerEdgeSequence::new(0),
            })
        );
        commit_candidate(&mut ledger, desktop, moved_journal(0, 1))
            .expect("desktop watermark was not advanced by rejection");
        ledger
            .retire_provider(desktop)
            .expect("retire desktop provider");

        let local_scope = PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(
            host(authority_domain),
            SurfaceLocalPointerEndpoint::Logical(SurfaceId::new(5)),
        ));
        let local = ledger
            .create_provider(local_scope, PointerEdgeSequence::new(10))
            .expect("local provider");
        assert_eq!(
            commit_candidate(&mut ledger, local, moved_journal(10, 11)),
            Err(PointerJournalLedgerError::LocationScopeMismatch {
                lease: local,
                sequence: PointerEdgeSequence::new(11),
                submitted: PointerEdgeLocationLane::Desktop,
            })
        );
        assert_eq!(
            ledger.active,
            Some(ActivePointerProvider {
                lease: local,
                committed_through: PointerEdgeSequence::new(10),
            })
        );
        commit_candidate(&mut ledger, local, local_moved_journal(10, 11))
            .expect("local watermark was not advanced by rejection");
    }

    #[test]
    fn capture_owner_is_fail_closed_against_the_provider_scope() {
        let authority_domain = domain(1);
        let mut ledger = PointerJournalLedger::new(authority_domain);
        let desktop = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(0),
            )
            .expect("desktop provider");
        let desktop_endpoint_capture = PointerEdgeJournal::new(
            PointerEdgeSequence::new(0),
            PointerEdgeSequence::new(1),
            vec![edge(
                1,
                PointerEdgeKind::CaptureChanged,
                desktop_location(Authority::Known(PointerWindow::None)),
                Authority::Known(PointerCaptureOwner::ProviderEndpoint),
            )],
        )
        .expect("complete desktop journal");

        assert_eq!(
            commit_candidate(&mut ledger, desktop, desktop_endpoint_capture),
            Err(PointerJournalLedgerError::CaptureScopeMismatch {
                lease: desktop,
                sequence: PointerEdgeSequence::new(1),
                submitted: PointerCaptureOwner::ProviderEndpoint,
            })
        );
        assert_eq!(ledger.last_stream_incarnation, PointerStreamIncarnation(0));

        let foreign_binding = binding(domain(2), 7, 70);
        let foreign_capture = PointerEdgeJournal::new(
            PointerEdgeSequence::new(0),
            PointerEdgeSequence::new(1),
            vec![edge(
                1,
                PointerEdgeKind::CaptureChanged,
                desktop_location(Authority::Known(PointerWindow::Foreign)),
                Authority::Known(PointerCaptureOwner::Native(foreign_binding)),
            )],
        )
        .expect("complete foreign capture journal");
        assert_eq!(
            commit_candidate(&mut ledger, desktop, foreign_capture),
            Err(PointerJournalLedgerError::ForeignCaptureBinding {
                lease: desktop,
                sequence: PointerEdgeSequence::new(1),
                binding: foreign_binding,
                expected: authority_domain,
                submitted: domain(2),
            })
        );

        let local_binding = binding(authority_domain, 7, 70);
        let valid_desktop_capture = PointerEdgeJournal::new(
            PointerEdgeSequence::new(0),
            PointerEdgeSequence::new(1),
            vec![edge(
                1,
                PointerEdgeKind::CaptureChanged,
                desktop_location(Authority::Known(PointerWindow::Dock(local_binding))),
                Authority::Known(PointerCaptureOwner::Native(local_binding)),
            )],
        )
        .expect("complete exact native capture journal");
        commit_candidate(&mut ledger, desktop, valid_desktop_capture)
            .expect("desktop provider may report an exact local-domain native binding");
        ledger
            .retire_provider(desktop)
            .expect("retire desktop provider");

        let local_scope = PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(
            host(authority_domain),
            SurfaceLocalPointerEndpoint::Native(local_binding),
        ));
        let local = ledger
            .create_provider(local_scope, PointerEdgeSequence::new(10))
            .expect("native-backed surface-local provider");
        let forged_native_capture = PointerEdgeJournal::new(
            PointerEdgeSequence::new(10),
            PointerEdgeSequence::new(11),
            vec![edge(
                11,
                PointerEdgeKind::CaptureChanged,
                local_location(),
                Authority::Known(PointerCaptureOwner::Native(local_binding)),
            )],
        )
        .expect("complete local capture journal");
        assert_eq!(
            commit_candidate(&mut ledger, local, forged_native_capture),
            Err(PointerJournalLedgerError::CaptureScopeMismatch {
                lease: local,
                sequence: PointerEdgeSequence::new(11),
                submitted: PointerCaptureOwner::Native(local_binding),
            })
        );

        let endpoint_capture = PointerEdgeJournal::new(
            PointerEdgeSequence::new(10),
            PointerEdgeSequence::new(11),
            vec![edge(
                11,
                PointerEdgeKind::CaptureChanged,
                local_location(),
                Authority::Known(PointerCaptureOwner::ProviderEndpoint),
            )],
        )
        .expect("complete endpoint capture journal");
        commit_candidate(&mut ledger, local, endpoint_capture)
            .expect("surface-local provider reports its frozen endpoint symbolically");
    }

    #[test]
    fn native_capture_identity_preserves_window_incarnation_across_token_reuse() {
        let authority_domain = domain(1);
        let stale = binding_with_incarnation(authority_domain, 7, 70, 1);
        let successor = binding_with_incarnation(authority_domain, 7, 70, 2);
        assert_ne!(stale, successor);

        let mut ledger = PointerJournalLedger::new(authority_domain);
        let lease = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(0),
            )
            .expect("desktop provider");
        let journal = PointerEdgeJournal::new(
            PointerEdgeSequence::new(0),
            PointerEdgeSequence::new(2),
            vec![
                edge(
                    1,
                    PointerEdgeKind::CaptureChanged,
                    desktop_location(Authority::Known(PointerWindow::Dock(stale))),
                    Authority::Known(PointerCaptureOwner::Native(stale)),
                ),
                edge(
                    2,
                    PointerEdgeKind::CaptureChanged,
                    desktop_location(Authority::Known(PointerWindow::Dock(successor))),
                    Authority::Known(PointerCaptureOwner::Native(successor)),
                ),
            ],
        )
        .expect("complete capture journal");

        let commit = commit_candidate(&mut ledger, lease, journal).expect("exact native captures");
        assert_eq!(
            commit.journal().edges()[0].capture_owner(),
            Authority::Known(PointerCaptureOwner::Native(stale))
        );
        assert_eq!(
            commit.journal().edges()[1].capture_owner(),
            Authority::Known(PointerCaptureOwner::Native(successor))
        );
        assert_eq!(
            commit.accepted_edges()[0].stream(),
            commit.accepted_edges()[1].stream()
        );
    }

    #[test]
    fn desktop_route_from_another_engine_domain_is_rejected_before_watermark_commit() {
        let local_domain = domain(1);
        let foreign_domain = domain(2);
        let mut ledger = PointerJournalLedger::new(local_domain);
        let lease = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(4),
            )
            .expect("desktop provider");
        let foreign = binding(foreign_domain, 7, 70);
        let desktop = PhysicalPoint::new(12.0, 34.0).expect("finite desktop point");
        let journal = PointerEdgeJournal::new(
            PointerEdgeSequence::new(4),
            PointerEdgeSequence::new(5),
            vec![edge(
                5,
                PointerEdgeKind::Moved,
                PointerEdgeLocation::Desktop {
                    route: DesktopRouteFact::dock_without_coordinate_route(
                        foreign,
                        Authority::Known(desktop),
                        AuthorityUnavailableReason::CoordinateUnavailable,
                    ),
                },
                Authority::Known(PointerCaptureOwner::None),
            )],
        )
        .expect("journal is structurally complete");

        assert_eq!(
            commit_candidate(&mut ledger, lease, journal),
            Err(PointerJournalLedgerError::ForeignDesktopRouteBinding {
                lease,
                sequence: PointerEdgeSequence::new(5),
                binding: foreign,
                expected: local_domain,
                submitted: foreign_domain,
            })
        );
        assert_eq!(
            ledger.active,
            Some(ActivePointerProvider {
                lease,
                committed_through: PointerEdgeSequence::new(4),
            })
        );
    }

    #[test]
    fn cancellation_and_pointer_reuse_in_one_batch_get_distinct_streams() {
        let mut ledger = PointerJournalLedger::new(domain(1));
        let lease = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(0),
            )
            .expect("provider");
        let location = desktop_location(Authority::Known(PointerWindow::None));
        let capture = Authority::Known(PointerCaptureOwner::None);
        let journal = PointerEdgeJournal::new(
            PointerEdgeSequence::new(0),
            PointerEdgeSequence::new(3),
            vec![
                edge(1, PointerEdgeKind::Moved, location, capture),
                edge(
                    2,
                    PointerEdgeKind::StreamCancelled(
                        PointerStreamCancelReason::ExplicitPlatformCancellation,
                    ),
                    location,
                    capture,
                ),
                edge(
                    3,
                    PointerEdgeKind::ButtonPressed(PointerButton::Primary),
                    location,
                    capture,
                ),
            ],
        )
        .expect("complete reused-pointer journal");

        let commit = commit_candidate(&mut ledger, lease, journal).expect("journal commit");
        let accepted = commit.accepted_edges();
        assert_eq!(accepted.len(), 3);
        assert_eq!(accepted[0].stream(), accepted[1].stream());
        assert_ne!(accepted[1].stream(), accepted[2].stream());
        assert_eq!(accepted[0].stream().incarnation(), 1);
        assert_eq!(accepted[1].stream().incarnation(), 1);
        assert_eq!(accepted[2].stream().incarnation(), 2);
        assert_eq!(accepted[1].stream().pointer(), PointerId::new(7));
    }

    #[test]
    fn normal_terminal_release_retires_touch_stream_without_becoming_cancellation() {
        let mut ledger = PointerJournalLedger::new(domain(1));
        let lease = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(0),
            )
            .expect("provider");
        let location = desktop_location(Authority::Known(PointerWindow::None));
        let capture = Authority::Known(PointerCaptureOwner::None);
        let journal = PointerEdgeJournal::new(
            PointerEdgeSequence::new(0),
            PointerEdgeSequence::new(2),
            vec![
                edge(
                    1,
                    PointerEdgeKind::ButtonReleased(PointerButton::Primary),
                    location,
                    capture,
                )
                .ending_stream(),
                edge(
                    2,
                    PointerEdgeKind::ButtonPressed(PointerButton::Primary),
                    location,
                    capture,
                ),
            ],
        )
        .expect("complete reused touch journal");

        let commit = commit_candidate(&mut ledger, lease, journal).expect("journal commit");
        assert!(matches!(
            commit.journal().edges()[0].kind(),
            PointerEdgeKind::ButtonReleased(PointerButton::Primary)
        ));
        assert!(commit.journal().edges()[0].ends_stream());
        assert_ne!(
            commit.accepted_edges()[0].stream(),
            commit.accepted_edges()[1].stream()
        );
    }

    #[test]
    fn ten_thousand_terminal_touch_releases_leave_no_active_streams() {
        let mut ledger = PointerJournalLedger::new(domain(1));
        let lease = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(0),
            )
            .expect("provider");
        let location = desktop_location(Authority::Known(PointerWindow::None));
        let capture = Authority::Known(PointerCaptureOwner::None);
        let edges = (1..=10_000)
            .map(|sequence| {
                edge(
                    sequence,
                    PointerEdgeKind::ButtonReleased(PointerButton::Primary),
                    location,
                    capture,
                )
                .ending_stream()
            })
            .collect::<Vec<_>>();
        let journal = PointerEdgeJournal::new(
            PointerEdgeSequence::new(0),
            PointerEdgeSequence::new(10_000),
            edges,
        )
        .expect("complete terminal touch journal");

        let commit = commit_candidate(&mut ledger, lease, journal).expect("journal commit");
        assert_eq!(commit.accepted_edges().len(), 10_000);
        assert!(ledger.active_streams.is_empty());
        assert_eq!(ledger.last_stream_incarnation.0, 10_000);
    }

    #[test]
    fn interleaved_pointers_retain_independent_monotonic_streams() {
        let mut ledger = PointerJournalLedger::new(domain(1));
        let lease = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(0),
            )
            .expect("provider");
        let location = desktop_location(Authority::Known(PointerWindow::None));
        let capture = Authority::Known(PointerCaptureOwner::None);
        let journal = PointerEdgeJournal::new(
            PointerEdgeSequence::new(0),
            PointerEdgeSequence::new(5),
            vec![
                edge_for_pointer(1, 7, PointerEdgeKind::Moved, location, capture),
                edge_for_pointer(2, 9, PointerEdgeKind::Moved, location, capture),
                edge_for_pointer(
                    3,
                    7,
                    PointerEdgeKind::StreamCancelled(
                        PointerStreamCancelReason::ExplicitPlatformCancellation,
                    ),
                    location,
                    capture,
                ),
                edge_for_pointer(4, 9, PointerEdgeKind::Moved, location, capture),
                edge_for_pointer(5, 7, PointerEdgeKind::Moved, location, capture),
            ],
        )
        .expect("complete interleaved journal");

        let commit = commit_candidate(&mut ledger, lease, journal).expect("journal commit");
        assert_eq!(
            commit
                .accepted_edges()
                .iter()
                .map(|accepted| accepted.stream().incarnation())
                .collect::<Vec<_>>(),
            vec![1, 2, 1, 2, 3]
        );
        assert_eq!(
            commit.accepted_edges()[1].stream(),
            commit.accepted_edges()[3].stream()
        );
    }

    #[test]
    fn failed_commit_and_retry_do_not_consume_stream_incarnations() {
        let mut ledger = PointerJournalLedger::new(domain(1));
        let lease = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(0),
            )
            .expect("provider");
        let cancelled = PointerEdgeJournal::new(
            PointerEdgeSequence::new(0),
            PointerEdgeSequence::new(1),
            vec![edge(
                1,
                PointerEdgeKind::StreamCancelled(
                    PointerStreamCancelReason::ExplicitPlatformCancellation,
                ),
                desktop_location(Authority::Known(PointerWindow::None)),
                Authority::Known(PointerCaptureOwner::None),
            )],
        )
        .expect("complete cancellation journal");
        let accepted = ledger
            .prepare_candidate(lease, cancelled.clone())
            .expect("accepted candidate");
        let stale = ledger
            .prepare_candidate(lease, cancelled)
            .expect("parallel candidate");
        assert_eq!(ledger.last_stream_incarnation, PointerStreamIncarnation(0));

        let first = ledger
            .commit_prepared(accepted)
            .expect("first candidate commits");
        assert_eq!(first.accepted_edges()[0].stream().incarnation(), 1);
        assert!(matches!(
            ledger.commit_prepared(stale),
            Err(PointerJournalLedgerError::PreparedJournalStale { .. })
        ));
        assert_eq!(ledger.last_stream_incarnation, PointerStreamIncarnation(1));

        let successor = commit_candidate(&mut ledger, lease, moved_journal(1, 2))
            .expect("same provider pointer starts a successor stream");
        assert_eq!(successor.accepted_edges()[0].stream().incarnation(), 2);
    }

    #[test]
    fn candidate_clone_commits_stream_state_without_mutating_the_source_snapshot() {
        let mut source = PointerJournalLedger::new(domain(1));
        let lease = source
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(0),
            )
            .expect("provider");
        let prepared = source
            .prepare_candidate(lease, moved_journal(0, 1))
            .expect("candidate before speculative clone");
        let mut candidate = source.clone();

        let commit = candidate
            .commit_prepared(prepared)
            .expect("shared ledger identity permits atomic candidate commit");
        assert_eq!(commit.accepted_edges()[0].stream().incarnation(), 1);
        assert_eq!(source.last_stream_incarnation, PointerStreamIncarnation(0));
        assert!(source.active_streams.is_empty());
        assert_eq!(
            candidate.last_stream_incarnation,
            PointerStreamIncarnation(1)
        );
        assert_eq!(candidate.active_streams.len(), 1);
    }

    #[test]
    fn replay_and_ahead_candidates_are_rejected_atomically() {
        let mut ledger = PointerJournalLedger::new(domain(1));
        let lease = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(0),
            )
            .expect("provider");
        commit_candidate(&mut ledger, lease, moved_journal(0, 2)).expect("initial journal");

        assert_eq!(
            commit_candidate(&mut ledger, lease, moved_journal(0, 1)),
            Err(PointerJournalLedgerError::JournalReplay {
                lease,
                committed_through: PointerEdgeSequence::new(2),
                submitted_previous: PointerEdgeSequence::new(0),
            })
        );
        assert_eq!(
            commit_candidate(&mut ledger, lease, moved_journal(3, 4)),
            Err(PointerJournalLedgerError::JournalAhead {
                lease,
                committed_through: PointerEdgeSequence::new(2),
                submitted_previous: PointerEdgeSequence::new(3),
            })
        );

        let accepted = commit_candidate(&mut ledger, lease, moved_journal(2, 3))
            .expect("rejections did not advance the committed watermark");
        assert_eq!(accepted.journal().through(), PointerEdgeSequence::new(3));
    }

    #[test]
    fn foreign_unknown_and_retired_leases_are_typed_rejections() {
        let local_domain = domain(1);
        let mut ledger = PointerJournalLedger::new(local_domain);
        let active = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(5),
            )
            .expect("provider");
        let foreign = desktop_lease(domain(2), active.incarnation());
        let unknown = desktop_lease(local_domain, 99);

        assert_eq!(
            commit_candidate(&mut ledger, foreign, moved_journal(5, 5)),
            Err(PointerJournalLedgerError::ForeignLease {
                expected: local_domain,
                submitted: domain(2),
            })
        );
        assert_eq!(
            commit_candidate(&mut ledger, unknown, moved_journal(5, 5)),
            Err(PointerJournalLedgerError::UnknownLease { lease: unknown })
        );

        ledger.retire_provider(active).expect("retire active");
        assert_eq!(
            commit_candidate(&mut ledger, active, moved_journal(5, 5)),
            Err(PointerJournalLedgerError::RetiredLease {
                lease: active,
                committed_through: PointerEdgeSequence::new(5),
            })
        );
    }

    #[test]
    fn quiesced_pointer_leases_compact_to_one_fail_closed_incarnation_range() {
        let domain = domain(1);
        let mut platform_authority = PlatformObservationAuthority::new(domain);
        let platform = platform_authority
            .create()
            .expect("test platform provider must mint");
        let presentation_host = host(domain);
        let mut ledger = PointerJournalLedger::new(domain);
        let mut producers = Vec::with_capacity(10_000);

        for _ in 0..10_000 {
            let lease = ledger
                .create_provider(
                    PointerProviderScope::DesktopGlobal,
                    PointerEdgeSequence::new(0),
                )
                .expect("provider incarnation must remain available");
            ledger
                .retire_provider(lease)
                .expect("the active provider must retire exactly once");
            let recorder = BackendIngressRecorder::new(
                platform,
                lease,
                presentation_host,
                PointerEdgeSequence::new(0),
            )
            .expect("the joined producer must bind the exact pointer lease");
            producers.push((lease, recorder));
        }

        let before = ledger.retention_manifest();
        assert_eq!(before.active_provider_count(), 0);
        assert_eq!(before.active_stream_count(), 0);
        assert_eq!(before.retired_lease_guards(), 10_000);
        assert_eq!(before.compacted_retirement_ranges(), 0);
        assert_eq!(before.logical_compacted_leases(), 0);
        assert_eq!(before.retained_structure_count(), 10_000);
        assert_eq!(
            before.terminal_release_barrier(),
            Some(crate::retention::RuntimeRetentionReleaseBarrier::PointerIngressQuiesced),
        );

        let first = producers[0].0;
        for (_, recorder) in producers {
            let receipt = recorder.drain();
            ledger
                .compact_quiesced_backend(&receipt)
                .expect("a joined producer releases its detailed tombstone");
        }

        let after = ledger.retention_manifest();
        assert_eq!(after.retired_lease_guards(), 0);
        assert_eq!(after.compacted_retirement_ranges(), 1);
        assert_eq!(after.logical_compacted_leases(), 10_000);
        assert_eq!(after.retained_structure_count(), 1);
        assert_eq!(after.terminal_release_barrier(), None);
        assert!(matches!(
            ledger.prepare_candidate(first, moved_journal(0, 0)),
            Err(PointerJournalLedgerError::CompactedLease { lease }) if lease == first
        ));
    }

    #[test]
    fn retention_accounts_for_the_live_provider_and_device_stream_index() {
        let mut ledger = PointerJournalLedger::new(domain(1));
        let lease = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(0),
            )
            .expect("provider");
        commit_candidate(&mut ledger, lease, moved_journal(0, 1))
            .expect("one pointer stream must be accepted");

        let retention = ledger.retention_manifest();
        assert_eq!(retention.active_provider_count(), 1);
        assert_eq!(retention.active_stream_count(), 1);
        assert_eq!(retention.retired_lease_guards(), 0);
        assert_eq!(retention.compacted_retirement_ranges(), 0);
        assert_eq!(retention.logical_compacted_leases(), 0);
        assert_eq!(retention.retained_structure_count(), 2);
        assert_eq!(retention.terminal_release_barrier(), None);
    }

    #[test]
    fn quiesced_compaction_version_exhaustion_preserves_the_detailed_tombstone() {
        let domain = domain(1);
        let mut platform_authority = PlatformObservationAuthority::new(domain);
        let platform = platform_authority
            .create()
            .expect("test platform provider must mint");
        let presentation_host = host(domain);
        let mut ledger = PointerJournalLedger::new(domain);
        let lease = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(7),
            )
            .expect("provider incarnation must remain available");
        ledger
            .retire_provider(lease)
            .expect("the active provider must retire exactly once");
        let receipt = BackendIngressRecorder::new(
            platform,
            lease,
            presentation_host,
            PointerEdgeSequence::new(7),
        )
        .expect("the joined producer must bind the exact pointer lease")
        .drain();
        ledger.version = PointerJournalLedgerVersion(u64::MAX);
        let before = ledger.clone();

        assert_eq!(
            ledger.compact_quiesced_backend(&receipt),
            Err(PointerJournalLedgerError::LedgerVersionExhausted)
        );
        assert_eq!(ledger, before);
        assert_eq!(ledger.retention_manifest().retired_lease_guards(), 1);
        assert_eq!(ledger.retention_manifest().compacted_retirement_ranges(), 0);
    }

    #[test]
    fn retirement_rejections_preserve_the_active_lease_and_watermark() {
        let local_domain = domain(1);
        let mut ledger = PointerJournalLedger::new(local_domain);
        let first = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(5),
            )
            .expect("first provider");
        commit_candidate(&mut ledger, first, moved_journal(5, 7)).expect("advance first watermark");
        let expected_first = ActivePointerProvider {
            lease: first,
            committed_through: PointerEdgeSequence::new(7),
        };
        let foreign = desktop_lease(domain(2), first.incarnation());
        let unknown = desktop_lease(local_domain, 99);

        assert_eq!(
            ledger.retire_provider(foreign),
            Err(PointerJournalLedgerError::ForeignLease {
                expected: local_domain,
                submitted: domain(2),
            })
        );
        assert_eq!(ledger.active, Some(expected_first));
        assert!(ledger.retired.is_empty());

        assert_eq!(
            ledger.retire_provider(unknown),
            Err(PointerJournalLedgerError::UnknownLease { lease: unknown })
        );
        assert_eq!(ledger.active, Some(expected_first));
        assert!(ledger.retired.is_empty());

        ledger
            .retire_provider(first)
            .expect("retire exact first lease");
        let successor = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(20),
            )
            .expect("successor provider");
        commit_candidate(&mut ledger, successor, moved_journal(20, 21))
            .expect("advance successor watermark");
        let expected_successor = ActivePointerProvider {
            lease: successor,
            committed_through: PointerEdgeSequence::new(21),
        };

        assert_eq!(
            ledger.retire_provider(first),
            Err(PointerJournalLedgerError::RetiredLease {
                lease: first,
                committed_through: PointerEdgeSequence::new(7),
            })
        );
        assert_eq!(ledger.active, Some(expected_successor));
        assert_eq!(ledger.retired.len(), 1);
        assert_eq!(
            ledger.retired.get(&first),
            Some(&RetiredPointerInputLease {
                lease: first,
                committed_through: PointerEdgeSequence::new(7),
            })
        );
    }

    #[test]
    fn successor_incarnation_cannot_collide_with_old_tickets_or_streams() {
        let mut ledger = PointerJournalLedger::new(domain(1));
        let first_lease = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(0),
            )
            .expect("first provider");
        let first_commit =
            commit_candidate(&mut ledger, first_lease, moved_journal(0, 1)).expect("first journal");
        let first_ticket = first_commit.accepted_edges()[0].ticket();
        let pointer = PointerId::new(7);
        let first_stream = first_commit.accepted_edges()[0].stream();
        ledger
            .retire_provider(first_lease)
            .expect("retire first provider");

        let next_lease = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(0),
            )
            .expect("successor provider");
        let next_commit = commit_candidate(&mut ledger, next_lease, moved_journal(0, 1))
            .expect("successor journal");
        let next_ticket = next_commit.accepted_edges()[0].ticket();
        let next_stream = next_commit.accepted_edges()[0].stream();

        assert_eq!(first_ticket.sequence(), next_ticket.sequence());
        assert_ne!(first_ticket.lease(), next_ticket.lease());
        assert_ne!(first_ticket, next_ticket);
        assert_eq!(first_stream.pointer(), pointer);
        assert_eq!(first_stream.lease(), first_lease);
        assert_eq!(first_stream.incarnation(), 1);
        assert_eq!(next_stream.incarnation(), 2);
        assert_ne!(first_stream, next_stream);
    }

    #[test]
    fn provider_incarnation_counter_never_wraps() {
        let mut ledger = PointerJournalLedger::new(domain(1));
        ledger.last_incarnation = PointerProviderIncarnation(u64::MAX);

        assert_eq!(
            ledger.create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(0),
            ),
            Err(PointerJournalLedgerError::ProviderIncarnationExhausted)
        );
        assert!(ledger.active.is_none());
        assert_eq!(
            ledger.last_incarnation,
            PointerProviderIncarnation(u64::MAX)
        );
    }

    #[test]
    fn stream_incarnation_exhaustion_is_atomic() {
        let mut ledger = PointerJournalLedger::new(domain(1));
        let lease = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(0),
            )
            .expect("provider");
        ledger.last_stream_incarnation = PointerStreamIncarnation(u64::MAX);
        let before_version = ledger.version;
        let prepared = ledger
            .prepare_candidate(lease, moved_journal(0, 1))
            .expect("journal is valid before stream identity planning");

        assert_eq!(
            ledger.commit_prepared(prepared),
            Err(PointerJournalLedgerError::StreamIncarnationExhausted)
        );
        assert_eq!(ledger.version, before_version);
        assert_eq!(
            ledger.active,
            Some(ActivePointerProvider {
                lease,
                committed_through: PointerEdgeSequence::new(0),
            })
        );
        assert!(ledger.active_streams.is_empty());
        assert_eq!(
            ledger.last_stream_incarnation,
            PointerStreamIncarnation(u64::MAX)
        );
    }

    #[test]
    fn empty_candidate_succeeds_without_fabricating_an_edge_ticket() {
        let mut ledger = PointerJournalLedger::new(domain(1));
        let lease = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(5),
            )
            .expect("provider");

        let empty = commit_candidate(&mut ledger, lease, moved_journal(5, 5))
            .expect("authoritative empty interval");
        assert!(empty.journal().is_empty());
        assert!(empty.accepted_edges().is_empty());

        let next = commit_candidate(&mut ledger, lease, moved_journal(5, 6))
            .expect("empty interval left watermark at five");
        assert_eq!(next.accepted_edges().len(), 1);
        assert_eq!(
            next.accepted_edges()[0].ticket().sequence(),
            PointerEdgeSequence::new(6)
        );
    }

    #[test]
    fn desktop_dock_route_uses_the_target_binding_generation_and_scale_once() {
        let domain = domain(7);
        let surface = SurfaceId::new(91);
        let (registry, binding, generation) =
            routeable_registry(domain, surface, WindowToken::new(901), 1_000.0, 2.0);
        let desktop = PhysicalPoint::new(1_200.0, 150.0).expect("finite desktop point");
        let surface_point = LogicalPoint::new(100.0, 75.0).expect("finite surface point");
        let fact = DesktopRouteFact::dock(DesktopDockRoute::new(
            binding,
            generation,
            desktop,
            surface_point,
        ));

        assert_eq!(
            fact.validate_against_registry(domain, &registry),
            DesktopRouteValidation::Known(ValidatedDesktopRoute::Dock(
                fact.dock_route()
                    .expect("dock fact retains its exact route")
            ))
        );
        assert_eq!(fact.position(), Authority::Known(desktop));
        assert_eq!(
            fact.hovered(),
            Authority::Known(DesktopHoveredWindow::Dock(binding))
        );
    }

    #[test]
    fn core_derived_desktop_route_uses_current_registry_coordinates() {
        let domain = domain(71);
        let surface = SurfaceId::new(911);
        let (registry, binding, generation) =
            routeable_registry(domain, surface, WindowToken::new(9_011), 1_000.0, 2.0);
        let desktop = PhysicalPoint::new(1_200.0, 150.0).expect("finite desktop point");
        let expected = DesktopDockRoute::new(
            binding,
            generation,
            desktop,
            LogicalPoint::new(100.0, 75.0).expect("finite surface point"),
        );
        let fact = DesktopRouteFact::dock_from_desktop_position(binding, Authority::Known(desktop));

        assert_eq!(fact.dock_route(), None);
        assert_eq!(
            fact.validate_against_registry(domain, &registry),
            DesktopRouteValidation::Known(ValidatedDesktopRoute::Dock(expected))
        );
    }

    #[test]
    fn core_derived_desktop_route_fails_closed_without_physical_position() {
        let domain = domain(72);
        let surface = SurfaceId::new(912);
        let (registry, binding, _) =
            routeable_registry(domain, surface, WindowToken::new(9_012), 0.0, 1.0);
        let fact = DesktopRouteFact::dock_from_desktop_position(
            binding,
            Authority::Unknown(AuthorityUnavailableReason::CoordinateUnavailable),
        );

        assert_eq!(
            fact.validate_against_registry(domain, &registry),
            DesktopRouteValidation::Unavailable(DesktopRouteUnavailable::DesktopPositionUnknown {
                binding,
                reason: AuthorityUnavailableReason::CoordinateUnavailable,
            })
        );
    }

    #[test]
    fn desktop_unknown_foreign_and_no_window_remain_distinct() {
        let domain = domain(8);
        let registry = ViewportRegistry::new(domain);
        let position = PhysicalPoint::new(12.0, 34.0).expect("finite desktop point");

        assert_eq!(
            DesktopRouteFact::unknown(
                Authority::Known(position),
                AuthorityUnavailableReason::NotReported,
            )
            .validate_against_registry(domain, &registry),
            DesktopRouteValidation::Unavailable(DesktopRouteUnavailable::HoverUnknown {
                position: Authority::Known(position),
                reason: AuthorityUnavailableReason::NotReported,
            })
        );
        assert_eq!(
            DesktopRouteFact::foreign(Authority::Known(position))
                .validate_against_registry(domain, &registry),
            DesktopRouteValidation::Known(ValidatedDesktopRoute::Foreign {
                position: Authority::Known(position),
            })
        );
        assert_eq!(
            DesktopRouteFact::no_window(
                Authority::Known(position),
                Authority::Unknown(AuthorityUnavailableReason::NotReported),
            )
            .validate_against_registry(domain, &registry),
            DesktopRouteValidation::Known(ValidatedDesktopRoute::NoWindow {
                position: Authority::Known(position),
                work_area: Authority::Unknown(AuthorityUnavailableReason::NotReported),
            })
        );
    }

    #[test]
    fn known_dock_hover_without_a_coordinate_route_cannot_become_a_local_target() {
        let domain = domain(81);
        let surface = SurfaceId::new(811);
        let (registry, binding, _) =
            routeable_registry(domain, surface, WindowToken::new(8_110), 0.0, 1.0);
        let position = PhysicalPoint::new(12.0, 34.0).expect("finite desktop point");
        let fact = DesktopRouteFact::dock_without_coordinate_route(
            binding,
            Authority::Known(position),
            AuthorityUnavailableReason::CoordinateUnavailable,
        );

        assert_eq!(
            fact.hovered(),
            Authority::Known(DesktopHoveredWindow::Dock(binding))
        );
        assert_eq!(
            fact.dock_route_authority(),
            Some(Authority::Unknown(
                AuthorityUnavailableReason::CoordinateUnavailable
            ))
        );
        assert_eq!(fact.dock_route(), None);
        assert_eq!(
            fact.validate_against_registry(domain, &registry),
            DesktopRouteValidation::Unavailable(DesktopRouteUnavailable::CoordinateRouteUnknown {
                binding,
                position: Authority::Known(position),
                reason: AuthorityUnavailableReason::CoordinateUnavailable,
            })
        );
    }

    #[test]
    fn desktop_route_rejects_a_reused_window_token_with_an_old_incarnation() {
        let domain = domain(9);
        let surface = SurfaceId::new(92);
        let (registry, current, _) =
            routeable_registry(domain, surface, WindowToken::new(902), 0.0, 1.0);
        let stale = ViewportBinding::new(
            domain,
            current.epoch(),
            surface,
            current.token(),
            WindowIncarnation::new(
                current
                    .incarnation()
                    .get()
                    .checked_add(1)
                    .expect("test incarnation has a successor"),
            ),
        );
        let desktop = PhysicalPoint::new(64.0, 48.0).expect("finite desktop point");
        let fact = DesktopRouteFact::dock_from_desktop_position(stale, Authority::Known(desktop));

        assert_eq!(
            fact.validate_against_registry(domain, &registry),
            DesktopRouteValidation::Unavailable(DesktopRouteUnavailable::StaleBinding {
                observed: stale,
                current,
            })
        );
    }

    #[test]
    fn desktop_route_rejects_an_adapter_logical_point_that_disagrees_with_coordinates() {
        let domain = domain(10);
        let surface = SurfaceId::new(93);
        let (registry, binding, generation) =
            routeable_registry(domain, surface, WindowToken::new(903), 1_000.0, 2.0);
        let desktop = PhysicalPoint::new(1_200.0, 150.0).expect("finite desktop point");
        let submitted = LogicalPoint::new(101.0, 75.0).expect("finite surface point");
        let expected = LogicalPoint::new(100.0, 75.0).expect("finite surface point");
        let fact = DesktopRouteFact::dock(DesktopDockRoute::new(
            binding, generation, desktop, submitted,
        ));

        assert_eq!(
            fact.validate_against_registry(domain, &registry),
            DesktopRouteValidation::Unavailable(DesktopRouteUnavailable::CoordinatePointMismatch {
                binding,
                physical: desktop,
                expected,
                submitted,
            })
        );
    }
}

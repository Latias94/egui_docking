use super::*;
use crate::presentation_observation::PresentedSurfaceAuthority;
use crate::viewport_registry::ViewportRegistry;

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

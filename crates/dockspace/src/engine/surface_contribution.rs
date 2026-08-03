//! Measurement, compilation, and preparation of one surface contribution.

use super::*;

/// Core-minted capture of one surface measurement callback's exact base facts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceContributionToken {
    base: SurfaceSceneStamp,
    pub(super) coordinates: SurfaceCoordinateCapture,
}

impl SurfaceContributionToken {
    /// Returns the exact presentation authority observed before measurement.
    #[must_use]
    pub const fn base(self) -> SurfaceSceneStamp {
        self.base
    }

    /// Returns the exact requirement the adapter must answer.
    #[must_use]
    pub const fn ticket(self) -> SurfaceMeasurementTicket {
        self.base.requirement()
    }

    /// Returns the sole logical surface this token can contribute.
    #[must_use]
    pub const fn surface(self) -> crate::ids::SurfaceId {
        self.base.surface()
    }
}

/// Paint result of a core-prepared surface contribution.
#[derive(Debug)]
pub enum PreparedSurfacePaintCandidate<'a> {
    /// A complete, validated candidate available for adapter resource preparation.
    Ready(PreparedReadySurfacePaintCandidate<'a>),
    /// The host explicitly retained the current exact ready candidate.
    ///
    /// This does not carry a replacement plan. The adapter must continue to
    /// paint the current core projection identified by `ticket`; a later exact
    /// presentation observation remains the only way to grant hit
    /// authority.
    Retained {
        /// Exact current ready candidate retained by this contribution.
        ticket: SurfacePresentationOutputTicket,
    },
    /// Exact measurements were accepted but cannot currently authorize a ready plan.
    Unavailable {
        /// Explicit non-authoritative reason retained for reduction.
        reason: SurfaceContributionUnavailableReason,
    },
}

/// Read-only capability tied to one exact prepared Ready candidate.
#[derive(Debug)]
pub struct PreparedReadySurfacePaintCandidate<'a> {
    plan: &'a PresentationPlan,
}

impl PreparedReadySurfacePaintCandidate<'_> {
    /// Returns the exact core-compiled plan covered by this capability.
    #[must_use]
    pub const fn plan(&self) -> &PresentationPlan {
        self.plan
    }
}

#[derive(Debug, PartialEq)]
pub(super) enum PreparedSurfaceContributionState {
    Ready {
        plan: PresentationPlan,
    },
    Retained {
        ticket: SurfacePresentationOutputTicket,
    },
    Unavailable(SurfaceContributionUnavailableReason),
}

/// One core-compiled surface contribution awaiting an atomic reducer boundary.
///
/// Private fields make this a capability minted only by
/// [`DockEngine::prepare_surface_contribution`]. Adapters may inspect the exact
/// paint candidate, but cannot replace the plan or its frozen authority.
#[derive(Debug, PartialEq)]
pub struct PreparedSurfaceContribution {
    pub(super) token: SurfaceContributionToken,
    pub(super) policy_revision: PolicyRevision,
    pub(super) state: PreparedSurfaceContributionState,
}

impl PreparedSurfaceContribution {
    /// Returns the core-minted callback token frozen by preparation.
    #[must_use]
    pub const fn token(&self) -> SurfaceContributionToken {
        self.token
    }

    /// Returns the sole logical surface this prepared contribution can update.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.token.surface()
    }

    /// Borrows the exact candidate for adapter resource preparation.
    ///
    /// A contribution never proves presentation and cannot grant hit authority. The host must
    /// later submit an exact presentation observation after presentation of a final
    /// host pass has been observed.
    #[must_use]
    pub fn paint_candidate(&self) -> PreparedSurfacePaintCandidate<'_> {
        match &self.state {
            PreparedSurfaceContributionState::Ready { plan } => {
                PreparedSurfacePaintCandidate::Ready(PreparedReadySurfacePaintCandidate { plan })
            }
            PreparedSurfaceContributionState::Retained { ticket } => {
                PreparedSurfacePaintCandidate::Retained { ticket: *ticket }
            }
            PreparedSurfaceContributionState::Unavailable(reason) => {
                PreparedSurfacePaintCandidate::Unavailable { reason: *reason }
            }
        }
    }
}

/// Failure to begin measurement for one logical surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SurfaceContributionBeginError {
    /// The surface is outside the current presentation roster.
    #[error("surface {surface} is outside the current presentation roster")]
    SurfaceOutsideRoster {
        /// Requested logical surface.
        surface: crate::ids::SurfaceId,
    },
}

/// Failure to turn adapter measurements into a core-owned contribution.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum SurfaceContributionPrepareError {
    /// The token's exact base authority was superseded before preparation.
    #[error("surface {surface} contribution base {submitted:?} is stale; current is {current:?}")]
    StaleBase {
        /// Surface frozen by the token.
        surface: SurfaceId,
        /// Authority frozen before measurement.
        submitted: SurfaceSceneStamp,
        /// Current authority, or `None` after roster removal.
        current: Option<SurfaceSceneStamp>,
    },
    /// Measurements did not echo the core-minted requirement ticket.
    #[error("surface {surface} measurements carry ticket {actual:?}, expected {expected:?}")]
    TicketMismatch {
        /// Surface frozen by the token.
        surface: SurfaceId,
        /// Ticket frozen by the token.
        expected: SurfaceMeasurementTicket,
        /// Ticket carried by the measurements.
        actual: SurfaceMeasurementTicket,
    },
    /// Policy authority changed before preparation.
    #[error("surface {surface} contribution policy {submitted:?} is stale; current is {current:?}")]
    PolicyAuthorityChanged {
        /// Surface frozen by the token.
        surface: SurfaceId,
        /// Policy authority frozen in the requirement ticket.
        submitted: PolicyRevision,
        /// Current policy authority.
        current: PolicyRevision,
    },
    /// Binding, lifecycle, or coordinates changed before preparation.
    #[error("surface {surface} coordinate authority changed before preparation")]
    CoordinateAuthorityChanged {
        /// Surface whose callback facts are late.
        surface: SurfaceId,
    },
    /// The exact surface state has no current Ready candidate to retain.
    #[error("surface {surface} has no ready presentation candidate to retain")]
    RetainedCandidateUnavailable {
        /// Surface whose non-emitting retention was requested.
        surface: SurfaceId,
    },
    /// Exact-set validation or semantic compilation rejected the measurements.
    #[error("surface contribution compilation failed: {0}")]
    Compilation(#[source] PresentationCompilationError),
    /// The core compiler produced a plan that failed final validation.
    #[error("surface contribution validation failed: {0}")]
    Validation(#[source] SceneBuildError),
}

impl DockEngine {
    /// Begins one surface measurement callback against exact current authority.
    ///
    /// The token freezes the entry stamp, requirement ticket, and native
    /// coordinate association. It remains deliberately usable after this method
    /// returns so a late callback can be deterministically rejected by
    /// [`Self::prepare_surface_contribution`] without replacing newer Ready
    /// facts. Once prepared, the contribution retains the same authority for
    /// final validation by [`CoreHostFrame::finish`].
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceContributionBeginError`] outside the current roster.
    pub fn begin_surface_contribution(
        &self,
        surface: crate::ids::SurfaceId,
    ) -> Result<SurfaceContributionToken, SurfaceContributionBeginError> {
        let state = self
            .presentation_authority
            .scene
            .surface(surface)
            .ok_or(SurfaceContributionBeginError::SurfaceOutsideRoster { surface })?;
        Ok(SurfaceContributionToken {
            base: state.stamp(),
            coordinates: self.capture_surface_coordinates(surface),
        })
    }

    /// Compiles and validates one exact measurement answer without mutating the engine.
    ///
    /// The returned capability contains either the exact plan an adapter may
    /// paint or an explicit unavailable result. Structural, exact-set, ticket,
    /// and final-plan failures are returned here rather than being deferred to
    /// reducer outcomes.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceContributionPrepareError`] when the token is already
    /// stale, no longer has coordinate or policy authority, the measurements do
    /// not answer its exact ticket, or compilation/final validation fails.
    pub fn prepare_surface_contribution(
        &self,
        token: SurfaceContributionToken,
        measurements: SurfaceMeasurements,
    ) -> Result<PreparedSurfaceContribution, SurfaceContributionPrepareError> {
        let surface = token.surface();
        let current = self
            .presentation_authority
            .scene
            .surface(surface)
            .map(SurfaceScene::stamp);
        if current != Some(token.base()) {
            return Err(SurfaceContributionPrepareError::StaleBase {
                surface,
                submitted: token.base(),
                current,
            });
        }
        if measurements.ticket() != token.ticket() {
            return Err(SurfaceContributionPrepareError::TicketMismatch {
                surface,
                expected: token.ticket(),
                actual: measurements.ticket(),
            });
        }
        let policy_revision = self.policy.revision();
        if token.ticket().policy() != policy_revision {
            return Err(SurfaceContributionPrepareError::PolicyAuthorityChanged {
                surface,
                submitted: token.ticket().policy(),
                current: policy_revision,
            });
        }
        if !Self::coordinate_capture_matches_current(
            token.coordinates,
            self.viewport.viewport(surface),
            self.viewport.surface_coordinate_authority(surface),
        ) {
            return Err(SurfaceContributionPrepareError::CoordinateAuthorityChanged { surface });
        }

        let resize_overrides = self.surface_resize_overrides(surface);
        let state = match compile_surface_measurements(
            &self.workspace,
            self.version,
            &self.policy,
            &self.presentation_authority.presentation_config,
            &self.presentation_authority.presentation_requirements,
            &measurements,
            &self.presentation_authority.tab_strip_states,
            &resize_overrides,
        ) {
            Ok(plan) => {
                let validator = PresentationPlanValidator::new(&self.workspace, &self.policy)
                    .map_err(SurfaceContributionPrepareError::Validation)?;
                let plan = validator
                    .validate_and_canonicalize(plan)
                    .map_err(SurfaceContributionPrepareError::Validation)?;
                if matches!(
                    token.coordinates,
                    SurfaceCoordinateCapture::NativeUnavailable { .. }
                ) {
                    PreparedSurfaceContributionState::Unavailable(
                        SurfaceContributionUnavailableReason::CoordinateAuthorityUnavailable,
                    )
                } else {
                    PreparedSurfaceContributionState::Ready { plan }
                }
            }
            Err(PresentationCompilationError::Authority(authority)) => {
                PreparedSurfaceContributionState::Unavailable(
                    SurfaceContributionUnavailableReason::MeasurementsUnavailable(authority),
                )
            }
            Err(PresentationCompilationError::Scene(
                crate::scene_compiler::SceneCompilationError::EmptySurfaceBounds { .. },
            )) => PreparedSurfaceContributionState::Unavailable(
                SurfaceContributionUnavailableReason::EmptyBounds,
            ),
            Err(error) => match self.popup_geometry_unavailable(surface, &error) {
                Some(reason) => PreparedSurfaceContributionState::Unavailable(reason),
                None => return Err(SurfaceContributionPrepareError::Compilation(error)),
            },
        };

        Ok(PreparedSurfaceContribution {
            token,
            policy_revision,
            state,
        })
    }

    fn popup_geometry_unavailable(
        &self,
        surface: SurfaceId,
        error: &PresentationCompilationError,
    ) -> Option<SurfaceContributionUnavailableReason> {
        let popup = self
            .presentation_authority
            .presentation_requirements
            .popup();
        let session = popup.session()?;
        let owner = popup.owner()?;
        let reason = match error {
            PresentationCompilationError::Scene(
                crate::scene_compiler::SceneCompilationError::EmptyPopupPlaneBounds {
                    surface: actual,
                },
            ) if *actual == surface => PopupGeometryUnavailableReason::EmptyPlane,
            PresentationCompilationError::Scene(
                crate::scene_compiler::SceneCompilationError::TabListMenuAnchorOutsidePopupPlane {
                    key,
                },
            ) if *key == owner && owner.surface() == surface => {
                PopupGeometryUnavailableReason::AnchorOutsidePlane
            }
            PresentationCompilationError::Scene(
                crate::scene_compiler::SceneCompilationError::TabListMenuPopupSpaceUnavailable {
                    key,
                },
            ) if *key == owner && owner.surface() == surface => {
                PopupGeometryUnavailableReason::NoSpace
            }
            PresentationCompilationError::Scene(
                crate::scene_compiler::SceneCompilationError::ActiveTabListMenuProjectionUnavailable {
                    session: actual,
                },
            ) if *actual == session && owner.surface() == surface => {
                PopupGeometryUnavailableReason::ProjectionUnavailable
            }
            _ => return None,
        };
        Some(SurfaceContributionUnavailableReason::PopupGeometryUnavailable { session, reason })
    }

    /// Prepares one exact, explicit unavailable answer for a rostered surface.
    ///
    /// A host frame cannot omit a surface because one callback did not run.
    /// Hosts that cannot measure a core-frozen surface must submit this
    /// contribution instead, preserving the complete roster while keeping the
    /// resulting scene non-authoritative.
    ///
    /// # Errors
    ///
    /// Returns the same stale-authority errors as
    /// [`Self::prepare_surface_contribution`] when `token` no longer names the
    /// current exact surface authority.
    pub fn prepare_surface_unavailable_contribution(
        &self,
        token: SurfaceContributionToken,
        reason: MeasurementUnavailableReason,
    ) -> Result<PreparedSurfaceContribution, SurfaceContributionPrepareError> {
        let surface = token.surface();
        let current = self
            .presentation_authority
            .scene
            .surface(surface)
            .map(SurfaceScene::stamp);
        if current != Some(token.base()) {
            return Err(SurfaceContributionPrepareError::StaleBase {
                surface,
                submitted: token.base(),
                current,
            });
        }
        let Some(requirements) = self
            .presentation_authority
            .presentation_requirements
            .surface(surface)
        else {
            return Err(SurfaceContributionPrepareError::StaleBase {
                surface,
                submitted: token.base(),
                current: None,
            });
        };
        self.prepare_surface_contribution(
            token,
            SurfaceMeasurements::unavailable(requirements, reason),
        )
    }

    /// Prepares a non-emitting contribution which retains the exact current
    /// Ready candidate.
    ///
    /// This is appropriate for host boundaries that process input but do not
    /// perform a paint pass. Unlike recording a painted surface contribution,
    /// this capability does not stage a concrete presentation output and
    /// therefore cannot take stream ownership or grant interaction authority.
    ///
    /// # Errors
    ///
    /// Returns a typed stale, policy, coordinate, or candidate error when the
    /// supplied token no longer identifies the exact current Ready candidate.
    pub fn prepare_surface_retained_contribution(
        &self,
        token: SurfaceContributionToken,
    ) -> Result<PreparedSurfaceContribution, SurfaceContributionPrepareError> {
        let surface = token.surface();
        let current = self
            .presentation_authority
            .scene
            .surface(surface)
            .map(SurfaceScene::stamp);
        if current != Some(token.base()) {
            return Err(SurfaceContributionPrepareError::StaleBase {
                surface,
                submitted: token.base(),
                current,
            });
        }
        let policy_revision = self.policy.revision();
        if token.ticket().policy() != policy_revision {
            return Err(SurfaceContributionPrepareError::PolicyAuthorityChanged {
                surface,
                submitted: token.ticket().policy(),
                current: policy_revision,
            });
        }
        if !Self::coordinate_capture_matches_current(
            token.coordinates,
            self.viewport.viewport(surface),
            self.viewport.surface_coordinate_authority(surface),
        ) {
            return Err(SurfaceContributionPrepareError::CoordinateAuthorityChanged { surface });
        }
        let ticket = match self.presentation_authority.scene.surface(surface) {
            Some(SurfaceScene::Ready(ready)) => ready.output_ticket(),
            Some(SurfaceScene::Stale(_)) | Some(SurfaceScene::Bootstrap(_)) | None => {
                return Err(
                    SurfaceContributionPrepareError::RetainedCandidateUnavailable { surface },
                );
            }
        };
        Ok(PreparedSurfaceContribution {
            token,
            policy_revision,
            state: PreparedSurfaceContributionState::Retained { ticket },
        })
    }

    fn surface_resize_overrides(
        &self,
        surface: SurfaceId,
    ) -> Vec<crate::layout::SplitWeightOverride<'_>> {
        self.interaction
            .resize_overrides()
            .map(|(_, updates)| {
                updates
                    .iter()
                    .filter(|update| {
                        matches!(
                            self.workspace.presentation_for_root(update.split().root()),
                            Some(crate::RootPresentationOwner::Main { surface: owner })
                                | Some(crate::RootPresentationOwner::Contained {
                                    surface: owner,
                                    ..
                                }) if owner == surface
                        )
                    })
                    .map(|update| {
                        crate::layout::SplitWeightOverride::new(update.split(), update.weights())
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

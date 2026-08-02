//! Core-derived physical presentation roster for one host-frame boundary.

use std::sync::atomic::{AtomicU64, Ordering};

use super::*;

/// One exact physical output slot frozen by the core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HostPresentationSlot {
    /// A live semantic docking surface.
    Surface {
        /// Logical surface whose exact output is requested.
        surface: SurfaceId,
    },
    /// A non-interactive native-create staging placeholder.
    NativeStaging {
        /// Exact lifecycle request which must be painted without invoking pane UI.
        presentation: NativeStagingPresentation,
    },
}

impl HostPresentationSlot {
    /// Returns the physical surface occupied by this slot.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        match self {
            Self::Surface { surface } => surface,
            Self::NativeStaging { presentation } => presentation.binding().surface(),
        }
    }

    /// Returns the native staging descriptor when this is a staging slot.
    #[must_use]
    pub const fn native_staging(self) -> Option<NativeStagingPresentation> {
        match self {
            Self::Surface { .. } => None,
            Self::NativeStaging { presentation } => Some(presentation),
        }
    }
}

/// Explicit reason why one frozen physical slot produced no authoritative output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HostPresentationUnavailableReason {
    /// The host can paint but cannot observe final renderer presentation.
    FinalPresentationUnobservable,
    /// The physical surface did not produce an output in this host frame.
    OutputNotProduced,
    /// A causally newer projection superseded the output before publication.
    SupersededBeforePublication,
    /// A retained staging resource was unavailable to the host renderer.
    RetainedResourceUnavailable,
    /// The base surface painted, but one frozen transient interaction visual did not.
    TransientVisualNotPainted,
    /// The renderer or platform backend definitively failed this output.
    BackendFailure,
}

/// Exact disposition of one core-issued presentation obligation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostPresentationDisposition {
    /// The host actually painted this exact frozen output and these transient visuals.
    Painted(HostInteractionPresentation),
    /// The host explicitly reports that no authoritative output was produced.
    Unavailable(HostPresentationUnavailableReason),
}

/// Committed disposition of one exact host-frame physical output slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostPresentationDispositionOutcome {
    slot: HostPresentationSlot,
    disposition: HostPresentationDisposition,
}

impl HostPresentationDispositionOutcome {
    const fn new(slot: HostPresentationSlot, disposition: HostPresentationDisposition) -> Self {
        Self { slot, disposition }
    }

    /// Returns the exact physical slot answered by the host.
    #[must_use]
    pub const fn slot(self) -> HostPresentationSlot {
        self.slot
    }

    /// Returns whether that slot was painted or explicitly unavailable.
    #[must_use]
    pub const fn disposition(self) -> HostPresentationDisposition {
        self.disposition
    }
}

/// Affine authority to answer one exact physical presentation slot.
///
/// This type intentionally implements neither `Clone` nor `Copy`. Its fields
/// are private, so only the core can mint tickets and each ticket can be
/// consumed by one disposition call.
#[derive(Debug, PartialEq, Eq)]
pub struct HostPresentationObligation {
    attempt: HostPresentationAttemptId,
    slot: HostPresentationSlot,
    ordinal: u64,
}

impl HostPresentationObligation {
    /// Returns the physical slot this ticket must answer.
    #[must_use]
    pub const fn slot(&self) -> HostPresentationSlot {
        self.slot
    }

    /// Returns the non-replayable host-frame attempt identity.
    #[must_use]
    pub const fn attempt(&self) -> HostPresentationAttemptId {
        self.attempt
    }

    pub(super) const fn ordinal(&self) -> u64 {
        self.ordinal
    }
}

/// Non-rollbackable issuer shared by every speculative engine candidate.
#[derive(Debug)]
pub(super) struct HostPresentationObligationIssuer {
    authority_domain: EngineAuthorityDomainId,
    last_attempt: AtomicU64,
}

impl PartialEq for HostPresentationObligationIssuer {
    fn eq(&self, other: &Self) -> bool {
        self.authority_domain == other.authority_domain
            && self.last_attempt.load(Ordering::Acquire)
                == other.last_attempt.load(Ordering::Acquire)
    }
}

impl Eq for HostPresentationObligationIssuer {}

impl HostPresentationObligationIssuer {
    pub(super) const fn new(authority_domain: EngineAuthorityDomainId) -> Self {
        Self {
            authority_domain,
            last_attempt: AtomicU64::new(0),
        }
    }

    pub(super) fn issue(&self) -> Result<HostPresentationAttemptId, CoreHostFrameError> {
        let previous = self
            .last_attempt
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |last| {
                last.checked_add(1)
            })
            .map_err(|_| CoreHostFrameError::PresentationAttemptSpaceExhausted)?;
        let serial = previous
            .checked_add(1)
            .ok_or(CoreHostFrameError::PresentationAttemptSpaceExhausted)?;
        Ok(HostPresentationAttemptId::mint(
            self.authority_domain,
            serial,
        ))
    }
}

/// One frozen semantic surface output eligible for an exact host paint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct FrozenSurfacePresentationOutput {
    stamp: SurfaceSceneStamp,
    endpoint: HostPresentationEndpoint,
    coordinates: SurfaceCoordinateCapture,
    payload: HostPresentationOutputPayload,
}

impl FrozenSurfacePresentationOutput {
    pub(super) const fn stamp(self) -> SurfaceSceneStamp {
        self.stamp
    }

    pub(super) const fn endpoint(self) -> HostPresentationEndpoint {
        self.endpoint
    }

    pub(super) const fn coordinates(self) -> SurfaceCoordinateCapture {
        self.coordinates
    }

    pub(super) const fn payload(self) -> HostPresentationOutputPayload {
        self.payload
    }

    fn same_projection(self, other: Self) -> bool {
        self.stamp == other.stamp
            && self.endpoint == other.endpoint
            && self
                .coordinates
                .same_projection_authority(other.coordinates)
            && match (self.payload, other.payload) {
                (
                    HostPresentationOutputPayload::Paint { scene: left, .. },
                    HostPresentationOutputPayload::Paint { scene: right, .. },
                ) => left == right,
                (
                    HostPresentationOutputPayload::Bootstrap,
                    HostPresentationOutputPayload::Bootstrap,
                )
                | (
                    HostPresentationOutputPayload::Unavailable,
                    HostPresentationOutputPayload::Unavailable,
                ) => true,
                (
                    HostPresentationOutputPayload::NativeStaging { .. },
                    HostPresentationOutputPayload::NativeStaging { .. },
                ) => false,
                _ => false,
            }
    }
}

/// Exact physical output inventory derived from the semantic scene and native lifecycle.
///
/// A physical surface has exactly one slot at a boundary: either a live docking
/// surface or a non-interactive native staging placeholder. Staging never enters
/// the semantic measurement, hit-test, focus, or accessibility roster.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct HostPresentationRoster {
    surfaces: BTreeMap<SurfaceId, FrozenSurfacePresentationOutput>,
    native_staging: BTreeMap<SurfaceId, NativeStagingPresentation>,
    native_resources: BTreeMap<NativeStagingResourceId, NativeStagingResourceDescriptor>,
}

impl HostPresentationRoster {
    pub(super) fn capture(candidate: &DockEngine) -> Result<Self, EngineError> {
        let surfaces = candidate
            .presentation_authority
            .presentation_requirements
            .surfaces()
            .map(|(surface, _)| (surface, Self::freeze_surface(candidate, surface)))
            .collect::<BTreeMap<_, _>>();

        let native_resources = candidate
            .viewport
            .retained_native_staging_resources()
            .map(|descriptor| (descriptor.id(), descriptor.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut native_staging = BTreeMap::new();
        for presentation in candidate.viewport.native_staging_presentations() {
            let surface = presentation.binding().surface();
            if presentation.resource().saga() != presentation.saga()
                || !native_resources.contains_key(&presentation.resource())
            {
                return Err(EngineError::HostPresentationStagingResourceMissing {
                    resource: presentation.resource(),
                });
            }
            if surfaces.contains_key(&surface)
                || native_staging.insert(surface, presentation).is_some()
            {
                return Err(EngineError::HostPresentationRosterCollision { surface });
            }
        }

        Ok(Self {
            surfaces,
            native_staging,
            native_resources,
        })
    }

    pub(super) fn surfaces(&self) -> impl ExactSizeIterator<Item = SurfaceId> + '_ {
        self.surfaces.keys().copied()
    }

    pub(super) fn slots(&self) -> impl Iterator<Item = HostPresentationSlot> + '_ {
        let surfaces = self
            .surfaces
            .keys()
            .copied()
            .map(|surface| HostPresentationSlot::Surface { surface });
        let native_staging = self
            .native_staging
            .values()
            .copied()
            .map(|presentation| HostPresentationSlot::NativeStaging { presentation });
        surfaces.chain(native_staging)
    }

    pub(super) fn surface_scope(&self) -> BTreeSet<SurfaceId> {
        self.surfaces().collect()
    }

    pub(super) fn contains_surface(&self, surface: SurfaceId) -> bool {
        self.surfaces.contains_key(&surface)
    }

    pub(super) fn surface_output(
        &self,
        surface: SurfaceId,
    ) -> Option<FrozenSurfacePresentationOutput> {
        self.surfaces.get(&surface).copied()
    }

    pub(super) fn native_staging_presentations(
        &self,
    ) -> impl ExactSizeIterator<Item = NativeStagingPresentation> + '_ {
        self.native_staging.values().copied()
    }

    pub(super) fn retained_native_staging_resources(
        &self,
    ) -> impl ExactSizeIterator<Item = &NativeStagingResourceDescriptor> {
        self.native_resources.values()
    }

    pub(super) fn contains_native_staging(&self, presentation: NativeStagingPresentation) -> bool {
        self.native_staging
            .get(&presentation.binding().surface())
            .is_some_and(|current| *current == presentation)
    }

    fn frozen_output(&self, slot: HostPresentationSlot) -> Option<FrozenHostPresentationOutput> {
        match slot {
            HostPresentationSlot::Surface { surface } => self
                .surface_output(surface)
                .map(FrozenHostPresentationOutput::Surface),
            HostPresentationSlot::NativeStaging { presentation }
                if self.contains_native_staging(presentation) =>
            {
                Some(FrozenHostPresentationOutput::NativeStaging(presentation))
            }
            HostPresentationSlot::NativeStaging { .. } => None,
        }
    }

    pub(super) fn same_semantic_projection(&self, other: &Self) -> bool {
        let current = self.surface_scope();
        if current != other.surface_scope() {
            return false;
        }
        current.into_iter().all(|surface| {
            self.surface_output(surface)
                .zip(other.surface_output(surface))
                .is_some_and(|(left, right)| left.same_projection(right))
        })
    }

    fn freeze_surface(
        candidate: &DockEngine,
        surface: SurfaceId,
    ) -> FrozenSurfacePresentationOutput {
        let scene = candidate
            .presentation_authority
            .scene
            .surface(surface)
            .expect("presentation requirements and scene roster must agree");
        let projection = scene.paint_projection();
        let interaction = HostInteractionPresentation::new(
            candidate
                .presentation_preview()
                .filter(|preview| preview.visual().surface() == surface)
                .map(crate::interaction::InteractionPreview::token),
            candidate
                .presentation_contained_transform_preview()
                .filter(|preview| preview.surface() == surface)
                .map(|preview| preview.token()),
        );
        let (coordinates, payload) = match projection {
            Some(projection) => (
                projection.coordinate_capture(),
                HostPresentationOutputPayload::Paint {
                    scene: projection.output_ticket(),
                    interaction,
                },
            ),
            None => (
                candidate.capture_surface_coordinates(surface),
                HostPresentationOutputPayload::Bootstrap,
            ),
        };
        FrozenSurfacePresentationOutput {
            stamp: scene.stamp(),
            endpoint: presentation_endpoint_from_capture(coordinates),
            coordinates,
            payload,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum FrozenHostPresentationOutput {
    Surface(FrozenSurfacePresentationOutput),
    NativeStaging(NativeStagingPresentation),
}

impl FrozenHostPresentationOutput {
    pub(super) const fn interaction(self) -> HostInteractionPresentation {
        match self {
            Self::Surface(output) => match output.payload() {
                HostPresentationOutputPayload::Paint { interaction, .. } => interaction,
                HostPresentationOutputPayload::NativeStaging { .. }
                | HostPresentationOutputPayload::Bootstrap
                | HostPresentationOutputPayload::Unavailable => {
                    HostInteractionPresentation::new(None, None)
                }
            },
            Self::NativeStaging(_) => HostInteractionPresentation::new(None, None),
        }
    }

    pub(super) const fn into_parts(
        self,
    ) -> (
        SurfaceId,
        HostPresentationEndpoint,
        HostPresentationOutputPayload,
    ) {
        match self {
            Self::Surface(output) => {
                let surface = match output.payload() {
                    HostPresentationOutputPayload::Paint { scene, .. } => scene.surface(),
                    HostPresentationOutputPayload::Bootstrap
                    | HostPresentationOutputPayload::Unavailable => output.stamp().surface(),
                    HostPresentationOutputPayload::NativeStaging { presentation } => {
                        presentation.binding().surface()
                    }
                };
                (surface, output.endpoint(), output.payload())
            }
            Self::NativeStaging(presentation) => {
                let binding = presentation.binding();
                (
                    binding.surface(),
                    HostPresentationEndpoint::Native(binding),
                    HostPresentationOutputPayload::NativeStaging { presentation },
                )
            }
        }
    }
}

/// Exact-set ledger for one host frame's affine presentation tickets.
#[derive(Debug)]
pub(super) struct HostPresentationObligationSet {
    attempt: HostPresentationAttemptId,
    roster: HostPresentationRoster,
    issued: bool,
    resolved: BTreeMap<HostPresentationSlot, HostPresentationDisposition>,
}

impl HostPresentationObligationSet {
    pub(super) const fn new(
        attempt: HostPresentationAttemptId,
        roster: HostPresentationRoster,
    ) -> Self {
        Self {
            attempt,
            roster,
            issued: false,
            resolved: BTreeMap::new(),
        }
    }

    pub(super) const fn attempt(&self) -> HostPresentationAttemptId {
        self.attempt
    }

    pub(super) const fn is_issued(&self) -> bool {
        self.issued
    }

    pub(super) fn replace_unissued_roster(
        &mut self,
        roster: HostPresentationRoster,
    ) -> Result<(), CoreHostFrameError> {
        if self.issued || !self.resolved.is_empty() {
            return Err(CoreHostFrameError::PresentationObligationsAlreadyIssued);
        }
        self.roster = roster;
        Ok(())
    }

    pub(super) fn issue(&mut self) -> Result<Vec<HostPresentationObligation>, CoreHostFrameError> {
        if self.issued {
            return Err(CoreHostFrameError::PresentationObligationsAlreadyIssued);
        }
        self.issued = true;
        self.roster
            .slots()
            .enumerate()
            .map(|(ordinal, slot)| {
                let ordinal = u64::try_from(ordinal)
                    .map_err(|_| CoreHostFrameError::PresentationOutputRequestExhausted)?;
                Ok(HostPresentationObligation {
                    attempt: self.attempt,
                    slot,
                    ordinal,
                })
            })
            .collect()
    }

    pub(super) fn validate(
        &self,
        obligation: &HostPresentationObligation,
    ) -> Result<(), CoreHostFrameError> {
        if !self.issued {
            return Err(CoreHostFrameError::PresentationObligationsNotIssued);
        }
        if obligation.attempt != self.attempt {
            return Err(CoreHostFrameError::PresentationObligationAttemptMismatch {
                expected: self.attempt,
                submitted: obligation.attempt,
            });
        }
        if self.roster.frozen_output(obligation.slot).is_none() {
            return Err(CoreHostFrameError::PresentationObligationOutsideRoster {
                slot: obligation.slot,
            });
        }
        if self.resolved.contains_key(&obligation.slot) {
            return Err(CoreHostFrameError::PresentationObligationAlreadyResolved {
                slot: obligation.slot,
            });
        }
        Ok(())
    }

    pub(super) fn frozen_output(
        &self,
        obligation: &HostPresentationObligation,
    ) -> Result<FrozenHostPresentationOutput, CoreHostFrameError> {
        self.validate(obligation)?;
        self.roster.frozen_output(obligation.slot).ok_or(
            CoreHostFrameError::PresentationObligationOutsideRoster {
                slot: obligation.slot,
            },
        )
    }

    pub(super) fn frozen_surface_output(
        &self,
        obligation: &HostPresentationObligation,
    ) -> Result<FrozenSurfacePresentationOutput, CoreHostFrameError> {
        match self.frozen_output(obligation)? {
            FrozenHostPresentationOutput::Surface(output) => Ok(output),
            FrozenHostPresentationOutput::NativeStaging(_) => {
                Err(CoreHostFrameError::PresentationObligationSurfaceMismatch {
                    slot: obligation.slot,
                    surface: obligation.slot.surface(),
                })
            }
        }
    }

    pub(super) fn resolve(
        &mut self,
        obligation: HostPresentationObligation,
        disposition: HostPresentationDisposition,
    ) {
        let previous = self.resolved.insert(obligation.slot, disposition);
        debug_assert!(
            previous.is_none(),
            "validated presentation obligation resolves once"
        );
    }

    pub(super) fn resolve_all_unissued(
        &mut self,
        disposition: HostPresentationDisposition,
    ) -> Result<(), CoreHostFrameError> {
        if self.issued {
            return if self.completion().is_none() {
                Ok(())
            } else {
                Err(CoreHostFrameError::PresentationObligationsPartiallyResolved)
            };
        }

        self.issued = true;
        self.resolved
            .extend(self.roster.slots().map(|slot| (slot, disposition)));
        Ok(())
    }

    pub(super) fn completion(
        &self,
    ) -> Option<(Vec<HostPresentationSlot>, Vec<HostPresentationSlot>)> {
        let expected = self.roster.slots().collect::<BTreeSet<_>>();
        let resolved = self.resolved.keys().copied().collect::<BTreeSet<_>>();
        if expected == resolved {
            return None;
        }
        Some((
            expected.into_iter().collect(),
            resolved.into_iter().collect(),
        ))
    }

    pub(super) fn dispositions(&self) -> Vec<HostPresentationDispositionOutcome> {
        self.resolved
            .iter()
            .map(|(slot, disposition)| HostPresentationDispositionOutcome::new(*slot, *disposition))
            .collect()
    }
}

pub(super) fn presentation_endpoint_from_capture(
    capture: SurfaceCoordinateCapture,
) -> HostPresentationEndpoint {
    match capture {
        SurfaceCoordinateCapture::Headless { .. } => HostPresentationEndpoint::Headless,
        SurfaceCoordinateCapture::NativeUnavailable { binding, .. } => {
            HostPresentationEndpoint::Native(binding)
        }
        SurfaceCoordinateCapture::NativeReady { coordinates, .. } => {
            HostPresentationEndpoint::Native(coordinates.binding())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{PhysicalRect, ScaleFactor};
    use crate::ids::{SurfaceId, WorkspaceEpoch};
    use crate::platform::WindowCoordinateObservation;
    use crate::scene_manifest::SurfaceSceneRevision;
    use crate::viewport::{
        CoordinateGeneration, CoordinateObservationGeneration, WindowIncarnation, WindowToken,
    };

    fn frozen_output(
        observation_generation: u64,
        coordinate_generation: u64,
        origin_x: f64,
    ) -> FrozenSurfacePresentationOutput {
        let authority_domain = EngineAuthorityDomainId::new_for_test(1);
        let surface = SurfaceId::new(1);
        let binding = ViewportBinding::new(
            authority_domain,
            WorkspaceEpoch::new(0),
            surface,
            WindowToken::new(1),
            WindowIncarnation::new(1),
        );
        let rect =
            PhysicalRect::new(origin_x, 20.0, 800.0, 600.0).expect("test rectangle is valid");
        let scale = ScaleFactor::new(1.0).expect("test scale is valid");
        let observation = WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(observation_generation),
            Authority::Known(rect),
            Authority::Known(rect),
            Authority::Known(scale),
            Authority::Known(scale),
        );
        let coordinate_generation = CoordinateGeneration::new(coordinate_generation);
        let coordinates =
            CoordinateSnapshot::from_observation(binding, coordinate_generation, observation)
                .expect("test coordinate observation is authoritative");
        let ticket = SurfaceMeasurementTicket::new(
            authority_domain,
            WorkspaceEpoch::new(0),
            PresentationConfigRevision::new(0),
            PolicyRevision::new(0),
            SurfaceRequirementRevision::new(0),
            surface,
        );
        FrozenSurfacePresentationOutput {
            stamp: SurfaceSceneStamp::new(ticket, SurfaceSceneRevision::new(1)),
            endpoint: HostPresentationEndpoint::Native(binding),
            coordinates: SurfaceCoordinateCapture::NativeReady {
                coordinates,
                authority_generation: coordinate_generation,
            },
            payload: HostPresentationOutputPayload::Bootstrap,
        }
    }

    #[test]
    fn semantic_projection_ignores_fresh_observations_of_unchanged_coordinates() {
        let previous = frozen_output(1, 3, 10.0);
        let refreshed = frozen_output(2, 3, 10.0);
        assert!(previous.same_projection(refreshed));

        assert!(!previous.same_projection(frozen_output(2, 4, 10.0)));
        assert!(!previous.same_projection(frozen_output(2, 3, 11.0)));
    }
}

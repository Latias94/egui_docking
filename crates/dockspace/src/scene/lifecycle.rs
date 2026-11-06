//! Surface presentation lifecycle, authority, and retained paint state.

use super::*;
use crate::tab_strip::PopupRoutingRevision;

/// Exact authority of one independently replaceable surface presentation.
///
/// The requirement ticket binds semantic topology, policy-facing presentation
/// configuration, and the logical surface. The revision binds the current
/// Ready/Stale/Bootstrap authority state. Deliberately omitting ordering keeps
/// callers from treating unrelated surface histories as one global timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceSceneStamp {
    requirement: SurfaceMeasurementTicket,
    revision: SurfaceSceneRevision,
}

impl SurfaceSceneStamp {
    pub(crate) const fn new(
        requirement: SurfaceMeasurementTicket,
        revision: SurfaceSceneRevision,
    ) -> Self {
        Self {
            requirement,
            revision,
        }
    }

    /// Returns the exact measurement requirement represented by this authority.
    #[must_use]
    pub const fn requirement(self) -> SurfaceMeasurementTicket {
        self.requirement
    }

    /// Returns this surface's engine-local authority revision.
    #[must_use]
    pub const fn revision(self) -> SurfaceSceneRevision {
        self.revision
    }

    /// Returns the sole surface whose presentation this stamp can authorize.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.requirement.surface()
    }
}

/// Why a paintable fallback currently lacks hit-test authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaleSurfaceSceneReason {
    /// Core-derived semantic requirements changed.
    RequirementsChanged,
    /// Binding, lifecycle, coordinate generation, bounds, or scale changed.
    CoordinateAuthorityChanged,
    /// The surface has a stable native association which is not ready to authorize geometry.
    CoordinateAuthorityUnavailable,
    /// The exact adapter contribution explicitly lacked one required fact.
    MeasurementsUnavailable(MeasurementAuthorityError),
    /// Exact host geometry could not project the active popup without violating its plane.
    PopupGeometryUnavailable(PopupGeometryUnavailableReason),
    /// Core-owned transient presentation input changed after the last projection.
    TransientPresentationChanged,
}

/// Why a rostered surface has neither hit-test authority nor a paint fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapSurfaceSceneReason {
    /// The surface has not delivered its first exact contribution.
    AwaitingContribution,
    /// Core-derived semantic requirements changed before any ready plan existed.
    RequirementsChanged,
    /// A workspace epoch replacement invalidated and discarded every prior plan.
    WorkspaceReplaced,
    /// Binding, lifecycle, coordinate generation, bounds, or scale changed.
    CoordinateAuthorityChanged,
    /// The surface has a stable native association which is not ready to authorize geometry.
    CoordinateAuthorityUnavailable,
    /// The exact adapter contribution explicitly lacked one required fact.
    MeasurementsUnavailable(MeasurementAuthorityError),
    /// The exact adapter contribution reported non-positive surface area.
    EmptyBounds,
    /// Exact host geometry could not project the active popup without violating its plane.
    PopupGeometryUnavailable(PopupGeometryUnavailableReason),
    /// Core-owned transient presentation input changed before a paint fallback existed.
    TransientPresentationChanged,
}

/// Why exact host geometry could not project the active popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupGeometryUnavailableReason {
    /// The measured popup plane had no positive area.
    EmptyPlane,
    /// The exact owner anchor was outside the measured popup plane.
    AnchorOutsidePlane,
    /// Neither side of the owner anchor had positive popup space.
    NoSpace,
    /// Complete measured geometry could not produce the required owner popup record.
    ProjectionUnavailable,
}

/// Current surface projection state with separate paint and interaction authority.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadySurfaceScene {
    stamp: SurfaceSceneStamp,
    candidate: Box<SurfacePlanScene>,
    paint_fallback: Option<ReadySurfacePaintFallback>,
    confirmed_paint_fallback: Option<SurfacePresentationOutputTicket>,
    interaction_authority: Option<PresentedSurfaceAuthority>,
}

#[derive(Debug, Clone, PartialEq)]
enum ReadySurfacePaintFallback {
    Candidate,
    Retained(Box<SurfacePlanScene>),
}

/// One exact compiled plan slot with its coordinate capture.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfacePlanScene {
    stamp: SurfaceSceneStamp,
    output_ticket: SurfacePresentationOutputTicket,
    plan: Arc<PresentationPlan>,
    hit_manifest: Arc<PresentationHitManifest>,
    paint_program: Option<Arc<SurfacePaintProgramLink>>,
    semantic_manifest: Arc<PresentationSemanticManifest>,
    coordinate_capture: SurfaceCoordinateCapture,
}

impl ReadySurfaceScene {
    /// Returns the current state and candidate stamp.
    #[must_use]
    pub const fn stamp(&self) -> SurfaceSceneStamp {
        self.stamp
    }

    /// Returns the exact candidate an adapter may paint.
    #[must_use]
    pub const fn candidate(&self) -> &SurfacePlanScene {
        &self.candidate
    }

    /// Returns the output capability of the current candidate.
    #[must_use]
    pub const fn output_ticket(&self) -> SurfacePresentationOutputTicket {
        self.candidate.output_ticket()
    }

    /// Returns the candidate plan an adapter may paint.
    #[must_use]
    pub fn plan(&self) -> &PresentationPlan {
        self.candidate.plan()
    }

    pub(crate) const fn coordinate_capture(&self) -> SurfaceCoordinateCapture {
        self.candidate.coordinate_capture()
    }

    /// Returns the last plan proven painted, which may differ from the candidate.
    #[must_use]
    pub fn paint_fallback(&self) -> Option<&SurfacePlanScene> {
        match self.paint_fallback.as_ref()? {
            ReadySurfacePaintFallback::Candidate => Some(&self.candidate),
            ReadySurfacePaintFallback::Retained(fallback) => Some(fallback),
        }
    }

    fn interaction_authority(&self) -> Option<PresentedSurfaceAuthority> {
        self.interaction_authority
    }

    fn interaction_plan(&self) -> Option<&SurfacePlanScene> {
        let authority = self.interaction_authority?;
        if self.candidate.output_ticket() == authority.ticket() {
            return Some(&self.candidate);
        }
        self.paint_fallback()
            .filter(|fallback| fallback.output_ticket() == authority.ticket())
    }

    fn into_retained_authority(
        self,
    ) -> (
        Option<Box<SurfacePlanScene>>,
        Option<PresentedSurfaceAuthority>,
        Option<SurfacePresentationOutputTicket>,
    ) {
        let interaction_authority = self.interaction_authority;
        let confirmed_paint_fallback = self.confirmed_paint_fallback;
        let paint_fallback = match self.paint_fallback {
            Some(ReadySurfacePaintFallback::Candidate) => Some(self.candidate),
            Some(ReadySurfacePaintFallback::Retained(fallback)) => Some(fallback),
            // A ticketed candidate may have reached an adapter before a final
            // presentation observation arrives. Keep it as the single delayed
            // fallback when a replacement commits in the meantime.
            None => Some(self.candidate),
        };
        debug_assert!(interaction_authority.is_none_or(|authority| {
            paint_fallback
                .as_deref()
                .is_some_and(|fallback| fallback.output_ticket() == authority.ticket())
        }));
        let confirmed_paint_fallback = confirmed_paint_fallback.filter(|ticket| {
            paint_fallback
                .as_deref()
                .is_some_and(|fallback| fallback.output_ticket() == *ticket)
        });
        (
            paint_fallback,
            interaction_authority,
            confirmed_paint_fallback,
        )
    }

    pub(crate) fn has_confirmed_paint_fallback(&self) -> bool {
        self.confirmed_paint_fallback.is_some_and(|ticket| {
            self.paint_fallback()
                .is_some_and(|fallback| fallback.output_ticket() == ticket)
        })
    }

    /// Transfers only an observed paint fallback across an authority
    /// invalidation. A pending output may remain available for a delayed
    /// observation while a replacement candidate is compiled under the same
    /// requirements, but it cannot survive a requirement or coordinate change
    /// without reintroducing stale hit/paint authority.
    fn into_confirmed_paint_fallback(self) -> Option<Box<SurfacePlanScene>> {
        let confirmed = self.confirmed_paint_fallback?;
        if self.candidate.output_ticket() == confirmed {
            return Some(self.candidate);
        }
        match self.paint_fallback {
            Some(ReadySurfacePaintFallback::Retained(fallback))
                if fallback.output_ticket() == confirmed =>
            {
                Some(fallback)
            }
            Some(ReadySurfacePaintFallback::Candidate)
            | Some(ReadySurfacePaintFallback::Retained(_))
            | None => None,
        }
    }

    fn retained_output(
        &self,
        ticket: SurfacePresentationOutputTicket,
    ) -> Option<&SurfacePlanScene> {
        if self.candidate.output_ticket() == ticket {
            return Some(&self.candidate);
        }
        self.paint_fallback()
            .filter(|fallback| fallback.output_ticket() == ticket)
    }

    fn validate_presented_authority(
        &self,
        authority: PresentedSurfaceAuthority,
        popup: PopupPlaneRequirement,
    ) -> Result<(), PresentationAuthorityRejection> {
        let ticket = authority.ticket();
        let Some(output) = self.retained_output(ticket) else {
            return Err(PresentationAuthorityRejection::TicketNotRetained {
                candidate: self.candidate.output_ticket(),
                paint_fallback: self.paint_fallback().map(SurfacePlanScene::output_ticket),
            });
        };
        if output
            .plan()
            .popup()
            .visual_for_surface(authority.surface())
            != popup.visual_for_surface(authority.surface())
            || !output
                .coordinate_capture()
                .matches_presented_authority(authority)
        {
            return Err(PresentationAuthorityRejection::OutputProofMismatch { ticket });
        }
        Ok(())
    }

    fn observe_presented_for_paint(
        &mut self,
        authority: PresentedSurfaceAuthority,
        popup: PopupPlaneRequirement,
    ) -> Result<(), PresentationAuthorityRejection> {
        self.validate_presented_authority(authority, popup)?;
        let ticket = authority.ticket();
        let retained_as_candidate = self.candidate.output_ticket() == ticket;
        if retained_as_candidate {
            self.paint_fallback = Some(ReadySurfacePaintFallback::Candidate);
        }
        self.confirmed_paint_fallback = Some(ticket);
        Ok(())
    }
}

impl SurfaceCoordinateCapture {
    fn matches_presented_authority(self, authority: PresentedSurfaceAuthority) -> bool {
        let (endpoint, generation) = match self {
            Self::Headless {
                authority_generation,
            } => (HostPresentationEndpoint::Headless, authority_generation),
            Self::NativeUnavailable {
                binding,
                authority_generation,
                ..
            } => (
                HostPresentationEndpoint::Native(binding),
                authority_generation,
            ),
            Self::NativeReady {
                coordinates,
                authority_generation,
            } => (
                HostPresentationEndpoint::Native(coordinates.binding()),
                authority_generation,
            ),
        };
        authority.endpoint() == endpoint && authority.coordinate_generation() == generation
    }
}

impl SurfacePlanScene {
    /// Retains an owned semantic witness while sharing the compiled plan and manifests.
    pub(crate) fn frozen_witness(&self) -> Arc<Self> {
        Arc::new(self.clone())
    }

    /// Returns the exact plan authority stamp.
    #[must_use]
    pub const fn stamp(&self) -> SurfaceSceneStamp {
        self.stamp
    }

    /// Returns the opaque output capability minted with this exact scene.
    #[must_use]
    pub const fn output_ticket(&self) -> SurfacePresentationOutputTicket {
        self.output_ticket
    }

    /// Returns the complete core-compiled presentation plan.
    #[must_use]
    pub fn plan(&self) -> &PresentationPlan {
        &self.plan
    }

    pub(crate) const fn shared_plan(&self) -> &Arc<PresentationPlan> {
        &self.plan
    }

    /// Returns the canonical hit inventory bound to this exact output.
    #[must_use]
    pub fn hit_manifest(&self) -> &PresentationHitManifest {
        &self.hit_manifest
    }

    /// Returns the preflight bound to this scene's exact immutable plan and manifest.
    pub(crate) fn paint_program(&self) -> Option<&SurfacePaintProgramLink> {
        self.paint_program.as_deref()
    }

    /// Returns the semantic receiver inventory bound to this exact output.
    #[must_use]
    pub fn semantic_manifest(&self) -> &PresentationSemanticManifest {
        &self.semantic_manifest
    }

    pub(crate) const fn coordinate_capture(&self) -> SurfaceCoordinateCapture {
        self.coordinate_capture
    }
}

/// A surface whose last ready plan may still paint but cannot authorize input.
#[derive(Debug, Clone, PartialEq)]
pub struct StaleSurfaceScene {
    stamp: SurfaceSceneStamp,
    paint_fallback: Box<SurfacePlanScene>,
    reason: StaleSurfaceSceneReason,
    interaction_authority: Option<PresentedSurfaceAuthority>,
}

impl StaleSurfaceScene {
    /// Returns the current non-interactive authority stamp.
    #[must_use]
    pub const fn stamp(&self) -> SurfaceSceneStamp {
        self.stamp
    }

    /// Returns the previous painted presentation retained only as a fallback.
    #[must_use]
    pub const fn paint_fallback(&self) -> &SurfacePlanScene {
        &self.paint_fallback
    }

    /// Returns why the current requirement has no hit-test authority.
    #[must_use]
    pub const fn reason(&self) -> StaleSurfaceSceneReason {
        self.reason
    }
}

/// A roster surface with no paintable ready presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootstrapSurfaceScene {
    stamp: SurfaceSceneStamp,
    pub(super) reason: BootstrapSurfaceSceneReason,
}

impl BootstrapSurfaceScene {
    /// Returns the unavailable roster surface.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.stamp.surface()
    }

    /// Returns the current non-interactive authority stamp.
    #[must_use]
    pub const fn stamp(self) -> SurfaceSceneStamp {
        self.stamp
    }

    /// Returns why no paint fallback or hit authority exists.
    #[must_use]
    pub const fn reason(self) -> BootstrapSurfaceSceneReason {
        self.reason
    }
}

/// Independent presentation authority state for one rostered surface.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceScene {
    /// A current candidate may paint; independent retained authority controls input.
    Ready(Box<ReadySurfaceScene>),
    /// A previous plan may paint, but current facts cannot authorize input.
    Stale(Box<StaleSurfaceScene>),
    /// No current or previous plan is available.
    Bootstrap(BootstrapSurfaceScene),
}

impl SurfaceScene {
    /// Returns the owning surface.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.stamp().surface()
    }

    /// Returns the exact current authority stamp.
    #[must_use]
    pub const fn stamp(&self) -> SurfaceSceneStamp {
        match self {
            Self::Ready(scene) => scene.stamp(),
            Self::Stale(scene) => scene.stamp(),
            Self::Bootstrap(scene) => scene.stamp(),
        }
    }

    /// Returns the current authoritative presentation, if one exists.
    #[must_use]
    pub const fn ready(&self) -> Option<&ReadySurfaceScene> {
        match self {
            Self::Ready(scene) => Some(scene),
            Self::Stale(_) | Self::Bootstrap(_) => None,
        }
    }

    fn interaction_projection(
        &self,
        popup_revision: PopupRoutingRevision,
    ) -> Option<SurfaceInteractionProjection<'_>> {
        let (authority, output) = match self {
            Self::Ready(scene) => (scene.interaction_authority()?, scene.interaction_plan()?),
            Self::Stale(scene) => (scene.interaction_authority?, scene.paint_fallback()),
            Self::Bootstrap(_) => return None,
        };
        debug_assert!(authority.matches_output(output.output_ticket()));
        Some(SurfaceInteractionProjection {
            output,
            authority,
            popup_revision,
        })
    }

    fn interaction_authority(&self) -> Option<PresentedSurfaceAuthority> {
        match self {
            Self::Ready(scene) => scene.interaction_authority(),
            Self::Stale(scene) => scene.interaction_authority,
            Self::Bootstrap(_) => None,
        }
    }

    /// Returns a typed paint-only projection, if one exists.
    #[must_use]
    pub fn paint_projection(&self) -> Option<SurfacePaintProjection<'_>> {
        match self {
            Self::Ready(scene) => Some(SurfacePaintProjection {
                plan: scene.plan(),
                hit_manifest: scene.candidate().hit_manifest(),
                plan_stamp: scene.candidate().stamp(),
                output_ticket: scene.candidate().output_ticket(),
                coordinate_capture: scene.candidate().coordinate_capture(),
                current_stamp: scene.stamp(),
            }),
            Self::Stale(scene) => Some(SurfacePaintProjection {
                plan: scene.paint_fallback().plan(),
                hit_manifest: scene.paint_fallback().hit_manifest(),
                plan_stamp: scene.paint_fallback().stamp(),
                output_ticket: scene.paint_fallback().output_ticket(),
                coordinate_capture: scene.paint_fallback().coordinate_capture(),
                current_stamp: scene.stamp(),
            }),
            Self::Bootstrap(_) => None,
        }
    }

    /// Returns exact plan stamps retained by this scene for adapter-side caching.
    #[must_use]
    pub fn retained_plan_stamps(&self) -> RetainedSurfacePlanStamps {
        match self {
            Self::Ready(scene) => RetainedSurfacePlanStamps {
                candidate: Some(scene.candidate().stamp()),
                paint_fallback: scene.paint_fallback().map(SurfacePlanScene::stamp),
            },
            Self::Stale(scene) => RetainedSurfacePlanStamps {
                candidate: None,
                paint_fallback: Some(scene.paint_fallback().stamp()),
            },
            Self::Bootstrap(_) => RetainedSurfacePlanStamps {
                candidate: None,
                paint_fallback: None,
            },
        }
    }
}

/// Exact output-bound projection eligible for pointer receiver validation.
///
/// Keeping these values in one view prevents adapters from pairing a current
/// candidate plan with authority retained for an older painted fallback.
#[derive(Debug, Clone, Copy)]
pub struct SurfaceInteractionProjection<'a> {
    output: &'a SurfacePlanScene,
    authority: PresentedSurfaceAuthority,
    popup_revision: PopupRoutingRevision,
}

impl<'a> SurfaceInteractionProjection<'a> {
    pub(crate) const fn from_frozen_parts(
        output: &'a SurfacePlanScene,
        authority: PresentedSurfaceAuthority,
        popup_revision: PopupRoutingRevision,
    ) -> Self {
        Self {
            output,
            authority,
            popup_revision,
        }
    }

    pub(crate) const fn output(self) -> &'a SurfacePlanScene {
        self.output
    }

    /// Returns the exact scene stamp compiled into this presented output.
    #[must_use]
    pub const fn plan_stamp(self) -> SurfaceSceneStamp {
        self.output.stamp()
    }

    /// Returns the exact semantic presentation output.
    #[must_use]
    pub const fn output_ticket(self) -> SurfacePresentationOutputTicket {
        self.output.output_ticket()
    }

    /// Returns the plan compiled for this exact output.
    #[must_use]
    pub fn plan(self) -> &'a PresentationPlan {
        self.output.plan()
    }

    /// Returns the hit manifest compiled for this exact output.
    #[must_use]
    pub fn hit_manifest(self) -> &'a PresentationHitManifest {
        self.output.hit_manifest()
    }

    /// Returns the semantic receiver inventory compiled for this exact output.
    #[must_use]
    pub fn semantic_manifest(self) -> &'a PresentationSemanticManifest {
        self.output.semantic_manifest()
    }

    /// Returns final-presentation authority for this exact output.
    #[must_use]
    pub const fn authority(self) -> PresentedSurfaceAuthority {
        self.authority
    }

    pub(crate) const fn popup_revision(self) -> PopupRoutingRevision {
        self.popup_revision
    }
}

/// Exact candidate and fallback identities retained for one surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetainedSurfacePlanStamps {
    candidate: Option<SurfaceSceneStamp>,
    paint_fallback: Option<SurfaceSceneStamp>,
}

impl RetainedSurfacePlanStamps {
    /// Returns the current paint candidate identity, when one exists.
    #[must_use]
    pub const fn candidate(self) -> Option<SurfaceSceneStamp> {
        self.candidate
    }

    /// Returns the retained painted fallback identity, when one exists.
    #[must_use]
    pub const fn paint_fallback(self) -> Option<SurfaceSceneStamp> {
        self.paint_fallback
    }

    /// Iterates unique retained identities in candidate-then-fallback order.
    pub fn unique(self) -> impl Iterator<Item = SurfaceSceneStamp> {
        let fallback = if self.paint_fallback == self.candidate {
            None
        } else {
            self.paint_fallback
        };
        [self.candidate, fallback].into_iter().flatten()
    }
}

/// Paint projection which keeps fallback identity separate from current authority.
#[derive(Debug, Clone, Copy)]
pub struct SurfacePaintProjection<'a> {
    plan: &'a PresentationPlan,
    hit_manifest: &'a PresentationHitManifest,
    plan_stamp: SurfaceSceneStamp,
    output_ticket: SurfacePresentationOutputTicket,
    coordinate_capture: SurfaceCoordinateCapture,
    current_stamp: SurfaceSceneStamp,
}

impl<'a> SurfacePaintProjection<'a> {
    /// Returns the plan eligible only for painting.
    #[must_use]
    pub const fn plan(self) -> &'a PresentationPlan {
        self.plan
    }

    /// Returns the hit inventory compiled from the same exact paint output.
    #[must_use]
    pub const fn hit_manifest(self) -> &'a PresentationHitManifest {
        self.hit_manifest
    }

    /// Returns the exact authority under which this paint candidate was compiled.
    #[must_use]
    pub const fn plan_stamp(self) -> SurfaceSceneStamp {
        self.plan_stamp
    }

    /// Returns the opaque ticket for the exact output that may be painted.
    #[must_use]
    pub const fn output_ticket(self) -> SurfacePresentationOutputTicket {
        self.output_ticket
    }

    pub(crate) const fn coordinate_capture(self) -> SurfaceCoordinateCapture {
        self.coordinate_capture
    }

    /// Returns the current surface state authority, which may be non-interactive.
    #[must_use]
    pub const fn current_stamp(self) -> SurfaceSceneStamp {
        self.current_stamp
    }
}

/// A surface scene and the pending native content provenance of that same slot.
#[derive(Debug, Clone, PartialEq)]
struct SurfaceSceneRecord {
    scene: SurfaceScene,
    native_first_live: Option<crate::engine::NativeFirstLiveProjectionStamp>,
}

impl SurfaceSceneRecord {
    fn new(scene: SurfaceScene) -> Self {
        Self {
            scene,
            native_first_live: None,
        }
    }

    fn scene(&self) -> &SurfaceScene {
        &self.scene
    }

    fn scene_mut(&mut self) -> &mut SurfaceScene {
        &mut self.scene
    }

    fn replace_scene(mut self, replace: impl FnOnce(SurfaceScene) -> SurfaceScene) -> Self {
        self.scene = replace(self.scene);
        self
    }
}

/// Complete independently revisioned presentation roster.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceSceneSet {
    surfaces: BTreeMap<SurfaceId, SurfaceSceneRecord>,
    revision_tombstones: BTreeMap<SurfaceId, SurfaceSceneRevision>,
    popup_requirement: PopupPlaneRequirement,
}

impl SurfaceSceneSet {
    /// A repeated pass restores derived scenes, but never reuses an allocated
    /// revision for different geometry within that same update.
    pub(crate) fn preserve_revision_frontier(&mut self, discarded: &Self) {
        for (surface, revision) in &discarded.revision_tombstones {
            let current = self.revision_tombstones.entry(*surface).or_default();
            *current = (*current).max(*revision);
        }
    }

    pub(crate) fn native_first_live_projection(
        &self,
        surface: SurfaceId,
    ) -> Option<crate::engine::NativeFirstLiveProjectionStamp> {
        self.surfaces.get(&surface)?.native_first_live
    }

    pub(crate) fn set_native_first_live_projection(
        &mut self,
        surface: SurfaceId,
        origin: Option<crate::engine::NativeFirstLiveProjectionStamp>,
    ) {
        if let Some(record) = self.surfaces.get_mut(&surface) {
            record.native_first_live = origin;
        }
    }

    pub(crate) fn new(manifest: &SceneRequirementManifest) -> Result<Self, SceneBuildError> {
        let mut set = Self {
            surfaces: BTreeMap::new(),
            revision_tombstones: BTreeMap::new(),
            popup_requirement: manifest.popup(),
        };
        for (surface, requirements) in manifest.surfaces() {
            let revision = set.next_revision(surface)?;
            set.surfaces.insert(
                surface,
                SurfaceSceneRecord::new(SurfaceScene::Bootstrap(BootstrapSurfaceScene {
                    stamp: SurfaceSceneStamp::new(requirements.ticket(), revision),
                    reason: BootstrapSurfaceSceneReason::AwaitingContribution,
                })),
            );
        }
        Ok(set)
    }

    /// Returns one surface authority, or `None` outside the current roster.
    #[must_use]
    pub fn surface(&self, surface: SurfaceId) -> Option<&SurfaceScene> {
        self.surfaces.get(&surface).map(SurfaceSceneRecord::scene)
    }

    /// Returns one ready surface eligible for hit testing and proof creation.
    #[must_use]
    pub fn ready_surface(&self, surface: SurfaceId) -> Option<&SurfacePlanScene> {
        self.interaction_projection(surface)
            .map(|projection| projection.output)
    }

    /// Returns one indivisible plan, hit manifest, output, and presentation
    /// authority view after its required popup-plane proof boundary is crossed.
    #[must_use]
    pub(crate) fn interaction_projection(
        &self,
        surface: SurfaceId,
    ) -> Option<SurfaceInteractionProjection<'_>> {
        if self.native_first_live_projection(surface).is_some() {
            return None;
        }
        let revision = self.popup_requirement.revision();
        self.surface(surface)
            .and_then(|scene| scene.interaction_projection(revision))
    }

    /// Returns the exact observed interaction authority after the applicable gate.
    #[must_use]
    pub(crate) fn interaction_authority(
        &self,
        surface: SurfaceId,
    ) -> Option<PresentedSurfaceAuthority> {
        self.interaction_projection(surface)
            .map(SurfaceInteractionProjection::authority)
    }

    pub(crate) fn interaction_authorities(&self) -> BTreeMap<SurfaceId, PresentedSurfaceAuthority> {
        #[cfg(test)]
        interaction_authority_work::record_scan(self.surfaces.len());
        self.surfaces
            .keys()
            .filter_map(|surface| {
                self.interaction_authority(*surface)
                    .map(|authority| (*surface, authority))
            })
            .collect()
    }

    /// Iterates concrete emissions retained inside ready scenes.
    pub(crate) fn retained_interaction_emissions(
        &self,
    ) -> impl Iterator<Item = crate::presentation_observation::HostFrameKey> + '_ {
        self.surfaces.values().filter_map(|record| {
            record
                .scene()
                .interaction_authority()
                .map(PresentedSurfaceAuthority::emission)
        })
    }

    /// Tests whether core retained this exact observed emission. Receiver use
    /// still requires the current popup routing revision and output binding.
    pub(crate) fn retains_observed_emission(
        &self,
        output: crate::presentation_observation::HostPresentationOutput,
    ) -> bool {
        let Some(scene) = self.surface(output.surface()) else {
            return false;
        };
        let Some(authority) = scene.interaction_authority() else {
            return false;
        };
        authority.emission() == output.key()
            && authority.stream() == output.stream()
            && authority.endpoint() == output.endpoint()
            && output
                .payload()
                .scene()
                .is_some_and(|scene| authority.matches_output(scene))
            && output.payload().coordinate_generation() == Some(authority.coordinate_generation())
            && self.validate_observed_authority(authority).is_ok()
    }

    pub(crate) fn ready_candidate(&self, surface: SurfaceId) -> Option<&SurfacePlanScene> {
        match self.surface(surface)? {
            SurfaceScene::Ready(ready) => Some(ready.candidate()),
            SurfaceScene::Bootstrap(_) | SurfaceScene::Stale(_) => None,
        }
    }

    /// Revokes only interaction authority minted by one of the terminated
    /// presentation streams, retaining every paint candidate and fallback.
    ///
    /// A superseded host may still have streams for a surface now authorized by
    /// another host. Exact stream matching prevents retirement of the former
    /// from disturbing the latter.
    pub(crate) fn revoke_interaction_authority_for_streams(
        &mut self,
        streams: &BTreeSet<crate::presentation_observation::HostPresentationStreamId>,
    ) -> BTreeSet<SurfaceId> {
        let affected = self
            .surfaces
            .iter()
            .filter_map(|(surface, record)| {
                record
                    .scene()
                    .interaction_authority()
                    .filter(|authority| streams.contains(&authority.stream()))
                    .map(|_| *surface)
            })
            .collect::<BTreeSet<_>>();
        self.revoke_independent_surface_authorities(&affected)
    }

    /// Returns exact retained candidate and fallback stamps for one rostered surface.
    #[must_use]
    pub fn retained_plan_stamps(&self, surface: SurfaceId) -> Option<RetainedSurfacePlanStamps> {
        self.surface(surface)
            .map(SurfaceScene::retained_plan_stamps)
    }

    pub(crate) fn retained_output_capture(
        &self,
        ticket: SurfacePresentationOutputTicket,
    ) -> Result<SurfaceCoordinateCapture, PresentationAuthorityRejection> {
        let surface = ticket.surface();
        let Some(scene) = self.surfaces.get(&surface).map(SurfaceSceneRecord::scene) else {
            return Err(PresentationAuthorityRejection::SurfaceOutsideRoster);
        };
        let SurfaceScene::Ready(ready) = scene else {
            return Err(PresentationAuthorityRejection::SurfaceNotReady);
        };
        let candidate = ready.candidate();
        if candidate.output_ticket() == ticket {
            return Ok(candidate.coordinate_capture());
        }
        if let Some(fallback) = ready.paint_fallback()
            && fallback.output_ticket() == ticket
        {
            return Ok(fallback.coordinate_capture());
        }
        Err(PresentationAuthorityRejection::TicketNotRetained {
            candidate: candidate.output_ticket(),
            paint_fallback: ready.paint_fallback().map(SurfacePlanScene::output_ticket),
        })
    }

    /// Borrows the exact immutable scene while an emission takes ownership.
    pub(crate) fn emitted_scene(
        &self,
        ticket: SurfacePresentationOutputTicket,
    ) -> Option<&SurfacePlanScene> {
        match self.surface(ticket.surface())? {
            SurfaceScene::Ready(ready) => ready.retained_output(ticket),
            SurfaceScene::Stale(stale) => {
                (stale.paint_fallback().output_ticket() == ticket).then_some(stale.paint_fallback())
            }
            SurfaceScene::Bootstrap(_) => None,
        }
    }

    /// Restores an emission-owned scene to the existing presentation slot.
    /// Retention is not permission to revive changed semantic requirements,
    /// routing or coordinates. The caller validates the physical endpoint too.
    pub(crate) fn retain_emitted_scene(
        &mut self,
        output: &SurfacePlanScene,
    ) -> Result<(), PresentationAuthorityRejection> {
        let ticket = output.output_ticket();
        let Some(scene) = self.surfaces.get_mut(&ticket.surface()) else {
            return Err(PresentationAuthorityRejection::SurfaceOutsideRoster);
        };
        let current = scene.scene().stamp().requirement();
        let emitted = output.stamp().requirement();
        if emitted.authority_domain() != current.authority_domain()
            || emitted.workspace_epoch() != current.workspace_epoch()
            || emitted.policy() != current.policy()
            || emitted.config() != current.config()
            || output.plan().popup().visual_for_surface(ticket.surface())
                != self.popup_requirement.visual_for_surface(ticket.surface())
        {
            return Err(PresentationAuthorityRejection::OutputProofMismatch { ticket });
        }
        if let SurfaceScene::Bootstrap(bootstrap) = scene.scene()
            && bootstrap.reason == BootstrapSurfaceSceneReason::RequirementsChanged
        {
            let stamp = bootstrap.stamp;
            *scene.scene_mut() = SurfaceScene::Stale(Box::new(StaleSurfaceScene {
                stamp,
                paint_fallback: Box::new(output.clone()),
                reason: StaleSurfaceSceneReason::RequirementsChanged,
                interaction_authority: None,
            }));
            return Ok(());
        }
        if let SurfaceScene::Stale(stale) = scene.scene_mut() {
            if stale.reason != StaleSurfaceSceneReason::RequirementsChanged
                || !output
                    .coordinate_capture()
                    .same_projection_authority(stale.paint_fallback.coordinate_capture())
            {
                return Err(PresentationAuthorityRejection::OutputProofMismatch { ticket });
            }
            if stale.paint_fallback.output_ticket() != ticket {
                stale.paint_fallback = Box::new(output.clone());
                stale.interaction_authority = None;
            }
            return Ok(());
        }
        let SurfaceScene::Ready(ready) = scene.scene_mut() else {
            return Err(PresentationAuthorityRejection::SurfaceNotReady);
        };
        if ready.retained_output(ticket).is_some() {
            return Ok(());
        }
        if !output
            .coordinate_capture()
            .same_projection_authority(ready.candidate().coordinate_capture())
        {
            return Err(PresentationAuthorityRejection::OutputProofMismatch { ticket });
        }
        ready.paint_fallback = Some(ReadySurfacePaintFallback::Retained(Box::new(
            output.clone(),
        )));
        // The new observed emission is validated and installed by the caller
        // before this candidate can publish. Any old fallback owner is revoked.
        ready.interaction_authority = None;
        ready.confirmed_paint_fallback = None;
        Ok(())
    }

    pub(crate) fn validate_observed_authority(
        &self,
        authority: PresentedSurfaceAuthority,
    ) -> Result<(), PresentationAuthorityRejection> {
        let surface = authority.surface();
        if !self.surfaces.contains_key(&surface) {
            return Err(PresentationAuthorityRejection::SurfaceOutsideRoster);
        }
        match self.surface(surface) {
            Some(SurfaceScene::Ready(ready)) => {
                ready.validate_presented_authority(authority, self.popup_requirement)
            }
            Some(SurfaceScene::Stale(stale))
                if stale.reason == StaleSurfaceSceneReason::RequirementsChanged
                    && stale.paint_fallback.output_ticket() == authority.ticket()
                    && stale
                        .paint_fallback
                        .plan()
                        .popup()
                        .visual_for_surface(surface)
                        == self.popup_requirement.visual_for_surface(surface)
                    && stale
                        .paint_fallback
                        .coordinate_capture()
                        .matches_presented_authority(authority) =>
            {
                Ok(())
            }
            _ => Err(PresentationAuthorityRejection::SurfaceNotReady),
        }
    }

    pub(crate) fn accept_observed_authority(
        &mut self,
        authority: PresentedSurfaceAuthority,
    ) -> Result<BTreeSet<SurfaceId>, PresentationAuthorityRejection> {
        let surface = authority.surface();
        if !self.surfaces.contains_key(&surface) {
            return Err(PresentationAuthorityRejection::SurfaceOutsideRoster);
        }
        self.validate_observed_authority(authority)?;
        let previous = self
            .surface(surface)
            .and_then(SurfaceScene::interaction_authority);
        if let Some(previous) = previous {
            if authority.emission() == previous.emission() {
                debug_assert_eq!(
                    authority, previous,
                    "one core-minted emission key must identify one exact authority"
                );
                return Ok(BTreeSet::new());
            }
            if authority.emission() < previous.emission() {
                return Err(PresentationAuthorityRejection::EmissionRegressed {
                    current: previous.emission(),
                    submitted: authority.emission(),
                });
            }
        }

        let mut changed = BTreeSet::new();

        let Some(scene) = self
            .surfaces
            .get_mut(&surface)
            .map(SurfaceSceneRecord::scene_mut)
        else {
            return Err(PresentationAuthorityRejection::SurfaceNotReady);
        };
        let authority_changed =
            previous.is_none_or(|current| !current.same_interaction_semantics(authority));
        match scene {
            SurfaceScene::Ready(ready) => {
                ready.observe_presented_for_paint(authority, self.popup_requirement)?;
                ready.interaction_authority = Some(authority);
            }
            SurfaceScene::Stale(stale) => stale.interaction_authority = Some(authority),
            SurfaceScene::Bootstrap(_) => {
                return Err(PresentationAuthorityRejection::SurfaceNotReady);
            }
        }
        if authority_changed {
            changed.insert(surface);
        }

        Ok(changed)
    }

    #[cfg(test)]
    pub(crate) fn invalidate_interaction_authority_for_streams(
        &mut self,
        streams: &BTreeSet<crate::presentation_observation::HostPresentationStreamId>,
    ) -> BTreeSet<SurfaceId> {
        self.revoke_interaction_authority_for_streams(streams)
    }

    pub(crate) fn invalidate_interaction_authority_for_current_streams(
        &mut self,
        streams: &BTreeSet<crate::presentation_observation::HostPresentationStreamId>,
        current_surfaces: &BTreeSet<SurfaceId>,
    ) -> BTreeSet<SurfaceId> {
        let affected_surfaces = current_surfaces
            .iter()
            .filter(|surface| self.surfaces.contains_key(surface))
            .copied()
            .collect::<BTreeSet<_>>();
        let proof_surfaces = self
            .surfaces
            .iter()
            .filter_map(|(surface, record)| {
                record
                    .scene()
                    .interaction_authority()
                    .filter(|authority| streams.contains(&authority.stream()))
                    .map(|_| *surface)
            })
            .collect::<BTreeSet<_>>();
        if affected_surfaces.is_empty() && proof_surfaces.is_empty() {
            return BTreeSet::new();
        }
        let mut revoked = affected_surfaces.clone();
        revoked.extend(proof_surfaces);
        let mut changed = self.revoke_independent_surface_authorities(&revoked);
        changed.extend(affected_surfaces);
        changed
    }

    /// Iterates the complete current roster in stable surface order.
    pub fn surfaces(&self) -> impl ExactSizeIterator<Item = (&SurfaceId, &SurfaceScene)> {
        self.surfaces
            .iter()
            .map(|(surface, record)| (surface, record.scene()))
    }

    pub(crate) fn reconcile_manifest(
        &mut self,
        manifest: &SceneRequirementManifest,
    ) -> Result<(), SceneBuildError> {
        let retained = manifest
            .surfaces()
            .map(|(surface, _)| surface)
            .collect::<HashSet<_>>();
        self.surfaces
            .retain(|surface, _| retained.contains(surface));

        for (surface, requirements) in manifest.surfaces() {
            let ticket = requirements.ticket();
            let Some(current) = self.surfaces.remove(&surface) else {
                let revision = self.next_revision(surface)?;
                self.surfaces.insert(
                    surface,
                    SurfaceSceneRecord::new(SurfaceScene::Bootstrap(BootstrapSurfaceScene {
                        stamp: SurfaceSceneStamp::new(ticket, revision),
                        reason: BootstrapSurfaceSceneReason::AwaitingContribution,
                    })),
                );
                continue;
            };
            if current.scene().stamp().requirement() == ticket {
                self.surfaces.insert(surface, current);
                continue;
            }

            let epoch_replaced =
                current.scene().stamp().requirement().workspace_epoch() != ticket.workspace_epoch();
            let previous_ticket = current.scene().stamp().requirement();
            let retained_authority = current.scene().interaction_authority().filter(|_| {
                !epoch_replaced
                    && previous_ticket.policy() == ticket.policy()
                    && previous_ticket.config() == ticket.config()
            });
            let revision = self.next_revision(surface)?;
            let replacement = current.replace_scene(|current_scene| {
                if epoch_replaced {
                    SurfaceScene::Bootstrap(BootstrapSurfaceScene {
                        stamp: SurfaceSceneStamp::new(ticket, revision),
                        reason: BootstrapSurfaceSceneReason::WorkspaceReplaced,
                    })
                } else {
                    match current_scene {
                        SurfaceScene::Ready(previous) => {
                            match previous.into_confirmed_paint_fallback() {
                                Some(paint_fallback) => {
                                    SurfaceScene::Stale(Box::new(StaleSurfaceScene {
                                        stamp: SurfaceSceneStamp::new(ticket, revision),
                                        paint_fallback,
                                        reason: StaleSurfaceSceneReason::RequirementsChanged,
                                        interaction_authority: retained_authority,
                                    }))
                                }
                                None => SurfaceScene::Bootstrap(BootstrapSurfaceScene {
                                    stamp: SurfaceSceneStamp::new(ticket, revision),
                                    reason: BootstrapSurfaceSceneReason::RequirementsChanged,
                                }),
                            }
                        }
                        SurfaceScene::Stale(previous) => {
                            SurfaceScene::Stale(Box::new(StaleSurfaceScene {
                                stamp: SurfaceSceneStamp::new(ticket, revision),
                                paint_fallback: previous.paint_fallback,
                                reason: StaleSurfaceSceneReason::RequirementsChanged,
                                interaction_authority: retained_authority,
                            }))
                        }
                        SurfaceScene::Bootstrap(_) => {
                            SurfaceScene::Bootstrap(BootstrapSurfaceScene {
                                stamp: SurfaceSceneStamp::new(ticket, revision),
                                reason: BootstrapSurfaceSceneReason::RequirementsChanged,
                            })
                        }
                    }
                }
            });
            self.surfaces.insert(surface, replacement);
        }
        self.popup_requirement = manifest.popup();
        self.revoke_invalid_independent_authorities();
        Ok(())
    }

    pub(crate) fn install_ready(
        &mut self,
        plan: impl Into<Arc<PresentationPlan>>,
        coordinate_capture: SurfaceCoordinateCapture,
        authority_domain: EngineAuthorityDomainId,
        output_serial: PresentationOutputSerial,
    ) -> Result<(SurfaceSceneStamp, SurfacePresentationOutputTicket), SceneBuildError> {
        let plan = plan.into();
        let surface = plan.surface();
        if plan.popup() != self.popup_requirement {
            return Err(SceneBuildError::PopupPlaneRecordSetMismatch { surface });
        }
        let ticket = plan
            .measurement_ticket()
            .ok_or(SceneBuildError::SyntheticPresentationRejected { surface })?;
        // Re-observing identical immutable facts does not replace an in-flight
        // candidate. Concrete paints still receive distinct emission keys.
        if let Some(SurfaceScene::Ready(ready)) =
            self.surfaces.get(&surface).map(SurfaceSceneRecord::scene)
            && ready.candidate().coordinate_capture() == coordinate_capture
            && (Arc::ptr_eq(ready.candidate().shared_plan(), &plan)
                || ready.candidate().plan() == plan.as_ref())
        {
            return Ok((ready.stamp(), ready.output_ticket()));
        }
        let current = self
            .surfaces
            .remove(&surface)
            .ok_or(SceneBuildError::SurfaceOutsideRoster { surface })?;
        let expected_ticket = current.scene().stamp().requirement();
        if expected_ticket != ticket {
            self.surfaces.insert(surface, current);
            return Err(SceneBuildError::PresentationTicketMismatch {
                surface,
                expected: Some(expected_ticket),
                actual: ticket,
            });
        }
        let revision = self.next_revision(surface)?;
        let stamp = SurfaceSceneStamp::new(ticket, revision);
        let output_ticket =
            SurfacePresentationOutputTicket::mint(authority_domain, output_serial, stamp);
        let previous = match current.scene() {
            SurfaceScene::Ready(ready) => Some(ready.candidate()),
            SurfaceScene::Stale(stale) => Some(stale.paint_fallback()),
            SurfaceScene::Bootstrap(_) => None,
        };
        let hit_manifest = PresentationHitManifest::compile_reusing(
            output_ticket,
            &plan,
            previous.map(|output| (output.plan(), output.hit_manifest())),
        )?;
        let paint_program = link_surface_paint_program(&plan, &hit_manifest).map(Arc::new);
        let semantic_manifest = PresentationSemanticManifest::compile(output_ticket, &plan)?;
        let current = current.replace_scene(|current_scene| {
            let (retained_fallback, interaction_authority, confirmed_paint_fallback) =
                match current_scene {
                    SurfaceScene::Ready(ready) => ready.into_retained_authority(),
                    SurfaceScene::Stale(stale) => {
                        let confirmed = stale.paint_fallback.output_ticket();
                        (
                            Some(stale.paint_fallback),
                            stale.interaction_authority,
                            Some(confirmed),
                        )
                    }
                    SurfaceScene::Bootstrap(_) => (None, None, None),
                };
            let paint_fallback = retained_fallback.map(ReadySurfacePaintFallback::Retained);
            SurfaceScene::Ready(Box::new(ReadySurfaceScene {
                stamp,
                candidate: Box::new(SurfacePlanScene {
                    stamp,
                    output_ticket,
                    plan,
                    hit_manifest: Arc::new(hit_manifest),
                    paint_program,
                    semantic_manifest: Arc::new(semantic_manifest),
                    coordinate_capture,
                }),
                paint_fallback,
                confirmed_paint_fallback,
                interaction_authority,
            }))
        });
        self.surfaces.insert(surface, current);
        self.revoke_invalid_independent_authorities();
        Ok((stamp, output_ticket))
    }

    pub(crate) fn demote_to_stale(
        &mut self,
        surface: SurfaceId,
        reason: StaleSurfaceSceneReason,
    ) -> Result<SurfaceSceneStamp, SceneBuildError> {
        match self.surfaces.get(&surface).map(SurfaceSceneRecord::scene) {
            Some(SurfaceScene::Ready(ready)) if !ready.has_confirmed_paint_fallback() => {
                return Err(SceneBuildError::ReadySurfaceHasNoPaintFallback { surface });
            }
            Some(SurfaceScene::Bootstrap(_)) => {
                return Err(SceneBuildError::BootstrapCannotBecomeStale { surface });
            }
            Some(SurfaceScene::Ready(_) | SurfaceScene::Stale(_)) => {}
            None => return Err(SceneBuildError::SurfaceOutsideRoster { surface }),
        }
        self.invalidate_popup_authority_for_surface(surface);
        let current = self
            .surfaces
            .remove(&surface)
            .expect("surface fallback preflight established roster membership");
        let current_stamp = current.scene().stamp();
        let revision = self.next_revision(surface)?;
        let stamp = SurfaceSceneStamp::new(current_stamp.requirement(), revision);
        let current = current.replace_scene(|current_scene| {
            let paint_fallback = match current_scene {
                SurfaceScene::Ready(ready) => ready
                    .into_confirmed_paint_fallback()
                    .expect("ready fallback preflight established a retained plan"),
                SurfaceScene::Stale(stale) => stale.paint_fallback,
                SurfaceScene::Bootstrap(_) => {
                    unreachable!("bootstrap preflight returned before scene removal")
                }
            };
            SurfaceScene::Stale(Box::new(StaleSurfaceScene {
                stamp,
                paint_fallback,
                reason,
                interaction_authority: None,
            }))
        });
        self.surfaces.insert(surface, current);
        Ok(stamp)
    }

    /// Advances one surface's authority after core-owned presentation input changes.
    ///
    /// A painted fallback remains paintable but loses hit authority. Surfaces
    /// without such a fallback remain bootstrap. In both cases the new revision
    /// makes every contribution token captured before this change stale.
    pub(crate) fn invalidate_presentation_input(
        &mut self,
        surface: SurfaceId,
    ) -> Result<SurfaceSceneStamp, SceneBuildError> {
        let current = self
            .surfaces
            .remove(&surface)
            .ok_or(SceneBuildError::SurfaceOutsideRoster { surface })?;
        let current_stamp = current.scene().stamp();
        let revision = self.next_revision(surface)?;
        self.invalidate_popup_authority_for_surface(surface);
        let stamp = SurfaceSceneStamp::new(current_stamp.requirement(), revision);
        let current = current.replace_scene(|current_scene| match current_scene {
            SurfaceScene::Ready(ready) => match ready.into_confirmed_paint_fallback() {
                Some(paint_fallback) => SurfaceScene::Stale(Box::new(StaleSurfaceScene {
                    stamp,
                    paint_fallback,
                    reason: StaleSurfaceSceneReason::TransientPresentationChanged,
                    interaction_authority: None,
                })),
                None => SurfaceScene::Bootstrap(BootstrapSurfaceScene {
                    stamp,
                    reason: BootstrapSurfaceSceneReason::TransientPresentationChanged,
                }),
            },
            SurfaceScene::Stale(stale) => SurfaceScene::Stale(Box::new(StaleSurfaceScene {
                stamp,
                paint_fallback: stale.paint_fallback,
                reason: StaleSurfaceSceneReason::TransientPresentationChanged,
                interaction_authority: None,
            })),
            SurfaceScene::Bootstrap(_) => SurfaceScene::Bootstrap(BootstrapSurfaceScene {
                stamp,
                reason: BootstrapSurfaceSceneReason::TransientPresentationChanged,
            }),
        });
        self.surfaces.insert(surface, current);
        Ok(stamp)
    }

    pub(crate) fn replace_with_bootstrap(
        &mut self,
        surface: SurfaceId,
        reason: BootstrapSurfaceSceneReason,
    ) -> Result<SurfaceSceneStamp, SceneBuildError> {
        let current = self
            .surfaces
            .remove(&surface)
            .ok_or(SceneBuildError::SurfaceOutsideRoster { surface })?;
        let revision = self.next_revision(surface)?;
        self.invalidate_popup_authority_for_surface(surface);
        let stamp = SurfaceSceneStamp::new(current.scene().stamp().requirement(), revision);
        let current = current
            .replace_scene(|_| SurfaceScene::Bootstrap(BootstrapSurfaceScene { stamp, reason }));
        self.surfaces.insert(surface, current);
        Ok(stamp)
    }

    fn invalidate_popup_authority_for_surface(&mut self, surface: SurfaceId) {
        self.revoke_independent_surface_authorities(&BTreeSet::from([surface]));
    }

    fn revoke_independent_surface_authorities(
        &mut self,
        surfaces: &BTreeSet<SurfaceId>,
    ) -> BTreeSet<SurfaceId> {
        let mut changed = BTreeSet::new();
        for surface in surfaces {
            let Some(scene) = self
                .surfaces
                .get_mut(surface)
                .map(SurfaceSceneRecord::scene_mut)
            else {
                continue;
            };
            let authority = match scene {
                SurfaceScene::Ready(ready) => &mut ready.interaction_authority,
                SurfaceScene::Stale(stale) => &mut stale.interaction_authority,
                SurfaceScene::Bootstrap(_) => continue,
            };
            if authority.take().is_some() {
                changed.insert(*surface);
            }
        }
        changed
    }

    pub(crate) fn revoke_surface_input(&mut self, surfaces: &BTreeSet<SurfaceId>) {
        self.revoke_independent_surface_authorities(surfaces);
    }

    fn revoke_invalid_independent_authorities(&mut self) -> BTreeSet<SurfaceId> {
        let invalid = self
            .surfaces
            .iter()
            .filter_map(|(surface, record)| {
                let authority = record.scene().interaction_authority()?;
                (authority.surface() != *surface
                    || self.validate_observed_authority(authority).is_err())
                .then_some(*surface)
            })
            .collect::<BTreeSet<_>>();
        self.revoke_independent_surface_authorities(&invalid)
    }

    fn next_revision(
        &mut self,
        surface: SurfaceId,
    ) -> Result<SurfaceSceneRevision, SceneBuildError> {
        let previous = self
            .revision_tombstones
            .get(&surface)
            .copied()
            .unwrap_or_default();
        let revision = previous
            .checked_next()
            .ok_or(SceneBuildError::SurfaceSceneRevisionExhausted { surface })?;
        self.revision_tombstones.insert(surface, revision);
        Ok(revision)
    }
}

#[cfg(test)]
mod tests;

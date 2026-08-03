//! Surface presentation lifecycle, authority, and retained paint state.

use super::*;

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

    fn has_confirmed_paint_fallback(&self) -> bool {
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
        if output.plan().popup() != popup
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

    /// Returns the canonical hit inventory bound to this exact output.
    #[must_use]
    pub fn hit_manifest(&self) -> &PresentationHitManifest {
        &self.hit_manifest
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
        popup_gate_revision: PopupInteractionGateRevision,
    ) -> Option<SurfaceInteractionProjection<'_>> {
        let Self::Ready(scene) = self else {
            return None;
        };
        let authority = scene.interaction_authority()?;
        let output = scene.interaction_plan()?;
        debug_assert!(authority.matches_output(output.output_ticket()));
        Some(SurfaceInteractionProjection {
            output,
            authority,
            popup_gate_revision,
        })
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PopupInteractionGatePhase {
    InactiveIndependent,
    Staging,
    ActivePresented,
}

/// Monotonic identity of one workspace-global popup interaction authority epoch.
///
/// Exhaustion permanently keeps the gate fail-closed rather than permitting an
/// ABA identity reuse.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct PopupInteractionGateRevision(u64);

impl PopupInteractionGateRevision {
    const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

/// Interaction authority coordinator for the workspace popup receiver plane.
///
/// With no active popup, each surface proves and loses authority independently.
/// An active popup spans the workspace, so opening it, replacing it, and
/// clearing it all require one exact-roster presentation barrier. Once the
/// inactive close successor crosses that barrier, surfaces return to independent
/// authority without weakening the exact per-output proof checks.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PopupInteractionGate {
    requirement: PopupPlaneRequirement,
    roster: BTreeSet<SurfaceId>,
    proofs: BTreeMap<SurfaceId, PresentedSurfaceAuthority>,
    phase: PopupInteractionGatePhase,
    revision: Option<PopupInteractionGateRevision>,
}

impl PopupInteractionGate {
    fn new(manifest: &SceneRequirementManifest) -> Self {
        let requirement = manifest.popup();
        Self {
            requirement,
            roster: manifest.surfaces().map(|(surface, _)| surface).collect(),
            proofs: BTreeMap::new(),
            phase: match requirement {
                PopupPlaneRequirement::Inactive { .. } => {
                    PopupInteractionGatePhase::InactiveIndependent
                }
                PopupPlaneRequirement::Active { .. } => PopupInteractionGatePhase::Staging,
            },
            revision: Some(PopupInteractionGateRevision::default()),
        }
    }

    fn requires_roster_barrier(&self) -> bool {
        self.phase != PopupInteractionGatePhase::InactiveIndependent
    }

    fn is_active_presented(&self) -> bool {
        self.phase == PopupInteractionGatePhase::ActivePresented && self.revision.is_some()
    }

    fn interaction_revision(&self) -> Option<PopupInteractionGateRevision> {
        if self.phase == PopupInteractionGatePhase::Staging {
            return None;
        }
        self.revision
    }

    fn reconcile_identity(&mut self, manifest: &SceneRequirementManifest) -> bool {
        let previous_requirement = self.requirement;
        let roster = manifest
            .surfaces()
            .map(|(surface, _)| surface)
            .collect::<BTreeSet<_>>();
        let requirement = manifest.popup();
        let changed = previous_requirement != requirement || self.roster != roster;
        let requires_fresh_barrier = changed
            && (matches!(previous_requirement, PopupPlaneRequirement::Active { .. })
                || matches!(requirement, PopupPlaneRequirement::Active { .. })
                || self.phase == PopupInteractionGatePhase::Staging);
        if changed {
            self.requirement = requirement;
            self.roster = roster;
        }
        requires_fresh_barrier
    }

    fn reset_barrier(&mut self) {
        self.revision = self
            .revision
            .and_then(PopupInteractionGateRevision::checked_next);
        self.proofs.clear();
        self.phase = PopupInteractionGatePhase::Staging;
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
    popup_gate_revision: PopupInteractionGateRevision,
}

impl<'a> SurfaceInteractionProjection<'a> {
    pub(crate) const fn from_frozen_parts(
        output: &'a SurfacePlanScene,
        authority: PresentedSurfaceAuthority,
        popup_gate_revision: PopupInteractionGateRevision,
    ) -> Self {
        Self {
            output,
            authority,
            popup_gate_revision,
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

    pub(crate) const fn popup_gate_revision(self) -> PopupInteractionGateRevision {
        self.popup_gate_revision
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

/// Complete independently revisioned presentation roster.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceSceneSet {
    surfaces: BTreeMap<SurfaceId, SurfaceScene>,
    revision_tombstones: BTreeMap<SurfaceId, SurfaceSceneRevision>,
    popup_interaction_gate: PopupInteractionGate,
}

impl SurfaceSceneSet {
    pub(crate) fn new(manifest: &SceneRequirementManifest) -> Result<Self, SceneBuildError> {
        let mut set = Self {
            surfaces: BTreeMap::new(),
            revision_tombstones: BTreeMap::new(),
            popup_interaction_gate: PopupInteractionGate::new(manifest),
        };
        for (surface, requirements) in manifest.surfaces() {
            let revision = set.next_revision(surface)?;
            set.surfaces.insert(
                surface,
                SurfaceScene::Bootstrap(BootstrapSurfaceScene {
                    stamp: SurfaceSceneStamp::new(requirements.ticket(), revision),
                    reason: BootstrapSurfaceSceneReason::AwaitingContribution,
                }),
            );
        }
        Ok(set)
    }

    /// Returns one surface authority, or `None` outside the current roster.
    #[must_use]
    pub fn surface(&self, surface: SurfaceId) -> Option<&SurfaceScene> {
        self.surfaces.get(&surface)
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
        let revision = self.popup_interaction_gate.interaction_revision()?;
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

    /// Iterates concrete emissions retained inside ready scenes, including proofs temporarily
    /// hidden behind the workspace-global popup presentation barrier.
    ///
    /// The latter must remain available to adapter receiver stores: completing the sibling
    /// roster can reactivate those proofs without emitting the same concrete output again.
    pub(crate) fn retained_interaction_emissions(
        &self,
    ) -> impl Iterator<Item = crate::presentation_observation::HostFrameKey> + '_ {
        self.surfaces.values().filter_map(|scene| match scene {
            SurfaceScene::Ready(ready) => ready
                .interaction_authority()
                .map(PresentedSurfaceAuthority::emission),
            SurfaceScene::Stale(_) | SurfaceScene::Bootstrap(_) => None,
        })
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
        if self.popup_interaction_gate.requires_roster_barrier() {
            let intersects_gate = self
                .popup_interaction_gate
                .proofs
                .values()
                .any(|authority| streams.contains(&authority.stream()));
            if !intersects_gate {
                return BTreeSet::new();
            }
            return self.reset_popup_interaction_barrier();
        }
        let affected = self
            .popup_interaction_gate
            .proofs
            .iter()
            .filter_map(|(surface, authority)| {
                streams.contains(&authority.stream()).then_some(*surface)
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
        let Some(scene) = self.surfaces.get(&surface) else {
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

    pub(crate) fn accept_observed_authority(
        &mut self,
        authority: PresentedSurfaceAuthority,
    ) -> Result<BTreeSet<SurfaceId>, PresentationAuthorityRejection> {
        let surface = authority.surface();
        if !self.popup_interaction_gate.roster.contains(&surface) {
            return Err(PresentationAuthorityRejection::SurfaceOutsideRoster);
        }
        let Some(SurfaceScene::Ready(ready)) = self.surfaces.get(&surface) else {
            return Err(PresentationAuthorityRejection::SurfaceNotReady);
        };
        ready.validate_presented_authority(authority, self.popup_interaction_gate.requirement)?;

        let previous = self.popup_interaction_gate.proofs.get(&surface).copied();
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

        let interaction_semantics_changed =
            previous.is_some_and(|previous| !previous.same_interaction_semantics(authority));
        let mut changed =
            if self.popup_interaction_gate.is_active_presented() && interaction_semantics_changed {
                self.reset_popup_interaction_barrier()
            } else {
                BTreeSet::new()
            };

        let Some(SurfaceScene::Ready(ready)) = self.surfaces.get_mut(&surface) else {
            return Err(PresentationAuthorityRejection::SurfaceNotReady);
        };
        ready.observe_presented_for_paint(authority, self.popup_interaction_gate.requirement)?;
        let phase = self.popup_interaction_gate.phase;
        let authority_changed = ready
            .interaction_authority
            .is_none_or(|current| !current.same_interaction_semantics(authority));
        if phase != PopupInteractionGatePhase::Staging {
            ready.interaction_authority = Some(authority);
            if authority_changed {
                changed.insert(surface);
            }
        }
        self.popup_interaction_gate
            .proofs
            .insert(surface, authority);

        if phase != PopupInteractionGatePhase::Staging {
            return Ok(changed);
        }
        changed.extend(self.try_present_popup_interaction_gate());
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
            .intersection(&self.popup_interaction_gate.roster)
            .copied()
            .collect::<BTreeSet<_>>();
        let proof_surfaces = self
            .popup_interaction_gate
            .proofs
            .iter()
            .filter_map(|(surface, authority)| {
                streams.contains(&authority.stream()).then_some(*surface)
            })
            .collect::<BTreeSet<_>>();
        if affected_surfaces.is_empty() && proof_surfaces.is_empty() {
            return BTreeSet::new();
        }
        if self.popup_interaction_gate.requires_roster_barrier() {
            let mut changed = self.reset_popup_interaction_barrier();
            changed.extend(affected_surfaces);
            return changed;
        }
        let mut revoked = affected_surfaces.clone();
        revoked.extend(proof_surfaces);
        let mut changed = self.revoke_independent_surface_authorities(&revoked);
        changed.extend(affected_surfaces);
        changed
    }

    /// Iterates the complete current roster in stable surface order.
    pub fn surfaces(&self) -> impl ExactSizeIterator<Item = (&SurfaceId, &SurfaceScene)> {
        self.surfaces.iter()
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
                    SurfaceScene::Bootstrap(BootstrapSurfaceScene {
                        stamp: SurfaceSceneStamp::new(ticket, revision),
                        reason: BootstrapSurfaceSceneReason::AwaitingContribution,
                    }),
                );
                continue;
            };
            if current.stamp().requirement() == ticket {
                self.surfaces.insert(surface, current);
                continue;
            }

            let epoch_replaced =
                current.stamp().requirement().workspace_epoch() != ticket.workspace_epoch();
            let revision = self.next_revision(surface)?;
            let replacement = if epoch_replaced {
                SurfaceScene::Bootstrap(BootstrapSurfaceScene {
                    stamp: SurfaceSceneStamp::new(ticket, revision),
                    reason: BootstrapSurfaceSceneReason::WorkspaceReplaced,
                })
            } else {
                match current {
                    SurfaceScene::Ready(previous) => {
                        match previous.into_confirmed_paint_fallback() {
                            Some(paint_fallback) => {
                                SurfaceScene::Stale(Box::new(StaleSurfaceScene {
                                    stamp: SurfaceSceneStamp::new(ticket, revision),
                                    paint_fallback,
                                    reason: StaleSurfaceSceneReason::RequirementsChanged,
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
                        }))
                    }
                    SurfaceScene::Bootstrap(_) => SurfaceScene::Bootstrap(BootstrapSurfaceScene {
                        stamp: SurfaceSceneStamp::new(ticket, revision),
                        reason: BootstrapSurfaceSceneReason::RequirementsChanged,
                    }),
                }
            };
            self.surfaces.insert(surface, replacement);
        }
        let requires_fresh_barrier = self.popup_interaction_gate.reconcile_identity(manifest);
        if requires_fresh_barrier
            || (self.popup_interaction_gate.requires_roster_barrier()
                && !self.popup_gate_proofs_are_current())
        {
            self.reset_popup_interaction_barrier();
        } else if !self.popup_interaction_gate.requires_roster_barrier() {
            self.revoke_invalid_independent_authorities();
        }
        Ok(())
    }

    pub(crate) fn install_ready(
        &mut self,
        plan: PresentationPlan,
        coordinate_capture: SurfaceCoordinateCapture,
        authority_domain: EngineAuthorityDomainId,
        output_serial: PresentationOutputSerial,
    ) -> Result<(SurfaceSceneStamp, SurfacePresentationOutputTicket), SceneBuildError> {
        let surface = plan.surface();
        if plan.popup() != self.popup_interaction_gate.requirement {
            return Err(SceneBuildError::PopupPlaneRecordSetMismatch { surface });
        }
        let ticket = plan
            .measurement_ticket()
            .ok_or(SceneBuildError::SyntheticPresentationRejected { surface })?;
        let current = self
            .surfaces
            .remove(&surface)
            .ok_or(SceneBuildError::SurfaceOutsideRoster { surface })?;
        let expected_ticket = current.stamp().requirement();
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
        let hit_manifest = PresentationHitManifest::compile(output_ticket, &plan);
        let semantic_manifest = PresentationSemanticManifest::compile(output_ticket, &plan);
        let (retained_fallback, retained_interaction, confirmed_paint_fallback) = match current {
            SurfaceScene::Ready(ready) => ready.into_retained_authority(),
            SurfaceScene::Stale(stale) => {
                let confirmed = stale.paint_fallback.output_ticket();
                (Some(stale.paint_fallback), None, Some(confirmed))
            }
            SurfaceScene::Bootstrap(_) => (None, None, None),
        };
        let paint_fallback = retained_fallback.map(ReadySurfacePaintFallback::Retained);
        let interaction_authority = retained_interaction;
        self.surfaces.insert(
            surface,
            SurfaceScene::Ready(Box::new(ReadySurfaceScene {
                stamp,
                candidate: Box::new(SurfacePlanScene {
                    stamp,
                    output_ticket,
                    plan: Arc::new(plan),
                    hit_manifest: Arc::new(hit_manifest),
                    semantic_manifest: Arc::new(semantic_manifest),
                    coordinate_capture,
                }),
                paint_fallback,
                confirmed_paint_fallback,
                interaction_authority,
            })),
        );
        if !self.popup_gate_proofs_are_current() {
            if self.popup_interaction_gate.requires_roster_barrier() {
                self.reset_popup_interaction_barrier();
            } else {
                self.revoke_invalid_independent_authorities();
            }
        }
        Ok((stamp, output_ticket))
    }

    pub(crate) fn demote_to_stale(
        &mut self,
        surface: SurfaceId,
        reason: StaleSurfaceSceneReason,
    ) -> Result<SurfaceSceneStamp, SceneBuildError> {
        match self.surfaces.get(&surface) {
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
        let current_stamp = current.stamp();
        let paint_fallback = match current {
            SurfaceScene::Ready(ready) => ready
                .into_confirmed_paint_fallback()
                .expect("ready fallback preflight established a retained plan"),
            SurfaceScene::Stale(stale) => stale.paint_fallback,
            SurfaceScene::Bootstrap(_) => {
                unreachable!("bootstrap preflight returned before scene removal")
            }
        };
        let revision = self.next_revision(surface)?;
        let stamp = SurfaceSceneStamp::new(current_stamp.requirement(), revision);
        self.surfaces.insert(
            surface,
            SurfaceScene::Stale(Box::new(StaleSurfaceScene {
                stamp,
                paint_fallback,
                reason,
            })),
        );
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
        let current_stamp = current.stamp();
        let revision = self.next_revision(surface)?;
        self.invalidate_popup_authority_for_surface(surface);
        let stamp = SurfaceSceneStamp::new(current_stamp.requirement(), revision);
        let replacement = match current {
            SurfaceScene::Ready(ready) => match ready.into_confirmed_paint_fallback() {
                Some(paint_fallback) => SurfaceScene::Stale(Box::new(StaleSurfaceScene {
                    stamp,
                    paint_fallback,
                    reason: StaleSurfaceSceneReason::TransientPresentationChanged,
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
            })),
            SurfaceScene::Bootstrap(_) => SurfaceScene::Bootstrap(BootstrapSurfaceScene {
                stamp,
                reason: BootstrapSurfaceSceneReason::TransientPresentationChanged,
            }),
        };
        self.surfaces.insert(surface, replacement);
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
        let stamp = SurfaceSceneStamp::new(current.stamp().requirement(), revision);
        self.surfaces.insert(
            surface,
            SurfaceScene::Bootstrap(BootstrapSurfaceScene { stamp, reason }),
        );
        Ok(stamp)
    }

    fn popup_gate_proofs_are_current(&self) -> bool {
        self.popup_interaction_gate
            .proofs
            .iter()
            .all(|(surface, authority)| {
                *surface == authority.surface()
                    && self
                        .surfaces
                        .get(surface)
                        .and_then(SurfaceScene::ready)
                        .is_some_and(|ready| {
                            ready
                                .validate_presented_authority(
                                    *authority,
                                    self.popup_interaction_gate.requirement,
                                )
                                .is_ok()
                        })
            })
    }

    fn try_present_popup_interaction_gate(&mut self) -> BTreeSet<SurfaceId> {
        if !self.popup_interaction_gate.requires_roster_barrier() {
            return BTreeSet::new();
        }
        if self.popup_interaction_gate.roster.is_empty()
            || self.popup_interaction_gate.proofs.len() != self.popup_interaction_gate.roster.len()
            || !self
                .popup_interaction_gate
                .roster
                .iter()
                .all(|surface| self.popup_interaction_gate.proofs.contains_key(surface))
            || !self.popup_gate_proofs_are_current()
        {
            return BTreeSet::new();
        }

        let (gate, surfaces) = (&mut self.popup_interaction_gate, &mut self.surfaces);
        let Some(_) = gate.revision else {
            return BTreeSet::new();
        };
        let mut changed = BTreeSet::new();
        for surface in &gate.roster {
            let authority = *gate
                .proofs
                .get(surface)
                .expect("popup gate preflight established an exact proof roster");
            let Some(SurfaceScene::Ready(ready)) = surfaces.get_mut(surface) else {
                unreachable!("popup gate preflight established an exact ready roster");
            };
            if ready
                .interaction_authority
                .is_none_or(|current| !current.same_interaction_semantics(authority))
            {
                changed.insert(*surface);
            }
            ready.interaction_authority = Some(authority);
        }
        gate.phase = match gate.requirement {
            PopupPlaneRequirement::Inactive { .. } => {
                PopupInteractionGatePhase::InactiveIndependent
            }
            PopupPlaneRequirement::Active { .. } => PopupInteractionGatePhase::ActivePresented,
        };
        changed
    }

    fn reset_popup_interaction_barrier(&mut self) -> BTreeSet<SurfaceId> {
        self.popup_interaction_gate.reset_barrier();
        let mut changed = BTreeSet::new();
        for (surface, scene) in &mut self.surfaces {
            let SurfaceScene::Ready(ready) = scene else {
                continue;
            };
            if ready.interaction_authority.take().is_some() {
                changed.insert(*surface);
            }
        }
        changed
    }

    fn invalidate_popup_authority_for_surface(&mut self, surface: SurfaceId) {
        if self.popup_interaction_gate.requires_roster_barrier() {
            self.reset_popup_interaction_barrier();
            return;
        }
        self.popup_interaction_gate.proofs.remove(&surface);
        if let Some(SurfaceScene::Ready(ready)) = self.surfaces.get_mut(&surface) {
            ready.interaction_authority = None;
        }
    }

    fn revoke_independent_surface_authorities(
        &mut self,
        surfaces: &BTreeSet<SurfaceId>,
    ) -> BTreeSet<SurfaceId> {
        debug_assert!(!self.popup_interaction_gate.requires_roster_barrier());
        let mut changed = BTreeSet::new();
        for surface in surfaces {
            self.popup_interaction_gate.proofs.remove(surface);
            let Some(SurfaceScene::Ready(ready)) = self.surfaces.get_mut(surface) else {
                continue;
            };
            if ready.interaction_authority.take().is_some() {
                changed.insert(*surface);
            }
        }
        changed
    }

    fn revoke_invalid_independent_authorities(&mut self) -> BTreeSet<SurfaceId> {
        debug_assert!(!self.popup_interaction_gate.requires_roster_barrier());
        let invalid = self
            .popup_interaction_gate
            .proofs
            .iter()
            .filter_map(|(surface, authority)| {
                let current = *surface == authority.surface()
                    && self
                        .surfaces
                        .get(surface)
                        .and_then(SurfaceScene::ready)
                        .is_some_and(|ready| {
                            ready
                                .validate_presented_authority(
                                    *authority,
                                    self.popup_interaction_gate.requirement,
                                )
                                .is_ok()
                        });
                (!current).then_some(*surface)
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

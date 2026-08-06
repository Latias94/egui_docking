//! Core-owned observation of actual presentation output.
//!
//! A [`SurfacePresentationOutputTicket`] identifies semantic scene output. It
//! does not prove that a renderer presented any particular paint pass. This
//! module owns the second identity domain: host leases, render streams, and
//! concrete emissions. Hosts may only report progress for keys the core
//! emitted to that exact stream.

mod retention;

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::command::MovePayload;
use crate::ids::{
    EngineAuthorityDomainId, HostPresentationAttemptId, NativeCreateSagaId, ReducerTickId,
    SurfaceId,
};
use crate::intent::{Authority, AuthorityUnavailableReason};
use crate::interaction::{ContainedTransformPreviewToken, PreviewToken};
use crate::platform_provider::PlatformObservationLease;
use crate::retention::PresentationHostRetentionManifest;
use crate::scene::SurfaceSceneStamp;
use crate::surface_recovery::SurfaceRecoveryObligationId;
use crate::viewport::{CoordinateGeneration, PresentationObservationGeneration, ViewportBinding};

use self::retention::RetiredPresentationHostRanges;

/// Monotonic engine-local identity of one committed semantic presentation output.
///
/// The representation remains crate-private so adapters cannot infer or mint
/// future tickets from an observed value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub(crate) struct PresentationOutputSerial(u64);

impl PresentationOutputSerial {
    #[cfg(test)]
    pub(crate) const fn new_for_test(value: u64) -> Self {
        Self(value)
    }

    pub(crate) const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    #[cfg(test)]
    pub(crate) const fn get(self) -> u64 {
        self.0
    }
}

/// Opaque capability for one exact core-committed semantic presentation output.
///
/// A ticket identifies scene output but never proves that a host actually
/// presented it. Concrete presentation proof is represented by a later
/// [`PresentedSurfaceAuthority`] created from a core-owned emission observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfacePresentationOutputTicket {
    authority_domain: EngineAuthorityDomainId,
    serial: PresentationOutputSerial,
    surface: SurfaceId,
    scene: SurfaceSceneStamp,
}

impl SurfacePresentationOutputTicket {
    pub(crate) const fn mint(
        authority_domain: EngineAuthorityDomainId,
        serial: PresentationOutputSerial,
        scene: SurfaceSceneStamp,
    ) -> Self {
        Self {
            authority_domain,
            serial,
            surface: scene.surface(),
            scene,
        }
    }

    /// Returns the sole logical surface whose output this ticket names.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    pub(crate) const fn was_issued_after(self, floor: PresentationOutputSerial) -> bool {
        self.serial.0 > floor.0
    }

    #[cfg(test)]
    pub(crate) const fn serial(self) -> PresentationOutputSerial {
        self.serial
    }
}

/// Opaque interaction authority minted only after a core-owned final-presentation
/// observation proves one exact emission.
///
/// Pointer ingress will carry this capability instead of a naked scene stamp.
/// Its stream and emission identity bind interaction to one real output rather
/// than to an adapter-owned frame counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PresentedSurfaceAuthority {
    ticket: SurfacePresentationOutputTicket,
    coordinate_generation: CoordinateGeneration,
    stream: HostPresentationStreamId,
    emission: HostFrameKey,
    endpoint: HostPresentationEndpoint,
}

impl PresentedSurfaceAuthority {
    /// Mints authority from one core-owned final-presentation observation.
    pub(crate) const fn mint_observed(
        ticket: SurfacePresentationOutputTicket,
        stream: HostPresentationStreamId,
        emission: HostFrameKey,
        endpoint: HostPresentationEndpoint,
        coordinate_generation: CoordinateGeneration,
    ) -> Self {
        Self {
            ticket,
            coordinate_generation,
            stream,
            emission,
            endpoint,
        }
    }

    #[cfg(test)]
    pub(crate) const fn mint_observed_for_test(
        ticket: SurfacePresentationOutputTicket,
        coordinate_generation: CoordinateGeneration,
    ) -> Self {
        Self::mint_observed_for_test_with_emission(ticket, coordinate_generation, 1)
    }

    #[cfg(test)]
    pub(crate) const fn mint_observed_for_test_with_emission(
        ticket: SurfacePresentationOutputTicket,
        coordinate_generation: CoordinateGeneration,
        emission_ordinal: u64,
    ) -> Self {
        let stream = HostPresentationStreamId {
            authority_domain: ticket.authority_domain,
            serial: PresentationStreamSerial(1),
        };
        Self::mint_observed(
            ticket,
            stream,
            HostFrameKey {
                stream,
                ordinal: emission_ordinal,
            },
            HostPresentationEndpoint::Headless,
            coordinate_generation,
        )
    }

    /// Returns the sole logical surface whose interaction this authority covers.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.ticket.surface()
    }

    /// Returns the exact native binding, if the observed output is native.
    #[must_use]
    pub const fn binding(self) -> Option<ViewportBinding> {
        match self.endpoint {
            HostPresentationEndpoint::Headless => None,
            HostPresentationEndpoint::Native(binding) => Some(binding),
        }
    }

    /// Returns the coordinate authority generation frozen by the observation.
    #[must_use]
    pub const fn coordinate_generation(self) -> CoordinateGeneration {
        self.coordinate_generation
    }

    /// Returns the core-owned stream that emitted the observed output.
    #[must_use]
    pub const fn stream(self) -> HostPresentationStreamId {
        self.stream
    }

    /// Returns the exact core-owned emission known to be finally presented.
    #[must_use]
    pub const fn emission(self) -> HostFrameKey {
        self.emission
    }

    /// Returns the endpoint frozen into the observed output.
    #[must_use]
    pub const fn endpoint(self) -> HostPresentationEndpoint {
        self.endpoint
    }

    /// Returns whether this authority proves the exact supplied semantic output.
    #[must_use]
    pub fn matches_output(self, output: SurfacePresentationOutputTicket) -> bool {
        self.ticket == output
    }

    /// Returns whether two observations authorize the same interaction geometry.
    ///
    /// A newer emission may refresh final-presentation provenance without
    /// changing the semantic scene, coordinates, or destination endpoint. Such
    /// a refresh must not clear a live preview merely because its concrete
    /// emission key advanced.
    pub(crate) fn same_interaction_semantics(self, other: Self) -> bool {
        self.ticket == other.ticket
            && self.coordinate_generation == other.coordinate_generation
            && self.endpoint == other.endpoint
    }

    pub(crate) const fn ticket(self) -> SurfacePresentationOutputTicket {
        self.ticket
    }
}

/// Why an internally observed presentation output could not become scene
/// interaction authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PresentationAuthorityRejection {
    /// The surface no longer belongs to the current scene roster.
    SurfaceOutsideRoster,
    /// The surface currently has no ready scene that can retain an output.
    SurfaceNotReady,
    /// The observed output is no longer the current candidate or retained fallback.
    TicketNotRetained {
        /// Current candidate output identity.
        candidate: SurfacePresentationOutputTicket,
        /// Retained fallback output identity, when one exists.
        paint_fallback: Option<SurfacePresentationOutputTicket>,
    },
    /// The retained output's popup, endpoint, or coordinate generation does not
    /// match the final-presentation proof.
    OutputProofMismatch {
        /// Output named by the rejected proof.
        ticket: SurfacePresentationOutputTicket,
    },
    /// A delayed observation attempted to replace newer concrete presentation
    /// provenance already retained by the scene.
    EmissionRegressed {
        /// The concrete emission currently authorizing interaction.
        current: HostFrameKey,
        /// The delayed concrete emission submitted for promotion.
        submitted: HostFrameKey,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
struct PresentationHostSerial(u64);

impl PresentationHostSerial {
    fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
struct PresentationStreamSerial(u64);

impl PresentationStreamSerial {
    fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

/// Opaque core-minted lease for one independent presentation host.
///
/// A host lease is deliberately distinct from a UI framework context or a
/// native window. One host may own multiple streams over its lifetime, and a
/// stream may outlive its logical surface while delayed presentation facts are
/// settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PresentationHostLease {
    authority_domain: EngineAuthorityDomainId,
    serial: PresentationHostSerial,
}

/// Why one presentation runtime permanently relinquished its host lease.
///
/// Retirement is a terminal lifecycle fact. It is deliberately distinct from
/// an observation that no output was presented: destruction proves that the
/// host can never emit or settle another output, not what happened to any
/// particular pending frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PresentationHostRetirementReason {
    /// The complete renderer or native runtime was destroyed.
    RuntimeDestroyed,
    /// The adapter detached this host from the engine while remaining alive.
    AdapterDetached,
    /// The owner performed an orderly explicit shutdown.
    ExplicitShutdown,
}

/// Durable terminal record retained for a retired presentation host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PresentationHostRetirementTombstone {
    retired_at: ReducerTickId,
    reason: PresentationHostRetirementReason,
}

impl PresentationHostRetirementTombstone {
    /// Returns the exact reducer boundary which retired the host.
    #[must_use]
    pub const fn retired_at(self) -> ReducerTickId {
        self.retired_at
    }

    /// Returns the terminal lifecycle reason recorded by the owner.
    #[must_use]
    pub const fn reason(self) -> PresentationHostRetirementReason {
        self.reason
    }
}

impl PresentationHostLease {
    pub(crate) const fn authority_domain(self) -> EngineAuthorityDomainId {
        self.authority_domain
    }
}

/// Opaque core-minted identity of one renderer presentation stream.
///
/// A stream is bound to one host lease, logical surface, and exact endpoint
/// incarnation. It cannot be recreated by a host from a surface identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HostPresentationStreamId {
    authority_domain: EngineAuthorityDomainId,
    serial: PresentationStreamSerial,
}

/// Affine renderer acknowledgement that one retiring stream is externally quiescent.
///
/// A host may obtain this only after it has stopped every path which could submit another
/// observation for the stream. Consuming it removes the detailed retiring-stream record once
/// core has also released every exact reference. The private fields and lack of `Clone` prevent
/// a renderer from manufacturing or replaying acknowledgements.
#[must_use = "submit stream quiescence to reclaim the retiring stream record"]
#[derive(Debug)]
pub struct PresentationStreamQuiescence {
    host: PresentationHostLease,
    stream: HostPresentationStreamId,
}

/// Opaque core-minted identity of one actual presentation emission.
///
/// Keys are stream-local and strictly monotonic. The host may retain and later
/// report a key, but cannot construct or advance one itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HostFrameKey {
    stream: HostPresentationStreamId,
    ordinal: u64,
}

impl HostFrameKey {
    /// Returns the sole stream that emitted this key.
    #[must_use]
    pub const fn stream(self) -> HostPresentationStreamId {
        self.stream
    }

    #[cfg(test)]
    pub(crate) const fn ordinal_for_test(self) -> u64 {
        self.ordinal
    }
}

/// Provider-captured monotonic generation for observations of one stream.
///
/// This is a provider fact rather than an authority capability, so adapters
/// may construct it from their own lossless capture generation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct HostPresentationCaptureGeneration(u64);

impl HostPresentationCaptureGeneration {
    /// Creates a provider-captured generation value.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the provider-captured generation representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Advances the generation without wrapping.
    #[must_use]
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

/// Exact endpoint associated with a core-owned presentation stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HostPresentationEndpoint {
    /// The logical surface was headless when its output was emitted.
    Headless,
    /// The output was emitted for this exact native viewport binding.
    Native(ViewportBinding),
}

/// Native bring-up staging pass which must be observed before lifecycle advance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NativeStagingPresentationPhase {
    /// Hidden-window output which must be presented before `ShowWindow` is requested.
    PreShow,
    /// Visible placeholder output which must be presented before owner-specific admission.
    PostShow,
}

/// Core-private lifecycle owner of one native staging sequence.
///
/// A recovery replacement can stage an established logical surface without a
/// retained tear-off resource. Keeping lifecycle identity separate from that
/// optional resource prevents recovery from fabricating a native-create saga.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum NativeStagingOwner {
    NativeCreate {
        resource: NativeStagingResourceId,
    },
    RecoveryReplacement {
        destroyed_binding: ViewportBinding,
        recovery_obligation: SurfaceRecoveryObligationId,
        retained_resource: Option<NativeStagingResourceId>,
    },
}

impl NativeStagingOwner {
    pub(crate) fn native_create(
        saga: NativeCreateSagaId,
        resource: NativeStagingResourceId,
    ) -> Option<Self> {
        if resource.saga() != saga {
            return None;
        }
        Some(Self::NativeCreate { resource })
    }

    pub(crate) fn recovery_replacement(
        destroyed_binding: ViewportBinding,
        recovery_obligation: SurfaceRecoveryObligationId,
        retained_resource: Option<NativeStagingResourceId>,
    ) -> Option<Self> {
        if let Some(resource) = retained_resource
            && resource.authority_domain() != destroyed_binding.authority_domain()
        {
            return None;
        }
        Some(Self::RecoveryReplacement {
            destroyed_binding,
            recovery_obligation,
            retained_resource,
        })
    }

    pub(crate) const fn authority_domain(self) -> EngineAuthorityDomainId {
        match self {
            Self::NativeCreate { resource, .. } => resource.authority_domain(),
            Self::RecoveryReplacement {
                destroyed_binding, ..
            } => destroyed_binding.authority_domain(),
        }
    }

    pub(crate) const fn retained_resource(self) -> Option<NativeStagingResourceId> {
        match self {
            Self::NativeCreate { resource, .. } => Some(resource),
            Self::RecoveryReplacement {
                retained_resource, ..
            } => retained_resource,
        }
    }

    pub(crate) const fn native_create_saga(self) -> Option<NativeCreateSagaId> {
        match self {
            Self::NativeCreate { resource } => Some(resource.saga()),
            Self::RecoveryReplacement { .. } => None,
        }
    }
}

/// Opaque identity of the retained source resource backing one native-create saga.
///
/// The identity is minted by the core and remains stable across pre-show and
/// post-show request reissues. Its private fields prevent a host from splicing
/// staging output into another engine or native-create saga.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NativeStagingResourceId {
    authority_domain: EngineAuthorityDomainId,
    saga: NativeCreateSagaId,
}

impl NativeStagingResourceId {
    pub(crate) const fn mint(
        authority_domain: EngineAuthorityDomainId,
        saga: NativeCreateSagaId,
    ) -> Self {
        Self {
            authority_domain,
            saga,
        }
    }

    pub(crate) const fn authority_domain(self) -> EngineAuthorityDomainId {
        self.authority_domain
    }

    pub(crate) const fn saga(self) -> NativeCreateSagaId {
        self.saga
    }
}

/// Immutable source content retained while a native-create saga is unresolved.
///
/// This descriptor names the exact presented source authority and logical move
/// payload. It does not grant mutation authority; the checked workspace command
/// remains the sole topology mutation protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeStagingResourceDescriptor {
    id: NativeStagingResourceId,
    source_presentation: PresentedSurfaceAuthority,
    payload: MovePayload,
}

impl NativeStagingResourceDescriptor {
    pub(crate) fn mint(
        id: NativeStagingResourceId,
        source_presentation: PresentedSurfaceAuthority,
        payload: MovePayload,
    ) -> Option<Self> {
        (id.authority_domain == source_presentation.ticket.authority_domain).then_some(Self {
            id,
            source_presentation,
            payload,
        })
    }

    /// Returns the stable resource identity retained by the core.
    #[must_use]
    pub const fn id(&self) -> NativeStagingResourceId {
        self.id
    }

    /// Returns the exact final-presentation authority of the source content.
    #[must_use]
    pub const fn source_presentation(&self) -> PresentedSurfaceAuthority {
        self.source_presentation
    }

    /// Returns the exact move payload frozen at the native tear-off release.
    #[must_use]
    pub const fn payload(&self) -> &MovePayload {
        &self.payload
    }
}

/// Exact platform facts which authorize one native staging presentation.
///
/// A request must be reissued when any member changes. In particular, retaining
/// only a viewport binding is insufficient because provider replacement and a
/// hidden-visible-hidden presentation ABA can leave the binding unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NativeStagingBasis {
    platform_provider: PlatformObservationLease,
    presentation_observation_generation: PresentationObservationGeneration,
    coordinate_generation: CoordinateGeneration,
    owner: NativeStagingOwner,
}

impl NativeStagingBasis {
    pub(crate) fn new(
        platform_provider: PlatformObservationLease,
        presentation_observation_generation: PresentationObservationGeneration,
        coordinate_generation: CoordinateGeneration,
        owner: NativeStagingOwner,
    ) -> Option<Self> {
        (platform_provider.authority_domain() == owner.authority_domain()).then_some(Self {
            platform_provider,
            presentation_observation_generation,
            coordinate_generation,
            owner,
        })
    }

    /// Returns the exact platform-provider incarnation which observed this basis.
    #[must_use]
    pub const fn platform_provider(self) -> PlatformObservationLease {
        self.platform_provider
    }

    /// Returns the exact binding-scoped presentation observation generation.
    #[must_use]
    pub const fn presentation_observation_generation(self) -> PresentationObservationGeneration {
        self.presentation_observation_generation
    }

    /// Returns the exact canonical coordinate generation for staging placement.
    #[must_use]
    pub const fn coordinate_generation(self) -> CoordinateGeneration {
        self.coordinate_generation
    }

    /// Returns the retained source resource used by this staging request, if any.
    #[must_use]
    pub const fn retained_resource(self) -> Option<NativeStagingResourceId> {
        self.owner.retained_resource()
    }
}

/// Opaque core-minted request to paint one exact native lifecycle staging window.
///
/// The request is not a dock scene and grants no interaction authority. It
/// exists solely to join an actual host paint and final-presentation fact to
/// one native bring-up owner and viewport incarnation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NativeStagingPresentation {
    binding: ViewportBinding,
    phase: NativeStagingPresentationPhase,
    basis: NativeStagingBasis,
}

impl NativeStagingPresentation {
    pub(crate) fn mint(
        binding: ViewportBinding,
        phase: NativeStagingPresentationPhase,
        basis: NativeStagingBasis,
    ) -> Option<Self> {
        (binding.authority_domain() == basis.owner.authority_domain()).then_some(Self {
            binding,
            phase,
            basis,
        })
    }

    #[must_use]
    pub const fn native_create_saga(self) -> Option<NativeCreateSagaId> {
        self.basis.owner.native_create_saga()
    }

    #[must_use]
    pub const fn binding(self) -> ViewportBinding {
        self.binding
    }

    #[must_use]
    pub const fn phase(self) -> NativeStagingPresentationPhase {
        self.phase
    }

    /// Returns the exact platform facts authorizing this staging request.
    #[must_use]
    pub const fn basis(self) -> NativeStagingBasis {
        self.basis
    }

    /// Returns the retained source resource used by this staging request, if any.
    #[must_use]
    pub const fn retained_resource(self) -> Option<NativeStagingResourceId> {
        self.basis.owner.retained_resource()
    }

    pub(crate) const fn owner(self) -> NativeStagingOwner {
        self.basis.owner
    }

    pub(crate) const fn authority_domain(self) -> EngineAuthorityDomainId {
        self.basis.owner.authority_domain()
    }
}

/// Exact final-presentation proof for one native staging request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PresentedNativeStagingPresentation {
    request: NativeStagingPresentation,
    stream: HostPresentationStreamId,
    output: HostFrameKey,
}

impl PresentedNativeStagingPresentation {
    pub(crate) const fn mint_observed(
        request: NativeStagingPresentation,
        stream: HostPresentationStreamId,
        output: HostFrameKey,
    ) -> Self {
        Self {
            request,
            stream,
            output,
        }
    }

    #[cfg(test)]
    pub(crate) const fn mint_observed_for_test(
        request: NativeStagingPresentation,
        ordinal: u64,
    ) -> Self {
        let stream = HostPresentationStreamId {
            authority_domain: request.authority_domain(),
            serial: PresentationStreamSerial(1),
        };
        Self::mint_observed(request, stream, HostFrameKey { stream, ordinal })
    }

    pub(crate) const fn request(self) -> NativeStagingPresentation {
        self.request
    }

    /// Returns the retained source resource proven by this final presentation, if any.
    #[must_use]
    pub const fn retained_resource(self) -> Option<NativeStagingResourceId> {
        self.request.retained_resource()
    }

    /// Returns whether this proof names the exact staging output.
    ///
    /// This keeps the proof's engine-private identities opaque while allowing
    /// protocol adapters to correlate a lifecycle effect with an output they
    /// previously retained.
    #[must_use]
    pub fn matches_output(self, output: HostPresentationOutput) -> bool {
        self.stream == output.stream
            && self.output == output.key
            && matches!(
                output.payload,
                HostPresentationOutputPayload::NativeStaging { presentation }
                    if presentation == self.request
            )
    }
}

/// Transient interaction visuals actually included in one host paint.
///
/// These tokens are core-minted at frame begin. A later final-presentation
/// observation can therefore prove that the exact preview, rather than merely
/// the same scene geometry, reached the screen after its publication.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct HostInteractionPresentation {
    drag_preview: Option<PreviewToken>,
    contained_transform_preview: Option<ContainedTransformPreviewToken>,
}

impl HostInteractionPresentation {
    /// Records the exact transient interaction visuals included in one host paint.
    #[must_use]
    pub const fn new(
        drag_preview: Option<PreviewToken>,
        contained_transform_preview: Option<ContainedTransformPreviewToken>,
    ) -> Self {
        Self {
            drag_preview,
            contained_transform_preview,
        }
    }

    /// Returns the exact drag preview included in this paint, when any.
    #[must_use]
    pub const fn drag_preview(self) -> Option<PreviewToken> {
        self.drag_preview
    }

    /// Returns the exact contained-transform preview included in this paint.
    #[must_use]
    pub const fn contained_transform_preview(self) -> Option<ContainedTransformPreviewToken> {
        self.contained_transform_preview
    }
}

/// Semantic payload emitted by one actual host render pass.
///
/// A paint ticket may occur in several emissions. Bootstrap and unavailable
/// output are tracked too so that their stream lifecycle cannot be inferred
/// from callback absence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostPresentationOutputPayload {
    /// One semantic scene output and its frozen interaction visuals were painted.
    Paint {
        /// Exact semantic scene output painted by the host.
        scene: SurfacePresentationOutputTicket,
        /// Coordinate authority frozen into that semantic output.
        coordinate_generation: CoordinateGeneration,
        /// Transient core-owned visuals painted above that scene.
        interaction: HostInteractionPresentation,
    },
    /// One non-interactive native lifecycle staging placeholder was painted.
    NativeStaging {
        /// Exact lifecycle request represented by this paint.
        presentation: NativeStagingPresentation,
    },
    /// A bootstrap placeholder was actually painted.
    Bootstrap,
    /// An unavailable placeholder was actually painted.
    Unavailable,
}

impl HostPresentationOutputPayload {
    /// Returns the painted semantic scene ticket, when this is a paint output.
    #[must_use]
    pub const fn scene(self) -> Option<SurfacePresentationOutputTicket> {
        match self {
            Self::Paint { scene, .. } => Some(scene),
            Self::NativeStaging { .. } | Self::Bootstrap | Self::Unavailable => None,
        }
    }

    /// Returns the coordinate authority frozen into a painted semantic output.
    #[must_use]
    pub const fn coordinate_generation(self) -> Option<CoordinateGeneration> {
        match self {
            Self::Paint {
                coordinate_generation,
                ..
            } => Some(coordinate_generation),
            Self::NativeStaging { .. } | Self::Bootstrap | Self::Unavailable => None,
        }
    }

    /// Returns the exact transient visuals included in this paint.
    #[must_use]
    pub const fn interaction(self) -> Option<HostInteractionPresentation> {
        match self {
            Self::Paint { interaction, .. } => Some(interaction),
            Self::NativeStaging { .. } | Self::Bootstrap | Self::Unavailable => None,
        }
    }
}

/// Core record returned after one actual host presentation emission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostPresentationOutput {
    key: HostFrameKey,
    stream: HostPresentationStreamId,
    surface: SurfaceId,
    endpoint: HostPresentationEndpoint,
    payload: HostPresentationOutputPayload,
}

/// Opaque request identity returned while one core host frame stages an actual
/// paint result.
///
/// This is not a presentation proof. It only binds an adapter's post-paint
/// record to the output that appears in the successful transition for the
/// same host lease and predecessor reducer tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HostPresentationEmissionRequest {
    host: PresentationHostLease,
    predecessor_tick: ReducerTickId,
    attempt: HostPresentationAttemptId,
    ordinal: u64,
}

impl HostPresentationEmissionRequest {
    pub(crate) const fn new(
        host: PresentationHostLease,
        predecessor_tick: ReducerTickId,
        attempt: HostPresentationAttemptId,
        ordinal: u64,
    ) -> Self {
        Self {
            host,
            predecessor_tick,
            attempt,
            ordinal,
        }
    }

    pub(crate) const fn host(self) -> PresentationHostLease {
        self.host
    }

    pub(crate) const fn predecessor_tick(self) -> ReducerTickId {
        self.predecessor_tick
    }

    pub(crate) const fn attempt(self) -> HostPresentationAttemptId {
        self.attempt
    }

    pub(crate) const fn ordinal(self) -> u64 {
        self.ordinal
    }
}

/// One actual paint result paired with the core output identity minted only
/// after the host frame committed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostPresentationEmission {
    request: HostPresentationEmissionRequest,
    output: HostPresentationOutput,
}

impl HostPresentationEmission {
    pub(crate) const fn new(
        request: HostPresentationEmissionRequest,
        output: HostPresentationOutput,
    ) -> Self {
        Self { request, output }
    }

    /// Returns the frame-local request supplied after the actual paint.
    #[must_use]
    pub const fn request(self) -> HostPresentationEmissionRequest {
        self.request
    }

    /// Returns the core-minted stream and emission identity.
    #[must_use]
    pub const fn output(self) -> HostPresentationOutput {
        self.output
    }
}

impl HostPresentationOutput {
    /// Returns the concrete emission key that a host may later observe.
    #[must_use]
    pub const fn key(self) -> HostFrameKey {
        self.key
    }

    /// Returns the stream that emitted this output.
    #[must_use]
    pub const fn stream(self) -> HostPresentationStreamId {
        self.stream
    }

    /// Returns the logical surface rendered by this emission.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    /// Returns the exact endpoint frozen for this emission.
    #[must_use]
    pub const fn endpoint(self) -> HostPresentationEndpoint {
        self.endpoint
    }

    /// Returns the semantic payload rendered by this emission.
    #[must_use]
    pub const fn payload(self) -> HostPresentationOutputPayload {
        self.payload
    }
}

/// Presentation facts submitted exactly once at the beginning of a host frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostPresentationObservation {
    /// The host captured no new final-presentation fact for any stream.
    NoUpdate,
    /// One complete exact-set observation for every stream pending at frame begin.
    Batch(Vec<HostPresentationObservationEntry>),
}

/// One stream's fact within a complete presentation observation batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostPresentationObservationEntry {
    stream: HostPresentationStreamId,
    observation: HostPresentationStreamObservation,
}

impl HostPresentationObservationEntry {
    /// Creates one fact for the exact core-issued stream.
    #[must_use]
    pub const fn new(
        stream: HostPresentationStreamId,
        observation: HostPresentationStreamObservation,
    ) -> Self {
        Self {
            stream,
            observation,
        }
    }

    /// Returns the observed stream identity.
    #[must_use]
    pub const fn stream(self) -> HostPresentationStreamId {
        self.stream
    }

    /// Returns the provider fact for this stream.
    #[must_use]
    pub const fn observation(self) -> HostPresentationStreamObservation {
        self.observation
    }
}

/// Provider fact for one exact presentation stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostPresentationStreamObservation {
    /// This stream had no newly captured provider fact.
    NoUpdate,
    /// A new provider capture observed stream progress.
    Captured {
        /// Strictly increasing provider capture generation.
        generation: HostPresentationCaptureGeneration,
        /// Final-presentation progress known at that capture.
        progress: HostPresentationProgress,
    },
}

/// Final-presentation progress proven by one provider capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostPresentationProgress {
    /// The provider cannot authoritatively determine final presentation state.
    Unknown(AuthorityUnavailableReason),
    /// The host proved that every emission through the watermark reached a
    /// terminal presentation outcome.
    Retired {
        /// Last concrete emission included in this terminal observation.
        settled_through: HostFrameKey,
        /// The one emission known to have reached final presentation, if any.
        /// `Known(None)` proves none did; `Unknown` permits cleanup but never
        /// grants interaction authority.
        presented: Authority<Option<HostFrameKey>>,
    },
}

/// Typed non-fatal rejection of one stream observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostPresentationObservationRejection {
    /// The provider capture generation did not strictly advance.
    CaptureGenerationNotIncreasing {
        /// Previously accepted capture generation.
        previous: HostPresentationCaptureGeneration,
        /// Submitted capture generation.
        submitted: HostPresentationCaptureGeneration,
    },
    /// The terminal watermark belongs to another stream.
    SettlementFromDifferentStream {
        /// Stream expected by the batch entry.
        expected: HostPresentationStreamId,
        /// Stream embedded in the submitted watermark.
        submitted: HostPresentationStreamId,
    },
    /// The terminal watermark did not strictly advance beyond the last one.
    SettlementNotIncreasing {
        /// Previously settled watermark.
        previous: HostFrameKey,
        /// Submitted terminal watermark.
        submitted: HostFrameKey,
    },
    /// The terminal watermark was never emitted or was already settled.
    SettlementNotPending {
        /// Submitted terminal watermark.
        submitted: HostFrameKey,
    },
    /// The known-presented key belongs to another stream.
    PresentedFromDifferentStream {
        /// Stream expected by the batch entry.
        expected: HostPresentationStreamId,
        /// Stream embedded in the submitted key.
        submitted: HostPresentationStreamId,
    },
    /// The known-presented key is outside the newly retired watermark range.
    PresentedOutsideRetirementRange {
        /// Submitted known-presented key.
        presented: HostFrameKey,
        /// Submitted terminal watermark.
        settled_through: HostFrameKey,
    },
    /// The known-presented key was never emitted or was already settled.
    PresentedNotPending {
        /// Submitted known-presented key.
        presented: HostFrameKey,
    },
}

/// Result of one independently processed stream observation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostPresentationObservationOutcome {
    /// The batch explicitly carried no update for this pending stream.
    NoUpdate {
        /// Stream named by the entry.
        stream: HostPresentationStreamId,
    },
    /// A newer capture was recorded but could not prove a final result.
    CapturedUnknown {
        /// Stream named by the entry.
        stream: HostPresentationStreamId,
        /// Newly accepted provider capture generation.
        generation: HostPresentationCaptureGeneration,
        /// Why final presentation was unavailable.
        reason: AuthorityUnavailableReason,
    },
    /// A terminal range was retired for this stream.
    Retired {
        /// Stream named by the entry.
        stream: HostPresentationStreamId,
        /// Newly accepted provider capture generation.
        generation: HostPresentationCaptureGeneration,
        /// Terminal watermark accepted for this stream.
        settled_through: HostFrameKey,
        /// Final-presentation fact supplied by the provider.
        presented: Authority<Option<HostFrameKey>>,
        /// Number of concrete emissions removed from the pending ledger.
        retired_output_count: usize,
        /// Whether the known-presented output remained eligible for an engine
        /// authority promotion. A retiring or superseded stream can settle but
        /// can never re-authorize input.
        promotion_eligible: bool,
    },
    /// This stream fact was rejected without advancing that stream.
    Rejected {
        /// Stream named by the entry.
        stream: HostPresentationStreamId,
        /// Exact rejection reason.
        reason: HostPresentationObservationRejection,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresentationStreamLifecycle {
    Active,
    Retiring(PresentationStreamRetirementCause),
    Terminated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresentationStreamRetirementCause {
    EndpointSuperseded,
    SurfaceRemoved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresentationHostLifecycle {
    Live,
    Retired(PresentationHostRetirementTombstone),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PresentationHostState {
    lifecycle: PresentationHostLifecycle,
    streams: BTreeSet<HostPresentationStreamId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PresentationHostRetirementStatus {
    Live,
    Detailed(PresentationHostRetirementTombstone),
    Compacted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PresentationStreamState {
    host: PresentationHostLease,
    surface: SurfaceId,
    endpoint: HostPresentationEndpoint,
    lifecycle: PresentationStreamLifecycle,
    next_output_ordinal: u64,
    last_capture_generation: Option<HostPresentationCaptureGeneration>,
    settled_through: Option<HostFrameKey>,
    pending: BTreeMap<HostFrameKey, HostPresentationOutputPayload>,
}

/// Internal candidate that may grant scene interaction authority after the
/// ledger has accepted a valid final-presentation observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PresentationPromotionCandidate {
    stream: HostPresentationStreamId,
    key: HostFrameKey,
    surface: SurfaceId,
    endpoint: HostPresentationEndpoint,
    payload: HostPresentationOutputPayload,
}

impl PresentationPromotionCandidate {
    pub(crate) const fn stream(self) -> HostPresentationStreamId {
        self.stream
    }

    pub(crate) const fn key(self) -> HostFrameKey {
        self.key
    }

    pub(crate) const fn endpoint(self) -> HostPresentationEndpoint {
        self.endpoint
    }

    pub(crate) const fn payload(self) -> HostPresentationOutputPayload {
        self.payload
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PresentationObservationReduction {
    outcomes: Vec<HostPresentationObservationOutcome>,
    promotions: Vec<PresentationPromotionCandidate>,
}

impl PresentationObservationReduction {
    pub(crate) fn outcomes(&self) -> &[HostPresentationObservationOutcome] {
        &self.outcomes
    }

    pub(crate) fn promotions(&self) -> &[PresentationPromotionCandidate] {
        &self.promotions
    }
}

/// Internal exact-set result of retiring one live presentation host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PresentationHostRetirement {
    host: PresentationHostLease,
    tombstone: PresentationHostRetirementTombstone,
    streams: BTreeSet<HostPresentationStreamId>,
    retired_outputs: BTreeSet<HostFrameKey>,
    released_active_surfaces: BTreeSet<SurfaceId>,
}

impl PresentationHostRetirement {
    pub(crate) const fn host(&self) -> PresentationHostLease {
        self.host
    }

    pub(crate) const fn tombstone(&self) -> PresentationHostRetirementTombstone {
        self.tombstone
    }

    pub(crate) const fn streams(&self) -> &BTreeSet<HostPresentationStreamId> {
        &self.streams
    }

    pub(crate) const fn retired_outputs(&self) -> &BTreeSet<HostFrameKey> {
        &self.retired_outputs
    }

    pub(crate) const fn released_active_surfaces(&self) -> &BTreeSet<SurfaceId> {
        &self.released_active_surfaces
    }

    pub(crate) fn retired_output_count(&self) -> usize {
        self.retired_outputs.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum PresentationLedgerError {
    #[error("presentation host lease serial is exhausted")]
    HostLeaseExhausted,
    #[error("presentation stream serial is exhausted")]
    StreamIdExhausted,
    #[error("presentation output ordinal is exhausted for stream {stream:?}")]
    OutputOrdinalExhausted { stream: HostPresentationStreamId },
    #[error("presentation host lease is not registered in this engine domain")]
    UnknownHostLease,
    #[error("presentation host {host:?} was retired at reducer tick {retired_at:?} ({reason:?})")]
    HostRetired {
        host: PresentationHostLease,
        retired_at: ReducerTickId,
        reason: PresentationHostRetirementReason,
    },
    #[error(
        "presentation host {host:?} was retired and its detailed terminal record was compacted"
    )]
    HostRetiredCompacted { host: PresentationHostLease },
    #[error("live presentation host {host:?} cannot be compacted")]
    HostNotRetiredForCompaction { host: PresentationHostLease },
    #[error("presentation host {host:?} cannot compact non-terminal stream {stream:?}")]
    HostCompactionStreamNotTerminated {
        host: PresentationHostLease,
        stream: HostPresentationStreamId,
    },
    #[error("presentation host {host:?} cannot compact stream {stream:?} with pending outputs")]
    HostCompactionStreamHasPendingOutputs {
        host: PresentationHostLease,
        stream: HostPresentationStreamId,
    },
    #[error(
        "presentation host {host:?} cannot compact stream {stream:?} while it owns surface {surface}"
    )]
    HostCompactionStreamOwnsSurface {
        host: PresentationHostLease,
        stream: HostPresentationStreamId,
        surface: SurfaceId,
    },
    #[error("presentation stream {stream:?} is not registered in this engine domain")]
    UnknownStream { stream: HostPresentationStreamId },
    #[error("presentation stream {stream:?} belongs to host {actual:?}, not {expected:?}")]
    StreamHostMismatch {
        expected: PresentationHostLease,
        actual: PresentationHostLease,
        stream: HostPresentationStreamId,
    },
    #[error("presentation stream {stream:?} is not retiring")]
    StreamNotRetiring { stream: HostPresentationStreamId },
    #[error("presentation stream {stream:?} still has pending outputs")]
    StreamHasPendingOutputs { stream: HostPresentationStreamId },
    #[error(
        "presentation host {host:?} cannot quiesce stream {stream:?} because surface {surface} is not owned by a same-host successor"
    )]
    StreamQuiescenceRequiresSameHostSuccessor {
        host: PresentationHostLease,
        stream: HostPresentationStreamId,
        surface: SurfaceId,
    },
    #[error("presentation stream {stream:?} remains retained by core authority")]
    StreamQuiescenceStillRetained { stream: HostPresentationStreamId },
    #[error(
        "surface-removed presentation stream {stream:?} unexpectedly regained active ownership of surface {surface}"
    )]
    SurfaceRemovedStreamRegainedOwnership {
        stream: HostPresentationStreamId,
        surface: SurfaceId,
    },
    #[error(
        "active presentation owner {stream:?} is inconsistent with surface {surface} during roster retirement"
    )]
    ActiveSurfaceOwnerInvariant {
        stream: HostPresentationStreamId,
        surface: SurfaceId,
    },
    #[error(
        "superseded presentation host {host:?} cannot emit surface {surface} while stream {active_stream:?} is active"
    )]
    SupersededHostCannotEmit {
        host: PresentationHostLease,
        surface: SurfaceId,
        active_stream: HostPresentationStreamId,
    },
    #[error(
        "presentation host cannot reuse retired endpoint {endpoint:?} for surface {surface}; stream {stream:?} owns that retired endpoint"
    )]
    RetiredEndpointCannotEmit {
        surface: SurfaceId,
        endpoint: HostPresentationEndpoint,
        stream: HostPresentationStreamId,
    },
    #[error("presentation output ticket belongs to surface {ticket_surface}, not {surface}")]
    OutputSurfaceMismatch {
        surface: SurfaceId,
        ticket_surface: SurfaceId,
    },
    #[error(
        "native staging output {presentation:?} does not match surface {surface} and endpoint {endpoint:?}"
    )]
    NativeStagingEndpointMismatch {
        surface: SurfaceId,
        endpoint: HostPresentationEndpoint,
        presentation: NativeStagingPresentation,
    },
    #[error("presentation observation batch omitted pending streams: {missing:?}")]
    BatchMissingStreams {
        missing: Vec<HostPresentationStreamId>,
    },
    #[error("presentation observation batch contains streams outside the frozen scope: {extra:?}")]
    BatchExtraStreams {
        extra: Vec<HostPresentationStreamId>,
    },
    #[error("presentation observation batch repeats stream {stream:?}")]
    BatchDuplicateStream { stream: HostPresentationStreamId },
}

/// Core-owned presentation host, stream, and emission ledger.
///
/// The ledger intentionally has no adapter-specific timing logic. An adapter
/// reports only lossless capture facts; core owns stream ownership, pending
/// watermarks, and the rule that a retired stream can never regain interaction
/// authority after another stream owns the same surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PresentationLedger {
    authority_domain: EngineAuthorityDomainId,
    last_host_serial: PresentationHostSerial,
    last_stream_serial: PresentationStreamSerial,
    hosts: BTreeMap<PresentationHostLease, PresentationHostState>,
    streams: BTreeMap<HostPresentationStreamId, PresentationStreamState>,
    active_streams: BTreeMap<SurfaceId, HostPresentationStreamId>,
    compacted_retired_hosts: RetiredPresentationHostRanges,
}

/// Structural counters used by conformance and lifecycle leak tests.
///
/// This is deliberately a count-only view: it cannot reveal or manufacture
/// host, stream, or emission identities.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PresentationLedgerDiagnostics {
    live_hosts: usize,
    retired_hosts: usize,
    active_streams: usize,
    retiring_streams: usize,
    terminated_streams: usize,
    active_surface_owners: usize,
    pending_streams: usize,
    pending_outputs: usize,
    compacted_retired_hosts: u64,
    compacted_retirement_ranges: usize,
    retained_host_states: usize,
    retained_stream_states: usize,
}

impl PresentationLedgerDiagnostics {
    #[must_use]
    pub const fn live_hosts(self) -> usize {
        self.live_hosts
    }

    #[must_use]
    pub const fn retired_hosts(self) -> usize {
        self.retired_hosts
    }

    #[must_use]
    pub const fn active_streams(self) -> usize {
        self.active_streams
    }

    #[must_use]
    pub const fn retiring_streams(self) -> usize {
        self.retiring_streams
    }

    #[must_use]
    pub const fn terminated_streams(self) -> usize {
        self.terminated_streams
    }

    #[must_use]
    pub const fn active_surface_owners(self) -> usize {
        self.active_surface_owners
    }

    #[must_use]
    pub const fn pending_streams(self) -> usize {
        self.pending_streams
    }

    #[must_use]
    pub const fn pending_outputs(self) -> usize {
        self.pending_outputs
    }

    /// Returns logical retired host identities represented without detailed state.
    #[must_use]
    pub const fn compacted_retired_hosts(self) -> u64 {
        self.compacted_retired_hosts
    }

    /// Returns the actual number of disjoint interval records used by compacted hosts.
    #[must_use]
    pub const fn compacted_retirement_ranges(self) -> usize {
        self.compacted_retirement_ranges
    }

    /// Returns detailed live and blocked-retirement host records retained in memory.
    #[must_use]
    pub const fn retained_host_states(self) -> usize {
        self.retained_host_states
    }

    /// Returns detailed active, retiring, and blocked terminal stream records.
    #[must_use]
    pub const fn retained_stream_states(self) -> usize {
        self.retained_stream_states
    }
}

impl PresentationLedger {
    pub(crate) const fn new(authority_domain: EngineAuthorityDomainId) -> Self {
        Self {
            authority_domain,
            last_host_serial: PresentationHostSerial(0),
            last_stream_serial: PresentationStreamSerial(0),
            hosts: BTreeMap::new(),
            streams: BTreeMap::new(),
            active_streams: BTreeMap::new(),
            compacted_retired_hosts: RetiredPresentationHostRanges::new(),
        }
    }

    pub(crate) fn create_host(&mut self) -> Result<PresentationHostLease, PresentationLedgerError> {
        let serial = self
            .last_host_serial
            .checked_next()
            .ok_or(PresentationLedgerError::HostLeaseExhausted)?;
        let lease = PresentationHostLease {
            authority_domain: self.authority_domain,
            serial,
        };
        self.last_host_serial = serial;
        self.hosts.insert(
            lease,
            PresentationHostState {
                lifecycle: PresentationHostLifecycle::Live,
                streams: BTreeSet::new(),
            },
        );
        Ok(lease)
    }

    /// Returns the monotonic allocation frontier for presentation-host leases.
    ///
    /// Host creation deliberately does not advance the reducer tick, so
    /// rollback candidates freeze this independent frontier to prevent a later
    /// publish from discarding a concurrently minted lease and reusing its
    /// serial.
    pub(crate) const fn host_frontier(&self) -> u64 {
        self.last_host_serial.0
    }

    pub(crate) fn diagnostics(&self) -> PresentationLedgerDiagnostics {
        let mut diagnostics = PresentationLedgerDiagnostics {
            active_surface_owners: self.active_streams.len(),
            compacted_retired_hosts: self.compacted_retired_hosts.logical_host_count(),
            compacted_retirement_ranges: self.compacted_retired_hosts.interval_count(),
            retained_host_states: self.hosts.len(),
            retained_stream_states: self.streams.len(),
            ..PresentationLedgerDiagnostics::default()
        };
        for host in self.hosts.values() {
            match host.lifecycle {
                PresentationHostLifecycle::Live => diagnostics.live_hosts += 1,
                PresentationHostLifecycle::Retired(_) => diagnostics.retired_hosts += 1,
            }
        }
        for stream in self.streams.values() {
            match stream.lifecycle {
                PresentationStreamLifecycle::Active => diagnostics.active_streams += 1,
                PresentationStreamLifecycle::Retiring(_) => diagnostics.retiring_streams += 1,
                PresentationStreamLifecycle::Terminated => diagnostics.terminated_streams += 1,
            }
            if !stream.pending.is_empty() {
                diagnostics.pending_streams += 1;
                diagnostics.pending_outputs += stream.pending.len();
            }
        }
        diagnostics
    }

    pub(crate) fn retention_manifest(&self) -> PresentationHostRetentionManifest {
        let diagnostics = self.diagnostics();
        PresentationHostRetentionManifest::new(
            diagnostics.live_hosts(),
            diagnostics.retired_hosts(),
            diagnostics.retained_stream_states(),
            diagnostics.compacted_retirement_ranges(),
            diagnostics.compacted_retired_hosts(),
        )
    }

    /// Iterates every concrete emission which has not reached an authoritative terminal
    /// settlement watermark.
    pub(crate) fn pending_output_keys(&self) -> impl Iterator<Item = HostFrameKey> + '_ {
        self.streams
            .values()
            .flat_map(|stream| stream.pending.keys().copied())
    }

    /// Iterates the exact stream incarnations still represented by detailed core state.
    pub(crate) fn retained_stream_ids(
        &self,
    ) -> impl Iterator<Item = HostPresentationStreamId> + '_ {
        self.streams.keys().copied()
    }

    pub(crate) fn pending_stream_scope(
        &self,
        lease: PresentationHostLease,
    ) -> Result<BTreeSet<HostPresentationStreamId>, PresentationLedgerError> {
        let host = self.host(lease)?;
        Ok(host
            .streams
            .iter()
            .copied()
            .filter(|stream| {
                self.streams
                    .get(stream)
                    .is_some_and(|state| !state.pending.is_empty())
            })
            .collect())
    }

    pub(crate) fn active_surface_for_stream(
        &self,
        stream: HostPresentationStreamId,
    ) -> Option<SurfaceId> {
        let state = self.streams.get(&stream)?;
        (state.lifecycle == PresentationStreamLifecycle::Active
            && self.active_streams.get(&state.surface) == Some(&stream))
        .then_some(state.surface)
    }

    pub(crate) fn active_surface_scope(
        &self,
        surface: SurfaceId,
    ) -> Option<(
        HostPresentationStreamId,
        PresentationHostLease,
        HostPresentationEndpoint,
    )> {
        let stream = self.active_streams.get(&surface)?;
        let state = self.streams.get(stream)?;
        (state.lifecycle == PresentationStreamLifecycle::Active).then_some((
            *stream,
            state.host,
            state.endpoint,
        ))
    }

    /// Retires active streams whose semantic surfaces are absent from the tick-final roster.
    ///
    /// This must be called only after every workspace mutation in one reducer tick has settled.
    /// Temporary mid-tick vacancy is not a terminal presentation fact.
    pub(crate) fn retire_absent_surface_streams(
        &mut self,
        live_surfaces: &BTreeSet<SurfaceId>,
    ) -> Result<(), PresentationLedgerError> {
        let retiring = self
            .active_streams
            .iter()
            .filter(|(surface, _)| !live_surfaces.contains(surface))
            .map(|(surface, stream)| (*surface, *stream))
            .collect::<Vec<_>>();
        for (surface, stream) in &retiring {
            let state = self.streams.get(stream).ok_or(
                PresentationLedgerError::ActiveSurfaceOwnerInvariant {
                    stream: *stream,
                    surface: *surface,
                },
            )?;
            if state.surface != *surface || state.lifecycle != PresentationStreamLifecycle::Active {
                return Err(PresentationLedgerError::ActiveSurfaceOwnerInvariant {
                    stream: *stream,
                    surface: *surface,
                });
            }
        }
        for (surface, stream) in retiring {
            let removed = self.active_streams.remove(&surface);
            debug_assert_eq!(removed, Some(stream));
            let state = self
                .streams
                .get_mut(&stream)
                .expect("the complete validation pass retained the active stream");
            state.lifecycle = PresentationStreamLifecycle::Retiring(
                PresentationStreamRetirementCause::SurfaceRemoved,
            );
        }
        Ok(())
    }

    pub(crate) fn host_for_stream(
        &self,
        stream: HostPresentationStreamId,
    ) -> Option<PresentationHostLease> {
        self.streams.get(&stream).map(|state| state.host)
    }

    /// Prepares the one-shot acknowledgement required to reclaim a settled retiring stream.
    ///
    /// The caller must invoke this only after its renderer has drained every path that could
    /// submit another observation for `stream`. Core rechecks the ledger conditions when the
    /// acknowledgement is consumed, because the candidate may have changed in the meantime.
    pub(crate) fn prepare_stream_quiescence(
        &self,
        host: PresentationHostLease,
        stream: HostPresentationStreamId,
    ) -> Result<PresentationStreamQuiescence, PresentationLedgerError> {
        self.validate_stream_quiescence(host, stream)?;
        Ok(PresentationStreamQuiescence { host, stream })
    }

    /// Consumes an externally acknowledged quiescence proof and drops one retiring stream.
    ///
    /// This deliberately retains no endpoint tombstone. The acknowledgement means the renderer
    /// can no longer submit the retired stream, while stream serials remain monotonic and frozen
    /// host-frame scopes still reject any stale observation that names it.
    pub(crate) fn compact_quiesced_retiring_stream(
        &mut self,
        quiescence: PresentationStreamQuiescence,
        retained_streams: &BTreeSet<HostPresentationStreamId>,
    ) -> Result<(), PresentationLedgerError> {
        let PresentationStreamQuiescence { host, stream } = quiescence;
        self.validate_stream_quiescence(host, stream)?;
        if retained_streams.contains(&stream) {
            return Err(PresentationLedgerError::StreamQuiescenceStillRetained { stream });
        }

        let removed = self.streams.remove(&stream);
        debug_assert!(removed.is_some(), "validated stream must still be present");
        let host_state = self
            .hosts
            .get_mut(&host)
            .ok_or(PresentationLedgerError::UnknownHostLease)?;
        let removed_from_host = host_state.streams.remove(&stream);
        debug_assert!(
            removed_from_host,
            "validated stream must be owned by its host"
        );
        Ok(())
    }

    pub(crate) fn validate_lease(
        &self,
        lease: PresentationHostLease,
    ) -> Result<(), PresentationLedgerError> {
        self.host(lease).map(|_| ())
    }

    pub(crate) fn validate_observation_entry(
        &self,
        lease: PresentationHostLease,
        entry: HostPresentationObservationEntry,
    ) -> Result<(), PresentationLedgerError> {
        self.validate_lease(lease)?;
        let stream = entry.stream();
        let actual = self
            .streams
            .get(&stream)
            .map(|state| state.host)
            .ok_or(PresentationLedgerError::UnknownStream { stream })?;
        if actual != lease {
            return Err(PresentationLedgerError::StreamHostMismatch {
                expected: lease,
                actual,
                stream,
            });
        }
        Ok(())
    }

    pub(crate) fn retirement_status(
        &self,
        lease: PresentationHostLease,
    ) -> Result<PresentationHostRetirementStatus, PresentationLedgerError> {
        if lease.authority_domain() != self.authority_domain {
            return Err(PresentationLedgerError::UnknownHostLease);
        }
        if let Some(host) = self.hosts.get(&lease) {
            return Ok(match host.lifecycle {
                PresentationHostLifecycle::Live => PresentationHostRetirementStatus::Live,
                PresentationHostLifecycle::Retired(tombstone) => {
                    PresentationHostRetirementStatus::Detailed(tombstone)
                }
            });
        }
        if self.compacted_retired_hosts.contains(lease.serial) {
            return Ok(PresentationHostRetirementStatus::Compacted);
        }
        Err(PresentationLedgerError::UnknownHostLease)
    }

    pub(crate) fn detailed_retired_host_rosters(
        &self,
    ) -> impl Iterator<Item = (PresentationHostLease, &BTreeSet<HostPresentationStreamId>)> {
        self.hosts.iter().filter_map(|(host, state)| {
            matches!(state.lifecycle, PresentationHostLifecycle::Retired(_))
                .then_some((*host, &state.streams))
        })
    }

    pub(crate) fn compact_retired_host(
        &mut self,
        lease: PresentationHostLease,
    ) -> Result<bool, PresentationLedgerError> {
        match self.retirement_status(lease)? {
            PresentationHostRetirementStatus::Compacted => return Ok(false),
            PresentationHostRetirementStatus::Live => {
                return Err(PresentationLedgerError::HostNotRetiredForCompaction { host: lease });
            }
            PresentationHostRetirementStatus::Detailed(_) => {}
        }

        let streams = self
            .hosts
            .get(&lease)
            .ok_or(PresentationLedgerError::UnknownHostLease)?
            .streams
            .clone();
        for stream in &streams {
            let state = self
                .streams
                .get(stream)
                .ok_or(PresentationLedgerError::UnknownStream { stream: *stream })?;
            if state.host != lease {
                return Err(PresentationLedgerError::StreamHostMismatch {
                    expected: lease,
                    actual: state.host,
                    stream: *stream,
                });
            }
            if state.lifecycle != PresentationStreamLifecycle::Terminated {
                return Err(PresentationLedgerError::HostCompactionStreamNotTerminated {
                    host: lease,
                    stream: *stream,
                });
            }
            if !state.pending.is_empty() {
                return Err(
                    PresentationLedgerError::HostCompactionStreamHasPendingOutputs {
                        host: lease,
                        stream: *stream,
                    },
                );
            }
            if let Some((surface, _)) = self
                .active_streams
                .iter()
                .find(|(_, active)| **active == *stream)
            {
                return Err(PresentationLedgerError::HostCompactionStreamOwnsSurface {
                    host: lease,
                    stream: *stream,
                    surface: *surface,
                });
            }
        }

        for stream in streams {
            self.streams.remove(&stream);
        }
        self.hosts.remove(&lease);
        let inserted = self.compacted_retired_hosts.insert(lease.serial);
        debug_assert!(
            inserted,
            "a detailed retirement cannot already be compacted"
        );
        Ok(true)
    }

    pub(crate) fn retire_host(
        &mut self,
        lease: PresentationHostLease,
        retired_at: ReducerTickId,
        reason: PresentationHostRetirementReason,
    ) -> Result<PresentationHostRetirement, PresentationLedgerError> {
        self.validate_lease(lease)?;
        let streams = self
            .host_state(lease)?
            .streams
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let tombstone = PresentationHostRetirementTombstone { retired_at, reason };
        let mut retired_outputs = BTreeSet::new();
        let mut released_active_surfaces = BTreeSet::new();

        for stream in &streams {
            let state = self
                .streams
                .get_mut(stream)
                .ok_or(PresentationLedgerError::UnknownStream { stream: *stream })?;
            retired_outputs.extend(state.pending.keys().copied());
            state.pending.clear();
            state.lifecycle = PresentationStreamLifecycle::Terminated;
            if self.active_streams.get(&state.surface) == Some(stream) {
                self.active_streams.remove(&state.surface);
                released_active_surfaces.insert(state.surface);
            }
        }

        let host = self
            .hosts
            .get_mut(&lease)
            .ok_or(PresentationLedgerError::UnknownHostLease)?;
        host.lifecycle = PresentationHostLifecycle::Retired(tombstone);
        Ok(PresentationHostRetirement {
            host: lease,
            tombstone,
            streams,
            retired_outputs,
            released_active_surfaces,
        })
    }

    pub(crate) fn reduce_observation(
        &mut self,
        lease: PresentationHostLease,
        frozen_scope: &BTreeSet<HostPresentationStreamId>,
        observation: HostPresentationObservation,
    ) -> Result<PresentationObservationReduction, PresentationLedgerError> {
        self.validate_lease(lease)?;
        match observation {
            HostPresentationObservation::NoUpdate => Ok(PresentationObservationReduction {
                outcomes: Vec::new(),
                promotions: Vec::new(),
            }),
            HostPresentationObservation::Batch(mut entries) => {
                let mut submitted = BTreeSet::new();
                for entry in &entries {
                    if !submitted.insert(entry.stream()) {
                        return Err(PresentationLedgerError::BatchDuplicateStream {
                            stream: entry.stream(),
                        });
                    }
                }
                let missing = frozen_scope
                    .difference(&submitted)
                    .copied()
                    .collect::<Vec<_>>();
                if !missing.is_empty() {
                    return Err(PresentationLedgerError::BatchMissingStreams { missing });
                }
                let extra = submitted
                    .difference(frozen_scope)
                    .copied()
                    .collect::<Vec<_>>();
                if !extra.is_empty() {
                    return Err(PresentationLedgerError::BatchExtraStreams { extra });
                }
                entries.sort_unstable_by_key(|entry| entry.stream());
                let mut outcomes = Vec::with_capacity(entries.len());
                let mut promotions = Vec::new();
                for entry in entries {
                    self.validate_observation_entry(lease, entry)?;
                    let (outcome, promotion) = self.reduce_stream_observation(entry)?;
                    outcomes.push(outcome);
                    if let Some(promotion) = promotion {
                        promotions.push(promotion);
                    }
                }
                Ok(PresentationObservationReduction {
                    outcomes,
                    promotions,
                })
            }
        }
    }

    pub(crate) fn reduce_observation_entry(
        &mut self,
        lease: PresentationHostLease,
        entry: HostPresentationObservationEntry,
    ) -> Result<PresentationObservationReduction, PresentationLedgerError> {
        self.validate_observation_entry(lease, entry)?;
        let (outcome, promotion) = self.reduce_stream_observation(entry)?;
        Ok(PresentationObservationReduction {
            outcomes: vec![outcome],
            promotions: promotion.into_iter().collect(),
        })
    }

    pub(crate) fn emit(
        &mut self,
        lease: PresentationHostLease,
        surface: SurfaceId,
        endpoint: HostPresentationEndpoint,
        payload: HostPresentationOutputPayload,
    ) -> Result<HostPresentationOutput, PresentationLedgerError> {
        self.validate_lease(lease)?;
        if let HostPresentationOutputPayload::Paint { scene, .. } = payload
            && scene.surface() != surface
        {
            return Err(PresentationLedgerError::OutputSurfaceMismatch {
                surface,
                ticket_surface: scene.surface(),
            });
        }
        if let HostPresentationOutputPayload::NativeStaging { presentation } = payload
            && (presentation.binding().surface() != surface
                || endpoint != HostPresentationEndpoint::Native(presentation.binding()))
        {
            return Err(PresentationLedgerError::NativeStagingEndpointMismatch {
                surface,
                endpoint,
                presentation,
            });
        }
        let stream = self.active_stream_for(lease, surface, endpoint)?;
        let state = self
            .streams
            .get_mut(&stream)
            .ok_or(PresentationLedgerError::UnknownStream { stream })?;
        let ordinal = state
            .next_output_ordinal
            .checked_add(1)
            .ok_or(PresentationLedgerError::OutputOrdinalExhausted { stream })?;
        state.next_output_ordinal = ordinal;
        let key = HostFrameKey { stream, ordinal };
        let previous = state.pending.insert(key, payload);
        debug_assert!(previous.is_none());
        Ok(HostPresentationOutput {
            key,
            stream,
            surface,
            endpoint,
            payload,
        })
    }

    fn host_state(
        &self,
        lease: PresentationHostLease,
    ) -> Result<&PresentationHostState, PresentationLedgerError> {
        if lease.authority_domain() != self.authority_domain {
            return Err(PresentationLedgerError::UnknownHostLease);
        }
        match self.hosts.get(&lease) {
            Some(host) => Ok(host),
            None if self.compacted_retired_hosts.contains(lease.serial) => {
                Err(PresentationLedgerError::HostRetiredCompacted { host: lease })
            }
            None => Err(PresentationLedgerError::UnknownHostLease),
        }
    }

    fn host(
        &self,
        lease: PresentationHostLease,
    ) -> Result<&PresentationHostState, PresentationLedgerError> {
        let host = self.host_state(lease)?;
        match host.lifecycle {
            PresentationHostLifecycle::Live => Ok(host),
            PresentationHostLifecycle::Retired(tombstone) => {
                Err(PresentationLedgerError::HostRetired {
                    host: lease,
                    retired_at: tombstone.retired_at(),
                    reason: tombstone.reason(),
                })
            }
        }
    }

    fn validate_stream_quiescence(
        &self,
        host: PresentationHostLease,
        stream: HostPresentationStreamId,
    ) -> Result<(), PresentationLedgerError> {
        self.host(host)?;
        let state = self
            .streams
            .get(&stream)
            .ok_or(PresentationLedgerError::UnknownStream { stream })?;
        if state.host != host {
            return Err(PresentationLedgerError::StreamHostMismatch {
                expected: host,
                actual: state.host,
                stream,
            });
        }
        let retirement = match state.lifecycle {
            PresentationStreamLifecycle::Retiring(retirement) => retirement,
            PresentationStreamLifecycle::Active | PresentationStreamLifecycle::Terminated => {
                return Err(PresentationLedgerError::StreamNotRetiring { stream });
            }
        };
        if !state.pending.is_empty() {
            return Err(PresentationLedgerError::StreamHasPendingOutputs { stream });
        }
        if retirement == PresentationStreamRetirementCause::SurfaceRemoved {
            if self.active_streams.contains_key(&state.surface) {
                return Err(
                    PresentationLedgerError::SurfaceRemovedStreamRegainedOwnership {
                        stream,
                        surface: state.surface,
                    },
                );
            }
            return Ok(());
        }
        let has_same_host_successor = self
            .active_streams
            .get(&state.surface)
            .and_then(|successor| self.streams.get(successor))
            .is_some_and(|successor| {
                successor.host == host && successor.lifecycle == PresentationStreamLifecycle::Active
            });
        if !has_same_host_successor {
            return Err(
                PresentationLedgerError::StreamQuiescenceRequiresSameHostSuccessor {
                    host,
                    stream,
                    surface: state.surface,
                },
            );
        }
        Ok(())
    }

    fn active_stream_for(
        &mut self,
        lease: PresentationHostLease,
        surface: SurfaceId,
        endpoint: HostPresentationEndpoint,
    ) -> Result<HostPresentationStreamId, PresentationLedgerError> {
        let existing = self.active_streams.get(&surface).copied().filter(|stream| {
            self.streams.get(stream).is_some_and(|state| {
                state.host == lease
                    && state.endpoint == endpoint
                    && state.lifecycle == PresentationStreamLifecycle::Active
            })
        });
        if let Some(stream) = existing {
            return Ok(stream);
        }

        let active_stream = self.active_streams.get(&surface).copied();
        let active_host = active_stream
            .and_then(|stream| self.streams.get(&stream).map(|state| (stream, state.host)));
        let retired_stream_for_surface = self.host(lease)?.streams.iter().copied().find(|stream| {
            self.streams.get(stream).is_some_and(|state| {
                state.surface == surface
                    && matches!(state.lifecycle, PresentationStreamLifecycle::Retiring(_))
            })
        });
        if let Some((active_stream, active_host)) = active_host
            && active_host != lease
            && retired_stream_for_surface.is_some()
        {
            // A foreign host has taken ownership. The superseded host may
            // settle its existing streams but cannot create any successor,
            // regardless of endpoint, and steal the surface back.
            return Err(PresentationLedgerError::SupersededHostCannotEmit {
                host: lease,
                surface,
                active_stream,
            });
        }
        if let Some(stream) = self.host(lease)?.streams.iter().copied().find(|stream| {
            self.streams.get(stream).is_some_and(|state| {
                state.surface == surface
                    && state.endpoint == endpoint
                    && matches!(state.lifecycle, PresentationStreamLifecycle::Retiring(_))
            })
        }) {
            // The same host may advance to a new core-frozen endpoint, but it
            // cannot return to an exact retired endpoint and create ABA.
            return Err(PresentationLedgerError::RetiredEndpointCannotEmit {
                surface,
                endpoint,
                stream,
            });
        }

        let serial = self
            .last_stream_serial
            .checked_next()
            .ok_or(PresentationLedgerError::StreamIdExhausted)?;
        let stream = HostPresentationStreamId {
            authority_domain: self.authority_domain,
            serial,
        };
        self.last_stream_serial = serial;
        if let Some(previous) = self.active_streams.insert(surface, stream)
            && let Some(previous) = self.streams.get_mut(&previous)
        {
            previous.lifecycle = PresentationStreamLifecycle::Retiring(
                PresentationStreamRetirementCause::EndpointSuperseded,
            );
        }
        self.streams.insert(
            stream,
            PresentationStreamState {
                host: lease,
                surface,
                endpoint,
                lifecycle: PresentationStreamLifecycle::Active,
                next_output_ordinal: 0,
                last_capture_generation: None,
                settled_through: None,
                pending: BTreeMap::new(),
            },
        );
        let host = self
            .hosts
            .get_mut(&lease)
            .ok_or(PresentationLedgerError::UnknownHostLease)?;
        host.streams.insert(stream);
        Ok(stream)
    }

    fn reduce_stream_observation(
        &mut self,
        entry: HostPresentationObservationEntry,
    ) -> Result<
        (
            HostPresentationObservationOutcome,
            Option<PresentationPromotionCandidate>,
        ),
        PresentationLedgerError,
    > {
        let stream = entry.stream();
        let surface = self
            .streams
            .get(&stream)
            .map(|state| state.surface)
            .ok_or(PresentationLedgerError::UnknownStream { stream })?;
        let active_stream = self.active_streams.get(&surface).copied();
        let state = self
            .streams
            .get_mut(&stream)
            .ok_or(PresentationLedgerError::UnknownStream { stream })?;
        match entry.observation() {
            HostPresentationStreamObservation::NoUpdate => Ok((
                HostPresentationObservationOutcome::NoUpdate { stream },
                None,
            )),
            HostPresentationStreamObservation::Captured {
                generation,
                progress: HostPresentationProgress::Unknown(reason),
            } => {
                if let Some(previous) = state.last_capture_generation
                    && generation <= previous
                {
                    return Ok((
                        HostPresentationObservationOutcome::Rejected {
                            stream,
                            reason: HostPresentationObservationRejection::CaptureGenerationNotIncreasing {
                                previous,
                                submitted: generation,
                            },
                        },
                        None,
                    ));
                }
                state.last_capture_generation = Some(generation);
                Ok((
                    HostPresentationObservationOutcome::CapturedUnknown {
                        stream,
                        generation,
                        reason,
                    },
                    None,
                ))
            }
            HostPresentationStreamObservation::Captured {
                generation,
                progress:
                    HostPresentationProgress::Retired {
                        settled_through,
                        presented,
                    },
            } => {
                if let Some(previous) = state.last_capture_generation
                    && generation <= previous
                {
                    return Ok((
                        HostPresentationObservationOutcome::Rejected {
                            stream,
                            reason: HostPresentationObservationRejection::CaptureGenerationNotIncreasing {
                                previous,
                                submitted: generation,
                            },
                        },
                        None,
                    ));
                }
                if settled_through.stream() != stream {
                    return Ok((
                        HostPresentationObservationOutcome::Rejected {
                            stream,
                            reason: HostPresentationObservationRejection::SettlementFromDifferentStream {
                                expected: stream,
                                submitted: settled_through.stream(),
                            },
                        },
                        None,
                    ));
                }
                if let Some(previous) = state.settled_through
                    && settled_through <= previous
                {
                    return Ok((
                        HostPresentationObservationOutcome::Rejected {
                            stream,
                            reason: HostPresentationObservationRejection::SettlementNotIncreasing {
                                previous,
                                submitted: settled_through,
                            },
                        },
                        None,
                    ));
                }
                if !state.pending.contains_key(&settled_through) {
                    return Ok((
                        HostPresentationObservationOutcome::Rejected {
                            stream,
                            reason: HostPresentationObservationRejection::SettlementNotPending {
                                submitted: settled_through,
                            },
                        },
                        None,
                    ));
                }

                let presented_output = match presented {
                    Authority::Known(Some(key)) => {
                        if key.stream() != stream {
                            return Ok((
                                HostPresentationObservationOutcome::Rejected {
                                    stream,
                                    reason: HostPresentationObservationRejection::PresentedFromDifferentStream {
                                        expected: stream,
                                        submitted: key.stream(),
                                    },
                                },
                                None,
                            ));
                        }
                        if state
                            .settled_through
                            .is_some_and(|previous| key <= previous)
                            || key > settled_through
                        {
                            return Ok((
                                HostPresentationObservationOutcome::Rejected {
                                    stream,
                                    reason: HostPresentationObservationRejection::PresentedOutsideRetirementRange {
                                        presented: key,
                                        settled_through,
                                    },
                                },
                                None,
                            ));
                        }
                        let Some(payload) = state.pending.get(&key).copied() else {
                            return Ok((
                                HostPresentationObservationOutcome::Rejected {
                                    stream,
                                    reason:
                                        HostPresentationObservationRejection::PresentedNotPending {
                                            presented: key,
                                        },
                                },
                                None,
                            ));
                        };
                        Some((key, payload))
                    }
                    Authority::Known(None) | Authority::Unknown(_) => None,
                };

                let retired_output_count = state.pending.range(..=settled_through).count();
                state.pending.retain(|key, _| *key > settled_through);
                state.last_capture_generation = Some(generation);
                state.settled_through = Some(settled_through);
                let promotion = presented_output.and_then(|(key, payload)| {
                    let active = active_stream.is_some_and(|active| active == stream)
                        && state.lifecycle == PresentationStreamLifecycle::Active;
                    match (active, payload) {
                        (
                            true,
                            HostPresentationOutputPayload::Paint { .. }
                            | HostPresentationOutputPayload::NativeStaging { .. },
                        ) => Some(PresentationPromotionCandidate {
                            stream,
                            key,
                            surface: state.surface,
                            endpoint: state.endpoint,
                            payload,
                        }),
                        (false, _)
                        | (_, HostPresentationOutputPayload::Bootstrap)
                        | (_, HostPresentationOutputPayload::Unavailable) => None,
                    }
                });
                Ok((
                    HostPresentationObservationOutcome::Retired {
                        stream,
                        generation,
                        settled_through,
                        presented,
                        retired_output_count,
                        promotion_eligible: promotion.is_some(),
                    },
                    promotion,
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::WorkspaceEpoch;
    use crate::viewport::{ViewportBinding, WindowIncarnation, WindowToken};

    const SURFACE_A: SurfaceId = SurfaceId::new(1);
    const SURFACE_B: SurfaceId = SurfaceId::new(2);

    fn ledger() -> PresentationLedger {
        PresentationLedger::new(EngineAuthorityDomainId::new_for_test(71))
    }

    fn native_endpoint(window: u64) -> HostPresentationEndpoint {
        HostPresentationEndpoint::Native(ViewportBinding::new(
            EngineAuthorityDomainId::new_for_test(71),
            WorkspaceEpoch::new(1),
            SURFACE_A,
            WindowToken::new(window),
            WindowIncarnation::new(1),
        ))
    }

    fn settle_retiring_stream(
        ledger: &mut PresentationLedger,
        host: PresentationHostLease,
        retiring: HostPresentationOutput,
        active: HostPresentationOutput,
        generation: u64,
    ) {
        let scope = ledger
            .pending_stream_scope(host)
            .expect("complete pending scope");
        assert!(scope.contains(&retiring.stream()));
        assert!(scope.contains(&active.stream()));
        ledger
            .reduce_observation(
                host,
                &scope,
                HostPresentationObservation::Batch(vec![
                    HostPresentationObservationEntry::new(
                        retiring.stream(),
                        HostPresentationStreamObservation::Captured {
                            generation: HostPresentationCaptureGeneration::new(generation),
                            progress: HostPresentationProgress::Retired {
                                settled_through: retiring.key(),
                                presented: Authority::Known(None),
                            },
                        },
                    ),
                    HostPresentationObservationEntry::new(
                        active.stream(),
                        HostPresentationStreamObservation::NoUpdate,
                    ),
                ]),
            )
            .expect("retiring stream settles without promoting authority");
    }

    #[test]
    fn repeated_emissions_have_distinct_core_keys_in_one_stream() {
        let mut ledger = ledger();
        let host = ledger.create_host().expect("host lease");
        let first = ledger
            .emit(
                host,
                SURFACE_A,
                HostPresentationEndpoint::Headless,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("first emission");
        let second = ledger
            .emit(
                host,
                SURFACE_A,
                HostPresentationEndpoint::Headless,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("second emission");

        assert_eq!(first.stream(), second.stream());
        assert_ne!(first.key(), second.key());
        assert_eq!(
            ledger.pending_stream_scope(host).expect("stream scope"),
            BTreeSet::from([first.stream()])
        );
    }

    #[test]
    fn host_scope_isolated_from_other_hosts() {
        let mut ledger = ledger();
        let first_host = ledger.create_host().expect("first host lease");
        let second_host = ledger.create_host().expect("second host lease");
        let first = ledger
            .emit(
                first_host,
                SURFACE_A,
                HostPresentationEndpoint::Headless,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("first emission");
        let second = ledger
            .emit(
                second_host,
                SURFACE_B,
                HostPresentationEndpoint::Headless,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("second emission");

        assert_eq!(
            ledger
                .pending_stream_scope(first_host)
                .expect("first host scope"),
            BTreeSet::from([first.stream()])
        );
        assert_eq!(
            ledger
                .pending_stream_scope(second_host)
                .expect("second host scope"),
            BTreeSet::from([second.stream()])
        );
    }

    #[test]
    fn batch_requires_the_complete_frozen_pending_scope() {
        let mut ledger = ledger();
        let host = ledger.create_host().expect("host lease");
        let first = ledger
            .emit(
                host,
                SURFACE_A,
                HostPresentationEndpoint::Headless,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("first emission");
        let _second = ledger
            .emit(
                host,
                SURFACE_B,
                HostPresentationEndpoint::Headless,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("second emission");
        let scope = ledger.pending_stream_scope(host).expect("stream scope");

        let error = ledger
            .reduce_observation(
                host,
                &scope,
                HostPresentationObservation::Batch(vec![HostPresentationObservationEntry::new(
                    first.stream(),
                    HostPresentationStreamObservation::NoUpdate,
                )]),
            )
            .expect_err("partial batch must be rejected");

        assert!(matches!(
            error,
            PresentationLedgerError::BatchMissingStreams { .. }
        ));
    }

    #[test]
    fn superseded_stream_can_settle_without_regaining_promotion() {
        let mut ledger = ledger();
        let first_host = ledger.create_host().expect("first host lease");
        let second_host = ledger.create_host().expect("second host lease");
        let old = ledger
            .emit(
                first_host,
                SURFACE_A,
                HostPresentationEndpoint::Headless,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("old emission");
        let _new = ledger
            .emit(
                second_host,
                SURFACE_A,
                HostPresentationEndpoint::Headless,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("new emission");
        let scope = ledger
            .pending_stream_scope(first_host)
            .expect("old host scope");

        let reduction = ledger
            .reduce_observation(
                first_host,
                &scope,
                HostPresentationObservation::Batch(vec![HostPresentationObservationEntry::new(
                    old.stream(),
                    HostPresentationStreamObservation::Captured {
                        generation: HostPresentationCaptureGeneration::new(1),
                        progress: HostPresentationProgress::Retired {
                            settled_through: old.key(),
                            presented: Authority::Known(Some(old.key())),
                        },
                    },
                )]),
            )
            .expect("old stream may settle");

        assert!(reduction.promotions().is_empty());
        assert!(matches!(
            reduction.outcomes(),
            [HostPresentationObservationOutcome::Retired {
                promotion_eligible: false,
                ..
            }]
        ));
        assert!(
            ledger
                .pending_stream_scope(first_host)
                .expect("old host scope")
                .is_empty()
        );
    }

    #[test]
    fn superseded_host_cannot_emit_a_replacement_stream_for_the_same_surface() {
        let mut ledger = ledger();
        let first_host = ledger.create_host().expect("first host lease");
        let second_host = ledger.create_host().expect("second host lease");
        let _old = ledger
            .emit(
                first_host,
                SURFACE_A,
                HostPresentationEndpoint::Headless,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("old emission");
        let current = ledger
            .emit(
                second_host,
                SURFACE_A,
                HostPresentationEndpoint::Headless,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("current emission");

        let error = ledger
            .emit(
                first_host,
                SURFACE_A,
                HostPresentationEndpoint::Headless,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect_err("superseded host must not reclaim the surface");
        assert!(matches!(
            error,
            PresentationLedgerError::SupersededHostCannotEmit {
                host,
                surface: SURFACE_A,
                active_stream,
            } if host == first_host && active_stream == current.stream()
        ));

        let current_again = ledger
            .emit(
                second_host,
                SURFACE_A,
                HostPresentationEndpoint::Headless,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("current host remains active");
        assert_eq!(current_again.stream(), current.stream());
    }

    #[test]
    fn same_host_may_advance_endpoint_but_cannot_reuse_a_retired_endpoint() {
        let mut ledger = ledger();
        let host = ledger.create_host().expect("host lease");
        let first_endpoint = HostPresentationEndpoint::Native(ViewportBinding::new(
            EngineAuthorityDomainId::new_for_test(71),
            WorkspaceEpoch::new(1),
            SURFACE_A,
            WindowToken::new(10),
            WindowIncarnation::new(1),
        ));
        let second_endpoint = HostPresentationEndpoint::Native(ViewportBinding::new(
            EngineAuthorityDomainId::new_for_test(71),
            WorkspaceEpoch::new(1),
            SURFACE_A,
            WindowToken::new(11),
            WindowIncarnation::new(1),
        ));
        let first = ledger
            .emit(
                host,
                SURFACE_A,
                first_endpoint,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("first endpoint emission");
        let successor = ledger
            .emit(
                host,
                SURFACE_A,
                second_endpoint,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("same host may advance to a new endpoint");
        let successor_again = ledger
            .emit(
                host,
                SURFACE_A,
                second_endpoint,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("active successor remains usable");
        assert_eq!(successor_again.stream(), successor.stream());

        let error = ledger
            .emit(
                host,
                SURFACE_A,
                first_endpoint,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect_err("retired endpoint must not regain authority");
        assert!(matches!(
            error,
            PresentationLedgerError::RetiredEndpointCannotEmit {
                surface: SURFACE_A,
                endpoint,
                stream,
            } if endpoint == first_endpoint && stream == first.stream()
        ));
    }

    #[test]
    fn tick_final_surface_removal_retires_and_reclaims_without_a_successor() {
        let mut ledger = ledger();
        let host = ledger.create_host().expect("host lease");
        let removed = ledger
            .emit(
                host,
                SURFACE_A,
                native_endpoint(10),
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("removed surface endpoint emits");
        let retained = ledger
            .emit(
                host,
                SURFACE_B,
                HostPresentationEndpoint::Headless,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("retained surface endpoint emits");

        ledger
            .retire_absent_surface_streams(&BTreeSet::from([SURFACE_B]))
            .expect("the tick-final roster must retire only absent surfaces");
        assert!(!ledger.active_streams.contains_key(&SURFACE_A));
        assert_eq!(
            ledger.active_streams.get(&SURFACE_B),
            Some(&retained.stream())
        );
        assert!(ledger.streams.get(&removed.stream()).is_some_and(|state| {
            state.lifecycle
                == PresentationStreamLifecycle::Retiring(
                    PresentationStreamRetirementCause::SurfaceRemoved,
                )
        }));
        assert!(matches!(
            ledger.prepare_stream_quiescence(host, removed.stream()),
            Err(PresentationLedgerError::StreamHasPendingOutputs { stream })
                if stream == removed.stream()
        ));

        let scope = ledger.pending_stream_scope(host).expect("pending scope");
        ledger
            .reduce_observation(
                host,
                &scope,
                HostPresentationObservation::Batch(vec![
                    HostPresentationObservationEntry::new(
                        removed.stream(),
                        HostPresentationStreamObservation::Captured {
                            generation: HostPresentationCaptureGeneration::new(1),
                            progress: HostPresentationProgress::Retired {
                                settled_through: removed.key(),
                                presented: Authority::Known(None),
                            },
                        },
                    ),
                    HostPresentationObservationEntry::new(
                        retained.stream(),
                        HostPresentationStreamObservation::NoUpdate,
                    ),
                ]),
            )
            .expect("removed surface output must settle without a successor");
        let proof = ledger
            .prepare_stream_quiescence(host, removed.stream())
            .expect("settled surface-removed stream must accept exact renderer quiescence");
        ledger
            .compact_quiesced_retiring_stream(proof, &BTreeSet::new())
            .expect("surface-removed stream must compact without a fabricated successor");
        assert!(!ledger.streams.contains_key(&removed.stream()));
        assert!(ledger.streams.contains_key(&retained.stream()));
    }

    #[test]
    fn quiescence_reclaims_only_a_settled_same_host_retiring_stream_and_late_results_fail_closed() {
        let mut ledger = ledger();
        let host = ledger.create_host().expect("host lease");
        let retiring = ledger
            .emit(
                host,
                SURFACE_A,
                native_endpoint(10),
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("first endpoint emits");
        let active = ledger
            .emit(
                host,
                SURFACE_A,
                native_endpoint(11),
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("successor endpoint emits");

        assert!(matches!(
            ledger.prepare_stream_quiescence(host, retiring.stream()),
            Err(PresentationLedgerError::StreamHasPendingOutputs { stream }) if stream == retiring.stream()
        ));

        settle_retiring_stream(&mut ledger, host, retiring, active, 1);
        let retained_before = crate::retention::PresentationRetentionManifest::from_resources(
            ledger.pending_output_keys(),
            ledger.retained_stream_ids(),
        );
        assert!(retained_before.retains_stream(retiring.stream()));
        let proof = ledger
            .prepare_stream_quiescence(host, retiring.stream())
            .expect("settled retiring stream may be externally quiesced");
        assert!(matches!(
            ledger.compact_quiesced_retiring_stream(proof, &BTreeSet::from([retiring.stream()])),
            Err(PresentationLedgerError::StreamQuiescenceStillRetained { stream }) if stream == retiring.stream()
        ));

        let proof = ledger
            .prepare_stream_quiescence(host, retiring.stream())
            .expect("failed compaction consumes only its proof");
        ledger
            .compact_quiesced_retiring_stream(proof, &BTreeSet::new())
            .expect("quiesced stream is reclaimed after every core reference disappears");
        assert!(!ledger.streams.contains_key(&retiring.stream()));
        assert!(
            ledger
                .hosts
                .get(&host)
                .is_some_and(|state| !state.streams.contains(&retiring.stream()))
        );
        assert_eq!(
            ledger.retained_stream_ids().collect::<Vec<_>>(),
            vec![active.stream()]
        );
        let retained_after = crate::retention::PresentationRetentionManifest::from_resources(
            ledger.pending_output_keys(),
            ledger.retained_stream_ids(),
        );
        assert!(!retained_after.retains_stream(retiring.stream()));

        let error = ledger
            .reduce_observation(
                host,
                &BTreeSet::new(),
                HostPresentationObservation::Batch(vec![HostPresentationObservationEntry::new(
                    retiring.stream(),
                    HostPresentationStreamObservation::NoUpdate,
                )]),
            )
            .expect_err("late results for a quiesced stream remain outside the frozen scope");
        assert!(matches!(
            error,
            PresentationLedgerError::BatchExtraStreams { extra } if extra == vec![retiring.stream()]
        ));
    }

    #[test]
    fn ten_thousand_same_host_native_endpoint_rotations_reclaim_quiesced_streams() {
        let mut ledger = ledger();
        let host = ledger.create_host().expect("host lease");
        let mut retiring = ledger
            .emit(
                host,
                SURFACE_A,
                native_endpoint(10),
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("initial endpoint emits");

        for generation in 1..=10_000 {
            let endpoint = if generation % 2 == 0 {
                native_endpoint(10)
            } else {
                native_endpoint(11)
            };
            let active = ledger
                .emit(
                    host,
                    SURFACE_A,
                    endpoint,
                    HostPresentationOutputPayload::Bootstrap,
                )
                .expect("quiesced endpoints may rotate without retaining old stream state");
            settle_retiring_stream(&mut ledger, host, retiring, active, generation);
            let proof = ledger
                .prepare_stream_quiescence(host, retiring.stream())
                .expect("settled old endpoint is externally quiescent");
            ledger
                .compact_quiesced_retiring_stream(proof, &BTreeSet::new())
                .expect("quiesced old endpoint stream is compacted");
            retiring = active;
        }

        let diagnostics = ledger.diagnostics();
        assert_eq!(diagnostics.live_hosts(), 1);
        assert_eq!(diagnostics.retained_host_states(), 1);
        assert_eq!(diagnostics.active_streams(), 1);
        assert_eq!(diagnostics.retiring_streams(), 0);
        assert_eq!(diagnostics.retained_stream_states(), 1);
        assert_eq!(diagnostics.pending_streams(), 1);
        assert_eq!(
            ledger.hosts[&host].streams,
            BTreeSet::from([retiring.stream()])
        );
    }

    #[test]
    fn host_retirement_terminates_its_complete_roster_without_releasing_a_successor() {
        let mut ledger = ledger();
        let retired_host = ledger.create_host().expect("retiring host lease");
        let current_host = ledger.create_host().expect("current host lease");
        let first_endpoint = HostPresentationEndpoint::Headless;
        let second_endpoint = HostPresentationEndpoint::Native(ViewportBinding::new(
            EngineAuthorityDomainId::new_for_test(71),
            WorkspaceEpoch::new(1),
            SURFACE_A,
            WindowToken::new(10),
            WindowIncarnation::new(1),
        ));
        let first = ledger
            .emit(
                retired_host,
                SURFACE_A,
                first_endpoint,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("first retiring-host stream emits");
        let second = ledger
            .emit(
                retired_host,
                SURFACE_A,
                second_endpoint,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("second endpoint creates another stream");
        let sibling = ledger
            .emit(
                retired_host,
                SURFACE_B,
                HostPresentationEndpoint::Headless,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("sibling surface stream emits");
        let successor = ledger
            .emit(
                current_host,
                SURFACE_A,
                HostPresentationEndpoint::Headless,
                HostPresentationOutputPayload::Bootstrap,
            )
            .expect("successor host takes surface ownership");

        let retirement = ledger
            .retire_host(
                retired_host,
                ReducerTickId::new(9),
                PresentationHostRetirementReason::RuntimeDestroyed,
            )
            .expect("host retirement succeeds");
        assert_eq!(
            retirement.streams(),
            &BTreeSet::from([first.stream(), second.stream(), sibling.stream()])
        );
        assert_eq!(
            retirement.retired_outputs(),
            &BTreeSet::from([first.key(), second.key(), sibling.key()])
        );
        assert_eq!(retirement.retired_output_count(), 3);
        assert_eq!(
            retirement.released_active_surfaces(),
            &BTreeSet::from([SURFACE_B])
        );
        assert_eq!(
            ledger.active_streams.get(&SURFACE_A),
            Some(&successor.stream())
        );
        assert!(!ledger.active_streams.contains_key(&SURFACE_B));
        assert!(retirement.streams().iter().all(|stream| {
            ledger.streams.get(stream).is_some_and(|state| {
                state.lifecycle == PresentationStreamLifecycle::Terminated
                    && state.pending.is_empty()
            })
        }));
        let diagnostics = ledger.diagnostics();
        assert_eq!(diagnostics.live_hosts(), 1);
        assert_eq!(diagnostics.retired_hosts(), 1);
        assert_eq!(diagnostics.active_streams(), 1);
        assert_eq!(diagnostics.retiring_streams(), 0);
        assert_eq!(diagnostics.terminated_streams(), 3);
        assert_eq!(diagnostics.active_surface_owners(), 1);
        assert_eq!(diagnostics.pending_outputs(), 1);

        assert!(matches!(
            ledger.validate_lease(retired_host),
            Err(PresentationLedgerError::HostRetired {
                host,
                retired_at,
                reason: PresentationHostRetirementReason::RuntimeDestroyed,
            }) if host == retired_host && retired_at == ReducerTickId::new(9)
        ));
        assert!(matches!(
            ledger.emit(
                retired_host,
                SURFACE_B,
                HostPresentationEndpoint::Headless,
                HostPresentationOutputPayload::Bootstrap,
            ),
            Err(PresentationLedgerError::HostRetired { host, .. }) if host == retired_host
        ));
    }

    #[test]
    fn ten_thousand_retired_hosts_compact_to_one_interval_without_reusing_identity() {
        let mut ledger = ledger();

        for value in 1..=10_000 {
            let host = ledger
                .create_host()
                .expect("host identity remains available");
            ledger
                .emit(
                    host,
                    SURFACE_A,
                    HostPresentationEndpoint::Headless,
                    HostPresentationOutputPayload::Bootstrap,
                )
                .expect("one stream emits before retirement");
            let retirement = ledger
                .retire_host(
                    host,
                    ReducerTickId::new(value),
                    PresentationHostRetirementReason::ExplicitShutdown,
                )
                .expect("host retirement remains available");
            assert_eq!(retirement.streams().len(), 1);
            assert!(
                ledger
                    .compact_retired_host(host)
                    .expect("terminal stream roster is compactable")
            );
            assert!(matches!(
                ledger.validate_lease(host),
                Err(PresentationLedgerError::HostRetiredCompacted { host: retired })
                    if retired == host
            ));
        }

        let diagnostics = ledger.diagnostics();
        assert_eq!(ledger.host_frontier(), 10_000);
        assert_eq!(diagnostics.retained_host_states(), 0);
        assert_eq!(diagnostics.retained_stream_states(), 0);
        assert_eq!(diagnostics.compacted_retired_hosts(), 10_000);
        assert_eq!(diagnostics.compacted_retirement_ranges(), 1);
        assert_eq!(diagnostics.pending_outputs(), 0);

        let successor = ledger
            .create_host()
            .expect("allocation continues beyond compacted identities");
        assert_eq!(successor.serial.0, 10_001);
    }

    #[test]
    fn one_live_host_is_an_exact_hole_in_compacted_retirement_ranges() {
        let mut ledger = ledger();
        let live = ledger.create_host().expect("live host identity");

        for value in 2..=10_000 {
            let host = ledger.create_host().expect("retiring host identity");
            ledger
                .retire_host(
                    host,
                    ReducerTickId::new(value),
                    PresentationHostRetirementReason::ExplicitShutdown,
                )
                .expect("host retirement");
            assert!(ledger.compact_retired_host(host).expect("compaction"));
        }

        let with_hole = ledger.diagnostics();
        assert_eq!(with_hole.live_hosts(), 1);
        assert_eq!(with_hole.retained_host_states(), 1);
        assert_eq!(with_hole.compacted_retired_hosts(), 9_999);
        assert_eq!(with_hole.compacted_retirement_ranges(), 1);
        ledger
            .validate_lease(live)
            .expect("the serial hole remains a live authority");

        ledger
            .retire_host(
                live,
                ReducerTickId::new(10_001),
                PresentationHostRetirementReason::ExplicitShutdown,
            )
            .expect("live hole retires");
        assert!(ledger.compact_retired_host(live).expect("hole compacts"));
        let merged = ledger.diagnostics();
        assert_eq!(merged.retained_host_states(), 0);
        assert_eq!(merged.compacted_retired_hosts(), 10_000);
        assert_eq!(merged.compacted_retirement_ranges(), 1);
    }
}
